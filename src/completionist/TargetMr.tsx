import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import ItemImg from "../ItemImg";
import Filters from "./Filters";
import { OpportunityRow } from "./WhatNext";
import { OpportunityModal } from "./OpportunityModal";
import { TAURI_COMMANDS } from "../constants/tauri";
import type { ClockFormat } from "../types/settings";
import { addable, addToPlan, candidates, moved, planView, prefill, rangeText, ringFractions, ringTitle, snapshotIntrinsicTargets, summaryText, TARGET_RANK_MAX, type RingFractions } from "./plan";
import { activeFilters, RESULT_OPTIONS, remainingText, type MasteryControls, type ResultView } from "./suggestions";
import type { MasteryOverview, MasteryPlan, Opportunity, PlanEntry, PlanEvaluation } from "../types/mastery";
import { MST_TOOLBAR, MST_TOOLBAR_WRAP, MST_VIEW_SWITCH, MST_BODY, MST_EMPTY, MST_GROUP, MST_GROUP_HEADER, MST_COUNT, MST_MR, MST_OPP_NAME, MST_SELECT, filterBtnClass, pillClass, rankClass, oppClass, MST_OPP_LIST, MST_OPP_MAIN, MST_OPP_TITLE, MST_OPP_DETAIL, MST_OPP_BLOCKERS, MST_PLAN_TARGET, MST_PLAN_SUMMARY_TEXT, MST_PLAN_RANGE, MST_PLAN_ROW, MST_PLAN_INDEX, MST_PLAN_CONTROLS, MST_PLAN_ALLOWANCES, planBtnClass, MST_RINGS, MST_RING_TRACK, MST_RING, MST_RING_EARNED, MST_RING_PLANNED, MST_RING_COVERED, MST_RING_BAND } from "./masteryStyles";

// Sixty rows are enough to browse, and the search narrows the rest.
const ADD_LIST_LIMIT = 60;

const RING_SIZE = 44;
const RING_RADII = [19, 14.5, 10];

function Ring({ radius, fraction, className }: { radius: number; fraction: number; className: string }) {
  const length = 2 * Math.PI * radius;
  const centre = RING_SIZE / 2;
  return (
    <>
      <circle className={MST_RING_TRACK} cx={centre} cy={centre} r={radius} />
      <circle className={`${MST_RING} ${className}`} cx={centre} cy={centre} r={radius}
        strokeDasharray={length} strokeDashoffset={length * (1 - Math.min(1, Math.max(0, fraction)))} />
    </>
  );
}

function Rings({ rings, target, title }: { rings: RingFractions | null; target: number; title: string }) {
  const [earned, planned, band] = rings ? [rings.earned, rings.planned, rings.band] : [0, 0, 0];
  return (
    <svg className={MST_RINGS} width={RING_SIZE} height={RING_SIZE} viewBox={`0 0 ${RING_SIZE} ${RING_SIZE}`} role="img" aria-label={title}>
      <title>{title}</title>
      <Ring radius={RING_RADII[0]} fraction={earned} className={MST_RING_EARNED} />
      <Ring radius={RING_RADII[1]} fraction={planned} className={planned >= 1 ? MST_RING_COVERED : MST_RING_PLANNED} />
      <Ring radius={RING_RADII[2]} fraction={band} className={MST_RING_BAND} />
      <text x="50%" y="50%" textAnchor="middle" dominantBaseline="central">{target}</text>
    </svg>
  );
}

interface Props {
  overview: MasteryOverview;
  controls: MasteryControls;
  onChange: (patch: Partial<MasteryControls>) => void;
  /** Unix seconds. */
  now: number;
  clockFormat: ClockFormat;
  tracked: string[];
  onTrackToggle: (uniqueName: string) => void;
  /** Tells the parent which result view the plan draws from, so a platinum plan gets quotes. */
  onView: (view: ResultView) => void;
}

