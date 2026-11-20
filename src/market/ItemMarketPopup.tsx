import { useState, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import ItemImg from "../ItemImg";
import { TAURI_COMMANDS } from "../constants/tauri";
import type { WfmItemInfo, WfmItemOrders, WfmPublicOrder, WfmStatPoint } from "../types/market";
import type { WfmCreateOrderArgs } from "../types/tauri";

// ── Tailwind class constants (formerly ItemMarketPopup.css) ───────────────────

const IMP_OVERLAY =
  "fixed inset-0 bg-black/65 flex items-center justify-center z-200 p-4";
const IMP_MODAL =
  "bg-surface border border-border/80 rounded-10 w-full max-w-170 max-h-[calc(85vh_/_var(--ff-scale,1))] flex flex-col overflow-hidden";

const IMP_HEADER =
  "flex items-center justify-between pt-3.5 px-4 pb-3 border-b border-b-border/50 shrink-0";
const IMP_ITEM_IDENTITY = "flex items-center gap-3";
const IMP_THUMB =
  "w-13 h-13 object-contain rounded-6 bg-white/5";
const IMP_THUMB_PLACEHOLDER =
  "w-13 h-13 rounded-6 bg-white/7 flex items-center justify-center text-20 font-bold text-muted";
const IMP_ITEM_NAME = "text-17 font-bold text-foreground";
const IMP_MEDIAN = "text-12 text-muted mt-0.5";
const IMP_MEDIAN_VALUE = "text-ducat font-semibold";
const IMP_CLOSE =
  "bg-transparent border-0 text-muted text-22 cursor-pointer px-1 leading-none transition-colors duration-100 self-start hover:text-danger";

const IMP_CHART_WRAP = "px-4 pt-2 pb-1 shrink-0";
const IMP_CHART = "w-full h-14 block";
const IMP_CHART_LABELS = "flex justify-between text-10 text-muted mt-0.5";
const IMP_CHART_LAST = "text-accent font-semibold";

const IMP_RANK_ROW =
  "flex items-center gap-1.5 px-4 py-1.5 border-b border-b-[var(--color-border)] shrink-0";
const IMP_RANK_LABEL = "text-11 text-muted";

const IMP_ORDERS_WRAP =
  "grid grid-cols-2 gap-0 flex-1 overflow-hidden border-t border-t-border/40";
const IMP_COL =
  "flex flex-col min-h-0 overflow-x-hidden overflow-y-auto first:border-r first:border-r-border/40";
const IMP_COL_HEADER =
  "flex justify-between items-center px-2.5 py-1.5 text-10 font-bold uppercase tracking-0.04 text-muted border-b border-b-border/30 shrink-0";
const IMP_COL_BEST = "font-bold text-ducat";
const IMP_COL_SUB = "text-10 font-normal text-muted ml-1";
const IMP_MSG = "px-2.5 py-3 text-11 text-muted text-center";

const IMP_ORDER_ROW =
  "flex items-center gap-1.5 px-2.5 py-1.25 border-b border-b-border/20 text-12 transition-[background] duration-100 last:border-b-0 hover:bg-white/4";
const IMP_STATUS_DOT = "w-1.5 h-1.5 rounded-full shrink-0";
const STATUS_DOT: Record<string, string> = {
  ingame: IMP_STATUS_DOT + " bg-success",
  online: IMP_STATUS_DOT + " bg-connected",
  offline: IMP_STATUS_DOT + " bg-muted",
};
const IMP_ORDER_PRICE = "font-bold tabular-nums min-w-13";
const IMP_ORDER_PRICE_SELL = IMP_ORDER_PRICE + " text-success";
const IMP_ORDER_PRICE_BUY = IMP_ORDER_PRICE + " text-accent";
const IMP_ORDER_QTY = "text-11 text-muted min-w-6";
const IMP_ORDER_USER =
  "flex-1 whitespace-nowrap overflow-hidden text-ellipsis min-w-0 text-foreground text-11";
const IMP_ORDER_RANK =
  "text-10 text-muted bg-white/8 px-1.25 py-0.25 rounded-3 shrink-0";
const IMP_LIST_BTN =
  "bg-transparent border border-border/60 text-muted text-10 px-1.5 py-0.25 rounded-3 cursor-pointer shrink-0 whitespace-nowrap transition-[background,color,border-color] duration-100 hover:bg-accent/15 hover:border-accent hover:text-accent";
const IMP_COPY_BTN =
  "bg-transparent border border-border/60 text-muted text-11 px-1.25 py-0.25 rounded-3 cursor-pointer shrink-0 leading-1.4 transition-[background,border-color,color] duration-100 hover:bg-white/6 hover:border-copy-hover/60 hover:text-foreground";
const IMP_COPY_BTN_DONE =
  IMP_COPY_BTN + " border-confirm/50! text-success!";

const IMP_ACTION_BAR =
  "px-3.5 py-2.5 border-t border-t-border/50 shrink-0 bg-black/12";
const IMP_EDIT_BAR = "flex items-center gap-1.5";
const IMP_ACTION_SELL =
  "text-12 font-semibold px-4 py-1.5 rounded-5 cursor-pointer border mr-2 transition-[background] duration-100 bg-success/15 border-[var(--color-success)] text-success hover:bg-success/28";
const IMP_ACTION_BUY =
  "text-12 font-semibold px-4 py-1.5 rounded-5 cursor-pointer border mr-2 transition-[background] duration-100 bg-accent/15 border-accent text-accent hover:bg-accent/28";
const IMP_EDIT_LABEL = "text-12 text-muted";
const IMP_EDIT_INPUT =
  "bg-[var(--color-background)] border border-[var(--color-border)] rounded-5 text-foreground text-13 px-1.75 py-1 w-17.5 outline-none focus:border-accent";
const IMP_EDIT_INPUT_SM =
  "bg-[var(--color-background)] border border-[var(--color-border)] rounded-5 text-foreground text-13 px-1.75 py-1 w-12 outline-none focus:border-accent";

const IMP_CREATE_FORM = "flex flex-col gap-1.5";
const IMP_CREATE_ROW = "flex items-center gap-2 flex-wrap";
const IMP_CREATE_LABEL = "text-11 text-muted shrink-0";
const IMP_TYPE_BTNS = "flex gap-0.75";
const IMP_TYPE_BTN =
  "bg-white/5 border border-border/60 text-muted text-11 px-2.5 py-0.75 rounded-4 cursor-pointer";
const IMP_TYPE_BTN_ACTIVE =
  "bg-accent/20 border border-accent text-accent text-11 px-2.5 py-0.75 rounded-4 cursor-pointer";
const IMP_NUM_INPUT =
  "bg-black/25 border border-border/80 rounded-4 text-foreground text-12 px-2 py-1 outline-none w-18 focus:border-accent";
const IMP_NUM_INPUT_SM =
  "bg-black/25 border border-border/80 rounded-4 text-foreground text-12 px-2 py-1 outline-none w-12 focus:border-accent";
const IMP_PLAT_LABEL = "text-12 text-ducat";
const IMP_POST_BTN =
  "bg-accent border-0 rounded-5 text-white text-12 font-semibold px-4 py-1.25 cursor-pointer transition-[opacity] duration-100 hover:opacity-85 disabled:opacity-40 disabled:cursor-default";
const IMP_CREATE_ERROR = "text-11 text-danger";
const IMP_CREATE_SUCCESS = "text-12 font-semibold text-success";
const IMP_LOGIN_HINT =
  "px-3.5 py-2.5 text-11 text-muted text-center border-t border-t-border/50 bg-black/12";

async function invokeWfm<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(command, args);
  } catch (e) {
    if (String(e).includes("401")) {
      const refreshed = await invoke("wfm_refresh_token").then(() => true).catch(() => false);
      if (refreshed) return await invoke<T>(command, args);
      throw new Error("Session expired. Please log in again in the Trading tab.");
    }
    throw e;
  }
}

