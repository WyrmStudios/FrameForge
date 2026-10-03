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
      <div className="flex shrink-0 gap-[2px] border-b border-border px-[12px] py-[6px]">
        <button className={`cursor-pointer rounded-[4px] border border-[rgba(48,54,61,.6)] bg-transparent px-[14px] py-[3px] text-[12px] text-muted transition-[background,color,border-color] duration-100 hover:bg-[rgba(255,255,255,.06)] hover:text-foreground ${tab === "schedule" ? "border-accent bg-[rgba(56,139,253,.15)] text-accent" : ""}`} onClick={() => setTab("schedule")}>Schedule</button>
        <button className={`cursor-pointer rounded-[4px] border border-[rgba(48,54,61,.6)] bg-transparent px-[14px] py-[3px] text-[12px] text-muted transition-[background,color,border-color] duration-100 hover:bg-[rgba(255,255,255,.06)] hover:text-foreground ${tab === "history" ? "border-accent bg-[rgba(56,139,253,.15)] text-accent" : ""}`} onClick={() => setTab("history")}>Run history</button>
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
      ? <div className="flex min-h-0 flex-1 flex-col overflow-y-auto pb-[8px]"><div className="flex items-center gap-[8px] px-[16px] py-[8px] text-[12px] text-danger">{error} <button className="cursor-pointer rounded-[3px] border border-danger bg-transparent px-[8px] py-[1px] text-[11px] text-danger" onClick={fetchSchedule}>Retry</button></div></div>
      : <div className="flex min-h-0 flex-1 flex-col overflow-y-auto pb-[8px]"><div className="px-[16px] py-[12px] text-center text-[12px] text-muted">Loading arbitration schedule…</div></div>;
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
      className={`shrink-0 cursor-pointer border-0 bg-transparent px-[2px] py-0 text-[14px] leading-none text-muted transition-colors duration-100 hover:text-[#f0c040] ${isFav(e) ? "text-[#f0c040]" : ""}`}
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
    <div className="flex min-h-0 flex-1 flex-col overflow-y-auto pb-[8px]">
      {stale && (
        <div className="flex items-center gap-[8px] border-b border-b-[rgba(240,192,64,.25)] bg-[rgba(240,192,64,.08)] px-[16px] py-[6px] text-[11px] text-[#f0c040]" title={schedule.warning ?? error}>
          Showing the last schedule that loaded; refreshing failed.
          <button className="cursor-pointer rounded-[3px] border border-[#f0c040] bg-transparent px-[8px] py-[1px] text-[11px] text-[#f0c040]" onClick={fetchSchedule}>Retry</button>
        </div>
      )}

      <div className="flex items-center gap-[10px] border-b border-border px-[16px] py-[6px] text-[11px] text-muted">
        <label className="flex items-center gap-[6px]">
          Alert me
          <input
            type="number"
            min={MIN_LEAD_MINS}
            max={MAX_LEAD_MINS}
            value={leadDraft}
            onChange={e => setLeadDraft(e.target.value)}
            onBlur={commitLead}
            onKeyDown={e => { if (e.key === "Enter") e.currentTarget.blur(); }}
            className="w-[52px] rounded-[3px] border border-border bg-surface px-[4px] py-[2px] text-[11px] text-foreground"
          />
          minutes before an alerting node starts
        </label>
        <TierSelect label="Alert for tiers" selected={alertTiers} onChange={onAlertTiersChange} />
        {!alertsOn && <span>Star a node or pick a tier to be alerted.</span>}
      </div>
      {permissionDenied && (
        <div className="px-[16px] py-[4px] text-[11px] text-[#f0c040]">FrameForge cannot send notifications. Check its permission in your system settings.</div>
      )}

      <div className="sticky top-0 z-[1] flex items-center gap-[8px] border-b border-b-[rgba(48,54,61,.4)] bg-surface px-[16px] pt-[6px] pb-[3px] text-[10px] font-bold uppercase tracking-[.04em] text-muted">Now</div>
      {current ? (
        <div className={`flex items-center gap-[10px] border-b border-b-[rgba(48,54,61,.35)] py-[12px] pr-[16px] pl-[8px] ${isFav(current) ? "bg-[rgba(240,192,64,.08)] hover:bg-[rgba(240,192,64,.14)]" : byTier(current) ? "bg-[rgba(56,139,253,.07)] hover:bg-[rgba(56,139,253,.12)]" : ""}`} title={alertTitle(current)}>
          {star(current)}
          <div className="min-w-0 flex-1">
            <div className={`text-[16px] font-semibold ${isFav(current) ? "text-[#f0c040]" : "text-foreground"}`}>{name(current)}</div>
            <div className="shrink-0 overflow-hidden text-ellipsis whitespace-nowrap text-[11px] text-muted">{detail(current)}</div>
          </div>
          <div className="text-right">
            <div className="min-w-[80px] shrink-0 text-right text-[18px] font-bold tabular-nums text-foreground">{fmtMs(current.end * 1000 - now)}</div>
            <div className="shrink-0 overflow-hidden text-ellipsis whitespace-nowrap text-[10px] text-muted">remaining</div>
          </div>
        </div>
      ) : (
        <div className="px-[16px] py-[12px] text-center text-[12px] text-muted">No arbitration in the schedule for this hour.</div>
      )}

      <div className="flex shrink-0 flex-wrap items-center gap-[12px] px-[16px]">
        <TierSelect label="Show tiers" selected={tierFilter} onChange={onTierFilterChange} />
        <span className="w-px self-stretch bg-border opacity-60" aria-hidden="true" />
        <span className="flex items-center gap-[6px]">
          <span className="text-[11px] text-muted">Show</span>
          <span className="relative flex items-center">
            <select
              className="cursor-pointer appearance-none rounded-[3px] border border-border bg-surface py-[3px] pr-[18px] pl-[6px] text-[11px] text-foreground hover:border-accent focus-visible:outline focus-visible:outline-1 focus-visible:outline-accent focus-visible:outline-offset-0"
              value={clampScheduleDays(scheduleDays)}
              onChange={e => onScheduleDaysChange(Number(e.target.value))}
            >
              {SCHEDULE_DAY_OPTIONS.map(d => (
                <option key={d} value={d}>{d} days</option>
              ))}
            </select>
            <span className="pointer-events-none absolute right-[6px] text-[11px] text-muted" aria-hidden="true">▾</span>
          </span>
        </span>
        <span className="ml-auto text-[11px] text-muted">{shown.length} of {upcoming.length} hours</span>
      </div>

      {upcoming.length === 0 && <div className="px-[16px] py-[12px] text-center text-[12px] text-muted">No upcoming arbitrations in the feed.</div>}
      {upcoming.length > 0 && shown.length === 0 &&
        <div className="px-[16px] py-[12px] text-center text-[12px] text-muted">No upcoming arbitrations in the tiers you are showing.</div>}
      {shown.map(e => {
        const day = dayLabel(e.start);
        const header = day !== lastDay ? <div className="sticky top-0 z-[1] flex items-center gap-[8px] border-b border-b-[rgba(48,54,61,.4)] bg-surface px-[16px] pt-[6px] pb-[3px] text-[10px] font-bold uppercase tracking-[.04em] text-muted">{day}</div> : null;
        lastDay = day;
        return (
          <div key={e.start}>
            {header}
            <div className={`flex min-h-[30px] items-center gap-[6px] border-b border-b-[rgba(48,54,61,.25)] py-[5px] pr-[12px] pl-[8px] transition-colors duration-100 ${isFav(e) ? "bg-[rgba(240,192,64,.08)] hover:bg-[rgba(240,192,64,.14)]" : byTier(e) ? "bg-[rgba(56,139,253,.07)] hover:bg-[rgba(56,139,253,.12)]" : "hover:bg-[rgba(255,255,255,.03)]"}`} title={alertTitle(e)}>
              {star(e)}
              <span className="min-w-[48px] shrink-0 text-[12px] tabular-nums text-muted">{formatUnixTime(e.start, clockFormat, systemLocale)}</span>
              <span className={`min-w-0 flex-1 overflow-hidden text-ellipsis whitespace-nowrap text-[12px] ${isFav(e) ? "text-[#f0c040]" : "text-foreground"}`}>{name(e)}</span>
              <span className="w-[210px] shrink-0 overflow-hidden text-ellipsis whitespace-nowrap text-[11px] text-muted">{detail(e)}</span>
              <span className="w-[96px] shrink-0 overflow-hidden text-ellipsis whitespace-nowrap text-right text-[10px] text-muted">in {fmtMs(e.start * 1000 - now)}</span>
            </div>
          </div>
        );
      })}
    </div>
  );
}
