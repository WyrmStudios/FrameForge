import { useEffect, useRef, useState } from "react";
import { fmtMs } from "../TimerHelper";
import { formatUnixTime } from "../lib/formatters";
import type { ClockFormat } from "../types/settings";
import { ensurePermission, permissionGranted } from "../lib/notify";
import { clampLead, MAX_LEAD_MINS, MIN_LEAD_MINS, type ScheduleEntry } from "./arbitrationAlerts";
import { useArbitrationSchedule, clampScheduleDays, SCHEDULE_DAY_OPTIONS } from "./arbitrationSchedule";
import { tierKey, type TierKey } from "./arbitrationTiers";
import TierSelect, { TierBadge } from "./TierSelect";
import ArbitrationHistory from "./ArbitrationHistory";

// The day header always reads English like the rest of the UI; only the time
// takes the locale-driven hour cycle.
const dayLabel = (unix: number) =>
  new Date(unix * 1000).toLocaleDateString("en-US", { weekday: "long", month: "short", day: "numeric" });

// Favorites and lead time are owned by App: the alerts have to keep firing
// while the user is looking at another module, and this component is unmounted
// for all of that time. Permission state too: a denial only this component
// knows about is invisible for as long as the user is elsewhere.
type Props = {
  favorites: string[];
  onToggleFavorite: (nodeId: string) => void;
  leadMins: number;
  onLeadChange: (mins: number) => void;
  permissionDenied: boolean;
  onPermissionChange: (denied: boolean) => void;
  tierFilter: TierKey[];
  onTierFilterChange: (next: TierKey[]) => void;
  alertTiers: TierKey[];
  onAlertTiersChange: (next: TierKey[]) => void;
  scheduleDays: number;
  onScheduleDaysChange: (days: number) => void;
  clockFormat: ClockFormat;
  systemLocale: string;
};

export default function Arbitrations(props: Props) {
  const [tab, setTab] = useState<"schedule" | "history">("schedule");
  return (
    <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
      <div className="flex shrink-0 gap-0.5 border-b border-border px-3 py-1.5">
        <button className={`cursor-pointer rounded-4 border border-border/60 bg-transparent px-3.5 py-0.75 text-12 text-muted transition-[background,color,border-color] duration-100 hover:bg-white/6 hover:text-foreground ${tab === "schedule" ? "border-accent bg-accent/15 text-accent" : ""}`} onClick={() => setTab("schedule")}>Schedule</button>
        <button className={`cursor-pointer rounded-4 border border-border/60 bg-transparent px-3.5 py-0.75 text-12 text-muted transition-[background,color,border-color] duration-100 hover:bg-white/6 hover:text-foreground ${tab === "history" ? "border-accent bg-accent/15 text-accent" : ""}`} onClick={() => setTab("history")}>Run history</button>
      </div>
      {tab === "schedule" ? <Schedule {...props} /> : <ArbitrationHistory clockFormat={props.clockFormat} systemLocale={props.systemLocale} />}
    </div>
  );
}

