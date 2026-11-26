//! Inventory-aware recipe expansion shared by the mastery views, the Foundry
//! and the tracked list.
//!
//! Targets plan in display order against one projected stock ledger, so an
//! ingredient two targets share is available to the first and short for the
//! second. Owned stock covers a need before anything expands. Only the
//! missing quantity is crafted, and only where the intermediate's blueprint
//! is in hand or is consumed per craft. A reusable blueprint the player lacks
//! (the Baro resource recipes, dojo research) is an acquisition of its own,
//! so a resource behind one stays a shortage rather than unfolding into the
//! hundreds of thousands of Alloy Plate its recipe asks for.

use std::sync::Arc;
use std::collections::{HashMap, HashSet};
use crate::app_state::AppState;
use crate::inventory_state::{load_inventory_state_cache, InventoryStateCache, CREDITS_PATH};
use crate::mastery_rules::DEFAULT_RANK_CAP;
use crate::monitor::CraftingJob;
use crate::wfcd::{RecipeComponent, WfcdItem};
use tauri::Manager;

/// `Partial` outranks every other state, so a line that stock half covers
/// reads yellow whatever the remainder would be. `MasterFirst` and
/// `Building` describe the item itself and only apply when stock covers
/// none of the line.
#[derive(serde::Serialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub(crate) enum IngredientState {
    Owned,
    Partial,
    Missing,
    /// The blueprint is in hand and every ingredient is ready.
    Buildable,
    /// The blueprint is in hand but an ingredient is short somewhere below.
    Blocked,
    /// The part's own blueprint is short, either a consumable one nobody has
    /// or a reusable one never acquired, so the part never unfolds.
    BlueprintMissing,
    /// The player owns an unmastered copy, which the ledger will not spend.
    MasterFirst,
    Building,
}

#[derive(serde::Serialize, Clone, PartialEq, Eq, Debug)]
pub(crate) struct Requirement {
    pub(crate) unique_name: String,
    pub(crate) name: String,
    pub(crate) image_name: Option<String>,
    pub(crate) category: Option<String>,
    pub(crate) needed: u32,
    /// The projected stock before this recipe drew on it, which can exceed
    /// `needed`.
    pub(crate) owned: u32,
    /// How many came out of projected stock. A reusable blueprint counts once
    /// and is not consumed.
    pub(crate) from_stock: u32,
    /// The remainder that neither stock nor a build covers.
    pub(crate) short: u32,
    pub(crate) state: IngredientState,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub(crate) reusable: bool,
    /// Marks a built part's own blueprint. The part's line already carries
    /// its state, so the icon row hides it.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub(crate) part_blueprint: bool,
}

#[derive(serde::Serialize, Clone, PartialEq, Eq, Debug)]
pub(crate) struct Build {
    pub(crate) unique_name: String,
    pub(crate) name: String,
    pub(crate) crafts: u32,
}

#[derive(serde::Serialize, Clone, PartialEq, Eq, Debug, Default)]
pub(crate) struct CraftPlan {
    /// One line per item the target needs, blueprints included, in recipe
    /// order. A component listed twice merges into one line.
    pub(crate) requirements: Vec<Requirement>,
    pub(crate) builds: Vec<Build>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) level_first: Vec<LevelFirst>,
    /// The build price of the target plus every build. It turns None as soon
    /// as any of those recipes carries no price, so a partial sum never reads
    /// as the full cost.
    pub(crate) credits: Option<u64>,
    /// Credits still missing once earlier targets took theirs. It stays 0
    /// while the cost or the balance is unknown.
    pub(crate) credits_short: u64,
    /// The projected balance before this plan took its price. It is None
    /// when the cost is unknown as well as when the balance is.
    pub(crate) credit_balance: Option<u64>,
}

#[derive(serde::Serialize, Clone, PartialEq, Eq, Debug)]
pub(crate) struct LevelFirst {
    pub(crate) unique_name: String,
    pub(crate) name: String,
    pub(crate) gain: u32,
}

impl CraftPlan {
    pub(crate) fn shortages(&self) -> impl Iterator<Item = &Requirement> {
        self.requirements.iter().filter(|r| r.short > 0)
    }

    pub(crate) fn craftable_now(&self) -> bool {
        self.builds.is_empty() && self.shortages().next().is_none()
    }

    pub(crate) fn buildable(&self) -> bool {
        !self.builds.is_empty() && self.shortages().next().is_none()
    }

    /// Foundry jobs and the catalogue live outside the ledger, so they land
    /// after the plan is drawn.
    pub(crate) fn decorate(&mut self, catalogue: &HashMap<String, Art>, building: impl Fn(&str) -> bool) {
        for line in &mut self.requirements {
            if let Some(art) = catalogue.get(&line.unique_name) {
                line.image_name = art.image_name.clone();
                line.category = Some(art.category.clone());
            }
            if line.from_stock == 0 && building(&line.unique_name) { line.state = IngredientState::Building; }
        }
    }

    /// A second listing of a component reads the stock the first listing
    /// already drew on, so `owned` keeps the first reading.
    fn require(&mut self, new: Requirement) {
        match self.requirements.iter_mut().find(|r| r.unique_name == new.unique_name) {
            Some(line) => {
                line.needed += new.needed;
                line.from_stock += new.from_stock;
                line.short += new.short;
                // A second listing of a blocked line stays blocked, because its
                // ingredients draw on the stock the first listing already ran
                // short of.
                let unmet = if line.state == IngredientState::Blocked { IngredientState::Blocked } else { new.state };
                line.state = stock_state(line.from_stock, line.needed, unmet);
            }
            None => self.requirements.push(new),
        }
    }

