use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::sync::mpsc::{Receiver, Sender};

use tauri::{Emitter, Manager};
use tracing::{debug, error, info, warn};

use crate::app_state::AppState;
use crate::cache::atomic_write;
use crate::catalogue;
use crate::db;
use crate::events;
use crate::inventory_state::{build_inventory_from_blob, load_inventory_state_cache};
use crate::memory_scanner;
use crate::monitor::{
    self, BlobStatusPayload, InventoryUpdate,
};
use crate::BlobBuildParams;

/// Shared state needed by the blob capture thread, extracted from AppState
/// to keep the clone list at the spawn site manageable.
pub(crate) struct BlobCaptureDeps {
    pub app: tauri::AppHandle,
    pub flag: Arc<AtomicBool>,
    pub db_path: std::path::PathBuf,
    pub inventory_state_cache_path: std::path::PathBuf,
    pub shared_quantities: Arc<Mutex<HashMap<String, i64>>>,
    pub shared_unique: Arc<Mutex<HashMap<String, i64>>>,
    pub shared_mods: Arc<Mutex<HashMap<String, memory_scanner::ModCount>>>,
    pub shared_crafting: Arc<Mutex<Vec<monitor::CraftingJob>>>,
    pub blob_log_enabled: Arc<AtomicBool>,
    pub blob_log_dir: std::path::PathBuf,
    pub debug_cat_enabled: Arc<AtomicBool>,
    pub unmatched_paths_dir: std::path::PathBuf,
    pub force_pid_check: Arc<AtomicBool>,
    pub blob_rx: Receiver<memory_scanner::BlobInventory>,
    pub blob_tx: Sender<memory_scanner::BlobInventory>,
}

