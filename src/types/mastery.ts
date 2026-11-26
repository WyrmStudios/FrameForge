export type MasteryState = "mastered" | "partial" | "missing" | "unknown";

/** Why no account can earn a source any more; settings exclude each class separately. */
export type Unobtainable = "founders" | "retiredEvent" | "removedNode";

export type Mode = "normal" | "steel_path";

/** A node's normal and Steel Path clears are two sources. The Steel Path row's `unique_name` ends in `/steel_path`. */
export interface NodeInfo {
  key: string;
  planet: string;
  mode: Mode;
  junction: boolean;
  amount: number;
}

export interface MasterySource {
  unique_name: string;
  name: string;
  category: string;
  image_name: string | null;
  mastery_req: number | null;
  cap: number;
  /** Absent while the source kind's progress is Unknown. */
  earned_rank: number | null;
  remaining_mastery: number | null;
  state: MasteryState;
  /** The corrections table's class, whatever the settings say. */
  unobtainable: Unobtainable | null;
  /** Settings exclude the class: the source sits in the Unobtainable bucket, outside `total`. */
  excluded: boolean;
  /** Present on star chart rows only, where `cap` is 1 and `earned_rank` is 0 or 1. */
  node?: NodeInfo;
  /** What the catalogue says about where the source comes from. An opportunity fills in the mechanical kinds once its plan is known. */
  route: RouteKind | null;
  /** Unmastered sources this owned copy is an ingredient of. */
  needed_for: string[];
}

/** Labels live in `constants/routes.ts`. */
export type RouteKind =
  | { kind: "craft" }
  | { kind: "relic" }
  | { kind: "drop" }
  /** Both are set on a curated route only. A derived vendor route leaves them out, since the catalogue's offers in `vendors` carry their own. */
  | { kind: "vendor"; syndicate?: string; rank?: number }
  | { kind: "trade" }
  | { kind: "adversary" }
  | { kind: "conservation" }
  /** `blueprint` is false where the Market sells the built item. */
  | { kind: "market_credits"; credits: number; blueprint: boolean }
  | { kind: "baro" }
  | { kind: "nightwave" }
  | { kind: "quest"; quest?: string }
  | { kind: "research"; lab: string };

export interface MasteryCounts {
  /** Excludes the `unobtainable` count. */
  total: number;
  mastered: number;
  partial: number;
  missing: number;
  unknown: number;
  unobtainable: number;
}

export interface MasteryCategory {
  category: string;
  counts: MasteryCounts;
  sources: MasterySource[];
}

export type ProvenanceState = "confirmed" | "unconfirmed" | "unknown";

export interface Provenance {
  state: ProvenanceState;
  /** Unix seconds; only Confirmed carries one. */
  observed_at: number | null;
}

export interface MasteryProvenance {
  equipment: Provenance;
  intrinsics: Provenance;
  /** Junctions read from the same `Missions` field, so they share the nodes kind. */
  nodes: Provenance;
  standing: Provenance;
}

/** Unsourced holds the rows with remaining mastery and no route at all. */
export type Stage = "level_claim" | "craft" | "acquire" | "unsourced";
/** Craft means everything is in stock. Build means intermediates need crafting first. Farm means parts are short and no vendor sells them. Trade is a whole item only players sell, so it belongs to the platinum view alone. Acquire is a whole item that comes from its route, or a source with no route. */
export type Action = "level" | "claim" | "spend" | "craft" | "build" | "buy" | "farm" | "trade" | "acquire" | "complete" | "unlock";
export type Access = "available" | "blocked" | "unknown";

export type Blocker =
  | { kind: "still_building" }
  | { kind: "mastery_rank_below"; required: number }
  | { kind: "mastery_rank_unknown" }
  | { kind: "credits_short"; short: number }
  | { kind: "credit_cost_unknown" }
  | { kind: "credits_unknown" }
  | { kind: "standing_unknown" }
  | { kind: "standing_rank_below"; required: number; syndicate: string }
  | { kind: "standing_short"; short: number }
  | { kind: "drop_sources_unknown" }
  | { kind: "missing_gate"; path: string; name: string }
  | { kind: "junction_tasks_unknown" }
  | { kind: "node_unlock_unknown" }
  | { kind: "baro_away" }
  | { kind: "baro_not_stocking" }
  | { kind: "baro_visit_unknown" };

