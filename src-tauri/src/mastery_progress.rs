//! FrameForge holds one account's progress. A player switch is not tracked,
//! so the next observation overwrites whatever the previous account left.

use std::collections::HashMap;
use std::path::PathBuf;
use tracing::error;
use crate::cache::atomic_write;
use crate::inventory_state::InventoryStateCache;
use crate::mastery_rules::rank_to_affinity;
use crate::memory_scanner::{BlobAffiliation, BlobMission};

#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ProvenanceState { Confirmed, Unconfirmed, #[default] Unknown }

#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(crate) struct Provenance {
    pub(crate) state: ProvenanceState,
    /// Unix seconds; only Confirmed carries one.
    pub(crate) observed_at: Option<i64>,
}

impl Provenance {
    /// What an entry's presence or absence in the observed field means. Only
    /// a Confirmed field vouches for absence. An Unconfirmed record came from
    /// a cache that dropped zero rows, so absence there says nothing.
    pub(crate) fn resolve<T>(self, observed: Option<T>, absent: T) -> Option<T> {
        match (self.state, observed) {
            (ProvenanceState::Unknown, _) | (ProvenanceState::Unconfirmed, None) => None,
            (ProvenanceState::Confirmed, None) => Some(absent),
            (_, Some(value)) => Some(value),
        }
    }
}

/// An empty selection list is a plan the player cleared, kept until they
/// regenerate.
#[derive(serde::Serialize, serde::Deserialize, Clone, PartialEq, Eq, Debug)]
pub(crate) struct MasteryPlan {
    pub(crate) target: u32,
    /// Kept as text so a view the frontend later renames still loads. The
    /// frontend falls back to Suggestions for a name it no longer knows.
    pub(crate) view: String,
    pub(crate) selections: Vec<String>,
    #[serde(default)]
    pub(crate) allowances: HashMap<String, u32>,
    /// Saved targets stay fixed when the player earns more Intrinsic points.
    #[serde(default)]
    pub(crate) intrinsic_targets: HashMap<String, HashMap<String, u32>>,
    #[serde(default)]
    pub(crate) purchase_comparison: String,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, Default)]
pub(crate) struct PlayerProgress {
    /// Affinity per unique_name as `XPInfo` reports it; ranks derive at read
    /// so a later rank-cap change recomputes instead of freezing.
    pub(crate) affinity: HashMap<String, i64>,
    pub(crate) equipment: Provenance,
    /// `PlayerSkills` as the game reports it, with ranks and banked points
    /// derived at read. Files written before Intrinsics were tracked lack
    /// this and `intrinsics`.
    #[serde(default)]
    pub(crate) skills: HashMap<String, i64>,
    #[serde(default)]
    pub(crate) intrinsics: Provenance,
    #[serde(default)]
    pub(crate) missions: HashMap<String, BlobMission>,
    /// Nodes and junctions both come from `Missions`, so one observation
    /// covers both kinds.
    #[serde(default)]
    pub(crate) nodes: Provenance,
    #[serde(default)]
    pub(crate) affiliations: HashMap<String, BlobAffiliation>,
    #[serde(default)]
    pub(crate) standing: Provenance,
    /// Observations never touch it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) plan: Option<MasteryPlan>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct ConfirmedKinds {
    equipment: bool,
    intrinsics: bool,
    nodes: bool,
    standing: bool,
}

pub(crate) struct MasteryProgress {
    path: PathBuf,
    record: PlayerProgress,
    /// What the last applied blob confirmed, or `None` when it confirmed
    /// nothing. An unchanged scan re-observes only those kinds.
    last_confirmed: Option<ConfirmedKinds>,
    /// An unchanged scan moves the stamp in memory once a minute. Writing
    /// the whole file that often bought nothing, so it reaches disk on an
    /// interval.
    last_reobserve_saved: Option<i64>,
    /// Set when the file exists but cannot be read: the session runs in
    /// memory and never overwrites history it could not load.
    write_blocked: bool,
}

impl MasteryProgress {
    pub(crate) fn load(path: PathBuf, pre_provenance: &InventoryStateCache) -> Self {
        match std::fs::read_to_string(&path) {
            Ok(text) => match serde_json::from_str(&text) {
                Ok(record) => Self { path, record, last_confirmed: None, last_reobserve_saved: None, write_blocked: false },
                Err(e) => Self::unreadable(path, &e),
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let record = import_pre_provenance(pre_provenance);
                let progress = Self { path, record, last_confirmed: None, last_reobserve_saved: None, write_blocked: false };
                progress.save();
                progress
            }
            Err(e) => Self::unreadable(path, &e),
        }
    }

