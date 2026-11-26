use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use crate::mastery::RouteKind;
use crate::mastery_progress::MasteryProgress;
use crate::mastery_rules::Unobtainable;
use crate::wfcd::{DropLocation, RecipeComponent, RelicReward, SyndicateOffer, WfcdItem};
use crate::memory_scanner;
use crate::wfm::Wfm;

use crate::monitor::CraftingJob;
use crate::companion_api::ApiModCopy;

type OcrFrame = (Vec<u8>, u32, u32);
type WorldstateCache = (std::time::Instant, Arc<serde_json::Value>, Arc<serde_json::Value>);

/// Bundled corrections file embedded at compile time. Never absent at runtime.
const BUNDLED_CORRECTIONS: &str = include_str!("../resources/corrections.json");

/// Load and merge corrections: bundled entries first, then user file overrides field by field.
pub(crate) fn load_corrections(user_path: &std::path::Path) -> HashMap<String, CorrectionEntry> {
    let bundled = serde_json::from_str::<Vec<CorrectionEntry>>(BUNDLED_CORRECTIONS).unwrap_or_default();
    let user = std::fs::read_to_string(user_path)
        .ok()
        .and_then(|content| serde_json::from_str::<Vec<CorrectionEntry>>(&content).ok())
        .unwrap_or_default();
    merge_corrections(bundled, user)
}

/// A user entry cannot clear a bundled field by leaving it out. That would need an
/// explicit null convention, and no entry has needed one yet.
fn merge_corrections(bundled: Vec<CorrectionEntry>, user: Vec<CorrectionEntry>) -> HashMap<String, CorrectionEntry> {
    let mut map: HashMap<String, CorrectionEntry> = bundled.into_iter().map(|e| (e.path.clone(), e)).collect();
    for e in user {
        match map.get_mut(&e.path) {
            Some(base) => base.overlay(e),
            None => { map.insert(e.path.clone(), e); }
        }
    }
    map
}

/// One entry in corrections.json — a hand-curated override for a specific Lotus path.
/// Fields are all optional so a minimal entry can omit unused columns.
#[derive(Debug, Clone, Default, serde::Deserialize)]
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
    pub masterable:    Option<bool>,
    pub rank_cap:      Option<u32>,
    pub unobtainable:  Option<Unobtainable>,
    /// Names the source when no dataset does, such as Baro's stock, a quest
    /// or a vendor the catalogue lacks, and wins over the derived route.
    pub route:         Option<RouteKind>,
}

impl CorrectionEntry {
    fn overlay(&mut self, other: CorrectionEntry) {
        self.name          = other.name.or(self.name.take());
        self.category      = other.category.or(self.category.take());
        self.tradeable_wfm = other.tradeable_wfm.or(self.tradeable_wfm);
        self.is_stackable  = other.is_stackable.or(self.is_stackable);
        self.masterable    = other.masterable.or(self.masterable);
        self.rank_cap      = other.rank_cap.or(self.rank_cap);
        self.unobtainable  = other.unobtainable.or(self.unobtainable);
        self.route         = other.route.or(self.route.take());
    }
}

pub struct AppState {
    pub db_path: PathBuf,
    pub items_cache_path: PathBuf,
    pub recipes_cache_path: PathBuf,
    pub relic_drops_cache_path: PathBuf,
    pub drop_locations_cache_path: PathBuf,
    pub relic_rewards_cache_path: PathBuf,
    pub quantities_cache_path: PathBuf,
    pub inventory_state_cache_path: PathBuf,
    pub mastery_progress: Arc<Mutex<MasteryProgress>>,
    pub settings_path: PathBuf,
    pub log_path: PathBuf,
    pub changes_log_path: PathBuf,
    pub conn: Mutex<rusqlite::Connection>,
    pub wfcd_items: Mutex<Vec<WfcdItem>>,
    /// ingredient unique_name → recipes that consume it
    pub recipe_consumers: Mutex<Arc<HashMap<String, Vec<String>>>>,
    /// parent unique_name → recipe component tree
    pub recipes: Mutex<Arc<HashMap<String, Vec<RecipeComponent>>>>,
    /// component unique_name → relic unique_names that drop it
    pub relic_drops: Mutex<HashMap<String, Vec<String>>>,
    /// item or component unique_name → non-relic places it drops
    pub drop_locations: Mutex<Arc<HashMap<String, Vec<DropLocation>>>>,
    /// relic unique_name → sorted reward list (Bronze×3, Silver×2, Gold×1)
    pub relic_rewards: Mutex<HashMap<String, Vec<RelicReward>>>,
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
    /// When true, log every suspected WFM trade whisper (raw EE.log line +
    /// parsed result) to the whisper debug log. Off by default — high-volume
    /// traders generate a lot of these.
    pub wfm_whisper_log_enabled: Arc<AtomicBool>,
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
    pub syndicate_catalog: Mutex<Arc<HashMap<String, Vec<SyndicateOffer>>>>,
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
    pub last_ocr_frame: Arc<Mutex<Option<OcrFrame>>>,
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
    pub worldstate_cache: Mutex<Option<WorldstateCache>>,
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
    /// Set by `poke_scan` to bypass the 5-second PID-check cooldown immediately.
    pub force_pid_check: Arc<AtomicBool>,
    pub arbitration_overlay_enabled: Arc<AtomicBool>,
    /// When false, the Relic Pick Overlay is suppressed even when EE.log triggers it.
    pub relic_pick_overlay_enabled: Arc<AtomicBool>,
    /// When false, all automatically-triggered overlays are suppressed.
    pub overlays_enabled: Arc<AtomicBool>,
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXCAL: &str = "/Lotus/Powersuits/Excalibur/ExcaliburPrime";

