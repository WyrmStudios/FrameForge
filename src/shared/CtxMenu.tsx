import { useState, useEffect, useCallback } from "react";

interface CtxMenuItem {
  label: string;
  action: () => void;
}
interface CtxMenuState {
  x: number;
  y: number;
  items: CtxMenuItem[];
}

export function useContextMenu() {
  const [ctxMenu, setCtxMenu] = useState<CtxMenuState | null>(null);

  useEffect(() => {
    if (!ctxMenu) return;
    const close = () => setCtxMenu(null);
    const onKey = (e: KeyboardEvent) => { if (e.key === "Escape") close(); };
    document.addEventListener("mousedown", close);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", close);
      document.removeEventListener("keydown", onKey);
    };
  }, [ctxMenu]);

  const open = useCallback((x: number, y: number, items: CtxMenuItem[]) => {
    const menuW = 180;
    const menuH = items.length * 30 + 8;
    const maxX = window.innerWidth - menuW;
    const maxY = window.innerHeight - menuH;
    setCtxMenu({ x: Math.min(x, maxX), y: Math.min(y, maxY), items });
  }, []);

  const close = useCallback(() => setCtxMenu(null), []);

  return { ctxMenu, open, close };
}

const MENU_CLASS =
  "fixed z-999 min-w-40 rounded-8 bg-surface shadow-[0_4px_16px_rgba(0,0,0,.5)] border";

const ITEM_CLASS =
  "block w-full cursor-default whitespace-nowrap border-none px-3.5 py-1.5 text-left text-foreground";

const SEP_CLASS = "h-px bg-border";

export function CtxMenu({ state, onClose }: { state: CtxMenuState; onClose: () => void }) {
  const [hovered, setHovered] = useState(false);
  return (
    <div className={`${MENU_CLASS} ${hovered ? "border-accent/50" : "border-border"}`}
         style={{ left: state.x, top: state.y }}
         onMouseDown={e => e.stopPropagation()}
         onMouseEnter={() => setHovered(true)}
         onMouseLeave={() => setHovered(false)}>
      {state.items.map((item, i) => (
        <span key={i}>
          {i > 0 && <div className={SEP_CLASS} />}
          <HoverItem onClick={() => { item.action(); onClose(); }}>
            {item.label}
          </HoverItem>
        </span>
      ))}
    </div>
  );
}

function HoverItem({ onClick, children }: { onClick: () => void; children: React.ReactNode }) {
  const [hovered, setHovered] = useState(false);
  return (
    <button className={`${ITEM_CLASS} ${hovered ? "bg-accent/15" : "bg-transparent"}`}
            onMouseEnter={() => setHovered(true)}
            onMouseLeave={() => setHovered(false)}
            onClick={onClick}>
      {children}
    </button>
  );
}
