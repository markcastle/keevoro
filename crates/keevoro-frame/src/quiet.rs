//! What the computer says about itself, for being a good guest: whether it
//! is on battery, how long since the person last touched it, and a way to
//! ask for the current thread to be run only when nothing else wants to.
//!
//! These live here because asking the operating system means calling into
//! it, and this is the one crate allowed to. Each answer is `None` where
//! the platform has no cheap way to give it, and the rules that use them
//! (`keevoro_core::background`) take `None` the careful way.
//!
//! | | Windows | macOS | Linux |
//! | --- | --- | --- | --- |
//! | Low priority | background mode (processor and disk) | background quality of service (processor and disk) | nice 10 (processor) |
//! | On battery, and how full | `GetSystemPowerStatus` | not known | `/sys/class/power_supply` |
//! | Idle time | `GetLastInputInfo` | `CGEventSourceSecondsSinceLastEventType` | not known |

/// Asks for the calling thread to be run at low priority, so it gives way
/// to everything else on the computer. On Windows and macOS this also
/// lowers the priority of its disk reads. Answers whether it was granted.
pub fn lower_this_threads_priority() -> bool {
    imp::lower_this_threads_priority()
}

/// True on battery, false on mains, `None` when this computer cannot say.
pub fn on_battery() -> Option<bool> {
    imp::on_battery()
}

/// How full the battery is, 0 to 100, or `None` when this computer has
/// none or cannot say.
pub fn battery_percent() -> Option<u8> {
    imp::battery_percent()
}

/// Seconds since the person last pressed a key or moved the mouse, or
/// `None` when this computer cannot say.
pub fn idle_seconds() -> Option<u64> {
    imp::idle_seconds()
}

#[cfg(windows)]
mod imp {
    use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
    use windows::Win32::System::SystemInformation::GetTickCount;
    use windows::Win32::System::Threading::{GetCurrentThread, SetThreadPriority, THREAD_MODE_BACKGROUND_BEGIN};
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};

    pub fn lower_this_threads_priority() -> bool {
        // SAFETY: GetCurrentThread returns a pseudo-handle that needs no
        // closing, and background mode only affects the calling thread.
        unsafe { SetThreadPriority(GetCurrentThread(), THREAD_MODE_BACKGROUND_BEGIN) }.is_ok()
    }

    pub fn on_battery() -> Option<bool> {
        let mut status = SYSTEM_POWER_STATUS::default();
        // SAFETY: the pointer is to a live, correctly sized struct.
        unsafe { GetSystemPowerStatus(&mut status) }.ok()?;
        match status.ACLineStatus {
            0 => Some(true),
            1 => Some(false),
            _ => None,
        }
    }

    pub fn battery_percent() -> Option<u8> {
        let mut status = SYSTEM_POWER_STATUS::default();
        // SAFETY: the pointer is to a live, correctly sized struct.
        unsafe { GetSystemPowerStatus(&mut status) }.ok()?;
        // 255 means not known.
        (status.BatteryLifePercent <= 100).then_some(status.BatteryLifePercent)
    }

    pub fn idle_seconds() -> Option<u64> {
        let mut info = LASTINPUTINFO {
            cbSize: u32::try_from(std::mem::size_of::<LASTINPUTINFO>()).ok()?,
            dwTime: 0,
        };
        // SAFETY: the pointer is to a live struct whose size is given in it.
        if !unsafe { GetLastInputInfo(&mut info) }.as_bool() {
            return None;
        }
        // SAFETY: GetTickCount takes nothing and cannot fail.
        let now = unsafe { GetTickCount() };
        // Both wrap after 49 days, together.
        Some(u64::from(now.wrapping_sub(info.dwTime)) / 1000)
    }
}