    /// A built line's state waits on its ingredients, which are only known
    /// once they have expanded below it. An owned unmastered copy keeps
    /// `MasterFirst` although the ledger builds another, since the copy to
    /// level is what the player needs to see.
    fn settle_build(&mut self, component: &RecipeComponent, ready: bool) {
        let blueprint = component.components.iter().find(|c| is_blueprint(c))
            .and_then(|bp| self.requirements.iter_mut().find(|r| r.unique_name == bp.unique_name));
        let blueprint_short = blueprint.is_some_and(|bp| { bp.part_blueprint = true; bp.short > 0 });
        let line = self.requirements.iter_mut().find(|r| r.unique_name == component.unique_name).expect("the built line was required first");
        if line.from_stock == 0 && !matches!(line.state, IngredientState::Blocked | IngredientState::MasterFirst) {
            line.state = if blueprint_short { IngredientState::BlueprintMissing } else if ready { IngredientState::Buildable } else { IngredientState::Blocked };
        }
    }

    fn build(&mut self, component: &RecipeComponent, crafts: u32) {
        match self.builds.iter_mut().find(|b| b.unique_name == component.unique_name) {
            Some(line) => line.crafts += crafts,
            None => self.builds.push(Build { unique_name: component.unique_name.clone(), name: component.name.clone(), crafts }),
        }
    }
}

fn stock_state(from_stock: u32, needed: u32, unmet: IngredientState) -> IngredientState {
    if from_stock >= needed { IngredientState::Owned } else if from_stock > 0 { IngredientState::Partial } else { unmet }
}

#[derive(Clone)]
pub(crate) struct Ledger<'a> {
    stock: HashMap<&'a str, i64>,
    unmastered: HashSet<&'a str>,
}

impl<'a> Ledger<'a> {
    /// Owned equipment stays out of stock, so a weapon the player owns is
    /// never spent on the next one. An allowance admits up to that many
    /// copies of a mastered piece the player owns. The returned list names
    /// the allowances refused, whether unmastered, of unknown mastery, or not
    /// owned.
    pub(crate) fn new(
        stock: &'a HashMap<String, i64>,
        equipment: &'a HashMap<String, i64>,
        mastered: &HashSet<&str>,
        allowances: &HashMap<String, u32>,
    ) -> (Self, Vec<String>) {
        let mut stock: HashMap<&'a str, i64> = stock.iter()
            .filter(|(path, _)| !equipment.contains_key(*path))
            .map(|(path, &n)| (path.as_str(), n))
            .collect();
        let mut rejected = vec![];
        for (path, &allowed) in allowances {
            match equipment.get_key_value(path) {
                Some((path, &owned)) if mastered.contains(path.as_str()) => {
                    stock.insert(path, i64::from(allowed).min(owned));
                }
                _ => rejected.push(path.clone()),
            }
        }
        rejected.sort();
        let unmastered = equipment.iter()
            .filter(|(path, &n)| n > 0 && !mastered.contains(path.as_str()))
            .map(|(path, _)| path.as_str())
            .collect();
        (Self { stock, unmastered }, rejected)
    }

    pub(crate) fn plan(&mut self, target: &str, components: &'a [RecipeComponent]) -> CraftPlan {
        self.plan_with(target, components, true)
    }

    /// Plans a need that no blueprint prices, such as the Forma a level cap
    /// takes, so the credits are those of the builds alone.
    pub(crate) fn plan_ingredients(&mut self, target: &str, components: &'a [RecipeComponent]) -> CraftPlan {
        self.plan_with(target, components, false)
    }

    fn plan_with(&mut self, target: &str, components: &'a [RecipeComponent], priced_by_blueprint: bool) -> CraftPlan {
        let mut plan = CraftPlan { credits: Some(0), ..Default::default() };
        self.expand(target, components, 1, &mut plan, priced_by_blueprint);
        if let (Some(cost), Some(&balance)) = (plan.credits, self.stock.get(CREDITS_PATH)) {
            let taken = cost.min(balance.max(0) as u64);
            self.stock.insert(CREDITS_PATH, balance - taken as i64);
            plan.credits_short = cost - taken;
            plan.credit_balance = Some(balance.max(0) as u64);
        }
        plan
    }

    pub(crate) fn supply(&mut self, components: &'a [RecipeComponent], quantities: &mut HashMap<&str, u32>) {
        for component in components {
            if let Some(count) = quantities.remove(component.unique_name.as_str()) {
                *self.stock.entry(&component.unique_name).or_insert(0) += i64::from(count);
            } else {
                self.supply(&component.components, quantities);
            }
        }
    }

    fn expand(&mut self, target: &str, components: &'a [RecipeComponent], crafts: u32, plan: &mut CraftPlan, priced_by_blueprint: bool) -> bool {
        let mut priced = !priced_by_blueprint;
        let mut ready = true;
        for (count, component) in merged(components) {
            // A recipe occasionally lists its own result among the ingredients.
            if component.unique_name == target { continue; }
            let blueprint = is_blueprint(component);
            if blueprint {
                plan.credits = match (plan.credits, component.credits) {
                    (Some(total), Some(price)) => Some(total + u64::from(price) * u64::from(crafts)),
                    _ => None,
                };
                priced = true;
            }
            let keep = blueprint && component.reusable;
            let needed = if keep { 1 } else { count * crafts };
            let path = component.unique_name.as_str();
            let have = u32::try_from(self.stock.get(path).copied().unwrap_or(0).max(0)).unwrap_or(u32::MAX);
            let from_stock = needed.min(have);
            if !keep { *self.stock.entry(path).or_insert(0) -= i64::from(from_stock); }
            let missing = needed - from_stock;
            let has_recipe = !component.components.is_empty();
            let expands = missing > 0 && has_recipe && self.can_expand(component);
            let unmet = if self.unmastered.contains(path) { IngredientState::MasterFirst }
                else if missing > 0 && has_recipe && !expands { IngredientState::BlueprintMissing }
                else { IngredientState::Missing };
            let state = stock_state(from_stock, needed, unmet);
            plan.require(Requirement {
                unique_name: component.unique_name.clone(), name: component.name.clone(), image_name: None, category: None,
                needed, owned: have, from_stock, short: if expands { 0 } else { missing }, state, reusable: keep, part_blueprint: false,
            });
            if expands {
                let per_craft = component.result_count.max(1);
                let builds = missing.div_ceil(per_craft);
                let surplus = builds * per_craft - missing;
                if surplus > 0 { *self.stock.entry(path).or_insert(0) += i64::from(surplus); }
                plan.build(component, builds);
                let built = self.expand(target, &component.components, builds, plan, true);
                plan.settle_build(component, built);
                ready &= built;
            } else {
                ready &= missing == 0;
            }
        }
        if !priced { plan.credits = None; }
        ready
    }

