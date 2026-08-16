use std::collections::HashMap;
use tracing::{debug, error, info, warn};
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// Write `data` to `path` atomically: write to a `.tmp` sibling, then rename over the target.
/// Prevents zero-byte corruption if the process or OS crashes mid-write.
fn atomic_write(path: &std::path::Path, data: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, data)?;
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}
fn truncate_chars(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}
use tauri::{Emitter, Manager, State};

mod console_login; // [console-login feature] remove this line to drop the feature
mod db;
mod logging;
mod memory_scanner;
mod ocr;
mod resolver;
mod wfcd;
mod wfm;

use db::{QuantityChange, SnapshotPoint, Trade, TrackedItem};
use resolver::ItemResolver;
use wfcd::{RecipeComponent, SyndicateOffer, WfcdItem};
use wfm::{to_wfm_slug, Wfm, WfmItem, WfmPrice, WfmRivenAttribute, WfmTopItem};

/// Bundled corrections file embedded at compile time. Never absent at runtime.
const BUNDLED_CORRECTIONS: &str = include_str!("../resources/corrections.json");

/// Load and merge corrections: bundled entries first, then user file overrides on a per-path basis.
fn load_corrections(user_path: &std::path::Path) -> HashMap<String, CorrectionEntry> {
    let mut map: HashMap<String, CorrectionEntry> = serde_json::from_str::<Vec<CorrectionEntry>>(BUNDLED_CORRECTIONS)
        .unwrap_or_default()
        .into_iter()
        .map(|e| (e.path.clone(), e))
        .collect();
    if let Ok(content) = std::fs::read_to_string(user_path) {
        if let Ok(entries) = serde_json::from_str::<Vec<CorrectionEntry>>(&content) {
            for e in entries { map.insert(e.path.clone(), e); }
        }
    }
    map
}

/// One entry in corrections.json — a hand-curated override for a specific Lotus path.
/// Fields are all optional so a minimal entry can omit unused columns.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct CorrectionEntry {
    pub path:          String,
    /// Display name override. Required unless category is "Ignored".
    pub name:          Option<String>,
    /// Display category override, or "Ignored" to suppress the path everywhere.
    pub category:      Option<String>,
    /// Explicit WFM tradeability flag. `false` means skip all WFM price lookups.
    /// When absent the app auto-detects from ducat_price / category.
    pub tradeable_wfm: Option<bool>,
    /// True when this item is stackable (quantity shown rather than binary owned).
    pub is_stackable:  Option<bool>,
}

pub struct AppState {
    pub db_path: PathBuf,
    pub items_cache_path: PathBuf,
    pub recipes_cache_path: PathBuf,
    pub relic_drops_cache_path: PathBuf,
    pub relic_rewards_cache_path: PathBuf,
    pub quantities_cache_path: PathBuf,
    pub inventory_state_cache_path: PathBuf,
    pub settings_path: PathBuf,
    pub log_path: PathBuf,
    pub changes_log_path: PathBuf,
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
    /// weapon unique_name → riven disposition (omegaAttenuation). Populated from All.json.
    pub weapon_dispositions: Mutex<HashMap<String, f32>>,
    /// Last-known quantities from memory scans. Shared with monitor thread.
    pub current_quantities: Arc<Mutex<HashMap<String, i64>>>,
    /// Stable unique items (weapons/warframes) seen in 2+ consecutive scans.
    /// Exposed so get_current_quantities can return them for overlay ownership checks.
    pub unique_quantities: Arc<Mutex<HashMap<String, i64>>>,
    /// Mod/arcane inventory: unique_name → {total, by_rank}. Shared with monitor thread.
    /// API data is merged in when available; falls back to scanner-only totals.
    pub current_mods: Arc<Mutex<HashMap<String, memory_scanner::ModCount>>>,
    /// Last-known crafting jobs from memory scans. Shared with monitor thread.
    pub current_crafting: Arc<Mutex<Vec<CraftingJob>>>,
    pub monitor_active: Arc<AtomicBool>,
    /// Controls the raw memory string-dump background thread.
    pub raw_scan_active: Arc<AtomicBool>,
    pub raw_scan_path: PathBuf,
    /// When true, save a timestamped inventory blob to blobs/ on each full scan pass.
    pub blob_log_enabled: Arc<AtomicBool>,
    pub blob_log_dir: PathBuf,
    /// When true, save the raw DE API response to api_logs/ on each fetch.
    pub api_log_enabled: Arc<AtomicBool>,
    pub api_log_dir: PathBuf,
    /// The warframe.market client: session, rate limiters, and the slug → price
    /// cache all live behind this one seam, shared (Arc) with the prefetch thread.
    pub wfm: Arc<Wfm>,
    /// Slugs waiting for a price fetch (normal priority). Drained by the WFM queue thread.
    pub wfm_price_queue: Arc<Mutex<std::collections::VecDeque<String>>>,
    /// High-priority slugs (popup / on-demand). Drained before wfm_price_queue.
    pub wfm_priority_queue: Arc<Mutex<std::collections::VecDeque<String>>>,
    /// Set to true once the WFM queue drain thread has been started.
    pub wfm_queue_started: Arc<AtomicBool>,
    /// Path to the persisted top-WFM-items cache (survives restarts).
    pub wfm_top_cache_path: PathBuf,
    /// syndicate name → purchasable items (all known syndicates)
    pub syndicate_catalog: Mutex<HashMap<String, Vec<SyndicateOffer>>>,
    pub syndicate_catalog_path: PathBuf,
    /// IDs of riven auctions created via FrameForge — persisted so hidden auctions survive restarts.
    pub auction_ids: Mutex<Vec<String>>,
    pub auction_ids_path: PathBuf,
    /// Companion API quantities held in memory so the scanner includes them in cache writes.
    pub api_quantities_cache: Arc<Mutex<HashMap<String, i64>>>,
    /// Companion API mod copies held in memory so the scanner includes them in cache writes.
    pub api_mod_copies_cache: Arc<Mutex<Vec<ApiModCopy>>>,
    /// Most recent OCR frame (top ~48% of Warframe window, BGRA, width, height).
    /// Stored by the OCR loop so auto-capture can write it without a second GPU readback.
    pub last_ocr_frame: Arc<Mutex<Option<(Vec<u8>, u32, u32)>>>,
    /// Local image cache directory — craftable item images downloaded here on first run.
    pub img_cache_dir: PathBuf,
    /// Port of the local HTTP image server (set in setup hook, 0 until started).
    pub img_server_port: Mutex<u16>,
    /// Local Warframe account name extracted from EE.log "Logged in NAME".
    /// Used to filter the player's own name from OCR captures and to display in the UI.
    pub local_player_name: Arc<Mutex<Option<String>>>,
    /// Last successfully locked relic reward payload { items, positions }.
    /// Written when the OCR loop emits relic-rewards; cleared on dismiss or when read.
    /// Overlay.tsx pulls this on mount so it never misses rewards that arrived before
    /// its relic-rewards listener was registered.
    pub pending_relic_rewards: Mutex<Option<serde_json::Value>>,
    /// relics.run daily bulk price cache: item display name (lowercase) → median sell price.
    pub relics_run_prices: Mutex<HashMap<String, u32>>,
    pub relics_run_prices_cache_path: PathBuf,
    /// Raw worldstate + Steam news from the last upstream fetch, with the time it
    /// was taken. Every window polls worldstate on its own timer, so without this
    /// two open windows mean two fetch pairs a minute against DE and Steam.
    /// Only the network payload is cached — parsing still runs per call, so
    /// activation/expiry filtering stays anchored to the current time. Held
    /// behind `Arc` so serving a hit shares the ~1MB tree instead of cloning it.
    pub worldstate_cache: Mutex<Option<(std::time::Instant, Arc<serde_json::Value>, Arc<serde_json::Value>)>>,
    /// When true, unmatched inventory paths are written to the Unmatched Paths debug folder.
    pub debug_cat_enabled: Arc<AtomicBool>,
    /// Subfolders under %LOCALAPPDATA%\warframe-companion\Debugging\
    pub auto_capture_dir: PathBuf,
    pub manual_capture_dir: PathBuf,
    pub memory_probe_path: PathBuf,
    pub unmatched_paths_dir: PathBuf,
    /// Merged bundled + user corrections: path → entry.
    /// Bundled file is embedded at compile time; user file from data dir overrides on a per-path basis.
    pub corrections: HashMap<String, CorrectionEntry>,
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
    pub max_level_cap: Option<u32>,
    /// Explicit tradeability flag from corrections.json. `Some(false)` = not on WFM.
    /// `None` = auto-detected from ducat_price / category (the normal case for WFCD items).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tradeable_wfm: Option<bool>,
}


/// Item captured when debug categorization is enabled and an inventory path either
/// has no WFCD catalog entry or falls through to the "Misc" catch-all.
#[derive(serde::Serialize, Clone)]
pub struct DebugUnmatched {
    pub path:             String,
    pub name:             String,
    pub item_type:        String,
    pub product_category: String,
    pub wfcd_category:    String,
    pub final_category:   String,
    /// "no_wfcd_match" = path not in catalog; "misc_fallback" = in catalog but landed in Misc
    pub reason:           String,
    // ── Blob fields present alongside this path ──────────────────────────────
    /// ItemCount from blob (stackable items only)
    pub item_count:   Option<i64>,
    /// Section from blob (unique items: "Suits", "LongGuns", "Melee", etc.)
    pub section:      Option<String>,
    /// Number of polarised slots from blob (unique items only)
    pub polarized:    Option<u32>,
    /// Total copies from blob (mods only)
    pub mod_total:    Option<i64>,
    /// Last 4 non-trivial path segments — helpful when WFCD has no entry for this path
    pub path_hint:    Vec<String>,
}

/// Split a PascalCase path segment into space-separated words.
/// e.g. "GarudaSystemsBlueprint" → "Garuda Systems Blueprint"
///      "ChromaBeaconCComponent"  → "Chroma Beacon C Component"
fn camel_to_words(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len() + 8);
    for (i, &c) in chars.iter().enumerate() {
        let prev_lower      = i > 0 && chars[i - 1].is_lowercase();
        let prev_up_next_lo = i > 0 && chars[i - 1].is_uppercase()
            && i + 1 < chars.len() && chars[i + 1].is_lowercase();
        if c.is_uppercase() && i > 0 && (prev_lower || prev_up_next_lo) {
            out.push(' ');
        }
        out.push(c);
    }
    out
}

/// Determine the correct display category for an item, using all three WFCD fields
/// in priority order: type → productCategory → category (display) → name/path heuristics.
fn fix_category(name: &str, item_type: &str, product_category: &str, wfcd_cat: &str, path: &str) -> String {
    // ── Tier 0: explicit exclusions ────────────────────────────────────────────
    // Exalted Weapons are frame abilities, not player inventory items.
    if matches!(item_type, "Exalted Weapon" | "Node") {
        return "Excluded".to_string();
    }
    // Nightwave/Season challenge definitions leak from the account blob.
    // They have item_type="Rifle"/"Pistol" etc. which would wrongly place them
    // in weapon categories. Exclude them entirely — they are not inventory items.
    if path.contains("/Types/Challenges/") {
        return "Excluded".to_string();
    }

    // ── Tier 1: Mods and Arcanes ───────────────────────────────────────────────
    // Checked BEFORE the Blueprint name rule — some mods/arcanes have "Blueprint"
    // in their display name (e.g. "Balefire Surge Blueprint") and must not flip.
    if wfcd_cat == "Mods"    { return "Mods".to_string(); }
    if wfcd_cat == "Arcanes" { return "Arcanes".to_string(); }

    // ── Tier 2: Blueprint name rule ────────────────────────────────────────────
    if name.contains("Blueprint") { return "Blueprints".to_string(); }

    // ── Tier 3: type field — most reliable, covers all 17 000 items ───────────
    match item_type {
        "Warframe" => return "Warframes".to_string(),

        // Companion weapons MUST come before Primary/Secondary checks — WFCD stores
        // Sentinel weapons (Akaten, Sweeper, Verglas, etc.) with category=Primary.
        "Companion Weapon" => return "Companions".to_string(),

        "Rifle" | "Shotgun" | "Bow" | "Sniper" | "Launcher" | "Throwing" => {
            // Railjack turrets/crew weapons share weapon types with Primary weapons.
            if product_category == "CrewShipWeapons" { return "Railjack".to_string(); }
            return "Primary".to_string();
        }

        "Pistol" | "Dual Pistols" => {
            // Guard against the noisy productCategory=Pistols bucket — Sirocco
            // (the Operator amp) has type=Pistol but productCategory=OperatorAmps.
            if product_category == "OperatorAmps" {
                return "Operator Weapons".to_string();
            }
            if product_category != "SentinelWeapons" {
                return "Secondary".to_string();
            }
        }

        "Melee"     => return "Melee".to_string(),
        "Sentinel"  => return "Companions".to_string(),
        "Pets"      => return "Companions".to_string(),
        "Archwing"  => return "Archwing".to_string(),
        "Arch-Gun"  => return "Archwing".to_string(),
        "Arch-Melee" => return "Archwing".to_string(),
        "Railjack Turret" => return "Railjack".to_string(),
        "Relic"     => return "Relics".to_string(),

        // Modular weapon and companion components → Parts
        "Zaw Component" | "Kitgun Component" | "K-Drive Component"
        | "Amp" | "Pet Resource" | "Pet Parts"
            => return "Parts".to_string(),

        // Forma, Catalysts, Reactors, Arcane Adapters — consumable equipment items.
        // WFCD groups these under their slot category (Primary/Secondary/etc.) but
        // they are not weapons — treat them as Resources.
        "Equipment Adapter" => return "Resources".to_string(),

        // Stackable resources
        "Resource" | "Fish" | "Fish Part" | "Gem" | "Cut Gem" | "Plant" | "Alloy"
        | "Medallion" | "Ayatan Sculpture" | "Ayatan Star" | "Eidolon Shard"
        | "Gear" | "Key" | "Conservation Tag" | "Conservation Prey" | "Boosters"
        | "Focus Way" | "Focus Lens" | "Currency" | "Fish Bait" | "Specter" | "Extractor"
            => return "Resources".to_string(),

        // Cosmetics — Sigils and Glyphs get their own tabs
        "Sigil" => return "Sigils".to_string(),
        "Glyph" => return "Glyphs".to_string(),

        "Skin" | "Emotes" | "Color Palette" | "Fur Color"
        | "Fur Pattern" | "Themes" | "Theme Background" | "Theme Sound"
        | "Ship Decoration" | "Syandana" | "Pet Collar" | "Captura" | "Simulacrum"
        | "Orbiter" | "Skins"
            => {
                if path.contains("/RailJack/") { return "Railjack".to_string(); }
                return "Skins".to_string();
            }

        _ => {} // fall through to productCategory
    }

    // ── Tier 4: productCategory — very clean for the 1 137 items that have it ─
    match product_category {
        "Suits" | "MechSuits"   => return "Warframes".to_string(),
        "LongGuns"              => return "Primary".to_string(),
        "Melee"                 => return "Melee".to_string(),
        "SentinelWeapons"       => return "Companions".to_string(),
        "OperatorAmps"          => return "Operator Weapons".to_string(),
        "SpaceSuits"            => return "Archwing".to_string(),
        "SpaceGuns"             => return "Archwing".to_string(),
        "SpaceMelee"            => return "Archwing".to_string(),
        "Sentinels" | "KubrowPets" => return "Companions".to_string(),
        "CrewShipWeapons"       => return "Railjack".to_string(),
        _ => {}
    }

    // ── Tier 5: wfcd_cat display-category fallback ─────────────────────────────
    match wfcd_cat {
        "Companions"  => return "Companions".to_string(),
        "Archwing"    => return "Archwing".to_string(),
        "Railjack"    => return "Railjack".to_string(),
        "Resources"   => return "Resources".to_string(),
        // wfcd.rs explicitly sets category="Parts" for built recipe components
        // (warframe parts, weapon components). Trust that assignment here so they
        // never fall to the Miscellaneous catch-all.
        "Parts"       => return "Parts".to_string(),
        "Primary"     => return "Primary".to_string(),
        "Secondary"   => return "Secondary".to_string(),
        "Warframes"   => return "Warframes".to_string(),
        "Relics" => {
            // Guard against non-relic items WFCD mis-groups under Relics (segments, etc.)
            let n = name.to_lowercase();
            if n.ends_with("intact") || n.ends_with("exceptional")
                || n.ends_with("flawless") || n.ends_with("radiant")
            { return "Relics".to_string(); }
        }
        "Sigils" => return "Sigils".to_string(),
        "Glyphs" => return "Glyphs".to_string(),
        _ => {}
    }

    // ── Tier 6: path guards for sub-components type/productCategory doesn't cover
    if path.contains("/MoaPetEngine") || path.contains("/MoaPetPayload") || path.contains("/MoaPetLeg")
        || path.contains("/ZanukaPetPartBody") || path.contains("/ZanukaPetPartLegs")
        || path.contains("/ZanukaPetPartTail") || path.contains("/CreaturePetParts/")
    {
        return "Parts".to_string();
    }

    // ── Tier 7: name-suffix fallback for direct-drop components ───────────────
    // Warframe-frame components (Chassis, Neuroptics, Systems) always carry
    // "Blueprint" in their name, caught above. These suffixes cover weapon parts
    // and companion components that drop pre-built.
    const PART_SUFFIXES: &[&str] = &[
        " receiver", " stock", " barrel", " blade", " handle", " guard",
        " hilt", " link", " gauntlet", " carapace", " cerebrum", " systems",
        " upper limb", " lower limb", " strike", " boot", " head", " grip",
        // Bow/thrown weapon components
        " string", " disc", " stars",
        // Modular companion (MOA) components — gyrome/loader/bracket are never the companion itself
        " gyrome", " loader", " bracket",
    ];
    let lower = name.to_lowercase();
    if PART_SUFFIXES.iter().any(|s| lower.ends_with(s)) {
        return "Parts".to_string();
    }

    // WFCD mis-tags some direct-drop components as "Blueprints" (no "Blueprint" in name).
    if wfcd_cat == "Blueprints" {
        return "Parts".to_string();
    }

    // ── Tier 8: path-prefix rules ──────────────────────────────────────────────
    // Bandaid categorization for paths WFCD doesn't cover. Items caught here still
    // land in the Unmatched Paths debug file (reason: "path_rule") so they can get
    // explicit name corrections in corrections.json in the future.
    if path.contains("/CosmeticEnhancers/Antiques/") { return "Arcanes".to_string(); }
    if path.contains("/SentinelPrecepts/")            { return "Mods".to_string(); }
    if path.contains("/MeleeTrees/")                  { return "Mods".to_string(); }

    // ── Catch-all ──────────────────────────────────────────────────────────────
    "Miscellaneous".to_string()
}

#[tauri::command]
fn get_all_items(state: State<AppState>) -> Vec<CatalogItem> {
    // Clone data and release locks immediately — the catalog build below is O(n²)
    // and holding the locks blocks the monitor thread and other commands.
    let items: Vec<wfcd::WfcdItem> = state.wfcd_items.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let bp_names: HashMap<String, (String, Option<u32>)> = state.blueprint_to_result.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let corrections = &state.corrections;
    let items = &items;
    let bp_names = &bp_names;

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

    // Vaulted lookup helper: exact name → base without " Blueprint" → "X Prime" set entry.
    // WFCD's vaulted flag is most reliably set on the assembled warframe/weapon ("Mag Prime",
    // "Venato Prime") rather than on every individual component.  Falling back to the set entry
    // means components never lose the lock icon just because WFCD left their own field null.
    let prime_vaulted = |name: &str| -> Option<bool> {
        let n = name.to_lowercase();
        let base = n.strip_suffix(" blueprint").unwrap_or(&n).to_string();
        let prime_key = n.find("prime").map(|pos| n[..pos + 5].to_string());
        wfcd_vaulted.get(&n).and_then(|v| *v)
            .or_else(|| wfcd_vaulted.get(&base).and_then(|v| *v))
            .or_else(|| prime_key.as_deref().and_then(|pk| wfcd_vaulted.get(pk).and_then(|v| *v)))
    };

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
            let vaulted = prime_vaulted(bp_name);
            result.push(CatalogItem {
                unique_name:   bp_unique.clone(),
                name:          bp_name.clone(),
                category:      "Blueprints".to_string(),
                image_name:    None,
                vaulted,
                ducats:        *bp_ducats,
                mastery_req:   None,
                max_level_cap: None,
                tradeable_wfm: None,
            });
        }
    }

    // Phase 2: WFCD items — keep WFCD categories, only fix blueprint names.
    // Skip blueprints already covered by ExportRecipes or already added
    // (WFCD may store the same blueprint at multiple paths).
    for i in items.iter().filter(|i| !i.unique_name.contains("PvPVariant")) {
        let cat = fix_category(&i.name, &i.item_type, &i.product_category, &i.category, &i.unique_name);
        if cat == "Excluded" { continue; }
        let n = i.name.to_lowercase();
        if cat == "Blueprints" {
            if !bp_names_added.insert(n) { continue; } // skip if already seen
        }
        // Inherit vaulted from the prime set entry when WFCD left the component field null.
        let vaulted = i.vaulted.or_else(|| {
            if i.name.to_lowercase().contains("prime") { prime_vaulted(&i.name) } else { None }
        });
        result.push(CatalogItem {
            unique_name:   i.unique_name.clone(),
            name:          i.name.clone(),
            category:      cat,
            image_name:    i.image_name.clone(),
            vaulted,
            ducats:        i.ducats,
            mastery_req:   i.mastery_req,
            max_level_cap: i.max_level_cap,
            tradeable_wfm: None,
        });
    }

    // Phase 3: WFCD-only blueprints NOT covered by ExportRecipes.
    for item in items.iter() {
        if !item.unique_name.starts_with("/Lotus/Types/Recipes/") { continue; }
        let n = item.name.to_lowercase();
        if !bp_names_added.insert(n) { continue; }
        let vaulted = item.vaulted.or_else(|| {
            if item.name.to_lowercase().contains("prime") { prime_vaulted(&item.name) } else { None }
        });
        result.push(CatalogItem {
            unique_name:   item.unique_name.clone(),
            name:          item.name.clone(),
            category:      "Blueprints".to_string(),
            image_name:    item.image_name.clone(),
            vaulted,
            ducats:        item.ducats,
            mastery_req:   item.mastery_req,
            max_level_cap: None,
            tradeable_wfm: None,
        });
    }

    // ── Corrections: remove Ignored items ────────────────────────────────────
    result.retain(|i| {
        corrections.get(&i.unique_name)
            .map(|c| c.category.as_deref() != Some("Ignored"))
            .unwrap_or(true)
    });

    // ── Corrections: override name/category/tradeable_wfm ─────────────────────
    for item in result.iter_mut() {
        if let Some(c) = corrections.get(&item.unique_name) {
            if let Some(ref name) = c.name { item.name = name.clone(); }
            if let Some(ref cat) = c.category {
                if cat != "Ignored" { item.category = cat.clone(); }
            }
            if c.tradeable_wfm.is_some() { item.tradeable_wfm = c.tradeable_wfm; }
        }
    }

    // ── Phase 2.5: correction-only items (not in WFCD, have a name) ───────────
    {
        let covered: std::collections::HashSet<String> =
            result.iter().map(|i| i.unique_name.clone()).collect();
        for (path, c) in corrections.iter() {
            if covered.contains(path) { continue; }
            if c.category.as_deref() == Some("Ignored") { continue; }
            let name = match c.name.as_deref() {
                Some(n) if !n.is_empty() => n.to_string(),
                _ => continue,
            };
            let category = c.category.clone().unwrap_or_else(|| "Miscellaneous".to_string());
            result.push(CatalogItem {
                unique_name:   path.clone(),
                name,
                category,
                image_name:    None,
                vaulted:       None,
                ducats:        None,
                mastery_req:   None,
                max_level_cap: None,
                tradeable_wfm: c.tradeable_wfm,
            });
        }
    }

    // Virtual currency entries (tracked via memory scan, not in WFCD).
    for (path, name, img) in [
        ("/_currency/Endo",         "Endo",            "/endo.webp"),
        ("/_currency/Credits",      "Credits",         "/credits.webp"),
        ("/_currency/Platinum",     "Platinum",        "/platinum.webp"),
        ("/_currency/PlatinumGift", "Platinum (Gift)", "/platinum-gift.webp"),
    ] {
        result.push(CatalogItem {
            unique_name:   path.to_string(),
            name:          name.to_string(),
            category:      "Miscellaneous".to_string(),
            image_name:    Some(img.to_string()),
            vaulted:       None,
            ducats:        None,
            mastery_req:   None,
            max_level_cap: None,
            tradeable_wfm: None,
        });
    }

    // Phase 4: Path-inferred items — blob paths not covered by any catalog source.
    // Rule: last path segment ends with "Blueprint" → category Blueprints, name from camelCase parse.
    // These items are still tracked in the Unmatched Paths debug file (reason: "path_inferred").
    {
        let covered: std::collections::HashSet<String> = result.iter()
            .map(|i| i.unique_name.clone()).collect();
        let quantities = state.current_quantities.lock().unwrap_or_else(|e| e.into_inner());
        for (path, _) in quantities.iter() {
            if covered.contains(path) { continue; }
            let last = path.rsplit('/').next().unwrap_or(path.as_str());
            if last.ends_with("Blueprint") && path.contains("/Recipes/") {
                result.push(CatalogItem {
                    unique_name:   path.clone(),
                    name:          camel_to_words(last),
                    category:      "Blueprints".to_string(),
                    image_name:    None,
                    vaulted:       None,
                    ducats:        None,
                    mastery_req:   None,
                    max_level_cap: None,
                    tradeable_wfm: None,
                });
            }
        }
    }

    // Final safety dedup by unique_name
    let mut seen_unique: std::collections::HashSet<String> = std::collections::HashSet::new();
    result.retain(|i| seen_unique.insert(i.unique_name.clone()));

    result
}

#[tauri::command]
fn get_current_quantities(state: State<AppState>) -> HashMap<String, i64> {
    let mut q = state.current_quantities.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let uq = state.unique_quantities.lock().unwrap_or_else(|e| e.into_inner());
    for (name, &qty) in uq.iter() {
        q.entry(name.clone()).or_insert(qty);
    }
    let mods = state.current_mods.lock().unwrap_or_else(|e| e.into_inner());
    for (path, mc) in mods.iter() {
        q.entry(path.clone()).or_insert(mc.total);
    }
    q
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
        "item_type": i.item_type, "product_category": i.product_category,
        "image_name": i.image_name, "vaulted": i.vaulted, "ducats": i.ducats,
        "mastery_req": i.mastery_req, "omega_attenuation": i.omega_attenuation,
        "fusion_limit": i.fusion_limit, "max_level_cap": i.max_level_cap
    })).collect::<Vec<_>>()) {
        let _ = std::fs::write(&state.items_cache_path, json);
    }

    // Persist recipes cache
    if let Ok(json) = serde_json::to_string(&result.recipes) {
        let _ = std::fs::write(&state.recipes_cache_path, json);
    }

    let patched_items: Vec<WfcdItem> = result.items.into_iter().map(|mut i| {
        i.name = patch_item_name(&i.unique_name, &i.name);
        i.category = patch_item_category(&i.name, &i.category, &i.unique_name);
        i
    }).collect();
    if let Ok(json) = serde_json::to_string(&result.relic_drops) {
        let _ = std::fs::write(&state.relic_drops_cache_path, json);
    }
    if let Ok(json) = serde_json::to_string(&result.relic_rewards) {
        let _ = std::fs::write(&state.relic_rewards_cache_path, json);
    }
    let deduped = dedup_known_aliases(patched_items);

    // Write mod_max_rank into inventory_state_cache.json for every mod/arcane so it is
    // available at startup without requiring wfcd_items to be loaded first.
    {
        let mut inv = load_inventory_state_cache(&state.inventory_state_cache_path);
        for item in deduped.iter().filter(|i| i.fusion_limit.is_some() || i.max_level_cap.is_some() || {
            let cat = fix_category(&i.name, &i.item_type, &i.product_category, &i.category, &i.unique_name);
            matches!(cat.as_str(), "Warframes" | "Primary" | "Secondary" | "Melee"
                                 | "Companions" | "Archwing" | "Operator Weapons")
        }) {
            let entry = inv.items.entry(item.unique_name.clone())
                .or_insert_with(|| CachedItem { unique_name: item.unique_name.clone(), ..Default::default() });
            if entry.name.is_empty() { entry.name = item.name.clone(); }
            if item.fusion_limit.is_some() { entry.mod_max_rank = item.fusion_limit; }
            // Effective level cap: use WFCD's explicit value when present (e.g. 40 for
            // Necramechs/Paracesis), otherwise fall back to the standard rank-30 cap for
            // all levelable categories. Non-levelable items get no entry.
            let effective_cap = item.max_level_cap.or_else(|| {
                let cat = fix_category(&item.name, &item.item_type, &item.product_category, &item.category, &item.unique_name);
                match cat.as_str() {
                    "Warframes" | "Primary" | "Secondary" | "Melee"
                    | "Companions" | "Archwing" | "Operator Weapons" => Some(30),
                    _ => None,
                }
            });
            if effective_cap.is_some() { entry.max_level_cap = effective_cap; }
        }
        if let Ok(json) = serde_json::to_string(&inv) {
            let _ = atomic_write(&state.inventory_state_cache_path, json.as_bytes());
        }
    }

    *state.wfcd_items.lock().map_err(|e| e.to_string())? = deduped;
    *state.recipes.lock().map_err(|e| e.to_string())? = result.recipes;
    *state.relic_drops.lock().map_err(|e| e.to_string())? = result.relic_drops;
    *state.relic_rewards.lock().map_err(|e| e.to_string())? = result.relic_rewards;
    *state.blueprint_to_result.lock().map_err(|e| e.to_string())? = result.blueprint_names;
    if !result.weapon_dispositions.is_empty() {
        *state.weapon_dispositions.lock().map_err(|e| e.to_string())? = result.weapon_dispositions;
    }
    if !result.wiki_reward_names.is_empty() {
        *state.wiki_reward_names.lock().map_err(|e| e.to_string())? = result.wiki_reward_names;
    }
    if !result.syndicate_catalog.is_empty() {
        if let Ok(json) = serde_json::to_string(&result.syndicate_catalog) {
            let _ = std::fs::write(&state.syndicate_catalog_path, json);
        }
        *state.syndicate_catalog.lock().map_err(|e| e.to_string())? = result.syndicate_catalog;
    }
    Ok(count)
}

// ─── Foundry / Recipes ────────────────────────────────────────────────────────

