import { Fragment, useEffect, useRef, useState } from "react";
import ItemImg from "./ItemImg";
import "./ChangeLog.css";

export const CHANGE_BATCH_GAP_SECONDS = 8;

export interface ChangeLogEntry {
  id: number;
  unique_name: string;
  item_name: string;
  old_qty: number;
  new_qty: number;
  delta: number;
  timestamp: number;
}

export interface ChangeLogCatalogItem {
  unique_name: string;
  name: string;
  category: string;
  image_name?: string;
}

interface ChangeLogProps {
  visibleChanges: ChangeLogEntry[];
  latestBatch: ChangeLogEntry[];
  arrivalToken: number;
  lastScanAt: number | null;
  catalog: ChangeLogCatalogItem[];
  clockFormat: "auto" | "12h" | "24h";
  systemLocale: string;
  expanded: boolean;
  height: number;
  onExpandedChange: (expanded: boolean) => void;
  onHeightChange: (height: number) => void;
  onItemClick: (uniqueName: string) => void;
  onChangeLogClick: () => void;
  onCategoryClick: (category: string) => void;
}

export function getVisibleChangeLogEntries(changes: ChangeLogEntry[], catalog: ChangeLogCatalogItem[]) {
  if (catalog.length === 0) return changes;
  const catalogIds = new Set(catalog.map(item => item.unique_name));
  return changes.filter(change => catalogIds.has(change.unique_name));
}

export function getLatestChangeBatch(changes: ChangeLogEntry[]) {
  if (changes.length === 0) return [];
  const batch = [changes[0]];
  for (let index = 1; index < changes.length; index++) {
    if (changes[index - 1].timestamp - changes[index].timestamp > CHANGE_BATCH_GAP_SECONDS) break;
    batch.push(changes[index]);
  }
  return batch;
}

function fmt(n: number) { return n.toLocaleString(); }
function deltaText(d: number) { return fmt(Math.abs(d)); }
function timeStr(ts: number, format: ChangeLogProps["clockFormat"], locale: string) {
  const opts: Intl.DateTimeFormatOptions = { hour: "2-digit", minute: "2-digit" };
  if (format === "12h") opts.hour12 = true;
  else if (format === "24h") opts.hour12 = false;
  return new Date(ts * 1000).toLocaleTimeString(locale, opts);
}

function changeKey(change: ChangeLogEntry) {
  return `${change.id}:${change.unique_name}:${change.timestamp}`;
}

