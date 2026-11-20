import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { runAlertPass, DEFAULT_LEAD_MINS, EVAL_INTERVAL_MS, type AlertRule, type ScheduleEntry } from "../arbitration/arbitrationAlerts";
import { useArbitrationSchedule } from "../arbitration/arbitrationSchedule";
import type { TierKey } from "../arbitration/arbitrationTiers";
import { TAURI_COMMANDS } from "../constants/tauri";
import { notify, permissionGranted } from "../lib/notify";
import { fmtMs } from "../TimerHelper";
import type { SettingsFile, SettingsPatch } from "../types/tauri";

// Called from App because Arbitrations does not mount until its tab is first
// opened, and an alert has to fire before that.
export function useArbitrationAlerts(
  favorites: string[],
  alertTiers: TierKey[],
  leadMins: number,
  settingsLoadedRef: React.MutableRefObject<boolean>,
) {
  // Persisted, so a restart inside the lead window does not alert a second
  // time for the same hour.
  const firedRef = useRef<string[]>([]);
  const firedDirtyRef = useRef(false);
  const alertsOn = favorites.length > 0 || alertTiers.length > 0;

  // Only the startup load restores this. A later settings-updated broadcast
  // could carry a list older than an unsaved one held here.
  const restoreFired = useCallback((s: SettingsFile) => {
    if (Array.isArray(s.arbitrationAlertsFired)) firedRef.current = s.arbitrationAlertsFired.filter((x: unknown) => typeof x === "string");
  }, []);

  const { schedule, error: scheduleError } = useArbitrationSchedule(alertsOn);

  // The loop reads its inputs from here rather than from the effect closure, so
  // starring a node changes what the next tick sees without tearing the timer
  // down and starting a fresh pass on top of one already running. This effect
  // has to stay above the loop's own, which reads the ref on its first tick.
  const inputsRef = useRef({ entries: [] as ScheduleEntry[], rule: {} as AlertRule, leadMins: DEFAULT_LEAD_MINS });
  useEffect(() => {
    inputsRef.current = {
      entries: schedule?.entries ?? [],
      rule: { favorites, tiers: alertTiers },
      leadMins,
    };
  });

  // A pass outlives its tick whenever the notification IPC is slow, and two
  // passes reading the same fired state would raise one occurrence twice.
  const checkingRef = useRef(false);

  // Only Arbitrations raises the prompt — it does that on a user gesture — but
  // a pass the platform refuses sets the flag too, so a blocked alert is shown
  // instead of retried every tick without a word.
  const [permissionDenied, setPermissionDenied] = useState(false);
  // One warning per stretch of denial, not one per pass.
  const deniedLoggedRef = useRef(false);

  useEffect(() => {
    if (alertsOn && scheduleError) {
      console.error("arbitration schedule unavailable, alerts paused:", scheduleError);
    }
  }, [alertsOn, scheduleError]);

  useEffect(() => {
    const check = async () => {
      // Loading settings must finish before a pass can replace persisted keys.
      if (!settingsLoadedRef.current || checkingRef.current) return;
      checkingRef.current = true;
      try {
        const { entries, rule, leadMins } = inputsRef.current;
        const nowMs = Date.now();
        const fired = await runAlertPass(
          entries, rule, leadMins, firedRef.current, nowMs / 1000,
          async e => {
            const handedOver = await notify(
              `Arbitration — ${e.node}${e.region ? ` (${e.region})` : ""}`,
              `${[e.mission_type, e.faction].filter(Boolean).join(" · ")} — ${e.start * 1000 > nowMs
                ? `starts in ${fmtMs(e.start * 1000 - nowMs)}`
                : `under way, ${fmtMs(e.end * 1000 - nowMs)} left`}`,
            );
            if (handedOver) {
              deniedLoggedRef.current = false;
              return true;
            }
            // A pass that could not hand over leaves the occurrence out of the
            // fired list, so the next one offers it again. Say so once rather
            // than retry in silence.
            if (!(await permissionGranted())) {
              if (!deniedLoggedRef.current) {
                deniedLoggedRef.current = true;
                console.warn("arbitration alert not sent: notification permission is missing; the occurrence stays due");
              }
              setPermissionDenied(true);
            }
            return false;
          });
        if (fired !== null) {
          firedRef.current = fired;
          firedDirtyRef.current = true;
        }
        // Keep successful handoffs in memory even if saving fails, then retry
        // persistence on the next pass without raising the same alert again.
        if (firedDirtyRef.current) {
          const patch: SettingsPatch = { arbitrationAlertsFired: firedRef.current };
          await invoke(TAURI_COMMANDS.SAVE_SETTINGS, { json: JSON.stringify(patch) });
          firedDirtyRef.current = false;
        }
      } catch (e) {
        console.error("saving arbitration alert state failed", e);
      } finally {
        checkingRef.current = false;
      }
    };

    // Persistence retries must continue after the last alert rule is removed.
    void check();
    const poll = setInterval(check, EVAL_INTERVAL_MS);
    return () => clearInterval(poll);
  }, [alertsOn, settingsLoadedRef]);

  return { permissionDenied, setPermissionDenied, restoreFired };
}
