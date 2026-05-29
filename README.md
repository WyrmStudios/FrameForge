# FrameForge — Warframe Companion `v1.2.0`

A **read-only** desktop companion for Warframe that shows your live inventory, tracks crafting recipes, displays market prices, runs a live timer dashboard, and auto-detects relic reward screens — all without touching the game process or sending your data anywhere.

> **Windows only** — requires Windows 10 or 11. Inventory scanning requires Warframe to be running; all other features work standalone.

---

## Features

### Live Inventory Scanning
Your inventory is read directly from the Warframe process memory every 10 seconds across 13 item categories — resources, mods, arcanes, relics, weapons, Warframes, companions, and more. No login required. The scanner is **strictly read-only**: it uses `ReadProcessMemory`, the same Windows API used by Overwolf and hardware monitors. It never writes to memory, injects code, or modifies the game in any way.

A quantity change log records every item gain and loss with timestamps.

### Foundry — Recipe Browser & Tracker
Browse every craftable item with full ingredient trees. Each component is colour-coded by status (owned, blueprint only, missing) and shows which relics drop it. Star items to **track** them in the Modular Window, which shows a per-item breakdown of exactly what you still need to farm.

### Market Helper
Look up platinum sell prices from [warframe.market](https://warframe.market) for any item or prime set. Shows total set value, per-part pricing, ducat values, and your ownership status. Prices are cached for the session and fetched on demand — no account required.

### Relic Helper
Browse void fissure drop tables with rarity colour-coding (Bronze/Silver/Gold), ownership status, and platinum value. Supports all refinement levels: Intact, Exceptional, Flawless, Radiant.

### Timers
A live dashboard fetching data from the official Warframe worldstate:

- **World Cycles** — Cetus (Day/Night), Orb Vallis (Warm/Cold), Cambion Drift, Zariman with countdown to next change
- **Bounties** — reset timers per open world (Cetus, Orb Vallis, Cambion Drift, Zariman, Hex)
- **Daily & Weekly** — Daily Reset, Weekly Reset, Sortie (expandable missions + modifier), Archon Hunt (expandable missions), The Circuit (frame and weapon picks), Deep Archimedea, Kahl / Break Narmer
- **Events** — Baro Ki'Teer and Prime Resurgence both show expandable inventories with item prices and **Owned** highlights; Nightwave, Darvo daily deal, and active community events
- **Alerts** — each alert as a tile with mission type, faction, reward, and countdown
- **Invasions** — each invasion with progress bar, attacker/defender factions, and rewards
- **Void Fissures** — Normal, Steel Path, and Void Storm tabs, displayed as a grid of tiles (tier, mission type, faction, node, countdown). Watched fissures are highlighted.
- **Fissure Watches** — configure a Mode + Tier + Mission Type filter; matching fissures are highlighted in the list and automatically appear in the Modular Window

Any timer can be pinned to the Modular Window for at-a-glance viewing.

### Modular Window
A customisable sidebar (or detachable floating window) with four reorderable sections:
- **Tracking** — items being crafted, with per-item ingredient requirements and collapse toggles
- **Favorites** — watched inventory items with live quantities
- **Timers** — pinned countdowns from the Timers tab
- **Watched Fissures** — live fissures matching your configured watches

Sections can be collapsed, reordered with arrow buttons, and the panel can be resized (160–500 px) or detached as a separate window.

### OCR Relic Reward Overlay
When a void fissure reward screen opens in-game, FrameForge automatically captures and reads all four reward cards using Windows OCR and displays a transparent overlay with each item's platinum price, ducat value, and set completion — so you can pick the best reward instantly without alt-tabbing. Priority mode is configurable: Item Completion, Most Set Value, Most Plat, or Most Ducats.

---

## Is This Safe? (Read This If You Found Us on Reddit)

| | What FrameForge does |
|---|---|
| **Memory access** | Read-only via `ReadProcessMemory` — never writes, never injects, never hooks |
| **Game modification** | Nothing. No DLL injection, no code hooking, no writing to memory |
| **Network requests** | `warframe.market` (prices), official Warframe worldstate API (timers/events), WFCD community GitHub repos (item catalog, recipe data, drop rates, localisation), Warframe CDN (item images). No FrameForge server. No telemetry. |
| **Your account** | Never touched unless you enable the optional Warframe Companion API feature — credentials are read from the game's own memory and never written to disk |
| **Your data** | Nothing leaves your machine except the network calls listed above |

Digital Extremes has historically permitted read-only companion tools. FrameForge follows the same approach used by other community tools.

**Don't take our word for it** — the full source code is in this repository under GPLv3. You can read every line, build it yourself, and verify exactly what it does. The distributed `.exe` contains minified frontend code (standard build practice), but the source here is the authoritative, readable version.

---

## Requirements

- Windows 10 or 11 (64-bit)
- [Warframe](https://www.warframe.com/) installed for inventory scanning (Foundry, Market, Relics, and Timers work without it)
- The OCR overlay works best in Windowed or Borderless Windowed mode; Fullscreen Exclusive is also supported via DXGI capture

---

## Installation

1. Go to [**Releases**](../../releases) and download the latest installer
2. Run it — Windows SmartScreen may warn you because the binary is not yet code-signed; click **More info → Run anyway**
3. Launch FrameForge from the Start menu or desktop shortcut

> The SmartScreen warning appears because code-signing certificates cost ~$300/year. The source code is fully public for independent verification.

---

## Building From Source

```powershell
# Prerequisites: Node.js 20+, pnpm, Rust (MSVC toolchain)
rustup default stable-x86_64-pc-windows-msvc

# Clone and install dependencies
git clone https://github.com/Sikewyrm/FrameForge.git
cd FrameForge
pnpm install

# Development mode (Vite hot-reload + Tauri window)
pnpm tauri dev

# Production build — installer output: src-tauri/target/release/bundle/
pnpm tauri build
```

---

## How It Works — Technical Overview

### Memory Scanning (`memory_scanner.rs`)
FrameForge enumerates committed, readable memory regions of the Warframe process using `VirtualQueryEx` and reads them with `ReadProcessMemory`. Three independent pattern matchers run over each region:

- **Resource scanner** — stackable items via JSON patterns like `"ItemCount":N,"ItemType":"/Lotus/..."`
- **Unique scanner** — weapons, Warframes, and companions via Aho-Corasick multi-pattern matching
- **Pending recipe scanner** — active Foundry jobs via ISO-8601 completion timestamps

A **stability buffer** requires a quantity to appear in two consecutive scans before it is committed. Only `MEM_COMMIT` regions with readable page protection are scanned; regions larger than 128 MB are skipped.

### Timers (`lib.rs`)
Worldstate data is fetched from the official DE worldstate endpoint (`api.warframe.com/cdn/worldState.php`). Node names are resolved from the WFCD sol nodes dataset. Event names are resolved from the WFCD localisation dataset. All parsing is done in Rust and served to the frontend via Tauri IPC — no browser-side fetch calls.

### OCR Overlay (`ocr.rs`)
1. `EE.log` is monitored for `openvoidprojectionrewardscreen`
2. After a 350 ms delay, the top 48% of the Warframe window is captured via `PrintWindow` (GDI) or DXGI Desktop Duplication
3. Windows WinRT OCR extracts text from the capture
4. Rarity bar colour analysis classifies each of the 4 reward slots
5. Fuzzy string matching maps OCR output to catalog item names
6. A transparent overlay displays each card's item name, price, ducat value, and set completion

### Item Data (`wfcd.rs`)
Item and recipe data is fetched on first launch and cached to disk:

| Source | Used for |
|---|---|
| [warframe-items (WFCD)](https://github.com/WFCD/warframe-items) | Item catalog, categories, ducat values, vaulted status |
| [warframe-public-export-plus](https://github.com/calamity-inc/warframe-public-export-plus) | Authoritative recipe trees (LZMA-compressed) |
| [Warframe Wiki Module:Void](https://wiki.warframe.com) | Canonical relic reward display names |

---

## Data & Privacy

- **No account required** for inventory scanning, Foundry, Market Helper, Relic Helper, or Timers
- **No telemetry** — FrameForge has no server and makes no analytics calls
- **Local storage only** — a SQLite database of your quantity changes lives at `%LOCALAPPDATA%\warframe-companion\`
- **Warframe.market requests** are made from your machine directly to warframe.market (same as visiting in a browser)
- If you use the optional Warframe Companion API feature, session credentials are held in RAM only and never written to disk

---

## Tech Stack

| Layer | Technology |
|---|---|
| Frontend | React 19, TypeScript 5.8, Vite 7 |
| Desktop shell | Tauri 2 |
| Backend | Rust 2021 edition |
| Database | SQLite via `rusqlite` (local only) |
| Async runtime | `tokio` |
| HTTP | `ureq` |
| Windows APIs | `ReadProcessMemory` / `VirtualQueryEx`, WinRT OCR, DXGI Desktop Duplication, GDI `PrintWindow` |

---

## License

FrameForge is free and open-source software released under the **GNU General Public License v3.0**.  
See the [LICENSE](LICENSE) file for the full license text.

---

## Contributing

Bug reports and pull requests are welcome. Please open an issue before submitting large changes so we can align on the approach first.

---

*FrameForge is not affiliated with Digital Extremes Ltd. or the Warframe brand. Warframe is a trademark of Digital Extremes Ltd.*
