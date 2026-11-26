import type { MasteryCategory, MasterySource } from "../types/mastery";

const GROUP_ORDER = ["Railjack", "Drifter", "Standard", "Zaw", "Kitgun", "Amp", "Prime", "Kuva", "Tenet", "Coda", "Wraith", "Vandal", "Prisma", "MK1"];

export function masteryGroup(source: MasterySource): string {
  // The game's own field naming is the only system marker a track row carries.
  if (source.category === "Intrinsics") return source.unique_name.startsWith("LPS_DRIFT_") ? "Drifter" : "Railjack";
  if (source.node) return source.node.planet;
  const path = source.unique_name;
  if (path.includes("/Ostron/Melee/")) return "Zaw";
  if (path.includes("/SolarisUnited/") || path.includes("/InfKitGun/")) return "Kitgun";
  if (path.includes("/OperatorAmplifiers/")) return "Amp";
  const name = source.name;
  if (/^mk1-/i.test(name)) return "MK1";
  if (name.startsWith("Kuva ")) return "Kuva";
  if (name.startsWith("Tenet ")) return "Tenet";
  if (name.startsWith("Coda ")) return "Coda";
  if (name.includes("Prime")) return "Prime";
  if (name.includes("Wraith")) return "Wraith";
  if (name.includes("Vandal")) return "Vandal";
  if (name.includes("Prisma")) return "Prisma";
  return "Standard";
}

export interface SourceGroup {
  group: string;
  sources: MasterySource[];
}

export function groupSources(sources: MasterySource[]): SourceGroup[] {
  const byGroup = new Map<string, MasterySource[]>();
  for (const source of sources) {
    const group = masteryGroup(source);
    const list = byGroup.get(group) ?? [];
    list.push(source);
    byGroup.set(group, list);
  }
  const rank = (group: string) => {
    const i = GROUP_ORDER.indexOf(group);
    return i === -1 ? GROUP_ORDER.length : i;
  };
  // Planets are not in GROUP_ORDER, so they rank equal and keep the chart
  // order they arrived in. Their rows stay unsorted for the same reason: the
  // backend already lists junctions first and pairs each node's modes.
  return [...byGroup.entries()]
    .sort(([a], [b]) => rank(a) - rank(b))
    .map(([group, list]) => ({ group, sources: list[0]?.node ? list : list.sort((a, b) => a.name.localeCompare(b.name)) }));
}

export function systemRank(tracks: MasterySource[]): string {
  const rank = tracks.some(t => t.earned_rank == null) ? "?" : tracks.reduce((sum, t) => sum + (t.earned_rank ?? 0), 0);
  return `${rank}/${tracks.reduce((sum, t) => sum + t.cap, 0)}`;
}

export function matchesSearch(source: MasterySource, query: string): boolean {
  const q = query.toLowerCase();
  // A group matches by prefix, since a substring match for "an" would list
  // every Standard weapon while someone types "Ankyros".
  return source.name.toLowerCase().includes(q) || masteryGroup(source).toLowerCase().startsWith(q);
}

export interface Section {
  key: string;
  header: string | null;
  sources: MasterySource[];
}

export function sectionCategories(categories: MasteryCategory[], keep: (source: MasterySource) => boolean): Section[] {
  const prefixed = categories.length > 1;
  return categories.flatMap(category => {
    // The lone-group test and the system rank read the whole category, so a
    // filter can neither flicker the header away nor shrink a system's sum.
    const whole = groupSources(category.sources);
    const lone = whole.length === 1;
    return groupSources(category.sources.filter(keep)).map(({ group, sources }) => {
      const label = category.category === "Intrinsics"
        ? `${group} ${systemRank(whole.find(g => g.group === group)?.sources ?? [])}`
        : group;
      const header = lone ? (prefixed ? category.category : null) : prefixed ? `${category.category} · ${label}` : label;
      return { key: `${category.category}/${group}`, header, sources };
    });
  });
}
