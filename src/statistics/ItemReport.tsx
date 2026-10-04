import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useCatalog } from "../hooks/useCatalog";
import type { CatalogItem } from "../types/items";
import type { SnapshotPoint, TrackedItem } from "../types/inventory";
import { PREFERENCE_KEYS } from "../constants/preferences";

type Timeframe = "7" | "30" | "90" | "all";

// ── Presentatie (ItemReport.css) ───────────────────────────────────────────────

const IR_ROOT = "flex min-h-0 flex-1 flex-col overflow-hidden";
const IR_ADD_BAR = "flex shrink-0 items-center border-b border-border px-3.5 py-2";
const IR_SEARCH_WRAP = "relative w-75";
const IR_SEARCH_INPUT =
  "w-full rounded-4 border border-border/80 bg-black/20 px-2.5 py-1.25 text-12 text-foreground outline-none focus:border-accent";
const IR_DROPDOWN =
  "absolute inset-x-0 top-[calc(100%_+_4px)] z-50 max-h-60 overflow-y-auto rounded-6 border border-border/80 bg-surface shadow-[0_4px_16px_rgba(0,0,0,0.5)]";
const IR_DROPDOWN_EMPTY = "px-3 py-2.5 text-center text-12 text-muted";
const IR_DROPDOWN_ROW =
  "flex w-full cursor-pointer items-center gap-2 border-0 bg-transparent px-3 py-1.5 text-left transition-[background] duration-100 hover:bg-white/6";
const IR_DROPDOWN_NAME = "min-w-0 flex-1 truncate text-12 text-foreground";
const IR_DROPDOWN_CAT = "shrink-0 text-10 text-muted";
const IR_SCROLL = "min-h-0 flex-1 overflow-y-auto px-3.5 py-3";
const IR_GRID = "grid grid-cols-[repeat(auto-fill,minmax(280px,1fr))] gap-2.5";
const IR_CARD =
  "flex cursor-default flex-col gap-2 rounded-8 border bg-surface px-3.5 py-3 transition-[border-color,opacity] duration-100";
const IR_DRAG_HANDLE =
  "shrink-0 select-none pr-0.5 text-16 leading-none text-muted opacity-40 transition-opacity duration-150 hover:opacity-100";
const IR_CARD_HEADER = "flex min-w-0 items-center justify-between gap-2";
const IR_CARD_NAME = "min-w-0 flex-1 truncate text-13 font-semibold text-foreground";
const IR_CARD_RIGHT = "flex shrink-0 items-center gap-1";
const IR_TF_BTNS = "flex";
const IR_TF_BASE =
  "cursor-pointer border-y border-l px-1.5 py-0.5 text-10 leading-1.4 transition-[background,color,border-color] duration-100";
const IR_TF_IDLE =
  "border-border/50 bg-transparent text-muted hover:bg-white/6 hover:text-foreground";
const IR_TF_ACTIVE = "border-accent bg-accent/15 text-accent";
const IR_TF_EDGES = "first:rounded-l-3 last:mr-1 last:rounded-r-3 last:border-r";
const IR_REMOVE_BTN =
  "cursor-pointer border-0 bg-transparent px-0.5 py-0 text-16 leading-none text-muted transition-colors duration-100 hover:text-danger";
const IR_CONFIRM_ROW = "flex shrink-0 items-center gap-1.5";
const IR_CONFIRM_MSG = "whitespace-nowrap text-11 text-danger";
const IR_CONFIRM_YES =
  "cursor-pointer rounded-3 border border-danger bg-danger/20 px-2 py-0.5 text-11 font-semibold text-danger transition-[background] duration-100 hover:bg-danger/35";
const IR_CONFIRM_NO =
  "cursor-pointer rounded-3 border border-border/60 bg-white/6 px-2 py-0.5 text-11 text-muted transition-[background] duration-100 hover:bg-white/12 hover:text-foreground";
const IR_CHART_SVG = "block h-15 w-full rounded-4 bg-black/15";
const IR_CHART_EMPTY =
  "flex h-15 items-center justify-center rounded-4 bg-black/10 text-11 italic text-muted";
