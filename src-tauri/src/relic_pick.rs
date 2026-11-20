use std::collections::HashMap;
use tauri::{Emitter, Manager, State};
use tracing::{debug, info};

use crate::app_state::AppState;
use crate::events;

// ── Relic pick overlay ────────────────────────────────────────────────────────

/// One overlay's saved x/y offset (logical pixels) from settings.json.
/// Missing settings or missing axes mean 0, which keeps the built-in placement.
fn saved_offset(app: &tauri::AppHandle, prefix: &str) -> (f64, f64) {
    use tauri::Manager;
    let Some(state) = app.try_state::<AppState>() else { return (0.0, 0.0) };
    let Ok(map) = crate::settings::read_settings_map(&state.settings_path) else { return (0.0, 0.0) };
    let Some(obj) = map.get("overlayOffsets").and_then(|v| v.as_object()) else { return (0.0, 0.0) };
    let axis = |suffix: &str| -> f64 {
        obj.get(&format!("{prefix}{suffix}")).and_then(|v| v.as_f64()).unwrap_or(0.0)
    };
    (axis("X"), axis("Y"))
}

pub(crate) fn relic_pick_show(app: &tauri::AppHandle) {
    use tauri::Manager;
    let Some(win) = app.get_webview_window("relic-pick-overlay") else { return };
    // Position: right edge of the primary monitor, 20px from top.
    let (x, _dpi) = win.primary_monitor()
        .ok()
        .flatten()
        .map(|m| {
            let dpi = m.scale_factor();
            let w   = m.size().width as f64 / dpi;
            (w - 440.0, dpi)
        })
        .unwrap_or((1920.0 - 440.0, 1.0));
    // User offset on top of that default (issue #73).
    let (off_x, off_y) = saved_offset(app, "relicPick");
    let _ = win.set_position(tauri::Position::Logical(tauri::LogicalPosition { x: x + off_x, y: 20.0 + off_y }));
    let _ = win.show();
}

pub(crate) fn relic_pick_hide(app: &tauri::AppHandle) {
    use tauri::Manager;
    let Some(win) = app.get_webview_window("relic-pick-overlay") else { return };
    let _ = win.hide();
}

/// Debug: run OCR on the top-left quarter of the Warframe window and report the detected era.
#[tauri::command]
pub(crate) fn debug_detect_fissure_era() -> String {
    match crate::ocr::detect_fissure_era() {
        Some(era) => format!("Detected era: {}", era),
        None => "No era detected — OCR found no known fissure era label in the top-left quarter.".to_string(),
    }
}

/// Debug: manually fire the relic pick overlay for a given era.
#[tauri::command]
pub(crate) fn test_relic_pick_overlay(era: String, app: tauri::AppHandle) -> String {
    let payload = build_relic_pick_payload(&era, &app);
    let relic_count = payload["relics"].as_array().map_or(0, |a| a.len());
    relic_pick_show(&app);
    let _ = app.emit(events::RELIC_PICK_OPEN, &payload);
    format!("Emitted relic-pick-open: era={}, {} relics in inventory", era, relic_count)
}

/// Debug/preview: hide the relic pick overlay again (pairs with test_relic_pick_overlay).
#[tauri::command]
pub(crate) fn hide_relic_pick_overlay(app: tauri::AppHandle) {
    relic_pick_hide(&app);
}

/// Position and show the pick window without emitting a payload — used by the
/// outline toggle and by live offset changes, which only need the placement.
#[tauri::command]
pub(crate) fn show_relic_pick_window(app: tauri::AppHandle) {
    relic_pick_show(&app);
}

/// Debug: return the last ~4 KB of EE.log so we can see what strings appear when
/// opening the relic selection screen. Call this immediately after opening the screen.
#[tauri::command]
pub(crate) fn debug_ee_log_tail() -> String {
    use std::io::{Read, Seek, SeekFrom};
    let log_path = match dirs::data_local_dir() {
        Some(d) => d.join("Warframe").join("EE.log"),
        None => return "Cannot find LocalAppData".to_string(),
    };
    let mut f = match std::fs::File::open(&log_path) {
        Ok(f) => f,
        Err(e) => return format!("Cannot open EE.log: {}", e),
    };
    let len = f.seek(SeekFrom::End(0)).unwrap_or(0);
    let start = len.saturating_sub(4096);
    if f.seek(SeekFrom::Start(start)).is_err() {
        return "Seek failed".to_string();
    }
    let mut buf = String::new();
    let _ = f.read_to_string(&mut buf);
    // Skip partial first line if we started mid-file
    let tail = if start > 0 {
        buf.find('\n').map(|i| &buf[i+1..]).unwrap_or(&buf)
    } else {
        &buf
    };
    tail.to_string()
}

#[derive(serde::Serialize, Clone)]
struct RelicPickReward {
    name:      String,
    rarity:    String,   // "Bronze" | "Silver" | "Gold"
    drop_rate: f64,      // relic_drop_rate(rarity, refinement) for this relic
    ducats:    u32,
    plat:      u32,
    vaulted:   bool,
    owned:     bool,
}

