import type { Dispatch, SetStateAction } from "react";
import { HelpTip } from "../shared/HelpTip";
import type { InventoryFilters } from "../types/filters";
import SearchBar from "../shared/SearchBar";
import { useDebouncedSearch } from "../hooks/useDebouncedSearch";
import { ViewToggle } from "../shared/ViewToggle";
import type { ViewMode } from "../types/ui";
import type { FilterPresetModule, FilterPresetSettings } from "../types/filterPresets";
import FilterPresets from "../shared/FilterPresets";
import { FilterBar, FilterChip, FilterLabel, FilterSeparator } from "../shared/ui/FilterControls";

const TOOLBAR = "flex items-center gap-3 px-4 py-2.5 border-b border-border shrink-0";
const ITEM_COUNT_LABEL = "text-muted text-11 whitespace-nowrap";
const IMAGE_TOGGLE =
  "group/images flex items-center gap-1.25 shrink-0 text-muted cursor-pointer text-11 whitespace-nowrap hover:text-foreground";
const IMAGE_CHECK =
  "appearance-none grid place-content-center size-3.5 shrink-0 cursor-pointer border border-border rounded-3 bg-white/3 " +
  "transition-[border-color,background] duration-120 " +
  "before:content-[''] before:h-0.75 before:w-1.5 before:border-solid before:border-b-2 before:border-l-2 before:border-t-0 before:border-r-0 before:border-white " +
  "before:-rotate-45 before:scale-0 before:transition-transform before:duration-100 " +
  "checked:border-accent checked:bg-accent checked:before:scale-100 group-hover/images:border-accent " +
  "focus-visible:outline-2 focus-visible:outline-[#388bfd73] focus-visible:outline-offset-2";
const CTRL_WRAP =
  "flex items-center shrink-0 h-6.25 overflow-hidden border border-border rounded-5 text-muted text-10 tabular-nums";
const CTRL_SPAN = "min-w-10.5 text-center";
const CTRL_BTN =
  "self-stretch w-6 border-0 bg-white/3 text-muted cursor-pointer text-15 leading-none hover:enabled:bg-white/8 hover:enabled:text-foreground disabled:opacity-35 disabled:cursor-default";

interface InventoryToolbarProps {
  filters: InventoryFilters;
  onFiltersChange: Dispatch<SetStateAction<InventoryFilters>>;
  onToggleRecent: () => void;
  platinumPriceStatus: "loading" | "ready" | "unavailable";
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
  filters, onFiltersChange, onToggleRecent, platinumPriceStatus, availableRanks, showRankFilters, itemCount, view, onViewChange, cardColumns, onCardColumnsChange, listTextScale, onListTextScaleChange, filterPresets, onFilterPresetsChange, onOpenSettings,
}: InventoryToolbarProps) {
  const { search, filterOwned, filterRecent, filterPrime, filterVaulted, filterUnvaulted, filterTradeable, filterRank, sortMode } = filters;
  const [inputSearch, setInputSearch] = useDebouncedSearch(search, value => onFiltersChange(previous => ({ ...previous, search: value })));
  const isCardView = view === "cards" || view === "text-cards";
  const isListView = view === "list" || view === "list-compact";
  const imagesVisible = view === "cards" || view === "list";
  const toggleSort = (primary: InventoryFilters["sortMode"], secondary: InventoryFilters["sortMode"]) =>
    onFiltersChange(previous => ({ ...previous, sortMode: previous.sortMode === primary ? secondary : primary }));
  const platinumSortActive = sortMode === "plat-desc" || sortMode === "plat-asc";
  const ducatRatioSortActive = sortMode === "ducat-ratio-desc" || sortMode === "ducat-ratio-asc";
  const itemCountLabel = (platinumSortActive || ducatRatioSortActive) && platinumPriceStatus !== "ready"
    ? platinumPriceStatus === "loading" ? "Loading Platinum prices…" : "Platinum prices unavailable"
    : `${itemCount} item${itemCount !== 1 ? "s" : ""}${itemCount === 1000 ? " (capped)" : ""}`;
  return (
    <>
      <div className={TOOLBAR}>
        <SearchBar
          placeholder="Search items (comma-separated)…"
          value={inputSearch}
          onChange={setInputSearch}
        />
      </div>
      <FilterBar>
        <FilterChip active={filterOwned} onClick={() => onFiltersChange(previous => ({ ...previous, filterOwned: !previous.filterOwned }))}>Owned</FilterChip>
        <FilterChip active={filterRecent} onClick={onToggleRecent}>Changed recently</FilterChip>
        <FilterChip active={filterPrime} onClick={() => onFiltersChange(previous => ({ ...previous, filterPrime: !previous.filterPrime }))}>Prime</FilterChip>
        <FilterChip active={filterVaulted} onClick={() => onFiltersChange(previous => ({ ...previous, filterVaulted: !previous.filterVaulted }))}>🔒 Vaulted</FilterChip>
        <FilterChip active={filterUnvaulted} onClick={() => onFiltersChange(previous => ({ ...previous, filterUnvaulted: !previous.filterUnvaulted }))}>🔓 Unvaulted</FilterChip>
        <FilterChip active={filterTradeable} aria-pressed={filterTradeable} onClick={() => onFiltersChange(previous => ({ ...previous, filterTradeable: !previous.filterTradeable }))}>Tradeable</FilterChip>
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
        <FilterChip active={sortMode === "qty-desc" || sortMode === "qty-asc"} onClick={() => toggleSort("qty-desc", "qty-asc")}>Qty{sortMode === "qty-desc" ? " ↓" : sortMode === "qty-asc" ? " ↑" : ""}</FilterChip>
        <FilterChip active={sortMode === "name-asc" || sortMode === "name-desc"} onClick={() => toggleSort("name-asc", "name-desc")}>{sortMode === "name-desc" ? "Z-A" : "A-Z"}</FilterChip>
        <FilterChip active={platinumSortActive} onClick={() => toggleSort("plat-desc", "plat-asc")}>Plat{platinumPriceStatus === "ready" ? sortMode === "plat-desc" ? " ↓" : sortMode === "plat-asc" ? " ↑" : "" : ""}</FilterChip>
        <FilterChip active={sortMode === "ducat-desc" || sortMode === "ducat-asc"} onClick={() => toggleSort("ducat-desc", "ducat-asc")}>Ducats{sortMode === "ducat-desc" ? " ↓" : sortMode === "ducat-asc" ? " ↑" : ""}</FilterChip>
        <FilterChip active={ducatRatioSortActive} title="Ducats per Platinum: descending favors Ducats; ascending favors trading" onClick={() => toggleSort("ducat-ratio-desc", "ducat-ratio-asc")}>D/P{platinumPriceStatus === "ready" ? sortMode === "ducat-ratio-desc" ? " ↓" : sortMode === "ducat-ratio-asc" ? " ↑" : "" : ""}</FilterChip>
        <span className={`${ITEM_COUNT_LABEL} ml-auto`} aria-live="polite">{itemCountLabel}</span>
        <ViewToggle
          view={view === "text-cards" ? "cards" : view === "list-compact" ? "list" : view}
          onChange={onViewChange}
          modes={["cards", "icons", "list"]}
        />
        {(isCardView || isListView) && (
          <>
            <label className={IMAGE_TOGGLE}>
              <input type="checkbox" checked={imagesVisible} className={IMAGE_CHECK}
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
