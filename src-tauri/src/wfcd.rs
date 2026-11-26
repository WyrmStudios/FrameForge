use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Mutex;
use std::io::{Cursor, Read};
use tracing::{info, warn};

use crate::cache::{self, Fetched};

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct WfcdItem {
    pub name: String,
    pub unique_name: String,
    pub category: String,
    /// WFCD `type` field — most granular discriminator (110 values). Always set
    /// by WFCD; empty on a stand-in built from a corrections-table row.
    pub item_type: String,
    /// WFCD `productCategory` field — inventory slot type. Empty string when not set (~93% of items).
    pub product_category: String,
    pub image_name: Option<String>,
    /// Some(true) = vaulted, Some(false) = unvaulted, None = no vault status (non-prime)
    pub vaulted: Option<bool>,
    pub ducats: Option<u32>,
    pub mastery_req: Option<u32>,
    /// Riven disposition multiplier (omegaAttenuation). Present on weapons only.
    pub omega_attenuation: Option<f32>,
    /// Maximum rank a mod can be fused to (fusionLimit from WFCD). Present on mods only.
    pub fusion_limit: Option<u32>,
    /// Maximum level cap override (maxLevelCap from WFCD). Present on items with non-standard max levels.
    pub max_level_cap: Option<u32>,
    /// Whether the item can be traded between players (tradable from WFCD).
    pub tradable: Option<bool>,
    /// Whether levelling this item grants mastery XP (masterable from WFCD).
    pub masterable: Option<bool>,
    /// What the Market charges in credits for the blueprint (bpCost from
    /// WFCD). Also set on dojo research items, where it is the replication
    /// cost rather than a Market price.
    #[serde(default)]
    pub bp_cost: Option<u32>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct RecipeComponent {
    pub unique_name: String,
    pub name: String,
    pub count: u32,
    /// How many of this item you receive when crafted (usually 1, but some recipes produce multiple)
    #[serde(default = "default_one")]
    pub result_count: u32,
    pub components: Vec<RecipeComponent>,
    /// Set on a blueprint node to the build price of the recipe it unlocks.
    /// It stays None when the export carried no price, so an unpriced recipe
    /// reads as unknown and never as free.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credits: Option<u32>,
    /// Set on a blueprint node when the blueprint survives the craft.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub reusable: bool,
}

fn default_one() -> u32 { 1 }

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct RelicReward {
    pub unique_name: String,
    pub name: String,
    /// "Bronze" = Common, "Silver" = Uncommon, "Gold" = Rare
    pub rarity: String,
    pub image_name: Option<String>,
    /// Relics.json's per-refinement drop chance in percent. The table's
    /// rarity labels are unreliable (a 25.33% Intact drop reads "Uncommon"),
    /// so this is the only field the probability maths trusts.
    #[serde(default)]
    pub chance: Option<f64>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SyndicateOffer {
    pub unique_name: String,
    pub name: String,
    pub category: String,
    pub image_name: Option<String>,
    pub tier: String,
    pub ducats: Option<u32>,
    #[serde(default)]
    pub standing: Option<u32>,
    /// For Blueprint items: the unique_name of the item crafted from this blueprint.
    /// None for mods, sigils, and other directly-owned items.
    #[serde(default)]
    pub result_unique: Option<String>,
}

/// A place other than a relic that drops an item, such as a mission
/// rotation, a vendor or a cache. `location` is WFCD's free text.
#[derive(serde::Serialize, serde::Deserialize, Clone, PartialEq, Debug)]
pub struct DropLocation {
    pub location: String,
    pub chance: Option<f64>,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct FetchResult {
    pub items: Vec<WfcdItem>,
    /// parent unique_name → list of components needed to craft it
    pub recipes: HashMap<String, Vec<RecipeComponent>>,
    /// component unique_name → list of relic unique_names that can drop it
    pub relic_drops: HashMap<String, Vec<String>>,
    /// item or component unique_name → every non-relic place it drops, one
    /// entry per location
    #[serde(default)]
    pub drop_locations: HashMap<String, Vec<DropLocation>>,
    /// relic unique_name → 6 rewards sorted Bronze×3, Silver×2, Gold×1
    pub relic_rewards: HashMap<String, Vec<RelicReward>>,
    /// blueprint_unique → (display name, ducats)
    /// Built from ExportRecipes × WFCD display_names. Used to enrich the frontend catalog.
    pub blueprint_names: HashMap<String, (String, Option<u32>)>,
    /// Canonical relic reward display names from the Warframe Wiki Module:Void.
    /// Lower-cased. Used as a name-based whitelist for the overlay catalog so that
    /// path-mismatch issues between ExportRecipes and WFCD never exclude a valid reward.
    pub wiki_reward_names: HashSet<String>,
    /// syndicate name → items available for purchase from that syndicate's store
    pub syndicate_catalog: HashMap<String, Vec<SyndicateOffer>>,
    /// weapon unique_name → omegaAttenuation (riven disposition).
    /// Extracted directly from All.json — no separate ExportWeapons.json fetch needed.
    pub weapon_dispositions: HashMap<String, f32>,
}

/// Fetch the complete list of relic reward display names from the Warframe Wiki's
/// Module:Void Lua table via the MediaWiki API.
/// Returns a set of lower-cased names like "xaku prime neuroptics blueprint".
#[tracing::instrument(level = "debug", skip_all)]
fn fetch_wiki_reward_names() -> HashSet<String> {
    let mut names: HashSet<String> = HashSet::new();

    // ── Source A: Module:Void wikitext ────────────────────────────────────────
    // Structured Lua table with Item + Part fields per relic reward entry.
    let url_mod = "https://wiki.warframe.com/api.php?\
                   action=parse&page=Module:Void&prop=wikitext&format=json";
    if let Some(body) = ureq::get(url_mod)
        .set("User-Agent", "FrameForge/3.1.0")
        .call().ok()
        .and_then(|r| r.into_string().ok())
    {
        let wikitext = serde_json::from_str::<serde_json::Value>(&body)
            .ok()
            .and_then(|v| v["parse"]["wikitext"]["*"].as_str().map(|s| s.to_string()))
            .unwrap_or_default();

        let item_re  = regex::Regex::new(r#"Item\s*=\s*"([^"]+)""#).unwrap();
        let part_re  = regex::Regex::new(r#"Part\s*=\s*"([^"]+)""#).unwrap();
        let block_re = regex::Regex::new(r"\{([^}]+)\}").unwrap();
        for block in block_re.captures_iter(&wikitext) {
            let content = &block[1];
            if let (Some(im), Some(pm)) = (item_re.captures(content), part_re.captures(content)) {
                let item = im[1].trim();
                let part = pm[1].trim();
                let full = if part == "Blueprint" {
                    format!("{} Blueprint", item)
                } else {
                    format!("{} {}", item, part)
                };
                names.insert(full.to_lowercase());
            }
        }
    }

    // ── Source B: Void_Relic/ByRelic rendered HTML ────────────────────────────
    // This page lists every relic with its Common / Uncommon / Rare reward columns.
    // We extract all linked item names from the rendered HTML — these are the
    // canonical display names used on the reward selection screen.
    let url_br = "https://wiki.warframe.com/api.php?\
                  action=parse&page=Void_Relic/ByRelic&prop=text&format=json";
    if let Some(html) = ureq::get(url_br)
        .set("User-Agent", "FrameForge/3.1.0")
        .call().ok()
        .and_then(|r| r.into_string().ok())
        .and_then(|b| {
            serde_json::from_str::<serde_json::Value>(&b).ok()
                .and_then(|v| v["parse"]["text"]["*"].as_str().map(|s| s.to_string()))
        })
    {
        // Extract text from anchor tags inside table cells.
        // Reward names appear as <a ...>Item Name</a> in the Common/Uncommon/Rare columns.
        // We capture every linked name that looks like a relic reward:
        //   • contains "Prime"
        //   • starts with "Forma"
        //   • ends with "Blueprint" or a known component suffix
        let link_re = regex::Regex::new(r#">([^<]{4,60})</a>"#).unwrap();
        for cap in link_re.captures_iter(&html) {
            let text = cap[1].trim();
            let lower = text.to_lowercase();
            let is_reward = lower.contains("prime")
                || lower.starts_with("forma")
                || lower.ends_with("blueprint")
                || lower.ends_with("neuroptics")
                || lower.ends_with("chassis")
                || lower.ends_with("systems")
                || lower.ends_with("barrel")
                || lower.ends_with("receiver")
                || lower.ends_with("stock")
                || lower.ends_with("handle")
                || lower.ends_with("blade")
                || lower.ends_with("carapace")
                || lower.ends_with("cerebrum")
                || lower.ends_with("disc")
                || lower.ends_with("pouch")
                || lower.ends_with("gauntlet")
                || lower.ends_with("wings");
            if is_reward {
                names.insert(lower);
            }
        }
    }

    names
}

/// Hand-curated exceptions where a prime part's ducat value doesn't follow the
/// standard drop-rarity formula (`ducat_value_from_rarities`). Mirrors the Warframe
/// Wiki's `Module:Void/data` `DUCAT_EXCEPTIONS` table. The authoritative copy lives
/// in our own `FrameForgePricing` mirror (`DUCAT_EXCEPTIONS_URL`) — not the wiki
/// directly, so no per-user runtime dependency on it is added — and gets fetched
/// through the same catalogue source pipeline as Relics/ExportRecipes/etc, which
/// means a new exception can ship without a FrameForge release. This bundled copy
/// is only the offline/first-run fallback for when that fetch fails.
/// Keys are lowercased full item+part display names.
const DUCAT_EXCEPTIONS_JSON: &str = include_str!("../resources/ducat_exceptions.json");

fn parse_ducat_exceptions_value(v: &serde_json::Value) -> Option<HashMap<String, u32>> {
    let obj = v.get("exceptions")?.as_object()?;
    Some(
        obj.iter()
            .filter_map(|(k, v)| v.as_u64().map(|n| (k.to_lowercase(), n as u32)))
            .collect(),
    )
}

fn load_ducat_exceptions(fetched: Option<&serde_json::Value>) -> HashMap<String, u32> {
    if let Some(map) = fetched.and_then(parse_ducat_exceptions_value) {
        if !map.is_empty() {
            return map;
        }
    }
    let bundled: serde_json::Value = match serde_json::from_str(DUCAT_EXCEPTIONS_JSON) {
        Ok(v) => v,
        Err(e) => {
            warn!(error = %e, "failed to parse bundled ducat_exceptions.json");
            return HashMap::new();
        }
    };
    parse_ducat_exceptions_value(&bundled).unwrap_or_default()
}

/// Derive a prime part/blueprint's ducat value from the set of rarities it drops as
/// across all relics that carry it. WFCD's `components[]` sub-objects stopped carrying
/// a `ducats` field upstream (same schema change that dropped `name`/`imageName`), and
/// unlike those two fields there is no parent-item fallback — ducat value genuinely
/// isn't stored anywhere else in WFCD's data. The Warframe Wiki computes it the same
/// way: it's a pure function of drop rarity, not an independently tracked number.
/// "Bronze"/"Silver"/"Gold" = Common/Uncommon/Rare (this codebase's naming, matching
/// `RelicReward::rarity`).
fn ducat_value_from_rarities(rarities: &HashSet<&str>) -> Option<u32> {
    let bronze = rarities.contains("Bronze");
    let silver = rarities.contains("Silver");
    let gold = rarities.contains("Gold");
    if bronze && gold { return Some(25); }
    if bronze && silver { return Some(25); }
    if silver && gold { return Some(65); }
    if gold { return Some(100); }
    if silver { return Some(45); }
    if bronze { return Some(15); }
    None
}

// ==============================================================================
// Upstream sources
// ==============================================================================

/// warframe-items stopped publishing the combined `All.json` once it outgrew
/// what the repository would carry, so the catalogue is rebuilt from the
/// per-category files that file used to be a concatenation of.
///
/// `Enemy`, `Node` and `Quests` are deliberately left out: nothing in them can
/// be owned, and node entries are discarded downstream in any case.
const CATEGORY_BASE: &str = "https://raw.githubusercontent.com/WFCD/warframe-items/master/data/json/";
const CATEGORIES: [&str; 21] = [
    "Arcanes",
    "Arch-Gun",
    "Arch-Melee",
    "Archwing",
    "Fish",
    "Gear",
    "Glyphs",
    "Melee",
    "Misc",
    "Mods",
    "Pets",
    "Primary",
    "Railjack",
    "Relics",
    "Resources",
    "Secondary",
    "Sentinels",
    "SentinelWeapons",
    "Sigils",
    "Skins",
    "Warframes",
];
const RECIPES_URLS: [&str; 2] = [
    "https://raw.githubusercontent.com/calamity-inc/warframe-public-export-plus/master/ExportRecipes.json",
    "https://raw.githubusercontent.com/calamity-inc/warframe-public-export-plus/HEAD/ExportRecipes.json",
];
/// DE's own name table: `ExportResources.json` maps a uniqueName to a language
/// key, and `dict.en.json` maps that key to the English display text.
const RESOURCES_URLS: [&str; 2] = [
    "https://raw.githubusercontent.com/calamity-inc/warframe-public-export-plus/master/ExportResources.json",
    "https://raw.githubusercontent.com/calamity-inc/warframe-public-export-plus/HEAD/ExportResources.json",
];
const DICT_EN_URLS: [&str; 2] = [
    "https://raw.githubusercontent.com/calamity-inc/warframe-public-export-plus/master/dict.en.json",
    "https://raw.githubusercontent.com/calamity-inc/warframe-public-export-plus/HEAD/dict.en.json",
];
const SYNDICATES_URL: &str =
    "https://raw.githubusercontent.com/WFCD/warframe-drop-data/gh-pages/data/syndicates.json";
/// Our own mirror (not a third-party dependency — same repo `pricing.rs` already
/// fetches bulk WFM prices from) so the ducat exceptions table can be corrected
/// between app releases instead of requiring a new build for every entry.
const DUCAT_EXCEPTIONS_URL: &str =
    "https://raw.githubusercontent.com/WyrmStudios/FrameForgePricing/main/ducat_exceptions.json";

/// One upstream file, and what its absence costs.
struct SourceSpec {
    /// Names both the stored body and the stored ETag.
    name: String,
    /// Mirrors, tried in order.
    urls: Vec<String>,
    /// A category the catalogue cannot be built without. The extras degrade to
    /// their empty defaults so a syndicate outage does not cost a new user the
    /// item list.
    required: bool,
}

fn source_specs() -> Vec<SourceSpec> {
    let mut specs: Vec<SourceSpec> = CATEGORIES
        .iter()
        .map(|name| SourceSpec {
            name: (*name).to_string(),
            urls: vec![format!("{CATEGORY_BASE}{name}.json")],
            // Arcanes.json occasionally returns HTTP 400 from the upstream repo;
            // don't let one optional category abort the whole catalogue build.
            required: *name != "Arcanes",
        })
        .collect();
    specs.push(SourceSpec {
        name: "ExportRecipes".to_string(),
        urls: RECIPES_URLS.iter().map(|u| u.to_string()).collect(),
        required: false,
    });
    specs.push(SourceSpec {
        name: "ExportResources".to_string(),
        urls: RESOURCES_URLS.iter().map(|u| u.to_string()).collect(),
        required: false,
    });
    specs.push(SourceSpec {
        name: "dict_en".to_string(),
        urls: DICT_EN_URLS.iter().map(|u| u.to_string()).collect(),
        required: false,
    });
    specs.push(SourceSpec {
        name: "syndicates".to_string(),
        urls: vec![SYNDICATES_URL.to_string()],
        required: false,
    });
    specs.push(SourceSpec {
        name: "DucatExceptions".to_string(),
        urls: vec![DUCAT_EXCEPTIONS_URL.to_string()],
        required: false,
    });
    specs.push(SourceSpec {
        // Fallback relic-reward source for when WFCD's own Relics.json ships
        // empty rewards[] (see parse_relics_rewards_from_drop_data). Reuses
        // the same URL get_drop_data/refresh_drop_data fetch independently for
        // RelicHelper — two separate cache layers for the same upstream body
        // is harmless and keeps this ladder free of a cross-module dependency.
        name: "DropData".to_string(),
        urls: vec![DROP_DATA_URL.to_string()],
        required: false,
    });
    specs
}

/// Where the raw upstream bodies live between builds.
///
/// Keeping them means an upstream commit to one file re-downloads that file
/// rather than all twenty-three.
trait BodyStore: Sync {
    fn read(&self, name: &str) -> Option<String>;
    fn write(&self, name: &str, body: &str);
}

struct DiskStore;

impl DiskStore {
    fn dir() -> std::path::PathBuf {
        let dir = crate::paths::cache_dir().join("catalogue");
        let _ = std::fs::create_dir_all(&dir);
        dir
    }

    fn etags_path() -> std::path::PathBuf {
        Self::dir().join("etags.json")
    }

    fn read_etags() -> Option<String> {
        std::fs::read_to_string(Self::etags_path()).ok()
    }

    fn write_etags(etags: &str) {
        if let Err(e) = cache::atomic_write(&Self::etags_path(), etags.as_bytes()) {
            warn!("cannot store catalogue ETags: {e}");
        }
    }
}

pub fn clear_cached_etags() {
    let _ = std::fs::remove_file(DiskStore::etags_path());
}

impl BodyStore for DiskStore {
    fn read(&self, name: &str) -> Option<String> {
        std::fs::read_to_string(Self::dir().join(format!("{name}.json"))).ok()
    }

    fn write(&self, name: &str, body: &str) {
        let path = Self::dir().join(format!("{name}.json"));
        if let Err(e) = cache::atomic_write(&path, body.as_bytes()) {
            warn!("cannot store {name}: {e}");
        }
    }
}

/// A conditional GET, injected so the resolution table below can be tested
/// without a network.
type FetchFn<'a> = dyn Fn(&str, Option<&str>) -> Result<Fetched<String>, String> + Sync + 'a;

/// What the server said when asked about one source.
enum Probe {
    Unchanged,
    /// The body as it arrived, kept unparsed so it reaches the store byte for
    /// byte and is only turned into a tree once.
    Body(String, Option<String>),
    /// Every mirror failed, or answered with something that would not parse.
    Failed(String),
}

/// Ask about one source, taking the first mirror that answers with usable JSON.
#[tracing::instrument(level = "debug", skip_all, fields(source = %spec.name, answer, bytes))]
fn probe_source(spec: &SourceSpec, etag: Option<&str>, force: bool, fetch: &FetchFn<'_>) -> Probe {
    let span = tracing::Span::current();
    let sent = if force { None } else { etag };
    let mut last = format!("{} unavailable", spec.name);
    for url in &spec.urls {
        match fetch(url, sent) {
            Ok(Fetched::NotModified) => {
                span.record("answer", "unchanged");
                return Probe::Unchanged;
            }
            // Validated but not kept: a mirror that answers with something
            // unparseable should fall through to the next one, and the tree is
            // only wanted once, in `resolve_source`.
            Ok(Fetched::New(body, new_etag)) => match serde_json::from_str::<serde::de::IgnoredAny>(&body) {
                Ok(_) => {
                    span.record("answer", "body");
                    span.record("bytes", body.len());
                    return Probe::Body(body, new_etag);
                }
                Err(e) => {
                    warn!("{url}: {e}");
                    last = e.to_string();
                }
            },
            Err(e) => {
                warn!("{url}: {e}");
                last = e;
            }
        }
    }
    span.record("answer", "failed");
    Probe::Failed(last)
}

/// One source's contribution to this build.
struct Resolved {
    json: Option<serde_json::Value>,
    etag: Option<String>,
}

/// Turn a probe into a body, reaching for the stored copy where the server did
/// not supply one.
#[tracing::instrument(level = "debug", skip_all, fields(source = %spec.name, origin))]
fn resolve_source(
    spec: &SourceSpec,
    etag: Option<&str>,
    probe: Probe,
    fetch: &FetchFn<'_>,
    store: &dyn BodyStore,
) -> Resolved {
    let span = tracing::Span::current();
    let kept = || etag.map(str::to_string);
    match probe {
        Probe::Body(body, new_etag) => {
            span.record("origin", "network");
            store.write(&spec.name, &body);
            Resolved { json: serde_json::from_str(&body).ok(), etag: new_etag }
        }
        Probe::Unchanged => match store.read(&spec.name).and_then(|b| serde_json::from_str(&b).ok())
        {
            Some(json) => {
                span.record("origin", "store");
                Resolved { json: Some(json), etag: kept() }
            }
            // The ETag outlived the body it described. Ask again without it.
            None => match probe_source(spec, None, true, fetch) {
                Probe::Body(body, new_etag) => {
                    span.record("origin", "refetched");
                    warn!("{}: the stored body was gone; fetched it again", spec.name);
                    store.write(&spec.name, &body);
                    Resolved { json: serde_json::from_str(&body).ok(), etag: new_etag }
                }
                _ => {
                    span.record("origin", "missing");
                    Resolved { json: None, etag: None }
                }
            },
        },
        Probe::Failed(e) => {
            match store.read(&spec.name).and_then(|b| serde_json::from_str(&b).ok()) {
                Some(json) => {
                    span.record("origin", "stale");
                    warn!("{}: {e}, building from the stored copy", spec.name);
                    Resolved { json: Some(json), etag: kept() }
                }
                None => {
                    span.record("origin", "missing");
                    Resolved { json: None, etag: None }
                }
            }
        }
    }
}

/// Probe every source, four at a time.
///
/// The bodies are megabytes each over one connection apiece, so this is waiting
/// on the network rather than on a CPU.
#[tracing::instrument(level = "debug", skip_all, fields(sources = specs.len(), force))]
fn probe_all(specs: &[SourceSpec], etags: &BTreeMap<String, String>, force: bool, fetch: &FetchFn<'_>) -> Vec<Probe> {
    let next = std::sync::atomic::AtomicUsize::new(0);
    let out: Mutex<Vec<(usize, Probe)>> = Mutex::new(Vec::with_capacity(specs.len()));

    std::thread::scope(|scope| {
        for _ in 0..4.min(specs.len()) {
            scope.spawn(|| loop {
                let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let Some(spec) = specs.get(i) else { return };
                let probe = probe_source(spec, etags.get(&spec.name).map(String::as_str), force, fetch);
                out.lock().unwrap_or_else(|e| e.into_inner()).push((i, probe));
            });
        }
    });

    let mut probes = out.into_inner().unwrap_or_else(|e| e.into_inner());
    probes.sort_by_key(|(i, _)| *i);
    probes.into_iter().map(|(_, p)| p).collect()
}

/// Rebuild the catalogue unless every source says nothing has moved.
///
/// `prev_etags` is what the last successful build stored; `force` asks for the
/// bodies regardless of it, and still uses it to tell an outage apart from a
/// source that was never there.
pub fn fetch_items(prev_etags: Option<&str>, force: bool) -> Result<Fetched<FetchResult>, String> {
    // Keep ETag reads and writes inside the lock so a queued non-forced caller
    // conditionally validates the catalogue produced by the preceding caller.
    static FETCH_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = FETCH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let persisted_etags = DiskStore::read_etags();
    let etags = prev_etags.or(persisted_etags.as_deref());
    let fetched = fetch_items_with(etags, force, &|url, etag| cache::get_conditional(url, etag), &DiskStore)?;
    if let Fetched::New(_, Some(etags)) = &fetched {
        DiskStore::write_etags(etags);
    }
    Ok(fetched)
}

#[tracing::instrument(level = "info", skip_all, fields(force))]
fn fetch_items_with(
    prev_etags: Option<&str>,
    force: bool,
    fetch: &FetchFn<'_>,
    store: &dyn BodyStore,
) -> Result<Fetched<FetchResult>, String> {
    let prev: BTreeMap<String, String> =
        prev_etags.and_then(|s| serde_json::from_str(s).ok()).unwrap_or_default();
    let specs = source_specs();

    let probes = probe_all(&specs, &prev, force, fetch);
    let unchanged = probes.iter().filter(|p| matches!(p, Probe::Unchanged)).count();
    let failed = probes.iter().filter(|p| matches!(p, Probe::Failed(_))).count();
    info!(
        sources = specs.len(),
        unchanged,
        downloaded = specs.len() - unchanged - failed,
        failed,
        "catalogue sources probed"
    );
    if unchanged == probes.len() {
        return Ok(Fetched::NotModified);
    }

    let mut etags: BTreeMap<String, String> = BTreeMap::new();
    let mut bodies: HashMap<&str, serde_json::Value> = HashMap::with_capacity(specs.len());
    for (spec, probe) in specs.iter().zip(probes) {
        let etag = prev.get(&spec.name).map(String::as_str);
        let resolved = resolve_source(spec, etag, probe, fetch, store);
        // A required source that answered on the last build and cannot be
        // reached now would rebuild the catalogue without its recipes or relic
        // rewards, and that hollowed-out payload would then be cached over the
        // good one for a day. Failing instead leaves the ladder to serve the
        // previous copy. Non-required sources (Arcanes) are skipped on failure
        // regardless of whether we hold a cached ETag for them.
        let Some(json) = resolved.json else {
            if spec.required {
                return Err(format!("{} unavailable", spec.name));
            }
            continue;
        };
        if let Some(tag) = resolved.etag {
            etags.insert(spec.name.clone(), tag);
        }
        bodies.insert(spec.name.as_str(), json);
    }

    // The upstream All.json was these same files concatenated, so the catalogue
    // builder still sees one flat item list. Order within it does not matter:
    // no uniqueName appears in two files, and the builder regroups by category.
    let arrays: Vec<&Vec<serde_json::Value>> =
        CATEGORIES.iter().filter_map(|c| bodies.get(c)?.as_array()).collect();
    let mut all_items: Vec<&serde_json::Value> =
        Vec::with_capacity(arrays.iter().map(|a| a.len()).sum());
    for arr in arrays {
        all_items.extend(arr.iter());
    }

    let relics_json = bodies.get("Relics");
    let recipes_json = bodies.get("ExportRecipes");
    let syndicates_json = bodies.get("syndicates");
    let resources_json = bodies.get("ExportResources");
    let dict_json = bodies.get("dict_en");
    let ducat_exceptions_json = bodies.get("DucatExceptions");
    let drop_data_json = bodies.get("DropData");

    info!(raw_items = all_items.len(), "catalogue sources assembled");
    let result = fetch_from_wfcd(
        &all_items, recipes_json, syndicates_json, relics_json, resources_json, dict_json,
        ducat_exceptions_json, drop_data_json,
    )?;
    info!(
        items = result.items.len(),
        recipes = result.recipes.len(),
        relic_rewards = result.relic_rewards.len(),
        syndicates = result.syndicate_catalog.len(),
        dispositions = result.weapon_dispositions.len(),
        "catalogue rebuilt"
    );

    Ok(Fetched::New(result, serde_json::to_string(&etags).ok()))
}

fn strip_tags(s: &str) -> &str {
    if s.starts_with('<') {
        s.find('>').map(|i| s[i + 1..].trim()).unwrap_or(s.trim())
    } else {
        s.trim()
    }
}

/// Every uniqueName → display name lookup the catalogue builder may use.
///
/// WFCD's `components[]` entries stopped carrying a `name`, and a component's
/// path is not a name: DE's path token for a warframe's Neuroptics is
/// `...HelmetComponent`. So a component is named by looking its uniqueName up
/// in a database that holds the real text, never by reading the path.
#[derive(Default)]
struct NameResolver {
    /// DE's ExportResources uniqueName → language key, through dict.en.json.
    /// Covers built parts ("Yareli Prime Neuroptics", "Akstiletto Prime Barrel").
    by_unique: HashMap<String, String>,
    /// ExportRecipes blueprint path → the path it crafts.
    blueprint_result: HashMap<String, String>,
    /// Relics.json `rewards[].item`, keyed by the same uniqueName.
    relic_names: HashMap<String, String>,
    /// Names WFCD itself gives top-level items.
    top_level: HashMap<String, String>,
    /// ExportResources uniqueName → icon file name (last segment of DE's `icon` path).
    icons: HashMap<String, String>,
}

impl NameResolver {
    /// The icon file DE's data gives `unique`. A blueprint has no icon entry of
    /// its own; it wears the icon of what it crafts, as it does in the game
    /// ("Yareli Prime Neuroptics Blueprint" shows the Neuroptics icon).
    fn icon(&self, unique: &str) -> Option<&str> {
        self.icons
            .get(unique)
            .or_else(|| self.icons.get(self.blueprint_result.get(unique)?))
            .map(|s| s.as_str())
    }

    /// The display name DE's data gives `unique`, or `None` when no database
    /// knows it. Callers decide what an unnamed entry means.
    fn resolve(&self, unique: &str) -> Option<String> {
        if let Some(n) = self.by_unique.get(unique) {
            return Some(n.clone());
        }
        // A blueprint has no text of its own in ExportResources: it is named
        // for what it crafts, as the game shows it ("Yareli Prime Neuroptics
        // Blueprint").
        if let Some(result) = self.blueprint_result.get(unique) {
            let crafted = self.by_unique.get(result).or_else(|| self.top_level.get(result));
            if let Some(n) = crafted {
                return Some(if n.ends_with(" Blueprint") { n.clone() } else { format!("{n} Blueprint") });
            }
        }
        self.relic_names.get(unique).or_else(|| self.top_level.get(unique)).cloned()
    }
}

/// ExportResources × dict.en: uniqueName → English name.
fn parse_name_db(
    resources: Option<&serde_json::Value>,
    dict: Option<&serde_json::Value>,
) -> HashMap<String, String> {
    let (Some(resources), Some(dict)) = (resources.and_then(|v| v.as_object()), dict.and_then(|v| v.as_object()))
    else {
        warn!("ExportResources/dict.en unavailable — component names will not resolve");
        return HashMap::new();
    };
    resources
        .iter()
        .filter_map(|(unique, entry)| {
            let key = entry.get("name")?.as_str()?;
            let text = strip_tags(dict.get(key)?.as_str()?);
            (!text.is_empty()).then(|| (unique.clone(), text.to_string()))
        })
        .collect()
}

/// ExportResources: uniqueName → icon file name. The image host serves DE's icons
/// under the bare file name of the `icon` path.
fn parse_icon_db(resources: Option<&serde_json::Value>) -> HashMap<String, String> {
    resources
        .and_then(|v| v.as_object())
        .map(|obj| {
            obj.iter()
                .filter_map(|(unique, entry)| {
                    let file = entry.get("icon")?.as_str()?.rsplit('/').next()?;
                    (!file.is_empty()).then(|| (unique.clone(), file.to_string()))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// ExportRecipes: blueprint path → crafted path, for every entry that has one.
fn parse_blueprint_results(json: Option<&serde_json::Value>) -> HashMap<String, String> {
    json.and_then(|v| v.as_object())
        .map(|obj| {
            obj.iter()
                .filter_map(|(bp, entry)| Some((bp.clone(), entry.get("resultType")?.as_str()?.to_string())))
                .collect()
        })
        .unwrap_or_default()
}

/// Relics.json reward items, uniqueName → name.
fn parse_relic_item_names(json: Option<&serde_json::Value>) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let Some(relics) = json.and_then(|v| v.as_array()) else { return out };
    for relic in relics {
        for reward in relic.get("rewards").and_then(|v| v.as_array()).into_iter().flatten() {
            let Some(item) = reward.get("item") else { continue };
            let (Some(unique), Some(name)) = (
                item.get("uniqueName").and_then(|v| v.as_str()),
                item.get("name").and_then(|v| v.as_str()),
            ) else { continue };
            if !name.is_empty() {
                out.entry(unique.trim().to_string()).or_insert_with(|| name.to_string());
            }
        }
    }
    out
}

/// Fetch the LZMA-compressed Warframe public export index and return a map of
/// endpoint filename → full URL (e.g. "ExportRecipes_en.json!HASH" → full URL).
#[allow(dead_code)]
fn fetch_export_index() -> Result<Vec<String>, String> {
    let index_url = "https://origin.warframe.com/PublicExport/index_en.txt.lzma";
    let resp = ureq::get(index_url)
        .set("User-Agent", "FrameForge/3.1.0")
        .call()
        .map_err(|e| format!("index fetch: {}", e))?;

    let mut compressed = Vec::new();
    resp.into_reader()
        .read_to_end(&mut compressed)
        .map_err(|e| format!("index read: {}", e))?;

    // Decompress LZMA1 "alone" format (13-byte header + raw stream)
    let mut decompressed = Vec::new();
    lzma_rs::lzma_decompress(&mut Cursor::new(&compressed), &mut decompressed)
        .map_err(|e| format!("lzma decompress: {}", e))?;

    let text = String::from_utf8_lossy(&decompressed);
    Ok(text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.trim().to_string())
        .collect())
}

/// One entry from the recipe data: the blueprint consumed + raw ingredients + result count.
struct ExportRecipe {
    blueprint_unique: String,
    ingredients: Vec<(String, u32)>,
    result_count: u32,
}

/// Read DE's recipe export, keyed by resultType (= what gets crafted).
#[tracing::instrument(level = "debug", skip_all)]
fn parse_export_recipes(json: Option<&serde_json::Value>) -> HashMap<String, ExportRecipe> {
    let mut map: HashMap<String, ExportRecipe> = HashMap::new();
    let json = match json {
        Some(j) => j,
        None => return map,
    };

    // warframe-public-export-plus format:
    //   { "/Lotus/Types/Recipes/...Blueprint": { "resultType": "...", "num": 1, "ingredients": [...] } }
    if let Some(obj) = json.as_object() {
        for (blueprint_unique, entry) in obj {
            let result_type = match entry["resultType"].as_str() {
                Some(s) => s.to_string(),
                None => continue,
            };
            let result_count = entry["num"].as_u64().unwrap_or(1) as u32;
            let ingredients: Vec<(String, u32)> = entry["ingredients"]
                .as_array()
                .map(|arr| {
                    arr.iter().filter_map(|ing| {
                        let item_type = ing["ItemType"].as_str()?.to_string();
                        let count = ing["ItemCount"].as_u64().unwrap_or(1) as u32;
                        Some((item_type, count))
                    }).collect()
                })
                .unwrap_or_default();
            if !ingredients.is_empty() {
                map.insert(result_type, ExportRecipe {
                    blueprint_unique: blueprint_unique.clone(),
                    ingredients,
                    result_count,
                });
            }
        }
    }
    map
}

/// Price and reusability of every blueprint in the export, keyed by the
/// blueprint path. Several results have two blueprints (a helmet and its
/// conversion, a lens and its convert recipe), and keying by result keeps
/// only one, so this walks the raw entries instead. An entry without
/// `consumeOnUse` counts as consumable, so a missing blueprint shows as a
/// shortage rather than gating its recipe.
fn blueprint_prices(json: Option<&serde_json::Value>) -> HashMap<&str, (Option<u32>, bool)> {
    json.and_then(|j| j.as_object()).into_iter().flatten()
        .map(|(blueprint, entry)| (blueprint.as_str(), (
            entry["buildPrice"].as_u64().map(|p| p as u32),
            !entry["consumeOnUse"].as_bool().unwrap_or(true),
        )))
        .collect()
}

/// Read the syndicate store catalog from warframe-drop-data/syndicates.json.
/// This covers all vendor-purchased items: sigils, specters, health restores,
/// weapon blueprints, augment mods — items that WFCD's `drops` field mostly omits.
#[tracing::instrument(level = "debug", skip_all)]
fn parse_syndicate_store_catalog(
    json: Option<&serde_json::Value>,
    items: &[WfcdItem],
) -> HashMap<String, Vec<SyndicateOffer>> {
    let json = match json {
        Some(v) => v,
        None => {
            warn!("no syndicates.json available");
            return HashMap::new();
        }
    };

    // Lowercase name → index lookup into items slice
    let by_name: HashMap<String, usize> = items
        .iter()
        .enumerate()
        .map(|(i, item)| (item.name.to_lowercase(), i))
        .collect();

    let mut catalog: HashMap<String, Vec<SyndicateOffer>> = HashMap::new();
    let mut seen: HashSet<(String, String)> = HashSet::new();

    let syndicates_obj = match json.get("syndicates").and_then(|v| v.as_object()) {
        Some(o) => o,
        None => return catalog,
    };

    for (raw_key, entries_val) in syndicates_obj {
        // Normalize "NecraLoid" → "Necraloid" to match SYNDICATE_META keys
        let syn_name = if raw_key == "NecraLoid" {
            "Necraloid".to_string()
        } else {
            raw_key.clone()
        };

        let entries = match entries_val.as_array() {
            Some(a) => a,
            None => continue,
        };

        for entry in entries {
            let raw_item = match entry.get("item").and_then(|v| v.as_str()) {
                Some(s) => s.trim(),
                None => continue,
            };

            // Cephalon Simaris has non-item gate entries like "Complete Natah (Quest)"
            if raw_item.starts_with("Complete ")
                || raw_item.starts_with("Defeat ")
                || raw_item.starts_with("Unlock ")
            {
                continue;
            }

            let raw_place = entry.get("place").and_then(|v| v.as_str()).unwrap_or("");

            // Extract tier from "SyndicateName, Tier" → "Tier".
            // For Cephalon Simaris the place holds the item name as the tier — use "" so
            // all Simaris items land in one un-tiered group.
            let tier = if syn_name == "Cephalon Simaris" {
                String::new()
            } else if raw_place.starts_with(raw_key.as_str()) {
                let after = &raw_place[raw_key.len()..];
                let t = after.trim_start_matches(", ").trim();
                // A few entries read "Rank 4\u{a0}: Protector" or "Rank 2:Valiant";
                // the title after the colon is what the other entries carry.
                match t.strip_prefix("Rank ").and_then(|rest| rest.split_once(':')) {
                    Some((_, title)) => title.trim().to_string(),
                    None => t.to_string(),
                }
            } else {
                String::new()
            };

            let (unique_name, display_name, category, image_name, ducats) =
                resolve_syn_item(raw_item, items, &by_name);

            if seen.insert((syn_name.clone(), unique_name.clone())) {
                catalog.entry(syn_name.clone()).or_default().push(SyndicateOffer {
                    unique_name,
                    name: display_name,
                    category,
                    image_name,
                    tier,
                    ducats,
                    standing: entry["standing"].as_u64().map(|s| s as u32),
                    result_unique: None,
                });
            }
        }
    }

    catalog
}

/// Resolve a syndicates.json display name to a WFCD catalog entry.
/// Returns (unique_name, display_name, category, image_name, ducats).
fn resolve_syn_item(
    raw: &str,
    items: &[WfcdItem],
    by_name: &HashMap<String, usize>,
) -> (String, String, String, Option<String>, Option<u32>) {
    // Normalize internal whitespace (some entries have double spaces)
    let normed: String = raw.split_whitespace().collect::<Vec<_>>().join(" ");

    // Strip leading "Nx " quantity prefix ("5x Roller Specter" → "Roller Specter")
    let no_qty = {
        let mut s = normed.clone();
        if let Some(x_pos) = s.find("x ") {
            if x_pos > 0 && s[..x_pos].chars().all(|c| c.is_ascii_digit()) {
                s = s[x_pos + 2..].trim().to_string();
            }
        }
        s
    };

    // Strip trailing " (Something)" only when string ends with ")" — augment mod labels like
    // "Path of Statues (Atlas)" → "Path of Statues". Does NOT strip "(Large)" from
    // "Squad Health Restore (Large) Blueprint" since that doesn't end with ")".
    let strip_trailing_paren = |s: &str| -> String {
        if s.ends_with(')') {
            if let Some(p) = s.rfind(" (") {
                return s[..p].trim().to_string();
            }
        }
        s.to_string()
    };

    let no_paren = strip_trailing_paren(&normed);
    let no_qty_no_paren = strip_trailing_paren(&no_qty);

    // Strip inline "xN" / "XN" quantity from resource bundle blueprints.
    // syndicates.json: "Tear Azurite x10 Blueprint" → WFCD: "Tear Azurite Blueprint"
    let no_inline_xqty = {
        let suffix = if normed.ends_with(" Blueprint") { " Blueprint" }
                     else if normed.ends_with(" blueprint") { " blueprint" }
                     else { "" };
        if !suffix.is_empty() {
            let base = &normed[..normed.len() - suffix.len()];
            // Try both uppercase " X" and lowercase " x"
            let x_pos = base.rfind(" X").or_else(|| base.rfind(" x"));
            if let Some(xp) = x_pos {
                let digits = &base[xp + 2..];
                if !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()) {
                    format!("{}{}", &base[..xp], suffix)
                } else { normed.clone() }
            } else { normed.clone() }
        } else { normed.clone() }
    };

    let candidates: [&str; 5] = [&normed, &no_qty, &no_paren, &no_qty_no_paren, &no_inline_xqty];

    for &candidate in &candidates {
        if candidate.is_empty() {
            continue;
        }
        if let Some(&idx) = by_name.get(&candidate.to_lowercase()) {
            let item = &items[idx];
            return (
                item.unique_name.clone(),
                item.name.clone(),
                item.category.clone(),
                item.image_name.clone(),
                item.ducats,
            );
        }
    }

    // No WFCD match — create a stub so the item still appears in the completionist view.
    // Stubs use a synthetic unique_name that never matches inventory paths.
    let stub_id = format!(
        "syndicate/stub/{}",
        normed.to_lowercase()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join("_")
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '_')
            .collect::<String>()
    );
    let lower = normed.to_lowercase();
    let category = if lower.contains("sigil") {
        "Sigils"
    } else if lower.contains("specter") {
        "Specters"
    } else if lower.ends_with("blueprint") || lower.contains("blueprint") {
        "Blueprints"
    } else if lower.contains("restore") {
        "Consumables"
    } else {
        "Unknown"
    };
    (stub_id, normed, category.to_string(), None, None)
}

/// Label for a node in a recipe tree: the catalogue's name, else DE's name
/// tables. A uniqueName no table knows is shown as its own last path segment,
/// verbatim, so the gap is visible rather than dressed up as a real name.
fn tree_node_name(unique: &str, display_names: &HashMap<String, String>, resolver: &NameResolver) -> String {
    display_names
        .get(unique)
        .cloned()
        .or_else(|| resolver.resolve(unique))
        .unwrap_or_else(|| {
            warn!(unique, "no name known for recipe ingredient");
            unique.rsplit('/').next().unwrap_or(unique).to_string()
        })
}

/// Build a recipe node. Prefers DE's ExportRecipes for sub-ingredients;
/// falls back to WFCD nested `components` for items not in ExportRecipes.
#[derive(Clone, Copy)]
struct RecipeCtx<'a> {
    display_names: &'a HashMap<String, String>,
    resolver: &'a NameResolver,
    export_recipes: &'a HashMap<String, ExportRecipe>,
}

fn build_recipe_node(
    unique_name: String,
    name: String,
    count: u32,
    wfcd_json: Option<&serde_json::Value>,
    ctx: RecipeCtx<'_>,
    depth: u32,
) -> RecipeComponent {
    let RecipeCtx { display_names, resolver, export_recipes } = ctx;
    if depth > 6 {
        return RecipeComponent { unique_name, name, count, result_count: 1, components: vec![], credits: None, reusable: false };
    }

    let (result_count, components) = if let Some(recipe) = export_recipes.get(&unique_name) {
        // Archwing parts arrive already named "… Blueprint", so the fallback
        // must not append it a second time.
        let blueprint_name = display_names
            .get(&recipe.blueprint_unique)
            .cloned()
            .unwrap_or_else(|| if name.ends_with(" Blueprint") { name.clone() } else { format!("{name} Blueprint") });

        let mut components = vec![RecipeComponent {
            unique_name: recipe.blueprint_unique.clone(),
            name: blueprint_name,
            count: 1,
            result_count: 1,
            components: vec![],
            credits: None,
            reusable: false,
        }];

        for (item_type, item_count) in &recipe.ingredients {
            let item_name = tree_node_name(item_type, display_names, resolver);
            components.push(build_recipe_node(
                item_type.clone(), item_name, *item_count,
                None, ctx, depth + 1,
            ));
        }
        (recipe.result_count, components)
    } else if let Some(json) = wfcd_json {
        let comps = json.get("components")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|c| {
                let cu = c["uniqueName"].as_str()?.trim().to_string();
                // Resolve by uniqueName: the catalog first, then DE's name
                // tables. WFCD's own `name` text on components[] is gone as of
                // the 2026 schema change and untrustworthy even if it returns,
                // so it's not consulted.
                let cn = tree_node_name(&cu, display_names, resolver);
                let cc = c["itemCount"].as_u64().unwrap_or(1) as u32;
                Some(build_recipe_node(cu, cn, cc, Some(c), ctx, depth + 1))
            }).collect())
            .unwrap_or_default();
        (1, comps)
    } else {
        (1, vec![])
    };

    RecipeComponent { unique_name, name, count, result_count, components, credits: None, reusable: false }
}

/// WFCD lists a result's own blueprint as a plain component, so its price and
/// reusability come from the export entry keyed by that blueprint.
fn price_blueprints(nodes: &mut [RecipeComponent], prices: &HashMap<&str, (Option<u32>, bool)>) {
    for node in nodes {
        if let Some(&(credits, reusable)) = prices.get(node.unique_name.as_str()) {
            node.credits = credits;
            node.reusable = reusable;
        }
        price_blueprints(&mut node.components, prices);
    }
}

fn wfcd_category_to_display(wfcd_cat: &str) -> &'static str {
    match wfcd_cat {
        "Misc" | "Resources" => "Resources",
        "Mods" => "Mods",
        "Relics" => "Relics",
        "Warframes" => "Warframes",
        "Primary" => "Primary",
        "Secondary" => "Secondary",
        "Melee" => "Melee",
        "Arcanes" => "Arcanes",
        "Sentinels" | "SentinelWeapons" | "Pets" => "Companions",
        "Archwing" | "Arch-Gun" | "Arch-Melee" => "Archwing",
        "Gear" | "Fish" => "Misc",
        "Sigils" => "Sigils",
        "Glyphs" => "Glyphs",
        "Skins" => "Skins",
        _ => "Misc",
    }
}

/// Read relic → reward mappings from WFCD Relics.json.
/// Each entry is one specific refinement (Bronze/Silver/Gold/Platinum) with a
/// uniqueName that matches the EE.log path exactly — no normalization needed.
#[tracing::instrument(level = "debug", skip_all)]
fn parse_relics_rewards(
    json: Option<&serde_json::Value>,
    image_by_name: &HashMap<String, String>,
) -> HashMap<String, Vec<RelicReward>> {
    let json = match json {
        Some(v) => v,
        None => {
            warn!("no Relics.json available");
            return HashMap::new();
        }
    };

    let relics = match json.as_array() {
        Some(a) => a,
        None => return HashMap::new(),
    };

    let mut result: HashMap<String, Vec<RelicReward>> = HashMap::new();

    for relic in relics {
        // Key by display name ("Lith A1 Intact") so build_relic_pick_payload can join
        // by name from wfcd_items. Accept any uniqueName path format (Lotus or /Game/).
        let relic_name = match relic.get("name").and_then(|v| v.as_str()) {
            Some(n) if !n.is_empty() => n.to_string(),
            _ => continue,
        };

        let rewards_arr = match relic.get("rewards").and_then(|v| v.as_array()) {
            Some(a) => a,
            None => continue,
        };

        // WFCD's rewards[].rarity string never labels the 3-slot Common tier —
        // across the whole Relics.json it only ever emits "Uncommon" or "Rare",
        // which silently collapses Common into Uncommon and breaks any Bronze-
        // tier ducat derivation downstream. Every relic's 6 reward slots always
        // split 3 Common / 2 Uncommon / 1 Rare sharing an identical `chance`
        // value, so derive the tier structurally from that grouping instead of
        // trusting the mislabeled string.
        let mut chance_counts: HashMap<String, usize> = HashMap::new();
        for r in rewards_arr {
            if let Some(chance) = r.get("chance").and_then(|v| v.as_f64()) {
                *chance_counts.entry(format!("{chance:.2}")).or_insert(0) += 1;
            }
        }

        let mut reward_list: Vec<RelicReward> = rewards_arr.iter().filter_map(|r| {
            // Relics.json structure: rewards[].item.name (not rewards[].name)
            let item = r.get("item")?;
            let name = item.get("name").and_then(|v| v.as_str())?.to_string();
            if name.is_empty() { return None; }
            let unique_name = item.get("uniqueName").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let chance = r.get("chance").and_then(|v| v.as_f64());
            let slot_count = chance.and_then(|c| chance_counts.get(&format!("{c:.2}")).copied());
            let rarity = match slot_count {
                Some(3) => "Bronze",
                Some(2) => "Silver",
                Some(1) => "Gold",
                _ => {
                    // Non-standard relic (e.g. a flat equal-odds reward table) —
                    // fall back to WFCD's string label (never "Common", but
                    // harmless here since these relics don't carry standard-
                    // formula ducat items).
                    let rarity_raw = r.get("rarity").and_then(|v| v.as_str()).unwrap_or("Common");
                    match rarity_raw.to_lowercase().as_str() {
                        "uncommon" => "Silver",
                        "rare"     => "Gold",
                        _          => "Bronze",
                    }
                }
            }.to_string();
            let image_name = image_by_name.get(&name.to_lowercase()).cloned()
                .or_else(|| {
                    let no_bp = name.to_lowercase().replace(" blueprint", "");
                    image_by_name.get(&no_bp).cloned()
                });
            Some(RelicReward { unique_name, name, rarity, image_name, chance })
        }).collect();

        reward_list.sort_by_key(|r| match r.rarity.as_str() { "Silver" => 1u8, "Gold" => 2, _ => 0 });
        if !reward_list.is_empty() {
            // Also key by uniqueName (EE.log path) so the OCR prefilter can look
            // up directly by the path it gets from EE.log without any decoding.
            if let Some(unique_name) = relic.get("uniqueName").and_then(|v| v.as_str()) {
                if !unique_name.is_empty() {
                    result.insert(unique_name.to_string(), reward_list.clone());
                }
            }
            result.insert(relic_name, reward_list);
        }
    }

    result
}

/// Fallback relic → reward mappings from `warframe-drop-data`'s `all.json`
/// (`DROP_DATA_URL`), used when WFCD's own Relics.json ships empty `rewards[]`
/// for every entry (observed since 2026-10; see project memory). This source
/// has no relic uniqueName (EE.log path) and no per-reward uniqueName, only
/// display names, so entries are keyed by display name only ("Lith A1
/// Intact") and `unique_name` is resolved through `name_to_unique`, a
/// lowercased item-name → uniqueName index built from the finished catalog.
/// Like WFCD's own data, this source's `rewards[].rarity` string never labels
/// the 3-slot Common tier (it mislabels those "Uncommon" too), so rarity is
/// derived structurally from `chance` grouping, exactly as `parse_relics_rewards`
/// does for WFCD's Relics.json.
/// Map relic display name ("Axi A1 Intact") → uniqueName (EE.log path),
/// built from WFCD's Relics.json top-level fields. Unlike `parse_relics_rewards`,
/// this does not require `rewards[]` to be non-empty — Relics.json currently
/// ships every relic with an empty `rewards[]` (see `parse_relics_rewards_from_drop_data`),
/// but the name/uniqueName pair on the relic entry itself is still intact, so
/// it can still be used to path-key the warframe-drop-data fallback rewards,
/// which otherwise have no uniqueName of their own.
fn build_relic_name_to_path(json: Option<&serde_json::Value>) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let Some(relics) = json.and_then(|v| v.as_array()) else { return out };
    for relic in relics {
        let (Some(name), Some(unique)) = (
            relic.get("name").and_then(|v| v.as_str()),
            relic.get("uniqueName").and_then(|v| v.as_str()),
        ) else { continue };
        if !name.is_empty() && !unique.is_empty() {
            out.insert(name.to_string(), unique.to_string());
        }
    }
    out
}

#[tracing::instrument(level = "debug", skip_all)]
fn parse_relics_rewards_from_drop_data(
    json: Option<&serde_json::Value>,
    image_by_name: &HashMap<String, String>,
    name_to_unique: &HashMap<String, String>,
    relic_name_to_path: &HashMap<String, String>,
) -> HashMap<String, Vec<RelicReward>> {
    let json = match json {
        Some(v) => v,
        None => {
            warn!("no drop-data available for relic rewards fallback");
            return HashMap::new();
        }
    };

    let relics = match json.get("relics").and_then(|v| v.as_array()) {
        Some(a) => a,
        None => return HashMap::new(),
    };

    let mut result: HashMap<String, Vec<RelicReward>> = HashMap::new();

    for relic in relics {
        let tier = relic.get("tier").and_then(|v| v.as_str()).unwrap_or("");
        let relic_name_short = relic.get("relicName").and_then(|v| v.as_str()).unwrap_or("");
        let state = relic.get("state").and_then(|v| v.as_str()).unwrap_or("");
        if tier.is_empty() || relic_name_short.is_empty() || state.is_empty() {
            continue;
        }
        let display_name = format!("{tier} {relic_name_short} {state}");

        let rewards_arr = match relic.get("rewards").and_then(|v| v.as_array()) {
            Some(a) => a,
            None => continue,
        };

        let mut chance_counts: HashMap<String, usize> = HashMap::new();
        for r in rewards_arr {
            if let Some(chance) = r.get("chance").and_then(|v| v.as_f64()) {
                *chance_counts.entry(format!("{chance:.2}")).or_insert(0) += 1;
            }
        }

        let mut reward_list: Vec<RelicReward> = rewards_arr.iter().filter_map(|r| {
            let name = r.get("itemName").and_then(|v| v.as_str())?.to_string();
            if name.is_empty() { return None; }
            let unique_name = name_to_unique.get(&name.to_lowercase()).cloned().unwrap_or_default();
            let chance = r.get("chance").and_then(|v| v.as_f64());
            let slot_count = chance.and_then(|c| chance_counts.get(&format!("{c:.2}")).copied());
            let rarity = match slot_count {
                Some(3) => "Bronze",
                Some(2) => "Silver",
                Some(1) => "Gold",
                _ => {
                    let rarity_raw = r.get("rarity").and_then(|v| v.as_str()).unwrap_or("Common");
                    match rarity_raw.to_lowercase().as_str() {
                        "uncommon" => "Silver",
                        "rare"     => "Gold",
                        _          => "Bronze",
                    }
                }
            }.to_string();
            let image_name = image_by_name.get(&name.to_lowercase()).cloned()
                .or_else(|| {
                    let no_bp = name.to_lowercase().replace(" blueprint", "");
                    image_by_name.get(&no_bp).cloned()
                });
            Some(RelicReward { unique_name, name, rarity, image_name, chance })
        }).collect();

        reward_list.sort_by_key(|r| match r.rarity.as_str() { "Silver" => 1u8, "Gold" => 2, _ => 0 });
        if !reward_list.is_empty() {
            // Also key by uniqueName (EE.log path) when Relics.json still carries
            // the name/uniqueName pair for this relic, so the OCR prefilter
            // (which looks up by path, not display name) can find it — see
            // build_relic_name_to_path.
            if let Some(path) = relic_name_to_path.get(&display_name) {
                result.insert(path.clone(), reward_list.clone());
            }
            result.insert(display_name, reward_list);
        }
    }

    result
}

/// WFCD repeats a resource's whole location table on every recipe that
/// consumes it, so locations dedup on insert. Relic entries dedup where
/// `relic_drops` is built.
fn record_drops(
    unique_name: &str,
    name: &str,
    drops: &[serde_json::Value],
    raw_drop_entries: &mut HashMap<String, Vec<(String, String, String)>>,
    drop_locations: &mut HashMap<String, Vec<DropLocation>>,
) {
    for drop in drops {
        let location = drop.get("location").and_then(|v| v.as_str()).map(str::trim).unwrap_or("");
        match drop.get("uniqueName").and_then(|v| v.as_str()) {
            Some(relic_path) if relic_path.contains("/Game/Projections/") => {
                let rarity_raw = drop.get("rarity").and_then(|v| v.as_str()).unwrap_or("Common");
                let rarity = match rarity_raw.to_lowercase().as_str() {
                    "uncommon" => "Silver",
                    "rare"     => "Gold",
                    _          => "Bronze",
                }.to_string();
                raw_drop_entries
                    .entry(relic_path.to_string())
                    .or_default()
                    .push((unique_name.to_string(), name.to_string(), rarity));
            }
            _ if location.is_empty() => {}
            _ => {
                let locations = drop_locations.entry(unique_name.to_string()).or_default();
                if locations.iter().any(|d| d.location == location) { continue; }
                locations.push(DropLocation { location: location.to_string(), chance: drop.get("chance").and_then(|v| v.as_f64()) });
            }
        }
    }
}

#[tracing::instrument(level = "debug", skip_all)]
#[allow(clippy::too_many_arguments)]
fn fetch_from_wfcd(
    all_items_raw: &[&serde_json::Value],
    recipes_json: Option<&serde_json::Value>,
    syndicates_json: Option<&serde_json::Value>,
    relics_json: Option<&serde_json::Value>,
    resources_json: Option<&serde_json::Value>,
    dict_json: Option<&serde_json::Value>,
    ducat_exceptions_json: Option<&serde_json::Value>,
    drop_data_json: Option<&serde_json::Value>,
) -> Result<FetchResult, String> {
    let mut items: Vec<WfcdItem> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut raw_craftable: Vec<(String, &serde_json::Value)> = Vec::new();
    // relic_path → Vec<(item_unique, item_name, rarity)>
    // Built by inverting each item's drops[] array (item→relics stored per-item).
    // WFCD canonicalizes all refinements under the Bronze path — dedup at build time.
    let mut raw_drop_entries: HashMap<String, Vec<(String, String, String)>> = HashMap::new();
    let mut drop_locations: HashMap<String, Vec<DropLocation>> = HashMap::new();

    // Group items by display category to preserve the two-pass structure below.
    let mut category_map: HashMap<String, Vec<&serde_json::Value>> = HashMap::new();
    for item in all_items_raw.iter() {
        let wfcd_cat = item.get("category").and_then(|v| v.as_str()).unwrap_or("");
        let display_cat = wfcd_category_to_display(wfcd_cat).to_string();
        category_map.entry(display_cat).or_default().push(item);
    }
    let mut all_files: Vec<(String, Vec<&serde_json::Value>)> = category_map.into_iter().collect();
    all_files.sort_by(|a, b| a.0.cmp(&b.0));

    // Pass 1: record all top-level unique_names before processing components.
    // "Bronco" (a standalone Secondary) must be known before it appears as a
    // component of "Akbolto" so it keeps "Secondary" not "Parts".
    let mut top_level_uniques: HashSet<String> = HashSet::new();
    let mut resolver = NameResolver {
        by_unique: parse_name_db(resources_json, dict_json),
        blueprint_result: parse_blueprint_results(recipes_json),
        relic_names: parse_relic_item_names(relics_json),
        top_level: HashMap::new(),
        icons: parse_icon_db(resources_json),
    };
    for (_, arr) in &all_files {
        for item in arr.iter() {
            if let Some(u) = item.get("uniqueName").and_then(|v| v.as_str()) {
                top_level_uniques.insert(u.trim().to_string());
                if let Some(n) = item.get("name").and_then(|v| v.as_str()) {
                    let n = strip_tags(n);
                    if n.len() >= 2 && n != "Blueprint" {
                        resolver.top_level.insert(u.trim().to_string(), n.to_string());
                    }
                }
            }
        }
    }
    let mut unresolved_components: Vec<String> = Vec::new();

    // Pass 2: full processing using cached data
    for (category, arr) in &all_files {
        for item in arr {
            let name = match item.get("name").and_then(|v| v.as_str()) {
                Some(n) => {
                    let s = strip_tags(n);
                    if s.len() < 2 { continue; }
                    if s == "Blueprint" { continue; }
                    s.to_string()
                }
                _ => continue,
            };
            let unique_name = match item.get("uniqueName").and_then(|v| v.as_str()) {
                Some(u) => u.trim().to_string(),
                None => continue,
            };

            let item_type         = item.get("type").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let product_category  = item.get("productCategory").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let image_name        = item.get("imageName").and_then(|v| v.as_str()).map(|s| s.to_string());
            let vaulted           = item.get("vaulted").and_then(|v| v.as_bool());
            let ducats            = item.get("ducats").and_then(|v| v.as_u64()).map(|n| n as u32);
            let mastery_req       = item.get("masteryReq").and_then(|v| v.as_u64()).map(|n| n as u32);
            let omega_attenuation = item.get("omegaAttenuation").and_then(|v| v.as_f64()).map(|n| n as f32);
            let fusion_limit      = item.get("fusionLimit").and_then(|v| v.as_u64()).map(|n| n as u32);
            let max_level_cap     = item.get("maxLevelCap").and_then(|v| v.as_u64()).map(|n| n as u32)
                .or_else(|| if unique_name.contains("/EntratiMech/") { Some(40) } else { None });
            let tradable          = item.get("tradable").and_then(|v| v.as_bool());
            let masterable        = item.get("masterable").and_then(|v| v.as_bool());
            let bp_cost           = item.get("bpCost").and_then(|v| v.as_u64()).map(|n| n as u32);

            // `category` (display category from wfcd_category_to_display) groups similar WFCD
            // categories together (e.g. "Sentinels"+"SentinelWeapons"+"Pets" → "Companions").
            // `wfcd_item_cat` is the raw WFCD category on the item itself; used to preserve
            // fine-grained categories that would otherwise fall into "Misc".
            let wfcd_item_cat = item.get("category").and_then(|v| v.as_str()).unwrap_or("");

            // Correct category before inserting:
            // • Blueprint items always go to "Blueprints"
            // • Fine-grained categories (Sigils, Skins) → keep as-is
            // • Non-relic items that WFCD groups under Relics (segments, etc.) → "Misc"
            let corrected_cat = if name.contains("Blueprint") {
                "Blueprints".to_string()
            } else if matches!(wfcd_item_cat, "Sigils" | "Skins") {
                wfcd_item_cat.to_string()
            } else if category == "Relics" {
                let n = name.to_lowercase();
                if n.ends_with("intact") || n.ends_with("exceptional")
                    || n.ends_with("flawless") || n.ends_with("radiant")
                {
                    "Relics".to_string()
                } else {
                    "Misc".to_string() // segments/blueprints that WFCD mis-groups under Relics
                }
            } else {
                category.clone()
            };

            if seen.insert(unique_name.clone()) {
                items.push(WfcdItem {
                    name: name.clone(),
                    unique_name: unique_name.clone(),
                    category: corrected_cat.clone(),
                    item_type: item_type.clone(),
                    product_category: product_category.clone(),
                    image_name: image_name.clone(),
                    vaulted, ducats, mastery_req, omega_attenuation, fusion_limit, max_level_cap,
                    tradable, masterable, bp_cost,
                });
            }

            if let Some(comps) = item.get("components").and_then(|v| v.as_array()) {
                if !comps.is_empty() {
                    raw_craftable.push((unique_name.clone(), *item));
                }
            }

            // All.json lists drops on the item, so the relic tables come from
            // inverting them. WFCD uses the Bronze path as the canonical key for
            // all refinements. relic_rewards is built from Relics.json, which
            // spells out every refinement.
            if let Some(drops_arr) = item.get("drops").and_then(|v| v.as_array()) {
                record_drops(&unique_name, &name, drops_arr, &mut raw_drop_entries, &mut drop_locations);
            }

            // Add component parts to catalog
            if let Some(comps) = item.get("components").and_then(|v| v.as_array()) {
                for comp in comps {
                    let cunique = match comp.get("uniqueName").and_then(|v| v.as_str()) {
                        Some(u) => u.trim().to_string(),
                        None => continue,
                    };
                    // WFCD's components[] no longer carry a `name`. Look the
                    // component's uniqueName up in DE's name tables; the result is
                    // the full display name ("Ash Prime Chassis"), parent included.
                    let resolved = resolver.resolve(&cunique);
                    let cname = resolved.as_deref().unwrap_or("");
                    // Relic and mission drops live on the component, and a
                    // resource keeps its table whether or not it makes the
                    // catalogue below.
                    if let Some(drops_arr) = comp.get("drops").and_then(|v| v.as_array()) {
                        record_drops(&cunique, cname, drops_arr, &mut raw_drop_entries, &mut drop_locations);
                    }
                    let is_part = cunique.starts_with("/Lotus/Types/Recipes/")
                        || cunique.starts_with("/Lotus/Powersuits/")
                        || cunique.starts_with("/Lotus/Weapons/")
                        || cunique.starts_with("/Lotus/Companions/")
                        || cunique.starts_with("/Lotus/Sentinels/")
                        || cunique.starts_with("/Lotus/Types/Sentinels/") // SentinelParts crafted components
                        || cunique.starts_with("/Lotus/Archwing/")
                        || cunique.starts_with("/Lotus/Types/Game/") // Kubrow/Kavat pet parts
                        || cname.contains("Blueprint");
                    if !is_part { continue; }
                    if resolved.is_none() {
                        unresolved_components.push(cunique.clone());
                        continue;
                    }

                    // KEY: if this component is a TOP-LEVEL item in any WFCD file,
                    // skip it here — it will be added with its correct standalone category.
                    // This prevents "Bronco" (Secondary) from being labelled "Parts"
                    // when it appears as a component of "Akbolto" (Primary).
                    if top_level_uniques.contains(&cunique) { continue; }

                    let comp_cat = if cunique.starts_with("/Lotus/Types/Recipes/") {
                        "Blueprints"
                    } else {
                        category // keep parent's category (Warframes, Primary, etc.)
                    };
                    let comp_image = comp.get("imageName")
                        .and_then(|v| v.as_str()).map(|s| s.to_string())
                        .or_else(|| image_name.clone());

                    if seen.insert(cunique.clone()) {
                        let raw_comp_name = if cunique.starts_with("/Lotus/Powersuits/")
                            || cunique.starts_with("/Lotus/Companions/")
                            || cunique.starts_with("/Lotus/Sentinels/")
                            || cunique.starts_with("/Lotus/Types/Sentinels/")
                            || cunique.starts_with("/Lotus/Types/Game/")
                            || cunique.starts_with("/Lotus/Archwing/")
                        {
                            // These paths are BUILT parts (not blueprints).
                            // WFCD sometimes names them "Chassis Blueprint" already —
                            // strip the " Blueprint" suffix so the built part gets the
                            // correct name and ExportRecipes can provide a distinct blueprint entry.
                            // Also guard against WFCD including the parent name in the component name.
                            cname.strip_suffix(" Blueprint").unwrap_or(cname).to_string()
                        } else {
                            cname.to_string()
                        };
                        if raw_comp_name.trim() == "Blueprint" || raw_comp_name.trim().is_empty() {
                            seen.remove(&cunique); continue;
                        }
                        // Warframe/Archwing component blueprints drop from relics and are
                        // displayed in-game with "Blueprint" in the name (e.g. "Lavos Prime
                        // Neuroptics Blueprint"). WFCD stores them as just "Neuroptics".
                        // Sentinel/MOA companion parts under /Weapons/WeaponParts/ are physical
                        // items (like weapon parts), NOT blueprints — Relics.json confirms this
                        // by omitting "Blueprint" from names like "Nautilus Prime Carapace".
                        // Only append "Blueprint" for /WarframeRecipes/ paths.
                        let comp_name = if comp_cat == "Blueprints"
                            && (category == "Warframes" || category == "Archwing"
                                || (category == "Companions" && cunique.contains("/WarframeRecipes/")))
                            && !raw_comp_name.ends_with("Blueprint")
                        {
                            format!("{} Blueprint", raw_comp_name)
                        } else {
                            raw_comp_name
                        };
                        let comp_ducats = comp.get("ducats").and_then(|v| v.as_u64()).map(|n| n as u32);
                        items.push(WfcdItem {
                            name: comp_name.clone(),
                            unique_name: cunique.clone(),
                            category: comp_cat.to_string(),
                            item_type: String::new(),
                            product_category: String::new(),
                            image_name: comp_image.clone(),
                            vaulted: None,
                            ducats: comp_ducats,
                            mastery_req: None,
                            omega_attenuation: None,
                            fusion_limit: None,
                            max_level_cap: None,
                            tradable: None,
                            masterable: None,
                            bp_cost: None,
                        });

                        // Note: blueprint entries for these components are provided by
                        // ExportRecipes (Phase 1 in get_all_items) which is DE's authoritative
                        // source. Adding them from WFCD sub-components caused false "X Blueprint"
                        // entries for weapon parts that drop directly and have no real blueprint.
                    }
                }
            }
        }
    }

    if !unresolved_components.is_empty() {
        // Left out rather than guessed at: no name table knows these paths yet.
        warn!(
            count = unresolved_components.len(),
            sample = ?unresolved_components.iter().take(10).collect::<Vec<_>>(),
            "components with no name in ExportResources, ExportRecipes or Relics.json were not catalogued"
        );
    }

    if items.is_empty() {
        return Err("upstream categories held no usable items".to_string());
    }

    let display_names: HashMap<String, String> = items
        .iter()
        .map(|i| (i.unique_name.clone(), i.name.clone()))
        .collect();

    // Use DE's authoritative recipe data (best-effort; fall back to WFCD-only if absent)
    let export_recipes = parse_export_recipes(recipes_json);

    // Reverse map: blueprint unique_name → result item unique_name
    let bp_to_result: HashMap<String, String> = export_recipes.iter()
        .map(|(result, recipe)| (recipe.blueprint_unique.clone(), result.clone()))
        .collect();

    // Build syndicate store catalog from pre-fetched JSON
    let mut syndicate_catalog = parse_syndicate_store_catalog(syndicates_json, &items);

    // Fill result_unique on blueprint offers so the frontend can show crafted-item status
    for offers in syndicate_catalog.values_mut() {
        for offer in offers.iter_mut() {
            if offer.category == "Blueprints" {
                offer.result_unique = bp_to_result.get(&offer.unique_name).cloned();
            }
        }
    }

    // ── Second pass: add Warframe component blueprints ────────────────────────
    // In Warframe, relics drop Chassis/Neuroptics/Systems BLUEPRINTs — separate items
    // from the built components.
    //
    // Strategy A: use ExportRecipes (resultType → blueprintUnique) if available.
    // Strategy B (fallback): generate synthetic entries for every Powersuits component
    //   already in the catalog whose name doesn't end with "Blueprint".
    //   The synthetic unique_name follows DE's pattern (<component_path>Blueprint).
    {
        let image_by_unique: std::collections::HashMap<String, Option<String>> = items.iter()
            .map(|i| (i.unique_name.clone(), i.image_name.clone()))
            .collect();

        let mut bp_items: Vec<WfcdItem> = Vec::new();
        // Tracks result_types (built-part paths) that Strategy A already handled,
        // so Strategy B doesn't create a duplicate synthetic blueprint for the same item.
        let mut handled_by_a: HashSet<String> = HashSet::new();

        // Strategy A: ExportRecipes — covers warframe components, sentinel parts, archwing
        // components, and any other intermediate craftable items.
        for (result_type, recipe) in &export_recipes {
            // Only process result paths that are tracked owned-item prefixes.
            let is_tracked = result_type.starts_with("/Lotus/Powersuits/")
                || result_type.starts_with("/Lotus/Types/Sentinels/SentinelParts/")
                || result_type.starts_with("/Lotus/Archwing/");
            if !is_tracked { continue; }
            let bp_unique = &recipe.blueprint_unique;

            // Always mark the result_type as handled so Strategy B never creates a
            // synthetic "X Blueprint" when the real blueprint already exists.
            handled_by_a.insert(result_type.clone());

            // Add the blueprint entry if it isn't in the catalog yet.
            if !seen.contains(bp_unique) {
                // Get the built-item name from display_names (older warframes that have the
                // Powersuits path item already), or derive it from the recipe path item name.
                let result_name = display_names.get(result_type).cloned();
                if let Some(rname) = result_name {
                    if seen.insert(bp_unique.clone()) {
                        bp_items.push(WfcdItem {
                            name:             format!("{} Blueprint", rname),
                            unique_name:      bp_unique.clone(),
                            category:         "Blueprints".to_string(),
                            item_type:        String::new(),
                            product_category: String::new(),
                            image_name:       image_by_unique.get(result_type).and_then(|i| i.clone()),
                            vaulted:          None,
                            ducats:           None,
                            mastery_req:      None,
                            omega_attenuation: None,
                            fusion_limit:     None,
                            max_level_cap:    None,
                            tradable:         None,
                            masterable:       None,
                            bp_cost:          None,
                        });
                    }
                }
            }

            // Add the built result item (e.g. "Xaku Prime Chassis") if missing.
            // WFCD stores newer warframe components only at their Recipe path, leaving
            // the Powersuits result path absent from the catalog — so built parts that
            // the memory scanner finds would never get a display name.
            if !seen.contains(result_type) {
                // Derive the built-item name from the blueprint catalog entry: strip " Blueprint".
                let built_name = display_names.get(bp_unique.as_str())
                    .and_then(|n| n.strip_suffix(" Blueprint"))
                    .map(|s| s.to_string())
                    .or_else(|| display_names.get(result_type).cloned());
                if let Some(bname) = built_name {
                    if seen.insert(result_type.clone()) {
                        bp_items.push(WfcdItem {
                            name:             bname,
                            unique_name:      result_type.clone(),
                            category:         "Parts".to_string(),
                            item_type:        String::new(),
                            product_category: String::new(),
                            image_name:       image_by_unique.get(result_type).and_then(|i| i.clone()),
                            vaulted:          None,
                            ducats:           None,
                            mastery_req:      None,
                            omega_attenuation: None,
                            fusion_limit:     None,
                            max_level_cap:    None,
                            tradable:         None,
                            masterable:       None,
                            bp_cost:          None,
                        });
                    }
                }
            }
        }

        // Strategy B: synthetic fallback for every warframe component not covered above.
        // Covers the case where export_recipes fetch failed or is incomplete.
        for item in items.iter() {
            if !item.unique_name.starts_with("/Lotus/Powersuits/") { continue; }
            if item.name.ends_with("Blueprint") { continue; }
            if item.category == "Blueprints" { continue; }
            // Skip if Strategy A already created a blueprint for this built-part path
            if handled_by_a.contains(&item.unique_name) { continue; }
            // Derive a synthetic blueprint unique_name by appending "Blueprint"
            let bp_unique = format!("{}Blueprint", item.unique_name);
            if seen.contains(&bp_unique) { continue; }
            if seen.insert(bp_unique.clone()) {
                bp_items.push(WfcdItem {
                    name:             format!("{} Blueprint", item.name),
                    unique_name:      bp_unique,
                    category:         "Blueprints".to_string(),
                    item_type:        String::new(),
                    product_category: String::new(),
                    image_name:       item.image_name.clone(),
                    vaulted:          None,
                    ducats:           None,
                    mastery_req:      None,
                    omega_attenuation: None,
                    fusion_limit:     None,
                    max_level_cap:    None,
                    tradable:         None,
                    masterable:       None,
                    bp_cost:          None,
                });
            }
        }

        items.extend(bp_items);
    }

    // ── Pass 3: Fix warframe component naming ─────────────────────────────────
    // ExportRecipes stores: blueprint_path → { resultType: component_path }.
    // Example: XakuPrimeChassisBlueprint → { resultType: XakuPrimeChassisComponent }
    // WFCD lists XakuPrimeChassisComponent as a warframe component, so our code
    // (correctly) classifies it as "Blueprints" — but it is actually the BUILT PART.
    // The actual relic-drop blueprint (XakuPrimeChassisBlueprint) is absent.
    // Fix: rename component-path items to strip " Blueprint", add the real blueprint.
    {
        // result_type → blueprint_unique, restricted to WarframeRecipes components
        let warframe_component_map: HashMap<String, String> = export_recipes
            .iter()
            .filter(|(result_type, recipe)| {
                result_type.starts_with("/Lotus/Types/Recipes/WarframeRecipes/") &&
                !result_type.ends_with("Blueprint") &&
                recipe.blueprint_unique.starts_with("/Lotus/Types/Recipes/WarframeRecipes/")
            })
            .map(|(rt, recipe)| (rt.clone(), recipe.blueprint_unique.clone()))
            .collect();

        let mut bp_additions: Vec<WfcdItem> = Vec::new();

        for item in items.iter_mut() {
            if let Some(bp_unique) = warframe_component_map.get(&item.unique_name) {
                if item.category == "Blueprints" && item.name.ends_with(" Blueprint") {
                    let built_name = item.name[..item.name.len() - " Blueprint".len()].to_string();

                    // Add the actual blueprint if it isn't already in the catalog
                    if !seen.contains(bp_unique) {
                        seen.insert(bp_unique.clone());
                        bp_additions.push(WfcdItem {
                            name:             item.name.clone(),
                            unique_name:      bp_unique.clone(),
                            category:         "Blueprints".to_string(),
                            item_type:        String::new(),
                            product_category: String::new(),
                            image_name:       item.image_name.clone(),
                            vaulted:          item.vaulted,
                            ducats:           item.ducats,
                            mastery_req:      item.mastery_req,
                            omega_attenuation: None,
                            fusion_limit:     None,
                            max_level_cap:    None,
                            tradable:         None,
                            masterable:       None,
                            bp_cost:          None,
                        });
                    }

                    // Rename to reflect this is the built component, not the blueprint
                    item.name     = built_name;
                    item.category = "Parts".to_string();
                }
            }
        }

        items.extend(bp_additions);
    }

    // Safety net: remove any remaining "Foo Blueprint Blueprint" names.
    for item in items.iter_mut() {
        if item.name.contains(" Blueprint Blueprint") {
            item.name = item.name.replace(" Blueprint Blueprint", " Blueprint");
        }
    }

    // Pass 3 renamed the built parts and added their blueprints, so the
    // recipe trees read names from the items as they stand now.
    let display_names: HashMap<String, String> = items.iter()
        .map(|i| (i.unique_name.clone(), i.name.clone()))
        .collect();

    // Build recipe trees
    let prices = blueprint_prices(recipes_json);
    let mut recipes: HashMap<String, Vec<RecipeComponent>> = HashMap::new();
    for (parent_unique, item_json) in &raw_craftable {
        if let Some(comps) = item_json.get("components").and_then(|v| v.as_array()) {
            let mut tree: Vec<RecipeComponent> = comps.iter().filter_map(|c| {
                let cu = c["uniqueName"].as_str()?.trim().to_string();
                // Resolve by uniqueName (see build_recipe_node).
                let cn = tree_node_name(&cu, &display_names, &resolver);
                let cc = c["itemCount"].as_u64().unwrap_or(1) as u32;
                Some(build_recipe_node(
                    cu, cn, cc, Some(c),
                    RecipeCtx { display_names: &display_names, resolver: &resolver, export_recipes: &export_recipes },
                    0,
                ))
            }).collect();
            price_blueprints(&mut tree, &prices);
            if !tree.is_empty() {
                recipes.insert(parent_unique.clone(), tree);
            }
        }
    }

    // WFCD records a part's drop table under the built part's path. A plan runs
    // short of the part's blueprint, which is what drops, so the same table has
    // to read under the blueprint path too.
    fn alias_part_drops(components: &[RecipeComponent], drop_locations: &mut HashMap<String, Vec<DropLocation>>) {
        for part in components {
            let Some(blueprint) = part.components.iter().find(|c| crate::mastery_recipe::is_blueprint(c)) else { continue };
            if let Some(locations) = drop_locations.get(&part.unique_name).cloned() {
                drop_locations.entry(blueprint.unique_name.clone()).or_insert(locations);
            }
            alias_part_drops(&part.components, drop_locations);
        }
    }
    for components in recipes.values() { alias_part_drops(components, &mut drop_locations); }

    // WFCD's components[] no longer carry an `imageName`, so every part and blueprint
    // above inherited its parent's image (all of Yareli Prime's parts wore
    // Yareli Prime's portrait). Give each the icon DE's data lists for its own
    // uniqueName; entries DE has no icon for keep what they have. This runs before
    // the name-based lookup below so relic rewards pick the right icons up too.
    for item in items.iter_mut() {
        if item.category == "Parts" || item.category == "Blueprints" {
            if let Some(icon) = resolver.icon(&item.unique_name) {
                item.image_name = Some(icon.to_string());
            }
        }
    }

    // Name-based image lookup passed to fetch_relics_rewards for icon enrichment.
    let image_by_name: HashMap<String, String> = items.iter()
        .filter_map(|i| i.image_name.as_ref().map(|img| (i.name.to_lowercase(), img.clone())))
        .collect();

    // Build relic_drops (item → relics) from raw_drop_entries — used by RelicHelper.
    let mut relic_drops: HashMap<String, Vec<String>> = HashMap::new();
    for (relic_path, entries) in &raw_drop_entries {
        let mut seen: HashSet<String> = HashSet::new();
        for (item_unique, _, _) in entries {
            if seen.insert(item_unique.clone()) {
                relic_drops.entry(item_unique.clone()).or_default().push(relic_path.clone());
            }
        }
    }

    // Build relic_rewards from both pre-fetched sources and merge them, rather
    // than treating one as a fallback for total failure of the other — DE's own
    // export (via Relics.json) lags behind on newly-released relics/Primes (e.g.
    // Corufell Prime shipped with zero entries there), while warframe-drop-data
    // picks those up faster but Relics.json carries richer per-item uniqueNames
    // for the EE.log-path lookup. Relics.json wins per-relic when both have an
    // entry; drop-data fills in any relic missing from Relics.json entirely.
    let mut relic_rewards = parse_relics_rewards(relics_json, &image_by_name);
    let name_to_unique: HashMap<String, String> = items.iter()
        .map(|i| (i.name.to_lowercase(), i.unique_name.clone()))
        .collect();
    let relic_name_to_path = build_relic_name_to_path(relics_json);
    let drop_data_rewards = parse_relics_rewards_from_drop_data(drop_data_json, &image_by_name, &name_to_unique, &relic_name_to_path);
    if relic_rewards.is_empty() {
        warn!("Relics.json rewards empty upstream — using warframe-drop-data for relic rewards");
        relic_rewards = drop_data_rewards;
    } else {
        let mut filled = 0u32;
        for (name, rewards) in drop_data_rewards {
            relic_rewards.entry(name).or_insert_with(|| { filled += 1; rewards });
        }
        if filled > 0 {
            info!(filled, "filled in relics missing from Relics.json using warframe-drop-data");
        }
    }

    // Backfill ducat values that WFCD's components[] no longer carries (the same
    // upstream schema change that dropped `name`/`imageName` from every component
    // sub-object also dropped `ducats`, but with no parent-item fallback available —
    // see `ducat_value_from_rarities`). This must run before blueprint_names below,
    // which reads item.ducats.
    {
        let ducat_exceptions = load_ducat_exceptions(ducat_exceptions_json);
        let mut rarities_by_unique: HashMap<String, HashSet<String>> = HashMap::new();
        let mut rarities_by_name: HashMap<String, HashSet<String>> = HashMap::new();
        for rewards in relic_rewards.values() {
            for r in rewards {
                if !r.unique_name.is_empty() {
                    rarities_by_unique.entry(r.unique_name.clone()).or_default().insert(r.rarity.clone());
                }
                rarities_by_name.entry(r.name.to_lowercase()).or_default().insert(r.rarity.clone());
            }
        }
        let mut backfilled = 0u32;
        for item in items.iter_mut() {
            if item.ducats.is_some() { continue; }
            if let Some(&v) = ducat_exceptions.get(&item.name.to_lowercase()) {
                item.ducats = Some(v);
                backfilled += 1;
                continue;
            }
            let rarities = rarities_by_unique.get(&item.unique_name)
                .or_else(|| rarities_by_name.get(&item.name.to_lowercase()));
            if let Some(rarities) = rarities {
                let set: HashSet<&str> = rarities.iter().map(|s| s.as_str()).collect();
                if let Some(v) = ducat_value_from_rarities(&set) {
                    item.ducats = Some(v);
                    backfilled += 1;
                }
            }
        }
        info!(backfilled, "backfilled ducat values from relic drop rarity (WFCD components[] no longer carries them)");
    }

    // Build blueprint_names: blueprint_path → (display_name, ducats)
    // Lets the frontend create virtual catalog entries for component blueprints that
    // are tracked by the API but may be absent from the WFCD catalog.
    // IMPORTANT: use the post-Pass3 items list — display_names was built before Pass 3
    // renamed warframe components, so deriving names from it would produce
    // "Xaku Prime Chassis Blueprint Blueprint" style duplicates.
    let items_by_unique: HashMap<&str, &WfcdItem> = items.iter()
        .map(|i| (i.unique_name.as_str(), i))
        .collect();

    let blueprint_names: HashMap<String, (String, Option<u32>)> = export_recipes.iter()
        .filter_map(|(result_type, recipe)| {
            let bp_unique = &recipe.blueprint_unique;
            // Primary: the blueprint item itself is in the post-Pass3 catalog
            if let Some(item) = items_by_unique.get(bp_unique.as_str()) {
                return Some((bp_unique.clone(), (item.name.clone(), item.ducats)));
            }
            // Fallback: derive name from result item, guarding against double "Blueprint"
            if let Some(result_item) = items_by_unique.get(result_type.as_str()) {
                let name = if result_item.name.ends_with(" Blueprint") {
                    result_item.name.clone()
                } else {
                    format!("{} Blueprint", result_item.name)
                };
                return Some((bp_unique.clone(), (name, result_item.ducats)));
            }
            // Last resort: display_names (older content not in items), guard "Blueprint Blueprint"
            let display = display_names.get(result_type)?;
            let name = if display.ends_with(" Blueprint") {
                display.clone()
            } else {
                format!("{} Blueprint", display)
            };
            Some((bp_unique.clone(), (name, None)))
        })
        .collect();

    // Fetch the canonical reward name list from the Warframe Wiki.
    // This is non-blocking on failure — if the wiki is unreachable, we fall back
    // to the existing prime/forma filters in the overlay catalog builder.
    let wiki_reward_names = fetch_wiki_reward_names();

    // Propagate imageName from items that have one to same-named items that don't.
    // StoreItems proxy entries (e.g. /Lotus/StoreItems/.../Kuva) often lack imageName while
    // the canonical inventory path (/Lotus/Types/.../Kuva) has it. If the scanner ever
    // returns the StoreItems path, the lookup would find no image without this fix.
    //
    // A blueprint is named "<crafted item> Blueprint" but is never *named* the same as
    // what it crafts, so the exact-name match above never fires for it — even when the
    // crafted item sits right there in the same catalog with a real WFCD imageName
    // (e.g. "Gorgon" the weapon vs. "Gorgon Blueprint" the separate catalog entry).
    // DE gives blueprints no icon of their own; they wear the icon of what they craft,
    // same as in-game, so strip the suffix and match again before giving up.
    {
        let name_to_image: HashMap<String, String> = items.iter()
            .filter_map(|i| i.image_name.as_ref().map(|img| (i.name.clone(), img.clone())))
            .collect();
        for item in items.iter_mut() {
            if item.image_name.is_none() {
                if let Some(img) = name_to_image.get(&item.name) {
                    item.image_name = Some(img.clone());
                } else if let Some(base) = item.name.strip_suffix(" Blueprint") {
                    if let Some(img) = name_to_image.get(base) {
                        item.image_name = Some(img.clone());
                    }
                }
            }
        }
    }

    // Build weapon disposition map from omegaAttenuation values already in the item list.
    // No separate ExportWeapons.json fetch required — All.json has this field.
    let weapon_dispositions: HashMap<String, f32> = items.iter()
        .filter_map(|i| i.omega_attenuation.map(|d| (i.unique_name.clone(), d)))
        .collect();

    Ok(FetchResult { items, recipes, relic_drops, drop_locations, relic_rewards, blueprint_names, wiki_reward_names, syndicate_catalog, weapon_dispositions })
}

pub fn fallback_items() -> Vec<WfcdItem> {
    vec![
        ("/Lotus/Types/Items/MiscItems/OrokinCell",    "Orokin Cell",    "Resources"),
        ("/Lotus/Types/Items/MiscItems/Neurodes",      "Neurodes",       "Resources"),
        ("/Lotus/Types/Items/MiscItems/NeuralSensors", "Neural Sensors", "Resources"),
        ("/Lotus/Types/Items/MiscItems/Morphics",      "Morphics",       "Resources"),
        ("/Lotus/Types/Items/MiscItems/Tellurium",     "Tellurium",      "Resources"),
        ("/Lotus/Types/Items/MiscItems/ArgonCrystal",  "Argon Crystal",  "Resources"),
        ("/Lotus/Types/Items/MiscItems/ControlModule", "Control Module", "Resources"),
        ("/Lotus/Types/Items/MiscItems/Gallium",       "Gallium",        "Resources"),
        ("/Lotus/Types/Items/MiscItems/Oxium",         "Oxium",          "Resources"),
        ("/Lotus/Types/Items/MiscItems/Rubedo",        "Rubedo",         "Resources"),
        ("/Lotus/Types/Items/MiscItems/Ferrite",       "Ferrite",        "Resources"),
        ("/Lotus/Types/Items/MiscItems/AlloyPlate",    "Alloy Plate",    "Resources"),
        ("/Lotus/Types/Items/MiscItems/Circuits",      "Circuits",       "Resources"),
        ("/Lotus/Types/Items/MiscItems/Salvage",       "Salvage",        "Resources"),
        ("/Lotus/Types/Items/MiscItems/NanoSpores",    "Nano Spores",    "Resources"),
    ]
    .into_iter()
    .map(|(u, n, c)| WfcdItem {
        unique_name: u.to_string(),
        name: n.to_string(),
        category: c.to_string(),
        item_type: String::new(), product_category: String::new(),
        image_name: None, vaulted: None, ducats: None, mastery_req: None, omega_attenuation: None, fusion_limit: None, max_level_cap: None, tradable: None, masterable: None, bp_cost: None,
    })
    .collect()
}

// ─── Drop data ───────────────────────────────────────────────────────────────

const DROP_DATA_URL: &str =
    "https://raw.githubusercontent.com/WFCD/warframe-drop-data/gh-pages/data/all.json";
const DROP_DATA_CACHE: &str = "drop-data-v1.json";
const DROP_DATA_TTL: std::time::Duration = std::time::Duration::from_secs(24 * 60 * 60);

fn fetch_drop_data(etag: Option<&str>) -> Result<crate::cache::Fetched<serde_json::Value>, String> {
    match crate::cache::get_conditional(DROP_DATA_URL, etag)? {
        crate::cache::Fetched::NotModified => Ok(crate::cache::Fetched::NotModified),
        crate::cache::Fetched::New(body, new_etag) => {
            let value: serde_json::Value = serde_json::from_str(&body)
                .map_err(|e| format!("drop-data parse: {e}"))?;
            Ok(crate::cache::Fetched::New(value, new_etag))
        }
    }
}

#[tauri::command]
pub async fn get_drop_data(force: Option<bool>) -> Result<serde_json::Value, String> {
    let force = force.unwrap_or(false);
    let (data, _source, warning) = crate::cache::get_or_refresh(
        DROP_DATA_CACHE,
        if force { std::time::Duration::ZERO } else { DROP_DATA_TTL },
        fetch_drop_data,
    );
    if let Some(w) = warning {
        tracing::warn!("{w}");
    }
    data.ok_or_else(|| "drop data unavailable".to_string())
}

pub fn refresh_drop_data(_app: &tauri::AppHandle, force: bool) -> Result<(), String> {
    let ttl = if force { std::time::Duration::ZERO } else { DROP_DATA_TTL };
    let (_, _source, warning) = crate::cache::get_or_refresh(DROP_DATA_CACHE, ttl, fetch_drop_data);
    if let Some(w) = warning {
        tracing::warn!("{w}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct MemStore(Mutex<HashMap<String, String>>);

    impl MemStore {
        fn with(name: &str, body: &str) -> Self {
            let store = Self::default();
            store.write(name, body);
            store
        }
    }

    impl BodyStore for MemStore {
        fn read(&self, name: &str) -> Option<String> {
            self.0.lock().expect("no test panics while holding this").get(name).cloned()
        }

        fn write(&self, name: &str, body: &str) {
            self.0
                .lock()
                .expect("no test panics while holding this")
                .insert(name.to_string(), body.to_string());
        }
    }

    fn spec() -> SourceSpec {
        SourceSpec { name: "Mods".into(), urls: vec!["https://example/Mods.json".into()], required: true }
    }

    fn never(_: &str, _: Option<&str>) -> Result<Fetched<String>, String> {
        panic!("no request expected")
    }

    #[test]
    fn fresh_body_is_used_and_stored() {
        let store = MemStore::default();
        let probe = Probe::Body("[1]".to_string(), Some("new".into()));
        let out = resolve_source(&spec(), Some("old"), probe, &never, &store);

        assert_eq!(out.json, Some(serde_json::json!([1])));
        assert_eq!(out.etag.as_deref(), Some("new"));
        assert_eq!(store.read("Mods").as_deref(), Some("[1]"));
    }

    #[test]
    fn confirmed_body_comes_back_from_the_store() {
        let store = MemStore::with("Mods", "[2]");
        let out = resolve_source(&spec(), Some("old"), Probe::Unchanged, &never, &store);

        assert_eq!(out.json, Some(serde_json::json!([2])));
        assert_eq!(out.etag.as_deref(), Some("old"));
    }

    #[test]
    fn confirmation_without_a_stored_body_refetches_unconditionally() {
        let store = MemStore::default();
        let sent: Mutex<Vec<Option<String>>> = Mutex::new(Vec::new());
        let fetch = |_: &str, etag: Option<&str>| {
            sent.lock().expect("no panic in this closure").push(etag.map(str::to_string));
            Ok(Fetched::New("[3]".to_string(), Some("fetched".into())))
        };

        let out = resolve_source(&spec(), Some("old"), Probe::Unchanged, &fetch, &store);

        assert_eq!(out.json, Some(serde_json::json!([3])));
        assert_eq!(out.etag.as_deref(), Some("fetched"));
        assert_eq!(sent.into_inner().expect("no panic in this closure"), vec![None]);
    }

    #[test]
    fn a_failed_source_falls_back_to_the_stored_body() {
        let store = MemStore::with("Mods", "[4]");
        let out =
            resolve_source(&spec(), Some("old"), Probe::Failed("timed out".into()), &never, &store);

        assert_eq!(out.json, Some(serde_json::json!([4])));
        assert_eq!(out.etag.as_deref(), Some("old"));
    }

    #[test]
    fn a_failed_source_with_nothing_stored_yields_no_body() {
        let store = MemStore::default();
        let out = resolve_source(&spec(), None, Probe::Failed("timed out".into()), &never, &store);

        assert!(out.json.is_none());
        assert!(out.etag.is_none());
    }

    // ── NameResolver: components are named by uniqueName lookup, never by path ─

    fn resolver_fixture() -> NameResolver {
        let resources = serde_json::json!({
            "/Lotus/Types/Recipes/WarframeRecipes/YareliPrimeHelmetComponent": {
                "name": "/Lotus/Language/Primes/CraftingComponent_YareliPrimeHelmetName",
                "icon": "/Lotus/Interface/Icons/StoreIcons/Resources/CraftingComponents/GenericWarframePrimeHelmet.png"
            },
            "/Lotus/Types/Recipes/Weapons/WeaponParts/AkstilettoPrimeBarrel": {
                "name": "/Lotus/Language/Menu/CraftingComponent_AkstilettoPrimeBarrel"
            },
            "/Lotus/Types/Items/NoText": { "name": "/Lotus/Language/Missing" }
        });
        let dict = serde_json::json!({
            "/Lotus/Language/Primes/CraftingComponent_YareliPrimeHelmetName": "Yareli Prime Neuroptics",
            "/Lotus/Language/Menu/CraftingComponent_AkstilettoPrimeBarrel": "Akstiletto Prime Barrel"
        });
        let recipes = serde_json::json!({
            "/Lotus/Types/Recipes/WarframeRecipes/YareliPrimeHelmetBlueprint": {
                "resultType": "/Lotus/Types/Recipes/WarframeRecipes/YareliPrimeHelmetComponent"
            },
            "/Lotus/Types/Recipes/Weapons/CorvasBlueprint": {
                "resultType": "/Lotus/Weapons/Corvas/Corvas"
            }
        });
        let relics = serde_json::json!([{ "rewards": [{ "item": {
            "uniqueName": "/Lotus/Types/Recipes/Weapons/WeaponParts/CorufellPrimeHandle",
            "name": "Corufell Prime Handle"
        }}]}]);
        let mut r = NameResolver {
            by_unique: parse_name_db(Some(&resources), Some(&dict)),
            blueprint_result: parse_blueprint_results(Some(&recipes)),
            relic_names: parse_relic_item_names(Some(&relics)),
            top_level: HashMap::new(),
            icons: parse_icon_db(Some(&resources)),
        };
        r.top_level.insert("/Lotus/Weapons/Corvas/Corvas".into(), "Corvas".into());
        r
    }

    #[test]
    fn parts_and_their_blueprints_wear_the_part_icon_not_the_parents() {
        let r = resolver_fixture();
        assert_eq!(
            r.icon("/Lotus/Types/Recipes/WarframeRecipes/YareliPrimeHelmetComponent"),
            Some("GenericWarframePrimeHelmet.png")
        );
        // The blueprint has no icon entry; it shows what it crafts.
        assert_eq!(
            r.icon("/Lotus/Types/Recipes/WarframeRecipes/YareliPrimeHelmetBlueprint"),
            Some("GenericWarframePrimeHelmet.png")
        );
        // Nothing known → no icon, so the caller keeps whatever image it already had.
        assert_eq!(r.icon("/Lotus/Types/Recipes/Weapons/CorvasBlueprint"), None);
        assert!(parse_icon_db(None).is_empty());
    }

    #[test]
    fn neuroptics_is_named_from_the_database_not_the_path() {
        // DE's path token for Neuroptics is "Helmet"; the name table says otherwise.
        let r = resolver_fixture();
        assert_eq!(
            r.resolve("/Lotus/Types/Recipes/WarframeRecipes/YareliPrimeHelmetComponent").as_deref(),
            Some("Yareli Prime Neuroptics")
        );
        assert_eq!(
            r.resolve("/Lotus/Types/Recipes/WarframeRecipes/YareliPrimeHelmetBlueprint").as_deref(),
            Some("Yareli Prime Neuroptics Blueprint")
        );
    }

    #[test]
    fn weapon_parts_and_blueprints_resolve() {
        let r = resolver_fixture();
        assert_eq!(
            r.resolve("/Lotus/Types/Recipes/Weapons/WeaponParts/AkstilettoPrimeBarrel").as_deref(),
            Some("Akstiletto Prime Barrel")
        );
        // A blueprint for a top-level item is named for the item it crafts.
        assert_eq!(
            r.resolve("/Lotus/Types/Recipes/Weapons/CorvasBlueprint").as_deref(),
            Some("Corvas Blueprint")
        );
    }

    #[test]
    fn relic_reward_names_cover_paths_the_export_lacks() {
        let r = resolver_fixture();
        assert_eq!(
            r.resolve("/Lotus/Types/Recipes/Weapons/WeaponParts/CorufellPrimeHandle").as_deref(),
            Some("Corufell Prime Handle")
        );
    }

    #[test]
    fn unknown_paths_are_never_guessed_from_their_tail() {
        let r = resolver_fixture();
        assert_eq!(r.resolve("/Lotus/Types/Recipes/WarframeRecipes/SomeNewHelmetComponent"), None);
        // A language key the dictionary cannot resolve yields no name either.
        assert_eq!(r.resolve("/Lotus/Types/Items/NoText"), None);
    }

    #[test]
    fn missing_name_tables_resolve_nothing() {
        assert!(parse_name_db(None, None).is_empty());
    }

    // ── ducat_value_from_rarities / load_ducat_exceptions ──────────────────
    // WFCD's components[] entries stopped carrying a `ducats` field upstream, with
    // no parent-item fallback available (unlike name/image). Ducat value is instead
    // derived from drop rarity, matching the Warframe Wiki's own Module:Void/data
    // formula, plus a small bundled exceptions table for parts that don't follow it.

    #[test]
    fn ducat_formula_matches_single_rarity_tiers() {
        assert_eq!(ducat_value_from_rarities(&["Bronze"].into_iter().collect()), Some(15));
        assert_eq!(ducat_value_from_rarities(&["Silver"].into_iter().collect()), Some(45));
        assert_eq!(ducat_value_from_rarities(&["Gold"].into_iter().collect()), Some(100));
    }

    #[test]
    fn ducat_formula_matches_mixed_rarity_tiers() {
        assert_eq!(ducat_value_from_rarities(&["Bronze", "Gold"].into_iter().collect()), Some(25));
        assert_eq!(ducat_value_from_rarities(&["Bronze", "Silver"].into_iter().collect()), Some(25));
        assert_eq!(ducat_value_from_rarities(&["Silver", "Gold"].into_iter().collect()), Some(65));
    }

    #[test]
    fn ducat_formula_none_for_empty_rarities() {
        assert_eq!(ducat_value_from_rarities(&HashSet::new()), None);
    }

    #[test]
    fn ducat_exceptions_fall_back_to_bundled_copy_when_nothing_was_fetched() {
        let exceptions = load_ducat_exceptions(None);
        assert!(!exceptions.is_empty(), "bundled ducat_exceptions.json should parse to a non-empty map");
        // Sanity-check a couple of entries against the wiki's DUCAT_EXCEPTIONS table.
        assert_eq!(exceptions.get("akstiletto prime receiver"), Some(&45));
        assert_eq!(exceptions.get("forma blueprint"), Some(&0));
        // The "_comment" key must never leak in as a fake exception entry.
        assert!(!exceptions.contains_key("_comment"));
    }

    #[test]
    fn ducat_exceptions_prefer_the_fetched_mirror_over_the_bundled_copy() {
        let fetched = serde_json::json!({ "exceptions": { "soma prime blueprint": 999 } });
        let exceptions = load_ducat_exceptions(Some(&fetched));
        assert_eq!(exceptions.get("soma prime blueprint"), Some(&999));
        // Only the fetched map is used — it does not merge with the bundled one.
        assert!(!exceptions.contains_key("akstiletto prime receiver"));
    }

    #[test]
    fn ducat_exceptions_fall_back_when_the_fetched_body_is_empty_or_malformed() {
        assert_eq!(
            load_ducat_exceptions(Some(&serde_json::json!({ "exceptions": {} }))).get("forma blueprint"),
            Some(&0)
        );
        assert_eq!(
            load_ducat_exceptions(Some(&serde_json::json!({ "not_exceptions": {} }))).get("forma blueprint"),
            Some(&0)
        );
    }

    // ── parse_relics_rewards: rarity derived from chance grouping, not the
    // mislabeled WFCD string (real bug: Lex Prime showed 45 ducats for all
    // three parts because WFCD's `rarity` field never emits "Common") ──────

    fn reward(name: &str, chance: f64) -> serde_json::Value {
        serde_json::json!({
            "item": { "name": name, "uniqueName": format!("/Lotus/{name}") },
            // WFCD mislabels every Common slot as "Uncommon" — the fixture
            // deliberately keeps that wrong string to prove the fix ignores it.
            "rarity": "Uncommon",
            "chance": chance
        })
    }

    #[test]
    fn relic_rarity_comes_from_chance_grouping_not_the_mislabeled_string() {
        // Mirrors a real relic shape: 3 Common slots sharing one chance value,
        // 2 Uncommon, 1 Rare — all three tiers wrongly say "Uncommon" upstream.
        let relics = serde_json::json!([{
            "name": "Lith A1 Intact",
            "uniqueName": "/Lotus/Relics/LithA1Intact",
            "rewards": [
                reward("Lex Prime Barrel", 25.33),
                reward("Lex Prime Barrel", 25.33),
                reward("Lex Prime Barrel", 25.33),
                reward("Lex Prime Receiver", 11.0),
                reward("Lex Prime Receiver", 11.0),
                reward("Lex Prime Blueprint", 2.0),
            ]
        }]);

        let result = parse_relics_rewards(Some(&relics), &HashMap::new());
        let rewards = result.get("Lith A1 Intact").expect("relic should be keyed by name");

        let rarity_of = |name: &str| {
            rewards.iter().find(|r| r.name == name).map(|r| r.rarity.clone())
        };
        assert_eq!(rarity_of("Lex Prime Barrel"), Some("Bronze".to_string()));
        assert_eq!(rarity_of("Lex Prime Receiver"), Some("Silver".to_string()));
        assert_eq!(rarity_of("Lex Prime Blueprint"), Some("Gold".to_string()));
    }

    #[test]
    fn relic_rarity_falls_back_to_string_label_for_non_standard_relics() {
        // Requiem Eterna Relic's real shape: 8 equal-chance slots, no 3/2/1
        // grouping exists, so the chance-based derivation can't apply.
        let relics = serde_json::json!([{
            "name": "Requiem Eterna Relic",
            "uniqueName": "/Lotus/Relics/RequiemEterna",
            "rewards": [
                reward("Requiem Mod A", 12.5),
                reward("Requiem Mod B", 12.5),
            ]
        }]);

        let result = parse_relics_rewards(Some(&relics), &HashMap::new());
        let rewards = result.get("Requiem Eterna Relic").expect("relic should be keyed by name");
        // Falls back to the (mislabeled) string, matching old behavior for this shape.
        assert!(rewards.iter().all(|r| r.rarity == "Silver"));
    }

    #[test]
    fn relic_vaulted_field_is_extracted_by_fetch_from_wfcd() {
        // Real WFCD Relics.json shape (trimmed) for "Axi A1 Intact".
        let item: serde_json::Value = serde_json::from_str(r#"{"uniqueName":"/Lotus/Types/Game/Projections/T4VoidProjectionEBronze","name":"Axi A1 Intact","type":"Relic","imageName":"RelicAxiD.png","category":"Relics","tradable":true,"rewards":[{"chance":11,"item":{"name":"Akstiletto Prime Barrel","uniqueName":"/Lotus/Types/Recipes/Weapons/WeaponParts/AkstilettoPrimeBarrel"},"rarity":"Uncommon"}],"vaulted":true,"masterable":false}"#).unwrap();
        let all_items: Vec<&serde_json::Value> = vec![&item];
        let result = fetch_from_wfcd(&all_items, None, None, None, None, None, None, None).expect("fetch_from_wfcd should succeed");
        let found = result.items.iter()
            .find(|i| i.unique_name == "/Lotus/Types/Game/Projections/T4VoidProjectionEBronze")
            .expect("relic item should be in result.items");
        assert_eq!(found.vaulted, Some(true));
    }

    #[test]
    fn component_drops_split_into_relics_and_locations() {
        const BARREL: &str = "/Lotus/Types/Recipes/Weapons/WeaponParts/BratonPrimeBarrel";
        const CELL: &str = "/Lotus/Types/Items/MiscItems/OrokinCell";
        const AXI: &str = "/Lotus/Types/Game/Projections/T4VoidProjectionGrendelPrimeDBronze";
        let braton = serde_json::json!({
            "name": "Braton Prime", "uniqueName": "/Lotus/Weapons/Tenno/Rifle/BratonPrime", "category": "Primary",
            "components": [
                { "name": "Barrel", "uniqueName": BARREL, "drops": [
                    { "location": "Axi A17 Relic (Radiant)", "uniqueName": AXI, "chance": 16.67, "rarity": "Uncommon" },
                    { "location": "Corrupted Vor", "uniqueName": null, "chance": 50.0 },
                    { "location": "Corrupted Vor", "uniqueName": null, "chance": 50.0 },
                    { "location": "Cephalon Simaris", "chance": null },
                ] },
                { "name": "Orokin Cell", "uniqueName": CELL, "drops": [{ "location": "Corrupted Vor", "chance": 50.0 }] },
            ],
        });
        let cell = serde_json::json!({ "name": "Orokin Cell", "uniqueName": CELL, "category": "Resources",
            "drops": [{ "location": "Saturn/Titan (Survival), Rotation C", "chance": 12.5 }] });
        let out = fetch_from_wfcd(&[&braton, &cell], None, None, None, None, None, None, None).expect("fixture builds");

        assert_eq!(out.relic_drops.get(BARREL).map(Vec::as_slice), Some(&[AXI.to_string()][..]));
        let at = |unique: &str| out.drop_locations.get(unique).map(|v| v.iter().map(|d| (d.location.as_str(), d.chance)).collect::<Vec<_>>());
        assert_eq!(at(BARREL), Some(vec![("Corrupted Vor", Some(50.0)), ("Cephalon Simaris", None)]));
        assert_eq!(at(CELL), Some(vec![("Corrupted Vor", Some(50.0)), ("Saturn/Titan (Survival), Rotation C", Some(12.5))]));
        assert!(!out.drop_locations.contains_key(AXI));
    }

    #[test]
    fn recipe_blueprint_nodes_carry_the_suffix_once_and_the_market_credit_price_is_kept() {
        const FROST_CHASSIS: &str = "/Lotus/Types/Recipes/WarframeRecipes/FrostChassisComponent";
        const FROST_CHASSIS_BP: &str = "/Lotus/Types/Recipes/WarframeRecipes/FrostChassisBlueprint";
        const ELYTRON_HARNESS: &str = "/Lotus/Types/Recipes/ArchwingRecipes/DemolitionArchwing/DemolitionArchwingChassisComponent";
        const ELYTRON_HARNESS_BP: &str = "/Lotus/Types/Recipes/ArchwingRecipes/DemolitionArchwing/DemolitionArchwingChassisBlueprint";
        const FERRITE: &str = "/Lotus/Types/Items/MiscItems/Ferrite";
        let frost = serde_json::json!({
            "name": "Frost", "uniqueName": "/Lotus/Powersuits/Frost/Frost", "category": "Warframes",
            "components": [{ "name": "Chassis", "uniqueName": FROST_CHASSIS, "itemCount": 1, "drops": [{ "location": "Lieutenant Lech Kril", "chance": 38.72 }] }],
        });
        let elytron = serde_json::json!({
            "name": "Elytron", "uniqueName": "/Lotus/Powersuits/Archwing/DemolitionJetPack/DemolitionJetPack", "category": "Archwing",
            "components": [{ "name": "Harness", "uniqueName": ELYTRON_HARNESS, "itemCount": 1 }],
        });
        let astilla = serde_json::json!({
            "name": "Astilla", "uniqueName": "/Lotus/Weapons/Tenno/Shotgun/TnSlugShotgun/TnSlugShotgunWeapon", "category": "Primary", "bpCost": 20000,
        });
        let export = serde_json::json!({
            FROST_CHASSIS_BP: { "resultType": FROST_CHASSIS, "buildPrice": 15000, "ingredients": [{ "ItemType": FERRITE, "ItemCount": 1000 }] },
            ELYTRON_HARNESS_BP: { "resultType": ELYTRON_HARNESS, "buildPrice": 15000, "ingredients": [{ "ItemType": FERRITE, "ItemCount": 1000 }] },
        });
        let resources = serde_json::json!({
            FROST_CHASSIS: { "name": "/Lotus/Language/Menu/CraftingComponent_FrostChassisName" },
            ELYTRON_HARNESS: { "name": "/Lotus/Language/Menu/CraftingComponent_DemolitionArchwingChassisName" },
        });
        let dict = serde_json::json!({
            "/Lotus/Language/Menu/CraftingComponent_FrostChassisName": "Frost Chassis",
            "/Lotus/Language/Menu/CraftingComponent_DemolitionArchwingChassisName": "Elytron Harness",
        });
        let out = fetch_from_wfcd(&[&frost, &elytron, &astilla], Some(&export), None, None, Some(&resources), Some(&dict), None, None).expect("fixture builds");

        let names = |result: &str| -> Vec<(String, String)> {
            out.recipes[result].iter().flat_map(|part| std::iter::once((part.unique_name.clone(), part.name.clone()))
                .chain(part.components.iter().map(|c| (c.unique_name.clone(), c.name.clone())))).collect()
        };
        assert_eq!(names("/Lotus/Powersuits/Frost/Frost"), [
            (FROST_CHASSIS.to_string(), "Frost Chassis".to_string()),
            (FROST_CHASSIS_BP.to_string(), "Frost Chassis Blueprint".to_string()),
            (FERRITE.to_string(), "Ferrite".to_string()),
        ]);
        assert_eq!(names("/Lotus/Powersuits/Archwing/DemolitionJetPack/DemolitionJetPack")[1], (ELYTRON_HARNESS_BP.to_string(), "Elytron Harness Blueprint".to_string()));
        let price = |name: &str| out.items.iter().find(|i| i.name == name).expect("listed").bp_cost;
        assert_eq!((price("Astilla"), price("Frost")), (Some(20_000), None));
        let at = |unique: &str| out.drop_locations.get(unique).map(|v| v.iter().map(|d| d.location.as_str()).collect::<Vec<_>>());
        assert_eq!((at(FROST_CHASSIS), at(FROST_CHASSIS_BP)), (Some(vec!["Lieutenant Lech Kril"]), Some(vec!["Lieutenant Lech Kril"])));
    }

    #[test]
    fn drop_locations_default_on_old_caches_and_round_trip() {
        let old = serde_json::json!({
            "items": [], "recipes": {}, "relic_drops": {}, "relic_rewards": {},
            "blueprint_names": {}, "wiki_reward_names": [], "syndicate_catalog": {}, "weapon_dispositions": {},
        });
        let mut cached: FetchResult = serde_json::from_value(old).expect("a cache without the field still loads");
        assert!(cached.drop_locations.is_empty());

        let locations = vec![DropLocation { location: "Corrupted Vor".into(), chance: Some(50.0) }, DropLocation { location: "Cephalon Simaris".into(), chance: None }];
        cached.drop_locations.insert("/Lotus/Types/Items/MiscItems/OrokinCell".into(), locations.clone());
        let again: FetchResult = serde_json::from_str(&serde_json::to_string(&cached).expect("serializes")).expect("parses");
        assert_eq!(again.drop_locations.get("/Lotus/Types/Items/MiscItems/OrokinCell"), Some(&locations));
    }

    #[test]
    fn syndicate_offers_keep_the_title_and_standing_whatever_the_place_prefix() {
        let syndicates = serde_json::json!({ "syndicates": {
            "Steel Meridian": [
                { "item": "Vaykor Hek", "place": "Steel Meridian, General", "standing": 125000 },
                { "item": "Vaykor Marelok", "place": "Steel Meridian, Rank 4\u{a0}: Protector", "standing": 100000 },
                { "item": "Ammo Case", "place": "Steel Meridian, Rank 2:Valiant" },
            ],
            "Ostron": [{ "item": "Jai Blueprint", "place": "Ostron (Hok), Neutral", "standing": 1000 }],
        } });
        let catalog = parse_syndicate_store_catalog(Some(&syndicates), &[]);
        let offer = |syndicate: &str, name: &str| catalog[syndicate].iter().find(|o| o.name == name).map(|o| (o.tier.clone(), o.standing)).expect(name);
        assert_eq!(offer("Steel Meridian", "Vaykor Hek"), ("General".into(), Some(125_000)));
        assert_eq!(offer("Steel Meridian", "Vaykor Marelok"), ("Protector".into(), Some(100_000)));
        assert_eq!(offer("Steel Meridian", "Ammo Case"), ("Valiant".into(), None));
        assert_eq!(offer("Ostron", "Jai Blueprint"), ("(Hok), Neutral".into(), Some(1_000)));
    }
}
