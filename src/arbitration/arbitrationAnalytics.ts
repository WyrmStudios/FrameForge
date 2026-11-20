import type { Tier } from "./arbitrationTiers.ts";

export type MissionType = "defense" | "interception" | "disruption" | "survival" | "other";
export type EndReason = "mission_end" | "aborted" | "new_mission" | "unterminated";

export interface RunRecord {
  uid: string;
  /// Null for an unrated node, and for a run recorded without the star chart
  /// node it happened on.
  tier: Tier | null;
  /// RFC-3339 UTC; null when the log had no boot-time header.
  started_at: string | null;
  node: string;
  mission_type: MissionType;
  end_reason: EndReason;
  duration_sec: number;
  rotations: number;
  waves: number;
  kills: number;
  drone_kills: number;
  vitus_mean: number;
  vitus_per_minute: number;
}

export const MISSION_TYPES: MissionType[] = ["defense", "interception", "disruption", "survival", "other"];

/// Only a run the mission itself ended counts toward rates. An abort, a host
/// migration into a new mission, or a log that ended mid-run all cut the
/// combat window short, and a partial window says nothing about a full run's
/// rate.
export const completed = (run: RunRecord) => run.end_reason === "mission_end";

export interface Filters {
  days: number | "all";
  missionType: MissionType | "all";
}

/// A run without a wall clock is outside every date range but "all", as in
/// the database query that stored it.
export function filterRuns(runs: RunRecord[], filters: Filters, now = Date.now()): RunRecord[] {
  const cutoff = filters.days === "all" ? null : new Date(now - filters.days * 86_400_000).toISOString();
  return runs.filter(run =>
    (filters.missionType === "all" || run.mission_type === filters.missionType)
    && (cutoff === null || (run.started_at !== null && run.started_at >= cutoff)),
  );
}

export interface Breakdown {
  key: string;
  runs: number;
  completed: number;
  vitus: number;
  /// Vitus per minute over the combined combat time of completed runs,
  /// so a long run weighs more than a short one; null with no completed run.
  perMinute: number | null;
}

export interface Summary {
  runs: number;
  /// Every run the mission itself did not end: aborted, left for another
  /// mission, or cut off by the end of the log.
  incomplete: number;
  playtimeSec: number;
  kills: number;
  vitus: number;
  perMinute: number | null;
  rate: number[];
  byNode: Breakdown[];
  byMissionType: Breakdown[];
}

function breakdown(key: string, runs: RunRecord[]): Breakdown {
  const done = runs.filter(completed);
  const vitus = done.reduce((s, r) => s + r.vitus_mean, 0);
  const minutes = done.reduce((s, r) => s + r.duration_sec, 0) / 60;
  return {
    key,
    runs: runs.length,
    completed: done.length,
    vitus,
    perMinute: minutes > 0 ? vitus / minutes : null,
  };
}

function groupBy(runs: RunRecord[], key: (r: RunRecord) => string): Breakdown[] {
  const groups = new Map<string, RunRecord[]>();
  for (const run of runs) {
    const k = key(run);
    const group = groups.get(k);
    if (group) group.push(run);
    else groups.set(k, [run]);
  }
  return [...groups]
    .map(([k, rs]) => breakdown(k, rs))
    .sort((a, b) => (b.perMinute ?? -1) - (a.perMinute ?? -1) || b.runs - a.runs);
}

// Runs without a wall clock go last, as the stored list orders them.
const byStart = (a: RunRecord, b: RunRecord) => {
  if (a.started_at === null || b.started_at === null) return Number(a.started_at === null) - Number(b.started_at === null);
  return a.started_at < b.started_at ? -1 : a.started_at > b.started_at ? 1 : 0;
};

export function summarize(runs: RunRecord[]): Summary {
  const all = breakdown("all", runs);
  return {
    runs: runs.length,
    incomplete: runs.length - all.completed,
    playtimeSec: runs.reduce((s, r) => s + r.duration_sec, 0),
    kills: runs.reduce((s, r) => s + r.kills, 0),
    vitus: all.vitus,
    perMinute: all.perMinute,
    rate: runs
      .filter(completed)
      .sort(byStart)
      .map(r => r.vitus_per_minute),
    byNode: groupBy(runs, r => r.node),
    byMissionType: groupBy(runs, r => r.mission_type),
  };
}
