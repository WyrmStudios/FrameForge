use std::collections::HashMap;
use std::io::Write;
use std::sync::atomic::Ordering;
use tauri::State;

use crate::app_state::AppState;
use crate::inventory_state::{load_inventory_state_cache, persist_inventory_state_cache, CachedItem};
use crate::{memory_scanner, truncate_chars};

// ─── Warframe companion API ───────────────────────────────────────────────────

/// Scan all Warframe memory regions for the session credentials (accountId + nonce).
/// These are placed in memory by the game itself after login — we never handle passwords.
#[tauri::command]
pub(crate) async fn scan_warframe_credentials() -> Result<(String, String, String), String> {
    tauri::async_runtime::spawn_blocking(scan_warframe_credentials_sync)
        .await
        .map_err(|e| e.to_string())?
}

fn scan_warframe_credentials_sync() -> Result<(String, String, String), String> {
    use crate::platform::{Platform, ProcessAccess};

    let pid = memory_scanner::find_warframe_pid_pub()
        .ok_or("Warframe is not running")?;

    let handle = Platform::open_process(pid)
        .map_err(|e| format!("Cannot open Warframe process: {}", e))?;

    let mut address: usize = 0x10000;

    loop {
        let regions: Vec<_> = handle.regions_from(address).collect();
        if regions.is_empty() { break; }

        for region in &regions {
            address = region.base_address + region.region_size;
            if !region.is_committed || !region.is_readable || region.is_executable { continue; }
            if region.region_size > 128 * 1024 * 1024 { continue; }

            let buffer = match handle.read(region.base_address, region.region_size) {
                Some(r) => r,
                None => continue,
            };
            if buffer.is_empty() { continue; }

            if let Some((id, nonce)) = memory_scanner::scan_auth_credentials(&buffer) {
                let steam_id = memory_scanner::scan_steam_id(&buffer).unwrap_or_default();
                return Ok((id, nonce, steam_id));
            }
        }
    }
    Err("Credentials not found in memory. Make sure you are in the orbiter (not loading screen) and Warframe has been running for a few minutes.".into())

}

/// Scan Warframe memory for API request URLs — reveals exact endpoints the game uses.
#[tauri::command]
pub(crate) async fn scan_warframe_api_urls() -> Result<Vec<String>, String> {
    tauri::async_runtime::spawn_blocking(|| {
        use crate::platform::{Platform, ProcessAccess};

        let pid = memory_scanner::find_warframe_pid_pub()
            .ok_or("Warframe is not running".to_string())?;

        let handle = Platform::open_process(pid)
            .map_err(|e| format!("Cannot open process: {}", e))?;

        let mut found = Vec::new();
        let mut address: usize = 0x10000;

        loop {
            let regions: Vec<_> = handle.regions_from(address).collect();
            if regions.is_empty() { break; }

            for region in &regions {
                address = region.base_address + region.region_size;
                if !region.is_committed || !region.is_readable || region.is_executable { continue; }
                if region.region_size > 64 * 1024 * 1024 { continue; }

                let buffer = match handle.read(region.base_address, region.region_size) {
                    Some(r) => r,
                    None => continue,
                };
                if buffer.is_empty() { continue; }

                let data = &buffer;
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
                                .map(|&b| if (0x20..0x7f).contains(&b) { b as char } else { ' ' })
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
            if found.len() >= 20 { break; }
        }
        Ok(found)
    }).await.map_err(|e| e.to_string())?
}

/// Persist mastery data (unique_name → rank 0-30) from the Companion API or any other source.
/// Merges into each item's entry in inventory_state_cache.json; higher rank always wins.
#[tauri::command]
pub(crate) fn save_mastery_data(
    state: tauri::State<'_, AppState>,
    data: HashMap<String, u32>,
) -> Result<(), String> {
    if data.is_empty() { return Ok(()); }
    persist_inventory_state_cache(&state.inventory_state_cache_path, |cache| {
        for (k, v) in &data {
            let entry = cache.items.entry(k.clone()).or_insert_with(|| CachedItem {
                unique_name: k.clone(), ..Default::default()
            });
            if *v > entry.mastery_rank { entry.mastery_rank = *v; }
        }
    })
}

/// Return statement for get_saved_inventory — camelCase so TypeScript receives it without conversion.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SavedInventory {
    api_quantities: HashMap<String, i64>,
    api_mod_copies: Vec<ApiModCopy>,
    consumed_suits: Vec<String>,
}

/// Companion API mod copy entry — camelCase so it round-trips through TypeScript without conversion.
#[derive(serde::Serialize, serde::Deserialize, Default, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ApiModCopy {
    pub(crate) unique_name: String,
    pub(crate) rank: Option<u32>,
    pub(crate) count: i64,
}

