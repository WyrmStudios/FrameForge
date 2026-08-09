use memchr::memmem;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::HashMap;

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
}

/// One owned unique item (warframe, weapon, companion, archwing, amp, mech).
/// Multiple entries with the same item_type = multiple owned copies.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlobUniqueEntry {
    pub item_type:     String,
    pub section:       String,
    pub polarized:     u32,
    pub pet_name:      Option<String>,
    pub focus_lens:    Option<String>,
    pub archon_shards: Vec<ArchonShard>,
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

/// Convert raw affinity XP to item rank (0–30).
/// Formula from Warframe wiki: cumulative XP to reach rank N is 1000×N² for
/// Warframes/Sentinels/companions, 500×N² for all weapon types.
/// Invert: rank = floor(sqrt(xp / base)).
pub fn xp_to_rank(xp: i64, path: &str) -> u32 {
    let base = if path.contains("/Powersuits/")
        || path.contains("/SentinelPowersuits/")
        || path.contains("/Types/Friendly/")
        || path.contains("/Types/Game/KubrowPet/")
        || path.contains("/Types/Game/CatbrowPet/")
    { 1000.0f64 } else { 500.0f64 };
    ((xp as f64 / base).sqrt().floor() as u32).min(30)
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
    for next in memmem::find_iter(data, id_key) {
        let id_start = next + id_key.len();
        // accountId is exactly 24 lowercase hex chars
        let id_slice = &data[id_start..id_start.saturating_add(26).min(data.len())];
        let close = id_slice.iter().position(|&b| b == b'"').unwrap_or(0);
        if close != 24 { continue; }
        let id_bytes = &id_slice[..24];
        if !id_bytes.iter().all(|&b| b.is_ascii_hexdigit()) { continue; }
        let account_id = std::str::from_utf8(id_bytes).unwrap_or("").to_string();

        // Look for Nonce within 2048 bytes
        let nonce_search_end = (id_start + 2048).min(data.len());
        if let Some(rel) = memmem::find(&data[id_start..nonce_search_end], nonce_key) {
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
    for next in memmem::find_iter(data, ak) {
        let id_start = next + ak.len();
        let id_end = data[id_start..].iter().position(|&b| !b.is_ascii_hexdigit()).map(|p| id_start + p).unwrap_or(data.len());
        if id_end - id_start != 24 { continue; }
        let account_id = std::str::from_utf8(&data[id_start..id_end]).unwrap_or("").to_string();
        // Nonce can appear anywhere within 512 bytes after the accountId
        let nonce_search_end = (id_end + 512).min(data.len());
        if let Some(rel) = memmem::find(&data[id_end..nonce_search_end], nk) {
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
    for next in memmem::find_iter(data, key) {
        let id_start = next + key.len();
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

// ─── Full-account blob parser ─────────────────────────────────────────────────

/// Find the end of the FULL_ACCOUNT blob by locating `"DeathSquadable":` and
/// the `}` that immediately follows its boolean value (true or false).
fn find_blob_end(raw: &[u8]) -> Option<usize> {
    const KEY: &[u8] = b"\"DeathSquadable\":";
    let key_pos = memmem::find(raw, KEY)?;
    let after   = key_pos + KEY.len();
    // Skip the boolean value and find the closing brace
    let brace = memchr::memchr(b'}', &raw[after..])?;
    Some(after + brace + 1)
}

const START_MARKER: &[u8] = b"\"SubscribedToEmails\"";
const ALT_STARTS: &[&[u8]] = &[
    b"\"MiscItems\":[{\"ItemType\":\"/Lotus/",
    b"\"Suits\":[{\"ItemType\":\"/Lotus/",
    b"\"RegularCredits\":",
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

/// Where a stitched buffer's blob begins: `(marker_at, seed_at)`.
///
/// `marker_at` is the marker occurrence the seed is anchored to; `seed_at` is
/// the brace enclosing it, or the marker itself when no occurrence has a
/// brace that reads as an object head. See `enclosing_object_start` for why
/// marker and brace are rarely the same offset.
///
/// With no primary marker anywhere the fallbacks keep both offsets in bounds,
/// down to the buffer's last byte when nothing matches at all. Such a seed is
/// junk and fails to parse, which is what the walk already does with a bad
/// start.
fn blob_seed_offsets(combined: &[u8]) -> (usize, usize) {
    // The heap can hold a stale copy of the blob ahead of the live one: its
    // marker survives but its opening brace was overwritten, so brace-matching
    // backward from it lands on a stray `{` in binary garbage. A live seed is
    // a JSON object head, so only accept a brace followed by a quote, and keep
    // trying later marker occurrences until one qualifies.
    let mut first_marker = None;
    let mut search_from = 0;
    while let Some(found) = memmem::find(&combined[search_from..], START_MARKER) {
        let marker_at = search_from + found;
        first_marker.get_or_insert(marker_at);
        if let Some(open) = enclosing_object_start(combined, marker_at) {
            if combined.get(open + 1) == Some(&b'"') {
                return (marker_at, open);
            }
        }
        search_from = marker_at + 1;
    }
    // Without a plausible brace anywhere, seed at the first marker: the
    // parser can rebuild the object head from a marker-anchored seed.
    let marker_at = first_marker
        .or_else(|| ALT_STARTS.iter().find_map(|a| memmem::find(combined, a)))
        .unwrap_or(combined.len().saturating_sub(1));
    (marker_at, first_marker.unwrap_or_else(||
        enclosing_object_start(combined, marker_at).unwrap_or(marker_at)))
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
    sorted.sort_by(|a, b| b.value.cmp(&a.value));
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
    extract_blob_json_ref(raw).map(Cow::into_owned)
}

/// Borrowing counterpart of [`extract_blob_json`]. The common case (buffer still
/// starts with the original `{`) needs no copy at all; only the fallback path,
/// where the opening brace was overwritten in memory and has to be reinstated,
/// allocates.
pub fn extract_blob_json_ref(raw: &[u8]) -> Option<Cow<'_, [u8]>> {
    let end_pos = find_blob_end(raw)?;
    extract_blob_json_at(raw, end_pos)
}

/// Same as [`extract_blob_json_ref`] but takes an already-known `end_pos`, so
/// callers that located the blob end for their own purposes (e.g. the
/// minimum-size check in [`parse_full_account_blob`]) don't pay for a second scan.
fn extract_blob_json_at(raw: &[u8], end_pos: usize) -> Option<Cow<'_, [u8]>> {
    if raw.first() == Some(&b'{') {
        Some(Cow::Borrowed(&raw[..end_pos]))
    } else {
        let start_pos = memmem::find(raw, START_MARKER)?;
        let mut v = Vec::with_capacity(end_pos - start_pos + 1);
        v.push(b'{');
        v.extend_from_slice(&raw[start_pos..end_pos]);
        Some(Cow::Owned(v))
    }
}

/// `raw` must span from the JSON opening `{` (or from `"SubscribedToEmails"`) through
/// `"DeathSquadable":`. Returns `None` if neither start can be located or JSON is malformed.
pub fn parse_full_account_blob(raw: &[u8]) -> Option<BlobInventory> {
    let end_pos = find_blob_end(raw)?;

    // Real FULL_ACCOUNT blobs are several hundred KB — anything smaller is a
    // small false-positive fragment that matched the end marker by coincidence.
    const MIN_PARSE_BYTES: usize = 50_000;
    if end_pos < MIN_PARSE_BYTES {
        eprintln!("[blob-parse] too small ({} B < {} B) — skipping", end_pos, MIN_PARSE_BYTES);
        return None;
    }

    let json_bytes = extract_blob_json_at(raw, end_pos)?;

    let json: serde_json::Value = serde_json::from_slice(&json_bytes)
        .map_err(|e| {
            let head: String = json_bytes[..json_bytes.len().min(48)]
                .iter().map(|&b| if b >= 0x20 && b < 0x7f { b as char } else { '.' }).collect();
            eprintln!("[blob-parse] JSON error: {} | head: {:?}", e, head);
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
                unique_items.push(BlobUniqueEntry {
                    item_type:     it.to_string(),
                    section:       sec.to_string(),
                    polarized:     e["Polarized"].as_u64().unwrap_or(0) as u32,
                    pet_name:      e["Details"]["Name"].as_str().map(String::from),
                    focus_lens:    e["FocusLens"].as_str().map(String::from),
                    archon_shards,
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
            // Duplicate ItemTypes dominate this map, so check get_mut before
            // paying for entry()'s unconditional to_string() allocation.
            let mc = match mods.get_mut(it) {
                Some(mc) => mc,
                None => mods.entry(it.to_string()).or_default(),
            };
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
            let mc = match mods.get_mut(it) {
                Some(mc) => mc,
                None => mods.entry(it.to_string()).or_default(),
            };
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
            match flavour_items.get_mut(it) {
                Some(v) => *v += 1,
                None => { flavour_items.insert(it.to_string(), 1); }
            }
        }
    }

    // WeaponSkins (sigils, cosmetic skins): each array entry = one owned copy,
    // count occurrences of the same ItemType.
    let mut weapon_skins: HashMap<String, i64> = HashMap::new();
    if let Some(arr) = json["WeaponSkins"].as_array() {
        for e in arr {
            let Some(it) = e["ItemType"].as_str() else { continue };
            if !it.starts_with("/Lotus/") { continue; }
            match weapon_skins.get_mut(it) {
                Some(v) => *v += 1,
                None => { weapon_skins.insert(it.to_string(), 1); }
            }
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

    Some(BlobInventory {
        credits, endo, platinum, free_platinum, mastery_level,
        unique_items, stackable_items, mods,
        flavour_items, weapon_skins, mastery_data, pending_recipes, consumed_suits,
        rivens,
    })
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
// On the next cycle we probe that address first — if the blob is still there we
// finish in milliseconds instead of walking the full address space.
static LAST_BLOB_REGION: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

// Digest of the last blob whose bytes were about to be parsed. Inventory
// changes maybe once per mission, so most 10s scan cycles find byte-identical
// JSON — hashing a few MB is far cheaper than rebuilding BlobInventory's
// HashMaps/Vecs from scratch every cycle.
static LAST_BLOB_DIGEST: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

#[cfg(target_os = "windows")]
enum CachedBlobScan {
    Fresh(usize, BlobInventory),
    Unchanged,
}

/// Set once the probe has reported that nothing changed, cleared as soon as
/// anything does. Probes run every couple of seconds and nearly all of them
/// find byte-identical JSON, so logging each one drowns out the rest of the
/// log. Only the transition into that state is logged.
static STEADY_STATE_LOGGED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

fn steady_state_notice_due() -> bool {
    !STEADY_STATE_LOGGED.swap(true, std::sync::atomic::Ordering::Relaxed)
}

/// Whether a previous scan left an address for the probe to re-read. Without
/// one the probe can only ever miss, so the caller must walk instead.
pub fn has_cached_blob() -> bool {
    LAST_BLOB_REGION.load(std::sync::atomic::Ordering::Relaxed) != 0
}

/// Clear the fast-path region cache. Call when Warframe's PID changes so the
/// next scan doesn't probe a stale address from the previous process instance.
pub fn reset_last_blob_region() {
    LAST_BLOB_REGION.store(0, std::sync::atomic::Ordering::Relaxed);
    LAST_BLOB_DIGEST.store(0, std::sync::atomic::Ordering::Relaxed);
    STEADY_STATE_LOGGED.store(false, std::sync::atomic::Ordering::Relaxed);
}

/// Discard the digest baseline so the next candidate is parsed no matter what
/// its bytes are. Call after a parse failure: skipping a re-parse is only safe
/// while the baseline names bytes that are known to parse, and `blob_unchanged`
/// records its argument before the parse outcome is known.
fn forget_blob_digest() {
    LAST_BLOB_DIGEST.store(0, std::sync::atomic::Ordering::Relaxed);
}

/// Checks `json` against the digest recorded by the previous call, then
/// records `json`'s digest as the new baseline — check-and-update in one
/// step, so a caller should invoke this exactly once per candidate blob,
/// right before deciding whether to parse it.
///
/// A caller that then fails to parse `json` must call `forget_blob_digest`:
/// the skip paths treat a match as "already parsed this successfully", and
/// unparseable bytes that persist across cycles would otherwise be mistaken
/// for a result and suppress the rest of the walk indefinitely.
///
/// Returns true when `json` is byte-identical to what the previous call saw.
fn blob_unchanged(json: &[u8]) -> bool {
    use std::hash::{DefaultHasher, Hash, Hasher};
    // Callers hand over a stitched buffer, not a trimmed blob. The stitch stops
    // at the region that closes the JSON, so everything past the closing brace
    // is whatever heap shared that region, tens of megabytes of it, rewritten
    // constantly by a running client. Hash that tail and the digest never
    // matches, so every scan reparses and the skip never happens. Bytes with no
    // blob end are hashed whole; they only need to compare equal to themselves.
    let blob = &json[..find_blob_end(json).unwrap_or(json.len())];
    let mut hasher = DefaultHasher::new();
    blob.hash(&mut hasher);
    // OR in a set bit so a hashed digest can never equal the 0 sentinel that
    // reset_last_blob_region stores — that sentinel must always compare as
    // "changed" to force a re-parse after a PID change.
    let digest = hasher.finish() | 1;
    let unchanged = LAST_BLOB_DIGEST.swap(digest, std::sync::atomic::Ordering::Relaxed) == digest;
    if !unchanged {
        // Bytes moved, so the next settle into the steady state is worth
        // saying out loud again.
        STEADY_STATE_LOGGED.store(false, std::sync::atomic::Ordering::Relaxed);
    }
    unchanged
}

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
// Shared by the cold walk and the cached-region fast path, so a region
// rejected as a mission delta or a scan dropped for growing past the cap
// means the same thing in either place.
#[cfg(target_os = "windows")]
const MAX_READ: usize = 64 * 1024 * 1024;
#[cfg(target_os = "windows")]
const MAX_SCAN: usize = 20 * 1024 * 1024;
#[cfg(target_os = "windows")]
const MISSION_DELTA: &[u8] = b"\"InventoryChanges\":";
#[cfg(target_os = "windows")]
const LOTUS_KEY: &[u8] = b"/Lotus/";
#[cfg(target_os = "windows")]
const ANCHORS: &[&[u8]] = &[
    b"\"SubscribedToEmails\"",
    b"\"MiscItems\":[",
    b"\"Suits\":[",
    b"\"LongGuns\":[",
    b"\"Melee\":[",
    b"\"Pistols\":[",
];

/// Re-read the blob straight from the address the last successful scan found
/// it at, stitching forward through following regions until the JSON closes.
///
/// Returns `None` whenever anything looks different from last time, which puts
/// the caller back on the full walk rather than reporting a stale inventory.
#[cfg(target_os = "windows")]
fn scan_windows_cached_blob(process: windows_sys::Win32::Foundation::HANDLE) -> Option<CachedBlobScan> {
    use std::ffi::c_void;
    use std::mem;
    use windows_sys::Win32::System::{
        Diagnostics::Debug::ReadProcessMemory,
        Memory::{VirtualQueryEx, MEMORY_BASIC_INFORMATION, MEM_COMMIT, PAGE_GUARD, PAGE_NOACCESS},
    };

    let cached_addr = LAST_BLOB_REGION.load(std::sync::atomic::Ordering::Relaxed) as usize;
    if cached_addr == 0 {
        return None;
    }

    let mut mbi = unsafe { mem::zeroed::<MEMORY_BASIC_INFORMATION>() };
    let ok = unsafe { VirtualQueryEx(process, cached_addr as *const c_void, &mut mbi,
        mem::size_of::<MEMORY_BASIC_INFORMATION>()) } != 0;
    if !ok || mbi.State != MEM_COMMIT
        || mbi.Protect & PAGE_GUARD != 0
        || mbi.Protect & PAGE_NOACCESS != 0
    {
        return None;
    }

    let read_cap = mbi.RegionSize.min(MAX_READ);
    let mut buf = vec![0u8; read_cap];
    let mut n = 0usize;
    let read_ok = unsafe { ReadProcessMemory(process, cached_addr as *const c_void,
        buf.as_mut_ptr() as *mut c_void, read_cap, &mut n) } != 0 && n >= 8;
    if !read_ok {
        return None;
    }

    buf.truncate(n);
    let chunk = &buf[..];
    let is_mission = memmem::find(chunk, MISSION_DELTA).is_some();
    let has_anchor = ANCHORS.iter().any(|a| memmem::find(chunk, a).is_some());
    let has_lotus  = memmem::find(chunk, LOTUS_KEY).is_some();
    // cached_addr is the exact byte of the blob's outer {, so seed from byte 0.
    // Accept regions that are blob data even when SubscribedToEmails is in a
    // later region (field order varies by account).
    if is_mission || !(has_anchor || has_lotus) || !chunk.starts_with(b"{\"") {
        return None;
    }

    let mut stitched = buf;
    let mut walk = cached_addr + n;
    while stitched.len() < MAX_SCAN && find_blob_end(&stitched).is_none() {
        let mut nmbi = unsafe { mem::zeroed::<MEMORY_BASIC_INFORMATION>() };
        if unsafe { VirtualQueryEx(process, walk as *const c_void, &mut nmbi,
            mem::size_of::<MEMORY_BASIC_INFORMATION>()) } == 0 { break; }
        let nr = nmbi.BaseAddress as usize;
        let ns = nmbi.RegionSize;
        walk = nr + ns;
        if nmbi.State != MEM_COMMIT
            || nmbi.Protect & PAGE_GUARD != 0
            || nmbi.Protect & PAGE_NOACCESS != 0
            || ns == 0 { continue; }
        let cap = ns.min(MAX_READ);
        let mut nb = vec![0u8; cap];
        let mut nn = 0usize;
        if unsafe { ReadProcessMemory(process, nr as *const c_void,
            nb.as_mut_ptr() as *mut c_void, cap, &mut nn) } == 0 { continue; }
        stitched.extend_from_slice(&nb[..nn]);
    }

    if blob_unchanged(&stitched) {
        if steady_state_notice_due() {
            eprintln!("[blob] fast-path hit at 0x{cached_addr:012x}: unchanged since last scan, quiet until it changes");
        }
        return Some(CachedBlobScan::Unchanged);
    }
    match parse_full_account_blob(&stitched) {
        Some(inventory) => Some(CachedBlobScan::Fresh(cached_addr, inventory)),
        None => {
            forget_blob_digest();
            None
        }
    }
}

/// When `save=true` also writes the raw text to `blob_dir` for debugging.
/// Returns the number of files written (always 0 when `save=false`).
#[cfg(target_os = "windows")]
pub fn capture_all_blobs(blob_dir: &std::path::Path, ts: &str, blob_tx: std::sync::mpsc::Sender<BlobInventory>, save: bool) -> usize {
    use std::ffi::c_void;
    use std::mem;
    use windows_sys::Win32::{
        Foundation::{CloseHandle, FALSE},
        System::{
            Diagnostics::Debug::ReadProcessMemory,
            Memory::{VirtualQueryEx, MEMORY_BASIC_INFORMATION, MEM_COMMIT, MEM_IMAGE, PAGE_GUARD, PAGE_NOACCESS},
            Threading::{OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ},
        },
    };

    let pid = match find_warframe_pid_pub() { Some(p) => p, None => return 0 };
    let process = unsafe { OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, FALSE, pid) };
    if process == 0 { return 0; }

    const MIN_REGION:    usize = 64_000;   // skip regions smaller than 64 KB
    const MAX_BLOBS:     usize = 25;

    // Executable pages never contain heap data — safe to skip.
    const PAGE_EXECUTE:      u32 = 0x10;
    const PAGE_EXECUTE_READ: u32 = 0x20;
    const PAGE_EXECUTE_RW:   u32 = 0x40;
    const PAGE_EXECUTE_WC:   u32 = 0x80;
    const EXEC_MASK: u32 = PAGE_EXECUTE | PAGE_EXECUTE_READ | PAGE_EXECUTE_RW | PAGE_EXECUTE_WC;

    // No fast path here on purpose: the monitor already ran that scan in
    // `probe_tick` and escalated to this walk on the result, so repeating it
    // would answer the same and skip the walk it asked for.
    struct ActiveScan {
        data: Vec<u8>,
        id: usize,
        /// Base address of the region where this scan was seeded (JSON start).
        /// Used to update LAST_BLOB_REGION correctly for multi-region blobs.
        start_region_addr: usize,
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

    let mut addr: usize = 0;
    let mut saved = 0usize;
    let mut regions_skipped = 0usize;
    let mut regions_read    = 0usize;
    let mut starts_found    = 0usize;
    let mut t_vquery = std::time::Duration::ZERO;
    let mut t_read   = std::time::Duration::ZERO;
    let mut t_search = std::time::Duration::ZERO;
    let mut bytes_read: u64 = 0;
    // Once we have at least one successful parse we stop opening new scans.
    // Active scans already in progress are still stitched to completion (or dropped).
    // The loop exits as soon as all active scans are gone.
    let mut found_result = false;

    loop {
        if saved >= MAX_BLOBS { break; }
        // Early exit: we have a result and no active scans left to finish.
        if found_result && scans.is_empty() && !save { break; }

        let t0 = std::time::Instant::now();
        let mut mbi = unsafe { mem::zeroed::<MEMORY_BASIC_INFORMATION>() };
        if unsafe { VirtualQueryEx(process, addr as *const c_void, &mut mbi,
            mem::size_of::<MEMORY_BASIC_INFORMATION>()) } == 0 { break; }
        t_vquery += t0.elapsed();

        let region_addr = mbi.BaseAddress as usize;
        let region_size = mbi.RegionSize;
        let next_addr   = region_addr.saturating_add(region_size);
        if next_addr <= addr { break; }
        addr = next_addr;

        // ── Region filters ──────────────────────────────────────────────────
        // Skip pages that can never hold heap JSON:
        // • must be committed and readable
        // • skip execute-only pages (code sections, JIT stubs)
        // • skip PE image sections — those hold string constants in the exe/DLLs,
        //   not live heap data; they false-trigger the Lotus anchor check and
        //   cost ~40 s scanning 20 MB+ without ever finding the blob end
        // • skip anything smaller than MIN_REGION
        if mbi.State   != MEM_COMMIT
            || mbi.Protect &  PAGE_GUARD    != 0
            || mbi.Protect &  PAGE_NOACCESS != 0
            || mbi.Protect &  EXEC_MASK     != 0
            || mbi.Type    == MEM_IMAGE
            || region_size  < MIN_REGION
        { regions_skipped += 1; continue; }

        let read_cap = region_size.min(MAX_READ);

        let t1 = std::time::Instant::now();
        let mut buf = vec![0u8; read_cap];
        let mut n = 0usize;
        if unsafe { ReadProcessMemory(process, region_addr as *const c_void,
            buf.as_mut_ptr() as *mut c_void, read_cap, &mut n) } == 0 || n < 8 {
            regions_skipped += 1; continue;
        }
        t_read += t1.elapsed();
        bytes_read += n as u64;
        let chunk = &buf[..n];
        regions_read += 1;

        // ── Step 1: append this chunk to every active scan and check for completion ──
        // search_from tracks where we left off so we only scan newly-appended bytes
        // (plus a small overlap for markers that straddle a region boundary).
        const END_MARKER: &[u8] = b"\"DeathSquadable\":";
        scans.retain_mut(|scan| {
            // A previous scan in this same retain_mut pass already succeeded.
            // Drop this one immediately — applying a second blob overwrites correct data
            // with a stale/parallel copy from a different memory region.
            if found_result && !save { return false; }
            // Advance the search cursor before appending so the overlap catches split markers.
            let search_from = scan.search_from;
            scan.search_from = scan.data.len().saturating_sub(END_MARKER.len() - 1);
            scan.data.extend_from_slice(chunk);
            if scan.data.len() > MAX_SCAN {
                eprintln!("[blob] scan#{} exceeded {} MB without end — dropped", scan.id, MAX_SCAN / 1024 / 1024);
                return false; // drop oversized scan
            }
            // Only search the newly-added window, not the full buffer.
            let has_end = memmem::find(&scan.data[search_from..], END_MARKER).is_some();
            if has_end && find_blob_end(&scan.data).is_some() {
                if !save && blob_unchanged(&scan.data) {
                    eprintln!("[blob] scan#{} unchanged since last scan — skipping parse", scan.id);
                    LAST_BLOB_REGION.store(scan.start_region_addr as u64, std::sync::atomic::Ordering::Relaxed);
                    found_result = true;
                    return false;
                }
                match parse_full_account_blob(&scan.data) {
                    Some(inv) => {
                        eprintln!("[blob] scan#{} SUCCESS at 0x{:012x}: {} unique, {} stackable, {} mods",
                            scan.id, region_addr, inv.unique_items.len(), inv.stackable_items.len(), inv.mods.len());
                        // Cache the START region (not this region) so the fast path works next cycle.
                        LAST_BLOB_REGION.store(scan.start_region_addr as u64, std::sync::atomic::Ordering::Relaxed);
                        if save {
                            let name = format!("Actual_inventory_FULL_ACCOUNT_{}_{:02}.txt", ts, saved + 1);
                            let path = blob_dir.join(&name);
                            if let Some(json) = extract_blob_json(&scan.data) {
                                if std::fs::write(&path, &json).is_ok() { saved += 1; }
                            }
                        }
                        blob_tx.send(inv).ok();
                        found_result = true;
                    }
                    None => {
                        eprintln!("[blob] scan#{} end marker found but JSON parse failed — dropped", scan.id);
                        forget_blob_digest();
                    }
                }
                false // remove completed (or failed) scan
            } else {
                true // keep waiting for end
            }
        });

        // ── Step 2: check if this chunk opens a new scan ──
        // Don't open new scans once we already have a result — drain the active ones then exit.
        if found_result { continue; }

        let t2 = std::time::Instant::now();
        let has_start     = memmem::find(chunk, START_MARKER).is_some();
        let has_alt_start = ALT_STARTS.iter().any(|a| memmem::find(chunk, a).is_some());
        let is_mission    = memmem::find(chunk, MISSION_DELTA).is_some();
        let has_anchor    = ANCHORS.iter().any(|a| memmem::find(chunk, a).is_some());
        let has_lotus     = memmem::find(chunk, LOTUS_KEY).is_some();
        let qualifies     = (has_start || has_alt_start) && !is_mission && (has_anchor || has_lotus);
        t_search += t2.elapsed();

        // Accumulate regions with Lotus paths (no SubscribedToEmails, no mission delta) into
        // a pre-buffer.  When SubscribedToEmails is found in a later region, we prepend
        // contiguous pre-buffer regions so that the backward {"  search finds the true
        // outermost JSON opening rather than a nested {"$oid":…} inside the blob.
        if !has_start && !is_mission && (has_anchor || has_lotus) {
            while pre_buf.iter().map(|p| p.data.len()).sum::<usize>() + n > PRE_BUF_BYTES
                && !pre_buf.is_empty()
            {
                pre_buf.pop_front();
            }
            pre_buf.push_back(PreChunk { addr: region_addr, end_addr: region_addr + n, data: chunk.to_vec() });
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
            eprintln!(
                "[blob] scan#{} started at 0x{:012x}+{} (json_open={} seed=0x{:012x} pre={}B)",
                id, region_addr, start_off, json_open, seed_addr, pre_bytes
            );
            let seed = combined[json_open..].to_vec();

            let seed_ends = find_blob_end(&seed).is_some();
            if seed_ends && !save && blob_unchanged(&seed) {
                eprintln!("[blob] scan#{} immediate hit: unchanged since last scan — skipping parse", id);
                LAST_BLOB_REGION.store(seed_addr as u64, std::sync::atomic::Ordering::Relaxed);
                found_result = true;
            } else if seed_ends {
                match parse_full_account_blob(&seed) {
                    Some(inv) => {
                        eprintln!("[blob] scan#{} immediate SUCCESS at 0x{:012x}: {} unique, {} stackable",
                            id, region_addr, inv.unique_items.len(), inv.stackable_items.len());
                        LAST_BLOB_REGION.store(seed_addr as u64, std::sync::atomic::Ordering::Relaxed);
                        if save {
                            let name = format!("Actual_inventory_FULL_ACCOUNT_{}_{:02}.txt", ts, saved + 1);
                            if let Some(json) = extract_blob_json(&seed) {
                                if std::fs::write(blob_dir.join(&name), &json).is_ok() { saved += 1; }
                            }
                        }
                        blob_tx.send(inv).ok();
                        found_result = true;
                    }
                    None => {
                        eprintln!("[blob] scan#{} immediate end found but parse failed — dropping", id);
                        forget_blob_digest();
                    }
                }
            } else {
                scans.push(ActiveScan { data: seed, id, start_region_addr: seed_addr, search_from: 0 });
            }
        }
    }

    eprintln!(
        "[blob-capture] done: read={} skipped={} starts={} saved={} bytes={}MB | \
         vquery={:.0}ms read={:.0}ms search={:.0}ms",
        regions_read, regions_skipped, starts_found, saved, bytes_read / 1_000_000,
        t_vquery.as_secs_f64() * 1000.0,
        t_read.as_secs_f64()   * 1000.0,
        t_search.as_secs_f64() * 1000.0,
    );
    if starts_found == 0 {
        eprintln!("[blob-capture] WARNING: no start-marker found — FULL_ACCOUNT not in memory \
            (game in mission, on login screen, or Arsenal not open?)");
    }
    unsafe { CloseHandle(process); }
    saved
}

#[cfg(not(target_os = "windows"))]
pub fn capture_all_blobs(_blob_dir: &std::path::Path, _ts: &str, _blob_tx: std::sync::mpsc::Sender<BlobInventory>, _save: bool) -> usize { 0 }

// ─── Cheap probe ──────────────────────────────────────────────────────────────

/// What a probe of the cached blob address concluded.
///
/// `Unchanged` and `Updated` are definitive answers obtained for a few
/// megabytes of reads. `CacheMiss` is not: the game may have reallocated the
/// blob because the inventory changed, or the address may be stale for some
/// unrelated reason, and telling those apart costs a full region walk.
#[derive(Debug, PartialEq, Eq)]
pub enum ScanOutcome {
    Unchanged,
    Updated,
    CacheMiss,
}

/// Map a cached-region scan onto a probe outcome, sending any fresh inventory.
#[cfg(target_os = "windows")]
fn probe_outcome(
    scan: Option<CachedBlobScan>,
    blob_tx: &std::sync::mpsc::Sender<BlobInventory>,
) -> ScanOutcome {
    match scan {
        Some(CachedBlobScan::Fresh(address, inventory)) => {
            eprintln!("[blob] probe hit at 0x{address:012x}: {} unique, {} stackable",
                inventory.unique_items.len(), inventory.stackable_items.len());
            blob_tx.send(inventory).ok();
            ScanOutcome::Updated
        }
        Some(CachedBlobScan::Unchanged) => ScanOutcome::Unchanged,
        None => ScanOutcome::CacheMiss,
    }
}

/// One monitor tick: re-read the blob from its remembered address, and check
/// whether the game has logged an inventory sync since the last tick.
///
/// Never falls back to a full region walk. `capture_all_blobs` does that, which
/// makes it unusable as a poll: probing at 1-2 Hz would mean walking memory at
/// 1-2 Hz for as long as the cached address stays stale. Splitting the two lets
/// the caller poll cheaply and decide for itself when a miss is worth the walk.
///
/// The marker is read first and every tick, because it is what tells the blob
/// scan it has something to look at. The scan itself runs only when `force` or
/// that marker says so; between syncs it can only ever conclude that nothing
/// moved. `None` means it was not scanned this tick, which is not the same as
/// a miss.
#[cfg(target_os = "windows")]
pub fn probe_tick(
    pid: u32,
    blob_tx: std::sync::mpsc::Sender<BlobInventory>,
    force: bool,
) -> (Option<ScanOutcome>, bool) {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, FALSE},
        System::Threading::{OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ},
    };

    let process = unsafe { OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, FALSE, pid) };
    if process == 0 {
        return (None, false);
    }
    let sync = sync_marker_is_new(windows_newest_sync_timestamp(process));
    let outcome = (force || sync)
        .then(|| probe_outcome(scan_windows_cached_blob(process), &blob_tx));
    unsafe { CloseHandle(process) };
    (outcome, sync)
}

#[cfg(not(target_os = "windows"))]
pub fn probe_tick(
    _pid: u32,
    _blob_tx: std::sync::mpsc::Sender<BlobInventory>,
    _force: bool,
) -> (Option<ScanOutcome>, bool) {
    (None, false)
}

// ─── Inventory-sync marker, read from memory rather than from EE.log ──────────
//
// Warframe composes its log lines in process memory long before they reach
// EE.log: the game buffers writes and flushes in bursts, and sampling the live
// client showed the newest in-memory line running 23 s ahead of the newest line
// on disk. Tailing the file therefore reports an inventory sync at an unknown,
// variable delay, and that delay lands on every capture gated behind it.
//
// The formatted lines are findable by content, so no pointer chain and no
// per-build offsets are involved:
//
//   19761.848 Sys [Info]: OnInventoryResults completed in 339ms
//
// Only the log text holds ` Sys [Info]: ` preceded by a seconds-since-launch
// timestamp, and once the buffer is found its address caches like
// LAST_BLOB_REGION.

/// The formatted marker. Shared with the EE.log tail in `start_log_watcher`
/// rather than re-spelled: a mismatch between the two readers degrades to
/// plain interval polling, which is hard to tell from working correctly.
pub const INVENTORY_SYNC_MARKER: &str = "OnInventoryResults completed in";

const SYNC_MARKER: &[u8] = INVENTORY_SYNC_MARKER.as_bytes();

/// Present on every log line, so it identifies the buffer regardless of what
/// the game happens to have logged recently.
const LOG_LINE_MARKER: &[u8] = b" Sys [Info]: ";

/// The candidate buffers are a few MB against several GB of readable mappings,
/// so anything larger is some other allocation that happens to quote a log line.
const MAX_LOG_REGION: usize = 16 * 1024 * 1024;

static LAST_LOG_REGION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Probes still to skip before the cold search may run again.
static LOG_SEARCH_BACKOFF: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Whether the cold search is allowed to run on this probe. That search reads
/// every non-executable region under [`MAX_LOG_REGION`], hundreds of MB.
///
/// Without this, a client whose log buffers cannot be located pays a walk-sized
/// read on the monitor thread every couple of seconds for the whole session.
/// Backing off costs only latency: the marker is an optimisation, and the
/// EE.log tail reports the same syncs meanwhile.
fn cold_log_search_due() -> bool {
    use std::sync::atomic::Ordering::Relaxed;
    LOG_SEARCH_BACKOFF
        .fetch_update(Relaxed, Relaxed, |left| Some(left.saturating_sub(1)))
        .is_ok_and(|left| left == 0)
}

/// Probes to sit out after a failed cold search, at the monitor's 2 s cadence.
const LOG_SEARCH_BACKOFF_PROBES: u64 = 30;

/// Game timestamp of the newest sync marker already reported, as `f64` bits.
static LAST_SYNC_TIMESTAMP: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Forget the log buffer's address and the marker baseline. Call alongside
/// [`reset_last_blob_region`] when the PID changes: the timestamps are seconds
/// since *that* client launched, so a baseline from the previous process would
/// swallow every marker the new one writes.
pub fn reset_log_region() {
    LAST_LOG_REGION.store(0, std::sync::atomic::Ordering::Relaxed);
    LAST_SYNC_TIMESTAMP.store(0, std::sync::atomic::Ordering::Relaxed);
    LOG_SEARCH_BACKOFF.store(0, std::sync::atomic::Ordering::Relaxed);
}

/// Seconds-since-launch stamp opening the line that `offset` falls inside, e.g.
/// `19761.848` from `19761.848 Sys [Info]: …`.
///
/// Both buffers hold complete formatted lines but end them differently: the
/// pending file-write buffer uses CRLF, the heap ring LF, so the search back
/// to the line start stops at either.
fn line_timestamp(chunk: &[u8], offset: usize) -> Option<f64> {
    let start = chunk[..offset]
        .iter()
        .rposition(|&byte| byte == b'\n' || byte == b'\r')
        .map_or(0, |index| index + 1);
    // `offset` lands on the space opening ` Sys [Info]: ` for one caller and
    // partway into the message for the other, so the stamp runs to whichever
    // comes first: the next space, or the marker itself.
    let line = &chunk[start..offset];
    let end = line.iter().position(|&byte| byte == b' ').unwrap_or(line.len());
    let stamp = std::str::from_utf8(&line[..end]).ok()?;
    // Reject anything that is not the timestamp shape: a bare integer or a
    // stray word would otherwise parse and then compare as a valid ordering.
    let (seconds, millis) = stamp.split_once('.')?;
    if seconds.is_empty()
        || !seconds.bytes().all(|byte| byte.is_ascii_digit())
        || millis.len() != 3
        || !millis.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    stamp.parse().ok()
}

/// True when `chunk` holds formatted log lines rather than, say, the `.rdata`
/// copy of the format string. The timestamp is what tells the two apart.
fn looks_like_log_buffer(chunk: &[u8]) -> bool {
    let mut from = 0;
    // A handful of probes is enough: the log text is dense with these, so a
    // buffer that fails several in a row is not it.
    for _ in 0..8 {
        let Some(hit) = memmem::find(&chunk[from..], LOG_LINE_MARKER) else { return false };
        let offset = from + hit;
        if line_timestamp(chunk, offset).is_some() {
            return true;
        }
        from = offset + LOG_LINE_MARKER.len();
    }
    false
}

/// Newest game timestamp among the sync markers in `chunk`.
///
/// Every match is examined rather than just the last, because the heap ring
/// wraps: the newest line is not necessarily the one at the highest address.
fn newest_sync_timestamp(chunk: &[u8]) -> Option<f64> {
    let mut newest: Option<f64> = None;
    let mut from = 0;
    while let Some(hit) = memmem::find(&chunk[from..], SYNC_MARKER) {
        let offset = from + hit;
        if let Some(stamp) = line_timestamp(chunk, offset) {
            newest = Some(newest.map_or(stamp, |best: f64| best.max(stamp)));
        }
        from = offset + SYNC_MARKER.len();
    }
    newest
}

/// Fold a freshly-observed marker timestamp into the baseline, reporting
/// whether it names a sync that has not been reported yet.
///
/// Any difference from the baseline counts, in both directions. The stamps are
/// seconds since the client launched, so the only way they run backwards is a
/// game restart, and the sync logged just after one is the login sync that
/// populates the inventory.
///
/// The first observation counts too, rather than being spent establishing a
/// baseline. A buffer that already holds markers at app start is reporting
/// history, but reporting it costs nothing, because the first capture walks
/// memory unconditionally and nothing reads the marker on that tick. Spending
/// the first observation would instead swallow the login sync after every
/// restart, since the PID change clears the baseline right before it arrives.
fn sync_marker_is_new(newest: Option<f64>) -> bool {
    let Some(newest) = newest else { return false };
    let previous = f64::from_bits(LAST_SYNC_TIMESTAMP.swap(newest.to_bits(), std::sync::atomic::Ordering::Relaxed));
    let is_new = newest != previous;
    if is_new {
        // Four in a 40-minute session, and the walk policy keys off them, so
        // they are logged rather than left to be inferred from the walks.
        eprintln!("[sync] inventory sync marker at {newest:.3}s");
    }
    is_new
}

/// Newest sync-marker timestamp currently in the game's log buffers, probing
/// the remembered region first and searching for it again when that fails.
#[cfg(target_os = "windows")]
fn windows_newest_sync_timestamp(process: windows_sys::Win32::Foundation::HANDLE) -> Option<f64> {
    use std::ffi::c_void;
    use std::mem;
    use windows_sys::Win32::System::{
        Diagnostics::Debug::ReadProcessMemory,
        Memory::{VirtualQueryEx, MEMORY_BASIC_INFORMATION, MEM_COMMIT, PAGE_GUARD, PAGE_NOACCESS},
    };

    // Executable pages hold the `.rdata` copy of the format string, never a
    // formatted line, so skipping them also skips the obvious false positive.
    const EXEC_MASK: u32 = 0x10 | 0x20 | 0x40 | 0x80;

    let mut buffer = Vec::new();
    let read_region = |address: usize, size: usize, buffer: &mut Vec<u8>| -> Option<usize> {
        buffer.resize(size.min(MAX_LOG_REGION), 0);
        let mut read = 0usize;
        let ok = unsafe { ReadProcessMemory(process, address as *const c_void,
            buffer.as_mut_ptr() as *mut c_void, buffer.len(), &mut read) } != 0;
        (ok && read > LOG_LINE_MARKER.len()).then_some(read)
    };
    let query = |address: usize| -> Option<MEMORY_BASIC_INFORMATION> {
        let mut mbi = unsafe { mem::zeroed::<MEMORY_BASIC_INFORMATION>() };
        let ok = unsafe { VirtualQueryEx(process, address as *const c_void, &mut mbi,
            mem::size_of::<MEMORY_BASIC_INFORMATION>()) } != 0;
        ok.then_some(mbi)
    };
    let readable = |mbi: &MEMORY_BASIC_INFORMATION| {
        mbi.State == MEM_COMMIT
            && mbi.Protect & PAGE_GUARD == 0
            && mbi.Protect & PAGE_NOACCESS == 0
            && mbi.Protect & EXEC_MASK == 0
            && mbi.RegionSize > 0
    };

    let cached = LAST_LOG_REGION.load(std::sync::atomic::Ordering::Relaxed) as usize;
    if cached != 0 {
        if let Some(mbi) = query(cached).filter(readable).filter(|mbi| mbi.BaseAddress as usize == cached) {
            if let Some(read) = read_region(cached, mbi.RegionSize, &mut buffer) {
                if looks_like_log_buffer(&buffer[..read]) {
                    return newest_sync_timestamp(&buffer[..read]);
                }
            }
        }
        // The region is gone or holds something else now; fall through and
        // look again rather than reporting a silent nothing from here on.
        LAST_LOG_REGION.store(0, std::sync::atomic::Ordering::Relaxed);
    }

    if !cold_log_search_due() {
        return None;
    }

    // Cold search. There are two copies of the log text: the pending
    // file-write buffer and a heap ring of recent lines. Which one is
    // further ahead depends on where the game is in its flush cycle, so both
    // are read and the newer marker wins.
    let mut newest: Option<f64> = None;
    let mut found = 0;
    let mut address = 0usize;
    while let Some(mbi) = query(address) {
        let base = mbi.BaseAddress as usize;
        let size = mbi.RegionSize;
        let Some(next) = base.checked_add(size).filter(|next| *next > address) else { break };
        address = next;
        if !readable(&mbi) || size > MAX_LOG_REGION {
            continue;
        }
        let Some(read) = read_region(base, size, &mut buffer) else { continue };
        let chunk = &buffer[..read];
        if !looks_like_log_buffer(chunk) {
            continue;
        }
        if found == 0 {
            eprintln!("[sync] sync-marker buffer at 0x{base:012x} ({} KB)", read / 1000);
            LAST_LOG_REGION.store(base as u64, std::sync::atomic::Ordering::Relaxed);
        }
        if let Some(stamp) = newest_sync_timestamp(chunk) {
            newest = Some(newest.map_or(stamp, |best: f64| best.max(stamp)));
        }
        found += 1;
        if found == 2 {
            break;
        }
    }
    if found == 0 {
        eprintln!("[sync] no in-memory log buffer found; sync markers come from the EE.log tail only");
        LOG_SEARCH_BACKOFF.store(LOG_SEARCH_BACKOFF_PROBES, std::sync::atomic::Ordering::Relaxed);
    }
    newest
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

#[cfg(test)]
mod seed_tests {
    use super::{enclosing_object_start, blob_seed_offsets, extract_blob_json, extract_blob_json_ref};
    use std::borrow::Cow;

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
    fn blob_json_stops_at_the_closing_brace_of_the_object() {
        // A stitched scan buffer: the blob, then the rest of the memory region
        // it happened to end in.
        let mut raw = br#"{"SubscribedToEmails":0,"DeathSquadable":false}"#.to_vec();
        let blob_len = raw.len();
        raw.extend(std::iter::repeat(0xABu8).take(1_000_000));

        let json = extract_blob_json(&raw).expect("end marker present");
        assert_eq!(json.len(), blob_len);
        assert!(serde_json::from_slice::<serde_json::Value>(&json).is_ok());
    }

    #[test]
    fn blob_json_reinstates_the_opening_brace_when_it_was_overwritten() {
        let mut raw = br#"x"SubscribedToEmails":0,"DeathSquadable":false}"#.to_vec();
        let blob_len = raw.len();
        raw.extend(std::iter::repeat(0xABu8).take(1024));

        let json = extract_blob_json(&raw).expect("end marker present");
        assert_eq!(json.len(), blob_len);
        assert_eq!(json[0], b'{');
        assert!(serde_json::from_slice::<serde_json::Value>(&json).is_ok());
    }

    #[test]
    fn blob_json_ref_borrows_in_the_common_case_and_owns_in_the_fallback() {
        let intact = br#"{"SubscribedToEmails":0,"DeathSquadable":false}"#;
        assert!(matches!(extract_blob_json_ref(intact), Some(Cow::Borrowed(_))));

        let overwritten = br#"x"SubscribedToEmails":0,"DeathSquadable":false}"#;
        assert!(matches!(extract_blob_json_ref(overwritten), Some(Cow::Owned(_))));
    }
}

#[cfg(test)]
mod blob_digest_tests {
    use super::{blob_unchanged, forget_blob_digest, reset_last_blob_region, steady_state_notice_due};

    // LAST_BLOB_DIGEST is a process-global static shared with every other test
    // in this binary. Resetting first is not enough on its own: these cases run
    // in parallel, so one can land its own digest write between another's reset
    // and its assertions. Taking this lock keeps them off each other.
    static DIGEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn digest_tracks_changes_and_resets() {
        let _guard = DIGEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        reset_last_blob_region();

        let blob = b"{\"SubscribedToEmails\":1,\"RegularCredits\":100}".to_vec();
        assert!(!blob_unchanged(&blob), "first call always reports changed");
        assert!(blob_unchanged(&blob), "identical bytes report unchanged");

        let mut mutated = blob.clone();
        mutated[0] = b'[';
        assert!(!blob_unchanged(&mutated), "a changed byte must report changed");
        assert!(blob_unchanged(&mutated), "the new bytes become the baseline");

        reset_last_blob_region();
        assert!(!blob_unchanged(&mutated), "reset forces the next call to report changed");
    }

    // The scan stitches whole regions, so the blob arrives with a tail of
    // unrelated heap that a running client rewrites between cycles. Digesting
    // that tail is indistinguishable from the inventory itself changing, which
    // reparses a settled inventory on every cycle.
    #[test]
    fn a_rewritten_tail_after_the_blob_is_not_a_change() {
        let _guard = DIGEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        reset_last_blob_region();

        let blob = br#"{"SubscribedToEmails":1,"DeathSquadable":false}"#;
        let mut first = blob.to_vec();
        first.extend_from_slice(b"\x00\x11garbage from a neighbouring allocation");
        let mut second = blob.to_vec();
        second.extend_from_slice(b"\xff\xfe an entirely different neighbour, and longer");

        assert!(!blob_unchanged(&first), "first sighting reports changed");
        assert!(blob_unchanged(&second), "same blob, different tail, is unchanged");
    }

    // Unparseable bytes that persist across scan cycles must not start
    // reporting as unchanged — the skip paths read that as "already parsed
    // this", which would wedge the walk on a region that never parsed.
    #[test]
    fn forgetting_after_a_failed_parse_forces_a_retry() {
        let _guard = DIGEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        reset_last_blob_region();

        let garbage = b"{\"MiscItems\":[ truncated".to_vec();
        assert!(!blob_unchanged(&garbage), "first sighting reports changed");
        forget_blob_digest();
        assert!(!blob_unchanged(&garbage), "same bytes report changed again after a failed parse");
    }

    /// Nearly every probe finds identical bytes, so the notice cannot be a
    /// per-probe line, otherwise it drowns out everything else in the log.
    #[test]
    fn the_steady_state_notice_fires_once_per_settle() {
        let _guard = DIGEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        reset_last_blob_region();

        let blob = b"{\"SubscribedToEmails\":1,\"RegularCredits\":100}".to_vec();
        assert!(!blob_unchanged(&blob), "first sighting reports changed");
        assert!(blob_unchanged(&blob), "second sighting is the steady state");
        assert!(steady_state_notice_due(), "entering the steady state logs once");
        assert!(!steady_state_notice_due(), "staying in it does not log again");

        let mut mutated = blob.clone();
        mutated[0] = b'[';
        assert!(!blob_unchanged(&mutated), "the bytes changed");
        assert!(blob_unchanged(&mutated), "and settled again");
        assert!(steady_state_notice_due(), "the next settle logs again");

        reset_last_blob_region();
        assert!(steady_state_notice_due(), "a new game process starts the cycle over");
    }
}

#[cfg(test)]
mod sync_marker_tests {
    use super::{
        cold_log_search_due, looks_like_log_buffer, newest_sync_timestamp, reset_log_region,
        sync_marker_is_new, LOG_SEARCH_BACKOFF, LOG_SEARCH_BACKOFF_PROBES,
    };

    // LAST_SYNC_TIMESTAMP and LOG_SEARCH_BACKOFF are process-global, and
    // reset_log_region clears both at once, so a test calling it races any
    // other test mid-sequence. Every test here that resets takes this lock.
    static LOG_STATE_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn a_failed_cold_search_sits_out_the_next_probes() {
        let _guard = LOG_STATE_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        reset_log_region();
        assert!(cold_log_search_due(), "the first search runs");

        LOG_SEARCH_BACKOFF.store(LOG_SEARCH_BACKOFF_PROBES, std::sync::atomic::Ordering::Relaxed);
        for probe in 0..LOG_SEARCH_BACKOFF_PROBES {
            assert!(!cold_log_search_due(), "probe {probe} searched during the backoff");
        }
        assert!(cold_log_search_due(), "the search resumes once the backoff expires");

        // A PID change clears it: the new client's buffers are worth looking for
        // straight away.
        LOG_SEARCH_BACKOFF.store(LOG_SEARCH_BACKOFF_PROBES, std::sync::atomic::Ordering::Relaxed);
        reset_log_region();
        assert!(cold_log_search_due());
    }

    /// The heap ring uses LF, the pending file-write buffer CRLF, and the
    /// marker has to be found in either.
    #[test]
    fn marker_is_read_from_both_buffer_shapes() {
        let ring = b"19760.121 Sys [Info]: SyncInventoryFromDB\n\
                     19761.848 Sys [Info]: OnInventoryResults completed in 339ms\n";
        assert_eq!(newest_sync_timestamp(ring), Some(19761.848));

        let pending = b"19760.121 Sys [Info]: SyncInventoryFromDB\r\n\
                        19761.848 Sys [Info]: OnInventoryResults completed in 339ms\r\n";
        assert_eq!(newest_sync_timestamp(pending), Some(19761.848));
    }

    /// The ring wraps, so the newest line is not the one at the highest
    /// address. Taking the last match would report an already-seen sync.
    #[test]
    fn newest_marker_wins_regardless_of_position() {
        let wrapped = b"19999.500 Sys [Info]: OnInventoryResults completed in 41ms\n\
                        11000.000 Sys [Info]: OnInventoryResults completed in 88ms\n";
        assert_eq!(newest_sync_timestamp(wrapped), Some(19999.500));
    }

    /// `OnInventoryResults completed in` also exists as a read-only format
    /// string, which carries no timestamp and must not be mistaken for a line
    /// the game actually wrote.
    #[test]
    fn format_string_without_a_timestamp_is_not_a_marker() {
        assert_eq!(newest_sync_timestamp(b"OnInventoryResults completed in %dms\0"), None);
        assert!(!looks_like_log_buffer(b"Sys [Info]: %s\0 Sys [Info]: %s\0"));
        assert!(looks_like_log_buffer(b"19761.848 Sys [Info]: Revive completed on KubrowPetAvatar14482\n"));
    }

    #[test]
    fn baseline_reports_only_unseen_syncs() {
        let _guard = LOG_STATE_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        reset_log_region();
        assert!(sync_marker_is_new(Some(100.000)), "the first marker seen is not yet reported");
        assert!(!sync_marker_is_new(Some(100.000)), "the same sync must not report twice");
        assert!(sync_marker_is_new(Some(140.250)), "a later sync reports");
        // Seconds since launch, so a stamp running backwards means the client
        // restarted; ignoring it would swallow markers until the new session
        // outran the old one.
        assert!(sync_marker_is_new(Some(12.500)), "a restarted client reports again");
        assert!(!sync_marker_is_new(None), "no marker in the buffer reports nothing");
        reset_log_region();
    }

    /// The PID change that clears the baseline lands moments before the login
    /// sync, which is the marker the gate is there to catch. Spending the
    /// first observation on a baseline would drop it on every restart.
    #[test]
    fn login_sync_after_a_restart_is_reported() {
        let _guard = LOG_STATE_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        reset_log_region();
        assert!(sync_marker_is_new(Some(9821.400)), "a marker from the previous client");
        reset_log_region();
        assert!(sync_marker_is_new(Some(13.036)), "the new client's login sync must report");
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
