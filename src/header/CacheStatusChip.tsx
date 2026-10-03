import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { TAURI_EVENTS } from "../constants/tauri";
import type { CacheStatuses } from "../types/cache";
import { CONN_CHIP, CONN_DOT, CONN_LABEL, CONN_STATUS } from "./connStatus";

function overall(statuses: CacheStatuses): "online" | "warn" | "offline" {
  const values = Object.values(statuses);
  if (values.length === 0) return "offline";
  if (values.some((s) => s.source === "fallback")) return "offline";
  if (values.some((s) => s.source === "stale" || s.warning)) return "warn";
  return "online";
}

function age(ts: number | null): string {
  if (ts == null) return "never";
  const secs = Math.floor(Date.now() / 1000) - ts;
  if (secs < 60) return `${secs}s ago`;
  if (secs < 3600) return `${Math.floor(secs / 60)}m ago`;
  if (secs < 86400) return `${Math.floor(secs / 3600)}h ago`;
  return `${Math.floor(secs / 86400)}d ago`;
}

const DISPLAY: Record<string, string> = {
  "arbitrations-v1.json": "Arbitrations",
  worldstate: "Worldstate",
  "bulk-prices": "Bulk Prices",
  catalogue: "Catalogue",
  "drop-data": "Drop Data",
  "riven-db": "Riven DB",
  "wfm-top": "WFM Top Items",
};

const SOURCE_CLASS: Record<string, string> = {
  fresh: "text-[#3fb950]",
  refreshed: "text-[#3fb950]",
  stale: "text-[#d29922]",
  fallback: "text-[#6e7681]",
};

export default function CacheStatusChip() {
  const [statuses, setStatuses] = useState<CacheStatuses>({});
  const [open, setOpen] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const chipRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    invoke<CacheStatuses>("get_cache_statuses").then(setStatuses).catch(() => {});
    const unsub = listen<CacheStatuses>(TAURI_EVENTS.CACHE_STATUS, (e) => setStatuses(e.payload));
    return () => { unsub.then((f) => f()); };
  }, []);

  useEffect(() => {
    if (!open) return;
    const handle = (e: MouseEvent) => {
      if (chipRef.current && !chipRef.current.contains(e.target as Node)) {
        setOpen(false);
      }
    };
    document.addEventListener("mousedown", handle);
    return () => document.removeEventListener("mousedown", handle);
  }, [open]);

  const state = overall(statuses);
  const s = CONN_STATUS[state];

  const handleRefresh = async () => {
    setRefreshing(true);
    await invoke("refresh_all_caches").catch(() => {});
    setTimeout(() => setRefreshing(false), 2000);
  };

  return (
    <div ref={chipRef} className="relative">
      <div
        className={`${CONN_CHIP} ${s.chip}`}
        onClick={() => setOpen((v) => !v)}
        title="Data cache status"
      >
        <span className={`${CONN_DOT} ${s.dot}`} />
        <span className={CONN_LABEL}>Data</span>
        <span className={s.detail}>
          {state === "online" ? "fresh" : state === "warn" ? "stale" : "offline"}
        </span>
      </div>

      {open && (
        <div className="absolute right-0 top-[calc(100%_+_6px)] z-[999] min-w-55 rounded-[8px] border border-border bg-surface px-3 py-2.5 shadow-[0_4px_16px_rgba(0,0,0,0.4)]">
          <div className="mb-2.5 flex flex-col gap-1.5">
            {Object.entries(statuses).map(([key, s]) => (
              <div key={key} className="flex items-center justify-between gap-2 text-[12px]">
                <span className="font-medium text-foreground">
                  {DISPLAY[key] ?? key}
                </span>
                <span className={`text-[11px] tabular-nums ${SOURCE_CLASS[s.source] ?? "text-[#6e7681]"}`}>
                  {s.source} · {age(s.last_updated)}
                </span>
              </div>
            ))}
            {Object.keys(statuses).length === 0 && (
              <span className="text-[12px] text-muted">
                No cache data yet
              </span>
            )}
          </div>
          <button
            onClick={handleRefresh}
            disabled={refreshing}
            className={`w-full cursor-pointer rounded-[5px] border-none bg-accent px-2.5 py-[5px] text-[12px] font-semibold text-[#1e1e2e] ${refreshing ? "opacity-60" : "opacity-100"}`}
          >
            {refreshing ? "Refreshing…" : "Refresh all data"}
          </button>
        </div>
      )}
    </div>
  );
}
