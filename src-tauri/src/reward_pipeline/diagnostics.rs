//! JSONL attempt logging for the OCR retry loop: one event line per run, the
//! moment it happens — no prose, no tree glyphs, no summaries. The retry
//! backoff values (400/500/700 ms) and the 3-attempt full-catalog expansion
//! stay in the retry loop; this module records their outcomes.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use tauri::{Emitter, Manager};

use crate::app_state::AppState;
use crate::diagnostics::write_bmp;

use super::events::AttemptEvent;

/// Immutable diagnostics owner for one reward trigger. Its mutex serializes
/// JSONL appends; no task resolves a mutable "current" diagnostics directory.
pub(crate) struct RewardDiagnosticSession {
    session_id: u64,
    directory: PathBuf,
    log_path: PathBuf,
    write_lock: Mutex<()>,
    finished: AtomicBool,
}

impl RewardDiagnosticSession {
    pub(crate) fn begin(root: &Path, session_id: u64) -> Option<std::sync::Arc<Self>> {
        if !crate::diagnostics::ocr_pipeline_diagnostics_enabled() {
            return None;
        }
        let millis = chrono::Local::now().format("%Y-%m-%d_%H-%M-%S%.3f");
        let directory = root.join(format!("{millis}-session-{session_id}"));
        if std::fs::create_dir_all(&directory).is_err() {
            return None;
        }
        Some(std::sync::Arc::new(Self {
            session_id,
            log_path: directory.join("ocr_session_log.jsonl"),
            directory,
            write_lock: Mutex::new(()),
            finished: AtomicBool::new(false),
        }))
    }

    pub(crate) fn log_path(&self) -> &Path {
        &self.log_path
    }
    pub(crate) fn artifact_path(&self, name: &str) -> PathBuf {
        self.directory.join(name)
    }

    pub(crate) fn log(&self, event: &impl serde::Serialize) {
        if !crate::diagnostics::ocr_pipeline_diagnostics_enabled()
            || self.finished.load(Ordering::SeqCst)
        {
            return;
        }
        let Ok(mut value) = serde_json::to_value(event) else {
            return;
        };
        if let Some(object) = value.as_object_mut() {
            object.insert("session_id".into(), serde_json::json!(self.session_id));
        }
        let Ok(line) = serde_json::to_string(&value) else {
            return;
        };
        let Ok(_guard) = self.write_lock.lock() else {
            return;
        };
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log_path)
        {
            let _ = writeln!(file, "{line}");
        }
    }

    pub(crate) fn finish(&self, reason: &str) {
        if self.finished.swap(true, Ordering::SeqCst)
            || !crate::diagnostics::ocr_pipeline_diagnostics_enabled()
        {
            return;
        }
        let event = serde_json::json!({ "t": super::events::now_ts(), "event": "finish", "reason": reason });
        let Ok(mut value) = serde_json::to_value(event) else {
            return;
        };
        value
            .as_object_mut()
            .expect("object event")
            .insert("session_id".into(), serde_json::json!(self.session_id));
        let Ok(line) = serde_json::to_string(&value) else {
            return;
        };
        let Ok(_guard) = self.write_lock.lock() else {
            return;
        };
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log_path)
        {
            let _ = writeln!(file, "{line}");
        }
    }
}

/// Append one attempt event to the session log (JSONL) and mirror the same
/// line to the last-result file, then surface the failure kind to the UI.
pub(crate) fn log_attempt(
    app: &tauri::AppHandle,
    event: &AttemptEvent,
    session: Option<&std::sync::Arc<RewardDiagnosticSession>>,
) {
    let diagnostics_enabled = crate::diagnostics::ocr_pipeline_diagnostics_enabled();
    if let Some(session) = session {
        session.log(event);
    }

    let status = match event.result {
        "dark_frame" => Some("⬛ Dark frame (PrintWindow) — retrying".to_string()),
        "ocr_empty" => Some("⬜ OCR found no text — retrying".to_string()),
        "ocr_error" => Some("⚠️ OCR engine error — retrying".to_string()),
        "capture_failed" => Some("⚠️ Capture failed".to_string()),
        "no_match" => Some("❌ No catalog match, retrying...".to_string()),
        _ => None,
    };
    if let Some(status) = status {
        let _ = app.emit(crate::events::FF_STATUS, status);
    }

    // Keep the first failed OCR frame so its filename states exactly why it was
    // recorded; later desktop captures cannot overwrite it.
    if diagnostics_enabled && event.n == 1 {
        let Some(file_name) = (match event.result {
            "no_match" => Some("ocr_no_match_attempt_1.bmp"),
            "dark_frame" => Some("ocr_dark_frame_attempt_1.bmp"),
            "ocr_empty" => Some("ocr_empty_attempt_1.bmp"),
            "ocr_error" => Some("ocr_error_attempt_1.bmp"),
            _ => None,
        }) else {
            return;
        };
        let frame = app
            .state::<AppState>()
            .last_ocr_frame
            .lock()
            .ok()
            .and_then(|g| g.clone());
        if let (Some((px, w, h)), Some(session)) = (frame, session) {
            if write_bmp(&session.artifact_path(file_name), &px, w, h).is_ok() {
                session.log(&serde_json::json!({ "t": super::events::now_ts(), "event": "artifact", "file": file_name }));
            }
        }
    }
}

/// Advance the no-match streak; on the 3rd consecutive strike the catalog
/// expands to the full item list. Returns true when this tick expanded it, so
/// the caller can record `catalog_expanded` on the attempt event.
pub(crate) fn no_match_tick(
    streak: &mut u32,
    cat: &mut std::sync::Arc<Vec<(String, String)>>,
    fallback_cat: &std::sync::Arc<Vec<(String, String)>>,
) -> bool {
    *streak += 1;
    if *streak == 3 && cat.len() < fallback_cat.len() {
        *cat = std::sync::Arc::clone(fallback_cat);
        true
    } else {
        false
    }
}
