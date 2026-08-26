//! Where FrameForge keeps its files.
//!
//! Everything used to live in one directory next to the database. The four
//! functions below split it along the XDG roles instead: throwaway downloads in
//! the cache root, the user's settings in the config root, the database and the
//! auction IDs in the data root, and logs and debug dumps in the state root.
//! `migrate_legacy` carries the irreplaceable files over from the old layout.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use tracing::{info, warn};

const APP_DIR: &str = "frameforge";

/// Layout used before the XDG split, still on disk for anyone upgrading.
const LEGACY_DIR: &str = "warframe-companion";

/// When set, all four roots become subdirectories of this path. Tests use it to
/// keep off the real user directories.
static ROOT_OVERRIDE: OnceLock<PathBuf> = OnceLock::new();

/// Fails if a root has already been chosen, which is why tests share one.
#[cfg(test)]
pub fn set_root_override(root: PathBuf) -> Result<(), PathBuf> {
    ROOT_OVERRIDE.set(root)
}

/// Downloaded catalogues, price snapshots, item images, OCR models: anything
/// that can be fetched again.
pub fn cache_dir() -> PathBuf {
    let dir = ensure(match ROOT_OVERRIDE.get() {
        Some(root) => root.join("cache"),
        None => base(dirs::cache_dir()),
    });
    // Tells backup and sync tools to skip the directory
    // (https://bford.info/cachedir/).
    let tag = dir.join("CACHEDIR.TAG");
    if !tag.exists() {
        let _ = fs::write(
            &tag,
            "Signature: 8a477f597d28d172789f06886806bc55\n\
             # This file is a cache directory tag created by FrameForge.\n\
             # For information about cache directory tags see https://bford.info/cachedir/\n",
        );
    }
    dir
}

/// settings.json.
pub fn config_dir() -> PathBuf {
    ensure(match ROOT_OVERRIDE.get() {
        Some(root) => root.join("config"),
        None => base(dirs::config_dir()),
    })
}

/// data.db and the WFM auction IDs: user state that no refetch can rebuild.
pub fn data_dir() -> PathBuf {
    ensure(match ROOT_OVERRIDE.get() {
        Some(root) => root.join("data"),
        None => base(dirs::data_dir()),
    })
}

/// Logs, session transcripts, screenshot dumps.
pub fn state_dir() -> PathBuf {
    ensure(match ROOT_OVERRIDE.get() {
        Some(root) => root.join("state"),
        // Only Linux defines a state directory; elsewhere this material sits
        // with the rest of the app data.
        None => base(dirs::state_dir().or_else(dirs::data_dir)),
    })
}

fn base(dir: Option<PathBuf>) -> PathBuf {
    dir.unwrap_or_else(|| PathBuf::from(".")).join(APP_DIR)
}

fn ensure(dir: PathBuf) -> PathBuf {
    if let Err(e) = fs::create_dir_all(&dir) {
        warn!("cannot create {}: {e}", dir.display());
    }
    dir
}

fn legacy_root() -> PathBuf {
    match ROOT_OVERRIDE.get() {
        Some(root) => root.join("legacy"),
        None => dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(LEGACY_DIR),
    }
}

/// Caches the old layout kept beside the user's real files. These caches are
/// deleted rather than moved. Each one refills itself on the next fetch.
const LEGACY_CACHE_FILES: &[&str] = &[
    "items_cache.json",
    "recipes_cache.json",
    "relic_drops_cache.json",
    "relic_rewards_cache.json",
    "wfm_top_cache.json",
    "syndicate_catalog.json",
    "relics_run_prices.json",
];

const LEGACY_CACHE_DIRS: &[&str] = &["img_cache", "ocr_models"];

/// Move the irreplaceable files out of the pre-XDG directory, then clear what is
/// left of its caches. Doing nothing when the old directory is gone makes this
/// safe to call on every launch, so no marker file records that it ran.
pub fn migrate_legacy() {
    migrate_into(&legacy_root(), &config_dir(), &data_dir(), &cache_dir());
}

