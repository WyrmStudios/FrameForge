
import assert from "node:assert/strict";
import { test } from "node:test";
import { clampScheduleDays, DEFAULT_SCHEDULE_DAYS, SCHEDULE_DAY_OPTIONS } from "./arbitrationSchedule.ts";

test("the window defaults to a week and the options reach 60 days", () => {
  assert.equal(DEFAULT_SCHEDULE_DAYS, 7);
  assert.deepEqual([...SCHEDULE_DAY_OPTIONS], [3, 7, 14, 30, 60]);
});

test("a stored selection outside the 1-60 day range clamps to its edge", () => {
  assert.equal(clampScheduleDays(0), 1);
  assert.equal(clampScheduleDays(-5), 1);
  assert.equal(clampScheduleDays(61), 60);
  assert.equal(clampScheduleDays(9999), 60);
  assert.equal(clampScheduleDays(14), 14);
});

test("a non-number selection falls back to the default", () => {
  assert.equal(clampScheduleDays(Number.NaN), DEFAULT_SCHEDULE_DAYS);
  assert.equal(clampScheduleDays(Number.POSITIVE_INFINITY), DEFAULT_SCHEDULE_DAYS);
});