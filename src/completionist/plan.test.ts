// Run with: node --experimental-strip-types --test src/completionist/plan.test.ts

import assert from "node:assert/strict";
import { test } from "node:test";
import { addable, addToPlan, candidates, moved, planView, prefill, rangeText, ringFractions, ringTitle, snapshotIntrinsicTargets, summaryText } from "./plan.ts";
import { DEFAULT_CONTROLS } from "./suggestions.ts";
import type { MasteryPlan, MasteryTotal, Opportunity, PlanEvaluation } from "../types/mastery.ts";

const opportunity = (name: string, remaining_mastery: number | null, over: Partial<Opportunity> = {}): Opportunity => ({
  unique_name: `/Lotus/Weapons/Tenno/${name}`, name, category: "Primary", image_name: null, mastery_req: null,
  cap: 30, earned_rank: 0, remaining_mastery, state: "missing", unobtainable: null, excluded: false,
  stage: "acquire", action: "farm", owned: false, owned_level: null, build_completion_ms: null,
  vendors: [], spend: null, baro: null, craft: null, relic: null, drop: null, purchase: null, access: "available", blockers: [], route: null, ...over,
});

const paths = (list: Opportunity[]) => list.map(o => o.unique_name);

test("adding and prefilling a craft puts its owned ingredient first and counts its mastery once", () => {
  const bolto = opportunity("Bolto", 1800, { owned: true, action: "level", stage: "level_claim" });
  const akbolto = opportunity("Akbolto", 3000, { craft: {
    requirements: [], builds: [], credits: 25000, credits_short: 0,
    level_first: [{ unique_name: bolto.unique_name, name: "Bolto", gain: 1800 }],
  } });
  assert.deepEqual(addToPlan([], akbolto), paths([bolto, akbolto]));
  assert.deepEqual(addToPlan(paths([bolto]), akbolto), paths([bolto, akbolto]));
  assert.deepEqual(addToPlan(paths([bolto, akbolto]), akbolto), paths([bolto, akbolto]));
  const hek = opportunity("Hek", 3000);
  assert.deepEqual(prefill([akbolto, hek], 4800), paths([bolto, akbolto]), "ingredient may be outside filtered candidates");
  assert.deepEqual(prefill([akbolto, bolto, hek], 4801), paths([bolto, akbolto, hek]));
  assert.deepEqual(prefill([bolto, akbolto, hek], 4801), paths([bolto, akbolto, hek]));
});

test("prefill takes candidates in view order until the known gains cover the gap or run out", () => {
  const list = [opportunity("Hek", 3000), opportunity("Braton", 1800), opportunity("Skana", null), opportunity("Lato", 3000)];
  assert.deepEqual(prefill(list, 4000), paths(list.slice(0, 2)));
  assert.deepEqual(prefill(list, 4800), paths(list.slice(0, 2)), "exactly covered stops");
  // An unknown gain counts for nothing, so the walk goes past it.
  assert.deepEqual(prefill(list, 5000), paths(list));
  assert.deepEqual(prefill(list, 20_000), paths(list), "candidates run out before the gap is covered");
  assert.deepEqual(prefill(list, 0), []);
  assert.deepEqual(prefill([], 5000), []);
  // A spend contributes only what the banked points buy.
  const railjack = opportunity("Railjack", 7500, { action: "spend", spend: { ranks: 2, points: 3, mastery: 3000, tracks: [] } });
  assert.deepEqual(prefill([railjack, list[0]], 3500), [railjack.unique_name, list[0].unique_name]);
  // An unsourced row is never chosen, however much it would cover.
  const unsourced = opportunity("Argonak", 3000, { stage: "unsourced", action: "acquire", access: "unknown" });
  assert.deepEqual(prefill([unsourced, list[0]], 3000), [list[0].unique_name]);
  assert.deepEqual(prefill([unsourced], 3000), []);
});

