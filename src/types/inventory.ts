import type { CraftingJob, QuantityMap } from "./items";

export interface ModCopy {
  uniqueName: string;
  rank: number | null; // null = raw (RawUpgrades), number = from Upgrades (0 = installed-unranked)
  count: number;
}

export interface RawArchonShard {
  upgrade_type: string;
  color: string; // raw string from game JSON, e.g. "ACC_CRIMSON", "ACC_AZURE_TAUFORGED"
}

export interface ChangeLogEntry {
  id: number;
  unique_name: string;
  item_name: string;
  old_qty: number;
  new_qty: number;
  delta: number;
  timestamp: number;
  rank?: number | null;
}

export interface InventoryUpdate {
  quantities: QuantityMap;
  crafting: CraftingJob[];
  mastery_rank?: number;
  mastery_data?: Record<string, number>;
  changes: ChangeLogEntry[];
  warframe_running: boolean;
  scanned_at: number;
  consumed_suits?: string[];
  mods?: Record<string, { total: number; by_rank: Record<string, number> }>;
  socketed_shards?: Record<string, RawArchonShard[]>;
  forma_counts?: Record<string, number>;
  is_full_pass?: boolean;
  player_name?: string;
}

export interface TrackedItem {
  unique_name: string;
  display_name: string;
  added_at: string;
}

export interface SnapshotPoint {
  date: string;
  quantity: number;
  change: number;
}
