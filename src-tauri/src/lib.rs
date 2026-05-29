use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{Emitter, Manager, State};

mod db;
mod memory_scanner;
mod ocr;
mod wfcd;

use db::QuantityChange;
use wfcd::{RecipeComponent, WfcdItem};

pub struct AppState {
    pub db_path: PathBuf,
    pub items_cache_path: PathBuf,
    pub recipes_cache_path: PathBuf,
    pub relic_drops_cache_path: PathBuf,
    pub relic_rewards_cache_path: PathBuf,
    pub quantities_cache_path: PathBuf,
    pub settings_path: PathBuf,
    pub log_path: PathBuf,
    pub conn: Mutex<rusqlite::Connection>,
    pub wfcd_items: Mutex<Vec<WfcdItem>>,
    /// parent unique_name → recipe component tree
    pub recipes: Mutex<HashMap<String, Vec<RecipeComponent>>>,
    /// component unique_name → relic unique_names that drop it
    pub relic_drops: Mutex<HashMap<String, Vec<String>>>,
    /// relic unique_name → sorted reward list (Bronze×3, Silver×2, Gold×1)
    pub relic_rewards: Mutex<HashMap<String, Vec<wfcd::RelicReward>>>,
    /// blueprint_unique → (display_name, ducats). Used to enrich virtual catalog entries.
    pub blueprint_to_result: Mutex<HashMap<String, (String, Option<u32>)>>,
    /// Canonical relic reward display names from the Warframe Wiki (lower-cased).
    pub wiki_reward_names: Mutex<std::collections::HashSet<String>>,
    /// Last-known quantities from memory scans. Shared with monitor thread.
    pub current_quantities: Arc<Mutex<HashMap<String, i64>>>,
    /// Last-known crafting jobs from memory scans. Shared with monitor thread.
    pub current_crafting: Arc<Mutex<Vec<CraftingJob>>>,
    pub monitor_active: Arc<AtomicBool>,
    /// WFM slug → median sell price (None = item not listed on WFM). Shared across all windows.
    pub wfm_price_cache: Mutex<HashMap<String, Option<u32>>>,
}

// ─── Item catalog ─────────────────────────────────────────────────────────────

#[derive(serde::Serialize, Clone)]
pub struct CatalogItem {
    pub unique_name: String,
    pub name: String,
    pub category: String,
    pub image_name: Option<String>,
    pub vaulted: Option<bool>,
    pub ducats: Option<u32>,
    pub mastery_req: Option<u32>,
}

/// Determine the correct display category for an item.
///
/// Rules (in order):
///   1. Name contains "Blueprint" → "Blueprints"
///   2. Name ends with a known weapon/warframe component suffix → "Parts"
///      (catches WFCD entries that are wrongly tagged as "Blueprints" or
///       assigned the parent weapon's category instead of their own)
///   3. WFCD says "Blueprints" but name has no "Blueprint" word → "Parts"
///      (defensive: WFCD sometimes mis-categorises direct-drop components)
///   4. Everything else → keep WFCD category as-is
fn fix_category(name: &str, wfcd_cat: &str) -> String {
    let lower = name.to_lowercase();

    // Mods and Arcanes are always themselves — check BEFORE the name-contains-
    // "blueprint" rule so that mods whose names include "Blueprint" (e.g.
    // "Ballistic Bullseye Blueprint", "Balefire Surge Blueprint") are never
    // reclassified as Blueprints.
    if wfcd_cat == "Mods" || wfcd_cat == "Arcanes" {
        return wfcd_cat.to_string();
    }

    if lower.contains("blueprint") {
        return "Blueprints".to_string();
    }

    // Warframe weapon / sentinel component name endings.
    // Warframe-frame components (Chassis, Neuroptics, Systems) always have
    // "Blueprint" in their name, so they are handled by rule 1 above.
    const PART_SUFFIXES: &[&str] = &[
        " receiver", " stock", " barrel", " blade", " handle", " guard",
        " hilt", " link", " gauntlet", " carapace", " cerebrum", " systems",
        " upper limb", " lower limb", " strike", " boot", " head",
    ];
    if PART_SUFFIXES.iter().any(|s| lower.ends_with(s)) {
        return "Parts".to_string();
    }

    // WFCD mis-tags some direct-drop components as "Blueprints".
    if wfcd_cat == "Blueprints" {
        return "Parts".to_string();
    }

    wfcd_cat.to_string()
}

#[tauri::command]
fn get_all_items(state: State<AppState>) -> Vec<CatalogItem> {
    let items    = state.wfcd_items.lock().unwrap_or_else(|e| e.into_inner());
    let bp_names = state.blueprint_to_result.lock().unwrap_or_else(|e| e.into_inner());

    // ExportRecipes is the authoritative source for blueprint items — their paths
    // match what the Warframe API returns in data.Recipes.
    // WFCD is authoritative for everything else (main warframes, weapons, parts).
    //
    // Strategy:
    //  1. Add all non-blueprint WFCD items (category ≠ "Blueprints" and
    //     unique_name doesn't start with /Lotus/Types/Recipes/)
    //  2. Add ALL ExportRecipes blueprint entries (no dedup needed — the map
    //     is keyed by unique_name so each entry appears only once)
    //  3. Add WFCD-only blueprints not covered by ExportRecipes (older content)
    //
    // This eliminates the "Dante Blueprint" duplicate: WFCD's recipe-path entry
    // is replaced by ExportRecipes' entry which matches the API path exactly.

    // ── Rebuild to eliminate cross-source blueprint duplicates ───────────────
    //
    // Root cause: WFCD stores the same blueprint at MULTIPLE paths (recipe path
    // + non-recipe path), causing it to appear in every category.
    //
    // Fix: ExportRecipes blueprints go in FIRST (authoritative API-matching
    // paths). WFCD blueprint items are then skipped if ExportRecipes already
    // has them by display name. WFCD non-blueprint items always go in.
    // ─────────────────────────────────────────────────────────────────────────

    let mut result: Vec<CatalogItem> = Vec::new();

    // Items whose base names can never have a real blueprint (Mods, Arcanes).
    // ExportRecipes sometimes contains phantom entries like "Ballistic Bullseye
    // Blueprint" even though mods cannot be crafted — we skip those here so
    // the inventory never shows a mod under the wrong name or category.
    let non_craftable_names: std::collections::HashSet<String> = items.iter()
        .filter(|i| i.category == "Mods" || i.category == "Arcanes")
        .map(|i| i.name.to_lowercase())
        .collect();

    // Phase 1: ExportRecipes blueprints (correct API paths, 1 per name)
    // Build a name→vaulted map from WFCD so blueprints inherit the correct vaulted status.
    // ExportRecipes has no vaulted field; WFCD does.  We look up by bp_name first, then
    // fall back to the base name without " Blueprint" (covers weapon/warframe entries).
    let wfcd_vaulted: std::collections::HashMap<String, Option<bool>> = items.iter()
        .map(|i| (i.name.to_lowercase(), i.vaulted))
        .collect();

    let mut bp_names_added: std::collections::HashSet<String> =
        std::collections::HashSet::new();
    for (bp_unique, (bp_name, bp_ducats)) in bp_names.iter() {
        // Skip phantom blueprint entries for mods/arcanes.
        // Strip the " Blueprint" suffix and check against the known mod names.
        let base = bp_name
            .strip_suffix(" Blueprint")
            .unwrap_or(bp_name)
            .to_lowercase();
        if non_craftable_names.contains(&base) { continue; }

        let n = bp_name.to_lowercase();
        if bp_names_added.insert(n.clone()) {
            // Inherit vaulted status from WFCD — try exact name first, then base name.
            let vaulted = wfcd_vaulted.get(&n).and_then(|v| *v)
                .or_else(|| wfcd_vaulted.get(&base).and_then(|v| *v));
            result.push(CatalogItem {
                unique_name: bp_unique.clone(),
                name:        bp_name.clone(),
                category:    "Blueprints".to_string(),
                image_name:  None,
                vaulted,
                ducats:      *bp_ducats,
                mastery_req: None,
            });
        }
    }

    // Phase 2: WFCD items — keep WFCD categories, only fix blueprint names.
    // Skip blueprints already covered by ExportRecipes or already added
    // (WFCD may store the same blueprint at multiple paths).
    for i in items.iter() {
        let cat = fix_category(&i.name, &i.category);
        let n = i.name.to_lowercase();
        if cat == "Blueprints" {
            if !bp_names_added.insert(n) { continue; } // skip if already seen
        }
        result.push(CatalogItem {
            unique_name: i.unique_name.clone(),
            name:        i.name.clone(),
            category:    cat,
            image_name:  i.image_name.clone(),
            vaulted:     i.vaulted,
            ducats:      i.ducats,
            mastery_req: i.mastery_req,
        });
    }

    // Phase 3: WFCD-only blueprints NOT covered by ExportRecipes.
    for item in items.iter() {
        if !item.unique_name.starts_with("/Lotus/Types/Recipes/") { continue; }
        let n = item.name.to_lowercase();
        if !bp_names_added.insert(n) { continue; }
        result.push(CatalogItem {
            unique_name: item.unique_name.clone(),
            name:        item.name.clone(),
            category:    "Blueprints".to_string(),
            image_name:  item.image_name.clone(),
            vaulted:     item.vaulted,
            ducats:      item.ducats,
            mastery_req: item.mastery_req,
        });
    }

    // Final safety dedup by unique_name
    let mut seen_unique: std::collections::HashSet<String> = std::collections::HashSet::new();
    result.retain(|i| seen_unique.insert(i.unique_name.clone()));

    result
}

