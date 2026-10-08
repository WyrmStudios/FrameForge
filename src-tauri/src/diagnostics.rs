use std::sync::atomic::{AtomicBool, Ordering};
use tauri::State;
use tracing::{info, warn};

use crate::app_state::AppState;

type MemoryPattern = (&'static str, &'static [u8], usize, usize, u64, u64);

static OCR_PIPELINE_DIAGNOSTICS_ENABLED: AtomicBool = AtomicBool::new(false);

pub(crate) fn ocr_pipeline_diagnostics_enabled() -> bool {
    OCR_PIPELINE_DIAGNOSTICS_ENABLED.load(Ordering::Relaxed)
}

pub(crate) fn set_ocr_pipeline_diagnostics_enabled(enabled: bool) {
    OCR_PIPELINE_DIAGNOSTICS_ENABLED.store(enabled, Ordering::Relaxed);
}

#[tauri::command]
pub(crate) fn set_ocr_pipeline_diagnostics(enabled: bool) {
    set_ocr_pipeline_diagnostics_enabled(enabled);
}

#[tauri::command]
pub(crate) fn get_app_version(app: tauri::AppHandle) -> String {
    // Use the Tauri runtime version — same source the updater plugin uses for comparison.
    // Falls back to CARGO_PKG_VERSION in dev mode where package_info may not be set.
    let runtime = app.package_info().version.to_string();
    if !runtime.is_empty() {
        return runtime;
    }
    env!("CARGO_PKG_VERSION").to_string()
}

#[tauri::command]
pub(crate) fn set_app_version(version: String) -> Result<(), String> {
    let tauri_conf = std::path::Path::new("src-tauri/tauri.conf.json");
    let package_json = std::path::Path::new("package.json");
    let cargo_toml = std::path::Path::new("src-tauri/Cargo.toml");
    if tauri_conf.exists() {
        update_version_in_file(tauri_conf, &version)?;
    }
    if package_json.exists() {
        update_version_in_file(package_json, &version)?;
    }
    if cargo_toml.exists() {
        update_cargo_toml_version(cargo_toml, &version)?;
    }
    Ok(())
}

fn update_version_in_file(path: &std::path::Path, version: &str) -> Result<(), String> {
    let content = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    // Replace first occurrence of "version": "x.y.z"
    let marker = "\"version\": \"";
    if let Some(start) = content.find(marker) {
        let after = start + marker.len();
        if let Some(end) = content[after..].find('"') {
            let mut updated = content.clone();
            updated.replace_range(after..after + end, version);
            std::fs::write(path, updated).map_err(|e| e.to_string())?;
            return Ok(());
        }
    }
    Err(format!("Version field not found in {}", path.display()))
}

fn update_cargo_toml_version(path: &std::path::Path, version: &str) -> Result<(), String> {
    let content = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    // Find [package] section then replace the first `version = "..."` line after it.
    // The newline prefix avoids matching version keys inside dependency inline tables.
    let pkg_marker = "[package]";
    let ver_marker = "\nversion = \"";
    if let Some(pkg_start) = content.find(pkg_marker) {
        let after_pkg = pkg_start + pkg_marker.len();
        if let Some(rel_start) = content[after_pkg..].find(ver_marker) {
            let abs_start = after_pkg + rel_start + ver_marker.len();
            if let Some(end) = content[abs_start..].find('"') {
                let mut updated = content.clone();
                updated.replace_range(abs_start..abs_start + end, version);
                std::fs::write(path, updated).map_err(|e| e.to_string())?;
                return Ok(());
            }
        }
    }
    Err(format!("Version field not found in {}", path.display()))
}

#[tauri::command]
pub(crate) fn read_scan_log(state: State<AppState>) -> Result<String, String> {
    std::fs::read_to_string(&state.log_path).map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) async fn dump_memory_probe(state: State<'_, AppState>) -> Result<String, String> {
    let log_path = state.memory_probe_path.clone();
    let lines = tokio::task::spawn_blocking(|| crate::memory_scanner::dump_inventory_regions(40))
        .await
        .map_err(|e| e.to_string())?;
    let output = lines.join("\n");
    std::fs::write(&log_path, &output).map_err(|e| e.to_string())?;
    Ok(output)
}

/// Enable or disable automatic per-pass inventory blob logging to blobs/.
#[tauri::command]
pub(crate) fn set_blob_log(enabled: bool, state: State<'_, AppState>) {
    state.blob_log_enabled.store(enabled, Ordering::SeqCst);
}

/// Enable or disable logging of raw DE API responses to api_logs/.
#[tauri::command]
pub(crate) fn set_api_log(enabled: bool, state: State<'_, AppState>) {
    state.api_log_enabled.store(enabled, Ordering::SeqCst);
}

