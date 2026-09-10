import { Fragment, useEffect, useRef, useState, type Dispatch, type PointerEvent as ReactPointerEvent, type SetStateAction } from "react";
import { createPortal } from "react-dom";
import type { FilterPreset, FilterPresetSettings } from "../types/filterPresets";
import type { InventoryFilters } from "../types/filters";

type InventoryPreset = Extract<FilterPreset, { module: "inventory" }>;

interface PresetDrag {
  id: string;
  pointerId: number;
  pinnedIds: string[];
  dropIndex: number;
  sourcePinned: boolean;
  targetPinned: boolean;
  startX: number;
  startY: number;
  started: boolean;
}

interface InventoryPresetsProps {
  filters: InventoryFilters;
  onFiltersChange: Dispatch<SetStateAction<InventoryFilters>>;
  filterPresets: FilterPresetSettings;
  onFilterPresetsChange: Dispatch<SetStateAction<FilterPresetSettings>>;
}

export default function InventoryPresets({ filters, onFiltersChange, filterPresets, onFilterPresetsChange }: InventoryPresetsProps) {
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
  const pinnedRowRefs = useRef(new Map<string, HTMLDivElement>());
  const dragRef = useRef<PresetDrag | null>(null);
  const [drag, setDrag] = useState<PresetDrag | null>(null);
  const inventoryPresets = filterPresets.presets.filter((preset): preset is InventoryPreset => preset.module === "inventory");
  const pinnedPresets = inventoryPresets.filter(preset => preset.pinned);
  const unpinnedPresets = inventoryPresets.filter(preset => !preset.pinned);
  const isPresetActive = (preset: InventoryPreset) =>
    Object.entries(preset.filters).every(([key, value]) => filters[key as keyof InventoryFilters] === value);

  const commitPresetOrder = (savedIds: string[], pinnedIds: string[]) => onFilterPresetsChange(current => {
    const presetsById = new Map(current.presets.filter((preset): preset is InventoryPreset => preset.module === "inventory").map(preset => [preset.id, preset]));
    const orderedIds = [...savedIds, ...pinnedIds];
    let index = 0;
    return {
      ...current,
      presets: current.presets.map(preset => {
        if (preset.module !== "inventory") return preset;
        const next = presetsById.get(orderedIds[index++])!;
        return { ...next, pinned: index > savedIds.length } as FilterPreset;
      }),
    };
  });

  const setPinnedOrder = (pinnedIds: string[]) => commitPresetOrder(
    inventoryPresets.filter(preset => !preset.pinned).map(preset => preset.id),
    pinnedIds,
  );

  const finishPinnedDrag = (commit: boolean) => {
    const activeDrag = dragRef.current;
    if (!activeDrag) return;
    dragRef.current = null;
    setDrag(null);
    if (!commit || !activeDrag.started) return;
    const pinnedIds = activeDrag.pinnedIds.filter(id => id !== activeDrag.id);
    pinnedIds.splice(activeDrag.dropIndex, 0, activeDrag.id);
    if (pinnedIds.some((id, index) => id !== activeDrag.pinnedIds[index])) setPinnedOrder(pinnedIds);
  };

  const updatePinnedDropIndex = (clientY: number) => {
    const activeDrag = dragRef.current;
    if (!activeDrag) return;
    const remainingIds = activeDrag.pinnedIds.filter(id => id !== activeDrag.id);
    const nextIndex = remainingIds.findIndex(id => {
      const row = pinnedRowRefs.current.get(id);
      return row ? clientY < row.getBoundingClientRect().top + row.getBoundingClientRect().height / 2 : false;
    });
    const dropIndex = nextIndex === -1 ? remainingIds.length : nextIndex;
    if (dropIndex !== activeDrag.dropIndex) {
      const nextDrag = { ...activeDrag, dropIndex };
      dragRef.current = nextDrag;
      setDrag(nextDrag);
    }
  };

  const close = () => {
    setOpen(false);
    setSaving(false);
    setEditingId(null);
    setDeleteId(null);
    requestAnimationFrame(() => triggerRef.current?.focus());
  };

  useEffect(() => {
    if (!open) return;
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
    const frame = requestAnimationFrame(() => {
      updatePosition();
      addButtonRef.current?.focus();
    });
    const onPointerDown = (event: PointerEvent) => {
      if (!popupRef.current?.contains(event.target as Node) && !triggerRef.current?.contains(event.target as Node)) close();
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      if (dragRef.current) {
        finishPinnedDrag(false);
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
      updatePinnedDropIndex(event.clientY);
    };
    const onPointerEnd = (event: PointerEvent) => {
      if (!dragRef.current || event.pointerId !== dragRef.current.pointerId) return;
      finishPinnedDrag(event.type === "pointerup");
    };
    document.addEventListener("pointerdown", onPointerDown);
    document.addEventListener("keydown", onKeyDown);
    document.addEventListener("pointermove", onPointerMove);
    document.addEventListener("pointerup", onPointerEnd);
    document.addEventListener("pointercancel", onPointerEnd);
    window.addEventListener("resize", updatePosition);
    return () => {
      cancelAnimationFrame(frame);
      document.removeEventListener("pointerdown", onPointerDown);
      document.removeEventListener("keydown", onKeyDown);
      document.removeEventListener("pointermove", onPointerMove);
      document.removeEventListener("pointerup", onPointerEnd);
      document.removeEventListener("pointercancel", onPointerEnd);
      window.removeEventListener("resize", updatePosition);
    };
  }, [open, saving, editingId, deleteId]);

  useEffect(() => {
    if (saving || editingId) inputRef.current?.focus();
  }, [saving, editingId]);

  const apply = (preset: InventoryPreset) => {
    // Preserve any future Inventory-only state that is not eligible for presets.
    onFiltersChange(current => ({ ...current, ...preset.filters }));
    close();
  };

  const saveName = () => {
    const trimmedName = name.trim();
    if (!trimmedName) return;
    if (saving) {
      const preset: InventoryPreset = { id: crypto.randomUUID(), name: trimmedName, module: "inventory", createdAt: Date.now(), filters: { ...filters } };
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

  const startPinnedDrag = (event: ReactPointerEvent<HTMLDivElement>, id: string) => {
    if (event.button !== 0) return;
    const activeDrag = {
      id,
      pointerId: event.pointerId,
      pinnedIds: pinnedPresets.map(preset => preset.id),
      dropIndex: pinnedPresets.findIndex(preset => preset.id === id),
      startX: event.clientX,
      startY: event.clientY,
      started: false,
      sourcePinned: true,
      targetPinned: true,
    };
    dragRef.current = activeDrag;
    event.currentTarget.setPointerCapture(event.pointerId);
  };

  const stopDrag = (event: ReactPointerEvent<HTMLElement>) => event.stopPropagation();

  const renderPresetRow = (preset: InventoryPreset, pinnedIndex?: number) => <div
    className={`inventory-presets-row${pinnedIndex === undefined ? "" : " inventory-presets-pinned-row"}`}
    key={preset.id}
    onPointerDown={pinnedIndex === undefined ? undefined : event => startPinnedDrag(event, preset.id)}
    ref={pinnedIndex === undefined ? undefined : element => {
      if (element) pinnedRowRefs.current.set(preset.id, element);
      else pinnedRowRefs.current.delete(preset.id);
    }}
  >
    <span className="inventory-presets-drag-grip" aria-hidden="true">::</span>
    {editingId === preset.id ? <form className="inventory-presets-inline-form" onPointerDown={stopDrag} onSubmit={event => { event.preventDefault(); saveName(); }}>
      <input ref={inputRef} value={name} onChange={event => setName(event.target.value)} aria-label="Preset name" />
      <button className="inventory-presets-icon" type="submit" disabled={!name.trim()} aria-label="Save preset name" title="Save">✓</button>
      <button className="inventory-presets-icon" type="button" onClick={() => { setEditingId(null); setName(""); }} aria-label="Cancel rename" title="Cancel">×</button>
    </form> : <button className="fchip inventory-presets-name" onPointerDown={stopDrag} onClick={() => { setEditingId(preset.id); setSaving(false); setName(preset.name); }} title="Rename preset">{preset.name}</button>}
    <button className="inventory-presets-icon" onPointerDown={stopDrag} onClick={() => apply(preset)} aria-label={`Apply ${preset.name}`} title="Apply preset">✓</button>
    <button className="inventory-presets-icon" onPointerDown={stopDrag} onClick={() => togglePin(preset.id)} aria-label={`${preset.pinned ? "Unpin" : "Pin"} ${preset.name}`} title={preset.pinned ? "Unpin" : "Pin"}>{preset.pinned ? "●" : "○"}</button>
    <button className="inventory-presets-icon inventory-presets-delete" onPointerDown={stopDrag} onClick={() => setDeleteId(preset.id)} aria-label={`Delete ${preset.name}`} title="Delete">×</button>
  </div>;

  return <>
    {pinnedPresets.map(preset => <button key={preset.id} className={`fchip inventory-preset-chip ${isPresetActive(preset) ? "fchip-on" : ""}`} onClick={() => apply(preset)}>{preset.name}</button>)}
    <button ref={triggerRef} className={`fchip inventory-preset-custom ${open ? "fchip-on" : ""}`} onClick={() => open ? close() : setOpen(true)} aria-expanded={open} aria-haspopup="dialog">Custom</button>
    {open && createPortal(
      <section ref={popupRef} className="inventory-presets-popup" style={{ left: position.left, top: position.top, maxHeight: position.maxHeight }} role="dialog" aria-label="Inventory filter presets">
        <header className="inventory-presets-header">
          <strong>Filter presets</strong>
          <button ref={addButtonRef} className="inventory-presets-action" onClick={() => { setSaving(true); setEditingId(null); setName(""); }} aria-label="Save current filters">+</button>
        </header>
        {saving && <form className="inventory-presets-form" onPointerDown={stopDrag} onSubmit={event => { event.preventDefault(); saveName(); }}>
          <input ref={inputRef} value={name} onChange={event => setName(event.target.value)} placeholder="Preset name" aria-label="Preset name" />
          <button type="submit" disabled={!name.trim()}>Save</button>
          <button type="button" onClick={() => { setSaving(false); setEditingId(null); setName(""); }}>Cancel</button>
        </form>}
        <div className="inventory-presets-list">
          {inventoryPresets.length === 0 && <p className="inventory-presets-empty">No saved presets.</p>}
          {pinnedPresets.length > 0 && <section className="inventory-presets-section" aria-label="Pinned presets">
            <h2>Pinned</h2>
            <div className="inventory-presets-table">
              {pinnedPresets.filter(preset => drag?.id !== preset.id).flatMap((preset, index) => <Fragment key={preset.id}>
                {drag?.dropIndex === index && <div className="inventory-presets-insertion" aria-hidden="true" />}
                {renderPresetRow(preset, pinnedPresets.findIndex(pinned => pinned.id === preset.id))}
              </Fragment>)}
              {drag?.dropIndex === pinnedPresets.length - 1 && <div className="inventory-presets-insertion" aria-hidden="true" />}
            </div>
          </section>}
          {unpinnedPresets.length > 0 && <section className="inventory-presets-section" aria-label="Saved presets">
            {pinnedPresets.length > 0 && <h2>Saved</h2>}
            <div className="inventory-presets-table">{unpinnedPresets.map(preset => renderPresetRow(preset))}</div>
          </section>}
        </div>
        {deleteId && <div className="inventory-presets-confirm" role="alert" onPointerDown={stopDrag}>
          <span>Delete this preset?</span>
          <button onClick={() => deletePreset(deleteId)}>Delete</button>
          <button onClick={() => setDeleteId(null)}>Cancel</button>
        </div>}
      </section>, document.body,
    )}
  </>;
}
