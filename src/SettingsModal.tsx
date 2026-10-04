import { useState, type Dispatch, type MutableRefObject, type SetStateAction } from "react";
import { invoke } from "@tauri-apps/api/core";
import { emit } from "@tauri-apps/api/event";
import { notify, ensurePermission } from "./lib/notify";
import { formatBytes } from "./lib/formatters";
import { hidePickOutline, hideRewardOutline, hideRivenOverlay, showPickOutline, showRewardOutline, showRivenOverlay } from "./lib/outlineWindows";
import FilterPresets from "./shared/FilterPresets";
import { ActionButton, DangerButton, SecondaryButton } from "./shared/ui/ActionButton";
import { ModalCloseButton } from "./shared/ui/ModalCloseButton";
import { PREFERENCE_KEYS } from "./constants/preferences";
import { CLOCK_FORMAT_OPTIONS, FOUNDRY_PAGE_SIZE_OPTIONS, RELIC_OVERLAY_PRIORITY_OPTIONS, RELIC_PICK_LINES_OPTIONS, RELIC_PICK_PRIORITY_OPTIONS } from "./constants/settings";
import { TAURI_COMMANDS, TAURI_EVENTS } from "./constants/tauri";
import type { ArchonShard, QuantityMap } from "./types/items";
import type { ChangeLogEntry, ModCopy } from "./types/inventory";
import type { ClockFormat, FoundryPageSize, OverlayOffsets, RelicOverlayPriority, RelicPickLines, RelicPickPriority, SettingsSnapshot } from "./types/settings";
import { OVERLAY_OFFSET_LIMIT, clampOverlayOffset } from "./types/settings";
import type { FilterPresetModule, FilterPresetSettings } from "./types/filterPresets";
import type { FoundryFilters, InventoryFilters, MarketFilters, RelicFilters } from "./types/filters";
import type { SaveApiInventoryArgs } from "./types/tauri";
import "./SettingsModal.css";

type SettingsTab = "general" | "overlays" | "market" | "filters" | "accessibility" | "notifications" | "data" | "debugging";
type Setter<T> = Dispatch<SetStateAction<T>>;
type ScannerMods = Record<string, { total: number; by_rank: Record<string, number> }>;
type ArchonShards = Record<string, ArchonShard[]>;

const ROW_CLASS = "flex items-center justify-between gap-4";
const ROW_INFO_CLASS = "flex min-w-0 flex-col gap-0.5";
const ROW_LABEL_CLASS = "text-13 font-medium text-foreground";
const ROW_DESC_CLASS = "text-11 leading-1.4 text-muted";
const SELECT_CLASS = "shrink-0 cursor-pointer rounded-5 border border-border bg-surface px-2 py-0.75 text-12 text-foreground [color-scheme:dark] hover:border-accent/50";
const STEP_BTN_CLASS = "flex min-w-0 cursor-pointer items-center justify-center border-0 border-border bg-white/3 p-0 text-6 leading-none text-muted first:border-b hover:bg-accent/18 hover:text-accent focus-visible:outline focus-visible:outline-accent focus-visible:-outline-offset-1";
const DEBUG_TABLE_CLASS = "debug-table grid grid-cols-[1fr_auto_auto_auto] items-center gap-x-2 gap-y-3";
const SECTION_CLASS = "border-b border-border/60 px-5 py-3 last:border-b-0";
const SECTION_TITLE_CLASS = "mb-2.5 text-10 font-bold uppercase tracking-0.07 text-muted";
const SECTION_MSG_CLASS = "mt-2 text-11 text-green";
const TOGGLE_BUTTON_CLASS = "min-w-16";
const toggleButtonClass = (active: boolean) => TOGGLE_BUTTON_CLASS + (active ? " border-accent! bg-accent/15!" : "");
const disabledClass = (disabled: boolean, debugOpacity = false) => disabled ? debugOpacity ? " opacity-40 pointer-events-none" : " opacity-45 pointer-events-none" : "";
const clearButtonClass = (hasData: boolean) => hasData ? "border-[var(--red)]! text-danger!" : "";

export interface SettingsModalProps {
  open: boolean;
  onClose: () => void;
  settingsTab: SettingsTab;
  setSettingsTab: (tab: SettingsTab) => void;
  settingsFilterModule: FilterPresetModule; setSettingsFilterModule: Setter<FilterPresetModule>;
  filterPresets: FilterPresetSettings; setFilterPresets: Setter<FilterPresetSettings>;
  inventoryFilters: InventoryFilters; setInventoryFilters: Setter<InventoryFilters>;
  foundryFilters: FoundryFilters; setFoundryFilters: Setter<FoundryFilters>;
  marketFilters: MarketFilters; setMarketFilters: Setter<MarketFilters>;
  relicFilters: RelicFilters; setRelicFilters: Setter<RelicFilters>;
  foundryPageSize: FoundryPageSize; setFoundryPageSize: Setter<FoundryPageSize>; settingsRef: MutableRefObject<SettingsSnapshot>; saveAllSettings: () => Promise<void>;
  memoryScannerEnabled: boolean; setMemoryScannerEnabled: Setter<boolean>; modularPopout: boolean; setModularPopout: Setter<boolean>; overlayStatus: string;
  overlayEnabled: boolean; setOverlayEnabled: Setter<boolean>; overlayPriority: RelicOverlayPriority; setOverlayPriority: Setter<RelicOverlayPriority>;
  overlayOffsets: OverlayOffsets; setOverlayOffsets: Setter<OverlayOffsets>;
  rivenEnabled: boolean; setRivenEnabled: Setter<boolean>;
  memTriggerEnabled: boolean; setMemTriggerEnabled: Setter<boolean>;
  relicPickEnabled: boolean; setRelicPickEnabled: Setter<boolean>; relicPickPriority: RelicPickPriority; setRelicPickPriority: Setter<RelicPickPriority>;
  relicPickLines: RelicPickLines; setRelicPickLines: Setter<RelicPickLines>; wfmLoggedIn: boolean;
  wfmInvisibleOnStart: boolean; setWfmInvisibleOnStart: Setter<boolean>; wfmInvisibleOnStartRef: MutableRefObject<boolean>; wfmInvisibleOnClose: boolean; setWfmInvisibleOnClose: Setter<boolean>; wfmInvisibleOnCloseRef: MutableRefObject<boolean>;
  wfmAutoInvisible: boolean; setWfmAutoInvisible: Setter<boolean>; wfmAutoInvisibleMins: number; setWfmAutoInvisibleMins: Setter<number>; wfmRecordSales: boolean; setWfmRecordSales: Setter<boolean>; colorblindMode: boolean; setColorblindMode: Setter<boolean>; textScale: number; setTextScale: Setter<number>;
  clockFormat: ClockFormat; setClockFormat: Setter<ClockFormat>; systemLocale: string; itemCount: number; recipeCount: number; handleFetch: () => Promise<void>; fetching: boolean; fetchMsg: string;
  fissureNotifications: boolean; onFissureNotificationsChange: (enabled: boolean) => void;
  setQuantities: Setter<QuantityMap>; setApiQuantities: Setter<QuantityMap>; setApiModCopies: Setter<ModCopy[]>; setScannerMods: Setter<ScannerMods>; setMasteryData: Setter<Record<string, number>>; setArchonShards: Setter<ArchonShards>; setFormaData: Setter<QuantityMap>;
  setChangeLog: Setter<ChangeLogEntry[]>; setLastChanged: Setter<Record<string, number>>; setWfConnected: Setter<boolean>; wfConnectedRef: MutableRefObject<boolean>; setItemsRefreshKey: Setter<number>; setClearMsg: Setter<string>; clearMsg: string;
  blobLogEnabled: boolean; setBlobLogEnabled: Setter<boolean>; blobLogSize: number; setBlobLogSize: Setter<number>; companionApiEnabled: boolean; apiLogEnabled: boolean; setApiLogEnabled: Setter<boolean>; apiLogSize: number; setApiLogSize: Setter<number>;
  setShowInventoryBatchPreview: Setter<boolean>; notifyTestResult: string; setNotifyTestResult: Setter<string>; overlayLogCopied: boolean; setOverlayLogCopied: Setter<boolean>; autoDiagEnabled: boolean; setAutoDiagEnabled: Setter<boolean>;
  diagFolderSize: number; setDiagFolderSize: Setter<number>; diagPath: string | null; diagCapturing: boolean; setDiagCapturing: Setter<boolean>; setDiagPath: Setter<string | null>; reloadDebugSizes: () => void;
  memoryProbing: boolean; setMemoryProbing: Setter<boolean>; probeSize: number; setProbeSize: Setter<number>; rawScanning: boolean; setRawScanning: Setter<boolean>; rawScanSize: number; setRawScanSize: Setter<number>;
  memRelicDebugRunning: boolean; setMemRelicDebugRunning: Setter<boolean>; relicPickOcrResult: string | null; relicPickOcrTesting: boolean; setRelicPickOcrTesting: Setter<boolean>; setRelicPickOcrResult: Setter<string | null>;
  relicPickTestResult: string | null; relicPickTestEra: string; setRelicPickTestEra: Setter<string>; setRelicPickTestResult: Setter<string | null>; eeLogTail: string | null; setEeLogTail: Setter<string | null>;
  debugCatEnabled: boolean; setDebugCatEnabled: Setter<boolean>; unmatchedPathsSize: number; setUnmatchedPathsSize: Setter<number>; appVersion: string;
}

