import { useState, useEffect, useMemo, useCallback, useRef, type Dispatch, type SetStateAction } from "react";
import { invoke } from "@tauri-apps/api/core";
import { HelpTip } from "./shared/HelpTip";
import FilterPresets from "./shared/FilterPresets";
import { SecondaryButton } from "./shared/ui/ActionButton";
import { EmptyMessage, FilterBar, FilterChip, FilterLabel, FilterSeparator, FoundrySearch } from "./shared/ui/FilterControls";
import { PREFERENCE_KEYS } from "./constants/preferences";
import { matchesSearchTerms, splitSearchTerms } from "./lib/search";
import { RELIC_DROP_RATES, RELIC_REFINEMENT_LABELS, RELIC_REFINEMENT_ORDER } from "./constants/relics";
import { warframeStatImageUrl } from "./constants/urls";
import { TAURI_COMMANDS } from "./constants/tauri";
import { useCatalog } from "./hooks/useCatalog";
import type { CatalogItem, InventoryItem } from "./types/items";
import type { RelicFilters } from "./types/filters";
import type { FilterPresetModule, FilterPresetSettings } from "./types/filterPresets";
import type { DropReward, RelicDrop } from "./types/relics";
import type { ViewMode } from "./types/ui";
import type { WfmCachedPrices, WfmItem } from "./types/market";
import { ViewToggle } from "./shared/ViewToggle";

interface Props {
  inventory: Record<string, InventoryItem>;
  colorblindMode?: boolean;
  filters: RelicFilters;
  onFiltersChange: Dispatch<SetStateAction<RelicFilters>>;
  filterPresets: FilterPresetSettings;
  onFilterPresetsChange: Dispatch<SetStateAction<FilterPresetSettings>>;
  onOpenSettings: (module: FilterPresetModule) => void;
}

// ─── Module-level constants ───────────────────────────────────────────────────

function toggle<T>(arr: T[], val: T): T[] {
  return arr.includes(val) ? arr.filter(x => x !== val) : [...arr, val];
}

const RARITY_SORT: Record<string, number> = { Common: 0, Uncommon: 1, Rare: 2 };
const RARITY_CSS:  Record<string, string> = { Common: "bronze", Uncommon: "silver", Rare: "gold" };

// Derive rarity from drop chance — more reliable than the WFCD rarity string
function chanceToRarity(chance: number): string {
  if (chance >= 15) return "Common";
  if (chance >= 5)  return "Uncommon";
  return "Rare";
}

// ─── Tailwind class constants ────────────────────────────────────────────────

type CardState = "none" | "unowned" | "complete";

const RL_RARITY_LABEL_COLOR: Record<string, string> = {
  bronze: "text-relic-bronze",
  silver: "text-rarity-silver",
  gold: "text-rarity-gold",
};
const PLANNER_RARITY_COLOR: Record<string, string> = {
  bronze: "text-rarity-bronze",
  silver: "text-rarity-silver",
  gold: "text-rarity-gold",
};
const RL_RBOX_TOP: Record<string, string> = {
  bronze: "border-t-4 border-t-rarity-bronze",
  silver: "border-t-4 border-t-rarity-silver",
  gold: "border-t-4 border-t-rarity-gold",
};
const RL_TEXT_REWARD_COLOR: Record<string, string> = {
  rare: "text-rarity-gold",
  uncommon: "text-rarity-silver",
  common: "text-muted",
};

const RL_CORNER = "absolute top-0.5 right-0.75 z-1 flex flex-col items-center gap-px";
const RL_RARITY_LABEL = "text-11 font-black leading-none tracking-[-.02em]";
const RL_CB_CHECK = "text-9 font-black leading-none tracking-[-.1em]";
const RL_CB_RELIC_CHECK = "text-11 font-black tracking-[-.1em] text-ducat";
const RL_RBOX =
  "flex flex-col items-center justify-center gap-1 px-1 py-1.5 relative border-r border-b border-r-border border-b-border overflow-hidden";
const RL_RBOX_EMPTY = `${RL_RBOX} opacity-25`;
const RL_RBOX_NAME = "w-full px-0.75 text-center text-9 leading-1.3 line-clamp-2";
const RL_CARD_LEFT =
  "flex flex-col gap-0.75 shrink-0 w-40 pl-3 pr-2.5 py-2.5 border-r border-r-border overflow-hidden";
const RL_CARD_LEFT_TEXT = "flex flex-col gap-0.75 shrink-0 w-30 p-2 border-r border-r-border overflow-hidden";
const RL_ICON_ROW = "flex items-center gap-2 shrink-0";
const RL_TOTAL = "text-18 font-bold text-foreground";
const RL_CARD_NAME = "shrink-0 text-12 font-semibold text-foreground leading-1.3";
const RL_REFINEMENTS = "flex flex-col gap-px";
const RL_REF = "text-10 whitespace-nowrap";
const RL_REWARDS_GRID = "grid grid-cols-3 grid-rows-[80px_80px] flex-1 overflow-hidden";
const RL_ICON_COUNT = "text-10 font-bold text-muted";
const RL_ROW_IMG = "shrink-0 [&_img]:size-6! [&_img]:object-contain!";
const RL_ROW_NAME =
  "flex-1 min-w-0 text-12 font-medium text-foreground whitespace-nowrap overflow-hidden text-ellipsis";
const RL_ROW_TOTAL = "shrink-0 text-12 font-bold text-muted";
const RL_ROW_REFS = "shrink-0 text-10 tracking-0.02 text-muted";
const RL_TEXT_REWARDS = "flex-1 min-w-0 px-2 py-1 flex flex-col justify-around";
const RL_TEXT_REWARD = "text-10 whitespace-nowrap overflow-hidden text-ellipsis leading-normal";
const RL_VAULT_BADGE = "whitespace-nowrap rounded-3 border border-vaulted/35 bg-vaulted/15 px-1 py-px text-9 font-bold tracking-0.02 text-vaulted";
const RL_PAGINATION = "flex items-center gap-2.5 px-3.5 py-1.5 border-b border-border shrink-0";
const RL_SUBTAB =
  "border-0 border-b-2 bg-transparent text-12 font-medium px-3.5 pt-1 pb-1.5 cursor-pointer transition-[color]";
const RL_SUBTAB_ON = `${RL_SUBTAB} border-b-accent text-accent hover:text-accent`;
const RL_SUBTAB_OFF = `${RL_SUBTAB} border-b-transparent text-muted hover:text-foreground`;
const RL_ROOT = "flex flex-col flex-1 overflow-hidden min-h-0";
const RL_SUBTAB_BAR = "flex gap-0.5 px-3 pt-1.5 border-b border-border shrink-0";

