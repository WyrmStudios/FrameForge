use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct ModCount {
    /// Total copies owned (all ranks combined)
    pub total: i64,
    /// rank (0 = unranked) → count at that rank
    pub by_rank: HashMap<u8, i64>,
}

#[derive(Debug, Serialize, Clone)]
pub struct FoundItem {
    pub unique_name: String,
    pub name: String,
    pub quantity: i64,
    pub explicit_count: bool,
    /// Raw memory context around where this item was found (printable ASCII, non-printable → '·')
    pub context: String,
}

fn extract_context(data: &[u8], match_pos: usize, before: usize, after: usize) -> String {
    let start = match_pos.saturating_sub(before);
    let end = data.len().min(match_pos + after);
    data[start..end].iter()
        .map(|&b| if b >= 0x20 && b < 0x7f { b as char } else { '·' })
        .collect()
}

#[derive(Debug, Serialize, Clone)]
pub struct PendingRecipe {
    pub unique_name: String,
    /// Unix timestamp in milliseconds when the craft completes
    pub completion_ms: i64,
}

/// One Archon Shard socketed into a Warframe.
/// One Archon Shard socketed into a Warframe.
/// `upgrade_type` is the effect path (e.g. `.../ArchonCrystalUpgradeWarframeEnergyMax`).
/// `color` is the raw string value from the JSON (e.g. `"ACC_CRIMSON"`, `"ACC_AZURE_TAUFORGED"`).
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ArchonShard {
    pub upgrade_type: String,
    pub color: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct ScanResult {
    pub warframe_running: bool,
    pub items_found: Vec<FoundItem>,
    pub pending_recipes: Vec<PendingRecipe>,
    pub mastery_rank: Option<u32>,
    /// unique_name → rank (0–30). Only populated for owned unique items.
    pub mastery_data: HashMap<String, u32>,
    pub regions_scanned: usize,
    /// True when any chunk in this scan window contained `"Created":{"$date":` —
    /// the actual account inventory root, not a mission-delta or NPC blob.
    pub found_actual_inventory: bool,
    pub error: Option<String>,
    pub log_lines: Vec<String>,
    /// 4 item paths when the relic reward screen is active, None otherwise.
    pub relic_rewards: Option<Vec<String>>,
    /// Address to pass as start_addr on the next call.
    /// 0 means the scan completed naturally — restart from the beginning.
    pub resume_addr: usize,
    /// Chunk base addresses where the inventory root ("MiscItems":[{) was found.
    /// Pass these back as hint_addrs on the next call for near-instant re-scan.
    pub hot_addrs: Vec<usize>,
    /// Chunk base addresses where gameplay mods (/Lotus/Upgrades/Mods/) were found outside
    /// the MiscItems chunk. Pass these back as mod_hint_addrs for fast re-scan.
    pub mod_hot_addrs: Vec<usize>,
    /// Warframe unique-name paths found in InfestedFoundry.ConsumedSuits (Helminth subsumed).
    pub consumed_suits: Vec<String>,
    /// Mod/arcane counts from RawUpgrades: unique_name → {total, by_rank}.
    pub mods_found: HashMap<String, ModCount>,
    /// Mods found specifically in the hint (inventory-root) and mod-hint regions this pass.
    /// Committed directly to known_mods — hint regions are live inventory memory.
    pub hint_mods: HashMap<String, ModCount>,
    /// Resource counts found specifically in the inventory-root hint region (from MiscItems).
    /// Used for fast-commit of count changes; absent-means-sold only for hint_confirmed paths.
    pub hint_resources: HashMap<String, i64>,
    /// Paths found in FlavourItems (glyphs, titles, emotes, colour palettes) this hint scan.
    /// Binary-owned items with no stale copies — absence means truly sold/removed.
    pub hint_flavour_items: Vec<String>,
    /// Warframe unique-name → socketed Archon Shards.
    /// Only populated for warframes where ArchonCrystalUpgrades was found in memory.
    pub socketed_shards: HashMap<String, Vec<ArchonShard>>,
}

// ─── Shared helpers ───────────────────────────────────────────────────────────

/// Returns true if fewer than 25% of the `window` bytes before `pos` are non-printable.
/// Stale/freed heap allocations have binary garbage before the JSON fragment;
/// live inventory blobs are pure ASCII JSON — this rejects the stale ones.
fn has_clean_prefix(data: &[u8], pos: usize, window: usize) -> bool {
    let start = pos.saturating_sub(window);
    let slice = &data[start..pos];
    if slice.is_empty() { return true; }
    let non_printable = slice.iter().filter(|&&b| b < 0x20 || b >= 0x7f).count();
    non_printable * 4 <= slice.len() // ≤25%
}

fn parse_int(data: &[u8], start: usize) -> Option<i64> {
    let mut n: i64 = 0;
    let mut found = false;
    for &b in data[start..].iter().take(12) {
        if b.is_ascii_digit() {
            n = n * 10 + (b - b'0') as i64;
            found = true;
        } else if found {
            break;
        }
    }
    if found { Some(n) } else { None }
}

fn valid_lotus_path(raw: &[u8]) -> Option<String> {
    if raw.len() < 8 || raw.len() > 511 { return None; }
    if !raw.iter().all(|&b| matches!(b, b'/' | b'_' | b'.' | b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9')) {
        return None;
    }
    let s = std::str::from_utf8(raw).ok()?;
    if s.starts_with("/Lotus/") { Some(s.to_string()) } else { None }
}

fn digits_end(data: &[u8], start: usize) -> usize {
    let mut i = start;
    while i < data.len() && data[i].is_ascii_digit() { i += 1; }
    i
}

/// Convert raw affinity XP to item rank (0–30).
/// Formula from Warframe wiki: cumulative XP to reach rank N is 1000×N² for
/// Warframes/Sentinels/companions, 500×N² for all weapon types.
/// Invert: rank = floor(sqrt(xp / base)).
fn xp_to_rank(xp: i64, path: &str) -> u32 {
    let base = if path.contains("/Powersuits/")
        || path.contains("/SentinelPowersuits/")
        || path.contains("/Types/Friendly/")
        || path.contains("/Types/Game/KubrowPet/")
        || path.contains("/Types/Game/CatbrowPet/")
    { 1000.0f64 } else { 500.0f64 };
    ((xp as f64 / base).sqrt().floor() as u32).min(30)
}

// ─── Scanner 1: Resources ─────────────────────────────────────────────────────
//
// Real MiscItems inventory entries are always {"ItemCount":N,"ItemType":"/Lotus/..."}
// — the two fields are strictly adjacent with only a comma between them.
//
// Reward/trade records use [{"ItemType":"...","ItemCount":N}] — ItemType first,
// wrapped in brackets. Requiring strict adjacency eliminates cross-matches where
// an ItemCount from one JSON object accidentally pairs with an ItemType from a
// different nearby object (which caused Fieldron to flip between 1 and 3).

/// Scans for top-level currency fields that share the MiscItems JSON object.
/// Returns (virtual_path, amount) pairs for any found.
fn scan_currency_fields(data: &[u8]) -> Vec<(&'static str, i64)> {
    const FIELDS: &[(&[u8], &str)] = &[
        (b"\"FusionPoints\":",       "/_currency/Endo"),
        (b"\"RegularCredits\":",     "/_currency/Credits"),
        (b"\"PremiumCredits\":",     "/_currency/Platinum"),
        (b"\"PremiumCreditsFree\":", "/_currency/PlatinumGift"),
    ];
    let mut out = Vec::new();
    for &(marker, path) in FIELDS {
        // Scan all occurrences and take the maximum value. The 1.5 MB hint read spans
        // multiple VirtualAlloc regions; a small delta blob earlier in the buffer can
        // have a stale lower value (e.g. FusionPoints:160) while the authoritative
        // account inventory later in the buffer has the real value (e.g. 123955).
        let mut max_val: i64 = 0;
        let mut pos = 0usize;
        while pos + marker.len() <= data.len() {
            match data[pos..].windows(marker.len()).position(|w| w == marker) {
                None => break,
                Some(rel) => {
                    let abs = pos + rel;
                    let after = &data[abs + marker.len()..];
                    let end = after.iter().position(|&b| !b.is_ascii_digit()).unwrap_or(after.len());
                    if end > 0 {
                        if let Ok(s) = std::str::from_utf8(&after[..end]) {
                            if let Ok(n) = s.parse::<i64>() {
                                if n > max_val { max_val = n; }
                            }
                        }
                    }
                    pos = abs + marker.len() + 1;
                }
            }
        }
        if max_val > 0 { out.push((path, max_val)); }
    }
    out
}

/// Extracts all ItemType paths from `"FlavourItems":[...]`.
/// FlavourItems are binary-owned cosmetics (glyphs, titles, emotes, color palettes) —
/// no ItemCount, presence means owned (qty = 1).
fn scan_flavour_items(data: &[u8]) -> Vec<String> {
    const MARKER: &[u8] = b"\"FlavourItems\":[";
    const ITEM_TYPE_KEY: &[u8] = b"\"ItemType\":\"";

    let Some(start) = data.windows(MARKER.len()).position(|w| w == MARKER) else {
        return Vec::new();
    };
    let array_start = start + MARKER.len() - 1; // position of the opening '['

    // Find the matching ']' using bracket depth (FlavourItems has no nested arrays).
    let mut depth = 0i32;
    let mut array_end = data.len();
    for (i, &b) in data[array_start..].iter().enumerate() {
        match b {
            b'[' => depth += 1,
            b']' => {
                depth -= 1;
                if depth == 0 {
                    array_end = array_start + i + 1;
                    break;
                }
            }
            _ => {}
        }
    }

    let slice = &data[array_start..array_end];
    let mut result = Vec::new();
    let mut pos = 0;
    while pos + ITEM_TYPE_KEY.len() < slice.len() {
        let Some(rel) = slice[pos..].windows(ITEM_TYPE_KEY.len()).position(|w| w == ITEM_TYPE_KEY) else {
            break;
        };
        let val_start = pos + rel + ITEM_TYPE_KEY.len();
        if val_start >= slice.len() { break; }
        let Some(end) = slice[val_start..].iter().position(|&b| b == b'"') else {
            break;
        };
        if let Ok(path) = std::str::from_utf8(&slice[val_start..val_start + end]) {
            if path.starts_with("/Lotus/") {
                result.push(path.to_string());
            }
        }
        pos = val_start + end + 1;
    }
    result
}

fn scan_inventory_resources(data: &[u8], unique_paths: &std::collections::HashSet<String>) -> Vec<(String, i64, String)> {
    let count_key = b"\"ItemCount\":";
    let type_key  = b"\"ItemType\":\"";

    let mut results: HashMap<String, (i64, String)> = HashMap::new();
    let mut pos = 0usize;

    loop {
        let count_rel = match data[pos..].windows(count_key.len()).position(|w| w == count_key) {
            Some(p) => p,
            None => break,
        };
        let count_pos = pos + count_rel;
        let num_start = count_pos + count_key.len();

        // First byte after "ItemCount": must be an ASCII digit.
        // Binary game structures also use this key but with non-ASCII integer bytes.
        if num_start >= data.len() || !data[num_start].is_ascii_digit() {
            pos = count_pos + 1;
            continue;
        }

        let qty = match parse_int(data, num_start) {
            Some(n) if n > 0 => n,
            _ => { pos = count_pos + 1; continue; }
        };

        // Require strict adjacency: digits must be immediately followed by ,"ItemType":"
        // with nothing in between — no brackets, no other fields.
        let num_end = digits_end(data, num_start);
        if num_end >= data.len() || data[num_end] != b',' {
            pos = count_pos + 1;
            continue;
        }
        let after_comma = num_end + 1;
        if data.len() < after_comma + type_key.len()
            || &data[after_comma..after_comma + type_key.len()] != type_key
        {
            pos = count_pos + 1;
            continue;
        }
        let type_start = after_comma + type_key.len();

        let path_end = match data[type_start..].iter().take(512).position(|&b| b == b'"') {
            Some(e) => type_start + e,
            None => { pos = count_pos + 1; continue; }
        };

        let path = match valid_lotus_path(&data[type_start..path_end]) {
            Some(p) => p,
            None => { pos = count_pos + 1; continue; }
        };

        if unique_paths.contains(&path) { pos = count_pos + 1; continue; }
        if path.starts_with("/Lotus/Upgrades/") { pos = count_pos + 1; continue; }

        // Reject Nightwave/store price entries. Store JSON looks like:
        //   "ItemPrices":[{"ItemCount":25,"ItemType":"/Lotus/...","ProductCategory":"MiscItems"}]
        // Inventory items never have "ProductCategory" after the path — skip if found within 20 bytes.
        {
            let after = path_end + 1;
            let window = data.get(after..after + 20).unwrap_or(&data[after.min(data.len())..]);
            if window.windows(b"\"ProductCategory\"".len()).any(|w| w == b"\"ProductCategory\"") {
                pos = count_pos + 1;
                continue;
            }
        }

        // Reject stale heap allocations: live inventory JSON is pure printable ASCII,
        // but freed/reused allocations have binary garbage before the fragment.
        if !has_clean_prefix(data, count_pos, 300) { pos = count_pos + 1; continue; }

        let cap: i64 = if path.starts_with("/Lotus/Types/Recipes/") { 9_999 } else { 1_000_000 };
        if qty <= cap {
            if path.starts_with("/Lotus/Types/Items/FusionTreasures/") {
                // FusionTreasures appears in both the authoritative inventory array
                // and in InventoryChanges delta blobs (per-mission reward deltas).
                // Delta blobs have "InventoryChanges" within ~1 KB before the match;
                // skip them so we only count the real totals, not per-session deltas.
                const INV_CHANGES: &[u8] = b"\"InventoryChanges\"";
                let look_start = count_pos.saturating_sub(1024);
                let in_delta = data[look_start..count_pos]
                    .windows(INV_CHANGES.len())
                    .any(|w| w == INV_CHANGES);
                if in_delta { pos = count_pos + 1; continue; }

                // Same sculpture type can appear multiple times in the FusionTreasures
                // array with different Sockets values (empty vs filled). Sum them all.
                let entry = results.entry(path).or_insert_with(|| {
                    (0, extract_context(data, count_pos, 300, 200))
                });
                entry.0 += qty;
            } else {
                // Keep the MAX quantity across all occurrences. The 1.5 MB hint read spans
                // multiple VirtualAlloc regions and may contain several MiscItems blocks
                // (e.g. a small loadout-delta block before the full account inventory block).
                // The authoritative account block always has the highest counts, so max-wins
                // is more reliable than first-wins for cross-region reads.
                let entry = results.entry(path).or_insert_with(|| {
                    (qty, extract_context(data, count_pos, 300, 200))
                });
                if qty > entry.0 { entry.0 = qty; }
            }
        }

        pos = path_end + 1;
    }

    results.into_iter().map(|(k, (q, c))| (k, q, c)).collect()
}

// ─── Scanner 1b: Mods / Arcanes ──────────────────────────────────────────────
//
// Searches the entire region for "ItemType":"/Lotus/Upgrades/" entries without
// requiring a specific enclosing array name. DE previously used "RawUpgrades"
// as the array header but that field no longer appears in memory.
//
// Each mod/arcane entry looks like:
//   {"ItemCount":N,"ItemType":"/Lotus/Upgrades/...","ItemLevel":R}
// ItemCount is found by walking backwards from ItemType (brace-depth tracking).
// ItemLevel (rank) is found by scanning forward from ItemType to the closing '}'.
// Entries without ItemCount default to qty=1. Entries without ItemLevel are rank 0.
//
// Delta blobs (InventoryChanges) are filtered by a 200-byte lookback for
// "InventoryChanges". Stale heap data is rejected by has_clean_prefix.

fn scan_inventory_mods(data: &[u8]) -> Vec<(String, ModCount, String)> {
    const TYPE_KEY:  &[u8] = b"\"ItemType\":\"";
    const COUNT_KEY: &[u8] = b"\"ItemCount\":";
    const LEVEL_KEY: &[u8] = b"\"ItemLevel\":";
    const MOD_PFX:   &[u8] = b"/Lotus/Upgrades/";
    const INV_CHANGES_KEY: &[u8] = b"\"InventoryChanges\"";

    let mut results: HashMap<String, (ModCount, String)> = HashMap::new();
    let mut pos = 0usize;

    loop {
        // Find the next "ItemType":"/Lotus/Upgrades/" hit anywhere in the region.
        let type_rel = match data[pos..].windows(TYPE_KEY.len()).position(|w| w == TYPE_KEY) {
            Some(p) => p,
            None => break,
        };
        let type_abs = pos + type_rel;
        let path_start = type_abs + TYPE_KEY.len();

        if data.len() < path_start + MOD_PFX.len()
            || &data[path_start..path_start + MOD_PFX.len()] != MOD_PFX
        {
            pos = type_abs + 1;
            continue;
        }

        let path_end = match data[path_start..].iter().take(512).position(|&b| b == b'"') {
            Some(e) => path_start + e,
            None => { pos = type_abs + 1; continue; }
        };

        let path = match valid_lotus_path(&data[path_start..path_end]) {
            Some(p) => p,
            None => { pos = type_abs + 1; continue; }
        };

        // Filter InventoryChanges delta entries — these carry per-mission deltas,
        // not authoritative totals. They appear with "InventoryChanges" within 200 bytes.
        let look_back = type_abs.saturating_sub(200);
        if data[look_back..type_abs].windows(INV_CHANGES_KEY.len()).any(|w| w == INV_CHANGES_KEY) {
            pos = path_end + 1;
            continue;
        }

        // Reject stale heap data — live JSON is clean ASCII.
        if !has_clean_prefix(data, type_abs, 300) {
            pos = path_end + 1;
            continue;
        }

        // Walk backwards from "ItemType" to find "ItemCount" in this object.
        // Track whether ItemCount was explicitly present: cosmetics in MiscItems have an
        // explicit ItemCount (single canonical value — use MAX to deduplicate stale blobs).
        // RawUpgrades mod entries have no ItemCount — each occurrence is a distinct copy,
        // so we use SUM to get the true per-rank count.
        let (qty, has_explicit_count) = {
            let search_start = type_abs.saturating_sub(512);
            let before = &data[search_start..type_abs];
            let mut qty_val = 1i64;
            let mut found = false;
            let mut depth: i32 = 0;
            let mut idx = before.len();
            while idx > 0 {
                idx -= 1;
                match before[idx] {
                    b'}' => depth += 1,
                    b'{' => {
                        if depth == 0 { break; }
                        depth -= 1;
                    }
                    _ => {}
                }
                if depth == 0
                    && idx + COUNT_KEY.len() <= before.len()
                    && &before[idx..idx + COUNT_KEY.len()] == COUNT_KEY
                {
                    let num_start = search_start + idx + COUNT_KEY.len();
                    if num_start < data.len() && data[num_start].is_ascii_digit() {
                        qty_val = parse_int(data, num_start).unwrap_or(1).max(1);
                        found = true;
                    }
                    break;
                }
            }
            (qty_val, found)
        };

        // Find rank: check BEFORE and AFTER ItemType for UpgradeFingerprint.
        //
        // New format: {"UpgradeFingerprint":"{\"lvl\":N}","ItemType":"...","ItemId":{...}}
        //   → FP comes BEFORE ItemType; forward scan from path_end misses it entirely.
        //
        // Old/MiscItems format: {"ItemType":"...","UpgradeFingerprint":"{\"lvl\":N}",...}
        //   → FP comes AFTER ItemType.
        //
        // Legacy fallback: "ItemLevel":N
        let rank: u8 = {
            const FP_STR_KEY:    &[u8] = b"\"UpgradeFingerprint\":\"";
            const FP_OBJ_KEY:    &[u8] = b"\"UpgradeFingerprint\":{";
            // \"lvl\": as raw bytes in escaped fingerprint string value.
            const LVL_ESCAPED:   &[u8] = b"\\\"lvl\\\":";
            // "lvl": as raw bytes in inline object or unescaped context.
            const LVL_UNESCAPED: &[u8] = b"\"lvl\":";

            // Helper: extract rank from a fingerprint string starting at fp_start in buf,
            // where inner quotes are backslash-escaped.
            fn rank_from_fp_str(buf: &[u8], fp_start: usize, base_abs: usize, data: &[u8]) -> u8 {
                // Walk forward, skipping \X pairs, to find the real closing '"'.
                let mut i = fp_start;
                let mut fp_end = fp_start;
                while i < buf.len() {
                    if buf[i] == b'\\' { i += 2; continue; }
                    if buf[i] == b'"' { fp_end = i; break; }
                    i += 1;
                }
                let fp_bytes = &buf[fp_start..fp_end];
                if let Some(lvl_rel) = fp_bytes.windows(LVL_ESCAPED.len()).position(|w| w == LVL_ESCAPED) {
                    let num_abs = base_abs + fp_start + lvl_rel + LVL_ESCAPED.len();
                    parse_int(data, num_abs).unwrap_or(0).min(255) as u8
                } else { 0 }
            }

            // ── Backward search: FP before ItemType (new RawUpgrades format) ──
            // Search within 96 bytes before type_abs; take the RIGHTMOST match
            // (closest to ItemType = belonging to this entry, not the previous one).
            let rank_before = {
                let bstart = type_abs.saturating_sub(96);
                let before = &data[bstart..type_abs];
                let fp_rel_opt = before.windows(FP_STR_KEY.len())
                    .enumerate().rev()
                    .find_map(|(i, w)| if w == FP_STR_KEY { Some(i) } else { None });
                if let Some(fp_rel) = fp_rel_opt {
                    let fp_start_in_before = fp_rel + FP_STR_KEY.len();
                    rank_from_fp_str(before, fp_start_in_before, bstart, data)
                } else {
                    // Also try inline-object form before ItemType.
                    let fp_obj_rel = before.windows(FP_OBJ_KEY.len())
                        .enumerate().rev()
                        .find_map(|(i, w)| if w == FP_OBJ_KEY { Some(i) } else { None });
                    if let Some(rel) = fp_obj_rel {
                        let content_start = bstart + rel + FP_OBJ_KEY.len() - 1;
                        let end = content_start.min(type_abs);
                        if let Some(lvl_rel) = data[content_start..end].windows(LVL_UNESCAPED.len()).position(|w| w == LVL_UNESCAPED) {
                            let num_abs = content_start + lvl_rel + LVL_UNESCAPED.len();
                            parse_int(data, num_abs).unwrap_or(0).min(255) as u8
                        } else { 0 }
                    } else { 0 }
                }
            };

            // ── Forward search: FP after ItemType (old/MiscItems format) ──
            let rank_after = if rank_before == 0 {
                let forward_start = path_end + 1;
                let forward_end = (forward_start + 256).min(data.len());
                let after = &data[forward_start..forward_end];
                let entry_close = {
                    let mut depth = 0i32;
                    let mut close = after.len();
                    for (i, &b) in after.iter().enumerate() {
                        match b { b'{' => depth += 1, b'}' => { if depth == 0 { close = i; break; } depth -= 1; } _ => {} }
                    }
                    close
                };
                let entry_slice = &after[..entry_close];
                if let Some(fp_rel) = entry_slice.windows(FP_STR_KEY.len()).position(|w| w == FP_STR_KEY) {
                    rank_from_fp_str(entry_slice, fp_rel + FP_STR_KEY.len(), forward_start, data)
                } else if let Some(fp_rel) = entry_slice.windows(FP_OBJ_KEY.len()).position(|w| w == FP_OBJ_KEY) {
                    let cs = fp_rel + FP_OBJ_KEY.len() - 1;
                    if let Some(lvl_rel) = entry_slice[cs..].windows(LVL_UNESCAPED.len()).position(|w| w == LVL_UNESCAPED) {
                        let num_abs = forward_start + cs + lvl_rel + LVL_UNESCAPED.len();
                        parse_int(data, num_abs).unwrap_or(0).min(255) as u8
                    } else { 0 }
                } else if let Some(rel) = entry_slice.windows(LEVEL_KEY.len()).position(|w| w == LEVEL_KEY) {
                    let num_start = forward_start + rel + LEVEL_KEY.len();
                    parse_int(data, num_start).unwrap_or(0).min(255) as u8
                } else { 0 }
            } else { 0 };

            rank_before.max(rank_after)
        };

        // Accumulate per rank.
        // MiscItems entries (explicit ItemCount): use MAX to deduplicate stale heap blobs.
        // RawUpgrades entries (no ItemCount, qty=1): use SUM — each occurrence is one copy.
        let entry = results.entry(path.clone()).or_insert_with(|| {
            (ModCount::default(), extract_context(data, type_abs, 300, 200))
        });
        let rank_cnt = entry.0.by_rank.entry(rank).or_insert(0);
        if has_explicit_count {
            *rank_cnt = (*rank_cnt).max(qty);
        } else {
            *rank_cnt += qty;
        }
        pos = path_end + 1;
    }

    // Recompute totals from by_rank (max-per-rank values set above).
    for (_, (mc, _)) in &mut results {
        mc.total = mc.by_rank.values().sum();
    }

    results.into_iter().map(|(k, (mc, c))| (k, mc, c)).collect()
}

// ─── Archon Shard extractor ───────────────────────────────────────────────────
//
// Reads ArchonCrystalUpgrades from a warframe's JSON entry in memory.
// Called from scan_inventory_unique for every validated warframe hit.
// Returns an empty Vec when no shards are socketed or the field is absent.

/// Returns `(shards, diag)`.  `diag` is non-empty on the first successful find
/// and contains the raw printable ASCII of the first 300 bytes of the array —
/// used to diagnose the actual Color field format when colors come back empty.
fn extract_archon_shards(data: &[u8], search_start: usize, search_end: usize) -> (Vec<ArchonShard>, String) {
    const SHARD_KEY:   &[u8] = b"\"ArchonCrystalUpgrades\":[";
    const UPGRADE_KEY: &[u8] = b"\"UpgradeType\":\"";

    let end = search_end.min(data.len());
    if search_start >= end { return (vec![], String::new()); }

    // Locate the ArchonCrystalUpgrades array.
    let rel = match data[search_start..end].windows(SHARD_KEY.len()).position(|w| w == SHARD_KEY) {
        Some(r) => r,
        None => return (vec![], String::new()),
    };
    let array_start = search_start + rel + SHARD_KEY.len();

    // Capture the first 300 printable bytes of the array for diagnostics.
    let diag: String = {
        let diag_end = (array_start + 300).min(end);
        data[array_start..diag_end].iter()
            .map(|&b| if b >= 0x20 && b < 0x7f { b as char } else { '·' })
            .collect()
    };

    // Find the matching closing ] (depth-tracked to skip nested arrays).
    let array_end = {
        let mut depth = 1i32;
        let mut found = end; // fallback: scan to limit
        for (i, &b) in data[array_start..end].iter().enumerate() {
            match b {
                b'[' => depth += 1,
                b']' => { depth -= 1; if depth == 0 { found = array_start + i; break; } }
                _ => {}
            }
        }
        found
    };

    // Color field keys — try quoted string first, fall back to integer.
    // Scope: only within the current entry ({...}).  Find the entry's } boundary,
    // then search Color within [entry_open, entry_close].
    const COLOR_STR: &[u8] = b"\"Color\":\"";   // "Color":"ACC_CRIMSON"
    const COLOR_INT: &[u8] = b"\"Color\":";      // "Color":0

    // Parse each {"UpgradeType":"...", "Color":...} entry within the array.
    let mut shards = Vec::new();
    let mut pos = array_start;
    loop {
        if pos >= array_end { break; }
        let Some(ur) = data[pos..array_end].windows(UPGRADE_KEY.len()).position(|w| w == UPGRADE_KEY) else { break };
        let path_start = pos + ur + UPGRADE_KEY.len();
        let Some(pend_rel) = data[path_start..array_end].iter().position(|&b| b == b'"') else { break };
        let path_end = path_start + pend_rel;

        if let Some(upgrade_type) = valid_lotus_path(&data[path_start..path_end]) {
            // Find the object boundaries of the current entry so Color search stays scoped.
            // entry_open: last { before the UpgradeType key start.
            let entry_open = data[pos..(pos + ur)].iter().rposition(|&b| b == b'{')
                .map(|r| pos + r + 1)
                .unwrap_or(pos);
            // entry_close: first } at depth 0 after entry_open.
            let entry_close = {
                let mut d = 1i32;
                let mut found = array_end;
                for (i, &b) in data[entry_open..array_end].iter().enumerate() {
                    match b { b'{' => d += 1, b'}' => { d -= 1; if d == 0 { found = entry_open + i; break; } } _ => {} }
                }
                found
            };

            let color = if let Some(cr) = data[entry_open..entry_close].windows(COLOR_STR.len()).position(|w| w == COLOR_STR) {
                let vs = entry_open + cr + COLOR_STR.len();
                let ve = data[vs..entry_close].iter().position(|&b| b == b'"').map(|e| vs + e).unwrap_or(vs);
                std::str::from_utf8(&data[vs..ve]).unwrap_or("").to_string()
            } else if let Some(cr) = data[entry_open..entry_close].windows(COLOR_INT.len()).position(|w| w == COLOR_INT) {
                parse_int(data, entry_open + cr + COLOR_INT.len())
                    .map(|n| n.to_string())
                    .unwrap_or_default()
            } else {
                String::new()
            };
            shards.push(ArchonShard { upgrade_type, color });
        }
        pos = path_end + 1;
    }
    (shards, diag)
}

// ─── Scanner 2: Unique items (warframes / weapons / companions) ───────────────
//
// Finds owned warframes, weapons, companions and archwings via:
//   "ItemType":"/Lotus/<path>","ItemId":{"$oid":"..."},...,"Configs":[...]
//
// Uses Aho-Corasick for all catalogued paths. Validates:
//   - "ItemId": within ±200 bytes (owned item, not relay/market data)
//   - "Configs": within 2000 bytes after the match (full loadout present)
//
// `ac` must be built once before the per-region loop.

/// Returns (pattern_idx, rank, shards).
fn scan_inventory_unique(data: &[u8], ac: &aho_corasick::AhoCorasick, unique_item_paths: &[String]) -> Vec<(usize, Option<u32>, Vec<ArchonShard>, String)> {
    let mut hits: Vec<(usize, Option<u32>, Vec<ArchonShard>, String)> = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for mat in ac.find_iter(data) {
        let idx = mat.pattern().as_usize();
        if !seen.insert(idx) { continue; }

        let start = mat.start();
        let end   = mat.end();

        let has_count_before = start >= 25 && {
            let w = &data[start.saturating_sub(25)..start];
            w.windows(12).any(|s| s == b"\"ItemCount\":")
        };
        if has_count_before { continue; }

        // Player-owned items in the inventory JSON always have "ItemId":{"$oid":"..."}
        // adjacent to "ItemType". NPC/enemy/mission blob items use "_id" (not "ItemId")
        // or no instance ID at all. By requiring "ItemId" in a tight 600-byte window
        // we reject paths from mission blobs, NPC loadouts, and other non-inventory
        // regions that happen to have a generic "_id" field somewhere in 15 KB.
        // (The old 5000/10000-byte window was too wide: it matched "_id" from completely
        // different JSON objects in the same memory region.)
        let id_pre  = start.saturating_sub(600);
        let id_post = (end + 600).min(data.len());
        if !data[id_pre..id_post].windows(9).any(|w| w == b"\"ItemId\":") { continue; }

        // Find the next item entry boundary so we don't bleed into adjacent items.
        // Used for both XP and Archon Shard searches.
        const NEXT_ITEM_KEY: &[u8] = b"\"ItemType\":\"/Lotus/";
        let item_entry_end = {
            let look_end = (end + 30_000).min(data.len());
            data[end..look_end]
                .windows(NEXT_ITEM_KEY.len())
                .position(|w| w == NEXT_ITEM_KEY)
                .map(|r| end + r)
                .unwrap_or(look_end)
        };

        // Extract "XP":N — cumulative affinity for this item.
        // The field appears AFTER the full "Configs" array (mod loadout), so it can be
        // several KB past "ItemType":. Blob confirmed Banshee's XP is ~2500 bytes after
        // the path.  Search up to the next item boundary to safely find it.
        // Rank derived from cumulative affinity XP using the wiki formula (see xp_to_rank).
        const XP_KEY: &[u8] = b"\"XP\":";
        let xp_rank: Option<u32> = {
            let path = unique_item_paths.get(idx).map(|s| s.as_str()).unwrap_or("");
            data[end..item_entry_end].windows(XP_KEY.len()).position(|w| w == XP_KEY)
                .and_then(|r| parse_int(data, end + r + XP_KEY.len()))
                .map(|xp| xp_to_rank(xp, path))
        };

        // Extract ArchonCrystalUpgrades — also after the Configs array.
        let (shards, diag) = extract_archon_shards(data, end, item_entry_end);

        hits.push((idx, xp_rank, shards, diag));
    }
    hits
}

// ─── Scanner 3: Pending foundry recipes ──────────────────────────────────────
//
// Warframe stores active crafting jobs in the inventory JSON as:
//   "PendingRecipes":[{"ItemType":"/Lotus/Types/Recipes/...","CompletionDate":{"$date":N},...}]
//
// "CompletionDate":{"$date":N} uses a Unix timestamp in milliseconds.
// Returns one PendingRecipe per active craft (may include long-running builds).

/// Diagnostic: find "CompletionDate" in any format and return a snippet of context.
#[allow(dead_code)]
pub fn scan_completion_date_context(data: &[u8]) -> Vec<String> {
    let key = b"\"CompletionDate\"";
    let mut results = Vec::new();
    let mut start = 0usize;
    loop {
        let next = match data[start..].iter().position(|&b| b == b'"') {
            Some(p) => start + p,
            None => break,
        };
        if next + key.len() > data.len() { break; }
        if data[next..next + key.len()] != *key {
            start = next + 1; continue;
        }
        // Capture 120 bytes of context starting 40 bytes before the key
        let ctx_start = next.saturating_sub(40);
        let ctx_end   = (next + 120).min(data.len());
        let ctx = &data[ctx_start..ctx_end];
        // Only include printable ASCII so the log is readable
        let s: String = ctx.iter()
            .map(|&b| if b >= 0x20 && b < 0x7f { b as char } else { '·' })
            .collect();
        results.push(s);
        start = next + key.len();
        if results.len() >= 3 { break; } // cap at 3 samples
    }
    results
}

fn scan_pending_recipes(data: &[u8]) -> Vec<PendingRecipe> {
    // Format in memory (unescaped JSON):
    //   "ItemType":"/Lotus/...","CompletionDate":{"$date":{"$numberLong":"1777056987000"}}
    //
    // The key was correct before; the bug was timestamp parsing expecting a bare number
    // but finding {"$numberLong":"..."} instead.
    let completion_key = b"\"CompletionDate\":{\"$date\":{\"$numberLong\":\"";
    let type_key       = b"\"ItemType\":\"";

    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);

    let mut results: Vec<PendingRecipe> = Vec::new();
    let mut search = 0usize;

    loop {
        let next = match data[search..].iter().position(|&b| b == b'"') {
            Some(p) => search + p,
            None => break,
        };
        if next + completion_key.len() > data.len() { break; }
        if data[next..next + completion_key.len()] != *completion_key {
            search = next + 1; continue;
        }
        let ts_start = next + completion_key.len();
        search = ts_start;

        // Timestamp digits end at the closing "
        let completion_ms = match parse_int(data, ts_start) {
            Some(n) if n > 1_000_000_000_000 => n,
            _ => continue,
        };

        // Only include crafts not yet finished
        if completion_ms <= now_ms { continue; }

        // Look backward up to 512 bytes for "ItemType":"/Lotus/..."
        let back_start = next.saturating_sub(512);
        let back_slice = &data[back_start..next];
        if let Some(rel) = back_slice.windows(type_key.len()).rposition(|w| w == *type_key) {
            let path_start = back_start + rel + type_key.len();
            if path_start < next {
                let path_slice = &data[path_start..next];
                if let Some(close) = path_slice.iter().position(|&b| b == b'"') {
                    if let Some(path) = valid_lotus_path(&path_slice[..close]) {
                        if !results.iter().any(|r| r.unique_name == path) {
                            results.push(PendingRecipe { unique_name: path, completion_ms });
                        }
                    }
                }
            }
        }
    }

    results
}

// ─── Scanner: Consumed suits (Helminth subsumed warframes) ───────────────────
//
// "ConsumedSuits" lives in the InfestedFoundry object of the same inventory
// JSON blob as MiscItems.  Each entry is either a bare path string or an object
// with an "ItemType" field — we extract all /Lotus/ paths we find between the
// opening "[" and closing "]" of the array.

fn scan_consumed_suits(data: &[u8]) -> Vec<String> {
    const KEY: &[u8] = b"\"ConsumedSuits\":[";
    let Some(key_pos) = data.windows(KEY.len()).position(|w| w == KEY) else { return vec![] };
    let start = key_pos + KEY.len();
    // Scan forward up to 8 KB for the closing bracket (handles large subsumption lists)
    let window = &data[start..data.len().min(start + 8192)];
    let end = window.iter().position(|&b| b == b']').unwrap_or(window.len());
    let window = &window[..end];

    let lotus: &[u8] = b"\"/Lotus/";
    let mut results = Vec::new();
    let mut pos = 0;
    while pos + lotus.len() < window.len() {
        let Some(found) = window[pos..].windows(lotus.len()).position(|w| w == lotus) else { break };
        let path_start = pos + found + 1; // skip opening "
        let Some(close) = window[path_start..].iter().position(|&b| b == b'"') else { break };
        if let Ok(s) = std::str::from_utf8(&window[path_start..path_start + close]) {
            results.push(s.to_string());
        }
        pos = path_start + close + 1;
    }
    results
}

// ─── XPInfo scanner ──────────────────────────────────────────────────────────
//
// "XPInfo":[{"ItemType":"/Lotus/...","XP":N}, ...]  lives at the inventory root
// alongside MiscItems.  Unlike per-item XP (read during the unique-item scan),
// this array covers EVERY item the account has ever levelled — including sold,
// deleted, and Helminth-subsumed items.  It is the authoritative mastery history.

fn scan_xpinfo(data: &[u8]) -> HashMap<String, u32> {
    const KEY: &[u8] = b"\"XPInfo\":[";
    let Some(key_pos) = data.windows(KEY.len()).position(|w| w == KEY) else { return HashMap::new() };
    let start = key_pos + KEY.len();

    // Array can hold 1 000+ entries (~70 bytes each) — allow up to 256 KB.
    let window_end = data.len().min(start + 256 * 1024);
    let array_end = {
        let mut depth = 1i32;
        let mut found = window_end;
        for (i, &b) in data[start..window_end].iter().enumerate() {
            match b {
                b'[' => depth += 1,
                b']' => { depth -= 1; if depth == 0 { found = start + i; break; } }
                _ => {}
            }
        }
        found
    };

    const ITEM_TYPE_KEY: &[u8] = b"\"ItemType\":\"";
    const XP_KEY: &[u8] = b"\"XP\":";

    let mut result: HashMap<String, u32> = HashMap::new();
    let mut pos = start;
    loop {
        if pos >= array_end { break; }
        let Some(tr) = data[pos..array_end].windows(ITEM_TYPE_KEY.len()).position(|w| w == ITEM_TYPE_KEY) else { break };
        let path_start = pos + tr + ITEM_TYPE_KEY.len();
        let Some(pend) = data[path_start..array_end].iter().position(|&b| b == b'"') else { break };
        let path_end = path_start + pend;

        if let Some(path) = valid_lotus_path(&data[path_start..path_end]) {
            // XP field is always within the same small JSON object — look within 200 bytes.
            let xp_search_end = (path_end + 200).min(array_end);
            if let Some(xr) = data[path_end..xp_search_end].windows(XP_KEY.len()).position(|w| w == XP_KEY) {
                if let Some(xp) = parse_int(data, path_end + xr + XP_KEY.len()) {
                    let rank = xp_to_rank(xp, &path);
                    let e = result.entry(path).or_insert(0u32);
                    if rank > *e { *e = rank; }
                }
            }
        }
        pos = path_end + 1;
    }
    result
}

// ─── Auth credentials scan ───────────────────────────────────────────────────
//
// When Warframe is running and logged in, the game stores the session credentials
// in memory as URL-encoded strings: accountId=<id>&nonce=<nonce>
// We scan for these to authenticate with the Warframe companion API.

pub fn scan_auth_credentials(data: &[u8]) -> Option<(String, String)> {
    // The Warframe game receives a login response JSON from DE's servers containing:
    //   {"id":"<24-char-hex-accountId>","Nonce":<large-integer>,...}
    // We search for this pattern. The Nonce is typically 9-13 digits.
    // We also try URL-encoded form: accountId=<id>&nonce=<nonce>
    //
    // Key insight from devtools: accountId=594144e63ade7f2f2091c48e (24ch), nonce len=9
    // The 24-char hex accountId is a MongoDB ObjectId — correct format.
    // The 9-digit nonce IS valid — it's a server-issued integer session token.

    // Search for "id":"<24hexchars>" near "Nonce":<digits>
    let id_key = b"\"id\":\"";
    let nonce_key = b"\"Nonce\":";
    let mut search = 0usize;
    while search + id_key.len() < data.len() {
        let next = match data[search..].iter().position(|&b| b == b'"') {
            Some(p) => search + p, None => break,
        };
        if next + id_key.len() > data.len() { break; }
        if data[next..next + id_key.len()] != *id_key { search = next + 1; continue; }

        let id_start = next + id_key.len();
        // accountId is exactly 24 lowercase hex chars
        let id_slice = &data[id_start..id_start.saturating_add(26).min(data.len())];
        let close = id_slice.iter().position(|&b| b == b'"').unwrap_or(0);
        if close != 24 { search = next + 1; continue; }
        let id_bytes = &id_slice[..24];
        if !id_bytes.iter().all(|&b| b.is_ascii_hexdigit()) { search = next + 1; continue; }
        let account_id = std::str::from_utf8(id_bytes).unwrap_or("").to_string();

        // Look for Nonce within 2048 bytes
        let nonce_search_end = (id_start + 2048).min(data.len());
        if let Some(rel) = data[id_start..nonce_search_end].windows(nonce_key.len()).position(|w| w == *nonce_key) {
            let ns = id_start + rel + nonce_key.len();
            let ne = digits_end(data, ns);
            if ne > ns && ne - ns >= 5 {
                if let Ok(nonce) = std::str::from_utf8(&data[ns..ne]) {
                    return Some((account_id, nonce.to_string()));
                }
            }
        }
        search = next + 1;
    }

    // URL-encoded: accountId=<24hexchars>&nonce=<10digits>&ct=STM
    let ak = b"accountId=";
    let nk = b"nonce=";
    let mut search = 0usize;
    while search + ak.len() < data.len() {
        let next = match data[search..].iter().position(|&b| b == b'a') {
            Some(p) => search + p, None => break,
        };
        if next + ak.len() > data.len() { break; }
        if data[next..next + ak.len()] != *ak { search = next + 1; continue; }
        let id_start = next + ak.len();
        let id_end = data[id_start..].iter().position(|&b| !b.is_ascii_hexdigit()).map(|p| id_start + p).unwrap_or(data.len());
        if id_end - id_start != 24 { search = next + 1; continue; }
        let account_id = std::str::from_utf8(&data[id_start..id_end]).unwrap_or("").to_string();
        // Nonce can appear anywhere within 512 bytes after the accountId
        let nonce_search_end = (id_end + 512).min(data.len());
        if let Some(rel) = data[id_end..nonce_search_end].windows(nk.len()).position(|w| w == *nk) {
            let ns = id_end + rel + nk.len();
            let ne = digits_end(data, ns);
            if ne > ns && ne - ns >= 5 {
                if let Ok(nonce) = std::str::from_utf8(&data[ns..ne]) {
                    return Some((account_id, nonce.to_string()));
                }
            }
        }
        search = next + 1;
    }
    None
}

/// Also extract steamId from memory (found near accountId/nonce in URL params).
pub fn scan_steam_id(data: &[u8]) -> Option<String> {
    let key = b"steamId=";
    let mut search = 0usize;
    loop {
        let next = match data[search..].iter().position(|&b| b == b's') {
            Some(p) => search + p, None => break,
        };
        if next + key.len() > data.len() { break; }
        if data[next..next + key.len()] != *key { search = next + 1; continue; }
        let id_start = next + key.len();
        let id_end = data[id_start..].iter().position(|&b| !b.is_ascii_digit()).map(|p| id_start + p).unwrap_or(data.len());
        if id_end - id_start >= 15 && id_end - id_start <= 20 {
            if let Ok(sid) = std::str::from_utf8(&data[id_start..id_end]) {
                return Some(sid.to_string());
            }
        }
        search = next + 1;
    }
    None
}

// ─── Mastery rank scan ────────────────────────────────────────────────────────
//
// Warframe stores the player's mastery rank in the inventory JSON as:
//   "PlayerLevel":N
// Returns the first plausible value found (0–30+).

fn scan_mastery_rank(data: &[u8]) -> Option<u32> {
    let key = b"\"PlayerLevel\":";
    let mut start = 0usize;
    loop {
        let next = match data[start..].iter().position(|&b| b == b'"') {
            Some(p) => start + p,
            None => break,
        };
        if next + key.len() > data.len() { break; }
        if data[next..next + key.len()] != *key {
            start = next + 1; continue;
        }
        let num_start = next + key.len();
        if let Some(rank) = parse_int(data, num_start) {
            if rank >= 0 && rank <= 60 {
                return Some(rank as u32);
            }
        }
        start = next + key.len();
    }
    None
}

fn has_number_long_in(data: &[u8]) -> bool {
    const LONG_KEY: &[u8] = b"$numberLong\"";
    data.windows(LONG_KEY.len()).any(|w| w == LONG_KEY)
}

// ─── Main scan entry point ────────────────────────────────────────────────────

#[cfg(target_os = "windows")]
pub fn scan_warframe_memory(
    unique_names: &[String],
    display_names: &[String],
    assembled_names: &[String],
    start_addr: usize,   // 0 = start from beginning; non-zero = resume from this address
    max_secs: u64,       // stop scanning after this many seconds and return resume_addr
    hint_addrs: &[usize], // MiscItems chunk addresses — scanned first for resources+mods
    mod_hint_addrs: &[usize], // RawUpgrades chunk addresses — scanned for mods only
) -> ScanResult {
    use std::ffi::c_void;
    use std::mem;
    use windows_sys::Win32::{
        Foundation::CloseHandle,
        System::{
            Diagnostics::Debug::ReadProcessMemory,
            Memory::{VirtualQueryEx, MEMORY_BASIC_INFORMATION, MEM_COMMIT, PAGE_GUARD, PAGE_NOACCESS},
            Threading::{OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ},
        },
    };

    if unique_names.is_empty() {
        return ScanResult {
            warframe_running: false, items_found: vec![], pending_recipes: vec![], mastery_rank: None, mastery_data: HashMap::new(), regions_scanned: 0,
            error: Some("No item paths loaded. Click 'Refresh item list' first.".to_string()),
            log_lines: vec![], relic_rewards: None, found_actual_inventory: false, resume_addr: 0, hot_addrs: vec![], mod_hot_addrs: vec![], consumed_suits: vec![], mods_found: HashMap::new(), hint_mods: HashMap::new(), hint_resources: HashMap::new(), hint_flavour_items: vec![], socketed_shards: HashMap::new(),
        };
    }

    let display_map: HashMap<String, String> = unique_names.iter()
        .zip(display_names.iter())
        .map(|(u, d)| (u.clone(), d.clone()))
        .collect();

    // Unique-item paths: assembled items owned via ItemId+Configs in the inventory JSON.
    // assembled_names is pre-filtered in lib.rs using fix_category so that component parts
    // sharing a /Lotus/Weapons/ path prefix (e.g. "Paris Prime String") are NOT included
    // here — those parts have ItemCount and must be processed by Scanner 1, not skipped.
    // NOTE: /Lotus/Types/Recipes/ is intentionally excluded — recipe blueprints
    // are stackable resources with ItemCount, handled by scanner 1, not here.
    let unique_item_paths: Vec<String> = assembled_names.iter()
        .filter(|p| {
            p.starts_with("/Lotus/Powersuits/")
                || p.starts_with("/Lotus/Weapons/")
                || p.starts_with("/Lotus/Archwing/")
                || p.starts_with("/Lotus/Types/Sentinels/SentinelPowersuits/")
                || p.starts_with("/Lotus/Types/Sentinels/SentinelWeapons/")
                || p.starts_with("/Lotus/Types/Friendly/")
                || p.starts_with("/Lotus/Types/Game/CatbrowPet/")
                || p.starts_with("/Lotus/Types/Game/KubrowPet/")
        })
        .cloned()
        .collect();

    // Set of paths handled by the unique scanner — resource scanner skips exactly these
    let unique_path_set: std::collections::HashSet<String> =
        unique_item_paths.iter().cloned().collect();

    // Build Aho-Corasick once — never inside the per-region loop
    let unique_ac = {
        use aho_corasick::AhoCorasick;
        let patterns: Vec<Vec<u8>> = unique_item_paths.iter().map(|p| {
            let mut pat = b"\"ItemType\":\"".to_vec();
            pat.extend_from_slice(p.as_bytes());
            pat.push(b'"');
            pat
        }).collect();
        let refs: Vec<&[u8]> = patterns.iter().map(|p| p.as_slice()).collect();
        match AhoCorasick::new(&refs) {
            Ok(a) => a,
            Err(e) => return ScanResult {
                warframe_running: false, items_found: vec![], pending_recipes: vec![], mastery_rank: None, mastery_data: HashMap::new(), regions_scanned: 0,
                error: Some(format!("AC build error: {}", e)),
                log_lines: vec![], relic_rewards: None, found_actual_inventory: false, resume_addr: 0, hot_addrs: vec![], mod_hot_addrs: vec![], consumed_suits: vec![], mods_found: HashMap::new(), hint_mods: HashMap::new(), hint_resources: HashMap::new(), hint_flavour_items: vec![], socketed_shards: HashMap::new(),
            },
        }
    };

    let pid = match find_warframe_pid() {
        Some(p) => p,
        None => return ScanResult {
            warframe_running: false, items_found: vec![], pending_recipes: vec![], mastery_rank: None, mastery_data: HashMap::new(), regions_scanned: 0,
            error: Some("Warframe is not running. Launch the game first.".to_string()),
            log_lines: vec!["[pid] find_warframe_pid returned None — process not found via ToolHelp snapshot".to_string()],
            relic_rewards: None, found_actual_inventory: false, resume_addr: 0, hot_addrs: vec![], mod_hot_addrs: vec![], consumed_suits: vec![], mods_found: HashMap::new(), hint_mods: HashMap::new(), hint_resources: HashMap::new(), hint_flavour_items: vec![], socketed_shards: HashMap::new(),
        },
    };

    let mut resources:    HashMap<String, (i64, String)> = HashMap::new();
    let mut hint_resources_out: HashMap<String, i64> = HashMap::new(); // hint-only MiscItems counts
    let mut hint_flavour_out: Vec<String> = Vec::new();               // hint-only FlavourItems paths
    let mut mods:         HashMap<String, (ModCount, String)> = HashMap::new(); // path → (count+ranks, ctx)
    let mut hint_mods_out: HashMap<String, ModCount> = HashMap::new(); // hint-only mods for stability
    let mut unique:          HashMap<String, usize>          = HashMap::new(); // path → best region hit-count
    let mut mastery_data:    HashMap<String, u32>            = HashMap::new(); // path → max rank seen
    let mut socketed_shards: HashMap<String, Vec<ArchonShard>> = HashMap::new(); // warframe path → shards
    let mut pending_recipes: Vec<PendingRecipe>              = Vec::new();
    let mut mastery_rank:    Option<u32>                   = None;
    let mut regions_scanned = 0usize;
    let mut log_lines: Vec<String> = vec![
        format!("[pid] found Warframe pid={}", pid),
        format!("[setup] unique_paths={} assembled={}", unique_item_paths.len(), assembled_names.len()),
    ];
    // Per-scan probe counter — log context for the first 5 regions that contain "ItemCount":
    let mut res_probe_count = 0usize;
    // Chunk addresses where the inventory root was found — returned so the caller
    // can pass them back as hint_addrs next call for a near-instant re-scan.
    let mut hot_addrs_out: Vec<usize> = Vec::new();
    let mut mod_hot_addrs_out: Vec<usize> = Vec::new();
    let mut consumed_suits_out: Vec<String> = Vec::new();
    let mut found_actual_inventory_out = false;
    // Declared outside unsafe so it's readable in the ScanResult at the end.
    let mut resume_addr_out: usize = 0;

    unsafe {
        let process = OpenProcess(PROCESS_VM_READ | PROCESS_QUERY_INFORMATION, 0, pid);
        if process == 0 {
            let err_code = windows_sys::Win32::Foundation::GetLastError();
            return ScanResult {
                warframe_running: true, items_found: vec![], pending_recipes: vec![], mastery_rank: None, mastery_data: HashMap::new(), regions_scanned: 0,
                error: Some(format!("Cannot open Warframe process (error {}). Run as Administrator.", err_code)),
                log_lines: vec![format!("[pid] OpenProcess failed for pid={} error={}", pid, err_code)],
                relic_rewards: None, found_actual_inventory: false, resume_addr: 0, hot_addrs: vec![], mod_hot_addrs: vec![], consumed_suits: vec![], mods_found: HashMap::new(), hint_mods: HashMap::new(), hint_resources: HashMap::new(), hint_flavour_items: vec![], socketed_shards: HashMap::new(),
            };
        }

        let mut address: usize = if start_addr >= 0x10000 { start_addr } else { 0x10000 };
        let mbi_size = mem::size_of::<MEMORY_BASIC_INFORMATION>();
        let start_time = std::time::Instant::now();

        // ── Fast path: re-scan previously discovered hot addresses first ──────
        // Skips the rolling VirtualQueryEx walk for the most common case (steady-
        // state: inventory JSON sits at the same heap address between game sessions).
        const CHUNK_SIZE_HINT: usize = 8 * 1024 * 1024;
        // Inventory root blobs span ~7 VirtualAlloc regions (~570 KB total).
        // ReadProcessMemory can cross region boundaries; using 1.5 MB ensures the
        // MiscItems region (which can be at offset ~283 KB from the Created region)
        // is always covered without being artificially cut off at region_end.
        const HINT_MAX_READ: usize = 1_500_000;
        const MISC_KEY: &[u8] = b"\"MiscItems\":[{";
        for &hint_base in hint_addrs {
            // Skip hints in the EXE/DLL image range — these are false positives.
            if hint_base >= 0x0004_0000_0000_0000 { continue; }
            let mut mbi: MEMORY_BASIC_INFORMATION = mem::zeroed();
            if VirtualQueryEx(process, hint_base as *const c_void, &mut mbi, mbi_size) == 0 { continue; }
            if mbi.State != MEM_COMMIT { continue; }
            let p = mbi.Protect;
            if p & PAGE_NOACCESS != 0 || p & PAGE_GUARD != 0 { continue; }
            if (mbi.BaseAddress as usize).saturating_add(mbi.RegionSize) <= hint_base { continue; }
            // Read up to HINT_MAX_READ bytes regardless of single-region boundary.
            // ReadProcessMemory succeeds across boundaries while all regions are committed.
            let read_size = HINT_MAX_READ.min(CHUNK_SIZE_HINT);
            let mut buf = vec![0u8; read_size];
            let mut bytes_read = 0usize;
            let ok = ReadProcessMemory(process, hint_base as *const c_void,
                buf.as_mut_ptr() as *mut c_void, read_size, &mut bytes_read);
            if ok == 0 || bytes_read < 16 { continue; }
            let data = &buf[..bytes_read];
            log_lines.push(format!("  [hint-read] addr=0x{:x} region_size={} bytes_read={}", hint_base, mbi.RegionSize, bytes_read));
            if !data.windows(MISC_KEY.len()).any(|w| w == MISC_KEY) {
                log_lines.push(format!("  [hint-skip] addr=0x{:x} no MISC_KEY", hint_base));
                continue;
            }
            let misc_key_off = data.windows(MISC_KEY.len()).position(|w| w == MISC_KEY).unwrap_or(0);
            // Still valid — run resource and mod scanners on it
            hot_addrs_out.push(hint_base);
            regions_scanned += 1;
            let res_pairs = scan_inventory_resources(data, &unique_path_set);
            if !res_pairs.is_empty() {
                log_lines.push(format!("  [hint-resources] count={} addr=0x{:x} misc_key_at={}", res_pairs.len(), hint_base, misc_key_off));
                for (path, qty, ctx) in res_pairs {
                    hint_resources_out.insert(path.clone(), qty);
                    resources.entry(path).or_insert((qty, ctx));
                }
            }
            for (path, qty) in scan_currency_fields(data) {
                // Max-wins: multiple hint addresses may each report a currency value;
                // always keep the highest (the account inventory total is always greatest).
                let he = hint_resources_out.entry(path.to_string()).or_insert(0);
                if qty > *he { *he = qty; }
                let re = resources.entry(path.to_string()).or_insert((qty, String::new()));
                if qty > re.0 { re.0 = qty; }
            }
            let flavour_paths = scan_flavour_items(data);
            if !flavour_paths.is_empty() {
                log_lines.push(format!("  [hint-flavour] count={}", flavour_paths.len()));
                for path in flavour_paths {
                    hint_flavour_out.push(path);
                }
            }
            let mod_pairs = scan_inventory_mods(data);
            if !mod_pairs.is_empty() {
                log_lines.push(format!("  [hint-mods] count={}", mod_pairs.len()));
                for (i, (path, mc, _ctx)) in mod_pairs.iter().enumerate().take(3) {
                    log_lines.push(format!("  [hint-mod-probe#{}] {} ranks={:?}", i+1, path.split('/').last().unwrap_or("?"), mc.by_rank));
                }
                for (path, mc, ctx) in mod_pairs {
                    let entry = mods.entry(path.clone()).or_insert_with(|| (ModCount::default(), ctx.clone()));
                    if mc.total > entry.0.total { entry.0 = mc.clone(); entry.1 = ctx; }
                    let hint_entry = hint_mods_out.entry(path).or_insert_with(ModCount::default);
                    // Per-rank MAX merge: MiscItems gives count of unranked copies;
                    // RawUpgrades gives ranked copies (1 per entry). MAX per rank correctly
                    // combines both without double-counting or discarding non-zero ranks.
                    for (&r, &cnt) in &mc.by_rank {
                        let e = hint_entry.by_rank.entry(r).or_insert(0);
                        *e = (*e).max(cnt);
                    }
                    hint_entry.total = hint_entry.by_rank.values().sum();
                }
            }
            if mastery_rank.is_none() { mastery_rank = scan_mastery_rank(data); }
            if has_number_long_in(data) {
                for h in scan_pending_recipes(data) { pending_recipes.push(h); }
            }
            let suits = scan_consumed_suits(data);
            if !suits.is_empty() {
                log_lines.push(format!("  [hint-consumed-suits] count={}", suits.len()));
                for s in suits { if !consumed_suits_out.contains(&s) { consumed_suits_out.push(s); } }
            }
            let xp_map = scan_xpinfo(data);
            if !xp_map.is_empty() {
                log_lines.push(format!("  [hint-xpinfo] {} entries", xp_map.len()));
                for (path, rank) in xp_map {
                    let e = mastery_data.entry(path).or_insert(0);
                    if rank > *e { *e = rank; }
                }
            }
        }

        // ── Mod-hint fast path: scan known RawUpgrades chunk addresses for mods only ──
        // These are chunks discovered by the full scan that contain gameplay mods but NOT
        // MiscItems (so they were skipped by the regular hint path above). Scanning them
        // gives us live mod counts from the inventory blob's RawUpgrades section.
        const MOD_HINT_CHECK: &[u8] = b"/Lotus/Upgrades/Mods/";
        for &mod_hint_base in mod_hint_addrs {
            if hot_addrs_out.contains(&mod_hint_base) { continue; } // already scanned above
            if mod_hint_base >= 0x0004_0000_0000_0000 { continue; }
            let mut mbi: MEMORY_BASIC_INFORMATION = mem::zeroed();
            if VirtualQueryEx(process, mod_hint_base as *const c_void, &mut mbi, mbi_size) == 0 { continue; }
            if mbi.State != MEM_COMMIT { continue; }
            let p = mbi.Protect;
            if p & PAGE_NOACCESS != 0 || p & PAGE_GUARD != 0 { continue; }
            let region_end = mbi.BaseAddress as usize + mbi.RegionSize;
            if mod_hint_base >= region_end { continue; }
            let read_size = CHUNK_SIZE_HINT.min(region_end - mod_hint_base);
            let mut buf = vec![0u8; read_size];
            let mut bytes_read = 0usize;
            let ok = ReadProcessMemory(process, mod_hint_base as *const c_void,
                buf.as_mut_ptr() as *mut c_void, read_size, &mut bytes_read);
            if ok == 0 || bytes_read < 16 { continue; }
            let data = &buf[..bytes_read];
            if !data.windows(MOD_HINT_CHECK.len()).any(|w| w == MOD_HINT_CHECK) { continue; }
            regions_scanned += 1;
            let mod_pairs = scan_inventory_mods(data);
            if !mod_pairs.is_empty() {
                log_lines.push(format!("  [mod-hint] addr=0x{:x} count={}", mod_hint_base, mod_pairs.len()));
                // Log first 3 mod contexts so we can verify ItemLevel is present in memory.
                for (i, (path, mc, ctx)) in mod_pairs.iter().enumerate().take(3) {
                    log_lines.push(format!("  [mod-probe#{}] {} ranks={:?} ctx={}", i+1, path.split('/').last().unwrap_or("?"), mc.by_rank, ctx));
                }
                for (path, mc, ctx) in mod_pairs {
                    let entry = mods.entry(path.clone()).or_insert_with(|| (ModCount::default(), ctx.clone()));
                    if mc.total > entry.0.total { entry.0 = mc.clone(); entry.1 = ctx; }
                    let hint_entry = hint_mods_out.entry(path).or_insert_with(ModCount::default);
                    for (&r, &cnt) in &mc.by_rank {
                        let e = hint_entry.by_rank.entry(r).or_insert(0);
                        *e = (*e).max(cnt);
                    }
                    hint_entry.total = hint_entry.by_rank.values().sum();
                }
            }
        }

        'region: loop {
            if start_time.elapsed().as_secs() >= max_secs {
                resume_addr_out = address;
                break;
            }

            let mut mbi: MEMORY_BASIC_INFORMATION = mem::zeroed();
            if VirtualQueryEx(process, address as *const c_void, &mut mbi, mbi_size) == 0 {
                resume_addr_out = 0;
                break;
            }

            let region_end = (mbi.BaseAddress as usize).saturating_add(mbi.RegionSize);
            if region_end <= address { break; }
            address = region_end;

            if mbi.State != MEM_COMMIT { continue; }
            let p = mbi.Protect;
            if p & PAGE_NOACCESS != 0 || p & PAGE_GUARD != 0 { continue; }
            // Skip pure execute pages (code sections) — same filter as raw_scan_pass.
            if p == 0x10 || p == 0x20 { continue; }
            if mbi.RegionSize < 4096 { continue; }
            // No upper size limit — raw_scan_pass has none, and inventory has been
            // confirmed in regions that dump_inventory_regions (256 MB cap) misses.

            const CHUNK_SIZE: usize = 8 * 1024 * 1024; // == CHUNK_SIZE_HINT above
            const OVERLAP:    usize = 65_536;
            let region_chunks = (mbi.RegionSize + CHUNK_SIZE - 1) / CHUNK_SIZE;
            for chunk_idx in 0..region_chunks {
            if start_time.elapsed().as_secs() >= max_secs {
                resume_addr_out = mbi.BaseAddress as usize + chunk_idx * CHUNK_SIZE;
                break 'region;
            }
            let chunk_off  = chunk_idx * CHUNK_SIZE;
            let chunk_base = mbi.BaseAddress as usize + chunk_off;
            let remaining  = mbi.RegionSize - chunk_off;
            let read_size  = (CHUNK_SIZE + if chunk_idx + 1 < region_chunks { OVERLAP } else { 0 }).min(remaining);

            let mut buffer = vec![0u8; read_size];
            let mut bytes_read: usize = 0;
            let ok = ReadProcessMemory(
                process, chunk_base as *const c_void,
                buffer.as_mut_ptr() as *mut c_void, read_size, &mut bytes_read,
            );
            if ok == 0 || bytes_read <= 4 { continue; }

            let data = &buffer[..bytes_read];
            regions_scanned += 1;

            const COUNT_KEY:    &[u8] = b"\"ItemCount\":";
            const LOTUS_KEY:    &[u8] = b"\"ItemType\":\"/Lotus/";
            const LONG_KEY:     &[u8] = b"$numberLong\"";
            const CONSUMED_KEY: &[u8] = b"\"ConsumedSuits\":[";
            const XPINFO_KEY:   &[u8] = b"\"XPInfo\":[";
            const FUSION_KEY:   &[u8] = b"\"FusionPoints\":";
            const CREDITS_KEY:  &[u8] = b"\"RegularCredits\":";
            const CREATED_KEY:  &[u8] = b"\"Created\":{\"$date\":";
            let has_item_count    = data.windows(COUNT_KEY.len()).any(|w| w == COUNT_KEY);
            let has_lotus_type    = data.windows(LOTUS_KEY.len()).any(|w| w == LOTUS_KEY);
            let has_number_long   = data.windows(LONG_KEY.len()).any(|w| w == LONG_KEY);
            let has_misc_root     = data.windows(MISC_KEY.len()).any(|w| w == MISC_KEY);
            let has_consumed_key  = data.windows(CONSUMED_KEY.len()).any(|w| w == CONSUMED_KEY);
            let has_xpinfo_key    = data.windows(XPINFO_KEY.len()).any(|w| w == XPINFO_KEY);
            let has_currency      = data.windows(FUSION_KEY.len()).any(|w| w == FUSION_KEY)
                                 || data.windows(CREDITS_KEY.len()).any(|w| w == CREDITS_KEY);
            let has_created       = data.windows(CREATED_KEY.len()).any(|w| w == CREATED_KEY);
            // Detect mission-reward delta blobs early — these have a different structure from
            // the full account inventory and must not become hot_addrs (hint targets).
            const INV_CHANGES_KEY: &[u8] = b"\"InventoryChanges\":";
            let is_mission_delta  = data.windows(INV_CHANGES_KEY.len()).any(|w| w == INV_CHANGES_KEY);
            // A chunk is from the actual account inventory when it has Created (inventory root)
            // and is not a mission delta. Set the per-scan flag here; lib.rs accumulates it.
            if has_created && !is_mission_delta { found_actual_inventory_out = true; }
            if !has_item_count && !has_lotus_type && !has_number_long && !has_consumed_key && !has_xpinfo_key && !has_currency { continue; }

            if has_misc_root {
                // Only record heap addresses as hot_addrs — skip EXE/DLL image range
                // (Windows maps executables above ~0x7FF0_0000_0000; game heap is below ~4 TB).
                // This prevents a false-positive match inside the game's read-only data section
                // from displacing the real inventory heap address.
                // Also skip mission delta blobs — their InventoryChanges.MiscItems section
                // matches "MiscItems":[{ but is not the player's live inventory.
                const MAX_HEAP_ADDR: usize = 0x0004_0000_0000_0000;
                if chunk_base < MAX_HEAP_ADDR && !hot_addrs_out.contains(&chunk_base) && !is_mission_delta {
                    hot_addrs_out.push(chunk_base);
                }
                log_lines.push(format!("  [inv-root] found MiscItems array at 0x{:x}{}{}", chunk_base,
                    if chunk_base >= MAX_HEAP_ADDR { " [EXE/DLL range — skipped]" } else { "" },
                    if is_mission_delta { " [mission delta — skipped as hint]" } else { "" }));
            }
            // Scan for ConsumedSuits in any chunk that contains the key — it may live
            // in a different 8 MB chunk than MiscItems in large inventory blobs.
            if has_consumed_key && consumed_suits_out.is_empty() {
                let suits = scan_consumed_suits(data);
                if !suits.is_empty() {
                    log_lines.push(format!("  [consumed-suits] count={}", suits.len()));
                    for s in suits { if !consumed_suits_out.contains(&s) { consumed_suits_out.push(s); } }
                }
            }
            // XPInfo covers every item ever levelled, including deleted/subsumed ones.
            if has_xpinfo_key {
                let xp_map = scan_xpinfo(data);
                if !xp_map.is_empty() {
                    log_lines.push(format!("  [xpinfo] {} entries", xp_map.len()));
                    for (path, rank) in xp_map {
                        let e = mastery_data.entry(path).or_insert(0);
                        if rank > *e { *e = rank; }
                    }
                }
            }

            // ── Currency fields (Endo/Credits/Platinum) ───────────────────────
            // FusionPoints/RegularCredits are top-level fields of the account inventory JSON.
            // Only read them from chunks that look like the account root:
            //   has_created  → "Created":{"$date":...} root marker (same JSON object as FusionPoints)
            //   has_misc_root → "MiscItems":[{  present (large inventories split across chunks)
            // Excluding other chunks prevents stale heap fragments / reward-notification buffers
            // (which have "FusionPoints":N for endo earned, not total) from corrupting the value.
            // Still skip explicit mission-delta blobs and use max-wins as belt-and-suspenders.
            if !is_mission_delta && has_currency && (has_created || has_misc_root) {
                for (path, qty) in scan_currency_fields(data) {
                    log_lines.push(format!(
                        "  [currency] {}={} addr=0x{:x}{}{}",
                        path.split('/').last().unwrap_or(path), qty, chunk_base,
                        if has_misc_root { " has-MiscItems" } else { "" },
                        if has_created  { " has-Created"   } else { "" },
                    ));
                    let e = resources.entry(path.to_string()).or_insert((qty, String::new()));
                    if qty > e.0 { e.0 = qty; }
                }
            } else if !is_mission_delta && has_currency {
                // Log skipped fragments so we can audit what we're ignoring.
                log_lines.push(format!(
                    "  [currency-skip] addr=0x{:x} no-inv-root (has_created={} has_misc_root={})",
                    chunk_base, has_created, has_misc_root,
                ));
            }

            // ── Scanner 1: Resources ──────────────────────────────────────────
            if has_item_count || has_lotus_type {
                let res_pairs = scan_inventory_resources(data, &unique_path_set);
                if !res_pairs.is_empty() {
                    // Require at least 10 items from any non-root chunk. Stale heap copies,
                    // marketplace listings, and reward-screen data produce only a handful of
                    // /Lotus/ paths — the real MiscItems blob always has hundreds. Chunks that
                    // contain the MiscItems root are always accepted regardless of item count.
                    const MIN_BLOB_ITEMS: usize = 10;
                    if !has_misc_root && res_pairs.len() < MIN_BLOB_ITEMS {
                        log_lines.push(format!(
                            "  [resources-skip] count={} < {} and no MiscItems root — stale blob",
                            res_pairs.len(), MIN_BLOB_ITEMS
                        ));
                    } else {
                    let preview: String = res_pairs.iter().take(5)
                        .map(|(p, q, _)| format!("{}={}", p.split('/').last().unwrap_or("?"), q))
                        .collect::<Vec<_>>().join(", ");
                    log_lines.push(format!(
                        "  [resources] count={}  {}{}",
                        res_pairs.len(), preview,
                        if res_pairs.len() > 5 { format!(" +{} more", res_pairs.len()-5) } else { String::new() }
                    ));
                    for (path, qty, ctx) in res_pairs {
                        resources.entry(path).or_insert((qty, ctx));
                    }
                    }
                } else if res_probe_count < 5 {
                    res_probe_count += 1;
                    if let Some(p) = data.windows(COUNT_KEY.len()).position(|w| w == COUNT_KEY) {
                        let ctx_start = p.saturating_sub(80);
                        let ctx_end   = data.len().min(p + 160);
                        let snip: String = data[ctx_start..ctx_end].iter()
                            .map(|&b| if b >= 0x20 && b < 0x7f { b as char } else { '·' })
                            .collect();
                        log_lines.push(format!("  [res-probe#{}] {}", res_probe_count, snip));
                    }
                }

                if mastery_rank.is_none() {
                    mastery_rank = scan_mastery_rank(data);
                }

                // ── Scanner 1b: Mods / Arcanes ────────────────────────────────
                // Scan for mods on:
                // (a) chunks containing MiscItems (inventory root) — finds cosmetics and any mods in same blob
                // (b) chunks containing /Lotus/Upgrades/Mods/ but NOT MiscItems — these are the
                //     RawUpgrades sections in large inventories where the blob spans multiple 8 MB chunks.
                let has_gameplay_mods = data.windows(MOD_HINT_CHECK.len()).any(|w| w == MOD_HINT_CHECK);
                if has_misc_root || has_gameplay_mods {
                    let mod_pairs = scan_inventory_mods(data);
                    if !mod_pairs.is_empty() {
                        log_lines.push(format!("  [mods] count={}", mod_pairs.len()));
                        for (path, mc, ctx) in mod_pairs {
                            let entry = mods.entry(path).or_insert_with(|| (ModCount::default(), ctx.clone()));
                            if mc.total > entry.0.total { entry.0 = mc; entry.1 = ctx; }
                        }
                    }
                    // Record non-MiscItems mod chunks so the hint scan can cover them next pass.
                    if has_gameplay_mods && !has_misc_root {
                        const MAX_HEAP_ADDR: usize = 0x0004_0000_0000_0000;
                        if chunk_base < MAX_HEAP_ADDR && !mod_hot_addrs_out.contains(&chunk_base) {
                            mod_hot_addrs_out.push(chunk_base);
                            log_lines.push(format!("  [mod-root] new RawUpgrades chunk at 0x{:x}", chunk_base));
                        }
                    }
                }
            }

            // ── Scanner 3: Pending recipes ────────────────────────────────────
            if has_number_long {
                let hits = scan_pending_recipes(data);
                if !hits.is_empty() {
                    log_lines.push(format!("  [crafting] {} active recipes", hits.len()));
                }
                for h in hits { pending_recipes.push(h); }
            }

            // ── Scanner 2: Unique items ───────────────────────────────────────
            // Skip regions that are mission-reward delta blobs (is_mission_delta computed above).
            // These contain item paths from rewarded items but are NOT the player's full
            // inventory — matching against them produces false positives like unowned warframes.
            let unique_hits = if has_lotus_type && !is_mission_delta { scan_inventory_unique(data, &unique_ac, &unique_item_paths) } else { vec![] };
            if !unique_hits.is_empty() {
                // Log every hit with the last two path segments so we can distinguish
                // e.g. "Weapons/BurstonPrime" from "Weapons/BurstonPrimeMk1".
                let all_names: String = unique_hits.iter()
                    .map(|(li, _, _, _)| {
                        let p = &unique_item_paths[*li];
                        let parts: Vec<&str> = p.split('/').filter(|s| !s.is_empty()).collect();
                        let tail = if parts.len() >= 2 {
                            format!("{}/{}", parts[parts.len()-2], parts[parts.len()-1])
                        } else {
                            parts.last().copied().unwrap_or("?").to_string()
                        };
                        tail
                    })
                    .collect::<Vec<_>>().join(", ");
                log_lines.push(format!("  [unique] count={}  {}", unique_hits.len(), all_names));
                let n = unique_hits.len();
                // Summarise XP extraction for this region (helps diagnose mastery data flow)
                let xp_found: Vec<String> = unique_hits.iter().filter_map(|(li, rank, _, _)| {
                    rank.map(|r| {
                        let p = &unique_item_paths[*li];
                        let tail = p.split('/').next_back().unwrap_or("?");
                        format!("{}=R{}", tail, r)
                    })
                }).collect();
                if !xp_found.is_empty() {
                    log_lines.push(format!("  [xp] {}", xp_found.join(", ")));
                }
                for (local_idx, rank, shards, shard_diag) in &unique_hits {
                    let path = unique_item_paths[*local_idx].clone();
                    let entry = unique.entry(path.clone()).or_insert(n);
                    if n > *entry { *entry = n; }
                    if let Some(r) = rank {
                        let mr = mastery_data.entry(path.clone()).or_insert(0);
                        if r > mr { *mr = *r; }
                    }
                    let full_path = &unique_item_paths[*local_idx];
                    if !shards.is_empty() {
                        let any_unknown = shards.iter().any(|s| s.color.is_empty());
                        socketed_shards.insert(path.clone(), shards.clone());
                        log_lines.push(format!("  [shards] {} shard(s) in {} colors=[{}]{}",
                            shards.len(), full_path,
                            shards.iter().map(|s| s.color.as_str()).collect::<Vec<_>>().join(","),
                            if any_unknown && !shard_diag.is_empty() {
                                format!("\n  [shard-raw] {}", &shard_diag[..shard_diag.len().min(280)])
                            } else { String::new() }
                        ));
                    } else if full_path.contains("/Powersuits/") {
                        // Warframe was fully validated (Configs found) but has no shards.
                        // Insert an empty marker so lib.rs can prune any stale cached entry.
                        socketed_shards.entry(path).or_insert_with(Vec::new);
                    }
                }
            }
            } // end chunk loop
        }

        log_lines.push(format!(
            "  [scan-done] elapsed={}ms regions={}",
            start_time.elapsed().as_millis(), regions_scanned
        ));

        CloseHandle(process);
    }

    // ── Assemble results ──────────────────────────────────────────────────────

    let mut items_found: Vec<FoundItem> = Vec::new();

    for (path, (qty, ctx)) in &resources {
        if let Some(name) = display_map.get(path) {
            items_found.push(FoundItem {
                unique_name: path.clone(),
                name: name.clone(),
                quantity: *qty,
                explicit_count: true,
                context: ctx.clone(),
            });
        }
    }

    // mastery_data is already path-keyed — use it directly.
    let mastery_data_out = mastery_data;

    for (path, _n) in &unique {
        if resources.contains_key(path) { continue; }
        // Subsumed warframes appear in ConsumedSuits memory — skip them so they
        // are not reported as owned unique items.
        if consumed_suits_out.contains(path) { continue; }
        if let Some(name) = display_map.get(path) {
            items_found.push(FoundItem {
                unique_name: path.clone(),
                name: name.clone(),
                quantity: 1,
                explicit_count: false,
                context: String::new(),
            });
        }
    }

    items_found.sort_by(|a, b| a.name.cmp(&b.name));

    let mods_found: HashMap<String, ModCount> = mods.into_iter().map(|(k, (mc, _))| (k, mc)).collect();

    log_lines.push(format!(
        "  TOTALS: resources={} mods={} unique={} total={}",
        resources.len(), mods_found.len(), unique.len(), items_found.len()
    ));

    // Deduplicate pending recipes by unique_name (keep latest completion time)
    pending_recipes.sort_by_key(|r| r.completion_ms);
    pending_recipes.dedup_by(|a, b| {
        if a.unique_name == b.unique_name { b.completion_ms = b.completion_ms.max(a.completion_ms); true }
        else { false }
    });

    ScanResult { warframe_running: true, items_found, pending_recipes, mastery_rank, mastery_data: mastery_data_out, regions_scanned, found_actual_inventory: found_actual_inventory_out, error: None, log_lines, relic_rewards: None, resume_addr: resume_addr_out, hot_addrs: hot_addrs_out, mod_hot_addrs: mod_hot_addrs_out, consumed_suits: consumed_suits_out, mods_found, hint_mods: hint_mods_out, hint_resources: hint_resources_out, hint_flavour_items: hint_flavour_out, socketed_shards }
}