export interface VendorOffer {
  syndicate: string;
  /** Holds the catalogue's title name and is empty on a curated offer, which carries only `rank`. */
  tier: string;
  blueprint: boolean;
  rank: number | null;
  standing: number | null;
}

/** Present carries Baro's departure and the summed price of what the row still needs from him, Unstocked his departure alone. Away carries his next arrival, and null when the fetched visit has already ended. Times are in ms. */
export type Baro =
  | { state: "present"; until: number; ducats: number; credits: number }
  | { state: "unstocked"; until: number }
  | { state: "away"; until: number | null };

export interface TrackSpend {
  track: string;
  from: number;
  to: number;
}

/** The ranks that banked Intrinsic points buy today, cheapest first. */
export interface Spend {
  ranks: number;
  points: number;
  mastery: number;
  /** Lists only the tracks that gain a rank, in the game's order. */
  tracks: TrackSpend[];
}

/**
 * Partial outranks the build states. Buildable and blocked both mean the blueprint is in hand, with
 * the ingredients ready or one short. Blueprint missing means the part's own blueprint is short.
 * Master first is an owned unmastered copy the ledger will not spend.
 */
export type IngredientState = "owned" | "partial" | "missing" | "buildable" | "blocked" | "blueprint_missing" | "master_first" | "building";

export interface Requirement {
  unique_name: string;
  name: string;
  image_name: string | null;
  category: string | null;
  needed: number;
  /** The projected stock before this recipe drew on it, which can exceed `needed`. */
  owned: number;
  from_stock: number;
  short: number;
  state: IngredientState;
  reusable?: boolean;
  /** Marks a built part's own blueprint. The part's line already carries its state, so the icon row hides it. */
  part_blueprint?: boolean;
}

export interface Build {
  unique_name: string;
  name: string;
  crafts: number;
}

export interface CraftPlan {
  requirements: Requirement[];
  builds: Build[];
  level_first?: { unique_name: string; name: string; gain: number }[];
  /** Null as soon as any recipe in the plan carries no price. */
  credits: number | null;
  credits_short: number;
  /** The projected balance before this plan took its price. It is null when the cost is unknown as well as when the balance is. */
  credit_balance: number | null;
}

/** One owned relic, at one refinement, that drops the part. */
export interface RelicStock {
  unique_name: string;
  name: string;
  count: number;
  /** The reward's share of one roll after the refinement's table is normalized to one. Null when the table carries no chances. */
  chance: number | null;
}

export interface RelicPart {
  unique_name: string;
  name: string;
  /** Copies still missing after stock and builds. */
  short: number;
  /** Sorted with the best chance first. */
  relics: RelicStock[];
  /** Names every relic whose table lists the part, whether the player owns it or not, with the refinement stripped and sorted. */
  dropped_by: string[];
}

/**
 * Complete: every part has relics enough to roll, and the chance that they all drop.
 * Partial: `missing` names parts no owned relic drops, and `short` names parts the owned relics cannot yield together.
 * Unknown: a relevant relic's table carries no chances, or the walk would take more states than allowed.
 */
export type Coverage =
  | { kind: "complete"; probability: number }
  | { kind: "partial"; missing: string[]; short: string[] }
  | { kind: "unknown" };

/** The chance covers the relic parts only, and the other shortages stay in `craft`. */
export interface RelicRoute {
  parts: RelicPart[];
  coverage: Coverage;
}

/** A place other than a relic that drops an item. `location` is WFCD's free text. */
export interface DropLocation {
  location: string;
  chance: number | null;
}

export interface DropPart {
  unique_name: string;
  name: string;
  /** Copies still missing after stock and builds. */
  short: number;
  /** Sorted with the best chance first and cut to the best few by the backend. */
  locations: DropLocation[];
}

/** Drop locations for the shortages the relic route leaves out. Nothing estimates the whole item. */
export interface DropRoute {
  parts: DropPart[];
}

/**
 * A warframe.market listing. A price with a fetch time is a quote of that age; a price without one
 * came from a cache written before quotes were stamped; no price with a fetch time means the market
 * does not list the slug; neither means nobody has asked yet.
 */
