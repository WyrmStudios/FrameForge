import { memo, type CSSProperties } from "react";
import ItemImg from "../ItemImg";
import type { ViewMode } from "../types/ui";
import { fmt, deltaClass, deltaText } from "../utils";
import { openWiki } from "../lib/wiki";


// ─── Presentatie ────────────────────────────────────────────────────────────

const ITEM_GRID_CLASS: Record<ViewMode, string> = {
  cards:
    "item-grid item-grid-cards flex-1 overflow-y-auto grid w-full content-start items-stretch justify-items-stretch px-2.5 py-4 max-w-[var(--inventory-grid-max-width)] mx-auto grid-cols-[repeat(auto-fit,minmax(min(var(--inventory-card-min-width,168px),100%),1fr))] gap-3",
  icons:
    "item-grid item-grid-icons flex-1 overflow-y-auto grid w-full content-start items-stretch justify-items-stretch px-2.5 py-4 max-w-none mx-0 grid-cols-[repeat(auto-fill,76px)] gap-1.5",
  "text-cards":
    "item-grid item-grid-text-cards flex-1 overflow-y-auto grid w-full content-start items-stretch justify-items-stretch px-2.5 py-4 max-w-[var(--inventory-grid-max-width)] mx-auto grid-cols-[repeat(auto-fit,minmax(min(var(--inventory-card-min-width,168px),100%),1fr))] gap-3",
  list:
    "item-grid item-grid-list flex-1 overflow-y-auto flex w-full content-start items-stretch justify-items-stretch max-w-none mx-0 flex-col gap-px px-0 py-1 text-[length:var(--inventory-list-base-size,13px)]",
  "list-compact":
    "item-grid item-grid-list-compact flex-1 overflow-y-auto flex w-full content-start items-stretch justify-items-stretch max-w-none mx-0 flex-col gap-px px-0 py-1 text-[length:var(--inventory-list-base-size,13px)]",
};

const INV_SKELETON =
  "min-h-30 bg-[linear-gradient(90deg,var(--color-surface)_25%,rgba(255,255,255,.04)_50%,var(--color-surface)_75%)] bg-[length:200%_100%] animate-skeleton-shimmer";

const EMPTY_MSG = "px-6 py-10 text-center leading-1.6 text-muted";

const INV_CARD_BASE =
  "relative flex w-full min-w-0 cursor-default flex-col items-center gap-1.25 self-stretch rounded-9 border border-border bg-surface px-2.5 pt-2.5 pb-3 transition-[border-color] duration-120 hover:border-accent/50";
const INV_CARD_BASE_EM =
  "relative flex w-full min-w-0 cursor-default flex-col gap-[.385em] self-stretch rounded-9 border border-border bg-surface text-[length:var(--inventory-card-base-size,13px)] transition-[border-color] duration-120 hover:border-accent/50";
const INV_CARD_PAD_EM = "px-[.769em] pt-[.769em] pb-[.923em]";
const INV_CARD_PAD_EM_TEXT = "px-[.769em] pt-[3em] pb-[.923em]";
const INV_CARD_PAD_MOD = "px-[.923em] pt-[2.154em] pb-[.923em]";
const INV_CARD_PAD_MOD_TEXT = "px-[.923em] pt-[3em] pb-[.923em]";
const INV_ICON_CELL =
  "relative flex min-w-0 cursor-pointer flex-col items-center justify-center gap-1.25 self-stretch h-19 w-19 rounded-8 border border-border bg-surface p-1.5 transition-[border-color] duration-120 hover:border-accent/50 " +
  "[&_.img]:h-14! [&_.img]:w-14!";
const INV_FAV_STAR =
  "absolute left-[.538em] top-[.462em] z-2 cursor-pointer border-0 bg-transparent p-0 text-[1.077em] leading-none transition-colors duration-100";