/// Returns "started" or "stopped" so the frontend can update button state.
#[tauri::command]
pub(crate) async fn toggle_raw_scan(state: State<'_, AppState>) -> Result<String, String> {
    let was_active = state.raw_scan_active.swap(true, Ordering::SeqCst);
    if was_active {
        // Already running — stop it
        state.raw_scan_active.store(false, Ordering::SeqCst);
        return Ok("stopped".to_string());
    }

    // Freshly started — truncate the output file and spawn the loop
    let out_path = state.raw_scan_path.clone();
    let flag = state.raw_scan_active.clone();

    // Truncate / create the file now so the frontend can see it immediately
    std::fs::write(&out_path, "").map_err(|e| e.to_string())?;

    std::thread::spawn(move || {
        let mut pass = 0u32;
        while flag.load(Ordering::SeqCst) {
            pass += 1;
            let ts = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
            let header = format!("\n=== Pass {} at {} ===\n", pass, ts);

            // Open for append each pass so file grows in real time
            match std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&out_path)
            {
                Ok(mut f) => {
                    use std::io::Write;
                    let _ = f.write_all(header.as_bytes());
                    match crate::memory_scanner::raw_scan_pass(&mut f) {
                        Ok(n) => {
                            let _ = writeln!(f, "--- pass {} done: {} strings ---", pass, n);
                        }
                        Err(e) => {
                            let _ = writeln!(f, "--- pass {} error: {} ---", pass, e);
                        }
                    }
                }
                Err(e) => {
                    warn!(error = %e, "raw_scan open failed");
                }
            }

            // Sleep between passes so the user has time to navigate menus
            for _ in 0..50 {
                if !flag.load(Ordering::SeqCst) {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        }
    });

    Ok("started".to_string())
}

#[tauri::command]
pub(crate) fn clear_cache(state: State<AppState>) -> Result<(), String> {
    // Clear change log from DB
    let conn = state.conn.lock().map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM quantity_changes", [])
        .map_err(|e| e.to_string())?;
    drop(conn);

    // Reset all in-memory inventory state
    state
        .current_quantities
        .lock()
        .map_err(|e| e.to_string())?
        .clear();
    state
        .unique_quantities
        .lock()
        .map_err(|e| e.to_string())?
        .clear();
    state
        .current_mods
        .lock()
        .map_err(|e| e.to_string())?
        .clear();
    state
        .api_quantities_cache
        .lock()
        .map_err(|e| e.to_string())?
        .clear();
    state
        .api_mod_copies_cache
        .lock()
        .map_err(|e| e.to_string())?
        .clear();

    // Delete cache and hint files so nothing reloads on next start
    let _ = std::fs::remove_file(&state.quantities_cache_path);
    let _ = std::fs::remove_file(&state.inventory_state_cache_path);
    let _ = std::fs::remove_file(
        state
            .inventory_state_cache_path
            .with_file_name("section_baseline.json"),
    );
    let _ = std::fs::remove_file(state.log_path.with_file_name("inventory_hints.json"));
    let _ = std::fs::remove_file(state.log_path.with_file_name("mod_hints.json"));

    Ok(())
}

/// Read the riven overlay session log.
#[tauri::command]
pub(crate) fn get_riven_session_log() -> String {
    let path = std::env::temp_dir().join("frameforge_riven_session.txt");
    std::fs::read_to_string(&path).unwrap_or_else(|_| {
        "(no riven session log yet — open the riven reroll screen first)".into()
    })
}

/// Read the current overlay session log.
#[tauri::command]
pub(crate) fn get_overlay_session_log(state: State<AppState>) -> String {
    let session = state
        .reward_diagnostic_session
        .lock()
        .ok()
        .and_then(|session| session.clone());
    session
        .and_then(|session| std::fs::read_to_string(session.log_path()).ok())
        .unwrap_or_else(|| "(no session log yet — trigger a Void Fissure first)".into())
}

/// Frontend tracing — App.tsx and Overlay.tsx call this to write diagnostic
/// lines into the same session log that gets copied to the diagnostics folder.
/// Logged as `{"event":"fe"}` JSONL so the log stays one consistent stream.
#[tauri::command]
pub(crate) fn log_relic_fe(msg: String, state: State<AppState>) {
    let session = state
        .reward_diagnostic_session
        .lock()
        .ok()
        .and_then(|session| session.clone());
    let Some(session) = session else { return };
    let event = serde_json::json!({
        "t": chrono::Local::now().format("%H:%M:%S%.3f").to_string(),
        "event": "fe",
        "msg": msg,
    });
    session.log(&event);
}

/// Force-set the relic-overlay window to HWND_TOPMOST via SetWindowPos.
/// Called from JS on a 150 ms interval while the overlay is visible, to beat
/// Warframe's continuous HWND_TOPMOST reassertion.
#[tauri::command]
pub(crate) fn set_overlay_topmost() {
    use crate::platform::{Platform, WindowManager};
    Platform::set_overlay_topmost();
}

/// Diagnostic: position a test window ON TOP OF WARFRAME (finds Warframe's HWND
/// to guarantee the correct monitor) and inject a full-screen coloured div via
/// evaluate_script — bypasses IPC and React entirely (Rust → WebView2 direct).
/// Creates the window from Rust if the pre-declared one doesn't exist.
/// Red = WebView renders, IPC broken. Green = WebView renders, IPC ok.
/// Nothing at all = window creation failed or WebView not rendering.
#[tauri::command]
pub(crate) fn inject_overlay_diagnostic(app: tauri::AppHandle) -> String {
    use tauri::{Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindowBuilder};

    // Find Warframe's client area to anchor the diagnostic window to the right monitor.
    use crate::platform::{Platform, WindowManager};
    let rect = Platform::get_warframe_window_rect().unwrap_or([0, 0, 1920, 1080]);
    let (wf_x, wf_y, wf_w, wf_h) = (rect[0], rect[1], rect[2], rect[3]);

    // Place diagnostic at the vertical centre of the Warframe client area, full width.
    let diag_x = wf_x;
    let diag_y = wf_y + wf_h / 2 - 150;
    let diag_w = wf_w.max(400) as u32;
    let diag_h = 300u32;

    let win = match app.get_webview_window("relic-overlay") {
        Some(w) => w,
        None => {
            // Pre-declared window missing — create a fresh one from Rust.
            match WebviewWindowBuilder::new(
                &app,
                "relic-overlay",
                WebviewUrl::App("index.html#overlay".into()),
            )
            .title("FrameForge Overlay")
            .position(diag_x as f64, diag_y as f64)
            .inner_size(diag_w as f64, diag_h as f64)
            .transparent(true)
            .decorations(false)
            .always_on_top(true)
            .skip_taskbar(true)
            .resizable(false)
            .focused(false)
            .build()
            {
                Ok(w) => w,
                Err(e) => return format!("create-err:{e}"),
            }
        }
    };

    let _ = win.set_position(tauri::Position::Physical(PhysicalPosition {
        x: diag_x,
        y: diag_y,
    }));
    let _ = win.set_size(tauri::Size::Physical(PhysicalSize {
        width: diag_w,
        height: diag_h,
    }));
    let _ = win.set_always_on_top(true);
    let _ = win.show();

    // Give WebView2 a moment to paint before we also eval.
    std::thread::sleep(std::time::Duration::from_millis(200));

    let script = r#"
        (function() {
            document.documentElement.style.cssText = 'margin:0;padding:0;width:100%;height:100%;';
            document.body.style.cssText = 'margin:0;padding:0;background:rgba(200,0,0,0.95);color:#fff;font-family:sans-serif;font-size:26px;font-weight:bold;display:flex;align-items:center;justify-content:center;height:100vh;box-sizing:border-box;';
            document.body.innerHTML = '<span>FF WEBVIEW ALIVE — IPC test pending...</span>';
            try {
                window.__TAURI_INTERNALS__.invoke('log_relic_fe', {msg:'[OV] inject_diagnostic IPC ok'});
                document.body.style.background = 'rgba(0,160,0,0.95)';
                document.body.innerHTML = '<span>FF WEBVIEW — IPC OK (you should see this in green)</span>';
            } catch(e) {
                document.body.innerHTML = '<span>FF WEBVIEW — NO IPC: ' + String(e).slice(0,100) + '</span>';
            }
        })();
    "#;
    match win.eval(script) {
        Ok(_) => format!("eval-ok wf=({wf_x},{wf_y},{wf_w},{wf_h}) diag=({diag_x},{diag_y})"),
        Err(e) => format!("eval-err:{e}"),
    }
}

/// Debug helper: create a test window from Rust side to verify whether JS-side
/// WebviewWindow creation is broken. Returns Ok("created") or Err(reason).
/// Uses a URL hash (#modular) so the Tauri asset protocol serves clean index.html
/// Toggle debug categorization mode. Returns the new state (true = enabled).
#[tauri::command]
pub(crate) fn toggle_debug_categorization(state: State<AppState>) -> bool {
    let prev = state.debug_cat_enabled.fetch_xor(true, Ordering::SeqCst);
    let enabled = !prev;
    info!(debug_cat = enabled, "debug categorization toggled");
    enabled
}

/// and the Tauri init script is injected properly — query strings prevent this.
#[tauri::command]
pub(crate) fn debug_create_window(app: tauri::AppHandle) -> Result<String, String> {
    use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};
    if let Some(existing) = app.get_webview_window("relic-overlay-solid") {
        let _ = existing.close();
        std::thread::sleep(std::time::Duration::from_millis(150));
    }
    WebviewWindowBuilder::new(
        &app,
        "relic-overlay-solid",
        WebviewUrl::App("index.html#modular".into()),
    )
    .title("FF Debug Window — look in taskbar!")
    .inner_size(800.0, 500.0)
    .position(200.0, 200.0)
    .transparent(false)
    .decorations(true)
    .always_on_top(false)
    .skip_taskbar(false)
    .resizable(true)
    .focused(true)
    .build()
    .map(|_| "created".to_string())
    .map_err(|e| format!("build() failed: {e}"))
}

