//! Arbitration run tracking from `EE.log`.
//!
//! Lines in, run events out. The parser holds no clock, does no I/O and keeps
//! no global state, so the same code serves the live log tail and a backfill
//! pass over a whole file.
//!
//! The marker set and the counting rules are a reimplementation of the
//! MIT-licensed run parser and vitus model in WFHelper
//! (<https://github.com/WFHelper/WFHelper>, `services/arbiRunParser.ts` and
//! `config/shared/arbiMath.ts`). Its vitus model in turn follows
//! svesk.github.io/arbi. See `THIRD_PARTY_NOTICES.md`.
//!
//! What the log can and cannot say shapes the record:
//!
//! - Only the host writes AI spawn lines, so a client-side run has no kill
//!   or drone counts. `host_telemetry` says which kind a record is.
//! - "Kills" are spawns the host logged. The log has no kill line, and in an
//!   arbitration every spawn that is not decorative dies or ends the run.
//! - Wave, reward and state-change lines are locale-independent; the mission
//!   name is not, which is why the `_EliteAlert` sector suffix is the primary
//!   arbitration marker and the "Arbitration" keyword only a fallback.
//!
//! Deliberately not ported: per-wave durations, the enemy-saturation
//! histogram, squad member names and pause bookkeeping. The run record does
//! not carry them and nothing downstream reads them yet.
//!
//! Deliberately different: the vitus estimate is filled for every mission
//! type, where the original withholds it outside defense, interception and
//! disruption. A per-run rate is the point of the record, and rotations and
//! drones are counted the same way everywhere.
//! TODO: the rotation-bonus term assumes three waves per rotation; survival
//! rotations are five-minute intervals and may deserve their own factor.

use chrono::{DateTime, NaiveDateTime, TimeDelta, Utc};
use regex::Regex;
use std::collections::HashSet;
use std::sync::LazyLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MissionType {
    Defense,
    Interception,
    Disruption,
    Survival,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndReason {
    MissionEnd,
    Aborted,
    NewMission,
    /// The input ended without an end marker; the run may still be going.
    Unterminated,
}

impl MissionType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Defense => "defense",
            Self::Interception => "interception",
            Self::Disruption => "disruption",
            Self::Survival => "survival",
            Self::Other => "other",
        }
    }

    /// An unrecognised name reads back as `Other` rather than failing, so a
    /// database written by a later build still opens in this one.
    pub fn from_stored(name: &str) -> Self {
        match name {
            "defense" => Self::Defense,
            "interception" => Self::Interception,
            "disruption" => Self::Disruption,
            "survival" => Self::Survival,
            _ => Self::Other,
        }
    }
}

impl EndReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::MissionEnd => "mission_end",
            Self::Aborted => "aborted",
            Self::NewMission => "new_mission",
            Self::Unterminated => "unterminated",
        }
    }

    /// A name this build does not know reads back as `Unterminated`: of the
    /// four it is the one that claims least, where defaulting to `MissionEnd`
    /// would report an unknown ending as a run the player completed.
    pub fn from_stored(name: &str) -> Self {
        match name {
            "mission_end" => Self::MissionEnd,
            "aborted" => Self::Aborted,
            "new_mission" => Self::NewMission,
            _ => Self::Unterminated,
        }
    }
}

/// Normal-approximation estimate of the vitus essence a run yields.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vitus {
    pub mean: f64,
    pub std: f64,
    /// `mean` over the run's combat window; 0 when the window is empty.
    pub per_minute: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Run {
    /// Wall clock of `run_start_sec`, when the log's boot-time header was seen.
    pub started_at: Option<DateTime<Utc>>,
    pub run_start_sec: f64,
    pub run_end_sec: Option<f64>,
    pub mission_name: String,
    /// The mission name stripped of its arbitration decoration.
    pub node: String,
    /// Star chart node id such as `SolNode167`.
    pub sol_node: Option<String>,
    pub mission_type: MissionType,
    /// The engine's `MT_*` type; outranks every name heuristic once seen.
    pub mission_type_raw: Option<String>,
    pub end_reason: EndReason,
    /// Combat window: from the first gameplay marker to the last combat event.
    pub duration_sec: f64,
    pub rotations: u32,
    /// Highest defense wave reached; 0 outside defense.
    pub waves: u32,
    pub waves_per_rotation: u32,
    /// Non-decorative spawns plus drones.
    pub kills: u32,
    pub drone_kills: u32,
    pub host_telemetry: bool,
    pub vitus: Vitus,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    RunStarted {
        node: String,
        mission_type: MissionType,
        game_time_sec: f64,
    },
    WaveAdvanced(u32),
    RotationAdvanced(u32),
    AgentSpawned {
        drone: bool,
    },
    RunEnded(Box<Run>),
}

// ==============================================================================
// Vitus model
// ==============================================================================

