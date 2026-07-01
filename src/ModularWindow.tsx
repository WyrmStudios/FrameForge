import React, { useState, useEffect, useMemo, useRef, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import "./ModularWindow.css";
import { WorldState, TIMER_LABELS, getTimerInfo, fmtMs, FissureWatch, matchesWatch, WsFissure, WsStorm } from "./TimerHelper";
import type { InventoryItem } from "./App";

interface CatalogItem {
  unique_name: string;
  name: string;
  category: string;
  image_name?: string;
}

interface RecipeComponent {
  unique_name: string;
  name: string;
  count: number;
  result_count: number;
  components: RecipeComponent[];
}

type CompStatus = "none" | "blueprint" | "part";

function fmt(n: number) { return n.toLocaleString(); }

function compStatus(comp: RecipeComponent, inventory: Record<string, InventoryItem>): CompStatus {
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
  acc: Map<string, { name: string; needed: number }>
) {
  for (const node of mergeComponents(nodes)) {
    const resultCount = node.result_count ?? 1;
    const craftsNeeded = Math.ceil((node.count * multiplier) / resultCount);
    if (node.components.length === 0) {
      // Leaf node: a raw material or blueprint you physically acquire
      const prev = acc.get(node.unique_name);
      acc.set(node.unique_name, { name: node.name, needed: (prev?.needed ?? 0) + node.count * multiplier });
    } else {
      // Intermediate crafted part: recurse into its ingredients only
      collectNeeds(node.components, craftsNeeded, acc);
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
  sectionOrder: string[];
  onSectionOrderChange: (order: string[]) => void;
}

export default function ModularWindow({
  tracked, onTrackedChange, onUntrack,
  favorites, onFavoritesChange, onUnfavorite,
  timerFavorites, onTimerFavoritesChange, onTimerUnfavorite,
  fissureWatches,
  inventory, catalog, width, onWidthChange,
  sectionOrder, onSectionOrderChange,
}: Props) {
  const [craftable, setCraftable] = useState<CatalogItem[]>([]);
  const [trackedRecipes, setTrackedRecipes] = useState<Map<string, RecipeComponent[]>>(new Map());
  const [trackingView, setTrackingView] = useState<"need" | "all">("need");
  const [collapsedReqs, setCollapsedReqs] = useState<Set<string>>(new Set());
  const [worldState, setWorldState] = useState<WorldState | null>(null);
  const [timerNow, setTimerNow] = useState(Date.now());

  const toggleCollapsedReqs = useCallback((id: string) => {
    setCollapsedReqs(prev => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id); else next.add(id);
      return next;
    });
  }, []);

  // resize state
  const isResizingRef = useRef(false);
  const resizeStartXRef = useRef(0);
  const resizeStartWRef = useRef(0);

  useEffect(() => {
    invoke<CatalogItem[]>("get_craftable_items").then(setCraftable).catch(() => {});
  }, []);

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
        invoke<RecipeComponent[]>("get_recipe", { uniqueName: id })
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
    if (!sectionOrder.includes("timers")) return;
    const fetch_ = () => invoke<WorldState>("fetch_worldstate").then(setWorldState).catch(() => {});
    fetch_();
    const iv = setInterval(fetch_, 60000);
    return () => clearInterval(iv);
  }, [sectionOrder]);

  useEffect(() => {
    const iv = setInterval(() => setTimerNow(Date.now()), 1000);
    return () => clearInterval(iv);
  }, []);

  const perItemNeeds = useMemo(() => {
    return tracked.map(id => {
      const recipe = trackedRecipes.get(id);
      if (!recipe || recipe.length === 0) return [];
      const acc = new Map<string, { name: string; needed: number }>();
      collectNeeds(recipe, 1, acc);
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

  const handleResizeMouseDown = useCallback((e: React.MouseEvent) => {
    if (!onWidthChange) return;
    e.preventDefault();
    isResizingRef.current = true;
    resizeStartXRef.current = e.clientX;
    resizeStartWRef.current = width ?? 240;
    const onMove = (me: MouseEvent) => {
      if (!isResizingRef.current) return;
      const dx = resizeStartXRef.current - me.clientX;
      onWidthChange(Math.max(160, Math.min(500, resizeStartWRef.current + dx)));
    };
    const onUp = () => {
      isResizingRef.current = false;
      document.removeEventListener("mousemove", onMove);
      document.removeEventListener("mouseup", onUp);
    };
    document.addEventListener("mousemove", onMove);
    document.addEventListener("mouseup", onUp);
  }, [width, onWidthChange]);

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
      <div className="modular-empty">Star ☆ items in Foundry to track them.</div>
    ) : (
      <div className="modular-tracked-list">
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
            <div key={id} className={`modular-tracked-group${isOwned ? " tracking-owned" : allDone ? " tracking-ready" : ""}`}>
              <div className="modular-tracked-row">
                <div className="modular-item-arrows">
                  <button className="modular-arrow-btn" disabled={idx === 0} onClick={() => moveTracked(idx, -1)} title="Move up">
                    <svg viewBox="0 0 10 6" fill="none"><path d="M1 5L5 1L9 5" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round"/></svg>
                  </button>
                  <button className="modular-arrow-btn" disabled={idx === tracked.length - 1} onClick={() => moveTracked(idx, 1)} title="Move down">
                    <svg viewBox="0 0 10 6" fill="none"><path d="M1 1L5 5L9 1" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round"/></svg>
                  </button>
                </div>
                <div
                  className={`modular-tracked-name-area${hasNeeds ? " has-reqs" : ""}`}
                  onClick={() => hasNeeds && toggleCollapsedReqs(id)}
                >
                  {hasNeeds && (
                    <svg viewBox="0 0 10 6" fill="none" className={`modular-tracked-chevron${collapsed ? " collapsed" : ""}`}>
                      <path d="M1 1L5 5L9 1" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round"/>
                    </svg>
                  )}
                  <span className="modular-item-name">{item.name}</span>
                </div>
                <span className="modular-item-status">
                  {isOwned ? "✓" : allDone ? "⚡" : allCovered ? <span style={{ color: "var(--green)" }}>✓</span> : ""}
                </span>
                <button className="modular-remove-btn" onClick={() => onUntrack(id)}>×</button>
              </div>

              {hasNeeds && !collapsed && (
                <div className="modular-inline-reqs">
                  {rows.length === 0 ? (
                    <div className="modular-req-all-good">✓ All resources covered</div>
                  ) : (
                    rows.map(r => (
                      <div key={`${id}-${r.unique_name}`} className={`modular-req-row${r.shortage > 0 ? " req-missing" : " req-ok"}`}>
                        <span className="modular-req-name">{r.name}</span>
                        <span className="modular-req-counts">
                          <span className={r.shortage === 0 ? "qty-have" : "qty-need"}>{fmt(r.owned)}</span>
                          <span className="qty-sep">/</span>
                          <span className="qty-required">{fmt(r.needed)}</span>
                          {r.shortage > 0 && <span className="recipe-shortage">−{fmt(r.shortage)}</span>}
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
        <div className="modular-empty">Star ☆ items in Inventory to favorite them.</div>
      ) : (
        <div className="modular-fav-list">
          {favorites.map((id, idx) => {
            const item = catalog.find(c => c.unique_name === id);
            if (!item) return null;
            const qty = inventory[id]?.quantity ?? 0;
            return (
              <div key={id} className="modular-fav-item">
                <div className="modular-item-arrows">
                  <button className="modular-arrow-btn" disabled={idx === 0} onClick={() => moveFavorite(idx, -1)} title="Move up">
                    <svg viewBox="0 0 10 6" fill="none"><path d="M1 5L5 1L9 5" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round"/></svg>
                  </button>
                  <button className="modular-arrow-btn" disabled={idx === favorites.length - 1} onClick={() => moveFavorite(idx, 1)} title="Move down">
                    <svg viewBox="0 0 10 6" fill="none"><path d="M1 1L5 5L9 1" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round"/></svg>
                  </button>
                </div>
                <span className="modular-fav-name">{item.name}</span>
                <span className="modular-fav-qty">{fmt(qty)}</span>
                <button className="modular-fav-star" title="Remove from favorites" onClick={() => onUnfavorite(id)}>★</button>
              </div>
            );
          })}
        </div>
      )}
    </>
  );

  const trackingToggle = (
    <div className="tracking-toggle">
      <button className={`tracking-toggle-btn${trackingView === "need" ? " active" : ""}`} onClick={e => { e.stopPropagation(); setTrackingView("need"); }}>Missing</button>
      <button className={`tracking-toggle-btn${trackingView === "all" ? " active" : ""}`} onClick={e => { e.stopPropagation(); setTrackingView("all"); }}>All</button>
    </div>
  );

  const timersBody = (
    timerFavorites.length === 0 ? (
      <div className="modular-empty">Pin ☆ timers in the Timers tab to show them here.</div>
    ) : (
      <div className="modular-fav-list">
        {timerFavorites.map((id, idx) => {
          const info = worldState ? getTimerInfo(id, worldState) : null;
          const label = TIMER_LABELS[id] ?? id;
          const remaining = info ? fmtMs(new Date(info.expiry).getTime() - timerNow) : "—";
          return (
            <div key={id} className="modular-fav-item">
              <div className="modular-item-arrows">
                <button className="modular-arrow-btn" disabled={idx === 0} onClick={() => moveTimer(idx, -1)} title="Move up">
                  <svg viewBox="0 0 10 6" fill="none"><path d="M1 5L5 1L9 5" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round"/></svg>
                </button>
                <button className="modular-arrow-btn" disabled={idx === timerFavorites.length - 1} onClick={() => moveTimer(idx, 1)} title="Move down">
                  <svg viewBox="0 0 10 6" fill="none"><path d="M1 1L5 5L9 1" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round"/></svg>
                </button>
              </div>
              <span className="modular-fav-name">{label}</span>
              {info && <span className="modular-timer-state">{info.state}</span>}
              <span className="modular-fav-qty modular-timer-cd">{remaining}</span>
              <button className="modular-fav-star" title="Remove" onClick={() => onTimerUnfavorite(id)}>★</button>
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
          return <div className="modular-empty">Add fissure watches in the Timers tab.</div>;
        }
        const TIER_COLOR: Record<string, string> = {
          Lith: "#c8853a", Meso: "#a8a9ad", Neo: "#f0c040",
          Axi: "#e5c04a", Requiem: "#9b6dff", Omnia: "#e0e0e0",
        };
        // Match each source array with explicit variant so checks are unambiguous
        type MatchedFissure = { f: WsFissure | WsStorm; variant: "normal" | "hard" | "storm" };
        const matched: MatchedFissure[] = [
          ...(worldState?.fissures   ?? []).filter(f => fissureWatches.some(w => matchesWatch(w, f, "normal"))).map(f => ({ f, variant: "normal" as const })),
          ...(worldState?.spFissures ?? []).filter(f => fissureWatches.some(w => matchesWatch(w, f, "hard"))).map(f => ({ f, variant: "hard" as const })),
          ...(worldState?.voidStorms ?? []).filter(s => fissureWatches.some(w => matchesWatch(w, s, "storm"))).map(s => ({ f: s, variant: "storm" as const })),
        ].sort((a, b) => a.f.tierNum - b.f.tierNum);

        if (matched.length === 0) {
          return <div className="modular-empty">No matching fissures active.</div>;
        }
        const variantLabel: Record<string, string> = { normal: "Normal", hard: "Steel Path", storm: "Storm" };
        return (
          <div className="modular-fav-list">
            {matched.map(({ f, variant }, i) => {
              const ms = new Date(f.expiry).getTime() - timerNow;
              return (
                <div key={i} className="modular-fav-item" style={{ flexDirection: "column", alignItems: "stretch", padding: "4px 8px" }}>
                  <div style={{ display: "flex", alignItems: "center", gap: 4 }}>
                    <span className="modular-fissure-tier" style={{ color: TIER_COLOR[f.tier] ?? "#ccc" }}>{f.tier}</span>
                    <span className="modular-fav-name">{f.missionType}</span>
                    <span style={{ fontSize: 10, color: "var(--muted)", flexShrink: 0 }}>{variantLabel[variant]}</span>
                    <span className="modular-fav-qty modular-timer-cd" style={{ marginLeft: "auto" }}>{fmtMs(ms)}</span>
                  </div>
                  <div style={{ fontSize: 10, color: "var(--muted)", paddingLeft: 2, marginTop: 1 }}>
                    {f.enemy && <span style={{ marginRight: 6 }}>{f.enemy}</span>}
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
      className="modular-window"
      style={width !== undefined ? { width } : { flex: 1 }}
    >
      {onWidthChange && (
        <div className="modular-resize-handle" onMouseDown={handleResizeMouseDown} />
      )}

      <div className="modular-inner">
        <div className="modular-header">
          <span className="modular-title">Modular Window</span>
        </div>

        {sectionOrder.map((id, idx) => {
          const sec = sectionData[id];
          if (!sec) return null;
          return (
            <div key={id} className="modular-section-wrap">
              {idx > 0 && <div className="modular-divider" />}
              <div className="modular-section-header">
                <span className="modular-section-label">{sec.label}</span>
                {sec.headerExtra}
                <div className="modular-section-arrows">
                  <button
                    className="modular-arrow-btn"
                    disabled={idx === 0}
                    onClick={e => { e.stopPropagation(); moveSectionUp(idx); }}
                    title="Move up"
                  >
                    <svg viewBox="0 0 10 6" fill="none">
                      <path d="M1 5L5 1L9 5" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round"/>
                    </svg>
                  </button>
                  <button
                    className="modular-arrow-btn"
                    disabled={idx === sectionOrder.length - 1}
                    onClick={e => { e.stopPropagation(); moveSectionDown(idx); }}
                    title="Move down"
                  >
                    <svg viewBox="0 0 10 6" fill="none">
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
