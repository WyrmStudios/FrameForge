import { useState, useEffect, useMemo } from "react";
import { invoke } from "@tauri-apps/api/core";
import ItemImg from "../ItemImg";
import type { InventoryItem } from "../types/items";
import type { SyndicateFilters } from "../types/filters";
import type { SyndicateItem, SyndicateStore } from "../types/syndicates";
import { FilterChip } from "../shared/ui/FilterControls";

// ── Completion status ─────────────────────────────────────────────────────────
// "complete"  = built/owned final item (or mod/sigil in inventory)
// "blueprint" = have the blueprint but haven't built it yet
// "subsumed"  = warframe was consumed by Helminth (Infested Foundry)
// "none"      = not owned at all

type CompStatus = "complete" | "blueprint" | "subsumed" | "none";

function itemStatus(item: SyndicateItem, inventory: Record<string, InventoryItem>): CompStatus {
  const qty = inventory[item.unique_name]?.quantity ?? item.owned;
  if (item.result_unique) {
    // This is a blueprint — check if the crafted item is built (path alias lookup)
    const resultItem = inventory[item.result_unique];
    if (resultItem?.subsumed) return "subsumed";
    const resultQty = resultItem?.quantity ?? item.result_owned;
    if (resultQty > 0) return "complete";
    if (qty > 0) return "blueprint";
    return "none";
  }
  // Mod, sigil, specter, part — directly owned
  if (inventory[item.unique_name]?.subsumed) return "subsumed";
  return qty > 0 ? "complete" : "none";
}

// ── Syndicate metadata ────────────────────────────────────────────────────────

type SynGroup = "main" | "openworld" | "other" | "lab";

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
  // ── Clan dojo research labs ──
  // tierOrder uses raw WFCD category names. Unknown categories append after the list.
  "Bio Lab":            { color: "#66bb6a", short: "Bio",      group: "lab", tierOrder: ["Primary", "Secondary", "Melee", "Companions", "Resources", "Misc"] },
  "Chem Lab":           { color: "#ffa726", short: "Chem",     group: "lab", tierOrder: ["Primary", "Secondary", "Melee", "Resources", "Misc"] },
  "Energy Lab":         { color: "#42a5f5", short: "Energy",   group: "lab", tierOrder: ["Primary", "Secondary", "Melee", "Companions", "Resources", "Misc"] },
  "Tenno Lab":          { color: "#ab47bc", short: "Tenno",    group: "lab", tierOrder: ["Warframes", "Archwing", "Parts", "Primary", "Secondary", "Melee", "Resources", "Misc", "Blueprints"] },
  "Orokin Lab":         { color: "#c8a951", short: "Orokin",   group: "lab", tierOrder: ["Misc"] },
  "Ventkids Bash Lab":  { color: "#8d6e63", short: "Ventkids", group: "lab", tierOrder: ["Warframes", "Parts", "Melee", "Blueprints", "Misc"] },
  "Dry Docks":          { color: "#78909c", short: "Dry Dock", group: "lab", tierOrder: ["Misc"] },
  "Dagath's Hollow":    { color: "#7e57c2", short: "Dagath",   group: "lab", tierOrder: ["Warframes", "Parts", "Melee", "Blueprints"] },
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
  lab:       "Research Labs",
};

// ── Presentation ──────────────────────────────────────────────────────────────

