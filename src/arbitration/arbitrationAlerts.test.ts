
import assert from "node:assert/strict";
import { test } from "node:test";
import {
  clampLead, dueAlerts, occurrenceKey, runAlertPass, DEFAULT_LEAD_MINS, EVAL_INTERVAL_MS,
  MAX_LEAD_MINS, MIN_LEAD_MINS, MIN_REMAINING_SECS, type ScheduleEntry,
} from "./arbitrationAlerts.ts";
import type { Tier } from "./arbitrationTiers.ts";

const HOUR = 3600;

const entry = (start: number, node_id = "SolNode1", tier: Tier | null = null): ScheduleEntry => ({
  start, end: start + HOUR, node_id,
  node: "Tessera", region: "Void", mission_type: "Defense", faction: "Corpus", tier,
});

const due = (r: { due: ScheduleEntry[] }) => r.due.map(e => e.node_id);

const persistAll = (r: { due: ScheduleEntry[]; kept: string[] }) => [...r.kept, ...r.due.map(occurrenceKey)];

const raiseAll = async () => true;
const raiseNone = async () => false;

test("a favorited hour alerts once the lead window opens", () => {
  const entries = [entry(10 * HOUR)];
  assert.deepEqual(due(dueAlerts(entries, { favorites: ["SolNode1"] }, 10, [], 10 * HOUR - 601)), []);
  assert.deepEqual(due(dueAlerts(entries, { favorites: ["SolNode1"] }, 10, [], 10 * HOUR - 600)), ["SolNode1"]);
  assert.deepEqual(due(dueAlerts(entries, { favorites: ["SolNode1"] }, 10, [], 10 * HOUR - 1)), ["SolNode1"]);
});

test("an hour whose lead window was slept through still alerts", () => {
  const entries = [entry(10 * HOUR)];
  assert.deepEqual(due(dueAlerts(entries, { favorites: ["SolNode1"] }, 10, [], 10 * HOUR)), ["SolNode1"]);
  assert.deepEqual(due(dueAlerts(entries, { favorites: ["SolNode1"] }, 10, [], 10 * HOUR + 30 * 60)), ["SolNode1"]);
});

test("an hour too far gone to join stays quiet", () => {
  const entries = [entry(10 * HOUR)];
  const lastUseful = 11 * HOUR - MIN_REMAINING_SECS;
  assert.deepEqual(due(dueAlerts(entries, { favorites: ["SolNode1"] }, 10, [], lastUseful - 1)), ["SolNode1"]);
  assert.deepEqual(due(dueAlerts(entries, { favorites: ["SolNode1"] }, 10, [], lastUseful)), []);
  assert.deepEqual(due(dueAlerts(entries, { favorites: ["SolNode1"] }, 10, [], 11 * HOUR)), []);
});

test("an hour alerted before it started is not alerted again once it runs", () => {
  const entries = [entry(10 * HOUR)];
  const fired = persistAll(dueAlerts(entries, { favorites: ["SolNode1"] }, 10, [], 10 * HOUR - 600));
  assert.deepEqual(due(dueAlerts(entries, { favorites: ["SolNode1"] }, 10, fired, 10 * HOUR + 60)), []);
  assert.deepEqual(due(dueAlerts(entries, { favorites: ["SolNode1"] }, 10, fired, 10 * HOUR + 30 * 60)), []);
});

test("nothing alerts without a favorite", () => {
  const entries = [entry(10 * HOUR), entry(11 * HOUR, "SolNode2")];
  const result = dueAlerts(entries, { favorites: [] }, 60, [], 10 * HOUR - 60);
  assert.deepEqual(result.due, []);
  assert.deepEqual(result.kept, []);
});

test("an unfavorited node in the same window stays quiet", () => {
  const entries = [entry(10 * HOUR, "SolNode1"), entry(10 * HOUR, "SolNode2")];
  assert.deepEqual(due(dueAlerts(entries, { favorites: ["SolNode2"] }, 10, [], 10 * HOUR - 60)), ["SolNode2"]);
});

test("an occurrence already fired is not repeated", () => {
  const entries = [entry(10 * HOUR)];
  const first = dueAlerts(entries, { favorites: ["SolNode1"] }, 10, [], 10 * HOUR - 600);
  assert.deepEqual(due(first), ["SolNode1"]);

  let fired = persistAll(first);
  for (let tick = 1; tick <= 5; tick++) {
    const next = dueAlerts(entries, { favorites: ["SolNode1"] }, 10, fired, 10 * HOUR - 600 + tick * 60);
    assert.deepEqual(next.due, []);
    fired = persistAll(next);
  }
});

