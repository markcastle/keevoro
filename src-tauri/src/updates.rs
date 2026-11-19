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
