export type CacheSource = "fresh" | "refreshed" | "stale" | "fallback";

export interface CacheStatus {
  source: CacheSource;
  last_updated: number | null;
  warning: string | null;
}

export type CacheStatuses = Record<string, CacheStatus>;
