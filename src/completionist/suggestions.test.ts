// Run with: node --experimental-strip-types --test src/completionist/suggestions.test.ts

import assert from "node:assert/strict";
import { test } from "node:test";
import {
  acquisitionLines, actionText, activePreset, alsoNeedsText, chanceText, costText, DEFAULT_CONTROLS, inSuggestions, parseControls, quoteText, PRESETS, rankPurchases, readyText, remainingText, shownControls, sourceLines, visibleOpportunities, visiblePurchases,
} from "./suggestions.ts";
import { RELIC_SUGGESTION_THRESHOLD } from "../constants/relics.ts";
import { blockerText } from "../constants/blockers.ts";
import { routeText, SOURCE_UNKNOWN } from "../constants/routes.ts";
import type { MasteryControls, Preset } from "./suggestions.ts";
import type { Coverage, DropPart, Listing, Opportunity, PartListing, Purchase, RelicPart, RouteKind } from "../types/mastery.ts";

const opportunity = (name: string, over: Partial<Opportunity> = {}): Opportunity => ({
  unique_name: `/Lotus/Weapons/Tenno/${name}`, name, category: "Primary", image_name: null, mastery_req: null,
  cap: 30, earned_rank: 12, remaining_mastery: 1800, state: "partial", unobtainable: null, excluded: false,
  stage: "level_claim", action: "level", owned: true, owned_level: 12, build_completion_ms: null,
  forma: null, vendors: [], spend: null, baro: null, craft: null, relic: null, drop: null, purchase: null, access: "available", blockers: [], route: null, ...over,
});

const NOW = 1_700_000_000;

const listing = (name: string, price: number | null, fetched_at: number | null = NOW - 120): Listing =>
  ({ slug: name.toLowerCase().replace(/ /g, "_"), name, price, fetched_at });

const part = (name: string, price: number | null, needed: number, short: number, fetched_at?: number | null): PartListing =>
  ({ ...listing(name, price, fetched_at), unique_name: `/Lotus/Types/Recipes/${name}`, needed, short });

/** Braton Prime is short two of three parts, at 25p and 30p, against a 40p set. */
const bratonPrime = (): Purchase => ({
  parts: [part("Braton Prime Blueprint", 25, 1, 1), part("Braton Prime Barrel", 15, 1, 0, null), part("Braton Prime Receiver", 30, 1, 1, NOW - 7 * 3600)],
  set: listing("Braton Prime Set", 40),
  missing_total: 55, full_total: 70,
  cheapest_finish: { platinum: 40, route: "set" }, full_purchase: { platinum: 40, route: "set" },
});

const buyer = (name: string, purchase: Purchase | null, over: Partial<Opportunity> = {}) =>
  opportunity(name, { stage: "acquire", action: "farm", owned: false, owned_level: null, state: "missing", earned_rank: 0, remaining_mastery: 3000, purchase, ...over });

test("comparisons rank differently and leave unpriced candidates last", () => {
  const braton = buyer("Braton Prime", bratonPrime());
  // The parts win the finish at 20p, and the set wins the full purchase at 60p.
  const lex = buyer("Lex Prime", {
    ...bratonPrime(), set: listing("Lex Prime Set", 60), missing_total: 20, full_total: 70,
    cheapest_finish: { platinum: 20, route: "parts" }, full_purchase: { platinum: 60, route: "set" },
  }, { remaining_mastery: 3000 });
  // Prisma Gorgon is cheaper than the Braton set but carries half the mastery.
  const gorgon = buyer("Prisma Gorgon", {
    parts: [], set: listing("Prisma Gorgon", 30), missing_total: null, full_total: null,
    cheapest_finish: { platinum: 30, route: "set" }, full_purchase: { platinum: 30, route: "set" },
  }, { action: "trade", remaining_mastery: 1500 });
  const unpriced = buyer("Akbolto Prime", {
    parts: [part("Akbolto Prime Link", null, 1, 1, null)], set: listing("Akbolto Prime Set", null, null), missing_total: null, full_total: null,
    cheapest_finish: null, full_purchase: null,
  });
  const noPurchase = buyer("Braton", null);
  const list = [unpriced, gorgon, braton, noPurchase, lex];
  const names = (o: Opportunity[]) => o.map(x => x.name);
  assert.deepEqual(names(rankPurchases(list, "cheapest")), ["Lex Prime", "Prisma Gorgon", "Braton Prime", "Akbolto Prime"]);
  assert.deepEqual(names(rankPurchases(list, "full")), ["Prisma Gorgon", "Braton Prime", "Lex Prime", "Akbolto Prime"]);
  assert.deepEqual(names(rankPurchases(list, "per_platinum")), ["Lex Prime", "Braton Prime", "Prisma Gorgon", "Akbolto Prime"]);
  // Unknown remaining mastery has no rate, so it drops behind a priced candidate with a rate.
  const unknownGain = buyer("Zenith", bratonPrime(), { remaining_mastery: null });
  assert.deepEqual(names(rankPurchases([unknownGain, gorgon], "per_platinum")), ["Prisma Gorgon", "Zenith"]);
  assert.deepEqual(names(rankPurchases([unknownGain, gorgon], "cheapest")), ["Prisma Gorgon", "Zenith"]);
  // Whole items only players sell stay out of Suggestions.
  assert.deepEqual(names(visibleOpportunities(list, DEFAULT_CONTROLS, "")), ["Akbolto Prime", "Braton Prime", "Braton", "Lex Prime"]);
  assert.equal(parseControls(JSON.stringify({ comparison: "full" })).comparison, "full");
  assert.equal(parseControls(JSON.stringify({ comparison: "bogus" })).comparison, "cheapest");
});

