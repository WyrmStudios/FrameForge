use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::{debug, info, warn};

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct ModCount {
    /// Total copies owned (all ranks combined)
    pub total: i64,
    /// rank (0 = unranked) → count at that rank
    pub by_rank: HashMap<u8, i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BlobRivenStat {
    pub tag:   String,
    pub value: i64,
}

/// Which stage of unlocking a riven is at. Matches warframe.market terminology.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum RivenState {
    /// From RawUpgrades — only the weapon type (Rifle/Pistol/Melee…) is visible.
    Unrevealed,
    /// From Upgrades with a `challenge` fingerprint but no `compat` — challenge is visible
    /// but not yet completed; weapon has not been assigned.
    Revealed,
    /// From Upgrades with a `compat` — weapon assigned, stats fully visible.
    #[default]
    Unlocked,
}

/// One owned riven mod (unrevealed, revealed, or fully unlocked).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlobRivenEntry {
    /// MongoDB ObjectId hex string (empty for unrevealed stacks).
    pub item_id:  String,
    /// Lotus path, e.g. /Lotus/Upgrades/Mods/Randomized/LotusMeleeRandomModRare
    pub item_type: String,
    /// Which stage this riven is at (unrevealed / revealed / unlocked).
    /// Old cache entries without this field default to Unlocked.
    #[serde(default)]
    pub riven_state: RivenState,
    /// Weapon unique_name from `compat` field. Only present for Unlocked rivens.
    pub compat:   Option<String>,
    /// Challenge path from fingerprint. Only present for Revealed rivens.
    /// e.g. "/Lotus/Types/Challenges/HighExterminationUndetected"
    #[serde(default)]
    pub challenge_type: Option<String>,
    /// Complication path. e.g. "/Lotus/Types/Challenges/Complications/SoloPlayer"
    #[serde(default)]
    pub challenge_complication: Option<String>,
    /// MR required to equip.
    pub lvl_req:  Option<u32>,
    /// Polarity slot (AP_ATTACK, AP_DEFENSE, etc.).
    pub polarity: Option<String>,
    pub buffs:    Vec<BlobRivenStat>,
    pub curses:   Vec<BlobRivenStat>,
    /// Current mod level (rank).
    pub mod_rank: u8,
    /// >1 for stacked unrevealed rivens of the same type.
    pub count:    u32,
    /// Number of times this riven has been re-rolled (Kuva spent). 0 = never rolled.
    #[serde(default)]
    pub rerolls:  u32,
    /// Generated riven mod name (e.g. "cronitron"). Computed from buffs at parse time.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub mod_name: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct PendingRecipe {
    pub unique_name: String,
    /// Unix timestamp in milliseconds when the craft completes
    pub completion_ms: i64,
}

/// One Archon Shard socketed into a Warframe.
/// `upgrade_type` is the effect path (e.g. `.../ArchonCrystalUpgradeWarframeEnergyMax`).
/// `color` is the raw string value from the JSON (e.g. `"ACC_CRIMSON"`, `"ACC_AZURE_TAUFORGED"`).
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ArchonShard {
    pub upgrade_type: String,
    pub color: String,
}

// ─── Blob inventory types ─────────────────────────────────────────────────────

/// Parsed representation of an Actual_inventory_FULL_ACCOUNT blob.
/// Single authoritative source for all inventory data.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BlobInventory {
    pub credits:         i64,
    pub endo:            i64,
    pub platinum:        i64,
    pub free_platinum:   i64,
    pub mastery_level:   u32,
    pub unique_items:    Vec<BlobUniqueEntry>,
    pub stackable_items: Vec<BlobStackableEntry>,
    /// Aggregated from RawUpgrades (unranked) + Upgrades (ranked).
    pub mods:            HashMap<String, ModCount>,
    /// FlavourItems — glyphs, palettes, emotes, titles, ship skins. Path → occurrence count.
    pub flavour_items:   HashMap<String, i64>,
    /// WeaponSkins — sigils and cosmetic weapon overlays. Path → occurrence count.
    pub weapon_skins:    HashMap<String, i64>,
    /// Path → mastery rank derived from XPInfo.
    pub mastery_data:    HashMap<String, u32>,
    pub pending_recipes: Vec<BlobPendingRecipe>,
    /// Warframe paths fed to Helminth (InfestedFoundry.ConsumedSuits).
    pub consumed_suits:  Vec<String>,
    /// All owned riven mods (veiled and revealed).
    pub rivens:          Vec<BlobRivenEntry>,
    /// Top-level JSON keys present in the blob — used to detect front-truncated captures.
    #[serde(default)]
    pub sections:        Vec<String>,
    /// Hash of the parsed inventory content (0 = unknown). Lets the monitor skip a
    /// blob identical to the one it already applied.
    #[serde(default)]
    pub content_hash:    u64,
}

impl BlobInventory {
    /// Deterministic hash of everything except `content_hash` itself. Goes through
    /// `serde_json::Value` (sorted maps) so HashMap iteration order can't change it.
    /// Returns 0 if serialization fails, which callers treat as "never equal".
    pub fn compute_content_hash(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut copy = self.clone();
        copy.content_hash = 0;
        let bytes = match serde_json::to_value(&copy).and_then(|v| serde_json::to_vec(&v)) {
            Ok(b) => b,
            Err(_) => return 0,
        };
        let mut h = std::collections::hash_map::DefaultHasher::new();
        bytes.hash(&mut h);
        h.finish().max(1)
    }
}

/// One owned unique item (warframe, weapon, companion, archwing, amp, mech).
/// Multiple entries with the same item_type = multiple owned copies.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlobUniqueEntry {
    pub item_type:     String,
    pub section:       String,
    pub polarized:     u32,
    /// Raw XP from the blob — used to compute rank via `xp_to_rank`.
    /// For gilded modular items (Amps, Kitguns, Zaws) XP resets to 0 on gilding,
    /// so this reflects post-gild progress.
    pub xp:            i64,
    /// Player-assigned name (set when an item is gilded in the Foundry).
    pub item_name:     Option<String>,
    pub pet_name:      Option<String>,
    pub focus_lens:    Option<String>,
    pub archon_shards: Vec<ArchonShard>,
    /// Component paths for modular items (Amps, Kitguns, Zaws).
    /// Populated from the blob's `ModularParts` array.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub modular_parts: Vec<String>,
}

/// A stackable item: resource, blueprint, relic, Ayatan sculpture, etc.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlobStackableEntry {
    pub item_type:  String,
    pub item_count: i64,
    /// Ayatan sockets (FusionTreasures only).
    pub sockets:    Option<i64>,
}

/// Active Foundry crafting job parsed from PendingRecipes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlobPendingRecipe {
    pub item_type:     String,
    pub completion_ms: i64,
}

fn digits_end(data: &[u8], start: usize) -> usize {
    let mut i = start;
    while i < data.len() && data[i].is_ascii_digit() { i += 1; }
    i
}

/// Convert raw affinity XP to item rank.
/// Formula from Warframe wiki: cumulative XP to reach rank N is 1000×N² for
/// Warframes/Sentinels/companions, 500×N² for all weapon types.
/// Invert: rank = floor(sqrt(xp / base)).
/// No upper cap — some weapons (e.g. Paracesis) can exceed rank 30.
///
/// MOA (`MoaPetComponents/...Weapon`) and Zanuka Hound
/// (`ZanukaPets/ZanukaPetMeleeWeapon<EL>`) weapons live under
/// `/Types/Friendly/Pets/...` — the companion-body prefix — even though
/// they're standalone weapons on the weapon curve. MOA weapon paths end in
/// "Weapon" (e.g. CryoxionWeapon); Zanuka weapon paths instead end in a
/// 2-letter element suffix (PS/IP/IS) after "ZanukaPetMeleeWeapon", so they
/// need a separate substring check. Catch both before the broader
/// Friendly-path match or they'd be misclassified as companions and
/// under-ranked.
pub fn xp_to_rank(xp: i64, path: &str) -> u32 {
    let is_companion_weapon = (path.contains("/MoaPetComponents/") && path.ends_with("Weapon"))
        || path.contains("ZanukaPetMeleeWeapon");
    let base = if is_companion_weapon {
        500.0f64
    } else if path.contains("/Powersuits/")
        || path.contains("/SentinelPowersuits/")
        || path.contains("/Types/Friendly/")
        || path.contains("/Types/Game/KubrowPet/")
        || path.contains("/Types/Game/CatbrowPet/")
    { 1000.0f64 } else { 500.0f64 };
    (xp as f64 / base).sqrt().floor() as u32
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
    for pos in memchr::memmem::find_iter(data, id_key) {
        let id_start = pos + id_key.len();
        let id_slice = &data[id_start..id_start.saturating_add(26).min(data.len())];
        let close = id_slice.iter().position(|&b| b == b'"').unwrap_or(0);
        if close != 24 { continue; }
        let id_bytes = &id_slice[..24];
        if !id_bytes.iter().all(|&b| b.is_ascii_hexdigit()) { continue; }
        let account_id = std::str::from_utf8(id_bytes).unwrap_or("").to_string();

        let nonce_search_end = (id_start + 2048).min(data.len());
        if let Some(rel) = memchr::memmem::find(&data[id_start..nonce_search_end], nonce_key) {
            let ns = id_start + rel + nonce_key.len();
            let ne = digits_end(data, ns);
            if ne > ns && ne - ns >= 5 {
                if let Ok(nonce) = std::str::from_utf8(&data[ns..ne]) {
                    return Some((account_id, nonce.to_string()));
                }
            }
        }
    }

    // URL-encoded: accountId=<24hexchars>&nonce=<10digits>&ct=STM
    let ak = b"accountId=";
    let nk = b"nonce=";
    for pos in memchr::memmem::find_iter(data, ak) {
        let id_start = pos + ak.len();
        let id_end = data[id_start..].iter().position(|&b| !b.is_ascii_hexdigit()).map(|p| id_start + p).unwrap_or(data.len());
        if id_end - id_start != 24 { continue; }
        let account_id = std::str::from_utf8(&data[id_start..id_end]).unwrap_or("").to_string();
        let nonce_search_end = (id_end + 512).min(data.len());
        if let Some(rel) = memchr::memmem::find(&data[id_end..nonce_search_end], nk) {
            let ns = id_end + rel + nk.len();
            let ne = digits_end(data, ns);
            if ne > ns && ne - ns >= 5 {
                if let Ok(nonce) = std::str::from_utf8(&data[ns..ne]) {
                    return Some((account_id, nonce.to_string()));
                }
            }
        }
    }
    None
}

/// Also extract steamId from memory (found near accountId/nonce in URL params).
pub fn scan_steam_id(data: &[u8]) -> Option<String> {
    let key = b"steamId=";
    for pos in memchr::memmem::find_iter(data, key) {
        let id_start = pos + key.len();
        let id_end = data[id_start..].iter().position(|&b| !b.is_ascii_digit()).map(|p| id_start + p).unwrap_or(data.len());
        if id_end - id_start >= 15 && id_end - id_start <= 20 {
            if let Ok(sid) = std::str::from_utf8(&data[id_start..id_end]) {
                return Some(sid.to_string());
            }
        }
    }
    None
}

