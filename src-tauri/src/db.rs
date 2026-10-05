use crate::arbitration::{EndReason, Event, MissionType, Parser as ArbitrationParser, Run};
use rusqlite::{params, Connection, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct QuantityChange {
    pub id: i64,
    pub unique_name: String,
    pub item_name: String,
    pub old_qty: i64,
    pub new_qty: i64,
    pub delta: i64,
    pub timestamp: i64,
    /// Rank of the mod/arcane that changed (None for regular items).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rank: Option<u8>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Trade {
    pub id: i64,
    pub timestamp: String,      // ISO-8601
    pub with_player: String,
    pub direction: String,      // "sold" | "bought" | "traded-out" | "traded-in"
    pub item_name: String,
    pub item_url: String,       // WFM slug (for price lookup), may be empty
    pub quantity: i64,
    pub platinum: i64,
    pub source: String,         // "wfm" | "ingame" | "manual"
    pub notes: String,
    pub session_id: String,     // groups items from the same trade session
    pub trade_type: String,     // "sale" | "purchase" | "trade" | "" (legacy)
}

pub fn init_db(db_path: &PathBuf) -> Result<Connection> {
    let conn = Connection::open(db_path)?;
    conn.execute_batch("PRAGMA journal_mode=WAL;")?;
    migrate(&conn)?;
    Ok(conn)
}

/// Schema migrations keyed by version number.
/// To add a schema change in a future release: add an `if version < N` block
/// with the ALTER/CREATE SQL and bump `pragma_update` to N.
fn migrate(conn: &Connection) -> Result<()> {
    let version: i32 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;

    if version < 2 {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS quantity_changes (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                unique_name TEXT    NOT NULL,
                item_name   TEXT    NOT NULL,
                old_qty     INTEGER NOT NULL,
                new_qty     INTEGER NOT NULL,
                delta       INTEGER NOT NULL,
                timestamp   INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS trades (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp   TEXT    NOT NULL,
                with_player TEXT    NOT NULL DEFAULT '',
                direction   TEXT    NOT NULL DEFAULT 'sold',
                item_name   TEXT    NOT NULL,
                item_url    TEXT    NOT NULL DEFAULT '',
                quantity    INTEGER NOT NULL DEFAULT 1,
                platinum    INTEGER NOT NULL DEFAULT 0,
                source      TEXT    NOT NULL DEFAULT 'manual',
                notes       TEXT    NOT NULL DEFAULT ''
            );
            CREATE TABLE IF NOT EXISTS saved_rivens (
                id          TEXT    PRIMARY KEY,
                weapon      TEXT    NOT NULL,
                label       TEXT    NOT NULL,
                stats_json  TEXT    NOT NULL,
                verdict     TEXT    NOT NULL DEFAULT '',
                score       REAL    NOT NULL DEFAULT 0,
                saved_at    TEXT    NOT NULL
            );
            CREATE TABLE IF NOT EXISTS tracked_items (
                unique_name  TEXT PRIMARY KEY,
                display_name TEXT NOT NULL,
                added_at     TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS item_snapshots (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                unique_name TEXT    NOT NULL,
                date        TEXT    NOT NULL,
                quantity    INTEGER NOT NULL,
                UNIQUE(unique_name, date)
            );",
        )?;
        conn.pragma_update(None, "user_version", 2)?;
    }

    if version < 3 {
        conn.execute_batch(
            "ALTER TABLE trades ADD COLUMN session_id TEXT NOT NULL DEFAULT '';
             ALTER TABLE trades ADD COLUMN trade_type TEXT NOT NULL DEFAULT '';"
        )?;
        conn.pragma_update(None, "user_version", 3)?;
    }

    if version < 4 {
        conn.execute_batch("ALTER TABLE quantity_changes ADD COLUMN rank INTEGER;")?;
        conn.pragma_update(None, "user_version", 4)?;
    }

    if version < 5 {
        let tx = conn.unchecked_transaction()?;
        tx.execute_batch(
            "CREATE TABLE IF NOT EXISTS arbitration_runs (
                uid                TEXT    PRIMARY KEY,
                started_at         TEXT,
                run_start_sec      REAL    NOT NULL,
                run_end_sec        REAL,
                mission_name       TEXT    NOT NULL,
                node               TEXT    NOT NULL,
                sol_node           TEXT,
                mission_type       TEXT    NOT NULL,
                mission_type_raw   TEXT,
                end_reason         TEXT    NOT NULL,
                duration_sec       REAL    NOT NULL,
                rotations          INTEGER NOT NULL,
                waves              INTEGER NOT NULL,
                waves_per_rotation INTEGER NOT NULL,
                kills              INTEGER NOT NULL,
                drone_kills        INTEGER NOT NULL,
                host_telemetry     INTEGER NOT NULL,
                vitus_mean         REAL    NOT NULL,
                vitus_std          REAL    NOT NULL,
                vitus_per_minute   REAL    NOT NULL,
                deleted            INTEGER NOT NULL DEFAULT 0
            );
            CREATE INDEX IF NOT EXISTS arbitration_runs_started
                ON arbitration_runs(started_at);",
        )?;
        tx.pragma_update(None, "user_version", 5)?;
        tx.commit()?;
    }

    // Prune entries older than 7 days so the log doesn't grow unbounded.
    conn.execute_batch(
        "DELETE FROM quantity_changes WHERE timestamp < unixepoch('now', '-7 days');"
    )?;

    Ok(())
}

// ── Tracked items / daily snapshots ──────────────────────────────────────────

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct TrackedItem {
    pub unique_name:  String,
    pub display_name: String,
    pub added_at:     String,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct SnapshotPoint {
    pub date:     String,
    pub quantity: i64,
    pub change:   i64,
}

pub fn add_tracked_item(conn: &Connection, unique_name: &str, display_name: &str) -> Result<()> {
    let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    conn.execute(
        "INSERT OR IGNORE INTO tracked_items (unique_name, display_name, added_at)
         VALUES (?1, ?2, ?3)",
        params![unique_name, display_name, now],
    )?;
    Ok(())
}

pub fn remove_tracked_item(conn: &Connection, unique_name: &str) -> Result<()> {
    conn.execute("DELETE FROM tracked_items  WHERE unique_name = ?1", params![unique_name])?;
    conn.execute("DELETE FROM item_snapshots WHERE unique_name = ?1", params![unique_name])?;
    Ok(())
}

pub fn get_tracked_items(conn: &Connection) -> Result<Vec<TrackedItem>> {
    let mut stmt = conn.prepare(
        "SELECT unique_name, display_name, added_at FROM tracked_items ORDER BY display_name",
    )?;
    let rows = stmt.query_map([], |row| Ok(TrackedItem {
        unique_name:  row.get(0)?,
        display_name: row.get(1)?,
        added_at:     row.get(2)?,
    }))?.filter_map(|r| r.ok()).collect();
    Ok(rows)
}

/// Record quantity for a tracked item on a given date.
/// INSERT OR IGNORE — the first scan of each day wins (stable historical record).
pub fn record_snapshot(conn: &Connection, unique_name: &str, date: &str, quantity: i64) -> Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO item_snapshots (unique_name, date, quantity) VALUES (?1, ?2, ?3)",
        params![unique_name, date, quantity],
    )?;
    Ok(())
}

