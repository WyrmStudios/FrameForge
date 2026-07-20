import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import "./ItemMarketPopup.css";

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

interface WfmOrder {
  id?: string;
  platinum: number;
  quantity: number;
  user: { ingameName: string; reputation: number; status: string };
  mod_rank?: number;
}

interface StatPoint { datetime: string; median: number; volume: number; }

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

function Sparkline({ data }: { data: StatPoint[] }) {
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
    <div className="imp-chart-wrap">
      <svg viewBox={`0 0 ${W} ${H}`} className="imp-chart" preserveAspectRatio="none">
        <defs>
          <linearGradient id="cg" x1="0" y1="0" x2="0" y2="1">
            <stop offset="0%" stopColor="var(--accent)" stopOpacity="0.3" />
            <stop offset="100%" stopColor="var(--accent)" stopOpacity="0" />
          </linearGradient>
        </defs>
        <polygon points={area} fill="url(#cg)" />
        <polyline points={polyline} fill="none" stroke="var(--accent)" strokeWidth="1.8" strokeLinejoin="round" />
      </svg>
      <div className="imp-chart-labels">
        <span className="imp-chart-lo">{lo}p</span>
        <span className="imp-chart-last">{last}p now</span>
        <span className="imp-chart-hi">{hi}p</span>
      </div>
    </div>
  );
}

// ── Order row ─────────────────────────────────────────────────────────────────

function StatusDot({ status }: { status: string }) {
  const cls = status === "ingame" ? "imp-dot-ingame" : status === "online" ? "imp-dot-online" : "imp-dot-offline";
  const label = status === "ingame" ? "In Game" : status === "online" ? "Online" : "Offline";
  return <span className={`imp-status-dot ${cls}`} title={label} />;
}

function OrderRow({ o, type, displayName, onList }: {
  o: WfmOrder; type: "sell" | "buy"; displayName?: string; onList?: (price: number) => void;
}) {
  const [copied, setCopied] = useState(false);

  const copyWhisper = () => {
    const action = type === "sell" ? "buy" : "sell";
    const msg = `/w ${o.user.ingameName} Hi! I want to ${action}: "${displayName}" for ${o.platinum} platinum. (warframe.market)`;
    navigator.clipboard.writeText(msg).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    });
  };

  return (
    <div className={`imp-order-row imp-order-${type}${o.user.status === "offline" ? " imp-order-offline" : ""}`}>
      <span className="imp-order-price">{fmt(o.platinum)}p</span>
      <span className="imp-order-qty">×{o.quantity}</span>
      <StatusDot status={o.user.status} />
      <span className="imp-order-user">{o.user.ingameName}</span>
      {o.mod_rank !== undefined && <span className="imp-order-rank">r{o.mod_rank}</span>}
      {displayName && (
        <button
          className={`imp-copy-btn${copied ? " imp-copy-btn-done" : ""}`}
          onClick={copyWhisper}
          title={`/w ${o.user.ingameName} Hi! I want to buy: "${displayName}" for ${o.platinum} platinum. (warframe.market)`}
        >
          {copied ? "✓" : "📋"}
        </button>
      )}
      {onList && (
        <button className="imp-list-btn" onClick={() => onList(o.platinum)} title="List at this price">
          {type === "sell" ? "↓ Match" : "↑ Match"}
        </button>
      )}
    </div>
  );
}

// ── Create order form ─────────────────────────────────────────────────────────

