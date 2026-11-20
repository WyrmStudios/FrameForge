import { useEffect, useRef, useState } from "react";

/**
 * Local-echo input state that commits to the parent-owned value 150 ms after
 * the user stops typing, so expensive filter/sort recomputation over large
 * lists doesn't run on every keystroke. Mirrors the pattern already used in
 * Foundry.tsx and statistics/ItemReport.tsx.
 */
export function useDebouncedSearch(externalValue: string, onCommit: (value: string) => void, delay = 150) {
  const [value, setValue] = useState(externalValue);
  const lastCommittedRef = useRef(externalValue);

  const onCommitRef = useRef(onCommit);
  onCommitRef.current = onCommit;

  // Sync local echo when the parent value changes from elsewhere (e.g. "Clear filters").
  // Skip when externalValue is just catching up to a commit we ourselves just sent —
  // otherwise a keystroke typed in the gap between that commit firing and this effect
  // running would get clobbered back to the stale pre-keystroke value.
  useEffect(() => {
    if (externalValue === lastCommittedRef.current) return;
    setValue(externalValue);
    lastCommittedRef.current = externalValue;
  }, [externalValue]);

  useEffect(() => {
    if (value === externalValue) return;
    const id = setTimeout(() => {
      lastCommittedRef.current = value;
      onCommitRef.current(value);
    }, delay);
    return () => clearTimeout(id);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [value]);

  return [value, setValue] as const;
}