fn dir_size_bytes(dir: &std::path::Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .filter_map(|e| e.ok())
        .map(|e| {
            let p = e.path();
            if p.is_dir() {
                dir_size_bytes(&p)
            } else {
                std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0)
            }
        })
        .sum()
}

fn reward_diagnostic_temp_paths() -> [std::path::PathBuf; 2] {
    let temp_dir = std::env::temp_dir();
    [
        temp_dir.join("frameforge_capture_debug.bmp"),
        temp_dir.join("frameforge_last_reward.txt"),
    ]
}

/// Return the total size of relic-reward diagnostics, including legacy temp files.
#[tauri::command]
pub(crate) fn get_diag_folder_size(state: State<AppState>) -> u64 {
    dir_size_bytes(&state.auto_capture_dir)
        + reward_diagnostic_temp_paths()
            .iter()
            .map(|path| {
                std::fs::metadata(path)
                    .map(|metadata| metadata.len())
                    .unwrap_or(0)
            })
            .sum::<u64>()
}

/// Delete all relic-reward diagnostics, including legacy temp files.
/// Returns the size after deletion (always 0 on success).
#[tauri::command]
pub(crate) fn clear_diag_folder(state: State<AppState>) -> u64 {
    let dir = state.auto_capture_dir.clone();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.filter_map(|e| e.ok()) {
            let p = entry.path();
            if p.is_dir() {
                let _ = std::fs::remove_dir_all(&p);
            } else {
                let _ = std::fs::remove_file(&p);
            }
        }
    }
    for path in reward_diagnostic_temp_paths() {
        let _ = std::fs::remove_file(path);
    }
    if let Ok(mut session) = state.reward_diagnostic_session.lock() {
        *session = None;
    }
    0
}

#[tauri::command]
pub(crate) fn open_debug_folder(state: State<AppState>, which: String) -> Result<(), String> {
    let path: std::path::PathBuf = match which.as_str() {
        "blobs" => state.blob_log_dir.clone(),
        "api_logs" => state.api_log_dir.clone(),
        "raw_scan" => state
            .raw_scan_path
            .parent()
            .ok_or("no parent")?
            .to_path_buf(),
        "probe" => state
            .memory_probe_path
            .parent()
            .ok_or("no parent")?
            .to_path_buf(),
        "diag" => state.auto_capture_dir.clone(),
        "manual_capture" => state.manual_capture_dir.clone(),
        "unmatched_paths" => state.unmatched_paths_dir.clone(),
        _ => return Err("Unknown debug folder".into()),
    };
    std::fs::create_dir_all(&path).ok();
    tauri_plugin_opener::open_path(&path, None::<&str>).map_err(|e| e.to_string())?;
    Ok(())
}

