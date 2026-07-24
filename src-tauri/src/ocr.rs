/// Screen capture + Windows OCR for Warframe relic reward detection.
///
/// Capture strategy (automatic, works for all display modes):
///   1. PrintWindow (GDI) — fast, window-specific, works for Windowed and Borderless Windowed.
///      Quick brightness check: if the result is dark (avg < 30) the game is almost certainly
///      in Fullscreen Exclusive mode and GDI can't reach the DX framebuffer.
///   2. DXGI Desktop Duplication — captures the display output at hardware level, bypasses DWM.
///      Works for Fullscreen Exclusive, Borderless Windowed, and Windowed.
///      The correct monitor is chosen dynamically: whichever monitor the Warframe window is on.

// ─── Screenshot ───────────────────────────────────────────────────────────────

/// Compute average pixel brightness from a BGRA buffer (sampled every 64 pixels).
fn avg_brightness(pixels: &[u8]) -> u32 {
    let sum: u32 = pixels.chunks_exact(4).step_by(64)
        .map(|p| (p[0] as u32 + p[1] as u32 + p[2] as u32) / 3)
        .sum();
    sum / (pixels.len() / 4 / 64).max(1) as u32
}

/// Main entry point. Tries PrintWindow first, falls back to DXGI if the frame is dark.
/// Returns (BGRA pixels, width, captured_height, full_height, capture_info).
/// captured_height covers the top 85% of the full window — reward cards are always in the
/// upper half of the screen. OCR line filtering (y >= 0.10 of cap_h) removes any HUD text
/// from the very top of the frame (FPS counters, ping overlays, etc.) without cropping.
/// capture_info describes which path was used and the pixel brightness, for session logging.
#[cfg(target_os = "windows")]
pub fn capture_warframe_reward_area() -> Option<(Vec<u8>, u32, u32, u32, String)> {
    // ── Path A: PrintWindow (Windowed / Borderless Windowed) ──────────────────
    if let Some((pixels, w, cap_h, full_h)) = capture_printwindow() {
        let avg = avg_brightness(&pixels);
        if avg >= 20 {
            let info = format!("PrintWindow  {}×{}px (top 80%, cap {}px)  avg_brightness={}", w, full_h, cap_h, avg);
            return Some((pixels, w, cap_h, full_h, info));
        }
        // Dark frame — Fullscreen Exclusive likely. Fall through to DXGI.
        let _ = avg;
        if let Some((px2, w2, cap_h2, full_h2)) = capture_dxgi(0.85) {
            let avg2 = avg_brightness(&px2);
            let info = format!(
                "DXGI  {}×{}px (top 80%, cap {}px)  avg_brightness={} \
                 (PrintWindow was dark: avg={})",
                w2, full_h2, cap_h2, avg2, avg
            );
            return Some((px2, w2, cap_h2, full_h2, info));
        }
        // Both paths failed — return the dark PrintWindow result so the caller
        // can classify it as dark-frame and log it properly.
        let info = format!(
            "PrintWindow  {}×{}px (top 80%, cap {}px)  avg_brightness={} [DARK — DXGI also failed]",
            w, full_h, cap_h, avg
        );
        return Some((pixels, w, cap_h, full_h, info));
    }

    // PrintWindow found no window (Warframe not running?) — try DXGI anyway
    if let Some((pixels, w, cap_h, full_h)) = capture_dxgi(0.85) {
        let avg = avg_brightness(&pixels);
        let info = format!(
            "DXGI  {}×{}px (top 80%, cap {}px)  avg_brightness={} (no Warframe window found)",
            w, full_h, cap_h, avg
        );
        return Some((pixels, w, cap_h, full_h, info));
    }

    None
}

/// GDI PrintWindow capture — works for Windowed and Borderless Windowed.
#[cfg(target_os = "windows")]
fn capture_printwindow() -> Option<(Vec<u8>, u32, u32, u32)> {
    use std::mem;
    use windows_sys::Win32::{
        Foundation::RECT,
        Graphics::Gdi::{
            CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject,
            GetDIBits, GetDC, ReleaseDC, SelectObject,
            BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, RGBQUAD,
        },
        UI::WindowsAndMessaging::{FindWindowW, GetClientRect},
    };
    #[link(name = "user32")]
    extern "system" { fn PrintWindow(hwnd: isize, hdcblt: isize, nflags: u32) -> i32; }
    const PW_RENDERFULLCONTENT: u32 = 2;

    unsafe {
        let title: Vec<u16> = "Warframe\0".encode_utf16().collect();
        let hwnd = FindWindowW(std::ptr::null(), title.as_ptr());
        if hwnd == 0 { return None; }

        let mut rect = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        GetClientRect(hwnd, &mut rect);
        let full_w = (rect.right - rect.left) as u32;
        let full_h = (rect.bottom - rect.top) as u32;
        if full_w < 100 || full_h < 100 { return None; }

        let cap_h = (full_h as f32 * 0.80) as u32;

        let hdc_win = GetDC(hwnd);
        let hdc_mem = CreateCompatibleDC(hdc_win);
        let hbm     = CreateCompatibleBitmap(hdc_win, full_w as i32, full_h as i32);
        let hbm_old = SelectObject(hdc_mem, hbm);

        PrintWindow(hwnd, hdc_mem, PW_RENDERFULLCONTENT);

        let mut bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize:          mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth:         full_w as i32,
                biHeight:        -(cap_h as i32),
                biPlanes:        1,
                biBitCount:      32,
                biCompression:   BI_RGB,
                biSizeImage:     0,
                biXPelsPerMeter: 0,
                biYPelsPerMeter: 0,
                biClrUsed:       0,
                biClrImportant:  0,
            },
            bmiColors: [RGBQUAD { rgbBlue: 0, rgbGreen: 0, rgbRed: 0, rgbReserved: 0 }],
        };
        let mut pixels = vec![0u8; (full_w * cap_h * 4) as usize];
        GetDIBits(hdc_mem, hbm, 0, cap_h, pixels.as_mut_ptr() as *mut _, &mut bmi, DIB_RGB_COLORS);

        SelectObject(hdc_mem, hbm_old);
        DeleteObject(hbm);
        DeleteDC(hdc_mem);
        ReleaseDC(hwnd, hdc_win);

        Some((pixels, full_w, cap_h, full_h))
    }
}

/// Capture a vertical slice of the Warframe window and run OCR on it.
/// y_start / y_end are fractions of the full window height (0.0–1.0).
/// Returns the raw OCR text.
/// Capture the Warframe window using PrintWindow and return raw BGRA pixels + dimensions.
/// Single capture can be reused for multiple OCR regions via `ocr_pixels_rect`.
#[cfg(target_os = "windows")]
pub fn capture_warframe_pixels() -> Result<(Vec<u8>, u32, u32), String> {
    use std::mem;
    use windows_sys::Win32::{
        Foundation::RECT,
        Graphics::Gdi::{
            CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject,
            GetDIBits, GetDC, ReleaseDC, SelectObject,
            BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, RGBQUAD,
        },
        UI::WindowsAndMessaging::{FindWindowW, GetClientRect},
    };
    #[link(name = "user32")]
    extern "system" { fn PrintWindow(hwnd: isize, hdcblt: isize, nflags: u32) -> i32; }
    const PW_RENDERFULLCONTENT: u32 = 2;

    unsafe {
        let title: Vec<u16> = "Warframe\0".encode_utf16().collect();
        let hwnd = FindWindowW(std::ptr::null(), title.as_ptr());
        if hwnd == 0 { return Err("Warframe window not found".into()); }

        let mut rect = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        GetClientRect(hwnd, &mut rect);
        let full_w = (rect.right  - rect.left) as u32;
        let full_h = (rect.bottom - rect.top)  as u32;
        if full_w < 100 || full_h < 100 { return Err("Window too small".into()); }

        let hdc_win = GetDC(hwnd);
        let hdc_mem = CreateCompatibleDC(hdc_win);
        let hbm     = CreateCompatibleBitmap(hdc_win, full_w as i32, full_h as i32);
        let hbm_old = SelectObject(hdc_mem, hbm);
        PrintWindow(hwnd, hdc_mem, PW_RENDERFULLCONTENT);

        let mut bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: full_w as i32,
                biHeight: -(full_h as i32),
                biPlanes: 1, biBitCount: 32, biCompression: BI_RGB,
                biSizeImage: 0, biXPelsPerMeter: 0, biYPelsPerMeter: 0,
                biClrUsed: 0, biClrImportant: 0,
            },
            bmiColors: [RGBQUAD { rgbBlue: 0, rgbGreen: 0, rgbRed: 0, rgbReserved: 0 }],
        };
        let mut pixels = vec![0u8; (full_w * full_h * 4) as usize];
        GetDIBits(hdc_mem, hbm, 0, full_h,
                  pixels.as_mut_ptr() as *mut _, &mut bmi, DIB_RGB_COLORS);
        SelectObject(hdc_mem, hbm_old);
        DeleteObject(hbm);
        DeleteDC(hdc_mem);
        ReleaseDC(hwnd, hdc_win);
        Ok((pixels, full_w, full_h))
    }
}

/// 2× nearest-neighbour upscale + contrast stretch on BGRA pixels.
/// WinRT OCR accuracy improves significantly on larger, high-contrast images.
/// Grayscale + contrast stretch on BGRA pixels.
/// Converting to grayscale is the key step: element icons (❄ Cold, 🔥 Heat, ☠ Toxin)
/// are colored glyphs — in the original BGRA image WinRT OCR rejects these lines as
/// graphics. After grayscale they become neutral-brightness shapes, so OCR reads the
/// white text on either side of the icon instead of dropping the whole line.
fn preprocess_for_ocr(pixels: &[u8], width: u32, height: u32) -> (Vec<u8>, u32, u32) {
    let mut out = pixels.to_vec();
    for px in out.chunks_mut(4) {
        // Standard luminance: 0.299 R + 0.587 G + 0.114 B (BGRA order)
        let gray = ((px[2] as u32 * 299 + px[1] as u32 * 587 + px[0] as u32 * 114) / 1000)
            .min(255) as u8;
        // Mild contrast stretch [20, 235] → [0, 255]
        let v = ((gray as i32 - 20) * 255 / 215).clamp(0, 255) as u8;
        px[0] = v;
        px[1] = v;
        px[2] = v;
    }
    (out, width, height)
}

