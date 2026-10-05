import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useOverlayWindow } from "../lib/useOverlayWindow";
import { fmtMs } from "../TimerHelper";
import "../styles/Overlay.css";
import { TAURI_EVENTS } from "../constants/tauri";

interface RunSummary {
  node:             string;
  mission_type:     string;
  duration_sec:     number;
  rotations:        number;
  waves:            number;
  kills:            number;
  drone_kills:      number;
  host_telemetry:   boolean;
  vitus_mean:       number;
  vitus_per_minute: number;
}

const AUTO_HIDE_MS = 12_000;

function isRunSummary(p: unknown): p is RunSummary {
  const r = p as Partial<RunSummary> | null;
  return !!r
    && typeof r.node === "string"
    && typeof r.mission_type === "string"
    && typeof r.duration_sec === "number"
    && typeof r.vitus_mean === "number"
    && typeof r.vitus_per_minute === "number";
}

export default function ArbitrationOverlay() {
  const [run, setRun] = useState<RunSummary | null>(null);
  const root  = useOverlayWindow(340, "top-center");
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);

  const hide = useCallback(() => {
    if (timer.current) { clearTimeout(timer.current); timer.current = null; }
    setRun(null);
    getCurrentWindow().hide().catch(() => {});
  }, []);

  // The backend shows the window before it emits. An event that lands before
  // this listener exists, or one that fails validation, would otherwise leave
  // an empty window over the game with nothing to ever hide it.
  useEffect(() => {
    if (!run) getCurrentWindow().hide().catch(() => {});
  }, [run]);

  useEffect(() => {
    const unEnded = listen<unknown>(TAURI_EVENTS.ARBITRATION_RUN_ENDED, e => {
      if (!isRunSummary(e.payload)) return;
      if (timer.current) clearTimeout(timer.current);
      timer.current = setTimeout(hide, AUTO_HIDE_MS);
      setRun(e.payload);
    });
    return () => {
      unEnded.then(f => f());
      if (timer.current) clearTimeout(timer.current);
    };
  }, [hide]);

  if (!run) return null;

  const [progressValue, progressLabel] = run.mission_type === "defense"
    ? [run.waves, "waves"]
    : [run.rotations, run.rotations === 1 ? "rotation" : "rotations"];

  return (
    <div className="box-border flex w-full flex-col gap-1.5 rounded-8 border border-warning/50 bg-background/92 px-2.5 py-2 font-inherit text-12 text-(--text,#e6edf3) backdrop-blur-xs" ref={root}>
      <div className="flex items-baseline gap-2">
        <span className="whitespace-nowrap text-11 font-bold uppercase tracking-0.04 text-warning">Arbitration Complete</span>
        <span className="flex-1 overflow-hidden text-ellipsis whitespace-nowrap text-12 text-foreground/75">{run.node}</span>
        <button className="cursor-pointer border-0 bg-transparent px-0.5 py-0 text-13 leading-none text-foreground/35 hover:text-foreground/85" onClick={hide} title="Close">✕</button>
      </div>
      <div className="grid grid-cols-4 gap-x-2 gap-y-1">
        <div className="flex flex-col items-center rounded-5 bg-white/4 px-0.5 py-1">
          <span className="text-16 font-bold tabular-nums">{fmtMs(run.duration_sec * 1000)}</span>
          <span className="text-10 uppercase tracking-0.03 text-foreground/55">duration</span>
        </div>
        <div className="flex flex-col items-center rounded-5 bg-white/4 px-0.5 py-1">
          <span className="text-16 font-bold tabular-nums">{progressValue}</span>
          <span className="text-10 uppercase tracking-0.03 text-foreground/55">{progressLabel}</span>
        </div>
        <div className="flex flex-col items-center rounded-5 bg-white/4 px-0.5 py-1">
          <span className="text-16 font-bold tabular-nums">{run.host_telemetry ? run.kills : "–"}</span>
          <span className="text-10 uppercase tracking-0.03 text-foreground/55">kills</span>
        </div>
        <div className="flex flex-col items-center rounded-5 bg-white/4 px-0.5 py-1">
          <span className="text-16 font-bold tabular-nums">{run.host_telemetry ? run.drone_kills : "–"}</span>
          <span className="text-10 uppercase tracking-0.03 text-foreground/55">drones</span>
        </div>
        <div className="col-span-full flex items-baseline justify-center gap-1.5 rounded-5 bg-warning/12 px-0.5 py-1">
          <span className="text-16 font-bold tabular-nums text-warning">{run.vitus_per_minute.toFixed(2)}</span>
          <span className="text-10 uppercase tracking-0.03 text-foreground/55">vitus/min · ~{Math.round(run.vitus_mean)} total</span>
        </div>
      </div>
      {!run.host_telemetry && (
        <div className="text-center text-10 text-foreground/45">Kill counts are only logged when hosting.</div>
      )}
    </div>
  );
}
