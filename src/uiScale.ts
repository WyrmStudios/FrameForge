import { currentMonitor } from "@tauri-apps/api/window";

function read(key: string, fallback: number): number {
  const v = parseFloat(localStorage.getItem(key) ?? "");
  return Number.isFinite(v) && v > 0 ? v : fallback;
}

export const appScale = () => read("ff-text-scale", 1);

// Overlays read their own key, so they can scale independently of the app
// window. Nothing writes that key yet, so for now they follow the app text size.
export const overlayScale = () => read("ff-overlay-scale", appScale());

export function applyScale(isOverlay: boolean) {
  const s = isOverlay ? overlayScale() : appScale();
  document.documentElement.style.setProperty("--ff-scale", s.toString());
}

// Limit a logical (CSS pixel) window size to the monitor that shows it. Monitor
// sizes are physical pixels, so divide them by the scale factor first.
export async function clampToMonitor(w: number, h: number): Promise<[number, number]> {
  try {
    const m = await currentMonitor();
    if (!m) return [w, h];
    const f = m.scaleFactor || 1;
    return [Math.min(w, m.size.width / f), Math.min(h, m.size.height / f)];
  } catch {
    return [w, h];
  }
}
