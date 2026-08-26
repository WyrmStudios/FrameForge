//! Disk cache with a stale-while-revalidate ladder.
//!
//! Every cached payload carries the time it was retrieved and the ETag it came
//! with, so freshness is a property of the file rather than of its mtime, and a
//! conditional GET can be built from it. `get_or_refresh` walks four rungs in
//! order (fresh copy, successful refetch, stale copy, nothing) and reports
//! which one answered so the UI can say how old what it shows is.
//!
//! The schema version belongs in the file name ("catalogue-v1.json"): a payload
//! whose shape changed simply misses instead of deserializing into the wrong
//! thing.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use tracing::warn;

use crate::paths;

pub fn atomic_write(path: &std::path::Path, data: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, data)?;
    // Without this the rename can hit the platter before the data does, and a
    // power loss leaves a complete-looking file full of zeroes.
    std::fs::File::open(&tmp)
        .and_then(|f| f.sync_all())
        .inspect_err(|_| {
            let _ = std::fs::remove_file(&tmp);
        })?;
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

#[derive(Serialize, Deserialize)]
pub struct Cached<T> {
    pub retrieved_at_unix: u64,
    pub etag: Option<String>,
    pub data: T,
}

/// Which rung of the ladder produced the data the caller is holding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    /// Cache was inside its TTL, or the server confirmed it with a 304.
    Fresh,
    Refreshed,
    /// Cache is past its TTL and the refetch failed.
    Stale,
    /// Nothing on disk and the fetch failed. The caller has to invent something.
    Fallback,
}

pub enum Fetched<T> {
    New(T, Option<String>),
    NotModified,
}

#[derive(Debug, Clone, Serialize)]
pub struct CacheStatus {
    pub source: Source,
    pub last_updated: Option<u64>,
    pub warning: Option<String>,
}

static STATUSES: Mutex<Option<HashMap<String, CacheStatus>>> = Mutex::new(None);

/// One lock per cache name, held for the length of a `get_or_refresh`. Two
/// callers after the same cache take turns, and the second one finds what the
/// first stored. A slow download of one cache never blocks another.
static REFRESHING: Mutex<Option<HashMap<String, Arc<Mutex<()>>>>> = Mutex::new(None);

fn refresh_lock(name: &str) -> Arc<Mutex<()>> {
    let mut guard = REFRESHING.lock().unwrap_or_else(|e| e.into_inner());
    guard
        .get_or_insert_with(HashMap::new)
        .entry(name.to_string())
        .or_default()
        .clone()
}

pub fn set_status(name: &str, status: CacheStatus) {
    if let Ok(mut guard) = STATUSES.lock() {
        guard
            .get_or_insert_with(HashMap::new)
            .insert(name.to_string(), status);
    }
}

pub fn statuses() -> HashMap<String, CacheStatus> {
    STATUSES
        .lock()
        .ok()
        .and_then(|g| g.clone())
        .unwrap_or_default()
}

fn path_of(name: &str) -> PathBuf {
    paths::cache_dir().join(name)
}

pub fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn load<T: DeserializeOwned>(name: &str) -> Option<Cached<T>> {
    let body = std::fs::read_to_string(path_of(name)).ok()?;
    match serde_json::from_str(&body) {
        Ok(cached) => Some(cached),
        Err(e) => {
            warn!("discarding unreadable cache {name}: {e}");
            None
        }
    }
}

pub fn store<T: Serialize>(name: &str, etag: Option<String>, data: &T) -> std::io::Result<()> {
    let cached = Cached {
        retrieved_at_unix: now_unix(),
        etag,
        data,
    };
    let body = serde_json::to_vec(&cached)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    atomic_write(&path_of(name), &body)
}