#[derive(serde::Serialize, Clone)]
struct RelicPickRelic {
    name:          String,   // "Lith A1 Intact"
    base_name:     String,   // "Lith A1"
    refinement:    String,   // "intact" | "exceptional" | "flawless" | "radiant"
    count:         i64,
    unowned_score: f64,
    ducat_score:   f64,
    plat_score:    f64,
    rewards:       Vec<RelicPickReward>,
}

fn relic_drop_rate(rarity: &str, refinement: &str) -> f64 {
    match (rarity, refinement) {
        ("Bronze", "intact")      => 0.2533,
        ("Bronze", "exceptional") => 0.2333,
        ("Bronze", "flawless")    => 0.20,
        ("Bronze", "radiant")     => 0.1667,
        ("Silver", "intact")      => 0.11,
        ("Silver", "exceptional") => 0.13,
        ("Silver", "flawless")    => 0.17,
        ("Silver", "radiant")     => 0.20,
        ("Gold",   "intact")      => 0.02,
        ("Gold",   "exceptional") => 0.04,
        ("Gold",   "flawless")    => 0.06,
        ("Gold",   "radiant")     => 0.10,
        _ => 0.0,
    }
}


pub(crate) fn build_relic_pick_payload(era: &str, app: &tauri::AppHandle) -> serde_json::Value {
    let state = app.state::<AppState>();
    // era_prefix matches wfcd_items display names (e.g. "Lith A1 Intact" starts with "Lith ")
    let era_prefix = match era {
        "LITH" => "Lith ",
        "MESO" => "Meso ",
        "NEO"  => "Neo ",
        "AXI"  => "Axi ",
        "ALL"  => "",
        _      => return serde_json::json!({ "era": era, "relics": [] }),
    };

    let quantities    = state.current_quantities.lock().unwrap_or_else(|e| e.into_inner()).clone();
    // relic_rewards is now keyed by display name ("Lith A1 Intact") after the wfcd.rs fix.
    let relic_rewards = state.relic_rewards.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let wfcd_items    = state.wfcd_items.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let plat_prices   = state.relics_run_prices.lock().unwrap_or_else(|e| e.into_inner()).clone();

    info!("relic-pick payload: quantities={} relic_rewards={} wfcd_items={} plat_prices={}",
          quantities.len(), relic_rewards.len(), wfcd_items.len(), plat_prices.len());

    let ducat_map: HashMap<String, u32> = wfcd_items.iter()
        .filter_map(|item| item.ducats.map(|d| (item.name.to_lowercase(), d)))
        .collect();

    let vaulted_map: HashMap<String, bool> = wfcd_items.iter()
        .filter_map(|item| item.vaulted.map(|v| (item.name.to_lowercase(), v)))
        .collect();

    // quantities is keyed by Lotus paths, not display names.
    // Build display-name → Lotus path for direct lookup.
    let name_to_unique: HashMap<String, String> = wfcd_items.iter()
        .map(|item| (item.name.to_lowercase(), item.unique_name.clone()))
        .collect();

    // Build reverse of recipes: component display name (lower) → parent display names.
    // Used to detect "owned" when the part was consumed crafting the parent (e.g. built Daikyu Prime).
    let recipes_lock = state.recipes.lock().unwrap_or_else(|e| e.into_inner());
    let mut comp_to_parents: HashMap<String, Vec<String>> = HashMap::new();
    for (parent_name, components) in recipes_lock.iter() {
        for comp in components {
            comp_to_parents
                .entry(comp.name.to_lowercase())
                .or_default()
                .push(parent_name.clone());
        }
    }
    drop(recipes_lock);

    // Returns true if the item itself OR any parent item (built result) is in inventory.
    let is_owned = |item_name: &str| -> bool {
        let key = item_name.to_lowercase();
        let direct = name_to_unique.get(&key)
            .and_then(|uname| quantities.get(uname))
            .is_some_and(|&q| q > 0);
        if direct { return true; }
        comp_to_parents.get(&key).is_some_and(|parents| {
            parents.iter().any(|p| {
                name_to_unique.get(&p.to_lowercase())
                    .and_then(|uname| quantities.get(uname))
                    .is_some_and(|&q| q > 0)
            })
        })
    };

    // Refinement suffixes as they appear in display names (capitalised).
    const REFINEMENTS: &[(&str, &str)] = &[
        ("Intact",      "intact"),
        ("Exceptional", "exceptional"),
        ("Flawless",    "flawless"),
        ("Radiant",     "radiant"),
    ];

    // Iterate wfcd_items for the relic catalog.
    // unique_name ("/Lotus/Upgrades/Relics/...") matches current_quantities keys from the blob.
    let mut relics: Vec<RelicPickRelic> = wfcd_items.iter()
        .filter(|item| item.category == "Relics")
        .filter(|item| era_prefix.is_empty() || item.name.starts_with(era_prefix))
        .filter_map(|item| {
            let count = *quantities.get(&item.unique_name).unwrap_or(&0);
            if count <= 0 { return None; }

            let (suffix_cap, refinement) = REFINEMENTS.iter()
                .find(|(cap, _)| item.name.ends_with(cap))?;
            let refinement = refinement.to_string();
            let base_name  = item.name[..item.name.len() - suffix_cap.len() - 1].to_string();

            // Rewards keyed by display name in relic_rewards (after wfcd.rs fix).
            let reward_list: Vec<RelicPickReward> = relic_rewards
                .get(&item.name)
                .map(|rewards| rewards.iter().map(|r| {
                    let key       = r.name.to_lowercase();
                    let drop_rate = relic_drop_rate(&r.rarity, &refinement);
                    let ducats    = ducat_map.get(&key).copied().unwrap_or(0);
                    let plat      = plat_prices.get(&key).copied().unwrap_or(0);
                    let vaulted   = vaulted_map.get(&key).copied().unwrap_or(false);
                    let owned     = is_owned(&r.name);
                    RelicPickReward { name: r.name.clone(), rarity: r.rarity.clone(), drop_rate, ducats, plat, vaulted, owned }
                }).collect())
                .unwrap_or_default();

            let unowned_score: f64 = reward_list.iter()
                .filter(|r| !r.owned)
                .map(|r| r.drop_rate)
                .sum();
            let ducat_score: f64 = reward_list.iter()
                .map(|r| r.drop_rate * r.ducats as f64)
                .sum();
            let plat_score: f64 = reward_list.iter()
                .map(|r| r.drop_rate * r.plat as f64)
                .sum();

            Some(RelicPickRelic {
                name: item.name.clone(), base_name, refinement, count,
                unowned_score, ducat_score, plat_score, rewards: reward_list,
            })
        })
        .collect();

    relics.sort_by(|a, b| b.ducat_score.partial_cmp(&a.ducat_score).unwrap_or(std::cmp::Ordering::Equal));
    info!("relic-pick payload: {} relics built for era={}", relics.len(), era);
    serde_json::json!({ "era": era, "relics": relics })
}

