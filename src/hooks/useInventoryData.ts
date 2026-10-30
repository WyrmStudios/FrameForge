import { useState, useEffect, useCallback, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getVersion } from "@tauri-apps/api/app";
import { listen } from "@tauri-apps/api/event";
import { COMPANION_API_SUSPENDED } from "../constants/tauri";
import { TAURI_COMMANDS, TAURI_EVENTS } from "../constants/tauri";
import type { CatalogItem, CraftingJob, QuantityMap } from "../types/items";
import type { ModCopy } from "../types/inventory";
import type { ChangeLogEntry, InventoryUpdate } from "../types/inventory";
import type { ItemListStatus, SaveApiInventoryArgs, SavedApiInventory, WarframeCredentials, WarframeInventoryRequest } from "../types/tauri";
import type { BlobRivenEntry, BlobRivenStat } from "../types/market";

interface UseInventoryDataReturn {
  // State
  catalog: CatalogItem[];
  quantities: QuantityMap;
  apiQuantities: QuantityMap;
  apiModCopies: ModCopy[];
  scannerMods: Record<string, { total: number; by_rank: Record<string, number> }>;
  crafting: CraftingJob[];
  masteryRank: number | null;
  masteryData: Record<string, number>;
  playerName: string | null;
  subsummedWarframes: Set<string>;
  archonShards: Record<string, { type: string; tauforged: boolean; color: string; boost?: string }[]>;
  formaData: Record<string, number>;
  lastApiRefresh: number | null;
  inventoryReady: boolean;
  lastInventoryScanAt: number | null;
  changeLog: ChangeLogEntry[];
  changeLogArrivalToken: number;
  lastChanged: Record<string, number>;
  monitoring: boolean;
  warframeRunning: boolean;
  itemCount: number;
  recipeCount: number;
  fetching: boolean;
  fetchMsg: string;
  wfConnected: boolean;
  imgCacheDir: string;
  itemsRefreshKey: number;

  // Refs
  wfConnectedRef: React.MutableRefObject<boolean>;
  inventoryRestoredRef: React.MutableRefObject<boolean>;
  catalogRef: React.MutableRefObject<CatalogItem[]>;
  manualCredsRef: React.MutableRefObject<{ accountId: string; nonce: string } | null>;

  // Callbacks
  handleFetch: () => Promise<void>;
  applyInventoryData: (raw: unknown) => void;

  // Setters (voor SettingsModal)
  setCatalog: React.Dispatch<React.SetStateAction<CatalogItem[]>>;
  setQuantities: React.Dispatch<React.SetStateAction<QuantityMap>>;
  setApiQuantities: React.Dispatch<React.SetStateAction<QuantityMap>>;
  setApiModCopies: React.Dispatch<React.SetStateAction<ModCopy[]>>;
  setScannerMods: React.Dispatch<React.SetStateAction<Record<string, { total: number; by_rank: Record<string, number> }>>>;
  setCrafting: React.Dispatch<React.SetStateAction<CraftingJob[]>>;
  setMasteryData: React.Dispatch<React.SetStateAction<Record<string, number>>>;
  setArchonShards: React.Dispatch<React.SetStateAction<Record<string, { type: string; tauforged: boolean; color: string; boost?: string }[]>>>;
  setFormaData: React.Dispatch<React.SetStateAction<Record<string, number>>>;
  setChangeLog: React.Dispatch<React.SetStateAction<ChangeLogEntry[]>>;
  setLastChanged: React.Dispatch<React.SetStateAction<Record<string, number>>>;
  setWfConnected: React.Dispatch<React.SetStateAction<boolean>>;
  setItemsRefreshKey: React.Dispatch<React.SetStateAction<number>>;
  setMonitoring: React.Dispatch<React.SetStateAction<boolean>>;
  setWarframeRunning: React.Dispatch<React.SetStateAction<boolean>>;
  setPlayerName: React.Dispatch<React.SetStateAction<string | null>>;
}

