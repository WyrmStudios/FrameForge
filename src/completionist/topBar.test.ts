// Run with: node --experimental-strip-types --test src/completionist/topBar.test.ts

import assert from "node:assert/strict";
import { test } from "node:test";
import { pillSummary, progressText, SOURCE_KINDS } from "./topBar.ts";
import type { MasteryCounts, MasteryProvenance, Provenance } from "../types/mastery.ts";

const confirmed = (observed_at: number): Provenance => ({ state: "confirmed", observed_at });
const unconfirmed: Provenance = { state: "unconfirmed", observed_at: null };
const unknown: Provenance = { state: "unknown", observed_at: null };
const NOW = 10_000;

const provenance = (patch: Partial<MasteryProvenance> = {}): MasteryProvenance =>
  ({ equipment: confirmed(NOW - 300), intrinsics: confirmed(NOW - 300), nodes: confirmed(NOW - 7_200), standing: confirmed(NOW - 300), ...patch });

test("every kind confirmed reads as scanned at the oldest observation", () => {
  const pill = pillSummary(provenance(), NOW, "24h");
  assert.equal(pill.state, "confirmed");
  assert.equal(pill.text, "Scanned 2h ago");
  assert.match(pill.title, /^Equipment: /m);
  assert.match(pill.title, /^Intrinsics: /m);
  assert.match(pill.title, /^Nodes: /m);
  assert.match(pill.title, /^Standing: /m);
  assert.doesNotMatch(pill.title, /Junctions/);
});

test("one unconfirmed kind wins over the confirmed ones", () => {
  const pill = pillSummary(provenance({ intrinsics: unconfirmed }), NOW, "24h");
  assert.equal(pill.state, "unconfirmed");
  assert.equal(pill.text, "Unconfirmed");
  assert.match(pill.title, /^Intrinsics: unconfirmed/m);
});

test("one unknown kind wins over unconfirmed and confirmed", () => {
  const pill = pillSummary(provenance({ equipment: unconfirmed, nodes: unknown }), NOW, "24h");
  assert.equal(pill.state, "unknown");
  assert.equal(pill.text, "No scan yet");
  assert.match(pill.title, /^Nodes: no observation yet/m);
  assert.match(pill.title, /^Equipment: unconfirmed/m);
});

test("the kind list carries no junctions entry", () => {
  assert.deepEqual(SOURCE_KINDS.map(k => k.key), ["equipment", "intrinsics", "nodes", "standing"]);
});

const counts = (patch: Partial<MasteryCounts> = {}): MasteryCounts =>
  ({ total: 1343, mastered: 838, partial: 20, missing: 485, unknown: 0, unobtainable: 7, ...patch });

test("the progress line carries mastered over total and hides unobtainable", () => {
  const line = progressText(counts(), "All");
  assert.equal(line.text, "All 838 / 1343 mastered");
  assert.equal(line.title, "All: 838 mastered, 20 partial, 485 missing");
});

test("the progress line adds unknown only when above zero", () => {
  const line = progressText(counts({ unknown: 265 }), "All");
  assert.equal(line.text, "All 838 / 1343 mastered · 265 unknown");
  assert.equal(line.title, "All: 838 mastered, 20 partial, 485 missing, 265 unknown");
});
