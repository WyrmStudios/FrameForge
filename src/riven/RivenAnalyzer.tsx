import { useState, useEffect, useCallback, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { checkRivenNow } from "../lib/rivenWindow";
import { TAURI_COMMANDS, TAURI_EVENTS } from "../constants/tauri";
import type { RivenAnalysis, RivenStat, SavedRiven } from "../types/rivens";
import type { AnalyzeRivenArgs, SaveRivenRollArgs } from "../types/tauri";

// ── Tailwind class constants (formerly RivenAnalyzer.css) ─────────────────────

const RA_ANALYZER =
  "flex flex-col gap-2.5 px-3.5 py-3 h-full min-h-0 overflow-y-auto";
const RA_HEADER = "flex items-center gap-2 shrink-0";
const RA_TITLE = "text-13 font-bold text-foreground";
const RA_DB_STATUS = "ml-auto text-10 text-muted";
const RA_CREDIT =
  "bg-transparent border-0 p-0 text-10 text-muted cursor-pointer opacity-60 transition-[opacity,color] duration-100 whitespace-nowrap hover:opacity-100 hover:text-accent";
const RA_CHECK_BTN =
  "bg-accent/15 border border-accent/40 text-info text-12 font-semibold cursor-pointer px-2.5 py-1 rounded-5 transition-[background] duration-150 hover:bg-accent/28";
const RA_REFRESH_BTN =
  "bg-transparent border-0 text-muted text-14 cursor-pointer px-0.5 py-0 transition-[color] duration-100 hover:text-foreground";

const RA_WEAPON_WRAP = "relative shrink-0";
const RA_WEAPON_INPUT =
  "w-full bg-black/20 border border-border/80 rounded-5 text-foreground text-13 font-semibold px-2.5 py-1.75 outline-none focus:border-accent";
const RA_SUGGESTIONS =
  "absolute top-full left-0 right-0 bg-surface border border-t-0 border-border/80 rounded-b-5 z-10 max-h-55 overflow-y-auto";
const RA_SUGGESTION =
  "px-2.5 py-1.5 text-12 text-foreground cursor-pointer transition-[background] duration-100 hover:bg-accent/12";

const RA_SECTION_LABEL =
  "text-10 font-bold uppercase tracking-0.04 text-muted shrink-0";
const RA_OPTIONAL = "font-normal normal-case tracking-normal italic";
const RA_STAT_GRID = "flex flex-wrap gap-1 shrink-0";
const RA_STAT_BTN =
  "bg-white/5 border border-border/60 text-muted text-11 px-2.25 py-0.75 rounded-4 cursor-pointer whitespace-nowrap transition-[background,color,border-color] duration-100 hover:bg-white/10 hover:text-foreground";
const RA_STAT_BTN_SELECTED =
  "bg-white/5 border border-border/60 text-muted text-11 px-2.25 py-0.75 rounded-4 cursor-pointer whitespace-nowrap transition-[background,color,border-color] duration-100 hover:bg-white/10 hover:text-foreground bg-success/15! border-[var(--green)]! text-success!";

const RA_VERDICT = "text-16 font-bold tracking-0.01";
const RA_STATS_BREAKDOWN = "flex flex-col gap-0.75";
const RA_STAT_ROW = "flex items-center gap-2 text-12 py-0.75";
const RA_STAT_ICON = "w-3.5 text-center shrink-0 text-11";
const RA_STAT_TAG =
  "ml-auto text-10 px-1.5 py-0.25 rounded-3 shrink-0";
const STAT_TONE: Record<string, { row: string; tag: string }> = {
  good: {
    row: RA_STAT_ROW + " text-success",
    tag: RA_STAT_TAG + " bg-success/12 text-success",
  },
  miss: {
    row: RA_STAT_ROW + " text-muted",
    tag: RA_STAT_TAG + " bg-white/6 text-muted",
  },
  safe: {
    row: RA_STAT_ROW + " text-state-cool",
    tag: RA_STAT_TAG + " bg-state-cool/12 text-state-cool",
  },
  bad: {
    row: RA_STAT_ROW + " text-danger",
    tag: RA_STAT_TAG + " bg-danger/12 text-danger",
  },
};

const RA_NOTES =
  "text-11 text-muted leading-normal border-t border-t-border/40 pt-2";
const RA_NEXT_ROLL =
  "bg-accent/12 border border-accent text-accent text-12 font-semibold px-4 py-1.75 rounded-5 cursor-pointer self-start transition-[background] duration-100 shrink-0 hover:bg-accent/25";

const RA_VALUE_INPUTS =
  "bg-black/20 border border-border/50 rounded-6 p-2.5 flex flex-col gap-1.5 shrink-0";
const RA_VALUE_ROW = "flex items-center gap-2";
const RA_VALUE_LABEL = "flex-1 text-12 text-foreground min-w-40";
const RA_VALUE_INPUT =
  "w-18 bg-black/30 border border-border/60 rounded-4 px-1.5 py-0.75 text-foreground text-12 text-right focus:outline-none focus:border-accent";
const RA_SAVE_BTN =
  "bg-accent/15 border border-accent/40 text-accent text-11 font-semibold px-3 py-1 rounded-4 cursor-pointer transition-[background] duration-100 hover:bg-accent/28";
const RA_CANCEL_EDIT =
  "bg-transparent border border-border/60 rounded-4 text-muted text-11 px-2 py-1 cursor-pointer transition-[border-color] duration-100 hover:border-danger hover:text-danger";

const RA_SAVED_SECTION =
  "border-t border-t-border/50 pt-3 shrink-0";
const RA_SAVED_GRID =
  "grid grid-cols-[repeat(auto-fill,minmax(200px,1fr))] gap-2";
const RA_SAVED_CARD =
  "bg-black/25 border border-border/50 rounded-7 px-2.5 py-2.25 flex flex-col gap-0.75 transition-[border-color] duration-100 hover:border-border/90";
const RA_SAVED_CARD_SEL =
  "bg-black/25 border border-border/50 rounded-7 px-2.5 py-2.25 flex flex-col gap-0.75 transition-[border-color] duration-100 hover:border-border/90 border-accent! bg-accent/6!";
const RA_SAVED_HEADER = "flex items-center gap-1 mb-0.75";
const RA_LABEL_INPUT =
  "flex-1 bg-transparent border-0 border-b border-b-transparent text-foreground text-11 font-semibold px-0.5 py-0 min-w-0 focus:outline-none focus:border-b-accent";
const RA_SAVED_ACTIONS = "flex gap-0.75 shrink-0";
const RA_CMP_BASE =
  "bg-transparent border border-border/60 rounded-3 text-muted text-10 w-5 h-5 cursor-pointer flex items-center justify-center transition-all duration-100 hover:border-accent hover:text-accent";
const RA_CMP_ACTIVE =
  "bg-transparent border border-border/60 rounded-3 text-muted text-10 w-5 h-5 cursor-pointer flex items-center justify-center transition-all duration-100 hover:border-accent hover:text-accent bg-accent/20! border-accent! text-accent!";
const RA_DELETE_BTN =
  "bg-transparent border border-border/60 rounded-3 text-muted text-10 w-5 h-5 cursor-pointer transition-all duration-100 hover:border-danger hover:text-danger";
const RA_EDIT_BTN =
  "bg-transparent border border-border/60 rounded-3 text-muted text-11 w-5 h-5 cursor-pointer transition-all duration-100 hover:border-accent hover:text-accent";
const RA_SAVED_STATS = "flex flex-col gap-0.5";
const RA_SAVED_STAT = "text-11 text-muted flex gap-1";

const RA_COMPARE_PANEL =
  "mt-3 bg-black/20 border border-accent/30 rounded-7 px-3 py-2.5";
const RA_COMPARE_GRID = "grid grid-cols-2 gap-3";
const RA_COMPARE_COL = "flex flex-col gap-0.75";
const RA_COMPARE_LABEL =
  "text-11 font-bold text-foreground mb-1 pb-1 border-b border-b-border/40";

const RA_SIGN_BASE =
  "min-w-5.5 h-5.5 rounded-4 border text-13 font-bold cursor-pointer shrink-0 transition-all duration-100";
const RA_SIGN_POS =
  RA_SIGN_BASE +
  " bg-success/15 border-success/50 text-success hover:bg-success/30";
const RA_SIGN_NEG =
  RA_SIGN_BASE +
  " bg-danger/12 border-danger/50 text-danger hover:bg-danger/25";
const RA_FMT_BTN =
  "min-w-6 h-5.5 bg-white/6 border border-border/70 rounded-4 text-muted text-11 font-semibold cursor-pointer shrink-0 transition-[background] duration-100 hover:bg-white/12";

const RA_ALTERNATIVES = "flex flex-col gap-2";
const RA_ALT_CARD =
  "bg-black/20 border border-border/50 rounded-7 px-3 py-2.5 flex flex-col gap-1.5";
const RA_ALT_HEADER = "flex items-center gap-2 flex-wrap";
const RA_ALT_LABEL =
  "text-10 font-bold uppercase tracking-wider text-muted bg-white/6 rounded-3 px-1.5 py-0.5";

// ── Types ─────────────────────────────────────────────────────────────────────

function verdictColor2(v: string) {
  if (v.startsWith("GREAT")) return "var(--green)";
  if (v.startsWith("GOOD"))  return "#a8d8a8";
  if (v.startsWith("MED"))   return "#f0c040";
  return "var(--red)";
}

// All riven stats in one list — sign (+/-) is set per-roll by the user
const ALL_STATS = [
  "Critical Damage", "Critical Chance", "Multishot", "Base Damage",
  "Fire Rate", "Status Chance", "Toxicity", "Heat", "Electricity",
  "Cold", "Punch Through", "Reload Speed", "Magazine Size",
  "Projectile Flight Speed", "Status Duration",
  "Damage to Infested", "Damage to Grineer", "Damage to Corpus",
  "Attack Speed", "Range", "Combo Count Chance", "Initial Combo",
  "Heavy Attack Efficiency", "Slide Critical Chance",
  "Zoom", "Recoil", "Puncture", "Impact", "Slash", "Ammo Maximum",
];


// ── Verdict colour helper ─────────────────────────────────────────────────────

function verdictColor(verdict: string): string {
  if (verdict.startsWith("GREAT"))    return "var(--green)";
  if (verdict.startsWith("GOOD"))     return "#a8d8a8";
  if (verdict.startsWith("MEDIOCRE")) return "#f0c040";
  return "var(--red)";
}

// ── Stat score bar ────────────────────────────────────────────────────────────

function ScoreBar({ score }: { score: number }) {
  const pct = Math.round(score * 100);
  const color = score >= 0.8 ? "var(--green)" : score >= 0.6 ? "#a8d8a8" : score >= 0.4 ? "#f0c040" : "var(--red)";
  return (
    <div className="flex items-center gap-2 h-1.5 bg-white/8 rounded-3 overflow-visible relative">
      <div
        className="h-1.5 rounded-3 transition-[width,background] duration-300 min-w-1"
        style={{ width: `${pct}%`, background: color }}
      />
      <span
        className="text-11 font-bold absolute right-0 -top-0.5 tabular-nums"
        style={{ color }}
      >
        {pct}%
      </span>
    </div>
  );
}

// ── Main component ────────────────────────────────────────────────────────────

export default function RivenAnalyzer() {
  const [weapons, setWeapons]         = useState<string[]>([]);
  const [weaponInput, setWeaponInput] = useState("");
  const [filtered, setFiltered]       = useState<string[]>([]);
  const [selectedWeapon, setSelectedWeapon] = useState("");
  const [analysis, setAnalysis]       = useState<RivenAnalysis | null>(null);
  const [_rollCount, setRollCount]     = useState(0);

  // Unified stat builder: each stat has a name, value, sign, and format
  const [builtStats, setBuiltStats]   = useState<RivenStat[]>([]);
  // editingId: if set, the save button becomes "Update" and targets this saved roll
  const [editingId, setEditingId]     = useState<string | null>(null);

  // Inline card edit state
  const [inlineEditId, setInlineEditId]       = useState<string | null>(null);
  const [inlineEditLabel, setInlineEditLabel] = useState("");
  const [inlineEditStats, setInlineEditStats] = useState<RivenStat[]>([]);

  // Derive positives/negatives for the analysis call
  const positives = builtStats.filter(s => s.positive).map(s => s.name);
  const negative  = builtStats.find(s => !s.positive)?.name ?? "";

  const toggleStat = (name: string) => {
    setBuiltStats(prev => {
      const exists = prev.find(s => s.name === name);
      if (exists) return prev.filter(s => s.name !== name);
      // Damage-to stats default to × multiplier format
      const useMultiplier = name.startsWith("Damage to");
      return [...prev, { name, value: "", positive: true, useMultiplier }];
    });
  };

  const updateStatValue = (name: string, value: string) =>
    setBuiltStats(prev => prev.map(s => s.name === name ? { ...s, value } : s));

  const toggleStatSign = (name: string) =>
    setBuiltStats(prev => prev.map(s => s.name === name ? { ...s, positive: !s.positive } : s));

  const toggleStatFormat = (name: string) =>
    setBuiltStats(prev => prev.map(s =>
      s.name === name ? { ...s, useMultiplier: !s.useMultiplier } : s
    ));

  const [dbStatus, setDbStatus]       = useState("");
  const [showLog, setShowLog]         = useState(false);
  const [sessionLog, setSessionLog]   = useState("");
  const inputRef = useRef<HTMLInputElement>(null);

  // ── Saved rivens ────────────────────────────────────────────────────────────
  const [savedRivens, setSavedRivens] = useState<SavedRiven[]>([]);
  const [saveStatus, setSaveStatus]   = useState("");
  // Comparison: set of selected ids
  const [compareIds, setCompareIds]   = useState<Set<string>>(new Set());
  // Inline rename: id → draft label
  const [renameDraft, setRenameDraft] = useState<Record<string, string>>({});

  const loadSavedRivens = useCallback(async () => {
    const list = await invoke<SavedRiven[]>("get_saved_riven_rolls").catch(() => []);
    setSavedRivens(list);
  }, []);

  useEffect(() => { loadSavedRivens(); }, [loadSavedRivens]);

  const saveCurrentRoll = async () => {
    if (!selectedWeapon) return;
    const stats = builtStats.filter(s => s.value.trim() !== "");
    if (stats.length === 0) { setSaveStatus("Add stat values before saving."); return; }
    const now = new Date();
    try {
      if (editingId) {
        // Update existing roll
        await invoke("delete_saved_riven_roll", { id: editingId });
        const args: SaveRivenRollArgs = {
          weapon: selectedWeapon,
          label: savedRivens.find(r => r.id === editingId)?.label ?? `${selectedWeapon.charAt(0).toUpperCase() + selectedWeapon.slice(1)} · ${now.getDate()} ${now.toLocaleString("en",{month:"short"})}`,
          statsJson: JSON.stringify(stats),
          verdict: analysis?.verdict ?? "", score: analysis?.score ?? 0,
        };
        await invoke(TAURI_COMMANDS.SAVE_RIVEN_ROLL, args);
        setEditingId(null);
        setSaveStatus("Updated!");
      } else {
        const label = `${selectedWeapon.charAt(0).toUpperCase() + selectedWeapon.slice(1)} · ${now.getDate()} ${now.toLocaleString("en",{month:"short"})} ${now.getFullYear()}`;
        const args: SaveRivenRollArgs = {
          weapon: selectedWeapon, label, statsJson: JSON.stringify(stats),
          verdict: analysis?.verdict ?? "", score: analysis?.score ?? 0,
        };
        await invoke(TAURI_COMMANDS.SAVE_RIVEN_ROLL, args);
        setSaveStatus("Saved!");
      }
      loadSavedRivens();
      setTimeout(() => setSaveStatus(""), 2000);
    } catch (e: unknown) { setSaveStatus(String(e)); }
  };

  const startInlineEdit = (r: SavedRiven) => {
    const stats: RivenStat[] = (() => { try { return JSON.parse(r.stats_json); } catch { return []; } })();
    setInlineEditId(r.id);
    setInlineEditLabel(r.label);
    setInlineEditStats(stats);
  };

  const saveInlineEdit = async (r: SavedRiven) => {
    const stats = inlineEditStats.filter(s => s.value.trim() !== "");
    try {
      await invoke("delete_saved_riven_roll", { id: r.id });
      const args: SaveRivenRollArgs = {
        weapon: r.weapon,
        label: inlineEditLabel,
        statsJson: JSON.stringify(stats),
        verdict: r.verdict,
        score: r.score,
      };
      await invoke(TAURI_COMMANDS.SAVE_RIVEN_ROLL, args);
      loadSavedRivens();
      setInlineEditId(null);
    } catch {}
  };

  const deleteSaved = async (id: string) => {
    await invoke("delete_saved_riven_roll", { id }).catch(() => {});
    setSavedRivens(prev => prev.filter(r => r.id !== id));
    setCompareIds(prev => { const s = new Set(prev); s.delete(id); return s; });
  };

  const applyRename = async (id: string) => {
    const label = renameDraft[id]?.trim();
    if (!label) return;
    await invoke("rename_saved_riven_roll", { id, label }).catch(() => {});
    setSavedRivens(prev => prev.map(r => r.id === id ? { ...r, label } : r));
    setRenameDraft(prev => { const d = { ...prev }; delete d[id]; return d; });
  };

  const toggleCompare = (id: string) => {
    setCompareIds(prev => {
      const s = new Set(prev);
      if (s.has(id)) { s.delete(id); } else if (s.size < 2) { s.add(id); }
      return s;
    });
  };

  const compareList = savedRivens.filter(r => compareIds.has(r.id));

  // Load weapons list on mount
  useEffect(() => {
    invoke<string[]>("get_riven_weapons")
      .then(w => { setWeapons(w); setDbStatus(`${w.length} weapons loaded`); })
      .catch(() => setDbStatus("Failed to load database — click Refresh"));
  }, []);

  // Filter weapon suggestions
  useEffect(() => {
    if (!weaponInput.trim()) { setFiltered([]); return; }
    const q = weaponInput.toLowerCase();
    setFiltered(weapons.filter(w => w.includes(q)).slice(0, 8));
  }, [weaponInput, weapons]);

  // Listen for EE.log riven events
  useEffect(() => {
    const unlistenUnveil  = listen(TAURI_EVENTS.RIVEN_UNVEILED, () => {
      setRollCount(0);
      inputRef.current?.focus();
    });
    const unlistenSaved = listen(TAURI_EVENTS.RIVEN_ROLL_SAVED, () => loadSavedRivens());
    return () => {
      unlistenUnveil.then(fn => fn());
      unlistenSaved.then(fn => fn());
    };
  }, [selectedWeapon, positives, negative]); // eslint-disable-line

  const selectWeapon = (w: string) => {
    setSelectedWeapon(w);
    setWeaponInput(w.charAt(0).toUpperCase() + w.slice(1));
    setFiltered([]);
    setBuiltStats([]);
    setAnalysis(null);
    setRollCount(0);
    setEditingId(null);
  };

  const runAnalysis = useCallback(async () => {
    if (!selectedWeapon || builtStats.length === 0) { setAnalysis(null); return; }
    const args: AnalyzeRivenArgs = {
      weapon: selectedWeapon,
      positives: builtStats.filter(s => s.positive).map(s => s.name),
      negatives: builtStats.filter(s => !s.positive).map(s => s.name),
    };
    const result = await invoke<RivenAnalysis | null>(TAURI_COMMANDS.ANALYZE_RIVEN, { ...args });
    setAnalysis(result ?? null);
  }, [selectedWeapon, builtStats]);

  useEffect(() => { if (selectedWeapon) runAnalysis(); }, [builtStats, runAnalysis]);

  const reset = () => {
    setBuiltStats([]);
    setAnalysis(null);
    setEditingId(null);
    setRollCount(c => c + 1);
  };

  const reloadDb = async () => {
    setDbStatus("Reloading…");
    try {
      const count = await invoke<number>("reload_riven_database");
      const w = await invoke<string[]>("get_riven_weapons");
      setWeapons(w);
      setDbStatus(`${count} weapons loaded`);
    } catch { setDbStatus("Reload failed"); }
  };

  return (
    <div className={RA_ANALYZER}>
      {/* Header */}
      <div className={RA_HEADER}>
        <span className={RA_TITLE}>Riven Analyzer</span>
        <button
          className={RA_CHECK_BTN}
          onClick={() => checkRivenNow()}
          title="Capture current riven card from Warframe screen"
        >
          🔍 Check Riven
        </button>
        <span className={RA_DB_STATUS}>{dbStatus}</span>
        <button
          className={RA_CREDIT}
          title="Open Riven price database on Google Sheets"
          onClick={() => invoke(TAURI_COMMANDS.OPEN_URL, { url: "https://docs.google.com/spreadsheets/d/1zbaeJBuBn44cbVKzJins_E3hTDpnmvOk8heYN-G8yy8" }).catch(() => {})}
        >data by 44bananas ↗</button>
        <button className={RA_REFRESH_BTN} onClick={reloadDb} title="Reload database from Google Sheet">↻</button>
        <button className={RA_REFRESH_BTN} title="View session log" onClick={async () => {
          const log = await invoke<string>("get_riven_session_log").catch(() => "Log unavailable");
          setSessionLog(log);
          setShowLog(v => !v);
        }}>📋</button>
      </div>

      {/* Weapon search */}
      <div className={RA_WEAPON_WRAP}>
        <input
          ref={inputRef}
          className={RA_WEAPON_INPUT}
          placeholder="Type weapon name…"
          value={weaponInput}
          onChange={e => { setWeaponInput(e.target.value); setSelectedWeapon(""); setAnalysis(null); }}
        />
        {filtered.length > 0 && (
          <div className={RA_SUGGESTIONS}>
            {filtered.map(w => (
              <div key={w} className={RA_SUGGESTION} onClick={() => selectWeapon(w)}>
                {w.charAt(0).toUpperCase() + w.slice(1)}
              </div>
            ))}
          </div>
        )}
      </div>

      {showLog && (
        <pre className="max-h-75 shrink-0 overflow-y-auto whitespace-pre-wrap break-all rounded-5 border border-border/60 bg-black/30 p-2.5 text-10 text-muted">
          {sessionLog}
        </pre>
      )}

      {selectedWeapon && (
        <>
          {/* Unified stat picker — click to add, sign toggled below */}
          <div className={RA_SECTION_LABEL}>Select stats rolled <span className={RA_OPTIONAL}>(click to add, set + / − below)</span></div>
          <div className={RA_STAT_GRID}>
            {ALL_STATS.map(stat => {
              const entry = builtStats.find(s => s.name === stat);
              return (
                <button
                  key={stat}
                  className={entry?.positive ? RA_STAT_BTN_SELECTED : RA_STAT_BTN}
                  onClick={() => toggleStat(stat)}
                >
                  {entry ? (entry.positive ? "+" : "−") : ""}{stat}
                </button>
              );
            })}
          </div>

          {/* Per-stat value rows with +/- and %/× toggles */}
          {builtStats.length > 0 && (
            <div className={RA_VALUE_INPUTS}>
              <div className={RA_SECTION_LABEL}>Stat values</div>
              {builtStats.map(stat => (
                <div key={stat.name} className={RA_VALUE_ROW}>
                  {/* +/- toggle */}
                  <button
                    className={stat.positive ? RA_SIGN_POS : RA_SIGN_NEG}
                    onClick={() => toggleStatSign(stat.name)}
                    title="Toggle positive / negative"
                  >{stat.positive ? "+" : "−"}</button>
                  <span className={RA_VALUE_LABEL}>{stat.name}</span>
                  <input
                    className={RA_VALUE_INPUT}
                    placeholder={stat.useMultiplier ? "e.g. 0.88" : "e.g. 85"}
                    value={stat.value}
                    onChange={e => updateStatValue(stat.name, e.target.value)}
                  />
                  {/* %/× toggle — click to switch format */}
                  <button
                    className={RA_FMT_BTN}
                    onClick={() => toggleStatFormat(stat.name)}
                    title="Click to switch between % and × (multiplier)"
                  >
                    {stat.useMultiplier ? "×" : "%"}
                  </button>
                </div>
              ))}
              <div className="mt-1 flex items-center gap-2">
                <button className={RA_SAVE_BTN} onClick={saveCurrentRoll}>
                  {editingId ? "✓ Update Roll" : "💾 Save Roll"}
                </button>
                {editingId && <button className={RA_CANCEL_EDIT} onClick={reset}>Cancel</button>}
                {saveStatus && <span className={`text-11 ${saveStatus.includes("!") || saveStatus.includes("✓") ? "text-green" : "text-red"}`}>{saveStatus}</span>}
              </div>
            </div>
          )}

          {/* Analysis — one card per build alternative */}
          {analysis && (
            <div className={RA_ALTERNATIVES}>
              {analysis.alternatives.map((alt, i) => (
                <div key={i} className={RA_ALT_CARD}>
                  <div className={RA_ALT_HEADER}>
                    {analysis.alternatives.length > 1 && (
                      <span className={RA_ALT_LABEL}>{alt.label}</span>
                    )}
                    <span className={RA_VERDICT} style={{ color: verdictColor(alt.verdict) }}>
                      {alt.verdict}
                    </span>
                  </div>
                  <ScoreBar score={alt.score} />
                  <div className={RA_STATS_BREAKDOWN}>
                    {alt.matched.map(s => (
                      <div key={s} className={STAT_TONE.good.row}>
                        <span className={RA_STAT_ICON}>✓</span><span>{s}</span>
                        <span className={STAT_TONE.good.tag}>Wanted</span>
                      </div>
                    ))}
                    {alt.missing.map(s => (
                      <div key={s} className={STAT_TONE.miss.row}>
                        <span className={RA_STAT_ICON}>○</span><span>{s}</span>
                        <span className={STAT_TONE.miss.tag}>Not rolled</span>
                      </div>
                    ))}
                    {i === 0 && analysis.safe_negatives_present.map(s => (
                      <div key={s} className={STAT_TONE.safe.row}>
                        <span className={RA_STAT_ICON}>✓</span><span>−{s}</span>
                        <span className={STAT_TONE.safe.tag}>Safe neg</span>
                      </div>
                    ))}
                    {i === 0 && analysis.harmful_negatives.map(s => (
                      <div key={s} className={STAT_TONE.bad.row}>
                        <span className={RA_STAT_ICON}>✗</span><span>−{s}</span>
                        <span className={STAT_TONE.bad.tag}>Harmful</span>
                      </div>
                    ))}
                  </div>
                </div>
              ))}
              {analysis.notes && (
                <div className={RA_NOTES}>ℹ {analysis.notes}</div>
              )}
            </div>
          )}

          <button className={RA_NEXT_ROLL} onClick={reset}>
            Next roll →
          </button>
        </>
      )}

      {/* ── Saved Rolls ──────────────────────────────────────────────────────── */}
      {savedRivens.length > 0 && (
        <div className={RA_SAVED_SECTION}>
          <div className={RA_SECTION_LABEL + " mb-2"}>
            Saved Rolls ({savedRivens.length}/50)
            {compareIds.size > 0 && <span className="ml-2 text-11 text-accent">
              {compareIds.size === 1 ? "Select 1 more to compare" : "Comparing ↓"}
            </span>}
          </div>

          <div className={RA_SAVED_GRID}>
            {savedRivens.map(r => {
              const stats: RivenStat[] = (() => { try { return JSON.parse(r.stats_json); } catch { return []; } })();
              const isSelected = compareIds.has(r.id);
              const isEditing = inlineEditId === r.id;
              return (
                <div key={r.id} className={isSelected ? RA_SAVED_CARD_SEL : RA_SAVED_CARD}>
                  {/* Card header */}
                  <div className={RA_SAVED_HEADER}>
                    <input
                      className={RA_LABEL_INPUT}
                      value={isEditing ? inlineEditLabel : r.label}
                      onChange={e => isEditing ? setInlineEditLabel(e.target.value) : setRenameDraft(p => ({ ...p, [r.id]: e.target.value }))}
                      onBlur={() => !isEditing && applyRename(r.id)}
                      onKeyDown={e => !isEditing && e.key === "Enter" && applyRename(r.id)}
                    />
                    <div className={RA_SAVED_ACTIONS}>
                      {isEditing ? (<>
                        <button className={RA_CMP_ACTIVE} onClick={() => saveInlineEdit(r)} title="Save changes">✓</button>
                        <button className={RA_DELETE_BTN} onClick={() => setInlineEditId(null)} title="Cancel edit">✕</button>
                      </>) : (<>
                        <button
                          className={isSelected ? RA_CMP_ACTIVE : RA_CMP_BASE}
                          onClick={() => toggleCompare(r.id)}
                          title="Select for comparison"
                        >{isSelected ? "✓" : "⚖"}</button>
                        <button className={RA_EDIT_BTN} onClick={() => startInlineEdit(r)} title="Edit roll">✎</button>
                        <button className={RA_DELETE_BTN} onClick={() => deleteSaved(r.id)} title="Delete">✕</button>
                      </>)}
                    </div>
                  </div>

                  {/* Verdict */}
                  {r.verdict && (
                    <div className="mb-1 text-11 font-bold" style={{ color: verdictColor2(r.verdict) }}>
                      {r.verdict.split("—")[0].trim()} · {Math.round(r.score * 100)}%
                    </div>
                  )}

                  {/* Stats — editable in edit mode */}
                  <div className={RA_SAVED_STATS}>
                    {(isEditing ? inlineEditStats : stats).map((s, i) => (
                      <div key={i} className={RA_SAVED_STAT + " items-center gap-1"}>
                        {isEditing ? (<>
                          <button
                            className={(s.positive ? RA_SIGN_POS : RA_SIGN_NEG) + " !size-4.5 !min-w-0 !p-0 !text-11"}
                            onClick={() => setInlineEditStats(prev => prev.map((x, j) => j === i ? { ...x, positive: !x.positive } : x))}
                          >{s.positive ? "+" : "−"}</button>
                          <input
                            className="w-12 rounded-3 border border-border/60 bg-black/30 px-1 py-px text-right text-11 text-foreground"
                            value={s.value}
                            onChange={e => setInlineEditStats(prev => prev.map((x, j) => j === i ? { ...x, value: e.target.value } : x))}
                          />
                          <span className="text-11 text-muted">% {s.name}</span>
                        </>) : (<>
                          <span className={s.positive ? "text-muted/70" : "text-red"}>
                            {s.positive ? "+" : "−"}
                          </span>
                          <span>{s.value && `${s.value}% `}{s.name}</span>
                        </>)}
                      </div>
                    ))}
                  </div>

                  <div className="mt-1 text-10 text-muted/40">
                    {r.saved_at.slice(0, 10)}
                  </div>
                </div>
              );
            })}
          </div>

          {/* Comparison panel */}
          {compareList.length === 2 && (
            <div className={RA_COMPARE_PANEL}>
              <div className={RA_SECTION_LABEL + " mb-2"}>Comparison</div>
              <div className={RA_COMPARE_GRID}>
                {compareList.map(r => {
                  const stats: RivenStat[] = (() => { try { return JSON.parse(r.stats_json); } catch { return []; } })();
                  return (
                    <div key={r.id} className={RA_COMPARE_COL}>
                      <div className={RA_COMPARE_LABEL}>{r.label}</div>
                      {r.verdict && (
                        <div className="mb-1.5 text-11 font-bold" style={{ color: verdictColor2(r.verdict) }}>
                          {r.verdict.split("—")[0].trim()} · {Math.round(r.score * 100)}%
                        </div>
                      )}
                      {stats.map((s, i) => (
                        <div key={i} className={RA_SAVED_STAT}>
                          <span className={s.positive ? "text-muted/70" : "text-red"}>
                            {s.positive ? "+" : "−"}
                          </span>
                          <span>{s.value && `${s.value}% `}{s.name}</span>
                        </div>
                      ))}
                    </div>
                  );
                })}
              </div>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
