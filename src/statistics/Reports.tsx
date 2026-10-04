import { useState, useEffect, useMemo } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import ItemImg from "../ItemImg";
import { TAURI_COMMANDS, TAURI_EVENTS } from "../constants/tauri";
import type { WfmTopItem } from "../types/market";
import type { Trade, TradeSession } from "../types/trades";
import "./Reports.css";

interface CategoryStat {
  category: string;
  revenue: number;
  expenses: number;
  profit: number;
  color: string;
}

interface ItemStat {
  item_name: string;
  quantity: number;
  total_plat: number;
}

interface WfmTopProgress {
  completed: number;
  total: number;
  refreshing: boolean;
}

const CATEGORY_COLORS: Record<string, string> = {
  Prime:   "#c4a44a",
  Riven:   "#9b59b6",
  Set:     "#4d8cca",
  Arcane:  "#e74c3c",
  Mod:     "#e67e22",
  Relic:   "#27ae60",
  Other:   "#7f8c8d",
};

function inferCategory(name: string): string {
  const n = name.toLowerCase();
  if (n.includes("riven"))  return "Riven";
  if (n.includes("arcane")) return "Arcane";
  if (n.includes("relic"))  return "Relic";
  if (n.includes("prime"))  return "Prime";
  if (/ set$/.test(n))      return "Set";
  if (/\bmod\b/.test(n))    return "Mod";
  return "Other";
}

function displayItemName(name: string): string {
  const rank = [...name].filter(char => /[\uE000-\uF8FF]/u.test(char)).length;
  const clean = name.replace(/[\uE000-\uF8FF\p{Cc}]/gu, "").trim();
  return rank > 0 ? `${clean} (R${rank})` : clean;
}

function groupBySessions(trades: Trade[]): TradeSession[] {
  const byId = new Map<string, Trade[]>();
  const legacy: Trade[] = [];

  for (const t of trades) {
    if (t.session_id) {
      if (!byId.has(t.session_id)) byId.set(t.session_id, []);
      byId.get(t.session_id)!.push(t);
    } else {
      legacy.push(t);
    }
  }

  const sessions: TradeSession[] = [];

  for (const [sid, rows] of byId) {
    const first = rows[0];
    const rawType = first.trade_type;
    const tradeType: TradeSession["tradeType"] =
      rawType === "purchase" ? "purchase" : rawType === "trade" ? "trade" : "sale";

    const givenItems = rows
      .filter(r => r.direction === "sold" || r.direction === "traded-out")
      .map(r => ({ name: r.item_name, qty: r.quantity }));
    const receivedItems = rows
      .filter(r => r.direction === "bought" || r.direction === "traded-in")
      .map(r => ({ name: r.item_name, qty: r.quantity }));
    // Plat is stored on the first row of the relevant direction
    const receivedPlat = rows.filter(r => r.direction === "sold").reduce((s, r) => s + r.platinum, 0);
    const givenPlat    = rows.filter(r => r.direction === "bought").reduce((s, r) => s + r.platinum, 0);

    sessions.push({ sessionId: sid, withPlayer: first.with_player, tradeType,
      givenItems, givenPlat, receivedItems, receivedPlat, timestamp: first.timestamp });
  }

  // Legacy rows (no session_id) — one row = one session
  for (const t of legacy) {
    const tradeType: TradeSession["tradeType"] =
      t.direction === "bought" ? "purchase" : "sale";
    sessions.push({
      sessionId: String(t.id),
      withPlayer: t.with_player,
      tradeType,
      givenItems:    t.direction === "sold"   ? [{ name: t.item_name, qty: t.quantity }] : [],
      givenPlat:     t.direction === "bought" ? t.platinum : 0,
      receivedItems: t.direction === "bought" ? [{ name: t.item_name, qty: t.quantity }] : [],
      receivedPlat:  t.direction === "sold"   ? t.platinum : 0,
      timestamp: t.timestamp,
    });
  }

  return sessions.sort((a, b) => new Date(b.timestamp).getTime() - new Date(a.timestamp).getTime());
}

const BADGE: Record<string, string> = { sale: "Sale", purchase: "Purchase", trade: "Trade" };
const BADGE_CLASS: Record<string, string> = {
  sale:     "border border-txn-sale/35 bg-txn-sale/20 text-txn-sale",
  purchase: "border border-txn-purchase/35 bg-txn-purchase/20 text-txn-purchase",
  trade:    "border border-txn-trade/35 bg-txn-trade/20 text-txn-trade",
};

// ── Presentation ──────────────────────────────────────────────────────────────

