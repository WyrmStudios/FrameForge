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
    <div className="flex flex-col gap-[4px]">
      <div className="text-[10px] uppercase tracking-[.04em] text-muted">{label}</div>
      <div className="overflow-hidden rounded-[6px] border border-[rgba(48,54,61,.4)] bg-[rgba(255,255,255,.03)]">
        <div className="grid grid-cols-[1fr_120px_70px_80px] gap-[8px] border-b border-b-[rgba(48,54,61,.4)] bg-[rgba(255,255,255,.04)] px-[10px] py-[5px] text-[10px] font-bold uppercase tracking-[.04em] text-muted [&>span:not(:first-child)]:text-right"><span>{label}</span><span>Runs</span><span>Vitus</span><span>Vitus/min</span></div>
        {rows.map(r => (
          <div className="grid grid-cols-[1fr_120px_70px_80px] gap-[8px] border-b border-b-[rgba(48,54,61,.2)] px-[10px] py-[5px] text-[12px] last:border-b-0 hover:bg-[rgba(255,255,255,.04)]" key={r.key}>
            <span className="min-w-0 overflow-hidden text-ellipsis whitespace-nowrap text-foreground">{name(r.key)}</span>
            <span className="text-right text-muted tabular-nums">{r.runs}{r.completed < r.runs && <span className="text-[11px] text-muted"> ({r.runs - r.completed} incomplete)</span>}</span>
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

  if (error && !runs) return <div className="flex items-center gap-[8px] px-[16px] py-[8px] text-[12px] text-danger">{error}</div>;
  if (!runs) return <div className="px-[16px] py-[12px] text-center text-[12px] text-muted">Loading run history…</div>;
  if (runs.length === 0) {
    return (
      <div className="flex flex-1 flex-col items-center justify-center gap-[10px] p-[40px] text-center">
        {error && <div className="flex items-center gap-[8px] px-[16px] py-[8px] text-[12px] text-danger">{error}</div>}
        <div className="text-[48px]">🏛️</div>
        <div className="text-[16px] font-semibold text-foreground">No arbitration runs recorded yet</div>
        <div className="text-[13px] leading-[1.6] text-muted">
          Runs are recorded from the game's log while FrameForge is open,
          <br />
          and runs from the current log are picked up on startup.
        </div>
      </div>
    );
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-[12px] overflow-y-auto px-[14px] py-[12px]">
      {error && <div className="flex items-center gap-[8px] px-[16px] py-[8px] text-[12px] text-danger">{error}</div>}

      <div className="flex shrink-0 flex-wrap items-center gap-[12px]">
        <div className="flex [&>button.active+button]:border-l-[rgba(48,54,61,.5)]">
          {RANGES.map(r => (
            <button key={r.label} className={`cursor-pointer border border-[rgba(48,54,61,.5)] border-r-0 bg-transparent px-[6px] py-[2px] text-[10px] leading-[1.4] text-muted transition-[background,color,border-color] duration-100 first:rounded-l-[3px] last:mr-[4px] last:rounded-r-[3px] last:border-r hover:bg-[rgba(255,255,255,.06)] hover:text-foreground ${filters.days === r.value ? "active border-accent! border-r! bg-[rgba(56,139,253,.15)] text-accent" : ""}`}
              onClick={() => setFilter({ days: r.value })}>{r.label}</button>
          ))}
        </div>
        <div className="flex [&>button.active+button]:border-l-[rgba(48,54,61,.5)]">
          {(["all", ...MISSION_TYPES] as (MissionType | "all")[]).map(t => (
            <button key={t} className={`cursor-pointer border border-[rgba(48,54,61,.5)] border-r-0 bg-transparent px-[6px] py-[2px] text-[10px] leading-[1.4] text-muted transition-[background,color,border-color] duration-100 first:rounded-l-[3px] last:mr-[4px] last:rounded-r-[3px] last:border-r hover:bg-[rgba(255,255,255,.06)] hover:text-foreground ${filters.missionType === t ? "active border-accent! border-r! bg-[rgba(56,139,253,.15)] text-accent" : ""}`}
              onClick={() => setFilter({ missionType: t })}>{cap(t)}</button>
          ))}
        </div>
        <span className="ml-auto text-[11px] text-muted">{shown.length} of {runs.length} runs</span>
      </div>

      <div className="flex cursor-default flex-col gap-[8px] rounded-[8px] border border-border bg-surface px-[14px] py-[12px] transition-[border-color,opacity] duration-100">
        <div className="text-[10px] uppercase tracking-[.04em] text-muted">Vitus per minute, run by run</div>
        <Sparkline values={summary.rate} empty="No completed runs in this range" />
        <div className="flex gap-[16px]">
          <div className="flex flex-col gap-[1px]"><span className="text-[10px] uppercase tracking-[.04em] text-muted">Runs</span><span className="text-[14px] font-bold tabular-nums text-foreground">{summary.runs}</span></div>
          <div className="flex flex-col gap-[1px]"><span className="text-[10px] uppercase tracking-[.04em] text-muted">Incomplete</span><span className="text-[14px] font-bold tabular-nums text-foreground">{summary.incomplete}</span></div>
          <div className="flex flex-col gap-[1px]"><span className="text-[10px] uppercase tracking-[.04em] text-muted">Playtime (all runs)</span><span className="text-[14px] font-bold tabular-nums text-foreground">{fmtMs(summary.playtimeSec * 1000)}</span></div>
          <div className="flex flex-col gap-[1px]"><span className="text-[10px] uppercase tracking-[.04em] text-muted">Kills</span><span className="text-[14px] font-bold tabular-nums text-foreground">{summary.kills.toLocaleString()}</span></div>
          <div className="flex flex-col gap-[1px]"><span className="text-[10px] uppercase tracking-[.04em] text-muted">Vitus (est.)</span><span className="text-[14px] font-bold tabular-nums text-foreground">{Math.round(summary.vitus)}</span></div>
          <div className="flex flex-col gap-[1px]"><span className="text-[10px] uppercase tracking-[.04em] text-muted">Vitus/min</span><span className="text-[14px] font-bold tabular-nums text-foreground">{fmtRate(summary.perMinute)}</span></div>
        </div>
      </div>

      <BreakdownTable label="Node" rows={summary.byNode} name={k => k} />
      <BreakdownTable label="Mission type" rows={summary.byMissionType} name={cap} />

      <div className="text-[10px] uppercase tracking-[.04em] text-muted">Runs</div>
      <div className="flex flex-col gap-[2px]">
        {shown.map(run => (
          <div className={`flex items-center gap-[10px] rounded-[5px] border border-[rgba(48,54,61,.3)] border-l-[3px] bg-[rgba(255,255,255,.03)] px-[10px] py-[6px] text-[12px] hover:bg-[rgba(255,255,255,.06)] ${completed(run) ? "border-l-success" : "border-l-danger"}`} key={run.uid}>
            <span className="min-w-[110px] shrink-0 text-[11px] tabular-nums text-muted">{fmtDate(run.started_at, clockFormat, systemLocale)}</span>
            <span className="flex min-w-0 flex-1 flex-col gap-[1px]">
              <span className={`min-w-0 flex-1 overflow-hidden text-ellipsis whitespace-nowrap text-[12px] ${completed(run) ? "text-foreground" : "text-muted"}`}><TierBadge tier={run.tier} />{run.node}</span>
              <span className="text-[11px] text-muted">
                {cap(run.mission_type)}
                {!completed(run) && <span className="text-danger"> · {endLabel[run.end_reason as keyof typeof endLabel]}</span>}
              </span>
            </span>
            <span className="min-w-[64px] shrink-0 text-right text-[11px] tabular-nums text-muted">{fmtMs(run.duration_sec * 1000)}</span>
            <span className="min-w-[64px] shrink-0 text-right text-[11px] tabular-nums text-muted">{run.waves > 0 ? `${run.waves} waves` : `${run.rotations} rotations`}</span>
            <span className="min-w-[64px] shrink-0 text-right text-[11px] tabular-nums text-muted">{run.kills} kills</span>
            <span className="min-w-[64px] shrink-0 text-right text-[11px] tabular-nums text-muted">{fmtRate(run.vitus_per_minute)}/min</span>
            {confirmUid === run.uid ? (
              <span className="flex shrink-0 items-center gap-[6px]">
                <span className="whitespace-nowrap text-[11px] text-danger">Delete run?</span>
                <button className="cursor-pointer rounded-[3px] border border-danger bg-[rgba(248,81,73,.2)] px-[8px] py-[2px] text-[11px] font-semibold text-danger transition-colors duration-100 hover:bg-[rgba(248,81,73,.35)]" onClick={() => void remove(run.uid)}>Delete</button>
                <button className="cursor-pointer rounded-[3px] border border-[rgba(48,54,61,.6)] bg-[rgba(255,255,255,.06)] px-[8px] py-[2px] text-[11px] text-muted transition-colors duration-100 hover:bg-[rgba(255,255,255,.12)] hover:text-foreground" onClick={() => setConfirmUid(null)}>Cancel</button>
              </span>
            ) : (
              <button className="cursor-pointer border-0 bg-transparent px-[2px] py-0 text-[16px] leading-none text-muted transition-colors duration-100 hover:text-danger" title="Delete run" onClick={() => setConfirmUid(run.uid)}>×</button>
            )}
          </div>
        ))}
      </div>
    </div>
  );
}