    /// A missing consumable blueprint shows up as a shortage like any part,
    /// because the player buys or finds one per craft. A missing reusable
    /// blueprint is a one-off acquisition that gates the whole recipe, so the
    /// intermediate does not expand until it is in hand.
    fn can_expand(&self, intermediate: &RecipeComponent) -> bool {
        intermediate.components.iter().filter(|c| is_blueprint(c))
            .all(|bp| !bp.reusable || self.stock.get(bp.unique_name.as_str()).is_some_and(|&n| n > 0))
    }
}

pub(crate) fn recipe_consumers(recipes: &HashMap<String, Vec<RecipeComponent>>) -> HashMap<String, Vec<String>> {
    fn record(result: &str, components: &[RecipeComponent], index: &mut HashMap<String, Vec<String>>) {
        for component in components {
            if component.unique_name == result { continue; }
            index.entry(component.unique_name.clone()).or_default().push(result.into());
            record(result, &component.components, index);
        }
    }
    let mut index = HashMap::new();
    for (result, components) in recipes { record(result, components, &mut index); }
    for consumers in index.values_mut() { consumers.sort(); consumers.dedup(); }
    index
}

pub(crate) const FORMA: &str = "/Lotus/Types/Items/MiscItems/Forma";

/// The Forma a copy needs to reach its rank cap, as one ingredient line that
/// expands through Forma's own recipe like any intermediate.
pub(crate) fn forma_ingredient(count: u32, recipes: &HashMap<String, Vec<RecipeComponent>>) -> Vec<RecipeComponent> {
    vec![RecipeComponent {
        unique_name: FORMA.into(), name: "Forma".into(), count, result_count: 1,
        components: recipes.get(FORMA).cloned().unwrap_or_default(), credits: None, reusable: false,
    }]
}

/// `standalone` plans each target against the whole stock, as if it were the
/// only one. The Foundry grid reads that so a card agrees with its modal,
/// which plans the item alone.
pub(crate) fn plan_targets(inventory: &InventoryStateCache, recipes: &HashMap<String, Vec<RecipeComponent>>, targets: &[String], standalone: bool) -> Vec<CraftPlan> {
    let (stock, owned, ranks) = (inventory.stackable_quantities(), inventory.owned_copies(), inventory.mastery_data());
    let (mut ledger, _) = Ledger::new(&stock, &owned, &mastered_by_rank(&ranks), &HashMap::new());
    targets.iter().map(|target| {
        let recipe: &[RecipeComponent] = recipes.get(target).map_or(&[], Vec::as_slice);
        if standalone { ledger.clone().plan(target, recipe) } else { ledger.plan(target, recipe) }
    }).collect()
}

/// The inventory cache carries ranks but no rank caps, so a rank-40 weapon
/// at 30 passes as mastered here. The mastery views know the caps and judge
/// their own plans.
fn mastered_by_rank(ranks: &HashMap<String, u32>) -> HashSet<&str> {
    ranks.iter().filter(|(_, &rank)| rank >= DEFAULT_RANK_CAP).map(|(path, _)| path.as_str()).collect()
}

#[tauri::command]
pub(crate) async fn plan_crafts(app: tauri::AppHandle, unique_names: Vec<String>, standalone: Option<bool>) -> Result<Vec<CraftPlan>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let inventory = load_inventory_state_cache(&state.inventory_state_cache_path);
        let recipes = Arc::clone(&state.recipes.lock().unwrap_or_else(|e| e.into_inner()));
        let mut plans = plan_targets(&inventory, &recipes, &unique_names, standalone.unwrap_or(false));
        let catalogue = catalogue_index(&state.wfcd_items.lock().unwrap_or_else(|e| e.into_inner()));
        let jobs = state.current_crafting.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let building = building_results(&jobs, &recipes);
        for plan in &mut plans {
            plan.decorate(&catalogue, |path| building.contains(path));
        }
        plans
    }).await.map_err(|e| e.to_string())
}

/// What a requirement line shows of its catalogue entry.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub(crate) struct Art {
    pub(crate) image_name: Option<String>,
    pub(crate) category: String,
}

pub(crate) fn catalogue_index(items: &[WfcdItem]) -> HashMap<String, Art> {
    items.iter().map(|i| (i.unique_name.clone(), Art { image_name: i.image_name.clone(), category: i.category.clone() })).collect()
}

/// A Foundry job carries its blueprint path, while the plan lists the result.
pub(crate) fn building_results<'a>(jobs: &'a [CraftingJob], recipes: &'a HashMap<String, Vec<RecipeComponent>>) -> HashSet<&'a str> {
    let results = blueprint_results(recipes);
    jobs.iter().map(|job| results.get(job.unique_name.as_str()).copied().unwrap_or(&job.unique_name)).collect()
}