const RPT_ROOT_CLASS = "flex min-h-0 flex-1 flex-col overflow-hidden bg-background";
const RPT_SCROLL_CLASS = "flex min-h-0 flex-1 flex-col gap-3.5 overflow-y-auto p-4";
const RPT_CARD_CLASS = "min-w-0 flex-1 rounded-6 border border-border bg-surface p-3.5";
const RPT_CARD_FLEX_CLASS = "flex flex-col gap-2.5";
const RPT_CARD_TITLE_CLASS = "mb-1 text-13 font-semibold text-foreground";
const RPT_TOP_LOADING_CLASS = "flex items-center gap-2.5 py-2.5 text-12 text-muted";
const RPT_TOP_SOURCE_CLASS = "mt-0.75 text-11 text-muted";
const RPT_TOP_PROGRESS_CLASS = "relative mt-2 h-1.5 w-[min(340px,60vw)] overflow-hidden rounded-3 bg-white/12";
const RPT_TOP_PROGRESS_FILL_CLASS = "block h-full rounded-[inherit] bg-accent transition-[width] duration-200 ease-out";
const RPT_TOP_PROGRESS_EM_CLASS = "absolute left-0 top-2.25 text-10 not-italic text-muted";
const RPT_TOP_REFRESHING_CLASS = "mb-2.5 text-11 text-muted";
const RPT_TOP_SPINNER_CLASS = "inline-block size-3.5 shrink-0 animate-[rpt-spin_0.8s_linear_infinite] rounded-full border-2 border-white/15 border-t-accent";
const RPT_TOP_WRAP_CLASS = "flex items-start gap-4";
const RPT_RANGE_ROW_CLASS = "flex items-center gap-1.5";
const RPT_VIEW_TOGGLE_CLASS = "mr-2 flex gap-1 border-r border-border pr-2";
const RPT_RANGE_LABEL_CLASS = "mr-0.5 text-12 text-muted";
const RPT_RANGE_BTN_CLASS = "cursor-pointer rounded-4 border px-2.5 py-0.75 text-12 transition-[background] duration-150";
const RPT_RANGE_IDLE_CLASS = "border-border bg-surface text-foreground hover:bg-[var(--surface-hover)]";
const RPT_RANGE_ACTIVE_CLASS = "border-accent bg-[var(--accent-dim)] text-accent";
const RPT_TRADE_COUNT_CLASS = "ml-1.5 text-11 text-muted";
const RPT_LOG_CLASS = "flex flex-col gap-2.5";
const RPT_SESSION_CARD_CLASS = "overflow-hidden rounded-8 border border-border bg-surface";
const RPT_SESSION_HEADER_CLASS = "flex items-center gap-2 border-b border-border bg-white/3 px-3 py-2";
const RPT_SESSION_BADGE_CLASS = "shrink-0 rounded-3 px-1.75 py-0.5 text-10 font-bold uppercase tracking-0.04";
const RPT_SESSION_PLAYER_CLASS = "text-13 font-semibold text-foreground";
const RPT_SESSION_DATE_CLASS = "ml-auto text-11 text-muted";
const RPT_SESSION_BODY_CLASS = "flex items-start gap-3 px-3 py-2.5";
const RPT_SESSION_SIDE_CLASS = "flex min-w-0 flex-1 flex-col gap-1";
const RPT_SESSION_GAVE_CLASS = "items-end";
const RPT_SESSION_RECEIVED_CLASS = "items-start";
const RPT_SESSION_SIDE_LABEL_CLASS = "mb-0.5 text-10 uppercase tracking-wider text-muted";
const RPT_SESSION_ITEM_CLASS = "flex items-center gap-1 text-12 text-foreground";
const RPT_SESSION_QTY_CLASS = "text-11 text-muted";
const RPT_SESSION_PLAT_CLASS = "font-semibold text-foreground";
const RPT_SESSION_EMPTY_CLASS = "text-12 text-muted";
const RPT_SESSION_ARROW_CLASS = "shrink-0 pt-5 text-18 text-muted";
const RPT_SUMMARY_CLASS = "flex gap-3";
const RPT_STAT_CARD_CLASS = "flex flex-1 flex-col gap-1 rounded-6 border border-border bg-surface px-3.5 py-2.5";
const RPT_STAT_HIGHLIGHT_CLASS = "border-accent bg-[var(--accent-dim)]";
const RPT_STAT_LABEL_CLASS = "text-11 uppercase tracking-0.04 text-muted";
const RPT_STAT_VALUE_CLASS = "flex items-center gap-1.25 text-22 font-bold";
const RPT_GREEN_CLASS = "text-success";
const RPT_RED_CLASS = "text-danger";
const RPT_MUTED_CLASS = "text-muted";
const RPT_ROW_CLASS = "flex items-start gap-3.5";
const RPT_CHART_WRAP_CLASS = "flex items-center gap-3";
const RPT_LEGEND_CLASS = "flex min-w-0 flex-1 flex-col gap-1.25 text-12";
const RPT_LEGEND_ROW_CLASS = "flex items-center gap-1.5";
const RPT_LEGEND_DOT_CLASS = "size-2.5 shrink-0 rounded-2";
const RPT_LEGEND_LABEL_CLASS = "flex-1 truncate text-foreground";
const RPT_LEGEND_PCT_CLASS = "shrink-0 text-11 text-muted";
const RPT_TABLE_CLASS = "w-full table-auto border-collapse text-12";
const RPT_TH_CLASS = "whitespace-nowrap border-b border-border px-2 py-1 font-semibold text-muted";
const RPT_TD_CLASS = "align-middle whitespace-nowrap border-b border-border/50 px-2 py-1.25 text-foreground group-hover:bg-[var(--surface-hover)]";
const RPT_TD_NUM_CLASS = "text-right tabular-nums";
const RPT_EMPTY_ROW_CLASS = "whitespace-nowrap border-b border-border/50 p-3 text-center align-middle text-muted group-hover:bg-[var(--surface-hover)]";
const RPT_TBODY_ROW_CLASS = "group [&:last-child>td]:border-b-0";
const RPT_DOT_CLASS = "mr-1.5 inline-block size-2 shrink-0 rounded-2";
const RPT_EMPTY_CLASS = "flex flex-1 flex-col items-center justify-center gap-2.5 p-10 text-center";
const RPT_EMPTY_ICON_CLASS = "text-48";
const RPT_EMPTY_TITLE_CLASS = "text-16 font-semibold text-foreground";
const RPT_EMPTY_DESC_CLASS = "text-13 leading-1.6 text-muted";
const RPT_LOADING_CLASS = "flex flex-1 items-center justify-center text-14 text-muted";