test("candidates follow the chosen view with the shared filters, and the addable list leaves out what is already planned", () => {
  const relic = opportunity("Braton Prime", 3000, { relic: { parts: [], coverage: { kind: "complete", probability: 0.5 } } });
  const priced = opportunity("Lex Prime", 3000, { purchase: { parts: [], set: { slug: "lex_prime_set", name: "Lex Prime Set", price: 20, fetched_at: null }, missing_total: null, full_total: null, cheapest_finish: { platinum: 20, route: "set" }, full_purchase: { platinum: 20, route: "set" } } });
  const trade = opportunity("Prisma Gorgon", 3000, { action: "trade", purchase: { parts: [], set: { slug: "prisma_gorgon", name: "Prisma Gorgon", price: 90, fetched_at: null }, missing_total: null, full_total: null, cheapest_finish: { platinum: 90, route: "set" }, full_purchase: { platinum: 90, route: "set" } } });
  const list = [opportunity("Hek", 3000, { category: "Melee" }), relic, priced, trade];
  assert.deepEqual(paths(candidates(list, DEFAULT_CONTROLS, "suggestions", "")), paths([list[0], priced]));
  assert.deepEqual(paths(candidates(list, DEFAULT_CONTROLS, "relics", "")), [relic.unique_name]);
  assert.deepEqual(paths(candidates(list, DEFAULT_CONTROLS, "platinum", "")), paths([priced, trade]));
  assert.deepEqual(paths(candidates(list, { ...DEFAULT_CONTROLS, category: "Melee" }, "suggestions", "")), [list[0].unique_name]);
  for (const view of ["suggestions", "relics", "platinum"] as const) {
    const matching = view === "relics" ? relic : priced;
    assert.deepEqual(prefill(candidates(list, DEFAULT_CONTROLS, view, ` ${matching.name.toUpperCase()} `), 10_000), [matching.unique_name]);
    assert.deepEqual(prefill(candidates(list, DEFAULT_CONTROLS, view, "no match"), 10_000), []);
  }
  const plan: MasteryPlan = { target: 5, view: "suggestions", selections: [priced.unique_name], allowances: {} };
  assert.deepEqual(paths(addable(list, plan)), paths([list[0], relic, trade]));
  assert.equal(planView("platinum"), "platinum");
  assert.equal(planView("bogus"), "suggestions");
});

test("reordering swaps neighbours and stops at the ends", () => {
  assert.deepEqual(moved(["a", "b", "c"], 0, 1), ["b", "a", "c"]);
  assert.deepEqual(moved(["a", "b", "c"], 2, -1), ["a", "c", "b"]);
  assert.deepEqual(moved(["a", "b", "c"], 0, -1), ["a", "b", "c"]);
  assert.deepEqual(moved(["a", "b", "c"], 2, 1), ["a", "b", "c"]);
});

test("Intrinsic targets stay fixed until a selection is removed or regenerated", () => {
  const railjack = opportunity("Railjack", 3000, { action: "spend", spend: {
    ranks: 2, points: 3, mastery: 3000, tracks: [{ track: "Piloting", from: 0, to: 2 }],
  } });
  const plan: MasteryPlan = { target: 5, view: "suggestions", selections: [railjack.unique_name], allowances: {} };
  const intrinsic_targets = snapshotIntrinsicTargets(plan, [railjack]);
  assert.deepEqual(intrinsic_targets, { [railjack.unique_name]: { Piloting: 2 } });
  const saved = { ...plan, intrinsic_targets };
  const later = { ...railjack, spend: { ...railjack.spend!, tracks: [{ track: "Piloting", from: 2, to: 4 }] } };
  assert.deepEqual(snapshotIntrinsicTargets(saved, [later]), intrinsic_targets);
  assert.deepEqual(snapshotIntrinsicTargets(saved, []), intrinsic_targets, "completed selections keep their targets");
  const cleared = snapshotIntrinsicTargets({ ...saved, selections: [] }, [later]);
  assert.deepEqual(cleared, {});
  assert.deepEqual(snapshotIntrinsicTargets({ ...saved, intrinsic_targets: cleared }, [later]), { [railjack.unique_name]: { Piloting: 4 } });
  assert.deepEqual(snapshotIntrinsicTargets({ ...saved, intrinsic_targets: {} }, [later]), { [railjack.unique_name]: { Piloting: 4 } });
});

