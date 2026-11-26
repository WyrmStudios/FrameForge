//! Chance of pulling every missing relic part out of the relics the player
//! already owns.
//!
//! Each owned relic is rolled once, solo, at the refinement it sits at, and
//! yields exactly one of its rewards. The rolls walk a distribution over how
//! many copies of each required part are in hand, capped at the count needed,
//! so two parts that share a relic compete for the same roll. Relics the
//! player could still acquire or refine are not part of the estimate.

use std::collections::{BTreeSet, HashMap, HashSet};
use crate::mastery_recipe::CraftPlan;
use crate::wfcd::RelicReward;

/// One owned relic, at one refinement, that drops the part.
#[derive(serde::Serialize, Clone, PartialEq, Debug)]
pub(crate) struct RelicStock {
    pub(crate) unique_name: String,
    pub(crate) name: String,
    pub(crate) count: u32,
    /// The reward's share of one roll after the refinement's table is
    /// normalized to one. None when the table carries no chances.
    pub(crate) chance: Option<f64>,
}

#[derive(serde::Serialize, Clone, PartialEq, Debug)]
pub(crate) struct RelicPart {
    pub(crate) unique_name: String,
    pub(crate) name: String,
    pub(crate) short: u32,
    /// Sorted with the best chance first.
    pub(crate) relics: Vec<RelicStock>,
    /// Names every relic whose table lists the part, whether the player owns
    /// it or not, with the refinement stripped and sorted. A part that no
    /// owned relic drops still says where to look.
    pub(crate) dropped_by: Vec<String>,
}

#[derive(serde::Serialize, Clone, PartialEq, Debug)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum Coverage {
    /// Every part has relics enough to roll, and `probability` is the chance
    /// that they all drop.
    Complete { probability: f64 },
    /// `missing` names parts no owned relic drops. `short` names parts the
    /// owned relics cannot yield together, whether one part alone has too
    /// few relics or several parts compete for the same rolls. The route
    /// carries no chance for the whole item.
    Partial { missing: Vec<String>, short: Vec<String> },
    /// A relevant relic's table carries no chances, or the walk would take
    /// more states than allowed.
    Unknown,
}

#[derive(serde::Serialize, Clone, PartialEq, Debug)]
pub(crate) struct RelicRoute {
    pub(crate) parts: Vec<RelicPart>,
    pub(crate) coverage: Coverage,
}

struct OwnedRelic {
    unique_name: String,
    name: String,
    count: u32,
    /// A table that repeats an item has its chances summed. Every value is
    /// None when the table sums to nothing or misses a chance.
    rewards: HashMap<String, Option<f64>>,
}

#[derive(Default)]
pub(crate) struct Relics {
    /// Every item any relic in the catalogue drops, whether the player owns
    /// that relic or not.
    parts: HashSet<String>,
    dropped_by: HashMap<String, BTreeSet<String>>,
    owned: Vec<OwnedRelic>,
}

const REFINEMENTS: [&str; 4] = [" Intact", " Exceptional", " Flawless", " Radiant"];

fn base_name(relic: &str) -> &str {
    REFINEMENTS.iter().find_map(|suffix| relic.strip_suffix(suffix)).unwrap_or(relic)
}

// Bounds the walk, which costs states × relic copies × drops per route. A
// recipe whose part counts multiply past this is not a relic build, so the
// route reads as Unknown instead of stalling.
const STATE_LIMIT: usize = 1 << 12;

