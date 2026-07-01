import { useState, useEffect, useMemo } from "react";
import { invoke } from "@tauri-apps/api/core";
import "./Syndicates.css";
import type { InventoryItem } from "./App";

// ── Types ────────────────────────────────────────────────────────────────────

interface SyndicateItem {
  unique_name: string;
  name: string;
  category: string;
  image_name?: string;
  tier: string;
  ducats?: number;
  owned: number;
  result_unique?: string;
  result_owned: number;
}

interface SyndicateStore {
  name: string;
  items: SyndicateItem[];
}

// ── Completion status ─────────────────────────────────────────────────────────
// "complete"  = built/owned final item (or mod/sigil in inventory)
// "blueprint" = have the blueprint but haven't built it yet
// "none"      = not owned at all

type CompStatus = "complete" | "blueprint" | "none";

function itemStatus(item: SyndicateItem, inventory: Record<string, InventoryItem>): CompStatus {
  const qty = inventory[item.unique_name]?.quantity ?? item.owned;
  if (item.result_unique) {
    // This is a blueprint — check if the crafted item is built (path alias lookup)
    const resultQty = inventory[item.result_unique]?.quantity ?? item.result_owned;
    if (resultQty > 0) return "complete";
    if (qty > 0) return "blueprint";
    return "none";
  }
  // Mod, sigil, specter, part — directly owned
  return qty > 0 ? "complete" : "none";
}

// ── Syndicate metadata ────────────────────────────────────────────────────────

type SynGroup = "main" | "openworld" | "other";

interface SynMeta {
  color: string;
  short: string;
  group: SynGroup;
  tierOrder: string[];
}

const SYNDICATE_META: Record<string, SynMeta> = {
  // ── Main faction syndicates ──
  "Steel Meridian":      { color: "#c43434", short: "Meridian",  group: "main",      tierOrder: ["Brave", "Valiant", "Defender", "Protector", "General"] },
  "Arbiters of Hexis":   { color: "#d4a017", short: "Arbiters",  group: "main",      tierOrder: ["Principled", "Authentic", "Lawful", "Crusader", "Maxim"] },
  "Cephalon Suda":       { color: "#00b5cc", short: "Suda",      group: "main",      tierOrder: ["Competent", "Intriguing", "Intelligent", "Wise", "Genius"] },
  "The Perrin Sequence": { color: "#3cb371", short: "Perrin",    group: "main",      tierOrder: ["Associate", "Senior Associate", "Executive", "Senior Executive", "Partner"] },
  "Red Veil":            { color: "#9e1515", short: "Red Veil",  group: "main",      tierOrder: ["Respected", "Honored", "Esteemed", "Revered", "Exalted"] },
  "New Loka":            { color: "#6dbf67", short: "New Loka",  group: "main",      tierOrder: ["Humane", "Bountiful", "Benevolent", "Pure", "Flawless", "Exalted"] },
  // ── Open world syndicates ──
  "Ostron":              { color: "#d4890a", short: "Ostron",    group: "openworld", tierOrder: ["Neutral", "Offworlder", "Visitor", "Trusted", "Surah", "Kin"] },
  "Solaris United":      { color: "#4fc3f7", short: "Solaris",   group: "openworld", tierOrder: ["Neutral", "Outworlder", "Rapscallion", "Doer", "Cove", "Old Mate"] },
  "Entrati":             { color: "#9b59b6", short: "Entrati",   group: "openworld", tierOrder: ["Neutral", "Stranger", "Acquaintance", "Associate", "Friend", "Family"] },
  "Necraloid":           { color: "#6c3483", short: "Necraloid", group: "openworld", tierOrder: ["Clearance Agnesis", "Clearance Modus", "Clearance Odima"] },
  "The Holdfasts":       { color: "#f39c12", short: "Holdfasts", group: "openworld", tierOrder: ["Neutral", "Fallen", "Watcher", "Guardian", "Seraph", "Angel"] },
  "Kahl's Garrison":     { color: "#7f8c8d", short: "Garrison",  group: "openworld", tierOrder: ["Encampment", "Fort", "Settlement", "Home"] },
  "Cavia":               { color: "#e91e8c", short: "Cavia",     group: "openworld", tierOrder: [] },
  // ── Sub-syndicates & others ──
  "The Quills":          { color: "#ecf0f1", short: "Quills",    group: "other",     tierOrder: ["Neutral", "Mote", "Observer", "Adherent", "Instrument", "Architect"] },
  "Vox Solaris":         { color: "#26a69a", short: "Vox Sol.",  group: "other",     tierOrder: ["Neutral", "Operative", "Agent", "Hand", "Instrument", "Shadow"] },
  "Ventkids":            { color: "#e74c3c", short: "Ventkids",  group: "other",     tierOrder: ["Neutral", "Glinty", "Whozit", "Proper Felon", "Primo", "Logical"] },
  "Cephalon Simaris":    { color: "#e67e22", short: "Simaris",   group: "other",     tierOrder: [] },
  "Conclave":            { color: "#c0392b", short: "Conclave",  group: "other",     tierOrder: ["Mistral", "Whirlwind", "Tempest", "Hurricane", "Typhoon"] },
  "Operational Supply":  { color: "#607d8b", short: "Op Supply", group: "other",     tierOrder: ["Neutral", "Collaborator", "Defender", "Champion"] },
};

