import { useState, useEffect, useCallback, useRef, useMemo } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import ItemMarketPopup from "./ItemMarketPopup";
import "./WfmTrading.css";

// ── Types ─────────────────────────────────────────────────────────────────────

interface WfmOrder {
  id: string;
  itemId?: string;
  type: "sell" | "buy";
  platinum: number;
  quantity: number;
  visible: boolean;
  item?: { slug?: string; urlName?: string; url_name?: string; en?: { item_name: string }; i18n?: { en?: { name: string } }; };
}

interface WfmWhisper {
  from: string;
  message: string;
  item?: string;
  price?: number;
  timestamp: string;
  /** Set when auto-completed by in-game trade detection. Ghost stays visible for 5 min. */
  completedAt?: number;
  /** Set after WFM listing is updated so the ghost can offer a Revert button. */
  revertInfo?: {
    orderId: string;
    itemId: string;
    platinum: number;
    originalQty: number;
    newQty: number;       // 0 means the order was deleted
    visible: boolean;
  };
}

interface WfmItemEntry { id: string; item_name: string; url_name: string; }

interface WfmRivenAttribute {
  url_name: string;
  positive: boolean;
  value:    number;
}

interface WfmAuction {
  id:                 string;
  starting_price:     number;
  buyout_price:       number | null;
  top_bid:            number | null;
  bids:               number;
  winner:             { ingame_name: string } | null;
  is_closed:          boolean;
  is_direct_sell:     boolean;
  visible:            boolean;
  note:               string;
  minimal_reputation: number;
  item: {
    weapon_url_name: string;
    name:            string;
    mastery_level:   number;
    mod_rank:        number;
    re_rolls:        number;
    polarity:        string;
    attributes:      WfmRivenAttribute[];
  };
}

interface Props {
  wfmLookup: Map<string, string>;
  wfmItems: WfmItemEntry[];
  imageMap: Map<string, string>;
  inventory: Record<string, unknown>;
  onNewWhisper: () => void;
  onLoginChange: (username: string | null) => void;
  auctionRefreshKey?: number;
}

function fmt(n: number) { return n.toLocaleString(); }

/** Debug helpers available from the browser console:
 *  window.__wfmDump('/v2/orders/my')   — raw JSON from any authenticated WFM endpoint
 *  window.__wfmAttrs()                  — list all valid riven attribute url_names
 */
if (typeof window !== "undefined") {
  (window as unknown as Record<string, unknown>).__wfmDump = async (path: string) => {
    const result = await invoke<string>("wfm_debug_dump", { path }).catch(e => String(e));
    console.log(result);
    return result;
  };
  (window as unknown as Record<string, unknown>).__wfmAttrs = async () => {
    const list = await invoke<string[]>("wfm_get_riven_attributes").catch(e => [String(e)]);
    console.log(list.join("\n"));
    return list;
  };
}

/** Invoke a WFM command. On 401, the v1 token has expired — surface SESSION_EXPIRED. */
async function invokeWfm<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(command, args);
  } catch (e) {
    if (String(e).includes("401")) {
      throw new Error("SESSION_EXPIRED");
    }
    throw e;
  }
}

// ── Login panel ───────────────────────────────────────────────────────────────

function LoginPanel({ onLogin }: { onLogin: (u: string) => void }) {
  const [email, setEmail]       = useState("");
  const [password, setPassword] = useState("");
  const [remember, setRemember] = useState(true);
  const [loading, setLoading]   = useState(false);
  const [error, setError]       = useState("");

  const submit = async () => {
    if (!email || !password) return;
    setLoading(true); setError("");
    try {
      const username = await invoke<string>("wfm_login", { email, password });
      if (remember) {
        const tokenJson = await invoke<string | null>("wfm_get_jwt").catch(() => null);
        if (tokenJson) invoke("wfm_save_credentials", { email, password: tokenJson }).catch(() => {});
      }
      onLogin(username);
    } catch (e) { setError(String(e)); setLoading(false); }
  };

  return (
    <div className="wfm-login-wrap">
      <div className="wfm-login-card">
        <div className="wfm-login-title">Connect warframe.market</div>
        <p className="wfm-login-desc">Log in to view live orders, manage listings, and receive trade whispers.</p>

        <div className="wfm-field">
          <label>Email</label>
          <input type="email" value={email} onChange={e => setEmail(e.target.value)}
            onKeyDown={e => e.key === "Enter" && submit()} autoComplete="email" />
        </div>
        <div className="wfm-field">
          <label>Password</label>
          <input type="password" value={password} onChange={e => setPassword(e.target.value)}
            onKeyDown={e => e.key === "Enter" && submit()} />
        </div>
        {error && <div className="wfm-error">{error}</div>}
        <label className="wfm-remember-row">
          <input type="checkbox" checked={remember} onChange={e => setRemember(e.target.checked)} />
          Remember credentials
        </label>
        <button className="wfm-btn-primary" onClick={submit} disabled={loading || !email || !password}>
          {loading ? "Logging in…" : "Log in"}
        </button>

        <p className="wfm-alt-login-note">
          Using Steam, Xbox, Discord, or GitHub to log in to warframe.market? You'll need to create
          email/password credentials first — go to{" "}
          <a href="https://warframe.market/settings/account" target="_blank" rel="noreferrer">
            warframe.market/settings/account
          </a>{" "}
          and fill in <strong>Create credentials</strong>, then use those here.
        </p>
      </div>
    </div>
  );
}