const VITUS_DROP_CHANCE: f64 = 0.15;
/// Chance the Retriever mod doubles a drop (4 instead of 2 vitus).
const VITUS_RETRIEVER_CHANCE: f64 = 0.18;
/// Chance a rotation reward is the bonus vitus bundle, per wave in the rotation.
const ROTATION_VITUS_CHANCE: f64 = 0.1;

pub fn vitus_model(rotations: u32, waves_per_rotation: u32, drones: u32) -> (f64, f64) {
    let rotations = f64::from(rotations);
    let waves = f64::from(waves_per_rotation);
    let drones = f64::from(drones);

    let drop_mean = 4.0 * VITUS_RETRIEVER_CHANCE + 2.0 * (1.0 - VITUS_RETRIEVER_CHANCE);
    let drop_sq = 16.0 * VITUS_RETRIEVER_CHANCE + 4.0 * (1.0 - VITUS_RETRIEVER_CHANCE);
    let drop_var = drop_sq - drop_mean * drop_mean;

    let rot_mean = rotations + rotations * ROTATION_VITUS_CHANCE * waves;
    let rot_var = rotations * ROTATION_VITUS_CHANCE * (1.0 - ROTATION_VITUS_CHANCE) * waves * waves;

    let drops_mean = drones * VITUS_DROP_CHANCE;
    let drops_var = drones * VITUS_DROP_CHANCE * (1.0 - VITUS_DROP_CHANCE);

    let mean = rot_mean + drops_mean * drop_mean;
    let variance = rot_var + drops_mean * drop_var + drop_mean * drop_mean * drops_var;
    (mean, variance.max(0.0).sqrt())
}

// ==============================================================================
// Line markers
// ==============================================================================

macro_rules! re {
    ($name:ident, $pattern:expr) => {
        static $name: LazyLock<Regex> =
            LazyLock::new(|| Regex::new($pattern).expect("pattern is a literal"));
    };
}

re!(MISSION_NAME, r"ThemedSquadOverlay\.lua: Mission name: (.*)");
re!(
    AGENT_FULL,
    r"OnAgentCreated.*?/Npc/(.+?)(\d+)\s+.*?MonitoredTicking\s+(\d+)"
);
re!(AGENT_NPC_NAME, r"/Npc/([A-Za-z0-9_]+)");
re!(
    AGENT_EXCLUDE,
    r"(?i)Replicant|RJCrew|petavatar|VoidClone|Turret|Dropship|CatbrowPetAgent|AllyAgent"
);
re!(DEFENSE_WAVE, r"WaveDefend\.lua: Defense wave: (\d+)");
re!(
    TERRITORY_START,
    r"(?i)TerritoryMission\.lua: .*(control|captured)"
);
// Selecting an arbitration logs its sector with an `_EliteAlert` suffix before
// the localised mission name, on both the squad-overlay and map paths.
re!(
    PENDING_SECTOR_PLAIN,
    r"(?:ThemedSquadOverlay\.lua: Pending mission:|MapRedux\.lua: Confirm sector) (\S+)"
);
re!(
    PENDING_SECTOR_JSON,
    r#"Set squad mission.*?"name":"([^"]+)""#
);
re!(ELITE_SECTOR, r"^(SolNode\d+)_EliteAlert$");
// A mid-mission join never logs "Mission name:"; the client load is the only
// start signal, and its sector still carries the suffix.
re!(
    CLIENT_MISSION_JOIN,
    r#"Client (?:joining mission in-progress|loaded)[^{]*\{"name":"([^"]+)"\}"#
);
re!(
    CACHED_MISSION_NAME,
    r"ThemedSquadOverlay\.lua: Cached mission name=(.+) \((SolNode\d+)\)"
);
re!(
    SYNC_CONSUMABLES,
    r"SyncAutoPopulatedConsumables for mission (MT_[A-Z_]+) with location (\S+)"
);
re!(
    STATE_STARTED,
    r"Game \[Info\]: OnStateStarted, mission type=(MT_[A-Z_]+)"
);

const DRONE_AGENT: &str = "CorpusEliteShieldDroneAgent";
const DEFENSE_REWARD: &str = "Sys [Info]: Created /Lotus/Interface/DefenseReward.swf";
const SURVIVAL_REWARD: &str = "Sys [Info]: Created /Lotus/Interface/SurvivalReward.swf";
const TERRITORY: &str = "Script [Info]: TerritoryMission.lua";
const DISRUPTION_ROUND_START: &str =
    "SentientArtifactMission.lua: Disruption: State change: ARTIFACT_ROUND";
const DISRUPTION_ROUND_DONE: &str =
    "SentientArtifactMission.lua: Disruption: State change: ARTIFACT_ROUND_DONE";
// The confirmed abort and the end-of-match inventory commit are the reliable
// ends. `EndOfMatch.lua: Initialize` and `Mission Succeeded` fire in-mission.
const ABORT_CONFIRMED: &str = "TopMenu.lua: Abort:";
const EOM_COMMIT: &str = "Sys [Info]: EOM missionLocationUnlocked=";
const BOOT_TIME_UTC: &str = "[UTC: ";

