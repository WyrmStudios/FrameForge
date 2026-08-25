import { useState, useEffect, useMemo, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { HelpTip } from "./HelpTip";
import { cdnUrl, useImgLadder } from "./ImgCacheDir";
import type { InventoryItem, ViewMode } from "./App";
import { ViewToggle } from "./App";

// ─── Types ────────────────────────────────────────────────────────────────────

interface CatalogItem {
  unique_name: string;
  name: string;
  category: string;
  image_name?: string;
  vaulted?: boolean | null;
  ducats?: number | null;
}

interface DropReward {
  itemName: string;
  chance: number;
  rarity: string; // "Common" | "Uncommon" | "Rare"
}

interface RelicDrop {
  tier: string;
  relicName: string;      // short: "A1 Relic"
  fullName: string;       // with tier: "Axi A1 Relic" — used for catalog lookup
  rewards: DropReward[];
}

export interface RelicFilters {
  search: string;
  tiers: string[];
  ownership: ("owned" | "notowned")[];
  vault: ("vaulted" | "unvaulted")[];
  completion: ("complete" | "incomplete")[];
  sortMode: "count" | "plat" | "ducats" | "az" | "za";
  ignoreFormaKuva: boolean;
}
export const RELIC_FILTERS_DEFAULT: RelicFilters = {
  search: "", tiers: [], ownership: [], vault: [], completion: [], sortMode: "count",
  ignoreFormaKuva: false,
};

interface Props {
  inventory: Record<string, InventoryItem>;
  refreshKey: number;
  colorblindMode?: boolean;
  filters: RelicFilters;
  onFiltersChange: (f: RelicFilters) => void;
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

function parseDropData(raw: unknown): RelicDrop[] {
  const relicsArray: any[] = Array.isArray((raw as any)?.relics) ? (raw as any).relics : [];

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
  const img = useImgLadder([src]);
  const base = { width: 44, height: 44, borderRadius: 6, flexShrink: 0 } as const;
  if (!img.src)
    return <div style={{ ...base, background: "rgba(255,255,255,.06)", display: "flex", alignItems: "center", justifyContent: "center", fontSize: 11, color: "#8b949e" }}>R</div>;
  return <img key={img.src} style={{ ...base, objectFit: "contain" }} src={img.src} alt="" loading="lazy" onError={img.onError} />;
}

const RARITY_BG: Record<string, string> = {
  Bronze: "rgba(205,127,50,.2)",
  Silver: "rgba(192,192,192,.15)",
  Gold:   "rgba(240,192,64,.2)",
};

function PartImg({ srcs, rarity }: { srcs: (string | undefined)[]; rarity?: string }) {
  const { src, onError } = useImgLadder(srcs);
  const base = { width: 40, height: 40, borderRadius: 4 } as const;
  if (!src) {
    const bg = rarity ? (RARITY_BG[rarity] ?? "rgba(255,255,255,.06)") : "rgba(255,255,255,.06)";
    return <div style={{ ...base, background: bg, display: "flex", alignItems: "center", justifyContent: "center", fontSize: 9, color: "rgba(255,255,255,.3)" }}>?</div>;
  }
  // key={src} forces React to unmount/remount the img when src changes,
  // preventing the broken-image icon from persisting between attempts
  return <img key={src} style={{ ...base, objectFit: "contain", display: "block" }} src={src} alt=""
    onError={onError} />;
}

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
  const state = isComplete ? "complete" : isOwned ? "owned" : "";
  const shortName = reward.itemName.replace(" Blueprint", "").replace("Prime", "P.").trim();
  return (
    <div
      className={["relic-rbox", `relic-rbox-${cls}`, state ? `relic-rbox-${state}` : "", isHighlighted ? "relic-rbox-highlight" : ""].join(" ").trim()}
      title={`${reward.itemName} — ${reward.rarity} (${reward.chance.toFixed(1)}%)`}
    >
      {/* Top-right corner: rarity label + optional colorblind checkmark stacked */}
      <span className="relic-corner-indicator">
        <span className={`relic-rarity-label relic-rl-${cls}`} title={reward.rarity}>
          {cls === "bronze" ? "C" : cls === "silver" ? "U" : "R"}
        </span>
        {colorblindMode && (isOwned || isComplete) && (
          <span className={`relic-cb-check relic-cb-${state}`}>{isComplete ? "✓✓" : "✓"}</span>
        )}
      </span>
      <PartImg srcs={imageSrcs} rarity={reward.rarity} />
      <span className="relic-rbox-name">{shortName}</span>
    </div>
  );
}

// ─── Relic card ───────────────────────────────────────────────────────────────

const REFINEMENT_SUFFIXES_CARD = ["intact", "exceptional", "flawless", "radiant"];
const REFINEMENT_LABELS_CARD   = ["Intact", "Except.", "Flawless", "Radiant"];

function isFormaOrKuva(itemName: string): boolean {
  return itemName.includes("Forma") || itemName === "Kuva";
}

function RelicCard({ drop, catalogRelicByName, inventory, ownedPrimeNames, searchQ, nameMap, colorblindMode, view, ignoreFormaKuva }: {
  drop: RelicDrop;
  catalogRelicByName: Map<string, CatalogItem>;
  inventory: Record<string, InventoryItem>;
  ownedPrimeNames: Set<string>;
  searchQ: string;
  nameMap: Map<string, CatalogItem>;
  colorblindMode: boolean;
  view: ViewMode;
  ignoreFormaKuva: boolean;
}) {
  const baseLower = drop.fullName.toLowerCase();

  // Per-refinement counts using catalog
  const refCounts = REFINEMENT_SUFFIXES_CARD.map((ref, i) => {
    const cat = catalogRelicByName.get(`${baseLower} ${ref}`);
    return { label: REFINEMENT_LABELS_CARD[i], count: cat ? (inventory[cat.unique_name]?.quantity ?? 0) : 0 };
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

  const cardClass = `relic-card${total === 0 ? " relic-card-unowned" : allComplete ? " relic-card-complete" : ""}`;

  if (view === "icons") {
    return (
      <div className={`${cardClass} relic-card-icon-only`} title={`${drop.fullName} ×${total}`}>
        <RelicImg src={cdnUrl(intactCat?.image_name)} />
        <span className="relic-icon-count">×{total}</span>
      </div>
    );
  }

  if (view === "list" || view === "list-compact") {
    const refCompact = refCounts.filter(r => r.count > 0)
      .map(r => `${r.label[0].toUpperCase()}:${r.count}`)
      .join(" ");
    return (
      <div className={`${cardClass} relic-card-row`}>
        {view === "list" && <div className="relic-row-img"><RelicImg src={cdnUrl(intactCat?.image_name)} /></div>}
        <div className="relic-row-name">{drop.fullName}</div>
        {intactCat?.vaulted && <span className="vault-badge vault-yes" style={{ fontSize: 9 }}>🔒</span>}
        <span className="relic-row-total">×{total}</span>
        {refCompact && <span className="relic-row-refs">{refCompact}</span>}
      </div>
    );
  }

  if (view === "text-cards") {
    return (
      <div className={`${cardClass} relic-text-card`}>
        <div className="relic-card-left">
          <div className="relic-card-name">{drop.fullName}</div>
          {intactCat?.vaulted && <span className="vault-badge vault-yes">🔒 Vaulted</span>}
          <div className="relic-refinements">
            {refCounts.some(r => r.count > 0)
              ? refCounts.map(r => (
                <span key={r.label} className={`relic-ref ${r.count > 0 ? "relic-ref-owned" : "relic-ref-zero"}`}>
                  {r.count} {r.label}
                </span>
              ))
              : <span className="relic-ref relic-ref-owned">Total: {total}</span>}
          </div>
        </div>
        <div className="relic-text-rewards">
          {drop.rewards.map((r, i) => (
            <div key={i} className={`relic-text-reward relic-rarity-${r.rarity?.toLowerCase() ?? "common"}`}>
              {r.itemName}
            </div>
          ))}
        </div>
      </div>
    );
  }

  return (
    <div className={cardClass}>
      <div className="relic-card-left">
        <div className="relic-card-icon-row">
          <RelicImg src={cdnUrl(intactCat?.image_name)} />
          <span className="relic-total">×{total}</span>
          {colorblindMode && allComplete && <span className="relic-cb-relic-check" title="All rewards obtained">✓✓</span>}
        </div>
        <div className="relic-card-name">{drop.fullName}</div>
        {intactCat?.vaulted && <span className="vault-badge vault-yes">🔒 Vaulted</span>}
        <div className="relic-refinements">
          {refCounts.some(r => r.count > 0)
            ? refCounts.map(r => (
              <span key={r.label} className={`relic-ref ${r.count > 0 ? "relic-ref-owned" : "relic-ref-zero"}`}>
                {r.count} {r.label}
              </span>
            ))
            : <span className="relic-ref relic-ref-owned">Total: {total}</span>
          }
        </div>
      </div>

      <div className="relic-rewards-grid">
        {slots.map((r, i) => {
          if (!r) return (
            <div key={i} className="relic-rbox relic-rbox-empty">
              <PartImg srcs={[]} rarity={undefined} />
              <span className="relic-rbox-name">—</span>
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
            cdnUrl(imageItem?.image_name),
            // 2. Parent prime warframe/weapon image
            cdnUrl(primeImageItem?.image_name),
            // 3. Construct from catalog unique_name: "YareliPrimeBlueprint" → "YareliPrime.png"
            (() => {
              const seg = (catalogItem?.unique_name ?? "").split("/").pop() ?? "";
              const file = seg.replace(/Blueprint$/, "");
              return cdnUrl(file ? `${file}.png` : undefined);
            })(),
            // 4. Construct from parent prime name: "Yareli Prime" → "YareliPrime.png"
            cdnUrl(primeName ? `${primeName.replace(/\s+/g, "")}.png` : undefined),
            // 5. Strip "Blueprint" from item name: "Forma Blueprint" → "Forma.png"
            cdnUrl(`${r.itemName.replace(" Blueprint", "").replace(/\s+/g, "")}.png`),
            // 6. Strip leading count prefix: "2X Forma" → "Forma.png"
            cdnUrl(`${r.itemName.replace(/^\d+[xX]\s*/, "").replace(" Blueprint", "").replace(/\s+/g, "")}.png`),
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
              isHighlighted={searchQ.length > 1 && r.itemName.toLowerCase().includes(searchQ)}
              colorblindMode={colorblindMode}
            />
          );
        })}
      </div>
    </div>
  );
}

// ─── Planner ─────────────────────────────────────────────────────────────────

const DROP_RATES = {
  intact:      { Common: 0.2533, Uncommon: 0.11,  Rare: 0.02 },
  exceptional: { Common: 0.2333, Uncommon: 0.13,  Rare: 0.04 },
  flawless:    { Common: 0.20,   Uncommon: 0.17,  Rare: 0.06 },
  radiant:     { Common: 0.1667, Uncommon: 0.20,  Rare: 0.10 },
} as const;

type PlannerTier = keyof typeof DROP_RATES;
const PLANNER_TIERS: PlannerTier[] = ["intact", "exceptional", "flawless", "radiant"];
const TIER_LABEL: Record<PlannerTier, string> = {
  intact: "Intact", exceptional: "Except.", flawless: "Flawless", radiant: "Radiant",
};

function wfmNorm(name: string): string {
  return name.toLowerCase().replace(/[^a-z0-9]+/g, "_").replace(/^_|_$/g, "");
}

function computeEV(
  rewards: DropReward[],
  tier: PlannerTier,
  vals: number[],
  squadSize: number,
): number {
  const rates = DROP_RATES[tier];
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

  // Load WFM items to build name→slug lookup, then fetch cached prices
  useEffect(() => {
    invoke<{ item_name: string; url_name: string }[]>("fetch_wfm_items")
      .then(items => {
        const lookup = new Map<string, string>();
        for (const w of items) lookup.set(wfmNorm(w.item_name), w.url_name);
        return lookup;
      })
      .then(lookup => {
        invoke<Record<string, number | null>>("wfm_get_cached_prices")
          .then(raw => {
            const m = new Map<string, number>();
            for (const [slug, price] of Object.entries(raw)) {
              if (price != null) m.set(slug, price);
            }
            // Also index by normalized item name for direct lookup
            for (const [norm, slug] of lookup) {
              const p = m.get(slug);
              if (p != null) m.set(norm, p);
            }
            setPlatPrices(m);
          })
          .catch(() => {});
      })
      .catch(() => {});
  }, []);

  const REFINEMENT_SUFFIXES = ["intact", "exceptional", "flawless", "radiant"];

  const getOwnedByTier = useCallback((drop: RelicDrop) => {
    const base = drop.fullName.toLowerCase();
    return REFINEMENT_SUFFIXES.reduce<Record<string, number>>((acc, ref) => {
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
          PLANNER_TIERS.map(t => [t, computeEV(rewards, t, vals, squadSize)])
        ) as Record<PlannerTier, number>;

        const bestTier = PLANNER_TIERS.reduce((best, t) =>
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
      <span className="planner-sort-arrow" style={{ visibility: col === sortCol ? "visible" : "hidden" }}>
        {sortDir === "desc" ? "▼" : "▲"}
      </span>
    );
  }

  return (
    <div className="planner-wrap">
      {/* Controls */}
      <div className="planner-controls">
        <div className="planner-control-group">
          <span className="planner-label">Metric</span>
          <button className={`fchip${metric === "plat"  ? " fchip-on" : ""}`} onClick={() => setMetric("plat")}>Platinum</button>
          <button className={`fchip${metric === "ducat" ? " fchip-on" : ""}`} onClick={() => setMetric("ducat")}>Ducats</button>
        </div>
        <div className="planner-control-group">
          <span className="planner-label">Squad</span>
          {([1, 2, 3, 4] as const).map(n => (
            <button key={n} className={`fchip${squadSize === n ? " fchip-on" : ""}`} onClick={() => setSquadSize(n)}>
              {n === 1 ? "Solo" : `${n}p`}
            </button>
          ))}
        </div>
        <div className="planner-control-group">
          <span className="planner-label">Era</span>
          {(["lith","meso","neo","axi"] as const).map(t => (
            <button key={t} className={`fchip${tierFilter.includes(t) ? " fchip-on" : ""}`}
              onClick={() => setTierFilter(prev => prev.includes(t) ? prev.filter(x => x !== t) : [...prev, t])}>
              {t[0].toUpperCase() + t.slice(1)}
            </button>
          ))}
        </div>
        <div className="planner-control-group">
          <button className={`fchip${vaultFilter === "unvaulted" ? " fchip-on" : ""}`} onClick={() => setVaultFilter(v => v === "unvaulted" ? "all" : "unvaulted")}>Unvaulted</button>
          <button className={`fchip${vaultFilter === "vaulted"   ? " fchip-on" : ""}`} onClick={() => setVaultFilter(v => v === "vaulted"   ? "all" : "vaulted")}>Vaulted</button>
          <button className={`fchip${ownedOnly ? " fchip-on" : ""}`} onClick={() => setOwnedOnly(v => !v)}>Owned Only</button>
        </div>
        <span className="planner-count" style={{ marginLeft: "auto" }}>{plannerRows.length} relics</span>
      </div>

      {/* Column header */}
      <div className="planner-header-row">
        <div className="planner-col-name">
          <button className={`planner-col-sortable${sortCol === "name" ? " active" : ""}`} onClick={() => handleSort("name")}>
            Relic{sortArrow("name")}
          </button>
          <button className={`planner-col-sortable planner-col-owned-hdr${sortCol === "owned" ? " active" : ""}`} onClick={() => handleSort("owned")}>
            Owned{sortArrow("owned")}
          </button>
        </div>
        {PLANNER_TIERS.map(t => (
          <button key={t} className={`planner-col-tier planner-col-sortable${sortCol === t ? " active" : ""}`} onClick={() => handleSort(t)}>
            {TIER_LABEL[t]}{sortArrow(t)}
          </button>
        ))}
        <button className={`planner-col-refine planner-col-sortable${sortCol === "gain" ? " active" : ""}`} onClick={() => handleSort("gain")}>
          Refine gain{sortArrow("gain")}
        </button>
        <div className="planner-expand-spacer" aria-hidden />
      </div>

      {/* Rows */}
      <div className="planner-list">
        {plannerRows.length === 0 ? (
          <div className="empty-msg">No relics match. Try turning off Owned Only.</div>
        ) : plannerRows.map(({ drop, rewards, vals, evByTier, bestTier, totalOwned, vaulted }) => {
          const isOpen = expanded === drop.fullName;
          const gain   = evByTier.radiant - evByTier.intact;
          return (
            <div key={drop.fullName} className="planner-row">
              <div className="planner-row-main" onClick={() => setExpanded(isOpen ? null : drop.fullName)}>
                <div className="planner-col-name">
                  <span className="planner-relic-name">{drop.fullName}</span>
                  {vaulted && <span className="vault-badge vault-yes" style={{ fontSize: 9 }}>🔒</span>}
                  <span className="planner-owned">×{totalOwned}</span>
                </div>
                {PLANNER_TIERS.map(t => (
                  <div key={t} className={`planner-col-tier planner-ev${t === bestTier ? " planner-ev-best" : ""}`}>
                    {evByTier[t] < 0.05 ? <span className="planner-ev-zero">—</span> : `${evByTier[t].toFixed(1)}${unit}`}
                  </div>
                ))}
                <div className="planner-col-refine">
                  {gain >= 0.1
                    ? <span className="planner-gain-pos">+{gain.toFixed(1)}{unit}</span>
                    : <span className="planner-gain-neg">{gain.toFixed(1)}{unit}</span>}
                </div>
                <button className="planner-expand-btn">{isOpen ? "▲" : "▼"}</button>
              </div>

              {isOpen && (
                <div className="planner-reward-detail">
                  <div className="planner-detail-tier-row">
                    {PLANNER_TIERS.map(t => (
                      <span key={t} className="planner-detail-tier-label">{TIER_LABEL[t]}: {DROP_RATES[t].Rare * 100}% rare</span>
                    ))}
                  </div>
                  {rewards.map((r, i) => {
                    const rates = DROP_RATES[bestTier];
                    const chance = rates[r.rarity as keyof typeof rates] ?? 0;
                    const cls = RARITY_CSS[r.rarity] ?? "bronze";
                    return (
                      <div key={i} className={`planner-reward-row planner-reward-${cls}`}>
                        <span className={`relic-rl-${cls} planner-reward-rarity`}>{r.rarity[0]}</span>
                        <span className="planner-reward-name">{r.itemName}</span>
                        <span className="planner-reward-chance">{(chance * 100).toFixed(2)}%</span>
                        <span className="planner-reward-val">{vals[i] > 0 ? `${vals[i]}${unit}` : "—"}</span>
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

export default function RelicHelper({ inventory, refreshKey, colorblindMode = false, filters, onFiltersChange }: Props) {
  const [plannerActive, setPlannerActive] = useState(false);
  const [relicView, setRelicView] = useState<ViewMode>(() =>
    (localStorage.getItem("ff-view-relic") as ViewMode | null) ?? "cards"
  );
  const [allItems,    setAllItems]    = useState<CatalogItem[]>([]);
  const [drops,       setDrops]       = useState<RelicDrop[]>([]);
  const [dropLoading, setDropLoading] = useState(false);
  const [dropError,   setDropError]   = useState(false);
  const [page,        setPage]        = useState(0);
  const PAGE_SIZE = 30;

  const { search, tiers, ownership, vault, completion, sortMode, ignoreFormaKuva } = filters;
  const set = <K extends keyof RelicFilters>(k: K, v: RelicFilters[K]) => onFiltersChange({ ...filters, [k]: v });

  const loadDrops = useCallback((force = false) => {
    setDropLoading(true);
    setDropError(false);
    invoke<unknown>("get_drop_data", { force })
      .then(d => setDrops(parseDropData(d)))
      .catch(() => setDropError(true))
      .finally(() => setDropLoading(false));
  }, []);

  useEffect(() => {
    invoke<CatalogItem[]>("get_all_items").then(setAllItems).catch(() => {});
  }, [refreshKey]);

  useEffect(() => {
    loadDrops();
  }, [loadDrops]);

  const nameMap = useMemo(() => {
    const m = new Map<string, CatalogItem>();
    for (const i of allItems) m.set(i.name.toLowerCase(), i);
    return m;
  }, [allItems]);

  const catalogRelicByName = useMemo(() => {
    const m = new Map<string, CatalogItem>();
    for (const i of allItems) if (i.category === "Relics") m.set(i.name.toLowerCase(), i);
    return m;
  }, [allItems]);


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
  const REFINEMENT_SUFFIXES = ["intact", "exceptional", "flawless", "radiant"];

  const getTotal = useCallback((drop: RelicDrop): number => {
    if (!drop?.fullName) return 0;
    const base = drop.fullName.toLowerCase();
    return REFINEMENT_SUFFIXES.reduce((sum, ref) => {
      const cat = catalogRelicByName.get(`${base} ${ref}`);
      return sum + (cat ? (inventory[cat.unique_name]?.quantity ?? 0) : 0);
    }, 0);
  }, [catalogRelicByName, inventory]);

  const searchQ = search.toLowerCase();

  const visibleDrops = useMemo(() => drops
    .filter(d => {
      if (!searchQ) return true;
      return (d.fullName ?? "").toLowerCase().includes(searchQ)
        || (d.relicName ?? "").toLowerCase().includes(searchQ)
        || d.rewards.some(r => (r.itemName ?? "").toLowerCase().includes(searchQ));
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
        const CHANCES: Record<string, number> = { Common: 0.2533, Uncommon: 0.11, Rare: 0.02 };
        const avg = (d: RelicDrop) => d.rewards.reduce((s, r) => {
          const cat = findCatalogItemGlobal(r.itemName, nameMap);
          return s + (cat?.ducats ?? 0) * (CHANCES[r.rarity] ?? 0);
        }, 0);
        return avg(b) - avg(a) || (a.relicName ?? "").localeCompare(b.relicName ?? "");
      }
      if (sortMode === "za") return (b.fullName ?? "").localeCompare(a.fullName ?? "");
      return (a.fullName ?? "").localeCompare(b.fullName ?? ""); // az + plat fallback
    }),
  [drops, searchQ, tiers, ownership, vault, completion, sortMode, getTotal, catalogRelicByName, nameMap, inventory, ownedPrimeNames]);

  const ownedCount = useMemo(() =>
    drops.filter(d => getTotal(d) > 0).length,
  [drops, getTotal]);

  useEffect(() => { setPage(0); }, [filters]);

  const pagedDrops = visibleDrops.slice(page * PAGE_SIZE, (page + 1) * PAGE_SIZE);
  const totalPages = Math.ceil(visibleDrops.length / PAGE_SIZE);

  const searchMatchesReward = searchQ.length > 1
    && drops.some(d => d.rewards.some(r => (r.itemName ?? "").toLowerCase().includes(searchQ)));

  return (
    <div className="relic-helper">
      {/* Sub-tab bar */}
      <div className="relic-subtab-bar">
        <button className={`relic-subtab${!plannerActive ? " active" : ""}`} onClick={() => setPlannerActive(false)}>Relics</button>
        <button className={`relic-subtab${plannerActive  ? " active" : ""}`} onClick={() => setPlannerActive(true)}>Planner</button>
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
        <input
          className="foundry-search" style={{ width: 220 }}
          placeholder="Relic name or item name…"
          value={search} onChange={e => set("search", e.target.value)}
        />
        <div className="filter-bar" style={{ border: "none", padding: 0, flex: 1, flexWrap: "wrap" }}>
          {(["Lith","Meso","Neo","Axi","Requiem"] as const).map(t => (
            <button key={t} className={`fchip ${tiers.includes(t.toLowerCase()) ? "fchip-on" : ""}`}
              onClick={() => set("tiers", toggle(tiers, t.toLowerCase()))}>{t}</button>
          ))}
          <span className="fbar-sep"/>
          <button className={`fchip ${ownership.includes("owned")   ? "fchip-on" : ""}`} onClick={() => set("ownership", toggle(ownership, "owned"))}>Owned</button>
          <button className={`fchip ${ownership.includes("notowned") ? "fchip-on" : ""}`} onClick={() => set("ownership", toggle(ownership, "notowned"))}>Not Owned</button>
          <span className="fbar-sep"/>
          <button className={`fchip ${vault.includes("vaulted")   ? "fchip-on" : ""}`} onClick={() => set("vault", toggle(vault, "vaulted"))}>Vaulted</button>
          <button className={`fchip ${vault.includes("unvaulted") ? "fchip-on" : ""}`} onClick={() => set("vault", toggle(vault, "unvaulted"))}>Unvaulted</button>
          <span className="fbar-sep"/>
          <button className={`fchip ${completion.includes("complete")   ? "fchip-on" : ""}`} onClick={() => set("completion", toggle(completion, "complete"))}>Completed</button>
          <button className={`fchip ${completion.includes("incomplete") ? "fchip-on" : ""}`} onClick={() => set("completion", toggle(completion, "incomplete"))}>Uncompleted</button>
          <button className={`fchip ${ignoreFormaKuva ? "fchip-on" : ""}`} onClick={() => set("ignoreFormaKuva", !ignoreFormaKuva)} title="Treat Forma and Kuva rewards as always obtained when checking completion">Ignore Forma/Kuva</button>
          <span className="fbar-sep"/>
          <span className="fbar-label">Sort:</span>
          <button className={`fchip ${sortMode === "count"  ? "fchip-on" : ""}`} onClick={() => set("sortMode", "count")}>Most Owned</button>
          <button className={`fchip ${sortMode === "plat"   ? "fchip-on" : ""}`} onClick={() => set("sortMode", "plat")}>Avg Plat</button>
          <button className={`fchip ${sortMode === "ducats" ? "fchip-on" : ""}`} onClick={() => set("sortMode", "ducats")}>Avg Ducats</button>
          <button className={`fchip ${sortMode === "az"     ? "fchip-on" : ""}`} onClick={() => set("sortMode", "az")}>A–Z</button>
          <button className={`fchip ${sortMode === "za"     ? "fchip-on" : ""}`} onClick={() => set("sortMode", "za")}>Z–A</button>
          <span className="fbar-sep"/>
          <button className="fchip fchip-reset" onClick={() => onFiltersChange(RELIC_FILTERS_DEFAULT)}>Show All</button>
          {dropError && <button className="btn-secondary" style={{ marginLeft: 4 }} onClick={() => loadDrops(true)}>↺ Retry</button>}
          <span style={{ marginLeft: "auto", fontSize: 11, color: "var(--muted)" }}>
            {dropLoading ? "Loading…" : `${visibleDrops.length} relics · ${ownedCount} owned`}
          </span>
          <ViewToggle view={relicView} onChange={v => { setRelicView(v); localStorage.setItem("ff-view-relic", v); }} />
          <HelpTip items={[
            { border: "#e8923a", icon: "C", label: "Common",   desc: "Bronze border — ~25% chance per run" },
            { border: "#c0c0c0", icon: "U", label: "Uncommon", desc: "Silver border — ~11% chance per run" },
            { border: "#f0c040", icon: "R", label: "Rare",     desc: "Gold border — ~2% chance per run" },
            { swatch: "rgba(63,185,80,.5)",  icon: "✓",  label: "Part owned",     desc: "Green box — blueprint or part in inventory" },
            { swatch: "rgba(240,192,64,.5)", icon: "✓✓", label: "Item complete",  desc: "Gold box — built warframe/weapon owned" },
          ]} />
        </div>
      </div>

      {searchMatchesReward && (
        <div style={{ padding: "4px 14px", fontSize: 11, color: "var(--accent)" }}>
          Showing relics that drop "<strong>{search}</strong>" — highlighted in blue
        </div>
      )}

      {visibleDrops.length > PAGE_SIZE && (
        <div className="relic-pagination">
          <button className="btn-secondary" disabled={page === 0} onClick={() => setPage(p => p - 1)}>← Prev</button>
          <span style={{ fontSize: 11, color: "var(--muted)" }}>
            {page + 1} / {totalPages} &nbsp;({visibleDrops.length} relics)
          </span>
          <button className="btn-secondary" disabled={page >= totalPages - 1} onClick={() => setPage(p => p + 1)}>Next →</button>
        </div>
      )}

      <div className={`relic-list relic-list-${relicView}`}>
        {visibleDrops.length === 0 ? (
          <div className="empty-msg">{dropLoading ? "Fetching drop data…" : "No relics match."}</div>
        ) : pagedDrops.map(drop => (
          <RelicCard
            key={drop.fullName}
            drop={drop}
            catalogRelicByName={catalogRelicByName}
            inventory={inventory}
            ownedPrimeNames={ownedPrimeNames}
            searchQ={searchQ}
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