function CreateOrderForm({ urlName, prefillPrice, prefillType, prefillModRank, onDone }: {
  urlName: string; prefillPrice: number; prefillType: "sell" | "buy"; prefillModRank?: number; onDone: () => void;
}) {
  const [orderType, setOrderType] = useState<"sell" | "buy">(prefillType);
  const [price, setPrice]         = useState(Math.round(prefillPrice));
  const [qty, setQty]             = useState(1);
  const [visible, setVisible]     = useState(true);
  const [loading, setLoading]     = useState(false);
  const [error, setError]         = useState("");
  const [success, setSuccess]     = useState(false);

  // Cache item info on mount: needed for itemId and to detect mods (modMaxRank present).
  const [itemId, setItemId]       = useState<string | null>(null);
  const [modMaxRank, setModMaxRank] = useState<number | null>(null);
  const [modRank, setModRank]       = useState(prefillModRank ?? 0);

  useEffect(() => {
    invoke<{ id?: string; modMaxRank?: number } | null>("wfm_get_item_info", { urlName })
      .then(info => {
        if (info?.id) setItemId(info.id);
        if (info?.modMaxRank !== undefined && info.modMaxRank > 0) {
          setModMaxRank(info.modMaxRank);
        }
      })
      .catch(() => {});
  }, [urlName]); // eslint-disable-line

  const submit = async () => {
    setLoading(true); setError("");
    try {
      const id = itemId ?? (await invokeWfm<{ id: string } | null>("wfm_get_item_info", { urlName }).catch(() => null))?.id;
      if (!id) throw new Error("This item isn't individually listed on warframe.market. Try listing the full set instead.");
      await invokeWfm("wfm_create_order", {
        itemId: id, orderType, platinum: price, quantity: qty, visible,
        modRank: modMaxRank !== null ? modRank : null,
      });
      setSuccess(true);
      setTimeout(onDone, 1500);
    } catch (e) { setError(String(e)); }
    setLoading(false);
  };

  return (
    <div className="imp-create-form">
      <div className="imp-create-row">
        <div className="imp-type-btns">
          <button className={orderType === "sell" ? "active" : ""} onClick={() => setOrderType("sell")}>Sell</button>
          <button className={orderType === "buy"  ? "active" : ""} onClick={() => setOrderType("buy")}>Buy</button>
        </div>
        <label>Price</label>
        <input type="number" value={price} min={1} step={1} onChange={e => setPrice(Math.round(+e.target.value))} className="imp-num-input" />
        <span className="imp-plat-label">p</span>
        <label>Qty</label>
        <input type="number" value={qty} min={1} max={99} onChange={e => setQty(+e.target.value)} className="imp-num-input imp-qty-input" />
        {modMaxRank !== null && (
          <>
            <label>Rank</label>
            <input type="number" value={modRank} min={0} max={modMaxRank}
              onChange={e => setModRank(Math.max(0, Math.min(modMaxRank, +e.target.value)))}
              className="imp-num-input imp-qty-input" title={`Mod rank (0–${modMaxRank})`} />
          </>
        )}
        <div className="imp-type-btns" style={{ marginLeft: "auto" }}>
          <button className={visible ? "active" : ""} onClick={() => setVisible(true)} title="Order appears on warframe.market">Visible</button>
          <button className={!visible ? "active" : ""} onClick={() => setVisible(false)} title="Order is saved but hidden from other players">Hidden</button>
        </div>
        <button className="imp-post-btn" onClick={submit} disabled={loading || !price}>
          {loading ? "…" : "Post"}
        </button>
      </div>
      {success && <div className="imp-create-success">✓ Order posted {visible ? "visibly" : "as hidden"}!</div>}
      {!success && error && <div className="imp-create-error">{error}</div>}
    </div>
  );
}

// ── Main popup ────────────────────────────────────────────────────────────────