/// The reward UI can be created twice for one rotation.
const REWARD_DEBOUNCE_SEC: f64 = 30.0;
/// Mirror Defense runs two waves per rotation instead of three.
const MIRROR_DEFENSE_NODES: [&str; 2] = ["munio", "tyana"];
const DISRUPTION_CONDUITS_PER_ROUND: u32 = 4;

/// Game-relative seconds at the head of a line. Warframe occasionally writes a
/// stray character before the number, so leading non-digits are skipped.
fn line_timestamp(line: &str) -> f64 {
    let start = line
        .find(|c: char| c.is_ascii_digit())
        .unwrap_or(line.len());
    let number = &line[start..];
    let end = number
        .find(|c: char| !c.is_ascii_digit() && c != '.')
        .unwrap_or(number.len());
    let number = &number[..end];
    if number.contains('.') {
        number.parse().unwrap_or(0.0)
    } else {
        0.0
    }
}

fn is_spam(line: &str) -> bool {
    line.contains("Game [Warning]:") || line.contains("DamagePct")
}

// ==============================================================================
// Parser
// ==============================================================================

struct SpawnEvent {
    name: Option<String>,
    tick: Option<u32>,
}

struct RunState {
    mission_name: String,
    node: String,
    sol_node: Option<String>,
    mission_type: MissionType,
    mission_type_raw: Option<String>,
    waves_per_rotation: u32,
    run_start_sec: f64,
    /// Wall clock at game-time zero, snapshotted when the run started: a log
    /// spanning several game launches carries a later header that would
    /// otherwise redate every earlier run.
    boot_time: Option<DateTime<Utc>>,
    /// `OnStateStarted`, the instant gameplay begins.
    mission_start_sec: Option<f64>,
    /// First wave, first territory change or first disruption round.
    precise_start_sec: Option<f64>,
    run_end_sec: Option<f64>,
    last_activity_sec: f64,
    host_telemetry: bool,
    rotations: u32,
    last_reward_sec: f64,
    waves: u32,
    drone_kills: u32,
    first_drone_sec: Option<f64>,
    spawns: Vec<SpawnEvent>,
}

impl RunState {
    fn activity(&mut self, ts: f64) {
        self.last_activity_sec = self.last_activity_sec.max(ts);
    }

    fn apply_raw_type(&mut self, raw: &str) {
        if self.mission_type_raw.is_some() {
            return;
        }
        self.mission_type_raw = Some(raw.to_string());
        self.mission_type = match raw {
            "MT_DEFENSE" => MissionType::Defense,
            "MT_TERRITORY" => MissionType::Interception,
            "MT_ARTIFACT" => MissionType::Disruption,
            "MT_SURVIVAL" => MissionType::Survival,
            _ => MissionType::Other,
        };
        // Node names never say "Disruption"; the engine type is the only place
        // the four-conduit rotation can be set.
        if self.mission_type == MissionType::Disruption {
            self.waves_per_rotation = DISRUPTION_CONDUITS_PER_ROUND;
        }
    }
}

/// Both name shapes exist: the legacy `Arbitration: Casta (Ceres)` and the
/// current `Oestrus (Eris) - Arbitration` suffix form.
fn classify_mission(mission_name: &str) -> (String, MissionType, u32) {
    let node = mission_name
        .replace("Arbitration:", "")
        .trim()
        .trim_end_matches("- Arbitration")
        .trim()
        .to_string();
    let lower = node.to_lowercase();
    let mirror = MIRROR_DEFENSE_NODES.iter().any(|m| lower.contains(m));
    let mission_type = if lower.contains("defense") || mirror {
        MissionType::Defense
    } else if lower.contains("interception") {
        MissionType::Interception
    } else {
        MissionType::Other
    };
    (node, mission_type, if mirror { 2 } else { 3 })
}

/// An agent name is confirmed ticking if any consecutive named pair shows its
/// tick counter advancing; names only ever seen non-advancing are decorative.
fn count_valid_spawns(spawns: &[SpawnEvent]) -> u32 {
    let named: Vec<&SpawnEvent> = spawns.iter().filter(|s| s.name.is_some()).collect();
    let mut confirmed = HashSet::new();
    let mut suspected = HashSet::new();
    for pair in named.windows(2) {
        let (prev, curr) = (pair[0], pair[1]);
        if let (Some(name), Some(prev_tick), Some(curr_tick)) = (&prev.name, prev.tick, curr.tick) {
            if curr_tick > prev_tick {
                confirmed.insert(name.as_str());
            } else {
                suspected.insert(name.as_str());
            }
        }
    }
    let decorative = |name: &str| suspected.contains(name) && !confirmed.contains(name);
    spawns
        .iter()
        .filter(|s| !s.name.as_deref().is_some_and(decorative))
        .count() as u32
}

