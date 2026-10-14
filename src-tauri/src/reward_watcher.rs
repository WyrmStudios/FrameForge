use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use tauri::{Emitter, Manager};

use crate::app_state::AppState;
use crate::events;
use crate::log_watcher;
use crate::wfcd::RelicReward;
use crate::append_to_file;

/// Shared state for the reward watcher thread.
pub(crate) struct RewardWatcherDeps {
    pub app: tauri::AppHandle,
    pub flag: Arc<AtomicBool>,
    pub relic_rewards: HashMap<String, Vec<RelicReward>>,
    pub auto_capture_dir: std::path::PathBuf,
}

/// Spawn the single EE.log tailer thread, started once at app startup and running
/// for the app's lifetime — independent of the memory scanner's on/off state.
///
/// This thread:
/// - Tails EE.log using FindFirstChangeNotificationW for instant wake-up
/// - Handles riven-screen open/close, relic-pick trigger/dismiss, trade completion,
///   and WFM whisper detection unconditionally (plain file I/O, not memory reading)
/// - Detects "VoidProjections: GetVoidProjectionReward" triggers and spawns async OCR
///   tasks to capture and match reward items — but only while `deps.flag` (the memory
///   scanner's `monitor_active` state) is true, matching the OCR feature's existing
///   dependency on the memory scanner being enabled
/// - Collects squad member names and session relics for OCR filtering
/// - Handles dismiss events and diagnostic logging
pub(crate) fn spawn_reward_watcher_thread(deps: RewardWatcherDeps) {
    let RewardWatcherDeps { app, flag, relic_rewards, auto_capture_dir } = deps;

    // Build OCR catalog from relic rewards
    let catalog_pairs = build_relic_reward_catalog(&relic_rewards);
    let catalog_pairs = Arc::new(catalog_pairs);

    let last_found_path = std::env::temp_dir().join("frameforge_last_reward.txt");

    // EE.log path
    let ee_log_path = dirs::data_local_dir()
        .map(|d| d.join("Warframe").join("EE.log"));

    // Shared flag: true while the reward screen is active according to EE.log
    let reward_screen_active = Arc::new(AtomicBool::new(false));
    let reward_screen_active2 = reward_screen_active.clone();

    // Unix-ms timestamp of the last relic-rewards emit with real items. Zero = never.
    // Written by the OCR task when it locks and emits; read by the dismiss handler to
    // enforce a minimum overlay display time so fast EE.log flushes don't hide the
    // overlay before the user has time to read it.
    let rewards_emitted_ms: Arc<std::sync::atomic::AtomicU64> =
        Arc::new(std::sync::atomic::AtomicU64::new(0));
    let rewards_emitted_ms_ocr  = rewards_emitted_ms.clone();
    let rewards_emitted_ms_ee   = rewards_emitted_ms.clone();

    // Mirror of the OCR loop's best-confirmed-so-far payload. If the reward screen
    // closes before OCR reaches "complete", dismiss_relic_rewards uses this to show
    // whatever was already confirmed instead of discarding it. Reset at the start
    // of each new trigger; consumed (and cleared) by dismiss_relic_rewards.
    let best_reward_payload: Arc<Mutex<Option<serde_json::Value>>> = Arc::new(Mutex::new(None));
    let best_reward_payload_ocr = best_reward_payload.clone();
    let best_reward_payload_ee  = best_reward_payload.clone();

    // Shared squad size: updated by EE.log watcher when VoidProjections sequence
    // completes, read by OCR loop for each attempt. This lets late-arriving squad
    // data (VoidProjections often arrives 1-2 s after the screen opens) inform
    // subsequent OCR retries so the card count is always correct.
    let shared_squad_size: Arc<Mutex<Option<usize>>> =
        Arc::new(Mutex::new(None));
    let shared_squad_size2 = Arc::clone(&shared_squad_size);

    // Squad member names collected from EE.log "AddSquadMember:" lines.
    // Passed to OCR so it can reject any text that fuzzy-matches a player name.
    let shared_squad_names: Arc<Mutex<Vec<String>>> =
        Arc::new(Mutex::new(Vec::new()));
    let shared_squad_names2 = Arc::clone(&shared_squad_names);

    let ee_ocr_app   = app.clone();
    let ee_catalog   = Arc::clone(&catalog_pairs);
    let ee_last_path = last_found_path.clone();
    let session_log_path = std::env::temp_dir().join("frameforge_overlay_session.txt");
    let ee_auto_capture_dir = auto_capture_dir.clone();

    if let Some(log_path) = ee_log_path {
        let flag = flag.clone();
        std::thread::spawn(move || {
            let mut file_pos: u64 = std::fs::metadata(&log_path)
                .map(|m| m.len()).unwrap_or(0);
            let mut active_since: Option<std::time::Instant> = None;
            // Last time a poll actually read new (non-empty) EE.log content.
            // Drives the fast/slow poll cadence below — recent game activity
            // means more triggers (riven, trade, relic-pick, reward) could be
            // seconds away, so every event type benefits from fast polling,
            // not just an already-open reward screen.
            let mut last_log_activity: Option<std::time::Instant> = None;
            const LOG_ACTIVITY_WINDOW: std::time::Duration = std::time::Duration::from_secs(5);
            // Cooldown: don't fire riven-screen-open again within 4 seconds of the last fire.
            let mut last_riven_fire: Option<std::time::Instant> = None;
            // Cooldown: prevent spawning multiple relic-pick OCR threads if the trigger fires rapidly.
            let mut last_relic_pick_trigger: Option<std::time::Instant> = None;
            // Rolling raw log text used to reconstruct multi-read trade dialogs.
            let mut trade_buffer = String::new();
            // Trailing partial-line buffer: a marker split across two reads by a
            // mid-write wake-up (e.g. a relic resource-load or dismiss line
            // half-flushed) must never be tested half-formed by the
            // `.contains()`/`.lines()` detectors below — a split "Resource load
            // completed" line silently drops that relic from session_relics.
            // Capped so a stalled or pathologically long line can't grow forever.
            let mut line_carry = String::new();
            const MAX_LINE_CARRY_BYTES: usize = 64 * 1024;
            use std::io::{Read, Seek, SeekFrom};

            log_watcher::seed_ee_log_names(&log_path, &shared_squad_names2, &ee_ocr_app);

            // ── VoidProjections reward sequence state ─────────────────────────
            // The game logs squad reward info BEFORE the screen trigger fires.
            // We accumulate it across poll iterations so it's ready when OCR starts.
            let mut vp_state = log_watcher::VoidProjectionState::default();
            // Cooldown: after any dismiss, block new triggers for 5 s to filter
            // stale EE.log lines that can arrive shortly after a dismiss.
            let mut last_dismiss_at: Option<std::time::Instant> = None;
            // ── Relic prefilter ───────────────────────────────────────────────────
            // Projection paths collected from "Resource load completed" EE.log lines
            // while squad loadouts download. Used at trigger time to narrow the OCR
            // candidate list from ~700 items to the ~6-24 rewards of the active relics.
            let mut session_relics: Vec<String> = Vec::new();
            // Catch up relics already loaded this mission before this thread
            // started (e.g. FrameForge launched mid-mission) — otherwise the
            // live tailer below, which only reads forward from file_pos, would
            // never see those earlier "Resource load completed" lines.
            log_watcher::seed_session_relics(&log_path, &mut session_relics);
            // One diagnostics folder per trigger→dismiss cycle.
            // Created at trigger, BMP written after overlay confirmed, session log at dismiss.
            let diag_arc: Arc<Mutex<Option<std::path::PathBuf>>> = Arc::new(Mutex::new(None));

            // Use FindFirstChangeNotificationW so we wake the instant EE.log is
            // written to disk instead of sleeping 200 ms between checks.
            let change_handle: isize = {
                use windows_sys::Win32::Storage::FileSystem::{
                    FindFirstChangeNotificationW, FILE_NOTIFY_CHANGE_LAST_WRITE,
                };
                let dir = log_path.parent().unwrap_or(std::path::Path::new("."));
                let dir_wide: Vec<u16> = dir.to_string_lossy()
                    .encode_utf16().chain(std::iter::once(0)).collect();
                unsafe { FindFirstChangeNotificationW(dir_wide.as_ptr(), 0, FILE_NOTIFY_CHANGE_LAST_WRITE) }
            };
            let use_notify = change_handle != -1isize; // -1 = INVALID_HANDLE_VALUE

            loop {
                // Poll fast (50 ms) whenever a reward screen is up OR the log has
                // written something within the last few seconds — active gameplay
                // means a riven unveil, trade, relic-pick, or reward trigger could
                // land any moment, and all of those benefit from fast detection,
                // not just an already-open reward screen. Falls back to a slow
                // 500 ms idle poll once the log (and thus the game) has been quiet.
                let reward_active = reward_screen_active2.load(Ordering::SeqCst);
                let log_recently_active = last_log_activity
                    .is_some_and(|t| t.elapsed() < LOG_ACTIVITY_WINDOW);
                let idle_wait_ms: u32 = if reward_active || log_recently_active { 50 } else { 500 };
                if use_notify {
                    use windows_sys::Win32::System::Threading::WaitForSingleObject;
                    use windows_sys::Win32::Storage::FileSystem::FindNextChangeNotification;
                    // Block until a write lands in the EE.log directory (the timeout keeps
                    // the flag check alive even when the game isn't writing).
                    unsafe { WaitForSingleObject(change_handle, idle_wait_ms); }
                    unsafe { FindNextChangeNotification(change_handle); }
                } else {
                    std::thread::sleep(std::time::Duration::from_millis(idle_wait_ms.min(200) as u64));
                }
                let Ok(mut f) = std::fs::File::open(&log_path) else { continue };
                let len = std::fs::metadata(&log_path).map(|m| m.len()).unwrap_or(0);
                if len < file_pos {
                    file_pos = 0;
                    // Log rotated/truncated — any carried fragment is from a
                    // different file and must not be glued onto the new one.
                    line_carry.clear();
                }
                if f.seek(SeekFrom::Start(file_pos)).is_err() { continue; }
                let mut raw = String::new();
                if f.read_to_string(&mut raw).is_err() { continue; }
                file_pos = len;
                if raw.is_empty() { continue; }
                last_log_activity = Some(std::time::Instant::now());

                line_carry.push_str(&raw);
                if line_carry.len() > MAX_LINE_CARRY_BYTES {
                    let mut cut = line_carry.len() - MAX_LINE_CARRY_BYTES;
                    while cut < line_carry.len() && !line_carry.is_char_boundary(cut) { cut += 1; }
                    line_carry.drain(..cut);
                }
                // Hand the detectors below only complete lines; a line still being
                // written by the game sits in line_carry until the next wake-up.
                let Some(split_at) = line_carry.rfind('\n') else { continue };
                let buf: String = line_carry.drain(..=split_at).collect();

                let lower = buf.to_lowercase();

                log_watcher::collect_void_projection_state(
                    &buf,
                    &mut vp_state,
                    &shared_squad_size2,
                    &session_log_path,
                );

                // Relics are announced before the reward screen opens, narrowing OCR candidates.
                log_watcher::collect_session_relics(&buf, &mut session_relics);

                // AddSquadMember, avatar changes and local login all feed the OCR filter.
                log_watcher::collect_ee_log_names(&buf, &shared_squad_names2, &ee_ocr_app);

                // ── WFM trade whisper detection ──────────────────────────────────
                if lower.contains("(warframe.market)") {
                    log_watcher::parse_and_emit_wfm_whisper(&ee_ocr_app, &buf);
                }

                log_watcher::handle_riven_events(&ee_ocr_app, &lower, &mut last_riven_fire);
                log_watcher::handle_relic_pick_events(&ee_ocr_app, &lower, &mut last_relic_pick_trigger);
                log_watcher::handle_trade_completion(&ee_ocr_app, &buf, &mut trade_buffer);

                // Unveil: riven challenge completion
                if lower.contains("modreveal") || (lower.contains("riven") && lower.contains("unveiled")) {
                    let _ = ee_ocr_app.emit(events::RIVEN_UNVEILED, ());
                }

                // Trigger: "VoidProjections: OpenVoidProjectionRewardScreenRMI" fires when
                // the reward screen actually opens on screen. A prior version of this code
                // waited for "GetVoidProjectionReward[s]" instead (the server delivering the
                // reward choices), believing the open event fired before cards were visible —
                // but field EE.log captures show the open event consistently precedes "Get"
                // by ~5s, and OCR attempts right after "Get" immediately see populated card
                // text, meaning those 5 seconds of real display time were being missed. Kept
                // "Get"/sequence-completed too since they're free (same line family) and
                // harmless as a fallback if "Open" is ever missing from a read batch.
                // Matching the singular prefix catches both "Reward" and "Rewards" variants.
                let has_trigger = lower.contains("voidprojections: openvoidprojectionrewardscreenrmi")
                    || lower.contains("voidprojections: getvoidprojectionreward")
                    || vp_state.consume_sequence_completed();

                let has_dismiss = log_watcher::dismiss_relic_rewards(
                    &ee_ocr_app,
                    &buf,
                    &session_log_path,
                    &diag_arc,
                    &reward_screen_active2,
                    &rewards_emitted_ms_ee,
                    &best_reward_payload_ee,
                    log_watcher::DismissState {
                        active_since: &mut active_since,
                        last_dismiss_at: &mut last_dismiss_at,
                        session_relics: &mut session_relics,
                        projection_state: &mut vp_state,
                    },
                );

                // The relic-pick grid only opens after the reward screen has closed.
                // Hide the reward overlay now rather than waiting for the "shut down" line.
                if active_since.is_some()
                    && lower.contains("themedprojectionmanager.lua: populateinventorygrid")
                {
                    log_watcher::close_reward_overlay(
                        &ee_ocr_app,
                        &session_log_path,
                        &diag_arc,
                        &reward_screen_active2,
                        &mut active_since,
                        &mut last_dismiss_at,
                        &rewards_emitted_ms_ee,
                        "PICK SCREEN OPENED (reward screen closed)",
                    );
                }

                // Requiem relics (Kuva Lich / Sister reward flow, path prefix
                // "T5VoidProjection") are never void-fissure rewards — the reward
                // catalog has no entries for them, so OCR would only ever mismatch.
                // Never show the overlay for a session that saw one.
                let has_requiem_relic = session_relics
                    .iter()
                    .any(|path| path.contains("T5VoidProjection"));

                // ── Trigger: skip if dismiss in same batch, screen already active,
                //    within 60 s of last dismiss, the memory scanner is off (this
                //    OCR feature has always been tied to `monitor_active`), or a
                //    Requiem relic was seen this session ─────────────────────────
                let trigger_allowed = !has_dismiss
                    && active_since.is_none()
                    && last_dismiss_at.is_none_or(|t| t.elapsed().as_secs() >= 5)
                    && flag.load(Ordering::SeqCst)
                    && !has_requiem_relic;
                if has_trigger && trigger_allowed {
                    reward_screen_active2.store(true, Ordering::SeqCst);
                    active_since = Some(std::time::Instant::now());

                    log_watcher::prepare_reward_trigger(
                        &ee_ocr_app,
                        &shared_squad_names,
                        &shared_squad_size,
                        &session_relics,
                    );

                    // Find the exact EE.log line that matched so we can log it
                    let trigger_line = buf.lines()
                        .find(|l| {
                            let ll = l.to_lowercase();
                            ll.contains("voidprojections: openvoidprojectionrewardscreenrmi")
                                || ll.contains("voidprojections: getvoidprojectionreward")
                        })
                        .unwrap_or("<unknown trigger line>")
                        .trim()
                        .to_string();

                    let ts0 = chrono::Local::now().format("%H:%M:%S%.3f");

                    let (filtered_cat, prefilter_log) = log_watcher::build_relic_reward_catalog(
                        &ee_ocr_app,
                        &session_relics,
                        &ee_catalog,
                    );
                    log_watcher::prepare_reward_session(
                        &session_log_path,
                        &shared_squad_names,
                        log_watcher::RewardTrigger {
                            timestamp: &ts0.to_string(),
                            trigger_line: &trigger_line,
                            prefilter_log: &prefilter_log,
                            catalog_len: filtered_cat.len(),
                        },
                        &ee_auto_capture_dir,
                        &diag_arc,
                        &ee_last_path,
                    );
                    // Fired immediately, before OCR starts — the ground-truth
                    // anchor for whether the trigger itself was seen on time.
                    log_watcher::schedule_trigger_moment_capture(Arc::clone(&diag_arc));

                    if ee_ocr_app.state::<AppState>().overlays_enabled.load(Ordering::SeqCst) {
                        let _ = ee_ocr_app.emit(events::FF_STATUS, "🔍 Relic reward screen detected");
                        // Tell App.tsx to pre-create the overlay window NOW, before OCR finishes.
                        // Window creation takes 1-2 s; pre-creating shaves that off the visible delay.
                        let _ = ee_ocr_app.emit(events::RELIC_TRIGGER, ());
                    }

                    let app          = ee_ocr_app.clone();
                    let cat          = filtered_cat; // relic prefilter (was: Arc::clone(&ee_catalog))
                    let _cat_len     = cat.len();
                    let fallback_cat = Arc::clone(&ee_catalog); // full catalog — used after 3 no-match attempts
                    let lpath        = ee_last_path.clone();
                    let slog         = session_log_path.clone();
                    let active       = reward_screen_active2.clone();
                    let emitted_ms   = rewards_emitted_ms_ocr.clone();
                    let shared_best  = best_reward_payload_ocr.clone();
                    let squad_arc    = Arc::clone(&shared_squad_size);
                    let names_arc    = Arc::clone(&shared_squad_names);
                    let diag_arc2    = Arc::clone(&diag_arc);
                    tauri::async_runtime::spawn(async move {
                        if !app.state::<AppState>().overlays_enabled.load(Ordering::SeqCst) {
                            return;
                        }
                        let deadline = std::time::Instant::now()
                            + std::time::Duration::from_secs(45);
                        if let Ok(mut g) = shared_best.lock() { *g = None; }
                        log_watcher::wait_for_squad_hint(&squad_arc).await;

                        // Allow the catalog to be rebuilt inside the loop — it may be empty
                        // when start_monitor fired before WFCD data finished loading.
                        let mut cat = cat;
                        let mut no_match_streak = 0u32;
                        let mut attempt = 0u32;
                        let mut best_item_count = 0usize;
                        let mut best_payload: Option<serde_json::Value> = None; // locked when complete
                        // When no EE squad hint is available, the first "complete" result may
                        // undercount cards (e.g. dark text hides a 2-line item name).
                        // soft_complete_at tracks the first attempt that returned complete-without-hint
                        // so we do one extra retry before locking.
                        let mut soft_complete_at: Option<usize> = None;
                        // Item count at the time soft_complete_at was set.
                        let mut soft_complete_count: usize = 0;
                        // If the same confirmed item set comes back on consecutive attempts,
                        // trust it instead of waiting out the full low-confidence/hint-wants-more
                        // retry window — a repeated read is itself evidence the answer is right,
                        // and this is what lets a stale EE.log squad hint (e.g. a player left
                        // mid-session) stop blocking on a card that will never be found.
                        let mut prev_confirmed: Option<Vec<String>> = None;
                        let mut stable_streak: u32 = 0;
                        const STABLE_ACCEPT: u32 = 2;
                        // Tracks the attempt where the confirmed set first became stable while
                        // still short of estimated_cards (e.g. one card's best catalog match
                        // never clears the confirm threshold). *complete only goes true once
                        // n_confirmed reaches the expected count, so the is_stable escape above
                        // — which lives inside the `if *complete` branch — can never fire for a
                        // card that's permanently unresolvable. This is the matching escape for
                        // the incomplete case: a handful of extra retries, then accept the stable
                        // partial read instead of spinning until dismiss or the 45s hard timeout.
                        let mut incomplete_stable_at: Option<usize> = None;
                        const INCOMPLETE_STABLE_GRACE: usize = 4;
                        loop {
                            attempt += 1;
                            // Rebuild catalog if WFCD hadn't loaded when this OCR session started.
                            // Runs only while cat is empty — once populated it stays populated.
                            if cat.is_empty() {
                                if let Some(fallback) = log_watcher::build_fallback_reward_catalog(&app) {
                                    cat = fallback;
                                }
                            }
                            let _ = app.emit(events::FF_STATUS, "📷 OCR scanning...");
                            let result = log_watcher::capture_reward_items(
                                &app,
                                Arc::clone(&cat),
                                Arc::clone(&squad_arc),
                                Arc::clone(&names_arc),
                            ).await;
                            // Re-read hint for confirm_ready logic below (same mutex, post-capture value).
                            let hint_squad = squad_arc.lock().ok().and_then(|g| *g);

                            let ts = chrono::Local::now().format("%H:%M:%S%.3f").to_string();
                            let sleep_ms = match &result {
                                // ✅ 1+ items found (solo=1, duo=2, trio=3, full squad=4)
                                Some((complete, low_confidence, ref items, ref positions, ref dbg)) if !items.is_empty() => {
                                    no_match_streak = 0;
                                    let payload = Some(serde_json::json!({
                                        "items": items, "positions": positions
                                    }));

                                    // "?:"-prefixed entries are unmatched OCR fragments, not real
                                    // catalog hits — count only the confirmed matches.
                                    let n_confirmed = items.iter().filter(|s| !s.starts_with("?:")).count();
                                    let confirmed_items: Vec<String> = items.iter()
                                        .filter(|s| !s.starts_with("?:")).cloned().collect();
                                    if !confirmed_items.is_empty() && prev_confirmed.as_ref() == Some(&confirmed_items) {
                                        stable_streak += 1;
                                    } else {
                                        stable_streak = if confirmed_items.is_empty() { 0 } else { 1 };
                                        prev_confirmed = Some(confirmed_items);
                                    }
                                    let is_stable = stable_streak >= STABLE_ACCEPT;

                                    let soft_retries_done = soft_complete_at
                                        .is_some_and(|sa| (attempt as usize).saturating_sub(sa) >= 3);
                                    let hint_wants_more = hint_squad
                                        .is_some_and(|h| h > n_confirmed);
                                    // A "complete" set of cards can still contain a low-confidence
                                    // pick (sparse OCR read let a coincidental score through) — give
                                    // it the same few extra retries as the no-squad-hint case before
                                    // accepting whatever the best read so far was. A repeated
                                    // identical read (is_stable) short-circuits both of those waits —
                                    // including a hint that's gone stale because a squad member left
                                    // after the expected-card count was computed.
                                    let confirm_ready = (!hint_wants_more || is_stable)
                                        && (hint_squad.is_some() || soft_retries_done)
                                        && (!*low_confidence || soft_retries_done || is_stable);

                                    // Save best result; only emit to overlay when confirmed (LOCK).
                                    let is_new_best = n_confirmed > best_item_count;
                                    if is_new_best {
                                        best_item_count = n_confirmed;
                                        best_payload = payload.clone();
                                        if let Ok(mut g) = shared_best.lock() { *g = payload.clone(); }
                                        log_watcher::log_reward_best_result(
                                            log_watcher::RewardAttempt {
                                                attempt, ts: &ts, items, dbg,
                                            },
                                            *complete, confirm_ready,
                                            log_watcher::RewardPaths {
                                                session_log_path: &slog,
                                                last_path: &lpath,
                                            },
                                        );
                                    } else if !*complete {
                                        // Non-empty OCR result that didn't beat the running best and
                                        // isn't a complete card set — e.g. attempt #1 landing on the
                                        // "Retrieving rewards..." transition and matching nothing.
                                        // Previously unlogged, leaving silent gaps in the session log.
                                        log_watcher::log_reward_partial_no_improvement(
                                            attempt, &ts, items, dbg, &slog,
                                        );
                                    }

                                    // Track how long the confirmed set has been stable while still
                                    // short of estimated_cards (see incomplete_stable_at comment above).
                                    if !*complete {
                                        if is_stable {
                                            if incomplete_stable_at.is_none() {
                                                incomplete_stable_at = Some(attempt as usize);
                                            }
                                        } else {
                                            incomplete_stable_at = None;
                                        }
                                    }

                                    // Stop retrying and emit ONLY when all expected cards found AND confirmed.
                                    if *complete {
                                        if confirm_ready {
                                            // Hard cutoff: if dismiss arrived while OCR was running, drop the result.
                                            if !active.load(Ordering::SeqCst) { break; }
                                            if !is_new_best {
                                                log_watcher::log_reward_confirm_no_improvement(
                                                    attempt, &ts, items, &slog,
                                                );
                                            }
                                            let _ = append_to_file(&slog, "[STEP 3] OVERLAY OPENED\n\n");
                                            let emit_val = if best_payload.is_some() { &best_payload } else { &payload };
                                            log_watcher::publish_relic_rewards(&app, emit_val.as_ref());
                                            emitted_ms.store(
                                                std::time::SystemTime::now()
                                                    .duration_since(std::time::UNIX_EPOCH)
                                                    .map(|d| d.as_millis() as u64)
                                                    .unwrap_or(0),
                                                Ordering::SeqCst,
                                            );
                                            if let Ok(mut g) = shared_best.lock() { *g = None; }
                                            log_watcher::schedule_reward_diagnostic_capture(Arc::clone(&diag_arc2));
                                            log_watcher::schedule_reward_safety_cleanup(
                                                app.clone(),
                                                slog.clone(),
                                                Arc::clone(&diag_arc2),
                                                true,
                                            );
                                            break;
                                        } else {
                                            if soft_complete_at.is_none() {
                                                soft_complete_at = Some(attempt as usize);
                                                soft_complete_count = best_item_count;
                                            }
                                        }
                                    } else if soft_complete_at.is_some() && n_confirmed <= soft_complete_count {
                                        if !active.load(Ordering::SeqCst) { break; }
                                        let emit_val = best_payload.clone().unwrap_or(serde_json::Value::Null);
                                        log_watcher::publish_relic_rewards(&app, Some(&emit_val));
                                        emitted_ms.store(
                                            std::time::SystemTime::now()
                                                .duration_since(std::time::UNIX_EPOCH)
                                                .map(|d| d.as_millis() as u64)
                                                .unwrap_or(0),
                                            Ordering::SeqCst,
                                        );
                                        if let Ok(mut g) = shared_best.lock() { *g = None; }
                                        let _ = append_to_file(&slog,
                                            "[STEP 3] OVERLAY OPENED (soft-complete confirmed — no improvement)\n\n");
                                        log_watcher::schedule_reward_diagnostic_capture(Arc::clone(&diag_arc2));
                                        log_watcher::schedule_reward_safety_cleanup(
                                            app.clone(),
                                            slog.clone(),
                                            Arc::clone(&diag_arc2),
                                            false,
                                        );
                                        break;
                                    } else if incomplete_stable_at
                                        .is_some_and(|sa| (attempt as usize).saturating_sub(sa) >= INCOMPLETE_STABLE_GRACE)
                                    {
                                        // Same confirmed subset for several seconds straight, still short
                                        // of the expected card count — the missing card(s) are either not
                                        // matchable against the catalog or not being read at all, and more
                                        // retries will not change that (confirmed here: Paris Prime Lower
                                        // Limb's best catalog match topped out at 0.61 across 29 identical
                                        // reads). Publish what we have instead of spinning until dismiss or
                                        // the 45s hard timeout with nothing ever shown to the player.
                                        if !active.load(Ordering::SeqCst) { break; }
                                        let emit_val = best_payload.clone()
                                            .or_else(|| payload.clone())
                                            .unwrap_or(serde_json::Value::Null);
                                        log_watcher::publish_relic_rewards(&app, Some(&emit_val));
                                        emitted_ms.store(
                                            std::time::SystemTime::now()
                                                .duration_since(std::time::UNIX_EPOCH)
                                                .map(|d| d.as_millis() as u64)
                                                .unwrap_or(0),
                                            Ordering::SeqCst,
                                        );
                                        if let Ok(mut g) = shared_best.lock() { *g = None; }
                                        let _ = append_to_file(&slog,
                                            "[STEP 3] OVERLAY OPENED (stable partial — gave up waiting for remaining card)\n\n");
                                        log_watcher::schedule_reward_diagnostic_capture(Arc::clone(&diag_arc2));
                                        log_watcher::schedule_reward_safety_cleanup(
                                            app.clone(),
                                            slog.clone(),
                                            Arc::clone(&diag_arc2),
                                            false,
                                        );
                                        break;
                                    }
                                    // Partial result (or soft-complete pending confirmation) — retry
                                    400u64
                                }
                                // ⬛ Dark/blank frame — PrintWindow returned nearly-black
                                Some((_, _, _, _, ref dbg)) if dbg.starts_with("dark-frame") => {
                                    log_watcher::log_reward_dark_frame(&app, attempt, &ts, dbg, &slog, &lpath)
                                }
                                // ⬜ OCR ran but returned no text
                                Some((_, _, _, _, ref dbg)) if dbg.starts_with("ocr-empty") => {
                                    log_watcher::log_reward_ocr_empty(&app, attempt, &ts, dbg, &slog, &lpath)
                                }
                                // ❌ Text found but no catalog match
                                Some((_, _, ref items, _, ref dbg)) => {
                                    log_watcher::log_reward_no_match(
                                        &app,
                                        log_watcher::RewardAttempt {
                                            attempt, ts: &ts, items, dbg,
                                        },
                                        &mut no_match_streak, &mut cat, &fallback_cat,
                                        log_watcher::RewardPaths {
                                            session_log_path: &slog,
                                            last_path: &lpath,
                                        },
                                        &diag_arc2,
                                    )
                                }
                                // ⚠️ Warframe window not found
                                None => {
                                    log_watcher::log_reward_capture_failed(&app, attempt, &ts, &slog, &lpath)
                                }
                            };

                            if std::time::Instant::now() >= deadline {
                                log_watcher::finalize_reward_ocr_timeout(
                                    &app,
                                    best_payload,
                                    &active,
                                    &slog,
                                    &diag_arc2,
                                );
                                break;
                            }
                            if !active.load(Ordering::SeqCst) {
                                log_watcher::log_reward_ocr_stopped(&slog);
                                break;
                            }
                            tokio::time::sleep(std::time::Duration::from_millis(sleep_ms)).await;
                        }
                    });

                } // end trigger block

                log_watcher::auto_dismiss_relic_rewards(
                    &ee_ocr_app,
                    &session_log_path,
                    &diag_arc,
                    &reward_screen_active2,
                    &mut active_since,
                    &mut last_dismiss_at,
                    &rewards_emitted_ms_ee,
                );
            }
        });
    }
}

/// Build the OCR catalog from relic rewards.
/// Two keys per relic (uniqueName + display name); dedup by unique_name.
fn build_relic_reward_catalog(
    relic_rewards: &HashMap<String, Vec<RelicReward>>,
) -> Vec<(String, String)> {
    let mut catalog_pairs: Vec<(String, String)> = relic_rewards
        .values()
        .flat_map(|rewards| rewards.iter())
        .filter(|r| !r.name.is_empty())
        .map(|r| (r.unique_name.clone(), r.name.clone()))
        .collect();
    catalog_pairs.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
    catalog_pairs.dedup_by(|a, b| !a.0.is_empty() && a.0 == b.0);
    catalog_pairs
}
