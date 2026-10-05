import { useState, useEffect, useMemo, useCallback, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getVersion } from "@tauri-apps/api/app";
import { listen } from "@tauri-apps/api/event";
import { applyScale } from "./lib/uiScale";
import { useContextMenu, CtxMenu } from "./shared/CtxMenu";
import { extractItemName } from "./lib/itemContext";
import { matchesSearchTerms, splitSearchTerms } from "./lib/search";
import { openWiki, copyWikiLink } from "./lib/wiki";
import { useModularWindow } from "./hooks/useModularWindow";
import { useSettings } from "./hooks/useSettings";
import { useInventoryData } from "./hooks/useInventoryData";
import { useBulkPrices } from "./hooks/useBulkPrices";
import { useOverlays } from "./hooks/useOverlays";
import { useTimerPreferences } from "./hooks/useTimerPreferences";
import { useFissureNotifications } from "./hooks/useFissureNotifications";
import { useArbitrationPreferences } from "./hooks/useArbitrationPreferences";
import { useArbitrationAlerts } from "./hooks/useArbitrationAlerts";
import { CATEGORIES } from "./constants/categories";
import { APP_TITLE, IS_DEV } from "./constants/app";

import { getCurrentWindow } from "@tauri-apps/api/window";

import { ImgCacheDirContext } from "./ImgCacheDir";
import Foundry from "./Foundry";
import CacheStatusChip from "./header/CacheStatusChip";
import MarketHelper from "./market/MarketHelper";
import RelicHelper from "./RelicHelper";
import RivenAnalyzer from "./riven/RivenAnalyzer";
import RivenOverlayWindow from "./riven/RivenOverlayWindow";
import RelicPickOverlay from "./relic-overlay/RelicPickOverlay";
import TimerHelper from "./TimerHelper";
import Arbitrations from "./arbitration/Arbitrations";
import ArbitrationOverlay from "./arbitration/ArbitrationOverlay";
import Statistics from "./statistics/Statistics";
import Overlay from "./relic-overlay/Overlay";
import ModularWindow from "./modular-window/ModularWindow";
import ModularWindowPage from "./modular-window/ModularWindowPage";
import SettingsModal from "./SettingsModal";
import ChangeLog from "./ChangeLog";
import InventoryGrid from "./inventory/InventoryGrid";
import InventoryBatchPreview from "./inventory/InventoryBatchPreview";
import InventoryToolbar from "./inventory/InventoryToolbar";
import AppNavigation, { type Module } from "./AppNavigation";
import InventorySidebar from "./inventory/InventorySidebar";
import CompletionistTabs from "./completionist/CompletionistTabs";
import HeaderActions from "./header/HeaderActions";
import ErrorBoundary from "./shared/ErrorBoundary";
import HeaderStatusBadges from "./header/HeaderStatusBadges";
import ConnectionStatusChip from "./header/ConnectionStatusChip";
import { CONN_CHIP, CONN_DOT, CONN_STATUS } from "./header/connStatus";
import KeepMountedWhenHidden from "./KeepMountedWhenHidden";
import { FOUNDRY_FILTERS_DEFAULT, INVENTORY_FILTERS_DEFAULT, MARKET_FILTERS_DEFAULT, RELIC_FILTERS_DEFAULT } from "./constants/filters";
import { PREFERENCE_KEYS } from "./constants/preferences";
import { TAURI_COMMANDS, TAURI_EVENTS } from "./constants/tauri";
import type { FoundryFilters, InventoryFilters, MarketFilters, RelicFilters } from "./types/filters";
import { type FilterPresetModule } from "./types/filterPresets";
import type { ViewMode } from "./types/ui";
import { formatUnixTime } from "./lib/formatters";
import type { CatalogItem, CraftingJob, InventoryItem } from "./types/items";
import type { ChangeLogEntry, ModCopy } from "./types/inventory";
import type { BlobStatusPayload, SettingsFile, SettingsPatch, WarframeCredentials, WarframeInventoryRequest, WfmCredentials, WfmSession } from "./types/tauri";
import "./styles/App.css";

const _winLabel = getCurrentWindow().label;
// Support all URL formats: query string (?overlay), hash (#overlay), or window label.
// v2.0.0 used query strings and they worked fine — keep as primary detection path.
const _params          = new URLSearchParams(window.location.search);
const _hash            = window.location.hash;
const IS_OVERLAY       = _params.has("overlay")      || _hash === "#overlay"      || _winLabel === "relic-overlay";
const IS_MODULAR       = _params.has("modular")      || _hash === "#modular"      || _winLabel === "modular-popout";
const IS_RIVEN_OVERLAY      = _params.has("rivenoverlay")      || _hash === "#rivenoverlay"      || _winLabel === "riven-overlay";
const IS_RELIC_PICK_OVERLAY = _params.has("relicpickoverlay") || _hash === "#relicpickoverlay" || _winLabel === "relic-pick-overlay";
const IS_OVERLAY_TEST       = _params.has("overlaytest")       || _hash === "#overlaytest"       || _winLabel === "overlay-test";
const IS_ARBITRATION_OVERLAY = _params.has("arbitrationoverlay") || _hash === "#arbitrationoverlay" || _winLabel === "arbitration-overlay";
const IS_ANY_OVERLAY = IS_ARBITRATION_OVERLAY || IS_OVERLAY || IS_MODULAR || IS_RIVEN_OVERLAY || IS_RELIC_PICK_OVERLAY;

// Overlay windows return from the router before any hook can run, which rules
// out applying the scale from an effect.
applyScale(IS_ANY_OVERLAY);
listen(TAURI_EVENTS.SETTINGS_UPDATED, () => applyScale(IS_ANY_OVERLAY));

// ─── App ──────────────────────────────────────────────────────────────────────

// RelicAndRivenTab is kept but now just shows RelicHelper — Rivens moved to own tab

// ── Isolated overlay test page ────────────────────────────────────────────────
// Rendered when window URL contains ?overlaytest.
// No data loading, no events — pure window-creation smoke test.
function OverlayTestPage() {
  useEffect(() => {
    [document.documentElement, document.body, document.getElementById('root')]
      .forEach(el => el?.style.setProperty('background', 'transparent', 'important'));
  }, []);

  return (
    <div className="box-border flex h-screen w-screen flex-col items-center justify-center gap-3 border-4 border-overlay-smoke bg-overlay-smoke-bg font-sans text-white">
      <div className="text-22 font-bold drop-shadow-[0_2px_6px_#000]">
        FrameForge Overlay Test
      </div>
      <div className="text-13 opacity-85">If you see green: window + React are working</div>
      <button
        onClick={() => getCurrentWindow().close().catch(() => {})}
        className="mt-2 cursor-pointer rounded-md border-0 bg-overlay-smoke px-6 py-2 text-sm font-bold text-black"
      >
        Close
      </button>
    </div>
  );
}

