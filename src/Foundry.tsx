import { useState, useEffect, useMemo, useCallback, memo, startTransition, useRef, useContext } from "react";
import { invoke } from "@tauri-apps/api/core";
import { ImgCacheDirContext } from "./ImgCacheDir";
import { HelpTip } from "./HelpTip";
import type { InventoryItem } from "./App";
import sentientIcon from "./assets/SentientFactionIcon.webp";

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

export interface FoundryFilters {
  search: string; activeCat: string;
  filterPrime: boolean; filterVaulted: boolean; filterUnvaulted: boolean;
  filterMastered: boolean; filterUnmastered: boolean;
  filterOwned: boolean; filterUnowned: boolean; filterReady: boolean;
}
export const FOUNDRY_FILTERS_DEFAULT: FoundryFilters = {
  search: "", activeCat: "Warframes",
  filterPrime: false, filterVaulted: false, filterUnvaulted: false,
  filterMastered: false, filterUnmastered: false,
  filterOwned: false, filterUnowned: false, filterReady: false,
};

interface Props {
  inventory: Record<string, InventoryItem>;
  refreshKey: number;
  crafting: CraftingJob[];
  colorblindMode?: boolean;
  subsummedWarframes?: Set<string>;
  tracked: string[];
  onTrackToggle: (id: string) => void;
  filters: FoundryFilters;
  onFiltersChange: (f: FoundryFilters) => void;
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

function compStatus(comp: RecipeComponent, inventory: Record<string, InventoryItem>): CompStatus {
  if ((inventory[comp.unique_name]?.quantity ?? 0) >= (comp.count || 1)) return "part";
  const bpUnique = comp.components[0]?.unique_name;
  if (bpUnique && (inventory[bpUnique]?.quantity ?? 0) > 0) return "blueprint";
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

// Pentagon where each of the 5 sides represents one shard slot.
// Filled slot → side drawn in shard color; empty slot → dim grey.
function ArchonCrystalIcon({ shards }: { shards: ArchonShard[] }) {
  try {
    const hasAny = shards && shards.length > 0;
    const lines = hasAny
      ? shards.map(s => `${s.tauforged ? "✦ " : ""}${s.type}${s.boost ? ` · ${s.boost}` : ""}`).join("\n")
      : "No Archon Shards";

    const R = 6.5;
    const cx = 8, cy = 8.2; // slight down-shift so top vertex sits at ~y=1.7
    // 5 vertices starting from the top (270°), going clockwise
    const verts = Array.from({ length: 5 }, (_, i) => {
      const a = (Math.PI / 180) * (270 + i * 72);
      return { x: cx + R * Math.cos(a), y: cy + R * Math.sin(a) };
    });

    const pts = verts.map(v => `${v.x.toFixed(2)},${v.y.toFixed(2)}`).join(" ");
    return (
      <span
        className="craft-icon-tag craft-icon-archon"
        title={`Archon Shards:\n${lines}`}
        style={{ padding: 0 }}
      >
        <svg width="18" height="18" viewBox="0 0 16 16" fill="none" style={{ display: "block" }}>
          {/* 1. Tauforged wedge fills — drawn first so outline goes on top */}
          {Array.from({ length: 5 }, (_, i) => {
            const s = shards?.[i];
            if (!s?.tauforged) return null;
            const a = verts[i], b = verts[(i + 1) % 5];
            const wpts = `${a.x.toFixed(2)},${a.y.toFixed(2)} ${b.x.toFixed(2)},${b.y.toFixed(2)} ${cx.toFixed(2)},${cy.toFixed(2)}`;
            return <polygon key={i} points={wpts} fill={s.color} fillOpacity={0.8} />;
          })}
          {/* 2. Dividing lines from each vertex to center */}
          {verts.map((v, i) => (
            <line key={`div-${i}`}
              x1={v.x.toFixed(2)} y1={v.y.toFixed(2)}
              x2={cx.toFixed(2)} y2={cy.toFixed(2)}
              stroke="rgba(0,0,0,0.45)" strokeWidth={0.6}
            />
          ))}
          {/* 3. Normal shard edge lines */}
          {Array.from({ length: 5 }, (_, i) => {
            const s = shards?.[i];
            if (!s || s.tauforged) return null;
            const a = verts[i], b = verts[(i + 1) % 5];
            return (
              <line key={`edge-${i}`}
                x1={a.x.toFixed(2)} y1={a.y.toFixed(2)}
                x2={b.x.toFixed(2)} y2={b.y.toFixed(2)}
                stroke={s.color} strokeWidth={2.4} strokeLinecap="butt"
              />
            );
          })}
          {/* 4. Outer pentagon outline drawn last — always on top, always one clean connected shape */}
          <polygon points={pts} fill="none" stroke="rgba(255,255,255,0.45)" strokeWidth={0.8} />
        </svg>
      </span>
    );
  } catch {
    return null;
  }
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
function ownsRelicVariant(relicUnique: string, inventory: Record<string, InventoryItem>): boolean {
  const base = relicUnique.replace(/(Bronze|Silver|Gold|Platinum)$/, "");
  return RELIC_SUFFIXES.some(s => (inventory[`${base}${s}`]?.quantity ?? 0) > 0);
}

// ─── Item image ───────────────────────────────────────────────────────────────

function ItemImg({ imageName, category, size = 40 }: { imageName?: string; category: string; size?: number }) {
  const baseUrl = useContext(ImgCacheDirContext);
  const [localFailed, setLocalFailed] = useState(false);
  const [cdnFailed,   setCdnFailed]   = useState(false);
  const style = { width: size, height: size, flexShrink: 0 };
  if (!imageName || cdnFailed)
    return <span className="item-img-fallback" style={{ ...style, fontSize: size * 0.35 }}>{category[0].toUpperCase()}</span>;
  const useLocal = Boolean(baseUrl) && !localFailed;
  const src = useLocal
    ? `${baseUrl}/${imageName}`
    : `https://cdn.warframestat.us/img/${imageName}`;
  return (
    <img className="item-img" style={style} src={src}
      alt="" loading="lazy"
      onError={() => useLocal ? setLocalFailed(true) : setCdnFailed(true)} />
  );
}

// ─── Comp row (used inside modal tree) ───────────────────────────────────────

function CompRow({ comp, inventory, relicDrops, relicNames }: {
  comp: RecipeComponent; inventory: Record<string, InventoryItem>;
  relicDrops: Record<string, string[]>; relicNames: Record<string, string>;
}) {
  const status = compStatus(comp, inventory);
  const ownedRelics = [...new Set(
    (relicDrops[comp.unique_name] ?? [])
      .filter(r => ownsRelicVariant(r, inventory))
      .map(r => {
        const base = r.replace(/(Bronze|Silver|Gold|Platinum)$/, "");
        const owned = RELIC_SUFFIXES.find(s => (inventory[`${base}${s}`]?.quantity ?? 0) > 0);
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

function TreeNode({ node, inventory, depth }: {
  node: RecipeComponent; inventory: Record<string, InventoryItem>; depth: number;
}) {
  const owned = inventory[node.unique_name]?.quantity ?? 0;
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
        <TreeNode key={i} node={child} inventory={inventory} depth={depth + 1} />
      ))}
    </div>
  );
}

// ─── Recipe modal ─────────────────────────────────────────────────────────────

function RecipeModal({ item, recipe, inventory, isTracked, onTrack, onClose, crafting }: {
  item: CatalogItem; recipe: RecipeComponent[] | null;
  inventory: Record<string, InventoryItem>; isTracked: boolean;
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
        unique_name, name, needed, owned: inventory[unique_name]?.quantity ?? 0,
      }))
      .filter(r => r.owned < r.needed)
      .sort((a, b) => a.name.localeCompare(b.name));
  }, [recipe, inventory]);

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
                mergeComponents(recipe).map((node, i) => <TreeNode key={i} node={node} inventory={inventory} depth={0} />)
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

const CraftCard = memo(function CraftCard({ item, recipe, inventory, relicDrops, relicNames, crafting, isTracked, onTrack, onOpen, subsummedWarframes }: {
  item: CatalogItem; recipe: RecipeComponent[] | null;
  inventory: Record<string, InventoryItem>; relicDrops: Record<string, string[]>;
  relicNames: Record<string, string>;
  crafting: CraftingJob[]; isTracked: boolean;
  onTrack: (item: CatalogItem) => void;
  onOpen: (item: CatalogItem) => void;
  subsummedWarframes: Set<string>;
}) {
  const invEntry   = inventory[item.unique_name];
  const isOwned    = (invEntry?.quantity ?? 0) > 0;
  const rank       = invEntry?.mastery_rank;
  const isMastered = rank != null && rank > 0 && rank >= 30;
  const isSubsumed  = item.category === "Warframes" && subsummedWarframes.has(item.unique_name);
  const shards      = item.category === "Warframes" ? (invEntry?.archon_shards ?? []) : [];
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
    (inventory[c.unique_name]?.quantity ?? 0) >= (c.count || 1)
  );

  return (
    <div
      className={`craft-card${isOwned ? " craft-card-owned" : ""}${allParts && !isOwned ? " craft-card-ready" : ""}`}
      onClick={() => onOpen(item)}
    >
      {/* Col 1, rows 1-4: image block with star/wiki/name overlaid */}
      <div className="cc-image">
        <ItemImg imageName={item.image_name} category={item.category} size={78} />
        <button className={`cc-star ${isTracked ? "tracked" : ""}`}
          onClick={e => { e.stopPropagation(); onTrack(item); }}>{isTracked ? "★" : "☆"}</button>
        <button className="cc-wiki"
          onClick={e => { e.stopPropagation(); invoke("plugin:opener|open_url", { url:`https://wiki.warframe.com/w/${item.name.replace(" Blueprint","").replace(/\s+/g,"_")}` }).catch(()=>{}); }}>wiki</button>
        <span className="cc-name">{item.name}</span>
      </div>

      {/* Col 1, row 5: MR requirement + subsumed indicator */}
      <div className="cc-mr">
        {isSubsumed && <img src={sentientIcon} className="cc-subsumed-icon" title="Subsumed into Helminth" alt="Subsumed" />}
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
            <CompRow key={i} comp={comp} inventory={inventory} relicDrops={relicDrops} relicNames={relicNames} />
          ))
        )}
      </div>
    </div>
  );
}, (prev, next) => {
  // Only re-render when props that affect this card's display actually change.
  // Avoids re-rendering all cards on every 10-second inventory scan.
  if (prev.item          !== next.item)          return false;
  if (prev.recipe        !== next.recipe)        return false;
  if (prev.isTracked     !== next.isTracked)     return false;
  if (prev.crafting      !== next.crafting)      return false;
  if (prev.onTrack       !== next.onTrack)       return false;
  if (prev.onOpen        !== next.onOpen)        return false;
  if (prev.relicDrops    !== next.relicDrops)    return false;
  if (prev.relicNames    !== next.relicNames)    return false;
  if (prev.subsummedWarframes !== next.subsummedWarframes) return false;
  // Inventory: only check keys this specific card reads
  const keys = new Set<string>([prev.item.unique_name]);
  for (const c of (prev.recipe ?? [])) {
    keys.add(c.unique_name);
    if (c.components[0]) keys.add(c.components[0].unique_name);
  }
  for (const k of keys) {
    if ((prev.inventory[k]?.quantity    ?? 0)    !== (next.inventory[k]?.quantity    ?? 0))    return false;
    if ((prev.inventory[k]?.mastery_rank ?? null) !== (next.inventory[k]?.mastery_rank ?? null)) return false;
  }
  const pShards = prev.inventory[prev.item.unique_name]?.archon_shards;
  const nShards = next.inventory[next.item.unique_name]?.archon_shards;
  if ((pShards?.length ?? 0) !== (nShards?.length ?? 0)) return false;
  return true;
});