test("the summary distinguishes lower bounds from exact totals and the details show the range", () => {
  const total: MasteryTotal = { lower: 122_500, upper: 159_999, exact: null, rank: 7, rank_upper: 7 };
  const evaluation = (over: Partial<PlanEvaluation>): PlanEvaluation => ({
    entries: [], total, total_reason: "Intrinsics unconfirmed", target_xp: 160_000, gap: 37_500, gains: 9_000, unknown_gains: 0,
    projected: { lower: 131_500, upper: 168_999, exact: null, rank: 7, rank_upper: 8 }, rejected_allowances: [], ...over,
  });
  assert.equal(summaryText(evaluation({}), 8), "Total 122,500 (lower bound) · MR 8 needs 37,500 more · plan adds 9,000 · projected at least MR 7");
  assert.equal(rangeText(evaluation({})), "Total between 122,500 and 159,999 · projected between 131,500 and 168,999, MR 7 to 8 · target MR needs 160,000");
  assert.equal(summaryText(evaluation({ unknown_gains: 2 }), 8), "Total 122,500 (lower bound) · MR 8 needs 37,500 more · plan adds 9,000 and 2 unknown · projected at least MR 7");
  const exact = evaluation({ total: { ...total, exact: 147_200 }, total_reason: null, gap: 12_800, projected: { lower: 131_500, upper: 168_999, exact: 156_200, rank: 7, rank_upper: 8 } });
  assert.equal(summaryText(exact, 8), "Total 147,200 · MR 8 needs 12,800 more · plan adds 9,000 · projected MR 7");
  assert.equal(rangeText(exact), "Total 147,200 exactly · projected 156,200, MR 7 · target MR needs 160,000");
  const reached = evaluation({ gap: 0, gains: 40_000, projected: { lower: 162_500, upper: 199_999, exact: null, rank: 8, rank_upper: 8 } });
  assert.equal(summaryText(reached, 8), "Total 122,500 (lower bound) · MR 8 is reached · plan adds 40,000 · projected at least MR 8");
  assert.equal(summaryText(evaluation({ total: null, gap: null, projected: null }), 8), "Mastery Rank unknown, so the gap and projection are too.");
});

test("the rings take the lower bound and clamp the plan at the gap", () => {
  const total: MasteryTotal = { lower: 10_000, upper: 22_499, exact: null, rank: 2, rank_upper: 2 };
  const evaluation = (over: Partial<PlanEvaluation>): PlanEvaluation => ({
    entries: [], total, total_reason: null, target_xp: 40_000, gap: 30_000, gains: 15_000, unknown_gains: 0,
    projected: null, rejected_allowances: [], ...over,
  });
  assert.deepEqual(ringFractions(evaluation({})), { earned: 0.25, planned: 0.5, band: 0 });
  assert.deepEqual(ringFractions(evaluation({ gains: 30_000 })), { earned: 0.25, planned: 1, band: 0 });
  assert.deepEqual(ringFractions(evaluation({ gains: 45_000 })), { earned: 0.25, planned: 1, band: 0 });
  const exact = { ...total, exact: 20_000 };
  assert.deepEqual(ringFractions(evaluation({ total: exact, gap: 20_000, gains: 5_000 })), { earned: 0.5, planned: 0.25, band: 0.8 });
  // A target at or below the current rank is reached with nothing planned.
  assert.deepEqual(ringFractions(evaluation({ total: exact, target_xp: 10_000, gap: 0, gains: 0 })), { earned: 2, planned: 1, band: 0.8 });
  assert.equal(ringFractions(evaluation({ total: null, gap: null })), null);
});

test("the ring tooltip carries the four numbers and says when the band is unknown", () => {
  const total: MasteryTotal = { lower: 122_500, upper: 159_999, exact: null, rank: 7, rank_upper: 7 };
  const evaluation = (over: Partial<PlanEvaluation>): PlanEvaluation => ({
    entries: [], total, total_reason: "Intrinsics unconfirmed", target_xp: 160_000, gap: 37_500, gains: 9_000, unknown_gains: 0,
    projected: { lower: 131_500, upper: 168_999, exact: null, rank: 7, rank_upper: 8 }, rejected_allowances: [], ...over,
  });
  assert.equal(ringTitle(evaluation({}), 8), "Earned at least 122,500\nProjected at least 131,500\nMR 8 needs 160,000\nProgress into MR 7 unknown");
  const exact = evaluation({ total: { ...total, exact: 147_200 }, projected: { lower: 131_500, upper: 168_999, exact: 156_200, rank: 7, rank_upper: 8 } });
  assert.equal(ringTitle(exact, 8), "Earned 147,200\nProjected 156,200\nMR 8 needs 160,000\n24,700 of 37,500 into MR 7");
  assert.equal(ringTitle(evaluation({ total: null, projected: null }), 8), "Mastery Rank unknown: no scan yet");
});
