import { useRef, useState } from "react";
import { useClickOutside } from "../shared/useClickOutside";
import { TIER_KEYS, TIER_LABELS, type TierKey } from "./arbitrationTiers";

type Props = {
  label: string;
  selected: readonly TierKey[];
  onChange: (next: TierKey[]) => void;
};

function summarize(selected: readonly TierKey[]): string {
  if (selected.length === 0) return "None";
  if (selected.length === TIER_KEYS.length) return "All";
  return TIER_KEYS.filter((k) => selected.includes(k))
    .map((k) => TIER_LABELS[k])
    .join(", ");
}

/// Multi-select over the arbitration tiers, shared by the schedule's filter
/// and its alert rule. A native `<select multiple>` would need a modifier key
/// held to pick a second tier, which is not discoverable for the one control
/// most users will touch here.
export default function TierSelect({ label, selected, onChange }: Props) {
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);

  useClickOutside(rootRef, () => setOpen(false), open);

  const toggle = (key: TierKey) =>
    onChange(
      TIER_KEYS.filter((k) =>
        k === key ? !selected.includes(k) : selected.includes(k),
      ),
    );

  return (
    <div className="relative flex items-center gap-1.5" ref={rootRef}>
      <span className="text-11 text-muted">{label}</span>
      <button
        type="button"
        className={`flex min-w-22 cursor-pointer items-center gap-1.5 rounded-3 border border-border bg-surface px-1.5 py-0.75 text-11 text-foreground hover:border-accent ${open ? "border-accent" : ""}`}
        aria-expanded={open}
        aria-label={label}
        onClick={() => setOpen((o) => !o)}
      >
        {summarize(selected)}
        <span className="ml-auto text-muted">▾</span>
      </button>
      {open && (
        <div className="absolute top-[calc(100%+4px)] left-0 z-20 min-w-35 rounded-4 border border-border bg-surface p-1 shadow-[0_6px_18px_rgba(0,0,0,.45)]">
          {TIER_KEYS.map((k) => (
            <label key={k} className="flex cursor-pointer items-center gap-1 rounded-3 px-1.25 py-0.75 text-11 hover:bg-white/5">
              <input
                type="checkbox"
                checked={selected.includes(k)}
                onChange={() => toggle(k)}
                className="m-0"
              />
              <TierBadge tier={k === "unrated" ? null : k} showUnrated />
              <span>{TIER_LABELS[k]}</span>
            </label>
          ))}
          <div className="mt-1 flex gap-1 border-t border-border pt-1">
            <button className="flex-1 cursor-pointer rounded-3 border border-border bg-transparent py-0.5 text-11 text-muted hover:border-accent hover:text-foreground" type="button" onClick={() => onChange([...TIER_KEYS])}>
              All
            </button>
            <button className="flex-1 cursor-pointer rounded-3 border border-border bg-transparent py-0.5 text-11 text-muted hover:border-accent hover:text-foreground" type="button" onClick={() => onChange([])}>
              None
            </button>
          </div>
        </div>
      )}
    </div>
  );
}

export function TierBadge({
  tier,
  showUnrated = false,
}: {
  tier: TierKey | null;
  showUnrated?: boolean;
}) {
  // An unrated node keeps the badge's width so the node names below each
  // other still line up; only the dropdown spells the absence out.
  if (tier === null || tier === "unrated") {
    return (
      <span className={`mr-1.5 inline-flex h-4 min-w-4 shrink-0 items-center justify-center rounded-3 px-0.75 text-10 leading-none font-bold ${showUnrated ? "border border-dashed border-border bg-transparent text-muted" : "bg-transparent text-background"}`}>
        {showUnrated ? "–" : ""}
      </span>
    );
  }
  return (
    <span className={`mr-1.5 inline-flex h-4 min-w-4 shrink-0 items-center justify-center rounded-3 px-0.75 text-10 leading-none font-bold text-background ${tier === "S" ? "bg-ducat" : tier === "A" ? "bg-success" : tier === "B" ? "bg-accent" : tier === "C" ? "bg-muted" : "bg-tier-d text-foreground"}`} title={`Tier ${tier}`}>
      {tier}
    </span>
  );
}