const RL_LIST_CLS: Record<ViewMode, string> = {
  cards:
    "grid grid-cols-[repeat(auto-fill,minmax(420px,1fr))] gap-2 content-start py-2.5 px-3 flex-1 overflow-y-auto",
  icons: "grid grid-cols-[repeat(auto-fill,76px)] gap-1.5 content-start p-2 flex-1 overflow-y-auto",
  "text-cards":
    "grid grid-cols-[repeat(auto-fill,minmax(220px,1fr))] gap-2 content-start py-2.5 px-3 flex-1 overflow-y-auto",
  list: "flex flex-col gap-px px-0 py-1 flex-1 overflow-y-auto",
  "list-compact": "flex flex-col gap-px px-0 py-1 flex-1 overflow-y-auto",
};

  // Card shell + state modifiers. Background and border color depend on state,
  // so complete uses its gold background only for cards and rows.
function relicCardCls(view: ViewMode, state: CardState): string {
  const isRow = view === "list" || view === "list-compact";
  const shell =
    view === "icons"
      ? "w-19 h-22 rounded-8 px-1 pt-1.5 pb-1 flex flex-col items-center justify-center gap-1 cursor-default transition-[border-color] [&_img]:size-13! [&_img]:object-contain!"
      : isRow
      ? "h-40 rounded-8 flex flex-row items-center gap-2 px-3 py-1.25 min-h-8 transition-[border-color]"
      : view === "text-cards"
      ? "h-40 rounded-8 flex flex-row gap-0 overflow-hidden min-h-25 transition-[border-color]"
      : "h-40 rounded-8 flex flex-row transition-[border-color]";
  const gold = "border-ducat/55!";
  const mod =
    state === "complete"
      ? isRow
        ? `border border-l-2! ${gold} bg-ducat/3`
        : view === "icons" || view === "text-cards"
        ? `border ${gold} bg-surface`
        : `border ${gold} bg-ducat/3`
      : state === "unowned"
      ? isRow
        ? "border border-t-border border-r-border border-l-border border-b-border/35 bg-surface opacity-45 hover:opacity-70 hover:border-muted/40!"
        : "border border-border bg-surface opacity-45 hover:opacity-70 hover:border-muted/40!"
      : isRow
      ? "border border-t-border border-r-border border-l-border border-b-border/35 bg-surface hover:border-accent/40"
      : "border border-border bg-surface hover:border-accent/40";
  return `${shell} ${mod}`;
}

// ── Planner ──
const PL_WRAP = "flex flex-col flex-1 overflow-hidden min-h-0";
const PL_CONTROLS = "flex flex-wrap items-center gap-2 px-3 py-2 border-b border-border shrink-0";
const PL_GROUP = "flex items-center gap-1";
const PL_LABEL = "text-10 text-muted mr-0.5 uppercase tracking-0.04";
const PL_COUNT = "text-11 text-muted";
const PL_HEADER =
  "flex items-center px-3 py-1 text-10 text-muted uppercase tracking-wider border-b border-border shrink-0";
const PL_LIST = "flex-1 overflow-y-auto";
const PL_COL_NAME = "flex-1 min-w-0 flex items-center gap-1.5";
const PL_SORTABLE =
  "bg-transparent border-0 cursor-pointer p-0 inline-flex items-center gap-0.75 transition-[color] uppercase tracking-wider text-10";
const PL_SORT_ARROW = "text-8 leading-none";
const PL_SPACER = "w-5 shrink-0";
const PL_ROW = "border-b border-border/50";
const PL_ROW_MAIN =
  "flex items-center gap-0 px-3 py-1.75 cursor-pointer transition-[background] duration-100 hover:bg-white/3";
const PL_RELIC_NAME =
  "text-12 font-semibold text-foreground whitespace-nowrap overflow-hidden text-ellipsis";
const PL_OWNED = "text-11 text-muted shrink-0 ml-auto";
const PL_EV = "w-18 text-right shrink-0 text-12";
const PL_EV_ZERO = "text-muted/40";
const PL_GAIN_POS = "text-11 text-success";
const PL_GAIN_NEG = "text-11 text-danger";
const PL_EXPAND_BTN = "bg-transparent border-0 text-muted text-10 cursor-pointer pl-2 shrink-0 w-5";
const PL_DETAIL = "pt-1.5 pr-3 pb-2.5 pl-6 bg-black/15 border-t border-border/40";
const PL_TIER_ROW = "flex gap-4 mb-1.5 text-10 text-muted";
const PL_REWARD_ROW = "flex items-center gap-2 py-0.75 text-11 border-b border-border/30 last:border-b-0";
const PL_REWARD_RARITY = "w-3 shrink-0 font-bold";
const PL_REWARD_NAME = "flex-1 text-foreground";
const PL_REWARD_CHANCE = "w-12 text-right text-muted shrink-0";
const PL_REWARD_VAL = "w-13 text-right text-accent shrink-0 font-semibold";

function plSortableCls(active: boolean, extra = "") {
  return `${PL_SORTABLE} ${extra} ${active ? "text-foreground" : "text-muted hover:text-foreground"}`;
}


// ─── Helpers ─────────────────────────────────────────────────────────────────

function findCatalogItemGlobal(itemName: string, nameMap: Map<string, CatalogItem>): CatalogItem | undefined {
  const n = itemName.toLowerCase();
  return nameMap.get(n) ?? nameMap.get(n + " blueprint") ?? nameMap.get(n.replace(" blueprint", ""));
}

/** True if the blueprint is in inventory OR the built version of it is. */
function isCatalogItemOwned(cat: CatalogItem | undefined, inventory: Record<string, InventoryItem>, nameMap: Map<string, CatalogItem>): boolean {
  if (!cat) return false;
  if ((inventory[cat.unique_name]?.quantity ?? 0) > 0) return true;
  if (cat.name.endsWith(" Blueprint")) {
    const builtName = cat.name.slice(0, -" Blueprint".length);
    if ((inventory[builtName]?.quantity ?? 0) > 0) return true;
    const builtItem = nameMap.get(builtName.toLowerCase());
    if (builtItem && (inventory[builtItem.unique_name]?.quantity ?? 0) > 0) return true;
  }
  return false;
}

function extractPrimeName(name: string): string | null {
  const idx = name.indexOf(" Prime");
  return idx >= 0 ? name.slice(0, idx + " Prime".length) : null;
}