/// OCR a rectangle from a pre-captured pixel buffer. All coordinates are 0.0–1.0 fractions.
/// Applies a mild contrast stretch before OCR (no upscaling — upscaling distorts numerals).
#[cfg(target_os = "windows")]
pub fn ocr_pixels_rect(
    pixels: &[u8], full_w: u32, full_h: u32,
    x_start: f32, x_end: f32, y_start: f32, y_end: f32,
) -> Result<String, String> {
    let col_s = (full_w as f32 * x_start.clamp(0.0, 1.0)) as usize;
    let col_e = ((full_w as f32 * x_end.clamp(0.0, 1.0)) as usize).min(full_w as usize);
    let row_s = (full_h as f32 * y_start.clamp(0.0, 1.0)) as usize;
    let row_e = ((full_h as f32 * y_end.clamp(0.0, 1.0)) as usize).min(full_h as usize);
    let rect_w = (col_e - col_s) as u32;
    let rect_h = (row_e - row_s) as u32;
    if rect_w < 4 || rect_h < 4 { return Err("Region too small".into()); }

    let src_stride  = full_w as usize * 4;
    let dst_stride  = rect_w as usize * 4;
    let mut cropped = vec![0u8; dst_stride * rect_h as usize];
    for row in 0..rect_h as usize {
        let src = (row_s + row) * src_stride + col_s * 4;
        let dst = row * dst_stride;
        cropped[dst..dst + dst_stride].copy_from_slice(&pixels[src..src + dst_stride]);
    }

    let (enhanced, ew, eh) = preprocess_for_ocr(&cropped, rect_w, rect_h);
    let bmp = to_bmp(&enhanced, ew, eh);
    run_windows_ocr(bmp, ew, eh).map(|(text, _)| text)
}

/// OCR a rectangle WITHOUT preprocessing — for white-on-dark text that OCRs fine raw.
#[cfg(target_os = "windows")]
pub fn ocr_pixels_rect_raw(
    pixels: &[u8], full_w: u32, full_h: u32,
    x_start: f32, x_end: f32, y_start: f32, y_end: f32,
) -> Result<String, String> {
    let col_s = (full_w as f32 * x_start.clamp(0.0, 1.0)) as usize;
    let col_e = ((full_w as f32 * x_end.clamp(0.0, 1.0)) as usize).min(full_w as usize);
    let row_s = (full_h as f32 * y_start.clamp(0.0, 1.0)) as usize;
    let row_e = ((full_h as f32 * y_end.clamp(0.0, 1.0)) as usize).min(full_h as usize);
    let rect_w = (col_e - col_s) as u32;
    let rect_h = (row_e - row_s) as u32;
    if rect_w < 4 || rect_h < 4 { return Err("Region too small".into()); }
    let src_stride = full_w as usize * 4;
    let dst_stride = rect_w as usize * 4;
    let mut cropped = vec![0u8; dst_stride * rect_h as usize];
    for row in 0..rect_h as usize {
        let src = (row_s + row) * src_stride + col_s * 4;
        let dst = row * dst_stride;
        cropped[dst..dst + dst_stride].copy_from_slice(&pixels[src..src + dst_stride]);
    }
    let bmp = to_bmp(&cropped, rect_w, rect_h);
    run_windows_ocr(bmp, rect_w, rect_h).map(|(text, _)| text)
}

/// Convenience: capture + OCR a vertical strip of the window (full width).
#[allow(dead_code)]
pub fn capture_and_ocr_region(y_start: f32, y_end: f32) -> Result<String, String> {
    let (pixels, w, h) = capture_warframe_pixels()?;
    ocr_pixels_rect(&pixels, w, h, 0.0, 1.0, y_start, y_end)
}

/// Convenience: capture + OCR a specific rectangle.
#[allow(dead_code)]
pub fn capture_rect_and_ocr(x_start: f32, x_end: f32, y_start: f32, y_end: f32) -> Result<String, String> {
    let (pixels, w, h) = capture_warframe_pixels()?;
    ocr_pixels_rect(&pixels, w, h, x_start, x_end, y_start, y_end)
}

/// Captures the full Warframe window region from the desktop using GDI BitBlt.
/// Because this reads the composited desktop surface (not the window in isolation),
/// any Tauri overlay window sitting on top is included in the result.
#[allow(dead_code)]
/// Full-resolution capture — kept for future use; manual capture uses the half-res variant.
/// Full-resolution capture for manual diagnostics.
/// Falls back to full-monitor DXGI if the frame is dark (fullscreen exclusive mode).
/// Returns BGRA pixels, width, height.
#[cfg(target_os = "windows")]
pub fn capture_screen_for_diagnostics() -> Result<(Vec<u8>, u32, u32), String> {
    if let Some((pixels, w, h)) = capture_screen_gdi_scaled(1, 1) {
        if avg_brightness(&pixels) >= 10 {
            return Ok((pixels, w, h));
        }
    }
    // Dark — fullscreen exclusive. Fall back to full-height DXGI.
    match capture_dxgi(1.0) {
        Some((pixels, w, _cap_h, full_h)) => Ok((pixels, w, full_h)),
        None => Err("Warframe window not found or capture failed".into()),
    }
}

/// Half-resolution capture for automatic diagnostics.
/// StretchBlt writes a smaller destination bitmap so GetDIBits reads 4× less data,
/// reducing GPU pipeline stalls on high-resolution displays.
#[cfg(target_os = "windows")]
pub fn capture_screen_for_diagnostics_half() -> Result<(Vec<u8>, u32, u32), String> {
    if let Some((pixels, w, h)) = capture_screen_gdi_scaled(1, 2) {
        if avg_brightness(&pixels) >= 10 {
            return Ok((pixels, w, h));
        }
    }
    match capture_dxgi(1.0) {
        Some((pixels, w, _cap_h, full_h)) => Ok((pixels, w, full_h)),
        None => Err("Warframe window not found or capture failed".into()),
    }
}

/// GDI capture of the Warframe window at a fractional scale (num/denom).
/// Uses StretchBlt so the destination bitmap — and therefore the GetDIBits readback —
/// is (num/denom)² smaller, which reduces GPU pipeline stall time proportionally.
/// Pass (1, 1) for full resolution; (1, 2) for half resolution, etc.
/// DWM composites all windows before BitBlt reads them, so overlay windows are visible.
#[cfg(target_os = "windows")]
fn capture_screen_gdi_scaled(num: u32, denom: u32) -> Option<(Vec<u8>, u32, u32)> {
    use std::mem;
    use windows_sys::Win32::{
        Foundation::RECT,
        Graphics::Gdi::{
            StretchBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject,
            GetDC, GetDIBits, ReleaseDC, SelectObject,
            BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, RGBQUAD,
            SRCCOPY, HALFTONE, SetStretchBltMode,
        },
        UI::WindowsAndMessaging::{FindWindowW, GetWindowRect},
    };
    unsafe {
        let title: Vec<u16> = "Warframe\0".encode_utf16().collect();
        let hwnd = FindWindowW(std::ptr::null(), title.as_ptr());
        if hwnd == 0 { return None; }

        let mut rect = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        GetWindowRect(hwnd, &mut rect);
        let src_w = (rect.right  - rect.left) as u32;
        let src_h = (rect.bottom - rect.top)  as u32;
        if src_w < 100 || src_h < 100 { return None; }

        // Destination size after scale — at least 1 pixel each dimension.
        let dst_w = ((src_w * num) / denom).max(1);
        let dst_h = ((src_h * num) / denom).max(1);

        let hdc_screen = GetDC(0);
        let hdc_mem    = CreateCompatibleDC(hdc_screen);
        let hbm        = CreateCompatibleBitmap(hdc_screen, dst_w as i32, dst_h as i32);
        let hbm_old    = SelectObject(hdc_mem, hbm);

        // HALFTONE gives better quality when downscaling.
        SetStretchBltMode(hdc_mem, HALFTONE as i32);
        StretchBlt(
            hdc_mem,    0, 0, dst_w as i32, dst_h as i32,  // dest
            hdc_screen, rect.left, rect.top, src_w as i32, src_h as i32, // src
            SRCCOPY,
        );

        let mut bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize:        mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth:       dst_w as i32,
                biHeight:      -(dst_h as i32), // negative = top-down row order
                biPlanes:      1,
                biBitCount:    32,
                biCompression: BI_RGB,
                biSizeImage: 0, biXPelsPerMeter: 0, biYPelsPerMeter: 0,
                biClrUsed: 0, biClrImportant: 0,
            },
            bmiColors: [RGBQUAD { rgbBlue: 0, rgbGreen: 0, rgbRed: 0, rgbReserved: 0 }],
        };
        let mut pixels = vec![0u8; (dst_w * dst_h * 4) as usize];
        GetDIBits(hdc_mem, hbm, 0, dst_h,
                  pixels.as_mut_ptr() as *mut _, &mut bmi, DIB_RGB_COLORS);

        SelectObject(hdc_mem, hbm_old);
        DeleteObject(hbm);
        DeleteDC(hdc_mem);
        ReleaseDC(0, hdc_screen);
        Some((pixels, dst_w, dst_h))
    }
}

