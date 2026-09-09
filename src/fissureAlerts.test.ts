// Run with: node --experimental-strip-types --test src/fissureAlerts.test.ts

import assert from "node:assert/strict";
import { test } from "node:test";
import { collectNewMatches, matchesWatch } from "./fissureAlerts.ts";
import type { FissureWatch } from "./types/settings.ts";
import type { SeenFissures, WorldState, WsFissure } from "./types/worldstate.ts";

const fissure = (id: string, tier = "Lith", missionType = "Capture"): WsFissure => ({
  id, tier, missionType, expiry: "2030-01-01T00:00:00Z", node: "Tessera (Void)",
  enemy: "Corpus", tierNum: 1, isStorm: false, isHard: false, active: true,
});

const watch = (id: string, tier = "Any", missionType = "Any"): FissureWatch =>
  ({ id, tier, missionType, variant: "any" });

const ws = (fissures: WsFissure[]): WorldState => ({ fissures } as WorldState);

test("watches restored from settings seed silently at launch", () => {
  const restored = new Set(["w1"]);
  const { fresh, live } = collectNewMatches(ws([fissure("a"), fissure("b")]), [watch("w1")], new Map(), restored);
  assert.deepEqual(fresh, []);
  assert.deepEqual([...live.get("w1")!], ["a", "b"]);
});

test("a restored watch still reports fissures that arrive after launch", () => {
  const restored = new Set(["w1"]);
  const { live } = collectNewMatches(ws([fissure("a")]), [watch("w1")], new Map(), restored);
  const { fresh } = collectNewMatches(ws([fissure("a"), fissure("b")]), [watch("w1")], live, restored);
  assert.deepEqual(fresh.map(m => m.f.id), ["b"]);
});

test("a watch absent from the restored set announces on its first poll", () => {
  const { fresh } = collectNewMatches(ws([fissure("a")]), [watch("w1")], new Map(), new Set(["other"]));
  assert.deepEqual(fresh.map(m => m.f.id), ["a"]);
});

test("only fissures unseen by that watch are reported", () => {
  const seen: SeenFissures = new Map([["w1", new Set(["a"])]]);
  const { fresh } = collectNewMatches(ws([fissure("a"), fissure("b")]), [watch("w1")], seen);
  assert.deepEqual(fresh.map(m => m.f.id), ["b"]);
});

test("a fissure matching two watches is reported once", () => {
  const seen: SeenFissures = new Map([["w1", new Set()], ["w2", new Set()]]);
  const { fresh } = collectNewMatches(ws([fissure("a")]), [watch("w1"), watch("w2", "Lith")], seen);
  assert.deepEqual(fresh.map(m => m.f.id), ["a"]);
});

test("a watch added at runtime announces what is already live", () => {
  const seen: SeenFissures = new Map([["w1", new Set(["a"])]]);
  const { fresh } = collectNewMatches(ws([fissure("a")]), [watch("w1"), watch("w2")], seen);
  assert.deepEqual(fresh.map(m => m.f.id), ["a"]);
});

test("the same fissure is never announced twice for one watch", () => {
  const watches = [watch("w1")];
  const worldState = ws([fissure("a")]);

  const first = collectNewMatches(worldState, watches, new Map());
  assert.deepEqual(first.fresh.map(m => m.f.id), ["a"]);

  // Every later poll still matches it, and must stay quiet for as long as it lives.
  let seen = first.live;
  for (let poll = 0; poll < 3; poll++) {
    const next = collectNewMatches(worldState, watches, seen);
    assert.deepEqual(next.fresh, []);
    seen = next.live;
  }
});

test("a fissure that expires and returns under a new id announces again", () => {
  const watches = [watch("w1")];
  const { live } = collectNewMatches(ws([fissure("a")]), watches, new Map());
  const { fresh } = collectNewMatches(ws([fissure("b")]), watches, live);
  assert.deepEqual(fresh.map(m => m.f.id), ["b"]);
});

test("expired fissures and removed watches drop out of the returned state", () => {
  const seen: SeenFissures = new Map([["w1", new Set(["a", "gone"])], ["removed", new Set(["a"])]]);
  const { live } = collectNewMatches(ws([fissure("a")]), [watch("w1")], seen);
  assert.deepEqual([...live.keys()], ["w1"]);
  assert.deepEqual([...live.get("w1")!], ["a"]);
});

test("entries with no id are ignored rather than sharing one key", () => {
  const seen: SeenFissures = new Map([["w1", new Set()]]);
  const { fresh, live } = collectNewMatches(ws([fissure(""), fissure("")]), [watch("w1")], seen);
  assert.deepEqual(fresh, []);
  assert.equal(live.get("w1")!.size, 0);
});

test("Omnia matches both ways except against Requiem", () => {
  const omniaWatch = watch("w", "Omnia");
  assert.equal(matchesWatch(omniaWatch, fissure("a", "Axi"), "normal"), true);
  assert.equal(matchesWatch(omniaWatch, fissure("a", "Requiem"), "normal"), false);
  assert.equal(matchesWatch(watch("w", "Axi"), fissure("a", "Omnia"), "normal"), true);
  assert.equal(matchesWatch(watch("w", "Requiem"), fissure("a", "Omnia"), "normal"), false);
});

test("a variant-specific watch ignores the other variants", () => {
  const hardWatch: FissureWatch = { id: "w", tier: "Any", missionType: "Any", variant: "hard" };
  assert.equal(matchesWatch(hardWatch, fissure("a"), "hard"), true);
  assert.equal(matchesWatch(hardWatch, fissure("a"), "normal"), false);
  assert.equal(matchesWatch(hardWatch, fissure("a"), "storm"), false);
});

test("mission type must match exactly unless the watch says Any", () => {
  assert.equal(matchesWatch(watch("w", "Any", "Capture"), fissure("a", "Lith", "Capture"), "normal"), true);
  assert.equal(matchesWatch(watch("w", "Any", "Capture"), fissure("a", "Lith", "Rescue"), "normal"), false);
  assert.equal(matchesWatch(watch("w"), fissure("a", "Lith", "Rescue"), "normal"), true);
});

test("a Steel Path fissure only reaches a watch that accepts hard", () => {
  const seen: SeenFissures = new Map([["w1", new Set()]]);
  const worldState = { spFissures: [fissure("sp")] } as unknown as WorldState;
  const hardWatch: FissureWatch = { id: "w1", tier: "Any", missionType: "Any", variant: "hard" };
  assert.deepEqual(collectNewMatches(worldState, [hardWatch], seen).fresh.map(m => m.variant), ["hard"]);

  const stormWatch: FissureWatch = { id: "w1", tier: "Any", missionType: "Any", variant: "storm" };
  assert.deepEqual(collectNewMatches(worldState, [stormWatch], seen).fresh, []);
});

test("an empty worldstate notifies nothing and seeds an empty set", () => {
  const seen: SeenFissures = new Map([["w1", new Set(["a"])]]);
  const { fresh, live } = collectNewMatches({} as WorldState, [watch("w1")], seen);
  assert.deepEqual(fresh, []);
  assert.equal(live.get("w1")!.size, 0);
});
