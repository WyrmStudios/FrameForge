import { useCallback, useEffect, useRef } from "react";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow, primaryMonitor, LogicalPosition, LogicalSize } from "@tauri-apps/api/window";
import { overlayScale } from "./uiScale";
import { TAURI_EVENTS } from "../constants/tauri";

export type OverlayAnchor = "top-right" | "top-center";

const TOP_MARGIN = 20;
const RIGHT_MARGIN = 100;

/**
 * Fits a transparent overlay window to its content and parks it at `anchor`
 * on the primary monitor. The backend only shows the window; it cannot place
 * it, because the overlay scale that decides the rendered size is a frontend
 * setting.
 *
 * Returns a callback ref for the overlay's root element. It has to be a
 * callback ref: the root does not exist until a payload arrives, so a
 * mount-time effect would have nothing to observe.
 */
export function useOverlayWindow(width: number, anchor: OverlayAnchor) {
  const roRef   = useRef<ResizeObserver | null>(null);
  const rootRef = useRef<HTMLDivElement | null>(null);

  // contentRect reports the pre-zoom layout size; the drawn size is × scale.
  const place = useCallback(async (layoutHeight: number) => {
    if (layoutHeight <= 0) return;
    const s = overlayScale();
    let w = width * s;
    let h = layoutHeight * s;
    let x = 0;
    try {
      const m = await primaryMonitor();
      if (m) {
        const f = m.scaleFactor || 1;
        const monW = m.size.width / f;
        const monH = m.size.height / f;
        w = Math.min(w, monW);
        h = Math.min(h, monH);
        x = anchor === "top-right" ? monW - w - RIGHT_MARGIN : (monW - w) / 2;
      }
      const win = getCurrentWindow();
      await win.setSize(new LogicalSize(Math.round(w), Math.round(h)));
      await win.setPosition(new LogicalPosition(Math.round(Math.max(0, x)), TOP_MARGIN));
    } catch {}
  }, [width, anchor]);

  const root = useCallback((el: HTMLDivElement | null) => {
    if (roRef.current) { roRef.current.disconnect(); roRef.current = null; }
    rootRef.current = el;
    if (!el) return;
    const ro = new ResizeObserver(entries => place(Math.ceil(entries[0].contentRect.height)));
    ro.observe(el);
    roRef.current = ro;
  }, [place]);

  useEffect(() => {
    const un = listen(TAURI_EVENTS.SETTINGS_UPDATED, () => {
      const el = rootRef.current;
      if (el) place(Math.ceil(el.getBoundingClientRect().height / overlayScale()));
    });
    return () => { un.then(f => f()); };
  }, [place]);

  return root;
}