function TradeCard({ session, clockFormat, systemLocale }: { session: TradeSession; clockFormat: "auto" | "12h" | "24h"; systemLocale: string }) {
  const date = new Date(session.timestamp);
  const dateStr = date.toLocaleDateString(systemLocale, { month: "short", day: "numeric", year: "numeric" });
  const timeOpts: Intl.DateTimeFormatOptions = { hour: "2-digit", minute: "2-digit" };
  if (clockFormat === "12h") timeOpts.hour12 = true;
  else if (clockFormat === "24h") timeOpts.hour12 = false;
  const timeStr = date.toLocaleTimeString(systemLocale, timeOpts);

  return (
    <div className={RPT_SESSION_CARD_CLASS}>
      <div className={RPT_SESSION_HEADER_CLASS}>
        <span className={`${RPT_SESSION_BADGE_CLASS} ${BADGE_CLASS[session.tradeType]}`}>
          {BADGE[session.tradeType]}
        </span>
        {session.withPlayer && (
          <span className={RPT_SESSION_PLAYER_CLASS}>{session.withPlayer}</span>
        )}
        <span className={RPT_SESSION_DATE_CLASS}>{dateStr} {timeStr}</span>
      </div>
      <div className={RPT_SESSION_BODY_CLASS}>
        <div className={`${RPT_SESSION_SIDE_CLASS} ${RPT_SESSION_GAVE_CLASS}`}>
          <span className={RPT_SESSION_SIDE_LABEL_CLASS}>Gave</span>
          {session.givenPlat > 0 && (
            <div className={RPT_SESSION_ITEM_CLASS}>
              <span className={RPT_SESSION_PLAT_CLASS}>{session.givenPlat.toLocaleString()}</span>
              <PlatIcon size={12} />
            </div>
          )}
          {session.givenItems.map((item, i) => (
            <div key={i} className={RPT_SESSION_ITEM_CLASS}>
              {item.qty > 1 && <span className={RPT_SESSION_QTY_CLASS}>{item.qty}×</span>}
              <span>{item.name}</span>
            </div>
          ))}
          {session.givenPlat === 0 && session.givenItems.length === 0 && (
            <span className={RPT_SESSION_EMPTY_CLASS}>—</span>
          )}
        </div>
        <div className={RPT_SESSION_ARROW_CLASS}>→</div>
        <div className={`${RPT_SESSION_SIDE_CLASS} ${RPT_SESSION_RECEIVED_CLASS}`}>
          <span className={RPT_SESSION_SIDE_LABEL_CLASS}>Received</span>
          {session.receivedPlat > 0 && (
            <div className={RPT_SESSION_ITEM_CLASS}>
              <span className={RPT_SESSION_PLAT_CLASS}>{session.receivedPlat.toLocaleString()}</span>
              <PlatIcon size={12} />
            </div>
          )}
          {session.receivedItems.map((item, i) => (
            <div key={i} className={RPT_SESSION_ITEM_CLASS}>
              {item.qty > 1 && <span className={RPT_SESSION_QTY_CLASS}>{item.qty}×</span>}
              <span>{item.name}</span>
            </div>
          ))}
          {session.receivedPlat === 0 && session.receivedItems.length === 0 && (
            <span className={RPT_SESSION_EMPTY_CLASS}>—</span>
          )}
        </div>
      </div>
    </div>
  );
}

