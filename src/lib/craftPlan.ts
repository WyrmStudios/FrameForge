import type { CraftPlan, Requirement } from "../types/mastery";
import type { RecipeComponent, RecipeComponentStatus } from "../types/items";

export type CraftRow = Requirement & { crafts: number };

export function craftRows(plan: CraftPlan): CraftRow[] {
  const crafts = new Map(plan.builds.map(b => [b.unique_name, b.crafts]));
  return plan.requirements
    .map(r => ({ ...r, crafts: crafts.get(r.unique_name) ?? 0 }))
    .sort((a, b) => a.name.localeCompare(b.name));
}

const covered = (plan: CraftPlan) => plan.requirements.every(r => r.short === 0);

/** A plan with no requirements belongs to a target without a recipe, so an empty plan never counts as craftable. */
export const craftableNow = (plan: CraftPlan) => plan.requirements.length > 0 && plan.builds.length === 0 && covered(plan);

export const buildable = (plan: CraftPlan) => plan.builds.length > 0 && covered(plan);

const line = (plan: CraftPlan, uniqueName: string) => plan.requirements.find(r => r.unique_name === uniqueName);

/** A recipe's own blueprint is the one component without ingredients, the same rule the backend uses. */
export const isBlueprint = (c: RecipeComponent) => c.components.length === 0 && c.unique_name.endsWith("Blueprint");

/** Akbolto lists Bolto twice. The plan already sums duplicates onto one line, so only the names need deduplicating here. */
export function distinct(comps: RecipeComponent[]): RecipeComponent[] {
  return comps.filter((c, i) => comps.findIndex(o => o.unique_name === c.unique_name) === i);
}

/** A line that stock covers whole reads as "part", and one the plan builds from a blueprint in hand reads as "blueprint". */
export function componentStatus(comp: RecipeComponent, plan: CraftPlan): RecipeComponentStatus {
  const own = line(plan, comp.unique_name);
  if (!own || own.short > 0) return "none";
  if (!plan.builds.some(b => b.unique_name === comp.unique_name)) return "part";
  const bp = comp.components.find(isBlueprint);
  return bp && (line(plan, bp.unique_name)?.from_stock ?? 0) > 0 ? "blueprint" : "none";
}
