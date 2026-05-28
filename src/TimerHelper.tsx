import React, { useState, useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import "./TimerHelper.css";

// ── Types ─────────────────────────────────────────────────────────────────────

interface WsCycle   { expiry: string; }
interface WsCetus   extends WsCycle { isDay: boolean; }
interface WsVallis  extends WsCycle { isWarm: boolean; }
interface WsCambion extends WsCycle { active: string; }
interface WsZariman extends WsCycle { active: boolean; }

interface WsSortieVariant { missionType: string; modifier: string; node: string; }
interface WsSortie   { expiry: string; boss: string; faction: string; variants: WsSortieVariant[]; active: boolean; }
interface WsMission  { type: string; node: string; }
interface WsArchon   { expiry: string; boss: string; faction: string; missions: WsMission[]; active: boolean; }
interface WsTrader   { expiry: string; activation: string; character: string; location: string; active: boolean; }
interface WsNight    { expiry: string; season: number; active: boolean; }
export interface WsFissure  { id: string; expiry: string; node: string; missionType: string; enemy: string; tier: string; tierNum: number; isStorm: boolean; isHard: boolean; active: boolean; }
export interface WsStorm    { id: string; expiry: string; node: string; missionType: string; enemy: string; tier: string; tierNum: number; active: boolean; }
interface WsAlert    { id: string; expiry: string; missionType: string; faction: string; node: string; rewardItem?: string; rewardCredits: number; }
interface WsInvasion { id: string; node: string; attacker: string; defender: string; attReward: string; defReward: string; pct: number; }
interface WsDarvo    { expiry: string; item: string; discount: number; originalPrice: number; salePrice: number; amountTotal: number; amountSold: number; }
interface WsCircuit  { expiry: string; normalFrames: string[]; hardWeapons: string[]; }
interface WsSimple   { expiry: string; }
interface WsBounty   { expiry: string; jobCount: number; }
interface WsEvent    { expiry: string; label: string; }

export interface FissureWatch {
  id: string;
  tier: string;        // "Any" | "Omnia" | "Lith" | "Meso" | "Neo" | "Axi" | "Requiem"
  missionType: string; // "Any" | "Rescue" | "Capture" | ...
  variant: "any" | "normal" | "hard" | "storm";
}

// actualVariant is passed explicitly from the caller who knows which array the fissure came from
export function matchesWatch(
  watch: FissureWatch,
  fissure: WsFissure | WsStorm,
  actualVariant: "normal" | "hard" | "storm",
): boolean {
  // Variant — checked first; quickest way to exit
  if (watch.variant !== "any" && watch.variant !== actualVariant) return false;

  // Tier — bidirectional Omnia:
  // • Watch "Omnia" matches any non-Requiem tier
  // • Fissure tier "Omnia" matches any watch except "Requiem"
  const fTier = fissure.tier;
  if (watch.tier !== "Any") {
    if (watch.tier === "Omnia") {
      if (fTier === "Requiem") return false;
    } else if (fTier === "Omnia") {
      if (watch.tier === "Requiem") return false;
    } else if (watch.tier !== fTier) {
      return false;
    }
  }

  // Mission type
  if (watch.missionType !== "Any" && fissure.missionType !== watch.missionType) return false;

  return true;
}

export interface WorldState {
  cetus?:          WsCetus;
  vallis?:         WsVallis;
  cambion?:        WsCambion;
  zariman?:        WsZariman;
  bounties?:       Record<string, WsBounty>;
  sortie?:         WsSortie;
  archonHunt?:     WsArchon;
  voidTrader?:     WsTrader;
  nightwave?:      WsNight;
  circuit?:        WsCircuit;
  kahl?:           WsSimple;
  deepArchimedea?: WsSimple;
  activeEvent?:    WsEvent;
  darvo?:          WsDarvo;
  alerts?:         WsAlert[];
  invasions?:      WsInvasion[];
  fissures?:       WsFissure[];
  spFissures?:     WsFissure[];
  voidStorms?:     WsStorm[];
}

// ── Helpers ───────────────────────────────────────────────────────────────────

export function fmtMs(ms: number): string {
  if (ms <= 0) return "—";
  const s = Math.floor(ms / 1000) % 60;
  const m = Math.floor(ms / 60000) % 60;
  const h = Math.floor(ms / 3600000) % 24;
  const d = Math.floor(ms / 86400000);
  if (d > 0) return `${d}d ${h}h ${m}m`;
  if (h > 0) return `${h}h ${m}m ${s}s`;
  return `${m}m ${s}s`;
}

export function fmtExpiry(expiry: string, now: number): string {
  return fmtMs(new Date(expiry).getTime() - now);
}

function nextUtcMidnight(): string {
  const n = new Date();
  return new Date(Date.UTC(n.getUTCFullYear(), n.getUTCMonth(), n.getUTCDate() + 1)).toISOString();
}
function nextWeeklyReset(): string {
  const n = new Date();
  const d = n.getUTCDay();
  return new Date(Date.UTC(n.getUTCFullYear(), n.getUTCMonth(), n.getUTCDate() + (d === 0 ? 1 : 8 - d))).toISOString();
}

const TIER_COLOR: Record<string, string> = {
  Lith: "#c8853a", Meso: "#a8a9ad", Neo: "#f0c040",
  Axi: "#e5c04a", Requiem: "#9b6dff", Omnia: "#e0e0e0",
};

export const TIMER_LABELS: Record<string, string> = {
  "cetus-cycle":    "Cetus",
  "vallis-cycle":   "Orb Vallis",
  "cambion-cycle":  "Cambion Drift",
  "zariman-cycle":  "Zariman",
  "bounty-cetus":   "Cetus Bounties",
  "bounty-vallis":  "Vallis Bounties",
  "bounty-cambion": "Cambion Bounties",
  "bounty-zariman": "Zariman Bounties",
  "bounty-hex":     "Hex Bounties",
  "sortie":         "Sortie",
  "archon-hunt":    "Archon Hunt",
  "daily-reset":    "Daily Reset",
  "weekly-reset":   "Weekly Reset",
  "void-trader":    "Void Trader",
  "nightwave":      "Nightwave",
  "circuit":        "The Circuit",
  "kahl":           "Kahl / Break Narmer",
  "deep-archimedea":"Deep Archimedea",
};

export function getTimerInfo(id: string, ws: WorldState): { state: string; expiry: string } | null {
  switch (id) {
    case "cetus-cycle":    return ws.cetus    ? { state: ws.cetus.isDay ? "Day" : "Night",        expiry: ws.cetus.expiry }    : null;
    case "vallis-cycle":   return ws.vallis   ? { state: ws.vallis.isWarm ? "Warm" : "Cold",      expiry: ws.vallis.expiry }   : null;
    case "cambion-cycle":  return ws.cambion  ? { state: "Cycle",                                 expiry: ws.cambion.expiry }  : null;
    case "zariman-cycle":  return ws.zariman  ? { state: "Active",                                expiry: ws.zariman.expiry }  : null;
    case "bounty-cetus":   return ws.bounties?.cetus    ? { state: `${ws.bounties.cetus.jobCount} jobs`,    expiry: ws.bounties.cetus.expiry }    : null;
    case "bounty-vallis":  return ws.bounties?.vallis   ? { state: `${ws.bounties.vallis.jobCount} jobs`,   expiry: ws.bounties.vallis.expiry }   : null;
    case "bounty-cambion": return ws.bounties?.cambion  ? { state: `${ws.bounties.cambion.jobCount} jobs`,  expiry: ws.bounties.cambion.expiry }  : null;
    case "bounty-zariman": return ws.bounties?.zariman  ? { state: `${ws.bounties.zariman.jobCount} jobs`,  expiry: ws.bounties.zariman.expiry }  : null;
    case "bounty-hex":     return ws.bounties?.hex      ? { state: `${ws.bounties.hex.jobCount} jobs`,      expiry: ws.bounties.hex.expiry }      : null;
    case "sortie":         return ws.sortie      ? { state: ws.sortie.faction,     expiry: ws.sortie.expiry }     : null;
    case "archon-hunt":    return ws.archonHunt  ? { state: ws.archonHunt.boss,    expiry: ws.archonHunt.expiry } : null;
    case "daily-reset":    return { state: "UTC 00:00", expiry: nextUtcMidnight() };
    case "weekly-reset":   return { state: "Monday",    expiry: nextWeeklyReset() };
    case "void-trader":    return ws.voidTrader ? { state: ws.voidTrader.active ? "Here" : "Away", expiry: ws.voidTrader.active ? ws.voidTrader.expiry : ws.voidTrader.activation } : null;
    case "nightwave":      return ws.nightwave?.active ? { state: `S${ws.nightwave.season}`, expiry: ws.nightwave.expiry } : null;
    case "circuit":        return ws.circuit    ? { state: "Weekly",  expiry: ws.circuit.expiry }        : null;
    case "kahl":           return ws.kahl       ? { state: "Weekly",  expiry: ws.kahl.expiry }           : null;
    case "deep-archimedea":return ws.deepArchimedea ? { state: "Weekly", expiry: ws.deepArchimedea.expiry } : null;
    default: return null;
  }
}

// ── Component ─────────────────────────────────────────────────────────────────

const TIERS = ["Any","Omnia","Lith","Meso","Neo","Axi","Requiem"];
const VARIANTS: { key: FissureWatch["variant"]; label: string }[] = [
  { key: "any",    label: "Any" },
  { key: "normal", label: "Normal" },
  { key: "hard",   label: "Steel Path" },
  { key: "storm",  label: "Storm" },
];
// Mission types available per variant (storms are Railjack — different pool)
const MISSION_TYPES_BY_VARIANT: Record<FissureWatch["variant"], string[]> = {
  any:    ["Any","Rescue","Capture","Defense","Survival","Excavation","Interception","Disruption","Sabotage","Spy","Mobile Defense","Extermination","Assassination","Skirmish","Volatile"],
  normal: ["Any","Rescue","Capture","Defense","Survival","Excavation","Interception","Disruption","Sabotage","Spy","Mobile Defense","Extermination","Assassination"],
  hard:   ["Any","Rescue","Capture","Defense","Survival","Excavation","Interception","Disruption","Sabotage","Spy","Mobile Defense","Extermination","Assassination"],
  storm:  ["Any","Skirmish","Volatile","Defense","Extermination","Sabotage","Assassination"],
};

interface Props {
  favorites: string[];
  onFavoriteToggle: (id: string) => void;
  fissureWatches: FissureWatch[];
  onAddWatch: (w: FissureWatch) => void;
  onRemoveWatch: (id: string) => void;
}

type FissureTab = "normal" | "hard" | "storm";

export default function TimerHelper({ favorites, onFavoriteToggle, fissureWatches, onAddWatch, onRemoveWatch }: Props) {
  const [ws, setWs] = useState<WorldState | null>(null);
  const [now, setNow] = useState(Date.now());
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");
  const [fissureTab, setFissureTab] = useState<FissureTab>("normal");
  const [showWatchForm, setShowWatchForm] = useState(false);
  const [wTier, setWTier] = useState("Any");
  const [wMission, setWMission] = useState("Any");
  const [wVariant, setWVariant] = useState<FissureWatch["variant"]>("any");

  const setVariant = useCallback((v: FissureWatch["variant"]) => {
    setWVariant(v);
    // Reset mission if it's not available in the new variant's list
    setWMission(m => MISSION_TYPES_BY_VARIANT[v].includes(m) ? m : "Any");
  }, []);

  const fetchWS = useCallback(() => {
    invoke<WorldState>("fetch_worldstate")
      .then(data => { setWs(data); setLoading(false); setError(""); })
      .catch(e => { setError(String(e)); setLoading(false); });
  }, []);

  useEffect(() => { fetchWS(); const iv = setInterval(fetchWS, 60000); return () => clearInterval(iv); }, [fetchWS]);
  useEffect(() => { const iv = setInterval(() => setNow(Date.now()), 1000); return () => clearInterval(iv); }, []);

  const isFav = (id: string) => favorites.includes(id);
  const cd = (expiry: string) => fmtMs(new Date(expiry).getTime() - now);

  function StarBtn({ id }: { id: string }) {
    return (
      <button className={`timer-star${isFav(id) ? " fav" : ""}`} onClick={() => onFavoriteToggle(id)} title={isFav(id) ? "Unpin" : "Pin to Modular Window"}>
        {isFav(id) ? "★" : "☆"}
      </button>
    );
  }

  function SectionHeader({ label, children }: { label: string; children?: React.ReactNode }) {
    return <div className="timer-group-label">{label}{children}</div>;
  }

  if (loading) return <div className="timer-loading">Loading worldstate…</div>;

  return (
    <div className="timer-helper">
      {error && <div className="timer-error">{error} <button onClick={fetchWS}>Retry</button></div>}

      {/* ── World Cycles ──────────────────────────────────────────────────── */}
      <SectionHeader label="World Cycles" />

      {ws?.cetus && (
        <div className="timer-row">
          <StarBtn id="cetus-cycle" />
          <span className="timer-name">Cetus</span>
          <span className={`timer-state ${ws.cetus.isDay ? "st-day" : "st-night"}`}>{ws.cetus.isDay ? "Day" : "Night"}</span>
          <span className="timer-cd">{cd(ws.cetus.expiry)}</span>
          <span className="timer-until">until {ws.cetus.isDay ? "Night" : "Day"}</span>
        </div>
      )}

      {ws?.vallis && (
        <div className="timer-row">
          <StarBtn id="vallis-cycle" />
          <span className="timer-name">Orb Vallis</span>
          <span className={`timer-state ${ws.vallis.isWarm ? "st-warm" : "st-cold"}`}>{ws.vallis.isWarm ? "Warm" : "Cold"}</span>
          <span className="timer-cd">{cd(ws.vallis.expiry)}</span>
          <span className="timer-until">until {ws.vallis.isWarm ? "Cold" : "Warm"}</span>
        </div>
      )}

      {ws?.cambion && (
        <div className="timer-row">
          <StarBtn id="cambion-cycle" />
          <span className="timer-name">Cambion Drift</span>
          <span className="timer-state st-fass">Active</span>
          <span className="timer-cd">{cd(ws.cambion.expiry)}</span>
          <span className="timer-until">next cycle</span>
        </div>
      )}

      {ws?.zariman && (
        <div className="timer-row">
          <StarBtn id="zariman-cycle" />
          <span className="timer-name">Zariman</span>
          <span className="timer-state st-neutral">Active</span>
          <span className="timer-cd">{cd(ws.zariman.expiry)}</span>
          <span className="timer-until">reset</span>
        </div>
      )}

      {/* ── Bounties ──────────────────────────────────────────────────────── */}
      {ws?.bounties && Object.keys(ws.bounties).length > 0 && (
        <>
          <SectionHeader label="Bounties" />
          {([
            ["cetus",   "Cetus",          "bounty-cetus"],
            ["vallis",  "Orb Vallis",     "bounty-vallis"],
            ["cambion", "Cambion Drift",  "bounty-cambion"],
            ["zariman", "Zariman",        "bounty-zariman"],
            ["hex",     "Hex / Albrecht", "bounty-hex"],
          ] as [string, string, string][]).map(([key, label, favId]) => {
            const b = ws.bounties![key];
            if (!b) return null;
            return (
              <div key={key} className="timer-row">
                <StarBtn id={favId} />
                <span className="timer-name">{label}</span>
                <span className="timer-state st-neutral">{b.jobCount} jobs</span>
                <span className="timer-cd">{cd(b.expiry)}</span>
                <span className="timer-until">reset</span>
              </div>
            );
          })}
        </>
      )}

      {/* ── Daily & Weekly ────────────────────────────────────────────────── */}
      <SectionHeader label="Daily &amp; Weekly" />

      <div className="timer-row">
        <StarBtn id="daily-reset" />
        <span className="timer-name">Daily Reset</span>
        <span className="timer-state st-neutral">UTC 00:00</span>
        <span className="timer-cd">{fmtMs(new Date(nextUtcMidnight()).getTime() - now)}</span>
      </div>

      <div className="timer-row">
        <StarBtn id="weekly-reset" />
        <span className="timer-name">Weekly Reset</span>
        <span className="timer-state st-neutral">Monday</span>
        <span className="timer-cd">{fmtMs(new Date(nextWeeklyReset()).getTime() - now)}</span>
      </div>

      {ws?.sortie && (
        <div className="timer-block">
          <div className="timer-row">
            <StarBtn id="sortie" />
            <span className="timer-name">Sortie</span>
            <span className="timer-state st-neutral">{ws.sortie.faction}</span>
            <span className="timer-cd">{cd(ws.sortie.expiry)}</span>
          </div>
          <div className="timer-sublist">
            {ws.sortie.variants?.map((v, i) => (
              <div key={i} className="timer-subrow">
                <span className="sub-node">{v.node}</span>
                <span className="sub-type">{v.missionType}</span>
                {v.modifier && <span className="sub-mod">{v.modifier}</span>}
              </div>
            ))}
          </div>
        </div>
      )}

      {ws?.archonHunt && (
        <div className="timer-block">
          <div className="timer-row">
            <StarBtn id="archon-hunt" />
            <span className="timer-name">Archon Hunt</span>
            <span className="timer-state st-neutral">{ws.archonHunt.boss}</span>
            <span className="timer-cd">{cd(ws.archonHunt.expiry)}</span>
          </div>
          <div className="timer-sublist">
            {ws.archonHunt.missions?.map((m, i) => (
              <div key={i} className="timer-subrow">
                <span className="sub-node">{m.node}</span>
                <span className="sub-type">{m.type}</span>
              </div>
            ))}
          </div>
        </div>
      )}

      {ws?.circuit && (
        <div className="timer-block">
          <div className="timer-row">
            <StarBtn id="circuit" />
            <span className="timer-name">The Circuit</span>
            <span className="timer-state st-duviri">Duviri</span>
            <span className="timer-cd">{cd(ws.circuit.expiry)}</span>
          </div>
          {(ws.circuit.normalFrames?.length > 0 || ws.circuit.hardWeapons?.length > 0) && (
            <div className="timer-sublist">
              {ws.circuit.normalFrames?.length > 0 && (
                <div className="timer-subrow"><span className="sub-node">Normal</span><span className="sub-type">{ws.circuit.normalFrames.join(" · ")}</span></div>
              )}
              {ws.circuit.hardWeapons?.length > 0 && (
                <div className="timer-subrow"><span className="sub-node">Hard</span><span className="sub-type">{ws.circuit.hardWeapons.join(" · ")}</span></div>
              )}
            </div>
          )}
        </div>
      )}

      {ws?.kahl && (
        <div className="timer-row">
          <StarBtn id="kahl" />
          <span className="timer-name">Kahl / Break Narmer</span>
          <span className="timer-state st-neutral">Weekly</span>
          <span className="timer-cd">{cd(ws.kahl.expiry)}</span>
        </div>
      )}

      {ws?.deepArchimedea && (
        <div className="timer-row">
          <StarBtn id="deep-archimedea" />
          <span className="timer-name">Deep Archimedea</span>
          <span className="timer-state st-neutral">Weekly</span>
          <span className="timer-cd">{cd(ws.deepArchimedea.expiry)}</span>
        </div>
      )}

      {/* ── Events ────────────────────────────────────────────────────────── */}
      <SectionHeader label="Events" />

      {ws?.voidTrader && (
        <div className="timer-row">
          <StarBtn id="void-trader" />
          <span className="timer-name">{ws.voidTrader.character}</span>
          <span className={`timer-state ${ws.voidTrader.active ? "st-active" : "st-away"}`}>{ws.voidTrader.active ? "Here" : "Away"}</span>
          <span className="timer-cd">{ws.voidTrader.active ? cd(ws.voidTrader.expiry) : fmtMs(new Date(ws.voidTrader.activation).getTime() - now)}</span>
          <span className="timer-until">{ws.voidTrader.active ? ws.voidTrader.location : "until arrival"}</span>
        </div>
      )}

      {ws?.nightwave?.active && (
        <div className="timer-row">
          <StarBtn id="nightwave" />
          <span className="timer-name">Nightwave</span>
          <span className="timer-state st-neutral">Season {ws.nightwave.season}</span>
          <span className="timer-cd">{cd(ws.nightwave.expiry)}</span>
        </div>
      )}

      {ws?.activeEvent && (
        <div className="timer-row">
          <span className="timer-star" style={{ visibility: "hidden" }}>☆</span>
          <span className="timer-name">{ws.activeEvent.label}</span>
          <span className="timer-state st-active">Event</span>
          <span className="timer-cd">{cd(ws.activeEvent.expiry)}</span>
        </div>
      )}

      {ws?.darvo && (
        <div className="timer-block">
          <div className="timer-row">
            <span className="timer-star" style={{ visibility: "hidden" }}>☆</span>
            <span className="timer-name">Darvo Deal</span>
            <span className="timer-state st-neutral">-{ws.darvo.discount}%</span>
            <span className="timer-cd">{cd(ws.darvo.expiry)}</span>
          </div>
          <div className="timer-sublist">
            <div className="timer-subrow">
              <span className="sub-node">{ws.darvo.item}</span>
              <span className="sub-type" style={{ color: "#f0c040" }}>{ws.darvo.salePrice}p</span>
              <span className="sub-mod" style={{ textDecoration: "line-through", color: "var(--muted)" }}>{ws.darvo.originalPrice}p</span>
            </div>
          </div>
        </div>
      )}

      {/* ── Alerts ────────────────────────────────────────────────────────── */}
      {ws?.alerts && ws.alerts.length > 0 && (
        <>
          <SectionHeader label={`Alerts (${ws.alerts.length})`} />
          {ws.alerts.map(a => (
            <div key={a.id} className="timer-row">
              <span className="timer-star" style={{ visibility: "hidden" }}>☆</span>
              <span className="timer-name">{a.missionType}</span>
              <span className="timer-state st-neutral">{a.faction}</span>
              <span className="timer-cd">{cd(a.expiry)}</span>
              {a.rewardItem && <span className="timer-until">{a.rewardItem}</span>}
            </div>
          ))}
        </>
      )}

      {/* ── Invasions ─────────────────────────────────────────────────────── */}
      {ws?.invasions && ws.invasions.length > 0 && (
        <>
          <SectionHeader label={`Invasions (${ws.invasions.length})`} />
          {ws.invasions.map(inv => (
            <div key={inv.id} className="timer-invasion">
              <div className="timer-row" style={{ borderBottom: "none", paddingBottom: 2 }}>
                <span className="timer-star" style={{ visibility: "hidden" }}>☆</span>
                <span className="timer-name">{inv.node}</span>
                <span className="timer-state st-neutral">{inv.pct}%</span>
              </div>
              <div className="invasion-bar-wrap">
                <div className="invasion-bar-inner" style={{ width: `${Math.min(100, inv.pct)}%` }} />
              </div>
              <div className="timer-sublist" style={{ marginTop: 2, marginBottom: 4 }}>
                <div className="timer-subrow">
                  <span className="sub-node invasion-att">{inv.attacker}</span>
                  <span className="sub-type">{inv.attReward || "—"}</span>
                </div>
                <div className="timer-subrow">
                  <span className="sub-node invasion-def">{inv.defender}</span>
                  <span className="sub-type">{inv.defReward || "—"}</span>
                </div>
              </div>
            </div>
          ))}
        </>
      )}

      {/* ── Void Fissures ─────────────────────────────────────────────────── */}
      {(() => {
        // Rust already filters: started (activation ≤ now) and not expired (expiry > now).
        // Just sort — no extra client-side filtering so count always equals what renders.
        const normal = [...(ws?.fissures   ?? [])].sort((a, b) => a.tierNum - b.tierNum);
        const hard   = [...(ws?.spFissures ?? [])].sort((a, b) => a.tierNum - b.tierNum);
        const storms = [...(ws?.voidStorms ?? [])].sort((a, b) => a.tierNum - b.tierNum);
        const list   = fissureTab === "normal" ? normal : fissureTab === "hard" ? hard : storms;

        const FissureRow = ({ f, v }: { f: WsFissure | WsStorm; v: "normal" | "hard" | "storm" }) => (
          <div className={`timer-row timer-fissure${fissureWatches.some(w => matchesWatch(w, f, v)) ? " fissure-watched" : ""}`}>
            <span className="fissure-tier" style={{ color: TIER_COLOR[f.tier] ?? "#ccc" }}>{f.tier}</span>
            <span className="timer-name">{f.missionType}</span>
            {f.enemy && <span className="timer-state st-neutral" style={{ fontSize: 10 }}>{f.enemy}</span>}
            <span className="timer-fissure-node">{f.node}</span>
            <span className="timer-cd">{cd(f.expiry)}</span>
          </div>
        );

        return (
          <>
            <div className="timer-group-label timer-group-label-fissures">
              <span>Void Fissures</span>
              <div className="fissure-tabs">
                <button className={fissureTab === "normal" ? "active" : ""} onClick={() => setFissureTab("normal")}>Normal</button>
                <button className={fissureTab === "hard"   ? "active" : ""} onClick={() => setFissureTab("hard")}>Steel Path</button>
                <button className={fissureTab === "storm"  ? "active" : ""} onClick={() => setFissureTab("storm")}>Storms</button>
              </div>
              <button className={`fissure-watch-btn${showWatchForm ? " active" : ""}`} onClick={() => setShowWatchForm(v => !v)} title="Manage fissure watches">
                <svg viewBox="0 0 16 16" fill="none"><path d="M2 4h12M4 8h8M6 12h4" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round"/></svg>
              </button>
            </div>

            {showWatchForm && (
              <div className="watch-panel">
                {fissureWatches.length > 0 && (
                  <div className="watch-chips">
                    {fissureWatches.map(w => (
                      <div key={w.id} className="watch-chip">
                        <span className="watch-chip-tier" style={{ color: TIER_COLOR[w.tier] ?? "var(--text)" }}>{w.tier}</span>
                        {w.missionType !== "Any" && <span className="watch-chip-mt">{w.missionType}</span>}
                        {w.variant !== "any" && <span className="watch-chip-var">{VARIANTS.find(v => v.key === w.variant)?.label}</span>}
                        <button className="watch-chip-del" onClick={() => onRemoveWatch(w.id)}>×</button>
                      </div>
                    ))}
                  </div>
                )}
                <div className="watch-form">
                  <div className="watch-form-row">
                    <span className="watch-form-label">Mode</span>
                    <div className="watch-pill-group">
                      {VARIANTS.map(v => (
                        <button key={v.key} className={`watch-pill${wVariant === v.key ? " active" : ""}`} onClick={() => setVariant(v.key)}>{v.label}</button>
                      ))}
                    </div>
                  </div>
                  <div className="watch-form-row">
                    <span className="watch-form-label">Tier</span>
                    <div className="watch-pill-group watch-pill-group-wrap">
                      {TIERS.map(t => (
                        <button key={t} className={`watch-pill${wTier === t ? " active" : ""}`}
                          style={wTier === t && TIER_COLOR[t] ? { borderColor: TIER_COLOR[t], color: TIER_COLOR[t] } : undefined}
                          onClick={() => setWTier(t)}>{t}</button>
                      ))}
                    </div>
                  </div>
                  <div className="watch-form-row">
                    <span className="watch-form-label">Type</span>
                    <div className="watch-pill-group watch-pill-group-wrap">
                      {MISSION_TYPES_BY_VARIANT[wVariant].map(m => (
                        <button key={m} className={`watch-pill${wMission === m ? " active" : ""}`} onClick={() => setWMission(m)}>{m}</button>
                      ))}
                    </div>
                  </div>
                  <button className="watch-add-btn" onClick={() => {
                    onAddWatch({ id: `${Date.now()}`, tier: wTier, missionType: wMission, variant: wVariant });
                    setWTier("Any"); setWMission("Any"); setVariant("any");
                  }}>+ Add Watch</button>
                </div>
              </div>
            )}

            {list.length === 0
              ? <div className="timer-empty">No active fissures.</div>
              : list.map((f, i) => <FissureRow key={i} f={f} v={fissureTab === "hard" ? "hard" : fissureTab === "storm" ? "storm" : "normal"} />)
            }
          </>
        );
      })()}
    </div>
  );
}