/// Reposition the pre-declared relic-overlay window and bring it on screen.
/// The overlay is pre-declared in tauri.conf.json at y=-3000 (off-screen) so
/// WebView2 initialises at app startup. We never create/destroy it — just move it.
#[tauri::command]
pub(crate) fn show_overlay_window(
    app: tauri::AppHandle,
    x: i32, y: i32, w: u32, h: u32,
) -> Result<(), String> {
    use tauri::Manager;
    let win = app.get_webview_window("relic-overlay")
        .ok_or_else(|| "relic-overlay window not found".to_string())?;
    let _ = win.set_size(tauri::Size::Physical(
        tauri::PhysicalSize { width: w, height: h }
    ));
    // Caller passes physical pixels, offsets are stored as logical — scale them
    // with the window's own monitor so the shift reads the same at any DPI.
    let (off_x, off_y) = saved_offset(&app, "relic");
    let scale = win.scale_factor().unwrap_or(1.0);
    let _ = win.set_position(tauri::Position::Physical(
        tauri::PhysicalPosition {
            x: x + (off_x * scale).round() as i32,
            y: y + (off_y * scale).round() as i32,
        }
    ));
    let _ = win.show();
    let _ = win.set_always_on_top(true);

    // On Windows 10, WebView2 defers loading the page when the window starts
    // off-screen. If it's still on about:blank, navigate to the overlay URL now.
    if let Ok(url) = win.url() {
        if url.as_str() == "about:blank" || url.as_str().starts_with("about:") {
            debug!(%url, "WebView2 deferred load detected, navigating to overlay URL");
            let overlay_url = if cfg!(debug_assertions) {
                "http://localhost:1420/index.html?overlay"
            } else {
                "tauri://localhost/index.html?overlay"
            };
            if let Ok(nav_url) = tauri::Url::parse(overlay_url) {
                let _ = win.navigate(nav_url);
            }
        }
    }

    Ok(())
}

/// Move the relic-overlay window back off-screen (visual "close" without destroying it).
/// Destroying and recreating transparent WebView2 windows deadlocks on Windows.
#[tauri::command]
pub(crate) fn move_overlay_offscreen(app: tauri::AppHandle) -> Result<(), String> {
    use tauri::Manager;
    if let Some(win) = app.get_webview_window("relic-overlay") {
        let _ = win.set_position(tauri::Position::Physical(
            tauri::PhysicalPosition { x: 0, y: -3000 }
        ));
    }
    Ok(())
}

/// Pull and clear the last locked relic reward payload { items, positions }.
/// Overlay.tsx calls this on mount so it never misses rewards that arrived before
/// its relic-rewards listener was registered (the tauri://created → React mount gap).
#[tauri::command]
pub(crate) fn get_pending_relic_rewards(state: State<'_, AppState>) -> Option<serde_json::Value> {
    state.pending_relic_rewards.lock().ok()?.take()
}