const GROUP_LABELS: Record<SynGroup, string> = {
  main:      "Main Syndicates",
  openworld: "Open World",
  other:     "Other",
};

// ── Image component ───────────────────────────────────────────────────────────

function SynItemImg({ imageName, category }: { imageName?: string; category: string }) {
  const [failed, setFailed] = useState(false);
  if (!imageName || failed) {
    return (
      <div className="syn-item-img-fallback">
        {category[0]?.toUpperCase() ?? "?"}
      </div>
    );
  }
  return (
    <img
      className="syn-item-img"
      src={`https://cdn.warframestat.us/img/${imageName}`}
      alt=""
      loading="lazy"
      onError={() => setFailed(true)}
    />
  );
}

// ── Status badge ─────────────────────────────────────────────────────────────

function StatusBadge({ status }: { status: CompStatus }) {
  if (status === "complete")  return <span className="syn-item-status status-complete">✓</span>;
  if (status === "blueprint") return <span className="syn-item-status status-blueprint">BP</span>;
  return <span className="syn-item-status status-none">—</span>;
}

// ── Main component ────────────────────────────────────────────────────────────

export interface SyndicateFilters {
  activeGroup: SynGroup; activeTab: string; missingOnly: boolean; search: string;
}
export const SYNDICATE_FILTERS_DEFAULT: SyndicateFilters = {
  activeGroup: "main", activeTab: "Steel Meridian", missingOnly: false, search: "",
};

interface Props {
  inventory: Record<string, InventoryItem>;
  filters: SyndicateFilters;
  onFiltersChange: (f: SyndicateFilters) => void;
}

