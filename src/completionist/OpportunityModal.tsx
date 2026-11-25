import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import ItemImg from "../ItemImg";
import { useModal } from "../shared/useModal";
import { IngredientIcon } from "../shared/IngredientIcons";
import { ModalCloseButton } from "../shared/ui/ModalCloseButton";
import { EmptyMessage } from "../shared/ui/FilterControls";
import { TAURI_COMMANDS } from "../constants/tauri";
import { CREDITS_PATH, ingredientRing } from "../constants/ingredients";
import { blockerText } from "../constants/blockers";
import { dayAndClock, formatUnixTime } from "../lib/formatters";
import type { ClockFormat } from "../types/settings";
import { availableText, ingredients, stateLabel } from "../lib/ingredients";
import { distinct, isBlueprint } from "../lib/craftPlan";
import { ACCESS_LABELS, acquisitionLines, actionText, alsoNeedsText, chanceText, sourceLines } from "./suggestions";
import type { Opportunity, Requirement } from "../types/mastery";
import type { RecipeComponent } from "../types/items";
import { MST_MR, pillClass, MST_PILL_KIND, MST_OPP_BLOCKERS, MST_OPP_ACTION, OPP_SOURCE, OPP_ACQUISITION, OPP_STATE, OPP_MODAL_OVERLAY, OPP_MODAL, OPP_MODAL_HEADER, OPP_MODAL_TITLE, OPP_MODAL_BODY, recipeRowClass, RECIPE_CHEVRON, RECIPE_CHEVRON_LEAF, RECIPE_NAME, RECIPE_COUNTS } from "./masteryStyles";

type RecipeNode = Pick<RecipeComponent, "unique_name" | "name" | "components">;

interface LineProps {
  node: RecipeNode;
  rows: Requirement[];
  opportunity: Opportunity;
  now: number;
  depth: number;
  blueprint?: boolean;
}

/**
 * The icon row hides a part's own blueprint. Here it keeps a line of its own, because its relics
 * and drops are where a "Blueprint missing" part gets fixed. A line the plan never reached, because
 * its parent came out of stock, keeps its place and shows no counts.
 */
function Line({ node, rows, opportunity, now, depth, blueprint = false }: LineProps) {
  const [open, setOpen] = useState(false);
  const row = rows.find(r => r.unique_name === node.unique_name);
  const acquisition = node.components.length > 0 ? [] : acquisitionLines(opportunity, node.unique_name, now, blueprint);
  const expandable = node.components.length > 0 || acquisition.length > 0;
  const counts = row && availableText(row);
  return (
    <div style={{ marginLeft: depth * 16 }}>
      <div className={recipeRowClass(expandable)} onClick={() => expandable && setOpen(o => !o)}>
        {expandable
          ? <span className={RECIPE_CHEVRON}>{open ? "▾" : "▸"}</span>
          : <span className={RECIPE_CHEVRON_LEAF}>·</span>}
        {row && <IngredientIcon line={row} />}
        <span className={RECIPE_NAME}>{node.name}</span>
        {counts && <span className={RECIPE_COUNTS}>{counts}</span>}
        {row && <span className={`${OPP_STATE} ${ingredientRing(row.state)}`}>{stateLabel(row)}</span>}
      </div>
      {open && distinct(node.components).map(child => (
        <Line key={child.unique_name} node={child} rows={rows} opportunity={opportunity} now={now} depth={depth + 1} />
      ))}
      {open && acquisition.map(text => <div key={text} className={OPP_ACQUISITION}>{text}</div>)}
    </div>
  );
}

const leaf = (r: Requirement): RecipeNode => ({ unique_name: r.unique_name, name: r.name, components: [] });

interface Props {
  opportunity: Opportunity;
  notes?: string[];
  /** Unix seconds. */
  now: number;
  clockFormat: ClockFormat;
  onClose: () => void;
}

export function OpportunityModal({ opportunity: o, notes = [], now, clockFormat, onClose }: Props) {
  const modal = useModal(onClose);
  const [recipe, setRecipe] = useState<RecipeComponent[] | null>(null);
  // An owned copy's plan is the Forma it still needs, which no recipe tree frames, so it renders flat.
  const tree = !!o.craft && !o.owned;
  useEffect(() => {
    if (!tree) return;
    let stale = false;
    invoke<RecipeComponent[]>(TAURI_COMMANDS.GET_RECIPE, { uniqueName: o.unique_name })
      .then(r => { if (!stale) setRecipe(r); })
      .catch(() => { if (!stale) setRecipe([]); });
    return () => { stale = true; };
  }, [tree, o.unique_name]);

  const readyAt = o.build_completion_ms == null ? undefined : formatUnixTime(Math.floor(o.build_completion_ms / 1000), clockFormat);
  const source = sourceLines(o, now, readyAt);
  const blockers = [...o.blockers.map(blockerText), ...notes];
  const rows = useMemo(() => o.craft ? ingredients(o.craft) : [], [o.craft]);
  const credits = rows.find(r => r.unique_name === CREDITS_PATH);
  const lineProps = { rows, opportunity: o, now, depth: 0 };
  return (
    <dialog className={OPP_MODAL_OVERLAY} {...modal}>
      <div className={OPP_MODAL} onClick={e => e.stopPropagation()}>
        <div className={OPP_MODAL_HEADER}>
          <ItemImg imageName={o.image_name ?? undefined} category={o.category} size={36} />
          <span className={OPP_MODAL_TITLE}>{o.name}</span>
          <span className={MST_MR}>{o.category}</span>
          {o.mastery_req != null && o.mastery_req > 0 && <span className={MST_MR}>MR{o.mastery_req}</span>}
          <span className={MST_OPP_ACTION}>{actionText(o, ms => dayAndClock(ms, clockFormat))}</span>
          {o.relic && (
            <span className={pillClass(`relic-${o.relic.coverage.kind}`)}><span className={MST_PILL_KIND}>Relics</span> {chanceText(o.relic.coverage)}</span>
          )}
          <span className={pillClass(o.access)}>{ACCESS_LABELS[o.access]}</span>
          <ModalCloseButton onClick={onClose}>✕</ModalCloseButton>
        </div>
        <div className={OPP_MODAL_BODY}>
          {(source.length > 0 || blockers.length > 0 || o.purchase) && (
            <div className={OPP_SOURCE}>
              {source.map((text, i) => <div key={i}>{text}</div>)}
              {o.purchase && <div>Also needs {alsoNeedsText(o)}</div>}
              {blockers.length > 0 && <div className={`${MST_OPP_BLOCKERS} whitespace-normal!`}>{blockers.join(" · ")}</div>}
            </div>
          )}
          {credits && <Line node={leaf(credits)} {...lineProps} />}
          {tree ? (
            recipe == null ? <EmptyMessage>Loading…</EmptyMessage>
            : recipe.length === 0 ? <EmptyMessage>No recipe data.</EmptyMessage>
            : distinct(recipe).map(node => (
              <Line key={node.unique_name} node={node} blueprint={isBlueprint(node)} {...lineProps} />
            ))
          ) : rows.filter(r => r !== credits).map(r => <Line key={r.unique_name} node={leaf(r)} {...lineProps} />)}
        </div>
      </div>
    </dialog>
  );
}
