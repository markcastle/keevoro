//! Writing small files so that another program never reads half of one.
//!
//! Two programs share a backup's lock, status and requests through files,
//! and each reads what the other writes while it may be writing. So every
//! file is written whole beside where it belongs, then moved into place in
//! one step. A reader sees the old file or the new one, never a mixture.
//! Names that begin with a dot are these half-made files, and are never
//! read as the real thing.

use std::fs::{self, File, OpenOptions};
use std::io::{self, ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// How long a file that another program has open for a moment is waited
/// for before giving up on it.
const PATIENCE: Duration = Duration::from_secs(2);

/// How often it is tried again meanwhile.
const AGAIN_AFTER: Duration = Duration::from_millis(20);

/// True for a refusal that passes: on Windows a file another program has
/// open (a virus checker or the search indexer looking at something just
/// written, or another Keevoro reading it) can refuse to be read, renamed
/// or replaced for a moment, saying "access denied", "in use by another
/// process" or "about to be deleted". Everywhere, "try again" passes; a
/// refused permission elsewhere is meant.
pub(crate) fn passes(e: &io::Error) -> bool {
    if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::Interrupted) {
        return true;
    }
    #[cfg(windows)]
    {
        // ERROR_ACCESS_DENIED, ERROR_SHARING_VIOLATION, ERROR_LOCK_VIOLATION
        // and ERROR_DELETE_PENDING.
        e.kind() == ErrorKind::PermissionDenied || matches!(e.raw_os_error(), Some(5 | 32 | 33 | 303))
    }
    #[cfg(not(windows))]
    {
        false
    }
}

/// Does `step`, trying again for a moment while it is refused in a way
/// that passes. Every file shared between programs goes through this, so
/// that a request is never dropped, and a status never left unwritten,
/// because something else glanced at the file.
pub(crate) fn patiently<T>(mut step: impl FnMut() -> io::Result<T>) -> io::Result<T> {
    let until = Instant::now() + PATIENCE;
    loop {
        match step() {
            Err(e) if passes(&e) && Instant::now() < until => std::thread::sleep(AGAIN_AFTER),
            done => return done,
        }
    }
}

/// A name beside `path` that nothing else will choose: hidden, and unique
/// to this program and this moment.
fn beside(path: &Path) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let name = path
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    path.with_file_name(format!(
        ".{name}.{}.{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::SeqCst)
    ))
}

