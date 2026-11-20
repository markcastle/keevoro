//! Looking inside a folder before it is added: what photos and videos it
//! holds, and a small picture of each, made as they come on screen.
//!
//! Nothing here is ever added to a backup, and nothing is written anywhere
//! but Keevoro's own cache folder: the pictures, and a small index
//! (`look.sqlite`) that remembers which picture belongs to which file, so a
//! second look reads the picture straight back without reading the file.
//! Files are only ever read, and one only in online storage is never even
//! opened, since opening it would download it.
//!
//! A picture is named by the fingerprint a search would give the file
//! (`keevoro_scan::fingerprint`), the same name the Discover screen's
//! pictures have (`preview.rs`), so once the folder is looked through
//! properly its pictures are already made. When the check for duplicates
//! later reads such a file in full, its pictures are renamed to the whole
//! fingerprint ([`now_known_as`]), so they are still found.
//!
//! Everything here sits in the same cache as those pictures, under its
//! 1 GB limit ([`trim_cache`]), which removes the pictures used longest ago
//! first. A picture removed while it is on screen is simply made again the
//! next time it is asked for.

use crate::preview::Look;
use anyhow::{anyhow, bail, Result};
use keevoro_catalog::Catalog;
use keevoro_core::{Kinds, MediaKind};
use keevoro_media::{render_photo, video, write_rendition, VideoTools};
use keevoro_scan::OnlineCheck;
use rusqlite::{params, Connection, OptionalExtension};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// A file that could not be pictured is not tried again for this long.
const RETRY_AFTER: i64 = 24 * 60 * 60;

/// The most files the index remembers; the least recently used go first.
const REMEMBERED: i64 = 100_000;

/// One photo or video in a folder, from the folder's listing alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    pub path: PathBuf,
    pub name: String,
    pub kind: MediaKind,
    pub bytes: u64,
    /// When it was last changed, in seconds since 1970. Not when it was
    /// taken: that is inside the file, which is not opened here.
    pub modified: i64,
    /// Only in online storage, not on this computer: never opened.
    pub online_only: bool,
}

impl FileEntry {
    /// How long a video lasts, in seconds, from its movie header: a few
    /// small reads and no decoding. `None` for a photo, for a video whose
    /// container does not say (only MP4, MOV, M4V and 3GP do), and always
    /// for a file only in online storage, which is never opened.
    pub fn length(&self) -> Option<f64> {
        if self.kind != MediaKind::Video || self.online_only {
            return None;
        }
        keevoro_scan::video::length_of(&self.path)
    }
}

/// The photos and videos directly in `folder`, by name, from one listing.
/// Only kinds of file `kinds` looks for are listed; links are not
/// followed, and names Keevoro always passes over (such as `._` files a
/// Mac leaves) are left out.
pub fn list_folder(folder: &Path, kinds: &Kinds, online: &dyn OnlineCheck) -> std::io::Result<Vec<FileEntry>> {
    let rules = keevoro_core::SkipRules::default();
    let mut files = Vec::new();
    for entry in std::fs::read_dir(folder)?.flatten() {
        let Ok(kind_of_entry) = entry.file_type() else { continue };
        if !kind_of_entry.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if rules.skip_file(&name).is_some() {
            continue;
        }
        let Some(kind) = name.rsplit_once('.').and_then(|(_, ending)| kinds.looked_for(ending)) else {
            continue;
        };
        let Ok(listing) = entry.metadata() else { continue };
        let path = entry.path();
        files.push(FileEntry {
            online_only: online.online_only(&path, &listing),
            modified: seconds(listing.modified().ok()),
            bytes: listing.len(),
            name,
            kind,
            path,
        });
    }
    files.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then(a.name.cmp(&b.name))
    });
    Ok(files)
}

fn seconds(time: Option<SystemTime>) -> i64 {
    time.and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

/// Why a picture of a file was not made, in words a person can read.
pub const ONLINE_ONLY: &str = "This one is only in online storage, not on this computer, so Keevoro won't open it.";

/// A file already found by a search, whose picture is that search's.
pub struct Found<'a> {
    pub home: &'a Path,
    pub catalog: &'a Catalog,
}