test("fired state survives a restart", () => {
  const entries = [entry(10 * HOUR)];
  const fired = persistAll(dueAlerts(entries, { favorites: ["SolNode1"] }, 10, [], 10 * HOUR - 600));
  assert.deepEqual(dueAlerts(entries, { favorites: ["SolNode1"] }, 10, [...fired], 10 * HOUR - 300).due, []);
});

test("the same node alerts again on its next occurrence", () => {
  const entries = [entry(10 * HOUR), entry(40 * HOUR)];
  const fired = persistAll(dueAlerts(entries, { favorites: ["SolNode1"] }, 10, [], 10 * HOUR - 60));
  assert.deepEqual(due(dueAlerts(entries, { favorites: ["SolNode1"] }, 10, fired, 40 * HOUR - 60)), ["SolNode1"]);
});

test("keys outlive the start they name, then are pruned", () => {
  const entries = [entry(40 * HOUR)];
  const stale = [`${10 * HOUR}@SolNode1`, `${40 * HOUR}@SolNode1`];

  // Still inside the hour it names, so it has to survive: the occurrence can
  // yet alert as a catch-up, and a dropped key would let it alert twice.
  assert.deepEqual(dueAlerts(entries, { favorites: ["SolNode1"] }, 10, stale, 10 * HOUR + 60).kept, stale);
  assert.deepEqual(dueAlerts(entries, { favorites: ["SolNode1"] }, 10, stale, 20 * HOUR).kept, [`${40 * HOUR}@SolNode1`]);
});

test("a key that parses to no number is dropped rather than kept forever", () => {
  const stale = ["Infinity@SolNode1", "garbage", "@SolNode1"];
  assert.deepEqual(dueAlerts([], { favorites: ["SolNode1"] }, 10, stale, 20 * HOUR).kept, []);
});

test("favoriting mid-window alerts on the next tick", () => {
  const entries = [entry(10 * HOUR)];
  const quiet = dueAlerts(entries, { favorites: [] }, 10, [], 10 * HOUR - 300);
  assert.deepEqual(due(dueAlerts(entries, { favorites: ["SolNode1"] }, 10, persistAll(quiet), 10 * HOUR - 240)), ["SolNode1"]);
});

test("a longer lead reaches an hour a shorter one does not", () => {
  const entries = [entry(10 * HOUR)];
  const now = 10 * HOUR - 30 * 60;
  assert.deepEqual(due(dueAlerts(entries, { favorites: ["SolNode1"] }, 10, [], now)), []);
  assert.deepEqual(due(dueAlerts(entries, { favorites: ["SolNode1"] }, 45, [], now)), ["SolNode1"]);
});

test("lead is clamped into range, so a zero or negative lead cannot fire late", () => {
  assert.equal(clampLead(0), MIN_LEAD_MINS);
  assert.equal(clampLead(-30), MIN_LEAD_MINS);
  assert.equal(clampLead(9999), MAX_LEAD_MINS);
  assert.equal(clampLead(NaN), DEFAULT_LEAD_MINS);
  assert.equal(clampLead(10.4), 10);

  const entries = [entry(10 * HOUR)];
  assert.deepEqual(due(dueAlerts(entries, { favorites: ["SolNode1"] }, 0, [], 10 * HOUR - 120)), []);
  assert.deepEqual(due(dueAlerts(entries, { favorites: ["SolNode1"] }, 0, [], 10 * HOUR - 60)), ["SolNode1"]);
});

test("the shortest lead is still offered on several consecutive ticks", () => {
  const entries = [entry(10 * HOUR)];
  const tickSecs = EVAL_INTERVAL_MS / 1000;
  const windowSecs = MIN_LEAD_MINS * 60;

  // One delayed timer must not be able to step over the narrowest window, so
  // more than one tick has to land inside it.
  let offered = 0;
  for (let t = 0; t * tickSecs < windowSecs; t++) {
    const now = 10 * HOUR - windowSecs + t * tickSecs;
    assert.deepEqual(due(dueAlerts(entries, { favorites: ["SolNode1"] }, MIN_LEAD_MINS, [], now)), ["SolNode1"]);
    offered++;
  }
  assert.ok(offered >= 2, `narrowest window fits only ${offered} evaluations`);
});