test("cost, quote age and what platinum does not cover are spelled out", () => {
  const braton = buyer("Braton Prime", bratonPrime(), {
    craft: { requirements: [
      { unique_name: "/Lotus/Types/Recipes/Braton Prime Receiver", name: "Braton Prime Receiver", needed: 1, from_stock: 0, short: 1 },
      { unique_name: "/Lotus/Types/Items/MiscItems/Ferrite", name: "Ferrite", needed: 100, from_stock: 0, short: 100 },
    ], builds: [], credits: 20_000, credits_short: 0 },
  });
  assert.equal(costText(braton, "cheapest"), "40p · complete set");
  assert.equal(costText(braton, "full"), "40p · complete set");
  assert.equal(costText(braton, "per_platinum"), "40p · complete set · 75 mastery/p");
  const parts = { ...bratonPrime(), cheapest_finish: { platinum: 55, route: "parts" as const }, full_purchase: { platinum: 70, route: "parts" as const } };
  assert.equal(costText(buyer("Braton Prime", parts), "cheapest"), "55p · 2 parts");
  assert.equal(costText(buyer("Braton Prime", parts), "full"), "70p · 3 parts");
  assert.equal(costText(buyer("Akbolto Prime", { ...parts, cheapest_finish: null }), "cheapest"), "Unpriced");
  assert.equal(costText(buyer("Prisma Gorgon", { ...parts, parts: [], cheapest_finish: { platinum: 90, route: "set" } }, { action: "trade" }), "cheapest"), "90p · whole item");
  assert.equal(actionText(buyer("Prisma Gorgon", null, { action: "trade" })), "Buy from players");

  assert.equal(quoteText(listing("Braton Prime Set", 40), NOW), "40p · 2m ago");
  assert.equal(quoteText(listing("Braton Prime Barrel", 15, null), NOW), "15p · age unknown");
  assert.equal(quoteText(listing("Braton Prime Barrel", null, NOW), NOW), "not listed");
  assert.equal(quoteText(listing("Braton Prime Barrel", null, null), NOW), "no quote yet");

  assert.equal(alsoNeedsText(braton), "Credits 20,000 · Ferrite ×100 · Weapon slot");
  assert.equal(alsoNeedsText(buyer("Prisma Gorgon", null, { action: "trade" })), "Weapon slot");
  assert.equal(alsoNeedsText(buyer("Frost Prime", null, { category: "Warframes", craft: { requirements: [], builds: [], credits: null, credits_short: 0 } })), "Credits unknown · Warframe slot");
});

const relicFarm = (name: string, coverage: Coverage, parts: RelicPart[] = [], over: Partial<Opportunity> = {}): Opportunity =>
  opportunity(name, { stage: "acquire", action: "farm", owned: false, owned_level: null, state: "missing", earned_rank: 0, remaining_mastery: 3000,
    craft: { requirements: [], builds: [], credits: 15_000, credits_short: 0 }, relic: { parts, coverage }, ...over });

const complete = (probability: number): Coverage => ({ kind: "complete", probability });

