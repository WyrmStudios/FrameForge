import type { MatchedFissure, SeenFissures, WorldState, WsFissure, WsStorm } from "./types/worldstate";
import type { FissureVariant, FissureWatch } from "./types/settings";

// actualVariant is passed explicitly from the caller who knows which array the fissure came from
export function matchesWatch(
  watch: FissureWatch,
  fissure: WsFissure | WsStorm,
  actualVariant: FissureVariant,
): boolean {
  // Variant — checked first; quickest way to exit
  if (watch.variant !== "any" && watch.variant !== actualVariant) return false;

  // Tier — bidirectional Omnia:
  // • Watch "Omnia" matches any non-Requiem tier
  // • Fissure tier "Omnia" matches any watch except "Requiem"
  const fTier = fissure.tier;
  if (watch.tier !== "Any") {
    if (watch.tier === "Omnia") {
      if (fTier === "Requiem") return false;
    } else if (fTier === "Omnia") {
      if (watch.tier === "Requiem") return false;
    } else if (watch.tier !== fTier) {
      return false;
    }
  }

  // Mission type
  if (watch.missionType !== "Any" && fissure.missionType !== watch.missionType) return false;

  return true;
}
// A watch with no entry in `seen` yet is new, and everything it matches right
// now is announced: someone adding a watch wants to know what is live, and
// should not have to wait for the next rotation to find out. `restoredIds`
// names the exception, the watches loaded from settings at startup, which are
// seeded in silence so launching the app is not a wall of notifications about
// fissures the user has been watching for days.
//
// The caller replaces its seen-state with `live` wholesale. That is what drops
// expired fissures and removed watches, so nothing accumulates for as long as
// the app runs.
export function collectNewMatches(
  worldState: WorldState,
  watches: FissureWatch[],
  seen: SeenFissures,
  restoredIds: ReadonlySet<string> = new Set(),
): { fresh: MatchedFissure[]; live: SeenFissures } {
  const byVariant: [(WsFissure | WsStorm)[], FissureVariant][] = [
    [worldState.fissures   ?? [], "normal"],
    [worldState.spFissures ?? [], "hard"],
    [worldState.voidStorms ?? [], "storm"],
  ];

  const live: SeenFissures = new Map();
  // Keyed by fissure id so a fissure covered by two watches is announced once.
  const fresh = new Map<string, MatchedFissure>();

  for (const watch of watches) {
    const matched = new Set<string>();
    const previous = seen.get(watch.id);
    const announces = previous !== undefined || !restoredIds.has(watch.id);
    for (const [list, variant] of byVariant) {
      for (const f of list) {
        // An id is only missing when DE's payload is malformed; without this
        // every such entry would collapse onto the same empty-string key.
        if (!f.id || !matchesWatch(watch, f, variant)) continue;
        matched.add(f.id);
        if (announces && !previous?.has(f.id)) fresh.set(f.id, { f, variant });
      }
    }
    live.set(watch.id, matched);
  }

  return { fresh: [...fresh.values()], live };
}
