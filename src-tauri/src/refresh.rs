//! Background refresh loop.
//!
//! One thread ticks every five seconds over a fixed table of tasks, each with
//! its own interval. A task that fails climbs a backoff ladder instead of
//! retrying on its normal schedule. An outage then costs a handful of requests
//! rather than one per interval. A success drops the task back to the ladder's foot.
//!
//! Every task is a cache-ladder call, so a run whose data is still fresh is a
//! disk read and nothing more.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter};
use tracing::warn;

use crate::cache;
use crate::events;

/// Seconds to wait after the 1st, 2nd, 3rd and any later consecutive failure.
const BACKOFF: [u64; 4] = [5, 30, 120, 300];

const TICK: Duration = Duration::from_secs(5);

/// How long to wait after `failures` consecutive failures, the last rung
/// repeating for as long as the outage lasts.
fn backoff_delay(failures: usize) -> Duration {
    Duration::from_secs(BACKOFF[failures.min(BACKOFF.len() - 1)])
}

struct Task {
    name: &'static str,
    interval: Duration,
    /// `false` = the first run is one full `interval` after launch instead of at
    /// the launch tick. The catalogue is the only such task: the frontend probes
    /// all 25 sources itself at mount, and a second pass seconds later would
    /// only walk them again.
    due_at_launch: bool,
    run: fn(&AppHandle, bool) -> Result<(), String>,
}

/// When `task` first becomes due, measured from launch.
fn first_due(task: &Task, now: Instant) -> Instant {
    if task.due_at_launch {
        now
    } else {
        now + task.interval
    }
}

const TASKS: &[Task] = &[
    Task {
        name: "arbitrations",
        interval: Duration::from_secs(3600),
        due_at_launch: true,
        run: crate::arbitrations::refresh_feed,
    },
    // Just under the 60s frontend poll, so a window's own tick is served from
    // the cache this fills rather than from the network.
    Task {
        name: "worldstate",
        interval: Duration::from_secs(55),
        due_at_launch: true,
        run: crate::worldstate::refresh_worldstate,
    },
    Task {
        name: "bulk-prices",
        interval: Duration::from_secs(3600),
        due_at_launch: true,
        run: crate::pricing::refresh_bulk_prices_task,
    },
    Task {
        name: "catalogue",
        interval: Duration::from_secs(24 * 3600),
        due_at_launch: false,
        run: crate::catalogue::refresh_catalogue,
    },
    Task {
        name: "drop-data",
        interval: Duration::from_secs(24 * 3600),
        due_at_launch: true,
        run: crate::wfcd::refresh_drop_data,
    },
    Task {
        name: "riven-db",
        interval: Duration::from_secs(24 * 3600),
        due_at_launch: true,
        run: crate::rivens::refresh_riven_db_task,
    },
    Task {
        name: "wfm-top",
        interval: Duration::from_secs(3 * 3600),
        due_at_launch: true,
        run: crate::wfm_top::refresh_wfm_top,
    },
];

/// Set by the manual refresh; consumed by the next tick, which then runs every
/// task at once and tells each one to ignore its ETag.
static FORCE_ALL: AtomicBool = AtomicBool::new(false);

pub fn force_all() {
    FORCE_ALL.store(true, Ordering::SeqCst);
}

pub fn spawn(app: AppHandle) {
    std::thread::spawn(move || {
        let now = Instant::now();
        let mut due: Vec<Instant> = TASKS.iter().map(|t| first_due(t, now)).collect();
        let mut failures: Vec<usize> = TASKS.iter().map(|_| 0).collect();

        loop {
            std::thread::sleep(TICK);
            let force = FORCE_ALL.swap(false, Ordering::SeqCst);

            for (i, task) in TASKS.iter().enumerate() {
                if !force && Instant::now() < due[i] {
                    continue;
                }

                // A panic in one fetcher must not take the whole loop, and
                // with it every other refresh, down with it.
                let result = catch_unwind(AssertUnwindSafe(|| (task.run)(&app, force)))
                    .unwrap_or_else(|_| Err(format!("{} refresh panicked", task.name)));

                match result {
                    Ok(()) => {
                        failures[i] = 0;
                        due[i] = Instant::now() + task.interval;
                    }
                    Err(e) => {
                        warn!(task = task.name, error = %e, "refresh failed");
                        due[i] = Instant::now() + backoff_delay(failures[i]);
                        failures[i] = failures[i].saturating_add(1);
                    }
                }

                let _ = app.emit(events::CACHE_STATUS, cache::statuses());
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ladder_climbs_once_and_then_holds() {
        assert_eq!(backoff_delay(0), Duration::from_secs(5));
        assert_eq!(backoff_delay(3), Duration::from_secs(300));
        assert_eq!(backoff_delay(99), Duration::from_secs(300));
    }
}
