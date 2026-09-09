import type { RelicRefinement } from "../types/settings";

export const RELIC_REFINEMENT_ORDER = ["intact", "exceptional", "flawless", "radiant"] as const;

export const RELIC_REFINEMENT_LABELS: Record<RelicRefinement, string> = {
  intact: "Intact",
  exceptional: "Except.",
  flawless: "Flawless",
  radiant: "Radiant",
};

export const RELIC_DROP_RATES = {
  intact:      { Common: 0.2533, Uncommon: 0.11,  Rare: 0.02 },
  exceptional: { Common: 0.2333, Uncommon: 0.13,  Rare: 0.04 },
  flawless:    { Common: 0.20,   Uncommon: 0.17,  Rare: 0.06 },
  radiant:     { Common: 0.1667, Uncommon: 0.20,  Rare: 0.10 },
} as const;
