import React, { useState, useEffect, useMemo, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { ResizeHandle } from "../shared/ResizeHandle";
import { TAURI_COMMANDS } from "../constants/tauri";
import { TIMER_LABELS } from "../constants/timers";
import { getTimerInfo, fmtMs, matchesWatch } from "../TimerHelper";
import type { FissureWatch } from "../types/settings";
import type { CatalogItem, InventoryItem, RecipeComponent, RecipeComponentStatus } from "../types/items";

const TRACKING_TOGGLE = "flex gap-0.5";
const TRACKING_TOGGLE_BTN =
  "px-1.75 py-0.5 rounded-4 border border-border bg-transparent text-muted cursor-pointer text-10 transition-all duration-150 hover:border-accent! hover:text-accent!";
const TRACKING_TOGGLE_ON = "border-accent! text-accent! bg-accent/10!";
import type { MatchedFissure } from "../types/worldstate";
import { useWorldState } from "../worldstate";

// ── Tailwind class constants (formerly ModularWindow.css) ─────────────────────

const MW_WINDOW =
  "relative shrink-0 flex flex-row border-l border-l-[var(--border)] bg-surface overflow-hidden min-h-0";
const MW_WINDOW_DOCKED =
  "max-w-[min(500px,max(160px,calc(100vw_-_582px)))]";
const MW_RESIZE =
  "absolute z-2 top-0 bottom-0 left-0 w-2 cursor-col-resize bg-transparent transition-[background] duration-150 hover:bg-accent/35";
const MW_INNER =
  "flex-1 flex flex-col overflow-y-auto overflow-x-hidden min-h-0 min-w-0";
const MW_HEADER =
  "flex items-center px-3 pt-2 pb-1.5 border-b border-b-[var(--border)] shrink-0";
const MW_TITLE =
  "text-11 font-bold text-muted uppercase tracking-wider flex-1";

const MW_SECTION_WRAP = "flex flex-col shrink-0";
const MW_SECTION_HEADER =
  "flex items-center gap-1.25 pt-1.25 pb-1 pr-2 pl-2.5 shrink-0 select-none";
const MW_SECTION_LABEL =
  "text-10 font-bold text-muted uppercase tracking-0.04 flex-1";
const MW_SECTION_ARROWS = "flex gap-0.25 shrink-0";
const MW_ARROW_BTN =
  "bg-transparent border-0 cursor-pointer text-muted px-1 py-0.75 flex items-center justify-center rounded-3 transition-[background,color] duration-100 leading-none enabled:hover:bg-white/10 enabled:hover:text-foreground disabled:opacity-20 disabled:cursor-default";
const MW_ARROW_BTN_ITEM =
  "bg-transparent border-0 cursor-pointer text-muted px-1.25 py-0.5 flex items-center justify-center rounded-3 transition-[background,color] duration-100 leading-none enabled:hover:bg-white/10 enabled:hover:text-foreground disabled:opacity-20 disabled:cursor-default";
const MW_ARROW_SVG = "w-2.5 h-1.5 block";
const MW_ARROW_SVG_ITEM = "w-3.25 h-2 block";
const MW_EMPTY =
  "px-3 py-2 text-11 text-muted text-center leading-1.4";
const MW_DIVIDER = "h-px bg-[var(--border)] shrink-0";

const MW_TRACKED_LIST = "shrink-0";
const MW_GROUP =
  "border-b-2 border-b-border/70 last:border-b-0";
const MW_TRACKED_ROW =
  "flex items-center gap-1 pt-0.75 pb-0.75 pr-2 pl-1 bg-white/2 transition-[background] duration-100 hover:bg-white/6";
const MW_ITEM_ARROWS = "flex flex-col gap-0 shrink-0";
const MW_NAME_AREA = "flex items-center gap-1 flex-1 min-w-0";
const MW_NAME_AREA_REQS =
  "flex items-center gap-1 flex-1 min-w-0 cursor-pointer group";
const MW_CHEVRON =
  "w-2.5 h-1.5 shrink-0 text-muted transition-transform duration-150";
const MW_CHEVRON_COLLAPSED =
  "w-2.5 h-1.5 shrink-0 text-muted transition-transform duration-150 -rotate-90";
const MW_ITEM_NAME =
  "text-11 text-foreground whitespace-nowrap overflow-hidden text-ellipsis min-w-0";
const MW_ITEM_STATUS = "text-10 font-bold shrink-0 w-3.5 text-center";
const MW_REMOVE_BTN =
  "bg-transparent border-0 cursor-pointer text-muted text-14 p-0 shrink-0 leading-none transition-colors duration-100 hover:text-danger";
const MW_INLINE_REQS =
  "pt-0.5 pb-1.25 pr-2 pl-7 border-t border-t-border/40 bg-black/18";
const MW_REQ_ROW =
  "flex items-center justify-between gap-1.5 py-0.5 text-11";
const MW_REQ_NAME =
  "flex-1 whitespace-nowrap overflow-hidden text-ellipsis min-w-0";
const MW_REQ_COUNTS = "flex items-center gap-0.5 shrink-0 text-11 tabular-nums";
const MW_REQ_ALL_GOOD = "pt-0.75 pb-1 text-11 text-success";
const MW_QTY_HAVE = "text-success";
const MW_QTY_NEED = "text-danger";
const MW_QTY_SEP = "text-muted";
const MW_QTY_REQUIRED = "text-muted";
const MW_SHORTAGE = "shrink-0 rounded-4 bg-danger/12 px-1.25 py-px text-11 font-semibold text-danger";

const MW_FAV_LIST = "shrink-0";
const MW_FAV_ITEM =
  "flex items-center gap-1 pt-0.75 pb-0.75 pr-2 pl-1 border-b border-b-border/35 transition-[background] duration-100 last:border-b-0 hover:bg-white/3";
const MW_FAV_NAME =
  "flex-1 text-11 text-foreground whitespace-nowrap overflow-hidden text-ellipsis min-w-0";
const MW_FAV_QTY =
  "text-12 font-bold text-accent shrink-0 tabular-nums min-w-6 text-right";
const MW_FAV_QTY_CD =
  "text-11 font-bold text-accent shrink-0 tabular-nums min-w-15 text-right";
const MW_FAV_STAR =
  "bg-transparent border-0 cursor-pointer text-ducat text-13 p-0 shrink-0 leading-none transition-colors duration-100 hover:text-muted/60";
const MW_TIMER_STATE =
  "text-10 font-semibold text-muted shrink-0 px-1.25 py-0.25 rounded-3 bg-white/6";
const MW_FISSURE_TIER = "text-11 font-bold shrink-0 w-11.5";

function fmt(n: number) { return n.toLocaleString(); }

function compStatus(comp: RecipeComponent, inventory: Record<string, InventoryItem>): RecipeComponentStatus {
  if ((inventory[comp.unique_name]?.quantity ?? 0) >= (comp.count || 1)) return "part";
  const bpUnique = comp.components[0]?.unique_name;
  if (bpUnique && (inventory[bpUnique]?.quantity ?? 0) > 0) return "blueprint";
  return "none";
}

function mergeComponents(comps: RecipeComponent[]): RecipeComponent[] {
  const seen = new Map<string, RecipeComponent>();
  for (const c of comps) {
    const existing = seen.get(c.unique_name);
    if (existing) {
      seen.set(c.unique_name, { ...existing, count: existing.count + c.count });
    } else {
      seen.set(c.unique_name, { ...c });
    }
  }
  return [...seen.values()];
}

function collectNeeds(
  nodes: RecipeComponent[],
  multiplier: number,
  acc: Map<string, { name: string; needed: number }>,
  inventory: Record<string, InventoryItem>
) {
  for (const node of mergeComponents(nodes)) {
    const resultCount = node.result_count ?? 1;
    const craftsNeeded = Math.ceil((node.count * multiplier) / resultCount);
    const totalNeeded = node.count * multiplier;
    const owned = inventory[node.unique_name]?.quantity ?? 0;
    if (node.components.length === 0 || owned > 0) {
      // Leaf node (raw material), or player already has some of this crafted intermediate —
      // show it directly so the display can compare owned vs needed instead of expanding.
      const prev = acc.get(node.unique_name);
      acc.set(node.unique_name, { name: node.name, needed: (prev?.needed ?? 0) + totalNeeded });
    } else {
      // Player has zero of this intermediate — recurse into its raw ingredients.
      collectNeeds(node.components, craftsNeeded, acc, inventory);
    }
  }
}

interface Props {
  tracked: string[];
  onTrackedChange: (newOrder: string[]) => void;
  onUntrack: (id: string) => void;
  favorites: string[];
  onFavoritesChange: (newOrder: string[]) => void;
  onUnfavorite: (id: string) => void;
  timerFavorites: string[];
  onTimerFavoritesChange: (newOrder: string[]) => void;
  onTimerUnfavorite: (id: string) => void;
  fissureWatches: FissureWatch[];
  inventory: Record<string, InventoryItem>;
  catalog: CatalogItem[];
  width?: number;
  onWidthChange?: (w: number) => void;
  onWidthCommit?: (w: number) => void;
  sectionOrder: string[];
  onSectionOrderChange: (order: string[]) => void;
}

export default function ModularWindow({
  tracked, onTrackedChange, onUntrack,
  favorites, onFavoritesChange, onUnfavorite,
  timerFavorites, onTimerFavoritesChange, onTimerUnfavorite,
  fissureWatches,
  inventory, catalog, width, onWidthChange, onWidthCommit,
  sectionOrder, onSectionOrderChange,
}: Props) {
  const [craftable, setCraftable] = useState<CatalogItem[]>([]);
  const [trackedRecipes, setTrackedRecipes] = useState<Map<string, RecipeComponent[]>>(new Map());
  const [trackingView, setTrackingView] = useState<"need" | "all">("need");
  const [collapsedReqs, setCollapsedReqs] = useState<Set<string>>(new Set());
  const { worldState } = useWorldState();
  const [timerNow, setTimerNow] = useState(Date.now());

  const toggleCollapsedReqs = useCallback((id: string) => {
    setCollapsedReqs(prev => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id); else next.add(id);
      return next;
    });
  }, []);

  useEffect(() => {
    invoke<CatalogItem[]>(TAURI_COMMANDS.GET_CRAFTABLE_ITEMS).then(setCraftable).catch(() => {});
  }, []);

  // Retry if tracked items are present but craftable hasn't loaded (e.g. backend was restarting).
  useEffect(() => {
    if (tracked.length > 0 && craftable.length === 0) {
      invoke<CatalogItem[]>(TAURI_COMMANDS.GET_CRAFTABLE_ITEMS).then(setCraftable).catch(() => {});
    }
  }, [tracked, craftable]);

  // Clean up collapsedReqs when items are removed from tracked, so re-added items start expanded.
  useEffect(() => {
    setCollapsedReqs(prev => {
      const trackedSet = new Set(tracked);
      const stale = [...prev].filter(id => !trackedSet.has(id));
      if (stale.length === 0) return prev;
      const next = new Set(prev);
      for (const id of stale) next.delete(id);
      return next;
    });
  }, [tracked]);

  useEffect(() => {
    const toLoad = tracked.filter(id => !trackedRecipes.has(id));
    setTrackedRecipes(prev => {
      const next = new Map(prev);
      for (const k of next.keys()) if (!tracked.includes(k)) next.delete(k);
      return next;
    });
    if (toLoad.length === 0) return;
    Promise.all(
      toLoad.map(id =>
        invoke<RecipeComponent[]>(TAURI_COMMANDS.GET_RECIPE, { uniqueName: id })
          .then(r => [id, r ?? []] as [string, RecipeComponent[]])
          .catch(() => [id, []] as [string, RecipeComponent[]])
      )
    ).then(results => {
      setTrackedRecipes(prev => {
        const next = new Map(prev);
        for (const [id, r] of results) if (r.length) next.set(id, r);
        return next;
      });
    });
  }, [tracked]); // eslint-disable-line

  useEffect(() => {
    const iv = setInterval(() => setTimerNow(Date.now()), 1000);
    return () => clearInterval(iv);
  }, []);

  const perItemNeeds = useMemo(() => {
    return tracked.map(id => {
      const recipe = trackedRecipes.get(id);
      if (!recipe || recipe.length === 0) return [];
      const acc = new Map<string, { name: string; needed: number }>();
      collectNeeds(recipe, 1, acc, inventory);
      // Remove the tracked item itself if it appears in its own requirements (data quirk)
      acc.delete(id);
      // Deduplicate by display name: recipe data can store the same item under multiple unique_names.
      // Use max(owned) across all matching keys to avoid double-counting.
      const byName = new Map<string, { unique_name: string; name: string; needed: number; allKeys: string[] }>();
      for (const [unique_name, { name, needed }] of acc.entries()) {
        const existing = byName.get(name);
        if (existing) {
          byName.set(name, { ...existing, needed: existing.needed + needed, allKeys: [...existing.allKeys, unique_name] });
        } else {
          byName.set(name, { unique_name, name, needed, allKeys: [unique_name] });
        }
      }
      return Array.from(byName.values())
        .map(({ unique_name, name, needed, allKeys }) => {
          const owned = Math.max(...allKeys.map(k => inventory[k]?.quantity ?? 0));
          return { unique_name, name, needed, owned, shortage: Math.max(0, needed - owned) };
        })
        .sort((a, b) => a.name.localeCompare(b.name));
    });
  }, [tracked, trackedRecipes, inventory]);

  const moveTracked = useCallback((idx: number, dir: -1 | 1) => {
    const next = [...tracked];
    const target = idx + dir;
    if (target < 0 || target >= next.length) return;
    [next[idx], next[target]] = [next[target], next[idx]];
    onTrackedChange(next);
  }, [tracked, onTrackedChange]);

  const moveFavorite = useCallback((idx: number, dir: -1 | 1) => {
    const next = [...favorites];
    const target = idx + dir;
    if (target < 0 || target >= next.length) return;
    [next[idx], next[target]] = [next[target], next[idx]];
    onFavoritesChange(next);
  }, [favorites, onFavoritesChange]);

  const moveTimer = useCallback((idx: number, dir: -1 | 1) => {
    const next = [...timerFavorites];
    const target = idx + dir;
    if (target < 0 || target >= next.length) return;
    [next[idx], next[target]] = [next[target], next[idx]];
    onTimerFavoritesChange(next);
  }, [timerFavorites, onTimerFavoritesChange]);

  const moveSectionUp = useCallback((idx: number) => {
    if (idx === 0) return;
    const next = [...sectionOrder];
    [next[idx - 1], next[idx]] = [next[idx], next[idx - 1]];
    onSectionOrderChange(next);
  }, [sectionOrder, onSectionOrderChange]);

  const moveSectionDown = useCallback((idx: number) => {
    if (idx === sectionOrder.length - 1) return;
    const next = [...sectionOrder];
    [next[idx], next[idx + 1]] = [next[idx + 1], next[idx]];
    onSectionOrderChange(next);
  }, [sectionOrder, onSectionOrderChange]);

  // ── Section bodies ───────────────────────────────────────────────────────

  const trackingBody = (
    tracked.length === 0 ? (
      <div className={MW_EMPTY}>Star ☆ items in Foundry to track them.</div>
    ) : (
      <div className={MW_TRACKED_LIST}>
        {tracked.map((id, idx) => {
          const item = craftable.find(c => c.unique_name === id);
          if (!item) return null;
          const recipe = trackedRecipes.get(id);
          const isOwned = (inventory[item.unique_name]?.quantity ?? 0) > 0;
          const allDone = recipe && recipe.length > 0 &&
            mergeComponents(recipe).every(c => compStatus(c, inventory) === "part");
          const needs = perItemNeeds[idx] ?? [];
          const collapsed = collapsedReqs.has(id);
          const rows = needs.filter(r => trackingView === "all" || r.shortage > 0);
          const allCovered = needs.length > 0 && needs.every(r => r.shortage === 0);
          const hasNeeds = needs.length > 0;

          return (
            <div key={id} className={MW_GROUP}>
              <div className={MW_TRACKED_ROW}>
                <div className={MW_ITEM_ARROWS}>
                  <button className={MW_ARROW_BTN_ITEM} disabled={idx === 0} onClick={() => moveTracked(idx, -1)} title="Move up">
                    <svg viewBox="0 0 10 6" fill="none" className={MW_ARROW_SVG_ITEM}><path d="M1 5L5 1L9 5" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round"/></svg>
                  </button>
                  <button className={MW_ARROW_BTN_ITEM} disabled={idx === tracked.length - 1} onClick={() => moveTracked(idx, 1)} title="Move down">
                    <svg viewBox="0 0 10 6" fill="none" className={MW_ARROW_SVG_ITEM}><path d="M1 1L5 5L9 1" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round"/></svg>
                  </button>
                </div>
                <div
                  className={hasNeeds ? MW_NAME_AREA_REQS : MW_NAME_AREA}
                  onClick={() => hasNeeds && toggleCollapsedReqs(id)}
                >
                  {hasNeeds && (
                    <svg viewBox="0 0 10 6" fill="none" className={collapsed ? MW_CHEVRON_COLLAPSED : MW_CHEVRON}>
                      <path d="M1 1L5 5L9 1" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round"/>
                    </svg>
                  )}
                  <span className={MW_ITEM_NAME +
                    (isOwned ? " text-ducat" : allDone ? " text-success" : "") +
                    (hasNeeds ? " group-hover:text-foreground" : "")}>{item.name}</span>
                </div>
                <span className={MW_ITEM_STATUS}>
                  {isOwned ? "✓" : allDone ? "⚡" : allCovered ? <span className="text-green">✓</span> : ""}
                </span>
                <button className={MW_REMOVE_BTN} onClick={() => onUntrack(id)}>×</button>
              </div>

              {hasNeeds && !collapsed && (
                <div className={MW_INLINE_REQS}>
                  {rows.length === 0 ? (
                    <div className={MW_REQ_ALL_GOOD}>✓ All resources covered</div>
                  ) : (
                    rows.map(r => (
                      <div key={`${id}-${r.unique_name}`} className={MW_REQ_ROW}>
                        <span className={MW_REQ_NAME + (r.shortage > 0 ? " text-foreground" : " text-muted")}>{r.name}</span>
                        <span className={MW_REQ_COUNTS}>
                          <span className={r.shortage === 0 ? MW_QTY_HAVE : MW_QTY_NEED}>{fmt(r.owned)}</span>
                          <span className={MW_QTY_SEP}>/</span>
                          <span className={MW_QTY_REQUIRED}>{fmt(r.needed)}</span>
                          {r.shortage > 0 && <span className={MW_SHORTAGE}>−{fmt(r.shortage)}</span>}
                        </span>
                      </div>
                    ))
                  )}
                </div>
              )}
            </div>
          );
        })}
      </div>
    )
  );

  const favoritesBody = (
    <>
      {favorites.length === 0 ? (
        <div className={MW_EMPTY}>Star ☆ items in Inventory to favorite them.</div>
      ) : (
        <div className={MW_FAV_LIST}>
          {favorites.map((id, idx) => {
            const item = catalog.find(c => c.unique_name === id);
            if (!item) return null;
            const qty = inventory[id]?.quantity ?? 0;
            return (
              <div key={id} className={MW_FAV_ITEM}>
                <div className={MW_ITEM_ARROWS}>
                  <button className={MW_ARROW_BTN_ITEM} disabled={idx === 0} onClick={() => moveFavorite(idx, -1)} title="Move up">
                    <svg viewBox="0 0 10 6" fill="none" className={MW_ARROW_SVG_ITEM}><path d="M1 5L5 1L9 5" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round"/></svg>
                  </button>
                  <button className={MW_ARROW_BTN_ITEM} disabled={idx === favorites.length - 1} onClick={() => moveFavorite(idx, 1)} title="Move down">
                    <svg viewBox="0 0 10 6" fill="none" className={MW_ARROW_SVG_ITEM}><path d="M1 1L5 5L9 1" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round"/></svg>
                  </button>
                </div>
                <span className={MW_FAV_NAME}>{item.name}</span>
                <span className={MW_FAV_QTY}>{fmt(qty)}</span>
                <button className={MW_FAV_STAR} title="Remove from favorites" onClick={() => onUnfavorite(id)}>★</button>
              </div>
            );
          })}
        </div>
      )}
    </>
  );

  const trackingToggle = (
    <div className={TRACKING_TOGGLE}>
      <button className={`${TRACKING_TOGGLE_BTN}${trackingView === "need" ? ` ${TRACKING_TOGGLE_ON}` : ""}`} onClick={e => { e.stopPropagation(); setTrackingView("need"); }}>Missing</button>
      <button className={`${TRACKING_TOGGLE_BTN}${trackingView === "all" ? ` ${TRACKING_TOGGLE_ON}` : ""}`} onClick={e => { e.stopPropagation(); setTrackingView("all"); }}>All</button>
    </div>
  );

  const timersBody = (
    timerFavorites.length === 0 ? (
      <div className={MW_EMPTY}>Pin ☆ timers in the Timers tab to show them here.</div>
    ) : (
      <div className={MW_FAV_LIST}>
        {timerFavorites.map((id, idx) => {
          const info = worldState ? getTimerInfo(id, worldState) : null;
          const label = TIMER_LABELS[id] ?? id;
          const remaining = info ? fmtMs(new Date(info.expiry).getTime() - timerNow) : "—";
          return (
            <div key={id} className={MW_FAV_ITEM}>
              <div className={MW_ITEM_ARROWS}>
                <button className={MW_ARROW_BTN_ITEM} disabled={idx === 0} onClick={() => moveTimer(idx, -1)} title="Move up">
                  <svg viewBox="0 0 10 6" fill="none" className={MW_ARROW_SVG_ITEM}><path d="M1 5L5 1L9 5" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round"/></svg>
                </button>
                <button className={MW_ARROW_BTN_ITEM} disabled={idx === timerFavorites.length - 1} onClick={() => moveTimer(idx, 1)} title="Move down">
                  <svg viewBox="0 0 10 6" fill="none" className={MW_ARROW_SVG_ITEM}><path d="M1 1L5 5L9 1" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round"/></svg>
                </button>
              </div>
              <span className={MW_FAV_NAME}>{label}</span>
              {info && <span className={MW_TIMER_STATE}>{info.state}</span>}
              <span className={MW_FAV_QTY_CD}>{remaining}</span>
              <button className={MW_FAV_STAR} title="Remove" onClick={() => onTimerUnfavorite(id)}>★</button>
            </div>
          );
        })}
      </div>
    )
  );

  const sectionData: Record<string, { label: string; body: React.ReactElement; headerExtra?: React.ReactElement }> = {
    tracking: {
      label: `Tracking${tracked.length > 0 ? ` (${tracked.length})` : ""}`,
      body: trackingBody,
      headerExtra: tracked.length > 0 ? trackingToggle : undefined,
    },
    favorites: {
      label: `Favorites${favorites.length > 0 ? ` (${favorites.length})` : ""}`,
      body: favoritesBody,
    },
    timers: {
      label: `Timers${timerFavorites.length > 0 ? ` (${timerFavorites.length})` : ""}`,
      body: timersBody,
    },
    fissures: {
      label: "Watched Fissures",
      body: (() => {
        if (fissureWatches.length === 0) {
          return <div className={MW_EMPTY}>Add fissure watches in the Timers tab.</div>;
        }
        const TIER_COLOR: Record<string, string> = {
          Lith: "#c8853a", Meso: "#a8a9ad", Neo: "#f0c040",
          Axi: "#e5c04a", Requiem: "#9b6dff", Omnia: "#e0e0e0",
        };
        // Match each source array with explicit variant so checks are unambiguous
        const matched: MatchedFissure[] = [
          ...(worldState?.fissures   ?? []).filter(f => fissureWatches.some(w => matchesWatch(w, f, "normal"))).map(f => ({ f, variant: "normal" as const })),
          ...(worldState?.spFissures ?? []).filter(f => fissureWatches.some(w => matchesWatch(w, f, "hard"))).map(f => ({ f, variant: "hard" as const })),
          ...(worldState?.voidStorms ?? []).filter(s => fissureWatches.some(w => matchesWatch(w, s, "storm"))).map(s => ({ f: s, variant: "storm" as const })),
        ].sort((a, b) => a.f.tierNum - b.f.tierNum);

        if (matched.length === 0) {
          return <div className={MW_EMPTY}>No matching fissures active.</div>;
        }
        const variantLabel: Record<string, string> = { normal: "Normal", hard: "Steel Path", storm: "Storm" };
        return (
          <div className={MW_FAV_LIST}>
            {matched.map(({ f, variant }, i) => {
              const ms = new Date(f.expiry).getTime() - timerNow;
              return (
                <div key={i} className={`${MW_FAV_ITEM} flex-col items-stretch px-2 py-1`}>
                  <div className="flex items-center gap-1">
                    <span className={MW_FISSURE_TIER} style={{ color: TIER_COLOR[f.tier] ?? "#ccc" }}>{f.tier}</span>
                    <span className={MW_FAV_NAME}>{f.missionType}</span>
                    <span className="shrink-0 text-10 text-muted">{variantLabel[variant]}</span>
                    <span className={`${MW_FAV_QTY_CD} ml-auto`}>{fmtMs(ms)}</span>
                  </div>
                  <div className="mt-px pl-0.5 text-10 text-muted">
                    {f.enemy && <span className="mr-1.5">{f.enemy}</span>}
                    {f.node && <span>{f.node}</span>}
                  </div>
                </div>
              );
            })}
          </div>
        );
      })(),
    },
  };

  return (
    <div
      className={`${MW_WINDOW}${width !== undefined ? ` ${MW_WINDOW_DOCKED}` : " flex-1"}`}
      style={width !== undefined ? { width } : undefined}
    >
      {onWidthChange && (
        <ResizeHandle className={MW_RESIZE} value={width ?? 240} axis="x" direction={-1} clamp={value => Math.max(160, Math.min(500, value))} onValueChange={onWidthChange} onValueCommit={onWidthCommit} />
      )}

      <div className={MW_INNER}>
        <div className={MW_HEADER}>
          <span className={MW_TITLE}>Modular Window</span>
        </div>

        {sectionOrder.map((id, idx) => {
          const sec = sectionData[id];
          if (!sec) return null;
          return (
            <div key={id} className={MW_SECTION_WRAP}>
              {idx > 0 && <div className={MW_DIVIDER} />}
              <div className={MW_SECTION_HEADER}>
                <span className={MW_SECTION_LABEL}>{sec.label}</span>
                {sec.headerExtra}
                <div className={MW_SECTION_ARROWS}>
                  <button
                    className={MW_ARROW_BTN}
                    disabled={idx === 0}
                    onClick={e => { e.stopPropagation(); moveSectionUp(idx); }}
                    title="Move up"
                  >
                    <svg viewBox="0 0 10 6" fill="none" className={MW_ARROW_SVG}>
                      <path d="M1 5L5 1L9 5" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round"/>
                    </svg>
                  </button>
                  <button
                    className={MW_ARROW_BTN}
                    disabled={idx === sectionOrder.length - 1}
                    onClick={e => { e.stopPropagation(); moveSectionDown(idx); }}
                    title="Move down"
                  >
                    <svg viewBox="0 0 10 6" fill="none" className={MW_ARROW_SVG}>
                      <path d="M1 1L5 5L9 1" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round"/>
                    </svg>
                  </button>
                </div>
              </div>
              {sec.body}
            </div>
          );
        })}
      </div>
    </div>
  );
}