/// Returns all owned riven mods (veiled and revealed) from the persisted inventory cache.
/// Runs in a blocking thread so the large inventory JSON deserialization doesn't stall the UI.
#[tauri::command]
pub(crate) async fn get_rivens(state: tauri::State<'_, AppState>) -> Result<Vec<memory_scanner::BlobRivenEntry>, String> {
    let path = state.inventory_state_cache_path.clone();
    tauri::async_runtime::spawn_blocking(move || {
        load_inventory_state_cache(&path).rivens
    })
    .await
    .map_err(|e| e.to_string())
}

/// Called once on startup so the frontend can restore state without waiting for Warframe to run.
#[tauri::command]
pub(crate) fn get_saved_inventory(state: tauri::State<'_, AppState>) -> SavedInventory {
    let cache = load_inventory_state_cache(&state.inventory_state_cache_path);
    SavedInventory {
        api_quantities: state.api_quantities_cache.lock().unwrap_or_else(|e| e.into_inner()).clone(),
        api_mod_copies: state.api_mod_copies_cache.lock().unwrap_or_else(|e| e.into_inner()).clone(),
        consumed_suits: cache.consumed_suits(),
    }
}

/// Persist Companion API quantities, mod copies, and subsumed warframes.
/// Updates AppState in-memory (scanner picks them up on next write) and writes immediately to disk.
#[tauri::command]
pub(crate) fn save_api_inventory(
    state: tauri::State<'_, AppState>,
    api_quantities: HashMap<String, i64>,
    api_mod_copies: Vec<ApiModCopy>,
    consumed_suits: Vec<String>,
) -> Result<(), String> {
    // Update in-memory cache so the scan loop picks these up without a file read.
    *state.api_quantities_cache.lock().unwrap_or_else(|e| e.into_inner()) = api_quantities.clone();
    *state.api_mod_copies_cache.lock().unwrap_or_else(|e| e.into_inner()) = api_mod_copies.clone();

    persist_inventory_state_cache(&state.inventory_state_cache_path, |cache| {
        // API quantities: only write items the scanner hasn't reported at all.
        // Scanner data is authoritative — API only fills gaps for items not yet scanned.
        // Checked against the live scanner map, not `entry.amount == 0`: the blob
        // scanner rebuilds this whole cache from scratch every ~10s (build_inventory_from_blob),
        // so a stale amount==0 on disk can't be told apart from "scanner genuinely
        // says zero" — that ambiguity let a stale API number briefly clobber a real
        // zero. current_quantities/current_mods answer "did the scanner ever see
        // this key" directly.
        {
            let scanned = state.current_quantities.lock().unwrap_or_else(|e| e.into_inner());
            for (k, qty) in &api_quantities {
                if scanned.contains_key(k) { continue; }
                let entry = cache.items.entry(k.clone()).or_insert_with(|| CachedItem {
                    unique_name: k.clone(), ..Default::default()
                });
                entry.amount = *qty;
            }
        }
        // API mod copies: same — only fill mods the scanner hasn't recorded.
        {
            let scanned_mods = state.current_mods.lock().unwrap_or_else(|e| e.into_inner());
            for mc in &api_mod_copies {
                if scanned_mods.contains_key(&mc.unique_name) { continue; }
                let entry = cache.items.entry(mc.unique_name.clone()).or_insert_with(|| CachedItem {
                    unique_name: mc.unique_name.clone(), ..Default::default()
                });
                if entry.mod_ranks.is_none() {
                    let ranks = entry.mod_ranks.get_or_insert_with(HashMap::new);
                    let rank_key = mc.rank.map(|r| r.to_string()).unwrap_or_else(|| "0".to_string());
                    *ranks.entry(rank_key).or_insert(0) = mc.count;
                    entry.amount = ranks.values().sum();
                }
            }
        }
        for suit in &consumed_suits {
            cache.items.entry(suit.clone()).or_insert_with(|| CachedItem {
                unique_name: suit.clone(), ..Default::default()
            }).subsumed = true;
        }
    })
}

/// Persist riven fingerprint detail (buffs/curses/compat/challenge/polarity/rerolls)
/// built from the Companion API response. Same gap-fill philosophy as
/// `save_api_inventory`: the scanner's blob parse is authoritative and rewrites
/// `rivens` wholesale every ~10s, so this only matters while the scanner hasn't
/// captured a blob this session (e.g. API-only users, or before the player has
/// opened the orbiter). Checked against `current_mods`, not cache content, for
/// the same reason documented on `save_api_inventory`. The retain-then-push
/// pattern re-derives the whole non-scanner-confirmed set from this call's
/// payload each time, so repeated polls don't accumulate duplicates.
#[tauri::command]
pub(crate) fn save_api_rivens(
    state: tauri::State<'_, AppState>,
    rivens: Vec<memory_scanner::BlobRivenEntry>,
) -> Result<(), String> {
    persist_inventory_state_cache(&state.inventory_state_cache_path, |cache| {
        let scanned_mods = state.current_mods.lock().unwrap_or_else(|e| e.into_inner());
        cache.rivens.retain(|r| scanned_mods.contains_key(&r.item_type));
        for riven in rivens {
            if scanned_mods.contains_key(&riven.item_type) { continue; }
            cache.rivens.push(riven);
        }
    })
}

