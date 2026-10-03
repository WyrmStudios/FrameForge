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
    <div className="relative flex items-center gap-[6px]" ref={rootRef}>
      <span className="text-[11px] text-muted">{label}</span>
      <button
        type="button"
        className={`flex min-w-[88px] cursor-pointer items-center gap-[6px] rounded-[3px] border border-border bg-surface px-[6px] py-[3px] text-[11px] text-foreground hover:border-accent ${open ? "border-accent" : ""}`}
        aria-expanded={open}
        aria-label={label}
        onClick={() => setOpen((o) => !o)}
      >
        {summarize(selected)}
        <span className="ml-auto text-muted">▾</span>
      </button>
      {open && (
        <div className="absolute top-[calc(100%+4px)] left-0 z-20 min-w-[140px] rounded-[4px] border border-border bg-surface p-[4px] shadow-[0_6px_18px_rgba(0,0,0,.45)]">
          {TIER_KEYS.map((k) => (
            <label key={k} className="flex cursor-pointer items-center gap-[4px] rounded-[3px] px-[5px] py-[3px] text-[11px] hover:bg-[rgba(255,255,255,.05)]">
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
          <div className="mt-[4px] flex gap-[4px] border-t border-border pt-[4px]">
            <button className="flex-1 cursor-pointer rounded-[3px] border border-border bg-transparent py-[2px] text-[11px] text-muted hover:border-accent hover:text-foreground" type="button" onClick={() => onChange([...TIER_KEYS])}>
              All
            </button>
            <button className="flex-1 cursor-pointer rounded-[3px] border border-border bg-transparent py-[2px] text-[11px] text-muted hover:border-accent hover:text-foreground" type="button" onClick={() => onChange([])}>
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
      <span className={`mr-[6px] inline-flex h-[16px] min-w-[16px] shrink-0 items-center justify-center rounded-[3px] px-[3px] text-[10px] leading-none font-bold ${showUnrated ? "border border-dashed border-border bg-transparent text-muted" : "bg-transparent text-[#0d1117]"}`}>
        {showUnrated ? "–" : ""}
      </span>
    );
  }
  return (
    <span className={`mr-[6px] inline-flex h-[16px] min-w-[16px] shrink-0 items-center justify-center rounded-[3px] px-[3px] text-[10px] leading-none font-bold text-[#0d1117] ${tier === "S" ? "bg-[#f0c040]" : tier === "A" ? "bg-[#3fb950]" : tier === "B" ? "bg-[#388bfd]" : tier === "C" ? "bg-[#8b949e]" : "bg-[#6e4a9e] text-[#e6edf3]"}`} title={`Tier ${tier}`}>
      {tier}
    </span>
  );
}