test("stored controls are validated field by field and fall back to defaults", () => {
  assert.deepEqual(parseControls(null), DEFAULT_CONTROLS);
  assert.deepEqual(parseControls("not json"), DEFAULT_CONTROLS);
  assert.deepEqual(parseControls(JSON.stringify({ view: "collection", sort: "name", progress: "bogus", category: "Melee", hideRelics: true })),
    { ...DEFAULT_CONTROLS, view: "collection", sort: "name", category: "Melee", hideRelics: true });
  assert.equal(parseControls(JSON.stringify({ hideRelics: "yes" })).hideRelics, false);
  assert.equal(parseControls(JSON.stringify({ easy: true })).easy, true);
  assert.equal(parseControls(JSON.stringify({ easy: "yes" })).easy, false);
  assert.equal(parseControls(JSON.stringify({ availability: "unblocked" })).availability, "unblocked");
});

test("presets set filters and sort only, and the active one is read back from the controls", () => {
  const custom = { ...DEFAULT_CONTROLS, result: "relics" as const, category: "Melee", comparison: "full" as const, easy: true };
  const applyPreset = (controls: MasteryControls, key: Preset) => ({ ...controls, ...PRESETS.find(p => p.key === key)!.patch });
  const quick = applyPreset(custom, "quick");
  assert.deepEqual(quick, { ...custom, availability: "available", progress: "all", sort: "mastery", hideRelics: true });
  assert.equal(activePreset(quick), "quick");
  const early = applyPreset(custom, "early");
  assert.deepEqual(early, { ...custom, availability: "unblocked", progress: "all", sort: "mastery", hideRelics: false });
  assert.equal(activePreset(early), "early");
  const all = applyPreset(custom, "completionist");
  assert.deepEqual(all, { ...custom, availability: "all", progress: "all", sort: "mastery", hideRelics: false });
  assert.equal(activePreset(all), "completionist");
  // A manual change afterwards leaves no preset active, and the defaults show everything.
  assert.equal(activePreset({ ...quick, sort: "name" }), null);
  assert.equal(activePreset(DEFAULT_CONTROLS), "completionist");
  assert.equal(activePreset(parseControls(JSON.stringify(early))), "early");
});

test("Easy mode shows everything not blocked, over the stored filters, in every result view", () => {
  const list = [
    opportunity("Braton"),
    opportunity("Hek", { state: "missing", earned_rank: 0, remaining_mastery: 3000, access: "blocked", blockers: [{ kind: "mastery_rank_below", required: 4 }], purchase: bratonPrime() }),
    opportunity("Skana", { category: "Melee", state: "unknown", earned_rank: null, remaining_mastery: null, access: "unknown", purchase: bratonPrime() }),
  ];
  const names = (o: Opportunity[]) => o.map(x => x.name);
  // The stored filters alone show only Hek. Easy mode looks past them without changing them.
  const stored = { ...DEFAULT_CONTROLS, category: "Primary", availability: "blocked" as const, sort: "name" as const, hideRelics: true };
  assert.deepEqual(names(visibleOpportunities(list, stored, "")), ["Hek"]);
  const easy = shownControls({ ...stored, easy: true });
  assert.deepEqual(easy, { ...DEFAULT_CONTROLS, availability: "unblocked", easy: true });
  assert.deepEqual(shownControls(stored), stored);
  assert.deepEqual(names(visibleOpportunities(list, easy, "")), ["Braton", "Skana"]);
  assert.deepEqual(names(visiblePurchases(list, easy, "")), ["Skana"]);
  assert.deepEqual(names(visiblePurchases(list, DEFAULT_CONTROLS, "")), ["Hek", "Skana"]);
});

test("a relic route joins Suggestions strictly above the threshold, on the unrounded chance", () => {
  assert.equal(RELIC_SUGGESTION_THRESHOLD, 0.85);
  assert.equal(inSuggestions(relicFarm("Braton Prime", complete(0.8499))), false);
  assert.equal(inSuggestions(relicFarm("Braton Prime", complete(0.85))), false);
  assert.equal(inSuggestions(relicFarm("Braton Prime", complete(0.8501))), true);
  assert.equal(inSuggestions(relicFarm("Braton Prime", { kind: "partial", missing: ["Barrel"], short: [] })), false);
  assert.equal(inSuggestions(relicFarm("Braton Prime", { kind: "unknown" })), false);
  assert.equal(inSuggestions(opportunity("Braton")), true);
});

