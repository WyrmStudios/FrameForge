import { useEffect, useRef, useState, type Dispatch, type SetStateAction } from "react";
import { createPortal } from "react-dom";
import type { FilterPreset, FilterPresetSettings } from "../types/filterPresets";
import type { InventoryFilters } from "../types/filters";

type InventoryPreset = Extract<FilterPreset, { module: "inventory" }>;

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
  const inventoryPresets = filterPresets.presets.filter((preset): preset is InventoryPreset => preset.module === "inventory");
  const pinnedPresets = inventoryPresets.filter(preset => preset.pinned);

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
    document.addEventListener("pointerdown", onPointerDown);
    document.addEventListener("keydown", onKeyDown);
    window.addEventListener("resize", updatePosition);
    return () => {
      cancelAnimationFrame(frame);
      document.removeEventListener("pointerdown", onPointerDown);
      document.removeEventListener("keydown", onKeyDown);
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

  return <>
    {pinnedPresets.map(preset => <button key={preset.id} className="fchip inventory-preset-chip" onClick={() => apply(preset)}>{preset.name}</button>)}
    <button ref={triggerRef} className={`fchip inventory-preset-custom ${open ? "fchip-on" : ""}`} onClick={() => open ? close() : setOpen(true)} aria-expanded={open} aria-haspopup="dialog">Custom</button>
    {open && createPortal(
      <section ref={popupRef} className="inventory-presets-popup" style={{ left: position.left, top: position.top, maxHeight: position.maxHeight }} role="dialog" aria-label="Inventory filter presets">
        <header className="inventory-presets-header">
          <strong>Filter presets</strong>
          <button ref={addButtonRef} className="inventory-presets-action" onClick={() => { setSaving(true); setEditingId(null); setName(""); }} aria-label="Save current filters">+</button>
        </header>
        {(saving || editingId) && <form className="inventory-presets-form" onSubmit={event => { event.preventDefault(); saveName(); }}>
          <input ref={inputRef} value={name} onChange={event => setName(event.target.value)} placeholder="Preset name" aria-label="Preset name" />
          <button type="submit" disabled={!name.trim()}>Save</button>
          <button type="button" onClick={() => { setSaving(false); setEditingId(null); setName(""); }}>Cancel</button>
        </form>}
        <div className="inventory-presets-list">
          {inventoryPresets.length === 0 && <p className="inventory-presets-empty">No saved presets.</p>}
          {inventoryPresets.map(preset => <div className="inventory-presets-row" key={preset.id}>
            <button className="inventory-presets-name" onClick={() => apply(preset)} title="Apply preset">{preset.name}</button>
            <button className="inventory-presets-icon" onClick={() => { setEditingId(preset.id); setSaving(false); setName(preset.name); }} aria-label={`Rename ${preset.name}`} title="Rename">Edit</button>
            <button className="inventory-presets-icon" onClick={() => togglePin(preset.id)} aria-label={`${preset.pinned ? "Unpin" : "Pin"} ${preset.name}`} title={preset.pinned ? "Unpin" : "Pin"}>{preset.pinned ? "Unpin" : "Pin"}</button>
            <button className="inventory-presets-icon inventory-presets-delete" onClick={() => setDeleteId(preset.id)} aria-label={`Delete ${preset.name}`} title="Delete">Delete</button>
          </div>)}
        </div>
        {deleteId && <div className="inventory-presets-confirm" role="alert">
          <span>Delete this preset?</span>
          <button onClick={() => deletePreset(deleteId)}>Delete</button>
          <button onClick={() => setDeleteId(null)}>Cancel</button>
        </div>}
      </section>, document.body,
    )}
  </>;
}