const SYN_ROOT_CLASS = "flex flex-1 min-h-0 flex-col overflow-hidden bg-background text-foreground";
const SYN_GROUPS_CLASS = "flex shrink-0 gap-0.5 border-b border-border px-3 pt-2 pb-1";
const SYN_GROUP_BTN_CLASS = "cursor-pointer rounded-4 border px-3.5 py-1 text-12 font-medium transition-all duration-150";
const SYN_GROUP_IDLE_CLASS = "border-border bg-transparent text-dim hover:bg-[var(--hover)] hover:text-foreground";
const SYN_GROUP_ACTIVE_CLASS = "border-accent bg-[var(--bg-card)] text-foreground";
const SYN_TABS_CLASS = "flex shrink-0 flex-wrap gap-0.5 border-b border-border px-3 pt-1.5";
const SYN_TAB_CLASS = "-mb-px cursor-pointer rounded-t-6 border-0 border-b-3 px-3.5 py-1.5 text-13 font-medium transition-[background,color] duration-150";
const SYN_TAB_IDLE_CLASS = "border-transparent bg-transparent text-dim hover:bg-[var(--hover)] hover:text-foreground";
const SYN_TAB_ACTIVE_CLASS = "border-[var(--syn-color,#888)] bg-[var(--bg-card)] text-foreground";
const SYN_TOOLBAR_CLASS = "flex shrink-0 items-center gap-3 px-3.5 pb-1.5 pt-2.5";
const SYN_SEARCH_CLASS = "w-40 shrink-0 rounded-5 border border-border bg-[var(--bg-card)] px-2 py-1 text-12 text-foreground placeholder:text-dim focus:border-accent focus:outline-none";
const SYN_PROGRESS_WRAP_CLASS = "flex flex-1 items-center gap-2";
const SYN_PROGRESS_BAR_CLASS = "h-1.5 max-w-50 flex-1 overflow-hidden rounded-3 bg-border";
const SYN_PROGRESS_FILL_CLASS = "h-full rounded-3 bg-[var(--syn-color,#888)] transition-[width] duration-300";
const SYN_PROGRESS_LABEL_CLASS = "whitespace-nowrap text-12 text-dim";
const SYN_FILTER_CLASS = "cursor-pointer rounded-5 border px-2.5 py-1 text-12 transition-all duration-150";
const SYN_FILTER_IDLE_CLASS = "border-border bg-transparent text-dim";
const SYN_FILTER_ACTIVE_CLASS = "border-[var(--syn-color,#888)] bg-[var(--syn-color,#888)] text-white";
const SYN_BODY_CLASS = "flex-1 overflow-y-auto px-3 pb-4 pt-2";
const SYN_TIER_GROUP_CLASS = "mb-5";
const SYN_TIER_HEADER_CLASS = "mb-1.5 border-b border-border pb-1.5 pt-1 text-11 font-semibold uppercase tracking-0.06 text-dim";
const SYN_GRID_CLASS = "grid grid-cols-[repeat(auto-fill,minmax(220px,1fr))] gap-1";
const SYN_ITEM_CLASS = "flex items-center gap-2.5 rounded-5 border px-2 py-1.5 transition-[background,border-color] duration-100 hover:bg-[var(--hover)]";
const SYN_ITEM_ROW_CLASS: Record<CompStatus, string> = {
  complete:  "border-complete/20 hover:border-complete/20",
  blueprint: "border-ducat/20 hover:border-ducat/20",
  subsumed:  "border-subsumed/20 hover:border-subsumed/20",
  none:      "border-transparent hover:border-border",
};
const SYN_ITEM_INFO_CLASS = "min-w-0 flex-1";
const SYN_ITEM_NAME_CLASS = "truncate text-13";
const SYN_ITEM_CAT_CLASS = "mt-px text-11 text-dim";
const SYN_STATUS_BASE_CLASS = "min-w-7 shrink-0 rounded-4 px-1.75 py-0.5 text-center text-11 font-bold";
const SYN_STATUS_CLASS: Record<CompStatus, string> = {
  complete:  "bg-complete/15 text-complete",
  blueprint: "bg-ducat/15 text-ducat",
  subsumed:  "bg-subsumed/12 text-subsumed",
  none:      "bg-rank-none/8 text-dim",
};
const SYN_EMPTY_CLASS = "py-10 text-center text-14 text-dim";
const SYN_LOADING_CLASS = "py-15 text-center text-14 text-dim";

// ── Status badge ─────────────────────────────────────────────────────────────

function StatusBadge({ status }: { status: CompStatus }) {
  if (status === "complete")  return <span className={`${SYN_STATUS_BASE_CLASS} ${SYN_STATUS_CLASS.complete}`}>✓</span>;
  if (status === "blueprint") return <span className={`${SYN_STATUS_BASE_CLASS} ${SYN_STATUS_CLASS.blueprint}`}>BP</span>;
  if (status === "subsumed")  return <span className={`${SYN_STATUS_BASE_CLASS} ${SYN_STATUS_CLASS.subsumed}`} title="Consumed by Helminth">H</span>;
  return <span className={`${SYN_STATUS_BASE_CLASS} ${SYN_STATUS_CLASS.none}`}>—</span>;
}

