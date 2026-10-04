import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow, LogicalSize } from "@tauri-apps/api/window";
import { clampToMonitor, overlayScale } from "../lib/uiScale";
import { DEFAULT_RELIC_PICK_LINES, DEFAULT_RELIC_PICK_PRIORITY, RELIC_PICK_LINES_OPTIONS, RELIC_PICK_PRIORITY_OPTIONS } from "../constants/settings";
import { TAURI_COMMANDS, TAURI_EVENTS } from "../constants/tauri";
import type { SettingsFile } from "../types/tauri";
import type { RelicPickPayload, RelicPickRelic, RelicPickReward } from "../types/relics";
import type { RelicPickLines, RelicPickPriority } from "../types/settings";
import "./RelicPickOverlay.css";

// ── Tailwind class constants (formerly RelicPickOverlay.css) ──────────────────
// Note: body transparency stays in RelicPickOverlay.css (document context).

const RPO_ROOT =
  "flex flex-col gap-1 py-1.5 px-2 bg-background/92 border border-accent/40 rounded-8 text-12 text-[color:var(--text,#e6edf3)] w-full h-auto backdrop-blur-xs";

const RPO_HEADER = "flex items-center gap-1.5 shrink-0";
const RPO_TITLE =
  "text-12 font-bold text-info tracking-0.04 uppercase flex-1";
const RPO_CLOSE =
  "bg-transparent border-0 text-foreground/35 text-13 cursor-pointer px-0.5 py-0 leading-none transition-colors duration-100 hover:text-foreground/85";
const RPO_EMPTY =
  "px-2 py-3 text-center text-foreground/40 text-11 whitespace-nowrap";
const RPO_LIST = "flex flex-col gap-1 overflow-y-auto max-h-115";

const RPO_CARD =
  "bg-white/4 border border-border/60 rounded-5 overflow-hidden shrink-0";
const RPO_CARD_BEST =
  "bg-accent/6 border border-accent/50 rounded-5 overflow-hidden shrink-0";
const RPO_CARD_HEADER =
  "flex items-center gap-1.25 px-2 py-1 border-b border-b-border/50";
const RPO_RANK = "text-10 font-bold text-foreground/30 min-w-4";
const RPO_RANK_BEST = "text-10 font-bold text-info min-w-4";
const RPO_RELIC_NAME =
  "flex-1 font-semibold text-12 whitespace-nowrap overflow-hidden text-ellipsis";

const REF_SHAPE =
  "text-9 font-semibold px-1 py-0.25 rounded-3 whitespace-nowrap";
const REF_BADGE: Record<string, string> = {
  intact:
    REF_SHAPE +
    " bg-white/7 text-foreground/50 border border-white/10",
  exceptional:
    REF_SHAPE +
    " text-refinement-exceptional border border-refinement-exceptional/30 bg-refinement-exceptional/8",
  flawless:
    REF_SHAPE +
    " text-refinement-flawless border border-refinement-flawless/30 bg-refinement-flawless/8",
  radiant:
    REF_SHAPE +
    " text-refinement-radiant border border-refinement-radiant/40 bg-refinement-radiant/10",
};

const RPO_COUNT = "text-11 text-foreground/45";
const RPO_SCORE = "text-11 font-semibold text-info whitespace-nowrap";

const RPO_ESTIMATED =
  "flex items-center gap-0.5 px-2 py-0.75 text-10 text-foreground/55";
const RPO_EST_SEP = "text-foreground/25";

const RPO_REWARDS = "flex flex-col";
const RPO_REWARD =
  "grid grid-cols-[14px_14px_1fr_auto_auto_auto] items-center gap-0.75 px-2 py-0.5 border-b border-b-border/25 text-10 last:border-b-0";
const RPO_VAULT = "text-9 text-center leading-none";
const RPO_OWNED = "text-10 font-bold text-center";
const RPO_OWNED_YES = RPO_OWNED + " text-success/85";
const RPO_OWNED_NO = RPO_OWNED + " text-foreground/25";
const RPO_REWARD_NAME = "whitespace-nowrap overflow-hidden text-ellipsis opacity-85";
const RARITY_NAME: Record<string, string> = {
  Bronze: RPO_REWARD_NAME + " text-reward-bronze",
  Silver: RPO_REWARD_NAME + " text-reward-silver",
  Gold: RPO_REWARD_NAME + " text-refinement-radiant",
};
const RPO_VAL_SHARED = "flex items-center gap-0.25 whitespace-nowrap tabular-nums";
const RPO_PLAT_VAL = RPO_VAL_SHARED + " text-refinement-exceptional/80";
const RPO_DUCAT_VAL = RPO_VAL_SHARED + " text-refinement-radiant/75";

const RPO_REC_SHAPE =
  "text-8 font-bold px-0.75 py-0.25 rounded-2 whitespace-nowrap border";
