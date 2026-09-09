import type {
  ClockFormat,
  FoundryPageSize,
  RelicOverlayPriority,
  RelicPickLines,
  RelicPickPriority,
  RelicRefinement,
} from "../types/settings";

export const CLOCK_FORMAT_OPTIONS = ["auto", "12h", "24h"] as const satisfies readonly ClockFormat[];
export const DEFAULT_CLOCK_FORMAT: ClockFormat = "auto";

export const RELIC_OVERLAY_PRIORITY_OPTIONS = ["completion", "setPlat", "plat", "ducat"] as const satisfies readonly RelicOverlayPriority[];
export const DEFAULT_RELIC_OVERLAY_PRIORITY: RelicOverlayPriority = "completion";

export const RELIC_PICK_PRIORITY_OPTIONS = ["unowned", "ducat", "platinum"] as const satisfies readonly RelicPickPriority[];
export const DEFAULT_RELIC_PICK_PRIORITY: RelicPickPriority = "unowned";

export const RELIC_PICK_REFINEMENT_OPTIONS = ["intact", "exceptional", "flawless", "radiant"] as const satisfies readonly RelicRefinement[];
export const DEFAULT_RELIC_PICK_REFINEMENT: RelicRefinement = "radiant";

export const RELIC_PICK_LINES_OPTIONS = ["all", "best", "estimated"] as const satisfies readonly RelicPickLines[];
export const DEFAULT_RELIC_PICK_LINES: RelicPickLines = "all";

export const FOUNDRY_PAGE_SIZE_OPTIONS = [30, 60, 100] as const satisfies readonly FoundryPageSize[];
export const DEFAULT_FOUNDRY_PAGE_SIZE: FoundryPageSize = 30;

export const MODULAR_SECTION_ORDER_DEFAULT = ["tracking", "favorites", "timers", "fissures"] as const;
