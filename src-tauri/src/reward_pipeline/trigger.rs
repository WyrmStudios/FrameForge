//! EE.log trigger handling: session relic collection, the VoidProjections
//! reward handshake, and catalog prefiltering for a new reward session.

use tauri::Manager;

use super::RewardDiagnosticSession;
use crate::app_state::AppState;

/// The trigger line that opened a reward session plus its catalog prefilter summary.
pub(crate) struct RewardTrigger<'a> {
    pub(crate) timestamp: &'a str,
    pub(crate) trigger_line: &'a str,
    pub(crate) prefilter_log: &'a str,
    pub(crate) catalog_len: usize,
}

/// The latest expected card count and the evidence that supplied it.
#[derive(Clone, Copy, Default)]
pub(crate) struct SquadHint {
    pub(crate) size: Option<usize>,
    /// 0 = none, 1 = relic-count guess, 2 = EE.log handshake.
    pub(crate) source: u8,
}

pub(crate) fn collect_session_relics(text: &str, session_relics: &mut Vec<String>) {
    for line in text.lines() {
        if line.contains("Resource load completed")
            && line.contains("/Lotus/Types/Game/Projections/")
        {
            if let Some(paren) = line.find("(/Lotus/Types/Game/Projections/") {
                let path = line[paren + 1..].split(')').next().unwrap_or("").trim();
                if !path.is_empty() && !session_relics.iter().any(|relic| relic == path) {
                    session_relics.push(path.to_string());
                }
            }
        }
    }
}

#[derive(Default)]
pub(crate) struct VoidProjectionState {
    in_sequence: bool,
    sequence_completed: bool,
    other_ids: std::collections::HashSet<String>,
    own_item: String,
}

impl VoidProjectionState {
    pub(crate) fn consume_sequence_completed(&mut self) -> bool {
        std::mem::take(&mut self.sequence_completed)
    }

    pub(crate) fn take_own_item(&mut self) -> Option<String> {
        if self.own_item.is_empty() {
            None
        } else {
            Some(std::mem::take(&mut self.own_item))
        }
    }
}

/// Update the VoidProjections reward-handshake state from newly appended EE.log lines.
///
/// `squad_hint` keeps the current size and its source under one lock so attempt
/// logs cannot report a value with stale provenance.
pub(crate) fn collect_void_projection_state(
    text: &str,
    state: &mut VoidProjectionState,
    squad_hint: &std::sync::Arc<std::sync::Mutex<SquadHint>>,
    session: Option<&std::sync::Arc<RewardDiagnosticSession>>,
) {
    for line in text.lines() {
        let lower = line.to_lowercase();
        if lower.contains("voidprojections: getvoidprojectionreward") {
            state.in_sequence = true;
            state.other_ids.clear();
            state.own_item.clear();
            let previous = squad_hint.lock().ok().and_then(|hint| hint.size);
            if let Ok(mut hint) = squad_hint.lock() {
                *hint = SquadHint::default();
            }
            if previous.is_some() {
                if let Some(session) = session {
                    session.log(&serde_json::json!({
                        "t": super::events::now_ts(),
                        "event": "hint",
                        "source": "none",
                        "size": serde_json::Value::Null,
                        "prev": previous,
                        "reason": "new_voidprojections_sequence",
                    }));
                }
            }
        }
        if lower.contains("gets reward /lotus/") {
            if let Some(index) = line.find("/Lotus/") {
                state.own_item = line[index..].trim().to_string();
            }
        }
        if state.in_sequence {
            if lower.contains("still waiting on response from") {
                if let Some(id) = lower.split_whitespace().last() {
                    state.other_ids.insert(id.to_string());
                }
            } else if lower.contains("has reward info for all players now") {
                let squad = (1 + state.other_ids.len()).clamp(1, 4);
                let previous = squad_hint.lock().ok().and_then(|hint| hint.size);
                if let Ok(mut hint) = squad_hint.lock() {
                    hint.size = Some(squad);
                    hint.source = 2;
                }
                state.in_sequence = false;
                state.sequence_completed = true;
                if let Some(session) = session {
                    session.log(&serde_json::json!({
                    "t": super::events::now_ts(),
                    "event": "hint",
                    "source": "ee_handshake",
                    "size": squad,
                    "prev": previous,
                    "own_item": if state.own_item.is_empty() { serde_json::Value::Null } else { state.own_item.clone().into() },
                    "other_ids": state.other_ids.len(),
                }));
                }
            }
        }
    }
}

/// Narrow the OCR catalog to rewards from relics seen in the current session.
pub(crate) fn filter_relic_reward_catalog(
    app: &tauri::AppHandle,
    session_relics: &[String],
    full_catalog: &std::sync::Arc<Vec<(String, String)>>,
) -> (std::sync::Arc<Vec<(String, String)>>, String) {
    if session_relics.is_empty() {
        return (
            std::sync::Arc::clone(full_catalog),
            "  No relics collected — using full catalog (FrameForge started mid-mission?)"
                .to_string(),
        );
    }
    let mut rewards: Vec<(String, String)> = {
        let state = app.state::<AppState>();
        let reward_map = state
            .relic_rewards
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        session_relics
            .iter()
            .filter_map(|path| reward_map.get(path.as_str()))
            .flat_map(|rewards| {
                rewards
                    .iter()
                    .map(|reward| (reward.unique_name.clone(), reward.name.clone()))
            })
            .filter(|(_, name)| !name.is_empty())
            .collect()
    };
    if rewards.is_empty() {
        return (
            std::sync::Arc::clone(full_catalog),
            format!(
                "  {} relic path(s) found but none matched relic_rewards — using full catalog\n  Paths: {:?}",
                session_relics.len(), session_relics
            ),
        );
    }
    rewards.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
    rewards.dedup_by(|a, b| !a.0.is_empty() && a.0 == b.0);
    let names: Vec<&str> = rewards.iter().map(|(_, name)| name.as_str()).collect();
    let sample = &names[..names.len().min(8)];
    let log = format!(
        "  {} relic(s) → {} rewards (direct from Relics.json)\n  Relics: {:?}\n  Rewards: {:?}",
        session_relics.len(),
        rewards.len(),
        session_relics,
        sample
    );
    (std::sync::Arc::new(rewards), log)
}

