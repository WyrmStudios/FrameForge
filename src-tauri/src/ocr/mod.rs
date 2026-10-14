//! Screen capture + OCR for Warframe relic reward detection.
//!
//! This module separates platform-specific capture/OCR code (`windows.rs`, `linux.rs`)
//! from platform-agnostic domain logic (preprocessing, BMP encoding, word matching,
//! catalog matching, and reward item extraction).

#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "linux")]
mod linux;

// ─── Re-exports from platform modules ────────────────────────────────────────

#[cfg(target_os = "windows")]
pub use windows::*;

#[cfg(target_os = "linux")]
pub use linux::*;

// ─── Domain logic (platform-agnostic) ────────────────────────────────────────

/// Grayscale + contrast stretch on BGRA pixels.
/// Converting to grayscale is the key step: element icons (Cold, Heat, Toxin)
/// are colored glyphs — in the original BGRA image WinRT OCR rejects these lines as
/// graphics. After grayscale they become neutral-brightness shapes, so OCR reads the
/// white text on either side of the icon instead of dropping the whole line.
pub fn preprocess_for_ocr(pixels: &[u8], width: u32, height: u32) -> (Vec<u8>, u32, u32) {
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
        bmp.extend(std::iter::repeat_n(0, padding as usize));
    }
    bmp
}

// ─── Structs ─────────────────────────────────────────────────────────────────

pub struct MatchParams<'a> {
    pub pixels: &'a [u8],
    pub pix_w: u32,
    pub pix_h: u32,
    pub raw_full: &'a str,
    pub ocr_lines: &'a [(String, f32, f32)],
    pub catalog: &'a [(String, String)],
    pub capture_info: &'a str,
    pub hint_squad_size: Option<usize>,
    pub player_names: &'a [String],
}

