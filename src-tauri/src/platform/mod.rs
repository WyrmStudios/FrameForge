//! Platform abstraction layer.
//!
//! Defines traits for OS-specific functionality and provides implementations
//! for Windows and Linux. This centralizes all platform-dependent code so
//! business logic stays platform-agnostic.

#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "linux")]
mod linux;

#[cfg(target_os = "windows")]
pub use windows::{file_identity, FileId};

// ─── Credential Store ─────────────────────────────────────────────────────────

/// Encrypted credential storage (OS keychain / credential manager).
///
/// Implementations may block on user interaction. A locked keychain prompts,
/// and the prompt is answered at human speed (or never). Call these from a
/// blocking context, not from a sync command handler.
pub trait CredentialStore {
    fn save_credentials(target: &str, email: &str, token: &str) -> Result<(), String>;
    fn load_credentials(target: &str) -> Result<Option<(String, String)>, String>;
    fn delete_credentials(target: &str) -> Result<(), String>;
}

// ─── Process Access ───────────────────────────────────────────────────────────

/// Find and interact with OS processes.
pub trait ProcessAccess {
    /// Find the PID of the Warframe process by name.
    fn find_warframe_pid() -> Option<u32>;

    /// Open a process handle for reading memory.
    fn open_process(pid: u32) -> Result<Box<dyn ProcessHandle>, String>;
}

/// An open handle to a process, allowing efficient repeated memory operations.
pub trait ProcessHandle {
    /// Reads into `buf` and returns how many bytes landed there. A short read
    /// is what a faulted page inside an otherwise readable region looks like,
    /// so the count is the whole answer: there is no "try again for the rest".
    fn read_into(&self, addr: usize, buf: &mut [u8]) -> usize;

    /// Reads `len` bytes, allocating a fresh buffer. Prefer `read_into` in
    /// hot loops; this is a convenience wrapper for one-off reads.
    fn read(&self, addr: usize, len: usize) -> Option<Vec<u8>> {
        let mut buf = vec![0u8; len];
        let n = self.read_into(addr, &mut buf);
        (n > 0).then(|| { buf.truncate(n); buf })
    }

    /// Maps regions at or after `from`, in ascending address order.
    /// Yields one region per step, as the OS queries it.
    fn regions_from(&self, from: usize) -> Box<dyn Iterator<Item = MemoryRegionInfo> + '_>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegionBacking {
    /// Private memory: the heap, the stack, plain anonymous mappings.
    /// On Windows this also covers `MEM_MAPPED` data files (not PE images),
    /// since they are first-tier candidates for heap scans.
    Anonymous,
    /// Backed by a file on disk. On Windows a PE image (`MEM_IMAGE`).
    /// `MEM_MAPPED` data files intentionally fall into `Anonymous` so they
    /// are not skipped during the heap scan — the Wine heap being anonymous
    /// is an implementation detail, not a guarantee.
    File,
    /// Pages the kernel provides that can never hold application data.
    ///
    /// On **Linux**: committed, readable pages like `[vvar]` and `[vsyscall]`.
    /// They are readable but cannot hold a heap allocation.
    ///
    /// On **Windows**: not reached in practice. Free/reserved address space
    /// is already filtered by `is_committed == false` before this arm is
    /// checked. Kept for completeness.
    Kernel,
}

#[derive(Debug, Clone)]
pub struct MemoryRegionInfo {
    pub base_address: usize,
    pub region_size: usize,
    pub is_committed: bool,
    pub is_readable: bool,
    pub is_writable: bool,
    pub is_executable: bool,
    pub backing: RegionBacking,
}

// ─── Locale ───────────────────────────────────────────────────────────────────

/// Get system locale information.
pub trait LocaleProvider {
    /// Get the user's default locale (e.g. "en-US", "nl-NL").
    fn get_system_locale() -> String;
}

// ─── COM Initialization ──────────────────────────────────────────────────────

/// Initialize COM (Component Object Model) for the current thread.
pub trait ComInit {
    fn initialize_com();
}

// ─── Window Management ────────────────────────────────────────────────────────

/// Interact with OS windows.
pub trait WindowManager {
    /// Find the Warframe window and return its client rect as [x, y, w, h].
    fn get_warframe_window_rect() -> Result<[i32; 4], String>;

    /// Force-set the overlay window to topmost position.
    fn set_overlay_topmost();
}

// ─── Platform selector ────────────────────────────────────────────────────────

/// The concrete platform implementation for the current OS.
#[cfg(target_os = "windows")]
pub struct Platform;

#[cfg(target_os = "windows")]
impl CredentialStore for Platform {
    fn save_credentials(target: &str, email: &str, token: &str) -> Result<(), String> {
        windows::save_credentials(target, email, token)
    }
    fn load_credentials(target: &str) -> Result<Option<(String, String)>, String> {
        windows::load_credentials(target)
    }
    fn delete_credentials(target: &str) -> Result<(), String> {
        windows::delete_credentials(target)
    }
}

#[cfg(target_os = "windows")]
impl ProcessAccess for Platform {
    fn find_warframe_pid() -> Option<u32> {
        windows::find_warframe_pid()
    }
    fn open_process(pid: u32) -> Result<Box<dyn ProcessHandle>, String> {
        windows::open_process(pid)
    }
}

#[cfg(target_os = "windows")]
impl LocaleProvider for Platform {
    fn get_system_locale() -> String {
        windows::get_system_locale()
    }
}

#[cfg(target_os = "windows")]
impl ComInit for Platform {
    fn initialize_com() {
        windows::initialize_com();
    }
}

#[cfg(target_os = "windows")]
impl WindowManager for Platform {
    fn get_warframe_window_rect() -> Result<[i32; 4], String> {
        windows::get_warframe_window_rect()
    }
    fn set_overlay_topmost() {
        windows::set_overlay_topmost()
    }
}

// ─── Linux stubs ──────────────────────────────────────────────────────────────

#[cfg(target_os = "linux")]
pub struct Platform;

#[cfg(target_os = "linux")]
impl CredentialStore for Platform {
    fn save_credentials(_target: &str, _email: &str, _token: &str) -> Result<(), String> {
        Err("Credentials not supported on Linux yet".into())
    }
    fn load_credentials(_target: &str) -> Result<Option<(String, String)>, String> {
        Ok(None)
    }
    fn delete_credentials(_target: &str) -> Result<(), String> {
        Ok(())
    }
}

#[cfg(target_os = "linux")]
impl ProcessAccess for Platform {
    fn find_warframe_pid() -> Option<u32> {
        linux::find_warframe_pid()
    }
    fn open_process(_pid: u32) -> Result<Box<dyn ProcessHandle>, String> {
        Err("Process access not supported on Linux yet".into())
    }
}

#[cfg(target_os = "linux")]
impl LocaleProvider for Platform {
    fn get_system_locale() -> String {
        linux::get_system_locale()
    }
}

#[cfg(target_os = "linux")]
impl ComInit for Platform {
    fn initialize_com() {}
}

#[cfg(target_os = "linux")]
impl WindowManager for Platform {
    fn get_warframe_window_rect() -> Result<[i32; 4], String> {
        Err("Window management not supported on Linux yet".into())
    }
    fn set_overlay_topmost() {}
}

// ─── Tauri command wrapper ────────────────────────────────────────────────────

#[tauri::command]
pub(crate) fn get_system_locale() -> String {
    <Platform as LocaleProvider>::get_system_locale()
}
