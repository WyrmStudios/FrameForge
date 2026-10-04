import { useState, useEffect, useCallback, useRef, useMemo } from "react";
import { invoke } from "@tauri-apps/api/core";
import { emit, listen } from "@tauri-apps/api/event";
import ItemMarketPopup from "./ItemMarketPopup";
import { TAURI_COMMANDS, TAURI_EVENTS } from "../constants/tauri";
import type { WfmAuction, WfmItem, WfmManagedOrder, WfmWhisper } from "../types/market";
import type { TradeCompletedEvent } from "../types/trades";
import type { AddTradeArgs, WfmCloseOrderArgs, WfmCreateOrderArgs, WfmCredentials, WfmSaveCredentialsArgs, WfmSession, WfmSetAuctionVisibleArgs, WfmUpdateOrderArgs } from "../types/tauri";

// ── Types ─────────────────────────────────────────────────────────────────────

interface ListingChangeEntry {
  id: string;
  timestamp: number;
  action: "decreased" | "completed";
  itemName: string;
  withPlayer: string;
  platinum: number;
  oldQty: number;
  newQty: number;
  revertInfo?: NonNullable<WfmWhisper["revertInfo"]>;
  reverting?: boolean;
  reverted?: boolean;
}

interface Props {
  wfmLookup: Map<string, string>;
  wfmItems: WfmItem[];
  imageMap: Map<string, string>;
  inventory: Record<string, unknown>;
  onNewWhisper: () => void;
  onLoginChange: (username: string | null) => void;
  auctionRefreshKey?: number;
  recordSales: boolean;
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

// ─── WFM classes (Tailwind) ──────────────────────────────────────────────────
const WFM_LOGIN_WRAP  = "flex-1 flex items-center justify-center p-6";
const WFM_LOGIN_CARD  = "bg-white/4 border border-border/60 rounded-8 p-6 w-full max-w-85 flex flex-col gap-3";
const WFM_LOGIN_TITLE = "text-14 font-bold text-foreground";
const WFM_LOGIN_DESC  = "text-12 text-muted leading-normal";
const WFM_FIELD       = "flex flex-col gap-1";
const WFM_FIELD_LABEL = "text-11 text-muted shrink-0 min-w-20";
const WFM_INPUT       = "bg-black/20 border border-border/80 rounded-4 text-foreground text-12 px-2 py-1.25 outline-none transition-[border-color] duration-100 w-full focus:border-accent";
const WFM_ERROR       = "text-12 text-danger py-1";
const WFM_REMEMBER    = "flex items-center gap-1.5 text-12 text-muted cursor-pointer select-none";
const WFM_BTN_PRIMARY = "bg-[var(--accent)] border-0 rounded-5 text-white text-13 font-semibold px-4 py-2 cursor-pointer transition-[opacity] duration-100 hover:opacity-85 disabled:opacity-40 disabled:cursor-default";
const WFM_ALT_NOTE    = "text-11 text-muted leading-normal mt-2";
const WFM_ALT_LINK    = "text-accent underline";
const WFM_TRADING     = "flex flex-col h-full overflow-hidden";
const WFM_HEADER      = "flex items-center px-3 py-1.5 border-b border-border shrink-0 gap-2";
const WFM_TABS        = "flex gap-0.5";
const WFM_TAB         = "relative border border-border/60 bg-transparent text-muted text-12 px-3 py-0.75 rounded-4 cursor-pointer transition-[background,color,border-color] duration-100 hover:bg-white/6 hover:text-foreground";
const WFM_TAB_ON      = "relative border border-accent bg-accent/15 text-accent text-12 px-3 py-0.75 rounded-4 cursor-pointer transition-[background,color,border-color] duration-100";
const WFM_BADGE       = "inline-flex items-center justify-center bg-danger text-white text-10 font-bold rounded-10 min-w-4 h-4 px-1 ml-1 align-middle";
const WFM_SESSION     = "flex items-center gap-1.25 ml-auto";
const WFM_STATUS_PICKER = "flex gap-0.75 items-center";
const WFM_OPT         = "border rounded-full w-4.5 h-4.5 text-8 cursor-pointer flex items-center justify-center transition-[opacity,border-color] duration-150 p-0 hover:enabled:opacity-70 disabled:cursor-default";
const WFM_ST          = { online: { c: "text-success", on: "bg-success/15" }, ingame: { c: "text-accent", on: "bg-accent/15" }, invisible: { c: "text-muted", on: "bg-muted/15" } } as const;
const WFM_USERNAME    = "text-12 text-foreground font-semibold";
const WFM_LOGOUT      = "bg-transparent border-0 text-muted text-14 cursor-pointer px-0.5 leading-none transition-[color] duration-100 hover:text-danger";
const WFM_PANEL       = "flex-1 min-h-0 overflow-y-auto px-3.5 py-2.5 flex flex-col gap-1";
const WFM_SECTION_LABEL = "text-10 font-bold uppercase tracking-0.04 text-muted pt-0.5 pb-1";
const WFM_SECTION_ROW = "flex items-center gap-0";
const WFM_REFRESH     = "bg-transparent border-0 text-muted text-14 cursor-pointer pl-1.5 leading-none transition-[color] duration-100 hover:text-foreground";
const WFM_BULK        = "bg-transparent border border-border/50 rounded-3 text-9 font-bold px-1.25 py-0.25 cursor-pointer whitespace-nowrap normal-case tracking-normal ml-1 transition-[background,border-color,color] duration-100";
const WFM_BULK_SHOW   = `${WFM_BULK} text-success hover:bg-success/12 hover:border-[var(--green)]`;
const WFM_BULK_HIDE   = `${WFM_BULK} text-muted hover:bg-white/6 hover:border-muted/50 hover:text-foreground`;
const WFM_EMPTY       = "p-5 text-center text-12 text-muted";
const WFM_VIS_BTN     = "bg-transparent border-0 cursor-pointer text-13 p-0 leading-none opacity-70 shrink-0 transition-[opacity] duration-100 hover:opacity-100";
const WFM_HINT        = "text-11 text-muted pb-2 italic";
const WFM_SEARCH      = "w-full bg-[var(--surface)] border border-border rounded-6 text-foreground text-12 px-2 py-1.25 mb-1.5 outline-none focus:border-accent";
const WFM_ORDERS      = "flex flex-col gap-0.5";
const WFM_ORDER_ROW   = "flex items-center gap-1.5 px-2 py-1.25 bg-white/3 border border-border/35 rounded-4 transition-[background] duration-100 hover:bg-white/6";
const WFM_TYPE        = "text-10 font-bold px-1.25 py-0.25 rounded-3 shrink-0";
const WFM_TYPES       = { sell: `${WFM_TYPE} bg-success/15 text-success`, buy: `${WFM_TYPE} bg-accent/15 text-accent`, direct: `${WFM_TYPE} bg-direct-trade/15 text-direct-trade`, auction: `${WFM_TYPE} bg-ducat/15 text-ducat` } as const;
const WFM_ORDER_NAME  = "flex-1 text-12 text-foreground truncate min-w-0";
const WFM_ORDER_PRICE = "text-12 font-bold text-ducat shrink-0 tabular-nums";
const WFM_ORDER_QTY   = "text-11 text-muted shrink-0 min-w-6";
const WFM_BUYOUT      = "text-11 text-muted shrink-0 min-w-15";
const WFM_SM_BASE     = "border rounded-3 text-11 px-1.75 py-0.5 cursor-pointer whitespace-nowrap transition-[background,color] duration-100";
const WFM_SM_OFF      = `${WFM_SM_BASE} bg-white/6 border-border/50 text-muted hover:bg-white/12 hover:text-foreground`;
const WFM_SM_DEL      = `${WFM_SM_BASE} bg-white/6 border-border/50 text-muted hover:bg-white/12 hover:border-danger hover:text-danger`;
const WFM_SM_SAVE     = `${WFM_SM_BASE} bg-white/6 border-[var(--green)] text-success hover:bg-white/12 hover:text-foreground`;
const WFM_SM_INVITE   = `${WFM_SM_BASE} bg-white/6 border-[var(--accent)] text-accent hover:bg-white/12 hover:text-foreground`;
const WFM_SM_REVERT   = `${WFM_SM_BASE} bg-white/6 !border-confirm/45 !text-success hover:bg-white/12 hover:!bg-confirm/12 disabled:opacity-50 disabled:cursor-default`;
const WFM_AE_OVERLAY  = "fixed inset-0 bg-black/60 flex items-center justify-center z-1000";
const WFM_AE_CARD     = "bg-transparent border border-border rounded-8 w-75 max-w-[95vw] flex flex-col overflow-hidden";
const WFM_AE_HEADER   = "flex items-center gap-2 px-3.5 py-3 border-b border-border";
const WFM_AE_TITLE    = "text-14 font-semibold text-foreground";
const WFM_RIVEN_MOD   = "italic text-muted text-[0.92em]";
const WFM_AE_BODY     = "flex flex-col gap-3 p-3.5";
const WFM_AE_FIELD    = "flex flex-col gap-1.25";
const WFM_AE_LABEL    = "text-11 text-muted";
const WFM_AE_VIS_ROW  = "flex gap-1";
const WFM_VIS_ON      = `${WFM_SM_BASE} bg-accent/20 border-accent text-accent`;
const WFM_AE_HINT     = "text-10 text-muted italic";
const WFM_AE_TYPE_WARN = "block mt-1 text-10 text-ducat";
const WFM_AE_INPUT_ROW = "flex items-center gap-1.25";
const WFM_AE_INPUT    = "w-20 bg-black/30 border border-border/80 rounded-3 text-foreground text-12 px-1.5 py-1 outline-none focus:border-accent";
const WFM_PLAT        = "text-11 text-ducat shrink-0";
const WFM_AE_ERROR    = "text-11 text-danger pt-0.5";
const WFM_AE_FOOTER   = "flex gap-1.5 px-3.5 py-2.5 border-t border-border justify-end";
const WFM_CHANGELOG   = "mt-3.5 border-t border-border pt-2";
const WFM_CH_ROW      = "flex items-center gap-1.5 px-1 py-1.25 rounded-4 text-12 border-b border-border/40 last:border-b-0";
const WFM_CH_BADGE    = "shrink-0 w-4.5 h-4.5 rounded-full flex items-center justify-center text-11 font-bold";
const WFM_CH_TEXT     = "flex-1 min-w-0 truncate text-foreground";
const WFM_CH_PLAYER   = "text-muted text-11";
const WFM_CH_TIME     = "shrink-0 text-10 text-muted";
const WFM_CH_REV_LABEL = "text-10 text-muted italic";
const WFM_CLEAR       = "bg-transparent border-0 text-muted text-11 cursor-pointer self-end pb-1 underline hover:text-danger";
const WFM_WHISPER     = "border rounded-6 px-3 py-2.5 flex flex-col gap-1.5";
const WFM_W_HEADER    = "flex justify-between items-center";
const WFM_W_FROM      = "text-13 font-bold text-foreground";
const WFM_W_TIME      = "text-10 text-muted";
const WFM_W_SUMMARY   = "text-12 text-muted";
const WFM_W_ITEM      = "text-foreground font-semibold";
const WFM_W_PRICE     = "text-ducat";
const WFM_W_GHOST_BADGE = "text-11 text-success font-semibold";
const WFM_W_ACTIONS   = "flex gap-1.25 flex-wrap";
const WFM_W_REVERT    = "flex items-center gap-2 mt-1";
const WFM_REVERT_HINT = "text-11 text-muted flex-1";

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
        if (tokenJson) invoke("wfm_save_credentials", { email, token: tokenJson } satisfies WfmSaveCredentialsArgs).catch(() => {});
      }
      onLogin(username);
    } catch (e) { setError(String(e)); setLoading(false); }
  };

  return (
    <div className={WFM_LOGIN_WRAP}>
      <div className={WFM_LOGIN_CARD}>
        <div className={WFM_LOGIN_TITLE}>Connect warframe.market</div>
        <p className={WFM_LOGIN_DESC}>Log in to view live orders, manage listings, and receive trade whispers.</p>

        <div className={WFM_FIELD}>
          <label className={WFM_FIELD_LABEL}>Email</label>
          <input className={WFM_INPUT} type="email" value={email} onChange={e => setEmail(e.target.value)}
            onKeyDown={e => e.key === "Enter" && submit()} autoComplete="email" />
        </div>
        <div className={WFM_FIELD}>
          <label className={WFM_FIELD_LABEL}>Password</label>
          <input className={WFM_INPUT} type="password" value={password} onChange={e => setPassword(e.target.value)}
            onKeyDown={e => e.key === "Enter" && submit()} />
        </div>
        {error && <div className={WFM_ERROR}>{error}</div>}
        <label className={WFM_REMEMBER}>
          <input type="checkbox" checked={remember} onChange={e => setRemember(e.target.checked)} />
          Remember credentials
        </label>
        <button className={WFM_BTN_PRIMARY} onClick={submit} disabled={loading || !email || !password}>
          {loading ? "Logging in…" : "Log in"}
        </button>

        <p className={WFM_ALT_NOTE}>
          Using Steam, Xbox, Discord, or GitHub to log in to warframe.market? You'll need to create
          email/password credentials first — go to{" "}
          <a className={WFM_ALT_LINK} href="https://warframe.market/settings/account" target="_blank" rel="noreferrer">
            warframe.market/settings/account
          </a>{" "}
          and fill in <strong>Create credentials</strong>, then use those here.
        </p>
      </div>
    </div>
  );
}