/// What the card icon looks like, used to constrain catalog matching.
#[derive(Debug, Clone, PartialEq)]
pub enum IconType {
    /// Generic REUSED component shape — same icon appears across many primes.
    Component(&'static str),
    /// Full 3D model of a unique warframe or weapon.
    FullModel,
    /// Forma spiral (distinctively blue)
    Forma,
    /// Could not classify
    Unknown,
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
pub fn word_found_in_set(
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
    if catalog_word.len() >= 6 {
        let suffix_len = (catalog_word.len() / 2).max(5);
        let suffix = &catalog_word[catalog_word.len() - suffix_len..];
        if ocr_words.iter().any(|w| w.find(suffix).is_some_and(|p| p != 1)) { return true; }
    }

    // Edit budget by word length
    let max_dist = if catalog_word.len() >= 8 {
        2
    } else if catalog_word.len() >= 5 {
        1
    } else {
        0
    };
    let wb = catalog_word.as_bytes();
    for ocr_w in ocr_words {
        if ocr_w.len() >= 4 {
            let dist = lev_dist(catalog_word, ocr_w);
            let len_diff = (catalog_word.len() as isize - ocr_w.len() as isize).unsigned_abs();
            if dist <= max_dist && !(len_diff == dist && len_diff >= 2) { return true; }
        }
        let ob = ocr_w.as_bytes();
        if ob.len() >= wb.len() + 4 {
            for (win_start, win) in ob.windows(wb.len()).enumerate() {
                let errs = wb.iter().zip(win.iter()).filter(|(a, b)| a != b).count();
                if errs == 0 && win_start + wb.len() == ob.len() && win_start >= 3 { continue; }
                if errs <= max_dist { return true; }
            }
        }
    }
    false
}

// ─── Catalog matching ─────────────────────────────────────────────────────────

/// Normalise OCR text for catalog matching.
pub fn normalise(s: &str) -> String {
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
pub fn find_rarity_bars(pixels: &[u8], pix_w: u32, pix_h: u32) -> (Option<(Vec<f32>, f32)>, String) {
    let x_lo = (pix_w as f32 * 0.05) as u32;
    let x_hi = (pix_w as f32 * 0.95) as u32;
    let y_lo = (pix_h as f32 * 0.55) as u32;
    let y_hi = (pix_h as f32 * 0.97) as u32;

    let scan_w = (x_hi - x_lo) as usize;

    #[inline]
    fn is_bar_pixel(b: u32, g: u32, r: u32) -> bool {
        let lum = (r + g + b) / 3;
        if lum < 25 { return false; }
        let is_orange = r > 80  && r > b + 20;
        let is_teal   = b > 65  && g > 50  && b > r + 8;
        let is_gold   = r > 100 && g > 80  && b < r.saturating_sub(10);
        let max_ch = r.max(g).max(b);
        let min_ch = r.min(g).min(b);
        let is_bright = lum > 160 && max_ch - min_ch < 50;
        is_orange || is_teal || is_gold || is_bright
    }

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

    let col_threshold = (max_col / 4).max(2);
    let mut lit: Vec<bool> = col_score.iter().map(|&s| s >= col_threshold).collect();

    let bridge = (scan_w / 100).max(3);
    {
        let mut xi = 0;
        while xi < scan_w {
            if !lit[xi] {
                let gap_start = xi;
                while xi < scan_w && !lit[xi] { xi += 1; }
                let gap_len = xi - gap_start;
                if gap_len <= bridge && gap_start > 0 && xi < scan_w {
                    lit[gap_start..xi].fill(true);
                }
            } else {
                xi += 1;
            }
        }
    }

    let min_band = (scan_w / 150).max(6);
    let mut bands: Vec<(usize, usize)> = Vec::new();
    let mut in_band = false;
    let mut band_start = 0usize;
    for (xi, &is_lit) in lit.iter().enumerate().take(scan_w) {
        match (is_lit, in_band) {
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

    let lit_xs: Vec<u32> = (0..scan_w as u32)
        .filter(|&xi| lit[xi as usize])
        .map(|xi| x_lo + xi)
        .collect();

    let mut best_row_y = (y_lo + y_hi) / 2;
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

// ─── Icon component classifier ────────────────────────────────────────────────

/// Classify the reward card icon using an 8×8 spatial brightness grid.
pub fn classify_card_icon(
    pixels: &[u8], pix_w: u32, pix_h: u32,
    x_left: f32, x_right: f32, bar_y: f32,
) -> IconType {
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
        if cnt[gy][gx] > 0 {
            lum [gy][gx] /= cnt[gy][gx] as f32;
            blue[gy][gx] /= cnt[gy][gx] as f32;
        }
    }}

    let total_cells = G * G;
    let lit_cells = lum.iter().flatten().filter(|&&l| l > 60.0).count();
    let fill_ratio = lit_cells as f32 / total_cells as f32;

    let mut min_x = G; let mut max_x = 0usize;
    let mut min_y = G; let mut max_y = 0usize;
    for (gy, row) in lum.iter().enumerate() { for (gx, &l) in row.iter().enumerate() {
        if l > 60.0 {
            min_x = min_x.min(gx); max_x = max_x.max(gx);
            min_y = min_y.min(gy); max_y = max_y.max(gy);
        }
    }}
    let bb_w = if max_x >= min_x { (max_x - min_x + 1) as f32 } else { 0.0 };
    let bb_h = if max_y >= min_y { (max_y - min_y + 1) as f32 } else { 0.0 };
    let aspect = if bb_h > 0.0 { bb_w / bb_h } else { 0.0 };

    let mut sum_y = 0.0f32;
    let mut sum_lum = 0.0f32;
    for (gy, row) in lum.iter().enumerate() { for &l in row.iter() {
        sum_y += l * gy as f32;
        sum_lum += l;
    }}
    let cm_y = if sum_lum > 0.0 { sum_y / sum_lum / (G - 1) as f32 } else { 0.5 };

    let mut left_sum = 0.0f32;
    let mut right_sum = 0.0f32;
    for row in lum.iter() { for &l in &row[..G/2] { left_sum += l; } }
    for row in lum.iter() { for &l in &row[G/2..] { right_sum += l; } }
    let symmetry = if left_sum + right_sum > 0.0 {
        left_sum.min(right_sum) / left_sum.max(right_sum)
    } else { 0.0 };

    let blue_avg: f32 = blue.iter().flatten().sum::<f32>() / total_cells as f32;
    let lum_avg: f32 = lum.iter().flatten().sum::<f32>() / total_cells as f32;
    let blue_dom = if lum_avg > 0.0 { blue_avg / lum_avg } else { 0.0 };

    // Forma: blue spiral
    if blue_dom > 0.55 && fill_ratio > 0.15 {
        return IconType::Forma;
    }
    // Full model: high fill + even spread
    if fill_ratio > 0.45 && aspect > 0.6 && aspect < 1.8 {
        return IconType::FullModel;
    }
    // neuroptics: bright top half, symmetric, roughly square
    if cm_y < 0.45 && symmetry > 0.7 && aspect > 0.7 && aspect < 1.3 {
        return IconType::Component("neuroptics");
    }
    // systems: bright central region, compact
    if cm_y > 0.35 && cm_y < 0.65 && fill_ratio > 0.25 && fill_ratio < 0.55 {
        return IconType::Component("systems");
    }
    // chassis: large central region, wider, lower CoM
    if cm_y > 0.45 && fill_ratio > 0.35 && aspect > 1.0 {
        return IconType::Component("chassis");
    }
    // barrel: wide aspect ratio
    if aspect > 1.5 {
        return IconType::Component("barrel");
    }
    // handle: tall aspect ratio
    if aspect < 0.5 && bb_h > 3.0 {
        return IconType::Component("handle");
    }
    // blade: low symmetry, moderate aspect
    if symmetry < 0.5 && fill_ratio > 0.2 {
        return IconType::Component("blade");
    }
    // upper/lower limb: low fill, arc-shaped
    if fill_ratio < 0.2 && bb_h > 2.0 {
        return IconType::Component("limb");
    }

    IconType::Unknown
}

// ─── Text matching helpers ────────────────────────────────────────────────────

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

fn bar_centers_are_valid(centers: &[f32]) -> bool {
    let n = centers.len();
    if n == 0 { return false; }
    if centers[0] < 0.15 || centers[n - 1] > 0.90 { return false; }
    if n < 2 { return true; }
    for pair in centers.windows(2) {
        if pair[1] - pair[0] < 0.08 { return false; }
    }
    if n >= 3 {
        let gaps: Vec<f32> = centers.windows(2).map(|p| p[1] - p[0]).collect();
        let mean = gaps.iter().sum::<f32>() / gaps.len() as f32;
        if gaps.iter().any(|g| (g - mean).abs() > 0.04) { return false; }
    }
    let span = centers[n - 1] - centers[0];
    let expected = match n {
        2 => 0.34f32,
        3 => 0.46,
        _ => 0.52,
    };
    (span - expected).abs() < 0.10
}

fn hardcoded_card_centers(n: usize) -> Vec<f32> {
    match n {
        1 => vec![0.50],
        2 => vec![0.435, 0.565],
        3 => vec![0.37, 0.50, 0.63],
        _ => vec![0.31, 0.44, 0.56, 0.69],
    }
}

/// Groups x-positions into clusters: a value joins the running cluster while it
/// stays within `gap` of that cluster's mean. Returns each cluster's mean, left to right.
fn cluster_x_centers(mut xs: Vec<f32>, gap: f32) -> Vec<f32> {
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mut clusters: Vec<(f32, usize)> = Vec::new(); // (sum, count)
    for x in xs {
        match clusters.last_mut() {
            Some((sum, n)) if x - *sum / *n as f32 <= gap => { *sum += x; *n += 1; }
            _ => clusters.push((x, 1)),
        }
    }
    clusters.into_iter().map(|(sum, n)| sum / n as f32).collect()
}

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
    let mut seen = std::collections::HashSet::new();
    let item_words: Vec<&str> = norm.split_whitespace()
        .filter(|&w| seen.insert(w))
        .collect();
    if item_words.is_empty() { return 0.0; }
    let n_catalog = item_words.len() as f32;
    let n_ocr = words.len() as f32;
    let matched = item_words.iter()
        .filter(|&&w| word_found_in_set(w, words))
        .count();

    // A full catalog match scores 1.0. A partial match is capped at 0.9, so an
    // extra unmatched catalog word (e.g. "Chassis" on "Lavos Prime Chassis
    // Blueprint") can't beat the exact name via the length bonus below.
    let base = if matched == item_words.len() {
        1.0
    } else {
        (matched as f32 / n_catalog)
            .max(if n_ocr > 0.0 { matched as f32 / n_ocr } else { 0.0 })
            * 0.9
    };

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

/// Post-OCR reward matching: rarity bars → card columns → catalog match → fill.
pub fn match_reward_items(
    params: MatchParams<'_>,
) -> (bool, bool, Vec<String>, Vec<f32>, String) {
    let MatchParams { pixels, pix_w, pix_h, raw_full, ocr_lines, catalog, capture_info, hint_squad_size, player_names } = params;

    let (bar_result, bar_diag) = find_rarity_bars(pixels, pix_w, pix_h);

    let (card_centers, _bar_y_frac): (Vec<f32>, f32) = match &bar_result {
        Some((centers, by)) => (centers.clone(), *by),
        None => (vec![], 0.0),
    };

    let ocr_y_max: f32 = 0.57;

    let is_player_name = |text: &str| -> bool {
        let t = text.trim().to_lowercase();
        if t.is_empty() { return false; }
        for name in player_names {
            let n = name.to_lowercase();
            if t.contains(&n) || n.contains(&t) { return true; }
            let max_len = t.len().max(n.len());
            let threshold = (max_len / 5).max(1);
            if lev_dist(&t, &n) <= threshold { return true; }
        }
        false
    };

    let is_ui_badge = |text: &str| -> bool {
        const BADGE_WORDS: &[&str] = &["owned", "crafted", "unranked", "mastered"];
        let meaningful: Vec<&str> = text.split_whitespace()
            .filter(|w| !w.starts_with('@') && w.parse::<u32>().is_err()
                    && w.len() > 1
                    && w.chars().any(|c| c.is_alphabetic()))
            .collect();
        !meaningful.is_empty()
            && meaningful.iter().all(|w| BADGE_WORDS.contains(&w.to_lowercase().as_str()))
    };

    let is_prime_like = |w: &str| -> bool {
        if w.starts_with("prim") && w.len() >= 4 { return true; }
        if w == "pri" { return true; }
        if w.len() >= 3 && w.len() <= 9 { return lev_dist(w, "prime") <= 1; }
        false
    };
    let is_forma_like = |w: &str| -> bool {
        if w == "forma" { return true; }
        if w.len() >= 3 && w.len() <= 7 { return lev_dist(w, "forma") <= 1; }
        false
    };

    // Title lines that survive the y/badge/player-name filters, by x-position.
    let title_lines: Vec<&str> = ocr_lines.iter()
        .filter(|(t, _, y)| t.trim().len() >= 3 && *y >= 0.10 && *y < ocr_y_max && !is_player_name(t) && !is_ui_badge(t))
        .map(|(t, _, _)| t.as_str())
        .collect();
    let title_xs: Vec<f32> = ocr_lines.iter()
        .filter(|(t, _, y)| t.trim().len() >= 3 && *y >= 0.10 && *y < ocr_y_max && !is_player_name(t) && !is_ui_badge(t))
        .map(|(_, x, _)| *x)
        .collect();
    // Count prime/forma keywords only in title-row text, not the full OCR blob.
    // Hover-tooltip descriptions and duplicate badge-style lines below the bar
    // (y >= ocr_y_max) also say "Prime"/"Forma" and previously inflated this
    // count above the true number of cards, making the estimated card count
    // (and therefore is_complete) unreachable — the retry loop would then spin
    // for the full timeout without ever handing off to the overlay.
    let raw_norm = normalise(&title_lines.join(" "));
    let prime_count = raw_norm.split_whitespace().filter(|&w| is_prime_like(w)).count();
    let forma_count  = raw_norm.split_whitespace().filter(|&w| is_forma_like(w)).count();
    let title_cluster_centers = cluster_x_centers(title_xs, 0.10);
    let ocr_cluster_count: usize = title_cluster_centers.len().min(4);
    let word_card_count = (prime_count + forma_count)
        .max(ocr_cluster_count)
        .max(hint_squad_size.unwrap_or(0))
        .clamp(1, 4);

    let bars_trusted = !card_centers.is_empty()
        && card_centers.len() == word_card_count
        && bar_centers_are_valid(&card_centers);
    // Fallback columns come from the title x-clusters, not a fixed 4-slot grid.
    // A wrapped title ("Dethcube Prime" / "Carapace") shares one cluster, so it
    // stays on one card. The fixed grid split it across two columns.
    let active_centers: Vec<f32> = if bars_trusted {
        card_centers.clone()
    } else if !title_cluster_centers.is_empty() && title_cluster_centers.len() <= 4 {
        title_cluster_centers.clone()
    } else {
        hardcoded_card_centers(word_card_count)
    };

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
        for (text, x, y) in ocr_lines {
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

    let mut items: Vec<String> = Vec::new();
    let mut positions: Vec<f32> = Vec::new();

    let (_bar_y_frac, have_bars) = match &bar_result {
        Some((_, by)) => (*by, true),
        None => (0.0f32, false),
    };

    let mut col_match_log: Vec<String> = Vec::new();
    let mut any_low_confidence = false;

    for (col_idx, (col_texts, cx)) in columns.iter().enumerate() {
        if items.len() >= active_centers.len() { break; }
        let words = build_word_set(col_texts);

        let col_preview: Vec<&str> = col_texts.iter().take(4).map(|s| s.trim()).collect();
        if words.is_empty() {
            col_match_log.push(format!(
                "  Col[{}] x={:.2}: (no words) — skipped\n    OCR: {:?}",
                col_idx, cx, col_preview));
            continue;
        }

        let mut best_score = 0.0f32;
        let mut best_word_count = 0usize;
        let mut best_unique: Option<String> = None;
        let mut top3: Vec<(f32, String)> = Vec::new();
        for (unique_name, display_name) in catalog {
            if display_name.len() < 5 { continue; }
            let s = score_item(display_name, &words);
            let wc = normalise(display_name).split_whitespace().count();
            if s > best_score || (s >= best_score - 1e-6 && wc > best_word_count) {
                best_score = s;
                best_word_count = wc;
                best_unique = Some(unique_name.clone());
            }
            if s > 0.0 {
                top3.push((s, display_name.clone()));
                top3.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
                top3.truncate(3);
            }
        }
        let top3_str = top3.iter()
            .map(|(s, n)| format!("{:.2} \"{}\"", s, n))
            .collect::<Vec<_>>().join(" · ");

        let mut icon_log = String::new();
        let mut icon_accepted = false;
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
                    icon_accepted = true;
                } else {
                    icon_log += "\n    Icon rejected (score < 0.40)";
                }
            }
        }

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

        if best_score < 0.67 {
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

        // A "confident" score can still come from a sparse read: if matched/n_ocr
        // hit 1.0 off only 1-2 OCR words, base score alone can't tell two same-length
        // catalog candidates apart (see the "Wisp Prime Neuroptics" vs "...Chassis"
        // false positive this was written for). Only trust the score outright when
        // every word of the matched display name was actually seen in OCR, or the
        // icon classifier independently confirmed it — otherwise flag for a retry.
        let full_word_match = catalog.iter().find(|(k, _)| *k == unique)
            .map(|(_, dn)| {
                let norm = normalise(dn);
                let mut seen = std::collections::HashSet::new();
                let iw: Vec<&str> = norm.split_whitespace().filter(|&w| seen.insert(w)).collect();
                !iw.is_empty() && iw.iter().all(|&w| word_found_in_set(w, &words))
            })
            .unwrap_or(false);
        if !icon_accepted && !full_word_match {
            any_low_confidence = true;
        }

        items.push(unique);
        positions.push(*cx);
        let _ = col_idx;
    }

    let estimated_cards = hint_squad_size
        .unwrap_or(0)
        .max(word_card_count)
        .max(if bars_trusted { card_centers.len() } else { 0 })
        .max(1);

    let fill_limit = estimated_cards.min(
        columns.iter().filter(|(t, _)| !build_word_set(t).is_empty()).count()
    );

    if items.len() < fill_limit {
        let all_words = build_word_set(
            &ocr_lines.iter()
                .filter(|(_, _, y)| *y >= 0.10 && *y < ocr_y_max)
                .map(|(t, _, _)| t.clone())
                .collect::<Vec<_>>()
        );

        const GENERIC: &[&str] = &["prime", "owned", "crafted", "blueprint"];

        let mut candidates: Vec<(usize, f32, usize, String)> = Vec::new();
        for (unique_name, display_name) in catalog {
            if display_name.len() < 5 { continue; }
            let s = score_item(display_name, &all_words);
            if s < 0.80 { continue; }

            let norm_dn = normalise(display_name);
            let key_words: Vec<&str> = norm_dn.split_whitespace()
                .filter(|w| w.len() >= 4 && !GENERIC.contains(w))
                .collect();

            let first_line = if key_words.is_empty() {
                500usize
            } else {
                ocr_lines.iter().enumerate()
                    .find(|(_, (line_text, _, _))| {
                        let lt = normalise(line_text);
                        key_words.iter().any(|&w| lt.contains(w))
                    })
                    .map(|(i, _)| i)
                    .unwrap_or(999)
            };

            candidates.push((first_line, s, display_name.len(), unique_name.clone()));
        }
        candidates.sort_by(|a, b|
            a.0.cmp(&b.0)
                .then(b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal))
                .then(b.2.cmp(&a.2))
        );

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
            if items.len() >= fill_limit { break; }
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
                if col_count < 2 || current_count >= col_count { continue; }
            } else {
                if ws.len() >= 2 {
                    let base = ws[..ws.len()-1].join(" ");
                    if seen_bases.contains(&base) { continue; }
                    seen_bases.insert(base);
                }
            }
            items.push(unique);
        }

        if !items.is_empty() {
            let n = estimated_cards.max(items.len());
            let spacing = 0.70 / (n as f32 + 1.0);
            positions = (0..items.len())
                .map(|i| 0.15 + spacing * (i as f32 + 1.0))
                .collect();
        }
    }

