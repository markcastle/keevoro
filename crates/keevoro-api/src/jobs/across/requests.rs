//! Requests: how one program asks another to stop, pause or carry on.
//!
//! Each request is its own small file in the backup's `requests` folder,
//! named so they sort in the order they were made. A folder of files, not
//! one file, because several programs may ask at once and none of them
//! should have to read, change and rewrite what another just wrote. The
//! holder looks in the folder a few times a second, does what is asked,
//! and removes each file as it reads it. A request addressed to an earlier
//! holder (its token is not the lock's) is removed unread.

use super::files;
use crate::jobs::{FrontEnd, JobId};
use serde::{Deserialize, Serialize};
use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

/// What is asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Ask {
    /// Stop these jobs once the file in hand is done.
    Stop,
    /// Every heavy job waits.
    Pause,
    /// Paused jobs go on.
    CarryOn,
}

/// One request, as written in its file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Request {
    /// The token of the lock it is for.
    pub to: String,
    pub ask: Ask,
    /// For [`Ask::Stop`]: which jobs.
    #[serde(default)]
    pub jobs: Vec<JobId>,
    pub by: FrontEnd,
    /// When it was asked, in milliseconds since 1970.
    pub at: i64,
}

/// Leaves a request in `folder` for its holder to find.
pub(crate) fn leave(folder: &Path, request: &Request) -> io::Result<()> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    std::fs::create_dir_all(folder)?;
    let name = format!(
        "{:016}-{}-{}.json",
        request.at.max(0),
        std::process::id(),
        NEXT.fetch_add(1, Ordering::SeqCst)
    );
    let bytes = serde_json::to_vec(request).map_err(io::Error::other)?;
    files::write_whole(&folder.join(name), &bytes)
}

/// Takes every request in `folder`, oldest first, and answers those for
/// `token`. Every file read is removed, including those for others and
/// those that cannot be read, so the folder never fills up.
pub(crate) fn take(folder: &Path, token: &str) -> Vec<Request> {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return Vec::new();
    };
    let mut names: Vec<_> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|name| !files::is_half_made(name))
        .collect();
    names.sort();
    names
        .into_iter()
        .filter_map(|name| {
            let path = folder.join(&name);
            let request = files::read(&path)
                .ok()
                .flatten()
                .and_then(|bytes| serde_json::from_slice::<Request>(&bytes).ok());
            let _ = files::remove(&path);
            request.filter(|r| r.to == token)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ask(to: &str, ask: Ask, at: i64) -> Request {
        Request {
            to: to.into(),
            ask,
            jobs: vec![JobId(3)],
            by: FrontEnd::Agent,
            at,
        }
    }

    #[test]
    fn requests_are_taken_in_the_order_asked_and_only_by_the_holder_they_were_for() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("requests");
        leave(&folder, &ask("mine", Ask::CarryOn, 2_000)).unwrap();
        leave(&folder, &ask("mine", Ask::Pause, 1_000)).unwrap();
        leave(&folder, &ask("an earlier holder", Ask::Stop, 1_500)).unwrap();
        std::fs::write(folder.join("garbled.json"), b"{ not json").unwrap();
        // One being written right now is not read yet.
        std::fs::write(folder.join(".1.json.12.0"), b"{").unwrap();

        let taken = take(&folder, "mine");
        assert_eq!(
            taken.iter().map(|r| r.ask).collect::<Vec<_>>(),
            [Ask::Pause, Ask::CarryOn]
        );
        assert_eq!(taken[0].jobs, [JobId(3)]);
        let left: Vec<_> = std::fs::read_dir(&folder)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(left, [".1.json.12.0"], "everything read was removed");
        assert!(take(&folder, "mine").is_empty());
        assert!(take(&dir.path().join("nowhere"), "mine").is_empty());
    }
}