export default function ItemMarketPopup({ urlName, displayName, imageName, onClose, isLoggedIn, editMode, prefillModRank }: Props) {
  const [orders, setOrders]     = useState<{ sell: WfmOrder[]; buy: WfmOrder[] } | null>(null);
  const [stats, setStats]       = useState<StatPoint[]>([]);
  const [loadingO, setLoadingO] = useState(true);
  const [loadingS, setLoadingS] = useState(true);
  const [ordersError, setOrdersError] = useState("");
  const [showForm, setShowForm] = useState(false);
  const [prefillPrice, setPrefillPrice] = useState(0);
  const [prefillType, setPrefillType]   = useState<"sell" | "buy">("sell");
  const [imgFailed, setImgFailed]       = useState(false);

  useEffect(() => {
    invoke<{ sell: WfmOrder[]; buy: WfmOrder[] }>("wfm_get_item_orders", { urlName })
      .then(o => { setOrders(o); setLoadingO(false); })
      .catch(e => { setOrdersError(String(e)); setLoadingO(false); });
    invoke<StatPoint[]>("wfm_get_item_statistics", { urlName })
      .then(s => { setStats(Array.isArray(s) ? s : []); setLoadingS(false); })
      .catch(() => setLoadingS(false));
  }, [urlName]);

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
    <div className="imp-overlay" onClick={onClose}>
      <div className="imp-modal" onClick={e => e.stopPropagation()}>

        {/* ── Header ── */}
        <div className="imp-header">
          <div className="imp-item-identity">
            {imageName && !imgFailed
              ? <img className="imp-thumb" src={`https://cdn.warframestat.us/img/${imageName}`}
                  alt="" onError={() => setImgFailed(true)} />
              : <div className="imp-thumb-placeholder">P</div>
            }
            <div className="imp-title-group">
              <div className="imp-item-name">{displayName}</div>
              {median48h && <div className="imp-median">48h median <span>{fmt(median48h)}p</span></div>}
            </div>
          </div>
          <button className="imp-close" onClick={onClose}>×</button>
        </div>

        {/* ── Price chart ── */}
        {!loadingS && stats.length > 0 && <Sparkline data={stats} />}

        {/* ── Orders ── */}
        <div className="imp-orders-wrap">
          {/* Sell column */}
          <div className="imp-col">
            <div className="imp-col-header">
              <span>Sellers <span className="imp-col-sub">you buy from</span></span>
              {lowestSell && <span className="imp-col-best">Cheapest: {fmt(lowestSell)}p</span>}
            </div>
            {loadingO ? <div className="imp-loading">Loading…</div> :
             ordersError ? <div className="imp-none" style={{ color: "var(--red)", padding: "8px 10px", fontSize: 11 }}>{ordersError}</div> :
             !orders?.sell.length ? <div className="imp-none">No sellers found</div> :
             orders.sell.map((o, i) => (
               <OrderRow key={i} o={o} type="sell" displayName={displayName}
                 onList={isLoggedIn ? (p) => openForm("sell", p) : undefined} />
             ))
            }
          </div>

          {/* Buy column */}
          <div className="imp-col">
            <div className="imp-col-header">
              <span>Buyers <span className="imp-col-sub">you sell to</span></span>
              {highestBuy && <span className="imp-col-best">Best offer: {fmt(highestBuy)}p</span>}
            </div>
            {loadingO ? <div className="imp-loading">Loading…</div> :
             ordersError ? <div className="imp-none">—</div> :
             !orders?.buy.length ? <div className="imp-none">No buyers found</div> :
             orders.buy.map((o, i) => (
               <OrderRow key={i} o={o} type="buy" displayName={displayName}
                 onList={isLoggedIn ? (p) => openForm("buy", p) : undefined} />
             ))
            }
          </div>
        </div>

        {/* ── Action bar ── */}
        {editMode ? (
          <div className="imp-action-bar imp-edit-bar">
            <span className="imp-edit-label">Price</span>
            <input className="imp-edit-input" type="number" min={1} value={editMode.pt}
              onChange={e => editMode.onPtChange(+e.target.value)} />
            <span className="imp-edit-label">p</span>
            <span className="imp-edit-label" style={{ marginLeft: 8 }}>Qty</span>
            <input className="imp-edit-input imp-edit-input-sm" type="number" min={1} value={editMode.qty}
              onChange={e => editMode.onQtyChange(+e.target.value)} />
            <div className="imp-type-btns" style={{ marginLeft: 8 }}>
              <button className={editMode.visible ? "active" : ""} onClick={() => editMode.onVisibleChange(true)}>Visible</button>
              <button className={!editMode.visible ? "active" : ""} onClick={() => editMode.onVisibleChange(false)}>Hidden</button>
            </div>
            <button className="imp-action-sell" style={{ marginLeft: "auto" }} onClick={editMode.onSave}>Save</button>
            <button className="imp-action-buy" onClick={onClose}>Cancel</button>
          </div>
        ) : isLoggedIn ? (
          <div className="imp-action-bar imp-edit-bar">
            {!showForm ? (
              <>
                <button className="imp-action-sell" onClick={() => openForm("sell")}>
                  + Sell {lowestSell ? `at ${fmt(lowestSell)}p` : ""}
                </button>
                <button className="imp-action-buy" onClick={() => openForm("buy")}>
                  + Buy {highestBuy ? `at ${fmt(highestBuy)}p` : ""}
                </button>
              </>
            ) : (
              <CreateOrderForm urlName={urlName} prefillPrice={prefillPrice} prefillType={prefillType}
                prefillModRank={prefillModRank} onDone={() => { setShowForm(false); }} />
            )}
          </div>
        ) : (
          <div className="imp-login-hint">Log in to warframe.market in the Trading tab to place orders.</div>
        )}
      </div>
    </div>
  );
}