#[derive(Default)]
pub struct Parser {
    /// Wall clock at game-time zero, from the log's `Current time:` header.
    boot_time: Option<DateTime<Utc>>,
    run: Option<RunState>,
    /// Sector of the most recent mission select, e.g. `SolNode167_EliteAlert`.
    pending_sector: Option<String>,
    /// Squad-overlay mission name, the only name source when joining in progress.
    cached_mission: Option<(String, String)>,
}

impl Parser {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_run_active(&self) -> bool {
        self.run.is_some()
    }

    /// Feed one log line. A mission-name line can both close the current run
    /// and open the next, which is why this returns a list.
    pub fn feed_line(&mut self, line: &str) -> Vec<Event> {
        let line = line.trim_end();
        if line.is_empty() || is_spam(line) {
            return Vec::new();
        }
        let ts = line_timestamp(line);

        if let Some(utc) = line
            .contains("Current time:")
            .then(|| line.split_once(BOOT_TIME_UTC))
            .flatten()
            .and_then(|(_, rest)| rest.strip_suffix(']'))
            .and_then(|s| NaiveDateTime::parse_from_str(s.trim(), "%a %b %e %H:%M:%S %Y").ok())
        {
            self.boot_time = Some(utc.and_utc());
        }

        if let Some(sector) = PENDING_SECTOR_PLAIN
            .captures(line)
            .or_else(|| PENDING_SECTOR_JSON.captures(line))
        {
            self.pending_sector = Some(sector[1].to_string());
        }
        if let Some(cached) = CACHED_MISSION_NAME.captures(line) {
            self.cached_mission = Some((cached[1].trim().to_string(), cached[2].to_string()));
        }

        if self.run.is_none() {
            if let Some(load) = CLIENT_MISSION_JOIN.captures(line) {
                if let Some(joined) = ELITE_SECTOR.captures(&load[1]) {
                    let sol_node = &joined[1];
                    let name = match &self.cached_mission {
                        Some((name, node)) if node == sol_node => name.clone(),
                        _ => sol_node.to_string(),
                    };
                    self.pending_sector = Some(load[1].to_string());
                    return vec![self.start_run(&name, ts)];
                }
            }
        }

        if let Some(mission) = MISSION_NAME.captures(line) {
            let name = mission[1].trim().to_string();
            let is_arbitration = name.contains("Arbitration")
                || self
                    .pending_sector
                    .as_deref()
                    .is_some_and(|s| ELITE_SECTOR.is_match(s));
            let Some(run) = self.run.as_mut() else {
                return if is_arbitration {
                    vec![self.start_run(&name, ts)]
                } else {
                    Vec::new()
                };
            };
            // After a host migration the log can repeat the arbitration's
            // mission-name line with an older timestamp; that is not a new run.
            if is_arbitration
                && ts > 0.0
                && run.last_activity_sec > 0.0
                && ts < run.last_activity_sec
            {
                return Vec::new();
            }
            if ts > 0.0 {
                run.run_end_sec = Some(ts);
            }
            let mut events = vec![self.end_run(EndReason::NewMission)];
            if is_arbitration {
                events.push(self.start_run(&name, ts));
            }
            return events;
        }

        let Some(run) = self.run.as_mut() else {
            return Vec::new();
        };

        if line.contains(ABORT_CONFIRMED) || line.contains(EOM_COMMIT) {
            if ts > 0.0 {
                run.run_end_sec = Some(ts);
            }
            let reason = if line.contains(ABORT_CONFIRMED) {
                EndReason::Aborted
            } else {
                EndReason::MissionEnd
            };
            return vec![self.end_run(reason)];
        }

        if let Some(sync) = SYNC_CONSUMABLES.captures(line) {
            run.apply_raw_type(&sync[1]);
            if run.sol_node.is_none() {
                run.sol_node = Some(sync[2].to_string());
            }
        }
        if let Some(state) = STATE_STARTED.captures(line) {
            run.apply_raw_type(&state[1]);
            if run.mission_start_sec.is_none() && ts > 0.0 {
                run.mission_start_sec = Some(ts);
            }
        }

        if line.contains(TERRITORY)
            && run.mission_type_raw.is_none()
            && run.mission_type == MissionType::Other
        {
            run.mission_type = MissionType::Interception;
            run.waves_per_rotation = 3;
        }

        if let Some(wave) = DEFENSE_WAVE.captures(line) {
            // Wave lines outrank the name heuristic, but not the engine type.
            if run.mission_type_raw.is_none() {
                run.mission_type = MissionType::Defense;
            }
            let wave: u32 = wave[1].parse().unwrap_or(0);
            run.waves = run.waves.max(wave);
            if ts > 0.0 {
                run.activity(ts);
                if run.precise_start_sec.is_none() && wave == 1 {
                    run.precise_start_sec = Some(ts);
                }
            }
            return vec![Event::WaveAdvanced(wave)];
        }
        if run.precise_start_sec.is_none() && ts > 0.0 && TERRITORY_START.is_match(line) {
            run.precise_start_sec = Some(ts);
        }

        // Disruption spams the survival reward UI, so only the round state
        // machine counts there.
        if run.mission_type == MissionType::Disruption && ts > 0.0 {
            if line.contains(DISRUPTION_ROUND_DONE) {
                run.rotations += 1;
                run.activity(ts);
                return vec![Event::RotationAdvanced(run.rotations)];
            }
            if line.ends_with(DISRUPTION_ROUND_START) {
                run.activity(ts);
                if run.precise_start_sec.is_none() {
                    run.precise_start_sec = Some(ts);
                }
                return Vec::new();
            }
        }

        // The survival reward UI also appears in other modes (seen 25s before
        // an interception's DefenseReward), so it only counts in survivals.
        let survival_reward =
            run.mission_type == MissionType::Survival && line.contains(SURVIVAL_REWARD);
        if survival_reward || line.contains(DEFENSE_REWARD) {
            if ts - run.last_reward_sec > REWARD_DEBOUNCE_SEC {
                run.rotations += 1;
                run.last_reward_sec = ts;
                run.activity(ts);
                return vec![Event::RotationAdvanced(run.rotations)];
            }
            return Vec::new();
        }

        if line.contains("OnAgentCreated") {
            run.host_telemetry = true;
            if line.contains(DRONE_AGENT) {
                if ts > 0.0 {
                    run.drone_kills += 1;
                    run.first_drone_sec.get_or_insert(ts);
                    run.activity(ts);
                }
                return vec![Event::AgentSpawned { drone: true }];
            }
            if AGENT_EXCLUDE.is_match(line) {
                return Vec::new();
            }
            let spawn = match AGENT_FULL.captures(line) {
                Some(full) => SpawnEvent {
                    name: Some(full[1].to_string()),
                    tick: full[3].parse().ok(),
                },
                None => SpawnEvent {
                    name: AGENT_NPC_NAME.captures(line).map(|npc| npc[1].to_string()),
                    tick: None,
                },
            };
            run.spawns.push(spawn);
            return vec![Event::AgentSpawned { drone: false }];
        }

        Vec::new()
    }