function parseDropData(raw: any): RelicDrop[] {
  const relicsArray: any[] = Array.isArray(raw?.relics) ? raw.relics : [];

  const map = new Map<string, RelicDrop>();
  for (const r of relicsArray) {
    if (!r || r.state !== "Intact") continue;
    const relicName: string = r.relicName ?? r.name ?? "";
    if (!relicName) continue;
    const tier: string = String(r.tier ?? "");
    // Drop data: tier="Meso", relicName="V13" → baseName "Meso V13"
    // Catalog stores per-refinement: "Meso V13 Intact", "Meso V13 Exceptional", etc.
    const fullName: string = tier ? `${tier} ${relicName}` : relicName;
    const rewards: DropReward[] = (Array.isArray(r.rewards) ? r.rewards : [])
      .map((x: any) => {
        const chance = Number(x.chance ?? 0);
        return {
          itemName: String(x.itemName ?? x.item_name ?? x.name ?? "Unknown"),
          chance,
          rarity: chanceToRarity(chance), // derived from chance, not the unreliable rarity string
        };
      })
      .filter((x: DropReward) => x.itemName !== "Unknown")
      .sort((a: DropReward, b: DropReward) =>
        (RARITY_SORT[a.rarity] ?? 0) - (RARITY_SORT[b.rarity] ?? 0)
      );
    map.set(fullName, { tier, relicName, fullName, rewards });
  }
  return Array.from(map.values());
}

// ─── Images ───────────────────────────────────────────────────────────────────

function RelicImg({ src }: { src?: string }) {
  const [failed, setFailed] = useState(false);
  if (!src || failed)
    return <div className="flex size-11 shrink-0 items-center justify-center rounded-md bg-white/6 text-11 text-muted">R</div>;
  return <img className="size-11 shrink-0 rounded-md object-contain" src={src} alt="" loading="lazy" onError={() => setFailed(true)} />;
}

const RARITY_BG: Record<string, string> = {
  Bronze: "rgba(205,127,50,.2)",
  Silver: "rgba(192,192,192,.15)",
  Gold:   "rgba(240,192,64,.2)",
};

function PartImg({ srcs, rarity }: { srcs: (string | undefined)[]; rarity?: string }) {
  // Deduplicate so the same failing URL isn't retried
  const valid = [...new Set(srcs.filter(Boolean) as string[])];
  const [idx, setIdx] = useState(0);
  const src = valid[idx];
  if (!src) {
    const bg = rarity ? (RARITY_BG[rarity] ?? "rgba(255,255,255,.06)") : "rgba(255,255,255,.06)";
    return <div className="flex size-10 items-center justify-center rounded bg-white/6 text-9 text-white/30" style={{ background: bg }}>?</div>;
  }
  // key={src} forces React to unmount/remount the img when src changes,
  // preventing the broken-image icon from persisting between attempts
  return <img key={src} className="block size-10 rounded object-contain" src={src} alt="" loading="lazy"
    onError={() => setIdx(i => i + 1)} />;
}

const CDN = (name?: string) => name ? warframeStatImageUrl(name) : undefined;

// ─── Reward box ───────────────────────────────────────────────────────────────

function RewardBox({ reward, imageSrcs, isOwned, isComplete, isHighlighted, colorblindMode }: {
  reward: DropReward;
  imageSrcs: (string | undefined)[];
  isOwned: boolean;
  isComplete: boolean;
  isHighlighted: boolean;
  colorblindMode: boolean;
}) {
  const cls   = RARITY_CSS[reward.rarity] ?? "bronze";
  const shortName = reward.itemName.replace(" Blueprint", "").replace("Prime", "P.").trim();
  const rboxState = isHighlighted
    ? " bg-accent/18 outline-2 outline-accent"
    : isComplete
    ? " bg-ducat/12"
    : isOwned
    ? " bg-success/10"
    : "";
  const nameColor = isHighlighted
    ? "text-relic-highlight"
    : isComplete
    ? "text-ducat"
    : isOwned
    ? "text-owned"
    : "text-muted";
  return (
    <div
      className={`relic-rbox ${RL_RBOX} ${RL_RBOX_TOP[cls]}${rboxState}`}
      title={`${reward.itemName} — ${reward.rarity} (${reward.chance.toFixed(1)}%)`}
    >
      {/* Top-right corner: rarity label + optional colorblind checkmark stacked */}
      <span className={RL_CORNER}>
        <span className={`${RL_RARITY_LABEL} ${RL_RARITY_LABEL_COLOR[cls]}`} title={reward.rarity}>
          {cls === "bronze" ? "C" : cls === "silver" ? "U" : "R"}
        </span>
        {colorblindMode && (isOwned || isComplete) && (
          <span className={`${RL_CB_CHECK} ${isComplete ? "text-ducat" : "text-owned"}`}>{isComplete ? "✓✓" : "✓"}</span>
        )}
      </span>
      <PartImg srcs={imageSrcs} rarity={reward.rarity} />
      <span className={`${RL_RBOX_NAME} ${nameColor}`}>{shortName}</span>
    </div>
  );
}

// ─── Relic card ───────────────────────────────────────────────────────────────

function isFormaOrKuva(itemName: string): boolean {
  return itemName.includes("Forma") || itemName === "Kuva";
}