#[cfg(target_os = "windows")]
pub fn find_warframe_pid_pub() -> Option<u32> { find_warframe_pid() }

#[cfg(not(target_os = "windows"))]
pub fn find_warframe_pid_pub() -> Option<u32> { None }

// ─── Raw memory format probe ──────────────────────────────────────────────────
//
// Scans Warframe's memory and returns raw text context around every occurrence
// of a set of known strings.  Capped at max_hits total.  Used to reverse-engineer
// the actual JSON format for inventory items without any parsing assumptions.

#[cfg(target_os = "windows")]
pub fn dump_inventory_regions(max_hits: usize) -> Vec<String> {
    use std::ffi::c_void;
    use std::mem;
    use windows_sys::Win32::{
        Foundation::CloseHandle,
        System::{
            Diagnostics::Debug::ReadProcessMemory,
            Memory::{VirtualQueryEx, MEMORY_BASIC_INFORMATION, MEM_COMMIT, PAGE_GUARD, PAGE_NOACCESS},
            Threading::{OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ},
        },
    };

    // Patterns to search for — ordered by diagnostic value.
    // "MiscItems":[{ marks the beginning of the actual inventory JSON array from DE's API
    // response (the most useful single needle for finding the real JSON blob).
    const NEEDLES: &[&[u8]] = &[
        b"\"MiscItems\":[{",      // inventory JSON array start — best diagnostic
        b"\"ItemCount\":",
        b"MiscItems",
        b"AlloyPlate",
        b"Circuits\"",
        b"/Lotus/Types/Items/MiscItems/",
    ];

    let pid = match find_warframe_pid() {
        Some(p) => p,
        None => return vec!["Warframe not running".to_string()],
    };

    let process = unsafe { OpenProcess(PROCESS_VM_READ | PROCESS_QUERY_INFORMATION, 0, pid) };
    if process == 0 { return vec!["OpenProcess failed".to_string()]; }

    let mut results: Vec<String> = Vec::new();
    let mut addr: usize = 0x10000;
    let mbi_size = mem::size_of::<MEMORY_BASIC_INFORMATION>();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);

    'outer: while std::time::Instant::now() < deadline && results.len() < max_hits {
        let mut mbi: MEMORY_BASIC_INFORMATION = unsafe { mem::zeroed() };
        if unsafe { VirtualQueryEx(process, addr as *const c_void, &mut mbi, mbi_size) } == 0 { break; }
        let region_end = (mbi.BaseAddress as usize).saturating_add(mbi.RegionSize);
        if region_end <= addr { break; }
        addr = region_end;

        if mbi.State != MEM_COMMIT { continue; }
        let p = mbi.Protect;
        if p & PAGE_NOACCESS != 0 || p & PAGE_GUARD != 0 { continue; }
        if p == 0x10 || p == 0x20 { continue; }    // skip executable (code) pages
        // Skip tiny or enormous regions; read large regions in 64 MB chunks
        const MAX_REGION: usize = 256 * 1024 * 1024;
        const CHUNK_SIZE: usize =  64 * 1024 * 1024;
        if mbi.RegionSize < 4096 || mbi.RegionSize > MAX_REGION { continue; }

        let chunks = if mbi.RegionSize > CHUNK_SIZE {
            (mbi.RegionSize + CHUNK_SIZE - 1) / CHUNK_SIZE
        } else { 1 };

        'chunk: for chunk_idx in 0..chunks {
            if results.len() >= max_hits { break 'outer; }
            if std::time::Instant::now() >= deadline { break 'outer; }

            let chunk_offset = chunk_idx * CHUNK_SIZE;
            let read_size    = CHUNK_SIZE.min(mbi.RegionSize - chunk_offset);
            let chunk_addr   = mbi.BaseAddress as usize + chunk_offset;

            let mut buf = vec![0u8; read_size];
            let mut bytes_read = 0usize;
            let ok = unsafe {
                ReadProcessMemory(process, chunk_addr as *const c_void,
                    buf.as_mut_ptr() as *mut c_void, read_size, &mut bytes_read)
            };
            if ok == 0 || bytes_read < 8 { continue 'chunk; }
            let data = &buf[..bytes_read];

        for needle in NEEDLES {
            if results.len() >= max_hits { break 'outer; }
            if let Some(pos) = data.windows(needle.len()).position(|w| w == *needle) {
                let ctx_start = pos.saturating_sub(80);
                let ctx_end   = data.len().min(pos + 200);
                let snip: String = data[ctx_start..ctx_end].iter()
                    .map(|&b| if b >= 0x20 && b < 0x7f { b as char } else { '·' })
                    .collect();
                results.push(format!(
                    "0x{:012x}  needle=\"{}\"  ctx: {}",
                    chunk_addr + ctx_start,
                    String::from_utf8_lossy(needle),
                    snip
                ));
                // Also grab up to 2 more occurrences of the same needle in this chunk
                let mut search = pos + needle.len();
                let mut extra = 0;
                while extra < 2 && search + needle.len() <= data.len() {
                    if let Some(rel) = data[search..].windows(needle.len()).position(|w| w == *needle) {
                        let p2 = search + rel;
                        let s2 = p2.saturating_sub(80);
                        let e2 = data.len().min(p2 + 200);
                        let snip2: String = data[s2..e2].iter()
                            .map(|&b| if b >= 0x20 && b < 0x7f { b as char } else { '·' })
                            .collect();
                        results.push(format!(
                            "0x{:012x}  needle=\"{}\"  ctx: {}",
                            chunk_addr + s2,
                            String::from_utf8_lossy(needle),
                            snip2
                        ));
                        search = p2 + needle.len();
                        extra += 1;
                    } else { break; }
                }
            }
        }
        } // end 'chunk loop
    }

    unsafe { CloseHandle(process); }
    if results.is_empty() { results.push("No matches found".to_string()); }
    results
}

#[cfg(not(target_os = "windows"))]
pub fn dump_inventory_regions(_max_hits: usize) -> Vec<String> {
    vec!["Only supported on Windows".to_string()]
}

// ─── One-shot inventory blob capture ─────────────────────────────────────────
//
// Scans all committed readable regions for the first chunk that contains the
// inventory root marker ("MiscItems":[).  Saves the full printable-text portion
// of that region to `output_path` so it can be inspected offline.
//
// Non-printable bytes are replaced with '.' so the file is text-editor friendly.
// Saves up to 8 MB centred on the MiscItems key (4 MB before, 4 MB after).

#[cfg(target_os = "windows")]
pub fn capture_inventory_blob(output_path: &std::path::Path) -> Result<String, String> {
    use std::ffi::c_void;
    use std::mem;
    use windows_sys::Win32::{
        Foundation::{CloseHandle, FALSE},
        System::{
            Diagnostics::Debug::ReadProcessMemory,
            Memory::{VirtualQueryEx, MEMORY_BASIC_INFORMATION, MEM_COMMIT, PAGE_GUARD, PAGE_NOACCESS},
            Threading::{OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ},
        },
    };

    let pid = find_warframe_pid_pub().ok_or_else(|| "Warframe is not running".to_string())?;

    let process = unsafe { OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, FALSE, pid) };
    if process == 0 { return Err("Could not open Warframe process".to_string()); }

    const MISC_KEY: &[u8]      = b"\"MiscItems\":[";
    const MIN_BLOB_BYTES: usize = 200_000;    // skip tiny chunks — real inventory is MB-scale
    const MAX_REGION_READ: usize = 128 * 1024 * 1024;
    const HALF_SAVE: usize      = 4 * 1024 * 1024;   // 4 MB either side of MiscItems

    let mut addr: usize = 0;
    let mut saved: Option<(usize, String)> = None; // (region size, message)

    'outer: loop {
        let mut mbi = unsafe { mem::zeroed::<MEMORY_BASIC_INFORMATION>() };
        if unsafe { VirtualQueryEx(process, addr as *const c_void, &mut mbi, mem::size_of::<MEMORY_BASIC_INFORMATION>()) } == 0 { break; }

        let region_addr = mbi.BaseAddress as usize;
        let region_size = mbi.RegionSize;
        let next_addr   = region_addr.saturating_add(region_size);

        if mbi.State == MEM_COMMIT
            && mbi.Protect & PAGE_GUARD    == 0
            && mbi.Protect & PAGE_NOACCESS == 0
            && region_size >= MIN_BLOB_BYTES
            && region_size <= MAX_REGION_READ
        {
            let mut data = vec![0u8; region_size];
            let mut n = 0usize;
            if unsafe { ReadProcessMemory(process, region_addr as *const c_void, data.as_mut_ptr() as *mut c_void, region_size, &mut n) } != 0 && n >= MIN_BLOB_BYTES {
                let data = &data[..n];
                if let Some(misc_pos) = data.windows(MISC_KEY.len()).position(|w| w == MISC_KEY) {
                    let start = misc_pos.saturating_sub(HALF_SAVE);
                    let end   = (misc_pos + HALF_SAVE).min(data.len());
                    let text: Vec<u8> = data[start..end].iter()
                        .map(|&b| if b >= 0x20 && b <= 0x7e || b == b'\n' || b == b'\t' { b } else { b'.' })
                        .collect();
                    if let Err(e) = std::fs::write(output_path, &text) {
                        unsafe { CloseHandle(process); }
                        return Err(format!("Write failed: {e}"));
                    }
                    saved = Some((text.len(), format!(
                        "Saved {}KB blob (region 0x{:x}, size {}KB, MiscItems at +{}KB) to {}",
                        text.len() / 1024, region_addr, n / 1024, misc_pos / 1024,
                        output_path.display()
                    )));
                    break 'outer;
                }
            }
        }

        if next_addr <= addr { break; }
        addr = next_addr;
    }

    unsafe { CloseHandle(process); }

    saved.map(|(_, msg)| msg)
         .ok_or_else(|| "No inventory blob found — make sure Warframe is running and inventory is loaded (open Arsenal or Inventory screen)".to_string())
}

