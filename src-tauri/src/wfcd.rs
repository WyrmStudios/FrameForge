use std::collections::{HashMap, HashSet};
use std::io::{Cursor, Read};

#[derive(Clone, Debug)]
pub struct WfcdItem {
    pub name: String,
    pub unique_name: String,
    pub category: String,
    pub image_name: Option<String>,
    /// Some(true) = vaulted, Some(false) = unvaulted, None = no vault status (non-prime)
    pub vaulted: Option<bool>,
    pub ducats: Option<u32>,
    pub mastery_req: Option<u32>,
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
}

fn default_one() -> u32 { 1 }

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct RelicReward {
    pub unique_name: String,
    pub name: String,
    /// "Bronze" = Common, "Silver" = Uncommon, "Gold" = Rare
    pub rarity: String,
    pub image_name: Option<String>,
}

pub struct FetchResult {
    pub items: Vec<WfcdItem>,
    /// parent unique_name → list of components needed to craft it
    pub recipes: HashMap<String, Vec<RecipeComponent>>,
    /// component unique_name → list of relic unique_names that can drop it
    pub relic_drops: HashMap<String, Vec<String>>,
    /// relic unique_name → 6 rewards sorted Bronze×3, Silver×2, Gold×1
    pub relic_rewards: HashMap<String, Vec<RelicReward>>,
    /// blueprint_unique → (display name, ducats)
    /// Built from ExportRecipes × WFCD display_names. Used to enrich the frontend catalog.
    pub blueprint_names: HashMap<String, (String, Option<u32>)>,
    /// Canonical relic reward display names from the Warframe Wiki Module:Void.
    /// Lower-cased. Used as a name-based whitelist for the overlay catalog so that
    /// path-mismatch issues between ExportRecipes and WFCD never exclude a valid reward.
    pub wiki_reward_names: HashSet<String>,
}