const RPO_REC: Record<string, string> = {
  intact: RPO_REC_SHAPE + " text-foreground/40 border-foreground/15",
  exceptional:
    RPO_REC_SHAPE +
    " text-refinement-exceptional border-refinement-exceptional/30 bg-refinement-exceptional/7",
  radiant:
    RPO_REC_SHAPE +
    " text-refinement-radiant border-refinement-radiant/35 bg-refinement-radiant/8",
};

const ERA_LABEL: Record<string, string> = {
  LITH: "Lith", MESO: "Meso", NEO: "Neo", AXI: "Axi", ALL: "All Eras",
};

const REWARD_ORDER: Record<string, number> = { Gold: 0, Silver: 1, Bronze: 2 };

// Bronze → run Intact (refining reduces common drop rate)
// Silver → Exceptional (solid improvement, low trace cost)
// Gold   → Radiant (rare items benefit most from full refinement)
function recRefinement(rarity: string): string {
  if (rarity === "Gold")   return "Radiant";
  if (rarity === "Silver") return "Exceptional";
  return "Intact";
}

function scoreOf(relic: RelicPickRelic, priority: RelicPickPriority): number {
  if (priority === "platinum") return relic.plat_score;
  if (priority === "ducat")    return relic.ducat_score;
  return relic.unowned_score;
}

function getDisplayRewards(relic: RelicPickRelic, lines: RelicPickLines, priority: RelicPickPriority): RelicPickReward[] {
  const byRarity = [...relic.rewards].sort(
    (a, b) => (REWARD_ORDER[a.rarity] ?? 3) - (REWARD_ORDER[b.rarity] ?? 3)
  );
  if (lines === "all" || lines === "estimated") return byRarity;

  // "best" mode
  if (priority === "platinum") {
    return [...relic.rewards].sort((a, b) => b.plat - a.plat).slice(0, 1);
  }
  if (priority === "ducat") {
    return [...relic.rewards].sort((a, b) => b.ducats - a.ducats).slice(0, 1);
  }
  // "unowned" best: all unowned items ranked by drop probability
  return relic.rewards
    .filter(r => !r.owned)
    .sort((a, b) => b.drop_rate - a.drop_rate);
}

function PlatIcon() {
  return <img src="/platinum.webp" alt="p" width={11} height={11}
    className="mb-px shrink-0 object-contain align-middle" />;
}
function DucatIcon() {
  return <img src="/ducats.webp" alt="d" width={11} height={11}
    className="mb-px shrink-0 object-contain align-middle" />;
}