/// A small or screen-sized WebP picture of `file`, a photo or video of
/// `kind`, made (or found already made) in `cache`. Only `cache` is
/// written. `found` is a backup whose search may know the file already.
pub fn picture_of_file(
    cache: &Path,
    file: &Path,
    kind: MediaKind,
    look: Look,
    video_tools: Option<&VideoTools>,
    online: &dyn OnlineCheck,
    found: Option<Found<'_>>,
) -> Result<Vec<u8>> {
    // What the listing says, without opening the file.
    let listing = std::fs::symlink_metadata(file).map_err(|_| anyhow!("That file isn't there any more."))?;
    if !listing.is_file() {
        bail!("That isn't a file.");
    }
    if online.online_only(file, &listing) {
        bail!(ONLINE_ONLY);
    }
    let index = Index::open(cache);
    let key = listing_key(file, &listing);
    let name = |id: &str| {
        cache.join(if kind == MediaKind::Photo && look == Look::Closer {
            format!("{id}.display.webp")
        } else {
            format!("{id}.webp")
        })
    };
    // Seen before: the picture is read back, or the problem said again.
    if let Some(seen) = index.as_ref().and_then(|i| i.seen(&key)) {
        match seen {
            Seen::Picture(id) => {
                if let Some(bytes) = read_and_use(&name(&id)) {
                    return Ok(bytes);
                }
            }
            Seen::Problem(problem, at) if now() - at < RETRY_AFTER => bail!(problem),
            Seen::Problem(..) => {}
        }
    }
    // Found by a search already: that search's picture, the backup's own
    // preview if it has made one.
    if let Some(found) = found {
        if let Some(id) = found.catalog.id_at(file, listing.len())? {
            if let Ok(bytes) = crate::preview_at(found.home, found.catalog, cache, &id, video_tools, look) {
                return Ok(bytes);
            }
        }
    }
    let id = keevoro_scan::fingerprint(file, true)?;
    let id = &id[..16];
    if let Some(bytes) = read_and_use(&name(id)) {
        if let Some(index) = &index {
            index.remember(&key, Ok(id));
        }
        return Ok(bytes);
    }
    let _busy = keevoro_core::doing::doing(keevoro_core::doing::Work::MakingThumbnails);
    let made = match kind {
        MediaKind::Photo => render_photo(file).map(|r| {
            // Both sizes come from one decoding; keep both.
            let _ = write_rendition(&cache.join(format!("{id}.webp")), &r.thumb);
            let _ = write_rendition(&cache.join(format!("{id}.display.webp")), &r.display);
            if look == Look::Closer {
                r.display
            } else {
                r.thumb
            }
        }),
        MediaKind::Video => video::still(video_tools, file).map(|still| {
            let _ = write_rendition(&cache.join(format!("{id}.webp")), &still.webp);
            still.webp
        }),
    };
    match made {
        Ok(bytes) => {
            if let Some(index) = &index {
                index.remember(&key, Ok(id));
            }
            Ok(bytes)
        }
        Err(_) => {
            let problem = "This one can't be shown.";
            if let Some(index) = &index {
                index.remember(&key, Err(problem));
            }
            bail!(problem)
        }
    }
}

fn now() -> i64 {
    seconds(Some(SystemTime::now()))
}

/// A picture from the cache, marked as just used, so the cache's limit
/// removes the pictures used longest ago, not those made longest ago.
pub fn read_and_use(picture: &Path) -> Option<Vec<u8>> {
    let bytes = std::fs::read(picture).ok()?;
    if let Ok(file) = std::fs::File::options().write(true).open(picture) {
        let _ = file.set_modified(SystemTime::now());
    }
    Some(bytes)
}

/// The two pictures a file can have in the cache, under a fingerprint's
/// first sixteen letters.
fn picture_names(id16: &str) -> [String; 2] {
    [format!("{id16}.webp"), format!("{id16}.display.webp")]
}

