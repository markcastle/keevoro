//! Finding the folders OneDrive, Dropbox, Google Drive and iCloud Drive
//! keep on this computer, to offer them by name on the first screen.
//!
//! What to make of what the computer says is decided in
//! [`keevoro_core::synced`], which is pure and tested with made-up facts.
//! This only gathers the facts: a few variables of the environment, the
//! text of Dropbox's small `info.json` where there is one, the listing of
//! `~/Library/CloudStorage` on a Mac, the drives' labels (already in the
//! system's quick list), and whether each folder that suggests exists. No
//! file inside a cloud folder is opened, and nothing is written.
//!
//! The first screen asks for its places each time it is shown, so the
//! answer is kept for [`crate::places::KEEP_FOR`], as the network drives'
//! are, and asking again within that costs nothing. It is asked from the
//! same thread as the drives, never the window's.

use keevoro_core::synced::{self, ComputerFacts, SyncedFolder, System};
use std::sync::{Mutex, PoisonError};
use std::time::Instant;

/// The folders on this computer a cloud service keeps in step, kept for
/// half a minute.
pub fn on_this_computer() -> Vec<SyncedFolder> {
    static KEPT: Mutex<Option<(Instant, Vec<SyncedFolder>)>> = Mutex::new(None);
    let mut kept = KEPT.lock().unwrap_or_else(PoisonError::into_inner);
    let now = Instant::now();
    if let Some((at, folders)) = kept.as_ref() {
        if now.saturating_duration_since(*at) < crate::places::KEEP_FOR {
            return folders.clone();
        }
    }
    let folders = synced::synced_folders(&gather());
    *kept = Some((now, folders.clone()));
    folders
}

/// What this computer says, as [`synced::synced_folders`] needs it.
pub fn gather() -> ComputerFacts {
    let system = if cfg!(windows) {
        System::Windows
    } else if cfg!(target_os = "macos") {
        System::Mac
    } else {
        System::Other
    };
    let home = dirs::home_dir().map(|h| h.display().to_string()).unwrap_or_default();
    let mut facts = ComputerFacts::new(system, &home);
    if system == System::Windows {
        for name in synced::ENVIRONMENT {
            if let Ok(value) = std::env::var(name) {
                facts.environment.insert((*name).to_string(), value);
            }
        }
    }
    // Only small files are read: a settings file is a few hundred bytes.
    const SMALL: u64 = 64 * 1024;
    for file in synced::files_to_read(&facts) {
        let small = std::fs::metadata(&file).is_ok_and(|m| m.is_file() && m.len() <= SMALL);
        if let Some(text) = small.then(|| std::fs::read_to_string(&file).ok()).flatten() {
            facts.files.insert(file, text);
        }
    }
    for folder in synced::folders_to_list(&facts) {
        if let Ok(entries) = std::fs::read_dir(&folder) {
            let names = entries
                .filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().to_string())
                .collect();
            facts.listings.insert(folder, names);
        }
    }
    facts.volumes = crate::places::labels();
    for place in synced::places_to_check(&facts) {
        if std::path::Path::new(&place).is_dir() {
            facts.existing.insert(place);
        }
    }
    facts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finding_the_synced_folders_is_quick_and_asking_again_is_quicker() {
        let started = Instant::now();
        let facts = gather();
        let gathered = started.elapsed();
        let found = synced::synced_folders(&facts);
        let first = started.elapsed();
        let again = Instant::now();
        let _ = on_this_computer();
        let _ = on_this_computer();
        let kept = again.elapsed();
        eprintln!(
            "gathering took {gathered:?}, deciding {:?}, found {}; two asks {kept:?}",
            first - gathered,
            found.len()
        );
        // Milliseconds on any computer; generous for a slow test machine.
        assert!(first.as_millis() < 1000, "{first:?}");
        // Every folder offered was found to be there.
        assert!(found.iter().all(|f| std::path::Path::new(&f.path).is_dir()));
    }
}