function Placeholder({ path, entry, overview }: { path: string; entry: PlanEntry | null; overview: MasteryOverview }) {
  // A system has no Collection row, so its Spend opportunity stands in for one until the evaluation lands.
  const source = entry?.source ?? overview.categories.flatMap(c => c.sources).find(s => s.unique_name === path)
    ?? overview.opportunities.find(o => o.unique_name === path) ?? null;
  const name = source?.name ?? path.split("/").pop() ?? path;
  return (
    <div className={oppClass(entry?.completed ?? false)}>
      <ItemImg imageName={source?.image_name ?? undefined} fallbackText={name[0]?.toUpperCase() ?? "?"} />
      <div className={MST_OPP_MAIN}>
        <div className={MST_OPP_TITLE}>
          <span className={MST_OPP_NAME}>{name}</span>
          {source && <span className={MST_MR}>{source.category}</span>}
        </div>
        {entry && entry.notes.length > 0 && <div className={MST_OPP_BLOCKERS}>{entry.notes.join(" · ")}</div>}
        {!entry && <div className={MST_OPP_DETAIL}>Evaluating…</div>}
      </div>
      {entry?.completed && <span className={pillClass("done")}>Completed</span>}
      {source && <span className={rankClass(source.state)}>{remainingText(entry ? entry.gain : source.remaining_mastery, entry?.opportunity?.forma)}</span>}
    </div>
  );
}

