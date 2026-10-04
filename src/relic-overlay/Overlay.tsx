import { useState, useEffect, useRef } from "react";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";

import { overlayScale } from "../lib/uiScale";
import { PREFERENCE_KEYS } from "../constants/preferences";
import { TAURI_COMMANDS, TAURI_EVENTS } from "../constants/tauri";
import type { CraftingJob, QuantityMap, ShallowRecipeComponent } from "../types/items";
import type { RelicOverlayPriority } from "../types/settings";
import type { PendingRelicRewards, RelicRewardsPayload } from "../types/tauri";
import type { InventoryUpdate } from "../types/inventory";
import "../styles/Overlay.css";

// ── Tailwind class constants (formerly Overlay.css) ───────────────────────────
// Note: :root vars + body transparency stay in Overlay.css (document context).

const OV_ROOT = "relative w-full h-full";
const OV_ARROW =
  "absolute top-0.5 w-9 h-7 pointer-events-none opacity-65 drop-shadow-[0_2px_4px_rgba(0,0,0,0.6)]";

const OV_CARD =
  "absolute top-8.5 bg-surface/82 border border-[var(--border)] rounded-8 pt-1.75 pb-1.5 px-2 flex flex-col gap-1 shadow-[0_4px_16px_rgba(0,0,0,0.55)]";
const OV_CARD_UNKNOWN =
  "absolute top-8.5 bg-unknown-card-bg/82 border border-unknown-card-border/60 rounded-8 pt-1.75 pb-1.5 px-2 flex flex-col gap-1 shadow-[0_4px_16px_rgba(0,0,0,0.55)]";

const OV_NAME =
  "text-14 font-bold text-reward-name leading-1.25 text-center overflow-hidden text-ellipsis line-clamp-2";

const OV_PRICE_ROW = "flex items-center justify-between gap-1";
const OV_PRICE_SHARED =
  "flex items-center gap-0.5 text-13 font-bold whitespace-nowrap";
const OV_PRICE_PLAT = OV_PRICE_SHARED + " text-price-platinum-overlay";
const OV_PRICE_DUCAT = OV_PRICE_SHARED + " text-ducat";
const OV_PRICE_NA = "text-muted font-normal";
const OV_LOCK =
  "text-13 leading-none bg-danger/15 border border-danger/35 rounded-4 px-1 py-0.25 shrink-0";

const OV_OWN_BAR =
  "text-center text-11 font-bold uppercase tracking-wider px-1 py-0.75 rounded-4";
const OV_OWN_YES =
  OV_OWN_BAR +
   " bg-ducat/15 border border-ducat/35 text-ducat";
const OV_OWN_NO =
  OV_OWN_BAR +
  " bg-danger/10 border border-danger/28 text-danger";

const OV_SET_PRICE =
  "flex items-center justify-center gap-0.5 text-13 font-bold text-price-platinum-overlay";

const OV_COMP_GRID = "grid grid-cols-2 gap-0.75";
const OV_GRID_CELL =
  "flex flex-col items-center justify-center px-1 py-0.75 rounded-4 border border-transparent min-w-0 text-center";
const OV_GRID_CELL_OWNED =
   OV_GRID_CELL + " bg-ducat/12 border-ducat/30";
const OV_GRID_CELL_MISS =
  OV_GRID_CELL + " bg-white/2 border-white/6";
const OV_GRID_NAME =
  "text-11 font-semibold whitespace-nowrap overflow-hidden text-ellipsis max-w-full leading-1.3";
const OV_GRID_NAME_OWNED = OV_GRID_NAME + " text-ducat";
const OV_GRID_NAME_MISS = OV_GRID_NAME + " text-muted";
const OV_GRID_QTY =
  "text-10 font-bold leading-none tabular-nums";
const OV_GRID_QTY_OWNED = OV_GRID_QTY + " text-ducat/65";
const OV_GRID_QTY_MISS = OV_GRID_QTY + " text-muted/60";

const OV_CAT =
  "text-10 font-bold uppercase text-muted/50 tracking-wider text-center";

