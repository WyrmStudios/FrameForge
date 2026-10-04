import { useState, useEffect, useMemo, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import ItemImg from "../ItemImg";
import type { InventoryItem, WeaponItem } from "../types/items";
import { FilterChip } from "../shared/ui/FilterControls";

// ── Helpers ───────────────────────────────────────────────────────────────────

const WEAPON_TABS = ["Primary", "Secondary", "Melee", "Operator"] as const;
type WeaponTab = typeof WEAPON_TABS[number];

const TAB_CATEGORY: Record<WeaponTab, string> = {
  Primary: "Primary",
  Secondary: "Secondary",
  Melee: "Melee",
  Operator: "Operator Weapons",
};

const GROUP_ORDER = ["Standard", "Zaw", "Prime", "Kuva", "Tenet", "Coda", "Wraith", "Vandal", "Prisma", "MK1"];
const LEVELABLE_CATS = new Set(["Primary", "Secondary", "Melee", "Operator Weapons"]);

function weaponGroup(item: WeaponItem): string {
  if (item.unique_name.includes("/Ostron/Melee/")) return "Zaw";
  const name = item.name;
  if (name.startsWith("MK1-"))  return "MK1";
  if (name.startsWith("Kuva ")) return "Kuva";
  if (name.startsWith("Tenet "))return "Tenet";
  if (name.startsWith("Coda ")) return "Coda";
  if (name.includes("Prime"))   return "Prime";
  if (name.includes("Wraith"))  return "Wraith";
  if (name.includes("Vandal"))  return "Vandal";
  if (name.includes("Prisma"))  return "Prisma";
  return "Standard";
}

function effectiveCap(item: WeaponItem): number {
  if (item.max_level_cap != null && item.max_level_cap > 0) return item.max_level_cap;
  return LEVELABLE_CATS.has(item.category) ? 30 : 0;
}

// ── Presentation ──────────────────────────────────────────────────────────────

const WPN_ROOT_CLASS = "flex flex-1 min-h-0 flex-col overflow-hidden bg-background text-foreground";
const WPN_TABS_CLASS = "flex shrink-0 gap-0.5 border-b border-border px-3 pt-2";
const WPN_TAB_CLASS = "-mb-px cursor-pointer rounded-t-6 border-0 border-b-3 px-5 py-1.5 text-13 font-medium transition-[background,color] duration-150";
const WPN_TAB_ACTIVE_CLASS = "border-accent bg-[var(--bg-card)] text-foreground";
const WPN_TAB_IDLE_CLASS = "border-transparent bg-transparent text-dim hover:bg-[var(--hover)] hover:text-foreground";
const WPN_TOOLBAR_CLASS = "flex shrink-0 items-center gap-3 px-3.5 pb-1.5 pt-2.5";
const WPN_SEARCH_CLASS = "w-40 shrink-0 rounded-5 border border-border bg-[var(--bg-card)] px-2 py-1 text-12 text-foreground placeholder:text-dim focus:border-accent focus:outline-none";
const WPN_PROGRESS_WRAP_CLASS = "flex flex-1 items-center gap-2";
const WPN_PROGRESS_BAR_CLASS = "h-1.5 max-w-50 flex-1 overflow-hidden rounded-3 bg-border";
const WPN_PROGRESS_FILL_CLASS = "h-full rounded-3 bg-accent transition-[width] duration-300";
const WPN_PROGRESS_LABEL_CLASS = "whitespace-nowrap text-12 text-dim";
const WPN_FILTER_CLASS = "cursor-pointer rounded-5 border px-2.5 py-1 text-12 transition-all duration-150";
const WPN_FILTER_ACTIVE_CLASS = "border-accent bg-accent text-white";
const WPN_FILTER_IDLE_CLASS = "border-border bg-transparent text-dim";
const WPN_BODY_CLASS = "flex-1 overflow-y-auto px-3 pb-4 pt-2";
const WPN_HEADER_CLASS = "mb-1.5 border-b border-border pb-1.5 pt-1 text-11 font-semibold uppercase tracking-0.06 text-dim";
const WPN_GRID_CLASS = "grid grid-cols-[repeat(auto-fill,minmax(240px,1fr))] gap-0.75";
const WPN_ITEM_CLASS = "flex items-center gap-2 rounded-5 border px-2 py-1.25 transition-[background] duration-100 hover:bg-[var(--hover)]";
const WPN_ITEM_MASTERED_CLASS = "border-complete/15 hover:border-complete/15";
const WPN_ITEM_IDLE_CLASS = "border-transparent hover:border-border";
const WPN_NAME_CLASS = "flex-1 truncate text-12";
const WPN_MR_CLASS = "shrink-0 text-10 text-dim";
const WPN_RANK_CLASS = "min-w-9 shrink-0 rounded-4 px-1.5 py-0.5 text-center text-11 font-bold";
const WPN_RANK_DONE_CLASS = "bg-complete/15 text-complete";
const WPN_RANK_PARTIAL_CLASS = "bg-ducat/12 text-ducat";
const WPN_RANK_ZERO_CLASS = "bg-rank-none/7 text-dim";
const WPN_EMPTY_CLASS = "py-10 text-center text-14 text-dim";

// ── Item row ──────────────────────────────────────────────────────────────────

function WeaponRow({ item, rank }: { item: WeaponItem; rank: number }) {
  const cap = effectiveCap(item);
  const mastered = cap > 0 && rank >= cap;
  const rankLabel = cap > 0 ? `R${rank}/${cap}` : `R${rank}`;

  return (
    <div className={`${WPN_ITEM_CLASS} ${mastered ? WPN_ITEM_MASTERED_CLASS : WPN_ITEM_IDLE_CLASS}${!mastered && rank === 0 ? " opacity-45" : ""}`}>
      <ItemImg imageName={item.image_name} fallbackText={item.name[0]?.toUpperCase() ?? "?"} />
      <span className={WPN_NAME_CLASS}>{item.name}</span>
      {item.mastery_req != null && item.mastery_req > 0 && (
        <span className={WPN_MR_CLASS} title={`Mastery Rank ${item.mastery_req} required`}>MR{item.mastery_req}</span>
      )}
      <span className={`${WPN_RANK_CLASS} ${mastered ? WPN_RANK_DONE_CLASS : rank > 0 ? WPN_RANK_PARTIAL_CLASS : WPN_RANK_ZERO_CLASS}`}>
        {mastered ? "✓" : rankLabel}
      </span>
    </div>
  );
}

// ── Main component ────────────────────────────────────────────────────────────

interface Props {
  inventory: Record<string, InventoryItem>;
  activeTab: WeaponTab;
  onTabChange: (t: WeaponTab) => void;
}

export default function Weapons({ inventory, activeTab, onTabChange }: Props) {
  const [allWeapons, setAllWeapons] = useState<WeaponItem[]>([]);
  const [loading, setLoading] = useState(true);
  const [search, setSearch] = useState("");
  const [unmasteredOnly, setUnmasteredOnly] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    invoke<WeaponItem[]>("get_weapon_catalog")
      .then(w => { setAllWeapons(w); setLoading(false); })
      .catch(() => setLoading(false));
  }, []);

  // Reset search when switching tabs
  useEffect(() => { setSearch(""); }, [activeTab]);

  const { groups, masteredCount, totalCount } = useMemo(() => {
    const q = search.toLowerCase();
    const tabWeapons = allWeapons.filter(w => w.category === TAB_CATEGORY[activeTab]);

    const byGroup = new Map<string, { item: WeaponItem; rank: number }[]>();
    let mastered = 0;
    let total = 0;

    for (const w of tabWeapons) {
      const rank = inventory[w.unique_name]?.mastery_rank ?? 0;
      const cap = effectiveCap(w);
      const isMastered = cap > 0 && rank >= cap;

      total++;
      if (isMastered) mastered++;
      if (unmasteredOnly && isMastered) continue;
      if (q && !w.name.toLowerCase().includes(q)) continue;

      const g = weaponGroup(w);
      const list = byGroup.get(g) ?? [];
      list.push({ item: w, rank });
      byGroup.set(g, list);
    }

    // Sort items within each group alphabetically
    for (const list of byGroup.values()) list.sort((a, b) => a.item.name.localeCompare(b.item.name));

    // Build ordered group array
    const ordered: { group: string; entries: { item: WeaponItem; rank: number }[] }[] = [];
    for (const g of GROUP_ORDER) {
      if (byGroup.has(g)) ordered.push({ group: g, entries: byGroup.get(g)! });
    }
    for (const [g, entries] of byGroup) {
      if (!GROUP_ORDER.includes(g)) ordered.push({ group: g, entries });
    }

    return { groups: ordered, masteredCount: mastered, totalCount: total };
  }, [allWeapons, activeTab, inventory, search, unmasteredOnly]);

  const isFiltered = search !== "" || unmasteredOnly;

  return (
    <div className={WPN_ROOT_CLASS}>
      {/* ── Sub-tab bar ── */}
      <div className={WPN_TABS_CLASS}>
        {WEAPON_TABS.map(tab => (
          <button
            key={tab}
            className={`${WPN_TAB_CLASS} ${activeTab === tab ? WPN_TAB_ACTIVE_CLASS : WPN_TAB_IDLE_CLASS}`}
            onClick={() => onTabChange(tab)}
          >
            {tab}
          </button>
        ))}
      </div>

      {/* ── Toolbar ── */}
      <div className={WPN_TOOLBAR_CLASS}>
        <input
          ref={inputRef}
          className={WPN_SEARCH_CLASS}
          placeholder="Search…"
          value={search}
          onChange={e => setSearch(e.target.value)}
        />
        <div className={WPN_PROGRESS_WRAP_CLASS}>
          <div className={WPN_PROGRESS_BAR_CLASS}>
            <div
              className={WPN_PROGRESS_FILL_CLASS}
              style={{ width: totalCount > 0 ? `${(masteredCount / totalCount) * 100}%` : "0%" }}
            />
          </div>
          <span className={WPN_PROGRESS_LABEL_CLASS}>{masteredCount} / {totalCount} mastered</span>
        </div>
        <button
          className={`${WPN_FILTER_CLASS} ${unmasteredOnly ? WPN_FILTER_ACTIVE_CLASS : WPN_FILTER_IDLE_CLASS}`}
          onClick={() => setUnmasteredOnly(v => !v)}
        >
          Unmastered only
        </button>
        {isFiltered && (
          <FilterChip reset onClick={() => { setSearch(""); setUnmasteredOnly(false); }}>
            Show All
          </FilterChip>
        )}
      </div>

      {/* ── Item list ── */}
      <div className={WPN_BODY_CLASS}>
        {loading && <div className={WPN_EMPTY_CLASS}>Loading weapons…</div>}
        {!loading && groups.length === 0 && (
          <div className={WPN_EMPTY_CLASS}>
            {unmasteredOnly ? "All weapons mastered — nice!" : "No weapons found."}
          </div>
        )}
        {groups.map(({ group, entries }) => (
          <div key={group} className="mb-5">
            <div className={WPN_HEADER_CLASS}>{group}</div>
            <div className={WPN_GRID_CLASS}>
              {entries.map(({ item, rank }) => (
                <WeaponRow key={item.unique_name} item={item} rank={rank} />
              ))}
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}
