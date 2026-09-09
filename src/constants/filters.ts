import type { FoundryFilters, InventoryFilters, MarketFilters, RelicFilters, SyndicateFilters } from "../types/filters";

export const INVENTORY_FILTERS_DEFAULT: InventoryFilters = {
  category: "all",
  search: "",
  filterOwned: false,
  filterRecent: false,
  filterPrime: false,
  filterVaulted: false,
  filterUnvaulted: false,
  filterRank: null,
  sortMode: "qty-desc",
};

export const FOUNDRY_FILTERS_DEFAULT: FoundryFilters = {
  search: "", activeCat: "Warframes",
  filterPrime: false, filterNonPrime: false, filterVaulted: false, filterUnvaulted: false,
  filterMastered: false, filterUnmastered: false,
  filterOwned: false, filterUnowned: false, filterReady: false,
  filterLvlCap: false,
  ignoreFormaKuva: false,
};

export const MARKET_FILTERS_DEFAULT: MarketFilters = {
  search: "", ownership: [], conditions: [], vault: [], sortMode: "ducats",
  activeMarketTab: "trading",
};

export const RELIC_FILTERS_DEFAULT: RelicFilters = {
  search: "", tiers: [], ownership: [], vault: [], completion: [], sortMode: "count",
  ignoreFormaKuva: false,
};

export const SYNDICATE_FILTERS_DEFAULT: SyndicateFilters = {
  activeGroup: "main", activeTab: "Steel Meridian", missingOnly: false, search: "",
};
