import { formatCount as n } from "../lib/formatters.ts";
import { pick, RESULT_OPTIONS, shownControls, visibleOpportunities, visiblePurchases, type MasteryControls, type ResultView } from "./suggestions.ts";
import type { MasteryPlan, MasteryTotal, Opportunity, PlanEvaluation } from "../types/mastery";

/** Legendary 5 is MR 35 today. The rest is headroom for ranks the game adds later. */
export const TARGET_RANK_MAX = 40;

export function planView(raw: string): ResultView {
  return pick(RESULT_OPTIONS, raw, "suggestions");
}

export function gainOf(o: Opportunity): number | null {
  return o.spend?.mastery ?? o.remaining_mastery;
}

export function candidates(list: Opportunity[], controls: MasteryControls, view: ResultView, search: string): Opportunity[] {
  const scoped = shownControls({ ...controls, result: view });
  return view === "platinum" ? visiblePurchases(list, scoped, search) : visibleOpportunities(list, scoped, search);
}

export function snapshotIntrinsicTargets(plan: MasteryPlan, opportunities: Opportunity[]): Record<string, Record<string, number>> {
  const targets: Record<string, Record<string, number>> = {};
  for (const path of plan.selections) {
    const saved = plan.intrinsic_targets?.[path];
    const spend = opportunities.find(o => o.unique_name === path)?.spend;
    if (saved) targets[path] = saved;
    else if (spend) targets[path] = Object.fromEntries(spend.tracks.map(t => [t.track, t.to]));
  }
  return targets;
}

export function addToPlan(selections: string[], opportunity: Opportunity): string[] {
  return [...new Set([...selections, ...opportunity.craft?.level_first?.map(step => step.unique_name) ?? [], opportunity.unique_name])];
}

// ponytail: takes candidates greedily in view order. The smallest covering set would need a search.
export function prefill(candidates: Opportunity[], gap: number): string[] {
  let selections: string[] = [];
  let covered = 0;
  for (const o of candidates) {
    if (covered >= gap) break;
    // Prefill skips a row with no route because the plan could not say how to get it.
    if (o.stage === "unsourced") continue;
    if (selections.includes(o.unique_name)) continue;
    for (const step of o.craft?.level_first ?? []) {
      if (!selections.includes(step.unique_name)) covered += step.gain;
    }
    selections = addToPlan(selections, o);
    covered += gainOf(o) ?? 0;
  }
  return selections;
}

export function addable(candidates: Opportunity[], plan: MasteryPlan): Opportunity[] {
  const selected = new Set(plan.selections);
  return candidates.filter(o => !selected.has(o.unique_name));
}

export function moved(selections: string[], index: number, by: -1 | 1): string[] {
  const to = index + by;
  if (to < 0 || to >= selections.length) return selections;
  const next = [...selections];
  [next[index], next[to]] = [next[to], next[index]];
  return next;
}

export function totalText(total: MasteryTotal): string {
  return total.exact == null ? `${n(total.lower)} (lower bound)` : n(total.exact);
}

export function rankText(total: MasteryTotal): string {
  return total.exact == null ? `at least MR ${total.rank}` : `MR ${total.rank}`;
}

export function summaryText(e: PlanEvaluation, target: number): string {
  if (e.total == null || e.projected == null || e.gap == null) return "Mastery Rank unknown, so the gap and projection are too.";
  const unknown = e.unknown_gains > 0 ? ` and ${e.unknown_gains} unknown` : "";
  const gap = e.gap === 0 ? `MR ${target} is reached` : `MR ${target} needs ${n(e.gap)} more`;
  return `Total ${totalText(e.total)} · ${gap} · plan adds ${n(e.gains)}${unknown} · projected ${rankText(e.projected)}`;
}

export interface RingFractions {
  /** Earned mastery over the target threshold, unclamped, so a target at or below the current rank reads above 1. */
  earned: number;
  /** Planned gains over the gap, clamped to 1, and 1 when there is no gap. */
  planned: number;
  /** Progress inside the current rank band, 0 when the total is only a lower bound. */
  band: number;
}

export function ringFractions(e: PlanEvaluation): RingFractions | null {
  if (e.total == null || e.gap == null) return null;
  const base = e.total.exact ?? e.total.lower;
  return {
    earned: base / e.target_xp,
    planned: e.gap === 0 ? 1 : Math.min(1, e.gains / e.gap),
    band: e.total.exact == null ? 0 : (e.total.exact - e.total.lower) / (e.total.upper + 1 - e.total.lower),
  };
}

export function ringTitle(e: PlanEvaluation, target: number): string {
  if (e.total == null || e.projected == null) return "Mastery Rank unknown: no scan yet";
  const bound = e.total.exact == null ? "at least " : "";
  const band = e.total.exact == null ? `Progress into MR ${e.total.rank} unknown`
    : `${n(e.total.exact - e.total.lower)} of ${n(e.total.upper + 1 - e.total.lower)} into MR ${e.total.rank}`;
  return [
    `Earned ${bound}${n(e.total.exact ?? e.total.lower)}`,
    `Projected ${bound}${n(e.projected.exact ?? e.projected.lower)}`,
    `MR ${target} needs ${n(e.target_xp)}`,
    band,
  ].join("\n");
}

export function rangeText(e: PlanEvaluation): string {
  if (e.total == null || e.projected == null) return "";
  const total = e.total.exact == null ? `Total between ${n(e.total.lower)} and ${n(e.total.upper)}` : `Total ${n(e.total.exact)} exactly`;
  const projected = e.projected.exact == null
    ? `projected between ${n(e.projected.lower)} and ${n(e.projected.upper)}, MR ${e.projected.rank} to ${e.projected.rank_upper}`
    : `projected ${n(e.projected.exact)}, MR ${e.projected.rank}`;
  return `${total} · ${projected} · target MR needs ${n(e.target_xp)}`;
}
