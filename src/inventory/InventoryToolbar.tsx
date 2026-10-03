import type { Dispatch, SetStateAction } from "react";
import { HelpTip } from "../shared/HelpTip";
import type { InventoryFilters } from "../types/filters";
import SearchBar from "../shared/SearchBar";
import { ViewToggle } from "../shared/ViewToggle";
import type { ViewMode } from "../types/ui";
import type { FilterPresetModule, FilterPresetSettings } from "../types/filterPresets";
import FilterPresets from "../shared/FilterPresets";
import { FilterBar, FilterChip, FilterLabel, FilterSeparator } from "../shared/ui/FilterControls";

const TOOLBAR = "flex items-center gap-[12px] px-[16px] py-[10px] border-b border-border shrink-0";
const ITEM_COUNT_LABEL = "text-muted text-[11px] whitespace-nowrap";
const IMAGE_TOGGLE =
  "inventory-image-toggle flex items-center gap-[5px] shrink-0 text-muted cursor-pointer text-[11px] whitespace-nowrap hover:text-foreground";
const CTRL_WRAP =
  "flex items-center shrink-0 h-[25px] overflow-hidden border border-border rounded-[5px] text-muted text-[10px] tabular-nums";
const CTRL_SPAN = "min-w-[42px] text-center";
const CTRL_BTN =
  "self-stretch w-[24px] border-0 bg-[rgba(255,255,255,.03)] text-muted cursor-pointer text-[15px] leading-none hover:enabled:bg-[rgba(255,255,255,.08)] hover:enabled:text-foreground disabled:opacity-35 disabled:cursor-default";

interface InventoryToolbarProps {
  filters: InventoryFilters;
  onFiltersChange: Dispatch<SetStateAction<InventoryFilters>>;
  onToggleRecent: () => void;
  onTogglePlat: () => void;
  onToggleDucats: () => void;
  availableRanks: number[];
  showRankFilters: boolean;
  itemCount: number;
  view: ViewMode;
  onViewChange: (view: ViewMode) => void;
  cardColumns: number;
  onCardColumnsChange: (columns: number) => void;
  listTextScale: number;
  onListTextScaleChange: (scale: number) => void;
  filterPresets: FilterPresetSettings;
  onFilterPresetsChange: Dispatch<SetStateAction<FilterPresetSettings>>;
  onOpenSettings: (module: FilterPresetModule) => void;
}

