import { useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { fmtMs } from "../TimerHelper";
import {
  completed, filterRuns, summarize, MISSION_TYPES,
  type Breakdown, type Filters, type MissionType, type RunRecord,
} from "./arbitrationAnalytics";
import Sparkline from "../shared/Sparkline";
import { TierBadge } from "./TierSelect";
import { formatUnixTime } from "../lib/formatters";
import { TAURI_EVENTS } from "../constants/tauri";
import type { ClockFormat } from "../types/settings";

const RANGES: { label: string; value: number | "all" }[] = [
  { label: "7d", value: 7 }, { label: "30d", value: 30 }, { label: "90d", value: 90 }, { label: "All", value: "all" },
];

const cap = (s: string) => s.charAt(0).toUpperCase() + s.slice(1);
const fmtRate = (r: number | null) => (r === null ? "—" : r.toFixed(2));
const fmtDate = (iso: string | null, format: ClockFormat, locale: string) => {
  if (iso === null) return "unknown time";
  const when = new Date(iso);
  // The month name stays English; only the time follows the hour format.
  const date = when.toLocaleDateString("en-US", { month: "short", day: "numeric" });
  return `${date}, ${formatUnixTime(when.getTime() / 1000, format, locale)}`;
};
const endLabel: Record<Exclude<RunRecord["end_reason"], "mission_end">, string> = {
  aborted: "aborted", new_mission: "left early", unterminated: "unfinished",
};

function BreakdownTable({ label, rows, name }: { label: string; rows: Breakdown[]; name: (key: string) => string }) {
  if (rows.length === 0) return null;
  return (
    <div className="flex flex-col gap-1">
      <div className="text-10 uppercase tracking-0.04 text-muted">{label}</div>
      <div className="overflow-hidden rounded-6 border border-border/40 bg-white/3">
        <div className="grid grid-cols-[1fr_120px_70px_80px] gap-2 border-b border-b-border/40 bg-white/4 px-2.5 py-1.25 text-10 font-bold uppercase tracking-0.04 text-muted [&>span:not(:first-child)]:text-right"><span>{label}</span><span>Runs</span><span>Vitus</span><span>Vitus/min</span></div>
        {rows.map(r => (
          <div className="grid grid-cols-[1fr_120px_70px_80px] gap-2 border-b border-b-border/20 px-2.5 py-1.25 text-12 last:border-b-0 hover:bg-white/4" key={r.key}>
            <span className="min-w-0 overflow-hidden text-ellipsis whitespace-nowrap text-foreground">{name(r.key)}</span>
            <span className="text-right text-muted tabular-nums">{r.runs}{r.completed < r.runs && <span className="text-11 text-muted"> ({r.runs - r.completed} incomplete)</span>}</span>
            <span className="text-right text-muted tabular-nums">{Math.round(r.vitus)}</span>
            <span className="text-right text-muted tabular-nums">{fmtRate(r.perMinute)}</span>
          </div>
        ))}
      </div>
    </div>
  );
}


export default function ArbitrationHistory({ clockFormat, systemLocale }: { clockFormat: ClockFormat; systemLocale: string }) {
  const [runs, setRuns] = useState<RunRecord[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [filters, setFilters] = useState<Filters>({ days: 30, missionType: "all" });
  const [confirmUid, setConfirmUid] = useState<string | null>(null);

  // The date window is measured from now, so the tab has to notice time
  // passing or a run keeps sitting inside "7d" for as long as it stays open.
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const tick = setInterval(() => setNow(Date.now()), 60_000);
    return () => clearInterval(tick);
  }, []);

  const load = useCallback(() =>
    invoke<RunRecord[]>("get_arbitration_runs")
      .then(r => { setRuns(r); setError(null); })
      .catch(e => setError(String(e))), []);

  useEffect(() => {
    void load();
    const unlisten = listen(TAURI_EVENTS.ARBITRATION_RUNS_CHANGED, () => void load());
    return () => { void unlisten.then(fn => fn()); };
  }, [load]);

  const shown = useMemo(() => (runs ? filterRuns(runs, filters, now) : []), [runs, filters, now]);
  const summary = useMemo(() => summarize(shown), [shown]);

  const setFilter = (patch: Partial<Filters>) => {
    setConfirmUid(null);
    setFilters(f => ({ ...f, ...patch }));
  };

  const remove = async (uid: string) => {
    setConfirmUid(null);
    try {
      await invoke("delete_arbitration_run", { uid });
      await load();
    } catch (e) {
      setError(String(e));
    }
  };

  if (error && !runs) return <div className="flex items-center gap-2 px-4 py-2 text-12 text-danger">{error}</div>;
  if (!runs) return <div className="px-4 py-3 text-center text-12 text-muted">Loading run history…</div>;
  if (runs.length === 0) {
    return (
      <div className="flex flex-1 flex-col items-center justify-center gap-2.5 p-10 text-center">
        {error && <div className="flex items-center gap-2 px-4 py-2 text-12 text-danger">{error}</div>}
        <div className="text-48">🏛️</div>
        <div className="text-16 font-semibold text-foreground">No arbitration runs recorded yet</div>
        <div className="text-13 leading-1.6 text-muted">
          Runs are recorded from the game's log while FrameForge is open,
          <br />
          and runs from the current log are picked up on startup.
        </div>
      </div>
    );
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto px-3.5 py-3">
      {error && <div className="flex items-center gap-2 px-4 py-2 text-12 text-danger">{error}</div>}

      <div className="flex shrink-0 flex-wrap items-center gap-3">
        <div className="flex [&>button.active+button]:border-l-border/50">
          {RANGES.map(r => (
            <button key={r.label} className={`cursor-pointer border border-border/50 border-r-0 bg-transparent px-1.5 py-0.5 text-10 leading-1.4 text-muted transition-[background,color,border-color] duration-100 first:rounded-l-3 last:mr-1 last:rounded-r-3 last:border-r hover:bg-white/6 hover:text-foreground ${filters.days === r.value ? "active border-accent! border-r! bg-accent/15 text-accent" : ""}`}
              onClick={() => setFilter({ days: r.value })}>{r.label}</button>
          ))}
        </div>
        <div className="flex [&>button.active+button]:border-l-border/50">
          {(["all", ...MISSION_TYPES] as (MissionType | "all")[]).map(t => (
            <button key={t} className={`cursor-pointer border border-border/50 border-r-0 bg-transparent px-1.5 py-0.5 text-10 leading-1.4 text-muted transition-[background,color,border-color] duration-100 first:rounded-l-3 last:mr-1 last:rounded-r-3 last:border-r hover:bg-white/6 hover:text-foreground ${filters.missionType === t ? "active border-accent! border-r! bg-accent/15 text-accent" : ""}`}
              onClick={() => setFilter({ missionType: t })}>{cap(t)}</button>
          ))}
        </div>
        <span className="ml-auto text-11 text-muted">{shown.length} of {runs.length} runs</span>
      </div>

      <div className="flex cursor-default flex-col gap-2 rounded-8 border border-border bg-surface px-3.5 py-3 transition-[border-color,opacity] duration-100">
        <div className="text-10 uppercase tracking-0.04 text-muted">Vitus per minute, run by run</div>
        <Sparkline values={summary.rate} empty="No completed runs in this range" />
        <div className="flex gap-4">
          <div className="flex flex-col gap-px"><span className="text-10 uppercase tracking-0.04 text-muted">Runs</span><span className="text-14 font-bold tabular-nums text-foreground">{summary.runs}</span></div>
          <div className="flex flex-col gap-px"><span className="text-10 uppercase tracking-0.04 text-muted">Incomplete</span><span className="text-14 font-bold tabular-nums text-foreground">{summary.incomplete}</span></div>
          <div className="flex flex-col gap-px"><span className="text-10 uppercase tracking-0.04 text-muted">Playtime (all runs)</span><span className="text-14 font-bold tabular-nums text-foreground">{fmtMs(summary.playtimeSec * 1000)}</span></div>
          <div className="flex flex-col gap-px"><span className="text-10 uppercase tracking-0.04 text-muted">Kills</span><span className="text-14 font-bold tabular-nums text-foreground">{summary.kills.toLocaleString()}</span></div>
          <div className="flex flex-col gap-px"><span className="text-10 uppercase tracking-0.04 text-muted">Vitus (est.)</span><span className="text-14 font-bold tabular-nums text-foreground">{Math.round(summary.vitus)}</span></div>
          <div className="flex flex-col gap-px"><span className="text-10 uppercase tracking-0.04 text-muted">Vitus/min</span><span className="text-14 font-bold tabular-nums text-foreground">{fmtRate(summary.perMinute)}</span></div>
        </div>
      </div>

      <BreakdownTable label="Node" rows={summary.byNode} name={k => k} />
      <BreakdownTable label="Mission type" rows={summary.byMissionType} name={cap} />

      <div className="text-10 uppercase tracking-0.04 text-muted">Runs</div>
      <div className="flex flex-col gap-0.5">
        {shown.map(run => (
          <div className={`flex items-center gap-2.5 rounded-5 border border-border/30 border-l-3 bg-white/3 px-2.5 py-1.5 text-12 hover:bg-white/6 ${completed(run) ? "border-l-success" : "border-l-danger"}`} key={run.uid}>
            <span className="min-w-27.5 shrink-0 text-11 tabular-nums text-muted">{fmtDate(run.started_at, clockFormat, systemLocale)}</span>
            <span className="flex min-w-0 flex-1 flex-col gap-px">
              <span className={`min-w-0 flex-1 overflow-hidden text-ellipsis whitespace-nowrap text-12 ${completed(run) ? "text-foreground" : "text-muted"}`}><TierBadge tier={run.tier} />{run.node}</span>
              <span className="text-11 text-muted">
                {cap(run.mission_type)}
                {!completed(run) && <span className="text-danger"> · {endLabel[run.end_reason as keyof typeof endLabel]}</span>}
              </span>
            </span>
            <span className="min-w-16 shrink-0 text-right text-11 tabular-nums text-muted">{fmtMs(run.duration_sec * 1000)}</span>
            <span className="min-w-16 shrink-0 text-right text-11 tabular-nums text-muted">{run.waves > 0 ? `${run.waves} waves` : `${run.rotations} rotations`}</span>
            <span className="min-w-16 shrink-0 text-right text-11 tabular-nums text-muted">{run.kills} kills</span>
            <span className="min-w-16 shrink-0 text-right text-11 tabular-nums text-muted">{fmtRate(run.vitus_per_minute)}/min</span>
            {confirmUid === run.uid ? (
              <span className="flex shrink-0 items-center gap-1.5">
                <span className="whitespace-nowrap text-11 text-danger">Delete run?</span>
                <button className="cursor-pointer rounded-3 border border-danger bg-danger/20 px-2 py-0.5 text-11 font-semibold text-danger transition-colors duration-100 hover:bg-danger/35" onClick={() => void remove(run.uid)}>Delete</button>
                <button className="cursor-pointer rounded-3 border border-border/60 bg-white/6 px-2 py-0.5 text-11 text-muted transition-colors duration-100 hover:bg-white/12 hover:text-foreground" onClick={() => setConfirmUid(null)}>Cancel</button>
              </span>
            ) : (
              <button className="cursor-pointer border-0 bg-transparent px-0.5 py-0 text-16 leading-none text-muted transition-colors duration-100 hover:text-danger" title="Delete run" onClick={() => setConfirmUid(run.uid)}>×</button>
            )}
          </div>
        ))}
      </div>
    </div>
  );
}
