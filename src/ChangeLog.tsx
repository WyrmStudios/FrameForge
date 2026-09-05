import { useEffect, useRef, useState } from "react";
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
  change, item, clockFormat, systemLocale, onClick, onExpand, feed = false,
}: {
  change: ChangeLogEntry;
  item?: CatalogItem;
  clockFormat: ChangeLogProps["clockFormat"];
  systemLocale: string;
  onClick: () => void;
  onExpand?: () => void;
  feed?: boolean;
}) {
  const name = item?.name ?? change.item_name;
  const category = item?.category ?? "Miscellaneous";
  return (
    <div
      className={`inv-card inv-card-row log-item-row${feed ? " log-feed-row" : ""}`}
      role="button"
      tabIndex={0}
      title={`Open ${name} in Inventory`}
      onClick={onClick}
      onKeyDown={e => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); onClick(); } }}
    >
      <span className="log-time">{timeStr(change.timestamp, clockFormat, systemLocale)}</span>
      <div className="inv-row-icon">
        <ItemImg imageName={item?.image_name} category={category} size={20} />
      </div>
      <div className="inv-row-name log-name-group">
        <span>{name}</span>
        <span className="log-cat">{category}</span>
      </div>
      <div className="inv-row-qty log-change-qty">
        <span className="log-range">{fmt(change.old_qty)} → {fmt(change.new_qty)}</span>
        <span className={`log-direction ${change.delta > 0 ? "log-positive" : "log-negative"}`} aria-label={change.delta > 0 ? "Increased" : "Decreased"}>
          {change.delta > 0 ? "↑" : "↓"}
        </span>
        <span className={`log-amount ${change.delta > 0 ? "log-positive" : "log-negative"}`}>{deltaText(change.delta)}</span>
      </div>
      {onExpand && <button className="log-expand-button" onClick={e => { e.stopPropagation(); onExpand(); }} aria-label="Open change log">+</button>}
    </div>
  );
}

export default function ChangeLog({ changes, catalog, clockFormat, systemLocale, onItemClick }: ChangeLogProps) {
  const [expanded, setExpanded] = useState(false);
  const [panelHeight, setPanelHeight] = useState(270);
  const [resizing, setResizing] = useState(false);
  const [feedIndex, setFeedIndex] = useState<number | null>(0);
  const feedPausedRef = useRef(false);
  const visibleChanges = catalog.length === 0
    ? changes
    : changes.filter(change => catalog.some(item => item.unique_name === change.unique_name));
  const latestTimestamp = visibleChanges[0]?.timestamp;
  const latestBatch = latestTimestamp == null
    ? []
    : visibleChanges.filter(change => Math.floor(change.timestamp / 60) === Math.floor(latestTimestamp / 60));

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
  const feedItem = feedChange && catalog.find(item => item.unique_name === feedChange.unique_name);

  return (
    <div
      className={`log-panel${expanded ? " log-panel-expanded" : ""}${resizing ? " log-panel-resizing" : ""}`}
      style={{ "--log-panel-height": `${panelHeight}px` } as React.CSSProperties}
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
            onClick={() => onItemClick(feedChange.unique_name)}
            onExpand={() => setExpanded(true)}
            feed
          />
        </div>
      )}

      {!expanded && !feedChange && (
        <button className="log-empty-collapsed" onClick={() => setExpanded(true)}>
          <span>Change log</span>
          <span className="log-collapsed-count">
            {latestBatch.length === 0 ? "No changes yet" : `${latestBatch.length} change${latestBatch.length === 1 ? "" : "s"}`}
            <span>+</span>
          </span>
        </button>
      )}

      {expanded && (
        <div className="log-expanded-body">
          <div
            className="log-resize-handle"
            onMouseDown={event => {
              event.preventDefault();
              event.stopPropagation();
              setResizing(true);
              const startY = event.clientY;
              const startHeight = panelHeight;
              document.body.style.userSelect = "none";
              const onMove = (moveEvent: MouseEvent) => {
                const maxHeight = Math.max(140, Math.min(800, window.innerHeight * 0.8));
                setPanelHeight(Math.max(100, Math.min(maxHeight, startHeight + startY - moveEvent.clientY)));
              };
              const onUp = () => {
                window.removeEventListener("mousemove", onMove);
                window.removeEventListener("mouseup", onUp);
                document.body.style.userSelect = "";
                setResizing(false);
              };
              window.addEventListener("mousemove", onMove);
              window.addEventListener("mouseup", onUp);
            }}
          />
          <button className="log-header" aria-expanded onClick={() => setExpanded(false)}>
            <span className="log-header-title">Change log</span>
            <span className="log-chevron" aria-hidden="true">−</span>
          </button>
          <div className="log-list">
            {visibleChanges.length === 0 ? (
              <span className="log-empty">No changes recorded yet.</span>
            ) : visibleChanges.map((change, index) => {
              const item = catalog.find(catalogItem => catalogItem.unique_name === change.unique_name);
              return (
                <ChangeRow
                  key={change.id || index}
                  change={change}
                  item={item}
                  clockFormat={clockFormat}
                  systemLocale={systemLocale}
                  onClick={() => onItemClick(change.unique_name)}
                />
              );
            })}
          </div>
        </div>
      )}
    </div>
  );
}