interface ComponentRow {
  unique_name: string;
  name: string;
  needed: number;
  owned: number;  // blueprint_qty + built_qty + crafting_qty combined
  plat?: number;
  ducats?: number;
}

interface RewardItem {
  unique_name: string;
  raw_unique: string;
  slot_x: number;
  name: string;
  category?: string;
  vaulted?: boolean;
  plat?: number;
  ducats?: number;
  owned_qty?: number;
  set_name?: string;
  components?: ComponentRow[];
  complete_sets?: number;
  missing_plat?: number;
  total_plat?: number;   // sum of all component prices × needed qty
}

// ─── Set-name derivation ──────────────────────────────────────────────────────
const WF_COMP_SUFF = [' Neuroptics Blueprint', ' Chassis Blueprint', ' Systems Blueprint'];
const PART_SUFF = [
  ' Upper Limb', ' Lower Limb', ' Receiver', ' Barrel', ' Stock',
  ' Blade', ' Handle', ' Guard', ' Hilt', ' Link', ' Gauntlet',
  ' Carapace', ' Cerebrum', ' Systems', ' Head', ' Strike', ' Boot',
  ' String', ' Disc', ' Neuroptics', ' Chassis', ' Stars',
];

function getSetName(itemName: string): string | null {
  for (const s of WF_COMP_SUFF) if (itemName.endsWith(s)) return itemName.slice(0, -s.length);
  for (const s of PART_SUFF)    if (itemName.endsWith(s)) return itemName.slice(0, -s.length);
  if (itemName.endsWith(' Blueprint')) {
    const base = itemName.slice(0, -' Blueprint'.length);
    for (const s of PART_SUFF) if (base.endsWith(s)) return base.slice(0, -s.length);
    return base;
  }
  return null;
}

function shortName(fullName: string, setName: string): string {
  return fullName.startsWith(setName + ' ') ? fullName.slice(setName.length + 1) : fullName;
}

// ─── Ownership resolver ───────────────────────────────────────────────────────
// Pure function — no closure captures, takes explicit snapshots of qty/crafting.
// Counts the total available copies of a component:
//   directQty  = inventory qty matched by catalog WFCD path
//   altQty     = qty of the complementary form (blueprint ↔ built sub-part)
//   craftQty   = active Foundry jobs for either form
function resolveOwnedFn(
  uniqueName: string,
  displayName: string,
  cat: Record<string, any>,
  qty: QuantityMap,
  crafting: QuantityMap,
): number {
  const clk = uniqueName.replace("/Lotus/StoreItems/", "/Lotus/");
  const catEntry = cat[clk] ?? cat[uniqueName]
    ?? (Object.values(cat).find((it: any) => it.name === displayName) as any);
  const catClk = catEntry
    ? (catEntry.unique_name as string).replace("/Lotus/StoreItems/", "/Lotus/")
    : clk;
  // Also try the raw catalog path (before StoreItems stripping) in case game memory
  // stores the item with the /Lotus/StoreItems/ prefix intact.
  const directQty = qty[catClk]
    ?? (catEntry ? (qty as Record<string,number>)[catEntry.unique_name] : undefined)
    ?? qty[clk]
    ?? qty[uniqueName]
    ?? 0;

  let altQty = 0;
  let altClk: string | null = null;
  if (displayName.endsWith(' Blueprint')) {
    const partName = displayName.slice(0, -' Blueprint'.length);
    const partEntry = Object.values(cat).find(
      (it: any) => it.name === partName && it.category === 'Parts'
    ) as any;
    if (partEntry) {
      altClk = (partEntry.unique_name as string).replace("/Lotus/StoreItems/", "/Lotus/");
      altQty = qty[altClk] ?? qty[partEntry.unique_name] ?? 0;
    }
  } else {
    const bpName = displayName + ' Blueprint';
    const bpEntry = Object.values(cat).find(
      (it: any) => it.name === bpName && it.category === 'Blueprints'
    ) as any;
    if (bpEntry) {
      altClk = (bpEntry.unique_name as string).replace("/Lotus/StoreItems/", "/Lotus/");
      altQty = qty[altClk] ?? qty[bpEntry.unique_name] ?? 0;
    }
  }

  const craftQty = crafting[catClk] ?? crafting[clk] ?? crafting[uniqueName]
    ?? (altClk ? (crafting[altClk] ?? 0) : 0);

  return directQty + altQty + craftQty;
}

