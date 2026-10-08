//! Reward overlay lifecycle: inventory application, dismissal, and the
//! auto-dismiss / safety-cleanup timers.

use std::sync::atomic::Ordering;
use tauri::{Emitter, Manager};

use super::RewardDiagnosticSession;
use crate::app_state::AppState;
use crate::events;

/// Mutable reward-screen watch state owned by the EE.log reader loop.
pub(crate) struct DismissState<'a> {
    pub(crate) active_since: &'a mut Option<std::time::Instant>,
    pub(crate) last_dismiss_at: &'a mut Option<std::time::Instant>,
    pub(crate) session_relics: &'a mut Vec<String>,
    pub(crate) projection_state: &'a mut super::VoidProjectionState,
    pub(crate) session_counter: &'a std::sync::Arc<std::sync::atomic::AtomicU64>,
}

pub(crate) struct CloseState<'a> {
    pub(crate) reward_screen_active: &'a std::sync::Arc<std::sync::atomic::AtomicBool>,
    pub(crate) active_since: &'a mut Option<std::time::Instant>,
    pub(crate) last_dismiss_at: &'a mut Option<std::time::Instant>,
    pub(crate) rewards_emitted_ms: &'a std::sync::Arc<std::sync::atomic::AtomicU64>,
}

/// Apply the local player's EE.log reward before the next memory scan completes.
pub(crate) fn apply_reward_inventory_update(
    app: &tauri::AppHandle,
    store_path: String,
    session: Option<&std::sync::Arc<RewardDiagnosticSession>>,
) {
    let inv_path = crate::worldstate::store_to_unique(&store_path);
    let state: tauri::State<AppState> = app.state();
    let (old_qty, new_qty) = {
        let mut quantities = state
            .current_quantities
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let old = *quantities.get(&inv_path).unwrap_or(&0);
        let new = old + 1;
        quantities.insert(inv_path.clone(), new);
        (old, new)
    };
    let item_name = inv_path.split('/').next_back().unwrap_or("?").to_string();
    let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&state.changes_log_path)
    {
        use std::io::Write;
        let _ = writeln!(
            file,
            "[{}] EE.log Reward | {} | {} → {} (gets reward)",
            timestamp, item_name, old_qty, new_qty
        );
    }
    let _ = app.emit(
        events::INVENTORY_REWARD,
        serde_json::json!({ "path": inv_path, "qty": new_qty }),
    );
    let event = serde_json::json!({
        "t": super::events::now_ts(),
        "event": "inventory_reward",
        "store_path": store_path,
        "inv_path": inv_path,
        "qty": [old_qty, new_qty],
        "source": "ee_log",
    });
    if let Some(session) = session {
        session.log(&event);
    }
}