export default function Syndicates({ inventory, filters, onFiltersChange }: Props) {
  const [stores, setStores] = useState<SyndicateStore[]>([]);
  const [loading, setLoading] = useState(true);

  const { activeGroup, activeTab, missingOnly, search } = filters;
  const set = <K extends keyof SyndicateFilters>(k: K, v: SyndicateFilters[K]) => onFiltersChange({ ...filters, [k]: v });
  const isFiltered = search !== "" || missingOnly;

  useEffect(() => {
    invoke<SyndicateStore[]>("get_syndicate_stores")
      .then(s => { setStores(s); setLoading(false); })
      .catch(() => setLoading(false));
  }, []);

  // Syndicates visible in current group
  const groupSyndicates = useMemo(() =>
    stores.filter(s => (SYNDICATE_META[s.name]?.group ?? "other") === activeGroup),
    [stores, activeGroup]
  );

  const handleGroupChange = (g: SynGroup) => {
    const first = stores.find(s => (SYNDICATE_META[s.name]?.group ?? "other") === g);
    onFiltersChange({ ...filters, activeGroup: g, search: "", activeTab: first?.name ?? filters.activeTab });
  };

  const handleTabChange = (name: string) => {
    onFiltersChange({ ...filters, activeTab: name, search: "" });
  };

  const activeStore = useMemo(() => {
    const store = stores.find(s => s.name === activeTab);
    if (!store) return null;
    return {
      ...store,
      items: store.items.map(item => ({
        ...item,
        owned: inventory[item.unique_name]?.quantity ?? item.owned,
        result_owned: item.result_unique
          ? (inventory[item.result_unique]?.quantity ?? item.result_owned)
          : item.result_owned,
      })),
    };
  }, [stores, activeTab, inventory]);

  const meta = SYNDICATE_META[activeTab];

  const { ownedCount, totalCount } = useMemo(() => {
    if (!activeStore) return { ownedCount: 0, totalCount: 0 };
    return {
      ownedCount: activeStore.items.filter(i => itemStatus(i, inventory) === "complete").length,
      totalCount: activeStore.items.length,
    };
  }, [activeStore, inventory]);

  // Group items by tier/vendor, preserving defined tier order
  const tierGroups = useMemo(() => {
    // Guard: if the active tab's syndicate is not in the visible group, don't render stale items
    if (!activeStore || !groupSyndicates.find(s => s.name === activeTab)) return [];
    const tierOrder = meta?.tierOrder ?? [];
    const q = search.toLowerCase();
    const groups = new Map<string, SyndicateItem[]>();
    for (const item of activeStore.items) {
      if (missingOnly && itemStatus(item, inventory) === "complete") continue;
      if (q && !item.name.toLowerCase().includes(q) && !item.category.toLowerCase().includes(q)) continue;
      const list = groups.get(item.tier) ?? [];
      list.push(item);
      groups.set(item.tier, list);
    }
    for (const [, list] of groups) list.sort((a, b) => a.name.localeCompare(b.name));
    const ordered: { tier: string; items: SyndicateItem[] }[] = [];
    for (const tier of tierOrder) {
      if (groups.has(tier)) ordered.push({ tier, items: groups.get(tier)! });
    }
    for (const [tier, items] of groups) {
      if (!tierOrder.includes(tier)) ordered.push({ tier, items });
    }
    return ordered;
  }, [activeStore, groupSyndicates, activeTab, missingOnly, meta, inventory, search]);

  return (
    <div className="syn-root">
      {/* ── Group selector ── */}
      <div className="syn-groups">
        {(["main", "openworld", "other"] as SynGroup[]).map(g => (
          <button
            key={g}
            className={`syn-group-btn ${activeGroup === g ? "active" : ""}`}
            onClick={() => handleGroupChange(g)}
          >
            {GROUP_LABELS[g]}
          </button>
        ))}
      </div>

      {/* ── Syndicate tabs ── */}
      <div className="syn-tabs">
        {groupSyndicates.map(store => {
          const m = SYNDICATE_META[store.name];
          const owned = store.items.filter(i => itemStatus({
            ...i,
            owned: inventory[i.unique_name]?.quantity ?? i.owned,
            result_owned: i.result_unique ? (inventory[i.result_unique]?.quantity ?? i.result_owned) : i.result_owned,
          }, inventory) === "complete").length;
          return (
            <button
              key={store.name}
              className={`syn-tab ${activeTab === store.name ? "active" : ""}`}
              style={{ ["--syn-color" as string]: m?.color ?? "#888" } as React.CSSProperties}
              onClick={() => handleTabChange(store.name)}
              title={`${store.name} — ${owned}/${store.items.length}`}
            >
              {m?.short ?? store.name}
            </button>
          );
        })}
      </div>

      {/* ── Toolbar ── */}
      <div className="syn-toolbar" style={{ ["--syn-color" as string]: meta?.color } as React.CSSProperties}>
        <input
          className="syn-search"
          placeholder="Search items…"
          value={search}
          onChange={e => set("search", e.target.value)}
        />
        <div className="syn-progress-wrap">
          <div className="syn-progress-bar">
            <div
              className="syn-progress-fill"
              style={{ width: totalCount > 0 ? `${(ownedCount / totalCount) * 100}%` : "0%" }}
            />
          </div>
          <span className="syn-progress-label">{ownedCount} / {totalCount} complete</span>
        </div>
        <button
          className={`syn-filter-btn ${missingOnly ? "active" : ""}`}
          style={missingOnly ? { ["--syn-color" as string]: meta?.color } as React.CSSProperties : undefined}
          onClick={() => set("missingOnly", !missingOnly)}
        >
          Missing only
        </button>
        {isFiltered && (
          <button className="fchip fchip-reset" onClick={() => onFiltersChange({ ...filters, missingOnly: false, search: "" })}>
            Show All
          </button>
        )}
      </div>

      {/* ── Item list ── */}
      <div className="syn-body">
        {loading && <div className="syn-loading">Loading syndicate data…</div>}
        {!loading && groupSyndicates.length === 0 && (
          <div className="syn-empty">No data yet. Refresh the item database from Settings.</div>
        )}
        {!loading && groupSyndicates.length > 0 && tierGroups.length === 0 && (
          <div className="syn-empty">
            {missingOnly ? "Nothing missing — all items complete!" : "No items for this syndicate."}
          </div>
        )}
        {tierGroups.map(({ tier, items }) => {
          const tierComplete = items.filter(i => itemStatus(i, inventory) === "complete").length;
          return (
            <div key={tier} className="syn-tier-group">
              <div className="syn-tier-header">
                {tier || "General"}
                <span style={{ color: "var(--text-dim)", fontWeight: 400, marginLeft: 6 }}>
                  — {tierComplete}/{items.length}
                </span>
              </div>
              <div className="syn-items-grid">
                {items.map(item => {
                  const status = itemStatus(item, inventory);
                  return (
                    <div
                      key={item.unique_name}
                      className={`syn-item ${status === "none" ? "missing" : ""} status-row-${status}`}
                    >
                      <SynItemImg imageName={item.image_name} category={item.category} />
                      <div className="syn-item-info">
                        <div className="syn-item-name">{item.name}</div>
                        <div className="syn-item-cat">{item.category}</div>
                      </div>
                      <StatusBadge status={status} />
                    </div>
                  );
                })}
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
}
