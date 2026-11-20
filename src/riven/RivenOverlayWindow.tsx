import { useState, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, emit } from "@tauri-apps/api/event";
import type { RivenAnalysis, RivenAnalysisUpdate, RivenStat } from "../types/rivens";
import { TAURI_COMMANDS, TAURI_EVENTS } from "../constants/tauri";
import type { SaveRivenRollArgs } from "../types/tauri";

// Tells App.tsx to run OCR again (for "Check New Roll" / "Start Comparison")
const triggerNewCheck = () => emit(TAURI_EVENTS.RIVEN_MANUAL_CHECK, {}).catch(() => {});

// Save current roll directly from overlay
async function saveOverlayRoll(
  weapon: string,
  stats: RivenStat[],
  verdict: string, score: number, rollCount: number
) {
  if (!weapon || stats.length === 0) return;
  const now = new Date();
  const label = `${weapon.charAt(0).toUpperCase() + weapon.slice(1)} · Roll #${rollCount} · ${now.getDate()} ${now.toLocaleString("en",{month:"short"})}`;
  const args: SaveRivenRollArgs = {
    weapon, label, statsJson: JSON.stringify(stats), verdict, score,
  };
  await invoke(TAURI_COMMANDS.SAVE_RIVEN_ROLL, args).catch(() => {});
  await emit(TAURI_EVENTS.RIVEN_ROLL_SAVED).catch(() => {});
}



// ── Tailwind class constants (formerly RivenOverlayWindow.css) ────────────────

const ROV_ROOT = "w-full h-full flex items-start justify-center p-2";
const ROV_CARD =
  "bg-background/93 border border-accent/30 rounded-10 px-3.5 py-3 w-full flex flex-col gap-2 backdrop-blur-10 shadow-[0_8px_32px_rgba(0,0,0,0.65)]";

const ROV_HEADER = "flex items-center gap-1.5";
const ROV_TITLE = "text-14 font-bold text-foreground flex-1";
const ROV_COMPARE =
  "bg-accent/15 border border-accent/35 text-info text-10 font-semibold cursor-pointer px-1.75 py-0.5 rounded-4 leading-1.4 transition-[background] duration-150 hover:bg-accent/28";
const ROV_SAVE =
  "bg-transparent border-0 text-muted/60 text-12 cursor-pointer px-1 py-0.5 rounded-4 leading-none transition-colors duration-150 hover:text-success";
const ROV_CLOSE =
  "bg-transparent border-0 text-muted/50 text-12 cursor-pointer px-1 py-0.5 rounded-4 leading-none transition-[color,background] duration-150 hover:text-danger hover:bg-danger/12";

const ROV_SCANNING = "text-12 text-muted/80 text-center py-1.5";
const ROV_VERDICT = "text-13 font-bold tracking-[0.3px]";

const ROV_SCORE_WRAP = "flex items-center gap-1.75";
const ROV_SCORE_TRACK =
  "flex-1 h-1.25 bg-white/8 rounded-3 overflow-hidden";
const ROV_SCORE_FILL = "h-full rounded-3 transition-[width] duration-400 ease-[ease]";
const ROV_SCORE_PCT = "text-11 font-semibold min-w-7.5 text-right";

const ROV_ROLLED =
  "flex flex-col gap-1 border-t border-t-white/6 pt-2";
const ROV_ROW = "flex items-center gap-1.75 text-12 px-1.5 py-0.75 rounded-5";
const ROV_ICON = "text-11 w-3.25 text-center shrink-0";
const ROV_NAME = "flex-1 text-foreground";
const ROV_VALUE = "text-12 font-semibold tabular-nums shrink-0";
const STAT_TONE: Record<string, { row: string; icon: string; name: string; value: string }> = {
  wanted: {
    row: ROV_ROW + " bg-success/10",
    icon: ROV_ICON + " text-success",
    name: ROV_NAME,
    value: ROV_VALUE + " text-success",
  },
  neutral: {
    row: ROV_ROW + " bg-white/4",
    icon: ROV_ICON + " text-muted/60",
    name: "flex-1 text-foreground/70",
    value: ROV_VALUE + " text-ducat",
  },
  safe_neg: {
    row: ROV_ROW + " bg-accent/8",
    icon: ROV_ICON + " text-info",
    name: ROV_NAME,
    value: ROV_VALUE + " text-info",
  },
  harmful: {
    row: ROV_ROW + " bg-danger/8",
    icon: ROV_ICON + " text-danger",
    name: ROV_NAME,
    value: ROV_VALUE + " text-danger",
  },
};