export default function InventoryToolbar({
  filters, onFiltersChange, onToggleRecent, onTogglePlat, onToggleDucats, availableRanks, showRankFilters, itemCount, view, onViewChange, cardColumns, onCardColumnsChange, listTextScale, onListTextScaleChange, filterPresets, onFilterPresetsChange, onOpenSettings,
}: InventoryToolbarProps) {
  const { search, filterOwned, filterRecent, filterPrime, filterVaulted, filterUnvaulted, filterTradeable, filterPlat, filterDucats, filterRank, sortMode } = filters;
  const isCardView = view === "cards" || view === "text-cards";
  const isListView = view === "list" || view === "list-compact";
  const imagesVisible = view === "cards" || view === "list";
  return (
    <>
      <div className={TOOLBAR}>
        <SearchBar
          placeholder="Search items (comma-separated)…"
          value={search}
          onChange={search => onFiltersChange(previous => ({ ...previous, search }))}
        />
      </div>
      <FilterBar>
        <FilterChip active={filterOwned} onClick={() => onFiltersChange(previous => ({ ...previous, filterOwned: !previous.filterOwned }))}>Owned</FilterChip>
        <FilterChip active={filterRecent} onClick={onToggleRecent}>Changed recently</FilterChip>
        <FilterChip active={filterPrime} onClick={() => onFiltersChange(previous => ({ ...previous, filterPrime: !previous.filterPrime }))}>Prime</FilterChip>
        <FilterChip active={filterVaulted} onClick={() => onFiltersChange(previous => ({ ...previous, filterVaulted: !previous.filterVaulted }))}>🔒 Vaulted</FilterChip>
        <FilterChip active={filterUnvaulted} onClick={() => onFiltersChange(previous => ({ ...previous, filterUnvaulted: !previous.filterUnvaulted }))}>🔓 Unvaulted</FilterChip>
        <FilterChip active={filterTradeable} aria-pressed={filterTradeable} onClick={() => onFiltersChange(previous => ({ ...previous, filterTradeable: !previous.filterTradeable }))}>Tradeable</FilterChip>
        <FilterChip active={filterPlat} aria-pressed={filterPlat} onClick={onTogglePlat}>
          Plat{sortMode === "plat-desc" ? " ↓" : sortMode === "plat-asc" ? " ↑" : ""}
        </FilterChip>
        <FilterChip active={filterDucats} aria-pressed={filterDucats} onClick={onToggleDucats}>
          Ducats{sortMode === "ducat-desc" ? " ↓" : sortMode === "ducat-asc" ? " ↑" : ""}
        </FilterChip>
        {showRankFilters && <>
          <FilterSeparator />
          <FilterLabel>Rank:</FilterLabel>
          <FilterChip active={filterRank === "unranked"} onClick={() => onFiltersChange(previous => ({ ...previous, filterRank: previous.filterRank === "unranked" ? null : "unranked" }))}>Unranked</FilterChip>
          {availableRanks.map(rank => (
            <FilterChip key={rank} active={filterRank === rank} onClick={() => onFiltersChange(previous => ({ ...previous, filterRank: previous.filterRank === rank ? null : rank }))}>R{rank}</FilterChip>
          ))}
        </>}
        <FilterSeparator />
        <FilterPresets module="inventory" {...{ filters, onFiltersChange, filterPresets, onFilterPresetsChange, onOpenSettings }} />
        <FilterSeparator />
        <FilterLabel>Sort:</FilterLabel>
        <FilterChip active={sortMode === "qty-desc" || sortMode === "qty-asc"} onClick={() => onFiltersChange(previous => ({ ...previous, sortMode: previous.sortMode === "qty-desc" ? "qty-asc" : "qty-desc" }))}>Qty {sortMode === "qty-asc" ? "↑" : "↓"}</FilterChip>
        <FilterChip active={sortMode === "name-asc" || sortMode === "name-desc"} onClick={() => onFiltersChange(previous => ({ ...previous, sortMode: previous.sortMode === "name-asc" ? "name-desc" : "name-asc" }))}>{sortMode === "name-desc" ? "Z-A" : "A-Z"}</FilterChip>
        <span className={`${ITEM_COUNT_LABEL} ml-auto`}>{itemCount} item{itemCount !== 1 ? "s" : ""}{itemCount === 1000 ? " (capped)" : ""}</span>
        <ViewToggle
          view={view === "text-cards" ? "cards" : view === "list-compact" ? "list" : view}
          onChange={onViewChange}
          modes={["cards", "icons", "list"]}
        />
        {(isCardView || isListView) && (
          <>
            <label className={IMAGE_TOGGLE}>
              <input type="checkbox" checked={imagesVisible}
                onChange={event => onViewChange(isCardView
                  ? (event.target.checked ? "cards" : "text-cards")
                  : (event.target.checked ? "list" : "list-compact"))} />
              Images
            </label>
            {isCardView && (
              <div className={CTRL_WRAP} aria-label="Maximum card columns">
                <button title="Fewer columns" aria-label="Fewer columns" className={CTRL_BTN} disabled={cardColumns <= 5}
                  onClick={() => onCardColumnsChange(cardColumns - 1)}>−</button>
                <span className={CTRL_SPAN}>{cardColumns} cols</span>
                <button title="More columns" aria-label="More columns" className={CTRL_BTN} disabled={cardColumns >= 24}
                  onClick={() => onCardColumnsChange(cardColumns + 1)}>+</button>
              </div>
            )}
            {isListView && (
              <div className={CTRL_WRAP} aria-label="List text size">
                <button title="Smaller text" aria-label="Smaller text" className={CTRL_BTN} disabled={listTextScale <= 80}
                  onClick={() => onListTextScaleChange(listTextScale - 10)}>−</button>
                <span className={CTRL_SPAN}>{listTextScale}%</span>
                <button title="Larger text" aria-label="Larger text" className={CTRL_BTN} disabled={listTextScale >= 150}
                  onClick={() => onListTextScaleChange(listTextScale + 10)}>+</button>
              </div>
            )}
          </>
        )}
        <HelpTip items={[
          { icon: "★", label: "★  Mastered", desc: "Shown above image — item levelled to rank 30" },
          { icon: "R5", label: "R{n}  Rank", desc: "Shown above image — current rank, not yet mastered" },
          { icon: "⚒", label: "⚒  Building", desc: "Shown on image — currently crafting in Foundry" },
          { swatch: "rgba(63,185,80,.5)", label: "Green border", desc: "Item recently gained" },
          { swatch: "rgba(248,81,73,.5)", label: "Red border", desc: "Item recently lost or consumed" },
        ]} />
      </FilterBar>
    </>
  );
}
