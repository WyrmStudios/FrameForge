import { useState, useEffect, useMemo, useRef, memo, useContext } from "react";
import { invoke } from "@tauri-apps/api/core";
import { ImgCacheDirContext } from "./ImgCacheDir";
import { listen } from "@tauri-apps/api/event";
import { HelpTip } from "./HelpTip";
import WfmTrading from "./WfmTrading";
import ItemMarketPopup from "./ItemMarketPopup";
import type { InventoryItem } from "./App";
import polMadurai  from "./assets/polarity/madurai.svg";
import polVazarin  from "./assets/polarity/vazarin.svg";
import polNaramon  from "./assets/polarity/naramon.svg";
import polZenurik  from "./assets/polarity/zenurik.svg";
import polUnairu   from "./assets/polarity/unairu.svg";
import polPenjaga  from "./assets/polarity/penjaga.svg";
import polUmbra    from "./assets/polarity/umbra.svg";

// ─── Types ────────────────────────────────────────────────────────────────────

interface CatalogItem {
  unique_name: string;
  name: string;
  category: string;
  image_name?: string;
  vaulted?: boolean | null;
  ducats?: number | null;
}

interface WfmItem { id: string; item_name: string; url_name: string; }
interface WfmPrice { url_name: string; sell_median?: number; }

interface CraftingJob { unique_name: string; item_name: string; completion_ms: number; }

export interface MarketFilters {
  search: string;
  ownership:  ("owned" | "notowned")[];
  conditions: ("dupes" | "itemowned" | "fullset" | "hasparts")[];
  vault:      ("vaulted" | "unvaulted")[];
  sortMode:   "plat" | "ducats" | "az" | "za";
  activeMarketTab: "trading" | "sets" | "mods" | "rivens" | "sisters";
}
export const MARKET_FILTERS_DEFAULT: MarketFilters = {
  search: "", ownership: [], conditions: [], vault: [], sortMode: "ducats",
  activeMarketTab: "trading",
};

interface RecipeComponent {
  unique_name: string;
  name: string;
  count: number;
  result_count: number;
  components: RecipeComponent[];
}

interface BlobRivenStat { tag: string; value: number; }
interface BlobRivenEntry {
  item_id:   string;
  item_type: string;
  mod_name:  string;   // pre-computed by Rust, persisted in cache
  /** "unrevealed" | "revealed" | "unlocked" */
  riven_state: "unrevealed" | "revealed" | "unlocked";
  compat:    string | null;
  challenge_type: string | null;
  challenge_complication: string | null;
  lvl_req:   number | null;
  polarity:  string | null;
  buffs:     BlobRivenStat[];
  curses:    BlobRivenStat[];
  mod_rank:  number;
  count:     number;
  rerolls:   number;
}

interface ModCopy {
  uniqueName: string;
  rank: number | null;
  count: number;
}

interface Props {
  inventory: Record<string, InventoryItem>;
  refreshKey: number;
  crafting: CraftingJob[];
  onWfmLoginChange?: (loggedIn: boolean) => void;
  filters: MarketFilters;
  onFiltersChange: (f: MarketFilters) => void;
  modCopiesMap?: Record<string, ModCopy[]>;
}

function toggle<T>(arr: T[], val: T): T[] {
  return arr.includes(val) ? arr.filter(x => x !== val) : [...arr, val];
}

// ─── Icons ────────────────────────────────────────────────────────────────────

