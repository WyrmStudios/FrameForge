import { useState, type Dispatch, type MutableRefObject, type SetStateAction } from "react";
import { invoke } from "@tauri-apps/api/core";
import { notify, ensurePermission } from "./lib/notify";
import { formatBytes } from "./lib/formatters";
import { PREFERENCE_KEYS } from "./constants/preferences";
import { CLOCK_FORMAT_OPTIONS, FOUNDRY_PAGE_SIZE_OPTIONS, RELIC_OVERLAY_PRIORITY_OPTIONS, RELIC_PICK_LINES_OPTIONS, RELIC_PICK_PRIORITY_OPTIONS } from "./constants/settings";
import { TAURI_COMMANDS, TAURI_EVENTS } from "./constants/tauri";
import type { ArchonShard, QuantityMap } from "./types/items";
import type { ChangeLogEntry, ModCopy } from "./types/inventory";
import type { ClockFormat, FoundryPageSize, RelicOverlayPriority, RelicPickLines, RelicPickPriority, SettingsSnapshot } from "./types/settings";
import type { SaveApiInventoryArgs } from "./types/tauri";
import "./SettingsModal.css";

type SettingsTab = "general" | "overlays" | "market" | "accessibility" | "data" | "debugging";
type Setter<T> = Dispatch<SetStateAction<T>>;
type ScannerMods = Record<string, { total: number; by_rank: Record<string, number> }>;
type ArchonShards = Record<string, ArchonShard[]>;

export interface SettingsModalProps {
  open: boolean;
  onClose: () => void;
  settingsTab: SettingsTab;
  setSettingsTab: (tab: SettingsTab) => void;
  foundryPageSize: FoundryPageSize; setFoundryPageSize: Setter<FoundryPageSize>; settingsRef: MutableRefObject<SettingsSnapshot>; saveAllSettings: () => void;
  memoryScannerEnabled: boolean; setMemoryScannerEnabled: Setter<boolean>; modularPopout: boolean; setModularPopout: Setter<boolean>; overlayStatus: string;
  overlayEnabled: boolean; setOverlayEnabled: Setter<boolean>; overlayPriority: RelicOverlayPriority; setOverlayPriority: Setter<RelicOverlayPriority>; memTriggerEnabled: boolean; setMemTriggerEnabled: Setter<boolean>;
  relicPickEnabled: boolean; setRelicPickEnabled: Setter<boolean>; relicPickPriority: RelicPickPriority; setRelicPickPriority: Setter<RelicPickPriority>;
  relicPickLines: RelicPickLines; setRelicPickLines: Setter<RelicPickLines>; wfmLoggedIn: boolean;
  wfmInvisibleOnStart: boolean; setWfmInvisibleOnStart: Setter<boolean>; wfmInvisibleOnStartRef: MutableRefObject<boolean>; wfmInvisibleOnClose: boolean; setWfmInvisibleOnClose: Setter<boolean>; wfmInvisibleOnCloseRef: MutableRefObject<boolean>;
  wfmAutoInvisible: boolean; setWfmAutoInvisible: Setter<boolean>; wfmAutoInvisibleMins: number; setWfmAutoInvisibleMins: Setter<number>; colorblindMode: boolean; setColorblindMode: Setter<boolean>; textScale: number; setTextScale: Setter<number>;
  clockFormat: ClockFormat; setClockFormat: Setter<ClockFormat>; systemLocale: string; itemCount: number; recipeCount: number; handleFetch: () => Promise<void>; fetching: boolean; fetchMsg: string;
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
  if (!confirm) return <button className="btn-danger" onClick={() => setConfirm(true)}>Factory Reset</button>;
  return <div style={{ display: "flex", gap: 8, alignItems: "center" }}><span style={{ fontSize: 12, color: "var(--red)" }}>Are you sure? This cannot be undone.</span><button className="btn-danger" disabled={resetting} onClick={() => { setResetting(true); invoke("factory_reset").catch(() => setResetting(false)); }}>{resetting ? "Resetting…" : "Yes, reset"}</button><button className="btn-secondary" onClick={() => setConfirm(false)}>Cancel</button></div>;
}

function BulkPriceRefreshButton() {
  const [state, setState] = useState<"idle" | "loading" | "ok" | "err">("idle");
  const label = state === "loading" ? "Fetching…" : state === "ok" ? "Done!" : state === "err" ? "Failed" : "Refresh Now";
  return <button className="btn-secondary" disabled={state === "loading"} style={{ minWidth: 100, borderColor: state === "ok" ? "var(--accent)" : state === "err" ? "#e05252" : undefined }} onClick={() => { setState("loading"); invoke("refresh_bulk_prices").then(() => { setState("ok"); setTimeout(() => setState("idle"), 3000); }).catch(() => { setState("err"); setTimeout(() => setState("idle"), 4000); }); }}>{label}</button>;
}