/// Handle an EE.log reward-screen dismissal and schedule the overlay cleanup.
pub(crate) fn dismiss_relic_rewards(
    app: &tauri::AppHandle,
    text: &str,
    session: Option<std::sync::Arc<RewardDiagnosticSession>>,
    reward_screen_active: &std::sync::Arc<std::sync::atomic::AtomicBool>,
    rewards_emitted_ms: &std::sync::Arc<std::sync::atomic::AtomicU64>,
    state: DismissState<'_>,
) -> bool {
    let DismissState {
        active_since,
        last_dismiss_at,
        session_relics,
        projection_state,
        session_counter,
    } = state;
    let lower = text.to_lowercase();
    let is_dismiss = lower.contains("relic reward screen shut down")
        || lower.contains("closevoidprojectionrewardscreen")
        || lower.contains("matchingservice::endsession");
    if !is_dismiss {
        return false;
    }

    let dismiss_line = text
        .lines()
        .find(|line| {
            let line = line.to_lowercase();
            line.contains("relic reward screen shut down")
                || line.contains("closevoidprojectionrewardscreen")
                || line.contains("matchingservice::endsession")
        })
        .unwrap_or("<unknown dismiss line>")
        .trim();
    let elapsed = active_since.map(|time| time.elapsed().as_secs_f64());
    let event = serde_json::json!({
        "t": super::events::now_ts(),
        "event": "dismiss",
        "reason": "ee_log",
        "line": dismiss_line,
        "open_for_s": elapsed,
    });
    if let Some(session) = &session {
        session.log(&event);
    }
    reward_screen_active.store(false, Ordering::SeqCst);
    *active_since = None;
    *last_dismiss_at = Some(std::time::Instant::now());
    if lower.contains("matchingservice::endsession") {
        session_relics.clear();
    }
    if let Some(store_path) = projection_state.take_own_item() {
        apply_reward_inventory_update(app, store_path, session.as_ref());
    }

    const MIN_DISPLAY_MS: u64 = 5_000;
    let emitted_at = rewards_emitted_ms.load(Ordering::SeqCst);
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0);
    let delay_ms = if emitted_at > 0 {
        MIN_DISPLAY_MS.saturating_sub(now_ms.saturating_sub(emitted_at))
    } else {
        0
    };
    rewards_emitted_ms.store(0, Ordering::SeqCst);

    let dismiss_app = app.clone();
    let dismiss_session = session_counter.load(Ordering::SeqCst);
    let dismiss_counter = std::sync::Arc::clone(session_counter);
    std::thread::spawn(move || {
        if delay_ms > 0 {
            std::thread::sleep(std::time::Duration::from_millis(delay_ms));
        }
        if dismiss_counter.load(Ordering::SeqCst) != dismiss_session {
            return;
        }
        if let Some(window) = dismiss_app.get_webview_window("relic-overlay") {
            let _ = window.set_position(tauri::Position::Physical(tauri::PhysicalPosition {
                x: 0,
                y: -3000,
            }));
        }
        if let Ok(mut rewards) = dismiss_app.state::<AppState>().pending_relic_rewards.lock() {
            *rewards = None;
        }
        let _ = dismiss_app.emit(events::RELIC_REWARDS, serde_json::Value::Null);
        if let Some(session) = session {
            session.finish("ee_log_dismiss");
        }
    });
    true
}

/// Dismiss a reward overlay that remained active beyond its maximum window.
pub(crate) fn auto_dismiss_relic_rewards(
    app: &tauri::AppHandle,
    session: Option<std::sync::Arc<RewardDiagnosticSession>>,
    reward_screen_active: &std::sync::Arc<std::sync::atomic::AtomicBool>,
    active_since: &mut Option<std::time::Instant>,
    last_dismiss_at: &mut Option<std::time::Instant>,
    rewards_emitted_ms: &std::sync::Arc<std::sync::atomic::AtomicU64>,
) {
    let Some(since) = *active_since else { return };
    if since.elapsed().as_secs() < 20 {
        return;
    }
    close_reward_overlay(
        app,
        session,
        CloseState {
            reward_screen_active,
            active_since,
            last_dismiss_at,
            rewards_emitted_ms,
        },
        "AUTO-DISMISS (20s timeout)",
    );
}

/// Hide the reward overlay now and clear its state. Used by the 20 s timeout and
/// when the relic-pick grid opens (the reward screen has closed by then, even if
/// its "shut down" line hasn't reached EE.log yet). Skips the 5 s minimum display.
pub(crate) fn close_reward_overlay(
    app: &tauri::AppHandle,
    session: Option<std::sync::Arc<RewardDiagnosticSession>>,
    state: CloseState<'_>,
    reason: &str,
) {
    let CloseState {
        reward_screen_active,
        active_since,
        last_dismiss_at,
        rewards_emitted_ms,
    } = state;
    let open_for = active_since.map(|since| since.elapsed().as_secs_f64());
    let event = serde_json::json!({
        "t": super::events::now_ts(),
        "event": "dismiss",
        "reason": reason,
        "open_for_s": open_for,
    });
    if let Some(session) = &session {
        session.log(&event);
        session.finish(reason);
    }
    reward_screen_active.store(false, Ordering::SeqCst);
    *active_since = None;
    *last_dismiss_at = Some(std::time::Instant::now());
    rewards_emitted_ms.store(0, Ordering::SeqCst);
    if let Some(window) = app.get_webview_window("relic-overlay") {
        let _ = window.set_position(tauri::Position::Physical(tauri::PhysicalPosition {
            x: 0,
            y: -3000,
        }));
    }
    if let Ok(mut rewards) = app.state::<AppState>().pending_relic_rewards.lock() {
        *rewards = None;
    }
    let _ = app.emit(events::RELIC_REWARDS, serde_json::Value::Null);
}