/// Serve `name`, refetching when it is older than `ttl`.
///
/// `fetch` receives the cached ETag so it can ask the server whether anything
/// changed. Returns the data, the rung that supplied it, and, when the answer
/// is not current, what went wrong, for the caller to surface.
pub fn get_or_refresh<T>(
    name: &str,
    ttl: Duration,
    fetch: impl FnOnce(Option<&str>) -> Result<Fetched<T>, String>,
) -> (Option<T>, Source, Option<String>)
where
    T: Serialize + DeserializeOwned,
{
    // The frontend and the background scheduler both ask for the same caches at
    // launch. Without this they download the catalogue twice and race each other
    // writing the same temporary file.
    let lock = refresh_lock(name);
    let _refreshing = lock.lock().unwrap_or_else(|e| e.into_inner());

    let cached = load::<T>(name);
    let now = now_unix();

    let still_fresh = cached
        .as_ref()
        .is_some_and(|c| now.saturating_sub(c.retrieved_at_unix) < ttl.as_secs());
    if still_fresh {
        let c = cached.expect("still_fresh is only true for a loaded cache");
        return report(
            name,
            Some(c.retrieved_at_unix),
            Source::Fresh,
            None,
            Some(c.data),
        );
    }

    let result = fetch(cached.as_ref().and_then(|c| c.etag.as_deref()));
    match result {
        Ok(Fetched::New(data, etag)) => {
            if let Err(e) = store(name, etag, &data) {
                warn!("cannot write cache {name}: {e}");
            }
            report(name, Some(now), Source::Refreshed, None, Some(data))
        }
        // 304 only means anything against a cached copy; without one there is
        // nothing to confirm, and the fetcher had no ETag to send in the first
        // place.
        Ok(Fetched::NotModified) => match cached {
            Some(c) => {
                if let Err(e) = store(name, c.etag, &c.data) {
                    warn!("cannot refresh cache timestamp {name}: {e}");
                }
                report(name, Some(now), Source::Fresh, None, Some(c.data))
            }
            None => {
                let warning = format!("{name}: server reported not-modified with no cached copy");
                report(name, None, Source::Fallback, Some(warning), None)
            }
        },
        Err(e) => match cached {
            Some(c) => {
                let warning = format!("{name}: showing cached data, refresh failed: {e}");
                report(
                    name,
                    Some(c.retrieved_at_unix),
                    Source::Stale,
                    Some(warning),
                    Some(c.data),
                )
            }
            None => {
                let warning = format!("{name}: no cached data and refresh failed: {e}");
                report(name, None, Source::Fallback, Some(warning), None)
            }
        },
    }
}

fn report<T>(
    name: &str,
    last_updated: Option<u64>,
    source: Source,
    warning: Option<String>,
    data: Option<T>,
) -> (Option<T>, Source, Option<String>) {
    set_status(
        name,
        CacheStatus {
            source,
            last_updated,
            warning: warning.clone(),
        },
    );
    (data, source, warning)
}

/// Bodies past this are refused as runaway responses. The catalogue sources are
/// the largest bodies we pull, and All.json alone is ~30 MB.
const MAX_BODY_BYTES: u64 = 256 * 1024 * 1024;