/// Spawn the blob-capture thread.
///
/// This thread is responsible for:
/// - Detecting whether Warframe is running (PID check every 5s)
/// - Capturing memory blobs every 10s when the game is active
/// - Processing incoming blobs via the channel and updating shared state
/// - Emitting inventory-update events to the UI
pub(crate) fn spawn_blob_capture_thread(
    deps: BlobCaptureDeps,
    catalog: monitor::MonitorCatalog,
) {
    let BlobCaptureDeps {
        app, flag, db_path, inventory_state_cache_path,
        shared_quantities, shared_unique, shared_mods, shared_crafting,
        blob_log_enabled, blob_log_dir, debug_cat_enabled,
        unmatched_paths_dir, force_pid_check, blob_rx, blob_tx,
    } = deps;

    let monitor::MonitorCatalog {
        path_to_name, path_to_ducat, path_to_vaulted,
        path_to_tradable, path_to_masterable, path_to_max_level_cap, path_to_category,
        path_to_item_type, path_to_product_category, path_to_wfcd_cat,
        alias_excluded, ignored_paths, stackable_paths, path_aliases,
        unique_names, display_names, relic_drops_snapshot,
    } = catalog;

    std::thread::spawn(move || {
        let conn = match rusqlite::Connection::open(&db_path) {
            Ok(c) => c,
            Err(e) => { error!(error = %e, "monitor DB open failed"); return; }
        };
        let _ = conn.execute_batch("PRAGMA journal_mode=WAL;");

        let section_baseline_path = inventory_state_cache_path.with_file_name("section_baseline.json");
        let mut section_baseline = memory_scanner::SectionBaseline::from_keys(
            std::fs::read(&section_baseline_path).ok()
                .and_then(|b| serde_json::from_slice::<Vec<String>>(&b).ok())
                .unwrap_or_default(),
        );
        let mut last_applied_hash: Option<u64> = None;

        let startup = monitor::init_monitor_startup_state(
            &shared_quantities, &shared_mods, &inventory_state_cache_path,
        );
        let mut known = startup.known;
        let mut prev_mods = startup.prev_mods;
        let mut unique_stable = startup.unique_stable;
        let mut confirmed_unique = startup.confirmed_unique;
        let mut known_mods = startup.known_mods;
        let mut last_snapshot_date = String::new();

        // Emit an immediate status before the first scan so the UI shows cached
        // inventory data without waiting for the scan to finish.
        {
            let game_found = memory_scanner::find_warframe_pid_pub().is_some();
            let now_pre = chrono::Utc::now().timestamp();
            let mut initial_qty = known.clone();
            for k in unique_stable.keys() { initial_qty.entry(k.clone()).or_insert(1); }
            for (path, mc) in &known_mods { initial_qty.entry(path.clone()).or_insert(mc.total); }
            let _ = app.emit(events::INVENTORY_UPDATE, InventoryUpdate {
                quantities: initial_qty,
                crafting: vec![],
                mastery_rank: startup.current_mastery_rank,
                mastery_data: startup.current_mastery_data.clone(),
                changes: vec![],
                consumed_suits: startup.current_consumed_suits.clone(),
                mods: known_mods.clone(),
                socketed_shards: startup.current_socketed_shards.clone(),
                forma_counts: startup.current_forma_counts.clone(),
                warframe_running: game_found,
                scanned_at: now_pre,
                is_full_pass: true,
                player_name: app.state::<AppState>().local_player_name
                    .lock().ok().and_then(|g| g.clone()),
            });
        }

        let mut current_mastery_rank = startup.current_mastery_rank;
        let mut current_mastery_data = startup.current_mastery_data;
        let mut current_recipes: Vec<memory_scanner::PendingRecipe> = Vec::new();
        let mut current_consumed_suits = startup.current_consumed_suits;
        let mut current_socketed_shards = startup.current_socketed_shards;
        let mut current_forma_counts = startup.current_forma_counts;
        let mut last_blob_time: Option<std::time::Instant> = None;
        // Guard against overlapping captures: a full memory walk can take >10 s on large
        // game processes, so without this flag we'd stack up concurrent scan threads.
        let blob_scan_active = Arc::new(AtomicBool::new(false));
        // Cache the game-running state so we only re-enumerate processes once every 5 s
        // instead of on every 2-second loop tick (CreateToolhelp32Snapshot is not free).
        let mut last_pid_check: Option<std::time::Instant> = None;
        let mut last_pid: Option<u32> = None;
        let mut cached_game_running = false;
        // Hysteresis against flapping: mark "running" on a single hit, only
        // mark "not running" after 2 consecutive misses (2x 5 s). A poke_scan
        // (forced) bypasses the hysteresis for an immediate result.
        let mut consecutive_misses: u32 = 0;
        // When game is not running, suppress redundant inventory-update emits.
        // Only emit on the status-change tick and then at most once every 30 s as a heartbeat.
        let mut prev_game_running = false;
        let mut last_not_running_emit: Option<std::time::Instant> = None;

        while flag.load(Ordering::SeqCst) {
            // If shared_quantities was cleared externally (clear_cache command), wipe local
            // state so the next blob logs everything as fresh.
            {
                let sq = shared_quantities.lock().unwrap_or_else(|e| e.into_inner());
                let local_has_data = !known.is_empty() || !unique_stable.is_empty() || !known_mods.is_empty();
                if sq.is_empty() && local_has_data {
                    known.clear();
                    unique_stable.clear();
                    confirmed_unique.clear();
                    known_mods.clear();
                    last_applied_hash = None;
                    section_baseline = memory_scanner::SectionBaseline::default();
                }
            }

            let now = chrono::Utc::now().timestamp();

            // Process any incoming blob (non-blocking)
            while let Ok(blob) = blob_rx.try_recv() {
                match section_baseline.evaluate(&blob.sections) {
                    Err(missing) => {
                        warn!(?missing, "blob rejected: sections present in last good blob are missing - truncated capture");
                        continue;
                    }
                    Ok(true) => {
                        if let Ok(json) = serde_json::to_string(&section_baseline.keys()) {
                            let _ = atomic_write(&section_baseline_path, json.as_bytes());
                        }
                    }
                    Ok(false) => {}
                }

                if blob.content_hash != 0 && Some(blob.content_hash) == last_applied_hash {
                    debug!("blob unchanged since last apply - skipping");
                    continue;
                }

                if process_blob(
                    &blob, &app, &conn, &inventory_state_cache_path,
                    &path_to_name, &path_to_category, &path_to_ducat, &path_to_vaulted,
                    &path_to_tradable, &path_to_masterable, &path_to_max_level_cap, &relic_drops_snapshot,
                    &alias_excluded, &stackable_paths, &path_aliases,
                    &path_to_item_type, &path_to_product_category, &path_to_wfcd_cat,
                    &ignored_paths, &unmatched_paths_dir, &debug_cat_enabled,
                    &shared_quantities, &shared_mods, &shared_unique, &shared_crafting,
                    &mut known, &mut prev_mods, &mut unique_stable, &mut confirmed_unique,
                    &mut known_mods, &mut current_mastery_rank, &mut current_mastery_data,
                    &mut current_recipes, &mut current_consumed_suits,
                    &mut current_socketed_shards, &mut current_forma_counts,
                    &mut last_snapshot_date, now,
                ) {
                    last_applied_hash = Some(blob.content_hash);
                }
            }

            // Re-enumerate processes at most every 5 s (CreateToolhelp32Snapshot overhead).
            // force_pid_check bypasses the cooldown (set by the poke_scan command).
            let forced = force_pid_check.swap(false, Ordering::SeqCst);
            let needs_pid_check = forced || last_pid_check
                .is_none_or(|t: std::time::Instant| t.elapsed().as_secs() >= 5);
            if needs_pid_check {
                let current_pid = memory_scanner::find_warframe_pid_pub();
                if current_pid.is_some() {
                    consecutive_misses = 0;
                    cached_game_running = true;
                } else if forced {
                    consecutive_misses = 0;
                    cached_game_running = false;
                } else {
                    consecutive_misses += 1;
                    if consecutive_misses >= 2 {
                        cached_game_running = false;
                    }
                }
                if current_pid != last_pid {
                    if current_pid.is_some() {
                        info!(?last_pid, ?current_pid, "Warframe PID changed, clearing blob region cache");
                        memory_scanner::reset_last_blob_region();
                    }
                    last_pid = current_pid;
                }
                last_pid_check = Some(std::time::Instant::now());
            }
            let game_running = cached_game_running;
            // Detection is decoupled from blob success: as soon as the PID exists
            // the UI knows the game is running — even while the memory scan is
            // still in flight or failing. Without this the chip stayed on
            // "No Game" until a blob parse succeeded, which explains the
            // sporadic recognition.
            if game_running && !prev_game_running {
                let mut emit_qty = known.clone();
                for k in &confirmed_unique { emit_qty.entry(k.clone()).or_insert(1); }
                for (p, mc) in &known_mods { emit_qty.entry(p.clone()).or_insert(mc.total); }
                let crafting = monitor::build_crafting_jobs(
                    &current_recipes.iter()
                        .map(|r| (r.unique_name.clone(), r.completion_ms))
                        .collect::<Vec<_>>(),
                    &display_names, &unique_names,
                );
                let _ = app.emit(events::INVENTORY_UPDATE, InventoryUpdate {
                    quantities: emit_qty, crafting,
                    mastery_rank: current_mastery_rank,
                    mastery_data: HashMap::new(),
                    changes: vec![], warframe_running: true, scanned_at: now,
                    consumed_suits: current_consumed_suits.clone(),
                    mods: known_mods.clone(),
                    socketed_shards: current_socketed_shards.clone(),
                    forma_counts: current_forma_counts.clone(),
                    is_full_pass: false,
                    player_name: app.state::<AppState>().local_player_name
                        .lock().ok().and_then(|g| g.clone()),
                });
            }
            if game_running {
                // ── Blob capture: unconditional scan every 10 seconds ─────────
                let should_capture = last_blob_time
                    .is_none_or(|t: std::time::Instant| t.elapsed() >= std::time::Duration::from_secs(10));
                let already_running = blob_scan_active.load(Ordering::SeqCst);

                if should_capture && !already_running {
                    blob_scan_active.store(true, Ordering::SeqCst);
                    last_blob_time = Some(std::time::Instant::now());
                    // UTC, suffixed "Z" so filenames aren't mistaken for local wall-clock time.
                    let ts     = chrono::Utc::now().format("%Y-%m-%dT%H-%M-%SZ").to_string();
                    let dir    = blob_log_dir.clone();
                    let save   = blob_log_enabled.load(Ordering::SeqCst);
                    let active = blob_scan_active.clone();
                    let tx     = blob_tx.clone();
                    let _ = app.emit(events::BLOB_STATUS, BlobStatusPayload {
                        stage:  "scanning".into(),
                        detail: "Reading Warframe memory\u{2026}".into(),
                    });
                    debug!(save, "blob capture starting");
                    std::thread::spawn(move || {
                        struct ClearOnDrop(Arc<AtomicBool>);
                        impl Drop for ClearOnDrop {
                            fn drop(&mut self) { self.0.store(false, Ordering::SeqCst); }
                        }
                        let _guard = ClearOnDrop(active);
                        let count = memory_scanner::capture_all_blobs(&dir, &ts, tx, save);
                        debug!(files_saved = count, save_flag = save, ts = %ts, "blob capture finished");
                    });
                }
                prev_game_running = true;
            } else {
                // Game not running — throttle emits: only on status-change and every 30 s heartbeat.
                // Without this guard the loop emits every 2 s with identical data, triggering a
                // full React render cascade (17 k-item useMemo rebuild) 30 times per minute.
                let status_changed = prev_game_running;
                let heartbeat_due  = last_not_running_emit
                    .is_none_or(|t: std::time::Instant| t.elapsed() >= std::time::Duration::from_secs(30));
                if status_changed || heartbeat_due {
                    let mut emit_qty = known.clone();
                    for k in &confirmed_unique { emit_qty.entry(k.clone()).or_insert(1); }
                    for (p, mc) in &known_mods { emit_qty.entry(p.clone()).or_insert(mc.total); }
                    let crafting = monitor::build_crafting_jobs(
                        &current_recipes.iter()
                            .map(|r| (r.unique_name.clone(), r.completion_ms))
                            .collect::<Vec<_>>(),
                        &display_names, &unique_names,
                    );
                    // Skip mastery_data on heartbeats — it hasn't changed and spreading 17k
                    // entries into React state on every tick is expensive.
                    let send_mastery = status_changed;
                    let _ = app.emit(events::INVENTORY_UPDATE, InventoryUpdate {
                        quantities: emit_qty, crafting,
                        mastery_rank: current_mastery_rank,
                        mastery_data: if send_mastery { current_mastery_data.clone() } else { HashMap::new() },
                        changes: vec![], warframe_running: false, scanned_at: now,
                        consumed_suits: current_consumed_suits.clone(),
                        mods: known_mods.clone(),
                        socketed_shards: current_socketed_shards.clone(),
                        forma_counts: current_forma_counts.clone(),
                        is_full_pass: false,
                        player_name: app.state::<AppState>().local_player_name
                            .lock().ok().and_then(|g| g.clone()),
                    });
                    last_not_running_emit = Some(std::time::Instant::now());
                }
                prev_game_running = false;
            }

            std::thread::sleep(std::time::Duration::from_secs(2));
        }
    });
}

