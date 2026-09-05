import { useEffect, useMemo, useRef, useState } from "react";
import ItemImg from "./ItemImg";
import "./ChangeLog.css";

export interface ChangeLogEntry {
  id: number;
  unique_name: string;
  item_name: string;
  old_qty: number;
  new_qty: number;
  delta: number;
  timestamp: number;
}

interface CatalogItem {
  unique_name: string;
  name: string;
  category: string;
  image_name?: string;
}

interface ChangeLogProps {
  changes: ChangeLogEntry[];
  catalog: CatalogItem[];
  clockFormat: "auto" | "12h" | "24h";
  systemLocale: string;
  onItemClick: (uniqueName: string) => void;
}

function fmt(n: number) { return n.toLocaleString(); }
function deltaText(d: number) { return fmt(Math.abs(d)); }
function timeStr(ts: number, format: ChangeLogProps["clockFormat"], locale: string) {
  const opts: Intl.DateTimeFormatOptions = { hour: "2-digit", minute: "2-digit" };
  if (format === "12h") opts.hour12 = true;
  else if (format === "24h") opts.hour12 = false;
  return new Date(ts * 1000).toLocaleTimeString(locale, opts);
}

function ChangeRow({
  change, item, clockFormat, systemLocale, onItemClick, onRowClick, timeBreak = false, feed = false,
}: {
  change: ChangeLogEntry;
  item?: CatalogItem;
  clockFormat: ChangeLogProps["clockFormat"];
  systemLocale: string;
  onItemClick: () => void;
  onRowClick?: () => void;
  timeBreak?: boolean;
  feed?: boolean;
}) {
  const name = item?.name ?? change.item_name;
  const category = item?.category ?? "Miscellaneous";
  return (
    <div
      className={`inv-card inv-card-row log-item-row${feed ? " log-feed-row" : ""}${timeBreak ? " log-time-break" : ""}`}
      role={onRowClick ? "button" : undefined}
      tabIndex={onRowClick ? 0 : undefined}
      title={onRowClick ? "Open changelog" : `Open ${name} in Inventory`}
      onClick={onRowClick}
      onKeyDown={e => { if (onRowClick && (e.key === "Enter" || e.key === " ")) { e.preventDefault(); onRowClick(); } }}
    >
      <span className="log-time">{timeStr(change.timestamp, clockFormat, systemLocale)}</span>
      <div className="inv-row-icon">
        <ItemImg imageName={item?.image_name} category={category} size={20} />
      </div>
      <div className="inv-row-name log-name-group">
        <button className="log-name-link" onClick={e => { e.stopPropagation(); onItemClick(); }}>{name}</button>
        <span className="log-cat">{category}</span>
      </div>
      <div className="inv-row-qty log-change-qty">
        <span className="log-range">{fmt(change.old_qty)} → {fmt(change.new_qty)}</span>
        <span className={`log-direction ${change.delta > 0 ? "log-positive" : "log-negative"}`} aria-label={change.delta > 0 ? "Increased" : "Decreased"}>
          {change.delta > 0 ? "↑" : "↓"}
        </span>
        <span className={`log-amount ${change.delta > 0 ? "log-positive" : "log-negative"}`}>{deltaText(change.delta)}</span>
      </div>
    </div>
  );
}