/// A file once known by its quick fingerprint is now known by its whole
/// one (the check for duplicates has read it in full). Its pictures in
/// `cache` take the new name, and the index follows, so nothing is made
/// twice. A picture already there under the new name is kept as it is.
/// Only Keevoro's own pictures in its own cache are ever moved.
pub fn now_known_as(cache: &Path, quick: &str, whole: &str) {
    let (Some(quick), Some(whole)) = (quick.get(..16), whole.get(..16)) else {
        return;
    };
    if quick == whole {
        return;
    }
    for (from, to) in picture_names(quick).iter().zip(picture_names(whole)) {
        let (from, to) = (cache.join(from), cache.join(to));
        if from.is_file() {
            if to.exists() {
                let _ = std::fs::remove_file(&from);
            } else {
                let _ = std::fs::rename(&from, &to);
            }
        }
    }
    if cache.join("look.sqlite").is_file() {
        if let Some(index) = Index::open(cache) {
            let _ = index
                .0
                .execute("UPDATE seen SET id = ?1 WHERE id = ?2", params![whole, quick]);
        }
    }
}

/// The most room Keevoro's cache may take: the pictures, the index and
/// the remembered surveys together. Past it the pictures used longest ago
/// are removed; any can be made again.
pub const CACHE_LIMIT: u64 = 1024 * 1024 * 1024;

/// Keeps everything in `cache` within `limit`, removing the pictures used
/// longest ago first. Everything there counts towards the limit, but only
/// plain `.webp` files directly in the cache folder, which Keevoro made,
/// are ever removed. Answers the bytes freed.
pub fn trim_cache(cache: &Path, limit: u64) -> u64 {
    let Ok(entries) = std::fs::read_dir(cache) else {
        return 0;
    };
    let mut total = 0;
    let mut pictures: Vec<(SystemTime, u64, PathBuf)> = Vec::new();
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.is_dir() {
            total += size_of_folder(&path);
            continue;
        }
        total += meta.len();
        if meta.is_file() && path.extension().is_some_and(|x| x == "webp") {
            pictures.push((meta.modified().unwrap_or(UNIX_EPOCH), meta.len(), path));
        }
    }
    pictures.sort();
    let mut freed = 0;
    for (_, bytes, path) in pictures {
        if total <= limit {
            break;
        }
        if std::fs::remove_file(&path).is_ok() {
            total = total.saturating_sub(bytes);
            freed += bytes;
        }
    }
    freed
}

/// Trims the cache to [`CACHE_LIMIT`] on a thread of its own at low
/// priority, so nobody waits for it. One trim at a time: asked again while
/// one runs, it does nothing.
pub fn trim_cache_quietly(cache: &Path) {
    use std::sync::atomic::{AtomicBool, Ordering};
    static TRIMMING: AtomicBool = AtomicBool::new(false);
    if TRIMMING.swap(true, Ordering::SeqCst) {
        return;
    }
    let cache = cache.to_path_buf();
    let started = std::thread::Builder::new().name("keevoro cache".into()).spawn(move || {
        keevoro_frame::quiet::lower_this_threads_priority();
        trim_cache(&cache, CACHE_LIMIT);
        TRIMMING.store(false, Ordering::SeqCst);
    });
    if started.is_err() {
        TRIMMING.store(false, Ordering::SeqCst);
    }
}

/// The bytes in the files directly in `folder` (Keevoro's own folders in
/// its cache are one level deep).
fn size_of_folder(folder: &Path) -> u64 {
    std::fs::read_dir(folder)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter_map(|e| std::fs::symlink_metadata(e.path()).ok())
        .filter(std::fs::Metadata::is_file)
        .map(|m| m.len())
        .sum()
}

/// What a file's listing says about it, as one key: where it is, its size
/// and when it was changed. A file changed since gets a new key.
fn listing_key(file: &Path, listing: &std::fs::Metadata) -> String {
    let changed = listing
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_nanos());
    let mut hasher = blake3::Hasher::new();
    hasher.update(file.to_string_lossy().as_bytes());
    hasher.update(&listing.len().to_le_bytes());
    hasher.update(&changed.to_le_bytes());
    hasher.finalize().to_hex()[..32].to_string()
}

enum Seen {
    Picture(String),
    Problem(String, i64),
}

/// Which picture belongs to which file, kept in the cache. It is only a
/// help: if it cannot be opened, pictures are simply worked out afresh.
struct Index(Connection);

