use tauri::State;
use crate::app_state::AppState;

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
pub(crate) struct SyndicateStore {
    name: String,
    items: Vec<SyndicateStoreItem>,
}

/// Returns all syndicate stores with owned quantities cross-referenced from the live inventory.
#[tracing::instrument(level = "debug", skip_all)]
#[tauri::command]
pub(crate) fn get_syndicate_stores(state: State<AppState>) -> Vec<SyndicateStore> {
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

/// Item display names per dojo research room, without the " Blueprint"
/// suffix. Each is looked up by name in the WFCD catalog, and a name the
/// catalog lacks is skipped.
pub(crate) const LABS: &[(&str, &[&str])] = &[
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

pub(crate) fn research_lab(name: &str) -> Option<&'static str> {
    LABS.iter().find(|(_, names)| names.iter().any(|n| n.eq_ignore_ascii_case(name))).map(|(lab, _)| *lab)
}

/// One vendor syndicate as the drop data names it, with the tag the
/// account blob's `Affiliations` uses and its rank titles from rank 1 up.
/// Rank 0 is "Neutral" everywhere; Kahl's Garrison starts at rank 1 and
/// Cephalon Simaris has no ranks at all.
pub(crate) struct Syndicate {
    pub(crate) name: &'static str,
    pub(crate) tag: &'static str,
    ranks: &'static [&'static str],
}

const SYNDICATES: &[Syndicate] = &[
    Syndicate { name: "Steel Meridian", tag: "SteelMeridianSyndicate", ranks: &["Brave", "Valiant", "Defender", "Protector", "General"] },
    Syndicate { name: "Arbiters of Hexis", tag: "ArbitersSyndicate", ranks: &["Principled", "Authentic", "Lawful", "Crusader", "Maxim"] },
    Syndicate { name: "Cephalon Suda", tag: "CephalonSudaSyndicate", ranks: &["Competent", "Intriguing", "Intelligent", "Wise", "Genius"] },
    Syndicate { name: "The Perrin Sequence", tag: "PerrinSyndicate", ranks: &["Associate", "Senior Associate", "Executive", "Senior Executive", "Partner"] },
    Syndicate { name: "Red Veil", tag: "RedVeilSyndicate", ranks: &["Respected", "Honored", "Esteemed", "Revered", "Exalted"] },
    Syndicate { name: "New Loka", tag: "NewLokaSyndicate", ranks: &["Humane", "Bountiful", "Benevolent", "Pure", "Flawless"] },
    Syndicate { name: "Conclave", tag: "ConclaveSyndicate", ranks: &["Mistral", "Whirlwind", "Tempest", "Hurricane", "Typhoon"] },
    Syndicate { name: "Cephalon Simaris", tag: "LibrarySyndicate", ranks: &[] },
    Syndicate { name: "Operational Supply", tag: "EventSyndicate", ranks: &["Collaborator", "Defender", "Champion"] },
    Syndicate { name: "Ostron", tag: "CetusSyndicate", ranks: &["Offworlder", "Visitor", "Trusted", "Surah", "Kin"] },
    Syndicate { name: "The Quills", tag: "QuillsSyndicate", ranks: &["Mote", "Observer", "Adherent", "Instrument", "Architect"] },
    Syndicate { name: "Solaris United", tag: "SolarisSyndicate", ranks: &["Outworlder", "Rapscallion", "Doer", "Cove", "Old Mate"] },
    Syndicate { name: "Vox Solaris", tag: "VoxSyndicate", ranks: &["Operative", "Agent", "Hand", "Instrument", "Shadow"] },
    Syndicate { name: "Ventkids", tag: "VentKidsSyndicate", ranks: &["Glinty", "Whozit", "Proper Felon", "Primo", "Logical"] },
    Syndicate { name: "Entrati", tag: "EntratiSyndicate", ranks: &["Stranger", "Acquaintance", "Associate", "Friend", "Family"] },
    Syndicate { name: "Necraloid", tag: "NecraloidSyndicate", ranks: &["Clearance Agnesis", "Clearance Modus", "Clearance Odima"] },
    Syndicate { name: "The Holdfasts", tag: "ZarimanSyndicate", ranks: &["Fallen", "Watcher", "Guardian", "Seraph", "Angel"] },
    Syndicate { name: "Kahl's Garrison", tag: "KahlSyndicate", ranks: &["Shelter", "Encampment", "Fort", "Settlement", "Home"] },
    Syndicate { name: "Cavia", tag: "EntratiLabSyndicate", ranks: &["Stranger", "Acquaintance", "Associate", "Friend", "Family"] },
    Syndicate { name: "The Hex", tag: "HexSyndicate", ranks: &["Stranger", "Acquaintance", "Associate", "Friend", "Family"] },
];

pub(crate) fn syndicate(name: &str) -> Option<&'static Syndicate> {
    SYNDICATES.iter().find(|s| s.name == name)
}

impl Syndicate {
    /// The rank an offer tier names. The drop data prefixes open-world
    /// tiers with the vendor, as in "(Hok), Neutral", so only the last
    /// comma-separated part is the title. A title the table does not list
    /// is `None`, which the caller treats as no rank requirement.
    pub(crate) fn rank_of(&self, tier: &str) -> Option<u32> {
        let title = tier.rsplit(", ").next().expect("rsplit yields at least one part").trim();
        if title == "Neutral" { return Some(0); }
        self.ranks.iter().position(|r| *r == title).map(|i| i as u32 + 1)
    }
}

/// Returns clan dojo research lab stores, one per lab. Items come from the
/// `LABS` name table because WFCD paths carry no lab segment to scan for.
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
#[tracing::instrument(level = "debug", skip_all)]
#[tauri::command]
pub(crate) fn get_research_lab_stores(state: State<AppState>) -> Vec<SyndicateStore> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offer_tiers_resolve_to_ranks_through_the_vendor_prefix() {
        let meridian = syndicate("Steel Meridian").expect("listed");
        assert_eq!((meridian.tag, meridian.rank_of("Protector"), meridian.rank_of("Neutral"), meridian.rank_of("Exalted")), ("SteelMeridianSyndicate", Some(4), Some(0), None));
        let ostron = syndicate("Ostron").expect("listed");
        assert_eq!((ostron.rank_of("(Hok), Neutral"), ostron.rank_of("(Old Man Suumbaat), Surah")), (Some(0), Some(4)));
        assert_eq!(syndicate("Kahl's Garrison").and_then(|s| s.rank_of("Home")), Some(5), "Kahl's Garrison starts at rank 1");
        assert_eq!(syndicate("Cephalon Simaris").and_then(|s| s.rank_of("")), None);
        assert!(syndicate("Baro Ki'Teer").is_none());
    }
}