    fn unreadable(path: PathBuf, error: &dyn std::fmt::Display) -> Self {
        error!(path = %path.display(), %error, "mastery progress unreadable; keeping the file, running in memory");
        Self { path, record: PlayerProgress::default(), last_confirmed: None, last_reobserve_saved: None, write_blocked: true }
    }

    /// `affinity` is `XPInfo`, `skills` is `PlayerSkills`, `missions` is
    /// `Missions` and `affiliations` is `Affiliations`. Each confirms its
    /// own source kind, and one that was not an array or object leaves its
    /// kind as it was.
    pub(crate) fn apply_blob(
        &mut self,
        affinity: Option<&HashMap<String, i64>>,
        skills: Option<&HashMap<String, i64>>,
        missions: Option<&HashMap<String, BlobMission>>,
        affiliations: Option<&HashMap<String, BlobAffiliation>>,
        now: i64,
    ) -> bool {
        if affinity.is_none() && skills.is_none() && missions.is_none() && affiliations.is_none() {
            self.last_confirmed = None;
            return false;
        }
        let record = &mut self.record;
        let confirmed = Provenance { state: ProvenanceState::Confirmed, observed_at: Some(now) };
        if let Some(affinity) = affinity {
            record.affinity = affinity.clone();
            record.equipment = confirmed;
        }
        if let Some(skills) = skills {
            record.skills = skills.clone();
            record.intrinsics = confirmed;
        }
        if let Some(missions) = missions {
            record.missions = missions.clone();
            record.nodes = confirmed;
        }
        if let Some(affiliations) = affiliations {
            record.affiliations = affiliations.clone();
            record.standing = confirmed;
        }
        self.last_confirmed = Some(ConfirmedKinds {
            equipment: affinity.is_some(), intrinsics: skills.is_some(), nodes: missions.is_some(), standing: affiliations.is_some(),
        });
        self.save();
        true
    }

    /// A blob rejected after parsing. Its bytes are what the next unchanged
    /// scan compares against, so they must not count as a re-observation.
    pub(crate) fn discard_blob(&mut self) {
        self.last_confirmed = None;
    }

    const REOBSERVE_SAVE_INTERVAL: i64 = 300;

    pub(crate) fn reobserve(&mut self, now: i64) -> bool {
        let Some(confirmed) = self.last_confirmed else { return false };
        let record = &mut self.record;
        if confirmed.equipment { record.equipment.observed_at = Some(now); }
        if confirmed.intrinsics { record.intrinsics.observed_at = Some(now); }
        if confirmed.nodes { record.nodes.observed_at = Some(now); }
        if confirmed.standing { record.standing.observed_at = Some(now); }
        if self.last_reobserve_saved.is_none_or(|at| now - at >= Self::REOBSERVE_SAVE_INTERVAL) {
            self.last_reobserve_saved = Some(now);
            self.save();
        }
        true
    }

    pub(crate) fn record(&self) -> &PlayerProgress { &self.record }

    pub(crate) fn set_plan(&mut self, plan: MasteryPlan) {
        self.record.plan = Some(plan);
        self.save();
    }

    pub(crate) fn clear(&mut self) {
        let _ = std::fs::remove_file(&self.path);
        self.record = PlayerProgress::default();
        self.last_confirmed = None;
        self.write_blocked = false;
    }

    fn save(&self) {
        if self.write_blocked { return; }
        match serde_json::to_string(&self.record) {
            Ok(json) => if let Err(e) = atomic_write(&self.path, json.as_bytes()) {
                error!(path = %self.path.display(), error = %e, "mastery progress not written");
            },
            Err(e) => error!(error = %e, "mastery progress not serialized"),
        }
    }
}

