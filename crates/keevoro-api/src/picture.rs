//! `photos.picture`: a small or screen-sized picture of one found file, or
//! of any photo or video in a folder being looked inside.
//!
//! Pictures are WebP, made from the file where it is (or taken from the
//! previews a backup made of it) and kept in Keevoro's cache folder so the
//! same picture is quick the second time. A video gives a still. A file
//! only in online storage is never opened.
//!
//! Only a few are made at once, whoever asks. A screen of pictures asks for
//! all of its previews together, and an agent could ask for many; making a
//! few at a time keeps the computer responsive, above all for videos, where
//! each picture runs a separate program. Each drive has turns of its own
//! as well, taken first: one at a time on a network folder, two on a disk,
//! so a slow drive is never asked for many files at once.

use crate::backup::Backup;
use crate::error::Result;
use keevoro_engine::Look;
use keevoro_media::VideoTools;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::{Condvar, Mutex, PoisonError};

/// How big a picture to make.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum PictureSize {
    /// A small picture, for a grid of many.
    #[default]
    Small,
    /// A screen-sized picture, for looking closely at one.
    Screen,
}

/// `photos.picture`
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PictureRequest {
    /// A found photo's short id, as `collections.photos` gives it. Give
    /// this or `file`, not both.
    #[serde(default)]
    pub photo: Option<String>,
    /// Any photo or video, by its path, as `places.files` gives it: a
    /// picture of a file not yet found by a search. Give this or `photo`.
    #[serde(default)]
    pub file: Option<String>,
    /// `small`, for a grid of many, or `screen`, for a closer look. Small
    /// when left out.
    #[serde(default)]
    pub size: PictureSize,
}

/// A picture, as JSON can carry it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Picture {
    /// Always `webp`.
    pub format: &'static str,
    /// The picture's bytes, in standard Base64.
    pub base64: String,
}

impl Picture {
    pub fn webp(bytes: &[u8]) -> Self {
        Self {
            format: "webp",
            base64: base64(bytes),
        }
    }

    /// The picture's bytes from what `photos.picture` answered, as the
    /// command line needs them to save the picture as a file. `None` when
    /// it is not a picture as written here.
    pub fn bytes_of(answer: &serde_json::Value) -> Option<Vec<u8>> {
        from_base64(answer.get("base64")?.as_str()?)
    }
}

/// Makes (or finds in `cache`) a picture of one file in `backup`, as WebP
/// bytes. Waits its turn behind the few being made already.
pub fn picture(backup: &Backup, cache: &Path, photo: &str, size: PictureSize) -> Result<Vec<u8>> {
    let look = match size {
        PictureSize::Small => Look::Small,
        PictureSize::Screen => Look::Closer,
    };
    let _turn = PICTURES.wait_for_a_turn();
    let bytes = keevoro_engine::preview_at(
        backup.home(),
        backup.catalog(),
        cache,
        photo,
        VideoTools::find().as_ref(),
        look,
    )?;
    pictured(cache);
    Ok(bytes)
}

/// Lets only a few slow jobs run at once; the rest wait their turn.
struct Turns {
    free: Mutex<usize>,
    freed: Condvar,
}

/// Holding one of these is having a turn. Dropping it passes the turn on.
struct Turn<'a>(&'a Turns);

impl Turns {
    const fn new(at_once: usize) -> Self {
        Self {
            free: Mutex::new(at_once),
            freed: Condvar::new(),
        }
    }

    fn wait_for_a_turn(&self) -> Turn<'_> {
        // A poisoned lock only means another picture panicked; the count
        // inside is still right, so carry on with it.
        let mut free = self.free.lock().unwrap_or_else(PoisonError::into_inner);
        while *free == 0 {
            free = self.freed.wait(free).unwrap_or_else(PoisonError::into_inner);
        }
        *free -= 1;
        Turn(self)
    }
}

impl Drop for Turn<'_> {
    fn drop(&mut self) {
        *self.0.free.lock().unwrap_or_else(PoisonError::into_inner) += 1;
        self.0.freed.notify_one();
    }
}