export default function RelicPickOverlay() {
  const [payload,  setPayload]  = useState<RelicPickPayload | null>(null);
  // Outline mode (Settings → Overlays → Show Outline): dashed frame instead of relic cards.
  const [outline,  setOutline]  = useState(false);
  const [priority, setPriority] = useState<RelicPickPriority>(DEFAULT_RELIC_PICK_PRIORITY);
  const [lines,    setLines]    = useState<RelicPickLines>(DEFAULT_RELIC_PICK_LINES);
  // Use a callback ref so the ResizeObserver is set up each time the root div
  // mounts (payload goes null→non-null). A plain useRef+useEffect misses this
  // because the root div doesn't exist yet when the effect runs at mount time.
  const roRef   = useRef<ResizeObserver | null>(null);
  const rootRef = useRef<HTMLDivElement | null>(null);

  // The scale is a CSS transform, so it does not change the measured layout size.
  // The window must grow by the same factor that the content is drawn at.
  const syncSize = useCallback((layoutHeight: number) => {
    if (layoutHeight <= 0) return;
    const s = overlayScale();
    clampToMonitor(340 * s, layoutHeight * s)
      .then(([w, h]) => getCurrentWindow().setSize(new LogicalSize(Math.round(w), Math.round(h))))
      .catch(() => {});
  }, []);

  const rootCallback = useCallback((el: HTMLDivElement | null) => {
    if (roRef.current) { roRef.current.disconnect(); roRef.current = null; }
    rootRef.current = el;
    if (!el) return;
    // Measure the border box: contentRect excludes the root's padding and
    // border (14 px), which clipped the bottom of the window.
    const ro = new ResizeObserver(() => {
      const node = rootRef.current;
      if (node) syncSize(Math.ceil(node.offsetHeight));
    });
    ro.observe(el);
    roRef.current = ro;
  }, [syncSize]);

  const hide = () => {
    setPayload(null);
    getCurrentWindow().hide().catch(() => {});
  };

  useEffect(() => {
    const unOpen = listen<RelicPickPayload>(TAURI_EVENTS.RELIC_PICK_OPEN, async e => {
      // Real screen open wins over an active outline.
      setOutline(false);
      // Reload settings fresh on every show — the main window may have changed them
      // since this overlay was first mounted at app startup.
      try {
        const json = await invoke<string>(TAURI_COMMANDS.LOAD_SETTINGS);
        if (json) {
          const s = JSON.parse(json) as SettingsFile;
          if (RELIC_PICK_PRIORITY_OPTIONS.includes(s.relicPickPriority)) setPriority(s.relicPickPriority);
          if (RELIC_PICK_LINES_OPTIONS.includes(s.relicPickLines))        setLines(s.relicPickLines);
        }
      } catch {}
      setPayload(e.payload);
    });
    const unClose = listen(TAURI_EVENTS.RELIC_PICK_CLOSE, () => hide());
    // A scale change does not alter the layout size, so the ResizeObserver never
    // fires. Measure again to resize a window that is already open.
    const unScale = listen(TAURI_EVENTS.SETTINGS_UPDATED, () => {
      const el = rootRef.current;
      if (el) syncSize(Math.ceil(el.offsetHeight));
    });
    const unOutline = listen<string>(TAURI_EVENTS.OVERLAY_OUTLINE, e => {
      if (e.payload === "relicPick") setOutline(true);
      else if (e.payload === "off-relicPick") { setOutline(false); setPayload(null); }
    });
    return () => { unOpen.then(f => f()); unClose.then(f => f()); unScale.then(f => f()); unOutline.then(f => f()); };
  }, [syncSize]);

  if (outline) {
    return (
      <div ref={rootCallback} className="box-border flex h-75 w-full items-center justify-center rounded-10 border-2 border-dashed border-accent/85 bg-surface/55 text-base font-semibold text-overlay-outline">
        Relic Pick Overlay — outline
      </div>
    );
  }

  if (!payload) return null;

  const sorted = [...payload.relics]
    .sort((a, b) => scoreOf(b, priority) - scoreOf(a, priority))
    .slice(0, 3);
  const eraLabel = ERA_LABEL[payload.era] ?? payload.era;

  return (
    <div className={RPO_ROOT} ref={rootCallback}>
      <div className={RPO_HEADER}>
        <span className={RPO_TITLE}>{eraLabel} Fissure</span>
        <button className={RPO_CLOSE} onClick={hide} title="Close">✕</button>
      </div>

      {sorted.length === 0 ? (
        <div className={RPO_EMPTY}>No {eraLabel} relics in inventory</div>
      ) : (
        <div className={RPO_LIST}>
          {sorted.map((relic, i) => {
            const displayRewards = getDisplayRewards(relic, lines, priority);
            const score = scoreOf(relic, priority);
            const scoreLabel = priority === "platinum"
              ? `${score.toFixed(0)}p EV`
              : priority === "ducat"
              ? `${score.toFixed(0)}⬡ EV`
              : `${(score * 100).toFixed(0)}% new`;

            return (
              <div key={relic.name} className={i === 0 ? RPO_CARD_BEST : RPO_CARD}>
                <div className={RPO_CARD_HEADER}>
                  <span className={i === 0 ? RPO_RANK_BEST : RPO_RANK}>#{i + 1}</span>
                  <span className={RPO_RELIC_NAME}>{relic.base_name}</span>
                  <span className={REF_BADGE[relic.refinement] ?? REF_BADGE.intact}>
                    {relic.refinement.charAt(0).toUpperCase() + relic.refinement.slice(1, relic.refinement === "exceptional" ? 5 : 4)}.
                  </span>
                  <span className={RPO_COUNT}>×{relic.count}</span>
                  <span className={RPO_SCORE}>{scoreLabel}</span>
                </div>

                {lines === "estimated" ? (
                  <div className={RPO_ESTIMATED}>
                    <span>{relic.plat_score.toFixed(0)}</span><PlatIcon />
                    <span className={RPO_EST_SEP}> · </span>
                    <span>{relic.ducat_score.toFixed(0)}</span><DucatIcon />
                    <span className={RPO_EST_SEP}> · </span>
                    <span>{relic.rewards.filter(r => !r.owned).length}/{relic.rewards.length} new</span>
                  </div>
                ) : (
                  <div className={RPO_REWARDS}>
                    {displayRewards.map(reward => (
                      <div key={reward.name} className={RPO_REWARD}>
                        <span className={RPO_VAULT}>{reward.vaulted ? "🔒" : " "}</span>
                        <span className={reward.owned ? RPO_OWNED_YES : RPO_OWNED_NO}>
                          {reward.owned ? "✓" : "✗"}
                        </span>
                        <span className={RARITY_NAME[reward.rarity] ?? RPO_REWARD_NAME}>{reward.name}</span>
                        {reward.plat > 0 && (
                          <span className={RPO_PLAT_VAL}>
                            {reward.plat}<PlatIcon />
                          </span>
                        )}
                        {reward.ducats > 0 && (
                          <span className={RPO_DUCAT_VAL}>
                            {reward.ducats}<DucatIcon />
                          </span>
                        )}
                        <span className={RPO_REC[recRefinement(reward.rarity).toLowerCase()] ?? RPO_REC.intact}>
                          {recRefinement(reward.rarity)}
                        </span>
                      </div>
                    ))}
                  </div>
                )}
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}
