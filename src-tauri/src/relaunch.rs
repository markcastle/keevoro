//! Coming back on the new version after an update on a Mac.
//!
//! On Windows the installer replaces Keevoro and opens it again (`/R`,
//! in `updates.rs`), and since an update never starts while work is in
//! hand, nothing is cut short. On a Mac the person drags the new app over
//! the old one while the old one keeps running, perhaps for weeks by the
//! clock. So once a minute, with nothing in hand, Keevoro compares the
//! version in its own bundle on disk with its own; when they differ it
//! opens the new one (hidden by the clock if its window is closed) and
//! goes.

/// The version a bundle's `Info.plist` names, if it names one. Pure.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn bundle_version(plist: &str) -> Option<String> {
    let key = "<key>CFBundleShortVersionString</key>";
    let after = &plist[plist.find(key)? + key.len()..];
    let start = after.find("<string>")? + "<string>".len();
    let end = after[start..].find("</string>")?;
    let version = after[start..start + end].trim();
    (!version.is_empty()).then(|| version.to_string())
}

/// Whether to open the version on disk now: it differs from this one, and
/// nothing is in hand. Pure.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn should_relaunch(on_disk: Option<&str>, running: &str, busy: bool) -> bool {
    on_disk.is_some_and(|v| v != running) && !busy
}

/// Opens the new version and quits this one, when it is time.
#[cfg(target_os = "macos")]
pub fn if_updated<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    use tauri::Manager;
    let Ok(exe) = std::env::current_exe() else { return };
    // Keevoro.app/Contents/MacOS/Keevoro
    let Some(contents) = exe.parent().and_then(std::path::Path::parent) else {
        return;
    };
    let Some(bundle) = contents.parent() else { return };
    let on_disk = std::fs::read_to_string(contents.join("Info.plist"))
        .ok()
        .and_then(|plist| bundle_version(&plist));
    let shared = app.state::<crate::Shared>();
    if !should_relaunch(
        on_disk.as_deref(),
        env!("CARGO_PKG_VERSION"),
        shared.keevoro.busy_here(),
    ) {
        return;
    }
    let hidden = app.get_webview_window(crate::window::WINDOW).is_none();
    shared.keevoro.log().say(&format!(
        "Keevoro {} is installed; opening it in place of {}",
        on_disk.unwrap_or_default(),
        env!("CARGO_PKG_VERSION")
    ));
    // Opened a moment after this one has gone, so the new one does not
    // find this one still running and hand over to it.
    let mut open = format!(
        "sleep 2; open -n '{}'",
        bundle.display().to_string().replace('\'', "'\\''")
    );
    if hidden {
        open.push_str(&format!(" --args {}", crate::closing::IN_BACKGROUND));
    }
    if std::process::Command::new("sh").args(["-c", &open]).spawn().is_ok() {
        app.exit(0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_version_is_read_from_a_bundles_info_plist() {
        let plist = r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
  <key>CFBundleName</key><string>Keevoro</string>
  <key>CFBundleShortVersionString</key>
  <string>0.45.0</string>
</dict></plist>"#;
        assert_eq!(bundle_version(plist).as_deref(), Some("0.45.0"));
        assert_eq!(bundle_version("<plist></plist>"), None);
    }

    #[test]
    fn it_relaunches_only_on_another_version_with_nothing_in_hand() {
        assert!(should_relaunch(Some("0.45.0"), "0.44.0", false));
        assert!(!should_relaunch(Some("0.45.0"), "0.44.0", true), "never mid-run");
        assert!(!should_relaunch(Some("0.44.0"), "0.44.0", false));
        assert!(!should_relaunch(None, "0.44.0", false));
    }
}
