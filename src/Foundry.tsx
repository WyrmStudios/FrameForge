import { useState, useEffect, useMemo, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { HelpTip } from "./HelpTip";

// ─── Types ────────────────────────────────────────────────────────────────────

interface CatalogItem {
  unique_name: string;
  name: string;
  category: string;
  image_name?: string;
  vaulted?: boolean | null;
  mastery_req?: number | null;
}

interface RecipeComponent {
  unique_name: string;
  name: string;
  count: number;
  result_count: number;
  components: RecipeComponent[];
}

interface CraftingJob {
  unique_name: string;
  item_name: string;
  completion_ms: number;
}

interface ArchonShard { type: string; tauforged: boolean; color: string; boost?: string; }

interface Props {
  quantities: Record<string, number>;
  masteryData: Record<string, number>;
  refreshKey: number;
  crafting: CraftingJob[];
  colorblindMode?: boolean;
  subsummedWarframes?: Set<string>;
  archonShards?: Record<string, ArchonShard[]>;
  tracked: string[];
  onTrackToggle: (id: string) => void;
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

function fmt(n: number) { return n.toLocaleString(); }

function collectNeeds(
  nodes: RecipeComponent[],
  multiplier: number,
  acc: Map<string, { name: string; needed: number }>
) {
  for (const node of mergeComponents(nodes)) {
    const resultCount = node.result_count ?? 1;
    const craftsNeeded = Math.ceil((node.count * multiplier) / resultCount);
    if (node.components.length === 0) {
      const prev = acc.get(node.unique_name);
      acc.set(node.unique_name, { name: node.name, needed: (prev?.needed ?? 0) + node.count * multiplier });
    } else {
      collectNeeds(node.components, craftsNeeded, acc);
      const prev = acc.get(node.unique_name);
      acc.set(node.unique_name, { name: node.name, needed: (prev?.needed ?? 0) + node.count * multiplier });
    }
  }
}

type CompStatus = "none" | "blueprint" | "part";

function compStatus(comp: RecipeComponent, quantities: Record<string, number>): CompStatus {
  // Check against the required count, not just > 0 (e.g. 2x Bolto: need qty >= 2)
  if ((quantities[comp.unique_name] ?? 0) >= (comp.count || 1)) return "part";
  const bpUnique = comp.components[0]?.unique_name;
  if (bpUnique && (quantities[bpUnique] ?? 0) > 0) return "blueprint";
  return "none";
}

/** Merge recipe components that share the same unique_name, summing their counts.
 *  Some recipes (e.g. Akbolto needing 2x Bolto) list the same item twice. */
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

function isLichWeapon(item: CatalogItem): boolean {
  return item.name.startsWith("Kuva ") || item.name.startsWith("Tenet ");
}

// ─── Relic helpers ────────────────────────────────────────────────────────────

function ArchonCrystalIcon({ shards }: { shards: ArchonShard[] }) {
  if (!shards || shards.length === 0) return null;
  try {
    const lines = shards.map(s =>
      `${s.tauforged ? "✦ " : ""}${s.type}${s.boost ? ` — ${s.boost}` : ""}`
    ).join("\n");
    const dominant = shards[0]?.color ?? "#88ccff";
    return (
      <span
        className="craft-icon-tag craft-icon-archon"
        title={`Archon Shards:\n${lines}`}
        style={{ borderColor: dominant, background: dominant + "22" }}
      >
        {/* Hexagon gem matching the 18×18 tag size */}
        <svg width="16" height="16" viewBox="0 0 16 16" fill="none" style={{ display: "block" }}>
          <polygon points="8,1 14,4.5 14,11.5 8,15 2,11.5 2,4.5"
            fill={dominant + "44"} stroke={dominant} strokeWidth="1.2"/>
          <polygon points="8,4.5 11,6.5 11,10.5 8,12.5 5,10.5 5,6.5"
            fill={dominant + "66"}/>
          {shards.length > 1 && (
            <text x="8" y="9" textAnchor="middle" dominantBaseline="middle"
              fill="#fff" fontSize="5" fontWeight="bold">{shards.length}</text>
          )}
        </svg>
      </span>
    );
  } catch {
    return null;
  }
}

function HelminthIcon() {
  return (
    <svg width="13" height="13" viewBox="0 0 13 13" fill="none" xmlns="http://www.w3.org/2000/svg">
      <circle cx="6.5" cy="6.5" r="5.5" fill="#1a3d1a" stroke="#5fbf4f" strokeWidth="1"/>
      <ellipse cx="6.5" cy="8" rx="3" ry="1.5" fill="#5fbf4f" opacity="0.8"/>
      <circle cx="4.5" cy="5.5" r="1" fill="#5fbf4f"/>
      <circle cx="8.5" cy="5.5" r="1" fill="#5fbf4f"/>
      <path d="M4 8 L4.5 9.2 M5 8 L5 9.2 M6 7.8 L6 9.2 M7 7.8 L7 9.2 M8 8 L7.5 9.2 M9 8 L8.5 9.2" stroke="#1a3d1a" strokeWidth="0.5"/>
    </svg>
  );
}

function RelicIcon() {
  return (
    <svg viewBox="0 0 20 26" width="11" height="14" fill="none" xmlns="http://www.w3.org/2000/svg" className="relic-icon">
      <ellipse cx="10" cy="13" rx="8.5" ry="11.5" fill="rgba(255,220,100,.15)" stroke="rgba(255,220,100,.7)" strokeWidth="1.2"/>
      <path d="M10 4 C7 7 6 10 8 13 C10 16 9 19 10 22" stroke="rgba(255,220,100,.9)" strokeWidth="1.3" strokeLinecap="round" fill="none"/>
      <path d="M10 4 C13 7 14 10 12 13 C10 16 11 19 10 22" stroke="rgba(255,220,100,.6)" strokeWidth="0.9" strokeLinecap="round" fill="none"/>
    </svg>
  );
}

const RELIC_SUFFIXES = ["Bronze", "Silver", "Gold", "Platinum"];
function ownsRelicVariant(relicUnique: string, quantities: Record<string, number>): boolean {
  const base = relicUnique.replace(/(Bronze|Silver|Gold|Platinum)$/, "");
  return RELIC_SUFFIXES.some(s => (quantities[`${base}${s}`] ?? 0) > 0);
}

// ─── Item image ───────────────────────────────────────────────────────────────

function ItemImg({ imageName, category, size = 40 }: { imageName?: string; category: string; size?: number }) {
  const [failed, setFailed] = useState(false);
  const style = { width: size, height: size, flexShrink: 0 };
  if (!imageName || failed)
    return <span className="item-img-fallback" style={{ ...style, fontSize: size * 0.35 }}>{category[0].toUpperCase()}</span>;
  return (
    <img className="item-img" style={style} src={`https://cdn.warframestat.us/img/${imageName}`}
      alt="" loading="lazy" onError={() => setFailed(true)} />
  );
}

// ─── Comp row (used inside modal tree) ───────────────────────────────────────

function CompRow({ comp, quantities, relicDrops, relicNames }: {
  comp: RecipeComponent; quantities: Record<string, number>;
  relicDrops: Record<string, string[]>; relicNames: Record<string, string>;
}) {
  const status = compStatus(comp, quantities);
  const ownedRelics = [...new Set(
    (relicDrops[comp.unique_name] ?? [])
      .filter(r => ownsRelicVariant(r, quantities))
      .map(r => {
        const base = r.replace(/(Bronze|Silver|Gold|Platinum)$/, "");
        const owned = RELIC_SUFFIXES.find(s => (quantities[`${base}${s}`] ?? 0) > 0);
        const key = owned ? `${base}${owned}` : r;
        return relicNames[key] ?? relicNames[r] ?? r.split("/").pop() ?? r;
      })
  )];
  return (
    <div className={`comp-row comp-row-${status}`}>
      {ownedRelics.length > 0 && (
        <span className="relic-icon-wrap" title={ownedRelics.join("\n")}><RelicIcon /></span>
      )}
      <span className="comp-row-name">{comp.name}</span>
      {status === "part"      && <span className="comp-row-badge">✓</span>}
      {status === "blueprint" && <span className="comp-row-badge">BP</span>}
    </div>
  );
}

// ─── Tree node (modal recipe tree) ───────────────────────────────────────────

function TreeNode({ node, quantities, depth }: {
  node: RecipeComponent; quantities: Record<string, number>; depth: number;
}) {
  const owned = quantities[node.unique_name] ?? 0;
  const enough = owned >= node.count;
  const hasChildren = node.components.length > 0;
  // Don't auto-expand satisfied nodes — hides unnecessary sub-trees (e.g. Control Module Blueprint when you have 443)
  const [open, setOpen] = useState(!enough && depth < 3);
  return (
    <div style={{ marginLeft: depth * 16 }}>
      <div
        className={`recipe-row ${enough ? "recipe-ok" : "recipe-missing"}`}
        onClick={() => hasChildren && setOpen(o => !o)}
        style={{ cursor: hasChildren ? "pointer" : "default" }}
      >
        {hasChildren
          ? <span className="recipe-chevron">{open ? "▾" : "▸"}</span>
          : <span className="recipe-chevron recipe-chevron-leaf">·</span>}
        <span className="recipe-name">{node.name}</span>
        <span className="recipe-counts">
          <span className={enough ? "qty-have" : "qty-need"}>{fmt(owned)}</span>
          <span className="qty-sep">/</span>
          <span className="qty-required">{fmt(node.count)}</span>
        </span>
        {!enough && <span className="recipe-shortage">−{fmt(node.count - owned)}</span>}
      </div>
      {hasChildren && open && mergeComponents(node.components).map((child, i) => (
        <TreeNode key={i} node={child} quantities={quantities} depth={depth + 1} />
      ))}
    </div>
  );
}

// ─── Recipe modal ─────────────────────────────────────────────────────────────

function RecipeModal({ item, recipe, quantities, isTracked, onTrack, onClose, crafting }: {
  item: CatalogItem; recipe: RecipeComponent[] | null;
  quantities: Record<string, number>; isTracked: boolean;
  onTrack: () => void; onClose: () => void; crafting: CraftingJob[];
}) {
  const [mode, setMode] = useState<"tree" | "needs">("tree");
  const isKuva = isLichWeapon(item);
  const craftJob = crafting.find(c =>
    c.unique_name === item.unique_name ||
    (recipe && recipe.length > 0 && recipe[0].unique_name === c.unique_name)
  );

  const needs = useMemo(() => {
    if (!recipe?.length) return [];
    const acc = new Map<string, { name: string; needed: number }>();
    collectNeeds(recipe, 1, acc);
    return Array.from(acc.entries())
      .map(([unique_name, { name, needed }]) => ({
        unique_name, name, needed, owned: quantities[unique_name] ?? 0,
      }))
      .filter(r => r.owned < r.needed)
      .sort((a, b) => a.name.localeCompare(b.name));
  }, [recipe, quantities]);

  return (
    <div className="craft-modal-overlay" onClick={onClose}>
      <div className="craft-modal" onClick={e => e.stopPropagation()}>

        {/* Header */}
        <div className="craft-modal-header">
          <ItemImg imageName={item.image_name} category={item.category} size={36} />
          <span className="craft-modal-title">{item.name}</span>
          {craftJob && <span className="craft-modal-foundry-badge" title={`Building — ${item.name}`}>⚒ Building</span>}
          <button className={`foundry-track-btn-large ${isTracked ? "tracked" : ""}`} onClick={onTrack}>
            {isTracked ? "★ Tracked" : "☆ Track"}
          </button>
          <button className="craft-detail-close" onClick={onClose}>✕</button>
        </div>

        {isKuva ? (
          <div className="craft-modal-body">
            <div className="craft-kuva-notice">
              <span className="craft-kuva-icon">🔱</span>
              <div>
                <strong>{item.name}</strong> is obtained by converting a{" "}
                {item.name.startsWith("Kuva ") ? <strong>Kuva Lich</strong> : <strong>Tenet Sister</strong>},
                not crafted from a Blueprint.
              </div>
            </div>
          </div>
        ) : (
          <>
            <div className="craft-modal-tabs">
              <button className={`toggle-btn ${mode === "tree" ? "toggle-active" : ""}`} onClick={() => setMode("tree")}>Full tree</button>
              <button className={`toggle-btn ${mode === "needs" ? "toggle-active" : ""}`} onClick={() => setMode("needs")}>What I need</button>
            </div>
            <div className="craft-modal-body">
              {!recipe ? (
                <div className="empty-msg">Loading…</div>
              ) : recipe.length === 0 ? (
                <div className="empty-msg">No recipe data.</div>
              ) : mode === "tree" ? (
                mergeComponents(recipe).map((node, i) => <TreeNode key={i} node={node} quantities={quantities} depth={0} />)
              ) : needs.length === 0 ? (
                <div className="empty-msg">✓ You have everything needed.</div>
              ) : (
                <div className="needs-list">
                  {needs.map(r => (
                    <div key={r.unique_name} className="needs-row">
                      <span className="needs-name">{r.name}</span>
                      <span className="needs-counts">
                        <span className="qty-need">{fmt(r.owned)}</span>
                        <span className="qty-sep">/</span>
                        <span className="qty-required">{fmt(r.needed)}</span>
                        <span className="recipe-shortage">−{fmt(r.needed - r.owned)}</span>
                      </span>
                    </div>
                  ))}
                </div>
              )}
            </div>
          </>
        )}
      </div>
    </div>
  );
}

// ─── Craft card ───────────────────────────────────────────────────────────────

function CraftCard({ item, recipe, quantities, relicDrops, relicNames, masteryData, crafting, isTracked, onTrack, onOpen, subsummedWarframes, archonShards }: {
  item: CatalogItem; recipe: RecipeComponent[] | null;
  quantities: Record<string, number>; relicDrops: Record<string, string[]>;
  relicNames: Record<string, string>; masteryData: Record<string, number>;
  crafting: CraftingJob[]; isTracked: boolean; onTrack: () => void; onOpen: () => void;
  subsummedWarframes: Set<string>; archonShards: Record<string, ArchonShard[]>;
}) {
  const isOwned    = (quantities[item.unique_name] ?? 0) > 0;
  const rank       = masteryData[item.unique_name];
  const isMastered = rank != null && rank >= 30;
  const isSubsumed  = item.category === "Warframes" && subsummedWarframes.has(item.unique_name);
  const shards      = item.category === "Warframes" ? (archonShards[item.unique_name] ?? []) : [];
  // Memory scanner stores the recipe/blueprint path; catalog uses the result-item path.
  // Check both so items like Forma (recipe path ≠ item path) still get the badge.
  const isCrafting = crafting.some(c =>
    c.unique_name === item.unique_name ||
    (recipe && recipe.length > 0 && recipe[0].unique_name === c.unique_name)
  );
  const isKuva     = isLichWeapon(item);
  const mergedRecipe = recipe ? mergeComponents(recipe) : recipe;
  // ⚡ Ready = you have every ingredient itself (not just its blueprint).
  const allParts = mergedRecipe && mergedRecipe.length > 0 && mergedRecipe.every(c =>
    (quantities[c.unique_name] ?? 0) >= (c.count || 1)
  );

  return (
    <div
      className={`craft-card${isOwned ? " craft-card-owned" : ""}${allParts && !isOwned ? " craft-card-ready" : ""}`}
      onClick={onOpen}
    >
      {/* Col 1, rows 1-4: image block with star/wiki/name overlaid */}
      <div className="cc-image">
        <ItemImg imageName={item.image_name} category={item.category} size={78} />
        <button className={`cc-star ${isTracked ? "tracked" : ""}`}
          onClick={e => { e.stopPropagation(); onTrack(); }}>{isTracked ? "★" : "☆"}</button>
        <button className="cc-wiki"
          onClick={e => { e.stopPropagation(); invoke("plugin:opener|open_url", { url:`https://wiki.warframe.com/w/${item.name.replace(" Blueprint","").replace(/\s+/g,"_")}` }).catch(()=>{}); }}>wiki</button>
        <span className="cc-name">{item.name}</span>
      </div>

      {/* Col 1, row 5: MR requirement */}
      <div className="cc-mr">
        {item.mastery_req != null && item.mastery_req > 0 &&
          <span className="craft-mr-req">MR {item.mastery_req}</span>}
      </div>

      {/* Col 1, row 6: vault / kuva badges */}
      <div className="cc-badges">
        {item.vaulted === true  && <span className="vault-badge vault-yes">🔒 Vaulted</span>}
        {item.vaulted === false && <span className="vault-badge vault-no">🔓 Unvaulted</span>}
        {isKuva && <span className="craft-icon-tag craft-icon-kuva" title="Lich/Sister">🔱</span>}
      </div>

      {/* Col 1, row 7: status tags */}
      <div className="cc-tags">
        {isMastered && <span className="craft-icon-tag craft-icon-mastered" title="Mastered">★</span>}
        {isOwned && !isMastered && rank != null && <span className="craft-icon-tag craft-icon-rank">R{rank}</span>}
        {isCrafting  && <span className="craft-icon-tag craft-icon-foundry" title="Building">⚒</span>}
        {isSubsumed  && <span className="craft-icon-tag craft-icon-helminth" title="Subsumed"><HelminthIcon /></span>}
        {shards.length > 0 && <ArchonCrystalIcon shards={shards} />}
        {isOwned     && <span className="foundry-cb-badge foundry-cb-owned">✓✓</span>}
        {!isOwned && allParts && <span className="foundry-cb-badge foundry-cb-ready">⚡</span>}
      </div>

      {/* Col 2, rows 1-7: ingredient list — rows grow to fill available height */}
      <div className="cc-ingredients">
        {recipe === null ? (
          <div className="comp-row-loading">Loading…</div>
        ) : recipe.length === 0 ? (
          <div className="comp-row-loading">No recipe</div>
        ) : (
          mergedRecipe!.map((comp, i) => (
            <CompRow key={i} comp={comp} quantities={quantities} relicDrops={relicDrops} relicNames={relicNames} />
          ))
        )}
      </div>
    </div>
  );
}

// ─── Foundry ─────────────────────────────────────────────────────────────────

const CRAFT_CATEGORIES = [
  "Warframes", "Primary", "Secondary", "Melee",
  "Companions", "Archwing", "Blueprints", "Misc",
];

export default function Foundry({ quantities, masteryData, refreshKey, crafting, subsummedWarframes = new Set(), archonShards = {}, tracked, onTrackToggle }: Props) {
  const [craftable, setCraftable] = useState<CatalogItem[]>([]);
  const [search, setSearch]       = useState("");
  const [activeCat, setActiveCat] = useState("Warframes");
  const [recipes, setRecipes]     = useState<Map<string, RecipeComponent[]>>(new Map());
  const [relicDrops, setRelicDrops] = useState<Record<string, string[]>>({});
  const [relicNames, setRelicNames] = useState<Record<string, string>>({});
  const [modalItem, setModalItem] = useState<CatalogItem | null>(null);

  // Mix-and-match filters
  const [filterPrime,      setFilterPrime]      = useState(false);
  const [filterVaulted,    setFilterVaulted]     = useState(false);
  const [filterUnvaulted,  setFilterUnvaulted]   = useState(false);
  const [filterMastered,   setFilterMastered]    = useState(false);
  const [filterUnmastered, setFilterUnmastered]  = useState(false);
  const [filterOwned,      setFilterOwned]       = useState(false);
  const [filterUnowned,    setFilterUnowned]     = useState(false);
  const [filterReady,      setFilterReady]       = useState(false);

  useEffect(() => {
    invoke<CatalogItem[]>("get_craftable_items").then(setCraftable).catch(() => setCraftable([]));
    invoke<Record<string, string[]>>("get_relic_drops").then(setRelicDrops).catch(() => {});
    invoke<Array<{ unique_name: string; name: string; category: string }>>("get_all_items")
      .then(items => {
        const map: Record<string, string> = {};
        for (const i of items) if (i.category === "Relics") map[i.unique_name] = i.name;
        setRelicNames(map);
      }).catch(() => {});
  }, [refreshKey]);

  const visible = useMemo(() => {
    const q = search.toLowerCase();
    return craftable
      .filter(i => i.category === activeCat || activeCat === "All")
      .filter(i => !q || i.name.toLowerCase().includes(q))
      .filter(i => !filterPrime     || i.name.includes("Prime") || i.vaulted != null)
      .filter(i => !filterVaulted   || i.vaulted === true)
      .filter(i => !filterUnvaulted || i.vaulted === false)
      .filter(i => {
        if (!filterMastered && !filterUnmastered) return true;
        const isMastered = (masteryData[i.unique_name] ?? 0) >= 30;
        return filterMastered ? isMastered : !isMastered;
      })
      .filter(i => {
        if (!filterOwned && !filterUnowned) return true;
        const owned = (quantities[i.unique_name] ?? 0) > 0;
        return filterOwned ? owned : !owned;
      })
      .filter(i => {
        if (!filterReady) return true;
        const r = recipes.get(i.unique_name);
        if (!r || r.length === 0) return false;
        if ((quantities[i.unique_name] ?? 0) > 0) return false;
        return mergeComponents(r).every(c => (quantities[c.unique_name] ?? 0) >= (c.count || 1));
      });
  }, [craftable, activeCat, search, filterPrime, filterVaulted, filterUnvaulted,
      filterMastered, filterUnmastered, filterOwned, filterUnowned, filterReady,
      recipes, quantities, masteryData]);

  // Load recipes for visible items
  useEffect(() => {
    const toLoad = visible.filter(i => !recipes.has(i.unique_name));
    if (toLoad.length === 0) return;
    let cancelled = false;
    Promise.all(
      toLoad.map(item =>
        invoke<RecipeComponent[]>("get_recipe", { uniqueName: item.unique_name })
          .then(r => [item.unique_name, r ?? []] as [string, RecipeComponent[]])
          .catch(() => [item.unique_name, []] as [string, RecipeComponent[]])
      )
    ).then(results => {
      if (cancelled) return;
      setRecipes(prev => {
        const next = new Map(prev);
        for (const [id, r] of results) next.set(id, r);
        return next;
      });
    });
    return () => { cancelled = true; };
  }, [visible]);

  // Load recipe for modal item
  useEffect(() => {
    if (!modalItem || recipes.has(modalItem.unique_name)) return;
    invoke<RecipeComponent[]>("get_recipe", { uniqueName: modalItem.unique_name })
      .then(r => setRecipes(prev => new Map(prev).set(modalItem.unique_name, r ?? [])))
      .catch(() => {});
  }, [modalItem]);

  const toggleTrack = useCallback((item: CatalogItem) => {
    onTrackToggle(item.unique_name);
  }, [onTrackToggle]);

  const categoryCounts = useMemo(() => {
    const counts: Record<string, number> = {};
    for (const i of craftable) counts[i.category] = (counts[i.category] ?? 0) + 1;
    return counts;
  }, [craftable]);

  const modalRecipe = modalItem ? (recipes.get(modalItem.unique_name) ?? null) : null;

  // Close modal on Escape
  useEffect(() => {
    if (!modalItem) return;
    const handler = (e: KeyboardEvent) => { if (e.key === "Escape") setModalItem(null); };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [modalItem]);

  return (
    <div className="foundry">

      {/* ── Modal overlay ── */}
      {modalItem && (
        <RecipeModal
          item={modalItem}
          recipe={modalRecipe}
          quantities={quantities}
          isTracked={tracked.includes(modalItem.unique_name)}
          onTrack={() => toggleTrack(modalItem)}
          onClose={() => setModalItem(null)}
          crafting={crafting}
        />
      )}

      {/* ── Col 1: Category sidebar ── */}
      <div className="foundry-sidebar">
        <div className="foundry-search-wrap">
          <input className="foundry-search" placeholder="Search…" value={search}
            onChange={e => { setSearch(e.target.value); if (e.target.value) setActiveCat("All" as any); }} />
        </div>
        {CRAFT_CATEGORIES.map(cat => (
          <button key={cat} className={`cat-btn ${activeCat === cat ? "cat-active" : ""}`}
            onClick={() => { setActiveCat(cat); setSearch(""); }}>
            <span className="cat-label">{cat}</span>
            {categoryCounts[cat] ? (
              <span className="cat-count"><span className="cat-total">{categoryCounts[cat]}</span></span>
            ) : null}
          </button>
        ))}
      </div>

      {/* ── Col 2: Card grid ── */}
      <div className="foundry-main">
        <div className="filter-bar">
          <button className={`fchip ${filterPrime     ? "fchip-on" : ""}`} onClick={() => setFilterPrime(v => !v)}>Prime</button>
          <button className={`fchip ${filterVaulted   ? "fchip-on" : ""}`} onClick={() => setFilterVaulted(v => !v)}>🔒 Vaulted</button>
          <button className={`fchip ${filterUnvaulted ? "fchip-on" : ""}`} onClick={() => setFilterUnvaulted(v => !v)}>🔓 Unvaulted</button>
          <span className="fbar-sep"/>
          <button className={`fchip ${filterOwned     ? "fchip-on" : ""}`} onClick={() => setFilterOwned(v => !v)}>✓ Owned</button>
          <button className={`fchip ${filterUnowned   ? "fchip-on" : ""}`} onClick={() => setFilterUnowned(v => !v)}>✕ Unowned</button>
          <button className={`fchip ${filterReady     ? "fchip-on" : ""}`} onClick={() => setFilterReady(v => !v)}>⚡ Ready</button>
          <span className="fbar-sep"/>
          <button className={`fchip ${filterMastered  ? "fchip-on" : ""}`} onClick={() => setFilterMastered(v => !v)}>★ Mastered</button>
          <button className={`fchip ${filterUnmastered? "fchip-on" : ""}`} onClick={() => setFilterUnmastered(v => !v)}>☆ Unmastered</button>
          <span style={{ marginLeft: "auto", fontSize: 11, color: "var(--muted)" }}>{visible.length} items</span>
          <HelpTip items={[
            { swatch: "rgba(240,192,64,.5)", icon: "✓✓", label: "Owned",          desc: "Gold border + ✓✓ — item built and in inventory" },
            { swatch: "rgba(56,139,253,.5)", icon: "⚡",  label: "Ready to craft", desc: "Blue border + ⚡ — all parts collected" },
            { swatch: "rgba(240,192,64,.4)", icon: "BP",  label: "Blueprint",      desc: "Gold comp row — blueprint in inventory" },
            { swatch: "rgba(63,185,80,.4)",  icon: "✓",   label: "Part owned",     desc: "Green comp row — component in inventory" },
            { icon: "★",  label: "★ Mastered", desc: "Item levelled to rank 30" },
            { icon: "⚒",  label: "⚒ Building", desc: "Currently crafting in the Foundry" },
            { icon: "MR", label: "MR{n}",       desc: "Required Mastery Rank to use" },
          ]} />
        </div>

        <div className="craft-grid">
          {visible.length === 0 && (
            <div className="empty-msg">
              {craftable.length === 0 ? "No recipes loaded — refresh item list first." : "No items match."}
            </div>
          )}
          {visible.map(item => (
            <CraftCard
              key={item.unique_name}
              item={item}
              recipe={recipes.has(item.unique_name) ? recipes.get(item.unique_name)! : null}
              quantities={quantities}
              relicDrops={relicDrops}
              relicNames={relicNames}
              masteryData={masteryData}
              crafting={crafting}
              isTracked={tracked.includes(item.unique_name)}
              onTrack={() => toggleTrack(item)}
              onOpen={() => setModalItem(item)}

              subsummedWarframes={subsummedWarframes}
              archonShards={archonShards}
            />
          ))}
        </div>
      </div>

    </div>
  );
}
