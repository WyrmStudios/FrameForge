import type { ComponentProps } from "react";
import Syndicates from "./Syndicates";
import Weapons from "./Weapons";

export type CompletionistView = "syndicates" | "weapons";

interface CompletionistTabsProps {
  view: CompletionistView;
  onViewChange: (view: CompletionistView) => void;
  syndicates: ComponentProps<typeof Syndicates>;
  weapons: ComponentProps<typeof Weapons>;
}

export default function CompletionistTabs({ view, onViewChange, syndicates, weapons }: CompletionistTabsProps) {
  return (
    <div style={{ flex: 1, display: "flex", flexDirection: "column", overflow: "hidden", minHeight: 0 }}>
      <div style={{ display: "flex", gap: 2, padding: "8px 12px 0", borderBottom: "1px solid var(--border)", flexShrink: 0 }}>
        {(["syndicates", "weapons"] as const).map(tab => (
          <button
            key={tab}
            onClick={() => onViewChange(tab)}
            style={{
              padding: "5px 16px", border: "none", borderRadius: "6px 6px 0 0",
              borderBottom: `3px solid ${view === tab ? "var(--accent, #888)" : "transparent"}`,
              background: view === tab ? "var(--bg-card)" : "transparent",
              color: view === tab ? "var(--text)" : "var(--text-dim)", cursor: "pointer",
              fontSize: 13, fontWeight: 500, marginBottom: -1,
              transition: "background 0.15s, color 0.15s", textTransform: "capitalize",
            }}
          >
            {tab === "syndicates" ? "Syndicates" : "Weapons"}
          </button>
        ))}
      </div>
      {view === "syndicates" && <Syndicates {...syndicates} />}
      {view === "weapons" && <Weapons {...weapons} />}
    </div>
  );
}
