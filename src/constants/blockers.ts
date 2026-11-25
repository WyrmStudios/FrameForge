import type { Blocker } from "../types/mastery";
import { formatCount } from "../lib/formatters.ts";

export function blockerText(blocker: Blocker): string {
  switch (blocker.kind) {
    case "still_building": return "Still building";
    case "mastery_rank_below": return `Requires MR ${blocker.required}`;
    case "mastery_rank_unknown": return "Mastery Rank unknown";
    case "credits_short": return `Needs ${formatCount(blocker.short)} more credits`;
    case "credit_cost_unknown": return "Credit cost unknown";
    case "credits_unknown": return "Credits unknown";
    case "standing_unknown": return "Standing unknown";
    case "standing_rank_below": return `Rank ${blocker.required} with ${blocker.syndicate} needed`;
    case "standing_short": return `${formatCount(blocker.short)} standing short`;
    case "drop_sources_unknown": return "Drop sources unknown";
    case "missing_gate": return `${blocker.name} not cleared`;
    case "junction_tasks_unknown": return "Junction tasks unknown";
    case "node_unlock_unknown": return "Node unlock unknown";
    case "baro_away": return "Baro away";
    case "baro_not_stocking": return "Not in Baro's stock";
    case "baro_visit_unknown": return "Baro's schedule unknown";
  }
}
