import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { Schedule } from "./arbitrationAlerts";

// One poll per window, shared by the browser and the alert loop. The backend
// command re-reads and re-parses the whole multi-year feed on every call, so a
// timer per consumer is not free the way a cache hit sounds like it should be.
//
// Hourly, because that is how often the rotation changes; the entries already
// reach ahead, so a schedule fetched fifty minutes ago still holds every hour
// anyone is about to be alerted about.
const POLL_MS = 60 * 60 * 1000;

// Until one fetch has landed there is nothing to alert from at all, and an
// hour's wait for the next attempt is long enough to walk past an occurrence
// entirely. Backs off so a feed that is down all afternoon is not hammered.
const RETRY_MS = [15_000, 60_000, 300_000];

export const DEFAULT_SCHEDULE_DAYS = 7;
export const SCHEDULE_DAY_OPTIONS = [3, 7, 14, 30, 60] as const;
const MIN_SCHEDULE_DAYS = 1;
const MAX_SCHEDULE_DAYS = 60;

/// A stored selection is any number, not just a preset; it must land inside
/// the range the backend honors rather than being rejected outright.
export function clampScheduleDays(days: number) {
  if (!Number.isFinite(days)) return DEFAULT_SCHEDULE_DAYS;
  return Math.min(MAX_SCHEDULE_DAYS, Math.max(MIN_SCHEDULE_DAYS, Math.round(days)));
}

type Snapshot = { schedule: Schedule | null; error: string };

let current: Snapshot = { schedule: null, error: "" };
let fetchedForDays = DEFAULT_SCHEDULE_DAYS;
let inFlight: Promise<void> | null = null;
let inFlightDays = DEFAULT_SCHEDULE_DAYS;
let timer: ReturnType<typeof setInterval> | null = null;
let retry: ReturnType<typeof setTimeout> | null = null;
let failures = 0;
const subscribers = new Set<(s: Snapshot) => void>();

function publish(next: Snapshot) {
  current = next;
  for (const notify of subscribers) notify(next);
}

function clearRetry() {
  if (retry) { clearTimeout(retry); retry = null; }
}

function fetchOnce(days: number): Promise<void> {
  days = clampScheduleDays(days);
  if (inFlight) {
    // A request for a horizon the running fetch is not building must not be
    // lost to it; the refresh follows once the running fetch settles.
    if (days === inFlightDays) return inFlight;
    return inFlight.then(() => fetchOnce(days));
  }
  inFlightDays = days;
  inFlight = invoke<Schedule>("fetch_arbitration_schedule", { horizonDays: days })
    .then(s => { failures = 0; clearRetry(); fetchedForDays = days; publish({ schedule: s, error: "" }); })
    // The last good schedule is published alongside the error rather than
    // cleared: its entries stay valid for days, so the alert loop keeps
    // working through a failed refresh and the browser can show what it has.
    .catch(e => { publish({ ...current, error: String(e) }); scheduleRetry(); })
    .finally(() => { inFlight = null; });
  return inFlight;
}

// Only while nothing usable is cached. Once a schedule is in hand the hourly
// poll is soon enough, because those entries already cover every hour anyone
// is about to be alerted about.
function scheduleRetry() {
  if (current.schedule || retry || subscribers.size === 0) return;
  retry = setTimeout(() => {
    retry = null;
    void fetchOnce(fetchedForDays);
  }, RETRY_MS[Math.min(failures++, RETRY_MS.length - 1)]);
}

// `enabled` is false for a consumer that wants the schedule only if someone
// else is already paying for it: the alert loop must not start an hourly fetch
// for a user who has never starred a node. `horizonDays` is how far ahead this
// consumer wants the entries to reach; the browser passes its choice, and the
// alert loop passes nothing — it has no window preference, so it must not pull
// the shared snapshot off the window the user picked.
export function useArbitrationSchedule(enabled: boolean, horizonDays?: number): Snapshot & { refresh: () => void } {
  const [snapshot, setSnapshot] = useState(current);
  const days = clampScheduleDays(horizonDays ?? DEFAULT_SCHEDULE_DAYS);
  const passive = horizonDays === undefined;

  useEffect(() => {
    if (!enabled) return;
    subscribers.add(setSnapshot);
    if (subscribers.size === 1) {
      fetchOnce(days);
      timer = setInterval(() => fetchOnce(fetchedForDays), POLL_MS);
    } else {
      setSnapshot(current);
      // Entries arrive windowed whole from the backend, so a request for a
      // horizon other than the snapshot's needs its own fetch in either
      // direction: narrower is not a client-side slice.
      if (!passive && (current.schedule === null || fetchedForDays !== days)) fetchOnce(days);
    }
    return () => {
      subscribers.delete(setSnapshot);
      if (subscribers.size > 0) return;
      if (timer) { clearInterval(timer); timer = null; }
      clearRetry();
      failures = 0;
    };
  }, [enabled, days]);

  return { ...snapshot, refresh: () => fetchOnce(days) };
}