pub fn get_snapshots(conn: &Connection, unique_name: &str, days: Option<u32>) -> Result<Vec<SnapshotPoint>> {
    let raw: Vec<(String, i64)> = match days {
        Some(d) => {
            let cutoff = format!("-{} days", d);
            let mut stmt = conn.prepare(
                "SELECT date, quantity FROM item_snapshots
                 WHERE unique_name = ?1 AND date >= date('now', ?2)
                 ORDER BY date ASC",
            )?;
            let rows: Vec<(String, i64)> = stmt
                .query_map(params![unique_name, cutoff], |r| Ok((r.get(0)?, r.get(1)?)))?
                .filter_map(|r| r.ok())
                .collect();
            rows
        }
        None => {
            let mut stmt = conn.prepare(
                "SELECT date, quantity FROM item_snapshots
                 WHERE unique_name = ?1 ORDER BY date ASC",
            )?;
            let rows: Vec<(String, i64)> = stmt
                .query_map(params![unique_name], |r| Ok((r.get(0)?, r.get(1)?)))?
                .filter_map(|r| r.ok())
                .collect();
            rows
        }
    };

    Ok(raw.iter().enumerate().map(|(i, (date, qty))| {
        let change = if i == 0 { 0 } else { qty - raw[i - 1].1 };
        SnapshotPoint { date: date.clone(), quantity: *qty, change }
    }).collect())
}

// ── Saved rivens ──────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SavedRiven {
    pub id: String,
    pub weapon: String,
    pub label: String,
    pub stats_json: String,   // JSON array of {name, value, positive}
    pub verdict: String,
    pub score: f64,
    pub saved_at: String,
}

pub fn save_riven(conn: &Connection, riven: &SavedRiven) -> Result<()> {
    conn.execute(
        "INSERT INTO saved_rivens (id, weapon, label, stats_json, verdict, score, saved_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![riven.id, riven.weapon, riven.label, riven.stats_json,
                riven.verdict, riven.score, riven.saved_at],
    )?;
    Ok(())
}

pub fn get_saved_rivens(conn: &Connection) -> Result<Vec<SavedRiven>> {
    let mut stmt = conn.prepare(
        "SELECT id, weapon, label, stats_json, verdict, score, saved_at
         FROM saved_rivens ORDER BY saved_at DESC LIMIT 50",
    )?;
    let rows = stmt.query_map([], |row| Ok(SavedRiven {
        id: row.get(0)?, weapon: row.get(1)?, label: row.get(2)?,
        stats_json: row.get(3)?, verdict: row.get(4)?,
        score: row.get(5)?, saved_at: row.get(6)?,
    }))?.filter_map(|r| r.ok()).collect();
    Ok(rows)
}

pub fn delete_saved_riven(conn: &Connection, id: &str) -> Result<()> {
    conn.execute("DELETE FROM saved_rivens WHERE id = ?1", params![id])?;
    Ok(())
}

pub fn rename_saved_riven(conn: &Connection, id: &str, label: &str) -> Result<()> {
    conn.execute("UPDATE saved_rivens SET label = ?1 WHERE id = ?2", params![label, id])?;
    Ok(())
}

pub fn count_saved_rivens(conn: &Connection) -> Result<i64> {
    conn.query_row("SELECT COUNT(*) FROM saved_rivens", [], |r| r.get(0))
}

