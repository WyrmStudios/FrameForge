import { useState, useEffect, useCallback, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { DEFAULT_LEAD_MINS } from "../arbitration/arbitrationAlerts";
import { DEFAULT_SCHEDULE_DAYS } from "../arbitration/arbitrationSchedule";
import { TIER_KEYS } from "../arbitration/arbitrationTiers";
import { PREFERENCE_KEYS } from "../constants/preferences";
import { TAURI_COMMANDS } from "../constants/tauri";
import {
  CLOCK_FORMAT_OPTIONS,
  DEFAULT_CLOCK_FORMAT,
  DEFAULT_FOUNDRY_PAGE_SIZE,
  DEFAULT_RELIC_OVERLAY_PRIORITY,
  DEFAULT_RELIC_PICK_LINES,
  DEFAULT_RELIC_PICK_PRIORITY,
  DEFAULT_RELIC_PICK_REFINEMENT,
  FOUNDRY_PAGE_SIZE_OPTIONS,
  RELIC_PICK_LINES_OPTIONS,
  RELIC_PICK_PRIORITY_OPTIONS,
  RELIC_PICK_REFINEMENT_OPTIONS,
} from "../constants/settings";
import type { ClockFormat, FissureWatch, FoundryPageSize, OverlayOffsets, RelicOverlayPriority, RelicPickLines, RelicPickPriority, RelicRefinement, SettingsSnapshot } from "../types/settings";
import { DEFAULT_OVERLAY_OFFSETS, parseOverlayOffsets } from "../types/settings";
import type { FilterPresetSettings } from "../types/filterPresets";
import { parseFilterPresetSettings } from "../types/filterPresets";
import type { SettingsFile, SettingsPatch } from "../types/tauri";

interface UseSettingsReturn {
  // Settings state
  memoryScannerEnabled: boolean;
  blobLogEnabled: boolean;
  apiLogEnabled: boolean;
  autoDiagEnabled: boolean;
  overlayEnabled: boolean;
  overlayPriority: RelicOverlayPriority;
  overlayOffsets: OverlayOffsets;
  rivenEnabled: boolean;
  textScale: number;
  colorblindMode: boolean;
  clockFormat: ClockFormat;
  systemLocale: string;
  foundryPageSize: FoundryPageSize;
  relicPickEnabled: boolean;
  memTriggerEnabled: boolean;
  relicPickPriority: RelicPickPriority;
  relicPickRefinement: RelicRefinement;
  relicPickLines: RelicPickLines;
  wfmInvisibleOnStart: boolean;
  wfmInvisibleOnClose: boolean;
  wfmAutoInvisible: boolean;
  wfmAutoInvisibleMins: number;
  wfmRecordSales: boolean;
  companionApiEnabled: boolean;
  filterPresets: FilterPresetSettings;

  // Setters
  setMemoryScannerEnabled: React.Dispatch<React.SetStateAction<boolean>>;
  setBlobLogEnabled: React.Dispatch<React.SetStateAction<boolean>>;
  setApiLogEnabled: React.Dispatch<React.SetStateAction<boolean>>;
  setAutoDiagEnabled: React.Dispatch<React.SetStateAction<boolean>>;
  setOverlayEnabled: React.Dispatch<React.SetStateAction<boolean>>;
  setOverlayPriority: React.Dispatch<React.SetStateAction<RelicOverlayPriority>>;
  setOverlayOffsets: React.Dispatch<React.SetStateAction<OverlayOffsets>>;
  setRivenEnabled: React.Dispatch<React.SetStateAction<boolean>>;
  setTextScale: React.Dispatch<React.SetStateAction<number>>;
  setColorblindMode: React.Dispatch<React.SetStateAction<boolean>>;
  setClockFormat: React.Dispatch<React.SetStateAction<ClockFormat>>;
  setSystemLocale: React.Dispatch<React.SetStateAction<string>>;
  setFoundryPageSize: React.Dispatch<React.SetStateAction<FoundryPageSize>>;
  setRelicPickEnabled: React.Dispatch<React.SetStateAction<boolean>>;
  setMemTriggerEnabled: React.Dispatch<React.SetStateAction<boolean>>;
  setRelicPickPriority: React.Dispatch<React.SetStateAction<RelicPickPriority>>;
  setRelicPickRefinement: React.Dispatch<React.SetStateAction<RelicRefinement>>;
  setRelicPickLines: React.Dispatch<React.SetStateAction<RelicPickLines>>;
  setWfmInvisibleOnStart: React.Dispatch<React.SetStateAction<boolean>>;
  setWfmInvisibleOnClose: React.Dispatch<React.SetStateAction<boolean>>;
  setWfmAutoInvisible: React.Dispatch<React.SetStateAction<boolean>>;
  setWfmAutoInvisibleMins: React.Dispatch<React.SetStateAction<number>>;
  setWfmRecordSales: React.Dispatch<React.SetStateAction<boolean>>;
  setFilterPresets: React.Dispatch<React.SetStateAction<FilterPresetSettings>>;

  // Refs
  settingsLoadedRef: React.MutableRefObject<boolean>;
  settingsRef: React.MutableRefObject<SettingsSnapshot>;
  wfmInvisibleOnStartRef: React.MutableRefObject<boolean>;
  wfmInvisibleOnCloseRef: React.MutableRefObject<boolean>;

  // Callbacks
  saveAllSettings: () => Promise<void>;
  loadSettings: () => Promise<SettingsFile | null>;
}

export function useSettings(
  setMonitoring: React.Dispatch<React.SetStateAction<boolean>>,
): UseSettingsReturn {
  // ── Settings state ──────────────────────────────────────────────────────────
  const [memoryScannerEnabled, setMemoryScannerEnabled] = useState(false);
  const [blobLogEnabled, setBlobLogEnabled] = useState(false);
  const [apiLogEnabled, setApiLogEnabled] = useState(false);
  const [autoDiagEnabled, setAutoDiagEnabled] = useState(false);
  const [overlayEnabled, setOverlayEnabled] = useState<boolean>(
    () => localStorage.getItem(PREFERENCE_KEYS.OVERLAY_ENABLED) !== "false"
  );
  const [overlayPriority, setOverlayPriority] = useState<RelicOverlayPriority>(
    () => (localStorage.getItem(PREFERENCE_KEYS.OVERLAY_PRIORITY) ?? DEFAULT_RELIC_OVERLAY_PRIORITY) as RelicOverlayPriority
  );
  const [overlayOffsets, setOverlayOffsets] = useState<OverlayOffsets>(DEFAULT_OVERLAY_OFFSETS);
  const [rivenEnabled, setRivenEnabled] = useState<boolean>(
    () => localStorage.getItem(PREFERENCE_KEYS.RIVEN_OVERLAY_ENABLED) !== "false"
  );
  const [textScale, setTextScale] = useState(() => {
    const s = parseFloat(localStorage.getItem(PREFERENCE_KEYS.TEXT_SCALE) ?? "1");
    document.documentElement.style.setProperty("--ff-scale", s.toString());
    return s;
  });
  const [colorblindMode, setColorblindMode] = useState(() =>
    localStorage.getItem(PREFERENCE_KEYS.COLORBLIND_MODE) === "true"
  );
  const [clockFormat, setClockFormat] = useState<ClockFormat>(DEFAULT_CLOCK_FORMAT);
  const [systemLocale, setSystemLocale] = useState("en-US");
  const [foundryPageSize, setFoundryPageSize] = useState<FoundryPageSize>(DEFAULT_FOUNDRY_PAGE_SIZE);
  const [relicPickEnabled, setRelicPickEnabled] = useState<boolean>(true);
  const [memTriggerEnabled, setMemTriggerEnabled] = useState<boolean>(false);
  const [relicPickPriority, setRelicPickPriority] = useState<RelicPickPriority>(DEFAULT_RELIC_PICK_PRIORITY);
  const [relicPickRefinement, setRelicPickRefinement] = useState<RelicRefinement>(DEFAULT_RELIC_PICK_REFINEMENT);
  const [relicPickLines, setRelicPickLines] = useState<RelicPickLines>(DEFAULT_RELIC_PICK_LINES);
  const [wfmInvisibleOnStart, setWfmInvisibleOnStart] = useState(false);
  const [wfmInvisibleOnClose, setWfmInvisibleOnClose] = useState(false);
  const [wfmAutoInvisible, setWfmAutoInvisible] = useState(false);
  const [wfmAutoInvisibleMins, setWfmAutoInvisibleMins] = useState(30);
  const [wfmRecordSales, setWfmRecordSales] = useState(false);
  const [filterPresets, setFilterPresets] = useState<FilterPresetSettings>(() => parseFilterPresetSettings(undefined));
  const companionApiEnabled = false; // Feature suspended pending DE clarification

  // ── Refs ────────────────────────────────────────────────────────────────────
  const settingsLoadedRef = useRef(false);
  const settingsRef = useRef<SettingsSnapshot>({
    arbitrationFavorites: [],
    arbitrationLeadMins: DEFAULT_LEAD_MINS,
    arbitrationOverlayEnabled: false,
    arbitrationTierFilter: [...TIER_KEYS],
    arbitrationAlertTiers: [],
    arbitrationScheduleDays: DEFAULT_SCHEDULE_DAYS,
    overlayEnabled: true,
    overlayPriority: DEFAULT_RELIC_OVERLAY_PRIORITY,
    overlayOffsets: DEFAULT_OVERLAY_OFFSETS,
    rivenEnabled: true,
    textScale: 1,
    colorblindMode: false,
    clockFormat: DEFAULT_CLOCK_FORMAT,
    companionApiEnabled: false,
    memoryScannerEnabled: false,
    blobLogEnabled: false,
    apiLogEnabled: false,
    autoDiagEnabled: false,
    tracked: [] as string[],
    favorites: [] as string[],
    timerFavorites: [] as string[],
    fissureWatches: [] as FissureWatch[],
    fissureNotifications: true,
    modularWidth: 240,
    modularSectionOrder: ["tracking", "favorites", "timers"] as string[],
    modularPopout: false,
    wfmInvisibleOnStart: false,
    wfmInvisibleOnClose: false,
    wfmAutoInvisible: false,
    wfmAutoInvisibleMins: 30,
    wfmRecordSales: false,
    relicPickEnabled: true,
    relicPickPriority: DEFAULT_RELIC_PICK_PRIORITY,
    relicPickRefinement: DEFAULT_RELIC_PICK_REFINEMENT,
    relicPickLines: DEFAULT_RELIC_PICK_LINES,
    foundryPageSize: DEFAULT_FOUNDRY_PAGE_SIZE,
    memTriggerEnabled: false,
    filterPresets: { presets: [], restorePreviousFiltersOnPresetClick: false },
  });
  const wfmInvisibleOnStartRef = useRef(false);
  const wfmInvisibleOnCloseRef = useRef(false);

  // ── Save settings ───────────────────────────────────────────────────────────
  const saveAllSettings = useCallback(() => {
    if (!settingsLoadedRef.current) {
      console.error("save_settings skipped: settings not loaded yet, saving now would clobber the file");
      return Promise.resolve();
    }
    const settings: SettingsPatch = { ...settingsRef.current };
    return invoke(TAURI_COMMANDS.SAVE_SETTINGS, { json: JSON.stringify(settings) }).then(() => {}).catch((e) => {
      console.error("save_settings failed:", e);
    });
  }, []);

  // ── Load settings from file ─────────────────────────────────────────────────
  const loadSettings = useCallback(async (): Promise<SettingsFile | null> => {
    try {
      const json = await invoke<string>(TAURI_COMMANDS.LOAD_SETTINGS);
      if (!json) { settingsLoadedRef.current = true; return null; }
      try {
        const s = JSON.parse(json) as SettingsFile;
        if (typeof s.memoryScannerEnabled === "boolean") setMemoryScannerEnabled(s.memoryScannerEnabled);
        if (typeof s.blobLogEnabled === "boolean") setBlobLogEnabled(s.blobLogEnabled);
        if (typeof s.apiLogEnabled === "boolean") setApiLogEnabled(s.apiLogEnabled);
        if (typeof s.autoDiagEnabled === "boolean") {
          setAutoDiagEnabled(s.autoDiagEnabled);
          localStorage.setItem(PREFERENCE_KEYS.AUTO_DIAGNOSTICS, String(s.autoDiagEnabled));
        }
        if (typeof s.overlayEnabled === "boolean") {
          setOverlayEnabled(s.overlayEnabled);
          localStorage.setItem(PREFERENCE_KEYS.OVERLAY_ENABLED, String(s.overlayEnabled));
        }
        if (typeof s.overlayPriority === "string") {
          setOverlayPriority(s.overlayPriority as RelicOverlayPriority);
          localStorage.setItem(PREFERENCE_KEYS.OVERLAY_PRIORITY, s.overlayPriority);
        }
        if (s.overlayOffsets) {
          const offsets = parseOverlayOffsets(s.overlayOffsets);
          setOverlayOffsets(offsets);
          // Mirror for overlay windows, which read localStorage directly.
          localStorage.setItem(PREFERENCE_KEYS.OVERLAY_OFFSETS, JSON.stringify(offsets));
        }
        if (typeof s.rivenEnabled === "boolean") {
          setRivenEnabled(s.rivenEnabled);
          localStorage.setItem(PREFERENCE_KEYS.RIVEN_OVERLAY_ENABLED, String(s.rivenEnabled));
        }
        if (typeof s.textScale === "number") {
          setTextScale(s.textScale);
          document.documentElement.style.setProperty("--ff-scale", s.textScale.toString());
          localStorage.setItem(PREFERENCE_KEYS.TEXT_SCALE, s.textScale.toString());
        }
        if (typeof s.colorblindMode === "boolean") {
          setColorblindMode(s.colorblindMode);
          localStorage.setItem(PREFERENCE_KEYS.COLORBLIND_MODE, String(s.colorblindMode));
        }
        if (typeof s.clockFormat === "string" && CLOCK_FORMAT_OPTIONS.includes(s.clockFormat)) {
          setClockFormat(s.clockFormat as ClockFormat);
        }
        if (typeof s.foundryPageSize === "string" && FOUNDRY_PAGE_SIZE_OPTIONS.includes(s.foundryPageSize)) {
          setFoundryPageSize(s.foundryPageSize);
        }
        if (typeof s.relicPickEnabled === "boolean") {
          setRelicPickEnabled(s.relicPickEnabled);
          invoke(TAURI_COMMANDS.SET_RELIC_PICK_ENABLED, { enabled: s.relicPickEnabled });
        }
        if (typeof s.memTriggerEnabled === "boolean") {
          setMemTriggerEnabled(s.memTriggerEnabled);
          invoke(TAURI_COMMANDS.SET_MEM_TRIGGER_ENABLED, { enabled: s.memTriggerEnabled });
        }
        if (RELIC_PICK_PRIORITY_OPTIONS.includes(s.relicPickPriority)) setRelicPickPriority(s.relicPickPriority);
        if (RELIC_PICK_REFINEMENT_OPTIONS.includes(s.relicPickRefinement)) setRelicPickRefinement(s.relicPickRefinement);
        if (RELIC_PICK_LINES_OPTIONS.includes(s.relicPickLines)) setRelicPickLines(s.relicPickLines);
        if (typeof s.wfmInvisibleOnStart === "boolean") {
          setWfmInvisibleOnStart(s.wfmInvisibleOnStart);
          wfmInvisibleOnStartRef.current = s.wfmInvisibleOnStart;
        }
        if (typeof s.wfmInvisibleOnClose === "boolean") {
          setWfmInvisibleOnClose(s.wfmInvisibleOnClose);
          wfmInvisibleOnCloseRef.current = s.wfmInvisibleOnClose;
        }
        if (typeof s.wfmAutoInvisible === "boolean") setWfmAutoInvisible(s.wfmAutoInvisible);
        if (typeof s.wfmAutoInvisibleMins === "number") setWfmAutoInvisibleMins(s.wfmAutoInvisibleMins);
        if (typeof s.wfmRecordSales === "boolean") setWfmRecordSales(s.wfmRecordSales);
        if (s.filterPresets) setFilterPresets(parseFilterPresetSettings(s.filterPresets));
        return s;
      } catch {
        settingsLoadedRef.current = true;
        return null;
      }
    } catch {
      return null;
    } finally {
      invoke<string>("get_system_locale").then(loc => { if (loc) setSystemLocale(loc); }).catch(() => {});
    }
  }, []);

  // ── Memory scanner toggle ───────────────────────────────────────────────────
  useEffect(() => {
    if (memoryScannerEnabled) {
      invoke("start_monitor").then(() => setMonitoring(true)).catch(() => {});
    } else {
      invoke("stop_monitor").then(() => setMonitoring(false)).catch(() => {});
    }
  }, [memoryScannerEnabled, setMonitoring]);

  // ── Blob log toggle ─────────────────────────────────────────────────────────
  useEffect(() => {
    invoke("set_blob_log", { enabled: blobLogEnabled }).catch(() => {});
  }, [blobLogEnabled]); // eslint-disable-line

  // ── API log toggle ──────────────────────────────────────────────────────────
  useEffect(() => {
    invoke("set_api_log", { enabled: apiLogEnabled }).catch(() => {});
  }, [apiLogEnabled]); // eslint-disable-line

  return {
    // Settings state
    memoryScannerEnabled,
    blobLogEnabled,
    apiLogEnabled,
    autoDiagEnabled,
    overlayEnabled,
    overlayPriority,
    overlayOffsets,
    rivenEnabled,
    textScale,
    colorblindMode,
    clockFormat,
    systemLocale,
    foundryPageSize,
    relicPickEnabled,
    memTriggerEnabled,
    relicPickPriority,
    relicPickRefinement,
    relicPickLines,
    wfmInvisibleOnStart,
    wfmInvisibleOnClose,
    wfmAutoInvisible,
    wfmAutoInvisibleMins,
    wfmRecordSales,
    companionApiEnabled,
    filterPresets,

    // Setters
    setMemoryScannerEnabled,
    setBlobLogEnabled,
    setApiLogEnabled,
    setAutoDiagEnabled,
    setOverlayEnabled,
    setOverlayPriority,
    setOverlayOffsets,
    setRivenEnabled,
    setTextScale,
    setColorblindMode,
    setClockFormat,
    setSystemLocale,
    setFoundryPageSize,
    setRelicPickEnabled,
    setMemTriggerEnabled,
    setRelicPickPriority,
    setRelicPickRefinement,
    setRelicPickLines,
    setWfmInvisibleOnStart,
    setWfmInvisibleOnClose,
    setWfmAutoInvisible,
    setWfmAutoInvisibleMins,
    setWfmRecordSales,
    setFilterPresets,

    // Refs
    settingsLoadedRef,
    settingsRef,
    wfmInvisibleOnStartRef,
    wfmInvisibleOnCloseRef,

    // Callbacks
    saveAllSettings,
    loadSettings,
  };
}