function Schedule({
  favorites, onToggleFavorite, leadMins, onLeadChange, permissionDenied, onPermissionChange,
  tierFilter, onTierFilterChange, alertTiers, onAlertTiersChange, scheduleDays, onScheduleDaysChange,
  clockFormat, systemLocale,
}: Props) {
  const { schedule, error, refresh: fetchSchedule } = useArbitrationSchedule(true, scheduleDays);
  const [now, setNow] = useState(() => Date.now());
  const [leadDraft, setLeadDraft] = useState(String(leadMins));

  useEffect(() => {
    const tick = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(tick);
  }, []);

  useEffect(() => setLeadDraft(String(leadMins)), [leadMins]);

  // The alert rule arrives from settings after mount, so this waits for it
  // rather than reading the empty state the first render sees.
  const alertsOn = favorites.length > 0 || alertTiers.length > 0;
  const askedRef = useRef(false);
  useEffect(() => {
    if (!alertsOn || askedRef.current) return;
    askedRef.current = true;
    void ensurePermission().then(granted => onPermissionChange(!granted));
  }, [alertsOn]);

  // Permission is granted in system settings, so the app hears about it by
  // getting the window back, not by anything happening inside it. Read-only:
  // a dialog raised by a focus event is one the user did nothing to invite.
  useEffect(() => {
    if (!alertsOn) return;
    const recheck = () => void permissionGranted().then(granted => onPermissionChange(!granted));
    window.addEventListener("focus", recheck);
    return () => window.removeEventListener("focus", recheck);
  }, [alertsOn, onPermissionChange]);

  // Committed on blur rather than per keystroke: clamping while the field is
  // half-typed rewrites what the user is in the middle of entering.
  const commitLead = () => {
    // A cleared field is someone retyping, not a request for the shortest lead
    // the app allows; Number("") would otherwise commit it as zero.
    if (leadDraft.trim() === "") {
      setLeadDraft(String(leadMins));
      return;
    }
    const mins = clampLead(Number(leadDraft));
    onLeadChange(mins);
    setLeadDraft(String(mins));
  };

  const toggleFavorite = async (nodeId: string) => {
    const adding = !favorites.includes(nodeId);
    onToggleFavorite(nodeId);
    if (adding) onPermissionChange(!(await ensurePermission()));
  };

  if (!schedule) {
    return error
      ? <div className="flex min-h-0 flex-1 flex-col overflow-y-auto pb-2"><div className="flex items-center gap-2 px-4 py-2 text-12 text-danger">{error} <button className="cursor-pointer rounded-3 border border-danger bg-transparent px-2 py-px text-11 text-danger" onClick={fetchSchedule}>Retry</button></div></div>
      : <div className="flex min-h-0 flex-1 flex-col overflow-y-auto pb-2"><div className="px-4 py-3 text-center text-12 text-muted">Loading arbitration schedule…</div></div>;
  }

  const nowSec = now / 1000;
  const live = schedule.entries.filter(e => e.end > nowSec);
  // The hour running now is the only one anyone can act on, so it shows
  // regardless of the filter; the filter is about what to plan around.
  const current = live.find(e => e.start <= nowSec);
  const upcoming = live.filter(e => e.start > nowSec);
  const isFav = (e: ScheduleEntry) => favorites.includes(e.node_id);
  // A starred node is exempt from the filter for the same reason the current
  // hour is: hiding it strands it, since the star that would unstar it lives
  // on the row itself.
  const shown = upcoming.filter(e => isFav(e) || tierFilter.includes(tierKey(e.tier)));
  const stale = schedule.source === "stale" || error;
  const byTier = (e: ScheduleEntry) => alertTiers.includes(tierKey(e.tier));
  const alertTitle = (e: ScheduleEntry) => (byTier(e) && !isFav(e) ? "Alerted by tier" : undefined);

  const star = (e: ScheduleEntry) => (
    <button
      className={`shrink-0 cursor-pointer border-0 bg-transparent px-0.5 py-0 text-14 leading-none text-muted transition-colors duration-100 hover:text-ducat ${isFav(e) ? "text-ducat" : ""}`}
      onClick={() => void toggleFavorite(e.node_id)}
      title={isFav(e) ? "Unfavorite node" : "Favorite node"}
    >★</button>
  );
  const detail = (e: ScheduleEntry) =>
    [e.mission_type, e.faction].filter(Boolean).join(" · ");
  const name = (e: ScheduleEntry) => (
    <>
      <TierBadge tier={e.tier} />
      {e.node}{e.region && <span className="font-normal text-muted"> ({e.region})</span>}
    </>
  );

  let lastDay = "";
  return (
    <div className="flex min-h-0 flex-1 flex-col overflow-y-auto pb-2">
      {stale && (
        <div className="flex items-center gap-2 border-b border-b-ducat/25 bg-ducat/8 px-4 py-1.5 text-11 text-ducat" title={schedule.warning ?? error}>
          Showing the last schedule that loaded; refreshing failed.
          <button className="cursor-pointer rounded-3 border border-ducat bg-transparent px-2 py-px text-11 text-ducat" onClick={fetchSchedule}>Retry</button>
        </div>
      )}

      <div className="flex items-center gap-2.5 border-b border-border px-4 py-1.5 text-11 text-muted">
        <label className="flex items-center gap-1.5">
          Alert me
          <input
            type="number"
            min={MIN_LEAD_MINS}
            max={MAX_LEAD_MINS}
            value={leadDraft}
            onChange={e => setLeadDraft(e.target.value)}
            onBlur={commitLead}
            onKeyDown={e => { if (e.key === "Enter") e.currentTarget.blur(); }}
            className="w-13 rounded-3 border border-border bg-surface px-1 py-0.5 text-11 text-foreground"
          />
          minutes before an alerting node starts
        </label>
        <TierSelect label="Alert for tiers" selected={alertTiers} onChange={onAlertTiersChange} />
        {!alertsOn && <span>Star a node or pick a tier to be alerted.</span>}
      </div>
      {permissionDenied && (
        <div className="px-4 py-1 text-11 text-ducat">FrameForge cannot send notifications. Check its permission in your system settings.</div>
      )}

      <div className="sticky top-0 z-1 flex items-center gap-2 border-b border-b-border/40 bg-surface px-4 pt-1.5 pb-0.75 text-10 font-bold uppercase tracking-0.04 text-muted">Now</div>
      {current ? (
        <div className={`flex items-center gap-2.5 border-b border-b-border/35 py-3 pr-4 pl-2 ${isFav(current) ? "bg-ducat/8 hover:bg-ducat/14" : byTier(current) ? "bg-accent/7 hover:bg-accent/12" : ""}`} title={alertTitle(current)}>
          {star(current)}
          <div className="min-w-0 flex-1">
            <div className={`text-16 font-semibold ${isFav(current) ? "text-ducat" : "text-foreground"}`}>{name(current)}</div>
            <div className="shrink-0 overflow-hidden text-ellipsis whitespace-nowrap text-11 text-muted">{detail(current)}</div>
          </div>
          <div className="text-right">
            <div className="min-w-20 shrink-0 text-right text-18 font-bold tabular-nums text-foreground">{fmtMs(current.end * 1000 - now)}</div>
            <div className="shrink-0 overflow-hidden text-ellipsis whitespace-nowrap text-10 text-muted">remaining</div>
          </div>
        </div>
      ) : (
        <div className="px-4 py-3 text-center text-12 text-muted">No arbitration in the schedule for this hour.</div>
      )}

      <div className="flex shrink-0 flex-wrap items-center gap-3 px-4">
        <TierSelect label="Show tiers" selected={tierFilter} onChange={onTierFilterChange} />
        <span className="w-px self-stretch bg-border opacity-60" aria-hidden="true" />
        <span className="flex items-center gap-1.5">
          <span className="text-11 text-muted">Show</span>
          <span className="relative flex items-center">
            <select
              className="cursor-pointer appearance-none rounded-3 border border-border bg-surface py-0.75 pr-4.5 pl-1.5 text-11 text-foreground hover:border-accent focus-visible:outline-1 focus-visible:outline-accent focus-visible:outline-offset-0"
              value={clampScheduleDays(scheduleDays)}
              onChange={e => onScheduleDaysChange(Number(e.target.value))}
            >
              {SCHEDULE_DAY_OPTIONS.map(d => (
                <option key={d} value={d}>{d} days</option>
              ))}
            </select>
            <span className="pointer-events-none absolute right-1.5 text-11 text-muted" aria-hidden="true">▾</span>
          </span>
        </span>
        <span className="ml-auto text-11 text-muted">{shown.length} of {upcoming.length} hours</span>
      </div>

      {upcoming.length === 0 && <div className="px-4 py-3 text-center text-12 text-muted">No upcoming arbitrations in the feed.</div>}
      {upcoming.length > 0 && shown.length === 0 &&
        <div className="px-4 py-3 text-center text-12 text-muted">No upcoming arbitrations in the tiers you are showing.</div>}
      {shown.map(e => {
        const day = dayLabel(e.start);
        const header = day !== lastDay ? <div className="sticky top-0 z-1 flex items-center gap-2 border-b border-b-border/40 bg-surface px-4 pt-1.5 pb-0.75 text-10 font-bold uppercase tracking-0.04 text-muted">{day}</div> : null;
        lastDay = day;
        return (
          <div key={e.start}>
            {header}
            <div className={`flex min-h-7.5 items-center gap-1.5 border-b border-b-border/25 py-1.25 pr-3 pl-2 transition-colors duration-100 ${isFav(e) ? "bg-ducat/8 hover:bg-ducat/14" : byTier(e) ? "bg-accent/7 hover:bg-accent/12" : "hover:bg-white/3"}`} title={alertTitle(e)}>
              {star(e)}
              <span className="min-w-12 shrink-0 text-12 tabular-nums text-muted">{formatUnixTime(e.start, clockFormat, systemLocale)}</span>
              <span className={`min-w-0 flex-1 overflow-hidden text-ellipsis whitespace-nowrap text-12 ${isFav(e) ? "text-ducat" : "text-foreground"}`}>{name(e)}</span>
              <span className="w-52.5 shrink-0 overflow-hidden text-ellipsis whitespace-nowrap text-11 text-muted">{detail(e)}</span>
              <span className="w-24 shrink-0 overflow-hidden text-ellipsis whitespace-nowrap text-right text-10 text-muted">in {fmtMs(e.start * 1000 - now)}</span>
            </div>
          </div>
        );
      })}
    </div>
  );
}
