interface InventorySidebarProps {
  categories: { id: string; label: string }[];
  category: string;
  categoryCounts: { owned: Record<string, number>; total: Record<string, number> };
  onCategoryChange: (category: string) => void;
  itemCount: number;
  recipeCount: number;
  onFetch: () => void | Promise<void>;
  fetching: boolean;
  fetchMsg: string;
}

export default function InventorySidebar({
  categories, category, categoryCounts, onCategoryChange, itemCount, recipeCount, onFetch, fetching, fetchMsg,
}: InventorySidebarProps) {
  return (
    <aside className="sidebar">
      <div className="sidebar-section-label">Categories</div>
      {categories.map(cat => {
        const owned = categoryCounts.owned[cat.id] ?? 0;
        const total = categoryCounts.total[cat.id] ?? 0;
        return (
          <button key={cat.id} className={`cat-btn ${category === cat.id ? "cat-active" : ""}`} onClick={() => onCategoryChange(cat.id)}>
            <span className="cat-label">{cat.label}</span>
            <span className="cat-count">
              {owned > 0 ? <span className="cat-owned">{owned}</span> : null}
              {owned > 0 && <span className="cat-sep">/</span>}
              <span className="cat-total">{total}</span>
            </span>
          </button>
        );
      })}
      <div className="sidebar-divider" />
      <div className="sidebar-section-label">Item Database</div>
      <div className="db-count">{itemCount.toLocaleString()} items · {recipeCount.toLocaleString()} recipes</div>
      <button className="btn-fetch" onClick={onFetch} disabled={fetching}>
        {fetching ? "Fetching…" : "Refresh item list"}
      </button>
      {fetchMsg && <div className="fetch-msg">{fetchMsg}</div>}
    </aside>
  );
}