function RelicCard({ drop, catalogRelicByName, inventory, ownedPrimeNames, searchTerms, nameMap, colorblindMode, view, ignoreFormaKuva }: {
  drop: RelicDrop;
  catalogRelicByName: Map<string, CatalogItem>;
  inventory: Record<string, InventoryItem>;
  ownedPrimeNames: Set<string>;
  searchTerms: readonly string[];
  nameMap: Map<string, CatalogItem>;
  colorblindMode: boolean;
  view: ViewMode;
  ignoreFormaKuva: boolean;
}) {
  const baseLower = drop.fullName.toLowerCase();

  // Per-refinement counts using catalog
  const refCounts = RELIC_REFINEMENT_ORDER.map(ref => {
    const cat = catalogRelicByName.get(`${baseLower} ${ref}`);
    return { label: RELIC_REFINEMENT_LABELS[ref], count: cat ? (inventory[cat.unique_name]?.quantity ?? 0) : 0 };
  });
  const total = refCounts.reduce((s, r) => s + r.count, 0);

  // Relic icon comes from the Intact catalog entry
  const intactCat = catalogRelicByName.get(`${baseLower} intact`);

  // Find catalog item by name — returns item with best available image_name
  const findCatalogItem = (itemName: string): CatalogItem | undefined => {
    const n = itemName.toLowerCase();

    // 1. Exact match
    let found = nameMap.get(n);
    // 2. Blueprint toggle
    if (!found) {
      found = n.endsWith(" blueprint")
        ? nameMap.get(n.slice(0, -" blueprint".length))
        : nameMap.get(n + " blueprint");
    }
    // 3. Fuzzy: all significant words must appear in catalog item name
    if (!found) {
      const words = n.replace(" blueprint", "").split(" ").filter(w => w.length > 2);
      if (words.length >= 2) {
        for (const [, item] of nameMap) {
          if (words.every(w => item.name.toLowerCase().includes(w))) { found = item; break; }
        }
      }
    }

    // 4. If found but no image, try parent prime item's image as fallback
    //    e.g. "Yareli Prime Blueprint" → look up "Yareli Prime" for its warframe image
    if (found && !found.image_name) {
      const parentName = extractPrimeName(itemName);
      if (parentName) {
        const parent = nameMap.get(parentName.toLowerCase());
        if (parent?.image_name) return { ...found, image_name: parent.image_name };
      }
    }

    return found;
  };

  const safeRewards = drop.rewards.filter(r => r?.itemName);
  const allComplete = safeRewards.length > 0 && safeRewards.every(r => {
    if (ignoreFormaKuva && isFormaOrKuva(r.itemName)) return true;
    const cat = findCatalogItem(r.itemName);
    const isOwned = isCatalogItemOwned(cat, inventory, nameMap);
    const p = extractPrimeName(r.itemName);
    const pItem = p ? nameMap.get(p.toLowerCase()) : undefined;
    const pInv = pItem ? inventory[pItem.unique_name] : undefined;
    const isComplete = pItem
      ? (pInv?.quantity ?? 0) > 0 || (pInv?.mastery_rank ?? 0) >= 30
      : (p ? ownedPrimeNames.has(p.toLowerCase()) : false);
    return isOwned || isComplete;
  });

  const slots: (DropReward | null)[] = [
    ...drop.rewards,
    ...Array<null>(Math.max(0, 6 - drop.rewards.length)).fill(null),
  ];

  const cardState: CardState = total === 0 ? "unowned" : allComplete ? "complete" : "none";

  if (view === "icons") {
    return (
      <div className={relicCardCls("icons", cardState)} title={`${drop.fullName} ×${total}`}>
        <RelicImg src={CDN(intactCat?.image_name)} />
        <span className={RL_ICON_COUNT}>×{total}</span>
      </div>
    );
  }

  if (view === "list" || view === "list-compact") {
    const refCompact = refCounts.filter(r => r.count > 0)
      .map(r => `${r.label[0].toUpperCase()}:${r.count}`)
      .join(" ");
    return (
      <div className={relicCardCls(view, cardState)}>
        {view === "list" && <div className={RL_ROW_IMG}><RelicImg src={CDN(intactCat?.image_name)} /></div>}
        <div className={RL_ROW_NAME}>{drop.fullName}</div>
        {intactCat?.vaulted && <span className={RL_VAULT_BADGE}>🔒</span>}
        <span className={RL_ROW_TOTAL}>×{total}</span>
        {refCompact && <span className={RL_ROW_REFS}>{refCompact}</span>}
      </div>
    );
  }

  if (view === "text-cards") {
    return (
      <div className={relicCardCls("text-cards", cardState)}>
        <div className={RL_CARD_LEFT_TEXT}>
          <div className={RL_CARD_NAME}>{drop.fullName}</div>
          {intactCat?.vaulted && <span className={RL_VAULT_BADGE}>🔒 Vaulted</span>}
          <div className={RL_REFINEMENTS}>
            {refCounts.some(r => r.count > 0)
              ? refCounts.map(r => (
                <span key={r.label} className={`${RL_REF} ${r.count > 0 ? "text-foreground font-medium" : "text-muted opacity-40"}`}>
                  {r.count} {r.label}
                </span>
              ))
              : <span className={`${RL_REF} text-foreground font-medium`}>Total: {total}</span>}
          </div>
        </div>
        <div className={RL_TEXT_REWARDS}>
          {drop.rewards.map((r, i) => (
            <div key={i} className={`${RL_TEXT_REWARD} ${RL_TEXT_REWARD_COLOR[r.rarity?.toLowerCase() ?? "common"] ?? "text-muted"}`}>
              {r.itemName}
            </div>
          ))}
        </div>
      </div>
    );
  }

  return (
    <div className={relicCardCls("cards", cardState)}>
      <div className={RL_CARD_LEFT}>
        <div className={RL_ICON_ROW}>
          <RelicImg src={CDN(intactCat?.image_name)} />
          <span className={RL_TOTAL}>×{total}</span>
          {colorblindMode && allComplete && <span className={RL_CB_RELIC_CHECK} title="All rewards obtained">✓✓</span>}
        </div>
        <div className={RL_CARD_NAME}>{drop.fullName}</div>
        {intactCat?.vaulted && <span className={RL_VAULT_BADGE}>🔒 Vaulted</span>}
        <div className={RL_REFINEMENTS}>
          {refCounts.some(r => r.count > 0)
            ? refCounts.map(r => (
              <span key={r.label} className={`${RL_REF} ${r.count > 0 ? "text-foreground font-medium" : "text-muted opacity-40"}`}>
                {r.count} {r.label}
              </span>
            ))
            : <span className={`${RL_REF} text-foreground font-medium`}>Total: {total}</span>
          }
        </div>
      </div>

      <div className={RL_REWARDS_GRID}>
        {slots.map((r, i) => {
          if (!r) return (
            <div key={i} className={`relic-rbox ${RL_RBOX_EMPTY}`}>
              <PartImg srcs={[]} rarity={undefined} />
              <span className={`${RL_RBOX_NAME} text-muted`}>—</span>
            </div>
          );
          const catalogItem = findCatalogItem(r.itemName);
          const isOwned = isCatalogItemOwned(catalogItem, inventory, nameMap);
          // Build list of image URLs to try in order (PartImg tries each, moves to next on 404)
          const imageItem = catalogItem?.image_name ? catalogItem : findCatalogItem(r.itemName);
          const primeName = extractPrimeName(r.itemName); // e.g. "Yareli Prime"
          const primeImageItem = primeName ? nameMap.get(primeName.toLowerCase()) : undefined;

          const imageSrcs: (string | undefined)[] = [
            // 1. Catalog item image (direct or parent-prime fallback from findCatalogItem)
            CDN(imageItem?.image_name),
            // 2. Parent prime warframe/weapon image
            CDN(primeImageItem?.image_name),
            // 3. Construct from catalog unique_name: "YareliPrimeBlueprint" → "YareliPrime.png"
            (() => {
              const seg = (catalogItem?.unique_name ?? "").split("/").pop() ?? "";
              const file = seg.replace(/Blueprint$/, "");
              return file ? warframeStatImageUrl(`${file}.png`) : undefined;
            })(),
            // 4. Construct from parent prime name: "Yareli Prime" → "YareliPrime.png"
            primeName ? warframeStatImageUrl(`${primeName.replace(/\s+/g, "")}.png`) : undefined,
            // 5. Strip "Blueprint" from item name: "Forma Blueprint" → "Forma.png"
            warframeStatImageUrl(`${r.itemName.replace(" Blueprint", "").replace(/\s+/g, "")}.png`),
            // 6. Strip leading count prefix: "2X Forma" → "Forma.png"
            warframeStatImageUrl(`${r.itemName.replace(/^\d+[xX]\s*/, "").replace(" Blueprint", "").replace(/\s+/g, "")}.png`),
          ];
          // Gold: the complete parent prime item is built and in inventory
          // "Burston Prime Barrel" → find "Burston Prime" → check inventory by name
          const parentName = extractPrimeName(r.itemName);
          const parentItem = parentName ? nameMap.get(parentName.toLowerCase()) : undefined;
          const parentInv = parentItem ? inventory[parentItem.unique_name] : undefined;
          const isComplete = (ignoreFormaKuva && isFormaOrKuva(r.itemName))
            || (parentName
              ? (inventory[parentName]?.quantity ?? 0) > 0 ||
                (parentInv ? (parentInv.quantity > 0 || parentInv.mastery_rank >= 30) : false) ||
                ownedPrimeNames.has(parentName.toLowerCase())
              : false);
          return (
            <RewardBox
              key={i}
              reward={r}
              imageSrcs={imageSrcs}
              isOwned={isOwned}
              isComplete={isComplete}
              isHighlighted={searchTerms.some(term => term.length > 1) && matchesSearchTerms(searchTerms.filter(term => term.length > 1), r.itemName)}
              colorblindMode={colorblindMode}
            />
          );
        })}
      </div>
    </div>
  );
}

