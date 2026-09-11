import type { FoundryFilters, InventoryFilters, MarketFilters, RelicFilters } from "./filters";

export type FilterPresetModule = "inventory" | "foundry" | "market" | "relics";

export type FoundryPresetFilters = Omit<FoundryFilters, "activeCat">;
export type MarketPresetFilters = Omit<MarketFilters, "activeMarketTab">;

export interface FilterPresetFiltersByModule {
  inventory: InventoryFilters;
  foundry: FoundryPresetFilters;
  market: MarketPresetFilters;
  relics: RelicFilters;
}

export type FilterPreset = {
  [M in FilterPresetModule]: {
    id: string;
    name: string;
    module: M;
    createdAt: number;
    pinned?: boolean;
    color?: string;
    dividerAfter?: boolean;
    filters: FilterPresetFiltersByModule[M];
  };
}[FilterPresetModule];

export interface FilterPresetSettings {
  presets: FilterPreset[];
  restorePreviousFiltersOnPresetClick: boolean;
}

const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === "object" && value !== null && !Array.isArray(value);

const isStringArray = (value: unknown, allowed: readonly string[]) =>
  Array.isArray(value) && value.every(item => typeof item === "string" && allowed.includes(item));

function parseInventoryFilters(value: unknown): InventoryFilters | null {
  if (!isRecord(value) || typeof value.category !== "string" || typeof value.search !== "string" ||
    typeof value.filterOwned !== "boolean" || typeof value.filterRecent !== "boolean" ||
    typeof value.filterPrime !== "boolean" || typeof value.filterVaulted !== "boolean" ||
    typeof value.filterUnvaulted !== "boolean" ||
    !(typeof value.filterRank === "number" || value.filterRank === "unranked" || value.filterRank === null) ||
    !["qty-desc", "qty-asc", "name-asc", "name-desc", "recent"].includes(value.sortMode as string)) return null;
  return {
    category: value.category, search: value.search, filterOwned: value.filterOwned, filterRecent: value.filterRecent,
    filterPrime: value.filterPrime, filterVaulted: value.filterVaulted, filterUnvaulted: value.filterUnvaulted,
    filterRank: value.filterRank, sortMode: value.sortMode as InventoryFilters["sortMode"],
  };
}

function parseFoundryFilters(value: unknown): FoundryPresetFilters | null {
  if (!isRecord(value) || typeof value.search !== "string" || typeof value.filterPrime !== "boolean" ||
    typeof value.filterNonPrime !== "boolean" || typeof value.filterVaulted !== "boolean" ||
    typeof value.filterUnvaulted !== "boolean" || typeof value.filterMastered !== "boolean" ||
    typeof value.filterUnmastered !== "boolean" || typeof value.filterOwned !== "boolean" ||
    typeof value.filterUnowned !== "boolean" || typeof value.filterReady !== "boolean" ||
    typeof value.filterLvlCap !== "boolean" || typeof value.ignoreFormaKuva !== "boolean") return null;
  return {
    search: value.search, filterPrime: value.filterPrime, filterNonPrime: value.filterNonPrime,
    filterVaulted: value.filterVaulted, filterUnvaulted: value.filterUnvaulted,
    filterMastered: value.filterMastered, filterUnmastered: value.filterUnmastered,
    filterOwned: value.filterOwned, filterUnowned: value.filterUnowned, filterReady: value.filterReady,
    filterLvlCap: value.filterLvlCap, ignoreFormaKuva: value.ignoreFormaKuva,
  };
}

function parseMarketFilters(value: unknown): MarketPresetFilters | null {
  if (!isRecord(value) || typeof value.search !== "string" ||
    !isStringArray(value.ownership, ["owned", "notowned"]) ||
    !isStringArray(value.conditions, ["dupes", "itemowned", "fullset", "hasparts"]) ||
    !isStringArray(value.vault, ["vaulted", "unvaulted"]) ||
    !["plat", "ducats", "az", "za"].includes(value.sortMode as string)) return null;
  return {
    search: value.search,
    ownership: value.ownership as MarketPresetFilters["ownership"],
    conditions: value.conditions as MarketPresetFilters["conditions"],
    vault: value.vault as MarketPresetFilters["vault"],
    sortMode: value.sortMode as MarketPresetFilters["sortMode"],
  };
}

function parseRelicFilters(value: unknown): RelicFilters | null {
  if (!isRecord(value) || typeof value.search !== "string" || !isStringArray(value.tiers, ["lith", "meso", "neo", "axi", "requiem"]) ||
    !isStringArray(value.ownership, ["owned", "notowned"]) ||
    !isStringArray(value.vault, ["vaulted", "unvaulted"]) ||
    !isStringArray(value.completion, ["complete", "incomplete"]) ||
    !["count", "plat", "ducats", "az", "za"].includes(value.sortMode as string) ||
    typeof value.ignoreFormaKuva !== "boolean") return null;
  return {
    search: value.search, tiers: value.tiers as string[], ownership: value.ownership as RelicFilters["ownership"],
    vault: value.vault as RelicFilters["vault"], completion: value.completion as RelicFilters["completion"],
    sortMode: value.sortMode as RelicFilters["sortMode"], ignoreFormaKuva: value.ignoreFormaKuva,
  };
}

function parseFilters(module: FilterPresetModule, value: unknown): FilterPresetFiltersByModule[FilterPresetModule] | null {
  switch (module) {
    case "inventory": return parseInventoryFilters(value);
    case "foundry": return parseFoundryFilters(value);
    case "market": return parseMarketFilters(value);
    case "relics": return parseRelicFilters(value);
  }
}

function parsePreset(value: unknown): FilterPreset | null {
  if (!isRecord(value) || typeof value.id !== "string" || !value.id || typeof value.name !== "string" || !value.name.trim() ||
    !["inventory", "foundry", "market", "relics"].includes(value.module as string) ||
    typeof value.createdAt !== "number" || !Number.isFinite(value.createdAt) || value.createdAt < 0 ||
    (value.pinned !== undefined && typeof value.pinned !== "boolean") ||
    (value.color !== undefined && (typeof value.color !== "string" || !value.color)) ||
    (value.dividerAfter !== undefined && typeof value.dividerAfter !== "boolean")) return null;
  const module = value.module as FilterPresetModule;
  const filters = parseFilters(module, value.filters);
  return filters ? { id: value.id, name: value.name.trim(), module, createdAt: value.createdAt, ...(value.pinned !== undefined && { pinned: value.pinned }), ...(value.color !== undefined && { color: value.color }), ...(value.dividerAfter !== undefined && { dividerAfter: value.dividerAfter }), filters } as FilterPreset : null;
}

export function parseFilterPresetSettings(value: unknown): FilterPresetSettings {
  if (!isRecord(value)) return { presets: [], restorePreviousFiltersOnPresetClick: false };
  return {
    presets: Array.isArray(value.presets) ? value.presets.map(parsePreset).filter((preset): preset is FilterPreset => preset !== null) : [],
    restorePreviousFiltersOnPresetClick: value.restorePreviousFiltersOnPresetClick === true,
  };
}
