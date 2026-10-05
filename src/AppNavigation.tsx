export type Module = "inventory" | "foundry" | "market" | "relics" | "rivens" | "timers" | "arbitrations" | "statistics" | "completionist";

const MODULE_NAV = "w-18 shrink-0 bg-surface border-r border-border flex flex-col items-center py-2 gap-1";
const MODULE_BTN =
  "flex flex-col items-center gap-1 w-17 px-1 py-2 bg-transparent border-0 rounded-8 text-muted cursor-pointer transition-[background,color] duration-100 overflow-hidden";
const MODULE_BTN_ON = "text-accent! bg-accent/12!";
const MODULE_BTN_OFF = "text-muted hover:bg-white/6 hover:text-foreground";
const MODULE_LABEL = "text-9 font-semibold tracking-normal w-full text-center line-clamp-2 break-all leading-1.3";

interface AppNavigationProps {
  activeModule: Module;
  onModuleChange: (module: Module) => void;
}

export default function AppNavigation({ activeModule, onModuleChange }: AppNavigationProps) {
  return (
    <nav className={MODULE_NAV}>
      <button className={`${MODULE_BTN} ${activeModule === "inventory" ? MODULE_BTN_ON : MODULE_BTN_OFF}`} onClick={() => onModuleChange("inventory")} title="Inventory">
        <img src="/inventory-icon.png" alt="" className="size-6 object-contain" />
        <span className={MODULE_LABEL}>Inventory</span>
      </button>
      <button className={`${MODULE_BTN} ${activeModule === "foundry" ? MODULE_BTN_ON : MODULE_BTN_OFF}`} onClick={() => onModuleChange("foundry")} title="Foundry">
        <img src="/foundry-icon.png" alt="" className="size-6 object-contain" />
        <span className={MODULE_LABEL}>Foundry</span>
      </button>
      <button className={`${MODULE_BTN} ${activeModule === "market" ? MODULE_BTN_ON : MODULE_BTN_OFF}`} onClick={() => onModuleChange("market")} title="Market Helper">
        <img src="/market-icon.png" alt="" className="size-6 object-contain" />
        <span className={MODULE_LABEL}>Market</span>
      </button>
      <button className={`${MODULE_BTN} ${activeModule === "relics" ? MODULE_BTN_ON : MODULE_BTN_OFF}`} onClick={() => onModuleChange("relics")} title="Relic Helper">
        <img src="/relic-icon.png" alt="" className="size-6 object-contain" />
        <span className={MODULE_LABEL}>Relics</span>
      </button>
      <button className={`${MODULE_BTN} ${activeModule === "timers" ? MODULE_BTN_ON : MODULE_BTN_OFF}`} onClick={() => onModuleChange("timers")} title="Timers">
        <img src="/timers-icon.png" alt="" className="size-6 object-contain" />
        <span className={MODULE_LABEL}>Timers</span>
      </button>
      <button className={`${MODULE_BTN} ${activeModule === "arbitrations" ? MODULE_BTN_ON : MODULE_BTN_OFF}`} onClick={() => onModuleChange("arbitrations")} title="Arbitrations">
        <span aria-hidden className="size-6 text-center text-20 leading-6">?</span>
        <span className={MODULE_LABEL}>Arbitrations</span>
      </button>
      <button className={`${MODULE_BTN} ${activeModule === "statistics" ? MODULE_BTN_ON : MODULE_BTN_OFF}`} onClick={() => onModuleChange("statistics")} title="Statistics">
        <img src="/statistics-icon.png" alt="" className="size-6 object-contain" />
        <span className={MODULE_LABEL}>Statistics</span>
      </button>
      <button className={`${MODULE_BTN} ${activeModule === "rivens" ? MODULE_BTN_ON : MODULE_BTN_OFF}`} onClick={() => onModuleChange("rivens")} title="Riven Analyzer">
        <img src="/riven-icon.png" alt="" className="size-6 object-contain" />
        <span className={MODULE_LABEL}>Rivens</span>
      </button>
      <button className={`${MODULE_BTN} ${activeModule === "completionist" ? MODULE_BTN_ON : MODULE_BTN_OFF}`} onClick={() => onModuleChange("completionist")} title="Completionist">
        <img src="/completionist-icon.png" alt="" className="size-6 object-contain" />
        <span className={MODULE_LABEL}>Completionist</span>
      </button>
    </nav>
  );
}