/// Names a recipe ingredient the market sells, and the count the whole
/// recipe takes regardless of stock.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct Part {
    pub(crate) unique_name: String,
    pub(crate) name: String,
    pub(crate) needed: u32,
}

/// Walks the recipe without a ledger and stops at each ingredient the market
/// sells, since a bought part covers its own recipe. A prime blueprint sits
/// one level down, inside the built component the recipe lists.
pub(crate) fn purchasable(target: &str, components: &[RecipeComponent], tradeable: &dyn Fn(&str) -> bool) -> Vec<Part> {
    let mut parts = vec![];
    collect_purchasable(target, components, 1, tradeable, &mut parts);
    parts
}

fn collect_purchasable(target: &str, components: &[RecipeComponent], crafts: u32, tradeable: &dyn Fn(&str) -> bool, parts: &mut Vec<Part>) {
    for (count, component) in merged(components) {
        if component.unique_name == target { continue; }
        let needed = count * crafts;
        if tradeable(&component.unique_name) {
            match parts.iter_mut().find(|p| p.unique_name == component.unique_name) {
                Some(part) => part.needed += needed,
                None => parts.push(Part { unique_name: component.unique_name.clone(), name: component.name.clone(), needed }),
            }
        } else if !component.components.is_empty() {
            collect_purchasable(target, &component.components, needed.div_ceil(component.result_count.max(1)), tradeable, parts);
        }
    }
}

pub(crate) fn is_blueprint(component: &RecipeComponent) -> bool {
    component.components.is_empty() && component.unique_name.ends_with("Blueprint")
}

/// A Foundry job carries the blueprint path. Each recipe lists its own
/// blueprint as the one component with an empty component list; part
/// blueprints like a chassis or barrel list their ingredients.
pub(crate) fn blueprint_results(recipes: &HashMap<String, Vec<RecipeComponent>>) -> HashMap<&str, &str> {
    recipes.iter().flat_map(|(result, components)| {
        components.iter()
            .filter(|c| is_blueprint(c))
            .map(move |c| (c.unique_name.as_str(), result.as_str()))
    }).collect()
}

