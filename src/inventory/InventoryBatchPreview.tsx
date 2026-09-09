import { useEffect, useRef, useState } from "react";
import InventoryGrid from "./InventoryGrid";
import { ViewToggle } from "../shared/ViewToggle";
import type { ViewMode } from "../types/ui";
import "./InventoryBatchPreview.css";

interface InventoryBatchPreviewProps {
  onClose: () => void;
}

const PREVIEW_STATES = [
  ["all", "All incoming"],
  ["gained", "Gained"],
  ["lost", "Lost"],
  ["crafting", "Crafting"],
  ["mod-transfer", "Rank transfer"],
  ["mod-gained", "Rank gain"],
  ["expired", "After 5 min"],
] as const;

type PreviewState = typeof PREVIEW_STATES[number][0];

export default function InventoryBatchPreview({ onClose }: InventoryBatchPreviewProps) {
  const [view, setView] = useState<ViewMode>("cards");
  const [favorites, setFavorites] = useState<Set<string>>(new Set());
  const [previewState, setPreviewState] = useState<PreviewState>("all");
  const dialogRef = useRef<HTMLElement>(null);
  const closeButtonRef = useRef<HTMLButtonElement>(null);
  const previousFocusRef = useRef<HTMLElement | null>(null);
  const now = Math.floor(Date.now() / 1000);
  const expired = previewState === "expired";
  const changeTimestamp = now - (expired ? 301 : 0);
  const items = [
    { unique_name: "preview-gained", name: "New resource stack", category: "Resources", qty: 12, state: "gained" },
    { unique_name: "preview-lost", name: "Consumed component", category: "Misc", qty: 0, state: "lost" },
    { unique_name: "preview-crafting", name: "Building item", category: "Primary", qty: 1, state: "crafting" },
    { unique_name: "preview-mod-transfer", name: "Rank transfer", category: "Mods", qty: 2, state: "mod-transfer" },
    { unique_name: "preview-mod-gained", name: "Ranked mod gain", category: "Mods", qty: 3, state: "mod-gained" },
  ];

  const changes = new Map<string, { delta: number; rank?: number; timestamp: number }[]>([
    ["preview-gained", [{ delta: 12, timestamp: changeTimestamp }]],
    ["preview-lost", [{ delta: -3, timestamp: changeTimestamp }]],
    ["preview-mod-transfer", [{ rank: 0, delta: -1, timestamp: changeTimestamp }, { rank: 5, delta: 1, timestamp: changeTimestamp }]],
    ["preview-mod-gained", [{ rank: 3, delta: 3, timestamp: changeTimestamp }]],
  ]);

  useEffect(() => {
    previousFocusRef.current = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    closeButtonRef.current?.focus();
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        onClose();
        return;
      }
      if (event.key !== "Tab" || !dialogRef.current) return;
      const focusable = dialogRef.current.querySelectorAll<HTMLElement>("button:not(:disabled), [href], input:not(:disabled), select:not(:disabled), textarea:not(:disabled), [tabindex]:not([tabindex='-1'])");
      const first = focusable[0];
      const last = focusable[focusable.length - 1];
      if (!first || !last) return;
      if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus();
      }
    };
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("keydown", onKeyDown);
      previousFocusRef.current?.focus();
    };
  }, [onClose]);

  return (
    <div className="inventory-preview-overlay" onClick={onClose}>
      <section ref={dialogRef} className="inventory-preview" onClick={event => event.stopPropagation()} role="dialog" aria-modal="true" aria-label="Inventory change preview">
        <header className="inventory-preview-header">
          <div>
            <strong>Incoming inventory batch preview</strong>
            <span>{expired ? "Recent indicators have expired." : "This does not change inventory data."}</span>
          </div>
          <ViewToggle view={view} onChange={setView} />
          <button ref={closeButtonRef} className="craft-detail-close" onClick={onClose} aria-label="Close preview">x</button>
        </header>
        <div className="inventory-preview-states" aria-label="Preview state">
          {PREVIEW_STATES.map(([state, label]) => (
            <button
              key={state}
              className={`fchip${previewState === state ? " fchip-on" : ""}`}
              onClick={() => setPreviewState(state)}
            >{label}</button>
          ))}
        </div>
        <InventoryGrid
          items={items.filter(item => previewState === "all" || expired || item.state === previewState)}
          loading={false}
          monitoring
          view={view}
          inventory={{
            "preview-gained": { mastery_rank: 30 },
            "preview-crafting": { mastery_rank: 14 },
          }}
          modCopies={{
            "preview-mod-transfer": [{ rank: 5, count: 2 }],
            "preview-mod-gained": [{ rank: 3, count: 3 }],
          }}
          favorites={favorites}
          lastChanged={{
            "preview-gained": changeTimestamp,
            "preview-lost": changeTimestamp,
            "preview-mod-transfer": changeTimestamp,
            "preview-mod-gained": changeTimestamp,
          }}
          changes={changes}
          crafting={new Map([["preview-crafting", { item_name: "Building item" }]])}
          filterRank={null}
          onToggleFavorite={id => setFavorites(previous => {
            const next = new Set(previous);
            if (next.has(id)) next.delete(id);
            else next.add(id);
            return next;
          })}
        />
      </section>
    </div>
  );
}