#[cfg(not(target_os = "windows"))]
pub fn capture_inventory_blob(_output_path: &std::path::Path) -> Result<String, String> {
    Err("Only supported on Windows".into())
}

/// Scan all Warframe process memory and save every relevant blob found into `blob_dir`.
/// "Relevant" = region ≥ 100 KB that contains at least one of: MiscItems, Suits,
/// LongGuns, Melee, Pistols, InventoryChanges (covers real inventory and mission blobs).
///
/// Naming convention:
///   Actual_inventory_<ts>.txt — blob contains "HasResetAccount" (the full account root)
///   blob_N_<ts>.txt            — all other relevant blobs (mission deltas, equipment chunks)
///
/// Returns the number of blobs saved.
#[cfg(target_os = "windows")]
pub fn capture_all_blobs(blob_dir: &std::path::Path, ts: &str) -> usize {
    use std::ffi::c_void;
    use std::mem;
    use windows_sys::Win32::{
        Foundation::{CloseHandle, FALSE},
        System::{
            Diagnostics::Debug::ReadProcessMemory,
            Memory::{VirtualQueryEx, MEMORY_BASIC_INFORMATION, MEM_COMMIT, PAGE_GUARD, PAGE_NOACCESS},
            Threading::{OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ},
        },
    };

    let pid = match find_warframe_pid_pub() { Some(p) => p, None => return 0 };
    let process = unsafe { OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, FALSE, pid) };
    if process == 0 { return 0; }

    const MIN_BLOB_BYTES: usize = 100_000;
    const MAX_REGION_READ: usize = 64 * 1024 * 1024;
    const ANCHORS: &[&[u8]] = &[
        b"\"MiscItems\":[",
        b"\"Suits\":[",
        b"\"LongGuns\":[",
        b"\"Melee\":[",
        b"\"Pistols\":[",
        b"\"InventoryChanges\":",
    ];
    // Two independent markers that identify the actual account inventory:
    // - "HasResetAccount" lives at the very END of the blob (region 6 in the layout)
    // - "Created":{"$date": lives at the very START of the blob (region 0)
    // They are always in DIFFERENT Windows memory regions, so we accept either one.
    // In practice region 0 (has Created + "Suits":[) triggers with REAL_MARKER_2,
    // and region 6 would trigger with REAL_MARKER_1 if it had an anchor (it doesn't).
    const REAL_MARKER_1:  &[u8] = b"\"HasResetAccount\"";
    const REAL_MARKER_2:  &[u8] = b"\"Created\":{\"$date\":";
    const LOTUS_KEY:      &[u8] = b"/Lotus/";
    const MAX_BLOBS: usize = 25;

    let mut addr: usize = 0;
    let mut saved = 0usize;

    loop {
        if saved >= MAX_BLOBS { break; }
        let mut mbi = unsafe { mem::zeroed::<MEMORY_BASIC_INFORMATION>() };
        if unsafe { VirtualQueryEx(process, addr as *const c_void, &mut mbi, mem::size_of::<MEMORY_BASIC_INFORMATION>()) } == 0 { break; }
        let region_addr = mbi.BaseAddress as usize;
        let region_size = mbi.RegionSize;
        let next_addr   = region_addr.saturating_add(region_size);
        if next_addr <= addr { break; }
        addr = next_addr;

        if mbi.State != MEM_COMMIT
            || mbi.Protect & PAGE_GUARD    != 0
            || mbi.Protect & PAGE_NOACCESS != 0
            || region_size < MIN_BLOB_BYTES
            || region_size > MAX_REGION_READ
        { continue; }

        let mut data = vec![0u8; region_size];
        let mut n = 0usize;
        if unsafe { ReadProcessMemory(process, region_addr as *const c_void, data.as_mut_ptr() as *mut c_void, region_size, &mut n) } == 0 || n < MIN_BLOB_BYTES { continue; }
        let data = &data[..n];

        // Find the FIRST anchor hit — save a focused window around it, not the full region.
        // This keeps files at a readable size (≤4 MB) and centred on the interesting data.
        let anchor_pos = ANCHORS.iter()
            .filter_map(|a| data.windows(a.len()).position(|w| w == *a))
            .min();
        let anchor_pos = match anchor_pos { Some(p) => p, None => continue };
        if !data.windows(LOTUS_KEY.len()).any(|w| w == LOTUS_KEY) { continue; }

        // Identify what kind of blob this is for the filename.
        // The full inventory spans multiple regions: region 0 has Created (start),
        // region 5 has MiscItems (middle), region 6 has HasResetAccount (end).
        // They are never in the same region data, so we accept either marker.
        let has_created      = data.windows(REAL_MARKER_2.len()).any(|w| w == REAL_MARKER_2);
        let has_reset_acct   = data.windows(REAL_MARKER_1.len()).any(|w| w == REAL_MARKER_1);
        let is_real          = has_created || has_reset_acct;
        let is_mission       = data.windows(b"\"InventoryChanges\":".len()).any(|w| w == b"\"InventoryChanges\":");
        let has_misc         = data.windows(b"\"MiscItems\":[".len()).any(|w| w == b"\"MiscItems\":[");
        let has_suits        = data.windows(b"\"Suits\":[".len()).any(|w| w == b"\"Suits\":[");

        // Non-real blobs: save ±2 MB around the anchor, capped to the region.
        // Real inventory root: only save when triggered by Created (region 0, the START).
        // HasResetAccount (region 6, the END) is skipped — the Created-triggered read
        // already captured the full 570 KB blob with a 1.5 MB ReadProcessMemory.
        // ReadProcessMemory crosses VirtualAlloc boundaries, so one call spans all regions.
        // Trim to "DeathSquadable":false} — the last field of the root JSON object.
        const FULL_INV_READ: usize = 1_500_000;
        const END_MARKER:    &[u8] = b"\"DeathSquadable\":false}";

        let save_data: Vec<u8>;
        let kind: &str;

        if has_created && !is_mission {
            // Triggered by Created (region 0, start of inventory).
            // One large read forward captures all 7 inventory regions (~570 KB).
            let mut buf = vec![0u8; FULL_INV_READ];
            let mut full_n = 0usize;
            let ok = unsafe { ReadProcessMemory(
                process, region_addr as *const c_void,
                buf.as_mut_ptr() as *mut c_void, FULL_INV_READ, &mut full_n,
            ) };
            let raw = if ok != 0 && full_n > 0 { &buf[..full_n] } else { data };
            // Trim at the closing field — first occurrence is the root object close.
            let end = raw.windows(END_MARKER.len())
                .position(|w| w == END_MARKER)
                .map(|p| p + END_MARKER.len())
                .unwrap_or(raw.len());
            save_data = raw[..end].to_vec();
            kind = "FULL_ACCOUNT";
        } else if has_reset_acct && !has_created && !is_mission {
            // HasResetAccount is the end of the same blob already captured from Created.
            // Skip to avoid a duplicate truncated file (addr is already at next_addr).
            continue;
        } else {
            // Non-inventory blob: save ±2 MB window around the anchor.
            const HALF_SAVE: usize = 2 * 1024 * 1024;
            let slice_start = anchor_pos.saturating_sub(HALF_SAVE);
            let slice_end   = (anchor_pos + HALF_SAVE).min(data.len());
            save_data = data[slice_start..slice_end].to_vec();
            kind = if is_mission { "MISSION_DELTA" } else if has_misc { "MISC_ITEMS" }
                   else if has_suits { "SUITS" } else { "OTHER" };
        }

        let prefix = if is_real && !is_mission { "Actual_inventory" } else { "blob" };
        let name = format!("{}_{}_{}.txt", prefix, kind, ts);
        // If a file with this name already exists (two regions of same type), append index.
        let path = {
            let candidate = blob_dir.join(&name);
            if candidate.exists() {
                blob_dir.join(format!("{}_{}_{:02}.txt", prefix, kind, saved + 1))
            } else {
                candidate
            }
        };

        // Printable-ASCII filter — keeps the file human-readable in any text editor.
        let text: Vec<u8> = save_data.iter()
            .map(|&b| if b >= 0x20 && b <= 0x7e || b == b'\n' || b == b'\t' { b } else { b'.' })
            .collect();
        if std::fs::write(&path, &text).is_ok() {
            saved += 1;
        }
    }

    unsafe { CloseHandle(process); }
    saved
}