/// Akbolto lists Bolto twice rather than once with count 2.
fn merged(components: &[RecipeComponent]) -> Vec<(u32, &RecipeComponent)> {
    let mut out: Vec<(u32, &RecipeComponent)> = vec![];
    for component in components {
        match out.iter_mut().find(|(_, seen)| seen.unique_name == component.unique_name) {
            Some((count, _)) => *count += component.count,
            None => out.push((component.count, component)),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inventory_state::{CachedItem, InventoryStateCache};
    use std::sync::LazyLock;

    const FROST: &str = "/Lotus/Powersuits/Frost/Frost";
    const FROST_BP: &str = "/Lotus/Types/Recipes/WarframeRecipes/FrostBlueprint";
    const CHASSIS: &str = "/Lotus/Types/Recipes/WarframeRecipes/FrostChassisComponent";
    const CHASSIS_BP: &str = "/Lotus/Types/Recipes/WarframeRecipes/FrostChassisBlueprint";
    const FERRITE: &str = "/Lotus/Types/Items/MiscItems/Ferrite";
    const CELL: &str = "/Lotus/Types/Items/MiscItems/OrokinCell";
    const CELL_BP: &str = "/Lotus/Types/Recipes/Components/OrokinCellResourceBlueprint";
    const ALLOY: &str = "/Lotus/Types/Items/MiscItems/AlloyPlate";
    const AKBOLTO: &str = "/Lotus/Weapons/Tenno/Akimbo/AkimboBolto";
    const AKBOLTO_BP: &str = "/Lotus/Types/Recipes/Weapons/AkBoltoBlueprint";
    const BOLTO: &str = "/Lotus/Weapons/Tenno/Pistol/CrossBow";
    const BOLTO_BP: &str = "/Lotus/Types/Recipes/Weapons/BoltoBlueprint";
    const LATO: &str = "/Lotus/Weapons/Tenno/Pistol/Pistol";
    const BALLA: &str = "/Lotus/Weapons/Ostron/Melee/ModularMelee01/Tip/TipOne";
    const BALLA_BP: &str = "/Lotus/Weapons/Ostron/Melee/ModularMelee01/Tip/TipOneBlueprint";
    const IRADITE: &str = "/Lotus/Types/Items/Gameplay/Eidolon/Resources/IraditeItem";
    const PAIR: &str = "/Lotus/Types/Items/MiscItems/Pair";
    const PAIR_BP: &str = "/Lotus/Types/Recipes/Components/PairBlueprint";
    const TARGET: &str = "/Lotus/Types/Target";
    const TARGET_BP: &str = "/Lotus/Types/Recipes/TargetBlueprint";
    const KUVA: &str = "/Lotus/Weapons/Grineer/KuvaLich/LongGuns/Karak/KuvaKarak";
    const FORMA_BP: &str = "/Lotus/Types/Recipes/Components/FormaBlueprint";

    static NO_EQUIPMENT: LazyLock<HashMap<String, i64>> = LazyLock::new(HashMap::new);

    fn leaf(path: &str, count: u32) -> RecipeComponent {
        RecipeComponent {
            unique_name: path.into(), name: path.rsplit('/').next().expect("path has a segment").into(),
            count, result_count: 1, components: vec![], credits: None, reusable: false,
        }
    }

    fn blueprint(path: &str, credits: Option<u32>, reusable: bool) -> RecipeComponent {
        RecipeComponent { credits, reusable, ..leaf(path, 1) }
    }

    fn part(path: &str, count: u32, result_count: u32, components: Vec<RecipeComponent>) -> RecipeComponent {
        RecipeComponent { result_count, components, ..leaf(path, count) }
    }

    fn cell(count: u32) -> RecipeComponent {
        part(CELL, count, 1, vec![blueprint(CELL_BP, Some(0), true), leaf(ALLOY, 50_000)])
    }

    fn frost() -> Vec<RecipeComponent> {
        vec![
            blueprint(FROST_BP, Some(25_000), false),
            part(CHASSIS, 1, 1, vec![blueprint(CHASSIS_BP, Some(15_000), false), leaf(FERRITE, 1_000)]),
            cell(1),
        ]
    }

    fn akbolto() -> Vec<RecipeComponent> {
        let bolto = || part(BOLTO, 1, 1, vec![blueprint(BOLTO_BP, Some(20_000), false), leaf(LATO, 1), cell(2)]);
        vec![blueprint(AKBOLTO_BP, Some(20_000), false), bolto(), bolto(), cell(1)]
    }

    fn stock_of(entries: &[(&str, i64)]) -> HashMap<String, i64> {
        entries.iter().map(|(path, n)| ((*path).to_string(), *n)).collect()
    }

    fn ledger<'a>(stock: &'a HashMap<String, i64>, equipment: &'a HashMap<String, i64>) -> Ledger<'a> {
        let (ledger, rejected) = Ledger::new(stock, equipment, &HashSet::new(), &HashMap::new());
        assert!(rejected.is_empty());
        ledger
    }

    fn short(plan: &CraftPlan) -> Vec<(&str, u32)> {
        plan.shortages().map(|r| (r.unique_name.as_str(), r.short)).collect()
    }

    fn builds(plan: &CraftPlan) -> Vec<(&str, u32)> {
        plan.builds.iter().map(|b| (b.unique_name.as_str(), b.crafts)).collect()
    }

    fn line<'p>(plan: &'p CraftPlan, path: &str) -> &'p Requirement {
        plan.requirements.iter().find(|r| r.unique_name == path).unwrap_or_else(|| panic!("{path} listed"))
    }

    /// Forma lists its blueprint after its resources, and Akbolto's part
    /// blueprints sit inside the built Boltos, so neither the first component
    /// nor every `Blueprint` path names the result.
    #[test]
    fn a_foundry_job_resolves_to_the_recipe_whose_own_blueprint_it_names() {
        let forma = vec![cell(1), leaf(FERRITE, 100), blueprint(FORMA_BP, Some(35_000), false)];
        let recipes: HashMap<String, Vec<RecipeComponent>> = [(FORMA.to_string(), forma), (AKBOLTO.to_string(), akbolto())].into();
        let results = blueprint_results(&recipes);
        assert_eq!(results.get(FORMA_BP), Some(&FORMA));
        assert_eq!(results.get(AKBOLTO_BP), Some(&AKBOLTO));
        assert_eq!(results.get(BOLTO_BP), None);
        assert_eq!(results.get(CELL_BP), None);
    }

    /// Bolto's blueprint sits inside the built Bolto, which is not sold, so
    /// the walk descends, and both Boltos merge into one line. Stock plays no
    /// part, since the count is what the whole recipe takes.
    #[test]
    fn purchasable_parts_stop_at_what_the_market_sells_and_merge_duplicates() {
        let sold: HashSet<&str> = [AKBOLTO_BP, BOLTO_BP, CELL].into();
        let parts = purchasable(AKBOLTO, &akbolto(), &|path| sold.contains(path));
        assert_eq!(parts.iter().map(|p| (p.unique_name.as_str(), p.needed)).collect::<Vec<_>>(),
            [(AKBOLTO_BP, 1), (BOLTO_BP, 2), (CELL, 5)]);
        assert!(purchasable(AKBOLTO, &akbolto(), &|_| false).is_empty());
    }

    /// Levelling has no blueprint of its own. The plan still prices the
    /// Forma it builds, where a recipe without a blueprint would price as
    /// unknown.
    #[test]
    fn forma_for_a_level_cap_draws_from_stock_and_builds_the_rest() {
        let forma = vec![cell(1), leaf(FERRITE, 100), blueprint(FORMA_BP, Some(35_000), false)];
        let recipes: HashMap<String, Vec<RecipeComponent>> = [(FORMA.to_string(), forma)].into();
        let five = forma_ingredient(5, &recipes);
        let stock = stock_of(&[(FORMA, 3), (FORMA_BP, 1), (CELL, 5), (FERRITE, 1_000), (CREDITS_PATH, 100_000)]);
        let plan = ledger(&stock, &NO_EQUIPMENT).plan_ingredients(KUVA, &five);
        assert_eq!((line(&plan, FORMA).needed, line(&plan, FORMA).from_stock, line(&plan, FORMA).short), (5, 3, 0));
        assert_eq!(builds(&plan), [(FORMA, 2)]);
        assert_eq!(short(&plan), [(FORMA_BP, 1)]);
        assert_eq!((plan.credits, plan.credits_short), (Some(70_000), 0));

        let stock = stock_of(&[(FORMA, 5)]);
        let plan = ledger(&stock, &NO_EQUIPMENT).plan_ingredients(KUVA, &five);
        assert!(plan.craftable_now());
        assert_eq!(plan.credits, Some(0));

        let five = forma_ingredient(5, &HashMap::new());
        let plan = ledger(&stock_of(&[]), &NO_EQUIPMENT).plan_ingredients(KUVA, &five);
        assert_eq!(short(&plan), [(FORMA, 5)]);
    }

    #[test]
    fn an_owned_intermediate_is_taken_as_is_and_only_the_missing_remainder_expands() {
        let recipe = frost();
        let stock = stock_of(&[(FROST_BP, 1), (CHASSIS, 1), (CELL, 1), (CREDITS_PATH, 100_000)]);
        let plan = ledger(&stock, &NO_EQUIPMENT).plan(FROST, &recipe);
        assert!(plan.craftable_now());
        assert_eq!((plan.credits, plan.credits_short), (Some(25_000), 0));
        assert!(!plan.requirements.iter().any(|r| r.unique_name == FERRITE));

        let recipe = akbolto();
        let stock = stock_of(&[(AKBOLTO_BP, 1), (BOLTO_BP, 1), (CELL, 3), (CREDITS_PATH, 100_000)]);
        let plan = ledger(&stock, &NO_EQUIPMENT).plan(AKBOLTO, &recipe);
        assert_eq!(builds(&plan), [(BOLTO, 2)]);
        assert_eq!(short(&plan), [(BOLTO_BP, 1), (LATO, 2), (CELL, 2)]);
        assert_eq!(plan.credits, Some(60_000));
        assert!(!plan.craftable_now() && !plan.buildable());

        let stock = stock_of(&[(AKBOLTO_BP, 1), (BOLTO_BP, 1), (LATO, 1), (CELL, 3), (CREDITS_PATH, 100_000)]);
        let plan = ledger(&stock, &NO_EQUIPMENT).plan(AKBOLTO, &recipe);
        assert_eq!(short(&plan), [(BOLTO_BP, 1), (LATO, 1), (CELL, 2)]);
        let bolto = line(&plan, BOLTO);
        assert_eq!((bolto.needed, bolto.from_stock, bolto.short), (2, 0, 0));

        let stock = stock_of(&[(AKBOLTO_BP, 1), (BOLTO, 1), (BOLTO_BP, 1), (LATO, 1), (CELL, 3), (CREDITS_PATH, 100_000)]);
        let plan = ledger(&stock, &NO_EQUIPMENT).plan(AKBOLTO, &recipe);
        assert_eq!(builds(&plan), [(BOLTO, 1)]);
        assert!(plan.buildable());
        assert_eq!(plan.credits, Some(40_000));
        let bolto = line(&plan, BOLTO);
        assert_eq!((bolto.needed, bolto.from_stock, bolto.short), (2, 1, 0));
    }

    #[test]
    fn a_shared_ingredient_goes_to_the_first_target_and_leaves_the_second_short() {
        let (frost, akbolto) = (frost(), akbolto());
        let stock = stock_of(&[(FROST_BP, 1), (CHASSIS, 1), (AKBOLTO_BP, 1), (BOLTO, 2), (CELL, 1), (CREDITS_PATH, 30_000)]);
        let mut ledger = ledger(&stock, &NO_EQUIPMENT);
        let first = ledger.plan(FROST, &frost);
        let second = ledger.plan(AKBOLTO, &akbolto);
        assert!(first.craftable_now());
        assert_eq!(short(&second), [(CELL, 1)]);
        assert_eq!((second.credits, second.credits_short), (Some(20_000), 15_000));
    }

    #[test]
    fn a_batch_yield_leaves_its_surplus_for_the_next_target() {
        let recipe = |n| vec![blueprint(TARGET_BP, Some(0), false),
            part(PAIR, n, 2, vec![blueprint(PAIR_BP, Some(0), false), leaf(FERRITE, 10)])];
        let (three, one) = (recipe(3), recipe(1));
        let stock = stock_of(&[(TARGET_BP, 2), (PAIR_BP, 2), (FERRITE, 20)]);
        let mut ledger = ledger(&stock, &NO_EQUIPMENT);
        let first = ledger.plan(TARGET, &three);
        assert_eq!(builds(&first), [(PAIR, 2)]);
        assert!(first.buildable());
        let second = ledger.plan(TARGET, &one);
        assert!(second.craftable_now());
        assert_eq!((line(&second, PAIR).from_stock, line(&second, PAIR).short), (1, 0));
    }

    #[test]
    fn a_reusable_blueprint_is_needed_once_and_never_consumed() {
        let strike = || vec![blueprint(BALLA_BP, Some(1_000), true), leaf(IRADITE, 20)];
        let (first, second) = (strike(), strike());
        let stock = stock_of(&[(BALLA_BP, 1), (IRADITE, 40), (CREDITS_PATH, 5_000)]);
        let mut shared = ledger(&stock, &NO_EQUIPMENT);
        assert!(shared.plan(BALLA, &first).craftable_now());
        let plan = shared.plan(BALLA, &second);
        assert!(plan.craftable_now());
        assert_eq!(plan.requirements[0], Requirement { unique_name: BALLA_BP.into(), name: "TipOneBlueprint".into(), image_name: None, category: None, needed: 1, owned: 1, from_stock: 1, short: 0, state: IngredientState::Owned, reusable: true, part_blueprint: false });

        let stock = stock_of(&[(IRADITE, 40)]);
        let plan = ledger(&stock, &NO_EQUIPMENT).plan(BALLA, &first);
        assert_eq!(short(&plan), [(BALLA_BP, 1)]);
    }

    #[test]
    fn a_resource_behind_a_reusable_blueprint_the_player_lacks_stays_a_shortage() {
        let recipe = frost();
        let stock = stock_of(&[(FROST_BP, 1), (CHASSIS, 1), (ALLOY, 1_000_000)]);
        let plan = ledger(&stock, &NO_EQUIPMENT).plan(FROST, &recipe);
        assert_eq!(short(&plan), [(CELL, 1)]);
        assert!(builds(&plan).is_empty());

        let stock = stock_of(&[(FROST_BP, 1), (CHASSIS, 1), (ALLOY, 1_000_000), (CELL_BP, 1)]);
        let plan = ledger(&stock, &NO_EQUIPMENT).plan(FROST, &recipe);
        assert_eq!(builds(&plan), [(CELL, 1)]);
        assert!(plan.buildable());
    }

    #[test]
    fn owned_equipment_is_never_an_ingredient_and_only_a_mastered_allowance_admits_it() {
        let recipe = akbolto();
        let stock = stock_of(&[(AKBOLTO_BP, 1), (BOLTO_BP, 2), (LATO, 5), (CELL, 5), (CREDITS_PATH, 100_000)]);
        let equipment = stock_of(&[(BOLTO, 2), (LATO, 1)]);
        let plan = ledger(&stock, &equipment).plan(AKBOLTO, &recipe);
        assert_eq!(builds(&plan), [(BOLTO, 2)]);
        assert_eq!(short(&plan), [(LATO, 2)]);

        let mastered: HashSet<&str> = [BOLTO].into();
        let allowances: HashMap<String, u32> = [(BOLTO.to_string(), 5), (LATO.to_string(), 1), (AKBOLTO.to_string(), 1)].into();
        let (mut ledger, rejected) = Ledger::new(&stock, &equipment, &mastered, &allowances);
        assert_eq!(rejected, [AKBOLTO, LATO]);
        let plan = ledger.plan(AKBOLTO, &recipe);
        assert!(plan.craftable_now());
        assert_eq!((line(&plan, BOLTO).needed, line(&plan, BOLTO).from_stock), (2, 2));
    }

    #[test]
    fn tracked_targets_draw_on_one_ledger_in_order_and_spare_owned_equipment() {
        let recipes: HashMap<String, Vec<RecipeComponent>> = [(FROST.to_string(), frost()), (AKBOLTO.to_string(), akbolto())].into();
        let mut inventory = InventoryStateCache::default();
        for (path, amount, category) in [
            (FROST_BP, 1, "Blueprints"), (CHASSIS, 1, "Parts"), (AKBOLTO_BP, 1, "Blueprints"),
            (BOLTO_BP, 1, "Blueprints"), (BOLTO, 1, "Secondary"), (LATO, 1, "Secondary"), (CELL, 1, "Resources"),
        ] {
            inventory.items.insert(path.into(), CachedItem { unique_name: path.into(), amount, category: category.into(), ..Default::default() });
        }

        let [frost, akbolto] = <[CraftPlan; 2]>::try_from(plan_targets(&inventory, &recipes, &[FROST.into(), AKBOLTO.into()], false)).expect("one plan per target");
        assert!(frost.craftable_now());
        assert!(!frost.requirements.iter().any(|r| r.unique_name == FERRITE));
        // The owned Bolto and Lato are equipment, so neither is an ingredient.
        assert_eq!(builds(&akbolto), [(BOLTO, 2)]);
        assert_eq!(short(&akbolto), [(BOLTO_BP, 1), (LATO, 2), (CELL, 5)]);

        let [akbolto, frost] = <[CraftPlan; 2]>::try_from(plan_targets(&inventory, &recipes, &[AKBOLTO.into(), FROST.into()], false)).expect("one plan per target");
        assert_eq!(short(&akbolto), [(BOLTO_BP, 1), (LATO, 2), (CELL, 4)]);
        assert_eq!(short(&frost), [(CELL, 1)]);

        assert!(plan_targets(&inventory, &recipes, &["/Lotus/Types/Unknown".into()], false)[0].requirements.is_empty());
    }

    #[test]
    fn standalone_targets_each_see_the_whole_stock() {
        let recipes: HashMap<String, Vec<RecipeComponent>> = [(FROST.to_string(), frost()), (TARGET.to_string(), vec![blueprint(TARGET_BP, None, false), cell(1)])].into();
        let mut inventory = InventoryStateCache::default();
        for (path, amount, category) in [(FROST_BP, 1, "Blueprints"), (CHASSIS, 1, "Parts"), (TARGET_BP, 1, "Blueprints"), (CELL, 1, "Resources")] {
            inventory.items.insert(path.into(), CachedItem { unique_name: path.into(), amount, category: category.into(), ..Default::default() });
        }
        let targets = [FROST.to_string(), TARGET.to_string()];
        let shared = plan_targets(&inventory, &recipes, &targets, false);
        assert_eq!(short(&shared[1]), [(CELL, 1)]);
        let alone = plan_targets(&inventory, &recipes, &targets, true);
        assert!(alone.iter().all(CraftPlan::craftable_now));
    }

    #[test]
    fn credits_stay_unknown_without_a_price_and_short_only_against_an_observed_balance() {
        let recipe = vec![blueprint(FROST_BP, None, false), leaf(FERRITE, 1)];
        let stock = stock_of(&[(FROST_BP, 1), (FERRITE, 1), (CREDITS_PATH, 10)]);
        let plan = ledger(&stock, &NO_EQUIPMENT).plan(FROST, &recipe);
        assert_eq!((plan.credits, plan.credits_short), (None, 0));
        assert!(plan.craftable_now());

        let recipe = frost();
        let stock = stock_of(&[(FROST_BP, 1), (CHASSIS_BP, 1), (FERRITE, 1_000), (CELL, 1)]);
        let plan = ledger(&stock, &NO_EQUIPMENT).plan(FROST, &recipe);
        assert_eq!((plan.credits, plan.credits_short), (Some(40_000), 0));
        assert!(plan.buildable());

        let stock = stock_of(&[(FROST_BP, 1), (CHASSIS_BP, 1), (FERRITE, 1_000), (CELL, 1), (CREDITS_PATH, 10_000)]);
        let plan = ledger(&stock, &NO_EQUIPMENT).plan(FROST, &recipe);
        assert_eq!(plan.credits_short, 30_000);
    }

    fn states(plan: &CraftPlan) -> Vec<(&str, IngredientState)> {
        plan.requirements.iter().map(|r| (r.unique_name.as_str(), r.state)).collect()
    }

    /// Frost's chassis is built from a blueprint in hand while its cell waits
    /// on a reusable blueprint the player lacks, so the two intermediates
    /// read differently although both are short of stock.
    #[test]
    fn each_line_carries_the_state_the_ledger_decided() {
        use IngredientState::*;
        let recipe = frost();
        let stock = stock_of(&[(FROST_BP, 1), (CHASSIS_BP, 1), (FERRITE, 400)]);
        let plan = ledger(&stock, &NO_EQUIPMENT).plan(FROST, &recipe);
        assert_eq!(states(&plan), [(FROST_BP, Owned), (CHASSIS, Blocked), (CHASSIS_BP, Owned), (FERRITE, Partial), (CELL, BlueprintMissing)]);
        assert!(!line(&plan, FROST_BP).part_blueprint && line(&plan, CHASSIS_BP).part_blueprint);

        // A consumable part blueprint nobody has is the part's first shortage, whatever else is missing below it.
        let stock = stock_of(&[(FROST_BP, 1), (FERRITE, 400)]);
        let plan = ledger(&stock, &NO_EQUIPMENT).plan(FROST, &recipe);
        assert_eq!(states(&plan), [(FROST_BP, Owned), (CHASSIS, BlueprintMissing), (CHASSIS_BP, Missing), (FERRITE, Partial), (CELL, BlueprintMissing)]);
        assert_eq!(short(&plan), [(CHASSIS_BP, 1), (FERRITE, 600), (CELL, 1)]);

        let stock = stock_of(&[(FROST_BP, 1), (CHASSIS_BP, 1), (FERRITE, 1_000), (CELL_BP, 1), (ALLOY, 50_000)]);
        let plan = ledger(&stock, &NO_EQUIPMENT).plan(FROST, &recipe);
        assert_eq!(states(&plan), [(FROST_BP, Owned), (CHASSIS, Buildable), (CHASSIS_BP, Owned), (FERRITE, Owned), (CELL, Buildable), (CELL_BP, Owned), (ALLOY, Owned)]);

        // One of two Boltos in stock reads partial even though the other gets built.
        let stock = stock_of(&[(AKBOLTO_BP, 1), (BOLTO, 1), (BOLTO_BP, 1), (LATO, 1), (CELL, 5)]);
        let plan = ledger(&stock, &NO_EQUIPMENT).plan(AKBOLTO, &akbolto());
        assert_eq!(line(&plan, BOLTO).state, Partial);
        assert_eq!(builds(&plan), [(BOLTO, 1)]);
    }

    #[test]
    fn an_owned_unmastered_ingredient_reads_master_first_and_a_mastered_one_missing() {
        let recipe = akbolto();
        let stock = stock_of(&[(AKBOLTO_BP, 1), (CELL, 5)]);
        let equipment = stock_of(&[(BOLTO, 1), (LATO, 1)]);
        let mastered: HashSet<&str> = [LATO].into();
        let (mut ledger, _) = Ledger::new(&stock, &equipment, &mastered, &HashMap::new());
        let plan = ledger.plan(AKBOLTO, &recipe);
        assert_eq!(line(&plan, BOLTO).state, IngredientState::MasterFirst);
        assert_eq!(line(&plan, LATO).state, IngredientState::Missing);
    }

    /// Akbolto lists its cell twice, once under the built Boltos and once on
    /// its own. The second listing reads what the first left, so the line
    /// keeps the first reading of the projected stock.
    #[test]
    fn owned_is_the_whole_stock_before_the_recipe_took_its_share() {
        let counts = |plan: &CraftPlan, path| { let l = line(plan, path); (l.owned, l.from_stock, l.needed, l.reusable) };
        let stock = stock_of(&[(FROST_BP, 1), (CHASSIS_BP, 3), (FERRITE, 5_000), (CELL_BP, 1), (CREDITS_PATH, 100_000)]);
        let plan = ledger(&stock, &NO_EQUIPMENT).plan(FROST, &frost());
        assert_eq!(counts(&plan, FERRITE), (5_000, 1_000, 1_000, false));
        assert_eq!(counts(&plan, CHASSIS_BP), (3, 1, 1, false));
        assert_eq!(counts(&plan, CELL_BP), (1, 1, 1, true));
        assert_eq!(counts(&plan, ALLOY), (0, 0, 50_000, false));
        assert_eq!(plan.credit_balance, Some(100_000));

        let stock = stock_of(&[(AKBOLTO_BP, 1), (BOLTO_BP, 2), (LATO, 2), (CELL, 6)]);
        let plan = ledger(&stock, &NO_EQUIPMENT).plan(AKBOLTO, &akbolto());
        assert_eq!(counts(&plan, CELL), (6, 5, 5, false));
        assert_eq!(plan.credit_balance, None);
    }

    #[test]
    fn decorating_marks_foundry_jobs_and_looks_up_the_catalogue() {
        let recipe = frost();
        let stock = stock_of(&[(FROST_BP, 1)]);
        let mut plan = ledger(&stock, &NO_EQUIPMENT).plan(FROST, &recipe);
        let catalogue = [(CHASSIS.to_string(), Art { image_name: Some("chassis.png".into()), category: "Warframes".into() })].into();
        plan.decorate(&catalogue, |path| path == CELL);
        let facts = |path| { let l = line(&plan, path); (l.image_name.as_deref(), l.category.as_deref(), l.state) };
        assert_eq!(facts(CHASSIS), (Some("chassis.png"), Some("Warframes"), IngredientState::BlueprintMissing));
        assert_eq!(facts(CELL), (None, None, IngredientState::Building));
        // Stock already covers the blueprint, so a job for it changes nothing.
        plan.decorate(&HashMap::new(), |path| path == FROST_BP);
        assert_eq!(line(&plan, FROST_BP).state, IngredientState::Owned);
    }
}
