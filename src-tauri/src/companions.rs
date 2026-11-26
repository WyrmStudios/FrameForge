//! Kubrow/Kavat companion resolution for the "Companions" tab.
//!
//! `Details` schema, confirmed from a real account dump (true Kubrow sample —
//! Kavat-only fields are still unverified, see below):
//! ```text
//! Details: {
//!   Name: String, IsPuppy: bool, HasCollar: bool, PrintsRemaining: i64,
//!   Status: String, HatchDate: { $date: { $numberLong } },
//!   IsMale: bool, Size: f64,                      // 0.0–1.0 scale, not a path
//!   DominantTraits: {
//!     BaseColor, SecondaryColor, TertiaryColor, AccentColor, EyeColor: <path>,
//!     FurPattern: <path>, Personality: <path> (== top-level ItemType),
//!     BodyType: <path>,
//!   },
//!   RecessiveTraits: { ...same shape as DominantTraits, unused here },
//! }
//! ```
//! Everything above is verified. `HeadType`/`TailType` under `DominantTraits`
//! for Kavats have no confirmed sample yet — `candidate_name` tries a couple of
//! plausible key names so a wrong guess only needs a key added here, not a
//! parser change. If `blobLogEnabled` is on, the raw `Details` object is dumped
//! to disk so a real Kavat sample can be confirmed later.

use std::collections::HashMap;
use std::sync::OnceLock;

use serde::Serialize;
use serde_json::Value;
use tracing::warn;

const LOCALIZATION_JSON: &str = include_str!("../resources/kubrow_kavat_localization.json");

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct ColorSwatch {
    pub name: String,
    pub hex: String,
}

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct ResolvedCompanion {
    pub item_type:  String,
    /// "kubrow" | "kavat"
    pub species:    String,
    pub breed_name: String,
    pub pet_name:   Option<String>,
    pub body_type:  Option<String>,
    /// `Details.Size`, raw 0.0–1.0 value from the game data, unmodified (e.g. "0.72").
    pub size:       Option<String>,
    /// Base, Secondary, Tertiary, Accent — in that fixed order.
    pub colors:     Vec<ColorSwatch>,
    pub pattern:    Option<String>,
    pub eye_color:  Option<ColorSwatch>,
    /// Kavat only (unverified field name).
    pub head_type:  Option<String>,
    /// Kavat only (unverified field name).
    pub tail_type:  Option<String>,
    pub gender:     Option<String>,
    pub is_puppy:   bool,
    pub prints_remaining: Option<i64>,
}

struct LocalizationTables {
    /// Path → {name, hex}. Merges kubrow.colors + kubrow.eyes + kavat.colors + kavat.eyes —
    /// full Lotus paths are already globally unique so species tables can share one map.
    colors: HashMap<String, ColorSwatch>,
    /// Path → display name. Merges kubrow.patterns + kubrow.bodyTypes + kavat.patterns +
    /// kavat.heads + kavat.tails.
    names: HashMap<String, String>,
}

fn parse_color_table(obj: &Value, into: &mut HashMap<String, ColorSwatch>) {
    let Some(map) = obj.as_object() else { return };
    for (path, v) in map {
        let Some(name) = v["name"].as_str() else { continue };
        let hex = v["hex"].as_str().unwrap_or("#808080");
        into.insert(path.clone(), ColorSwatch { name: name.to_string(), hex: hex.to_string() });
    }
}

fn parse_name_table(obj: &Value, into: &mut HashMap<String, String>) {
    let Some(map) = obj.as_object() else { return };
    for (path, v) in map {
        if let Some(name) = v.as_str() {
            into.insert(path.clone(), name.to_string());
        }
    }
}

fn load_localization_tables() -> LocalizationTables {
    let mut colors: HashMap<String, ColorSwatch> = HashMap::new();
    let mut names: HashMap<String, String> = HashMap::new();

    let parsed: Value = match serde_json::from_str(LOCALIZATION_JSON) {
        Ok(v) => v,
        Err(e) => {
            warn!(error = %e, "failed to parse bundled kubrow_kavat_localization.json");
            return LocalizationTables { colors, names };
        }
    };

    parse_color_table(&parsed["kubrow"]["colors"], &mut colors);
    parse_color_table(&parsed["kubrow"]["eyes"], &mut colors);
    parse_color_table(&parsed["kavat"]["colors"], &mut colors);
    parse_color_table(&parsed["kavat"]["eyes"], &mut colors);

    parse_name_table(&parsed["kubrow"]["patterns"], &mut names);
    parse_name_table(&parsed["kubrow"]["bodyTypes"], &mut names);
    parse_name_table(&parsed["kavat"]["patterns"], &mut names);
    parse_name_table(&parsed["kavat"]["heads"], &mut names);
    parse_name_table(&parsed["kavat"]["tails"], &mut names);

    LocalizationTables { colors, names }
}

fn tables() -> &'static LocalizationTables {
    static TABLES: OnceLock<LocalizationTables> = OnceLock::new();
    TABLES.get_or_init(load_localization_tables)
}

/// Unresolved path → readable fallback, e.g. `.../KubrowPetRegularBodyType`
/// → "Kubrow Pet Regular Body Type". Mirrors what unmapped tokens look like
/// in-game data dumps; only used for a trait path the bundled table doesn't cover.
fn humanize_path_tail(path: &str) -> String {
    let tail = path.rsplit('/').next().unwrap_or(path);
    let mut out = String::new();
    for (i, ch) in tail.chars().enumerate() {
        if i > 0 && ch.is_uppercase() {
            out.push(' ');
        }
        out.push(ch);
    }
    out
}