    let col_mode = if bars_trusted { "bar columns (validated)" }
                   else if have_bars { "hardcoded (bars rejected)" }
                   else { "hardcoded (no bars)" };
    let ff_items: Vec<&str> = items.iter().map(|s| {
        catalog.iter().find(|(u,_)| u == s).map(|(_,n)| n.as_str()).unwrap_or(s.as_str())
    }).collect();
    let n_confirmed = items.iter().filter(|s| !s.starts_with("?:")).count();
    let is_complete = n_confirmed > 0 && n_confirmed >= estimated_cards;
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
         ├─ Expected : {} cards (from {}){}{}\n\
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
        if any_low_confidence { " ⚠ low-confidence card(s) — retrying" } else { "" },
        raw_ocr_log,
        col_mode, columns.len(),
        col_match_log.join("\n"),
        ff_items,
    );

    (is_complete, any_low_confidence, items, positions, debug)
}

/// Relic reward detection — the main entry point for OCR-based reward extraction.
#[cfg(target_os = "windows")]
pub fn extract_reward_items_twophase(
    params: super::OcrParams<'_>,
) -> (bool, bool, Vec<String>, Vec<f32>, String) {
    let super::OcrParams { pixels, pix_w, pix_h, game_h: _game_h, catalog, capture_info, hint_squad_size, player_names } = params;

    let (raw_full, ocr_lines) =
        match windows::run_windows_ocr(to_bmp(pixels, pix_w, pix_h), pix_w, pix_h) {
            Ok(r) => r,
            Err(e) => return (false, false, vec![], vec![],
                format!("├─ Capture  : {}\n└─ OCR error: {}", capture_info, e)),
        };
    if raw_full.len() < 4 {
        let _ = std::fs::write(
            std::env::temp_dir().join("frameforge_capture_debug.bmp"),
            to_bmp(pixels, pix_w, pix_h),
        );
        let avg = windows::avg_brightness(pixels);
        let kind = if avg < 30 { "dark-frame" } else { "ocr-empty" };
        return (false, false, vec![], vec![], format!(
            "├─ Capture  : {}\n└─ OCR      : returned no text ({}, avg={})\n   Saved: %TEMP%\\frameforge_capture_debug.bmp",
            capture_info, kind, avg
        ));
    }

    {
        let lower = raw_full.to_lowercase();
        const QUALITY: &[&str] = &["intact", "exceptional", "flawless", "radiant"];
        // The reward card screen's own header names the cracked relic (e.g. "Axi D6
        // Intact"), which previously tripped this same check and caused the real
        // reward screen to be skipped for its entire lifetime. The pre-mission/
        // inter-rotation relic PICKER screen is the thing this check is meant to
        // catch, and it lists several of the player's relics at once — so require
        // at least two quality-word hits (one single mention is just the reward
        // screen's own header) before treating it as the picker and skipping.
        let quality_hits: usize = QUALITY.iter()
            .map(|q| lower.matches(q).count())
            .sum();
        if lower.contains(" relic") && quality_hits >= 2 {
            return (false, true, vec![], vec![], format!(
                "├─ Capture  : {}\n└─ OCR      : relic selection screen detected (skipped)",
                capture_info
            ));
        }
    }

    match_reward_items(MatchParams {
        pixels, pix_w, pix_h, raw_full: &raw_full, ocr_lines: &ocr_lines,
        catalog, capture_info, hint_squad_size, player_names,
    })
}

