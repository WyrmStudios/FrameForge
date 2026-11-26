import type { MasteryState } from "../types/mastery";

const FOCUS = "focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-accent";
const ROW_HOVER = "hover:border-border hover:bg-white/6 focus-visible:border-border focus-visible:bg-white/6";

export const MST_ROOT = "flex min-h-0 flex-1 flex-col overflow-hidden bg-background text-foreground";
export const MST_TOOLBAR = "flex shrink-0 items-center gap-3 px-3.5 pt-2.5 pb-1.5";
export const MST_TOOLBAR_WRAP = `${MST_TOOLBAR} flex-wrap`;
export const MST_VIEWS = "flex shrink-0 items-center gap-3 border-b border-border px-3.5 pt-2.5 pb-2";
export const MST_BUCKETS = "flex shrink-0 items-center gap-1.5 px-3.5 pb-1.5";
export const MST_VIEW_SWITCH = "flex gap-1";
export const MST_TABS = "flex shrink-0 gap-0.5 border-b border-border px-3 pt-2";
export const MST_BODY = "flex-1 overflow-y-auto px-3 pt-2 pb-4";
export const MST_EMPTY = "py-10 text-center text-14 text-muted";
export const MST_GROUP = "mb-5";
export const MST_GROUP_HEADER = "mb-1.5 border-b border-border pt-1 pb-1.5 text-11 font-semibold uppercase tracking-0.06 text-muted";
export const MST_GROUP_GRID = "grid grid-cols-[repeat(auto-fill,minmax(240px,1fr))] gap-0.75";
export const MST_COUNT = "opacity-70";
export const MST_MR = "shrink-0 text-10 text-muted";
export const MST_NAME = "min-w-0 flex-1 truncate text-12";
export const MST_OPP_NAME = "min-w-0 flex-initial truncate text-12";
export const MST_SEARCH = "w-40 shrink-0 rounded-5 border border-border bg-surface px-2 py-1 text-12 text-foreground placeholder:text-muted focus:border-accent focus:outline-none";
export const MST_SELECT =
  "flex items-center gap-1.5 text-12 text-muted [&_input[type=checkbox]]:m-0 [&_input:focus-visible]:outline-2 [&_input:focus-visible]:-outline-offset-2 [&_input:focus-visible]:outline-accent " +
  "[&_select]:rounded-5 [&_select]:border [&_select]:border-border [&_select]:bg-surface [&_select]:px-1.5 [&_select]:py-0.75 [&_select]:text-12 [&_select]:text-foreground " +
  "[&_select:focus-visible]:outline-2 [&_select:focus-visible]:-outline-offset-2 [&_select:focus-visible]:outline-accent";

export const mstTabClass = (active: boolean) =>
  `-mb-px cursor-pointer rounded-t-6 border-0 border-b-3 px-5 py-1.5 text-13 font-medium transition-[background,color] duration-150 ${FOCUS} ` +
  (active ? "border-accent bg-surface text-foreground" : "border-transparent bg-transparent text-muted hover:bg-white/6 hover:text-foreground");

export const filterBtnClass = (active = false) =>
  `cursor-pointer rounded-5 border px-2.5 py-1 text-12 transition-all duration-150 ${FOCUS} ` +
  (active ? "border-accent bg-accent text-white" : "border-border bg-transparent text-muted");

export const MST_PROGRESS_WRAP = "flex min-w-0 flex-1 items-center gap-2";
export const MST_PROGRESS_BAR = "h-1.5 max-w-50 flex-1 overflow-hidden rounded-3 bg-border";
export const MST_PROGRESS_FILL = "h-full rounded-3 bg-accent transition-[width] duration-300";
export const MST_PROGRESS_LABEL = "whitespace-nowrap text-12 text-muted";

const PILL = "shrink-0 whitespace-nowrap rounded-5 border px-2 py-0.5 text-11 font-semibold";
const PILL_WARNING = "border-warning/30 bg-warning/10 text-warning";
const PILL_TONES: Record<string, string> = {
  unknown: PILL_WARNING,
  unconfirmed: PILL_WARNING,
  blocked: "border-danger/30 bg-danger/10 text-danger",
  "relic-complete": "border-refinement-flawless/30 bg-refinement-flawless/10 text-refinement-flawless",
  done: "border-success/30 bg-success/10 text-success",
};
export const pillClass = (tone: string) => `${PILL} ${PILL_TONES[tone] ?? "border-border text-muted"}`;
export const MST_PILL_KIND = "font-medium opacity-70";

const RANK = "min-w-9 shrink-0 rounded-4 px-1.5 py-0.5 text-center text-11 font-bold";
const RANK_TONES: Partial<Record<MasteryState, string>> = {
  mastered: "bg-complete/15 text-complete",
  partial: "bg-ducat/12 text-ducat",
};
export const rankClass = (state: MasteryState) => `${RANK} ${RANK_TONES[state] ?? "bg-rank-none/7 text-muted"}`;

const ITEM = `flex items-center gap-2 rounded-5 border px-2 py-1.25 transition-[background] duration-100 hover:bg-white/6 focus-visible:bg-white/6 ${FOCUS}`;
const ITEM_STATES: Record<MasteryState, string> = {
  mastered: "border-complete/15",
  partial: "border-transparent hover:border-border focus-visible:border-border",
  missing: "border-transparent opacity-45 hover:border-border focus-visible:border-border",
  unknown: "border-transparent opacity-60 hover:border-border focus-visible:border-border",
};
export const itemClass = (state: MasteryState) => `${ITEM} ${ITEM_STATES[state]}`;

