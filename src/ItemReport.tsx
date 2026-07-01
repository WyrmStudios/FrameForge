import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import "./ItemReport.css";

interface TrackedItem {
  unique_name: string;
  display_name: string;
  added_at: string;
}

interface SnapshotPoint {
  date: string;
  quantity: number;
  change: number;
}

interface CatalogItem {
  unique_name: string;
  name: string;
  category: string;
}

type Timeframe = "7" | "30" | "90" | "all";

// ── SVG line chart ─────────────────────────────────────────────────────────────

function ItemChart({ data }: { data: SnapshotPoint[] }) {
  const W = 300, H = 60;
  const pt = 6, pb = 6, pl = 2, pr = 2;
  const cW = W - pl - pr;
  const cH = H - pt - pb;

  if (data.length === 0) {
    return <div className="ir-chart-empty">Awaiting first snapshot…</div>;
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
      <svg viewBox={`0 0 ${W} ${H}`} className="ir-chart-svg" preserveAspectRatio="none">
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
    <svg viewBox={`0 0 ${W} ${H}`} className="ir-chart-svg" preserveAspectRatio="none">
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
  isDragSource: boolean;
  isDragOver: boolean;
}

function TrackedItemCard({ item, allSnapshots, timeframe, onTimeframeChange, onRemove, onHandleMouseDown, onCardMouseEnter, isDragSource, isDragOver }: CardProps) {
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
      className={`ir-card${isDragSource ? " ir-card-dragsource" : ""}${isDragOver ? " ir-card-dragover" : ""}`}
      onMouseEnter={onCardMouseEnter}
    >
      <div className="ir-card-header">
        <span className="ir-drag-handle" title="Drag to reorder" onMouseDown={onHandleMouseDown}>⠿</span>
        <span className="ir-card-name">{item.display_name}</span>
        {confirmDelete ? (
          <div className="ir-confirm-row">
            <span className="ir-confirm-msg">Delete all history?</span>
            <button className="ir-confirm-yes" onClick={onRemove}>Delete</button>
            <button className="ir-confirm-no" onClick={() => setConfirmDelete(false)}>Cancel</button>
          </div>
        ) : (
          <div className="ir-card-right">
            <div className="ir-tf-btns">
              {(["7", "30", "90", "all"] as Timeframe[]).map(tf => (
                <button
                  key={tf}
                  className={`ir-tf-btn${timeframe === tf ? " active" : ""}`}
                  onClick={() => onTimeframeChange(tf)}
                >
                  {tf === "all" ? "All" : `${tf}d`}
                </button>
              ))}
            </div>
            <button className="ir-remove-btn" onClick={() => setConfirmDelete(true)} title="Remove tracking">×</button>
          </div>
        )}
      </div>

      <ItemChart data={displayData} />

      <div className="ir-card-stats">
        <div className="ir-stat">
          <span className="ir-stat-label">Current</span>
          <span className="ir-stat-value">
            {latest !== null ? latest.toLocaleString() : "—"}
          </span>
        </div>
        {avgPerDay !== null && (
          <div className="ir-stat">
            <span className="ir-stat-label">Avg/day</span>
            <span className={`ir-stat-value ${avgPerDay > 0 ? "ir-pos" : avgPerDay < 0 ? "ir-neg" : ""}`}>
              {fmtChange(avgPerDay)}
            </span>
          </div>
        )}
        {displayData.length > 1 && (
          <div className="ir-stat">
            <span className="ir-stat-label">{timeframe === "all" ? "Total" : `${timeframe}d total`}</span>
            <span className={`ir-stat-value ${totalGain > 0 ? "ir-pos" : totalGain < 0 ? "ir-neg" : ""}`}>
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
  const [tracked, setTracked] = useState<TrackedItem[]>([]);
  const [snapshots, setSnapshots] = useState<Record<string, SnapshotPoint[]>>({});
  const [timeframes, setTimeframes] = useState<Record<string, Timeframe>>({});
  const [loading, setLoading] = useState(true);

  const [cardOrder, setCardOrder] = useState<string[]>(() => {
    try { return JSON.parse(localStorage.getItem("ff-item-report-order") ?? "[]"); }
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
        localStorage.setItem("ff-item-report-order", JSON.stringify(order));
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
  const [catalog, setCatalog] = useState<CatalogItem[] | null>(null);
  const [searchOpen, setSearchOpen] = useState(false);
  const [loadingCatalog, setLoadingCatalog] = useState(false);
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
    if (catalog !== null || loadingCatalog) return;
    setLoadingCatalog(true);
    try {
      const items = await invoke<CatalogItem[]>("get_all_items");
      setCatalog(items);
    } finally {
      setLoadingCatalog(false);
    }
  }, [catalog, loadingCatalog]);

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

  if (loading) return <div className="ir-loading">Loading…</div>;

  return (
    <div className="ir-root">
      <div className="ir-add-bar">
        <div className="ir-search-wrap" ref={searchRef}>
          <input
            className="ir-search-input"
            placeholder="Search items to track…"
            value={searchQuery}
            onFocus={openSearch}
            onChange={e => { setSearchQuery(e.target.value); setSearchOpen(true); }}
          />
          {searchOpen && (searchQuery.trim() !== "" || loadingCatalog) && (
            <div className="ir-dropdown">
              {loadingCatalog && (
                <div className="ir-dropdown-empty">Loading catalog…</div>
              )}
              {!loadingCatalog && filteredCatalog.length === 0 && debouncedQuery.trim() && (
                <div className="ir-dropdown-empty">No results</div>
              )}
              {filteredCatalog.map(item => (
                <button
                  key={item.unique_name}
                  className="ir-dropdown-row"
                  onMouseDown={e => { e.preventDefault(); handleAddItem(item); }}
                >
                  <span className="ir-dropdown-name">{item.name}</span>
                  <span className="ir-dropdown-cat">{item.category}</span>
                </button>
              ))}
            </div>
          )}
        </div>
      </div>

      {tracked.length === 0 ? (
        <div className="ir-empty">
          <div className="ir-empty-icon">📊</div>
          <div className="ir-empty-title">No items tracked yet</div>
          <div className="ir-empty-desc">
            Search for an item above to start tracking its daily quantity.
            <br />
            Snapshots are recorded once per day when FrameForge is running.
          </div>
        </div>
      ) : (
        <div className="ir-scroll">
          <div className={`ir-grid${draggingFrom ? " ir-grid-dragging" : ""}`}>
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