pub fn add_trade(conn: &Connection, trade: &Trade) -> Result<i64> {
    conn.execute(
        "INSERT INTO trades (timestamp, with_player, direction, item_name, item_url, quantity, platinum, source, notes, session_id, trade_type)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            trade.timestamp, trade.with_player, trade.direction,
            trade.item_name, trade.item_url, trade.quantity,
            trade.platinum, trade.source, trade.notes,
            trade.session_id, trade.trade_type,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn get_trades(conn: &Connection) -> Result<Vec<Trade>> {
    let mut stmt = conn.prepare(
        "SELECT id, timestamp, with_player, direction, item_name, item_url,
                quantity, platinum, source, notes, session_id, trade_type
         FROM trades ORDER BY timestamp DESC",
    )?;
    let rows = stmt.query_map([], |row| Ok(Trade {
        id: row.get(0)?,
        timestamp: row.get(1)?,
        with_player: row.get(2)?,
        direction: row.get(3)?,
        item_name: row.get(4)?,
        item_url: row.get(5)?,
        quantity: row.get(6)?,
        platinum: row.get(7)?,
        source: row.get(8)?,
        notes: row.get(9)?,
        session_id: row.get(10)?,
        trade_type: row.get(11)?,
    }))?.filter_map(|r| r.ok()).collect();
    Ok(rows)
}

pub fn delete_trade(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM trades WHERE id = ?1", params![id])?;
    Ok(())
}

pub fn add_quantity_change(
    conn: &Connection,
    unique_name: &str,
    item_name: &str,
    old_qty: i64,
    new_qty: i64,
    rank: Option<u8>,
) -> Result<()> {
    let ts = chrono::Utc::now().timestamp();
    conn.execute(
        "INSERT INTO quantity_changes (unique_name, item_name, old_qty, new_qty, delta, timestamp, rank)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![unique_name, item_name, old_qty, new_qty, new_qty - old_qty, ts, rank],
    )?;
    Ok(())
}

pub fn get_quantity_changes(conn: &Connection, limit: i64) -> Result<Vec<QuantityChange>> {
    let mut stmt = conn.prepare(
        "SELECT id, unique_name, item_name, old_qty, new_qty, delta, timestamp, rank
         FROM quantity_changes
         ORDER BY id DESC
         LIMIT ?1",
    )?;
    let rows = stmt
        .query_map([limit], |row| {
            Ok(QuantityChange {
                id: row.get(0)?,
                unique_name: row.get(1)?,
                item_name: row.get(2)?,
                old_qty: row.get(3)?,
                new_qty: row.get(4)?,
                delta: row.get(5)?,
                timestamp: row.get(6)?,
                rank: row.get(7)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

// ── Change detection helpers ──────────────────────────────────────────────────

pub fn detect_quantity_changes(
    conn: &Connection,
    prev_all: &std::collections::HashMap<String, i64>,
    emit_qty: &std::collections::HashMap<String, i64>,
    ignored_paths: &std::collections::HashSet<String>,
    path_to_name: &std::collections::HashMap<String, String>,
) -> Vec<QuantityChange> {
    let mut changes = Vec::new();
    if prev_all.is_empty() { return changes; }
    let ts = chrono::Utc::now().timestamp();
    let all_keys: std::collections::HashSet<&String> =
        prev_all.keys().chain(emit_qty.keys()).collect();
    for key in all_keys {
        if ignored_paths.contains(key.as_str()) { continue; }
        let old_qty = *prev_all.get(key).unwrap_or(&0);
        let new_qty = *emit_qty.get(key).unwrap_or(&0);
        if old_qty == new_qty { continue; }
        let item_name = path_to_name.get(key.as_str())
            .cloned()
            .unwrap_or_else(|| key.split('/').next_back().unwrap_or("?").to_string());
        let _ = add_quantity_change(conn, key, &item_name, old_qty, new_qty, None);
        changes.push(QuantityChange {
            id: 0,
            unique_name: key.clone(),
            item_name,
            old_qty,
            new_qty,
            delta: new_qty - old_qty,
            timestamp: ts,
            rank: None,
        });
    }
    changes
}

pub fn detect_mod_rank_changes(
    conn: &Connection,
    prev_mods: &std::collections::HashMap<String, crate::memory_scanner::ModCount>,
    known_mods: &std::collections::HashMap<String, crate::memory_scanner::ModCount>,
    ignored_paths: &std::collections::HashSet<String>,
    path_to_name: &std::collections::HashMap<String, String>,
) -> Vec<QuantityChange> {
    let mut changes = Vec::new();
    if prev_mods.is_empty() { return changes; }
    let ts = chrono::Utc::now().timestamp();
    let all_paths: std::collections::HashSet<&String> =
        prev_mods.keys().chain(known_mods.keys()).collect();
    for path in all_paths {
        if ignored_paths.contains(path.as_str()) { continue; }
        let prev = prev_mods.get(path);
        let current = known_mods.get(path);
        let all_ranks: std::collections::HashSet<u8> = prev.into_iter()
            .flat_map(|mods| mods.by_rank.keys())
            .chain(current.into_iter().flat_map(|mods| mods.by_rank.keys()))
            .cloned()
            .collect();
        for rank in all_ranks {
            let old_count = prev.map(|p| *p.by_rank.get(&rank).unwrap_or(&0)).unwrap_or(0);
            let new_count = current.map(|mods| *mods.by_rank.get(&rank).unwrap_or(&0)).unwrap_or(0);
            if old_count == new_count { continue; }
            let item_name = path_to_name.get(path.as_str())
                .cloned()
                .unwrap_or_else(|| path.split('/').next_back().unwrap_or("?").to_string());
            let _ = add_quantity_change(conn, path, &item_name, old_count, new_count, Some(rank));
            changes.push(QuantityChange {
                id: 0,
                unique_name: path.clone(),
                item_name,
                old_qty: old_count,
                new_qty: new_count,
                delta: new_count - old_count,
                timestamp: ts,
                rank: Some(rank),
            });
        }
    }
    changes
}

// ── Arbitration runs ──────────────────────────────────────────────────────────
//
// Runs arrive from two directions — the tail of a live log and a backfill pass
// over the whole file at startup — and both go through `ArbitrationRecorder`,
// so there is one parser and one insert. Re-reading a log the database has
// already seen must add nothing, which is what `uid` buys.

/// Identity of a run, from the two things a log fixes about it: when it began
/// and where. Wall clock comes from the log's boot-time header; without one,
/// game time since launch stands in.
///
/// ponytail: two runs on the same node at the same game-time offset in a log
/// with no boot header collide and the second is dropped. Store a per-launch
/// discriminator if a header-less log ever shows up in practice.
fn run_uid(run: &Run) -> String {
    let when = match run.started_at {
        Some(t) => stored_timestamp(t),
        None => format!("t{:.3}", run.run_start_sec),
    };
    format!("{when}\u{1f}{}", run.node)
}

/// Date ranges are compared as text in SQL, so every timestamp that reaches
/// the column has to be written the same way: UTC, milliseconds, `Z`. Chrono's
/// plain `to_rfc3339` writes `+00:00`, which sorts *below* the `Z` form that
/// every other tool produces, and a bound in one form silently excludes rows
/// stored in the other.
fn stored_timestamp(t: chrono::DateTime<chrono::Utc>) -> String {
    t.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

pub fn store_arbitration_run(conn: &Connection, run: &Run) -> Result<bool> {
    let changed = conn.execute(
        "INSERT OR IGNORE INTO arbitration_runs (
             uid, started_at, run_start_sec, run_end_sec, mission_name, node,
             sol_node, mission_type, mission_type_raw, end_reason, duration_sec,
             rotations, waves, waves_per_rotation, kills, drone_kills,
             host_telemetry, vitus_mean, vitus_std, vitus_per_minute)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14,
                 ?15, ?16, ?17, ?18, ?19, ?20)",
        params![
            run_uid(run),
            run.started_at.map(stored_timestamp),
            run.run_start_sec,
            run.run_end_sec,
            run.mission_name,
            run.node,
            run.sol_node,
            run.mission_type.as_str(),
            run.mission_type_raw,
            run.end_reason.as_str(),
            run.duration_sec,
            run.rotations,
            run.waves,
            run.waves_per_rotation,
            run.kills,
            run.drone_kills,
            run.host_telemetry,
            run.vitus.mean,
            run.vitus.std,
            run.vitus.per_minute,
        ],
    )?;
    Ok(changed > 0)
}

#[derive(Debug, Clone, Serialize)]
pub struct RunRecord {
    pub uid: String,
    pub started_at: Option<String>,
    pub node: String,
    /// Looked up from the run's star chart node rather than stored: the
    /// rating is a view of the node, and a run recorded before the rating
    /// existed still shows today's.
    pub tier: Option<&'static str>,
    pub mission_type: &'static str,
    pub end_reason: &'static str,
    pub duration_sec: f64,
    pub rotations: u32,
    pub waves: u32,
    pub kills: u32,
    pub drone_kills: u32,
    pub vitus_mean: f64,
    pub vitus_per_minute: f64,
}

/// Newest first. Filtering happens in the view: the whole history is small,
/// and the filters change far more often than the data.
///
/// The key is read back from the row rather than re-derived, so a delete
/// names exactly what was stored even if the derivation changes.
pub fn list_arbitration_runs(conn: &Connection) -> Result<Vec<RunRecord>> {
    let mut stmt = conn.prepare(
        "SELECT uid, started_at, node, mission_type, end_reason, duration_sec,
                rotations, waves, kills, drone_kills, vitus_mean, vitus_per_minute,
                sol_node
         FROM arbitration_runs
         WHERE deleted = 0
         ORDER BY started_at DESC, run_start_sec DESC",
    )?;
    let rows = stmt
        .query_map([], |row| {
            let mission_type: String = row.get(3)?;
            let end_reason: String = row.get(4)?;
            Ok(RunRecord {
                uid: row.get(0)?,
                started_at: row.get(1)?,
                node: row.get(2)?,
                tier: row
                    .get::<_, Option<String>>(12)?
                    .as_deref()
                    .and_then(crate::arbitrations::tier),
                mission_type: MissionType::from_stored(&mission_type).as_str(),
                end_reason: EndReason::from_stored(&end_reason).as_str(),
                duration_sec: row.get(5)?,
                rotations: row.get(6)?,
                waves: row.get(7)?,
                kills: row.get(8)?,
                drone_kills: row.get(9)?,
                vitus_mean: row.get(10)?,
                vitus_per_minute: row.get(11)?,
            })
        })?
        .collect::<Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn delete_arbitration_run(conn: &Connection, uid: &str) -> Result<bool> {
    let changed = conn.execute(
        "UPDATE arbitration_runs SET deleted = 1 WHERE uid = ?1",
        params![uid],
    )?;
    Ok(changed > 0)
}

/// Log text in, stored runs out, in two steps so that the database is only
/// locked for the writes: parsing a whole log at startup takes far longer than
/// the handful of inserts it produces, and every other query waits behind that
/// same lock.
///
/// Holds the parser across calls so a run split over several reads still ends
/// as one run.
#[derive(Default)]
pub struct ArbitrationRecorder {
    parser: ArbitrationParser,
    /// A read of the growing log ends wherever the game happened to flush, so
    /// the last line of a chunk is often half a line. It waits here for its
    /// other half rather than reaching the parser mangled.
    partial: String,
    /// Runs parsed but not yet written. The lines behind a run are consumed by
    /// the parser as they are read and cannot be replayed, so a run whose
    /// insert failed waits here for the next attempt instead of being lost.
    pending: std::collections::VecDeque<Run>,
}

/// Enough runs for a long session of failed writes. Past it the oldest go,
/// because a queue that grows for as long as the database is broken is a leak.
///
/// ponytail: the queue lives in memory, so a crash loses it either way, and
/// the oldest go first on the assumption that a newer run is the one worth
/// saving. Persist it if runs lost to a full disk turn out to matter.
const MAX_PENDING_RUNS: usize = 256;

impl ArbitrationRecorder {
    /// A replaced log belongs to a new launch, but queued writes still belong
    /// to history and must survive the parser reset.
    pub fn restart(&mut self) {
        self.parser = ArbitrationParser::new();
        self.partial.clear();
    }

    /// Reads runs out of the text. A run still in progress is not parsed out:
    /// it has no end and no final counts yet.
    ///
    /// TODO: a log that stops mid-run because the game crashed looks identical
    /// to one whose run is still going, so that last run is never recorded.
    /// Recovering it needs a signal that the log is finished with.
    pub fn parse(&mut self, chunk: String) -> Vec<Run> {
        // Taking the text whole rather than copying it in matters at startup,
        // where the chunk is the entire log.
        if self.partial.is_empty() {
            self.partial = chunk;
        } else {
            self.partial.push_str(&chunk);
        }
        let Some(last_newline) = self.partial.rfind('\n') else {
            return Vec::new();
        };
        let half_line = self.partial.split_off(last_newline + 1);
        let complete = std::mem::replace(&mut self.partial, half_line);

        let mut ended = Vec::new();
        for line in complete.lines() {
            for event in self.parser.feed_line(line) {
                if let Event::RunEnded(run) = event {
                    if self.pending.len() == MAX_PENDING_RUNS {
                        self.pending.pop_front();
                        tracing::warn!(
                            queued = MAX_PENDING_RUNS,
                            "arbitration run queue is full; dropping the oldest run"
                        );
                    }
                    self.pending.push_back((*run).clone());
                    ended.push(*run);
                }
            }
        }
        ended
    }

    /// Writes what `parse` found, and returns how many rows that added. A run
    /// stays queued until its insert succeeds, so a database that is busy or
    /// full delays runs rather than dropping them.
    ///
    /// One transaction for the batch, because a startup backfill's runs would
    /// otherwise cost a disk sync each. The queue is cleared only once the
    /// commit lands: a failure rolls the whole batch back, and the runs behind
    /// it have to still be there for the next attempt.
    pub fn store(&mut self, conn: &Connection) -> Result<usize> {
        if self.pending.is_empty() {
            return Ok(0);
        }
        let tx = conn.unchecked_transaction()?;
        let mut stored = 0;
        for run in &self.pending {
            if store_arbitration_run(&tx, run)? {
                stored += 1;
            }
        }
        tx.commit()?;
        self.pending.clear();
        Ok(stored)
    }
}

#[derive(Debug, Serialize, Clone, PartialEq)]
pub struct RunSummary {
    pub node: String,
    pub mission_type: &'static str,
    pub duration_sec: f64,
    pub rotations: u32,
    pub waves: u32,
    pub kills: u32,
    pub drone_kills: u32,
    pub host_telemetry: bool,
    pub vitus_mean: f64,
    pub vitus_per_minute: f64,
}

/// Which run, if any, the post-run overlay shows for the runs a read just
/// ended. Only the newest is the one the player just left, and if that one
/// was aborted an older completed run must not stand in for it.
///
/// `live` is false for text that may be old, such as the startup backfill: a
/// run that ended hours ago must not put a summary over the game now.
pub fn live_run_summary(ended: &[Run], live: bool, enabled: bool) -> Option<RunSummary> {
    if !(live && enabled) {
        return None;
    }
    ended.last().and_then(post_run_summary)
}

/// `None` unless the run ended normally: an abort, or a new mission cutting
/// the old one off, has nothing worth putting over the game.
pub fn post_run_summary(run: &Run) -> Option<RunSummary> {
    if run.end_reason != EndReason::MissionEnd {
        return None;
    }
    Some(RunSummary {
        node: run.node.clone(),
        mission_type: run.mission_type.as_str(),
        duration_sec: run.duration_sec,
        rotations: run.rotations,
        waves: run.waves,
        kills: run.kills,
        drone_kills: run.drone_kills,
        host_telemetry: run.host_telemetry,
        vitus_mean: run.vitus.mean,
        vitus_per_minute: run.vitus.per_minute,
    })
}

#[cfg(test)]
fn db(_name: &str) -> Connection {
    let conn = Connection::open_in_memory().expect("in-memory database opens");
    migrate(&conn).expect("fresh schema migrates");
    conn
}

#[cfg(test)]
mod arbitration_storage_tests {
    use super::*;

    const DEFENSE: &str = include_str!("../tests/fixtures/arbitration/stoefler-defense.txt");
    const SURVIVAL: &str = include_str!("../tests/fixtures/arbitration/mot-survival.txt");
    const ABORT: &str = include_str!("../tests/fixtures/arbitration/oestrus-abort.txt");

    /// Splits the log into chunks that mostly end mid-line, the way a read of
    /// a file the game is still writing does.
    fn watch(conn: &Connection, log: &str, chunk_len: usize) -> usize {
        let mut recorder = ArbitrationRecorder::default();
        let bytes = log.as_bytes();
        let mut stored = 0;
        let mut at = 0;
        while at < bytes.len() {
            let mut end = (at + chunk_len).min(bytes.len());
            while !log.is_char_boundary(end) {
                end += 1;
            }
            recorder.parse(log[at..end].to_string());
            stored += recorder.store(conn).expect("recording succeeds");
            at = end;
        }
        stored
    }

    fn stored(conn: &Connection) -> Vec<Run> {
        let mut stmt = conn
            .prepare(
                "SELECT started_at, run_start_sec, run_end_sec, mission_name, node,
                        sol_node, mission_type, mission_type_raw, end_reason,
                        duration_sec, rotations, waves, waves_per_rotation, kills,
                        drone_kills, host_telemetry, vitus_mean, vitus_std,
                        vitus_per_minute
                 FROM arbitration_runs
                 WHERE deleted = 0
                 ORDER BY started_at, run_start_sec",
            )
            .expect("statement compiles");
        stmt.query_map([], |row| {
            let started_at: Option<String> = row.get(0)?;
            let mission_type: String = row.get(6)?;
            let end_reason: String = row.get(8)?;
            Ok(Run {
                started_at: started_at
                    .and_then(|t| chrono::DateTime::parse_from_rfc3339(&t).ok())
                    .map(|t| t.with_timezone(&chrono::Utc)),
                run_start_sec: row.get(1)?,
                run_end_sec: row.get(2)?,
                mission_name: row.get(3)?,
                node: row.get(4)?,
                sol_node: row.get(5)?,
                mission_type: MissionType::from_stored(&mission_type),
                mission_type_raw: row.get(7)?,
                end_reason: EndReason::from_stored(&end_reason),
                duration_sec: row.get(9)?,
                rotations: row.get(10)?,
                waves: row.get(11)?,
                waves_per_rotation: row.get(12)?,
                kills: row.get(13)?,
                host_telemetry: row.get(15)?,
                drone_kills: row.get(14)?,
                vitus: crate::arbitration::Vitus {
                    mean: row.get(16)?,
                    std: row.get(17)?,
                    per_minute: row.get(18)?,
                },
            })
        })
        .expect("query runs")
        .collect::<Result<Vec<_>>>()
        .expect("rows decode")
    }

    #[test]
    fn fresh_schema_has_soft_deletion_and_started_index() {
        let conn = db("schema");
        assert_eq!(
            conn.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .expect("version reads"),
            5
        );
        let deleted: i64 = conn.query_row(
            "SELECT COUNT(*) FROM pragma_table_info('arbitration_runs') WHERE name = 'deleted' AND dflt_value = '0'",
            [], |r| r.get(0),
        ).expect("columns read");
        assert_eq!(deleted, 1);
        let indexes: i64 = conn.query_row(
            "SELECT COUNT(*) FROM pragma_index_list('arbitration_runs') WHERE name = 'arbitration_runs_started'",
            [], |r| r.get(0),
        ).expect("indexes read");
        assert_eq!(indexes, 1);
    }

    #[test]
    fn populated_v4_upgrade_preserves_existing_records_and_schema() {
        let conn = db("upgrade");
        conn.execute_batch(
            "DROP TABLE arbitration_runs;
             PRAGMA user_version = 4;
             INSERT INTO trades (timestamp, item_name, session_id, trade_type)
                VALUES ('2026-07-08T12:00:00Z', 'Revenant Prime Set', 'trade-session', 'sale');
             INSERT INTO quantity_changes (unique_name, item_name, old_qty, new_qty, delta, timestamp, rank)
                VALUES ('/Lotus/Mods/Flow', 'Flow', 1, 2, 1, unixepoch(), 5);
             INSERT INTO saved_rivens VALUES ('riven', 'Rubico', 'Critical', '[]', 'Keep', 4, '2026-07-08');
             INSERT INTO tracked_items VALUES ('/Lotus/Mods/Flow', 'Flow', '2026-07-08');
             INSERT INTO item_snapshots (unique_name, date, quantity) VALUES ('/Lotus/Mods/Flow', '2026-07-08', 2);"
        ).expect("upstream v4 records insert");
        migrate(&conn).expect("v4 upgrade succeeds");
        migrate(&conn).expect("migration rerun succeeds");
        let trades = get_trades(&conn).expect("trades read");
        assert_eq!(trades.len(), 1);
        assert_eq!(trades[0].session_id, "trade-session");
        assert_eq!(trades[0].trade_type, "sale");
        assert_eq!(
            get_quantity_changes(&conn, 10).expect("changes read")[0].rank,
            Some(5)
        );
        assert_eq!(get_saved_rivens(&conn).expect("rivens read")[0].id, "riven");
        assert_eq!(
            get_tracked_items(&conn).expect("tracked items read").len(),
            1
        );
        assert_eq!(
            get_snapshots(&conn, "/Lotus/Mods/Flow", None).expect("snapshots read")[0].quantity,
            2
        );
        let trade_uid: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('trades') WHERE name = 'uid'",
                [],
                |r| r.get(0),
            )
            .expect("trade columns read");
        assert_eq!(trade_uid, 0);
        assert_eq!(watch(&conn, DEFENSE, 97), 1);
    }

    #[test]
    fn failed_v5_migration_rolls_back_schema_and_version() {
        let conn = db("migration-failure");
        conn.execute_batch(
            "DROP TABLE arbitration_runs;
             CREATE TABLE arbitration_runs_started (id INTEGER);
             PRAGMA user_version = 4;",
        )
        .expect("index-name conflict prepares");
        assert!(migrate(&conn).is_err());
        assert_eq!(
            conn.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .expect("version reads"),
            4
        );
        let tables: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE name = 'arbitration_runs'",
                [],
                |r| r.get(0),
            )
            .expect("schema reads");
        assert_eq!(tables, 0);
        conn.execute_batch("DROP TABLE arbitration_runs_started;")
            .expect("conflict removes");
        migrate(&conn).expect("migration retries");
        assert_eq!(watch(&conn, DEFENSE, 97), 1);
    }

    #[test]
    fn a_run_watched_in_chunks_is_stored_as_one_run() {
        let conn = db("live");
        assert_eq!(watch(&conn, DEFENSE, 97), 1);

        let runs = stored(&conn);
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].node, "Stöfler (Lua)");
        assert_eq!(
            runs[0].end_reason,
            crate::arbitration::EndReason::MissionEnd
        );
        assert_eq!(
            runs[0].mission_type,
            crate::arbitration::MissionType::Defense
        );
    }

    #[test]
    fn parse_hands_back_the_runs_that_ended_in_the_chunk() {
        let mut recorder = ArbitrationRecorder::default();
        let ended = recorder.parse(DEFENSE.to_string());
        assert_eq!(ended.len(), 1);
        let summary = post_run_summary(&ended[0]).expect("a completed run has a summary");
        assert_eq!(summary.node, "Stöfler (Lua)");
        assert_eq!(summary.mission_type, "defense");
        assert!(summary.waves > 0);
        assert!(summary.duration_sec > 0.0);

        assert!(recorder.parse(String::new()).is_empty());
    }

    #[test]
    fn only_a_normally_ended_run_has_a_summary() {
        let mut recorder = ArbitrationRecorder::default();
        let ended = recorder.parse(ABORT.to_string());
        assert_eq!(ended.len(), 1);
        assert_eq!(ended[0].end_reason, EndReason::Aborted);
        assert!(post_run_summary(&ended[0]).is_none());

        let mut cut_off = completed_run();
        cut_off.end_reason = EndReason::NewMission;
        assert!(post_run_summary(&cut_off).is_none());
    }

    fn completed_run() -> Run {
        let mut recorder = ArbitrationRecorder::default();
        recorder.parse(DEFENSE.to_string()).remove(0)
    }

    fn aborted_run() -> Run {
        let mut recorder = ArbitrationRecorder::default();
        recorder.parse(ABORT.to_string()).remove(0)
    }

    #[test]
    fn the_overlay_fires_for_a_live_completed_run_only_when_enabled() {
        let ended = [completed_run()];
        assert!(live_run_summary(&ended, true, true).is_some());
        assert!(live_run_summary(&ended, false, true).is_none(), "backfill");
        assert!(
            live_run_summary(&ended, true, false).is_none(),
            "toggle off"
        );
        assert!(live_run_summary(&[], true, true).is_none());
    }

    #[test]
    fn the_newest_run_decides_whether_the_overlay_fires() {
        assert!(live_run_summary(&[completed_run(), aborted_run()], true, true).is_none());
        assert!(live_run_summary(&[aborted_run(), completed_run()], true, true).is_some());
    }

    #[test]
    fn an_aborted_run_is_stored_too() {
        let conn = db("abort");
        assert_eq!(watch(&conn, ABORT, 4096), 1);
        assert_eq!(
            stored(&conn)[0].end_reason,
            crate::arbitration::EndReason::Aborted
        );
    }

    #[test]
    fn processing_the_same_log_twice_stores_nothing_the_second_time() {
        let conn = db("idempotent");
        assert_eq!(watch(&conn, DEFENSE, 4096), 1);
        assert_eq!(watch(&conn, DEFENSE, 4096), 0);
        assert_eq!(stored(&conn).len(), 1);
    }

    #[test]
    fn a_run_still_in_progress_is_not_stored_until_it_ends() {
        let conn = db("in-progress");
        let cut = DEFENSE.len() / 2;
        let head = &DEFENSE[..DEFENSE[..cut].rfind('\n').expect("the fixture has lines") + 1];

        let mut recorder = ArbitrationRecorder::default();
        recorder.parse(head.to_string());
        assert_eq!(recorder.store(&conn).expect("recording succeeds"), 0);
        assert!(stored(&conn).is_empty());

        recorder.parse(DEFENSE[head.len()..].to_string());
        assert_eq!(recorder.store(&conn).expect("recording succeeds"), 1);
        assert_eq!(stored(&conn).len(), 1);
    }

    #[test]
    fn a_listed_run_carries_the_tier_of_the_node_it_ran_on() {
        let conn = db("run-tier");
        watch(&conn, DEFENSE, 4096);
        let listed = list_arbitration_runs(&conn).expect("read succeeds");
        let run = listed.first().expect("the defense run is listed");
        assert_eq!(run.node, "Stöfler (Lua)");
        assert_eq!(run.tier, Some("D"));
    }

    #[test]
    fn a_deleted_run_is_gone_and_stays_gone_across_a_backfill() {
        let conn = db("delete");
        watch(&conn, DEFENSE, 4096);
        watch(&conn, SURVIVAL, 4096);
        let listed = list_arbitration_runs(&conn).expect("read succeeds");
        assert_eq!(listed.len(), 2);
        assert!(listed[0].started_at > listed[1].started_at, "newest first");

        let survival = listed
            .iter()
            .find(|r| r.node == "Mot (Void)")
            .expect("the survival run is listed");
        assert!(delete_arbitration_run(&conn, &survival.uid).expect("delete succeeds"));
        assert!(!delete_arbitration_run(&conn, "no such run").expect("a miss is not an error"));
        assert_eq!(stored(&conn).len(), 1);
        assert_eq!(
            list_arbitration_runs(&conn).expect("read succeeds").len(),
            1
        );
        assert_eq!(stored(&conn)[0].node, "Stöfler (Lua)");

        assert_eq!(watch(&conn, SURVIVAL, 4096), 0);
        assert_eq!(stored(&conn).len(), 1);
    }

    /// The uid is the start time plus the node, so two runs on one node at one
    /// game-time offset in a log without a boot header are one row and the
    /// second is dropped. `run_uid` documents that as a known collision waiting
    /// for a header-less log; this test is what makes the drop visible.
    #[test]
    fn two_headerless_runs_on_one_node_at_one_offset_store_once() {
        let conn = db("uid-collision");
        let mut first = completed_run();
        first.started_at = None;
        first.run_start_sec = 100.0;
        let mut second = first.clone();
        second.run_end_sec = Some(200.0);

        assert!(
            store_arbitration_run(&conn, &first).expect("the first run stores"),
            "a new identity is a new row"
        );
        assert!(
            !store_arbitration_run(&conn, &second).expect("a collision is not an error"),
            "the colliding run is dropped rather than stored twice"
        );
        assert_eq!(stored(&conn).len(), 1);
    }

    /// The parser has already consumed the lines behind a queued run, so a
    /// write that fails has to leave the run recoverable rather than drop it.
    #[test]
    fn a_run_survives_a_failed_write_and_lands_on_the_next_attempt() {
        let conn = db("write-failure");
        conn.execute_batch("ALTER TABLE arbitration_runs RENAME TO arbitration_runs_hidden;")
            .expect("the table can be moved out of the way");

        let mut recorder = ArbitrationRecorder::default();
        recorder.parse(DEFENSE.to_string());
        assert!(
            recorder.store(&conn).is_err(),
            "the write must fail with no table"
        );

        conn.execute_batch("ALTER TABLE arbitration_runs_hidden RENAME TO arbitration_runs;")
            .expect("the table can be put back");
        assert_eq!(
            recorder.store(&conn).expect("the retry succeeds"),
            1,
            "the run held on across the failure"
        );
        assert_eq!(stored(&conn).len(), 1);
    }

    #[test]
    fn pending_writes_survive_log_replacement() {
        let conn = db("replacement-retry");
        let mut recorder = ArbitrationRecorder::default();
        recorder.parse(DEFENSE.to_string());
        conn.execute_batch("ALTER TABLE arbitration_runs RENAME TO arbitration_runs_hidden;")
            .expect("table hides");
        assert!(recorder.store(&conn).is_err());
        recorder.parse("100.000 Script [Info]: ThemedSquadOverlay.lua: Mission name: Arbitration: Casta (Ceres)\npartial".into());
        recorder.restart();
        assert!(recorder.parse(String::new()).is_empty());
        assert_eq!(recorder.parse(SURVIVAL.to_string()).len(), 1);
        conn.execute_batch("ALTER TABLE arbitration_runs_hidden RENAME TO arbitration_runs;")
            .expect("table restores");
        assert_eq!(recorder.store(&conn).expect("queued runs retry"), 2);
        assert_eq!(stored(&conn).len(), 2);
    }

    #[test]
    fn a_stored_run_equals_the_run_the_parser_produced() {
        let whole = crate::arbitration::parse_log(DEFENSE.lines());
        for chunk_len in [1, 13, 512, 100_000] {
            let conn = db(&format!("boundary-{chunk_len}"));
            watch(&conn, DEFENSE, chunk_len);
            assert_eq!(stored(&conn), whole, "chunk length {chunk_len}");
        }
    }
}