/// Clear debug data for a specific category.
/// `which`: "blobs" | "api_logs" | "raw_scan" | "probe"
#[tauri::command]
pub(crate) fn clear_debug_data(state: State<AppState>, which: String) -> Result<(), String> {
    let clear_dir = |dir: &std::path::Path| {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for e in entries.filter_map(|e| e.ok()) {
                let _ = std::fs::remove_file(e.path());
            }
        }
    };
    match which.as_str() {
        "blobs" => clear_dir(&state.blob_log_dir),
        "api_logs" => clear_dir(&state.api_log_dir),
        "raw_scan" => {
            let _ = std::fs::remove_file(&state.raw_scan_path);
        }
        "probe" => {
            let _ = std::fs::remove_file(&state.memory_probe_path);
        }
        "unmatched_paths" => clear_dir(&state.unmatched_paths_dir),
        "manual_capture" => {
            if let Ok(entries) = std::fs::read_dir(&state.manual_capture_dir) {
                for e in entries.filter_map(|e| e.ok()) {
                    let p = e.path();
                    if p.is_dir() {
                        let _ = std::fs::remove_dir_all(&p);
                    } else {
                        let _ = std::fs::remove_file(&p);
                    }
                }
            }
        }
        _ => return Err("Unknown debug data type".into()),
    }
    Ok(())
}

/// Return the byte size of a debug folder or file.
/// `which`: "blobs" | "api_logs" | "raw_scan" | "probe" | "diag" | "manual_capture" | "unmatched_paths"
#[tauri::command]
pub(crate) fn get_debug_data_size(state: State<AppState>, which: String) -> u64 {
    match which.as_str() {
        "blobs" => dir_size_bytes(&state.blob_log_dir),
        "api_logs" => dir_size_bytes(&state.api_log_dir),
        "raw_scan" => std::fs::metadata(&state.raw_scan_path)
            .map(|m| m.len())
            .unwrap_or(0),
        "probe" => std::fs::metadata(&state.memory_probe_path)
            .map(|m| m.len())
            .unwrap_or(0),
        "diag" => dir_size_bytes(&state.auto_capture_dir),
        "manual_capture" => dir_size_bytes(&state.manual_capture_dir),
        "unmatched_paths" => dir_size_bytes(&state.unmatched_paths_dir),
        _ => 0,
    }
}

/// Write BGRA pixels as an uncompressed 24-bit BGR BMP file.
/// BMP is lossless and writes in microseconds regardless of resolution —
/// PNG compression at 2560×1440 blocks for 1–3 s and froze the overlay.
/// 24-bit BGR (BI_RGB) uses a standard 54-byte header with no colour masks,
/// opening correctly in every image viewer.
pub(crate) fn write_bmp(
    path: &std::path::Path,
    bgra: &[u8],
    w: u32,
    h: u32,
) -> std::io::Result<()> {
    use std::io::Write;
    // 24-bit BGR rows must be padded to a 4-byte boundary.
    let row_bytes = (w as usize) * 3;
    let padding = (4 - (row_bytes % 4)) % 4;
    let padded_row = row_bytes + padding;
    let pixel_data_size = padded_row * h as usize;
    let file_size = 54usize + pixel_data_size;
    let mut f = std::io::BufWriter::new(std::fs::File::create(path)?);
    // BMP file header (14 bytes)
    f.write_all(b"BM")?;
    f.write_all(&(file_size as u32).to_le_bytes())?;
    f.write_all(&[0u8; 4])?; // reserved
    f.write_all(&54u32.to_le_bytes())?; // pixel data starts immediately after 54-byte header
                                        // BITMAPINFOHEADER (40 bytes)
    f.write_all(&40u32.to_le_bytes())?;
    f.write_all(&w.to_le_bytes())?;
    f.write_all(&(h as i32).wrapping_neg().to_le_bytes())?; // negative height = top-down
    f.write_all(&1u16.to_le_bytes())?; // colour planes
    f.write_all(&24u16.to_le_bytes())?; // bits per pixel
    f.write_all(&0u32.to_le_bytes())?; // BI_RGB — no compression, no extra masks
    f.write_all(&(pixel_data_size as u32).to_le_bytes())?;
    f.write_all(&[0u8; 16])?; // XPelsPerMeter, YPelsPerMeter, ClrUsed, ClrImportant
                              // Pixel data: drop alpha channel (BGRA → BGR), pad each row to 4-byte boundary.
    let pad = [0u8; 4];
    for row in bgra.chunks_exact(w as usize * 4) {
        for px in row.chunks_exact(4) {
            f.write_all(&px[..3])?; // B, G, R
        }
        if padding > 0 {
            f.write_all(&pad[..padding])?;
        }
    }
    Ok(())
}