impl Index {
    fn open(cache: &Path) -> Option<Self> {
        std::fs::create_dir_all(cache).ok()?;
        let conn = Connection::open(cache.join("look.sqlite")).ok()?;
        // The app and the command line may share it.
        let _ = conn.busy_timeout(std::time::Duration::from_secs(2));
        let _ = conn.pragma_update(None, "journal_mode", "WAL");
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS seen (
                 key TEXT PRIMARY KEY,
                 id TEXT,
                 problem TEXT,
                 at INTEGER NOT NULL,
                 used INTEGER NOT NULL
             );
             CREATE INDEX IF NOT EXISTS seen_used ON seen (used);",
        )
        .ok()?;
        Some(Self(conn))
    }

    fn seen(&self, key: &str) -> Option<Seen> {
        let row: Option<(Option<String>, Option<String>, i64)> = self
            .0
            .query_row("SELECT id, problem, at FROM seen WHERE key = ?1", [key], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .optional()
            .ok()
            .flatten();
        let _ = self
            .0
            .execute("UPDATE seen SET used = ?1 WHERE key = ?2", params![now(), key]);
        match row? {
            (Some(id), _, _) => Some(Seen::Picture(id)),
            (None, Some(problem), at) => Some(Seen::Problem(problem, at)),
            _ => None,
        }
    }

    fn remember(&self, key: &str, what: std::result::Result<&str, &str>) {
        let (id, problem) = match what {
            Ok(id) => (Some(id), None),
            Err(problem) => (None, Some(problem)),
        };
        let _ = self.0.execute(
            "INSERT OR REPLACE INTO seen (key, id, problem, at, used) VALUES (?1, ?2, ?3, ?4, ?4)",
            params![key, id, problem, now()],
        );
        let _ = self.0.execute(
            "DELETE FROM seen WHERE key IN (SELECT key FROM seen ORDER BY used DESC LIMIT -1 OFFSET ?1)",
            [REMEMBERED],
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use keevoro_scan::ByAttributes;
    use std::collections::BTreeMap;
    use std::fs;

    fn real_jpeg(shade: u8) -> Vec<u8> {
        let img = image::RgbImage::from_pixel(1200, 800, image::Rgb([shade, 120, 60]));
        let mut bytes = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Jpeg)
            .unwrap();
        bytes
    }

    /// A photo too busy to squeeze small, so the search samples it.
    fn big_jpeg() -> Vec<u8> {
        let mut seed = 7u32;
        let img = image::RgbImage::from_fn(1600, 1200, |_, _| {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let [a, b, c, _] = seed.to_le_bytes();
            image::Rgb([a, b, c])
        });
        let mut bytes = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Jpeg)
            .unwrap();
        bytes
    }

    /// Every file under `dir`, with its bytes and when it was changed.
    fn snapshot(dir: &Path) -> BTreeMap<PathBuf, (Vec<u8>, SystemTime)> {
        let mut all = BTreeMap::new();
        for entry in walk(dir) {
            let meta = fs::metadata(&entry).unwrap();
            all.insert(entry.clone(), (fs::read(&entry).unwrap(), meta.modified().unwrap()));
        }
        all
    }

    fn walk(dir: &Path) -> Vec<PathBuf> {
        let mut out = Vec::new();
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                if entry.path().is_dir() {
                    out.extend(walk(&entry.path()));
                } else {
                    out.push(entry.path());
                }
            }
        }
        out
    }

    #[derive(Debug)]
    struct NamedOnline;

    impl OnlineCheck for NamedOnline {
        fn online_only(&self, file: &Path, _: &std::fs::Metadata) -> bool {
            file.to_string_lossy().contains("(online)")
        }
    }

    #[test]
    fn a_folder_is_listed_by_name_with_only_the_kinds_looked_for() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["b.JPG", "a.jpg", "clip.mp4", "notes.txt", "._a.jpg", "C (online).jpg"] {
            fs::write(dir.path().join(name), b"x").unwrap();
        }
        fs::create_dir(dir.path().join("inside.jpg")).unwrap();
        let listed = list_folder(dir.path(), &Kinds::default(), &NamedOnline).unwrap();
        let names: Vec<&str> = listed.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["a.jpg", "b.JPG", "C (online).jpg", "clip.mp4"]);
        assert_eq!(listed[3].kind, MediaKind::Video);
        assert!(listed[2].online_only && !listed[0].online_only);
        let photos_only = list_folder(dir.path(), &Kinds::default().only(MediaKind::Photo), &NamedOnline).unwrap();
        assert_eq!(photos_only.len(), 3, "a backup for photos only lists no videos");
    }

    #[test]
    fn looking_leaves_the_source_exactly_as_it_was_and_writes_only_in_the_cache() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("Pictures");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("a.jpg"), real_jpeg(40)).unwrap();
        let cache = dir.path().join("cache");
        let before = snapshot(dir.path());

        let small = picture_of_file(
            &cache,
            &source.join("a.jpg"),
            MediaKind::Photo,
            Look::Small,
            None,
            &ByAttributes,
            None,
        )
        .unwrap();
        assert_eq!(&small[8..12], b"WEBP");
        let closer = picture_of_file(
            &cache,
            &source.join("a.jpg"),
            MediaKind::Photo,
            Look::Closer,
            None,
            &ByAttributes,
            None,
        )
        .unwrap();
        assert_ne!(closer, small);

        let after = snapshot(dir.path());
        for (path, was) in &before {
            assert_eq!(after.get(path), Some(was), "{path:?} is byte for byte as it was");
        }
        let new: Vec<&PathBuf> = after.keys().filter(|p| !before.contains_key(*p)).collect();
        assert!(!new.is_empty());
        assert!(
            new.iter().all(|p| p.starts_with(&cache)),
            "only the cache is written: {new:?}"
        );
    }

    #[test]
    fn the_second_look_reads_the_index_not_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.jpg");
        fs::write(&file, real_jpeg(80)).unwrap();
        let cache = dir.path().join("cache");
        let first = picture_of_file(&cache, &file, MediaKind::Photo, Look::Small, None, &ByAttributes, None).unwrap();
        // The file can no longer be read; its listing is unchanged.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&file, fs::Permissions::from_mode(0o000)).unwrap();
        }
        let second = picture_of_file(&cache, &file, MediaKind::Photo, Look::Small, None, &ByAttributes, None).unwrap();
        assert_eq!(first, second);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&file, fs::Permissions::from_mode(0o644)).unwrap();
        }
        let index = Index::open(&cache).unwrap();
        assert!(matches!(
            index.seen(&listing_key(&file, &fs::metadata(&file).unwrap())),
            Some(Seen::Picture(_))
        ));
    }

    #[test]
    fn a_file_only_online_is_refused_without_being_opened() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("away (online).jpg");
        fs::write(&file, real_jpeg(10)).unwrap();
        #[cfg(unix)]
        {
            // Opening it would fail differently: it must not be opened.
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&file, fs::Permissions::from_mode(0o000)).unwrap();
        }
        let cache = dir.path().join("cache");
        let e = picture_of_file(&cache, &file, MediaKind::Photo, Look::Small, None, &NamedOnline, None).unwrap_err();
        assert_eq!(e.to_string(), ONLINE_ONLY);
        assert!(walk(&cache).iter().all(|p| p.extension().is_none_or(|x| x != "webp")));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&file, fs::Permissions::from_mode(0o644)).unwrap();
        }
    }

    #[test]
    fn a_file_that_cannot_be_shown_says_so_and_is_not_tried_again_at_once() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("broken.jpg");
        fs::write(&file, b"not really a photo").unwrap();
        let cache = dir.path().join("cache");
        let e = picture_of_file(&cache, &file, MediaKind::Photo, Look::Small, None, &ByAttributes, None).unwrap_err();
        assert_eq!(e.to_string(), "This one can't be shown.");
        let index = Index::open(&cache).unwrap();
        assert!(matches!(
            index.seen(&listing_key(&file, &fs::metadata(&file).unwrap())),
            Some(Seen::Problem(..))
        ));
    }

    #[test]
    fn a_later_search_finds_the_picture_already_made() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("Pictures");
        fs::create_dir_all(&source).unwrap();
        // Big enough for the quick fingerprint the app's searches use.
        fs::write(source.join("a.jpg"), big_jpeg()).unwrap();
        assert!(fs::metadata(source.join("a.jpg")).unwrap().len() > 256 * 1024);
        let cache = dir.path().join("cache");
        let file = fs::canonicalize(source.join("a.jpg")).unwrap();
        picture_of_file(&cache, &file, MediaKind::Photo, Look::Small, None, &ByAttributes, None).unwrap();
        let made: Vec<PathBuf> = walk(&cache)
            .into_iter()
            .filter(|p| p.extension().is_some_and(|x| x == "webp"))
            .collect();

        let library = dir.path().join("library");
        let mut catalog = Catalog::open_in_library(&library).unwrap();
        let quick = keevoro_scan::ScanOptions {
            quick: true,
            ..Default::default()
        };
        let report = crate::scan_into_with(&mut catalog, &source, None, &quick, &mut |_| {}, &mut || {}).unwrap();
        let id = report.items[0].id().unwrap().to_string();
        // With the photo moved away, a picture can only come from the cache.
        let aside = dir.path().join("aside.jpg");
        fs::rename(&file, &aside).unwrap();
        crate::preview(&library, &catalog, &cache, &id, None).expect("no picture was made twice");
        fs::rename(&aside, &file).unwrap();
        assert!(
            cache.join(format!("{id}.webp")).is_file(),
            "the search's picture is the one made while looking"
        );
        assert_eq!(
            walk(&cache)
                .iter()
                .filter(|p| p.extension().is_some_and(|x| x == "webp"))
                .count(),
            made.len()
        );

        // And looked at again with the backup in hand, it is the search's.
        let found = Found {
            home: &library,
            catalog: &catalog,
        };
        picture_of_file(
            &cache,
            &file,
            MediaKind::Photo,
            Look::Small,
            None,
            &ByAttributes,
            Some(found),
        )
        .unwrap();
    }

    fn age(path: &Path, seconds_ago: u64) {
        let file = fs::File::options().write(true).open(path).unwrap();
        file.set_modified(SystemTime::now() - std::time::Duration::from_secs(seconds_ago))
            .unwrap();
    }

    #[test]
    fn the_cache_is_kept_within_its_limit_used_longest_ago_first_and_only_pictures_are_removed() {
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().join("previews");
        fs::create_dir_all(cache.join("surveys")).unwrap();
        for (n, ago) in [(1, 300), (2, 200), (3, 100)] {
            let path = cache.join(format!("{n}.webp"));
            fs::write(&path, vec![0u8; 1000]).unwrap();
            age(&path, ago);
        }
        // The oldest is looked at again, so it is the newest used.
        assert!(read_and_use(&cache.join("1.webp")).is_some());
        // The index and the surveys count towards the limit, and are kept.
        fs::write(cache.join("look.sqlite"), vec![0u8; 500]).unwrap();
        fs::write(cache.join("surveys").join("a.jsonl"), vec![0u8; 500]).unwrap();
        assert_eq!(trim_cache(&cache, 3000), 1000);
        assert!(!cache.join("2.webp").exists(), "used longest ago");
        assert!(cache.join("1.webp").is_file() && cache.join("3.webp").is_file());
        assert!(cache.join("look.sqlite").is_file() && cache.join("surveys").join("a.jsonl").is_file());
        assert_eq!(trim_cache(&cache, 3000), 0);
        assert_eq!(trim_cache(&dir.path().join("none"), 0), 0);
        // Nothing outside the cache is ever touched, however low the limit.
        fs::write(dir.path().join("mine.webp"), b"x").unwrap();
        trim_cache(&cache, 0);
        assert!(dir.path().join("mine.webp").is_file());
    }

    #[test]
    fn a_picture_the_limit_removed_is_simply_made_again() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.jpg");
        fs::write(&file, real_jpeg(90)).unwrap();
        let cache = dir.path().join("cache");
        let first = picture_of_file(&cache, &file, MediaKind::Photo, Look::Small, None, &ByAttributes, None).unwrap();
        // Even the picture on screen goes, when the limit says so.
        trim_cache(&cache, 0);
        assert!(walk(&cache).iter().all(|p| p.extension().is_none_or(|x| x != "webp")));
        let again = picture_of_file(&cache, &file, MediaKind::Photo, Look::Small, None, &ByAttributes, None).unwrap();
        assert_eq!(&again[8..12], b"WEBP");
        assert_eq!(again.len(), first.len(), "the same picture, made again");
        let closer = picture_of_file(&cache, &file, MediaKind::Photo, Look::Closer, None, &ByAttributes, None).unwrap();
        assert_eq!(&closer[8..12], b"WEBP");
    }

    /// The least of an MP4: a movie header saying 61,200 ticks of 1/600 of
    /// a second.
    fn mp4_lasting_102_seconds() -> Vec<u8> {
        let boxed = |name: &[u8; 4], body: &[u8]| {
            let mut v = u32::try_from(body.len() + 8).unwrap().to_be_bytes().to_vec();
            v.extend_from_slice(name);
            v.extend_from_slice(body);
            v
        };
        let mut mvhd = vec![0u8; 12];
        mvhd.extend_from_slice(&600u32.to_be_bytes());
        mvhd.extend_from_slice(&61_200u32.to_be_bytes());
        mvhd.extend_from_slice(&[0; 16]);
        [boxed(b"ftyp", b"isom"), boxed(b"moov", &boxed(b"mvhd", &mvhd))].concat()
    }

    fn entry(path: &Path, kind: MediaKind, online_only: bool) -> FileEntry {
        FileEntry {
            path: path.to_path_buf(),
            name: path.file_name().unwrap().to_string_lossy().into_owned(),
            kind,
            bytes: 0,
            modified: 0,
            online_only,
        }
    }

    #[test]
    fn a_videos_length_comes_from_its_header_and_never_from_a_file_only_online() {
        let dir = tempfile::tempdir().unwrap();
        let clip = dir.path().join("clip.mp4");
        fs::write(&clip, mp4_lasting_102_seconds()).unwrap();
        assert_eq!(entry(&clip, MediaKind::Video, false).length(), Some(102.0));
        assert_eq!(entry(&clip, MediaKind::Photo, false).length(), None, "a photo has none");
        #[cfg(unix)]
        {
            // Opening it would fail: it must not be opened at all.
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&clip, fs::Permissions::from_mode(0o000)).unwrap();
        }
        assert_eq!(entry(&clip, MediaKind::Video, true).length(), None);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&clip, fs::Permissions::from_mode(0o644)).unwrap();
        }
    }

    #[test]
    fn pictures_follow_a_file_to_its_whole_fingerprint() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.jpg");
        fs::write(&file, big_jpeg()).unwrap();
        let cache = dir.path().join("cache");
        picture_of_file(&cache, &file, MediaKind::Photo, Look::Small, None, &ByAttributes, None).unwrap();
        let quick = keevoro_scan::fingerprint(&file, true).unwrap();
        let whole = keevoro_scan::whole_hash(&file).unwrap();
        assert_ne!(quick, whole);
        now_known_as(&cache, &quick, &whole);
        assert!(!cache.join(format!("{}.webp", &quick[..16])).exists());
        let renamed = cache.join(format!("{}.webp", &whole[..16]));
        let display = cache.join(format!("{}.display.webp", &whole[..16]));
        assert!(renamed.is_file() && display.is_file());
        let made = fs::metadata(&renamed).unwrap().len();
        // The index follows, so looking again reads it, and makes nothing.
        age(&renamed, 1000);
        let before = walk(&cache).len();
        picture_of_file(&cache, &file, MediaKind::Photo, Look::Small, None, &ByAttributes, None).unwrap();
        assert_eq!(walk(&cache).len(), before, "nothing new was made");
        assert_eq!(fs::metadata(&renamed).unwrap().len(), made);
        // A name already taken is left as it is.
        fs::write(cache.join("1111111111111111.webp"), b"old").unwrap();
        fs::write(cache.join("2222222222222222.webp"), b"kept").unwrap();
        now_known_as(&cache, "1111111111111111aa", "2222222222222222bb");
        assert_eq!(fs::read(cache.join("2222222222222222.webp")).unwrap(), b"kept");
        assert!(!cache.join("1111111111111111.webp").exists());
    }
}
