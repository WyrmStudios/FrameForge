import type { WfmItem } from "./types/market";

/** Format a number with locale-appropriate separators (e.g. 1234 → "1,234"). */
export function fmt(n: number) { return n.toLocaleString(); }

/** CSS class for a positive or negative delta. */
export function deltaClass(d: number) { return d > 0 ? "delta-pos" : "delta-neg"; }

/** Render a signed delta string (e.g. +5 / -3). */
export function deltaText(d: number) { return d > 0 ? `+${fmt(d)}` : fmt(d); }

export function normalizeForWfm(name: string): string {
  return name.toLowerCase().replace(/[^a-z0-9]+/g, "_").replace(/^_|_$/g, "");
}

/**
 * Normalised item name → warframe.market slug. Exact names win; a Blueprint ↔
 * no-Blueprint alias fills the gaps, since WFM is inconsistent about which
 * component blueprints keep the suffix.
 */
export function wfmSlugLookup(items: WfmItem[]): Map<string, string> {
  const map = new Map<string, string>();
  for (const w of items) map.set(normalizeForWfm(w.item_name), w.url_name);
  for (const w of items) {
    const key = normalizeForWfm(w.item_name);
    const alias = key.endsWith("_blueprint") ? key.slice(0, -"_blueprint".length) : key + "_blueprint";
    if (!map.has(alias)) map.set(alias, w.url_name);
  }
  return map;
}