// ─── Best-pick calculation ────────────────────────────────────────────────────
function bestPickIndex(items: RewardItem[], priority: RelicOverlayPriority): number {
  if (items.length === 0) return -1;

  const scores = items.map(item => {
    switch (priority) {
      case "plat":    return item.plat ?? 0;
      case "ducat":   return item.ducats ?? 0;
      case "setPlat": return item.missing_plat ?? item.plat ?? 0;
      case "completion": {
        if (!item.components || !item.set_name) return item.plat ?? 0;
        const sn = item.set_name;
        const sn_short = shortName(item.name, sn);
        const thisComp = item.components.find(c => shortName(c.name, sn) === sn_short);
        if (!thisComp) return 0;
        const stillNeed = thisComp.needed - thisComp.owned;
        if (stillNeed <= 0) return -1;
        const totalNeeded = item.components.reduce((a, c) => a + Math.max(0, c.needed - c.owned), 0);
        return stillNeed * 100 - totalNeeded;
      }
    }
  });

  let best = 0;
  for (let i = 1; i < scores.length; i++) if ((scores[i] ?? 0) > (scores[best] ?? 0)) best = i;
  return (scores[best] ?? 0) > 0 ? best : -1;
}

// ─── Icons ────────────────────────────────────────────────────────────────────
function PlatIcon({ size = 14 }: { size?: number }) {
  return <img src="/platinum.webp" alt="p" width={size} height={size} className="shrink-0 object-contain align-middle" />;
}
function DucatIcon({ size = 14 }: { size?: number }) {
  return <img src="/ducats.webp" alt="d" width={size} height={size} className="shrink-0 object-contain align-middle" />;
}

// ─── Column layout ────────────────────────────────────────────────────────────
// Fixed column layout symmetric around screen center.
// Column spacing ≈ 12.7 % of screen width (measured from game at multiple resolutions).
export const COL_SPACING_FRAC = 0.127;

// For N cards, columns map to the game's actual slot positions:
//   N=1: center; N=2: cols 2&3 (±0.5s); N=3: cols 1, 2.5, 4 (±1.5s and 0);
//   N=4: cols 1-4 (±0.5s and ±1.5s).
export function columnCenters(layoutW: number, n: number): number[] {
  const s = layoutW * COL_SPACING_FRAC, c = layoutW / 2;
  if (n === 1) return [c];
  if (n === 2) return [c - 0.5 * s, c + 0.5 * s];
  if (n === 3) return [c - s, c, c + s];
  return [c - 1.5 * s, c - 0.5 * s, c + 0.5 * s, c + 1.5 * s];
}

// ─── Pick arrow ───────────────────────────────────────────────────────────────
function PickArrow({ slotX, winW }: { slotX: number; winW: number }) {
  const cx = Math.round(winW * slotX);
  return (
    <svg className={OV_ARROW} style={{ left: cx - 18 }} viewBox="0 0 36 28" fill="none">
      <polygon points="18,2 34,26 2,26" fill="#f0d060" />
    </svg>
  );
}

