import { useEffect, useRef, type MouseEvent, type SyntheticEvent } from "react";

/** The dialog has no close call: the parent unmounts it. */
export function useModal(onClose: () => void) {
  const ref = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    const dialog = ref.current;
    dialog?.showModal();
    return () => dialog?.close();
  }, []);
  return {
    ref,
    onCancel: (e: SyntheticEvent) => { e.preventDefault(); onClose(); },
    onClick: (e: MouseEvent) => { if (e.target === e.currentTarget) onClose(); },
  };
}
