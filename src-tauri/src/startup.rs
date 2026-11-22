//! Starting Keevoro when the computer starts, quietly, with no window.
//!
//! Only ever on the person's say: ticked in the dialog the first time the
//! window is hidden, or in Settings. It uses the official autostart plugin,
//! which keeps the entry where the system lists such things, under the
//! name Keevoro, with no administrator rights: a value under
//! `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` on Windows (shown
//! in Task Manager's Startup apps), a Launch Agent in
//! `~/Library/LaunchAgents` on a Mac, and a `.desktop` file in
//! `~/.config/autostart` on Linux. It starts Keevoro with
//! [`IN_BACKGROUND`](crate::closing::IN_BACKGROUND).
//!
//! The entry itself is the truth: Keevoro keeps no copy of whether it is
//! on, so turning it off in Task Manager or System Settings is seen at
//! once. The tests use one kept in memory, as Tauri's stand-in for a window
//! has no system to write to.

use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Runtime};

/// The computer's list of programs started when someone signs in.
pub trait StartWithComputer: Send + Sync {
    /// Whether Keevoro is on it. A sentence when the system will not say.
    fn is_on(&self) -> Result<bool, String>;
    /// Puts Keevoro on it, or takes it off.
    fn set(&self, on: bool) -> Result<(), String>;
}

/// The computer's own list, through the autostart plugin.
pub struct ThroughThePlugin<R: Runtime>(pub AppHandle<R>);

impl<R: Runtime> StartWithComputer for ThroughThePlugin<R> {
    fn is_on(&self) -> Result<bool, String> {
        use tauri_plugin_autostart::ManagerExt;
        self.0.autolaunch().is_enabled().map_err(|e| e.to_string())
    }

    fn set(&self, on: bool) -> Result<(), String> {
        use tauri_plugin_autostart::ManagerExt;
        let entry = self.0.autolaunch();
        if on { entry.enable() } else { entry.disable() }.map_err(|e| e.to_string())
    }
}

/// A list kept in memory, for tests.
#[derive(Default)]
pub struct InMemory(AtomicBool);

impl StartWithComputer for InMemory {
    fn is_on(&self) -> Result<bool, String> {
        Ok(self.0.load(Ordering::SeqCst))
    }

    fn set(&self, on: bool) -> Result<(), String> {
        self.0.store(on, Ordering::SeqCst);
        Ok(())
    }
}

/// What is said when the system would not do it.
pub fn could_not(on: bool, why: &str) -> String {
    if on {
        format!("Keevoro couldn't add itself to the programs this computer starts with: {why}.")
    } else {
        format!("Keevoro couldn't take itself off the programs this computer starts with: {why}.")
    }
}
