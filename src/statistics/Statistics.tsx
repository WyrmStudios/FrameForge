import Reports from "./Reports";
import ItemReport from "./ItemReport";
import "./Statistics.css";

interface Props {
  tab: "trade" | "item";
  onTabChange: (t: "trade" | "item") => void;
  dateRange: number | "all";
  onDateRangeChange: (r: number | "all") => void;
  clockFormat: "auto" | "12h" | "24h";
  systemLocale: string;
}

export default function Statistics({ tab, onTabChange, dateRange, onDateRangeChange, clockFormat, systemLocale }: Props) {
  return (
    <div className="statistics">
      <div className="stat-sub-tabs">
        <button className={tab === "trade" ? "active" : ""} onClick={() => onTabChange("trade")}>
          Trade Report
        </button>
        <button className={tab === "item" ? "active" : ""} onClick={() => onTabChange("item")}>
          Item Report
        </button>
      </div>
      {tab === "trade" ? <Reports dateRange={dateRange} onDateRangeChange={onDateRangeChange} clockFormat={clockFormat} systemLocale={systemLocale} /> : <ItemReport />}
    </div>
  );
}