fn migrate_into(old: &Path, config: &Path, data: &Path, cache: &Path) {
    if !old.is_dir() {
        return;
    }
    info!("migrating from {}", old.display());

    move_file(&old.join("settings.json"), &config.join("settings.json"));
    move_file(
        &old.join("corrections.json"),
        &config.join("corrections.json"),
    );
    move_file(
        &old.join("auction_ids.json"),
        &data.join("auction_ids.json"),
    );
    // Named like caches and stored with them, but only a live game scan can
    // produce them again, so they travel rather than get dropped.
    move_file(
        &old.join("quantities_cache.json"),
        &cache.join("quantities_cache.json"),
    );
    move_file(
        &old.join("inventory_state_cache.json"),
        &cache.join("inventory_state_cache.json"),
    );
    move_db(old, data);

    for name in LEGACY_CACHE_FILES {
        let _ = fs::remove_file(old.join(name));
    }
    for name in LEGACY_CACHE_DIRS {
        let _ = fs::remove_dir_all(old.join(name));
    }
    // Anything unrecognised still in there keeps the old directory alive; the
    // user can look at what is left and delete it themselves.
    let _ = fs::remove_dir(old);
}

/// SQLite's write-ahead log and shared-memory files only make sense next to the
/// database they belong to, so either all three arrive or none do.
fn move_db(old: &Path, data: &Path) {
    let db = old.join("data.db");
    if !db.exists() || data.join("data.db").exists() {
        return;
    }
    if !move_file(&db, &data.join("data.db")) {
        return;
    }
    for suffix in ["data.db-wal", "data.db-shm"] {
        let src = old.join(suffix);
        if !src.exists() {
            continue;
        }
        if !move_file(&src, &data.join(suffix)) {
            warn!("rolling the database back to {}", old.display());
            let _ = move_file(&data.join("data.db"), &db);
            for done in ["data.db-wal", "data.db-shm"] {
                let _ = move_file(&data.join(done), &old.join(done));
            }
            return;
        }
    }
}

/// Returns whether `dst` now holds the file. An existing destination is left
/// untouched and reported as success: the new location wins, and the stale
/// copy stays where it is for the user to inspect.
fn move_file(src: &Path, dst: &Path) -> bool {
    if !src.exists() {
        return false;
    }
    if dst.exists() {
        warn!("{} already exists, keeping it", dst.display());
        return true;
    }
    match fs::rename(src, dst) {
        Ok(()) => true,
        // A rename across filesystems fails, which is the normal case when the
        // XDG roots are on different mounts.
        Err(_) => match copy_then_delete(src, dst) {
            Ok(()) => true,
            Err(e) => {
                warn!("cannot move {} to {}: {e}", src.display(), dst.display());
                false
            }
        },
    }
}