/// Fetch the complete list of relic reward display names from the Warframe Wiki's
/// Module:Void Lua table via the MediaWiki API.
/// Returns a set of lower-cased names like "xaku prime neuroptics blueprint".
fn fetch_wiki_reward_names() -> HashSet<String> {
    let mut names: HashSet<String> = HashSet::new();

    // ── Source A: Module:Void wikitext ────────────────────────────────────────
    // Structured Lua table with Item + Part fields per relic reward entry.
    let url_mod = "https://wiki.warframe.com/api.php?\
                   action=parse&page=Module:Void&prop=wikitext&format=json";
    if let Some(body) = ureq::get(url_mod)
        .set("User-Agent", "WarframeCompanion/0.1")
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
        .set("User-Agent", "WarframeCompanion/0.1")
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

pub fn fetch_items() -> Result<FetchResult, String> {
    fetch_from_wfcd()
}

fn strip_tags(s: &str) -> &str {
    if s.starts_with('<') {
        s.find('>').map(|i| s[i + 1..].trim()).unwrap_or(s.trim())
    } else {
        s.trim()
    }
}

/// Fetch the LZMA-compressed Warframe public export index and return a map of
/// endpoint filename → full URL (e.g. "ExportRecipes_en.json!HASH" → full URL).
#[allow(dead_code)]
fn fetch_export_index() -> Result<Vec<String>, String> {
    let index_url = "https://origin.warframe.com/PublicExport/index_en.txt.lzma";
    let resp = ureq::get(index_url)
        .set("User-Agent", "WarframeCompanion/0.1")
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

/// Fetch ExportRecipes from warframe-public-export-plus (stable URL, pre-processed, always current).
/// Returns a map from resultType (= what gets crafted) → ExportRecipe.
fn fetch_export_recipes() -> Result<HashMap<String, ExportRecipe>, String> {
    // Try two URLs: the warframe-public-export-plus repo (pre-processed) and a fallback
    let urls = [
        "https://raw.githubusercontent.com/calamity-inc/warframe-public-export-plus/master/ExportRecipes.json",
        "https://raw.githubusercontent.com/calamity-inc/warframe-public-export-plus/HEAD/ExportRecipes.json",
    ];
    let url = urls[0]; // primary

    let json: serde_json::Value = ureq::get(url)
        .set("User-Agent", "FrameForge/0.1")
        .call()
        .map_err(|e| format!("ExportRecipes fetch: {}", e))?
        .into_json()
        .map_err(|e| format!("ExportRecipes parse: {}", e))?;

    let mut map: HashMap<String, ExportRecipe> = HashMap::new();

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
    Ok(map)
}

/// Build a recipe node. Prefers DE's ExportRecipes for sub-ingredients;
/// falls back to WFCD nested `components` for items not in ExportRecipes.
fn build_recipe_node(
    unique_name: String,
    name: String,
    count: u32,
    wfcd_json: Option<&serde_json::Value>,
    display_names: &HashMap<String, String>,
    export_recipes: &HashMap<String, ExportRecipe>,
    depth: u32,
) -> RecipeComponent {
    if depth > 6 {
        return RecipeComponent { unique_name, name, count, result_count: 1, components: vec![] };
    }

    let (result_count, components) = if let Some(recipe) = export_recipes.get(&unique_name) {
        let blueprint_name = display_names
            .get(&recipe.blueprint_unique)
            .cloned()
            .unwrap_or_else(|| format!("{} Blueprint", name));

        let mut components = vec![RecipeComponent {
            unique_name: recipe.blueprint_unique.clone(),
            name: blueprint_name,
            count: 1,
            result_count: 1,
            components: vec![],
        }];

        for (item_type, item_count) in &recipe.ingredients {
            let item_name = display_names
                .get(item_type)
                .cloned()
                .unwrap_or_else(|| item_type.split('/').last().unwrap_or("Unknown").to_string());
            components.push(build_recipe_node(
                item_type.clone(), item_name, *item_count,
                None, display_names, export_recipes, depth + 1,
            ));
        }
        (recipe.result_count, components)
    } else if let Some(json) = wfcd_json {
        let comps = json.get("components")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|c| {
                let cu = c["uniqueName"].as_str()?.trim().to_string();
                let raw = c["name"].as_str().unwrap_or("Unknown");
                let cn = display_names.get(&cu).cloned()
                    .unwrap_or_else(|| strip_tags(raw).to_string());
                let cc = c["itemCount"].as_u64().unwrap_or(1) as u32;
                Some(build_recipe_node(cu, cn, cc, Some(c), display_names, export_recipes, depth + 1))
            }).collect())
            .unwrap_or_default();
        (1, comps)
    } else {
        (1, vec![])
    };

    RecipeComponent { unique_name, name, count, result_count, components }
}

