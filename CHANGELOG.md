# Changelog

## v2.6.0 — 2026-07-24

Bulk price feed, trade log, mod rank filtering, recipe-aware duplication, and a batch of OCR and scanner reliability fixes.

✨ New — relics.run bulk price feed
FrameForge now fetches a single daily bulk file from relics.run on startup instead of queuing hundreds of per-item warframe.market calls. The bulk file covers mods, arcanes, syndicate weapons, and prime parts with authoritative WFM slugs. The old FrameForgePricing GitHub cache is no longer used. Prices load instantly on the first tab open and stay fresh for the rest of the day without any extra network traffic.

✨ New — Trade Log
The Reports tab has a new **Log** view alongside the existing Analytics view. Every completed in-game trade shows as a card: the type badge (Sale / Purchase / Trade), the trading partner's name, the date and time, and a side-by-side breakdown of what you gave and what you received. Switch between Log and Analytics with the toggle above the date range picker.

✨ New — Full multi-item and item-for-item trade detection
The trade parser previously captured only the first item from one side of a trade and missed item-for-item barters entirely. It now reads every item from both sides of the trade dialog. Sales (items → plat) and purchases (plat → items) correctly list all components. Item-for-item trades where no platinum is involved are now detected and stored with direction "trade". All items from a single session share a `session_id` so the Log can reconstruct the full exchange.

✨ New — Mod rank filter in orders popup
The item market popup now shows a rank selector for mods and arcanes. Changing the rank re-fetches orders filtered to that specific rank — no more scrolling past rank-0 listings when you want to price a maxed mod. The rank selector also stays in sync with the listing form.

✨ New — Recipe-aware duplication detection
Multi-count recipes (Aksomati Prime needs 2× Barrel and 2× Receiver, not 1) are now reflected correctly. The "Has dupes" filter and the ducat tally in Market Helper both account for the required count per component, so a single copy of a 2× ingredient no longer shows as a sellable duplicate.

🔧 Bug Fixes — OCR
Fixed reward cards being cut off on tall layouts. The capture height was raised from 75% to 80% of the Warframe window. The OCR vertical cutoff is now a fixed 0.95 instead of a dynamic bar-position calculation — the bar-based cutoff was unreliable and frequently deleted valid item text above the rarity bar.

🔧 Bug Fixes — Relic rewards
Relic reward data is now fetched from WFCD's dedicated `Relics.json` (one entry per refinement level, exact EE.log paths) instead of inverting the `rewards[]` field in `All.json`. This fixes rewards appearing under the wrong relic paths and ensures the overlay candidate filter covers all active relics correctly.

🔧 Bug Fixes — Item categorization
Sentinel and MOA companion parts (Nautilus Prime Carapace, Nautilus Prime Cerebrum, Nautilus Prime Systems) were incorrectly stored as "Blueprints" because WFCD assigns that category to all `/Recipes/` paths. The categorisation rule now distinguishes `/WarframeRecipes/` (actual blueprints) from `/Weapons/WeaponParts/` (physical parts like weapon components). Items in the second group are stored as "Parts".

🔧 Bug Fixes — Memory scanner
Fixed the scanner stalling after a game restart. The fast-path blob region cache now resets when Warframe's process ID changes, preventing the next scan from probing a stale address from the previous session. The blob capture flag is also cleared via an RAII guard so a thread panic can no longer leave the scanner permanently suspended.

🔧 Bug Fixes — Relic overlay event loop
Removed the `emitTo("relic-overlay", "relic-rewards")` relay in App.tsx. Rust already sends a global emit that Overlay.tsx receives directly; the relay was creating an infinite feedback loop in Tauri 2 that caused duplicate reward events. The overlay window open path is simplified: no pending-items buffer, no forwarding.

🔧 Bug Fixes — Overlay catalog self-heal
The overlay now re-fetches the item catalog if it finds itself with an empty catalog on the first inventory update. This fixes a startup race where the overlay window loaded before `load_wfcd_data` had finished.

🔧 Bug Fixes — Diagnostic screenshot
The one-click diagnostic screenshot now captures via GDI BitBlt instead of DXGI, so the FrameForge overlay appears in the image. DXGI captures GPU output before DWM composites transparent overlay windows, making the overlay invisible in diagnostics.

📌 Warframe Companion API suspended
The `api.warframe.com/api/inventory.php` feature has been temporarily disabled. DE confirmed third-party tools run at your own risk but could not clarify whether this specific undocumented endpoint is permitted. The enable/disable toggle has been replaced with an informational notice. The feature will return once clearer guidance is available.

---

## v2.4.0 — 2026-07-15

Inventory scanner compatibility fix, a few Completionist bugs squashed, and WFM rate limiting corrections.

🔧 Bug Fixes
Fixed the inventory scanner failing entirely for some accounts. The FULL_ACCOUNT blob in memory has a different field order per account — on affected accounts the scanner's start-marker search landed inside a nested JSON object, producing an invalid fragment that was silently discarded. The scanner now buffers preceding memory regions and correctly locates the true blob opening regardless of field order.

Fixed the Helminth subsumed badge (green "H") not appearing in the Completionist Research Labs tab. Subsumed warframes with qty=0 were excluded from the inventory map so the badge condition always evaluated against undefined.

Fixed combined-weapon components being incorrectly marked as unowned. Weapons that are ingredients for another weapon (e.g. Kohmak → Twin Kohmak) were being redirected to their combined parent, causing the base weapon to show "—" even when owned. The redirect now only applies to warframe/archwing component parts.

✨ New — Auction Rate Limiter
Warframe.market enforces a separate limit of 10 requests per minute on contract endpoints (rivens, liches, sisters). FrameForge now applies this limit correctly in addition to the existing 3 req/sec general limiter, preventing 429 errors when browsing or managing riven auctions.

📌 Note
The "Sisters" tab in Market Helper has been renamed to "Variants" to better reflect the full scope it will cover (Sisters of Parvos, Liches, Tenet and Kuva weapons).

---

## v2.3.0

Previous release.