const IR_CARD_STATS = "flex gap-4";
const IR_STAT = "flex flex-col gap-px";
const IR_STAT_LABEL = "text-10 uppercase tracking-0.04 text-muted";
const IR_STAT_VALUE = "text-14 font-bold tabular-nums";
const IR_LOADING = "flex flex-1 items-center justify-center text-14 text-muted";
const IR_EMPTY = "flex flex-1 flex-col items-center justify-center gap-2.5 p-10 text-center";
const IR_EMPTY_ICON = "text-48";
const IR_EMPTY_TITLE = "text-16 font-semibold text-foreground";
const IR_EMPTY_DESC = "text-13 leading-1.6 text-muted";

// ── SVG line chart ─────────────────────────────────────────────────────────────

function ItemChart({ data }: { data: SnapshotPoint[] }) {
  const W = 300, H = 60;
  const pt = 6, pb = 6, pl = 2, pr = 2;
  const cW = W - pl - pr;
  const cH = H - pt - pb;

  if (data.length === 0) {
    return <div className={IR_CHART_EMPTY}>Awaiting first snapshot…</div>;
  }

  const qtys = data.map(d => d.quantity);
  const minQ = Math.min(...qtys);
  const maxQ = Math.max(...qtys);
  const range = maxQ - minQ || 1;

  const xi = (i: number) =>
    pl + (data.length === 1 ? cW / 2 : (i / (data.length - 1)) * cW);
  const yi = (q: number) => pt + (1 - (q - minQ) / range) * cH;

  if (data.length === 1) {
    return (
      <svg viewBox={`0 0 ${W} ${H}`} className={IR_CHART_SVG} preserveAspectRatio="none">
        <line
          x1={pl} y1={H / 2} x2={pl + cW} y2={H / 2}
          stroke="var(--accent)" strokeWidth="1" strokeOpacity="0.4" strokeDasharray="4 3"
        />
        <circle cx={xi(0)} cy={H / 2} r="3" fill="var(--accent)" />
      </svg>
    );
  }

  const linePts = data.map((d, i) => `${xi(i).toFixed(1)},${yi(d.quantity).toFixed(1)}`).join(" ");
  const fillPts = `${pl},${pt + cH} ${linePts} ${pl + cW},${pt + cH}`;

  return (
    <svg viewBox={`0 0 ${W} ${H}`} className={IR_CHART_SVG} preserveAspectRatio="none">
      <polygon points={fillPts} fill="var(--accent)" fillOpacity="0.12" />
      <polyline
        points={linePts}
        fill="none"
        stroke="var(--accent)"
        strokeWidth="1.5"
        strokeLinejoin="round"
        strokeLinecap="round"
      />
    </svg>
  );
}

// ── Tracked item card ──────────────────────────────────────────────────────────

interface CardProps {
  item: TrackedItem;
  allSnapshots: SnapshotPoint[];
  timeframe: Timeframe;
  onTimeframeChange: (tf: Timeframe) => void;
  onRemove: () => void;
  onHandleMouseDown: (e: React.MouseEvent) => void;
  onCardMouseEnter: () => void;
  isDragging: boolean;
  isDragSource: boolean;
  isDragOver: boolean;
}

