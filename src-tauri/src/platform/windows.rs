//! Windows platform implementation.

use super::*;

// ─── Credential Store ─────────────────────────────────────────────────────────

pub fn save_credentials(target: &str, email: &str, token: &str) -> Result<(), String> {
    use windows_sys::Win32::Security::Credentials::{
        CredWriteW, CREDENTIALW, CRED_TYPE_GENERIC, CRED_PERSIST_LOCAL_MACHINE,
    };
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    let target_wide: Vec<u16> = OsStr::new(target).encode_wide().chain(Some(0)).collect();
    let user_wide: Vec<u16> = OsStr::new(email).encode_wide().chain(Some(0)).collect();
    let token_bytes = token.as_bytes();

    let cred = CREDENTIALW {
        Flags: 0,
        Type: CRED_TYPE_GENERIC,
        TargetName: target_wide.as_ptr() as *mut _,
        Comment: std::ptr::null_mut(),
        LastWritten: unsafe { std::mem::zeroed() },
        CredentialBlobSize: token_bytes.len() as u32,
        CredentialBlob: token_bytes.as_ptr() as *mut _,
        Persist: CRED_PERSIST_LOCAL_MACHINE,
        AttributeCount: 0,
        Attributes: std::ptr::null_mut(),
        TargetAlias: std::ptr::null_mut(),
        UserName: user_wide.as_ptr() as *mut _,
    };
    let ok = unsafe { CredWriteW(&cred, 0) };
    if ok == 0 { Err("Failed to save to Windows Credential Manager".into()) } else { Ok(()) }
}

pub fn load_credentials(target: &str) -> Result<Option<(String, String)>, String> {
    use windows_sys::Win32::Security::Credentials::{
        CredReadW, CredFree, CREDENTIALW, CRED_TYPE_GENERIC,
    };
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use std::slice;

    let target_wide: Vec<u16> = OsStr::new(target).encode_wide().chain(Some(0)).collect();
    let mut cred_ptr: *mut CREDENTIALW = std::ptr::null_mut();
    let ok = unsafe { CredReadW(target_wide.as_ptr(), CRED_TYPE_GENERIC, 0, &mut cred_ptr) };
    if ok == 0 || cred_ptr.is_null() { return Ok(None); }

    let cred = unsafe { &*cred_ptr };
    let email = unsafe {
        let ptr = cred.UserName;
        if ptr.is_null() { String::new() } else {
            let len = (0..).take_while(|&i| *ptr.offset(i) != 0).count();
            String::from_utf16_lossy(slice::from_raw_parts(ptr, len))
        }
    };
    let token = unsafe {
        if cred.CredentialBlob.is_null() || cred.CredentialBlobSize == 0 { String::new() } else {
            String::from_utf8_lossy(slice::from_raw_parts(cred.CredentialBlob, cred.CredentialBlobSize as usize)).to_string()
        }
    };
    unsafe { CredFree(cred_ptr as *mut _); }
    Ok(Some((email, token)))
}

pub fn delete_credentials(target: &str) -> Result<(), String> {
    use windows_sys::Win32::Security::Credentials::{CredDeleteW, CRED_TYPE_GENERIC};
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    let target_wide: Vec<u16> = OsStr::new(target).encode_wide().chain(Some(0)).collect();
    unsafe { CredDeleteW(target_wide.as_ptr(), CRED_TYPE_GENERIC, 0); }
    Ok(())
}

// ─── Process Access ───────────────────────────────────────────────────────────

pub fn find_warframe_pid() -> Option<u32> {
    use std::mem;
    use windows_sys::Win32::{
        Foundation::{CloseHandle, INVALID_HANDLE_VALUE},
        System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Process32First, Process32Next,
            PROCESSENTRY32, TH32CS_SNAPPROCESS,
        },
    };
    // CreateToolhelp32Snapshot can fail sporadically while processes are
    // spawning/exiting (e.g. during Warframe startup via the launcher). Only a
    // failed snapshot is worth retrying; a successful enumeration without a
    // match genuinely means "not found".
    for _ in 0..3 {
        let found = unsafe {
            let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if snapshot == INVALID_HANDLE_VALUE {
                None::<Option<u32>>
            } else {
                let mut entry: PROCESSENTRY32 = mem::zeroed();
                entry.dwSize = mem::size_of::<PROCESSENTRY32>() as u32;

                let mut found = None;
                if Process32First(snapshot, &mut entry) != 0 {
                    loop {
                        let name_len = entry.szExeFile.iter().position(|&b| b == 0).unwrap_or(260);
                        let name = String::from_utf8_lossy(&entry.szExeFile[..name_len]).to_lowercase();
                        if is_game_process(&name) {
                            found = Some(entry.th32ProcessID);
                            break;
                        }
                        if Process32Next(snapshot, &mut entry) == 0 { break; }
                    }
                }
                CloseHandle(snapshot);
                Some(found)
            }
        };
        match found {
            // Snapshot succeeded (with or without a match): definitive result.
            Some(pid) => return pid,
            // Snapshot failed: wait briefly and retry.
            None => std::thread::sleep(std::time::Duration::from_millis(50)),
        }
    }
    None
}

/// True for the actual game process; launcher/helper processes excluded.
fn is_game_process(name: &str) -> bool {
    name.starts_with("warframe")
        && !name.contains("launcher")
        && !name.contains("companion")
        && !name.contains("crash")
        && !name.contains("downloader")
        && !name.contains("installer")
        && !name.contains("updater")
}

// ─── Process Handle ──────────────────────────────────────────────────────────

pub struct WindowsProcessHandle {
    handle: isize,
}

