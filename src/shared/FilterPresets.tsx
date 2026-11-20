import { Fragment, useEffect, useRef, useState, type Dispatch, type PointerEvent as ReactPointerEvent, type SetStateAction } from "react";
import { createPortal } from "react-dom";
import { FOUNDRY_FILTERS_DEFAULT, INVENTORY_FILTERS_DEFAULT, MARKET_FILTERS_DEFAULT, RELIC_FILTERS_DEFAULT } from "../constants/filters";
import type { FilterPreset, FilterPresetFiltersByModule, FilterPresetModule, FilterPresetSettings } from "../types/filterPresets";
import type { FoundryFilters, InventoryFilters, MarketFilters, RelicFilters } from "../types/filters";
import { FilterChip } from "./ui/FilterControls";

type ModulePreset<M extends FilterPresetModule> = Extract<FilterPreset, { module: M }>;
type CurrentFiltersByModule = {
  inventory: InventoryFilters;
  foundry: FoundryFilters;
  market: MarketFilters;
  relics: RelicFilters;
};

const POPUP_CONTROL_CLASS = "cursor-pointer rounded border border-border bg-white/4 text-muted [font:inherit] hover:border-accent hover:text-foreground focus-visible:outline-2 focus-visible:outline-accent focus-visible:outline-offset-1";
const POPUP_ICON_CLASS = `${POPUP_CONTROL_CLASS} w-5.75 shrink-0 p-0.75 text-xs leading-none disabled:cursor-default disabled:opacity-45`;

function presetFilters<M extends FilterPresetModule>(module: M, filters: CurrentFiltersByModule[M]): FilterPresetFiltersByModule[M] {
  if (module === "foundry") {
    const { activeCat, ...preset } = filters as FoundryFilters;
    return preset as FilterPresetFiltersByModule[M];
  }
  if (module === "market") {
    const { activeMarketTab, ...preset } = filters as MarketFilters;
    return preset as FilterPresetFiltersByModule[M];
  }
  return filters as FilterPresetFiltersByModule[M];
}

function defaultFilters<M extends FilterPresetModule>(module: M, filters: CurrentFiltersByModule[M]): CurrentFiltersByModule[M] {
  if (module === "foundry") return { ...FOUNDRY_FILTERS_DEFAULT, activeCat: (filters as FoundryFilters).activeCat } as CurrentFiltersByModule[M];
  if (module === "market") return { ...MARKET_FILTERS_DEFAULT, activeMarketTab: (filters as MarketFilters).activeMarketTab } as CurrentFiltersByModule[M];
  if (module === "relics") return RELIC_FILTERS_DEFAULT as CurrentFiltersByModule[M];
  return INVENTORY_FILTERS_DEFAULT as CurrentFiltersByModule[M];
}

function sameFilters(left: object, right: object): boolean {
  return JSON.stringify(left) === JSON.stringify(right);
}

interface PresetDrag {
  id: string;
  pointerId: number;
  sourcePinned: boolean;
  targetPinned: boolean;
  targetIndex: number;
  startX: number;
  startY: number;
  started: boolean;
}

interface FilterPresetsProps<M extends FilterPresetModule> {
  module: M;
  filters: CurrentFiltersByModule[M];
  onFiltersChange: Dispatch<SetStateAction<CurrentFiltersByModule[M]>>;
  filterPresets: FilterPresetSettings;
  onFilterPresetsChange: Dispatch<SetStateAction<FilterPresetSettings>>;
  variant?: "toolbar" | "settings";
  onOpenSettings?: (module: M) => void;
}