function TrackedItemCard({ item, allSnapshots, timeframe, onTimeframeChange, onRemove, onHandleMouseDown, onCardMouseEnter, isDragging, isDragSource, isDragOver }: CardProps) {
  const [confirmDelete, setConfirmDelete] = useState(false);

  const displayData = useMemo(() => {
    if (timeframe === "all") return allSnapshots;
    const days = Number(timeframe);
    const cutoff = new Date();
    cutoff.setDate(cutoff.getDate() - days);
    const cutStr = cutoff.toISOString().split("T")[0];
    return allSnapshots.filter(s => s.date >= cutStr);
  }, [allSnapshots, timeframe]);

  const latest = displayData.length > 0 ? displayData[displayData.length - 1].quantity : null;
  const totalGain = displayData.slice(1).reduce((s, d) => s + d.change, 0);
  const changeDays = Math.max(displayData.length - 1, 1);
  const avgPerDay = displayData.length > 1 ? Math.round(totalGain / changeDays) : null;

  const fmtChange = (n: number) =>
    n > 0 ? `+${n.toLocaleString()}` : n.toLocaleString();

  return (
    <div
      className={[
        IR_CARD,
        isDragOver ? "border-accent shadow-[0_0_0_1px_var(--accent)]" : "border-border",
        isDragSource ? "opacity-45" : "",
      ]
        .filter(Boolean)
        .join(" ")}
      onMouseEnter={onCardMouseEnter}
    >
      <div className={IR_CARD_HEADER}>
        <span className={`${IR_DRAG_HANDLE} ${isDragging ? "cursor-grabbing" : "cursor-grab"}`} title="Drag to reorder" onMouseDown={onHandleMouseDown}>⠿</span>
        <span className={IR_CARD_NAME}>{item.display_name}</span>
        {confirmDelete ? (
          <div className={IR_CONFIRM_ROW}>
            <span className={IR_CONFIRM_MSG}>Delete all history?</span>
            <button className={IR_CONFIRM_YES} onClick={onRemove}>Delete</button>
            <button className={IR_CONFIRM_NO} onClick={() => setConfirmDelete(false)}>Cancel</button>
          </div>
        ) : (
          <div className={IR_CARD_RIGHT}>
            <div className={IR_TF_BTNS}>
              {(["7", "30", "90", "all"] as Timeframe[]).map(tf => (
                <button
                  key={tf}
                  className={[
                    IR_TF_BASE,
                    timeframe === tf ? IR_TF_ACTIVE : IR_TF_IDLE,
                    timeframe === tf ? "border-r" : "",
                    IR_TF_EDGES,
                  ]
                    .filter(Boolean)
                    .join(" ")}
                  onClick={() => onTimeframeChange(tf)}
                >
                  {tf === "all" ? "All" : `${tf}d`}
                </button>
              ))}
            </div>
            <button className={IR_REMOVE_BTN} onClick={() => setConfirmDelete(true)} title="Remove tracking">×</button>
          </div>
        )}
      </div>

      <ItemChart data={displayData} />

      <div className={IR_CARD_STATS}>
        <div className={IR_STAT}>
          <span className={IR_STAT_LABEL}>Current</span>
          <span className={`${IR_STAT_VALUE} text-foreground`}>
            {latest !== null ? latest.toLocaleString() : "—"}
          </span>
        </div>
        {avgPerDay !== null && (
          <div className={IR_STAT}>
            <span className={IR_STAT_LABEL}>Avg/day</span>
            <span
              className={`${IR_STAT_VALUE} ${
                avgPerDay > 0 ? "text-success" : avgPerDay < 0 ? "text-danger" : "text-foreground"
              }`}
            >
              {fmtChange(avgPerDay)}
            </span>
          </div>
        )}
        {displayData.length > 1 && (
          <div className={IR_STAT}>
            <span className={IR_STAT_LABEL}>{timeframe === "all" ? "Total" : `${timeframe}d total`}</span>
            <span
              className={`${IR_STAT_VALUE} ${
                totalGain > 0 ? "text-success" : totalGain < 0 ? "text-danger" : "text-foreground"
              }`}
            >
              {fmtChange(totalGain)}
            </span>
          </div>
        )}
      </div>
    </div>
  );
}

// ── Main component ─────────────────────────────────────────────────────────────

