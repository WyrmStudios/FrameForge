import type { ReactNode } from "react";

interface KeepMountedWhenHiddenProps {
  active: boolean;
  children: ReactNode;
}

// Children stay mounted while hidden so their background listeners keep running.
export default function KeepMountedWhenHidden({ active, children }: KeepMountedWhenHiddenProps) {
  return <div style={{ display: active ? "contents" : "none" }}>{children}</div>;
}
