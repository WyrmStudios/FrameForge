
import assert from "node:assert/strict";
import { test } from "node:test";
import { filterRuns, summarize, type RunRecord } from "./arbitrationAnalytics.ts";

const DAY = 86_400_000;
const NOW = Date.UTC(2026, 8, 4, 12);

let seq = 0;
const run = (over: Partial<RunRecord> = {}): RunRecord => ({
  uid: `run-${seq++}`,
  started_at: new Date(NOW - DAY).toISOString(),
  node: "Mot (Void)",
  tier: null,
  mission_type: "survival",
  end_reason: "mission_end",
  duration_sec: 600,
  rotations: 2,
  waves: 0,
  kills: 300,
  drone_kills: 10,
  vitus_mean: 20,
  vitus_per_minute: 2,
  ...over,
});

const daysAgo = (days: number) => new Date(NOW - days * DAY).toISOString();

test("rates come from completed runs only; totals count every run", () => {
  const s = summarize([
    run({ duration_sec: 600, vitus_mean: 20, kills: 100 }),
    run({ duration_sec: 1200, vitus_mean: 60, kills: 200 }),
    run({ end_reason: "aborted", duration_sec: 6000, vitus_mean: 1000, kills: 50 }),
    run({ end_reason: "new_mission", duration_sec: 60, vitus_mean: 0, kills: 7 }),
  ]);
  assert.equal(s.runs, 4);
  assert.equal(s.incomplete, 2);
  assert.equal(s.playtimeSec, 7860);
  assert.equal(s.kills, 357);
  assert.equal(s.vitus, 80);
  assert.equal(s.perMinute, 80 / 30);
  assert.deepEqual(s.rate, [2, 2]);
});

test("empty history has no rate, not a division by zero", () => {
  const s = summarize([]);
  assert.equal(s.runs, 0);
  assert.equal(s.perMinute, null);
  assert.deepEqual(s.rate, []);
  assert.deepEqual(s.byNode, []);
  assert.equal(summarize([run({ end_reason: "aborted" })]).perMinute, null);
});

test("rate series is chronological even when runs arrive newest first", () => {
  const s = summarize([
    run({ started_at: daysAgo(1), vitus_per_minute: 3 }),
    run({ started_at: daysAgo(3), vitus_per_minute: 1 }),
    run({ started_at: daysAgo(2), vitus_per_minute: 2 }),
  ]);
  assert.deepEqual(s.rate, [1, 2, 3]);
});

test("breakdowns group by node and by mission type, best rate first", () => {
  const s = summarize([
    run({ node: "Mot (Void)", mission_type: "survival", duration_sec: 600, vitus_mean: 10 }),
    run({ node: "Mot (Void)", mission_type: "survival", end_reason: "aborted", vitus_mean: 999 }),
    run({ node: "Stöfler (Lua)", mission_type: "defense", duration_sec: 600, vitus_mean: 30 }),
    run({ node: "Casta (Ceres)", mission_type: "defense", end_reason: "unterminated" }),
  ]);
  assert.deepEqual(
    s.byNode.map(b => [b.key, b.runs, b.completed, b.vitus, b.perMinute]),
    [["Stöfler (Lua)", 1, 1, 30, 3], ["Mot (Void)", 2, 1, 10, 1], ["Casta (Ceres)", 1, 0, 0, null]],
  );
  assert.deepEqual(
    s.byMissionType.map(b => [b.key, b.runs, b.perMinute]),
    [["defense", 2, 3], ["survival", 2, 1]],
  );
});

test("date range and mission type filters combine", () => {
  const runs = [
    run({ uid: "old-survival", started_at: daysAgo(40) }),
    run({ uid: "new-survival", started_at: daysAgo(2) }),
    run({ uid: "new-defense", started_at: daysAgo(2), mission_type: "defense" }),
    run({ uid: "no-clock", started_at: null }),
  ];
  const ids = (days: number | "all", missionType: "all" | "survival" | "defense") =>
    filterRuns(runs, { days, missionType }, NOW).map(r => r.uid);

  assert.deepEqual(ids("all", "all"), ["old-survival", "new-survival", "new-defense", "no-clock"]);
  assert.deepEqual(ids(30, "all"), ["new-survival", "new-defense"]);
  assert.deepEqual(ids(30, "survival"), ["new-survival"]);
  assert.deepEqual(ids("all", "defense"), ["new-defense"]);
  assert.deepEqual(ids(7, "defense"), ["new-defense"]);
});
