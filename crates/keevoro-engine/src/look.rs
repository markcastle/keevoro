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
//! properly its pictures are already made. The pictures sit in the same
//! cache as those, under its 1 GB limit.

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
                if let Ok(bytes) = std::fs::read(name(&id)) {
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
    if let Ok(bytes) = std::fs::read(name(id)) {
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
        let times: Vec<SystemTime> = made
            .iter()
            .map(|p| fs::metadata(p).unwrap().modified().unwrap())
            .collect();

        let library = dir.path().join("library");
        let mut catalog = Catalog::open_in_library(&library).unwrap();
        let quick = keevoro_scan::ScanOptions {
            quick: true,
            ..Default::default()
        };
        let report = crate::scan_into_with(&mut catalog, &source, None, &quick, &mut |_| {}, &mut || {}).unwrap();
        let id = report.items[0].id().unwrap().to_string();
        crate::preview(&library, &catalog, &cache, &id, None).unwrap();
        let again: Vec<SystemTime> = made
            .iter()
            .map(|p| fs::metadata(p).unwrap().modified().unwrap())
            .collect();
        assert_eq!(times, again, "no picture was made twice");
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
}
