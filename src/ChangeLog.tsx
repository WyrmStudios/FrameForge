import { Fragment, useEffect, useMemo, useRef, useState } from "react";
import ItemImg from "./ItemImg";
import { ResizeHandle } from "./shared/ResizeHandle";
import SearchBar from "./shared/SearchBar";
import { useContextMenu, CtxMenu } from "./shared/CtxMenu";
import { openWiki, copyWikiLink } from "./lib/wiki";
import { formatUnixTime } from "./lib/formatters";
import type { ClockFormat } from "./types/settings";
import type { ChangeLogEntry } from "./types/inventory";


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
  clockFormat: ClockFormat;
  systemLocale: string;
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
function changeKey(change: ChangeLogEntry) {
  return `${change.id}:${change.unique_name}:${change.timestamp}`;
}

// ─── ChangeLog classes (Tailwind) ─────────────────────────────────────────────
const CL_PANEL = "log-panel border-t border-border bg-surface flex flex-col shrink-0 h-8.75 max-h-8.75 overflow-hidden relative";
const CL_PANEL_ON = "shadow-[0_-8px_24px_#0000002e]";
const CL_HEADER = "log-header flex items-center gap-2 w-full min-h-8.5 py-1.5 px-3 bg-transparent border-b border-border text-muted cursor-pointer text-left text-11 font-semibold uppercase tracking-0.06 shrink-0 transition-[background,color] duration-150 motion-reduce:transition-none hover:bg-white/4 hover:text-foreground max-[600px]:px-2 max-[600px]:gap-1.25";
const CL_HEADER_TITLE = "log-header-title p-0 border-0 bg-transparent text-inherit cursor-pointer [font:inherit] tracking-[inherit] uppercase hover:text-foreground hover:underline focus-visible:text-foreground focus-visible:underline focus-visible:outline-none";
const CL_ARRIVAL = "log-header-arrival log-arrival-notice flex items-center gap-1.5 text-muted text-10 font-semibold normal-case tracking-normal whitespace-nowrap animate-log-arrival motion-reduce:animate-none";
const CL_DIVIDER = "log-status-divider text-muted text-11 font-normal";
const CL_LAST_SCAN = "log-last-scan text-muted text-10 font-normal normal-case tracking-normal whitespace-nowrap";
const CL_FEED = "log-feed relative z-1 self-end w-[min(680px,calc(100%_-_250px))] mr-3 overflow-hidden animate-log-feed motion-reduce:animate-none max-[600px]:mr-2 max-[480px]:hidden";
const CL_ROW = "inv-card inv-card-row log-item-row relative flex items-center gap-2 py-[.308em] px-3 min-h-[2.308em] w-full min-w-0 self-stretch rounded-none transition-[border-color] duration-120 max-[800px]:gap-1.25 max-[800px]:pl-2 max-[800px]:pr-2";
const CL_ROW_LIST = "border-t border-x border-b border-t-border border-x-border border-b-border/35 bg-surface cursor-default";
const CL_ROW_LIST_TB = "border-t-3 border-x border-b border-t-border border-x-border border-b-border/35 bg-surface cursor-default";
const CL_ROW_FEED = "bg-transparent cursor-pointer hover:bg-transparent";
const CL_FEED_OPEN = "log-feed-open absolute z-1 inset-0 border-0 bg-transparent cursor-pointer focus-visible:outline-2 focus-visible:outline-accent focus-visible:-outline-offset-2";
const CL_TIME = "log-time text-muted shrink-0 min-w-10 text-left text-11 max-[800px]:hidden";
const CL_ICON = "inv-row-icon w-5.5 h-5.5 shrink-0 relative flex items-center";
const CL_ROW_NAME = "inv-row-name log-name-group flex-1 min-w-0 text-[1em] font-medium text-foreground truncate";
const CL_NAME_LINK = "log-name-link min-w-0 overflow-hidden truncate p-0 border-0 bg-transparent text-foreground cursor-pointer [font:inherit] text-left hover:text-accent hover:underline";
const CL_RANK = "log-rank-badge text-9 font-bold shrink-0 py-0.25 px-1 rounded-3 text-accent bg-info/12";
const CL_CAT = "log-cat log-category-link max-w-25 truncate text-muted text-9 font-semibold uppercase tracking-0.03 p-0 border-0 bg-transparent cursor-pointer [font-family:inherit] hover:text-accent hover:underline focus-visible:text-accent focus-visible:underline focus-visible:outline-none max-[800px]:max-w-18";
const CL_QTY = "inv-row-qty log-change-qty shrink-0 text-right text-[1.077em] font-bold text-foreground flex items-center justify-end gap-1.25 min-w-40 max-[800px]:min-w-32 max-[600px]:min-w-0";
const CL_RANGE = "log-range text-muted shrink-0 whitespace-nowrap text-11 font-normal max-[600px]:max-w-24 max-[600px]:truncate";
const CL_DIRECTION = "log-direction text-14 font-bold leading-none";
const CL_AMOUNT = "log-amount text-11 font-bold";
const CL_BODY = "log-expanded-body flex flex-col min-h-0 flex-1";
const CL_RESIZE = "log-resize-edge absolute z-2 top-0 left-0 right-0 h-2 cursor-ns-resize hover:bg-accent/28";
const CL_SEARCH = "log-search flex py-1.5 px-3 border-b border-border shrink-0";
const CL_SEARCH_LABEL = "log-search-label sr-only";
const CL_LIST = "log-list flex-1 min-h-0 overflow-y-auto";
const CL_EMPTY = "log-empty block py-3 px-4 text-muted text-12";
const CL_ARRIVAL_WRAP = "log-row-arrival-wrap grid grid-rows-[minmax(0,0fr)] overflow-hidden opacity-0 animate-log-row motion-reduce:animate-none motion-reduce:grid-rows-[minmax(0,1fr)] motion-reduce:opacity-100 [&>*]:min-h-7.5";