impl Relics {
    /// `tables` is keyed by relic `unique_name`, and harmlessly by display
    /// name as well. `names` maps a relic `unique_name` to what the game
    /// calls it.
    pub(crate) fn new(stock: &HashMap<String, i64>, tables: &HashMap<String, Vec<RelicReward>>, names: &HashMap<String, String>) -> Self {
        let parts = tables.values().flatten()
            .filter(|r| !r.unique_name.is_empty())
            .map(|r| r.unique_name.clone())
            .collect();
        let mut dropped_by: HashMap<String, BTreeSet<String>> = HashMap::new();
        for (key, table) in tables {
            // A key without a display name would show as a raw path, so it is
            // left out.
            let Some(name) = names.get(key).map(String::as_str).or_else(|| (!key.starts_with('/')).then_some(key.as_str())) else { continue };
            for reward in table.iter().filter(|r| !r.unique_name.is_empty()) {
                dropped_by.entry(reward.unique_name.clone()).or_default().insert(base_name(name).to_string());
            }
        }
        let mut owned: Vec<OwnedRelic> = stock.iter()
            .filter(|(_, &count)| count > 0)
            .filter_map(|(unique_name, &count)| {
                let table = tables.get(unique_name)?;
                let total = table.iter().map(|r| r.chance).sum::<Option<f64>>().filter(|&total| total > 0.0);
                let mut rewards: HashMap<String, Option<f64>> = HashMap::new();
                for reward in table.iter().filter(|r| !r.unique_name.is_empty()) {
                    let chance = total.and_then(|total| reward.chance.map(|c| c / total));
                    let seen = rewards.entry(reward.unique_name.clone()).or_insert(Some(0.0));
                    *seen = seen.zip(chance).map(|(a, b)| a + b);
                }
                let name = names.get(unique_name).cloned()
                    .unwrap_or_else(|| unique_name.rsplit('/').next().expect("rsplit yields at least one piece").to_string());
                Some(OwnedRelic { unique_name: unique_name.clone(), name, count: u32::try_from(count).unwrap_or(u32::MAX), rewards })
            })
            .collect();
        owned.sort_by(|a, b| a.name.cmp(&b.name));
        Self { parts, dropped_by, owned }
    }

    pub(crate) fn is_part(&self, unique_name: &str) -> bool {
        self.parts.contains(unique_name)
    }

    pub(crate) fn route(&self, plan: &CraftPlan) -> Option<RelicRoute> {
        let parts: Vec<RelicPart> = plan.shortages()
            .filter(|r| self.is_part(&r.unique_name))
            .map(|r| {
                let mut relics: Vec<RelicStock> = self.owned.iter()
                    .filter_map(|o| o.rewards.get(&r.unique_name).map(|&chance| RelicStock { unique_name: o.unique_name.clone(), name: o.name.clone(), count: o.count, chance }))
                    .collect();
                relics.sort_by(|a, b| b.chance.partial_cmp(&a.chance).unwrap_or(std::cmp::Ordering::Equal).then_with(|| a.name.cmp(&b.name)));
                let dropped_by = self.dropped_by.get(&r.unique_name).map(|set| set.iter().cloned().collect()).unwrap_or_default();
                RelicPart { unique_name: r.unique_name.clone(), name: r.name.clone(), short: r.short, relics, dropped_by }
            })
            .collect();
        if parts.is_empty() { return None; }
        let missing: Vec<String> = parts.iter().filter(|p| p.relics.is_empty()).map(|p| p.name.clone()).collect();
        let short: Vec<String> = parts.iter()
            .filter(|p| !p.relics.is_empty() && p.relics.iter().map(|r| r.count).sum::<u32>() < p.short)
            .map(|p| p.name.clone())
            .collect();
        let coverage = if !missing.is_empty() || !short.is_empty() {
            Coverage::Partial { missing, short }
        } else {
            match self.probability(&parts) {
                // Mass only reaches the full state through a possible sequence
                // of rolls, so an exact zero means the parts compete for too
                // few rolls between them.
                Some(0.0) => Coverage::Partial { missing, short: parts.iter().map(|p| p.name.clone()).collect() },
                Some(probability) => Coverage::Complete { probability },
                None => Coverage::Unknown,
            }
        };
        Some(RelicRoute { parts, coverage })
    }

