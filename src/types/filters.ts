export type InventorySortMode = "qty-desc" | "qty-asc" | "name-asc" | "name-desc" | "recent";

export interface InventoryFilters {
  category: string;
  search: string;
  filterOwned: boolean;
  filterRecent: boolean;
  filterPrime: boolean;
  filterVaulted: boolean;
  filterUnvaulted: boolean;
  filterRank: number | "unranked" | null;
  sortMode: InventorySortMode;
}

export interface FoundryFilters {
  search: string; activeCat: string;
  filterPrime: boolean; filterNonPrime: boolean; filterVaulted: boolean; filterUnvaulted: boolean;
  filterMastered: boolean; filterUnmastered: boolean;
  filterOwned: boolean; filterUnowned: boolean; filterReady: boolean;
  filterLvlCap: boolean;
  ignoreFormaKuva: boolean;
}

export interface MarketFilters {
  search: string;
  ownership: ("owned" | "notowned")[];
  conditions: ("dupes" | "itemowned" | "fullset" | "hasparts")[];
  vault: ("vaulted" | "unvaulted")[];
  sortMode: "plat" | "ducats" | "az" | "za";
  activeMarketTab: "trading" | "sets" | "mods" | "rivens" | "sisters";
}

export interface RelicFilters {
  search: string;
  tiers: string[];
  ownership: ("owned" | "notowned")[];
  vault: ("vaulted" | "unvaulted")[];
  completion: ("complete" | "incomplete")[];
  sortMode: "count" | "plat" | "ducats" | "az" | "za";
  ignoreFormaKuva: boolean;
}

export interface SyndicateFilters {
  activeGroup: "main" | "openworld" | "other" | "lab";
  activeTab: string;
  missingOnly: boolean;
  search: string;
}