function ChangeRow({
  change, item, clockFormat, systemLocale, onItemClick, onCategoryClick, onFeedExpand, onContextMenu, timeBreak = false, feed = false,
}: {
  change: ChangeLogEntry;
  item?: ChangeLogCatalogItem;
  clockFormat: ChangeLogProps["clockFormat"];
  systemLocale: string;
  onItemClick: () => void;
  onCategoryClick: (category: string) => void;
  onFeedExpand?: () => void;
  onContextMenu?: (e: React.MouseEvent) => void;
  timeBreak?: boolean;
  feed?: boolean;
}) {
  const name = item?.name ?? change.item_name;
  const category = item?.category ?? "Miscellaneous";
  return (
    <div
      className={`${CL_ROW}${feed ? " log-feed-row" : ""}${timeBreak ? " log-time-break" : ""} ${feed ? CL_ROW_FEED : timeBreak ? CL_ROW_LIST_TB : CL_ROW_LIST}`}
      onContextMenu={onContextMenu}
    >
      {onFeedExpand && <button className={CL_FEED_OPEN} onClick={onFeedExpand} aria-label="Open change log" />}
      <span className={CL_TIME}>{formatUnixTime(change.timestamp, clockFormat, systemLocale)}</span>
      <div className={CL_ICON}>
        <ItemImg imageName={item?.image_name} category={category} size={20} />
      </div>
      <div className={`${CL_ROW_NAME}${feed ? " relative z-2" : ""}`}>
        <button className={CL_NAME_LINK} onClick={e => { e.stopPropagation(); onItemClick(); }}>{name}</button>
        {change.rank != null && <span className={CL_RANK}>R{change.rank}</span>}
        <button className={CL_CAT} onClick={e => { e.stopPropagation(); onCategoryClick(category); }}>{category}</button>
      </div>
      <div className={`${CL_QTY}${feed ? " relative z-2 ml-auto" : ""}`}>
        <span className={CL_RANGE}>{fmt(change.old_qty)} → {fmt(change.new_qty)}</span>
        <span className={`${CL_DIRECTION} ${change.delta > 0 ? "text-success" : "text-danger"}`} aria-label={change.delta > 0 ? "Increased" : "Decreased"}>
          {change.delta > 0 ? "↑" : "↓"}
        </span>
        <span className={`${CL_AMOUNT} ${change.delta > 0 ? "text-success" : "text-danger"}`}>{deltaText(change.delta)}</span>
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
    <div className={`${CL_HEADER} ${expanded ? "relative" : "absolute inset-0"}`} onClick={() => onExpandedChange(!expanded)}>
      <button className={CL_HEADER_TITLE} aria-label="Show recent inventory changes" onClick={event => { event.stopPropagation(); onChangeLogClick(); }}>Changelog</button>
      {showArrival && <span className={CL_ARRIVAL} role="status" key={arrivalToken}>
        {positiveChanges > 0 && <span className="text-success">+{positiveChanges}</span>}
        {negativeChanges > 0 && <span className="text-danger">-{negativeChanges}</span>}
      </span>}
      <span className={CL_DIVIDER} aria-hidden="true">·</span>
      <span className={CL_LAST_SCAN}>last scan {lastScanAt == null ? "not yet" : formatUnixTime(lastScanAt, clockFormat, systemLocale)}</span>
    </div>
  );
}

