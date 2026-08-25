import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow, LogicalSize } from "@tauri-apps/api/window";
import { clampToMonitor, overlayScale } from "./uiScale";
import "./RelicPickOverlay.css";

type Priority = "unowned" | "ducat" | "platinum";
type Lines    = "all" | "best" | "estimated";

interface PickReward {
  name:      string;
  rarity:    string;   // "Bronze" | "Silver" | "Gold"
  drop_rate: number;
  ducats:    number;
  plat:      number;
  vaulted:   boolean;
  owned:     boolean;
}

interface PickRelic {
  name:          string;
  base_name:     string;
  refinement:    string;
  count:         number;
  unowned_score: number;
  ducat_score:   number;
  plat_score:    number;
  rewards:       PickReward[];
}

interface PickPayload {
  era:    string;
  relics: PickRelic[];
}

const ERA_LABEL: Record<string, string> = {
  LITH: "Lith", MESO: "Meso", NEO: "Neo", AXI: "Axi", ALL: "All Eras",
};

const RARITY_CLASS: Record<string, string> = {
  Bronze: "rpo-bronze", Silver: "rpo-silver", Gold: "rpo-gold",
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

function scoreOf(relic: PickRelic, priority: Priority): number {
  if (priority === "platinum") return relic.plat_score;
  if (priority === "ducat")    return relic.ducat_score;
  return relic.unowned_score;
}

function getDisplayRewards(relic: PickRelic, lines: Lines, priority: Priority): PickReward[] {
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
    style={{ objectFit: "contain", flexShrink: 0, verticalAlign: "middle", marginBottom: 1 }} />;
}
function DucatIcon() {
  return <img src="/ducats.webp" alt="d" width={11} height={11}
    style={{ objectFit: "contain", flexShrink: 0, verticalAlign: "middle", marginBottom: 1 }} />;
}

export default function RelicPickOverlay() {
  const [payload,  setPayload]  = useState<PickPayload | null>(null);
  const [priority, setPriority] = useState<Priority>("unowned");
  const [lines,    setLines]    = useState<Lines>("all");
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
    const ro = new ResizeObserver(entries => syncSize(Math.ceil(entries[0].contentRect.height)));
    ro.observe(el);
    roRef.current = ro;
  }, [syncSize]);

  const hide = () => {
    setPayload(null);
    getCurrentWindow().hide().catch(() => {});
  };

  useEffect(() => {
    const unOpen = listen<PickPayload>("relic-pick-open", async e => {
      // Reload settings fresh on every show — the main window may have changed them
      // since this overlay was first mounted at app startup.
      try {
        const json = await invoke<string>("load_settings");
        if (json) {
          const s = JSON.parse(json);
          if (["unowned","ducat","platinum"].includes(s.relicPickPriority)) setPriority(s.relicPickPriority);
          if (["all","best","estimated"].includes(s.relicPickLines))        setLines(s.relicPickLines);
        }
      } catch {}
      setPayload(e.payload);
    });
    const unClose = listen("relic-pick-close", () => hide());
    // A scale change does not alter the layout size, so the ResizeObserver never
    // fires. Measure again to resize a window that is already open.
    const unScale = listen("settings-updated", () => {
      const el = rootRef.current;
      if (el) syncSize(Math.ceil(el.getBoundingClientRect().height / overlayScale()));
    });
    return () => { unOpen.then(f => f()); unClose.then(f => f()); unScale.then(f => f()); };
  }, [syncSize]);

  if (!payload) return null;

  const sorted = [...payload.relics]
    .sort((a, b) => scoreOf(b, priority) - scoreOf(a, priority))
    .slice(0, 3);
  const eraLabel = ERA_LABEL[payload.era] ?? payload.era;

  return (
    <div className="rpo-root" ref={rootCallback}>
      <div className="rpo-header">
        <span className="rpo-title">{eraLabel} Fissure</span>
        <button className="rpo-close" onClick={hide} title="Close">✕</button>
      </div>

      {sorted.length === 0 ? (
        <div className="rpo-empty">No {eraLabel} relics in inventory</div>
      ) : (
        <div className="rpo-list">
          {sorted.map((relic, i) => {
            const displayRewards = getDisplayRewards(relic, lines, priority);
            const score = scoreOf(relic, priority);
            const scoreLabel = priority === "platinum"
              ? `${score.toFixed(0)}p EV`
              : priority === "ducat"
              ? `${score.toFixed(0)}⬡ EV`
              : `${(score * 100).toFixed(0)}% new`;

            return (
              <div key={relic.name} className={`rpo-card${i === 0 ? " best" : ""}`}>
                <div className="rpo-card-header">
                  <span className="rpo-rank">#{i + 1}</span>
                  <span className="rpo-relic-name">{relic.base_name}</span>
                  <span className={`rpo-ref-badge rpo-ref-${relic.refinement}`}>
                    {relic.refinement.charAt(0).toUpperCase() + relic.refinement.slice(1, relic.refinement === "exceptional" ? 5 : 4)}.
                  </span>
                  <span className="rpo-count">×{relic.count}</span>
                  <span className="rpo-score">{scoreLabel}</span>
                </div>

                {lines === "estimated" ? (
                  <div className="rpo-estimated">
                    <span>{relic.plat_score.toFixed(0)}</span><PlatIcon />
                    <span className="rpo-est-sep"> · </span>
                    <span>{relic.ducat_score.toFixed(0)}</span><DucatIcon />
                    <span className="rpo-est-sep"> · </span>
                    <span>{relic.rewards.filter(r => !r.owned).length}/{relic.rewards.length} new</span>
                  </div>
                ) : (
                  <div className="rpo-rewards">
                    {displayRewards.map(reward => (
                      <div key={reward.name} className={`rpo-reward ${RARITY_CLASS[reward.rarity] ?? ""}`}>
                        <span className="rpo-vault">{reward.vaulted ? "🔒" : " "}</span>
                        <span className={`rpo-owned-icon ${reward.owned ? "yes" : "no"}`}>
                          {reward.owned ? "✓" : "✗"}
                        </span>
                        <span className="rpo-reward-name">{reward.name}</span>
                        {reward.plat > 0 && (
                          <span className="rpo-plat-val">
                            {reward.plat}<PlatIcon />
                          </span>
                        )}
                        {reward.ducats > 0 && (
                          <span className="rpo-ducat-val">
                            {reward.ducats}<DucatIcon />
                          </span>
                        )}
                        <span className={`rpo-rec rpo-rec-${recRefinement(reward.rarity).toLowerCase()}`}>
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