// ── Listings panel ────────────────────────────────────────────────────────────

function orderName(o: WfmOrder, itemIdMap: Map<string, string>): string {
  return (
    o.item?.i18n?.en?.name
    ?? o.item?.en?.item_name
    ?? o.item?.urlName
    ?? o.item?.url_name
    ?? (o.item?.slug ? (o.item.slug as string).replace(/_/g, ' ').replace(/\b\w/g, (c: string) => c.toUpperCase()) : null)
    ?? ((o as unknown as Record<string, unknown>).itemId ? itemIdMap.get((o as unknown as Record<string, unknown>).itemId as string) : null)
    ?? "—"
  );
}

function isRivenOrder(o: WfmOrder, itemIdMap: Map<string, string>): boolean {
  const url = (o.item?.urlName ?? o.item?.url_name ?? o.item?.slug ?? "").toLowerCase();
  const name = orderName(o, itemIdMap).toLowerCase();
  return url.includes("riven") || name.includes("riven");
}

function AuctionEditPopup({ auction, onSave, onClose }: {
  auction: WfmAuction;
  onSave: (id: string, start: number, buyout: number | null, visible: boolean, newIsDirect: boolean) => Promise<void>;
  onClose: () => void;
}) {
  const weaponName = auction.item.weapon_url_name.replace(/_/g, " ").replace(/\b\w/g, c => c.toUpperCase());
  const modName    = auction.item.name ? auction.item.name.charAt(0).toUpperCase() + auction.item.name.slice(1) : "";

  const [isDirect, setIsDirect]       = useState(auction.is_direct_sell);
  const [price, setPrice]             = useState(auction.buyout_price ?? auction.starting_price);
  const [startPrice, setStartPrice]   = useState(auction.starting_price);
  const [buyoutPrice, setBuyoutPrice] = useState(auction.buyout_price != null ? String(auction.buyout_price) : "");
  const [visible, setVisible]         = useState(auction.visible);
  const [busy, setBusy]               = useState(false);
  const [error, setError]             = useState("");

  const switchType = (newDirect: boolean) => {
    if (newDirect) {
      setPrice(auction.buyout_price ?? auction.starting_price);
    } else {
      setStartPrice(auction.starting_price);
      setBuyoutPrice(auction.buyout_price != null ? String(auction.buyout_price) : "");
    }
    setIsDirect(newDirect);
  };

  const save = async () => {
    setBusy(true); setError("");
    try {
      if (isDirect) {
        const p = Math.max(1, Math.round(price));
        await onSave(auction.id, p, p, visible, true);
      } else {
        const start  = Math.max(1, Math.round(startPrice));
        const buyout = buyoutPrice.trim() === "" ? null : Math.max(1, Math.round(+buyoutPrice));
        await onSave(auction.id, start, buyout, visible, false);
      }
    } catch (e) { setError(String(e)); setBusy(false); }
  };

  const typeChanged = isDirect !== auction.is_direct_sell;

  return (
    <div className="wfm-ae-popup-overlay" onClick={onClose}>
      <div className="wfm-ae-popup-card" onClick={e => e.stopPropagation()}>
        <div className="wfm-ae-popup-header">
          <span className="wfm-ae-popup-title">
            {weaponName}{modName && <em className="wfm-riven-mod"> {modName}</em>}
          </span>
          <button className="wfm-logout-btn" style={{ fontSize: 18 }} onClick={onClose}>×</button>
        </div>
        <div className="wfm-ae-popup-body">
          <div className="wfm-ae-popup-field">
            <label>Listing type</label>
            <div className="wfm-ae-vis-row">
              <button className={`wfm-btn-sm wfm-ae-vis${!isDirect ? " active" : ""}`} onClick={() => switchType(false)}>Auction</button>
              <button className={`wfm-btn-sm wfm-ae-vis${isDirect ? " active" : ""}`} onClick={() => switchType(true)}>Direct Sale</button>
            </div>
            {typeChanged && (
              <span className="wfm-ae-popup-hint wfm-ae-type-warn">
                ⚠ Switching type deletes and recreates the listing — may take up to 20 s due to WFM rate limits
              </span>
            )}
          </div>

          {isDirect ? (
            <div className="wfm-ae-popup-field">
              <label>Price</label>
              <div className="wfm-ae-popup-input-row">
                <input type="number" min={1} className="wfm-ae-input" value={price}
                  onChange={e => setPrice(+e.target.value)} />
                <span className="wfm-plat">p</span>
              </div>
            </div>
          ) : (
            <>
              <div className="wfm-ae-popup-field">
                <label>Start price</label>
                <div className="wfm-ae-popup-input-row">
                  <input type="number" min={1} className="wfm-ae-input" value={startPrice}
                    onChange={e => setStartPrice(+e.target.value)} />
                  <span className="wfm-plat">p</span>
                </div>
              </div>
              <div className="wfm-ae-popup-field">
                <label>Buyout price</label>
                <div className="wfm-ae-popup-input-row">
                  <input type="number" min={1} placeholder="none" className="wfm-ae-input" value={buyoutPrice}
                    onChange={e => setBuyoutPrice(e.target.value)} />
                  <span className="wfm-plat">p</span>
                  <span className="wfm-ae-popup-hint">empty = no buyout</span>
                </div>
              </div>
            </>
          )}

          <div className="wfm-ae-popup-field">
            <label>Visibility</label>
            <div className="wfm-ae-vis-row">
              <button className={`wfm-btn-sm wfm-ae-vis${visible ? " active" : ""}`} onClick={() => setVisible(true)}>Visible</button>
              <button className={`wfm-btn-sm wfm-ae-vis${!visible ? " active" : ""}`} onClick={() => setVisible(false)}>Hidden</button>
            </div>
          </div>
          {error && <div className="wfm-ae-error">{error}</div>}
        </div>
        <div className="wfm-ae-popup-footer">
          <button className="wfm-btn-sm wfm-btn-save" onClick={save} disabled={busy}>{busy ? "Saving…" : "Save"}</button>
          <button className="wfm-btn-sm" onClick={onClose}>Cancel</button>
        </div>
      </div>
    </div>
  );
}