export default function ChangeLog({
  changes, catalog, clockFormat, systemLocale,
  arrivalToken, lastScanAt, onItemClick, onChangeLogClick, onCategoryClick,
}: ChangeLogProps) {
  const [expanded, onExpandedChange] = useState(false);
  const [height, onHeightChange] = useState(270);
  const handledArrivalRef = useRef(0);
  const [showArrival, setShowArrival] = useState(false);
  const [feedIndex, setFeedIndex] = useState<number | null>(null);
  const [arrivingEntryKeys, setArrivingEntryKeys] = useState<Set<string>>(new Set());
  const [search, setSearch] = useState("");
  const catalogById = useMemo(() => new Map(catalog.map(item => [item.unique_name, item])), [catalog]);
  const latestBatch = useMemo(() => getLatestChangeBatch(changes), [changes]);
  const { ctxMenu, open: openCtx, close: closeCtx } = useContextMenu();

  const handleContextMenu = (e: React.MouseEvent) => {
    const card = (e.target as HTMLElement).closest(".inv-card");
    if (!card) return;
    const nameEl = card.querySelector(".log-name-link");
    const name = nameEl?.textContent?.trim();
    if (name) {
      e.preventDefault();
      openCtx(e.clientX, e.clientY, [
        { label: "Open Wiki", action: () => openWiki(name) },
        { label: "Copy Wiki Link", action: () => copyWikiLink(name) },
      ]);
    }
  };
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
      className={`${CL_PANEL}${expanded ? ` ${CL_PANEL_ON}` : ""}`}
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
        className={CL_FEED}
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
          onContextMenu={handleContextMenu}
          feed
        />
      </div>}

      {expanded && (
        <div className={CL_BODY}>
          <ResizeHandle className={CL_RESIZE} value={height} axis="y" direction={-1} clamp={clampLogHeight} onValueChange={onHeightChange} />
          <div className={CL_SEARCH}>
            <label className={CL_SEARCH_LABEL} htmlFor="change-log-search">Search changes</label>
            <SearchBar id="change-log-search" value={search} onChange={setSearch} placeholder="Search changes..." />
          </div>
          <div className={CL_LIST} id="change-log-list">
            {filteredChanges.length === 0 ? (
              <span className={CL_EMPTY}>{search ? "No matching changes." : "No changes recorded yet."}</span>
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
                  onContextMenu={handleContextMenu}
                  timeBreak={index > 0 && filteredChanges[index - 1].timestamp - change.timestamp > CHANGE_BATCH_GAP_SECONDS}
                />
              );
              return arriving
                ? <div className={CL_ARRIVAL_WRAP} key={key}>{row}</div>
                : <Fragment key={key}>{row}</Fragment>;
            })}
          </div>
        </div>
      )}
      {ctxMenu && <CtxMenu state={ctxMenu} onClose={closeCtx} />}
    </div>
  );
}
