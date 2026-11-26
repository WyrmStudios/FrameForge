//! The game's mastery rules, kept apart from the catalogue: WFCD says which
//! equipment exists and carries `maxLevelCap` where it knows one, but it ships
//! no cap for Necramechs and marks Amp Prisms, Infested Kitgun chambers and
//! Venari non-masterable although the game's XPInfo credits them. Every consumer
//! derives rank, cap, and masterability through here rather than from the
//! catalogue fields directly.
//!
//! Precedence: a corrections-table entry for the path, then the path rules
//! below, then the catalogue. The table carries per-source facts; the rules
//! carry whole classes the catalogue gets wrong, so a new prism or Necramech
//! needs no table row.

use std::collections::HashMap;
use crate::app_state::CorrectionEntry;

pub(crate) const DEFAULT_RANK_CAP: u32 = 30;

/// Mastery Rank 30 is the last rank on the quadratic curve. Every Legendary
/// rank past it costs the same flat amount.
const QUADRATIC_MASTERY_PER_RANK: u64 = 2_500;
const QUADRATIC_MASTERY_RANK_CAP: u64 = 30;
const LEGENDARY_RANK_MASTERY: u64 = 147_500;

/// Total mastery the account needs to hold `rank`.
pub(crate) fn mastery_rank_xp(rank: u32) -> u64 {
    let rank = u64::from(rank);
    let quadratic = rank.min(QUADRATIC_MASTERY_RANK_CAP);
    QUADRATIC_MASTERY_PER_RANK * quadratic * quadratic + (rank - quadratic) * LEGENDARY_RANK_MASTERY
}

pub(crate) fn mastery_rank_from_xp(xp: u64) -> u32 {
    let mut rank = 0;
    while mastery_rank_xp(rank + 1) <= xp { rank += 1; }
    rank
}

pub(crate) const INTRINSIC_RANK_CAP: u32 = 10;
pub(crate) const INTRINSIC_MASTERY_PER_RANK: u32 = 1_500;

/// An Intrinsic system's banked-point pool and the tracks it feeds, named
/// by the game's own `PlayerSkills` fields. The rank costs come from the
/// wiki's Railjack and Drifter Intrinsics pages.
pub(crate) struct IntrinsicSystem {
    pub(crate) name: &'static str,
    /// The `LPP_*` field holds the points still banked. A capture held
    /// `LPP_DRIFTER: 0` with every Drifter track at 10, which rules out
    /// lifetime earnings.
    pub(crate) points: &'static str,
    /// Each track's name and `LPS_*` rank field, in the game's display order.
    pub(crate) tracks: &'static [(&'static str, &'static str)],
    /// The points each rank costs, starting at rank 1.
    pub(crate) rank_costs: [u32; INTRINSIC_RANK_CAP as usize],
}

pub(crate) const INTRINSIC_SYSTEMS: [IntrinsicSystem; 2] = [
    IntrinsicSystem {
        name: "Railjack",
        points: "LPP_SPACE",
        tracks: &[
            ("Tactical", "LPS_TACTICAL"), ("Piloting", "LPS_PILOTING"), ("Gunnery", "LPS_GUNNERY"),
            ("Engineering", "LPS_ENGINEERING"), ("Command", "LPS_COMMAND"),
        ],
        rank_costs: [1, 2, 4, 8, 16, 32, 64, 128, 256, 512],
    },
    IntrinsicSystem {
        name: "Drifter",
        points: "LPP_DRIFTER",
        tracks: &[
            ("Combat", "LPS_DRIFT_COMBAT"), ("Riding", "LPS_DRIFT_RIDING"),
            ("Opportunity", "LPS_DRIFT_OPPORTUNITY"), ("Endurance", "LPS_DRIFT_ENDURANCE"),
        ],
        rank_costs: [20, 25, 30, 45, 65, 90, 125, 160, 205, 255],
    },
];

impl IntrinsicSystem {
    pub(crate) fn rank_cap(&self) -> u32 {
        self.tracks.len() as u32 * INTRINSIC_RANK_CAP
    }

    /// A field the game never wrote is rank 0, and a value outside 0..=10 is
    /// not a rank the game hands out.
    pub(crate) fn track_ranks(&self, skills: &HashMap<String, i64>) -> Vec<u32> {
        self.tracks.iter()
            .map(|(_, field)| skills.get(*field).copied().unwrap_or(0).clamp(0, i64::from(INTRINSIC_RANK_CAP)) as u32)
            .collect()
    }

