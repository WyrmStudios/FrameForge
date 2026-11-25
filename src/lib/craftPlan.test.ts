// Run with: node --experimental-strip-types --test src/lib/craftPlan.test.ts

import assert from "node:assert/strict";
import { test } from "node:test";
import { buildable, componentStatus, craftableNow, craftRows } from "./craftPlan.ts";
import type { CraftPlan } from "../types/mastery.ts";
import type { RecipeComponent } from "../types/items.ts";

const plan = (over: Partial<CraftPlan> = {}): CraftPlan => ({
  requirements: [
    { unique_name: "/cell", name: "Orokin Cell", needed: 3, from_stock: 1, short: 2 },
    { unique_name: "/bp", name: "Akbolto Blueprint", needed: 1, from_stock: 1, short: 0 },
    { unique_name: "/bolto", name: "Bolto", needed: 2, from_stock: 1, short: 0 },
  ],
  builds: [{ unique_name: "/bolto", name: "Bolto", crafts: 1 }],
  credits: 40_000,
  credits_short: 0,
  ...over,
});

test("rows sort by name and carry the build on the part's own row", () => {
  assert.deepEqual(craftRows(plan()), [
    { unique_name: "/bp", name: "Akbolto Blueprint", needed: 1, from_stock: 1, short: 0, crafts: 0 },
    { unique_name: "/bolto", name: "Bolto", needed: 2, from_stock: 1, short: 0, crafts: 1 },
    { unique_name: "/cell", name: "Orokin Cell", needed: 3, from_stock: 1, short: 2, crafts: 0 },
  ]);
});

test("craftable needs no build and no shortage; buildable needs a build and no shortage", () => {
  assert.equal(craftableNow(plan()), false);
  assert.equal(buildable(plan()), false);
  const covered = plan({ requirements: plan().requirements.map(r => ({ ...r, short: 0 })) });
  assert.equal(craftableNow(covered), false);
  assert.equal(buildable(covered), true);
  const stocked = plan({ requirements: covered.requirements, builds: [] });
  assert.equal(craftableNow(stocked), true);
  assert.equal(buildable(stocked), false);
});

test("a plan with nothing in it is not craftable", () => {
  assert.equal(craftableNow(plan({ requirements: [], builds: [] })), false);
});

test("component status reads the plan: stocked part, part built from an owned blueprint, else none", () => {
  const leaf = (unique_name: string, count = 1): RecipeComponent => ({ unique_name, name: unique_name, count, result_count: 1, components: [] });
  const bolto: RecipeComponent = { ...leaf("/bolto", 2), components: [leaf("/BoltoBlueprint"), leaf("/lato")] };
  const withBp = plan({ requirements: [...plan().requirements, { unique_name: "/BoltoBlueprint", name: "Bolto Blueprint", needed: 1, from_stock: 1, short: 0 }] });
  assert.equal(componentStatus(leaf("/bp"), withBp), "part");
  assert.equal(componentStatus(bolto, withBp), "blueprint");
  assert.equal(componentStatus(bolto, plan()), "none");
  assert.equal(componentStatus(leaf("/cell"), plan()), "none");
  assert.equal(componentStatus(leaf("/ferrite"), plan()), "none");
});