function RivensSection({ rivenOrders, itemIdMap, auctionRefreshKey, onEditOrder, onDeleteOrder, onToggleOrderVisible, onBulkOrdersVisible }: {
  rivenOrders: WfmOrder[];
  itemIdMap: Map<string, string>;
  auctionRefreshKey?: number;
  onEditOrder: (o: WfmOrder) => void;
  onDeleteOrder: (id: string) => void;
  onToggleOrderVisible: (o: WfmOrder) => void;
  onBulkOrdersVisible: (vis: boolean) => Promise<void>;
}) {
  const [auctions, setAuctions] = useState<WfmAuction[]>([]);
  const [busy, setBusy] = useState(false);
  const [editingAuction, setEditingAuction] = useState<WfmAuction | null>(null);

  const load = useCallback(async () => {
    setBusy(true);
    try {
      const res = await invokeWfm<{ payload?: { auctions?: WfmAuction[] } }>("wfm_get_my_riven_auctions");
      const list = (res?.payload?.auctions ?? []).filter((a: WfmAuction) => !a.is_closed);
      setAuctions([...list].sort((a, b) => (b.visible ? 1 : 0) - (a.visible ? 1 : 0)));
    } catch {}
    setBusy(false);
  }, []); // eslint-disable-line

  useEffect(() => { load(); }, [load, auctionRefreshKey]);

  const toggleAuctionVisible = (id: string, currentlyVisible: boolean) => {
    invoke("wfm_set_auction_visible", { auctionId: id, visible: !currentlyVisible })
      .then(() => load())
      .catch((e: unknown) => alert(String(e)));
  };

  const deleteAuction = (id: string) => {
    invoke("wfm_delete_auction", { auctionId: id })
      .then(() => load())
      .catch((e: unknown) => alert(String(e)));
  };

  const setAllAuctionsVisible = async (visible: boolean) => {
    await Promise.all(auctions.map(a =>
      invoke("wfm_set_auction_visible", { auctionId: a.id, visible }).catch(() => {})
    ));
    load();
  };

  const setAllVisible = async (vis: boolean) => {
    await Promise.all([onBulkOrdersVisible(vis), setAllAuctionsVisible(vis)]);
  };

  const saveAuctionEdit = async (id: string, start: number, buyout: number | null, visible: boolean, newIsDirect: boolean) => {
    const original = auctions.find(a => a.id === id);
    if (!original) throw new Error("Auction not found");
    if (newIsDirect !== original.is_direct_sell) {
      // Server-side: fetch full detail → delete → recreate with new type.
      // This guarantees all riven fields (attributes, polarity, etc.) are complete.
      // 3 rate-limited API calls: may take up to ~20 s if the limit is near.
      await invokeWfm("wfm_switch_riven_type", {
        auctionId:        id,
        newIsDirectSell:  newIsDirect,
        startingPrice:    start,
        buyoutPrice:      newIsDirect ? start : (buyout ?? null),
        visible,
      });
    } else {
      await invokeWfm("wfm_update_auction", {
        auctionId: id, startingPrice: start, buyoutPrice: buyout ?? null, visible,
      });
    }
    setEditingAuction(null);
    // Small delay: WFM may not reflect the new listing immediately.
    await new Promise(r => setTimeout(r, 1500));
    load();
  };

  const openAuctionEdit = (a: WfmAuction) => setEditingAuction(a);

  const totalCount = rivenOrders.length + auctions.length;

  return (
    <div style={{ marginTop: 16 }}>
      <div className="wfm-section-label wfm-section-label-row">
        <span>Rivens ({totalCount})</span>
        <button className="wfm-refresh-btn" onClick={load} title="Refresh" disabled={busy}>↻</button>
        {totalCount > 0 && <>
          <button className="wfm-bulk-btn wfm-bulk-show" onClick={() => setAllVisible(true)} title="Set all rivens visible">Vis All</button>
          <button className="wfm-bulk-btn wfm-bulk-hide" onClick={() => setAllVisible(false)} title="Set all rivens hidden">Hide All</button>
        </>}
      </div>
      {busy && totalCount === 0 ? (
        <div className="wfm-empty">Loading…</div>
      ) : totalCount === 0 ? (
        <div className="wfm-empty">No active riven listings. Post from Market → Rivens tab.</div>
      ) : (
        <div className="wfm-orders">
          {rivenOrders.map(o => (
            <div key={o.id} className={`wfm-order-row${o.visible ? "" : " wfm-order-hidden"}`}>
              <button
                className="wfm-vis-btn"
                title={o.visible ? "Visible — click to hide" : "Hidden — click to show"}
                onClick={() => onToggleOrderVisible(o)}>
                {o.visible ? "👁" : "🚫"}
              </button>
              <span className={`wfm-order-type ${o.type}`}>{o.type === "sell" ? "S" : "B"}</span>
              <span className="wfm-order-name">{orderName(o, itemIdMap)}</span>
              <span className="wfm-order-price">{fmt(o.platinum)}p</span>
              <span className="wfm-order-qty">×{o.quantity}</span>
              <button className="wfm-btn-sm" onClick={() => onEditOrder(o)}>Edit</button>
              <button className="wfm-btn-sm wfm-btn-del" onClick={() => onDeleteOrder(o.id)}>✕</button>
            </div>
          ))}
          {auctions.map(a => {
            const weaponName = a.item.weapon_url_name.replace(/_/g, " ").replace(/\b\w/g, (c: string) => c.toUpperCase());
            const modName = a.item.name ? a.item.name.charAt(0).toUpperCase() + a.item.name.slice(1) : "";
            return (
              <div key={a.id} className={`wfm-order-row${a.visible ? "" : " wfm-order-hidden"}`}>
                <button
                  className="wfm-vis-btn"
                  title={a.visible ? "Visible — click to hide" : "Hidden — click to show"}
                  onClick={() => toggleAuctionVisible(a.id, a.visible)}>
                  {a.visible ? "👁" : "🚫"}
                </button>
                <span className={`wfm-order-type ${a.is_direct_sell ? "direct" : "auction"}`}>
                  {a.is_direct_sell ? "DIR" : "AUC"}
                </span>
                <span className="wfm-order-name">
                  {weaponName}{modName && <em className="wfm-riven-mod"> {modName}</em>}
                </span>
                <span className="wfm-order-price">
                  {a.is_direct_sell ? (a.buyout_price ?? a.starting_price) : a.starting_price}p
                </span>
                {!a.is_direct_sell && (
                  <span className="wfm-auction-buyout">
                    {a.buyout_price != null ? `bo: ${a.buyout_price}p` : "bo: —"}
                  </span>
                )}
                {!a.is_direct_sell && (
                  <span className="wfm-order-qty">
                    {a.bids ?? 0} {(a.bids ?? 0) === 1 ? "bid" : "bids"}
                  </span>
                )}
                <button className="wfm-btn-sm" onClick={() => openAuctionEdit(a)}>Edit</button>
                <button className="wfm-btn-sm wfm-btn-del" onClick={() => deleteAuction(a.id)}>✕</button>
              </div>
            );
          })}
        </div>
      )}
      {editingAuction && (
        <AuctionEditPopup
          auction={editingAuction}
          onSave={saveAuctionEdit}
          onClose={() => setEditingAuction(null)}
        />
      )}
    </div>
  );
}