/// Login to Warframe API with email + password (same flow as mobile companion app).
/// Password is hashed with Whirlpool before sending — never sent in plaintext.
/// Returns (accountId, nonce) for subsequent API calls.
#[tauri::command]
pub(crate) async fn warframe_login(email: String, password: String) -> Result<(String, String), String> {
    use whirlpool::{Whirlpool, Digest};
    let hash = format!("{:x}", Whirlpool::digest(password.as_bytes()));
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();

    // Try multiple endpoint + body format variants.
    // mobile=true prevents clobbering an active game session.
    // date=9999999999999999 is required by some versions of the API (device-ID placeholder).
    let form_body = format!(
        "email={}&password={}&time={}&mobile=true&appVersion=live&date=9999999999999999",
        urlencoding(&email), hash, now
    );
    let json_body = format!(
        r#"{{"email":"{}","password":"{}","time":{},"date":9999999999999999,"mobile":true,"appVersion":"live"}}"#,
        email.replace('"', "\\\""), hash, now
    );

    let candidates: &[(&str, &str, &str)] = &[
        ("https://api.warframe.com/api/login.php",     "application/json",                  &json_body),
        ("https://mobile.warframe.com/api/login.php",  "application/json",                  &json_body),
        ("https://api.warframe.com/api/login.php",     "application/x-www-form-urlencoded", &form_body),
        ("https://mobile.warframe.com/api/login.php",  "application/x-www-form-urlencoded", &form_body),
    ];

    let mut errors: Vec<String> = Vec::new();
    for (url, ct, body) in candidates {
        let result = ureq::post(url)
            .set("X-Titanium-Id", "9bbd1ddd-f7f2-402d-9777-873f458cb50c")
            .set("X-Requested-With", "XMLHttpRequest")
            .set("Content-Type", ct)
            .set("User-Agent", "Dalvik/2.1.0 (Linux; U; Android 8.1.0)")
            .send_string(body);
        match result {
            Ok(resp) => {
                let text = resp.into_string().unwrap_or_default();
                let json: serde_json::Value = match serde_json::from_str(&text) {
                    Ok(v) => v,
                    Err(_) => { errors.push(format!("{}: non-JSON: {}", url, truncate_chars(&text, 200))); continue; }
                };
                let id    = json["id"].as_str().unwrap_or("").to_string();
                let nonce = json["Nonce"].to_string().trim_matches('"').to_string();
                if !id.is_empty() && nonce != "null" {
                    return Ok((id, nonce));
                }
                errors.push(format!("{}: rejected: {}", url, truncate_chars(&text, 300)));
            }
            Err(ureq::Error::Status(code, resp)) => {
                let body = resp.into_string().unwrap_or_default();
                errors.push(format!("{}: HTTP {}: {}", url, code, truncate_chars(&body, 200)));
            }
            Err(e) => { errors.push(format!("{}: {}", url, e)); }
        }
    }
    Err(format!("All login endpoints failed:\n{}", errors.join("\n")))
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
pub(crate) async fn fetch_warframe_inventory(account_id: String, nonce: String, steam_id: String, state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    let log_enabled = state.api_log_enabled.load(Ordering::SeqCst);
    let log_dir     = state.api_log_dir.clone();

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
                if log_enabled {
                    let endpoint_name = url.split('/').next_back().unwrap_or("response");
                    let ts   = chrono::Utc::now().format("%Y-%m-%dT%H-%M-%S").to_string();
                    let path = log_dir.join(format!("{}_{}.json", ts, endpoint_name));
                    let _ = std::fs::write(&path, &text);
                }
                if status == 200 {
                    return serde_json::from_str(&text)
                        .map_err(|e| format!("Parse failed: {} — body: {}", e, truncate_chars(&text, 200)));
                }
                last_err = format!("HTTP {} from {}: {}", status, url, truncate_chars(&text, 100));
            }
            Err(e) => { last_err = format!("Request to {} failed: {}", url, e); }
        }
    }
    Err(last_err)
}

#[derive(serde::Deserialize)]
pub(crate) struct ApiChange {
    pub item_name: String,
    pub old_qty: i64,
    pub new_qty: i64,
}

#[tauri::command]
pub(crate) fn log_api_changes(state: State<AppState>, changes: Vec<ApiChange>) -> Result<(), String> {
    let mut f = std::fs::OpenOptions::new()
        .create(true).append(true)
        .open(&state.changes_log_path)
        .map_err(|e| e.to_string())?;
    let ts = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
    for c in &changes {
        let _ = writeln!(f, "[{}] Companion API  | {} | {} → {}", ts, c.item_name, c.old_qty, c.new_qty);
    }
    Ok(())
}
