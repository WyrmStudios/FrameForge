import { useState } from "react";
import type { InventoryItem } from "../types/items";
import type { ClockFormat } from "../types/settings";
import Syndicates from "./Syndicates";
import Mastery from "./Mastery";
import { SYNDICATE_FILTERS_DEFAULT } from "../constants/filters";
import type { SyndicateFilters } from "../types/filters";

export type CompletionistView = "syndicates" | "mastery";

interface CompletionistTabsProps {
  inventory: Record<string, InventoryItem>;
  refreshKey: number;
  clockFormat: ClockFormat;
  tracked: string[];
  onTrackToggle: (uniqueName: string) => void;
}

export default function CompletionistTabs({ inventory, refreshKey, clockFormat, tracked, onTrackToggle }: CompletionistTabsProps) {
  const [view, setView] = useState<CompletionistView>("syndicates");
  const [syndicateFilters, setSyndicateFilters] = useState<SyndicateFilters>(SYNDICATE_FILTERS_DEFAULT);

  return (
    <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
      <div className="flex shrink-0 gap-0.5 border-b border-border px-3 pt-2">
        {(["syndicates", "mastery"] as const).map(tab => (
          <button
            key={tab}
            onClick={() => setView(tab)}
            className={`-mb-px cursor-pointer rounded-t-6 border-0 border-b-3 px-4 py-1.25 text-13 font-medium capitalize transition-[background,color] duration-150 ${view === tab ? "border-accent bg-surface text-foreground" : "border-transparent bg-transparent text-muted"}`}
          >
            {tab === "syndicates" ? "Syndicates" : "Mastery"}
          </button>
        ))}
      </div>
      {view === "syndicates" && (
        <Syndicates
          inventory={inventory}
          filters={syndicateFilters}
          onFiltersChange={setSyndicateFilters}
        />
      )}
      {view === "mastery" && (
        <Mastery inventory={inventory} refreshKey={refreshKey} clockFormat={clockFormat} tracked={tracked} onTrackToggle={onTrackToggle} />
      )}
    </div>
  );
}
