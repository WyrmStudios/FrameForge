import { useEffect, useState, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { TAURI_COMMANDS } from "../constants/tauri";
import type { WfmItem, WfmCachedPrices } from "../types/market";
import { normalizeForWfm } from "../utils";

// ─── Helpers ──────────────────────────────────────────────────────────────────


// ─── Singleton ────────────────────────────────────────────────────────────────
// One IPC call per consumer tree, shared by every module that needs WFM
// item data or cached prices.

type Snapshot = {
  wfmItems: WfmItem[];
  wfmPrices: Map<string, number>;
  loaded: boolean;
};

let current: Snapshot = { wfmItems: [], wfmPrices: new Map(), loaded: false };
let inFlight: Promise<void> | null = null;
const subscribers = new Set<(s: Snapshot) => void>();

function publish(next: Snapshot) {
  current = next;
  for (const notify of subscribers) notify(next);
}

function fetchOnce(): Promise<void> {
  inFlight ??= Promise.all([
    invoke<WfmItem[]>(TAURI_COMMANDS.FETCH_WFM_ITEMS),
    invoke<WfmCachedPrices>("wfm_get_cached_prices"),
  ])
    .then(([items, rawPrices]) => {
      // Build name→slug lookup
      const lookup = new Map<string, string>();
      for (const w of items) lookup.set(normalizeForWfm(w.item_name), w.url_name);

      // Build price map: slug→price and normalized name→price
      const prices = new Map<string, number>();
      if (rawPrices) {
        for (const [slug, price] of Object.entries(rawPrices)) {
          if (price != null) prices.set(slug, price);
        }
        for (const [norm, slug] of lookup) {
          const p = prices.get(slug);
          if (p != null) prices.set(norm, p);
        }
      }

      publish({ wfmItems: items, wfmPrices: prices, loaded: true });
    })
    .catch(() => {
      publish({ ...current, loaded: false });
    })
    .finally(() => {
      inFlight = null;
    });
  return inFlight;
}

// ─── Hook ─────────────────────────────────────────────────────────────────────

export interface UseMarketDataReturn {
  wfmItems: WfmItem[];
  wfmPrices: Map<string, number>;
  loaded: boolean;
  refresh: () => void;
}

export function useMarketData(): UseMarketDataReturn {
  const [snapshot, setSnapshot] = useState(current);

  useEffect(() => {
    subscribers.add(setSnapshot);
    if (subscribers.size === 1) {
      fetchOnce();
    } else {
      setSnapshot(current);
    }
    return () => {
      subscribers.delete(setSnapshot);
    };
  }, []);

  const refresh = useCallback(() => {
    inFlight = null;
    fetchOnce();
  }, []);

  return { ...snapshot, refresh };
}
