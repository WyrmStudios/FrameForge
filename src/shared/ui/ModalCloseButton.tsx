import type { ComponentPropsWithRef, ReactNode } from "react";

const CLOSE_BASE =
  "bg-transparent border-0 cursor-pointer text-muted text-14 px-1.5 py-0.5 shrink-0 rounded-4 transition-colors duration-100 hover:text-foreground hover:bg-white/6";

export function ModalCloseButton({ className = "", children, ...rest }: ComponentPropsWithRef<"button"> & { children?: ReactNode }) {
  return (
    <button className={`${CLOSE_BASE}${className ? ` ${className}` : ""}`} {...rest}>
      {children}
    </button>
  );
}