const INV_FAV_STAR_ON = "text-ducat";
const INV_FAV_STAR_OFF = "text-white/25 hover:text-ducat/80";
const INV_MASTERY_ROW = "flex h-[1.538em] w-full items-center justify-center";
const INV_MASTERY_STAR = "text-[1.077em] leading-none text-ducat";
const INV_MASTERY_RANK =
  "rounded-3 bg-white/6 px-[.462em] py-[.154em] text-[.846em] font-semibold text-muted";
const INV_CARD_IMG_WRAP =
  "inv-card-img-wrap relative flex h-[var(--inventory-card-image-size,56px)] w-[var(--inventory-card-image-size,56px)] shrink-0 items-center justify-center " +
  "[&_.img]:h-[var(--inventory-card-image-size,56px)]! [&_.img]:w-[var(--inventory-card-image-size,56px)]! " +
  "[&_.img-fallback]:h-[var(--inventory-card-image-size,56px)]! [&_.img-fallback]:w-[var(--inventory-card-image-size,56px)]! " +
  "[&_.img-fallback]:text-[1.508em]!";
const INV_FOUNDRY_ICON =
  "absolute right-[-.538em] top-[-.385em] text-[1em] drop-shadow-[0_0_3px_rgba(0,0,0,0.9)]";
const INV_CARD_NAME =
  "line-clamp-2 w-full overflow-hidden text-center text-[1em] font-medium leading-1.35";
const INV_CARD_CAT =
  "-mt-0.25 w-full overflow-hidden text-ellipsis whitespace-nowrap text-center text-[.769em] font-semibold uppercase tracking-0.04 text-muted/60";
const INV_CARD_NAME_MOD =
  "line-clamp-2 w-full overflow-hidden text-left text-[1em] font-medium leading-1.35";
const INV_CARD_CAT_MOD =
  "-mt-0.25 w-full overflow-hidden text-ellipsis whitespace-nowrap text-left text-[.769em] font-semibold uppercase tracking-0.04 text-muted/60";
const INV_CARD_IMG_WRAP_MOD =
  "inv-card-img-wrap relative flex h-[var(--inventory-mod-image-size,48px)] w-[var(--inventory-mod-image-size,48px)] shrink-0 items-center justify-center " +
  "[&_.img]:h-[var(--inventory-mod-image-size,48px)]! [&_.img]:w-[var(--inventory-mod-image-size,48px)]! " +
  "[&_.img-fallback]:h-[var(--inventory-mod-image-size,48px)]! [&_.img-fallback]:w-[var(--inventory-mod-image-size,48px)]! " +
  "[&_.img-fallback]:text-[1.292em]!";
const INV_MOD_TOTAL =
  "inv-card-qty mod-total flex w-full items-center justify-between gap-[.313em] border-t border-border mt-[.462em] pt-[.385em] text-[1em] font-bold tabular-nums text-foreground " +
  "before:content-['Total'] before:text-[0.846em] before:font-semibold before:text-muted";
const INV_ITEM_UPDATED = "item-updated text-[.846em] text-muted whitespace-nowrap";
const INV_CARD_SIDE =
  "absolute top-[.462em] right-[.538em] z-2 flex flex-col items-end gap-[.308em]";
const INV_CARD_SIDE_ROW =
  "absolute top-[.462em] right-[.538em] z-2 flex flex-row items-center gap-[.308em]";
const INV_WIKI_BTN =
  "cursor-pointer rounded-4 border border-accent/40 bg-black/40 px-[.6em] py-[.3em] text-[.769em] font-bold leading-1.3 text-wiki-link transition-[background] duration-100 hover:bg-accent/25 hover:text-wiki-link-hover";
const INV_ROW_ICON =
  "inv-row-icon relative flex h-[var(--inventory-list-icon-wrap-size,30px)] w-[var(--inventory-list-icon-wrap-size,30px)] shrink-0 items-center " +
  "[&_.img]:h-[var(--inventory-list-icon-size,28px)]! [&_.img]:w-[var(--inventory-list-icon-size,28px)]!";
const INV_ROW_NAME =
  "inv-row-name min-w-0 flex-1 text-[1em] font-medium text-foreground whitespace-nowrap overflow-hidden text-ellipsis";