#[cfg(not(target_os = "windows"))]
pub fn capture_all_blobs(_blob_dir: &std::path::Path, _ts: &str) -> usize { 0 }

// ─── Continuous raw memory string dump ───────────────────────────────────────
//
// Scans every committed readable region in the Warframe process and extracts
// every run of 12+ consecutive printable ASCII bytes.  Each string is written
// to `out_file` as: `0xADDR  <string>\n`.  No needle filtering — everything.
//
// Designed to be called repeatedly from a loop: one call = one full pass.
// Returns the number of strings written this pass, or an error string.
//
// Large regions (>64 MB) are read in 64 MB chunks so the heap stays bounded.
// The caller is responsible for not holding the file lock across sleeps.

#[cfg(target_os = "windows")]
pub fn raw_scan_pass(out: &mut impl std::io::Write) -> Result<usize, String> {
    use std::ffi::c_void;
    use std::mem;
    use windows_sys::Win32::{
        Foundation::CloseHandle,
        System::{
            Diagnostics::Debug::ReadProcessMemory,
            Memory::{VirtualQueryEx, MEMORY_BASIC_INFORMATION, MEM_COMMIT, PAGE_GUARD, PAGE_NOACCESS},
            Threading::{OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ},
        },
    };

    const MIN_LEN:  usize = 8;
    const CHUNK:    usize = 64 * 1024 * 1024;
    const TIMEOUT:  u64   = 600; // 10 minutes — full coverage over full scan

    let pid = find_warframe_pid().ok_or("Warframe not running")?;
    let process = unsafe { OpenProcess(PROCESS_VM_READ | PROCESS_QUERY_INFORMATION, 0, pid) };
    if process == 0 { return Err("OpenProcess failed".into()); }

    let mut addr: usize = 0x10000;
    let mbi_size = mem::size_of::<MEMORY_BASIC_INFORMATION>();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(TIMEOUT);
    let mut count = 0usize;

    while std::time::Instant::now() < deadline {
        let mut mbi: MEMORY_BASIC_INFORMATION = unsafe { mem::zeroed() };
        if unsafe { VirtualQueryEx(process, addr as *const c_void, &mut mbi, mbi_size) } == 0 { break; }
        let region_end = (mbi.BaseAddress as usize).saturating_add(mbi.RegionSize);
        if region_end <= addr { break; }
        addr = region_end;

        if mbi.State != MEM_COMMIT { continue; }
        let p = mbi.Protect;
        if p & PAGE_NOACCESS != 0 || p & PAGE_GUARD != 0 { continue; }
        // Only skip pure-execute (no read bit) — PAGE_EXECUTE_READ (0x20) is kept
        // because game DLL const-string sections use that protection.
        if p == 0x10 { continue; }

        let chunks = (mbi.RegionSize + CHUNK - 1) / CHUNK;
        for ci in 0..chunks {
            if std::time::Instant::now() >= deadline { break; }
            let off        = ci * CHUNK;
            let read_size  = CHUNK.min(mbi.RegionSize - off);
            let chunk_base = mbi.BaseAddress as usize + off;

            let mut buf = vec![0u8; read_size];
            let mut bytes_read = 0usize;
            let ok = unsafe {
                ReadProcessMemory(process, chunk_base as *const c_void,
                    buf.as_mut_ptr() as *mut c_void, read_size, &mut bytes_read)
            };
            if ok == 0 || bytes_read < MIN_LEN { continue; }

            // Extract printable ASCII runs of MIN_LEN+
            let data = &buf[..bytes_read];
            let mut run_start: Option<usize> = None;
            for (i, &b) in data.iter().enumerate() {
                let printable = b >= 0x20 && b < 0x7f;
                if printable {
                    if run_start.is_none() { run_start = Some(i); }
                } else {
                    if let Some(s) = run_start.take() {
                        let len = i - s;
                        if len >= MIN_LEN {
                            let s_str = std::str::from_utf8(&data[s..i]).unwrap_or("?");
                            let _ = writeln!(out, "0x{:012x}  {}", chunk_base + s, s_str);
                            count += 1;
                        }
                    }
                }
            }
            // flush any run that reaches end of chunk
            if let Some(s) = run_start {
                let len = bytes_read - s;
                if len >= MIN_LEN {
                    let s_str = std::str::from_utf8(&data[s..bytes_read]).unwrap_or("?");
                    let _ = writeln!(out, "0x{:012x}  {}", chunk_base + s, s_str);
                    count += 1;
                }
            }
        }
    }

    unsafe { CloseHandle(process); }
    Ok(count)
}