// ── Listings panel ────────────────────────────────────────────────────────────

function orderName(o: WfmManagedOrder, itemIdMap: Map<string, string>): string {
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

function isRivenOrder(o: WfmManagedOrder, itemIdMap: Map<string, string>): boolean {
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
    <div className={WFM_AE_OVERLAY} onClick={onClose}>
      <div className={WFM_AE_CARD} onClick={e => e.stopPropagation()}>
        <div className={WFM_AE_HEADER}>
          <span className={WFM_AE_TITLE}>
            {weaponName}{modName && <em className={WFM_RIVEN_MOD}> {modName}</em>}
          </span>
          <button className={`${WFM_LOGOUT} text-lg`} onClick={onClose}>×</button>
        </div>
        <div className={WFM_AE_BODY}>
          <div className={WFM_AE_FIELD}>
            <label className={WFM_AE_LABEL}>Listing type</label>
            <div className={WFM_AE_VIS_ROW}>
              <button className={isDirect ? WFM_SM_OFF : WFM_VIS_ON} onClick={() => switchType(false)}>Auction</button>
              <button className={isDirect ? WFM_VIS_ON : WFM_SM_OFF} onClick={() => switchType(true)}>Direct Sale</button>
            </div>
            {typeChanged && (
              <span className={WFM_AE_TYPE_WARN}>
                ⚠ Switching type deletes and recreates the listing — may take up to 20 s due to WFM rate limits
              </span>
            )}
          </div>

          {isDirect ? (
            <div className={WFM_AE_FIELD}>
              <label className={WFM_AE_LABEL}>Price</label>
              <div className={WFM_AE_INPUT_ROW}>
                <input type="number" min={1} className={WFM_AE_INPUT} value={price}
                  onChange={e => setPrice(+e.target.value)} />
                <span className={WFM_PLAT}>p</span>
              </div>
            </div>
          ) : (
            <>
              <div className={WFM_AE_FIELD}>
                <label className={WFM_AE_LABEL}>Start price</label>
                <div className={WFM_AE_INPUT_ROW}>
                  <input type="number" min={1} className={WFM_AE_INPUT} value={startPrice}
                    onChange={e => setStartPrice(+e.target.value)} />
                  <span className={WFM_PLAT}>p</span>
                </div>
              </div>
              <div className={WFM_AE_FIELD}>
                <label className={WFM_AE_LABEL}>Buyout price</label>
                <div className={WFM_AE_INPUT_ROW}>
                  <input type="number" min={1} placeholder="none" className={WFM_AE_INPUT} value={buyoutPrice}
                    onChange={e => setBuyoutPrice(e.target.value)} />
                  <span className={WFM_PLAT}>p</span>
                  <span className={WFM_AE_HINT}>empty = no buyout</span>
                </div>
              </div>
            </>
          )}

          <div className={WFM_AE_FIELD}>
            <label className={WFM_AE_LABEL}>Visibility</label>
            <div className={WFM_AE_VIS_ROW}>
              <button className={visible ? WFM_VIS_ON : WFM_SM_OFF} onClick={() => setVisible(true)}>Visible</button>
              <button className={visible ? WFM_SM_OFF : WFM_VIS_ON} onClick={() => setVisible(false)}>Hidden</button>
            </div>
          </div>
          {error && <div className={WFM_AE_ERROR}>{error}</div>}
        </div>
        <div className={WFM_AE_FOOTER}>
          <button className={WFM_SM_SAVE} onClick={save} disabled={busy}>{busy ? "Saving…" : "Save"}</button>
          <button className={WFM_SM_OFF} onClick={onClose}>Cancel</button>
        </div>
      </div>
    </div>
  );
}