#[tauri::command]
pub(crate) async fn capture_diagnostics(state: State<'_, AppState>) -> Result<String, String> {
    let log_path = state.log_path.clone();
    let changes_path = state.changes_log_path.clone();
    let manual_capture_dir = state.manual_capture_dir.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let ts = chrono::Local::now().format("%Y-%m-%d_%H-%M-%S").to_string();
        let folder = manual_capture_dir.join(&ts);
        std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;

        if log_path.exists() {
            let _ = std::fs::copy(&log_path, folder.join("scan_log.txt"));
        }
        if changes_path.exists() {
            let _ = std::fs::copy(&changes_path, folder.join("changes_log.txt"));
        }

        // Half-resolution capture: StretchBlt destination is 4× smaller, so GetDIBits
        // reads 4× less data — significantly reduces GPU stall time.
        match crate::ocr::capture_screen_for_diagnostics_half() {
            Ok((pixels_bgra, w, h)) => {
                let _ = write_bmp(&folder.join("screenshot.bmp"), &pixels_bgra, w, h);
            }
            Err(e) => {
                let _ = std::fs::write(folder.join("screenshot_error.txt"), &e);
            }
        }

        Ok(folder.to_string_lossy().into_owned())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Returns the Warframe game CLIENT AREA as [x, y, width, height] in screen pixels.
/// Uses GetClientRect + ClientToScreen so the rect matches what the OCR captures —
/// both exclude the window title bar and borders in windowed mode.
#[tauri::command]
pub(crate) fn get_warframe_window_rect() -> Result<[i32; 4], String> {
    use crate::platform::{Platform, WindowManager};
    Platform::get_warframe_window_rect()
}

// ── Memory Relic Debug ─────────────────────────────────────────────────────
//
// Completely independent of the EE.log + OCR flow.  Tails EE.log for all raw
// lines AND scans Warframe's process memory for relic-reward-related patterns,
// logging everything to a single file.  Purpose: determine whether memory
// holds the reward choices so we can replace or supplement OCR.

static MEM_RELIC_DEBUG_RUNNING: AtomicBool = AtomicBool::new(false);

#[tauri::command]
pub(crate) fn start_memory_relic_debug() -> Result<String, String> {
    if MEM_RELIC_DEBUG_RUNNING.swap(true, Ordering::SeqCst) {
        return Err("Memory relic debug already running".to_string());
    }
    let log_path = std::env::temp_dir().join("frameforge_mem_relic_debug.log");
    let log_str = log_path.to_string_lossy().to_string();
    let header = format!(
        "══════════════════════════════════════════════════\n\
         MEMORY RELIC DEBUG — {}\n\
         Log: {}\n\
         ══════════════════════════════════════════════════\n\n\
         Patterns searched in memory:\n\
           \"HasFissureum\":true           — SLOW: squad member in fissure (JSON-exact, live blob only)\n\
           \"VoidProjection\":{{\"ItemType\":\"  — SLOW: squad member's relic (JSON-exact, live blob only)\n\
           VoidProjection               — SLOW: broad scan, 256-byte context; shows all types in memory\n\
           gets reward                  — FAST+ONE-SHOT: in-memory log buffer [playerID gets reward path]\n\
           Host has reward info for all — FAST+ONE-SHOT: fires when all players have responded\n\
           [RW] /Lotus/ in small heap   — REWARD WINDOW: runs every 2s while reward screen is open;\n\
                                          scans heap 0x0001_0000_0000..0x0300_0000_0000, regions <128KB\n\n",
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
        log_str,
    );
    std::fs::write(&log_path, header.as_bytes()).map_err(|e| e.to_string())?;
    let lp = log_path.clone();
    std::thread::spawn(move || {
        mem_relic_debug_loop(&lp);
        MEM_RELIC_DEBUG_RUNNING.store(false, Ordering::SeqCst);
    });
    Ok(log_str)
}

#[tauri::command]
pub(crate) fn stop_memory_relic_debug() {
    MEM_RELIC_DEBUG_RUNNING.store(false, Ordering::SeqCst);
}

#[cfg(target_os = "windows")]
fn mem_relic_debug_loop(log_path: &std::path::Path) {
    use crate::platform::{Platform, ProcessAccess};
    use std::collections::HashMap;
    use std::io::{Read, Seek, SeekFrom};

    // Pattern tuple: (name, bytes, ctx_before, ctx_after, min_addr, max_region_size)
    // max_region_size=0 means no limit. Use a small cap (e.g. 4 MB) for heap-heap patterns
    // to avoid a full memory walk on the fast tick.
    //
    // Slow patterns: full memory walk every cycle (~55 s). min_addr=0 → all regions.
    // Keep this list SHORT — each pattern adds ~55 s to the cycle time.
    const PATTERNS_SLOW: &[MemoryPattern] = &[
        ("HasFissureum", b"\"HasFissureum\":true", 128, 1024, 0, 0),
        (
            "VoidProjection.relic",
            b"\"VoidProjection\":{\"ItemType\":\"",
            128,
            1024,
            0,
            0,
        ),
        ("VoidProjection.raw", b"VoidProjection", 16, 256, 0, 0),
        // Lua event name found in heap. 512 bytes after = see if item paths land nearby.
        (
            "RewardVoidProjection",
            b"RewardVoidProjection",
            32,
            512,
            0,
            0,
        ),
    ];

    // One-shot patterns: scanned immediately when EE.log emits the reward-screen trigger.
    // Goal: snapshot exactly what text is in heap memory at that precise moment.
    // min_addr=0, max_region_size=0 → all committed readable regions.
    const PATTERNS_ONESHOT: &[MemoryPattern] = &[
        (
            "os.open_trigger",
            b"VoidProjections: GetVoidProjectionReward",
            96,
            256,
            0,
            0,
        ),
        ("os.gets_reward", b"gets reward /Lotus/", 96, 256, 0, 0),
        (
            "os.all_rewards",
            b"Host has reward info for all players now",
            64,
            256,
            0,
            0,
        ),
        (
            "os.close_trigger",
            b"Relic reward screen shut down",
            96,
            256,
            0,
            0,
        ),
        (
            "os.close_rmi",
            b"CloseVoidProjectionRewardScreen",
            64,
            256,
            0,
            0,
        ),
    ];

    // Fast patterns: two tiers.
    // Tier A (min_addr=LOG_BUF_MIN): game binary only — scans in milliseconds every tick.
    // Tier B (max_region_size=HEAP_SMALL): small heap regions only — still fast (<200 ms).
    // Live ring-buffer entries end with \r\n; static format strings end with \n\0.
    const LOG_BUF_MIN: u64 = 0x0000_7f00_0000_0000; // binary/DLL range
    const HEAP_SMALL: u64 = 4 * 1024 * 1024; // skip large heap allocs (JSON blobs etc.)
    const PATTERNS_FAST: &[MemoryPattern] = &[
        // Tier A — binary range: format strings and any ring-buffer copy there
        (
            "bin.gets_reward",
            b"gets reward /Lotus/",
            96,
            256,
            LOG_BUF_MIN,
            0,
        ),
        (
            "bin.all_rewards_in",
            b"Host has reward info for all players now!\r",
            0,
            64,
            LOG_BUF_MIN,
            0,
        ),
        // Tier B — small heap regions: look for live EE.log ring-buffer text
        (
            "heap.open_trigger",
            b"VoidProjections: GetVoidProjectionReward",
            96,
            256,
            0,
            HEAP_SMALL,
        ),
        (
            "heap.close_trigger",
            b"Relic reward screen shut down",
            96,
            256,
            0,
            HEAP_SMALL,
        ),
        (
            "heap.gets_reward",
            b"gets reward /Lotus/",
            96,
            256,
            0,
            HEAP_SMALL,
        ),
        (
            "heap.all_rewards",
            b"Host has reward info for all players now",
            64,
            256,
            0,
            HEAP_SMALL,
        ),
    ];

    fn append(path: &std::path::Path, s: &str) {
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            let _ = f.write_all(s.as_bytes());
        }
    }

    // Targeted fast scan: /Lotus/ in small heap regions only.
    // Heap addresses observed empirically: 0x0000_0150_xxxx to 0x0000_0155_xxxx.
    // We use a wider window (0x0001 to 0x0300) to be safe.
    // Skips regions > 128 KB to avoid the FULL_ACCOUNT blob and other large allocs.
    // Returns (region_base, match_addr, context_bytes).
    fn scan_lotus_in_heap(pid: u32) -> Vec<(u64, u64, Vec<u8>)> {
        const HEAP_MIN: usize = 0x0000_0001_0000_0000;
        const HEAP_MAX: usize = 0x0000_0300_0000_0000;
        const REGION_MAX: usize = 128 * 1024;
        const CTX: usize = 192;
        const STRIDE: usize = 256; // skip forward after each match (de-noise)
        let pat: &[u8] = b"/Lotus/";
        let mut results = Vec::new();

        let handle = match Platform::open_process(pid) {
            Ok(h) => h,
            Err(_) => return results,
        };

        let mut addr = HEAP_MIN;
        loop {
            if addr >= HEAP_MAX {
                break;
            }
            let regions: Vec<_> = handle.regions_from(addr).collect();
            if regions.is_empty() {
                break;
            }

            for region in &regions {
                addr = region.base_address + region.region_size;
                if !region.is_committed || !region.is_readable {
                    continue;
                }
                if region.region_size > REGION_MAX {
                    continue;
                }
                if !(HEAP_MIN..HEAP_MAX).contains(&region.base_address) {
                    continue;
                }

                let buf = match handle.read(region.base_address, region.region_size) {
                    Some(r) => r,
                    None => continue,
                };
                if buf.is_empty() {
                    continue;
                }

                let region_base = region.base_address as u64;
                let mut search_from = 0usize;
                while let Some(pos) = buf[search_from..]
                    .windows(pat.len())
                    .position(|w| w == pat)
                    .map(|p| p + search_from)
                {
                    let match_addr = region_base + pos as u64;
                    let start = pos.saturating_sub(8);
                    let end = (pos + CTX).min(buf.len());
                    results.push((region_base, match_addr, buf[start..end].to_vec()));
                    search_from = pos + STRIDE;
                }
            }
        }
        results
    }

    fn find_warframe_pid() -> Option<u32> {
        Platform::find_warframe_pid()
    }

    fn scan_process(pid: u32, patterns: &[MemoryPattern]) -> Vec<(String, u64, u64, u64, Vec<u8>)> // (pat_name, region_base, region_size, match_addr, context)
    {
        let mut results = Vec::new();

        let handle = match Platform::open_process(pid) {
            Ok(h) => h,
            Err(_) => return results,
        };

        let mut addr: usize = 0;
        loop {
            let regions: Vec<_> = handle.regions_from(addr).collect();
            if regions.is_empty() {
                break;
            }

            for region in &regions {
                addr = region.base_address + region.region_size;
                if !region.is_committed || !region.is_readable {
                    continue;
                }
                if region.region_size > 128 * 1024 * 1024 {
                    continue;
                }

                let region_base = region.base_address as u64;
                let region_size = region.region_size as u64;

                // Only read the region if at least one pattern applies to it.
                let any_match = patterns.iter().any(|&(_, _, _, _, min_addr, max_sz)| {
                    region_base >= min_addr && (max_sz == 0 || region_size <= max_sz)
                });
                if !any_match {
                    continue;
                }

                let buf = match handle.read(region.base_address, region.region_size) {
                    Some(r) => r,
                    None => continue,
                };
                if buf.is_empty() {
                    continue;
                }

                for &(name, pat, ctx_before, ctx_after, min_addr, max_region_size) in patterns {
                    if region_base < min_addr {
                        continue;
                    }
                    if max_region_size > 0 && region_size > max_region_size {
                        continue;
                    }
                    let mut search_from = 0usize;
                    while let Some(pos) = buf[search_from..]
                        .windows(pat.len())
                        .position(|w| w == pat)
                        .map(|p| p + search_from)
                    {
                        let match_addr = region_base + pos as u64;
                        let start = pos.saturating_sub(ctx_before);
                        let end = (pos + ctx_after).min(buf.len());
                        let ctx = buf[start..end].to_vec();
                        results.push((name.to_string(), region_base, region_size, match_addr, ctx));
                        search_from = pos + pat.len();
                    }
                }
            }
        }
        results
    }

    fn fmt_context(ctx: &[u8]) -> String {
        // Hex + ASCII side-by-side, 32 bytes per row.
        let mut out = String::new();
        for chunk in ctx.chunks(32) {
            let hex: String = chunk.iter().map(|b| format!("{:02x} ", b)).collect();
            let ascii: String = chunk
                .iter()
                .map(|&b| {
                    if (0x20..0x7F).contains(&b) {
                        b as char
                    } else {
                        '.'
                    }
                })
                .collect();
            out.push_str(&format!("  {:<96} {}\n", hex, ascii));
        }
        out
    }

    // EE.log path
    let ee_path = match dirs::data_local_dir() {
        Some(d) => d.join("Warframe").join("EE.log"),
        None => {
            append(log_path, "[ERROR] Cannot find %LOCALAPPDATA%\n");
            return;
        }
    };

    // Open EE.log and seek to end so we only see NEW lines.
    let mut ee_file = std::fs::File::open(&ee_path).ok();
    if let Some(ref mut f) = ee_file {
        let _ = f.seek(SeekFrom::End(0));
    }
    let mut ee_leftover = String::new();

    append(
        log_path,
        "[READY] Waiting for Warframe and EE.log events…\n\n",
    );

    // ── Slow scan thread ─────────────────────────────────────────────────
    // Runs full memory walk (~60 s) continuously in background.
    // Results sent via channel; main loop picks them up each tick.
    let (slow_tx, slow_rx) = std::sync::mpsc::channel::<Vec<(String, u64, u64, u64, Vec<u8>)>>();
    std::thread::spawn(move || {
        while MEM_RELIC_DEBUG_RUNNING.load(Ordering::SeqCst) {
            if let Some(pid) = find_warframe_pid() {
                let results = scan_process(pid, PATTERNS_SLOW);
                if slow_tx.send(results).is_err() {
                    break;
                }
            } else {
                std::thread::sleep(std::time::Duration::from_secs(2));
            }
        }
    });

    // ── One-shot scan thread ──────────────────────────────────────────────
    // Triggered by a channel message when EE.log emits the reward-screen open/close line.
    // Scans ALL heap memory immediately and logs results, then waits for the next trigger.
    let (oneshot_tx, oneshot_rx) = std::sync::mpsc::channel::<(u32, String)>(); // (pid, label)
    let oneshot_log = log_path.to_path_buf();
    std::thread::spawn(move || {
        while let Ok((pid, label)) = oneshot_rx.recv() {
            let ts = chrono::Local::now().format("%H:%M:%S%.3f");
            append(
                &oneshot_log,
                &format!("\n[ONE-SHOT @ {} — {}]\n", ts, label),
            );
            let results = scan_process(pid, PATTERNS_ONESHOT);
            let ts2 = chrono::Local::now().format("%H:%M:%S%.3f");
            if results.is_empty() {
                append(
                    &oneshot_log,
                    &format!("  (no matches) scan finished @ {}\n", ts2),
                );
            } else {
                let mut block = format!("  {} match(es), scan finished @ {}\n", results.len(), ts2);
                for (name, region_base, _rs, match_addr, ctx) in &results {
                    block.push_str(&format!(
                        "  MATCH {} | match=0x{:016x}  region=0x{:016x}\n",
                        name, match_addr, region_base
                    ));
                    for chunk in ctx.chunks(32) {
                        let hex: String = chunk.iter().map(|b| format!("{:02x} ", b)).collect();
                        let ascii: String = chunk
                            .iter()
                            .map(|&b| {
                                if (0x20..0x7f).contains(&b) {
                                    b as char
                                } else {
                                    '.'
                                }
                            })
                            .collect();
                        block.push_str(&format!("    {:<96} {}\n", hex, ascii));
                    }
                    block.push('\n');
                }
                append(&oneshot_log, &block);
            }
        }
    });

    // ── Reward-window heap scan thread ───────────────────────────────────
    // Fires every 2 s while the relic reward screen is open.
    // Scans only small heap regions for /Lotus/ paths — these would be
    // the 4 reward item paths stored in Lua string tables or C++ structures.
    let rw_pid: std::sync::Arc<std::sync::Mutex<Option<u32>>> =
        std::sync::Arc::new(std::sync::Mutex::new(None));
    {
        let rw_pid_cl = rw_pid.clone();
        let rw_log = log_path.to_path_buf();
        std::thread::spawn(move || {
            let mut scan_num = 0u32;
            let mut was_open = false;
            loop {
                let maybe_pid = *rw_pid_cl.lock().unwrap();
                if let Some(pid) = maybe_pid {
                    if !was_open {
                        was_open = true;
                        scan_num = 0;
                        let ts = chrono::Local::now().format("%H:%M:%S%.3f");
                        append(
                            &rw_log,
                            &format!(
                                "\n[RW OPEN @ {}] Starting /Lotus/ heap scan every 2 s\n",
                                ts
                            ),
                        );
                    }
                    scan_num += 1;
                    let ts = chrono::Local::now().format("%H:%M:%S%.3f");
                    let hits = scan_lotus_in_heap(pid);
                    if hits.is_empty() {
                        append(
                            &rw_log,
                            &format!(
                                "[RW #{} @ {}] (no /Lotus/ in small heap regions)\n",
                                scan_num, ts
                            ),
                        );
                    } else {
                        let mut block = format!(
                            "[RW #{} @ {}] {} /Lotus/ hit(s) in small heap:\n",
                            scan_num,
                            ts,
                            hits.len()
                        );
                        for (region_base, match_addr, ctx) in &hits {
                            block.push_str(&format!(
                                "  match=0x{:016x}  region=0x{:016x}\n",
                                match_addr, region_base
                            ));
                            for chunk in ctx.chunks(32) {
                                let hex: String =
                                    chunk.iter().map(|b| format!("{:02x} ", b)).collect();
                                let ascii: String = chunk
                                    .iter()
                                    .map(|&b| {
                                        if (0x20..0x7f).contains(&b) {
                                            b as char
                                        } else {
                                            '.'
                                        }
                                    })
                                    .collect();
                                block.push_str(&format!("    {:<96} {}\n", hex, ascii));
                            }
                            block.push('\n');
                        }
                        append(&rw_log, &block);
                    }
                    std::thread::sleep(std::time::Duration::from_secs(2));
                } else {
                    if was_open {
                        was_open = false;
                        let ts = chrono::Local::now().format("%H:%M:%S%.3f");
                        append(&rw_log, &format!("[RW CLOSED @ {}]\n\n", ts));
                    }
                    std::thread::sleep(std::time::Duration::from_millis(500));
                }
            }
        });
    }

    // Separate prev-match maps so fast and slow don't interfere.
    // Value = (region_base, context_bytes) — region_base lets us log which alloc the match came from.
    let mut fast_prev: HashMap<(String, u64), (u64, Vec<u8>)> = HashMap::new();
    let mut slow_prev: HashMap<(String, u64), (u64, Vec<u8>)> = HashMap::new();
    let mut scan_num = 0u32;
    let mut warned_no_wf = false;
    let mut last_oneshot_pid: Option<u32> = None;

    // ── Main loop: fast pass every second ────────────────────────────────
    while MEM_RELIC_DEBUG_RUNNING.load(Ordering::SeqCst) {
        let ts = chrono::Local::now().format("%H:%M:%S%.3f");

        // ── EE.log tail ──────────────────────────────────────────────────
        if ee_file.is_none() {
            ee_file = std::fs::File::open(&ee_path).ok();
            if let Some(ref mut f) = ee_file {
                let _ = f.seek(SeekFrom::End(0));
            }
        }
        if let Some(ref mut f) = ee_file {
            let mut chunk = String::new();
            if f.read_to_string(&mut chunk).is_ok() && !chunk.is_empty() {
                ee_leftover.push_str(&chunk);
                let mut log_buf = String::new();
                while let Some(nl) = ee_leftover.find('\n') {
                    let line = ee_leftover[..nl].trim_end_matches('\r').to_string();
                    ee_leftover = ee_leftover[nl + 1..].to_string();
                    if !line.is_empty() {
                        log_buf.push_str(&format!("[EE] {}  {}\n", ts, line));
                        // Fire one-shot scans on key reward-screen events.
                        if let Some(pid) = last_oneshot_pid {
                            let ll = line.to_lowercase();
                            if ll.contains("voidprojections: getvoidprojectionreward") {
                                let _ = oneshot_tx.send((pid, "OPEN trigger".to_string()));
                                // Start reward-window heap scan.
                                *rw_pid.lock().unwrap() = Some(pid);
                            } else if ll.contains("relic reward screen shut down")
                                || ll.contains("closevoidprojectionrewardscreen")
                            {
                                let _ = oneshot_tx.send((pid, "CLOSE trigger".to_string()));
                                // Stop reward-window heap scan.
                                *rw_pid.lock().unwrap() = None;
                            } else if ll.contains("gets reward /lotus/") {
                                let _ = oneshot_tx.send((pid, "gets reward".to_string()));
                            }
                        }
                    }
                }
                if !log_buf.is_empty() {
                    append(log_path, &log_buf);
                }
            }
        }

        let log_set =
            |label: &str, keys: &[(String, u64)], m: &HashMap<(String, u64), (u64, Vec<u8>)>| {
                let mut s = String::new();
                for key in keys {
                    if let Some((region_base, ctx)) = m.get(key) {
                        s.push_str(&format!(
                            "  {} {} | match=0x{:016x}  region=0x{:016x}\n{}\n",
                            label,
                            key.0,
                            key.1,
                            region_base,
                            fmt_context(ctx)
                        ));
                    }
                }
                s
            };

        // ── Fast pass: binary range + small heap regions, every tick ─────
        if let Some(pid) = find_warframe_pid() {
            last_oneshot_pid = Some(pid);
            warned_no_wf = false;
            append(log_path, &format!("[LOG SCAN @ {}]\n", ts));
            let matches = scan_process(pid, PATTERNS_FAST);
            let mut new_keys: Vec<(String, u64)> = Vec::new();
            let mut chg_keys: Vec<(String, u64)> = Vec::new();
            let mut gone_keys: Vec<(String, u64)> = Vec::new();
            let mut cur_map: HashMap<(String, u64), (u64, Vec<u8>)> = HashMap::new();
            for (name, region_base, _rs, addr, ctx) in &matches {
                let key = (name.clone(), *addr);
                if let Some((_, prev)) = fast_prev.get(&key) {
                    if prev != ctx {
                        chg_keys.push(key.clone());
                    }
                } else {
                    new_keys.push(key.clone());
                }
                cur_map.insert(key, (*region_base, ctx.clone()));
            }
            for key in fast_prev.keys() {
                if !cur_map.contains_key(key) {
                    gone_keys.push(key.clone());
                }
            }
            fast_prev = cur_map;

            if !new_keys.is_empty() || !chg_keys.is_empty() || !gone_keys.is_empty() {
                let mut block = format!("\n[LOG SCAN @ {} — PID {}]\n", ts, pid);
                block.push_str(&log_set("NEW    ", &new_keys, &fast_prev));
                block.push_str(&log_set("CHANGED", &chg_keys, &fast_prev));
                for key in &gone_keys {
                    block.push_str(&format!("  GONE   {} @ 0x{:016x}\n", key.0, key.1));
                }
                append(log_path, &block);
            }
        } else if !warned_no_wf {
            warned_no_wf = true;
            append(
                log_path,
                &format!("[MEM @ {}] Warframe not running — will retry\n", ts),
            );
        }

        // ── Drain slow-scan results (non-blocking) ────────────────────────
        while let Ok(matches) = slow_rx.try_recv() {
            scan_num += 1;
            let ts2 = chrono::Local::now().format("%H:%M:%S%.3f");
            let mut new_keys: Vec<(String, u64)> = Vec::new();
            let mut chg_keys: Vec<(String, u64)> = Vec::new();
            let mut gone_keys: Vec<(String, u64)> = Vec::new();
            let mut cur_map: HashMap<(String, u64), (u64, Vec<u8>)> = HashMap::new();
            for (name, region_base, _rs, addr, ctx) in &matches {
                let key = (name.clone(), *addr);
                if let Some((_, prev)) = slow_prev.get(&key) {
                    if prev != ctx {
                        chg_keys.push(key.clone());
                    }
                } else {
                    new_keys.push(key.clone());
                }
                cur_map.insert(key, (*region_base, ctx.clone()));
            }
            for key in slow_prev.keys() {
                if !cur_map.contains_key(key) {
                    gone_keys.push(key.clone());
                }
            }
            slow_prev = cur_map;

            if !new_keys.is_empty() || !chg_keys.is_empty() || !gone_keys.is_empty() {
                let mut block = format!(
                    "\n[MEM SCAN #{} @ {} — PID {}]\n",
                    scan_num,
                    ts2,
                    find_warframe_pid().unwrap_or(0)
                );
                block.push_str(&log_set("NEW    ", &new_keys, &slow_prev));
                block.push_str(&log_set("CHANGED", &chg_keys, &slow_prev));
                for key in &gone_keys {
                    block.push_str(&format!("  GONE   {} @ 0x{:016x}\n", key.0, key.1));
                }
                append(log_path, &block);
            }
        }

        std::thread::sleep(std::time::Duration::from_secs(1));
    }

    append(log_path, "\n[STOPPED]\n");
}

#[cfg(not(target_os = "windows"))]
fn mem_relic_debug_loop(_log_path: &std::path::Path) {}
