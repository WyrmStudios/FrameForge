const UPDATE_BADGE =
  "inline-flex items-center gap-1 text-10 font-bold text-success bg-success/15 border border-success/40 rounded-4 px-1.75 py-0.5 shrink-0 tracking-0.02 select-none whitespace-nowrap hover:bg-success/25";
const UPDATE_INSTALL =
  "bg-transparent border-0 text-inherit cursor-pointer [font:inherit] tracking-[inherit] p-0 focus-visible:outline focus-visible:outline-2 focus-visible:outline-accent focus-visible:outline-offset-2 disabled:cursor-wait";
const UPDATE_DISMISS =
  "bg-transparent border-0 text-white/65 cursor-pointer text-13 leading-none px-0.5 py-0 flex items-center hover:text-white focus-visible:outline focus-visible:outline-2 focus-visible:outline-accent focus-visible:outline-offset-2";

interface UpdateBadgeProps {
  version: string;
  installing: boolean;
  onInstall: () => void;
  onDismiss: () => void;
}

export default function UpdateBadge({ version, installing, onInstall, onDismiss }: UpdateBadgeProps) {
  return (
    <span className={UPDATE_BADGE}>
      <button className={UPDATE_INSTALL} title={installing ? "Installing update…" : `v${version} is available — click to install`} onClick={onInstall} disabled={installing}>
        {installing ? "Installing…" : `v${version} ↑`}
      </button>
      {!installing && <button className={UPDATE_DISMISS} title="Dismiss" onClick={event => { event.stopPropagation(); onDismiss(); }}>×</button>}
    </span>
  );
}