/// DXGI Desktop Duplication capture — works for Fullscreen Exclusive (and all other modes).
///
/// Dynamically determines which monitor the Warframe window is on so this works correctly
/// for any number of monitors, any primary/secondary arrangement, and any resolution.
/// Falls back to the primary monitor if the Warframe window can't be found.
#[cfg(target_os = "windows")]
fn capture_dxgi(cap_frac: f32) -> Option<(Vec<u8>, u32, u32, u32)> {
    use windows::core::Interface; // required for .cast() on COM types
    use windows::Win32::Graphics::{
        Direct3D::D3D_DRIVER_TYPE_HARDWARE,
        Direct3D11::{
            D3D11CreateDevice, D3D11_CPU_ACCESS_READ, D3D11_MAP_READ,
            D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
            ID3D11Resource, ID3D11Texture2D, D3D11_MAPPED_SUBRESOURCE,
        },
        Dxgi::{
            CreateDXGIFactory1, IDXGIFactory1, IDXGIOutput, IDXGIOutput1,
            IDXGIResource, DXGI_OUTDUPL_FRAME_INFO,
        },
        Dxgi::Common::DXGI_SAMPLE_DESC,
    };

    // In fullscreen exclusive mode, DuplicateOutput only succeeds for the output
    // that the game has exclusive ownership of. We use this to find the correct
    // monitor automatically — no GetDesc() or HMONITOR matching needed.
    //
    // For borderless/windowed games, PrintWindow already handled capture above;
    // we only reach this code when PrintWindow returned a dark frame.
    unsafe {
        // D3D11 device — required by DuplicateOutput
        let mut device = None;
        let mut ctx    = None;
        D3D11CreateDevice(
            None, D3D_DRIVER_TYPE_HARDWARE, None,
            Default::default(), None,
            7, // D3D11_SDK_VERSION
            Some(&mut device), None, Some(&mut ctx),
        ).ok()?;
        let device = device?;
        let ctx    = ctx?;
        let unk: windows::core::IUnknown = device.cast().ok()?;

        let factory: IDXGIFactory1 = CreateDXGIFactory1().ok()?;

        // Walk every adapter → every output. In fullscreen exclusive mode, only the
        // output the game owns accepts DuplicateOutput; all others return an error.
        // This lets us find the right monitor for any adapter/display configuration.
        let mut result: Option<(Vec<u8>, u32, u32, u32)> = None;

        'outer: for ai in 0u32.. {
            let adapter = match factory.EnumAdapters(ai) { Ok(a) => a, Err(_) => break };
            for oi in 0u32.. {
                let output: IDXGIOutput = match adapter.EnumOutputs(oi) { Ok(o) => o, Err(_) => break };
                let out1: IDXGIOutput1  = match output.cast() { Ok(o) => o, Err(_) => continue };

                // This fails for all outputs except the one the game is running on
                let dupl = match out1.DuplicateOutput(&unk) { Ok(d) => d, Err(_) => continue };

                // Acquire current frame (500 ms timeout)
                let mut fi  = DXGI_OUTDUPL_FRAME_INFO::default();
                let mut res: Option<IDXGIResource> = None;
                if dupl.AcquireNextFrame(500, &mut fi, &mut res).is_err() { continue; }
                let res = match res { Some(r) => r, None => { let _ = dupl.ReleaseFrame(); continue } };

                // Get the desktop texture and read its dimensions
                let src: ID3D11Texture2D = match res.cast() {
                    Ok(t) => t,
                    Err(_) => { let _ = dupl.ReleaseFrame(); continue }
                };
                let mut src_desc = D3D11_TEXTURE2D_DESC::default();
                src.GetDesc(&mut src_desc);
                let full_w = src_desc.Width;
                let full_h = src_desc.Height;
                if full_w < 100 || full_h < 100 { let _ = dupl.ReleaseFrame(); continue; }

                // Create CPU-readable staging texture (full monitor size)
                let staging_desc = D3D11_TEXTURE2D_DESC {
                    Width:          full_w,
                    Height:         full_h,
                    MipLevels:      1,
                    ArraySize:      1,
                    Format:         src_desc.Format,
                    SampleDesc:     DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                    Usage:          D3D11_USAGE_STAGING,
                    BindFlags:      Default::default(),
                    CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
                    MiscFlags:      Default::default(),
                };
                let mut staging: Option<ID3D11Texture2D> = None;
                if device.CreateTexture2D(&staging_desc, None, Some(&mut staging)).is_err() {
                    let _ = dupl.ReleaseFrame(); continue;
                }
                let staging = match staging { Some(s) => s, None => { let _ = dupl.ReleaseFrame(); continue } };

                // GPU blit → staging → map to CPU
                ctx.CopyResource(&staging.cast::<ID3D11Resource>().ok()?,
                                 &src.cast::<ID3D11Resource>().ok()?);

                let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
                if ctx.Map(&staging.cast::<ID3D11Resource>().ok()?, 0, D3D11_MAP_READ, 0, Some(&mut mapped)).is_err() {
                    let _ = dupl.ReleaseFrame(); continue;
                }

                let cap_h     = ((full_h as f32 * cap_frac) as u32).max(1);
                let row_pitch = mapped.RowPitch as usize;
                let src_ptr   = mapped.pData as *const u8;

                // DXGI is typically BGRA. Swap R↔B if RGBA so OCR pipeline always gets BGRA.
                use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_R8G8B8A8_UNORM;
                let swap_rb = src_desc.Format == DXGI_FORMAT_R8G8B8A8_UNORM;

                let mut pixels = Vec::with_capacity((full_w * cap_h * 4) as usize);
                for row in 0..(cap_h as usize) {
                    let slice = std::slice::from_raw_parts(
                        src_ptr.add(row * row_pitch), full_w as usize * 4);
                    if swap_rb {
                        for px in slice.chunks_exact(4) {
                            pixels.extend_from_slice(&[px[2], px[1], px[0], px[3]]);
                        }
                    } else {
                        pixels.extend_from_slice(slice);
                    }
                }

                ctx.Unmap(&staging.cast::<ID3D11Resource>().ok()?, 0);
                let _ = dupl.ReleaseFrame();

                result = Some((pixels, full_w, cap_h, full_h));
                break 'outer;
            }
        }

        result
    }
}

// ─── BMP encoding ─────────────────────────────────────────────────────────────

/// Encode BGRA pixels as a 24-bit BGR BMP (no alpha — BitmapDecoder handles it fine).
pub fn to_bmp(pixels_bgra: &[u8], width: u32, height: u32) -> Vec<u8> {
    let row_bytes = width * 3;
    let padding   = (4 - row_bytes % 4) % 4;
    let row_stride = row_bytes + padding;
    let image_size = row_stride * height;
    let file_size  = 54 + image_size;

    let mut bmp = Vec::with_capacity(file_size as usize);
    // File header
    bmp.extend_from_slice(b"BM");
    bmp.extend_from_slice(&file_size.to_le_bytes());
    bmp.extend_from_slice(&0u32.to_le_bytes());
    bmp.extend_from_slice(&54u32.to_le_bytes());
    // Info header
    bmp.extend_from_slice(&40u32.to_le_bytes());
    bmp.extend_from_slice(&(width as i32).to_le_bytes());
    bmp.extend_from_slice(&(-(height as i32)).to_le_bytes()); // top-down
    bmp.extend_from_slice(&1u16.to_le_bytes());
    bmp.extend_from_slice(&24u16.to_le_bytes());
    bmp.extend_from_slice(&0u32.to_le_bytes()); // BI_RGB
    bmp.extend_from_slice(&image_size.to_le_bytes());
    bmp.extend_from_slice(&0u32.to_le_bytes());
    bmp.extend_from_slice(&0u32.to_le_bytes());
    bmp.extend_from_slice(&0u32.to_le_bytes());
    bmp.extend_from_slice(&0u32.to_le_bytes());
    // Pixel rows (BGRA → BGR + padding)
    for row in 0..height {
        for col in 0..width {
            let i = ((row * width + col) * 4) as usize;
            bmp.push(pixels_bgra[i]);
            bmp.push(pixels_bgra[i + 1]);
            bmp.push(pixels_bgra[i + 2]);
        }
        for _ in 0..padding { bmp.push(0); }
    }
    bmp
}

// ─── Windows OCR ──────────────────────────────────────────────────────────────

/// Run Windows.Media.Ocr on a BMP. Returns (full_text, line_positions).
/// line_positions: Vec<(line_text, x_frac)> — X centre per line from word bounding rects.
#[cfg(target_os = "windows")]
pub fn run_windows_ocr(bmp: Vec<u8>, img_w: u32, img_h: u32) -> Result<(String, Vec<(String, f32, f32)>), String> {
    // Ensure COM is initialized for this thread. Tokio spawn_blocking threads
    // start without a COM apartment; WinRT calls fail or return empty silently.
    // CoInitializeEx returns S_OK (first init), S_FALSE (already MTA), or
    // RPC_E_CHANGED_MODE (already STA) — all safe to ignore.
    unsafe {
        windows_sys::Win32::System::Com::CoInitializeEx(
            std::ptr::null(),
            windows_sys::Win32::System::Com::COINIT_MULTITHREADED.try_into().unwrap_or(0),
        );
    }

    use windows::{
        Foundation::Collections::IVectorView,
        Globalization::Language,
        Graphics::Imaging::BitmapDecoder,
        Media::Ocr::{OcrEngine, OcrLine},
        Storage::Streams::{DataWriter, InMemoryRandomAccessStream},
    };

    (|| -> windows::core::Result<(String, Vec<(String, f32, f32)>)> {
        let stream = InMemoryRandomAccessStream::new()?;
        let writer = DataWriter::CreateDataWriter(&stream)?;
        writer.WriteBytes(&bmp)?;
        writer.StoreAsync()?.get()?;
        writer.FlushAsync()?.get()?;
        writer.DetachStream()?;
        stream.Seek(0)?;

        let decoder = BitmapDecoder::CreateAsync(&stream)?.get()?;
        let bitmap  = decoder.GetSoftwareBitmapAsync()?.get()?;

        // Warframe text is always English. Try "en-US" first so the engine
        // works correctly on non-English Windows installations (Dutch, etc.).
        // Fall back to user profile language if English pack isn't installed.
        let engine = Language::CreateLanguage(&windows::core::HSTRING::from("en-US"))
            .and_then(|lang| OcrEngine::TryCreateFromLanguage(&lang))
            .or_else(|_| OcrEngine::TryCreateFromUserProfileLanguages())?;
        let result = engine.RecognizeAsync(&bitmap)?.get()?;

        let mut full = String::new();
        let mut lines_out: Vec<(String, f32, f32)> = Vec::new();
        let lines: IVectorView<OcrLine> = result.Lines()?;
        let count = lines.Size()?;
        // Inter-card gap: reward cards are separated by ~10–12% of image width.
        // Word gaps within a single item name are ≤ 3%. Splitting at 7% cleanly
        // divides "Daikyu Prime Upper Limb Nautilus Prime Systems" (which WinRT
        // merges into one line when both names share the same baseline Y) into the
        // two separate card entries the column-assignment logic expects.
        const WORD_GAP: f32 = 0.07;
        for i in 0..count {
            let line = lines.GetAt(i)?;
            let words = line.Words()?;
            let wc = words.Size()?;
            if wc == 0 {
                let text = line.Text()?.to_string();
                full.push_str(&text); full.push('\n');
                lines_out.push((text, 0.5, 0.5));
                continue;
            }
            // Walk words left-to-right; flush a sub-line whenever the horizontal
            // gap to the next word exceeds WORD_GAP.
            let mut seg_texts: Vec<String> = Vec::new();
            let mut seg_sx = 0.0f32;
            let mut seg_sy = 0.0f32;
            let mut seg_n  = 0u32;
            let mut prev_right = -1.0f32;
            for j in 0..wc {
                let w  = words.GetAt(j)?;
                let r  = w.BoundingRect()?;
                let cx = if img_w > 0 { (r.X + r.Width  / 2.0) / img_w as f32 } else { 0.5 };
                let cy = if img_h > 0 { (r.Y + r.Height / 2.0) / img_h as f32 } else { 0.5 };
                let xl = if img_w > 0 { r.X / img_w as f32 } else { 0.0 };
                let xr = if img_w > 0 { (r.X + r.Width) / img_w as f32 } else { 1.0 };
                if prev_right >= 0.0 && (xl - prev_right) > WORD_GAP && !seg_texts.is_empty() {
                    let sub = seg_texts.join(" ");
                    full.push_str(&sub); full.push('\n');
                    lines_out.push((sub, seg_sx / seg_n as f32, seg_sy / seg_n as f32));
                    seg_texts.clear(); seg_sx = 0.0; seg_sy = 0.0; seg_n = 0;
                }
                seg_texts.push(w.Text()?.to_string());
                seg_sx += cx; seg_sy += cy; seg_n += 1;
                prev_right = xr;
            }
            if !seg_texts.is_empty() {
                let sub = seg_texts.join(" ");
                full.push_str(&sub); full.push('\n');
                lines_out.push((sub, seg_sx / seg_n as f32, seg_sy / seg_n as f32));
            }
        }
        Ok((full, lines_out))
    })().map_err(|e| e.to_string())
}

// ─── Word matching helpers ────────────────────────────────────────────────────

