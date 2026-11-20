// The community's S-to-D rating of how well a node farms Vitus Essence. The
// table itself lives in the backend, next to the schedule it annotates; this
// is the vocabulary the browser needs to show and filter by it.

export type Tier = "S" | "A" | "B" | "C" | "D";

/// A node the rating does not cover is Unrated, which is a value the filter
/// and the alert rule can be set to like any other.
export type TierKey = Tier | "unrated";

export const TIER_KEYS: readonly TierKey[] = [
  "S",
  "A",
  "B",
  "C",
  "D",
  "unrated",
];

export const TIER_LABELS: Record<TierKey, string> = {
  S: "S",
  A: "A",
  B: "B",
  C: "C",
  D: "D",
  unrated: "Unrated",
};

export const tierKey = (tier: Tier | null): TierKey => tier ?? "unrated";

/// Settings are a file the user can edit and an older version can have
/// written, so a stored selection is filtered down to keys this version
/// knows rather than trusted. Null for anything that is not a list at all,
/// which leaves the caller's default in place.
export function sanitizeTierKeys(value: unknown): TierKey[] | null {
  if (!Array.isArray(value)) return null;
  return TIER_KEYS.filter((k) => value.includes(k));
}