export default function ItemReport() {
  const { catalog } = useCatalog();
  const [tracked, setTracked] = useState<TrackedItem[]>([]);
  const [snapshots, setSnapshots] = useState<Record<string, SnapshotPoint[]>>({});
  const [timeframes, setTimeframes] = useState<Record<string, Timeframe>>({});
  const [loading, setLoading] = useState(true);

  const [cardOrder, setCardOrder] = useState<string[]>(() => {
    try { return JSON.parse(localStorage.getItem(PREFERENCE_KEYS.ITEM_REPORT_ORDER) ?? "[]"); }
    catch { return []; }
  });
  const [draggingFrom, setDraggingFrom] = useState<string | null>(null);
  const [dragTarget, setDragTarget]     = useState<string | null>(null);
  const draggingRef = useRef<string | null>(null);
  const dragTargetRef = useRef<string | null>(null);

  const orderedTracked = useMemo(() => {
    if (cardOrder.length === 0) return tracked;
    return [...tracked].sort((a, b) => {
      const ai = cardOrder.indexOf(a.unique_name);
      const bi = cardOrder.indexOf(b.unique_name);
      if (ai === -1 && bi === -1) return 0;
      if (ai === -1) return 1;
      if (bi === -1) return -1;
      return ai - bi;
    });
  }, [tracked, cardOrder]);

  const commitDrop = useCallback(() => {
    const src = draggingRef.current;
    const tgt = dragTargetRef.current;
    if (src && tgt && src !== tgt) {
      setCardOrder(prev => {
        const base = orderedTracked.map(t => t.unique_name);
        // Use most-recent prev if available, fall back to current visible order
        const order = prev.length > 0 ? [...prev] : base;
        // Ensure all current items are represented
        for (const id of base) if (!order.includes(id)) order.push(id);
        const from = order.indexOf(src);
        const to   = order.indexOf(tgt);
        if (from !== -1 && to !== -1) {
          order.splice(from, 1);
          order.splice(to, 0, src);
        }
        localStorage.setItem(PREFERENCE_KEYS.ITEM_REPORT_ORDER, JSON.stringify(order));
        return order;
      });
    }
    draggingRef.current  = null;
    dragTargetRef.current = null;
    setDraggingFrom(null);
    setDragTarget(null);
  }, [orderedTracked]);

  // Global mouseup ends the drag from anywhere
  useEffect(() => {
    if (!draggingFrom) return;
    const up = () => commitDrop();
    window.addEventListener("mouseup", up);
    return () => window.removeEventListener("mouseup", up);
  }, [draggingFrom, commitDrop]);

  const startDrag = useCallback((id: string, e: React.MouseEvent) => {
    e.preventDefault();
    draggingRef.current  = id;
    dragTargetRef.current = id;
    setDraggingFrom(id);
    setDragTarget(id);
  }, []);

  const [searchQuery, setSearchQuery] = useState("");
  const [debouncedQuery, setDebouncedQuery] = useState("");
  const [searchOpen, setSearchOpen] = useState(false);
  const searchRef = useRef<HTMLDivElement>(null);
  const debounceRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    (async () => {
      try {
        const items = await invoke<TrackedItem[]>("get_tracked_items");
        setTracked(items);
        const snap: Record<string, SnapshotPoint[]> = {};
        await Promise.all(
          items.map(async item => {
            try {
              snap[item.unique_name] = await invoke<SnapshotPoint[]>(
                "get_item_snapshots",
                { uniqueName: item.unique_name, days: null }
              );
            } catch {
              snap[item.unique_name] = [];
            }
          })
        );
        setSnapshots(snap);
      } finally {
        setLoading(false);
      }
    })();
  }, []);

  useEffect(() => {
    if (debounceRef.current) clearTimeout(debounceRef.current);
    debounceRef.current = setTimeout(() => setDebouncedQuery(searchQuery), 150);
    return () => { if (debounceRef.current) clearTimeout(debounceRef.current); };
  }, [searchQuery]);

  const openSearch = useCallback(async () => {
    setSearchOpen(true);
  }, []);

  useEffect(() => {
    const handler = (e: MouseEvent) => {
      if (searchRef.current && !searchRef.current.contains(e.target as Node)) {
        setSearchOpen(false);
      }
    };
    document.addEventListener("mousedown", handler);
    return () => document.removeEventListener("mousedown", handler);
  }, []);

  const trackedSet = useMemo(
    () => new Set(tracked.map(t => t.unique_name)),
    [tracked]
  );

  const filteredCatalog = useMemo(() => {
    if (!catalog || !debouncedQuery.trim()) return [];
    const q = debouncedQuery.toLowerCase();
    return catalog
      .filter(c => !trackedSet.has(c.unique_name) && c.name.toLowerCase().includes(q))
      .sort((a, b) => {
        const an = a.name.toLowerCase(), bn = b.name.toLowerCase();
        const aExact = an === q, bExact = bn === q;
        if (aExact !== bExact) return aExact ? -1 : 1;
        const aStart = an.startsWith(q), bStart = bn.startsWith(q);
        if (aStart !== bStart) return aStart ? -1 : 1;
        return an.localeCompare(bn);
      })
      .slice(0, 20);
  }, [catalog, debouncedQuery, trackedSet]);

  const handleAddItem = useCallback(async (item: CatalogItem) => {
    try {
      await invoke("add_tracked_item", { uniqueName: item.unique_name, displayName: item.name });
      setTracked(prev => [...prev, {
        unique_name: item.unique_name,
        display_name: item.name,
        added_at: new Date().toISOString(),
      }]);
      setSnapshots(prev => ({ ...prev, [item.unique_name]: [] }));
      setSearchQuery("");
      setDebouncedQuery("");
      setSearchOpen(false);
    } catch (e) {
      console.error("add_tracked_item failed:", e);
    }
  }, []);

  const handleRemove = useCallback(async (uniqueName: string) => {
    try {
      await invoke("remove_tracked_item", { uniqueName });
      setTracked(prev => prev.filter(t => t.unique_name !== uniqueName));
      setSnapshots(prev => {
        const next = { ...prev };
        delete next[uniqueName];
        return next;
      });
    } catch (e) {
      console.error("remove_tracked_item failed:", e);
    }
  }, []);

  const handleTimeframeChange = useCallback((uniqueName: string, tf: Timeframe) => {
    setTimeframes(prev => ({ ...prev, [uniqueName]: tf }));
  }, []);

  if (loading) return <div className={IR_LOADING}>Loading…</div>;

  return (
    <div className={IR_ROOT}>
      <div className={IR_ADD_BAR}>
        <div className={IR_SEARCH_WRAP} ref={searchRef}>
          <input
            className={IR_SEARCH_INPUT}
            placeholder="Search items to track…"
            value={searchQuery}
            onFocus={openSearch}
            onChange={e => { setSearchQuery(e.target.value); setSearchOpen(true); }}
          />
          {searchOpen && searchQuery.trim() !== "" && (
            <div className={IR_DROPDOWN}>
              {filteredCatalog.length === 0 && debouncedQuery.trim() && (
                <div className={IR_DROPDOWN_EMPTY}>No results</div>
              )}
              {filteredCatalog.map(item => (
                <button
                  key={item.unique_name}
                  className={IR_DROPDOWN_ROW}
                  onMouseDown={e => { e.preventDefault(); handleAddItem(item); }}
                >
                  <span className={IR_DROPDOWN_NAME}>{item.name}</span>
                  <span className={IR_DROPDOWN_CAT}>{item.category}</span>
                </button>
              ))}
            </div>
          )}
        </div>
      </div>

      {tracked.length === 0 ? (
        <div className={IR_EMPTY}>
          <div className={IR_EMPTY_ICON}>📊</div>
          <div className={IR_EMPTY_TITLE}>No items tracked yet</div>
          <div className={IR_EMPTY_DESC}>
            Search for an item above to start tracking its daily quantity.
            <br />
            Snapshots are recorded once per day when FrameForge is running.
          </div>
        </div>
      ) : (
        <div className={IR_SCROLL}>
          <div
            className={`${IR_GRID}${draggingFrom ? " select-none cursor-grabbing" : ""}`}
          >
            {orderedTracked.map(item => (
              <TrackedItemCard
                key={item.unique_name}
                item={item}
                allSnapshots={snapshots[item.unique_name] ?? []}
                timeframe={timeframes[item.unique_name] ?? "30"}
                onTimeframeChange={tf => handleTimeframeChange(item.unique_name, tf)}
                onRemove={() => handleRemove(item.unique_name)}
                onHandleMouseDown={e => startDrag(item.unique_name, e)}
                onCardMouseEnter={() => {
                  if (!draggingRef.current) return;
                  dragTargetRef.current = item.unique_name;
                  setDragTarget(item.unique_name);
                }}
                isDragging={draggingFrom !== null}
                isDragSource={draggingFrom === item.unique_name}
                isDragOver={dragTarget === item.unique_name && draggingFrom !== item.unique_name}
              />
            ))}
          </div>
        </div>
      )}
    </div>
  );
}
