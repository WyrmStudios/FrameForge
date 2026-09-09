import type { Dispatch, SetStateAction } from "react";
import { HelpTip } from "../shared/HelpTip";
import type { InventoryFilters } from "../types/filters";
import SearchBar from "../shared/SearchBar";
import { ViewToggle } from "../shared/ViewToggle";
import type { ViewMode } from "../types/ui";

interface InventoryToolbarProps {
  filters: InventoryFilters;
  onFiltersChange: Dispatch<SetStateAction<InventoryFilters>>;
  onToggleRecent: () => void;
  availableRanks: number[];
  showRankFilters: boolean;
  itemCount: number;
  view: ViewMode;
  onViewChange: (view: ViewMode) => void;
}

export default function InventoryToolbar({
  filters, onFiltersChange, onToggleRecent, availableRanks, showRankFilters, itemCount, view, onViewChange,
}: InventoryToolbarProps) {
  const { search, filterOwned, filterRecent, filterPrime, filterVaulted, filterUnvaulted, filterRank, sortMode } = filters;
  return (
    <>
      <div className="toolbar">
        <SearchBar
          placeholder="Search items…"
          value={search}
          onChange={search => onFiltersChange(previous => ({ ...previous, search }))}
        />
      </div>
      <div className="filter-bar">
        <button className={`fchip ${filterOwned ? "fchip-on" : ""}`} onClick={() => onFiltersChange(previous => ({ ...previous, filterOwned: !previous.filterOwned }))}>Owned</button>
        <button className={`fchip ${filterRecent ? "fchip-on" : ""}`} onClick={onToggleRecent}>Changed recently</button>
        <button className={`fchip ${filterPrime ? "fchip-on" : ""}`} onClick={() => onFiltersChange(previous => ({ ...previous, filterPrime: !previous.filterPrime }))}>Prime</button>
        <button className={`fchip ${filterVaulted ? "fchip-on" : ""}`} onClick={() => onFiltersChange(previous => ({ ...previous, filterVaulted: !previous.filterVaulted }))}>🔒 Vaulted</button>
        <button className={`fchip ${filterUnvaulted ? "fchip-on" : ""}`} onClick={() => onFiltersChange(previous => ({ ...previous, filterUnvaulted: !previous.filterUnvaulted }))}>🔓 Unvaulted</button>
        {showRankFilters && <>
          <span className="fbar-sep" />
          <span className="fbar-label">Rank:</span>
          <button className={`fchip ${filterRank === "unranked" ? "fchip-on" : ""}`} onClick={() => onFiltersChange(previous => ({ ...previous, filterRank: previous.filterRank === "unranked" ? null : "unranked" }))}>Unranked</button>
          {availableRanks.map(rank => (
            <button key={rank} className={`fchip ${filterRank === rank ? "fchip-on" : ""}`} onClick={() => onFiltersChange(previous => ({ ...previous, filterRank: previous.filterRank === rank ? null : rank }))}>R{rank}</button>
          ))}
        </>}
        <span className="fbar-sep" />
        <span className="fbar-label">Sort:</span>
        <button className={`fchip ${sortMode === "qty-desc" ? "fchip-on" : ""}`} onClick={() => onFiltersChange(previous => ({ ...previous, sortMode: "qty-desc" }))}>Qty ↓</button>
        <button className={`fchip ${sortMode === "qty-asc" ? "fchip-on" : ""}`} onClick={() => onFiltersChange(previous => ({ ...previous, sortMode: "qty-asc" }))}>Qty ↑</button>
        <button className={`fchip ${sortMode === "name-asc" ? "fchip-on" : ""}`} onClick={() => onFiltersChange(previous => ({ ...previous, sortMode: "name-asc" }))}>A-Z</button>
        <button className={`fchip ${sortMode === "name-desc" ? "fchip-on" : ""}`} onClick={() => onFiltersChange(previous => ({ ...previous, sortMode: "name-desc" }))}>Z-A</button>
        <span className="item-count-label" style={{ marginLeft: "auto" }}>{itemCount} item{itemCount !== 1 ? "s" : ""}{itemCount === 1000 ? " (capped)" : ""}</span>
        <ViewToggle view={view} onChange={onViewChange} />
        <HelpTip items={[
          { icon: "★", label: "★  Mastered", desc: "Shown above image — item levelled to rank 30" },
          { icon: "R5", label: "R{n}  Rank", desc: "Shown above image — current rank, not yet mastered" },
          { icon: "⚒", label: "⚒  Building", desc: "Shown on image — currently crafting in Foundry" },
          { swatch: "rgba(63,185,80,.5)", label: "Green border", desc: "Item recently gained" },
          { swatch: "rgba(248,81,73,.5)", label: "Red border", desc: "Item recently lost or consumed" },
        ]} />
      </div>
    </>
  );
}
