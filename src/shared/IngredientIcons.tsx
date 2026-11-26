import ItemImg from "../ItemImg";
import { iconLines, ingredientTitle } from "../lib/ingredients";
import { BLUEPRINT_ART, INGREDIENT_STATE_MARKS, ingredientRing } from "../constants/ingredients";
import type { CraftPlan, Requirement } from "../types/mastery";

const INGREDIENT = "relative inline-flex size-6.25 shrink-0 cursor-help items-center justify-center rounded-full border-[1.5px] border-[var(--ring)] bg-[color-mix(in_srgb,var(--ring)_12%,var(--color-surface))]";
// The art clips inside its own wrapper, because clipping the whole disc would cut off the corner mark.
const INGREDIENT_ART = "flex size-5.5 items-center justify-center overflow-hidden rounded-full";
// Item art is a dark render on a mostly transparent canvas, so at this size it is a dark speck on a
// dark disc. Zooming fills the circle with the render, and the lift plus the light halo keep its
// silhouette readable.
const INGREDIENT_IMG = "scale-120 [filter:brightness(1.3)_drop-shadow(0_0_1px_rgba(255,255,255,.55))]";
// The stock blueprint art is a grid of grey lines at 40% alpha on a transparent 462px canvas, and
// bilinear downsampling to this size erases it. Black behind it, nearest-neighbour sampling of a
// zoomed crop, and three zero-offset white shadows that stack the alpha keep a few lines visible.
const BLUEPRINT_ART_WRAP = `${INGREDIENT_ART} bg-black`;
const BLUEPRINT_IMG = "scale-250 [image-rendering:pixelated] [filter:drop-shadow(0_0_0_#fff)_drop-shadow(0_0_0_#fff)_drop-shadow(0_0_0_#fff)_brightness(1.8)]";
const INGREDIENT_FALLBACK = "text-9 font-semibold leading-none text-muted";
const INGREDIENT_MARK = "absolute -right-0.75 -bottom-0.75 text-10 leading-none text-[var(--ring)] [text-shadow:0_0_2px_var(--color-background),0_0_2px_var(--color-background)]";

export function IngredientIcon({ line, title }: { line: Requirement; title?: string }) {
  const blueprint = line.image_name === BLUEPRINT_ART;
  return (
    <span className={`${INGREDIENT} ${ingredientRing(line.state)}`} title={title}>
      <span className={blueprint ? BLUEPRINT_ART_WRAP : INGREDIENT_ART}>
        <ItemImg imageName={line.image_name ?? undefined} size={22} className={blueprint ? BLUEPRINT_IMG : INGREDIENT_IMG}
          fallback={<span className={INGREDIENT_FALLBACK}>{line.name.slice(0, 2)}</span>} />
      </span>
      {INGREDIENT_STATE_MARKS[line.state] && <span className={INGREDIENT_MARK}>{INGREDIENT_STATE_MARKS[line.state]}</span>}
    </span>
  );
}

export function IngredientIcons({ plan }: { plan: CraftPlan }) {
  const lines = iconLines(plan);
  if (lines.length === 0) return null;
  return (
    <div className="flex flex-wrap gap-1">
      {lines.map(line => <IngredientIcon key={line.unique_name} line={line} title={ingredientTitle(line)} />)}
    </div>
  );
}