function fmtK(n: number): string {
  if (n >= 10000) return `${(n / 1000).toFixed(1)}K`;
  return n.toLocaleString();
}

function PlatIcon({ size = 14 }: { size?: number }) {
  return <img src="/platinum.webp" alt="" width={size} height={size} className="shrink-0 object-contain align-middle" />;
}

// ── SVG Donut Chart ─────────────────────────────────────────────────────────

function DonutChart({ data }: { data: { label: string; value: number; color: string }[] }) {
  const total = data.reduce((s, d) => s + d.value, 0);
  if (total === 0) return null;

  const cx = 90, cy = 90, r = 75, ir = 48;
  let angle = -Math.PI / 2;

  const slices = data.map(d => {
    const start = angle;
    const sweep = (d.value / total) * 2 * Math.PI;
    angle += sweep;
    const end = angle;
    const large = sweep > Math.PI ? 1 : 0;

    const ox1 = cx + r * Math.cos(start), oy1 = cy + r * Math.sin(start);
    const ox2 = cx + r * Math.cos(end),   oy2 = cy + r * Math.sin(end);
    const ix1 = cx + ir * Math.cos(start), iy1 = cy + ir * Math.sin(start);
    const ix2 = cx + ir * Math.cos(end),   iy2 = cy + ir * Math.sin(end);

    // Full circle: use two arcs to avoid degenerate path
    const path = sweep >= 2 * Math.PI - 0.001
      ? `M${cx},${cy - r} A${r},${r} 0 1,1 ${cx - 0.001},${cy - r} Z`
      : `M${ox1},${oy1} A${r},${r} 0 ${large},1 ${ox2},${oy2} L${ix2},${iy2} A${ir},${ir} 0 ${large},0 ${ix1},${iy1} Z`;

    const mid = start + sweep / 2;
    const labelR = (r + ir) / 2;
    const lx = cx + labelR * Math.cos(mid);
    const ly = cy + labelR * Math.sin(mid);
    const pct = Math.round((d.value / total) * 100);

    return { ...d, path, lx, ly, pct };
  });

  return (
    <svg viewBox="0 0 180 180" width={180} height={180} className="shrink-0">
      {slices.map((s, i) => (
        <path key={i} d={s.path} fill={s.color} strokeWidth={1.5} style={{ stroke: "var(--bg)" }} />
      ))}
      {slices.filter(s => s.pct >= 7).map((s, i) => (
        <text key={i} x={s.lx} y={s.ly} textAnchor="middle" dominantBaseline="middle"
          fontSize="11" fill="#fff" fontWeight="700">{s.pct}%</text>
      ))}
    </svg>
  );
}

// ── Legend ──────────────────────────────────────────────────────────────────

function Legend({ items }: { items: { label: string; color: string; value: number }[] }) {
  const total = items.reduce((s, d) => s + d.value, 0);
  return (
    <div className={RPT_LEGEND_CLASS}>
      {items.map(item => (
        <div key={item.label} className={RPT_LEGEND_ROW_CLASS}>
          <span className={RPT_LEGEND_DOT_CLASS} style={{ background: item.color }} />
          <span className={RPT_LEGEND_LABEL_CLASS}>{item.label}</span>
          <span className={RPT_LEGEND_PCT_CLASS}>{total > 0 ? Math.round((item.value / total) * 100) : 0}%</span>
        </div>
      ))}
    </div>
  );
}

interface Props {
  dateRange: number | "all";
  onDateRangeChange: (r: number | "all") => void;
  clockFormat: "auto" | "12h" | "24h";
  systemLocale: string;
}