export const oppClass = (dimmed: boolean) =>
  `flex items-center gap-2.5 rounded-5 border border-transparent px-2 py-1.5 [&[tabindex]]:cursor-pointer ${ROW_HOVER} ${FOCUS}${dimmed ? " opacity-60" : ""}`;
export const MST_OPP_LIST = "flex flex-col gap-0.75";
export const MST_OPP_MAIN = "flex min-w-0 flex-1 flex-col gap-0.5";
export const MST_OPP_TITLE = "flex items-center gap-2";
export const MST_OPP_DETAIL = "overflow-hidden text-ellipsis whitespace-nowrap text-11 text-muted";
export const MST_OPP_BLOCKERS = "overflow-hidden text-ellipsis whitespace-nowrap text-11 text-warning";
export const MST_OPP_ACTION = "shrink-0 whitespace-nowrap text-12";
export const MST_OPP_LISTINGS = "flex flex-wrap gap-1";
export const quoteClass = (chosen: boolean) =>
  `cursor-pointer whitespace-nowrap rounded-4 border bg-transparent px-1.5 py-px text-11 hover:bg-white/6 hover:text-foreground ${FOCUS} ` +
  (chosen ? "border-accent text-foreground" : "border-border text-muted");

export const OPP_SOURCE = "flex flex-col gap-0.5 px-3 pb-2.5 text-12 text-muted";
export const OPP_ACQUISITION = "ml-6 mr-2 px-3 py-0.5 text-11 text-muted";

export const MST_PLAN_TARGET = "w-[4em] rounded-5 border border-border bg-surface px-1.5 py-0.75 text-12 text-foreground [color-scheme:dark] focus:border-accent focus:outline-none";
export const MST_PLAN_SUMMARY_TEXT = "text-12";
export const MST_PLAN_RANGE = "basis-full text-11 text-muted";
// In a plan row the index and the controls are spans, so the first div is the opportunity row.
export const MST_PLAN_ROW = "flex flex-wrap items-start gap-1.5 [&>div:first-of-type]:min-w-0 [&>div:first-of-type]:flex-1";
export const MST_PLAN_INDEX = "w-[1.5em] shrink-0 pt-2 text-right text-11 text-muted";
export const MST_PLAN_CONTROLS = "flex shrink-0 gap-0.5";
export const MST_PLAN_ALLOWANCES = "flex basis-full flex-wrap gap-2 pr-2 pb-1 pl-[2.5em]";
export const planBtnClass = (starred = false) =>
  `cursor-pointer rounded-4 border border-border bg-transparent px-1.5 py-0.5 text-12 leading-[1.2] enabled:hover:bg-white/6 enabled:hover:text-foreground disabled:cursor-default disabled:opacity-35 ${FOCUS} ` +
  (starred ? "text-ducat" : "text-muted");

export const MST_RINGS = "shrink-0 [&_circle]:fill-none [&_circle]:stroke-3 [&_text]:fill-foreground [&_text]:text-11 [&_text]:font-semibold";
export const MST_RING_TRACK = "stroke-muted opacity-25";
export const MST_RING = "origin-center -rotate-90 [stroke-linecap:round] transition-[stroke-dashoffset] duration-300";
export const MST_RING_EARNED = "stroke-accent";
export const MST_RING_PLANNED = "stroke-accent opacity-60";
export const MST_RING_COVERED = "stroke-success";
export const MST_RING_BAND = "stroke-muted";

export const OPP_STATE = "shrink-0 text-11 text-[var(--ring)]";

// These copy the Foundry recipe modal's classes. The overlay's extra resets undo the browser's default <dialog>
// box, so the overlay fills the top layer.
export const OPP_MODAL_OVERLAY = "fixed inset-0 z-200 flex size-auto max-h-none max-w-none items-center justify-center bg-black/72 p-5 text-inherit backdrop:bg-transparent";
export const OPP_MODAL = "flex max-h-[calc(82vh_/_var(--ff-scale,1))] w-[min(680px,95vw)] flex-col overflow-hidden rounded-12 border border-border bg-surface shadow-[0_20px_60px_rgba(0,0,0,.6)]";
export const OPP_MODAL_HEADER = "flex shrink-0 items-center gap-2.5 border-b border-border px-4.5 py-3.5";
export const OPP_MODAL_TITLE = "min-w-0 flex-1 overflow-hidden text-ellipsis whitespace-nowrap text-15 font-semibold";
export const OPP_MODAL_BODY = "flex-1 overflow-y-auto px-4.5 py-3";
export const recipeRowClass = (expandable: boolean) =>
  `mx-2 my-px flex items-center gap-1.5 rounded-4 px-3 py-1.25 text-13 transition-colors duration-100 hover:bg-white/4 ${expandable ? "cursor-pointer" : "cursor-default"}`;
export const RECIPE_CHEVRON = "w-3 shrink-0 text-11 text-muted";
export const RECIPE_CHEVRON_LEAF = "w-3 shrink-0 text-11 text-border";
export const RECIPE_NAME = "min-w-0 flex-1 overflow-hidden text-ellipsis whitespace-nowrap text-foreground";
export const RECIPE_COUNTS = "flex shrink-0 items-center gap-0.5 text-12 tabular-nums";