export default function SettingsModal(props: SettingsModalProps) {
  const { settingsTab, setSettingsTab, foundryPageSize, setFoundryPageSize, settingsRef, saveAllSettings, memoryScannerEnabled, setMemoryScannerEnabled, modularPopout, setModularPopout, overlayStatus, overlayEnabled, setOverlayEnabled, overlayPriority, setOverlayPriority, memTriggerEnabled, setMemTriggerEnabled, relicPickEnabled, setRelicPickEnabled, relicPickPriority, setRelicPickPriority, relicPickLines, setRelicPickLines, wfmLoggedIn, wfmInvisibleOnStart, setWfmInvisibleOnStart, wfmInvisibleOnStartRef, wfmInvisibleOnClose, setWfmInvisibleOnClose, wfmInvisibleOnCloseRef, wfmAutoInvisible, setWfmAutoInvisible, wfmAutoInvisibleMins, setWfmAutoInvisibleMins, colorblindMode, setColorblindMode, textScale, setTextScale, clockFormat, setClockFormat, systemLocale, itemCount, recipeCount, handleFetch, fetching, fetchMsg, setQuantities, setApiQuantities, setApiModCopies, setScannerMods, setMasteryData, setArchonShards, setFormaData, setChangeLog, setLastChanged, setWfConnected, wfConnectedRef, setItemsRefreshKey, setClearMsg, clearMsg, blobLogEnabled, setBlobLogEnabled, blobLogSize, setBlobLogSize, companionApiEnabled, apiLogEnabled, setApiLogEnabled, apiLogSize, setApiLogSize, setShowInventoryBatchPreview, notifyTestResult, setNotifyTestResult, overlayLogCopied, setOverlayLogCopied, autoDiagEnabled, setAutoDiagEnabled, diagFolderSize, setDiagFolderSize, diagPath, diagCapturing, setDiagCapturing, setDiagPath, reloadDebugSizes, memoryProbing, setMemoryProbing, probeSize, setProbeSize, rawScanning, setRawScanning, rawScanSize, setRawScanSize, memRelicDebugRunning, setMemRelicDebugRunning, relicPickOcrResult, relicPickOcrTesting, setRelicPickOcrTesting, setRelicPickOcrResult, relicPickTestResult, relicPickTestEra, setRelicPickTestEra, setRelicPickTestResult, eeLogTail, setEeLogTail, debugCatEnabled, setDebugCatEnabled, unmatchedPathsSize, setUnmatchedPathsSize, appVersion } = props;
  if (!props.open) return null;
  const onClose = props.onClose;
  return (
      <div className="settings-overlay" onClick={() => onClose()}>
        <div className="settings-modal" onClick={e => e.stopPropagation()}>
          <div className="settings-header">
            <span className="settings-title">Settings</span>
            <button className="craft-detail-close" onClick={() => onClose()}>✕</button>
          </div>

          <div className="settings-layout">
            {/* ── Sidebar nav ── */}
            <nav className="settings-sidebar">
              {(["general", "overlays", "market", "accessibility", "data", "debugging"] as const).map(tab => (
                <button
                  key={tab}
                  className={`settings-tab-item${settingsTab === tab ? " active" : ""}`}
                  onClick={() => setSettingsTab(tab)}
                >
                  {tab.charAt(0).toUpperCase() + tab.slice(1)}
                </button>
              ))}
            </nav>

            {/* ── Tab content ── */}
            <div className="settings-body">

              {/* ════════════ GENERAL ════════════ */}
              {settingsTab === "general" && <>

                {/* Foundry */}
                <div className="settings-section">
                  <div className="settings-section-title">Foundry</div>
                  <div className="settings-row">
                    <div className="settings-row-info">
                      <span className="settings-row-label">Items per page</span>
                      <span className="settings-row-desc">How many items to show per page in the Foundry browser.</span>
                    </div>
                    <select className="settings-select" value={foundryPageSize}
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
                <div className="settings-section" style={{ borderColor: memoryScannerEnabled ? "rgba(240,192,64,.3)" : undefined }}>
                  <div className="settings-section-title" style={{ display: "flex", alignItems: "center", gap: 8 }}>
                    Memory Scanner
                    <span style={{ fontSize: 10, background: "rgba(240,192,64,.15)", color: "#f0c040", border: "1px solid rgba(240,192,64,.35)", borderRadius: 3, padding: "1px 6px", fontWeight: 700 }}>
                      EULA GREY AREA
                    </span>
                  </div>
                  <div style={{ fontSize: 11, color: "var(--muted)", marginBottom: 8, lineHeight: 1.5 }}>
                    Reads live inventory, crafting jobs, and mod ranks from Warframe's process memory via <code style={{ fontSize: 10 }}>ReadProcessMemory</code>. DE has historically tolerated read-only tools, but has not given explicit permission. Enable at your own risk.
                  </div>
                  <div className="settings-row">
                    <div>
                      <span className="settings-row-label">Enable</span>
                      <span className="settings-row-desc">Required for live inventory, quantity tracking, and mod ranks</span>
                    </div>
                    <button
                      className="btn-secondary"
                      style={{ minWidth: 64, background: memoryScannerEnabled ? "rgba(240,192,64,.15)" : undefined, borderColor: memoryScannerEnabled ? "#f0c040" : undefined, color: memoryScannerEnabled ? "#f0c040" : undefined }}
                        onClick={() => setMemoryScannerEnabled(v => !v)}
                    >
                      {memoryScannerEnabled ? "Enabled" : "Disabled"}
                    </button>
                  </div>
                </div>

                {/* Warframe API */}
                <div className="settings-section">
                  <div className="settings-section-title" style={{ display: "flex", alignItems: "center", gap: 8 }}>
                    Warframe API
                    <span style={{ fontSize: 10, background: "rgba(240,192,64,.15)", color: "#f0c040", border: "1px solid rgba(240,192,64,.35)", borderRadius: 3, padding: "1px 6px", fontWeight: 700 }}>
                      SUSPENDED
                    </span>
                  </div>
                  <div style={{
                    fontSize: 11, color: "var(--muted)", lineHeight: 1.6,
                    background: "rgba(240,192,64,.06)", border: "1px solid rgba(240,192,64,.25)",
                    borderRadius: 6, padding: "8px 10px",
                  }}>
                    <strong style={{ color: "#f0c040" }}>Temporarily unavailable.</strong>
                    {" "}This feature connects to an undocumented DE endpoint (<code style={{ fontSize: 10 }}>api.warframe.com/api/inventory.php</code>).
                    {" "}DE confirmed third-party tools run at your own risk but could not clarify whether this specific endpoint is permitted.
                    {" "}The feature is disabled until we receive clearer guidance.
                  </div>
                </div>

                {/* Account Login */}
                <div className="settings-section">
                  <div className="settings-section-title">Account Login</div>
                  <div style={{
                    fontSize: 11, color: "var(--muted)", lineHeight: 1.6,
                    background: "rgba(255,100,100,.07)", border: "1px solid rgba(255,100,100,.2)",
                    borderRadius: 6, padding: "8px 10px",
                  }}>
                    <strong style={{ color: "#ff8080" }}>Login is temporarily unavailable.</strong>
                    {" "}Digital Extremes encrypted their login API in March 2026, which blocked all third-party tools — including FrameForge — from authenticating on your behalf.
                    {" "}PC players are not affected: inventory is synced automatically while the game is running.
                  </div>
                  <div style={{
                    marginTop: 8, fontSize: 11, color: "var(--muted)", lineHeight: 1.6,
                    background: "rgba(100,180,255,.06)", border: "1px solid rgba(100,180,255,.18)",
                    borderRadius: 6, padding: "8px 10px",
                  }}>
                    FrameForge is actively exploring ways to restore inventory access for console and non-PC players.
                    {" "}Follow the project for updates.
                  </div>
                </div>

                {/* Modular Window */}
                <div className="settings-section">
                  <div className="settings-section-title">Modular Window</div>
                  <div className="settings-row">
                    <div className="settings-row-info">
                      <span className="settings-row-label">Pop-out</span>
                      <span className="settings-row-desc">Detach the Modular Window into its own floating window.</span>
                    </div>
                    <button
                      className="btn-secondary"
                      style={{ minWidth: 64, background: modularPopout ? "rgba(56,139,253,.15)" : undefined, borderColor: modularPopout ? "var(--accent)" : undefined }}
                      onClick={() => {
                        const next = !modularPopout;
                        setModularPopout(next);
                        settingsRef.current = { ...settingsRef.current, modularPopout: next };
                        saveAllSettings();
                      }}
                    >{modularPopout ? "On" : "Off"}</button>
                  </div>
                </div>

              </>}

              {/* ════════════ OVERLAYS ════════════ */}
              {settingsTab === "overlays" && <>

                {/* Relic Overlay */}
                <div className="settings-section">
                  <div className="settings-section-title">Relic Overlay</div>
                  {overlayStatus && (
                    <div style={{ fontSize: 12, padding: '4px 8px', marginBottom: 6,
                      background: 'rgba(255,255,255,0.05)', borderRadius: 4,
                      color: '#9ecaed', fontFamily: 'monospace' }}>
                      {overlayStatus}
                    </div>
                  )}
                  <div className="settings-row">
                    <div className="settings-row-info">
                      <span className="settings-row-label">Overlay</span>
                      <span className="settings-row-desc">Auto-shows reward cards when a Void Fissure screen is detected.</span>
                    </div>
                    <button
                      className="btn-secondary"
                      style={{ minWidth: 64, background: overlayEnabled ? "rgba(56,139,253,.15)" : undefined, borderColor: overlayEnabled ? "var(--accent)" : undefined }}
                      onClick={() => {
                        const next = !overlayEnabled;
                        setOverlayEnabled(next);
                        localStorage.setItem(PREFERENCE_KEYS.OVERLAY_ENABLED, String(next));
                        settingsRef.current = { ...settingsRef.current, overlayEnabled: next };
                        saveAllSettings();
                        if (!next) {
                          import("@tauri-apps/api/event").then(({ emit }) =>
                            emit(TAURI_EVENTS.RELIC_SCREEN, true).catch(() => {})
                          );
                        }
                      }}
                    >{overlayEnabled ? "On" : "Off"}</button>
                  </div>
                  <div className="settings-row" style={{ marginTop: 8 }}>
                    <div className="settings-row-info">
                      <span className="settings-row-label">Pick priority</span>
                      <span className="settings-row-desc">Which card the overlay highlights as the best pick.</span>
                    </div>
                    <select
                      className="settings-select"
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
                </div>

                {/* Relic Overlay — Memory Trigger */}
                <div className="settings-section">
                  <div className="settings-section-title">Memory Trigger <span style={{ fontSize: 11, opacity: 0.55, fontWeight: 400, marginLeft: 6 }}>in development</span></div>
                  <div className="settings-row">
                    <div className="settings-row-info">
                      <span className="settings-row-label">Use memory scan</span>
                      <span className="settings-row-desc">
                        Still in development — for testing only. Polls Warframe's process memory for the reward screen event in parallel with EE.log.
                        Timing for both paths is written to the session log so they can be compared.
                        The EE.log overlay is unaffected regardless of this setting.
                      </span>
                    </div>
                    <button
                      className="btn-secondary"
                      style={{ minWidth: 64, background: memTriggerEnabled ? "rgba(56,139,253,.15)" : undefined, borderColor: memTriggerEnabled ? "var(--accent)" : undefined }}
                      onClick={() => {
                        const next = !memTriggerEnabled;
                        setMemTriggerEnabled(next);
                        settingsRef.current = { ...settingsRef.current, memTriggerEnabled: next };
                        saveAllSettings();
                        invoke(TAURI_COMMANDS.SET_MEM_TRIGGER_ENABLED, { enabled: next });
                      }}
                    >{memTriggerEnabled ? "On" : "Off"}</button>
                  </div>
                </div>

                {/* Relic Pick Overlay */}
                <div className="settings-section">
                  <div className="settings-section-title">Relic Pick Overlay</div>
                  <div className="settings-row">
                    <div className="settings-row-info">
                      <span className="settings-row-label">Enable Overlay</span>
                      <span className="settings-row-desc">Show the relic pick overlay when opening the relic selection screen.</span>
                    </div>
                    <button
                      className="btn-secondary"
                      style={{ minWidth: 64, background: relicPickEnabled ? "rgba(56,139,253,.15)" : undefined, borderColor: relicPickEnabled ? "var(--accent)" : undefined }}
                      onClick={() => {
                        const next = !relicPickEnabled;
                        setRelicPickEnabled(next);
                        settingsRef.current = { ...settingsRef.current, relicPickEnabled: next };
                        saveAllSettings();
                        invoke(TAURI_COMMANDS.SET_RELIC_PICK_ENABLED, { enabled: next });
                      }}
                    >{relicPickEnabled ? "Enabled" : "Disabled"}</button>
                  </div>
                  <div className="settings-row" style={{ marginTop: 8, opacity: relicPickEnabled ? 1 : 0.45, pointerEvents: relicPickEnabled ? "auto" : "none" }}>
                    <div className="settings-row-info">
                      <span className="settings-row-label">Recommendation Base</span>
                      <span className="settings-row-desc">How relics are ranked in the overlay.</span>
                    </div>
                    <select className="settings-select" value={relicPickPriority}
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
                  <div className="settings-row" style={{ marginTop: 8, opacity: relicPickEnabled ? 1 : 0.45, pointerEvents: relicPickEnabled ? "auto" : "none" }}>
                    <div className="settings-row-info">
                      <span className="settings-row-label">Shown Lines Per Relic</span>
                      <span className="settings-row-desc">How much reward detail to show per relic card.</span>
                    </div>
                    <select className="settings-select" value={relicPickLines}
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
                </div>

              </>}

              {/* ════════════ MARKET ════════════ */}
              {settingsTab === "market" && <>
                <div className="settings-section">
                  <div className="settings-section-title">Bulk Prices</div>
                  <div className="settings-row">
                    <div className="settings-row-info">
                      <span className="settings-row-label">Force Refresh</span>
                      <span className="settings-row-desc">Re-download price data from FrameForgePricing right now. Use this if prices look stale or missing.</span>
                    </div>
                    <BulkPriceRefreshButton />
                  </div>
                </div>
                <div className="settings-section">
                  <div className="settings-section-title">Status Automation</div>
                  {!wfmLoggedIn && (
                    <div style={{ fontSize: 11, color: "var(--muted)", marginBottom: 10, lineHeight: 1.5,
                      padding: "6px 10px", background: "rgba(255,255,255,.04)", borderRadius: 5 }}>
                      Log in to warframe.market in the <strong>Market</strong> tab to enable these features.
                    </div>
                  )}
                  <div className="settings-row" style={{ opacity: wfmLoggedIn ? 1 : 0.45, pointerEvents: wfmLoggedIn ? "auto" : "none" }}>
                    <div className="settings-row-info">
                      <span className="settings-row-label">Go Invisible on startup</span>
                      <span className="settings-row-desc">When FrameForge opens, immediately set your WFM status to Invisible.</span>
                    </div>
                    <button
                      className="btn-secondary"
                      style={{ minWidth: 64, background: wfmInvisibleOnStart ? "rgba(56,139,253,.15)" : undefined, borderColor: wfmInvisibleOnStart ? "var(--accent)" : undefined }}
                      onClick={() => {
                        const next = !wfmInvisibleOnStart;
                        setWfmInvisibleOnStart(next);
                        wfmInvisibleOnStartRef.current = next;
                        settingsRef.current = { ...settingsRef.current, wfmInvisibleOnStart: next };
                        saveAllSettings();
                      }}
                    >{wfmInvisibleOnStart ? "On" : "Off"}</button>
                  </div>

                  <div className="settings-row" style={{ marginTop: 8, opacity: wfmLoggedIn ? 1 : 0.45, pointerEvents: wfmLoggedIn ? "auto" : "none" }}>
                    <div className="settings-row-info">
                      <span className="settings-row-label">Go Invisible on close</span>
                      <span className="settings-row-desc">Before FrameForge exits (X button or taskbar close), set your WFM status to Invisible.</span>
                    </div>
                    <button
                      className="btn-secondary"
                      style={{ minWidth: 64, background: wfmInvisibleOnClose ? "rgba(56,139,253,.15)" : undefined, borderColor: wfmInvisibleOnClose ? "var(--accent)" : undefined }}
                      onClick={() => {
                        const next = !wfmInvisibleOnClose;
                        setWfmInvisibleOnClose(next);
                        wfmInvisibleOnCloseRef.current = next;
                        settingsRef.current = { ...settingsRef.current, wfmInvisibleOnClose: next };
                        saveAllSettings();
                      }}
                    >{wfmInvisibleOnClose ? "On" : "Off"}</button>
                  </div>

                  <div className="settings-row" style={{ marginTop: 8, opacity: wfmLoggedIn ? 1 : 0.45, pointerEvents: wfmLoggedIn ? "auto" : "none" }}>
                    <div className="settings-row-info">
                      <span className="settings-row-label">Auto-invisible timer</span>
                      <span className="settings-row-desc">
                        After{" "}
                        <input
                          type="number" min={1} max={480} value={wfmAutoInvisibleMins}
                          disabled={!wfmAutoInvisible}
                          style={{ width: 48, fontSize: 12, background: "var(--surface)", border: "1px solid var(--border)", borderRadius: 4, color: "var(--text)", padding: "1px 4px", textAlign: "center" }}
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
                    <button
                      className="btn-secondary"
                      style={{ minWidth: 64, background: wfmAutoInvisible ? "rgba(56,139,253,.15)" : undefined, borderColor: wfmAutoInvisible ? "var(--accent)" : undefined }}
                      onClick={() => {
                        const next = !wfmAutoInvisible;
                        setWfmAutoInvisible(next);
                        settingsRef.current = { ...settingsRef.current, wfmAutoInvisible: next };
                        saveAllSettings();
                      }}
                    >{wfmAutoInvisible ? "On" : "Off"}</button>
                  </div>
                </div>
              </>}

              {/* ════════════ ACCESSIBILITY ════════════ */}
              {settingsTab === "accessibility" && <>
                <div className="settings-section">
                  <div className="settings-section-title">Appearance</div>
                  <div className="settings-row">
                    <div className="settings-row-info">
                      <span className="settings-row-label">Colorblind Mode</span>
                      <span className="settings-row-desc">Adds ✓ / ✓✓ symbols to relic reward boxes so status doesn't rely on color alone.</span>
                    </div>
                    <button
                      className="btn-secondary"
                      style={{ minWidth: 64, background: colorblindMode ? "rgba(56,139,253,.15)" : undefined, borderColor: colorblindMode ? "var(--accent)" : undefined }}
                      onClick={() => {
                        const next = !colorblindMode;
                        setColorblindMode(next);
                        localStorage.setItem(PREFERENCE_KEYS.COLORBLIND_MODE, String(next));
                        settingsRef.current = { ...settingsRef.current, colorblindMode: next };
                        saveAllSettings();
                      }}
                    >{colorblindMode ? "On" : "Off"}</button>
                  </div>
                  <div className="settings-row" style={{ marginTop: 8 }}>
                    <div className="settings-row-info">
                      <span className="settings-row-label">Text Size</span>
                      <span className="settings-row-desc">{Math.round(textScale * 100)}%</span>
                    </div>
                    <input type="range" min="0.8" max="2.0" step="0.1" value={textScale}
                      style={{ width: 120 }}
                      onChange={e => {
                        const v = parseFloat(e.target.value);
                        setTextScale(v);
                        document.documentElement.style.setProperty("--ff-scale", v.toString());
                        localStorage.setItem(PREFERENCE_KEYS.TEXT_SCALE, v.toString());
                        settingsRef.current = { ...settingsRef.current, textScale: v };
                        saveAllSettings();
                      }} />
                  </div>
                  <div className="settings-row" style={{ marginTop: 8 }}>
                    <div className="settings-row-info">
                      <span className="settings-row-label">Clock Format</span>
                      <span className="settings-row-desc">How times are displayed throughout the app.{clockFormat === "auto" ? ` System locale: ${systemLocale}` : ""}</span>
                    </div>
                    <div style={{ display: "flex", gap: 4 }}>
                      {CLOCK_FORMAT_OPTIONS.map(f => (
                        <button key={f} className="btn-secondary" style={{ minWidth: 44, background: clockFormat === f ? "rgba(56,139,253,.15)" : undefined, borderColor: clockFormat === f ? "var(--accent)" : undefined }}
                          onClick={() => {
                            setClockFormat(f);
                            settingsRef.current = { ...settingsRef.current, clockFormat: f };
                            saveAllSettings();
                          }}>
                          {f === "auto" ? "Auto" : f}
                        </button>
                      ))}
                    </div>
                  </div>
                </div>
              </>}

              {/* ════════════ DATA ════════════ */}
              {settingsTab === "data" && <>
                <div className="settings-section">
                  <div className="settings-section-title">Item Database</div>
                  <div className="settings-row">
                    <div className="settings-row-info">
                      <span className="settings-row-label">Catalog</span>
                      <span className="settings-row-desc">{itemCount.toLocaleString()} items · {recipeCount.toLocaleString()} recipes cached</span>
                    </div>
                    <button className="btn-secondary" onClick={() => { onClose(); handleFetch(); }} disabled={fetching}>
                      {fetching ? "Fetching…" : "Refresh"}
                    </button>
                  </div>
                  {fetchMsg && <div className="settings-msg">{fetchMsg}</div>}
                </div>
                <div className="settings-section">
                  <div className="settings-section-title">Inventory Cache</div>
                  <div className="settings-row">
                    <div className="settings-row-info">
                      <span className="settings-row-label">Clear Cache</span>
                      <span className="settings-row-desc">Reset all scanned quantities and changelog.</span>
                    </div>
                    <button
                      className="btn-danger"
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
                        } catch (e) { setClearMsg(`Error: ${e}`); }
                      }}
                    >Clear Cache</button>
                  </div>
                  {clearMsg && <div className="settings-msg">{clearMsg}</div>}
                </div>
                <div className="settings-section" style={{ borderColor: "rgba(224,82,82,.3)" }}>
                  <div className="settings-section-title" style={{ color: "var(--red)" }}>Factory Reset</div>
                  <div className="settings-row">
                    <div className="settings-row-info">
                      <span className="settings-row-label">Reset Everything</span>
                      <span className="settings-row-desc">Delete all app data — settings, inventory cache, trade log, market prices, WFM login — and restart. Cannot be undone.</span>
                    </div>
                    <FactoryResetButton />
                  </div>
                </div>
              </>}

              {/* ════════════ DEBUGGING ════════════ */}
              {settingsTab === "debugging" && <>

                <div className="settings-section">
                  <div className="settings-section-title">Loggers</div>
                  <div className="debug-table">

                    {/* Inventory Snapshots */}
                    <div className="settings-row-info" style={{ opacity: memoryScannerEnabled ? 1 : 0.4 }}>
                      <span className="settings-row-label">Inventory Snapshots</span>
                      <span className="settings-row-desc">Saves a JSON snapshot on each memory scan.</span>
                    </div>
                    <button className="btn-secondary" style={{ opacity: memoryScannerEnabled ? 1 : 0.4, pointerEvents: memoryScannerEnabled ? "auto" : "none" }}
                      onClick={() => invoke("open_debug_folder", { which: "blobs" }).catch(() => {})}>Go To Folder</button>
                    <button className="btn-secondary"
                      style={{ background: blobLogEnabled ? "rgba(56,139,253,.15)" : undefined, borderColor: blobLogEnabled ? "var(--accent)" : undefined, opacity: memoryScannerEnabled ? 1 : 0.4, pointerEvents: memoryScannerEnabled ? "auto" : "none" }}
                        onClick={() => setBlobLogEnabled(v => !v)}>{blobLogEnabled ? "On" : "Off"}</button>
                    <button className="btn-secondary"
                      style={{ color: blobLogSize > 0 ? "var(--red)" : undefined, borderColor: blobLogSize > 0 ? "var(--red)" : undefined, opacity: memoryScannerEnabled ? 1 : 0.4, pointerEvents: memoryScannerEnabled ? "auto" : "none" }}
                      disabled={blobLogSize === 0}
                      onClick={async () => { await invoke("clear_debug_data", { which: "blobs" }); setBlobLogSize(0); }}
                    >{blobLogSize > 0 ? `Clear (${formatBytes(blobLogSize)})` : "Clear"}</button>

                    {/* API Responses */}
                    <div className="settings-row-info" style={{ opacity: companionApiEnabled ? 1 : 0.4 }}>
                      <span className="settings-row-label">API Responses</span>
                      <span className="settings-row-desc">Records raw DE API responses on each inventory fetch.</span>
                    </div>
                    <button className="btn-secondary" style={{ opacity: companionApiEnabled ? 1 : 0.4, pointerEvents: companionApiEnabled ? "auto" : "none" }}
                      onClick={() => invoke("open_debug_folder", { which: "api_logs" }).catch(() => {})}>Go To Folder</button>
                    <button className="btn-secondary"
                      style={{ background: apiLogEnabled ? "rgba(56,139,253,.15)" : undefined, borderColor: apiLogEnabled ? "var(--accent)" : undefined, opacity: companionApiEnabled ? 1 : 0.4, pointerEvents: companionApiEnabled ? "auto" : "none" }}
                        onClick={() => setApiLogEnabled(v => !v)}>{apiLogEnabled ? "On" : "Off"}</button>
                    <button className="btn-secondary"
                      style={{ color: apiLogSize > 0 ? "var(--red)" : undefined, borderColor: apiLogSize > 0 ? "var(--red)" : undefined, opacity: companionApiEnabled ? 1 : 0.4, pointerEvents: companionApiEnabled ? "auto" : "none" }}
                      disabled={apiLogSize === 0}
                      onClick={async () => { await invoke("clear_debug_data", { which: "api_logs" }); setApiLogSize(0); }}
                    >{apiLogSize > 0 ? `Clear (${formatBytes(apiLogSize)})` : "Clear"}</button>

                  </div>
                </div>

                <div className="settings-section">
                  <div className="settings-section-title">Inventory Preview</div>
                  <div className="settings-row">
                    <div className="settings-row-info">
                      <span className="settings-row-label">Incoming Batch</span>
                      <span className="settings-row-desc">Preview gained, lost, crafting, and mod rank changes without modifying your inventory.</span>
                    </div>
                    <button className="btn-secondary" onClick={() => setShowInventoryBatchPreview(true)}>Preview</button>
                  </div>
                </div>

                <div className="settings-section">
                  <div className="settings-section-title">Diagnostics</div>
                  <div className="debug-table">

                    {/* Test Notification */}
                    <div className="settings-row-info">
                      <span className="settings-row-label">Test Notification</span>
                      <span className="settings-row-desc">
                        Send a desktop notification now, to check the OS delivers them at all.
                        {notifyTestResult && <span style={{ display: "block", marginTop: 2, color: notifyTestResult.startsWith("Sent") ? "var(--green)" : "var(--red)", fontSize: 11 }}>{notifyTestResult}</span>}
                      </span>
                    </div>
                    <div />{/* Go To Folder placeholder */}
                    <button className="btn-secondary" onClick={async () => {
                      setNotifyTestResult("");
                      if (!(await ensurePermission())) {
                        setNotifyTestResult("Permission denied — notifications are blocked for FrameForge in your system settings.");
                        return;
                      }
                      await notify("FrameForge", "Test notification — watched fissure alerts will look like this.");
                      setNotifyTestResult("Sent. If nothing appeared, the OS notification daemon is dropping it.");
                    }}>Send</button>
                    <div />{/* Clear placeholder */}

                    {/* Overlay Log */}
                    <div className="settings-row-info">
                      <span className="settings-row-label">Overlay Log</span>
                      <span className="settings-row-desc">Step-by-step log of the last relic overlay attempt.</span>
                    </div>
                    <div />{/* Go To Folder placeholder */}
                    <button className="btn-secondary" onClick={async () => {
                      try { alert(await invoke<string>("get_overlay_session_log")); }
                      catch (e) { alert(`Error: ${e}`); }
                    }}>View</button>
                    <button className="btn-secondary" onClick={async () => {
                      try {
                        const log = await invoke<string>("get_overlay_session_log");
                        navigator.clipboard.writeText(log).then(() => {
                          setOverlayLogCopied(true);
                          setTimeout(() => setOverlayLogCopied(false), 1500);
                        }).catch(() => {});
                      }
                      catch (e) { alert(`Error: ${e}`); }
                    }}>{overlayLogCopied ? "✓ Copied" : "Copy"}</button>

                    {/* Auto-capture */}
                    <div className="settings-row-info">
                      <span className="settings-row-label">Auto-capture</span>
                      <span className="settings-row-desc">Automatically saves a screenshot and OCR session log for every relic reward screen. One folder is created per relic in the diagnostics directory.</span>
                    </div>
                    <button className="btn-secondary" onClick={() => invoke("open_debug_folder", { which: "diag" }).catch(() => {})}>Go To Folder</button>
                    <button className="btn-secondary"
                      style={{ background: autoDiagEnabled ? "rgba(56,139,253,.15)" : undefined, borderColor: autoDiagEnabled ? "var(--accent)" : undefined }}
                      onClick={() => {
                        const next = !autoDiagEnabled;
                        setAutoDiagEnabled(next);
                        localStorage.setItem(PREFERENCE_KEYS.AUTO_DIAGNOSTICS, String(next));
                        settingsRef.current = { ...settingsRef.current, autoDiagEnabled: next };
                        saveAllSettings();
                      }}>{autoDiagEnabled ? "On" : "Off"}</button>
                    <button className="btn-secondary"
                      style={{ color: diagFolderSize > 0 ? "var(--red)" : undefined, borderColor: diagFolderSize > 0 ? "var(--red)" : undefined }}
                      disabled={diagFolderSize === 0}
                      onClick={async () => { await invoke("clear_diag_folder"); setDiagFolderSize(0); }}
                    >{diagFolderSize > 0 ? `Clear (${formatBytes(diagFolderSize)})` : "Clear"}</button>

                    {/* Manual Capture */}
                    <div className="settings-row-info">
                      <span className="settings-row-label">Manual Capture</span>
                      <span className="settings-row-desc">
                        Take a diagnostic screenshot + scan log right now.
                        {diagPath && <span style={{ display: "block", marginTop: 2, color: "var(--green)", fontSize: 11 }}>Saved.</span>}
                      </span>
                    </div>
                    <button className="btn-secondary" onClick={() => invoke("open_debug_folder", { which: "manual_capture" }).catch(() => {})}>Go To Folder</button>
                    <button className="btn-secondary" disabled={diagCapturing}
                      onClick={async () => {
                        setDiagCapturing(true); setDiagPath(null);
                        try { const p = await invoke<string>("capture_diagnostics"); setDiagPath(p); reloadDebugSizes(); }
                        catch (e) { setDiagPath(`Error: ${e}`); }
                        finally { setDiagCapturing(false); }
                      }}>{diagCapturing ? "Working…" : "Capture"}</button>
                    <div />{/* Clear placeholder */}

                    {/* Memory Probe */}
                    <div className="settings-row-info">
                      <span className="settings-row-label">Memory Probe</span>
                      <span className="settings-row-desc">Dumps inventory strings from Warframe's memory.</span>
                    </div>
                    <button className="btn-secondary" onClick={() => invoke("open_debug_folder", { which: "probe" }).catch(() => {})}>Go To Folder</button>
                    <button className="btn-secondary" disabled={memoryProbing} onClick={() => {
                      setMemoryProbing(true);
                      invoke<string>("dump_memory_probe")
                        .then(result => {
                          alert(`Probe complete — ${result.split("\n").filter(l => l.trim()).length} entries written.`);
                          reloadDebugSizes();
                        })
                        .catch(e => alert("Probe failed: " + String(e)))
                        .finally(() => setMemoryProbing(false));
                    }}>{memoryProbing ? "Running…" : "Run"}</button>
                    <button className="btn-secondary"
                      style={{ color: probeSize > 0 ? "var(--red)" : undefined, borderColor: probeSize > 0 ? "var(--red)" : undefined }}
                      disabled={probeSize === 0}
                      onClick={async () => { await invoke("clear_debug_data", { which: "probe" }); setProbeSize(0); }}
                    >{probeSize > 0 ? `Clear (${formatBytes(probeSize)})` : "Clear"}</button>

                    {/* Raw Memory Record */}
                    <div className="settings-row-info">
                      <span className="settings-row-label">Raw Memory Record</span>
                      <span className="settings-row-desc">{rawScanning ? "Recording — navigate in-game, then click Stop." : "Records all readable memory strings while you navigate in-game."}</span>
                    </div>
                    <button className="btn-secondary" onClick={() => invoke("open_debug_folder", { which: "raw_scan" }).catch(() => {})}>Go To Folder</button>
                    <button className={rawScanning ? "btn-danger" : "btn-secondary"}
                      onClick={() => {
                        invoke<string>("toggle_raw_scan")
                          .then(status => { const active = status === "started"; setRawScanning(active); if (!active) reloadDebugSizes(); })
                          .catch(e => alert("Error: " + String(e)));
                      }}>{rawScanning ? "Stop" : "Record"}</button>
                    <button className="btn-secondary"
                      style={{ color: rawScanSize > 0 ? "var(--red)" : undefined, borderColor: rawScanSize > 0 ? "var(--red)" : undefined }}
                      disabled={rawScanSize === 0 || rawScanning}
                      onClick={async () => { await invoke("clear_debug_data", { which: "raw_scan" }); setRawScanSize(0); }}
                    >{rawScanSize > 0 ? `Clear (${formatBytes(rawScanSize)})` : "Clear"}</button>

                    {/* Memory Relic Debug */}
                    <div className="settings-row-info">
                      <span className="settings-row-label">Memory Relic Debug</span>
                      <span className="settings-row-desc">{memRelicDebugRunning ? "Running — tailing EE.log and scanning Warframe memory. Log at %TEMP%\\frameforge_mem_relic_debug.log." : "Tails EE.log and scans Warframe memory for relic reward patterns. Writes everything to a log file."}</span>
                    </div>
                    <div />{/* no folder button */}
                    <button className={memRelicDebugRunning ? "btn-danger" : "btn-secondary"}
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
                      }}>{memRelicDebugRunning ? "End" : "Start"}</button>
                    <div />{/* no clear button */}

                  </div>
                </div>

                <div className="settings-section">
                  <div className="settings-section-title">Relic Pick Overlay</div>
                  <div className="debug-table">

                    {/* OCR Era Test */}
                    <div className="settings-row-info">
                      <span className="settings-row-label">OCR Era Detect</span>
                      <span className="settings-row-desc">
                        Reads the top-left quarter of the Warframe window and reports which fissure era OCR finds.
                        {relicPickOcrResult && <span style={{ display: "block", marginTop: 2, color: "var(--accent)", fontSize: 11 }}>{relicPickOcrResult}</span>}
                      </span>
                    </div>
                    <div />
                    <button className="btn-secondary" disabled={relicPickOcrTesting} onClick={async () => {
                      setRelicPickOcrTesting(true); setRelicPickOcrResult(null);
                      try { setRelicPickOcrResult(await invoke<string>("debug_detect_fissure_era")); }
                      catch (e) { setRelicPickOcrResult(`Error: ${e}`); }
                      finally { setRelicPickOcrTesting(false); }
                    }}>{relicPickOcrTesting ? "Running…" : "Test OCR"}</button>
                    <div />

                    {/* Test Overlay */}
                    <div className="settings-row-info">
                      <span className="settings-row-label">Test Overlay</span>
                      <span className="settings-row-desc">
                        Manually fire the relic pick overlay with a specific era.
                        {relicPickTestResult && <span style={{ display: "block", marginTop: 2, color: "var(--accent)", fontSize: 11 }}>{relicPickTestResult}</span>}
                      </span>
                    </div>
                    <div />
                    <select
                      value={relicPickTestEra}
                      onChange={e => setRelicPickTestEra(e.target.value)}
                      style={{ fontSize: 12, padding: "3px 6px", borderRadius: 4, background: "var(--bg2)", border: "1px solid var(--border)", color: "var(--text)" }}
                    >
                      {["LITH","MESO","NEO","AXI","ALL"].map(e => <option key={e} value={e}>{e}</option>)}
                    </select>
                    <button className="btn-secondary" onClick={async () => {
                      setRelicPickTestResult(null);
                      try { setRelicPickTestResult(await invoke<string>("test_relic_pick_overlay", { era: relicPickTestEra })); }
                      catch (e) { setRelicPickTestResult(`Error: ${e}`); }
                    }}>Launch</button>

                    {/* EE.log tail — reveals what string to trigger on */}
                    <div className="settings-row-info">
                      <span className="settings-row-label">EE.log Tail</span>
                      <span className="settings-row-desc">
                        Open the relic screen in-game, then click this to see what EE.log wrote.
                        Paste the result here so we can find the correct trigger string.
                      </span>
                    </div>
                    <div />
                    <button className="btn-secondary" onClick={async () => {
                      setEeLogTail(null);
                      try { setEeLogTail(await invoke<string>("debug_ee_log_tail")); }
                      catch (e) { setEeLogTail(`Error: ${e}`); }
                    }}>Tail Log</button>
                    <div />
                    {eeLogTail && (
                      <div style={{ gridColumn: "1 / -1", marginTop: 4 }}>
                        <textarea
                          readOnly
                          value={eeLogTail}
                          style={{
                            width: "100%", height: 160, fontSize: 10, fontFamily: "monospace",
                            background: "var(--bg2)", border: "1px solid var(--border)",
                            color: "var(--text)", borderRadius: 4, padding: 6,
                            resize: "vertical", boxSizing: "border-box"
                          }}
                        />
                      </div>
                    )}

                  </div>
                </div>

                {/* ── Categorization Debug ── */}
                <div className="settings-section">
                  <div className="settings-section-title">Categorization Debug</div>
                  <div className="debug-table">
                    <div className="settings-row-info">
                      <span className="settings-row-label">Unmatched Paths</span>
                      <span className="settings-row-desc">
                        When on, writes a JSON file per scan to the Unmatched Paths folder for any inventory path with no WFCD match or that fell to the Misc catch-all.
                      </span>
                    </div>
                    <button className="btn-secondary"
                      onClick={() => invoke("open_debug_folder", { which: "unmatched_paths" }).catch(() => {})}>Go To Folder</button>
                    <button className="btn-secondary"
                      style={{ background: debugCatEnabled ? "rgba(56,139,253,.15)" : undefined, borderColor: debugCatEnabled ? "var(--accent)" : undefined }}
                      onClick={() => invoke<boolean>("toggle_debug_categorization").then(setDebugCatEnabled).catch(() => {})}>
                      {debugCatEnabled ? "On" : "Off"}
                    </button>
                    <button className="btn-secondary"
                      style={{ color: unmatchedPathsSize > 0 ? "var(--red)" : undefined, borderColor: unmatchedPathsSize > 0 ? "var(--red)" : undefined }}
                      disabled={unmatchedPathsSize === 0}
                      onClick={async () => { await invoke("clear_debug_data", { which: "unmatched_paths" }); setUnmatchedPathsSize(0); }}>
                      {unmatchedPathsSize > 0 ? `Clear (${formatBytes(unmatchedPathsSize)})` : "Clear"}
                    </button>
                  </div>
                </div>

              </>}

              {/* ════ Shared About footer — always visible ════ */}
              <div className="settings-section" style={{ marginTop: "auto", borderTop: "1px solid var(--border)", borderBottom: "none" }}>
                <div className="settings-row">
                  <div className="settings-row-info">
                    <span className="settings-row-label">FrameForge</span>
                    <span className="settings-row-desc">Version <strong>{appVersion}</strong></span>
                  </div>
                </div>
              </div>

            </div>{/* end settings-body */}
          </div>{/* end settings-layout */}
        </div>
      </div>
    );


}