/// GET `url`, asking the server to skip the body when `etag` still matches.
pub fn get_conditional(url: &str, etag: Option<&str>) -> Result<Fetched<String>, String> {
    let mut req = ureq::get(url)
        .set(
            "User-Agent",
            concat!("FrameForge/", env!("CARGO_PKG_VERSION")),
        )
        // Generous because of the body sizes, but bounded: ureq has no default
        // timeout, and a black-holed connection here would otherwise hang the
        // caller, and with it the refresh lock, forever.
        .timeout(std::time::Duration::from_secs(300));
    if let Some(tag) = etag {
        req = req.set("If-None-Match", tag);
    }
    match req.call() {
        Ok(resp) => {
            let etag = resp.header("etag").map(str::to_string);
            // Not `into_string()`: ureq caps that at 10 MB.
            use std::io::Read;
            // Capped so a lying Content-Length cannot make us allocate the
            // whole cap up front.
            let hint = resp
                .header("content-length")
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(0)
                .min(64 * 1024 * 1024);
            let mut body = Vec::with_capacity(hint);
            // One byte past the cap so a body of exactly the cap's size is
            // distinguishable from a truncated one.
            resp.into_reader()
                .take(MAX_BODY_BYTES + 1)
                .read_to_end(&mut body)
                .map_err(|e| e.to_string())?;
            if body.len() as u64 > MAX_BODY_BYTES {
                return Err(format!(
                    "{url}: response body exceeds {MAX_BODY_BYTES} bytes"
                ));
            }
            let body = String::from_utf8(body).map_err(|e| e.to_string())?;
            Ok(Fetched::New(body, etag))
        }
        Err(ureq::Error::Status(304, _)) => Ok(Fetched::NotModified),
        Err(e) => Err(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One process, one root: every test names its own cache file instead.
    fn scratch(name: &str) -> String {
        let root = std::env::temp_dir().join("frameforge-cache-tests");
        let _ = paths::set_root_override(root);
        let file = format!("{name}.json");
        let _ = std::fs::remove_file(paths::cache_dir().join(&file));
        file
    }

    fn fetches(body: &str) -> impl FnOnce(Option<&str>) -> Result<Fetched<String>, String> + '_ {
        move |_| Ok(Fetched::New(body.to_string(), None))
    }

    fn fails(_: Option<&str>) -> Result<Fetched<String>, String> {
        Err("offline".to_string())
    }

    #[test]
    fn fresh_cache_skips_the_fetch() {
        let name = scratch("fresh");
        store(&name, None, &"cached".to_string()).unwrap();

        let (data, source, warning) =
            get_or_refresh::<String>(&name, Duration::from_secs(3600), |_| {
                panic!("a fresh cache must not be refetched")
            });

        assert_eq!(data.as_deref(), Some("cached"));
        assert_eq!(source, Source::Fresh);
        assert!(warning.is_none());
    }

    #[test]
    fn expired_cache_is_replaced_by_the_fetch() {
        let name = scratch("refreshed");
        store(&name, None, &"old".to_string()).unwrap();

        let (data, source, _) = get_or_refresh(&name, Duration::ZERO, fetches("new"));

        assert_eq!(data.as_deref(), Some("new"));
        assert_eq!(source, Source::Refreshed);
        assert_eq!(load::<String>(&name).unwrap().data, "new");
    }

    #[test]
    fn a_failed_refresh_still_serves_the_stale_copy() {
        let name = scratch("stale");
        store(&name, None, &"old".to_string()).unwrap();

        let (data, source, warning) = get_or_refresh(&name, Duration::ZERO, fails);

        assert_eq!(data.as_deref(), Some("old"));
        assert_eq!(source, Source::Stale);
        assert!(warning.unwrap().contains("offline"));
    }

    #[test]
    fn nothing_cached_and_no_network_leaves_the_caller_empty() {
        let name = scratch("fallback");

        let (data, source, warning) = get_or_refresh::<String>(&name, Duration::ZERO, fails);

        assert!(data.is_none());
        assert_eq!(source, Source::Fallback);
        assert!(warning.is_some());
    }

    #[test]
    fn not_modified_keeps_the_payload_and_clears_the_staleness() {
        let name = scratch("not-modified");
        store(&name, Some("abc".to_string()), &"body".to_string()).unwrap();
        let before = load::<String>(&name).unwrap().retrieved_at_unix;

        let seen_etag = Mutex::new(None);
        let (data, source, _) = get_or_refresh(&name, Duration::ZERO, |etag| {
            *seen_etag.lock().unwrap() = etag.map(str::to_string);
            Ok(Fetched::<String>::NotModified)
        });

        assert_eq!(seen_etag.into_inner().unwrap().as_deref(), Some("abc"));
        assert_eq!(data.as_deref(), Some("body"));
        assert_eq!(source, Source::Fresh);
        let after = load::<String>(&name).unwrap();
        assert_eq!(after.etag.as_deref(), Some("abc"));
        assert!(after.retrieved_at_unix >= before);
    }

    #[test]
    fn a_new_schema_version_misses_the_old_file() {
        let v1 = scratch("schema-v1");
        let v2 = scratch("schema-v2");
        store(&v1, None, &"old shape".to_string()).unwrap();

        let (data, source, _) =
            get_or_refresh(&v2, Duration::from_secs(3600), fetches("new shape"));

        assert_eq!(data.as_deref(), Some("new shape"));
        assert_eq!(source, Source::Refreshed);
    }

    #[test]
    fn two_callers_after_a_cold_cache_fetch_once() {
        let name = scratch("concurrent");
        static FETCHES: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

        std::thread::scope(|s| {
            for _ in 0..2 {
                s.spawn(|| {
                    get_or_refresh(&name, Duration::from_secs(3600), |_| {
                        FETCHES.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        std::thread::sleep(Duration::from_millis(50));
                        Ok(Fetched::New("body".to_string(), None))
                    })
                });
            }
        });

        assert_eq!(FETCHES.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[test]
    fn status_records_the_last_rung_taken() {
        let name = scratch("status");

        let _ = get_or_refresh::<String>(&name, Duration::ZERO, fails);

        let status = statuses().remove(&name).expect("status recorded");
        assert_eq!(status.source, Source::Fallback);
        assert!(status.warning.is_some());
    }
}
