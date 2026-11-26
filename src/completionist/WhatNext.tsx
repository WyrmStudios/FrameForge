import { useEffect, useMemo, useState, type KeyboardEvent, type MouseEvent, type ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import ItemImg from "../ItemImg";
import ItemMarketPopup from "../market/ItemMarketPopup";
import { IngredientIcons } from "../shared/IngredientIcons";
import Filters from "./Filters";
import { TAURI_COMMANDS } from "../constants/tauri";
import { wfmSlugLookup } from "../utils";
import { dayAndClock } from "../lib/formatters";
import type { ClockFormat } from "../types/settings";
import {
  actionText, activeFilters, chanceText, costText, quoteText, remainingText, shownControls, visibleOpportunities, visiblePurchases,
  ACCESS_LABELS, COMPARISON_OPTIONS, RELIC_GROUP_LABELS, RELIC_GROUP_ORDER, RESULT_OPTIONS, STAGE_LABELS, STAGE_ORDER,
  type Comparison, type MasteryControls, type Priced,
} from "./suggestions";
import { OpportunityModal } from "./OpportunityModal";
import type { Listing, MasteryOverview, Opportunity } from "../types/mastery";
import type { WfmItem } from "../types/market";
import type { WfmSession } from "../types/tauri";
import { MST_TABS, MST_BODY, MST_EMPTY, MST_GROUP, MST_GROUP_HEADER, MST_COUNT, MST_MR, MST_OPP_NAME, mstTabClass, pillClass, MST_PILL_KIND, rankClass, oppClass, MST_OPP_LIST, MST_OPP_MAIN, MST_OPP_TITLE, MST_OPP_BLOCKERS, MST_OPP_ACTION, MST_OPP_LISTINGS, quoteClass } from "./masteryStyles";

/** A click on a button inside the row belongs to the button. Enter opens only from the row itself, because a focused button's Enter bubbles up too. */
const rowOpeners = (open: () => void) => ({
  onClick: (e: MouseEvent) => { if (!(e.target as HTMLElement).closest("button")) open(); },
  onKeyDown: (e: KeyboardEvent) => { if (e.key === "Enter" && e.target === e.currentTarget) open(); },
});

function Title({ opportunity: { category, mastery_req, name, action, needed_for } }: { opportunity: Opportunity }) {
  return (
    <div className={MST_OPP_TITLE}>
      <span className={MST_OPP_NAME}>{name}</span>
      {action === "level" && needed_for.length > 0 && <span className={MST_MR}>Needed for {needed_for.join(", ")}</span>}
      <span className={MST_MR}>{category}</span>
      {mastery_req != null && mastery_req > 0 && (
        <span className={MST_MR} title={`Mastery Rank ${mastery_req} required`}>MR{mastery_req}</span>
      )}
    </div>
  );
}

function Remaining({ opportunity: { remaining_mastery, state, spend, forma } }: { opportunity: Opportunity }) {
  return (
    <span className={rankClass(state)} title={remaining_mastery == null ? "Remaining mastery unknown: no scan yet" : "Remaining mastery"}>
      {remainingText(spend?.mastery ?? remaining_mastery, forma)}
    </span>
  );
}

interface RowProps {
  opportunity: Opportunity;
  clockFormat: ClockFormat;
  notes?: string[];
  levelFirst?: boolean;
  onOpen: () => void;
  children?: ReactNode;
}

export function OpportunityRow({ opportunity, clockFormat, notes = [], levelFirst = false, onOpen, children }: RowProps) {
  const { access, craft, image_name, name, relic } = opportunity;
  return (
    <div className={oppClass(access === "blocked")} tabIndex={0} {...rowOpeners(onOpen)}>
      <ItemImg imageName={image_name ?? undefined} fallbackText={name[0]?.toUpperCase() ?? "?"} />
      <div className={MST_OPP_MAIN}>
        <Title opportunity={opportunity} />
        {craft && <IngredientIcons plan={craft} />}
        {notes.length > 0 && <div className={MST_OPP_BLOCKERS}>{notes.join(" · ")}</div>}
      </div>
      <span className={MST_OPP_ACTION}>{levelFirst ? `Level ${name} first` : actionText(opportunity, ms => dayAndClock(ms, clockFormat))}</span>
      {relic && (
        <span className={pillClass(`relic-${relic.coverage.kind}`)} title="Chance of every missing relic part dropping from the relics you own, run solo at their current refinement">
          <span className={MST_PILL_KIND}>Relics</span> {chanceText(relic.coverage)}
        </span>
      )}
      <span className={pillClass(access)}>{ACCESS_LABELS[access]}</span>
      <Remaining opportunity={opportunity} />
      {children}
    </div>
  );
}

interface PurchaseRowProps {
  opportunity: Priced;
  comparison: Comparison;
  now: number;
  onQuote: (listing: Listing) => void;
  onOpen: () => void;
}

function PurchaseRow({ opportunity, comparison, now, onQuote, onOpen }: PurchaseRowProps) {
  const { access, image_name, name, purchase } = opportunity;
  const set = purchase.set;
  const cost = comparison === "full" ? purchase.full_purchase : purchase.cheapest_finish;
  const count = (part: { needed: number; short: number }) => comparison === "full" ? part.needed : part.short;
  const parts = purchase.parts.filter(p => count(p) > 0);
  return (
    <div className={oppClass(access === "blocked")} tabIndex={0} {...rowOpeners(onOpen)}>
      <ItemImg imageName={image_name ?? undefined} fallbackText={name[0]?.toUpperCase() ?? "?"} />
      <div className={MST_OPP_MAIN}>
        <Title opportunity={opportunity} />
        <div className={MST_OPP_LISTINGS}>
          {set && (
            <button className={quoteClass(cost?.route === "set")} title="Market details" onClick={() => onQuote(set)}>
              {set.name}: {quoteText(set, now)}
            </button>
          )}
          {parts.map(p => (
            <button key={p.unique_name} className={quoteClass(cost?.route === "parts")} title="Market details" onClick={() => onQuote(p)}>
              {p.name}{count(p) > 1 ? ` ×${count(p)}` : ""}: {quoteText(p, now)}
            </button>
          ))}
        </div>
      </div>
      <span className={MST_OPP_ACTION}>{costText(opportunity, comparison)}</span>
      <span className={pillClass(access)}>{ACCESS_LABELS[access]}</span>
      <Remaining opportunity={opportunity} />
    </div>
  );
}

interface Props {
  overview: MasteryOverview;
  controls: MasteryControls;
  onChange: (patch: Partial<MasteryControls>) => void;
  /** Unix seconds. */
  now: number;
  clockFormat: ClockFormat;
}

export default function WhatNext({ overview, controls, onChange, now, clockFormat }: Props) {
  const [search, setSearch] = useState("");
  const [popup, setPopup] = useState<Listing | null>(null);
  const [opened, setOpened] = useState<Opportunity | null>(null);
  const [wfmUsername, setWfmUsername] = useState<string | null>(null);
  const [wfmLookup, setWfmLookup] = useState<Map<string, string>>(new Map());
  const { category, isFiltered } = activeFilters(overview, controls, search);
  const shown = useMemo(() => shownControls({ ...controls, category }), [controls, category]);
  const platinum = controls.result === "platinum";
  const visible = useMemo(
    () => platinum ? [] : visibleOpportunities(overview.opportunities, shown, search),
    [platinum, overview.opportunities, shown, search]);
  const purchases = useMemo(
    () => platinum ? visiblePurchases(overview.opportunities, shown, search) : [],
    [platinum, overview.opportunities, shown, search]);
  const groups = controls.result === "relics"
    ? RELIC_GROUP_ORDER.map(group => ({ key: group, label: RELIC_GROUP_LABELS[group], items: visible.filter(o => o.relic?.coverage.kind === group) }))
    : STAGE_ORDER.map(stage => ({ key: stage, label: STAGE_LABELS[stage], items: visible.filter(o => o.stage === stage) }));
  const listed = !platinum;
  const suggestions = controls.result === "suggestions";

  // A quote can sit under a catalogue slug the market does not list (a prime
  // part "Blueprint"), so the popup opens the slug the item list knows.
  useEffect(() => {
    if (!platinum || wfmLookup.size > 0) return;
    invoke<WfmItem[]>(TAURI_COMMANDS.FETCH_WFM_ITEMS)
      .then(items => setWfmLookup(wfmSlugLookup(items)))
      .catch(() => {});
  }, [platinum, wfmLookup.size]);

  // The popup can place orders once the Trading tab has logged in.
  useEffect(() => {
    if (!popup || wfmUsername != null) return;
    invoke<WfmSession | null>(TAURI_COMMANDS.WFM_GET_SESSION)
      .then(session => { if (session) setWfmUsername(session[0]); })
      .catch(() => {});
  }, [popup, wfmUsername]);

  return (
    <>
      <div className={MST_TABS} role="group" aria-label="Result view">
        {RESULT_OPTIONS.map(v => (
          <button
            key={v.key}
            aria-pressed={controls.result === v.key}
            className={mstTabClass(controls.result === v.key)}
            onClick={() => onChange({ result: v.key })}
          >
            {v.label}
          </button>
        ))}
      </div>

      <Filters overview={overview} controls={controls} onChange={onChange} result={controls.result} search={search} onSearch={setSearch} />

      <div className={MST_BODY}>
        {suggestions && overview.opportunities.length === 0 && (
          <div className={MST_EMPTY}>No suggestions yet: nothing owned or building with mastery left, and no known recipe or vendor route.</div>
        )}
        {controls.result === "relics" && !overview.opportunities.some(o => o.relic) && (
          <div className={MST_EMPTY}>No relic routes: nothing with mastery left is short a part that drops from a relic.</div>
        )}
        {listed && overview.opportunities.length > 0 && visible.length === 0 && (
          <div className={MST_EMPTY}>Nothing matches.</div>
        )}
        {listed && groups.filter(g => g.items.length > 0).map(({ key, label, items }) => (
          <section key={key} className={MST_GROUP} aria-label={label}>
            <div className={MST_GROUP_HEADER}>{label} <span className={MST_COUNT}>{items.length}</span></div>
            <div className={MST_OPP_LIST}>
              {items.map(o => <OpportunityRow key={o.unique_name} opportunity={o} clockFormat={clockFormat} onOpen={() => setOpened(o)} />)}
            </div>
          </section>
        ))}
        {platinum && purchases.length === 0 && (
          <div className={MST_EMPTY}>
            {isFiltered ? "Nothing matches." : "Nothing to buy: no missing part or whole item with mastery left is sold on warframe.market."}
          </div>
        )}
        {platinum && purchases.length > 0 && (
          <section className={MST_GROUP} aria-label="Purchases">
            <div className={MST_GROUP_HEADER}>
              Cached estimates, ranked by {COMPARISON_OPTIONS.find(c => c.key === controls.comparison)?.label.toLowerCase()} <span className={MST_COUNT}>{purchases.length}</span>
            </div>
            <div className={MST_OPP_LIST}>
              {purchases.map(o => (
                <PurchaseRow key={o.unique_name} opportunity={o} comparison={controls.comparison} now={now} onQuote={setPopup} onOpen={() => setOpened(o)} />
              ))}
            </div>
          </section>
        )}
      </div>

      {opened && <OpportunityModal opportunity={opened} now={now} clockFormat={clockFormat} onClose={() => setOpened(null)} />}
      {popup && (
        <ItemMarketPopup
          urlName={wfmLookup.get(popup.slug) ?? popup.slug}
          displayName={popup.name}
          onClose={() => setPopup(null)}
          isLoggedIn={wfmUsername != null}
          myUsername={wfmUsername ?? undefined}
        />
      )}
    </>
  );
}
