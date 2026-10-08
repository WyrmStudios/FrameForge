// ─── Imports ──────────────────────────────────────────────────────────────────

use std::collections::HashMap;
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use tauri::{Emitter, Manager, State};
use tracing::info;

use app_state::{load_corrections, AppState};
use catalogue::load_items_cache;
use image_cache::serve_image_files;
use inventory_state::{is_unique_path, load_inventory_state_cache, load_recipes_cache};
use pricing::{fetch_relics_run_data, load_relics_run_cache};
use settings::{merge_settings, read_settings_map, restore_window_state, save_window_state};
use wfcd::SyndicateOffer;
use wfm::Wfm;

// ─── Modules ──────────────────────────────────────────────────────────────────

mod app_state;
mod blob_capture;
mod cache;
mod catalogue;
mod companion_api;
mod console_login; // [console-login feature] remove this line to drop the feature
mod credentials;
mod db;
mod diagnostics;
mod events;
mod image_cache;
mod inventory_state;
mod log_watcher;
mod logging;
mod mem_regions;
mod memory_scanner;
mod monitor;
mod ocr;
mod ocr_fallback;
mod paths;
mod platform;
mod pricing;
mod refresh;
mod relic_pick;
mod resolver;
mod reward_pipeline;
mod reward_watcher;
mod rivens;
mod settings;
mod stats;
mod syndicates;
mod trade_log;
mod updater;
mod wfcd;
mod wfm;
mod wfm_commands;
mod wfm_queue;
mod wfm_top;
mod worldstate;

// ─── Structs ──────────────────────────────────────────────────────────────────

pub struct BlobBuildParams<'a> {
    blob: &'a memory_scanner::BlobInventory,
    path_to_name: &'a HashMap<String, String>,
    path_to_category: &'a HashMap<String, String>,
    path_to_ducat: &'a HashMap<String, u32>,
    path_to_vaulted: &'a HashMap<String, bool>,
    path_to_tradable: &'a HashMap<String, bool>,
    path_to_masterable: &'a HashMap<String, bool>,
    relic_drops: &'a HashMap<String, Vec<String>>,
    existing_wfm_prices: &'a HashMap<String, u32>,
    excluded_paths: &'a std::collections::HashSet<String>,
    stackable_paths: &'a std::collections::HashSet<String>,
}

/// Pre-loaded initial state derived from on-disk caches.
/// Built once at startup before the Tauri builder, then moved into `AppState`.
struct InitialState {
    items: Vec<wfcd::WfcdItem>,
    weapon_dispositions: HashMap<String, f32>,
    recipes: HashMap<String, Vec<wfcd::RecipeComponent>>,
    relic_drops: HashMap<String, Vec<String>>,
    relic_rewards: HashMap<String, Vec<wfcd::RelicReward>>,
    quantities: HashMap<String, i64>,
    unique: HashMap<String, i64>,
    mods: HashMap<String, memory_scanner::ModCount>,
    syndicate_catalog: HashMap<String, Vec<SyndicateOffer>>,
    auction_ids: Vec<String>,
    relics_run_prices: HashMap<String, u32>,
    wfm_prices: HashMap<String, Option<u32>>,
    corrections: HashMap<String, app_state::CorrectionEntry>,
}

// ─── Helper functions ─────────────────────────────────────────────────────────

pub(crate) fn truncate_chars(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

fn sanitize_chat_item_name(s: &str) -> String {
    let rank = s
        .chars()
        .filter(|&c| ('\u{E000}'..='\u{F8FF}').contains(&c))
        .count();
    let clean = s
        .chars()
        .filter(|&c| !('\u{E000}'..='\u{F8FF}').contains(&c) && !c.is_control())
        .collect::<String>()
        .trim()
        .to_string();
    if rank > 0 {
        format!("{clean} (R{rank})")
    } else {
        clean
    }
}

pub(crate) fn append_to_file(path: &std::path::Path, text: &str) -> std::io::Result<()> {
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    f.write_all(text.as_bytes())
}

// ─── State initialisation ─────────────────────────────────────────────────────

/// Cache file locations handed to `load_initial_state`.
struct InitialCachePaths<'a> {
    items_cache_path: &'a std::path::PathBuf,
    recipes_cache_path: &'a std::path::PathBuf,
    relic_drops_cache_path: &'a std::path::PathBuf,
    relic_rewards_cache_path: &'a std::path::PathBuf,
    inventory_state_cache_path: &'a std::path::PathBuf,
    syndicate_catalog_path: &'a std::path::PathBuf,
    auction_ids_path: &'a std::path::PathBuf,
    relics_run_prices_cache_path: &'a std::path::PathBuf,
    corrections_path: &'a std::path::PathBuf,
}