function FactoryResetButton() {
  const [confirm, setConfirm] = useState(false);
  const [resetting, setResetting] = useState(false);
  if (!confirm) return <DangerButton onClick={() => setConfirm(true)}>Factory Reset</DangerButton>;
  return <div className="flex items-center gap-2"><span className="text-12 text-danger">Are you sure? This cannot be undone.</span><DangerButton disabled={resetting} onClick={() => { setResetting(true); invoke("factory_reset").catch(() => setResetting(false)); }}>{resetting ? "Resetting…" : "Yes, reset"}</DangerButton><SecondaryButton onClick={() => setConfirm(false)}>Cancel</SecondaryButton></div>;
}

function BulkPriceRefreshButton() {
  const [state, setState] = useState<"idle" | "loading" | "ok" | "err">("idle");
  const label = state === "loading" ? "Fetching…" : state === "ok" ? "Done!" : state === "err" ? "Failed" : "Refresh Now";
  const stateClass = { idle: "", loading: "", ok: "border-accent!", err: "border-field-error!" }[state];
  return <SecondaryButton className={`min-w-25 ${stateClass}`} disabled={state === "loading"} onClick={() => { setState("loading"); invoke("refresh_bulk_prices").then(() => { setState("ok"); setTimeout(() => setState("idle"), 3000); }).catch(() => { setState("err"); setTimeout(() => setState("idle"), 4000); }); }}>{label}</SecondaryButton>;
}

