// ── ocrs pure-Rust OCR fallback ───────────────────────────────────────────────
// Fallback for systems where Windows.Media.Ocr is unavailable (missing language
// packs, LTSC, managed enterprise images). Downloads two neural-net model files
// (~10 MB total) to the app data directory on first use.
//
// TO REMOVE THIS FALLBACK (delete in one go):
//   1. Delete this file (src-tauri/src/ocr_fallback.rs)
//   2. Remove `mod ocr_fallback;` from lib.rs
//   3. Remove the fallback block in ocr.rs (search "BEGIN ocrs fallback")
//   4. Remove the ocrs + rten lines from Cargo.toml (search "BEGIN ocrs fallback")

use std::{
    io::Read as _,
    path::PathBuf,
    sync::{Arc, Mutex, OnceLock},
};

use ocrs::{ImageSource, OcrEngine, OcrEngineParams};
use rten::Model;

const DETECTION_URL: &str =
    "https://ocrs-models.s3-accelerate.amazonaws.com/text-detection.rten";
const RECOGNITION_URL: &str =
    "https://ocrs-models.s3-accelerate.amazonaws.com/text-recognition.rten";

static DATA_DIR: OnceLock<PathBuf> = OnceLock::new();

// None  = not yet successfully initialized (retried each call).
// Some  = engine is ready.
// The OnceLock only holds the Mutex; failure is NOT cached so each call retries.
static ENGINE: OnceLock<Mutex<Option<Arc<Mutex<OcrEngine>>>>> = OnceLock::new();

/// Call once at app startup with the local-data directory path.
pub fn set_data_dir(dir: PathBuf) {
    let _ = DATA_DIR.set(dir);
}

fn models_dir() -> Option<PathBuf> {
    DATA_DIR.get().map(|d| d.join("ocr_models"))
}

fn model_paths() -> Option<(PathBuf, PathBuf)> {
    let dir = models_dir()?;
    Some((
        dir.join("text-detection.rten"),
        dir.join("text-recognition.rten"),
    ))
}

/// Atomic download: write to a `.tmp` sibling, then rename on success.
/// This prevents a partial download from being treated as a valid model file.
fn download_to(url: &str, dest: &PathBuf) -> Result<(), String> {
    let tmp = dest.with_extension("tmp");
    tracing::info!("ocr_fallback: downloading {url}");
    let resp = ureq::get(url)
        .call()
        .map_err(|e| format!("ocr_fallback: download {url}: {e}"))?;
    let mut buf = Vec::new();
    resp.into_reader()
        .read_to_end(&mut buf)
        .map_err(|e| format!("ocr_fallback: read body from {url}: {e}"))?;
    tracing::info!("ocr_fallback: downloaded {} bytes from {url}", buf.len());
    // Models are several MB; a small response means the URL returned an error page.
    if buf.len() < 1_000_000 {
        return Err(format!(
            "ocr_fallback: {url} returned only {} bytes — check network/proxy access to S3",
            buf.len()
        ));
    }
    std::fs::write(&tmp, &buf)
        .map_err(|e| format!("ocr_fallback: write {tmp:?}: {e}"))?;
    std::fs::rename(&tmp, dest)
        .map_err(|e| format!("ocr_fallback: rename to {dest:?}: {e}"))?;
    tracing::info!("ocr_fallback: saved {} bytes to {dest:?}", buf.len());
    Ok(())
}

fn ensure_models(det: &PathBuf, rec: &PathBuf) -> Result<(), String> {
    let dir = models_dir().ok_or_else(|| "ocr_fallback: DATA_DIR not set".to_string())?;
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("ocr_fallback: create_dir: {e}"))?;
    if !det.exists() {
        download_to(DETECTION_URL, det)?;
    }
    if !rec.exists() {
        download_to(RECOGNITION_URL, rec)?;
    }
    Ok(())
}

