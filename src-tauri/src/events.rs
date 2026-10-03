//! Centralized Tauri event name constants — every `.emit()` call site in
//! `src-tauri/src` should reference one of these instead of a string literal.
//! Mirrors `src/constants/tauri.ts`'s `TAURI_EVENTS` object; keep both in sync
//! when adding, renaming, or retiring an event.
//!
//! Some constants here (the RIVEN_* window-lifecycle events, RELIC_SCREEN) have
//! no Rust-side `.emit()` call — they're broadcast window-to-window entirely on
//! the frontend (main window ↔ riven-overlay / relic-pick-overlay). They're kept
//! here anyway so this file stays the single manifest of every event name in the
//! app, matching `TAURI_EVENTS` 1:1.
#![allow(dead_code)]

pub const SETTINGS_UPDATED: &str = "settings-updated";

pub const RELIC_TRIGGER: &str = "relic-trigger";
pub const RELIC_REWARDS: &str = "relic-rewards";
pub const RELIC_SCREEN: &str = "relic-screen";
pub const RELIC_PICK_OPEN: &str = "relic-pick-open";
pub const RELIC_PICK_CLOSE: &str = "relic-pick-close";

pub const RIVEN_MANUAL_CHECK: &str = "riven-manual-check";
pub const RIVEN_WINDOW_READY: &str = "riven-window-ready";
pub const RIVEN_ANALYSIS_UPDATE: &str = "riven-analysis-update";
pub const RIVEN_ROLL_SAVED: &str = "riven-roll-saved";
pub const RIVEN_OVERLAY_HIDE: &str = "riven-overlay-hide";
pub const RIVEN_SCANNING_START: &str = "riven-scanning-start";
pub const RIVEN_SCREEN_OPEN: &str = "riven-screen-open";
pub const RIVEN_SCREEN_CLOSE: &str = "riven-screen-close";
pub const RIVEN_UNVEILED: &str = "riven-unveiled";

pub const INVENTORY_UPDATE: &str = "inventory-update";
pub const INVENTORY_REWARD: &str = "inventory-reward";
pub const CATALOGUE_UPDATED: &str = "catalogue-updated";
pub const BULK_PRICES_UPDATED: &str = "bulk-prices-updated";
pub const TRADE_COMPLETED: &str = "trade-completed";

pub const ARBITRATION_RUN_ENDED: &str = "arbitration-run-ended";
pub const ARBITRATION_RUNS_CHANGED: &str = "arbitration-runs-changed";

pub const BLOB_STATUS: &str = "blob-status";
pub const CACHE_STATUS: &str = "cache-status";
pub const CONSOLE_LOGIN_SUCCESS: &str = "console-login-success";
pub const FF_STATUS: &str = "ff-status";
pub const PLAYER_NAME: &str = "player-name";

pub const WFM_WHISPER: &str = "wfm-whisper";
pub const WFM_PRICE_UPDATE: &str = "wfm-price-update";
pub const WFM_TOP_PROGRESS: &str = "wfm-top-progress";
pub const WFM_TOP_UPDATED: &str = "wfm-top-updated";
