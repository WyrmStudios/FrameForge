//! Subscriber setup: a compact daily-rolling file log plus a coloured console
//! log, both fed from the same `tracing` events.

use std::sync::OnceLock;

use tracing_subscriber::{
    EnvFilter, fmt,
    layer::{Layer, SubscriberExt},
    util::SubscriberInitExt,
};

/// Dropping the `WorkerGuard` flushes the non-blocking writer's buffer, so it
/// has to outlive every log call; otherwise the last events (including a
/// panic) never reach disk.
static FILE_GUARD: OnceLock<tracing_appender::non_blocking::WorkerGuard> = OnceLock::new();

/// Installs the global subscriber, log-crate bridge and panic hook. Safe to
/// call more than once; later calls do nothing.
pub fn init() {
    if FILE_GUARD.get().is_some() {
        return;
    }

    // Logs live with the rest of the app's files, under the plain `frameforge`
    // name. The Tauri log dir is keyed on the app identifier and holds none of
    // them, so release builds migrate their old files out of it below; dev
    // builds never wrote there.
    let log_dir = crate::paths::state_dir().join("logs");
    let _ = std::fs::create_dir_all(&log_dir);

    let appender = tracing_appender::rolling::Builder::new()
        .rotation(tracing_appender::rolling::Rotation::DAILY)
        .filename_prefix("frameforge")
        .filename_suffix("log")
        .max_log_files(5)
        .build(&log_dir)
        .expect("log dir was just created");
    // Non-blocking so the scanner and OCR hot loops never stall on disk I/O.
    let (writer, guard) = tracing_appender::non_blocking(appender);
    let _ = FILE_GUARD.set(guard);

    let file_layer = fmt::layer()
        .compact()
        .with_ansi(false)
        .with_target(true)
        // Span-close events carry time.busy/time.idle, which is where the
        // scanner and OCR timings surface.
        .with_span_events(fmt::format::FmtSpan::CLOSE)
        .with_writer(writer)
        .with_filter(filter("warn,warframe_companion_lib=debug"));

    let console_layer = fmt::layer()
        .compact()
        .with_ansi(true)
        .with_target(true)
        .with_writer(std::io::stderr)
        .with_filter(filter("info,warframe_companion_lib=info"));

    let _ = tracing_subscriber::registry()
        .with(file_layer)
        .with(console_layer)
        .try_init();

    let _ = tracing_log::LogTracer::init();

    // After the subscriber is live so the move itself is recorded in the new
    // log file. Release builds only: dev never wrote to the identifier dir.
    if crate::paths::root().is_none() {
        migrate_legacy_logs(&log_dir);
    }

    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let payload = info.payload();
        let message = payload
            .downcast_ref::<&str>()
            .copied()
            .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
            .unwrap_or("<non-string panic payload>");
        tracing::error!(
            location = %info.location().map_or_else(|| "unknown".to_string(), ToString::to_string),
            message,
            "panic"
        );
        previous(info);
    }));

    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        pid = std::process::id(),
        os = std::env::consts::OS,
        log_dir = %log_dir.display(),
        "FrameForge starting"
    );
}

/// Moves log files written before logs moved to `state_dir()/logs` out of the
/// Tauri identifier dir (the `com.` folder under the local app-data dir, in a
/// `logs` subfolder on Windows and Linux). Only that logs directory is
/// addressed; the sibling WebView2 profile stays put. Runs once per launch and
/// stops as soon as the old directory is gone.
///
/// The old identifier is hardcoded rather than read from the live config: every
/// release before the identifier was renamed to `com.wyrmstudios.frameforge`
/// shipped as `com.jochem.frameforge`. Deriving this path from Tauri's
/// `app_log_dir()` would resolve against *today's* identifier and never find
/// the files an upgrading install actually has on disk.
fn migrate_legacy_logs(log_dir: &std::path::Path) {
    const OLD_IDENTIFIER: &str = "com.jochem.frameforge";
    let Some(base) = dirs::data_local_dir() else { return; };
    let old_dir = base.join(OLD_IDENTIFIER).join("logs");
    if old_dir == log_dir || !old_dir.is_dir() {
        return;
    }
    let mut moved = 0usize;
    if let Ok(entries) = std::fs::read_dir(&old_dir) {
        for entry in entries.flatten() {
            let src = entry.path();
            if src.is_file() && crate::paths::move_file(&src, &log_dir.join(entry.file_name())) {
                moved += 1;
            }
        }
    }
    // Fails while anything else is left, which is the desired behaviour.
    let _ = std::fs::remove_dir(&old_dir);
    tracing::info!(
        from = %old_dir.display(),
        to = %log_dir.display(),
        moved,
        "migrated legacy logs"
    );
}

fn filter(default: &str) -> EnvFilter {
    EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default))
}
