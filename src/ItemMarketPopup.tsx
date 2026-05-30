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

interface Props {
  urlName: string;
  displayName: string;
  imageName?: string;
  onClose: () => void;
  isLoggedIn: boolean;
  myUsername?: string;
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

function OrderRow({ o, type, onList }: { o: WfmOrder; type: "sell" | "buy"; onList?: (price: number) => void }) {
  return (
    <div className={`imp-order-row imp-order-${type}`}>
      <span className="imp-order-price">{fmt(o.platinum)}p</span>
      <span className="imp-order-qty">×{o.quantity}</span>
      <span className="imp-order-user">{o.user.ingameName}</span>
      {o.mod_rank !== undefined && <span className="imp-order-rank">r{o.mod_rank}</span>}
      {onList && (
        <button className="imp-list-btn" onClick={() => onList(o.platinum)} title="List at this price">
          {type === "sell" ? "↓ Match" : "↑ Match"}
        </button>
      )}
    </div>
  );
}

// ── Create order form ─────────────────────────────────────────────────────────

function CreateOrderForm({ urlName, prefillPrice, prefillType, onDone }: {
  urlName: string; prefillPrice: number; prefillType: "sell" | "buy"; onDone: () => void;
}) {
  const [orderType, setOrderType] = useState<"sell" | "buy">(prefillType);
  const [price, setPrice]         = useState(prefillPrice);
  const [qty, setQty]             = useState(1);
  const [loading, setLoading]     = useState(false);
  const [error, setError]         = useState("");
  const [success, setSuccess]     = useState(false);

  const submit = async () => {
    setLoading(true); setError("");
    try {
      // Try to get the item ID from WFM's item page
      // v2 item info returns { id, slug, ... } directly
      const info = await invokeWfm<{ id: string; slug: string } | null>("wfm_get_item_info", { urlName })
        .catch(() => null);
      const itemId = info?.id;
      if (!itemId) {
        throw new Error("This item isn't individually listed on warframe.market. Try listing the full set instead.");
      }
      await invokeWfm("wfm_create_order", { itemId, orderType, platinum: price, quantity: qty });
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
        <input type="number" value={price} min={1} onChange={e => setPrice(+e.target.value)} className="imp-num-input" />
        <span className="imp-plat-label">p</span>
        <label>Qty</label>
        <input type="number" value={qty} min={1} max={99} onChange={e => setQty(+e.target.value)} className="imp-num-input imp-qty-input" />
        <button className="imp-post-btn" onClick={submit} disabled={loading || !price}>
          {loading ? "…" : "Post"}
        </button>
      </div>
      {success && <div className="imp-create-success">✓ Order posted successfully!</div>}
      {!success && error && <div className="imp-create-error">{error}</div>}
    </div>
  );
}

// ── Main popup ────────────────────────────────────────────────────────────────

export default function ItemMarketPopup({ urlName, displayName, imageName, onClose, isLoggedIn, myUsername }: Props) {
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
    setPrefillPrice(price ?? fallback);
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
              <span>Sell orders</span>
              {lowestSell && <span className="imp-col-best">Lowest: {fmt(lowestSell)}p</span>}
            </div>
            {loadingO ? <div className="imp-loading">Loading…</div> :
             ordersError ? <div className="imp-none" style={{ color: "var(--red)", padding: "8px 10px", fontSize: 11 }}>{ordersError}</div> :
             !orders?.sell.length ? <div className="imp-none">No sellers found</div> :
             orders.sell.map((o, i) => (
               <OrderRow key={i} o={o} type="sell"
                 onList={isLoggedIn ? (p) => openForm("sell", p) : undefined} />
             ))
            }
          </div>

          {/* Buy column */}
          <div className="imp-col">
            <div className="imp-col-header">
              <span>Buy orders</span>
              {highestBuy && <span className="imp-col-best">Highest: {fmt(highestBuy)}p</span>}
            </div>
            {loadingO ? <div className="imp-loading">Loading…</div> :
             ordersError ? <div className="imp-none">—</div> :
             !orders?.buy.length ? <div className="imp-none">No buyers found</div> :
             orders.buy.map((o, i) => (
               <OrderRow key={i} o={o} type="buy"
                 onList={isLoggedIn ? (p) => openForm("buy", p) : undefined} />
             ))
            }
          </div>
        </div>

        {/* ── Action bar ── */}
        {isLoggedIn && (
          <div className="imp-action-bar">
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
                onDone={() => { setShowForm(false); }} />
            )}
          </div>
        )}
        {!isLoggedIn && (
          <div className="imp-login-hint">Log in to warframe.market in the Trading tab to place orders.</div>
        )}
      </div>
    </div>
  );
}