test("Suggestions and More relics split the list, and More relics ranks by chance then partial then unknown", () => {
  const list = [
    opportunity("Braton"),
    relicFarm("Akstiletto Prime", { kind: "unknown" }),
    relicFarm("Vectis Prime", complete(0.42)),
    relicFarm("Saryn Prime", { kind: "partial", missing: ["Neuroptics Blueprint"], short: [] }, [], { remaining_mastery: 6000 }),
    relicFarm("Braton Prime", complete(0.9)),
    relicFarm("Vasto Prime", complete(0.85)),
  ];
  const names = (o: Opportunity[]) => o.map(x => x.name);
  assert.deepEqual(names(visibleOpportunities(list, DEFAULT_CONTROLS, "")), ["Braton", "Braton Prime"]);
  assert.deepEqual(names(visibleOpportunities(list, { ...DEFAULT_CONTROLS, hideRelics: true }, "")), ["Braton"]);
  const relics = { ...DEFAULT_CONTROLS, result: "relics" as const };
  assert.deepEqual(names(visibleOpportunities(list, relics, "")), ["Vasto Prime", "Vectis Prime", "Saryn Prime", "Akstiletto Prime"]);
  assert.deepEqual(names(visibleOpportunities(list, { ...relics, hideRelics: true }, "")), ["Vasto Prime", "Vectis Prime", "Saryn Prime", "Akstiletto Prime"]);
  assert.deepEqual(names(visibleOpportunities(list, relics, "prime")), ["Vasto Prime", "Vectis Prime", "Saryn Prime", "Akstiletto Prime"]);
  assert.deepEqual(names(visibleOpportunities(list, { ...relics, availability: "blocked" }, "")), []);
  // A stock change that lifts the chance past the threshold moves the row.
  const lifted = list.map(o => o.name === "Vectis Prime" ? relicFarm("Vectis Prime", complete(0.86)) : o);
  assert.deepEqual(names(visibleOpportunities(lifted, DEFAULT_CONTROLS, "")), ["Braton", "Vectis Prime", "Braton Prime"]);
  assert.deepEqual(names(visibleOpportunities(lifted, relics, "")), ["Vasto Prime", "Saryn Prime", "Akstiletto Prime"]);
});

test("filters narrow by category, progress and availability; search matches the name", () => {
  const list = [
    opportunity("Braton"),
    opportunity("Hek", { category: "Primary", state: "missing", earned_rank: 0, remaining_mastery: 3000, access: "blocked", blockers: [{ kind: "mastery_rank_below", required: 4 }] }),
    opportunity("Skana", { category: "Melee", state: "unknown", earned_rank: null, remaining_mastery: null, access: "unknown" }),
  ];
  const names = (o: Opportunity[]) => o.map(x => x.name);
  assert.deepEqual(names(visibleOpportunities(list, DEFAULT_CONTROLS, "")), ["Braton", "Hek", "Skana"]);
  assert.deepEqual(names(visibleOpportunities(list, { ...DEFAULT_CONTROLS, category: "Melee" }, "")), ["Skana"]);
  assert.deepEqual(names(visibleOpportunities(list, { ...DEFAULT_CONTROLS, progress: "missing" }, "")), ["Hek"]);
  assert.deepEqual(names(visibleOpportunities(list, { ...DEFAULT_CONTROLS, availability: "available" }, "")), ["Braton"]);
  assert.deepEqual(names(visibleOpportunities(list, { ...DEFAULT_CONTROLS, availability: "unknown" }, "")), ["Skana"]);
  assert.deepEqual(names(visibleOpportunities(list, DEFAULT_CONTROLS, "sKa")), ["Skana"]);
});

test("sorting by name stays inside the stage order", () => {
  const list = [
    opportunity("Zenith", { remaining_mastery: 3000 }),
    opportunity("Braton"),
    opportunity("Amprex", { stage: "acquire", action: "buy", owned: false, owned_level: null, vendors: [{ syndicate: "Steel Meridian", tier: "General", blueprint: false, rank: 5, standing: 20_000 }] }),
    opportunity("Vectis", { stage: "acquire", action: "buy", owned: false, owned_level: null, remaining_mastery: null }),
    opportunity("Argonak", { stage: "unsourced", action: "acquire", owned: false, owned_level: null, access: "blocked", remaining_mastery: 3000 }),
  ];
  assert.deepEqual(visibleOpportunities(list, DEFAULT_CONTROLS, "").map(o => o.name), ["Zenith", "Braton", "Amprex", "Vectis", "Argonak"]);
  assert.deepEqual(visibleOpportunities(list, { ...DEFAULT_CONTROLS, sort: "name" }, "").map(o => o.name), ["Braton", "Zenith", "Amprex", "Vectis", "Argonak"]);
  assert.deepEqual(visibleOpportunities(list, { ...DEFAULT_CONTROLS, availability: "available" }, "").map(o => o.name), ["Zenith", "Braton", "Amprex", "Vectis"], "an unsourced row filters like any other");
});