static PICTURES: Turns = Turns::new(3);

/// Which drive a file is on, for its drive's turns: on Windows its drive
/// letter or network share, elsewhere the device it is on.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum DriveKey {
    #[cfg_attr(not(windows), allow(dead_code))]
    Named(String),
    #[cfg_attr(not(unix), allow(dead_code))]
    Device(u64),
    Unknown,
}

pub(crate) fn drive_key(file: &Path) -> DriveKey {
    #[cfg(windows)]
    {
        match file.components().next() {
            Some(std::path::Component::Prefix(prefix)) => {
                DriveKey::Named(prefix.as_os_str().to_string_lossy().to_lowercase())
            }
            _ => DriveKey::Unknown,
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        std::fs::metadata(file).map_or(DriveKey::Unknown, |m| DriveKey::Device(m.dev()))
    }
    #[cfg(not(any(windows, unix)))]
    {
        let _ = file;
        DriveKey::Unknown
    }
}

/// How many pictures one drive is asked for at once: one across a
/// network, two on a disk of this computer. As with a search, a drive is
/// never asked for many files at the same moment.
pub(crate) const fn at_once_on(across_a_network: bool) -> usize {
    if across_a_network {
        1
    } else {
        2
    }
}

/// A turn on one drive, kept while it is held.
pub(crate) struct DriveTurn(std::sync::Arc<Turns>);

impl Drop for DriveTurn {
    fn drop(&mut self) {
        *self.0.free.lock().unwrap_or_else(PoisonError::into_inner) += 1;
        self.0.freed.notify_one();
    }
}

/// Each drive's turns.
#[derive(Default)]
pub(crate) struct DriveTurns(Mutex<std::collections::HashMap<DriveKey, std::sync::Arc<Turns>>>);

impl DriveTurns {
    /// Waits for a turn on the drive `key`, which allows `at_once` (asked
    /// only the first time the drive is seen).
    pub(crate) fn wait_for_a_turn(&self, key: DriveKey, at_once: impl FnOnce() -> usize) -> DriveTurn {
        let turns = self
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(key)
            .or_insert_with(|| std::sync::Arc::new(Turns::new(at_once())))
            .clone();
        std::mem::forget(turns.wait_for_a_turn());
        DriveTurn(turns)
    }
}

static DRIVES: std::sync::LazyLock<DriveTurns> = std::sync::LazyLock::new(DriveTurns::default);

/// Pictures given since Keevoro started, so the cache is kept within its
/// limit every so often, not only at start-up.
static MADE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// After this many pictures, the cache is trimmed again.
const TRIM_EVERY: usize = 500;

/// Makes (or finds in `cache`) a picture of any photo or video, by its
/// path, as WebP bytes. Only kinds of file on the list (`kinds`) are
/// pictured, switched on or off. `found` is a backup whose search may know the file already.
pub fn picture_of_file(
    cache: &Path,
    file: &Path,
    kinds: &keevoro_core::Kinds,
    size: PictureSize,
    found: Option<&Backup>,
) -> Result<Vec<u8>> {
    let not_there = || {
        crate::Error::not_found(format!(
            "Keevoro can't find {}. Is its drive plugged in?",
            keevoro_engine::friendly_path(file)
        ))
    };
    let real = std::fs::canonicalize(file).map_err(|_| not_there())?;
    let ending = real
        .extension()
        .map(|e| e.to_string_lossy().into_owned())
        .unwrap_or_default();
    let Some((kind, _)) = kinds.listed(&ending) else {
        return Err(crate::Error::refused(
            "Keevoro doesn't look for that kind of file here, so it makes no picture of it.",
        ));
    };
    let look = match size {
        PictureSize::Small => Look::Small,
        PictureSize::Screen => Look::Closer,
    };
    let drive = real.parent().unwrap_or(&real).to_path_buf();
    let _drive = DRIVES.wait_for_a_turn(drive_key(&real), || {
        at_once_on(keevoro_engine::reach::is_across_a_network(&drive))
    });
    let _turn = PICTURES.wait_for_a_turn();
    let bytes = keevoro_engine::look::picture_of_file(
        cache,
        &real,
        kind,
        look,
        VideoTools::find().as_ref(),
        &keevoro_scan::ByAttributes,
        found.map(|b| keevoro_engine::look::Found {
            home: b.home(),
            catalog: b.catalog(),
        }),
    )
    .map_err(|e| crate::Error::refused(e.to_string()))?;
    pictured(cache);
    Ok(bytes)
}

/// One more picture was given. Every [`TRIM_EVERY`] the cache is trimmed
/// to its limit, so even a long look at thousands of files stays within it.
fn pictured(cache: &Path) {
    if MADE.fetch_add(1, std::sync::atomic::Ordering::Relaxed) % TRIM_EVERY == TRIM_EVERY - 1 {
        keevoro_engine::look::trim_cache_quietly(cache);
    }
}

/// Standard Base64 with padding. A few lines here rather than a crate for
/// one use.
pub(crate) fn base64(bytes: &[u8]) -> String {
    const LETTERS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for (i, shift) in [18, 12, 6, 0].into_iter().enumerate() {
            if i <= chunk.len() {
                out.push(char::from(LETTERS[((n >> shift) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// The bytes written by [`base64`], or `None` for anything else.
pub(crate) fn from_base64(text: &str) -> Option<Vec<u8>> {
    let value = |c: u8| match c {
        b'A'..=b'Z' => Some(c - b'A'),
        b'a'..=b'z' => Some(c - b'a' + 26),
        b'0'..=b'9' => Some(c - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    };
    if text.len() % 4 != 0 {
        return None;
    }
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    for chunk in text.as_bytes().chunks(4) {
        let padding = chunk.iter().rev().take_while(|&&c| c == b'=').count();
        if padding > 2 {
            return None;
        }
        let mut n = 0u32;
        for &c in &chunk[..4 - padding] {
            n = (n << 6) | u32::from(value(c)?);
        }
        n <<= 6 * padding;
        let bytes = n.to_be_bytes();
        out.extend_from_slice(&bytes[1..4 - padding]);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backup::tests::one_backup;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn a_picture_is_made_once_and_kept_in_the_cache() {
        let (dir, _card, backup) = one_backup();
        let cache = dir.path().join("cache");
        let photo = backup.collection_photos("Holidays / Cornwall").unwrap()[0]
            .photo
            .id
            .clone();
        let small = picture(&backup, &cache, &photo, PictureSize::Small).unwrap();
        assert_eq!(&small[..4], b"RIFF");
        assert_eq!(&small[8..12], b"WEBP");
        let screen = picture(&backup, &cache, &photo, PictureSize::Screen).unwrap();
        assert_ne!(screen, small, "a screen-sized picture is another picture");
        assert!(cache.is_dir() && std::fs::read_dir(&cache).unwrap().count() > 0);

        let e = picture(&backup, &cache, "0000", PictureSize::Small).unwrap_err();
        assert_eq!(e.message, "That photo isn't known to this backup.");
    }

    #[test]
    fn a_file_in_no_backup_is_pictured_by_its_path_and_only_kinds_on_the_list() {
        let (dir, card, _backup) = one_backup();
        let cache = dir.path().join("cache");
        let photo = walk(&card)
            .into_iter()
            .find(|p| p.extension().is_some_and(|x| x == "jpg"))
            .unwrap();
        let kinds = keevoro_core::Kinds::default();
        let small = picture_of_file(&cache, &photo, &kinds, PictureSize::Small, None).unwrap();
        assert_eq!(&small[8..12], b"WEBP");
        // A kind switched off is still pictured, so the look inside can show
        // it and say it is left out; one not on the list at all is not.
        let videos_only = kinds
            .clone()
            .wanted_by(keevoro_core::Wants::only(keevoro_core::MediaKind::Video));
        assert!(picture_of_file(&cache, &photo, &videos_only, PictureSize::Small, None).is_ok());
        let text = card.join("notes.txt");
        std::fs::write(&text, b"words").unwrap();
        let e = picture_of_file(&cache, &text, &kinds, PictureSize::Small, None).unwrap_err();
        assert_eq!(e.kind, crate::Kind::Refused);
        std::fs::remove_file(&text).unwrap();
        let e = picture_of_file(&cache, &card.join("gone.jpg"), &kinds, PictureSize::Small, None).unwrap_err();
        assert_eq!(e.kind, crate::Kind::NotFound);
    }

    fn walk(dir: &Path) -> Vec<std::path::PathBuf> {
        let mut out = Vec::new();
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            if entry.path().is_dir() {
                out.extend(walk(&entry.path()));
            } else {
                out.push(entry.path());
            }
        }
        out
    }

    #[test]
    fn a_network_drive_is_asked_for_one_picture_at_a_time_and_a_disk_for_two() {
        assert_eq!(at_once_on(true), 1);
        assert_eq!(at_once_on(false), 2);
        let drives = std::sync::Arc::new(DriveTurns::default());
        for (key, network, expected) in [
            (DriveKey::Named("\\\\nas\\photos".into()), true, 1),
            (DriveKey::Device(7), false, 2),
        ] {
            let running = std::sync::Arc::new(AtomicUsize::new(0));
            let most = std::sync::Arc::new(AtomicUsize::new(0));
            let jobs: Vec<_> = (0..8)
                .map(|_| {
                    let (drives, key, running, most) = (drives.clone(), key.clone(), running.clone(), most.clone());
                    std::thread::spawn(move || {
                        let _turn = drives.wait_for_a_turn(key, || at_once_on(network));
                        let now = running.fetch_add(1, Ordering::SeqCst) + 1;
                        most.fetch_max(now, Ordering::SeqCst);
                        std::thread::sleep(std::time::Duration::from_millis(5));
                        running.fetch_sub(1, Ordering::SeqCst);
                    })
                })
                .collect();
            for job in jobs {
                job.join().unwrap();
            }
            assert_eq!(most.load(Ordering::SeqCst), expected, "{key:?}");
        }
    }

    #[test]
    fn base64_is_the_standard_kind() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64(&[0xff, 0xfe, 0x00]), "//4A");
        assert_eq!(
            serde_json::to_value(Picture::webp(b"foo")).unwrap(),
            serde_json::json!({ "format": "webp", "base64": "Zm9v" })
        );
    }

    #[test]
    fn a_picture_written_as_base64_reads_back_as_itself() {
        for bytes in [&b""[..], b"f", b"fo", b"foo", b"foobar", &[0xff, 0xfe, 0x00, 0x01]] {
            let answer = serde_json::to_value(Picture::webp(bytes)).unwrap();
            assert_eq!(Picture::bytes_of(&answer).as_deref(), Some(bytes));
        }
        let all: Vec<u8> = (0..=255).collect();
        assert_eq!(from_base64(&base64(&all)), Some(all));
        for broken in ["Zm9", "Zm9v!", "Z===", "*m9v"] {
            assert_eq!(from_base64(broken), None, "{broken}");
        }
        assert_eq!(Picture::bytes_of(&serde_json::json!({ "format": "webp" })), None);
    }

    #[test]
    fn only_a_few_slow_jobs_run_at_once() {
        static TURNS: Turns = Turns::new(2);
        static RUNNING: AtomicUsize = AtomicUsize::new(0);
        static MOST: AtomicUsize = AtomicUsize::new(0);
        let jobs: Vec<_> = (0..12)
            .map(|_| {
                std::thread::spawn(|| {
                    let _turn = TURNS.wait_for_a_turn();
                    let now = RUNNING.fetch_add(1, Ordering::SeqCst) + 1;
                    MOST.fetch_max(now, Ordering::SeqCst);
                    std::thread::sleep(std::time::Duration::from_millis(5));
                    RUNNING.fetch_sub(1, Ordering::SeqCst);
                })
            })
            .collect();
        for job in jobs {
            job.join().unwrap();
        }
        assert_eq!(MOST.load(Ordering::SeqCst), 2);
        assert_eq!(RUNNING.load(Ordering::SeqCst), 0);
    }
}