// ─── Reward card ──────────────────────────────────────────────────────────────
function RewardCard({ item, left, width }: { item: RewardItem; left: number; width: number }) {
  const hasSet      = !!item.components;
  const ownedFull   = (item.complete_sets ?? 0) >= 1;
  const isUnknown   = item.category === "Unrecognized";

  return (
    <div className={isUnknown ? OV_CARD_UNKNOWN : OV_CARD} style={{ left, width }}>

      {/* Item name */}
      <div className={OV_NAME}>{isUnknown ? "? " : ""}{item.name}</div>

      {/* Plat price | vaulted lock | ducat value */}
      <div className={OV_PRICE_ROW}>
        <span className={OV_PRICE_PLAT}>
          {item.plat != null
            ? <><PlatIcon size={14} />&nbsp;{item.plat}</>
            : <span className={OV_PRICE_NA}>—</span>}
        </span>
        {item.vaulted && <span className={OV_LOCK}>🔒</span>}
        <span className={OV_PRICE_DUCAT}>
          {item.ducats != null && item.ducats > 0
            ? <><DucatIcon size={14} />&nbsp;{item.ducats}</>
            : <span className={OV_PRICE_NA}>—</span>}
        </span>
      </div>

      {/* Set section — only when recipe data is available */}
      {hasSet && <>
        {/* Ownership status bar */}
        <div className={ownedFull ? OV_OWN_YES : OV_OWN_NO}>
          {ownedFull ? 'Full Item Owned' : 'Full Item Not Owned'}
        </div>

        {/* Total set value */}
        <div className={OV_SET_PRICE}>
          <PlatIcon size={14} />
          &nbsp;{(item.total_plat ?? 0) > 0 ? item.total_plat : '—'}
        </div>

        {/* 2×2 component grid */}
        <div className={OV_COMP_GRID}>
          {(item.components ?? []).slice(0, 4).map(c => {
            const raw  = shortName(c.name, item.set_name ?? '');
            // Strip trailing " Blueprint" from the cell label (keep "Blueprint" alone as-is)
            const stripped = raw !== 'Blueprint' && raw.endsWith(' Blueprint')
              ? raw.slice(0, -' Blueprint'.length)
              : raw;
            const owned = c.owned >= c.needed;
            return (
              <div key={c.unique_name} className={owned ? OV_GRID_CELL_OWNED : OV_GRID_CELL_MISS}>
                <span className={owned ? OV_GRID_NAME_OWNED : OV_GRID_NAME_MISS}>{stripped}</span>
                <span className={owned ? OV_GRID_QTY_OWNED : OV_GRID_QTY_MISS}>{c.owned}</span>
              </div>
            );
          })}
        </div>
      </>}

      {/* Category */}
      {item.category && <div className={OV_CAT}>{item.category}</div>}
    </div>
  );
}