fn fetch_from_wfcd() -> Result<FetchResult, String> {
    let categories: &[(&str, &str)] = &[
        ("Misc",           "Resources"),
        ("Resources",      "Resources"),
        ("Mods",           "Mods"),
        ("Relics",         "Relics"),
        ("Warframes",      "Warframes"),
        ("Primary",        "Primary"),
        ("Secondary",      "Secondary"),
        ("Melee",          "Melee"),
        ("Arcanes",        "Arcanes"),
        ("Sentinels",      "Companions"),
        ("SentinelWeapons","Companions"),
        ("Pets",           "Companions"),
        ("Archwing",       "Archwing"),
        ("Arch-Gun",       "Archwing"),
        ("Arch-Melee",     "Archwing"),
        ("Gear",           "Misc"),
        ("Fish",           "Misc"),
    ];

    let mut items: Vec<WfcdItem> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut errors: Vec<String> = Vec::new();
    let mut raw_craftable: Vec<(String, serde_json::Value)> = Vec::new();
    // Relics stored separately: (relic_unique_name, rewards_array)
    let mut raw_relics: Vec<(String, serde_json::Value)> = Vec::new();

    // ── Two-pass fetch ───────────────────────────────────────────────────────
    // Pass 1: Fetch all files → collect top-level unique_names.
    //   "Bronco" (Secondary.json top-level) must be known BEFORE it appears as
    //   a component of "Akbolto" (Primary.json) so it keeps "Secondary" not "Parts".
    // Pass 2: Process items and components using the cached data.
    // ────────────────────────────────────────────────────────────────────────
    let mut all_files: Vec<(String, Vec<serde_json::Value>)> = Vec::new(); // (category, items)
    let mut top_level_uniques: HashSet<String> = HashSet::new();

    for (file, category) in categories {
        let url = format!(
            "https://raw.githubusercontent.com/WFCD/warframe-items/master/data/json/{}.json",
            file
        );
        match ureq::get(&url).set("User-Agent", "WarframeCompanion/0.1").call() {
            Ok(resp) => {
                let json: serde_json::Value = match resp.into_json() {
                    Ok(j) => j,
                    Err(e) => { errors.push(format!("{}: {}", file, e)); continue; }
                };
                if let Some(arr) = json.as_array() {
                    // Mini-pass: record all top-level unique_names in this file
                    for item in arr.iter() {
                        if let Some(u) = item.get("uniqueName").and_then(|v| v.as_str()) {
                            top_level_uniques.insert(u.trim().to_string());
                        }
                    }
                    all_files.push((category.to_string(), arr.clone()));
                }
            }
            Err(e) => errors.push(format!("{}: {}", file, e)),
        }
    }

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

            let image_name = item.get("imageName").and_then(|v| v.as_str()).map(|s| s.to_string());
            let vaulted     = item.get("vaulted").and_then(|v| v.as_bool());
            let ducats      = item.get("ducats").and_then(|v| v.as_u64()).map(|n| n as u32);
            let mastery_req = item.get("masteryReq").and_then(|v| v.as_u64()).map(|n| n as u32);

            // Correct category before inserting:
            // • Blueprint items always go to "Blueprints"
            // • Non-relic items miscategorised under "Relics" → "Misc"
            let corrected_cat = if name.contains("Blueprint") {
                "Blueprints".to_string()
            } else if category == "Relics" {
                let n = name.to_lowercase();
                if n.ends_with("intact") || n.ends_with("exceptional")
                    || n.ends_with("flawless") || n.ends_with("radiant")
                {
                    "Relics".to_string()
                } else {
                    "Misc".to_string() // blueprints/segments/etc. wrongly in WFCD Relics file
                }
            } else {
                category.clone()
            };

            if seen.insert(unique_name.clone()) {
                items.push(WfcdItem {
                    name: name.clone(),
                    unique_name: unique_name.clone(),
                    category: corrected_cat,
                    image_name: image_name.clone(),
                    vaulted, ducats, mastery_req,
                });
            }

            if let Some(comps) = item.get("components").and_then(|v| v.as_array()) {
                if !comps.is_empty() {
                    raw_craftable.push((unique_name.clone(), item.clone()));
                }
            }

            // Collect relic reward data
            if let Some(rewards_val) = item.get("rewards") {
                let flat: Vec<serde_json::Value> = if let Some(arr) = rewards_val.as_array() {
                    arr.clone()
                } else if let Some(obj) = rewards_val.as_object() {
                    obj.get("Intact")
                        .or_else(|| obj.values().next())
                        .and_then(|v| v.as_array())
                        .cloned()
                        .unwrap_or_default()
                } else { vec![] };
                if !flat.is_empty() {
                    raw_relics.push((unique_name.clone(), serde_json::Value::Array(flat)));
                }
            }

            // Add component parts to catalog
            if let Some(comps) = item.get("components").and_then(|v| v.as_array()) {
                for comp in comps {
                    let cname = match comp.get("name").and_then(|v| v.as_str()) {
                        Some(n) => n.trim(),
                        None => continue,
                    };
                    let cunique = match comp.get("uniqueName").and_then(|v| v.as_str()) {
                        Some(u) => u.trim().to_string(),
                        None => continue,
                    };
                    let is_part = cunique.starts_with("/Lotus/Types/Recipes/")
                        || cunique.starts_with("/Lotus/Powersuits/")
                        || cunique.starts_with("/Lotus/Weapons/")
                        || cunique.starts_with("/Lotus/Companions/")
                        || cunique.starts_with("/Lotus/Sentinels/")
                        || cunique.starts_with("/Lotus/Archwing/")
                        || cname.contains("Blueprint");
                    if !is_part { continue; }

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
                            || cunique.starts_with("/Lotus/Archwing/")
                        {
                            if cname.starts_with(&name) { cname.to_string() }
                            else { format!("{} {}", name, cname) }
                        } else {
                            format!("{} {}", name, cname)
                        };
                        if raw_comp_name.trim() == "Blueprint" || raw_comp_name.trim().is_empty() {
                            seen.remove(&cunique); continue;
                        }
                        // Warframe (and companion/archwing) component blueprints drop from relics
                        // and are displayed in-game with "Blueprint" in the name
                        // (e.g. "Lavos Prime Neuroptics Blueprint").
                        // WFCD stores the component as just "Neuroptics", so we append it here.
                        // Weapon parts (category Primary/Secondary/Melee) intentionally excluded:
                        // they appear in-game without "Blueprint" ("Braton Prime Stock", not "...Blueprint").
                        let comp_name = if comp_cat == "Blueprints"
                            && (category == "Warframes" || category == "Companions" || category == "Archwing")
                            && !raw_comp_name.ends_with("Blueprint")
                        {
                            format!("{} Blueprint", raw_comp_name)
                        } else {
                            raw_comp_name
                        };
                        items.push(WfcdItem {
                            name: comp_name.clone(),
                            unique_name: cunique.clone(),
                            category: comp_cat.to_string(),
                            image_name: comp_image.clone(),
                            vaulted: None,
                            ducats: comp.get("ducats").and_then(|v| v.as_u64()).map(|n| n as u32),
                            mastery_req: None,
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

    if items.is_empty() {
        return Err(format!("All sources failed: {}", errors.join("; ")));
    }

    let display_names: HashMap<String, String> = items
        .iter()
        .map(|i| (i.unique_name.clone(), i.name.clone()))
        .collect();

    // Fetch DE's authoritative recipe data (best-effort; fall back to WFCD-only if it fails)
    let export_recipes = fetch_export_recipes().unwrap_or_default();

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

        // Strategy A: ExportRecipes
        for (result_type, recipe) in &export_recipes {
            let bp_unique = &recipe.blueprint_unique;
            if seen.contains(bp_unique) { continue; }
            if !result_type.starts_with("/Lotus/Powersuits/") { continue; }
            let result_name = match display_names.get(result_type) {
                Some(n) => n.clone(),
                None => continue,
            };
            if seen.insert(bp_unique.clone()) {
                bp_items.push(WfcdItem {
                    name:        format!("{} Blueprint", result_name),
                    unique_name: bp_unique.clone(),
                    category:    "Blueprints".to_string(),
                    image_name:  image_by_unique.get(result_type).and_then(|i| i.clone()),
                    vaulted:     None,
                    ducats:      None,
                    mastery_req: None,
                });
            }
        }

        // Strategy B: synthetic fallback for every warframe component not covered above.
        // Covers the case where export_recipes fetch failed or is incomplete.
        for item in items.iter() {
            if !item.unique_name.starts_with("/Lotus/Powersuits/") { continue; }
            if item.name.ends_with("Blueprint") { continue; }
            if item.category == "Blueprints" { continue; }
            // Derive a synthetic blueprint unique_name by appending "Blueprint"
            let bp_unique = format!("{}Blueprint", item.unique_name);
            if seen.contains(&bp_unique) { continue; }
            if seen.insert(bp_unique.clone()) {
                bp_items.push(WfcdItem {
                    name:        format!("{} Blueprint", item.name),
                    unique_name: bp_unique,
                    category:    "Blueprints".to_string(),
                    image_name:  item.image_name.clone(),
                    vaulted:     None,
                    ducats:      None,
                    mastery_req: None,
                });
            }
        }

        // Debug: write counts to temp file so we can diagnose issues
        let powersuits_in_recipes = export_recipes.keys()
            .filter(|k| k.starts_with("/Lotus/Powersuits/")).count();
        let _ = std::fs::write(
            std::env::temp_dir().join("frameforge_wfcd_debug.txt"),
            format!(
                "export_recipes total={} powersuits_entries={}\n\
                 strategy_a candidates checked={}\n\
                 strategy_b items scanned={}\n\
                 total bp_items added={}\n\
                 first 5 bp items:\n{}",
                export_recipes.len(),
                powersuits_in_recipes,
                export_recipes.iter().filter(|(k,_)| k.starts_with("/Lotus/Powersuits/")).count(),
                items.iter().filter(|i| i.unique_name.starts_with("/Lotus/Powersuits/")).count(),
                bp_items.len(),
                bp_items.iter().take(5).map(|i| format!("  {} = {}", i.unique_name, i.name)).collect::<Vec<_>>().join("\n")
            )
        );

        items.extend(bp_items);
    }

    // Build recipe trees
    let mut recipes: HashMap<String, Vec<RecipeComponent>> = HashMap::new();
    for (parent_unique, item_json) in &raw_craftable {
        if let Some(comps) = item_json.get("components").and_then(|v| v.as_array()) {
            let tree: Vec<RecipeComponent> = comps.iter().filter_map(|c| {
                let cu = c["uniqueName"].as_str()?.trim().to_string();
                let raw = c["name"].as_str().unwrap_or("Unknown");
                let cn = display_names.get(&cu).cloned()
                    .unwrap_or_else(|| strip_tags(raw).to_string());
                let cc = c["itemCount"].as_u64().unwrap_or(1) as u32;
                Some(build_recipe_node(
                    cu, cn, cc, Some(c), &display_names, &export_recipes, 0,
                ))
            }).collect();
            if !tree.is_empty() {
                recipes.insert(parent_unique.clone(), tree);
            }
        }
    }

    // Build relic drop map: component unique_name → [relic unique_names that drop it]
    // WFCD relic rewards list items by "itemName" (display name).
    // We match against our catalog names to get unique_names.
    let name_to_unique: HashMap<String, String> = items.iter()
        .map(|i| (i.name.clone(), i.unique_name.clone()))
        .collect();

    let mut relic_drops: HashMap<String, Vec<String>> = HashMap::new();
    for (relic_unique, rewards_val) in &raw_relics {
        if let Some(rewards) = rewards_val.as_array() {
            for reward in rewards {
                // WFCD structure: reward.item.name  (not reward.itemName)
                let item_name = reward
                    .get("item").and_then(|v| v.get("name")).and_then(|v| v.as_str())
                    .or_else(|| reward.get("itemName").and_then(|v| v.as_str()));
                if let Some(name) = item_name {
                    if let Some(comp_unique) = name_to_unique.get(name) {
                        relic_drops.entry(comp_unique.clone()).or_default().push(relic_unique.clone());
                    }
                }
            }
        }
    }

    // Image lookup maps for relic rewards
    let image_by_unique: HashMap<String, String> = items.iter()
        .filter_map(|i| i.image_name.as_ref().map(|img| (i.unique_name.clone(), img.clone())))
        .collect();
    // Name-based lookup (lowercase) for when unique_names differ between data sources
    let image_by_name: HashMap<String, String> = items.iter()
        .filter_map(|i| i.image_name.as_ref().map(|img| (i.name.to_lowercase(), img.clone())))
        .collect();

    // Build relic rewards map: relic unique_name → rewards sorted Bronze/Silver/Gold
    let mut relic_rewards: HashMap<String, Vec<RelicReward>> = HashMap::new();
    for (relic_unique, rewards_val) in &raw_relics {
        if let Some(rewards) = rewards_val.as_array() {
            let mut list: Vec<RelicReward> = rewards.iter().filter_map(|r| {
                let item_unique = r.get("item")
                    .and_then(|v| v.get("uniqueName"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("").trim().to_string();
                if item_unique.is_empty() { return None; }
                let item_name = r.get("item").and_then(|v| v.get("name")).and_then(|v| v.as_str())
                    .or_else(|| r.get("itemName").and_then(|v| v.as_str()))
                    .unwrap_or("Unknown").to_string();
                let rarity_raw = r.get("rarity").and_then(|v| v.as_str()).unwrap_or("common").to_lowercase();
                let rarity = match rarity_raw.as_str() {
                    "uncommon" => "Silver",
                    "rare"     => "Gold",
                    _          => "Bronze",
                }.to_string();
                let image_name = r.get("item")
                    .and_then(|v| v.get("imageName"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    // Try unique_name lookup in catalog
                    .or_else(|| image_by_unique.get(&item_unique).cloned())
                    // Try name lookup (lowercase) — handles path mismatches between data sources
                    .or_else(|| image_by_name.get(&item_name.to_lowercase()).cloned())
                    // Try name without "Blueprint" suffix
                    .or_else(|| {
                        let no_bp = item_name.to_lowercase().replace(" blueprint", "");
                        image_by_name.get(&no_bp).cloned()
                    })
                    // For prime parts: fall back to parent prime item's image
                    // e.g. "Yareli Prime Blueprint" → look up "Yareli Prime"
                    .or_else(|| {
                        if let Some(idx) = item_name.find(" Prime") {
                            let prime_name = item_name[..idx + " Prime".len()].to_lowercase();
                            image_by_name.get(&prime_name).cloned()
                        } else {
                            None
                        }
                    });
                Some(RelicReward { unique_name: item_unique, name: item_name, rarity, image_name })
            }).collect();
            list.sort_by_key(|r| match r.rarity.as_str() { "Silver" => 1u8, "Gold" => 2, _ => 0 });
            if !list.is_empty() {
                relic_rewards.insert(relic_unique.clone(), list);
            }
        }
    }

    // Build blueprint_names: blueprint_path → (display_name, ducats)
    // Lets the frontend create virtual catalog entries for component blueprints that
    // are tracked by the API but may be absent from the WFCD catalog.
    // Ducats are pulled from any existing WFCD item that matches the blueprint path.
    let ducat_by_unique: HashMap<String, u32> = items.iter()
        .filter_map(|i| i.ducats.map(|d| (i.unique_name.clone(), d)))
        .collect();

    let blueprint_names: HashMap<String, (String, Option<u32>)> = export_recipes.iter()
        .filter_map(|(result_type, recipe)| {
            let display = display_names.get(result_type)?;
            let name   = format!("{} Blueprint", display);
            let ducats = ducat_by_unique.get(&recipe.blueprint_unique).copied();
            Some((recipe.blueprint_unique.clone(), (name, ducats)))
        })
        .collect();

    // Fetch the canonical reward name list from the Warframe Wiki.
    // This is non-blocking on failure — if the wiki is unreachable, we fall back
    // to the existing prime/forma filters in the overlay catalog builder.
    let wiki_reward_names = fetch_wiki_reward_names();

    Ok(FetchResult { items, recipes, relic_drops, relic_rewards, blueprint_names, wiki_reward_names })
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
        image_name: None, vaulted: None, ducats: None, mastery_req: None,
    })
    .collect()
}
