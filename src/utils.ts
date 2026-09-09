/** Format a number with locale-appropriate separators (e.g. 1234 → "1,234"). */
export function fmt(n: number) { return n.toLocaleString(); }

/** CSS class for a positive or negative delta. */
export function deltaClass(d: number) { return d > 0 ? "delta-pos" : "delta-neg"; }

/** Render a signed delta string (e.g. +5 / -3). */
export function deltaText(d: number) { return d > 0 ? `+${fmt(d)}` : fmt(d); }