fn get_engine() -> Result<Arc<Mutex<OcrEngine>>, String> {
    let cell = ENGINE.get_or_init(|| Mutex::new(None));
    let mut guard = cell.lock().unwrap_or_else(|p| p.into_inner());

    // Already initialized in a previous call.
    if let Some(engine) = guard.as_ref() {
        return Ok(Arc::clone(engine));
    }

    let (det, rec) = model_paths()
        .ok_or_else(|| "ocr_fallback: DATA_DIR not set".to_string())?;

    ensure_models(&det, &rec)?;

    let det_model = Model::load_file(&det).map_err(|e| {
        // Corrupt download. Delete both so the next call re-downloads clean.
        let _ = std::fs::remove_file(&det);
        let _ = std::fs::remove_file(&rec);
        format!("ocr_fallback: load detection model: {e} (corrupt files deleted — will re-download)")
    })?;
    let rec_model = Model::load_file(&rec).map_err(|e| {
        let _ = std::fs::remove_file(&rec);
        format!("ocr_fallback: load recognition model: {e} (corrupt file deleted — will re-download)")
    })?;
    let engine = OcrEngine::new(OcrEngineParams {
        detection_model: Some(det_model),
        recognition_model: Some(rec_model),
        ..Default::default()
    })
    .map_err(|e| format!("ocr_fallback: engine init: {e}"))?;

    let arc = Arc::new(Mutex::new(engine));
    *guard = Some(Arc::clone(&arc));
    Ok(arc)
}

/// Decode a BMP produced by `ocr::to_bmp()` (54-byte header, BGR top-down) → RGB bytes.
fn bmp_to_rgb(bmp: &[u8], width: u32, height: u32) -> Vec<u8> {
    let row_stride = ((width * 3 + 3) & !3) as usize;
    let mut rgb = Vec::with_capacity((width * height * 3) as usize);
    for row in 0..height as usize {
        let base = 54 + row * row_stride;
        for col in 0..width as usize {
            let i = base + col * 3;
            if i + 2 < bmp.len() {
                rgb.push(bmp[i + 2]); // R  (BMP stores B, G, R)
                rgb.push(bmp[i + 1]); // G
                rgb.push(bmp[i]); //     B
            }
        }
    }
    rgb
}

/// Run ocrs on BMP bytes produced by `ocr::to_bmp()`.
///
/// Returns `(full_text, [(line_text, x_center_normalized, y_center_normalized)])`.
/// The coordinate format matches `run_windows_ocr`'s output so
/// `extract_reward_items_twophase` can consume it directly.
pub fn run_ocrs(
    bmp: &[u8],
    img_w: u32,
    img_h: u32,
) -> Result<crate::ocr::OcrResult, String> {
    let engine_arc = get_engine()?;
    let engine = engine_arc
        .lock()
        .map_err(|_| "ocr_fallback: engine mutex poisoned".to_string())?;

    let rgb = bmp_to_rgb(bmp, img_w, img_h);
    let source = ImageSource::from_bytes(&rgb, (img_w, img_h))
        .map_err(|e| format!("ocr_fallback: ImageSource: {e}"))?;
    let input = engine
        .prepare_input(source)
        .map_err(|e| format!("ocr_fallback: prepare_input: {e}"))?;

    let words = engine
        .detect_words(&input)
        .map_err(|e| format!("ocr_fallback: detect_words: {e}"))?;
    let line_groups = engine.find_text_lines(&input, &words);
    let recognized = engine
        .recognize_text(&input, &line_groups)
        .map_err(|e| format!("ocr_fallback: recognize_text: {e}"))?;

    let mut full = String::new();
    let mut lines_out: Vec<(String, f32, f32)> = Vec::new();

    for (opt_line, line_words) in recognized.iter().zip(line_groups.iter()) {
        let Some(line) = opt_line else {
            continue;
        };
        let text = line.to_string();
        if text.trim().is_empty() {
            continue;
        }
        let cx = if img_w > 0 && !line_words.is_empty() {
            let sum: f32 = line_words.iter().map(|r| r.center().x).sum();
            sum / line_words.len() as f32 / img_w as f32
        } else {
            0.5
        };
        let cy = line_words
            .first()
            .map(|r| r.center().y / img_h as f32)
            .unwrap_or(0.3);
        full.push_str(&text);
        full.push('\n');
        lines_out.push((text, cx, cy));
    }

    Ok((full, lines_out))
}
