//! Installing a newer version of the app.
//!
//! Whether there is one is `keevoro.update`, in the API. Installing it is
//! the app's own business: it downloads that release's installer, proves it
//! matches the checksums published with the release, runs it and, on
//! Windows, closes so the installer can replace it. The download is checked
//! by `keevoro-update`, the same rules `keevoro update` uses, so there is
//! one set of rules for what counts as a trustworthy download.

use keevoro_update::{Check, GitHub, UpdateError};
use std::path::{Path, PathBuf};

/// Downloads and checks the newest installer, and returns where it was put.
pub fn fetch(current: &str, os: &str, into: &Path) -> Result<PathBuf, UpdateError> {
    let web = GitHub::from_env();
    let release = match keevoro_update::check(&web, current)? {
        Check::Available(release) => release,
        Check::UpToDate => return Err(UpdateError::Install("Keevoro is already up to date".into())),
    };
    let (name, bytes) = keevoro_update::download_installer(&web, &release, os)?;
    let fail = |e: std::io::Error| UpdateError::Install(e.to_string());
    std::fs::create_dir_all(into).map_err(fail)?;
    let file = into.join(name);
    std::fs::write(&file, bytes).map_err(fail)?;
    Ok(file)
}

/// What pressing Update does, given whether this app is in the middle of a
/// search, a backup or a check, and whether the person asked to stop it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Install {
    /// Nothing is in hand: update now.
    Now,
    /// Stop what is in hand after the file in hand, then update.
    StopFirst,
    /// Wait for what is in hand to finish. Updating closes the app, which
    /// would cut it short.
    Wait,
}

/// What is said while an update waits for the work in hand.
pub const WAITING: &str =
    "Keevoro will update once the work in hand finishes, so nothing is cut short. You can stop it and update now instead.";

/// Pure: whether to update now.
pub fn may_install(busy: bool, stop_first: bool) -> Install {
    match (busy, stop_first) {
        (false, _) => Install::Now,
        (true, true) => Install::StopFirst,
        (true, false) => Install::Wait,
    }
}

/// How to run a downloaded installer: the program and what to pass it.
///
/// On Windows the installer runs without questions and reopens Keevoro when
/// it finishes. On a Mac the disk image is opened, ready to drag across.
pub fn launch_plan(os: &str, installer: &Path) -> Option<(PathBuf, Vec<String>)> {
    match os {
        "windows" => Some((installer.to_path_buf(), vec!["/P".into(), "/R".into()])),
        "macos" => Some((PathBuf::from("open"), vec![installer.display().to_string()])),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_installs_quietly_and_reopens_the_app() {
        let (program, args) = launch_plan("windows", Path::new("C:/Temp/Keevoro-windows-x64-setup.exe")).unwrap();
        assert_eq!(program, Path::new("C:/Temp/Keevoro-windows-x64-setup.exe"));
        assert_eq!(args, ["/P", "/R"]);
    }

    #[test]
    fn an_update_never_cuts_short_the_work_in_hand() {
        assert_eq!(may_install(false, false), Install::Now);
        assert_eq!(may_install(false, true), Install::Now);
        assert_eq!(may_install(true, false), Install::Wait);
        assert_eq!(may_install(true, true), Install::StopFirst);
    }

    #[test]
    fn a_mac_opens_the_disk_image() {
        let (program, args) = launch_plan("macos", Path::new("/tmp/Keevoro-macos-universal.dmg")).unwrap();
        assert_eq!(program, Path::new("open"));
        assert_eq!(args, ["/tmp/Keevoro-macos-universal.dmg"]);
    }

    #[test]
    fn other_computers_have_nothing_to_run() {
        assert_eq!(launch_plan("linux", Path::new("/tmp/x")), None);
    }
}
