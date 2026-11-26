import { useMemo, useState, type ReactNode } from "react";
import type { ColorSwatch, ResolvedCompanion } from "../types/inventory";

const COMPANIONS_CLASS = "flex min-h-0 flex-1 flex-col overflow-hidden";
const SUB_TABS_CLASS = "flex shrink-0 gap-0.5 border-b border-border px-3 py-1.5";
const SUB_TAB_CLASS =
  "cursor-pointer rounded-4 border px-3.5 py-0.75 text-12 transition-[background,color,border-color] duration-100 flex items-center gap-1.5";
const SUB_TAB_IDLE_CLASS = "border-border/60 bg-transparent text-muted hover:bg-white/6 hover:text-foreground";
const SUB_TAB_ACTIVE_CLASS = "border-accent bg-accent/15 text-accent";
const SUB_TAB_COUNT = "text-10 bg-accent/18 text-accent py-0.25 px-1.25 rounded-10 min-w-4.5 text-center";

const GRID_CLASS = "flex-1 overflow-y-auto p-3 grid gap-3 grid-cols-[repeat(auto-fill,minmax(260px,1fr))] content-start";
const EMPTY_CLASS = "flex-1 flex items-center justify-center text-muted text-13 p-6 text-center";

const CARD_CLASS = "bg-surface border border-border rounded-8 overflow-hidden flex flex-col";
const HEADER_CLASS = "p-3 border-b border-border";
const NAME_CLASS = "text-16 font-bold text-foreground truncate flex items-center gap-1.5";
const PUPPY_BADGE_CLASS = "text-9 font-semibold uppercase tracking-wide bg-accent/18 text-accent py-0.25 px-1.5 rounded-10 shrink-0";
const META_CLASS = "mt-1 flex flex-wrap gap-x-4 gap-y-0.5 text-12 text-muted";
const META_LABEL_CLASS = "font-semibold text-foreground/80";

const TABLE_ROW_GRID = "grid grid-cols-[100px_1fr]";
const TABLE_HEAD_CLASS = `${TABLE_ROW_GRID} bg-white/5 text-11 font-semibold uppercase tracking-wide text-muted px-3 py-1.5 border-b border-border`;
const ROW_CLASS = `${TABLE_ROW_GRID} items-center gap-2 px-3 py-1.25 border-b border-border/40 last:border-b-0 text-12`;
const ROW_LABEL_CLASS = "text-muted";
const ROW_VALUE_CLASS = "text-foreground font-medium truncate flex items-center gap-1.5";

type SubTab = "kubrows" | "kavats" | "kubrowImprints" | "kavatImprints";

interface Props {
  companions: ResolvedCompanion[];
}

function Swatch({ swatch }: { swatch: ColorSwatch }) {
  return (
    <>
      <span className="inline-block size-3 rounded-full border border-white/15 shrink-0" style={{ background: swatch.hex }} />
      <span className="truncate">{swatch.name}</span>
    </>
  );
}

function Row({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className={ROW_CLASS}>
      <span className={ROW_LABEL_CLASS}>{label}</span>
      <span className={ROW_VALUE_CLASS}>{children}</span>
    </div>
  );
}

function CompanionCard({ companion }: { companion: ResolvedCompanion }) {
  const c = companion;
  const colorLabels = ["Base Color", "Secondary Color", "Tertiary Color", "Accent Color"];
  return (
    <div className={CARD_CLASS}>
      <div className={HEADER_CLASS}>
        <div className={NAME_CLASS}>
          {c.pet_name || c.breed_name}
          {c.is_puppy && <span className={PUPPY_BADGE_CLASS}>Puppy</span>}
        </div>
        <div className={META_CLASS}>
          <span><span className={META_LABEL_CLASS}>Breed:</span> {c.breed_name}</span>
          {c.gender && <span><span className={META_LABEL_CLASS}>Gender:</span> {c.gender}</span>}
          {c.size && <span><span className={META_LABEL_CLASS}>Size:</span> {c.size}</span>}
          {c.prints_remaining != null && <span><span className={META_LABEL_CLASS}>Prints:</span> {c.prints_remaining}</span>}
        </div>
      </div>
      <div className={TABLE_HEAD_CLASS}>
        <span>Trait</span>
        <span>Appearance</span>
      </div>
      <div>
        <Row label="Body type">{c.body_type ?? "—"}</Row>
        {colorLabels.map((label, i) => (
          <Row key={label} label={label.replace(" Color", " color")}>{c.colors[i] ? <Swatch swatch={c.colors[i]} /> : "—"}</Row>
        ))}
        <Row label="Eye color">{c.eye_color ? <Swatch swatch={c.eye_color} /> : "—"}</Row>
        <Row label="Pattern">{c.pattern ?? "—"}</Row>
        {c.species === "kavat" && (
          <>
            <Row label="Head type">{c.head_type ?? "—"}</Row>
            <Row label="Tail type">{c.tail_type ?? "—"}</Row>
          </>
        )}
      </div>
    </div>
  );
}

export default function Companions({ companions }: Props) {
  const [tab, setTab] = useState<SubTab>("kubrows");

  const kubrows = useMemo(() => companions.filter(c => c.species === "kubrow"), [companions]);
  const kavats = useMemo(() => companions.filter(c => c.species === "kavat"), [companions]);

  const list = tab === "kubrows" ? kubrows : tab === "kavats" ? kavats : [];

  return (
    <div className={COMPANIONS_CLASS}>
      <div className={SUB_TABS_CLASS}>
        <button className={`${SUB_TAB_CLASS} ${tab === "kubrows" ? SUB_TAB_ACTIVE_CLASS : SUB_TAB_IDLE_CLASS}`} onClick={() => setTab("kubrows")}>
          Kubrows
          <span className={SUB_TAB_COUNT}>{kubrows.length}</span>
        </button>
        <button className={`${SUB_TAB_CLASS} ${tab === "kavats" ? SUB_TAB_ACTIVE_CLASS : SUB_TAB_IDLE_CLASS}`} onClick={() => setTab("kavats")}>
          Kavats
          <span className={SUB_TAB_COUNT}>{kavats.length}</span>
        </button>
        <button className={`${SUB_TAB_CLASS} ${tab === "kubrowImprints" ? SUB_TAB_ACTIVE_CLASS : SUB_TAB_IDLE_CLASS}`} onClick={() => setTab("kubrowImprints")}>
          Kubrow Imprints
        </button>
        <button className={`${SUB_TAB_CLASS} ${tab === "kavatImprints" ? SUB_TAB_ACTIVE_CLASS : SUB_TAB_IDLE_CLASS}`} onClick={() => setTab("kavatImprints")}>
          Kavat Imprints
        </button>
      </div>
      {tab === "kubrowImprints" || tab === "kavatImprints" ? (
        <div className={EMPTY_CLASS}>Imprint tracking isn't available yet — this is coming in a future update.</div>
      ) : list.length === 0 ? (
        <div className={EMPTY_CLASS}>
          No {tab === "kubrows" ? "Kubrows" : "Kavats"} found yet. Open your Arsenal in-game (or connect the Companion API in Settings) to let FrameForge pick it up.
        </div>
      ) : (
        <div className={GRID_CLASS}>
          {list.map((c, i) => (
            <CompanionCard key={`${c.item_type}-${c.pet_name ?? i}`} companion={c} />
          ))}
        </div>
      )}
    </div>
  );
}
