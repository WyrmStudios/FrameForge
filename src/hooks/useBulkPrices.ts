import { useEffect, useState, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { TAURI_COMMANDS, TAURI_EVENTS } from "../constants/tauri";

// ─── Singleton ────────────────────────────────────────────────────────────────
// One IPC call for every consumer needing the daily bulk price map
// (lowercase display name → plat). Backend keeps it fresh; consumers
// re-fetch via refresh() when caches are refreshed.

type Snapshot = {
  bulkPrices: Map<string, number>;
  loaded: boolean;
  error: boolean;
};

let current: Snapshot = { bulkPrices: new Map(), loaded: false, error: false };
let inFlight: Promise<void> | null = null;
let refreshQueued = false;
let listenerReady: Promise<void> | null = null;
const subscribers = new Set<(s: Snapshot) => void>();

function publish(next: Snapshot) {
  current = next;
  for (const notify of subscribers) notify(next);
}

function fetchOnce(): Promise<void> {
  if (inFlight) return inFlight;
  if (!current.loaded && current.error) publish({ ...current, error: false });
  inFlight = invoke<Record<string, number>>(TAURI_COMMANDS.GET_BULK_PRICES)
    .then(raw => {
      if (refreshQueued) return;
      const prices = new Map<string, number>();
      for (const [name, price] of Object.entries(raw ?? {})) prices.set(name, price);
      publish({ bulkPrices: prices, loaded: true, error: false });
    })
    .catch(() => {
      if (!refreshQueued && !current.loaded) publish({ ...current, error: true });
    })
    .finally(() => {
      inFlight = null;
      if (refreshQueued) {
        refreshQueued = false;
        void fetchOnce();
      }
    });
  return inFlight;
}

function ensureListener(): Promise<void> {
  listenerReady ??= listen(TAURI_EVENTS.BULK_PRICES_UPDATED, () => {
    if (inFlight) {
      refreshQueued = true;
    } else {
      void fetchOnce();
    }
  }).then(() => undefined).catch(error => {
    listenerReady = null;
    throw error;
  });
  return listenerReady;
}

// ─── Hook ─────────────────────────────────────────────────────────────────────

export interface UseBulkPricesReturn {
  bulkPrices: Map<string, number>;
  loaded: boolean;
  error: boolean;
  refresh: () => void;
}

export function useBulkPrices(): UseBulkPricesReturn {
  const [snapshot, setSnapshot] = useState(current);

  useEffect(() => {
    subscribers.add(setSnapshot);
    if (subscribers.size === 1) {
      void ensureListener().then(() => fetchOnce(), () => fetchOnce());
    } else {
      setSnapshot(current);
    }
    return () => {
      subscribers.delete(setSnapshot);
    };
  }, []);

  const refresh = useCallback(() => {
    void ensureListener().then(() => fetchOnce(), () => fetchOnce());
  }, []);

  return { ...snapshot, refresh };
}
