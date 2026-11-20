use std::collections::HashMap;
use std::path::PathBuf;
use tauri::{Emitter, Manager, State};

use crate::app_state::AppState;
use crate::wfm::to_wfm_slug;
use crate::{cache, refresh};

/// Delete the bulk price cache and re-fetch from FrameForgePricing.
/// Updates both relics_run_prices and the WFM price cache in-place.
#[tauri::command]
pub(crate) async fn refresh_bulk_prices(app: tauri::AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let _ = std::fs::remove_file(&state.relics_run_prices_cache_path);

    let (by_name, by_slug) = tauri::async_runtime::spawn_blocking(fetch_relics_run_data)
        .await
        .map_err(|e| e.to_string())?;

    if by_name.is_empty() {
        return Err("Failed to fetch bulk prices — check your internet connection.".to_string());
    }

    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let j = serde_json::json!({ "date": today, "by_name": &by_name, "by_slug": &by_slug });
    if let Ok(s) = serde_json::to_string(&j) {
        let _ = std::fs::write(&state.relics_run_prices_cache_path, s);
    }

    *state.relics_run_prices.lock().map_err(|e| e.to_string())? = by_name;
    for (slug, price) in by_slug {
        state.wfm.cache_price(slug, Some(price));
    }
    let _ = app.emit(crate::events::BULK_PRICES_UPDATED, ());
    Ok(())
}

/// Return the in-memory bulk price map (lowercase display name → plat).
/// Loaded at startup from relics_run_prices.json and refreshed daily.
#[tauri::command]
pub(crate) fn get_bulk_prices(state: State<'_, AppState>) -> HashMap<String, u32> {
    state.relics_run_prices.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Load today's relics.run price cache from disk.
/// Returns (by_name, by_slug) or None if missing/stale.
pub(crate) fn load_relics_run_cache(path: &PathBuf) -> Option<(HashMap<String, u32>, HashMap<String, u32>)> {
    let s = std::fs::read_to_string(path).ok()?;
    let v: serde_json::Value = serde_json::from_str(&s).ok()?;
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    if v.get("date").and_then(|d| d.as_str()) != Some(today.as_str()) { return None; }
    let by_name: HashMap<String, u32> = serde_json::from_value(v["by_name"].clone()).ok()?;
    let by_slug: HashMap<String, u32> = serde_json::from_value(v["by_slug"].clone()).ok()?;
    Some((by_name, by_slug))
}

const PRICING_BASE: &str = "https://raw.githubusercontent.com/WyrmStudios/FrameForgePricing/main";

/// Fetch items.json + today's price_history from the FrameForgePricing mirror.
/// Returns (by_name, by_slug):
///   by_name: item display name (lowercase) → median sell price  (for get_item_price)
///   by_slug: authoritative WFM slug         → median sell price  (for wfm_price_cache)
#[tracing::instrument(level = "debug", skip_all)]
pub(crate) fn fetch_relics_run_data() -> (HashMap<String, u32>, HashMap<String, u32>) {
    // items.json gives the authoritative name → WFM slug mapping for every tradeable item.
    let name_to_slug: HashMap<String, String> = ureq::get(&format!("{}/items.json", PRICING_BASE))
        .call().ok()
        .and_then(|r| r.into_json::<Vec<serde_json::Value>>().ok())
        .unwrap_or_default()
        .into_iter()
        .filter_map(|v| {
            let name = v["i18n"]["en"]["name"].as_str()?.to_lowercase();
            let slug = v["slug"].as_str()?.to_string();
            Some((name, slug))
        })
        .collect();

    let price_json: serde_json::Value = ureq::get(
        &format!("{}/price_history_latest.json", PRICING_BASE)
    ).call().ok().and_then(|r| r.into_json().ok()).unwrap_or_default();

    let mut by_name: HashMap<String, u32> = HashMap::new();
    let mut by_slug: HashMap<String, u32> = HashMap::new();

    if let Some(obj) = price_json.as_object() {
        for (name, records) in obj {
            let price = records.as_array()
                .and_then(|arr| arr.iter()
                    .find(|r| r["order_type"].as_str() == Some("closed"))
                    .and_then(|r| r["median"].as_f64()));
            if let Some(p) = price {
                let price_u32 = p.round() as u32;
                let name_lower = name.to_lowercase();
                // Use authoritative slug from items.json; heuristic fallback for unknown items.
                let slug = name_to_slug.get(&name_lower)
                    .cloned()
                    .unwrap_or_else(|| to_wfm_slug(&name_lower));
                by_name.insert(name_lower, price_u32);
                by_slug.insert(slug, price_u32);
            }
        }
    }

    (by_name, by_slug)
}

pub fn refresh_bulk_prices_task(app: &tauri::AppHandle, _force: bool) -> Result<(), String> {
    let state = app.state::<AppState>();
    let path = state.relics_run_prices_cache_path.clone();
    let _ = std::fs::remove_file(&path);
    let (by_name, by_slug) = fetch_relics_run_data();
    if by_name.is_empty() {
        return Err("bulk prices fetch returned no data".to_string());
    }
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let j = serde_json::json!({ "date": today, "by_name": &by_name, "by_slug": &by_slug });
    if let Ok(s) = serde_json::to_string(&j) {
        let _ = std::fs::write(&path, s);
    }
    *state.relics_run_prices.lock().unwrap_or_else(|e| e.into_inner()) = by_name;
    for (slug, price) in by_slug {
        state.wfm.cache_price(slug, Some(price));
    }
    let _ = app.emit(crate::events::BULK_PRICES_UPDATED, ());
    Ok(())
}

// ─── Cache status commands ────────────────────────────────────────────────────

#[tauri::command]
pub(crate) fn get_cache_statuses() -> HashMap<String, cache::CacheStatus> {
    cache::statuses()
}

#[tauri::command]
pub(crate) fn refresh_all_caches() {
    refresh::force_all();
}
