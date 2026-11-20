import { useState } from "react";
import Reports from "./Reports";
import ItemReport from "./ItemReport";

const STATISTICS_CLASS = "flex min-h-0 flex-1 flex-col overflow-hidden";
const SUB_TABS_CLASS = "flex shrink-0 gap-0.5 border-b border-border px-3 py-1.5";
const SUB_TAB_CLASS = "cursor-pointer rounded-4 border px-3.5 py-0.75 text-12 transition-[background,color,border-color] duration-100";
const SUB_TAB_IDLE_CLASS = "border-border/60 bg-transparent text-muted hover:bg-white/6 hover:text-foreground";
const SUB_TAB_ACTIVE_CLASS = "border-accent bg-accent/15 text-accent";

interface Props {
  clockFormat: "auto" | "12h" | "24h";
  systemLocale: string;
}

export default function Statistics({ clockFormat, systemLocale }: Props) {
  const [tab, setTab] = useState<"trade" | "item">("trade");
  const [dateRange, setDateRange] = useState<number | "all">(30);

  return (
    <div className={STATISTICS_CLASS}>
      <div className={SUB_TABS_CLASS}>
        <button className={`${SUB_TAB_CLASS} ${tab === "trade" ? SUB_TAB_ACTIVE_CLASS : SUB_TAB_IDLE_CLASS}`} onClick={() => setTab("trade")}>
          Trade Report
        </button>
        <button className={`${SUB_TAB_CLASS} ${tab === "item" ? SUB_TAB_ACTIVE_CLASS : SUB_TAB_IDLE_CLASS}`} onClick={() => setTab("item")}>
          Item Report
        </button>
      </div>
      {tab === "trade" ? <Reports dateRange={dateRange} onDateRangeChange={setDateRange} clockFormat={clockFormat} systemLocale={systemLocale} /> : <ItemReport />}
    </div>
  );
}
