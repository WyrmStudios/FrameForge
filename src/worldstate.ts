import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { WorldState } from "./types/worldstate";

// One poll per window, shared by every consumer. A timer per component would
// give the Timers tab and the modular sidebar unsynchronised views of the same
// data, and each fetch crosses the IPC boundary and re-parses ~1MB of JSON even
// when the backend serves it from cache.

const POLL_MS = 60_000;

type Snapshot = { worldState: WorldState | null; error: string };

let current: Snapshot = { worldState: null, error: "" };
let inFlight: Promise<void> | null = null;
let timer: ReturnType<typeof setInterval> | null = null;
const subscribers = new Set<(s: Snapshot) => void>();

function publish(next: Snapshot) {
  current = next;
  for (const notify of subscribers) notify(next);
}

function fetchOnce(): Promise<void> {
  inFlight ??= invoke<WorldState>("fetch_worldstate")
    .then(ws => publish({ worldState: ws, error: "" }))
    .catch(e => publish({ ...current, error: String(e) }))
    .finally(() => { inFlight = null; });
  return inFlight;
}

export function useWorldState(): Snapshot & { refresh: () => void } {
  const [snapshot, setSnapshot] = useState(current);

  useEffect(() => {
    subscribers.add(setSnapshot);
    if (subscribers.size === 1) {
      fetchOnce();
      timer = setInterval(fetchOnce, POLL_MS);
    } else {
      setSnapshot(current);
    }
    return () => {
      subscribers.delete(setSnapshot);
      if (subscribers.size === 0 && timer) {
        clearInterval(timer);
        timer = null;
      }
    };
  }, []);

  return { ...snapshot, refresh: fetchOnce };
}