const INV_ROW_CAT =
  "inv-row-cat w-[10em] shrink-0 text-[.846em] text-muted text-right uppercase tracking-0.03 max-[900px]:hidden";
const INV_ROW_QTY =
  "inv-row-qty flex w-[5em] shrink-0 items-center justify-end gap-1 text-right text-[1.077em] font-bold text-foreground max-[900px]:w-[4em]";
const INV_ROW_VALUES =
  "inv-row-values flex shrink-0 items-center justify-start gap-1 w-[7.385em] max-[900px]:w-auto";
const INV_FOUNDRY_ROW = "inv-foundry-icon-row absolute -right-1.5 -top-1 text-9";

function invRowFavClass(view: ViewMode, isFavorite: boolean): string {
  const size = view === "list" ? "text-14" : "text-11";
  const color = isFavorite
    ? "text-ducat"
    : "text-white/25 hover:text-ducat/80";
  return `inv-fav-star-row shrink-0 cursor-pointer border-0 bg-transparent p-0 leading-none transition-colors duration-100 ${size} ${color}`;
}

function invCardRowClass(view: ViewMode): string {
  return view === "list"
    ? "relative flex w-full min-w-0 flex-row items-center gap-3 px-4 py-[.615em] min-h-[3.385em] self-stretch border border-border border-b-border/35 bg-surface cursor-default transition-[border-color] duration-120 hover:border-accent/50 max-[900px]:gap-1.5 max-[900px]:px-2"
    : "relative flex w-full min-w-0 flex-row items-center gap-2 px-3 py-[.308em] min-h-[2.308em] self-stretch border border-border border-b-border/35 bg-surface cursor-default transition-[border-color] duration-120 hover:border-accent/50 max-[900px]:gap-1.5 max-[900px]:px-2";
}

function invWikiRowClass(view: ViewMode): string {
  return view === "list"
    ? "inv-wiki-row shrink-0 cursor-pointer border border-accent/30 bg-transparent px-1.75 py-1 text-[.769em] font-bold leading-1.3 text-wiki-link transition-all duration-100 hover:bg-accent/20 hover:text-wiki-link-hover"
    : "inv-wiki-row shrink-0 cursor-pointer border border-accent/30 bg-transparent px-1.25 py-0.5 text-[.769em] font-bold leading-1.3 text-wiki-link transition-all duration-100 hover:bg-accent/20 hover:text-wiki-link-hover";
}

function invCardQtyClass(isZero: boolean): string {
  return `inv-card-qty flex items-center gap-[.313em] text-[1.231em] font-bold tabular-nums ${isZero ? "text-muted" : "text-foreground"}`;
}

function invItemDeltaClass(view: ViewMode, d: number): string {
  const size = view === "list" || view === "list-compact"
    ? "text-[.857em] px-1.5 py-0.25"
    : "text-[.75em] px-[.375em] py-[.063em]";
  const tone = d > 0
    ? "text-success bg-success/12"
    : "text-danger bg-danger/12";
  return `item-delta ${deltaClass(d)} shrink-0 rounded-4 font-semibold ${size} ${tone}`;
}

// ─── Types ──────────────────────────────────────────────────────────────────


export interface InventoryGridItem {
  unique_name: string;
  name: string;
  category: string;
  image_name?: string | null;
  qty: number;
  plat?: number | null;
  ducats?: number | null;
}

interface InventoryGridProps {
  items: InventoryGridItem[];
  loading: boolean;
  monitoring: boolean;
  view: ViewMode;
  cardColumns: number;
  listTextScale: number;
  inventory: Record<string, { mastery_rank: number }>;
  modCopies: Record<string, { rank: number | null; count: number }[]>;
  favorites: Set<string>;
  lastChanged: Record<string, number>;
  changes: Map<string, { delta: number; rank?: number | null; timestamp: number }[]>;
  crafting: Map<string, { item_name: string }>;
  filterRank: number | "unranked" | null;
  onToggleFavorite: (id: string) => void;
  onContextMenu?: (e: React.MouseEvent) => void;
}