// ─── Planner ─────────────────────────────────────────────────────────────────

type PlannerTier = keyof typeof RELIC_DROP_RATES;

function wfmNorm(name: string): string {
  return name.toLowerCase().replace(/[^a-z0-9]+/g, "_").replace(/^_|_$/g, "");
}

function computeEV(
  rewards: DropReward[],
  tier: PlannerTier,
  vals: number[],
  squadSize: number,
): number {
  const rates = RELIC_DROP_RATES[tier];
  const probs = rewards.map(r => rates[r.rarity as keyof typeof rates] ?? 0);
  const n = rewards.length;
  if (n === 0) return 0;

  if (squadSize === 1) {
    return rewards.reduce((s, r, i) => s + (rates[r.rarity as keyof typeof rates] ?? 0) * vals[i], 0);
  }

  // For N>1: iterate all n^N draw combinations, weight by probability, take max value.
  // n=6, N≤4 → at most 1296 combinations — runs in <1ms.
  let ev = 0;
  const iterate = (player: number, prob: number, maxVal: number) => {
    if (player === squadSize) { ev += prob * maxVal; return; }
    for (let i = 0; i < n; i++) iterate(player + 1, prob * probs[i], Math.max(maxVal, vals[i]));
  };
  iterate(0, 1.0, 0);
  return ev;
}