// ─── Foundry ─────────────────────────────────────────────────────────────────

const CRAFT_CATEGORIES = [
  "Warframes", "Primary", "Secondary", "Melee",
  "Companions", "Archwing", "Blueprints", "Misc",
];

export default function Foundry({ inventory, refreshKey, crafting, subsummedWarframes = new Set(), tracked, onTrackToggle, filters, onFiltersChange }: Props) {
  const [craftable, setCraftable] = useState<CatalogItem[]>([]);
  const [recipes, setRecipes]     = useState<Map<string, RecipeComponent[]>>(new Map());
  const [relicDrops, setRelicDrops] = useState<Record<string, string[]>>({});
  const [relicNames, setRelicNames] = useState<Record<string, string>>({});
  const [modalItem, setModalItem] = useState<CatalogItem | null>(null);
  const [inputSearch, setInputSearch] = useState(filters.search);
  const [page, setPage] = useState(0);

  // Refs so debounce closure always reads latest values without stale captures
  const filtersRef = useRef(filters);
  filtersRef.current = filters;
  const onFiltersChangeRef = useRef(onFiltersChange);
  onFiltersChangeRef.current = onFiltersChange;

  const trackedSet = useMemo(() => new Set(tracked), [tracked]);

  // Sync local input when parent resets search (e.g. "Show All" button)
  useEffect(() => { setInputSearch(filters.search); }, [filters.search]); // eslint-disable-line

  // Wait 150 ms after last keystroke before propagating to parent filters
  useEffect(() => {
    if (inputSearch === filtersRef.current.search) return;
    const id = setTimeout(() => {
      const f = filtersRef.current;
      onFiltersChangeRef.current({ ...f, search: inputSearch, ...(inputSearch ? { activeCat: "All" as any } : {}) });
    }, 150);
    return () => clearTimeout(id);
  }, [inputSearch]); // eslint-disable-line

  const { search, activeCat, filterPrime, filterVaulted, filterUnvaulted, filterMastered, filterUnmastered, filterOwned, filterUnowned, filterReady } = filters;
  const set = <K extends keyof FoundryFilters>(k: K, v: FoundryFilters[K]) => onFiltersChange({ ...filters, [k]: v });
  const isFiltered = search !== "" || filterPrime || filterVaulted || filterUnvaulted || filterMastered || filterUnmastered || filterOwned || filterUnowned || filterReady;

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
        const isMastered = (inventory[i.unique_name]?.mastery_rank ?? 0) >= 30;
        return filterMastered ? isMastered : !isMastered;
      })
      .filter(i => {
        if (!filterOwned && !filterUnowned) return true;
        const owned = (inventory[i.unique_name]?.quantity ?? 0) > 0;
        return filterOwned ? owned : !owned;
      })
      .filter(i => {
        if (!filterReady) return true;
        const r = recipes.get(i.unique_name);
        if (!r || r.length === 0) return false;
        if ((inventory[i.unique_name]?.quantity ?? 0) > 0) return false;
        return mergeComponents(r).every(c => (inventory[c.unique_name]?.quantity ?? 0) >= (c.count || 1));
      });
  }, [craftable, activeCat, search, filterPrime, filterVaulted, filterUnvaulted,
      filterMastered, filterUnmastered, filterOwned, filterUnowned, filterReady,
      // Only pull in inventory/recipes when a filter that actually reads them is active.
      // Without this guard, every 10-second scanner update re-renders all 100+ cards.
      (filterMastered || filterUnmastered || filterOwned || filterUnowned || filterReady) ? inventory : null,
      filterReady ? recipes : null,
  ]);

  // Reset to page 1 when the user changes a filter, search, or category —
  // but NOT when inventory or recipes update in the background.
  // Without this guard, every 10-second scan resets the page mid-browse.
  useEffect(() => { setPage(0); }, [ // eslint-disable-line
    activeCat, search, filterPrime, filterVaulted, filterUnvaulted,
    filterMastered, filterUnmastered, filterOwned, filterUnowned, filterReady,
    craftable,
  ]);
  const PAGE_SIZE = 30;
  const pageCount = Math.ceil(visible.length / PAGE_SIZE);
  const pagedItems = useMemo(() => visible.slice(page * PAGE_SIZE, (page + 1) * PAGE_SIZE), [visible, page]);

  // Load recipes for visible items — one bulk IPC call instead of N concurrent calls
  useEffect(() => {
    const toLoad = visible.filter(i => !recipes.has(i.unique_name));
    if (toLoad.length === 0) return;
    let cancelled = false;
    invoke<Record<string, RecipeComponent[]>>("get_recipes_bulk", {
      uniqueNames: toLoad.map(i => i.unique_name),
    }).then(result => {
      if (cancelled) return;
      startTransition(() => {
        setRecipes(prev => {
          const next = new Map(prev);
          for (const item of toLoad) {
            next.set(item.unique_name, result[item.unique_name] ?? []);
          }
          return next;
        });
      });
    }).catch(() => {});
    return () => { cancelled = true; };
  }, [visible]);

  // Load recipe for modal item
  useEffect(() => {
    if (!modalItem || recipes.has(modalItem.unique_name)) return;
    invoke<RecipeComponent[]>("get_recipe", { uniqueName: modalItem.unique_name })
      .then(r => setRecipes(prev => new Map(prev).set(modalItem.unique_name, r ?? [])))
      .catch(() => {});
  }, [modalItem]);

  const handleTrack = useCallback((item: CatalogItem) => {
    onTrackToggle(item.unique_name);
  }, [onTrackToggle]);

  const handleOpen = useCallback((item: CatalogItem) => {
    setModalItem(item);
  }, []);

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
          inventory={inventory}
          isTracked={tracked.includes(modalItem.unique_name)}
          onTrack={() => handleTrack(modalItem)}
          onClose={() => setModalItem(null)}
          crafting={crafting}
        />
      )}

      {/* ── Col 1: Category sidebar ── */}
      <div className="foundry-sidebar">
        <div className="foundry-search-wrap">
          <input className="foundry-search" placeholder="Search…" value={inputSearch}
            onChange={e => setInputSearch(e.target.value)} />
        </div>
        {CRAFT_CATEGORIES.map(cat => (
          <button key={cat} className={`cat-btn ${activeCat === cat ? "cat-active" : ""}`}
            onClick={() => onFiltersChange({ ...filters, activeCat: cat, search: "" })}>
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
          <button className={`fchip ${filterPrime     ? "fchip-on" : ""}`} onClick={() => set("filterPrime", !filterPrime)}>Prime</button>
          <button className={`fchip ${filterVaulted   ? "fchip-on" : ""}`} onClick={() => set("filterVaulted", !filterVaulted)}>🔒 Vaulted</button>
          <button className={`fchip ${filterUnvaulted ? "fchip-on" : ""}`} onClick={() => set("filterUnvaulted", !filterUnvaulted)}>🔓 Unvaulted</button>
          <span className="fbar-sep"/>
          <button className={`fchip ${filterOwned     ? "fchip-on" : ""}`} onClick={() => set("filterOwned", !filterOwned)}>✓ Owned</button>
          <button className={`fchip ${filterUnowned   ? "fchip-on" : ""}`} onClick={() => set("filterUnowned", !filterUnowned)}>✕ Unowned</button>
          <button className={`fchip ${filterReady     ? "fchip-on" : ""}`} onClick={() => set("filterReady", !filterReady)}>⚡ Ready</button>
          <span className="fbar-sep"/>
          <button className={`fchip ${filterMastered  ? "fchip-on" : ""}`} onClick={() => set("filterMastered", !filterMastered)}>★ Mastered</button>
          <button className={`fchip ${filterUnmastered? "fchip-on" : ""}`} onClick={() => set("filterUnmastered", !filterUnmastered)}>☆ Unmastered</button>
          {isFiltered && <button className="fchip fchip-reset" onClick={() => onFiltersChange({ ...FOUNDRY_FILTERS_DEFAULT, activeCat })}>Show All</button>}
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
          {pagedItems.map(item => (
            <CraftCard
              key={item.unique_name}
              item={item}
              recipe={recipes.has(item.unique_name) ? recipes.get(item.unique_name)! : null}
              inventory={inventory}
              relicDrops={relicDrops}
              relicNames={relicNames}
              crafting={crafting}
              isTracked={trackedSet.has(item.unique_name)}
              onTrack={handleTrack}
              onOpen={handleOpen}
              subsummedWarframes={subsummedWarframes}
            />
          ))}
        </div>
        {pageCount > 1 && (
          <div className="foundry-pagination">
            <button className="btn-secondary" disabled={page === 0} onClick={() => setPage(p => p - 1)}>← Prev</button>
            <span className="foundry-pg-label">Page {page + 1} of {pageCount}</span>
            <button className="btn-secondary" disabled={page >= pageCount - 1} onClick={() => setPage(p => p + 1)}>Next →</button>
          </div>
        )}
      </div>

    </div>
  );
}
