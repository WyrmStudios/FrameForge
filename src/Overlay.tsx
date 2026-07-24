import { useState, useEffect, useRef } from "react";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";

import "./Overlay.css";

export type PickPriority = "completion" | "plat" | "ducat" | "setPlat";

interface ComponentRow {
  unique_name: string;
  name: string;
  needed: number;
  owned: number;  // blueprint_qty + built_qty + crafting_qty combined
  plat?: number;
  ducats?: number;
}

interface CraftingJobTs {
  unique_name: string;
  item_name: string;
  completion_ms: number;
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
  qty: Record<string, number>,
  crafting: Record<string, number>,
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
function bestPickIndex(items: RewardItem[], priority: PickPriority): number {
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
  return <img src="/platinum.webp" alt="p" width={size} height={size} style={{ objectFit: "contain", flexShrink: 0, verticalAlign: "middle" }} />;
}
function DucatIcon({ size = 14 }: { size?: number }) {
  return <img src="/ducats.webp" alt="d" width={size} height={size} style={{ objectFit: "contain", flexShrink: 0, verticalAlign: "middle" }} />;
}

// ─── Pick arrow ───────────────────────────────────────────────────────────────
function PickArrow({ slotX, winW }: { slotX: number; winW: number }) {
  const cx = Math.round(winW * slotX);
  return (
    <svg className="ov-arrow" style={{ left: cx - 18 }} viewBox="0 0 36 28" fill="none">
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
    <div className={`ov-card${isUnknown ? " ov-card-unknown" : ""}`} style={{ left, width }}>

      {/* Item name */}
      <div className="ov-name">{isUnknown ? "? " : ""}{item.name}</div>

      {/* Plat price | vaulted lock | ducat value */}
      <div className="ov-price-row">
        <span className="ov-price-plat">
          {item.plat != null
            ? <><PlatIcon size={14} />&nbsp;{item.plat}</>
            : <span className="ov-price-na">—</span>}
        </span>
        {item.vaulted && <span className="ov-lock">🔒</span>}
        <span className="ov-price-ducat">
          {item.ducats != null && item.ducats > 0
            ? <><DucatIcon size={14} />&nbsp;{item.ducats}</>
            : <span className="ov-price-na">—</span>}
        </span>
      </div>

      {/* Set section — only when recipe data is available */}
      {hasSet && <>
        {/* Ownership status bar */}
        <div className={`ov-own-bar ${ownedFull ? 'ov-own-yes' : 'ov-own-no'}`}>
          {ownedFull ? 'Full Item Owned' : 'Full Item Not Owned'}
        </div>

        {/* Total set value */}
        <div className="ov-set-price">
          <PlatIcon size={14} />
          &nbsp;{(item.total_plat ?? 0) > 0 ? item.total_plat : '—'}
        </div>

        {/* 2×2 component grid */}
        <div className="ov-comp-grid">
          {(item.components ?? []).slice(0, 4).map(c => {
            const raw  = shortName(c.name, item.set_name ?? '');
            // Strip trailing " Blueprint" from the cell label (keep "Blueprint" alone as-is)
            const stripped = raw !== 'Blueprint' && raw.endsWith(' Blueprint')
              ? raw.slice(0, -' Blueprint'.length)
              : raw;
            const owned = c.owned >= c.needed;
            return (
              <div key={c.unique_name} className={`ov-grid-cell ${owned ? 'ov-grid-owned' : 'ov-grid-miss'}`}>
                <span className="ov-grid-name">{stripped}</span>
                <span className="ov-grid-qty">{c.owned}</span>
              </div>
            );
          })}
        </div>
      </>}

      {/* Category */}
      {item.category && <div className="ov-cat">{item.category}</div>}
    </div>
  );
}

// ─── Main overlay ─────────────────────────────────────────────────────────────
export default function Overlay() {
  const [rewards, setRewards] = useState<RewardItem[]>([]);
  // winW = the overlay window's own pixel width, which equals the Warframe client
  // width (App.tsx creates the window with width: ww). No URL param needed.
  const winW     = window.innerWidth || 1920;
  // priority comes from localStorage (shared origin with main window).
  const priority = (localStorage.getItem("ff-overlay-priority") ?? "completion") as PickPriority;

  const prevKey      = useRef<string>("");
  const catalogRef   = useRef<Record<string, any>>({});
  const quantRef     = useRef<Record<string, number>>({});
  const craftingRef  = useRef<Record<string, number>>({});  // normalized unique_name → crafting count

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
    invoke("log_relic_fe", { msg: "[OV] Overlay.tsx useEffect start" }).catch(() => {});
    type EventPayload = { paths: string[]; positions: number[] };
    let pendingEvent: EventPayload | null = null;
    let dataReady = false;

    const processPayload = (paths: string[], positions: number[]) => {
      invoke("log_relic_fe", { msg: `[OV] processPayload(${paths.length} items)` }).catch(() => {});
      const key = paths.join(",");
      if (key === prevKey.current) { invoke("log_relic_fe", { msg: "[OV] processPayload: duplicate key, skipping" }).catch(() => {}); return; }

      const currentCount = rewards.length;
      if (paths.length <= currentCount && currentCount > 0) { invoke("log_relic_fe", { msg: `[OV] processPayload: count guard (${paths.length}<=${currentCount}), skipping` }).catch(() => {}); return; }

      prevKey.current = key;

      const byUnique = catalogRef.current;
      const qty      = quantRef.current;
      invoke("log_relic_fe", { msg: `[OV] building ${paths.length} base items` }).catch(() => {});
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
      invoke("log_relic_fe", { msg: `[OV] setRewards(${base.length} items)` }).catch(() => {});
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
        const cat       = catalogRef.current;
        const setPrefix = setName + " ";

        type RC = { unique_name: string; name: string; count: number; result_count: number };
        let components: ComponentRow[] | null = null;

        // The "built item" entry (weapon/warframe entity, not blueprint or part).
        const setEntry = Object.values(cat).find(item =>
          item.name === setName &&
          item.category !== 'Blueprints' &&
          item.category !== 'Parts'
        );

        // Attempt 1: recipe lookup (gives exact ingredient list + correct needed counts)
        if (setEntry) {
          const recipe = await invoke<RC[]>("get_recipe", { unique_name: setEntry.unique_name }).catch(() => []);
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
    const unsubTrigger = listen<null>("relic-trigger", () => {
      invoke("log_relic_fe", { msg: "[OV] relic-trigger → clearing rewards" }).catch(() => {});
      setRewards([]);
      prevKey.current = "";
    });

    invoke("log_relic_fe", { msg: "[OV] registering relic-rewards listener" }).catch(() => {});
    const unsub = listen<{ items: string[]; positions: number[] } | null>(
      "relic-rewards",
      async (e) => {
        const payload = e.payload;
        invoke("log_relic_fe", { msg: `[OV] relic-rewards event: items=${payload?.items?.length ?? "null"} dataReady=${dataReady}` }).catch(() => {});
        if (!payload || payload.items.length === 0) {
          invoke("log_relic_fe", { msg: "[OV] null/empty payload → moving off-screen" }).catch(() => {});
          setRewards([]);
          prevKey.current = "";
          invoke("move_overlay_offscreen").catch(() => {});
          return;
        }

        if (dataReady) {
          processPayload(payload.items, payload.positions);
        } else {
          invoke("log_relic_fe", { msg: "[OV] buffering event (dataReady=false)" }).catch(() => {});
          pendingEvent = { paths: payload.items, positions: payload.positions };
        }
      }
    );

    // Pull any rewards already stored in AppState — guards against the gap between
    // tauri://created firing (in App.tsx) and this listener being registered.
    // .take() on the Rust side clears the stored value atomically, so there is no
    // double-processing if the listener also receives the event.
    invoke("log_relic_fe", { msg: "[OV] calling get_pending_relic_rewards" }).catch(() => {});
    invoke<{ items: string[]; positions: number[] } | null>("get_pending_relic_rewards")
      .then(pending => {
        invoke("log_relic_fe", { msg: `[OV] pull result: ${pending ? pending.items.length + " items" : "null"}` }).catch(() => {});
        if (pending && pending.items.length > 0) {
          if (dataReady) {
            processPayload(pending.items, pending.positions);
          } else {
            pendingEvent = { paths: pending.items, positions: pending.positions };
          }
        }
      })
      .catch((err) => { invoke("log_relic_fe", { msg: `[OV] pull error: ${err}` }).catch(() => {}); });

    invoke("log_relic_fe", { msg: "[OV] starting Promise.allSettled for catalog/qty/crafting" }).catch(() => {});
    Promise.allSettled([
      invoke<any[]>("get_all_items"),
      invoke<Record<string, number>>("get_current_quantities"),
      invoke<CraftingJobTs[]>("get_current_crafting"),
    ]).then(([itemsR, quantitiesR, craftingR]) => {
      if (itemsR.status === 'fulfilled') {
        const byUnique: Record<string, any> = {};
        for (const i of itemsR.value) byUnique[i.unique_name] = i;
        catalogRef.current = byUnique;
      }
      if (quantitiesR.status === 'fulfilled') {
        quantRef.current = quantitiesR.value;
      }
      if (craftingR.status === 'fulfilled') {
        const byCraft: Record<string, number> = {};
        for (const job of craftingR.value) {
          const lk = job.unique_name.replace("/Lotus/StoreItems/", "/Lotus/");
          byCraft[lk] = (byCraft[lk] ?? 0) + 1;
        }
        craftingRef.current = byCraft;
      }
      dataReady = true;
      invoke("log_relic_fe", { msg: `[OV] dataReady=true — pendingEvent=${pendingEvent ? pendingEvent.paths.length + " items" : "null"}` }).catch(() => {});

      if (pendingEvent) {
        processPayload(pendingEvent.paths, pendingEvent.positions);
        pendingEvent = null;
      }
    });

    // On every inventory scan, fully recompute component owned counts AND the
    // built-item check.  This fixes the race where processPayload ran before the
    // scanner had committed items (all counts showed 0), and ensures warframes
    // that appear in unique_quantities after 2+ consecutive scans flip the card.
    const unsubInv = listen<{ quantities: Record<string, number> }>("inventory-update", (e) => {
      const newQty = e.payload?.quantities;
      if (!newQty) return;
      quantRef.current = newQty;

      // Self-heal: if catalog is still empty (lost the race with load_wfcd_data on startup),
      // re-fetch now that the scanner has confirmed items are loaded.
      if (Object.keys(catalogRef.current).length === 0) {
        invoke<any[]>("get_all_items").then(items => {
          if (items.length > 0) {
            const byUnique: Record<string, any> = {};
            for (const i of items) byUnique[i.unique_name] = i;
            catalogRef.current = byUnique;
          }
        }).catch(() => {});
      }

      setRewards(prev => prev.map(r => {
        if (!r.components || !r.set_name) return r;

        const cat = catalogRef.current;

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

  if (rewards.length === 0) return null;

  const bestIdx = bestPickIndex(rewards, priority);

  const n  = rewards.length;
  // Fixed column layout symmetric around screen center.
  // Column spacing ≈ 12.7 % of screen width (measured from game at multiple resolutions).
  // For N cards, columns map to the game's actual slot positions:
  //   N=1: center; N=2: cols 2&3 (±0.5s); N=3: cols 1, 2.5, 4 (±1.5s and 0);
  //   N=4: cols 1-4 (±0.5s and ±1.5s).
  const colSpacing    = winW * 0.127;
  const screenCenter  = winW / 2;
  const cardW = Math.max(80, Math.round(colSpacing - 10));
  const colCenters: number[] = (() => {
    const s = colSpacing, c = screenCenter;
    if (n === 1) return [c];
    if (n === 2) return [c - 0.5 * s, c + 0.5 * s];
    if (n === 3) return [c - s, c, c + s];
    return [c - 1.5 * s, c - 0.5 * s, c + 0.5 * s, c + 1.5 * s];
  })();
  const cardLeft = (idx: number) => Math.round((colCenters[idx] ?? screenCenter) - cardW / 2);

  return (
    <div className="ov-root">
      {bestIdx >= 0 && <PickArrow slotX={(colCenters[bestIdx] ?? screenCenter) / winW} winW={winW} />}
      {rewards.map((item, idx) => (
        <RewardCard key={item.unique_name} item={item} left={cardLeft(idx)} width={cardW} />
      ))}
    </div>
  );
}