#[cfg(not(target_os = "windows"))]
pub fn raw_scan_pass(_out: &mut impl std::io::Write) -> Result<usize, String> {
    Err("Only supported on Windows".into())
}

// ─── Riven validity flag scanner ──────────────────────────────────────────────
//
// GEP (gep_warframeext.dll) uses Pattern D-2 to locate a single byte in
// Warframe's .text section that acts as an open/closed flag for the riven
// reroll UI. The byte is non-zero while the screen is shown, zero when closed.
//
// Pattern D-2 (13 bytes):
//   80 3d ?? ?? ?? ?? 00  48 8b ?? ??  0f 85
//   CMP byte ptr [RIP+disp32], 0   MOV ...   JNZ ...
//
// Resolving the flag VA:
//   The CMP instruction is 7 bytes. RIP at execution = match_va + 7.
//   flag_va = (match_va + 7) + i32::from_le_bytes(bytes[2..6])

#[cfg(target_os = "windows")]
fn find_pattern_d2(data: &[u8], base_va: usize) -> Option<usize> {
    let len = data.len();
    if len < 13 { return None; }
    for i in 0..len - 13 {
        if data[i]    != 0x80 || data[i+1]  != 0x3d { continue; }
        if data[i+6]  != 0x00 { continue; }
        if data[i+7]  != 0x48 || data[i+8]  != 0x8b { continue; }
        if data[i+11] != 0x0f || data[i+12] != 0x85 { continue; }
        let disp = i32::from_le_bytes([data[i+2], data[i+3], data[i+4], data[i+5]]);
        let flag_va = (base_va + i + 7) as i64 + disp as i64;
        if flag_va > 0x10000 && flag_va < 0x7fff_ffff_ffff {
            return Some(flag_va as usize);
        }
    }
    None
}

