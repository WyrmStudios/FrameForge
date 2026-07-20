# FrameForge — Warframe Companion `v2.5.0`

A desktop companion for Warframe — live inventory, market prices, trading, timers, relic overlay, and riven analysis. Read-only, no game modification.

> **Windows 10/11 only.** Inventory scanning requires Warframe to be running; all other features work standalone.

---

## Features

### Live Inventory
Reads your inventory directly from Warframe's process memory (read-only, same API as Overwolf). Instead of scanning for individual item patterns, FrameForge now locates and captures the full account JSON blob that the game client holds in memory — the same authoritative data the game itself uses.

This gives complete coverage: resources, mods, arcanes, relics, weapons, Warframes, companions, blueprints, cosmetics (glyphs, palettes, emotes, titles, ship skins), sigils, pending Foundry jobs, credits, and more. Items that leave your inventory (traded, consumed, or expired) are correctly detected as dropped to zero. Inventory is persisted to disk and restored instantly on next launch — no login required.

### Foundry
Browse every craftable item with full ingredient trees. Components are colour-coded by ownership status and show which relics drop them. Star items to track them in the Modular Window. Filter by Prime, Vaulted/Unvaulted, Owned/Unowned, Ready to build, and Mastered/Unmastered.

### Market Helper
Browse Prime sets and mods with live platinum prices from [warframe.market](https://warframe.market). Click any item for a live order popup with sell/buy orders, 3-week price chart, and one-click listing (requires WFM login). Mod cards show per-rank chips — click a chip to pre-fill the listing form with that exact rank and quantity.

**WFM Status Automation** (requires WFM login):
- Go Invisible on startup — set your status to Invisible the moment FrameForge opens
- Go Invisible on close — set your status before the app exits (X button or taskbar close)
- Auto-invisible timer — automatically go Invisible after a configurable number of minutes

### Trading
Full warframe.market integration — manage active listings, post new orders, and receive trade whispers from in-game chat.

- **One-click whisper copy** — every order row has a 📋 button that copies the correct WFM trade message to your clipboard: `I want to buy` from sellers, `I want to sell` to buyers.
- **Auto trade detection** — when a trade completes in-game, the matching WFM whisper is automatically marked complete, the sold reply is copied to your clipboard, and the whisper stays visible as a ghost for 5 minutes.
- **Auto listing update** — after a sale is detected, the corresponding WFM sell listing is automatically decremented (or deleted if the last copy). A Revert button appears on the ghost whisper to undo the change if needed.
- **Status auto-reconnect** — if WFM drops your status to offline, it is automatically restored without any action needed. Session token stored in Windows Credential Manager.
- **Riven auction management** — view all active riven auctions with AUC/DIR badges, starting price, and buyout price. Edit price, visibility, and auction type (AUC↔DIR) via a dedicated popup. Type switching is handled server-side with a guaranteed full data fetch.
- **In-app WFM login** — authenticate via a built-in WebView browser popup instead of entering credentials manually.

### Relic Helper
Browse void fissure drop tables with rarity colour-coding, ownership status, and platinum values. Supports all refinement levels (Intact → Radiant).

### Timers
Live dashboard from DE's worldstate API:
- World cycles (Cetus, Orb Vallis, Cambion Drift, Zariman) with countdowns
- Bounty reset timers per open world
- Daily/Weekly resets, Sortie, Archon Hunt, The Circuit, Deep Archimedea
- Baro Ki'Teer, Prime Resurgence, Nightwave, Darvo deal, community events
- Alerts, Invasions, Void Fissures with configurable fissure watches

### Statistics
- **Trades** — auto-detected from EE.log and WFM messages, with manual entry fallback
- **Reports** — date-filtered KPIs, platinum charts, per-item breakdown, top trading partners
- **Item Report** — track any item's quantity over time with daily snapshots and drag-to-reorder cards

### Riven Analyzer
Analyses riven rolls against the community-curated [44bananas spreadsheet](https://docs.google.com/spreadsheets/d/1zbaeJBuBn44cbVKzJins_E3hTDpnmvOk8heYN-G8yy8) (413+ weapons). Click **Check Riven** while the riven screen is open for instant per-stat quality ratings. Comparison mode shows old vs new roll side-by-side after each cycle. Supports primary, secondary, melee, and archwing weapons.

### OCR Relic Reward Overlay
When a void fissure reward screen opens, FrameForge automatically captures it via Windows OCR and shows a transparent overlay with platinum price, ducat value, and set completion for each card. Priority mode: Completion / Plat / Ducats / Set Value.

### Modular Window
Customisable sidebar with reorderable sections: tracked crafting items, favourite inventory items, pinned timers, and watched fissures.

### Settings
Reorganised into a tabbed sidebar layout: **General** (overlay, scanner, API, account info, pop-out), **Market** (WFM status automation), **Accessibility** (colorblind mode, text size), **Data** (item database, cache), and **Debugging** (loggers, diagnostic tools with folder access and one-click clear).

---

## EULA Transparency

Two features touch EULA grey areas and are **off by default** with explicit opt-in warnings:

- **Memory Scanner** — `ReadProcessMemory` for live inventory. Read-only, same API as Overwolf.
- **Warframe Companion API** — `api.warframe.com/api/inventory.php` for mod ranks and inventory detail.

Everything else (Foundry, Market, Relics, Timers, Statistics) runs on public data only.

---

## Is This Safe?

| | |
|---|---|
| Memory access | Read-only `ReadProcessMemory` — never writes, never injects |
| Game modification | None |
| Network | warframe.market, DE worldstate, WFCD GitHub repos. No FrameForge server, no telemetry |
| Credentials | WFM token in Windows Credential Manager. Warframe API credentials never written to disk |

Source code is fully public under GPLv3 — build and verify it yourself.

---

## Requirements

- Windows 10 or 11 (64-bit)
- Warframe installed for inventory scanning (other features work without it)
- [warframe.market](https://warframe.market) account for trading features (optional)

---

## Installation

1. Download the latest installer from [**Releases**](../../releases)
2. Run it — click **More info → Run anyway** if SmartScreen warns you (no code-signing certificate)
3. Launch FrameForge from Start or the desktop shortcut

---

## Building From Source

```powershell
# Prerequisites: Node.js 20+, pnpm, Rust MSVC toolchain
rustup default stable-x86_64-pc-windows-msvc

git clone https://github.com/WyrmStudios/FrameForge.git
cd FrameForge
pnpm install
pnpm tauri dev      # dev mode with hot reload
pnpm tauri build    # installer → src-tauri/target/release/bundle/
```

---

## Tech Stack

| Layer | Technology |
|---|---|
| Frontend | React 19, TypeScript 5.8, Vite 7 |
| Desktop shell | Tauri 2 |
| Backend | Rust 2021 edition |
| Database | SQLite (local only) |
| Windows APIs | ReadProcessMemory, WinRT OCR, DXGI, GDI, Windows Credential Manager |

---

## Data & Privacy

- No account required for most features
- No telemetry — no FrameForge server
- All data stored locally at `%LOCALAPPDATA%\warframe-companion\`
- WFM session token stored in Windows Credential Manager if "Stay logged in" is enabled

---

## License

GPLv3 — see [LICENSE](LICENSE).

---

## Contributing

Bug reports, feature requests, and PRs welcome via [GitHub Issues](../../issues). Use the issue templates. For large changes, open an issue first to align on approach.

---

*FrameForge is not affiliated with Digital Extremes Ltd. Warframe is a trademark of Digital Extremes Ltd.*
