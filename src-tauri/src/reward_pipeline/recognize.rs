//! Rarity-bar detection, icon classification, and catalog matching — turns raw
//! OCR output + captured pixels into a list of matched reward unique-names.

use super::events::{AttemptDiag, BarsDiag, ColumnDiag, IconDiag, OcrDiag, OcrLineDiag};
use super::matching::{
    bar_centers_are_valid, build_word_set, cluster_x_centers, extract_item_name_words,
    hardcoded_card_centers, lev_dist, normalise, score_item, word_found_in_set,
};

struct MatchParams<'a> {
    pixels: &'a [u8],
    pix_w: u32,
    pix_h: u32,
    raw_full: &'a str,
    ocr_lines: &'a [(String, f32, f32)],
    catalog: &'a [(String, String)],
    capture_info: &'a str,
    hint_squad_size: Option<usize>,
    /// Where `hint_squad_size` came from: 0 = none, 1 = relic-count guess at
    /// trigger time, 2 = EE.log VoidProjections handshake. Diagnostics only —
    /// decides the label printed in the attempt's debug block so a guessed
    /// squad size is never reported as an EE.log fact.
    hint_source: u8,
    player_names: &'a [String],
}

/// What the card icon looks like, used to constrain catalog matching.
#[derive(Debug, Clone, PartialEq)]
enum IconType {
    /// Generic REUSED component shape — same icon appears across many primes.
    Component(&'static str),
    /// Full 3D model of a unique warframe or weapon.
    FullModel,
    /// Forma spiral (distinctively blue)
    Forma,
    /// Could not classify
    Unknown,
}

#[derive(Clone, Copy)]
enum OcrLineSkip {
    TopHud,
    BelowBar,
    PlayerName,
    UiBadge,
    EndlessBonus,
}

/// Scan the captured image for the coloured rarity bars below each reward card.
/// Returns (card_x_centers, bar_y_frac) where centers are fractions of image width.
fn find_rarity_bars(pixels: &[u8], pix_w: u32, pix_h: u32) -> (Option<(Vec<f32>, f32)>, String) {
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

/// Classify the reward card icon using an 8×8 spatial brightness grid.
fn classify_card_icon(
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

/// Post-OCR reward matching: rarity bars → card columns → catalog match → fill.
///
/// Returns `(complete, low_confidence, items, positions, diag)`; `diag` carries
/// the full structured outcome (bars, columns, per-column scores/flags, raw OCR)
/// that gets serialized flat into the attempt's JSONL event.
fn match_reward_items(
    params: MatchParams<'_>,
) -> (bool, bool, Vec<String>, Vec<f32>, AttemptDiag) {
    let MatchParams { pixels, pix_w, pix_h, raw_full, ocr_lines, catalog, capture_info, hint_squad_size, hint_source, player_names } = params;

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
    let line_skip = |text: &str, y: f32| -> Option<OcrLineSkip> {
        if y < 0.10 {
            Some(OcrLineSkip::TopHud)
        } else if y >= ocr_y_max {
            Some(OcrLineSkip::BelowBar)
        } else if is_player_name(text) {
            Some(OcrLineSkip::PlayerName)
        } else if is_ui_badge(text) {
            Some(OcrLineSkip::UiBadge)
        } else {
            let text = text.to_lowercase();
            (text.contains("booster") || text.contains("relic opened") || text.contains("endless bonus"))
                .then_some(OcrLineSkip::EndlessBonus)
        }
    };

    let raw_norm = normalise(raw_full);
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
    let prime_count = raw_norm.split_whitespace().filter(|&w| is_prime_like(w)).count();
    let forma_count  = raw_norm.split_whitespace().filter(|&w| is_forma_like(w)).count();

    // Title lines that are eligible for card matching, by x-position.
    let title_xs: Vec<f32> = ocr_lines.iter()
        .filter(|(t, _, y)| t.trim().len() >= 3 && line_skip(t, *y).is_none())
        .map(|(_, x, _)| *x)
        .collect();
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
    // `col_source` names the branch actually taken so the attempt event never
    // claims "hardcoded" when cluster fallback did the work (and vice versa).
    let (active_centers, col_source): (Vec<f32>, &'static str) = if bars_trusted {
        (card_centers.clone(), "bar_columns")
    } else if !title_cluster_centers.is_empty() && title_cluster_centers.len() <= 4 {
        (title_cluster_centers.clone(), "title_x_clusters")
    } else {
        (hardcoded_card_centers(word_card_count), "hardcoded_grid")
    };

    let raw_ocr_lines: Vec<OcrLineDiag> = ocr_lines.iter().enumerate()
        .map(|(i, (text, x, y))| {
            let skip = match line_skip(text, *y) {
                Some(OcrLineSkip::TopHud) => Some(format!("y={y:.2} < 0.10 top-HUD cutoff")),
                Some(OcrLineSkip::BelowBar) => Some(format!("y={y:.2} >= {ocr_y_max:.2} below-bar cutoff")),
                Some(OcrLineSkip::PlayerName) => Some("player name".into()),
                Some(OcrLineSkip::UiBadge) => Some("UI badge".into()),
                Some(OcrLineSkip::EndlessBonus) => Some("endless bonus UI".into()),
                None => None,
            };
            OcrLineDiag {
                i,
                x: *x,
                y: *y,
                text: text.trim().to_string(),
                used: skip.is_none(),
                skip,
            }
        })
        .collect();

    let columns: Vec<(Vec<String>, f32)> = {
        let mut cols: Vec<(Vec<String>, f32)> =
            active_centers.iter().map(|&cx| (Vec::new(), cx)).collect();
        for (text, x, y) in ocr_lines {
            if line_skip(text, *y).is_some() { continue; }
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
    let mut filled: Vec<String> = Vec::new();

    let (bar_y, have_bars) = match &bar_result {
        Some((_, by)) => (Some(*by), true),
        None => (None, false),
    };

    let mut column_diags: Vec<ColumnDiag> = Vec::new();
    let mut any_low_confidence = false;

    for (col_idx, (col_texts, cx)) in columns.iter().enumerate() {
        if items.len() >= active_centers.len() { break; }
        let words = build_word_set(col_texts);

        let col_preview: Vec<String> = col_texts.iter().take(4).map(|s| s.trim().to_string()).collect();
        if words.is_empty() {
            column_diags.push(ColumnDiag {
                x: *cx,
                score: 0.0,
                item: None,
                unknown: None,
                ocr_words: 0,
                ocr: col_preview,
                top3: Vec::new(),
                evidence: None,
                missing: Vec::new(),
                flags: vec!["no_words".into()],
                icon: None,
            });
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
        let mut icon_diag: Option<IconDiag> = None;
        let mut icon_accepted = false;
        if best_score < 0.67 && have_bars {
            let bar_y = bar_y.unwrap_or(0.0);
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
            let mut diag = IconDiag {
                classifier: format!("{icon_type:?}"),
                filter: component_filter.map(str::to_string),
                score: 0.0,
                accepted: false,
                top3: Vec::new(),
            };

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
                diag.score = icon_best_score;
                diag.top3 = icon_top3;
                if icon_best_score >= 0.4 {
                    diag.accepted = true;
                    best_score = icon_best_score;
                    best_unique = icon_best_unique;
                    icon_accepted = true;
                }
            }
            icon_diag = Some(diag);
        }

        let best_display = best_unique.as_ref()
            .and_then(|u| catalog.iter().find(|(k, _)| k == u))
            .map(|(_, n)| n.as_str())
            .unwrap_or("—");
        // Word-level evidence: which catalog words of the chosen name were
        // actually seen in OCR, plus the score margin over the runner-up. A
        // sparse read off one generic word ("blueprint") with a near-zero
        // margin is the exact failure mode the plain score hides (0.93 vs
        // 0.92) — carried as flags so a reader can spot it without math.
        let dn_norm = normalise(best_display);
        let mut dn_seen = std::collections::HashSet::new();
        let dn_words: Vec<&str> = dn_norm.split_whitespace().filter(|&w| dn_seen.insert(w)).collect();
        let n_found = dn_words.iter().filter(|w| word_found_in_set(w, &words)).count();
        let missing: Vec<String> = dn_words.iter()
            .filter(|w| !word_found_in_set(w, &words))
            .map(|w| (*w).to_string())
            .collect();
        let margin = if top3.len() >= 2 { Some(top3[0].0 - top3[1].0) } else { None };
        let mut flags: Vec<String> = Vec::new();
        if best_score < 0.67 { flags.push("below_threshold".into()); }
        if words.len() <= 1 { flags.push("sparse".into()); }
        if margin.is_some_and(|m| m < 0.05) { flags.push("ambiguous".into()); }
        let mut diag = ColumnDiag {
            x: *cx,
            score: best_score,
            item: None,
            unknown: None,
            ocr_words: words.len(),
            ocr: col_texts.iter().map(|s| s.trim().to_string()).collect(),
            top3,
            evidence: (!dn_words.is_empty() && best_score > 0.0)
                .then_some((n_found, dn_words.len())),
            missing,
            flags,
            icon: icon_diag,
        };

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
                diag.unknown = Some(raw);
            }
            column_diags.push(diag);
            continue;
        }
        let unique = match best_unique { Some(u) => u, None => { column_diags.push(diag); continue; } };

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
            diag.flags.push("partial_words".into());
        } else if icon_accepted {
            diag.flags.push("icon_confirmed".into());
        }

        diag.item = Some(best_display.to_string());
        column_diags.push(diag);
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
                .filter(|(text, _, y)| line_skip(text, *y).is_none())
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
            items.push(unique.clone());
            filled.push(unique);
        }

        if !items.is_empty() {
            let n = estimated_cards.max(items.len());
            let spacing = 0.70 / (n as f32 + 1.0);
            positions = (0..items.len())
                .map(|i| 0.15 + spacing * (i as f32 + 1.0))
                .collect();
        }
    }

    let n_confirmed = items.iter().filter(|s| !s.starts_with("?:")).count();
    let is_complete = n_confirmed > 0 && n_confirmed >= estimated_cards;
    let expected_src = match (hint_squad_size, !card_centers.is_empty()) {
        (Some(h), _) if h >= word_card_count && h >= card_centers.len() => "squad_hint",
        (_, true) if card_centers.len() >= word_card_count => "bars",
        _ if ocr_cluster_count > prime_count + forma_count => "x_clusters",
        _ => "prime_forma",
    };
    // Label the hint with its real source: a guess made at trigger time from the
    // session's relic count is not an EE.log fact — reporting it as one was how
    // a solo/duo run ended up showing squad size 4 as an EE.log handshake.
    let hint_source = match hint_source {
        1 => "relic_guess",
        2 => "ee_handshake",
        0 => "none",
        _ => "unknown",
    };
    let display_items: Vec<String> = items.iter().map(|s| {
        catalog.iter().find(|(u, _)| u == s).map(|(_, n)| n.clone()).unwrap_or_else(|| s.clone())
    }).collect();
    let bars = BarsDiag {
        ok: have_bars,
        y: bar_y,
        segments: card_centers.len(),
        note: bar_diag,
    };
    let diag = AttemptDiag {
        capture_kind: "ok",
        capture: capture_info.to_string(),
        note: None,
        ocr: Some(OcrDiag {
            chars: raw_full.len(),
            lines: ocr_lines.len(),
            raw: raw_full.to_string(),
            text: raw_ocr_lines,
        }),
        bars,
        cols_source: col_source.to_string(),
        cols: columns.len(),
        hint_size: hint_squad_size,
        hint_source,
        expected: estimated_cards,
        expected_src,
        prime_cards: prime_count,
        forma_cards: forma_count,
        text_clusters: ocr_cluster_count,
        columns: column_diags,
        filled,
        display_items,
    };

    (is_complete, any_low_confidence, items, positions, diag)
}

/// Relic reward detection — the main entry point for OCR-based reward extraction.
#[cfg(target_os = "windows")]
pub(crate) fn extract_reward_items_twophase(
    params: super::OcrParams<'_>,
) -> (bool, bool, Vec<String>, Vec<f32>, AttemptDiag) {
    let super::OcrParams {
        pixels, pix_w, pix_h, game_h: _game_h, catalog, capture_info, hint_squad_size,
        hint_source, player_names, preprocess_text_pass,
    } = params;

    // Rarity-bar and icon classification below always read the raw `pixels` —
    // only the bytes handed to the text-recognition engine are swapped for a
    // preprocessed copy, since grayscale would destroy the hue checks those
    // use (see `preprocess_text_pass` doc comment on `OcrParams`).
    let text_bmp = if preprocess_text_pass {
        let (pre_pixels, pre_w, pre_h) = crate::ocr::preprocess_for_ocr(pixels, pix_w, pix_h);
        crate::ocr::to_bmp(&pre_pixels, pre_w, pre_h)
    } else {
        crate::ocr::to_bmp(pixels, pix_w, pix_h)
    };
    let (raw_full, ocr_lines) =
        match crate::ocr::run_windows_ocr(text_bmp, pix_w, pix_h) {
            Ok(r) => r,
            Err(e) => {
                let mut diag = AttemptDiag::capture_only("ocr_error", capture_info.to_string());
                diag.note = Some(e.to_string());
                return (false, false, vec![], vec![], diag);
            }
    };
    if raw_full.len() < 4 {
        let avg = crate::ocr::avg_brightness(pixels);
        let kind = if avg < 30 { "dark_frame" } else { "ocr_empty" };
        let mut diag = AttemptDiag::capture_only(kind, capture_info.to_string());
        diag.note = Some(format!("avg brightness {avg:.1}"));
        return (false, false, vec![], vec![], diag);
    }

    {
        // The real relic-selection screen lists each relic as a single tight
        // phrase, e.g. "Meso O1 Relic (Radiant)" — name, "Relic", and quality
        // all on one OCR line. Checking `raw_full` as a whole blob (two
        // independent `.contains()` calls, no proximity requirement) false-
        // positived on genuine reward-screen captures: the reward screen's
        // footer almost always reads "... | N Relics Opened", and once ANY
        // quality word turned up anywhere else in the frame (a tooltip, a
        // multi-relic endless run's relic tracker panel, stray OCR noise)
        // the two independent substrings both matched and this returned
        // early — permanently, since the same static background re-matches
        // every subsequent attempt, with no path back to confirming an
        // already-found soft-complete candidate. Requiring both substrings
        // on the same OCR line reproduces the real screen's phrase shape
        // without matching the reward screen's unrelated "Relics Opened".
        const QUALITY: &[&str] = &["intact", "exceptional", "flawless", "radiant"];
        let is_relic_select_line = |line: &str| {
            let lower = line.to_lowercase();
            lower.contains("relic") && QUALITY.iter().any(|q| lower.contains(q))
        };
        if ocr_lines.iter().any(|(text, _, _)| is_relic_select_line(text)) {
            return (false, true, vec![], vec![],
                AttemptDiag::capture_only("relic_select", capture_info.to_string()));
        }
    }

    match_reward_items(MatchParams {
        pixels, pix_w, pix_h, raw_full: &raw_full, ocr_lines: &ocr_lines,
        catalog, capture_info, hint_squad_size, hint_source, player_names,
    })
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn extract_reward_items_twophase(
    _params: super::OcrParams<'_>,
) -> (bool, bool, Vec<String>, Vec<f32>, AttemptDiag) {
    (false, false, vec![], vec![],
        AttemptDiag::capture_only("unsupported", "OCR not supported on this platform".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

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
            catalog: &catalog, capture_info: "replay", hint_squad_size: None, hint_source: 0,
            player_names: &player_names,
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
            "expected exactly the three real rewards, no fabricated fourth\n{}",
            serde_json::to_string_pretty(&diag).unwrap_or_default()
        );
    }
}