/// Build a fallback OCR catalog when the initial cache was empty at monitor startup.
pub(crate) fn build_fallback_reward_catalog(
    app: &tauri::AppHandle,
) -> Option<std::sync::Arc<Vec<(String, String)>>> {
    let state = app.state::<AppState>();
    let items = state.wfcd_items.lock().unwrap_or_else(|e| e.into_inner());
    if items.is_empty() {
        return None;
    }
    let blueprints = state
        .blueprint_to_result
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let excluded = [
        "Warframes",
        "Primary",
        "Secondary",
        "Melee",
        "Companion",
        "Sentinels",
        "Archwing",
        "Arch-Gun",
        "Arch-Melee",
        "Pets",
        "Robotic",
    ];
    let mut catalog: Vec<(String, String)> = items
        .iter()
        .filter(|item| {
            let name = item.name.to_lowercase();
            !excluded.contains(&item.category.as_str())
                && !name.ends_with("intact")
                && !name.ends_with("exceptional")
                && !name.ends_with("flawless")
                && !name.ends_with("radiant")
                && (name.contains("prime") || name.starts_with("forma"))
        })
        .map(|item| (item.unique_name.clone(), item.name.clone()))
        .collect();
    for (path, (name, _)) in blueprints.iter() {
        let lower = name.to_lowercase();
        if lower.contains("prime") || lower.starts_with("forma") {
            catalog.push((path.clone(), name.clone()));
        }
    }
    catalog.sort_by(|a, b| a.0.cmp(&b.0));
    catalog.dedup_by(|a, b| a.0 == b.0);
    (!catalog.is_empty()).then(|| std::sync::Arc::new(catalog))
}

/// Start the diagnostic directory and direct JSONL log for one relic-reward session.
pub(crate) fn prepare_reward_session(
    session_id: u64,
    squad_names: &std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    trigger: RewardTrigger<'_>,
    auto_capture_dir: &std::path::Path,
) -> Option<std::sync::Arc<RewardDiagnosticSession>> {
    let session = RewardDiagnosticSession::begin(auto_capture_dir, session_id)?;
    let RewardTrigger {
        timestamp,
        trigger_line,
        prefilter_log,
        catalog_len,
    } = trigger;
    let names = squad_names
        .lock()
        .map(|names| names.clone())
        .unwrap_or_default();
    let prefilter: Vec<&str> = prefilter_log
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    let event = serde_json::json!({
        "t": timestamp,
        "event": "session_start",
        "trigger_line": trigger_line,
        "prefilter": prefilter,
        "catalog": catalog_len,
        "players": names,
        "log_path": session.log_path().display().to_string(),
    });
    session.log(&event);
    Some(session)
}

/// Prepare OCR hints immediately when a relic reward screen is triggered.
///
/// The squad-size guess derived from the session relic count is marked as a
/// guess (1) — it is only a stand-in until the EE.log VoidProjections handshake
/// (2) arrives.
pub(crate) fn prepare_reward_trigger(
    app: &tauri::AppHandle,
    squad_names: &std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    squad_hint: &std::sync::Arc<std::sync::Mutex<SquadHint>>,
    session_relics: &[String],
) {
    if let Ok(local_player) = app.state::<AppState>().local_player_name.lock() {
        if let Some(name) = local_player.as_ref() {
            if let Ok(mut names) = squad_names.lock() {
                if !names.iter().any(|existing| existing == name) {
                    names.push(name.clone());
                }
            }
        }
    }
    let relic_hint = session_relics.len().min(4);
    if relic_hint >= 1 {
        if let Ok(mut hint) = squad_hint.lock() {
            if hint.size.is_none() {
                hint.size = Some(relic_hint);
                hint.source = 1;
            }
        }
    }
}

/// Record the squad-size hint in effect when the session started, including
/// where it came from — a relic-count guess is never an EE.log fact.
pub(crate) fn log_session_hint(
    squad_hint: &std::sync::Arc<std::sync::Mutex<SquadHint>>,
    session_relics: &[String],
    session: Option<&std::sync::Arc<RewardDiagnosticSession>>,
) {
    let hint = squad_hint.lock().map(|hint| *hint).unwrap_or_default();
    let source = match hint.source {
        1 => "relic_guess",
        2 => "ee_handshake",
        0 => "none",
        _ => "unknown",
    };
    if let Some(session) = session {
        session.log(&serde_json::json!({
            "t": super::events::now_ts(),
            "event": "hint",
            "source": source,
            "size": hint.size,
            "session_relics": session_relics.len(),
            "note": (source == "relic_guess")
                .then(|| format!("min(session_relics={}, 4)", session_relics.len())),
        }));
    }
}
