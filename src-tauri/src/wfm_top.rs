use std::collections::HashMap;

use tauri::{Emitter, Manager, State};
use tracing::{info, warn};

use crate::app_state::AppState;
use crate::events;
use crate::wfm::{sanitize_display_name, to_wfm_slug, WfmTopItem};
use crate::wfcd::WfcdItem;

// ── Top WFM items by 7-day trade volume ───────────────────────────────────────

#[derive(serde::Serialize, serde::Deserialize)]
struct WfmTopDiskCache {
    saved_at: u64,          // Unix seconds
    items: Vec<WfmTopItem>,
}

#[derive(serde::Serialize, Clone)]
struct WfmTopProgress {
    completed: usize,
    total: usize,
    refreshing: bool,
}

/// Guards against concurrent scans: only one get_wfm_top_items scan runs at a time.
/// Concurrent callers wait (polling the cache) rather than starting a second scan.
/// Scan orchestration is the command's concern; the cached result it produces
/// lives in `Wfm`.
static WFM_SCAN_RUNNING: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Display name, warframe.market slug, image file — the three facts the scan
/// needs about each arcane candidate.
type ArcaneCandidate = (String, String, Option<String>);

pub(crate) fn start_wfm_top_scan(app: tauri::AppHandle) {
    use std::sync::atomic::Ordering;
    if WFM_SCAN_RUNNING.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).is_err() {
        return;
    }

    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        let previous = state.wfm.top_items();
        let refreshing = previous.is_some();
        let disk_cache_path = state.wfm_top_cache_path.clone();
        // One lock for both lookups: the scan needs the arcane list and the
        // name → image index, and a second lock here would deadlock.
        let (arcane_candidates, image_index): (Vec<ArcaneCandidate>, HashMap<String, String>) =
            match state.wfcd_items.lock() {
                Ok(items) => (
                    items.iter()
                        .filter(|i| i.category == "Arcanes")
                        .map(|i| (sanitize_display_name(&i.name), to_wfm_slug(&i.name), i.image_name.clone()))
                        .collect(),
                    index_images(&items),
                ),
                Err(e) => {
                    WFM_SCAN_RUNNING.store(false, Ordering::SeqCst);
                    warn!("WFM top-items scan could not read the catalog: {e}");
                    return;
                }
            };
        let wfm = state.wfm.clone();
        let progress_app = app.clone();
        let scan_result = tokio::task::spawn_blocking(move || {
            let prime_sets = wfm.prime_sets();
            let total = prime_sets.len() + arcane_candidates.len();
            let mut completed = 0;
            let mut out: Vec<WfmTopItem> = Vec::new();
            let report_progress = |completed| {
                let _ = progress_app.emit(events::WFM_TOP_PROGRESS, WfmTopProgress { completed, total, refreshing });
            };
            report_progress(completed);

            for (name, url_name) in &prime_sets {
                if let Some((price, daily_vol)) = wfm.stats_7day(url_name) {
                    out.push(WfmTopItem {
                        name: name.clone(), url_name: url_name.clone(), image_name: None,
                        unit_price: price, daily_volume: daily_vol,
                        total_value_7d: (price as f64 * daily_vol * 7.0) as u64,
                    });
                }
                completed += 1;
                report_progress(completed);
            }

            for (name, slug, image_name) in &arcane_candidates {
                if let Some((price, daily_vol)) = wfm.stats_7day(slug) {
                    out.push(WfmTopItem {
                        name: name.clone(), url_name: slug.clone(), image_name: image_name.clone(),
                        unit_price: price, daily_volume: daily_vol,
                        total_value_7d: (price as f64 * daily_vol * 7.0) as u64,
                    });
                }
                completed += 1;
                report_progress(completed);
            }
            out.sort_by_key(|b| std::cmp::Reverse(b.total_value_7d));
            out.truncate(10);
            backfill_images(&mut out, &image_index);
            out
        }).await;

        WFM_SCAN_RUNNING.store(false, Ordering::SeqCst);
        let results = match scan_result {
            Ok(results) => results,
            Err(error) => {
                warn!("WFM top-items scan failed to join: {error}");
                return;
            }
        };
        if results.is_empty() {
            warn!("WFM top-items scan returned no market data");
            return;
        }

        let changed = previous.as_ref().is_none_or(|old| {
            old.iter().map(|i| &i.url_name).ne(results.iter().map(|i| &i.url_name))
        });
        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
        if let Ok(json) = serde_json::to_string(&WfmTopDiskCache { saved_at: now_secs, items: results.clone() }) {
            let _ = std::fs::write(&disk_cache_path, json);
        }
        state.wfm.set_top_items(results.clone());
        info!(changed, "WFM top-items ranking refreshed");
        let _ = app.emit(events::WFM_TOP_UPDATED, results);
    });
}