/// Returns all Primary / Secondary / Melee weapons from the catalog (for the
/// Weapons completionist tracker). Includes non-craftable weapons (Coda, etc.).
#[tauri::command]
fn get_weapon_catalog(state: State<AppState>) -> Vec<CatalogItem> {
    let items = state.wfcd_items.lock().unwrap_or_else(|e| e.into_inner());
    items.iter()
        .filter(|i| {
            (i.category == "Primary" || i.category == "Secondary" || i.category == "Melee")
                && !i.unique_name.contains("PvPVariant")
        })
        .map(|i| CatalogItem {
            unique_name:   i.unique_name.clone(),
            name:          i.name.clone(),
            category:      i.category.clone(),
            image_name:    i.image_name.clone(),
            vaulted:       i.vaulted,
            ducats:        i.ducats,
            mastery_req:   i.mastery_req,
            max_level_cap: i.max_level_cap,
            tradeable_wfm: None,
        })
        .collect()
}

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
        .filter(|i| recipe_keys.contains(&i.unique_name) && !i.unique_name.contains("PvPVariant"))
        .filter_map(|i| {
            let cat = fix_category(&i.name, &i.item_type, &i.product_category, &i.category, &i.unique_name);
            if cat == "Excluded" { return None; }
            Some(CatalogItem {
                unique_name:   i.unique_name.clone(),
                name:          i.name.clone(),
                category:      cat,
                image_name:    i.image_name.clone(),
                vaulted:       i.vaulted,
                ducats:        i.ducats,
                mastery_req:   i.mastery_req,
                max_level_cap: i.max_level_cap,
                tradeable_wfm: None,
            })
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

#[tauri::command]
fn get_recipes_bulk(state: State<AppState>, unique_names: Vec<String>) -> HashMap<String, Vec<RecipeComponent>> {
    let recipes = state.recipes.lock().unwrap_or_else(|e| e.into_inner());
    unique_names.into_iter()
        .map(|name| {
            let r = recipes.get(&name).cloned().unwrap_or_default();
            (name, r)
        })
        .collect()
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

/// Persist mastery data (unique_name → rank 0-30) from the Companion API or any other source.
/// Merges into each item's entry in inventory_state_cache.json; higher rank always wins.
#[tauri::command]
fn save_mastery_data(
    state: tauri::State<'_, AppState>,
    data: HashMap<String, u32>,
) -> Result<(), String> {
    if data.is_empty() { return Ok(()); }
    let path = state.inventory_state_cache_path.clone();
    let mut cache: InventoryStateCache = std::fs::read_to_string(&path).ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    for (k, v) in &data {
        let entry = cache.items.entry(k.clone()).or_insert_with(|| CachedItem {
            unique_name: k.clone(), ..Default::default()
        });
        if *v > entry.mastery_rank { entry.mastery_rank = *v; }
    }
    serde_json::to_string(&cache).map_err(|e| e.to_string())
        .and_then(|json| atomic_write(&path, json.as_bytes()).map_err(|e| e.to_string()))
}

/// Return statement for get_saved_inventory — camelCase so TypeScript receives it without conversion.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct SavedInventory {
    api_quantities: HashMap<String, i64>,
    api_mod_copies: Vec<ApiModCopy>,
    consumed_suits: Vec<String>,
}

/// Returns all owned riven mods (veiled and revealed) from the persisted inventory cache.
/// Runs in a blocking thread so the large inventory JSON deserialization doesn't stall the UI.
#[tauri::command]
async fn get_rivens(state: tauri::State<'_, AppState>) -> Result<Vec<memory_scanner::BlobRivenEntry>, String> {
    let path = state.inventory_state_cache_path.clone();
    tauri::async_runtime::spawn_blocking(move || {
        load_inventory_state_cache(&path).rivens
    })
    .await
    .map_err(|e| e.to_string())
}

/// Called once on startup so the frontend can restore state without waiting for Warframe to run.
#[tauri::command]
fn get_saved_inventory(state: tauri::State<'_, AppState>) -> SavedInventory {
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
fn save_api_inventory(
    state: tauri::State<'_, AppState>,
    api_quantities: HashMap<String, i64>,
    api_mod_copies: Vec<ApiModCopy>,
    consumed_suits: Vec<String>,
) -> Result<(), String> {
    // Update in-memory cache so the scan loop picks these up without a file read.
    *state.api_quantities_cache.lock().unwrap_or_else(|e| e.into_inner()) = api_quantities.clone();
    *state.api_mod_copies_cache.lock().unwrap_or_else(|e| e.into_inner()) = api_mod_copies.clone();

    let path = state.inventory_state_cache_path.clone();
    let mut cache: InventoryStateCache = std::fs::read_to_string(&path).ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    // API quantities: only write items not already present from the scanner.
    // Scanner data is authoritative — API only fills gaps for items not yet scanned.
    for (k, qty) in &api_quantities {
        let entry = cache.items.entry(k.clone()).or_insert_with(|| CachedItem {
            unique_name: k.clone(), ..Default::default()
        });
        if entry.amount == 0 { entry.amount = *qty; }
    }
    // API mod copies: same — only fill mods the scanner hasn't recorded.
    for mc in &api_mod_copies {
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
    for suit in consumed_suits {
        cache.items.entry(suit.clone()).or_insert_with(|| CachedItem {
            unique_name: suit.clone(), ..Default::default()
        }).subsumed = true;
    }
    serde_json::to_string(&cache).map_err(|e| e.to_string())
        .and_then(|json| atomic_write(&path, json.as_bytes()).map_err(|e| e.to_string()))
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
async fn fetch_warframe_inventory(account_id: String, nonce: String, steam_id: String, state: State<'_, AppState>) -> Result<serde_json::Value, String> {
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
                    let endpoint_name = url.split('/').last().unwrap_or("response");
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

// ─── Warframe.market ──────────────────────────────────────────────────────────

// ─── Warframe.market trading ──────────────────────────────────────────────────
// The WFM client lives in `wfm.rs`; the command handlers below are thin adapters
// over `state.wfm`. Session acquisition (this login webview) and keyring
// persistence stay here at the Tauri boundary.

/// Open warframe.market signin in an embedded WebView.
/// Emits `wfm-login-window-closed` if the window is closed before auth completes.
#[tauri::command]
fn wfm_open_login_window(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(existing) = app.get_webview_window("wfm-login") {
        let _ = existing.set_focus();
        return Ok(());
    }
    let win = open_wfm_webview(&app, "https://warframe.market/auth/signin")?;
    let app2 = app.clone();
    win.on_window_event(move |event| {
        if matches!(event, tauri::WindowEvent::Destroyed) {
            let _ = app2.emit("wfm-login-window-closed", ());
        }
    });
    Ok(())
}

/// Close the WFM login popup programmatically (e.g. after an auto-timeout).
#[tauri::command]
fn wfm_close_login_window(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(win) = app.get_webview_window("wfm-login") {
        let _ = win.close();
    }
    Ok(())
}

/// Opens a configured WebView at `start_url` with the shared injection script.
fn open_wfm_webview(app: &tauri::AppHandle, start_url: &str) -> Result<tauri::WebviewWindow, String> {
    static SCRIPT: &str = r#"
(function() {
  // ── Anti-detection ──────────────────────────────────────────────────────────
  // WebView2 signals that Steam/Xbox/Discord use to show blank pages:
  //   navigator.webdriver      = true   → automation flag
  //   window.chrome.webview    = object → WebView2-specific object
  //   navigator.userAgentData  exposes brand "Microsoft Edge WebView2"
  //   navigator.languages      often missing or wrong
  try { Object.defineProperty(navigator, 'webdriver', { get: function(){ return undefined; } }); } catch(e) {}
  try { if (window.chrome && window.chrome.webview) { delete window.chrome.webview; } } catch(e) {}
  try {
    Object.defineProperty(navigator, 'languages', { get: function(){ return ['en-US','en']; } });
  } catch(e) {}
  // Override userAgentData so brands list looks like real Chrome, not WebView2.
  try {
    var _uaBrands = [
      { brand: 'Google Chrome',  version: '125' },
      { brand: 'Chromium',       version: '125' },
      { brand: 'Not/A)Brand',    version: '24'  },
    ];
    var _uaData = {
      brands:   _uaBrands,
      mobile:   false,
      platform: 'Windows',
      getHighEntropyValues: function(hints) {
        return Promise.resolve({
          architecture:    'x86',
          bitness:         '64',
          brands:          _uaBrands,
          fullVersionList: [
            { brand: 'Google Chrome',  version: '125.0.6422.141' },
            { brand: 'Chromium',       version: '125.0.6422.141' },
            { brand: 'Not/A)Brand',    version: '24.0.0.0'       },
          ],
          mobile:          false,
          model:           '',
          platform:        'Windows',
          platformVersion: '15.0.0',
          uaFullVersion:   '125.0.6422.141',
          wow64:           false,
        });
      },
      toJSON: function() {
        return { brands: _uaBrands, mobile: false, platform: 'Windows' };
      },
    };
    Object.defineProperty(navigator, 'userAgentData', { get: function(){ return _uaData; } });
  } catch(e) {}

  // ── Nav bar (only on external OAuth pages so user can always go back) ───────
  if (location.hostname !== 'warframe.market' && !location.hostname.endsWith('.warframe.market')) {
    function injectNavBar() {
      if (document.getElementById('__ff_nav') || !document.body) return;
      var bar = document.createElement('div');
      bar.id = '__ff_nav';
      bar.style.cssText = 'position:fixed;top:0;left:0;right:0;z-index:2147483647;height:32px;background:#1a1a2e;border-bottom:1px solid #333;display:flex;align-items:center;gap:6px;padding:0 8px;font-family:sans-serif;font-size:12px;color:#ccc;';
      function btn(label, action) {
        var b = document.createElement('button');
        b.textContent = label;
        b.style.cssText = 'background:#2a2a4a;border:1px solid #444;color:#ccc;padding:2px 10px;border-radius:4px;cursor:pointer;font-size:12px;';
        b.onmouseenter = function(){ b.style.background='#3a3a5a'; };
        b.onmouseleave = function(){ b.style.background='#2a2a4a'; };
        b.onclick = action;
        return b;
      }
      bar.appendChild(btn('← Back', function(){ history.back(); }));
      bar.appendChild(btn('⌂ Login page', function(){ window.location.href='https://warframe.market/auth/signin'; }));
      var lbl = document.createElement('span');
      lbl.style.cssText = 'flex:1;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;opacity:.5;font-size:11px;';
      lbl.textContent = location.hostname;
      bar.appendChild(lbl);
      var s = document.createElement('style');
      s.textContent = 'html{margin-top:32px!important}';
      document.head.appendChild(s);
      document.body.insertBefore(bar, document.body.firstChild);
    }
    if (document.body) injectNavBar(); else window.addEventListener('DOMContentLoaded', injectNavBar);
  }

  // ── WFM-only: token capture ─────────────────────────────────────────────────
  if (location.hostname !== 'warframe.market' && !location.hostname.endsWith('.warframe.market')) return;

  // Strip target="_blank" from all links before the user clicks them.
  // WebView2 fires NewWindowRequested at the native level before any JavaScript
  // click handler can call preventDefault — so a capture-phase interceptor is
  // always too late. Removing the target attribute in advance means WebView2
  // never sees target="_blank" and treats every link as a same-window navigation.
  // This keeps Steam/Xbox/Discord OAuth flows inside this configured window
  // (Chrome UA, anti-detection) instead of spawning a blank unconfigured popup.
  function stripTargets(root) {
    (root || document).querySelectorAll('a[target]').forEach(function(a) {
      a.removeAttribute('target');
      a.removeAttribute('rel');
    });
  }
  if (document.body) { stripTargets(); } else { window.addEventListener('DOMContentLoaded', function() { stripTargets(); }); }
  new MutationObserver(function(mutations) {
    mutations.forEach(function(m) {
      m.addedNodes.forEach(function(n) {
        if (n.nodeType !== 1) return;
        if (n.tagName === 'A') { n.removeAttribute('target'); n.removeAttribute('rel'); }
        if (n.querySelectorAll) { stripTargets(n); }
      });
    });
  }).observe(document.documentElement, { childList: true, subtree: true });

  // Backup: override window.open() for any JS-triggered popups.
  var _origOpen = window.open;
  window.open = function(url, target, features) {
    if (url && typeof url === 'string' && url.length > 0) {
      window.location.href = url;
      return null;
    }
    return _origOpen.apply(this, arguments);
  };

  var _clientId = '', _deviceId = '';
  function sendTokens(d, v1Jwt) {
    if (!d || !d.accessToken || window.__wfmDone) return;
    window.__wfmDone = true;
    setTimeout(function() {
      var csrfMeta = document.querySelector('meta[name="csrf-token"]');
      var csrf = csrfMeta ? csrfMeta.getAttribute('content') : '';
      if (window.__TAURI__) {
        window.__TAURI__.core.invoke('wfm_receive_tokens', {
          accessToken:  d.accessToken,
          refreshToken: d.refreshToken || '',
          clientId:     _clientId,
          deviceId:     _deviceId,
          v1Jwt:        v1Jwt || null,
          csrfToken:    csrf || null,
        }).catch(function() {});
      }
    }, 500);
  }
  var origFetch = window.fetch;
  window.fetch = function(input, init) {
    var url = typeof input === 'string' ? input : (input && input.url) || '';
    if (url.includes('/auth/signin') && init && init.body) {
      try { var b = JSON.parse(init.body); _clientId = b.clientId||''; _deviceId = b.deviceId||''; } catch(e) {}
    }
    var p = origFetch.apply(this, arguments);
    if (url.includes('/auth/')) {
      p.then(function(r) {
        var v1Jwt = r.headers.get('Authorization') || '';
        if (v1Jwt.startsWith('JWT ')) v1Jwt = v1Jwt.slice(4);
        r.clone().json().then(function(j) {
          if (j && j.data && j.data.accessToken) sendTokens(j.data, v1Jwt || null);
        }).catch(function(){});
      }).catch(function(){});
    }
    return p;
  };
  // Also capture device_id from the URL — used by OAuth flows that start
  // at /auth/steam?device_id=... instead of via the email/password form.
  try {
    var _urlDeviceId = new URLSearchParams(location.search).get('device_id');
    if (_urlDeviceId) _deviceId = _urlDeviceId;
  } catch(e) {}

  var origOpen = XMLHttpRequest.prototype.open;
  var origSend = XMLHttpRequest.prototype.send;
  var _xhrUrl = '';
  XMLHttpRequest.prototype.open = function(m, u) { _xhrUrl = u || ''; return origOpen.apply(this, arguments); };
  XMLHttpRequest.prototype.send = function(body) {
    if (_xhrUrl.includes('/auth/')) {
      var self = this;
      self.addEventListener('load', function() {
        try { var j = JSON.parse(self.responseText); if (j && j.data) sendTokens(j.data); } catch(e) {}
      });
      if (body) { try { var b = JSON.parse(body); _clientId = b.clientId||_clientId; _deviceId = b.deviceId||_deviceId; } catch(e) {} }
    }
    return origSend.apply(this, arguments);
  };
})();
"#;

    build_wfm_webview(app, start_url, SCRIPT)
}




fn build_wfm_webview(app: &tauri::AppHandle, url: &str, script: &str) -> Result<tauri::WebviewWindow, String> {
    tauri::WebviewWindowBuilder::new(
        app,
        "wfm-login",
        tauri::WebviewUrl::External(url.parse()
            .map_err(|e| format!("URL parse: {}", e))?),
    )
    .title("Log in to warframe.market")
    .inner_size(520.0, 760.0)
    .resizable(true)
    .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/125.0.0.0 Safari/537.36")
    .devtools(true)
    .initialization_script(script)
    .build()
    .map_err(|e| format!("Window create: {}", e))
}

/// Legacy — the new injection script calls wfm_receive_tokens directly.
/// Kept so older injected scripts that only captured the JWT still work.
#[tauri::command]
fn wfm_receive_jwt(app: tauri::AppHandle, state: State<AppState>, jwt: String) -> Result<(), String> {
    wfm_receive_tokens(app, state, jwt, String::new(), String::new(), String::new(), None, None)
}

/// Receive tokens captured by the WebView injection script.
/// Calls /v2/me to get the username, stores session, closes login window.
#[tauri::command]
fn wfm_receive_tokens(
    app: tauri::AppHandle, state: State<AppState>,
    access_token: String, refresh_token: String,
    client_id: String, device_id: String,
    #[allow(non_snake_case)] v1Jwt: Option<String>,
    #[allow(non_snake_case)] csrfToken: Option<String>,
) -> Result<(), String> {
    let (username, _status) = state.wfm.adopt_tokens(
        access_token, refresh_token, client_id, device_id,
        v1Jwt.unwrap_or_default(), csrfToken,
    )?;
    if let Some(win) = app.get_webview_window("wfm-login") { let _ = win.close(); }
    let _ = app.emit("wfm-auth-complete", &username);
    Ok(())
}

/// Use the stored refresh token to silently get a new access token.
#[tauri::command]
fn wfm_refresh_token(state: State<AppState>) -> Result<(), String> {
    state.wfm.refresh()
}

/// Restore a session from saved token data (JSON string).
/// Returns (username, status) so the frontend can set both in one step.
#[tauri::command]
fn wfm_set_jwt(state: State<AppState>, jwt: String) -> Result<(String, String), String> {
    // `jwt` is the JSON bundle saved by wfm_save_credentials (or, for old saves,
    // a bare access token — restore_from_json handles both).
    state.wfm.restore_from_json(&jwt)
}

/// Log in via v1 signin (current recommended method per WFM Discord).
/// Token is returned in the set-cookie header: "JWT=eyJ...; Path=/; ..."
/// Use it as: Authorization: Bearer <token>
#[tauri::command]
fn wfm_login(state: State<AppState>, email: String, password: String) -> Result<String, String> {
    state.wfm.login(&email, &password)
}

/// Fetch current in-game buy and sell orders for an item, sorted by price.
/// When `mod_rank` is provided the results are filtered to that specific rank only.
#[tauri::command]
fn wfm_get_item_orders(state: State<AppState>, url_name: String, mod_rank: Option<u32>) -> Result<serde_json::Value, String> {
    state.wfm.item_orders(&url_name, mod_rank)
}

/// Fetch 90-day price statistics for an item (daily medians for the chart).
#[tauri::command]
fn wfm_get_item_statistics(state: State<AppState>, url_name: String) -> Result<serde_json::Value, String> {
    state.wfm.item_statistics(&url_name)
}

// ── Top WFM items by 7-day trade volume ───────────────────────────────────────

#[derive(serde::Serialize, serde::Deserialize)]
struct WfmTopDiskCache {
    saved_at: u64,          // Unix seconds
    items: Vec<WfmTopItem>,
}

/// Return the top 10 most-traded items on warframe.market by 7-day total value.
/// Queries Prime Sets and Arcanes from the local WFCD catalog (already loaded).
/// Results are cached for 3 hours so repeated tab opens are instant.
#[tauri::command]
async fn get_wfm_top_items(state: State<'_, AppState>) -> Result<Vec<WfmTopItem>, String> {
    const TOP_TTL: std::time::Duration = std::time::Duration::from_secs(3 * 3600);

    // In-memory cache, fresh within the TTL — the client owns it.
    if let Some(items) = state.wfm.cached_top_items(TOP_TTL) {
        return Ok(items);
    }

    // Disk cache — survives app restarts.
    let disk_cache_path = state.wfm_top_cache_path.clone();
    if let Ok(s) = std::fs::read_to_string(&disk_cache_path) {
        if let Ok(dc) = serde_json::from_str::<WfmTopDiskCache>(&s) {
            let now_secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
            if now_secs.saturating_sub(dc.saved_at) < TOP_TTL.as_secs() && !dc.items.is_empty() {
                state.wfm.set_top_items(dc.items.clone());
                return Ok(dc.items);
            }
        }
    }

    // Only one scan at a time. If another is already running, wait for it to populate
    // the cache rather than starting a second 90-second scan that would compete for the
    // rate-limiter budget and double the total time.
    if WFM_SCAN_RUNNING.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).is_err() {
        for _ in 0..120u32 {  // poll every 5 s, max 10 minutes
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            if let Some(items) = state.wfm.cached_top_items(TOP_TTL) {
                return Ok(items);
            }
        }
        return Err("WFM top items scan timed out".to_string());
    }

    // Collect arcane candidates from WFCD without holding the lock across await points.
    // Prime Sets come from WFM's own item list (fetched inside spawn_blocking below) so
    // that we get canonical slugs — WFCD doesn't have set-level entries.
    let arcane_candidates: Vec<(String, String, Option<String>)> = {
        let items = state.wfcd_items.lock().map_err(|e| e.to_string())?;
        items.iter()
            .filter(|i| i.category == "Arcanes")
            .map(|i| (i.name.clone(), to_wfm_slug(&i.name), i.image_name.clone()))
            .collect()
    };

    // Run blocking ureq calls on the thread pool — keeps the async runtime free
    let wfm = state.wfm.clone();
    let scan_result = tokio::task::spawn_blocking(move || {
        let prime_sets = wfm.prime_sets();
        let mut out: Vec<WfmTopItem> = Vec::new();

        for (name, url_name) in &prime_sets {
            if let Some((price, daily_vol)) = wfm.stats_7day(url_name) {
                out.push(WfmTopItem {
                    name:           name.clone(),
                    url_name:       url_name.clone(),
                    image_name:     None,
                    unit_price:     price,
                    daily_volume:   daily_vol,
                    total_value_7d: (price as f64 * daily_vol * 7.0) as u64,
                });
            }
        }

        for (name, slug, image_name) in &arcane_candidates {
            if let Some((price, daily_vol)) = wfm.stats_7day(slug) {
                out.push(WfmTopItem {
                    name:           name.clone(),
                    url_name:       slug.clone(),
                    image_name:     image_name.clone(),
                    unit_price:     price,
                    daily_volume:   daily_vol,
                    total_value_7d: (price as f64 * daily_vol * 7.0) as u64,
                });
            }
        }

        out.sort_by(|a, b| b.total_value_7d.cmp(&a.total_value_7d));
        out.truncate(10);
        out
    }).await;

    // Release the scan slot before propagating any error
    WFM_SCAN_RUNNING.store(false, Ordering::SeqCst);

    let results = scan_result.map_err(|e| e.to_string())?;

    // Write to disk so the results survive an app restart
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
    if let Ok(json) = serde_json::to_string(&WfmTopDiskCache { saved_at: now_secs, items: results.clone() }) {
        let _ = std::fs::write(&disk_cache_path, json);
    }

    state.wfm.set_top_items(results.clone());
    Ok(results)
}

/// Save the WFM access token to Windows Credential Manager (encrypted by the OS).
/// Stored under "FrameForge_WFM" — username field = the email, blob = the token.
#[tauri::command]
#[cfg(target_os = "windows")]
fn wfm_save_credentials(email: String, token: String) -> Result<(), String> {
    use windows_sys::Win32::Security::Credentials::{
        CredWriteW, CREDENTIALW, CRED_TYPE_GENERIC, CRED_PERSIST_LOCAL_MACHINE,
    };
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    let target: Vec<u16> = OsStr::new("FrameForge_WFM").encode_wide().chain(Some(0)).collect();
    let user:   Vec<u16> = OsStr::new(&email).encode_wide().chain(Some(0)).collect();
    let token_bytes = token.as_bytes();

    let cred = CREDENTIALW {
        Flags: 0,
        Type: CRED_TYPE_GENERIC,
        TargetName: target.as_ptr() as *mut _,
        Comment: std::ptr::null_mut(),
        LastWritten: unsafe { std::mem::zeroed() },
        CredentialBlobSize: token_bytes.len() as u32,
        CredentialBlob: token_bytes.as_ptr() as *mut _,
        Persist: CRED_PERSIST_LOCAL_MACHINE,
        AttributeCount: 0,
        Attributes: std::ptr::null_mut(),
        TargetAlias: std::ptr::null_mut(),
        UserName: user.as_ptr() as *mut _,
    };
    let ok = unsafe { CredWriteW(&cred, 0) };
    if ok == 0 { Err("Failed to save to Windows Credential Manager".into()) } else { Ok(()) }
}

/// Load WFM credentials from Windows Credential Manager.
#[tauri::command]
#[cfg(target_os = "windows")]
fn wfm_load_credentials() -> Result<Option<(String, String)>, String> {
    use windows_sys::Win32::Security::Credentials::{
        CredReadW, CredFree, CREDENTIALW, CRED_TYPE_GENERIC,
    };
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use std::slice;

    let target: Vec<u16> = OsStr::new("FrameForge_WFM").encode_wide().chain(Some(0)).collect();
    let mut cred_ptr: *mut CREDENTIALW = std::ptr::null_mut();
    let ok = unsafe { CredReadW(target.as_ptr(), CRED_TYPE_GENERIC, 0, &mut cred_ptr) };
    if ok == 0 || cred_ptr.is_null() { return Ok(None); }

    let cred = unsafe { &*cred_ptr };
    let email = unsafe {
        let ptr = cred.UserName;
        if ptr.is_null() { String::new() } else {
            let len = (0..).take_while(|&i| *ptr.offset(i) != 0).count();
            String::from_utf16_lossy(slice::from_raw_parts(ptr, len))
        }
    };
    let token = unsafe {
        if cred.CredentialBlob.is_null() || cred.CredentialBlobSize == 0 { String::new() } else {
            String::from_utf8_lossy(slice::from_raw_parts(cred.CredentialBlob, cred.CredentialBlobSize as usize)).to_string()
        }
    };
    unsafe { CredFree(cred_ptr as *mut _); }
    Ok(Some((email, token)))
}

/// Delete saved WFM credentials from Windows Credential Manager.
///
/// Async only so that every platform's delete has one shape for `wfm_logout` to
/// await; `CredDeleteW` itself returns immediately and never prompts.
#[tauri::command]
#[cfg(target_os = "windows")]
async fn wfm_delete_credentials() -> Result<(), String> {
    use windows_sys::Win32::Security::Credentials::{CredDeleteW, CRED_TYPE_GENERIC};
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    let target: Vec<u16> = OsStr::new("FrameForge_WFM").encode_wide().chain(Some(0)).collect();
    unsafe { CredDeleteW(target.as_ptr(), CRED_TYPE_GENERIC, 0); }
    Ok(())
}

/// Clear the stored WFM session.
///
/// A saved token outlives the in-memory session: the next launch restores it
/// before the user sees anything, so a logout that only cleared memory would
/// appear not to have happened. The delete sits here rather than at the call
/// site so every route to a logout inherits it.
#[tauri::command]
async fn wfm_logout(state: State<'_, AppState>) -> Result<(), String> {
    state.wfm.clear_session();
    wfm_delete_credentials().await
}

/// Return (username, status) for the current session, or None if not logged in.
#[tauri::command]
fn wfm_get_session(state: State<AppState>) -> Option<(String, String)> {
    state.wfm.identity()
}

/// Fetch the user's actual current status from WFM (`/v2/me`).
/// Returns one of: "online" | "ingame" | "invisible" | "offline".
/// Call this after session restore so the UI reflects what WFM actually has,
/// not just the hardcoded default.
#[tauri::command]
fn wfm_fetch_status(state: State<AppState>) -> Result<String, String> {
    state.wfm.fetch_status()
}

/// Return the current session token data as JSON for saving.
#[tauri::command]
fn wfm_get_jwt(state: State<AppState>) -> Option<String> {
    state.wfm.token_json()
}

/// Fetch the authenticated user's active buy + sell orders.
#[tauri::command]
fn wfm_get_orders(state: State<AppState>) -> Result<serde_json::Value, String> {
    state.wfm.my_orders()
}

/// Set WFM online status via WebSocket.
/// Connects, authenticates, sends status with 6-hour duration, then disconnects.
/// The duration means status persists even after the connection closes.
/// Values: "online" | "ingame" | "invisible"
#[tauri::command]
async fn wfm_set_status(state: State<'_, AppState>, status: String) -> Result<(), String> {
    // The WebSocket round-trip is blocking; run it off the async runtime.
    let wfm = state.wfm.clone();
    tokio::task::spawn_blocking(move || wfm.set_status(&status))
        .await
        .map_err(|e| format!("Task: {}", e))?
}

// ─── Riven database ───────────────────────────────────────────────────────────

static RIVEN_ABBREVIATIONS: &[(&str, &str)] = &[
    ("CD",    "Critical Damage"),
    ("CC",    "Critical Chance"),
    ("MS",    "Multishot"),
    ("DMG",   "Base Damage"),
    ("FR",    "Fire Rate"),
    ("SC",    "Status Chance"),
    ("TOX",   "Toxicity"),
    ("HEAT",  "Heat"),
    ("ELEC",  "Electricity"),
    ("COLD",  "Cold"),
    ("PT",    "Punch Through"),
    ("RLS",   "Reload Speed"),
    ("MAG",   "Magazine Size"),
    ("AMMO",  "Ammo Maximum"),
    ("ZOOM",  "Zoom"),
    ("REC",   "Recoil"),
    ("SLASH", "Slash"),
    ("PUNC",  "Puncture"),
    ("IMP",   "Impact"),
    ("PFS",   "Projectile Flight Speed"),
    ("SD",    "Status Duration"),
    ("DTI",   "Damage to Infested"),
    ("DTG",   "Damage to Grineer"),
    ("DTC",   "Damage to Corpus"),
    ("RLS",   "Reload Speed"),
    ("AS",    "Attack Speed"),
    ("RANGE", "Range"),
    ("IC",    "Initial Combo"),
    ("CC",    "Combo Count Chance"),
    ("EFF",   "Heavy Attack Efficiency"),
    ("SLIDE", "Slide Critical Chance"),
    ("FIN",   "Finisher Damage"),
    ("HA",    "Heavy Attack Damage"),
    ("SLAM",  "Slam Attack"),
];

/// Expand all-caps abbreviations in a notes string using the abbreviations table.
/// "PUNC gives 5%CC" → "Puncture gives 5% Critical Chance"
fn expand_abbrevs_in_notes(notes: &str) -> String {
    let bytes = notes.as_bytes();
    let mut result = String::with_capacity(notes.len() * 2);
    let mut last = 0;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_uppercase() {
            let start = i;
            while i < bytes.len() && bytes[i].is_ascii_uppercase() {
                i += 1;
            }
            // Only expand if surrounded by non-alphabetic chars (word boundary)
            let prev_alpha = start > 0 && bytes[start - 1].is_ascii_alphabetic();
            let next_alpha = i < bytes.len() && bytes[i].is_ascii_alphabetic();
            if !prev_alpha && !next_alpha {
                let word = &notes[start..i];
                if let Some((_, full)) = RIVEN_ABBREVIATIONS.iter().find(|(a, _)| *a == word) {
                    result.push_str(&notes[last..start]);
                    result.push_str(full);
                    last = i;
                }
            }
        } else {
            i += 1;
        }
    }
    result.push_str(&notes[last..]);
    result
}

fn riven_abbrev_to_full(abbrev: &str) -> String {
    let up = abbrev.trim().to_uppercase();
    RIVEN_ABBREVIATIONS.iter()
        .find(|(a, _)| *a == up.as_str())
        .map(|(_, f)| f.to_string())
        .unwrap_or_else(|| abbrev.to_string())
}

/// Parse spreadsheet stat string into alternatives, each containing slot groups.
/// "or" = completely separate valid build paths — scored independently.
/// Space-separated = each token is its own required slot.
/// Slash-separated = any one of these fills that slot.
///
/// "TOX DTC or TOX DTG or CD MS/TOX/FR" →
///   [ [[TOX],[DTC]], [[TOX],[DTG]], [[CD],[MS,TOX,FR]] ]
fn parse_stat_alternatives(s: &str) -> Vec<Vec<Vec<String>>> {
    let without_note = s.split('(').next().unwrap_or(s);
    let mut alternatives: Vec<Vec<Vec<String>>> = Vec::new();
    for alt in without_note.split(" or ") {
        let mut groups: Vec<Vec<String>> = Vec::new();
        for token in alt.split_whitespace() {
            let options: Vec<String> = token.split('/')
                .filter_map(|t| { let t = t.trim(); if t.is_empty() { None } else { Some(riven_abbrev_to_full(t)) } })
                .collect();
            if !options.is_empty() { groups.push(options); }
        }
        if !groups.is_empty() { alternatives.push(groups); }
    }
    if alternatives.is_empty() { alternatives.push(vec![]); }
    alternatives
}

/// Flat list helper — kept for the wanted display (unique stat names across all alternatives)
fn parse_stat_groups(s: &str) -> Vec<Vec<String>> {
    let alts = parse_stat_alternatives(s);
    let mut all: Vec<Vec<String>> = Vec::new();
    for alt in alts {
        for group in alt {
            if !all.iter().any(|g| g == &group) { all.push(group); }
        }
    }
    all
}

/// Whether `text` (already lowercased) carries the riven screen's "FITS IN"
/// panel label. The label is small enough on a 4K frame that an engine can close
/// the word gap and report "FITSIN", so both sides are compared with spaces
/// removed.
fn says_fits_in(text: &str) -> bool {
    text.replace(' ', "").contains("fitsin")
}

/// Weapon-name candidates from the "FITS IN" panel's OCR, top to bottom.
///
/// The panel is mostly icon and border debris (single glyphs, punctuation)
/// with the weapon name and the panel's own buttons as the only real words, so a
/// candidate is a line of at least four letters that is not one of those
/// buttons. The name sits below the "FITS IN" label and above "SHOW RANKED",
/// which is why callers take the last candidate rather than the first.
fn panel_weapon_candidates(panel: &str) -> Vec<String> {
    panel
        .lines()
        .map(|l| l.trim().to_lowercase())
        .filter(|l| {
            l.chars().filter(|c| c.is_alphabetic()).count() >= 4
                && !says_fits_in(l)
                && !l.contains("show ranked")
                && !l.contains("close")
                && !l.contains("cancel")
        })
        .collect()
}

/// Rejoin a riven card's OCR text into one line per stat.
///
/// A stat starts with `+<digit>`, `-<digit>` or `x<digit>`; the digit matters
/// because the card's dividers arrive as bare signs. Long names wrap onto a
/// second line ("+22.2% Magazine" / "Capacity"), so a following line is normally
/// the tail of the stat above it.
///
/// The border, rank pips and element icons also arrive as short punctuation
/// (`_`, `;`, `==`, `¢ Y`). Gluing those into a name breaks the lookup
/// ("Magazine _ Capacity"), so a continuation has to read as a word: three or
/// more letters, which also excludes the "MR11" rank label. Trailing debris is
/// left alone, since the lookup matches on substrings.
fn join_wrapped_stat_lines(text: &str) -> Vec<String> {
    let mut joined: Vec<String> = Vec::new();
    let mut pending: Option<String> = None;
    for line in text.lines() {
        // Artwork bleed puts stray glyphs in front of a sign ("v & -34.3%
        // Critical Chance"), hiding it so the stat joins upward and two are lost.
        // Trim only a short prefix carrying no word of its own: "Re-1oad Speed"
        // is a wrapped name, and "MR-1" would become a stat the card never had.
        // Counted in chars, not bytes, since this is where multi-byte glyphs land.
        let l = line.trim();
        let l = match l.find(['+', '-', 'x', 'X']) {
            Some(at) if at > 0
                && l[..at].chars().count() <= 4
                && !l[..at].ends_with(|c: char| c.is_alphanumeric())
                && l[at + 1..].starts_with(|c: char| c.is_ascii_digit())
                && l[..at].chars().filter(|c| c.is_alphabetic()).count() <= 2
                => &l[at..],
            _ => l,
        };
        if l.is_empty() { continue; }
        let ll = l.to_lowercase();
        // OCR sometimes misreads '+' as '•', '·', or similar bullet chars
        let first_char = l.chars().next().unwrap_or(' ');
        let is_ocr_plus = "•·○●◦".contains(first_char)
            && l.len() > 1
            && l.chars().nth(1).map_or(false, |c| c.is_ascii_digit());
        // A sign alone is not a stat: dividers come through as bare "-" lines,
        // which invented a negative stat on every card. Require a digit behind it.
        let is_signed_value = (l.starts_with('+') || l.starts_with('-'))
            && l[1..].trim_start().starts_with(|c: char| c.is_ascii_digit());
        let is_stat_start = is_signed_value
            || (ll.starts_with('x') && l.len() > 2 && l.chars().nth(1).map_or(false, |c| c.is_ascii_digit()))
            || is_ocr_plus;
        // "Damage to Grineer/Corpus/Infested" arrives unprefixed when the OCR
        // drops the leading "x0.88" multiplier.
        let is_orphan_stat = ll.starts_with("damage to grineer")
            || ll.starts_with("damage to corpus")
            || ll.starts_with("damage to infested");
        // "kuva" comes off the weapon-name filter below but stays here: a reroll
        // comparison screen stacks two cards, so the lower card's title follows
        // the upper card's stats with nothing between, and a title reads as a word.
        // TODO: only Kuva titles are caught. "Boltor Conci-" still glues, which
        // needs a title recognised as a title rather than another word on a list.
        let is_ui_noise = ll.contains("fits in") || ll.starts_with("mr ")
            || ll.contains("inventory") || ll.contains("cycle")
            || ll.contains("kuva") || ll.contains("remaining")
            || ll.contains("show ranked") || ll.contains("cancel");
        let reads_as_a_word = l.chars().filter(|c| c.is_alphabetic()).count() >= 3;
        if is_stat_start {
            if let Some(prev) = pending.take() { joined.push(prev); }
            pending = Some(l.to_string());
        } else if is_orphan_stat {
            if let Some(prev) = pending.take() { joined.push(prev); }
            joined.push(format!("+?% {}", l));
        } else if is_ui_noise {
            if let Some(prev) = pending.take() { joined.push(prev); }
        } else if reads_as_a_word {
            if let Some(ref mut prev) = pending {
                prev.push(' ');
                prev.push_str(l);
            }
        }
    }
    if let Some(prev) = pending { joined.push(prev); }
    joined
}

/// Flat dedup list of all stats across all groups — kept for backwards compat where needed.
fn parse_riven_stat_str(s: &str) -> Vec<String> {
    let mut result = Vec::new();
    for group in parse_stat_groups(s) {
        for stat in group {
            if !result.contains(&stat) { result.push(stat); }
        }
    }
    result
}

fn csv_split_line(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut cur = String::new();
    let mut in_q = false;
    for ch in line.chars() {
        match ch {
            '"' => in_q = !in_q,
            ',' if !in_q => { fields.push(cur.trim().to_string()); cur = String::new(); }
            c => cur.push(c),
        }
    }
    fields.push(cur.trim().to_string());
    fields
}

#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct RivenEntry {
    pub weapon: String,
    /// Outer Vec = "or" alternatives (each is a completely separate valid build).
    /// Middle Vec = slot groups within that alternative.
    /// Inner Vec  = options for that slot (slash-separated).
    /// "TOX DTC or TOX DTG" → [[[TOX],[DTC]], [[TOX],[DTG]]]
    pub stat_alternatives: Vec<Vec<Vec<String>>>,
    /// Flat dedup list for backwards-compat display (unique groups across all alternatives)
    pub stat_groups: Vec<Vec<String>>,
    pub safe_negatives: Vec<String>,
    pub notes: String,
}

#[derive(serde::Serialize, Clone)]
pub struct AlternativeResult {
    pub label: String,        // "Option 1", "Option 2", etc.
    pub matched: Vec<String>,
    pub missing: Vec<String>,
    pub score: f32,
    pub verdict: String,
}

#[derive(serde::Serialize)]
pub struct RivenAnalysis {
    pub weapon: String,
    pub matched_positives: Vec<String>,   // best alternative
    pub missing_positives: Vec<String>,   // best alternative
    pub safe_negatives_present: Vec<String>,
    pub harmful_negatives: Vec<String>,
    pub total_wanted: usize,
    pub score: f32,
    pub verdict: String,
    pub notes: String,
    pub alternatives: Vec<AlternativeResult>, // one per "or" path
}

static RIVEN_DB: std::sync::OnceLock<std::sync::Mutex<HashMap<String, RivenEntry>>> =
    std::sync::OnceLock::new();

/// Returns a map of weapon unique_name → riven disposition (omegaAttenuation).
/// Data comes from All.json (fetched during item load) — no extra HTTP request.
#[tauri::command]
fn get_weapon_dispositions(state: State<AppState>) -> HashMap<String, f32> {
    state.weapon_dispositions.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Guards against concurrent scans: only one get_wfm_top_items scan runs at a time.
/// Concurrent callers wait (polling the cache) rather than starting a second scan.
/// Scan orchestration is the command's concern; the cached result it produces
/// lives in `Wfm`.
static WFM_SCAN_RUNNING: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Cache: (warframe_pid, Option<flag_va>). None inner = scanned this PID, pattern not found.
/// Re-scanned only when PID changes (game restart). Prevents 200ms re-scan storm.
static RIVEN_FLAG_VA: std::sync::OnceLock<std::sync::Mutex<Option<(u32, Option<usize>)>>> =
    std::sync::OnceLock::new();

/// Guard: prevents spawning multiple watcher threads if start_riven_memory_watcher is called again.
static RIVEN_WATCHER_RUNNING: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

fn get_riven_db() -> &'static std::sync::Mutex<HashMap<String, RivenEntry>> {
    RIVEN_DB.get_or_init(|| {
        std::sync::Mutex::new(load_riven_csv_from_url().unwrap_or_default())
    })
}

const RIVEN_SHEET_ID: &str = "1zbaeJBuBn44cbVKzJins_E3hTDpnmvOk8heYN-G8yy8";
// Tabs: 0=primary, 1505239276=secondary, 1413904270=melee, 289737427=archwing, 965095749=other
// 1687910063 is the legend/info page — skip it
const RIVEN_SHEET_GIDS: &[u64] = &[0, 1505239276, 1413904270, 289737427, 965095749];

fn load_riven_csv_from_url() -> Result<HashMap<String, RivenEntry>, String> {
    let mut combined = HashMap::new();
    for &gid in RIVEN_SHEET_GIDS {
        let url = format!(
            "https://docs.google.com/spreadsheets/d/{}/export?format=csv&gid={}",
            RIVEN_SHEET_ID, gid
        );
        match ureq::get(&url)
            .set("User-Agent", "FrameForge/3.2.0")
            .call().map_err(|e| e.to_string())
            .and_then(|r| r.into_string().map_err(|e| e.to_string()))
        {
            Ok(csv) => { combined.extend(parse_riven_csv(&csv)); }
            Err(e) => { warn!(gid, error = %e, "failed to load riven sheet tab"); }
        }
    }
    if combined.is_empty() {
        return Err("No riven data loaded from any sheet tab".into());
    }
    Ok(combined)
}

fn parse_riven_csv(csv: &str) -> HashMap<String, RivenEntry> {
    let mut map = HashMap::new();
    let mut lines = csv.lines();

    // Read header to find which column holds "NEGATIVE STATS:" — it varies by tab
    let header = match lines.next() { Some(h) => h, None => return map };
    let hf = csv_split_line(header);
    let neg_col = hf.iter().position(|c| c.trim().to_lowercase().contains("negative")).unwrap_or(5);
    let notes_col = hf.iter().position(|c| c.trim().to_lowercase().contains("note")).unwrap_or(8);

    for line in lines {
        let f = csv_split_line(line);
        if f.len() < neg_col + 1 { continue; }
        let weapon = f[0].trim().to_lowercase();
        if weapon.is_empty() { continue; }
        let stat_alternatives = parse_stat_alternatives(&f[1]);
        let stat_groups = parse_stat_groups(&f[1]);
        let safe_neg    = parse_riven_stat_str(&f[neg_col]);
        let raw_notes   = f.get(notes_col).map(|s| s.trim().trim_matches('"').to_string()).unwrap_or_default();
        let notes       = expand_abbrevs_in_notes(&raw_notes);
        map.insert(weapon.clone(), RivenEntry { weapon, stat_alternatives, stat_groups, safe_negatives: safe_neg, notes });
    }
    map
}

/// Like ocr_stat_to_full but first tries the full conditional name, then strips "for X" and retries.
/// "Critical Chance for Slide Attack" → "Slide Critical Chance" (full wins)
/// "Critical Damage for Slide Attack" → stripped → "Critical Damage" (full doesn't match, fallback)
fn ocr_stat_to_full_with_condition(ocr_name: &str) -> String {
    let full_try = ocr_stat_to_full(ocr_name);
    if full_try != ocr_name {
        return full_try; // matched on full name
    }
    // Strip "for <condition>" and try again
    let stripped = ocr_name.split(" for ").next().unwrap_or(ocr_name).trim();
    if stripped != ocr_name {
        let stripped_try = ocr_stat_to_full(stripped);
        if stripped_try != stripped {
            return stripped_try;
        }
    }
    full_try // return best effort even if unrecognized
}

/// In-game stat names → database full names (handles abbreviations and element icons stripped by OCR)
fn ocr_stat_to_full(ocr_name: &str) -> String {
    // Strip leading OCR artifacts from element icons (e.g. "61-leat" → "leat" from 🔥Heat,
    // "ld" from ❄Cold, etc.) before pattern matching.
    let stripped = ocr_name.trim().trim_start_matches(|c: char| !c.is_alphabetic());
    let n = stripped.to_lowercase();
    match n.as_str() {
        // Conditional melee stats — checked FIRST so "critical chance for slide attack" wins
        // over the generic "critical chance" pattern below
        s if s.contains("critical chance") && (s.contains("slide") || s.contains("slide attack")) => "Slide Critical Chance",
        s if s.contains("critical chance") && s.contains("aerial") => "Aerial Critical Chance",
        s if s.contains("critical chance") && s.contains("wall") => "Wall Critical Chance",
        s if s.contains("critical damage") || s.contains("crit. damage") || s.contains("crit damage") => "Critical Damage",
        s if s.contains("critical chance") || s.contains("crit. chance") || s.contains("crit chance") => "Critical Chance",
        s if s.contains("multishot") => "Multishot",
        s if s.contains("fire rate") => "Fire Rate",
        s if s.contains("status chance") => "Status Chance",
        s if s.contains("base damage") || (s.contains("damage") && !s.contains("critical") && !s.contains("infested") && !s.contains("grineer") && !s.contains("corpus")) => "Base Damage",
        // Toxin — icon may eat 'T', leaving "oxin" or "oxicity"
        s if s.contains("toxin") || s.contains("toxicity") || s.starts_with("oxin") => "Toxicity",
        // Heat — fire icon may eat 'H', leaving "eat" or "leat"
        s if s.contains("heat") || s.contains("fire damage")
            || s == "eat" || s == "leat" || (s.ends_with("eat") && s.len() <= 7) => "Heat",
        // Electricity — icon may eat 'E', leaving "lectricity" etc.
        s if s.contains("electricity") || s.contains("electric") || s.starts_with("lectr") => "Electricity",
        // Cold — ice icon may eat 'C', leaving "old"
        s if s.contains("cold") || s.contains("freeze") || s == "old" => "Cold",
        s if s.contains("punch through") => "Punch Through",
        s if s.contains("reload speed") || s.contains("reload") => "Reload Speed",
        s if s.contains("magazine size") || s.contains("magazine") || s.contains("mag size") => "Magazine Size",
        s if s.contains("ammo max") || s.contains("ammo maximum") => "Ammo Maximum",
        s if s.contains("zoom") => "Zoom",
        s if s.contains("recoil") => "Recoil",
        s if s.contains("slash") => "Slash",
        s if s.contains("puncture") => "Puncture",
        s if s.contains("impact") => "Impact",
        s if s.contains("flight speed") || s.contains("proj. flight") || s.contains("projectile") => "Projectile Flight Speed",
        s if s.contains("status duration") => "Status Duration",
        s if s.contains("infested") => "Damage to Infested",
        s if s.contains("grineer") => "Damage to Grineer",
        s if s.contains("corpus") => "Damage to Corpus",
        // Melee-specific stats
        s if s.contains("attack speed") || s.contains("attack spd") => "Attack Speed",
        s if s.contains("combo duration") => "Combo Duration",
        s if s.contains("combo count") => "Combo Count Chance",
        s if s.contains("heavy attack") && s.contains("efficiency") => "Heavy Attack Efficiency",
        s if s.contains("heavy attack") => "Heavy Attack Damage",
        s if s.contains("slam") => "Slam Attack",
        s if s.contains("slide") && s.contains("crit") => "Slide Critical Chance",
        s if s.contains("range") => "Range",
        _ => return ocr_name.to_string(),
    }.to_string()
}

/// Parse stat lines from a card's OCR text, returning rolled_stats JSON array.
fn parse_original_stats(text: Option<&str>) -> Vec<serde_json::Value> {
    let Some(text) = text else { return vec![]; };
    let mut out = Vec::new();
    for line in text.lines() {
        let l = line.trim();
        if l.to_lowercase().starts_with('x') && l.len() > 2 && l.chars().nth(1).map_or(false, |c| c.is_ascii_digit() || c == ' ') {
            let alpha_start = l.find(|c: char| c.is_alphabetic() && c != 'x').unwrap_or(l.len());
            let val = l[..alpha_start].split_whitespace().collect::<Vec<_>>().join("");
            let name_part = l[alpha_start..].trim().split(" (").next().unwrap_or("").trim();
            if !name_part.is_empty() {
                out.push(serde_json::json!({"name": ocr_stat_to_full_with_condition(name_part), "value": val, "positive": true}));
            }
            continue;
        }
        let fc = l.chars().next().unwrap_or(' ');
        let (is_pos, part) = if l.starts_with('+') { (true, l.trim_start_matches('+')) }
                             else if l.starts_with('-') { (false, l.trim_start_matches('-')) }
                             else if "•·○●◦".contains(fc) { (true, l.trim_start_matches(|c: char| "•·○●◦".contains(c))) }
                             else { continue; };
        let val = if part.contains('%') {
            let n = part.split('%').next().unwrap_or("").trim();
            format!("{}{}%", if is_pos { "+" } else { "-" }, n)
        } else {
            let e = part.find(|c: char| !c.is_ascii_digit() && c != '.').unwrap_or(part.len());
            format!("{}{}%", if is_pos { "+" } else { "-" }, &part[..e])
        };
        let sname: &str = if let Some(a) = part.splitn(2, '%').nth(1) { a.trim() }
                          else { let e = part.find(|c: char| c.is_alphabetic()).unwrap_or(0);
                                 part[e..].trim_start_matches(|c: char| !c.is_alphabetic()) };
        if sname.is_empty() { continue; }
        let sname = sname.trim_start_matches(|c: char| !c.is_alphabetic());
        let sname = sname.split(" (").next().unwrap_or(sname).trim();
        out.push(serde_json::json!({"name": ocr_stat_to_full_with_condition(sname), "value": val, "positive": is_pos}));
    }
    out
}

/// Capture the riven reroll screen and OCR the stats + weapon name.
/// Returns (weapon_name, positives, negatives).
#[tauri::command]
async fn ocr_riven_screen() -> Result<serde_json::Value, String> {
    let riven_log = std::env::temp_dir().join("frameforge_riven_session.txt");
    let ts1 = chrono::Local::now().format("%H:%M:%S%.3f").to_string();

    let _ = append_to_file(&riven_log, &format!(
        "[STEP 2] OCR STARTED — {}\n\
         ├─ Capture region : y 0%–75% (header + card + FITS IN panel)\n\
         └─ Validating: expects \"INVENTORY/MODS\" at top + \"FITS IN\" on right\n",
        ts1
    ));

    // Capture y 0–0.75: includes the "INVENTORY / MODS" header at the top and the
    // "FITS IN" weapon panel on the right. We retry until both markers are visible —
    // this filters out false EE.log triggers and handles slow screen transitions.
    const MAX_ATTEMPTS: u32 = 6;
    const RETRY_MS: u64 = 350;

    let mut text = String::new();
    let mut full_text_for_fallback = String::new();
    let mut panel_for_weapon = String::new();
    let mut confirmed = false;

    for attempt in 0..MAX_ATTEMPTS {
        if attempt > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(RETRY_MS)).await;
        }

        let riven_log2 = riven_log.clone();
        // One PrintWindow capture; two OCR passes from the same pixels:
        //   • Full width (0–100%) for validation markers ("INVENTORY/MODS" + "FITS IN")
        //   • Card column only (20–65%) for stat parsing — excludes the right panel whose
        //     "FITS IN" / weapon label text can interfere with reading the card's bottom stats.
        let attempt_result = tokio::task::spawn_blocking(move || {
            let ts = chrono::Local::now().format("%H:%M:%S%.3f").to_string();
            let px = ocr::capture_warframe_pixels().map_err(|e| format!("Capture: {}", e))?;
            let (pixels, w, h) = px;
            let full_text = ocr::ocr_pixels_rect(&pixels, w, h, 0.0, 1.0, 0.0, 0.82)
                .unwrap_or_default();
            let card_text = ocr::ocr_pixels_rect(&pixels, w, h, 0.20, 0.65, 0.28, 0.82)
                .unwrap_or_default();
            let panel_text = ocr::ocr_pixels_rect_raw(&pixels, w, h, 0.73, 1.0, 0.30, 0.95)
                .unwrap_or_default();
            let _ = append_to_file(&riven_log2, &format!(
                "[STEP 2] OCR attempt {} — {}\n├─ Full text:\n{}\n├─ Panel text:\n{}\n└─ Card text:\n{}\n\n",
                attempt + 1, ts, full_text, panel_text, card_text
            ));
            Ok::<_, String>((full_text, panel_text, card_text))
        }).await.map_err(|e| format!("Task: {}", e))??;

        let (full_text, panel_text, card_text) = attempt_result;
        let lower = full_text.to_lowercase();
        let has_header  = lower.contains("inventory") || lower.contains("mods");
        let has_fits_in = says_fits_in(&lower) || says_fits_in(&panel_text.to_lowercase());

        let _ = append_to_file(&riven_log, &format!(
            "[STEP 2] attempt {} — header={} fits_in={}\n",
            attempt + 1, has_header, has_fits_in
        ));

        // Count stat lines in card_text — 5+ means comparison mode (two cards visible).
        // In comparison mode the "FITS IN" panel shifts and may not OCR correctly.
        // Accept header-only confirmation when we already see enough stat lines.
        let stat_count = card_text.lines()
            .filter(|l| { let t = l.trim(); t.starts_with('+') || t.starts_with('-') })
            .count();
        let comparison_likely = stat_count >= 5;

        if (has_header && has_fits_in) || (has_header && comparison_likely) {
            text = card_text;
            full_text_for_fallback = full_text;
            panel_for_weapon = panel_text;
            confirmed = true;
            if comparison_likely && !has_fits_in {
                let _ = append_to_file(&riven_log, &format!(
                    "[STEP 2] Comparison mode early-confirm ({} stat lines, no FITS IN)\n", stat_count
                ));
            }
            break;
        }
        text = card_text;
        full_text_for_fallback = full_text;
        panel_for_weapon = panel_text;
    }

    if !confirmed {
        let _ = append_to_file(&riven_log, "[STEP 2] Screen markers not confirmed after all attempts — proceeding with last OCR result anyway\n\n");
    }

    // Detect comparison mode: >4 stat lines means two cards are visible (3–4 stats each).
    // A riven can have at most 4 stats (3 pos + 1 neg), so 5+ total implies 2 cards.
    let stat_line_count = text.lines()
        .filter(|l| { let t = l.trim(); t.starts_with('+') || t.starts_with('-') })
        .count();
    let is_comparison = stat_line_count > 4;

    if is_comparison {
        let _ = append_to_file(&riven_log, &format!(
            "[STEP 2] COMPARISON MODE detected ({} stat lines) — capturing card columns separately\n", stat_line_count
        ));
    }

    // In comparison mode: one PrintWindow capture, OCR left and right card columns.
    // Original card is ALWAYS on the left; new roll is always on the right.
    // Card area x 20–65% is split roughly in half: left=20–42%, right=42–65%.
    let (left_text, right_text) = if is_comparison {
        let riven_log3 = riven_log.clone();
        let cols = tokio::task::spawn_blocking(move || {
            match ocr::capture_warframe_pixels() {
                Ok((px, w, h)) => {
                    // Wider y range to catch element-icon stat lines near card bottom
                    let left  = ocr::ocr_pixels_rect(&px, w, h, 0.18, 0.44, 0.25, 0.84).unwrap_or_default();
                    let right = ocr::ocr_pixels_rect(&px, w, h, 0.44, 0.68, 0.25, 0.84).unwrap_or_default();
                    let _ = append_to_file(&riven_log3, &format!(
                        "[STEP 2] Original (left):\n{}\n\nNew roll (right):\n{}\n\n", left, right
                    ));
                    (left, right)
                }
                Err(e) => {
                    let _ = append_to_file(&riven_log3, &format!("[STEP 2] Column capture failed: {}\n", e));
                    (String::new(), String::new())
                }
            }
        }).await.map_err(|e| format!("Task: {}", e))?;
        cols
    } else {
        (String::new(), String::new())
    };

    // Which text to parse for the new roll:
    // - Comparison mode: right column = new roll, left column = original
    // - Single card mode: card column text; fall back to full text if card column had no stats
    let card_has_stats = text.lines().any(|l| { let t = l.trim(); t.starts_with('+') || t.starts_with('-') });
    let parse_text = if is_comparison && !right_text.is_empty() {
        &right_text
    } else if !card_has_stats && !full_text_for_fallback.is_empty() {
        // Card column empty — fall back to the full-width validated text
        let _ = append_to_file(&riven_log, "[STEP 2] Card column had no stats — using full-width text as fallback\n");
        &full_text_for_fallback
    } else {
        &text
    };
    let original_parse_text = if is_comparison && !left_text.is_empty() { Some(left_text.as_str()) } else { None };

    // Parse weapon name.
    // In the unveil screen "FITS IN" appears on its own line, weapon name on the next line.
    // In the reroll screen the mod name is "WeaponName RivenIdentifier" (e.g. "Hirudo Geli-plecinus").
    let lines: Vec<&str> = parse_text.lines().collect();

    // Helper: try to match a candidate string against the riven DB, trying word-prefix
    // substrings from longest to shortest (handles "Dual Cleavers Cronitron" → "dual cleavers").
    let find_in_db = |candidate: &str| -> Option<String> {
        let db = get_riven_db().lock().unwrap_or_else(|e| e.into_inner());
        let words: Vec<&str> = candidate.split_whitespace().collect();
        // Try 4-word prefix, then 3, 2, 1
        for len in (1..=words.len().min(4)).rev() {
            let prefix = words[..len].join(" ");
            if db.contains_key(&prefix) {
                return Some(prefix);
            }
        }
        None
    };

    // The "FITS IN" panel is the only place the game states the weapon outright,
    // and it states the real one: a Kuva Nukor riven is titled "Nukor Crita-
    // hexapha" above the card, which resolves to the ordinary Nukor and its
    // different disposition.
    //
    // The grading sheet is a curated list, not a weapon index. It carries
    // "kuva bramma" but not "kuva nukor", so a panel name it does not know is
    // still the right answer. Reporting it unmatched costs the roll analysis
    // (analyze_riven returns nothing for an unknown weapon, which the UI
    // already handles) and buys not silently grading a Kuva Nukor as the base
    // Nukor it is titled after, on a different disposition.
    let panel_candidates = panel_weapon_candidates(&panel_for_weapon);
    let weapon = panel_candidates.iter()
        .find_map(|l| find_in_db(l))
        .or_else(|| panel_candidates.last().cloned())
        .or_else(|| lines.iter().enumerate()
            .find(|(_, l)| says_fits_in(&l.to_lowercase()))
            .and_then(|(i, _)| lines.get(i + 1))
            .and_then(|l| {
                let lc = l.trim().to_lowercase();
                find_in_db(&lc).or(Some(lc))
            }))
        // Fallback: first non-stat, non-UI line is the mod name "WeaponName RivenId".
        // Only accept if it matches a weapon in the DB — avoids returning currency values
        // like "D '5,598" (Endo count) that pass the basic filter.
        .or_else(|| {
            lines.iter()
                .find_map(|l| {
                    let lt = l.trim().to_lowercase();
                    if lt.is_empty() { return None; }
                    // Skip UI noise. "kuva" is deliberately absent: it prefixes a
                    // whole weapon family, so skipping it lost the name of every
                    // Kuva riven. "Remaining Kuva 102,773" is already caught by
                    // "remaining" and the currency-value rules below.
                    if lt.contains("fits in") || lt.contains("cycle")
                    || lt.contains("mr ") || lt.contains("inventory") || lt.contains("mods")
                    || lt.contains("remaining") || lt.contains("show ranked") || lt.contains("cancel")
                    || lt.starts_with('+') || lt.starts_with('-') || lt.starts_with('x')
                    || lt.chars().next().map_or(false, |c| c.is_ascii_digit())
                    // Skip lines that look like currency values (contain digit+comma or digit+apostrophe)
                    || (lt.contains(',') && lt.chars().any(|c| c.is_ascii_digit()))
                    || (lt.contains('\'') && lt.chars().any(|c| c.is_ascii_digit()))
                    {
                        return None;
                    }
                    find_in_db(&lt) // only return if it's actually in the DB
                })
        })
        .unwrap_or_default();

    let joined = join_wrapped_stat_lines(parse_text);

    // Parse stat lines and collect rolled_stats (name + formatted value for display).
    let mut positives: Vec<String> = Vec::new();
    let mut negatives: Vec<String> = Vec::new();
    // Each entry: { "name": "Combo Count Chance", "value": "+47.2%", "positive": true }
    let mut rolled_stats: Vec<serde_json::Value> = Vec::new();

    for line in &joined {
        let l = line.trim();

        // Handle multiplier format "x1.62 Damage to Corpus"
        // OCR may insert spaces inside the number ("x1 .62"), so collect everything
        // before the first alphabetic char and join to remove those spaces.
        if l.to_lowercase().starts_with('x') && l.len() > 2 && l.chars().nth(1).map_or(false, |c| c.is_ascii_digit() || c == ' ') {
            let alpha_start = l.find(|c: char| c.is_alphabetic() && c != 'x').unwrap_or(l.len());
            let val_str = l[..alpha_start].split_whitespace().collect::<Vec<_>>().join(""); // e.g. "x1.62"
            let stat_name = l[alpha_start..].trim();
            let stat_name = stat_name.split(" (").next().unwrap_or(stat_name).trim();
            if !stat_name.is_empty() {
                let full = ocr_stat_to_full_with_condition(stat_name);
                rolled_stats.push(serde_json::json!({"name": full, "value": val_str, "positive": true}));
                positives.push(full);
            }
            continue;
        }

        let first_l = l.chars().next().unwrap_or(' ');
        let (is_pos, stat_part) = if l.starts_with('+') {
            (true, l.trim_start_matches('+'))
        } else if l.starts_with('-') {
            (false, l.trim_start_matches('-'))
        } else if "•·○●◦".contains(first_l) {
            // OCR misread '+' as a bullet/dot character — treat as positive stat
            (true, l.trim_start_matches(|c: char| "•·○●◦".contains(c)))
        } else { continue; };

        // Extract the numeric value string.
        // Must explicitly check for '%' first — split('%').next() returns Some(whole_string)
        // even when no '%' is present, which would produce "+51 'Toxin%" for element stats.
        let pct_val = if stat_part.starts_with("?%") {
            // Synthesised from orphan stat — OCR dropped the x-multiplier value
            "x?".to_string()
        } else if stat_part.contains('%') {
            let n = stat_part.split('%').next().unwrap_or("").trim();
            format!("{}{}%", if is_pos { "+" } else { "-" }, n)
        } else {
            // No % sign (element stats, OCR dropped it) — extract leading digits only
            let num_end = stat_part.find(|c: char| !c.is_ascii_digit() && c != '.').unwrap_or(stat_part.len());
            format!("{}{}%", if is_pos { "+" } else { "-" }, &stat_part[..num_end])
        };

        // Extract stat name
        let stat_name: &str = if let Some(after_pct) = stat_part.splitn(2, '%').nth(1) {
            after_pct.trim()
        } else {
            let num_end = stat_part.find(|c: char| c.is_alphabetic()).unwrap_or(0);
            stat_part[num_end..].trim_start_matches(|c: char| !c.is_alphabetic())
        };
        if stat_name.is_empty() { continue; }

        // Strip leading OCR icon artifacts: "61-leat" → "leat", " 🔥Heat" → "Heat"
        let stat_name = stat_name.trim_start_matches(|c: char| !c.is_alphabetic());
        if stat_name.is_empty() { continue; }

        // Strip parenthetical qualifiers: "Critical Chance (x2 for Heavy Attacks)" → "Critical Chance"
        let stat_name = stat_name.split(" (").next().unwrap_or(stat_name).trim();

        // Try to match with the full conditional name first so "Critical Chance for Slide Attack"
        // maps to "Slide Critical Chance" (not just "Critical Chance"). Fall back to stripped form.
        let full = ocr_stat_to_full_with_condition(stat_name);
        rolled_stats.push(serde_json::json!({"name": full, "value": pct_val, "positive": is_pos}));
        if is_pos { positives.push(full); } else { negatives.push(full); }
    }

    let ts3 = chrono::Local::now().format("%H:%M:%S%.3f").to_string();
    let _ = append_to_file(&riven_log, &format!(
        "[STEP 3] PARSE RESULT — {}\n\
         ├─ Weapon    : \"{}\"\n\
         ├─ Positives : {:?}\n\
         └─ Negatives : {:?}\n\n",
        ts3, weapon, positives, negatives
    ));

    Ok(serde_json::json!({
        "weapon": weapon,
        "positives": positives,
        "negatives": negatives,
        "rolled_stats": rolled_stats,
        "is_comparison": is_comparison,
        "original_rolled_stats": parse_original_stats(original_parse_text),
        "raw": text,
    }))
}

// ── Trade dialog parser ───────────────────────────────────────────────────────

struct ParsedTrade {
    with_player: String,
    trade_type: String,
    offered_items: Vec<(String, i64)>,
    offered_plat: i64,
    received_items: Vec<(String, i64)>,
    received_plat: i64,
    session_id: String,
    timestamp: String,
}

/// Clean a single item line from a trade dialog:
/// strips Warframe PUA rank-dot characters and normalises mod rank suffixes.
fn clean_trade_item(raw: &str) -> String {
    let raw = raw.trim();
    let filled = raw.chars().filter(|&c| c == '\u{E114}').count();
    let total  = raw.chars().filter(|&c| c == '\u{E114}' || c == '\u{E112}').count();
    if total > 0 {
        let base: String = raw.chars().take_while(|&c| c != '\u{E114}' && c != '\u{E112}').collect();
        let base = base.trim();
        return if filled == 0 { format!("{} (R0)", base) } else { format!("{} (R{})", base, filled) };
    }
    if let Some(p) = raw.find(" (") {
        let inside = &raw[p + 2..];
        if let Some(r) = inside.to_lowercase().find("rank ") {
            let rank_n = inside[r + 5..].trim_end_matches(')').trim();
            return format!("{} (R{})", &raw[..p], rank_n);
        }
        return raw[..p].trim().to_string();
    }
    raw.to_string()
}

/// Parse all items from one section of a trade dialog (offered or received).
/// Handles both repeated-line stacking and "Item x N" inline quantities.
fn extract_trade_items(section: &str) -> Vec<(String, i64)> {
    let mut order: Vec<String> = Vec::new();
    let mut counts: HashMap<String, i64> = HashMap::new();
    for line in section.lines() {
        let raw = line.trim();
        if raw.is_empty() || raw.to_lowercase().contains("platinum") { continue; }
        let (raw_name, qty) = if let Some(x_pos) = raw.rfind(" x ") {
            let qty_part = raw[x_pos + 3..].trim();
            if let Ok(n) = qty_part.parse::<i64>() { (&raw[..x_pos], n) } else { (raw, 1i64) }
        } else {
            (raw, 1i64)
        };
        let name = clean_trade_item(raw_name);
        if !name.is_empty() {
            if !counts.contains_key(&name) { order.push(name.clone()); }
            *counts.entry(name).or_insert(0) += qty;
        }
    }
    order.into_iter().map(|k| { let q = counts[&k]; (k, q) }).collect()
}

/// Parse the full trade confirmation dialog from EE.log.
/// Returns None if the dialog doesn't contain the expected markers.
fn parse_trade_dialog(raw: &str) -> Option<ParsedTrade> {
    let with_player = raw.find("will receive from ")
        .and_then(|i| { let a = &raw[i + 18..]; a.find(" the following").map(|j| a[..j].trim().to_string()) })?;
    let offered_raw = raw.find("You are offering:")
        .and_then(|i| { let a = &raw[i + 17..]; a.find("and will receive from").map(|j| a[..j].trim().to_string()) })
        .unwrap_or_default();
    let received_raw = raw.find("the following:")
        .and_then(|i| { let a = &raw[i + 14..]; a.find(", title=").map(|j| a[..j].trim().to_string()) })
        .unwrap_or_default();

    let parse_plat = |s: &str| -> i64 {
        s.find("Platinum x ")
            .and_then(|i| s[i + 11..].split(|c: char| !c.is_ascii_digit()).next())
            .and_then(|n| n.parse().ok())
            .unwrap_or(0)
    };

    let offered_plat  = parse_plat(&offered_raw);
    let received_plat = parse_plat(&received_raw);
    let offered_items  = extract_trade_items(&offered_raw);
    let received_items = extract_trade_items(&received_raw);

    if offered_items.is_empty() && received_items.is_empty() && offered_plat == 0 && received_plat == 0 {
        return None;
    }

    let trade_type = if offered_plat > 0 { "purchase" } else if received_plat > 0 { "sale" } else { "trade" };
    let now = chrono::Utc::now();

    Some(ParsedTrade {
        with_player,
        trade_type: trade_type.to_string(),
        offered_items,
        offered_plat,
        received_items,
        received_plat,
        session_id: now.format("%Y%m%dT%H%M%S%3f").to_string(),
        timestamp: now.to_rfc3339(),
    })
}

/// Start a lightweight EE.log watcher for features that don't need the memory scanner:
/// riven reroll detection, trade completion detection, WFM whisper detection.
/// Called unconditionally at app startup — EE.log is plain file I/O, not memory reading.
#[tauri::command]
fn start_log_watcher(app: tauri::AppHandle) -> Result<(), String> {
    let log_path = dirs::data_local_dir()
        .map(|d| d.join("Warframe").join("EE.log"))
        .ok_or("Cannot find LocalAppData")?;

    std::thread::spawn(move || {
        use std::io::{Read, Seek, SeekFrom};
        let mut file_pos: u64 = std::fs::metadata(&log_path).map(|m| m.len()).unwrap_or(0);
        let mut pending_trade: Option<String> = None;
        // Cooldown: don't fire riven-screen-open again within 4 seconds of the last fire.
        // Guards against the same EE.log buffer being processed twice by React StrictMode listeners.
        let mut last_riven_fire: Option<std::time::Instant> = None;
        // Cooldown: prevent spawning multiple OCR threads if the trigger fires rapidly.
        let mut last_relic_pick_trigger: Option<std::time::Instant> = None;

        // Use FindFirstChangeNotificationW so we wake up the instant EE.log is written,
        // instead of sleeping and polling. This is how Overwolf achieves low latency.
        let change_handle: isize = {
            use windows_sys::Win32::Storage::FileSystem::{
                FindFirstChangeNotificationW, FILE_NOTIFY_CHANGE_LAST_WRITE,
            };
            let dir = log_path.parent().unwrap_or(std::path::Path::new("."));
            let dir_wide: Vec<u16> = dir.to_string_lossy().encode_utf16().chain(std::iter::once(0)).collect();
            unsafe { FindFirstChangeNotificationW(dir_wide.as_ptr(), 0, FILE_NOTIFY_CHANGE_LAST_WRITE) }
        };
        let use_notify = change_handle != -1; // -1 = INVALID_HANDLE_VALUE

        loop {
            if use_notify {
                use windows_sys::Win32::System::Threading::WaitForSingleObject;
                use windows_sys::Win32::Storage::FileSystem::FindNextChangeNotification;
                // Block until EE.log directory has a write — then process immediately
                unsafe { WaitForSingleObject(change_handle, 500); } // 500ms safety timeout
                unsafe { FindNextChangeNotification(change_handle); }
            } else {
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            let Ok(mut f) = std::fs::File::open(&log_path) else { continue };
            let len = std::fs::metadata(&log_path).map(|m| m.len()).unwrap_or(0);
            if len < file_pos { file_pos = 0; }
            if len == file_pos { continue; } // nothing new since last read
            if f.seek(SeekFrom::Start(file_pos)).is_err() { continue; }
            let mut buf = String::new();
            if f.read_to_string(&mut buf).is_err() { continue; }
            file_pos = len;
            if buf.is_empty() { continue; }
            let lower = buf.to_lowercase();

            // ── Riven reroll / unveil ─────────────────────────────────────────
            let riven_trigger =
                lower.contains("omegarerollselection.swf") ||
                lower.contains("samodeusdioramaloaded");

            let cooldown_ok = last_riven_fire
                .map_or(true, |t| t.elapsed().as_secs() >= 4);

            if riven_trigger && cooldown_ok {
                last_riven_fire = Some(std::time::Instant::now());
                let _ = app.emit("riven-screen-open", ());
                let _ = app.emit("ff-status", "🎲 Riven screen detected");
            }

            // ── Riven screen close — card UI hidden (primary) ─────────────────
            // DiegeticArtifactCards.lua: DBG: HudVis 0 fires when the mod card
            // overlay is hidden — the most direct signal the riven screen closed.
            // Guard: only fire ≥1 s after the open trigger (so open+close in the
            // same EE.log buffer don't cancel each other out).
            if lower.contains("digeticartifactcards.lua: dbg: hudvis 0") {
                let riven_active = last_riven_fire.map_or(false, |t| {
                    let e = t.elapsed().as_secs();
                    e >= 1 && e < 600
                });
                if riven_active {
                    last_riven_fire = None;
                    let riven_log = std::env::temp_dir().join("frameforge_riven_session.txt");
                    let ts = chrono::Local::now().format("%H:%M:%S%.3f").to_string();
                    let _ = append_to_file(&riven_log, &format!(
                        "[STEP 4] CLOSE (DiegeticArtifactCards HudVis 0) — {}\n\n", ts
                    ));
                    let _ = app.emit("riven-screen-close", ());
                }
            }

            // ── Riven screen close — orbiter scene reload (fallback) ──────────
            // When the player exits the riven screen, the orbiter scene reloads
            // and creates VolumetricFog render targets. Kept as a fallback in case
            // the HudVis 0 trigger is missed.
            if lower.contains("creating render target: /ee/materials/volumetricfog") {
                let riven_active = last_riven_fire.map_or(false, |t| {
                    let e = t.elapsed().as_secs();
                    e >= 3 && e < 600
                });
                if riven_active {
                    last_riven_fire = None;
                    let riven_log = std::env::temp_dir().join("frameforge_riven_session.txt");
                    let ts = chrono::Local::now().format("%H:%M:%S%.3f").to_string();
                    let _ = append_to_file(&riven_log, &format!(
                        "[STEP 4] CLOSE (VolumetricFog render target = orbiter loaded) — {}\n\n", ts
                    ));
                    let _ = app.emit("riven-screen-close", ());
                }
            }

            // ── WFM trade whisper ─────────────────────────────────────────────
            if lower.contains("(warframe.market)") {
                let raw = buf.as_str();
                let from = raw.find("@From ").map(|i| &raw[i+6..])
                    .and_then(|s| s.split(" :").next())
                    .map(|s| s.trim().to_string()).unwrap_or_else(|| "Unknown".to_string());
                let item = { let p="want to buy "; let s=" for ";
                    raw.find(p).and_then(|i| { let r=&raw[i+p.len()..]; r.find(s).map(|j| r[..j].to_string()) })
                };
                let price: Option<u64> = raw.find(" for ").and_then(|i| {
                    let r=&raw[i+5..]; r.find(" platinum").and_then(|j| r[..j].trim().parse().ok())
                });
                let _ = app.emit("wfm-whisper", serde_json::json!({
                    "from": from, "message": raw.trim(), "item": item, "price": price,
                    "timestamp": chrono::Local::now().format("%H:%M:%S").to_string(),
                }));
            }

            // ── Relic selection screen ───────────────────────────────────────
            // Trigger: relic grid fully loaded → OCR the era from top-left quarter.
            if lower.contains("themedprojectionmanager.lua: populateinventorygrid") {
                info!("relic-pick: PopulateInventoryGrid detected — spawning OCR thread");
                let now = std::time::Instant::now();
                let should_trigger = last_relic_pick_trigger
                    .map_or(true, |t| now.duration_since(t).as_secs() >= 5);
                if should_trigger {
                    last_relic_pick_trigger = Some(now);
                    let app_clone = app.clone();
                    std::thread::spawn(move || {
                        // Brief delay for the screen to finish rendering before capture.
                        std::thread::sleep(std::time::Duration::from_millis(400));
                        let era = crate::ocr::detect_fissure_era();
                        info!("relic-pick: OCR result = {:?}", era);
                        if let Some(era) = era {
                            let payload = build_relic_pick_payload(&era, &app_clone);
                            let relic_count = payload["relics"].as_array().map_or(0, |a| a.len());
                            info!("relic-pick: emitting relic-pick-open era={} relics={}", era, relic_count);
                            // Show the overlay window from Rust — more reliable than
                            // calling win.show() from the WebView (avoids timing races).
                            relic_pick_show(&app_clone);
                            let _ = app_clone.emit("relic-pick-open", payload);
                        }
                    });
                } else {
                    info!("relic-pick: trigger suppressed by 5-second cooldown");
                }
            }
            // Dismiss: solar map regains input focus (player cancelled or mission started).
            let mapredux_dismiss = lower.contains("subscribing for /lotus/interface/mapredux.swf")
                && lower.contains("mapreduxinputfilter");
            // Candidate: entitlement service completing signals the refinement screen closed.
            let entitlement_dismiss = lower.contains("onentitlementservicecomplete false:");
            if mapredux_dismiss || entitlement_dismiss {
                let which = if entitlement_dismiss { "OnEntitlementServiceComplete" } else { "mapredux" };
                info!("relic-pick: dismiss fired ({})", which);
                relic_pick_hide(&app);
                let _ = app.emit("relic-pick-close", ());
            }

            // ── In-game trade completion ──────────────────────────────────────
            if lower.contains("dialog::createokcancel") && lower.contains("you are offering") {
                pending_trade = Some(buf.clone());
            }
            if lower.contains("the trade was successful") {
                if let Some(ref trade_raw) = pending_trade.clone() {
                    if let Some(t) = parse_trade_dialog(trade_raw) {
                        let _ = app.emit("trade-completed", serde_json::json!({
                            "sessionId":     t.session_id,
                            "withPlayer":    t.with_player,
                            "tradeType":     t.trade_type,
                            "offeredItems":  t.offered_items.iter().map(|(n, q)| serde_json::json!({"name": n, "qty": q})).collect::<Vec<_>>(),
                            "offeredPlat":   t.offered_plat,
                            "receivedItems": t.received_items.iter().map(|(n, q)| serde_json::json!({"name": n, "qty": q})).collect::<Vec<_>>(),
                            "receivedPlat":  t.received_plat,
                            "timestamp":     t.timestamp,
                        }));
                    }
                }
                pending_trade = None;
            }
        }
    });
    Ok(())
}

/// 3-state riven screen status:
///  "open"    = inventory header visible + "FITS IN" on right panel
///  "closed"  = inventory header visible + "FITS IN" gone (user exited riven screen)
///  "unknown" = inventory header not visible (alt-tabbed, or left inventory entirely)
#[tauri::command]
fn riven_screen_status() -> String {
    let riven_log = std::env::temp_dir().join("frameforge_riven_session.txt");
    let ts = chrono::Local::now().format("%H:%M:%S%.3f").to_string();

    let Ok((pixels, w, h)) = ocr::capture_warframe_pixels() else {
        let _ = append_to_file(&riven_log, &format!("[POLL {}] capture failed → unknown\n", ts));
        return "unknown".into();
    };

    let header = ocr::ocr_pixels_rect_raw(&pixels, w, h, 0.0, 0.55, 0.0, 0.10)
        .unwrap_or_default();
    let in_inventory = header.to_lowercase().contains("inventory");

    if !in_inventory {
        let _ = append_to_file(&riven_log, &format!("[POLL {}] no inventory header → unknown\n", ts));
        return "unknown".into();
    }

    let right = ocr::ocr_pixels_rect_raw(&pixels, w, h, 0.73, 1.0, 0.30, 0.80)
        .unwrap_or_default();
    let rl = right.to_lowercase();
    // In comparison mode "FITS IN" may be partially cut off, reading as "SIN", "IN", "TS IN" etc.
    // Accept any fragment that is a suffix of "FITS IN".
    let fits_in = rl.contains("fits in") || rl.contains("fits") || rl.contains("ts in")
        || rl.contains("its in") || (rl.trim() == "in") || (rl.trim() == "sin");
    let preview = right.lines().filter(|l| !l.trim().is_empty()).collect::<Vec<_>>().join(" | ");

    let status = if fits_in { "open" } else { "closed" };
    let _ = append_to_file(&riven_log, &format!(
        "[POLL {}] inventory=true fits_in={} ocr=\"{}\" → {}\n",
        ts, fits_in, truncate_chars(&preview, 80), status
    ));
    status.into()
}

/// Is the riven reroll screen still open?
/// Checks for "FITS IN" text on the right panel using RAW OCR (no preprocessing).
/// "FITS IN" is white text on dark — readable without grayscale conversion.
/// Only closes the overlay when Warframe is still focused (INVENTORY/MODS header present)
/// AND "FITS IN" is gone — so alt-tabbing away doesn't trigger a false close.
#[tauri::command]
fn riven_screen_visible() -> bool {
    let riven_log = std::env::temp_dir().join("frameforge_riven_session.txt");
    let ts = chrono::Local::now().format("%H:%M:%S%.3f").to_string();

    let Ok((pixels, w, h)) = ocr::capture_warframe_pixels() else {
        let _ = append_to_file(&riven_log, &format!("[POLL {}] capture failed → true (assume open)\n", ts));
        return true; // can't capture = can't confirm closed
    };

    // Check header (x 0–55%, y 0–10%) for "INVENTORY" — confirms Warframe is focused
    // and we're in the mods screen. If header is absent, user alt-tabbed; keep overlay.
    let header = ocr::ocr_pixels_rect_raw(&pixels, w, h, 0.0, 0.55, 0.0, 0.10)
        .unwrap_or_default();
    let in_inventory = header.to_lowercase().contains("inventory");

    if !in_inventory {
        let _ = append_to_file(&riven_log, &format!(
            "[POLL {}] no inventory header → true (alt-tabbed or different screen)\n", ts
        ));
        return true; // Warframe not in focus or wrong screen — don't close
    }

    // Check right panel (x 73–100%, y 30–80%) for "FITS IN"
    let right = ocr::ocr_pixels_rect_raw(&pixels, w, h, 0.73, 1.0, 0.30, 0.80)
        .unwrap_or_default();
    let fits_in_visible = right.to_lowercase().contains("fits");
    let right_preview = right.lines().filter(|l| !l.trim().is_empty()).collect::<Vec<_>>().join(" | ");

    let _ = append_to_file(&riven_log, &format!(
        "[POLL {}] inventory=true fits_in={} ocr=\"{}\"\n",
        ts, fits_in_visible, truncate_chars(&right_preview, 120)
    ));

    fits_in_visible
}

/// Read the single validity-flag byte that Overwolf GEP uses to track the riven reroll screen.
/// Non-zero = screen open; 0 = closed. Returns true on any error (fail-open avoids false closes).
/// The VA is found once via Pattern D-2 and cached; re-scanned only when the game restarts.
#[tauri::command]
/// Read the riven validity flag byte. Returns None if Warframe is not running.
/// Returns Some(true) = screen open, Some(false) = screen closed.
/// Fails open (Some(true)) on read errors so the overlay is never falsely dismissed.
#[cfg(target_os = "windows")]
fn read_riven_flag_byte() -> Option<bool> {
    use windows_sys::Win32::{
        Foundation::CloseHandle,
        System::{
            Diagnostics::Debug::ReadProcessMemory,
            Threading::{OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ},
        },
    };
    use std::ffi::c_void;

    let pid = memory_scanner::find_warframe_pid_pub()?;

    let cache = RIVEN_FLAG_VA.get_or_init(|| std::sync::Mutex::new(None));
    let mut cached = cache.lock().unwrap_or_else(|e| e.into_inner());
    if cached.map_or(true, |(p, _)| p != pid) {
        // Scan once per PID. Store (pid, None) if pattern not found so we don't re-scan every 200ms.
        let va = memory_scanner::find_riven_validity_va(pid);
        *cached = Some((pid, va));
    }
    let flag_va = match *cached {
        Some((_, Some(va))) => va,
        // Pattern not found for this PID — return None so the watcher ignores this tick.
        // Do NOT fail-open here: that would fire a false open event on every app start.
        Some((_, None)) | None => { return None; }
    };
    drop(cached);

    let handle = unsafe { OpenProcess(PROCESS_VM_READ | PROCESS_QUERY_INFORMATION, 0, pid) };
    if handle == 0 { return Some(true); }

    let mut byte: u8 = 0;
    let mut read = 0usize;
    let ok = unsafe {
        ReadProcessMemory(handle, flag_va as *const c_void,
            &mut byte as *mut u8 as *mut c_void, 1, &mut read)
    };
    unsafe { CloseHandle(handle); }

    if ok == 0 || read == 0 { return Some(true); } // read failed — fail open
    Some(byte != 0)
}

#[cfg(not(target_os = "windows"))]
fn read_riven_flag_byte() -> Option<bool> { None }

/// Background thread: polls the riven validity flag every 200 ms and emits
/// riven-screen-open-mem / riven-screen-close-mem on state transitions.
/// Open fires on the first non-zero reading (fast). Close requires 2 consecutive
/// zero readings (400 ms) to avoid false dismissals.
#[tauri::command]
fn start_riven_memory_watcher(app: tauri::AppHandle) {
    use std::sync::atomic::Ordering;
    if RIVEN_WATCHER_RUNNING.swap(true, Ordering::SeqCst) {
        return; // already running — don't spawn a second thread
    }
    std::thread::spawn(move || {
        let mut prev_open = false;
        let mut close_streak: u8 = 0;
        let mut warframe_was_running = false;

        loop {
            std::thread::sleep(std::time::Duration::from_millis(200));

            let pid_found = memory_scanner::find_warframe_pid_pub().is_some();
            if !pid_found {
                // Warframe not running — reset state
                if warframe_was_running {
                    prev_open = false;
                    close_streak = 0;
                    warframe_was_running = false;
                }
                continue;
            }
            warframe_was_running = true;

            match read_riven_flag_byte() {
                None => {
                    // Warframe running but pattern VA not found yet — don't change state,
                    // just wait. This avoids a false open event on app start.
                }
                Some(true) => {
                    close_streak = 0;
                    if !prev_open {
                        prev_open = true;
                        let _ = app.emit("riven-screen-open-mem", ());
                    }
                }
                Some(false) => {
                    if prev_open {
                        close_streak += 1;
                        if close_streak >= 2 {
                            prev_open = false;
                            close_streak = 0;
                            let _ = app.emit("riven-screen-close-mem", ());
                        }
                    } else {
                        close_streak = 0;
                    }
                }
            }
        }
    });
}

/// Write an error into the riven session log (called from TypeScript when OCR command fails).
#[tauri::command]
fn ocr_riven_log_error(error: String) {
    let path = std::env::temp_dir().join("frameforge_riven_session.txt");
    let ts = chrono::Local::now().format("%H:%M:%S%.3f").to_string();
    let _ = append_to_file(&path, &format!(
        "[STEP 2] OCR COMMAND FAILED — {}\n└─ Error: {}\n\n", ts, error
    ));
}

// ── Saved rivens commands ─────────────────────────────────────────────────────

#[tauri::command]
fn save_riven_roll(
    state: tauri::State<'_, AppState>,
    weapon: String, label: String, stats_json: String,
    verdict: String, score: f64,
) -> Result<String, String> {
    let conn = state.conn.lock().map_err(|e| e.to_string())?;
    let count = crate::db::count_saved_rivens(&conn).unwrap_or(0);
    if count >= 50 {
        return Err("Maximum of 50 saved rivens reached. Delete some to save more.".into());
    }
    let id = format!("{:x}", std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos());
    let saved_at = chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string();
    let riven = crate::db::SavedRiven { id: id.clone(), weapon, label, stats_json, verdict, score, saved_at };
    crate::db::save_riven(&conn, &riven).map_err(|e| e.to_string())?;
    Ok(id)
}

#[tauri::command]
fn get_saved_riven_rolls(state: tauri::State<'_, AppState>) -> Result<Vec<crate::db::SavedRiven>, String> {
    let conn = state.conn.lock().map_err(|e| e.to_string())?;
    crate::db::get_saved_rivens(&conn).map_err(|e| e.to_string())
}

#[tauri::command]
fn delete_saved_riven_roll(state: tauri::State<'_, AppState>, id: String) -> Result<(), String> {
    let conn = state.conn.lock().map_err(|e| e.to_string())?;
    crate::db::delete_saved_riven(&conn, &id).map_err(|e| e.to_string())
}

#[tauri::command]
fn rename_saved_riven_roll(state: tauri::State<'_, AppState>, id: String, label: String) -> Result<(), String> {
    let conn = state.conn.lock().map_err(|e| e.to_string())?;
    crate::db::rename_saved_riven(&conn, &id, &label).map_err(|e| e.to_string())
}

/// Return all weapon names that have riven data.
#[tauri::command]
fn get_riven_weapons() -> Vec<String> {
    let db = get_riven_db().lock().unwrap_or_else(|e| e.into_inner());
    let mut weapons: Vec<String> = db.keys().cloned().collect();
    weapons.sort();
    weapons
}

/// Reload the riven database from the Google Sheet.
#[tauri::command]
fn reload_riven_database() -> Result<usize, String> {
    let fresh = load_riven_csv_from_url()?;
    let count = fresh.len();
    *get_riven_db().lock().unwrap_or_else(|e| e.into_inner()) = fresh;
    Ok(count)
}

/// Analyse a riven roll for a given weapon.
/// positives / negatives are full stat names (e.g. "Critical Damage", "Zoom").
#[tauri::command]
fn analyze_riven(weapon: String, positives: Vec<String>, negatives: Vec<String>) -> Option<RivenAnalysis> {
    let db = get_riven_db().lock().unwrap_or_else(|e| e.into_inner());
    let key = weapon.to_lowercase();
    let entry = db.get(&key)?;

    let normalize = |s: &str| s.to_lowercase();

    // Score every "or" alternative independently — collect all results, pick best.
    let make_verdict = |s: f32, neg_ok: bool| -> String {
        match (s, neg_ok) {
            (s, true)  if s >= 0.80 => "GREAT ROLL — Consider keeping".into(),
            (s, true)  if s >= 0.60 => "GOOD ROLL — Decent for selling".into(),
            (s, _)     if s >= 0.40 => "MEDIOCRE — Keep rolling".into(),
            _                        => "BAD ROLL — Keep rolling".into(),
        }
    };
    // neg_ok = no harmful negatives rolled (i.e. rolled negs are NOT in the bad list)
    let neg_ok_pre = negatives.iter().all(|neg| {
        !entry.safe_negatives.iter().any(|s| normalize(s) == normalize(neg))
    });

    let mut all_alternatives: Vec<AlternativeResult> = Vec::new();
    let mut best_matched: Vec<String> = Vec::new();
    let mut best_missing: Vec<String> = Vec::new();
    let mut best_score: f32 = -1.0_f32;

    for (idx, alternative) in entry.stat_alternatives.iter().enumerate() {
        if alternative.is_empty() { continue; }
        let mut m: Vec<String> = Vec::new();
        let mut ms: Vec<String> = Vec::new();
        for group in alternative {
            let hit = positives.iter().find(|p| group.iter().any(|g| normalize(g) == normalize(p)));
            if let Some(stat) = hit { m.push(stat.clone()); }
            else { ms.push(group.join(" / ")); }
        }
        let s = m.len() as f32 / alternative.len() as f32;
        let label = if entry.stat_alternatives.len() == 1 {
            "Build".to_string()
        } else {
            format!("Option {}", idx + 1)
        };
        all_alternatives.push(AlternativeResult {
            label, matched: m.clone(), missing: ms.clone(),
            score: s, verdict: make_verdict(s, neg_ok_pre),
        });
        let better = s > best_score || (s == best_score && m.len() > best_matched.len());
        if better { best_score = s; best_matched = m; best_missing = ms; }
    }

    let matched = best_matched;
    let missing = best_missing;
    let score   = if best_score < 0.0 { 0.0 } else { best_score };
    let total   = entry.stat_alternatives.iter().map(|a| a.len()).min().unwrap_or(1).max(1);

    // The spreadsheet "NEGATIVE STATS" column lists HARMFUL negatives to avoid.
    // Any negative NOT in that list is safe (doesn't matter for this weapon).
    let mut safe_present: Vec<String> = Vec::new();
    let mut harmful: Vec<String> = Vec::new();
    for neg in &negatives {
        if entry.safe_negatives.iter().any(|s| normalize(s) == normalize(neg)) {
            harmful.push(neg.clone());      // listed = BAD for this weapon
        } else {
            safe_present.push(neg.clone()); // not listed = safe/irrelevant
        }
    }
    let neg_ok = harmful.is_empty();

    let verdict = match (score, neg_ok) {
        (s, true)  if s >= 0.80 => "GREAT ROLL — Consider keeping".to_string(),
        (s, true)  if s >= 0.60 => "GOOD ROLL — Decent for selling".to_string(),
        (s, _)     if s >= 0.40 => "MEDIOCRE — Keep rolling".to_string(),
        _                        => "BAD ROLL — Keep rolling".to_string(),
    };

    Some(RivenAnalysis {
        weapon: entry.weapon.clone(),
        matched_positives: matched,
        missing_positives: missing,
        safe_negatives_present: safe_present,
        harmful_negatives: harmful,
        total_wanted: total,
        score,
        verdict,
        notes: entry.notes.clone(),
        alternatives: all_alternatives,
    })
}

/// Debug: return the raw JSON from any authenticated WFM endpoint.
#[tauri::command]
fn wfm_debug_dump(state: State<AppState>, path: String) -> Result<String, String> {
    state.wfm.debug_dump(&path)
}

/// Collect known riven attribute url_names by sampling real auction listings.
/// /v1/riven/attributes was removed; this scrapes url_names from search results instead.
/// Exposed so the browser console can call: window.__wfmAttrs()
#[tauri::command]
fn wfm_get_riven_attributes(state: State<AppState>) -> Result<Vec<String>, String> {
    state.wfm.riven_attributes()
}

/// Get the internal WFM item ID for a URL slug (needed to create orders).
/// Also returns `modMaxRank` from the local WFCD item cache when the item is a mod,
/// so the frontend never needs a second network request to detect this.
#[tauri::command]
fn wfm_get_item_info(state: State<AppState>, url_name: String) -> Result<serde_json::Value, String> {
    let mut data = state.wfm.item_info(&url_name)?;

    // Enrich with modMaxRank from inventory_state_cache.json — the canonical source.
    // Match by display name since url_name ↔ unique_name conversion isn't 1:1.
    if let Some(wfm_name) = data["i18n"]["en"]["name"].as_str()
        .or_else(|| data["name"].as_str())
    {
        let wfm_name_lc = wfm_name.to_lowercase();
        let inv = load_inventory_state_cache(&state.inventory_state_cache_path);
        if let Some(max_rank) = inv.items.values()
            .find(|item| item.name.to_lowercase() == wfm_name_lc)
            .and_then(|item| item.mod_max_rank)
        {
            data["modMaxRank"] = serde_json::json!(max_rank);
        }
    }

    Ok(data)
}

/// Create a new buy or sell order. `mod_rank` must be set for mods — WFM returns 400 without it.
#[tauri::command]
fn wfm_create_order(state: State<AppState>, item_id: String, order_type: String, platinum: u32, quantity: u32, visible: bool, mod_rank: Option<u32>) -> Result<serde_json::Value, String> {
    state.wfm.create_order(&item_id, &order_type, platinum, quantity, visible, mod_rank)
}

/// Update an existing order's price, quantity, or visibility.
#[tauri::command]
fn wfm_update_order(state: State<AppState>, order_id: String, platinum: u32, quantity: u32, visible: bool) -> Result<serde_json::Value, String> {
    state.wfm.update_order(&order_id, platinum, quantity, visible)
}

/// Delete an order.
#[tauri::command]
fn wfm_delete_order(state: State<AppState>, order_id: String) -> Result<(), String> {
    state.wfm.delete_order(&order_id)
}

/// Post a revealed riven as an auction on warframe.market.
#[tauri::command]
fn wfm_create_riven_auction(
    state: State<AppState>,
    weapon_url_name: String,
    riven_name: String,
    mastery_level: u32,
    mod_rank: u8,
    re_rolls: u32,
    polarity: String,
    attributes: Vec<WfmRivenAttribute>,
    starting_price: u32,
    buyout_price: Option<u32>,
    minimal_reputation: u32,
    note: String,
    visible: bool,
    is_direct_sell: bool,
) -> Result<serde_json::Value, String> {
    let json = state.wfm.create_riven_auction(
        &weapon_url_name, &riven_name, mastery_level, mod_rank, re_rolls, &polarity,
        &attributes, starting_price, buyout_price, minimal_reputation, &note, visible, is_direct_sell,
    )?;
    record_new_auction_id(&state, &json);
    Ok(json)
}

/// Fetch the current user's active riven auctions from warframe.market.
/// Tries v2 /auctions/my first (returns all including hidden); falls back to the v1 profile
/// endpoint which only returns visible auctions.
#[tauri::command]
async fn wfm_get_my_riven_auctions(state: tauri::State<'_, AppState>) -> Result<serde_json::Value, String> {
    let stored_ids = state.auction_ids.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let wfm = state.wfm.clone();
    tauri::async_runtime::spawn_blocking(move || wfm.my_riven_auctions(&stored_ids))
        .await
        .map_err(|e| e.to_string())?
}

fn save_auction_ids(state: &State<AppState>) {
    let ids = state.auction_ids.lock().unwrap_or_else(|e| e.into_inner()).clone();
    if let Ok(json) = serde_json::to_string(&ids) {
        let _ = atomic_write(&state.auction_ids_path, json.as_bytes());
    }
}

/// Record a newly created auction's id so hidden auctions survive restarts.
/// FrameForge-created auctions can be hidden, and the WFM profile endpoint only
/// lists visible ones, so their ids are the only way to fetch them back.
fn record_new_auction_id(state: &State<AppState>, json: &serde_json::Value) {
    if let Some(id) = json["payload"]["auction"]["id"].as_str() {
        let mut ids = state.auction_ids.lock().unwrap_or_else(|e| e.into_inner());
        if !ids.contains(&id.to_string()) {
            ids.push(id.to_string());
            drop(ids);
            save_auction_ids(state);
        }
    }
}

/// Switch a riven auction between Auction and Direct Sale types.
/// The close-and-recreate lives in `Wfm`; here we reconcile the stored auction
/// ids — drop the closed one, record its replacement.
#[tauri::command]
fn wfm_switch_riven_type(
    state: State<AppState>,
    auction_id: String,
    new_is_direct_sell: bool,
    starting_price: u32,
    buyout_price: Option<u32>,
    visible: bool,
) -> Result<serde_json::Value, String> {
    let json = state.wfm.switch_riven_type(&auction_id, new_is_direct_sell, starting_price, buyout_price, visible)?;
    // The old listing is now closed; drop its id and record the replacement.
    state.auction_ids.lock().unwrap_or_else(|e| e.into_inner()).retain(|id| id != &auction_id);
    save_auction_ids(&state);
    record_new_auction_id(&state, &json);
    Ok(json)
}

/// Delete a riven auction via the /close endpoint.
#[tauri::command]
fn wfm_delete_auction(state: State<AppState>, auction_id: String) -> Result<(), String> {
    state.wfm.delete_auction(&auction_id)?;
    state.auction_ids.lock().unwrap_or_else(|e| e.into_inner()).retain(|id| id != &auction_id);
    save_auction_ids(&state);
    Ok(())
}

/// Update a riven auction's starting price, buyout price, and visibility.
/// Sends PUT /v1/auctions/entry/{id}. Pass buyout_price=None to clear the buyout.
#[tauri::command]
fn wfm_update_auction(state: State<AppState>, auction_id: String, starting_price: u32, buyout_price: Option<u32>, visible: bool) -> Result<(), String> {
    state.wfm.update_auction(&auction_id, starting_price, buyout_price, visible)
}

/// Toggle visibility of a riven auction (visible / hidden).
#[tauri::command]
fn wfm_set_auction_visible(state: State<AppState>, auction_id: String, visible: bool) -> Result<(), String> {
    state.wfm.set_auction_visible(&auction_id, visible)
}

/// Fetch warframe.market item list using v2 API (v1 /items returns 404).
#[tauri::command]
fn fetch_wfm_items(state: State<AppState>) -> Result<Vec<WfmItem>, String> {
    state.wfm.items()
}

/// Fetch 48-hour median sell price for a single item from warframe.market.
/// Tries the slug as-is first, then retries with the Blueprint suffix added or
/// removed — WFM is inconsistent about whether component blueprints include it.
#[tauri::command]
fn fetch_wfm_price(state: State<AppState>, url_name: String) -> Result<WfmPrice, String> {
    // A lookup that errors and one that finds no listing both surface the same way
    // to the UI — no price — so `price_with_fallback` collapsing both to None is fine.
    let sell_median = state.wfm.price_with_fallback(&url_name).map(|p| p as f64);
    Ok(WfmPrice { url_name, sell_median, buy_median: None })
}

/// Fetch the 48-hour median sell price for an item by display name.
/// Results are cached in AppState so the overlay and main window share them.
/// Returns None when the item is not listed on warframe.market.
#[tauri::command]
fn get_item_price(item_name: String, state: State<AppState>) -> Result<Option<u32>, String> {
    // 1. Check relics.run bulk price cache (no network call needed)
    {
        let prices = state.relics_run_prices.lock().map_err(|e| e.to_string())?;
        let key = item_name.to_lowercase();
        if let Some(&price) = prices.get(&key) {
            return Ok(Some(price));
        }
    }

    let slug = to_wfm_slug(&item_name);

    if let Some(cached) = state.wfm.cached_price(&slug) {
        return Ok(cached);
    }

    // Only strip "_blueprint" here — never append it. This is called with inventory
    // display names, where a prime component's name carries "Blueprint" but WFM lists
    // it without the suffix. A non-blueprint name must NOT fall back to a _blueprint
    // slug, or a frame would be priced as its blueprint.
    let price = state.wfm.price_for_slug(&slug)?.or_else(|| {
        slug.strip_suffix("_blueprint")
            .and_then(|base| state.wfm.price_for_slug(base).unwrap_or(None))
    });
    state.wfm.cache_price(slug, price);

    // Persist WFM price into the inventory cache file so it survives restarts.
    // Only write for tradeable items: prime parts/blueprints (have ducats) and mods/arcanes.
    if let Some(plat) = price {
        let cache_path = &state.inventory_state_cache_path;
        let mut inv = load_inventory_state_cache(cache_path);
        let items = state.wfcd_items.lock().map_err(|e| e.to_string())?;
        // Key the cache entry on the canonical unique_name, not the display string.
        let unique = ItemResolver::from_items(&items)
            .by_display(&item_name)
            .map(|r| r.unique_name.clone());
        if let Some(item) = unique.and_then(|u| items.iter().find(|i| i.unique_name == u)) {
            let cat = fix_category(&item.name, &item.item_type, &item.product_category, &item.category, &item.unique_name);
            let tradeable = item.ducats.is_some() || matches!(cat.as_str(), "Mods" | "Arcanes");
            if tradeable {
                inv.items.entry(item.unique_name.clone())
                    .or_insert_with(|| CachedItem { unique_name: item.unique_name.clone(), ..Default::default() })
                    .wfm_price = Some(plat);
                if let Ok(json) = serde_json::to_string(&inv) {
                    let _ = atomic_write(cache_path, json.as_bytes());
                }
            }
        }
    }

    Ok(price)
}

// ─── WFM price queue ──────────────────────────────────────────────────────────
// All warframe.market price fetches are routed through a single background
// thread that enforces the ≤3 req/sec rate limit globally. The frontend enqueues
// slugs via wfm_queue_prices / wfm_queue_price_priority and listens for
// "wfm-price-update" events instead of calling fetch_wfm_price directly.

#[derive(serde::Serialize, Clone)]
struct WfmPriceUpdate {
    url_name:     String,
    sell_median:  Option<u32>,
    tradeable:    bool,
}

/// Start the WFM price queue drain thread (no-op if already running).
/// Must be called after fetch_item_list so wfcd_items is populated.
#[tauri::command]
fn start_wfm_queue(app: tauri::AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    if state.wfm_queue_started.swap(true, Ordering::SeqCst) {
        return Ok(());
    }

    // Pre-populate the in-memory price cache from inventory_state_cache.json so that
    // wfm_get_cached_prices() returns previously-fetched prices immediately on startup
    // and the queue drain skips slugs that already have a fresh price.
    {
        let disk = load_inventory_state_cache(&state.inventory_state_cache_path);
        for item in disk.items.values() {
            if !item.name.is_empty() {
                let slug = to_wfm_slug(&item.name);
                if !slug.is_empty() {
                    // Only insert if we have a price; None entries are kept absent so they get re-queued.
                    if let Some(p) = item.wfm_price {
                        state.wfm.cache_price(slug, Some(p));
                    }
                }
            }
        }
    }

    // Build slug → unique_name + tradeable map from a snapshot of wfcd_items.
    // Items are loaded once and the thread keeps this snapshot (items rarely change).
    let slug_map: HashMap<String, (String, bool)> = {
        let items = state.wfcd_items.lock().unwrap_or_else(|e| e.into_inner());
        let resolver = ItemResolver::from_items(&items);
        let mut m = HashMap::new();
        for item in items.iter() {
            let cat = fix_category(&item.name, &item.item_type, &item.product_category, &item.category, &item.unique_name);
            let tradeable = item.ducats.is_some() || matches!(cat.as_str(), "Mods" | "Arcanes");
            if !tradeable { continue; }
            let Some(resolved) = resolver.by_unique(&item.unique_name) else { continue };
            for slug in resolver::slug_variants(&resolved.slug) {
                m.insert(slug, (item.unique_name.clone(), true));
            }
        }
        m
    };

    let queue          = state.wfm_price_queue.clone();
    let priority_queue = state.wfm_priority_queue.clone();
    let wfm            = state.wfm.clone();
    let cache_path     = state.inventory_state_cache_path.clone();

    std::thread::spawn(move || {
        loop {
            // Priority queue drains first; fall back to normal queue.
            let slug = {
                let mut pq = priority_queue.lock().unwrap_or_else(|e| e.into_inner());
                pq.pop_front()
            }.or_else(|| {
                let mut q = queue.lock().unwrap_or_else(|e| e.into_inner());
                q.pop_front()
            });

            let slug = match slug {
                Some(s) => s,
                None => {
                    std::thread::sleep(std::time::Duration::from_millis(200));
                    continue;
                }
            };

            // Skip if already cached (avoid redundant API calls within a session).
            if wfm.is_price_cached(&slug) { continue; }

            // Fetch — the rate limiter inside enforces the 3 req/sec limit.
            let price = wfm.price_with_fallback(&slug);
            let tradeable = price.is_some();

            // Update in-memory cache.
            wfm.cache_price(slug.clone(), price);

            // Write price + tradeable_wfm into inventory_state_cache.json if we know the item.
            if let Some((unique_name, _)) = slug_map.get(&slug) {
                let mut inv = load_inventory_state_cache(&cache_path);
                let entry = inv.items.entry(unique_name.clone())
                    .or_insert_with(|| CachedItem { unique_name: unique_name.clone(), ..Default::default() });
                entry.wfm_price     = price;
                entry.tradeable_wfm = tradeable;
                if let Ok(json) = serde_json::to_string(&inv) {
                    let _ = atomic_write(&cache_path, json.as_bytes());
                }
            }

            // Notify the frontend.
            let _ = app.emit("wfm-price-update", WfmPriceUpdate {
                url_name: slug, sell_median: price, tradeable,
            });
        }
    });

    Ok(())
}

/// Add slugs to the normal-priority WFM price queue.
/// Slugs already cached in-memory are silently skipped.
#[tauri::command]
fn wfm_queue_prices(state: State<'_, AppState>, url_names: Vec<String>) {
    let mut q = state.wfm_price_queue.lock().unwrap_or_else(|e| e.into_inner());
    // Snapshot existing queue entries to deduplicate without holding a borrow during push_back.
    let already_queued: std::collections::HashSet<String> = q.iter().cloned().collect();
    for slug in url_names {
        if !state.wfm.is_price_cached(&slug) && !already_queued.contains(&slug) {
            q.push_back(slug);
        }
    }
}

/// Push a single slug to the front of the priority queue (for popup / on-demand fetches).
/// Forces a fresh fetch even if cached.
#[tauri::command]
fn wfm_queue_price_priority(state: State<'_, AppState>, url_name: String) {
    // Remove any existing cached entry so the drain thread fetches fresh.
    state.wfm.uncache_price(&url_name);
    state.wfm_priority_queue.lock().unwrap_or_else(|e| e.into_inner())
        .push_front(url_name);
}

/// Return the current in-memory WFM price cache (slug → price).
/// Frontend calls this on startup to populate prices without waiting for the queue.
#[tauri::command]
fn wfm_get_cached_prices(state: State<'_, AppState>) -> HashMap<String, Option<u32>> {
    state.wfm.cached_prices()
}


// ─── Change log ───────────────────────────────────────────────────────────────

#[tauri::command]
fn get_change_log(state: State<AppState>, limit: i64) -> Result<Vec<QuantityChange>, String> {
    let conn = state.conn.lock().map_err(|e| e.to_string())?;
    db::get_quantity_changes(&conn, limit).map_err(|e| e.to_string())
}

// ─── Tracked items / snapshots ───────────────────────────────────────────────

#[tauri::command]
fn get_tracked_items(state: State<AppState>) -> Result<Vec<TrackedItem>, String> {
    let conn = state.conn.lock().map_err(|e| e.to_string())?;
    db::get_tracked_items(&conn).map_err(|e| e.to_string())
}

#[tauri::command]
fn add_tracked_item(state: State<AppState>, unique_name: String, display_name: String) -> Result<(), String> {
    let conn = state.conn.lock().map_err(|e| e.to_string())?;
    db::add_tracked_item(&conn, &unique_name, &display_name).map_err(|e| e.to_string())
}

#[tauri::command]
fn remove_tracked_item(state: State<AppState>, unique_name: String) -> Result<(), String> {
    let conn = state.conn.lock().map_err(|e| e.to_string())?;
    db::remove_tracked_item(&conn, &unique_name).map_err(|e| e.to_string())
}

#[tauri::command]
fn get_item_snapshots(state: State<AppState>, unique_name: String, days: Option<u32>) -> Result<Vec<SnapshotPoint>, String> {
    let conn = state.conn.lock().map_err(|e| e.to_string())?;
    db::get_snapshots(&conn, &unique_name, days).map_err(|e| e.to_string())
}

// ─── Trade log ────────────────────────────────────────────────────────────────

#[tauri::command]
fn get_trades(state: State<AppState>) -> Result<Vec<Trade>, String> {
    let conn = state.conn.lock().map_err(|e| e.to_string())?;
    db::get_trades(&conn).map_err(|e| e.to_string())
}

#[tauri::command]
fn add_trade(
    state: State<AppState>,
    with_player: String,
    direction: String,
    item_name: String,
    item_url: String,
    quantity: i64,
    platinum: i64,
    source: String,
    notes: String,
    session_id: Option<String>,
    trade_type: Option<String>,
    timestamp: Option<String>,
) -> Result<i64, String> {
    let trade = Trade {
        id: 0,
        timestamp: timestamp.unwrap_or_else(|| chrono::Utc::now().to_rfc3339()),
        with_player,
        direction,
        item_name,
        item_url,
        quantity,
        platinum,
        source,
        notes,
        session_id: session_id.unwrap_or_default(),
        trade_type: trade_type.unwrap_or_default(),
    };
    let conn = state.conn.lock().map_err(|e| e.to_string())?;
    db::add_trade(&conn, &trade).map_err(|e| e.to_string())
}

#[tauri::command]
fn delete_trade(state: State<AppState>, id: i64) -> Result<(), String> {
    let conn = state.conn.lock().map_err(|e| e.to_string())?;
    db::delete_trade(&conn, id).map_err(|e| e.to_string())
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

/// Hard-exit the process. Called from the frontend close handler when destroy()
/// is unreliable (e.g. after a Promise.race timeout on a hanging WFM API call).
#[tauri::command]
fn force_quit() {
    std::process::exit(0);
}

#[tauri::command]
fn load_settings(state: State<AppState>) -> String {
    std::fs::read_to_string(&state.settings_path).unwrap_or_default()
}

// settings.json is written from several threads (the save_settings command,
// window-event handlers on every move/resize). Every writer must go through
// merge_settings; an unserialized or non-atomic write used to tear the file
// and wipe all settings on the next merge.
static SETTINGS_WRITE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn read_settings_map(path: &std::path::Path) -> Result<serde_json::Map<String, serde_json::Value>, String> {
    let raw = std::fs::read_to_string(path).unwrap_or_default();
    if raw.trim().is_empty() {
        return Ok(serde_json::Map::new());
    }
    match serde_json::from_str::<serde_json::Value>(&raw) {
        Ok(serde_json::Value::Object(m)) => Ok(m),
        _ => Err(format!("{} exists but is not a valid JSON object; refusing to overwrite it", path.display())),
    }
}

fn merge_settings(path: &std::path::Path, apply: impl FnOnce(&mut serde_json::Map<String, serde_json::Value>)) -> Result<(), String> {
    // Poison recovery is safe: the lock guards the file, not the map, and a
    // panicking closure bails before the write, leaving the file untouched.
    let _guard = SETTINGS_WRITE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut map = read_settings_map(path)?;
    apply(&mut map);
    atomic_write(path, serde_json::Value::Object(map).to_string().as_bytes()).map_err(|e| e.to_string())
}

#[tauri::command]
fn save_settings(app: tauri::AppHandle, state: State<AppState>, json: String) -> Result<(), String> {
    // Merge over existing file so geometry fields written by save_window_state are never erased
    let new_vals: serde_json::Value = serde_json::from_str(&json).map_err(|e| e.to_string())?;
    merge_settings(&state.settings_path, |existing| {
        if let serde_json::Value::Object(new_map) = new_vals {
            for (k, v) in new_map { existing.insert(k, v); }
        }
    })?;
    app.emit("settings-updated", ()).ok();
    Ok(())
}

#[tauri::command]
fn read_scan_log(state: State<AppState>) -> Result<String, String> {
    std::fs::read_to_string(&state.log_path).map_err(|e| e.to_string())
}

#[derive(serde::Deserialize)]
pub struct ApiChange {
    pub item_name: String,
    pub old_qty: i64,
    pub new_qty: i64,
}

#[tauri::command]
fn log_api_changes(state: State<AppState>, changes: Vec<ApiChange>) -> Result<(), String> {
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

#[tauri::command]
async fn dump_memory_probe(state: State<'_, AppState>) -> Result<String, String> {
    let log_path = state.memory_probe_path.clone();
    let lines = tokio::task::spawn_blocking(|| {
        memory_scanner::dump_inventory_regions(40)
    }).await.map_err(|e| e.to_string())?;
    let output = lines.join("\n");
    std::fs::write(&log_path, &output).map_err(|e| e.to_string())?;
    Ok(output)
}

/// Enable or disable automatic per-pass inventory blob logging to blobs/.
#[tauri::command]
fn set_blob_log(enabled: bool, state: State<'_, AppState>) {
    state.blob_log_enabled.store(enabled, Ordering::SeqCst);
}

/// Enable or disable logging of raw DE API responses to api_logs/.
#[tauri::command]
fn set_api_log(enabled: bool, state: State<'_, AppState>) {
    state.api_log_enabled.store(enabled, Ordering::SeqCst);
}

/// Returns "started" or "stopped" so the frontend can update button state.
#[tauri::command]
async fn toggle_raw_scan(state: State<'_, AppState>) -> Result<String, String> {
    let was_active = state.raw_scan_active.swap(true, Ordering::SeqCst);
    if was_active {
        // Already running — stop it
        state.raw_scan_active.store(false, Ordering::SeqCst);
        return Ok("stopped".to_string());
    }

    // Freshly started — truncate the output file and spawn the loop
    let out_path  = state.raw_scan_path.clone();
    let flag      = state.raw_scan_active.clone();

    // Truncate / create the file now so the frontend can see it immediately
    std::fs::write(&out_path, "").map_err(|e| e.to_string())?;

    std::thread::spawn(move || {
        let mut pass = 0u32;
        while flag.load(Ordering::SeqCst) {
            pass += 1;
            let ts = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
            let header = format!("\n=== Pass {} at {} ===\n", pass, ts);

            // Open for append each pass so file grows in real time
            match std::fs::OpenOptions::new().create(true).append(true).open(&out_path) {
                Ok(mut f) => {
                    use std::io::Write;
                    let _ = f.write_all(header.as_bytes());
                    match memory_scanner::raw_scan_pass(&mut f) {
                        Ok(n)  => { let _ = writeln!(f, "--- pass {} done: {} strings ---", pass, n); }
                        Err(e) => { let _ = writeln!(f, "--- pass {} error: {} ---", pass, e); }
                    }
                }
                Err(e) => { warn!(error = %e, "raw_scan open failed"); }
            }

            // Sleep between passes so the user has time to navigate menus
            for _ in 0..50 {
                if !flag.load(Ordering::SeqCst) { break; }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        }
    });

    Ok("started".to_string())
}

#[tauri::command]
fn clear_cache(state: State<AppState>) -> Result<(), String> {
    // Clear change log from DB
    let conn = state.conn.lock().map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM quantity_changes", []).map_err(|e| e.to_string())?;
    drop(conn);

    // Reset all in-memory inventory state
    state.current_quantities.lock().map_err(|e| e.to_string())?.clear();
    state.unique_quantities.lock().map_err(|e| e.to_string())?.clear();
    state.current_mods.lock().map_err(|e| e.to_string())?.clear();
    state.api_quantities_cache.lock().map_err(|e| e.to_string())?.clear();
    state.api_mod_copies_cache.lock().map_err(|e| e.to_string())?.clear();

    // Delete cache and hint files so nothing reloads on next start
    let _ = std::fs::remove_file(&state.quantities_cache_path);
    let _ = std::fs::remove_file(&state.inventory_state_cache_path);
    let _ = std::fs::remove_file(state.log_path.with_file_name("inventory_hints.json"));
    let _ = std::fs::remove_file(state.log_path.with_file_name("mod_hints.json"));

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
pub struct BlobStatusPayload {
    pub stage:   String,  // "scanning" | "done" | "error"
    pub detail:  String,  // human-readable detail
}

// ── Relic pick overlay ────────────────────────────────────────────────────────

fn relic_pick_show(app: &tauri::AppHandle) {
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
    let _ = win.set_position(tauri::Position::Logical(tauri::LogicalPosition { x, y: 20.0 }));
    let _ = win.show();
}

fn relic_pick_hide(app: &tauri::AppHandle) {
    use tauri::Manager;
    let Some(win) = app.get_webview_window("relic-pick-overlay") else { return };
    let _ = win.hide();
}

/// Debug: run OCR on the top-left quarter of the Warframe window and report the detected era.
#[tauri::command]
fn debug_detect_fissure_era() -> String {
    match crate::ocr::detect_fissure_era() {
        Some(era) => format!("Detected era: {}", era),
        None => "No era detected — OCR found no known fissure era label in the top-left quarter.".to_string(),
    }
}

/// Debug: manually fire the relic pick overlay for a given era.
#[tauri::command]
fn test_relic_pick_overlay(era: String, app: tauri::AppHandle) -> String {
    let payload = build_relic_pick_payload(&era, &app);
    let relic_count = payload["relics"].as_array().map_or(0, |a| a.len());
    relic_pick_show(&app);
    let _ = app.emit("relic-pick-open", &payload);
    format!("Emitted relic-pick-open: era={}, {} relics in inventory", era, relic_count)
}

/// Debug: return the last ~4 KB of EE.log so we can see what strings appear when
/// opening the relic selection screen. Call this immediately after opening the screen.
#[tauri::command]
fn debug_ee_log_tail() -> String {
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


fn build_relic_pick_payload(era: &str, app: &tauri::AppHandle) -> serde_json::Value {
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
            .map_or(false, |&q| q > 0);
        if direct { return true; }
        comp_to_parents.get(&key).map_or(false, |parents| {
            parents.iter().any(|p| {
                name_to_unique.get(&p.to_lowercase())
                    .and_then(|uname| quantities.get(uname))
                    .map_or(false, |&q| q > 0)
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

#[derive(serde::Serialize, Clone)]
pub struct InventoryUpdate {
    pub quantities: HashMap<String, i64>,
    pub crafting: Vec<CraftingJob>,
    pub mastery_rank: Option<u32>,
    pub mastery_data: HashMap<String, u32>,
    pub changes: Vec<QuantityChange>,
    pub warframe_running: bool,
    pub scanned_at: i64,
    /// Warframe unique-name paths from InfestedFoundry.ConsumedSuits (Helminth subsumed).
    /// Non-empty only when the memory scanner found the ConsumedSuits array this window.
    pub consumed_suits: Vec<String>,
    /// Mod/arcane inventory: unique_name → {total, by_rank}.
    /// Empty when no scan data available yet; scanner-sourced until API provides rank detail.
    pub mods: HashMap<String, memory_scanner::ModCount>,
    /// Warframe unique-name → socketed Archon Shards read from memory.
    /// Only populated for warframes where ArchonCrystalUpgrades was found.
    pub socketed_shards: HashMap<String, Vec<memory_scanner::ArchonShard>>,
    /// Item unique-name → number of Forma applied (polarized count from blob).
    /// Only populated for items that have at least one Forma applied.
    pub forma_counts: HashMap<String, u32>,
    /// True only on the end-of-full-pass emit. Frontend should REPLACE archonShards
    /// state instead of merging so stale entries are cleaned up.
    pub is_full_pass: bool,
    /// Local Warframe account name ("Logged in NAME" from EE.log). None until detected.
    pub player_name: Option<String>,
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
    let mut unique_names: Vec<String> = items.iter().map(|i| i.unique_name.clone()).collect();
    let mut display_names: Vec<String> = items.iter().map(|i| i.name.clone()).collect();
    // Virtual catalog entries for currency fields not present in WFCD.
    for (path, name) in [
        ("/_currency/Endo",        "Endo"),
        ("/_currency/Credits",     "Credits"),
        ("/_currency/Platinum",    "Platinum"),
        ("/_currency/PlatinumGift","Platinum (Gift)"),
    ] {
        unique_names.push(path.to_string());
        display_names.push(name.to_string());
    }
    // Items that share a game path with a canonical counterpart (dual-body warframes,
    // renamed items, etc.).  Map  secondary_path → primary_path.
    // The scanner searches for ALL paths, but stores results under the primary so the
    // inventory shows one entry with the canonical display name.
    let path_aliases: HashMap<&str, &str> = [
        // Sirius & Orion: two WFCD entries for one warframe.
        // "Orion & Sirius" (OrionSuit) is the alternate; "Sirius & Orion" (SiriusSuit) is canonical.
        ("/Lotus/Powersuits/SiriusOrion/OrionSuit",
         "/Lotus/Powersuits/SiriusOrion/SiriusSuit"),
        // Blueprint has the same duplication — Orion & Sirius Blueprint → Sirius & Orion Blueprint.
        ("/Lotus/Powersuits/SiriusOrion/OrionSuitBlueprint",
         "/Lotus/Types/Recipes/WarframeRecipes/SiriusOrionBlueprint"),
    ].into_iter().collect();

    // Alias keys (secondary paths) are excluded from the inventory cache entirely —
    // they would show as phantom zero-quantity duplicates of the canonical entry.
    let mut alias_excluded: std::collections::HashSet<String> =
        path_aliases.keys().map(|s| s.to_string()).collect();

    // Build path→name and path→ducat lookups once from the catalog snapshot.
    // Alternate paths in path_aliases resolve to the canonical name.
    let mut path_to_name: HashMap<String, String> = unique_names.iter().zip(display_names.iter())
        .map(|(u, d)| (u.clone(), d.clone()))
        .collect();
    for (alt, primary) in &path_aliases {
        if let Some(name) = path_to_name.get(*primary).cloned() {
            path_to_name.insert(alt.to_string(), name);
        }
    }
    let path_to_ducat: HashMap<String, u32> = items.iter()
        .filter_map(|i| i.ducats.map(|d| (i.unique_name.clone(), d)))
        .collect();
    let path_to_vaulted: HashMap<String, bool> = items.iter()
        .filter_map(|i| i.vaulted.map(|v| (i.unique_name.clone(), v)))
        .collect();
    // Owned maps for debug capture — cloned once, no borrow from `items`.
    let path_to_item_type: HashMap<String, String> = items.iter()
        .map(|i| (i.unique_name.clone(), i.item_type.clone())).collect();
    let path_to_product_category: HashMap<String, String> = items.iter()
        .map(|i| (i.unique_name.clone(), i.product_category.clone())).collect();
    let path_to_wfcd_cat: HashMap<String, String> = items.iter()
        .map(|i| (i.unique_name.clone(), i.category.clone())).collect();
    let mut path_to_category: HashMap<String, String> = items.iter()
        .map(|i| (i.unique_name.clone(), fix_category(&i.name, &i.item_type, &i.product_category, &i.category, &i.unique_name)))
        .collect();
    for (path, name) in [
        ("/_currency/Endo",        "Endo"),
        ("/_currency/Credits",     "Credits"),
        ("/_currency/Platinum",    "Platinum"),
        ("/_currency/PlatinumGift","Platinum (Gift)"),
    ] {
        path_to_name.insert(path.to_string(), name.to_string());
        path_to_category.insert(path.to_string(), "Miscellaneous".to_string());
    }

    // ── Apply corrections to path lookups ─────────────────────────────────────
    let ignored_paths: std::collections::HashSet<String> = state.corrections.iter()
        .filter(|(_, c)| c.category.as_deref() == Some("Ignored"))
        .map(|(path, _)| path.clone())
        .collect();
    for p in &ignored_paths {
        path_to_name.remove(p);
        path_to_category.remove(p);
    }
    for (path, c) in &state.corrections {
        if ignored_paths.contains(path) { continue; }
        if let Some(ref name) = c.name {
            if !name.is_empty() { path_to_name.insert(path.clone(), name.clone()); }
        }
        if let Some(ref cat) = c.category {
            path_to_category.insert(path.clone(), cat.clone());
        }
    }
    // Ignored paths are suppressed from the inventory cache just like alias secondaries.
    alias_excluded.extend(ignored_paths.iter().cloned());

    let relic_drops_snapshot: HashMap<String, Vec<String>> =
        state.relic_drops.lock().unwrap_or_else(|e| e.into_inner()).clone();

    let flag = state.monitor_active.clone();
    let db_path = state.db_path.clone();
    let inventory_state_cache_path = state.inventory_state_cache_path.clone();
    let shared_quantities    = state.current_quantities.clone();
    let shared_unique        = state.unique_quantities.clone();
    let shared_mods          = state.current_mods.clone();
    let shared_crafting      = state.current_crafting.clone();
    let blob_log_enabled     = state.blob_log_enabled.clone();
    let blob_log_dir         = state.blob_log_dir.clone();
    let debug_cat_enabled    = state.debug_cat_enabled.clone();
    let auto_capture_dir     = state.auto_capture_dir.clone();
    let unmatched_paths_dir  = state.unmatched_paths_dir.clone();
    let reward_app = app.clone();  // clone before app is moved into the inventory thread

    // Channel for the blob capture thread to deliver a parsed BlobInventory to the monitor loop.
    let (blob_tx, blob_rx) = std::sync::mpsc::channel::<memory_scanner::BlobInventory>();

    std::thread::spawn(move || {
        let conn = match rusqlite::Connection::open(&db_path) {
            Ok(c) => c,
            Err(e) => { error!(error = %e, "monitor DB open failed"); return; }
        };
        let _ = conn.execute_batch("PRAGMA journal_mode=WAL;");

        // Start from whatever quantities were last known (survives restarts).
        let mut known: HashMap<String, i64> =
            shared_quantities.lock().unwrap_or_else(|e| e.into_inner()).clone();

        // Load the full inventory state from the last session so the UI shows data
        // immediately on restart without waiting for the first full scan pass.
        let startup_cache = load_inventory_state_cache(&inventory_state_cache_path);

        // Pre-populate known with cached resource quantities so that per-cycle hint
        // emits never replace the frontend display with a partial inventory.
        // is_stackable overrides is_unique_path: Kubrow Eggs, Kavat Genetic Codes,
        // cosmetics, and Railjack weapons share path prefixes with actual unique items
        // but have counts > 1 from MiscItems/FlavourItems — they must go into known.
        for (path, item) in &startup_cache.items {
            if item.amount > 0 && item.mod_ranks.is_none()
                && (item.is_stackable || !is_unique_path(path))
            {
                known.entry(path.clone()).or_insert(item.amount as i64);
            }
        }
        // Keep shared_quantities in sync so the cache-clear detector doesn't misfire.
        {
            let mut q = shared_quantities.lock().unwrap_or_else(|e| e.into_inner());
            if q.is_empty() && !known.is_empty() { *q = known.clone(); }
        }

        // Stability buffer for unique scanner items (weapons/warframes).
        // Pre-seed confirmed items at count=4 so they show immediately on restart.
        // Exclude is_stackable items — they are seeded into known above, not here.
        let mut unique_stable: HashMap<String, u8> = startup_cache.items.iter()
            .filter(|(k, v)| v.mod_ranks.is_none() && v.amount > 0 && !v.subsumed
                          && !v.is_stackable && is_unique_path(k))
            .map(|(k, _)| (k.clone(), 4u8))
            .collect();
        let mut confirmed_unique: std::collections::HashSet<String> =
            unique_stable.keys().cloned().collect();

        // Mods: commit hint results directly on every partial pass.
        // The hint is the live inventory-root region and is always authoritative.
        // No stability buffer needed — wrong counts on a bad scan self-correct next pass.
        // Pre-seed from startup cache so mods/arcanes show immediately on restart instead
        // of going blank until the hint scan rediscovers the RawUpgrades region.
        let mut known_mods: HashMap<String, memory_scanner::ModCount> = {
            let from_shared = shared_mods.lock().unwrap_or_else(|e| e.into_inner()).clone();
            if !from_shared.is_empty() {
                from_shared
            } else {
                startup_cache.items.iter()
                    .filter(|(_, v)| v.mod_ranks.is_some())
                    .map(|(path, v)| {
                        let by_rank: HashMap<u8, i64> = v.mod_ranks.as_ref()
                            .map(|ranks| ranks.iter()
                                .filter_map(|(r, &c)| r.parse::<u8>().ok().map(|rank| (rank, c)))
                                .collect())
                            .unwrap_or_default();
                        let total = by_rank.values().sum();
                        (path.clone(), memory_scanner::ModCount { total, by_rank })
                    })
                    .collect()
            }
        };
        // Track the last date we recorded daily snapshots (YYYY-MM-DD).
        // Initialise to yesterday so the first scan of a new day always fires.
        let mut last_snapshot_date = String::new();

        // Emit an immediate status before the first scan so the UI shows cached
        // inventory data without waiting for the scan to finish.
        {
            let game_found = memory_scanner::find_warframe_pid_pub().is_some();
            let now_pre = chrono::Utc::now().timestamp();
            let mut initial_qty = known.clone();
            for k in unique_stable.keys() { initial_qty.entry(k.clone()).or_insert(1); }
            for (path, mc) in &known_mods { initial_qty.entry(path.clone()).or_insert(mc.total); }
            let _ = app.emit("inventory-update", InventoryUpdate {
                quantities: initial_qty,
                crafting: vec![],
                mastery_rank: startup_cache.mastery_rank,
                mastery_data: startup_cache.items.iter()
                    .filter(|(_, v)| v.mastery_rank > 0)
                    .map(|(k, v)| (k.clone(), v.mastery_rank))
                    .collect(),
                changes: vec![],
                consumed_suits: startup_cache.consumed_suits(),
                mods: known_mods.clone(),
                socketed_shards: startup_cache.items.iter()
                    .filter(|(_, v)| !v.archon_shards.is_empty())
                    .map(|(k, v)| (k.clone(), v.archon_shards.clone()))
                    .collect(),
                forma_counts: startup_cache.items.iter()
                    .filter_map(|(k, v)| v.forma_count.map(|n| (k.clone(), n)))
                    .collect(),
                warframe_running: game_found,
                scanned_at: now_pre,
                is_full_pass: true,
                player_name: app.state::<AppState>().local_player_name
                    .lock().ok().and_then(|g| g.clone()),
            });
        }

        let mut current_mastery_rank: Option<u32> = startup_cache.mastery_rank;
        let mut current_mastery_data: HashMap<String, u32> = startup_cache.items.iter()
            .filter(|(_, v)| v.mastery_rank > 0)
            .map(|(k, v)| (k.clone(), v.mastery_rank))
            .collect();
        let mut current_recipes: Vec<memory_scanner::PendingRecipe> = Vec::new();
        let mut current_consumed_suits: Vec<String> = startup_cache.consumed_suits();
        let mut current_socketed_shards: HashMap<String, Vec<memory_scanner::ArchonShard>> = startup_cache.items.iter()
            .filter(|(_, v)| !v.archon_shards.is_empty())
            .map(|(k, v)| (k.clone(), v.archon_shards.clone()))
            .collect();
        let mut current_forma_counts: HashMap<String, u32> = startup_cache.items.iter()
            .filter_map(|(k, v)| v.forma_count.map(|n| (k.clone(), n)))
            .collect();
        let mut last_blob_time: Option<std::time::Instant> = None;
        // Guard against overlapping captures: a full memory walk can take >10 s on large
        // game processes, so without this flag we'd stack up concurrent scan threads.
        let blob_scan_active = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        // Cache the game-running state so we only re-enumerate processes once every 5 s
        // instead of on every 2-second loop tick (CreateToolhelp32Snapshot is not free).
        let mut last_pid_check: Option<std::time::Instant> = None;
        let mut last_pid: Option<u32> = None;
        let mut cached_game_running = false;
        // When game is not running, suppress redundant inventory-update emits.
        // Only emit on the status-change tick and then at most once every 30 s as a heartbeat.
        let mut prev_game_running = false;
        let mut last_not_running_emit: Option<std::time::Instant> = None;

        while flag.load(Ordering::SeqCst) {
            // If shared_quantities was cleared externally (clear_cache command), wipe local
            // state so the next blob logs everything as fresh.
            {
                let sq = shared_quantities.lock().unwrap_or_else(|e| e.into_inner());
                let local_has_data = !known.is_empty() || !unique_stable.is_empty() || !known_mods.is_empty();
                if sq.is_empty() && local_has_data {
                    known.clear();
                    unique_stable.clear();
                    confirmed_unique.clear();
                    known_mods.clear();
                }
            }

            let now = chrono::Utc::now().timestamp();

            // Process any incoming blob (non-blocking)
            while let Ok(blob) = blob_rx.try_recv() {
                let existing_wfm: HashMap<String, u32> =
                    load_inventory_state_cache(&inventory_state_cache_path)
                        .items.into_iter()
                        .filter_map(|(k, v)| v.wfm_price.map(|p| (k, p)))
                        .collect();
                let sc = build_inventory_from_blob(
                    &blob,
                    &path_to_name, &path_to_category, &path_to_ducat, &path_to_vaulted,
                    &relic_drops_snapshot, &existing_wfm, &alias_excluded,
                );
                if let Ok(json) = serde_json::to_string(&sc) {
                    let _ = atomic_write(&inventory_state_cache_path, json.as_bytes());
                }

                // Snapshot previous full inventory (known + uniques + mods) for change detection.
                let prev_all: HashMap<String, i64> = {
                    let mut m = known.clone();
                    for k in &confirmed_unique { m.entry(k.clone()).or_insert(1); }
                    for (p, mc) in &known_mods { m.entry(p.clone()).or_insert(mc.total); }
                    m
                };

                // Completeness guard: parse_full_account_blob already rejects blobs missing
                // required sections (MiscItems, RegularCredits, etc.) — see memory_scanner.rs.
                // Keep this secondary guard for the unique-items case as a belt-and-suspenders
                // defence against incomplete blobs that slipped through parsing.
                let prev_unique_count = confirmed_unique.len();
                if blob.unique_items.is_empty() && prev_unique_count > 0 {
                    warn!("blob rejected at commit: 0 unique items vs {} previously — incomplete blob", prev_unique_count);
                    continue;
                }

                // Blob is authoritative — full replacement, not a merge.
                // Clear known so items that disappeared from the blob drop to 0.
                known.clear();

                // Currency
                known.insert("/_currency/Credits".to_string(),      blob.credits);
                known.insert("/_currency/Endo".to_string(),         blob.endo);
                known.insert("/_currency/Platinum".to_string(),     blob.platinum - blob.free_platinum);
                known.insert("/_currency/PlatinumGift".to_string(), blob.free_platinum);

                // Stackable items
                for entry in &blob.stackable_items {
                    known.insert(entry.item_type.clone(), entry.item_count);
                }

                // Unique items — full replacement (blob is authoritative)
                unique_stable.clear();
                confirmed_unique.clear();
                current_socketed_shards.clear();
                current_forma_counts.clear();
                for entry in &blob.unique_items {
                    let canonical = path_aliases.get(entry.item_type.as_str())
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| entry.item_type.clone());
                    if blob.consumed_suits.contains(&canonical) { continue; }
                    unique_stable.insert(canonical.clone(), 4);
                    confirmed_unique.insert(canonical.clone());
                    if !entry.archon_shards.is_empty() {
                        current_socketed_shards.insert(canonical.clone(), entry.archon_shards.clone());
                    }
                    if entry.polarized > 0 {
                        current_forma_counts.insert(canonical, entry.polarized);
                    }
                }

                // Mods — full replacement
                known_mods.clear();
                for (path, mc) in &blob.mods {
                    known_mods.insert(path.clone(), mc.clone());
                }
                // Rivens — group by item_type so they appear in inventory like regular mods
                for riven in &blob.rivens {
                    let mc = known_mods.entry(riven.item_type.clone()).or_default();
                    mc.total += riven.count as i64;
                    *mc.by_rank.entry(riven.mod_rank).or_insert(0) += riven.count as i64;
                }

                // Cosmetics (FlavourItems + WeaponSkins) — occurrence-counted, go into known
                for (path, &count) in blob.flavour_items.iter().chain(blob.weapon_skins.iter()) {
                    known.insert(path.clone(), count);
                }

                // Debug: write paths with no WFCD entry or Misc fallback to the Unmatched Paths folder.
                if debug_cat_enabled.load(Ordering::Relaxed) {
                    // ── Reference file (written once per session) ─────────────────────
                    // Lists every distinct item_type / product_category / wfcd_category value
                    // present in the catalog, together with the display category fix_category()
                    // assigns to each.  Useful for adding new tiers to fix_category.
                    let ref_path = unmatched_paths_dir.join("_reference.json");
                    if !ref_path.exists() {
                        // Collect distinct values; BTreeMap keeps them alphabetically sorted.
                        // Iterate over path_to_name (covers ALL catalog entries, including
                        // blueprints that have item_type = "" but wfcd_category = "Blueprints").
                        let mut item_types: std::collections::BTreeMap<String, String> = Default::default();
                        let mut prod_cats:  std::collections::BTreeMap<String, String> = Default::default();
                        let mut wfcd_cats:  std::collections::BTreeMap<String, String> = Default::default();
                        for (path, nm) in &path_to_name {
                            let it  = path_to_item_type.get(path).map(|s| s.as_str()).unwrap_or("");
                            let pc  = path_to_product_category.get(path).map(|s| s.as_str()).unwrap_or("");
                            let wc  = path_to_wfcd_cat.get(path).map(|s| s.as_str()).unwrap_or("");
                            let cat = fix_category(nm, it, pc, wc, path);
                            if !it.is_empty() { item_types.entry(it.to_string()).or_insert(cat.clone()); }
                            if !pc.is_empty() { prod_cats.entry(pc.to_string()).or_insert(cat.clone()); }
                            if !wc.is_empty() { wfcd_cats.entry(wc.to_string()).or_insert(cat); }
                        }
                        let ref_json = serde_json::json!({
                            "note": "Distinct field values from the loaded WFCD catalog. 'maps_to' shows the display category fix_category() assigns when that field is the deciding factor.",
                            "item_type": item_types.iter().map(|(v, c)| serde_json::json!({ "value": v, "maps_to": c })).collect::<Vec<_>>(),
                            "product_category": prod_cats.iter().map(|(v, c)| serde_json::json!({ "value": v, "maps_to": c })).collect::<Vec<_>>(),
                            "wfcd_category": wfcd_cats.iter().map(|(v, c)| serde_json::json!({ "value": v, "maps_to": c })).collect::<Vec<_>>(),
                        });
                        if let Ok(s) = serde_json::to_string_pretty(&ref_json) {
                            let _ = std::fs::write(&ref_path, s);
                        }
                    }

                    // ── Per-scan unmatched file ───────────────────────────────────────
                    // Build per-path blob field lookups.
                    let stackable_count: std::collections::HashMap<&str, i64> = blob.stackable_items.iter()
                        .map(|e| (e.item_type.as_str(), e.item_count)).collect();
                    let unique_section: std::collections::HashMap<&str, &str> = blob.unique_items.iter()
                        .map(|e| (e.item_type.as_str(), e.section.as_str())).collect();
                    let unique_polarized: std::collections::HashMap<&str, u32> = blob.unique_items.iter()
                        .map(|e| (e.item_type.as_str(), e.polarized)).collect();

                    let all_paths: Vec<&str> = blob.stackable_items.iter().map(|e| e.item_type.as_str())
                        .chain(blob.unique_items.iter().map(|e| e.item_type.as_str()))
                        .chain(blob.mods.keys().map(|k| k.as_str()))
                        .collect();
                    let mut new_entries: Vec<DebugUnmatched> = Vec::new();
                    for p in all_paths {
                        if p.starts_with("/_currency/") { continue; }
                        if ignored_paths.contains(p) { continue; }
                        let name = path_to_name.get(p).cloned().unwrap_or_default();
                        let (reason, final_cat) = if name.is_empty() {
                            // Check path-prefix rules first (Tier 8 in fix_category).
                            let inferred_cat = fix_category("", "", "", "", p);
                            if inferred_cat != "Miscellaneous" && inferred_cat != "Excluded" {
                                ("path_rule".to_string(), inferred_cat)
                            } else {
                                let last = p.rsplit('/').next().unwrap_or("");
                                if last.ends_with("Blueprint") && p.contains("/Recipes/") {
                                    ("path_inferred".to_string(), "Blueprints".to_string())
                                } else {
                                    ("no_wfcd_match".to_string(), "Unknown".to_string())
                                }
                            }
                        } else {
                            let cat = path_to_category.get(p).map(|s| s.as_str()).unwrap_or("Miscellaneous");
                            if cat != "Miscellaneous" { continue; }
                            ("misc_fallback".to_string(), "Misc".to_string())
                        };
                        // Last 4 non-trivial segments for quick identification.
                        let path_hint: Vec<String> = p.split('/')
                            .filter(|s| !s.is_empty() && *s != "Lotus")
                            .rev().take(4).collect::<Vec<_>>()
                            .into_iter().rev().map(|s| s.to_string()).collect();
                        new_entries.push(DebugUnmatched {
                            path: p.to_string(),
                            name,
                            item_type:        path_to_item_type.get(p).cloned().unwrap_or_default(),
                            product_category: path_to_product_category.get(p).cloned().unwrap_or_default(),
                            wfcd_category:    path_to_wfcd_cat.get(p).cloned().unwrap_or_default(),
                            final_category:   final_cat,
                            reason,
                            item_count:  stackable_count.get(p).copied(),
                            section:     unique_section.get(p).map(|s| s.to_string()),
                            polarized:   unique_polarized.get(p).copied(),
                            mod_total:   blob.mods.get(p).map(|m| m.total),
                            path_hint,
                        });
                    }
                    if !new_entries.is_empty() {
                        let ts = chrono::Utc::now().format("%Y-%m-%dT%H-%M-%SZ").to_string();
                        let out = unmatched_paths_dir.join(format!("{}.json", ts));
                        if let Ok(json) = serde_json::to_string_pretty(&new_entries) {
                            let _ = std::fs::write(&out, json);
                        }
                    }
                }

                // Meta
                current_mastery_rank = Some(blob.mastery_level);
                for (path, &rank) in &blob.mastery_data {
                    current_mastery_data.insert(path.clone(), rank);
                }
                current_consumed_suits = blob.consumed_suits.clone();
                for suit in &current_consumed_suits {
                    confirmed_unique.remove(suit);
                    unique_stable.remove(suit);
                }
                current_recipes = blob.pending_recipes.iter().map(|r| memory_scanner::PendingRecipe {
                    unique_name:   r.item_type.clone(),
                    completion_ms: r.completion_ms,
                }).collect();

                // Sync shared state
                if let Ok(mut q)  = shared_quantities.lock() { *q = known.clone(); }
                if let Ok(mut sm) = shared_mods.lock()       { *sm = known_mods.clone(); }
                if let Ok(mut uq) = shared_unique.lock() {
                    uq.clear();
                    for name in &confirmed_unique { uq.insert(name.clone(), 1); }
                }

                // Emit inventory update
                let mut emit_qty = known.clone();
                for k in &confirmed_unique { emit_qty.entry(k.clone()).or_insert(1); }
                for (p, mc) in &known_mods { emit_qty.entry(p.clone()).or_insert(mc.total); }

                // Detect and record every quantity change (up, down, new, gone-to-0).
                // Skip on the very first blob of the session (prev_all empty = no prior baseline).
                let mut changes: Vec<QuantityChange> = vec![];
                if !prev_all.is_empty() {
                    let ts = chrono::Utc::now().timestamp();
                    let all_keys: std::collections::HashSet<&String> =
                        prev_all.keys().chain(emit_qty.keys()).collect();
                    for key in all_keys {
                        let old_qty = *prev_all.get(key).unwrap_or(&0);
                        let new_qty = *emit_qty.get(key).unwrap_or(&0);
                        if old_qty == new_qty { continue; }
                        let item_name = path_to_name.get(key.as_str())
                            .cloned()
                            .unwrap_or_else(|| key.split('/').last().unwrap_or("?").to_string());
                        let _ = db::add_quantity_change(&conn, key, &item_name, old_qty, new_qty);
                        changes.push(QuantityChange {
                            id: 0,
                            unique_name: key.clone(),
                            item_name,
                            old_qty,
                            new_qty,
                            delta: new_qty - old_qty,
                            timestamp: ts,
                        });
                    }
                }

                let crafting: Vec<CraftingJob> = blob.pending_recipes.iter().map(|r| {
                    let name = display_names.iter().zip(unique_names.iter())
                        .find(|(_, u)| **u == r.item_type)
                        .map(|(d, _)| d.clone())
                        .unwrap_or_else(|| r.item_type.split('/').last().unwrap_or("?").to_string());
                    CraftingJob { unique_name: r.item_type.clone(), item_name: name, completion_ms: r.completion_ms }
                }).collect();
                *shared_crafting.lock().unwrap_or_else(|e| e.into_inner()) = crafting.clone();
                let _ = app.emit("inventory-update", InventoryUpdate {
                    quantities: emit_qty,
                    crafting,
                    mastery_rank: current_mastery_rank,
                    mastery_data: current_mastery_data.clone(),
                    changes,
                    warframe_running: true,
                    scanned_at:   now,
                    consumed_suits:   current_consumed_suits.clone(),
                    mods:             known_mods.clone(),
                    socketed_shards:  current_socketed_shards.clone(),
                    forma_counts:     current_forma_counts.clone(),
                    is_full_pass:     true,
                    player_name: app.state::<AppState>().local_player_name
                        .lock().ok().and_then(|g| g.clone()),
                });

                let detail = format!(
                    "{} unique · {} resources · {} mods · {} flavour",
                    blob.unique_items.len(), blob.stackable_items.len(),
                    blob.mods.len(), blob.flavour_items.len()
                );
                info!(detail = %detail, "blob applied");
                let _ = app.emit("blob-status", BlobStatusPayload {
                    stage: "done".into(),
                    detail,
                });

                // Daily snapshots
                let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
                if today != last_snapshot_date {
                    last_snapshot_date = today.clone();
                    if let Ok(tracked) = db::get_tracked_items(&conn) {
                        for item in &tracked {
                            let qty = *known.get(&item.unique_name).unwrap_or(&0);
                            let _ = db::record_snapshot(&conn, &item.unique_name, &today, qty);
                        }
                    }
                }
            }

            // Re-enumerate processes at most every 5 s (CreateToolhelp32Snapshot overhead).
            let needs_pid_check = last_pid_check
                .map_or(true, |t: std::time::Instant| t.elapsed().as_secs() >= 5);
            if needs_pid_check {
                let current_pid = memory_scanner::find_warframe_pid_pub();
                cached_game_running = current_pid.is_some();
                if current_pid != last_pid {
                    if current_pid.is_some() {
                        info!(?last_pid, ?current_pid, "Warframe PID changed, clearing blob region cache");
                        memory_scanner::reset_last_blob_region();
                    }
                    last_pid = current_pid;
                }
                last_pid_check = Some(std::time::Instant::now());
            }
            let game_running = cached_game_running;
            if game_running {
                // ── Blob capture: unconditional scan every 10 seconds ─────────
                let should_capture = last_blob_time
                    .map_or(true, |t: std::time::Instant| t.elapsed() >= std::time::Duration::from_secs(10));
                let already_running = blob_scan_active.load(Ordering::SeqCst);

                if should_capture && !already_running {
                    blob_scan_active.store(true, Ordering::SeqCst);
                    last_blob_time = Some(std::time::Instant::now());
                    let ts     = chrono::Utc::now().format("%Y-%m-%dT%H-%M-%S").to_string();
                    let dir    = blob_log_dir.clone();
                    let tx     = blob_tx.clone();
                    let save   = blob_log_enabled.load(Ordering::SeqCst);
                    let active = blob_scan_active.clone();
                    let _ = app.emit("blob-status", BlobStatusPayload {
                        stage:  "scanning".into(),
                        detail: "Reading Warframe memory\u{2026}".into(),
                    });
                    debug!(save, "blob capture starting");
                    std::thread::spawn(move || {
                        struct ClearOnDrop(std::sync::Arc<std::sync::atomic::AtomicBool>);
                        impl Drop for ClearOnDrop {
                            fn drop(&mut self) { self.0.store(false, Ordering::SeqCst); }
                        }
                        let _guard = ClearOnDrop(active);
                        let count = memory_scanner::capture_all_blobs(&dir, &ts, tx, save);
                        debug!(files_saved = count, save_flag = save, ts = %ts, "blob capture finished");
                    });
                }
                prev_game_running = true;
            } else {
                // Game not running — throttle emits: only on status-change and every 30 s heartbeat.
                // Without this guard the loop emits every 2 s with identical data, triggering a
                // full React render cascade (17 k-item useMemo rebuild) 30 times per minute.
                let status_changed = prev_game_running;
                let heartbeat_due  = last_not_running_emit
                    .map_or(true, |t: std::time::Instant| t.elapsed() >= std::time::Duration::from_secs(30));
                if status_changed || heartbeat_due {
                    let mut emit_qty = known.clone();
                    for k in &confirmed_unique { emit_qty.entry(k.clone()).or_insert(1); }
                    for (p, mc) in &known_mods { emit_qty.entry(p.clone()).or_insert(mc.total); }
                    let crafting: Vec<CraftingJob> = current_recipes.iter().map(|r| {
                        let name = display_names.iter().zip(unique_names.iter())
                            .find(|(_, u)| *u == &r.unique_name)
                            .map(|(d, _)| d.clone())
                            .unwrap_or_else(|| r.unique_name.split('/').last().unwrap_or("?").to_string());
                        CraftingJob { unique_name: r.unique_name.clone(), item_name: name, completion_ms: r.completion_ms }
                    }).collect();
                    // Skip mastery_data on heartbeats — it hasn't changed and spreading 17k
                    // entries into React state on every tick is expensive.
                    let send_mastery = status_changed;
                    let _ = app.emit("inventory-update", InventoryUpdate {
                        quantities: emit_qty, crafting,
                        mastery_rank: current_mastery_rank,
                        mastery_data: if send_mastery { current_mastery_data.clone() } else { HashMap::new() },
                        changes: vec![], warframe_running: false, scanned_at: now,
                        consumed_suits: current_consumed_suits.clone(),
                        mods: known_mods.clone(),
                        socketed_shards: current_socketed_shards.clone(),
                        forma_counts: current_forma_counts.clone(),
                        is_full_pass: false,
                        player_name: app.state::<AppState>().local_player_name
                            .lock().ok().and_then(|g| g.clone()),
                    });
                    last_not_running_emit = Some(std::time::Instant::now());
                }
                prev_game_running = false;
            }

            std::thread::sleep(std::time::Duration::from_secs(2));
        }
    });

    // ── Dedicated relic reward thread — OCR poll every 500 ms ───────────────
    // Takes a screenshot of the Warframe window, runs Windows OCR on the
    // reward area, matches names against the catalog. Emits "relic-rewards"
    // only when the result changes (screen opens/closes or items change).
    let reward_flag   = state.monitor_active.clone();
    let reward_items  = state.wfcd_items.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let relic_rewards_map = state.relic_rewards.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let wiki_names    = state.wiki_reward_names.lock().unwrap_or_else(|e| e.into_inner()).clone();

    // ── Catalog: build from Relics.json reward names ─────────────────────────
    //
    // relic_rewards_map is already populated from Relics.json — every name in it
    // IS a confirmed relic reward. Start from those names and look up unique_names
    // in the WFCD item catalog. This guarantees only actual relic rewards appear
    // as OCR candidates, preventing false matches against items like
    // "Titan Extractor Prime Blueprint" or "Kavasa Prime Kubrow Collar Kavasa Prime Band"
    // that contain "prime" but are not relic rewards.

    // Collect all reward display names (lowercase) from Relics.json + wiki corrections.
    let reward_display_names: std::collections::HashSet<String> = relic_rewards_map
        .values()
        .flat_map(|rewards| rewards.iter().map(|r| r.name.to_lowercase()))
        .chain(wiki_names.iter().cloned())
        .collect();

    // Build a lowercase-name → (unique_name, original_display_name) lookup over the WFCD
    // item catalog. Excludes assembled warframes/weapons and relics (never relic rewards).
    let wfcd_by_name: std::collections::HashMap<String, (String, String)> = reward_items.iter()
        .filter(|i| {
            let lower = i.name.to_lowercase();
            let is_relic = lower.ends_with("intact") || lower.ends_with("exceptional")
                || lower.ends_with("flawless") || lower.ends_with("radiant");
            let is_built = matches!(i.category.as_str(),
                "Warframes" | "Primary" | "Secondary" | "Melee" | "Companion" |
                "Sentinels" | "Archwing" | "Arch-Gun" | "Arch-Melee" | "Pets" | "Robotic");
            !is_relic && !is_built
        })
        .map(|i| (i.name.to_lowercase(), (i.unique_name.clone(), i.name.clone())))
        .collect();

    // For each known reward name find the WFCD unique_name.
    // Handles the WFCD "Blueprint" suffix inconsistency in both directions:
    //   Relics.json "Lavos Prime Chassis Blueprint" ↔ WFCD item "Lavos Prime Chassis"
    let mut catalog_pairs: Vec<(String, String)> = reward_display_names.iter()
        .filter_map(|reward_lower| {
            // Exact match
            if let Some((unique, display)) = wfcd_by_name.get(reward_lower.as_str()) {
                return Some((unique.clone(), display.clone()));
            }
            // Reward has " blueprint" suffix but WFCD item doesn't
            if let Some(stem) = reward_lower.strip_suffix(" blueprint") {
                if let Some((unique, display)) = wfcd_by_name.get(stem) {
                    return Some((unique.clone(), format!("{} Blueprint", display)));
                }
            }
            // Reward lacks " blueprint" but WFCD item has it
            let with_bp = format!("{} blueprint", reward_lower);
            if let Some((unique, display)) = wfcd_by_name.get(&with_bp) {
                return Some((unique.clone(), display.clone()));
            }
            // Not in WFCD item catalog — skip (no unique_name means no price/inventory data)
            None
        })
        .collect();

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
        .map(|d| d.join("Warframe").join("EE.log"));

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

    // Squad member names collected from EE.log "AddSquadMember:" lines.
    // Passed to OCR so it can reject any text that fuzzy-matches a player name.
    let shared_squad_names: std::sync::Arc<std::sync::Mutex<Vec<String>>> =
        std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let shared_squad_names2 = std::sync::Arc::clone(&shared_squad_names);

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
    let ee_auto_capture_dir = auto_capture_dir.clone();

    if let Some(log_path) = ee_log_path {
        let flag = reward_flag.clone();
        std::thread::spawn(move || {
            let mut file_pos: u64 = std::fs::metadata(&log_path)
                .map(|m| m.len()).unwrap_or(0);
            let mut active_since: Option<std::time::Instant> = None;
            use std::io::{Read, Seek, SeekFrom};

            // ── Startup scan: seed player names from the existing log ─────────
            // The tail starts at file-end so lines written before FrameForge launched
            // are invisible to it. Two bounded reads cover both cases:
            //  • First 64 KB  → "Logged in NAME" is always within the first ~100 lines.
            //  • Last 1 MB    → AddSquadMember fires during mission load-in (recent).
            // Bounded reads avoid stalling on a log file that has grown to hundreds of MB.
            {
                use std::io::{Read, Seek, SeekFrom};

                // Read the last 1 MB of EE.log. This covers both cases:
                //   • EE.log resets on game launch → whole file fits in 1 MB.
                //   • EE.log accumulates → current session's "Logged in" is near the end.
                // Searching only the first 64 KB misses the current session when the log
                // has grown large from previous runs.
                if let Ok(mut f) = std::fs::File::open(&log_path) {
                    let file_len = f.seek(SeekFrom::End(0)).unwrap_or(0);
                    let read_from = file_len.saturating_sub(1_048_576); // last 1 MB
                    let _ = f.seek(SeekFrom::Start(read_from));
                    let mut buf = Vec::with_capacity(1_048_576);
                    let _ = f.read_to_end(&mut buf);
                    // Skip first (potentially partial) line when starting mid-file.
                    let start = if read_from > 0 { buf.iter().position(|&b| b == b'\n').map_or(0, |i| i + 1) } else { 0 };
                    if let Ok(text) = std::str::from_utf8(&buf[start..]) {
                        // ── Local player name (most recent "Logged in NAME") ──────────
                        parse_logged_in_name(text, &shared_squad_names2, &ee_ocr_app);

                        // ── Squad mate names ──────────────────────────────────────────
                        for line in text.lines() {
                            if line.contains("AddSquadMember: ") {
                                if let Some(after) = line.find("AddSquadMember: ").map(|i| &line[i + 16..]) {
                                    if let Some(name) = after.split(',').next().map(str::trim) {
                                        if !name.is_empty() {
                                            if let Ok(mut g) = shared_squad_names2.lock() {
                                                if !g.iter().any(|n: &String| n == name) {
                                                    g.push(name.to_string());
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            // "NAME - new avatar: CombatOperatorAvatar…" fires when any player
                            // switches to Operator or Necramech — catches squadmates who joined
                            // before FrameForge started (and thus missed AddSquadMember lines).
                            if line.contains(" - new avatar: ") {
                                if let Some(after_bracket) = line.find("]: ").map(|i| &line[i + 3..]) {
                                    if let Some(name) = after_bracket.split(" - new avatar:").next() {
                                        let name = name.trim();
                                        if name.len() >= 3 && !name.contains(' ') {
                                            if let Ok(mut g) = shared_squad_names2.lock() {
                                                if !g.iter().any(|n: &String| n == name) {
                                                    g.push(name.to_string());
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // ── VoidProjections reward sequence state ─────────────────────────
            // The game logs squad reward info BEFORE the screen trigger fires.
            // We accumulate it across poll iterations so it's ready when OCR starts.
            let mut vp_in_seq        = false;
            let mut vp_seq_completed = false; // set when sequence finishes; used as fallback trigger
            let mut vp_other_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
            let mut vp_own_item = String::new(); // local player's reward path from EE.log
            // Cooldown: after any dismiss, block new triggers for 5 s to filter
            // stale EE.log lines that can arrive shortly after a dismiss.
            let mut last_dismiss_at: Option<std::time::Instant> = None;
            // ── Relic prefilter ───────────────────────────────────────────────────
            // Projection paths collected from "Resource load completed" EE.log lines
            // while squad loadouts download. Used at trigger time to narrow the OCR
            // candidate list from ~700 items to the ~6-24 rewards of the active relics.
            // To revert: delete this Vec, the collection block below, the clear in the
            // dismiss handler, and the filtered_cat block at trigger time.
            let mut session_relics: Vec<String> = Vec::new();
            // One diagnostics folder per trigger→dismiss cycle.
            // Created at trigger, BMP written after overlay confirmed, session log at dismiss.
            let diag_arc: Arc<Mutex<Option<std::path::PathBuf>>> = Arc::new(Mutex::new(None));

            // Use FindFirstChangeNotificationW so we wake the instant EE.log is
            // written to disk instead of sleeping 200 ms between checks.
            let change_handle: isize = {
                use windows_sys::Win32::Storage::FileSystem::{
                    FindFirstChangeNotificationW, FILE_NOTIFY_CHANGE_LAST_WRITE,
                };
                let dir = log_path.parent().unwrap_or(std::path::Path::new("."));
                let dir_wide: Vec<u16> = dir.to_string_lossy()
                    .encode_utf16().chain(std::iter::once(0)).collect();
                unsafe { FindFirstChangeNotificationW(dir_wide.as_ptr(), 0, FILE_NOTIFY_CHANGE_LAST_WRITE) }
            };
            let use_notify = change_handle != -1isize; // -1 = INVALID_HANDLE_VALUE

            loop {
                if !flag.load(Ordering::SeqCst) { break; }
                if use_notify {
                    use windows_sys::Win32::System::Threading::WaitForSingleObject;
                    use windows_sys::Win32::Storage::FileSystem::FindNextChangeNotification;
                    // Block until a write lands in the EE.log directory (500 ms safety timeout
                    // keeps the flag check alive even when the game isn't writing).
                    unsafe { WaitForSingleObject(change_handle, 500); }
                    unsafe { FindNextChangeNotification(change_handle); }
                } else {
                    std::thread::sleep(std::time::Duration::from_millis(200));
                }
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
                //   "VoidProjections: GetVoidProjectionReward[s]"  → sequence start
                //   "[id] gets reward /Lotus/..."                  → local player's item
                //   "Still waiting on response from [id]"          → one other player
                //   "Client has reward info for all players now"   → sequence complete
                //
                // squad_size = 1 (local) + count("Still waiting") lines.
                // Logging only for now; item path matching is a future improvement.
                for line in buf.lines() {
                    let ll = line.to_lowercase();
                    if ll.contains("voidprojections: getvoidprojectionreward") {
                        vp_in_seq  = true;
                        vp_other_ids.clear();
                        vp_own_item.clear();
                        // Reset the shared mutex so any OCR loop that's still
                        // retrying from a previous fissure doesn't carry a stale
                        // squad count into the next one.
                        if let Ok(mut g) = shared_squad_size2.lock() { *g = None; }
                    }
                    // Capture "gets reward" whenever it appears — inside or outside
                    // the VP sequence. The line fires when the server confirms the local
                    // player's reward assignment, which can happen just after the screen
                    // opens (same EE.log flush, after vp_in_seq has already closed).
                    if ll.contains("gets reward /lotus/") {
                        if let Some(i) = line.find("/Lotus/") {
                            vp_own_item = line[i..].trim().to_string();
                        }
                    }
                    if vp_in_seq {
                        if ll.contains("gets reward /lotus/") {
                            // Already captured above — handled outside the block.
                        } else if ll.contains("still waiting on response from") {
                            // Extract the player ID (last whitespace-separated token)
                            if let Some(id) = ll.split_whitespace().last() {
                                vp_other_ids.insert(id.to_string());
                            }
                        } else if ll.contains("has reward info for all players now") {
                            // squad = local player (1) + unique other IDs seen
                            let squad = (1 + vp_other_ids.len()).clamp(1, 4);
                            // Update the shared mutex so any pending OCR retry reads the correct count.
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

                // ── Relic prefilter: collect squad projection paths ───────────────
                // "Resource load completed 0x... (/Lotus/Types/Game/Projections/T?VoidProjection...)"
                // fires once per squad member's relic as their loadout is downloaded.
                // All 4 relics appear seconds before the mission starts, well ahead of
                // the reward screen trigger (~200+ seconds later).
                for line in buf.lines() {
                    if line.contains("Resource load completed")
                        && line.contains("/Lotus/Types/Game/Projections/")
                    {
                        if let Some(paren) = line.find("(/Lotus/Types/Game/Projections/") {
                            let rest = &line[paren + 1..]; // skip the '('
                            let path = rest.split(')').next().unwrap_or("").trim().to_string();
                            if !path.is_empty() && !session_relics.contains(&path) {
                                session_relics.push(path.clone());
                            }
                        }
                    }
                }

                // ── Squad member name collection ─────────────────────────────────
                // "AddSquadMember: NAME, mm=..." fires when each squadmate loads in.
                // "Logged in NAME" fires when the local player signs in — their name
                // never appears in AddSquadMember (that's only for squad mates).
                // Both sets feed the OCR filter so usernames don't fuzzy-match items.
                for line in buf.lines() {
                    if line.contains("AddSquadMember: ") {
                        if let Some(after) = line.find("AddSquadMember: ").map(|i| &line[i + 16..]) {
                            if let Some(name) = after.split(',').next().map(str::trim) {
                                if !name.is_empty() {
                                    if let Ok(mut g) = shared_squad_names2.lock() {
                                        if !g.iter().any(|n: &String| n == name) {
                                            g.push(name.to_string());
                                        }
                                    }
                                }
                            }
                        }
                    }
                    if line.contains(" - new avatar: ") {
                        if let Some(after_bracket) = line.find("]: ").map(|i| &line[i + 3..]) {
                            if let Some(name) = after_bracket.split(" - new avatar:").next() {
                                let name = name.trim();
                                if name.len() >= 3 && !name.contains(' ') {
                                    if let Ok(mut g) = shared_squad_names2.lock() {
                                        if !g.iter().any(|n: &String| n == name) {
                                            g.push(name.to_string());
                                        }
                                    }
                                }
                            }
                        }
                    }
                    if line.contains("Logged in ") {
                        parse_logged_in_name(line, &shared_squad_names2, &ee_ocr_app);
                    }
                }

                // ── WFM trade whisper detection ──────────────────────────────────
                if lower.contains("(warframe.market)") {
                    // EE.log whisper format: "@From Username : Hi! I want to buy Item for N platinum. (warframe.market)"
                    let raw = buf.as_str();
                    let from = raw.find("@From ")
                        .map(|i| &raw[i+6..])
                        .and_then(|s| s.split(" :").next())
                        .map(|s| s.trim().to_string())
                        .unwrap_or_else(|| "Unknown".to_string());
                    let item = {
                        let prefix = "want to buy ";
                        let suffix = " for ";
                        raw.find(prefix).and_then(|i| {
                            let rest = &raw[i+prefix.len()..];
                            rest.find(suffix).map(|j| rest[..j].to_string())
                        })
                    };
                    let price: Option<u64> = raw.find(" for ").and_then(|i| {
                        let rest = &raw[i+5..];
                        rest.find(" platinum").and_then(|j| rest[..j].trim().parse().ok())
                    });
                    let _ = ee_ocr_app.emit("wfm-whisper", serde_json::json!({
                        "from": from,
                        "message": raw.trim(),
                        "item": item,
                        "price": price,
                        "timestamp": chrono::Local::now().format("%H:%M:%S").to_string(),
                    }));
                }

                // Riven trigger and close events are handled exclusively by start_log_watcher
                // (always-on) — do not duplicate them here.

                // Unveil: riven challenge completion
                if lower.contains("modreveal") || (lower.contains("riven") && lower.contains("unveiled")) {
                    let _ = ee_ocr_app.emit("riven-unveiled", ());
                }

                // Trigger: "VoidProjections: GetVoidProjectionReward[s]" fires when the
                // server actually delivers the reward choices to the client — later than
                // the old "initialized" / "openvoidprojectionrewardscreen" lines, which
                // fired before the cards were visible in endless missions.
                // Matching the singular prefix catches both "Reward" and "Rewards" variants.
                let has_trigger = lower.contains("voidprojections: getvoidprojectionreward")
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
                    let elapsed_s = active_since.map(|t| t.elapsed().as_secs_f64());
                    let dismiss_block = format!(
                        "[STEP 4] DISMISS\n\
                         ├─ Time     : {}\n\
                         ├─ Line     : \"{}\"\n\
                         └─ Open for : {}\n\n",
                        ts_d, dismiss_line,
                        elapsed_s.map(|s| format!("{:.1}s", s)).unwrap_or_else(|| "(unknown)".to_string())
                    );
                    append_to_diag(&session_log_path, &dismiss_block);
                    // Copy the completed session log to the diagnostics folder for this run.
                    if let Ok(mut g) = diag_arc.lock() {
                        if let Some(folder) = g.take() {
                            let _ = std::fs::copy(&session_log_path, folder.join("ocr_session_log.txt"));
                        }
                    }
                    reward_screen_active2.store(false, Ordering::SeqCst);
                    active_since = None;
                    last_dismiss_at = Some(std::time::Instant::now());
                    // Only clear relics on actual mission end. In survival fissures the reward
                    // screen fires "shut down" after every round, but the next round's relic is
                    // selected between that event and the relic selection screen closing. Clearing
                    // here would leave session_relics empty for every round after the first.
                    if lower.contains("matchingservice::endsession") {
                        session_relics.clear();
                    }

                    // ── Immediate inventory update from EE.log reward line ────────
                    // "gets reward /Lotus/StoreItems/..." fires when the player
                    // confirms their reward. Convert to the inventory path and
                    // increment shared_quantities so the UI updates instantly
                    // without waiting for the next memory-scan cycle (~10 s).
                    if !vp_own_item.is_empty() {
                        let store_path = std::mem::take(&mut vp_own_item);
                        let inv_path = store_to_unique(&store_path);
                        let state: tauri::State<AppState> = ee_ocr_app.state();
                        let (old_qty, new_qty) = {
                            let mut qty = state.current_quantities
                                .lock().unwrap_or_else(|e| e.into_inner());
                            let old = *qty.get(&inv_path).unwrap_or(&0);
                            let new = old + 1;
                            qty.insert(inv_path.clone(), new);
                            (old, new)
                        };
                        let item_name = inv_path.split('/').last().unwrap_or("?").to_string();
                        let ts_log = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
                        if let Ok(mut f) = std::fs::OpenOptions::new()
                            .create(true).append(true)
                            .open(&state.changes_log_path)
                        {
                            use std::io::Write;
                            let _ = writeln!(f,
                                "[{}] EE.log Reward | {} | {} → {} (gets reward)",
                                ts_log, item_name, old_qty, new_qty);
                        }
                        let _ = ee_ocr_app.emit("inventory-reward",
                            serde_json::json!({ "path": inv_path, "qty": new_qty }));
                        append_to_diag(&session_log_path, &format!(
                            "[REWARD] Inventory updated from EE.log\n\
                             ├─ Store path : {}\n\
                             ├─ Inv path   : {}\n\
                             └─ Qty        : {} → {}\n\n",
                            store_path, inv_path, old_qty, new_qty
                        ));
                    }

                    if let Some(win) = ee_ocr_app.get_webview_window("relic-overlay") {
                        let _ = win.set_position(tauri::Position::Physical(tauri::PhysicalPosition { x: 0, y: -3000 }));
                    }
                    if let Ok(mut g) = ee_ocr_app.state::<AppState>().pending_relic_rewards.lock() { *g = None; }
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
                    && last_dismiss_at.map_or(true, |t| t.elapsed().as_secs() >= 5);
                if has_trigger && trigger_allowed {
                    reward_screen_active2.store(true, Ordering::SeqCst);
                    active_since = Some(std::time::Instant::now());

                    // Always ensure the local player's name is in the OCR filter — it may be
                    // absent if FrameForge started after the "Logged in" line was written.
                    if let Ok(local_name) = ee_ocr_app.state::<AppState>().local_player_name.lock() {
                        if let Some(ref name) = *local_name {
                            if let Ok(mut g) = shared_squad_names.lock() {
                                if !g.iter().any(|n: &String| n == name) {
                                    g.push(name.clone());
                                }
                            }
                        }
                    }

                    // Find the exact EE.log line that matched so we can log it
                    let trigger_line = buf.lines()
                        .find(|l| {
                            let ll = l.to_lowercase();
                            ll.contains("voidprojections: getvoidprojectionreward")
                        })
                        .unwrap_or("<unknown trigger line>")
                        .trim()
                        .to_string();

                    let ts0 = chrono::Local::now().format("%H:%M:%S%.3f");

                    // Start a fresh session log for this reward screen
                    let known_names_str = {
                        let names = shared_squad_names.lock()
                            .map(|g| g.clone()).unwrap_or_default();
                        if names.is_empty() {
                            "  (none — names not yet seen in EE.log)".to_string()
                        } else {
                            names.iter().map(|n| format!("  • {}", n)).collect::<Vec<_>>().join("\n")
                        }
                    };
                    // ── Relic prefilter: build narrowed catalog from session relics ──
                    // Union the rewards of all collected relics; fall back to the full
                    // catalog if none were seen (FrameForge started mid-mission, solo, etc.)
                    // To revert: delete this block and restore the two lines below it.
                    let (filtered_cat, prefilter_log) = if !session_relics.is_empty() {
                        // Collect reward display names from Relics.json for all active relics.
                        // Relics.json keys match EE.log paths exactly (full path incl. refinement).
                        // Filter ee_catalog by name — avoids unique_name format mismatches.
                        let allowed_names: std::collections::HashSet<String> = {
                            let state = ee_ocr_app.state::<AppState>();
                            let rw = state.relic_rewards.lock().unwrap_or_else(|e| e.into_inner());
                            session_relics.iter()
                                .filter_map(|p| rw.get(p.as_str()))
                                .flat_map(|rewards| rewards.iter().map(|r| r.name.to_lowercase()))
                                .collect()
                        };
                        if allowed_names.is_empty() {
                            let msg = format!(
                                "  {} relic path(s) found but none matched relic_rewards — using full catalog\n  Paths: {:?}",
                                session_relics.len(), session_relics
                            );
                            (Arc::clone(&ee_catalog), msg)
                        } else {
                            let filtered: Vec<(String, String)> = ee_catalog.iter()
                                .filter(|(_, display_name)| {
                                    let dn = display_name.to_lowercase();
                                    // Relics.json omits " Blueprint" from component names
                                    // (e.g. "Nautilus Prime Carapace") while the item catalog
                                    // stores them as "Nautilus Prime Carapace Blueprint".
                                    // Strip the suffix before comparing so both forms match.
                                    let dn_no_bp = dn.strip_suffix(" blueprint").unwrap_or(&dn);
                                    allowed_names.contains(dn_no_bp) || allowed_names.contains(dn.as_str())
                                })
                                .cloned()
                                .collect();
                            if filtered.is_empty() {
                                let mut sample: Vec<&String> = allowed_names.iter().take(8).collect();
                                sample.sort();
                                let msg = format!(
                                    "  {} relic(s) → 0 catalog matches (allowed_names={}) — using full catalog\n  Relics: {:?}\n  Names sample: {:?}",
                                    session_relics.len(), allowed_names.len(), session_relics, sample
                                );
                                (Arc::clone(&ee_catalog), msg)
                            } else {
                                let msg = format!(
                                    "  {} relic(s) → {} candidates (full catalog: {})\n  Relics: {:?}",
                                    session_relics.len(), filtered.len(), ee_catalog.len(), session_relics
                                );
                                (std::sync::Arc::new(filtered), msg)
                            }
                        }
                    } else {
                        (Arc::clone(&ee_catalog), "  No relics collected — using full catalog (FrameForge started mid-mission?)".to_string())
                    };
                    // ── END relic prefilter ───────────────────────────────────────

                    let write_err = std::fs::write(&session_log_path, format!(
                        "══════════════════════════════════════════════\n\
                         RELIC OVERLAY SESSION — {}\n\
                         ══════════════════════════════════════════════\n\
                         Log path  : {}\n\n\
                         [KNOWN PLAYERS — OCR username filter]\n\
                         {}\n\n\
                         [STEP 1] EE.log TRIGGER\n\
                         ├─ Time     : {}\n\
                         ├─ Line     : \"{}\"\n\
                         ├─ Prefilter: {}\n\
                         └─ Catalog  : {} items\n\n",
                        ts0, session_log_path.display(), known_names_str,
                        ts0, trigger_line, prefilter_log, filtered_cat.len()
                    ));
                    if let Err(e) = write_err {
                        warn!(error = %e, "session log write failed");
                    }
                    // Create one diagnostics folder for this entire run.
                    let run_diag_dir = ee_auto_capture_dir.join(
                        chrono::Local::now().format("%Y-%m-%d_%H-%M-%S").to_string()
                    );
                    let _ = std::fs::create_dir_all(&run_diag_dir);
                    if let Ok(mut g) = diag_arc.lock() { *g = Some(run_diag_dir); }
                    let _ = std::fs::write(&ee_last_path, format!(
                        "=== {} ===\nEE.log trigger fired\n{}\n", ts0, trigger_line
                    ));

                    let _ = ee_ocr_app.emit("ff-status", "🔍 Relic reward screen detected");
                    // Tell App.tsx to pre-create the overlay window NOW, before OCR finishes.
                    // Window creation takes 1-2 s; pre-creating shaves that off the visible delay.
                    let _ = ee_ocr_app.emit("relic-trigger", ());

                    let app        = ee_ocr_app.clone();
                    let cat        = filtered_cat; // relic prefilter (was: Arc::clone(&ee_catalog))
                    let cat_len    = cat.len();
                    let lpath      = ee_last_path.clone();
                    let slog       = session_log_path.clone();
                    let active     = reward_screen_active2.clone();
                    let squad_arc  = std::sync::Arc::clone(&shared_squad_size);
                    let names_arc  = std::sync::Arc::clone(&shared_squad_names);
                    let diag_arc2  = Arc::clone(&diag_arc);
                    // Do NOT write ee_squad_size here. The mutex is already reset to None
                    // when GetVoidProjectionRewards fires (above), and is updated to the
                    // correct squad count when the sequence completes (line ~3395).
                    // Writing ee_squad_size here would corrupt the mutex if the sequence
                    // completed in this same poll (the per-line loop runs before this code).

                    tauri::async_runtime::spawn(async move {
                        let deadline = std::time::Instant::now()
                            + std::time::Duration::from_secs(45);
                        // Wait for the VoidProjections EE.log sequence (squad size hint) to
                        // arrive, or proceed after 1500ms if it never comes (solo / missing).
                        // The sequence fires after the server responds to GetVoidProjectionRewards
                        // which can take 800–1500ms after the screen opens. Poll in 100ms ticks.
                        {
                            let hint_deadline = std::time::Instant::now()
                                + std::time::Duration::from_millis(1500);
                            while std::time::Instant::now() < hint_deadline {
                                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                                let has_hint = squad_arc.lock().ok()
                                    .map(|g| g.is_some()).unwrap_or(false);
                                if has_hint { break; }
                            }
                        }

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
                            // Clone the Arc so the hint can be read inside spawn_blocking.
                            // Reading AFTER capture (~100-400 ms) rather than before gives the
                            // EE.log VoidProjections sequence time to complete and write the
                            // correct squad count before we decide how many columns to use.
                            let squad_arc2    = std::sync::Arc::clone(&squad_arc);
                            let names_arc2    = std::sync::Arc::clone(&names_arc);
                            let ocr_frame_arc = Arc::clone(&app.state::<AppState>().last_ocr_frame);
                            let result = tauri::async_runtime::spawn_blocking(move || {
                                let (pixels, w, cap_h, full_h, cap_info) =
                                    ocr::capture_warframe_reward_area()?;
                                // Cache the raw frame so auto-capture can write it to disk
                                // without a second GPU readback (no extra GetDIBits stall).
                                if let Ok(mut g) = ocr_frame_arc.lock() {
                                    *g = Some((pixels.clone(), w, cap_h));
                                }
                                // Read hint AFTER capture — the sequence may have completed
                                // during the PrintWindow/DXGI call.
                                let hint_squad = squad_arc2.lock().ok().and_then(|g| *g);
                                let player_names = names_arc2.lock()
                                    .map(|g| g.clone()).unwrap_or_default();
                                Some(ocr::extract_reward_items_twophase(
                                    &pixels, w, cap_h, full_h, &cat2, &cap_info,
                                    hint_squad, &player_names,
                                ))
                            }).await.ok().flatten();
                            // Re-read hint for confirm_ready logic below (same mutex, post-capture value).
                            let hint_squad = squad_arc.lock().ok().and_then(|g| *g);

                            let ts = chrono::Local::now().format("%H:%M:%S%.3f");
                            let sleep_ms = match &result {
                                // ✅ 1+ items found (solo=1, duo=2, trio=3, full squad=4)
                                Some((complete, _, ref items, ref positions, ref dbg)) if !items.is_empty() => {
                                    let payload = Some(serde_json::json!({
                                        "items": items, "positions": positions
                                    }));

                                    // Determine whether this complete result should be locked now.
                                    // If we have an EE squad hint the count is authoritative.
                                    // If we don't, wait 3 retries (≈1.2 s) before confirming —
                                    // the VoidProjections EE.log sequence typically arrives 1–2 s
                                    // after the trigger, and we need it before we can validate the
                                    // card count. Waiting 3 extra attempts gives it time to arrive.
                                    let soft_retries_done = soft_complete_at
                                        .map_or(false, |sa| (attempt as usize).saturating_sub(sa) >= 3);
                                    // If the EE hint just arrived saying the squad is LARGER than
                                    // what we matched, suppress confirmation and keep retrying.
                                    // The next pass will use word_card_count = hint_squad, split
                                    // the columns correctly, and find the missing card.
                                    let hint_wants_more = hint_squad
                                        .map_or(false, |h| h > items.len());
                                    let confirm_ready = !hint_wants_more
                                        && (hint_squad.is_some() || soft_retries_done);

                                    // Save best result; only emit to overlay when confirmed (LOCK).
                                    // Partial updates are intentionally suppressed — emitting
                                    // partial data while the user is still hovering cards causes
                                    // the overlay to flicker with wrong items between attempts.
                                    let is_new_best = items.len() > best_item_count;
                                    if is_new_best {
                                        best_item_count = items.len();
                                        best_payload = payload.clone();
                                        let label = if *complete && confirm_ready { "✅" } else { "⚡" };
                                        let status_label = if *complete && confirm_ready { "locked" }
                                            else if *complete { "soft-complete, waiting for EE hint" }
                                            else { "waiting" };
                                        let _ = app.emit("ff-status",
                                            format!("{} {} items ({})", label, items.len(), status_label));
                                        let result_label = if *complete && confirm_ready { "LOCKED & emitting" }
                                            else if *complete { "soft-complete, retrying (waiting for EE hint)" }
                                            else { "saved, retrying" };
                                        let session_entry = format!(
                                            "[STEP 2] OCR ATTEMPT #{}\n\
                                             ├─ Time     : {}\n\
                                             {}\n\
                                             └─ RESULT   : {} items found → {}\n\
                                             └─ Items    : {:?}\n\n",
                                            attempt, ts, dbg, items.len(),
                                            result_label,
                                            items,
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
                                            // Log the confirming attempt only when the improvement block above
                                            // didn't already log this attempt (is_new_best = false means item
                                            // count didn't change, so the block above was skipped).
                                            if !is_new_best {
                                                let _ = append_to_file(&slog, &format!(
                                                    "[STEP 2] OCR ATTEMPT #{} (confirm)\n\
                                                     ├─ Time     : {}\n\
                                                     └─ {} items — same as before, confirmed\n\n",
                                                    attempt, ts, items.len()
                                                ));
                                            }
                                            let _ = append_to_file(&slog, "[STEP 3] OVERLAY OPENED\n\n");
                                            // Always emit the BEST result captured so far, not the
                                            // current attempt — later attempts may have worse OCR
                                            // quality (player-name pollution, brightness change).
                                            let emit_val = if best_payload.is_some() { &best_payload } else { &payload };
                                            // Store so Overlay.tsx can pull it on mount (race-condition fix).
                                            if let Some(v) = emit_val.as_ref() {
                                                if let Ok(mut g) = app.state::<AppState>().pending_relic_rewards.lock() {
                                                    *g = Some(v.clone());
                                                }
                                            }
                                            let _ = app.emit("relic-rewards", emit_val);
                                            // After 1.5 s the overlay has finished animating in —
                                            // capture the full desktop (DXGI) so the BMP shows the overlay.
                                            {
                                                let diag_snap = diag_arc2.lock().ok().and_then(|g| g.clone());
                                                if let Some(folder) = diag_snap {
                                                    tauri::async_runtime::spawn(async move {
                                                        tokio::time::sleep(std::time::Duration::from_millis(4000)).await;
                                                        tauri::async_runtime::spawn_blocking(move || {
                                                            if let Some((px, w, h)) = ocr::capture_desktop_for_diag() {
                                                                let _ = write_bmp(&folder.join("screenshot.bmp"), &px, w, h);
                                                            }
                                                        }).await.ok();
                                                    });
                                                }
                                            }
                                            let app2 = app.clone();
                                            let slog2 = slog.clone();
                                            let diag_arc_fb = Arc::clone(&diag_arc2);
                                            let slog_fb = slog.clone();
                                            tauri::async_runtime::spawn(async move {
                                                // 20s safety fallback — normally the overlay closes
                                                // when EE.log fires "relic timer closed" (player picks).
                                                tokio::time::sleep(std::time::Duration::from_secs(20)).await;
                                                if let Ok(mut g) = app2.state::<AppState>().pending_relic_rewards.lock() { *g = None; }
                                                let _ = app2.emit("relic-rewards", serde_json::Value::Null);
                                                if let Some(w) = app2.get_webview_window("relic-overlay") {
                                                    let _ = w.set_position(tauri::Position::Physical(tauri::PhysicalPosition { x: 0, y: -3000 }));
                                                }
                                                append_to_diag(&slog2,
                                                    "[STEP 4] AUTO-DISMISS (20s safety fallback)\n\n");
                                                if let Ok(mut g) = diag_arc_fb.lock() {
                                                    if let Some(folder) = g.take() {
                                                        let _ = std::fs::copy(&slog_fb, folder.join("ocr_session_log.txt"));
                                                    }
                                                }
                                            });
                                            break;
                                        } else {
                                            // Complete result but no EE hint yet — set once and keep
                                            // retrying.  Must NOT overwrite on subsequent iterations
                                            // or the retry counter resets to 1 every loop.
                                            if soft_complete_at.is_none() {
                                                soft_complete_at = Some(attempt as usize);
                                                soft_complete_count = best_item_count;
                                            }
                                        }
                                    } else if soft_complete_at.is_some() && items.len() <= soft_complete_count {
                                        // Soft-complete confirmation retry found no more items.
                                        // A late EE hint may have raised estimated_cards above what
                                        // the screen actually shows (e.g. squad=4 but only 3 unique
                                        // cards because one player lacked reactant or shared a reward).
                                        // Emit best_payload now rather than retrying until timeout.
                                        if !active.load(Ordering::SeqCst) { break; }
                                        let emit_val = best_payload.clone().unwrap_or(serde_json::Value::Null);
                                        if !emit_val.is_null() {
                                            if let Ok(mut g) = app.state::<AppState>().pending_relic_rewards.lock() {
                                                *g = Some(emit_val.clone());
                                            }
                                        }
                                        let _ = app.emit("relic-rewards", &emit_val);
                                        let _ = append_to_file(&slog,
                                            "[STEP 3] OVERLAY OPENED (soft-complete confirmed — no improvement)\n\n");
                                        {
                                            let diag_snap = diag_arc2.lock().ok().and_then(|g| g.clone());
                                            if let Some(folder) = diag_snap {
                                                tauri::async_runtime::spawn(async move {
                                                    tokio::time::sleep(std::time::Duration::from_millis(4000)).await;
                                                    tauri::async_runtime::spawn_blocking(move || {
                                                        if let Some((px, w, h)) = ocr::capture_desktop_for_diag() {
                                                            let _ = write_bmp(&folder.join("screenshot.bmp"), &px, w, h);
                                                        }
                                                    }).await.ok();
                                                });
                                            }
                                        }
                                        let app2 = app.clone();
                                        let slog2 = slog.clone();
                                        let diag_arc_fb = Arc::clone(&diag_arc2);
                                        let slog_fb = slog.clone();
                                        tauri::async_runtime::spawn(async move {
                                            tokio::time::sleep(std::time::Duration::from_secs(20)).await;
                                            if let Ok(mut g) = app2.state::<AppState>().pending_relic_rewards.lock() { *g = None; }
                                            let _ = app2.emit("relic-rewards", serde_json::Value::Null);
                                            if let Some(w) = app2.get_webview_window("relic-overlay") {
                                                let _ = w.set_position(tauri::Position::Physical(tauri::PhysicalPosition { x: 0, y: -3000 }));
                                            }
                                            let _ = append_to_file(&slog2,
                                                "[STEP 4] AUTO-DISMISS (20s safety fallback)\n\n");
                                            if let Ok(mut g) = diag_arc_fb.lock() {
                                                if let Some(folder) = g.take() {
                                                    let _ = std::fs::copy(&slog_fb, folder.join("ocr_session_log.txt"));
                                                }
                                            }
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
                                if !emit_val.is_null() {
                                    if let Ok(mut g) = app.state::<AppState>().pending_relic_rewards.lock() {
                                        *g = Some(emit_val.clone());
                                    }
                                }
                                let _ = app.emit("relic-rewards", &emit_val);
                                let _ = append_to_file(&slog,
                                    "[STEP 2] OCR TIMEOUT — 45 seconds elapsed, emitting best result\n\n");
                                if let Some(win) = app.get_webview_window("relic-overlay") {
                                    let _ = win.set_position(tauri::Position::Physical(tauri::PhysicalPosition { x: 0, y: -3000 }));
                                }
                                active.store(false, Ordering::SeqCst);
                                if let Ok(mut g) = diag_arc2.lock() {
                                    if let Some(folder) = g.take() {
                                        let _ = std::fs::copy(&slog, folder.join("ocr_session_log.txt"));
                                    }
                                }
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
                        append_to_diag(&session_log_path, &format!(
                            "[STEP 4] AUTO-DISMISS (20s timeout)\n\
                             ├─ Time     : {}\n\
                             └─ Open for : {:.1}s\n\n",
                            ts_a, since.elapsed().as_secs_f64()
                        ));
                        if let Ok(mut g) = diag_arc.lock() {
                            if let Some(folder) = g.take() {
                                let _ = std::fs::copy(&session_log_path, folder.join("ocr_session_log.txt"));
                            }
                        }
                        reward_screen_active2.store(false, Ordering::SeqCst);
                        active_since = None;
                        last_dismiss_at = Some(std::time::Instant::now());
                        if let Some(win) = ee_ocr_app.get_webview_window("relic-overlay") {
                            let _ = win.set_position(tauri::Position::Physical(tauri::PhysicalPosition { x: 0, y: -3000 }));
                        }
                        if let Ok(mut g) = ee_ocr_app.state::<AppState>().pending_relic_rewards.lock() { *g = None; }
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

/// Extract the local player name from EE.log lines containing "Logged in NAME".
/// Adds the name to shared_squad_names (for OCR filtering) and AppState.local_player_name
/// (for UI display). Safe to call with a single line or the full log contents.
fn parse_logged_in_name(
    text: &str,
    squad_names: &std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    app: &tauri::AppHandle,
) {
    // Target: "Sys [Info]: Logged in Sikewyrm"
    // The account-login line has exactly ONE token after "Logged in" and nothing more.
    // Lines like "Logged in to region server" have multiple tokens — skip them.
    // Match "]: Logged in " so we don't trigger on unrelated "Logged in …" phrases.
    const MARKER: &str = "]: Logged in ";
    for line in text.lines().rev() {
        let Some(pos) = line.find(MARKER) else { continue };
        let after = line[pos + MARKER.len()..].trim();
        let name: String = after.chars().take_while(|c| !c.is_whitespace()).collect();
        // Skip if anything follows the name — that means it's "Logged in to X", not an account.
        let remainder = after[name.len()..].trim();
        if name.len() < 3 || !remainder.is_empty() { continue; }
        if let Ok(mut g) = squad_names.lock() {
            if !g.iter().any(|n: &String| n == &name) { g.push(name.clone()); }
        }
        if let Ok(mut n) = app.state::<AppState>().local_player_name.lock() {
            *n = Some(name.clone());
        }
        // Emit immediately so the header updates without waiting for the next scan tick.
        let _ = app.emit("player-name", &name);
        return;
    }
}

fn append_to_file(path: &std::path::Path, text: &str) -> std::io::Result<()> {
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(path)?;
    f.write_all(text.as_bytes())
}

/// Append text to both the global overlay session log and the per-session diagnostic file.
/// The diagnostic target is found by picking the most recently modified folder under
/// %TEMP%\warframe-companion\diagnostics\ that contains an ocr_session_log.txt.
fn append_to_diag(global_log: &std::path::Path, text: &str) {
    let _ = append_to_file(global_log, text);
    let diag_base = std::env::temp_dir().join("warframe-companion").join("diagnostics");
    if let Ok(entries) = std::fs::read_dir(&diag_base) {
        let mut folders: Vec<std::path::PathBuf> = entries
            .filter_map(|e| e.ok().map(|d| d.path()))
            .filter(|p| p.is_dir())
            .collect();
        folders.sort();
        if let Some(latest) = folders.last() {
            let diag_log = latest.join("ocr_session_log.txt");
            if diag_log.exists() {
                let _ = append_to_file(&diag_log, text);
            }
        }
    }
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
                "EntratiSyndicate" => {
                    // Cambion Drift — one entry per 150-min cycle; show countdown to cycle end.
                    // (Frontend shows generic "Active" state, no Fass/Vome distinction needed.)
                    if activation_ms <= now_ms && now_ms < expiry_ms {
                        cambion = json!({ "expiry": ms_to_iso(expiry_ms), "active": "cycle" });
                    }
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

// ─── Syndicate stores ─────────────────────────────────────────────────────────

#[derive(serde::Serialize)]
struct SyndicateStoreItem {
    unique_name: String,
    name: String,
    category: String,
    image_name: Option<String>,
    tier: String,
    ducats: Option<u32>,
    /// Quantity of the item/blueprint itself in inventory.
    owned: u32,
    /// For blueprint items: unique_name of the crafted result.
    result_unique: Option<String>,
    /// For blueprint items: quantity of the crafted result in inventory.
    result_owned: u32,
}

#[derive(serde::Serialize)]
struct SyndicateStore {
    name: String,
    items: Vec<SyndicateStoreItem>,
}

/// Returns all syndicate stores with owned quantities cross-referenced from the live inventory.
#[tauri::command]
fn get_syndicate_stores(state: State<AppState>) -> Vec<SyndicateStore> {
    // Preferred display order; any extra syndicates found in the catalog are appended after.
    const ORDER: &[&str] = &[
        "Steel Meridian", "Arbiters of Hexis", "Cephalon Suda",
        "The Perrin Sequence", "Red Veil", "New Loka",
        "Ostron", "Solaris United", "Entrati", "Necraloid",
        "The Holdfasts", "Kahl's Garrison", "Cavia",
        "The Quills", "Vox Solaris", "Ventkids",
        "Cephalon Simaris", "Conclave", "Operational Supply",
    ];
    let catalog = state.syndicate_catalog.lock().unwrap_or_else(|e| e.into_inner());
    let qtys    = state.current_quantities.lock().unwrap_or_else(|e| e.into_inner());

    let mut result: Vec<SyndicateStore> = ORDER.iter()
        .filter_map(|&name| {
            catalog.get(name).map(|offers| {
                let items = offers.iter().map(|o| {
                    let owned = qtys.get(&o.unique_name).copied().unwrap_or(0) as u32;
                    let result_owned = o.result_unique.as_ref()
                        .and_then(|r| qtys.get(r))
                        .copied()
                        .unwrap_or(0) as u32;
                    SyndicateStoreItem {
                        unique_name: o.unique_name.clone(),
                        name: o.name.clone(),
                        category: o.category.clone(),
                        image_name: o.image_name.clone(),
                        tier: o.tier.clone(),
                        ducats: o.ducats,
                        owned,
                        result_unique: o.result_unique.clone(),
                        result_owned,
                    }
                }).collect();
                SyndicateStore { name: name.to_string(), items }
            })
        })
        .collect();

    // Append any syndicates in the catalog that weren't in ORDER
    let known: std::collections::HashSet<&str> = ORDER.iter().copied().collect();
    for (name, offers) in catalog.iter() {
        if known.contains(name.as_str()) { continue; }
        let items = offers.iter().map(|o| {
            let owned = qtys.get(&o.unique_name).copied().unwrap_or(0) as u32;
            let result_owned = o.result_unique.as_ref()
                .and_then(|r| qtys.get(r))
                .copied()
                .unwrap_or(0) as u32;
            SyndicateStoreItem {
                unique_name: o.unique_name.clone(),
                name: o.name.clone(),
                category: o.category.clone(),
                image_name: o.image_name.clone(),
                tier: o.tier.clone(),
                ducats: o.ducats,
                owned,
                result_unique: o.result_unique.clone(),
                result_owned,
            }
        }).collect();
        result.push(SyndicateStore { name: name.clone(), items });
    }
    result
}

// ─── Research lab stores ─────────────────────────────────────────────────────

/// Returns clan dojo research lab stores, one per lab.
///
/// Items are discovered by scanning the WFCD catalog for unique_name paths that
/// contain the lab's path segment (e.g. ".../BioLab/...").  This is authoritative
/// and self-updating — no item list hardcoding needed.
///
/// For each discovered item:
///   • If a matching "<Name> Blueprint" exists in the catalog:
///     unique_name = blueprint path, result_unique = built-item path
///     → Complete / Blueprint / None status in the UI.
///   • Otherwise (no blueprint entry in WFCD):
///     unique_name = built-item path → Complete / None status.
///
/// Consumable / resource categories (Gear, Resources, Misc) are excluded since
/// owning 0 restores does not mean the research is incomplete.
#[tauri::command]
fn get_research_lab_stores(state: State<AppState>) -> Vec<SyndicateStore> {
    // Hardcoded item display names per lab (base name, no " Blueprint" suffix).
    // Looked up by name in the WFCD catalog; items not found are silently skipped.
    const LABS: &[(&str, &[&str])] = &[
        ("Bio Lab", &[
            // Resources
            "Infested Catalyst", "Mutagen Mass",
            // Consumables
            "Squad Health Restore (Medium)", "Squad Health Restore (Large)",
            // Weapons / Companions
            "Acrid", "Bubonico", "Caustacyst", "Catabolyst", "Cerata",
            "Djinn", "Dual Ichor", "Dual Toxocyst", "Embolist", "Hema",
            "Mios", "Mutalist Quanta", "Paracyst", "Phage", "Pox",
            "Pupacyst", "Scoliac", "Synapse", "Torid",
        ]),
        ("Chem Lab", &[
            // Resources
            "Detonite Injector",
            // Consumables
            "Squad Ammo Restore (Medium)", "Squad Ammo Restore (Large)",
            // Weapons
            "Ack & Brunt", "Argonak", "Buzlok", "Grinlok", "Grattler",
            "Ignis", "Ignis Wraith", "Javlok", "Jat Kittag", "Jat Kusar",
            "Kesheg", "Knux", "Kohmak", "Marelok", "Nukor",
            "Ogris", "Sydon", "Twin Krohkur",
        ]),
        ("Energy Lab", &[
            // Resources
            "Fieldron", "Antiserum Injector",
            // Consumables
            "Squad Shield Restore (Medium)", "Squad Shield Restore (Large)",
            "Squad Energy Restore (Medium)", "Squad Energy Restore (Large)",
            // Weapons / Companions
            "Amprex", "Arca Plasmor", "Arca Scisco", "Battacor", "Convectrix",
            "Cycron", "Cyanex", "Dera", "Dual Cestra", "Falcor",
            "Ferrox", "Flux Rifle", "Glaxion", "Helios", "Komorex",
            "Kreska", "Lanka", "Lenz", "Ocucor", "Opticor",
            "Prova", "Quanta", "Serro", "Spectra", "Staticor", "Supra",
        ]),
        ("Tenno Lab", &[
            // Misc / consumables
            "Air Support Charges", "Cipher", "Synthula", "Loc-Pin", "Gravimag",
            "Calcifin Stim", "Adrenal Stim", "Refract Stim", "Clotra Stim",
            // Segments
            "Kavat Incubator Upgrade Segment", "Landing Craft Foundry Segment",
            "Nutrio Incubator Upgrade Segment",
            // Weapons
            "Akstiletto", "Anku", "Attica", "Baza", "Cassowar",
            "Castanas", "Daikyu", "Dark Split-Sword", "Dual Raza", "Endura",
            "Fluctus", "Gazal Machete", "Guandao", "Gunsen", "Lacera",
            "Larkspur", "Masseter", "Nami Skyla", "Nikana", "Okina",
            "Pyrana", "Scourge", "Shaku", "Silva & Aegis", "Sybaris",
            "Talons", "Tenora", "Tonbo", "Veldt", "Velocitus",
            "Venato", "Venka", "Zakti",
            // Warframes + components
            "Banshee", "Banshee Chassis", "Banshee Neuroptics", "Banshee Systems",
            "Nezha",   "Nezha Chassis",   "Nezha Neuroptics",   "Nezha Systems",
            "Volt",    "Volt Chassis",    "Volt Neuroptics",    "Volt Systems",
            "Wukong",  "Wukong Chassis",  "Wukong Neuroptics",  "Wukong Systems",
            "Zephyr",  "Zephyr Chassis",  "Zephyr Neuroptics",  "Zephyr Systems",
            // Archwings + components
            "Amesha", "Amesha Harness", "Amesha Systems", "Amesha Wings",
            "Elytron", "Elytron Harness", "Elytron Systems", "Elytron Wings",
            "Itzal",   "Itzal Harness",   "Itzal Systems",   "Itzal Wings",
        ]),
        ("Orokin Lab", &[
            "Bleeding Dragon Key", "Decaying Dragon Key",
            "Extinguished Dragon Key", "Hobbled Dragon Key",
        ]),
        ("Ventkids Bash Lab", &[
            // Yareli components (base blueprint from Waverider quest, not dojo)
            "Yareli Neuroptics", "Yareli Chassis", "Yareli Systems",
            // Ghoulsaw + components
            "Ghoulsaw", "Ghoulsaw Blade", "Ghoulsaw Chassis", "Ghoulsaw Engine", "Ghoulsaw Grip",
            // Emotes / cosmetics
            "Greedy Milk", "Hang Tenno", "Puppeteer",
            "Ostron Explorer", "Ostron Gatherer", "Ostron Relaxed", "Ostron Trader Woman",
            "Solaris Foreman", "Solaris Hazard Worker", "Solaris Rig Jockey",
        ]),
        ("Dry Docks", &[
            // Railjack weapons (Mk I/II/III — WFCD uses lowercase roman numerals but lookup is case-insensitive)
            "Apoc Mk I",      "Apoc Mk II",      "Apoc Mk III",
            "Carcinnox Mk I", "Carcinnox Mk II", "Carcinnox Mk III",
            "Cryophon Mk I",  "Cryophon Mk II",  "Cryophon Mk III",
            "Galvarc Mk I",   "Galvarc Mk II",   "Galvarc Mk III",
            "Glazio Mk I",    "Glazio Mk II",    "Glazio Mk III",
            "Laith Mk I",     "Laith Mk II",     "Laith Mk III",
            "Milati Mk I",    "Milati Mk II",    "Milati Mk III",
            "Photor Mk I",    "Photor Mk II",    "Photor Mk III",
            "Pulsar Mk I",    "Pulsar Mk II",    "Pulsar Mk III",
            "Talyn Mk I",     "Talyn Mk II",     "Talyn Mk III",
            "Tycho Seeker Mk I", "Tycho Seeker Mk II", "Tycho Seeker Mk III",
            "Vort Mk I",      "Vort Mk II",      "Vort Mk III",
            // Railjack components
            "Engines Mk I",     "Engines Mk II",     "Engines Mk III",
            "Plating Mk I",     "Plating Mk II",     "Plating Mk III",
            "Reactor Mk I",     "Reactor Mk II",     "Reactor Mk III",
            "Shield Array Mk I","Shield Array Mk II","Shield Array Mk III",
        ]),
        ("Dagath's Hollow", &[
            // Dagath warframe + components
            "Dagath", "Dagath Chassis", "Dagath Neuroptics", "Dagath Systems",
            // Dorrclave weapon + components (components are raw blueprints in WFCD)
            "Dorrclave", "Dorrclave Blade", "Dorrclave Hilt", "Dorrclave Hook", "Dorrclave String",
        ]),
    ];

    // Build reverse ingredient map before acquiring other locks.
    // ingredient_unique_name → parent_unique_name (from ExportRecipes data)
    let ingredient_to_parent: std::collections::HashMap<String, String> = {
        let recipes = state.recipes.lock().unwrap_or_else(|e| e.into_inner());
        let mut map = std::collections::HashMap::new();
        for (parent_unique, components) in recipes.iter() {
            for comp in components {
                map.insert(comp.unique_name.clone(), parent_unique.clone());
            }
        }
        map
    };

    let items = state.wfcd_items.lock().unwrap_or_else(|e| e.into_inner());
    let qtys  = state.current_quantities.lock().unwrap_or_else(|e| e.into_inner());

    // Build lowercase-name → index for blueprint ↔ built-item pairing
    let by_name: std::collections::HashMap<String, usize> = items
        .iter()
        .enumerate()
        .map(|(i, item)| (item.name.to_lowercase(), i))
        .collect();

    LABS.iter().map(|(lab_name, item_names)| {
        let mut store_items: Vec<SyndicateStoreItem> = Vec::new();

        for &base_name in item_names.iter() {
            let bp_key   = format!("{} blueprint", base_name.to_lowercase());
            let item_key = base_name.to_lowercase();

            let (unique_name, owned, result_unique, result_owned, category, image_name) =
                if let Some(&bi) = by_name.get(&bp_key) {
                    // Blueprint found — pair with built item if it exists
                    let bp = &items[bi];
                    let bp_owned = qtys.get(&bp.unique_name).copied().unwrap_or(0) as u32;
                    let (ru, ro, cat, img) = match by_name.get(&item_key) {
                        Some(&wi) => {
                            let w = &items[wi];
                            let ro = qtys.get(&w.unique_name).copied().unwrap_or(0) as u32;
                            (Some(w.unique_name.clone()), ro, w.category.clone(), w.image_name.clone())
                        }
                        None => (None, 0, bp.category.clone(), bp.image_name.clone()),
                    };
                    (bp.unique_name.clone(), bp_owned, ru, ro, cat, img)
                } else if let Some(&wi) = by_name.get(&item_key) {
                    // No separate blueprint entry — track the built item directly
                    let w = &items[wi];
                    let wo = qtys.get(&w.unique_name).copied().unwrap_or(0) as u32;
                    (w.unique_name.clone(), wo, None, 0, w.category.clone(), w.image_name.clone())
                } else {
                    continue; // not in catalog yet, skip silently
                };

            store_items.push(SyndicateStoreItem {
                unique_name,
                name:     base_name.to_string(),
                tier:     category.clone(),
                category,
                image_name,
                ducats:   None,
                owned,
                result_unique,
                result_owned,
            });
        }

        // Post-pass: components consumed during crafting show qty=0 even when the
        // final assembled item is owned. Two sub-passes handle this:
        //
        // Pass A — recipe-based (blueprint+built-item pairs like warframe components):
        //   If the built part is an ingredient in ExportRecipes AND the parent is
        //   currently in qtys, redirect result_unique → parent and set result_owned.
        //   We also set result_unique even when parent_qty==0 so the TypeScript live
        //   inventory lookup fires correctly once a scan runs later.
        //
        // Pass B — name-prefix fallback (directly-tracked items like Dorrclave Blade):
        //   These have result_unique==None; we find the parent item in the same lab
        //   by name prefix and set result_unique to its built unique_name. result_owned
        //   stays 0 so the TypeScript live-inventory path (not the stale Rust qty) is
        //   what decides "complete".

        // Snapshot parent→result_unique map before mutating store_items.
        let parent_ru_map: std::collections::HashMap<String, String> = store_items
            .iter()
            .filter_map(|si| si.result_unique.as_ref().map(|ru| (si.name.clone(), ru.clone())))
            .collect();

        for si in &mut store_items {
            if si.result_owned > 0 { continue; }

            if let Some(built_unique) = si.result_unique.as_deref() {
                // Pass A: warframe/archwing component parts only.
                // Guard on tier=="Parts" so weapons that are ingredients for another weapon
                // (e.g. Kohmak → Twin Kohmak) are not incorrectly redirected.
                if si.tier == "Parts" {
                    if let Some(parent_unique) = ingredient_to_parent.get(built_unique) {
                        let parent_qty = qtys.get(parent_unique).copied().unwrap_or(0) as u32;
                        // Always point at the parent so TypeScript live inventory can pick it up.
                        si.result_unique = Some(parent_unique.clone());
                        if parent_qty > 0 { si.result_owned = parent_qty; }
                    }
                }
            } else {
                // Pass B: directly-tracked item (e.g. Dorrclave Blade) — no built-part pair.
                // First try recipe map by the item's own unique.
                let found_via_recipe = if let Some(parent_unique) =
                    ingredient_to_parent.get(&si.unique_name)
                {
                    let parent_qty = qtys.get(parent_unique).copied().unwrap_or(0) as u32;
                    si.result_unique = Some(parent_unique.clone());
                    if parent_qty > 0 { si.result_owned = parent_qty; }
                    true
                } else { false };

                // Fallback: name-prefix heuristic (catches content not in ExportRecipes).
                if !found_via_recipe {
                    if let Some(parent_ru) = parent_ru_map.iter().find_map(|(pname, ru)| {
                        (si.name.len() > pname.len()
                            && si.name.starts_with(pname.as_str())
                            && si.name.as_bytes().get(pname.len()) == Some(&b' '))
                        .then_some(ru)
                    }) {
                        si.result_unique = Some(parent_ru.clone());
                        // result_owned stays 0 — TypeScript live inventory decides "complete".
                    }
                }
            }
        }

        store_items.sort_by(|a, b| a.tier.cmp(&b.tier).then(a.name.cmp(&b.name)));
        SyndicateStore { name: lab_name.to_string(), items: store_items }
    }).collect()
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

/// Read the riven overlay session log.
#[tauri::command]
fn get_riven_session_log() -> String {
    let path = std::env::temp_dir().join("frameforge_riven_session.txt");
    std::fs::read_to_string(&path)
        .unwrap_or_else(|_| "(no riven session log yet — open the riven reroll screen first)".into())
}

/// Read the current overlay session log.
#[tauri::command]
fn get_overlay_session_log() -> String {
    let path = std::env::temp_dir().join("frameforge_overlay_session.txt");
    std::fs::read_to_string(&path).unwrap_or_else(|_| "(no session log yet — trigger a Void Fissure first)".into())
}

/// Frontend tracing — App.tsx and Overlay.tsx call this to write diagnostic
/// lines into the same session log that gets copied to the diagnostics folder.
#[tauri::command]
fn log_relic_fe(msg: String) {
    let path = std::env::temp_dir().join("frameforge_overlay_session.txt");
    let _ = append_to_file(&path, &format!("[FE] {}\n", msg));
}

/// Force-set the relic-overlay window to HWND_TOPMOST via SetWindowPos.
/// Called from JS on a 150 ms interval while the overlay is visible, to beat
/// Warframe's continuous HWND_TOPMOST reassertion.
#[tauri::command]
fn set_overlay_topmost() {
    #[cfg(target_os = "windows")]
    unsafe {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            FindWindowW, SetWindowPos,
            SWP_NOMOVE, SWP_NOSIZE, SWP_NOACTIVATE,
            HWND_TOPMOST,
        };
        let title: Vec<u16> = "FrameForge Overlay\0".encode_utf16().collect();
        let hwnd = FindWindowW(std::ptr::null(), title.as_ptr());
        if hwnd != 0 {
            SetWindowPos(hwnd, HWND_TOPMOST, 0, 0, 0, 0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
        }
    }
}

/// Diagnostic: position a test window ON TOP OF WARFRAME (finds Warframe's HWND
/// to guarantee the correct monitor) and inject a full-screen coloured div via
/// evaluate_script — bypasses IPC and React entirely (Rust → WebView2 direct).
/// Creates the window from Rust if the pre-declared one doesn't exist.
/// Red = WebView renders, IPC broken. Green = WebView renders, IPC ok.
/// Nothing at all = window creation failed or WebView not rendering.
#[tauri::command]
fn inject_overlay_diagnostic(app: tauri::AppHandle) -> String {
    use tauri::{Manager, PhysicalPosition, PhysicalSize, WebviewWindowBuilder, WebviewUrl};

    // Find Warframe's client area to anchor the diagnostic window to the right monitor.
    #[cfg(target_os = "windows")]
    let (wf_x, wf_y, wf_w, wf_h) = unsafe {
        use windows_sys::Win32::Foundation::{POINT, RECT};
        use windows_sys::Win32::UI::WindowsAndMessaging::{FindWindowW, GetClientRect};
        use windows_sys::Win32::Graphics::Gdi::ClientToScreen;
        let title: Vec<u16> = "Warframe\0".encode_utf16().collect();
        let hwnd = FindWindowW(std::ptr::null(), title.as_ptr());
        if hwnd != 0 {
            let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
            GetClientRect(hwnd, &mut r);
            let mut pt = POINT { x: 0, y: 0 };
            ClientToScreen(hwnd, &mut pt);
            (pt.x, pt.y, (r.right - r.left) as i32, (r.bottom - r.top) as i32)
        } else {
            (0, 0, 1920i32, 1080i32)
        }
    };
    #[cfg(not(target_os = "windows"))]
    let (wf_x, wf_y, wf_w, wf_h) = (0i32, 0i32, 1920i32, 1080i32);

    // Place diagnostic at the vertical centre of the Warframe client area, full width.
    let diag_x = wf_x;
    let diag_y = wf_y + wf_h / 2 - 150;
    let diag_w = wf_w.max(400) as u32;
    let diag_h = 300u32;

    let win = match app.get_webview_window("relic-overlay") {
        Some(w) => w,
        None => {
            // Pre-declared window missing — create a fresh one from Rust.
            match WebviewWindowBuilder::new(&app, "relic-overlay",
                WebviewUrl::App("index.html#overlay".into()))
                .title("FrameForge Overlay")
                .position(diag_x as f64, diag_y as f64)
                .inner_size(diag_w as f64, diag_h as f64)
                .transparent(true).decorations(false)
                .always_on_top(true).skip_taskbar(true)
                .resizable(false).focused(false)
                .build()
            {
                Ok(w) => w,
                Err(e) => return format!("create-err:{e}"),
            }
        }
    };

    let _ = win.set_position(tauri::Position::Physical(PhysicalPosition { x: diag_x, y: diag_y }));
    let _ = win.set_size(tauri::Size::Physical(PhysicalSize { width: diag_w, height: diag_h }));
    let _ = win.set_always_on_top(true);
    let _ = win.show();

    // Give WebView2 a moment to paint before we also eval.
    std::thread::sleep(std::time::Duration::from_millis(200));

    let script = r#"
        (function() {
            document.documentElement.style.cssText = 'margin:0;padding:0;width:100%;height:100%;';
            document.body.style.cssText = 'margin:0;padding:0;background:rgba(200,0,0,0.95);color:#fff;font-family:sans-serif;font-size:26px;font-weight:bold;display:flex;align-items:center;justify-content:center;height:100vh;box-sizing:border-box;';
            document.body.innerHTML = '<span>FF WEBVIEW ALIVE — IPC test pending...</span>';
            try {
                window.__TAURI_INTERNALS__.invoke('log_relic_fe', {msg:'[OV] inject_diagnostic IPC ok'});
                document.body.style.background = 'rgba(0,160,0,0.95)';
                document.body.innerHTML = '<span>FF WEBVIEW — IPC OK (you should see this in green)</span>';
            } catch(e) {
                document.body.innerHTML = '<span>FF WEBVIEW — NO IPC: ' + String(e).slice(0,100) + '</span>';
            }
        })();
    "#;
    match win.eval(script) {
        Ok(_) => format!("eval-ok wf=({wf_x},{wf_y},{wf_w},{wf_h}) diag=({diag_x},{diag_y})"),
        Err(e) => format!("eval-err:{e}"),
    }
}

/// Debug helper: create a test window from Rust side to verify whether JS-side
/// WebviewWindow creation is broken. Returns Ok("created") or Err(reason).
/// Uses a URL hash (#modular) so the Tauri asset protocol serves clean index.html
/// Toggle debug categorization mode. Returns the new state (true = enabled).
#[tauri::command]
fn toggle_debug_categorization(state: State<AppState>) -> bool {
    let prev = state.debug_cat_enabled.fetch_xor(true, Ordering::SeqCst);
    let enabled = !prev;
    info!(debug_cat = enabled, "debug categorization toggled");
    enabled
}

/// and the Tauri init script is injected properly — query strings prevent this.
#[tauri::command]
fn debug_create_window(app: tauri::AppHandle) -> Result<String, String> {
    use tauri::{Manager, WebviewWindowBuilder, WebviewUrl};
    if let Some(existing) = app.get_webview_window("relic-overlay-solid") {
        let _ = existing.close();
        std::thread::sleep(std::time::Duration::from_millis(150));
    }
    WebviewWindowBuilder::new(
        &app,
        "relic-overlay-solid",
        WebviewUrl::App("index.html#modular".into()),
    )
    .title("FF Debug Window — look in taskbar!")
    .inner_size(800.0, 500.0)
    .position(200.0, 200.0)
    .transparent(false)
    .decorations(true)
    .always_on_top(false)
    .skip_taskbar(false)
    .resizable(true)
    .focused(true)
    .build()
    .map(|_| "created".to_string())
    .map_err(|e| format!("build() failed: {e}"))
}

/// Reposition the pre-declared relic-overlay window and bring it on screen.
/// The overlay is pre-declared in tauri.conf.json at y=-3000 (off-screen) so
/// WebView2 initialises at app startup. We never create/destroy it — just move it.
#[tauri::command]
fn show_overlay_window(
    app: tauri::AppHandle,
    x: i32, y: i32, w: u32, h: u32,
) -> Result<(), String> {
    use tauri::Manager;
    let win = app.get_webview_window("relic-overlay")
        .ok_or_else(|| "relic-overlay window not found".to_string())?;
    let _ = win.set_size(tauri::Size::Physical(
        tauri::PhysicalSize { width: w, height: h }
    ));
    let _ = win.set_position(tauri::Position::Physical(
        tauri::PhysicalPosition { x, y }
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
fn move_overlay_offscreen(app: tauri::AppHandle) -> Result<(), String> {
    use tauri::Manager;
    if let Some(win) = app.get_webview_window("relic-overlay") {
        let _ = win.set_position(tauri::Position::Physical(
            tauri::PhysicalPosition { x: 0, y: -3000 }
        ));
    }
    Ok(())
}

/// Show the pre-declared overlay-test window.
/// Pre-declared in tauri.conf.json so WebView2 initialises during app startup
/// (dynamic build() deadlocks because the Win32 event loop can't process messages while
/// the calling closure is running).
#[tauri::command]
fn show_test_overlay_window(app: tauri::AppHandle) -> Result<(), String> {
    use tauri::Manager;
    let win = app.get_webview_window("overlay-test")
        .ok_or_else(|| "overlay-test window not found".to_string())?;
    // Move to a visible position using logical coords (DPI-safe)
    let _ = win.set_position(tauri::Position::Logical(
        tauri::LogicalPosition { x: 400.0, y: 300.0 }
    ));
    let _ = win.set_always_on_top(true);
    let _ = win.set_focus();
    // Log current URL and force navigation in case WebView2 deferred loading while off-screen
    match win.url() {
        Ok(url) => {
            debug!(%url, "current url");
            // Only re-navigate if we're on blank (WebView2 never loaded the app URL)
            if url.as_str() == "about:blank" || url.as_str().starts_with("about:") {
                debug!("was on about:blank, navigating to app URL");
                if let Ok(nav_url) = tauri::Url::parse("http://localhost:1420/index.html?overlaytest") {
                    let _ = win.navigate(nav_url);
                }
            }
        }
        Err(e) => warn!(error = %e, "url() error"),
    }
    debug!("show_test_overlay_window: moved to logical(400,300), alwaysOnTop=true");
    Ok(())
}

/// Move the overlay-test window back off-screen and remove always-on-top.
#[tauri::command]
fn hide_test_overlay_window(app: tauri::AppHandle) -> Result<(), String> {
    use tauri::Manager;
    let win = app.get_webview_window("overlay-test")
        .ok_or_else(|| "overlay-test window not found".to_string())?;
    let _ = win.set_always_on_top(false);
    let _ = win.set_position(tauri::Position::Physical(
        tauri::PhysicalPosition { x: 0, y: -3000 }
    ));
    debug!("hide_test_overlay_window: moved offscreen");
    Ok(())
}

/// Pull and clear the last locked relic reward payload { items, positions }.
/// Overlay.tsx calls this on mount so it never misses rewards that arrived before
/// its relic-rewards listener was registered (the tauri://created → React mount gap).
#[tauri::command]
fn get_pending_relic_rewards(state: State<'_, AppState>) -> Option<serde_json::Value> {
    state.pending_relic_rewards.lock().ok()?.take()
}

// diag_dir() removed — all callers now use state.auto_capture_dir directly.

fn dir_size_bytes(dir: &std::path::Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(dir) else { return 0; };
    entries.filter_map(|e| e.ok()).map(|e| {
        let p = e.path();
        if p.is_dir() { dir_size_bytes(&p) }
        else { std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0) }
    }).sum()
}


/// Return the total size of %TEMP%\warframe-companion\diagnostics\ in bytes.
#[tauri::command]
fn get_diag_folder_size(state: State<AppState>) -> u64 {
    dir_size_bytes(&state.auto_capture_dir)
}

/// Delete all timestamped capture folders inside the auto-capture directory.
/// Returns the size after deletion (always 0 on success).
#[tauri::command]
fn clear_diag_folder(state: State<AppState>) -> u64 {
    let dir = state.auto_capture_dir.clone();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.filter_map(|e| e.ok()) {
            let p = entry.path();
            if p.is_dir() { let _ = std::fs::remove_dir_all(&p); }
            else          { let _ = std::fs::remove_file(&p); }
        }
    }
    0
}

/// Minimal HTTP file server for the local image cache.
/// Accepts GET /{filename} and serves files from `cache_dir`.
async fn serve_image_files(listener: tokio::net::TcpListener, cache_dir: PathBuf) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let cache_dir = Arc::new(cache_dir);
    loop {
        let Ok((mut stream, _)) = listener.accept().await else { continue };
        let dir = Arc::clone(&cache_dir);
        tokio::spawn(async move {
            let mut buf = vec![0u8; 512];
            let n = match stream.read(&mut buf).await {
                Ok(n) if n > 0 => n,
                _ => return,
            };
            let req = std::str::from_utf8(&buf[..n]).unwrap_or("");
            let filename = match req.lines().next()
                .and_then(|l| l.strip_prefix("GET /"))
                .and_then(|l| l.split_whitespace().next())
            {
                Some(f) if !f.is_empty() && !f.contains("..") && !f.contains('/') && !f.contains('\\') => f,
                _ => {
                    let _ = stream.write_all(b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\n\r\n").await;
                    return;
                }
            };
            match tokio::fs::read(dir.join(filename)).await {
                Ok(data) => {
                    let mime = if filename.ends_with(".png") { "image/png" }
                        else if filename.ends_with(".jpg") || filename.ends_with(".jpeg") { "image/jpeg" }
                        else if filename.ends_with(".webp") { "image/webp" }
                        else { "application/octet-stream" };
                    let header = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nContent-Length: {}\r\nCache-Control: public, max-age=86400\r\n\r\n",
                        mime, data.len()
                    );
                    let _ = stream.write_all(header.as_bytes()).await;
                    let _ = stream.write_all(&data).await;
                }
                Err(_) => {
                    let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n").await;
                }
            }
        });
    }
}

/// Returns the base URL of the local image server, e.g. "http://127.0.0.1:51234".
/// Frontend uses this as `${baseUrl}/${imageName}` to load cached images from disk.
#[tauri::command]
fn get_img_cache_dir(state: State<AppState>) -> String {
    let port = *state.img_server_port.lock().unwrap();
    format!("http://127.0.0.1:{}", port)
}

/// Download images for all craftable items that aren't already cached to disk.
/// Returns immediately — downloads happen on background threads (8 in parallel).
/// Safe to call every startup; already-cached files are skipped via existence check.
#[tauri::command]
async fn prewarm_image_cache(state: tauri::State<'_, AppState>) -> Result<(), String> {
    use std::collections::HashSet;
    use std::sync::Arc;
    let items: Vec<_> = state.wfcd_items.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let recipe_names: HashSet<String> = state.recipes.lock()
        .unwrap_or_else(|e| e.into_inner()).keys().cloned().collect();
    let cache_dir = Arc::new(state.img_cache_dir.clone());

    tokio::task::spawn_blocking(move || {
        use std::io::Read;
        let names: Vec<String> = items.iter()
            .filter(|i| recipe_names.contains(&i.unique_name))
            .filter_map(|i| i.image_name.clone())
            .collect::<HashSet<_>>()
            .into_iter()
            .filter(|n| !cache_dir.join(n).exists())
            .collect();

        if names.is_empty() { return; }
        debug!(count = names.len(), "prewarming images in background");

        for chunk in names.chunks(8) {
            let handles: Vec<_> = chunk.iter().map(|name| {
                let dir = Arc::clone(&cache_dir);
                let name = name.clone();
                std::thread::spawn(move || {
                    let url = format!("https://cdn.warframestat.us/img/{}", name);
                    if let Ok(resp) = ureq::get(&url).call() {
                        let mut buf = Vec::new();
                        if resp.into_reader().read_to_end(&mut buf).is_ok() {
                            let _ = std::fs::write(dir.join(&name), buf);
                        }
                    }
                })
            }).collect();
            for h in handles { let _ = h.join(); }
        }
        debug!("prewarm complete");
    }); // intentionally not awaited — fire and forget

    Ok(())
}

#[tauri::command]
fn open_debug_folder(state: State<AppState>, which: String) -> Result<(), String> {
    let path: std::path::PathBuf = match which.as_str() {
        "blobs"           => state.blob_log_dir.clone(),
        "api_logs"        => state.api_log_dir.clone(),
        "raw_scan"        => state.raw_scan_path.parent().ok_or("no parent")?.to_path_buf(),
        "probe"           => state.memory_probe_path.parent().ok_or("no parent")?.to_path_buf(),
        "diag"            => state.auto_capture_dir.clone(),
        "manual_capture"  => state.manual_capture_dir.clone(),
        "unmatched_paths" => state.unmatched_paths_dir.clone(),
        _ => return Err("Unknown debug folder".into()),
    };
    std::fs::create_dir_all(&path).ok();
    std::process::Command::new("explorer")
        .arg(path.to_string_lossy().as_ref())
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Clear debug data for a specific category.
/// `which`: "blobs" | "api_logs" | "raw_scan" | "probe"
#[tauri::command]
fn clear_debug_data(state: State<AppState>, which: String) -> Result<(), String> {
    let clear_dir = |dir: &std::path::Path| {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for e in entries.filter_map(|e| e.ok()) {
                let _ = std::fs::remove_file(e.path());
            }
        }
    };
    match which.as_str() {
        "blobs"           => clear_dir(&state.blob_log_dir),
        "api_logs"        => clear_dir(&state.api_log_dir),
        "raw_scan"        => { let _ = std::fs::remove_file(&state.raw_scan_path); }
        "probe"           => { let _ = std::fs::remove_file(&state.memory_probe_path); }
        "unmatched_paths" => clear_dir(&state.unmatched_paths_dir),
        "manual_capture"  => {
            if let Ok(entries) = std::fs::read_dir(&state.manual_capture_dir) {
                for e in entries.filter_map(|e| e.ok()) {
                    let p = e.path();
                    if p.is_dir() { let _ = std::fs::remove_dir_all(&p); }
                    else          { let _ = std::fs::remove_file(&p); }
                }
            }
        }
        _ => return Err("Unknown debug data type".into()),
    }
    Ok(())
}

/// Return the byte size of a debug folder or file.
/// `which`: "blobs" | "api_logs" | "raw_scan" | "probe" | "diag" | "manual_capture" | "unmatched_paths"
#[tauri::command]
fn get_debug_data_size(state: State<AppState>, which: String) -> u64 {
    match which.as_str() {
        "blobs"           => dir_size_bytes(&state.blob_log_dir),
        "api_logs"        => dir_size_bytes(&state.api_log_dir),
        "raw_scan"        => std::fs::metadata(&state.raw_scan_path).map(|m| m.len()).unwrap_or(0),
        "probe"           => std::fs::metadata(&state.memory_probe_path).map(|m| m.len()).unwrap_or(0),
        "diag"            => dir_size_bytes(&state.auto_capture_dir),
        "manual_capture"  => dir_size_bytes(&state.manual_capture_dir),
        "unmatched_paths" => dir_size_bytes(&state.unmatched_paths_dir),
        _ => 0,
    }
}

/// Write BGRA pixels as an uncompressed 24-bit BGR BMP file.
/// BMP is lossless and writes in microseconds regardless of resolution —
/// PNG compression at 2560×1440 blocks for 1–3 s and froze the overlay.
/// 24-bit BGR (BI_RGB) uses a standard 54-byte header with no colour masks,
/// opening correctly in every image viewer.
fn write_bmp(path: &std::path::Path, bgra: &[u8], w: u32, h: u32) -> std::io::Result<()> {
    use std::io::Write;
    // 24-bit BGR rows must be padded to a 4-byte boundary.
    let row_bytes  = (w as usize) * 3;
    let padding    = (4 - (row_bytes % 4)) % 4;
    let padded_row = row_bytes + padding;
    let pixel_data_size = padded_row * h as usize;
    let file_size = 54usize + pixel_data_size;
    let mut f = std::io::BufWriter::new(std::fs::File::create(path)?);
    // BMP file header (14 bytes)
    f.write_all(b"BM")?;
    f.write_all(&(file_size as u32).to_le_bytes())?;
    f.write_all(&[0u8; 4])?;            // reserved
    f.write_all(&54u32.to_le_bytes())?; // pixel data starts immediately after 54-byte header
    // BITMAPINFOHEADER (40 bytes)
    f.write_all(&40u32.to_le_bytes())?;
    f.write_all(&w.to_le_bytes())?;
    f.write_all(&(h as i32).wrapping_neg().to_le_bytes())?; // negative height = top-down
    f.write_all(&1u16.to_le_bytes())?;  // colour planes
    f.write_all(&24u16.to_le_bytes())?; // bits per pixel
    f.write_all(&0u32.to_le_bytes())?;  // BI_RGB — no compression, no extra masks
    f.write_all(&(pixel_data_size as u32).to_le_bytes())?;
    f.write_all(&[0u8; 16])?;           // XPelsPerMeter, YPelsPerMeter, ClrUsed, ClrImportant
    // Pixel data: drop alpha channel (BGRA → BGR), pad each row to 4-byte boundary.
    let pad = [0u8; 4];
    for row in bgra.chunks_exact(w as usize * 4) {
        for px in row.chunks_exact(4) {
            f.write_all(&px[..3])?; // B, G, R
        }
        if padding > 0 { f.write_all(&pad[..padding])?; }
    }
    Ok(())
}

/// Capture a diagnostic bundle: scan log + screenshot of the full Warframe window
/// (including any overlay on top via GDI desktop BitBlt / DXGI fallback).
/// Saves everything to %TEMP%\warframe-companion\diagnostics\<timestamp>\ and
/// returns the folder path so the frontend can show it.
#[tauri::command]
async fn save_auto_diag_capture(state: State<'_, AppState>) -> Result<String, String> {
    // Reuse the frame already captured by the OCR pipeline — no second GPU readback,
    // so no GetDIBits stall that used to freeze the whole PC during fissure VFX.
    let frame = state.last_ocr_frame.lock()
        .ok()
        .and_then(|g| g.clone());
    let auto_capture_dir = state.auto_capture_dir.clone();

    tauri::async_runtime::spawn_blocking(move || {
        let ts = chrono::Local::now().format("%Y-%m-%d_%H-%M-%S").to_string();
        let folder = auto_capture_dir.join(&ts);
        std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;

        let session_log = std::env::temp_dir().join("frameforge_overlay_session.txt");
        if session_log.exists() {
            let _ = std::fs::copy(&session_log, folder.join("ocr_session_log.txt"));
        }

        match frame {
            Some((pixels, w, h)) => {
                let _ = write_bmp(&folder.join("screenshot.bmp"), &pixels, w, h);
            }
            None => {
                let _ = std::fs::write(
                    folder.join("screenshot_note.txt"),
                    "No OCR frame captured yet — trigger a Void Fissure first.",
                );
            }
        }

        Ok(folder.to_string_lossy().into_owned())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn capture_diagnostics(state: State<'_, AppState>) -> Result<String, String> {
    let log_path          = state.log_path.clone();
    let changes_path      = state.changes_log_path.clone();
    let manual_capture_dir = state.manual_capture_dir.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let ts = chrono::Local::now().format("%Y-%m-%d_%H-%M-%S").to_string();
        let folder = manual_capture_dir.join(&ts);
        std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;

        if log_path.exists()     { let _ = std::fs::copy(&log_path,     folder.join("scan_log.txt")); }
        if changes_path.exists() { let _ = std::fs::copy(&changes_path, folder.join("changes_log.txt")); }

        // Half-resolution capture: StretchBlt destination is 4× smaller, so GetDIBits
        // reads 4× less data — significantly reduces GPU stall time.
        match ocr::capture_screen_for_diagnostics_half() {
            Ok((pixels_bgra, w, h)) => { let _ = write_bmp(&folder.join("screenshot.bmp"), &pixels_bgra, w, h); }
            Err(e) => { let _ = std::fs::write(folder.join("screenshot_error.txt"), &e); }
        }

        Ok(folder.to_string_lossy().into_owned())
    })
    .await
    .map_err(|e| e.to_string())?
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

#[tauri::command]
fn get_system_locale() -> String {
    let mut buf = [0u16; 85]; // LOCALE_NAME_MAX_LENGTH
    let len = unsafe { windows_sys::Win32::Globalization::GetUserDefaultLocaleName(buf.as_mut_ptr(), buf.len() as i32) };
    if len > 1 {
        String::from_utf16_lossy(&buf[..(len as usize - 1)])
    } else {
        "en-US".to_string()
    }
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

fn patch_item_category(name: &str, category: &str, unique_name: &str) -> String {
    if unique_name.contains("/Recipes/") {
        return if name.contains("Blueprint") { "Blueprints".to_string() } else { "Parts".to_string() };
    }
    if name.contains("Blueprint") { "Blueprints".to_string() } else { category.to_string() }
}

/// Load today's relics.run price cache from disk.
/// Returns (by_name, by_slug) or None if missing/stale.
fn load_relics_run_cache(path: &PathBuf) -> Option<(HashMap<String, u32>, HashMap<String, u32>)> {
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
fn fetch_relics_run_data() -> (HashMap<String, u32>, HashMap<String, u32>) {
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

fn load_items_cache(path: &PathBuf) -> Option<Vec<WfcdItem>> {
    let s = std::fs::read_to_string(path).ok()?;
    let arr: Vec<serde_json::Value> = serde_json::from_str(&s).ok()?;
    // If the cache predates the item_type/product_category fields, discard it so
    // a fresh fetch populates the new fields needed by fix_category.
    if arr.first().map_or(false, |v| v.get("item_type").is_none()) {
        let _ = std::fs::remove_file(path);
        return None;
    }
    let items: Vec<WfcdItem> = arr.into_iter().filter_map(|v| {
        let unique_name = v["unique_name"].as_str()?.to_string();
        let raw_name = v["name"].as_str()?.to_string();
        let name = patch_item_name(&unique_name, &raw_name);
        let image_name = v["image_name"].as_str().map(|s| s.to_string());
        let vaulted = v["vaulted"].as_bool();
        let ducats = v["ducats"].as_u64().map(|n| n as u32);
        let raw_cat          = v["category"].as_str()?.to_string();
        let category         = patch_item_category(&name, &raw_cat, &unique_name);
        let item_type        = v["item_type"].as_str().unwrap_or("").to_string();
        let product_category = v["product_category"].as_str().unwrap_or("").to_string();
        let mastery_req       = v["mastery_req"].as_u64().map(|n| n as u32);
        let omega_attenuation = v["omega_attenuation"].as_f64().map(|n| n as f32);
        let fusion_limit      = v["fusion_limit"].as_u64().map(|n| n as u32);
        let max_level_cap     = v["max_level_cap"].as_u64().map(|n| n as u32)
            .or_else(|| if unique_name.contains("/EntratiMech/") { Some(40) } else { None });
        Some(WfcdItem { unique_name, name, category, item_type, product_category, image_name, vaulted, ducats, mastery_req, omega_attenuation, fusion_limit, max_level_cap })
    }).collect();
    if items.is_empty() { None } else { Some(dedup_known_aliases(items)) }
}

/// Remove known duplicate entries caused by the game listing the same warframe under
/// two name orderings (e.g. "Orion & Sirius" vs "Sirius & Orion").
/// Extend this list whenever DE adds another dual-character warframe with swapped names.
fn dedup_known_aliases(mut items: Vec<WfcdItem>) -> Vec<WfcdItem> {
    // Each tuple: (alias to drop, canonical name to keep)
    const ALIASES: &[(&str, &str)] = &[
        ("Orion & Sirius",           "Sirius & Orion"),
        ("Orion & Sirius Blueprint", "Sirius & Orion Blueprint"),
    ];
    for (alias, canonical) in ALIASES {
        let has_canonical = items.iter().any(|i| i.name == *canonical);
        if has_canonical {
            items.retain(|i| &i.name.as_str() != alias);
        }
    }
    items
}

/// Companion API mod copy entry — camelCase so it round-trips through TypeScript without conversion.
#[derive(serde::Serialize, serde::Deserialize, Default, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ApiModCopy {
    unique_name: String,
    rank: Option<u32>,
    count: i64,
}

/// One resolved modular component (Amp Prism/Scaffold/Brace, Kitgun barrel, etc.)
/// stored inside the parent item's cache entry.
#[derive(serde::Serialize, serde::Deserialize, Default, Clone)]
struct ModularPart {
    path: String,
    name: String,
}

/// One item's complete persisted state — all data for a single inventory entry in one place.
#[derive(serde::Serialize, serde::Deserialize, Default, Clone)]
struct CachedItem {
    /// Lotus path — stable cross-session identifier.
    unique_name: String,
    /// Human-readable display name (populated from WFCD catalog when available).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    name: String,
    /// Total owned copies (or quantity for stackable resources).
    #[serde(default)]
    amount: i64,
    /// Mastery rank 0-30 (0 = not mastered or not applicable).
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    mastery_rank: u32,
    /// Socketed Archon Shards (warframes only).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    archon_shards: Vec<memory_scanner::ArchonShard>,
    /// Resolved modular components (Amp Prism/Scaffold/Brace, Kitgun parts, etc.).
    /// Populated from the blob's ModularParts array with names looked up from WFCD + corrections.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    modular_parts: Vec<ModularPart>,
    /// Maximum rank this mod/arcane can reach (from WFCD fusionLimit). Absent for non-mod items.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    mod_max_rank: Option<u32>,
    /// Maximum level cap override (from WFCD maxLevelCap). Only set for items that exceed rank 30
    /// (e.g. Paracesis, Ironbride, Necramechs). Absent when the standard 30-cap applies.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    max_level_cap: Option<u32>,
    /// Mod/arcane rank breakdown: rank (as string) → copy count at that rank.
    /// Present only for mods and arcanes. Sum of values equals `amount`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    mod_ranks: Option<HashMap<String, i64>>,
    /// Number of Forma applied (placeholder — not yet scanned, reserved for future use).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    forma_count: Option<u32>,
    /// True when this warframe has been fed to the Helminth (subsumed).
    #[serde(default, skip_serializing_if = "is_false")]
    subsumed: bool,
    /// Ducat trade-in value from the WFCD catalog (prime parts/blueprints only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ducat_price: Option<u32>,
    /// Last-fetched warframe.market 48-hour median sell price (platinum).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    wfm_price: Option<u32>,
    /// Whether this item is currently vaulted (None = not applicable / unknown).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    vaulted: Option<bool>,
    /// Normalised item category (Warframes, Weapons, Mods, Parts, Blueprints, Resources, …).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    category: String,
    /// True when this item can drop from void relics.
    #[serde(default, skip_serializing_if = "is_false")]
    relic_reward: bool,
    /// True when this item is listed and tradeable on warframe.market.
    /// Set to false if a WFM price fetch confirmed the item is not listed.
    #[serde(default, skip_serializing_if = "is_false")]
    tradeable_wfm: bool,
    /// True when this item was detected via the FlavourItems array (glyphs, skins,
    /// colour palettes, animation sets, etc.).
    #[serde(default, skip_serializing_if = "is_false")]
    is_flavour: bool,
    /// True when this item came from MiscItems (stackable resources/relics) or
    /// FlavourItems/WeaponSkins (occurrence-counted cosmetics). Prevents items
    /// whose Lotus path matches is_unique_path() (e.g. Kubrow Eggs, Kavat Genetic
    /// Codes, helmets under /Lotus/Powersuits/) from being treated as binary-owned
    /// on startup, which would cause spurious 1→N change log entries every session.
    #[serde(default, skip_serializing_if = "is_false")]
    is_stackable: bool,
}

fn is_false(v: &bool) -> bool { !v }

fn is_zero_u32(v: &u32) -> bool { *v == 0 }

/// Full inventory snapshot persisted to disk. Survives app restarts.
#[derive(serde::Serialize, serde::Deserialize, Default, Clone)]
struct InventoryStateCache {
    /// All owned items: unique_name → item entry.
    #[serde(default)]
    items: HashMap<String, CachedItem>,
    /// Player-level mastery rank (separate from per-item ranks).
    #[serde(default)]
    mastery_rank: Option<u32>,
    /// All owned riven mods (veiled and revealed), populated from blob scans.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    rivens: Vec<memory_scanner::BlobRivenEntry>,
}

impl InventoryStateCache {
    /// Derive consumed_suits from items so callers don't need to know the internal layout.
    fn consumed_suits(&self) -> Vec<String> {
        self.items.iter()
            .filter(|(_, v)| v.subsumed)
            .map(|(k, _)| k.clone())
            .collect()
    }
}

/// True for unique items tracked by the unique scanner (warframes, weapons, companions,
/// archwings, sentinels). These are seeded into unique_quantities on startup.
/// Glyphs and sigils are intentionally excluded — they are detected via FlavourItems
/// and seeded through initial_quantities like stackable resources.
fn is_unique_path(p: &str) -> bool {
    p.starts_with("/Lotus/Powersuits/")
        || p.starts_with("/Lotus/Weapons/")
        || p.starts_with("/Lotus/Archwing/")
        || p.starts_with("/Lotus/Types/Sentinels/SentinelPowersuits/")
        || p.starts_with("/Lotus/Types/Sentinels/SentinelWeapons/")
        || p.starts_with("/Lotus/Types/Friendly/")
        || (p.starts_with("/Lotus/Types/Game/CatbrowPet/") && !p.contains("/Colors/"))
        || (p.starts_with("/Lotus/Types/Game/KubrowPet/") && !p.contains("/Colors/"))
        || p.starts_with("/Lotus/Types/Game/CrewShip/")
        || p.starts_with("/Lotus/Types/Enemies/")
}


/// Build a fresh `InventoryStateCache` from a parsed FULL_ACCOUNT blob.
/// All sections are authoritative — this fully replaces scanner-derived data.
fn build_inventory_from_blob(
    blob: &memory_scanner::BlobInventory,
    path_to_name: &HashMap<String, String>,
    path_to_category: &HashMap<String, String>,
    path_to_ducat: &HashMap<String, u32>,
    path_to_vaulted: &HashMap<String, bool>,
    relic_drops: &HashMap<String, Vec<String>>,
    existing_wfm_prices: &HashMap<String, u32>,
    excluded_paths: &std::collections::HashSet<String>,
) -> InventoryStateCache {
    let mut items: HashMap<String, CachedItem> = HashMap::new();

    macro_rules! upsert {
        ($path:expr) => {{
            let p: &str = $path;
            items.entry(p.to_string()).or_insert_with(|| CachedItem {
                unique_name: p.to_string(),
                name: path_to_name.get(p).cloned().unwrap_or_default(),
                ..Default::default()
            })
        }};
    }

    // Currency (virtual paths not in WFCD catalog).
    upsert!("/_currency/Credits").amount     = blob.credits;
    upsert!("/_currency/Endo").amount        = blob.endo;
    upsert!("/_currency/Platinum").amount    = blob.platinum - blob.free_platinum;
    upsert!("/_currency/PlatinumGift").amount = blob.free_platinum;

    // Unique items — binary owned (amount = 1).
    for entry in &blob.unique_items {
        if excluded_paths.contains(&entry.item_type) { continue; }
        let item = upsert!(&entry.item_type);
        item.amount        = 1;
        item.archon_shards = entry.archon_shards.clone();
        if entry.polarized > 0 { item.forma_count = Some(entry.polarized); }
        if !entry.modular_parts.is_empty() {
            item.modular_parts = entry.modular_parts.iter()
                .map(|p| ModularPart {
                    path: p.clone(),
                    name: path_to_name.get(p).cloned().unwrap_or_default(),
                })
                .collect();
        }
    }

    // Subsumed warframes (InfestedFoundry.ConsumedSuits).
    for path in &blob.consumed_suits {
        if excluded_paths.contains(path) { continue; }
        upsert!(path).subsumed = true;
    }

    // Stackable items — resources, relics, blueprints, ayatan, decorations.
    for entry in &blob.stackable_items {
        if excluded_paths.contains(&entry.item_type) { continue; }
        if entry.item_count <= 0 { continue; }
        let item = upsert!(&entry.item_type);
        item.amount      = entry.item_count;
        item.is_stackable = true;
    }

    // Mods and arcanes (merged from RawUpgrades + Upgrades).
    for (path, mc) in &blob.mods {
        if excluded_paths.contains(path) { continue; }
        let item = upsert!(path);
        item.amount    = mc.total;
        item.mod_ranks = Some(mc.by_rank.iter().map(|(&r, &c)| (r.to_string(), c)).collect());
    }

    // Rivens — group by item_type so they land in `items` with mod_ranks.
    // This ensures the startup cache seeds known_mods with riven counts, preventing
    // spurious 0→N change log entries on every app restart.
    let mut riven_counts: HashMap<String, memory_scanner::ModCount> = HashMap::new();
    for riven in &blob.rivens {
        let mc = riven_counts.entry(riven.item_type.clone()).or_default();
        mc.total += riven.count as i64;
        *mc.by_rank.entry(riven.mod_rank).or_insert(0) += riven.count as i64;
    }
    for (path, mc) in &riven_counts {
        if excluded_paths.contains(path) { continue; }
        let item = upsert!(path);
        item.amount    = mc.total;
        item.mod_ranks = Some(mc.by_rank.iter().map(|(&r, &c)| (r.to_string(), c)).collect());
    }

    // FlavourItems (glyphs, palettes, emotes, titles, ship skins) and
    // WeaponSkins (sigils, cosmetic overlays): occurrence count = amount owned.
    for (path, &count) in blob.flavour_items.iter().chain(blob.weapon_skins.iter()) {
        if excluded_paths.contains(path) { continue; }
        let item = upsert!(path);
        item.amount      = count;
        item.is_flavour  = true;
        item.is_stackable = true; // cosmetics can have count > 1; never treat as binary-owned
    }

    // Mastery rank per item from XPInfo.
    for (path, &rank) in &blob.mastery_data {
        if rank > 0 { upsert!(path).mastery_rank = rank; }
    }

    // Catalog-derived fields + carry forward fetched WFM prices.
    for (path, item) in items.iter_mut() {
        item.ducat_price  = path_to_ducat.get(path).copied();
        item.vaulted      = path_to_vaulted.get(path).copied();
        item.category     = path_to_category.get(path).cloned().unwrap_or_default();
        item.relic_reward = relic_drops.contains_key(path.as_str());
        let tradeable = item.ducat_price.is_some()
            || matches!(item.category.as_str(), "Mods" | "Arcanes");
        item.tradeable_wfm = tradeable;
        if tradeable {
            if let Some(&p) = existing_wfm_prices.get(path) { item.wfm_price = Some(p); }
        }
    }

    for path in excluded_paths { items.remove(path); }

    InventoryStateCache {
        items,
        mastery_rank: if blob.mastery_level > 0 { Some(blob.mastery_level) } else { None },
        rivens: blob.rivens.clone(),
    }
}

fn load_inventory_state_cache(path: &PathBuf) -> InventoryStateCache {
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
    let maximized = window.is_maximized().unwrap_or(false);
    let minimized = window.is_minimized().unwrap_or(false);
    let pos  = window.outer_position().ok();
    let size = window.outer_size().ok();

    let result = merge_settings(settings_path, |map| {
        map.insert(format!("{}Maximized", prefix), maximized.into());
        // Only overwrite position/size when not maximised/minimised.
        // Also guard against the Windows minimized sentinel (-32000,-32000) and dummy size (160×28)
        // which can slip through when is_minimized() is unreliable at CloseRequested time.
        if !maximized && !minimized {
            if let Some(p) = pos {
                if p.x > -10_000 && p.y > -10_000 {
                    map.insert(format!("{}X", prefix), p.x.into());
                    map.insert(format!("{}Y", prefix), p.y.into());
                }
            }
            if let Some(s) = size {
                if s.width >= 100 && s.height >= 50 {
                    map.insert(format!("{}Width",  prefix), (s.width  as i64).into());
                    map.insert(format!("{}Height", prefix), (s.height as i64).into());
                }
            }
        }
    });
    if let Err(e) = result {
        warn!(error = %e, "not saving window state");
    }
}

fn restore_window_state(app: &tauri::AppHandle, window: &tauri::WebviewWindow, settings_path: &std::path::Path, prefix: &str, min_w: u32, min_h: u32) {
    let Ok(map) = read_settings_map(settings_path) else { return };

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
        // Guard against Windows' minimized-window sentinel (-32000, -32000) and positions
        // that fall outside every connected monitor (e.g. secondary unplugged since last run).
        if x > -10_000 && y > -10_000 {
            let monitors = app.available_monitors().unwrap_or_default();
            let on_screen = monitors.iter().any(|m| {
                let mp = m.position();
                let ms = m.size();
                x >= mp.x as i64 && x < (mp.x as i64 + ms.width as i64) &&
                y >= mp.y as i64 && y < (mp.y as i64 + ms.height as i64)
            });
            if on_screen {
                let _ = window.set_position(tauri::PhysicalPosition::new(x as i32, y as i32));
            }
            // If off-screen, leave the window at its default centered position.
        }
    }
    if let (Some(w), Some(h)) = (w, h) {
        if w >= min_w && h >= min_h {
            // Clamp to the monitor that contains the window's top-left corner so the
            // bottom edge never ends up off-screen (e.g. a session saved on a 1440p monitor
            // restored on a 768p monitor would otherwise put the bottom 432px off-screen,
            // making the scrollbar unreachable and the bottom of every page inaccessible).
            let monitors = app.available_monitors().unwrap_or_default();
            let wx = x.unwrap_or(0);
            let wy = y.unwrap_or(0);
            let max_h = monitors.iter()
                .find(|m| {
                    let mp = m.position();
                    let ms = m.size();
                    wx >= mp.x as i64 && wx < (mp.x as i64 + ms.width as i64) &&
                    wy >= mp.y as i64 && wy < (mp.y as i64 + ms.height as i64)
                })
                .map(|m| {
                    // Leave 60px for the Windows taskbar (physical pixels, before DPI scale).
                    m.size().height.saturating_sub(60)
                });
            let clamped_h = if let Some(max) = max_h { h.min(max) } else { h };
            let max_w = monitors.iter()
                .find(|m| {
                    let mp = m.position();
                    let ms = m.size();
                    wx >= mp.x as i64 && wx < (mp.x as i64 + ms.width as i64) &&
                    wy >= mp.y as i64 && wy < (mp.y as i64 + ms.height as i64)
                })
                .map(|m| m.size().width);
            let clamped_w = if let Some(max) = max_w { w.min(max) } else { w };
            let _ = window.set_size(tauri::PhysicalSize::new(clamped_w, clamped_h));
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
    let inventory_state_cache_path = data_dir.join("inventory_state_cache.json");
    let settings_path = data_dir.join("settings.json");
    let log_path = data_dir.join("scan_log.txt");
    let changes_log_path = data_dir.join("inventory_changes.txt");
    let debug_root = data_dir.join("Debugging");
    let blob_log_dir = debug_root.join("Inventory Snapshots");
    let api_log_dir = debug_root.join("Api Responses");
    let auto_capture_dir = debug_root.join("Auto-Capture");
    let manual_capture_dir = debug_root.join("Manual Capture");
    let memory_probe_dir = debug_root.join("Memory Probe");
    let raw_scan_dir = debug_root.join("Raw Memory Record");
    let unmatched_paths_dir = debug_root.join("Unmatched Paths");
    let raw_scan_path = raw_scan_dir.join("raw_scan.txt");
    let memory_probe_path = memory_probe_dir.join("memory_probe.txt");
    for dir in &[&blob_log_dir, &api_log_dir, &auto_capture_dir, &manual_capture_dir,
                 &memory_probe_dir, &raw_scan_dir, &unmatched_paths_dir] {
        let _ = std::fs::create_dir_all(dir);
    }
    let wfm_top_cache_path = data_dir.join("wfm_top_cache.json");
    let syndicate_catalog_path = data_dir.join("syndicate_catalog.json");
    let img_cache_dir = data_dir.join("img_cache");
    let _ = std::fs::create_dir_all(&img_cache_dir);
    let auction_ids_path = data_dir.join("auction_ids.json");
    let initial_auction_ids: Vec<String> = std::fs::read_to_string(&auction_ids_path)
        .ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
    let relics_run_prices_cache_path = data_dir.join("relics_run_prices.json");
    let initial_relics_run_prices = load_relics_run_cache(&relics_run_prices_cache_path)
        .map(|(by_name, _)| by_name)
        .unwrap_or_default();

    let conn = db::init_db(&db_path).expect("Failed to initialize database");

    let initial_items = load_items_cache(&items_cache_path)
        .unwrap_or_else(wfcd::fallback_items);
    let initial_weapon_dispositions: HashMap<String, f32> = initial_items.iter()
        .filter_map(|i| i.omega_attenuation.map(|d| (i.unique_name.clone(), d)))
        .collect();
    let initial_recipes = load_recipes_cache(&recipes_cache_path);
    let initial_relic_drops: HashMap<String, Vec<String>> = std::fs::read_to_string(&relic_drops_cache_path)
        .ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
    // Load relic rewards cache. Two invalidation conditions:
    // 1. Format: must contain at least one EE.log path key ("/Lotus/...") — old caches only
    //    had display-name keys and would cause the OCR prefilter to always miss.
    // 2. Age: discard after 24 hours so new relics added with game updates are picked up.
    let initial_relic_rewards: HashMap<String, Vec<wfcd::RelicReward>> = {
        let cache_age_ok = std::fs::metadata(&relic_rewards_cache_path)
            .and_then(|m| m.modified())
            .map(|t| t.elapsed().unwrap_or(std::time::Duration::MAX) < std::time::Duration::from_secs(86_400))
            .unwrap_or(false);
        let loaded: Option<HashMap<String, Vec<wfcd::RelicReward>>> = if cache_age_ok {
            std::fs::read_to_string(&relic_rewards_cache_path)
                .ok().and_then(|s| serde_json::from_str(&s).ok())
        } else {
            None
        };
        match loaded {
            Some(map) if map.keys().any(|k| k.starts_with("/Lotus/")) => map,
            Some(_) => {
                info!("relic_rewards cache is old format (no path keys) — discarding, will regenerate");
                let _ = std::fs::remove_file(&relic_rewards_cache_path);
                HashMap::new()
            }
            None => {
                let _ = std::fs::remove_file(&relic_rewards_cache_path);
                HashMap::new()
            }
        }
    };
    // Load unified inventory state cache. All data lives in items: unique_name → CachedItem.
    let initial_state = load_inventory_state_cache(&inventory_state_cache_path);
    // Stackable resources: non-mod, non-unique paths.
    // Also include items whose path would match is_unique_path but whose category is
    // Blueprints or Parts — e.g. ClanTech blueprints live under /Lotus/Weapons/ClanTech/
    // but are stackable resource-scanner items, not unique weapon instances.
    let initial_quantities: HashMap<String, i64> = initial_state.items.iter()
        .filter(|(k, v)| {
            // FlavourItems (skins/cosmetics) are binary-owned. Load them at qty=1 regardless
            // of mod_ranks (the mod scanner picks them up from RawUpgrades and writes mod_ranks
            // to the cache, which would otherwise exclude them from initial_quantities).
            if v.is_flavour { return true; }
            v.mod_ranks.is_none()
                && (!is_unique_path(k) || matches!(v.category.as_str(), "Blueprints" | "Parts"))
                && v.amount > 0
        })
        .map(|(k, v)| (k.clone(), if v.is_flavour { 1 } else { v.amount }))
        .collect();
    // Unique items: warframes, weapons, companions.
    // Exclude blueprint/parts items even when their path matches is_unique_path.
    let initial_unique: HashMap<String, i64> = initial_state.items.iter()
        .filter(|(k, v)| {
            v.mod_ranks.is_none() && is_unique_path(k) && v.amount > 0
                && !matches!(v.category.as_str(), "Blueprints" | "Parts")
        })
        .map(|(k, _)| (k.clone(), 1i64))
        .collect();
    // Mods and arcanes.
    let initial_mods: HashMap<String, memory_scanner::ModCount> = initial_state.items.iter()
        .filter(|(_, v)| v.mod_ranks.is_some())
        .map(|(k, v)| {
            let mc = memory_scanner::ModCount {
                total: v.amount,
                by_rank: v.mod_ranks.as_ref().unwrap()
                    .iter()
                    .filter_map(|(r, &c)| r.parse::<u8>().ok().map(|rank| (rank, c)))
                    .collect(),
            };
            (k.clone(), mc)
        })
        .collect();
    let initial_syndicate_catalog: HashMap<String, Vec<SyndicateOffer>> = std::fs::read_to_string(&syndicate_catalog_path)
        .ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();

    let corrections = load_corrections(&data_dir.join("corrections.json"));

    tauri::Builder::default()
        .register_uri_scheme_protocol("ffauth", |ctx, req| console_login::handle_ffauth(ctx.app_handle(), &req)) // [console-login feature]
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .manage(AppState {
            db_path,
            items_cache_path,
            recipes_cache_path,
            relic_drops_cache_path,
            relic_rewards_cache_path,
            quantities_cache_path,
            inventory_state_cache_path,
            settings_path,
            log_path,
            changes_log_path,
            conn: Mutex::new(conn),
            wfcd_items: Mutex::new(initial_items),
            recipes: Mutex::new(initial_recipes),
            relic_drops: Mutex::new(initial_relic_drops),
            relic_rewards: Mutex::new(initial_relic_rewards),
            blueprint_to_result: Mutex::new(HashMap::new()),
            wiki_reward_names: Mutex::new(std::collections::HashSet::new()),
            weapon_dispositions: Mutex::new(initial_weapon_dispositions),
            current_quantities: Arc::new(Mutex::new(initial_quantities)),
            unique_quantities: Arc::new(Mutex::new(initial_unique)),
            current_mods: Arc::new(Mutex::new(initial_mods)),
            api_quantities_cache: Arc::new(Mutex::new(HashMap::new())),
            api_mod_copies_cache: Arc::new(Mutex::new(Vec::new())),
            last_ocr_frame: Arc::new(Mutex::new(None)),
            current_crafting: Arc::new(Mutex::new(vec![])),
            monitor_active: Arc::new(AtomicBool::new(false)),
            raw_scan_active: Arc::new(AtomicBool::new(false)),
            raw_scan_path,
            blob_log_enabled: Arc::new(AtomicBool::new(false)),
            blob_log_dir,
            api_log_enabled: Arc::new(AtomicBool::new(false)),
            api_log_dir,
            wfm: Arc::new(Wfm::new()),
            wfm_price_queue: Arc::new(Mutex::new(std::collections::VecDeque::new())),
            wfm_priority_queue: Arc::new(Mutex::new(std::collections::VecDeque::new())),
            wfm_queue_started: Arc::new(AtomicBool::new(false)),
            wfm_top_cache_path,
            syndicate_catalog: Mutex::new(initial_syndicate_catalog),
            syndicate_catalog_path,
            auction_ids: Mutex::new(initial_auction_ids),
            auction_ids_path,
            img_cache_dir,
            img_server_port: Mutex::new(0),
            local_player_name: Arc::new(Mutex::new(None)),
            pending_relic_rewards: Mutex::new(None),
            relics_run_prices: Mutex::new(initial_relics_run_prices),
            relics_run_prices_cache_path,
            worldstate_cache: Mutex::new(None),
            debug_cat_enabled: Arc::new(AtomicBool::new(false)),
            auto_capture_dir,
            manual_capture_dir,
            memory_probe_path,
            unmatched_paths_dir,
            corrections,
        })
        .setup(|app| {
            use tauri::Manager;

            logging::init(app.handle());

            // Spin up a tiny local HTTP server that serves cached item images from disk.
            // This is more reliable than convertFileSrc (which needs assetProtocol scope).
            // Bind the std listener here (sync) to get the port, then convert to tokio
            // inside the spawned async block where the tokio runtime is active.
            {
                let img_cache_dir = app.state::<AppState>().img_cache_dir.clone();
                let std_listener = std::net::TcpListener::bind("127.0.0.1:0")
                    .map_err(|e| e.to_string())?;
                let port = std_listener.local_addr().map_err(|e| e.to_string())?.port();
                *app.state::<AppState>().img_server_port.lock().unwrap() = port;
                tauri::async_runtime::spawn(async move {
                    std_listener.set_nonblocking(true).ok();
                    if let Ok(tokio_listener) = tokio::net::TcpListener::from_std(std_listener) {
                        serve_image_files(tokio_listener, img_cache_dir).await;
                    }
                });
            }

            if let Some(window) = app.get_webview_window("main") {
                let icon = tauri::image::Image::from_bytes(
                    include_bytes!("../icons/icon.png")
                ).map_err(|e| e.to_string())?;
                window.set_icon(icon).map_err(|e| e.to_string())?;

                // Restore saved window geometry, then show (window starts hidden so
                // it doesn't flash at the default position on the primary monitor first)
                let state = app.state::<AppState>();
                restore_window_state(app.handle(), &window, &state.settings_path, "window", 400, 300);
                let _ = window.show();
            }

            // Overlay windows start as visible:false in tauri.conf.json.
            // We call show() here (while they are still at y=-3000) so WebView2 can
            // initialise their content in the background without flashing on screen.
            // We NEVER call hide() on relic-overlay — hiding a transparent WebView2
            // window breaks DirectComposition: JS keeps running but pixels stop reaching
            // the screen.  Instead we park it at y=-3000 and move it on-screen during
            // fissures.  overlay-test is not transparent so hide() is safe for it, but
            // we use the same show()-once pattern for consistency.
            // show() triggers WebView2 initialisation; immediately park off-screen so
            // nothing is visible to the user at startup.
            // We NEVER call hide() on relic-overlay — hiding a transparent WebView2
            // window breaks DirectComposition.
            // Only relic-overlay needs pre-initialization at startup (to avoid the
            // WebView2 init delay on the first fissure).  overlay-test is on-demand only.
            if let Some(win) = app.get_webview_window("relic-overlay") {
                let _ = win.show();
                // Windows may reposition a newly-shown window that is outside all
                // monitors.  Force it back off-screen immediately after show().
                let _ = win.set_position(tauri::Position::Physical(
                    tauri::PhysicalPosition { x: 0, y: -3000 }
                ));
            }

            // Load relics.run prices in the background. On a cache hit (today's file exists)
            // this is just a disk read; on a miss it fetches items.json + price_history.
            {
                let app_handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    let state = app_handle.state::<AppState>();
                    let (by_name, by_slug) = match load_relics_run_cache(&state.relics_run_prices_cache_path) {
                        Some(cached) => cached,
                        None => {
                            let data = tauri::async_runtime::spawn_blocking(fetch_relics_run_data)
                                .await.unwrap_or_default();
                            if !data.0.is_empty() {
                                let today = chrono::Local::now().format("%Y-%m-%d").to_string();
                                let j = serde_json::json!({ "date": today, "by_name": &data.0, "by_slug": &data.1 });
                                if let Ok(s) = serde_json::to_string(&j) {
                                    let _ = std::fs::write(&state.relics_run_prices_cache_path, s);
                                }
                            }
                            data
                        }
                    };
                    if by_name.is_empty() { return; }
                    *state.relics_run_prices.lock().unwrap_or_else(|e| e.into_inner()) = by_name;
                    for (slug, price) in by_slug {
                        if !state.wfm.is_price_cached(&slug) {
                            state.wfm.cache_price(slug, Some(price));
                        }
                    }
                });
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_all_items,
            get_current_quantities,
            get_item_list_status,
            fetch_item_list,
            get_change_log,
            get_tracked_items,
            add_tracked_item,
            remove_tracked_item,
            get_item_snapshots,
            get_trades,
            add_trade,
            delete_trade,
            clear_cache,
            load_settings,
            save_settings,
            read_scan_log,
            log_api_changes,
            dump_memory_probe,
            toggle_raw_scan,
            set_blob_log,
            set_api_log,
            get_app_version,
            set_app_version,
            force_quit,
            get_weapon_catalog,
            get_craftable_items,
            toggle_debug_categorization,
            get_recipe,
            get_recipes_bulk,
            get_relic_drops,
            get_relic_rewards,
            fetch_wfm_items,
            fetch_wfm_price,
            start_wfm_queue,
            wfm_queue_prices,
            wfm_queue_price_priority,
            wfm_get_cached_prices,
            get_wfm_top_items,
            get_item_price,
            wfm_set_status,
            start_log_watcher,
            ocr_riven_log_error,
            start_riven_memory_watcher,
            riven_screen_visible,
            riven_screen_status,
            save_riven_roll,
            get_saved_riven_rolls,
            delete_saved_riven_roll,
            rename_saved_riven_roll,
            get_riven_weapons,
            reload_riven_database,
            analyze_riven,
            ocr_riven_screen,
            get_riven_session_log,
            wfm_debug_dump,
            wfm_get_riven_attributes,
            wfm_get_item_orders,
            wfm_get_item_statistics,
            wfm_open_login_window,
            wfm_close_login_window,
            wfm_receive_jwt,
            wfm_receive_tokens,
            wfm_refresh_token,
            wfm_set_jwt,
            wfm_get_jwt,
            wfm_save_credentials,
            wfm_load_credentials,
            wfm_delete_credentials,
            wfm_login,
            wfm_logout,
            wfm_get_session,
            wfm_fetch_status,
            wfm_get_orders,
            wfm_get_item_info,
            wfm_create_order,
            wfm_update_order,
            wfm_delete_order,
            wfm_create_riven_auction,
            wfm_switch_riven_type,
            wfm_get_my_riven_auctions,
            wfm_delete_auction,
            wfm_update_auction,
            wfm_set_auction_visible,
            scan_warframe_credentials,
            scan_warframe_api_urls,
            warframe_login,
            fetch_warframe_inventory,
            save_mastery_data,
            get_saved_inventory,
            get_rivens,
            get_weapon_dispositions,
            save_api_inventory,
            get_syndicate_stores,
            get_research_lab_stores,
            fetch_worldstate,
            get_warframe_window_rect,
            get_overlay_session_log,
            get_pending_relic_rewards,
            log_relic_fe,
            set_overlay_topmost,
            inject_overlay_diagnostic,
            debug_create_window,
            show_overlay_window,
            move_overlay_offscreen,
            show_test_overlay_window,
            hide_test_overlay_window,
            get_diag_folder_size,
            clear_diag_folder,
            save_auto_diag_capture,
            capture_diagnostics,
            get_img_cache_dir,
            prewarm_image_cache,
            open_debug_folder,
            clear_debug_data,
            get_debug_data_size,
            start_monitor,
            stop_monitor,
            get_monitor_status,
            get_blueprint_names,
            get_system_locale,
            get_current_crafting,
            debug_detect_fissure_era,
            test_relic_pick_overlay,
            debug_ee_log_tail,
            console_login::open_console_login, // [console-login feature]
        ])
        .on_window_event(|window, event| {
            let label = window.label().to_string();
            if label == "main" || label == "modular-popout" {
                let prefix = if label == "main" { "window" } else { "modularWin" };
                match event {
                    tauri::WindowEvent::Moved(_) | tauri::WindowEvent::Resized(_) => {
                        // Persist good position/size eagerly so a subsequent minimize-then-close
                        // doesn't overwrite with sentinel coordinates (-32000,-32000).
                        let app = window.app_handle();
                        if let Some(wv) = app.get_webview_window(&label) {
                            let state = app.state::<AppState>();
                            save_window_state(&wv, &state.settings_path, prefix);
                        }
                    }
                    tauri::WindowEvent::CloseRequested { .. } => {
                        // Do NOT call save_window_state here — window position/size methods
                        // can deadlock when called from within a main-thread event handler.
                        // State is already saved on every Moved/Resized event.
                    }
                    tauri::WindowEvent::Destroyed => {
                        // Kill the process only when the main window is destroyed
                        // (prevents orphaned overlay/modular windows keeping the process alive)
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

#[cfg(test)]
mod settings_merge_tests {
    use super::{merge_settings, read_settings_map};
    use std::path::PathBuf;

    /// Each test gets its own file so they can run in parallel.
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join("frameforge-settings-tests");
        std::fs::create_dir_all(&dir).expect("temp dir is always writable");
        let path = dir.join(format!("{name}.json"));
        let _ = std::fs::remove_file(&path);
        path
    }

    /// A truncated settings.json (crash or app kill mid-write, e.g. during an
    /// update) used to parse as "no settings", and the next merge rewrote the
    /// file from an empty map, wiping tracked and favorites. A file that
    /// exists but does not parse must be left exactly as it is.
    #[test]
    fn a_corrupt_settings_file_is_never_replaced() {
        let path = scratch("corrupt");
        let truncated = r#"{"tracked":["/Lotus/Weapons/Boar"],"favorites":["/Lo"#;
        std::fs::write(&path, truncated).expect("scratch file is writable");

        let result = merge_settings(&path, |map| {
            map.insert("windowX".into(), 10.into());
        });

        assert!(result.is_err(), "merging over an unparseable file must refuse, not wipe");
        let after = std::fs::read_to_string(&path).expect("file still exists");
        assert_eq!(after, truncated, "the corrupt file must be preserved for recovery");
    }

    /// A missing or empty file is an ordinary first launch, not corruption.
    #[test]
    fn a_missing_or_empty_file_is_a_fresh_start() {
        let path = scratch("fresh");
        assert!(read_settings_map(&path).expect("missing file is fine").is_empty());
        std::fs::write(&path, "").expect("scratch file is writable");
        assert!(read_settings_map(&path).expect("empty file is fine").is_empty());
        merge_settings(&path, |map| {
            map.insert("tracked".into(), serde_json::json!(["a"]));
        })
        .expect("merging into a fresh file succeeds");
    }

    #[test]
    fn merging_preserves_unrelated_keys() {
        let path = scratch("preserve");
        std::fs::write(&path, r#"{"tracked":["a"],"favorites":["b"]}"#).expect("scratch file is writable");
        merge_settings(&path, |map| {
            map.insert("windowX".into(), 42.into());
        })
        .expect("merge succeeds");
        let map = read_settings_map(&path).expect("file parses");
        assert_eq!(map["tracked"], serde_json::json!(["a"]));
        assert_eq!(map["favorites"], serde_json::json!(["b"]));
        assert_eq!(map["windowX"], serde_json::json!(42));
    }

    /// save_window_state fires on every window move while save_settings runs on
    /// the command thread. Unserialized, one writer read the file mid-truncate
    /// of the other and resurrected a stale or empty map.
    #[test]
    fn concurrent_merges_do_not_lose_keys() {
        let path = scratch("concurrent");
        std::fs::write(&path, r#"{"tracked":["a"]}"#).expect("scratch file is writable");
        std::thread::scope(|s| {
            for t in 0..8 {
                let path = &path;
                s.spawn(move || {
                    for i in 0..25 {
                        merge_settings(path, |map| {
                            map.insert(format!("k{t}_{i}"), i.into());
                        })
                        .expect("merge never fails on a valid file");
                    }
                });
            }
        });
        let map = read_settings_map(&path).expect("file parses after the storm");
        assert_eq!(map["tracked"], serde_json::json!(["a"]));
        for t in 0..8 {
            for i in 0..25 {
                assert!(map.contains_key(&format!("k{t}_{i}")), "lost k{t}_{i}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_chars_splits_on_characters_not_bytes() {
        assert_eq!(truncate_chars("éé", 3), "éé");
        assert_eq!(truncate_chars("éé", 1), "é");
        assert_eq!(truncate_chars("abc", 2), "ab");
    }

    /// Verbatim OCR for the right-hand card of a reroll comparison screen (Kuva
    /// Bramma, 3840×2160), border and rank pips included as punctuation.
    const KUVA_BRAMMA_CARD_OCR: &str = "\
Kuva Bramma Lexi-
==
fevatin
;
-
+1.4 Punch Through
;
+22.2% Magazine
_
Capacity
-
-
\"
+23.6% Reload Speed ¢ Y
MR11
K Y
S
e
";

    #[test]
    fn wrapped_stat_names_rejoin_without_the_card_border() {
        let joined = join_wrapped_stat_lines(KUVA_BRAMMA_CARD_OCR);
        assert_eq!(
            joined,
            vec![
                "+1.4 Punch Through",
                // Wrapped across two lines with a border fragment between the halves.
                "+22.2% Magazine Capacity",
                // Trailing "¢ Y" is part of the line, so it survives; "MR11" does not.
                "+23.6% Reload Speed ¢ Y",
            ]
        );

        // All three still resolve. Trailing debris is fine, debris *inside* a
        // name ("Magazine _ Capacity") is not.
        assert_eq!(ocr_stat_to_full_with_condition("Magazine Capacity"), "Magazine Size");
        assert_eq!(ocr_stat_to_full_with_condition("Reload Speed ¢ Y"), "Reload Speed");
        assert_eq!(ocr_stat_to_full_with_condition("Punch Through"), "Punch Through");
    }

    /// Both cards of a Kuva Nukor reroll screen. The left card's artwork puts
    /// stray glyphs in front of a sign, and its "Magazine Capacity" wraps.
    #[test]
    fn stats_survive_glyphs_in_front_of_the_sign() {
        let joined = join_wrapped_stat_lines("\
Nukor Mantitin
)
+30.9% Magazine
Capacity
x1.29 Damage to Corpus P
v & -34.3% Critical Chance
H
O\\
M
");
        assert_eq!(
            joined,
            vec![
                "+30.9% Magazine Capacity",
                "x1.29 Damage to Corpus P",
                // Without the prefix trim this line joined onto the multiplier
                // above it, losing both stats in one go.
                "-34.3% Critical Chance",
            ]
        );

        // The new roll, whose only oddity is the element icon read as "W".
        let joined = join_wrapped_stat_lines("\
\\ukor Crita-hexapha
+76.6% Critical Chance
;
+43.3% Status Chance
+39.9% W Heat
p
-74.7% Damage
g
MR13
X N,
");
        assert_eq!(
            joined,
            vec![
                "+76.6% Critical Chance",
                "+43.3% Status Chance",
                "+39.9% W Heat",
                "-74.7% Damage",
            ]
        );
        // Constructed, from the multi-byte glyphs this OCR emits elsewhere on the
        // card: four characters but six bytes, so a byte bound would leave it.
        assert_eq!(
            join_wrapped_stat_lines("x1.29 Damage to Corpus P\n¢ ¥ -34.3% Critical Chance\n"),
            vec!["x1.29 Damage to Corpus P", "-34.3% Critical Chance"]
        );

        assert_eq!(ocr_stat_to_full_with_condition("W Heat"), "Heat");
        assert_eq!(ocr_stat_to_full_with_condition("Damage to Corpus P"), "Damage to Corpus");
    }

    /// The trim that rescues a stat could also destroy one, so it is bounded from
    /// both sides: reach the multiplier in either case, stop at anything wordlike.
    #[test]
    fn debris_trimming_stops_at_a_word_boundary() {
        // The multiplier is matched case-insensitively elsewhere, so debris in
        // front of a capital "X" has to be trimmed too.
        assert_eq!(
            join_wrapped_stat_lines("+50% Critical Chance\nv & X1.29 Damage to Corpus\n"),
            vec!["+50% Critical Chance", "X1.29 Damage to Corpus"]
        );

        // A sign glued to a word is part of it: this is a name wrapping
        // mid-hyphen, and trimming would leave "-1oad Speed".
        assert_eq!(
            join_wrapped_stat_lines("+50% Critical Chance\nRe-1oad Speed\n"),
            vec!["+50% Critical Chance Re-1oad Speed"]
        );

        // The rank label would otherwise trim into "-1" and read as a curse.
        assert_eq!(
            join_wrapped_stat_lines("+50% Critical Chance\nMR-1\n"),
            vec!["+50% Critical Chance"]
        );
    }

    /// Verbatim panel OCR from three reroll screens. The weapon name has to
    /// survive whether or not the grading sheet lists it: "kuva nukor" is not in
    /// the sheet, and reporting the base Nukor in its place would grade the roll
    /// against a different weapon's disposition.
    #[test]
    fn the_panel_yields_the_weapon_name_over_its_own_chrome() {
        let nukor = "o\n=\n\\\n[\"\no\nIN\n\u{fb01} 'A l\u{2019}\u{2019})\n\u{2014}\nKuva Nukor\n";
        assert_eq!(panel_weapon_candidates(nukor).last().unwrap(), "kuva nukor");

        let bramma = "-\nD\n)\nA\n~\n3\n\u{00a5}\nFITSIN\ne\nKuva Bramma\nSHOW RANKED\n";
        assert_eq!(panel_weapon_candidates(bramma).last().unwrap(), "kuva bramma");

        // The single-card screen adds a CLOSE button below SHOW RANKED.
        let single = "\\\nE_ 3\n-\n-~\nFITSIN\n@\nKuva Bramma\nSHOW RANKED\nCLOSE\n";
        assert_eq!(panel_weapon_candidates(single).last().unwrap(), "kuva bramma");

        // A panel that read as nothing but debris must not name a weapon.
        assert!(panel_weapon_candidates("\u{201c} \\\\\n>~ \u{2018}\n").is_empty());
    }

    /// The label is small enough that an engine can close the word gap.
    #[test]
    fn the_fits_in_marker_is_matched_without_its_space() {
        assert!(says_fits_in("fitsin"));
        assert!(says_fits_in("fits in"));
        assert!(says_fits_in("e\nfitsin\nkuva bramma"));
        assert!(!says_fits_in("inventory/mods"));
    }

    /// Titles must not glue onto a stat, and a negative stat is a real curse
    /// rather than junk. The second title is the one that matters: it follows the
    /// card above with no blank line, and is what the "kuva" noise rule holds back.
    #[test]
    fn stat_joining_keeps_curses_and_drops_the_mod_name() {
        let joined = join_wrapped_stat_lines("\
Kuva Bramma Conci-
satitio
+50.3% Electricity
+57% Projectile Speed
+52.4% Multishot
-25.4% Ammo Maximum
Kuva Bramma Lexi-
MR 11
");
        assert_eq!(
            joined,
            vec![
                "+50.3% Electricity",
                "+57% Projectile Speed",
                "+52.4% Multishot",
                "-25.4% Ammo Maximum",
            ]
        );
    }
}

