import { useState, useEffect, useMemo, useCallback, useRef, memo, useContext, Component, ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getVersion } from "@tauri-apps/api/app";
import { listen } from "@tauri-apps/api/event";
import { WebviewWindow } from "@tauri-apps/api/webviewWindow";

// ── Riven overlay — module-level window management ────────────────────────────
// Stored OUTSIDE React so StrictMode remounts don't destroy/recreate the window.
let _rivenWin: WebviewWindow | null = null;
let _rivenRollCount = 0;
let _rivenLastTriggerMs = 0;
let _rivenManualTrigger: (() => void) | null = null;
export function checkRivenNow() { _rivenManualTrigger?.(); }

function rivenWinHide(reason = "rivenWinHide") {
  const win = _rivenWin;
  if (!win) { return; }
  invoke("ocr_riven_log_error", { error: `[HIDE] ${reason}` }).catch(() => {});
  _rivenWin = null;
  win.close().catch(() => {});
}

async function ensureRivenWindow(wx: number, wy: number, wh: number): Promise<{ win: WebviewWindow; fresh: boolean } | null> {
  // 1. Existing valid handle
  if (_rivenWin) return { win: _rivenWin, fresh: false };

  // 2. Window exists but JS lost reference (HMR, page reload)
  const existing = await WebviewWindow.getByLabel("riven-overlay").catch(() => null);
  if (existing) {
    _rivenWin = existing;
    _rivenWin.once("tauri://destroyed", () => { _rivenWin = null; });
    return { win: _rivenWin, fresh: false };
  }

  // 3. Create fresh at correct position — shows immediately
  try {
    _rivenWin = new WebviewWindow("riven-overlay", {
      url: `index.html#rivenoverlay`,
      title: "FrameForge Riven",
      transparent: true, decorations: false,
      alwaysOnTop: true, skipTaskbar: true,
      resizable: false, focus: false,
      x: wx + 10, y: wy + Math.round(wh * 0.20),
      width: 300, height: Math.round(wh * 0.60),
    });
    _rivenWin.once("tauri://destroyed", () => { _rivenWin = null; });
    return { win: _rivenWin, fresh: true };
  } catch {
    _rivenWin = null;
    return null;
  }
}
import { getCurrentWindow, availableMonitors } from "@tauri-apps/api/window";

import { ImgCacheDirContext } from "./ImgCacheDir";
import Foundry from "./Foundry";
import MarketHelper, { MARKET_FILTERS_DEFAULT } from "./MarketHelper";
import RelicHelper, { RELIC_FILTERS_DEFAULT } from "./RelicHelper";
import RivenAnalyzer from "./RivenAnalyzer";
import RivenOverlayWindow from "./RivenOverlayWindow";
import TimerHelper, { FissureWatch } from "./TimerHelper";
import Statistics from "./Statistics";
import Syndicates from "./Syndicates";
import Overlay from "./Overlay";
import ModularWindow from "./ModularWindow";
import { HelpTip } from "./HelpTip";
import "./App.css";

const _winLabel = getCurrentWindow().label;
// Support all URL formats: query string (?overlay), hash (#overlay), or window label.
// v2.0.0 used query strings and they worked fine — keep as primary detection path.
const _params          = new URLSearchParams(window.location.search);
const _hash            = window.location.hash;
const IS_OVERLAY       = _params.has("overlay")      || _hash === "#overlay"      || _winLabel === "relic-overlay";
const IS_MODULAR       = _params.has("modular")      || _hash === "#modular"      || _winLabel === "modular-popout";
const IS_RIVEN_OVERLAY = _params.has("rivenoverlay") || _hash === "#rivenoverlay" || _winLabel === "riven-overlay";
const IS_OVERLAY_TEST  = _params.has("overlaytest")  || _hash === "#overlaytest"  || _winLabel === "overlay-test";

class ErrorBoundary extends Component<{ children: ReactNode }, { err: string | null }> {
  constructor(props: any) { super(props); this.state = { err: null }; }
  static getDerivedStateFromError(e: Error) { return { err: e.message }; }
  render() {
    if (this.state.err)
      return <div style={{ padding: 24, color: "#f85149", fontFamily: "monospace", whiteSpace: "pre-wrap" }}>
        <strong>Render error:</strong>{"\n"}{this.state.err}
      </div>;
    return this.props.children;
  }
}

// ─── Types ────────────────────────────────────────────────────────────────────

interface CatalogItem {
  unique_name: string;
  name: string;
  category: string;
  image_name?: string;
  vaulted?: boolean | null;
  ducats?: number | null;
  mastery_req?: number | null;
}

export interface InventoryItem {
  unique_name: string;
  quantity: number;
  mastery_rank: number;
  archon_shards: { type: string; tauforged: boolean; color: string; boost?: string }[];
  subsumed: boolean;
  vaulted: boolean | null;
  category: string;
  ducat_price: number | null;
  wfm_price: number | null;
  image_name: string | null;
  mastery_req: number | null;
}

interface QuantityChange {
  id: number;
  unique_name: string;
  item_name: string;
  old_qty: number;
  new_qty: number;
  delta: number;
  timestamp: number;
}

interface CraftingJob {
  unique_name: string;
  item_name: string;
  completion_ms: number;
}

interface ModCopy {
  uniqueName: string;
  rank: number | null; // null = raw (RawUpgrades), number = from Upgrades (0 = installed-unranked)
  count: number;
}

interface ArchonShard {
  upgrade_type: string;
  color: string; // raw string from game JSON, e.g. "ACC_CRIMSON", "ACC_AZURE_TAUFORGED"
}

interface InventoryUpdate {
  quantities: Record<string, number>;
  crafting: CraftingJob[];
  mastery_rank?: number;
  mastery_data?: Record<string, number>;
  changes: QuantityChange[];
  warframe_running: boolean;
  scanned_at: number;
  consumed_suits?: string[];
  mods?: Record<string, { total: number; by_rank: Record<string, number> }>;
  socketed_shards?: Record<string, ArchonShard[]>;
  is_full_pass?: boolean;
  player_name?: string;
}

type Module = "inventory" | "foundry" | "market" | "relics" | "rivens" | "timers" | "statistics" | "completionist";

// ─── Constants ────────────────────────────────────────────────────────────────

const CATEGORIES = [
  { id: "all",        label: "All Owned" },
  { id: "Resources",  label: "Resources" },
  { id: "Mods",       label: "Mods" },
  { id: "Relics",     label: "Relics" },
  { id: "Arcanes",    label: "Arcanes" },
  { id: "Warframes",  label: "Warframes" },
  { id: "Primary",    label: "Primary" },
  { id: "Secondary",  label: "Secondary" },
  { id: "Melee",      label: "Melee" },
  { id: "Companions", label: "Companions" },
  { id: "Archwing",   label: "Archwing" },
  { id: "Parts",      label: "Parts" },
  { id: "Blueprints", label: "Blueprints" },
  { id: "Miscellaneous", label: "Miscellaneous" },
  { id: "Sigils",     label: "Sigils" },
  { id: "Glyphs",     label: "Glyphs" },
  { id: "Skins",      label: "Skins" },
  { id: "Railjack",   label: "Railjack" },
];

function BlueprintIcon() {
  return (
    <svg className="item-img-fallback" viewBox="0 0 32 32" fill="none" xmlns="http://www.w3.org/2000/svg">
      <rect x="5" y="2" width="17" height="22" rx="1.5" fill="#0d1f33" stroke="#388bfd" strokeWidth="1.2"/>
      <path d="M18 2 L22 6 L18 6 Z" fill="#388bfd" opacity="0.5"/>
      <line x1="8" y1="11" x2="19" y2="11" stroke="#388bfd" strokeWidth="1" opacity="0.9"/>
      <line x1="8" y1="14" x2="19" y2="14" stroke="#388bfd" strokeWidth="1" opacity="0.9"/>
      <line x1="8" y1="17" x2="14" y2="17" stroke="#388bfd" strokeWidth="1" opacity="0.9"/>
      <circle cx="23" cy="23" r="6" fill="#0d1117" stroke="#388bfd" strokeWidth="1.2"/>
      <line x1="23" y1="20" x2="23" y2="26" stroke="#388bfd" strokeWidth="1.2"/>
      <line x1="20" y1="23" x2="26" y2="23" stroke="#388bfd" strokeWidth="1.2"/>
    </svg>
  );
}

function ItemImg({ imageName, category, size = 32 }: { imageName?: string; category: string; size?: number }) {
  const baseUrl = useContext(ImgCacheDirContext);
  const [localFailed, setLocalFailed] = useState(false);
  const [failed, setFailed] = useState(false);
  const style = { width: size, height: size, flexShrink: 0 as const };
  if (!imageName || failed) {
    if (category === "Blueprints") return <BlueprintIcon />;
    return <span className="item-img-fallback" style={{ ...style, fontSize: size * 0.35 }}>{category[0].toUpperCase()}</span>;
  }
  if (imageName.startsWith("http") || imageName.startsWith("/")) {
    return <img className="item-img" style={style} src={imageName} alt="" loading="lazy" onError={() => setFailed(true)} />;
  }
  const useLocal = Boolean(baseUrl) && !localFailed;
  const src = useLocal ? `${baseUrl}/${imageName}` : `https://cdn.warframestat.us/img/${imageName}`;
  return (
    <img className="item-img" style={style} src={src} alt="" loading="lazy"
      onError={() => useLocal ? setLocalFailed(true) : setFailed(true)} />
  );
}


function fmt(n: number) { return n.toLocaleString(); }
function fmtBytes(n: number) {
  if (n >= 1024 * 1024) return `${(n / (1024 * 1024)).toFixed(1)} MB`;
  if (n >= 1024)        return `${Math.round(n / 1024)} KB`;
  return `${n} B`;
}
function deltaClass(d: number) { return d > 0 ? "delta-pos" : "delta-neg"; }
function deltaText(d: number) { return d > 0 ? `+${fmt(d)}` : fmt(d); }
function timeStr(ts: number) {
  return new Date(ts * 1000).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
}

// ─── Standalone modular window page (runs in pop-out Tauri window) ────────────