#[cfg(not(target_os = "windows"))]
pub fn extract_reward_items_twophase(
    _params: super::OcrParams<'_>,
) -> (bool, bool, Vec<String>, Vec<f32>, String) {
    (false, false, vec![], vec![], "OCR not supported on this platform".into())
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn ocr_words(words: &[&str]) -> std::collections::HashSet<String> {
        words.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn four_character_words_must_be_read_exactly() {
        for (catalog_word, on_screen) in [
            ("limb", "limbo"),
            ("gara", "galatine"),
            ("khra", "khora"),
            ("star", "stars"),
        ] {
            assert!(
                !word_found_in_set(catalog_word, &ocr_words(&[on_screen])),
                "{catalog_word:?} must not match {on_screen:?}"
            );
        }
    }

    #[test]
    fn longer_words_keep_their_edit_tolerance() {
        assert!(word_found_in_set("blueprint", &ocr_words(&["bluepnnt"])));
        assert!(word_found_in_set("tenora", &ocr_words(&["tenova"])));
        assert!(word_found_in_set("limb", &ocr_words(&["lim"])));
    }

    #[test]
    fn bar_centers_must_be_evenly_spaced() {
        assert!(!bar_centers_are_valid(&[0.204, 0.316, 0.655]));
        assert!(bar_centers_are_valid(&[0.27, 0.50, 0.73]));
        assert!(bar_centers_are_valid(&[0.24, 0.41, 0.59, 0.76]));
    }

    #[test]
    fn a_hovered_card_tooltip_does_not_fabricate_a_fourth_reward() {
        let lines: &[(&str, f32, f32)] = &[
            ("0", 0.05, 0.01),
            ("99%", 0.09, 0.01),
            ("C", 0.11, 0.01),
            ("15%", 0.15, 0.01),
            ("53\"", 0.16, 0.01),
            ("——— = VOID FISSURE/REWARDS", 0.19, 0.08),
            ("3", 0.50, 0.19),
            ("& Crafted", 0.59, 0.28),
            ("® Owned", 0.34, 0.28),
            ("® Owned", 0.46, 0.28),
            ("Lavos Prime Chassis", 0.50, 0.49),
            ("Lex Prime Barrel", 0.37, 0.52),
            ("Blueprint", 0.50, 0.52),
            ("2 X Forma Blueprint", 0.61, 0.52),
            ("LEX PRIME BARREL", 0.32, 0.57),
            ("teOwl12 5a", 0.52, 0.59),
            ("Falcon1719+", 0.62, 0.59),
            ("N", 0.47, 0.64),
            ("©@ 1 Owned", 0.30, 0.63),
            ("A prime weapon-crafting component.", 0.34, 0.70),
            ("Can be exchanged for", 0.33, 0.76),
            ("15 Ducats", 0.43, 0.76),
            ("Steel Path Bonus", 0.53, 0.77),
            ("+1 Steel Essence", 0.53, 0.80),
            ("Endless Bonus Affinity Booster | 1 Relic Opened", 0.51, 0.89),
        ];
        let ocr_lines: Vec<(String, f32, f32)> =
            lines.iter().map(|(t, x, y)| (t.to_string(), *x, *y)).collect();
        let raw_full = lines.iter().map(|(t, _, _)| *t).collect::<Vec<_>>().join(" ");

        let catalog: Vec<(String, String)> = [
            "Lex Prime Barrel", "Lex Prime Blueprint", "Lex Prime Receiver",
            "Lavos Prime Blueprint", "Lavos Prime Chassis Blueprint",
            "Lavos Prime Neuroptics Blueprint", "Lavos Prime Systems Blueprint",
            "Forma Blueprint", "2X Forma Blueprint",
            "Trinity Prime Blueprint", "Trinity Prime Chassis Blueprint",
            "Trinity Prime Neuroptics Blueprint", "Trinity Prime Systems Blueprint",
            "Atlas Prime Chassis Blueprint", "Acceltra Prime Barrel",
            "Afuris Prime Barrel", "Boltor Prime Barrel",
        ]
        .iter()
        .map(|n| (n.to_string(), n.to_string()))
        .collect();

        let player_names: Vec<String> = [
            "Vireo_", "q-lox", "grimlo1994", "Duo-vertex",
            "yubblenix", "TheVortexKnave1", "Falcon1719", "PrivateOwl12",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();

        let pixels = vec![0u8; 8 * 8 * 4];

        let (_complete, _skip, items, _positions, diag) = match_reward_items(MatchParams {
            pixels: &pixels, pix_w: 8, pix_h: 8, raw_full: &raw_full, ocr_lines: &ocr_lines,
            catalog: &catalog, capture_info: "replay", hint_squad_size: None, player_names: &player_names,
        });

        let fold = |n: &str| n.trim_start_matches("2X ").to_string();
        let mut got: Vec<String> = items.iter().map(|n| fold(n)).collect();
        got.sort();
        assert_eq!(
            got,
            vec![
                "Forma Blueprint".to_string(),
                "Lavos Prime Chassis Blueprint".to_string(),
                "Lex Prime Barrel".to_string(),
            ],
            "expected exactly the three real rewards, no fabricated fourth\n{diag}"
        );
    }
}