function ListingsPanel({ username: _username, itemIdMap, wfmItems, imageMap, auctionRefreshKey }: {
  username: string; itemIdMap: Map<string, string>; wfmItems: WfmItemEntry[]; imageMap: Map<string, string>;
  auctionRefreshKey?: number;
}) {
  const [orders, setOrders] = useState<{ sell: WfmOrder[]; buy: WfmOrder[] }>({ sell: [], buy: [] });
  const [loading, setLoading] = useState(true);
  const [search, setSearch]   = useState("");
  const [editing, setEditing] = useState<{ id: string; urlName: string; name: string; imageName?: string; pt: number; qty: number; visible: boolean } | null>(null);

  const nameToUrl = useMemo(() =>
    new Map(wfmItems.map(i => [i.item_name.toLowerCase(), i.url_name])),
    [wfmItems]
  );

  const loadOrders = useCallback(async () => {
    setLoading(true);
    try {
      const all = await invokeWfm<WfmOrder[]>("wfm_get_orders");
      setOrders({
        sell: (all ?? []).filter(o => o.type === "sell"),
        buy:  (all ?? []).filter(o => o.type === "buy"),
      });
    } catch {}
    setLoading(false);
  }, []);

  useEffect(() => { loadOrders(); }, [loadOrders]);

  const deleteOrder = async (id: string) => {
    await invokeWfm("wfm_delete_order", { orderId: id }).catch(() => {});
    loadOrders();
  };

  const toggleOrderVisible = async (o: WfmOrder) => {
    const cur = orders.sell.find(x => x.id === o.id) ?? orders.buy.find(x => x.id === o.id);
    if (!cur) return;
    await invokeWfm("wfm_update_order", { orderId: o.id, platinum: cur.platinum, quantity: cur.quantity, visible: !cur.visible }).catch(() => {});
    loadOrders();
  };

  const setAllOrdersVisible = async (vis: boolean) => {
    const all = [...orders.sell, ...orders.buy];
    await Promise.all(all.map(o =>
      invokeWfm("wfm_update_order", { orderId: o.id, platinum: o.platinum, quantity: o.quantity, visible: vis }).catch(() => {})
    ));
    loadOrders();
  };

  const saveEdit = async () => {
    if (!editing) return;
    await invokeWfm("wfm_update_order", { orderId: editing.id, platinum: editing.pt, quantity: editing.qty, visible: editing.visible }).catch(() => {});
    setEditing(null);
    loadOrders();
  };

  const startEdit = (o: WfmOrder) => {
    const name = orderName(o, itemIdMap);
    const urlName = nameToUrl.get(name.toLowerCase())
      ?? o.item?.slug ?? o.item?.urlName ?? o.item?.url_name ?? "";
    const imageName = imageMap.get(name.toLowerCase());
    setEditing({ id: o.id, urlName, name, imageName, pt: o.platinum, qty: o.quantity, visible: o.visible });
  };

  const allOrders = [...orders.sell, ...orders.buy];
  const rivenOrders = allOrders.filter(o => isRivenOrder(o, itemIdMap));
  const nonRivenOrders = allOrders.filter(o => !isRivenOrder(o, itemIdMap));
  const q = search.trim().toLowerCase();
  const visibleOrders = q ? nonRivenOrders.filter(o => orderName(o, itemIdMap).toLowerCase().includes(q)) : nonRivenOrders;

  const bulkRivenOrdersVisible = async (vis: boolean) => {
    await Promise.all(rivenOrders.map(o =>
      invokeWfm("wfm_update_order", { orderId: o.id, platinum: o.platinum, quantity: o.quantity, visible: vis }).catch(() => {})
    ));
    loadOrders();
  };

  return (
    <div className="wfm-panel">
      <div className="wfm-section-label wfm-section-label-row">
        <span>Active Listings ({nonRivenOrders.length})</span>
        <button className="wfm-refresh-btn" onClick={loadOrders} title="Refresh">↻</button>
        {nonRivenOrders.length > 0 && <>
          <button className="wfm-bulk-btn wfm-bulk-show" onClick={() => setAllOrdersVisible(true)} title="Set all listings visible">Vis All</button>
          <button className="wfm-bulk-btn wfm-bulk-hide" onClick={() => setAllOrdersVisible(false)} title="Set all listings hidden">Hide All</button>
        </>}
      </div>
      <div className="wfm-listings-hint">To post a new listing, click any set in the Prime Sets tab.</div>
      <input
        className="wfm-listings-search"
        type="text"
        placeholder="Search listings…"
        value={search}
        onChange={e => setSearch(e.target.value)}
      />
      {loading ? <div className="wfm-empty">Loading…</div> :
       visibleOrders.length === 0 ? <div className="wfm-empty">{q ? "No listings match." : "No active listings."}</div> :
       <div className="wfm-orders">
         {visibleOrders.map(o => (
           <div key={o.id} className={`wfm-order-row${o.visible ? "" : " wfm-order-hidden"}`}>
             <button
               className="wfm-vis-btn"
               title={o.visible ? "Visible — click to hide" : "Hidden — click to show"}
               onClick={() => toggleOrderVisible(o)}>
               {o.visible ? "👁" : "🚫"}
             </button>
             <span className={`wfm-order-type ${o.type}`}>{o.type === "sell" ? "S" : "B"}</span>
             <span className="wfm-order-name">{orderName(o, itemIdMap)}</span>
             <span className="wfm-order-price">{fmt(o.platinum)}p</span>
             <span className="wfm-order-qty">×{o.quantity}</span>
             <button className="wfm-btn-sm" onClick={() => startEdit(o)}>Edit</button>
             <button className="wfm-btn-sm wfm-btn-del" onClick={() => deleteOrder(o.id)}>✕</button>
           </div>
         ))}
       </div>
      }
      {editing && editing.urlName && (
        <ItemMarketPopup
          urlName={editing.urlName}
          displayName={editing.name}
          imageName={editing.imageName}
          onClose={() => setEditing(null)}
          isLoggedIn={true}
          editMode={{
            pt: editing.pt, qty: editing.qty, visible: editing.visible,
            onPtChange: v => setEditing(e => e && { ...e, pt: v }),
            onQtyChange: v => setEditing(e => e && { ...e, qty: v }),
            onVisibleChange: v => setEditing(e => e && { ...e, visible: v }),
            onSave: saveEdit,
          }}
        />
      )}
      <RivensSection
        rivenOrders={rivenOrders}
        itemIdMap={itemIdMap}
        auctionRefreshKey={auctionRefreshKey}
        onEditOrder={startEdit}
        onDeleteOrder={deleteOrder}
        onToggleOrderVisible={toggleOrderVisible}
        onBulkOrdersVisible={bulkRivenOrdersVisible}
      />
    </div>
  );
}