export interface Listing {
  slug: string;
  name: string;
  price: number | null;
  /** Unix seconds. */
  fetched_at: number | null;
}

export interface PartListing extends Listing {
  unique_name: string;
  /** The whole recipe takes this many, owned or not. */
  needed: number;
  /** The finish is still short this many after projected stock. */
  short: number;
}

export type Route = "parts" | "set";

export interface Cost {
  platinum: number;
  route: Route;
}

/** What platinum buys toward one source. Each total is null while any part it sums lacks a price. */
export interface Purchase {
  parts: PartListing[];
  /** Holds the complete set, or the item itself where nothing is crafted. */
  set: Listing | null;
  missing_total: number | null;
  full_total: number | null;
  /** The cheaper of the missing parts and the set. A tie goes to the set. */
  cheapest_finish: Cost | null;
  full_purchase: Cost | null;
}

/** The part of a Level row's remaining mastery above the copy's level cap, and the Forma that lift the cap to the rank cap. */
export interface FormaGate {
  level_cap: number;
  forma: number;
  mastery: number;
}

export interface Opportunity extends MasterySource {
  stage: Stage;
  action: Action;
  owned: boolean;
  /** Null on caches from before levels were stored. With several copies this is the one with the highest level cap. */
  owned_level: number | null;
  /** Present on a Level row whose copy cannot reach the rank cap without Forma; `craft` then lists the Forma as a requirement. */
  forma: FormaGate | null;
  build_completion_ms: number | null;
  vendors: VendorOffer[];
  spend: Spend | null;
  /** Present on a row that comes from Baro, once a worldstate is in hand. */
  baro: Baro | null;
  /** A source that is neither owned nor building carries its plan whenever a recipe exists. */
  craft: CraftPlan | null;
  /** Present when something short in the plan drops from a relic. */
  relic: RelicRoute | null;
  /** Present when a shortage outside the relic route has a known drop location. */
  drop: DropRoute | null;
  purchase: Purchase | null;
  access: Access;
  blockers: Blocker[];
}

export interface MasteryOverview {
  counts: MasteryCounts;
  categories: MasteryCategory[];
  provenance: MasteryProvenance;
  mastery_rank: number | null;
  /** The backend sorts these by stage, then remaining mastery high to low with unknown last, then name. */
  opportunities: Opportunity[];
}

/** An empty selection list is a plan the player cleared, kept until they regenerate. */
export interface MasteryPlan {
  target: number;
  view: string;
  selections: string[];
  allowances: Record<string, number>;
  intrinsic_targets?: Record<string, Record<string, number>>;
  purchase_comparison?: string;
}

/** A shortage the plan could cover with mastered equipment the player owns, once an allowance names how many copies it may spend. */
export interface Allowable {
  unique_name: string;
  name: string;
  owned: number;
}

export interface PlanEntry {
  unique_name: string;
  /** Null when the catalogue no longer lists the path. */
  source: MasterySource | null;
  /** The row as Suggestions would show it, planned in plan order against the plan's own ledger. Null once the source is mastered, when the path repeats an earlier selection, and when nothing suggests it. */
  opportunity: Opportunity | null;
  completed: boolean;
  /** What finishing the entry adds to the projection. A repeat and a completed entry add zero. */
  gain: number | null;
  notes: string[];
  allowable: Allowable[];
}

/**
 * A total the observed Mastery Rank bounds to the range between two thresholds. `exact` is the
 * summed earned mastery, present only when every source kind is Confirmed, every source's credit is
 * known and the sum lands on the observed rank. `rank` follows `exact` where there is one and the
 * lower bound otherwise.
 */
export interface MasteryTotal {
  lower: number;
  upper: number;
  exact: number | null;
  rank: number;
  rank_upper: number;
}

export interface PlanEvaluation {
  entries: PlanEntry[];
  /** Null until the Mastery Rank is observed, as are `gap` and `projected`. */
  total: MasteryTotal | null;
  total_reason: string | null;
  target_xp: number;
  /** Mastery still to earn for the target, from the exact total or the lower bound. */
  gap: number | null;
  /** The known gains of every pending entry. */
  gains: number;
  /** Pending entries whose gain is unknown and so outside `gains`. */
  unknown_gains: number;
  projected: MasteryTotal | null;
  rejected_allowances: string[];
}
