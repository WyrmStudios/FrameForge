use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use tauri::Emitter;

use crate::{arbitrations, db, events, log_tail};
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
            let mut arbitration_runs = db::ArbitrationRecorder::default();
            let existed_at_start = std::fs::metadata(&log_path).is_ok();
            let mut tail = log_tail::LogTail::from_start(log_path.clone());
            // Only arbitration consumes history; old trade prompts and reward overlays
            // must not replay. Keep the parser so an active run can finish live.
            // A log that was already there holds history to drain; one that does
            // not exist yet will only ever contain bytes written while we watch,
            // so those are live. Drained in pieces so a large file is never one
            // allocation.
            if existed_at_start {
                while let Some(chunk) = tail.read() {
                    arbitrations::record_arbitration_runs(&ee_ocr_app, &mut arbitration_runs, chunk.text, false);
                }
            }
            let mut backfilled = !existed_at_start || tail.has_read();
            let mut pending_lines = String::new();
            let mut active_since: Option<std::time::Instant> = None;
            // Cooldown: don't fire riven-screen-open again within 4 seconds of the last fire.
            let mut last_riven_fire: Option<std::time::Instant> = None;
            // Cooldown: prevent spawning multiple relic-pick OCR threads if the trigger fires rapidly.
            let mut last_relic_pick_trigger: Option<std::time::Instant> = None;
            // Rolling raw log text used to reconstruct multi-read trade dialogs.
            let mut trade_buffer = String::new();

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
                if use_notify {
                    use windows_sys::Win32::System::Threading::WaitForSingleObject;
                    use windows_sys::Win32::Storage::FileSystem::FindNextChangeNotification;
                    // Block until a write lands in the EE.log directory (500 ms safety timeout
                    // keeps the flag check alive even when the game isn't writing).
                    unsafe { WaitForSingleObject(change_handle, 500); }
                    unsafe { FindNextChangeNotification(change_handle); }
                } else {
                    std::thread::sleep(std::time::Duration::from_millis(200));
                }
                let Some(chunk) = tail.read() else {
                    backfilled |= tail.has_read();
                    arbitrations::record_arbitration_runs(&ee_ocr_app, &mut arbitration_runs, String::new(), false);
                    continue;
                };
                if chunk.restarted {
                    // A replacement starts a new parser but retains failed writes.
                    arbitration_runs.restart();
                    pending_lines.clear();
                    trade_buffer.clear();
                    last_riven_fire = None;
                    last_relic_pick_trigger = None;
                }
                // The boundary is known once the tail has read the file at least
                // once: what follows a replacement file is new bytes of a new
                // session, so it is live like any other append.
                let live = backfilled;
                backfilled = true;
                let buf = chunk.text;
                arbitrations::record_arbitration_runs(&ee_ocr_app, &mut arbitration_runs, buf.clone(), live);
                if !live { continue; }
                pending_lines.push_str(&buf);
                let Some(end) = pending_lines.rfind('\n') else { continue; };
                let remainder = pending_lines.split_off(end + 1);
                let buf = std::mem::replace(&mut pending_lines, remainder);

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

                // Trigger: "VoidProjections: GetVoidProjectionReward[s]" fires when the
                // server actually delivers the reward choices to the client — later than
                // the old "initialized" / "openvoidprojectionrewardscreen" lines, which
                // fired before the cards were visible in endless missions.
                // Matching the singular prefix catches both "Reward" and "Rewards" variants.
                let has_trigger = lower.contains("voidprojections: getvoidprojectionreward")
                    || vp_state.consume_sequence_completed();

                let has_dismiss = log_watcher::dismiss_relic_rewards(
                    &ee_ocr_app,
                    &buf,
                    &session_log_path,
                    &diag_arc,
                    &reward_screen_active2,
                    &rewards_emitted_ms_ee,
                    log_watcher::DismissState {
                        active_since: &mut active_since,
                        last_dismiss_at: &mut last_dismiss_at,
                        session_relics: &mut session_relics,
                        projection_state: &mut vp_state,
                    },
                );

                // ── Trigger: skip if dismiss in same batch, screen already active,
                //    within 60 s of last dismiss, or the memory scanner is off (this
                //    OCR feature has always been tied to `monitor_active`) ───────────
                let trigger_allowed = !has_dismiss
                    && active_since.is_none()
                    && last_dismiss_at.is_none_or(|t| t.elapsed().as_secs() >= 5)
                    && flag.load(Ordering::SeqCst);
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
                            ll.contains("voidprojections: getvoidprojectionreward")
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

                    let _ = ee_ocr_app.emit(events::FF_STATUS, "🔍 Relic reward screen detected");
                    // Tell App.tsx to pre-create the overlay window NOW, before OCR finishes.
                    // Window creation takes 1-2 s; pre-creating shaves that off the visible delay.
                    let _ = ee_ocr_app.emit(events::RELIC_TRIGGER, ());

                    let app          = ee_ocr_app.clone();
                    let cat          = filtered_cat; // relic prefilter (was: Arc::clone(&ee_catalog))
                    let _cat_len     = cat.len();
                    let fallback_cat = Arc::clone(&ee_catalog); // full catalog — used after 3 no-match attempts
                    let lpath        = ee_last_path.clone();
                    let slog         = session_log_path.clone();
                    let active       = reward_screen_active2.clone();
                    let emitted_ms   = rewards_emitted_ms_ocr.clone();
                    let squad_arc    = Arc::clone(&shared_squad_size);
                    let names_arc    = Arc::clone(&shared_squad_names);
                    let diag_arc2    = Arc::clone(&diag_arc);
                    tauri::async_runtime::spawn(async move {
                        let deadline = std::time::Instant::now()
                            + std::time::Duration::from_secs(45);
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

                                    let soft_retries_done = soft_complete_at
                                        .is_some_and(|sa| (attempt as usize).saturating_sub(sa) >= 3);
                                    let hint_wants_more = hint_squad
                                        .is_some_and(|h| h > items.len());
                                    // A "complete" set of cards can still contain a low-confidence
                                    // pick (sparse OCR read let a coincidental score through) — give
                                    // it the same few extra retries as the no-squad-hint case before
                                    // accepting whatever the best read so far was.
                                    let confirm_ready = !hint_wants_more
                                        && (hint_squad.is_some() || soft_retries_done)
                                        && (!*low_confidence || soft_retries_done);

                                    // Save best result; only emit to overlay when confirmed (LOCK).
                                    let is_new_best = items.len() > best_item_count;
                                    if is_new_best {
                                        best_item_count = items.len();
                                        best_payload = payload.clone();
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
                                    } else if soft_complete_at.is_some() && items.len() <= soft_complete_count {
                                        if !active.load(Ordering::SeqCst) { break; }
                                        let emit_val = best_payload.clone().unwrap_or(serde_json::Value::Null);
                                        log_watcher::publish_relic_rewards(&app, Some(&emit_val));
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