/// Scan Warframe's executable image sections for the riven screen validity flag VA.
/// Returns the virtual address of the single byte: non-zero = screen open, 0 = closed.
/// Scans once; caller should cache the result and re-scan only on PID change.
#[cfg(target_os = "windows")]
pub fn find_riven_validity_va(pid: u32) -> Option<usize> {
    use std::ffi::c_void;
    use std::mem;
    use windows_sys::Win32::{
        Foundation::CloseHandle,
        System::{
            Diagnostics::Debug::ReadProcessMemory,
            Memory::{VirtualQueryEx, MEMORY_BASIC_INFORMATION, MEM_COMMIT},
            Threading::{OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ},
        },
    };

    let process = unsafe { OpenProcess(PROCESS_VM_READ | PROCESS_QUERY_INFORMATION, 0, pid) };
    if process == 0 { return None; }

    let mut result: Option<usize> = None;
    let mut addr: usize = 0x10000;
    let mbi_size = mem::size_of::<MEMORY_BASIC_INFORMATION>();
    let start_time = std::time::Instant::now();

    while start_time.elapsed().as_secs() < 60 && result.is_none() {
        let mut mbi: MEMORY_BASIC_INFORMATION = unsafe { mem::zeroed() };
        if unsafe { VirtualQueryEx(process, addr as *const c_void, &mut mbi, mbi_size) } == 0 { break; }
        let region_end = (mbi.BaseAddress as usize).saturating_add(mbi.RegionSize);
        if region_end <= addr { break; }
        addr = region_end;

        // Only scan committed, executable, memory-mapped PE image regions (MEM_IMAGE = 0x1000000).
        // 0x20 = PAGE_EXECUTE_READ (normal .text), 0x40 = PAGE_EXECUTE_READWRITE (patched pages).
        let is_exec_image = mbi.State == MEM_COMMIT
            && matches!(mbi.Protect, 0x20 | 0x40)
            && mbi.Type == 0x1000000
            && mbi.RegionSize >= 13
            && mbi.RegionSize <= 64 * 1024 * 1024;

        if !is_exec_image { continue; }

        let mut buf = vec![0u8; mbi.RegionSize];
        let mut bytes_read = 0usize;
        let ok = unsafe {
            ReadProcessMemory(
                process, mbi.BaseAddress as *const c_void,
                buf.as_mut_ptr() as *mut c_void, mbi.RegionSize, &mut bytes_read,
            )
        };
        if ok == 0 || bytes_read < 13 { continue; }

        result = find_pattern_d2(&buf[..bytes_read], mbi.BaseAddress as usize);
    }

    unsafe { CloseHandle(process); }
    result
}