    pub fn finish(&mut self) -> Option<Run> {
        self.run
            .is_some()
            .then(|| match self.end_run(EndReason::Unterminated) {
                Event::RunEnded(run) => *run,
                _ => unreachable!("end_run only builds RunEnded"),
            })
    }

    fn start_run(&mut self, mission_name: &str, ts: f64) -> Event {
        let (node, mission_type, waves_per_rotation) = classify_mission(mission_name);
        // Consume the sector so a stale `_EliteAlert` cannot mark a later
        // mission as an arbitration.
        let sol_node = self
            .pending_sector
            .take()
            .and_then(|s| ELITE_SECTOR.captures(&s).map(|c| c[1].to_string()));
        self.run = Some(RunState {
            mission_name: mission_name.to_string(),
            node: node.clone(),
            sol_node,
            mission_type,
            mission_type_raw: None,
            waves_per_rotation,
            run_start_sec: ts,
            boot_time: self.boot_time,
            mission_start_sec: None,
            precise_start_sec: None,
            run_end_sec: None,
            last_activity_sec: ts,
            host_telemetry: false,
            rotations: 0,
            last_reward_sec: 0.0,
            waves: 0,
            drone_kills: 0,
            first_drone_sec: None,
            spawns: Vec::new(),
        });
        Event::RunStarted {
            node,
            mission_type,
            game_time_sec: ts,
        }
    }

    fn end_run(&mut self, end_reason: EndReason) -> Event {
        let r = self.run.take().expect("callers check a run is active");
        let drone_kills = r.drone_kills;
        let kills = count_valid_spawns(&r.spawns) + drone_kills;

        let start_sec = r
            .precise_start_sec
            .or(r.first_drone_sec)
            .or(r.mission_start_sec)
            .unwrap_or(r.run_start_sec);
        let mut duration_sec = (r.last_activity_sec - start_sec).max(0.0);
        // No real combat window (an early abort; a few load-time drones can
        // still span milliseconds): the end marker pins the mission window.
        if duration_sec < 1.0 {
            if let Some(end) = r.run_end_sec {
                duration_sec = (end - start_sec).max(0.0);
            }
        }

        let (mean, std) = vitus_model(r.rotations, r.waves_per_rotation, drone_kills);
        let per_minute = if duration_sec > 0.0 {
            mean / (duration_sec / 60.0)
        } else {
            0.0
        };

        let started_at = r.boot_time.and_then(|boot| {
            std::time::Duration::try_from_secs_f64(r.run_start_sec)
                .ok()
                .and_then(|duration| TimeDelta::from_std(duration).ok())
                .map(|d| boot + d)
        });

        Event::RunEnded(Box::new(Run {
            started_at,
            run_start_sec: r.run_start_sec,
            run_end_sec: r.run_end_sec,
            mission_name: r.mission_name,
            node: r.node,
            sol_node: r.sol_node,
            mission_type: r.mission_type,
            mission_type_raw: r.mission_type_raw,
            end_reason,
            duration_sec,
            rotations: r.rotations,
            waves: r.waves,
            waves_per_rotation: r.waves_per_rotation,
            kills,
            drone_kills,
            host_telemetry: r.host_telemetry,
            vitus: Vitus {
                mean,
                std,
                per_minute,
            },
        }))
    }
}

