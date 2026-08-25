import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

type Source = "fresh" | "refreshed" | "stale" | "fallback";

type CacheStatus = {
  source: Source;
  last_updated: number | null;
  warning: string | null;
};

type Statuses = Record<string, CacheStatus>;

/** Worst rung any cache is currently on decides the chip's colour. */
function overall(statuses: Statuses): "online" | "warn" | "offline" {
  const values = Object.values(statuses);
  if (values.some(s => s.source === "fallback")) return "offline";
  if (values.some(s => s.source === "stale" || s.warning)) return "warn";
  return "online";
}

function age(ts: number | null): string {
  if (!ts) return "never";
  const mins = Math.floor((Date.now() / 1000 - ts) / 60);
  if (mins < 1) return "just now";
  if (mins < 60) return `${mins}m ago`;
  const hours = Math.floor(mins / 60);
  if (hours < 24) return `${hours}h ago`;
  return `${Math.floor(hours / 24)}d ago`;
}

export default function CacheStatusChip() {
  const [statuses, setStatuses] = useState<Statuses>({});
  const [open, setOpen] = useState(false);
  const [refreshing, setRefreshing] = useState(false);

  useEffect(() => {
    invoke<Statuses>("get_cache_statuses").then(setStatuses).catch(() => {});
    const unlisten = listen<Statuses>("cache-status", e => setStatuses(e.payload));
    return () => { unlisten.then(fn => fn()); };
  }, []);

  const state = overall(statuses);
  const names = Object.keys(statuses).sort();
  const detail = state === "online" ? "Fresh" : state === "warn" ? "Stale" : "Offline";

  return (
    <span style={{ position: "relative" }}>
      <span
        className={`conn-chip conn-${state}`}
        title="Cached game data. Click for details."
        style={{ cursor: "pointer" }}
        onClick={() => setOpen(o => !o)}
      >
        <span className="conn-dot" />
        <span className="conn-label">Data</span>
        <span className="conn-detail">{detail}</span>
      </span>
      {open && (
        <div
          style={{
            position: "absolute", top: "calc(100% + 6px)", right: 0, zIndex: 50,
            minWidth: 280, padding: 10, borderRadius: 6,
            background: "var(--panel, #1b1d22)", border: "1px solid rgba(255,255,255,.12)",
            boxShadow: "0 6px 18px rgba(0,0,0,.5)", fontSize: 11, lineHeight: 1.5,
          }}
        >
          {names.length === 0 && <div style={{ opacity: .7 }}>No refresh has run yet.</div>}
          {names.map(name => {
            const s = statuses[name];
            return (
              <div key={name} style={{ marginBottom: 6 }}>
                <div style={{ display: "flex", justifyContent: "space-between", gap: 12 }}>
                  <strong>{name.replace(/-v\d+\.json$/, "")}</strong>
                  <span style={{ opacity: .8 }}>{s.source} · {age(s.last_updated)}</span>
                </div>
                {s.warning && <div style={{ color: "#e0a052" }}>{s.warning}</div>}
              </div>
            );
          })}
          <button
            className="btn-secondary"
            style={{ marginTop: 4, width: "100%" }}
            disabled={refreshing}
            onClick={() => {
              setRefreshing(true);
              invoke("refresh_all_caches")
                .catch(() => {})
                .finally(() => setTimeout(() => setRefreshing(false), 5000));
            }}
          >{refreshing ? "Refreshing…" : "Refresh all data"}</button>
        </div>
      )}
    </span>
  );
}