export default function FilterPresets<M extends FilterPresetModule>({ module, filters, onFiltersChange, filterPresets, onFilterPresetsChange, variant = "toolbar", onOpenSettings }: FilterPresetsProps<M>) {
  const [open, setOpen] = useState(false);
  const [saving, setSaving] = useState(false);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [name, setName] = useState("");
  const [deleteId, setDeleteId] = useState<string | null>(null);
  const [position, setPosition] = useState({ left: 8, top: 8, maxHeight: 240 });
  const triggerRef = useRef<HTMLButtonElement>(null);
  const popupRef = useRef<HTMLDivElement>(null);
  const addButtonRef = useRef<HTMLButtonElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const presetRowRefs = useRef(new Map<string, HTMLDivElement>());
  const sectionRefs = useRef(new Map<boolean, HTMLElement>());
  const dragRef = useRef<PresetDrag | null>(null);
  const [drag, setDrag] = useState<PresetDrag | null>(null);
  const [filtersBeforeClear, setFiltersBeforeClear] = useState<CurrentFiltersByModule[M] | null>(null);
  const [filtersBeforePreset, setFiltersBeforePreset] = useState<CurrentFiltersByModule[M] | null>(null);
  const modulePresets = filterPresets.presets.filter((preset): preset is ModulePreset<M> => preset.module === module);
  const pinnedPresets = modulePresets.filter(preset => preset.pinned);
  const unpinnedPresets = modulePresets.filter(preset => !preset.pinned);
  const isPresetActive = (preset: ModulePreset<M>) =>
    Object.entries(preset.filters).every(([key, value]) => JSON.stringify(filters[key as keyof FilterPresetFiltersByModule[M]]) === JSON.stringify(value));

  const commitPresetOrder = (savedIds: string[], pinnedIds: string[]) => onFilterPresetsChange(current => {
    const presetsById = new Map(current.presets.filter((preset): preset is ModulePreset<M> => preset.module === module).map(preset => [preset.id, preset]));
    const orderedIds = [...savedIds, ...pinnedIds];
    let index = 0;
    return {
      ...current,
      presets: current.presets.map(preset => {
        if (preset.module !== module) return preset;
        const next = presetsById.get(orderedIds[index++])!;
        return { ...next, pinned: index > savedIds.length } as FilterPreset;
      }),
    };
  });

  const finishPresetDrag = (commit: boolean) => {
    const activeDrag = dragRef.current;
    if (!activeDrag) return;
    dragRef.current = null;
    setDrag(null);
    if (!commit || !activeDrag.started) return;
    const savedIds = unpinnedPresets.map(preset => preset.id).filter(id => id !== activeDrag.id);
    const pinnedIds = pinnedPresets.map(preset => preset.id).filter(id => id !== activeDrag.id);
    const targetIds = activeDrag.targetPinned ? pinnedIds : savedIds;
    targetIds.splice(activeDrag.targetIndex, 0, activeDrag.id);
    const sourceIds = activeDrag.sourcePinned ? pinnedPresets.map(preset => preset.id) : unpinnedPresets.map(preset => preset.id);
    const nextSourceIds = activeDrag.sourcePinned ? pinnedIds : savedIds;
    if (activeDrag.sourcePinned !== activeDrag.targetPinned || sourceIds.some((id, index) => id !== nextSourceIds[index])) {
      commitPresetOrder(savedIds, pinnedIds);
    }
  };

  const updatePresetDropTarget = (clientX: number, clientY: number) => {
    const activeDrag = dragRef.current;
    if (!activeDrag) return;
    const targetPinned = [...sectionRefs.current.entries()].find(([, section]) => {
      const rect = section.getBoundingClientRect();
      return clientX >= rect.left && clientX <= rect.right && clientY >= rect.top && clientY <= rect.bottom;
    })?.[0] ?? activeDrag.targetPinned;
    const targetPresets = (targetPinned ? pinnedPresets : unpinnedPresets).filter(preset => preset.id !== activeDrag.id);
    const nextIndex = targetPresets.findIndex(preset => {
      const row = presetRowRefs.current.get(preset.id);
      return row ? clientY < row.getBoundingClientRect().top + row.getBoundingClientRect().height / 2 : false;
    });
    const targetIndex = nextIndex === -1 ? targetPresets.length : nextIndex;
    if (targetPinned !== activeDrag.targetPinned || targetIndex !== activeDrag.targetIndex) {
      const nextDrag = { ...activeDrag, targetPinned, targetIndex };
      dragRef.current = nextDrag;
      setDrag(nextDrag);
    }
  };

  const close = () => {
    setOpen(false);
    setSaving(false);
    setEditingId(null);
    setDeleteId(null);
    if (variant === "toolbar") requestAnimationFrame(() => triggerRef.current?.focus());
  };

  useEffect(() => {
    if (variant === "toolbar" && !open) return;
    const updatePosition = () => {
      const trigger = triggerRef.current;
      const popup = popupRef.current;
      if (!trigger || !popup) return;
      const triggerRect = trigger.getBoundingClientRect();
      const margin = 8;
      const maxHeight = Math.max(160, window.innerHeight - margin * 2);
      const popupHeight = Math.min(popup.scrollHeight, maxHeight);
      const below = window.innerHeight - triggerRect.bottom - margin;
      const above = triggerRect.top - margin;
      const opensBelow = below >= popupHeight || below >= above;
      setPosition({
        left: Math.max(margin, Math.min(triggerRect.left, window.innerWidth - 310 - margin)),
        top: opensBelow ? triggerRect.bottom + margin : Math.max(margin, triggerRect.top - popupHeight - margin),
        maxHeight,
      });
    };
    const frame = variant === "toolbar" ? requestAnimationFrame(() => {
      updatePosition();
      addButtonRef.current?.focus();
    }) : 0;
    const onPointerDown = (event: PointerEvent) => {
      if (variant === "toolbar" && !dragRef.current && !popupRef.current?.contains(event.target as Node) && !triggerRef.current?.contains(event.target as Node)) close();
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      if (dragRef.current) {
        finishPresetDrag(false);
        return;
      }
      if (saving || editingId) {
        setSaving(false);
        setEditingId(null);
        setName("");
      } else if (deleteId) {
        setDeleteId(null);
      } else {
        close();
      }
    };
    const onPointerMove = (event: PointerEvent) => {
      const activeDrag = dragRef.current;
      if (!activeDrag || event.pointerId !== activeDrag.pointerId) return;
      if (!activeDrag.started) {
        if (Math.hypot(event.clientX - activeDrag.startX, event.clientY - activeDrag.startY) < 4) return;
        const startedDrag = { ...activeDrag, started: true };
        dragRef.current = startedDrag;
        setDrag(startedDrag);
      }
      event.preventDefault();
      updatePresetDropTarget(event.clientX, event.clientY);
    };
    const onPointerEnd = (event: PointerEvent) => {
      if (!dragRef.current || event.pointerId !== dragRef.current.pointerId) return;
      finishPresetDrag(event.type === "pointerup");
    };
    document.addEventListener("pointerdown", onPointerDown);
    document.addEventListener("keydown", onKeyDown);
    document.addEventListener("pointermove", onPointerMove);
    document.addEventListener("pointerup", onPointerEnd);
    document.addEventListener("pointercancel", onPointerEnd);
    if (variant === "toolbar") window.addEventListener("resize", updatePosition);
    return () => {
      cancelAnimationFrame(frame);
      document.removeEventListener("pointerdown", onPointerDown);
      document.removeEventListener("keydown", onKeyDown);
      document.removeEventListener("pointermove", onPointerMove);
      document.removeEventListener("pointerup", onPointerEnd);
      document.removeEventListener("pointercancel", onPointerEnd);
      if (variant === "toolbar") window.removeEventListener("resize", updatePosition);
    };
  }, [variant, open, saving, editingId, deleteId]);

  useEffect(() => {
    if (saving || editingId) inputRef.current?.focus();
  }, [saving, editingId]);

  useEffect(() => {
    if (filtersBeforeClear && !sameFilters(filters, defaultFilters(module, filters))) setFiltersBeforeClear(null);
  }, [module, filters, filtersBeforeClear]);

  const clearFilters = () => {
    if (filtersBeforeClear) {
      onFiltersChange(filtersBeforeClear);
      setFiltersBeforeClear(null);
      return;
    }
    setFiltersBeforeClear({ ...filters });
    onFiltersChange(current => defaultFilters(module, current));
  };
  const canClearFilters = filtersBeforeClear !== null || !sameFilters(filters, defaultFilters(module, filters));

  const apply = (preset: ModulePreset<M>, activeChip = false) => {
    if (activeChip && isPresetActive(preset)) {
      if (filterPresets.restorePreviousFiltersOnPresetClick && filtersBeforePreset) {
        onFiltersChange(filtersBeforePreset);
        setFiltersBeforePreset(null);
      } else {
        clearFilters();
      }
      return;
    }
    // The module-specific preset payload excludes UI state that must stay local.
    setFiltersBeforeClear(null);
    setFiltersBeforePreset({ ...filters });
    onFiltersChange(current => ({ ...current, ...preset.filters }) as CurrentFiltersByModule[M]);
    close();
  };

  const saveName = () => {
    const trimmedName = name.trim();
    if (!trimmedName) return;
    if (saving) {
      const preset: ModulePreset<M> = { id: crypto.randomUUID(), name: trimmedName, module, createdAt: Date.now(), filters: presetFilters(module, filters) } as unknown as ModulePreset<M>;
      onFilterPresetsChange(current => ({ ...current, presets: [...current.presets, preset] }));
    } else if (editingId) {
      onFilterPresetsChange(current => ({ ...current, presets: current.presets.map(preset => preset.id === editingId ? { ...preset, name: trimmedName } as FilterPreset : preset) }));
    }
    setSaving(false);
    setEditingId(null);
    setName("");
  };

  const togglePin = (id: string) => onFilterPresetsChange(current => ({
    ...current,
    presets: current.presets.map(preset => preset.id === id ? { ...preset, pinned: !preset.pinned } as FilterPreset : preset),
  }));

  const deletePreset = (id: string) => {
    onFilterPresetsChange(current => ({ ...current, presets: current.presets.filter(preset => preset.id !== id) }));
    setDeleteId(null);
  };

  const startPresetDrag = (event: ReactPointerEvent<HTMLSpanElement>, preset: ModulePreset<M>) => {
    if (event.button !== 0) return;
    event.preventDefault();
    const activeDrag = {
      id: preset.id,
      pointerId: event.pointerId,
      targetIndex: (preset.pinned ? pinnedPresets : unpinnedPresets).findIndex(candidate => candidate.id === preset.id),
      startX: event.clientX,
      startY: event.clientY,
      started: false,
      sourcePinned: !!preset.pinned,
      targetPinned: !!preset.pinned,
    };
    dragRef.current = activeDrag;
    event.currentTarget.setPointerCapture(event.pointerId);
  };

  const stopDrag = (event: ReactPointerEvent<HTMLElement>) => event.stopPropagation();

  const renderPresetRow = (preset: ModulePreset<M>) => <div
    className="flex items-center gap-1 p-0.75"
    key={preset.id}
    ref={element => {
      if (element) presetRowRefs.current.set(preset.id, element);
      else presetRowRefs.current.delete(preset.id);
    }}
  >
    <span className="inline-block w-3 touch-none select-none whitespace-pre-wrap font-mono text-13/[9px] font-bold tracking-[-2px] text-muted cursor-grab active:cursor-grabbing" onPointerDown={event => startPresetDrag(event, preset)} aria-hidden="true">::</span>
    {editingId === preset.id ? <form className="flex min-w-0 flex-1 items-center gap-1" onPointerDown={stopDrag} onSubmit={event => { event.preventDefault(); saveName(); }}>
      <input className="min-w-0 flex-1 rounded-xl border border-accent bg-background px-1.25 py-0.75 text-foreground [font:inherit] focus:outline-none focus-visible:outline-2 focus-visible:outline-accent focus-visible:outline-offset-1" ref={inputRef} value={name} onChange={event => setName(event.target.value)} aria-label="Preset name" />
      <button className={POPUP_ICON_CLASS} type="submit" disabled={!name.trim()} aria-label="Save preset name" title="Save">✓</button>
      <button className={POPUP_ICON_CLASS} type="button" onClick={() => { setEditingId(null); setName(""); }} aria-label="Cancel rename" title="Cancel">×</button>
    </form> : <FilterChip className="min-w-0 flex-1 overflow-hidden text-left text-ellipsis" onPointerDown={stopDrag} onClick={() => apply(preset)} title="Apply preset">{preset.name}</FilterChip>}
    <button className={POPUP_ICON_CLASS} onPointerDown={stopDrag} onClick={() => { setEditingId(preset.id); setSaving(false); setName(preset.name); }} aria-label={`Rename ${preset.name}`} title="Rename preset">✎</button>
    <button className={POPUP_ICON_CLASS} onPointerDown={stopDrag} onClick={() => togglePin(preset.id)} aria-label={`${preset.pinned ? "Unpin" : "Pin"} ${preset.name}`} title={preset.pinned ? "Unpin" : "Pin"}>{preset.pinned ? "●" : "○"}</button>
    <button className={`${POPUP_ICON_CLASS} hover:border-red hover:text-red`} onPointerDown={stopDrag} onClick={() => setDeleteId(preset.id)} aria-label={`Delete ${preset.name}`} title="Delete">×</button>
  </div>;

  const manager = <section ref={popupRef} className={variant === "toolbar" ? "fixed z-700 flex w-77.5 max-w-[calc(100vw-16px)] flex-col overflow-hidden rounded-lg border border-border bg-surface shadow-[0_8px_28px_rgba(0,0,0,.7)]" : "flex min-h-0 flex-1 flex-col"} style={variant === "toolbar" ? { left: position.left, top: position.top, maxHeight: position.maxHeight } : undefined} role="dialog" aria-label={`${module} filter presets`}>
        <header className="flex items-center gap-2 border-b border-border px-2.5 py-2.25 text-xs">
          <strong className="flex-1">Filter presets</strong>
          {variant === "toolbar" && <>
            <button className={`${POPUP_CONTROL_CLASS} h-5.75 w-5.75 text-base leading-none`} onClick={() => { close(); onOpenSettings?.(module); }} aria-label="Manage presets in Settings" title="Manage presets">⚙</button>
            <button ref={addButtonRef} className={`${POPUP_CONTROL_CLASS} h-5.75 w-5.75 text-base leading-none`} onClick={() => { setSaving(true); setEditingId(null); setName(""); }} aria-label="Save current filters">+</button>
          </>}
        </header>
        {saving && <form className="flex gap-1.25 border-b border-border px-2.5 py-2" onPointerDown={stopDrag} onSubmit={event => { event.preventDefault(); saveName(); }}>
          <input className="min-w-0 flex-1 rounded border border-border bg-background px-1.5 py-1 text-foreground [font:inherit] focus:border-accent focus:outline-none focus-visible:outline-2 focus-visible:outline-accent focus-visible:outline-offset-1" ref={inputRef} value={name} onChange={event => setName(event.target.value)} placeholder="Preset name" aria-label="Preset name" />
          <button className={`${POPUP_CONTROL_CLASS} px-1.5 py-1 text-11 disabled:cursor-default disabled:opacity-50`} type="submit" disabled={!name.trim()}>Save</button>
          <button className={`${POPUP_CONTROL_CLASS} px-1.5 py-1 text-11`} type="button" onClick={() => { setSaving(false); setEditingId(null); setName(""); }}>Cancel</button>
        </form>}
        <div className="min-h-0 flex-1 overflow-y-auto p-1">
          {modulePresets.length === 0 && <p className="px-1.5 py-2 text-11 text-muted">No saved presets.</p>}
          <section className="py-0.75" ref={element => {
            if (element) sectionRefs.current.set(false, element);
            else sectionRefs.current.delete(false);
          }} aria-label="Saved presets">
            <h2 className="px-1.5 py-0.75 text-10 font-bold uppercase tracking-0.06 text-muted">Saved</h2>
            <div className="flex flex-col">
              {unpinnedPresets.filter(preset => !drag?.started || drag.id !== preset.id).flatMap((preset, index) => <Fragment key={preset.id}>
                {drag?.started && !drag.targetPinned && drag.targetIndex === index && <div className="mx-0.75 my-px h-0.5 rounded-1 bg-accent" aria-hidden="true" />}
                {renderPresetRow(preset)}
              </Fragment>)}
              {drag?.started && !drag.targetPinned && drag.targetIndex === unpinnedPresets.filter(preset => preset.id !== drag.id).length && <div className="mx-0.75 my-px h-0.5 rounded-1 bg-accent" aria-hidden="true" />}
            </div>
          </section>
          <section className="py-0.75" ref={element => {
            if (element) sectionRefs.current.set(true, element);
            else sectionRefs.current.delete(true);
          }} aria-label="Pinned presets">
            <h2 className="px-1.5 py-0.75 text-10 font-bold uppercase tracking-0.06 text-muted">Pinned</h2>
            <div className="flex flex-col">
              {pinnedPresets.filter(preset => !drag?.started || drag.id !== preset.id).flatMap((preset, index) => <Fragment key={preset.id}>
                {drag?.started && drag.targetPinned && drag.targetIndex === index && <div className="mx-0.75 my-px h-0.5 rounded-1 bg-accent" aria-hidden="true" />}
                {renderPresetRow(preset)}
              </Fragment>)}
              {drag?.started && drag.targetPinned && drag.targetIndex === pinnedPresets.filter(preset => preset.id !== drag.id).length && <div className="mx-0.75 my-px h-0.5 rounded-1 bg-accent" aria-hidden="true" />}
            </div>
          </section>
        </div>
        {deleteId && <div className="flex items-center gap-1.25 border-t border-border px-2.5 py-2 text-11" role="alert" onPointerDown={stopDrag}>
          <span className="flex-1">Delete this preset?</span>
          <button className={`${POPUP_CONTROL_CLASS} border-danger/55 px-1.5 py-1 text-11 text-red hover:border-red hover:text-red`} onClick={() => deletePreset(deleteId)}>Delete</button>
          <button className={`${POPUP_CONTROL_CLASS} px-1.5 py-1 text-11`} onClick={() => setDeleteId(null)}>Cancel</button>
        </div>}
      </section>;

  if (variant === "settings") return manager;
  return <>
    {pinnedPresets.map(preset => <FilterChip key={preset.id} className="max-w-37.5 overflow-hidden text-ellipsis" active={isPresetActive(preset)} onClick={() => apply(preset, true)}>{preset.name}</FilterChip>)}
    <FilterChip ref={triggerRef} className="inventory-preset-custom" active={open} onClick={() => open ? close() : setOpen(true)} aria-expanded={open} aria-haspopup="dialog">Custom</FilterChip>
    <FilterChip reset onClick={clearFilters} disabled={!canClearFilters}>{filtersBeforeClear ? "Restore filters" : "Clear filters"}</FilterChip>
    {open && createPortal(manager, document.body)}
  </>;
}
