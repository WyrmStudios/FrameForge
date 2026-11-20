import type { FilterPresetSettings } from "./filterPresets";

export type ClockFormat = "auto" | "12h" | "24h";
export type RelicOverlayPriority = "completion" | "plat" | "ducat" | "setPlat";
export type RelicPickPriority = "unowned" | "ducat" | "platinum";
export type RelicRefinement = "intact" | "exceptional" | "flawless" | "radiant";
export type RelicPickLines = "all" | "best" | "estimated";
export type FoundryPageSize = 30 | 60 | 100;

export type FissureVariant = "normal" | "hard" | "storm";

export interface FissureWatch {
  id: string;
  tier: string;
  missionType: string;
  variant: "any" | FissureVariant;
}

/** Pixel offsets applied on top of each overlay's built-in placement. */
export interface OverlayOffsets {
  relicPickX: number; relicPickY: number;
  relicX: number; relicY: number;
  rivenX: number; rivenY: number;
}

export const DEFAULT_OVERLAY_OFFSETS: OverlayOffsets = { relicPickX: 0, relicPickY: 0, relicX: 0, relicY: 0, rivenX: 0, rivenY: 0 };

export const OVERLAY_OFFSET_LIMIT = 3000;

export function clampOverlayOffset(v: number): number {
  if (!Number.isFinite(v)) return 0;
  return Math.max(-OVERLAY_OFFSET_LIMIT, Math.min(OVERLAY_OFFSET_LIMIT, Math.round(v)));
}

/** Read saved offsets defensively: unknown/absent axes fall back to 0 (= default placement). */
export function parseOverlayOffsets(v: unknown): OverlayOffsets {
  const o = (v && typeof v === "object" ? v : {}) as Record<string, unknown>;
  const axis = (k: keyof OverlayOffsets) => clampOverlayOffset(Number(o[k] ?? 0));
  return {
    relicPickX: axis("relicPickX"), relicPickY: axis("relicPickY"),
    relicX: axis("relicX"), relicY: axis("relicY"),
    rivenX: axis("rivenX"), rivenY: axis("rivenY"),
  };
}

export interface SettingsSnapshot {
  overlaysEnabled: boolean; overlayEnabled: boolean; overlayPriority: RelicOverlayPriority; overlayOffsets: OverlayOffsets;
  rivenEnabled: boolean; textScale: number; colorblindMode: boolean;
  clockFormat: ClockFormat; companionApiEnabled: boolean; memoryScannerEnabled: boolean;
  blobLogEnabled: boolean; apiLogEnabled: boolean; autoDiagEnabled: boolean; tracked: string[];
  favorites: string[]; timerFavorites: string[]; fissureWatches: FissureWatch[]; fissureNotifications: boolean;
  modularWidth: number; modularSectionOrder: string[]; modularPopout: boolean; wfmInvisibleOnStart: boolean;
  wfmInvisibleOnClose: boolean; wfmAutoInvisible: boolean; wfmAutoInvisibleMins: number; wfmRecordSales: boolean; relicPickEnabled: boolean;
  relicPickPriority: RelicPickPriority; relicPickRefinement: RelicRefinement;
  relicPickLines: RelicPickLines; foundryPageSize: FoundryPageSize;
  filterPresets: FilterPresetSettings;
}