test("route labels come from the kind, and an acquire row without one reads source unknown", () => {
  const labels: [RouteKind, string][] = [
    [{ kind: "craft" }, "Recipe"],
    [{ kind: "relic" }, "Relic drop"],
    [{ kind: "drop" }, "Mission drop"],
    [{ kind: "vendor" }, "Vendor"],
    [{ kind: "trade" }, "Player trade"],
    [{ kind: "adversary" }, "Lich, Sister or Technocyte Coda reward"],
    [{ kind: "conservation" }, "Revived by Son on Deimos"],
    [{ kind: "market_credits", credits: 25_000, blueprint: true }, "Market, 25,000 credits blueprint"],
    [{ kind: "market_credits", credits: 25_000, blueprint: false }, "Market, 25,000 credits"],
    [{ kind: "baro" }, "Baro Ki'Teer"],
    [{ kind: "nightwave" }, "Nightwave"],
    [{ kind: "quest" }, "Quest"],
    [{ kind: "quest", quest: "The Teacher" }, "Quest: The Teacher"],
    [{ kind: "vendor", syndicate: "The Hex", rank: 5 }, "Vendor"],
    [{ kind: "research", lab: "Tenno Lab" }, "Research at Tenno Lab"],
  ];
  for (const [route, label] of labels) assert.equal(routeText(route), label);
  const acquire = (route: RouteKind | null) => opportunity("Fluctus", { stage: "acquire", action: "acquire", owned: false, owned_level: null, route });
  assert.equal(actionText(acquire({ kind: "adversary" })), "Lich, Sister or Technocyte Coda reward");
  assert.equal(actionText(acquire(null)), SOURCE_UNKNOWN);
  assert.deepEqual(sourceLines(acquire({ kind: "research", lab: "Tenno Lab" }), 0), [], "the action slot already names the route");
  // A farm row tells its recipe, relic and drop through the plan, so only an origin kind is repeated in the detail.
  const farm = (route: RouteKind) => opportunity("Fluctus", { stage: "craft", action: "farm", owned: false, owned_level: null, route,
    craft: { requirements: [], builds: [], credits: 15_000, credits_short: 0 } });
  assert.deepEqual(sourceLines(farm({ kind: "research", lab: "Tenno Lab" }), 0), ["Research at Tenno Lab"]);
  assert.deepEqual(sourceLines(farm({ kind: "market_credits", credits: 20_000, blueprint: true }), 0), ["Market, 20,000 credits blueprint"]);
  assert.deepEqual(sourceLines(farm({ kind: "relic" }), 0), [], "the ingredient icons carry the credits");
  assert.deepEqual(sourceLines(opportunity("Kuva Karak", { route: { kind: "adversary" } }), 0), ["Owned copy"], "an owned copy is past its route");
});

test("a Baro row reads his stock while he is here and his return while he is away", () => {
  const until = (ms: number) => `T${ms}`;
  const baro = (baro: Opportunity["baro"], blockers: Opportunity["blockers"] = []) =>
    opportunity("Mara Detron", { stage: "acquire", action: "acquire", owned: false, owned_level: null, route: { kind: "baro" }, baro, blockers });
  assert.equal(actionText(baro({ state: "present", until: 2_000, ducats: 500, credits: 200_000 }), until), "At Baro until T2000, 500 ducats + 200,000 credits");
  assert.equal(actionText(baro({ state: "away", until: 3_000 }, [{ kind: "baro_away" }]), until), "Baro, away until T3000");
  assert.equal(actionText(baro({ state: "unstocked", until: 2_000 }, [{ kind: "baro_not_stocking" }]), until), "Baro, not in this visit (until T2000)");
  assert.equal(actionText(baro({ state: "away", until: null }, [{ kind: "baro_away" }]), until), "Baro, away");
  assert.equal(actionText(baro(null, [{ kind: "baro_visit_unknown" }]), until), "Baro Ki'Teer");
  assert.equal(blockerText({ kind: "baro_away" }), "Baro away");
  assert.equal(blockerText({ kind: "baro_not_stocking" }), "Not in Baro's stock");
  assert.equal(blockerText({ kind: "baro_visit_unknown" }), "Baro's schedule unknown");
});