function ModularWindowPage() {
  const [tracked, setTracked] = useState<string[]>([]);
  const [favorites, setFavorites] = useState<string[]>([]);
  const [timerFavorites, setTimerFavorites] = useState<string[]>([]);
  const [fissureWatches, setFissureWatches] = useState<FissureWatch[]>([]);
  const [quantities, setQuantities] = useState<Record<string, number>>({});
  const [catalog, setCatalog] = useState<CatalogItem[]>([]);
  const inventory = useMemo<Record<string, InventoryItem>>(() => {
    const pathToCatalog = new Map<string, CatalogItem>();
    for (const item of catalog) pathToCatalog.set(item.unique_name, item);
    const inv: Record<string, InventoryItem> = {};
    for (const [path, qty] of Object.entries(quantities)) {
      const cat = pathToCatalog.get(path);
      const name = cat?.name ?? path;
      const entry: InventoryItem = {
        unique_name:   path,
        quantity:      qty,
        mastery_rank:  0,
        archon_shards: [],
        subsumed:      false,
        vaulted:       cat?.vaulted ?? null,
        category:      cat?.category ?? "",
        ducat_price:   cat?.ducats ?? null,
        wfm_price:     null,
        image_name:    cat?.image_name ?? null,
        mastery_req:   cat?.mastery_req ?? null,
      };
      inv[name] = entry;
      if (path !== name) inv[path] = entry;
    }
    return inv;
  }, [catalog, quantities]);
  const [sectionOrder, setSectionOrder] = useState<string[]>(["tracking", "favorites", "timers", "fissures"]);

  useEffect(() => {
    invoke<string>("load_settings").then(json => {
      if (!json) return;
      try {
        const s = JSON.parse(json);
        if (Array.isArray(s.tracked)) setTracked(s.tracked);
        if (Array.isArray(s.favorites)) setFavorites(s.favorites);
        if (Array.isArray(s.timerFavorites)) setTimerFavorites(s.timerFavorites);
        if (Array.isArray(s.fissureWatches)) setFissureWatches(s.fissureWatches);
        if (Array.isArray(s.modularSectionOrder)) {
          const order: string[] = s.modularSectionOrder;
          if (!order.includes("timers"))   order.push("timers");
          if (!order.includes("fissures")) order.push("fissures");
          setSectionOrder(order);
        }
      } catch {}
    }).catch(() => {});
    invoke<CatalogItem[]>("get_all_items").then(setCatalog).catch(() => {});
    invoke<Record<string, number>>("get_current_quantities").then(setQuantities).catch(() => {});
  }, []);

  useEffect(() => {
    const unlisten = listen<InventoryUpdate>("inventory-update", e => {
      setQuantities(e.payload.quantities);
    });
    return () => { unlisten.then(fn => fn()); };
  }, []);

  useEffect(() => {
    const unlisten = listen("settings-updated", () => {
      invoke<string>("load_settings").then(json => {
        if (!json) return;
        try {
          const s = JSON.parse(json);
          if (Array.isArray(s.tracked)) setTracked(s.tracked);
          if (Array.isArray(s.favorites)) setFavorites(s.favorites);
          if (Array.isArray(s.timerFavorites)) setTimerFavorites(s.timerFavorites);
          if (Array.isArray(s.fissureWatches)) setFissureWatches(s.fissureWatches);
          if (Array.isArray(s.modularSectionOrder)) setSectionOrder(s.modularSectionOrder);
        } catch {}
      }).catch(() => {});
    });
    return () => { unlisten.then(fn => fn()); };
  }, []);

  const saveModularSettings = useCallback((patch: object) => {
    invoke("save_settings", { json: JSON.stringify(patch) }).catch((e) => {
      console.error("save_settings failed:", e);
    });
  }, []);

  useEffect(() => {
    const win = getCurrentWindow();
    let t: ReturnType<typeof setTimeout> | null = null;
    const save = () => {
      if (t) clearTimeout(t);
      t = setTimeout(() => {
        Promise.all([win.outerPosition(), win.outerSize()]).then(([pos, size]) => {
          saveModularSettings({
            modularWinX: pos.x, modularWinY: pos.y,
            modularWinWidth: size.width, modularWinHeight: size.height,
          });
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
  }, [saveModularSettings]);

  const handleTrackedChange = (next: string[]) => {
    setTracked(next);
    saveModularSettings({ tracked: next });
  };
  const handleUntrack = (id: string) => {
    const next = tracked.filter(i => i !== id);
    setTracked(next);
    saveModularSettings({ tracked: next });
  };
  const handleFavoritesChange = (next: string[]) => {
    setFavorites(next);
    saveModularSettings({ favorites: next });
  };
  const handleUnfavorite = (id: string) => {
    const next = favorites.filter(i => i !== id);
    setFavorites(next);
    saveModularSettings({ favorites: next });
  };
  const handleSectionOrderChange = (next: string[]) => {
    setSectionOrder(next);
    saveModularSettings({ modularSectionOrder: next });
  };

  return (
    <div style={{ display: "flex", height: "100vh", background: "var(--surface)", overflow: "hidden" }}>
      <ModularWindow
        tracked={tracked}
        onTrackedChange={handleTrackedChange}
        onUntrack={handleUntrack}
        favorites={favorites}
        onFavoritesChange={handleFavoritesChange}
        onUnfavorite={handleUnfavorite}
        timerFavorites={timerFavorites}
        onTimerFavoritesChange={setTimerFavorites}
        onTimerUnfavorite={id => setTimerFavorites(prev => prev.filter(x => x !== id))}
        fissureWatches={fissureWatches}
        inventory={inventory}
        catalog={catalog}
        sectionOrder={sectionOrder}
        onSectionOrderChange={handleSectionOrderChange}
      />
    </div>
  );
}


// ─── Memoized inventory card components ──────────────────────────────────────

interface InvModCardProps {
  unique_name: string;
  name: string;
  category: string;
  image_name?: string | null;
  ranks: { rank: number; count: number }[];
  total: number;
}
const InvModCard = memo(function InvModCard({ unique_name, name, category, image_name, ranks, total }: InvModCardProps) {
  return (
    <div key={unique_name} className="inv-card inv-card-mod">
      <div className="inv-card-img-wrap">
        <ItemImg imageName={image_name ?? undefined} category={category} size={40} />
      </div>
      <div className="inv-card-name">{name}</div>
      <div className="inv-card-cat">{category}</div>
      <div className="mod-rank-table">
        {ranks.map(r => (
          <div key={r.rank} className={`mod-rank-row${r.count === 0 ? " mod-rank-zero" : ""}`}>
            <span className="mod-rank-label">R{r.rank}</span>
            <span className="mod-rank-count">{r.count}</span>
          </div>
        ))}
      </div>
      <div className="inv-card-qty mod-total">{fmt(total)}</div>
    </div>
  );
}, (prev, next) =>
  prev.unique_name === next.unique_name &&
  prev.name === next.name &&
  prev.total === next.total &&
  prev.image_name === next.image_name &&
  prev.ranks.length === next.ranks.length &&
  prev.ranks.every((r, i) => r.rank === next.ranks[i].rank && r.count === next.ranks[i].count)
);

interface InvCardProps {
  unique_name: string;
  name: string;
  category: string;
  image_name?: string | null;
  qty: number;
  isFavorite: boolean;
  changedAt: number | undefined;
  recentDelta: number | null;
  craftJobName: string | null;
  masteryRank: number | undefined;
  onToggleFavorite: (id: string) => void;
}
const InvCard = memo(function InvCard({
  unique_name, name, category, image_name, qty,
  isFavorite, changedAt, recentDelta, craftJobName, masteryRank, onToggleFavorite,
}: InvCardProps) {
  const nowSec = Date.now() / 1000;
  const secAgo = changedAt != null ? nowSec - changedAt : null;
  const isRecent = secAgo !== null && secAgo < 300;
  const isZero = qty === 0 && !craftJobName;
  const isMastered = masteryRank != null && masteryRank >= 30;
  const showRank = masteryRank != null && masteryRank > 0;
  return (
    <div
      className={`inv-card ${isZero ? "inv-card-zero" : ""} ${isRecent ? (recentDelta != null && recentDelta > 0 ? "inv-card-gained" : "inv-card-lost") : ""}`}>
      <button
        className={`inv-fav-star ${isFavorite ? "active" : ""}`}
        title={isFavorite ? "Remove from Modular Window" : "Add to Modular Window"}
        onClick={e => { e.stopPropagation(); onToggleFavorite(unique_name); }}
      >{isFavorite ? "★" : "☆"}</button>
      <div className="inv-mastery-row">
        {isMastered
          ? <span className="inv-mastery-star" title="Mastered">★</span>
          : showRank
            ? <span className="inv-mastery-rank" title={`Rank ${masteryRank}`}>R{masteryRank}</span>
            : null}
      </div>
      <div className="inv-card-img-wrap">
        <ItemImg imageName={image_name ?? undefined} category={category} size={48} />
        {craftJobName && <span className="inv-foundry-icon" title={`Building — ${craftJobName}`}>⚒</span>}
      </div>
      <div className="inv-card-name">
        {name}
        {isRecent && secAgo !== null && (
          <span className="item-updated">{Math.floor(secAgo / 60) === 0 ? "· now" : `· ${Math.floor(secAgo / 60)}m`}</span>
        )}
      </div>
      <div className="inv-card-cat">{category}</div>
      <div className={`inv-card-qty ${isZero ? "inv-card-qty-zero" : ""}`}>
        {fmt(qty)}
        {isRecent && recentDelta != null && (
          <span className={`item-delta ${deltaClass(recentDelta)}`}>{deltaText(recentDelta)}</span>
        )}
      </div>
    </div>
  );
}, (prev, next) => {
  if (
    prev.unique_name !== next.unique_name ||
    prev.qty !== next.qty ||
    prev.isFavorite !== next.isFavorite ||
    prev.image_name !== next.image_name ||
    prev.masteryRank !== next.masteryRank ||
    prev.craftJobName !== next.craftJobName ||
    prev.recentDelta !== next.recentDelta ||
    prev.changedAt !== next.changedAt
  ) return false;
  // Recently-changed items must re-render so elapsed time stays fresh
  const nowSec = Date.now() / 1000;
  if (prev.changedAt != null && nowSec - prev.changedAt < 300) return false;
  return true;
});

// Feature 3 — api.warframe.com/api/inventory.php
// DE confirmed third-party tools are used "at your own risk" but could not clarify
// whether accessing this undocumented endpoint specifically is permitted.
// Set to false to re-enable once clearer guidance is received.
const COMPANION_API_SUSPENDED = true;

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
    <div style={{
      width: '100vw', height: '100vh', boxSizing: 'border-box',
      background: '#00cc55',
      border: '4px solid #00ff88',
      display: 'flex', flexDirection: 'column', alignItems: 'center', justifyContent: 'center',
      gap: 12, fontFamily: 'sans-serif', color: '#fff',
    }}>
      <div style={{ fontSize: 22, fontWeight: 700, textShadow: '0 2px 6px #000' }}>
        FrameForge Overlay Test
      </div>
      <div style={{ fontSize: 13, opacity: 0.85 }}>If you see green: window + React are working</div>
      <button
        onClick={() => getCurrentWindow().close().catch(() => {})}
        style={{ marginTop: 8, padding: '8px 24px', cursor: 'pointer', fontSize: 14,
          background: '#00ff88', color: '#000', border: 'none', borderRadius: 6, fontWeight: 700 }}
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
  if (IS_OVERLAY) return <Overlay />;
  if (IS_RIVEN_OVERLAY) return <RivenOverlayWindow />;
  // If we're the pop-out modular window, render the standalone modular UI
  if (IS_MODULAR) return <ModularWindowPage />;

  const [activeModule, setActiveModule] = useState<Module>("inventory");

  const [catalog, setCatalog] = useState<CatalogItem[]>([]);
  const [quantities, setQuantities] = useState<Record<string, number>>({});
  const [apiQuantities, setApiQuantities] = useState<Record<string, number>>({});
  const [apiModCopies, setApiModCopies] = useState<ModCopy[]>([]);
  const [scannerMods, setScannerMods] = useState<Record<string, { total: number; by_rank: Record<string, number> }>>({});
  const [crafting, setCrafting] = useState<CraftingJob[]>([]);
  const [masteryRank, setMasteryRank] = useState<number | null>(null);
  const [masteryData, setMasteryData] = useState<Record<string, number>>({});
  const [playerName, setPlayerName] = useState<string | null>(null);
  const [wfConnected, setWfConnected] = useState(false);
  const [memoryProbing, setMemoryProbing] = useState(false);
  const [rawScanning, setRawScanning] = useState(false);
  const [diagCapturing, setDiagCapturing] = useState(false);
  const [diagPath, setDiagPath] = useState<string | null>(null);
  const [autoDiagEnabled, setAutoDiagEnabled] = useState(false);
  const [diagFolderSize, setDiagFolderSize] = useState<number>(0);
  const [companionApiEnabled] = useState(false);
  const [memoryScannerEnabled, setMemoryScannerEnabled] = useState(false);
const [blobLogEnabled, setBlobLogEnabled] = useState(false);
  const [apiLogEnabled,  setApiLogEnabled]  = useState(false);
  const [wfmLoggedIn, setWfmLoggedIn] = useState(false);
  const [wfmInvisibleOnStart,   setWfmInvisibleOnStart]   = useState(false);
  const [wfmInvisibleOnClose,   setWfmInvisibleOnClose]   = useState(false);
  const [wfmAutoInvisible,      setWfmAutoInvisible]      = useState(false);
  const [wfmAutoInvisibleMins,  setWfmAutoInvisibleMins]  = useState(30);
  const [overlayStatus, setOverlayStatus] = useState("");
  const [subsummedWarframes, setSubsummedWarframes] = useState<Set<string>>(new Set());
  const [archonShards, setArchonShards] = useState<Record<string, {type: string; tauforged: boolean; color: string; boost?: string}[]>>({});
  const [lastApiRefresh, setLastApiRefresh] = useState<number | null>(null);
  const wfConnectedRef = useRef(false);
  const inventoryRestoredRef = useRef(false);
  const wfmInvisibleOnStartRef  = useRef(false);
  const wfmInvisibleOnCloseRef  = useRef(false);
  const wfmLoggedInRef          = useRef(false);
  const catalogRef = useRef<CatalogItem[]>([]);
  const prevApiQtyRef = useRef<Record<string, number>>({});
  const manualCredsRef = useRef<{ accountId: string; nonce: string } | null>(null);
  const [changeLog, setChangeLog] = useState<QuantityChange[]>([]);
  const [category, setCategory] = useState("all");
  const [search, setSearch] = useState("");
  const [filterOwned,    setFilterOwned]    = useState(false);
  const [filterRecent,   setFilterRecent]   = useState(false);
  const [filterPrime,    setFilterPrime]    = useState(false);
  const [filterVaulted,  setFilterVaulted]  = useState(false);
  const [filterUnvaulted,setFilterUnvaulted]= useState(false);
  const [sortMode, setSortMode] = useState<"qty-desc" | "qty-asc" | "name-asc" | "name-desc" | "recent">("qty-desc");
  const [filterRank, setFilterRank] = useState<number | "unranked" | null>(null);

  // ── Per-tab persisted filter state ────────────────────────────────────────
  const [foundryFilters, setFoundryFilters] = useState({
    search: "", activeCat: "Warframes",
    filterPrime: false, filterVaulted: false, filterUnvaulted: false,
    filterMastered: false, filterUnmastered: false,
    filterOwned: false, filterUnowned: false, filterReady: false,
  });
  const [marketFilters, setMarketFilters] = useState(MARKET_FILTERS_DEFAULT);
  const [relicFilters, setRelicFilters] = useState(RELIC_FILTERS_DEFAULT);
  const [syndicateFilters, setSyndicateFilters] = useState({
    activeGroup: "main" as "main" | "openworld" | "other" | "lab",
    activeTab: "Steel Meridian", missingOnly: false, search: "",
  });
  const [statsTab, setStatsTab] = useState<"trade" | "item">("trade");
  const [reportsDateRange, setReportsDateRange] = useState<number | "all">(30);
  const [lastChanged, setLastChanged] = useState<Record<string, number>>({});
  const [logPanelH, setLogPanelH] = useState(180);
  const [monitoring, setMonitoring] = useState(false);
  const [warframeRunning, setWarframeRunning] = useState(false);
  const [itemCount, setItemCount] = useState(0);
  const [recipeCount, setRecipeCount] = useState(0);
  const [fetching, setFetching] = useState(false);
  const [fetchMsg, setFetchMsg] = useState("");
  const [showSettings, setShowSettings] = useState(false);
  const [settingsTab, setSettingsTab] = useState<'general' | 'market' | 'accessibility' | 'data' | 'debugging'>('general');
  const [overlayEnabled, setOverlayEnabled] = useState<boolean>(
    () => localStorage.getItem("ff-overlay-enabled") !== "false"
  );
  const [overlayPriority, setOverlayPriority] = useState<string>(
    () => localStorage.getItem("ff-overlay-priority") ?? "completion"
  );
  const [clearMsg, setClearMsg] = useState("");
  const [appVersion, setAppVersion] = useState("");
  const [updateAvailable, setUpdateAvailable] = useState<string | null>(null);
  const [blobLogSize,    setBlobLogSize]    = useState(0);
  const [apiLogSize,     setApiLogSize]     = useState(0);
  const [rawScanSize,    setRawScanSize]    = useState(0);
  const [probeSize,      setProbeSize]      = useState(0);
  // "scanning" while blob capture is running, "done" briefly after it finishes
  const [blobStage, setBlobStage] = useState<"scanning" | "done" | null>(null);
  const blobDoneTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const [textScale, setTextScale] = useState(() => {
    const s = parseFloat(localStorage.getItem("ff-text-scale") ?? "1");
    document.documentElement.style.setProperty("--ff-scale", s.toString());
    return s;
  });
  const [colorblindMode, setColorblindMode] = useState(() =>
    localStorage.getItem("ff-colorblind") === "true"
  );
  const [itemsRefreshKey, setItemsRefreshKey] = useState(0);
  const [imgCacheDir, setImgCacheDir] = useState("");

  // ── Modular Window state ───────────────────────────────────────────────────
  const [tracked, setTracked] = useState<string[]>([]);
  const [favorites, setFavorites] = useState<string[]>([]);
  const [timerFavorites, setTimerFavorites] = useState<string[]>([]);
  const [fissureWatches, setFissureWatches] = useState<FissureWatch[]>([]);
  const [modularWidth, setModularWidth] = useState(240);
  const [modularSectionOrder, setModularSectionOrder] = useState<string[]>(["tracking", "favorites", "timers", "fissures"]);
  const [modularPopout, setModularPopout] = useState(false);
  const modularWinRef = useRef<WebviewWindow | null>(null);
  const modularWinGeomRef = useRef<{ x?: number; y?: number; w?: number; h?: number }>({});

  const handleWfmLoginChange = useCallback((loggedIn: boolean) => {
    setWfmLoggedIn(loggedIn);
    wfmLoggedInRef.current = loggedIn;
  }, []);

  // ── Settings helpers ──────────────────────────────────────────────────────
  // Refs so we can read the latest state in the save callback without stale closures
  const settingsLoadedRef = useRef(false);
  const settingsRef = useRef({
    overlayEnabled: true, overlayPriority: "completion", textScale: 1, colorblindMode: false, companionApiEnabled: false, memoryScannerEnabled: false, blobLogEnabled: false, apiLogEnabled: false, autoDiagEnabled: false,
    tracked: [] as string[], favorites: [] as string[], timerFavorites: [] as string[], fissureWatches: [] as FissureWatch[], modularWidth: 240,
    modularSectionOrder: ["tracking", "favorites", "timers"] as string[], modularPopout: false,
    wfmInvisibleOnStart: false, wfmInvisibleOnClose: false, wfmAutoInvisible: false, wfmAutoInvisibleMins: 30,
  });
  settingsRef.current = { overlayEnabled, overlayPriority, textScale, colorblindMode, companionApiEnabled, memoryScannerEnabled, blobLogEnabled, apiLogEnabled, autoDiagEnabled, tracked, favorites, timerFavorites, fissureWatches, modularWidth, modularSectionOrder, modularPopout, wfmInvisibleOnStart, wfmInvisibleOnClose, wfmAutoInvisible, wfmAutoInvisibleMins };

  const saveAllSettings = useCallback(() => {
    invoke("save_settings", { json: JSON.stringify(settingsRef.current) }).catch((e) => {
      console.error("save_settings failed:", e);
    });
  }, []); // eslint-disable-line

  // ── Memory scanner toggle ─────────────────────────────────────────────────
  useEffect(() => {
    if (memoryScannerEnabled) {
      invoke("start_monitor").then(() => setMonitoring(true)).catch(() => {});
    } else {
      invoke("stop_monitor").then(() => setMonitoring(false)).catch(() => {});
    }
  }, [memoryScannerEnabled]); // eslint-disable-line

  // ── Blob log toggle ───────────────────────────────────────────────────────
  useEffect(() => {
    invoke("set_blob_log", { enabled: blobLogEnabled }).catch(() => {});
  }, [blobLogEnabled]); // eslint-disable-line

  // ── API log toggle ────────────────────────────────────────────────────────
  useEffect(() => {
    invoke("set_api_log", { enabled: apiLogEnabled }).catch(() => {});
  }, [apiLogEnabled]); // eslint-disable-line

  // ── Debug data sizes — reload when the Debugging settings tab opens ─────────
  const reloadDebugSizes = useCallback(() => {
    invoke<number>("get_debug_data_size", { which: "blobs"    }).then(setBlobLogSize).catch(() => {});
    invoke<number>("get_debug_data_size", { which: "api_logs" }).then(setApiLogSize).catch(() => {});
    invoke<number>("get_debug_data_size", { which: "raw_scan" }).then(setRawScanSize).catch(() => {});
    invoke<number>("get_debug_data_size", { which: "probe"    }).then(setProbeSize).catch(() => {});
    invoke<number>("get_diag_folder_size").then(setDiagFolderSize).catch(() => {});
  }, []);

  useEffect(() => {
    if (showSettings && settingsTab === "debugging") reloadDebugSizes();
  }, [showSettings, settingsTab]); // eslint-disable-line

  // ── Log watcher — always start regardless of memory scanner toggle ─────────
  // EE.log is plain file I/O (not memory reading) — handles riven detection,
  // trade completion, and WFM whisper detection unconditionally.
  useEffect(() => {
    invoke("start_log_watcher").catch(() => {});
  }, []); // eslint-disable-line

  // ── WFM auto-login at app start ───────────────────────────────────────────
  // Restores the session into Rust's AppState so the Trading tab is instantly
  // ready when the user opens it — no need to visit the tab first.
  // Also pre-warms the top-items cache in the background so the Statistics tab
  // loads instantly rather than running ~2 minutes of API calls on first open.
  useEffect(() => {
    (async () => {
      const creds = await invoke<[string, string] | null>("wfm_load_credentials").catch(() => null);
      if (creds) {
        const session = await invoke<[string, string] | null>("wfm_set_jwt", { jwt: creds[1] }).catch(() => null);
        if (session) {
          setWfmLoggedIn(true);
          wfmLoggedInRef.current = true;
          if (wfmInvisibleOnStartRef.current) {
            invoke("wfm_set_status", { status: "invisible" }).catch(() => {});
          }
        }
      }
    })();
    // Fire-and-forget: populates WFM_TOP_CACHE so the Statistics tab is instant
    invoke("get_wfm_top_items").catch(() => {});
    invoke<string>("get_img_cache_dir").then(setImgCacheDir).catch(() => {});
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
          invoke("wfm_set_status", { status: "invisible" }).catch(() => {}),
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
      invoke("wfm_set_status", { status: "invisible" }).catch(() => {});
    }, wfmAutoInvisibleMins * 60 * 1000);
    return () => clearTimeout(id);
  }, [wfmAutoInvisible, wfmAutoInvisibleMins, wfmLoggedIn]);

  // ── Bootstrap ──────────────────────────────────────────────────────────────

  useEffect(() => {
    // Restore all inventory data from the single Rust-side cache file
    invoke<{ apiQuantities: Record<string, number>; apiModCopies: ModCopy[]; consumedSuits: string[] }>("get_saved_inventory")
      .then(data => {
        if (Object.keys(data.apiQuantities).length > 0) setApiQuantities(data.apiQuantities);
        if (data.apiModCopies.length > 0) setApiModCopies(data.apiModCopies);
        if (data.consumedSuits.length > 0) setSubsummedWarframes(new Set(data.consumedSuits));
      })
      .catch(() => {})
      .finally(() => { inventoryRestoredRef.current = true; });

    // Load user settings from file — survives reinstalls unlike localStorage
    invoke<string>("load_settings").then(json => {
      if (!json) return;
      try {
        const s = JSON.parse(json);
        // companionApiEnabled intentionally not loaded — feature suspended pending DE clarification
        if (typeof s.memoryScannerEnabled === "boolean") setMemoryScannerEnabled(s.memoryScannerEnabled);
        if (typeof s.blobLogEnabled === "boolean") setBlobLogEnabled(s.blobLogEnabled);
        if (typeof s.apiLogEnabled  === "boolean") setApiLogEnabled(s.apiLogEnabled);
if (typeof s.autoDiagEnabled === "boolean") {
          setAutoDiagEnabled(s.autoDiagEnabled);
          localStorage.setItem("ff-auto-diag", String(s.autoDiagEnabled));
        }
        if (typeof s.overlayEnabled === "boolean") {
          setOverlayEnabled(s.overlayEnabled);
          localStorage.setItem("ff-overlay-enabled", String(s.overlayEnabled));
        }
        if (typeof s.overlayPriority === "string") {
          setOverlayPriority(s.overlayPriority);
          localStorage.setItem("ff-overlay-priority", s.overlayPriority);
        }
        if (typeof s.textScale === "number") {
          setTextScale(s.textScale);
          document.documentElement.style.setProperty("--ff-scale", s.textScale.toString());
          localStorage.setItem("ff-text-scale", s.textScale.toString());
        }
        if (typeof s.colorblindMode === "boolean") {
          setColorblindMode(s.colorblindMode);
          localStorage.setItem("ff-colorblind", String(s.colorblindMode));
        }
        if (Array.isArray(s.tracked)) setTracked(s.tracked);
        if (Array.isArray(s.favorites)) setFavorites(s.favorites);
        if (Array.isArray(s.timerFavorites)) setTimerFavorites(s.timerFavorites);
        if (Array.isArray(s.fissureWatches)) setFissureWatches(s.fissureWatches);
        if (typeof s.modularWidth === "number") setModularWidth(s.modularWidth);
        if (Array.isArray(s.modularSectionOrder)) {
          const order: string[] = s.modularSectionOrder;
          if (!order.includes("timers"))   order.push("timers");
          if (!order.includes("fissures")) order.push("fissures");
          setModularSectionOrder(order);
        }
        if (typeof s.modularPopout === "boolean") setModularPopout(s.modularPopout);
        if (typeof s.modularWinX === "number") modularWinGeomRef.current.x = s.modularWinX;
        if (typeof s.modularWinY === "number") modularWinGeomRef.current.y = s.modularWinY;
        if (typeof s.modularWinWidth === "number") modularWinGeomRef.current.w = s.modularWinWidth;
        if (typeof s.modularWinHeight === "number") modularWinGeomRef.current.h = s.modularWinHeight;
        if (typeof s.wfmInvisibleOnStart === "boolean") { setWfmInvisibleOnStart(s.wfmInvisibleOnStart); wfmInvisibleOnStartRef.current = s.wfmInvisibleOnStart; }
        if (typeof s.wfmInvisibleOnClose === "boolean") { setWfmInvisibleOnClose(s.wfmInvisibleOnClose); wfmInvisibleOnCloseRef.current = s.wfmInvisibleOnClose; }
        if (typeof s.wfmAutoInvisible    === "boolean") setWfmAutoInvisible(s.wfmAutoInvisible);
        if (typeof s.wfmAutoInvisibleMins === "number") setWfmAutoInvisibleMins(s.wfmAutoInvisibleMins);
        settingsLoadedRef.current = true;
      } catch {}
    }).catch(() => {});

    invoke<CatalogItem[]>("get_all_items").then(items => { setCatalog(items); catalogRef.current = items; });
    invoke<Record<string, number>>("get_current_quantities").then(setQuantities);
    invoke<number>("get_diag_folder_size").then(setDiagFolderSize).catch(() => {});
    invoke<QuantityChange[]>("get_change_log", { limit: 200 }).then(log => {
      setChangeLog(log);
      const lc: Record<string, number> = {};
      for (const c of log) lc[c.unique_name] = Math.max(lc[c.unique_name] ?? 0, c.timestamp);
      setLastChanged(lc);
    });
    invoke<{ count: number; recipe_count: number }>("get_item_list_status").then(s => {
      setItemCount(s.count);
      setRecipeCount(s.recipe_count);
    });

    // Check for updates from GitHub on launch and every hour
    const semverGt = (a: string, b: string) => {
      const [ma, mi, pa] = a.split(".").map(Number);
      const [mb, mii, pb] = b.split(".").map(Number);
      if (ma !== mb) return ma > mb;
      if (mi !== mii) return mi > mii;
      return pa > pb;
    };
    const checkForUpdate = (v: string) =>
      fetch("https://api.github.com/repos/WyrmStudios/FrameForge/releases/latest")
        .then(r => r.json())
        .then(d => { const latest = (d.tag_name ?? "").replace(/^v/, ""); if (latest && semverGt(latest, v)) setUpdateAvailable(latest); })
        .catch(() => {});

    getVersion().then(v => {
      setAppVersion(v);
      checkForUpdate(v);
      const interval = setInterval(() => checkForUpdate(v), 60 * 60 * 1000);
      return () => clearInterval(interval);
    }).catch(() => {});

    // Auto-start monitor on launch — only if memory scanner is explicitly enabled
    invoke<boolean>("get_monitor_status").then(active => {
      if (!active) {
        // memoryScannerEnabled not yet loaded from settings at this point;
        // the effect below handles delayed auto-start after settings load.
      } else {
        setMonitoring(true);
      }
    });
  }, []);

  // Refresh diagnostics folder size every minute so the Clear button stays current.
  useEffect(() => {
    const id = setInterval(() => {
      invoke<number>("get_diag_folder_size").then(setDiagFolderSize).catch(() => {});
    }, 60_000);
    return () => clearInterval(id);
  }, []);

  // ── Inventory update events ────────────────────────────────────────────────

  useEffect(() => {
    const unlisten = listen<InventoryUpdate>("inventory-update", (e) => {
      const p = e.payload;
      // Only replace quantities if the content actually changed.
      // The monitor loop re-emits cached state periodically; without this guard
      // every emit triggers a full 17k-item useMemo rebuild cascade.
      setQuantities(prev => {
        const next = p.quantities;
        const prevKeys = Object.keys(prev);
        const nextKeys = Object.keys(next);
        if (prevKeys.length !== nextKeys.length) return next;
        for (const k of nextKeys) { if (next[k] !== prev[k]) return next; }
        return prev;
      });
      if (p.crafting) setCrafting(p.crafting);
      if (p.mastery_rank != null) setMasteryRank(p.mastery_rank);
      if (p.player_name) setPlayerName(p.player_name);
      if (p.mastery_data && Object.keys(p.mastery_data).length > 0)
        setMasteryData(prev => ({ ...prev, ...p.mastery_data }));
      // When Warframe restarts (was running → stopped → running again),
      // clear manual credentials so fresh ones are scanned from the new session
      if (!p.warframe_running && wfConnectedRef.current) {
        manualCredsRef.current = null;
      }
      setWarframeRunning(p.warframe_running);
      if (p.consumed_suits && p.consumed_suits.length > 0) {
        setSubsummedWarframes(prev => {
          const next = new Set(prev);
          for (const s of p.consumed_suits!) next.add(s);
          return next;
        });
      }
      if (p.mods && Object.keys(p.mods).length > 0) {
        setScannerMods(p.mods);
      }
      if (p.socketed_shards) {
        // In-memory color values use ACC_RED/BLUE/YELLOW/GREEN/PURPLE.
        // Tauforged variants include "TAU" in the string (e.g. ACC_TAU_RED).
        const SHARD_COLORS: { prefix: string; type: string; colorHex: string; tauHex: string }[] = [
          { prefix: "ACC_RED",    type: "Crimson",  colorHex: "#e04040", tauHex: "#ff7070" },
          { prefix: "ACC_BLUE",   type: "Azure",    colorHex: "#4488ff", tauHex: "#77aaff" },
          { prefix: "ACC_GREEN",  type: "Viridian", colorHex: "#44cc66", tauHex: "#66ff99" },
          { prefix: "ACC_YELLOW", type: "Amber",    colorHex: "#ffaa00", tauHex: "#ffcc44" },
          { prefix: "ACC_PURPLE", type: "Violet",   colorHex: "#9944ff", tauHex: "#bb77ff" },
        ];
        const INT_TO_ACC = ["ACC_RED","ACC_BLUE","ACC_GREEN","ACC_YELLOW","ACC_PURPLE"];
        const parsed: Record<string, { type: string; tauforged: boolean; color: string; boost?: string }[]> = {};
        for (const [wfPath, shards] of Object.entries(p.socketed_shards)) {
          parsed[wfPath] = shards.map(s => {
            let raw = s.color.toUpperCase();
            // If it's a pure integer, normalise to ACC_ string
            if (/^\d+$/.test(raw)) {
              const n = parseInt(raw);
              raw = INT_TO_ACC[n % 5] ?? raw;  // %5 so tau-forged (5-9) maps to base color
            }
            // In memory: tauforged shards use the suffix "_MYTHIC" (e.g. "ACC_RED_MYTHIC").
            const tauforged = raw.includes("MYTHIC") || raw.includes("TAU") || parseInt(s.color) >= 5;
            const entry = SHARD_COLORS.find(e => raw.startsWith(e.prefix));
            const colorInfo = entry ?? { type: "Unknown", colorHex: "#b0b0b0", tauHex: "#d0d0d0" };
            const seg = s.upgrade_type.split("/").pop() ?? "";
            const boostRaw = seg.replace(/^ArchonCrystalUpgrade(?:Warframe|Companion)?/, "");
            const boost = boostRaw.replace(/([A-Z])/g, " $1").trim() || undefined;
            // Tauforged shards use a brighter colour so they stand out from normal shards.
            const color = tauforged ? colorInfo.tauHex : colorInfo.colorHex;
            return { type: colorInfo.type, tauforged, color, boost };
          });
        }
        if (p.is_full_pass) {
          // Full pass = authoritative complete state; replace so removed shards don't linger.
          setArchonShards(parsed);
        } else if (Object.keys(parsed).length > 0) {
          setArchonShards(prev => ({ ...prev, ...parsed }));
        }
      }
      if (p.changes.length > 0) {
        setChangeLog(prev => [...p.changes, ...prev].slice(0, 200));
        setLastChanged(prev => {
          const next = { ...prev };
          for (const c of p.changes) next[c.unique_name] = c.timestamp;
          return next;
        });
      }
    });
    return () => { unlisten.then(fn => fn()); };
  }, []);

  // ── Blob processing status ────────────────────────────────────────────────
  useEffect(() => {
    const unlisten = listen<{ stage: string; detail: string }>("blob-status", e => {
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

  // ── Player name (immediate, from EE.log "Logged in NAME") ───────────────
  useEffect(() => {
    const unlisten = listen<string>("player-name", e => {
      setPlayerName(e.payload);
    });
    return () => { unlisten.then(fn => fn()); };
  }, []);

  // ── Sync modular state from pop-out window ────────────────────────────────
  // When the pop-out saves (unstar, reorder), Rust emits settings-updated.
  // Compare before setting to avoid a save → emit → re-read → save loop.
  useEffect(() => {
    const unlisten = listen("settings-updated", () => {
      invoke<string>("load_settings").then(json => {
        if (!json) return;
        try {
          const s = JSON.parse(json);
          const cur = settingsRef.current;
          if (Array.isArray(s.favorites) && JSON.stringify(s.favorites) !== JSON.stringify(cur.favorites))
            setFavorites(s.favorites);
          if (Array.isArray(s.tracked) && JSON.stringify(s.tracked) !== JSON.stringify(cur.tracked))
            setTracked(s.tracked);
          if (Array.isArray(s.modularSectionOrder) && JSON.stringify(s.modularSectionOrder) !== JSON.stringify(cur.modularSectionOrder))
            setModularSectionOrder(s.modularSectionOrder);
        } catch {}
      }).catch(() => {});
    });
    return () => { unlisten.then(fn => fn()); };
  }, []); // eslint-disable-line

  // ── Persist main window geometry on move/resize ───────────────────────────
  useEffect(() => {
    const win = getCurrentWindow();
    let t: ReturnType<typeof setTimeout> | null = null;
    const save = () => {
      if (t) clearTimeout(t);
      t = setTimeout(() => {
        Promise.all([win.outerPosition(), win.outerSize()]).then(([pos, size]) => {
          invoke("save_settings", { json: JSON.stringify({
            windowX: pos.x, windowY: pos.y,
            windowWidth: size.width, windowHeight: size.height,
          }) }).catch(() => {});
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

  const toggleTracked = useCallback((id: string) => {
    setTracked(prev => prev.includes(id) ? prev.filter(i => i !== id) : [...prev, id]);
  }, []);

  const toggleFavorite = useCallback((id: string) => {
    setFavorites(prev => prev.includes(id) ? prev.filter(i => i !== id) : [...prev, id]);
  }, []);

  // ── Fetch item list ────────────────────────────────────────────────────────

  const handleFetch = async () => {
    setFetching(true);
    setFetchMsg("Fetching…");
    // Stop monitor during refresh so it restarts with the new item list
    const wasMonitoring = monitoring;
    if (wasMonitoring) {
      await invoke("stop_monitor");
      setMonitoring(false);
    }
    try {
      const count = await invoke<number>("fetch_item_list");
      setItemCount(count);
      const items = await invoke<CatalogItem[]>("get_all_items");
      setCatalog(items);
      catalogRef.current = items;
      const status = await invoke<{ count: number; recipe_count: number }>("get_item_list_status");
      setRecipeCount(status.recipe_count);
      setFetchMsg(`Loaded ${count.toLocaleString()} items, ${status.recipe_count.toLocaleString()} recipes`);
      setItemsRefreshKey(k => k + 1);
      invoke("prewarm_image_cache").catch(() => {});
    } catch (e) {
      setFetchMsg(`Error: ${e}`);
    } finally {
      setFetching(false);
      if (wasMonitoring) {
        await invoke("start_monitor");
        setMonitoring(true);
      }
    }
  };

  // Auto-refresh item database on every app start so the OCR catalog stays current.
  // eslint-disable-next-line react-hooks/exhaustive-deps
  useEffect(() => { handleFetch(); }, []);

  // ── Warframe API: process inventory response ──────────────────────────────

  const applyInventoryData = useCallback((data: any) => {
    const apiQty: Record<string, number> = {};
    const ownedArrayKeys = [
      "Suits", "LongGuns", "Pistols", "Melee",
      "Sentinels", "SentinelWeapons",
      "SpaceSuits", "SpaceGuns", "SpaceMelee",
      "MechSuits", "KubrowPets",
      "CrewShipWeapons", "OperatorAmps", "OperatorSuits",
    ];
    const masteryUpdate: Record<string, number> = {};
    for (const key of ownedArrayKeys) {
      const arr = data[key];
      if (!Array.isArray(arr)) continue;
      for (const item of arr) {
        const t: string = item.ItemType;
        if (!t) continue;
        apiQty[t] = (apiQty[t] ?? 0) + 1;
        // Extract mastery rank from XP field — 30,000 XP per rank, cap at 30
        if (item.XP != null) {
          masteryUpdate[t] = Math.min(30, Math.floor(item.XP / 30000));
        }
      }
    }
    if (Object.keys(masteryUpdate).length > 0)
      setMasteryData(prev => ({ ...prev, ...masteryUpdate }));
    for (const r of (Array.isArray(data.Recipes) ? data.Recipes : [])) {
      const t: string = r.ItemType;
      if (t) {
        apiQty[t] = (apiQty[t] ?? 0) + (r.ItemCount ?? 1);
      }
    }
    // MiscItems: pull everything (relics, resources like Carbides/Cubic Diodes, etc.)
    // so the API is authoritative and mission-pickup counts from the scanner don't override.
    for (const m of (Array.isArray(data.MiscItems) ? data.MiscItems : [])) {
      const t: string = m.ItemType;
      if (t) apiQty[t] = (apiQty[t] ?? 0) + (m.ItemCount ?? 1);
    }
    const rawModMap: Record<string, number> = {};
    for (const r of (Array.isArray(data.RawUpgrades) ? data.RawUpgrades : [])) {
      if (r.ItemType) rawModMap[r.ItemType] = (rawModMap[r.ItemType] ?? 0) + (r.ItemCount ?? 1);
    }
    const rankedModMap: Record<string, Record<number, number>> = {};
    for (const u of (Array.isArray(data.Upgrades) ? data.Upgrades : [])) {
      if (!u.ItemType) continue;
      let rank = 0;
      try { if (u.UpgradeFingerprint) rank = JSON.parse(u.UpgradeFingerprint)?.lvl ?? 0; } catch { rank = 0; }
      if (!rankedModMap[u.ItemType]) rankedModMap[u.ItemType] = {};
      rankedModMap[u.ItemType][rank] = (rankedModMap[u.ItemType][rank] ?? 0) + 1;
    }
    const copies: ModCopy[] = [];
    for (const [t, cnt] of Object.entries(rawModMap)) {
      copies.push({ uniqueName: t, rank: null, count: cnt });
      apiQty[t] = (apiQty[t] ?? 0) + cnt;
    }
    for (const [t, ranks] of Object.entries(rankedModMap)) {
      apiQty[t] = (apiQty[t] ?? 0) + Object.values(ranks).reduce((a, b) => a + b, 0);
      for (const [r, cnt] of Object.entries(ranks)) {
        copies.push({ uniqueName: t, rank: Number(r), count: cnt });
      }
    }
    setApiModCopies(copies);
    setApiQuantities(prev => {
      const changes = Object.entries(apiQty)
        .filter(([k, v]) => (prev[k] ?? 0) !== v)
        .map(([k, v]) => ({ item_name: k, old_qty: prev[k] ?? 0, new_qty: v }));
      if (changes.length > 0)
        invoke("log_api_changes", { changes }).catch(() => {});
      return apiQty;
    });
    if (data.PlayerLevel != null) setMasteryRank(data.PlayerLevel);

    // Extract Archon Shard data from Suits
    // API format: suit.ArchonCrystalUpgrades = [{Color: "ACC_YELLOW", UpgradeType: "/Lotus/.../ArchonCrystalUpgradeWarframeAbilityStrength"}, ...]
    const COLOR_MAP: Record<string, { type: string; color: string; tauColor: string }> = {
      ACC_RED:     { type: "Crimson",  color: "#e04040", tauColor: "#ff7070" },
      ACC_BLUE:    { type: "Azure",    color: "#4488ff", tauColor: "#77aaff" },
      ACC_GREEN:   { type: "Viridian", color: "#44cc66", tauColor: "#66ff99" },
      ACC_YELLOW:  { type: "Amber",    color: "#ffaa00", tauColor: "#ffcc44" },
      ACC_PURPLE:  { type: "Violet",   color: "#9944ff", tauColor: "#bb77ff" },
    };
    const newShards: Record<string, { type: string; tauforged: boolean; color: string; boost: string }[]> = {};
    for (const suit of (Array.isArray(data.Suits) ? data.Suits : [])) {
      const upgrades = suit.ArchonCrystalUpgrades;
      if (!Array.isArray(upgrades) || upgrades.length === 0) continue;
      const uniqueName: string = suit.ItemType ?? "";
      if (!uniqueName) continue;
      newShards[uniqueName] = upgrades.map((u: any) => {
        const colorRaw: string = (u.Color ?? "").toUpperCase();
        const upgradeType: string = u.UpgradeType ?? "";
        const tauforged = colorRaw.includes("MYTHIC") || colorRaw.includes("TAU") || upgradeType.toLowerCase().includes("tau");
        // Strip color prefix for map lookup (e.g. "ACC_YELLOW_TAUFORGED" → "ACC_YELLOW")
        const colorKey = Object.keys(COLOR_MAP).find(k => colorRaw.startsWith(k)) ?? "";
        const info = COLOR_MAP[colorKey] ?? { type: colorRaw || "Unknown", color: "#b0b0b0", tauColor: "#d0d0d0" };
        // Extract boost name from UpgradeType path last segment
        const seg = upgradeType.split("/").pop() ?? "";
        const boost = seg
          .replace(/ArchonCrystalUpgrade(Warframe)?/g, "")
          .replace(/([A-Z])/g, " $1").trim();
        return { type: info.type, tauforged, color: tauforged ? info.tauColor : info.color, boost };
      });
    }
    if (Object.keys(newShards).length > 0) setArchonShards(prev => ({ ...prev, ...newShards }));

    // Extract subsumed warframes from InfestedFoundry (Helminth)
    const consumed = data.InfestedFoundry?.ConsumedSuits;
    if (Array.isArray(consumed)) {
      const s = new Set<string>(
        consumed.map((e: any) => (typeof e === "string" ? e : e?.ItemType ?? "")).filter(Boolean)
      );
      setSubsummedWarframes(s);
    }

    // XPInfo from API → fill mastery data for items no longer owned (memory scanner can't see these)
    if (Array.isArray(data.XPInfo)) {
      const xpMastery: Record<string, number> = {};
      for (const x of data.XPInfo) {
        if (!x.ItemType || x.XP == null) continue;
        // ~30 000 XP per rank; cap at 30
        xpMastery[x.ItemType] = Math.min(30, Math.floor(x.XP / 30_000));
      }
      // Memory-scanner values win (they read actual rank); XP fills the gaps
      setMasteryData(prev => ({ ...xpMastery, ...prev }));
      // Persist so ranks survive restarts without requiring another API call
      invoke("save_mastery_data", { data: xpMastery }).catch(() => {});
    }

    // PendingRecipes from API → update crafting state (authoritative, covers cases memory scanner misses)
    if (Array.isArray(data.PendingRecipes) && data.PendingRecipes.length > 0) {
      const apiJobs: CraftingJob[] = data.PendingRecipes
        .filter((r: any) => r.ItemType)
        .map((r: any) => {
          const completionMs = r.CompletionDate?.$date?.$numberLong
            ? Number(r.CompletionDate.$date.$numberLong)
            : 0;
          const item = catalogRef.current.find(i => i.unique_name === r.ItemType);
          const name = item?.name ?? r.ItemType.split("/").pop() ?? r.ItemType;
          return { unique_name: r.ItemType, item_name: name, completion_ms: completionMs };
        });
      setCrafting(prev => {
        const merged = [...apiJobs];
        for (const job of prev) {
          if (!merged.some(c => c.unique_name === job.unique_name)) merged.push(job);
        }
        return merged;
      });
    }
    const now = Math.floor(Date.now() / 1000);
    setLastApiRefresh(now);

    // Diff against previous API quantities to generate change log entries
    const prev = prevApiQtyRef.current;
    if (Object.keys(prev).length > 0) {
      const allKeys = new Set([...Object.keys(prev), ...Object.keys(apiQty)]);
      const changes: QuantityChange[] = [];
      for (const key of allKeys) {
        const oldQty = prev[key] ?? 0;
        const newQty = apiQty[key] ?? 0;
        if (oldQty !== newQty) {
          const item = catalogRef.current.find(i => i.unique_name === key);
          const name = item?.name ?? key.split("/").pop() ?? key;
          changes.push({ id: 0, unique_name: key, item_name: name, old_qty: oldQty, new_qty: newQty, delta: newQty - oldQty, timestamp: now });
        }
      }
      if (changes.length > 0) {
        setChangeLog(prev => [...changes, ...prev].slice(0, 200));
        setLastChanged(prev => {
          const next = { ...prev };
          for (const c of changes) next[c.unique_name] = c.timestamp;
          return next;
        });
      }
    }
    prevApiQtyRef.current = { ...apiQty };
  }, []); // eslint-disable-line

  // ── Persist API inventory data to inventory_state_cache.json via Rust ────

  useEffect(() => {
    if (!inventoryRestoredRef.current) return;
    if (Object.keys(apiQuantities).length === 0 && apiModCopies.length === 0 && subsummedWarframes.size === 0) return;
    invoke("save_api_inventory", {
      apiQuantities,
      apiModCopies,
      consumedSuits: [...subsummedWarframes],
    }).catch(() => {});
  }, [apiQuantities, apiModCopies, subsummedWarframes]);

  useEffect(() => {
    if (settingsLoadedRef.current) saveAllSettings();
  }, [tracked, favorites, timerFavorites, fissureWatches, modularWidth, memoryScannerEnabled, companionApiEnabled, blobLogEnabled, apiLogEnabled, autoDiagEnabled, modularSectionOrder, modularPopout]); // eslint-disable-line

  // ── Modular pop-out window ─────────────────────────────────────────────────
  useEffect(() => {
    if (modularPopout) {
      if (modularWinRef.current) return;
      const g = modularWinGeomRef.current;

      // Only restore saved position if it lands on a currently connected monitor.
      // Guards against secondary monitor being unplugged since last session.
      const createWin = (usePos: boolean) => new WebviewWindow("modular-popout", {
        url: "index.html#modular",
        title: "FrameForge — Modular Window",
        width: g.w ?? modularWidth,
        height: g.h ?? 700,
        ...(usePos && g.x !== undefined ? { x: g.x } : {}),
        ...(usePos && g.y !== undefined ? { y: g.y } : {}),
        minWidth: 180,
        minHeight: 300,
        resizable: true,
        decorations: true,
        alwaysOnTop: false,
      });

      let win: WebviewWindow;
      if (g.x !== undefined && g.y !== undefined) {
        availableMonitors().then(monitors => {
          const onScreen = monitors.some(m => {
            const mp = m.position; const ms = m.size;
            return g.x! >= mp.x && g.x! < mp.x + ms.width &&
                   g.y! >= mp.y && g.y! < mp.y + ms.height;
          });
          win = createWin(onScreen);
          modularWinRef.current = win;
          win.once("tauri://destroyed", () => { modularWinRef.current = null; setModularPopout(false); });
        }).catch(() => {
          win = createWin(false);
          modularWinRef.current = win;
          win.once("tauri://destroyed", () => { modularWinRef.current = null; setModularPopout(false); });
        });
        return;
      }
      win = createWin(false);
      modularWinRef.current = win;
      win.once("tauri://destroyed", () => {
        modularWinRef.current = null;
        setModularPopout(false);
      });
    } else {
      modularWinRef.current?.close().catch(() => {});
      modularWinRef.current = null;
    }
  }, [modularPopout]); // eslint-disable-line

  // ── Auto-refresh API: 8 s while connecting, 30 s once connected ─────────

  useEffect(() => {
    if (!companionApiEnabled || COMPANION_API_SUSPENDED) {
      setWfConnected(false);
      wfConnectedRef.current = false;
      return;
    }
    let cancelled = false;
    let timeoutId: ReturnType<typeof setTimeout>;

    const doFetch = async () => {
      if (cancelled) return;
      let accountId = "", nonce = "", steamId = "";
      try {
        [accountId, nonce, steamId] = await invoke<[string, string, string]>("scan_warframe_credentials");
        // Cache successful auto-scan so fallback works if scan fails next time
        manualCredsRef.current = { accountId, nonce };
      } catch {
        // Scan failed — fall back to last known credentials (auto-scanned or manual)
        const mc = manualCredsRef.current;
        if (!mc) { schedule(); return; }
        accountId = mc.accountId; nonce = mc.nonce; steamId = "";
      }
      try {
        const data = await invoke<any>("fetch_warframe_inventory", { accountId, nonce, steamId });
        if (!cancelled) {
          applyInventoryData(data);
          setWfConnected(true);
          wfConnectedRef.current = true;
          setWarframeRunning(true);
        }
      } catch {
        // API rejected the credentials — clear cached creds so next scan starts fresh
        if (wfConnectedRef.current) {
          setWfConnected(false);
          wfConnectedRef.current = false;
          manualCredsRef.current = null;
        }
      }
      schedule();
    };

    const schedule = () => {
      if (cancelled) return;
      timeoutId = setTimeout(doFetch, wfConnectedRef.current ? 300_000 : 60_000);
    };

    doFetch();
    return () => { cancelled = true; clearTimeout(timeoutId); };
  }, [applyInventoryData, companionApiEnabled]); // eslint-disable-line

  // ── Riven overlay ─────────────────────────────────────────────────────────
  // Window state lives in module-level _rivenWin (above) — unaffected by StrictMode.
  // No pre-creation: show() silently fails on visible:false windows with this config.
  // Fresh window created on trigger (shows correctly); existing visible window reused for cycling.
  useEffect(() => {
    // Core OCR + overlay display. Called manually via button or (future) auto-detection.
    const runRivenCheck = async () => {
      _rivenLastTriggerMs = Date.now();
      _rivenRollCount++;
      const { emit } = await import("@tauri-apps/api/event");

      let rect: [number, number, number, number] = [0, 0, 0, 800];
      try { rect = await invoke<[number, number, number, number]>("get_warframe_window_rect"); } catch {}
      const [wx, wy, , wh] = rect;
      const result = await ensureRivenWindow(wx, wy, wh);
      let pendingPayload: object | null = null;
      let windowReady = false;

      if (result && !result.fresh) {
        // Existing window — reset overlay state
        await emit("riven-scanning-start", {}).catch(() => {});
        windowReady = true;
      } else if (result?.fresh) {
        // Fresh window — send data once its listener signals ready
        const unsubReady = await listen("riven-window-ready", async () => {
          unsubReady();
          windowReady = true;
          if (pendingPayload) { await emit("riven-analysis-update", pendingPayload).catch(() => {}); pendingPayload = null; }
        });
      }

      try {
        const ocrResult = await invoke<{ weapon: string; positives: string[]; negatives: string[]; rolled_stats: {name:string;value:string;positive:boolean}[]; is_comparison: boolean; original_rolled_stats: {name:string;value:string;positive:boolean}[]; raw: string }>("ocr_riven_screen");
        const analysis = (ocrResult.weapon || ocrResult.positives.length > 0)
          ? await invoke("analyze_riven", { weapon: ocrResult.weapon, positives: ocrResult.positives, negatives: ocrResult.negatives }).catch(() => null)
          : null;
        const payload = { analysis, ocrRaw: ocrResult.raw, weapon: ocrResult.weapon, positives: ocrResult.positives, negatives: ocrResult.negatives, rolledStats: ocrResult.rolled_stats, isComparison: ocrResult.is_comparison, originalStats: ocrResult.original_rolled_stats, rollCount: _rivenRollCount };
        if (windowReady) { await emit("riven-analysis-update", payload).catch(() => {}); }
        else              { pendingPayload = payload; }
      } catch (e) {
        await invoke("ocr_riven_log_error", { error: String(e) }).catch(() => {});
        const payload = { analysis: null, ocrRaw: `OCR ERROR: ${e}`, weapon: "", positives: [], negatives: [], rolledStats: [], isComparison: false, originalStats: [], rollCount: _rivenRollCount };
        if (windowReady) { await emit("riven-analysis-update", payload).catch(() => {}); }
        else              { pendingPayload = payload; }
      }
    };

    // Wire module-level trigger so "Check Riven" button and "Start Comparison" can call it
    _rivenManualTrigger = () => { runRivenCheck().catch(() => {}); };

    // overlay "Start Comparison" button emits this event
    const unsubManual = listen("riven-manual-check", () => runRivenCheck().catch(() => {}));

    // Open trigger: EE.log watcher fires "riven-screen-open" via FindFirstChangeNotificationW
    // (instant file-write notification — no polling delay).
    // 4 s cooldown prevents double-fires from the same log buffer flush.
    const triggerOpen = () => {
      const now = Date.now();
      if (now - _rivenLastTriggerMs < 4000) return;
      runRivenCheck().catch(() => {});
    };
    const unsubAutoDetect = listen("riven-screen-open", () => triggerOpen());

    // Close triggers: EE.log (DiegeticArtifactCards HudVis 0) + manual dismiss.
    const unsubClose   = listen("riven-screen-close",   () => rivenWinHide("screen-close"));
    const unsubHideReq = listen<{ reason?: string }>("riven-overlay-hide", e => rivenWinHide(e.payload?.reason ?? "overlay-hide"));

    return () => {
      unsubManual.then(fn => fn());
      unsubAutoDetect.then(fn => fn());
      unsubClose.then(fn => fn());
      unsubHideReq.then(fn => fn());
      _rivenManualTrigger = null;
    };
  }, []); // eslint-disable-line

  // ── Relic reward overlay ──────────────────────────────────────────────────
  // The overlay window is pre-declared in tauri.conf.json (y=-3000, off-screen).
  // We never create/destroy it — just reposition it on-screen and back off-screen.
  // This avoids a Windows deadlock: WebviewWindowBuilder::build() called dynamically
  // (from tokio threads, run_on_main_thread, or JS new WebviewWindow) all hang because
  // the Win32 event loop cannot process messages while our code is executing.
  useEffect(() => {
    let overlayVisible = false;

    const closeOverlay = async () => {
      overlayVisible = false;
      await invoke("move_overlay_offscreen").catch(() => {});
    };

    const unsubStatus = listen<string>("ff-status", (e) => {
      setOverlayStatus(e.payload);
      setTimeout(() => setOverlayStatus(""), 4000);
    });

    const openOverlay = async (
      wx: number, wy: number, ww: number, wh: number,
      yFrac: number, hFrac: number,
    ): Promise<boolean> => {
      const stripY = wy + Math.round(wh * yFrac);
      const stripH = Math.round(wh * hFrac);
      try {
        await invoke("show_overlay_window", { x: wx, y: stripY, w: ww, h: stripH });
        overlayVisible = true;
        return true;
      } catch { return false; }
    };

    const unsubTrigger = listen<null>("relic-trigger", async () => {
      const enabled = localStorage.getItem("ff-overlay-enabled") !== "false";
      if (!enabled) return;
      try {
        const [wx, wy, ww, wh] = await invoke<[number, number, number, number]>("get_warframe_window_rect");
        invoke("log_relic_fe", { msg: `[APP] relic-trigger: wf(${wx},${wy} ${ww}×${wh})` }).catch(() => {});
        await openOverlay(wx, wy, ww, wh, 0.60, 0.30);
      } catch (e) {
        // get_warframe_window_rect failed (Warframe may be in a different state).
        // Fall back to screen dimensions so the overlay still moves on-screen and
        // WebView2 un-freezes its JS before relic-rewards arrives.
        invoke("log_relic_fe", { msg: `[APP] relic-trigger: wf-rect failed (${e}), falling back to screen dims` }).catch(() => {});
        const sw = window.screen.width, sh = window.screen.height;
        await openOverlay(0, 0, sw, sh, 0.60, 0.30);
      }
    });

    const unsubRelic = listen<boolean>("relic-screen", () => { closeOverlay(); });

    const unsub = listen<{ items: string[]; positions: number[] } | null>("relic-rewards", async (e) => {
      const rewards = e.payload;
      if (!rewards || rewards.items.length === 0) { closeOverlay(); return; }
      const enabled = localStorage.getItem("ff-overlay-enabled") !== "false";
      if (!enabled) return;
      invoke("log_relic_fe", { msg: `[APP] relic-rewards: ${rewards.items.length} items, overlayVisible=${overlayVisible}` }).catch(() => {});
      // Overlay.tsx already receives this event directly from Rust's global emit.
      // We only need to ensure the overlay window is on-screen; no forwarding needed
      // (forwarding via emitTo caused an infinite feedback loop in Tauri 2).
      if (!overlayVisible) {
        try {
          const [wx, wy, ww, wh] = await invoke<[number, number, number, number]>("get_warframe_window_rect");
          invoke("log_relic_fe", { msg: `[APP] relic-rewards fallback: wf(${wx},${wy} ${ww}×${wh})` }).catch(() => {});
          await openOverlay(wx, wy, ww, wh, 0.54, 0.28);
        } catch (err) {
          invoke("log_relic_fe", { msg: `[APP] relic-rewards fallback: wf-rect failed (${err}), using screen dims` }).catch(() => {});
          const sw = window.screen.width, sh = window.screen.height;
          await openOverlay(0, 0, sw, sh, 0.54, 0.28);
        }
      }
    });

    const unsubReward = listen<{ path: string; qty: number }>("inventory-reward", (e) => {
      const { path, qty } = e.payload;
      setQuantities(prev => ({ ...prev, [path]: qty }));
    });

    return () => {
      unsub.then(fn => fn());
      unsubRelic.then(fn => fn());
      unsubTrigger.then(fn => fn());
      unsubStatus.then(fn => fn());
      unsubReward.then(fn => fn());
      invoke("move_overlay_offscreen").catch(() => {});
    };
  }, []);

  // ── In-game trade detection ───────────────────────────────────────────────
  // Rust emits "trade-completed" when "The trade was successful!" is detected in
  // EE.log. One event covers ALL items from both sides of the trade session.
  useEffect(() => {
    const unlisten = listen<{
      sessionId: string;
      withPlayer: string;
      tradeType: "sale" | "purchase" | "trade";
      offeredItems: { name: string; qty: number }[];
      offeredPlat: number;
      receivedItems: { name: string; qty: number }[];
      receivedPlat: number;
      timestamp: string;
    }>("trade-completed", async (e) => {
      const p = e.payload;
      const save = (dir: string, name: string, qty: number, plat: number) =>
        invoke("add_trade", {
          withPlayer: p.withPlayer,
          direction:  dir,
          itemName:   name,
          itemUrl:    "",
          quantity:   qty,
          platinum:   plat,
          source:     "in-game",
          notes:      "",
          sessionId:  p.sessionId,
          tradeType:  p.tradeType,
          timestamp:  p.timestamp,
        }).catch(() => {});

      if (p.tradeType === "sale") {
        // Gave items, received platinum — put plat on the first row only
        for (let i = 0; i < p.offeredItems.length; i++) {
          const item = p.offeredItems[i];
          await save("sold", item.name, item.qty, i === 0 ? p.receivedPlat : 0);
        }
      } else if (p.tradeType === "purchase") {
        // Gave platinum, received items — put plat on the first row only
        for (let i = 0; i < p.receivedItems.length; i++) {
          const item = p.receivedItems[i];
          await save("bought", item.name, item.qty, i === 0 ? p.offeredPlat : 0);
        }
      } else {
        // Item-for-item trade
        for (const item of p.offeredItems)  await save("traded-out", item.name, item.qty, 0);
        for (const item of p.receivedItems) await save("traded-in",  item.name, item.qty, 0);
      }
    });
    return () => { unlisten.then(fn => fn()); };
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
  }, [catalog, quantities, apiQuantities, masteryData, archonShards, subsummedWarframes, companionApiEnabled, scannerMods]);

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
    const m = new Map<string, QuantityChange>();
    for (const c of changeLog) {
      if (!m.has(c.unique_name)) m.set(c.unique_name, c);
    }
    return m;
  }, [changeLog]);

  const craftingMap = useMemo(() => {
    const m = new Map<string, CraftingJob>();
    for (const c of crafting) m.set(c.unique_name, c);
    return m;
  }, [crafting]);

  const visibleItems = useMemo(() => {
    const q = search.toLowerCase();
    const out: (CatalogItem & { qty: number })[] = [];
    for (const i of catalog) {
      if (i.name === "Blueprint") continue;
      if (category !== "all" && i.category !== category) continue;
      if (q && !i.name.toLowerCase().includes(q)) continue;
      const qty = inventory[i.unique_name]?.quantity ?? 0;
      if (filterOwned    && qty === 0) continue;
      if (filterRecent   && lastChanged[i.unique_name] == null) continue;
      if (filterPrime    && !i.name.includes("Prime") && i.vaulted == null) continue;
      if (filterVaulted  && i.vaulted !== true) continue;
      if (filterUnvaulted && i.vaulted !== false) continue;
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
      out.push({ ...i, qty });
    }
    out.sort((a, b) => {
      if (sortMode === "recent") {
        const at = lastChanged[a.unique_name] ?? 0;
        const bt = lastChanged[b.unique_name] ?? 0;
        return bt - at || a.name.localeCompare(b.name);
      }
      const aOwned = a.qty > 0 ? 1 : 0;
      const bOwned = b.qty > 0 ? 1 : 0;
      if (bOwned !== aOwned) return bOwned - aOwned;
      if (sortMode === "name-asc")  return a.name.localeCompare(b.name);
      if (sortMode === "name-desc") return b.name.localeCompare(a.name);
      if (sortMode === "qty-asc")   return a.qty - b.qty || a.name.localeCompare(b.name);
      return b.qty - a.qty || a.name.localeCompare(b.name);
    });
    return out.slice(0, 1000);
  }, [catalog, inventory, category, search, filterOwned, filterRecent, filterPrime, filterVaulted, filterUnvaulted, filterRank, sortMode, lastChanged, modCopiesMap]); // eslint-disable-line

  // ─── Render ─────────────────────────────────────────────────────────────────

  return (
    <ImgCacheDirContext.Provider value={imgCacheDir}>
    <div className="shell">

      {/* ── Header ── */}
      <header className="header">
        <span className="header-title">FrameForge</span>
          {updateAvailable && (
            <a
              className="update-badge"
              title={`v${updateAvailable} available — click to download`}
              onClick={() => invoke("plugin:opener|open_url", { url: "https://github.com/WyrmStudios/FrameForge/releases/latest" }).catch(() => {})}
            >⬆ v{updateAvailable}</a>
          )}
        {masteryRank !== null && (
          <span className="mastery-badge" title="Mastery Rank">MR {masteryRank}</span>
        )}
        {playerName && (
          <span className="player-name-badge" title="Logged-in Warframe account">{playerName}</span>
        )}
        {blobStage === "done" && (
          <span className="blob-status-badge blob-status-done" title="Inventory loaded from Warframe memory">
            Inventory Loaded
          </span>
        )}
        <div className="header-right">
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
              : "No Game";

            // WF API chip
            const wfApiState: "online"|"warn"|"offline"|"disabled" =
              !companionApiEnabled                    ? "disabled"
              : wfConnected                           ? "online"
              : warframeRunning                       ? "warn"
              : "offline";
            const wfApiDetail =
              !companionApiEnabled                    ? "OFF"
              : wfConnected && lastApiRefresh         ? timeStr(lastApiRefresh)
              : wfConnected                           ? "Connected"
              : warframeRunning                       ? "Connecting…"
              : "Waiting";
            const wfApiClick = (!wfConnected && warframeRunning && companionApiEnabled)
              ? async () => {
                  try {
                    const [accountId, nonce, steamId] = await invoke<[string, string, string]>("scan_warframe_credentials");
                    const data = await invoke<any>("fetch_warframe_inventory", { accountId, nonce, steamId });
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
                <span
                  className={`conn-chip conn-${scanState}`}
                  title={!memoryScannerEnabled ? "Memory scanner disabled — enable in Settings" : warframeRunning ? "Warframe detected — scanning memory" : "Warframe not detected"}
                  onClick={!memoryScannerEnabled ? () => setShowSettings(true) : undefined}
                >
                  <span className="conn-dot" />
                  <span className="conn-label">Memory</span>
                  <span className="conn-detail">{scanDetail}</span>
                </span>
                <span
                  className={`conn-chip conn-${wfApiState}`}
                  title={!companionApiEnabled ? "Warframe API disabled — enable in Settings" : wfConnected ? "Warframe API connected — auto-refreshes every 30s" : warframeRunning ? "Click to retry credential scan" : "Waiting for Warframe to start"}
                  onClick={!companionApiEnabled ? () => setShowSettings(true) : wfApiClick}
                  style={wfApiClick ? { cursor: "pointer" } : undefined}
                >
                  <span className="conn-dot" />
                  <span className="conn-label">WF API</span>
                  <span className="conn-detail">{wfApiDetail}</span>
                </span>
                <span
                  className={`conn-chip conn-${wfmState}`}
                  title={wfmLoggedIn ? "Logged in to warframe.market" : "Not logged in to warframe.market — open the Market tab to log in"}
                  onClick={!wfmLoggedIn ? () => setActiveModule("market") : undefined}
                  style={!wfmLoggedIn ? { cursor: "pointer" } : undefined}
                >
                  <span className="conn-dot" />
                  <span className="conn-label">WFM</span>
                  <span className="conn-detail">{wfmDetail}</span>
                </span>
                {overlayStatus && (
                  <span className="conn-chip conn-overlay">
                    <span className="conn-dot" />
                    <span className="conn-detail">{overlayStatus}</span>
                  </span>
                )}
              </>
            );
          })()}
          <button
            className="btn-icon-brand btn-discord"
            title="Join our Discord"
            onClick={() => invoke("plugin:opener|open_url", { url: "https://discord.gg/7NMsN9J8vy" }).catch(() => {})}
          >
            <svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor">
              <path d="M20.317 4.37a19.791 19.791 0 0 0-4.885-1.515.074.074 0 0 0-.079.037c-.21.375-.444.864-.608 1.25a18.27 18.27 0 0 0-5.487 0 12.64 12.64 0 0 0-.617-1.25.077.077 0 0 0-.079-.037A19.736 19.736 0 0 0 3.677 4.37a.07.07 0 0 0-.032.027C.533 9.046-.32 13.58.099 18.057a.082.082 0 0 0 .031.057 19.9 19.9 0 0 0 5.993 3.03.078.078 0 0 0 .084-.028c.462-.63.874-1.295 1.226-1.994a.076.076 0 0 0-.041-.106 13.107 13.107 0 0 1-1.872-.892.077.077 0 0 1-.008-.128 10.2 10.2 0 0 0 .372-.292.074.074 0 0 1 .077-.01c3.928 1.793 8.18 1.793 12.062 0a.074.074 0 0 1 .078.01c.12.098.246.198.373.292a.077.077 0 0 1-.006.127 12.299 12.299 0 0 1-1.873.892.077.077 0 0 0-.041.107c.36.698.772 1.362 1.225 1.993a.076.076 0 0 0 .084.028 19.839 19.839 0 0 0 6.002-3.03.077.077 0 0 0 .032-.054c.5-5.177-.838-9.674-3.549-13.66a.061.061 0 0 0-.031-.03z"/>
            </svg>
          </button>
          <button
            className="btn-icon-brand btn-kofi"
            title="Support on Ko-Fi"
            onClick={() => invoke("plugin:opener|open_url", { url: "https://ko-fi.com/sikewyrm" }).catch(() => {})}
          >
            <svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor">
              <path d="M23.881 8.948c-.773-4.085-4.859-4.593-4.859-4.593H.723c-.604 0-.679.798-.679.798s-.082 7.324-.022 11.822c.164 2.424 2.586 2.672 2.586 2.672s8.267-.023 11.966-.049c2.438-.426 2.683-2.566 2.658-3.734 4.352.24 7.422-2.831 6.649-6.916zm-11.062 3.511c-1.246 1.453-4.011 3.976-4.011 3.976s-.121.119-.31.023c-.076-.057-.108-.09-.108-.09-.443-.441-3.368-3.049-4.034-3.954-.709-.965-1.041-2.7-.091-3.71.951-1.01 3.005-1.086 4.363.407 0 0 1.565-1.782 3.468-.963 1.904.82 1.832 2.833.723 4.311zm6.173.478c-.928.116-1.218-.443-1.218-.443s.001-1.929 0-2.535c-.003-.434-.423-.782-.857-.782-.434 0-.836.348-.836.782v3.09c0 .434.402.782.836.782.434 0 .857-.026.857-.026s-.038.639.525.98c.562.341 1.423.231 1.423.231 1.302-.269 2.023-1.63 1.27-2.079z"/>
            </svg>
          </button>
          <button
            className="btn-icon-brand btn-report"
            title="Report a bug or suggest a feature"
            onClick={() => invoke("plugin:opener|open_url", { url: "https://github.com/WyrmStudios/FrameForge/issues/new/choose" }).catch(() => {})}
          >
            <svg width="16" height="16" viewBox="0 0 16 16" fill="currentColor">
              <path d="M8 0c4.42 0 8 3.58 8 8a8.013 8.013 0 0 1-5.45 7.59c-.4.08-.55-.17-.55-.38 0-.27.01-1.13.01-2.2 0-.75-.25-1.23-.54-1.48 1.78-.2 3.65-.88 3.65-3.95 0-.88-.31-1.59-.82-2.15.08-.2.36-1.02-.08-2.12 0 0-.67-.22-2.2.82-.64-.18-1.32-.27-2-.27-.68 0-1.36.09-2 .27-1.53-1.03-2.2-.82-2.2-.82-.44 1.1-.16 1.92-.08 2.12-.51.56-.82 1.28-.82 2.15 0 3.06 1.86 3.75 3.64 3.95-.23.2-.44.55-.51 1.07-.46.21-1.61.55-2.33-.66-.15-.24-.6-.83-1.23-.82-.67.01-.27.38.01.53.34.19.73.9.82 1.13.16.45.68 1.31 2.69.94 0 .67.01 1.3.01 1.49 0 .21-.15.45-.55.38A7.995 7.995 0 0 1 0 8c0-4.42 3.58-8 8-8Z"/>
            </svg>
          </button>
          <button className="btn-settings" title="Settings" onClick={() => {
              setShowSettings(true); setClearMsg("");
              getVersion().then(v => setAppVersion(v)).catch(() => {});
            }}>⚙</button>
        </div>
      </header>

      {/* ── Settings modal ── */}
      {showSettings && (
        <div className="settings-overlay" onClick={() => setShowSettings(false)}>
          <div className="settings-modal" onClick={e => e.stopPropagation()}>
            <div className="settings-header">
              <span className="settings-title">Settings</span>
              <button className="craft-detail-close" onClick={() => setShowSettings(false)}>✕</button>
            </div>

            <div className="settings-layout">
              {/* ── Sidebar nav ── */}
              <nav className="settings-sidebar">
                {(["general", "market", "accessibility", "data", "debugging"] as const).map(tab => (
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
                          localStorage.setItem("ff-overlay-enabled", String(next));
                          settingsRef.current = { ...settingsRef.current, overlayEnabled: next };
                          saveAllSettings();
                          if (!next) {
                            import("@tauri-apps/api/event").then(({ emit }) =>
                              emit("relic-screen", true).catch(() => {})
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
                          const next = e.target.value;
                          setOverlayPriority(next);
                          localStorage.setItem("ff-overlay-priority", next);
                          settingsRef.current = { ...settingsRef.current, overlayPriority: next };
                          saveAllSettings();
                        }}
                      >
                        <option value="completion">Item Completion</option>
                        <option value="setPlat">Most Set Value (plat)</option>
                        <option value="plat">Most Plat (item)</option>
                        <option value="ducat">Most Ducats</option>
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

                {/* ════════════ MARKET ════════════ */}
                {settingsTab === "market" && <>
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
                          localStorage.setItem("ff-colorblind", String(next));
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
                      <input type="range" min="0.8" max="1.4" step="0.05" value={textScale}
                        style={{ width: 120 }}
                        onChange={e => {
                          const v = parseFloat(e.target.value);
                          setTextScale(v);
                          document.documentElement.style.setProperty("--ff-scale", v.toString());
                          localStorage.setItem("ff-text-scale", v.toString());
                          settingsRef.current = { ...settingsRef.current, textScale: v };
                          saveAllSettings();
                        }} />
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
                      <button className="btn-secondary" onClick={() => { setShowSettings(false); handleFetch(); }} disabled={fetching}>
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
                        <span className="settings-row-desc">Reset all scanned quantities and change log.</span>
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
                            setChangeLog([]);
                            setLastChanged({});
                            setWfConnected(false);
                            wfConnectedRef.current = false;
                            invoke("save_api_inventory", { apiQuantities: {}, apiModCopies: [], consumedSuits: [] }).catch(() => {});
                            setItemsRefreshKey(k => k + 1);
                            setClearMsg("Cache cleared.");
                          } catch (e) { setClearMsg(`Error: ${e}`); }
                        }}
                      >Clear Cache</button>
                    </div>
                    {clearMsg && <div className="settings-msg">{clearMsg}</div>}
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
                      >{blobLogSize > 0 ? `Clear (${fmtBytes(blobLogSize)})` : "Clear"}</button>

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
                      >{apiLogSize > 0 ? `Clear (${fmtBytes(apiLogSize)})` : "Clear"}</button>

                    </div>
                  </div>

                  <div className="settings-section">
                    <div className="settings-section-title">Diagnostics</div>
                    <div className="debug-table">

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
                      <div />{/* Clear placeholder */}

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
                          localStorage.setItem("ff-auto-diag", String(next));
                          settingsRef.current = { ...settingsRef.current, autoDiagEnabled: next };
                          saveAllSettings();
                        }}>{autoDiagEnabled ? "On" : "Off"}</button>
                      <button className="btn-secondary"
                        style={{ color: diagFolderSize > 0 ? "var(--red)" : undefined, borderColor: diagFolderSize > 0 ? "var(--red)" : undefined }}
                        disabled={diagFolderSize === 0}
                        onClick={async () => { await invoke("clear_diag_folder"); setDiagFolderSize(0); }}
                      >{diagFolderSize > 0 ? `Clear (${fmtBytes(diagFolderSize)})` : "Clear"}</button>

                      {/* Manual Capture */}
                      <div className="settings-row-info">
                        <span className="settings-row-label">Manual Capture</span>
                        <span className="settings-row-desc">
                          Take a diagnostic screenshot + scan log right now.
                          {diagPath && <span style={{ display: "block", marginTop: 2, color: "var(--green)", fontSize: 11 }}>Saved.</span>}
                        </span>
                      </div>
                      <button className="btn-secondary" onClick={() => invoke("open_debug_folder", { which: "diag" }).catch(() => {})}>Go To Folder</button>
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
                      >{probeSize > 0 ? `Clear (${fmtBytes(probeSize)})` : "Clear"}</button>

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
                      >{rawScanSize > 0 ? `Clear (${fmtBytes(rawScanSize)})` : "Clear"}</button>

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
      )}

      <div className="body">

        {/* ── Module navigation ── */}
        <nav className="module-nav">
          <button
            className={`module-btn ${activeModule === "inventory" ? "module-active" : ""}`}
            onClick={() => setActiveModule("inventory")}
            title="Inventory"
          >
            <img src="/inventory-icon.png" alt="" style={{ width: 24, height: 24, objectFit: "contain" }} />
            <span className="module-label">Inventory</span>
          </button>
          <button
            className={`module-btn ${activeModule === "foundry" ? "module-active" : ""}`}
            onClick={() => setActiveModule("foundry")}
            title="Foundry"
          >
            <img src="/foundry-icon.png" alt="" style={{ width: 24, height: 24, objectFit: "contain" }} />
            <span className="module-label">Foundry</span>
          </button>
          <button
            className={`module-btn ${activeModule === "market" ? "module-active" : ""}`}
            onClick={() => setActiveModule("market")}
            title="Market Helper"
          >
            <img src="/market-icon.png" alt="" style={{ width: 24, height: 24, objectFit: "contain" }} />
            <span className="module-label">Market</span>
          </button>
          <button
            className={`module-btn ${activeModule === "relics" ? "module-active" : ""}`}
            onClick={() => setActiveModule("relics")}
            title="Relic Helper"
          >
            <img src="/relic-icon.png" alt="" style={{ width: 24, height: 24, objectFit: "contain" }} />
            <span className="module-label">Relics</span>
          </button>
          <button
            className={`module-btn ${activeModule === "timers" ? "module-active" : ""}`}
            onClick={() => setActiveModule("timers")}
            title="Timers"
          >
            <img src="/timers-icon.png" alt="" style={{ width: 24, height: 24, objectFit: "contain" }} />
            <span className="module-label">Timers</span>
          </button>
          <button
            className={`module-btn ${activeModule === "statistics" ? "module-active" : ""}`}
            onClick={() => setActiveModule("statistics")}
            title="Statistics"
          >
            <img src="/statistics-icon.png" alt="" style={{ width: 24, height: 24, objectFit: "contain" }} />
            <span className="module-label">Statistics</span>
          </button>
          <button
            className={`module-btn ${activeModule === "rivens" ? "module-active" : ""}`}
            onClick={() => setActiveModule("rivens")}
            title="Riven Analyzer"
          >
            <img src="/riven-icon.png" alt="" style={{ width: 24, height: 24, objectFit: "contain" }} />
            <span className="module-label">Rivens</span>
          </button>
          <button
            className={`module-btn ${activeModule === "completionist" ? "module-active" : ""}`}
            onClick={() => setActiveModule("completionist")}
            title="Completionist"
          >
            <img src="/completionist-icon.png" alt="" style={{ width: 24, height: 24, objectFit: "contain" }} />
            <span className="module-label">Completionist</span>
          </button>
        </nav>

        {/* ── Inventory module ── */}
        {activeModule === "inventory" && (
          <>
            <aside className="sidebar">
              <div className="sidebar-section-label">Categories</div>
              {CATEGORIES.map(cat => {
                const owned = categoryCounts.owned[cat.id] ?? 0;
                const total = categoryCounts.total[cat.id] ?? 0;
                return (
                  <button
                    key={cat.id}
                    className={`cat-btn ${category === cat.id ? "cat-active" : ""}`}
                    onClick={() => setCategory(cat.id)}
                  >
                    <span className="cat-label">{cat.label}</span>
                    <span className="cat-count">
                      {owned > 0 ? <span className="cat-owned">{owned}</span> : null}
                      {owned > 0 && <span className="cat-sep">/</span>}
                      <span className="cat-total">{total}</span>
                    </span>
                  </button>
                );
              })}
              <div className="sidebar-divider" />
              <div className="sidebar-section-label">Item Database</div>
              <div className="db-count">{itemCount.toLocaleString()} items · {recipeCount.toLocaleString()} recipes</div>
              <button className="btn-fetch" onClick={handleFetch} disabled={fetching}>
                {fetching ? "Fetching…" : "Refresh item list"}
              </button>
              {fetchMsg && <div className="fetch-msg">{fetchMsg}</div>}
            </aside>

            <div className="main">
              {monitoring && warframeRunning && !inventorySynced && (
                <div className="sync-banner">
                  Inventory not synced yet — complete a mission or visit a relay to load your inventory
                </div>
              )}

              <div className="toolbar">
                <input
                  className="search-box"
                  placeholder="Search items…"
                  value={search}
                  onChange={e => setSearch(e.target.value)}
                />
              </div>
              <div className="filter-bar">
                <button className={`fchip ${filterOwned?"fchip-on":""}`} onClick={()=>setFilterOwned(v=>!v)}>Owned</button>
                <button className={`fchip ${filterRecent?"fchip-on":""}`} onClick={()=>setFilterRecent(v=>!v)}>Changed recently</button>
                <button className={`fchip ${filterPrime?"fchip-on":""}`} onClick={()=>setFilterPrime(v=>!v)}>Prime</button>
                <button className={`fchip ${filterVaulted?"fchip-on":""}`} onClick={()=>setFilterVaulted(v=>!v)}>🔒 Vaulted</button>
                <button className={`fchip ${filterUnvaulted?"fchip-on":""}`} onClick={()=>setFilterUnvaulted(v=>!v)}>🔓 Unvaulted</button>
                {apiModCopies.length > 0 && (<>
                  <span className="fbar-sep"/>
                  <span className="fbar-label">Rank:</span>
                  <button className={`fchip ${filterRank==="unranked"?"fchip-on":""}`} onClick={()=>setFilterRank(v=>v==="unranked"?null:"unranked")}>Unranked</button>
                  {availableRanks.map(r=>(
                    <button key={r} className={`fchip ${filterRank===r?"fchip-on":""}`} onClick={()=>setFilterRank(v=>v===r?null:r)}>R{r}</button>
                  ))}
                </>)}
                <span className="fbar-sep"/>
                <span className="fbar-label">Sort:</span>
                <button className={`fchip ${sortMode==="qty-desc"?"fchip-on":""}`} onClick={()=>setSortMode("qty-desc")}>Qty ↓</button>
                <button className={`fchip ${sortMode==="qty-asc"?"fchip-on":""}`} onClick={()=>setSortMode("qty-asc")}>Qty ↑</button>
                <button className={`fchip ${sortMode==="name-asc"?"fchip-on":""}`} onClick={()=>setSortMode("name-asc")}>A-Z</button>
                <button className={`fchip ${sortMode==="name-desc"?"fchip-on":""}`} onClick={()=>setSortMode("name-desc")}>Z-A</button>
                <span className="item-count-label" style={{marginLeft:"auto"}}>{visibleItems.length} item{visibleItems.length!==1?"s":""}{visibleItems.length===1000?" (capped)":""}</span>
                <HelpTip items={[
                  { icon: "★",  label: "★  Mastered",  desc: "Shown above image — item levelled to rank 30" },
                  { icon: "R5", label: "R{n}  Rank",   desc: "Shown above image — current rank, not yet mastered" },
                  { icon: "⚒",  label: "⚒  Building",  desc: "Shown on image — currently crafting in Foundry" },
                  { swatch: "rgba(63,185,80,.5)",  label: "Green border", desc: "Item recently gained" },
                  { swatch: "rgba(248,81,73,.5)",  label: "Red border",   desc: "Item recently lost or consumed" },
                ]} />
              </div>

              <div className="item-grid">
                {visibleItems.length === 0 ? (
                  <div className="empty-msg" style={{gridColumn:"1/-1"}}>
                    {monitoring
                      ? "No items found. Complete a mission or visit a relay to sync inventory."
                      : "Start the monitor to begin tracking your inventory."}
                  </div>
                ) : (
                  visibleItems.flatMap(item => {
                    // Mods & Arcanes: single card with inline rank breakdown
                    if ((item.category === "Mods" || item.category === "Arcanes") && modCopiesMap[item.unique_name]) {
                      const copies = modCopiesMap[item.unique_name];
                      const byRank: Record<number, number> = {};
                      for (const c of copies) byRank[c.rank ?? 0] = (byRank[c.rank ?? 0] ?? 0) + c.count;
                      const maxRank = Math.max(...Object.keys(byRank).map(Number));
                      const ranks = Array.from({ length: maxRank + 1 }, (_, r) => ({ rank: r, count: byRank[r] ?? 0 })).filter(r => r.count > 0);
                      if (filterRank !== null) {
                        const targetRank = filterRank === "unranked" ? 0 : filterRank as number;
                        if ((byRank[targetRank] ?? 0) === 0) return [];
                      }
                      const total = Object.values(byRank).reduce((a, b) => a + b, 0);
                      return [(
                        <InvModCard key={item.unique_name}
                          unique_name={item.unique_name} name={item.name}
                          category={item.category} image_name={item.image_name}
                          ranks={ranks} total={total} />
                      )];
                    }

                    // Normal item card
                    const changedAt = lastChanged[item.unique_name];
                    const recentChange = changedAt != null ? changeLogMap.get(item.unique_name) : undefined;
                    const craftJob = craftingMap.get(item.unique_name);
                    return [(
                      <InvCard key={item.unique_name}
                        unique_name={item.unique_name} name={item.name}
                        category={item.category} image_name={item.image_name}
                        qty={item.qty}
                        isFavorite={favoritesSet.has(item.unique_name)}
                        changedAt={changedAt}
                        recentDelta={recentChange?.delta ?? null}
                        craftJobName={craftJob?.item_name ?? null}
                        masteryRank={inventory[item.unique_name]?.mastery_rank}
                        onToggleFavorite={toggleFavorite} />
                    )];
                  })
                )}
              </div>

              <div className="log-panel" style={{ height: logPanelH }}>
                <div
                  className="log-resize-handle"
                  onMouseDown={e => {
                    const startY = e.clientY;
                    const startH = logPanelH;
                    const onMove = (me: MouseEvent) => {
                      const delta = startY - me.clientY;
                      setLogPanelH(Math.max(80, Math.min(600, startH + delta)));
                    };
                    const onUp = () => {
                      window.removeEventListener("mousemove", onMove);
                      window.removeEventListener("mouseup", onUp);
                    };
                    window.addEventListener("mousemove", onMove);
                    window.addEventListener("mouseup", onUp);
                  }}
                />
                <div className="log-header">Change log</div>
                <div className="log-list">
                  {changeLog.length === 0 ? (
                    <span className="log-empty">No changes recorded yet.</span>
                  ) : (
                    changeLog.map((c, i) => {
                      const logItem = catalogRef.current.find(ci => ci.unique_name === c.unique_name);
                      return (
                        <div key={c.id || i} className="log-row">
                          <span className="log-name">
                            {c.item_name}
                            {logItem && <span className="log-cat">{logItem.category}</span>}
                          </span>
                          <span className={`log-delta ${deltaClass(c.delta)}`}>{deltaText(c.delta)}</span>
                          <span className="log-range">{fmt(c.old_qty)} → {fmt(c.new_qty)}</span>
                          <span className="log-time">{timeStr(c.timestamp)}</span>
                        </div>
                      );
                    })
                  )}
                </div>
              </div>
            </div>
          </>
        )}

        {/* ── Foundry module ── */}
        {activeModule === "foundry" && (
          <ErrorBoundary>
            <Foundry inventory={inventory} refreshKey={itemsRefreshKey} crafting={crafting} colorblindMode={colorblindMode} subsummedWarframes={subsummedWarframes} tracked={tracked} onTrackToggle={toggleTracked} filters={foundryFilters} onFiltersChange={setFoundryFilters} />
          </ErrorBoundary>
        )}

        {/* ── Market Helper module ── */}
        {activeModule === "market" && (
          <MarketHelper inventory={inventory} refreshKey={itemsRefreshKey} crafting={crafting} onWfmLoginChange={handleWfmLoginChange} filters={marketFilters} onFiltersChange={setMarketFilters} modCopiesMap={modCopiesMap} />
        )}

        {/* ── Relics module ── */}
        {activeModule === "relics" && (
          <ErrorBoundary>
            <RelicHelper inventory={inventory} refreshKey={itemsRefreshKey} colorblindMode={colorblindMode} filters={relicFilters} onFiltersChange={setRelicFilters} />
          </ErrorBoundary>
        )}

        {/* ── Rivens module ── */}
        {activeModule === "rivens" && (
          <ErrorBoundary>
            <div style={{ flex: 1, display: "flex", flexDirection: "column", overflow: "hidden", minHeight: 0 }}>
              <RivenAnalyzer />
            </div>
          </ErrorBoundary>
        )}

        {/* ── Timers module ── */}
        {activeModule === "timers" && (
          <ErrorBoundary>
            <TimerHelper
              favorites={timerFavorites}
              onFavoriteToggle={id => setTimerFavorites(prev =>
                prev.includes(id) ? prev.filter(x => x !== id) : [...prev, id]
              )}
              fissureWatches={fissureWatches}
              onAddWatch={w => setFissureWatches(prev => [...prev, w])}
              onRemoveWatch={id => setFissureWatches(prev => prev.filter(w => w.id !== id))}
              inventory={inventory}
            />
          </ErrorBoundary>
        )}

        {/* ── Statistics module ── */}
        {activeModule === "statistics" && (
          <ErrorBoundary>
            <Statistics tab={statsTab} onTabChange={setStatsTab} dateRange={reportsDateRange} onDateRangeChange={setReportsDateRange} />
          </ErrorBoundary>
        )}

        {/* ── Completionist module ── */}
        {activeModule === "completionist" && (
          <ErrorBoundary>
            <div style={{ flex: 1, display: "flex", flexDirection: "column", overflow: "hidden", minHeight: 0 }}>
              <Syndicates inventory={inventory} filters={syndicateFilters} onFiltersChange={setSyndicateFilters} />
            </div>
          </ErrorBoundary>
        )}


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
          sectionOrder={modularSectionOrder}
          onSectionOrderChange={setModularSectionOrder}
        />}

      </div>
    </div>
    </ImgCacheDirContext.Provider>
  );
}