// ── Types ─────────────────────────────────────────────────────────────────────

interface EditMode {
  pt: number;
  qty: number;
  visible: boolean;
  onPtChange: (v: number) => void;
  onQtyChange: (v: number) => void;
  onVisibleChange: (v: boolean) => void;
  onSave: () => void;
}

interface Props {
  urlName: string;
  displayName: string;
  imageName?: string;
  onClose: () => void;
  isLoggedIn: boolean;
  myUsername?: string;
  editMode?: EditMode;
  /** Pre-selected mod rank when opening from a rank-specific inventory chip. */
  prefillModRank?: number;
}

function fmt(n: number) { return Math.round(n).toLocaleString(); }

// ── Sparkline ─────────────────────────────────────────────────────────────────

function Sparkline({ data }: { data: WfmStatPoint[] }) {
  const pts = data.slice(-21); // last 3 weeks
  if (pts.length < 2) return null;
  const prices = pts.map(d => d.median);
  const W = 320, H = 56;
  const min = Math.min(...prices) * 0.97;
  const max = Math.max(...prices) * 1.03;
  const range = max - min || 1;
  const x = (i: number) => (i / (pts.length - 1)) * W;
  const y = (p: number) => H - ((p - min) / range) * H;
  const polyline = pts.map((p, i) => `${x(i)},${y(p.median)}`).join(" ");
  const area = `${x(0)},${H} ` + polyline + ` ${x(pts.length - 1)},${H}`;
  const lo = fmt(Math.min(...prices));
  const hi = fmt(Math.max(...prices));
  const last = fmt(prices[prices.length - 1]);

  return (
    <div className={IMP_CHART_WRAP}>
      <svg viewBox={`0 0 ${W} ${H}`} className={IMP_CHART} preserveAspectRatio="none">
        <defs>
          <linearGradient id="cg" x1="0" y1="0" x2="0" y2="1">
            <stop offset="0%" stopColor="var(--color-accent)" stopOpacity="0.3" />
            <stop offset="100%" stopColor="var(--color-accent)" stopOpacity="0" />
          </linearGradient>
        </defs>
        <polygon points={area} fill="url(#cg)" />
        <polyline points={polyline} fill="none" stroke="var(--color-accent)" strokeWidth="1.8" strokeLinejoin="round" />
      </svg>
      <div className={IMP_CHART_LABELS}>
        <span>{lo}p</span>
        <span className={IMP_CHART_LAST}>{last}p now</span>
        <span>{hi}p</span>
      </div>
    </div>
  );
}

