import React, { useState, useEffect, useMemo, useRef, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import "./ModularWindow.css";

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

function compStatus(comp: RecipeComponent, quantities: Record<string, number>): CompStatus {
  if ((quantities[comp.unique_name] ?? 0) >= (comp.count || 1)) return "part";
  const bpUnique = comp.components[0]?.unique_name;
  if (bpUnique && (quantities[bpUnique] ?? 0) > 0) return "blueprint";
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
  quantities: Record<string, number>;
  catalog: CatalogItem[];
  width?: number;
  onWidthChange?: (w: number) => void;
  sectionOrder: string[];
  onSectionOrderChange: (order: string[]) => void;
}

export default function ModularWindow({
  tracked, onTrackedChange, onUntrack,
  favorites, onFavoritesChange, onUnfavorite,
  quantities, catalog, width, onWidthChange,
  sectionOrder, onSectionOrderChange,
}: Props) {
  const [craftable, setCraftable] = useState<CatalogItem[]>([]);
  const [trackedRecipes, setTrackedRecipes] = useState<Map<string, RecipeComponent[]>>(new Map());
  const [trackingView, setTrackingView] = useState<"need" | "all">("need");
  const [collapsedReqs, setCollapsedReqs] = useState<Set<string>>(new Set());

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
          const owned = Math.max(...allKeys.map(k => quantities[k] ?? 0));
          return { unique_name, name, needed, owned, shortage: Math.max(0, needed - owned) };
        })
        .sort((a, b) => a.name.localeCompare(b.name));
    });
  }, [tracked, trackedRecipes, quantities]);

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
          const isOwned = (quantities[item.unique_name] ?? 0) > 0;
          const allDone = recipe && recipe.length > 0 &&
            mergeComponents(recipe).every(c => compStatus(c, quantities) === "part");
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
            const qty = quantities[id] ?? 0;
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