#[allow(clippy::too_many_arguments)]
fn process_blob(
    blob: &memory_scanner::BlobInventory,
    app: &tauri::AppHandle,
    conn: &rusqlite::Connection,
    inventory_state_cache_path: &std::path::Path,
    path_to_name: &HashMap<String, String>,
    path_to_category: &HashMap<String, String>,
    path_to_ducat: &HashMap<String, u32>,
    path_to_vaulted: &HashMap<String, bool>,
    path_to_tradable: &HashMap<String, bool>,
    path_to_masterable: &HashMap<String, bool>,
    path_to_max_level_cap: &HashMap<String, u32>,
    relic_drops_snapshot: &HashMap<String, Vec<String>>,
    alias_excluded: &std::collections::HashSet<String>,
    stackable_paths: &std::collections::HashSet<String>,
    path_aliases: &HashMap<String, String>,
    path_to_item_type: &HashMap<String, String>,
    path_to_product_category: &HashMap<String, String>,
    path_to_wfcd_cat: &HashMap<String, String>,
    ignored_paths: &std::collections::HashSet<String>,
    unmatched_paths_dir: &std::path::Path,
    debug_cat_enabled: &AtomicBool,
    shared_quantities: &Arc<Mutex<HashMap<String, i64>>>,
    shared_mods: &Arc<Mutex<HashMap<String, memory_scanner::ModCount>>>,
    shared_unique: &Arc<Mutex<HashMap<String, i64>>>,
    shared_crafting: &Arc<Mutex<Vec<monitor::CraftingJob>>>,
    known: &mut HashMap<String, i64>,
    prev_mods: &mut HashMap<String, memory_scanner::ModCount>,
    unique_stable: &mut HashMap<String, u8>,
    confirmed_unique: &mut std::collections::HashSet<String>,
    known_mods: &mut HashMap<String, memory_scanner::ModCount>,
    current_mastery_rank: &mut Option<u32>,
    current_mastery_data: &mut HashMap<String, u32>,
    current_recipes: &mut Vec<memory_scanner::PendingRecipe>,
    current_consumed_suits: &mut Vec<String>,
    current_socketed_shards: &mut HashMap<String, Vec<memory_scanner::ArchonShard>>,
    current_forma_counts: &mut HashMap<String, u32>,
    last_snapshot_date: &mut String,
    now: i64,
) -> bool {
    let existing_wfm: HashMap<String, u32> =
        load_inventory_state_cache(&inventory_state_cache_path.to_path_buf())
            .items.into_iter()
            .filter_map(|(k, v)| v.wfm_price.map(|p| (k, p)))
            .collect();
    let sc = build_inventory_from_blob(BlobBuildParams {
        blob,
        path_to_name, path_to_category,
        path_to_ducat, path_to_vaulted,
        path_to_tradable, path_to_masterable, path_to_max_level_cap,
        relic_drops: relic_drops_snapshot, existing_wfm_prices: &existing_wfm,
        excluded_paths: alias_excluded, stackable_paths,
    });
    if let Ok(json) = serde_json::to_string(&sc) {
        let _ = atomic_write(inventory_state_cache_path, json.as_bytes());
    }

    // Snapshot previous full inventory (known + uniques + mods) for change detection.
    let prev_all: HashMap<String, i64> = {
        let mut m = known.clone();
        for k in confirmed_unique.iter() { m.entry(k.clone()).or_insert(1); }
        for (p, mc) in known_mods.iter() { m.entry(p.clone()).or_insert(mc.total); }
        m
    };

    // Completeness guard: parse_full_account_blob already rejects blobs missing
    // required sections (MiscItems, RegularCredits, etc.) — see memory_scanner.rs.
    // Keep this secondary guard for the unique-items case as a belt-and-suspenders
    // defence against incomplete blobs that slipped through parsing.
    let prev_unique_count = confirmed_unique.len();
    if blob.unique_items.is_empty() && prev_unique_count > 0 {
        warn!("blob rejected at commit: 0 unique items vs {} previously — incomplete blob", prev_unique_count);
        return false;
    }

    // Blob is authoritative — full replacement, not a merge.
    {
        let mut inv_state = monitor::InventoryState {
            known,
            unique_stable,
            confirmed_unique,
            known_mods,
            current_socketed_shards,
            current_forma_counts,
            current_mastery_rank,
            current_mastery_data,
            current_consumed_suits,
            current_recipes,
        };
        monitor::apply_blob_to_state(
            blob, &mut inv_state, path_aliases, stackable_paths,
        );
    }

    // Debug: write paths with no WFCD entry or Misc fallback to the Unmatched Paths folder.
    if debug_cat_enabled.load(Ordering::Relaxed) {
        catalogue::write_debug_unmatched_paths(
            blob,
            catalogue::UnmatchedPathMaps {
                path_to_name, path_to_item_type,
                path_to_product_category, path_to_wfcd_cat,
                path_to_category,
            },
            ignored_paths, unmatched_paths_dir,
        );
    }

    // Sync shared state
    if let Ok(mut q)  = shared_quantities.lock() { *q = known.clone(); }
    if let Ok(mut sm) = shared_mods.lock()       { *sm = known_mods.clone(); }
    if let Ok(mut uq) = shared_unique.lock() {
        uq.clear();
        for name in confirmed_unique.iter() { uq.insert(name.clone(), 1); }
    }

    // Emit inventory update
    let mut emit_qty = known.clone();
    for k in confirmed_unique.iter() { emit_qty.entry(k.clone()).or_insert(1); }
    for (p, mc) in known_mods.iter() { emit_qty.entry(p.clone()).or_insert(mc.total); }

    // Detect and record every quantity change (up, down, new, gone-to-0).
    let mut changes = db::detect_quantity_changes(
        conn, &prev_all, &emit_qty, ignored_paths, path_to_name,
    );

    // Rank-specific change detection for mods/arcanes.
    {
        let rank_changes = db::detect_mod_rank_changes(
            conn, prev_mods, known_mods, ignored_paths, path_to_name,
        );
        changes.extend(rank_changes);
    }
    // Update prev_mods for next iteration
    *prev_mods = known_mods.clone();

    let crafting = monitor::build_crafting_jobs(
        &blob.pending_recipes.iter()
            .map(|r| (r.item_type.clone(), r.completion_ms))
            .collect::<Vec<_>>(),
        &[], // display_names not needed here — already in path_to_name
        &[], // unique_names not needed here
    );
    *shared_crafting.lock().unwrap_or_else(|e| e.into_inner()) = crafting.clone();
    let _ = app.emit(events::INVENTORY_UPDATE, InventoryUpdate {
        quantities: emit_qty,
        crafting,
        mastery_rank: *current_mastery_rank,
        mastery_data: current_mastery_data.clone(),
        changes,
        warframe_running: true,
        scanned_at:   now,
        consumed_suits:   current_consumed_suits.clone(),
        mods:             known_mods.clone(),
        socketed_shards:  current_socketed_shards.clone(),
        forma_counts:     current_forma_counts.clone(),
        is_full_pass:     true,
        player_name: app.state::<AppState>().local_player_name
            .lock().ok().and_then(|g| g.clone()),
    });

    let detail = format!(
        "{} unique · {} resources · {} mods · {} flavour",
        blob.unique_items.len(), blob.stackable_items.len(),
        blob.mods.len(), blob.flavour_items.len()
    );
    info!(detail = %detail, "blob applied");
    let _ = app.emit(events::BLOB_STATUS, BlobStatusPayload {
        stage: "done".into(),
        detail,
    });

    // Daily snapshots
    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
    if *last_snapshot_date != today {
        *last_snapshot_date = today.clone();
        if let Ok(tracked) = db::get_tracked_items(conn) {
            for item in &tracked {
                let qty = *known.get(&item.unique_name).unwrap_or(&0);
                let _ = db::record_snapshot(conn, &item.unique_name, &today, qty);
            }
        }
    }
    true
}