// ── Main component ────────────────────────────────────────────────────────────

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
    Promise.all([
      invoke<SyndicateStore[]>("get_syndicate_stores"),
      invoke<SyndicateStore[]>("get_research_lab_stores"),
    ]).then(([syn, labs]) => {
      setStores([...syn, ...labs]);
      setLoading(false);
    }).catch(() => setLoading(false));
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
    <div className={SYN_ROOT_CLASS}>
      {/* ── Group selector ── */}
      <div className={SYN_GROUPS_CLASS}>
        {(["main", "openworld", "other", "lab"] as SynGroup[]).map(g => (
          <button
            key={g}
            className={`${SYN_GROUP_BTN_CLASS} ${activeGroup === g ? SYN_GROUP_ACTIVE_CLASS : SYN_GROUP_IDLE_CLASS}`}
            onClick={() => handleGroupChange(g)}
          >
            {GROUP_LABELS[g]}
          </button>
        ))}
      </div>

      {/* ── Syndicate tabs ── */}
      <div className={SYN_TABS_CLASS}>
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
              className={`${SYN_TAB_CLASS} ${activeTab === store.name ? SYN_TAB_ACTIVE_CLASS : SYN_TAB_IDLE_CLASS}`}
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
      <div className={SYN_TOOLBAR_CLASS} style={{ ["--syn-color" as string]: meta?.color } as React.CSSProperties}>
        <input
          className={SYN_SEARCH_CLASS}
          placeholder="Search items…"
          value={search}
          onChange={e => set("search", e.target.value)}
        />
        <div className={SYN_PROGRESS_WRAP_CLASS}>
          <div className={SYN_PROGRESS_BAR_CLASS}>
            <div
              className={SYN_PROGRESS_FILL_CLASS}
              style={{ width: totalCount > 0 ? `${(ownedCount / totalCount) * 100}%` : "0%" }}
            />
          </div>
          <span className={SYN_PROGRESS_LABEL_CLASS}>{ownedCount} / {totalCount} complete</span>
        </div>
        <button
          className={`${SYN_FILTER_CLASS} ${missingOnly ? SYN_FILTER_ACTIVE_CLASS : SYN_FILTER_IDLE_CLASS}`}
          style={missingOnly ? { ["--syn-color" as string]: meta?.color } as React.CSSProperties : undefined}
          onClick={() => set("missingOnly", !missingOnly)}
        >
          Missing only
        </button>
        {isFiltered && (
          <FilterChip reset onClick={() => onFiltersChange({ ...filters, missingOnly: false, search: "" })}>
            Show All
          </FilterChip>
        )}
      </div>

      {/* ── Item list ── */}
      <div className={SYN_BODY_CLASS}>
        {loading && <div className={SYN_LOADING_CLASS}>Loading syndicate data…</div>}
        {!loading && groupSyndicates.length === 0 && (
          <div className={SYN_EMPTY_CLASS}>No data yet. Refresh the item database from Settings.</div>
        )}
        {!loading && groupSyndicates.length > 0 && tierGroups.length === 0 && (
          <div className={SYN_EMPTY_CLASS}>
            {missingOnly ? "Nothing missing — all items complete!" : "No items for this syndicate."}
          </div>
        )}
        {tierGroups.map(({ tier, items }) => {
          const tierComplete = items.filter(i => itemStatus(i, inventory) === "complete").length;
          return (
            <div key={tier} className={SYN_TIER_GROUP_CLASS}>
              <div className={SYN_TIER_HEADER_CLASS}>
                {tier || "General"}
                <span className="ml-1.5 font-normal text-dim">
                  — {tierComplete}/{items.length}
                </span>
              </div>
              <div className={SYN_GRID_CLASS}>
                {items.map(item => {
                  const status = itemStatus(item, inventory);
                  return (
                    <div
                      key={item.unique_name}
                      className={`${SYN_ITEM_CLASS} ${SYN_ITEM_ROW_CLASS[status]}${status === "none" ? " opacity-50" : ""}`}
                    >
                      <ItemImg imageName={item.image_name} category={item.category} />
                      <div className={SYN_ITEM_INFO_CLASS}>
                        <div className={SYN_ITEM_NAME_CLASS}>{item.name}</div>
                        <div className={SYN_ITEM_CAT_CLASS}>{item.category}</div>
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
