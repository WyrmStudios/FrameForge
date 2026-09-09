import type { ClockFormat } from "../types/settings";

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