// ── Order row ─────────────────────────────────────────────────────────────────

function StatusDot({ status }: { status: string }) {
  const label = status === "ingame" ? "In Game" : status === "online" ? "Online" : "Offline";
  return <span className={STATUS_DOT[status] ?? STATUS_DOT.offline} title={label} />;
}

function OrderRow({ o, type, displayName, onList }: {
  o: WfmPublicOrder; type: "sell" | "buy"; displayName?: string; onList?: (price: number) => void;
}) {
  const [copied, setCopied] = useState(false);

  const action = type === "sell" ? "buy" : "sell";
  const whisper = `/w ${o.user.ingameName} Hi! I want to ${action}: "${displayName}" for ${o.platinum} platinum. (warframe.market)`;

  const copyWhisper = () => {
    navigator.clipboard.writeText(whisper).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    });
  };

  return (
    <div className={IMP_ORDER_ROW + (o.user.status === "offline" ? " opacity-50" : "")}>
      <span className={type === "sell" ? IMP_ORDER_PRICE_SELL : IMP_ORDER_PRICE_BUY}>{fmt(o.platinum)}p</span>
      <span className={IMP_ORDER_QTY}>×{o.quantity}</span>
      <StatusDot status={o.user.status} />
      <span className={IMP_ORDER_USER}>{o.user.ingameName}</span>
      {o.mod_rank !== undefined && <span className={IMP_ORDER_RANK}>r{o.mod_rank}</span>}
      {displayName && (
        <button
          className={copied ? IMP_COPY_BTN_DONE : IMP_COPY_BTN}
          onClick={copyWhisper}
          title={whisper}
        >
          {copied ? "✓" : "📋"}
        </button>
      )}
      {onList && (
        <button className={IMP_LIST_BTN} onClick={() => onList(o.platinum)} title="List at this price">
          {type === "sell" ? "↓ Match" : "↑ Match"}
        </button>
      )}
    </div>
  );
}

// ── Create order form ─────────────────────────────────────────────────────────