export default function TargetMr({ overview, controls, onChange, now, clockFormat, tracked, onTrackToggle, onView }: Props) {
  const [plan, setPlan] = useState<MasteryPlan | null>(null);
  // A plan that was never saved prefills itself once the gap is known. A cleared plan stays empty.
  const [fresh, setFresh] = useState(false);
  const [evaluation, setEvaluation] = useState<PlanEvaluation | null>(null);
  const [search, setSearch] = useState("");
  const [showRange, setShowRange] = useState(false);
  const [opened, setOpened] = useState<{ opportunity: Opportunity; notes?: string[] } | null>(null);

  useEffect(() => {
    let stale = false;
    setPlan(null);
    setEvaluation(null);
    invoke<MasteryPlan | null>(TAURI_COMMANDS.LOAD_MASTERY_PLAN)
      .then(saved => {
        if (stale) return;
        if (saved) { setPlan({ ...saved, view: planView(saved.view) }); setFresh(false); }
        else { setPlan({ target: (overview.mastery_rank ?? 0) + 1, view: "suggestions", selections: [], allowances: {} }); setFresh(true); }
      })
      .catch(e => console.error("load_mastery_plan failed:", e));
    return () => { stale = true; };
  }, [overview.mastery_rank]);

  // Every observation re-evaluates the same selections. Only the player changes them.
  useEffect(() => {
    if (!plan) return;
    let stale = false;
    invoke<PlanEvaluation>(TAURI_COMMANDS.EVALUATE_MASTERY_PLAN, { plan: { ...plan, purchase_comparison: controls.comparison } })
      .then(e => { if (!stale) setEvaluation(e); })
      .catch(() => {});
    return () => { stale = true; };
  }, [plan, overview, controls.comparison]);

  const view = plan ? planView(plan.view) : "suggestions";
  useEffect(() => { onView(view); }, [view]); // eslint-disable-line
  // A stale saved category filters nothing, as in What next.
  const { category } = activeFilters(overview, controls, search);
  const pool = useMemo(() => candidates(overview.opportunities, { ...controls, category }, view, search), [overview.opportunities, controls, category, view, search]);

  const save = (next: MasteryPlan) => {
    next = { ...next, intrinsic_targets: snapshotIntrinsicTargets(next, overview.opportunities), purchase_comparison: controls.comparison };
    setPlan(next);
    setFresh(false);
    invoke<void>(TAURI_COMMANDS.SAVE_MASTERY_PLAN, { plan: next }).catch(e => console.error("save_mastery_plan failed:", e));
  };

  // Before the rank is observed the default target is MR 1 and the gap zero, so the prefill waits.
  // The load above runs again once the rank lands and picks the target from it.
  useEffect(() => {
    if (!fresh || !plan || overview.mastery_rank == null || evaluation?.gap == null) return;
    save({ ...plan, selections: prefill(pool, evaluation.gap) });
  }, [fresh, evaluation]); // eslint-disable-line

  if (!plan) return <div className={MST_BODY}><div className={MST_EMPTY}>Loading plan…</div></div>;

  const entries = evaluation && evaluation.entries.length === plan.selections.length ? evaluation.entries : null;
  const entryAt = (i: number): PlanEntry | null => entries && entries[i].unique_name === plan.selections[i] ? entries[i] : null;
  const toAdd = addable(pool, plan);
  const setTarget = (raw: string) => {
    const target = Math.min(TARGET_RANK_MAX, Math.max(1, Number.parseInt(raw, 10) || 1));
    if (target !== plan.target) save({ ...plan, target });
  };
  const setAllowance = (path: string, raw: string, max: number) => {
    const allowances = { ...plan.allowances };
    const count = Math.min(max, Math.max(0, Number.parseInt(raw, 10) || 0));
    if (count > 0) allowances[path] = count; else delete allowances[path];
    save({ ...plan, allowances });
  };

  return (
    <>
      <div className={MST_TOOLBAR}>
        <label className={MST_SELECT}>Target
          <input type="number" className={MST_PLAN_TARGET} min={1} max={TARGET_RANK_MAX} value={plan.target} onChange={e => setTarget(e.target.value)} aria-label="Target Mastery Rank" />
        </label>
        <div role="group" aria-label="Prefill from" className={MST_VIEW_SWITCH}>
          {RESULT_OPTIONS.map(v => (
            <button
              key={v.key}
              aria-pressed={view === v.key}
              className={filterBtnClass(view === v.key)}
              onClick={() => save({ ...plan, view: v.key })}
            >
              {v.label}
            </button>
          ))}
        </div>
        <button className={filterBtnClass()} disabled={evaluation?.gap == null} title="Fill the plan again from the chosen view, replacing the current selections"
          onClick={() => evaluation?.gap != null && save({ ...plan, selections: prefill(pool, evaluation.gap), intrinsic_targets: {} })}>
          Regenerate
        </button>
        <button className={filterBtnClass()} disabled={plan.selections.length === 0} onClick={() => save({ ...plan, selections: [] })}>Clear all</button>
      </div>

      <div className={MST_TOOLBAR_WRAP}>
        <Rings rings={evaluation ? ringFractions(evaluation) : null} target={plan.target} title={evaluation ? ringTitle(evaluation, plan.target) : "Evaluating plan…"} />
        {evaluation ? (
          <>
            <span className={MST_PLAN_SUMMARY_TEXT} title='Projection means "if these actions finish"'>{summaryText(evaluation, plan.target)}</span>
            {evaluation.total && (
              <button className={filterBtnClass()} aria-expanded={showRange} onClick={() => setShowRange(s => !s)}>{showRange ? "Hide range" : "Range"}</button>
            )}
            {evaluation.total_reason && <span className={MST_PLAN_RANGE}>Lower bound: {evaluation.total_reason}</span>}
            {showRange && <span className={MST_PLAN_RANGE}>{rangeText(evaluation)}</span>}
            {evaluation.rejected_allowances.length > 0 && (
              <span className={MST_OPP_BLOCKERS}>Allowance refused for unmastered or unowned equipment</span>
            )}
          </>
        ) : <span className={MST_PLAN_SUMMARY_TEXT}>Evaluating plan…</span>}
      </div>

      <Filters overview={overview} controls={controls} onChange={onChange} result={view} search={search} onSearch={setSearch} />

      <div className={MST_BODY}>
        <section className={MST_GROUP} aria-label="Plan">
          <div className={MST_GROUP_HEADER}>Plan <span className={MST_COUNT}>{plan.selections.length}</span></div>
          {plan.selections.length === 0 && (
            <div className={MST_EMPTY}>
              {evaluation?.gap == null ? "Mastery Rank unknown. Add actions below, or regenerate once a scan has seen the account."
                : "Nothing planned. Regenerate to fill from the chosen view, or add actions below."}
            </div>
          )}
          <div className={MST_OPP_LIST}>
            {plan.selections.map((path, i) => {
              const entry = entryAt(i);
              const o = entry?.opportunity ?? null;
              const controlsFor = (
                <span className={MST_PLAN_CONTROLS}>
                  {o?.craft && !o.owned && (
                    <button className={planBtnClass(tracked.includes(path))} title={tracked.includes(path) ? "Tracked in Foundry" : "Track in Foundry"} onClick={() => onTrackToggle(path)}>
                      {tracked.includes(path) ? "★" : "☆"}
                    </button>
                  )}
                  <button className={planBtnClass()} aria-label="Move up" disabled={i === 0} onClick={() => save({ ...plan, selections: moved(plan.selections, i, -1) })}>↑</button>
                  <button className={planBtnClass()} aria-label="Move down" disabled={i === plan.selections.length - 1} onClick={() => save({ ...plan, selections: moved(plan.selections, i, 1) })}>↓</button>
                  <button className={planBtnClass()} aria-label="Remove" onClick={() => save({ ...plan, selections: plan.selections.filter((_, j) => j !== i) })}>✕</button>
                </span>
              );
              return (
                <div key={`${path}#${i}`} className={MST_PLAN_ROW}>
                  <span className={MST_PLAN_INDEX}>{i + 1}</span>
                  {o ? (
                    <OpportunityRow opportunity={o} notes={entry?.notes} clockFormat={clockFormat} onOpen={() => setOpened({ opportunity: o, notes: entry?.notes })}
                      levelFirst={o.action === "level" && entries?.slice(i + 1).some(e => e.opportunity?.craft?.level_first?.some(step => step.unique_name === path))}>
                      {controlsFor}
                    </OpportunityRow>
                  ) : (
                    <><Placeholder path={path} entry={entry} overview={overview} />{controlsFor}</>
                  )}
                  {entry && entry.allowable.length > 0 && (
                    <div className={MST_PLAN_ALLOWANCES}>
                      {entry.allowable.map(a => (
                        <label key={a.unique_name} className={MST_SELECT} title="How many copies of this mastered piece the plan may spend as an ingredient">
                          Spend owned {a.name}
                          <input type="number" className={MST_PLAN_TARGET} min={0} max={a.owned} value={plan.allowances[a.unique_name] ?? 0}
                            onChange={e => setAllowance(a.unique_name, e.target.value, a.owned)} />
                          <span className={MST_COUNT}>of {a.owned}</span>
                        </label>
                      ))}
                    </div>
                  )}
                </div>
              );
            })}
          </div>
        </section>

        <section className={MST_GROUP} aria-label="Add to plan">
          <div className={MST_GROUP_HEADER}>
            Add from {RESULT_OPTIONS.find(v => v.key === view)?.label} <span className={MST_COUNT}>{toAdd.length}</span>
          </div>
          {toAdd.length === 0 && <div className={MST_EMPTY}>Nothing left to add from this view.</div>}
          <div className={MST_OPP_LIST}>
            {toAdd.slice(0, ADD_LIST_LIMIT).map(o => (
              <OpportunityRow key={o.unique_name} opportunity={o} clockFormat={clockFormat} onOpen={() => setOpened({ opportunity: o })}>
                <span className={MST_PLAN_CONTROLS}>
                  <button className={planBtnClass()} aria-label={`Add ${o.name}`} onClick={() => save({ ...plan, selections: addToPlan(plan.selections, o) })}>+</button>
                </span>
              </OpportunityRow>
            ))}
          </div>
          {toAdd.length > ADD_LIST_LIMIT && <div className={MST_EMPTY}>{toAdd.length - ADD_LIST_LIMIT} more; narrow the search.</div>}
        </section>
      </div>

      {opened && <OpportunityModal {...opened} now={now} clockFormat={clockFormat} onClose={() => setOpened(null)} />}
    </>
  );
}
