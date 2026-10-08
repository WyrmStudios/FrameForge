//! Screen capture + OCR invocation for a single reward-pipeline attempt.

use tauri::Manager;

use super::events::{self, AttemptDiag};
use super::RewardDiagnosticSession;
use crate::app_state::AppState;
use crate::diagnostics::write_bmp;

pub(crate) type RewardOcrResult = (bool, bool, Vec<String>, Vec<f32>, AttemptDiag);

/// Capture the reward area and run OCR after reading the latest EE.log hints.
///
/// `reuse_last_frame`: when true, skip the fresh `PrintWindow`/DXGI capture and
/// re-run OCR on the cached `last_ocr_frame` through `preprocess_for_ocr`
/// instead. Used for exactly one retry after a low-confidence, text-bearing
/// result — the frame already proved it has readable text, so a contrast-
/// stretched re-read of the same pixels can resolve ambiguity more cheaply
/// (and without the frame-to-frame variability of a brand-new capture) than
/// immediately grabbing another screenshot. Falls back to a fresh capture if
/// no cached frame is available yet.
pub(crate) async fn capture_reward_items(
    app: &tauri::AppHandle,
    catalog: std::sync::Arc<Vec<(String, String)>>,
    squad_hint: std::sync::Arc<std::sync::Mutex<super::SquadHint>>,
    squad_names: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    reuse_last_frame: bool,
) -> Option<RewardOcrResult> {
    let frame = std::sync::Arc::clone(&app.state::<AppState>().last_ocr_frame);
    let cached_frame = if reuse_last_frame {
        frame.lock().ok().and_then(|guard| guard.clone())
    } else {
        None
    };
    tauri::async_runtime::spawn_blocking(move || {
        let (pixels, width, capture_height, game_height, capture_info, preprocess_text_pass) =
            if let Some((cached_pixels, cached_w, cached_h)) = cached_frame {
                (
                    cached_pixels,
                    cached_w,
                    cached_h,
                    cached_h,
                    "reused last frame (preprocessed)".to_string(),
                    true,
                )
            } else {
                let (pixels, width, capture_height, game_height, capture_info) =
                    crate::ocr::capture_warframe_reward_area()?;
                // Keep the source frame for diagnostics without another GPU readback.
                if let Ok(mut cached) = frame.lock() {
                    *cached = Some((pixels.clone(), width, capture_height));
                }
                (
                    pixels,
                    width,
                    capture_height,
                    game_height,
                    capture_info,
                    false,
                )
            };
        let hint = squad_hint.lock().map(|hint| *hint).unwrap_or_default();
        let player_names = squad_names
            .lock()
            .map(|names| names.clone())
            .unwrap_or_default();
        Some(super::extract_reward_items_twophase(super::OcrParams {
            pixels: &pixels,
            pix_w: width,
            pix_h: capture_height,
            game_h: game_height,
            catalog: &catalog,
            capture_info: &capture_info,
            hint_squad_size: hint.size,
            hint_source: hint.source,
            player_names: &player_names,
            preprocess_text_pass,
        }))
    })
    .await
    .ok()
    .flatten()
}

/// Save a delayed desktop screenshot after the relic overlay has animated in.
///
/// `active`/`session_counter`/`my_session` mirror the OCR loop's own
/// `session_still_valid` check: a relic reward screen can be dismissed well
/// under the 4 s settle delay (observed as low as ~3.5 s after the overlay
/// opened), and capturing anyway produced a screenshot of whatever appeared
/// after the reward screen closed instead of the reward cards. Skipping the
/// capture once the session that scheduled it is no longer current avoids
/// saving that misleading frame.
pub(crate) fn schedule_reward_diagnostic_capture(
    session: Option<std::sync::Arc<RewardDiagnosticSession>>,
    active: std::sync::Arc<std::sync::atomic::AtomicBool>,
    session_counter: std::sync::Arc<std::sync::atomic::AtomicU64>,
    my_session: u64,
) {
    if let Some(session) = session {
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(4000)).await;
            use std::sync::atomic::Ordering;
            if !active.load(Ordering::SeqCst)
                || session_counter.load(Ordering::SeqCst) != my_session
            {
                return;
            }
            if !crate::diagnostics::ocr_pipeline_diagnostics_enabled() {
                return;
            }
            let artifact_session = session.clone();
            let written = tauri::async_runtime::spawn_blocking(move || {
                if let Some((pixels, width, height)) = crate::ocr::capture_desktop_for_diag() {
                    write_bmp(
                        &artifact_session.artifact_path("desktop_after_publish.bmp"),
                        &pixels,
                        width,
                        height,
                    )
                    .is_ok()
                } else {
                    false
                }
            })
            .await
            .unwrap_or(false);
            if written {
                // Name-and-shame the file in the event stream: desktop_after_publish.bmp is a
                // delayed *desktop* capture taken after publish (overlay included) —
                // it is not the frame the OCR scored (that's ocr_frame.bmp).
                session.log(&serde_json::json!({
                    "t": events::now_ts(),
                    "event": "artifact",
                    "file": "desktop_after_publish.bmp",
                    "kind": "desktop_after_publish",
                    "note": "+4s delayed desktop capture, includes overlay — NOT the OCR frame",
                }));
            }
        });
    }
}
