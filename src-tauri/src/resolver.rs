//! Item resolution keyed on `unique_name`.
//!
//! `unique_name` is the canonical identity. A display name is a localised
//! label; a slug is a foreign key. The resolver derives the slug from the
//! item's canonical display name, never from an observed label.

use crate::wfcd::WfcdItem;
use crate::wfm::to_wfm_slug;
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedItem {
    pub unique_name: String,
    pub display_name: String,
    pub slug: String,
}

/// Built per call, not stored in `AppState`: the index is cheap to rebuild.
pub struct ItemResolver {
    by_unique: HashMap<String, ResolvedItem>,
    /// Lower-cased display name → `unique_name`.
    by_display: HashMap<String, String>,
}

impl ItemResolver {
    pub fn from_items(items: &[WfcdItem]) -> Self {
        let mut by_unique = HashMap::with_capacity(items.len());
        let mut by_display = HashMap::with_capacity(items.len());

        for item in items {
            // First writer wins: the catalog can list one display name twice,
            // and keeping the first keeps resolution stable.
            by_display
                .entry(item.name.to_lowercase())
                .or_insert_with(|| item.unique_name.clone());

            by_unique.insert(
                item.unique_name.clone(),
                ResolvedItem {
                    unique_name: item.unique_name.clone(),
                    display_name: item.name.clone(),
                    slug: to_wfm_slug(&item.name),
                },
            );
        }

        Self { by_unique, by_display }
    }

    pub fn by_unique(&self, unique_name: &str) -> Option<&ResolvedItem> {
        self.by_unique.get(unique_name)
    }

    /// Case-insensitive: observed labels vary in case.
    pub fn by_display(&self, display_name: &str) -> Option<&ResolvedItem> {
        let unique = self.by_display.get(&display_name.to_lowercase())?;
        self.by_unique.get(unique)
    }
}

/// A slug and its blueprint-suffix sibling. warframe.market lists a prime
/// component blueprint under the suffixless slug (`nautilus_prime_systems`)
/// while the catalog names it "... Blueprint", so a price can arrive under
/// either spelling.
pub fn slug_variants(slug: &str) -> [String; 2] {
    let sibling = match slug.strip_suffix("_blueprint") {
        Some(base) => base.to_string(),
        None => format!("{}_blueprint", slug),
    };
    [slug.to_string(), sibling]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Vec<WfcdItem> {
        let mk = |name: &str, unique: &str, category: &str| WfcdItem {
            name: name.to_string(),
            unique_name: unique.to_string(),
            category: category.to_string(),
            item_type: String::new(),
            product_category: String::new(),
            image_name: None,
            vaulted: None,
            ducats: None,
            mastery_req: None,
            omega_attenuation: None,
            fusion_limit: None,
            max_level_cap: None,
        };
        vec![
            mk(
                "Ash Prime Neuroptics Blueprint",
                "/Lotus/Types/Recipes/Warframes/AshPrimeHelmetBlueprint",
                "Blueprints",
            ),
            mk("Serration", "/Lotus/Upgrades/Mods/Rifle/WeaponDamageAmountMod", "Mods"),
            mk("Arcane Energize", "/Lotus/Types/Game/ZarimanPistolArcane", "Arcanes"),
        ]
    }

    #[test]
    fn slug_derives_from_canonical_display_name() {
        let r = ItemResolver::from_items(&fixture());
        // Must match the display-name slugify, so existing price lookups keep
        // hitting the same market endpoint.
        for item in fixture() {
            let resolved = r.by_unique(&item.unique_name).expect("fixture item is indexed");
            assert_eq!(resolved.slug, to_wfm_slug(&item.name));
        }
    }

    #[test]
    fn known_items_keep_their_slugs() {
        let r = ItemResolver::from_items(&fixture());
        assert_eq!(
            r.by_display("Ash Prime Neuroptics Blueprint").unwrap().slug,
            "ash_prime_neuroptics_blueprint"
        );
        assert_eq!(r.by_display("Serration").unwrap().slug, "serration");
        assert_eq!(r.by_display("Arcane Energize").unwrap().slug, "arcane_energize");
    }

    #[test]
    fn display_lookup_is_case_insensitive_and_canonicalises() {
        let r = ItemResolver::from_items(&fixture());
        let resolved = r.by_display("serration").expect("case-insensitive match");
        assert_eq!(resolved.unique_name, "/Lotus/Upgrades/Mods/Rifle/WeaponDamageAmountMod");
        assert_eq!(resolved.display_name, "Serration");
    }

    #[test]
    fn unknown_display_name_does_not_resolve() {
        let r = ItemResolver::from_items(&fixture());
        assert!(r.by_display("Definitely Not An Item").is_none());
    }

    #[test]
    fn slug_variants_adds_and_strips_blueprint_suffix() {
        assert_eq!(
            slug_variants("nautilus_prime_systems_blueprint"),
            ["nautilus_prime_systems_blueprint", "nautilus_prime_systems"]
        );
        assert_eq!(slug_variants("serration"), ["serration", "serration_blueprint"]);
    }
}
