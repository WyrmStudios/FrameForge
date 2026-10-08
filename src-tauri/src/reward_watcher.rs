use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use tauri::{Emitter, Manager};

use crate::app_state::AppState;
use crate::events;
use crate::log_watcher;
use crate::reward_pipeline;
use crate::reward_pipeline::AttemptEvent;
use crate::wfcd::RelicReward;

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
    let RewardWatcherDeps {
        app,
        flag,
        relic_rewards,
        auto_capture_dir,
    } = deps;

    // Build OCR catalog from relic rewards
    let catalog_pairs = build_relic_reward_catalog(&relic_rewards);
    let catalog_pairs = Arc::new(catalog_pairs);

    // EE.log path
    let ee_log_path = dirs::data_local_dir().map(|d| d.join("Warframe").join("EE.log"));

    // Shared flag: true while the reward screen is active according to EE.log
    let reward_screen_active = Arc::new(AtomicBool::new(false));
    let reward_screen_active2 = reward_screen_active.clone();

    // Monotonic counter bumped on every new trigger. The spawned OCR task
    // captures its own value at spawn time and re-checks it (alongside
    // `reward_screen_active`) before publishing or touching shared overlay
    // state. A bare bool can't tell "my session is still active" apart from
    // "a NEWER session is active" — if a stale task from a cancelled burst
    // wakes up after a fast dismiss-then-retrigger, `reward_screen_active`
    // may already be true again for the new session, so the bool alone would
    // let the stale task publish its late result over the new one's.
    let reward_session_id = Arc::new(AtomicU64::new(0));
    let reward_session_id2 = reward_session_id.clone();

    // Unix-ms timestamp of the last relic-rewards emit with real items. Zero = never.
    // Written by the OCR task when it locks and emits; read by the dismiss handler to
    // enforce a minimum overlay display time so fast EE.log flushes don't hide the
    // overlay before the user has time to read it.
    let rewards_emitted_ms: Arc<std::sync::atomic::AtomicU64> =
        Arc::new(std::sync::atomic::AtomicU64::new(0));
    let rewards_emitted_ms_ocr = rewards_emitted_ms.clone();
    let rewards_emitted_ms_ee = rewards_emitted_ms.clone();

    // The EE.log watcher updates this between OCR attempts. Keep the size and
    // its provenance together so each attempt snapshots a consistent hint.
    let shared_squad_hint: Arc<Mutex<reward_pipeline::SquadHint>> =
        Arc::new(Mutex::new(reward_pipeline::SquadHint::default()));
    let shared_squad_hint_ee = Arc::clone(&shared_squad_hint);

    // Squad member names collected from EE.log "AddSquadMember:" lines.
    // Passed to OCR so it can reject any text that fuzzy-matches a player name.
    let shared_squad_names: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let shared_squad_names2 = Arc::clone(&shared_squad_names);

    let ee_ocr_app = app.clone();
    let ee_catalog = Arc::clone(&catalog_pairs);
    let ee_auto_capture_dir = auto_capture_dir.clone();

    if let Some(log_path) = ee_log_path {
        let flag = flag.clone();
        std::thread::spawn(move || {
            let mut file_pos: u64 = std::fs::metadata(&log_path).map(|m| m.len()).unwrap_or(0);
            let mut active_since: Option<std::time::Instant> = None;
            // Cooldown: don't fire riven-screen-open again within 4 seconds of the last fire.
            let mut last_riven_fire: Option<std::time::Instant> = None;
            // Cooldown: prevent spawning multiple relic-pick OCR threads if the trigger fires rapidly.
            let mut last_relic_pick_trigger: Option<std::time::Instant> = None;
            // Rolling raw log text used to reconstruct multi-read trade dialogs.
            let mut trade_buffer = String::new();
            // Trailing partial-line buffer: a marker split across two reads by a
            // mid-write wake-up (e.g. the dismiss line half-flushed) must never be
            // tested half-formed by the `.contains()`/`.lines()` detectors below.
            // Capped so a stalled or pathologically long line can't grow forever.
            let mut line_carry = String::new();
            const MAX_LINE_CARRY_BYTES: usize = 64 * 1024;
            use std::io::{Read, Seek, SeekFrom};

            log_watcher::seed_ee_log_names(&log_path, &shared_squad_names2, &ee_ocr_app);

            // ── VoidProjections reward sequence state ─────────────────────────
            // The game logs squad reward info BEFORE the screen trigger fires.
            // We accumulate it across poll iterations so it's ready when OCR starts.
            let mut vp_state = reward_pipeline::VoidProjectionState::default();
            // Cooldown: after any dismiss, block new triggers for 5 s to filter
            // stale EE.log lines that can arrive shortly after a dismiss.
            let mut last_dismiss_at: Option<std::time::Instant> = None;
            // ── Relic prefilter ───────────────────────────────────────────────────
            // Projection paths collected from "Resource load completed" EE.log lines
            // while squad loadouts download. Used at trigger time to narrow the OCR
            // candidate list from ~700 items to the ~6-24 rewards of the active relics.
            let mut session_relics: Vec<String> = Vec::new();
            // Each trigger owns an immutable diagnostics handle. Stale tasks keep
            // their own handle and cannot write into a newer session's directory.
            let mut diagnostic_session: Option<Arc<reward_pipeline::RewardDiagnosticSession>> =
                None;

            // Use FindFirstChangeNotificationW so we wake the instant EE.log is
            // written to disk instead of sleeping 200 ms between checks.
            let change_handle: isize = {
                use windows_sys::Win32::Storage::FileSystem::{
                    FindFirstChangeNotificationW, FILE_NOTIFY_CHANGE_LAST_WRITE,
                };
                let dir = log_path.parent().unwrap_or(std::path::Path::new("."));
                let dir_wide: Vec<u16> = dir
                    .to_string_lossy()
                    .encode_utf16()
                    .chain(std::iter::once(0))
                    .collect();
                unsafe {
                    FindFirstChangeNotificationW(
                        dir_wide.as_ptr(),
                        0,
                        FILE_NOTIFY_CHANGE_LAST_WRITE,
                    )
                }
            };
            let use_notify = change_handle != -1isize; // -1 = INVALID_HANDLE_VALUE

            loop {
                // While a reward screen is up, poll much more often so the closing
                // line is picked up quickly. The 20 s auto-dismiss still applies.
                // Also switch to fast polling as soon as relics are known (well
                // before the trigger line itself): this moves the up-to-500ms
                // idle-detection tax to a moment nobody's watching the overlay for,
                // so the actual "VoidProjections: GetVoidProjectionReward" line is
                // caught within ~50ms instead of up to 500ms late.
                let reward_active =
                    reward_screen_active2.load(Ordering::SeqCst) || !session_relics.is_empty();
                let idle_wait_ms: u32 = if reward_active { 50 } else { 500 };
                if use_notify {
                    use windows_sys::Win32::Storage::FileSystem::FindNextChangeNotification;
                    use windows_sys::Win32::System::Threading::WaitForSingleObject;
                    // Block until a write lands in the EE.log directory (the timeout keeps
                    // the flag check alive even when the game isn't writing).
                    unsafe {
                        WaitForSingleObject(change_handle, idle_wait_ms);
                    }
                    unsafe {
                        FindNextChangeNotification(change_handle);
                    }
                } else {
                    std::thread::sleep(std::time::Duration::from_millis(
                        idle_wait_ms.min(200) as u64
                    ));
                }
                let Ok(mut f) = std::fs::File::open(&log_path) else {
                    continue;
                };
                let len = std::fs::metadata(&log_path).map(|m| m.len()).unwrap_or(0);
                if len < file_pos {
                    file_pos = 0;
                    // Log rotated/truncated — any carried fragment is from a
                    // different file and must not be glued onto the new one.
                    line_carry.clear();
                }
                if f.seek(SeekFrom::Start(file_pos)).is_err() {
                    continue;
                }
                let mut raw = String::new();
                if f.read_to_string(&mut raw).is_err() {
                    continue;
                }
                file_pos = len;
                if raw.is_empty() {
                    continue;
                }

                line_carry.push_str(&raw);
                if line_carry.len() > MAX_LINE_CARRY_BYTES {
                    let mut cut = line_carry.len() - MAX_LINE_CARRY_BYTES;
                    while cut < line_carry.len() && !line_carry.is_char_boundary(cut) {
                        cut += 1;
                    }
                    line_carry.drain(..cut);
                }
                // Hand the detectors below only complete lines; a line still being
                // written by the game sits in line_carry until the next wake-up.
                let Some(split_at) = line_carry.rfind('\n') else {
                    continue;
                };
                let buf: String = line_carry.drain(..=split_at).collect();

                let lower = buf.to_lowercase();

                reward_pipeline::collect_void_projection_state(
                    &buf,
                    &mut vp_state,
                    &shared_squad_hint_ee,
                    diagnostic_session.as_ref(),
                );

                // Relics are announced before the reward screen opens, narrowing OCR candidates.
                reward_pipeline::collect_session_relics(&buf, &mut session_relics);

                // AddSquadMember, avatar changes and local login all feed the OCR filter.
                log_watcher::collect_ee_log_names(&buf, &shared_squad_names2, &ee_ocr_app);

                // ── WFM trade whisper detection ──────────────────────────────────
                if lower.contains("(warframe.market)") {
                    log_watcher::parse_and_emit_wfm_whisper(&ee_ocr_app, &buf);
                }

                log_watcher::handle_riven_events(&ee_ocr_app, &lower, &mut last_riven_fire);
                log_watcher::handle_relic_pick_events(
                    &ee_ocr_app,
                    &lower,
                    &mut last_relic_pick_trigger,
                );
                log_watcher::handle_trade_completion(&ee_ocr_app, &buf, &mut trade_buffer);

                // Unveil: riven challenge completion
                if lower.contains("modreveal")
                    || (lower.contains("riven") && lower.contains("unveiled"))
                {
                    let _ = ee_ocr_app.emit(events::RIVEN_UNVEILED, ());
                }

                // Trigger: "VoidProjections: GetVoidProjectionReward[s]" fires when the
                // server actually delivers the reward choices to the client — later than
                // the old "initialized" / "openvoidprojectionrewardscreen" lines, which
                // fired before the cards were visible in endless missions.
                // Matching the singular prefix catches both "Reward" and "Rewards" variants.
                const TRIGGER_MARKER: &str = "voidprojections: getvoidprojectionreward";
                const DISMISS_MARKERS: [&str; 3] = [
                    "relic reward screen shut down",
                    "closevoidprojectionrewardscreen",
                    "matchingservice::endsession",
                ];
                let trigger_marker_pos = lower.find(TRIGGER_MARKER);
                let dismiss_marker_pos = DISMISS_MARKERS.iter().filter_map(|m| lower.find(m)).min();
                let has_trigger =
                    trigger_marker_pos.is_some() || vp_state.consume_sequence_completed();

                let has_dismiss = reward_pipeline::dismiss_relic_rewards(
                    &ee_ocr_app,
                    &buf,
                    diagnostic_session.clone(),
                    &reward_screen_active2,
                    &rewards_emitted_ms_ee,
                    reward_pipeline::DismissState {
                        active_since: &mut active_since,
                        last_dismiss_at: &mut last_dismiss_at,
                        session_relics: &mut session_relics,
                        projection_state: &mut vp_state,
                        session_counter: &reward_session_id2,
                    },
                );

                // The relic-pick grid only opens after the reward screen has closed.
                // Hide the reward overlay now rather than waiting for the "shut down" line.
                if active_since.is_some()
                    && lower.contains("themedprojectionmanager.lua: populateinventorygrid")
                {
                    reward_pipeline::close_reward_overlay(
                        &ee_ocr_app,
                        diagnostic_session.clone(),
                        reward_pipeline::CloseState {
                            reward_screen_active: &reward_screen_active2,
                            active_since: &mut active_since,
                            last_dismiss_at: &mut last_dismiss_at,
                            rewards_emitted_ms: &rewards_emitted_ms_ee,
                        },
                        "PICK SCREEN OPENED (reward screen closed)",
                    );
                }

                // ── Trigger: skip if screen already active, too soon after the last
                //    dismiss, or the memory scanner is off (this OCR feature has
                //    always been tied to `monitor_active`) ──────────────────────────
                //
                // A trigger sharing a read with a dismiss is only the stale tail of
                // the occurrence that's closing if it textually precedes (or is) the
                // dismiss line. Cracking relics back-to-back (e.g. Void Flood) can
                // land a dismiss and the NEXT relic's trigger in one read, or land
                // the trigger within a couple seconds of the previous dismiss on a
                // separate read — both are real, new occurrences, and dropping them
                // loses them for good since the reader has already advanced past
                // that text. Only treat the trigger as stale when it's genuinely
                // ambiguous (no literal marker to order it against the dismiss).
                let trigger_is_new_after_dismiss = match (trigger_marker_pos, dismiss_marker_pos) {
                    (Some(t), Some(d)) => t > d,
                    _ => false,
                };
                let trigger_allowed = (!has_dismiss || trigger_is_new_after_dismiss)
                    && active_since.is_none()
                    && (trigger_is_new_after_dismiss
                        || last_dismiss_at.is_none_or(|t| t.elapsed().as_millis() >= 1000))
                    && flag.load(Ordering::SeqCst);
                if has_trigger && trigger_allowed {
                    reward_screen_active2.store(true, Ordering::SeqCst);
                    active_since = Some(std::time::Instant::now());
                    let my_session = reward_session_id2.fetch_add(1, Ordering::SeqCst) + 1;
                    let trigger_at = std::time::Instant::now();

                    reward_pipeline::prepare_reward_trigger(
                        &ee_ocr_app,
                        &shared_squad_names,
                        &shared_squad_hint,
                        &session_relics,
                    );

                    // Find the exact EE.log line that matched so we can log it
                    let trigger_line = buf
                        .lines()
                        .find(|l| {
                            let ll = l.to_lowercase();
                            ll.contains("voidprojections: getvoidprojectionreward")
                        })
                        .unwrap_or("<unknown trigger line>")
                        .trim()
                        .to_string();

                    let ts0 = chrono::Local::now().format("%H:%M:%S%.3f");

                    let (filtered_cat, prefilter_log) =
                        reward_pipeline::filter_relic_reward_catalog(
                            &ee_ocr_app,
                            &session_relics,
                            &ee_catalog,
                        );
                    diagnostic_session = reward_pipeline::prepare_reward_session(
                        my_session,
                        &shared_squad_names,
                        reward_pipeline::RewardTrigger {
                            timestamp: &ts0.to_string(),
                            trigger_line: &trigger_line,
                            prefilter_log: &prefilter_log,
                            catalog_len: filtered_cat.len(),
                        },
                        &ee_auto_capture_dir,
                    );
                    if let Ok(mut current) = ee_ocr_app
                        .state::<AppState>()
                        .reward_diagnostic_session
                        .lock()
                    {
                        *current = diagnostic_session.clone();
                    }
                    reward_pipeline::log_session_hint(
                        &shared_squad_hint,
                        &session_relics,
                        diagnostic_session.as_ref(),
                    );

                    if ee_ocr_app
                        .state::<AppState>()
                        .overlays_enabled
                        .load(Ordering::SeqCst)
                    {
                        let _ =
                            ee_ocr_app.emit(events::FF_STATUS, "🔍 Relic reward screen detected");
                        // Tell App.tsx to pre-create the overlay window NOW, before OCR finishes.
                        // Window creation takes 1-2 s; pre-creating shaves that off the visible delay.
                        let _ = ee_ocr_app.emit(events::RELIC_TRIGGER, ());
                    }

                    let app = ee_ocr_app.clone();
                    let cat = filtered_cat; // relic prefilter (was: Arc::clone(&ee_catalog))
                    let _cat_len = cat.len();
                    let fallback_cat = Arc::clone(&ee_catalog); // full catalog — used after 3 no-match attempts
                    let active = reward_screen_active2.clone();
                    let session_counter = reward_session_id2.clone();
                    let emitted_ms = rewards_emitted_ms_ocr.clone();
                    let squad_hint_arc = Arc::clone(&shared_squad_hint);
                    let names_arc = Arc::clone(&shared_squad_names);
                    let diagnostic_session = diagnostic_session.clone();
                    tauri::async_runtime::spawn(async move {
                        if !app
                            .state::<AppState>()
                            .overlays_enabled
                            .load(Ordering::SeqCst)
                        {
                            return;
                        }
                        let deadline =
                            std::time::Instant::now() + std::time::Duration::from_secs(45);
                        // True while this task's trigger is still the current one — a
                        // bare `active` bool can flip back to true for a NEWER session
                        // before this (now-stale) task notices, so both must hold.
                        let session_still_valid = || {
                            active.load(Ordering::SeqCst)
                                && session_counter.load(Ordering::SeqCst) == my_session
                        };

                        // Allow the catalog to be rebuilt inside the loop — it may be empty
                        // when start_monitor fired before WFCD data finished loading.
                        let mut cat = cat;
                        let mut no_match_streak = 0u32;
                        let mut attempt = 0u32;
                        let mut best_item_count = 0usize;
                        let mut best_attempt = 0u32;
                        let mut best_payload: Option<serde_json::Value> = None; // locked when complete
                                                                                // When no EE squad hint is available, the first "complete" result may
                                                                                // undercount cards (e.g. dark text hides a 2-line item name).
                                                                                // soft_complete_at tracks the first attempt that returned complete-without-hint
                                                                                // so we do one extra retry before locking.
                        let mut soft_complete_at: Option<usize> = None;
                        // Item count at the time soft_complete_at was set.
                        let mut soft_complete_count: usize = 0;
                        // Set for exactly one attempt after a low-confidence, text-bearing
                        // result, so the next attempt re-reads the same frame through
                        // preprocessing instead of paying for a brand-new capture.
                        let mut reuse_next_frame = false;
                        loop {
                            attempt += 1;
                            let reuse_this_attempt = reuse_next_frame;
                            reuse_next_frame = false;
                            // Rebuild catalog if WFCD hadn't loaded when this OCR session started.
                            // Runs only while cat is empty — once populated it stays populated.
                            if cat.is_empty() {
                                if let Some(fallback) =
                                    reward_pipeline::build_fallback_reward_catalog(&app)
                                {
                                    cat = fallback;
                                }
                            }
                            let _ = app.emit(events::FF_STATUS, "📷 OCR scanning...");
                            let result = reward_pipeline::capture_reward_items(
                                &app,
                                Arc::clone(&cat),
                                Arc::clone(&squad_hint_arc),
                                Arc::clone(&names_arc),
                                reuse_this_attempt,
                            )
                            .await;
                            // Re-read hint for confirm_ready logic below (same mutex, post-capture value).
                            let hint_squad = squad_hint_arc.lock().ok().and_then(|hint| hint.size);

                            let ts = reward_pipeline::now_ts();
                            let after_ms = trigger_at.elapsed().as_millis() as u64;
                            let mode = if reuse_this_attempt { "reuse" } else { "fresh" };
                            let sleep_ms = match &result {
                                // ✅ 1+ items found (solo=1, duo=2, trio=3, full squad=4)
                                Some((
                                    complete,
                                    low_confidence,
                                    ref items,
                                    ref positions,
                                    ref diag,
                                )) if !items.is_empty() => {
                                    no_match_streak = 0;
                                    // Give the frame-reuse fallback exactly one shot: only
                                    // arm it off a fresh capture, never chain reuse-of-reuse
                                    // (preprocessing the same pixels twice yields the same
                                    // result, so that would just burn the 45 s deadline).
                                    if *low_confidence && !reuse_this_attempt {
                                        reuse_next_frame = true;
                                    }
                                    let payload = Some(serde_json::json!({
                                        "items": items, "positions": positions
                                    }));

                                    // Count only catalog-matched items. A capture of the
                                    // in-game relic-selection screen (shown for picking the
                                    // *next* relic, which can bleed into view before this
                                    // reward screen's EE.log dismiss line arrives) produces
                                    // mostly unmatched "?:"-prefixed fragments — raw
                                    // `items.len()` would let that noisier read masquerade
                                    // as "more complete" than an earlier, clean read.
                                    let n_confirmed =
                                        items.iter().filter(|s| !s.starts_with("?:")).count();

                                    let soft_retries_done = soft_complete_at.is_some_and(|sa| {
                                        (attempt as usize).saturating_sub(sa) >= 3
                                    });
                                    let hint_wants_more =
                                        hint_squad.is_some_and(|h| h > n_confirmed);
                                    // A "complete" set of cards can still contain a low-confidence
                                    // pick (sparse OCR read let a coincidental score through) — give
                                    // it the same few extra retries as the no-squad-hint case before
                                    // accepting whatever the best read so far was.
                                    let confirm_ready = !hint_wants_more
                                        && (hint_squad.is_some() || soft_retries_done)
                                        && (!*low_confidence || soft_retries_done);

                                    // Save best result; only emit to overlay when confirmed (LOCK).
                                    let is_new_best = n_confirmed > best_item_count;
                                    // Honest blocker: why this result is not locking yet (the
                                    // old status line blamed "EE hint" even when a low-confidence
                                    // column was the real reason).
                                    let blocker = if *complete && confirm_ready {
                                        None
                                    } else if hint_wants_more {
                                        Some(format!("hint_wants_more: hint {hint_squad:?} > {n_confirmed} confirmed"))
                                    } else if *low_confidence && !soft_retries_done {
                                        Some("low_confidence".to_string())
                                    } else if hint_squad.is_none() && !soft_retries_done {
                                        Some("hint_none".to_string())
                                    } else {
                                        Some("soft_retries_pending".to_string())
                                    };
                                    let prev_best = (!is_new_best && best_attempt > 0).then_some(
                                        reward_pipeline::PrevBest {
                                            n: best_attempt,
                                            confirmed: best_item_count,
                                        },
                                    );
                                    let event = AttemptEvent {
                                        t: ts.clone(),
                                        event: "attempt",
                                        n: attempt,
                                        mode,
                                        result: if is_new_best {
                                            "best"
                                        } else if *complete && confirm_ready {
                                            "confirm"
                                        } else {
                                            "no_improvement"
                                        },
                                        after_ms,
                                        retry_ms: Some(400),
                                        items: items.clone(),
                                        confirmed: n_confirmed,
                                        unknown: items.len().saturating_sub(n_confirmed),
                                        complete: *complete,
                                        low_confidence: *low_confidence,
                                        blocker,
                                        best: is_new_best,
                                        prev_best,
                                        catalog_expanded: None,
                                        diag: Some(diag.clone()),
                                    };
                                    reward_pipeline::log_attempt(
                                        &app,
                                        &event,
                                        diagnostic_session.as_ref(),
                                    );

                                    if is_new_best {
                                        best_item_count = n_confirmed;
                                        best_payload = payload.clone();
                                        best_attempt = attempt;
                                        // Persist the exact frame the best payload was scored
                                        // from (a reuse attempt keeps its original capture).
                                        let frame = app
                                            .state::<AppState>()
                                            .last_ocr_frame
                                            .lock()
                                            .ok()
                                            .and_then(|g| g.clone());
                                        if crate::diagnostics::ocr_pipeline_diagnostics_enabled() {
                                            if let (Some(session), Some((px, w, h))) =
                                                (diagnostic_session.as_ref(), frame)
                                            {
                                                let file_name =
                                                    format!("ocr_best_attempt_{attempt}.bmp");
                                                if crate::diagnostics::write_bmp(
                                                    &session.artifact_path(&file_name),
                                                    &px,
                                                    w,
                                                    h,
                                                )
                                                .is_ok()
                                                {
                                                    session.log(&serde_json::json!({
                                                        "t": reward_pipeline::now_ts(),
                                                        "event": "artifact",
                                                        "file": file_name,
                                                        "kind": "ocr_frame",
                                                        "attempt": attempt,
                                                    }));
                                                }
                                            }
                                        }
                                    }

                                    // Stop retrying and emit ONLY when all expected cards found AND confirmed.
                                    if *complete {
                                        if confirm_ready {
                                            // Hard cutoff: if dismiss arrived (or a newer trigger
                                            // took over) while OCR was running, drop the result.
                                            if !session_still_valid() {
                                                break;
                                            }
                                            let emit_val = if best_payload.is_some() {
                                                &best_payload
                                            } else {
                                                &payload
                                            };
                                            if let Some(session) = diagnostic_session.as_ref() {
                                                session.log(&serde_json::json!({
                                                    "t": reward_pipeline::now_ts(),
                                                    "event": "publish",
                                                    "attempt": attempt,
                                                    "best_from": best_attempt,
                                                    "reason": "all_confirmed",
                                                    "after_ms": after_ms,
                                                    "payload": emit_val,
                                                }));
                                            }
                                            reward_pipeline::publish_relic_rewards(
                                                &app,
                                                emit_val.as_ref(),
                                            );
                                            emitted_ms.store(
                                                std::time::SystemTime::now()
                                                    .duration_since(std::time::UNIX_EPOCH)
                                                    .map(|d| d.as_millis() as u64)
                                                    .unwrap_or(0),
                                                Ordering::SeqCst,
                                            );
                                            reward_pipeline::schedule_reward_diagnostic_capture(
                                                diagnostic_session.clone(),
                                                active.clone(),
                                                session_counter.clone(),
                                                my_session,
                                            );
                                            reward_pipeline::schedule_reward_safety_cleanup(
                                                app.clone(),
                                                diagnostic_session.clone(),
                                                session_counter.clone(),
                                                my_session,
                                            );
                                            break;
                                        } else {
                                            if soft_complete_at.is_none() {
                                                soft_complete_at = Some(attempt as usize);
                                                soft_complete_count = best_item_count;
                                            }
                                        }
                                    } else if soft_complete_at.is_some()
                                        && n_confirmed <= soft_complete_count
                                    {
                                        if !session_still_valid() {
                                            break;
                                        }
                                        let emit_val =
                                            best_payload.clone().unwrap_or(serde_json::Value::Null);
                                        if let Some(session) = diagnostic_session.as_ref() {
                                            session.log(&serde_json::json!({
                                                "t": reward_pipeline::now_ts(),
                                                "event": "publish",
                                                "attempt": attempt,
                                                "best_from": best_attempt,
                                                "reason": "soft_complete_no_improvement",
                                                "after_ms": after_ms,
                                                "payload": emit_val,
                                            }));
                                        }
                                        reward_pipeline::publish_relic_rewards(
                                            &app,
                                            Some(&emit_val),
                                        );
                                        reward_pipeline::schedule_reward_diagnostic_capture(
                                            diagnostic_session.clone(),
                                            active.clone(),
                                            session_counter.clone(),
                                            my_session,
                                        );
                                        reward_pipeline::schedule_reward_safety_cleanup(
                                            app.clone(),
                                            diagnostic_session.clone(),
                                            session_counter.clone(),
                                            my_session,
                                        );
                                        break;
                                    }
                                    // Partial result (or soft-complete pending confirmation) — retry
                                    400u64
                                }
                                // No items this run: dark frame, empty OCR, OCR engine error,
                                // relic-select bleed-through, or a genuine no-match. These all
                                // used to fall through to the no-match arm (the old dark/empty
                                // guards could never match their prefix), so the streak, the
                                // 3-strike catalog expansion and the 700 ms backoff apply
                                // exactly as before — only the `result` label now reports
                                // what actually happened.
                                Some((_, low_confidence, ref items, _, ref diag)) => {
                                    let result_kind = match diag.capture_kind {
                                        "dark_frame" | "ocr_empty" | "ocr_error"
                                        | "relic_select" | "unsupported" => diag.capture_kind,
                                        _ => "no_match",
                                    };
                                    let expanded = reward_pipeline::no_match_tick(
                                        &mut no_match_streak,
                                        &mut cat,
                                        &fallback_cat,
                                    );
                                    let event = AttemptEvent {
                                        t: ts.clone(),
                                        event: "attempt",
                                        n: attempt,
                                        mode,
                                        result: result_kind,
                                        after_ms,
                                        retry_ms: Some(700),
                                        items: items.clone(),
                                        confirmed: 0,
                                        unknown: items.len(),
                                        complete: false,
                                        low_confidence: *low_confidence,
                                        blocker: None,
                                        best: false,
                                        prev_best: None,
                                        catalog_expanded: Some(expanded),
                                        diag: Some(diag.clone()),
                                    };
                                    reward_pipeline::log_attempt(
                                        &app,
                                        &event,
                                        diagnostic_session.as_ref(),
                                    );
                                    700u64
                                }
                                // ⚠️ Warframe window not found
                                None => {
                                    let event = AttemptEvent {
                                        t: ts.clone(),
                                        event: "attempt",
                                        n: attempt,
                                        mode,
                                        result: "capture_failed",
                                        after_ms,
                                        retry_ms: Some(500),
                                        items: Vec::new(),
                                        confirmed: 0,
                                        unknown: 0,
                                        complete: false,
                                        low_confidence: false,
                                        blocker: None,
                                        best: false,
                                        prev_best: None,
                                        catalog_expanded: None,
                                        diag: None,
                                    };
                                    reward_pipeline::log_attempt(
                                        &app,
                                        &event,
                                        diagnostic_session.as_ref(),
                                    );
                                    500u64
                                }
                            };

                            if std::time::Instant::now() >= deadline {
                                reward_pipeline::finalize_reward_ocr_timeout(
                                    &app,
                                    best_payload,
                                    &active,
                                    diagnostic_session.clone(),
                                    session_still_valid(),
                                    &serde_json::json!({
                                        "attempt": attempt,
                                        "best_from": best_attempt,
                                        "after_ms": after_ms,
                                    }),
                                );
                                break;
                            }
                            if !session_still_valid() {
                                // `active` can go false for two different reasons, and only
                                // one of them means this task's result is still ours to show:
                                // the dismiss line already fired (reward screen closed before
                                // OCR ever reached a confirmed/complete result — common on a
                                // fast Void Flood cycle) vs. a newer trigger already took over
                                // `active` for a different session. Only the latter is truly
                                // stale; publishing then would stomp the newer session.
                                let superseded =
                                    session_counter.load(Ordering::SeqCst) != my_session;
                                if !superseded && best_payload.is_some() {
                                    if let Some(session) = diagnostic_session.as_ref() {
                                        session.log(&serde_json::json!({
                                            "t": reward_pipeline::now_ts(),
                                            "event": "publish",
                                            "attempt": attempt,
                                            "best_from": best_attempt,
                                            "reason": "dismiss_salvage",
                                            "after_ms": after_ms,
                                            "payload": best_payload.clone(),
                                        }));
                                    }
                                    reward_pipeline::publish_relic_rewards(
                                        &app,
                                        best_payload.as_ref(),
                                    );
                                    emitted_ms.store(
                                        std::time::SystemTime::now()
                                            .duration_since(std::time::UNIX_EPOCH)
                                            .map(|d| d.as_millis() as u64)
                                            .unwrap_or(0),
                                        Ordering::SeqCst,
                                    );
                                    reward_pipeline::schedule_reward_safety_cleanup(
                                        app.clone(),
                                        diagnostic_session.clone(),
                                        session_counter.clone(),
                                        my_session,
                                    );
                                } else if !superseded {
                                    // Dismissed before anything was lockable. When superseded,
                                    // a newer session owns the log file — appending here would
                                    // corrupt its events.
                                    if let Some(session) = diagnostic_session.as_ref() {
                                        session.log(&serde_json::json!({
                                            "t": reward_pipeline::now_ts(),
                                            "event": "stop",
                                            "reason": "dismissed_before_lock",
                                            "after_ms": after_ms,
                                            "attempts": attempt,
                                        }));
                                    }
                                }
                                break;
                            }
                            tokio::time::sleep(std::time::Duration::from_millis(sleep_ms)).await;
                        }
                    });
                } // end trigger block

                reward_pipeline::auto_dismiss_relic_rewards(
                    &ee_ocr_app,
                    diagnostic_session.clone(),
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
