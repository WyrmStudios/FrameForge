import { useState, useEffect, useMemo, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useImgLadder, cdnUrl } from "./ImgCacheDir";
import "./Weapons.css";
import type { InventoryItem } from "./App";

// ── Types ─────────────────────────────────────────────────────────────────────

interface WeaponItem {
  unique_name: string;
  name: string;
  category: string;
  image_name?: string;
  mastery_req?: number;
  max_level_cap?: number;
}

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

// ── Image ─────────────────────────────────────────────────────────────────────

function WeaponImg({ imageName, name }: { imageName?: string; name: string }) {
  const { src, onError } = useImgLadder([cdnUrl(imageName)]);
  if (!src) {
    return <div className="wpn-img-fallback">{name[0]?.toUpperCase() ?? "?"}</div>;
  }
  return <img key={src} className="wpn-img" src={src} alt="" loading="lazy" onError={onError} />;
}

// ── Item row ──────────────────────────────────────────────────────────────────

function WeaponRow({ item, rank }: { item: WeaponItem; rank: number }) {
  const cap = effectiveCap(item);
  const mastered = cap > 0 && rank >= cap;
  const rankLabel = cap > 0 ? `R${rank}/${cap}` : `R${rank}`;

  return (
    <div className={`wpn-item ${mastered ? "wpn-mastered" : rank > 0 ? "wpn-partial" : "wpn-none"}`}>
      <WeaponImg imageName={item.image_name} name={item.name} />
      <span className="wpn-name">{item.name}</span>
      {item.mastery_req != null && item.mastery_req > 0 && (
        <span className="wpn-mr" title={`Mastery Rank ${item.mastery_req} required`}>MR{item.mastery_req}</span>
      )}
      <span className={`wpn-rank ${mastered ? "rank-done" : rank > 0 ? "rank-partial" : "rank-zero"}`}>
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
    <div className="wpn-root">
      {/* ── Sub-tab bar ── */}
      <div className="wpn-tabs">
        {WEAPON_TABS.map(tab => (
          <button
            key={tab}
            className={`wpn-tab ${activeTab === tab ? "active" : ""}`}
            onClick={() => onTabChange(tab)}
          >
            {tab}
          </button>
        ))}
      </div>

      {/* ── Toolbar ── */}
      <div className="wpn-toolbar">
        <input
          ref={inputRef}
          className="wpn-search"
          placeholder="Search…"
          value={search}
          onChange={e => setSearch(e.target.value)}
        />
        <div className="wpn-progress-wrap">
          <div className="wpn-progress-bar">
            <div
              className="wpn-progress-fill"
              style={{ width: totalCount > 0 ? `${(masteredCount / totalCount) * 100}%` : "0%" }}
            />
          </div>
          <span className="wpn-progress-label">{masteredCount} / {totalCount} mastered</span>
        </div>
        <button
          className={`wpn-filter-btn ${unmasteredOnly ? "active" : ""}`}
          onClick={() => setUnmasteredOnly(v => !v)}
        >
          Unmastered only
        </button>
        {isFiltered && (
          <button className="fchip fchip-reset" onClick={() => { setSearch(""); setUnmasteredOnly(false); }}>
            Show All
          </button>
        )}
      </div>

      {/* ── Item list ── */}
      <div className="wpn-body">
        {loading && <div className="wpn-empty">Loading weapons…</div>}
        {!loading && groups.length === 0 && (
          <div className="wpn-empty">
            {unmasteredOnly ? "All weapons mastered — nice!" : "No weapons found."}
          </div>
        )}
        {groups.map(({ group, entries }) => (
          <div key={group} className="wpn-group">
            <div className="wpn-group-header">{group}</div>
            <div className="wpn-group-grid">
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