impl Drop for WindowsProcessHandle {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.handle);
        }
    }
}

impl ProcessHandle for WindowsProcessHandle {
    fn read_into(&self, addr: usize, buf: &mut [u8]) -> usize {
        use std::ffi::c_void;
        use windows_sys::Win32::System::Diagnostics::Debug::ReadProcessMemory;

        unsafe {
            let mut n = 0usize;
            let ok = ReadProcessMemory(
                self.handle,
                addr as *const c_void,
                buf.as_mut_ptr() as *mut c_void,
                buf.len(),
                &mut n,
            );
            if ok == 0 { 0 } else { n }
        }
    }

    fn regions_from(&self, from: usize) -> Box<dyn Iterator<Item = MemoryRegionInfo> + '_> {
        Box::new(RegionIter { handle: self, addr: from })
    }
}

struct RegionIter<'a> {
    handle: &'a WindowsProcessHandle,
    addr: usize,
}

impl Iterator for RegionIter<'_> {
    type Item = MemoryRegionInfo;

    fn next(&mut self) -> Option<MemoryRegionInfo> {
        use std::ffi::c_void;
        use std::mem;
        use windows_sys::Win32::System::Memory::{
            VirtualQueryEx, MEMORY_BASIC_INFORMATION, MEM_COMMIT,
        };

        unsafe {
            let mut mbi: MEMORY_BASIC_INFORMATION = mem::zeroed();
            if VirtualQueryEx(
                self.handle.handle,
                self.addr as *const c_void,
                &mut mbi,
                mem::size_of::<MEMORY_BASIC_INFORMATION>(),
            ) == 0 {
                return None;
            }

            let region_end = (mbi.BaseAddress as usize).saturating_add(mbi.RegionSize);
            if region_end <= self.addr { return None; }
            self.addr = region_end;

            let protect = mbi.Protect;
            Some(MemoryRegionInfo {
                base_address: mbi.BaseAddress as usize,
                region_size: mbi.RegionSize,
                is_committed: mbi.State == MEM_COMMIT,
                is_readable: mbi.State == MEM_COMMIT
                    && (protect & 0x02 != 0 || protect & 0x04 != 0 || protect & 0x08 != 0
                        || protect & 0x20 != 0 || protect & 0x40 != 0 || protect & 0x80 != 0),
                is_executable: protect & 0x10 != 0 || protect & 0x20 != 0 || protect & 0x40 != 0,
                backing: if mbi.Type == 0x1000000 {
                    RegionBacking::File
                } else if mbi.State != MEM_COMMIT {
                    RegionBacking::Kernel
                } else {
                    RegionBacking::Anonymous
                },
            })
        }
    }
}

pub fn open_process(pid: u32) -> Result<Box<dyn ProcessHandle>, String> {
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ};

    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, 0, pid);
        if handle == 0 {
            let err = windows_sys::Win32::Foundation::GetLastError();
            return Err(format!("OpenProcess failed (error {})", err));
        }
        Ok(Box::new(WindowsProcessHandle { handle }))
    }
}

// ─── Screen Capture ───────────────────────────────────────────────────────────
// Delegated to crate::ocr::windows — ocr is self-contained with its own
// Windows capture code, no circular dependency needed.

// ─── OCR Engine ───────────────────────────────────────────────────────────────
// Delegated to crate::ocr::windows — same as above.

// ─── Locale ───────────────────────────────────────────────────────────────────

pub fn get_system_locale() -> String {
    let mut buf = [0u16; 85]; // LOCALE_NAME_MAX_LENGTH
    let len = unsafe {
        windows_sys::Win32::Globalization::GetUserDefaultLocaleName(
            buf.as_mut_ptr(),
            buf.len() as i32,
        )
    };
    if len > 1 {
        String::from_utf16_lossy(&buf[..(len as usize - 1)])
    } else {
        "en-US".to_string()
    }
}

pub fn initialize_com() {
    unsafe {
        windows_sys::Win32::System::Com::CoInitializeEx(
            std::ptr::null(),
            windows_sys::Win32::System::Com::COINIT_MULTITHREADED
                .try_into()
                .unwrap(),
        );
    }
}

// ─── Window Management ────────────────────────────────────────────────────────

pub fn get_warframe_window_rect() -> Result<[i32; 4], String> {
    use windows_sys::Win32::Foundation::{POINT, RECT};
    use windows_sys::Win32::UI::WindowsAndMessaging::{FindWindowW, GetClientRect};
    use windows_sys::Win32::Graphics::Gdi::ClientToScreen;

    let title: Vec<u16> = "Warframe\0".encode_utf16().collect();
    let hwnd = unsafe { FindWindowW(std::ptr::null(), title.as_ptr()) };
    if hwnd == 0 { return Err("Warframe window not found".into()); }

    let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    unsafe { GetClientRect(hwnd, &mut r) };
    let mut origin = POINT { x: 0, y: 0 };
    unsafe { ClientToScreen(hwnd, &mut origin) };

    Ok([origin.x, origin.y, r.right - r.left, r.bottom - r.top])
}

pub fn set_overlay_topmost() {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        FindWindowW, SetWindowPos,
        SWP_NOMOVE, SWP_NOSIZE, SWP_NOACTIVATE,
        HWND_TOPMOST,
    };

    let title: Vec<u16> = "FrameForge Overlay\0".encode_utf16().collect();
    let hwnd = unsafe { FindWindowW(std::ptr::null(), title.as_ptr()) };
    if hwnd != 0 {
        unsafe {
            SetWindowPos(hwnd, HWND_TOPMOST, 0, 0, 0, 0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
        }
    }
}
