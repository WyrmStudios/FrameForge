export type Module = "inventory" | "foundry" | "market" | "relics" | "rivens" | "timers" | "statistics" | "completionist";

interface AppNavigationProps {
  activeModule: Module;
  onModuleChange: (module: Module) => void;
}

export default function AppNavigation({ activeModule, onModuleChange }: AppNavigationProps) {
  return (
    <nav className="module-nav">
      <button className={`module-btn ${activeModule === "inventory" ? "module-active" : ""}`} onClick={() => onModuleChange("inventory")} title="Inventory">
        <img src="/inventory-icon.png" alt="" style={{ width: 24, height: 24, objectFit: "contain" }} />
        <span className="module-label">Inventory</span>
      </button>
      <button className={`module-btn ${activeModule === "foundry" ? "module-active" : ""}`} onClick={() => onModuleChange("foundry")} title="Foundry">
        <img src="/foundry-icon.png" alt="" style={{ width: 24, height: 24, objectFit: "contain" }} />
        <span className="module-label">Foundry</span>
      </button>
      <button className={`module-btn ${activeModule === "market" ? "module-active" : ""}`} onClick={() => onModuleChange("market")} title="Market Helper">
        <img src="/market-icon.png" alt="" style={{ width: 24, height: 24, objectFit: "contain" }} />
        <span className="module-label">Market</span>
      </button>
      <button className={`module-btn ${activeModule === "relics" ? "module-active" : ""}`} onClick={() => onModuleChange("relics")} title="Relic Helper">
        <img src="/relic-icon.png" alt="" style={{ width: 24, height: 24, objectFit: "contain" }} />
        <span className="module-label">Relics</span>
      </button>
      <button className={`module-btn ${activeModule === "timers" ? "module-active" : ""}`} onClick={() => onModuleChange("timers")} title="Timers">
        <img src="/timers-icon.png" alt="" style={{ width: 24, height: 24, objectFit: "contain" }} />
        <span className="module-label">Timers</span>
      </button>
      <button className={`module-btn ${activeModule === "statistics" ? "module-active" : ""}`} onClick={() => onModuleChange("statistics")} title="Statistics">
        <img src="/statistics-icon.png" alt="" style={{ width: 24, height: 24, objectFit: "contain" }} />
        <span className="module-label">Statistics</span>
      </button>
      <button className={`module-btn ${activeModule === "rivens" ? "module-active" : ""}`} onClick={() => onModuleChange("rivens")} title="Riven Analyzer">
        <img src="/riven-icon.png" alt="" style={{ width: 24, height: 24, objectFit: "contain" }} />
        <span className="module-label">Rivens</span>
      </button>
      <button className={`module-btn ${activeModule === "completionist" ? "module-active" : ""}`} onClick={() => onModuleChange("completionist")} title="Completionist">
        <img src="/completionist-icon.png" alt="" style={{ width: 24, height: 24, objectFit: "contain" }} />
        <span className="module-label">Completionist</span>
      </button>
    </nav>
  );
}