#[cfg(not(target_os = "windows"))]
pub fn find_riven_validity_va(_pid: u32) -> Option<usize> { None }

#[cfg(target_os = "windows")]
fn find_warframe_pid() -> Option<u32> {
    use std::mem;
    use windows_sys::Win32::{
        Foundation::{CloseHandle, INVALID_HANDLE_VALUE},
        System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Process32First, Process32Next,
            PROCESSENTRY32, TH32CS_SNAPPROCESS,
        },
    };
    // CreateToolhelp32Snapshot gives process names without needing OpenProcess,
    // so EAC blocking read access on the game process doesn't prevent detection.
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE { return None; }

        let mut entry: PROCESSENTRY32 = mem::zeroed();
        entry.dwSize = mem::size_of::<PROCESSENTRY32>() as u32;

        let mut found = None;
        if Process32First(snapshot, &mut entry) != 0 {
            loop {
                let name_len = entry.szExeFile.iter().position(|&b| b == 0).unwrap_or(260);
                let name = String::from_utf8_lossy(&entry.szExeFile[..name_len]).to_lowercase();
                if name.starts_with("warframe") && !name.contains("launcher") && !name.contains("companion") {
                    found = Some(entry.th32ProcessID);
                    break;
                }
                if Process32Next(snapshot, &mut entry) == 0 { break; }
            }
        }
        CloseHandle(snapshot);
        found
    }
}

#[cfg(not(target_os = "windows"))]
pub fn scan_warframe_memory(_unique_names: &[String], _display_names: &[String], _assembled_names: &[String], _start_addr: usize, _max_secs: u64, _hint_addrs: &[usize], _mod_hint_addrs: &[usize]) -> ScanResult {
    ScanResult {
        warframe_running: false, items_found: vec![], pending_recipes: vec![], mastery_rank: None, mastery_data: HashMap::new(), regions_scanned: 0,
        error: Some("Memory scanning is only supported on Windows.".to_string()),
        log_lines: vec![], relic_rewards: None, found_actual_inventory: false,
        resume_addr: 0, hot_addrs: vec![], mod_hot_addrs: vec![], consumed_suits: vec![], mods_found: HashMap::new(), hint_mods: HashMap::new(), hint_resources: HashMap::new(), hint_flavour_items: vec![], socketed_shards: HashMap::new(),
    }
}
