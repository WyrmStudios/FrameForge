import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { CacheStatuses } from "../types/cache";

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
  worldstate: "Worldstate",
  "bulk-prices": "Bulk Prices",
  catalogue: "Catalogue",
  "drop-data": "Drop Data",
  "riven-db": "Riven DB",
  "wfm-top": "WFM Top Items",
};

export default function CacheStatusChip() {
  const [statuses, setStatuses] = useState<CacheStatuses>({});
  const [open, setOpen] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const chipRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    invoke<CacheStatuses>("get_cache_statuses").then(setStatuses).catch(() => {});
    const unsub = listen<CacheStatuses>("cache-status", (e) => setStatuses(e.payload));
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

  const handleRefresh = async () => {
    setRefreshing(true);
    await invoke("refresh_all_caches").catch(() => {});
    setTimeout(() => setRefreshing(false), 2000);
  };

  return (
    <div ref={chipRef} style={{ position: "relative" }}>
      <div
        className={`conn-chip conn-${state}`}
        onClick={() => setOpen((v) => !v)}
        title="Data cache status"
      >
        <span className="conn-dot" />
        <span className="conn-label">Data</span>
        <span className="conn-detail">
          {state === "online" ? "fresh" : state === "warn" ? "stale" : "offline"}
        </span>
      </div>

      {open && (
        <div style={{
          position: "absolute", top: "calc(100% + 6px)", right: 0,
          background: "var(--surface, #1e1e2e)", border: "1px solid var(--border, #333)",
          borderRadius: 8, padding: "10px 12px", minWidth: 220, zIndex: 999,
          boxShadow: "0 4px 16px rgba(0,0,0,0.4)",
        }}>
          <div style={{ display: "flex", flexDirection: "column", gap: 6, marginBottom: 10 }}>
            {Object.entries(statuses).map(([key, s]) => (
              <div key={key} style={{
                display: "flex", justifyContent: "space-between", alignItems: "center",
                fontSize: 12, gap: 8,
              }}>
                <span style={{ color: "var(--text, #cdd6f4)", fontWeight: 500 }}>
                  {DISPLAY[key] ?? key}
                </span>
                <span style={{
                  color: s.source === "fresh" || s.source === "refreshed"
                    ? "#3fb950"
                    : s.source === "stale" ? "#d29922" : "#6e7681",
                  fontVariantNumeric: "tabular-nums",
                  fontSize: 11,
                }}>
                  {s.source} · {age(s.last_updated)}
                </span>
              </div>
            ))}
            {Object.keys(statuses).length === 0 && (
              <span style={{ color: "var(--muted, #6e7681)", fontSize: 12 }}>
                No cache data yet
              </span>
            )}
          </div>
          <button
            onClick={handleRefresh}
            disabled={refreshing}
            style={{
              width: "100%", padding: "5px 10px", fontSize: 12, cursor: "pointer",
              background: "var(--accent, #89b4fa)", color: "#1e1e2e",
              border: "none", borderRadius: 5, fontWeight: 600,
              opacity: refreshing ? 0.6 : 1,
            }}
          >
            {refreshing ? "Refreshing…" : "Refresh all data"}
          </button>
        </div>
      )}
    </div>
  );
}
