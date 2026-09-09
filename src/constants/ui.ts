import type { ViewMode } from "../types/ui";

export const VIEW_MODE_OPTIONS: readonly { mode: ViewMode; label: string }[] = [
  { mode: "cards", label: "Cards (icon + text)" },
  { mode: "icons", label: "Icon grid" },
  { mode: "text-cards", label: "Text cards (no icons)" },
  { mode: "list", label: "List with icon" },
  { mode: "list-compact", label: "Compact list (text only)" },
];