#[tauri::command]
fn get_current_quantities(state: State<AppState>) -> HashMap<String, i64> {
    state.current_quantities.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

#[tauri::command]
fn get_current_crafting(state: State<AppState>) -> Vec<CraftingJob> {
    state.current_crafting.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

#[tauri::command]
fn get_item_list_status(state: State<AppState>) -> serde_json::Value {
    let items = state.wfcd_items.lock().unwrap_or_else(|e| e.into_inner());
    let recipes = state.recipes.lock().unwrap_or_else(|e| e.into_inner());
    // Sample a few recipe keys for diagnostics
    let sample: Vec<&String> = recipes.keys().take(3).collect();
    serde_json::json!({
        "count": items.len(),
        "recipe_count": recipes.len(),
        "recipe_sample": sample,
    })
}

#[tauri::command]
async fn fetch_item_list(state: State<'_, AppState>) -> Result<usize, String> {
    let result = tauri::async_runtime::spawn_blocking(wfcd::fetch_items)
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e)?;

    let count = result.items.len();

    // Persist items cache
    if let Ok(json) = serde_json::to_string(&result.items.iter().map(|i| serde_json::json!({
        "unique_name": i.unique_name, "name": i.name, "category": i.category,
        "image_name": i.image_name, "vaulted": i.vaulted, "ducats": i.ducats,
        "mastery_req": i.mastery_req
    })).collect::<Vec<_>>()) {
        let _ = std::fs::write(&state.items_cache_path, json);
    }

    // Persist recipes cache
    if let Ok(json) = serde_json::to_string(&result.recipes) {
        let _ = std::fs::write(&state.recipes_cache_path, json);
    }

    let patched_items: Vec<WfcdItem> = result.items.into_iter().map(|mut i| {
        i.name = patch_item_name(&i.unique_name, &i.name);
        i.category = patch_item_category(&i.name, &i.category);
        i
    }).collect();
    if let Ok(json) = serde_json::to_string(&result.relic_drops) {
        let _ = std::fs::write(&state.relic_drops_cache_path, json);
    }
    if let Ok(json) = serde_json::to_string(&result.relic_rewards) {
        let _ = std::fs::write(&state.relic_rewards_cache_path, json);
    }
    *state.wfcd_items.lock().map_err(|e| e.to_string())? = patched_items;
    *state.recipes.lock().map_err(|e| e.to_string())? = result.recipes;
    *state.relic_drops.lock().map_err(|e| e.to_string())? = result.relic_drops;
    *state.relic_rewards.lock().map_err(|e| e.to_string())? = result.relic_rewards;
    *state.blueprint_to_result.lock().map_err(|e| e.to_string())? = result.blueprint_names;
    if !result.wiki_reward_names.is_empty() {
        *state.wiki_reward_names.lock().map_err(|e| e.to_string())? = result.wiki_reward_names;
    }
    Ok(count)
}

// ─── Foundry / Recipes ────────────────────────────────────────────────────────

/// Returns all items that have a crafting recipe (for the Foundry search list).
#[tauri::command]
fn get_craftable_items(state: State<AppState>) -> Vec<CatalogItem> {
    // Collect recipe keys first, drop the lock, then lock items separately
    // to avoid holding two locks simultaneously (prevents potential deadlock
    // with fetch_item_list which locks in the opposite order).
    let recipe_keys: std::collections::HashSet<String> = {
        let recipes = state.recipes.lock().unwrap_or_else(|e| e.into_inner());
        recipes.keys().cloned().collect()
    };
    let items = state.wfcd_items.lock().unwrap_or_else(|e| e.into_inner());
    items.iter()
        .filter(|i| recipe_keys.contains(&i.unique_name))
        .map(|i| CatalogItem {
            unique_name: i.unique_name.clone(),
            name: i.name.clone(),
            category: i.category.clone(),
            image_name: i.image_name.clone(),
            vaulted: i.vaulted,
            ducats: i.ducats,
            mastery_req: i.mastery_req,
        })
        .collect()
}

/// Returns the recipe component tree for a single item (empty vec = not found).
/// Returns Vec instead of Option to avoid Tauri serialization edge cases.
#[tauri::command]
fn get_recipe(state: State<AppState>, unique_name: String) -> Vec<RecipeComponent> {
    let recipes = state.recipes.lock().unwrap_or_else(|e| e.into_inner());
    recipes.get(&unique_name).cloned().unwrap_or_default()
}

/// Returns the relic drop map: component unique_name → relic unique_names.
#[tauri::command]
fn get_relic_drops(state: State<AppState>) -> HashMap<String, Vec<String>> {
    state.relic_drops.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Returns the relic rewards map: relic unique_name → sorted reward list.
#[tauri::command]
fn get_relic_rewards(state: State<AppState>) -> HashMap<String, Vec<wfcd::RelicReward>> {
    state.relic_rewards.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

// ─── Warframe companion API ───────────────────────────────────────────────────

/// Scan all Warframe memory regions for the session credentials (accountId + nonce).
/// These are placed in memory by the game itself after login — we never handle passwords.
#[tauri::command]
async fn scan_warframe_credentials() -> Result<(String, String, String), String> {
    tauri::async_runtime::spawn_blocking(scan_warframe_credentials_sync)
        .await
        .map_err(|e| e.to_string())?
}

fn scan_warframe_credentials_sync() -> Result<(String, String, String), String> {
    #[cfg(not(target_os = "windows"))]
    { return Err("Only supported on Windows".into()); }
    #[cfg(target_os = "windows")]
    use windows_sys::Win32::{
        Foundation::CloseHandle,
        System::{
            Diagnostics::Debug::ReadProcessMemory,
            Memory::{VirtualQueryEx, MEMORY_BASIC_INFORMATION, MEM_COMMIT, PAGE_GUARD, PAGE_NOACCESS},
            Threading::{OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ},
        },
    };
    use std::ffi::c_void;
    use std::mem;

    let pid = memory_scanner::find_warframe_pid_pub()
        .ok_or("Warframe is not running")?;

    unsafe {
        let process = OpenProcess(PROCESS_VM_READ | PROCESS_QUERY_INFORMATION, 0, pid);
        if process == 0 { return Err("Cannot open Warframe process".into()); }

        let mut address: usize = 0x10000;
        let mbi_size = mem::size_of::<MEMORY_BASIC_INFORMATION>();

        loop {
            let mut mbi: MEMORY_BASIC_INFORMATION = mem::zeroed();
            if VirtualQueryEx(process, address as *const c_void, &mut mbi, mbi_size) == 0 { break; }
            let region_end = (mbi.BaseAddress as usize).saturating_add(mbi.RegionSize);
            if region_end <= address { break; }
            address = region_end;

            if mbi.State != MEM_COMMIT { continue; }
            let p = mbi.Protect;
            if p & PAGE_NOACCESS != 0 || p & PAGE_GUARD != 0 { continue; }
            if p == 0x10 || p == 0x20 { continue; }
            if mbi.RegionSize > 128 * 1024 * 1024 { continue; }

            let mut buffer = vec![0u8; mbi.RegionSize];
            let mut bytes_read: usize = 0;
            let ok = ReadProcessMemory(
                process, mbi.BaseAddress as *const c_void,
                buffer.as_mut_ptr() as *mut c_void, mbi.RegionSize, &mut bytes_read,
            );
            if ok == 0 || bytes_read == 0 { continue; }

            if let Some((id, nonce)) = memory_scanner::scan_auth_credentials(&buffer[..bytes_read]) {
                let steam_id = memory_scanner::scan_steam_id(&buffer[..bytes_read]).unwrap_or_default();
                CloseHandle(process);
                return Ok((id, nonce, steam_id));
            }
        }
        CloseHandle(process);
    }
    Err("Credentials not found in memory. Make sure you are in the orbiter (not loading screen) and Warframe has been running for a few minutes.".into())

}

/// Scan Warframe memory for API request URLs — reveals exact endpoints the game uses.
#[tauri::command]
async fn scan_warframe_api_urls() -> Result<Vec<String>, String> {
    tauri::async_runtime::spawn_blocking(|| {
        use windows_sys::Win32::{
            Foundation::CloseHandle,
            System::{
                Diagnostics::Debug::ReadProcessMemory,
                Memory::{VirtualQueryEx, MEMORY_BASIC_INFORMATION, MEM_COMMIT, PAGE_GUARD, PAGE_NOACCESS},
                Threading::{OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ},
            },
        };
        use std::ffi::c_void;
        use std::mem;

        let pid = memory_scanner::find_warframe_pid_pub()
            .ok_or("Warframe not running".to_string())?;

        let mut found = Vec::new();
        unsafe {
            let process = OpenProcess(PROCESS_VM_READ | PROCESS_QUERY_INFORMATION, 0, pid);
            if process == 0 { return Err("Cannot open process".into()); }

            let mut address: usize = 0x10000;
            let mbi_size = mem::size_of::<MEMORY_BASIC_INFORMATION>();

            loop {
                let mut mbi: MEMORY_BASIC_INFORMATION = mem::zeroed();
                if VirtualQueryEx(process, address as *const c_void, &mut mbi, mbi_size) == 0 { break; }
                let region_end = (mbi.BaseAddress as usize).saturating_add(mbi.RegionSize);
                if region_end <= address { break; }
                address = region_end;

                if mbi.State != MEM_COMMIT { continue; }
                let p = mbi.Protect;
                if p & PAGE_NOACCESS != 0 || p & PAGE_GUARD != 0 { continue; }
                if p == 0x10 || p == 0x20 { continue; }
                if mbi.RegionSize > 64 * 1024 * 1024 { continue; }

                let mut buffer = vec![0u8; mbi.RegionSize];
                let mut bytes_read: usize = 0;
                let ok = ReadProcessMemory(
                    process, mbi.BaseAddress as *const c_void,
                    buffer.as_mut_ptr() as *mut c_void, mbi.RegionSize, &mut bytes_read,
                );
                if ok == 0 || bytes_read == 0 { continue; }

                let data = &buffer[..bytes_read];
                // Search for various Warframe API patterns
                let needles: &[&[u8]] = &[
                    b"/API/PHP/", b"inventory.php", b"login.php",
                    b"warframe.com/A", b"Nonce", b"accountId",
                ];
                for needle in needles {
                    let mut i = 0;
                    while i + needle.len() < data.len() {
                        if &data[i..i + needle.len()] == *needle {
                            let start = i.saturating_sub(30);
                            let end = (i + 100).min(data.len());
                            let ctx: String = data[start..end].iter()
                                .map(|&b| if b >= 0x20 && b < 0x7f { b as char } else { ' ' })
                                .collect();
                            let trimmed = ctx.split_whitespace().collect::<Vec<_>>().join(" ");
                            let label = format!("[{}] {}", std::str::from_utf8(needle).unwrap_or("?"), trimmed);
                            if !found.iter().any(|s: &String| s.contains(&trimmed[..trimmed.len().min(30)])) {
                                found.push(label);
                            }
                            if found.len() >= 40 { break; }
                        }
                        i += 1;
                    }
                }
                if found.len() >= 20 { break; }
            }
            CloseHandle(process);
        }
        Ok(found)
    }).await.map_err(|e| e.to_string())?
}

/// Login to Warframe API with email + password (same flow as mobile companion app).
/// Password is hashed with Whirlpool before sending — never sent in plaintext.
/// Returns (accountId, nonce) for subsequent API calls.
#[tauri::command]
async fn warframe_login(email: String, password: String) -> Result<(String, String), String> {
    use whirlpool::{Whirlpool, Digest};
    let hash = format!("{:x}", Whirlpool::digest(password.as_bytes()));
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
    let body = format!(
        "email={}&password={}&time={}&type=pc&appVersion=live",
        urlencoding(&email), hash, now
    );
    let resp = ureq::post("https://api.warframe.com/API/PHP/login.php")
        .set("X-Titanium-Id", "9bbd1ddd-f7f2-402d-9777-873f458cb50c")
        .set("X-Requested-With", "XMLHttpRequest")
        .set("Content-Type", "application/x-www-form-urlencoded")
        .set("User-Agent", "Dalvik/2.1.0 (Linux; U; Android 8.1.0)")
        .send_string(&body)
        .map_err(|e| format!("Login failed: {}", e))?;
    let text = resp.into_string().map_err(|e| e.to_string())?;
    let json: serde_json::Value = serde_json::from_str(&text)
        .map_err(|_| format!("Login response invalid: {}", &text[..text.len().min(200)]))?;
    let id = json["id"].as_str().unwrap_or("").to_string();
    let nonce = json["Nonce"].to_string().trim_matches('"').to_string();
    if id.is_empty() || nonce == "null" {
        return Err(format!("Login rejected: {}", &text[..text.len().min(200)]));
    }
    Ok((id, nonce))
}

fn urlencoding(s: &str) -> String {
    s.chars().flat_map(|c| match c {
        'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => vec![c],
        '@' => vec!['%', '4', '0'],
        _ => format!("%{:02X}", c as u8).chars().collect(),
    }).collect()
}

/// Fetch the player's full inventory from the Warframe companion API.
#[tauri::command]
async fn fetch_warframe_inventory(account_id: String, nonce: String, steam_id: String) -> Result<serde_json::Value, String> {
    // Base URL uses lowercase /api/ (not /API/PHP/). ct=STM for Steam platform.
    let endpoints = [
        "https://api.warframe.com/api/inventory.php",
        "https://api.warframe.com/api/profile.php",
    ];
    let body = format!(
        "accountId={}&nonce={}&ct=STM{}&SteamOnly=1",
        account_id, nonce,
        if !steam_id.is_empty() { format!("&steamId={}", steam_id) } else { String::new() }
    );
    let headers = [
        ("Content-Type", "application/x-www-form-urlencoded"),
        ("User-Agent", "Mozilla/5.0"),
        ("Accept", "application/json"),
        ("Host", "api.warframe.com"),
    ];

    let mut last_err = String::new();
    for url in &endpoints {
        let mut req = ureq::post(url);
        for (k, v) in &headers { req = req.set(k, v); }
        match req.send_string(&body) {
            Ok(resp) => {
                let status = resp.status();
                let text = resp.into_string().unwrap_or_default();
                if status == 200 {
                    return serde_json::from_str(&text)
                        .map_err(|e| format!("Parse failed: {} — body: {}", e, &text[..text.len().min(200)]));
                }
                last_err = format!("HTTP {} from {}: {}", status, url, &text[..text.len().min(100)]);
            }
            Err(e) => { last_err = format!("Request to {} failed: {}", url, e); }
        }
    }
    Err(last_err)
}

// ─── Warframe.market ──────────────────────────────────────────────────────────

#[derive(serde::Serialize)]
pub struct WfmItem {
    pub item_name: String,
    pub url_name: String,
}

/// Fetch warframe.market item list using v2 API (v1 /items returns 404).
#[tauri::command]
fn fetch_wfm_items() -> Result<Vec<WfmItem>, String> {
    let json: serde_json::Value = ureq::get("https://api.warframe.market/v2/items")
        .call()
        .map_err(|e| format!("wfm items: {}", e))?
        .into_json()
        .map_err(|e| format!("wfm items parse: {}", e))?;

    // v2 format: { "data": [{ "slug": "rhino_prime_set", "i18n": { "en": { "name": "Rhino Prime Set" } } }] }
    let items = json["data"]
        .as_array()
        .ok_or("no data array in v2 response")?
        .iter()
        .filter_map(|v| Some(WfmItem {
            item_name: v["i18n"]["en"]["name"].as_str()?.to_string(),
            url_name:  v["slug"].as_str()?.to_string(),
        }))
        .collect();
    Ok(items)
}

#[derive(serde::Serialize)]
pub struct WfmPrice {
    pub url_name: String,
    pub sell_median: Option<f64>,
    pub buy_median: Option<f64>,
}

/// Fetch 48-hour median sell price for a single item from warframe.market.
/// Tries the slug as-is first, then retries with the Blueprint suffix added or
/// removed — WFM is inconsistent about whether component blueprints include it.
#[tauri::command]
fn fetch_wfm_price(url_name: String) -> Result<WfmPrice, String> {
    let sell_median = wfm_price_for_slug(&url_name).map_err(|e| e)?
        .or_else(|| {
            if url_name.ends_with("_blueprint") {
                let stripped = &url_name[..url_name.len() - "_blueprint".len()];
                wfm_price_for_slug(stripped).unwrap_or(None)
            } else {
                let with_bp = format!("{}_blueprint", url_name);
                wfm_price_for_slug(&with_bp).unwrap_or(None)
            }
        })
        .map(|p| p as f64);

    Ok(WfmPrice { url_name, sell_median, buy_median: None })
}

/// Convert a display name to a warframe.market URL slug.
/// E.g. "Ash Prime Neuroptics Blueprint" → "ash_prime_neuroptics_blueprint"
fn to_wfm_slug(name: &str) -> String {
    name.to_lowercase()
        .chars()
        .map(|c| if c == ' ' { '_' } else { c })
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect()
}

/// Fetch the 48-hour median sell price for an item by display name.
/// Results are cached in AppState so the overlay and main window share them.
/// Returns None when the item is not listed on warframe.market.
#[tauri::command]
fn get_item_price(item_name: String, state: State<AppState>) -> Result<Option<u32>, String> {
    let slug = to_wfm_slug(&item_name);

    {
        let cache = state.wfm_price_cache.lock().map_err(|e| e.to_string())?;
        if let Some(&cached) = cache.get(&slug) {
            return Ok(cached);
        }
    }

    let price = wfm_price_for_slug(&slug).map_err(|e| e)?
        .or_else(|| {
            // WFM lists prime component blueprints WITHOUT the "_blueprint" suffix.
            // e.g. "nautilus_prime_systems_blueprint" → "nautilus_prime_systems"
            if slug.ends_with("_blueprint") {
                let stripped = &slug[..slug.len() - "_blueprint".len()];
                wfm_price_for_slug(stripped).unwrap_or(None)
            } else {
                None
            }
        });

    {
        let mut cache = state.wfm_price_cache.lock().map_err(|e| e.to_string())?;
        cache.insert(slug, price);
    }

    Ok(price)
}

fn wfm_price_for_slug(slug: &str) -> Result<Option<u32>, String> {
    let url = format!("https://api.warframe.market/v1/items/{}/statistics", slug);
    match ureq::get(&url).call() {
        Ok(resp) => {
            let json: serde_json::Value = resp.into_json()
                .map_err(|e| format!("wfm price parse: {}", e))?;
            let closed = &json["payload"]["statistics_closed"]["48hours"];
            let p = closed.as_array()
                .and_then(|arr| arr.last())
                .and_then(|e| e["median"].as_f64())
                .map(|f| f.round() as u32);
            Ok(p.or_else(|| {
                json["payload"]["statistics_closed"]["90days"].as_array()
                    .and_then(|arr| arr.last())
                    .and_then(|e| e["median"].as_f64())
                    .map(|f| f.round() as u32)
            }))
        }
        Err(_) => Ok(None),
    }
}

// ─── Change log ───────────────────────────────────────────────────────────────

#[tauri::command]
fn get_change_log(state: State<AppState>, limit: i64) -> Result<Vec<QuantityChange>, String> {
    let conn = state.conn.lock().map_err(|e| e.to_string())?;
    db::get_quantity_changes(&conn, limit).map_err(|e| e.to_string())
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

#[tauri::command]
fn get_app_version() -> String {
    // In dev mode the source tauri.conf.json is in the current directory
    let config = std::path::Path::new("src-tauri/tauri.conf.json");
    if config.exists() {
        if let Ok(text) = std::fs::read_to_string(config) {
            let marker = "\"version\": \"";
            if let Some(start) = text.find(marker) {
                let after = start + marker.len();
                if let Some(end) = text[after..].find('"') {
                    return text[after..after + end].to_string();
                }
            }
        }
    }
    env!("CARGO_PKG_VERSION").to_string()
}

#[tauri::command]
fn set_app_version(version: String) -> Result<(), String> {
    let tauri_conf = std::path::Path::new("src-tauri/tauri.conf.json");
    let package_json = std::path::Path::new("package.json");
    if tauri_conf.exists() { update_version_in_file(tauri_conf, &version)?; }
    if package_json.exists() { update_version_in_file(package_json, &version)?; }
    Ok(())
}

#[tauri::command]
fn load_settings(state: State<AppState>) -> String {
    std::fs::read_to_string(&state.settings_path).unwrap_or_default()
}

#[tauri::command]
fn save_settings(app: tauri::AppHandle, state: State<AppState>, json: String) -> Result<(), String> {
    // Merge over existing file so geometry fields written by save_window_state are never erased
    let new_vals: serde_json::Value = serde_json::from_str(&json).map_err(|e| e.to_string())?;
    let mut existing: serde_json::Map<String, serde_json::Value> = std::fs::read_to_string(&state.settings_path)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| if let serde_json::Value::Object(m) = v { Some(m) } else { None })
        .unwrap_or_default();
    if let serde_json::Value::Object(new_map) = new_vals {
        for (k, v) in new_map { existing.insert(k, v); }
    }
    std::fs::write(&state.settings_path, serde_json::Value::Object(existing).to_string())
        .map_err(|e| e.to_string())?;
    app.emit("settings-updated", ()).ok();
    Ok(())
}

#[tauri::command]
fn read_scan_log(state: State<AppState>) -> Result<String, String> {
    std::fs::read_to_string(&state.log_path).map_err(|e| e.to_string())
}

#[tauri::command]
fn clear_cache(state: State<AppState>) -> Result<(), String> {
    // Clear change log from DB
    let conn = state.conn.lock().map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM quantity_changes", []).map_err(|e| e.to_string())?;
    drop(conn);

    // Reset in-memory quantities
    let mut q = state.current_quantities.lock().map_err(|e| e.to_string())?;
    q.clear();
    drop(q);

    // Delete quantities cache file so it doesn't reload on next start
    let _ = std::fs::remove_file(&state.quantities_cache_path);

    Ok(())
}

// ─── Live monitor ─────────────────────────────────────────────────────────────

#[derive(serde::Serialize, Clone)]
pub struct CraftingJob {
    pub unique_name: String,
    pub item_name: String,
    pub completion_ms: i64,
}

#[derive(serde::Serialize, Clone)]
pub struct InventoryUpdate {
    pub quantities: HashMap<String, i64>,
    pub crafting: Vec<CraftingJob>,
    pub mastery_rank: Option<u32>,
    pub mastery_data: HashMap<String, u32>,
    pub changes: Vec<QuantityChange>,
    pub warframe_running: bool,
    pub scanned_at: i64,
}

#[tauri::command]
async fn start_monitor(app: tauri::AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    if state.monitor_active.swap(true, Ordering::SeqCst) {
        return Ok(()); // already running
    }
    // Capture the Tokio runtime handle while we're in the async context.
    // The monitoring thread (std::thread::spawn) has no COM/WinRT, so all OCR
    // calls are routed through spawn_blocking which runs on Tokio's thread pool
    // (which DOES have COM initialized, same as the Capture debug button).
    let _rt = tokio::runtime::Handle::current();

    let items = state.wfcd_items.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let unique_names: Vec<String> = items.iter().map(|i| i.unique_name.clone()).collect();
    let display_names: Vec<String> = items.iter().map(|i| i.name.clone()).collect();
    let flag = state.monitor_active.clone();
    let db_path = state.db_path.clone();
    let log_path = state.log_path.clone();
    let quantities_cache_path = state.quantities_cache_path.clone();
    let shared_quantities = state.current_quantities.clone();
    let shared_crafting   = state.current_crafting.clone();
    let reward_app = app.clone();  // clone before app is moved into the inventory thread

    std::thread::spawn(move || {
        let conn = match rusqlite::Connection::open(&db_path) {
            Ok(c) => c,
            Err(e) => { eprintln!("Monitor DB open failed: {}", e); return; }
        };
        let _ = conn.execute_batch("PRAGMA journal_mode=WAL;");

        // Start from whatever quantities were last known (survives restarts).
        let mut known: HashMap<String, i64> =
            shared_quantities.lock().unwrap_or_else(|e| e.into_inner()).clone();

        // Stability buffer: (unique_name → (candidate_qty, consecutive_count))
        // A new quantity must appear in 2 consecutive scans before being committed.
        // This filters out transient reads: mission reward screens, clan showcases,
        // open-world drop popups — all appear for only 1 scan cycle.
        let mut pending: HashMap<String, (i64, u8)> = HashMap::new();

        while flag.load(Ordering::SeqCst) {
            let result = memory_scanner::scan_warframe_memory(&unique_names, &display_names);
            let now = chrono::Utc::now().timestamp();
            let now_str = chrono::DateTime::from_timestamp(now, 0)
                .map(|dt: chrono::DateTime<chrono::Utc>| dt.format("%Y-%m-%d %H:%M:%S").to_string())
                .unwrap_or_else(|| now.to_string());

            let mut changes: Vec<QuantityChange> = Vec::new();

            // If Warframe is not running, skip all quantity updates.
            // Windows keeps recently-closed process memory accessible, which means
            // the scanner would find stale data and re-populate a cleared cache.
            if !result.warframe_running {
                pending.clear(); // also clear pending so stale candidates don't commit when game reopens
                let _ = app.emit("inventory-update", InventoryUpdate {
                    quantities: known.clone(),
                    crafting: vec![],
                    mastery_rank: None,
                    mastery_data: HashMap::new(),
                    changes: vec![],
                    warframe_running: false,
                    scanned_at: now,
                });
                std::thread::sleep(std::time::Duration::from_secs(10));
                continue;
            }

            // Build a set of unique_names seen this scan (to reset pending for missing items)
            let mut seen_this_scan: HashMap<String, i64> = HashMap::new();
            for item in &result.items_found {
                seen_this_scan.insert(item.unique_name.clone(), item.quantity);
            }

            for item in &result.items_found {
                let old_qty = *known.get(&item.unique_name).unwrap_or(&0);
                let new_qty = item.quantity;

                // Only track items where an actual count was read from memory.
                // Implicit count (no number found near path) = path appeared in
                // relic tables, codex, market data etc. — NOT real inventory.
                // Blueprints/warframes/weapons without explicit counts are
                // handled reliably by the Warframe companion API instead.
                if !item.explicit_count { continue; }
                if new_qty == old_qty {
                    pending.remove(&item.unique_name);
                    continue;
                }

                // Stability check: require the same new value for 2 consecutive scans
                let entry = pending.entry(item.unique_name.clone()).or_insert((new_qty, 0));
                if entry.0 != new_qty {
                    // Value changed mid-pending — reset the counter
                    *entry = (new_qty, 1);
                    continue;
                }
                entry.1 += 1;
                if entry.1 < 2 {
                    // Require 2 consecutive scans before committing explicit counts
                    continue;
                }
                // Confirmed — commit the change
                pending.remove(&item.unique_name);
                let change = QuantityChange {
                    id: 0,
                    unique_name: item.unique_name.clone(),
                    item_name: item.name.clone(),
                    old_qty,
                    new_qty,
                    delta: new_qty - old_qty,
                    timestamp: now,
                };
                let _ = db::add_quantity_change(
                    &conn, &item.unique_name, &item.name, old_qty, new_qty,
                );
                known.insert(item.unique_name.clone(), new_qty);
                changes.push(change);
            }

            // Clear pending entries for items no longer visible in memory
            pending.retain(|k, _| seen_this_scan.contains_key(k));


            // Persist running quantities so restarts pick up where we left off.
            if let Ok(mut q) = shared_quantities.lock() {
                *q = known.clone();
            }
            if let Ok(json) = serde_json::to_string(&known) {
                let _ = std::fs::write(&quantities_cache_path, json);
            }

            // Overwrite log AFTER scan completes so it's always readable between cycles
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true).write(true).truncate(true).open(&log_path)
            {
                let _ = writeln!(f, "=== Scan at {} ===", now_str);
                if let Some(ref err) = result.error {
                    let _ = writeln!(f, "  ERROR: {}", err);
                }
                let _ = writeln!(f,
                    "  warframe_running={} regions_scanned={} items_found={}",
                    result.warframe_running, result.regions_scanned, result.items_found.len()
                );
                for line in &result.log_lines {
                    let _ = writeln!(f, "{}", line);
                }
                if !result.items_found.is_empty() {
                    let _ = writeln!(f, "  --- Final inventory ---");
                    for item in &result.items_found {
                        let _ = writeln!(f,
                            "  {:>7} {}  {}  [{}]",
                            item.quantity,
                            if item.explicit_count { "E" } else { "I" },
                            item.name,
                            item.unique_name,
                        );
                    }
                }
                if !changes.is_empty() {
                    let _ = writeln!(f, "  --- Changes this scan ---");
                    for c in &changes {
                        let _ = writeln!(f, "  {} -> {}  ({:+})  {}",
                            c.old_qty, c.new_qty, c.delta, c.item_name);
                    }
                }
            }

            let crafting: Vec<CraftingJob> = result.pending_recipes.iter().map(|r| {
                let name = display_names.iter().zip(unique_names.iter())
                    .find(|(_, u)| *u == &r.unique_name)
                    .map(|(d, _)| d.clone())
                    .unwrap_or_else(|| r.unique_name.split('/').last().unwrap_or("?").to_string());
                CraftingJob { unique_name: r.unique_name.clone(), item_name: name, completion_ms: r.completion_ms }
            }).collect();

            *shared_crafting.lock().unwrap_or_else(|e| e.into_inner()) = crafting.clone();

            let _ = app.emit("inventory-update", InventoryUpdate {
                quantities: known.clone(),
                crafting,
                mastery_rank: result.mastery_rank,
                mastery_data: result.mastery_data,
                changes,
                warframe_running: result.warframe_running,
                scanned_at: now,
            });

            std::thread::sleep(std::time::Duration::from_secs(10));
        }
    });

    // ── Dedicated relic reward thread — OCR poll every 500 ms ───────────────
    // Takes a screenshot of the Warframe window, runs Windows OCR on the
    // reward area, matches names against the catalog. Emits "relic-rewards"
    // only when the result changes (screen opens/closes or items change).
    let reward_flag   = state.monitor_active.clone();
    let reward_items  = state.wfcd_items.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let bp_items      = state.blueprint_to_result.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let relic_rewards_map = state.relic_rewards.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let wiki_names    = state.wiki_reward_names.lock().unwrap_or_else(|e| e.into_inner()).clone();

    // ── Catalog: build by display-name match, not by path ────────────────────
    //
    // The root cause of path-based matching failures:
    //   WFCD relic drops store reward unique_names as /Lotus/StoreItems/Types/...
    //   WFCD items catalog stores items as /Lotus/Types/... (no StoreItems prefix)
    //   ExportRecipes also uses /Lotus/Types/... paths
    //   → filter(valid_relic_rewards.contains(&i.unique_name)) finds nothing,
    //     and the catalog ends up populated with relics instead of reward items.
    //
    // Name-based matching bypasses this entirely:
    //   1. Wiki reward names  — canonical, lowercase, from Warframe Wiki (most accurate)
    //   2. WFCD reward names  — display names from relic drops table (fallback)
    //   3. Content filter      — all "prime" / "forma" items (last resort)

    // Source 1: wiki canonical reward names (lowercase display names)
    let mut reward_display_names: std::collections::HashSet<String> = wiki_names;

    // Source 2: WFCD relic drop display names — always merged (not just fallback).
    // Wiki parsing may miss recently-added primes; WFCD covers them.
    for rewards in relic_rewards_map.values() {
        for r in rewards {
            reward_display_names.insert(r.name.to_lowercase());
        }
    }

    let have_reward_names = !reward_display_names.is_empty();

    // Filter reward_items by display name (case-insensitive).
    // Uses filter_map so we can return a corrected display name when WFCD's name
    // differs from the in-game reward text (e.g. "Lavos Prime Chassis" in WFCD
    // vs "Lavos Prime Chassis Blueprint" shown on the fissure reward screen).
    let mut catalog_pairs: Vec<(String, String)> = reward_items.iter()
        .filter_map(|i| {
            let lower = i.name.to_lowercase();
            // Skip assembled warframes/weapons and relics — only parts+blueprints
            let is_relic = lower.ends_with("intact") || lower.ends_with("exceptional")
                || lower.ends_with("flawless") || lower.ends_with("radiant");
            if is_relic { return None; }
            // Built warframes/weapons are never fissure rewards (you always get parts/blueprints).
            // Excluding them prevents "Oberon Prime" (Warframes) from beating "Oberon Prime
            // Blueprint" when OCR misses the word "Blueprint".
            let is_built_item = matches!(i.category.as_str(),
                "Warframes" | "Primary" | "Secondary" | "Melee" | "Companion" |
                "Sentinels" | "Archwing" | "Arch-Gun" | "Arch-Melee" | "Pets" | "Robotic");
            if is_built_item { return None; }
            // Warframe prime component blueprints (Chassis/Neuroptics/Systems Blueprint)
            // are exclusively relic rewards. Always include them even when missing from
            // the wiki/WFCD reward name list (newly-added primes lag behind the wiki).
            let is_prime_wf_component = lower.contains("prime") && (
                lower.ends_with("chassis blueprint")
                || lower.ends_with("neuroptics blueprint")
                || lower.ends_with("systems blueprint")
            );
            if is_prime_wf_component { return Some((i.unique_name.clone(), i.name.clone())); }
            if have_reward_names {
                if reward_display_names.contains(&lower) {
                    return Some((i.unique_name.clone(), i.name.clone()));
                }
                // WFCD omits "Blueprint" from some component names that the in-game reward
                // screen includes (e.g. WFCD "Lavos Prime Chassis" vs in-game
                // "Lavos Prime Chassis Blueprint").  If appending " blueprint" hits a
                // known relic reward, include the item with the corrected display name
                // so OCR scoring works against the actual card text.
                let lower_bp = format!("{} blueprint", lower);
                if reward_display_names.contains(&lower_bp) {
                    return Some((i.unique_name.clone(), format!("{} Blueprint", i.name)));
                }
                None
            } else {
                // Last resort: everything that looks like a relic reward
                if lower.contains("prime") || lower.starts_with("forma") {
                    Some((i.unique_name.clone(), i.name.clone()))
                } else {
                    None
                }
            }
        })
        .collect();

    // Also pull blueprints from ExportRecipes that match reward names
    for (bp_unique, (bp_name, _)) in bp_items.iter() {
        let lower = bp_name.to_lowercase();
        // Check for exact match OR for the case where the catalog already has this
        // item with a " Blueprint" suffix appended (from the WFCD name-correction above).
        let already = catalog_pairs.iter().any(|(_, n)| {
            let nl = n.to_lowercase();
            nl == lower || nl == format!("{} blueprint", lower) || format!("{} blueprint", nl) == lower
        });
        if already { continue; }
        let is_prime_wf_component = lower.contains("prime") && (
            lower.ends_with("chassis blueprint")
            || lower.ends_with("neuroptics blueprint")
            || lower.ends_with("systems blueprint")
        );
        let (include, display_name) = if is_prime_wf_component {
            (true, bp_name.clone())
        } else if have_reward_names {
            if reward_display_names.contains(&lower) {
                (true, bp_name.clone())
            } else {
                let lower_bp = format!("{} blueprint", lower);
                if reward_display_names.contains(&lower_bp) {
                    (true, format!("{} Blueprint", bp_name))
                } else {
                    (false, bp_name.clone())
                }
            }
        } else {
            (lower.contains("prime") || lower.starts_with("forma"), bp_name.clone())
        };
        if include {
            catalog_pairs.push((bp_unique.clone(), display_name));
        }
    }

    // Deduplicate by unique_name
    catalog_pairs.sort_by(|a, b| a.0.cmp(&b.0));
    catalog_pairs.dedup_by(|a, b| a.0 == b.0);

    // Wrap catalog in Arc so it can be cheaply shared with spawn_blocking closures
    let catalog_pairs = std::sync::Arc::new(catalog_pairs);

    // Build a name-lookup map from catalog_pairs for the debug file.
    let _catalog_name_map: std::collections::HashMap<String, String> = catalog_pairs
        .iter()
        .map(|(u, n)| (u.clone(), n.clone()))
        .collect();

    let debug_path      = std::env::temp_dir().join("frameforge_reward_debug.txt");
    let last_found_path = std::env::temp_dir().join("frameforge_last_reward.txt");

    // ── EE.log watcher ────────────────────────────────────────────────────────
    // Warframe writes "Script [Info]: Got rewards" to EE.log the moment the
    // Void Fissure reward selection screen becomes active.  All open-source
    // tools (WFInfo, warframeocr, Sentinel) use this string as their trigger.
    // We tail the log file instead of relying on fragile OCR gate heuristics.
    let ee_log_path = dirs::data_local_dir()
        .map(|d| d.join("Warframe").join("EE.log"))
        .filter(|p| p.exists());

    // Shared flag: true while the reward screen is active according to EE.log
    let reward_screen_active = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let reward_screen_active2 = reward_screen_active.clone();

    // Shared squad size: updated by EE.log watcher when VoidProjections sequence
    // completes, read by OCR loop for each attempt. This lets late-arriving squad
    // data (VoidProjections often arrives 1-2 s after the screen opens) inform
    // subsequent OCR retries so the card count is always correct.
    let shared_squad_size: std::sync::Arc<std::sync::Mutex<Option<usize>>> =
        std::sync::Arc::new(std::sync::Mutex::new(None));
    let shared_squad_size2 = std::sync::Arc::clone(&shared_squad_size);

    // ── EE.log watcher → AlecaFrame-style OCR trigger ────────────────────────
    //
    // When Warframe writes "Got rewards" to EE.log, the reward screen is active.
    // We immediately schedule an OCR capture (same path as the working Capture
    // button) and emit the result as a "relic-rewards" event.
    // No polling needed — this is exactly how AlecaFrame works.

    let ee_ocr_app   = reward_app.clone();
    let ee_catalog   = std::sync::Arc::clone(&catalog_pairs);
    let ee_last_path = last_found_path.clone();
    let session_log_path = std::env::temp_dir().join("frameforge_overlay_session.txt");

    if let Some(log_path) = ee_log_path {
        let flag = reward_flag.clone();
        std::thread::spawn(move || {
            let mut file_pos: u64 = std::fs::metadata(&log_path)
                .map(|m| m.len()).unwrap_or(0);
            let mut active_since: Option<std::time::Instant> = None;
            use std::io::{Read, Seek, SeekFrom};

            // ── VoidProjections reward sequence state ─────────────────────────
            // The game logs squad reward info BEFORE the screen trigger fires.
            // We accumulate it across poll iterations so it's ready when OCR starts.
            let mut vp_in_seq        = false;
            let mut vp_seq_completed = false; // set when sequence finishes; used as fallback trigger
            let mut vp_other_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
            let mut vp_own_item = String::new(); // local player's reward path from EE.log
            let mut ee_squad_size: Option<usize> = None; // committed when sequence completes
            // Cooldown: after any dismiss, block new triggers for 60 s.
            // Prevents the overlay from re-firing on the Last Mission Results screen,
            // which replays some EE.log events from the same mission.
            let mut last_dismiss_at: Option<std::time::Instant> = None;

            loop {
                if !flag.load(Ordering::SeqCst) { break; }
                std::thread::sleep(std::time::Duration::from_millis(200));
                let Ok(mut f) = std::fs::File::open(&log_path) else { continue };
                let len = std::fs::metadata(&log_path).map(|m| m.len()).unwrap_or(0);
                if len < file_pos { file_pos = 0; }
                if f.seek(SeekFrom::Start(file_pos)).is_err() { continue; }
                let mut buf = String::new();
                if f.read_to_string(&mut buf).is_err() { continue; }
                file_pos = len;
                if buf.is_empty() { continue; }

                let lower = buf.to_lowercase();

                // ── VoidProjections squad parsing ─────────────────────────────
                // Parse the reward-handshake sequence that fires before the screen opens:
                //   "VoidProjections: GetVoidProjectionRewards"   → sequence start
                //   "[id] gets reward /Lotus/..."                  → local player's item
                //   "Still waiting on response from [id]"          → one other player
                //   "Client has reward info for all players now"   → sequence complete
                //
                // squad_size = 1 (local) + count("Still waiting") lines.
                // Logging only for now; item path matching is a future improvement.
                for line in buf.lines() {
                    let ll = line.to_lowercase();
                    if ll.contains("voidprojections: getvoidprojectionrewards") {
                        vp_in_seq  = true;
                        vp_other_ids.clear();
                        vp_own_item.clear();
                        ee_squad_size = None;
                    }
                    if vp_in_seq {
                        if ll.contains("gets reward /lotus/") {
                            if let Some(i) = line.find("/Lotus/") {
                                vp_own_item = line[i..].trim().to_string();
                            }
                        } else if ll.contains("still waiting on response from") {
                            // Extract the player ID (last whitespace-separated token)
                            if let Some(id) = ll.split_whitespace().last() {
                                vp_other_ids.insert(id.to_string());
                            }
                        } else if ll.contains("has reward info for all players now") {
                            // squad = local player (1) + unique other IDs seen
                            let squad = (1 + vp_other_ids.len()).clamp(1, 4);
                            ee_squad_size = Some(squad);
                            // Share with OCR loop so any pending retry uses the correct count.
                            if let Ok(mut g) = shared_squad_size2.lock() { *g = Some(squad); }
                            vp_in_seq = false;
                            vp_seq_completed = true; // fallback trigger signal
                            let _ = append_to_file(&session_log_path, &format!(
                                "[EE.log] VoidProjections squad\n\
                                 ├─ Local item : {}\n\
                                 ├─ Other players (unique IDs) : {}\n\
                                 └─ Squad size : {} total\n\n",
                                if vp_own_item.is_empty() { "(not found)" } else { &vp_own_item },
                                vp_other_ids.len(),
                                squad,
                            ));
                        }
                    }
                }

                // Trigger: "ProjectionRewardChoice.lua: Relic rewards initialized" fires
                // when the selection screen first becomes visible — specific to this Lua
                // script so it won't fire for login/mission rewards.
                // "openvoidprojectionrewardscreen" and vp_seq_completed kept as fallbacks
                // since they appear in some configurations.
                let has_trigger = lower.contains("projectionrewardchoice.lua: relic rewards initialized")
                    || lower.contains("openvoidprojectionrewardscreen")
                    || vp_seq_completed;
                vp_seq_completed = false; // consume the flag

                // Dismiss: "Relic reward screen shut down" fires when the player selects
                // a reward (or the countdown expires). DO NOT use "relic timer closed" —
                // that fires at 874.265 when the screen OPENS, not when it closes, causing
                // triggers and dismisses to appear in the same 200ms EE.log flush.
                // "CloseVoidProjectionRewardScreen" fires at the same moment as shut down.
                // "EndSession" is the final fallback for abrupt disconnects/exits.
                // Host migration is NOT a dismiss — the mission continues with a new host.
                let has_dismiss = lower.contains("relic reward screen shut down")
                    || lower.contains("closevoidprojectionrewardscreen")
                    || lower.contains("matchingservice::endsession");

                // ── Dismiss — always processed first (even if same batch as trigger) ──
                if has_dismiss {
                    let dismiss_line = buf.lines()
                        .find(|l| {
                            let ll = l.to_lowercase();
                            ll.contains("relic reward screen shut down")
                                || ll.contains("closevoidprojectionrewardscreen")
                                || ll.contains("matchingservice::endsession")
                        })
                        .unwrap_or("<unknown dismiss line>")
                        .trim()
                        .to_string();
                    let ts_d = chrono::Local::now().format("%H:%M:%S%.3f");
                    let _ = append_to_file(&session_log_path, &format!(
                        "[STEP 4] DISMISS\n\
                         ├─ Time     : {}\n\
                         └─ Line     : \"{}\"\n\n",
                        ts_d, dismiss_line
                    ));
                    reward_screen_active2.store(false, Ordering::SeqCst);
                    active_since = None;
                    last_dismiss_at = Some(std::time::Instant::now());
                    if let Some(win) = ee_ocr_app.get_webview_window("relic-overlay") {
                        let _ = win.close();
                    }
                    let _ = ee_ocr_app.emit("relic-rewards", serde_json::Value::Null);
                }

                // ── Trigger: skip if dismiss in same batch, screen already active, or
                //    within 60 s of last dismiss ───────────────────────────────────────
                // active_since.is_some() guards against duplicate triggers: EE.log is
                // polled every 200 ms, and multiple matching lines (e.g. "Client has
                // reward info" + "relic rewards initialized" 250 ms later) can fire in
                // consecutive polls while the same reward screen is still open.  Without
                // this guard, a second OCR task would spawn, emit different card
                // positions, and make the overlay stutter.
                let trigger_allowed = !has_dismiss
                    && active_since.is_none()
                    && last_dismiss_at.map_or(true, |t| t.elapsed().as_secs() >= 60);
                if has_trigger && trigger_allowed {
                    reward_screen_active2.store(true, Ordering::SeqCst);
                    active_since = Some(std::time::Instant::now());

                    // Find the exact EE.log line that matched so we can log it
                    let trigger_line = buf.lines()
                        .find(|l| {
                            let ll = l.to_lowercase();
                            ll.contains("relic rewards initialized")
                                || ll.contains("openvoidprojectionrewardscreen")
                                || ll.contains("has reward info for all players now")
                        })
                        .unwrap_or("<unknown trigger line>")
                        .trim()
                        .to_string();

                    let ts0 = chrono::Local::now().format("%H:%M:%S%.3f");

                    // Start a fresh session log for this reward screen
                    let write_err = std::fs::write(&session_log_path, format!(
                        "══════════════════════════════════════════════\n\
                         RELIC OVERLAY SESSION — {}\n\
                         ══════════════════════════════════════════════\n\
                         Log path  : {}\n\n\
                         [STEP 1] EE.log TRIGGER\n\
                         ├─ Time     : {}\n\
                         ├─ Line     : \"{}\"\n\
                         └─ Catalog  : {} items\n\n",
                        ts0, session_log_path.display(), ts0, trigger_line, ee_catalog.len()
                    ));
                    if let Err(e) = write_err {
                        eprintln!("[FrameForge] session log write failed: {e}");
                    }
                    let _ = std::fs::write(&ee_last_path, format!(
                        "=== {} ===\nEE.log trigger fired\n{}\n", ts0, trigger_line
                    ));

                    let _ = ee_ocr_app.emit("ff-status", "🔍 Relic reward screen detected");
                    // Tell App.tsx to pre-create the overlay window NOW, before OCR finishes.
                    // Window creation takes 1-2 s; pre-creating shaves that off the visible delay.
                    let _ = ee_ocr_app.emit("relic-trigger", ());

                    let app        = ee_ocr_app.clone();
                    let cat        = std::sync::Arc::clone(&ee_catalog);
                    let cat_len    = cat.len();
                    let lpath      = ee_last_path.clone();
                    let slog       = session_log_path.clone();
                    let active     = reward_screen_active2.clone();
                    let squad_arc  = std::sync::Arc::clone(&shared_squad_size);
                    // Also reset the shared squad size so stale data from a previous
                    // fissure doesn't bleed into this new screen's OCR loop.
                    if let Ok(mut g) = squad_arc.lock() { *g = ee_squad_size; }

                    tauri::async_runtime::spawn(async move {
                        let deadline = std::time::Instant::now()
                            + std::time::Duration::from_secs(45);
                        // 500ms initial delay — gives cards time to finish fading in and
                        // allows the VoidProjections EE.log sequence to arrive before the
                        // first OCR attempt reads hint_squad (race-condition fix).
                        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

                        // Allow the catalog to be rebuilt inside the loop — it may be empty
                        // when start_monitor fired before WFCD data finished loading.
                        let mut cat = cat;
                        let mut attempt = 0u32;
                        let mut best_item_count = 0usize;
                        let mut best_payload: Option<serde_json::Value> = None; // locked when complete
                        // When no EE squad hint is available, the first "complete" result may
                        // undercount cards (e.g. dark text hides a 2-line item name).
                        // soft_complete_at tracks the first attempt that returned complete-without-hint
                        // so we do one extra retry before locking.
                        let mut soft_complete_at: Option<usize> = None;
                        // Item count at the time soft_complete_at was set.
                        // If the follow-up attempt finds no more items, emit best_payload even if
                        // a newly-arrived EE hint raised estimated_cards above the count we saw.
                        // (Warframe can show fewer unique cards than squad size when players share
                        // the same relic reward — one player lacking reactant is another example.)
                        let mut soft_complete_count: usize = 0;
                        loop {
                            attempt += 1;
                            // Rebuild catalog if WFCD hadn't loaded when this OCR session started.
                            // Runs only while cat is empty — once populated it stays populated.
                            if cat.is_empty() {
                                let s = app.state::<AppState>();
                                let items_lock = s.wfcd_items.lock().unwrap_or_else(|e| e.into_inner());
                                if !items_lock.is_empty() {
                                    let bp_lock = s.blueprint_to_result.lock().unwrap_or_else(|e| e.into_inner());
                                    let bad = ["Warframes","Primary","Secondary","Melee","Companion",
                                               "Sentinels","Archwing","Arch-Gun","Arch-Melee","Pets","Robotic"];
                                    let mut fresh: Vec<(String,String)> = items_lock.iter()
                                        .filter(|i| {
                                            let lo = i.name.to_lowercase();
                                            !bad.contains(&i.category.as_str())
                                            && !lo.ends_with("intact") && !lo.ends_with("exceptional")
                                            && !lo.ends_with("flawless") && !lo.ends_with("radiant")
                                            && (lo.contains("prime") || lo.starts_with("forma"))
                                        })
                                        .map(|i| (i.unique_name.clone(), i.name.clone()))
                                        .collect();
                                    for (u, (n, _)) in bp_lock.iter() {
                                        let lo = n.to_lowercase();
                                        if lo.contains("prime") || lo.starts_with("forma") {
                                            fresh.push((u.clone(), n.clone()));
                                        }
                                    }
                                    fresh.sort_by(|a, b| a.0.cmp(&b.0));
                                    fresh.dedup_by(|a, b| a.0 == b.0);
                                    if !fresh.is_empty() {
                                        cat = std::sync::Arc::new(fresh);
                                    }
                                }
                            }
                            let _ = app.emit("ff-status", "📷 OCR scanning...");
                            let cat2 = std::sync::Arc::clone(&cat);
                            // Read squad size fresh for each attempt — it may arrive after
                            // the first attempt if VoidProjections sequence completes late.
                            let hint_squad = squad_arc.lock().ok().and_then(|g| *g);
                            let result = tauri::async_runtime::spawn_blocking(move || {
                                let (pixels, w, cap_h, full_h, cap_info) =
                                    ocr::capture_warframe_reward_area()?;
                                Some(ocr::extract_reward_items_twophase(
                                    &pixels, w, cap_h, full_h, &cat2, &cap_info, hint_squad,
                                ))
                            }).await.ok().flatten();

                            let ts = chrono::Local::now().format("%H:%M:%S%.3f");
                            let sleep_ms = match &result {
                                // ✅ 1+ items found (solo=1, duo=2, trio=3, full squad=4)
                                Some((complete, _, ref items, ref positions, ref dbg)) if !items.is_empty() => {
                                    let payload = Some(serde_json::json!({
                                        "items": items, "positions": positions
                                    }));

                                    // Determine whether this complete result should be locked now.
                                    // If we have an EE squad hint the count is authoritative.
                                    // If we don't (hint arrived late or solo/friend group),
                                    // do one extra retry first — a dark/fading card may have been
                                    // missed and the prime/forma word count underestimates 2-line names.
                                    let confirm_ready = hint_squad.is_some() || soft_complete_at.is_some();

                                    // Save best result; only emit to overlay when confirmed (LOCK).
                                    // Partial updates are intentionally suppressed — emitting
                                    // partial data while the user is still hovering cards causes
                                    // the overlay to flicker with wrong items between attempts.
                                    if items.len() > best_item_count {
                                        best_item_count = items.len();
                                        best_payload = payload.clone();
                                        let label = if *complete && confirm_ready { "✅" } else { "⚡" };
                                        let status_label = if *complete && confirm_ready { "locked" }
                                            else if *complete { "soft-complete, confirming" }
                                            else { "waiting" };
                                        let _ = app.emit("ff-status",
                                            format!("{} {} items ({})", label, items.len(), status_label));
                                        let result_label = if *complete && confirm_ready { "LOCKED & emitting" }
                                            else if *complete { "soft-complete, retrying once" }
                                            else { "saved, retrying" };
                                        let session_entry = format!(
                                            "[STEP 2] OCR ATTEMPT #{}\n\
                                             ├─ Time     : {}\n\
                                             {}\n\
                                             └─ RESULT   : {} items found → {}\n\
                                             └─ Items    : {:?}\n\n{}",
                                            attempt, ts, dbg, items.len(),
                                            result_label,
                                            items,
                                            if *complete && confirm_ready { "[STEP 3] OVERLAY OPENED\n\n" } else { "" }
                                        );
                                        let _ = append_to_file(&slog, &session_entry);
                                        let _ = std::fs::write(&lpath, format!(
                                            "=== {} ===\nItems: {:?}\n{}\n", ts, items, dbg));
                                    }

                                    // Stop retrying and emit ONLY when all expected cards found AND confirmed.
                                    if *complete {
                                        if confirm_ready {
                                            // Hard cutoff: if dismiss arrived while OCR was running, drop the result.
                                            if !active.load(Ordering::SeqCst) { break; }
                                            // Log the confirming attempt when item count didn't improve
                                            // (the logging block above only fires when items.len() > best_item_count).
                                            if items.len() <= best_item_count {
                                                let _ = append_to_file(&slog, &format!(
                                                    "[STEP 2] OCR ATTEMPT #{} (confirm)\n\
                                                     ├─ Time     : {}\n\
                                                     └─ {} items — same as before, confirmed\n\n\
                                                     [STEP 3] OVERLAY OPENED\n\n",
                                                    attempt, ts, items.len()
                                                ));
                                            }
                                            let _ = app.emit("relic-rewards", &payload);
                                            let app2 = app.clone();
                                            let slog2 = slog.clone();
                                            tauri::async_runtime::spawn(async move {
                                                // 20s safety fallback — normally the overlay closes
                                                // when EE.log fires "relic timer closed" (player picks).
                                                tokio::time::sleep(std::time::Duration::from_secs(20)).await;
                                                let _ = app2.emit("relic-rewards", serde_json::Value::Null);
                                                if let Some(w) = app2.get_webview_window("relic-overlay") {
                                                    let _ = w.close();
                                                }
                                                let _ = append_to_file(&slog2,
                                                    "[STEP 4] AUTO-DISMISS (20s safety fallback)\n\n");
                                            });
                                            break;
                                        } else {
                                            // First complete without an EE hint — mark and retry once.
                                            soft_complete_at = Some(attempt as usize);
                                            soft_complete_count = best_item_count;
                                        }
                                    } else if soft_complete_at.is_some() && items.len() <= soft_complete_count {
                                        // Soft-complete confirmation retry found no more items.
                                        // A late EE hint may have raised estimated_cards above what
                                        // the screen actually shows (e.g. squad=4 but only 3 unique
                                        // cards because one player lacked reactant or shared a reward).
                                        // Emit best_payload now rather than retrying until timeout.
                                        if !active.load(Ordering::SeqCst) { break; }
                                        let emit_val = best_payload.clone().unwrap_or(serde_json::Value::Null);
                                        let _ = app.emit("relic-rewards", &emit_val);
                                        let _ = append_to_file(&slog,
                                            "[STEP 3] OVERLAY OPENED (soft-complete confirmed — no improvement)\n\n");
                                        let app2 = app.clone();
                                        let slog2 = slog.clone();
                                        tauri::async_runtime::spawn(async move {
                                            tokio::time::sleep(std::time::Duration::from_secs(20)).await;
                                            let _ = app2.emit("relic-rewards", serde_json::Value::Null);
                                            if let Some(w) = app2.get_webview_window("relic-overlay") {
                                                let _ = w.close();
                                            }
                                            let _ = append_to_file(&slog2,
                                                "[STEP 4] AUTO-DISMISS (20s safety fallback)\n\n");
                                        });
                                        break;
                                    }
                                    // Partial result (or soft-complete pending confirmation) — retry
                                    400u64
                                }
                                // ⬛ Dark/blank frame — PrintWindow returned nearly-black
                                Some((_, _, _, _, ref dbg)) if dbg.starts_with("dark-frame") => {
                                    let entry = format!(
                                        "[STEP 2] OCR ATTEMPT #{}\n\
                                         ├─ Time     : {}\n\
                                         └─ RESULT   : {} → PrintWindow returned dark image\n\
                                            Check %TEMP%\\frameforge_capture_debug.bmp\n\
                                            Fix: switch Warframe to Borderless Windowed mode\n\
                                            Retrying in 100ms…\n\n",
                                        attempt, ts, dbg);
                                    let _ = append_to_file(&slog, &entry);
                                    let _ = std::fs::write(&lpath,
                                        format!("=== {} ===\n{} — retrying\n", ts, dbg));
                                    let _ = app.emit("ff-status", format!("⬛ {}", dbg));
                                    100u64
                                }
                                // ⬜ OCR ran but returned no text
                                Some((_, _, _, _, ref dbg)) if dbg.starts_with("ocr-empty") => {
                                    let entry = format!(
                                        "[STEP 2] OCR ATTEMPT #{}\n\
                                         ├─ Time     : {}\n\
                                         └─ RESULT   : {} → image has content but OCR found no text\n\
                                            Check %TEMP%\\frameforge_capture_debug.bmp\n\
                                            Retrying in 300ms…\n\n",
                                        attempt, ts, dbg);
                                    let _ = append_to_file(&slog, &entry);
                                    let _ = std::fs::write(&lpath,
                                        format!("=== {} ===\n{} — retrying\n", ts, dbg));
                                    let _ = app.emit("ff-status", format!("⬜ {}", dbg));
                                    300u64
                                }
                                // ❌ Text found but no catalog match
                                Some((_, _, ref items, _, ref dbg)) => {
                                    let entry = format!(
                                        "[STEP 2] OCR ATTEMPT #{}\n\
                                         ├─ Time     : {}\n\
                                         {}\n\
                                         └─ RESULT   : no catalog match (catalog={}) → retrying in 700ms\n\n",
                                        attempt, ts, dbg, cat_len);
                                    let _ = append_to_file(&slog, &entry);
                                    let _ = std::fs::write(&lpath, format!(
                                        "=== {} ===\nno match (catalog={}): {:?}\n{}\n",
                                        ts, cat_len, items, dbg));
                                    let _ = app.emit("ff-status", "❌ No catalog match, retrying...");
                                    700u64
                                }
                                // ⚠️ Warframe window not found
                                None => {
                                    let entry = format!(
                                        "[STEP 2] OCR ATTEMPT #{}\n\
                                         ├─ Time     : {}\n\
                                         └─ RESULT   : capture failed — Warframe window not found\n\
                                            Retrying in 500ms…\n\n",
                                        attempt, ts);
                                    let _ = append_to_file(&slog, &entry);
                                    let _ = std::fs::write(&lpath,
                                        format!("=== {} ===\nCapture failed (window not found?)\n", ts));
                                    let _ = app.emit("ff-status", "⚠️ Capture failed");
                                    500u64
                                }
                            };

                            if std::time::Instant::now() >= deadline {
                                // Emit best partial result if we found anything, otherwise null.
                                // This means even a timeout shows something rather than nothing
                                // when OCR found cards but couldn't reach the expected count.
                                let emit_val = if active.load(Ordering::SeqCst) {
                                    best_payload.unwrap_or(serde_json::Value::Null)
                                } else {
                                    serde_json::Value::Null
                                };
                                let _ = app.emit("relic-rewards", &emit_val);
                                let _ = append_to_file(&slog,
                                    "[STEP 2] OCR TIMEOUT — 45 seconds elapsed, emitting best result\n\n");
                                if let Some(win) = app.get_webview_window("relic-overlay") {
                                    let _ = win.close();
                                }
                                active.store(false, Ordering::SeqCst);
                                break;
                            }
                            if !active.load(Ordering::SeqCst) {
                                let _ = append_to_file(&slog,
                                    "[STEP 2] OCR STOPPED — dismiss signal received\n\n");
                                break;
                            }
                            tokio::time::sleep(std::time::Duration::from_millis(sleep_ms)).await;
                        }
                    });

                } // end trigger block

                // Auto-dismiss after 20 s — safety net only.
                // Normal close path is EE.log "relic timer closed" above.
                if let Some(since) = active_since {
                    if since.elapsed().as_secs() >= 20 {
                        let ts_a = chrono::Local::now().format("%H:%M:%S%.3f");
                        let _ = append_to_file(&session_log_path, &format!(
                            "[STEP 4] AUTO-DISMISS (20s timeout)\n\
                             └─ Time : {}\n\n",
                            ts_a
                        ));
                        reward_screen_active2.store(false, Ordering::SeqCst);
                        active_since = None;
                        last_dismiss_at = Some(std::time::Instant::now());
                        if let Some(win) = ee_ocr_app.get_webview_window("relic-overlay") {
                            let _ = win.close();
                        }
                        let _ = ee_ocr_app.emit("relic-rewards", serde_json::Value::Null);
                    }
                }
            }
        });
    }

    // OCR polling fallback removed — it ran every second with no EE.log context
    // guard, causing false overlays on Mission Complete, orbiter, Last Mission
    // Results, and any screen with Prime item names visible.
    // The EE.log watcher already retries OCR for 45 seconds after the trigger,
    // so the fallback is both redundant and harmful.

    std::thread::spawn(move || {
        // Initialize COM (required for Windows OCR / WinRT APIs).
        // std::thread::spawn creates a raw OS thread with no COM apartment;
        // WinRT calls silently fail without this, returning empty strings.
        #[cfg(target_os = "windows")]
        unsafe {
            windows_sys::Win32::System::Com::CoInitializeEx(
                std::ptr::null(),
                windows_sys::Win32::System::Com::COINIT_MULTITHREADED.try_into().unwrap(),
            );
        }

        while reward_flag.load(Ordering::SeqCst) {
            let _relic_screen = false;
            let mut debug = String::new();
            let ts = chrono::Local::now().format("%H:%M:%S%.3f");
            debug.push_str(&format!("=== {} ===\n", ts));

            // OCR is now triggered by the EE.log watcher (AlecaFrame-style),
            // not by this polling loop. This loop only handles inventory scanning.
            let rewards: Option<serde_json::Value> = None;

            let _ = std::fs::write(&debug_path, &debug);
            if rewards.is_some() {
                let _ = std::fs::write(&last_found_path, &debug);
            }

            // Overlay is controlled entirely by the EE.log watcher — do NOT emit here.
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
    });

    Ok(())
}

/// Append a string to a file, creating the file if it doesn't exist.
fn append_to_file(path: &std::path::Path, text: &str) -> std::io::Result<()> {
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(path)?;
    f.write_all(text.as_bytes())
}

// ─── Localisation lookup ──────────────────────────────────────────────────────

static LANG: std::sync::OnceLock<std::collections::HashMap<String, String>> = std::sync::OnceLock::new();

fn get_lang() -> &'static std::collections::HashMap<String, String> {
    LANG.get_or_init(|| {
        ureq::get("https://raw.githubusercontent.com/WFCD/warframe-worldstate-data/master/data/languages.json")
            .call()
            .ok()
            .and_then(|r| r.into_json::<serde_json::Value>().ok())
            .and_then(|v| v.as_object().map(|obj| {
                obj.iter().filter_map(|(k, val)| {
                    let text = val.get("value")?.as_str()?;
                    Some((k.clone(), text.to_string()))
                }).collect()
            }))
            .unwrap_or_default()
    })
}

/// Resolve a /Lotus/Language/... path to its English display name.
fn loc(path: &str) -> String {
    if let Some(name) = get_lang().get(path) {
        return name.clone();
    }
    // Fallback: strip the path prefix and convert the last component from PascalCase
    path_display_name(path)
}

// ─── Node name lookup ─────────────────────────────────────────────────────────

#[derive(Clone)]
struct SolNode {
    display: String,
    enemy: String,
    mission_type: String,
}

static SOL_NODES: std::sync::OnceLock<std::collections::HashMap<String, SolNode>> = std::sync::OnceLock::new();

fn get_sol_nodes() -> &'static std::collections::HashMap<String, SolNode> {
    SOL_NODES.get_or_init(|| {
        ureq::get("https://raw.githubusercontent.com/WFCD/warframe-worldstate-data/master/data/solNodes.json")
            .call()
            .ok()
            .and_then(|r| r.into_json::<serde_json::Value>().ok())
            .and_then(|v| v.as_object().map(|obj| {
                obj.iter().filter_map(|(k, val)| {
                    let display = val.get("value")?.as_str()?.to_string();
                    let enemy = val.get("enemy").and_then(|e| e.as_str()).unwrap_or("").to_string();
                    let mission_type = val.get("type").and_then(|t| t.as_str()).unwrap_or("").to_string();
                    Some((k.clone(), SolNode { display, enemy, mission_type }))
                }).collect()
            }))
            .unwrap_or_default()
    })
}

fn resolve_node(id: &str) -> String {
    if let Some(n) = get_sol_nodes().get(id) { return n.display.clone(); }
    if id.ends_with("HUB") { return format!("{} Relay", &id[..id.len()-3]); }
    if id.starts_with("CrewBattleNode") { return format!("Railjack {}", &id[14..]); }
    id.to_string()
}

fn node_enemy(id: &str) -> String {
    get_sol_nodes().get(id).map(|n| n.enemy.clone()).unwrap_or_default()
}

fn node_mission_type(id: &str) -> String {
    get_sol_nodes().get(id).map(|n| n.mission_type.clone()).unwrap_or_default()
}

/// Convert a Unix millisecond timestamp to an ISO-8601 string without external crates.
fn ms_to_iso(ms: i64) -> String {
    let millis = ms.rem_euclid(1000);
    let total_secs = ms / 1000;
    let s_in_day = total_secs.rem_euclid(86400) as u32;
    let days = total_secs.div_euclid(86400);
    let hour = s_in_day / 3600;
    let min = (s_in_day % 3600) / 60;
    let sec = s_in_day % 60;
    // Howard Hinnant civil_from_days
    let z = days + 719468_i64;
    let era = z.div_euclid(146097_i64);
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe/1460 + doe/36524 - doe/146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365*yoe + yoe/4 - yoe/100);
    let mp = (5*doy + 2) / 153;
    let d = doy - (153*mp + 2)/5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z", year, m, d, hour, min, sec, millis)
}

