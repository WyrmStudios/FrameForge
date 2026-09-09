import type { ModCopy } from "./inventory";
import type { QuantityMap } from "./items";
import type { WfmRivenAttribute } from "./market";
import type { RivenStat } from "./rivens";
import type { SettingsSnapshot } from "./settings";

export interface RelicRewardsPayload {
  items: string[];
  positions: number[];
}

export type PendingRelicRewards = RelicRewardsPayload | null;

export interface SavedApiInventory extends Record<string, unknown> {
  apiQuantities: QuantityMap;
  apiModCopies: ModCopy[];
  consumedSuits: string[];
}

export type SaveApiInventoryArgs = SavedApiInventory;

export interface WarframeInventoryRequest extends Record<string, unknown> {
  accountId: WarframeCredentials[0];
  nonce: WarframeCredentials[1];
  steamId: WarframeCredentials[2];
}

export interface OverlayWindowBounds extends Record<string, unknown> {
  x: number;
  y: number;
  w: number;
  h: number;
}

export interface InventoryRewardPayload extends Record<string, unknown> {
  path: string;
  qty: number;
}

export interface ConsoleLoginSuccessPayload extends Record<string, unknown> {
  accountId: string;
  nonce: string;
}

export interface OcrRivenScreenResult {
  weapon: string;
  positives: string[];
  negatives: string[];
  rolled_stats: RivenStat[];
  is_comparison: boolean;
  original_rolled_stats: RivenStat[];
  raw: string;
}

export interface AnalyzeRivenArgs extends Record<string, unknown> {
  weapon: string;
  positives: string[];
  negatives: string[];
}

export interface SaveRivenRollArgs extends Record<string, unknown> {
  weapon: string;
  label: string;
  statsJson: string;
  verdict: string;
  score: number;
}

export interface AddTradeArgs extends Record<string, unknown> {
  withPlayer: string;
  direction: string;
  itemName: string;
  itemUrl: string;
  quantity: number;
  platinum: number;
  source: string;
  notes: string;
  sessionId?: string;
  tradeType?: string;
  timestamp?: string;
}

export interface WfmCreateRivenAuctionArgs extends Record<string, unknown> {
  weaponUrlName: string;
  rivenName: string;
  masteryLevel: number;
  modRank: number;
  reRolls: number;
  polarity: string;
  attributes: WfmRivenAttribute[];
  startingPrice: number;
  buyoutPrice: number | null;
  minimalReputation: number;
  note: string;
  visible: boolean;
  isDirectSell: boolean;
}

export interface WfmCreateOrderArgs extends Record<string, unknown> {
  itemId: string;
  orderType: "sell" | "buy";
  platinum: number;
  quantity: number;
  visible: boolean;
  modRank?: number | null;
}

export interface WfmUpdateOrderArgs extends Record<string, unknown> {
  orderId: string;
  platinum: number;
  quantity: number;
  visible: boolean;
}

export interface WfmSetAuctionVisibleArgs extends Record<string, unknown> {
  auctionId: string;
  visible: boolean;
}

export interface WfmSaveCredentialsArgs extends Record<string, unknown> {
  email: string;
  token: string;
}

export interface SettingsGeometry {
  windowX?: number;
  windowY?: number;
  windowWidth?: number;
  windowHeight?: number;
  modularWinX?: number;
  modularWinY?: number;
  modularWinWidth?: number;
  modularWinHeight?: number;
}

// Settings are persisted as an open JSON object and merged by Rust. Keep unknown keys intact.
export interface SettingsFile extends SettingsSnapshot, SettingsGeometry, Record<string, unknown> {}

export interface SettingsPatch extends Partial<SettingsSnapshot>, SettingsGeometry, Record<string, unknown> {}

export interface ItemListStatus {
  count: number;
  recipe_count: number;
  recipe_sample: string[];
}

export interface BlobStatusPayload {
  stage: string;
  detail: string;
}

export type WarframeCredentials = [accountId: string, nonce: string, steamId: string];
export type WarframeWindowRect = [x: number, y: number, width: number, height: number];
export type WfmCredentials = [email: string, token: string];
export type WfmSession = [username: string, status: string];