const ROV_ORIGINAL = "border-t border-t-white/6 pt-1.5";
const ROV_SECTION_LABEL =
  "text-10 font-semibold text-muted/55 uppercase tracking-[0.5px] mb-1";
const ROV_ALT_CARD =
  "border-t border-t-white/6 pt-1.5 first:border-t-0 first:pt-0";
const ROV_ALT_LABEL =
  "text-9 font-bold uppercase tracking-wider text-muted/55 bg-white/6 rounded-3 px-1.25 py-0.25 inline-block mb-0.75";
const ROV_MISSING =
  "text-10.5 text-muted/65 border-t border-t-white/6 pt-1.5 leading-1.6 break-words";
const ROV_MISSING_LABEL = "font-semibold";
const ROV_MISSING_STAT = "text-muted/90";
const ROV_NOTES =
  "text-10 text-muted/70 italic border-t border-t-white/6 pt-1.25";

// No auto-hide — user dismisses with ✕ or the poll detects screen closure.
// Only a very long emergency fallback (60 min) in case everything else fails.

// Ask App.tsx to hide this overlay — App.tsx owns the rivenWin reference
// Tell App.tsx to hide the overlay AND log the reason BEFORE hiding
const requestHide = (reason: string) => {
  emit(TAURI_EVENTS.RIVEN_OVERLAY_HIDE, { reason }).catch(() => {});
};

function verdictColor(verdict: string): string {
  if (verdict.startsWith("GREAT"))    return "var(--color-success)";
  if (verdict.startsWith("GOOD"))     return "#a8d8a8";
  if (verdict.startsWith("MEDIOCRE")) return "#f0c040";
  return "var(--color-danger)";
}

function ScoreBar({ score }: { score: number }) {
  const pct = Math.round(score * 100);
  const color = score >= 0.8 ? "var(--color-success)" : score >= 0.6 ? "#a8d8a8" : score >= 0.4 ? "#f0c040" : "var(--color-danger)";
  return (
    <div className={ROV_SCORE_WRAP}>
      <div className={ROV_SCORE_TRACK}>
        <div className={ROV_SCORE_FILL} style={{ width: `${pct}%`, background: color }} />
      </div>
      <span className={ROV_SCORE_PCT} style={{ color }}>{pct}%</span>
    </div>
  );
}