function ChangeRow({
  change, item, clockFormat, systemLocale, onItemClick, onCategoryClick, onFeedExpand, timeBreak = false, feed = false,
}: {
  change: ChangeLogEntry;
  item?: ChangeLogCatalogItem;
  clockFormat: ChangeLogProps["clockFormat"];
  systemLocale: string;
  onItemClick: () => void;
  onCategoryClick: () => void;
  onFeedExpand?: () => void;
  timeBreak?: boolean;
  feed?: boolean;
}) {
  const name = item?.name ?? change.item_name;
  const category = item?.category ?? "Miscellaneous";
  return (
    <div
      className={`inv-card inv-card-row log-item-row${feed ? " log-feed-row" : ""}${timeBreak ? " log-time-break" : ""}`}
    >
      {onFeedExpand && <button className="log-feed-open" onClick={onFeedExpand} aria-label="Open change log" />}
      <span className="log-time">{timeStr(change.timestamp, clockFormat, systemLocale)}</span>
      <div className="inv-row-icon">
        <ItemImg imageName={item?.image_name} category={category} size={20} />
      </div>
      <div className="inv-row-name log-name-group">
        <button className="log-name-link" onClick={e => { e.stopPropagation(); onItemClick(); }}>{name}</button>
        <button className="log-cat log-category-link" onClick={e => { e.stopPropagation(); onCategoryClick(); }}>{category}</button>
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

function ChangeLogHeader({
  expanded, showArrival, arrivalToken, positiveChanges, negativeChanges, lastScanAt,
  clockFormat, systemLocale, onExpandedChange, onChangeLogClick,
}: {
  expanded: boolean;
  showArrival: boolean;
  arrivalToken: number;
  positiveChanges: number;
  negativeChanges: number;
  lastScanAt: number | null;
  clockFormat: ChangeLogProps["clockFormat"];
  systemLocale: string;
  onExpandedChange: (expanded: boolean) => void;
  onChangeLogClick: () => void;
}) {
  return (
    <div className="log-header">
      <button
        className="log-header-toggle"
        aria-expanded={expanded}
        aria-controls={expanded ? "change-log-list" : undefined}
        aria-label={expanded ? "Collapse change log" : "Open change log"}
        onClick={() => onExpandedChange(!expanded)}
      />
      <button className="log-header-title" onClick={onChangeLogClick}>Changelog</button>
      {showArrival && <span className="log-header-arrival log-arrival-notice" role="status" key={arrivalToken}>
        {positiveChanges > 0 && <span className="log-positive">+{positiveChanges}</span>}
        {negativeChanges > 0 && <span className="log-negative">-{negativeChanges}</span>}
      </span>}
      <span className="log-status-divider" aria-hidden="true">·</span>
      <span className="log-last-scan">last scan {lastScanAt == null ? "not yet" : timeStr(lastScanAt, clockFormat, systemLocale)}</span>
    </div>
  );
}

export default function ChangeLog({
  visibleChanges, latestBatch, catalog, clockFormat, systemLocale, expanded, height,
  arrivalToken, lastScanAt, onExpandedChange, onHeightChange, onItemClick, onChangeLogClick, onCategoryClick,
}: ChangeLogProps) {
  const resizeFrameRef = useRef<number | null>(null);
  const resizeHeightRef = useRef(height);
  const handledArrivalRef = useRef(0);
  const [showArrival, setShowArrival] = useState(false);
  const [feedIndex, setFeedIndex] = useState<number | null>(null);
  const [arrivingEntryKeys, setArrivingEntryKeys] = useState<Set<string>>(new Set());
  const catalogById = new Map(catalog.map(item => [item.unique_name, item]));
  const batchKey = latestBatch.map(changeKey).join("|");

  useEffect(() => {
    if (arrivalToken === 0 || arrivalToken === handledArrivalRef.current) return;
    handledArrivalRef.current = arrivalToken;
    if (!batchKey) {
      setShowArrival(false);
      setFeedIndex(null);
      return;
    }
    setShowArrival(true);
    if (expanded) {
      setFeedIndex(null);
      setArrivingEntryKeys(new Set(latestBatch.map(changeKey)));
      const timer = window.setTimeout(() => setArrivingEntryKeys(new Set()), 160);
      return () => window.clearTimeout(timer);
    }
    setArrivingEntryKeys(new Set());
    setFeedIndex(0);
  }, [arrivalToken, batchKey, expanded, latestBatch]);

  useEffect(() => {
    if (!expanded) return;
    setFeedIndex(null);
  }, [expanded]);

  useEffect(() => {
    if (expanded || feedIndex === null) return;
    const timer = window.setTimeout(() => {
      setFeedIndex(index => index === null || index + 1 >= latestBatch.length ? null : index + 1);
    }, 4500);
    return () => window.clearTimeout(timer);
  }, [expanded, feedIndex, latestBatch.length]);

  const positiveChanges = latestBatch.filter(change => change.delta > 0).length;
  const negativeChanges = latestBatch.filter(change => change.delta < 0).length;
  const feedChange = feedIndex === null ? undefined : latestBatch[feedIndex];
  const feedItem = feedChange && catalogById.get(feedChange.unique_name);

  return (
    <div
      className={`log-panel${expanded ? " log-panel-expanded" : ""}`}
      style={{ height: expanded ? height : undefined }}
    >
      <ChangeLogHeader
        expanded={expanded}
        showArrival={showArrival}
        arrivalToken={arrivalToken}
        positiveChanges={positiveChanges}
        negativeChanges={negativeChanges}
        lastScanAt={lastScanAt}
        clockFormat={clockFormat}
        systemLocale={systemLocale}
        onExpandedChange={onExpandedChange}
        onChangeLogClick={onChangeLogClick}
      />
      {!expanded && feedChange && <div
        className="log-feed"
        key={`${arrivalToken}:${feedIndex}:${changeKey(feedChange)}`}
      >
        <ChangeRow
          change={feedChange}
          item={feedItem}
          clockFormat={clockFormat}
          systemLocale={systemLocale}
          onItemClick={() => onItemClick(feedChange.unique_name)}
          onCategoryClick={() => onCategoryClick(feedItem?.category ?? "Miscellaneous")}
          onFeedExpand={() => onExpandedChange(true)}
          feed
        />
      </div>}

      {expanded && (
        <div className="log-expanded-body">
          <div
            className="log-resize-edge"
            role="separator"
            tabIndex={0}
            aria-label="Change log height"
            aria-controls="change-log-list"
            aria-orientation="horizontal"
            aria-valuenow={height}
            aria-valuemin={100}
            aria-valuemax={Math.max(140, Math.min(800, window.innerHeight * 0.8))}
            onKeyDown={event => {
              const maxHeight = Math.max(140, Math.min(800, window.innerHeight * 0.8));
              const next = event.key === "ArrowUp" ? height + 20
                : event.key === "ArrowDown" ? height - 20
                : event.key === "Home" ? 100
                : event.key === "End" ? maxHeight
                : null;
              if (next !== null) {
                event.preventDefault();
                onHeightChange(Math.max(100, Math.min(maxHeight, next)));
              }
            }}
            onMouseDown={event => {
              event.preventDefault();
              event.stopPropagation();
              const startY = event.clientY;
              const startHeight = height;
              const scale = parseFloat(getComputedStyle(document.documentElement).getPropertyValue("--ff-scale")) || 1;
              document.body.style.userSelect = "none";
              const onMove = (moveEvent: MouseEvent) => {
                const maxHeight = Math.max(140, Math.min(800, window.innerHeight * 0.8));
                const nextHeight = Math.max(100, Math.min(maxHeight, startHeight + (startY - moveEvent.clientY) / scale));
                resizeHeightRef.current = nextHeight;
                if (resizeFrameRef.current === null) {
                  resizeFrameRef.current = window.requestAnimationFrame(() => {
                    onHeightChange(resizeHeightRef.current);
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
                onHeightChange(resizeHeightRef.current);
                document.body.style.userSelect = "";
              };
              window.addEventListener("mousemove", onMove);
              window.addEventListener("mouseup", onUp);
            }}
          />
          <div className="log-list" id="change-log-list">
            {visibleChanges.length === 0 ? (
              <span className="log-empty">No changes recorded yet.</span>
            ) : visibleChanges.map((change, index) => {
              const item = catalogById.get(change.unique_name);
              const key = change.id || index;
              const arriving = arrivingEntryKeys.has(changeKey(change));
              const row = (
                <ChangeRow
                  change={change}
                  item={item}
                  clockFormat={clockFormat}
                  systemLocale={systemLocale}
                  onItemClick={() => onItemClick(change.unique_name)}
                  onCategoryClick={() => onCategoryClick(item?.category ?? "Miscellaneous")}
                  timeBreak={index > 0 && visibleChanges[index - 1].timestamp - change.timestamp > CHANGE_BATCH_GAP_SECONDS}
                />
              );
              return arriving
                ? <div className="log-row-arrival-wrap" key={key}>{row}</div>
                : <Fragment key={key}>{row}</Fragment>;
            })}
          </div>
        </div>
      )}
    </div>
  );
}