    fn bundled_excal() -> CorrectionEntry {
        CorrectionEntry {
            path: EXCAL.into(),
            category: Some("Warframes".into()),
            tradeable_wfm: Some(false),
            masterable: Some(true),
            unobtainable: Some(Unobtainable::Founders),
            ..Default::default()
        }
    }

    #[test]
    fn user_rename_keeps_bundled_fields() {
        let user = CorrectionEntry { path: EXCAL.into(), name: Some("Excal P".into()), ..Default::default() };
        let map = merge_corrections(vec![bundled_excal()], vec![user]);
        let e = &map[EXCAL];
        assert_eq!(e.name.as_deref(), Some("Excal P"));
        assert_eq!(e.category.as_deref(), Some("Warframes"));
        assert_eq!(e.tradeable_wfm, Some(false));
        assert_eq!(e.masterable, Some(true));
        assert_eq!(e.unobtainable, Some(Unobtainable::Founders));
    }

    #[test]
    fn user_overrides_single_field() {
        let user = CorrectionEntry { path: EXCAL.into(), masterable: Some(false), ..Default::default() };
        let map = merge_corrections(vec![bundled_excal()], vec![user]);
        let e = &map[EXCAL];
        assert_eq!(e.masterable, Some(false));
        assert_eq!(e.unobtainable, Some(Unobtainable::Founders));
    }

    #[test]
    fn user_only_path_is_added() {
        let user = CorrectionEntry { path: "/Lotus/New".into(), rank_cap: Some(40), ..Default::default() };
        let map = merge_corrections(vec![bundled_excal()], vec![user]);
        assert_eq!(map.len(), 2);
        assert_eq!(map["/Lotus/New"].rank_cap, Some(40));
    }

    #[test]
    fn user_entry_adds_a_route_to_a_bundled_path() {
        let route = RouteKind::Quest { quest: Some("The Teacher".into()) };
        let user = CorrectionEntry { path: EXCAL.into(), route: Some(route.clone()), ..Default::default() };
        let map = merge_corrections(vec![bundled_excal()], vec![user]);
        assert_eq!((map[EXCAL].route.as_ref(), map[EXCAL].unobtainable), (Some(&route), Some(Unobtainable::Founders)));
        let untouched = merge_corrections(vec![CorrectionEntry { path: EXCAL.into(), route: Some(route.clone()), ..Default::default() }], vec![CorrectionEntry { path: EXCAL.into(), name: Some("Excal P".into()), ..Default::default() }]);
        assert_eq!(untouched[EXCAL].route.as_ref(), Some(&route));
    }

    #[test]
    fn bundled_file_parses() {
        let bundled: Vec<CorrectionEntry> = serde_json::from_str(BUNDLED_CORRECTIONS).expect("bundled corrections.json is valid");
        assert!(bundled.iter().any(|e| e.path == EXCAL && e.unobtainable == Some(Unobtainable::Founders)));
        const MARA_DETRON: &str = "/Lotus/Weapons/VoidTrader/VTDetron";
        const VESPER: &str = "/Lotus/Weapons/Lasria/LasSilencedPistol/LasSilencedPistolWeapon";
        assert!(bundled.iter().any(|e| e.path == MARA_DETRON && e.route == Some(RouteKind::Baro)));
        assert!(bundled.iter().any(|e| e.path == VESPER && e.route == Some(RouteKind::Vendor { syndicate: Some("The Hex".into()), rank: Some(3) })));
        // A vendor outside the syndicate table can never read Available.
        for entry in &bundled {
            if let Some(RouteKind::Vendor { syndicate: Some(name), .. }) = &entry.route {
                assert!(crate::syndicates::syndicate(name).is_some(), "{}: {name} has no standing to read", entry.path);
            }
        }
    }
}
