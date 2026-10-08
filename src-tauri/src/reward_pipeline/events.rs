//! JSONL session log: one JSON object per line, appended the moment it happens.
//!
//! The session log exists to be *read by an AI* (or a human with `jq`), not to
//! be pretty — every line is a self-contained event with its timestamp, deltas
//! and full diagnostic payload, so a reader can reconstruct what each OCR run
//! did without cross-referencing prose or a summary block. No tree glyphs, no
//! summaries, no prose state lines.

use serde::Serialize;

/// Local wall-clock timestamp `HH:MM:SS.mmm` used as the `t` field.
pub(crate) fn now_ts() -> String {
    chrono::Local::now().format("%H:%M:%S%.3f").to_string()
}

/// Per-attempt raw OCR lines, captured verbatim for the `ocr` field.
#[derive(Clone, Serialize)]
pub(crate) struct OcrDiag {
    pub chars: usize,
    pub lines: usize,
    pub raw: String,
    pub text: Vec<OcrLineDiag>,
}

/// One OCR line with its position and whether the matcher used it.
#[derive(Clone, Serialize)]
pub(crate) struct OcrLineDiag {
    pub i: usize,
    pub x: f32,
    pub y: f32,
    pub text: String,
    pub used: bool,
    /// Why the line was excluded, when `used` is false:
    /// `y<0.10 top-HUD`, `below-bar`, `player name`, `UI badge`, `endless bonus UI`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skip: Option<String>,
}

/// Rarity-bar detector outcome.
#[derive(Clone, Serialize)]
pub(crate) struct BarsDiag {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub y: Option<f32>,
    pub segments: usize,
    pub note: String,
}

/// Icon-classifier outcome for a below-threshold column.
#[derive(Clone, Serialize)]
pub(crate) struct IconDiag {
    pub classifier: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter: Option<String>,
    pub score: f32,
    pub accepted: bool,
    pub top3: Vec<(f32, String)>,
}

/// One reward card column as scored by the matcher.
#[derive(Clone, Serialize)]
pub(crate) struct ColumnDiag {
    pub x: f32,
    pub score: f32,
    /// Display name of the chosen catalog item (confirmed columns).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub item: Option<String>,
    /// Raw OCR text when the column stayed below the 0.67 threshold (`?:` items).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unknown: Option<String>,
    pub ocr_words: usize,
    pub ocr: Vec<String>,
    pub top3: Vec<(f32, String)>,
    /// Catalog-name words seen in OCR: (found, total).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evidence: Option<(usize, usize)>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub missing: Vec<String>,
    /// `below_threshold`, `sparse` (≤1 OCR word), `ambiguous` (<0.05 margin),
    /// `partial_words` (score without every catalog word), `icon_confirmed`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub flags: Vec<String>,
    /// Present when the icon classifier was consulted (text score < 0.67).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<IconDiag>,
}

/// Everything one attempt's pipeline stages produced, serialized flat into the
/// attempt event. `capture_kind` doubles as the machine-readable outcome:
/// `ok`, `dark_frame`, `ocr_empty`, `ocr_error`, `relic_select`.
#[derive(Clone, Serialize)]
pub(crate) struct AttemptDiag {
    pub capture_kind: &'static str,
    pub capture: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ocr: Option<OcrDiag>,
    pub bars: BarsDiag,
    pub cols_source: String,
    pub cols: usize,
    pub hint_size: Option<usize>,
    /// `none` | `relic_guess` | `ee_handshake`.
    pub hint_source: &'static str,
    pub expected: usize,
    /// `squad_hint` | `bars` | `x_clusters` | `prime_forma`.
    pub expected_src: &'static str,
    pub prime_cards: usize,
    pub forma_cards: usize,
    pub text_clusters: usize,
    pub columns: Vec<ColumnDiag>,
    /// Items appended by the whole-screen fill pass (not scored per column).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub filled: Vec<String>,
    /// Items as display names (what the overlay receives); `?:` entries pass through raw.
    pub display_items: Vec<String>,
}

impl AttemptDiag {
    /// Minimal diag for attempts that never reached column matching.
    pub(crate) fn capture_only(capture_kind: &'static str, capture: String) -> Self {
        Self {
            capture_kind,
            capture,
            note: None,
            ocr: None,
            bars: BarsDiag {
                ok: false,
                y: None,
                segments: 0,
                note: "not evaluated".into(),
            },
            cols_source: "none".into(),
            cols: 0,
            hint_size: None,
            hint_source: "none",
            expected: 0,
            expected_src: "none",
            prime_cards: 0,
            forma_cards: 0,
            text_clusters: 0,
            columns: Vec::new(),
            filled: Vec::new(),
            display_items: Vec::new(),
        }
    }
}

/// Which payload the loop currently considers best, for `prev_best`.
#[derive(Clone, Serialize)]
pub(crate) struct PrevBest {
    pub n: u32,
    pub confirmed: usize,
}

/// The `{"event":"attempt"}` line — one per OCR run, written as it happens.
#[derive(Clone, Serialize)]
pub(crate) struct AttemptEvent {
    pub t: String,
    pub event: &'static str,
    pub n: u32,
    /// `fresh` | `reuse` (re-read of the cached frame through preprocessing).
    pub mode: &'static str,
    /// Loop outcome for this run: `best`, `no_improvement`, `confirm`,
    /// `dark_frame`, `ocr_empty`, `ocr_error`, `no_match`, `capture_failed`.
    pub result: &'static str,
    /// Milliseconds between the EE.log trigger and this attempt.
    pub after_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry_ms: Option<u64>,
    pub items: Vec<String>,
    pub confirmed: usize,
    pub unknown: usize,
    pub complete: bool,
    pub low_confidence: bool,
    /// Why the loop could not lock on this result (empty when it could).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocker: Option<String>,
    /// True when this attempt replaced the current best payload.
    pub best: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prev_best: Option<PrevBest>,
    /// On no-match events: true when this strike expanded the catalog to full.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub catalog_expanded: Option<bool>,
    #[serde(flatten)]
    pub diag: Option<AttemptDiag>,
}