function RivensSection({ rivenOrders, itemIdMap, auctionRefreshKey, onEditOrder, onDeleteOrder, onToggleOrderVisible, onBulkOrdersVisible }: {
  rivenOrders: WfmManagedOrder[];
  itemIdMap: Map<string, string>;
  auctionRefreshKey?: number;
  onEditOrder: (o: WfmManagedOrder) => void;
  onDeleteOrder: (id: string) => void;
  onToggleOrderVisible: (o: WfmManagedOrder) => void;
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
    invoke("wfm_set_auction_visible", { auctionId: id, visible: !currentlyVisible } satisfies WfmSetAuctionVisibleArgs)
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
      invoke("wfm_set_auction_visible", { auctionId: a.id, visible } satisfies WfmSetAuctionVisibleArgs).catch(() => {})
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
    <div className="mt-4">
      <div className={`${WFM_SECTION_LABEL} ${WFM_SECTION_ROW}`}>
        <span>Rivens ({totalCount})</span>
        <button className={WFM_REFRESH} onClick={load} title="Refresh" disabled={busy}>↻</button>
        {totalCount > 0 && <>
          <button className={WFM_BULK_SHOW} onClick={() => setAllVisible(true)} title="Set all rivens visible">Vis All</button>
          <button className={WFM_BULK_HIDE} onClick={() => setAllVisible(false)} title="Set all rivens hidden">Hide All</button>
        </>}
      </div>
      {busy && totalCount === 0 ? (
        <div className={WFM_EMPTY}>Loading…</div>
      ) : totalCount === 0 ? (
        <div className={WFM_EMPTY}>No active riven listings. Post from Market → Rivens tab.</div>
      ) : (
        <div className={WFM_ORDERS}>
          {rivenOrders.map(o => (
            <div key={o.id} className={`${WFM_ORDER_ROW}${o.visible ? "" : " opacity-50"}`}>
              <button
                className={WFM_VIS_BTN}
                title={o.visible ? "Visible — click to hide" : "Hidden — click to show"}
                onClick={() => onToggleOrderVisible(o)}>
                {o.visible ? "👁" : "🚫"}
              </button>
              <span className={WFM_TYPES[o.type]}>{o.type === "sell" ? "S" : "B"}</span>
              <span className={WFM_ORDER_NAME}>{orderName(o, itemIdMap)}</span>
              <span className={WFM_ORDER_PRICE}>{fmt(o.platinum)}p</span>
              <span className={WFM_ORDER_QTY}>×{o.quantity}</span>
              <button className={WFM_SM_OFF} onClick={() => onEditOrder(o)}>Edit</button>
              <button className={WFM_SM_DEL} onClick={() => onDeleteOrder(o.id)}>✕</button>
            </div>
          ))}
          {auctions.map(a => {
            const weaponName = a.item.weapon_url_name.replace(/_/g, " ").replace(/\b\w/g, (c: string) => c.toUpperCase());
            const modName = a.item.name ? a.item.name.charAt(0).toUpperCase() + a.item.name.slice(1) : "";
            return (
              <div key={a.id} className={`${WFM_ORDER_ROW}${a.visible ? "" : " opacity-50"}`}>
                <button
                  className={WFM_VIS_BTN}
                  title={a.visible ? "Visible — click to hide" : "Hidden — click to show"}
                  onClick={() => toggleAuctionVisible(a.id, a.visible)}>
                  {a.visible ? "👁" : "🚫"}
                </button>
                <span className={a.is_direct_sell ? WFM_TYPES.direct : WFM_TYPES.auction}>
                  {a.is_direct_sell ? "DIR" : "AUC"}
                </span>
                <span className={WFM_ORDER_NAME}>
                  {weaponName}{modName && <em className={WFM_RIVEN_MOD}> {modName}</em>}
                </span>
                <span className={WFM_ORDER_PRICE}>
                  {a.is_direct_sell ? (a.buyout_price ?? a.starting_price) : a.starting_price}p
                </span>
                {!a.is_direct_sell && (
                  <span className={WFM_BUYOUT}>
                    {a.buyout_price != null ? `bo: ${a.buyout_price}p` : "bo: —"}
                  </span>
                )}
                {!a.is_direct_sell && (
                  <span className={WFM_ORDER_QTY}>
                    {a.bids ?? 0} {(a.bids ?? 0) === 1 ? "bid" : "bids"}
                  </span>
                )}
                <button className={WFM_SM_OFF} onClick={() => openAuctionEdit(a)}>Edit</button>
                <button className={WFM_SM_DEL} onClick={() => deleteAuction(a.id)}>✕</button>
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

function ListingsPanel({ username: _username, itemIdMap, wfmItems, imageMap, auctionRefreshKey, changelog, onUndo }: {
  username: string; itemIdMap: Map<string, string>; wfmItems: WfmItem[]; imageMap: Map<string, string>;
  auctionRefreshKey?: number;
  changelog?: ListingChangeEntry[];
  onUndo?: (entry: ListingChangeEntry) => void;
}) {
  const [orders, setOrders] = useState<{ sell: WfmManagedOrder[]; buy: WfmManagedOrder[] }>({ sell: [], buy: [] });
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
      const all = await invokeWfm<WfmManagedOrder[]>("wfm_get_orders");
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

  const toggleOrderVisible = async (o: WfmManagedOrder) => {
    const cur = orders.sell.find(x => x.id === o.id) ?? orders.buy.find(x => x.id === o.id);
    if (!cur) return;
    await invokeWfm("wfm_update_order", { orderId: o.id, platinum: cur.platinum, quantity: cur.quantity, visible: !cur.visible } satisfies WfmUpdateOrderArgs).catch(() => {});
    loadOrders();
  };

  const setAllOrdersVisible = async (vis: boolean) => {
    const all = [...orders.sell, ...orders.buy];
    await Promise.all(all.map(o =>
      invokeWfm("wfm_update_order", { orderId: o.id, platinum: o.platinum, quantity: o.quantity, visible: vis } satisfies WfmUpdateOrderArgs).catch(() => {})
    ));
    loadOrders();
  };

  const saveEdit = async () => {
    if (!editing) return;
    await invokeWfm("wfm_update_order", { orderId: editing.id, platinum: editing.pt, quantity: editing.qty, visible: editing.visible } satisfies WfmUpdateOrderArgs).catch(() => {});
    setEditing(null);
    loadOrders();
  };

  const startEdit = (o: WfmManagedOrder) => {
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
      invokeWfm("wfm_update_order", { orderId: o.id, platinum: o.platinum, quantity: o.quantity, visible: vis } satisfies WfmUpdateOrderArgs).catch(() => {})
    ));
    loadOrders();
  };

  return (
    <div className={WFM_PANEL}>
      <div className={`${WFM_SECTION_LABEL} ${WFM_SECTION_ROW}`}>
        <span>Active Listings ({nonRivenOrders.length})</span>
        <button className={WFM_REFRESH} onClick={loadOrders} title="Refresh">↻</button>
        {nonRivenOrders.length > 0 && <>
          <button className={WFM_BULK_SHOW} onClick={() => setAllOrdersVisible(true)} title="Set all listings visible">Vis All</button>
          <button className={WFM_BULK_HIDE} onClick={() => setAllOrdersVisible(false)} title="Set all listings hidden">Hide All</button>
        </>}
      </div>
      <div className={WFM_HINT}>To post a new listing, click any set in the Prime Sets tab.</div>
      <input
        className={WFM_SEARCH}
        type="text"
        placeholder="Search listings…"
        value={search}
        onChange={e => setSearch(e.target.value)}
      />
      {loading ? <div className={WFM_EMPTY}>Loading…</div> :
       visibleOrders.length === 0 ? <div className={WFM_EMPTY}>{q ? "No listings match." : "No active listings."}</div> :
       <div className={WFM_ORDERS}>
         {visibleOrders.map(o => (
           <div key={o.id} className={`${WFM_ORDER_ROW}${o.visible ? "" : " opacity-50"}`}>
             <button
               className={WFM_VIS_BTN}
               title={o.visible ? "Visible — click to hide" : "Hidden — click to show"}
               onClick={() => toggleOrderVisible(o)}>
               {o.visible ? "👁" : "🚫"}
             </button>
             <span className={WFM_TYPES[o.type]}>{o.type === "sell" ? "S" : "B"}</span>
             <span className={WFM_ORDER_NAME}>{orderName(o, itemIdMap)}</span>
             <span className={WFM_ORDER_PRICE}>{fmt(o.platinum)}p</span>
             <span className={WFM_ORDER_QTY}>×{o.quantity}</span>
             <button className={WFM_SM_OFF} onClick={() => startEdit(o)}>Edit</button>
             <button className={WFM_SM_DEL} onClick={() => deleteOrder(o.id)}>✕</button>
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
      {changelog && changelog.length > 0 && (
        <div className={WFM_CHANGELOG}>
          <div className={WFM_SECTION_LABEL}>Auto-updated listings</div>
          {changelog.map(entry => (
            <div key={entry.id} className={`${WFM_CH_ROW}${entry.reverted ? " opacity-55" : ""}`}>
              <span className={`${WFM_CH_BADGE} ${entry.action === "decreased" ? "bg-accent/20 text-accent" : "bg-success/20 text-success"}`}>
                {entry.action === "decreased" ? "−" : "✓"}
              </span>
              <span className={WFM_CH_TEXT}>
                {entry.action === "decreased"
                  ? <><strong>{entry.itemName}</strong> ({entry.platinum}p) ×{entry.oldQty} → ×{entry.newQty}</>
                  : <><strong>{entry.itemName}</strong> ({entry.platinum}p) listing sold</>
                }
                <span className={WFM_CH_PLAYER}> · {entry.withPlayer}</span>
              </span>
              <span className={WFM_CH_TIME}>{new Date(entry.timestamp).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}</span>
              {!entry.reverted && entry.revertInfo && onUndo && (
                <button
                  className={WFM_SM_REVERT}
                  disabled={entry.reverting}
                  onClick={() => onUndo(entry)}
                >
                  {entry.reverting ? "Undoing…" : "↺ Undo"}
                </button>
              )}
              {entry.reverted && <span className={WFM_CH_REV_LABEL}>Reverted</span>}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

// ── Messages panel ────────────────────────────────────────────────────────────

function MessagesPanel({ username: _username, wfmItems, recordSales, onListingChange }: {
  username: string;
  wfmItems: WfmItem[];
  recordSales: boolean;
  onListingChange?: (entry: Omit<ListingChangeEntry, "id" | "reverting" | "reverted">) => void;
}) {
  const [whispers, setWhispers] = useState<WfmWhisper[]>([]);
  const [copied, setCopied]     = useState<string | null>(null);
  const [reverting, setReverting] = useState<number | null>(null);
  const bottomRef               = useRef<HTMLDivElement>(null);
  const ghostTimers             = useRef<ReturnType<typeof setTimeout>[]>([]);
  const whispersRef             = useRef<WfmWhisper[]>([]);
  // Keep a stable ref so the trade-completed handler always sees current itemIdMap
  const itemIdMapRef            = useRef<Map<string, string>>(new Map());
  const recordSalesRef          = useRef(recordSales);

  useEffect(() => {
    itemIdMapRef.current = new Map(wfmItems.map(i => [i.id, i.item_name]));
  }, [wfmItems]);

  useEffect(() => {
    recordSalesRef.current = recordSales;
  }, [recordSales]);

  useEffect(() => {
    whispersRef.current = whispers;
  }, [whispers]);

  useEffect(() => {
    return () => { ghostTimers.current.forEach(clearTimeout); };
  }, []);

  useEffect(() => {
    const unlisten = listen<WfmWhisper>(TAURI_EVENTS.WFM_WHISPER, e => {
      setWhispers(prev => [...prev, e.payload]);
    });
    return () => { unlisten.then(fn => fn()); };
  }, []);

  // Auto-complete a matching whisper when an in-game trade finishes.
  useEffect(() => {
    const unlisten = listen<TradeCompletedEvent>(TAURI_EVENTS.TRADE_COMPLETED, (e) => {
      const { withPlayer, tradeType, offeredItems } = e.payload;
      const matchedWhisper = whispersRef.current.find(
        w => !w.completedAt && w.from.toLowerCase() === withPlayer.toLowerCase()
      );

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

      // Phase 2: update WFM listings and attach revert info when the change is reversible.
      if (tradeType === "sale") {
        (async () => {
          try {
            const allOrders = await invokeWfm<WfmManagedOrder[]>("wfm_get_orders");
            const sellOrders = (allOrders ?? []).filter(o => o.type === "sell");
            const idMap = itemIdMapRef.current;

            const completeOrder = async (match: WfmManagedOrder, soldQty: number, itemName: string) => {
              const originalQty = match.quantity;
              const closedQty   = Math.min(originalQty, soldQty);
              const newQty      = originalQty - closedQty;
              const itemId      = (match as unknown as Record<string, unknown>).itemId as string | undefined ?? "";
              const recordSale  = recordSalesRef.current;

              const revertInfo: NonNullable<WfmWhisper["revertInfo"]> = {
                orderId: match.id,
                itemId,
                platinum: match.platinum,
                originalQty,
                newQty,
                visible: match.visible,
                modRank: match.rank,
              };

              if (recordSale) {
                await invokeWfm("wfm_close_order", {
                  orderId: match.id,
                  quantity: closedQty,
                } satisfies WfmCloseOrderArgs);
              } else if (newQty > 0) {
                await invokeWfm("wfm_update_order", {
                  orderId: match.id,
                  platinum: match.platinum,
                  quantity: newQty,
                  visible: match.visible,
                } satisfies WfmUpdateOrderArgs);
              } else {
                await invokeWfm("wfm_delete_order", { orderId: match.id });
              }

              onListingChange?.({
                timestamp: Date.now(),
                action: newQty > 0 ? "decreased" : "completed",
                itemName,
                withPlayer,
                platinum: match.platinum,
                oldQty: originalQty,
                newQty,
                revertInfo: recordSale ? undefined : revertInfo,
              });

              if (!recordSale) setWhispers(prev => {
                const idx = prev.findIndex(
                  w => w.completedAt && w.from === (matchedFrom ?? withPlayer) && !w.revertInfo
                );
                if (idx === -1) return prev;
                const updated = [...prev];
                updated[idx] = { ...updated[idx], revertInfo };
                return updated;
              });

              match.quantity = newQty;
            };

            // A full set appears in EE.log as its individual parts. The WFM
            // whisper retains the actual listing name, e.g. "Burston Prime Set".
            const requestedItem = matchedWhisper?.item?.trim();
            if (requestedItem) {
              const requestedLower = requestedItem.toLowerCase();
              const match = sellOrders.find(o => orderName(o, idMap).toLowerCase() === requestedLower);
              if (match) {
                await completeOrder(match, 1, requestedItem);
                return;
              }
            }

            // No matching whisper listing: fall back to individual EE.log items.
            for (const soldItem of offeredItems) {
              const tradeLower = soldItem.name.toLowerCase();

              // Match by display name — exact first, then substring
              const match = sellOrders.find(o => orderName(o, idMap).toLowerCase() === tradeLower)
                ?? sellOrders.find(o => {
                  const n = orderName(o, idMap).toLowerCase();
                  return n.includes(tradeLower) || tradeLower.includes(n);
                });

              if (!match) continue;
              await completeOrder(match, soldItem.qty, soldItem.name);
            }
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
      const args: AddTradeArgs = {
        withPlayer: from,
        direction: "sold",
        itemName: item,
        itemUrl: "",
        quantity: 1,
        platinum: price ?? 0,
        source: "wfm",
        notes: "",
      };
      invoke(TAURI_COMMANDS.ADD_TRADE, { params: args })
        .then(() => emit(TAURI_EVENTS.TRADES_UPDATED).catch(() => {}))
        .catch((e) => console.error("[trade-log] add_trade failed:", e));
    }
    setWhispers(prev => prev.filter(w => w.from !== from));
  };

  const revertOrder = async (w: WfmWhisper, idx: number) => {
    if (!w.revertInfo) return;
    const { orderId, itemId, platinum, originalQty, newQty, visible, modRank } = w.revertInfo;
    setReverting(idx);
    try {
      if (newQty > 0) {
        // We reduced qty → restore to original
        await invokeWfm("wfm_update_order", { orderId, platinum, quantity: originalQty, visible } satisfies WfmUpdateOrderArgs);
      } else {
        // We deleted the listing → re-create it
        await invokeWfm(TAURI_COMMANDS.WFM_CREATE_ORDER, { itemId, orderType: "sell", platinum, quantity: originalQty, visible, modRank } satisfies WfmCreateOrderArgs);
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
    <div className={WFM_PANEL}>
      {whispers.length === 0 ? (
        <div className={WFM_EMPTY}>
          <div>No trade whispers yet.</div>
          <div className="mt-1 text-11 text-muted">
            When someone whispers you a warframe.market trade offer, it will appear here.
          </div>
        </div>
      ) : (
        <>
          <button className={WFM_CLEAR} onClick={() => setWhispers([])}>Clear all</button>
          {whispers.map((w, i) => (
            <div key={i} className={`${WFM_WHISPER}${w.completedAt ? " border-confirm/35 bg-confirm/5 opacity-75" : " border-border/40 bg-white/4"}`}>
              <div className={WFM_W_HEADER}>
                <span className={WFM_W_FROM}>{w.from}</span>
                <span className={WFM_W_TIME}>{w.timestamp}</span>
              </div>
              {w.completedAt && (
                <div className={WFM_W_GHOST_BADGE}>✓ Completed in-game · auto-closing in 5 min</div>
              )}
              {w.item && (
                <div className={WFM_W_SUMMARY}>
                  Wants: <span className={WFM_W_ITEM}>{w.item}</span>
                  {w.price && <span className={WFM_W_PRICE}> · {fmt(w.price)}p</span>}
                </div>
              )}
              {!w.completedAt && (
                <div className={WFM_W_ACTIONS}>
                  <button className={WFM_SM_INVITE} onClick={() => copyInvite(w.from)}>
                    {copied === w.from ? "✓ Copied!" : "📋 Copy invite"}
                  </button>
                  <button className={WFM_SM_SAVE} onClick={() => copySold(w.from, w.item, w.price)}>
                    ✓ Sold
                  </button>
                  <button className={WFM_SM_OFF} onClick={() => setWhispers(prev => prev.filter((_, j) => j !== i))}>
                    Ignore
                  </button>
                </div>
              )}
              {w.completedAt && w.revertInfo && (
                <div className={WFM_W_REVERT}>
                  <span className={WFM_REVERT_HINT}>
                    {w.revertInfo.newQty > 0
                      ? `WFM qty: ${w.revertInfo.originalQty} → ${w.revertInfo.newQty}`
                      : `WFM listing sold (was ×${w.revertInfo.originalQty})`}
                  </span>
                  <button
                    className={WFM_SM_REVERT}
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

export default function WfmTrading({ wfmLookup: _wfmLookup, wfmItems, imageMap, inventory: _inventory, onNewWhisper, onLoginChange, auctionRefreshKey, recordSales }: Props) {
  const [tab, setTab]           = useState<"listings" | "messages">("listings");
  const [username, setUsername]         = useState<string | null>(null);
  const [checking, setChecking]         = useState(true);
  const [unread, setUnread]             = useState(0);
  const [wfmStatus, setWfmStatus]       = useState<"online" | "ingame" | "invisible" | "offline">("offline");
  const [statusBusy, setStatusBusy]     = useState(false);
  const [statusError, setStatusError]   = useState("");
  const [listingChangelog, setListingChangelog] = useState<ListingChangeEntry[]>([]);
  // The status the user actually wants — used to auto-reapply when WFM drops us to offline
  const targetStatusRef  = useRef<"online" | "ingame" | "invisible" | null>(null);
  const reconnectingRef  = useRef(false);

  const handleListingChange = useCallback((entry: Omit<ListingChangeEntry, "id" | "reverting" | "reverted">) => {
    setListingChangelog(prev => [
      { ...entry, id: `${entry.timestamp}-${Math.random().toString(36).slice(2, 7)}` },
      ...prev,
    ].slice(0, 50));
  }, []);

  const handleUndo = useCallback(async (entry: ListingChangeEntry) => {
    if (!entry.revertInfo) return;
    setListingChangelog(prev => prev.map(e => e.id === entry.id ? { ...e, reverting: true } : e));
    try {
      const { orderId, itemId, platinum, originalQty, newQty, visible, modRank } = entry.revertInfo;
      if (newQty > 0) {
        await invokeWfm("wfm_update_order", { orderId, platinum, quantity: originalQty, visible } satisfies WfmUpdateOrderArgs);
      } else {
        await invokeWfm(TAURI_COMMANDS.WFM_CREATE_ORDER, { itemId, orderType: "sell", platinum, quantity: originalQty, visible, modRank } satisfies WfmCreateOrderArgs);
      }
      setListingChangelog(prev => prev.map(e => e.id === entry.id ? { ...e, reverted: true, reverting: false } : e));
    } catch (err) {
      console.error("[undo listing]", err);
      setListingChangelog(prev => prev.map(e => e.id === entry.id ? { ...e, reverting: false } : e));
    }
  }, []);

  const syncStatus = () => {
    invoke<string>("wfm_fetch_status")
      .then(async (s) => {
        if (s !== "online" && s !== "ingame" && s !== "invisible" && s !== "offline") return;
        if (s === "offline" && targetStatusRef.current && !reconnectingRef.current) {
          // WFM dropped our status — silently reapply the last known target
          reconnectingRef.current = true;
          try {
            await invoke(TAURI_COMMANDS.WFM_SET_STATUS, { status: targetStatusRef.current });
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

      const existing = await invoke<WfmSession | null>(TAURI_COMMANDS.WFM_GET_SESSION).catch(() => null);
      if (existing) {
        [resolvedUser] = existing;
      } else {
        const creds = await invoke<WfmCredentials | null>(TAURI_COMMANDS.WFM_LOAD_CREDENTIALS).catch(() => null);
        if (creds) {
          try {
            [resolvedUser] = await invoke<WfmSession>(TAURI_COMMANDS.WFM_SET_JWT, { jwt: creds[1] });
            // Re-save with any newly-fetched CSRF token so it persists across
            // restarts, under the same email it was stored with. Failing here
            // is not worth reporting: nobody asked for it, and the token
            // already on disk still works.
            const tokenJson = await invoke<string | null>("wfm_get_jwt").catch(() => null);
            if (tokenJson) await invoke("wfm_save_credentials", { email: creds[0], token: tokenJson } satisfies WfmSaveCredentialsArgs).catch(() => {});
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
          invoke(TAURI_COMMANDS.WFM_SET_STATUS, { status: "invisible" }).catch(() => {});
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
    const unlisten = listen(TAURI_EVENTS.WFM_WHISPER, () => {
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
    return <div className={WFM_LOGIN_WRAP}><div className="wfm-login-loading mt-10">Connecting to warframe.market…</div></div>;
  }

  if (!username) {
    return <LoginPanel onLogin={u => { setUsername(u); onLoginChange(u); }} />;
  }

  return (
    <div className={WFM_TRADING}>
      <div className={WFM_HEADER}>
        <div className={WFM_TABS}>
          <button className={tab === "listings" ? WFM_TAB_ON : WFM_TAB} onClick={() => setTab("listings")}>Listings</button>
          <button className={tab === "messages" ? WFM_TAB_ON : WFM_TAB} onClick={switchToMessages}>
            Messages {unread > 0 && <span className={WFM_BADGE}>{unread}</span>}
          </button>
        </div>
        <div className={WFM_SESSION}>
          <div className={WFM_STATUS_PICKER}
            title={wfmStatus === "offline"
              ? "WFM set you offline — reconnecting automatically, or click a dot to force"
              : `Status: ${wfmStatus}. Click to change.`}>
            {(["online", "ingame", "invisible"] as const).map(s => (
              <button key={s} disabled={statusBusy}
                className={`${WFM_OPT} ${wfmStatus === s ? `border-transparent opacity-100 ${WFM_ST[s].on}` : "bg-transparent border-border/50 opacity-35"} ${WFM_ST[s].c}`}
                title={{ online: "Set Online", ingame: "Set In Game", invisible: "Set Invisible" }[s]}
                onClick={async () => {
                  setStatusBusy(true); setStatusError("");
                  try {
                    await invoke(TAURI_COMMANDS.WFM_SET_STATUS, { status: s });
                    setWfmStatus(s);
                    targetStatusRef.current = s;
                  } catch (e) { setStatusError(String(e)); }
                  setStatusBusy(false);
                }}>●</button>
            ))}
          </div>
          <span className={WFM_USERNAME}>{username}</span>
          <button className={WFM_LOGOUT} onClick={logout} title="Log out">⏻</button>
        </div>
      </div>

      {statusError && (
        <div className="border-b border-danger/20 bg-danger/8 px-3 py-1 text-11 text-red">
          {statusError}
        </div>
      )}

      {/* Both panels stay mounted so MessagesPanel's trade-completed listener
          fires even when the user is on the Listings tab. */}
      <div className={tab === "listings" ? "contents" : "hidden"}>
        <ListingsPanel username={username} itemIdMap={new Map(wfmItems.map(i => [i.id, i.item_name]))} wfmItems={wfmItems} imageMap={imageMap} auctionRefreshKey={auctionRefreshKey} changelog={listingChangelog} onUndo={handleUndo} />
      </div>
      <div className={tab === "messages" ? "contents" : "hidden"}>
        <MessagesPanel username={username} wfmItems={wfmItems} recordSales={recordSales} onListingChange={handleListingChange} />
      </div>
    </div>
  );
}