fn import_pre_provenance(cache: &InventoryStateCache) -> PlayerProgress {
    if !cache.has_account_observation() { return PlayerProgress::default(); }
    PlayerProgress {
        affinity: cache.mastery_data().into_iter()
            .map(|(path, rank)| { let affinity = rank_to_affinity(rank, &path); (path, affinity) })
            .collect(),
        equipment: Provenance { state: ProvenanceState::Unconfirmed, observed_at: None },
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inventory_state::{CachedItem, CREDITS_PATH};
    use crate::memory_scanner::BlobMission;
    use crate::mastery_rules::xp_to_rank;

    const BRATON: &str = "/Lotus/Weapons/Tenno/Rifle/Braton";
    const KUVA: &str = "/Lotus/Weapons/Grineer/KuvaLich/LongGuns/Karak/KuvaKarak";
    const MAG: &str = "/Lotus/Powersuits/Mag/Mag";

    fn scratch(label: &str) -> (tempfile::TempDir, PathBuf) { crate::cache::test_scratch(label) }

    fn pre_provenance_cache(ranks: &[(&str, u32)], observed: bool) -> InventoryStateCache {
        let mut cache = InventoryStateCache::default();
        for (path, rank) in ranks {
            cache.items.insert((*path).into(), CachedItem { unique_name: (*path).into(), mastery_rank: *rank, ..Default::default() });
        }
        if observed {
            cache.items.insert(CREDITS_PATH.into(), CachedItem { unique_name: CREDITS_PATH.into(), amount: 1, ..Default::default() });
        }
        cache
    }

    fn affinity(entries: &[(&str, i64)]) -> HashMap<String, i64> {
        entries.iter().map(|(path, xp)| ((*path).to_string(), *xp)).collect()
    }

    fn missions(entries: &[(&str, u32, Option<u32>)]) -> HashMap<String, BlobMission> {
        entries.iter().map(|(key, completes, tier)| ((*key).to_string(), BlobMission { completes: *completes, tier: *tier })).collect()
    }

    fn fresh(label: &str) -> (tempfile::TempDir, PathBuf, MasteryProgress) {
        let (dir, path) = scratch(label);
        let progress = MasteryProgress::load(path.clone(), &InventoryStateCache::default());
        (dir, path, progress)
    }

    #[test]
    fn pre_provenance_cache_with_account_observation_imports_as_unconfirmed() {
        let (_dir, path) = scratch("import");
        let progress = MasteryProgress::load(path.clone(), &pre_provenance_cache(&[(BRATON, 30), (KUVA, 40), (MAG, 12)], true));
        let record = progress.record();
        assert_eq!(record.equipment, Provenance { state: ProvenanceState::Unconfirmed, observed_at: None });
        assert_eq!(xp_to_rank(record.affinity[BRATON], BRATON), 30);
        assert_eq!(xp_to_rank(record.affinity[KUVA], KUVA), 40);
        assert_eq!(xp_to_rank(record.affinity[MAG], MAG), 12);
        assert!(path.exists(), "import persisted");
    }

    #[test]
    fn catalogue_only_cache_imports_nothing() {
        let (_dir, path) = scratch("catalogue");
        let progress = MasteryProgress::load(path.clone(), &pre_provenance_cache(&[(BRATON, 0)], false));
        assert_eq!(progress.record().equipment, Provenance::default());
        assert!(progress.record().affinity.is_empty());
    }

    #[test]
    fn accepted_blob_confirms_equipment_and_replaces_the_previous_map() {
        let (_dir, _path, mut progress) = fresh("apply");
        let first = affinity(&[(BRATON, 450_000), (MAG, 144_000)]);
        assert!(progress.apply_blob(Some(&first), None, None, None, 1_000));
        let record = progress.record();
        assert_eq!(record.equipment, Provenance { state: ProvenanceState::Confirmed, observed_at: Some(1_000) });
        assert_eq!(record.affinity, first);

        let second = affinity(&[(BRATON, 450_000)]);
        assert!(progress.apply_blob(Some(&second), None, None, None, 2_000));
        let record = progress.record();
        assert_eq!(record.affinity, second, "absent from a confirmed field is zero, not carried over");
        assert_eq!(record.equipment.observed_at, Some(2_000));

        assert!(!progress.apply_blob(None, None, None, None, 3_000), "XPInfo not an array confirms nothing");
        let record = progress.record();
        assert_eq!((record.affinity.clone(), record.equipment.observed_at), (second, Some(2_000)));
    }

    #[test]
    fn unchanged_scan_reobserves_only_what_the_last_blob_confirmed() {
        let (_dir, _path, mut progress) = fresh("reobserve");
        assert!(!progress.reobserve(500), "nothing confirmed yet");
        progress.apply_blob(Some(&affinity(&[(BRATON, 450_000)])), None, None, None, 1_000);
        assert!(progress.reobserve(1_060));
        assert_eq!(progress.record().equipment.observed_at, Some(1_060));

        progress.apply_blob(None, None, None, None, 1_200);
        assert!(!progress.reobserve(1_260), "last blob confirmed nothing");

        progress.apply_blob(Some(&affinity(&[(BRATON, 450_000)])), None, None, None, 1_300);
        progress.discard_blob();
        assert!(!progress.reobserve(1_360), "rejected bytes are not an observation");
        assert_eq!(progress.record().equipment.observed_at, Some(1_300));
    }

    #[test]
    fn unreadable_file_is_kept_and_never_overwritten() {
        let (_dir, path) = scratch("corrupt");
        std::fs::write(&path, b"{ not json").expect("scratch writable");
        let mut progress = MasteryProgress::load(path.clone(), &pre_provenance_cache(&[(BRATON, 30)], true));
        assert_eq!(progress.record().equipment, Provenance::default(), "no import over an existing file");
        progress.apply_blob(Some(&affinity(&[(BRATON, 450_000)])), None, None, None, 1_000);
        assert_eq!(progress.record().equipment.observed_at, Some(1_000), "in-memory progress works");
        assert_eq!(std::fs::read(&path).expect("file kept"), b"{ not json");
    }

    #[test]
    fn missions_confirm_nodes_separately_from_equipment() {
        let (_dir, _path, mut progress) = fresh("missions");
        let cleared = missions(&[("SolNode27", 14, Some(1)), ("EarthToVenusJunction", 2, None)]);
        assert!(progress.apply_blob(Some(&affinity(&[(BRATON, 450_000)])), None, Some(&cleared), None, 1_000));
        let record = progress.record();
        assert_eq!(record.missions, cleared);
        assert_eq!(record.nodes, Provenance { state: ProvenanceState::Confirmed, observed_at: Some(1_000) });

        assert!(progress.apply_blob(Some(&affinity(&[(BRATON, 450_000)])), None, None, None, 2_000), "XPInfo alone still confirms equipment");
        let record = progress.record();
        assert_eq!((record.equipment.observed_at, record.nodes.observed_at), (Some(2_000), Some(1_000)));
        assert_eq!(record.missions, cleared, "Missions not an array leaves the last observation in place");
        assert!(progress.reobserve(2_060));
        assert_eq!((progress.record().equipment.observed_at, progress.record().nodes.observed_at), (Some(2_060), Some(1_000)),
            "an unchanged scan re-observes only what the last blob confirmed");

        assert!(progress.apply_blob(None, None, Some(&missions(&[("SolNode27", 15, Some(1))])), None, 3_000), "Missions alone confirms nodes");
        let record = progress.record();
        assert_eq!((record.equipment.observed_at, record.nodes.observed_at), (Some(2_060), Some(3_000)));
        assert_eq!(record.missions.len(), 1, "absent from a confirmed field is zero, not carried over");

        assert!(!progress.apply_blob(None, None, None, None, 4_000));
        assert!(!progress.reobserve(4_060));
    }

    #[test]
    fn a_file_from_before_missions_loads_them_as_unknown() {
        let (_dir, path) = scratch("pre-missions");
        std::fs::write(&path, format!(
            r#"{{"affinity":{{"{BRATON}":450000}},"equipment":{{"state":"confirmed","observed_at":1000}}}}"#
        )).expect("scratch writable");
        let progress = MasteryProgress::load(path.clone(), &InventoryStateCache::default());
        let record = progress.record();
        assert_eq!(record.equipment.state, ProvenanceState::Confirmed);
        assert_eq!(record.nodes, Provenance::default());
        assert!(record.missions.is_empty());
    }

    #[test]
    fn a_plan_outlives_observations_and_restarts() {
        let (_dir, path, mut progress) = fresh("plan");
        let plan = |target: u32, selections: &[&str]| MasteryPlan {
            target, view: "suggestions".into(), selections: selections.iter().map(|s| (*s).to_string()).collect(), allowances: HashMap::new(), intrinsic_targets: HashMap::new(), purchase_comparison: String::new(),
        };
        progress.apply_blob(Some(&affinity(&[(BRATON, 450_000)])), None, None, None, 1_000);
        assert_eq!(progress.record().plan, None, "no plan until one is saved");
        progress.set_plan(plan(12, &[KUVA, MAG]));
        progress.apply_blob(Some(&affinity(&[(BRATON, 450_000), (KUVA, 800_000)])), None, None, None, 2_000);
        assert_eq!(progress.record().plan, Some(plan(12, &[KUVA, MAG])), "a new observation leaves the plan alone");

        let reloaded = MasteryProgress::load(path.clone(), &InventoryStateCache::default());
        assert_eq!(reloaded.record().plan, Some(plan(12, &[KUVA, MAG])));

        progress.set_plan(plan(5, &[]));
        let reloaded = MasteryProgress::load(path.clone(), &InventoryStateCache::default());
        assert_eq!(reloaded.record().plan, Some(plan(5, &[])), "a cleared plan is still a plan");
    }

    #[test]
    fn clear_forgets_progress_and_unblocks_writes() {
        let (_dir, path) = scratch("clear");
        std::fs::write(&path, b"{ not json").expect("scratch writable");
        let mut progress = MasteryProgress::load(path.clone(), &InventoryStateCache::default());
        progress.apply_blob(Some(&affinity(&[(BRATON, 450_000)])), None, None, None, 1_000);
        progress.clear();
        assert_eq!(progress.record().equipment, Provenance::default());
        assert!(!path.exists());
        progress.apply_blob(Some(&affinity(&[(BRATON, 450_000)])), None, None, None, 2_000);
        assert!(path.exists(), "writes resume after clear");
    }

    #[test]
    fn intrinsics_confirm_apart_from_equipment_and_a_legacy_file_leaves_them_unknown() {
        let (_dir, path) = scratch("intrinsics");
        std::fs::write(&path, r#"{"affinity":{},"equipment":{"state":"confirmed","observed_at":900}}"#)
            .expect("scratch writable");
        let mut progress = MasteryProgress::load(path.clone(), &InventoryStateCache::default());
        let a = progress.record();
        assert_eq!((a.equipment.state, a.intrinsics), (ProvenanceState::Confirmed, Provenance::default()));

        let skills = affinity(&[("LPS_GUNNERY", 8), ("LPP_SPACE", 89_930)]);
        assert!(progress.apply_blob(None, Some(&skills), None, None, 1_000), "skills alone confirm something");
        let a = progress.record();
        assert_eq!(a.intrinsics, Provenance { state: ProvenanceState::Confirmed, observed_at: Some(1_000) });
        assert_eq!((a.skills.clone(), a.equipment.observed_at), (skills.clone(), Some(900)));

        assert!(progress.reobserve(1_060));
        let a = progress.record();
        assert_eq!((a.intrinsics.observed_at, a.equipment.observed_at), (Some(1_060), Some(900)), "only what the last blob confirmed re-observes");

        assert!(progress.apply_blob(Some(&affinity(&[(BRATON, 450_000)])), None, None, None, 2_000));
        let a = progress.record();
        assert_eq!((a.equipment.observed_at, a.intrinsics.observed_at), (Some(2_000), Some(1_060)), "skills not an object: intrinsics kept as they were");
        assert!(progress.reobserve(2_060));
        let a = progress.record();
        assert_eq!((a.equipment.observed_at, a.intrinsics.observed_at), (Some(2_060), Some(1_060)));

        let reloaded = MasteryProgress::load(path.clone(), &InventoryStateCache::default());
        let a = reloaded.record();
        assert_eq!((a.skills.clone(), a.intrinsics.observed_at), (skills, Some(1_060)));
    }

    #[test]
    fn affiliations_confirm_standing_apart_from_the_other_kinds() {
        let (_dir, path, mut progress) = fresh("standing");
        let hex: HashMap<String, BlobAffiliation> = [("HexSyndicate".to_string(), BlobAffiliation { standing: 135_500, title: 5 })].into();
        assert!(progress.apply_blob(None, None, None, Some(&hex), 1_000), "affiliations alone confirm standing");
        let a = progress.record();
        assert_eq!((a.affiliations.clone(), a.standing), (hex.clone(), Provenance { state: ProvenanceState::Confirmed, observed_at: Some(1_000) }));
        assert_eq!(a.equipment, Provenance::default());

        assert!(progress.apply_blob(Some(&affinity(&[(BRATON, 450_000)])), None, None, None, 2_000));
        assert!(progress.reobserve(2_060));
        let a = progress.record();
        assert_eq!((a.equipment.observed_at, a.standing.observed_at), (Some(2_060), Some(1_000)), "Affiliations not an array: standing kept as it was");

        let reloaded = MasteryProgress::load(path.clone(), &InventoryStateCache::default());
        let a = reloaded.record();
        assert_eq!((a.affiliations.clone(), a.standing.observed_at), (hex, Some(1_000)));
    }
}