fn lev_dist(a: &str, b: &str) -> usize {
    let a = a.as_bytes();
    let b = b.as_bytes();
    let (m, n) = (a.len(), b.len());
    if m.abs_diff(n) > 3 { return 99; }
    let mut prev: Vec<usize> = (0..=n).collect();
    let mut curr = vec![0usize; n + 1];
    for i in 1..=m {
        curr[0] = i;
        for j in 1..=n {
            curr[j] = if a[i-1] == b[j-1] { prev[j-1] }
                      else { 1 + prev[j].min(curr[j-1]).min(prev[j-1]) };
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[n]
}

/// Check whether `catalog_word` appears in `ocr_words` via:
///   1. Exact match
///   2. Prefix match: OCR truncated ("prime"→"pri", "voruna"→"vor")
///   3. Suffix substring: "neuroptics" → OCR gives "rüroptics"/"tearoptics" which
///      both contain "optics" — the distinctive suffix is preserved even when the
///      prefix is garbled. Check last 5+ chars as a substring in any OCR word.
///   4. Levenshtein ≤ 1 (or ≤ 2 for ≥8-char words) for single-char typos
///   5. Sliding-window inside longer merged tokens ("Sevagotfirime")
fn word_found_in_set(
    catalog_word: &str,
    ocr_words: &std::collections::HashSet<String>,
) -> bool {
    if ocr_words.contains(catalog_word) { return true; }
    if catalog_word.len() < 4 { return false; }

    // Prefix: OCR word is the leading portion of the catalog word
    for ocr_w in ocr_words {
        if ocr_w.len() >= 3 && catalog_word.starts_with(ocr_w.as_str()) { return true; }
    }

    // Suffix substring: check if last N chars of catalog word appear inside any OCR word
    // Handles "neuroptics" → "rüroptics" because both contain "optics"
    // Guard: reject when the suffix appears at exactly position 1 — that means an OCR
    // word is a prefix-stripped version of the catalog word (e.g. "bronco" contains
    // suffix "ronco" of "akbronco" at position 1, which is a false positive).
    if catalog_word.len() >= 6 {
        let suffix_len = (catalog_word.len() / 2).max(5); // half the word, min 5 chars
        let suffix = &catalog_word[catalog_word.len() - suffix_len..];
        if ocr_words.iter().any(|w| w.find(suffix).map_or(false, |p| p != 1)) { return true; }
    }

    let max_dist = if catalog_word.len() >= 8 { 2 } else { 1 };
    let wb = catalog_word.as_bytes();
    for ocr_w in ocr_words {
        // Full-word Levenshtein — reject pure prefix/suffix insertions (len_diff == dist && >= 2)
        // e.g. dist("akbronco","bronco")=2 with len_diff=2 is just "ak" prepended, not a typo.
        // Also require OCR word ≥4 chars: 3-char HUD noise ("RAM","FPS","GPU") must not
        // fuzzy-match 4-char catalog words ("gram","fang"…) regardless of screen position.
        if ocr_w.len() >= 4 {
            let dist = lev_dist(catalog_word, ocr_w);
            let len_diff = (catalog_word.len() as isize - ocr_w.len() as isize).unsigned_abs();
            if dist <= max_dist && !(len_diff == dist && len_diff >= 2) { return true; }
        }
        // Sliding window (merged tokens — e.g. OCR reads "SevagothPrime" as one word)
        let ob = ocr_w.as_bytes();
        if ob.len() >= wb.len() {
            for (win_start, win) in ob.windows(wb.len()).enumerate() {
                let errs = wb.iter().zip(win.iter()).filter(|(a, b)| a != b).count();
                // Guard: reject exact suffix matches where the catalog word cleanly
                // terminates a longer OCR word (e.g. "fang" ending "sarofang").
                // "Sarofang" is a single correctly-read word; "fang" appearing at its
                // tail is a lexical coincidence, not an OCR merge artifact.
                // Only guard exact matches (errs == 0) — fuzzy matches are always valid.
                // win_start >= 3 avoids blocking short prefixes like "ak" in "akbronco".
                if errs == 0 && win_start + wb.len() == ob.len() && win_start >= 3 { continue; }
                if errs <= max_dist { return true; }
            }
        }
    }
    false
}

// ─── Catalog matching ─────────────────────────────────────────────────────────

/// Normalise OCR text for catalog matching.
/// ASCII letters are lowercased. Common diacritics are mapped to their ASCII
/// base (é→e, ü→u, …) so fuzzy matching still works when Windows OCR returns
/// accented surrogates instead of plain letters. Everything else → space.
fn normalise(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii() { return c.to_ascii_lowercase(); }
            match c {
                'À'|'Á'|'Â'|'Ã'|'Ä'|'Å'|'à'|'á'|'â'|'ã'|'ä'|'å' => 'a',
                'È'|'É'|'Ê'|'Ë'|'è'|'é'|'ê'|'ë' => 'e',
                'Ì'|'Í'|'Î'|'Ï'|'ì'|'í'|'î'|'ï' => 'i',
                'Ò'|'Ó'|'Ô'|'Õ'|'Ö'|'ò'|'ó'|'ô'|'õ'|'ö' => 'o',
                'Ù'|'Ú'|'Û'|'Ü'|'ù'|'ú'|'û'|'ü' => 'u',
                'Ñ'|'ñ' => 'n',
                'Ç'|'ç' => 'c',
                'Ý'|'ý'|'ÿ' => 'y',
                _ => ' ',
            }
        })
        .collect()
}

// ─── Rarity bar detection ─────────────────────────────────────────────────────

/// Scan the captured image for the coloured rarity bars below each reward card.
/// Returns (card_x_centers, bar_y_frac) where centers are fractions of image width.
///
/// Uses column aggregation: for each X column, count how many rows in the search
/// band have bar-coloured pixels. Columns that are consistently orange or teal
/// across many rows score high. This is far more robust than row-by-row detection
/// because it tolerates thin bars, color gradients, and single-row noise.
#[cfg(target_os = "windows")]
/// Returns `(Some((centers, bar_y_frac)), diagnostic_string)`.
/// `centers` are fractions of image width — the diamond icon X per card.
/// The diagnostic string is always populated for session log inclusion.
fn find_rarity_bars(pixels: &[u8], pix_w: u32, pix_h: u32) -> (Option<(Vec<f32>, f32)>, String) {
    let x_lo = (pix_w as f32 * 0.05) as u32;
    let x_hi = (pix_w as f32 * 0.95) as u32;
    // In the full 48% capture (no crops), bar position varies with UI scale:
    //   • Large window / small UI (e.g. 1440p native): bars at ~65–70% of cap_h
    //   • Smaller window / larger UI: bars at ~88–93% of cap_h
    // Starting at 55% skips the card artwork (weapon/helmet icons) while
    // catching both configurations. Upper bound stays at 0.97 to cover all cases.
    let y_lo = (pix_h as f32 * 0.55) as u32;
    let y_hi = (pix_h as f32 * 0.97) as u32;

    let scan_w = (x_hi - x_lo) as usize;

    // Rarity colours (BGRA from PrintWindow/DXGI). Permissive — Warframe's UI
    // background is very dark (avg_brightness often 30–40), so bar pixels can
    // be quite dim. The diamond/arrow icon at each card's centre is near-white.
    //   Orange/bronze : R dominant over B
    //   Silver/teal   : B/G dominant, cool cast
    //   Gold/rare     : warm, R > G > B
    //   Diamond icon  : near-white, brightest point in the bar
    #[inline]
    fn is_bar_pixel(b: u32, g: u32, r: u32) -> bool {
        let lum = (r + g + b) / 3;
        if lum < 25 { return false; }
        let is_orange = r > 80  && r > b + 20;
        let is_teal   = b > 65  && g > 50  && b > r + 8;
        let is_gold   = r > 100 && g > 80  && b < r.saturating_sub(10);
        let is_bright = lum > 100 && r > 70 && g > 70 && b > 70;
        is_orange || is_teal || is_gold || is_bright
    }

    // ── Step 1: Column projection ────────────────────────────────────────────
    //
    // For each X column sum how many rows in the search band contain a
    // bar-coloured pixel.  Accumulating vertically makes this robust to:
    //   • Thin bars    — even a 1-px-tall bar contributes to every column it covers
    //   • Small icons  — the rarity diamond is only ~20-30 px wide but several
    //                    rows tall; rows accumulate into a clear column peak
    //   • Colour noise — one mis-classified pixel doesn't ruin a whole column
    //
    // The previous per-row scan required ≥25 % of scan width (~430 px) lit in a
    // SINGLE row.  With only the small diamond icons present (~4 × 25 px = 100 px)
    // NO row ever reached that threshold → "0 coloured rows" in the log.
    let mut col_score = vec![0u32; scan_w];
    for y in y_lo..y_hi {
        for (xi, x) in (x_lo..x_hi).enumerate() {
            let i = ((y * pix_w + x) * 4) as usize;
            if i + 2 < pixels.len()
                && is_bar_pixel(pixels[i] as u32, pixels[i+1] as u32, pixels[i+2] as u32)
            {
                col_score[xi] += 1;
            }
        }
    }

    let max_col = col_score.iter().max().copied().unwrap_or(0);
    if max_col < 2 {
        return (None, format!(
            "no bars — column projection: max_col={} (need ≥2; y={:.0}–{:.0}%)",
            max_col,
            y_lo as f32 / pix_h as f32 * 100.0,
            y_hi as f32 / pix_h as f32 * 100.0,
        ));
    }

    // ── Step 2: Threshold + gap bridging + segment counting ──────────────────
    //
    // A column is "lit" when its score ≥ max_col/4.
    // Relative threshold handles both full-width bars (many columns, lower peak)
    // and icon-only bars (few columns but a taller, sharper peak).
    let col_threshold = (max_col / 4).max(2);
    let mut lit: Vec<bool> = col_score.iter().map(|&s| s >= col_threshold).collect();

    // Bridge tiny dark notches within one arrow (≤1 % of scan width).
    // Inter-card gaps are ~10 % of scan width and will NOT be bridged.
    let bridge = (scan_w / 100).max(3);
    {
        let mut xi = 0;
        while xi < scan_w {
            if !lit[xi] {
                let gap_start = xi;
                while xi < scan_w && !lit[xi] { xi += 1; }
                let gap_len = xi - gap_start;
                if gap_len <= bridge && gap_start > 0 && xi < scan_w {
                    for gxi in gap_start..xi { lit[gxi] = true; }
                }
            } else {
                xi += 1;
            }
        }
    }

    // Each continuous lit segment = one rarity bar = one reward card.
    // The rarity indicator is a small downward-pointing arrow (~30 px wide at 1080p).
    // min_band = 0.7% of scan width — passes arrows of ~10 px and above.
    let min_band = (scan_w / 150).max(6);
    let mut bands: Vec<(usize, usize)> = Vec::new();
    let mut in_band = false;
    let mut band_start = 0usize;
    for xi in 0..scan_w {
        match (lit[xi], in_band) {
            (true,  false) => { band_start = xi; in_band = true; }
            (false, true)  => {
                if xi - band_start >= min_band { bands.push((band_start, xi)); }
                in_band = false;
            }
            _ => {}
        }
    }
    if in_band && scan_w - band_start >= min_band { bands.push((band_start, scan_w)); }

    let lit_count = lit.iter().filter(|&&b| b).count();
    if bands.is_empty() {
        return (None, format!(
            "no bars — {} lit columns (threshold={}/{}), no segment ≥{}px (bridge={}px)",
            lit_count, col_threshold, max_col, min_band, bridge
        ));
    }
    if bands.len() > 4 {
        return (None, format!(
            "no bars — {} segments after bridging (expected 1–4); max_col={}, threshold={}",
            bands.len(), max_col, col_threshold
        ));
    }

    // ── Step 3: Bar Y position (for icon classifier) ─────────────────────────
    //
    // Restrict the row scan to lit X columns only, then find the row with the
    // most bar pixels.  classify_card_icon uses bar_y to locate the icon region
    // above the rarity bar for each card.
    let lit_xs: Vec<u32> = (0..scan_w as u32)
        .filter(|&xi| lit[xi as usize])
        .map(|xi| x_lo + xi)
        .collect();

    let mut best_row_y = (y_lo + y_hi) / 2; // fallback: geometric centre
    let mut best_row_cnt = 0u32;
    for y in y_lo..y_hi {
        let mut cnt = 0u32;
        for &x in &lit_xs {
            let i = ((y * pix_w + x) * 4) as usize;
            if i + 2 < pixels.len()
                && is_bar_pixel(pixels[i] as u32, pixels[i+1] as u32, pixels[i+2] as u32)
            {
                cnt += 1;
            }
        }
        if cnt > best_row_cnt { best_row_cnt = cnt; best_row_y = y; }
    }

    // ── Step 4: Card X center — peak column within each band ─────────────────
    //
    // The diamond/arrow icon sits at the exact centre of each card.
    // The column with the highest accumulated score within each band is the
    // most reliably lit X → use it as the card center.
    let centers: Vec<f32> = bands.iter().map(|(s, e)| {
        let best_xi = (*s..*e)
            .max_by_key(|&xi| col_score[xi])
            .unwrap_or((s + e) / 2);
        (x_lo as f32 + best_xi as f32) / pix_w as f32
    }).collect();

    let bar_y = best_row_y as f32 / pix_h as f32;
    let diag = format!(
        "{} bars — centers x=[{}], bar_y={:.2} ({:.0}%), max_col={}px, threshold={}px, lit={}px",
        bands.len(),
        centers.iter().map(|x| format!("{:.3}", x)).collect::<Vec<_>>().join(", "),
        bar_y, bar_y * 100.0, max_col, col_threshold, lit_count,
    );
    (Some((centers, bar_y)), diag)
}