test("a curated vendor offer names its rank where the catalogue would name a title", () => {
  const buy = (vendors: Opportunity["vendors"]) => opportunity("Vesper 77", { stage: "craft", action: "buy", owned: false, owned_level: null, route: { kind: "vendor", syndicate: "The Hex", rank: 3 }, vendors });
  assert.deepEqual(sourceLines(buy([{ syndicate: "The Hex", tier: "", rank: 3, blueprint: true }]), 0), ["The Hex, Rank 3 (blueprint)"]);
  assert.deepEqual(sourceLines(buy([{ syndicate: "Steel Meridian", tier: "General", rank: null, blueprint: false }]), 0), ["Steel Meridian, General"]);
  // A blueprint offer sits under the blueprint's line. A built-item offer has no part to sit under, so it stays in the source block.
  const offers = buy([{ syndicate: "The Hex", tier: "", rank: 3, blueprint: true }, { syndicate: "Steel Meridian", tier: "General", rank: null, blueprint: false }]);
  assert.deepEqual(acquisitionLines(offers, "/bp", 0, true), ["Vendor: The Hex, Rank 3"]);
  assert.deepEqual(acquisitionLines(offers, "/bp", 0), []);
});

test("a part line quotes the market and leaves out what it has nothing for", () => {
  const braton = buyer("Braton Prime", bratonPrime());
  const blueprint = braton.purchase!.parts[0];
  assert.deepEqual(acquisitionLines(braton, blueprint.unique_name, NOW), ["Market: 25p · 2m ago"]);
  assert.deepEqual(acquisitionLines(braton, "/Lotus/Types/Recipes/Nothing", NOW), []);
});