export default function RivenOverlayWindow() {
  const [analysis, setAnalysis]         = useState<RivenAnalysis | null>(null);
  const [rolledStats, setRolledStats]   = useState<RivenStat[]>([]);
  const [originalStats, setOriginalStats] = useState<RivenStat[]>([]);
  const [isComparison, setIsComparison] = useState(false);
  const [ocrRaw, setOcrRaw]             = useState("");
  const [parsedWeapon, setParsedWeapon] = useState("");
  const [rollCount, setRollCount]       = useState(0);
  const [scanning, setScanning]         = useState(true);
  const [saved, setSaved]               = useState(false);
  const scanTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const sourceRef = useRef<"idle" | "preview" | "live">("idle");

  const resetToScanning = (source: "preview" | "live") => {
    sourceRef.current = source;
    setScanning(true);
    setAnalysis(null);
    setRolledStats([]);
    setOriginalStats([]);
    setIsComparison(false);
    setOcrRaw("");
    setParsedWeapon("");
    if (scanTimerRef.current) clearTimeout(scanTimerRef.current);
    // 60-min emergency fallback — only fires if poll completely breaks
    scanTimerRef.current = setTimeout(() => requestHide("emergency-60min"), 3_600_000);
  };

  useEffect(() => {
    const unlistenStart = listen(TAURI_EVENTS.RIVEN_SCANNING_START, () => resetToScanning("live"));
    const unlistenPreviewStart = listen(TAURI_EVENTS.RIVEN_PREVIEW_START, () => {
      if (sourceRef.current !== "live") resetToScanning("preview");
    });

    const unlistenUpdate = listen<RivenAnalysisUpdate>(TAURI_EVENTS.RIVEN_ANALYSIS_UPDATE, e => {
      if (e.payload.dummy) {
        if (sourceRef.current !== "preview") return;
      } else {
        sourceRef.current = "live";
      }
      if (scanTimerRef.current) clearTimeout(scanTimerRef.current);
      setAnalysis(e.payload.analysis ?? null);
      setRollCount(e.payload.rollCount);
      setOcrRaw(e.payload.ocrRaw ?? "");
      setParsedWeapon(e.payload.weapon ?? "");
      setRolledStats(e.payload.rolledStats ?? []);
      setOriginalStats(e.payload.originalStats ?? []);
      setIsComparison(e.payload.isComparison ?? false);
      setScanning(false);
      // Reset emergency fallback timer — 60 min from last data shown
      scanTimerRef.current = setTimeout(() => requestHide("emergency-60min"), 3_600_000);
    });
    const unlistenPreviewClose = listen(TAURI_EVENTS.RIVEN_PREVIEW_CLOSE, () => {
      if (sourceRef.current !== "live") requestHide("preview-close");
    });

    // Tell App.tsx the listener is registered and the pending payload can be sent now.
    emit(TAURI_EVENTS.RIVEN_WINDOW_READY, {}).catch(() => {});

    // Initial hide fallback — same as resetToScanning's timer
    scanTimerRef.current = setTimeout(() => requestHide("emergency-60min"), 3_600_000);

    return () => {
      unlistenStart.then(fn => fn());
      unlistenPreviewStart.then(fn => fn());
      unlistenUpdate.then(fn => fn());
      unlistenPreviewClose.then(fn => fn());
      if (scanTimerRef.current) clearTimeout(scanTimerRef.current);
    };
  }, []); // eslint-disable-line

  const weaponName = analysis?.weapon ?? parsedWeapon;
  const displayName = weaponName
    ? weaponName.split(" ").map(w => w.charAt(0).toUpperCase() + w.slice(1)).join(" ")
    : "Riven Analyzer";

  // All wanted stat names — matched (in new roll) + missing (from groups like "MS / TOX / DMG")
  const allWantedNames = new Set<string>();
  if (analysis) {
    analysis.matched_positives.forEach(s => allWantedNames.add(s));
    analysis.missing_positives.forEach(group =>
      group.split(" / ").forEach(s => allWantedNames.add(s.trim()))
    );
  }

  // Classify a rolled stat against the analysis
  const classifyStat = (stat: RivenStat): "wanted" | "neutral" | "safe_neg" | "harmful" => {
    if (!analysis) return "neutral";
    if (!stat.positive) {
      return analysis.safe_negatives_present.includes(stat.name) ? "safe_neg" : "harmful";
    }
    return analysis.matched_positives.includes(stat.name) ? "wanted" : "neutral";
  };

  // Classify an original-roll stat — same logic but uses the full wanted list
  const classifyOriginalStat = (stat: RivenStat): "wanted" | "neutral" | "safe_neg" | "harmful" => {
    if (!analysis) return "neutral";
    if (!stat.positive) {
      // For original negatives, mark as safe if in the weapon's safe list (same DB)
      return analysis.safe_negatives_present.includes(stat.name) ? "safe_neg" : "neutral";
    }
    return allWantedNames.has(stat.name) ? "wanted" : "neutral";
  };

  const statIcon = (cls: "wanted" | "neutral" | "safe_neg" | "harmful") => {
    if (cls === "wanted")   return "✓";
    if (cls === "safe_neg") return "✓";
    if (cls === "harmful")  return "✗";
    return "○";
  };

  return (
    <div className={ROV_ROOT}>
      <div className={ROV_CARD}>
        {/* Header */}
        <div className={ROV_HEADER}>
          <span className={ROV_TITLE}>{displayName}</span>
          <button className={ROV_COMPARE} onClick={() => triggerNewCheck()} title="Re-scan (use after cycling for comparison)">
            {isComparison ? "🔄 Refresh" : "⚡ New Roll"}
          </button>
          {rolledStats.length > 0 && (
            <button className={ROV_SAVE} title="Save this roll"
              onClick={async () => {
                await saveOverlayRoll(weaponName, rolledStats, analysis?.verdict ?? "", analysis?.score ?? 0, rollCount);
                setSaved(true); setTimeout(() => setSaved(false), 2000);
              }}>
              {saved ? "✓" : "💾"}
            </button>
          )}
          <button className={ROV_CLOSE} onClick={() => requestHide("x-button")} title="Dismiss">✕</button>
        </div>

        {/* Scanning */}
        {scanning && (
          <div className={ROV_SCANNING}>Scanning stats…</div>
        )}

        {/* No result */}
        {!scanning && rolledStats.length === 0 && !analysis && (
          <div className={ROV_SCANNING + " !text-danger"}>
            Could not read card stats
            {parsedWeapon && <div className="mt-0.75 text-10 text-muted/70">Weapon: "{parsedWeapon}"</div>}
          </div>
        )}

        {/* Result */}
        {!scanning && (rolledStats.length > 0 || analysis) && (
          <>
            {/* One analysis card per build alternative */}
            {analysis && analysis.alternatives.map((alt, i) => (
              <div key={i} className={ROV_ALT_CARD}>
                {analysis.alternatives.length > 1 && (
                  <span className={ROV_ALT_LABEL}>{alt.label}</span>
                )}
                <div className={ROV_VERDICT} style={{ color: verdictColor(alt.verdict) }}>
                  {alt.verdict}
                </div>
                <ScoreBar score={alt.score} />
                {/* Negatives shown once on first card */}
                {i === 0 && analysis.safe_negatives_present.map(s => (
                  <div key={s} className={STAT_TONE.safe_neg.row}>
                    <span className={STAT_TONE.safe_neg.icon}>✓</span>
                    <span className={STAT_TONE.safe_neg.name}>−{s}</span>
                    <span className={STAT_TONE.safe_neg.value + " !text-10"}>Safe</span>
                  </div>
                ))}
                {i === 0 && analysis.harmful_negatives.map(s => (
                  <div key={s} className={STAT_TONE.harmful.row}>
                    <span className={STAT_TONE.harmful.icon}>✗</span>
                    <span className={STAT_TONE.harmful.name}>−{s}</span>
                    <span className={STAT_TONE.harmful.value + " !text-10"}>Harmful</span>
                  </div>
                ))}
                {alt.missing.length > 0 && (
                  <div className={ROV_MISSING}>
                    <span className={ROV_MISSING_LABEL}>Wanted: </span>
                    {alt.missing.map((s, j) => (
                      <span key={s} className={ROV_MISSING_STAT}>
                        {s}{j < alt.missing.length - 1 ? ", " : ""}
                      </span>
                    ))}
                  </div>
                )}
              </div>
            ))}

            {/* Rolled stats — what's actually on the card */}
            {rolledStats.length > 0 && (
              <div className={ROV_ROLLED}>
              {isComparison && <div className={ROV_SECTION_LABEL}>New roll</div>}
                {rolledStats.map((stat, i) => {
                  const cls = classifyStat(stat);
                  const tone = STAT_TONE[cls];
                  return (
                    <div key={i} className={tone.row}>
                      <span className={tone.icon}>{statIcon(cls)}</span>
                      <span className={tone.name}>{stat.name}</span>
                      <span className={tone.value}>{stat.value}</span>
                    </div>
                  );
                })}
              </div>
            )}

            {/* Original roll (comparison mode only) — same quality colors as new roll */}
            {isComparison && originalStats.length > 0 && (
              <div className={ROV_ORIGINAL}>
                <div className={ROV_SECTION_LABEL}>Original roll</div>
                <div className={ROV_ROLLED}>
                  {originalStats.map((stat, i) => {
                    const cls = classifyOriginalStat(stat);
                    const tone = STAT_TONE[cls];
                    return (
                      <div key={i} className={tone.row + " opacity-75"}>
                        <span className={tone.icon}>{statIcon(cls)}</span>
                        <span className={tone.name}>{stat.name}</span>
                        <span className={tone.value}>{stat.value}</span>
                      </div>
                    );
                  })}
                </div>
              </div>
            )}


            {/* No DB entry */}
            {!analysis && rolledStats.length > 0 && (
              <div className={ROV_MISSING + " !text-muted/60"}>
                No database entry for {displayName}
              </div>
            )}

            {analysis?.notes && (
              <div className={ROV_NOTES}>ℹ {analysis.notes}</div>
            )}

            {/* Raw OCR fallback */}
            {!analysis && ocrRaw && (
              <details className="mt-1">
                <summary className="cursor-pointer text-10 text-muted/50">Raw OCR</summary>
                <pre className="mt-0.75 max-h-25 overflow-y-auto whitespace-pre-wrap text-9 text-muted/60">{ocrRaw}</pre>
              </details>
            )}
          </>
        )}
      </div>
    </div>
  );
}