/// Extract milliseconds from a MongoDB Extended JSON date: {"$date":{"$numberLong":"..."}}
fn ws_ms(v: &serde_json::Value) -> i64 {
    v.get("$date")
        .and_then(|d| d.get("$numberLong"))
        .and_then(|n| n.as_str())
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(0)
}

fn ws_mission_type(mt: &str) -> String {
    let known = match mt {
        "MT_ASSASSINATION"    => "Assassination",
        "MT_CAPTURE"          => "Capture",
        "MT_DEFENSE"          => "Defense",
        "MT_EVACUATION"       => "Defection",
        "MT_EXCAVATE"         => "Excavation",
        "MT_EXTERMINATION"    => "Extermination",
        "MT_HIVE"             => "Hive",
        "MT_HIVE_SABOTAGE"    => "Hive Sabotage",
        "MT_INFECTION"        => "Infested Salvage",
        "MT_INTEL"            => "Spy",
        "MT_MOBILE_DEFENSE"   => "Mobile Defense",
        "MT_RESCUE"           => "Rescue",
        "MT_RETRIEVAL"        => "Retrieval",
        "MT_SABOTAGE"         => "Sabotage",
        "MT_SPY"              => "Spy",
        "MT_SURVIVAL"         => "Survival",
        "MT_TERRITORY"        => "Interception",
        "MT_PURIFY"           => "Onslaught",
        "MT_ARTIFACT"         => "Disruption",
        "MT_RAILJACK"         => "Railjack",
        "MT_SKIRMISH"         => "Skirmish",
        "MT_JUNCTION"         => "Junction",
        "MT_LANDSCAPE"        => "Open World",
        "MT_FREE_ROAM"        => "Free Roam",
        "MT_ARENA"            => "Arena",
        "MT_ASSAULT"          => "Assault",
        "MT_ORPHIX"           => "Orphix",
        "MT_VOID_CASCADE"     => "Void Cascade",
        "MT_VOID_FLOOD"       => "Void Flood",
        "MT_VOID_ARMAGEDDON"  => "Void Armageddon",
        "MT_MIRROR_DEFENSE"   => "Mirror Defense",
        "MT_CAMP"             => "Volatile",
        "MT_BOUNTY"           => "Bounty",
        _ => "",
    };
    if !known.is_empty() {
        return known.to_string();
    }
    // Strip MT_ prefix and convert SCREAMING_SNAKE_CASE to Title Case
    let stripped = mt.strip_prefix("MT_").unwrap_or(mt);
    stripped.split('_')
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                None => String::new(),
                Some(f) => f.to_uppercase().to_string() + &c.as_str().to_lowercase(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn ws_sortie_boss(boss: &str) -> (&'static str, &'static str) {
    // Returns (display_name, faction)
    match boss {
        "SORTIE_BOSS_RAPTOR"       => ("Raptor",              "Corpus"),
        "SORTIE_BOSS_ALAD_V"       => ("Alad V",              "Corpus"),
        "SORTIE_BOSS_HYENA"        => ("Hyena Pack",          "Corpus"),
        "SORTIE_BOSS_AMBULAS"      => ("Ambulas",             "Corpus"),
        "SORTIE_BOSS_SERGEANT"     => ("The Sergeant",        "Corpus"),
        "SORTIE_BOSS_JACKAL"       => ("Jackal",              "Corpus"),
        "SORTIE_BOSS_ROPALOLYST"   => ("Ropalolyst",          "Corpus"),
        "SORTIE_BOSS_KELA"         => ("Kela De Thaym",       "Grineer"),
        "SORTIE_BOSS_VOR"          => ("Captain Vor",         "Grineer"),
        "SORTIE_BOSS_RUK"          => ("General Sargas Ruk",  "Grineer"),
        "SORTIE_BOSS_THW"          => ("Tyl Regor",           "Grineer"),
        "SORTIE_BOSS_LECH_KRIL"    => ("Lt. Lech Kril",       "Grineer"),
        "SORTIE_BOSS_KRIL_AND_VOR" => ("Vor & Kril",          "Grineer"),
        "SORTIE_BOSS_CORRUPTED_VOR"=> ("Corrupted Vor",       "Orokin"),
        _                          => ("Unknown Boss",        "Unknown"),
    }
}

fn ws_sortie_modifier(m: &str) -> &'static str {
    match m {
        "SORTIE_MODIFIER_RADIATION"          => "Radiation Hazard",
        "SORTIE_MODIFIER_MAGNETIC"           => "Magnetic Anomaly",
        "SORTIE_MODIFIER_BOW_ONLY"           => "Bow Only",
        "SORTIE_MODIFIER_SHOTGUN_ONLY"       => "Shotgun Only",
        "SORTIE_MODIFIER_SNIPER_ONLY"        => "Sniper Rifle Only",
        "SORTIE_MODIFIER_MELEE_ONLY"         => "Melee Only",
        "SORTIE_MODIFIER_LOW_ENERGY"         => "Low Energy",
        "SORTIE_MODIFIER_EXIMUS"             => "Eximus Stronghold",
        "SORTIE_MODIFIER_SECONDARY_ONLY"     => "Secondary Only",
        "SORTIE_MODIFIER_ASSAULT_RIFLE_ONLY" => "Assault Rifle Only",
        "SORTIE_MODIFIER_IMPACT"             => "Augmented Enemy Armor",
        "SORTIE_MODIFIER_ELEMENTAL_ENHANCEMENT" => "Elemental Enhancement",
        _                                    => "Modifier",
    }
}

fn ws_faction(f: &str) -> String {
    match f {
        "FC_GRINEER"    => "Grineer",
        "FC_CORPUS"     => "Corpus",
        "FC_INFESTATION"=> "Infested",
        "FC_OROKIN"     => "Orokin",
        "FC_CORRUPTED"  => "Corrupted",
        "FC_TENNO"      => "Tenno",
        "FC_MITW"       => "Murmur",
        _               => f.trim_start_matches("FC_"),
    }.to_string()
}

/// Extract a display name from a /Lotus/ asset path.
fn path_display_name(path: &str) -> String {
    let last = path.split('/').last().unwrap_or(path);
    // Strip known internal prefixes that are never part of the display name
    let stripped = last
        .strip_prefix("MPV")   // MegaPrimeVault bundles, e.g. MPVRhinoPrimeSinglePack
        .unwrap_or(last);
    // Convert PascalCase → "Pascal Case"
    let mut out = String::with_capacity(stripped.len() + 8);
    let mut prev_was_upper = false;
    for (i, ch) in stripped.chars().enumerate() {
        if ch.is_uppercase() && i > 0 && !prev_was_upper {
            out.push(' ');
        }
        out.push(ch);
        prev_was_upper = ch.is_uppercase();
    }
    // Strip common suffixes that add no value
    for suffix in &[" Item", " Resource Item", " Reward"] {
        if out.ends_with(suffix) {
            out.truncate(out.len() - suffix.len());
            break;
        }
    }
    out
}

/// Map store item paths to catalog unique_names where possible.
/// /Lotus/StoreItems/X   → /Lotus/X        (direct catalog items like mods, primes)
/// /Lotus/Types/StoreItems/... → unchanged  (bundle packages — no catalog entry)
fn store_to_unique(path: &str) -> String {
    path.replacen("/Lotus/StoreItems/", "/Lotus/", 1)
}

/// Resolve a store item path to a display name using the catalog, falling back to path parsing.
fn item_display_name(path: &str, catalog: &std::collections::HashMap<String, String>) -> String {
    // Try /Lotus/StoreItems/X → /Lotus/X mapping
    let unique = store_to_unique(path);
    if let Some(name) = catalog.get(&unique) {
        return name.clone();
    }
    // Try /Lotus/Types/StoreItems/... → /Lotus/Types/... (cosmetics, song items, etc.)
    if let Some(rest) = path.strip_prefix("/Lotus/Types/StoreItems/") {
        let alt = format!("/Lotus/Types/{}", rest);
        if let Some(name) = catalog.get(&alt) {
            return name.clone();
        }
    }
    path_display_name(path)
}

/// Parse DE raw worldstate JSON into the shape TimerHelper.tsx expects.
fn parse_worldstate_value(raw: &serde_json::Value, now_ms: i64, catalog: &std::collections::HashMap<String, String>) -> serde_json::Value {
    use serde_json::{json, Value};

    // ── World cycles ──────────────────────────────────────────────────────
    let mut cetus   = Value::Null;
    let mut vallis  = Value::Null;
    let mut cambion = Value::Null;

    if let Some(missions) = raw["SyndicateMissions"].as_array() {
        for m in missions {
            let tag = m["Tag"].as_str().unwrap_or("");
            let expiry_ms     = ws_ms(&m["Expiry"]);
            let activation_ms = ws_ms(&m["Activation"]);
            let duration_ms   = expiry_ms - activation_ms;
            match tag {
                "CetusSyndicate" => {
                    // Day ~6000 s, Night ~3000 s; threshold 4500 s
                    cetus = json!({ "expiry": ms_to_iso(expiry_ms), "isDay": duration_ms > 4_500_000_i64 });
                }
                "SolarisSyndicate" => {
                    // Cold ~1600 s, Warm ~400 s; threshold 1000 s
                    vallis = json!({ "expiry": ms_to_iso(expiry_ms), "isWarm": duration_ms < 1_000_000_i64 });
                }
                "EntatiSyndicate" => {
                    // Cambion Drift — Fass/Vome have equal duration; show countdown only
                    cambion = json!({ "expiry": ms_to_iso(expiry_ms), "active": "cycle" });
                }
                _ => {}
            }
        }
    }

    // ── Sortie ────────────────────────────────────────────────────────────
    let sortie = raw["Sorties"].as_array()
        .and_then(|a| a.first())
        .map(|s| {
            let expiry_ms = ws_ms(&s["Expiry"]);
            let boss_key  = s["Boss"].as_str().unwrap_or("");
            let (boss, faction) = ws_sortie_boss(boss_key);
            let variants: Vec<Value> = s["Variants"].as_array()
                .map(|arr| arr.iter().map(|v| json!({
                    "missionType": ws_mission_type(v["missionType"].as_str().unwrap_or("")),
                    "modifier":    ws_sortie_modifier(v["modifierType"].as_str().unwrap_or("")),
                    "node":        v["node"].as_str().unwrap_or(""),
                })).collect())
                .unwrap_or_default();
            json!({ "expiry": ms_to_iso(expiry_ms), "boss": boss, "faction": faction,
                    "variants": variants, "active": now_ms < expiry_ms })
        })
        .unwrap_or(Value::Null);

    // ── Archon Hunt (LiteSorties) ─────────────────────────────────────────
    let archon_hunt = raw["LiteSorties"].as_array()
        .and_then(|a| a.first())
        .map(|s| {
            let expiry_ms = ws_ms(&s["Expiry"]);
            let boss_raw  = s["Boss"].as_str().unwrap_or("");
            // Boss might be a /Lotus/ path; extract the last component
            let boss = boss_raw.split('/').last().unwrap_or(boss_raw)
                .trim_start_matches("Archon");
            let missions: Vec<Value> = s["Variants"].as_array()
                .map(|arr| arr.iter().map(|v| json!({
                    "type": ws_mission_type(v["missionType"].as_str().unwrap_or("")),
                    "node": v["node"].as_str().unwrap_or(""),
                })).collect())
                .unwrap_or_default();
            json!({ "expiry": ms_to_iso(expiry_ms), "boss": boss, "faction": "Infested",
                    "missions": missions, "active": now_ms < expiry_ms })
        })
        .unwrap_or(Value::Null);

    // ── Void Trader ───────────────────────────────────────────────────────
    let void_trader = raw["VoidTraders"].as_array()
        .and_then(|a| a.first())
        .map(|t| {
            let activation_ms = ws_ms(&t["Activation"]);
            let expiry_ms     = ws_ms(&t["Expiry"]);
            let node          = t["Node"].as_str().unwrap_or("");
            let active = now_ms >= activation_ms && now_ms < expiry_ms;
            let manifest: Vec<Value> = if active {
                t["Manifest"].as_array().map(|arr| arr.iter().map(|item| {
                    let raw_path = item["ItemType"].as_str().unwrap_or("");
                    let name = item_display_name(raw_path, catalog);
                    json!({
                        "name": name,
                        "uniqueName": store_to_unique(raw_path),
                        "primePrice": item["PrimePrice"].as_i64().unwrap_or(0),
                        "regularPrice": item["RegularPrice"].as_i64().unwrap_or(0),
                    })
                }).collect()).unwrap_or_default()
            } else { vec![] };
            json!({
                "activation": ms_to_iso(activation_ms),
                "expiry":     ms_to_iso(expiry_ms),
                "character":  "Baro Ki'Teer",
                "location":   resolve_node(node),
                "active":     active,
                "manifest":   manifest,
            })
        })
        .unwrap_or(Value::Null);

    // ── Prime Resurgence (PrimeVaultTraders) ──────────────────────────────
    let prime_resurgence = raw["PrimeVaultTraders"].as_array()
        .and_then(|a| a.first())
        .map(|t| {
            let activation_ms = ws_ms(&t["Activation"]);
            let expiry_ms     = ws_ms(&t["Expiry"]);
            let active = now_ms >= activation_ms && now_ms < expiry_ms;
            let manifest: Vec<Value> = t["Manifest"].as_array().map(|arr| arr.iter().map(|item| {
                let raw_path = item["ItemType"].as_str().unwrap_or("");
                let name = item_display_name(raw_path, catalog);
                let aya = item["PrimePrice"].as_i64().unwrap_or(0);
                json!({ "name": name, "uniqueName": store_to_unique(raw_path), "ayaPrice": aya })
            }).collect()).unwrap_or_default();
            json!({
                "activation": ms_to_iso(activation_ms),
                "expiry":     ms_to_iso(expiry_ms),
                "active":     active,
                "manifest":   manifest,
            })
        })
        .unwrap_or(Value::Null);

    // ── Nightwave (SeasonInfo) ────────────────────────────────────────────
    let nightwave = raw.get("SeasonInfo")
        .filter(|s| !s.is_null())
        .map(|s| {
            let expiry_ms = ws_ms(&s["Expiry"]);
            let season    = s["Season"].as_i64().unwrap_or(0);
            json!({ "expiry": ms_to_iso(expiry_ms), "season": season, "active": now_ms < expiry_ms })
        })
        .unwrap_or(Value::Null);

    // ── Fissures (ActiveMissions) ─────────────────────────────────────────
    let fissures: Vec<Value> = raw["ActiveMissions"].as_array()
        .map(|arr| arr.iter().filter_map(|f| {
            let modifier = f["Modifier"].as_str()?;
            if !modifier.starts_with("VoidT") { return None; }
            if f["Hard"].as_bool().unwrap_or(false) { return None; }
            let activation_ms = ws_ms(&f["Activation"]);
            let expiry_ms     = ws_ms(&f["Expiry"]);
            if activation_ms > now_ms { return None; } // not started yet
            if expiry_ms <= now_ms    { return None; }
            let (tier, tier_num) = match modifier {
                "VoidT1" => ("Lith",    1u32),
                "VoidT2" => ("Meso",    2),
                "VoidT3" => ("Neo",     3),
                "VoidT4" => ("Axi",     4),
                "VoidT5" => ("Requiem", 5),
                "VoidT6" => ("Omnia",   6),
                _        => return None,
            };
            let id   = f["_id"]["$oid"].as_str().unwrap_or("").to_string();
            let node = f["Node"].as_str().unwrap_or("");
            let mt   = ws_mission_type(f["MissionType"].as_str().unwrap_or(""));
            let enemy = node_enemy(node);
            Some(json!({
                "id": id, "expiry": ms_to_iso(expiry_ms),
                "node": resolve_node(node), "missionType": mt,
                "tier": tier, "tierNum": tier_num,
                "enemy": enemy, "isStorm": false, "isHard": false, "active": true,
            }))
        }).collect())
        .unwrap_or_default();

    // ── Bounties (all open worlds) ────────────────────────────────────────
    let mut bounties = serde_json::Map::new();
    for m in raw["SyndicateMissions"].as_array().iter().flat_map(|a| a.iter()) {
        let tag = m["Tag"].as_str().unwrap_or("");
        let expiry_ms = ws_ms(&m["Expiry"]);
        let job_count = m["Jobs"].as_array().map(|j| j.len()).unwrap_or(0);
        let label = match tag {
            "CetusSyndicate"     => "cetus",
            "SolarisSyndicate"   => "vallis",
            "EntratiSyndicate"   => "cambion",
            "ZarimanSyndicate"   => "zariman",
            "HexSyndicate"       => "hex",
            "EntratiLabSyndicate"=> "entrati-lab",
            _                    => continue,
        };
        bounties.insert(label.to_string(), json!({
            "expiry": ms_to_iso(expiry_ms),
            "jobCount": job_count,
        }));
        // Also set cycle state for Zariman
        if tag == "ZarimanSyndicate" {
            // Zariman cycle is tied to bounty rotation
        }
    }

    // ── Zariman cycle (same expiry as bounties) ───────────────────────────
    let zariman = bounties.get("zariman")
        .map(|b| json!({ "expiry": b["expiry"], "active": true }))
        .unwrap_or(Value::Null);

    // ── Alerts ────────────────────────────────────────────────────────────
    let alerts: Vec<Value> = raw["Alerts"].as_array()
        .map(|arr| arr.iter().filter_map(|a| {
            let expiry_ms = ws_ms(&a["Expiry"]);
            if expiry_ms <= now_ms { return None; }
            let mi = &a["MissionInfo"];
            let reward = mi["missionReward"].as_object();
            let reward_item = reward
                .and_then(|r| r.get("countedItems"))
                .and_then(|ci| ci.as_array())
                .and_then(|arr| arr.first())
                .and_then(|item| item["ItemType"].as_str())
                .map(path_display_name);
            let reward_credits = reward
                .and_then(|r| r.get("credits"))
                .and_then(|c| c.as_i64())
                .unwrap_or(0);
            let id = a["_id"]["$oid"].as_str().unwrap_or("").to_string();
            Some(json!({
                "id": id,
                "expiry": ms_to_iso(expiry_ms),
                "missionType": ws_mission_type(mi["missionType"].as_str().unwrap_or("")),
                "faction": ws_faction(mi["faction"].as_str().unwrap_or("")),
                "node": mi["location"].as_str().unwrap_or(""),
                "rewardItem": reward_item,
                "rewardCredits": reward_credits,
            }))
        }).collect())
        .unwrap_or_default();

    // ── Invasions (active only) ────────────────────────────────────────────
    let invasions: Vec<Value> = raw["Invasions"].as_array()
        .map(|arr| arr.iter().filter_map(|inv| {
            if inv["Completed"].as_bool().unwrap_or(false) { return None; }
            let id   = inv["_id"]["$oid"].as_str().unwrap_or("").to_string();
            let node = resolve_node(inv["Node"].as_str().unwrap_or(""));
            let attacker = ws_faction(inv["Faction"].as_str().unwrap_or(""));
            let defender = ws_faction(inv["DefenderFaction"].as_str().unwrap_or(""));
            let count = inv["Count"].as_i64().unwrap_or(0);
            let goal  = inv["Goal"].as_i64().unwrap_or(1);
            let pct   = (count.abs() as f64 / goal.abs().max(1) as f64 * 100.0) as i64;
            let att_reward = inv["AttackerReward"]["countedItems"].as_array()
                .and_then(|a| a.first()).and_then(|i| i["ItemType"].as_str())
                .map(path_display_name).unwrap_or_default();
            let def_reward = inv["DefenderReward"]["countedItems"].as_array()
                .and_then(|a| a.first()).and_then(|i| i["ItemType"].as_str())
                .map(path_display_name).unwrap_or_default();
            Some(json!({
                "id": id, "node": node,
                "attacker": attacker, "defender": defender,
                "attReward": att_reward, "defReward": def_reward,
                "pct": pct,
            }))
        }).collect())
        .unwrap_or_default();

    // ── Steel Path fissures ────────────────────────────────────────────────
    let sp_fissures: Vec<Value> = raw["ActiveMissions"].as_array()
        .map(|arr| arr.iter().filter_map(|f| {
            if !f["Hard"].as_bool().unwrap_or(false) { return None; }
            let modifier      = f["Modifier"].as_str()?;
            if !modifier.starts_with("VoidT") { return None; }
            let activation_ms = ws_ms(&f["Activation"]);
            let expiry_ms     = ws_ms(&f["Expiry"]);
            if activation_ms > now_ms { return None; }
            if expiry_ms <= now_ms    { return None; }
            let (tier, tier_num) = match modifier {
                "VoidT1" => ("Lith", 1u32), "VoidT2" => ("Meso", 2),
                "VoidT3" => ("Neo", 3),     "VoidT4" => ("Axi", 4),
                "VoidT5" => ("Requiem", 5), "VoidT6" => ("Omnia", 6),
                _ => return None,
            };
            let id    = f["_id"]["$oid"].as_str().unwrap_or("").to_string();
            let node  = f["Node"].as_str().unwrap_or("");
            let enemy = node_enemy(node);
            Some(json!({
                "id": id, "expiry": ms_to_iso(expiry_ms),
                "node": resolve_node(node),
                "missionType": ws_mission_type(f["MissionType"].as_str().unwrap_or("")),
                "tier": tier, "tierNum": tier_num,
                "enemy": enemy, "isStorm": false, "isHard": true, "active": true,
            }))
        }).collect())
        .unwrap_or_default();

    // ── Void Storms ────────────────────────────────────────────────────────
    let void_storms: Vec<Value> = raw["VoidStorms"].as_array()
        .map(|arr| arr.iter().filter_map(|s| {
            let activation_ms = ws_ms(&s["Activation"]);
            let expiry_ms     = ws_ms(&s["Expiry"]);
            if activation_ms > now_ms { return None; }
            if expiry_ms <= now_ms    { return None; }
            let modifier = s["ActiveMissionTier"].as_str().unwrap_or("");
            let (tier, tier_num) = match modifier {
                "VoidT1" => ("Lith", 1u32), "VoidT2" => ("Meso", 2),
                "VoidT3" => ("Neo", 3),     "VoidT4" => ("Axi", 4),
                "VoidT5" => ("Requiem", 5), "VoidT6" => ("Omnia", 6),
                _ => return None,
            };
            let id       = s["_id"]["$oid"].as_str().unwrap_or("").to_string();
            let node_id  = s["Node"].as_str().unwrap_or("");
            let mt       = node_mission_type(node_id);
            let enemy    = node_enemy(node_id);
            Some(json!({
                "id": id, "expiry": ms_to_iso(expiry_ms),
                "node": resolve_node(node_id),
                "missionType": if mt.is_empty() { "Railjack".to_string() } else { mt },
                "enemy": enemy,
                "tier": tier, "tierNum": tier_num,
                "active": true,
            }))
        }).collect())
        .unwrap_or_default();

    // ── Darvo Daily Deal ──────────────────────────────────────────────────
    let darvo = raw["DailyDeals"].as_array()
        .and_then(|a| a.first())
        .map(|d| {
            let expiry_ms = ws_ms(&d["Expiry"]);
            let item_path = d["StoreItem"].as_str().unwrap_or("");
            json!({
                "expiry": ms_to_iso(expiry_ms),
                "item": path_display_name(item_path),
                "discount": d["Discount"].as_i64().unwrap_or(0),
                "originalPrice": d["OriginalPrice"].as_i64().unwrap_or(0),
                "salePrice": d["SalePrice"].as_i64().unwrap_or(0),
                "amountTotal": d["AmountTotal"].as_i64().unwrap_or(0),
                "amountSold": d["AmountSold"].as_i64().unwrap_or(0),
            })
        })
        .unwrap_or(Value::Null);

    // ── The Circuit (Duviri weekly) ───────────────────────────────────────
    let circuit = raw["EndlessXpSchedule"].as_array()
        .and_then(|a| a.first())
        .map(|s| {
            let expiry_ms = ws_ms(&s["Expiry"]);
            let choices = s["CategoryChoices"].as_array();
            let normal: Vec<&str> = choices.iter().flat_map(|a| a.iter())
                .find(|c| c["Category"].as_str() == Some("EXC_NORMAL"))
                .and_then(|c| c["Choices"].as_array())
                .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
                .unwrap_or_default();
            let hard: Vec<&str> = choices.iter().flat_map(|a| a.iter())
                .find(|c| c["Category"].as_str() == Some("EXC_HARD"))
                .and_then(|c| c["Choices"].as_array())
                .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
                .unwrap_or_default();
            json!({
                "expiry": ms_to_iso(expiry_ms),
                "normalFrames": normal,
                "hardWeapons": hard,
            })
        })
        .unwrap_or(Value::Null);

    // ── Kahl / Break Narmer ───────────────────────────────────────────────
    let kahl = raw["SyndicateMissions"].as_array()
        .and_then(|a| a.iter().find(|m| m["Tag"].as_str() == Some("KahlSyndicate")))
        .map(|m| {
            let expiry_ms = ws_ms(&m["Expiry"]);
            json!({ "expiry": ms_to_iso(expiry_ms) })
        })
        .unwrap_or(Value::Null);

    // ── Deep Archimedea (Descents) ────────────────────────────────────────
    let deep_archimedea = raw["Descents"].as_array()
        .and_then(|a| a.first())
        .map(|d| {
            let expiry_ms = ws_ms(&d["Expiry"]);
            json!({ "expiry": ms_to_iso(expiry_ms) })
        })
        .unwrap_or(Value::Null);

    // ── Active Goal / Community Event ─────────────────────────────────────
    let active_event = raw["Goals"].as_array()
        .and_then(|a| a.iter().find(|g| {
            ws_ms(&g["Expiry"]) > now_ms
        }))
        .map(|g| {
            let expiry_ms = ws_ms(&g["Expiry"]);
            let desc = g["Desc"].as_str().unwrap_or("");
            let label = loc(desc);
            json!({ "expiry": ms_to_iso(expiry_ms), "label": label })
        })
        .unwrap_or(Value::Null);

    json!({
        "cetus": cetus, "vallis": vallis, "cambion": cambion, "zariman": zariman,
        "bounties": bounties,
        "sortie": sortie, "archonHunt": archon_hunt,
        "voidTrader": void_trader, "primeResurgence": prime_resurgence, "nightwave": nightwave,
        "circuit": circuit, "kahl": kahl, "deepArchimedea": deep_archimedea,
        "activeEvent": active_event,
        "darvo": darvo,
        "alerts": alerts,
        "invasions": invasions,
        "fissures": fissures,
        "spFissures": sp_fissures,
        "voidStorms": void_storms,
    })
}

/// Fetch and parse the DE official Warframe worldstate.
/// Runs on a blocking thread so the async runtime is never stalled.
#[tauri::command]
async fn fetch_worldstate(state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    // Snapshot catalog for name lookups — do this before entering spawn_blocking
    let catalog: std::collections::HashMap<String, String> = {
        let items = state.wfcd_items.lock().unwrap_or_else(|e| e.into_inner());
        items.iter().map(|i| (i.unique_name.clone(), i.name.clone())).collect()
    };
    tokio::task::spawn_blocking(move || -> Result<serde_json::Value, String> {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        let raw = ureq::get("https://api.warframe.com/cdn/worldState.php")
            .set("User-Agent", "FrameForge/1.0")
            .call()
            .map_err(|e| format!("worldstate fetch failed: {}", e))?
            .into_json::<serde_json::Value>()
            .map_err(|e| format!("worldstate parse failed: {}", e))?;
        Ok(parse_worldstate_value(&raw, now_ms, &catalog))
    })
    .await
    .map_err(|e| format!("task error: {}", e))?
}

/// Read the current overlay session log.
#[tauri::command]
fn get_overlay_session_log() -> String {
    let path = std::env::temp_dir().join("frameforge_overlay_session.txt");
    std::fs::read_to_string(&path).unwrap_or_else(|_| "(no session log yet — trigger a Void Fissure first)".into())
}

/// Returns the Warframe game CLIENT AREA as [x, y, width, height] in screen pixels.
/// Uses GetClientRect + ClientToScreen so the rect matches what the OCR captures —
/// both exclude the window title bar and borders in windowed mode.
#[tauri::command]
fn get_warframe_window_rect() -> Result<[i32; 4], String> {
    #[cfg(not(target_os = "windows"))]
    { return Err("Windows only".into()); }
    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::Foundation::{POINT, RECT};
        use windows_sys::Win32::UI::WindowsAndMessaging::{FindWindowW, GetClientRect};
        use windows_sys::Win32::Graphics::Gdi::ClientToScreen;

        let title: Vec<u16> = "Warframe\0".encode_utf16().collect();
        let hwnd = unsafe { FindWindowW(std::ptr::null(), title.as_ptr()) };
        if hwnd == 0 { return Err("Warframe window not found".into()); }

        // Client rect is always (0,0,w,h) — convert origin to screen coords
        let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        unsafe { GetClientRect(hwnd, &mut r) };
        let mut origin = POINT { x: 0, y: 0 };
        unsafe { ClientToScreen(hwnd, &mut origin) };

        Ok([origin.x, origin.y, r.right - r.left, r.bottom - r.top])
    }
}

