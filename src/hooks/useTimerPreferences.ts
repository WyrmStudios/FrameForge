import { useCallback, useState } from "react";
import type { FissureWatch } from "../types/settings";
import type { SettingsFile } from "../types/tauri";

const sameArray = <T,>(left: T[], right: T[]) =>
  left.length === right.length && left.every((value, index) => value === right[index]);

const sameWatches = (left: FissureWatch[], right: FissureWatch[]) =>
  left.length === right.length && left.every((watch, index) =>
    watch.id === right[index].id &&
    watch.tier === right[index].tier &&
    watch.missionType === right[index].missionType &&
    watch.variant === right[index].variant &&
    watch.planet === right[index].planet
  );

interface UseTimerPreferencesReturn {
  timerFavorites: string[];
  fissureWatches: FissureWatch[];
  fissureNotifications: boolean;
  setTimerFavorites: React.Dispatch<React.SetStateAction<string[]>>;
  setFissureWatches: React.Dispatch<React.SetStateAction<FissureWatch[]>>;
  setFissureNotifications: React.Dispatch<React.SetStateAction<boolean>>;
  applySettings: (settings: SettingsFile) => void;
}

export function useTimerPreferences(): UseTimerPreferencesReturn {
  const [timerFavorites, setTimerFavorites] = useState<string[]>([]);
  const [fissureWatches, setFissureWatches] = useState<FissureWatch[]>([]);
  const [fissureNotifications, setFissureNotifications] = useState(true);

  const applySettings = useCallback((settings: SettingsFile) => {
    if (Array.isArray(settings.timerFavorites)) {
      setTimerFavorites(current => sameArray(current, settings.timerFavorites) ? current : settings.timerFavorites);
    }
    if (Array.isArray(settings.fissureWatches)) {
      setFissureWatches(current => sameWatches(current, settings.fissureWatches) ? current : settings.fissureWatches);
    }
    if (typeof settings.fissureNotifications === "boolean") {
      setFissureNotifications(current => current === settings.fissureNotifications ? current : settings.fissureNotifications);
    }
  }, []);

  return {
    timerFavorites,
    fissureWatches,
    fissureNotifications,
    setTimerFavorites,
    setFissureWatches,
    setFissureNotifications,
    applySettings,
  };
}
