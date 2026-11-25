// Run with: node --experimental-strip-types --test src/lib/ingredients.test.ts

import assert from "node:assert/strict";
import { test } from "node:test";
import { availableText, iconLines, ingredients, ingredientTitle, stateLabel } from "./ingredients.ts";
import type { CraftPlan, IngredientState, Requirement } from "../types/mastery.ts";

const line = (over: Partial<Requirement>): Requirement => ({
  unique_name: "/ferrite", name: "Ferrite", image_name: "ferrite.png", category: "Resources", needed: 10, owned: 25, from_stock: 10, short: 0, state: "owned", ...over,
});

const plan = (over: Partial<CraftPlan> = {}): CraftPlan => ({
  requirements: [
    line({ unique_name: "/bp", name: "Frost Blueprint", image_name: null, category: "Blueprints", needed: 1, owned: 1, from_stock: 1 }),
    line({ unique_name: "/chassis", name: "Frost Chassis", category: "Warframes", needed: 1, owned: 0, from_stock: 0, state: "blocked" }),
    line({}),
  ],
  builds: [{ unique_name: "/chassis", name: "Frost Chassis", crafts: 1 }],
  credits: 40_000,
  credits_short: 15_000,
  credit_balance: 25_000,
  ...over,
});

test("credits lead and the rest keep recipe order", () => {
  assert.deepEqual(ingredients(plan()).map(i => [i.name, i.state]), [
    ["Credits", "partial"], ["Frost Blueprint", "owned"], ["Frost Chassis", "blocked"], ["Ferrite", "owned"],
  ]);
});

test("only parts get an icon: resources and part blueprints drop out, Forma and an unlisted ingredient stay", () => {
  const forma = line({ unique_name: "/Lotus/Types/Items/MiscItems/Forma", name: "Forma", needed: 2, owned: 0, from_stock: 0, state: "missing" });
  const unlisted = line({ unique_name: "/mystery", name: "Mystery", category: null });
  const chassisBp = line({ unique_name: "/chassisbp", name: "Frost Chassis Blueprint", category: "Blueprints", needed: 1, owned: 0, from_stock: 0, short: 1, state: "missing", part_blueprint: true });
  assert.deepEqual(iconLines(plan({ requirements: [...plan().requirements, chassisBp, forma, unlisted] })).map(i => i.name),
    ["Frost Blueprint", "Frost Chassis", "Forma", "Mystery"]);
});

test("credits read owned when nothing is short, missing when nothing is covered or the cost is unknown, and drop out at zero", () => {
  const credits = (over: Partial<CraftPlan>) => ingredients(plan(over))[0];
  assert.deepEqual(credits({ credits_short: 0, credit_balance: 90_000 }),
    { unique_name: "/_currency/Credits", name: "Credits", image_name: null, category: null, needed: 40_000, owned: 90_000, from_stock: 40_000, short: 0, state: "owned" });
  assert.equal(credits({ credits_short: 40_000 }).state, "missing");
  const unknown = credits({ credits: null, credits_short: 0, credit_balance: null });
  assert.equal(unknown.state, "missing");
  assert.equal(ingredientTitle(unknown), "Credits");
  assert.equal(ingredients(plan({ credits: 0, credits_short: 0 }))[0].name, "Frost Blueprint");
});

test("the title is the name, the whole stock over the need, and a third line only for the states that need one", () => {
  const icon = (state: IngredientState): Requirement => line({ unique_name: "/bolto", name: "Bolto", image_name: null, needed: 2, owned: 0, from_stock: 0, state });
  assert.equal(ingredientTitle({ ...icon("owned"), owned: 3, from_stock: 2 }), "Bolto\nAvailable: 3 / 2");
  assert.equal(ingredientTitle({ ...icon("partial"), owned: 1, from_stock: 1 }), "Bolto\nAvailable: 1 / 2");
  assert.equal(ingredientTitle(icon("missing")), "Bolto\nAvailable: 0 / 2");
  assert.equal(ingredientTitle(icon("buildable")), "Bolto\nAvailable: 0 / 2\nBuildable");
  assert.equal(ingredientTitle(icon("blocked")), "Bolto\nAvailable: 0 / 2\nBlocked by a missing ingredient");
  assert.equal(ingredientTitle(icon("master_first")), "Bolto\nAvailable: 0 / 2\nMaster first");
  assert.equal(ingredientTitle(icon("building")), "Bolto\nAvailable: 0 / 2\nBuilding in the Foundry");
  assert.equal(ingredientTitle(ingredients(plan())[0]), "Credits\nAvailable: 25,000 / 40,000");
  // Before a scan the balance is unknown and nothing is short, so the count falls back to what the plan drew.
  assert.equal(ingredientTitle(ingredients(plan({ credits_short: 0, credit_balance: null }))[0]), "Credits\nAvailable: 40,000 / 40,000");
});

test("a tree line names every state, while the tooltip names only the build states", () => {
  const icon = (state: IngredientState): Requirement => line({ unique_name: "/bolto", name: "Bolto", image_name: null, needed: 2, owned: 1, from_stock: 1, short: 1, state });
  assert.equal(stateLabel(icon("partial")), "Partial");
  assert.equal(stateLabel(icon("blocked")), "Blocked by a missing ingredient");
  assert.equal(ingredientTitle(icon("blueprint_missing")), "Bolto\nAvailable: 1 / 2\nBlueprint missing");
  assert.equal(availableText(icon("partial")), "Available: 1 / 2");
  assert.equal(availableText(ingredients(plan({ credits: null, credits_short: 0, credit_balance: null }))[0]), null);
});

test("a reusable blueprint is infinite once owned", () => {
  const reusable = line({ unique_name: "/cellbp", name: "Orokin Cell Blueprint", category: "Blueprints", needed: 1, reusable: true });
  assert.equal(ingredientTitle({ ...reusable, owned: 1, from_stock: 1, state: "owned" }), "Orokin Cell Blueprint\nAvailable: ∞ / 1");
  assert.equal(ingredientTitle({ ...reusable, owned: 0, from_stock: 0, short: 1, state: "missing" }), "Orokin Cell Blueprint\nAvailable: 0 / 1");
});
