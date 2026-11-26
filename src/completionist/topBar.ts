import { formatAge } from "../lib/formatters.ts";
import { dayAndClock } from "../lib/formatters.ts";
import type { ClockFormat } from "../types/settings.ts";
import type { MasteryCounts, MasteryProvenance, Provenance, ProvenanceState } from "../types/mastery";

export const SOURCE_KINDS: { key: keyof MasteryProvenance; label: string }[] = [
  { key: "equipment",  label: "Equipment" },
  { key: "intrinsics", label: "Intrinsics" },
  { key: "nodes",      label: "Nodes" },
  { key: "standing",   label: "Standing" },
];

const WORST: ProvenanceState[] = ["unknown", "unconfirmed", "confirmed"];

function kindLine(label: string, { state, observed_at }: Provenance, clockFormat: ClockFormat): string {
  if (state === "confirmed" && observed_at != null) {
    return `${label}: observed ${dayAndClock(observed_at * 1000, clockFormat)}`;
  }
  if (state === "unconfirmed") return `${label}: unconfirmed, carried over from a cache with no observation time`;
  return `${label}: no observation yet`;
}

/** The scan age is the oldest observation across kinds, since a stale kind is what the player needs to know about. */
export function pillSummary(provenance: MasteryProvenance, now: number, clockFormat: ClockFormat): { state: ProvenanceState; text: string; title: string } {
  const kinds = SOURCE_KINDS.map(k => ({ label: k.label, provenance: provenance[k.key] }));
  const state = WORST.find(s => kinds.some(k => k.provenance.state === s)) ?? "confirmed";
  const oldest = Math.min(...kinds.map(k => k.provenance.observed_at ?? Infinity));
  const text = state === "unknown" ? "No scan yet" : state === "unconfirmed" ? "Unconfirmed" : `Scanned ${formatAge(oldest, now)}`;
  return { state, text, title: kinds.map(k => kindLine(k.label, k.provenance, clockFormat)).join("\n") };
}

/** Unobtainable is already outside `total`, so it never appears on the line. */
export function progressText(counts: MasteryCounts, label: string): { text: string; title: string } {
  const unknown = counts.unknown > 0 ? ` · ${counts.unknown} unknown` : "";
  const title = `${label}: ${counts.mastered} mastered, ${counts.partial} partial, ${counts.missing} missing` + (counts.unknown > 0 ? `, ${counts.unknown} unknown` : "");
  return { text: `${label} ${counts.mastered} / ${counts.total} mastered${unknown}`, title };
}