// ─── Value + wiki helpers ─────────────────────────────────────────────────────

function PlatIcon({ size = 11 }: { size?: number }) {
  return <img src="/platinum.webp" alt="plat" width={size} height={size} className="inline-block shrink-0 object-contain" />;
}
function DucatIcon({ size = 11 }: { size?: number }) {
  return <img src="/ducats.webp" alt="ducat" width={size} height={size} className="inline-block shrink-0 object-contain" />;
}

function valueTitle(plat: number | null, ducats: number | null | undefined): string {
  const parts: string[] = [];
  if (plat != null) parts.push(`${fmt(plat)}p`);
  if (ducats != null && ducats > 0) parts.push(`${fmt(ducats)} ducats`);
  return parts.length ? ` · ${parts.join(" · ")}` : "";
}

function PriceChip({ kind, value }: { kind: "plat" | "ducat"; value: number }) {
  return kind === "plat" ? (
    <span className="inline-flex items-center gap-0.5 rounded-4 bg-price-platinum/10 px-[.091em] py-[.455em] text-[.846em] font-bold leading-1.4 text-price-platinum tabular-nums" title={`Market: ${fmt(value)} plat`}><PlatIcon />{fmt(value)}</span>
  ) : (
    <span className="inline-flex items-center gap-0.5 rounded-4 bg-ducat/10 px-[.091em] py-[.455em] text-[.846em] font-bold leading-1.4 text-ducat tabular-nums" title={`Ducats: ${fmt(value)}`}><DucatIcon />{fmt(value)}</span>
  );
}

function ValueChips({ plat, ducats, className }: {
  plat: number | null;
  ducats: number | null | undefined;
  className: string;
}) {
  // Always render the container so neighbouring rows keep identical column
  // positions whether or not this item has values.
  const showDucats = ducats != null && ducats > 0;
  return (
    <div className={className}>
      {plat != null && <PriceChip kind="plat" value={plat} />}
      {showDucats && <PriceChip kind="ducat" value={ducats!} />}
    </div>
  );
}

// Card view: wiki with plat/ducats stacked underneath, pinned to the top-right
// corner so the content flow (name/cat/rank/qty) is identical on every card.
function CardSide({ name, plat, ducats, horizontal }: {
  name: string;
  plat: number | null;
  ducats: number | null | undefined;
  horizontal: boolean;
}) {
  const showDucats = ducats != null && ducats > 0;
  return (
    <div className={horizontal ? INV_CARD_SIDE_ROW : INV_CARD_SIDE}>
      <WikiButton name={name} className={INV_WIKI_BTN} />
      {plat != null && <PriceChip kind="plat" value={plat} />}
      {showDucats && <PriceChip kind="ducat" value={ducats!} />}
    </div>
  );
}

