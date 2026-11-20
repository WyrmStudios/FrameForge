import { useEffect, useRef, type RefObject } from "react";

export function useClickOutside(ref: RefObject<HTMLElement | null>, onClose: () => void, active = true) {
  const close = useRef(onClose);
  close.current = onClose;
  useEffect(() => {
    if (!active) return;
    const onDown = (e: MouseEvent) => { if (!ref.current?.contains(e.target as Node)) close.current(); };
    const onKey = (e: KeyboardEvent) => { if (e.key === "Escape") close.current(); };
    document.addEventListener("mousedown", onDown);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", onDown);
      document.removeEventListener("keydown", onKey);
    };
  }, [ref, active]);
}