test("labels spell out the action, the route and unknowns", () => {
  const now = 10_000_000;
  assert.equal(remainingText(1800), "+1,800");
  assert.equal(remainingText(null), "Unknown");
  assert.equal(remainingText(4000, { level_cap: 30, forma: 5, mastery: 1000 }), "+3,000 to 30 + 1,000 with 5 Forma");
  assert.equal(remainingText(null, { level_cap: 30, forma: 5, mastery: 1000 }), "Unknown");
  const gated = opportunity("Kuva Karak", { cap: 40, remaining_mastery: 4000, forma: { level_cap: 30, forma: 5, mastery: 1000 },
    craft: { requirements: [{ unique_name: "/Lotus/Types/Items/MiscItems/Forma", name: "Forma", needed: 5, from_stock: 2, short: 3 }], builds: [], credits: 0, credits_short: 0 } });
  assert.deepEqual(sourceLines(gated, now), ["Owned copy"], "the Forma shortage is an ingredient icon");
  assert.equal(actionText({ ...gated, owned_level: 12 }), "Level R12 → R30", "the action stops at the copy's level cap");
  assert.equal(actionText(opportunity("Braton", { owned_level: 5 })), "Level R5 → R30");
  assert.equal(actionText(opportunity("Braton", { owned_level: null })), "Level to R30");
  assert.equal(actionText(opportunity("Hek", { action: "claim", owned: false, owned_level: null, build_completion_ms: 5_000_000 })), "Claim from Foundry");
  const bought = opportunity("Hek", { action: "buy", owned: false, owned_level: null, vendors: [
    { syndicate: "Cephalon Simaris", tier: "", rank: null, blueprint: true },
    { syndicate: "Steel Meridian", tier: "General", rank: null, blueprint: false },
  ] });
  assert.equal(actionText(bought), "Buy blueprint from Cephalon Simaris");
  assert.deepEqual(sourceLines(bought, now), ["Cephalon Simaris (blueprint)", "Steel Meridian, General"]);
  assert.deepEqual(sourceLines(opportunity("Braton"), now), ["Owned copy"]);
  const akbolto = opportunity("Akbolto", { owned: false, action: "farm", craft: {
    requirements: [], builds: [], credits: 25000, credits_short: 0,
    level_first: [{ unique_name: "/Lotus/Weapons/Tenno/Pistol/Bolto", name: "Bolto", gain: 1800 }],
  } });
  assert.deepEqual(sourceLines(akbolto, now), ["Level Bolto first (+1,800 mastery)"]);
  const node = { key: "SolNode27", planet: "Earth", mode: "normal" as const, junction: false, amount: 24 };
  const ePrime = opportunity("E Prime", { unique_name: "SolNode27", category: "Star Chart", cap: 1, earned_rank: 0, state: "missing",
    stage: "acquire", action: "complete", remaining_mastery: null, owned: false, owned_level: null, access: "unknown", blockers: [{ kind: "node_unlock_unknown" }], node });
  assert.equal(actionText(ePrime), "Complete node");
  assert.deepEqual(sourceLines(ePrime, now), ["Earth"]);
  assert.equal(actionText({ ...ePrime, action: "unlock", node: { ...node, junction: true } }), "Unlock junction");
  assert.deepEqual(sourceLines(opportunity("Braton", { build_completion_ms: now * 1000 - 1 }), now), ["Owned copy", "Build ready"]);
  assert.deepEqual(sourceLines(opportunity("Braton", { build_completion_ms: now * 1000 + 60_000 }), now, "14:32"), ["Owned copy", "Build ready in 1m (14:32)"]);
  const spend = opportunity("Railjack", {
    unique_name: "LPP_SPACE", category: "Intrinsics", cap: 50, earned_rank: 45, remaining_mastery: 7500, owned: false, owned_level: null,
    action: "spend", spend: { ranks: 5, points: 2048, mastery: 7500, tracks: [
      { track: "Piloting", from: 9, to: 10 }, { track: "Gunnery", from: 8, to: 10 }, { track: "Engineering", from: 8, to: 10 },
    ] },
  });
  assert.equal(actionText(spend), "Spend 2,048 points for 5 ranks");
  assert.deepEqual(sourceLines(spend, now), ["+7,500 mastery", "Piloting R9 → R10", "Gunnery R8 → R10", "Engineering R8 → R10"]);
  assert.equal(actionText(opportunity("Drifter", { action: "spend", spend: { ranks: 1, points: 205, mastery: 1500, tracks: [{ track: "Endurance", from: 8, to: 9 }] } })), "Spend 205 points for 1 rank");
  const chassis = { unique_name: "/Lotus/Types/Recipes/Parts/Chassis", name: "Chassis", needed: 1, from_stock: 0, short: 0 };
  const ferrite = { unique_name: "/Lotus/Types/Items/MiscItems/Ferrite", name: "Ferrite", needed: 150, from_stock: 50, short: 100 };
  const crafted = opportunity("Hek", { stage: "craft", action: "craft", owned: false, owned_level: null,
    craft: { requirements: [chassis], builds: [], credits: 15_000, credits_short: 0 } });
  assert.equal(actionText(crafted), "Craft now");
  assert.equal(actionText({ ...crafted, access: "blocked", blockers: [{ kind: "credits_short", short: 5_000 }] }), "Craft");
  assert.deepEqual(sourceLines(crafted, now), []);
  const built = opportunity("Hek", { stage: "craft", action: "build", owned: false, owned_level: null,
    craft: { requirements: [chassis], builds: [{ unique_name: chassis.unique_name, name: "Chassis", crafts: 2 }], credits: null, credits_short: 0 } });
  assert.equal(actionText(built), "Build 1 part, then craft");
  assert.deepEqual(sourceLines(built, now), []);
  const farmed = opportunity("Hek", { stage: "craft", action: "farm", owned: false, owned_level: null,
    craft: { requirements: [chassis, ferrite], builds: [], credits: 20_000, credits_short: 5_000 } });
  assert.equal(actionText(farmed), "Farm 1 item");
  assert.deepEqual(sourceLines(farmed, now), []);
  const barrel = { unique_name: "/Lotus/Types/Recipes/Weapons/WeaponParts/BratonPrimeBarrel", name: "Barrel", needed: 1, from_stock: 0, short: 1 };
  const relicParts: RelicPart[] = [{ unique_name: barrel.unique_name, name: "Barrel", short: 2, relics: [
    { unique_name: "/Lotus/Types/Game/Projections/T1VoidProjectionBPlatinum", name: "Lith B4 Radiant", count: 3, chance: 0.1667 },
    { unique_name: "/Lotus/Types/Game/Projections/T1VoidProjectionBBronze", name: "Lith B4 Intact", count: 1, chance: 0.2533 },
  ], dropped_by: ["Lith B4", "Meso F2"] }];
  const relicFarmed = relicFarm("Braton Prime", complete(0.4231), relicParts,
    { craft: { requirements: [barrel, ferrite], builds: [], credits: 15_000, credits_short: 0 } });
  assert.equal(actionText(relicFarmed), "Farm relics + 1 item");
  assert.equal(actionText({ ...relicFarmed, craft: { ...relicFarmed.craft!, requirements: [barrel] } }), "Farm relics");
  assert.equal(chanceText(relicFarmed.relic!.coverage), "42%");
  assert.equal(chanceText(complete(0.005)), "0.5%");
  assert.equal(chanceText(complete(0)), "0%");
  assert.equal(chanceText({ kind: "partial", missing: [], short: ["Barrel"] }), "Partial");
  assert.equal(chanceText({ kind: "unknown" }), "Unknown");
  // The per-part relic and drop lines sit under the part in the modal's tree, so the source block keeps only the coverage summary.
  assert.deepEqual(sourceLines(relicFarmed, now), []);
  assert.deepEqual(acquisitionLines(relicFarmed, barrel.unique_name, NOW), ["Relics: Lith B4 (Radiant ×3, Intact ×1), Meso F2"]);
  assert.deepEqual(acquisitionLines(relicFarmed, ferrite.unique_name, NOW), []);
  const partial = relicFarm("Braton Prime", { kind: "partial", missing: ["Blueprint"], short: ["Barrel"] },
    [{ unique_name: "/bp", name: "Blueprint", short: 1, relics: [], dropped_by: ["Axi B1"] }, ...relicParts]);
  assert.deepEqual(sourceLines(partial, now), ["No relic for Blueprint", "Too few relics for Barrel"]);
  // A part no owned relic drops still names the relics that do.
  assert.deepEqual(acquisitionLines(partial, "/bp", NOW), ["Relics: Axi B1"]);
  const cell = { unique_name: "/Lotus/Types/Items/MiscItems/OrokinCell", name: "Orokin Cell", needed: 10, from_stock: 7, short: 3 };
  const dropParts: DropPart[] = [{ unique_name: cell.unique_name, name: "Orokin Cell", short: 3, locations: [
    { location: "Corrupted Vor", chance: 50 }, { location: "Saturn/Titan (Survival), Rotation C", chance: 12.5 }, { location: "Cephalon Simaris", chance: null },
  ] }];
  const dropFarmed = { ...relicFarmed, craft: { ...relicFarmed.craft!, requirements: [barrel, cell, ferrite] }, drop: { parts: dropParts } };
  assert.equal(actionText(dropFarmed), "Farm relics + 2 items");
  assert.deepEqual(acquisitionLines(dropFarmed, cell.unique_name, NOW), ["Drops: Corrupted Vor (50%), Saturn/Titan (Survival), Rotation C (12.5%), Cephalon Simaris"]);
  const onlyDrops = { ...dropFarmed, relic: null, craft: { ...dropFarmed.craft, requirements: [cell] }, drop: { parts: [{ ...dropParts[0], short: 1 }] } };
  assert.equal(actionText(onlyDrops), "Farm 1 item");
  assert.deepEqual(sourceLines(onlyDrops, now), []);
  assert.equal(readyText(5_000_000, now), "ready");
  assert.equal(readyText(now + 3_720_000, now), "ready in 1h 2m");
  assert.equal(readyText(now + 45_000, now), "ready in 1m");
});

test("blocker labels come from the kind, with the numbers formatted", () => {
  assert.equal(blockerText({ kind: "mastery_rank_below", required: 4 }), "Requires MR 4");
  assert.equal(blockerText({ kind: "credits_short", short: 15_000 }), "Needs 15,000 more credits");
  assert.equal(blockerText({ kind: "missing_gate", path: "EarthToMarsJunction", name: "Mars Junction" }), "Mars Junction not cleared");
  for (const kind of ["mastery_rank_unknown", "credits_unknown", "standing_unknown", "junction_tasks_unknown", "node_unlock_unknown"] as const) {
    assert.match(blockerText({ kind }), /unknown$/);
    assert.doesNotMatch(blockerText({ kind }), /observed/);
  }
  assert.equal(blockerText({ kind: "standing_rank_below", required: 3, syndicate: "The Hex" }), "Rank 3 with The Hex needed");
  assert.equal(blockerText({ kind: "standing_short", short: 4_000 }), "4,000 standing short");
});