// ── Messages panel ────────────────────────────────────────────────────────────

function MessagesPanel({ username: _username, wfmItems }: { username: string; wfmItems: WfmItemEntry[] }) {
  const [whispers, setWhispers] = useState<WfmWhisper[]>([]);
  const [copied, setCopied]     = useState<string | null>(null);
  const [reverting, setReverting] = useState<number | null>(null);
  const bottomRef               = useRef<HTMLDivElement>(null);
  const ghostTimers             = useRef<ReturnType<typeof setTimeout>[]>([]);
  // Keep a stable ref so the trade-completed handler always sees current itemIdMap
  const itemIdMapRef            = useRef<Map<string, string>>(new Map());

  useEffect(() => {
    itemIdMapRef.current = new Map(wfmItems.map(i => [i.id, i.item_name]));
  }, [wfmItems]);

  useEffect(() => {
    return () => { ghostTimers.current.forEach(clearTimeout); };
  }, []);

  useEffect(() => {
    const unlisten = listen<WfmWhisper>("wfm-whisper", e => {
      setWhispers(prev => [...prev, e.payload]);
    });
    return () => { unlisten.then(fn => fn()); };
  }, []);

  // Auto-complete a matching whisper when an in-game trade finishes.
  useEffect(() => {
    const unlisten = listen<{
      withPlayer: string; direction: string; itemName: string;
      quantity: number; platinum: number; timestamp: string;
    }>("trade-completed", (e) => {
      const { withPlayer, direction, itemName, quantity } = e.payload;

      // Phase 1: immediately mark the ghost (synchronous state update)
      let matchedFrom: string | null = null;
      setWhispers(prev => {
        const idx = prev.findIndex(
          w => !w.completedAt && w.from.toLowerCase() === withPlayer.toLowerCase()
        );
        if (idx === -1) return prev;

        const updated = [...prev];
        const now = Date.now();
        updated[idx] = { ...updated[idx], completedAt: now };
        matchedFrom = updated[idx].from;

        // Copy the sold reply to clipboard automatically
        const w = updated[idx];
        if (w.item) {
          navigator.clipboard.writeText(`/w ${w.from} ${w.item} sold! Thank you.`).catch(() => {});
        }

        // Remove the ghost after 5 minutes
        const t = setTimeout(() => {
          const cutoff = Date.now() - 5 * 60 * 1000;
          setWhispers(curr => curr.filter(ww => !ww.completedAt || ww.completedAt > cutoff));
        }, 5 * 60 * 1000);
        ghostTimers.current.push(t);

        return updated;
      });

      // Phase 2: update WFM listing and attach revert info (async, only for sales)
      if (direction === "sold") {
        (async () => {
          try {
            const allOrders = await invokeWfm<WfmOrder[]>("wfm_get_orders");
            const sellOrders = (allOrders ?? []).filter(o => o.type === "sell");
            const idMap = itemIdMapRef.current;
            const tradeLower = itemName.toLowerCase();

            // Match by display name — exact first, then substring
            const match = sellOrders.find(o => orderName(o, idMap).toLowerCase() === tradeLower)
              ?? sellOrders.find(o => {
                const n = orderName(o, idMap).toLowerCase();
                return n.includes(tradeLower) || tradeLower.includes(n);
              });

            if (!match) return;

            const originalQty = match.quantity;
            const newQty      = Math.max(0, originalQty - quantity);
            const itemId      = (match as unknown as Record<string, unknown>).itemId as string | undefined ?? "";

            const revertInfo: NonNullable<WfmWhisper["revertInfo"]> = {
              orderId: match.id,
              itemId,
              platinum: match.platinum,
              originalQty,
              newQty,
              visible: match.visible,
            };

            if (newQty > 0) {
              await invokeWfm("wfm_update_order", { orderId: match.id, platinum: match.platinum, quantity: newQty, visible: match.visible });
            } else {
              await invokeWfm("wfm_delete_order", { orderId: match.id });
            }

            // Phase 2 state update: attach revertInfo to the ghost
            setWhispers(prev => {
              const idx = prev.findIndex(
                w => w.completedAt && w.from === (matchedFrom ?? withPlayer) && !w.revertInfo
              );
              if (idx === -1) return prev;
              const updated = [...prev];
              updated[idx] = { ...updated[idx], revertInfo };
              return updated;
            });
          } catch (err) {
            console.warn("[trade-completed] WFM order update failed:", err);
          }
        })();
      }
    });
    return () => { unlisten.then(fn => fn()); };
  }, []); // eslint-disable-line

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [whispers]);

  const copyInvite = (from: string) => {
    const msg = `/w ${from} Hi! I'm online, come to my orbiter.`;
    navigator.clipboard.writeText(msg).then(() => {
      setCopied(from);
      setTimeout(() => setCopied(null), 2000);
    });
  };

  const copySold = (from: string, item?: string, price?: number) => {
    const msg = item
      ? `/w ${from} ${item} sold! Thank you.`
      : `/w ${from} Sold! Thank you.`;
    navigator.clipboard.writeText(msg);
    // Auto-log the trade to Statistics
    if (item) {
      invoke("add_trade", {
        withPlayer: from,
        direction: "sold",
        itemName: item,
        itemUrl: "",
        quantity: 1,
        platinum: price ?? 0,
        source: "wfm",
        notes: "",
      }).catch(() => {});
    }
    setWhispers(prev => prev.filter(w => w.from !== from));
  };

  const revertOrder = async (w: WfmWhisper, idx: number) => {
    if (!w.revertInfo) return;
    const { orderId, itemId, platinum, originalQty, newQty, visible } = w.revertInfo;
    setReverting(idx);
    try {
      if (newQty > 0) {
        // We reduced qty → restore to original
        await invokeWfm("wfm_update_order", { orderId, platinum, quantity: originalQty, visible });
      } else {
        // We deleted the listing → re-create it
        await invokeWfm("wfm_create_order", { itemId, orderType: "sell", platinum, quantity: originalQty, visible });
      }
      // Clear revertInfo after a successful revert so the button disappears
      setWhispers(prev => {
        const updated = [...prev];
        if (updated[idx]) updated[idx] = { ...updated[idx], revertInfo: undefined };
        return updated;
      });
    } catch (err) {
      console.error("[revert] failed:", err);
    }
    setReverting(null);
  };

  return (
    <div className="wfm-panel">
      {whispers.length === 0 ? (
        <div className="wfm-empty-msg">
          <div>No trade whispers yet.</div>
          <div style={{ marginTop: 4, fontSize: 11, color: "var(--muted)" }}>
            When someone whispers you a warframe.market trade offer, it will appear here.
          </div>
        </div>
      ) : (
        <>
          <button className="wfm-clear-btn" onClick={() => setWhispers([])}>Clear all</button>
          {whispers.map((w, i) => (
            <div key={i} className={`wfm-whisper${w.completedAt ? " wfm-whisper-ghost" : ""}`}>
              <div className="wfm-whisper-header">
                <span className="wfm-whisper-from">{w.from}</span>
                <span className="wfm-whisper-time">{w.timestamp}</span>
              </div>
              {w.completedAt && (
                <div className="wfm-whisper-ghost-badge">✓ Completed in-game · auto-closing in 5 min</div>
              )}
              {w.item && (
                <div className="wfm-whisper-summary">
                  Wants: <span className="wfm-whisper-item">{w.item}</span>
                  {w.price && <span className="wfm-whisper-price"> · {fmt(w.price)}p</span>}
                </div>
              )}
              {!w.completedAt && (
                <div className="wfm-whisper-actions">
                  <button className="wfm-btn-sm wfm-btn-invite" onClick={() => copyInvite(w.from)}>
                    {copied === w.from ? "✓ Copied!" : "📋 Copy invite"}
                  </button>
                  <button className="wfm-btn-sm wfm-btn-sold" onClick={() => copySold(w.from, w.item, w.price)}>
                    ✓ Sold
                  </button>
                  <button className="wfm-btn-sm" onClick={() => setWhispers(prev => prev.filter((_, j) => j !== i))}>
                    Ignore
                  </button>
                </div>
              )}
              {w.completedAt && w.revertInfo && (
                <div className="wfm-whisper-revert">
                  <span className="wfm-revert-hint">
                    {w.revertInfo.newQty > 0
                      ? `WFM qty: ${w.revertInfo.originalQty} → ${w.revertInfo.newQty}`
                      : `WFM listing deleted (was ×${w.revertInfo.originalQty})`}
                  </span>
                  <button
                    className="wfm-btn-sm wfm-btn-revert"
                    disabled={reverting === i}
                    onClick={() => revertOrder(w, i)}
                  >
                    {reverting === i ? "Reverting…" : "↺ Revert"}
                  </button>
                </div>
              )}
            </div>
          ))}
          <div ref={bottomRef} />
        </>
      )}
    </div>
  );
}

