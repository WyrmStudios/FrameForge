import { useState, useEffect, useMemo } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { HelpTip } from "./HelpTip";
import WfmTrading from "./WfmTrading";
import ItemMarketPopup from "./ItemMarketPopup";
import type { InventoryItem } from "./App";

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
  activeMarketTab: "sets" | "trading";
}
export const MARKET_FILTERS_DEFAULT: MarketFilters = {
  search: "", ownership: [], conditions: [], vault: [], sortMode: "ducats",
  activeMarketTab: "sets",
};

interface Props {
  inventory: Record<string, InventoryItem>;
  refreshKey: number;
  crafting: CraftingJob[];
  onWfmLoginChange?: (loggedIn: boolean) => void;
  filters: MarketFilters;
  onFiltersChange: (f: MarketFilters) => void;
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

function ItemImg({ imageName, size = 32 }: { imageName?: string; size?: number }) {
  const [failed, setFailed] = useState(false);
  const s = { width: size, height: size, objectFit: "contain" as const, flexShrink: 0, borderRadius: 4 };
  if (!imageName || failed)
    return <span style={{ ...s, background: "rgba(255,255,255,.06)", border: "1px solid #30363d", display: "flex", alignItems: "center", justifyContent: "center", fontSize: size * .3, color: "#8b949e" }}>P</span>;
  return <img style={s} src={`https://cdn.warframestat.us/img/${imageName}`} alt="" loading="lazy" onError={() => setFailed(true)} />;
}

// ─── Set card ─────────────────────────────────────────────────────────────────

interface SetPart { item: CatalogItem; qty: number; sellMedian?: number; loading: boolean; urlName: string; }

function SetCard({ setKey, parts, parentItem, setPrice, setPriceLoading, pricesFetched, crafting, onCardClick, onPartClick }: {
  setKey: string; parts: SetPart[]; parentItem?: CatalogItem;
  setPrice?: WfmPrice; setPriceLoading: boolean; pricesFetched: boolean;
  crafting: CraftingJob[]; onCardClick?: () => void;
  onPartClick?: (urlName: string, displayName: string, imageName?: string) => void;
}) {
  const totalDucats  = parts.reduce((s, p) => s + (p.item.ducats ?? 0) * p.qty, 0);
  const ownedCount   = parts.filter(p => p.qty > 0).length;
  const isComplete   = ownedCount === parts.length;
  const hasDupes     = parts.some(p => p.qty > 1);
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

export default function MarketHelper({ inventory, refreshKey, crafting, onWfmLoginChange, filters, onFiltersChange }: Props) {
  const [allItems, setAllItems]           = useState<CatalogItem[]>([]);
  const [wfmItems, setWfmItems]           = useState<WfmItem[]>([]);
  const [wfmLoading, setWfmLoading]       = useState(false);
  const [wfmError, setWfmError]           = useState(false);
  const [prices, setPrices]               = useState<Map<string, WfmPrice>>(new Map());
  const [wfmBadge, setWfmBadge]           = useState(0);
  const [wfmUsername, setWfmUsername]     = useState<string | null>(null);
  const [popup, setPopup] = useState<{ urlName: string; displayName: string; imageName?: string } | null>(null);
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
  useEffect(() => {
    const unlisten = listen<{ url_name: string; sell_median: number | null }>(
      "wfm-price-update",
      ({ payload }) => {
        setPrices(prev => {
          const m = new Map(prev);
          m.set(payload.url_name, { url_name: payload.url_name, sell_median: payload.sell_median ?? undefined });
          return m;
        });
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
    primeItems.reduce((s, i) => s + (i.ducats ?? 0) * Math.max(0, (inventory[i.unique_name]?.quantity ?? 0) - 1), 0),
  [primeItems, inventory]);

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
          const hasDupes   = parts.some(p => (inventory[p.unique_name]?.quantity ?? 0) > 1);
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
  }, [sets, inventory, ownership, conditions, vault, sortMode, search, parentItems, prices, wfmLookup]);

  return (
    <div className="market-helper">
      {/* ── Market tab strip ── */}
      <div className="market-tab-strip">
        <button className={activeMarketTab === "sets" ? "active" : ""} onClick={() => set("activeMarketTab", "sets")}>
          Prime Sets
        </button>
        <button className={activeMarketTab === "trading" ? "active" : ""} onClick={() => { set("activeMarketTab", "trading"); setWfmBadge(0); }}>
          Trading {wfmBadge > 0 && <span className="market-tab-badge">{wfmBadge}</span>}
        </button>
      </div>

      {activeMarketTab === "trading" && (
        <WfmTrading
          wfmLookup={wfmLookup}
          wfmItems={wfmItems}
          imageMap={imageMap}
          inventory={inventory}
          onNewWhisper={() => { if (activeMarketTab !== "trading") setWfmBadge(n => n + 1); }}
          onLoginChange={u => setWfmUsername(u)}
        />
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
          onClose={() => setPopup(null)}
          isLoggedIn={!!wfmUsername}
          myUsername={wfmUsername ?? undefined}
        />
      )}
    </div>
  );
}