export default function ChangeLog({ changes, catalog, clockFormat, systemLocale, onItemClick }: ChangeLogProps) {
  const [expanded, setExpanded] = useState(false);
  const [panelHeight, setPanelHeight] = useState(270);
  const [resizing, setResizing] = useState(false);
  const [feedIndex, setFeedIndex] = useState<number | null>(0);
  const feedPausedRef = useRef(false);
  const resizeFrameRef = useRef<number | null>(null);
  const resizeHeightRef = useRef(panelHeight);
  const catalogById = useMemo(
    () => new Map(catalog.map(item => [item.unique_name, item])),
    [catalog]
  );
  const visibleChanges = useMemo(
    () => catalog.length === 0
      ? changes
      : changes.filter(change => catalogById.has(change.unique_name)),
    [catalog.length, catalogById, changes]
  );
  const latestTimestamp = visibleChanges[0]?.timestamp;
  const latestBatch = latestTimestamp == null
    ? []
    : visibleChanges.filter(change => change.timestamp === latestTimestamp);

  useEffect(() => {
    setFeedIndex(latestBatch.length === 0 ? null : 0);
  }, [latestTimestamp, latestBatch.length]);

  useEffect(() => {
    if (expanded || latestBatch.length === 0) return;
    const timer = window.setInterval(() => {
      if (feedPausedRef.current) return;
      setFeedIndex(index => {
        if (index === null || index >= latestBatch.length - 1) {
          window.clearInterval(timer);
          return null;
        }
        return index + 1;
      });
    }, 4200);
    return () => window.clearInterval(timer);
  }, [expanded, latestTimestamp, latestBatch.length]);

  const feedChange = feedIndex === null ? undefined : latestBatch[feedIndex];
  const feedItem = feedChange && catalogById.get(feedChange.unique_name);

  return (
    <div
      className={`log-panel${expanded ? " log-panel-expanded" : ""}${resizing ? " log-panel-resizing" : ""}`}
      style={{ height: expanded ? panelHeight : undefined }}
    >
      {!expanded && feedChange && (
        <div
          className="log-feed"
          key={feedChange.id || feedIndex}
          onMouseEnter={() => { feedPausedRef.current = true; }}
          onMouseLeave={() => { feedPausedRef.current = false; }}
        >
          <ChangeRow
            change={feedChange}
            item={feedItem}
            clockFormat={clockFormat}
            systemLocale={systemLocale}
            onItemClick={() => onItemClick(feedChange.unique_name)}
            onRowClick={() => setExpanded(true)}
            feed
          />
        </div>
      )}

      {!expanded && !feedChange && (
        <button className="log-empty-collapsed" onClick={() => setExpanded(true)}>
          <span>Changelog</span>
          <span className="log-collapsed-count">
            {latestBatch.length === 0 ? "No changes yet" : `${latestBatch.length} change${latestBatch.length === 1 ? "" : "s"}`}
          </span>
        </button>
      )}

      {expanded && (
        <div className="log-expanded-body">
          <div
            className="log-resize-edge"
            onMouseDown={event => {
              event.preventDefault();
              event.stopPropagation();
              setResizing(true);
              const startY = event.clientY;
              const startHeight = panelHeight;
              const scale = parseFloat(getComputedStyle(document.documentElement).getPropertyValue("--ff-scale")) || 1;
              document.body.style.userSelect = "none";
              const onMove = (moveEvent: MouseEvent) => {
                const maxHeight = Math.max(140, Math.min(800, window.innerHeight * 0.8));
                const nextHeight = Math.max(100, Math.min(maxHeight, startHeight + (startY - moveEvent.clientY) / scale));
                resizeHeightRef.current = nextHeight;
                if (resizeFrameRef.current === null) {
                  resizeFrameRef.current = window.requestAnimationFrame(() => {
                    setPanelHeight(resizeHeightRef.current);
                    resizeFrameRef.current = null;
                  });
                }
              };
              const onUp = () => {
                window.removeEventListener("mousemove", onMove);
                window.removeEventListener("mouseup", onUp);
                if (resizeFrameRef.current !== null) {
                  window.cancelAnimationFrame(resizeFrameRef.current);
                  resizeFrameRef.current = null;
                }
                setPanelHeight(resizeHeightRef.current);
                document.body.style.userSelect = "";
                setResizing(false);
              };
              window.addEventListener("mousemove", onMove);
              window.addEventListener("mouseup", onUp);
            }}
          />
          <button className="log-header" aria-expanded onClick={() => setExpanded(false)}>
            <span className="log-header-title">Changelog</span>
            <span className="log-header-summary">
              {latestBatch.length} change{latestBatch.length === 1 ? "" : "s"}
            </span>
          </button>
          <div className="log-list">
            {visibleChanges.length === 0 ? (
              <span className="log-empty">No changes recorded yet.</span>
            ) : visibleChanges.map((change, index) => {
              const item = catalogById.get(change.unique_name);
              return (
                <ChangeRow
                  key={change.id || index}
                  change={change}
                  item={item}
                  clockFormat={clockFormat}
                  systemLocale={systemLocale}
                  onItemClick={() => onItemClick(change.unique_name)}
                  timeBreak={index > 0 && change.timestamp !== visibleChanges[index - 1].timestamp}
                />
              );
            })}
          </div>
        </div>
      )}
    </div>
  );
}
