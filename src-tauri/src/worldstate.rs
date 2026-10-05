use std::sync::Arc;
use tauri::{Manager, State};

use crate::app_state::AppState;

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

pub(crate) fn resolve_node(id: &str) -> String {
    if let Some(n) = get_sol_nodes().get(id) { return n.display.clone(); }
    if let Some(stripped) = id.strip_suffix("HUB") { return format!("{} Relay", stripped); }
    if let Some(stripped) = id.strip_prefix("CrewBattleNode") { return format!("Railjack {}", stripped); }
    id.to_string()
}

pub(crate) fn node_enemy(id: &str) -> String {
    get_sol_nodes().get(id).map(|n| n.enemy.clone()).unwrap_or_default()
}

pub(crate) fn node_mission_type(id: &str) -> String {
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
        "MT_CORRUPTION"       => "Void Flood",
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
    let last = path.split('/').next_back().unwrap_or(path);
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
pub(crate) fn store_to_unique(path: &str) -> String {
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
            match tag {
                "CetusSyndicate" => {
                    // Worldstate now provides one entry per full 150-min cycle (day+night).
                    // Only use the currently-active entry; skip the pre-loaded next cycle.
                    // Day = first 6000 s of the cycle, Night = remaining ~3000 s.
                    if activation_ms <= now_ms && now_ms < expiry_ms {
                        let time_into_cycle = now_ms - activation_ms;
                        let is_day = time_into_cycle < 6_000_000;
                        let phase_expiry = if is_day { activation_ms + 6_000_000 } else { expiry_ms };
                        cetus = json!({ "expiry": ms_to_iso(phase_expiry), "isDay": is_day });
                    }
                }
                "SolarisSyndicate" => {
                    // Orb Vallis cycle: 1608 s total (26 m 48 s), Warm = 360 s (6 min),
                    // Cold = 1248 s (20 m 48 s). Phase is global — not relative to entry.
                    // Epoch: warm starts when (now_ms - 96_000) % 1_608_000 == 0.
                    if activation_ms <= now_ms && now_ms < expiry_ms {
                        const CYCLE: i64 = 1_608_000;
                        const WARM:  i64 =   360_000;
                        const EPOCH: i64 =    96_000;
                        let adj          = now_ms - EPOCH;
                        let base_adj     = (adj / CYCLE) * CYCLE;
                        let phase        = adj - base_adj;
                        let is_warm      = phase < WARM;
                        let phase_expiry = if is_warm {
                            base_adj + WARM + EPOCH
                        } else {
                            base_adj + CYCLE + EPOCH
                        };
                        vallis = json!({ "expiry": ms_to_iso(phase_expiry), "isWarm": is_warm });
                    }
                }
                "EntratiSyndicate"
                    // Cambion Drift — one entry per 150-min cycle; show countdown to cycle end.
                    // (Frontend shows generic "Active" state, no Fass/Vome distinction needed.)
                    if activation_ms <= now_ms && now_ms < expiry_ms => {
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
            let boss = boss_raw.split('/').next_back().unwrap_or(boss_raw)
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
                let price = item["PrimePrice"].as_i64().unwrap_or(0);
                // Regal Aya = bundle packs under MegaPrimeVault/; Aya = direct item paths
                let is_regal = raw_path.contains("/MegaPrimeVault/");
                let mut obj = serde_json::Map::new();
                obj.insert("name".into(), json!(name));
                obj.insert("uniqueName".into(), json!(store_to_unique(raw_path)));
                if is_regal {
                    obj.insert("regalAyaPrice".into(), json!(price));
                } else {
                    obj.insert("ayaPrice".into(), json!(price));
                }
                serde_json::Value::Object(obj)
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
        let tag           = m["Tag"].as_str().unwrap_or("");
        let expiry_ms     = ws_ms(&m["Expiry"]);
        let activation_ms = ws_ms(&m["Activation"]);
        let job_count     = m["Jobs"].as_array().map(|j| j.len()).unwrap_or(0);
        let label = match tag {
            "CetusSyndicate"     => "cetus",
            "SolarisSyndicate"   => "vallis",
            "EntratiSyndicate"   => "cambion",
            "ZarimanSyndicate"   => "zariman",
            "HexSyndicate"       => "hex",
            "EntratiLabSyndicate"=> "entrati-lab",
            _                    => continue,
        };
        // DE pre-loads the next cycle entry. Prefer the currently-active entry; only
        // insert a future entry if nothing is set yet (fallback near a boundary).
        let is_active = activation_ms <= now_ms && now_ms < expiry_ms;
        if bounties.contains_key(label) && !is_active {
            continue;
        }
        bounties.insert(label.to_string(), json!({
            "expiry": ms_to_iso(expiry_ms),
            "jobCount": job_count,
        }));
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
    // KahlSyndicate in SyndicateMissions is the daily bounty rotation, not the
    // weekly mission. Kahl's weekly missions reset on the same Monday schedule
    // as the Archon Hunt (LiteSorties), so we borrow that expiry.
    let kahl = raw["LiteSorties"].as_array()
        .and_then(|a| a.first())
        .map(|s| {
            let expiry_ms = ws_ms(&s["Expiry"]);
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

    // ── Active Goals / Events ──────────────────────────────────────────────
    let events: Vec<Value> = raw["Goals"].as_array()
        .map(|a| a.iter()
            .filter(|g| ws_ms(&g["Expiry"]) > now_ms)
            .filter_map(|g| {
                let expiry_ms = ws_ms(&g["Expiry"]);
                let desc = g["Desc"].as_str().unwrap_or("");
                let label = loc(desc);
                if label.is_empty() { return None; }
                Some(json!({ "expiry": ms_to_iso(expiry_ms), "label": label }))
            })
            .collect()
        )
        .unwrap_or_default();

    json!({
        "cetus": cetus, "vallis": vallis, "cambion": cambion, "zariman": zariman,
        "bounties": bounties,
        "sortie": sortie, "archonHunt": archon_hunt,
        "voidTrader": void_trader, "primeResurgence": prime_resurgence, "nightwave": nightwave,
        "circuit": circuit, "kahl": kahl, "deepArchimedea": deep_archimedea,
        "events": events,
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
pub(crate) async fn fetch_worldstate(state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    // Snapshot catalog for name lookups — do this before entering spawn_blocking
    let catalog: std::collections::HashMap<String, String> = {
        let items = state.wfcd_items.lock().unwrap_or_else(|e| e.into_inner());
        items.iter().map(|i| (i.unique_name.clone(), i.name.clone())).collect()
    };
    // Slightly under the 60s frontend poll, so a window's own next tick always
    // refetches while a second window polling on a different offset is served
    // from here. Two racing callers can both miss and fetch once each; the
    // window is small enough that a lock held across the network call is the
    // worse trade.
    const WORLDSTATE_TTL: std::time::Duration = std::time::Duration::from_secs(55);
    let started_at = std::time::Instant::now();
    let cached = {
        let cache = state.worldstate_cache.lock().unwrap_or_else(|e| e.into_inner());
        cache.as_ref()
            .filter(|(fetched_at, _, _)| fetched_at.elapsed() < WORLDSTATE_TTL)
            .map(|(_, raw, news)| (Arc::clone(raw), Arc::clone(news)))
    };

    type CachedPayload = (Arc<serde_json::Value>, Arc<serde_json::Value>);
    let (result, fetched) = tokio::task::spawn_blocking(move || -> Result<(serde_json::Value, Option<CachedPayload>), String> {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        if let Some((raw, news)) = cached {
            let mut result = parse_worldstate_value(&raw, now_ms, &catalog);
            if let Some(obj) = result.as_object_mut() {
                obj.insert("news".to_string(), news.as_ref().clone());
            }
            return Ok((result, None));
        }
        let raw = ureq::get("https://api.warframe.com/cdn/worldState.php")
            .set("User-Agent", "FrameForge/3.2.0")
            .timeout(std::time::Duration::from_secs(20))
            .call()
            .map_err(|e| format!("worldstate fetch failed: {}", e))?
            .into_json::<serde_json::Value>()
            .map_err(|e| format!("worldstate parse failed: {}", e))?;
        let mut result = parse_worldstate_value(&raw, now_ms, &catalog);

        // Fetch news/promotions from Steam — official Warframe community announcements only.
        // warframestat.us/pc/news was removed from that API entirely.
        let news: Vec<serde_json::Value> = ureq::get(
            "https://api.steampowered.com/ISteamNews/GetNewsForApp/v2/?appid=230410&count=10&maxlength=500&format=json"
        )
            .set("User-Agent", "FrameForge/3.2.0")
            .timeout(std::time::Duration::from_secs(10))
            .call()
            .ok()
            .and_then(|r| r.into_json::<serde_json::Value>().ok())
            .and_then(|v| v["appnews"]["newsitems"].as_array().cloned())
            .unwrap_or_default()
            .into_iter()
            .filter(|item| item["feed_type"].as_i64().unwrap_or(0) == 1)
            .map(|item| {
                let title = item["title"].as_str().unwrap_or("").to_string();
                let lower = title.to_lowercase();
                let ts_ms = item["date"].as_i64().unwrap_or(0) * 1000;
                serde_json::json!({
                    "message":     title,
                    "link":        item["url"].as_str().unwrap_or(""),
                    "date":        ts_ms,
                    "stream":      false,
                    "primeAccess": lower.contains("prime access") || lower.contains("prime "),
                    "update":      lower.contains("update") || lower.contains("patch notes"),
                })
            })
            .collect();
        let news_value = serde_json::json!(news);
        if let Some(obj) = result.as_object_mut() {
            obj.insert("news".to_string(), news_value.clone());
        }
        Ok((result, Some((Arc::new(raw), Arc::new(news_value)))))
    })
    .await
    .map_err(|e| format!("task error: {}", e))??;

    if let Some((raw, news)) = fetched {
        let mut cache = state.worldstate_cache.lock().unwrap_or_else(|e| e.into_inner());
        // Two callers that both missed can finish out of order, so a response
        // that started before what is already stored must not replace it.
        let stored_is_newer = cache.as_ref().is_some_and(|(fetched_at, _, _)| *fetched_at > started_at);
        if !stored_is_newer {
            *cache = Some((std::time::Instant::now(), raw, news));
        }
    }
    Ok(result)
}

pub fn refresh_worldstate(app: &tauri::AppHandle, _force: bool) -> Result<(), String> {
    let state = app.state::<AppState>();
    // Invalidate the in-memory worldstate cache so the next fetch goes upstream.
    *state.worldstate_cache.lock().unwrap_or_else(|e| e.into_inner()) = None;
    Ok(())
}