#[cfg(not(target_os = "windows"))]
fn find_rarity_bars(_: &[u8], _: u32, _: u32) -> (Option<(Vec<f32>, f32)>, String) {
    (None, "not supported on non-Windows".into())
}

// ─── Icon component classifier ────────────────────────────────────────────────

/// What the card icon looks like, used to constrain catalog matching.
#[derive(Debug, Clone, PartialEq)]
pub enum IconType {
    /// Generic REUSED component shape — same icon appears across many primes.
    /// e.g. all neuroptics share the same helmet silhouette, all barrels look alike.
    /// The TEXT below identifies WHICH prime it belongs to.
    Component(&'static str), // "neuroptics" | "systems" | "chassis" |
                              // "barrel" | "stock" | "receiver" | "handle" |
                              // "blade" | "grip" | "upper limb" | "lower limb"
    /// Full 3D model of a unique warframe or weapon.
    /// Every prime has its own unique render → card always shows "[Name] Prime Blueprint".
    /// The TEXT (or partial text) gives us the [Name].
    FullModel,
    /// Forma spiral (distinctively blue)
    Forma,
    /// Could not classify
    Unknown,
}

/// Classify the reward card icon using an 8×8 spatial brightness grid.
///
/// Features extracted:
///   fill_ratio — fraction of grid cells above threshold (dense = full model)
///   aspect     — bounding-box width / height (> 1 wide, < 1 tall)
///   cm_y       — vertical centre-of-mass (0 = top, 1 = bottom)
///   symmetry   — left / right balance (1 = symmetric)
///   blue_dom   — blue channel dominance (Forma indicator)
///
/// Rule set (in priority order):
///   ① Forma        — blue channel dominates → blue spiral icon
///   ② FullModel    — high fill + even spread → complete warframe/weapon render;
///                    text gives "[Name] Prime Blueprint"
///   ③ neuroptics   — bright top half, symmetric, roughly square (helmet shape)
///   ④ systems      — bright central region, compact, somewhat circular (gear)
///   ⑤ chassis      — large central region, wider, lower CoM (torso)
///   ⑥ barrel       — wide aspect ratio (elongated horizontal part)
///   ⑦ handle       — tall aspect ratio (elongated vertical / melee handle)
///   ⑧ blade        — low symmetry, moderate aspect (flat asymmetric part)
///   ⑨ upper/lower limb — low fill, arc-shaped (bow components)
///   Unknown        — ambiguous; fall back to text-only matching
#[cfg(target_os = "windows")]
fn classify_card_icon(
    pixels: &[u8], pix_w: u32, pix_h: u32,
    x_left: f32, x_right: f32, bar_y: f32,
) -> IconType {
    // Card icon sits between the card top and the rarity bar.
    // In the capture buffer the icon occupies roughly bar_y-0.28 → bar_y-0.04.
    let iy_top = ((bar_y - 0.28).max(0.0) * pix_h as f32) as u32;
    let iy_bot = ((bar_y - 0.04).min(1.0) * pix_h as f32) as u32;
    let ix_lo  = (x_left  * pix_w as f32) as u32;
    let ix_hi  = (x_right * pix_w as f32).min(pix_w as f32) as u32;
    if ix_hi <= ix_lo || iy_bot <= iy_top { return IconType::Unknown; }

    const G: usize = 8;
    let mut lum  = [[0.0f32; G]; G];
    let mut blue = [[0.0f32; G]; G];
    let mut cnt  = [[0u32;  G]; G];

    for y in iy_top..iy_bot {
        let gy = (((y - iy_top) as f32 / (iy_bot - iy_top) as f32) * G as f32)
                     .min(G as f32 - 1.0) as usize;
        for x in ix_lo..ix_hi {
            let gx = (((x - ix_lo) as f32 / (ix_hi - ix_lo) as f32) * G as f32)
                         .min(G as f32 - 1.0) as usize;
            let i = ((y * pix_w + x) * 4) as usize;
            if i + 2 >= pixels.len() { continue; }
            let b = pixels[i]     as f32;
            let g = pixels[i + 1] as f32;
            let r = pixels[i + 2] as f32;
            lum [gy][gx] += (r + g + b) / 3.0;
            blue[gy][gx] += b;
            cnt [gy][gx] += 1;
        }
    }
    for gy in 0..G { for gx in 0..G {
        let c = cnt[gy][gx];
        if c > 0 { lum[gy][gx] /= c as f32; blue[gy][gx] /= c as f32; }
    }}

    let avg_lum  = lum.iter().flatten().sum::<f32>()  / (G*G) as f32;
    let avg_blue = blue.iter().flatten().sum::<f32>() / (G*G) as f32;

    // ① Forma: blue channel clearly stronger than average luminance
    if avg_blue > 75.0 && avg_blue > avg_lum * 1.35 { return IconType::Forma; }

    // Threshold: cells are "bright" if > 40 % of the peak cell
    let peak = lum.iter().flatten().cloned().fold(0.0f32, f32::max);
    let thr  = peak * 0.40;

    let mut bright_rows = [false; G];
    let mut bright_cols = [false; G];
    let mut n_bright = 0usize;
    let mut cx_sum   = 0.0f32;
    let mut cy_sum   = 0.0f32;

    for gy in 0..G { for gx in 0..G {
        if lum[gy][gx] > thr {
            bright_rows[gy] = true;
            bright_cols[gx] = true;
            n_bright += 1;
            cx_sum += gx as f32;
            cy_sum += gy as f32;
        }
    }}
    if n_bright == 0 { return IconType::Unknown; }

    // Centre-of-mass (0 = top/left, 1 = bottom/right)
    let cm_x = cx_sum / n_bright as f32 / (G-1) as f32;
    let cm_y = cy_sum / n_bright as f32 / (G-1) as f32;

    // Bounding box of bright region
    let row_lo = bright_rows.iter().position(|&b| b).unwrap_or(0)    as f32 / (G-1) as f32;
    let row_hi = bright_rows.iter().rposition(|&b| b).unwrap_or(G-1) as f32 / (G-1) as f32;
    let col_lo = bright_cols.iter().position(|&b| b).unwrap_or(0)    as f32 / (G-1) as f32;
    let col_hi = bright_cols.iter().rposition(|&b| b).unwrap_or(G-1) as f32 / (G-1) as f32;

    let bb_h   = (row_hi - row_lo).max(0.01);
    let bb_w   = (col_hi - col_lo).max(0.01);
    let aspect = bb_w / bb_h;            // > 1 wide,  < 1 tall
    let fill   = n_bright as f32 / (G*G) as f32;  // 0 – 1

    // Left / right symmetry score
    let l: f32 = (0..G).map(|gy| (0..G/2).map(|gx| lum[gy][gx]).sum::<f32>()).sum();
    let r: f32 = (0..G).map(|gy| (G/2..G).map(|gx| lum[gy][gx]).sum::<f32>()).sum();
    let symmetry = 1.0 - (l - r).abs() / (l + r + 0.001);

    let _ = cm_x; // reserved for future use

    // ② FullModel — complete warframe pose or full weapon render.
    //    Fills the card frame densely and relatively evenly.
    //    Text below gives "[Name]" → result is "[Name] Prime Blueprint".
    if fill > 0.55 && avg_lum > 70.0 { return IconType::FullModel; }

    // ③ Neuroptics — helmet silhouette, rounded top.
    //    CoM upper half, symmetric left/right, roughly square bounding box.
    if cm_y < 0.45 && symmetry > 0.72 && (0.5..=2.0).contains(&aspect) {
        return IconType::Component("neuroptics");
    }

    // ④ Systems — round mechanical ring / gear.
    //    Central CoM, compact, relatively symmetric and circular.
    if cm_y > 0.35 && cm_y < 0.65 && symmetry > 0.68 && (0.6..=1.7).contains(&aspect) && fill > 0.20 {
        return IconType::Component("systems");
    }

    // ⑤ Chassis — larger torso / body piece.
    //    CoM centre-to-low, more filled, wider than neuroptics.
    if cm_y > 0.42 && fill > 0.28 && (0.7..=2.2).contains(&aspect) {
        return IconType::Component("chassis");
    }

    // ⑥ Barrel / Stock / Receiver — elongated horizontal.
    //    Bounding box much wider than tall (aspect > 2).
    if aspect > 2.0 { return IconType::Component("barrel"); }

    // ⑦ Handle / Grip — elongated vertical (melee handle).
    //    Bounding box much taller than wide (aspect < 0.5).
    if aspect < 0.5 { return IconType::Component("handle"); }

    // ⑧ Blade — flat, angular, asymmetric.
    //    Moderate aspect but low left/right symmetry.
    if symmetry < 0.60 && (0.7..=3.0).contains(&aspect) {
        return IconType::Component("blade");
    }

    // ⑨ Upper / Lower Limb — curved bow piece (arc = low fill, hollow centre).
    if fill < 0.22 && (0.7..=2.5).contains(&aspect) {
        return if cm_y < 0.50 {
            IconType::Component("upper limb")
        } else {
            IconType::Component("lower limb")
        };
    }

    IconType::Unknown
}

#[cfg(not(target_os = "windows"))]
fn classify_card_icon(_: &[u8], _: u32, _: u32, _: f32, _: f32, _: f32) -> IconType {
    IconType::Unknown
}

/// Given a word set from OCR text, extract the most likely item NAME
/// (strip known non-name words: "prime", "blueprint", component names, "owned", etc.)
fn extract_item_name_words(words: &std::collections::HashSet<String>) -> Vec<String> {
    const SKIP: &[&str] = &[
        "prime", "blueprint", "owned", "crafted", "bl", "neuroptics", "systems",
        "chassis", "barrel", "stock", "receiver", "handle", "blade", "grip",
        "limb", "upper", "lower", "string", "link", "carapace", "cerebrum",
        "forma", "riven", "sliver", "ayatan",
    ];
    words.iter()
        .filter(|w| w.len() >= 3 && !SKIP.contains(&w.as_str()))
        .cloned()
        .collect()
}

/// Sanity-check detected bar centers.
/// Rejects detections caused by card artwork (orange forma gear, gold weapons)
/// which produce centers that are bunched together or out of range.
/// Valid 4-card centers span ~0.52 (e.g. 0.24→0.76); false-positive clusters
/// span much less (e.g. 0.372→0.706 = 0.334, seen with forma-heavy rewards).
fn bar_centers_are_valid(centers: &[f32]) -> bool {
    let n = centers.len();
    if n == 0 { return false; }
    // Outermost centers must be in a plausible screen zone
    if centers[0] < 0.15 || centers[n - 1] > 0.85 { return false; }
    if n < 2 { return true; }
    // Reject if any two adjacent bars are closer than 0.08.
    // The expected gap between cards is ~0.17 (4-card layout).
    // Bars within 0.08 of each other are a double-detection of the same bar
    // or a false positive from card artwork — they'd leave one column with no
    // OCR text and another column absorbing text from two cards at once.
    for pair in centers.windows(2) {
        if pair[1] - pair[0] < 0.08 { return false; }
    }
    let span = centers[n - 1] - centers[0];
    // Expected spans per card count (measured from real captures)
    let expected = match n {
        2 => 0.34f32,
        3 => 0.46,
        _ => 0.52, // 4 cards
    };
    (span - expected).abs() < 0.10
}

/// Evenly-distributed card X centers (fraction of image width) for N cards.
/// Calibrated from bar-detected centers on 1920×1080 captures: 4-card spread
/// is 0.31→0.69 (spacing ≈0.127), not the old 0.24→0.76.
/// Used as the fallback when rarity bar detection fails.
fn hardcoded_card_centers(n: usize) -> Vec<f32> {
    match n {
        1 => vec![0.50],
        2 => vec![0.435, 0.565],
        3 => vec![0.37, 0.50, 0.63],
        _ => vec![0.31, 0.44, 0.56, 0.69], // 4 cards (default / full squad)
    }
}

// ─── Matching helpers (standalone fns — no closure capture issues) ────────────

fn build_word_set(texts: &[String]) -> std::collections::HashSet<String> {
    let corrected = texts.join(" ")
        .replace('@', "bl").replace(')', "d").replace('&', " p");
    normalise(&corrected).chars()
        .map(|c| if c.is_ascii_alphabetic() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .filter(|w| w.len() >= 3)
        .map(|s| s.to_string())
        .collect()
}

fn score_item(display_name: &str, words: &std::collections::HashSet<String>) -> f32 {
    let norm = normalise(display_name);
    let item_words: Vec<&str> = norm.split_whitespace().collect();
    if item_words.is_empty() { return 0.0; }
    let n_catalog = item_words.len() as f32;
    let n_ocr = words.len() as f32;
    let matched = item_words.iter()
        .filter(|&&w| word_found_in_set(w, words))
        .count();

    // Use max of two coverage ratios:
    //  • matched/n_catalog — what fraction of the catalog item's words appear in OCR
    //  • matched/n_ocr     — what fraction of the OCR words the catalog item explains
    // This prevents long item names from being penalised when they explain ALL the OCR
    // words. E.g. OCR {assis, blueprint}: "Xaku Prime Chassis Blueprint" (4 words)
    // matches both via suffix and exact → max(2/4, 2/2) = 1.0, beating "Forma Blueprint"
    // (2 words, matches 1/2) whose pure catalog-coverage score 0.50 previously won.
    let base = (matched as f32 / n_catalog)
        .max(if n_ocr > 0.0 { matched as f32 / n_ocr } else { 0.0 });

    // Length-affinity bonus for unmatched catalog words.
    // OCR almost always preserves word length (substitutes chars, not inserts them),
    // so prefer catalog words whose length is close to the OCR word length.
    // Max bonus per unmatched word is 0.08/n — always less than one matched word (1/n).
    let len_bonus: f32 = item_words.iter()
        .filter(|&&w| !word_found_in_set(w, words))
        .map(|&cw| {
            words.iter()
                .map(|ow| {
                    let diff = (cw.len() as isize - ow.len() as isize).unsigned_abs();
                    if diff == 0 { 0.08_f32 } else if diff == 1 { 0.04 } else { 0.0 }
                })
                .fold(0.0_f32, f32::max)
        })
        .sum::<f32>() / n_catalog;

    base + len_bonus
}

// ─── Reward item extraction ───────────────────────────────────────────────────

/// Relic reward detection.
///
/// 1. Find rarity bars → card X positions + bar Y (reliable visual anchor).
/// 2. Full-frame raw OCR → text with line X positions.
/// 3. Assign each OCR line to the nearest card (by X).
/// 4. Per-card word set → prefix + fuzzy match against relic catalog.
/// 5. Full-frame fallback if bar detection fails.
#[cfg(target_os = "windows")]
pub fn extract_reward_items_twophase(
    pixels: &[u8], pix_w: u32, pix_h: u32, _game_h: u32,
    catalog: &[(String, String)],
    capture_info: &str,
    hint_squad_size: Option<usize>,
    player_names: &[String],
) -> (bool, bool, Vec<String>, Vec<f32>, String) {

    // ── 1. Raw OCR ────────────────────────────────────────────────────────────
    let (raw_full, ocr_lines) =
        match run_windows_ocr(to_bmp(pixels, pix_w, pix_h), pix_w, pix_h) {
            Ok(r) => r,
            Err(e) => return (false, false, vec![], vec![],
                format!("├─ Capture  : {}\n└─ OCR error: {}", capture_info, e)),
        };
    if raw_full.len() < 4 {
        // Save the captured BMP — open in photo viewer to diagnose:
        //   Black image  → PrintWindow didn't capture DX content (try borderless windowed mode)
        //   Game content → OCR engine issue (COM/language)
        let _ = std::fs::write(
            std::env::temp_dir().join("frameforge_capture_debug.bmp"),
            to_bmp(pixels, pix_w, pix_h),
        );
        let avg = avg_brightness(pixels);
        let kind = if avg < 30 { "dark-frame" } else { "ocr-empty" };
        return (false, false, vec![], vec![], format!(
            "├─ Capture  : {}\n└─ OCR      : returned no text ({}, avg={})\n   Saved: %TEMP%\\frameforge_capture_debug.bmp",
            capture_info, kind, avg
        ));
    }

    // Relic selection screens show relics named with a quality tier
    // ("Axi N9 Intact Relic", "Neo R3 Flawless Relic", etc.).
    // The reward screen never shows quality words — but endless missions show
    // "1 Relic Opened" in the Endless Bonus tracker, which a bare " relic" check
    // incorrectly matched, causing every reward-screen attempt to be skipped.
    {
        let lower = raw_full.to_lowercase();
        const QUALITY: &[&str] = &["intact", "exceptional", "flawless", "radiant"];
        if lower.contains(" relic") && QUALITY.iter().any(|q| lower.contains(q)) {
            return (false, true, vec![], vec![], format!(
                "├─ Capture  : {}\n└─ OCR      : relic selection screen detected (skipped)",
                capture_info
            ));
        }
    }

    // ── 2. Find card positions from rarity bars ───────────────────────────────
    // Rarity bars are always present regardless of Owned/Crafted labels.
    // If detection fails, fall back to X-gap grouping of OCR lines.
    let (bar_result, bar_diag) = find_rarity_bars(pixels, pix_w, pix_h);

    let (card_centers, _bar_y_frac): (Vec<f32>, f32) = match &bar_result {
        Some((centers, by)) => (centers.clone(), *by),
        None => (vec![], 0.0),
    };

    // Fixed cutoff near the bottom of the capture.
    // is_player_name handles name filtering; the bar-based cutoff was unreliable
    // (bar detection often placed bar_y too high, deleting valid item text).
    let ocr_y_max: f32 = 0.95;

    // Returns true if `text` resembles a known player name (≥80% char similarity).
    // Handles typical OCR garbling: "Dragonivan65" → "Dragonivan650", trailing symbols, etc.
    let is_player_name = |text: &str| -> bool {
        let t = text.trim().to_lowercase();
        if t.is_empty() { return false; }
        for name in player_names {
            let n = name.to_lowercase();
            // Fast path: exact substring in either direction
            if t.contains(&n) || n.contains(&t) { return true; }
            // Fuzzy path: Levenshtein ≤ 20% of the longer string
            let max_len = t.len().max(n.len());
            let threshold = (max_len / 5).max(1);
            if lev_dist(&t, &n) <= threshold { return true; }
        }
        false
    };

    // Returns true if the line is a card UI badge (ownership/craft status), not an item name.
    // Matches "Owned", "Crafted", "@ 4 Owned", "8 Crafted", "Unranked", etc.
    // Strips "@", numbers, and symbols — if only stopwords remain, it's a badge line.
    let is_ui_badge = |text: &str| -> bool {
        const BADGE_WORDS: &[&str] = &["owned", "crafted", "unranked", "mastered"];
        let meaningful: Vec<&str> = text.split_whitespace()
            .filter(|w| !w.starts_with('@') && w.parse::<u32>().is_err()
                    && w.chars().any(|c| c.is_alphabetic()))
            .collect();
        !meaningful.is_empty()
            && meaningful.iter().all(|w| BADGE_WORDS.contains(&w.to_lowercase().as_str()))
    };

    // ── 2b. Card count — prime+forma word count ──────────────────────────────
    // Every fissure reward is a prime item ("Prime" in name) or Forma Blueprint.
    // OCR frequently garbles "Prime" into "+rime", "Prtme", or merges it with the
    // next word ("Primeteüroptics").  Count any word that is "prime"-like:
    //   • starts with "prim"         → catches merged tokens like "primete..."
    //   • within edit-distance 1     → catches "+rime", "pnme", "prlme" etc.
    //   • "forma" or ≤1 edit of it  → catches "rorma", "torma" etc.
    let raw_norm = normalise(&raw_full);
    let is_prime_like = |w: &str| -> bool {
        if w.starts_with("prim") && w.len() >= 4 { return true; }
        if w == "pri" { return true; }  // OCR truncation: "Lavos Prime" → "Lavos Pri"
        if w.len() >= 3 && w.len() <= 9 { return lev_dist(w, "prime") <= 1; }
        false
    };
    let is_forma_like = |w: &str| -> bool {
        if w == "forma" { return true; }
        if w.len() >= 3 && w.len() <= 7 { return lev_dist(w, "forma") <= 1; }
        false
    };
    let prime_count = raw_norm.split_whitespace().filter(|&w| is_prime_like(w)).count();
    let forma_count  = raw_norm.split_whitespace().filter(|&w| is_forma_like(w)).count();

    // Count distinct x-position clusters in OCR output.
    // Each card's text groups at a consistent x — gaps > 10% of width mark a new card.
    // Uses centroid-based clustering (not single-linkage) so that a single off-centre
    // OCR line between two adjacent card columns doesn't bridge them together.
    // Example: cards at 0.41 and 0.59 with a bridge line at 0.50 →
    //   single-linkage: 0.50-0.41=0.09 < 0.10 (merged), 0.59-0.50=0.09 < 0.10 (merged) → 1 cluster
    //   centroid:       0.50-0.41=0.09 < 0.10 (extend, center→0.455), 0.59-0.455=0.135 > 0.10 → 2 clusters
    let ocr_cluster_count: usize = {
        // Filter to lines that are (a) long enough to be item text and
        // (b) NOT in the top 10% of the capture. The reward cards never appear
        // there — only the game title bar and screen-edge HUD overlays (FPS
        // counters, GPU widgets) do. Excluding them prevents spurious x-clusters.
        let mut xs: Vec<f32> = ocr_lines.iter()
            .filter(|(t, _, y)| t.trim().len() >= 3 && *y >= 0.10 && *y < ocr_y_max && !is_player_name(t) && !is_ui_badge(t))
            .map(|(_, x, _)| *x)
            .collect();
        xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        if xs.is_empty() { 0 }
        else {
            let mut count = 1usize;
            let mut cluster_sum = xs[0];
            let mut cluster_n   = 1usize;
            for &x in &xs[1..] {
                let center = cluster_sum / cluster_n as f32;
                if x - center > 0.10 {
                    count += 1;
                    cluster_sum = x;
                    cluster_n   = 1;
                } else {
                    cluster_sum += x;
                    cluster_n   += 1;
                }
            }
            count.min(4)
        }
    };
    // EE hint (squad size from EE.log) is authoritative when OCR word-count undercounts.
    // e.g. 4-player run where OCR only sees 3 "Prime" tokens → use 4 from hint so that
    // hardcoded_card_centers(4) spreads columns wide enough to separate adjacent cards.
    let word_card_count = (prime_count + forma_count)
        .max(ocr_cluster_count)
        .max(hint_squad_size.unwrap_or(0))
        .clamp(1, 4);

    // ── 2c. Assign OCR lines to card columns ──────────────────────────────────
    // Use bar centers only when:
    //   • count matches prime+forma (guards against partial detection), AND
    //   • centers pass the spacing sanity check (guards against false positives
    //     from card artwork — orange/gold item renders trigger is_bar_pixel and
    //     produce bunched centers like [0.37, 0.50, 0.62, 0.71] instead of the
    //     expected even spread [0.24, 0.41, 0.59, 0.76]).
    let bars_trusted = !card_centers.is_empty()
        && card_centers.len() == word_card_count
        && bar_centers_are_valid(&card_centers);
    let active_centers: Vec<f32> = if bars_trusted {
        card_centers.clone()
    } else {
        hardcoded_card_centers(word_card_count)
    };

    // ── Raw OCR lines log (all lines, accepted + skipped with reason) ────────
    let raw_ocr_log: String = {
        let mut lines_log = Vec::new();
        for (i, (text, x, y)) in ocr_lines.iter().enumerate() {
            let tl = text.to_lowercase();
            let skip = if *y < 0.10 {
                Some(format!("y={:.2} < 0.10 top-HUD cutoff", y))
            } else if *y >= ocr_y_max {
                Some(format!("y={:.2} >= {:.2} below-bar cutoff", y, ocr_y_max))
            } else if is_player_name(text) {
                Some("player name".into())
            } else if is_ui_badge(text) {
                Some("UI badge".into())
            } else if tl.contains("booster") || tl.contains("relic opened") || tl.contains("endless bonus") {
                Some("endless bonus UI".into())
            } else {
                None
            };
            let entry = match skip {
                Some(r) => format!("  [{:>2}] {:>4} x={:.2} y={:.2}  ✗ {} — \"{}\"",
                    i, "", x, y, r, text.trim()),
                None    => format!("  [{:>2}] {:>4} x={:.2} y={:.2}  ✓ \"{}\"",
                    i, "", x, y, text.trim()),
            };
            lines_log.push(entry);
        }
        lines_log.join("\n")
    };

    let columns: Vec<(Vec<String>, f32)> = {
        let mut cols: Vec<(Vec<String>, f32)> =
            active_centers.iter().map(|&cx| (Vec::new(), cx)).collect();
        for (text, x, y) in &ocr_lines {
            if *y < 0.10 || *y >= ocr_y_max || is_player_name(text) || is_ui_badge(text) { continue; }
            let idx = active_centers.iter().enumerate()
                .min_by(|(_, a), (_, b)| {
                    (x - *a).abs().partial_cmp(&(x - *b).abs())
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .map(|(i, _)| i)
                .unwrap_or(0);
            cols[idx].0.push(text.clone());
        }
        cols
    };

    // ── 3a. Per-card matching (only when rarity bars gave reliable columns) ─────
    // X-gap fallback columns are unreliable: OCR clusters all right-side card text
    // into the same column (wrong X positions), so per-column matching on fallback
    // columns produces wrong items. Only use per-column when bars were detected.
    let mut items: Vec<String> = Vec::new();
    let mut positions: Vec<f32> = Vec::new();

    let (_bar_y_frac, have_bars) = match &bar_result {
        Some((_, by)) => (*by, true),
        None => (0.0f32, false),
    };

    let mut col_match_log: Vec<String> = Vec::new();

    for (col_idx, (col_texts, cx)) in columns.iter().enumerate() {
        if items.len() >= active_centers.len() { break; }
        let words = build_word_set(col_texts);

        // Log what OCR text this column contains
        let col_preview: Vec<&str> = col_texts.iter().take(4).map(|s| s.trim()).collect();
        if words.is_empty() {
            col_match_log.push(format!(
                "  Col[{}] x={:.2}: (no words) — skipped\n    OCR: {:?}",
                col_idx, cx, col_preview));
            continue;
        }

        // ── Text-based scoring ───────────────────────────────────────────────
        let mut best_score = 0.0f32;
        let mut best_word_count = 0usize;
        let mut best_unique: Option<String> = None;
        let mut top3: Vec<(f32, String)> = Vec::new(); // for logging
        for (unique_name, display_name) in catalog {
            if display_name.len() < 5 { continue; }
            let s = score_item(display_name, &words);
            let wc = normalise(display_name).split_whitespace().count();
            if s > best_score || (s >= best_score - 1e-6 && wc > best_word_count) {
                best_score = s;
                best_word_count = wc;
                best_unique = Some(unique_name.clone());
            }
            // Collect top-3 for logging (keep sorted, max 3)
            if s > 0.0 {
                top3.push((s, display_name.clone()));
                top3.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
                top3.truncate(3);
            }
        }
        let top3_str = top3.iter()
            .map(|(s, n)| format!("{:.2} \"{}\"", s, n))
            .collect::<Vec<_>>().join(" · ");

        // ── Icon-based fallback when text match is weak ──────────────────────
        let mut icon_log = String::new();
        if best_score < 0.67 && have_bars {
            let bar_y = _bar_y_frac;
            let half_w = if columns.len() > 1 { 0.56 / columns.len() as f32 / 2.0 } else { 0.10 };
            let icon_type = classify_card_icon(
                pixels, pix_w, pix_h,
                (cx - half_w).max(0.0), (cx + half_w).min(1.0), bar_y
            );
            let name_words = extract_item_name_words(&words);
            let component_filter: Option<&str> = match &icon_type {
                IconType::Component(c) => Some(c),
                IconType::Forma        => Some("forma"),
                IconType::FullModel    => Some("blueprint"),
                IconType::Unknown      => None,
            };
            icon_log = format!("\n    Icon: text={:.2} < 0.67 → classifier={:?}{}",
                best_score, icon_type,
                component_filter.map(|c| format!(" suffix=\"{}\"", c)).unwrap_or_default());

            if let Some(comp) = component_filter {
                let comp_norm = normalise(comp);
                let mut icon_best_score = 0.0f32;
                let mut icon_best_unique: Option<String> = None;
                let mut icon_top3: Vec<(f32, String)> = Vec::new();
                for (unique_name, display_name) in catalog {
                    if display_name.len() < 5 { continue; }
                    let dn = normalise(display_name);
                    if !dn.contains(comp_norm.as_str()) { continue; }
                    let name_matched = name_words.iter()
                        .filter(|nw| dn.contains(nw.as_str()))
                        .count();
                    let s = if name_words.is_empty() { 0.5 }
                            else { name_matched as f32 / name_words.len() as f32 };
                    if s > icon_best_score {
                        icon_best_score = s;
                        icon_best_unique = Some(unique_name.clone());
                    }
                    if s > 0.0 {
                        icon_top3.push((s, display_name.clone()));
                        icon_top3.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
                        icon_top3.truncate(3);
                    }
                }
                let icon_top3_str = icon_top3.iter()
                    .map(|(s, n)| format!("{:.2} \"{}\"", s, n))
                    .collect::<Vec<_>>().join(" · ");
                icon_log += &format!("\n    Icon top3: {}", icon_top3_str);
                if icon_best_score >= 0.4 {
                    icon_log += &format!("\n    Icon accepted: {:.2} → \"{}\"",
                        icon_best_score,
                        icon_best_unique.as_ref().and_then(|u| catalog.iter().find(|(k,_)| k==u)).map(|(_,n)| n.as_str()).unwrap_or("?"));
                    best_score = icon_best_score;
                    best_unique = icon_best_unique;
                } else {
                    icon_log += "\n    Icon rejected (score < 0.40)";
                }
            }
        }

        // ── Log this column ──────────────────────────────────────────────────
        let best_display = best_unique.as_ref()
            .and_then(|u| catalog.iter().find(|(k, _)| k == u))
            .map(|(_, n)| n.as_str())
            .unwrap_or("—");
        let col_preview: Vec<&str> = col_texts.iter().map(|s| s.trim()).collect();
        let words_str: String = {
            let mut ws: Vec<&str> = words.iter().map(|s| s.as_str()).collect();
            ws.sort();
            ws.join(", ")
        };
        col_match_log.push(format!(
            "  Col[{}] x={:.2}: score={:.2} → \"{}\"\n    OCR: {:?}\n    Words: {{{}}}\n    Top3: {}{}",
            col_idx, cx, best_score, best_display, col_preview, words_str, top3_str, icon_log
        ));

        // Require 0.67 for per-column. Items where only "prime"+"blueprint" match
        // score exactly 0.667 (still rejected). A specific word matched via suffix
        // or Levenshtein + one generic word scores ≥0.69 and is now accepted,
        // preventing the fallback which can cross-contaminate words from other columns.
        if best_score < 0.67 {
            // Unknown item (WFCD not yet updated or OCR garbled).
            // Emit raw OCR text with a "?:" prefix so the overlay can still show something.
            let raw = col_texts.iter()
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .take(3)
                .collect::<Vec<_>>()
                .join(" ");
            if !raw.is_empty() {
                items.push(format!("?:{}", raw));
                positions.push(*cx);
            }
            continue;
        }
        let unique = match best_unique { Some(u) => u, None => continue };
        // No dedup here — each column is a distinct physical card.
        // Two players cracking the same relic legitimately show the same reward twice.
        // The `seen` set is only used in section 3b (full-frame fallback) where we
        // don't have column separation.
        items.push(unique);
        positions.push(*cx);
        let _ = col_idx;
    }

    // ── 3b. Full-frame fill ───────────────────────────────────────────────────
    // Determine expected card count — take the max of all three signals so that
    // any one reliable source prevents early lock-in:
    //   • EE.log squad size  (ground truth when available)
    //   • prime+forma count  (fuzzy word count from OCR)
    //   • rarity bar count   (visual, only when bars passed spacing validation)
    // IMPORTANT: only include bar count when bars_trusted. Rejected bars can give
    // wrong counts (e.g. 4 bars detected on a 3-card screen) that keep the OCR
    // loop retrying forever on a number it can never reach.
    let estimated_cards = hint_squad_size
        .unwrap_or(0)
        .max(word_card_count)
        .max(if bars_trusted { card_centers.len() } else { 0 })
        .max(1);

    if items.len() < estimated_cards {
        // Apply the same y-range filter used in column matching: exclude top-HUD
        // lines (y < 0.10) AND lines below the rarity bars (y >= ocr_y_max).
        // Without the bar cutoff, player names just below the bars (e.g. "HAR180::")
        // leak into the word set and produce false item matches like "Harrow Prime Blueprint".
        let all_words = build_word_set(
            &ocr_lines.iter()
                .filter(|(_, _, y)| *y >= 0.10 && *y < ocr_y_max)
                .map(|(t, _, _)| t.clone())
                .collect::<Vec<_>>()
        );

        // Words that appear in almost every reward and carry no item-specific
        // information. Excluded when finding which OCR line "anchors" each item
        // (for left-to-right ordering), but still used in scoring.
        const GENERIC: &[&str] = &["prime", "owned", "crafted", "blueprint"];

        // Find candidates with score ≥ 0.80 and sort by their first OCR line index.
        // OCR reads left-to-right, so line index approximates screen position.
        // Example: "Dual Zoren Prime Blueprint" → key word "zoren" → OCR line 1
        //          "Forma Blueprint"             → key word "forma"  → OCR line 4
        //          "Venato Prime Handle"         → key word "venato" → OCR line 6
        // Sorting by these indices gives the correct left→right overlay order
        // without requiring accurate X positions from OCR bounding rects.
        let mut candidates: Vec<(usize, f32, usize, String)> = Vec::new(); // (line_idx, score, name_len, unique)
        for (unique_name, display_name) in catalog {
            if display_name.len() < 5 { continue; }
            let s = score_item(display_name, &all_words);
            if s < 0.80 { continue; }

            let norm_dn = normalise(display_name);
            let key_words: Vec<&str> = norm_dn.split_whitespace()
                .filter(|w| w.len() >= 4 && !GENERIC.contains(w))
                .collect();

            // Find the earliest OCR line that contains one of this item's key words
            let first_line = if key_words.is_empty() {
                500usize // no unique identifier → sort after items with known positions
            } else {
                ocr_lines.iter().enumerate()
                    .find(|(_, (line_text, _, _))| {
                        let lt = normalise(line_text);
                        key_words.iter().any(|&w| lt.contains(w))
                    })
                    .map(|(i, _)| i)
                    .unwrap_or(999) // not found in OCR → last priority
            };

            candidates.push((first_line, s, display_name.len(), unique_name.clone()));
        }
        // Primary: OCR line order (left → right). Secondary: score. Tertiary: name length.
        candidates.sort_by(|a, b|
            a.0.cmp(&b.0)
                .then(b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal))
                .then(b.2.cmp(&a.2))
        );

        // Seed base-name dedup from items already found by per-column matching.
        // Also track per-column duplicate counts: an item that appeared in N different
        // columns is legitimately repeated N times (4 players cracking the same relic).
        // We only re-allow it in the fill if it genuinely appeared multiple times.
        let mut seen_bases: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut per_col_counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        for un in &items {
            *per_col_counts.entry(un.clone()).or_insert(0) += 1;
            if let Some((_, dn)) = catalog.iter().find(|(u, _)| u == un) {
                let norm = normalise(dn);
                let ws: Vec<&str> = norm.split_whitespace().collect();
                if ws.len() >= 2 { seen_bases.insert(ws[..ws.len()-1].join(" ")); }
            }
        }

        for (_, _, _, unique) in candidates {
            if items.len() >= estimated_cards { break; }
            let dn = match catalog.iter().find(|(u, _)| *u == unique) {
                Some((_, n)) => n.clone(),
                None => continue,
            };
            let dk = normalise(&dn);
            let current_count = items.iter().filter(|u| *u == &unique).count();
            let col_count = per_col_counts.get(&unique).copied().unwrap_or(0);
            let is_exact_duplicate = current_count > 0;
            let ws: Vec<&str> = dk.split_whitespace().collect();

            if is_exact_duplicate {
                // Only allow adding another copy if per-column matching confirmed
                // the same item in ≥2 columns (genuine multi-player duplicate).
                // Prevents filling missing-column gaps with re-copies of already-found items.
                if col_count < 2 || current_count >= col_count { continue; }
            } else {
                // Sibling dedup: block a DIFFERENT item from the same base name
                // (e.g. "Dual Zoren Prime Handle" blocked if "Dual Zoren Prime Blueprint" found)
                if ws.len() >= 2 {
                    let base = ws[..ws.len()-1].join(" ");
                    if seen_bases.contains(&base) { continue; }
                    seen_bases.insert(base);
                }
            }
            items.push(unique);
        }

        // Assign positions using the estimated card count for even spacing.
        // Cards are evenly distributed across the central ~70% of the screen.
        if !items.is_empty() {
            let n = estimated_cards.max(items.len());
            let spacing = 0.70 / (n as f32 + 1.0);
            positions = (0..items.len())
                .map(|i| 0.15 + spacing * (i as f32 + 1.0))
                .collect();
        }
    }

    // ── Diagnostic string ─────────────────────────────────────────────────────
    let col_mode = if bars_trusted { "bar columns (validated)" }
                   else if have_bars { "hardcoded (bars rejected)" }
                   else { "hardcoded (no bars)" };
    let ff_items: Vec<&str> = items.iter().map(|s| {
        catalog.iter().find(|(u,_)| u == s).map(|(_,n)| n.as_str()).unwrap_or(s.as_str())
    }).collect();
    // is_complete = true means "found all cards expected for this squad size".
    // lib.rs uses this to decide when to stop retrying OCR.
    let is_complete = !items.is_empty() && items.len() >= estimated_cards;
    let expected_src = match (hint_squad_size, !card_centers.is_empty()) {
        (Some(h), _) if h >= word_card_count && h >= card_centers.len() => "EE.log",
        (_, true) if card_centers.len() >= word_card_count => "bars",
        _ if ocr_cluster_count > prime_count + forma_count => "x-clusters",
        _ => "prime+forma",
    };
    let ee_hint_str = match hint_squad_size {
        Some(n) => format!("{} players (from EE.log)", n),
        None    => "(not available — VoidProjections sequence not seen yet)".into(),
    };
    let debug = format!(
        "├─ Capture  : {}\n\
         ├─ OCR      : {} chars, {} lines\n\
         ├─ Bars     : {}\n\
         ├─ Prime/Forma: {}p + {}f + {}x = {} cards\n\
         ├─ EE hint  : {}\n\
         ├─ Expected : {} cards (from {}){}\n\
         ├─ Raw lines:\n{}\n\
         ├─ Match    : {} — {} formed\n\
         {}\n\
         └─ Items    : {:?}",
        capture_info,
        raw_full.len(), ocr_lines.len(),
        bar_diag,
        prime_count, forma_count, ocr_cluster_count, word_card_count,
        ee_hint_str,
        estimated_cards, expected_src,
        if is_complete { " ✅ complete" } else { " ⚡ partial" },
        raw_ocr_log,
        col_mode, columns.len(),
        col_match_log.join("\n"),
        ff_items,
    );

    (is_complete, false, items, positions, debug)
}



/// Captures the full desktop for diagnostics so the FrameForge overlay is visible.
/// Uses GDI BitBlt from the desktop DC — DWM composites all windows (including the
/// WebView2 overlay) before BitBlt reads them, so the overlay appears in the BMP.
/// DXGI captures GPU output before DWM overlay compositing and misses WebView2 windows.
/// Returns (BGRA pixels, width, height) or None on failure.
#[cfg(target_os = "windows")]
pub fn capture_desktop_for_diag() -> Option<(Vec<u8>, u32, u32)> {
    if let Some((pixels, w, h)) = capture_screen_gdi_scaled(1, 1) {
        if avg_brightness(&pixels) >= 5 {
            return Some((pixels, w, h));
        }
    }
    // Fallback: DXGI (fullscreen exclusive — overlay not visible there anyway).
    if let Some((pixels, w, cap_h, _full_h)) = capture_dxgi(1.0) {
        return Some((pixels, w, cap_h));
    }
    None
}

#[cfg(not(target_os = "windows"))]
pub fn capture_desktop_for_diag() -> Option<(Vec<u8>, u32, u32)> { None }

#[cfg(not(target_os = "windows"))]
pub fn capture_warframe_reward_area() -> Option<(Vec<u8>, u32, u32, u32, String)> { None }

#[cfg(not(target_os = "windows"))]
pub fn run_windows_ocr(_bmp: Vec<u8>, _w: u32, _h: u32) -> Result<(String, Vec<(String, f32, f32)>), String> {
    Err("Windows only".into())
}

#[cfg(not(target_os = "windows"))]
pub fn extract_reward_items_twophase(
    _pixels: &[u8], _w: u32, _cap_h: u32, _full_h: u32,
    _catalog: &[(String, String)], _capture_info: &str,
    _hint_squad_size: Option<usize>, _player_names: &[String],
) -> (bool, bool, Vec<String>, Vec<f32>, String) {
    (false, false, vec![], vec![], String::new())
}
