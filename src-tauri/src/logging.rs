//! Subscriber setup: a compact daily-rolling file log plus a coloured console
//! log, both fed from the same `tracing` events.

use std::sync::OnceLock;

use tauri::Manager;
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
pub fn init(app: &tauri::AppHandle) {
    if FILE_GUARD.get().is_some() {
        return;
    }

    let log_dir = app
        .path()
        .app_log_dir()
        .expect("Tauri always resolves a log dir on supported platforms");
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

fn filter(default: &str) -> EnvFilter {
    EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default))
}
