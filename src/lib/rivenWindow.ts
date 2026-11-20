import { invoke } from "@tauri-apps/api/core";
import { WebviewWindow } from "@tauri-apps/api/webviewWindow";
import { LogicalSize, availableMonitors } from "@tauri-apps/api/window";
import { overlayScale } from "./uiScale";
import { APP_TITLE } from "../constants/app";
import { PREFERENCE_KEYS } from "../constants/preferences";
import { DEFAULT_OVERLAY_OFFSETS, parseOverlayOffsets, type OverlayOffsets } from "../types/settings";

// ── Riven overlay — module-level window management ────────────────────────────
// Stored OUTSIDE React so StrictMode remounts don't destroy/recreate the window.
let _rivenWin: WebviewWindow | null = null;
let _rivenRollCount = 0;
let _rivenLastTriggerMs = 0;
let _rivenManualTrigger: (() => void) | null = null;

export function checkRivenNow() { _rivenManualTrigger?.(); }

export function setRivenManualTrigger(fn: (() => void) | null) {
  _rivenManualTrigger = fn;
}

export function incrementRivenRollCount() {
  _rivenRollCount++;
  return _rivenRollCount;
}

export function getRivenRollCount() {
  return _rivenRollCount;
}

export function getRivenLastTriggerMs() {
  return _rivenLastTriggerMs;
}

export function setRivenLastTriggerMs(ms: number) {
  _rivenLastTriggerMs = ms;
}

export async function resizeRivenForScale() {
  const win = _rivenWin;
  if (!win) return;
  try {
    const factor = await win.scaleFactor();
    const cur = (await win.innerSize()).toLogical(factor);
    await win.setSize(new LogicalSize(Math.round(300 * overlayScale()), cur.height));
  } catch {}
}

export function rivenWinHide(reason = "rivenWinHide", log = true) {
  const win = _rivenWin;
  if (!win) { return; }
  if (log) invoke("ocr_riven_log_error", { error: `[HIDE] ${reason}` }).catch(() => {});
  _rivenWin = null;
  win.close().catch(() => {});
}

/** Position offsets from Settings → Overlays; the main window mirrors them to localStorage. */
function savedOffsets(): OverlayOffsets {
  try {
    return parseOverlayOffsets(JSON.parse(localStorage.getItem(PREFERENCE_KEYS.OVERLAY_OFFSETS) ?? "null"));
  } catch {
    return DEFAULT_OVERLAY_OFFSETS;
  }
}

/**
 * The Warframe rect arrives in physical pixels, but WebviewWindow takes
 * x/y/width/height in logical pixels, so it has to be divided by the scale
 * factor of the monitor holding that rect.
 */
async function scaleFactorAt(x: number, y: number): Promise<number> {
  try {
    const monitors = await availableMonitors();
    const hit = monitors.find(m =>
      x >= m.position.x && x < m.position.x + m.size.width &&
      y >= m.position.y && y < m.position.y + m.size.height);
    return hit?.scaleFactor ?? monitors[0]?.scaleFactor ?? 1;
  } catch {
    return 1;
  }
}

/**
 * Where the riven window belongs for a given game rect (physical pixels):
 * x/y/width/height in logical pixels, offsets applied on top (issue #73).
 */
export async function rivenPlacement(
  wx: number, wy: number, wh: number,
): Promise<{ x: number; y: number; width: number; height: number }> {
  const off = savedOffsets();
  const f = await scaleFactorAt(wx, wy);
  const lx = wx / f, ly = wy / f, lh = wh / f;
  return {
    x: Math.round(lx + 10 + off.rivenX),
    y: Math.round(ly + lh * 0.20 + off.rivenY),
    width: Math.round(300 * overlayScale()),
    height: Math.round(lh * 0.60),
  };
}

export async function ensureRivenWindow(wx: number, wy: number, wh: number, hidden = false): Promise<{ win: WebviewWindow; fresh: boolean } | null> {
  // 1. Existing valid handle
  if (_rivenWin) return { win: _rivenWin, fresh: false };

  // 2. Window exists but JS lost reference (HMR, page reload)
  const existing = await WebviewWindow.getByLabel("riven-overlay").catch(() => null);
  if (existing) {
    _rivenWin = existing;
    _rivenWin.once("tauri://destroyed", () => { _rivenWin = null; });
    return { win: _rivenWin, fresh: false };
  }

  // 3. Create fresh at the correct position. Settings previews stay hidden until
  // their listener is ready so a cancelled preview cannot flash a scan window.
  try {
    const p = await rivenPlacement(wx, wy, wh);
    _rivenWin = new WebviewWindow("riven-overlay", {
      url: `index.html#rivenoverlay`,
      title: `${APP_TITLE} Riven`,
      transparent: true, decorations: false,
      alwaysOnTop: true, skipTaskbar: true,
      visible: !hidden,
      resizable: false, focus: false,
      x: p.x, y: p.y, width: p.width, height: p.height,
    });
    _rivenWin.once("tauri://destroyed", () => { _rivenWin = null; });
    return { win: _rivenWin, fresh: true };
  } catch {
    _rivenWin = null;
    return null;
  }
}
