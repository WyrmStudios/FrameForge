import { useState, useEffect, useMemo, useCallback, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { TAURI_COMMANDS, TAURI_EVENTS } from "../constants/tauri";
import { MODULAR_SECTION_ORDER_DEFAULT } from "../constants/settings";
import ModularWindow from "./ModularWindow";
import type { FissureWatch } from "../types/settings";
import type { CatalogItem, InventoryItem, QuantityMap } from "../types/items";
import type { InventoryUpdate } from "../types/inventory";
import type { SettingsFile, SettingsPatch } from "../types/tauri";

export default function ModularWindowPage() {
  const [tracked, setTracked] = useState<string[]>([]);
  const [favorites, setFavorites] = useState<string[]>([]);
  const [timerFavorites, setTimerFavorites] = useState<string[]>([]);
  const [fissureWatches, setFissureWatches] = useState<FissureWatch[]>([]);
  const [quantities, setQuantities] = useState<QuantityMap>({});
  const [catalog, setCatalog] = useState<CatalogItem[]>([]);
  const inventory = useMemo<Record<string, InventoryItem>>(() => {
    const pathToCatalog = new Map<string, CatalogItem>();
    for (const item of catalog) pathToCatalog.set(item.unique_name, item);
    const inv: Record<string, InventoryItem> = {};
    for (const [path, qty] of Object.entries(quantities)) {
      const cat = pathToCatalog.get(path);
      const name = cat?.name ?? path;
      const entry: InventoryItem = {
        unique_name: path,
        quantity: qty,
        mastery_rank: 0,
        archon_shards: [],
        forma_count: 0,
        subsumed: false,
        vaulted: cat?.vaulted ?? null,
        category: cat?.category ?? "",
        ducat_price: cat?.ducats ?? null,
        wfm_price: null,
        image_name: cat?.image_name ?? null,
        mastery_req: cat?.mastery_req ?? null,
      };
      inv[name] = entry;
      if (path !== name) inv[path] = entry;
    }
    return inv;
  }, [catalog, quantities]);
  const [sectionOrder, setSectionOrder] = useState<string[]>([...MODULAR_SECTION_ORDER_DEFAULT]);

  const popoutSettingsLoadedRef = useRef(false);
  useEffect(() => {
    invoke<string>(TAURI_COMMANDS.LOAD_SETTINGS).then(json => {
      // A missing file is a first launch and safe to write to.
      if (!json) { popoutSettingsLoadedRef.current = true; return; }
      try {
        const s = JSON.parse(json) as SettingsFile;
        if (Array.isArray(s.tracked)) setTracked(s.tracked);
        if (Array.isArray(s.favorites)) setFavorites(s.favorites);
        if (Array.isArray(s.timerFavorites)) setTimerFavorites(s.timerFavorites);
        if (Array.isArray(s.fissureWatches)) setFissureWatches(s.fissureWatches);
        if (Array.isArray(s.modularSectionOrder)) {
          const order: string[] = s.modularSectionOrder;
          if (!order.includes("timers")) order.push("timers");
          if (!order.includes("fissures")) order.push("fissures");
          setSectionOrder(order);
        }
      } catch {}
      // Unblock saving even if the file failed to parse, since the backend
      // refuses to overwrite a settings.json that is not a valid JSON object.
      popoutSettingsLoadedRef.current = true;
    }).catch(() => {});
    invoke<CatalogItem[]>(TAURI_COMMANDS.GET_ALL_ITEMS).then(setCatalog).catch(() => {});
    invoke<QuantityMap>(TAURI_COMMANDS.GET_CURRENT_QUANTITIES).then(setQuantities).catch(() => {});
  }, []);

  useEffect(() => {
    const unlisten = listen<InventoryUpdate>(TAURI_EVENTS.INVENTORY_UPDATE, e => {
      setQuantities(e.payload.quantities);
    });
    return () => { unlisten.then(fn => fn()); };
  }, []);

  useEffect(() => {
    const unlisten = listen(TAURI_EVENTS.SETTINGS_UPDATED, () => {
      invoke<string>(TAURI_COMMANDS.LOAD_SETTINGS).then(json => {
        if (!json) return;
        try {
          const s = JSON.parse(json) as SettingsFile;
          if (Array.isArray(s.tracked)) setTracked(s.tracked);
          if (Array.isArray(s.favorites)) setFavorites(s.favorites);
          if (Array.isArray(s.timerFavorites)) setTimerFavorites(s.timerFavorites);
          if (Array.isArray(s.fissureWatches)) setFissureWatches(s.fissureWatches);
          if (Array.isArray(s.modularSectionOrder)) setSectionOrder(s.modularSectionOrder);
        } catch {}
      }).catch(() => {});
    });
    return () => { unlisten.then(fn => fn()); };
  }, []);

  const saveModularSettings = useCallback((patch: SettingsPatch) => {
    if (!popoutSettingsLoadedRef.current) {
      console.error("save_settings skipped: settings not loaded yet in pop-out");
      return;
    }
    invoke(TAURI_COMMANDS.SAVE_SETTINGS, { json: JSON.stringify(patch) }).catch((e) => {
      console.error("save_settings failed:", e);
    });
  }, []); // eslint-disable-line

  useEffect(() => {
    const win = getCurrentWindow();
    let t: ReturnType<typeof setTimeout> | null = null;
    const save = () => {
      if (t) clearTimeout(t);
      t = setTimeout(() => {
        Promise.all([win.outerPosition(), win.outerSize()]).then(([pos, size]) => {
          saveModularSettings({
            modularWinX: pos.x, modularWinY: pos.y,
            modularWinWidth: size.width, modularWinHeight: size.height,
          });
        }).catch(() => {});
      }, 400);
    };
    const unlistenMove = win.onMoved(save);
    const unlistenResize = win.onResized(save);
    return () => {
      if (t) clearTimeout(t);
      unlistenMove.then(fn => fn());
      unlistenResize.then(fn => fn());
    };
  }, [saveModularSettings]);

  const handleTrackedChange = (next: string[]) => {
    setTracked(next);
    saveModularSettings({ tracked: next });
  };
  const handleUntrack = (id: string) => {
    const next = tracked.filter(i => i !== id);
    setTracked(next);
    saveModularSettings({ tracked: next });
  };
  const handleFavoritesChange = (next: string[]) => {
    setFavorites(next);
    saveModularSettings({ favorites: next });
  };
  const handleUnfavorite = (id: string) => {
    const next = favorites.filter(i => i !== id);
    setFavorites(next);
    saveModularSettings({ favorites: next });
  };
  const handleSectionOrderChange = (next: string[]) => {
    setSectionOrder(next);
    saveModularSettings({ modularSectionOrder: next });
  };

  return (
    <div style={{ display: "flex", height: "100vh", background: "var(--surface)", overflow: "hidden" }}>
      <ModularWindow
        tracked={tracked}
        onTrackedChange={handleTrackedChange}
        onUntrack={handleUntrack}
        favorites={favorites}
        onFavoritesChange={handleFavoritesChange}
        onUnfavorite={handleUnfavorite}
        timerFavorites={timerFavorites}
        onTimerFavoritesChange={setTimerFavorites}
        onTimerUnfavorite={id => setTimerFavorites(prev => prev.filter(x => x !== id))}
        fissureWatches={fissureWatches}
        inventory={inventory}
        catalog={catalog}
        sectionOrder={sectionOrder}
        onSectionOrderChange={handleSectionOrderChange}
      />
    </div>
  );
}