test("only occurrences inside the window join the fired state", () => {
  const entries = [entry(10 * HOUR), entry(20 * HOUR)];
  assert.deepEqual(persistAll(dueAlerts(entries, { favorites: ["SolNode1"] }, 10, [], 10 * HOUR - 60)),
    [`${10 * HOUR}@SolNode1`]);
});

test("a raised alert joins the persisted state", async () => {
  const entries = [entry(10 * HOUR)];
  assert.deepEqual(await runAlertPass(entries, { favorites: ["SolNode1"] }, 10, [], 10 * HOUR - 600, raiseAll),
    [`${10 * HOUR}@SolNode1`]);
});

test("an alert that could not be raised is not recorded, and comes back", async () => {
  const entries = [entry(10 * HOUR)];
  assert.equal(await runAlertPass(entries, { favorites: ["SolNode1"] }, 10, [], 10 * HOUR - 600, raiseNone), null);

  const seen: string[] = [];
  await runAlertPass(entries, { favorites: ["SolNode1"] }, 10, [], 10 * HOUR - 540, async e => {
    seen.push(e.node_id);
    return true;
  });
  assert.deepEqual(seen, ["SolNode1"]);
});

test("only the occurrences that were raised are recorded", async () => {
  const entries = [entry(10 * HOUR, "SolNode1"), entry(10 * HOUR, "SolNode2")];
  assert.deepEqual(
    await runAlertPass(entries, { favorites: ["SolNode1", "SolNode2"] }, 10, [], 10 * HOUR - 600,
      async e => e.node_id === "SolNode1"),
    [`${10 * HOUR}@SolNode1`]);
});

test("a pass that changes nothing reports nothing to save", async () => {
  const entries = [entry(10 * HOUR)];
  const fired = [`${10 * HOUR}@SolNode1`];
  assert.equal(await runAlertPass(entries, { favorites: ["SolNode1"] }, 10, fired, 10 * HOUR - 300, raiseAll), null);
});

test("a pass that only prunes still reports the pruned state", async () => {
  assert.deepEqual(await runAlertPass([], { favorites: [] }, 10, [`${8 * HOUR}@SolNode1`], 10 * HOUR, raiseAll), []);
});


test("an hour alerts on its tier without the node being favorited", () => {
  const entries = [entry(10 * HOUR, "SolNode1", "S")];
  assert.deepEqual(due(dueAlerts(entries, { tiers: ["S"] }, 10, [], 10 * HOUR - 600)), ["SolNode1"]);
  assert.deepEqual(due(dueAlerts(entries, { tiers: ["A"] }, 10, [], 10 * HOUR - 600)), []);
});

test("a rule matching both ways raises the hour once", () => {
  const entries = [entry(10 * HOUR, "SolNode1", "S")];
  const rule = { favorites: ["SolNode1"], tiers: ["S"] as const };
  const first = dueAlerts(entries, rule, 10, [], 10 * HOUR - 600);
  assert.deepEqual(due(first), ["SolNode1"]);
  assert.deepEqual(due(dueAlerts(entries, rule, 10, persistAll(first), 10 * HOUR - 540)), []);
});

test("an unrated node is alertable as its own tier, not as every tier", () => {
  const entries = [entry(10 * HOUR, "SolNode1", null)];
  assert.deepEqual(due(dueAlerts(entries, { tiers: ["unrated"] }, 10, [], 10 * HOUR - 600)), ["SolNode1"]);
  assert.deepEqual(due(dueAlerts(entries, { tiers: ["S", "A", "B", "C", "D"] }, 10, [], 10 * HOUR - 600)), []);
});

test("an empty rule alerts nothing at all", () => {
  const entries = [entry(10 * HOUR, "SolNode1", "S")];
  assert.deepEqual(due(dueAlerts(entries, {}, 10, [], 10 * HOUR - 600)), []);
});

test("a tier alert obeys the same lead and catch-up limits as a favorite", () => {
  const entries = [entry(10 * HOUR, "SolNode1", "S")];
  const rule = { tiers: ["S"] as const };
  assert.deepEqual(due(dueAlerts(entries, rule, 10, [], 10 * HOUR - 601)), []);
  assert.deepEqual(due(dueAlerts(entries, rule, 10, [], 11 * HOUR - MIN_REMAINING_SECS)), []);
});
