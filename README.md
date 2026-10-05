# FrameForge `v4.5.0`

A desktop companion for Warframe — live inventory, market prices, trading, timers, relic overlay, and riven analysis. Read-only, no game modification.

> **Windows 10/11 only.** Inventory scanning requires Warframe to be running; all other features work standalone.

---

## Features

### Arbitrations

Browse the cached arbitration schedule up to 60 days ahead (seven by default),
filter by farming tier, and favorite nodes. The rotation comes from the
community feed at [browse.wf](https://browse.wf). Optional alerts match
favorites or selected tiers, with a ten-minute default lead time.

Completed runs are recovered from EE.log at startup and tracked live. History
supports deletion and analytics; Vitus values are estimates, and kill counts
require host telemetry. An optional summary overlay closes after 12 seconds
and is disabled by default. Data sources and parser/model attribution:
[third-party notices](THIRD_PARTY_NOTICES.md).

### Live Inventory
Reads your inventory directly from Warframe's process memory (read-only, same API as Overwolf). Instead of scanning for individual item patterns, FrameForge locates and captures the full account JSON blob that the game client holds in memory — the same authoritative data the game itself uses.

This gives complete coverage: resources, mods, arcanes, relics, weapons, Warframes, companions, blueprints, cosmetics (glyphs, palettes, emotes, titles, ship skins), sigils, pending Foundry jobs, credits, and more. Items that leave your inventory (traded, consumed, or expired) are correctly detected as dropped to zero. Inventory is persisted to disk and restored instantly on next launch — no login required.

**Truncation-safe captures** — the game sometimes leaves only part of the account blob in memory. FrameForge learns which sections your account normally contains and rejects any capture that has lost one of them, so a half-read blob never makes items flicker out and back in. Captures identical to the one already applied are skipped, so the changelog only moves when something actually changed.

**View modes** — Cards (icon + text), Icon grid, Text cards, List with icon, or Compact list (text-only, maximum density). Persisted per tab.

### Foundry
Browse every craftable item with full ingredient trees. Components are colour-coded by ownership status and show which relics drop them. Star items to track them in the Modular Window. Filter by Prime, Non-Prime, Vaulted/Unvaulted, Owned/Unowned, Ready to build, Mastered/Unmastered, and Lvl > 30 (Kuva/Tenet/Incarnon weapons, Necramechs). Items per page is configurable in Settings → General (30 / 60 / 100). **Operator Weapons** (Amp Prisms) appear as their own category.

**Ignore Forma/Kuva** — a toggle chip in the filter bar treats Forma blueprints and Kuva as always owned, so they never clutter the Unowned view.

Items with Forma applied show a **Forma icon badge** with the count overlaid — sourced live from memory, no manual tracking needed. Mastered / Unmastered filters use each item's actual level cap (30 for standard gear, higher for Incarnon/Kuva/Tenet weapons), and exclude non-levelable items (parts, blueprints) from both filters entirely.

**View modes** — switch between Cards (icon + text), Icon grid, Text cards, List (compact row with icon), and Compact list (text-only) via the toggle in the filter bar. Each tab remembers its own preference.

### Weapons
Track mastery progress across all weapon categories: Primary, Secondary, Melee, and **Operator** (Amp Prisms + Sirocco). Each tab shows items grouped by type — Standard, Prime, Kuva, Tenet, Coda, Wraith, Vandal, Prisma, MK1, and **Zaw** (Strike components) in the Melee tab. Mastered items are highlighted; a progress bar shows how many you've mastered in each category. Filter to unmastered items only.

Amp Prisms and Zaw Strikes use the same modular mastery mechanic — a gilded amp or Zaw at rank 30 counts as mastered, keyed by its Prism or Strike component so each component tracks independently.

### Market Helper
Browse Prime sets and mods with live platinum prices from [warframe.market](https://warframe.market). Prices are loaded from the [FrameForgePricing](https://github.com/WyrmStudios/FrameForgePricing) mirror on startup — a daily bulk cache updated twice per day from relics.run. No per-item network calls needed. Click any item for a live order popup with sell/buy orders, 3-week price chart, and one-click listing (requires WFM login). Mod cards show per-rank chips — click a chip to pre-fill the listing form with that exact rank and quantity. A **rank selector** in the orders popup filters listings to a specific mod/arcane rank.

**Recipe-aware duplication detection** — multi-count recipes (e.g. Aksomati Prime requires 2× Barrel) are respected when calculating sellable duplicates and the "Has dupes" filter.

**WFM Status Automation** (requires WFM login):
- Go Invisible on startup — set your status to Invisible the moment FrameForge opens
- Go Invisible on close — set your status before the app exits (X button or taskbar close)
- Auto-invisible timer — automatically go Invisible after a configurable number of minutes

### Trading
Full warframe.market integration — manage active listings, post new orders, and receive trade whispers from in-game chat.

- **One-click whisper copy** — every order row has a 📋 button that copies the correct WFM trade message to your clipboard: `I want to buy` from sellers, `I want to sell` to buyers.
- **Auto trade detection** — when a trade completes in-game, the matching WFM whisper is automatically marked complete, the sold reply is copied to your clipboard, and the whisper stays visible as a ghost for 5 minutes.
- **Auto listing update** — after a sale is detected, the corresponding WFM sell listing is automatically decremented (or deleted if the last copy). Works regardless of which tab is active. An **Auto-updated listings** changelog at the bottom of the Listings tab shows every automatic change with an Undo button per entry.
- **Status auto-reconnect** — if WFM drops your status to offline, it is automatically restored without any action needed. Session token stored in Windows Credential Manager.
- **Riven auction management** — view all active riven auctions with AUC/DIR badges, starting price, and buyout price. Edit price, visibility, and auction type (AUC↔DIR) via a dedicated popup. Type switching is handled server-side with a guaranteed full data fetch.
- **In-app WFM login** — authenticate via a built-in WebView browser popup instead of entering credentials manually.

### Relic Helper
Browse void fissure drop tables with rarity colour-coding, ownership status, and platinum values. Supports all refinement levels (Intact → Radiant). View modes apply here too. Mastered-but-sold prime items (e.g. Bronco crafted into Akbronco) are correctly treated as completed. **Ignore Forma/Kuva** chip hides Forma and Kuva rewards from the Uncompleted filter.

**Relic Planner** — EV (expected value) calculator per refinement tier. Pick your metric (Platinum or Ducats), squad size (Solo → 4-player radshare), and filter by era, owned, or vaulted. Sortable columns: click any header (Relic, Owned, Intact, Except., Flawless, Radiant, Refine gain) to sort ascending or descending.

### Timers
Live dashboard from DE's worldstate API:
- World cycles (Cetus, Orb Vallis, Cambion Drift, Zariman) with countdowns
- Bounty reset timers per open world
- Daily/Weekly resets, Sortie, Archon Hunt, The Circuit, Deep Archimedea
- Baro Ki'Teer, Prime Resurgence, Nightwave, Darvo deal, community events
- Alerts, Invasions, Void Fissures with configurable fissure watches and **OS desktop notifications** — get alerted the moment a watched fissure appears, no matter which tab is open

### Statistics
- **Trades** — auto-detected from EE.log. Captures all items from both sides of every trade, including item-for-item barters (no platinum involved). Each trade is classified as Sale, Purchase, or Trade.
- **Trade Log** — individual trade cards showing the full exchange: what you gave, what you received, with which player, and when. Toggle between Log and Analytics views in the Reports tab.
- **Reports** — date-filtered KPIs, platinum charts, per-item breakdown, top trading partners
- **Item Report** — track any item's quantity over time with daily snapshots and drag-to-reorder cards

### Riven Analyzer
Analyses riven rolls against the community-curated [44bananas spreadsheet](https://docs.google.com/spreadsheets/d/1zbaeJBuBn44cbVKzJins_E3hTDpnmvOk8heYN-G8yy8) (413+ weapons). Click **Check Riven** while the riven screen is open for instant per-stat quality ratings. Comparison mode shows old vs new roll side-by-side after each cycle. Supports primary, secondary, melee, and archwing weapons.

### Relic Pick Overlay
When you open the Void Relics / Refinement screen, FrameForge detects the fissure era via OCR and instantly shows a compact overlay at the top-right of your screen ranking your relics by the metric you care about most.

**Priority modes** — Collect (probability of getting an item you don't own yet), Platinum (expected plat value per run), or Ducats (expected ducat yield). The top 3 relics are shown with refinement badge, copy count, score, and per-reward detail: vaulted status, ownership, platinum price, ducat value, and a refinement recommendation (REC) based on rarity.

**Shown lines** — All rewards (full 6-item list per relic), Best pick (the single highest-value reward), or Score summary (compact one-liner: plat EV · ducat EV · new items). Configure in Settings → Overlays.

The overlay auto-sizes to its content and dismisses when you return to the star map.

### OCR Relic Reward Overlay
When a void fissure reward screen opens, FrameForge automatically captures it via Windows OCR and shows a transparent overlay with platinum price, ducat value, and set completion for each card. Priority mode: Completion / Plat / Ducats / Set Value.

The item catalog used for OCR matching is built exclusively from known relic reward names — no false matches from non-reward items. Survival fissure multi-round sessions are fully supported: the selected relic carries over between rounds correctly.

### Modular Window
Customisable sidebar with reorderable sections: tracked crafting items, favourite inventory items, pinned timers, and watched fissures.

### Item Catalogue
The item database (items, recipes, relic drops) is fetched and rebuilt in the background on launch, and every open screen reloads on its own when it finishes — no manual **Refresh item list** needed, even right after an update. Source ETags are persisted to disk, so unchanged data is never re-downloaded. Tabs stay mounted while hidden, so switching between modules keeps your scroll position and filters.

### Settings
Tabbed sidebar layout: **General** (Foundry page size, scanner, API, account info, pop-out), **Overlays** (Relic Overlay and Relic Pick Overlay settings), **Market** (WFM status automation), **Filters** (filter preset management per module), **Accessibility** (colorblind mode, text size up to 200%), **Data** (item database, cache), and **Debugging** (loggers, diagnostic tools with folder access and one-click clear).

**Filter Presets** — save, pin, and reorder named filter presets for Inventory, Foundry, Market, and Relic Helper, from each toolbar or from Settings → Filters. Search boxes across the app accept comma-separated terms for OR-style matching (e.g. `braton, soma` shows both).

---

## EULA Transparency

One feature touches a EULA grey area and is **off by default** with an explicit opt-in warning:

- **Memory Scanner** — `ReadProcessMemory` for live inventory. Read-only, same API as Overwolf.

The **Warframe Companion API** (`api.warframe.com/api/inventory.php`) has been **temporarily suspended**. DE confirmed third-party tools run at your own risk but could not clarify whether this specific undocumented endpoint is permitted. The feature is disabled until clearer guidance is received.

Everything else (Foundry, Market, Relics, Timers, Statistics) runs on public data only.

---

## Is This Safe?

| | |
|---|---|
| Memory access | Read-only `ReadProcessMemory` — never writes, never injects |
| Game modification | None |
| Network | warframe.market, DE worldstate, WFCD GitHub repos, browse.wf, FrameForgePricing mirror. No FrameForge server, no telemetry |
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
# `pnpm test` needs Node 22.6+ (it runs the TypeScript tests through --experimental-strip-types)
rustup default stable-x86_64-pc-windows-msvc

git clone https://github.com/WyrmStudios/FrameForge.git
cd FrameForge
pnpm install
pnpm tauri dev      # dev mode with hot reload
pnpm tauri build    # installer → src-tauri/target/release/bundle/
pnpm test           # unit tests (Node 22.6+)
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
- All data stored locally — settings/database at `%APPDATA%\frameforge\`, caches at `%LOCALAPPDATA%\frameforge\`
- Debug builds (`pnpm tauri dev`) keep a completely separate settings/database/caches in `.frameforge-dev\` next to the project and are titled **FrameForge Dev**, so development never reads or writes the installed app's files. Release builds compile that path out entirely.
- WFM session token stored in Windows Credential Manager if "Stay logged in" is enabled

---

## License

GPLv3 — see [LICENSE](LICENSE).

---

## Contributing

Bug reports, feature requests, and PRs welcome via [GitHub Issues](../../issues). Use the issue templates. For large changes, open an issue first to align on approach.

---

*FrameForge is not affiliated with Digital Extremes Ltd. Warframe is a trademark of Digital Extremes Ltd.*
