# FrameForge v2.6.0

Download: https://github.com/WyrmStudios/FrameForge/releases/latest

---

🔧 Bug Fixes
Fixed reward cards being cut off on some screen resolutions — the capture area is a bit taller now so nothing gets clipped.
Fixed Nautilus Prime parts (Carapace, Cerebrum, Systems) showing the wrong category. They now correctly show as Parts instead of Blueprints.
Fixed the inventory scanner stalling after restarting Warframe without restarting FrameForge. The scanner now resets cleanly when it detects the game closed and reopened.
Fixed relic rewards occasionally showing under the wrong relics. Switched to a better data source that has exact per-refinement info.
Fixed the relic overlay firing duplicate reward events in some cases.
Fixed set completion not showing in the overlay if it loaded before the item database was ready. It now recovers automatically.

✨ New — Prices load instantly
The app now downloads one bulk price file on startup that covers prime parts, mods, arcanes, and syndicate weapons all at once. Previously prices had to be fetched one item at a time through warframe.market. By the time you open the Market tab, everything is already priced.

✨ New — Trade Log
The Reports tab has a new Log view alongside the existing Analytics view. Every in-game trade shows as a card with the player name, date and time, and a full breakdown of what you gave and what you received. Trades are tagged as Sale, Purchase, or Trade so you can see at a glance what kind of exchange it was.

✨ New — Item-for-item trades now tracked
If you trade an item for an item with no platinum on either side, that used to be silently dropped. It now shows up in the Log as a Trade card with both sides recorded. Trades where you sell or buy multiple different items in one session also show all of them instead of just the first.

✨ New — Mod rank filter in the market popup
When you open a mod or arcane in the market popup, there is now a rank selector above the order list. Change the rank and the buy/sell orders update to match — no more scrolling past rank 0 listings when you want to price a maxed mod.

✨ New — Recipe-aware duplicate detection
Some items need more than one copy for crafting — Aksomati Prime needs 2 Barrels for example. FrameForge now loads the actual recipe counts and uses them when calculating sellable duplicates and the Has Dupes filter. A single copy of a 2× ingredient no longer shows as a dupe.

✨ New — Smarter relic overlay matching
Before the reward screen opens, FrameForge watches which relics your squad loaded in. By the time the reward screen pops, it already knows which items can possibly appear — usually 6 to 24 depending on squad size — and only matches your screen against those instead of all ~700 possible relic reward items. This means fewer misreads, especially for items with similar names.

📌 Note
The Warframe Companion API feature (detailed inventory data fetched directly from DE's servers) is temporarily disabled. We asked DE whether third-party tools are allowed to use that specific endpoint. They confirmed tools run at your own risk but could not give a clear answer on the endpoint itself. It is off until we get clearer guidance.