/// Every run in a log, in order. A run still open at the end of the input is
/// included with `EndReason::Unterminated`.
pub fn parse_log<'a>(lines: impl IntoIterator<Item = &'a str>) -> Vec<Run> {
    let mut parser = Parser::new();
    let mut runs = Vec::new();
    for line in lines {
        for event in parser.feed_line(line) {
            if let Event::RunEnded(run) = event {
                runs.push(*run);
            }
        }
    }
    runs.extend(parser.finish());
    runs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stored_names_round_trip_and_unknown_ones_claim_nothing() {
        for reason in [
            EndReason::MissionEnd,
            EndReason::Aborted,
            EndReason::NewMission,
            EndReason::Unterminated,
        ] {
            assert_eq!(EndReason::from_stored(reason.as_str()), reason);
        }
        for mission in [
            MissionType::Defense,
            MissionType::Interception,
            MissionType::Disruption,
            MissionType::Survival,
            MissionType::Other,
        ] {
            assert_eq!(MissionType::from_stored(mission.as_str()), mission);
        }

        assert_eq!(EndReason::from_stored("crashed"), EndReason::Unterminated);
        assert_eq!(MissionType::from_stored("excavation"), MissionType::Other);
    }

    fn fixture(text: &str) -> Vec<Run> {
        parse_log(text.lines())
    }

    fn single(text: &str) -> Run {
        let mut runs = fixture(text);
        assert_eq!(runs.len(), 1, "expected exactly one run, got {runs:#?}");
        runs.remove(0)
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-3
    }

    #[test]
    fn completed_defense_run() {
        let run = single(include_str!(
            "../tests/fixtures/arbitration/stoefler-defense.txt"
        ));
        assert_eq!(run.node, "Stöfler (Lua)");
        assert_eq!(run.sol_node.as_deref(), Some("SolNode305"));
        assert_eq!(run.mission_type, MissionType::Defense);
        assert_eq!(run.mission_type_raw.as_deref(), Some("MT_DEFENSE"));
        assert_eq!(run.end_reason, EndReason::MissionEnd);
        assert_eq!(run.waves, 6);
        assert_eq!(run.rotations, 2);
        assert_eq!(run.waves_per_rotation, 3);
        assert_eq!(run.drone_kills, 3);
        // Four lancers plus the kubrow that spawned while loading, plus drones.
        assert_eq!(run.kills, 8);
        assert!(run.host_telemetry);
        // Wave 1 at 16786.896 to the last reward at 16936.816.
        assert!(close(run.duration_sec, 149.92), "{}", run.duration_sec);
        assert_eq!(
            run.started_at.map(|t| t.to_rfc3339()).as_deref(),
            Some("2026-07-08T02:17:25.750+00:00")
        );
        assert!(close(run.vitus.mean, 3.662), "{}", run.vitus.mean);
        assert!(close(run.vitus.per_minute, 3.662 / (149.92 / 60.0)));
    }

    #[test]
    fn aborted_run_then_non_arbitration_mission() {
        let run = single(include_str!(
            "../tests/fixtures/arbitration/oestrus-abort.txt"
        ));
        assert_eq!(run.node, "Oestrus (Eris)");
        assert_eq!(run.mission_name, "Oestrus (Eris) - Arbitration");
        assert_eq!(run.sol_node.as_deref(), Some("SolNode167"));
        assert_eq!(run.mission_type, MissionType::Other);
        assert_eq!(run.mission_type_raw.as_deref(), Some("MT_PURIFY"));
        assert_eq!(run.end_reason, EndReason::Aborted);
        assert_eq!(run.run_end_sec, Some(234.503));
        assert_eq!(run.rotations, 0);
        assert_eq!(run.drone_kills, 0);
        // The two flight agents never tick: decorative.
        assert_eq!(run.kills, 0);
        // OnStateStarted at 184.403 to the abort at 234.503.
        assert!(close(run.duration_sec, 50.1), "{}", run.duration_sec);
        assert_eq!(
            run.started_at.map(|t| t.to_rfc3339()).as_deref(),
            Some("2026-07-06T09:43:29.428+00:00")
        );
    }

    #[test]
    fn survival_rotations() {
        let run = single(include_str!(
            "../tests/fixtures/arbitration/mot-survival.txt"
        ));
        assert_eq!(run.node, "Mot (Void)");
        assert_eq!(run.mission_type, MissionType::Survival);
        assert_eq!(run.end_reason, EndReason::MissionEnd);
        assert_eq!(run.rotations, 1);
        assert_eq!(run.waves, 0);
        assert_eq!(run.drone_kills, 7);
        assert_eq!(run.kills, 7);
        // First drone at 434.607 to the last at 735.957; the in-mission
        // EndOfMatch screens in between do not end the run.
        assert!(close(run.duration_sec, 301.35), "{}", run.duration_sec);
    }

    // An interception where the survival reward UI popped
    // 25s before the real rotation reward.
    #[test]
    fn interception_ignores_stray_survival_reward() {
        let run = single(include_str!(
            "../tests/fixtures/arbitration/rhea-interception.txt"
        ));
        assert_eq!(run.mission_type, MissionType::Interception);
        assert_eq!(run.rotations, 1);
        assert_eq!(run.drone_kills, 5);
        // First point captured at 133.194 to the reward at 356.940.
        assert!(close(run.duration_sec, 223.746), "{}", run.duration_sec);
    }

    // The four fixtures above are the original project's own excerpts; the
    // join, mirror-defense and spam ones are composed from recorded line shapes.
    #[test]
    fn mid_mission_join_starts_at_the_join_point() {
        let run = single(include_str!(
            "../tests/fixtures/arbitration/apollo-join.txt"
        ));
        assert_eq!(run.node, "Apollo (Lua)");
        assert_eq!(run.sol_node.as_deref(), Some("SolNode308"));
        assert_eq!(run.run_start_sec, 2047.451);
        assert_eq!(run.mission_type, MissionType::Disruption);
        assert_eq!(run.waves_per_rotation, 4);
        // Rounds count; the survival reward UI disruption spams does not.
        assert_eq!(run.rotations, 2);
        assert_eq!(run.drone_kills, 3);
        assert_eq!(run.end_reason, EndReason::MissionEnd);
        // First round at 2060 to the last round done at 2600.
        assert!(close(run.duration_sec, 540.0), "{}", run.duration_sec);
        assert_eq!(
            run.started_at.map(|t| t.to_rfc3339()).as_deref(),
            Some("2026-07-08T18:45:09.451+00:00")
        );
    }

    #[test]
    fn mirror_defense_has_two_waves_per_rotation() {
        let run = single(include_str!(
            "../tests/fixtures/arbitration/tyana-mirror-defense.txt"
        ));
        assert_eq!(run.node, "Tyana Pass (Mars)");
        assert_eq!(run.mission_type, MissionType::Defense);
        assert_eq!(run.waves_per_rotation, 2);
        assert_eq!(run.waves, 4);
        assert_eq!(run.rotations, 2);
        assert_eq!(run.drone_kills, 2);
        assert!(close(run.duration_sec, 240.01), "{}", run.duration_sec);
        let (mean, _) = vitus_model(2, 2, 2);
        assert!(close(run.vitus.mean, mean));
        assert!(close(mean, 3.108), "{mean}");
    }

    #[test]
    fn spam_and_decorative_agents_are_excluded() {
        let run = single(include_str!(
            "../tests/fixtures/arbitration/casta-defense-spam.txt"
        ));
        assert_eq!(run.node, "Casta (Ceres)");
        // Three lancers and one drone. Skipped: the Game [Warning] and
        // DamagePct lines, the turret, and the crates whose tick never advances.
        assert_eq!(run.drone_kills, 1);
        assert_eq!(run.kills, 4);
        // Rewards at 355 and 360 are one rotation; the Sys [Error] mention of
        // the reward movie is not a reward; 400 is the second rotation.
        assert_eq!(run.rotations, 2);
        assert_eq!(run.waves, 1);
        assert!(close(run.duration_sec, 80.0), "{}", run.duration_sec);
    }

    #[test]
    fn non_arbitration_missions_produce_no_records() {
        let runs = fixture(
            "100.000 Script [Info]: ThemedSquadOverlay.lua: Pending mission: SolNode149\n\
             105.000 Script [Info]: ThemedSquadOverlay.lua: Mission name: Casta (Ceres)\n\
             110.000 Game [Info]: OnStateStarted, mission type=MT_DEFENSE\n\
             120.000 Script [Info]: WaveDefend.lua: Defense wave: 1\n\
             200.000 Sys [Info]: Created /Lotus/Interface/DefenseReward.swf\n\
             210.000 Sys [Info]: EOM missionLocationUnlocked=1\n\
             300.000 Script [Info]: Client loaded {\"name\":\"SolNode308\"} with MissionInfo:\n",
        );
        assert!(runs.is_empty(), "{runs:#?}");
    }

    #[test]
    fn elite_sector_marks_an_arbitration_whose_name_lacks_the_keyword() {
        let runs = fixture(
            "158.074 Script [Info]: ThemedSquadOverlay.lua: Pending mission: SolNode167_EliteAlert\n\
             178.428 Script [Info]: ThemedSquadOverlay.lua: Mission name: Oestrus (Eris)\n\
             234.503 Script [Info]: TopMenu.lua: Abort: host/no session\n\
             255.746 Script [Info]: ThemedSquadOverlay.lua: Mission name: Isos (Eris)\n",
        );
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].sol_node.as_deref(), Some("SolNode167"));
        assert_eq!(runs[0].end_reason, EndReason::Aborted);
    }

    #[test]
    fn back_to_back_arbitrations_and_host_migration_replay() {
        let mut parser = Parser::new();
        let name = |ts: &str, n: &str| {
            format!("{ts} Script [Info]: ThemedSquadOverlay.lua: Mission name: {n}")
        };
        assert!(matches!(
            parser.feed_line(&name("100.000", "Arbitration: Casta (Ceres)"))[..],
            [Event::RunStarted { .. }]
        ));
        parser.feed_line("500.000 AI [Info]: OnAgentCreated /Npc/CorpusEliteShieldDroneAgent7 MonitoredTicking 2");
        // Older timestamp after a migration: still the same run.
        assert!(parser
            .feed_line(&name("300.000", "Arbitration: Casta (Ceres)"))
            .is_empty());
        // The next arbitration's name line closes this run and opens the next.
        let events = parser.feed_line(&name("600.000", "Arbitration: Casta (Ceres)"));
        let [Event::RunEnded(first), Event::RunStarted { game_time_sec, .. }] = &events[..] else {
            panic!("{events:#?}");
        };
        assert_eq!(first.end_reason, EndReason::NewMission);
        assert_eq!(first.run_end_sec, Some(600.0));
        assert_eq!(*game_time_sec, 600.0);
        let last = parser.finish().expect("second run is open");
        assert_eq!(last.end_reason, EndReason::Unterminated);
        assert!(parser.finish().is_none());
    }

    #[test]
    fn live_events_are_emitted_as_lines_arrive() {
        let mut parser = Parser::new();
        parser.feed_line("100.000 Script [Info]: ThemedSquadOverlay.lua: Mission name: Arbitration: Hydron (Sedna)");
        assert_eq!(
            parser.feed_line("110.000 Script [Info]: WaveDefend.lua: Defense wave: 1"),
            vec![Event::WaveAdvanced(1)]
        );
        assert_eq!(
            parser.feed_line("120.000 AI [Info]: OnAgentCreated /Npc/CorpusEliteShieldDroneAgent7 MonitoredTicking 2"),
            vec![Event::AgentSpawned { drone: true }]
        );
        assert_eq!(
            parser.feed_line("200.000 Sys [Info]: Created /Lotus/Interface/DefenseReward.swf"),
            vec![Event::RotationAdvanced(1)]
        );
        assert!(parser.feed_line("210.000 Script [Info]: Dialog.lua: Dialog::CreateOkCancel(description=/Lotus/Language/Menu/AbortMissionConfirm)").is_empty());
        assert!(parser.is_run_active());
    }

    #[test]
    fn run_keeps_boot_time_from_its_start() {
        let runs = fixture(
            "0.000 Sys [Diag]: Current time: Mon Jul  6 17:46:29 2026 [UTC: Mon Jul  6 15:46:29 2026]\n\
             10.000 Script [Info]: ThemedSquadOverlay.lua: Mission name: Arbitration: Casta (Ceres)\n\
             20.000 Sys [Info]: EOM missionLocationUnlocked=1\n\
             0.000 Sys [Diag]: Current time: Tue Jul  7 17:46:29 2026 [UTC: Tue Jul  7 15:46:29 2026]\n\
             30.000 Script [Info]: ThemedSquadOverlay.lua: Mission name: Arbitration: Casta (Ceres)\n",
        );
        assert_eq!(runs.len(), 2);
        assert_eq!(
            runs[0].started_at.map(|t| t.to_rfc3339()).as_deref(),
            Some("2026-07-06T15:46:39+00:00")
        );
    }

    #[test]
    fn oversized_timestamp_does_not_panic() {
        let runs = fixture(
            "99999999999999999999.0 Script [Info]: ThemedSquadOverlay.lua: Mission name: Arbitration: Casta (Ceres)\n\
             100.000 Sys [Info]: EOM missionLocationUnlocked=1\n",
        );
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].started_at, None);
    }

    #[test]
    fn vitus_model_matches_the_reference_numbers() {
        assert_eq!(vitus_model(0, 3, 0), (0.0, 0.0));
        let (mean, std) = vitus_model(2, 3, 3);
        assert!(close(mean, 3.662), "{mean}");
        // rotation variance 1.62, drop-count variance 0.3825 scaled by the
        // squared drop mean 5.5696, drop-value variance 0.5904 scaled by 0.45
        assert!(close(std, (1.62 + 0.26568 + 2.130372f64).sqrt()), "{std}");
    }
}