fn write_new(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = File::create(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

/// True for a name this module uses while a file is being made.
pub(crate) fn is_half_made(name: &str) -> bool {
    name.starts_with('.')
}

/// Writes `bytes` to `path`, replacing whatever was there in one step.
///
/// On Windows a file another program has open can briefly refuse to be
/// replaced, and a file just written can briefly refuse to be moved, so
/// that is waited for (see [`patiently`]) rather than given up on.
pub(crate) fn write_whole(path: &Path, bytes: &[u8]) -> io::Result<()> {
    write_whole_with(path, bytes, &mut |from, to| fs::rename(from, to))
}

/// [`write_whole`], moving the file into place with `rename`: for tests,
/// one that refuses for a while.
fn write_whole_with(
    path: &Path,
    bytes: &[u8],
    rename: &mut dyn FnMut(&Path, &Path) -> io::Result<()>,
) -> io::Result<()> {
    let temp = beside(path);
    let mut result = patiently(|| write_new(&temp, bytes));
    if result.is_ok() {
        result = patiently(|| rename(&temp, path));
    }
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

/// Makes `path` with `bytes` in it, but only if nothing is there. True if
/// it made it; false if something was there already.
///
/// The file is made whole beside its place and linked into it, which the
/// operating system refuses if the place is taken, so two programs racing
/// for it cannot both win and nobody ever reads it half-written. On a
/// drive that cannot link files it is made in two steps instead; readers
/// allow for that (see `lock::read`).
pub(crate) fn create_whole(path: &Path, bytes: &[u8]) -> io::Result<bool> {
    let temp = beside(path);
    write_new(&temp, bytes)?;
    let linked = fs::hard_link(&temp, path);
    let _ = fs::remove_file(&temp);
    match linked {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == ErrorKind::AlreadyExists => Ok(false),
        Err(_) => match OpenOptions::new().write(true).create_new(true).open(path) {
            Ok(mut file) => {
                file.write_all(bytes)?;
                file.sync_all()?;
                Ok(true)
            }
            Err(e) if e.kind() == ErrorKind::AlreadyExists => Ok(false),
            Err(e) => Err(e),
        },
    }
}

/// The file's contents, or `None` if it is not there. A file another
/// program has open for a moment is waited for (see [`patiently`]).
pub(crate) fn read(path: &Path) -> io::Result<Option<Vec<u8>>> {
    match patiently(|| fs::read(path)) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// Removes a file, and is content if it was already gone.
pub(crate) fn remove(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Err(e) if e.kind() != ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_written_whole_replaces_the_old_one_and_leaves_nothing_beside_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("status.json");
        write_whole(&path, b"first").unwrap();
        write_whole(&path, b"second").unwrap();
        assert_eq!(read(&path).unwrap().unwrap(), b"second");
        assert_eq!(
            fs::read_dir(dir.path()).unwrap().count(),
            1,
            "no half-made file is left"
        );
    }

    #[test]
    fn a_file_that_refuses_to_be_moved_into_place_for_a_moment_is_still_written() {
        // What Windows does while a virus checker looks at a file just
        // written: refuses, as "in use", for a while. That once lost a
        // request to pause, so another program never paused.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("request.json");
        let mut refusals = 0;
        write_whole_with(&path, b"pause", &mut |from, to| {
            if refusals < 10 {
                refusals += 1;
                return Err(io::Error::from(ErrorKind::WouldBlock));
            }
            fs::rename(from, to)
        })
        .unwrap();
        assert_eq!(read(&path).unwrap().unwrap(), b"pause");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1, "nothing left beside it");
    }

    #[test]
    fn a_refusal_that_does_not_pass_is_given_up_on_at_once_and_leaves_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("status.json");
        let started = Instant::now();
        let e = write_whole_with(&path, b"x", &mut |_, _| Err(io::Error::from(ErrorKind::InvalidInput))).unwrap_err();
        assert_eq!(e.kind(), ErrorKind::InvalidInput);
        assert!(started.elapsed() < PATIENCE, "not waited on");
        assert_eq!(
            fs::read_dir(dir.path()).unwrap().count(),
            0,
            "the half-made file is removed"
        );
    }

    #[test]
    fn a_refusal_that_never_passes_is_given_up_on_after_a_while() {
        let mut tries = 0;
        let e = patiently(|| -> io::Result<()> {
            tries += 1;
            Err(io::Error::from(ErrorKind::WouldBlock))
        })
        .unwrap_err();
        assert_eq!(e.kind(), ErrorKind::WouldBlock);
        assert!(tries > 1, "tried again");
    }

    #[test]
    fn only_one_of_two_can_make_a_file_and_the_loser_changes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("holder.json");
        assert!(create_whole(&path, b"mine").unwrap());
        assert!(!create_whole(&path, b"theirs").unwrap());
        assert_eq!(read(&path).unwrap().unwrap(), b"mine");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn many_racing_to_make_a_file_have_exactly_one_winner() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("holder.json");
        let winners: usize = std::thread::scope(|scope| {
            let racers: Vec<_> = (0..16)
                .map(|n| {
                    let path = &path;
                    scope.spawn(move || usize::from(create_whole(path, format!("{n}").as_bytes()).unwrap()))
                })
                .collect();
            racers.into_iter().map(|r| r.join().unwrap()).sum()
        });
        assert_eq!(winners, 1);
    }

    #[test]
    fn reading_or_removing_what_is_not_there_is_not_a_problem() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("gone");
        assert_eq!(read(&path).unwrap(), None);
        remove(&path).unwrap();
        assert!(is_half_made(".status.json.12.0"));
        assert!(!is_half_made("status.json"));
    }
}