function PlannerTab({
  drops, nameMap, catalogRelicByName, inventory,
}: {
  drops: RelicDrop[];
  nameMap: Map<string, CatalogItem>;
  catalogRelicByName: Map<string, CatalogItem>;
  inventory: Record<string, InventoryItem>;
}) {
  const [metric, setMetric]         = useState<"plat" | "ducat">("plat");
  const [squadSize, setSquadSize]   = useState<1 | 2 | 3 | 4>(1);
  const [ownedOnly, setOwnedOnly]   = useState(true);
  const [vaultFilter, setVaultFilter] = useState<"all" | "vaulted" | "unvaulted">("all");
  const [tierFilter, setTierFilter] = useState<string[]>([]);
  const [expanded, setExpanded]     = useState<string | null>(null);
  const [sortCol, setSortCol]       = useState<"name" | "owned" | PlannerTier | "gain">("radiant");
  const [sortDir, setSortDir]       = useState<"desc" | "asc">("desc");
  // WFM url_name → price map (keyed by url_name slug)
  const [platPrices, setPlatPrices] = useState<Map<string, number>>(new Map());

  useEffect(() => {
    let cancelled = false;
    invoke<WfmItem[]>(TAURI_COMMANDS.FETCH_WFM_ITEMS)
      .then(items => {
        if (cancelled) return null;
        const lookup = new Map<string, string>();
        for (const w of items) lookup.set(wfmNorm(w.item_name), w.url_name);
        return lookup;
      })
      .then(lookup => {
        if (!lookup || cancelled) return;
        invoke<WfmCachedPrices>("wfm_get_cached_prices")
          .then(raw => {
            if (cancelled) return;
            const m = new Map<string, number>();
            for (const [slug, price] of Object.entries(raw)) {
              if (price != null) m.set(slug, price);
            }
            for (const [norm, slug] of lookup) {
              const p = m.get(slug);
              if (p != null) m.set(norm, p);
            }
            setPlatPrices(m);
          })
          .catch(() => {});
      })
      .catch(() => {});
    return () => { cancelled = true; };
  }, []);

  const getOwnedByTier = useCallback((drop: RelicDrop) => {
    const base = drop.fullName.toLowerCase();
    return RELIC_REFINEMENT_ORDER.reduce<Record<string, number>>((acc, ref) => {
      const cat = catalogRelicByName.get(`${base} ${ref}`);
      acc[ref] = cat ? (inventory[cat.unique_name]?.quantity ?? 0) : 0;
      return acc;
    }, {});
  }, [catalogRelicByName, inventory]);

  const getTotal = useCallback((drop: RelicDrop) => {
    const tiers = getOwnedByTier(drop);
    return Object.values(tiers).reduce((s, n) => s + n, 0);
  }, [getOwnedByTier]);

  // Precompute per-relic EV at all tiers
  const plannerRows = useMemo(() => {
    return drops
      .filter(d => {
        if (!d.relicName) return false;
        if (ownedOnly && getTotal(d) === 0) return false;
        if (tierFilter.length > 0 && !tierFilter.includes(d.tier.toLowerCase())) return false;
        if (vaultFilter !== "all") {
          const cat = catalogRelicByName.get(`${d.fullName.toLowerCase()} intact`);
          if (vaultFilter === "vaulted" && cat?.vaulted !== true) return false;
          if (vaultFilter === "unvaulted" && cat?.vaulted === true) return false;
        }
        return true;
      })
      .map(drop => {
        const rewards = drop.rewards.filter(r => r?.itemName);
        const vals = rewards.map(r => {
          if (metric === "ducat") {
            const cat = findCatalogItemGlobal(r.itemName, nameMap);
            return cat?.ducats ?? 0;
          }
          // plat: lookup by slug or normalized name
          const slug = platPrices.get(wfmNorm(r.itemName));
          if (slug != null) return slug;
          return platPrices.get(wfmNorm(r.itemName)) ?? 0;
        });

        const evByTier = Object.fromEntries(
          RELIC_REFINEMENT_ORDER.map(t => [t, computeEV(rewards, t, vals, squadSize)])
        ) as Record<PlannerTier, number>;

        const bestTier = RELIC_REFINEMENT_ORDER.reduce((best, t) =>
          evByTier[t] > evByTier[best] ? t : best, "intact" as PlannerTier);

        const ownedByTier = getOwnedByTier(drop);
        const totalOwned  = Object.values(ownedByTier).reduce((s, n) => s + n, 0);
        const vaulted     = catalogRelicByName.get(`${drop.fullName.toLowerCase()} intact`)?.vaulted === true;

        return { drop, rewards, vals, evByTier, bestTier, ownedByTier, totalOwned, vaulted };
      })
      .sort((a, b) => {
        let delta = 0;
        if (sortCol === "name")  delta = a.drop.fullName.localeCompare(b.drop.fullName);
        else if (sortCol === "owned") delta = a.totalOwned - b.totalOwned;
        else if (sortCol === "gain")  delta = (a.evByTier.radiant - a.evByTier.intact) - (b.evByTier.radiant - b.evByTier.intact);
        else delta = a.evByTier[sortCol] - b.evByTier[sortCol];
        return sortDir === "desc" ? -delta : delta;
      });
  }, [drops, metric, squadSize, ownedOnly, tierFilter, vaultFilter, sortCol, sortDir, platPrices, nameMap, catalogRelicByName, inventory, getOwnedByTier, getTotal]);

  const unit = metric === "plat" ? "p" : " dc";

  function handleSort(col: typeof sortCol) {
    if (col === sortCol) setSortDir(d => d === "desc" ? "asc" : "desc");
    else { setSortCol(col); setSortDir(col === "name" ? "asc" : "desc"); }
  }
  function sortArrow(col: typeof sortCol) {
    return (
      <span className={`${PL_SORT_ARROW} ${col === sortCol ? "visible" : "invisible"}`}>
        {sortDir === "desc" ? "▼" : "▲"}
      </span>
    );
  }

  return (
    <div className={PL_WRAP}>
      {/* Controls */}
      <div className={PL_CONTROLS}>
        <div className={PL_GROUP}>
          <span className={PL_LABEL}>Metric</span>
          <FilterChip active={metric === "plat"} onClick={() => setMetric("plat")}>Platinum</FilterChip>
          <FilterChip active={metric === "ducat"} onClick={() => setMetric("ducat")}>Ducats</FilterChip>
        </div>
        <div className={PL_GROUP}>
          <span className={PL_LABEL}>Squad</span>
          {([1, 2, 3, 4] as const).map(n => (
            <FilterChip key={n} active={squadSize === n} onClick={() => setSquadSize(n)}>
              {n === 1 ? "Solo" : `${n}p`}
            </FilterChip>
          ))}
        </div>
        <div className={PL_GROUP}>
          <span className={PL_LABEL}>Era</span>
          {(["lith","meso","neo","axi"] as const).map(t => (
            <FilterChip key={t} active={tierFilter.includes(t)}
              onClick={() => setTierFilter(prev => prev.includes(t) ? prev.filter(x => x !== t) : [...prev, t])}>
              {t[0].toUpperCase() + t.slice(1)}
            </FilterChip>
          ))}
        </div>
        <div className={PL_GROUP}>
          <FilterChip active={vaultFilter === "unvaulted"} onClick={() => setVaultFilter(v => v === "unvaulted" ? "all" : "unvaulted")}>Unvaulted</FilterChip>
          <FilterChip active={vaultFilter === "vaulted"} onClick={() => setVaultFilter(v => v === "vaulted" ? "all" : "vaulted")}>Vaulted</FilterChip>
          <FilterChip active={ownedOnly} onClick={() => setOwnedOnly(v => !v)}>Owned Only</FilterChip>
        </div>
        <span className={`${PL_COUNT} ml-auto`}>{plannerRows.length} relics</span>
      </div>

      {/* Column header */}
      <div className={PL_HEADER}>
        <div className={PL_COL_NAME}>
          <button className={plSortableCls(sortCol === "name")} onClick={() => handleSort("name")}>
            Relic{sortArrow("name")}
          </button>
          <button className={plSortableCls(sortCol === "owned", "ml-auto")} onClick={() => handleSort("owned")}>
            Owned{sortArrow("owned")}
          </button>
        </div>
        {RELIC_REFINEMENT_ORDER.map(t => (
          <button key={t} className={plSortableCls(sortCol === t, "w-18 text-right shrink-0 justify-end")} onClick={() => handleSort(t)}>
            {RELIC_REFINEMENT_LABELS[t]}{sortArrow(t)}
          </button>
        ))}
        <button className={plSortableCls(sortCol === "gain", "w-32.5 text-right shrink-0 pr-8 justify-end")} onClick={() => handleSort("gain")}>
          Refine gain{sortArrow("gain")}
        </button>
        <div className={PL_SPACER} aria-hidden />
      </div>

      {/* Rows */}
      <div className={PL_LIST}>
        {plannerRows.length === 0 ? (
          <EmptyMessage>No relics match. Try turning off Owned Only.</EmptyMessage>
        ) : plannerRows.map(({ drop, rewards, vals, evByTier, bestTier, totalOwned, vaulted }) => {
          const isOpen = expanded === drop.fullName;
          const gain   = evByTier.radiant - evByTier.intact;
          return (
            <div key={drop.fullName} className={PL_ROW}>
              <div className={PL_ROW_MAIN} onClick={() => setExpanded(isOpen ? null : drop.fullName)}>
                <div className={PL_COL_NAME}>
                  <span className={PL_RELIC_NAME}>{drop.fullName}</span>
                  {vaulted && <span className={RL_VAULT_BADGE}>🔒</span>}
                  <span className={PL_OWNED}>×{totalOwned}</span>
                </div>
                {RELIC_REFINEMENT_ORDER.map(t => (
                  <div key={t} className={`${PL_EV} ${t === bestTier ? "text-success font-bold" : "text-muted"}`}>
                    {evByTier[t] < 0.05 ? <span className={PL_EV_ZERO}>—</span> : `${evByTier[t].toFixed(1)}${unit}`}
                  </div>
                ))}
                <div className="w-32.5 text-right shrink-0 pr-8">
                  {gain >= 0.1
                    ? <span className={PL_GAIN_POS}>+{gain.toFixed(1)}{unit}</span>
                    : <span className={PL_GAIN_NEG}>{gain.toFixed(1)}{unit}</span>}
                </div>
                <button className={PL_EXPAND_BTN}>{isOpen ? "▲" : "▼"}</button>
              </div>

              {isOpen && (
                <div className={PL_DETAIL}>
                  <div className={PL_TIER_ROW}>
                    {RELIC_REFINEMENT_ORDER.map(t => (
                      <span key={t}>{RELIC_REFINEMENT_LABELS[t]}: {RELIC_DROP_RATES[t].Rare * 100}% rare</span>
                    ))}
                  </div>
                  {rewards.map((r, i) => {
                    const rates = RELIC_DROP_RATES[bestTier];
                    const chance = rates[r.rarity as keyof typeof rates] ?? 0;
                    const cls = RARITY_CSS[r.rarity] ?? "bronze";
                    return (
                      <div key={i} className={PL_REWARD_ROW}>
                        <span className={`${PL_REWARD_RARITY} ${PLANNER_RARITY_COLOR[cls]}`}>{r.rarity[0]}</span>
                        <span className={PL_REWARD_NAME}>{r.itemName}</span>
                        <span className={PL_REWARD_CHANCE}>{(chance * 100).toFixed(2)}%</span>
                        <span className={PL_REWARD_VAL}>{vals[i] > 0 ? `${vals[i]}${unit}` : "—"}</span>
                      </div>
                    );
                  })}
                </div>
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
}

// ─── Main component ───────────────────────────────────────────────────────────

export default function RelicHelper({ inventory, colorblindMode = false, filters, onFiltersChange, filterPresets, onFilterPresetsChange, onOpenSettings }: Props) {
  const { catalog } = useCatalog();
  const [plannerActive, setPlannerActive] = useState(false);
  const [relicView, setRelicView] = useState<ViewMode>(() =>
    (localStorage.getItem(PREFERENCE_KEYS.RELIC_VIEW) as ViewMode | null) ?? "cards"
  );
  const [drops,       setDrops]       = useState<RelicDrop[]>([]);
  const [dropLoading, setDropLoading] = useState(false);
  const [dropError,   setDropError]   = useState(false);
  const [page,        setPage]        = useState(0);
  const dropRequestRef = useRef(0);
  const PAGE_SIZE = 30;

  const { search, tiers, ownership, vault, completion, sortMode, ignoreFormaKuva } = filters;
  const set = <K extends keyof RelicFilters>(k: K, v: RelicFilters[K]) => onFiltersChange({ ...filters, [k]: v });

  const loadDrops = useCallback((force = false) => {
    const request = ++dropRequestRef.current;
    setDropLoading(true);
    setDropError(false);
    invoke<unknown>("get_drop_data", { force })
      .then(d => {
        if (request !== dropRequestRef.current) return;
        const result = parseDropData(d);
        setDrops(result);
      })
      .catch(() => { if (request === dropRequestRef.current) setDropError(true); })
      .finally(() => { if (request === dropRequestRef.current) setDropLoading(false); });
  }, []);

  useEffect(() => {
    loadDrops();
    return () => { dropRequestRef.current++; };
  }, [loadDrops]);

  const nameMap = useMemo(() => {
    const m = new Map<string, CatalogItem>();
    for (const i of catalog) m.set(i.name.toLowerCase(), i);
    return m;
  }, [catalog]);

  const catalogRelicByName = useMemo(() => {
    const m = new Map<string, CatalogItem>();
    for (const i of catalog) if (i.category === "Relics") m.set(i.name.toLowerCase(), i);
    return m;
  }, [catalog]);


  const ownedPrimeNames = useMemo(() => {
    const s = new Set<string>();
    for (const [key, entry] of Object.entries(inventory)) {
      // Owned OR mastered (sold after mastery still counts as done)
      if (entry.quantity <= 0 && entry.mastery_rank < 30) continue;
      // Only process name-keyed entries (path aliases start with "/")
      if (!key.startsWith("/") && key.includes("Prime")) s.add(key.toLowerCase());
    }
    return s;
  }, [inventory]);

  // Catalog stores per-refinement: "Meso V13 Intact", "Meso V13 Exceptional", "Meso V13 Flawless", "Meso V13 Radiant"
  const getTotal = useCallback((drop: RelicDrop): number => {
    if (!drop?.fullName) return 0;
    const base = drop.fullName.toLowerCase();
    return RELIC_REFINEMENT_ORDER.reduce((sum, ref) => {
      const cat = catalogRelicByName.get(`${base} ${ref}`);
      return sum + (cat ? (inventory[cat.unique_name]?.quantity ?? 0) : 0);
    }, 0);
  }, [catalogRelicByName, inventory]);

  const searchTerms = useMemo(() => splitSearchTerms(search), [search]);

  const visibleDrops = useMemo(() => drops
    .filter(d => {
      return matchesSearchTerms(searchTerms, d.fullName ?? "", d.relicName ?? "", ...d.rewards.map(reward => reward.itemName ?? ""));
    })
    .filter(d => {
      if (tiers.length === 0) return true;
      return tiers.includes((d.tier ?? "").toLowerCase());
    })
    .filter(d => {
      if (ownership.length === 0 || ownership.length === 2) return true;
      const owned = getTotal(d) > 0;
      return ownership.includes("owned") ? owned : !owned;
    })
    .filter(d => {
      if (vault.length === 0 || vault.length === 2) return true;
      const cat = catalogRelicByName.get(`${d.fullName.toLowerCase()} intact`);
      return vault.includes("vaulted") ? cat?.vaulted === true : cat?.vaulted === false;
    })
    .filter(d => {
      if (completion.length === 0 || completion.length === 2) return true;
      const allDone = d.rewards.length > 0 && d.rewards.every(r => {
        if (ignoreFormaKuva && isFormaOrKuva(r.itemName)) return true;
        const cat = findCatalogItemGlobal(r.itemName, nameMap);
        const p = extractPrimeName(r.itemName);
        const pItem = p ? nameMap.get(p.toLowerCase()) : undefined;
        const pInv = pItem ? inventory[pItem.unique_name] : undefined;
        return isCatalogItemOwned(cat, inventory, nameMap)
          || (p ? ownedPrimeNames.has(p.toLowerCase()) : false)
          || (p ? (inventory[p]?.quantity ?? 0) > 0 : false)
          || (pInv ? pInv.mastery_rank >= 30 : false);
      });
      return completion.includes("complete") ? allDone : !allDone;
    })
    .filter(d => d?.relicName)
    .sort((a, b) => {
      if (sortMode === "count") return getTotal(b) - getTotal(a) || (a.relicName ?? "").localeCompare(b.relicName ?? "");
      if (sortMode === "ducats") {
        const avg = (d: RelicDrop) => d.rewards.reduce((s, r) => {
          const cat = findCatalogItemGlobal(r.itemName, nameMap);
          return s + (cat?.ducats ?? 0) * (RELIC_DROP_RATES.intact[r.rarity as keyof typeof RELIC_DROP_RATES.intact] ?? 0);
        }, 0);
        return avg(b) - avg(a) || (a.relicName ?? "").localeCompare(b.relicName ?? "");
      }
      if (sortMode === "za") return (b.fullName ?? "").localeCompare(a.fullName ?? "");
      return (a.fullName ?? "").localeCompare(b.fullName ?? ""); // az + plat fallback
    }),
  [drops, searchTerms, tiers, ownership, vault, completion, sortMode, getTotal, catalogRelicByName, nameMap, inventory, ownedPrimeNames]);

  const ownedCount = useMemo(() =>
    drops.filter(d => getTotal(d) > 0).length,
  [drops, getTotal]);

  useEffect(() => { setPage(0); }, [filters]);

  const pagedDrops = visibleDrops.slice(page * PAGE_SIZE, (page + 1) * PAGE_SIZE);
  const totalPages = Math.ceil(visibleDrops.length / PAGE_SIZE);

  const highlightSearchTerms = searchTerms.filter(term => term.length > 1);
  const searchMatchesReward = highlightSearchTerms.length > 0
    && drops.some(d => d.rewards.some(reward => matchesSearchTerms(highlightSearchTerms, reward.itemName ?? "")));

  return (
    <div className={RL_ROOT}>
      {/* Sub-tab bar */}
      <div className={RL_SUBTAB_BAR}>
        <button className={plannerActive ? RL_SUBTAB_OFF : RL_SUBTAB_ON} onClick={() => setPlannerActive(false)}>Relics</button>
        <button className={plannerActive ? RL_SUBTAB_ON : RL_SUBTAB_OFF} onClick={() => setPlannerActive(true)}>Planner</button>
      </div>

      {plannerActive ? (
        <PlannerTab
          drops={drops}
          nameMap={nameMap}
          catalogRelicByName={catalogRelicByName}
          inventory={inventory}
        />
      ) : (<>
      <div className="market-header">
        <FoundrySearch
          className="w-55"
          placeholder="Relic or item names (comma-separated)…"
          value={search} onChange={e => set("search", e.target.value)}
        />
        <FilterBar className="flex-1 flex-wrap border-0 p-0">
          {(["Lith","Meso","Neo","Axi","Requiem"] as const).map(t => (
            <FilterChip key={t} active={tiers.includes(t.toLowerCase())}
              onClick={() => set("tiers", toggle(tiers, t.toLowerCase()))}>{t}</FilterChip>
          ))}
          <FilterSeparator />
          <FilterChip active={ownership.includes("owned")} onClick={() => set("ownership", toggle(ownership, "owned"))}>Owned</FilterChip>
          <FilterChip active={ownership.includes("notowned")} onClick={() => set("ownership", toggle(ownership, "notowned"))}>Not Owned</FilterChip>
          <FilterSeparator />
          <FilterChip active={vault.includes("vaulted")} onClick={() => set("vault", toggle(vault, "vaulted"))}>Vaulted</FilterChip>
          <FilterChip active={vault.includes("unvaulted")} onClick={() => set("vault", toggle(vault, "unvaulted"))}>Unvaulted</FilterChip>
          <FilterSeparator />
          <FilterChip active={completion.includes("complete")} onClick={() => set("completion", toggle(completion, "complete"))}>Completed</FilterChip>
          <FilterChip active={completion.includes("incomplete")} onClick={() => set("completion", toggle(completion, "incomplete"))}>Uncompleted</FilterChip>
          <FilterChip active={ignoreFormaKuva} onClick={() => set("ignoreFormaKuva", !ignoreFormaKuva)} title="Treat Forma and Kuva rewards as always obtained when checking completion">Ignore Forma/Kuva</FilterChip>
          <FilterSeparator />
          <FilterPresets module="relics" {...{ filters, onFiltersChange, filterPresets, onFilterPresetsChange, onOpenSettings }} />
          <FilterSeparator />
          <FilterLabel>Sort:</FilterLabel>
          <FilterChip active={sortMode === "count"} onClick={() => set("sortMode", "count")}>Most Owned</FilterChip>
          <FilterChip active={sortMode === "plat"} onClick={() => set("sortMode", "plat")}>Avg Plat</FilterChip>
          <FilterChip active={sortMode === "ducats"} onClick={() => set("sortMode", "ducats")}>Avg Ducats</FilterChip>
          <FilterChip active={sortMode === "az"} onClick={() => set("sortMode", "az")}>A–Z</FilterChip>
          <FilterChip active={sortMode === "za"} onClick={() => set("sortMode", "za")}>Z–A</FilterChip>
          <FilterSeparator />
          {dropError && <SecondaryButton className="ml-1" onClick={() => loadDrops(true)}>↺ Retry</SecondaryButton>}
          <span className="ml-auto text-11 text-muted">
            {dropLoading ? "Loading…" : `${visibleDrops.length} relics · ${ownedCount} owned`}
          </span>
          <ViewToggle view={relicView} onChange={v => { setRelicView(v); localStorage.setItem(PREFERENCE_KEYS.RELIC_VIEW, v); }} />
          <HelpTip items={[
            { border: "#e8923a", icon: "C", label: "Common",   desc: "Bronze border — ~25% chance per run" },
            { border: "#c0c0c0", icon: "U", label: "Uncommon", desc: "Silver border — ~11% chance per run" },
            { border: "#f0c040", icon: "R", label: "Rare",     desc: "Gold border — ~2% chance per run" },
            { swatch: "rgba(63,185,80,.5)",  icon: "✓",  label: "Part owned",     desc: "Green box — blueprint or part in inventory" },
            { swatch: "rgba(240,192,64,.5)", icon: "✓✓", label: "Item complete",  desc: "Gold box — built warframe/weapon owned" },
          ]} />
        </FilterBar>
      </div>

      {searchMatchesReward && (
        <div className="px-3.5 py-1 text-11 text-accent">
          Showing relics with reward drops matching one or more search terms — highlighted in blue
        </div>
      )}

      {visibleDrops.length > PAGE_SIZE && (
        <div className={RL_PAGINATION}>
          <SecondaryButton disabled={page === 0} onClick={() => setPage(p => p - 1)}>← Prev</SecondaryButton>
          <span className="text-11 text-muted">
            {page + 1} / {totalPages} &nbsp;({visibleDrops.length} relics)
          </span>
          <SecondaryButton disabled={page >= totalPages - 1} onClick={() => setPage(p => p + 1)}>Next →</SecondaryButton>
        </div>
      )}

      <div className={`relic-list relic-list-${relicView} ${RL_LIST_CLS[relicView]}`}>
        {visibleDrops.length === 0 ? (
          <EmptyMessage>{dropLoading ? "Fetching drop data…" : "No relics match."}</EmptyMessage>
        ) : pagedDrops.map(drop => (
          <RelicCard
            key={drop.fullName}
            drop={drop}
            catalogRelicByName={catalogRelicByName}
            inventory={inventory}
            ownedPrimeNames={ownedPrimeNames}
            searchTerms={searchTerms}
            nameMap={nameMap}
            colorblindMode={colorblindMode}
            view={relicView}
            ignoreFormaKuva={ignoreFormaKuva}
          />
        ))}
      </div>
      </>)}
    </div>
  );
}
