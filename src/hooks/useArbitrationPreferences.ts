import { useCallback, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { clampLead, DEFAULT_LEAD_MINS } from "../arbitration/arbitrationAlerts";
import { clampScheduleDays, DEFAULT_SCHEDULE_DAYS } from "../arbitration/arbitrationSchedule";
import { sanitizeTierKeys, TIER_KEYS, type TierKey } from "../arbitration/arbitrationTiers";
import { TAURI_COMMANDS } from "../constants/tauri";
import type { SettingsFile } from "../types/tauri";

interface UseArbitrationPreferencesReturn {
  arbFavorites: string[];
  arbLeadMins: number;
  arbTierFilter: TierKey[];
  arbAlertTiers: TierKey[];
  arbScheduleDays: number;
  arbOverlayEnabled: boolean;
  setArbFavorites: React.Dispatch<React.SetStateAction<string[]>>;
  setArbLeadMins: React.Dispatch<React.SetStateAction<number>>;
  setArbTierFilter: React.Dispatch<React.SetStateAction<TierKey[]>>;
  setArbAlertTiers: React.Dispatch<React.SetStateAction<TierKey[]>>;
  setArbScheduleDays: React.Dispatch<React.SetStateAction<number>>;
  setArbOverlayEnabled: React.Dispatch<React.SetStateAction<boolean>>;
  applySettings: (settings: SettingsFile) => void;
}

export function useArbitrationPreferences(): UseArbitrationPreferencesReturn {
  const [arbFavorites, setArbFavorites] = useState<string[]>([]);
  const [arbLeadMins, setArbLeadMins] = useState(DEFAULT_LEAD_MINS);
  // The filter starts wide and the alert rule starts empty: showing every hour
  // is what a browser is for, while alerting is opt-in.
  const [arbTierFilter, setArbTierFilter] = useState<TierKey[]>([...TIER_KEYS]);
  const [arbAlertTiers, setArbAlertTiers] = useState<TierKey[]>([]);
  const [arbScheduleDays, setArbScheduleDays] = useState(DEFAULT_SCHEDULE_DAYS);
  const [arbOverlayEnabled, setArbOverlayEnabled] = useState(false);

  const applySettings = useCallback((s: SettingsFile) => {
    if (Array.isArray(s.arbitrationFavorites)) setArbFavorites(s.arbitrationFavorites.filter((x: unknown) => typeof x === "string"));
    if (typeof s.arbitrationLeadMins === "number") setArbLeadMins(clampLead(s.arbitrationLeadMins));
    const storedFilter = sanitizeTierKeys(s.arbitrationTierFilter);
    if (storedFilter) setArbTierFilter(storedFilter);
    const storedAlertTiers = sanitizeTierKeys(s.arbitrationAlertTiers);
    if (storedAlertTiers) setArbAlertTiers(storedAlertTiers);
    if (typeof s.arbitrationScheduleDays === "number") setArbScheduleDays(clampScheduleDays(s.arbitrationScheduleDays));
    if (typeof s.arbitrationOverlayEnabled === "boolean") {
      setArbOverlayEnabled(s.arbitrationOverlayEnabled);
      invoke(TAURI_COMMANDS.SET_ARBITRATION_OVERLAY_ENABLED, { enabled: s.arbitrationOverlayEnabled });
    }
  }, []);

  return {
    arbFavorites,
    arbLeadMins,
    arbTierFilter,
    arbAlertTiers,
    arbScheduleDays,
    arbOverlayEnabled,
    setArbFavorites,
    setArbLeadMins,
    setArbTierFilter,
    setArbAlertTiers,
    setArbScheduleDays,
    setArbOverlayEnabled,
    applySettings,
  };
}
