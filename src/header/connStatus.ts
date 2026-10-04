export const CONN_CHIP =
  "inline-flex items-center gap-1.25 text-11 px-2 py-0.75 rounded-4 border bg-white/3 text-muted whitespace-nowrap select-none";
export const CONN_BUTTON =
  "appearance-none [font:inherit] cursor-pointer text-left focus-visible:outline focus-visible:outline-2 focus-visible:outline-accent focus-visible:outline-offset-2";
export const CONN_DOT = "w-1.5 h-1.5 rounded-full shrink-0";
export const CONN_LABEL = "font-medium text-foreground opacity-70";

export type ConnState = "online" | "warn" | "offline" | "disabled" | "overlay";

export const CONN_STATUS: Record<ConnState, { chip: string; dot: string; detail: string }> = {
  online: {
    chip: "border-success/25",
    dot: "bg-success shadow-[0_0_5px_#3fb95066]",
    detail: "text-success",
  },
  warn: {
    chip: "border-warning/30",
    dot: "bg-warning",
    detail: "text-warning",
  },
  offline: {
    chip: "border-border",
    dot: "bg-inactive",
    detail: "text-muted",
  },
  disabled: {
    chip: "border-border opacity-45",
    dot: "bg-inactive",
    detail: "text-muted",
  },
  overlay: {
    chip: "border-overlay-chip/25",
    dot: "bg-connected animate-[pulse-ocr_1.2s_ease-in-out_infinite]",
    detail: "text-connected font-mono",
  },
};