/// Catalogue display name (lower-cased) → image file, for the items that have one.
fn index_images(items: &[WfcdItem]) -> HashMap<String, String> {
    items.iter()
        .filter_map(|i| Some((i.name.to_lowercase(), i.image_name.clone()?)))
        .collect()
}

/// The same index, taken straight from the already-loaded catalogue.
fn image_index(state: &AppState) -> HashMap<String, String> {
    match state.wfcd_items.lock() {
        Ok(items) => index_images(&items),
        Err(e) => {
            warn!("WFM top-items image lookup could not read the catalog: {e}");
            HashMap::new()
        }
    }
}

/// Prime sets are not catalogue entries of their own, so a set's picture is its
/// parent item's — "Citrine Prime Set" → "Citrine Prime". That's the same
/// catalogue entry the market's set cards use, so the file is already covered
/// by the image cache prewarm and served from the local image server.
fn backfill_images(items: &mut [WfmTopItem], index: &HashMap<String, String>) {
    for item in items.iter_mut() {
        if item.image_name.is_some() { continue; }
        let lower = item.name.to_lowercase();
        let Some(base) = lower.strip_suffix(" set") else { continue };
        item.image_name = index.get(base).cloned();
    }
}

/// Return the top 10 most-traded items on warframe.market by 7-day total value.
/// Queries Prime Sets and Arcanes from the local WFCD catalog (already loaded).
/// Results are cached for 3 hours so repeated tab opens are instant.
#[tauri::command]
pub(crate) async fn get_wfm_top_items(app: tauri::AppHandle, state: State<'_, AppState>) -> Result<Vec<WfmTopItem>, String> {
    const TOP_TTL: std::time::Duration = std::time::Duration::from_secs(3 * 3600);

    // In-memory cache, fresh within the TTL — the client owns it.
    if let Some(items) = state.wfm.cached_top_items(TOP_TTL) {
        return Ok(items);
    }

    // Load a disk result even when stale so it remains visible during refresh.
    let disk_cache_path = state.wfm_top_cache_path.clone();
    if let Ok(s) = std::fs::read_to_string(&disk_cache_path) {
        if let Ok(dc) = serde_json::from_str::<WfmTopDiskCache>(&s) {
            if !dc.items.is_empty() {
                // A cache written before the image lookup existed still serves
                // the old rows; fill them in rather than waiting out the TTL.
                let mut dc = dc;
                backfill_images(&mut dc.items, &image_index(&state));
                let now_secs = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
                let fresh = now_secs.saturating_sub(dc.saved_at) < TOP_TTL.as_secs();
                state.wfm.set_top_items(dc.items.clone());
                if fresh { return Ok(dc.items); }
            }
        }
    }

    if let Some(items) = state.wfm.top_items() {
        start_wfm_top_scan(app);
        return Ok(items);
    }

    start_wfm_top_scan(app);
    for _ in 0..150u32 {
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        if let Some(items) = state.wfm.cached_top_items(TOP_TTL) { return Ok(items); }
    }
    Err("WFM top items scan timed out".to_string())
}

pub fn refresh_wfm_top(app: &tauri::AppHandle, force: bool) -> Result<(), String> {
    const TOP_TTL: std::time::Duration = std::time::Duration::from_secs(3 * 3600);
    if !force && app.state::<AppState>().wfm.cached_top_items(TOP_TTL).is_some() {
        return Ok(());
    }
    start_wfm_top_scan(app.clone());
    Ok(())
}
