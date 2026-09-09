export interface CatalogItem {
  unique_name: string;
  name: string;
  category: string;
  image_name?: string;
  vaulted?: boolean | null;
  ducats?: number | null;
  mastery_req?: number | null;
  max_level_cap?: number | null;
  masterable?: boolean | null;
  source_type?: string;
}

export interface WeaponItem {
  unique_name: string;
  name: string;
  category: string;
  image_name?: string;
  mastery_req?: number;
  max_level_cap?: number;
}

export interface CraftingJob {
  unique_name: string;
  item_name: string;
  completion_ms: number;
}

export type QuantityMap = Record<string, number>;
export type RecipeMap = Record<string, RecipeComponent[]>;
export type RelicDropMap = Record<string, string[]>;

export interface RecipeComponent {
  unique_name: string;
  name: string;
  count: number;
  result_count: number;
  components: RecipeComponent[];
}

export type RecipeComponentStatus = "none" | "blueprint" | "part";

export interface ShallowRecipeComponent {
  unique_name: string;
  name: string;
  count: number;
  result_count: number;
}

export interface ArchonShard {
  type: string;
  tauforged: boolean;
  color: string;
  boost?: string;
}

export interface InventoryItem {
  unique_name: string;
  quantity: number;
  mastery_rank: number;
  archon_shards: ArchonShard[];
  forma_count: number;
  subsumed: boolean;
  vaulted: boolean | null;
  category: string;
  ducat_price: number | null;
  wfm_price: number | null;
  image_name: string | null;
  mastery_req: number | null;
}