// ── Main export ───────────────────────────────────────────────────────────────

export default function WfmTrading({ wfmLookup: _wfmLookup, wfmItems, imageMap, inventory: _inventory, onNewWhisper, onLoginChange, auctionRefreshKey }: Props) {
  const [tab, setTab]           = useState<"listings" | "messages">("listings");
  const [username, setUsername]         = useState<string | null>(null);
  const [checking, setChecking]         = useState(true);
  const [unread, setUnread]             = useState(0);
  const [wfmStatus, setWfmStatus]       = useState<"online" | "ingame" | "invisible" | "offline">("offline");
  const [statusBusy, setStatusBusy]     = useState(false);
  const [statusError, setStatusError]   = useState("");
  // The status the user actually wants — used to auto-reapply when WFM drops us to offline
  const targetStatusRef  = useRef<"online" | "ingame" | "invisible" | null>(null);
  const reconnectingRef  = useRef(false);

  const syncStatus = () => {
    invoke<string>("wfm_fetch_status")
      .then(async (s) => {
        if (s !== "online" && s !== "ingame" && s !== "invisible" && s !== "offline") return;
        if (s === "offline" && targetStatusRef.current && !reconnectingRef.current) {
          // WFM dropped our status — silently reapply the last known target
          reconnectingRef.current = true;
          try {
            await invoke("wfm_set_status", { status: targetStatusRef.current });
            setWfmStatus(targetStatusRef.current);
          } catch {
            setWfmStatus("offline");
          }
          reconnectingRef.current = false;
        } else {
          setWfmStatus(s);
        }
      })
      .catch(() => {});
  };

  // On mount: restore existing Rust session OR try saved credentials.
  // Both paths return [username, status] — dots update with no extra network call.
  useEffect(() => {
    (async () => {
      let resolvedUser: string | null = null;

      const existing = await invoke<[string, string] | null>("wfm_get_session").catch(() => null);
      if (existing) {
        [resolvedUser] = existing;
      } else {
        const creds = await invoke<[string, string] | null>("wfm_load_credentials").catch(() => null);
        if (creds) {
          try {
            [resolvedUser] = await invoke<[string, string]>("wfm_set_jwt", { jwt: creds[1] });
            // Re-save with any newly-fetched CSRF token so it persists across restarts
            const tokenJson = await invoke<string | null>("wfm_get_jwt").catch(() => null);
            if (tokenJson) await invoke("wfm_save_credentials", { email: "token", password: tokenJson }).catch(() => {});
          } catch { /* token expired — show login form */ }
        }
      }

      if (resolvedUser) {
        setUsername(resolvedUser);
        onLoginChange(resolvedUser);
        if (existing) {
          // Returning to the tab — restore the cached status (already updated by wfm_set_status).
          // Avoids an HTTP round-trip and the brief "nothing selected" flash from an async fetch.
          const cachedStatus = existing[1] as "online" | "ingame" | "invisible" | "offline";
          if (cachedStatus === "online" || cachedStatus === "ingame" || cachedStatus === "invisible") {
            setWfmStatus(cachedStatus);
            targetStatusRef.current = cachedStatus;
          }
        } else {
          // Fresh session start — default to invisible so the user controls when they appear.
          setWfmStatus("invisible");
          targetStatusRef.current = "invisible";
          invoke("wfm_set_status", { status: "invisible" }).catch(() => {});
        }
      }
      setChecking(false);
    })();
  }, []); // eslint-disable-line

  // Poll every 2 minutes — WFM can drop status to offline; syncStatus auto-reapplies
  useEffect(() => {
    if (!username) return;
    const id = setInterval(syncStatus, 2 * 60 * 1000);
    return () => clearInterval(id);
  }, [username]); // eslint-disable-line

  // Listen for whispers to increment badge
  useEffect(() => {
    const unlisten = listen("wfm-whisper", () => {
      if (tab !== "messages") {
        setUnread(n => n + 1);
        onNewWhisper();
      }
    });
    return () => { unlisten.then(fn => fn()); };
  }, [tab, onNewWhisper]);

  const switchToMessages = () => { setTab("messages"); setUnread(0); };

  const logout = () => {
    invoke("wfm_logout").catch(() => {});
    setUsername(null);
    onLoginChange(null);
  };

  if (checking) {
    return <div className="wfm-login-wrap"><div className="wfm-login-loading" style={{ marginTop: 40 }}>Connecting to warframe.market…</div></div>;
  }

  if (!username) {
    return <LoginPanel onLogin={u => { setUsername(u); onLoginChange(u); }} />;
  }

  return (
    <div className="wfm-trading">
      <div className="wfm-header">
        <div className="wfm-tabs">
          <button className={tab === "listings" ? "active" : ""} onClick={() => setTab("listings")}>Listings</button>
          <button className={tab === "messages" ? "active" : ""} onClick={switchToMessages}>
            Messages {unread > 0 && <span className="wfm-badge">{unread}</span>}
          </button>
        </div>
        <div className="wfm-session-info">
          <div className="wfm-status-picker"
            title={wfmStatus === "offline"
              ? "WFM set you offline — reconnecting automatically, or click a dot to force"
              : `Status: ${wfmStatus}. Click to change.`}>
            {(["online", "ingame", "invisible"] as const).map(s => (
              <button key={s} disabled={statusBusy}
                className={`wfm-status-opt${wfmStatus === s ? " active" : ""} wfm-status-${s}`}
                title={{ online: "Set Online", ingame: "Set In Game", invisible: "Set Invisible" }[s]}
                onClick={async () => {
                  setStatusBusy(true); setStatusError("");
                  try {
                    await invoke("wfm_set_status", { status: s });
                    setWfmStatus(s);
                    targetStatusRef.current = s;
                  } catch (e) { setStatusError(String(e)); }
                  setStatusBusy(false);
                }}>●</button>
            ))}
          </div>
          <span className="wfm-username">{username}</span>
          <button className="wfm-logout-btn" onClick={logout} title="Log out">⏻</button>
        </div>
      </div>

      {statusError && (
        <div style={{ padding: "4px 12px", fontSize: 11, color: "var(--red)", background: "rgba(248,81,73,.08)", borderBottom: "1px solid rgba(248,81,73,.2)" }}>
          {statusError}
        </div>
      )}

      {tab === "listings"
        ? <ListingsPanel username={username} itemIdMap={new Map(wfmItems.map(i => [i.id, i.item_name]))} wfmItems={wfmItems} imageMap={imageMap} auctionRefreshKey={auctionRefreshKey} />
        : <MessagesPanel username={username} wfmItems={wfmItems} />
      }
    </div>
  );
}