#[cfg(target_os = "macos")]
mod imp {
    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGEventSourceSecondsSinceLastEventType(state: i32, event_type: u32) -> f64;
    }
    /// `kCGEventSourceStateCombinedSessionState`.
    const COMBINED_SESSION: i32 = 0;
    /// `kCGAnyInputEventType`.
    const ANY_INPUT: u32 = !0;

    pub fn lower_this_threads_priority() -> bool {
        // SAFETY: changes only the calling thread's quality of service.
        unsafe { libc::pthread_set_qos_class_self_np(libc::qos_class_t::QOS_CLASS_BACKGROUND, 0) == 0 }
    }

    pub fn on_battery() -> Option<bool> {
        // Asking needs IOKit's power sources, which is more than this is
        // worth yet; not knowing is not a reason to wait.
        None
    }

    pub fn battery_percent() -> Option<u8> {
        None
    }

    pub fn idle_seconds() -> Option<u64> {
        // SAFETY: a plain query with constant arguments, no pointers.
        let seconds = unsafe { CGEventSourceSecondsSinceLastEventType(COMBINED_SESSION, ANY_INPUT) };
        (seconds.is_finite() && seconds >= 0.0).then_some(seconds as u64)
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
mod imp {
    pub fn lower_this_threads_priority() -> bool {
        // On Linux a thread's niceness is its own, named by its thread id.
        #[cfg(target_os = "linux")]
        {
            // SAFETY: gettid takes nothing; setpriority changes only the
            // niceness of the calling thread, and only downwards.
            unsafe { libc::setpriority(libc::PRIO_PROCESS, libc::gettid() as libc::id_t, 10) == 0 }
        }
        #[cfg(not(target_os = "linux"))]
        {
            false
        }
    }

    pub fn on_battery() -> Option<bool> {
        super::on_battery_from(std::path::Path::new("/sys/class/power_supply"))
    }

    pub fn battery_percent() -> Option<u8> {
        super::battery_percent_from(std::path::Path::new("/sys/class/power_supply"))
    }

    pub fn idle_seconds() -> Option<u64> {
        // X11 and Wayland each have their own way, neither cheap from here.
        None
    }
}

#[cfg(not(any(windows, unix)))]
mod imp {
    pub fn lower_this_threads_priority() -> bool {
        false
    }
    pub fn on_battery() -> Option<bool> {
        None
    }
    pub fn battery_percent() -> Option<u8> {
        None
    }
    pub fn idle_seconds() -> Option<u64> {
        None
    }
}

/// Reads how full Linux's batteries are, from their `capacity`: the lowest
/// of them, so a laptop with a second battery nearly empty counts as low.
#[cfg_attr(not(all(unix, not(target_os = "macos"))), allow(dead_code))]
fn battery_percent_from(supplies: &std::path::Path) -> Option<u8> {
    let read = |dir: &std::path::Path, name: &str| std::fs::read_to_string(dir.join(name)).ok();
    std::fs::read_dir(supplies)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|dir| read(dir, "type").as_deref().map(str::trim) == Some("Battery"))
        .filter_map(|dir| read(&dir, "capacity")?.trim().parse::<u8>().ok())
        .filter(|p| *p <= 100)
        .min()
}

/// Reads Linux's list of power supplies: on mains when any mains supply is
/// online, on battery when there is a battery and no mains supply online.
#[cfg_attr(not(all(unix, not(target_os = "macos"))), allow(dead_code))]
fn on_battery_from(supplies: &std::path::Path) -> Option<bool> {
    let read = |dir: &std::path::Path, name: &str| std::fs::read_to_string(dir.join(name)).ok();
    let (mut mains, mut mains_online, mut battery) = (false, false, false);
    for entry in std::fs::read_dir(supplies).ok()?.flatten() {
        let dir = entry.path();
        match read(&dir, "type").as_deref().map(str::trim) {
            Some("Mains") => {
                mains = true;
                mains_online |= read(&dir, "online").as_deref().map(str::trim) == Some("1");
            }
            Some("Battery") => battery = true,
            _ => {}
        }
    }
    if mains_online {
        Some(false)
    } else if battery && mains {
        Some(true)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_signals_can_be_asked_for_on_any_computer() {
        // What they answer depends on the computer; asking must never fail.
        let _ = on_battery();
        let _ = battery_percent();
        let _ = idle_seconds();
        let worker = std::thread::spawn(lower_this_threads_priority);
        let _ = worker.join().unwrap();
    }

    #[test]
    fn linux_power_supplies_are_read_as_mains_or_battery() {
        let dir = tempfile::tempdir().unwrap();
        let supply = |name: &str, kind: &str, online: Option<&str>| {
            let d = dir.path().join(name);
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join("type"), format!("{kind}\n")).unwrap();
            if let Some(online) = online {
                std::fs::write(d.join("online"), format!("{online}\n")).unwrap();
            }
        };
        assert_eq!(on_battery_from(dir.path()), None, "nothing to say");
        supply("BAT0", "Battery", None);
        supply("AC", "Mains", Some("0"));
        assert_eq!(on_battery_from(dir.path()), Some(true));
        supply("AC", "Mains", Some("1"));
        assert_eq!(on_battery_from(dir.path()), Some(false));
        assert_eq!(on_battery_from(&dir.path().join("missing")), None);
    }

    #[test]
    fn linux_batteries_say_how_full_they_are_the_emptiest_counting() {
        let dir = tempfile::tempdir().unwrap();
        let battery = |name: &str, capacity: &str| {
            let d = dir.path().join(name);
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join("type"), "Battery\n").unwrap();
            std::fs::write(d.join("capacity"), format!("{capacity}\n")).unwrap();
        };
        assert_eq!(battery_percent_from(dir.path()), None, "no battery");
        battery("BAT0", "87");
        assert_eq!(battery_percent_from(dir.path()), Some(87));
        battery("BAT1", "12");
        assert_eq!(battery_percent_from(dir.path()), Some(12));
        battery("BAT2", "not a number");
        assert_eq!(battery_percent_from(dir.path()), Some(12));
    }
}