// ─── Main overlay ─────────────────────────────────────────────────────────────
export default function Overlay() {
  const [rewards, setRewards] = useState<RewardItem[]>([]);
  // Outline mode (Settings → Overlays → Show Outline): draw an empty frame at
  // the overlay's current position so offsets can be judged without a fissure run.
  const [outline, setOutline] = useState(false);
  // winW = the overlay window's own pixel width, which equals the Warframe client
  // width (App.tsx creates the window with width: ww). No URL param needed.
  //
  // Divide by the scale because the cards sit inside #root, which is 1/scale of
  // the window width and then scaled back up. The scale applies twice to any
  // coordinate measured against the window itself.
  const winW     = (window.innerWidth || 1920) / overlayScale();
  // priority comes from localStorage (shared origin with main window).
  const priority = (localStorage.getItem(PREFERENCE_KEYS.OVERLAY_PRIORITY) ?? "completion") as RelicOverlayPriority;

  const prevKey      = useRef<string>("");
  const sessionCatalogRef = useRef<Record<string, any>>({}); // populated per-session by get_items_by_paths
  const quantRef     = useRef<QuantityMap>({});
  const craftingRef  = useRef<QuantityMap>({});  // normalized unique_name → crafting count

  // Force document-level transparency — only runs when this overlay window mounts,
  // never in the main app. App.css sets background on html/#root which overrides
  // the body-only rule in Overlay.css, so we clear it via JS here.
  useEffect(() => {
    const important = (el: HTMLElement | null) => {
      if (el) el.style.setProperty('background', 'transparent', 'important');
    };
    important(document.documentElement);
    important(document.getElementById('root'));
  }, []);

  useEffect(() => {
    const unsub = listen<string>(TAURI_EVENTS.OVERLAY_OUTLINE, (e) => {
      if (e.payload === "relic") setOutline(true);
      else if (e.payload === "off-relic") setOutline(false);
    });
    return () => { unsub.then(fn => fn()); };
  }, []);

  useEffect(() => {
      invoke(TAURI_COMMANDS.LOG_RELIC_FE, { msg: "[OV] Overlay.tsx useEffect start" }).catch(() => {});
    let pendingEvent: RelicRewardsPayload | null = null;
    let dataReady = false;

    const processPayload = (paths: string[], positions: number[]) => {
      invoke(TAURI_COMMANDS.LOG_RELIC_FE, { msg: `[OV] processPayload(${paths.length} items)` }).catch(() => {});
      const key = paths.join(",");
      if (key === prevKey.current) { invoke(TAURI_COMMANDS.LOG_RELIC_FE, { msg: "[OV] processPayload: duplicate key, skipping" }).catch(() => {}); return; }

      const currentCount = rewards.length;
      if (paths.length <= currentCount && currentCount > 0) { invoke(TAURI_COMMANDS.LOG_RELIC_FE, { msg: `[OV] processPayload: count guard (${paths.length}<=${currentCount}), skipping` }).catch(() => {}); return; }

      prevKey.current = key;

      const byUnique = sessionCatalogRef.current;
      const qty      = quantRef.current;
      invoke(TAURI_COMMANDS.LOG_RELIC_FE, { msg: `[OV] building ${paths.length} base items` }).catch(() => {});
      const base: RewardItem[] = paths.map((path, i) => {
        if (path.startsWith("?:")) {
          return {
            unique_name: path + "-" + i,
            raw_unique:  "",
            slot_x:      positions[i] ?? 0.5,
            name:        path.slice(2),
            category:    "Unrecognized",
          };
        }
        const lk   = path.replace("/Lotus/StoreItems/", "/Lotus/");
        const meta = byUnique[lk] ?? byUnique[path];
        return {
          unique_name: path + "-" + i,
          raw_unique: lk,
          slot_x: positions[i] ?? 0.5,
          name:      meta?.name    ?? path.split("/").pop() ?? path,
          category:  meta?.category,
          vaulted:   meta?.vaulted,
          ducats:    meta?.ducats,
          owned_qty: qty[lk] ?? qty[path] ?? 0,
        };
      });
      invoke(TAURI_COMMANDS.LOG_RELIC_FE, { msg: `[OV] setRewards(${base.length} items)` }).catch(() => {});
      setRewards(base);

      paths.forEach(async (path, i) => {
        // Unknown items — nothing to look up, card already shows raw OCR text.
        if (path.startsWith("?:")) return;

        const lk   = path.replace("/Lotus/StoreItems/", "/Lotus/");
        const meta = byUnique[lk] ?? byUnique[path];
        const name = meta?.name ?? path.split("/").pop() ?? path;

        invoke<number | null>("get_item_price", { itemName: name })
          .then(plat => { if (plat != null) setRewards(prev => prev.map((r, idx) => idx === i ? { ...r, plat } : r)); })
          .catch(() => {});

        const setName = getSetName(name);
        if (!setName) return;

        // Read the catalog once for this card's async chain; quantities are read
        // from quantRef.current AFTER the recipe await so they reflect any
        // inventory-update that fired while we were waiting.
        const cat       = sessionCatalogRef.current;
        const setPrefix = setName + " ";

        let components: ComponentRow[] | null = null;

        // The "built item" entry (weapon/warframe entity, not blueprint or part).
        const setEntry = Object.values(cat).find(item =>
          item.name === setName &&
          item.category !== 'Blueprints' &&
          item.category !== 'Parts'
        );

        // Attempt 1: recipe lookup (gives exact ingredient list + correct needed counts)
        if (setEntry) {
          const recipe = await invoke<ShallowRecipeComponent[]>(TAURI_COMMANDS.GET_RECIPE, { unique_name: setEntry.unique_name }).catch(() => []);
          if (recipe.length) {
            // Filter out raw resources (Rubedo, Circuits etc.) — show only craftable parts
            const parts = recipe.filter(c => {
              const clk = c.unique_name.replace("/Lotus/StoreItems/", "/Lotus/");
              const cm  = cat[clk] ?? cat[c.unique_name];
              return cm?.category === 'Parts' || cm?.category === 'Blueprints';
            });
            if (parts.length >= 1) {
              // Read quantities AFTER the recipe await — scanner may have committed
              // items to current_quantities during the async gap.
              const liveQty = quantRef.current;
              const liveCraft = craftingRef.current;
              components = parts.map(c => {
                const clk = c.unique_name.replace("/Lotus/StoreItems/", "/Lotus/");
                const cm  = cat[clk] ?? cat[c.unique_name];
                return {
                  unique_name: c.unique_name,
                  name:        c.name,
                  needed:      c.count ?? 1,
                  owned:       resolveOwnedFn(c.unique_name, c.name, cat, liveQty, liveCraft),
                  ducats:      cm?.ducats,
                };
              });
            }
          }
        }

        // Attempt 2: catalog prefix search — works even when recipe cache is empty.
        // Needed count defaults to 1 (imprecise for parts that require 2, but better than nothing).
        if (!components) {
          const liveQty   = quantRef.current;
          const liveCraft = craftingRef.current;
          const parts = Object.values(cat)
            .filter((item: any) => item.name?.startsWith(setPrefix) &&
                            (item.category === 'Parts' || item.category === 'Blueprints'))
            .map((item: any) => ({
              unique_name: item.unique_name,
              name:        item.name as string,
              needed:      1,
              owned:       resolveOwnedFn(item.unique_name, item.name as string, cat, liveQty, liveCraft),
              ducats:      item.ducats as number | undefined,
            }));
          if (parts.length >= 2) components = parts;
        }

        if (!components) return;

        // "Owned" = user has enough components to build, OR already built the item.
        const liveQty  = quantRef.current;
        const compSets = components.length > 0
          ? Math.floor(Math.min(...components.map(c => c.owned / c.needed)))
          : 0;
        const builtLk  = (setEntry?.unique_name ?? '').replace("/Lotus/StoreItems/", "/Lotus/");
        const builtQty = setEntry
          ? (liveQty[builtLk] ?? liveQty[setEntry.unique_name] ?? 0)
          : 0;
        const completeSets = builtQty > 0 ? Math.max(compSets, 1) : compSets;

        setRewards(prev => prev.map((r, idx) => idx === i
          ? { ...r, set_name: setName, components: components!, complete_sets: completeSets, missing_plat: 0, total_plat: 0 }
          : r
        ));

        await Promise.all(components.map(async comp => {
          const plat = await invoke<number | null>("get_item_price", { itemName: comp.name }).catch(() => null);
          if (plat == null) return;
          setRewards(prev => prev.map((r, idx) => {
            if (idx !== i || !r.components) return r;
            const comps = r.components.map(c =>
              c.unique_name === comp.unique_name ? { ...c, plat } : c
            );
            const missing_plat = comps.reduce((acc, c) => {
              if (c.owned < c.needed && c.plat) return acc + c.plat * (c.needed - c.owned);
              return acc;
            }, 0);
            const total_plat = comps.reduce((acc, c) =>
              c.plat != null ? acc + c.plat * (c.needed ?? 1) : acc, 0
            );
            return { ...r, components: comps, missing_plat, total_plat };
          }));
        }));
      });
    };

    // Clear stale rewards when a new fissure starts so the diagnostic div shows
    // while OCR is running instead of the previous fissure's stale cards.
    const unsubTrigger = listen<null>(TAURI_EVENTS.RELIC_TRIGGER, () => {
      invoke(TAURI_COMMANDS.LOG_RELIC_FE, { msg: "[OV] relic-trigger → clearing rewards" }).catch(() => {});
      setRewards([]);
      prevKey.current = "";
    });

    invoke(TAURI_COMMANDS.LOG_RELIC_FE, { msg: "[OV] registering relic-rewards listener" }).catch(() => {});
    const unsub = listen<PendingRelicRewards>(
      TAURI_EVENTS.RELIC_REWARDS,
      async (e) => {
        const payload = e.payload;
        invoke(TAURI_COMMANDS.LOG_RELIC_FE, { msg: `[OV] relic-rewards event: items=${payload?.items?.length ?? "null"} dataReady=${dataReady} catalogSize=${Object.keys(sessionCatalogRef.current).length}` }).catch(() => {});
        if (!payload || payload.items.length === 0) {
          invoke(TAURI_COMMANDS.LOG_RELIC_FE, { msg: "[OV] null/empty payload → moving off-screen" }).catch(() => {});
          setRewards([]);
          prevKey.current = "";
          invoke(TAURI_COMMANDS.MOVE_OVERLAY_OFFSCREEN).catch(() => {});
          return;
        }

        if (dataReady) {
          // Fetch only the items relevant to this relic session (the reward items +
          // their full prime set siblings for the component grid).  Rust already has
          // the catalog loaded at this point, so this is a fast targeted lookup.
          try {
            const items = await invoke<any[]>("get_items_by_paths", { paths: payload.items });
            const byUnique: Record<string, any> = {};
            for (const i of items) byUnique[i.unique_name] = i;
            sessionCatalogRef.current = byUnique;
            invoke(TAURI_COMMANDS.LOG_RELIC_FE, { msg: `[OV] session catalog: ${items.length} items for ${payload.items.length} rewards` }).catch(() => {});
          } catch (err) {
            invoke(TAURI_COMMANDS.LOG_RELIC_FE, { msg: `[OV] get_items_by_paths failed: ${err}` }).catch(() => {});
          }
          processPayload(payload.items, payload.positions);
        } else {
          invoke(TAURI_COMMANDS.LOG_RELIC_FE, { msg: "[OV] buffering event (dataReady=false)" }).catch(() => {});
          pendingEvent = payload;
        }
      }
    );

    // Pull any rewards already stored in AppState — guards against the gap between
    // tauri://created firing (in App.tsx) and this listener being registered.
    // .take() on the Rust side clears the stored value atomically, so there is no
    // double-processing if the listener also receives the event.
    invoke(TAURI_COMMANDS.LOG_RELIC_FE, { msg: "[OV] calling get_pending_relic_rewards" }).catch(() => {});
    invoke<PendingRelicRewards>("get_pending_relic_rewards")
      .then(pending => {
        invoke(TAURI_COMMANDS.LOG_RELIC_FE, { msg: `[OV] pull result: ${pending ? pending.items.length + " items" : "null"}` }).catch(() => {});
        if (pending && pending.items.length > 0) {
          if (dataReady) {
            processPayload(pending.items, pending.positions);
          } else {
            pendingEvent = pending;
          }
        }
      })
      .catch((err) => { invoke(TAURI_COMMANDS.LOG_RELIC_FE, { msg: `[OV] pull error: ${err}` }).catch(() => {}); });

    invoke(TAURI_COMMANDS.LOG_RELIC_FE, { msg: "[OV] starting Promise.allSettled for qty/crafting" }).catch(() => {});
    Promise.allSettled([
      invoke<QuantityMap>(TAURI_COMMANDS.GET_CURRENT_QUANTITIES),
      invoke<CraftingJob[]>("get_current_crafting"),
    ]).then(async ([quantitiesR, craftingR]) => {
      if (quantitiesR.status === 'fulfilled') {
        quantRef.current = quantitiesR.value;
      }
      if (craftingR.status === 'fulfilled') {
        const byCraft: QuantityMap = {};
        for (const job of craftingR.value) {
          const lk = job.unique_name.replace("/Lotus/StoreItems/", "/Lotus/");
          byCraft[lk] = (byCraft[lk] ?? 0) + 1;
        }
        craftingRef.current = byCraft;
      }
      dataReady = true;
      invoke(TAURI_COMMANDS.LOG_RELIC_FE, { msg: `[OV] dataReady=true — pendingEvent=${pendingEvent ? pendingEvent.items.length + " items" : "null"}` }).catch(() => {});

      if (pendingEvent) {
        const ev = pendingEvent;
        pendingEvent = null;
        try {
          const items = await invoke<any[]>("get_items_by_paths", { paths: ev.items });
          const byUnique: Record<string, any> = {};
          for (const i of items) byUnique[i.unique_name] = i;
          sessionCatalogRef.current = byUnique;
          invoke(TAURI_COMMANDS.LOG_RELIC_FE, { msg: `[OV] pending: session catalog: ${items.length} items` }).catch(() => {});
        } catch (err) {
          invoke(TAURI_COMMANDS.LOG_RELIC_FE, { msg: `[OV] pending: get_items_by_paths failed: ${err}` }).catch(() => {});
        }
        processPayload(ev.items, ev.positions);
      }
    });

    // On every inventory scan, fully recompute component owned counts AND the
    // built-item check.  This fixes the race where processPayload ran before the
    // scanner had committed items (all counts showed 0), and ensures warframes
    // that appear in unique_quantities after 2+ consecutive scans flip the card.
    const unsubInv = listen<InventoryUpdate>(TAURI_EVENTS.INVENTORY_UPDATE, (e) => {
      const newQty = e.payload?.quantities;
      if (!newQty) return;
      quantRef.current = newQty;

      setRewards(prev => prev.map(r => {
        if (!r.components || !r.set_name) return r;

        const cat = sessionCatalogRef.current;

        // Recompute every component's owned count with the fresh quantities.
        const updatedComponents = r.components.map(c => ({
          ...c,
          owned: resolveOwnedFn(c.unique_name, c.name, cat, newQty, craftingRef.current),
        }));

        const compSets = updatedComponents.length > 0
          ? Math.floor(Math.min(...updatedComponents.map(c => c.owned / c.needed)))
          : 0;

        // Re-check whether the assembled item (warframe / weapon) is already owned.
        const setEntry = Object.values(cat).find((item: any) =>
          item.name === r.set_name &&
          item.category !== 'Blueprints' &&
          item.category !== 'Parts'
        ) as any;
        const builtLk  = setEntry
          ? (setEntry.unique_name as string).replace("/Lotus/StoreItems/", "/Lotus/")
          : '';
        const builtQty = setEntry
          ? (newQty[builtLk] ?? newQty[setEntry.unique_name] ?? 0)
          : 0;

        const completeSets = builtQty > 0 ? Math.max(compSets, 1) : compSets;

        return { ...r, components: updatedComponents, complete_sets: completeSets };
      }));
    });

    return () => { unsub.then(fn => fn()); unsubInv.then(fn => fn()); unsubTrigger.then(fn => fn()); };
  }, []);

  if (rewards.length === 0 && !outline) return null;

  if (rewards.length === 0) {
    // Outline: dashed frame at the window's current position.
    return (
      <div className={OV_ROOT}>
        <div className="absolute inset-x-8 top-10 bottom-10 flex items-center justify-center rounded-10 border-2 border-dashed border-accent/85 bg-surface/55 text-22 font-semibold text-overlay-outline">
          Relic Reward Overlay — outline
        </div>
      </div>
    );
  }

  const bestIdx = bestPickIndex(rewards, priority);

  const n  = rewards.length;
  const screenCenter = winW / 2;
  const colCenters   = columnCenters(winW, n);
  const cardW = Math.max(80, Math.round(winW * COL_SPACING_FRAC - 10));
  const cardLeft = (idx: number) => Math.round((colCenters[idx] ?? screenCenter) - cardW / 2);

  return (
    <div className={OV_ROOT}>
      {bestIdx >= 0 && <PickArrow slotX={(colCenters[bestIdx] ?? screenCenter) / winW} winW={winW} />}
      {rewards.map((item, idx) => (
        <RewardCard key={item.unique_name} item={item} left={cardLeft(idx)} width={cardW} />
      ))}
    </div>
  );
}
