//! One-off benchmark comparing WinRT `Windows.Media.Ocr` against the `ocrs`
//! pure-Rust fallback engine on real captured relic-reward diagnostic
//! screenshots. Not part of the normal suite (network + hardcoded folder) —
//! run explicitly:
//!   cargo test --lib ocr_bench -- --ignored --nocapture
#![cfg(test)]

use std::path::Path;
use std::time::Instant;

/// Parse width/height out of the 54-byte BMP header `to_bmp`/`write_bmp` produce,
/// rather than trusting a hardcoded resolution.
fn read_bmp_dims(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < 26 || &bytes[0..2] != b"BM" {
        return None;
    }
    let w = i32::from_le_bytes(bytes[18..22].try_into().ok()?) as u32;
    let h_raw = i32::from_le_bytes(bytes[22..26].try_into().ok()?);
    Some((w, h_raw.unsigned_abs()))
}

#[test]
#[ignore]
fn compare_winrt_vs_ocrs_on_real_screenshots() {
    let root = Path::new(r"C:\Users\Jochem\Desktop\ScreenShots Test");
    assert!(root.exists(), "benchmark folder not found: {root:?}");

    // ocrs downloads its two model files here on first use (~10 MB, one-time).
    let data_dir = std::env::temp_dir().join("frameforge_ocr_bench_models");
    crate::ocr_fallback::set_data_dir(data_dir);

    let mut entries: Vec<_> = std::fs::read_dir(root)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .collect();
    entries.sort_by_key(|e| e.file_name());

    let mut report = String::new();
    let mut winrt_times: Vec<u128> = Vec::new();
    let mut ocrs_times: Vec<u128> = Vec::new();
    let mut winrt_lines_total = 0usize;
    let mut ocrs_lines_total = 0usize;
    let mut winrt_errors = 0usize;
    let mut ocrs_errors = 0usize;
    let mut compared = 0usize;

    for entry in entries {
        let folder = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        let bmp_path = folder.join("screenshot.bmp");
        if !bmp_path.exists() {
            continue;
        }
        let bytes = match std::fs::read(&bmp_path) {
            Ok(b) => b,
            Err(e) => {
                report.push_str(&format!("[{name}] read error: {e}\n"));
                continue;
            }
        };
        let Some((w, h)) = read_bmp_dims(&bytes) else {
            report.push_str(&format!("[{name}] could not parse BMP header\n"));
            continue;
        };

        compared += 1;
        report.push_str(&format!("\n== {name} ({w}x{h}) ==\n"));

        let t0 = Instant::now();
        let winrt = crate::ocr::run_windows_ocr(bytes.clone(), w, h);
        let winrt_ms = t0.elapsed().as_millis();
        winrt_times.push(winrt_ms);

        let t1 = Instant::now();
        let ocrs = crate::ocr_fallback::run_ocrs(&bytes, w, h);
        let ocrs_ms = t1.elapsed().as_millis();
        ocrs_times.push(ocrs_ms);

        match &winrt {
            Ok((_, lines)) => {
                winrt_lines_total += lines.len();
                report.push_str(&format!("  WinRT ({winrt_ms:>6} ms, {} lines):\n", lines.len()));
                for (text, x, y) in lines {
                    report.push_str(&format!("    x={x:.2} y={y:.2}  \"{text}\"\n"));
                }
            }
            Err(e) => {
                winrt_errors += 1;
                report.push_str(&format!("  WinRT ({winrt_ms:>6} ms): ERROR {e}\n"));
            }
        }

        match &ocrs {
            Ok((_, lines)) => {
                ocrs_lines_total += lines.len();
                report.push_str(&format!("  ocrs  ({ocrs_ms:>6} ms, {} lines):\n", lines.len()));
                for (text, x, y) in lines {
                    report.push_str(&format!("    x={x:.2} y={y:.2}  \"{text}\"\n"));
                }
            }
            Err(e) => {
                ocrs_errors += 1;
                report.push_str(&format!("  ocrs  ({ocrs_ms:>6} ms): ERROR {e}\n"));
            }
        }
    }

    let avg = |v: &[u128]| {
        if v.is_empty() {
            0
        } else {
            v.iter().sum::<u128>() / v.len() as u128
        }
    };
    report.push_str(&format!(
        "\n\n==== SUMMARY ====\n\
         Screenshots compared : {compared}\n\
         WinRT : avg {} ms, total lines found {winrt_lines_total}, errors {winrt_errors}\n\
         ocrs  : avg {} ms, total lines found {ocrs_lines_total}, errors {ocrs_errors}\n",
        avg(&winrt_times),
        avg(&ocrs_times),
    ));

    let out_path = std::env::temp_dir().join("frameforge_ocr_bench_report.txt");
    std::fs::write(&out_path, &report).unwrap();
    println!("{report}");
    println!("\nFull report written to {}", out_path.display());
}