function CreateOrderForm({ urlName, itemId, prefillPrice, prefillType, modRank, modMaxRank, onModRankChange, onDone }: {
  urlName: string;
  itemId: string | null;
  prefillPrice: number;
  prefillType: "sell" | "buy";
  modRank: number;
  modMaxRank: number | null;
  onModRankChange: (r: number) => void;
  onDone: () => void;
}) {
  const [orderType, setOrderType] = useState<"sell" | "buy">(prefillType);
  const [price, setPrice]         = useState(Math.round(prefillPrice));
  const [qty, setQty]             = useState(1);
  const [visible, setVisible]     = useState(true);
  const [loading, setLoading]     = useState(false);
  const [error, setError]         = useState("");
  const [success, setSuccess]     = useState(false);

  const submit = async () => {
    setLoading(true); setError("");
    try {
      const id = itemId ?? (await invokeWfm<WfmItemInfo | null>(TAURI_COMMANDS.WFM_GET_ITEM_INFO, { urlName }).catch(() => null))?.id;
      if (!id) throw new Error("This item isn't individually listed on warframe.market. Try listing the full set instead.");
      const args: WfmCreateOrderArgs = {
        itemId: id, orderType, platinum: price, quantity: qty, visible,
        modRank: modMaxRank !== null ? modRank : null,
      };
      await invokeWfm(TAURI_COMMANDS.WFM_CREATE_ORDER, args);
      setSuccess(true);
      setTimeout(onDone, 1500);
    } catch (e) { setError(String(e)); }
    setLoading(false);
  };

  return (
    <div className={IMP_CREATE_FORM}>
      <div className={IMP_CREATE_ROW}>
        <div className={IMP_TYPE_BTNS}>
          <button className={orderType === "sell" ? IMP_TYPE_BTN_ACTIVE : IMP_TYPE_BTN} onClick={() => setOrderType("sell")}>Sell</button>
          <button className={orderType === "buy" ? IMP_TYPE_BTN_ACTIVE : IMP_TYPE_BTN} onClick={() => setOrderType("buy")}>Buy</button>
        </div>
        <label className={IMP_CREATE_LABEL}>Price</label>
        <input type="number" value={price} min={1} step={1} onChange={e => setPrice(Math.round(+e.target.value))} className={IMP_NUM_INPUT} />
        <span className={IMP_PLAT_LABEL}>p</span>
        <label className={IMP_CREATE_LABEL}>Qty</label>
        <input type="number" value={qty} min={1} max={99} onChange={e => setQty(+e.target.value)} className={IMP_NUM_INPUT_SM} />
        {modMaxRank !== null && (
          <>
            <label className={IMP_CREATE_LABEL}>Rank</label>
            <input type="number" value={modRank} min={0} max={modMaxRank}
              onChange={e => onModRankChange(Math.max(0, Math.min(modMaxRank, +e.target.value)))}
              className={IMP_NUM_INPUT_SM} title={`Mod rank (0–${modMaxRank})`} />
          </>
        )}
        <div className={IMP_TYPE_BTNS + " ml-auto"}>
          <button className={visible ? IMP_TYPE_BTN_ACTIVE : IMP_TYPE_BTN} onClick={() => setVisible(true)} title="Order appears on warframe.market">Visible</button>
          <button className={!visible ? IMP_TYPE_BTN_ACTIVE : IMP_TYPE_BTN} onClick={() => setVisible(false)} title="Order is saved but hidden from other players">Hidden</button>
        </div>
        <button className={IMP_POST_BTN} onClick={submit} disabled={loading || !price}>
          {loading ? "…" : "Post"}
        </button>
      </div>
      {success && <div className={IMP_CREATE_SUCCESS}>✓ Order posted {visible ? "visibly" : "as hidden"}!</div>}
      {!success && error && <div className={IMP_CREATE_ERROR}>{error}</div>}
    </div>
  );
}

// ── Main popup ────────────────────────────────────────────────────────────────