#[tauri::command]
fn stop_monitor(state: State<AppState>) {
    state.monitor_active.store(false, Ordering::SeqCst);
}

#[tauri::command]
fn get_monitor_status(state: State<AppState>) -> bool {
    state.monitor_active.load(Ordering::SeqCst)
}

/// Returns blueprint_path → display_name map (names only, for compatibility).
#[tauri::command]
fn get_blueprint_names(state: State<AppState>) -> HashMap<String, String> {
    state.blueprint_to_result.lock().unwrap_or_else(|e| e.into_inner())
        .iter()
        .map(|(k, (name, _))| (k.clone(), name.clone()))
        .collect()
}

// ─── App entry point ──────────────────────────────────────────────────────────

/// WFCD has a recurring bug where dual-pistol component weapons get the parent's
/// name prepended. These overrides replace the bad names with the correct ones.
fn patch_item_name(unique_name: &str, name: &str) -> String {
    match unique_name {
        "/Lotus/Weapons/Tenno/Pistols/Magnum/Magnum"                    => "Magnus".into(),
        "/Lotus/Weapons/Tenno/Pistols/PrimeMagnus/PrimeMagnusWeapon"    => "Magnus Prime".into(),
        "/Lotus/Weapons/Tenno/Pistol/BroncoPrime"                       => "Bronco Prime".into(),
        "/Lotus/Weapons/Tenno/Pistols/PrimeLex/PrimeLex"                => "Lex Prime".into(),
        "/Lotus/Weapons/Tenno/Pistols/PrimeVasto/PrimeVastoPistol"      => "Vasto Prime".into(),
        "/Lotus/Weapons/Tenno/Melee/Swords/KatanaAndWakizashi/Katana"   => "Dragon Nikana".into(),
        "/Lotus/Types/Recipes/Weapons/WeaponParts/WarBlade"             => "Broken War Blade".into(),
        "/Lotus/Types/Recipes/Weapons/WeaponParts/WarHilt"              => "Broken War Hilt".into(),
        "/Lotus/Types/Recipes/Weapons/WeaponParts/ArchHeavyPistolsBarrel"    => "Dual Decurion Barrel".into(),
        "/Lotus/Types/Recipes/Weapons/WeaponParts/ArchHeavyPistolsReceiver"  => "Dual Decurion Receiver".into(),
        _ => name.to_string(),
    }
}