/// Schedule the final cleanup when the normal EE.log dismissal never arrives.
pub(crate) fn schedule_reward_safety_cleanup(
    app: tauri::AppHandle,
    session: Option<std::sync::Arc<RewardDiagnosticSession>>,
    session_counter: std::sync::Arc<std::sync::atomic::AtomicU64>,
    session_id: u64,
) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(20)).await;
        if session_counter.load(Ordering::SeqCst) != session_id {
            return;
        }
        if let Ok(mut rewards) = app.state::<AppState>().pending_relic_rewards.lock() {
            *rewards = None;
        }
        let _ = app.emit(events::RELIC_REWARDS, serde_json::Value::Null);
        if let Some(window) = app.get_webview_window("relic-overlay") {
            let _ = window.set_position(tauri::Position::Physical(tauri::PhysicalPosition {
                x: 0,
                y: -3000,
            }));
        }
        let event = serde_json::json!({
            "t": super::events::now_ts(),
            "event": "dismiss",
            "reason": "auto_20s_safety",
        });
        if let Some(session) = session {
            session.log(&event);
            session.finish("auto_20s_safety");
        }
    });
}

/// Store the payload for a late overlay mount, then notify the frontend.
pub(crate) fn publish_relic_rewards(app: &tauri::AppHandle, payload: Option<&serde_json::Value>) {
    if payload.is_some_and(|payload| !payload.is_null())
        && !app
            .state::<AppState>()
            .overlays_enabled
            .load(Ordering::SeqCst)
    {
        return;
    }
    if let Some(payload) = payload.filter(|payload| !payload.is_null()) {
        if let Ok(mut pending) = app.state::<AppState>().pending_relic_rewards.lock() {
            *pending = Some(payload.clone());
        }
    }
    let _ = app.emit(events::RELIC_REWARDS, payload);
}

/// Emit the best partial result after OCR timed out and hide the overlay.
///
/// `session_valid` is false when a newer trigger has already taken over the
/// shared `reward_screen_active` flag by the time this stale task's 45 s
/// deadline fires (e.g. a fast dismiss-then-retrigger). In that case every
/// action below is skipped — touching the active flag, overlay position or
/// pending payload here would stomp on the newer session instead of just
/// cleaning up this one.
///
/// `timeout_info` carries attempt context from the retry loop (`attempt`,
/// `best_from`, `after_ms`); it is merged into the `timeout` event.
pub(crate) fn finalize_reward_ocr_timeout(
    app: &tauri::AppHandle,
    best_payload: Option<serde_json::Value>,
    active: &std::sync::atomic::AtomicBool,
    session: Option<std::sync::Arc<RewardDiagnosticSession>>,
    session_valid: bool,
    timeout_info: &serde_json::Value,
) {
    let mut event = timeout_info.clone();
    let obj = event
        .as_object_mut()
        .expect("timeout_info must be an object");
    obj.insert("t".into(), serde_json::json!(super::events::now_ts()));
    obj.insert("event".into(), serde_json::json!("timeout"));
    obj.insert("stale".into(), serde_json::json!(!session_valid));
    if !session_valid {
        obj.insert(
            "reason".into(),
            serde_json::json!("superseded_by_newer_trigger"),
        );
        if let Some(session) = &session {
            session.log(&event);
            session.finish("superseded_by_newer_trigger");
        }
        return;
    }
    let emit_val = if active.load(Ordering::SeqCst) {
        best_payload.unwrap_or(serde_json::Value::Null)
    } else {
        serde_json::Value::Null
    };
    let published = emit_val
        .as_object()
        .and_then(|o| o.get("items"))
        .is_some_and(|items| !items.is_null());
    obj.insert("published".into(), serde_json::json!(published));
    obj.insert("reason".into(), serde_json::json!("deadline_45s"));
    if let Some(session) = &session {
        session.log(&event);
    }
    publish_relic_rewards(app, Some(&emit_val));
    if let Some(win) = app.get_webview_window("relic-overlay") {
        let _ = win.set_position(tauri::Position::Physical(tauri::PhysicalPosition {
            x: 0,
            y: -3000,
        }));
    }
    active.store(false, Ordering::SeqCst);
    if let Some(session) = session {
        session.finish("deadline_45s");
    }
}
