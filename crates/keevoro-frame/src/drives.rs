//! Hearing that a drive arrived or left, and which disk a folder is on.
//!
//! The first screen offers the drives plugged in by name. It looks again by
//! itself every couple of seconds while it is showing (that check is cheap
//! and needs nothing from here), and on Windows the system's own notice
//! makes it look at once: Windows tells every top-level window when a
//! volume arrives or is removed (`WM_DEVICECHANGE` with
//! `DBT_DEVICEARRIVAL` or `DBT_DEVICEREMOVECOMPLETE`, for a
//! `DBT_DEVTYP_VOLUME`), including a memory card pushed into a reader that
//! was already plugged in. A message-only window is not told of such
//! broadcasts, so the window here is an ordinary top-level one that is
//! never shown, on a thread of its own with its own message loop.
//!
//! On macOS and Linux the notices need a run loop (Disk Arbitration) or a
//! device library (udev) that would bring far more of this crate's sort of
//! code for no gain over the check every couple of seconds, which reads one
//! small list the system keeps in memory. So there [`on_change`] answers
//! false and the check alone does the work.
//!
//! [`volume_of`] names the volume a folder is on, so a backup folder whose
//! drive was unplugged, leaving an empty folder behind on the computer's
//! own disk, is not taken for the backup's drive. On macOS and Linux the
//! standard library says this without calling the system directly (the
//! folder's device number), so it is not here; on Windows it is the
//! volume's own name, `\\?\Volume{...}\`, which stays the same whatever
//! letter the drive is given.

use std::path::Path;

/// What is told that a drive arrived or left.
pub type Notice = Box<dyn Fn() + Send + Sync>;

/// Asks to be told, by `notice`, each time a drive arrives or leaves. True
/// when this computer will tell; false when it will not (macOS and Linux),
/// and the caller should look again from time to time instead. Asking a
/// second time keeps the first `notice`.
pub fn on_change(notice: Notice) -> bool {
    imp::on_change(notice)
}

/// The name of the volume `folder` is on, on Windows; `None` elsewhere, and
/// when the system cannot say (a folder that is not there).
pub fn volume_of(folder: &Path) -> Option<String> {
    imp::volume_of(folder)
}

#[cfg(windows)]
mod imp {
    use super::Notice;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;
    use std::sync::OnceLock;
    use windows::core::{w, PCWSTR};
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::Storage::FileSystem::{GetVolumeNameForVolumeMountPointW, GetVolumePathNameW};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, RegisterClassW, TranslateMessage,
        DBT_DEVICEARRIVAL, DBT_DEVICEREMOVECOMPLETE, DBT_DEVTYP_VOLUME, DEV_BROADCAST_HDR, MSG, WINDOW_EX_STYLE,
        WM_DEVICECHANGE, WNDCLASSW, WS_OVERLAPPED,
    };

    /// Who to tell. Set once; the window's procedure has nowhere else to
    /// find it.
    static NOTICE: OnceLock<Notice> = OnceLock::new();

    pub fn on_change(notice: Notice) -> bool {
        if NOTICE.set(notice).is_err() {
            // Already listening.
            return true;
        }
        std::thread::Builder::new()
            .name("keevoro drives".into())
            .spawn(listen)
            .is_ok()
    }

    /// Makes the hidden window and hands it every message until the
    /// program ends.
    fn listen() {
        // SAFETY: GetModuleHandleW(None) gives this program's own module,
        // which needs no freeing.
        let Ok(module) = (unsafe { GetModuleHandleW(PCWSTR::null()) }) else {
            return;
        };
        let class = w!("KeevoroDriveNotices");
        let window_class = WNDCLASSW {
            lpfnWndProc: Some(procedure),
            hInstance: module.into(),
            lpszClassName: class,
            ..Default::default()
        };
        // SAFETY: the class lives for the whole call, and its name is a
        // static wide string.
        if unsafe { RegisterClassW(&window_class) } == 0 {
            return;
        }
        // SAFETY: a plain window of the class just registered, never shown,
        // with no parent, menu or creation data.
        let made = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                class,
                w!("Keevoro"),
                WS_OVERLAPPED,
                0,
                0,
                0,
                0,
                None,
                None,
                Some(module.into()),
                None,
            )
        };
        if made.is_err() {
            return;
        }
        let mut message = MSG::default();
        // SAFETY: the message is a live struct on this thread, which owns
        // the window, so its messages come here. GetMessageW answers 0 when
        // the program ends and -1 on a problem; both stop the loop.
        while unsafe { GetMessageW(&mut message, None, 0, 0) }.0 > 0 {
            // SAFETY: the message was just filled in by GetMessageW.
            unsafe {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
    }

    /// The hidden window's procedure: a volume arrived or was removed is
    /// told; everything else is left to Windows.
    unsafe extern "system" fn procedure(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if message == WM_DEVICECHANGE {
            let event = u32::try_from(wparam.0).unwrap_or(0);
            if event == DBT_DEVICEARRIVAL || event == DBT_DEVICEREMOVECOMPLETE {
                let header = lparam.0 as *const DEV_BROADCAST_HDR;
                // SAFETY: for these two events Windows passes a pointer to
                // a broadcast header that is valid for this call; it is
                // checked for null and only its type is read.
                let volume = !header.is_null() && unsafe { (*header).dbch_devicetype } == DBT_DEVTYP_VOLUME;
                if volume {
                    if let Some(notice) = NOTICE.get() {
                        notice();
                    }
                }
            }
            return LRESULT(1);
        }
        // SAFETY: passing on the very message Windows gave this window.
        unsafe { DefWindowProcW(window, message, wparam, lparam) }
    }

    pub fn volume_of(folder: &Path) -> Option<String> {
        let wide: Vec<u16> = folder.as_os_str().encode_wide().chain([0]).collect();
        let mut root = [0u16; 1024];
        // SAFETY: the path ends in a nul, and the buffer's length is given.
        unsafe { GetVolumePathNameW(PCWSTR(wide.as_ptr()), &mut root) }.ok()?;
        let mut name = [0u16; 64];
        // SAFETY: the mount point came back nul-terminated from Windows,
        // and the buffer is longer than any volume name (49 characters).
        unsafe { GetVolumeNameForVolumeMountPointW(PCWSTR(root.as_ptr()), &mut name) }.ok()?;
        let len = name.iter().position(|&c| c == 0).unwrap_or(name.len());
        Some(String::from_utf16_lossy(&name[..len]))
    }
}

#[cfg(not(windows))]
mod imp {
    use super::Notice;
    use std::path::Path;

    pub fn on_change(_notice: Notice) -> bool {
        false
    }

    pub fn volume_of(_folder: &Path) -> Option<String> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asking_to_hear_of_drives_says_whether_this_computer_will_tell() {
        assert_eq!(on_change(Box::new(|| {})), cfg!(windows));
    }

    #[test]
    fn a_folders_volume_is_named_where_the_system_says_it() {
        let dir = std::env::temp_dir();
        let named = volume_of(&dir);
        if cfg!(windows) {
            assert!(named.is_some_and(|n| n.starts_with(r"\\?\Volume{")));
        } else {
            assert_eq!(named, None, "the standard library says it there");
        }
    }
}