fn load_initial_state(paths: InitialCachePaths<'_>) -> InitialState {
    let InitialCachePaths {
        items_cache_path,
        recipes_cache_path,
        relic_drops_cache_path,
        relic_rewards_cache_path,
        inventory_state_cache_path,
        syndicate_catalog_path,
        auction_ids_path,
        relics_run_prices_cache_path,
        corrections_path,
    } = paths;
    let items = load_items_cache(items_cache_path).unwrap_or_else(wfcd::fallback_items);
    let weapon_dispositions: HashMap<String, f32> = items
        .iter()
        .filter_map(|i| i.omega_attenuation.map(|d| (i.unique_name.clone(), d)))
        .collect();
    let recipes = load_recipes_cache(recipes_cache_path);
    let relic_drops: HashMap<String, Vec<String>> = std::fs::read_to_string(relic_drops_cache_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();

    // Relic rewards: invalidate on old format or age > 24 h.
    let relic_rewards: HashMap<String, Vec<wfcd::RelicReward>> = {
        let cache_age_ok = std::fs::metadata(relic_rewards_cache_path)
            .and_then(|m| m.modified())
            .map(|t| {
                t.elapsed().unwrap_or(std::time::Duration::MAX)
                    < std::time::Duration::from_secs(86_400)
            })
            .unwrap_or(false);
        let loaded: Option<HashMap<String, Vec<wfcd::RelicReward>>> = if cache_age_ok {
            std::fs::read_to_string(relic_rewards_cache_path)
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
        } else {
            None
        };
        match loaded {
            Some(map) if map.keys().any(|k| k.starts_with("/Lotus/")) => map,
            Some(_) => {
                info!("relic_rewards cache is old format (no path keys) — discarding, will regenerate");
                let _ = std::fs::remove_file(relic_rewards_cache_path);
                HashMap::new()
            }
            None => {
                let _ = std::fs::remove_file(relic_rewards_cache_path);
                HashMap::new()
            }
        }
    };

    // Inventory state → split into quantities, uniques, mods.
    let inv_state = load_inventory_state_cache(inventory_state_cache_path);

    let quantities: HashMap<String, i64> = inv_state
        .items
        .iter()
        .filter(|(k, v)| {
            if v.is_flavour {
                return true;
            }
            v.mod_ranks.is_none()
                && (!is_unique_path(k) || matches!(v.category.as_str(), "Blueprints" | "Parts"))
                && v.amount > 0
        })
        .map(|(k, v)| (k.clone(), if v.is_flavour { 1 } else { v.amount }))
        .collect();

    let unique: HashMap<String, i64> = inv_state
        .items
        .iter()
        .filter(|(k, v)| {
            v.mod_ranks.is_none()
                && is_unique_path(k)
                && v.amount > 0
                && !matches!(v.category.as_str(), "Blueprints" | "Parts")
        })
        .map(|(k, _)| (k.clone(), 1i64))
        .collect();

    let mods: HashMap<String, memory_scanner::ModCount> = inv_state
        .items
        .iter()
        .filter(|(_, v)| v.mod_ranks.is_some())
        .map(|(k, v)| {
            let mc = memory_scanner::ModCount {
                total: v.amount,
                by_rank: v
                    .mod_ranks
                    .as_ref()
                    .unwrap()
                    .iter()
                    .filter_map(|(r, &c)| r.parse::<u8>().ok().map(|rank| (rank, c)))
                    .collect(),
            };
            (k.clone(), mc)
        })
        .collect();

    let syndicate_catalog: HashMap<String, Vec<SyndicateOffer>> =
        std::fs::read_to_string(syndicate_catalog_path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();

    let auction_ids: Vec<String> = std::fs::read_to_string(auction_ids_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();

    let relics_run = load_relics_run_cache(relics_run_prices_cache_path);
    let relics_run_prices = relics_run
        .as_ref()
        .map(|(by_name, _)| by_name.clone())
        .unwrap_or_default();
    let wfm_prices: HashMap<String, Option<u32>> = relics_run
        .map(|(_, by_slug)| by_slug.into_iter().map(|(k, v)| (k, Some(v))).collect())
        .unwrap_or_default();

    let corrections = load_corrections(corrections_path);

    InitialState {
        items,
        weapon_dispositions,
        recipes,
        relic_drops,
        relic_rewards,
        quantities,
        unique,
        mods,
        syndicate_catalog,
        auction_ids,
        relics_run_prices,
        wfm_prices,
        corrections,
    }
}

// ─── App setup ────────────────────────────────────────────────────────────────

fn setup_app(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    use tauri::Manager;

    logging::init();

    match paths::root() {
        Some(root) => info!("app root: {} (dev)", root.display()),
        None => info!("app root: user directories (production)"),
    }

    // Local HTTP server for cached item images.
    {
        let img_cache_dir = app.state::<AppState>().img_cache_dir.clone();
        let std_listener = std::net::TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
        let port = std_listener.local_addr().map_err(|e| e.to_string())?.port();
        *app.state::<AppState>().img_server_port.lock().unwrap() = port;
        tauri::async_runtime::spawn(async move {
            std_listener.set_nonblocking(true).ok();
            if let Ok(tokio_listener) = tokio::net::TcpListener::from_std(std_listener) {
                serve_image_files(tokio_listener, img_cache_dir).await;
            }
        });
    }

    // Main window: set icon, restore geometry, show.
    if let Some(window) = app.get_webview_window("main") {
        let icon = tauri::image::Image::from_bytes(include_bytes!("../icons/icon.png"))
            .map_err(|e| e.to_string())?;
        window.set_icon(icon).map_err(|e| e.to_string())?;
        if cfg!(debug_assertions) {
            let _ = window.set_title("FrameForge Dev");
        }
        let state = app.state::<AppState>();
        restore_window_state(
            app.handle(),
            &window,
            &state.settings_path,
            "window",
            400,
            300,
        );
        let _ = window.show();
    }

    // Overlay: show once for WebView2 init, then park off-screen.
    // NEVER hide relic-overlay — breaks DirectComposition on transparent WebView2.
    if let Some(win) = app.get_webview_window("relic-overlay") {
        let _ = win.show();
        let _ = win.set_position(tauri::Position::Physical(tauri::PhysicalPosition {
            x: 0,
            y: -3000,
        }));
    }

    // Single EE.log tailer — started once, unconditionally, and runs for the app's
    // lifetime. Handles riven-screen, relic-pick, trade-completion and WFM-whisper
    // detection regardless of the memory scanner toggle (plain file I/O, not memory
    // reading); the relic-reward OCR trigger inside it self-gates on `monitor_active`.
    {
        let state = app.state::<AppState>();
        let relic_rewards_map = state
            .relic_rewards
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        reward_watcher::spawn_reward_watcher_thread(reward_watcher::RewardWatcherDeps {
            app: app.handle().clone(),
            flag: state.monitor_active.clone(),
            relic_rewards: relic_rewards_map,
            auto_capture_dir: state.auto_capture_dir.clone(),
        });
    }

    // Background: load relics.run prices.
    {
        let app_handle = app.handle().clone();
        tauri::async_runtime::spawn(async move {
            let state = app_handle.state::<AppState>();
            let (by_name, by_slug) = match load_relics_run_cache(
                &state.relics_run_prices_cache_path,
            ) {
                Some(cached) => cached,
                None => {
                    let data = tauri::async_runtime::spawn_blocking(fetch_relics_run_data)
                        .await
                        .unwrap_or_default();
                    if !data.0.is_empty() {
                        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
                        let j = serde_json::json!({ "date": today, "by_name": &data.0, "by_slug": &data.1 });
                        if let Ok(s) = serde_json::to_string(&j) {
                            let _ = std::fs::write(&state.relics_run_prices_cache_path, s);
                        }
                    }
                    data
                }
            };
            if by_name.is_empty() {
                return;
            }
            *state
                .relics_run_prices
                .lock()
                .unwrap_or_else(|e| e.into_inner()) = by_name;
            for (slug, price) in by_slug {
                if !state.wfm.is_price_cached(&slug) {
                    state.wfm.cache_price(slug, Some(price));
                }
            }
            let _ = app_handle.emit(events::BULK_PRICES_UPDATED, ());
        });
    }

    // Background refresh loop (worldstate, bulk prices, catalogue, etc.)
    refresh::spawn(app.handle().clone());

    Ok(())
}

// ─── Live monitor ─────────────────────────────────────────────────────────────

#[tauri::command]
async fn start_monitor(app: tauri::AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    if state.monitor_active.swap(true, Ordering::SeqCst) {
        return Ok(());
    }

    let catalog = {
        let items = state
            .wfcd_items
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let relic_drops = state
            .relic_drops
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        monitor::build_monitor_catalog(&items, &state.corrections, &relic_drops)
    };

    let flag = state.monitor_active.clone();

    let (blob_tx, blob_rx) = std::sync::mpsc::channel::<memory_scanner::BlobInventory>();

    blob_capture::spawn_blob_capture_thread(
        blob_capture::BlobCaptureDeps {
            app: app.clone(),
            flag: flag.clone(),
            db_path: state.db_path.clone(),
            inventory_state_cache_path: state.inventory_state_cache_path.clone(),
            shared_quantities: state.current_quantities.clone(),
            shared_unique: state.unique_quantities.clone(),
            shared_mods: state.current_mods.clone(),
            shared_crafting: state.current_crafting.clone(),
            blob_log_enabled: state.blob_log_enabled.clone(),
            blob_log_dir: state.blob_log_dir.clone(),
            debug_cat_enabled: state.debug_cat_enabled.clone(),
            unmatched_paths_dir: state.unmatched_paths_dir.clone(),
            force_pid_check: state.force_pid_check.clone(),
            blob_rx,
            blob_tx,
        },
        catalog,
    );

    // The EE.log tailer itself runs unconditionally from app startup (see `setup_app`) —
    // only the relic-reward OCR trigger and this scan-only legacy worker are tied to
    // the memory scanner's on/off state.
    let debug_path = std::env::temp_dir().join("frameforge_reward_debug.txt");
    monitor::start_legacy_reward_worker(flag, debug_path);

    Ok(())
}

// ─── App entry point ──────────────────────────────────────────────────────────

pub fn run() {
    // Before any path is read: decides which root the whole run lives in.
    paths::init_root();
    paths::migrate_legacy();

    // Directory layout:
    //   cache_dir  (%LOCALAPPDATA%\frameforge\) — caches, logs, debug dumps, img_cache
    //   data_dir   (%APPDATA%\frameforge\)      — data.db, auction_ids.json
    //   config_dir (%APPDATA%\frameforge\)      — settings.json, corrections.json
    let config_dir = paths::config_dir();
    let data_dir = paths::data_dir();
    let cache_dir = paths::cache_dir();

    ocr_fallback::set_data_dir(cache_dir.clone());

    // ── Path definitions ───────────────────────────────────────────────────
    let db_path = data_dir.join("data.db");
    let items_cache_path = cache_dir.join("items_cache.json");
    let recipes_cache_path = cache_dir.join("recipes_cache.json");
    let relic_drops_cache_path = cache_dir.join("relic_drops_cache.json");
    let relic_rewards_cache_path = cache_dir.join("relic_rewards_cache.json");
    let quantities_cache_path = cache_dir.join("quantities_cache.json");
    let inventory_state_cache_path = cache_dir.join("inventory_state_cache.json");
    let settings_path = config_dir.join("settings.json");
    let log_path = cache_dir.join("scan_log.txt");
    let changes_log_path = cache_dir.join("inventory_changes.txt");

    let debug_root = cache_dir.join("Debugging");
    let blob_log_dir = debug_root.join("Inventory Snapshots");
    let api_log_dir = debug_root.join("Api Responses");
    let auto_capture_dir = debug_root.join("Auto-Capture");
    let manual_capture_dir = debug_root.join("Manual Capture");
    let memory_probe_dir = debug_root.join("Memory Probe");
    let raw_scan_dir = debug_root.join("Raw Memory Record");
    let unmatched_paths_dir = debug_root.join("Unmatched Paths");
    let raw_scan_path = raw_scan_dir.join("raw_scan.txt");
    let memory_probe_path = memory_probe_dir.join("memory_probe.txt");

    for dir in &[
        &blob_log_dir,
        &api_log_dir,
        &auto_capture_dir,
        &manual_capture_dir,
        &memory_probe_dir,
        &raw_scan_dir,
        &unmatched_paths_dir,
    ] {
        let _ = std::fs::create_dir_all(dir);
    }

    let wfm_top_cache_path = cache_dir.join("wfm_top_cache.json");
    let syndicate_catalog_path = cache_dir.join("syndicate_catalog.json");
    let img_cache_dir = cache_dir.join("img_cache");
    let _ = std::fs::create_dir_all(&img_cache_dir);
    let auction_ids_path = data_dir.join("auction_ids.json");
    let relics_run_prices_cache_path = cache_dir.join("relics_run_prices.json");

    // ── Factory reset ──────────────────────────────────────────────────────
    let reset_marker = paths::factory_reset_marker();
    if reset_marker.exists() {
        let _ = std::fs::remove_file(&reset_marker);
        for suffix in ["data.db", "data.db-wal", "data.db-shm"] {
            let _ = std::fs::remove_file(data_dir.join(suffix));
        }
    }

    // ── Database ───────────────────────────────────────────────────────────
    let conn = db::init_db(&db_path).expect("Failed to initialize database");

    // ── Version-based cache invalidation ───────────────────────────────────
    {
        const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
        let last_version = read_settings_map(&settings_path).ok().and_then(|m| {
            m.get("lastVersion")
                .and_then(|v| v.as_str().map(String::from))
        });
        if last_version.as_deref() != Some(CURRENT_VERSION) {
            for path in &[
                &items_cache_path,
                &recipes_cache_path,
                &relic_drops_cache_path,
                &relic_rewards_cache_path,
            ] {
                let _ = std::fs::remove_file(path);
            }
            wfcd::clear_cached_etags();
            let _ = merge_settings(&settings_path, |map| {
                map.insert(
                    "lastVersion".to_string(),
                    serde_json::Value::String(CURRENT_VERSION.to_string()),
                );
            });
        }
    }

    // ── Load initial state from caches ─────────────────────────────────────
    let initial = load_initial_state(InitialCachePaths {
        items_cache_path: &items_cache_path,
        recipes_cache_path: &recipes_cache_path,
        relic_drops_cache_path: &relic_drops_cache_path,
        relic_rewards_cache_path: &relic_rewards_cache_path,
        inventory_state_cache_path: &inventory_state_cache_path,
        syndicate_catalog_path: &syndicate_catalog_path,
        auction_ids_path: &auction_ids_path,
        relics_run_prices_cache_path: &relics_run_prices_cache_path,
        corrections_path: &config_dir.join("corrections.json"),
    });
    let diagnostics_enabled = read_settings_map(&settings_path)
        .ok()
        .and_then(|settings| {
            settings
                .get("ocrDiagnosticsEnabled")
                .and_then(|value| value.as_bool())
        })
        .unwrap_or(false);
    diagnostics::set_ocr_pipeline_diagnostics_enabled(diagnostics_enabled);

    // ── Tauri builder ──────────────────────────────────────────────────────
    tauri::Builder::default()
        .register_uri_scheme_protocol("ffauth", |ctx, req| {
            console_login::handle_ffauth(ctx.app_handle(), &req)
        })
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(AppState {
            db_path,
            items_cache_path,
            recipes_cache_path,
            relic_drops_cache_path,
            relic_rewards_cache_path,
            quantities_cache_path,
            inventory_state_cache_path,
            settings_path,
            log_path,
            changes_log_path,
            conn: Mutex::new(conn),
            wfcd_items: Mutex::new(initial.items),
            recipes: Mutex::new(initial.recipes),
            relic_drops: Mutex::new(initial.relic_drops),
            relic_rewards: Mutex::new(initial.relic_rewards),
            blueprint_to_result: Mutex::new(HashMap::new()),
            wiki_reward_names: Mutex::new(std::collections::HashSet::new()),
            weapon_dispositions: Mutex::new(initial.weapon_dispositions),
            current_quantities: Arc::new(Mutex::new(initial.quantities)),
            unique_quantities: Arc::new(Mutex::new(initial.unique)),
            current_mods: Arc::new(Mutex::new(initial.mods)),
            api_quantities_cache: Arc::new(Mutex::new(HashMap::new())),
            api_mod_copies_cache: Arc::new(Mutex::new(Vec::new())),
            last_ocr_frame: Arc::new(Mutex::new(None)),
            reward_diagnostic_session: Mutex::new(None),
            current_crafting: Arc::new(Mutex::new(vec![])),
            monitor_active: Arc::new(AtomicBool::new(false)),
            raw_scan_active: Arc::new(AtomicBool::new(false)),
            raw_scan_path,
            blob_log_enabled: Arc::new(AtomicBool::new(false)),
            blob_log_dir,
            api_log_enabled: Arc::new(AtomicBool::new(false)),
            api_log_dir,
            wfm: {
                let w = Arc::new(Wfm::new());
                for (slug, price) in initial.wfm_prices {
                    w.cache_price(slug, price);
                }
                w
            },
            wfm_price_queue: Arc::new(Mutex::new(std::collections::VecDeque::new())),
            wfm_priority_queue: Arc::new(Mutex::new(std::collections::VecDeque::new())),
            wfm_queue_started: Arc::new(AtomicBool::new(false)),
            wfm_top_cache_path,
            syndicate_catalog: Mutex::new(initial.syndicate_catalog),
            syndicate_catalog_path,
            auction_ids: Mutex::new(initial.auction_ids),
            auction_ids_path,
            img_cache_dir,
            img_server_port: Mutex::new(0),
            local_player_name: Arc::new(Mutex::new(None)),
            pending_relic_rewards: Mutex::new(None),
            relics_run_prices: Mutex::new(initial.relics_run_prices),
            relics_run_prices_cache_path,
            worldstate_cache: Mutex::new(None),
            debug_cat_enabled: Arc::new(AtomicBool::new(false)),
            auto_capture_dir,
            manual_capture_dir,
            memory_probe_path,
            unmatched_paths_dir,
            corrections: initial.corrections,
            force_pid_check: Arc::new(AtomicBool::new(false)),
            relic_pick_overlay_enabled: Arc::new(AtomicBool::new(false)),
            overlays_enabled: Arc::new(AtomicBool::new(false)),
        })
        .setup(setup_app)
        .invoke_handler(tauri::generate_handler![
            catalogue::get_all_items,
            catalogue::get_items_by_paths,
            catalogue::get_current_quantities,
            catalogue::get_player_name,
            catalogue::get_item_list_status,
            catalogue::fetch_item_list,
            stats::get_change_log,
            stats::get_tracked_items,
            stats::add_tracked_item,
            stats::remove_tracked_item,
            stats::get_item_snapshots,
            trade_log::get_trades,
            trade_log::add_trade,
            trade_log::delete_trade,
            diagnostics::clear_cache,
            settings::load_settings,
            settings::save_settings,
            diagnostics::read_scan_log,
            companion_api::log_api_changes,
            diagnostics::dump_memory_probe,
            diagnostics::toggle_raw_scan,
            diagnostics::set_blob_log,
            diagnostics::set_api_log,
            diagnostics::get_app_version,
            diagnostics::set_app_version,
            settings::force_quit,
            catalogue::get_weapon_catalog,
            catalogue::get_craftable_items,
            diagnostics::toggle_debug_categorization,
            catalogue::get_recipe,
            catalogue::get_recipes_bulk,
            catalogue::get_relic_drops,
            catalogue::get_relic_rewards,
            wfm_commands::fetch_wfm_items,
            wfm_commands::fetch_wfm_price,
            wfm_queue::start_wfm_queue,
            wfm_queue::wfm_queue_prices,
            wfm_queue::wfm_queue_price_priority,
            wfm_queue::wfm_get_cached_prices,
            wfm_top::get_wfm_top_items,
            wfm_commands::get_item_price,
            pricing::refresh_bulk_prices,
            pricing::get_bulk_prices,
            updater::check_for_update,
            updater::install_update,
            settings::factory_reset,
            wfm_commands::wfm_set_status,
            rivens::ocr_riven_log_error,
            rivens::riven_screen_visible,
            rivens::riven_screen_status,
            rivens::save_riven_roll,
            rivens::get_saved_riven_rolls,
            rivens::delete_saved_riven_roll,
            rivens::rename_saved_riven_roll,
            rivens::get_riven_weapons,
            rivens::reload_riven_database,
            rivens::analyze_riven,
            rivens::ocr_riven_screen,
            diagnostics::get_riven_session_log,
            wfm_commands::wfm_debug_dump,
            wfm_commands::wfm_get_riven_attributes,
            wfm_commands::wfm_get_item_orders,
            wfm_commands::wfm_get_item_statistics,
            wfm_commands::wfm_open_login_window,
            wfm_commands::wfm_close_login_window,
            wfm_commands::wfm_receive_jwt,
            wfm_commands::wfm_receive_tokens,
            wfm_commands::wfm_refresh_token,
            wfm_commands::wfm_set_jwt,
            wfm_commands::wfm_get_jwt,
            credentials::wfm_save_credentials,
            credentials::wfm_load_credentials,
            credentials::wfm_delete_credentials,
            wfm_commands::wfm_login,
            wfm_commands::wfm_logout,
            wfm_commands::wfm_get_session,
            wfm_commands::wfm_fetch_status,
            wfm_commands::wfm_get_orders,
            wfm_commands::wfm_get_item_info,
            wfm_commands::wfm_create_order,
            wfm_commands::wfm_update_order,
            wfm_commands::wfm_delete_order,
            wfm_commands::wfm_close_order,
            wfm_commands::wfm_create_riven_auction,
            wfm_commands::wfm_switch_riven_type,
            wfm_commands::wfm_get_my_riven_auctions,
            wfm_commands::wfm_delete_auction,
            wfm_commands::wfm_update_auction,
            wfm_commands::wfm_set_auction_visible,
            companion_api::scan_warframe_credentials,
            companion_api::scan_warframe_api_urls,
            companion_api::warframe_login,
            companion_api::fetch_warframe_inventory,
            companion_api::save_mastery_data,
            companion_api::get_saved_inventory,
            companion_api::get_rivens,
            rivens::get_weapon_dispositions,
            companion_api::save_api_inventory,
            syndicates::get_syndicate_stores,
            syndicates::get_research_lab_stores,
            worldstate::fetch_worldstate,
            diagnostics::get_warframe_window_rect,
            diagnostics::get_overlay_session_log,
            relic_pick::get_pending_relic_rewards,
            diagnostics::log_relic_fe,
            diagnostics::set_overlay_topmost,
            diagnostics::inject_overlay_diagnostic,
            diagnostics::debug_create_window,
            relic_pick::show_overlay_window,
            relic_pick::move_overlay_offscreen,
            diagnostics::get_diag_folder_size,
            diagnostics::clear_diag_folder,
            diagnostics::set_ocr_pipeline_diagnostics,
            diagnostics::capture_diagnostics,
            image_cache::get_img_cache_dir,
            image_cache::prewarm_image_cache,
            diagnostics::open_debug_folder,
            diagnostics::clear_debug_data,
            diagnostics::get_debug_data_size,
            start_monitor,
            monitor::stop_monitor,
            monitor::poke_scan,
            monitor::set_relic_pick_enabled,
            monitor::set_overlays_enabled,
            monitor::get_monitor_status,
            catalogue::get_blueprint_names,
            platform::get_system_locale,
            catalogue::get_current_crafting,
            relic_pick::debug_detect_fissure_era,
            relic_pick::test_relic_pick_overlay,
            relic_pick::hide_relic_pick_overlay,
            relic_pick::show_relic_pick_window,
            relic_pick::debug_ee_log_tail,
            console_login::open_console_login,
            wfcd::get_drop_data,
            pricing::get_cache_statuses,
            pricing::refresh_all_caches,
            diagnostics::start_memory_relic_debug,
            diagnostics::stop_memory_relic_debug,
        ])
        .on_window_event(|window, event| {
            let label = window.label().to_string();
            if label == "main" || label == "modular-popout" {
                let prefix = if label == "main" {
                    "window"
                } else {
                    "modularWin"
                };
                match event {
                    tauri::WindowEvent::Moved(_) | tauri::WindowEvent::Resized(_) => {
                        let app = window.app_handle();
                        if let Some(wv) = app.get_webview_window(&label) {
                            let state = app.state::<AppState>();
                            save_window_state(&wv, &state.settings_path, prefix);
                        }
                    }
                    tauri::WindowEvent::CloseRequested { .. } => {
                        // State is already saved on every Moved/Resized event.
                    }
                    tauri::WindowEvent::Destroyed if label == "main" => {
                        std::process::exit(0);
                    }
                    _ => {}
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_chars_splits_on_characters_not_bytes() {
        assert_eq!(truncate_chars("éé", 3), "éé");
        assert_eq!(truncate_chars("éé", 1), "é");
        assert_eq!(truncate_chars("abc", 2), "ab");
    }
}