    pub(crate) fn banked(&self, skills: &HashMap<String, i64>) -> u32 {
        skills.get(self.points).copied().unwrap_or(0).clamp(0, i64::from(u32::MAX)) as u32
    }
}

/// Finds the system whose point pool field is a Spend row's or plan
/// selection's `unique_name`.
pub(crate) fn intrinsic_system(unique_name: &str) -> Option<&'static IntrinsicSystem> {
    INTRINSIC_SYSTEMS.iter().find(|s| s.points == unique_name)
}

/// Why no account can earn a mastery source any more. Settings exclude each
/// class from the progress denominator independently.
#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Unobtainable {
    Founders,
    /// Only rewards no recurring event, Baro Ki'Teer or Nightwave brings
    /// back; the wiki's Exclusive Mastery page lists none today, so the
    /// bundled table has no row of this class.
    RetiredEvent,
    /// Players who cleared a node before its removal keep the credit.
    /// TODO: add removed-node catalogue entries to preserve their mastery credit.
    RemovedNode,
}

impl Unobtainable {
    pub(crate) const ALL: [Unobtainable; 3] = [Self::Founders, Self::RetiredEvent, Self::RemovedNode];
}

/// Warframe-like equipment needs 1000·rank² affinity per rank and awards 200
/// mastery per rank; everything else masterable is a weapon at 500·rank² and
/// 100. Pet weapons share the `/Types/Friendly/Pets/` prefix with the pets
/// themselves, so the pet side is matched on model parts and power suits.
pub(crate) fn is_warframe_like(path: &str) -> bool {
    path.contains("/Powersuits/")
        || path.contains("/SentinelPowersuits/")
        || path.ends_with("PetPowerSuit")
        || path.contains("/MoaPetParts/")
        || path.contains("/ZanukaPetParts/")
        || path.contains("/Vehicles/Hoverboard/")
        || path.ends_with("/RailJack/DefaultHarness")
}

fn affinity_base(path: &str) -> i64 {
    if is_warframe_like(path) { 1000 } else { 500 }
}

pub(crate) fn mastery_per_rank(unique_name: &str) -> u32 {
    if INTRINSIC_SYSTEMS.iter().any(|s| s.tracks.iter().any(|(_, field)| *field == unique_name)) { INTRINSIC_MASTERY_PER_RANK }
    else if is_warframe_like(unique_name) { 200 }
    else { 100 }
}

pub(crate) fn xp_to_rank(xp: i64, path: &str) -> u32 {
    (xp.max(0) as f64 / affinity_base(path) as f64).sqrt().floor() as u32
}

/// Lowest affinity that reaches `rank`. A stored rank round-trips through
/// [`xp_to_rank`], and a rank that was capped at 30 on an item now capped at
/// 40 comes back as a lower bound.
pub(crate) fn rank_to_affinity(rank: u32, path: &str) -> i64 {
    i64::from(rank) * i64::from(rank) * affinity_base(path)
}

pub(crate) fn known_cap(correction: Option<&CorrectionEntry>, catalogue_cap: Option<u32>) -> Option<u32> {
    correction.and_then(|c| c.rank_cap).or(catalogue_cap)
}

pub(crate) fn rank_cap(correction: Option<&CorrectionEntry>, path: &str, catalogue_cap: Option<u32>) -> u32 {
    known_cap(correction, catalogue_cap).unwrap_or(if path.contains("/EntratiMech/") { 40 } else { DEFAULT_RANK_CAP })
}

pub(crate) fn earned_rank(xp: i64, path: &str, known_cap: Option<u32>) -> u32 {
    xp_to_rank(xp, path).min(rank_cap(None, path, known_cap))
}

const LEVELS_PER_FORMA: u32 = 2;

pub(crate) fn level_cap(forma: u32, rank_cap: u32) -> u32 {
    (DEFAULT_RANK_CAP + forma * LEVELS_PER_FORMA).min(rank_cap)
}

pub(crate) fn forma_to_cap(level_cap: u32, rank_cap: u32) -> u32 {
    rank_cap.saturating_sub(level_cap).div_ceil(LEVELS_PER_FORMA)
}