export default function SettingsModal(props: SettingsModalProps) {
  const { settingsTab, setSettingsTab, settingsFilterModule, setSettingsFilterModule, filterPresets, setFilterPresets, inventoryFilters, setInventoryFilters, foundryFilters, setFoundryFilters, marketFilters, setMarketFilters, relicFilters, setRelicFilters, foundryPageSize, setFoundryPageSize, settingsRef, saveAllSettings, memoryScannerEnabled, setMemoryScannerEnabled, modularPopout, setModularPopout, overlayStatus, overlayEnabled, setOverlayEnabled, overlayPriority, setOverlayPriority, overlayOffsets, setOverlayOffsets, rivenEnabled, setRivenEnabled, memTriggerEnabled, setMemTriggerEnabled, relicPickEnabled, setRelicPickEnabled, relicPickPriority, setRelicPickPriority, relicPickLines, setRelicPickLines, wfmLoggedIn, wfmInvisibleOnStart, setWfmInvisibleOnStart, wfmInvisibleOnStartRef, wfmInvisibleOnClose, setWfmInvisibleOnClose, wfmInvisibleOnCloseRef, wfmAutoInvisible, setWfmAutoInvisible, wfmAutoInvisibleMins, setWfmAutoInvisibleMins, wfmRecordSales, setWfmRecordSales, colorblindMode, setColorblindMode, textScale, setTextScale, clockFormat, setClockFormat, systemLocale, itemCount, recipeCount, handleFetch, fetching, fetchMsg, fissureNotifications, onFissureNotificationsChange, setQuantities, setApiQuantities, setApiModCopies, setScannerMods, setMasteryData, setArchonShards, setFormaData, setChangeLog, setLastChanged, setWfConnected, wfConnectedRef, setItemsRefreshKey, setClearMsg, clearMsg, blobLogEnabled, setBlobLogEnabled, blobLogSize, setBlobLogSize, companionApiEnabled, apiLogEnabled, setApiLogEnabled, apiLogSize, setApiLogSize, setShowInventoryBatchPreview, notifyTestResult, setNotifyTestResult, overlayLogCopied, setOverlayLogCopied, autoDiagEnabled, setAutoDiagEnabled, diagFolderSize, setDiagFolderSize, diagPath, diagCapturing, setDiagCapturing, setDiagPath, reloadDebugSizes, memoryProbing, setMemoryProbing, probeSize, setProbeSize, rawScanning, setRawScanning, rawScanSize, setRawScanSize, memRelicDebugRunning, setMemRelicDebugRunning, relicPickOcrResult, relicPickOcrTesting, setRelicPickOcrTesting, setRelicPickOcrResult, relicPickTestResult, relicPickTestEra, setRelicPickTestEra, setRelicPickTestResult, eeLogTail, setEeLogTail, debugCatEnabled, setDebugCatEnabled, unmatchedPathsSize, setUnmatchedPathsSize, appVersion } = props;

  // Outline toggles for the reward and pick overlays; the riven overlay shows
  // itself directly (Settings → Overlays).
  const [outlineReward, setOutlineReward] = useState(false);
  const [outlinePick,   setOutlinePick]   = useState(false);
  const [rivenShown,    setRivenShown]    = useState(false);
  const [notifPermissionDenied, setNotifPermissionDenied] = useState(false);
  if (!props.open) return null;
  const onClose = props.onClose;

  // ── Position offset row (Settings → Overlays) ─────────────────────────────
  // Adds a pixel delta on top of the overlay's built-in placement; 0 = default.
  const offsetRow = (keyX: keyof OverlayOffsets, keyY: keyof OverlayOffsets, disabled: boolean) => {
    const saveOffsets = async (next: OverlayOffsets) => {
      setOverlayOffsets(next);
      settingsRef.current = { ...settingsRef.current, overlayOffsets: next };
      localStorage.setItem(PREFERENCE_KEYS.OVERLAY_OFFSETS, JSON.stringify(next));
      // Rust reads offsets from settings.json, so wait for the write to land
      // before re-applying placement of an outline that is currently shown.
      await saveAllSettings();
      try {
        if (outlineReward) await showRewardOutline();
        if (outlinePick)   await showPickOutline();
        if (rivenShown)    await showRivenOverlay();
      } catch {}
    };
    const setAxis = (key: keyof OverlayOffsets, n: number) => {
      void saveOffsets({ ...overlayOffsets, [key]: clampOverlayOffset(n) });
    };
    const resetOffsets = () => {
      void saveOffsets({ ...overlayOffsets, [keyX]: 0, [keyY]: 0 });
    };
    const axisInput = (key: keyof OverlayOffsets, axis: "X" | "Y") => (
      <div className="grid h-6.5 w-14 grid-cols-[1fr_16px] overflow-hidden rounded-5 border border-border bg-background transition-colors duration-120 hover:border-accent/65 focus-within:border-accent/65">
        <input
          className="settings-offset-input h-6 min-w-0 w-full border-0 bg-transparent px-1.25 text-right text-12 text-foreground tabular-nums outline-none [appearance:textfield]"
          type="number" min={-OVERLAY_OFFSET_LIMIT} max={OVERLAY_OFFSET_LIMIT} step={10}
          value={overlayOffsets[key]}
          aria-label={`${axis} offset`}
          onChange={e => { const n = e.target.valueAsNumber; if (!Number.isNaN(n)) setAxis(key, n); }}
        />
        <div className="grid grid-rows-2 border-l border-border">
          <button type="button" className={STEP_BTN_CLASS} aria-label={`Increase ${axis} offset`} onClick={() => setAxis(key, overlayOffsets[key] + 10)}>▲</button>
          <button type="button" className={STEP_BTN_CLASS} aria-label={`Decrease ${axis} offset`} onClick={() => setAxis(key, overlayOffsets[key] - 10)}>▼</button>
        </div>
      </div>
    );
    return (
      <div className={`${ROW_CLASS} mt-2${disabledClass(disabled)}`}>
        <div className={ROW_INFO_CLASS}>
          <span className={ROW_LABEL_CLASS}>Position offset</span>
          <span className={ROW_DESC_CLASS}>Moves the overlay this many pixels from its built-in spot. 0 keeps the current placement.</span>
        </div>
        <div className="flex shrink-0 items-center gap-1">
          <span className="w-2.25 text-center text-11 text-muted">X</span>
          {axisInput(keyX, "X")}
          <span className="w-2.25 text-center text-11 text-muted">Y</span>
          {axisInput(keyY, "Y")}
          <SecondaryButton className="min-w-14 h-6.5" disabled={overlayOffsets[keyX] === 0 && overlayOffsets[keyY] === 0}
            onClick={resetOffsets}>Reset</SecondaryButton>
        </div>
      </div>
    );
  };

  return (
      <div className="fixed inset-0 z-300 flex items-center justify-center bg-black/60 p-5" onClick={() => onClose()}>
        <div
          className={`flex w-[min(849px,95vw)] flex-col rounded-12 border border-border bg-surface shadow-[0_20px_60px_rgba(0,0,0,.6)] settings-modal-${settingsTab}`}
          style={{ height: "calc(90vh / var(--ff-scale, 1))", maxHeight: "calc(90vh / var(--ff-scale, 1))" }}
          onClick={e => e.stopPropagation()}
        >
          <div className="flex shrink-0 items-center justify-between border-b border-border px-5 py-4">
            <span className="text-15 font-semibold">Settings</span>
            <ModalCloseButton onClick={() => onClose()}>✕</ModalCloseButton>
          </div>

          <div className="flex min-h-0 flex-1">
            {/* ── Sidebar nav ── */}
            <nav className="flex min-w-32.5 shrink-0 flex-col gap-0.5 border-r border-border px-2 py-2.5">
              {(["general", "overlays", "market", "filters", "accessibility", "notifications", "data", "debugging"] as const).map(tab => (
                <button
                  key={tab}
                  className={`cursor-pointer rounded-6 border-none px-3 py-1.75 text-left text-13 transition-colors duration-120 ${settingsTab === tab ? "bg-accent/15 font-semibold text-accent" : "bg-transparent text-muted hover:bg-white/6 hover:text-foreground"}`}
                  onClick={() => setSettingsTab(tab)}
                >
                  {tab.charAt(0).toUpperCase() + tab.slice(1)}
                </button>
              ))}
            </nav>

            {/* ── Tab content ── */}
            <div className="flex flex-1 flex-col overflow-y-auto py-2">

              {/* ════════════ GENERAL ════════════ */}
              {settingsTab === "general" && <>

                {/* Foundry */}
                <div className={SECTION_CLASS}>
                  <div className={SECTION_TITLE_CLASS}>Foundry</div>
                  <div className={ROW_CLASS}>
                    <div className={ROW_INFO_CLASS}>
                      <span className={ROW_LABEL_CLASS}>Items per page</span>
                      <span className={ROW_DESC_CLASS}>How many items to show per page in the Foundry browser.</span>
                    </div>
                    <select className={SELECT_CLASS} value={foundryPageSize}
                      onChange={e => {
                        const next = Number(e.target.value) as FoundryPageSize;
                        setFoundryPageSize(next);
                        settingsRef.current = { ...settingsRef.current, foundryPageSize: next };
                        saveAllSettings();
                      }}>
                      {FOUNDRY_PAGE_SIZE_OPTIONS.map(size => <option key={size} value={size}>{size}</option>)}
                    </select>
                  </div>
                </div>

                {/* Memory Scanner */}
                <div className={`${SECTION_CLASS}${memoryScannerEnabled ? " border-ducat/30" : ""}`}>
                  <div className={`${SECTION_TITLE_CLASS} flex items-center gap-2`}>
                    Memory Scanner
                    <span className="rounded-3 border border-ducat/35 bg-ducat/15 px-1.5 py-px text-10 font-bold text-ducat">
                      EULA GREY AREA
                    </span>
                  </div>
                  <div className="mb-2 text-11 leading-normal text-muted">
                    Reads live inventory, crafting jobs, and mod ranks from Warframe's process memory via <code className="text-10">ReadProcessMemory</code>. DE has historically tolerated read-only tools, but has not given explicit permission. Enable at your own risk.
                  </div>
                  <div className={ROW_CLASS}>
                    <div>
                      <span className={ROW_LABEL_CLASS}>Enable</span>
                      <span className={ROW_DESC_CLASS}>Required for live inventory, quantity tracking, and mod ranks</span>
                    </div>
                    <SecondaryButton
                      className={`${TOGGLE_BUTTON_CLASS}${memoryScannerEnabled ? " border-ducat! bg-ducat/15! text-ducat!" : ""}`}
                        onClick={() => setMemoryScannerEnabled(v => !v)}
                    >
                      {memoryScannerEnabled ? "On" : "Off"}
                    </SecondaryButton>
                  </div>
                </div>

                {/* Warframe API */}
                <div className={SECTION_CLASS}>
                  <div className={`${SECTION_TITLE_CLASS} flex items-center gap-2`}>
                    Warframe API
                    <span className="rounded-3 border border-ducat/35 bg-ducat/15 px-1.5 py-px text-10 font-bold text-ducat">
                      SUSPENDED
                    </span>
                  </div>
                  <div className="rounded-6 border border-ducat/25 bg-ducat/6 px-2.5 py-2 text-11 leading-1.6 text-muted">
                    <strong className="text-ducat">Temporarily unavailable.</strong>
                    {" "}This feature connects to an undocumented DE endpoint (<code className="text-10">api.warframe.com/api/inventory.php</code>).
                    {" "}DE confirmed third-party tools run at your own risk but could not clarify whether this specific endpoint is permitted.
                    {" "}The feature is disabled until we receive clearer guidance.
                  </div>
                </div>

                {/* Account Login */}
                <div className={SECTION_CLASS}>
                  <div className={SECTION_TITLE_CLASS}>Account Login</div>
                  <div className="rounded-6 border border-notice-error/20 bg-notice-error/7 px-2.5 py-2 text-11 leading-1.6 text-muted">
                    <strong className="text-notice-error-ink">Login is temporarily unavailable.</strong>
                    {" "}Digital Extremes encrypted their login API in March 2026, which blocked all third-party tools — including FrameForge — from authenticating on your behalf.
                    {" "}PC players are not affected: inventory is synced automatically while the game is running.
                  </div>
                  <div className="mt-2 rounded-6 border border-notice-info/18 bg-notice-info/6 px-2.5 py-2 text-11 leading-1.6 text-muted">
                    FrameForge is actively exploring ways to restore inventory access for console and non-PC players.
                    {" "}Follow the project for updates.
                  </div>
                </div>

                {/* Modular Window */}
                <div className={SECTION_CLASS}>
                  <div className={SECTION_TITLE_CLASS}>Modular Window</div>
                  <div className={ROW_CLASS}>
                    <div className={ROW_INFO_CLASS}>
                      <span className={ROW_LABEL_CLASS}>Pop-out</span>
                      <span className={ROW_DESC_CLASS}>Detach the Modular Window into its own floating window.</span>
                    </div>
                    <SecondaryButton
                      className={toggleButtonClass(modularPopout)}
                      onClick={() => {
                        const next = !modularPopout;
                        setModularPopout(next);
                        settingsRef.current = { ...settingsRef.current, modularPopout: next };
                        saveAllSettings();
                      }}
                    >{modularPopout ? "On" : "Off"}</SecondaryButton>
                  </div>
                </div>

              </>}

              {/* ════════════ OVERLAYS ════════════ */}
              {settingsTab === "overlays" && <>

                {/* Relic Overlay */}
                <div className={SECTION_CLASS}>
                  <div className={SECTION_TITLE_CLASS}>Relic Reward Overlay</div>
                  {overlayStatus && (
                    <div className="mb-1.5 rounded-4 bg-white/5 px-2 py-1 font-mono text-12 text-connected">
                      {overlayStatus}
                    </div>
                  )}
                  <div className={ROW_CLASS}>
                    <div className={ROW_INFO_CLASS}>
                      <span className={ROW_LABEL_CLASS}>Enable</span>
                      <span className={ROW_DESC_CLASS}>Auto-shows reward cards when a Void Fissure screen is detected.</span>
                    </div>
                    <SecondaryButton
                      className={toggleButtonClass(overlayEnabled)}
                      onClick={() => {
                        const next = !overlayEnabled;
                        setOverlayEnabled(next);
                        localStorage.setItem(PREFERENCE_KEYS.OVERLAY_ENABLED, String(next));
                        settingsRef.current = { ...settingsRef.current, overlayEnabled: next };
                        saveAllSettings();
                        if (!next) {
                          emit(TAURI_EVENTS.RELIC_SCREEN, true).catch(() => {});
                        }
                      }}
                    >{overlayEnabled ? "On" : "Off"}</SecondaryButton>
                  </div>
                  <div className={ROW_CLASS + " mt-2"}>
                    <div className={ROW_INFO_CLASS}>
                      <span className={ROW_LABEL_CLASS}>Pick priority</span>
                      <span className={ROW_DESC_CLASS}>Which card the overlay highlights as the best pick.</span>
                    </div>
                    <select
                      className={SELECT_CLASS}
                      value={overlayPriority}
                      disabled={!overlayEnabled}
                      onChange={e => {
                        const next = e.target.value as RelicOverlayPriority;
                        setOverlayPriority(next);
                        localStorage.setItem(PREFERENCE_KEYS.OVERLAY_PRIORITY, next);
                        settingsRef.current = { ...settingsRef.current, overlayPriority: next };
                        saveAllSettings();
                      }}
                    >
                      {RELIC_OVERLAY_PRIORITY_OPTIONS.map(priority => (
                        <option key={priority} value={priority}>{priority === "completion" ? "Item Completion" : priority === "setPlat" ? "Most Set Value (plat)" : priority === "plat" ? "Most Plat (item)" : "Most Ducats"}</option>
                      ))}
                    </select>
                  </div>
                  {offsetRow("relicX", "relicY", !overlayEnabled)}
                  <div className={ROW_CLASS + " mt-2"}>
                    <div className={ROW_INFO_CLASS}>
                      <span className={ROW_LABEL_CLASS}>Show Outline</span>
                      <span className={ROW_DESC_CLASS}>Toggle a dashed outline of the overlay at its current position.</span>
                    </div>
                    <SecondaryButton className={toggleButtonClass(outlineReward)}
                      onClick={() => {
                        if (outlineReward) { hideRewardOutline().catch(() => {}); setOutlineReward(false); }
                        else { showRewardOutline().catch(() => {}); setOutlineReward(true); }
                      }}>{outlineReward ? "Hide" : "Show"}</SecondaryButton>
                  </div>
                </div>

                {/* Relic Pick Overlay */}
                <div className={SECTION_CLASS}>
                  <div className={SECTION_TITLE_CLASS}>Relic Pick Overlay</div>
                  <div className={ROW_CLASS}>
                    <div className={ROW_INFO_CLASS}>
                      <span className={ROW_LABEL_CLASS}>Enable</span>
                      <span className={ROW_DESC_CLASS}>Show the relic pick overlay when opening the relic selection screen.</span>
                    </div>
                    <SecondaryButton
                      className={toggleButtonClass(relicPickEnabled)}
                      onClick={() => {
                        const next = !relicPickEnabled;
                        setRelicPickEnabled(next);
                        settingsRef.current = { ...settingsRef.current, relicPickEnabled: next };
                        saveAllSettings();
                        invoke(TAURI_COMMANDS.SET_RELIC_PICK_ENABLED, { enabled: next });
                      }}
                    >{relicPickEnabled ? "On" : "Off"}</SecondaryButton>
                  </div>
                  <div className={`${ROW_CLASS} mt-2${disabledClass(!relicPickEnabled)}`}>
                    <div className={ROW_INFO_CLASS}>
                      <span className={ROW_LABEL_CLASS}>Recommendation Base</span>
                      <span className={ROW_DESC_CLASS}>How relics are ranked in the overlay.</span>
                    </div>
                    <select className={SELECT_CLASS} value={relicPickPriority}
                      onChange={e => {
                        const next = e.target.value as RelicPickPriority;
                        setRelicPickPriority(next);
                        settingsRef.current = { ...settingsRef.current, relicPickPriority: next };
                        saveAllSettings();
                      }}>
                      {RELIC_PICK_PRIORITY_OPTIONS.map(priority => (
                        <option key={priority} value={priority}>{priority === "unowned" ? "Unowned / Mastery" : priority === "ducat" ? "Most Ducats (EV)" : "Most Platinum (EV)"}</option>
                      ))}
                    </select>
                  </div>
                  <div className={`${ROW_CLASS} mt-2${disabledClass(!relicPickEnabled)}`}>
                    <div className={ROW_INFO_CLASS}>
                      <span className={ROW_LABEL_CLASS}>Shown Lines Per Relic</span>
                      <span className={ROW_DESC_CLASS}>How much reward detail to show per relic card.</span>
                    </div>
                    <select className={SELECT_CLASS} value={relicPickLines}
                      onChange={e => {
                        const next = e.target.value as RelicPickLines;
                        setRelicPickLines(next);
                        settingsRef.current = { ...settingsRef.current, relicPickLines: next };
                        saveAllSettings();
                      }}>
                      {RELIC_PICK_LINES_OPTIONS.map(lines => (
                        <option key={lines} value={lines}>{lines === "all" ? "All items" : lines === "best" ? "Only most valuable" : "Score summary only"}</option>
                      ))}
                    </select>
                  </div>
                  {offsetRow("relicPickX", "relicPickY", !relicPickEnabled)}
                  <div className={`${ROW_CLASS} mt-2`}>
                    <div className={ROW_INFO_CLASS}>
                      <span className={ROW_LABEL_CLASS}>Show Outline</span>
                      <span className={ROW_DESC_CLASS}>Toggle a dashed outline of the overlay at its current position.</span>
                    </div>
                    <SecondaryButton className={toggleButtonClass(outlinePick)}
                      onClick={() => {
                        if (outlinePick) { hidePickOutline().catch(() => {}); setOutlinePick(false); }
                        else { showPickOutline().catch(() => {}); setOutlinePick(true); }
                      }}>{outlinePick ? "Hide" : "Show"}</SecondaryButton>
                  </div>
                </div>

                {/* Riven Overlay */}
                <div className={SECTION_CLASS}>
                  <div className={SECTION_TITLE_CLASS}>Riven Overlay</div>
                  <div className={ROW_CLASS}>
                    <div className={ROW_INFO_CLASS}>
                      <span className={ROW_LABEL_CLASS}>Enable</span>
                      <span className={ROW_DESC_CLASS}>Show the riven overlay when a riven card screen is detected.</span>
                    </div>
                    <SecondaryButton
                      className={toggleButtonClass(rivenEnabled)}
                      onClick={() => {
                        const next = !rivenEnabled;
                        setRivenEnabled(next);
                        localStorage.setItem(PREFERENCE_KEYS.RIVEN_OVERLAY_ENABLED, String(next));
                        settingsRef.current = { ...settingsRef.current, rivenEnabled: next };
                        saveAllSettings();
                      }}
                    >{rivenEnabled ? "On" : "Off"}</SecondaryButton>
                  </div>
                  {offsetRow("rivenX", "rivenY", !rivenEnabled)}
                  <div className={`${ROW_CLASS} mt-2`}>
                    <div className={ROW_INFO_CLASS}>
                      <span className={ROW_LABEL_CLASS}>Show</span>
                      <span className={ROW_DESC_CLASS}>Show the riven overlay at its current position.</span>
                    </div>
                    <SecondaryButton className={toggleButtonClass(rivenShown)}
                      onClick={() => {
                        if (rivenShown) { hideRivenOverlay().catch(() => {}); setRivenShown(false); }
                        else { showRivenOverlay().catch(() => {}); setRivenShown(true); }
                      }}>{rivenShown ? "Hide" : "Show"}</SecondaryButton>
                  </div>
                </div>

                {/* Relic Overlay — Memory Trigger */}
                <div className={SECTION_CLASS}>
                  <div className={SECTION_TITLE_CLASS}>Memory Trigger <span className="ml-1.5 text-11 font-normal opacity-55">in development</span></div>
                  <div className={ROW_CLASS}>
                    <div className={ROW_INFO_CLASS}>
                      <span className={ROW_LABEL_CLASS}>Use memory scan</span>
                      <span className={ROW_DESC_CLASS}>
                        Still in development — for testing only. Polls Warframe's process memory for the reward screen event in parallel with EE.log.
                        Timing for both paths is written to the session log so they can be compared.
                        The EE.log overlay is unaffected regardless of this setting.
                      </span>
                    </div>
                    <SecondaryButton
                      className={toggleButtonClass(memTriggerEnabled)}
                      onClick={() => {
                        const next = !memTriggerEnabled;
                        setMemTriggerEnabled(next);
                        settingsRef.current = { ...settingsRef.current, memTriggerEnabled: next };
                        saveAllSettings();
                        invoke(TAURI_COMMANDS.SET_MEM_TRIGGER_ENABLED, { enabled: next });
                      }}
                    >{memTriggerEnabled ? "On" : "Off"}</SecondaryButton>
                  </div>
                </div>

              </>}

              {/* ════════════ MARKET ════════════ */}
              {settingsTab === "market" && <>
                <div className={SECTION_CLASS}>
                  <div className={SECTION_TITLE_CLASS}>Bulk Prices</div>
                  <div className={ROW_CLASS}>
                    <div className={ROW_INFO_CLASS}>
                      <span className={ROW_LABEL_CLASS}>Force Refresh</span>
                      <span className={ROW_DESC_CLASS}>Re-download price data from FrameForgePricing right now. Use this if prices look stale or missing.</span>
                    </div>
                    <BulkPriceRefreshButton />
                  </div>
                </div>
                <div className={SECTION_CLASS}>
                  <div className={SECTION_TITLE_CLASS}>Status Automation</div>
                  {!wfmLoggedIn && (
                    <div className="mb-2.5 rounded-5 bg-white/4 px-2.5 py-1.5 text-11 leading-normal text-muted">
                      Log in to warframe.market in the <strong>Market</strong> tab to enable these features.
                    </div>
                  )}
                  <div className={ROW_CLASS + disabledClass(!wfmLoggedIn)}>
                    <div className={ROW_INFO_CLASS}>
                      <span className={ROW_LABEL_CLASS}>Go Invisible on startup</span>
                      <span className={ROW_DESC_CLASS}>When FrameForge opens, immediately set your WFM status to Invisible.</span>
                    </div>
                    <SecondaryButton
                      className={toggleButtonClass(wfmInvisibleOnStart)}
                      onClick={() => {
                        const next = !wfmInvisibleOnStart;
                        setWfmInvisibleOnStart(next);
                        wfmInvisibleOnStartRef.current = next;
                        settingsRef.current = { ...settingsRef.current, wfmInvisibleOnStart: next };
                        saveAllSettings();
                      }}
                    >{wfmInvisibleOnStart ? "On" : "Off"}</SecondaryButton>
                  </div>

                  <div className={`${ROW_CLASS} mt-2${disabledClass(!wfmLoggedIn)}`}>
                    <div className={ROW_INFO_CLASS}>
                      <span className={ROW_LABEL_CLASS}>Go Invisible on close</span>
                      <span className={ROW_DESC_CLASS}>Before FrameForge exits (X button or taskbar close), set your WFM status to Invisible.</span>
                    </div>
                    <SecondaryButton
                      className={toggleButtonClass(wfmInvisibleOnClose)}
                      onClick={() => {
                        const next = !wfmInvisibleOnClose;
                        setWfmInvisibleOnClose(next);
                        wfmInvisibleOnCloseRef.current = next;
                        settingsRef.current = { ...settingsRef.current, wfmInvisibleOnClose: next };
                        saveAllSettings();
                      }}
                    >{wfmInvisibleOnClose ? "On" : "Off"}</SecondaryButton>
                  </div>

                  <div className={`${ROW_CLASS} mt-2${disabledClass(!wfmLoggedIn)}`}>
                    <div className={ROW_INFO_CLASS}>
                      <span className={ROW_LABEL_CLASS}>Auto-invisible timer</span>
                      <span className={ROW_DESC_CLASS}>
                        After{" "}
                        <input
                          type="number" min={1} max={480} value={wfmAutoInvisibleMins}
                          disabled={!wfmAutoInvisible}
                          className="w-12 rounded-4 border border-border bg-surface px-1 py-px text-center text-12 text-foreground"
                          onChange={e => {
                            const v = Math.max(1, Math.min(480, parseInt(e.target.value) || 30));
                            setWfmAutoInvisibleMins(v);
                            settingsRef.current = { ...settingsRef.current, wfmAutoInvisibleMins: v };
                            saveAllSettings();
                          }}
                        />{" "}
                        minutes, automatically set status to Invisible.
                      </span>
                    </div>
                    <SecondaryButton
                      className={toggleButtonClass(wfmAutoInvisible)}
                      onClick={() => {
                        const next = !wfmAutoInvisible;
                        setWfmAutoInvisible(next);
                        settingsRef.current = { ...settingsRef.current, wfmAutoInvisible: next };
                        saveAllSettings();
                      }}
                    >{wfmAutoInvisible ? "On" : "Off"}</SecondaryButton>
                  </div>
                </div>
                <div className={SECTION_CLASS}>
                  <div className={SECTION_TITLE_CLASS}>Sale Automation</div>
                  <div className={ROW_CLASS + disabledClass(!wfmLoggedIn)}>
                    <div className={ROW_INFO_CLASS}>
                      <span className={ROW_LABEL_CLASS}>Record automatic sales</span>
                      <span className={ROW_DESC_CLASS}>Record automatically detected sales in your warframe.market statistics. When off, FrameForge only reduces or removes the listing. Recorded sales cannot be undone from FrameForge.</span>
                    </div>
                    <SecondaryButton
                      className={toggleButtonClass(wfmRecordSales)}
                      onClick={() => {
                        const next = !wfmRecordSales;
                        setWfmRecordSales(next);
                        settingsRef.current = { ...settingsRef.current, wfmRecordSales: next };
                        saveAllSettings();
                      }}
                    >{wfmRecordSales ? "On" : "Off"}</SecondaryButton>
                  </div>
                </div>
              </>}

              {/* ════════════ FILTERS ════════════ */}
              {settingsTab === "filters" && <>
                <div className={SECTION_CLASS}>
                  <div className={SECTION_TITLE_CLASS}>Filter Presets</div>
                  <div className={ROW_CLASS}>
                    <div className={ROW_INFO_CLASS}>
                      <span className={ROW_LABEL_CLASS}>Page</span>
                      <span className={ROW_DESC_CLASS}>Manage saved and pinned presets for one page.</span>
                    </div>
                    <select className={SELECT_CLASS} value={settingsFilterModule} onChange={event => setSettingsFilterModule(event.target.value as FilterPresetModule)}>
                      <option value="inventory">Inventory</option>
                      <option value="foundry">Foundry</option>
                      <option value="market">Market Sets</option>
                      <option value="relics">Relic Browser</option>
                    </select>
                  </div>
                  <div className={ROW_CLASS + " mt-3"}>
                    <div className={ROW_INFO_CLASS}>
                      <span className={ROW_LABEL_CLASS}>Restore previous filters on active preset click</span>
                      <span className={ROW_DESC_CLASS}>When clicking an active pinned preset again, restore the filters from before it was applied instead of clearing filters.</span>
                    </div>
                    <SecondaryButton className={toggleButtonClass(filterPresets.restorePreviousFiltersOnPresetClick)} onClick={() => setFilterPresets(current => ({ ...current, restorePreviousFiltersOnPresetClick: !current.restorePreviousFiltersOnPresetClick }))}>{filterPresets.restorePreviousFiltersOnPresetClick ? "On" : "Off"}</SecondaryButton>
                  </div>
                </div>
                {settingsFilterModule === "inventory" && <FilterPresets variant="settings" module="inventory" filters={inventoryFilters} onFiltersChange={setInventoryFilters} filterPresets={filterPresets} onFilterPresetsChange={setFilterPresets} />}
                {settingsFilterModule === "foundry" && <FilterPresets variant="settings" module="foundry" filters={foundryFilters} onFiltersChange={setFoundryFilters} filterPresets={filterPresets} onFilterPresetsChange={setFilterPresets} />}
                {settingsFilterModule === "market" && <FilterPresets variant="settings" module="market" filters={marketFilters} onFiltersChange={setMarketFilters} filterPresets={filterPresets} onFilterPresetsChange={setFilterPresets} />}
                {settingsFilterModule === "relics" && <FilterPresets variant="settings" module="relics" filters={relicFilters} onFiltersChange={setRelicFilters} filterPresets={filterPresets} onFilterPresetsChange={setFilterPresets} />}
              </>}

              {/* ════════════ ACCESSIBILITY ════════════ */}
              {settingsTab === "accessibility" && <>
                <div className={SECTION_CLASS}>
                  <div className={SECTION_TITLE_CLASS}>Appearance</div>
                  <div className={ROW_CLASS}>
                    <div className={ROW_INFO_CLASS}>
                      <span className={ROW_LABEL_CLASS}>Colorblind Mode</span>
                      <span className={ROW_DESC_CLASS}>Adds ✓ / ✓✓ symbols to relic reward boxes so status doesn't rely on color alone.</span>
                    </div>
                    <SecondaryButton
                      className={toggleButtonClass(colorblindMode)}
                      onClick={() => {
                        const next = !colorblindMode;
                        setColorblindMode(next);
                        localStorage.setItem(PREFERENCE_KEYS.COLORBLIND_MODE, String(next));
                        settingsRef.current = { ...settingsRef.current, colorblindMode: next };
                        saveAllSettings();
                      }}
                    >{colorblindMode ? "On" : "Off"}</SecondaryButton>
                  </div>
                  <div className={ROW_CLASS + " mt-2"}>
                    <div className={ROW_INFO_CLASS}>
                      <span className={ROW_LABEL_CLASS}>Text Size</span>
                      <span className={ROW_DESC_CLASS}>{Math.round(textScale * 100)}%</span>
                    </div>
                    <input type="range" min="0.8" max="2.0" step="0.1" value={textScale}
                      className="w-30"
                      onChange={e => {
                        const v = parseFloat(e.target.value);
                        setTextScale(v);
                        document.documentElement.style.setProperty("--ff-scale", v.toString());
                        localStorage.setItem(PREFERENCE_KEYS.TEXT_SCALE, v.toString());
                        settingsRef.current = { ...settingsRef.current, textScale: v };
                        saveAllSettings();
                      }} />
                  </div>
                  <div className={ROW_CLASS + " mt-2"}>
                    <div className={ROW_INFO_CLASS}>
                      <span className={ROW_LABEL_CLASS}>Clock Format</span>
                      <span className={ROW_DESC_CLASS}>How times are displayed throughout the app.{clockFormat === "auto" ? " System locale: " + systemLocale : ""}</span>
                    </div>
                    <div className="flex gap-1">
                      {CLOCK_FORMAT_OPTIONS.map(f => (
                        <SecondaryButton key={f} className={"min-w-11" + (clockFormat === f ? " border-accent! bg-accent/15!" : "")}
                          onClick={() => {
                            setClockFormat(f);
                            settingsRef.current = { ...settingsRef.current, clockFormat: f };
                            saveAllSettings();
                          }}>
                          {f === "auto" ? "Auto" : f}
                        </SecondaryButton>
                      ))}
                    </div>
                  </div>
                </div>
              </>}

              {/* ════════════ NOTIFICATIONS ════════════ */}
              {settingsTab === "notifications" && <>
                <div className={SECTION_CLASS}>
                  <div className={SECTION_TITLE_CLASS}>Fissure Watches</div>
                  <div className={ROW_CLASS}>
                    <div className={ROW_INFO_CLASS}>
                      <span className={ROW_LABEL_CLASS}>Enable</span>
                      <span className={ROW_DESC_CLASS}>Desktop notification when a fissure matching one of your Timers watches appears.</span>
                    </div>
                    <SecondaryButton
                      className={toggleButtonClass(fissureNotifications) + (fissureNotifications ? " text-accent!" : "")}
                      onClick={async () => {
                        if (fissureNotifications) { onFissureNotificationsChange(false); return; }
                        // Turn it back off when the OS refuses, rather than leaving
                        // it on promising alerts that cannot arrive.
                        const granted = await ensurePermission();
                        onFissureNotificationsChange(granted);
                        setNotifPermissionDenied(!granted);
                      }}
                    >
                      {fissureNotifications ? "On" : "Off"}
                    </SecondaryButton>
                  </div>
                  {notifPermissionDenied && (
                    <div className="mt-1.5 text-11 text-danger">Notifications are blocked for FrameForge in your system settings.</div>
                  )}
                </div>

                <div className={SECTION_CLASS}>
                  <div className={SECTION_TITLE_CLASS}>Diagnostics</div>
                  <div className={ROW_CLASS}>
                    <div className={ROW_INFO_CLASS}>
                      <span className={ROW_LABEL_CLASS}>Test Notification</span>
                      <span className={ROW_DESC_CLASS}>
                        Send a desktop notification now, to check the OS delivers them at all.
                        {notifyTestResult && <span className={"mt-0.5 block text-11 " + (notifyTestResult.startsWith("Sent") ? "text-green" : "text-danger")}>{notifyTestResult}</span>}
                      </span>
                    </div>
                    <SecondaryButton onClick={async () => {
                      setNotifyTestResult("");
                      if (!(await ensurePermission())) {
                        setNotifyTestResult("Permission denied — notifications are blocked for FrameForge in your system settings.");
                        return;
                      }
                      await notify("FrameForge", "Test notification — watched fissure alerts will look like this.");
                      setNotifyTestResult("Sent. If nothing appeared, the OS notification daemon is dropping it.");
                    }}>Send</SecondaryButton>
                  </div>
                  <div className="mt-1.5 text-11 leading-normal text-muted">
                    If "Sent" shows but nothing appears, check Windows Settings → System → Notifications and make sure the master "Notifications" toggle at the top is on — Windows drops every toast silently when it's off, with no error here and nothing in Action Center.
                  </div>
                </div>
              </>}

              {/* ════════════ DATA ════════════ */}
              {settingsTab === "data" && <>
                <div className={SECTION_CLASS}>
                  <div className={SECTION_TITLE_CLASS}>Item Database</div>
                  <div className={ROW_CLASS}>
                    <div className={ROW_INFO_CLASS}>
                      <span className={ROW_LABEL_CLASS}>Catalog</span>
                      <span className={ROW_DESC_CLASS}>{itemCount.toLocaleString()} items · {recipeCount.toLocaleString()} recipes cached</span>
                    </div>
                    <SecondaryButton onClick={() => { onClose(); handleFetch(); }} disabled={fetching}>
                      {fetching ? "Fetching…" : "Refresh"}
                    </SecondaryButton>
                  </div>
                  {fetchMsg && <div className={SECTION_MSG_CLASS}>{fetchMsg}</div>}
                </div>
                <div className={SECTION_CLASS}>
                  <div className={SECTION_TITLE_CLASS}>Inventory Cache</div>
                  <div className={ROW_CLASS}>
                    <div className={ROW_INFO_CLASS}>
                      <span className={ROW_LABEL_CLASS}>Clear Cache</span>
                      <span className={ROW_DESC_CLASS}>Reset all scanned quantities and changelog.</span>
                    </div>
                    <DangerButton
                      onClick={async () => {
                        try {
                          await invoke("clear_cache");
                          setQuantities({});
                          setApiQuantities({});
                          setApiModCopies([]);
                          setScannerMods({});
                          setMasteryData({});
                          setArchonShards({});
                          setFormaData({});
                          setChangeLog([]);
                          setLastChanged({});
                          setWfConnected(false);
                          wfConnectedRef.current = false;
                           const args: SaveApiInventoryArgs = { apiQuantities: {}, apiModCopies: [], consumedSuits: [] };
                           invoke(TAURI_COMMANDS.SAVE_API_INVENTORY, args).catch(() => {});
                            setItemsRefreshKey(k => k + 1);
                          setClearMsg("Cache cleared.");
                        } catch (e) { setClearMsg("Error: " + e); }
                      }}
                    >Clear Cache</DangerButton>
                  </div>
                  {clearMsg && <div className={SECTION_MSG_CLASS}>{clearMsg}</div>}
                </div>
                <div className={SECTION_CLASS + " border-field-error/30"}>
                  <div className={SECTION_TITLE_CLASS + " text-danger"}>Factory Reset</div>
                  <div className={ROW_CLASS}>
                    <div className={ROW_INFO_CLASS}>
                      <span className={ROW_LABEL_CLASS}>Reset Everything</span>
                      <span className={ROW_DESC_CLASS}>Delete all app data — settings, inventory cache, trade log, market prices, WFM login — and restart. Cannot be undone.</span>
                    </div>
                    <FactoryResetButton />
                  </div>
                </div>
              </>}

              {/* ════════════ DEBUGGING ════════════ */}
              {settingsTab === "debugging" && <>

                <div className={SECTION_CLASS}>
                  <div className={SECTION_TITLE_CLASS}>Loggers</div>
                  <div className={DEBUG_TABLE_CLASS}>

                    {/* Inventory Snapshots */}
                    <div className={ROW_INFO_CLASS + disabledClass(!memoryScannerEnabled, true)}>
                      <span className={ROW_LABEL_CLASS}>Inventory Snapshots</span>
                      <span className={ROW_DESC_CLASS}>Saves a JSON snapshot on each memory scan.</span>
                    </div>
                    <SecondaryButton className={disabledClass(!memoryScannerEnabled, true)}
                      onClick={() => invoke("open_debug_folder", { which: "blobs" }).catch(() => {})}>Go To Folder</SecondaryButton>
                    <SecondaryButton
                      className={toggleButtonClass(blobLogEnabled) + disabledClass(!memoryScannerEnabled, true)}
                        onClick={() => setBlobLogEnabled(v => !v)}>{blobLogEnabled ? "On" : "Off"}</SecondaryButton>
                    <SecondaryButton
                      className={clearButtonClass(blobLogSize > 0) + disabledClass(!memoryScannerEnabled, true)}
                      disabled={blobLogSize === 0}
                      onClick={async () => { await invoke("clear_debug_data", { which: "blobs" }); setBlobLogSize(0); }}
                    >{blobLogSize > 0 ? "Clear (" + formatBytes(blobLogSize) + ")" : "Clear"}</SecondaryButton>

                    {/* API Responses */}
                    <div className={ROW_INFO_CLASS + disabledClass(!companionApiEnabled, true)}>
                      <span className={ROW_LABEL_CLASS}>API Responses</span>
                      <span className={ROW_DESC_CLASS}>Records raw DE API responses on each inventory fetch.</span>
                    </div>
                    <SecondaryButton className={disabledClass(!companionApiEnabled, true)}
                      onClick={() => invoke("open_debug_folder", { which: "api_logs" }).catch(() => {})}>Go To Folder</SecondaryButton>
                    <SecondaryButton
                      className={toggleButtonClass(apiLogEnabled) + disabledClass(!companionApiEnabled, true)}
                        onClick={() => setApiLogEnabled(v => !v)}>{apiLogEnabled ? "On" : "Off"}</SecondaryButton>
                    <SecondaryButton
                      className={clearButtonClass(apiLogSize > 0) + disabledClass(!companionApiEnabled, true)}
                      disabled={apiLogSize === 0}
                      onClick={async () => { await invoke("clear_debug_data", { which: "api_logs" }); setApiLogSize(0); }}
                    >{apiLogSize > 0 ? "Clear (" + formatBytes(apiLogSize) + ")" : "Clear"}</SecondaryButton>

                  </div>
                </div>

                <div className={SECTION_CLASS}>
                  <div className={SECTION_TITLE_CLASS}>Inventory Preview</div>
                  <div className={ROW_CLASS}>
                    <div className={ROW_INFO_CLASS}>
                      <span className={ROW_LABEL_CLASS}>Incoming Batch</span>
                      <span className={ROW_DESC_CLASS}>Preview gained, lost, crafting, and mod rank changes without modifying your inventory.</span>
                    </div>
                    <SecondaryButton onClick={() => setShowInventoryBatchPreview(true)}>Preview</SecondaryButton>
                  </div>
                </div>

                <div className={SECTION_CLASS}>
                  <div className={SECTION_TITLE_CLASS}>Diagnostics</div>
                  <div className={DEBUG_TABLE_CLASS}>

                    {/* Overlay Log */}
                    <div className={ROW_INFO_CLASS}>
                      <span className={ROW_LABEL_CLASS}>Overlay Log</span>
                      <span className={ROW_DESC_CLASS}>Step-by-step log of the last relic overlay attempt.</span>
                    </div>
                    <div />{/* Go To Folder placeholder */}
                    <SecondaryButton onClick={async () => {
                      try { alert(await invoke<string>("get_overlay_session_log")); }
                      catch (e) { alert("Error: " + e); }
                    }}>View</SecondaryButton>
                    <SecondaryButton onClick={async () => {
                      try {
                        const log = await invoke<string>("get_overlay_session_log");
                        navigator.clipboard.writeText(log).then(() => {
                          setOverlayLogCopied(true);
                          setTimeout(() => setOverlayLogCopied(false), 1500);
                        }).catch(() => {});
                      }
                      catch (e) { alert("Error: " + e); }
                    }}>{overlayLogCopied ? "✓ Copied" : "Copy"}</SecondaryButton>

                    {/* Auto-capture */}
                    <div className={ROW_INFO_CLASS}>
                      <span className={ROW_LABEL_CLASS}>Auto-capture</span>
                      <span className={ROW_DESC_CLASS}>Automatically saves a screenshot and OCR session log for every relic reward screen. One folder is created per relic in the diagnostics directory.</span>
                    </div>
                    <SecondaryButton onClick={() => invoke("open_debug_folder", { which: "diag" }).catch(() => {})}>Go To Folder</SecondaryButton>
                    <SecondaryButton
                      className={toggleButtonClass(autoDiagEnabled)}
                      onClick={() => {
                        const next = !autoDiagEnabled;
                        setAutoDiagEnabled(next);
                        localStorage.setItem(PREFERENCE_KEYS.AUTO_DIAGNOSTICS, String(next));
                        settingsRef.current = { ...settingsRef.current, autoDiagEnabled: next };
                        saveAllSettings();
                      }}>{autoDiagEnabled ? "On" : "Off"}</SecondaryButton>
                    <SecondaryButton
                      className={clearButtonClass(diagFolderSize > 0)}
                      disabled={diagFolderSize === 0}
                      onClick={async () => { await invoke("clear_diag_folder"); setDiagFolderSize(0); }}
                    >{diagFolderSize > 0 ? "Clear (" + formatBytes(diagFolderSize) + ")" : "Clear"}</SecondaryButton>

                    {/* Manual Capture */}
                    <div className={ROW_INFO_CLASS}>
                      <span className={ROW_LABEL_CLASS}>Manual Capture</span>
                      <span className={ROW_DESC_CLASS}>
                        Take a diagnostic screenshot + scan log right now.
                        {diagPath && <span className="mt-0.5 block text-11 text-green">Saved.</span>}
                      </span>
                    </div>
                    <SecondaryButton onClick={() => invoke("open_debug_folder", { which: "manual_capture" }).catch(() => {})}>Go To Folder</SecondaryButton>
                    <SecondaryButton disabled={diagCapturing}
                      onClick={async () => {
                        setDiagCapturing(true); setDiagPath(null);
                        try { const p = await invoke<string>("capture_diagnostics"); setDiagPath(p); reloadDebugSizes(); }
                        catch (e) { setDiagPath(`Error: ${e}`); }
                        finally { setDiagCapturing(false); }
                      }}>{diagCapturing ? "Working…" : "Capture"}</SecondaryButton>
                    <div />{/* Clear placeholder */}

                    {/* Memory Probe */}
                    <div className={ROW_INFO_CLASS}>
                      <span className={ROW_LABEL_CLASS}>Memory Probe</span>
                      <span className={ROW_DESC_CLASS}>Dumps inventory strings from Warframe's memory.</span>
                    </div>
                    <SecondaryButton onClick={() => invoke("open_debug_folder", { which: "probe" }).catch(() => {})}>Go To Folder</SecondaryButton>
                    <SecondaryButton disabled={memoryProbing} onClick={() => {
                      setMemoryProbing(true);
                      invoke<string>("dump_memory_probe")
                        .then(result => {
                          alert(`Probe complete — ${result.split("\n").filter(l => l.trim()).length} entries written.`);
                          reloadDebugSizes();
                        })
                        .catch(e => alert("Probe failed: " + String(e)))
                        .finally(() => setMemoryProbing(false));
                    }}>{memoryProbing ? "Running…" : "Run"}</SecondaryButton>
                    <SecondaryButton
                      className={clearButtonClass(probeSize > 0)}
                      disabled={probeSize === 0}
                      onClick={async () => { await invoke("clear_debug_data", { which: "probe" }); setProbeSize(0); }}
                    >{probeSize > 0 ? "Clear (" + formatBytes(probeSize) + ")" : "Clear"}</SecondaryButton>

                    {/* Raw Memory Record */}
                    <div className={ROW_INFO_CLASS}>
                      <span className={ROW_LABEL_CLASS}>Raw Memory Record</span>
                      <span className={ROW_DESC_CLASS}>{rawScanning ? "Recording — navigate in-game, then click Stop." : "Records all readable memory strings while you navigate in-game."}</span>
                    </div>
                    <SecondaryButton onClick={() => invoke("open_debug_folder", { which: "raw_scan" }).catch(() => {})}>Go To Folder</SecondaryButton>
                    <ActionButton variant={rawScanning ? "danger" : "secondary"}
                      onClick={() => {
                        invoke<string>("toggle_raw_scan")
                          .then(status => { const active = status === "started"; setRawScanning(active); if (!active) reloadDebugSizes(); })
                          .catch(e => alert("Error: " + String(e)));
                      }}>{rawScanning ? "Stop" : "Record"}</ActionButton>
                    <SecondaryButton
                      className={clearButtonClass(rawScanSize > 0)}
                      disabled={rawScanSize === 0 || rawScanning}
                      onClick={async () => { await invoke("clear_debug_data", { which: "raw_scan" }); setRawScanSize(0); }}
                    >{rawScanSize > 0 ? "Clear (" + formatBytes(rawScanSize) + ")" : "Clear"}</SecondaryButton>

                    {/* Memory Relic Debug */}
                    <div className={ROW_INFO_CLASS}>
                      <span className={ROW_LABEL_CLASS}>Memory Relic Debug</span>
                      <span className={ROW_DESC_CLASS}>{memRelicDebugRunning ? "Running — tailing EE.log and scanning Warframe memory. Log at %TEMP%\\frameforge_mem_relic_debug.log." : "Tails EE.log and scans Warframe memory for relic reward patterns. Writes everything to a log file."}</span>
                    </div>
                    <div />{/* no folder button */}
                    <ActionButton variant={memRelicDebugRunning ? "danger" : "secondary"}
                      onClick={async () => {
                        if (memRelicDebugRunning) {
                          await invoke("stop_memory_relic_debug").catch(() => {});
                          setMemRelicDebugRunning(false);
                        } else {
                          try {
                            const path = await invoke<string>("start_memory_relic_debug");
                            setMemRelicDebugRunning(true);
                            alert(`Memory relic debug started.\nLog: ${path}`);
                          } catch (e) { alert("Error: " + String(e)); }
                        }
                      }}>{memRelicDebugRunning ? "End" : "Start"}</ActionButton>
                    <div />{/* no clear button */}

                  </div>
                </div>

                <div className={SECTION_CLASS}>
                  <div className={SECTION_TITLE_CLASS}>Relic Pick Overlay</div>
                  <div className={DEBUG_TABLE_CLASS}>

                    {/* OCR Era Test */}
                    <div className={ROW_INFO_CLASS}>
                      <span className={ROW_LABEL_CLASS}>OCR Era Detect</span>
                      <span className={ROW_DESC_CLASS}>
                        Reads the top-left quarter of the Warframe window and reports which fissure era OCR finds.
                        {relicPickOcrResult && <span className="mt-0.5 block text-11 text-accent">{relicPickOcrResult}</span>}
                      </span>
                    </div>
                    <div />
                    <SecondaryButton disabled={relicPickOcrTesting} onClick={async () => {
                      setRelicPickOcrTesting(true); setRelicPickOcrResult(null);
                      try { setRelicPickOcrResult(await invoke<string>("debug_detect_fissure_era")); }
                      catch (e) { setRelicPickOcrResult(`Error: ${e}`); }
                      finally { setRelicPickOcrTesting(false); }
                    }}>{relicPickOcrTesting ? "Running…" : "Test OCR"}</SecondaryButton>
                    <div />

                    {/* Test Overlay */}
                    <div className={ROW_INFO_CLASS}>
                      <span className={ROW_LABEL_CLASS}>Test Overlay</span>
                      <span className={ROW_DESC_CLASS}>
                        Manually fire the relic pick overlay with a specific era.
                        {relicPickTestResult && <span className="mt-0.5 block text-11 text-accent">{relicPickTestResult}</span>}
                      </span>
                    </div>
                    <div />
                    <select
                      className={SELECT_CLASS}
                      value={relicPickTestEra}
                      onChange={e => setRelicPickTestEra(e.target.value)}
                    >
                      {["LITH","MESO","NEO","AXI","ALL"].map(e => <option key={e} value={e}>{e}</option>)}
                    </select>
                    <SecondaryButton onClick={async () => {
                      setRelicPickTestResult(null);
                      try { setRelicPickTestResult(await invoke<string>("test_relic_pick_overlay", { era: relicPickTestEra })); }
                      catch (e) { setRelicPickTestResult(`Error: ${e}`); }
                    }}>Launch</SecondaryButton>

                    {/* EE.log tail — reveals what string to trigger on */}
                    <div className={ROW_INFO_CLASS}>
                      <span className={ROW_LABEL_CLASS}>EE.log Tail</span>
                      <span className={ROW_DESC_CLASS}>
                        Open the relic screen in-game, then click this to see what EE.log wrote.
                        Paste the result here so we can find the correct trigger string.
                      </span>
                    </div>
                    <div />
                    <SecondaryButton onClick={async () => {
                      setEeLogTail(null);
                      try { setEeLogTail(await invoke<string>("debug_ee_log_tail")); }
                      catch (e) { setEeLogTail(`Error: ${e}`); }
                    }}>Tail Log</SecondaryButton>
                    <div />
                    {eeLogTail && (
                      <div className="col-span-full mt-1">
                        <textarea
                          readOnly
                          value={eeLogTail}
                          className="box-border h-40 w-full resize-y rounded-4 border border-border bg-background p-1.5 font-mono text-10 text-foreground"
                        />
                      </div>
                    )}

                  </div>
                </div>

                {/* ── Categorization Debug ── */}
                <div className={SECTION_CLASS}>
                  <div className={SECTION_TITLE_CLASS}>Categorization Debug</div>
                  <div className={DEBUG_TABLE_CLASS}>
                    <div className={ROW_INFO_CLASS}>
                      <span className={ROW_LABEL_CLASS}>Unmatched Paths</span>
                      <span className={ROW_DESC_CLASS}>
                        When on, writes a JSON file per scan to the Unmatched Paths folder for any inventory path with no WFCD match or that fell to the Misc catch-all.
                      </span>
                    </div>
                    <SecondaryButton
                      onClick={() => invoke("open_debug_folder", { which: "unmatched_paths" }).catch(() => {})}>Go To Folder</SecondaryButton>
                    <SecondaryButton
                      className={toggleButtonClass(debugCatEnabled)}
                      onClick={() => invoke<boolean>("toggle_debug_categorization").then(setDebugCatEnabled).catch(() => {})}>
                      {debugCatEnabled ? "On" : "Off"}
                    </SecondaryButton>
                    <SecondaryButton
                      className={clearButtonClass(unmatchedPathsSize > 0)}
                      disabled={unmatchedPathsSize === 0}
                      onClick={async () => { await invoke("clear_debug_data", { which: "unmatched_paths" }); setUnmatchedPathsSize(0); }}>
                      {unmatchedPathsSize > 0 ? "Clear (" + formatBytes(unmatchedPathsSize) + ")" : "Clear"}
                    </SecondaryButton>
                  </div>
                </div>

              </>}

              {/* ════ Shared About footer — always visible ════ */}
              <div className={`${SECTION_CLASS} mt-auto border-t border-b-0 border-border`}>
                <div className={ROW_CLASS}>
                  <div className={ROW_INFO_CLASS}>
                    <span className={ROW_LABEL_CLASS}>FrameForge</span>
                    <span className={ROW_DESC_CLASS}>Version <strong>{appVersion}</strong></span>
                  </div>
                </div>
              </div>

            </div>{/* end tab content */}
          </div>{/* end layout */}
        </div>
      </div>
    );


}