export default function ItemMarketPopup({ urlName, displayName, imageName, onClose, isLoggedIn, editMode, prefillModRank }: Props) {
  const [orders, setOrders]     = useState<{ sell: WfmPublicOrder[]; buy: WfmPublicOrder[] } | null>(null);
  const [stats, setStats]       = useState<WfmStatPoint[]>([]);
  const [loadingO, setLoadingO] = useState(true);
  const [loadingS, setLoadingS] = useState(true);
  const [ordersError, setOrdersError] = useState("");
  const [showForm, setShowForm] = useState(false);
  const [prefillPrice, setPrefillPrice] = useState(0);
  const [prefillType, setPrefillType]   = useState<"sell" | "buy">("sell");

  // Mod rank state — lifted here so orders re-fetch when rank changes
  const [modRankInput, setModRankInput] = useState(prefillModRank ?? 0);
  const [modRank, setModRank]           = useState(prefillModRank ?? 0);
  const [modMaxRank, setModMaxRank]     = useState<number | null>(null);
  const [itemId, setItemId]             = useState<string | null>(null);
  const debounceRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const handleModRankChange = (r: number) => {
    setModRankInput(r);
    if (debounceRef.current) clearTimeout(debounceRef.current);
    debounceRef.current = setTimeout(() => setModRank(r), 350);
  };

  // Fetch item info once to determine modMaxRank and itemId
  useEffect(() => {
    invoke<WfmItemInfo | null>(TAURI_COMMANDS.WFM_GET_ITEM_INFO, { urlName })
      .then(info => {
        if (info?.id) setItemId(info.id);
        if (info?.modMaxRank !== undefined && info.modMaxRank > 0) setModMaxRank(info.modMaxRank);
      })
      .catch(() => {});
    invoke<WfmStatPoint[]>("wfm_get_item_statistics", { urlName })
      .then(s => { setStats(Array.isArray(s) ? s : []); setLoadingS(false); })
      .catch(() => setLoadingS(false));
  }, [urlName]); // eslint-disable-line

  // Re-fetch orders whenever the item or the active mod rank changes
  useEffect(() => {
    setLoadingO(true);
    setOrdersError("");
    invoke<WfmItemOrders>("wfm_get_item_orders", {
      urlName,
      modRank: modMaxRank !== null ? modRank : undefined,
    })
      .then(o => { setOrders(o); setLoadingO(false); })
      .catch(e => { setOrdersError(String(e)); setLoadingO(false); });
  }, [urlName, modRank, modMaxRank]); // eslint-disable-line

  const lowestSell  = orders?.sell[0]?.platinum;
  const highestBuy  = orders?.buy[0]?.platinum;
  const median48h   = stats.slice(-2)[0]?.median;

  const openForm = (type: "sell" | "buy", price?: number) => {
    const fallback = type === "sell" ? (lowestSell ?? median48h ?? 0) : (highestBuy ?? median48h ?? 0);
    // Always whole platinum — market shows decimals but you can't trade fractions.
    setPrefillPrice(Math.round(price ?? fallback));
    setPrefillType(type);
    setShowForm(true);
  };

  return (
    <div className={IMP_OVERLAY} onClick={onClose}>
      <div className={IMP_MODAL} onClick={e => e.stopPropagation()}>

        {/* ── Header ── */}
        <div className={IMP_HEADER}>
          <div className={IMP_ITEM_IDENTITY}>
            <ItemImg imageName={imageName} size={52}
              className={IMP_THUMB} fallbackClassName={IMP_THUMB_PLACEHOLDER} fallbackText="P" />
            <div>
              <div className={IMP_ITEM_NAME}>{displayName}</div>
              {median48h && <div className={IMP_MEDIAN}>48h median <span className={IMP_MEDIAN_VALUE}>{fmt(median48h)}p</span></div>}
            </div>
          </div>
          <button className={IMP_CLOSE} onClick={onClose}>×</button>
        </div>

        {/* ── Price chart ── */}
        {!loadingS && stats.length > 0 && <Sparkline data={stats} />}

        {/* ── Rank selector (mods/arcanes only) ── */}
        {modMaxRank !== null && (
          <div className={IMP_RANK_ROW}>
            <span className={IMP_RANK_LABEL}>Showing rank</span>
            <input
              type="number" value={modRankInput} min={0} max={modMaxRank}
              onChange={e => handleModRankChange(Math.max(0, Math.min(modMaxRank, +e.target.value)))}
              className={IMP_NUM_INPUT_SM}
            />
            <span className={IMP_RANK_LABEL}>/ {modMaxRank}</span>
          </div>
        )}

        {/* ── Orders ── */}
        <div className={IMP_ORDERS_WRAP}>
          {/* Sell column */}
          <div className={IMP_COL}>
            <div className={IMP_COL_HEADER}>
              <span>Sellers <span className={IMP_COL_SUB}>you buy from</span></span>
              {lowestSell && <span className={IMP_COL_BEST}>Cheapest: {fmt(lowestSell)}p</span>}
            </div>
            {loadingO ? <div className={IMP_MSG}>Loading…</div> :
             ordersError ? <div className={IMP_MSG + " px-2.5! py-2! text-11! text-danger!"}>{ordersError}</div> :
             !orders?.sell.length ? <div className={IMP_MSG}>No sellers found</div> :
             orders.sell.map((o, i) => (
               <OrderRow key={i} o={o} type="sell" displayName={displayName}
                 onList={isLoggedIn ? (p) => openForm("sell", p) : undefined} />
             ))
            }
          </div>

          {/* Buy column */}
          <div className={IMP_COL}>
            <div className={IMP_COL_HEADER}>
              <span>Buyers <span className={IMP_COL_SUB}>you sell to</span></span>
              {highestBuy && <span className={IMP_COL_BEST}>Best offer: {fmt(highestBuy)}p</span>}
            </div>
            {loadingO ? <div className={IMP_MSG}>Loading…</div> :
             ordersError ? <div className={IMP_MSG}>—</div> :
             !orders?.buy.length ? <div className={IMP_MSG}>No buyers found</div> :
             orders.buy.map((o, i) => (
               <OrderRow key={i} o={o} type="buy" displayName={displayName}
                 onList={isLoggedIn ? (p) => openForm("buy", p) : undefined} />
             ))
            }
          </div>
        </div>

        {/* ── Action bar ── */}
        {editMode ? (
          <div className={IMP_ACTION_BAR + " " + IMP_EDIT_BAR}>
            <span className={IMP_EDIT_LABEL}>Price</span>
            <input className={IMP_EDIT_INPUT} type="number" min={1} value={editMode.pt}
              onChange={e => editMode.onPtChange(+e.target.value)} />
            <span className={IMP_EDIT_LABEL}>p</span>
            <span className={IMP_EDIT_LABEL + " ml-2"}>Qty</span>
            <input className={IMP_EDIT_INPUT_SM} type="number" min={1} value={editMode.qty}
              onChange={e => editMode.onQtyChange(+e.target.value)} />
            <div className={IMP_TYPE_BTNS + " ml-2"}>
              <button className={editMode.visible ? IMP_TYPE_BTN_ACTIVE : IMP_TYPE_BTN} onClick={() => editMode.onVisibleChange(true)}>Visible</button>
              <button className={!editMode.visible ? IMP_TYPE_BTN_ACTIVE : IMP_TYPE_BTN} onClick={() => editMode.onVisibleChange(false)}>Hidden</button>
            </div>
            <button className={IMP_ACTION_SELL + " ml-auto"} onClick={editMode.onSave}>Save</button>
            <button className={IMP_ACTION_BUY} onClick={onClose}>Cancel</button>
          </div>
        ) : isLoggedIn ? (
          <div className={IMP_ACTION_BAR + " " + IMP_EDIT_BAR}>
            {!showForm ? (
              <>
                <button className={IMP_ACTION_SELL} onClick={() => openForm("sell")}>
                  + Sell {lowestSell ? `at ${fmt(lowestSell)}p` : ""}
                </button>
                <button className={IMP_ACTION_BUY} onClick={() => openForm("buy")}>
                  + Buy {highestBuy ? `at ${fmt(highestBuy)}p` : ""}
                </button>
              </>
            ) : (
              <CreateOrderForm urlName={urlName} itemId={itemId} prefillPrice={prefillPrice} prefillType={prefillType}
                modRank={modRankInput} modMaxRank={modMaxRank} onModRankChange={handleModRankChange}
                onDone={() => { setShowForm(false); }} />
            )}
          </div>
        ) : (
          <div className={IMP_LOGIN_HINT}>Log in to warframe.market in the Trading tab to place orders.</div>
        )}
      </div>
    </div>
  );
}
