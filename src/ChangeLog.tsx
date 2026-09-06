import { Fragment, useEffect, useMemo, useRef, useState } from "react";
import ItemImg from "./ItemImg";
import SearchBar from "./SearchBar";
import "./ChangeLog.css";

export const CHANGE_BATCH_GAP_SECONDS = 8;
const MIN_LOG_HEIGHT = 100;
const MAX_LOG_HEIGHT = 800;

function getScale() {
  return parseFloat(getComputedStyle(document.documentElement).getPropertyValue("--ff-scale")) || 1;
}

function getMinLogHeight() {
  return MIN_LOG_HEIGHT / getScale();
}

function getMaxLogHeight() {
  const scale = getScale();
  return Math.max(getMinLogHeight(), Math.min(MAX_LOG_HEIGHT / scale, window.innerHeight * 0.8 / scale));
}

function clampLogHeight(height: number) {
  return Math.max(getMinLogHeight(), Math.min(getMaxLogHeight(), height));
}

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
  changes: ChangeLogEntry[];
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

function getLatestChangeBatch(changes: ChangeLogEntry[]) {
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
  onCategoryClick: (category: string) => void;
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
        <button className="log-cat log-category-link" onClick={e => { e.stopPropagation(); onCategoryClick(category); }}>{category}</button>
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
    <div className="log-header" onClick={() => onExpandedChange(!expanded)}>
      <button className="log-header-title" aria-label="Show recent inventory changes" onClick={event => { event.stopPropagation(); onChangeLogClick(); }}>Changelog</button>
      {showArrival && <span className="log-header-arrival log-arrival-notice" role="status" key={arrivalToken}>
        {positiveChanges > 0 && <span className="log-positive">+{positiveChanges}</span>}
        {negativeChanges > 0 && <span className="log-negative">-{negativeChanges}</span>}
      </span>}
      <span className="log-status-divider" aria-hidden="true">·</span>
      <span className="log-last-scan">last scan {lastScanAt == null ? "not yet" : timeStr(lastScanAt, clockFormat, systemLocale)}</span>
    </div>
  );
}

function ChangeLogResizeHandle({ height, onHeightChange }: Pick<ChangeLogProps, "height" | "onHeightChange">) {
  const resizeFrameRef = useRef<number | null>(null);
  const resizeHeightRef = useRef(height);
  const resizeCleanupRef = useRef<(() => void) | null>(null);
  useEffect(() => () => resizeCleanupRef.current?.(), []);
  return (
    <div
      className="log-resize-edge"
      onMouseDown={event => {
        event.preventDefault();
        event.stopPropagation();
        resizeCleanupRef.current?.();
        resizeHeightRef.current = height;
        const startY = event.clientY;
        const startHeight = height;
        const scale = parseFloat(getComputedStyle(document.documentElement).getPropertyValue("--ff-scale")) || 1;
        document.body.style.userSelect = "none";
        const onMove = (moveEvent: MouseEvent) => {
          const nextHeight = clampLogHeight(startHeight + (startY - moveEvent.clientY) / scale);
          resizeHeightRef.current = nextHeight;
          if (resizeFrameRef.current === null) {
            resizeFrameRef.current = window.requestAnimationFrame(() => {
              onHeightChange(resizeHeightRef.current);
              resizeFrameRef.current = null;
            });
          }
        };
        const onUp = () => {
          onHeightChange(resizeHeightRef.current);
          cleanup();
        };
        const cleanup = () => {
          window.removeEventListener("mousemove", onMove);
          window.removeEventListener("mouseup", onUp);
          window.removeEventListener("blur", onUp);
          if (resizeFrameRef.current !== null) {
            window.cancelAnimationFrame(resizeFrameRef.current);
            resizeFrameRef.current = null;
          }
          document.body.style.userSelect = "";
          resizeCleanupRef.current = null;
        };
        window.addEventListener("mousemove", onMove);
        window.addEventListener("mouseup", onUp);
        window.addEventListener("blur", onUp);
        resizeCleanupRef.current = cleanup;
      }}
    />
  );
}

export default function ChangeLog({
  changes, catalog, clockFormat, systemLocale, expanded, height,
  arrivalToken, lastScanAt, onExpandedChange, onHeightChange, onItemClick, onChangeLogClick, onCategoryClick,
}: ChangeLogProps) {
  const handledArrivalRef = useRef(0);
  const [showArrival, setShowArrival] = useState(false);
  const [feedIndex, setFeedIndex] = useState<number | null>(null);
  const [arrivingEntryKeys, setArrivingEntryKeys] = useState<Set<string>>(new Set());
  const [search, setSearch] = useState("");
  const catalogById = useMemo(() => new Map(catalog.map(item => [item.unique_name, item])), [catalog]);
  const latestBatch = useMemo(() => getLatestChangeBatch(changes), [changes]);
  const filteredChanges = useMemo(() => {
    const query = search.trim().toLocaleLowerCase();
    if (!query) return changes;
    return changes.filter(change => {
      const item = catalogById.get(change.unique_name);
      const name = item?.name ?? change.item_name;
      const category = item?.category ?? "Miscellaneous";
      return name.toLocaleLowerCase().includes(query) || category.toLocaleLowerCase().includes(query);
    });
  }, [changes, catalogById, search]);
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

  let positiveChanges = 0;
  let negativeChanges = 0;
  for (const change of latestBatch) {
    if (change.delta > 0) positiveChanges++;
    else if (change.delta < 0) negativeChanges++;
  }
  const feedChange = feedIndex === null ? undefined : latestBatch[feedIndex];
  const feedItem = feedChange && catalogById.get(feedChange.unique_name);

  return (
    <div
      className={`log-panel${expanded ? " log-panel-expanded" : ""}`}
      style={{ height: expanded ? height : undefined, maxHeight: expanded ? getMaxLogHeight() : undefined }}
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
          onCategoryClick={onCategoryClick}
          onFeedExpand={() => onExpandedChange(true)}
          feed
        />
      </div>}

      {expanded && (
        <div className="log-expanded-body">
          <ChangeLogResizeHandle height={height} onHeightChange={onHeightChange} />
          <div className="log-search">
            <label className="log-search-label" htmlFor="change-log-search">Search changes</label>
            <SearchBar id="change-log-search" value={search} onChange={setSearch} placeholder="Search changes..." />
          </div>
          <div className="log-list" id="change-log-list">
            {filteredChanges.length === 0 ? (
              <span className="log-empty">{search ? "No matching changes." : "No changes recorded yet."}</span>
            ) : filteredChanges.map((change, index) => {
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
                  onCategoryClick={onCategoryClick}
                  timeBreak={index > 0 && filteredChanges[index - 1].timestamp - change.timestamp > CHANGE_BATCH_GAP_SECONDS}
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
