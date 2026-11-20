
import assert from "node:assert/strict";
import { test } from "node:test";
import { sanitizeTierKeys, tierKey, TIER_KEYS } from "./arbitrationTiers.ts";

test("an unrated node filters as its own key", () => {
  assert.equal(tierKey(null), "unrated");
  assert.equal(tierKey("S"), "S");
});

test("a stored selection keeps only keys this version knows, in display order", () => {
  assert.deepEqual(sanitizeTierKeys(["unrated", "B", "S", "Z", 7]), [
    "S",
    "B",
    "unrated",
  ]);
  assert.deepEqual(sanitizeTierKeys([]), []);
  assert.deepEqual(sanitizeTierKeys([...TIER_KEYS]), [...TIER_KEYS]);
});

test("a selection that is not a list at all leaves the default alone", () => {
  assert.equal(sanitizeTierKeys(undefined), null);
  assert.equal(sanitizeTierKeys("S"), null);
  assert.equal(sanitizeTierKeys(null), null);
});
