import type { ViewMode } from "../types/ui";
import { VIEW_MODE_OPTIONS } from "../constants/ui";

const VIEW_TOGGLE = "flex items-center gap-0.5 shrink-0";
const VIEW_BTN =
  "bg-transparent border border-transparent rounded-4 cursor-pointer text-muted px-1.25 py-0.75 flex items-center justify-center transition-[color,border-color,background] duration-120 leading-none";
const VIEW_BTN_HOVER = "hover:text-foreground! hover:bg-white/6!";
const VIEW_BTN_ON = "text-accent! border-accent/40! bg-accent/8!";


function ViewIcon({ mode }: { mode: ViewMode }) {
  switch (mode) {
    case "cards": return (
      <svg width="16" height="13" viewBox="0 0 16 13" fill="currentColor">
        <rect x="0" y="0" width="3" height="3" rx="0.5"/><rect x="4" y="0.5" width="3.5" height="1.5" rx="0.4"/>
        <rect x="9" y="0" width="3" height="3" rx="0.5"/><rect x="13" y="0.5" width="3.5" height="1.5" rx="0.4"/>
        <rect x="0" y="5" width="3" height="3" rx="0.5"/><rect x="4" y="5.5" width="3.5" height="1.5" rx="0.4"/>
        <rect x="9" y="5" width="3" height="3" rx="0.5"/><rect x="13" y="5.5" width="3.5" height="1.5" rx="0.4"/>
        <rect x="0" y="10" width="3" height="3" rx="0.5"/><rect x="4" y="10.5" width="3.5" height="1.5" rx="0.4"/>
        <rect x="9" y="10" width="3" height="3" rx="0.5"/><rect x="13" y="10.5" width="3.5" height="1.5" rx="0.4"/>
      </svg>
    );
    case "icons": return (
      <svg width="16" height="13" viewBox="0 0 16 13" fill="currentColor">
        <rect x="0" y="0" width="4" height="4" rx="0.5"/><rect x="6" y="0" width="4" height="4" rx="0.5"/><rect x="12" y="0" width="4" height="4" rx="0.5"/>
        <rect x="0" y="5" width="4" height="4" rx="0.5"/><rect x="6" y="5" width="4" height="4" rx="0.5"/><rect x="12" y="5" width="4" height="4" rx="0.5"/>
        <rect x="0" y="10" width="4" height="4" rx="0.5"/><rect x="6" y="10" width="4" height="4" rx="0.5"/><rect x="12" y="10" width="4" height="4" rx="0.5"/>
      </svg>
    );
    case "text-cards": return (
      <svg width="16" height="13" viewBox="0 0 16 13" fill="currentColor">
        <rect x="0" y="0" width="7" height="1.5" rx="0.4"/><rect x="0" y="2.5" width="5" height="1" rx="0.4"/>
        <rect x="9" y="0" width="7" height="1.5" rx="0.4"/><rect x="9" y="2.5" width="5" height="1" rx="0.4"/>
        <rect x="0" y="5" width="7" height="1.5" rx="0.4"/><rect x="0" y="7.5" width="5" height="1" rx="0.4"/>
        <rect x="9" y="5" width="7" height="1.5" rx="0.4"/><rect x="9" y="7.5" width="5" height="1" rx="0.4"/>
        <rect x="0" y="10" width="7" height="1.5" rx="0.4"/><rect x="0" y="12" width="5" height="1" rx="0.4"/>
        <rect x="9" y="10" width="7" height="1.5" rx="0.4"/><rect x="9" y="12" width="5" height="1" rx="0.4"/>
      </svg>
    );
    case "list": return (
      <svg width="16" height="13" viewBox="0 0 16 13" fill="currentColor">
        <rect x="0" y="0" width="2.5" height="2.5" rx="0.4"/><rect x="4" y="0.5" width="12" height="1.5" rx="0.4"/>
        <rect x="0" y="3.5" width="2.5" height="2.5" rx="0.4"/><rect x="4" y="4" width="12" height="1.5" rx="0.4"/>
        <rect x="0" y="7" width="2.5" height="2.5" rx="0.4"/><rect x="4" y="7.5" width="12" height="1.5" rx="0.4"/>
        <rect x="0" y="10.5" width="2.5" height="2.5" rx="0.4"/><rect x="4" y="11" width="12" height="1.5" rx="0.4"/>
      </svg>
    );
    case "list-compact": return (
      <svg width="16" height="13" viewBox="0 0 16 13" fill="currentColor">
        <rect x="0" y="0" width="16" height="1.5" rx="0.4"/>
        <rect x="0" y="2.5" width="11" height="1.5" rx="0.4"/>
        <rect x="0" y="5" width="16" height="1.5" rx="0.4"/>
        <rect x="0" y="7.5" width="13" height="1.5" rx="0.4"/>
        <rect x="0" y="10" width="16" height="1.5" rx="0.4"/>
        <rect x="0" y="12" width="10" height="1" rx="0.4"/>
      </svg>
    );
  }
}

export function ViewToggle({ view, onChange, modes }: { view: ViewMode; onChange: (v: ViewMode) => void; modes?: readonly ViewMode[] }) {
  return (
    <div className={VIEW_TOGGLE}>
      {VIEW_MODE_OPTIONS.filter(({ mode }) => !modes || modes.includes(mode)).map(({ mode, label }) => (
        <button key={mode} className={`${VIEW_BTN} ${VIEW_BTN_HOVER}${view === mode ? ` ${VIEW_BTN_ON}` : ""}`}
          title={label} aria-label={label} aria-pressed={view === mode} onClick={() => onChange(mode)}>
          <ViewIcon mode={mode} />
        </button>
      ))}
    </div>
  );
}