function WikiButton({ name, className }: { name: string; className: string }) {
  return (
    <button className={className} title="Open wiki"
      onClick={e => { e.stopPropagation(); openWiki(name); }}>wiki</button>
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
  plat: number | null;
  ducats: number | null;
  view: ViewMode;
  changedAt?: number;
  recentDelta?: number | null;
  rankDeltas?: { rank: number; delta: number }[];
  isFavorite: boolean;
  onToggleFavorite: (id: string) => void;
}
const InvModCard = memo(function InvModCard({ unique_name, name, category, image_name, ranks, total, plat, ducats, view, changedAt, recentDelta, rankDeltas, isFavorite, onToggleFavorite }: InvModCardProps) {
  const nowSec = Date.now() / 1000;
  const secAgo = changedAt != null ? nowSec - changedAt : null;
  const isRecent = secAgo !== null && secAgo < 300;
  const hasPositive = rankDeltas != null && rankDeltas.some(d => d.delta > 0);
  const hasNegative = rankDeltas != null && rankDeltas.some(d => d.delta < 0);
  const mixedChange = isRecent && hasPositive && hasNegative;
  const baseClass = `inv-card${isRecent ? (mixedChange ? " inv-card-mixed border-l-2 border-l-inv-mixed bg-inv-mixed/4!" : (recentDelta != null && recentDelta > 0 ? " inv-card-gained border-l-2 border-l-success bg-success/4!" : " inv-card-lost border-l-2 border-l-danger bg-danger/4!")) : ""}`;

  if (view === "icons") {
    return (
      <div key={unique_name} className={`${baseClass} inv-card-icon-only ${INV_ICON_CELL}`} role="img"
        aria-label={`${name} ×${fmt(total)}${valueTitle(plat, ducats)}`}
        title={`${name} ×${fmt(total)}${valueTitle(plat, ducats)}`}>
        <ItemImg imageName={image_name ?? undefined} category={category} size={52} />
      </div>
    );
  }
  if (view === "list" || view === "list-compact") {
    return (
      <div key={unique_name} className={`${baseClass} inv-card-row ${invCardRowClass(view)}`}>
        <button className={invRowFavClass(view, isFavorite)}
          title={isFavorite ? "Remove from Modular Window" : "Add to Modular Window"}
          aria-label={isFavorite ? "Remove from Modular Window" : "Add to Modular Window"}
          onClick={e => { e.stopPropagation(); onToggleFavorite(unique_name); }}>
          {isFavorite ? "★" : "☆"}
        </button>
        {view === "list" && <div className={INV_ROW_ICON}><ItemImg imageName={image_name ?? undefined} category={category} size={28} /></div>}
        <div className={INV_ROW_NAME}>{name}</div>
        <ValueChips plat={plat} ducats={ducats} className={INV_ROW_VALUES} />
        <div className={INV_ROW_CAT}>{category}</div>
        <div className={INV_ROW_QTY}>{fmt(total)}</div>
        <WikiButton name={name} className={invWikiRowClass(view)} />
      </div>
    );
  }
  return (
    <div key={unique_name} className={`${baseClass} inv-card-mod ${INV_CARD_BASE_EM} items-start ${view === "text-cards" ? INV_CARD_PAD_MOD_TEXT : INV_CARD_PAD_MOD}`}>
      <button
        className={[INV_FAV_STAR, isFavorite ? INV_FAV_STAR_ON : INV_FAV_STAR_OFF].join(" ")}
        title={isFavorite ? "Remove from Modular Window" : "Add to Modular Window"}
        aria-label={isFavorite ? "Remove from Modular Window" : "Add to Modular Window"}
        onClick={e => { e.stopPropagation(); onToggleFavorite(unique_name); }}
      >{isFavorite ? "★" : "☆"}</button>
      {view !== "text-cards" && (
        <div className={INV_CARD_IMG_WRAP_MOD}>
          <ItemImg imageName={image_name ?? undefined} category={category} size={48} />
        </div>
      )}
      <div className={`inv-card-name ${INV_CARD_NAME_MOD}`}>{name}</div>
      <div className={INV_CARD_CAT_MOD}>{category}</div>
      <div className="mod-rank-table grid w-full grid-cols-[1fr_auto] gap-[.231em_.615em] mt-[.538em]">
        {ranks.map(r => {
          const rankDelta = rankDeltas?.find(rd => rd.rank === r.rank);
          return (
            <div key={r.rank} className={`mod-rank-row contents${r.count === 0 ? " mod-rank-zero" : ""}`}>
              <span className={r.count === 0 ? "mod-rank-label text-[.846em] font-semibold tabular-nums text-muted/30" : "mod-rank-label text-[.846em] font-semibold tabular-nums text-muted"}>R{r.rank}</span>
              <span className="mod-rank-value flex items-center justify-end gap-[.308em]">
                <span className={r.count === 0 ? "mod-rank-count text-[.846em] font-bold tabular-nums text-right text-muted/30" : "mod-rank-count text-[.846em] font-bold tabular-nums text-right text-foreground"}>{r.count}</span>
                {isRecent && rankDelta && (
                  <span className={`mod-rank-delta ${rankDelta.delta > 0 ? "text-success bg-success/12" : "text-danger bg-danger/12"} text-[.846em] font-semibold px-[.231em] rounded-2`}>
                    {rankDelta.delta > 0 ? `+${rankDelta.delta}` : rankDelta.delta}
                  </span>
                )}
              </span>
            </div>
          );
        })}
      </div>
      <div className={INV_MOD_TOTAL}>{fmt(total)}</div>
      <CardSide name={name} plat={plat} ducats={ducats} horizontal={view === "text-cards"} />
    </div>
  );
}, (prev, next) =>
  prev.view === next.view &&
  prev.unique_name === next.unique_name &&
  prev.name === next.name &&
  prev.total === next.total &&
  prev.plat === next.plat &&
  prev.ducats === next.ducats &&
  prev.isFavorite === next.isFavorite &&
  prev.image_name === next.image_name &&
  prev.ranks.length === next.ranks.length &&
  prev.ranks.every((r, i) => r.rank === next.ranks[i].rank && r.count === next.ranks[i].count) &&
  prev.changedAt === next.changedAt &&
  prev.recentDelta === next.recentDelta &&
  prev.rankDeltas === next.rankDeltas
);

interface InvCardProps {
  unique_name: string;
  name: string;
  category: string;
  image_name?: string | null;
  qty: number;
  plat: number | null;
  ducats: number | null;
  isFavorite: boolean;
  changedAt: number | undefined;
  recentDelta: number | null;
  craftJobName: string | null;
  masteryRank: number | undefined;
  onToggleFavorite: (id: string) => void;
  view: ViewMode;
}
const InvCard = memo(function InvCard({
  unique_name, name, category, image_name, qty, plat, ducats,
  isFavorite, changedAt, recentDelta, craftJobName, masteryRank, onToggleFavorite, view,
}: InvCardProps) {
  const nowSec = Date.now() / 1000;
  const secAgo = changedAt != null ? nowSec - changedAt : null;
  const isRecent = secAgo !== null && secAgo < 300;
  const isZero = qty === 0 && !craftJobName;
  const isMastered = masteryRank != null && masteryRank >= 30;
  const showRank = masteryRank != null && masteryRank > 0;
  const recentLabel = secAgo !== null ? (Math.floor(secAgo / 60) === 0 ? "· now" : `· ${Math.floor(secAgo / 60)}m`) : null;
  const baseClass = `inv-card${isZero ? " inv-card-zero opacity-35" : ""}${isRecent ? (recentDelta != null && recentDelta > 0 ? " inv-card-gained border-l-2 border-l-success bg-success/4!" : " inv-card-lost border-l-2 border-l-danger bg-danger/4!") : ""}`;

  if (view === "icons") {
    return (
      <div className={`${baseClass} inv-card-icon-only ${INV_ICON_CELL}`} role="img"
        aria-label={`${name} (${fmt(qty)})${valueTitle(plat, ducats)}`}
        title={`${name} (${fmt(qty)})${valueTitle(plat, ducats)}`}>
        <ItemImg imageName={image_name ?? undefined} category={category} size={52} />
      </div>
    );
  }
  if (view === "list" || view === "list-compact") {
    return (
      <div className={`${baseClass} inv-card-row ${invCardRowClass(view)}`}>
        <button className={invRowFavClass(view, isFavorite)}
          title={isFavorite ? "Remove from Modular Window" : "Add to Modular Window"}
          aria-label={isFavorite ? "Remove from Modular Window" : "Add to Modular Window"}
          onClick={e => { e.stopPropagation(); onToggleFavorite(unique_name); }}>
          {isFavorite ? "★" : "☆"}
        </button>
        {view === "list" && (
          <div className={INV_ROW_ICON}>
            <ItemImg imageName={image_name ?? undefined} category={category} size={28} />
            {craftJobName && <span className={INV_FOUNDRY_ROW} title={`Building — ${craftJobName}`}>⚒</span>}
          </div>
        )}
        <div className={INV_ROW_NAME}>
          {name}
          {isRecent && <span className={INV_ITEM_UPDATED}>{recentLabel}</span>}
        </div>
        <ValueChips plat={plat} ducats={ducats} className={INV_ROW_VALUES} />
        <div className={INV_ROW_CAT}>{category}</div>
        <div className={INV_ROW_QTY}>
          {fmt(qty)}
          {isRecent && recentDelta != null && <span className={invItemDeltaClass(view, recentDelta)}>{deltaText(recentDelta)}</span>}
        </div>
        <WikiButton name={name} className={invWikiRowClass(view)} />
      </div>
    );
  }
  return (
    <div className={`${baseClass} ${INV_CARD_BASE_EM} items-center ${view === "text-cards" ? INV_CARD_PAD_EM_TEXT : INV_CARD_PAD_EM}`}>
      <button
        className={[INV_FAV_STAR, isFavorite ? INV_FAV_STAR_ON : INV_FAV_STAR_OFF].join(" ")}
        title={isFavorite ? "Remove from Modular Window" : "Add to Modular Window"}
        aria-label={isFavorite ? "Remove from Modular Window" : "Add to Modular Window"}
        onClick={e => { e.stopPropagation(); onToggleFavorite(unique_name); }}
      >{isFavorite ? "★" : "☆"}</button>
      <div className={view === "text-cards" && !showRank ? "hidden" : INV_MASTERY_ROW}>
        {isMastered
          ? <span className={INV_MASTERY_STAR} title="Mastered">★</span>
          : showRank
            ? <span className={INV_MASTERY_RANK} title={`Rank ${masteryRank}`}>R{masteryRank}</span>
            : null}
      </div>
      {view !== "text-cards" && (
        <div className={INV_CARD_IMG_WRAP}>
          <ItemImg imageName={image_name ?? undefined} category={category} size={56} />
          {craftJobName && <span className={INV_FOUNDRY_ICON} title={`Building — ${craftJobName}`}>⚒</span>}
        </div>
      )}
      <div className={`inv-card-name ${INV_CARD_NAME}`}>
        {name}
        {isRecent && <span className={INV_ITEM_UPDATED}>{recentLabel}</span>}
      </div>
      <div className={INV_CARD_CAT}>{category}</div>
      <div className={invCardQtyClass(isZero)}>
        {fmt(qty)}
        {isRecent && recentDelta != null && (
          <span className={invItemDeltaClass(view, recentDelta)}>{deltaText(recentDelta)}</span>
        )}
      </div>
      <CardSide name={name} plat={plat} ducats={ducats} horizontal={view === "text-cards"} />
    </div>
  );
}, (prev, next) => {
  if (prev.view !== next.view) return false;
  if (
    prev.unique_name !== next.unique_name ||
    prev.qty !== next.qty ||
    prev.plat !== next.plat ||
    prev.ducats !== next.ducats ||
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

// ─── Main grid component ────────────────────────────────────────────────────

export default memo(function InventoryGrid({
  items, loading, monitoring, view, cardColumns, listTextScale,
  inventory, modCopies, favorites, lastChanged, changes, crafting,
  filterRank, onToggleFavorite, onContextMenu,
}: InventoryGridProps) {
  const cardScale = Math.max(.72, Math.min(1.08, 1 - (cardColumns - 9) * .0187));
  const scaledCardMinWidth = Math.round(168 - (cardColumns - 9) * (64 / 15));
  const cardMinWidth = view === "text-cards"
    ? Math.max(150, scaledCardMinWidth + 24)
    : scaledCardMinWidth;
  return (
    <div className={ITEM_GRID_CLASS[view]}
         style={{
           "--inventory-grid-max-width": `${cardColumns * (cardMinWidth + 12) + 20}px`,
           "--inventory-card-min-width": `${cardMinWidth}px`,
           "--inventory-card-base-size": `${13 * cardScale}px`,
           "--inventory-card-image-size": `${56 * cardScale}px`,
           "--inventory-mod-image-size": `${48 * cardScale}px`,
           "--inventory-list-base-size": `${13 * listTextScale / 100}px`,
           "--inventory-list-icon-wrap-size": `${30 * listTextScale / 100}px`,
           "--inventory-list-icon-size": `${28 * listTextScale / 100}px`,
         } as CSSProperties}
         onContextMenu={onContextMenu}>
      {loading ? (
        Array.from({ length: 20 }, (_, i) => (
          <div key={i} className={`inv-card ${INV_CARD_BASE} ${INV_SKELETON}`} />
        ))
      ) : items.length === 0 ? (
        <div className={`${EMPTY_MSG} col-span-full`}>
          {monitoring
            ? "No items found. Complete a mission or visit a relay to sync inventory."
            : "Start the monitor to begin tracking your inventory."}
        </div>
      ) : (
        items.flatMap(item => {
          // Mods & Arcanes: single card with inline rank breakdown
          if ((item.category === "Mods" || item.category === "Arcanes") && modCopies[item.unique_name]) {
            const copies = modCopies[item.unique_name];
            const byRank: Record<number, number> = {};
            for (const c of copies) byRank[c.rank ?? 0] = (byRank[c.rank ?? 0] ?? 0) + c.count;
            const changeEntries = lastChanged[item.unique_name] != null ? changes.get(item.unique_name) : undefined;
            const nowSec = Date.now() / 1000;
            const recentChanges = changeEntries?.filter(change => nowSec - change.timestamp < 300) ?? [];
            const rankDeltas = recentChanges.filter(c => c.rank != null).map(c => ({ rank: c.rank!, delta: c.delta }));
            const isRecent = lastChanged[item.unique_name] != null && nowSec - lastChanged[item.unique_name] < 300;
            const ranks = [...new Set([...Object.keys(byRank).map(Number), ...rankDeltas.map(delta => delta.rank)])]
              .sort((a, b) => a - b)
              .map(rank => ({ rank, count: byRank[rank] ?? 0 }))
              .filter(r => r.count > 0 || (isRecent && rankDeltas.some(delta => delta.rank === r.rank)));
            if (filterRank !== null) {
              const targetRank = filterRank === "unranked" ? 0 : filterRank;
              if ((byRank[targetRank] ?? 0) === 0) return [];
            }
            const total = Object.values(byRank).reduce((a, b) => a + b, 0);
            const totalDelta = recentChanges.find(c => c.rank == null)?.delta ?? rankDeltas.reduce((s, d) => s + d.delta, 0);
            return [(
              <InvModCard key={item.unique_name}
                unique_name={item.unique_name} name={item.name}
                category={item.category} image_name={item.image_name}
                ranks={ranks} total={total}
                plat={item.plat ?? null} ducats={item.ducats ?? null} view={view}
                isFavorite={favorites.has(item.unique_name)}
                onToggleFavorite={onToggleFavorite}
                changedAt={lastChanged[item.unique_name]}
                recentDelta={totalDelta || null}
                rankDeltas={rankDeltas} />
            )];
          }

          // Normal item card
          const changedAt = lastChanged[item.unique_name];
          const changeEntries = changedAt != null ? changes.get(item.unique_name) : undefined;
          const recentChange = changeEntries?.find(change => Date.now() / 1000 - change.timestamp < 300 && change.rank == null)
            ?? changeEntries?.find(change => Date.now() / 1000 - change.timestamp < 300);
          const craftJob = crafting.get(item.unique_name);
          return [(
            <InvCard key={item.unique_name}
              unique_name={item.unique_name} name={item.name}
              category={item.category} image_name={item.image_name}
              qty={item.qty}
              plat={item.plat ?? null} ducats={item.ducats ?? null}
              isFavorite={favorites.has(item.unique_name)}
              changedAt={changedAt}
              recentDelta={recentChange?.delta ?? null}
              craftJobName={craftJob?.item_name ?? null}
              masteryRank={inventory[item.unique_name]?.mastery_rank}
              onToggleFavorite={onToggleFavorite}
              view={view} />
          )];
        })
      )}
    </div>
  );
});