function PlatIcon({ size = 14 }: { size?: number }) {
  return <img src="/platinum.webp" alt="plat" width={size} height={size} style={{ objectFit: "contain", flexShrink: 0 }} />;
}
function DucatIcon({ size = 14 }: { size?: number }) {
  return <img src="/ducats.webp" alt="ducat" width={size} height={size} style={{ objectFit: "contain", flexShrink: 0 }} />;
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

function fmt(n: number) { return n.toLocaleString(); }
function fmtPt(n: number) { return Math.round(n).toString(); }

function setName(itemName: string): string {
  const words = itemName.split(" ");
  const primeIdx = words.lastIndexOf("Prime");
  if (primeIdx >= 0) return words.slice(0, primeIdx + 1).join(" ");
  return words.slice(0, 2).join(" ");
}

function partLabel(itemName: string, set: string): string {
  return itemName.startsWith(set) ? itemName.slice(set.length).trim() || itemName : itemName;
}

function normalizeForWfm(name: string): string {
  return name.toLowerCase().replace(/[^a-z0-9]+/g, "_").replace(/^_|_$/g, "");
}

function flattenRecipeCounts(comps: RecipeComponent[], multiplier: number, out: Map<string, number>): void {
  for (const c of comps) {
    const total = multiplier * c.count;
    out.set(c.unique_name, Math.max(out.get(c.unique_name) ?? 0, total));
    if (c.components.length > 0) flattenRecipeCounts(c.components, total, out);
  }
}

function ItemImg({ imageName, size = 32 }: { imageName?: string; size?: number }) {
  const baseUrl = useContext(ImgCacheDirContext);
  const [localFailed, setLocalFailed] = useState(false);
  const [cdnFailed,   setCdnFailed]   = useState(false);
  const s = { width: size, height: size, objectFit: "contain" as const, flexShrink: 0, borderRadius: 4 };
  if (!imageName || cdnFailed)
    return <span style={{ ...s, background: "rgba(255,255,255,.06)", border: "1px solid #30363d", display: "flex", alignItems: "center", justifyContent: "center", fontSize: size * .3, color: "#8b949e" }}>P</span>;
  const useLocal = Boolean(baseUrl) && !localFailed;
  const src = useLocal
    ? `${baseUrl}/${imageName}`
    : `https://cdn.warframestat.us/img/${imageName}`;
  return <img style={s} src={src} alt="" loading="lazy"
    onError={() => useLocal ? setLocalFailed(true) : setCdnFailed(true)} />;
}

// ─── Set card ─────────────────────────────────────────────────────────────────

interface SetPart { item: CatalogItem; qty: number; required_count: number; sellMedian?: number; loading: boolean; urlName: string; }

function SetCard({ setKey, parts, parentItem, setPrice, setPriceLoading, pricesFetched, crafting, onCardClick, onPartClick }: {
  setKey: string; parts: SetPart[]; parentItem?: CatalogItem;
  setPrice?: WfmPrice; setPriceLoading: boolean; pricesFetched: boolean;
  crafting: CraftingJob[]; onCardClick?: () => void;
  onPartClick?: (urlName: string, displayName: string, imageName?: string) => void;
}) {
  const totalDucats  = parts.reduce((s, p) => s + (p.item.ducats ?? 0) * p.qty, 0);
  const ownedCount   = parts.filter(p => p.qty > 0).length;
  const isComplete   = ownedCount === parts.length;
  const hasDupes     = parts.some(p => p.qty > p.required_count);
  const isCrafting   = crafting.some(c =>
    c.unique_name === parentItem?.unique_name ||
    parts.some(p => p.item.unique_name === c.unique_name)
  );

  return (
    <div className={`market-card${isComplete ? " market-card-complete" : ""}`}>
      <div className={`market-card-left${onCardClick ? " market-card-clickable" : ""}`} onClick={onCardClick} title={onCardClick ? "View orders & prices" : undefined}>
        <div style={{ position: "relative", display: "inline-block" }}>
          <ItemImg imageName={parentItem?.image_name} size={64} />
          {isCrafting && (
            <span style={{ position: "absolute", top: -4, right: -6, fontSize: 13 }} title="Building in Foundry">⚒</span>
          )}
        </div>
        <div className="market-set-name">{setKey}</div>
        <div className="market-set-badges">
          {isComplete && <span className="mset-badge mset-complete">✓ Complete</span>}
          {!isComplete && <span className="mset-badge mset-parts">{ownedCount}/{parts.length}</span>}
          {hasDupes && <span className="mset-badge mset-dupes">+ Dupes</span>}
        </div>
        <div className="market-set-price-box">
          {setPriceLoading ? (
            <span className="market-price-spin">…</span>
          ) : setPrice?.sell_median ? (
            <div className="market-set-price">
              <PlatIcon size={16} />
              <span className="market-price-big">{fmtPt(setPrice.sell_median)}</span>
              <span className="market-price-lbl">set</span>
            </div>
          ) : pricesFetched ? (
            <span className="market-price-na">—</span>
          ) : null}
        </div>
      </div>

      <div className="market-card-right">
        {parts.map(part => {
          const qty      = part.qty;
          const qtyClass = qty === 0 ? "mqty-zero" : qty === 1 ? "mqty-one" : "mqty-dupe";
          const canClick = !!onPartClick;
          return (
            <div
              key={part.item.unique_name}
              className={`market-part-row${qty === 0 ? " part-missing" : ""}${canClick ? " market-part-clickable" : ""}`}
              onClick={canClick ? () => onPartClick(part.urlName, part.item.name, part.item.image_name ?? undefined) : undefined}
              title={canClick ? "View orders & prices" : undefined}
            >
              <DucatIcon size={12} />
              <span className="mpart-ducat-val">{part.item.ducats ?? "—"}</span>
              <span className="mpart-sep">/</span>
              <PlatIcon size={12} />
              <span className="mpart-plat-val">
                {part.loading ? "…" : part.sellMedian ? fmtPt(part.sellMedian) : "—"}
              </span>
              <span className="mpart-sep">/</span>
              <span className="mpart-name">{partLabel(part.item.name, setKey)}</span>
              <span className={`mpart-qty ${qtyClass}`}>{qty}</span>
            </div>
          );
        })}
        {totalDucats > 0 && (
          <div className="mpart-totals"><DucatIcon size={11} /> {fmt(totalDucats)} ducats total</div>
        )}
      </div>
    </div>
  );
}

// ─── Market Helper ────────────────────────────────────────────────────────────

export default function MarketHelper({ inventory, refreshKey, crafting, onWfmLoginChange, filters, onFiltersChange, modCopiesMap = {} }: Props) {
  const [allItems, setAllItems]           = useState<CatalogItem[]>([]);
  const [wfmItems, setWfmItems]           = useState<WfmItem[]>([]);
  const [wfmLoading, setWfmLoading]       = useState(false);
  const [wfmError, setWfmError]           = useState(false);
  const [prices, setPrices]               = useState<Map<string, WfmPrice>>(new Map());
  const [wfmBadge, setWfmBadge]           = useState(0);
  const [wfmUsername, setWfmUsername]     = useState<string | null>(null);
  const [auctionRefreshKey, setAuctionRefreshKey] = useState(0);
  const [popup, setPopup] = useState<{ urlName: string; displayName: string; imageName?: string; prefillModRank?: number } | null>(null);
  const [recipeCountMap, setRecipeCountMap]       = useState<Map<string, number>>(new Map());
  const [rivens, setRivens]               = useState<BlobRivenEntry[]>([]);
  const { search, ownership, conditions, vault, sortMode, activeMarketTab } = filters;
  const set = <K extends keyof MarketFilters>(k: K, v: MarketFilters[K]) => onFiltersChange({ ...filters, [k]: v });

  useEffect(() => {
    invoke<CatalogItem[]>("get_all_items").then(setAllItems).catch(() => {});
  }, [refreshKey]);

  // Reflect WFM login state immediately — App.tsx loads the JWT into Rust on startup,
  // so wfm_get_session succeeds even before the Trading tab has been opened.
  useEffect(() => {
    invoke<[string, string] | null>("wfm_get_session")
      .then(existing => { if (existing) setWfmUsername(existing[0]); })
      .catch(() => {});
  }, []); // eslint-disable-line

  // Propagate login state to parent
  useEffect(() => {
    onWfmLoginChange?.(!!wfmUsername);
  }, [wfmUsername]); // eslint-disable-line

  // Start the Rust-side WFM queue drain thread once on mount.
  useEffect(() => {
    invoke("start_wfm_queue").catch(() => {});
  }, []); // eslint-disable-line

  // Fetch rivens from cache whenever the tab is opened or data refreshes.
  useEffect(() => {
    if (activeMarketTab === "rivens") {
      invoke<BlobRivenEntry[]>("get_rivens").then(setRivens).catch(() => {});
    }
  }, [activeMarketTab, refreshKey]);

  // Load prices already cached in inventory_state_cache (survive restarts).
  useEffect(() => {
    invoke<Record<string, number | null>>("wfm_get_cached_prices")
      .then(cached => {
        if (!cached) return;
        setPrices(prev => {
          const m = new Map(prev);
          for (const [urlName, price] of Object.entries(cached)) {
            m.set(urlName, { url_name: urlName, sell_median: price ?? undefined });
          }
          return m;
        });
      })
      .catch(() => {});
  }, []); // eslint-disable-line


  // Listen for prices arriving from the Rust queue drain thread.
  // Batch updates with rAF so bursts of events don't cause a re-render per price.
  const pendingPrices = useRef<Map<string, WfmPrice>>(new Map());
  const priceRafRef   = useRef<number | null>(null);
  useEffect(() => {
    const unlisten = listen<{ url_name: string; sell_median: number | null }>(
      "wfm-price-update",
      ({ payload }) => {
        pendingPrices.current.set(payload.url_name, { url_name: payload.url_name, sell_median: payload.sell_median ?? undefined });
        if (!priceRafRef.current) {
          priceRafRef.current = requestAnimationFrame(() => {
            priceRafRef.current = null;
            const batch = new Map(pendingPrices.current);
            pendingPrices.current.clear();
            setPrices(prev => {
              const m = new Map(prev);
              for (const [k, v] of batch) m.set(k, v);
              return m;
            });
          });
        }
      }
    );
    return () => { unlisten.then(fn => fn()); };
  }, []); // eslint-disable-line

  // Fetch WFM item list (used to build the name→slug lookup).
  useEffect(() => {
    setWfmLoading(true);
    setWfmError(false);
    invoke<WfmItem[]>("fetch_wfm_items")
      .then(items => {
        setWfmItems(items);
        if (!items.length) setWfmError(true);
      })
      .catch(() => { setWfmError(true); })
      .finally(() => setWfmLoading(false));
  }, []);

  const wfmLookup = useMemo(() => {
    const map = new Map<string, string>();

    // Pass 1: exact matches — highest priority, never overwritten
    for (const w of wfmItems) {
      map.set(normalizeForWfm(w.item_name), w.url_name);
    }

    // Pass 2: fill gaps only — add Blueprint ↔ no-Blueprint aliases for keys
    // that don't already have an exact entry, so we handle WFM's inconsistency
    // (some items listed with "Blueprint" suffix, some without)
    for (const w of wfmItems) {
      const key = normalizeForWfm(w.item_name);
      if (key.endsWith("_blueprint")) {
        // WFM has "…Blueprint" → also expose without suffix for catalog names that omit it
        const stripped = key.slice(0, -"_blueprint".length);
        if (!map.has(stripped)) map.set(stripped, w.url_name);
      } else {
        // WFM has no "Blueprint" → also expose with suffix for catalog names that include it
        const withBp = key + "_blueprint";
        if (!map.has(withBp)) map.set(withBp, w.url_name);
      }
    }

    return map;
  }, [wfmItems]);

  const primeItems = useMemo(() =>
    allItems.filter(i =>
      i.name.includes("Prime") &&
      // Include items with known ducat value OR any blueprint (even if ducats not yet catalogued)
      (i.ducats != null || i.name.endsWith("Blueprint"))
    ),
  [allItems]);

  const parentItems = useMemo(() => {
    const map = new Map<string, CatalogItem>();
    for (const i of allItems) {
      if (i.name.includes("Prime") && ["Warframes","Primary","Secondary","Melee","Companions","Archwing"].includes(i.category))
        map.set(i.name, i);
    }
    return map;
  }, [allItems]);


  // Load recipe trees for all prime set parents so we know the required count per component.
  // Accounts for multi-count cases like Aksomati Prime (2× Barrel, 2× Receiver).
  useEffect(() => {
    if (parentItems.size === 0) return;
    const uniqueNames = Array.from(parentItems.values()).map(p => p.unique_name);
    invoke<Record<string, RecipeComponent[]>>("get_recipes_bulk", { uniqueNames })
      .then(result => {
        const map = new Map<string, number>();
        for (const comps of Object.values(result)) flattenRecipeCounts(comps, 1, map);
        setRecipeCountMap(map);
      })
      .catch(() => {});
  }, [parentItems]);

  // item name (lowercase) → image_name — used by the Trading tab edit popup
  const imageMap = useMemo(() => {
    const m = new Map<string, string>();
    for (const i of allItems) if (i.image_name) m.set(i.name.toLowerCase(), i.image_name);
    return m;
  }, [allItems]);

  // ducats lookup by name for fallback (e.g. "Chassis" → 15 so "Chassis Blueprint" also gets 15)
  const ducatsByName = useMemo(() => {
    const m = new Map<string, number>();
    for (const i of allItems) if (i.ducats != null) m.set(i.name, i.ducats);
    return m;
  }, [allItems]);

  const sets = useMemo(() => {
    const map = new Map<string, CatalogItem[]>();
    for (const item of primeItems) {
      const key = setName(item.name);
      if (!map.has(key)) map.set(key, []);
      map.get(key)!.push(item);
    }

    for (const [key, parts] of map) {
      // 1. Dedup by display label — keep the item with ducats; drop the bare version
      //    when a blueprint counterpart exists (e.g. remove "Chassis" when "Chassis Blueprint" exists)
      const byLabel = new Map<string, CatalogItem>();
      for (const part of parts) {
        const label = partLabel(part.name, key);
        const existing = byLabel.get(label);
        if (!existing || (part.ducats != null && existing.ducats == null)) {
          byLabel.set(label, part);
        }
      }
      // 2. Remove built component rows when a Blueprint variant exists in the set.
      //    Exclude the plain "Blueprint" label itself — that's the main warframe blueprint,
      //    not a "something Blueprint" compound — so it must never be removed.
      const bpBaseLabels = new Set(
        [...byLabel.keys()]
          .filter(l => l.endsWith("Blueprint") && l !== "Blueprint")
          .map(l => l.replace(/ Blueprint$/, ""))
      );
      const deduped = [...byLabel.values()].filter(p => !bpBaseLabels.has(partLabel(p.name, key)));

      // 3. Inherit ducats from base component when blueprint lacks them.
      //    "Chassis Blueprint" inherits from "Revenant Prime Chassis" (15 ducats).
      const augmented = deduped.map(p => {
        if (p.ducats != null) return p;
        if (p.name.endsWith("Blueprint")) {
          const baseName = p.name.replace(/ Blueprint$/, "");
          const d = ducatsByName.get(baseName) ?? ducatsByName.get(`${key} ${baseName.slice(key.length).trim()}`);
          if (d != null) return { ...p, ducats: d };
        }
        return p;
      });

      // 4. Drop parts that still have no ducat value after inheritance — these are
      //    exalted weapon blueprints (Talons, Artemis Bow…) that are not relic drops.
      const finalParts = augmented.filter(p => p.ducats != null);

      // 5. Remove sets that end up with 0 tradeable parts — these are exalted abilities
      //    (Artemis Bow Prime, Balefire Charger Prime…) or single-unit extractors that
      //    aren't obtainable from relics. An empty set trivially shows as "Complete".
      if (finalParts.length === 0) { map.delete(key); continue; }
      map.set(key, finalParts);
    }
    return map;
  }, [primeItems, allItems, ducatsByName]);

  const totalDucats = useMemo(() =>
    primeItems.reduce((s, i) => s + (i.ducats ?? 0) * (inventory[i.unique_name]?.quantity ?? 0), 0),
  [primeItems, inventory]);

  const dupeDucats = useMemo(() =>
    primeItems.reduce((s, i) => s + (i.ducats ?? 0) * Math.max(0, (inventory[i.unique_name]?.quantity ?? 0) - (recipeCountMap.get(i.unique_name) ?? 1)), 0),
  [primeItems, inventory, recipeCountMap]);

  // Enqueue only the slugs the Market tab actually displays, owned sets first.
  // Runs once when wfmLookup and sets are both ready; Rust deduplicates if called again.
  useEffect(() => {
    if (wfmLookup.size === 0 || sets.size === 0) return;

    const getSetUrls = (setKey: string, parts: CatalogItem[]): string[] => {
      const urls: string[] = [];
      const setUrl = wfmLookup.get(normalizeForWfm(setKey + " Set"));
      if (setUrl) urls.push(setUrl);
      for (const p of parts) {
        const url = wfmLookup.get(normalizeForWfm(p.name)) ?? normalizeForWfm(p.name);
        if (!urls.includes(url)) urls.push(url);
      }
      return urls;
    };

    const owned: string[] = [];
    const unowned: string[] = [];
    const seen = new Set<string>();

    for (const [key, parts] of sets) {
      const isOwned = parts.some(p => (inventory[p.unique_name]?.quantity ?? 0) > 0);
      for (const url of getSetUrls(key, parts)) {
        if (seen.has(url)) continue;
        seen.add(url);
        (isOwned ? owned : unowned).push(url);
      }
    }

    // Owned sets are queued first so they appear quickly; unowned follow.
    invoke("wfm_queue_prices", { urlNames: [...owned, ...unowned] }).catch(() => {});
  }, [wfmLookup.size, sets.size]); // eslint-disable-line

  const visibleSets = useMemo(() => {
    const q = search.toLowerCase();
    return Array.from(sets.entries())
      .filter(([key]) => !q || key.toLowerCase().includes(q))
      .filter(([key, parts]) => {
        const ownedAny    = parts.some(p => (inventory[p.unique_name]?.quantity ?? 0) > 0);
        const parent      = parentItems.get(key);
        // "Item owned" = the fully built item appears in inventory under its display name.
        // inventory[key] uses the name-based index (e.g. "Ash Prime" → InventoryItem).
        const isItemOwned = (inventory[key]?.quantity ?? 0) > 0;

        // Group 1: Owned / Not Owned — checks whether the built item is in inventory
        if (ownership.length > 0 && ownership.length < 2) {
          if (ownership.includes("owned")    && !isItemOwned) return false;
          if (ownership.includes("notowned") &&  isItemOwned) return false;
        }

        // Group 2: specific conditions (OR — set matches if it satisfies ANY selected)
        if (conditions.length > 0) {
          const hasDupes   = parts.some(p => (inventory[p.unique_name]?.quantity ?? 0) > (recipeCountMap.get(p.unique_name) ?? 1));
          const isFullSet  = parts.every(p => (inventory[p.unique_name]?.quantity ?? 0) > 0);
          const ok = conditions.some(c =>
            (c === "hasparts"  && ownedAny)    ||
            (c === "dupes"     && hasDupes)    ||
            (c === "fullset"   && isFullSet)   ||
            (c === "itemowned" && isItemOwned)
          );
          if (!ok) return false;
        }

        // Group 3: Vaulted / Unvaulted
        if (vault.length > 0 && vault.length < 2) {
          const isVaulted = parent
            ? parent.vaulted === true
            : parts.some(p => p.vaulted === true);
          if (vault.includes("vaulted")   && !isVaulted) return false;
          if (vault.includes("unvaulted") &&  isVaulted) return false;
        }

        return true;
      })
      .sort(([aKey, aParts], [bKey, bParts]) => {
        if (sortMode === "ducats") {
          const ad = aParts.reduce((s, p) => s + (p.ducats ?? 0) * (inventory[p.unique_name]?.quantity ?? 0), 0);
          const bd = bParts.reduce((s, p) => s + (p.ducats ?? 0) * (inventory[p.unique_name]?.quantity ?? 0), 0);
          return bd - ad || aKey.localeCompare(bKey);
        }
        if (sortMode === "plat") {
          const getSetPrice = (key: string) => {
            const url = wfmLookup.get(normalizeForWfm(key + " Set")) ?? normalizeForWfm(key + " Set");
            return prices.get(url)?.sell_median ?? 0;
          };
          return getSetPrice(bKey) - getSetPrice(aKey) || aKey.localeCompare(bKey);
        }
        if (sortMode === "za") return bKey.localeCompare(aKey);
        return aKey.localeCompare(bKey); // az
      });
  }, [sets, inventory, ownership, conditions, vault, sortMode, search, parentItems, prices, wfmLookup, recipeCountMap]);

  return (
    <div className="market-helper">
      {/* ── Market tab strip ── */}
      <div className="market-tab-strip">
        <button className={activeMarketTab === "trading" ? "active" : ""} onClick={() => { set("activeMarketTab", "trading"); setWfmBadge(0); }}>
          Trading {wfmBadge > 0 && <span className="market-tab-badge">{wfmBadge}</span>}
        </button>
        <button className={activeMarketTab === "sets" ? "active" : ""} onClick={() => set("activeMarketTab", "sets")}>
          Prime Sets
        </button>
        <button className={activeMarketTab === "mods" ? "active" : ""} onClick={() => set("activeMarketTab", "mods")}>
          Mods &amp; Arcanes
        </button>
        <button className={activeMarketTab === "rivens" ? "active" : ""} onClick={() => set("activeMarketTab", "rivens")}>
          Rivens
        </button>
        <button className={activeMarketTab === "sisters" ? "active" : ""} onClick={() => set("activeMarketTab", "sisters")}>
          Variants
        </button>
      </div>

      {/* Keep WfmTrading mounted at all times so auction/whisper state isn't lost on tab switch */}
      <div style={{ display: activeMarketTab === "trading" ? "flex" : "none", flex: 1, flexDirection: "column", minHeight: 0, overflow: "hidden" }}>
        <WfmTrading
          wfmLookup={wfmLookup}
          wfmItems={wfmItems}
          imageMap={imageMap}
          inventory={inventory}
          onNewWhisper={() => { if (activeMarketTab !== "trading") setWfmBadge(n => n + 1); }}
          onLoginChange={u => setWfmUsername(u)}
          auctionRefreshKey={auctionRefreshKey}
        />
      </div>

      {activeMarketTab === "mods" && (
        <ModsTab
          allItems={allItems}
          inventory={inventory}
          wfmLookup={wfmLookup}
          prices={prices}
          modCopiesMap={modCopiesMap}
          onOpenPopup={(urlName, displayName, imageName, prefillModRank) => {
            invoke("wfm_queue_price_priority", { urlName }).catch(() => {});
            setPopup({ urlName, displayName, imageName, prefillModRank });
          }}
        />
      )}

      {activeMarketTab === "rivens" && (
        <RivensTab rivens={rivens} allItems={allItems} wfmUsername={wfmUsername} onAuctionPosted={() => setAuctionRefreshKey(k => k + 1)} />
      )}

      {activeMarketTab === "sisters" && (
        <div className="market-placeholder">
          <p>Sisters / Tenet weapons market coming soon.</p>
        </div>
      )}

      {activeMarketTab === "sets" && <>
      <div className="market-header">
        <input className="foundry-search" style={{ width: 200 }} placeholder="Search sets…"
          value={search} onChange={e => set("search", e.target.value)} />
        <div className="filter-bar" style={{ border: "none", padding: 0, flex: 1, flexWrap: "wrap" }}>
          <button className={`fchip ${ownership.includes("owned")    ? "fchip-on" : ""}`} onClick={() => set("ownership", toggle(ownership, "owned"))}>Owned</button>
          <button className={`fchip ${ownership.includes("notowned") ? "fchip-on" : ""}`} onClick={() => set("ownership", toggle(ownership, "notowned"))}>Not Owned</button>
          <span className="fbar-sep"/>
          <button className={`fchip ${conditions.includes("dupes")     ? "fchip-on" : ""}`} onClick={() => set("conditions", toggle(conditions, "dupes"))}>Dupes</button>
          <button className={`fchip ${conditions.includes("itemowned") ? "fchip-on" : ""}`} onClick={() => set("conditions", toggle(conditions, "itemowned"))}>Item Owned</button>
          <button className={`fchip ${conditions.includes("fullset")   ? "fchip-on" : ""}`} onClick={() => set("conditions", toggle(conditions, "fullset"))}>Full Set</button>
          <button className={`fchip ${conditions.includes("hasparts")  ? "fchip-on" : ""}`} onClick={() => set("conditions", toggle(conditions, "hasparts"))}>Has Parts</button>
          <span className="fbar-sep"/>
          <button className={`fchip ${vault.includes("vaulted")   ? "fchip-on" : ""}`} onClick={() => set("vault", toggle(vault, "vaulted"))}>Vaulted</button>
          <button className={`fchip ${vault.includes("unvaulted") ? "fchip-on" : ""}`} onClick={() => set("vault", toggle(vault, "unvaulted"))}>Unvaulted</button>
          <span className="fbar-sep"/>
          <span className="fbar-label">Sort:</span>
          <button className={`fchip ${sortMode === "plat"   ? "fchip-on" : ""}`} onClick={() => set("sortMode", "plat")}>Most Plat</button>
          <button className={`fchip ${sortMode === "ducats" ? "fchip-on" : ""}`} onClick={() => set("sortMode", "ducats")}>Most Ducats</button>
          <button className={`fchip ${sortMode === "az"     ? "fchip-on" : ""}`} onClick={() => set("sortMode", "az")}>A–Z</button>
          <button className={`fchip ${sortMode === "za"     ? "fchip-on" : ""}`} onClick={() => set("sortMode", "za")}>Z–A</button>
          <span className="fbar-sep"/>
          <button className="fchip fchip-reset" onClick={() => onFiltersChange(MARKET_FILTERS_DEFAULT)}>Show All</button>
          <span style={{ marginLeft: "auto", fontSize: 11, color: "var(--muted)" }}>{visibleSets.length} sets</span>
          <HelpTip items={[
            { swatch: "rgba(240,192,64,.5)", icon: "✓", label: "Complete set", desc: "Gold border + ✓ — all parts in inventory" },
            { icon: "+",  label: "+ Dupes",    desc: "Extra copies of at least one part" },
            { icon: "⚒",  label: "⚒ Building", desc: "Item is currently crafting in Foundry" },
          ]} />
        </div>
      </div>

      <div className="market-summary">
        <DucatIcon size={13} />
        <span><strong>{fmt(totalDucats)}</strong> total ducats (owned parts)</span>
        <span className="fbar-sep"/>
        <DucatIcon size={13} />
        <span><strong style={{ color: "#f0c040" }}>{fmt(dupeDucats)}</strong> from dupes</span>
        {wfmLoading && <span style={{ color: "var(--muted)", fontSize: 11 }}>· Connecting to warframe.market…</span>}
        {wfmError && <span style={{ color: "var(--red)", fontSize: 11 }}>· warframe.market unavailable</span>}
        {!wfmLoading && !wfmError && wfmItems.length > 0 && <span style={{ color: "var(--green)", fontSize: 11 }}>· {wfmItems.length.toLocaleString()} items from warframe.market</span>}
      </div>

      <div className="market-grid">
        {visibleSets.length === 0 ? (
          <div className="empty-msg" style={{ gridColumn: "1/-1" }}>No sets match. Adjust filters or own some prime parts first.</div>
        ) : visibleSets.map(([setKey, parts]) => {
          const setNormalKey = normalizeForWfm(setKey + " Set");
          const setUrl       = wfmLookup.get(setNormalKey) ?? setNormalKey;
          const parent       = parentItems.get(setKey) ?? parts[0];
          const setPriceData = prices.get(setUrl);
          const setParts: SetPart[] = [...parts]
            .sort((a, b) => {
              const qa = inventory[a.unique_name]?.quantity ?? 0;
              const qb = inventory[b.unique_name]?.quantity ?? 0;
              return qb - qa || a.name.localeCompare(b.name);
            })
            .map(p => {
              const normalKey = normalizeForWfm(p.name);
              const url = wfmLookup.get(normalKey) ?? normalKey;
              const priceData = prices.get(url);
              return { item: p, qty: inventory[p.unique_name]?.quantity ?? 0,
                required_count: recipeCountMap.get(p.unique_name) ?? 1,
                sellMedian: priceData?.sell_median, loading: false, urlName: url };
            });
          return (
            <SetCard key={setKey} setKey={setKey} parts={setParts} parentItem={parent}
              setPrice={setPriceData}
              setPriceLoading={false}
              pricesFetched={prices.size > 0}
              crafting={crafting}
              onCardClick={() => {
                invoke("wfm_queue_price_priority", { urlName: setUrl }).catch(() => {});
                setPopup({ urlName: setUrl, displayName: setKey + " Set", imageName: parent?.image_name ?? undefined });
              }}
              onPartClick={(urlName, displayName, imageName) => {
                invoke("wfm_queue_price_priority", { urlName }).catch(() => {});
                setPopup({ urlName, displayName, imageName });
              }} />
          );
        })}
      </div>
      </>}

      {popup && (
        <ItemMarketPopup
          urlName={popup.urlName}
          displayName={popup.displayName}
          imageName={popup.imageName}
          prefillModRank={popup.prefillModRank}
          onClose={() => setPopup(null)}
          isLoggedIn={!!wfmUsername}
          myUsername={wfmUsername ?? undefined}
        />
      )}
    </div>
  );
}

// ─── Mods & Arcanes tab ───────────────────────────────────────────────────────

const MODS_PAGE_SIZE = 60;

function ModsTab({ allItems, inventory, wfmLookup, prices, modCopiesMap, onOpenPopup }: {
  allItems: CatalogItem[];
  inventory: Record<string, InventoryItem>;
  wfmLookup: Map<string, string>;
  prices: Map<string, WfmPrice>;
  modCopiesMap: Record<string, ModCopy[]>;
  onOpenPopup: (urlName: string, displayName: string, imageName?: string, prefillModRank?: number) => void;
}) {
  const [search, setSearch]       = useState("");
  const [catFilter, setCatFilter] = useState<"all" | "mods" | "arcanes">("all");
  const [ownFilter, setOwnFilter] = useState<"all" | "owned" | "notowned">("all");
  const [sortMode, setSortMode]   = useState<"qty" | "plat" | "az" | "za">("qty");
  const [page, setPage]           = useState(0);

  const catalog = useMemo(() =>
    allItems.filter(i => i.category === "Mods" || i.category === "Arcanes"),
  [allItems]);

  const filtered = useMemo(() => {
    const q = search.toLowerCase();
    return catalog
      .filter(i => {
        if (q && !i.name.toLowerCase().includes(q)) return false;
        const qty = inventory[i.unique_name]?.quantity ?? 0;
        if (ownFilter === "owned"    && qty === 0) return false;
        if (ownFilter === "notowned" && qty  >  0) return false;
        if (catFilter === "mods"    && i.category !== "Mods")    return false;
        if (catFilter === "arcanes" && i.category !== "Arcanes") return false;
        return true;
      })
      .sort((a, b) => {
        if (sortMode === "qty") {
          const qa = inventory[a.unique_name]?.quantity ?? 0;
          const qb = inventory[b.unique_name]?.quantity ?? 0;
          return qb - qa || a.name.localeCompare(b.name);
        }
        if (sortMode === "plat") {
          const au = wfmLookup.get(normalizeForWfm(a.name));
          const bu = wfmLookup.get(normalizeForWfm(b.name));
          const ap = au ? (prices.get(au)?.sell_median ?? 0) : 0;
          const bp = bu ? (prices.get(bu)?.sell_median ?? 0) : 0;
          return bp - ap || a.name.localeCompare(b.name);
        }
        if (sortMode === "za") return b.name.localeCompare(a.name);
        return a.name.localeCompare(b.name);
      });
  }, [catalog, inventory, search, ownFilter, catFilter, sortMode, prices, wfmLookup]);

  useEffect(() => { setPage(0); }, [search, catFilter, ownFilter, sortMode]);

  const totalPages = Math.ceil(filtered.length / MODS_PAGE_SIZE);
  const pageItems  = useMemo(
    () => filtered.slice(page * MODS_PAGE_SIZE, (page + 1) * MODS_PAGE_SIZE),
    [filtered, page]
  );

  useEffect(() => {
    const urls = pageItems
      .map(i => wfmLookup.get(normalizeForWfm(i.name)))
      .filter((u): u is string => !!u);
    if (urls.length) invoke("wfm_queue_prices", { urlNames: urls }).catch(() => {});
  }, [pageItems, wfmLookup]);

  const ownedCount = useMemo(
    () => catalog.filter(i => (inventory[i.unique_name]?.quantity ?? 0) > 0).length,
    [catalog, inventory]
  );

  return (
    <div style={{ display: "flex", flexDirection: "column", flex: 1, minHeight: 0 }}>
      <div className="market-header">
        <input className="foundry-search" style={{ width: 200 }} placeholder="Search mods &amp; arcanes…"
          value={search} onChange={e => setSearch(e.target.value)} />
        <div className="filter-bar" style={{ border: "none", padding: 0, flex: 1, flexWrap: "wrap" }}>
          <button className={`fchip ${catFilter === "all"     ? "fchip-on" : ""}`} onClick={() => setCatFilter("all")}>All</button>
          <button className={`fchip ${catFilter === "mods"    ? "fchip-on" : ""}`} onClick={() => setCatFilter("mods")}>Mods</button>
          <button className={`fchip ${catFilter === "arcanes" ? "fchip-on" : ""}`} onClick={() => setCatFilter("arcanes")}>Arcanes</button>
          <span className="fbar-sep"/>
          <button className={`fchip ${ownFilter === "all"      ? "fchip-on" : ""}`} onClick={() => setOwnFilter("all")}>All</button>
          <button className={`fchip ${ownFilter === "owned"    ? "fchip-on" : ""}`} onClick={() => setOwnFilter("owned")}>Owned</button>
          <button className={`fchip ${ownFilter === "notowned" ? "fchip-on" : ""}`} onClick={() => setOwnFilter("notowned")}>Not Owned</button>
          <span className="fbar-sep"/>
          <span className="fbar-label">Sort:</span>
          <button className={`fchip ${sortMode === "qty"  ? "fchip-on" : ""}`} onClick={() => setSortMode("qty")}>Most Owned</button>
          <button className={`fchip ${sortMode === "plat" ? "fchip-on" : ""}`} onClick={() => setSortMode("plat")}>Most Plat</button>
          <button className={`fchip ${sortMode === "az"   ? "fchip-on" : ""}`} onClick={() => setSortMode("az")}>A–Z</button>
          <button className={`fchip ${sortMode === "za"   ? "fchip-on" : ""}`} onClick={() => setSortMode("za")}>Z–A</button>
        </div>
      </div>

      <div className="market-summary">
        <span><strong>{ownedCount.toLocaleString()}</strong> owned · <strong>{catalog.length.toLocaleString()}</strong> total</span>
        <span className="fbar-sep"/>
        <span style={{ color: "var(--muted)" }}>{filtered.length.toLocaleString()} shown</span>
      </div>

      <div className="mods-grid">
        {pageItems.map(item => {
          const urlName  = wfmLookup.get(normalizeForWfm(item.name)) ?? normalizeForWfm(item.name);
          const price    = prices.get(urlName)?.sell_median;
          const qty      = inventory[item.unique_name]?.quantity ?? 0;
          const isArcane = item.category === "Arcanes";
          const copies   = modCopiesMap[item.unique_name];
          // Ranked copies sorted highest-first (null rank = unknown, shown last)
          const rankedCopies = copies
            ? [...copies].filter(c => c.count > 0).sort((a, b) => (b.rank ?? -1) - (a.rank ?? -1))
            : [];
          return (
            <div
              key={item.unique_name}
              className={`mod-card ${qty === 0 ? "mod-card-unowned" : ""}`}
              onClick={() => onOpenPopup(urlName, item.name, item.image_name ?? undefined)}
            >
              <span className={`mod-cat-badge ${isArcane ? "mod-cat-arcane" : "mod-cat-mod"}`}>
                {isArcane ? "Arcane" : "Mod"}
              </span>
              <div className="mod-card-img">
                <ItemImg imageName={item.image_name ?? undefined} size={56} />
              </div>
              <div className="mod-card-name">{item.name}</div>
              <div className="mod-card-footer">
                {rankedCopies.length > 0 ? (
                  <div className="mod-rank-chips">
                    {rankedCopies.map(c => (
                      <span
                        key={c.rank ?? "null"}
                        className="mod-rank-chip"
                        title={`Open market for rank ${c.rank ?? "?"}`}
                        onClick={e => {
                          e.stopPropagation();
                          onOpenPopup(urlName, item.name, item.image_name ?? undefined, c.rank ?? undefined);
                        }}
                      >
                        {c.rank !== null ? `R${c.rank}` : "?"} ×{c.count}
                      </span>
                    ))}
                  </div>
                ) : (
                  <span className="mod-rank-badge mod-rank-none">—</span>
                )}
                {price != null
                  ? <span className="mod-plat-cell"><PlatIcon size={10} />{fmtPt(price)}</span>
                  : <span className="mod-plat-cell market-price-na">—</span>}
              </div>
            </div>
          );
        })}
        {pageItems.length === 0 && (
          <div className="empty-msg" style={{ padding: 24, gridColumn: "1/-1" }}>No items match the current filters.</div>
        )}
      </div>

      {totalPages > 1 && (
        <div className="mods-pagination">
          <button className="fchip" disabled={page === 0} onClick={() => setPage(p => p - 1)}>‹ Prev</button>
          <span className="mods-page-info">Page {page + 1} of {totalPages} · {filtered.length} items</span>
          <button className="fchip" disabled={page >= totalPages - 1} onClick={() => setPage(p => p + 1)}>Next ›</button>
        </div>
      )}
    </div>
  );
}

// ─── Rivens tab ───────────────────────────────────────────────────────────────

function rivenCategory(itemType: string): string {
  if (itemType.includes("Melee"))   return "Melee";
  if (itemType.includes("Rifle"))   return "Rifle";
  if (itemType.includes("Pistol") || itemType.includes("Kitgun")) return "Pistol";
  if (itemType.includes("Shotgun")) return "Shotgun";
  if (itemType.includes("Archgun") || itemType.includes("Archwing")) return "Arch-gun";
  if (itemType.includes("Zaw"))     return "Zaw";
  return "Riven";
}

// Riven stat formula — exact port of calamity-inc/warframe-riven-info RivenParser.js
//
// Buff:  baseCap × (1.5 × omega × 10) × 1.25^numCurses × lerp(0.9,1.1,raw/0x3FFFFFFF) × buffAtten[numBuffs] × (rank+1)
// Curse: −baseCap × (1.5 × omega × 10) × lerp × curseAtten[numBuffs] × buffAtten[numCurses] × (rank+1)
//
// baseCap values are per-stat per riven type, sourced from riven_tags.json in that repo.
// Kitgun caps == Pistol caps; Zaw caps == Melee caps — merged into PL and ME respectively.
// Units: '%' → +X.X%; 'x' → faction damage shown as ×1.07; 'm' → meters; 's' → seconds; 'n' → flat.

const BUFF_ATTEN  = [0, 1, 0.66000003, 0.5, 0.40000001, 0.34999999] as const;
const CURSE_ATTEN = [0, 1, 0.33000001, 0.5, 1.25,       1.5       ] as const;

// AG=Archgun  ME=Melee/Zaw  PL=Pistol/Kitgun  RI=Rifle  SG=Shotgun
type RT = 'AG' | 'ME' | 'PL' | 'RI' | 'SG';
interface RS { label: string; unit: '%' | 'x' | 'm' | 's' | 'n'; caps: Partial<Record<RT, number>> }
const RIVEN_STAT: Record<string, RS> = {
  // ── Melee / Zaw only ─────────────────────────────────────────────────────────
  WeaponMeleeDamageMod:             { label: "Damage",               unit: '%', caps: { ME: 0.0183   } },
  WeaponMeleeRangeIncMod:           { label: "Range",                unit: 'm', caps: { ME: 0.02158  } },
  WeaponMeleeComboEfficiencyMod:    { label: "Heavy Atk Efficiency", unit: '%', caps: { ME: 0.00816  } },
  WeaponMeleeFinisherDamageMod:     { label: "Finisher Damage",      unit: '%', caps: { ME: 0.0133   } },
  WeaponMeleeComboInitialBonusMod:  { label: "Initial Combo",        unit: 'n', caps: { ME: 0.27224  } },
  WeaponMeleeComboBonusOnHitMod:    { label: "Combo Count Chance",   unit: '%', caps: { ME: 0.00653  } },
  WeaponMeleeComboPointsOnHitMod:   { label: "Combo Count Chance",   unit: '%', caps: { ME: -0.01165 } },
  SlideAttackCritChanceMod:         { label: "Slide Crit Chance",    unit: '%', caps: { ME: 0.013334 } },
  ComboDurationMod:                 { label: "Combo Duration",       unit: 's', caps: { ME: 0.09     } },
  WeaponMeleeFactionDamageCorpus:   { label: "Damage to Corpus",     unit: 'x', caps: { ME: 0.005   } },
  WeaponMeleeFactionDamageGrineer:  { label: "Damage to Grineer",    unit: 'x', caps: { ME: 0.005   } },
  WeaponMeleeFactionDamageInfested: { label: "Damage to Infested",   unit: 'x', caps: { ME: 0.005   } },
  // ── Ranged only ──────────────────────────────────────────────────────────────
  WeaponDamageAmountMod:    { label: "Damage",            unit: '%', caps: { AG: 0.0111,   PL: 0.0244,   RI: 0.018333, SG: 0.0183   } },
  WeaponFireIterationsMod:  { label: "Multishot",         unit: '%', caps: { AG: 0.0067,   PL: 0.0133,   RI: 0.01,     SG: 0.0133   } },
  WeaponAmmoMaxMod:         { label: "Ammo Maximum",      unit: '%', caps: { AG: 0.0111,   PL: 0.01,     RI: 0.00555,  SG: 0.01     } },
  WeaponClipMaxMod:         { label: "Magazine Capacity", unit: '%', caps: { AG: 0.0067,   PL: 0.005555, RI: 0.005555, SG: 0.005555 } },
  WeaponReloadSpeedMod:     { label: "Reload Speed",      unit: '%', caps: { AG: 0.0111,   PL: 0.005555, RI: 0.005555, SG: 0.005555 } },
  WeaponProjectileSpeedMod: { label: "Projectile Speed",  unit: '%', caps: {               PL: 0.01,     RI: 0.01,     SG: 0.01     } },
  WeaponZoomFovMod:         { label: "Zoom",              unit: '%', caps: { AG: 0.006666, PL: 0.0089,   RI: 0.006666              } },
  WeaponPunctureDepthMod:   { label: "Punch Through",     unit: 'm', caps: { AG: 0.03,     PL: 0.03,     RI: 0.03,     SG: 0.03     } },
  WeaponRecoilReductionMod: { label: "Recoil",            unit: '%', caps: { AG: -0.01,    PL: -0.01,    RI: -0.01,    SG: -0.01    } },
  WeaponFactionDamageCorpus:   { label: "Damage to Corpus",   unit: 'x', caps: { AG: 0.005, PL: 0.005, RI: 0.005, SG: 0.005 } },
  WeaponFactionDamageGrineer:  { label: "Damage to Grineer",  unit: 'x', caps: { AG: 0.005, PL: 0.005, RI: 0.005, SG: 0.005 } },
  WeaponFactionDamageInfested: { label: "Damage to Infested", unit: 'x', caps: {             PL: 0.005, RI: 0.005, SG: 0.005 } },
  // ── All weapons ──────────────────────────────────────────────────────────────
  WeaponCritChanceMod:          { label: "Critical Chance",     unit: '%', caps: { AG: 0.0111,  ME: 0.02,     PL: 0.016666, RI: 0.016666, SG: 0.01     } },
  WeaponCritDamageMod:          { label: "Critical Damage",     unit: '%', caps: { AG: 0.0089,  ME: 0.01,     PL: 0.01,     RI: 0.013333, SG: 0.01     } },
  WeaponArmorPiercingDamageMod: { label: "Puncture",            unit: '%', caps: { AG: 0.01,    ME: 0.0133,   PL: 0.01333,  RI: 0.01333,  SG: 0.01333  } },
  WeaponImpactDamageMod:        { label: "Impact",              unit: '%', caps: { AG: 0.01,    ME: 0.0133,   PL: 0.013333, RI: 0.013333, SG: 0.013333 } },
  WeaponSlashDamageMod:         { label: "Slash",               unit: '%', caps: { AG: 0.01,    ME: 0.0133,   PL: 0.013333, RI: 0.013333, SG: 0.013333 } },
  WeaponElectricityDamageMod:   { label: "Electricity",         unit: '%', caps: { AG: 0.0133,  ME: 0.01,     PL: 0.01,     RI: 0.01,     SG: 0.01     } },
  WeaponFireDamageMod:          { label: "Heat",                unit: '%', caps: { AG: 0.0133,  ME: 0.01,     PL: 0.01,     RI: 0.01,     SG: 0.01     } },
  WeaponFreezeDamageMod:        { label: "Cold",                unit: '%', caps: { AG: 0.0133,  ME: 0.01,     PL: 0.01,     RI: 0.01,     SG: 0.01     } },
  WeaponToxinDamageMod:         { label: "Toxin",               unit: '%', caps: { AG: 0.0133,  ME: 0.01,     PL: 0.01,     RI: 0.01,     SG: 0.01     } },
  WeaponFireRateMod:            { label: "Fire Rate / Atk Spd", unit: '%', caps: { AG: 0.00667, ME: 0.0061,   PL: 0.0083,   RI: 0.00667,  SG: 0.01     } },
  WeaponProcTimeMod:            { label: "Status Duration",     unit: '%', caps: { AG: 0.01111, ME: 0.01111,  PL: 0.01111,  RI: 0.01111,  SG: 0.01111  } },
  WeaponStunChanceMod:          { label: "Status Chance",       unit: '%', caps: { AG: 0.0067,  ME: 0.01,     PL: 0.01,     RI: 0.01,     SG: 0.01     } },
};

function rivenTypeKey(category: string): RT {
  if (category === "Arch-gun")                  return "AG";
  if (category === "Melee" || category === "Zaw") return "ME";
  if (category === "Shotgun")                   return "SG";
  if (category === "Rifle")                     return "RI";
  return "PL"; // Pistol, Kitgun, unknown
}

const RIVEN_CHALLENGE: Record<string, string> = {
  "/Lotus/Types/Challenges/RandomizedFinisherKill":         "Kill X enemies with Finishers",
  "/Lotus/Types/Challenges/RandomizedStyleKill":            "Kill X enemies while Sliding",
  "/Lotus/Types/Challenges/RandomizedHeadshot":             "Kill X enemies with Headshots",
  "/Lotus/Types/Challenges/RandomizedHeadshotUnawareBallistas": "Kill X unalerted Tusk Ballistas with a Headshot",
  "/Lotus/Types/Challenges/RandomizedLongRangeSniper":      "Kill X enemies with Headshots from 75m+",
  "/Lotus/Types/Challenges/RandomizedKillPassengers":       "Kill X enemies on a Dropship",
  "/Lotus/Types/Challenges/RandomizedFisherman":            "Catch X fish without missing a throw",
  "/Lotus/Types/Challenges/PlainsTimedVariety":             "Catch 1 fish, mine 1 gem, kill 1 enemy in 30s",
  "/Lotus/Types/Challenges/HighPerfectDefense":             "Defense (lvl 30+) without objective taking damage",
  "/Lotus/Types/Challenges/HighExterminationUndetected":    "Extermination (lvl 30+) undetected",
  "/Lotus/Types/Challenges/HighSoloInterceptionHobbled":    "Solo Interception (lvl 30+) with Hobbled Key",
  "/Lotus/Types/Challenges/HighSurvivalPacifist":           "Survival (lvl 30+) without killing anyone",
  "/Lotus/Types/Challenges/RandomizedSkiffArcher":          "Destroy X Dargyns with a bow",
  "/Lotus/Types/Challenges/RandomizedAntiAntiAir":          "Destroy X Vruush Turrets in Archwing",
  "/Lotus/Types/Challenges/RandomizedFindCaches":           "Find X caches",
  "/Lotus/Types/Challenges/RandomizedFindRareMedallions":   "Find X Syndicate Medallions",
  "/Lotus/Types/Challenges/RandomizedHeadshotGlide":        "3 headshot kills in a single Aim Glide",
  "/Lotus/Types/Challenges/RandomizedWallClingKillstreak":  "Kill X enemies while wall dashing/clinging",
  "/Lotus/Types/Challenges/RandomizedKillSentients":        "Kill X Sentients",
  "/Lotus/Types/Challenges/RandomizedKillFallingPilots":    "Kill X Dargyn Pilots before they hit the ground",
  "/Lotus/Types/Challenges/RandomizedKill":                 "Kill X enemies",
  "/Lotus/Types/Challenges/RandomizedFlyingHeadshotSeries": "X consecutive headshots in Archwing (Plains)",
  "/Lotus/Types/Challenges/SustainMeleeComboThree":         "Sustain a 6x melee combo for 30 seconds",
  "/Lotus/Types/Challenges/LimitedSynthesis":               "Synthesize a Simaris target (no traps/abilities, Hobbled Key)",
};

const RIVEN_COMPLICATION: Record<string, string> = {
  "/Lotus/Types/Challenges/Complications/Undetected":               "while undetected",
  "/Lotus/Types/Challenges/Complications/Sliding":                  "while sliding",
  "/Lotus/Types/Challenges/Complications/AimGliding":               "while Aim Gliding",
  "/Lotus/Types/Challenges/Complications/ResetOnDamageTaken":       "without taking damage",
  "/Lotus/Types/Challenges/Complications/ResetOnDowned":            "without dying or becoming downed",
  "/Lotus/Types/Challenges/Complications/PetPresent":               "with an active pet present",
  "/Lotus/Types/Challenges/Complications/SentinelPresent":          "with an active sentinel present",
  "/Lotus/Types/Challenges/Complications/SoloPlayer":               "while alone or in Solo Mode",
  "/Lotus/Types/Challenges/Complications/ResetOnMissionFailure":    "without failing a mission",
  "/Lotus/Types/Challenges/Complications/ResetOnAlarmRaised":       "without raising any alarms",
  "/Lotus/Types/Challenges/Complications/ResetOnDisrupt":           "without being disrupted by Magnetic Damage",
  "/Lotus/Types/Challenges/Complications/ResetOnProc":              "without getting afflicted by a Status Effect",
  "/Lotus/Types/Challenges/Complications/ResetOnAllyDowned":        "without an ally becoming downed",
  "/Lotus/Types/Challenges/Complications/Invisible":                "while invisible",
  "/Lotus/Types/Challenges/Complications/EquippedDamageDebuffKey":  "with an Extinguished Dragon Key",
  "/Lotus/Types/Challenges/Complications/EquippedHealthDebuffKey":  "with a Bleeding Dragon Key",
  "/Lotus/Types/Challenges/Complications/EquippedShieldDebuffKey":  "with a Decaying Dragon Key",
  "/Lotus/Types/Challenges/Complications/EquippedSpeedDebuffKey":   "with a Hobbled Dragon Key",
  "/Lotus/Types/Challenges/Complications/ResetOnGearCipher":        "without using ciphers",
  "/Lotus/Types/Challenges/Complications/ResetOnGearAirSupport":    "without using air support",
  "/Lotus/Types/Challenges/Complications/ResetOnGearHealthRestores":"without using health consumables",
  "/Lotus/Types/Challenges/Complications/ResetOnGearShieldRestores":"without using shield-restoring consumables",
  "/Lotus/Types/Challenges/Complications/ResetOnGearAmmoRestores":  "without using ammo consumables",
  "/Lotus/Types/Challenges/Complications/ResetOnGearEnergyRestores":"without using energy consumables",
  "/Lotus/Types/Challenges/Complications/ResetOnNewDay":            "in one day",
};

function formatChallengeName(type: string | null, complication: string | null): string {
  const challenge = type
    ? (RIVEN_CHALLENGE[type] ?? (type.split("/").pop()?.replace(/([A-Z])/g, " $1").trim() ?? "Unknown challenge"))
    : "Unknown challenge";
  if (!complication) return challenge;
  const comp = RIVEN_COMPLICATION[complication] ?? (complication.split("/").pop()?.replace(/([A-Z])/g, " $1").trim() ?? "");
  return comp ? `${challenge}, ${comp}` : challenge;
}

// Riven name prefix/suffix words per stat tag (from Warframe wiki naming table).
const RIVEN_NAME_PARTS: Record<string, { p: string; s: string }> = {
  WeaponMeleeComboBonusOnHitMod:    { p: "Laci",  s: "Nus"  },
  WeaponMeleeComboPointsOnHitMod:   { p: "Laci",  s: "Nus"  },
  WeaponAmmoMaxMod:                 { p: "Ampi",  s: "Bin"  },
  WeaponMeleeFactionDamageCorpus:   { p: "Manti", s: "Tron" },
  WeaponFactionDamageCorpus:        { p: "Manti", s: "Tron" },
  WeaponMeleeFactionDamageGrineer:  { p: "Argi",  s: "Con"  },
  WeaponFactionDamageGrineer:       { p: "Argi",  s: "Con"  },
  WeaponMeleeFactionDamageInfested: { p: "Pura",  s: "Ada"  },
  WeaponFactionDamageInfested:      { p: "Pura",  s: "Ada"  },
  WeaponFreezeDamageMod:            { p: "Geli",  s: "Do"   },
  ComboDurationMod:                 { p: "Tempi", s: "Nem"  },
  WeaponCritChanceMod:              { p: "Crita", s: "Cron" },
  SlideAttackCritChanceMod:         { p: "Pleci", s: "Nent" },
  WeaponCritDamageMod:              { p: "Acri",  s: "Tis"  },
  WeaponDamageAmountMod:            { p: "Visi",  s: "Ata"  },
  WeaponMeleeDamageMod:             { p: "Visi",  s: "Ata"  },
  WeaponElectricityDamageMod:       { p: "Vexi",  s: "Tio"  },
  WeaponFireDamageMod:              { p: "Igni",  s: "Pha"  },
  WeaponMeleeFinisherDamageMod:     { p: "Exi",   s: "Cta"  },
  WeaponFireRateMod:                { p: "Croni", s: "Dra"  },
  WeaponProjectileSpeedMod:         { p: "Conci", s: "Nak"  },
  WeaponMeleeComboInitialBonusMod:  { p: "Para",  s: "Um"   },
  WeaponImpactDamageMod:            { p: "Magna", s: "Ton"  },
  WeaponClipMaxMod:                 { p: "Arma",  s: "Tin"  },
  WeaponMeleeComboEfficiencyMod:    { p: "Forti", s: "Us"   },
  WeaponFireIterationsMod:          { p: "Sati",  s: "Can"  },
  WeaponToxinDamageMod:             { p: "Toxi",  s: "Tox"  },
  WeaponPunctureDepthMod:           { p: "Lexi",  s: "Nok"  },
  WeaponArmorPiercingDamageMod:     { p: "Insi",  s: "Cak"  },
  WeaponReloadSpeedMod:             { p: "Feva",  s: "Tak"  },
  WeaponMeleeRangeIncMod:           { p: "Locti", s: "Tor"  },
  WeaponSlashDamageMod:             { p: "Sci",   s: "Sus"  },
  WeaponStunChanceMod:              { p: "Hexa",  s: "Dex"  },
  WeaponProcTimeMod:                { p: "Deci",  s: "Des"  },
  WeaponRecoilReductionMod:         { p: "Zeti",  s: "Mag"  },
  WeaponZoomFovMod:                 { p: "Hera",  s: "Lis"  },
};

// Compute the deterministic riven mod name from its stats.
// Only buffs (positive stats) determine the name — curses are ignored.
// 1 buff  → CoreSuffix         (buff's prefix word + buff's suffix word)
// 2 buffs → CoreSuffix         (higher's prefix + lower's suffix, no dash)
// 3 buffs → Prefix-CoreSuffix  (highest's prefix - second's prefix + lowest's suffix)
// Returns lowercase (WFM format); capitalize first letter for display.
function rivenModName(riven: BlobRivenEntry): string {
  if (riven.buffs.length === 0) return "";
  const sorted = [...riven.buffs].sort((a, b) => b.value - a.value);
  const hi  = RIVEN_NAME_PARTS[sorted[0].tag];
  const lo  = RIVEN_NAME_PARTS[sorted[sorted.length - 1].tag];
  if (!hi || !lo) return "";
  const suffix = lo.s.toLowerCase();
  if (sorted.length >= 3) {
    const mid = RIVEN_NAME_PARTS[sorted[1].tag];
    if (mid) return `${hi.p.toLowerCase()}-${mid.p.toLowerCase()}${suffix}`;
  }
  // 1-2 buffs: core word from highest buff, suffix word from lowest buff — no dash.
  return `${hi.p.toLowerCase()}${suffix}`;
}

function rivenStatLabel(stat: BlobRivenStat, positive: boolean, disposition: number, category: string, numBuffs: number, numCurses: number, rank: number): string {
  const frac  = stat.value / 0x3FFFFFFF; // 1073741823 — matches RivenParser.js rivenIntToFloat
  const roll  = 0.9 + frac * 0.2;
  const entry = RIVEN_STAT[stat.tag];
  const label = entry?.label ?? stat.tag;
  const rt    = rivenTypeKey(category);

  const baseCap = entry?.caps[rt] ?? null;
  if (baseCap === null) {
    const sign = positive ? "+" : "-";
    return `${sign}${(frac * 100).toFixed(1)}% ${label}`;
  }

  const nb    = Math.min(numBuffs, BUFF_ATTEN.length - 1);
  const nc    = Math.min(numCurses, BUFF_ATTEN.length - 1);
  const atten = 1.5 * disposition * 10; // SPECIFIC_FIT_ATTENUATION × omega × base_drain

  const v = positive
    ? baseCap * atten * Math.pow(1.25, numCurses) * roll * BUFF_ATTEN[nb] * (rank + 1)
    : -(baseCap * atten * roll * CURSE_ATTEN[nb] * BUFF_ATTEN[nc] * (rank + 1));

  const s = v >= 0 ? "+" : ""; // negative values carry their own sign
  switch (entry!.unit) {
    case '%': return `${s}${(v * 100).toFixed(1)}% ${label}`;
    case 'x': return `x${(1 + v).toFixed(2)} ${label}`;
    case 'm': return `${s}${v.toFixed(1)}m ${label}`;
    case 's': return `${s}${v.toFixed(1)}s ${label}`;
    case 'n': return `${s}${Math.round(v)} ${label}`;
  }
}

// ── WFM riven auction helpers ─────────────────────────────────────────────────

// Internal stat tag → warframe.market v1 attribute url_name.
// Names from real auction data via /v1/auctions/search — WFM no longer uses
// trailing underscores; combined stats use the "_/_" separator.
const RIVEN_WFM_ATTR: Record<string, string> = {
  // ── Melee / Zaw ──────────────────────────────────────────────────────────────
  WeaponMeleeDamageMod:             "base_damage_/_melee_damage",
  WeaponMeleeRangeIncMod:           "range",
  WeaponMeleeComboEfficiencyMod:    "channeling_efficiency",      // WFM still uses old "channeling" name
  WeaponMeleeFinisherDamageMod:     "finisher_damage",
  WeaponMeleeComboInitialBonusMod:  "initial_combo",
  WeaponMeleeComboBonusOnHitMod:    "chance_to_gain_extra_combo_count", // "Additional Combo Count Chance" on WFM
  WeaponMeleeComboPointsOnHitMod:   "chance_to_gain_combo_count",
  SlideAttackCritChanceMod:         "critical_chance_on_slide_attack",
  ComboDurationMod:                 "combo_duration",
  WeaponMeleeFactionDamageCorpus:   "damage_vs_corpus",
  WeaponMeleeFactionDamageGrineer:  "damage_vs_grineer",
  WeaponMeleeFactionDamageInfested: "damage_vs_infested",
  // ── Ranged ───────────────────────────────────────────────────────────────────
  WeaponDamageAmountMod:            "base_damage_/_melee_damage",
  WeaponFireIterationsMod:          "multishot",
  WeaponAmmoMaxMod:                 "ammo_maximum",
  WeaponClipMaxMod:                 "magazine_capacity",
  WeaponReloadSpeedMod:             "reload_speed",
  WeaponProjectileSpeedMod:         "projectile_speed",
  WeaponZoomFovMod:                 "zoom",
  WeaponPunctureDepthMod:           "punch_through",
  WeaponRecoilReductionMod:         "recoil",
  WeaponFactionDamageCorpus:        "damage_vs_corpus",
  WeaponFactionDamageGrineer:       "damage_vs_grineer",
  WeaponFactionDamageInfested:      "damage_vs_infested",
  // ── All weapons ──────────────────────────────────────────────────────────────
  WeaponCritChanceMod:              "critical_chance",
  WeaponCritDamageMod:              "critical_damage",
  WeaponArmorPiercingDamageMod:     "puncture_damage",
  WeaponImpactDamageMod:            "impact_damage",
  WeaponSlashDamageMod:             "slash_damage",
  WeaponElectricityDamageMod:       "electricity_damage",
  WeaponFireDamageMod:              "heat_damage",
  WeaponFreezeDamageMod:            "cold_damage",
  WeaponToxinDamageMod:             "toxin_damage",
  WeaponFireRateMod:                "fire_rate_/_attack_speed",
  WeaponProcTimeMod:                "status_duration",
  WeaponStunChanceMod:              "status_chance",
};

// WFM's combined attributes cover both melee and ranged with a single url_name,
// so no category-specific overrides are needed.
function rivenWfmAttr(tag: string, _category: string): string | undefined {
  return RIVEN_WFM_ATTR[tag];
}

// Stats WFM classifies as negative-only regardless of how the game labels them.
// These must always be sent as positive:false with a negative value.
const WFM_NEGATIVE_ONLY = new Set<string>([]);

// Warframe inventory polarity IDs → WFM polarity names
function wfmPolarity(pol: string | null): string {
  const MAP: Record<string, string> = {
    AP_ATTACK:  "madurai",
    AP_DEFENSE: "vazarin",
    AP_TACTIC:  "naramon",
    AP_WARD:    "unairu",
    AP_POWER:   "zenurik",
    AP_UMBRA:   "umbra",
    AP_PRECEPT: "penjaga",
  };
  return MAP[pol ?? ""] ?? "madurai";
}

// Polarity display: icon + human name
const POLARITY_DISPLAY: Record<string, { icon: string; name: string }> = {
  AP_ATTACK:  { icon: polMadurai,  name: "Madurai"  },
  AP_DEFENSE: { icon: polVazarin,  name: "Vazarin"  },
  AP_TACTIC:  { icon: polNaramon,  name: "Naramon"  },
  AP_WARD:    { icon: polUnairu,   name: "Unairu"   },
  AP_POWER:   { icon: polZenurik,  name: "Zenurik"  },
  AP_UMBRA:   { icon: polUmbra,    name: "Umbra"    },
  AP_PRECEPT: { icon: polPenjaga,  name: "Penjaga"  },
};

// Display name → WFM URL slug (mirrors to_wfm_slug in Rust)
function toWfmSlug(name: string): string {
  return name.toLowerCase().replace(/['']/g, "").replace(/&/g, "and").replace(/[^a-z0-9]+/g, "_").replace(/^_+|_+$/g, "");
}

// Compute the signed numeric value used for WFM auction attributes.
// WFM format: % stats → signed percentage (9.5 buff, -6.0 curse); x stats → full multiplier (1.07 buff).
function rivenStatWfmValue(
  stat: BlobRivenStat, positive: boolean,
  disposition: number, category: string,
  numBuffs: number, numCurses: number, rank: number
): number {
  const frac  = stat.value / 0x3FFFFFFF;
  const roll  = 0.9 + frac * 0.2;
  const entry = RIVEN_STAT[stat.tag];
  const rt    = rivenTypeKey(category);
  const baseCap = entry?.caps[rt];

  if (!baseCap || !entry) return parseFloat((frac * 100 * (positive ? 1 : -1)).toFixed(1));

  const nb    = Math.min(numBuffs, BUFF_ATTEN.length - 1);
  const nc    = Math.min(numCurses, BUFF_ATTEN.length - 1);
  const atten = 1.5 * disposition * 10;

  const v = positive
    ? baseCap * atten * Math.pow(1.25, numCurses) * roll * BUFF_ATTEN[nb] * (rank + 1)
    : -(baseCap * atten * roll * CURSE_ATTEN[nb] * BUFF_ATTEN[nc] * (rank + 1));

  // v is positive for buffs, negative for curses
  switch (entry.unit) {
    case '%': return parseFloat((v * 100).toFixed(1));       // e.g. 9.5 or -6.0
    case 'x': return parseFloat((1 + Math.abs(v)).toFixed(2)); // WFM: full multiplier 1.07 (faction dmg only ever a buff)
    case 'm': return parseFloat(v.toFixed(2));
    case 's': return parseFloat(v.toFixed(1));
    case 'n': return Math.round(v);
  }
}

// ── Veiled riven WFM URL names (regular sell orders, not auctions) ────────────
const VEILED_WFM_SLUG: Record<string, string> = {
  "Rifle":   "rifle_riven_mod",
  "Pistol":  "pistol_riven_mod",
  "Melee":   "melee_riven_mod",
  "Shotgun": "shotgun_riven_mod",
  "Arch-gun":"archgun_riven_mod",
  "Zaw":     "zaw_riven_mod",
};

// ── Sell modal for UNVEILED rivens (WFM auction) ──────────────────────────────

interface SellModalProps {
  riven:      BlobRivenEntry;
  weaponName: string;
  disposition: number;
  category:   string;
  onClose:    () => void;
  onSuccess:  () => void;
}

function RivenSellModal({ riven, weaponName, disposition, category, onClose, onSuccess }: SellModalProps) {
  const [saleType,    setSaleType]    = useState<"auction" | "direct">("auction");
  const [startPrice,  setStartPrice]  = useState("100");
  const [buyoutPrice, setBuyoutPrice] = useState("");
  const [directPrice, setDirectPrice] = useState("100");
  const [minRep,      setMinRep]      = useState("0");
  const [note,        setNote]        = useState("");
  const [visible,     setVisible]     = useState(true);
  const [busy,        setBusy]        = useState(false);
  const [error,       setError]       = useState<string | null>(null);

  const attrs = [
    ...riven.buffs.map(b => {
      const url_name = rivenWfmAttr(b.tag, category);
      const isNegOnly = !!(url_name && WFM_NEGATIVE_ONLY.has(url_name));
      // Always compute via buff formula; negate result if WFM requires negative-only
      const rawValue = rivenStatWfmValue(b, true, disposition, category, riven.buffs.length, riven.curses.length, riven.mod_rank);
      return { url_name, positive: !isNegOnly, value: isNegOnly ? -Math.abs(rawValue) : rawValue };
    }),
    ...riven.curses.map(c => ({
      url_name: rivenWfmAttr(c.tag, category),
      positive: false,
      value: rivenStatWfmValue(c, false, disposition, category, riven.buffs.length, riven.curses.length, riven.mod_rank),
    })),
  ];
  const unmapped = attrs.filter(a => !a.url_name);
  const validAttrs = attrs.filter(a => !!a.url_name);

  async function handleSubmit() {
    let sp: number, bp: number | null;
    if (saleType === "direct") {
      const p = parseInt(directPrice, 10);
      if (!p || p < 1) { setError("Selling price must be at least 1 platinum."); return; }
      sp = p; bp = p;
    } else {
      sp = parseInt(startPrice, 10);
      if (!sp || sp < 1) { setError("Starting price must be at least 1 platinum."); return; }
      const bpRaw = buyoutPrice.trim() ? parseInt(buyoutPrice, 10) : undefined;
      if (bpRaw !== undefined && bpRaw < sp) { setError("Buyout price must be ≥ starting price."); return; }
      bp = bpRaw ?? null;
    }
    setBusy(true);
    setError(null);
    const computedName = riven.mod_name || rivenModName(riven);
    try {
      await invoke("wfm_create_riven_auction", {
        weaponUrlName:       toWfmSlug(weaponName),
        rivenName:           computedName,
        masteryLevel:        riven.lvl_req ?? 0,
        modRank:             riven.mod_rank,
        reRolls:             riven.rerolls,
        polarity:            wfmPolarity(riven.polarity),
        attributes:          validAttrs,
        startingPrice:       sp,
        buyoutPrice:         bp,
        minimalReputation:   saleType === "direct" ? 0 : (parseInt(minRep, 10) || 0),
        note,
        visible,
      });
      onSuccess();
      onClose();
    } catch (e: unknown) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="riven-modal-overlay" onClick={e => { if (e.target === e.currentTarget) onClose(); }}>
      <div className="riven-modal">
        <div className="riven-modal-title">
          Post Riven Auction
          <button className="riven-modal-close" onClick={onClose}>×</button>
        </div>

        <div>
          <div className="riven-modal-weapon">{weaponName}{(() => { const mn = (riven.mod_name || rivenModName(riven)); return mn ? <> <span className="riven-mod-name">{mn.replace(/^./, c => c.toUpperCase())}</span></> : null; })()}</div>
          <div className="riven-modal-meta">{category} · MR {riven.lvl_req ?? "?"} · Rank {riven.mod_rank} · {disposition.toFixed(2)}x · {riven.rerolls} roll{riven.rerolls !== 1 ? "s" : ""}{riven.polarity && POLARITY_DISPLAY[riven.polarity] ? <> · <img src={POLARITY_DISPLAY[riven.polarity].icon} className="polarity-icon" alt={POLARITY_DISPLAY[riven.polarity].name} /> {POLARITY_DISPLAY[riven.polarity].name}</> : ""}</div>
        </div>

        <div className="riven-modal-stats">
          {riven.buffs.map((b, i) => (
            <span key={i} className="riven-stat riven-buff">{rivenStatLabel(b, true, disposition, category, riven.buffs.length, riven.curses.length, riven.mod_rank)}</span>
          ))}
          {riven.curses.map((c, i) => (
            <span key={i} className="riven-stat riven-curse">{rivenStatLabel(c, false, disposition, category, riven.buffs.length, riven.curses.length, riven.mod_rank)}</span>
          ))}
        </div>

        {unmapped.length > 0 && (
          <div className="riven-modal-warn">
            {unmapped.length} stat{unmapped.length > 1 ? "s" : ""} have no WFM attribute mapping and will be omitted from the listing.
          </div>
        )}

        <hr className="riven-modal-divider" />

        <div className="riven-modal-row">
          <span className="riven-modal-label">Type</span>
          <div className="riven-sale-type">
            <button className={`riven-sale-type-btn${saleType === "auction" ? " active" : ""}`}
              onClick={() => setSaleType("auction")}>Auction</button>
            <button className={`riven-sale-type-btn${saleType === "direct" ? " active" : ""}`}
              onClick={() => setSaleType("direct")}>Direct Sale</button>
          </div>
        </div>

        {saleType === "direct" ? (
          <div className="riven-modal-row">
            <span className="riven-modal-label">Selling price (plat)</span>
            <input className="riven-modal-input" type="number" min={1} value={directPrice}
              onChange={e => setDirectPrice(e.target.value)} />
          </div>
        ) : (<>
          <div className="riven-modal-row">
            <span className="riven-modal-label">Starting price (plat)</span>
            <input className="riven-modal-input" type="number" min={1} value={startPrice}
              onChange={e => setStartPrice(e.target.value)} />
          </div>
          <div className="riven-modal-row">
            <span className="riven-modal-label">Buyout price (opt.)</span>
            <input className="riven-modal-input" type="number" min={1} placeholder="—"
              value={buyoutPrice} onChange={e => setBuyoutPrice(e.target.value)} />
          </div>
          <div className="riven-modal-row">
            <span className="riven-modal-label">Min. reputation</span>
            <input className="riven-modal-input" type="number" min={0} max={5} value={minRep}
              onChange={e => setMinRep(e.target.value)} />
          </div>
        </>)}

        <div className="riven-modal-row">
          <span className="riven-modal-label">Note (optional)</span>
          <textarea className="riven-modal-input riven-modal-note" value={note}
            onChange={e => setNote(e.target.value)} />
        </div>
        <div className="riven-modal-row riven-modal-row-toggle">
          <span className="riven-modal-label">Visible on WFM</span>
          <label className="riven-toggle">
            <input type="checkbox" checked={visible} onChange={e => setVisible(e.target.checked)} />
            <span className="riven-toggle-track"><span className="riven-toggle-thumb" /></span>
            <span className="riven-toggle-label">{visible ? "Visible" : "Hidden"}</span>
          </label>
        </div>

        {error && <div className="riven-modal-error">{error}</div>}

        <button className="riven-modal-submit" onClick={handleSubmit} disabled={busy}>
          {busy ? "Posting…" : saleType === "direct" ? "Post Direct Sale on warframe.market" : "Post Auction on warframe.market"}
        </button>
      </div>
    </div>
  );
}

// ── Sell modal for VEILED rivens (WFM regular sell order) ─────────────────────

interface VeiledSellModalProps {
  category: string;
  count:    number;
  onClose:  () => void;
  onSuccess: () => void;
}

function VeiledSellModal({ category, count, onClose, onSuccess }: VeiledSellModalProps) {
  const [price,    setPrice]    = useState("20");
  const [quantity, setQuantity] = useState(String(Math.min(count, 1)));
  const [visible,  setVisible]  = useState(true);
  const [busy,     setBusy]     = useState(false);
  const [error,    setError]    = useState<string | null>(null);

  const slug = VEILED_WFM_SLUG[category];

  async function handleSubmit() {
    const plat = parseInt(price, 10);
    const qty  = parseInt(quantity, 10);
    if (!plat || plat < 1) { setError("Price must be at least 1 platinum."); return; }
    if (!qty  || qty  < 1) { setError("Quantity must be at least 1."); return; }
    if (!slug) { setError(`No WFM listing found for ${category} Riven Mod.`); return; }
    setBusy(true);
    setError(null);
    try {
      const info = await invoke<{ item: { id: string } }>("wfm_get_item_info", { urlName: slug });
      const itemId = info?.item?.id ?? (info as Record<string, Record<string, string>>)?.["data"]?.["id"];
      if (!itemId) throw new Error("Could not find WFM item ID for this riven type.");
      await invoke("wfm_create_order", { itemId, orderType: "sell", platinum: plat, quantity: qty, visible });
      onSuccess();
      onClose();
    } catch (e: unknown) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="riven-modal-overlay" onClick={e => { if (e.target === e.currentTarget) onClose(); }}>
      <div className="riven-modal">
        <div className="riven-modal-title">
          Sell Unrevealed Riven
          <button className="riven-modal-close" onClick={onClose}>×</button>
        </div>

        <div>
          <div className="riven-modal-weapon">{category} Riven Mod (Unrevealed)</div>
          <div className="riven-modal-meta">{count} in inventory</div>
        </div>

        <hr className="riven-modal-divider" />

        <div className="riven-modal-row">
          <span className="riven-modal-label">Price per riven (plat)</span>
          <input className="riven-modal-input" type="number" min={1} value={price}
            onChange={e => setPrice(e.target.value)} />
        </div>
        <div className="riven-modal-row">
          <span className="riven-modal-label">Quantity to list</span>
          <input className="riven-modal-input" type="number" min={1} max={count} value={quantity}
            onChange={e => setQuantity(e.target.value)} />
        </div>
        <div className="riven-modal-row riven-modal-row-toggle">
          <span className="riven-modal-label">Visible on WFM</span>
          <label className="riven-toggle">
            <input type="checkbox" checked={visible} onChange={e => setVisible(e.target.checked)} />
            <span className="riven-toggle-track"><span className="riven-toggle-thumb" /></span>
            <span className="riven-toggle-label">{visible ? "Visible" : "Hidden"}</span>
          </label>
        </div>

        {!slug && (
          <div className="riven-modal-warn">This riven type may not be individually listable on warframe.market.</div>
        )}
        {error && <div className="riven-modal-error">{error}</div>}

        <button className="riven-modal-submit" onClick={handleSubmit} disabled={busy || !slug}>
          {busy ? "Listing…" : "Create Sell Order on warframe.market"}
        </button>
      </div>
    </div>
  );
}


const RivensTab = memo(function RivensTab({ rivens, allItems, wfmUsername, onAuctionPosted }: {
  rivens: BlobRivenEntry[];
  allItems: CatalogItem[];
  wfmUsername: string | null;
  onAuctionPosted?: () => void;
}) {
  const [dispositions, setDispositions] = useState<Record<string, number>>({});
  const [sellTarget,   setSellTarget]   = useState<BlobRivenEntry | null>(null);
  const [sellVeiled,   setSellVeiled]   = useState<BlobRivenEntry | null>(null);

  useEffect(() => {
    invoke<Record<string, number>>("get_weapon_dispositions")
      .then(setDispositions)
      .catch(() => {});
  }, []);

  const pathToName = useMemo(() => {
    const m: Record<string, string> = {};
    for (const it of allItems) m[it.unique_name] = it.name;
    return m;
  }, [allItems]);

  if (rivens.length === 0) {
    return (
      <div className="market-placeholder">
        <p>No rivens found. Inventory blob must be captured at least once while Warframe is running.</p>
      </div>
    );
  }

  const unrevealed = rivens.filter(r => r.riven_state === "unrevealed");
  const revealed   = rivens.filter(r => r.riven_state === "revealed");
  const unlocked   = rivens.filter(r => r.riven_state === "unlocked" || (!r.riven_state && r.compat !== null));

  const sellTargetWeaponName = sellTarget?.compat
    ? (pathToName[sellTarget.compat] ?? sellTarget.compat.split("/").pop() ?? sellTarget.compat)
    : "";
  const sellTargetDisp = sellTarget?.compat ? (dispositions[sellTarget.compat] ?? 1.0) : 1.0;
  const sellTargetCat  = sellTarget ? rivenCategory(sellTarget.item_type) : "";

  return (
    <div className="rivens-tab">


      {unlocked.length > 0 && (
        <section>
          <div className="rivens-section-header">Riven ({unlocked.length})</div>
          <div className="rivens-list">
            {unlocked.map((r, i) => {
              const weaponName = r.compat ? (pathToName[r.compat] ?? r.compat.split("/").pop() ?? r.compat) : "Unknown";
              const disp = r.compat ? (dispositions[r.compat] ?? 1.0) : 1.0;
              const cat  = rivenCategory(r.item_type);
              return (
                <div key={r.item_id || i} className="riven-card">
                  <div className="riven-card-header">
                    <span className="riven-weapon">{weaponName}{(() => { const mn = (r.mod_name || rivenModName(r)); return mn ? <> <span className="riven-mod-name">{mn.replace(/^./, c => c.toUpperCase())}</span></> : null; })()}</span>
                    <button className="riven-sell-btn" title={wfmUsername ? "Post auction on warframe.market" : "Login to WFM to sell"}
                      onClick={() => { if (wfmUsername) setSellTarget(r); else alert("Log in to warframe.market first (Market → Trading tab)."); }}>
                      Sell ↗
                    </button>
                  </div>
                  <div className="riven-card-meta">
                    <span>{cat}</span>
                    <span>MR {r.lvl_req ?? "?"}</span>
                    <span>Rank {r.mod_rank}</span>
                    <span>{disp.toFixed(2)}x</span>
                    <span>{r.rerolls} roll{r.rerolls !== 1 ? "s" : ""}</span>
                    {r.polarity && POLARITY_DISPLAY[r.polarity] && (
                      <span className="riven-polarity"><img src={POLARITY_DISPLAY[r.polarity].icon} className="polarity-icon" alt={POLARITY_DISPLAY[r.polarity].name} /> {POLARITY_DISPLAY[r.polarity].name}</span>
                    )}
                  </div>
                  <div className="riven-stats">
                    {r.buffs.map((b, j) => (
                      <span key={j} className="riven-stat riven-buff">{rivenStatLabel(b, true, disp, cat, r.buffs.length, r.curses.length, r.mod_rank)}</span>
                    ))}
                    {r.curses.map((c, j) => (
                      <span key={j} className="riven-stat riven-curse">{rivenStatLabel(c, false, disp, cat, r.buffs.length, r.curses.length, r.mod_rank)}</span>
                    ))}
                  </div>
                </div>
              );
            })}
          </div>
        </section>
      )}

      {revealed.length > 0 && (
        <section>
          <div className="rivens-section-header">Revealed Riven ({revealed.length})</div>
          <div className="rivens-list">
            {revealed.map((r, i) => {
              const cat = rivenCategory(r.item_type);
              const challenge = formatChallengeName(r.challenge_type, r.challenge_complication);
              return (
                <div key={r.item_id || i} className="riven-card riven-revealed">
                  <div className="riven-card-header">
                    <span className="riven-weapon">{cat} Riven Mod</span>
                    <span className="riven-meta riven-challenge">{challenge}</span>
                  </div>
                </div>
              );
            })}
          </div>
        </section>
      )}

      {unrevealed.length > 0 && (
        <section>
          <div className="rivens-section-header">Unrevealed Riven ({unrevealed.reduce((s, r) => s + r.count, 0)})</div>
          <div className="rivens-list">
            {unrevealed.map((r, i) => (
              <div key={i} className="riven-card riven-veiled">
                <div className="riven-card-header">
                  <span className="riven-weapon">{rivenCategory(r.item_type)} Riven Mod</span>
                  {r.count > 1 && <span className="riven-meta">×{r.count}</span>}
                  <button className="riven-sell-btn" title={wfmUsername ? "List sell order on warframe.market" : "Login to WFM to sell"}
                    onClick={() => { if (wfmUsername) setSellVeiled(r); else alert("Log in to warframe.market first (Market → Trading tab)."); }}>
                    Sell ↗
                  </button>
                </div>
              </div>
            ))}
          </div>
        </section>
      )}

      {sellTarget && (
        <RivenSellModal
          riven={sellTarget}
          weaponName={sellTargetWeaponName}
          disposition={sellTargetDisp}
          category={sellTargetCat}
          onClose={() => setSellTarget(null)}
          onSuccess={() => setTimeout(() => onAuctionPosted?.(), 1500)}
        />
      )}
      {sellVeiled && (
        <VeiledSellModal
          category={rivenCategory(sellVeiled.item_type)}
          count={sellVeiled.count}
          onClose={() => setSellVeiled(null)}
          onSuccess={() => {}}
        />
      )}
    </div>
  );
});