// ─── Public helpers ──────────────────────────────────────────────────────────

pub fn find_warframe_pid_pub() -> Option<u32> {
    use crate::platform::{ProcessAccess, Platform};
    Platform::find_warframe_pid()
}

// ─── Raw memory format probe ──────────────────────────────────────────────────
//
// Scans Warframe's memory and returns raw text context around every occurrence
// of a set of known strings.  Capped at max_hits total.  Used to reverse-engineer
// the actual JSON format for inventory items without any parsing assumptions.

#[tracing::instrument(level = "info", skip_all, fields(max_hits = max_hits))]
pub fn dump_inventory_regions(max_hits: usize) -> Vec<String> {
    use crate::platform::{Platform, ProcessAccess};

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

    let handle = match Platform::open_process(pid) {
        Ok(h) => h,
        Err(e) => return vec![format!("OpenProcess failed: {}", e)],
    };

    let mut results: Vec<String> = Vec::new();
    let mut addr: usize = 0x10000;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);

    const MAX_REGION: usize = 256 * 1024 * 1024;
    const CHUNK_SIZE: usize = 64 * 1024 * 1024;

    'outer: while std::time::Instant::now() < deadline && results.len() < max_hits {
        let regions: Vec<_> = handle.regions_from(addr).collect();
        if regions.is_empty() { break; }

        for region in &regions {
            if results.len() >= max_hits { break 'outer; }
            if std::time::Instant::now() >= deadline { break 'outer; }

            addr = region.base_address + region.region_size;
            if !region.is_committed || !region.is_readable || region.is_executable { continue; }
            if region.region_size < 4096 || region.region_size > MAX_REGION { continue; }

            let chunks = if region.region_size > CHUNK_SIZE {
                region.region_size.div_ceil(CHUNK_SIZE)
            } else { 1 };

            'chunk: for chunk_idx in 0..chunks {
                if results.len() >= max_hits { break 'outer; }
                if std::time::Instant::now() >= deadline { break 'outer; }

                let chunk_offset = chunk_idx * CHUNK_SIZE;
                let read_size    = CHUNK_SIZE.min(region.region_size - chunk_offset);
                let chunk_addr   = region.base_address + chunk_offset;

                let buf = match handle.read(chunk_addr, read_size) {
                    Some(r) => r,
                    None => continue 'chunk,
                };
                if buf.len() < 8 { continue 'chunk; }
                let data = &buf;

                for needle in NEEDLES {
                    if results.len() >= max_hits { break 'outer; }
                    if let Some(pos) = data.windows(needle.len()).position(|w| w == *needle) {
                        let ctx_start = pos.saturating_sub(80);
                        let ctx_end   = data.len().min(pos + 200);
                        let snip: String = data[ctx_start..ctx_end].iter()
                            .map(|&b| if (0x20..0x7f).contains(&b) { b as char } else { '·' })
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
                                    .map(|&b| if (0x20..0x7f).contains(&b) { b as char } else { '·' })
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
    }

    if results.is_empty() { results.push("No matches found".to_string()); }
    results
}

// Scan all Warframe process memory and save every relevant blob found into `blob_dir`.
// "Relevant" = region ≥ 100 KB that contains at least one of: MiscItems, Suits,
// ─── Full-account blob parser ─────────────────────────────────────────────────

/// Find the end of the FULL_ACCOUNT blob.
///
/// A marker field is the true end of the blob only when its boolean value is
/// immediately followed by `}` (the outer closing brace) with no `,` or further
/// fields between them. This distinguishes a genuine end-of-object occurrence
/// from a mid-blob occurrence where the same field name appears earlier in the
/// JSON with other fields still following it.
///
/// All occurrences of each marker are checked; the rightmost valid one wins.
fn find_blob_end(raw: &[u8]) -> Option<usize> {
    const MARKERS: &[&[u8]] = &[
        b"\"HWIDProtectEnabled\":",
        b"\"DeathSquadable\":",
    ];
    const BOOL_VALUES: &[&[u8]] = &[b"true", b"false"];
    let mut best: Option<usize> = None;
    for marker in MARKERS {
        let mut search_from = 0;
        while let Some(rel) = memchr::memmem::find(&raw[search_from..], marker) {
            let key_pos = search_from + rel;
            let after_colon = key_pos + marker.len();
            // Skip optional whitespace after the colon.
            let val_start = raw[after_colon..].iter()
                .position(|&b| !matches!(b, b' ' | b'\t' | b'\r' | b'\n'))
                .map_or(raw.len(), |p| after_colon + p);
            // Consume the boolean value.
            if let Some(val_len) = BOOL_VALUES.iter()
                .find(|v| raw[val_start..].starts_with(v))
                .map(|v| v.len())
            {
                let val_end = val_start + val_len;
                // Skip optional whitespace after the value.
                let after_val = raw[val_end..].iter()
                    .position(|&b| !matches!(b, b' ' | b'\t' | b'\r' | b'\n'))
                    .map_or(raw.len(), |p| val_end + p);
                // Only treat this as a blob end when `}` follows directly —
                // meaning no comma or additional fields come after the value.
                if raw.get(after_val) == Some(&b'}') {
                    let end = after_val + 1;
                    if best.is_none_or(|prev| end > prev) {
                        best = Some(end);
                    }
                }
            }
            search_from = key_pos + marker.len();
        }
    }
    best
}

/// Cross-checks `find_blob_end`'s literal-marker result by independently
/// walking forward from `buf`'s start (always the JSON `{`, by construction —
/// every caller passes a buffer already sliced to the true object start) and
/// counting brace/bracket depth, string- and escape-aware, until it returns
/// to zero.
///
/// This never changes what gets parsed — the literal marker stays the sole
/// source of truth for where a blob is cut. It only logs a warning on
/// disagreement, so a future DE blob restructuring that moves the literal end
/// markers (`HWIDProtectEnabled`/`DeathSquadable`) shows up in logs
/// immediately instead of silently truncating or corrupting a scan.
fn verify_blob_end_by_brace_count(buf: &[u8], literal_end: usize, scan_id: usize) {
    let mut depth: i32 = 0;
    let mut in_string = false;
    let mut escaped = false;
    for (i, &b) in buf.iter().enumerate() {
        if escaped { escaped = false; continue; }
        if in_string {
            match b {
                b'\\' => escaped = true,
                b'"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match b {
            b'"' => in_string = true,
            b'{' | b'[' => depth += 1,
            b'}' | b']' => {
                depth -= 1;
                if depth == 0 {
                    let brace_end = i + 1;
                    if brace_end != literal_end {
                        warn!(
                            target: "frameforge::blob_parse",
                            scan_id, literal_end, brace_count_end = brace_end,
                            "blob end mismatch: literal marker vs brace-count validator"
                        );
                    }
                    return;
                }
            }
            _ => {}
        }
    }
    warn!(
        target: "frameforge::blob_parse",
        scan_id, literal_end,
        "brace-count validator never closed — buffer may be malformed"
    );
}

const START_MARKER: &[u8] = b"\"SubscribedToEmails\"";
const ALT_STARTS: &[&[u8]] = &[
    b"\"RegularCredits\":",
    b"\"Created\":",
];

/// Walk backward from `marker_off` counting braces; return the offset of the
/// outermost `{` that encloses the marker.  Returns `None` when the buffer is
/// inconsistent (freed/partial blob where the opening brace was overwritten).
fn enclosing_object_start(buf: &[u8], marker_off: usize) -> Option<usize> {
    let mut depth: i32 = 0;
    for i in (0..marker_off).rev() {
        match buf[i] {
            b'}' => depth += 1,
            b'{' => {
                depth -= 1;
                if depth < 0 { return Some(i); }
            }
            _ => {}
        }
    }
    None
}

/// Locate seed offsets for the FULL_ACCOUNT blob within `buf`.
/// Returns `(marker_off, json_open)`:
///   - `marker_off`: byte position of the chosen start marker
///   - `json_open`:  byte position of the outermost `{` enclosing the marker
///
/// When multiple copies of the start marker exist in `buf` (a freed/stale copy
/// followed by the live one), the copy whose enclosing `{` is immediately followed
/// by a `"` (i.e. a proper JSON object opening `{"`) is preferred.  A stale copy
/// has its `{` overwritten with garbage, so its brace check fails and it is skipped
/// in favour of the live copy.
fn blob_seed_offsets(buf: &[u8]) -> (usize, usize) {
    // Scan all START_MARKER occurrences and pick the first that has a clean {"  brace.
    let mut fallback: Option<(usize, usize)> = None;
    for marker_off in memchr::memmem::find_iter(buf, START_MARKER) {
        match enclosing_object_start(buf, marker_off) {
            Some(pos) if buf[pos..].starts_with(b"{\"") => {
                return (marker_off, pos);
            }
            _ => {
                if fallback.is_none() {
                    fallback = Some((marker_off, marker_off));
                }
            }
        }
    }
    if let Some(fb) = fallback {
        return fb;
    }
    // No START_MARKER — try ALT_STARTS
    let marker_off = ALT_STARTS.iter()
        .find_map(|a| memchr::memmem::find(buf, a))
        .unwrap_or(buf.len().saturating_sub(1));
    let json_open = enclosing_object_start(buf, marker_off).unwrap_or(marker_off);
    (marker_off, json_open)
}

/// Parse a FULL_ACCOUNT blob from raw memory bytes into structured inventory data.
///
/// Compute the deterministic riven mod name from its buff stats.
/// Mirrors the RIVEN_NAME_PARTS table in MarketHelper.tsx.
/// 1 buff  → coreSuffix       (buff's prefix word + buff's suffix word)
/// 2 buffs → coreSuffix       (higher's prefix + lower's suffix, no dash)
/// 3 buffs → prefix-coreSuffix (highest - second + lowest, with dash)
pub fn compute_riven_mod_name(buffs: &[BlobRivenStat]) -> String {
    fn parts(tag: &str) -> Option<(&'static str, &'static str)> {
        match tag {
            "WeaponMeleeComboBonusOnHitMod" | "WeaponMeleeComboPointsOnHitMod" => Some(("Laci",  "Nus"  )),
            "WeaponAmmoMaxMod"                                                  => Some(("Ampi",  "Bin"  )),
            "WeaponMeleeFactionDamageCorpus"   | "WeaponFactionDamageCorpus"   => Some(("Manti", "Tron" )),
            "WeaponMeleeFactionDamageGrineer"  | "WeaponFactionDamageGrineer"  => Some(("Argi",  "Con"  )),
            "WeaponMeleeFactionDamageInfested" | "WeaponFactionDamageInfested" => Some(("Pura",  "Ada"  )),
            "WeaponFreezeDamageMod"            => Some(("Geli",  "Do"   )),
            "ComboDurationMod"                 => Some(("Tempi", "Nem"  )),
            "WeaponCritChanceMod"              => Some(("Crita", "Cron" )),
            "SlideAttackCritChanceMod"         => Some(("Pleci", "Nent" )),
            "WeaponCritDamageMod"              => Some(("Acri",  "Tis"  )),
            "WeaponDamageAmountMod" | "WeaponMeleeDamageMod" => Some(("Visi", "Ata")),
            "WeaponElectricityDamageMod"       => Some(("Vexi",  "Tio"  )),
            "WeaponFireDamageMod"              => Some(("Igni",  "Pha"  )),
            "WeaponMeleeFinisherDamageMod"     => Some(("Exi",   "Cta"  )),
            "WeaponFireRateMod"                => Some(("Croni", "Dra"  )),
            "WeaponProjectileSpeedMod"         => Some(("Conci", "Nak"  )),
            "WeaponMeleeComboInitialBonusMod"  => Some(("Para",  "Um"   )),
            "WeaponImpactDamageMod"            => Some(("Magna", "Ton"  )),
            "WeaponClipMaxMod"                 => Some(("Arma",  "Tin"  )),
            "WeaponMeleeComboEfficiencyMod"    => Some(("Forti", "Us"   )),
            "WeaponFireIterationsMod"          => Some(("Sati",  "Can"  )),
            "WeaponToxinDamageMod"             => Some(("Toxi",  "Tox"  )),
            "WeaponPunctureDepthMod"           => Some(("Lexi",  "Nok"  )),
            "WeaponArmorPiercingDamageMod"     => Some(("Insi",  "Cak"  )),
            "WeaponReloadSpeedMod"             => Some(("Feva",  "Tak"  )),
            "WeaponMeleeRangeIncMod"           => Some(("Locti", "Tor"  )),
            "WeaponSlashDamageMod"             => Some(("Sci",   "Sus"  )),
            "WeaponStunChanceMod"              => Some(("Hexa",  "Dex"  )),
            "WeaponProcTimeMod"                => Some(("Deci",  "Des"  )),
            "WeaponRecoilReductionMod"         => Some(("Zeti",  "Mag"  )),
            "WeaponZoomFovMod"                 => Some(("Hera",  "Lis"  )),
            _ => None,
        }
    }
    if buffs.is_empty() { return String::new(); }
    let mut sorted: Vec<&BlobRivenStat> = buffs.iter().collect();
    sorted.sort_by_key(|b| std::cmp::Reverse(b.value));
    let Some((hi_p, _))  = parts(&sorted[0].tag)                   else { return String::new(); };
    let Some((_, lo_s))  = parts(&sorted[sorted.len() - 1].tag)    else { return String::new(); };
    if sorted.len() >= 3 {
        if let Some((mid_p, _)) = parts(&sorted[1].tag) {
            return format!("{}-{}{}", hi_p.to_lowercase(), mid_p.to_lowercase(), lo_s.to_lowercase());
        }
    }
    format!("{}{}", hi_p.to_lowercase(), lo_s.to_lowercase())
}

/// Cut the JSON object out of a stitched memory buffer.
///
/// A scan buffer is whole memory regions glued together, so the blob's last
/// brace is followed by whatever heap bytes happened to sit in the tail of the
/// final region — potentially tens of megabytes of noise. This trims to just
/// the valid JSON object so both the parser and the debug dump files see clean data.
pub fn extract_blob_json(raw: &[u8]) -> Option<Vec<u8>> {
    Some(extract_blob_json_ref(raw)?.into_owned())
}

/// Zero-copy variant: borrows when the blob starts with `{`, allocates only when the
/// opening brace was overwritten and must be reinstated.
pub fn extract_blob_json_ref(raw: &[u8]) -> Option<std::borrow::Cow<'_, [u8]>> {
    let end_pos = find_blob_end(raw)?;
    if raw.first() == Some(&b'{') {
        Some(std::borrow::Cow::Borrowed(&raw[..end_pos]))
    } else {
        let start_pos = memchr::memmem::find(raw, START_MARKER)?;
        let mut v = Vec::with_capacity(end_pos - start_pos + 1);
        v.push(b'{');
        v.extend_from_slice(&raw[start_pos..end_pos]);
        Some(std::borrow::Cow::Owned(v))
    }
}

/// `raw` must span from the JSON opening `{` (or from `"SubscribedToEmails"`) through
/// `"DeathSquadable":`. Returns `None` if neither start can be located or JSON is malformed.
#[tracing::instrument(level = "debug", skip_all)]
pub fn parse_full_account_blob(raw: &[u8]) -> Option<BlobInventory> {
    let end_pos = find_blob_end(raw)?;

    // Real FULL_ACCOUNT blobs are several hundred KB — anything smaller is a
    // small false-positive fragment that matched the end marker by coincidence.
    const MIN_PARSE_BYTES: usize = 50_000;
    if end_pos < MIN_PARSE_BYTES {
        debug!(target: "frameforge::blob_parse", end_pos, min = MIN_PARSE_BYTES, "too small — skipping");
        return None;
    }

    // Section completeness check: field order varies per account, so the true
    // outer `{` of the blob can sit in a memory region the stitcher never
    // recovered — the resulting seed still parses as valid JSON, just starting
    // mid-object (e.g. right at SubscribedToEmails) and silently missing every
    // top-level section that came before it. The same failure mode also hits
    // the *tail* of the blob: a mis-seeded or prematurely-closed candidate can
    // still find a structurally valid end and parse cleanly while missing
    // everything after some cutoff.
    //
    // Checked against the real DE account schema (same field order as the
    // Companion API's own inventory response — verified directly, not
    // assumed): these six keys used to cluster in the first ~77% of the
    // object, so nothing guarded the back ~23% — which holds 8 of the 12
    // unique-item sections (SpaceSuits/SpaceMelee/SpaceGuns, Sentinels,
    // SentinelWeapons, KubrowPets, OperatorAmps, MechSuits), the riven list
    // (Upgrades), and platinum (PremiumCredits). A capture cut short anywhere
    // past PlayerLevel used to sail through this gate and get displayed with
    // those sections silently empty. Upgrades/MechSuits/PremiumCredits below
    // sit at ~80%/~98%/~99.9% of the real object respectively, so requiring
    // them closes that blind spot without any added false-rejection risk —
    // every key here is schema-guaranteed present regardless of account
    // progression, same as the original six.
    //
    // Which optional sections exist differs per account, so this is only a
    // small universal core that every real account has. The per-account check
    // ("a section the last good blob had is now gone") lives in `SectionBaseline`,
    // applied by the monitor loop using `BlobInventory::sections`.
    const CORE_SECTIONS: &[&[u8]] = &[
        b"\"RegularCredits\":",
        b"\"PlayerLevel\":",
        b"\"XPInfo\":",
        b"\"Suits\":",
        b"\"MiscItems\":",
        b"\"RawUpgrades\":",
        b"\"Upgrades\":",
        b"\"MechSuits\":",
        b"\"PremiumCredits\":",
    ];
    let search_range = &raw[..end_pos.min(raw.len())];
    for required in CORE_SECTIONS {
        if memchr::memmem::find(search_range, required).is_none() {
            debug!(
                target: "frameforge::blob_parse",
                missing = %std::str::from_utf8(required).unwrap_or("?"),
                "incomplete blob — missing required section, skipping"
            );
            return None;
        }
    }

    let json_bytes = extract_blob_json(raw)?;

    let json: serde_json::Value = serde_json::from_slice(&json_bytes)
        .map_err(|e| {
            let head: String = json_bytes[..json_bytes.len().min(48)]
                .iter().map(|&b| if (0x20..0x7f).contains(&b) { b as char } else { '.' }).collect();
            debug!(target: "frameforge::blob_parse", error = %e, head = ?head, "JSON error");
        })
        .ok()?;

    // Scalars
    let credits       = json["RegularCredits"].as_i64().unwrap_or(0);
    let endo          = json["FusionPoints"].as_i64().unwrap_or(0);
    let platinum      = json["PremiumCredits"].as_i64().unwrap_or(0);
    let free_platinum = json["PremiumCreditsFree"].as_i64().unwrap_or(0);
    let mastery_level = json["PlayerLevel"].as_u64().unwrap_or(0) as u32;

    // Unique item sections — each array entry = one owned copy
    const UNIQUE_SECS: &[&str] = &[
        "Suits", "LongGuns", "Pistols", "Melee",
        "SpaceSuits", "SpaceMelee", "SpaceGuns",
        "Sentinels", "SentinelWeapons", "KubrowPets",
        "OperatorAmps", "MechSuits",
    ];
    let mut unique_items = Vec::new();
    for &sec in UNIQUE_SECS {
        if let Some(arr) = json[sec].as_array() {
            for e in arr {
                let Some(it) = e["ItemType"].as_str() else { continue };
                if !it.starts_with("/Lotus/") { continue; }
                let archon_shards = e["ArchonCrystalUpgrades"].as_array()
                    .map(|a| a.iter().filter_map(|s| {
                        Some(ArchonShard {
                            color:        s["Color"].as_str()?.to_string(),
                            upgrade_type: s["UpgradeType"].as_str().unwrap_or("").to_string(),
                        })
                    }).collect())
                    .unwrap_or_default();
                let modular_parts = e["ModularParts"].as_array()
                    .map(|a| a.iter().filter_map(|p| p.as_str().map(String::from)).collect())
                    .unwrap_or_default();
                unique_items.push(BlobUniqueEntry {
                    item_type:     it.to_string(),
                    section:       sec.to_string(),
                    polarized:     e["Polarized"].as_u64().unwrap_or(0) as u32,
                    xp:            e["XP"].as_i64().unwrap_or(0),
                    item_name:     e["ItemName"].as_str().map(String::from),
                    pet_name:      e["Details"]["Name"].as_str().map(String::from),
                    focus_lens:    e["FocusLens"].as_str().map(String::from),
                    archon_shards,
                    modular_parts,
                });
            }
        }
    }

    // Stackable item sections
    const STACK_SECS: &[(&str, bool)] = &[
        ("MiscItems",          false),
        ("Recipes",            false),
        ("FusionTreasures",    true),   // has Sockets
        ("CrewShipRawSalvage", false),
        ("ShipDecorations",    false),
    ];
    let mut stackable_items = Vec::new();
    for &(sec, has_sockets) in STACK_SECS {
        if let Some(arr) = json[sec].as_array() {
            for e in arr {
                let Some(it) = e["ItemType"].as_str() else { continue };
                if !it.starts_with("/Lotus/") { continue; }
                let count = e["ItemCount"].as_i64().unwrap_or(0);
                if count <= 0 { continue; }
                stackable_items.push(BlobStackableEntry {
                    item_type:  it.to_string(),
                    item_count: count,
                    sockets:    if has_sockets { e["Sockets"].as_i64() } else { None },
                });
            }
        }
    }

    // Rivens + Mods: RawUpgrades (unranked, ItemCount) + Upgrades (ranked, one entry = one copy).
    // Riven paths contain "RandomMod" — extract them separately and skip from mods map.
    let mut rivens: Vec<BlobRivenEntry> = Vec::new();
    let mut mods: HashMap<String, ModCount> = HashMap::new();
    if let Some(arr) = json["RawUpgrades"].as_array() {
        for e in arr {
            let Some(it) = e["ItemType"].as_str() else { continue };
            if !it.starts_with("/Lotus/") { continue; }
            let count = e["ItemCount"].as_i64().unwrap_or(0);
            if count <= 0 { continue; }
            if it.contains("RandomMod") {
                // Unrevealed riven: stacked in RawUpgrades, only type visible.
                rivens.push(BlobRivenEntry {
                    item_id:  String::new(),
                    item_type: it.to_string(),
                    riven_state: RivenState::Unrevealed,
                    compat: None, challenge_type: None, challenge_complication: None,
                    lvl_req: None, polarity: None,
                    buffs: vec![], curses: vec![],
                    mod_rank: 0, count: count as u32, rerolls: 0,
                    mod_name: String::new(),
                });
                continue;
            }
            let mc = mods.entry(it.to_string()).or_default();
            *mc.by_rank.entry(0).or_insert(0) += count;
            mc.total += count;
        }
    }
    if let Some(arr) = json["Upgrades"].as_array() {
        for e in arr {
            let Some(it) = e["ItemType"].as_str() else { continue };
            if !it.starts_with("/Lotus/") { continue; }
            if it.contains("RandomMod") {
                let fp_str = e["UpgradeFingerprint"].as_str().unwrap_or("{}");
                if let Ok(fp) = serde_json::from_str::<serde_json::Value>(fp_str) {
                    let item_id = e["ItemId"]["$oid"].as_str().unwrap_or("").to_string();
                    if let Some(compat) = fp["compat"].as_str() {
                        // Unlocked riven: weapon assigned + stats visible.
                        let buffs: Vec<BlobRivenStat> = fp["buffs"].as_array()
                            .map(|a| a.iter().filter_map(|s| Some(BlobRivenStat {
                                tag:   s["Tag"].as_str()?.to_string(),
                                value: s["Value"].as_i64().unwrap_or(0),
                            })).collect())
                            .unwrap_or_default();
                        let curses: Vec<BlobRivenStat> = fp["curses"].as_array()
                            .map(|a| a.iter().filter_map(|s| Some(BlobRivenStat {
                                tag:   s["Tag"].as_str()?.to_string(),
                                value: s["Value"].as_i64().unwrap_or(0),
                            })).collect())
                            .unwrap_or_default();
                        let mod_name = compute_riven_mod_name(&buffs);
                        rivens.push(BlobRivenEntry {
                            item_id, item_type: it.to_string(),
                            riven_state: RivenState::Unlocked,
                            compat: Some(compat.to_string()),
                            challenge_type: None, challenge_complication: None,
                            lvl_req:  fp["lvlReq"].as_u64().map(|v| v as u32),
                            polarity: fp["pol"].as_str().map(String::from),
                            mod_rank: fp["lvl"].as_u64().map(|v| v as u8).unwrap_or(0),
                            count: 1,
                            rerolls: fp["rerolls"].as_u64().unwrap_or(0) as u32,
                            mod_name,
                            buffs,
                            curses,
                        });
                        continue;
                    } else if fp["challenge"].is_object() {
                        // Revealed riven: challenge assigned but not yet completed.
                        let challenge_type = fp["challenge"]["Type"].as_str().map(String::from);
                        let challenge_complication = fp["challenge"]["Complication"].as_str().map(String::from);
                        rivens.push(BlobRivenEntry {
                            item_id, item_type: it.to_string(),
                            riven_state: RivenState::Revealed,
                            compat: None, challenge_type, challenge_complication,
                            lvl_req: None, polarity: None,
                            buffs: vec![], curses: vec![],
                            mod_rank: 0, count: 1, rerolls: 0,
                            mod_name: String::new(),
                        });
                        continue;
                    }
                }
            }
            let rank = blob_extract_mod_rank(e["UpgradeFingerprint"].as_str());
            let mc = mods.entry(it.to_string()).or_default();
            *mc.by_rank.entry(rank).or_insert(0) += 1;
            mc.total += 1;
        }
    }

    // FlavourItems (glyphs, palettes, emotes, titles, ship skins): each entry = one copy.
    let mut flavour_items: HashMap<String, i64> = HashMap::new();
    if let Some(arr) = json["FlavourItems"].as_array() {
        for e in arr {
            let Some(it) = e["ItemType"].as_str() else { continue };
            if !it.starts_with("/Lotus/") { continue; }
            *flavour_items.entry(it.to_string()).or_insert(0) += 1;
        }
    }

    // WeaponSkins (sigils, cosmetic skins): each array entry = one owned copy,
    // count occurrences of the same ItemType.
    let mut weapon_skins: HashMap<String, i64> = HashMap::new();
    if let Some(arr) = json["WeaponSkins"].as_array() {
        for e in arr {
            let Some(it) = e["ItemType"].as_str() else { continue };
            if !it.starts_with("/Lotus/") { continue; }
            *weapon_skins.entry(it.to_string()).or_insert(0) += 1;
        }
    }

    // XPInfo → mastery ranks (covers items no longer owned)
    let mut mastery_data: HashMap<String, u32> = HashMap::new();
    if let Some(arr) = json["XPInfo"].as_array() {
        for e in arr {
            let Some(it) = e["ItemType"].as_str() else { continue };
            if let Some(xp) = e["XP"].as_i64() {
                let rank = xp_to_rank(xp, it);
                if rank > 0 { mastery_data.insert(it.to_string(), rank); }
            }
        }
    }

    // PendingRecipes (Foundry)
    let pending_recipes: Vec<BlobPendingRecipe> = json["PendingRecipes"].as_array()
        .map(|a| a.iter().filter_map(|e| {
            let it = e["ItemType"].as_str()?.to_string();
            let ms = e["CompletionDate"]["$date"]["$numberLong"]
                .as_str().and_then(|s| s.parse::<i64>().ok())
                .or_else(|| e["CompletionDate"]["$date"]["$numberLong"].as_i64())
                .unwrap_or(0);
            Some(BlobPendingRecipe { item_type: it, completion_ms: ms })
        }).collect())
        .unwrap_or_default();

    // Helminth consumed suits
    let consumed_suits: Vec<String> = json["InfestedFoundry"]["ConsumedSuits"].as_array()
        .map(|a| a.iter().filter_map(|e| e["s"].as_str().map(String::from)).collect())
        .unwrap_or_default();

    // Every valid FULL_ACCOUNT blob at the orbiter has at least one Warframe in Suits.
    // An empty unique_items means we captured an incomplete blob (game is mid-write,
    // returning from mission, or the blob sections were partially stitched out of order).
    if unique_items.is_empty() {
        debug!(target: "frameforge::blob_parse", "blob has no warframes/weapons — incomplete blob, rejecting");
        return None;
    }

    let sections: Vec<String> = json.as_object()
        .map(|o| o.keys().cloned().collect())
        .unwrap_or_default();

    let mut inv = BlobInventory {
        credits, endo, platinum, free_platinum, mastery_level,
        unique_items, stackable_items, mods,
        flavour_items, weapon_skins, mastery_data, pending_recipes, consumed_suits,
        rivens, sections, content_hash: 0,
    };
    inv.content_hash = inv.compute_content_hash();
    Some(inv)
}

/// Consecutive rejected blobs after which a missing section is accepted as
/// genuinely gone (~2 min at the 10 s scan interval).
pub const MAX_MISSING_STREAK: u32 = 12;

/// Top-level JSON sections that appear and disappear from memory depending on
/// game state (e.g. after a vendor interaction or inventory sync). These should
/// never trigger a truncated-capture rejection because their absence is normal.
const VOLATILE_SECTIONS: &[&str] = &[
    "RecentVendorPurchases",
    "MiscAccountData",
];

/// Per-account memory of which top-level sections a complete blob contains.
///
/// The monitor applies each accepted blob as a full replacement, so a blob that
/// lost its front (sections before `SubscribedToEmails`) would wipe those items
/// and the next full capture would restore them — visible flicker. Instead of a
/// fixed section list (which rejects accounts that never had an optional
/// section), a blob is rejected only if it lacks a section the previous accepted
/// blob had. Gaining sections is always fine.
#[derive(Debug, Default, Clone)]
pub struct SectionBaseline {
    known: std::collections::BTreeSet<String>,
    missing_streak: u32,
}

impl SectionBaseline {
    pub fn from_keys<I: IntoIterator<Item = String>>(keys: I) -> Self {
        Self { known: keys.into_iter().collect(), missing_streak: 0 }
    }

    pub fn keys(&self) -> Vec<String> {
        self.known.iter().cloned().collect()
    }

    /// `Ok(changed)` — blob accepted; `changed` is true when the baseline was updated
    /// and should be persisted. `Err(missing)` — blob rejected as truncated.
    pub fn evaluate(&mut self, sections: &[String]) -> Result<bool, Vec<String>> {
        let current: std::collections::BTreeSet<&str> = sections.iter().map(String::as_str).collect();
        let missing: Vec<String> = self.known.iter()
            .filter(|k| !current.contains(k.as_str()))
            .filter(|k| !VOLATILE_SECTIONS.contains(&k.as_str()))
            .cloned()
            .collect();

        if !missing.is_empty() {
            self.missing_streak += 1;
            if self.missing_streak < MAX_MISSING_STREAK {
                return Err(missing);
            }
            // Persistently absent: the account really dropped these sections.
        }

        self.missing_streak = 0;
        // Filter volatile sections from the baseline so they are never tracked.
        let new_known: std::collections::BTreeSet<String> = sections.iter()
            .filter(|k| !VOLATILE_SECTIONS.contains(&k.as_str()))
            .cloned()
            .collect();
        let changed = self.known != new_known;
        if changed {
            self.known = new_known;
        }
        Ok(changed)
    }
}

/// Extract the `lvl` field from a mod UpgradeFingerprint JSON string.
/// Returns 0 for unranked or missing fingerprint.
fn blob_extract_mod_rank(fingerprint: Option<&str>) -> u8 {
    fingerprint
        .and_then(|fp| {
            let pos = fp.find("\"lvl\":")?;
            let after = fp[pos + 6..].trim_start();
            let end = after.find(|c: char| !c.is_ascii_digit()).unwrap_or(after.len());
            after[..end].parse::<u8>().ok()
        })
        .unwrap_or(0)
}

// ─── Blob capture ─────────────────────────────────────────────────────────────

// Cache: remember the region address where the blob was last successfully found.
// On the next cycle the fast path re-reads that address first — if the blob is
// still there we finish in milliseconds instead of walking the full address space.
static LAST_BLOB_REGION: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

/// Clear the region cache. Call when Warframe's PID changes so the next scan
/// doesn't probe a stale address from the previous process instance.
pub fn reset_last_blob_region() {
    LAST_BLOB_REGION.store(0, std::sync::atomic::Ordering::Relaxed);
}

// ─── Shared constants ─────────────────────────────────────────────────────────

const MAX_READ: usize = 64 * 1024 * 1024;
const MAX_SCAN: usize = 20 * 1024 * 1024;
const MISSION_DELTA: &[u8] = b"\"InventoryChanges\":";
const LOTUS_KEY: &[u8] = b"/Lotus/";
const ANCHORS: &[&[u8]] = &[
    b"\"SubscribedToEmails\"",
    b"\"MiscItems\":[",
    b"\"Suits\":[",
    b"\"LongGuns\":[",
    b"\"Melee\":[",
    b"\"Pistols\":[",
];

/// Scans Warframe process memory for the FULL_ACCOUNT inventory blob and sends it
/// through `blob_tx` for the monitor loop to apply.
///
/// Multi-scan strategy: the blob may span many memory regions and multiple copies
/// can exist at different addresses. We track every potential start point as a
/// separate in-flight scan and stitch them all in parallel as the region walk
/// advances. The first scan that produces a valid JSON blob wins; all others are
/// dropped. This is far more robust than the old single-start approach when the
/// blob is large or when the first start hit leads to a truncated region.
///
/// Algorithm:
///   1. Walk every committed readable region.
///   2. If a region has START_MARKER ("SubscribedToEmails") and is NOT a mission
///      delta ("InventoryChanges"), open a new ActiveScan seeded with that region's
///      data from the START_MARKER offset onwards.
///   3. Every readable region is appended to ALL active scans (stitching).
///   4. After each append, check every scan for the end marker. If found, parse it.
///      On success send the inventory to the monitor loop. On failure drop the scan.
///      The walk always continues through all of memory — every blob start is found.
///   5. Drop any scan that grows past MAX_SCAN_BYTES without finding the end.
///
/// When `save=true` also writes the raw text to `blob_dir` for debugging.
/// Returns the number of files written (always 0 when `save=false`).
#[tracing::instrument(level = "debug", skip_all, fields(save = save))]
pub fn capture_all_blobs(blob_dir: &std::path::Path, ts: &str, blob_tx: std::sync::mpsc::Sender<BlobInventory>, save: bool) -> usize {
    use crate::mem_regions;
    const MIN_REGION: usize = 64_000;

    let pid = match find_warframe_pid_pub() { Some(p) => p, None => return 0 };
    let mut src = match mem_regions::open_region_source(
        pid,
        MIN_REGION,
        MAX_READ,
        MAX_SCAN,
        None, // no deadline — one-shot caller
        Some(Box::new(|r| {
            // Skip executable pages (code, JIT) and file-backed mappings
            // (PE image sections whose string constants false-trigger the
            // anchor check). Only anonymous heap/stack memory is useful.
            !r.is_executable && r.backing != crate::platform::RegionBacking::File
        })),
    ) {
        Some(s) => s,
        None => return 0,
    };

    let cached_addr = LAST_BLOB_REGION.load(std::sync::atomic::Ordering::Relaxed) as usize;
    if !save && cached_addr != 0 && try_cached_blob(&*src, cached_addr, &blob_tx) {
        return 0;
    }

    let saved = stitch_blobs(&mut *src, blob_dir, ts, blob_tx, save);
    let stats = src.stats();
    debug!(
        target: "frameforge::blob_capture",
        regions_skipped = stats.regions_skipped,
        vquery_ms = stats.enumerate_ms,
        read_ms = stats.read_ms,
        "source stats"
    );
    saved
}

/// Fast path: re-read the cached address from the last successful scan.
/// Returns true if a valid blob was found and sent, false if the fast path missed.
fn try_cached_blob(
    src: &dyn crate::mem_regions::RegionSource,
    cached_addr: usize,
    blob_tx: &std::sync::mpsc::Sender<BlobInventory>,
) -> bool {
    // Quick check: is the cached address still mapped? Avoids a costly read_at
    // on a region that has been unmapped since the last scan.
    match src.region_at(cached_addr) {
        Some(r) if r.is_committed && r.is_readable => {}
        _ => {
            debug!(addr = format_args!("0x{cached_addr:012x}"), "fast-path miss — region gone or unmapped");
            return false;
        }
    }

    let (mut next_addr, first_bytes) = match src.read_at(cached_addr, MAX_SCAN) {
        Some(r) => r,
        None => {
            debug!(addr = format_args!("0x{cached_addr:012x}"), "fast-path miss — read failed");
            return false;
        }
    };
    if first_bytes.len() < 8 {
        debug!(addr = format_args!("0x{cached_addr:012x}"), "fast-path miss — unreadable");
        return false;
    }

    let is_mission = memchr::memmem::find(&first_bytes, MISSION_DELTA).is_some();
    let has_anchor = ANCHORS.iter().any(|a| memchr::memmem::find(&first_bytes, a).is_some());
    let has_lotus  = memchr::memmem::find(&first_bytes, LOTUS_KEY).is_some();

    if is_mission || (!has_anchor && !has_lotus) || !first_bytes.starts_with(b"{\"") {
        debug!(addr = format_args!("0x{cached_addr:012x}"), "fast-path miss — falling through to cold walk");
        return false;
    }

    let mut stitched = first_bytes;
    while stitched.len() < MAX_SCAN && find_blob_end(&stitched).is_none() {
        let remaining = MAX_SCAN - stitched.len();
        match src.read_at(next_addr, remaining) {
            Some((end, bytes)) => {
                next_addr = end;
                if bytes.is_empty() { break; }
                stitched.extend_from_slice(&bytes);
            }
            None => break,
        }
    }

    if memchr::memmem::find(&stitched, START_MARKER).is_some() {
        if let Some(inv) = parse_full_account_blob(&stitched) {
            info!(
                addr = format_args!("0x{cached_addr:012x}"),
                unique = inv.unique_items.len(),
                stackable = inv.stackable_items.len(),
                "fast-path hit"
            );
            blob_tx.send(inv).ok();
            return true;
        }
    }

    debug!(addr = format_args!("0x{cached_addr:012x}"), "fast-path miss — falling through to cold walk");
    false
}

/// Walk all memory regions via `src`, stitch blobs, parse and send them.
/// Returns the number of blob files saved (always 0 when `save=false`).
fn stitch_blobs(
    src: &mut dyn crate::mem_regions::RegionSource,
    blob_dir: &std::path::Path,
    ts: &str,
    blob_tx: std::sync::mpsc::Sender<BlobInventory>,
    save: bool,
) -> usize {
    const MAX_BLOBS: usize = 25;
    // Outside save mode: cap on how many distinct candidate seeds (start-marker
    // occurrences) we'll open scans for, and how much further (in bytes read)
    // we'll keep walking after the first candidate resolves, looking for a second,
    // competing one. Bounded so the common case (exactly one blob in memory) still
    // exits about as fast as before, while a stale-vs-live pair — which sits in the
    // same general heap area, not scattered across the whole address space — still
    // gets caught. See `candidate_at_bytes`/`completed` below for why this matters.
    const MAX_CANDIDATE_SEEDS: usize = 6;
    const EXTRA_SEED_BUDGET: u64 = 64 * 1024 * 1024;

    struct ActiveScan {
        data: Vec<u8>,
        id: usize,
        /// Absolute address of the JSON opening brace this scan was seeded at
        /// (mid-region, not a region base). Cached in LAST_BLOB_REGION on success.
        seed_addr: usize,
        /// Minimum offset at which the end-marker search should start next append.
        /// Avoids rescanning already-checked data on every region append (O(n²) → O(n)).
        search_from: usize,
    }
    let mut scans: Vec<ActiveScan> = Vec::new();
    let mut next_scan_id = 0usize;

    // Pre-buffer: rolling window of recent regions that have Lotus paths / anchor keys
    // but no SubscribedToEmails yet.  Used to recover the true JSON start when the
    // outer `{` of the FULL_ACCOUNT blob lives in a region that precedes the region
    // containing SubscribedToEmails (field order varies by account).
    struct PreChunk { addr: usize, end_addr: usize, data: Vec<u8> }
    let mut pre_buf: std::collections::VecDeque<PreChunk> = std::collections::VecDeque::new();
    const PRE_BUF_BYTES: usize = 8 * 1024 * 1024; // keep ≤8 MB of prefix history

    let mut saved = 0usize;
    let mut regions_read    = 0usize;
    let mut starts_found    = 0usize;
    let mut t_search = std::time::Duration::ZERO;
    let mut bytes_read: u64 = 0;
    // Once we have at least one successful parse we stop opening new scans.
    // Active scans already in progress are still stitched to completion (or dropped).
    // The loop exits as soon as all active scans are gone.
    let mut found_result = false;
    // Multiple copies of the FULL_ACCOUNT blob can be readable in memory at once
    // (e.g. a buffer from before a re-serialize, not yet reclaimed/overwritten —
    // `blob_seed_offsets`'s `{"` check only catches a copy whose brace was already
    // clobbered, not one that's merely stale but still fully intact). Taking
    // whichever candidate happens to finish stitching first is a race that can pick
    // that stale-but-intact copy over the live one, silently rolling the displayed
    // inventory back. So outside `save` mode we don't commit to the first candidate:
    // every scan already in flight is left to resolve (succeed or get dropped for
    // growing too large) and the most complete one is chosen once none are left.
    // `save` mode is unaffected — it's a diagnostic dump of every candidate found,
    // not the live inventory feed, and keeps sending on each completion as before.
    // `candidate_at_bytes` is set the moment the first candidate resolves and drives
    // the EXTRA_SEED_BUDGET window below (see also MAX_CANDIDATE_SEEDS above).
    let mut candidate_at_bytes: Option<u64> = None;
    let mut completed: Vec<(usize, BlobInventory)> = Vec::new();

    loop {
        if saved >= MAX_BLOBS { break; }
        let seed_budget_exhausted = candidate_at_bytes
            .is_some_and(|b| bytes_read.saturating_sub(b) > EXTRA_SEED_BUDGET);
        let no_more_seeds = found_result
            || starts_found >= MAX_CANDIDATE_SEEDS
            || (!save && seed_budget_exhausted);
        // Early exit: we won't open any more candidate seeds and none are still in flight.
        if no_more_seeds && scans.is_empty() { break; }

        let (region_addr, buf) = match src.next_region() {
            Some(r) => r,
            None => break,
        };
        let n = buf.len();
        bytes_read += n as u64;
        let chunk = buf;
        regions_read += 1;

        // ── Step 1: append this chunk to every active scan and check for completion ──
        // search_from tracks where we left off so we only scan newly-appended bytes
        // (plus a small overlap for markers that straddle a region boundary).
        // Must stay in sync with find_blob_end's MARKERS list.
        const END_MARKERS: &[&[u8]] = &[b"\"HWIDProtectEnabled\":", b"\"DeathSquadable\":"];
        // Longest marker length drives the overlap window for straddled-boundary detection.
        const END_MARKER_MAX_LEN: usize = 21; // len("\"HWIDProtectEnabled\":")
        scans.retain_mut(|scan| {
            // Every scan already in flight is left to run to its own completion (or
            // get dropped below for growing too large) regardless of whether another
            // scan already completed — in save mode that's always been true (each
            // completion is its own diagnostic dump); outside save mode it's now true
            // too, so the decision between multiple valid candidates (see
            // `completed` above) is made once, after every in-flight scan has
            // resolved, not by which one merely finishes stitching first.
            // Advance the search cursor before appending so the overlap catches split markers.
            let search_from = scan.search_from;
            scan.search_from = scan.data.len().saturating_sub(END_MARKER_MAX_LEN - 1);
            scan.data.extend_from_slice(chunk);
            if scan.data.len() > MAX_SCAN {
                warn!(scan_id = scan.id, max_mb = MAX_SCAN / 1024 / 1024, "scan exceeded size limit without end — dropped");
                return false; // drop oversized scan
            }
            // Only search the newly-added window, not the full buffer.
            let window = &scan.data[search_from..];
            let has_end = END_MARKERS.iter().any(|m| memchr::memmem::find(window, m).is_some());
            let end_pos = if has_end { find_blob_end(&scan.data) } else { None };
            if let Some(end_pos) = end_pos {
                verify_blob_end_by_brace_count(&scan.data, end_pos, scan.id);
                match parse_full_account_blob(&scan.data) {
                    Some(inv) => {
                        info!(
                            scan_id = scan.id,
                            addr = format_args!("0x{:012x}", scan.seed_addr),
                            unique = inv.unique_items.len(),
                            stackable = inv.stackable_items.len(),
                            mods = inv.mods.len(),
                            "scan SUCCESS"
                        );
                        if save {
                            LAST_BLOB_REGION.store(scan.seed_addr as u64, std::sync::atomic::Ordering::Relaxed);
                            let name = format!("Actual_inventory_FULL_ACCOUNT_v{}_{}_{:02}.txt", env!("CARGO_PKG_VERSION"), ts, saved + 1);
                            let path = blob_dir.join(&name);
                            if let Some(json) = extract_blob_json(&scan.data) {
                                if std::fs::write(&path, &json).is_ok() { saved += 1; }
                            }
                            blob_tx.send(inv).ok();
                            found_result = true;
                        } else {
                            if candidate_at_bytes.is_none() { candidate_at_bytes = Some(bytes_read); }
                            completed.push((scan.seed_addr, inv));
                        }
                    }
                    None => {
                        warn!(scan_id = scan.id, addr = format_args!("0x{:012x}", scan.seed_addr), "end marker found but JSON parse failed — dropped");
                    }
                }
                false // remove completed (or failed) scan
            } else {
                true // keep waiting for end
            }
        });

        // ── Step 2: check if this chunk opens a new scan ──
        // Reuses `no_more_seeds` computed at the top of the loop (found_result, the
        // seed cap, or the post-first-candidate search budget). Deliberately *not*
        // gated on merely having one candidate already — a single realized candidate
        // must not stop us from also opening the one true live copy if it happens to
        // sit at a later address than a stale-but-still-intact one, which is exactly
        // the race this whole restructure exists to close.
        if no_more_seeds { continue; }

        let t2 = std::time::Instant::now();
        let has_start     = memchr::memmem::find(chunk, START_MARKER).is_some();
        let has_alt_start = ALT_STARTS.iter().any(|a| memchr::memmem::find(chunk, a).is_some());
        let is_mission    = memchr::memmem::find(chunk, MISSION_DELTA).is_some();
        let has_anchor    = ANCHORS.iter().any(|a| memchr::memmem::find(chunk, a).is_some());
        let has_lotus     = memchr::memmem::find(chunk, LOTUS_KEY).is_some();
        let qualifies     = (has_start || has_alt_start) && !is_mission && (has_anchor || has_lotus);
        t_search += t2.elapsed();

        // Accumulate regions with Lotus paths (no SubscribedToEmails, no mission delta) into
        // a pre-buffer.  When SubscribedToEmails is found in a later region, we prepend
        // contiguous pre-buffer regions so that the backward {"  search finds the true
        // outermost JSON opening rather than a nested {"$oid":…} inside the blob.
        if !has_start && !is_mission && (has_anchor || has_lotus) {
            // Keep only the tail of large regions in the pre-buffer: a chunk
            // bigger than PRE_BUF_BYTES can never be fully kept, so trim it to
            // PRE_BUF_BYTES before enqueueing and skip the normal eviction loop.
            let chunk_data = if n > PRE_BUF_BYTES {
                chunk[n - PRE_BUF_BYTES..].to_vec()
            } else {
                while pre_buf.iter().map(|p| p.data.len()).sum::<usize>() + n > PRE_BUF_BYTES
                    && !pre_buf.is_empty()
                {
                    pre_buf.pop_front();
                }
                chunk.to_vec()
            };
            let stored_len = chunk_data.len();
            pre_buf.push_back(PreChunk {
                addr:     region_addr + (n - stored_len),
                end_addr: region_addr + n,
                data:     chunk_data,
            });
        }

        if qualifies {
            // Prepend any contiguous pre-buffer regions that immediately precede this one.
            // This recovers the full blob when the outer { lives in an earlier region and
            // SubscribedToEmails appears later (field order varies per account/build).
            let mut combined: Vec<u8> = Vec::new();
            let mut blob_start_addr = region_addr;
            {
                let mut expect_end = region_addr;
                let mut chain: Vec<usize> = Vec::new();
                for (i, pc) in pre_buf.iter().enumerate().rev() {
                    // Allow ≤4 KB alignment gap between regions.
                    if pc.end_addr <= expect_end && pc.end_addr + 4096 >= expect_end {
                        chain.push(i);
                        expect_end = pc.addr;
                    } else if pc.end_addr < expect_end.saturating_sub(4096) {
                        break;
                    }
                }
                chain.reverse();
                for &i in &chain {
                    let pc = &pre_buf[i];
                    if combined.is_empty() { blob_start_addr = pc.addr; }
                    combined.extend_from_slice(&pc.data);
                }
            }
            combined.extend_from_slice(chunk);

            let (start_off, json_open) = blob_seed_offsets(&combined);

            // Absolute memory address of the seed start (for LAST_BLOB_REGION cache).
            let seed_addr = blob_start_addr + json_open;

            let id = next_scan_id;
            next_scan_id += 1;
            starts_found += 1;
            let pre_bytes = combined.len() - n;
            debug!(
                scan_id = id,
                addr = format_args!("0x{region_addr:012x}"),
                start_off,
                json_open,
                seed = format_args!("0x{seed_addr:012x}"),
                pre_bytes,
                "scan started"
            );
            let seed = combined[json_open..].to_vec();

            let seed_end = find_blob_end(&seed);
            if let Some(end_pos) = seed_end {
                verify_blob_end_by_brace_count(&seed, end_pos, id);
                match parse_full_account_blob(&seed) {
                    Some(inv) => {
                        info!(
                            scan_id = id,
                            addr = format_args!("0x{region_addr:012x}"),
                            unique = inv.unique_items.len(),
                            stackable = inv.stackable_items.len(),
                            "scan immediate SUCCESS"
                        );
                        if save {
                            LAST_BLOB_REGION.store(seed_addr as u64, std::sync::atomic::Ordering::Relaxed);
                            let name = format!("Actual_inventory_FULL_ACCOUNT_v{}_{}_{:02}.txt", env!("CARGO_PKG_VERSION"), ts, saved + 1);
                            if let Some(json) = extract_blob_json(&seed) {
                                if std::fs::write(blob_dir.join(&name), &json).is_ok() { saved += 1; }
                            }
                            blob_tx.send(inv).ok();
                            found_result = true;
                        } else {
                            if candidate_at_bytes.is_none() { candidate_at_bytes = Some(bytes_read); }
                            completed.push((seed_addr, inv));
                        }
                    }
                    None => {
                        warn!(scan_id = id, addr = format_args!("0x{seed_addr:012x}"), "immediate end found but parse failed — dropped");
                    }
                }
            } else {
                scans.push(ActiveScan { data: seed, id, seed_addr, search_from: 0 });
            }
        }
    }

    // Among every candidate this pass resolved, keep the most complete one — a
    // still-intact stale copy of the blob is a full, validly-parsing snapshot too,
    // just an older one, so item/mod/riven count (which only grows in the very
    // common case, since players rarely lose most of their account at once) is a
    // far better signal of "current" than "which one finished stitching first".
    // Ties (e.g. a single candidate) keep the last one, which is fine — it's the
    // only one, or an arbitrary but deterministic pick among truly-identical data.
    let candidates = completed.len();
    if let Some((seed_addr, inv)) = completed.into_iter().max_by_key(|(_, inv)| {
        inv.unique_items.len() + inv.stackable_items.len() + inv.mods.len()
    }) {
        LAST_BLOB_REGION.store(seed_addr as u64, std::sync::atomic::Ordering::Relaxed);
        info!(
            addr = format_args!("0x{seed_addr:012x}"),
            candidates,
            unique = inv.unique_items.len(),
            stackable = inv.stackable_items.len(),
            mods = inv.mods.len(),
            "picked most-complete candidate of this capture pass"
        );
        blob_tx.send(inv).ok();
    }

    debug!(
        target: "frameforge::blob_capture",
        regions_read,
        starts_found,
        saved,
        candidates,
        bytes_mb = bytes_read / 1_000_000,
        search_ms = t_search.as_secs_f64() * 1000.0,
        "capture done"
    );
    if starts_found == 0 {
        warn!(target: "frameforge::blob_capture", "no start-marker found — FULL_ACCOUNT not in memory (game in mission, on login screen, or Arsenal not open?)");
    }
    saved
}


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

pub fn raw_scan_pass(out: &mut impl std::io::Write) -> Result<usize, String> {
    use crate::platform::{Platform, ProcessAccess};

    const MIN_LEN:  usize = 8;
    const CHUNK:    usize = 64 * 1024 * 1024;
    const TIMEOUT:  u64   = 600; // 10 minutes — full coverage over full scan

    let pid = find_warframe_pid().ok_or("Warframe not running")?;
    let handle = Platform::open_process(pid)
        .map_err(|e| format!("OpenProcess failed: {}", e))?;

    let mut addr: usize = 0x10000;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(TIMEOUT);
    let mut count = 0usize;

    while std::time::Instant::now() < deadline {
        let regions: Vec<_> = handle.regions_from(addr).collect();
        if regions.is_empty() { break; }

        for region in &regions {
            if std::time::Instant::now() >= deadline { break; }

            addr = region.base_address + region.region_size;
            if !region.is_committed || !region.is_readable { continue; }

            let chunks = region.region_size.div_ceil(CHUNK);
            for ci in 0..chunks {
                if std::time::Instant::now() >= deadline { break; }
                let off        = ci * CHUNK;
                let read_size  = CHUNK.min(region.region_size - off);
                let chunk_base = region.base_address + off;

                let buf = match handle.read(chunk_base, read_size) {
                    Some(r) => r,
                    None => continue,
                };
                if buf.len() < MIN_LEN { continue; }

                // Extract printable ASCII runs of MIN_LEN+
                let data = &buf;
                let mut run_start: Option<usize> = None;
                for (i, &b) in data.iter().enumerate() {
                    let printable = (0x20..0x7f).contains(&b);
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
                    let len = data.len() - s;
                    if len >= MIN_LEN {
                        let s_str = std::str::from_utf8(&data[s..]).unwrap_or("?");
                        let _ = writeln!(out, "0x{:012x}  {}", chunk_base + s, s_str);
                        count += 1;
                    }
                }
            }
        }
    }

    Ok(count)
}

fn find_warframe_pid() -> Option<u32> {
    <crate::platform::Platform as crate::platform::ProcessAccess>::find_warframe_pid()
}

#[cfg(test)]
mod seed_tests {
    use super::{enclosing_object_start, blob_seed_offsets, extract_blob_json, extract_blob_json_ref, find_blob_end};

    #[test]
    fn enclosing_finds_outer_brace() {
        let buf = b"{\"SubscribedToEmails\":1}";
        let off = buf.windows(b"SubscribedToEmails".len())
            .position(|w| w == b"SubscribedToEmails").unwrap();
        assert_eq!(enclosing_object_start(buf, off), Some(0));
    }

    #[test]
    fn enclosing_skips_nested_braces() {
        let buf = b"{\"nested\":{\"a\":1},\"SubscribedToEmails\":1}";
        let off = buf.windows(b"SubscribedToEmails".len())
            .position(|w| w == b"SubscribedToEmails").unwrap();
        assert_eq!(enclosing_object_start(buf, off), Some(0));
    }

    #[test]
    fn enclosing_returns_none_when_no_open_brace() {
        // Stale/freed blob — the outer { was overwritten
        let buf = b"x\"SubscribedToEmails\":1}";
        assert_eq!(enclosing_object_start(buf, 0), None);
    }

    #[test]
    fn seed_offsets_with_start_marker() {
        let buf = b"{\"SubscribedToEmails\":1,\"RegularCredits\":100}";
        let (marker_off, json_open) = blob_seed_offsets(buf);
        assert_eq!(json_open, 0);
        // Both markers are present; the primary one must win over the alt-start.
        assert_eq!(marker_off, 1);
    }

    #[test]
    fn seed_offsets_with_alt_start_regular_credits() {
        // No SubscribedToEmails — falls through to RegularCredits alt-start
        let buf = b"{\"RegularCredits\":999}";
        let (marker_off, json_open) = blob_seed_offsets(buf);
        assert_eq!(json_open, 0);
        assert_eq!(marker_off, 1);
    }

    #[test]
    fn seed_offsets_stale_blob_falls_back_to_marker() {
        // The outer { was overwritten — json_open falls back to marker_off
        let buf = b"x\"SubscribedToEmails\":1}";
        let (marker_off, json_open) = blob_seed_offsets(buf);
        assert_eq!(json_open, marker_off);
    }

    #[test]
    fn blob_json_stops_at_the_closing_brace_of_the_object() {
        // A stitched scan buffer: the blob, then the rest of the memory region
        // it happened to end in.
        let mut raw = br#"{"SubscribedToEmails":0,"DeathSquadable":false}"#.to_vec();
        let blob_len = raw.len();
        raw.extend(std::iter::repeat_n(0xABu8, 1_000_000));

        let json = extract_blob_json(&raw).expect("end marker present");
        assert_eq!(json.len(), blob_len);
        assert!(serde_json::from_slice::<serde_json::Value>(&json).is_ok());
    }

    /// A freed copy of the blob can sit ahead of the live one with its marker
    /// intact but its opening brace overwritten. Brace-matching backward from
    /// that copy lands on a stray `{` in binary garbage ("key must be a string
    /// at line 1 column 2"), so the stale occurrence has to be skipped in
    /// favour of the live one.
    #[test]
    fn a_stale_headless_copy_is_skipped_for_the_live_blob() {
        let mut combined = b"\x00{J>\x01\x02 garbage ".to_vec();
        combined.extend_from_slice(br#""SubscribedToEmails":0,"RegularCredits":1,"#);
        combined.extend_from_slice(b"\x03\x04 more garbage ");
        let live_at = combined.len();
        combined.extend_from_slice(br#"{"SubscribedToEmails":0,"RegularCredits":42}"#);

        let (marker_at, seed_at) = blob_seed_offsets(&combined);
        assert_eq!(seed_at, live_at, "seed is the live copy's opening brace");
        assert!(marker_at > live_at, "the marker used is the live copy's");
    }

    /// With only the headless copy in the buffer, seeding at the marker lets
    /// the parser rebuild the object head instead of parsing garbage.
    #[test]
    fn a_lone_headless_copy_seeds_at_its_marker() {
        let mut combined = b"\x00{J>\x01\x02 garbage ".to_vec();
        let marker = combined.len();
        combined.extend_from_slice(br#""SubscribedToEmails":0,"RegularCredits":42,"#);

        let (marker_at, seed_at) = blob_seed_offsets(&combined);
        assert_eq!(marker_at, marker);
        assert_eq!(seed_at, marker, "seed skips the garbage brace");
    }

    #[test]
    fn blob_json_reinstates_the_opening_brace_when_it_was_overwritten() {
        let mut raw = br#"x"SubscribedToEmails":0,"DeathSquadable":false}"#.to_vec();
        let blob_len = raw.len();
        raw.extend(std::iter::repeat_n(0xABu8, 1024));

        let json = extract_blob_json(&raw).expect("end marker present");
        assert_eq!(json.len(), blob_len);
        assert_eq!(json[0], b'{');
        assert!(serde_json::from_slice::<serde_json::Value>(&json).is_ok());
    }

    #[test]
    fn blob_json_ref_borrows_in_the_common_case_and_owns_in_the_fallback() {
        use std::borrow::Cow;
        let intact = br#"{"SubscribedToEmails":0,"DeathSquadable":false}"#;
        assert!(matches!(extract_blob_json_ref(intact), Some(Cow::Borrowed(_))));

        let overwritten = br#"x"SubscribedToEmails":0,"DeathSquadable":false}"#;
        assert!(matches!(extract_blob_json_ref(overwritten), Some(Cow::Owned(_))));
    }

    /// A marker that appears mid-blob (followed by `,` and more fields) must not
    /// be treated as the end of the blob — only occurrences directly followed by
    /// `}` are valid end positions.
    #[test]
    fn mid_blob_marker_is_not_treated_as_end() {
        // HWIDProtectEnabled appears mid-blob with fields after it; DeathSquadable is absent.
        // The first `}` after HWIDProtectEnabled's value closes a nested object, not the blob.
        // find_blob_end must return None here — not a position inside the blob.
        let raw = br#"{"SubscribedToEmails":0,"HWIDProtectEnabled":false,"MiscItems":[{"ItemType":"/x","ItemCount":1}]}"#;
        // The outer `}` is the real end; but HWIDProtectEnabled is followed by `,` so it
        // must be skipped. Neither marker appears as a true last field, so result is None.
        assert!(find_blob_end(raw).is_none(), "mid-blob marker must not be accepted as blob end");
    }

    /// When HWIDProtectEnabled is genuinely the last field (directly followed by `}`),
    /// it must be accepted as the end marker.
    #[test]
    fn hwid_marker_accepted_when_last_field() {
        let raw = br#"{"SubscribedToEmails":0,"MiscItems":[],"HWIDProtectEnabled":false}"#;
        let end = find_blob_end(raw).expect("HWIDProtectEnabled is last field — must be found");
        assert_eq!(end, raw.len(), "end position must be the outer closing brace");
    }

    /// When both markers are present, the rightmost valid one wins.
    #[test]
    fn rightmost_valid_marker_wins() {
        // DeathSquadable comes after HWIDProtectEnabled and is the true last field.
        let raw = br#"{"HWIDProtectEnabled":true,"DeathSquadable":false}"#;
        let end = find_blob_end(raw).expect("DeathSquadable is last — must be found");
        assert_eq!(end, raw.len());
    }
}

#[cfg(test)]
mod credential_scan_tests {
    use super::{scan_auth_credentials, scan_steam_id};

    #[test]
    fn auth_credentials_finds_json_form() {
        let buf = br#"{"id":"594144e63ade7f2f2091c48e","Nonce":123456789}"#;
        let (account_id, nonce) = scan_auth_credentials(buf).expect("should find credentials");
        assert_eq!(account_id, "594144e63ade7f2f2091c48e");
        assert_eq!(nonce, "123456789");
    }

    #[test]
    fn auth_credentials_finds_url_encoded_form() {
        let buf = b"accountId=594144e63ade7f2f2091c48e&nonce=123456789&ct=STM";
        let (account_id, nonce) = scan_auth_credentials(buf).expect("should find credentials");
        assert_eq!(account_id, "594144e63ade7f2f2091c48e");
        assert_eq!(nonce, "123456789");
    }

    #[test]
    fn auth_credentials_none_on_no_match() {
        let buf = b"nothing interesting in here at all";
        assert_eq!(scan_auth_credentials(buf), None);
    }

    #[test]
    fn steam_id_finds_value_past_false_starts() {
        // Leading 's' bytes are false starts for the old byte-at-a-time scanner.
        let buf = b"ssssssssteamId=steamId=76561198012345678";
        let sid = scan_steam_id(buf).expect("should find steam id");
        assert_eq!(sid, "76561198012345678");
    }

    #[test]
    fn steam_id_none_on_no_match() {
        let buf = b"steamId=short";
        assert_eq!(scan_steam_id(buf), None);
    }
}

#[cfg(test)]
mod stitch_engine_tests {
    use super::{stitch_blobs, BlobInventory};
    use crate::mem_regions::RecordedRegions;

    /// Builds a blob with every section `parse_full_account_blob`'s completeness
    /// checks require: all sections in `CORE_SECTIONS` (plus the optional ones), padded past
    /// `MIN_PARSE_BYTES`, and one real Suit entry (parse_full_account_blob also
    /// rejects a blob with zero unique items as incomplete). `fields` is
    /// interpolated last, right before the padding/close, so a test's override
    /// (e.g. `"RegularCredits":42`) wins over the section's default here — JSON
    /// objects with a duplicate key keep the last occurrence's value.
    fn make_blob(fields: &str) -> Vec<u8> {
        let padding = "x".repeat(60_000);
        format!(
            r#"{{"SubscribedToEmails":0,
"RegularCredits":0,"FusionPoints":0,"PremiumCredits":0,"PremiumCreditsFree":0,"PlayerLevel":0,
"FlavourItems":[],"InfestedFoundry":{{}},"PendingRecipes":[],"RawUpgrades":[],"Upgrades":[],"WeaponSkins":[],"XPInfo":[],
"Suits":[{{"ItemType":"/Lotus/Powersuits/Test/Test"}}],"LongGuns":[],"Pistols":[],"Melee":[],"SpaceSuits":[],"SpaceMelee":[],"SpaceGuns":[],"Sentinels":[],"SentinelWeapons":[],"KubrowPets":[],"OperatorAmps":[],"MechSuits":[],
"MiscItems":[],"Recipes":[],"FusionTreasures":[],"CrewShipRawSalvage":[],"ShipDecorations":[],
"_pad":"{padding}",
{fields},
"DeathSquadable":false}}"#
        )
        .into_bytes()
    }

    fn run(regions: Vec<(usize, Vec<u8>)>) -> Option<BlobInventory> {
        let mut src = RecordedRegions::new(regions);
        let (tx, rx) = std::sync::mpsc::channel();
        let dir = std::env::temp_dir();
        stitch_blobs(&mut src, &dir, "test", tx, false);
        rx.try_recv().ok()
    }

    #[test]
    fn single_region_blob_is_parsed() {
        let blob = make_blob(r#""RegularCredits":12345"#);
        let inv = run(vec![(0x1000, blob)]).expect("should parse");
        assert_eq!(inv.credits, 12345);
    }

    #[test]
    fn blob_spanning_two_regions_is_stitched() {
        let blob = make_blob(r#""RegularCredits":999"#);
        let mid = blob.len() / 2;
        let r1 = (0x1000, blob[..mid].to_vec());
        let r2 = (0x1000 + mid, blob[mid..].to_vec());
        let inv = run(vec![r1, r2]).expect("should stitch and parse");
        assert_eq!(inv.credits, 999);
    }

    #[test]
    fn mission_delta_region_is_skipped() {
        let delta = b"\"InventoryChanges\":[{\"Credits\":1}]".to_vec();
        let blob  = make_blob(r#""RegularCredits":77"#);
        // Delta at a lower address; real blob follows.
        let inv = run(vec![(0x1000, delta), (0x2000, blob)]).expect("real blob should win");
        assert_eq!(inv.credits, 77);
    }

    #[test]
    fn oversized_scan_is_dropped_and_does_not_panic() {
        // First region qualifies (has start marker) but never closes;
        // second region is the real complete blob.
        let mut open = make_blob(r#""RegularCredits":1"#);
        // Strip the closing brace so it looks like a truncated blob.
        open.pop();
        // Pad it past MAX_SCAN so the engine drops it.
        open.extend(vec![b' '; super::MAX_SCAN + 1]);

        let real = make_blob(r#""RegularCredits":42"#);
        let inv = run(vec![(0x1000, open), (0x9000_0000, real)]).expect("real blob should parse");
        assert_eq!(inv.credits, 42);
    }

    // Regression coverage for: two independently-valid, independently-complete
    // copies of the FULL_ACCOUNT blob exist in memory at once (e.g. a buffer from
    // before a re-serialize, still fully intact and not yet overwritten, alongside
    // the live one). Taking whichever one finishes stitching first is a race that
    // can silently apply the stale, smaller copy over the live one. The fix: every
    // candidate this capture pass finds is left to resolve, and the most complete
    // one (by item/mod count) is the one actually sent — regardless of which
    // address it was found at or which one happened to finish first.

    #[test]
    fn stale_copy_at_a_lower_address_does_not_beat_the_live_one() {
        let stale = make_blob(r#""RegularCredits":1"#); // 1 Suit, from the template default
        let live = make_blob(
            r#""RegularCredits":2,"Suits":[{"ItemType":"/Lotus/Powersuits/A/A"},{"ItemType":"/Lotus/Powersuits/B/B"},{"ItemType":"/Lotus/Powersuits/C/C"}]"#,
        );
        let inv = run(vec![(0x1000, stale), (0x9000, live)]).expect("should pick a candidate");
        assert_eq!(inv.credits, 2, "the more-complete candidate should win even though the smaller one was found first");
        assert_eq!(inv.unique_items.len(), 3);
    }

    #[test]
    fn stale_copy_at_a_higher_address_does_not_beat_the_live_one() {
        // Same scenario with addresses swapped — the outcome must not depend on
        // which candidate the region walk happens to encounter first.
        let live = make_blob(
            r#""RegularCredits":2,"Suits":[{"ItemType":"/Lotus/Powersuits/A/A"},{"ItemType":"/Lotus/Powersuits/B/B"},{"ItemType":"/Lotus/Powersuits/C/C"}]"#,
        );
        let stale = make_blob(r#""RegularCredits":1"#);
        let inv = run(vec![(0x1000, live), (0x9000, stale)]).expect("should pick a candidate");
        assert_eq!(inv.credits, 2, "the more-complete candidate should win regardless of scan order");
        assert_eq!(inv.unique_items.len(), 3);
    }
}

#[cfg(test)]
mod section_baseline_tests {
    use super::{parse_full_account_blob, SectionBaseline, MAX_MISSING_STREAK};

    /// Account with only the core sections — no Sentinels, KubrowPets, etc.
    /// (MechSuits/Upgrades/PremiumCredits are themselves part of `CORE_SECTIONS`
    /// now, so they're present-but-empty here, same as the other core keys.)
    fn minimal_account_blob() -> Vec<u8> {
        let padding = "x".repeat(60_000);
        format!(
            r#"{{"SubscribedToEmails":0,"RegularCredits":5,"PremiumCredits":0,"PlayerLevel":3,"XPInfo":[],
"Suits":[{{"ItemType":"/Lotus/Powersuits/Test/Test"}}],"MiscItems":[],"RawUpgrades":[],"Upgrades":[],"MechSuits":[],
"_pad":"{padding}","DeathSquadable":false}}"#
        )
        .into_bytes()
    }

    fn keys(v: &[&str]) -> Vec<String> { v.iter().map(|s| s.to_string()).collect() }

    #[test]
    fn account_without_optional_sections_is_accepted() {
        let inv = parse_full_account_blob(&minimal_account_blob()).expect("core-only account must parse");
        assert_eq!(inv.credits, 5);
        assert!(inv.sections.iter().any(|s| s == "Suits"));
        assert!(!inv.sections.iter().any(|s| s == "Sentinels"));
    }

    #[test]
    fn identical_blobs_hash_equal_and_changed_content_differs() {
        let a = parse_full_account_blob(&minimal_account_blob()).unwrap();
        let b = parse_full_account_blob(&minimal_account_blob()).unwrap();
        assert_ne!(a.content_hash, 0);
        assert_eq!(a.content_hash, b.content_hash);

        let changed = String::from_utf8(minimal_account_blob()).unwrap()
            .replace("\"RegularCredits\":5", "\"RegularCredits\":6");
        let c = parse_full_account_blob(changed.as_bytes()).unwrap();
        assert_ne!(a.content_hash, c.content_hash);
    }

    #[test]
    fn blob_missing_a_core_section_is_rejected() {
        let s = String::from_utf8(minimal_account_blob()).unwrap().replace("\"MiscItems\":[],", "");
        assert!(parse_full_account_blob(s.as_bytes()).is_none());
    }

    #[test]
    fn first_blob_is_accepted_and_sets_baseline() {
        let mut b = SectionBaseline::default();
        assert_eq!(b.evaluate(&keys(&["A", "B"])), Ok(true));
        assert_eq!(b.keys(), vec!["A", "B"]);
    }

    #[test]
    fn identical_blob_does_not_change_baseline() {
        let mut b = SectionBaseline::from_keys(keys(&["A", "B"]));
        assert_eq!(b.evaluate(&keys(&["B", "A"])), Ok(false));
    }

    #[test]
    fn gaining_a_section_is_accepted() {
        let mut b = SectionBaseline::from_keys(keys(&["A", "B"]));
        assert_eq!(b.evaluate(&keys(&["A", "B", "C"])), Ok(true));
        assert_eq!(b.keys(), vec!["A", "B", "C"]);
    }

    #[test]
    fn truncated_blob_after_good_blob_is_rejected() {
        let mut b = SectionBaseline::from_keys(keys(&["A", "B", "C"]));
        assert_eq!(b.evaluate(&keys(&["B", "C"])), Err(keys(&["A"])));
        // Baseline untouched: the next complete blob is still accepted unchanged.
        assert_eq!(b.evaluate(&keys(&["A", "B", "C"])), Ok(false));
    }

    #[test]
    fn interleaved_good_blobs_reset_the_streak() {
        let mut b = SectionBaseline::from_keys(keys(&["A", "B"]));
        for _ in 0..(MAX_MISSING_STREAK * 3) {
            assert!(b.evaluate(&keys(&["B"])).is_err());
            assert_eq!(b.evaluate(&keys(&["A", "B"])), Ok(false));
        }
    }

    #[test]
    fn persistently_missing_section_is_eventually_accepted() {
        let mut b = SectionBaseline::from_keys(keys(&["A", "B"]));
        for _ in 0..(MAX_MISSING_STREAK - 1) {
            assert!(b.evaluate(&keys(&["B"])).is_err());
        }
        assert_eq!(b.evaluate(&keys(&["B"])), Ok(true));
        assert_eq!(b.keys(), vec!["B"]);
    }
}

#[cfg(test)]
mod xp_to_rank_tests {
    use super::xp_to_rank;

    #[test]
    fn moa_weapons_use_weapon_curve_not_companion_curve() {
        // Real paths for Cryotra, Helstrum, Tazicor, Vulcax, Multron.
        for path in [
            "/Lotus/Types/Friendly/Pets/MoaPets/MoaPetComponents/CryoxionWeapon",
            "/Lotus/Types/Friendly/Pets/MoaPets/MoaPetComponents/SwarmerWeapon",
            "/Lotus/Types/Friendly/Pets/MoaPets/MoaPetComponents/TazronWeapon",
            "/Lotus/Types/Friendly/Pets/MoaPets/MoaPetComponents/ThermocorMoaWeapon",
            "/Lotus/Types/Friendly/Pets/MoaPets/MoaPetComponents/HextraWeapon",
        ] {
            // 500*30^2 = 450000 XP should land exactly on rank 30 under the
            // weapon curve; the companion curve (1000*N^2) would only reach
            // rank 21 at this XP, so this also catches a regression back to
            // the old behaviour.
            assert_eq!(xp_to_rank(450_000, path), 30, "path: {path}");
        }
    }

    #[test]
    fn zanuka_weapons_use_weapon_curve_not_companion_curve() {
        // Real paths for Akaten, Batoten, Lacerten (element-suffixed, don't
        // end in "Weapon").
        for path in [
            "/Lotus/Types/Friendly/Pets/ZanukaPets/ZanukaPetMeleeWeaponPS",
            "/Lotus/Types/Friendly/Pets/ZanukaPets/ZanukaPetMeleeWeaponIP",
            "/Lotus/Types/Friendly/Pets/ZanukaPets/ZanukaPetMeleeWeaponIS",
        ] {
            assert_eq!(xp_to_rank(450_000, path), 30, "path: {path}");
        }
    }

    #[test]
    fn moa_companion_body_still_uses_companion_curve() {
        // The MOA companion body itself (not its weapon) should still get
        // the companion curve — only the *Weapon sub-paths are weapons.
        let path = "/Lotus/Types/Friendly/Pets/MoaPets/MoaPetPowerSuit";
        assert_eq!(xp_to_rank(450_000, path), 21);
    }

    #[test]
    fn sentinel_weapon_unaffected_by_companion_weapon_carveout() {
        // Sentinel weapons already lived outside /Types/Friendly/ and should
        // be untouched by this fix.
        let path = "/Lotus/Types/Sentinels/SentinelWeapons/DethMachineRifle";
        assert_eq!(xp_to_rank(450_000, path), 30);
    }
}