pub(crate) fn masterable(correction: Option<&CorrectionEntry>, wfcd: Option<bool>, path: &str) -> Option<bool> {
    if let Some(masterable) = correction.and_then(|c| c.masterable) { return Some(masterable); }
    if !path.ends_with("Blueprint") {
        // Bare "Barrel": the Mote Prism path has no `/Barrel/` segment.
        if path.contains("/OperatorAmplifiers/") && path.contains("Barrel") {
            return Some(true);
        }
        if path.contains("/InfKitGun/Barrels/") {
            return Some(true);
        }
        // Operator amp weapons (Sirocco, etc.) grant mastery.
        if path.contains("/Operator/Pistols/") {
            return Some(true);
        }
        if path.contains("/Khora/Kavat/") {
            return Some(true);
        }
    }
    wfcd
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOUND_WEAPON: &str = "/Lotus/Types/Friendly/Pets/ZanukaPets/ZanukaPetMeleeWeaponIS";
    const MOA_WEAPON: &str = "/Lotus/Types/Friendly/Pets/MoaPets/MoaPetComponents/HextraWeapon";
    const HOUND_MODEL: &str = "/Lotus/Types/Friendly/Pets/ZanukaPets/ZanukaPetParts/ZanukaPetPartHeadA";
    const MOA_MODEL: &str = "/Lotus/Types/Friendly/Pets/MoaPets/MoaPetParts/MoaPetHeadLambeo";
    const VULPAPHYLA: &str = "/Lotus/Types/Friendly/Pets/CreaturePets/ArmoredInfestedCatbrowPetPowerSuit";
    const KUBROW: &str = "/Lotus/Types/Game/KubrowPet/GuardKubrowPetPowerSuit";
    const SENTINEL: &str = "/Lotus/Types/Sentinels/SentinelPowersuits/CarrierPowerSuit";
    const SENTINEL_WEAPON: &str = "/Lotus/Types/Sentinels/SentinelWeapons/Gremlin";
    const KDRIVE: &str = "/Lotus/Types/Vehicles/Hoverboard/HoverboardParts/PartComponents/HoverboardSolarisA/HoverboardSolarisADeck";
    const PLEXUS: &str = "/Lotus/Types/Game/CrewShip/RailJack/DefaultHarness";
    const SUN_AND_MOON: &str = "/Lotus/Types/Friendly/PlayerControllable/Weapons/DuviriDualSwordsWeapon";
    const VOIDRIG: &str = "/Lotus/Powersuits/EntratiMech/NechroTech";

    /// Hound and MOA weapons at 640,341 XP are rank 30 in the game's profile;
    /// the warframe base would leave them at 25.
    #[test]
    fn pet_weapons_rank_on_the_weapon_base_and_pets_on_the_warframe_base() {
        for weapon in [HOUND_WEAPON, MOA_WEAPON, SENTINEL_WEAPON, SUN_AND_MOON] {
            assert!(!is_warframe_like(weapon), "{weapon}");
            assert_eq!(earned_rank(640_341, weapon, None), 30, "{weapon}");
        }
        for suit in [HOUND_MODEL, MOA_MODEL, VULPAPHYLA, KUBROW, SENTINEL, KDRIVE, PLEXUS, VOIDRIG] {
            assert!(is_warframe_like(suit), "{suit}");
            assert_eq!(xp_to_rank(640_341, suit), 25, "{suit}");
        }
    }

    #[test]
    fn each_forma_lifts_the_level_cap_by_two_up_to_the_rank_cap() {
        assert_eq!([level_cap(0, 40), level_cap(3, 40), level_cap(5, 40), level_cap(9, 40), level_cap(4, 30)], [30, 36, 40, 40, 30]);
        assert_eq!([forma_to_cap(30, 40), forma_to_cap(36, 40), forma_to_cap(40, 40), forma_to_cap(30, 30)], [5, 2, 0, 0]);
    }

    #[test]
    fn rank_40_equipment_caps_at_40() {
        const KUVA_NUKOR: &str = "/Lotus/Weapons/Grineer/KuvaLich/Secondaries/Nukor/KuvaNukor";
        const CODA_POX: &str = "/Lotus/Weapons/Infested/InfestedLich/Pistols/CodaPox";
        const PARACESIS: &str = "/Lotus/Weapons/Orokin/BallasSword/BallasSwordWeapon";
        const TENET_EXEC: &str = "/Lotus/Weapons/Tenno/Melee/Swords/CrpBigSlash/CrpBigSlash";
        for path in [KUVA_NUKOR, CODA_POX, PARACESIS, TENET_EXEC] {
            assert_eq!(rank_cap(None, path, Some(40)), 40, "{path}");
        }
        assert_eq!(rank_cap(None, VOIDRIG, None), 40);
        assert_eq!(rank_cap(None, KUBROW, None), 30);
        // Rank 40 on the weapon base is 800,000 affinity; on the Necramech base 1,600,000.
        assert_eq!(earned_rank(799_999, KUVA_NUKOR, Some(40)), 39);
        assert_eq!(earned_rank(129_043_438, KUVA_NUKOR, Some(40)), 40);
        assert_eq!(earned_rank(3_527_278, VOIDRIG, None), 40);
    }

    /// The affinity thresholds the game shows: 450k for a rank-30 weapon,
    /// 900k for a rank-30 Warframe, 800k for a rank-40 Kuva weapon.
    #[test]
    fn rank_to_affinity_is_the_threshold_xp_to_rank_accepts() {
        let braton = "/Lotus/Weapons/Tenno/Rifle/Braton";
        let kuva = "/Lotus/Weapons/Grineer/KuvaLich/LongGuns/Karak/KuvaKarak";
        let mag = "/Lotus/Powersuits/Mag/Mag";
        assert_eq!(rank_to_affinity(30, braton), 450_000);
        assert_eq!(rank_to_affinity(30, mag), 900_000);
        assert_eq!(rank_to_affinity(40, kuva), 800_000);
        assert_eq!(rank_to_affinity(0, mag), 0);
        assert_eq!(xp_to_rank(rank_to_affinity(30, braton) - 1, braton), 29);
    }

    /// The wiki's thresholds: MR 1 at 2,500, MR 30 at 2,250,000, Legendary 1
    /// at 2,397,500.
    #[test]
    fn mastery_rank_thresholds_are_quadratic_to_30_then_flat() {
        assert_eq!(mastery_rank_xp(0), 0);
        assert_eq!(mastery_rank_xp(1), 2_500);
        assert_eq!(mastery_rank_xp(16), 640_000);
        assert_eq!(mastery_rank_xp(30), 2_250_000);
        assert_eq!(mastery_rank_xp(31), 2_397_500);
        assert_eq!(mastery_rank_xp(35), 2_987_500);
        assert_eq!(mastery_rank_from_xp(0), 0);
        assert_eq!(mastery_rank_from_xp(2_499), 0);
        assert_eq!(mastery_rank_from_xp(2_500), 1);
        assert_eq!(mastery_rank_from_xp(2_249_999), 29);
        assert_eq!(mastery_rank_from_xp(2_397_499), 30);
        assert_eq!(mastery_rank_from_xp(2_397_500), 31);
    }

    #[test]
    fn negative_xp_is_rank_zero() {
        assert_eq!(xp_to_rank(-1, KUBROW), 0);
    }

    #[test]
    fn a_table_entry_beats_the_path_rules_and_the_catalogue() {
        const MOTE_PRISM: &str = "/Lotus/Weapons/Sentients/OperatorAmplifiers/SentTrainingAmplifier/SentAmpTrainingBarrel";
        let entry = CorrectionEntry { masterable: Some(false), rank_cap: Some(40), ..Default::default() };
        assert_eq!(masterable(None, Some(false), MOTE_PRISM), Some(true));
        assert_eq!(masterable(Some(&entry), Some(false), MOTE_PRISM), Some(false));
        assert_eq!(rank_cap(Some(&entry), KUBROW, Some(30)), 40);
        assert_eq!(rank_cap(Some(&CorrectionEntry::default()), KUBROW, None), 30);
    }

    /// XPInfo shows Venari at 900,000 affinity, the rank-30 Warframe
    /// threshold, while WFCD marks it non-masterable.
    #[test]
    fn venari_is_masterable_on_the_warframe_base() {
        const VENARI: &str = "/Lotus/Powersuits/Khora/Kavat/KhoraKavatPowerSuit";
        const VENARI_PRIME: &str = "/Lotus/Powersuits/Khora/Kavat/KhoraPrimeKavatPowerSuit";
        for path in [VENARI, VENARI_PRIME] {
            assert_eq!(masterable(None, Some(false), path), Some(true), "{path}");
            assert_eq!(rank_cap(None, path, None), 30, "{path}");
            assert_eq!(earned_rank(899_999, path, None), 29, "{path}");
            assert_eq!(earned_rank(900_000, path, None), 30, "{path}");
        }
    }
}
