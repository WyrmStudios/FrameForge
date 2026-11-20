import { invoke } from "@tauri-apps/api/core";
import { emit, listen } from "@tauri-apps/api/event";
import { LogicalPosition, LogicalSize, availableMonitors } from "@tauri-apps/api/window";
import { TAURI_COMMANDS, TAURI_EVENTS } from "../constants/tauri";
import { ensureRivenWindow, rivenPlacement } from "./rivenWindow";
import { overlayScale } from "./uiScale";

type Rect = [number, number, number, number];
let rewardPreviewRequest = 0;
let pickPreviewRequest = 0;
let rivenPreviewRequest = 0;

async function rectOrScreen(): Promise<Rect> {
  try {
    return await invoke<Rect>("get_warframe_window_rect");
  } catch {
    try {
      const m = (await availableMonitors())[0];
      if (m) return [m.position.x, m.position.y, m.size.width, m.size.height];
    } catch {}
    return [0, 0, 1920, 1080];
  }
}

export async function showRewardOverlay(): Promise<void> {
  const request = ++rewardPreviewRequest;
  const [wx, wy, ww, wh] = await rectOrScreen();
  if (request !== rewardPreviewRequest) return;
  const offsetY = Math.round(wh * 0.60);
  const stripH = Math.min(Math.round(wh * 0.30 * overlayScale()), wh - offsetY);
  try {
    await invoke("show_overlay_window", { x: wx, y: wy + offsetY, w: ww, h: stripH });
    if (request !== rewardPreviewRequest) {
      await emit(TAURI_EVENTS.RELIC_REWARD_PREVIEW_CLOSE, {});
      return;
    }
    await emit(TAURI_EVENTS.RELIC_REWARD_PREVIEW, {});
  } catch {}
}

export async function hideRewardOverlay(): Promise<void> {
  rewardPreviewRequest += 1;
  await emit(TAURI_EVENTS.RELIC_REWARD_PREVIEW_CLOSE, {});
}

export async function showPickOverlay(): Promise<void> {
  const request = ++pickPreviewRequest;
  await emit(TAURI_EVENTS.RELIC_PICK_PREVIEW, {});
  if (request !== pickPreviewRequest) return;
  await invoke(TAURI_COMMANDS.SHOW_RELIC_PICK).catch(() => {});
  if (request !== pickPreviewRequest) await emit(TAURI_EVENTS.RELIC_PICK_PREVIEW_CLOSE, {});
}

export async function placePickOverlay(): Promise<void> {
  await showPickOverlay();
}

export async function hidePickOverlay(): Promise<void> {
  pickPreviewRequest += 1;
  await emit(TAURI_EVENTS.RELIC_PICK_PREVIEW_CLOSE, {});
}

async function ensurePlacedRiven(hidden = false): Promise<{ win: import("@tauri-apps/api/webviewWindow").WebviewWindow; fresh: boolean } | null> {
  const [wx, wy, , wh] = await rectOrScreen();
  const result = await ensureRivenWindow(wx, wy, wh, hidden);
  if (!result) return null;
  if (!result.fresh) {
    try {
      const p = await rivenPlacement(wx, wy, wh);
      await result.win.setPosition(new LogicalPosition(p.x, p.y));
      await result.win.setSize(new LogicalSize(p.width, p.height));
    } catch {}
  }
  return result;
}

export async function showRivenOverlay(): Promise<void> {
  await ensurePlacedRiven();
}

export async function hideRivenOverlay(): Promise<void> {
  rivenPreviewRequest += 1;
  await emit(TAURI_EVENTS.RIVEN_PREVIEW_CLOSE, {});
}

export async function showRivenDummy(): Promise<void> {
  const request = ++rivenPreviewRequest;
  const placed = await ensurePlacedRiven(true);
  if (!placed) return;
  if (placed.fresh) {
    const ready = await new Promise<boolean>(resolve => {
      let unlisten: (() => void) | undefined;
      let settled = false;
      const finish = (isReady: boolean) => {
        if (settled) return;
        settled = true;
        clearTimeout(timer);
        unlisten?.();
        resolve(isReady);
      };
      const timer = setTimeout(() => finish(false), 3000);
      listen(TAURI_EVENTS.RIVEN_WINDOW_READY, () => finish(true))
        .then(fn => { unlisten = fn; if (settled) fn(); })
        .catch(() => finish(false));
    });
    if (request !== rivenPreviewRequest) return;
    if (!ready) {
      await emit(TAURI_EVENTS.RIVEN_PREVIEW_CLOSE, {});
      return;
    }
  }

  if (request !== rivenPreviewRequest) {
    await emit(TAURI_EVENTS.RIVEN_PREVIEW_CLOSE, {});
    return;
  }
  await emit(TAURI_EVENTS.RIVEN_PREVIEW_START, {});
  await emit(TAURI_EVENTS.RIVEN_ANALYSIS_UPDATE, {
    dummy: true,
    analysis: {
      weapon: "Tenora Prime",
      matched_positives: ["Critical Chance", "Damage to Grineer"],
      missing_positives: ["Multi-shot"],
      safe_negatives_present: ["Zoom"],
      harmful_negatives: ["Damage to Corpus"],
      total_wanted: 4,
      score: 78,
      verdict: "Strong roll",
      notes: "Dummy payload",
      alternatives: [],
    },
    rollCount: 4,
    ocrRaw: "TENORA PRIME\n+180% Critical Chance",
    weapon: "Tenora Prime",
    positives: ["+180% Critical Chance", "+145% Damage to Grineer"],
    negatives: ["-20% Damage to Corpus"],
    rolledStats: [
      { name: "Critical Chance", value: "+180%", positive: true },
      { name: "Damage to Grineer", value: "+145%", positive: true },
      { name: "Multi-shot", value: "+85%", positive: true },
      { name: "Damage to Corpus", value: "-20%", positive: false },
    ],
    originalStats: [
      { name: "Critical Chance", value: "+153%", positive: true },
      { name: "Damage to Grineer", value: "+121%", positive: true },
      { name: "Multi-shot", value: "+77%", positive: true },
      { name: "Zoom", value: "-10%", positive: false },
    ],
    isComparison: false,
  });
  await placed.win.show().catch(() => {});
}