export default function Reports({ dateRange, onDateRangeChange, clockFormat, systemLocale }: Props) {
  const [trades, setTrades]         = useState<Trade[]>([]);
  const [loading, setLoading]       = useState(true);
  const [tradesError, setTradesError] = useState<string | null>(null);
  const [topItems, setTopItems]     = useState<WfmTopItem[]>([]);
  const [topLoading, setTopLoading] = useState(true);
  const [topError, setTopError]     = useState<string | null>(null);
  const [topProgress, setTopProgress] = useState<WfmTopProgress | null>(null);
  const [view, setView]             = useState<"analytics" | "log">("analytics");

  useEffect(() => {
    const fetchTrades = () => {
      invoke<Trade[]>("get_trades")
        .then(t => { setTrades(t.map(trade => ({ ...trade, item_name: displayItemName(trade.item_name) }))); setLoading(false); })
        .catch((e) => { console.error("[Reports] get_trades failed:", e); setTradesError(String(e)); setLoading(false); });
    };
    fetchTrades();
    const unlistenTrades = listen(TAURI_EVENTS.TRADES_UPDATED, fetchTrades);

    const unlistenProgress = listen<WfmTopProgress>(TAURI_EVENTS.WFM_TOP_PROGRESS, ({ payload }) => {
      setTopProgress(payload);
    });
    const unlistenUpdated = listen<WfmTopItem[]>(TAURI_EVENTS.WFM_TOP_UPDATED, ({ payload }) => {
      setTopItems(payload);
      setTopLoading(false);
      setTopError(null);
      setTopProgress(null);
    });

    invoke<WfmTopItem[]>(TAURI_COMMANDS.GET_WFM_TOP_ITEMS)
      .then(items => { setTopItems(items); setTopLoading(false); })
      .catch((e) => { console.error("[Reports] get_wfm_top_items failed:", e); setTopError(String(e)); setTopLoading(false); });
    return () => {
      unlistenTrades.then(fn => fn());
      unlistenProgress.then(fn => fn());
      unlistenUpdated.then(fn => fn());
    };
  }, []);

  const filtered = useMemo(() => {
    if (dateRange === "all") return trades;
    const cutoff = Date.now() - dateRange * 86_400_000;
    return trades.filter(t => new Date(t.timestamp).getTime() >= cutoff);
  }, [trades, dateRange]);

  const totalRevenue  = useMemo(() => filtered.filter(t => t.direction === "sold" || t.direction === "traded-out").reduce((s, t) => s + t.platinum * t.quantity, 0), [filtered]);
  const totalExpenses = useMemo(() => filtered.filter(t => t.direction === "bought" || t.direction === "traded-in").reduce((s, t) => s + t.platinum * t.quantity, 0), [filtered]);
  const profit        = totalRevenue - totalExpenses;

  const byCategory = useMemo((): CategoryStat[] => {
    const map: Record<string, { revenue: number; expenses: number }> = {};
    for (const t of filtered) {
      const cat = inferCategory(t.item_name);
      if (!map[cat]) map[cat] = { revenue: 0, expenses: 0 };
      const val = t.platinum * t.quantity;
      if (t.direction === "sold" || t.direction === "traded-out") map[cat].revenue  += val;
      else                                                        map[cat].expenses += val;
    }
    return Object.entries(map)
      .map(([category, { revenue, expenses }]) => ({
        category, revenue, expenses,
        profit: revenue - expenses,
        color: CATEGORY_COLORS[category] ?? "#7f8c8d",
      }))
      .sort((a, b) => b.profit - a.profit);
  }, [filtered]);

  const topSold = useMemo((): ItemStat[] => {
    const map: Record<string, ItemStat> = {};
    for (const t of filtered.filter(t => t.direction === "sold" || t.direction === "traded-out")) {
      if (!map[t.item_name]) map[t.item_name] = { item_name: t.item_name, quantity: 0, total_plat: 0 };
      map[t.item_name].quantity   += t.quantity;
      map[t.item_name].total_plat += t.platinum * t.quantity;
    }
    return Object.values(map).sort((a, b) => b.total_plat - a.total_plat).slice(0, 7);
  }, [filtered]);

  const topBought = useMemo((): ItemStat[] => {
    const map: Record<string, ItemStat> = {};
    for (const t of filtered.filter(t => t.direction === "bought" || t.direction === "traded-in")) {
      if (!map[t.item_name]) map[t.item_name] = { item_name: t.item_name, quantity: 0, total_plat: 0 };
      map[t.item_name].quantity   += t.quantity;
      map[t.item_name].total_plat += t.platinum * t.quantity;
    }
    return Object.values(map).sort((a, b) => b.total_plat - a.total_plat).slice(0, 7);
  }, [filtered]);

  const profitChartData = useMemo(() =>
    byCategory
      .filter(c => c.profit > 0)
      .map(c => ({ label: c.category, value: c.profit, color: c.color })),
  [byCategory]);

  const topTradedItems = useMemo(() => {
    const map: Record<string, { total_plat: number; quantity: number }> = {};
    for (const t of filtered) {
      if (!map[t.item_name]) map[t.item_name] = { total_plat: 0, quantity: 0 };
      map[t.item_name].total_plat += t.platinum * t.quantity;
      map[t.item_name].quantity   += t.quantity;
    }
    return Object.entries(map)
      .sort((a, b) => b[1].total_plat - a[1].total_plat || b[1].quantity - a[1].quantity)
      .slice(0, 7)
      .map(([item_name, v]) => ({ item_name, total_plat: v.total_plat, quantity: v.quantity }));
  }, [filtered]);

  const topItemsChartData = useMemo(() =>
    topTradedItems.map((item, i) => ({
      label: item.item_name,
      value: item.total_plat > 0 ? item.total_plat : item.quantity,
      color: Object.values(CATEGORY_COLORS)[i % Object.values(CATEGORY_COLORS).length],
    })),
  [topTradedItems]);

  const sessions = useMemo(() => groupBySessions(filtered), [filtered]);

  const RANGES: { label: string; value: number | "all" }[] = [
    { label: "7d",  value: 7  },
    { label: "30d", value: 30 },
    { label: "90d", value: 90 },
    { label: "All", value: "all" },
  ];

  const topItemsChartForWfm = topItems.slice(0, 7).map((item, i) => ({
    label: item.name,
    value: item.total_value_7d,
    color: Object.values(CATEGORY_COLORS)[i % Object.values(CATEGORY_COLORS).length],
  }));
  const topProgressPercent = topProgress && topProgress.total > 0
    ? Math.round((topProgress.completed / topProgress.total) * 100)
    : 0;

  if (loading) return <div className={RPT_ROOT_CLASS}><div className={RPT_LOADING_CLASS}>Loading…</div></div>;

  return (
    <div className={RPT_ROOT_CLASS}>
      <div className={RPT_SCROLL_CLASS}>

        {/* ── Top WFM items ── always visible, independent of trade history */}
        <div className={RPT_CARD_CLASS}>
          <div className={RPT_CARD_TITLE_CLASS}>Top Warframe.Market items (last 7 days)</div>
          {topLoading ? (
            <div className={RPT_TOP_LOADING_CLASS}>
              <span className={RPT_TOP_SPINNER_CLASS} />
              <div>
                <div>Downloading 7-day statistics from Warframe.Market…</div>
                <div className={RPT_TOP_SOURCE_CLASS}>The first complete ranking can take a few minutes; it refreshes automatically every 3 hours.</div>
                {topProgress && (
                  <div className={RPT_TOP_PROGRESS_CLASS} aria-label="Market ranking progress">
                    <span className={RPT_TOP_PROGRESS_FILL_CLASS} style={{ width: `${topProgressPercent}%` }} />
                    <em className={RPT_TOP_PROGRESS_EM_CLASS}>{topProgress.completed}/{topProgress.total} items</em>
                  </div>
                )}
              </div>
            </div>
          ) : topError ? (
            <div className={`${RPT_TOP_LOADING_CLASS} text-red`}>
              Failed to load market data<br />
              <span className="text-11 text-muted">{topError}</span>
            </div>
          ) : topItems.length === 0 ? (
            <div className={`${RPT_TOP_LOADING_CLASS} text-muted`}>No market data available</div>
          ) : (
            <>
            {topProgress?.refreshing && (
              <div className={RPT_TOP_REFRESHING_CLASS}>
                Updating from Warframe.Market: {topProgress.completed}/{topProgress.total} items ({topProgressPercent}%). Showing the previous ranking until the scan finishes.
              </div>
            )}
            <div className={RPT_TOP_WRAP_CLASS}>
              <table className={`${RPT_TABLE_CLASS} min-w-0 flex-1`}>
                <thead>
                  <tr>
                    <th className={`${RPT_TH_CLASS} text-left`}>Item</th>
                    <th className={`${RPT_TH_CLASS} text-right`}>Unit price</th>
                    <th className={`${RPT_TH_CLASS} text-right`}>Volume (day)</th>
                    <th className={`${RPT_TH_CLASS} text-right`}>Total value</th>
                  </tr>
                </thead>
                <tbody>
                  {topItems.map((item, i) => (
                    <tr key={item.url_name} className={RPT_TBODY_ROW_CLASS}>
                      <td className={RPT_TD_CLASS}>
                        <div className="flex items-center gap-2">
                          <ItemImg imageName={item.image_name} size={24} fallbackText="" />
                          <span className={RPT_DOT_CLASS} style={{ background: topItemsChartForWfm[i]?.color }} />
                          {item.name}
                        </div>
                      </td>
                      <td className={`${RPT_TD_CLASS} ${RPT_TD_NUM_CLASS}`}>{item.unit_price.toLocaleString()} <PlatIcon /></td>
                      <td className={`${RPT_TD_CLASS} ${RPT_TD_NUM_CLASS}`}>{Math.round(item.daily_volume).toLocaleString()}</td>
                      <td className={`${RPT_TD_CLASS} ${RPT_TD_NUM_CLASS} ${RPT_GREEN_CLASS}`}>{fmtK(item.total_value_7d)} <PlatIcon /></td>
                    </tr>
                  ))}
                </tbody>
              </table>
              <div className="shrink-0">
                <DonutChart data={topItemsChartForWfm} />
                <Legend items={topItemsChartForWfm} />
              </div>
            </div>
            </>
          )}
        </div>

        {/* ── Controls row (view toggle + date range) ── */}
        <div className={RPT_RANGE_ROW_CLASS}>
          <div className={RPT_VIEW_TOGGLE_CLASS}>
            <button
              className={`${RPT_RANGE_BTN_CLASS} ${view === "analytics" ? RPT_RANGE_ACTIVE_CLASS : RPT_RANGE_IDLE_CLASS}`}
              onClick={() => setView("analytics")}>Analytics</button>
            <button
              className={`${RPT_RANGE_BTN_CLASS} ${view === "log" ? RPT_RANGE_ACTIVE_CLASS : RPT_RANGE_IDLE_CLASS}`}
              onClick={() => setView("log")}>Log</button>
          </div>
          <span className={RPT_RANGE_LABEL_CLASS}>Period:</span>
          {RANGES.map(r => (
            <button key={String(r.value)}
              className={`${RPT_RANGE_BTN_CLASS} ${dateRange === r.value ? RPT_RANGE_ACTIVE_CLASS : RPT_RANGE_IDLE_CLASS}`}
              onClick={() => onDateRangeChange(r.value)}>
              {r.label}
            </button>
          ))}
          <span className={RPT_TRADE_COUNT_CLASS}>{filtered.length} trade{filtered.length !== 1 ? "s" : ""}</span>
        </div>

        {tradesError ? (
            <div className={RPT_EMPTY_CLASS}>
            <div className={`${RPT_EMPTY_TITLE_CLASS} text-red`}>Failed to load trades</div>
            <div className={RPT_EMPTY_DESC_CLASS}>{tradesError}</div>
          </div>
        ) : trades.length === 0 ? (
          <div className={RPT_EMPTY_CLASS}>
            <div className={RPT_EMPTY_ICON_CLASS}>📊</div>
            <div className={RPT_EMPTY_TITLE_CLASS}>No trade history yet</div>
            <div className={RPT_EMPTY_DESC_CLASS}>
              Trades are automatically recorded from in-game trade sessions.<br />
              Complete a trade in-game and it will appear here.
            </div>
          </div>
        ) : view === "log" ? (
          <div className={RPT_LOG_CLASS}>
            {sessions.length === 0 ? (
              <div className={RPT_EMPTY_CLASS}>
                <div className={RPT_EMPTY_TITLE_CLASS}>No trades in this period</div>
              </div>
            ) : sessions.map(s => <TradeCard key={s.sessionId} session={s} clockFormat={clockFormat} systemLocale={systemLocale} />)}
          </div>
        ) : <>

        {/* ── Summary stats ── */}
        <div className={RPT_SUMMARY_CLASS}>
          <div className={RPT_STAT_CARD_CLASS}>
            <span className={RPT_STAT_LABEL_CLASS}>Total revenue</span>
            <span className={`${RPT_STAT_VALUE_CLASS} ${RPT_GREEN_CLASS}`}>{fmtK(totalRevenue)} <PlatIcon /></span>
          </div>
          <div className={RPT_STAT_CARD_CLASS}>
            <span className={RPT_STAT_LABEL_CLASS}>Total expenses</span>
            <span className={`${RPT_STAT_VALUE_CLASS} ${RPT_RED_CLASS}`}>{fmtK(totalExpenses)} <PlatIcon /></span>
          </div>
          <div className={`${RPT_STAT_CARD_CLASS} ${RPT_STAT_HIGHLIGHT_CLASS}`}>
            <span className={RPT_STAT_LABEL_CLASS}>Profit</span>
            <span className={`${RPT_STAT_VALUE_CLASS} ${profit >= 0 ? RPT_GREEN_CLASS : RPT_RED_CLASS}`}>
              {profit >= 0 ? "+" : ""}{fmtK(profit)} <PlatIcon />
            </span>
          </div>
        </div>

        {/* ── Top items + category breakdown ── */}
        <div className={RPT_ROW_CLASS}>

          {/* Top traded items */}
          <div className={`${RPT_CARD_CLASS} ${RPT_CARD_FLEX_CLASS}`}>
            <div className={RPT_CARD_TITLE_CLASS}>Top traded items</div>
            <div className={RPT_CHART_WRAP_CLASS}>
              <DonutChart data={topItemsChartData} />
              <Legend items={topItemsChartData.map(d => ({ label: d.label, color: d.color, value: d.value }))} />
            </div>
            <table className={RPT_TABLE_CLASS}>
              <thead>
                <tr>
                  <th className={`${RPT_TH_CLASS} text-left`}>Item</th>
                  <th className={`${RPT_TH_CLASS} text-right`}>Total value</th>
                </tr>
              </thead>
              <tbody>
                {topTradedItems.map((item, i) => (
                  <tr key={item.item_name} className={RPT_TBODY_ROW_CLASS}>
                    <td className={RPT_TD_CLASS}>
                      <span className={RPT_DOT_CLASS} style={{ background: topItemsChartData[i]?.color }} />
                      {item.item_name}
                    </td>
                    <td className={`${RPT_TD_CLASS} ${RPT_TD_NUM_CLASS}`}>{item.total_plat > 0 ? <>{fmtK(item.total_plat)} <PlatIcon /></> : <>{item.quantity.toLocaleString()}×</>}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>

          {/* Category breakdown */}
          <div className={`${RPT_CARD_CLASS} ${RPT_CARD_FLEX_CLASS}`}>
            <div className={RPT_CARD_TITLE_CLASS}>Your trade history stats</div>
            <div className={RPT_CHART_WRAP_CLASS}>
              <DonutChart data={profitChartData} />
              <Legend items={profitChartData} />
            </div>
            <table className={RPT_TABLE_CLASS}>
              <thead>
                <tr>
                  <th className={`${RPT_TH_CLASS} text-left`}>Type</th>
                  <th className={`${RPT_TH_CLASS} text-right`}>Revenue</th>
                  <th className={`${RPT_TH_CLASS} text-right`}>Expenses</th>
                  <th className={`${RPT_TH_CLASS} text-right`}>Profit</th>
                </tr>
              </thead>
              <tbody>
                {byCategory.map(cat => (
                  <tr key={cat.category} className={RPT_TBODY_ROW_CLASS}>
                    <td className={RPT_TD_CLASS}>
                      <span className={RPT_DOT_CLASS} style={{ background: cat.color }} />
                      {cat.category}
                    </td>
                    <td className={`${RPT_TD_CLASS} ${RPT_TD_NUM_CLASS}`}>{cat.revenue > 0 ? <>{fmtK(cat.revenue)} <PlatIcon /></> : <span className={RPT_MUTED_CLASS}>–</span>}</td>
                    <td className={`${RPT_TD_CLASS} ${RPT_TD_NUM_CLASS}`}>{cat.expenses > 0 ? <>{fmtK(cat.expenses)} <PlatIcon /></> : <span className={RPT_MUTED_CLASS}>–</span>}</td>
                    <td className={`${RPT_TD_CLASS} ${RPT_TD_NUM_CLASS} ${cat.profit >= 0 ? RPT_GREEN_CLASS : RPT_RED_CLASS}`}>
                      {cat.profit >= 0 ? "+" : ""}{fmtK(cat.profit)} <PlatIcon />
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>

        </div>

        {/* ── Sales / Purchases ── */}
        <div className={RPT_ROW_CLASS}>

          <div className={RPT_CARD_CLASS}>
            <div className={RPT_CARD_TITLE_CLASS}>Sales</div>
            <table className={RPT_TABLE_CLASS}>
              <thead>
                <tr>
                  <th className={`${RPT_TH_CLASS} text-left`}>Item</th>
                  <th className={`${RPT_TH_CLASS} text-right`}>Amount</th>
                  <th className={`${RPT_TH_CLASS} text-right`}>Total value</th>
                </tr>
              </thead>
              <tbody>
                {topSold.length === 0
                  ? <tr className={RPT_TBODY_ROW_CLASS}><td colSpan={3} className={RPT_EMPTY_ROW_CLASS}>No sales recorded</td></tr>
                  : topSold.map(item => (
                    <tr key={item.item_name} className={RPT_TBODY_ROW_CLASS}>
                      <td className={RPT_TD_CLASS}>{item.item_name}</td>
                      <td className={`${RPT_TD_CLASS} ${RPT_TD_NUM_CLASS}`}>{item.quantity.toLocaleString()}</td>
                      <td className={`${RPT_TD_CLASS} ${RPT_TD_NUM_CLASS} ${RPT_GREEN_CLASS}`}>{fmtK(item.total_plat)} <PlatIcon /></td>
                    </tr>
                  ))
                }
              </tbody>
            </table>
          </div>

          <div className={RPT_CARD_CLASS}>
            <div className={RPT_CARD_TITLE_CLASS}>Purchases</div>
            <table className={RPT_TABLE_CLASS}>
              <thead>
                <tr>
                  <th className={`${RPT_TH_CLASS} text-left`}>Item</th>
                  <th className={`${RPT_TH_CLASS} text-right`}>Amount</th>
                  <th className={`${RPT_TH_CLASS} text-right`}>Total value</th>
                </tr>
              </thead>
              <tbody>
                {topBought.length === 0
                  ? <tr className={RPT_TBODY_ROW_CLASS}><td colSpan={3} className={RPT_EMPTY_ROW_CLASS}>No purchases recorded</td></tr>
                  : topBought.map(item => (
                    <tr key={item.item_name} className={RPT_TBODY_ROW_CLASS}>
                      <td className={RPT_TD_CLASS}>{item.item_name}</td>
                      <td className={`${RPT_TD_CLASS} ${RPT_TD_NUM_CLASS}`}>{item.quantity.toLocaleString()}</td>
                      <td className={`${RPT_TD_CLASS} ${RPT_TD_NUM_CLASS} ${RPT_RED_CLASS}`}>{fmtK(item.total_plat)} <PlatIcon /></td>
                    </tr>
                  ))
                }
              </tbody>
            </table>
          </div>

        </div>

        </> /* end analytics view */}

      </div>
    </div>
  );
}