fn patch_item_category(name: &str, category: &str) -> String {
    if name.contains("Blueprint") { "Blueprints".to_string() } else { category.to_string() }
}

fn load_items_cache(path: &PathBuf) -> Option<Vec<WfcdItem>> {
    let s = std::fs::read_to_string(path).ok()?;
    let arr: Vec<serde_json::Value> = serde_json::from_str(&s).ok()?;
    let items: Vec<WfcdItem> = arr.into_iter().filter_map(|v| {
        let unique_name = v["unique_name"].as_str()?.to_string();
        let raw_name = v["name"].as_str()?.to_string();
        let name = patch_item_name(&unique_name, &raw_name);
        let image_name = v["image_name"].as_str().map(|s| s.to_string());
        let vaulted = v["vaulted"].as_bool();
        let ducats = v["ducats"].as_u64().map(|n| n as u32);
        let raw_cat = v["category"].as_str()?.to_string();
        let category = patch_item_category(&name, &raw_cat);
        let mastery_req = v["mastery_req"].as_u64().map(|n| n as u32);
        Some(WfcdItem { unique_name, name, category, image_name, vaulted, ducats, mastery_req })
    }).collect();
    if items.is_empty() { None } else { Some(items) }
}

fn load_quantities_cache(path: &PathBuf) -> HashMap<String, i64> {
    std::fs::read_to_string(path).ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn load_recipes_cache(path: &PathBuf) -> HashMap<String, Vec<RecipeComponent>> {
    std::fs::read_to_string(path).ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_window_state(window: &tauri::WebviewWindow, settings_path: &std::path::Path, prefix: &str) {
    // Skip saving if minimized — minimized windows have junk coordinates on Windows
    if window.is_minimized().unwrap_or(false) { return; }

    let maximized = window.is_maximized().unwrap_or(false);
    let pos  = window.outer_position().ok();
    let size = window.outer_size().ok();

    let mut map: serde_json::Map<String, serde_json::Value> = std::fs::read_to_string(settings_path)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| if let serde_json::Value::Object(m) = v { Some(m) } else { None })
        .unwrap_or_default();

    map.insert(format!("{}Maximized", prefix), maximized.into());
    // Only overwrite position/size when not maximised — maximised coords are the full screen
    if !maximized {
        if let Some(p) = pos {
            map.insert(format!("{}X", prefix), p.x.into());
            map.insert(format!("{}Y", prefix), p.y.into());
        }
        if let Some(s) = size {
            map.insert(format!("{}Width",  prefix), (s.width  as i64).into());
            map.insert(format!("{}Height", prefix), (s.height as i64).into());
        }
    }

    let _ = std::fs::write(settings_path, serde_json::Value::Object(map).to_string());
}

fn restore_window_state(window: &tauri::WebviewWindow, settings_path: &std::path::Path, prefix: &str, min_w: u32, min_h: u32) {
    let json = match std::fs::read_to_string(settings_path) {
        Ok(s) => s,
        Err(_) => return,
    };
    let map = match serde_json::from_str::<serde_json::Value>(&json) {
        Ok(serde_json::Value::Object(m)) => m,
        _ => return,
    };

    let maximized = map.get(&format!("{}Maximized", prefix)).and_then(|v| v.as_bool()).unwrap_or(false);
    if maximized {
        let _ = window.maximize();
        return;
    }

    let x = map.get(&format!("{}X", prefix)).and_then(|v| v.as_i64());
    let y = map.get(&format!("{}Y", prefix)).and_then(|v| v.as_i64());
    let w = map.get(&format!("{}Width",  prefix)).and_then(|v| v.as_i64()).map(|v| v as u32);
    let h = map.get(&format!("{}Height", prefix)).and_then(|v| v.as_i64()).map(|v| v as u32);

    if let (Some(x), Some(y)) = (x, y) {
        // Guard against Windows' minimized-window sentinel coordinates (-32000, -32000)
        if x > -10_000 && y > -10_000 {
            let _ = window.set_position(tauri::PhysicalPosition::new(x as i32, y as i32));
        }
    }
    if let (Some(w), Some(h)) = (w, h) {
        if w >= min_w && h >= min_h {
            let _ = window.set_size(tauri::PhysicalSize::new(w, h));
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let data_dir = dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("warframe-companion");

    std::fs::create_dir_all(&data_dir).expect("Failed to create data directory");

    let db_path = data_dir.join("data.db");
    let items_cache_path = data_dir.join("items_cache.json");
    let recipes_cache_path = data_dir.join("recipes_cache.json");
    let relic_drops_cache_path = data_dir.join("relic_drops_cache.json");
    let relic_rewards_cache_path = data_dir.join("relic_rewards_cache.json");
    let quantities_cache_path = data_dir.join("quantities_cache.json");
    let settings_path = data_dir.join("settings.json");
    let log_path = data_dir.join("scan_log.txt");

    let conn = db::init_db(&db_path).expect("Failed to initialize database");

    let initial_items = load_items_cache(&items_cache_path)
        .unwrap_or_else(wfcd::fallback_items);
    let initial_recipes = load_recipes_cache(&recipes_cache_path);
    let initial_relic_drops: HashMap<String, Vec<String>> = std::fs::read_to_string(&relic_drops_cache_path)
        .ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
    let initial_relic_rewards: HashMap<String, Vec<wfcd::RelicReward>> = std::fs::read_to_string(&relic_rewards_cache_path)
        .ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
    let initial_quantities = load_quantities_cache(&quantities_cache_path);

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(AppState {
            db_path,
            items_cache_path,
            recipes_cache_path,
            relic_drops_cache_path,
            relic_rewards_cache_path,
            quantities_cache_path,
            settings_path,
            log_path,
            conn: Mutex::new(conn),
            wfcd_items: Mutex::new(initial_items),
            recipes: Mutex::new(initial_recipes),
            relic_drops: Mutex::new(initial_relic_drops),
            relic_rewards: Mutex::new(initial_relic_rewards),
            blueprint_to_result: Mutex::new(HashMap::new()),
            wiki_reward_names: Mutex::new(std::collections::HashSet::new()),
            current_quantities: Arc::new(Mutex::new(initial_quantities)),
            current_crafting: Arc::new(Mutex::new(vec![])),
            monitor_active: Arc::new(AtomicBool::new(false)),
            wfm_price_cache: Mutex::new(HashMap::new()),
        })
        .setup(|app| {
            use tauri::Manager;
            if let Some(window) = app.get_webview_window("main") {
                let icon = tauri::image::Image::from_bytes(
                    include_bytes!("../icons/icon.png")
                ).map_err(|e| e.to_string())?;
                window.set_icon(icon).map_err(|e| e.to_string())?;

                // Restore saved window geometry before the window is shown
                let state = app.state::<AppState>();
                restore_window_state(&window, &state.settings_path, "window", 400, 300);
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_all_items,
            get_current_quantities,
            get_item_list_status,
            fetch_item_list,
            get_change_log,
            clear_cache,
            load_settings,
            save_settings,
            read_scan_log,
            get_app_version,
            set_app_version,
            get_craftable_items,
            get_recipe,
            get_relic_drops,
            get_relic_rewards,
            fetch_wfm_items,
            fetch_wfm_price,
            get_item_price,
            scan_warframe_credentials,
            scan_warframe_api_urls,
            warframe_login,
            fetch_warframe_inventory,
            fetch_worldstate,
            get_warframe_window_rect,
            get_overlay_session_log,
            start_monitor,
            stop_monitor,
            get_monitor_status,
            get_blueprint_names,
            get_current_crafting,
        ])
        .on_window_event(|window, event| {
            let label = window.label().to_string();
            if label == "main" || label == "modular-popout" {
                let prefix = if label == "main" { "window" } else { "modularWin" };
                match event {
                    tauri::WindowEvent::CloseRequested { .. } => {
                        // Save window geometry before the window is destroyed
                        let app = window.app_handle();
                        if let Some(wv) = app.get_webview_window(&label) {
                            let state = app.state::<AppState>();
                            save_window_state(&wv, &state.settings_path, prefix);
                        }
                    }
                    tauri::WindowEvent::Destroyed => {
                        // Kill the process only when the main window is destroyed
                        // (prevents orphaned overlay/modular windows)
                        if label == "main" {
                            std::process::exit(0);
                        }
                    }
                    _ => {}
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
