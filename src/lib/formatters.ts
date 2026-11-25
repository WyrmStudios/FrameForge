import type { ClockFormat } from "../types/settings";

export function formatCount(value: number): string {
  return value.toLocaleString("en-US");
}

export function formatAge(ts: number | null, now = Math.floor(Date.now() / 1000)): string {
  if (ts == null) return "never";
  const secs = now - ts;
  if (secs < 60) return `${secs}s ago`;
  if (secs < 3600) return `${Math.floor(secs / 60)}m ago`;
  if (secs < 86400) return `${Math.floor(secs / 3600)}h ago`;
  return `${Math.floor(secs / 86400)}d ago`;
}

export function formatBytes(bytes: number): string {
  if (bytes >= 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  if (bytes >= 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${bytes} B`;
}

export function formatUnixTime(timestamp: number, format: ClockFormat = "auto", locale = "en-US"): string {
  const options: Intl.DateTimeFormatOptions = { hour: "2-digit", minute: "2-digit" };
  if (format === "12h") options.hour12 = true;
  else if (format === "24h") options.hour12 = false;
  return new Date(timestamp * 1000).toLocaleTimeString(locale, options);
}

/** A time outside today carries its day, since a clock alone would read as today. */
export function dayAndClock(ms: number, format: ClockFormat = "auto", locale = navigator.language): string {
  return `${new Date(ms).toLocaleDateString(locale, { month: "short", day: "numeric" })} ${formatUnixTime(Math.floor(ms / 1000), format, locale)}`;
}