fn copy_then_delete(src: &Path, dst: &Path) -> io::Result<()> {
    fs::copy(src, dst)?;
    // The delete below makes the copy the only remaining version, so it has to
    // be on the platter before then, directory entry included: a power loss
    // right after the unlink would lose the file despite its synced contents.
    fs::File::open(dst)?.sync_all()?;
    #[cfg(unix)]
    if let Some(dir) = dst.parent() {
        fs::File::open(dir)?.sync_all()?;
    }
    fs::remove_file(src)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join("frameforge-paths-tests")
            .join(name);
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("old")).unwrap();
        fs::create_dir_all(dir.join("config")).unwrap();
        fs::create_dir_all(dir.join("data")).unwrap();
        fs::create_dir_all(dir.join("cache")).unwrap();
        dir
    }

    fn write(path: PathBuf, body: &str) {
        fs::write(path, body).unwrap();
    }

    #[test]
    fn moves_user_state_and_drops_caches() {
        let dir = scratch("moves");
        let (old, config, data, cache) = (
            dir.join("old"),
            dir.join("config"),
            dir.join("data"),
            dir.join("cache"),
        );
        write(old.join("settings.json"), "{}");
        write(old.join("auction_ids.json"), "[]");
        write(old.join("corrections.json"), "[]");
        write(old.join("inventory_state_cache.json"), "inv");
        write(old.join("quantities_cache.json"), "qty");
        write(old.join("data.db"), "db");
        write(old.join("data.db-wal"), "wal");
        write(old.join("items_cache.json"), "[]");
        fs::create_dir_all(old.join("img_cache")).unwrap();

        migrate_into(&old, &config, &data, &cache);

        assert_eq!(
            fs::read_to_string(config.join("settings.json")).unwrap(),
            "{}"
        );
        assert_eq!(
            fs::read_to_string(data.join("auction_ids.json")).unwrap(),
            "[]"
        );
        assert_eq!(fs::read_to_string(data.join("data.db")).unwrap(), "db");
        assert_eq!(fs::read_to_string(data.join("data.db-wal")).unwrap(), "wal");
        assert_eq!(
            fs::read_to_string(config.join("corrections.json")).unwrap(),
            "[]"
        );
        assert_eq!(
            fs::read_to_string(cache.join("inventory_state_cache.json")).unwrap(),
            "inv"
        );
        assert_eq!(
            fs::read_to_string(cache.join("quantities_cache.json")).unwrap(),
            "qty"
        );
        assert!(!old.join("items_cache.json").exists());
        assert!(!old.join("img_cache").exists());
        assert!(!old.exists(), "emptied old directory should be gone");
    }

    #[test]
    fn never_overwrites_the_destination() {
        let dir = scratch("no-overwrite");
        let (old, config, data, cache) = (
            dir.join("old"),
            dir.join("config"),
            dir.join("data"),
            dir.join("cache"),
        );
        write(old.join("settings.json"), "old");
        write(config.join("settings.json"), "new");
        write(old.join("data.db"), "old-db");
        write(data.join("data.db"), "new-db");

        migrate_into(&old, &config, &data, &cache);

        assert_eq!(
            fs::read_to_string(config.join("settings.json")).unwrap(),
            "new"
        );
        assert_eq!(fs::read_to_string(data.join("data.db")).unwrap(), "new-db");
        assert_eq!(
            fs::read_to_string(old.join("settings.json")).unwrap(),
            "old"
        );
    }

    #[test]
    fn keeps_the_old_directory_when_something_is_left_in_it() {
        let dir = scratch("leftovers");
        let (old, config, data, cache) = (
            dir.join("old"),
            dir.join("config"),
            dir.join("data"),
            dir.join("cache"),
        );
        write(old.join("settings.json"), "{}");
        write(old.join("scan_log.txt"), "log");

        migrate_into(&old, &config, &data, &cache);

        assert!(old.join("scan_log.txt").exists());
        assert!(old.exists());
    }

    #[test]
    fn does_nothing_without_an_old_directory() {
        let dir = scratch("absent");
        fs::remove_dir_all(dir.join("old")).unwrap();
        migrate_into(
            &dir.join("old"),
            &dir.join("config"),
            &dir.join("data"),
            &dir.join("cache"),
        );
        assert!(!dir.join("config").join("settings.json").exists());
    }

    #[test]
    fn a_second_run_changes_nothing() {
        let dir = scratch("idempotent");
        let (old, config, data, cache) = (
            dir.join("old"),
            dir.join("config"),
            dir.join("data"),
            dir.join("cache"),
        );
        write(old.join("settings.json"), "{}");

        migrate_into(&old, &config, &data, &cache);
        migrate_into(&old, &config, &data, &cache);

        assert_eq!(
            fs::read_to_string(config.join("settings.json")).unwrap(),
            "{}"
        );
    }
}