    /// The state is the copies of each part in hand, capped at the count
    /// needed, packed in mixed radix. Every roll of a relic that drops any
    /// part moves probability mass from each state to the states one reward
    /// away, or leaves it where a reward is one nobody needs.
    fn probability(&self, parts: &[RelicPart]) -> Option<f64> {
        let mut strides = Vec::with_capacity(parts.len());
        let mut states = 1usize;
        for part in parts {
            strides.push(states);
            states = states.checked_mul(part.short as usize + 1)?;
            if states > STATE_LIMIT { return None; }
        }
        let mut dist = vec![0.0; states];
        let mut next = vec![0.0; states];
        dist[0] = 1.0;
        for relic in &self.owned {
            let drops: Vec<(usize, f64)> = parts.iter().enumerate()
                .filter_map(|(i, p)| relic.rewards.get(&p.unique_name).map(|chance| chance.map(|c| (i, c))))
                .collect::<Option<_>>()?;
            if drops.is_empty() { continue; }
            let stay = (1.0 - drops.iter().map(|(_, c)| c).sum::<f64>()).max(0.0);
            for _ in 0..relic.count {
                next.fill(0.0);
                for (state, &mass) in dist.iter().enumerate().filter(|(_, &mass)| mass > 0.0) {
                    next[state] += mass * stay;
                    for &(i, chance) in &drops {
                        let have = (state / strides[i]) % (parts[i].short as usize + 1);
                        let to = if have < parts[i].short as usize { state + strides[i] } else { state };
                        next[to] += mass * chance;
                    }
                }
                std::mem::swap(&mut dist, &mut next);
            }
        }
        Some(dist[states - 1])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mastery_recipe::{IngredientState, Requirement};

    const BARREL: &str = "/Lotus/Types/Recipes/Weapons/WeaponParts/AkstilettoPrimeBarrel";
    const RECEIVER: &str = "/Lotus/Types/Recipes/Weapons/WeaponParts/AkstilettoPrimeReceiver";
    const LINK: &str = "/Lotus/Types/Recipes/Weapons/WeaponParts/AkstilettoPrimeLink";
    const FERRITE: &str = "/Lotus/Types/Items/MiscItems/Ferrite";
    /// A relic reward that no recipe needs, so it never counts as a part.
    const FILLER: &str = "/Lotus/Upgrades/Mods/Immortal/ImmortalOneMod";
    const LITH_INTACT: &str = "/Lotus/Types/Game/Projections/T1VoidProjectionGBronze";
    const LITH_RADIANT: &str = "/Lotus/Types/Game/Projections/T1VoidProjectionGPlatinum";
    const MESO_INTACT: &str = "/Lotus/Types/Game/Projections/T2VoidProjectionHBronze";
    const NEO_INTACT: &str = "/Lotus/Types/Game/Projections/T3VoidProjectionKBronze";

    fn reward(unique_name: &str, chance: f64) -> RelicReward {
        RelicReward { unique_name: unique_name.into(), name: short_name(unique_name).into(), rarity: String::new(), image_name: None, chance: Some(chance) }
    }

    fn short_name(unique_name: &str) -> &str {
        unique_name.rsplit('/').next().expect("rsplit yields at least one piece")
    }

    fn tables(entries: &[(&str, &[(&str, f64)])]) -> HashMap<String, Vec<RelicReward>> {
        entries.iter().map(|(path, rewards)| ((*path).to_string(), rewards.iter().map(|&(item, chance)| reward(item, chance)).collect())).collect()
    }

    fn stock(entries: &[(&str, i64)]) -> HashMap<String, i64> {
        entries.iter().map(|&(path, n)| (path.to_string(), n)).collect()
    }

    fn names() -> HashMap<String, String> {
        [(LITH_INTACT, "Lith A1 Intact"), (LITH_RADIANT, "Lith A1 Radiant"), (MESO_INTACT, "Meso H2 Intact")]
            .into_iter().map(|(path, name)| (path.to_string(), name.to_string())).collect()
    }

    fn plan(short: &[(&str, u32)]) -> CraftPlan {
        CraftPlan {
            requirements: short.iter().map(|&(path, short)| Requirement {
                unique_name: path.into(), name: short_name(path).into(), image_name: None, category: None, needed: short, owned: 0, from_stock: 0, short, state: IngredientState::Missing, reusable: false, part_blueprint: false,
            }).collect(),
            ..Default::default()
        }
    }

    /// Walks every sequence of roll outcomes and adds up the ones that end
    /// with every part in hand.
    fn exhaustive(relics: &Relics, needed: &[(&str, u32)]) -> f64 {
        let rolls: Vec<&OwnedRelic> = relics.owned.iter().flat_map(|o| std::iter::repeat_n(o, o.count as usize)).collect();
        fn walk(rolls: &[&OwnedRelic], have: &mut Vec<u32>, needed: &[(&str, u32)], mass: f64) -> f64 {
            let Some((roll, rest)) = rolls.split_first() else {
                return if have.iter().zip(needed).all(|(h, (_, n))| h >= n) { mass } else { 0.0 };
            };
            let mut total = 0.0;
            let mut dropped = 0.0;
            for (item, chance) in &roll.rewards {
                let chance = chance.expect("test tables carry chances");
                dropped += chance;
                match needed.iter().position(|(p, _)| p == item) {
                    Some(i) => { have[i] += 1; total += walk(rest, have, needed, mass * chance); have[i] -= 1; }
                    None => total += walk(rest, have, needed, mass * chance),
                }
            }
            total + walk(rest, have, needed, mass * (1.0 - dropped).max(0.0))
        }
        walk(&rolls, &mut vec![0; needed.len()], needed, 1.0)
    }

    fn probability(relics: &Relics, needed: &[(&str, u32)]) -> f64 {
        match relics.route(&plan(needed)).expect("relic parts short").coverage {
            Coverage::Complete { probability } => probability,
            other => panic!("expected a probability, got {other:?}"),
        }
    }

    #[test]
    fn tiny_cases_match_exhaustive_enumeration() {
        // Barrel and Receiver compete in every Lith roll, while Link only
        // drops from the Meso at a different table.
        let tables = tables(&[
            (LITH_INTACT, &[(BARREL, 25.33), (RECEIVER, 11.0), (FILLER, 63.67)]),
            (LITH_RADIANT, &[(BARREL, 16.67), (RECEIVER, 20.0), (FILLER, 63.33)]),
            (MESO_INTACT, &[(LINK, 2.0), (BARREL, 25.33), (FILLER, 72.67)]),
        ]);
        let cases: &[(&[(&str, i64)], &[(&str, u32)])] = &[
            (&[(LITH_INTACT, 2)], &[(BARREL, 1), (RECEIVER, 1)]),
            (&[(LITH_INTACT, 3)], &[(BARREL, 2)]),
            (&[(LITH_INTACT, 1), (LITH_RADIANT, 2)], &[(BARREL, 1), (RECEIVER, 1)]),
            (&[(LITH_INTACT, 2), (LITH_RADIANT, 1), (MESO_INTACT, 3)], &[(BARREL, 2), (RECEIVER, 2), (LINK, 1)]),
        ];
        for (owned, needed) in cases {
            let relics = Relics::new(&stock(owned), &tables, &names());
            let dp = probability(&relics, needed);
            let brute = exhaustive(&relics, needed);
            assert!((dp - brute).abs() < 1e-12, "{needed:?} from {owned:?}: dp {dp} vs exhaustive {brute}");
        }
        // Two competing parts from two rolls succeed as Barrel then Receiver
        // or the other way round.
        let relics = Relics::new(&stock(&[(LITH_INTACT, 2)]), &tables, &names());
        assert!((probability(&relics, &[(BARREL, 1), (RECEIVER, 1)]) - 2.0 * 0.2533 * 0.11).abs() < 1e-12);
    }

    #[test]
    fn each_refinement_table_normalizes_to_one_before_rolling() {
        // The same table at twice the scale rolls the same. A Requiem-style
        // table whose chances sum to 76 spreads the rest across its rewards.
        let doubled = tables(&[(LITH_INTACT, &[(BARREL, 50.66), (FILLER, 149.34)])]);
        let plain = tables(&[(LITH_INTACT, &[(BARREL, 25.33), (FILLER, 74.67)])]);
        let owned = stock(&[(LITH_INTACT, 2)]);
        let from_doubled = probability(&Relics::new(&owned, &doubled, &names()), &[(BARREL, 1)]);
        let from_plain = probability(&Relics::new(&owned, &plain, &names()), &[(BARREL, 1)]);
        assert!((from_doubled - from_plain).abs() < 1e-12);
        let partial_sum = tables(&[(LITH_INTACT, &[(BARREL, 38.0), (FILLER, 38.0)])]);
        assert!((probability(&Relics::new(&owned, &partial_sum, &names()), &[(BARREL, 1)]) - 0.75).abs() < 1e-12);
    }

    #[test]
    fn zero_stock_missing_sources_and_short_stock_read_as_partial_without_a_percentage() {
        let tables = tables(&[
            (LITH_INTACT, &[(BARREL, 25.33), (FILLER, 74.67)]),
            (MESO_INTACT, &[(LINK, 2.0), (FILLER, 98.0)]),
            (NEO_INTACT, &[(RECEIVER, 11.0), (FILLER, 89.0)]),
        ]);
        let none = Relics::new(&stock(&[]), &tables, &names());
        assert_eq!(none.route(&plan(&[(BARREL, 1), (LINK, 1)])).map(|r| r.coverage),
            Some(Coverage::Partial { missing: vec!["AkstilettoPrimeBarrel".into(), "AkstilettoPrimeLink".into()], short: vec![] }));
        // One Lith cannot yield two Barrels, and the Receiver drops from
        // nothing owned.
        let one = Relics::new(&stock(&[(LITH_INTACT, 1)]), &tables, &names());
        let route = one.route(&plan(&[(BARREL, 2), (RECEIVER, 1), (FERRITE, 500)])).expect("relic parts short");
        assert_eq!(route.coverage, Coverage::Partial { missing: vec!["AkstilettoPrimeReceiver".into()], short: vec!["AkstilettoPrimeBarrel".into()] });
        assert_eq!(route.parts.iter().map(|p| (p.name.as_str(), p.short, p.relics.len())).collect::<Vec<_>>(),
            [("AkstilettoPrimeBarrel", 2, 1), ("AkstilettoPrimeReceiver", 1, 0)]);
        let [lith] = route.parts[0].relics.as_slice() else { panic!("one relic drops the barrel") };
        assert_eq!((lith.name.as_str(), lith.count), ("Lith A1 Intact", 1));
        // The Neo has no display name in the test catalogue, so it stays out. The Lith reads once for both refinements.
        assert_eq!(route.parts.iter().map(|p| p.dropped_by.clone()).collect::<Vec<_>>(), [vec!["Lith A1".to_string()], vec![]]);
        let both = Relics::new(&stock(&[]), &self::tables(&[(LITH_INTACT, &[(BARREL, 25.33)]), (LITH_RADIANT, &[(BARREL, 10.0)]), ("Meso H2 Radiant", &[(BARREL, 2.0)])]), &names());
        assert_eq!(both.route(&plan(&[(BARREL, 1)])).expect("relic parts short").parts[0].dropped_by, ["Lith A1", "Meso H2"]);
        assert!((lith.chance.expect("table carries chances") - 0.2533).abs() < 1e-12);
        assert!(one.route(&plan(&[(FERRITE, 500)])).is_none());
        assert!(one.route(&plan(&[])).is_none());
    }

    #[test]
    fn parts_competing_for_too_few_rolls_read_as_partial_not_zero_percent() {
        let tables = tables(&[(LITH_INTACT, &[(BARREL, 25.33), (RECEIVER, 11.0), (FILLER, 63.67)])]);
        let one = Relics::new(&stock(&[(LITH_INTACT, 1)]), &tables, &names());
        assert_eq!(one.route(&plan(&[(BARREL, 1), (RECEIVER, 1)])).map(|r| r.coverage),
            Some(Coverage::Partial { missing: vec![], short: vec!["AkstilettoPrimeBarrel".into(), "AkstilettoPrimeReceiver".into()] }));
        let two = Relics::new(&stock(&[(LITH_INTACT, 2)]), &tables, &names());
        assert!(probability(&two, &[(BARREL, 1), (RECEIVER, 1)]) > 0.0);
    }

    #[test]
    fn a_relevant_table_without_chances_leaves_the_probability_unknown() {
        let mut tables = tables(&[
            (LITH_INTACT, &[(BARREL, 0.0), (FILLER, 0.0)]),
            (MESO_INTACT, &[(LINK, 2.0), (FILLER, 98.0)]),
            (NEO_INTACT, &[(RECEIVER, 11.0), (FILLER, 89.0)]),
        ]);
        // One reward without a chance spoils its whole table.
        tables.get_mut(NEO_INTACT).expect("listed")[1].chance = None;
        let relics = Relics::new(&stock(&[(LITH_INTACT, 3), (MESO_INTACT, 3), (NEO_INTACT, 3)]), &tables, &names());
        assert_eq!(relics.route(&plan(&[(BARREL, 1)])).map(|r| r.coverage), Some(Coverage::Unknown));
        assert_eq!(relics.route(&plan(&[(RECEIVER, 1)])).map(|r| r.coverage), Some(Coverage::Unknown));
        // A blank table only matters for parts it drops.
        assert!(matches!(relics.route(&plan(&[(LINK, 1)])).map(|r| r.coverage), Some(Coverage::Complete { .. })));
    }

    #[test]
    fn a_table_that_repeats_an_item_rolls_it_once_at_the_summed_chance() {
        let tables = tables(&[(LITH_INTACT, &[(BARREL, 10.0), (BARREL, 15.33), (FILLER, 74.67)])]);
        let relics = Relics::new(&stock(&[(LITH_INTACT, 1)]), &tables, &names());
        assert!((probability(&relics, &[(BARREL, 1)]) - 0.2533).abs() < 1e-12);
    }
}