fn resolve_color(path: &str) -> ColorSwatch {
    tables().colors.get(path).cloned().unwrap_or_else(|| ColorSwatch {
        name: humanize_path_tail(path),
        hex: "#808080".to_string(),
    })
}

fn resolve_name(path: &str) -> String {
    tables().names.get(path).cloned().unwrap_or_else(|| humanize_path_tail(path))
}

/// First string value found in `details` under any of `keys`, resolved through
/// the localization table (path → display name).
fn candidate_name(details: &Value, keys: &[&str]) -> Option<String> {
    for &k in keys {
        if let Some(s) = details[k].as_str() {
            return Some(resolve_name(s));
        }
    }
    None
}

fn candidate_color(details: &Value, keys: &[&str]) -> Option<ColorSwatch> {
    for &k in keys {
        if let Some(s) = details[k].as_str() {
            return Some(resolve_color(s));
        }
    }
    None
}

fn candidate_gender(details: &Value) -> Option<String> {
    details["IsMale"].as_bool().map(|b| if b { "Male" } else { "Female" }.to_string())
}

/// Resolve one raw blob companion entry into display-ready data for the frontend.
/// `path_to_name` is the existing WFCD-catalogue-derived map (same one used for
/// every other item in the app) — breed names are looked up there, not guessed.
pub fn resolve_companion(
    entry: &crate::memory_scanner::BlobCompanionEntry,
    path_to_name: &HashMap<String, String>,
) -> Option<ResolvedCompanion> {
    let species = if entry.item_type.starts_with("/Lotus/Types/Game/KubrowPet/") {
        "kubrow"
    } else if entry.item_type.starts_with("/Lotus/Types/Game/CatbrowPet/") {
        "kavat"
    } else {
        return None;
    };

    let breed_name = path_to_name.get(&entry.item_type).cloned()
        .unwrap_or_else(|| humanize_path_tail(&entry.item_type));

    let d = &entry.details;
    let dom = &d["DominantTraits"];

    let body_type = candidate_name(dom, &["BodyType"]);
    let size = d["Size"].as_f64().map(|f| format!("{f}"));
    let colors = ["BaseColor", "SecondaryColor", "TertiaryColor", "AccentColor"]
        .iter()
        .filter_map(|&k| dom[k].as_str())
        .map(resolve_color)
        .collect();
    let pattern = candidate_name(dom, &["FurPattern", "Pattern"]);
    let eye_color = candidate_color(dom, &["EyeColor"]);
    let head_type = if species == "kavat" {
        candidate_name(dom, &["HeadType", "DominantHeadType", "Head"])
    } else {
        None
    };
    let tail_type = if species == "kavat" {
        candidate_name(dom, &["TailType", "DominantTailType", "Tail"])
    } else {
        None
    };
    let gender = candidate_gender(d);
    let is_puppy = d["IsPuppy"].as_bool().unwrap_or(false);
    let prints_remaining = d["PrintsRemaining"].as_i64();

    Some(ResolvedCompanion {
        item_type: entry.item_type.clone(),
        species: species.to_string(),
        breed_name,
        pet_name: entry.pet_name.clone(),
        body_type,
        size,
        colors,
        pattern,
        eye_color,
        head_type,
        tail_type,
        gender,
        is_puppy,
        prints_remaining,
    })
}

/// Resolve companions from the Warframe Companion API's `KubrowPets` array
/// (same shape as the FULL_ACCOUNT blob's `KubrowPets` section — the API
/// returns the player's live account document). Lets the Companions tab
/// populate even when the game isn't running, as long as the Companion API
/// is connected — unlike the memory scanner, which requires the orbiter/Arsenal
/// to be open.
#[tauri::command]
pub(crate) fn resolve_companions_from_api(
    state: tauri::State<'_, crate::app_state::AppState>,
    kubrow_pets: Vec<Value>,
) -> Result<Vec<ResolvedCompanion>, String> {
    let path_to_name = {
        let items = state.wfcd_items.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let relic_drops = state.relic_drops.lock().unwrap_or_else(|e| e.into_inner()).clone();
        crate::monitor::build_monitor_catalog(&items, &state.corrections, &relic_drops).path_to_name
    };

    let entries: Vec<crate::memory_scanner::BlobCompanionEntry> = kubrow_pets.iter()
        .filter_map(|e| {
            let item_type = e["ItemType"].as_str()?.to_string();
            let pet_name = e["Details"]["Name"].as_str().map(String::from);
            Some(crate::memory_scanner::BlobCompanionEntry { item_type, pet_name, details: e["Details"].clone() })
        })
        .collect();

    Ok(entries.iter().filter_map(|e| resolve_companion(e, &path_to_name)).collect())
}

/// Dump every companion's raw `Details` object for one blob to a debug file,
/// gated by `companionDebugLogEnabled` (same pattern as `blobLogEnabled`). This
/// is the mechanism for confirming/correcting the guessed trait keys above
/// against a real play session.
pub fn write_companion_debug_log(
    companions: &[crate::memory_scanner::BlobCompanionEntry],
    dir: &std::path::Path,
    ts: &str,
) {
    if companions.is_empty() { return; }
    let dump: Vec<Value> = companions.iter().map(|c| serde_json::json!({
        "item_type": c.item_type,
        "pet_name": c.pet_name,
        "details": c.details,
    })).collect();
    if let Ok(json) = serde_json::to_string_pretty(&dump) {
        let path = dir.join(format!("companions-{ts}.json"));
        let _ = std::fs::write(path, json);
    }
}