export default function App() {
  // Isolated overlay test — no data, no events, just proves the window appears.
  if (IS_OVERLAY_TEST) return <OverlayTestPage />;
  // If we're the overlay window, render only the overlay UI
  if (IS_ARBITRATION_OVERLAY) return <ArbitrationOverlay />;
  if (IS_OVERLAY) return <Overlay />;
  if (IS_RIVEN_OVERLAY) return <RivenOverlayWindow />;
  if (IS_RELIC_PICK_OVERLAY) return <RelicPickOverlay />;
  // If we're the pop-out modular window, render the standalone modular UI
  if (IS_MODULAR) return <ModularWindowPage />;

  const [activeModule, setActiveModule] = useState<Module>("inventory");
  const [visitedModules, setVisitedModules] = useState<Set<Module>>(() => new Set(["inventory"]));
  const activateModule = useCallback((module: Module) => {
    setVisitedModules(previous => previous.has(module) ? previous : new Set([...previous, module]));
    setActiveModule(module);
  }, []);
  const { ctxMenu, open: openCtx, close: closeCtx } = useContextMenu();

  // ── Custom hooks ──────────────────────────────────────────────────────────
  const inv = useInventoryData();
  const settings = useSettings(inv.setMonitoring);
  const modular = useModularWindow();
  const timerPreferences = useTimerPreferences();
  const arbitrationPreferences = useArbitrationPreferences();

  const {
    memoryScannerEnabled, setMemoryScannerEnabled,
    blobLogEnabled, setBlobLogEnabled,
    apiLogEnabled, setApiLogEnabled,
    autoDiagEnabled, setAutoDiagEnabled,
    overlayEnabled, setOverlayEnabled,
    overlayPriority, setOverlayPriority,
    overlayOffsets, setOverlayOffsets,
    rivenEnabled, setRivenEnabled,
    textScale, setTextScale,
    colorblindMode, setColorblindMode,
    clockFormat, setClockFormat,
    systemLocale,
    foundryPageSize, setFoundryPageSize,
    relicPickEnabled, setRelicPickEnabled,
    memTriggerEnabled, setMemTriggerEnabled,
    relicPickPriority, setRelicPickPriority,
    relicPickRefinement,
    relicPickLines, setRelicPickLines,
    wfmInvisibleOnStart, setWfmInvisibleOnStart,
    wfmInvisibleOnClose, setWfmInvisibleOnClose,
    wfmAutoInvisible, setWfmAutoInvisible,
    wfmAutoInvisibleMins, setWfmAutoInvisibleMins,
    wfmRecordSales, setWfmRecordSales,
    companionApiEnabled,
    filterPresets, setFilterPresets,
    settingsLoadedRef, settingsRef,
    wfmInvisibleOnStartRef, wfmInvisibleOnCloseRef,
    saveAllSettings, loadSettings,
  } = settings;

  const { tracked, favorites, modularWidth, modularSectionOrder, modularPopout, toggleTracked, toggleFavorite, applySettings: applyModularSettings, setTracked, setFavorites, setModularWidth, setModularSectionOrder, setModularPopout } = modular;
  const { timerFavorites, fissureWatches, fissureNotifications, applySettings: applyTimerSettings, setTimerFavorites, setFissureWatches, setFissureNotifications } = timerPreferences;
  useFissureNotifications(fissureWatches, fissureNotifications);
  const {
    arbFavorites, setArbFavorites,
    arbLeadMins, setArbLeadMins,
    arbTierFilter, setArbTierFilter,
    arbAlertTiers, setArbAlertTiers,
    arbScheduleDays, setArbScheduleDays,
    arbOverlayEnabled, setArbOverlayEnabled,
    applySettings: applyArbitrationSettings,
  } = arbitrationPreferences;
  const arbitrationAlerts = useArbitrationAlerts(arbFavorites, arbAlertTiers, arbLeadMins, settings.settingsLoadedRef);

  const {
    catalog, quantities, apiQuantities, apiModCopies, scannerMods,
    crafting, masteryRank, masteryData, playerName, wfConnected,
    subsummedWarframes, archonShards, formaData, lastApiRefresh,
    inventoryReady, lastInventoryScanAt, changeLog, changeLogArrivalToken,
    lastChanged, monitoring, warframeRunning, itemCount, recipeCount,
    fetching, fetchMsg, imgCacheDir, itemsRefreshKey,
    wfConnectedRef, manualCredsRef,
    handleFetch, applyInventoryData,
    setQuantities, setApiQuantities, setApiModCopies,
    setScannerMods, setMasteryData, setArchonShards,
    setFormaData, setChangeLog, setLastChanged, setWfConnected,
    setItemsRefreshKey,
  } = inv;

  const overlays = useOverlays(setQuantities);
  const { overlayStatus } = overlays;

  // ── Remaining local state (not in any hook) ──────────────────────────────
  const [memoryProbing, setMemoryProbing] = useState(false);
  const [poking, setPoking] = useState(false);
  const [rawScanning, setRawScanning] = useState(false);
  const [memRelicDebugRunning, setMemRelicDebugRunning] = useState(false);
  const [diagCapturing, setDiagCapturing] = useState(false);
  const [notifyTestResult, setNotifyTestResult] = useState("");
  const [relicPickOcrTesting, setRelicPickOcrTesting] = useState(false);
  const [relicPickOcrResult, setRelicPickOcrResult] = useState<string | null>(null);
  const [relicPickTestEra, setRelicPickTestEra] = useState("LITH");
  const [relicPickTestResult, setRelicPickTestResult] = useState<string | null>(null);
  const [eeLogTail, setEeLogTail] = useState<string | null>(null);
  const [diagPath, setDiagPath] = useState<string | null>(null);
  const [diagFolderSize, setDiagFolderSize] = useState<number>(0);
  const [overlayLogCopied, setOverlayLogCopied] = useState(false);
  const [pendingUpdate, setPendingUpdate] = useState<string | null>(null);
  const [updateInstalling, setUpdateInstalling] = useState(false);
  const [wfmLoggedIn, setWfmLoggedIn] = useState(false);
  const wfmLoggedInRef = useRef(false);
  const [inventoryFilters, setInventoryFilters] = useState<InventoryFilters>(INVENTORY_FILTERS_DEFAULT);
  const [foundryFilters, setFoundryFilters] = useState<FoundryFilters>(FOUNDRY_FILTERS_DEFAULT);
  const [marketFilters, setMarketFilters] = useState<MarketFilters>(MARKET_FILTERS_DEFAULT);
  const [relicFilters, setRelicFilters] = useState<RelicFilters>(RELIC_FILTERS_DEFAULT);
  const { category, search, filterOwned, filterRecent, filterPrime, filterVaulted, filterUnvaulted, filterTradeable, filterRank, sortMode } = inventoryFilters;
  const { bulkPrices, loaded: bulkPricesLoaded, error: bulkPricesError, refresh: refreshBulkPrices } = useBulkPrices();
  const platinumPriceStatus = bulkPricesError || (bulkPricesLoaded && bulkPrices.size === 0)
    ? "unavailable" as const
    : bulkPricesLoaded ? "ready" as const : "loading" as const;
  const isPlatinumSort = sortMode === "plat-desc" || sortMode === "plat-asc";
  const isDucatSort = sortMode === "ducat-desc" || sortMode === "ducat-asc";
  const isDucatRatioSort = sortMode === "ducat-ratio-desc" || sortMode === "ducat-ratio-asc";
  const needsPlatinumPrices = isPlatinumSort || isDucatRatioSort;
  const lastPriceIndependentSortRef = useRef<InventoryFilters["sortMode"]>("qty-desc");
  // Bulk prices refresh hourly in the background — re-fetch on each Inventory
  // activation so the chips reflect the current map.
  useEffect(() => { if (activeModule === "inventory") refreshBulkPrices(); }, [activeModule, refreshBulkPrices]);
  useEffect(() => {
    if (!needsPlatinumPrices) lastPriceIndependentSortRef.current = sortMode;
  }, [needsPlatinumPrices, sortMode]);
  const toggleInventoryRecent = useCallback(() => setInventoryFilters(previous => ({
    ...previous,
    filterRecent: !previous.filterRecent,
  })), []);
  const [inventoryView, setInventoryView] = useState<ViewMode>(() =>
    (localStorage.getItem(PREFERENCE_KEYS.INVENTORY_VIEW) as ViewMode | null) ?? "cards"
  );
  const setInventoryViewPreference = useCallback((view: ViewMode) => {
    setInventoryView(view);
    localStorage.setItem(PREFERENCE_KEYS.INVENTORY_VIEW, view);
  }, []);
  const [inventoryCardColumns, setInventoryCardColumns] = useState(() => {
    const saved = Number(localStorage.getItem(PREFERENCE_KEYS.INVENTORY_CARD_COLUMNS));
    return Number.isInteger(saved) && saved >= 5 && saved <= 24 ? saved : 9;
  });
  const setInventoryCardColumnsPreference = useCallback((columns: number) => {
    const next = Math.max(5, Math.min(24, columns));
    setInventoryCardColumns(next);
    localStorage.setItem(PREFERENCE_KEYS.INVENTORY_CARD_COLUMNS, String(next));
  }, []);
  const [inventoryListTextScale, setInventoryListTextScale] = useState(() => {
    const saved = Number(localStorage.getItem(PREFERENCE_KEYS.INVENTORY_LIST_TEXT_SCALE));
    return Number.isInteger(saved) && saved >= 80 && saved <= 150 && saved % 10 === 0 ? saved : 100;
  });
  const setInventoryListTextScalePreference = useCallback((scale: number) => {
    const next = Math.max(80, Math.min(150, Math.round(scale / 10) * 10));
    setInventoryListTextScale(next);
    localStorage.setItem(PREFERENCE_KEYS.INVENTORY_LIST_TEXT_SCALE, String(next));
  }, []);
  const [showSettings, setShowSettings] = useState(false);
  const [settingsTab, setSettingsTab] = useState<'general' | 'overlays' | 'market' | 'filters' | 'accessibility' | 'notifications' | 'data' | 'debugging'>('general');
  const [settingsFilterModule, setSettingsFilterModule] = useState<FilterPresetModule>("inventory");
  const openFilterSettings = useCallback((module: FilterPresetModule) => {
    setSettingsFilterModule(module);
    setSettingsTab("filters");
    setShowSettings(true);
  }, []);
  const [clearMsg, setClearMsg] = useState("");
  const [appVersion, setAppVersion] = useState("");
  const [blobLogSize, setBlobLogSize] = useState(0);
  const [apiLogSize, setApiLogSize] = useState(0);
  const [rawScanSize, setRawScanSize] = useState(0);
  const [probeSize, setProbeSize] = useState(0);
  const [debugCatEnabled, setDebugCatEnabled] = useState(false);
  const [showInventoryBatchPreview, setShowInventoryBatchPreview] = useState(false);
  const [unmatchedPathsSize, setUnmatchedPathsSize] = useState(0);
  const [blobStage, setBlobStage] = useState<"scanning" | "done" | null>(null);
  const blobDoneTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const handleWfmLoginChange = useCallback((loggedIn: boolean) => {
    setWfmLoggedIn(loggedIn);
    wfmLoggedInRef.current = loggedIn;
  }, []);

  settingsRef.current = { arbitrationFavorites: arbFavorites, arbitrationLeadMins: arbLeadMins, arbitrationOverlayEnabled: arbOverlayEnabled, arbitrationTierFilter: arbTierFilter, arbitrationAlertTiers: arbAlertTiers, arbitrationScheduleDays: arbScheduleDays, overlayEnabled, overlayPriority, overlayOffsets, rivenEnabled, textScale, colorblindMode, clockFormat, companionApiEnabled, memoryScannerEnabled, blobLogEnabled, apiLogEnabled, autoDiagEnabled, tracked, favorites, timerFavorites, fissureWatches, fissureNotifications, modularWidth, modularSectionOrder, modularPopout, wfmInvisibleOnStart, wfmInvisibleOnClose, wfmAutoInvisible, wfmAutoInvisibleMins, wfmRecordSales, relicPickEnabled, relicPickPriority, relicPickRefinement, relicPickLines, foundryPageSize, memTriggerEnabled, filterPresets };

  // ── Debug data sizes — reload when the Debugging settings tab opens ─────────
  const reloadDebugSizes = useCallback(() => {
    invoke<number>("get_debug_data_size", { which: "blobs"           }).then(setBlobLogSize).catch(() => {});
    invoke<number>("get_debug_data_size", { which: "api_logs"        }).then(setApiLogSize).catch(() => {});
    invoke<number>("get_debug_data_size", { which: "raw_scan"        }).then(setRawScanSize).catch(() => {});
    invoke<number>("get_debug_data_size", { which: "probe"           }).then(setProbeSize).catch(() => {});
    invoke<number>("get_debug_data_size", { which: "unmatched_paths" }).then(setUnmatchedPathsSize).catch(() => {});
    invoke<number>("get_diag_folder_size").then(setDiagFolderSize).catch(() => {});
  }, []);

  useEffect(() => {
    if (showSettings && settingsTab === "debugging") reloadDebugSizes();
  }, [showSettings, settingsTab]); // eslint-disable-line

  // ── WFM auto-login at app start ───────────────────────────────────────────
  // Restores the session into Rust's AppState so the Trading tab is instantly
  // ready when the user opens it — no need to visit the tab first.
  // Also pre-warms the top-items cache in the background so the Statistics tab
  // loads instantly rather than running ~2 minutes of API calls on first open.
  useEffect(() => {
    (async () => {
      const creds = await invoke<WfmCredentials | null>(TAURI_COMMANDS.WFM_LOAD_CREDENTIALS).catch(() => null);
      if (creds) {
        const session = await invoke<WfmSession | null>(TAURI_COMMANDS.WFM_SET_JWT, { jwt: creds[1] }).catch(() => null);
        if (session) {
          setWfmLoggedIn(true);
          wfmLoggedInRef.current = true;
          if (wfmInvisibleOnStartRef.current) {
            invoke(TAURI_COMMANDS.WFM_SET_STATUS, { status: "invisible" }).catch(() => {});
          }
        }
      }
    })();
    // Fire-and-forget: populates WFM_TOP_CACHE so the Statistics tab is instant
    invoke(TAURI_COMMANDS.GET_WFM_TOP_ITEMS).catch(() => {});
  }, []); // eslint-disable-line

  // ── WFM: intercept window close to go invisible first ─────────────────────
  // Only runs in the main window — overlay/test windows must not call force_quit.
  useEffect(() => {
    if (_winLabel !== "main") return;
    let unlistenFn: (() => void) | null = null;
    getCurrentWindow().onCloseRequested(async event => {
      event.preventDefault();
      if (wfmInvisibleOnCloseRef.current && wfmLoggedInRef.current) {
        await Promise.race([
          invoke(TAURI_COMMANDS.WFM_SET_STATUS, { status: "invisible" }).catch(() => {}),
          new Promise<void>(resolve => setTimeout(resolve, 8000)),
        ]);
      }
      invoke("force_quit").catch(() => {});
    }).then(fn => { unlistenFn = fn; });
    return () => { unlistenFn?.(); };
  }, []); // eslint-disable-line

  // ── WFM: auto-invisible countdown timer ───────────────────────────────────
  useEffect(() => {
    if (!wfmAutoInvisible || !wfmLoggedIn) return;
    const id = setTimeout(() => {
      invoke(TAURI_COMMANDS.WFM_SET_STATUS, { status: "invisible" }).catch(() => {});
    }, wfmAutoInvisibleMins * 60 * 1000);
    return () => clearTimeout(id);
  }, [wfmAutoInvisible, wfmAutoInvisibleMins, wfmLoggedIn]);

  // ── Bootstrap ──────────────────────────────────────────────────────────────

  useEffect(() => {
    loadSettings().then(settings => {
      if (settings) {
        applyModularSettings(settings);
        applyTimerSettings(settings);
        applyArbitrationSettings(settings);
        arbitrationAlerts.restoreFired(settings);
        settingsLoadedRef.current = true;
      }
    });
    invoke<number>("get_diag_folder_size").then(setDiagFolderSize).catch(() => {});
    getVersion().then(v => { setAppVersion(v); }).catch(() => {});
  }, [loadSettings, applyModularSettings, applyTimerSettings, applyArbitrationSettings, arbitrationAlerts.restoreFired]);

  useEffect(() => {
    const unlisten = listen(TAURI_EVENTS.SETTINGS_UPDATED, () => {
      invoke<string>(TAURI_COMMANDS.LOAD_SETTINGS).then(json => {
        if (!json) return;
        try {
          const updated = JSON.parse(json) as SettingsFile;
          applyModularSettings(updated);
          applyTimerSettings(updated);
          applyArbitrationSettings(updated);
        } catch {}
      }).catch(() => {});
    });
    return () => { unlisten.then(fn => fn()); };
  }, [applyModularSettings, applyTimerSettings, applyArbitrationSettings]);

  // Refresh diagnostics folder size every minute so the Clear button stays current.
  useEffect(() => {
    const id = setInterval(() => {
      invoke<number>("get_diag_folder_size").then(setDiagFolderSize).catch(() => {});
    }, 60_000);
    return () => clearInterval(id);
  }, []);

  // ── Inventory update events ────────────────────────────────────────────────

  // ── Blob processing status ────────────────────────────────────────────────
  useEffect(() => {
    const unlisten = listen<BlobStatusPayload>(TAURI_EVENTS.BLOB_STATUS, e => {
      const { stage } = e.payload;
      if (stage === "scanning") {
        if (blobDoneTimerRef.current) clearTimeout(blobDoneTimerRef.current);
        setBlobStage("scanning");
      } else if (stage === "done") {
        setBlobStage("done");
        blobDoneTimerRef.current = setTimeout(() => setBlobStage(null), 4000);
      }
    });
    return () => { unlisten.then(fn => fn()); };
  }, []);

  // ── Persist main window geometry on move/resize ───────────────────────────
  useEffect(() => {
    const win = getCurrentWindow();
    let t: ReturnType<typeof setTimeout> | null = null;
    const save = () => {
      if (t) clearTimeout(t);
      t = setTimeout(() => {
        Promise.all([win.outerPosition(), win.outerSize()]).then(([pos, size]) => {
          const patch: SettingsPatch = {
            windowX: pos.x, windowY: pos.y,
            windowWidth: size.width, windowHeight: size.height,
          };
          invoke(TAURI_COMMANDS.SAVE_SETTINGS, { json: JSON.stringify(patch) }).catch(() => {});
        }).catch(() => {});
      }, 400);
    };
    const unlistenMove = win.onMoved(save);
    const unlistenResize = win.onResized(save);
    return () => {
      if (t) clearTimeout(t);
      unlistenMove.then(fn => fn());
      unlistenResize.then(fn => fn());
    };
  }, []); // eslint-disable-line

  // ── Auto-update check ─────────────────────────────────────────────────────
  useEffect(() => {
    // A dev build must never download a release over itself.
    if (IS_DEV) return;
    invoke<string | null>("check_for_update")
      .then(v => { if (v) setPendingUpdate(v); })
      .catch(() => {});
  }, []); // eslint-disable-line

  useEffect(() => {
    if (settingsLoadedRef.current) saveAllSettings();
  }, [tracked, favorites, timerFavorites, fissureWatches, fissureNotifications, arbFavorites, arbLeadMins, arbTierFilter, arbAlertTiers, arbScheduleDays, memoryScannerEnabled, companionApiEnabled, blobLogEnabled, apiLogEnabled, autoDiagEnabled, modularSectionOrder, modularPopout, filterPresets]); // eslint-disable-line

  const commitModularWidth = useCallback((width: number) => {
    const patch: SettingsPatch = { modularWidth: width };
    invoke(TAURI_COMMANDS.SAVE_SETTINGS, { json: JSON.stringify(patch) }).catch(() => {});
  }, []);

  // ── Derived data ───────────────────────────────────────────────────────────

  // Central inventory: keyed by display name AND unique_name path (alias).
  // Both inventory["Ash Prime"] and inventory["/Lotus/Powersuits/Ninja/AshPrime"] resolve to the same entry.
  const inventory = useMemo(() => {
    const pathToCatalog = new Map<string, CatalogItem>();
    for (const item of catalog) pathToCatalog.set(item.unique_name, item);

    const allPaths = new Set([
      ...Object.keys(quantities),
      ...(companionApiEnabled ? Object.keys(apiQuantities) : []),
      ...Object.keys(masteryData),
      ...Object.keys(archonShards),
      ...Object.keys(scannerMods),
      ...subsummedWarframes,
    ]);

    const inv: Record<string, InventoryItem> = {};
    for (const path of allPaths) {
      const cat = pathToCatalog.get(path);
      const name = cat?.name ?? path;
      let qty = quantities[path] ?? 0;
      if (scannerMods[path]) qty = Math.max(qty, scannerMods[path].total);
      if (companionApiEnabled) qty = Math.max(qty, apiQuantities[path] ?? 0);
      if (subsummedWarframes.has(path)) qty = 0;

      const entry: InventoryItem = {
        unique_name:   path,
        quantity:      qty,
        mastery_rank:  masteryData[path] ?? 0,
        archon_shards: archonShards[path] ?? [],
        forma_count:   formaData[path] ?? 0,
        subsumed:      subsummedWarframes.has(path),
        vaulted:       cat?.vaulted ?? null,
        category:      cat?.category ?? "",
        ducat_price:   cat?.ducats ?? null,
        wfm_price:     null,
        image_name:    cat?.image_name ?? null,
        mastery_req:   cat?.mastery_req ?? null,
      };
      inv[name] = entry;
      if (path !== name) inv[path] = entry; // path alias so existing unique_name lookups still work
    }
    return inv;
  }, [catalog, quantities, apiQuantities, masteryData, archonShards, formaData, subsummedWarframes, companionApiEnabled, scannerMods]);

  const modCopiesMap = useMemo(() => {
    const map: Record<string, ModCopy[]> = {};
    for (const c of apiModCopies) {
      if (!map[c.uniqueName]) map[c.uniqueName] = [];
      map[c.uniqueName].push(c);
    }
    // Fill in rank breakdown from scanner for mods not covered by API data.
    for (const [path, mc] of Object.entries(scannerMods)) {
      if (!map[path]) {
        map[path] = Object.entries(mc.by_rank)
          .map(([rankStr, count]) => ({ uniqueName: path, rank: parseInt(rankStr), count }))
          .sort((a, b) => (b.rank ?? -1) - (a.rank ?? -1));
      }
    }
    // Sort each entry: highest rank first, then rank-0, then raw (null)
    for (const copies of Object.values(map)) {
      copies.sort((a, b) => (b.rank ?? -1) - (a.rank ?? -1));
    }
    return map;
  }, [apiModCopies, scannerMods]);

  const inventorySynced = Object.keys(quantities).length > 0;

  const availableRanks = useMemo(() => {
    const set = new Set<number>();
    for (const c of apiModCopies) if (c.rank !== null && c.rank > 0) set.add(c.rank);
    return [...set].sort((a, b) => a - b);
  }, [apiModCopies]);

  // Total counts only depend on the catalog — stable until item list is refreshed.
  const categoryTotals = useMemo(() => {
    const total: Record<string, number> = { all: catalog.length };
    for (const item of catalog) total[item.category] = (total[item.category] ?? 0) + 1;
    return total;
  }, [catalog]);

  // Owned counts depend on quantities — recalculates every inventory scan.
  const categoryOwned = useMemo(() => {
    const owned: Record<string, number> = { all: 0 };
    for (const item of catalog) {
      if ((inventory[item.unique_name]?.quantity ?? 0) > 0) {
        owned.all++;
        owned[item.category] = (owned[item.category] ?? 0) + 1;
      }
    }
    return owned;
  }, [catalog, inventory]);

  const categoryCounts = useMemo(
    () => ({ owned: categoryOwned, total: categoryTotals }),
    [categoryOwned, categoryTotals]
  );

  const favoritesSet = useMemo(() => new Set(favorites), [favorites]);

  const changeLogMap = useMemo(() => {
    const m = new Map<string, ChangeLogEntry[]>();
    for (const c of changeLog) {
      const arr = m.get(c.unique_name);
      if (arr) arr.push(c);
      else m.set(c.unique_name, [c]);
    }
    return m;
  }, [changeLog]);

  const craftingMap = useMemo(() => {
    const m = new Map<string, CraftingJob>();
    for (const c of crafting) m.set(c.unique_name, c);
    return m;
  }, [crafting]);

  const visibleItems = useMemo(() => {
    const searchTerms = splitSearchTerms(search);
    const out: (CatalogItem & { qty: number; plat: number | null })[] = [];
    for (const i of catalog) {
      if (i.name === "Blueprint") continue;
      if (category !== "all" && i.category !== category) continue;
      if (!matchesSearchTerms(searchTerms, i.name)) continue;
      const qty = inventory[i.unique_name]?.quantity ?? 0;
      if (filterOwned    && qty === 0) continue;
      if (filterRecent   && lastChanged[i.unique_name] == null) continue;
      if (filterPrime    && !i.name.includes("Prime") && i.vaulted == null) continue;
      if (filterVaulted  && i.vaulted !== true) continue;
      if (filterUnvaulted && i.vaulted !== false) continue;
      const plat = bulkPrices.get(i.name.toLowerCase()) ?? null;
      // Explicit corrections are authoritative; otherwise match the production
      // heuristic from get_item_price: has a price, ducats, or a mod/arcane category.
      const isTradeable = i.tradeable_wfm ?? (
        plat != null || (i.ducats != null && i.ducats > 0) || i.category === "Mods" || i.category === "Arcanes"
      );
      if (filterTradeable && !isTradeable) continue;
      if (isDucatRatioSort) {
        if (platinumPriceStatus === "ready" && (!(plat != null && plat > 0) || !(i.ducats != null && i.ducats > 0))) continue;
      } else {
        if (isPlatinumSort && platinumPriceStatus === "ready" && !(plat != null && plat > 0)) continue;
        if (isDucatSort && !(i.ducats != null && i.ducats > 0)) continue;
      }
      if (filterRank !== null) {
        if (i.category === "Mods" || i.category === "Arcanes") {
          const copies = modCopiesMap[i.unique_name];
          if (!copies) continue;
          if (filterRank === "unranked") {
            if (!copies.some(c => c.rank === null || c.rank === 0)) continue;
          } else {
            if (!copies.some(c => c.rank === filterRank)) continue;
          }
        }
      }
      out.push({ ...i, qty, plat });
    }
    const activeSortMode = needsPlatinumPrices && platinumPriceStatus !== "ready" ? lastPriceIndependentSortRef.current : sortMode;
    out.sort((a, b) => {
      if (activeSortMode === "plat-asc") return (a.plat ?? 0) - (b.plat ?? 0) || a.name.localeCompare(b.name);
      if (activeSortMode === "plat-desc") return (b.plat ?? 0) - (a.plat ?? 0) || a.name.localeCompare(b.name);
      if (activeSortMode === "ducat-asc") return (a.ducats ?? 0) - (b.ducats ?? 0) || a.name.localeCompare(b.name);
      if (activeSortMode === "ducat-desc") return (b.ducats ?? 0) - (a.ducats ?? 0) || a.name.localeCompare(b.name);
      if (activeSortMode === "ducat-ratio-asc") return (a.ducats! / a.plat!) - (b.ducats! / b.plat!) || a.name.localeCompare(b.name);
      if (activeSortMode === "ducat-ratio-desc") return (b.ducats! / b.plat!) - (a.ducats! / a.plat!) || a.name.localeCompare(b.name);
      const aOwned = a.qty > 0 ? 1 : 0;
      const bOwned = b.qty > 0 ? 1 : 0;
      if (bOwned !== aOwned) return bOwned - aOwned;
      if (activeSortMode === "name-asc")  return a.name.localeCompare(b.name);
      if (activeSortMode === "name-desc") return b.name.localeCompare(a.name);
      if (activeSortMode === "qty-asc")   return a.qty - b.qty || a.name.localeCompare(b.name);
      return b.qty - a.qty || a.name.localeCompare(b.name);
    });
    return out.slice(0, 1000);
  }, [catalog, inventory, inventoryFilters, bulkPrices, isDucatRatioSort, isDucatSort, isPlatinumSort, needsPlatinumPrices, platinumPriceStatus, lastChanged, modCopiesMap]);

  const resetInventoryFilters = ({
    recent,
    searchTerm = "",
    categoryId = "all",
  }: {
    recent: boolean;
    searchTerm?: string;
    categoryId?: string;
  }) => {
    activateModule("inventory");
    setInventoryFilters(previous => ({
      ...INVENTORY_FILTERS_DEFAULT,
      category: categoryId,
      search: searchTerm,
      filterRecent: recent,
      sortMode: previous.sortMode,
    }));
  };

  // Navigate to an item from the changelog — only switches module and sets search,
  // leaving any existing inventory filters in place (chip filters the user set should survive).
  const openChangeLogItem = (uniqueName: string) => {
    const item = catalog.find(candidate => candidate.unique_name === uniqueName);
    activateModule("inventory");
    setInventoryFilters(previous => ({ ...previous, search: item?.name ?? "" }));
  };

  const openRecentChanges = () => resetInventoryFilters({ recent: true });
  const openRecentCategory = (categoryId: string) => resetInventoryFilters({ recent: true, categoryId });
  const closeInventoryBatchPreview = useCallback(() => setShowInventoryBatchPreview(false), []);
  const handleInventoryContextMenu = useCallback((e: React.MouseEvent) => {
    const name = extractItemName(e);
    if (name) {
      e.preventDefault();
      openCtx(e.clientX, e.clientY, [
        { label: "Open Wiki", action: () => openWiki(name) },
        { label: "Copy Wiki Link", action: () => copyWikiLink(name) },
      ]);
    }
  }, [openCtx]);

  // ─── Render ─────────────────────────────────────────────────────────────────

  return (
    <ImgCacheDirContext.Provider value={imgCacheDir}>
    <div className="flex h-full flex-col overflow-hidden">

      {/* ── Header ── */}
      <header className="flex h-(--header-h) shrink-0 items-center gap-3 border-b border-border bg-surface px-4">
        <span className="text-15 font-semibold text-foreground">{APP_TITLE}</span>
        <HeaderStatusBadges
          masteryRank={masteryRank}
          playerName={playerName}
          pendingUpdate={pendingUpdate}
          updateInstalling={updateInstalling}
          inventoryLoaded={blobStage === "done"}
          onInstallUpdate={() => {
            if (updateInstalling) return;
            setUpdateInstalling(true);
            invoke("install_update").catch(() => setUpdateInstalling(false));
          }}
          onDismissUpdate={() => setPendingUpdate(null)}
        />
        <div className="ml-auto flex items-center gap-1.5">
          {/* ── Connection status chips ── */}
          {(() => {
            // Memory chip
            const scanState: "online"|"warn"|"offline"|"disabled" =
              !memoryScannerEnabled ? "disabled"
              : warframeRunning     ? "online"
              : "offline";
            const scanDetail =
              !memoryScannerEnabled ? "OFF"
              : !monitoring         ? "Idle"
              : warframeRunning     ? "Scanning"
              : poking              ? "Checking…"
              : "No Game";

            // WF API chip
            const wfApiState: "online"|"warn"|"offline"|"disabled" =
              !companionApiEnabled                    ? "disabled"
              : wfConnected                           ? "online"
              : warframeRunning                       ? "warn"
              : "offline";
            const wfApiDetail =
              !companionApiEnabled                    ? "OFF"
              : wfConnected && lastApiRefresh         ? formatUnixTime(lastApiRefresh, clockFormat, systemLocale)
              : wfConnected                           ? "Connected"
              : warframeRunning                       ? "Connecting…"
              : "Waiting";
            const wfApiClick = (!wfConnected && warframeRunning && companionApiEnabled)
              ? async () => {
                  try {
                    const [accountId, nonce, steamId] = await invoke<WarframeCredentials>("scan_warframe_credentials");
                    const args: WarframeInventoryRequest = { accountId, nonce, steamId };
                    const data = await invoke<unknown>("fetch_warframe_inventory", args);
                    applyInventoryData(data);
                    setWfConnected(true);
                    wfConnectedRef.current = true;
                    manualCredsRef.current = { accountId, nonce };
                  } catch (e) {
                    alert(`Credential scan failed:\n${e}\n\nMake sure you are in the Orbiter (not in a mission or loading screen).`);
                  }
                }
              : undefined;

            // WFM chip
            const wfmState: "online"|"offline" = wfmLoggedIn ? "online" : "offline";
            const wfmDetail = wfmLoggedIn ? "Online" : "Not logged in";

            return (
              <>
                <ConnectionStatusChip
                  label="Memory"
                  state={scanState}
                  detail={scanDetail}
                  title={!memoryScannerEnabled ? "Memory scanner disabled — enable in Settings" : warframeRunning ? "Warframe detected — scanning memory" : "Click to recheck for Warframe"}
                  onClick={
                    !memoryScannerEnabled ? () => setShowSettings(true)
                    : !warframeRunning && monitoring ? () => {
                        setPoking(true);
                        invoke("poke_scan").finally(() => setTimeout(() => setPoking(false), 3000));
                      }
                    : undefined
                  }
                />
                <ConnectionStatusChip
                  label="WF API"
                  state={wfApiState}
                  detail={wfApiDetail}
                  title={!companionApiEnabled ? "Warframe API disabled — enable in Settings" : wfConnected ? "Warframe API connected — auto-refreshes every 30s" : warframeRunning ? "Click to retry credential scan" : "Waiting for Warframe to start"}
                  onClick={!companionApiEnabled ? () => setShowSettings(true) : wfApiClick}
                />
                <ConnectionStatusChip
                  label="WFM"
                  state={wfmState}
                  detail={wfmDetail}
                  title={wfmLoggedIn ? "Logged in to warframe.market" : "Not logged in to warframe.market — open the Market tab to log in"}
                  onClick={!wfmLoggedIn ? () => activateModule("market") : undefined}
                />
                {overlayStatus && (
                  <span className={`${CONN_CHIP} ${CONN_STATUS.overlay.chip}`}>
                    <span className={`${CONN_DOT} ${CONN_STATUS.overlay.dot}`} />
                    <span className={CONN_STATUS.overlay.detail}>{overlayStatus}</span>
                  </span>
                )}
                <CacheStatusChip />
              </>
            );
          })()}
          <HeaderActions
            onOpenExternalUrl={url => invoke(TAURI_COMMANDS.OPEN_URL, { url }).catch(() => {})}
            onOpenSettings={() => {
              setShowSettings(true);
              setClearMsg("");
              getVersion().then(version => setAppVersion(version)).catch(() => {});
            }}
          />
        </div>
      </header>

      <SettingsModal open={showSettings} onClose={() => setShowSettings(false)} {...{ settingsTab, setSettingsTab, settingsFilterModule, setSettingsFilterModule, filterPresets, setFilterPresets, inventoryFilters, setInventoryFilters, foundryFilters, setFoundryFilters, marketFilters, setMarketFilters, relicFilters, setRelicFilters, foundryPageSize, setFoundryPageSize, settingsRef, saveAllSettings, memoryScannerEnabled, setMemoryScannerEnabled, modularPopout, setModularPopout, overlayStatus, overlayEnabled, setOverlayEnabled, overlayPriority, setOverlayPriority, overlayOffsets, setOverlayOffsets, rivenEnabled, setRivenEnabled, memTriggerEnabled, setMemTriggerEnabled, relicPickEnabled, setRelicPickEnabled, relicPickPriority, setRelicPickPriority, relicPickLines, setRelicPickLines, wfmLoggedIn, wfmInvisibleOnStart, setWfmInvisibleOnStart, wfmInvisibleOnStartRef, wfmInvisibleOnClose, setWfmInvisibleOnClose, wfmInvisibleOnCloseRef, wfmAutoInvisible, setWfmAutoInvisible, wfmAutoInvisibleMins, setWfmAutoInvisibleMins, wfmRecordSales, setWfmRecordSales, colorblindMode, setColorblindMode, textScale, setTextScale, clockFormat, setClockFormat, systemLocale, itemCount, recipeCount, handleFetch, fetching, fetchMsg, fissureNotifications, onFissureNotificationsChange: setFissureNotifications, setQuantities, setApiQuantities, setApiModCopies, setScannerMods, setMasteryData, setArchonShards, setFormaData, setChangeLog, setLastChanged, setWfConnected, wfConnectedRef, setItemsRefreshKey, setClearMsg, clearMsg, blobLogEnabled, setBlobLogEnabled, blobLogSize, setBlobLogSize, companionApiEnabled, apiLogEnabled, setApiLogEnabled, apiLogSize, setApiLogSize, setShowInventoryBatchPreview, notifyTestResult, setNotifyTestResult, overlayLogCopied, setOverlayLogCopied, autoDiagEnabled, setAutoDiagEnabled, diagFolderSize, setDiagFolderSize, diagPath, diagCapturing, setDiagCapturing, setDiagPath, reloadDebugSizes, memoryProbing, setMemoryProbing, probeSize, setProbeSize, rawScanning, setRawScanning, rawScanSize, setRawScanSize, memRelicDebugRunning, setMemRelicDebugRunning, relicPickOcrResult, relicPickOcrTesting, setRelicPickOcrTesting, setRelicPickOcrResult, relicPickTestResult, relicPickTestEra, setRelicPickTestEra, setRelicPickTestResult, eeLogTail, setEeLogTail, debugCatEnabled, setDebugCatEnabled, unmatchedPathsSize, setUnmatchedPathsSize, appVersion, arbOverlayEnabled, setArbOverlayEnabled }} />

      {showInventoryBatchPreview && <InventoryBatchPreview onClose={closeInventoryBatchPreview} />}

      <div className="flex flex-1 overflow-hidden min-h-0">

        <AppNavigation activeModule={activeModule} onModuleChange={activateModule} />

        <div className="flex min-w-0 min-h-0 flex-1 flex-col overflow-hidden">
        <div className="flex min-w-0 min-h-0 flex-1">
        {/* ── Inventory module ── */}
        {visitedModules.has("inventory") && (
        <KeepMountedWhenHidden active={activeModule === "inventory"}>
          <>
            <InventorySidebar
              categories={CATEGORIES}
              category={category}
              categoryCounts={categoryCounts}
              onCategoryChange={category => setInventoryFilters(previous => ({ ...previous, category }))}
              itemCount={itemCount}
              recipeCount={recipeCount}
              onFetch={handleFetch}
              fetching={fetching}
              fetchMsg={fetchMsg}
            />

            <div className="flex min-w-0 min-h-0 flex-1 flex-col overflow-hidden">
              {monitoring && warframeRunning && !inventorySynced && (
                <div className="shrink-0 border-b border-caution/20 bg-caution/8 px-4 py-2 text-xs text-caution">
                  Inventory not synced yet — complete a mission or visit a relay to load your inventory
                </div>
              )}

              <InventoryToolbar
                filters={inventoryFilters}
                onFiltersChange={setInventoryFilters}
                onToggleRecent={toggleInventoryRecent}
                platinumPriceStatus={platinumPriceStatus}
                availableRanks={availableRanks}
                showRankFilters={apiModCopies.length > 0}
                itemCount={visibleItems.length}
                view={inventoryView}
                onViewChange={setInventoryViewPreference}
                cardColumns={inventoryCardColumns}
                onCardColumnsChange={setInventoryCardColumnsPreference}
                listTextScale={inventoryListTextScale}
                onListTextScaleChange={setInventoryListTextScalePreference}
                filterPresets={filterPresets}
                onFilterPresetsChange={setFilterPresets}
                onOpenSettings={openFilterSettings}
              />

              <InventoryGrid
                items={visibleItems}
                loading={!inventoryReady}
                monitoring={monitoring}
                view={inventoryView}
                cardColumns={inventoryCardColumns}
                listTextScale={inventoryListTextScale}
                inventory={inventory}
                modCopies={modCopiesMap}
                favorites={favoritesSet}
                lastChanged={lastChanged}
                changes={changeLogMap}
                crafting={craftingMap}
                filterRank={filterRank}
                onToggleFavorite={toggleFavorite}
                onContextMenu={handleInventoryContextMenu}
              />

            </div>
          </>
        </KeepMountedWhenHidden>
        )}

        {ctxMenu && <CtxMenu state={ctxMenu} onClose={closeCtx} />}

        {/* ── Foundry module ── */}
        {visitedModules.has("foundry") && (
        <KeepMountedWhenHidden active={activeModule === "foundry"}>
          <ErrorBoundary>
            <Foundry inventory={inventory} refreshKey={itemsRefreshKey} crafting={crafting} filters={foundryFilters} onFiltersChange={setFoundryFilters} filterPresets={filterPresets} onFilterPresetsChange={setFilterPresets} onOpenSettings={openFilterSettings} colorblindMode={colorblindMode} subsummedWarframes={subsummedWarframes} tracked={tracked} onTrackToggle={toggleTracked} pageSize={foundryPageSize} />
          </ErrorBoundary>
        </KeepMountedWhenHidden>
        )}

        {/* ── Market Helper module ── */}
        {/* Keep mounted at all times so WfmTrading's trade-completed listener
            (auto listing update) fires regardless of which tab is active. */}
        <KeepMountedWhenHidden active={activeModule === "market"}>
          <MarketHelper inventory={inventory} refreshKey={itemsRefreshKey} crafting={crafting} filters={marketFilters} onFiltersChange={setMarketFilters} filterPresets={filterPresets} onFilterPresetsChange={setFilterPresets} onOpenSettings={openFilterSettings} onWfmLoginChange={handleWfmLoginChange} modCopiesMap={modCopiesMap} wfmRecordSales={wfmRecordSales} />
        </KeepMountedWhenHidden>

        {/* ── Relics module ── */}
        {visitedModules.has("relics") && (
        <KeepMountedWhenHidden active={activeModule === "relics"}>
          <ErrorBoundary>
            <RelicHelper inventory={inventory} filters={relicFilters} onFiltersChange={setRelicFilters} filterPresets={filterPresets} onFilterPresetsChange={setFilterPresets} onOpenSettings={openFilterSettings} colorblindMode={colorblindMode} />
          </ErrorBoundary>
        </KeepMountedWhenHidden>
        )}

        {/* ── Rivens module ── */}
        {visitedModules.has("rivens") && (
        <KeepMountedWhenHidden active={activeModule === "rivens"}>
          <ErrorBoundary>
            <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
              <RivenAnalyzer />
            </div>
          </ErrorBoundary>
        </KeepMountedWhenHidden>
        )}

        {/* ── Timers module ── */}
        {visitedModules.has("timers") && (
        <KeepMountedWhenHidden active={activeModule === "timers"}>
          <ErrorBoundary>
            <TimerHelper
              active={activeModule === "timers"}
              favorites={timerFavorites}
              onFavoriteToggle={id => setTimerFavorites(prev =>
                prev.includes(id) ? prev.filter(x => x !== id) : [...prev, id]
              )}
              fissureWatches={fissureWatches}
              onAddWatch={w => setFissureWatches(prev => [...prev, w])}
              onRemoveWatch={id => setFissureWatches(prev => prev.filter(w => w.id !== id))}
              fissureNotifications={fissureNotifications}
              onFissureNotificationsChange={setFissureNotifications}
              inventory={inventory}
            />
          </ErrorBoundary>
        </KeepMountedWhenHidden>
        )}

        {/* ── Arbitrations module ── */}
        {visitedModules.has("arbitrations") && (
        <KeepMountedWhenHidden active={activeModule === "arbitrations"}>
          <ErrorBoundary>
            <Arbitrations
              favorites={arbFavorites}
              onToggleFavorite={id => setArbFavorites(prev =>
                prev.includes(id) ? prev.filter(x => x !== id) : [...prev, id]
              )}
              leadMins={arbLeadMins}
              onLeadChange={setArbLeadMins}
              permissionDenied={arbitrationAlerts.permissionDenied}
              onPermissionChange={arbitrationAlerts.setPermissionDenied}
              tierFilter={arbTierFilter}
              onTierFilterChange={setArbTierFilter}
              alertTiers={arbAlertTiers}
              onAlertTiersChange={setArbAlertTiers}
              scheduleDays={arbScheduleDays}
              onScheduleDaysChange={setArbScheduleDays}
              clockFormat={clockFormat} systemLocale={systemLocale}
            />
          </ErrorBoundary>
        </KeepMountedWhenHidden>
        )}

        {/* ── Statistics module ── */}
        {visitedModules.has("statistics") && (
        <KeepMountedWhenHidden active={activeModule === "statistics"}>
          <ErrorBoundary>
            <Statistics clockFormat={clockFormat} systemLocale={systemLocale} />
          </ErrorBoundary>
        </KeepMountedWhenHidden>
        )}

        {/* ── Completionist module ── */}
        {visitedModules.has("completionist") && (
        <KeepMountedWhenHidden active={activeModule === "completionist"}>
          <ErrorBoundary>
            <CompletionistTabs inventory={inventory} />
          </ErrorBoundary>
        </KeepMountedWhenHidden>
        )}

        </div>

        <ChangeLog
          changes={changeLog}
          arrivalToken={changeLogArrivalToken}
          lastScanAt={lastInventoryScanAt}
          catalog={catalog}
          clockFormat={clockFormat}
          systemLocale={systemLocale}
          onItemClick={openChangeLogItem}
          onChangeLogClick={openRecentChanges}
          onCategoryClick={openRecentCategory}
        />
        </div>

        {/* ── Modular Window — always visible unless popped out ── */}
        {!modularPopout && <ModularWindow
          tracked={tracked}
          onTrackedChange={setTracked}
          onUntrack={toggleTracked}
          favorites={favorites}
          onFavoritesChange={setFavorites}
          onUnfavorite={toggleFavorite}
          timerFavorites={timerFavorites}
          onTimerFavoritesChange={setTimerFavorites}
          onTimerUnfavorite={id => setTimerFavorites(prev => prev.filter(x => x !== id))}
          fissureWatches={fissureWatches}
          inventory={inventory}
          catalog={catalog}
          width={modularWidth}
          onWidthChange={setModularWidth}
          onWidthCommit={commitModularWidth}
          sectionOrder={modularSectionOrder}
          onSectionOrderChange={setModularSectionOrder}
        />}

      </div>
    </div>
    </ImgCacheDirContext.Provider>
  );
}