export function useInventoryData(): UseInventoryDataReturn {
  // ── State ───────────────────────────────────────────────────────────────────
  const [catalog, setCatalog] = useState<CatalogItem[]>([]);
  const [quantities, setQuantities] = useState<QuantityMap>({});
  const [apiQuantities, setApiQuantities] = useState<QuantityMap>({});
  const [apiModCopies, setApiModCopies] = useState<ModCopy[]>([]);
  const [apiRivens, setApiRivens] = useState<BlobRivenEntry[]>([]);
  const [scannerMods, setScannerMods] = useState<Record<string, { total: number; by_rank: Record<string, number> }>>({});
  const [crafting, setCrafting] = useState<CraftingJob[]>([]);
  const [masteryRank, setMasteryRank] = useState<number | null>(null);
  const [masteryData, setMasteryData] = useState<Record<string, number>>({});
  const [playerName, setPlayerName] = useState<string | null>(null);
  const [subsummedWarframes, setSubsummedWarframes] = useState<Set<string>>(new Set());
  const [archonShards, setArchonShards] = useState<Record<string, { type: string; tauforged: boolean; color: string; boost?: string }[]>>({});
  const [formaData, setFormaData] = useState<Record<string, number>>({});
  const [lastApiRefresh, setLastApiRefresh] = useState<number | null>(null);
  const [inventoryReady, setInventoryReady] = useState(false);
  const [lastInventoryScanAt, setLastInventoryScanAt] = useState<number | null>(null);
  const [changeLog, setChangeLog] = useState<ChangeLogEntry[]>([]);
  const [changeLogArrivalToken, setChangeLogArrivalToken] = useState(0);
  const [lastChanged, setLastChanged] = useState<Record<string, number>>({});
  const [monitoring, setMonitoring] = useState(false);
  const [warframeRunning, setWarframeRunning] = useState(false);
  const [itemCount, setItemCount] = useState(0);
  const [recipeCount, setRecipeCount] = useState(0);
  const [fetching, setFetching] = useState(false);
  const [fetchMsg, setFetchMsg] = useState("");
  const [wfConnected, setWfConnected] = useState(false);
  const [imgCacheDir, setImgCacheDir] = useState("");
  const [itemsRefreshKey, setItemsRefreshKey] = useState(0);

  // ── Refs ────────────────────────────────────────────────────────────────────
  const wfConnectedRef = useRef(false);
  const inventoryRestoredRef = useRef(false);
  const inventoryReadyRef = useRef(false);
  const catalogRef = useRef<CatalogItem[]>([]);
  const prevApiQtyRef = useRef<QuantityMap>({});
  const manualCredsRef = useRef<{ accountId: string; nonce: string } | null>(null);

  // ── Apply inventory data from Warframe API ──────────────────────────────────
  const applyInventoryData = useCallback((raw: unknown) => {
    // Companion inventory is an evolving external payload; keep it untyped at the transport boundary.
    const data: any = raw;
    const apiQty: QuantityMap = {};
    const ownedArrayKeys = [
      "Suits", "LongGuns", "Pistols", "Melee",
      "Sentinels", "SentinelWeapons",
      "SpaceSuits", "SpaceGuns", "SpaceMelee",
      "MechSuits", "KubrowPets",
      "CrewShipWeapons", "OperatorAmps", "OperatorSuits",
    ];
    const masteryUpdate: Record<string, number> = {};
    for (const key of ownedArrayKeys) {
      const arr = data[key];
      if (!Array.isArray(arr)) continue;
      for (const item of arr) {
        const t: string = item.ItemType;
        if (!t) continue;
        apiQty[t] = (apiQty[t] ?? 0) + 1;
        // Extract mastery rank from XP field — 30,000 XP per rank, cap at 30
        if (item.XP != null) {
          masteryUpdate[t] = Math.min(30, Math.floor(item.XP / 30000));
        }
      }
    }
    if (Object.keys(masteryUpdate).length > 0)
      setMasteryData(prev => ({ ...prev, ...masteryUpdate }));
    for (const r of (Array.isArray(data.Recipes) ? data.Recipes : [])) {
      const t: string = r.ItemType;
      if (t) {
        apiQty[t] = (apiQty[t] ?? 0) + (r.ItemCount ?? 1);
      }
    }
    // MiscItems: pull everything (relics, resources like Carbides/Cubic Diodes, etc.)
    // so the API is authoritative and mission-pickup counts from the scanner don't override.
    for (const m of (Array.isArray(data.MiscItems) ? data.MiscItems : [])) {
      const t: string = m.ItemType;
      if (t) apiQty[t] = (apiQty[t] ?? 0) + (m.ItemCount ?? 1);
    }
    const rawModMap: Record<string, number> = {};
    for (const r of (Array.isArray(data.RawUpgrades) ? data.RawUpgrades : [])) {
      if (r.ItemType) rawModMap[r.ItemType] = (rawModMap[r.ItemType] ?? 0) + (r.ItemCount ?? 1);
    }
    const rankedModMap: Record<string, Record<number, number>> = {};
    for (const u of (Array.isArray(data.Upgrades) ? data.Upgrades : [])) {
      if (!u.ItemType) continue;
      let rank = 0;
      try { if (u.UpgradeFingerprint) rank = JSON.parse(u.UpgradeFingerprint)?.lvl ?? 0; } catch { rank = 0; }
      if (!rankedModMap[u.ItemType]) rankedModMap[u.ItemType] = {};
      rankedModMap[u.ItemType][rank] = (rankedModMap[u.ItemType][rank] ?? 0) + 1;
    }
    const copies: ModCopy[] = [];
    for (const [t, cnt] of Object.entries(rawModMap)) {
      copies.push({ uniqueName: t, rank: null, count: cnt });
      apiQty[t] = (apiQty[t] ?? 0) + cnt;
    }
    for (const [t, ranks] of Object.entries(rankedModMap)) {
      apiQty[t] = (apiQty[t] ?? 0) + Object.values(ranks).reduce((a, b) => a + b, 0);
      for (const [r, cnt] of Object.entries(ranks)) {
        copies.push({ uniqueName: t, rank: Number(r), count: cnt });
      }
    }
    setApiModCopies(copies);

    // Riven fingerprint detail (buffs/curses/compat/challenge/polarity/rerolls) —
    // mirrors memory_scanner.rs's parse_full_account_blob RawUpgrades/Upgrades
    // handling field-for-field so the Companion API path can populate the same
    // owned-rivens detail the memory scanner provides. mod_name is left blank;
    // MarketHelper already falls back to a client-computed name (rivenModName)
    // when mod_name is empty, same as it does for scanner-sourced unlocked rivens.
    const newRivens: BlobRivenEntry[] = [];
    for (const r of (Array.isArray(data.RawUpgrades) ? data.RawUpgrades : [])) {
      const t: string = r.ItemType;
      if (!t || !t.startsWith("/Lotus/") || !t.includes("RandomMod")) continue;
      const count = r.ItemCount ?? 0;
      if (count <= 0) continue;
      newRivens.push({
        item_id: "", item_type: t, mod_name: "",
        riven_state: "unrevealed",
        compat: null, challenge_type: null, challenge_complication: null,
        lvl_req: null, polarity: null, buffs: [], curses: [],
        mod_rank: 0, count, rerolls: 0,
      });
    }
    const toStats = (arr: unknown): BlobRivenStat[] =>
      Array.isArray(arr) ? arr.map((s: any) => ({ tag: s.Tag ?? "", value: s.Value ?? 0 })) : [];
    for (const u of (Array.isArray(data.Upgrades) ? data.Upgrades : [])) {
      const t: string = u.ItemType;
      if (!t || !t.startsWith("/Lotus/") || !t.includes("RandomMod")) continue;
      let fp: any;
      try { fp = u.UpgradeFingerprint ? JSON.parse(u.UpgradeFingerprint) : null; } catch { continue; }
      if (!fp) continue;
      const itemId: string = u.ItemId?.$oid ?? "";
      if (typeof fp.compat === "string") {
        newRivens.push({
          item_id: itemId, item_type: t, mod_name: "",
          riven_state: "unlocked",
          compat: fp.compat, challenge_type: null, challenge_complication: null,
          lvl_req: fp.lvlReq ?? null, polarity: fp.pol ?? null,
          buffs: toStats(fp.buffs), curses: toStats(fp.curses),
          mod_rank: fp.lvl ?? 0, count: 1, rerolls: fp.rerolls ?? 0,
        });
      } else if (fp.challenge && typeof fp.challenge === "object") {
        newRivens.push({
          item_id: itemId, item_type: t, mod_name: "",
          riven_state: "revealed",
          compat: null, challenge_type: fp.challenge.Type ?? null, challenge_complication: fp.challenge.Complication ?? null,
          lvl_req: null, polarity: null, buffs: [], curses: [],
          mod_rank: 0, count: 1, rerolls: 0,
        });
      }
    }
    if (newRivens.length > 0) setApiRivens(newRivens);

    // Currency (virtual paths not in the WFCD catalog) — mirrors monitor.rs's
    // apply_blob_to_state so Companion API users see the same Credits/Endo/
    // Platinum readouts in the Inventory grid that scanner users already get.
    if (data.RegularCredits != null) apiQty["/_currency/Credits"] = data.RegularCredits;
    if (data.FusionPoints != null) apiQty["/_currency/Endo"] = data.FusionPoints;
    if (data.PremiumCredits != null) {
      const free = data.PremiumCreditsFree ?? 0;
      apiQty["/_currency/Platinum"] = data.PremiumCredits - free;
      apiQty["/_currency/PlatinumGift"] = free;
    }

    // Cosmetics (FlavourItems + WeaponSkins) — occurrence-counted, mirrors
    // monitor.rs/memory_scanner.rs so glyphs/sigils/skins show up generically
    // in the Inventory grid for Companion API users too.
    for (const c of [
      ...(Array.isArray(data.FlavourItems) ? data.FlavourItems : []),
      ...(Array.isArray(data.WeaponSkins) ? data.WeaponSkins : []),
    ]) {
      const t: string = c.ItemType;
      if (!t || !t.startsWith("/Lotus/")) continue;
      apiQty[t] = (apiQty[t] ?? 0) + 1;
    }

    setApiQuantities(prev => {
      const changes = Object.entries(apiQty)
        .filter(([k, v]) => (prev[k] ?? 0) !== v)
        .map(([k, v]) => ({ item_name: k, old_qty: prev[k] ?? 0, new_qty: v }));
      if (changes.length > 0)
        invoke("log_api_changes", { changes }).catch(() => {});
      return apiQty;
    });
    if (data.PlayerLevel != null) setMasteryRank(data.PlayerLevel);

    // Extract Archon Shard data from Suits
    const COLOR_MAP: Record<string, { type: string; color: string; tauColor: string }> = {
      ACC_RED:     { type: "Crimson",  color: "#e04040", tauColor: "#ff7070" },
      ACC_BLUE:    { type: "Azure",    color: "#4488ff", tauColor: "#77aaff" },
      ACC_GREEN:   { type: "Viridian", color: "#44cc66", tauColor: "#66ff99" },
      ACC_YELLOW:  { type: "Amber",    color: "#ffaa00", tauColor: "#ffcc44" },
      ACC_PURPLE:  { type: "Violet",   color: "#9944ff", tauColor: "#bb77ff" },
    };
    const newShards: Record<string, { type: string; tauforged: boolean; color: string; boost: string }[]> = {};
    for (const suit of (Array.isArray(data.Suits) ? data.Suits : [])) {
      const upgrades = suit.ArchonCrystalUpgrades;
      if (!Array.isArray(upgrades) || upgrades.length === 0) continue;
      const uniqueName: string = suit.ItemType ?? "";
      if (!uniqueName) continue;
      newShards[uniqueName] = upgrades.map((u: any) => {
        const colorRaw: string = (u.Color ?? "").toUpperCase();
        const upgradeType: string = u.UpgradeType ?? "";
        const tauforged = colorRaw.includes("MYTHIC") || colorRaw.includes("TAU") || upgradeType.toLowerCase().includes("tau");
        const colorKey = Object.keys(COLOR_MAP).find(k => colorRaw.startsWith(k)) ?? "";
        const info = COLOR_MAP[colorKey] ?? { type: colorRaw || "Unknown", color: "#b0b0b0", tauColor: "#d0d0d0" };
        const seg = upgradeType.split("/").pop() ?? "";
        const boost = seg
          .replace(/ArchonCrystalUpgrade(Warframe)?/g, "")
          .replace(/([A-Z])/g, " $1").trim();
        return { type: info.type, tauforged, color: tauforged ? info.tauColor : info.color, boost };
      });
    }
    if (Object.keys(newShards).length > 0) setArchonShards(prev => ({ ...prev, ...newShards }));

    // Extract subsumed warframes from InfestedFoundry (Helminth)
    const consumed = data.InfestedFoundry?.ConsumedSuits;
    if (Array.isArray(consumed)) {
      const s = new Set<string>(
        consumed.map((e: any) => (typeof e === "string" ? e : e?.ItemType ?? "")).filter(Boolean)
      );
      setSubsummedWarframes(s);
    }

    // XPInfo from API → fill mastery data for items no longer owned (memory scanner can't see these)
    if (Array.isArray(data.XPInfo)) {
      const xpMastery: Record<string, number> = {};
      for (const x of data.XPInfo) {
        if (!x.ItemType || x.XP == null) continue;
        xpMastery[x.ItemType] = Math.min(30, Math.floor(x.XP / 30_000));
      }
      setMasteryData(prev => ({ ...xpMastery, ...prev }));
      invoke("save_mastery_data", { data: xpMastery }).catch(() => {});
    }

    // PendingRecipes from API → update crafting state (authoritative, covers cases memory scanner misses)
    if (Array.isArray(data.PendingRecipes) && data.PendingRecipes.length > 0) {
      const apiJobs: CraftingJob[] = data.PendingRecipes
        .filter((r: any) => r.ItemType)
        .map((r: any) => {
          const completionMs = r.CompletionDate?.$date?.$numberLong
            ? Number(r.CompletionDate.$date.$numberLong)
            : 0;
          const item = catalogRef.current.find(i => i.unique_name === r.ItemType);
          const name = item?.name ?? r.ItemType.split("/").pop() ?? r.ItemType;
          return { unique_name: r.ItemType, item_name: name, completion_ms: completionMs };
        });
      setCrafting(prev => {
        const merged = [...apiJobs];
        for (const job of prev) {
          if (!merged.some(c => c.unique_name === job.unique_name)) merged.push(job);
        }
        return merged;
      });
    }
    const now = Math.floor(Date.now() / 1000);
    setLastApiRefresh(now);

    // Diff against previous API quantities to generate changelog entries
    const prev = prevApiQtyRef.current;
    if (Object.keys(prev).length > 0) {
      const allKeys = new Set([...Object.keys(prev), ...Object.keys(apiQty)]);
      const changes: ChangeLogEntry[] = [];
      for (const key of allKeys) {
        const oldQty = prev[key] ?? 0;
        const newQty = apiQty[key] ?? 0;
        if (oldQty !== newQty) {
          const item = catalogRef.current.find(i => i.unique_name === key);
          const name = item?.name ?? key.split("/").pop() ?? key;
          changes.push({ id: 0, unique_name: key, item_name: name, old_qty: oldQty, new_qty: newQty, delta: newQty - oldQty, timestamp: now });
        }
      }
      if (changes.length > 0) {
        setChangeLog(prev => [...changes, ...prev].slice(0, 200));
        setChangeLogArrivalToken(token => token + 1);
        setLastChanged(prev => {
          const next = { ...prev };
          for (const c of changes) next[c.unique_name] = c.timestamp;
          return next;
        });
      }
    }
    prevApiQtyRef.current = { ...apiQty };
  }, []); // eslint-disable-line

  // ── Fetch item list ─────────────────────────────────────────────────────────
  const handleFetch = async () => {
    setFetching(true);
    setFetchMsg("Fetching…");
    const wasMonitoring = monitoring;
    if (wasMonitoring) {
      await invoke("stop_monitor");
      setMonitoring(false);
    }
    try {
      const count = await invoke<number>("fetch_item_list", { force: true });
      setItemCount(count);
      const items = await invoke<CatalogItem[]>(TAURI_COMMANDS.GET_ALL_ITEMS);
      setCatalog(items);
      catalogRef.current = items;
      const status = await invoke<ItemListStatus>("get_item_list_status");
      setRecipeCount(status.recipe_count);
      setFetchMsg(`Loaded ${count.toLocaleString()} items, ${status.recipe_count.toLocaleString()} recipes`);
      setItemsRefreshKey(k => k + 1);
      invoke("prewarm_image_cache").catch(() => {});
    } catch (e) {
      setFetchMsg(`Error: ${e}`);
    } finally {
      setFetching(false);
      if (wasMonitoring) {
        await invoke("start_monitor");
        setMonitoring(true);
      }
    }
  };

  // ── Bootstrap ───────────────────────────────────────────────────────────────
  useEffect(() => {
    // Restore all inventory data from the single Rust-side cache file
    invoke<SavedApiInventory>("get_saved_inventory")
      .then(data => {
        if (Object.keys(data.apiQuantities).length > 0) setApiQuantities(data.apiQuantities);
        if (data.apiModCopies.length > 0) setApiModCopies(data.apiModCopies);
        if (data.consumedSuits.length > 0) setSubsummedWarframes(new Set(data.consumedSuits));
      })
      .catch(() => {})
      .finally(() => { inventoryRestoredRef.current = true; });

    invoke<string>("get_system_locale").catch(() => {});
    invoke<string | null>("get_player_name").then(name => { if (name) setPlayerName(name); }).catch(() => {});
    invoke<CatalogItem[]>(TAURI_COMMANDS.GET_ALL_ITEMS).then(items => { setCatalog(items); catalogRef.current = items; });
    invoke<QuantityMap>(TAURI_COMMANDS.GET_CURRENT_QUANTITIES)
      .then(setQuantities)
      .catch(() => {})
      .finally(() => {
        if (!inventoryReadyRef.current) {
          inventoryReadyRef.current = true;
          setInventoryReady(true);
        }
      });
    invoke<ChangeLogEntry[]>("get_change_log", { limit: 200 }).then(log => {
      setChangeLog(log);
      const lc: Record<string, number> = {};
      for (const c of log) lc[c.unique_name] = Math.max(lc[c.unique_name] ?? 0, c.timestamp);
      setLastChanged(lc);
    });
    invoke<ItemListStatus>("get_item_list_status").then(s => {
      setItemCount(s.count);
      setRecipeCount(s.recipe_count);
    });

    // Explicitly complete the startup catalogue refresh instead of relying on
    // a background event that may have fired before this WebView subscribed.
    invoke<number>("fetch_item_list", { force: false })
      .then(async count => {
        const [items, status] = await Promise.all([
          invoke<CatalogItem[]>(TAURI_COMMANDS.GET_ALL_ITEMS),
          invoke<ItemListStatus>("get_item_list_status"),
        ]);
        setCatalog(items);
        catalogRef.current = items;
        setItemCount(count);
        setRecipeCount(status.recipe_count);
        setItemsRefreshKey(k => k + 1);
        invoke("prewarm_image_cache").catch(() => {});
      })
      .catch(() => { /* keep cached data and the manual refresh fallback */ });

    getVersion().catch(() => {});

    invoke<string>("get_img_cache_dir").then(setImgCacheDir).catch(() => {});
    invoke("prewarm_image_cache").catch(() => {});
  }, []);

  // Reload catalogue state after the startup or manual refresh rebuilds it.
  useEffect(() => {
    const unlisten = listen<number>(TAURI_EVENTS.CATALOGUE_UPDATED, async () => {
      try {
        const items = await invoke<CatalogItem[]>(TAURI_COMMANDS.GET_ALL_ITEMS);
        setCatalog(items);
        catalogRef.current = items;
        const status = await invoke<ItemListStatus>("get_item_list_status");
        setItemCount(status.count);
        setRecipeCount(status.recipe_count);
        setItemsRefreshKey(k => k + 1);
        invoke("prewarm_image_cache").catch(() => {});
      } catch {
        /* keep the current catalogue; the manual refresh button still works */
      }
    });
    return () => { unlisten.then(fn => fn()); };
  }, []);

  // ── Inventory update events ─────────────────────────────────────────────────
  useEffect(() => {
    const unlisten = listen<InventoryUpdate>(TAURI_EVENTS.INVENTORY_UPDATE, (e) => {
      const p = e.payload;
      setLastInventoryScanAt(p.scanned_at);
      if (!inventoryReadyRef.current) {
        inventoryReadyRef.current = true;
        setInventoryReady(true);
      }
      setQuantities(prev => {
        const next = p.quantities;
        const prevKeys = Object.keys(prev);
        const nextKeys = Object.keys(next);
        if (prevKeys.length !== nextKeys.length) return next;
        for (const k of nextKeys) { if (next[k] !== prev[k]) return next; }
        return prev;
      });
      if (p.crafting) setCrafting(p.crafting);
      if (p.mastery_rank != null) setMasteryRank(p.mastery_rank);
      if (p.player_name) setPlayerName(p.player_name);
      if (p.mastery_data && Object.keys(p.mastery_data).length > 0)
        setMasteryData(prev => ({ ...prev, ...p.mastery_data }));
      if (!p.warframe_running && wfConnectedRef.current) {
        manualCredsRef.current = null;
      }
      setWarframeRunning(p.warframe_running);
      if (p.consumed_suits && p.consumed_suits.length > 0) {
        setSubsummedWarframes(prev => {
          const next = new Set(prev);
          for (const s of p.consumed_suits!) next.add(s);
          return next;
        });
      }
      if (p.mods && Object.keys(p.mods).length > 0) {
        setScannerMods(p.mods);
      }
      if (p.socketed_shards) {
        const SHARD_COLORS: { prefix: string; type: string; colorHex: string; tauHex: string }[] = [
          { prefix: "ACC_RED",    type: "Crimson",  colorHex: "#e04040", tauHex: "#ff7070" },
          { prefix: "ACC_BLUE",   type: "Azure",    colorHex: "#4488ff", tauHex: "#77aaff" },
          { prefix: "ACC_GREEN",  type: "Viridian", colorHex: "#44cc66", tauHex: "#66ff99" },
          { prefix: "ACC_YELLOW", type: "Amber",    colorHex: "#ffaa00", tauHex: "#ffcc44" },
          { prefix: "ACC_PURPLE", type: "Violet",   colorHex: "#9944ff", tauHex: "#bb77ff" },
        ];
        const INT_TO_ACC = ["ACC_RED","ACC_BLUE","ACC_GREEN","ACC_YELLOW","ACC_PURPLE"];
        const parsed: Record<string, { type: string; tauforged: boolean; color: string; boost?: string }[]> = {};
        for (const [wfPath, shards] of Object.entries(p.socketed_shards)) {
          parsed[wfPath] = shards.map(s => {
            let raw = s.color.toUpperCase();
            if (/^\d+$/.test(raw)) {
              const n = parseInt(raw);
              raw = INT_TO_ACC[n % 5] ?? raw;
            }
            const tauforged = raw.includes("MYTHIC") || raw.includes("TAU") || parseInt(s.color) >= 5;
            const entry = SHARD_COLORS.find(e => raw.startsWith(e.prefix));
            const colorInfo = entry ?? { type: "Unknown", colorHex: "#b0b0b0", tauHex: "#d0d0d0" };
            const seg = s.upgrade_type.split("/").pop() ?? "";
            const boostRaw = seg.replace(/^ArchonCrystalUpgrade(?:Warframe|Companion)?/, "");
            const boost = boostRaw.replace(/([A-Z])/g, " $1").trim() || undefined;
            const color = tauforged ? colorInfo.tauHex : colorInfo.colorHex;
            return { type: colorInfo.type, tauforged, color, boost };
          });
        }
        if (p.is_full_pass) {
          setArchonShards(parsed);
        } else if (Object.keys(parsed).length > 0) {
          setArchonShards(prev => ({ ...prev, ...parsed }));
        }
      }
      if (p.forma_counts) {
        if (p.is_full_pass) {
          setFormaData(p.forma_counts);
        } else if (Object.keys(p.forma_counts).length > 0) {
          setFormaData(prev => ({ ...prev, ...p.forma_counts }));
        }
      }
      if (p.changes.length > 0) {
        setChangeLog(prev => [...p.changes, ...prev].slice(0, 200));
        setChangeLogArrivalToken(token => token + 1);
        setLastChanged(prev => {
          const next = { ...prev };
          for (const c of p.changes) next[c.unique_name] = c.timestamp;
          return next;
        });
      }
    });
    return () => { unlisten.then(fn => fn()); };
  }, []);

  // ── Player name (immediate, from EE.log "Logged in NAME") ──────────────────
  useEffect(() => {
    const unlisten = listen<string>(TAURI_EVENTS.PLAYER_NAME, e => {
      setPlayerName(e.payload);
    });
    return () => { unlisten.then(fn => fn()); };
  }, []);

  // ── Auto-refresh API: 60 s while connecting, 300 s once connected ──────────
  useEffect(() => {
    if (COMPANION_API_SUSPENDED) {
      setWfConnected(false);
      wfConnectedRef.current = false;
      return;
    }
    let cancelled = false;
    let timeoutId: ReturnType<typeof setTimeout>;

    const doFetch = async () => {
      if (cancelled) return;
      let accountId = "", nonce = "", steamId = "";
      try {
        [accountId, nonce, steamId] = await invoke<WarframeCredentials>("scan_warframe_credentials");
        manualCredsRef.current = { accountId, nonce };
      } catch {
        const mc = manualCredsRef.current;
        if (!mc) { schedule(); return; }
        accountId = mc.accountId; nonce = mc.nonce; steamId = "";
      }
      try {
        const args: WarframeInventoryRequest = { accountId, nonce, steamId };
        const data = await invoke<unknown>("fetch_warframe_inventory", args);
        if (!cancelled) {
          applyInventoryData(data);
          setWfConnected(true);
          wfConnectedRef.current = true;
          setWarframeRunning(true);
        }
      } catch {
        if (wfConnectedRef.current) {
          setWfConnected(false);
          wfConnectedRef.current = false;
          manualCredsRef.current = null;
        }
      }
      schedule();
    };

    const schedule = () => {
      if (cancelled) return;
      timeoutId = setTimeout(doFetch, wfConnectedRef.current ? 300_000 : 60_000);
    };

    doFetch();
    return () => { cancelled = true; clearTimeout(timeoutId); };
  }, [applyInventoryData]); // eslint-disable-line

  // ── Persist API inventory data to inventory_state_cache.json via Rust ──────
  useEffect(() => {
    if (!inventoryRestoredRef.current) return;
    if (Object.keys(apiQuantities).length === 0 && apiModCopies.length === 0 && subsummedWarframes.size === 0) return;
    const args: SaveApiInventoryArgs = {
      apiQuantities,
      apiModCopies,
      consumedSuits: [...subsummedWarframes],
    };
    invoke(TAURI_COMMANDS.SAVE_API_INVENTORY, args).catch(() => {});
  }, [apiQuantities, apiModCopies, subsummedWarframes]);

  // ── Persist API riven fingerprint detail to inventory_state_cache.json ─────
  useEffect(() => {
    if (!inventoryRestoredRef.current) return;
    if (apiRivens.length === 0) return;
    invoke(TAURI_COMMANDS.SAVE_API_RIVENS, { rivens: apiRivens }).catch(() => {});
  }, [apiRivens]);

  return {
    // State
    catalog,
    quantities,
    apiQuantities,
    apiModCopies,
    scannerMods,
    crafting,
    masteryRank,
    masteryData,
    playerName,
    subsummedWarframes,
    archonShards,
    formaData,
    lastApiRefresh,
    inventoryReady,
    lastInventoryScanAt,
    changeLog,
    changeLogArrivalToken,
    lastChanged,
    monitoring,
    warframeRunning,
    itemCount,
    recipeCount,
    fetching,
    fetchMsg,
    wfConnected,
    imgCacheDir,
    itemsRefreshKey,

    // Refs
    wfConnectedRef,
    inventoryRestoredRef,
    catalogRef,
    manualCredsRef,

    // Callbacks
    handleFetch,
    applyInventoryData,

    // Setters
    setCatalog,
    setQuantities,
    setApiQuantities,
    setApiModCopies,
    setScannerMods,
    setCrafting,
    setMasteryData,
    setArchonShards,
    setFormaData,
    setChangeLog,
    setLastChanged,
    setWfConnected,
    setItemsRefreshKey,
    setMonitoring,
    setWarframeRunning,
    setPlayerName,
  };
}
