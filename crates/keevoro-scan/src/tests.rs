use super::*;
use std::fs;
use tempfile::TempDir;

/// A tiny but valid JPEG carrying only an EXIF DateTimeOriginal.
fn jpeg_with_exif(date: &str) -> Vec<u8> {
    assert_eq!(date.len(), 19);
    let mut tiff = Vec::new();
    tiff.extend_from_slice(b"MM\0\x2a\0\0\0\x08"); // big-endian header, IFD0 at 8
    tiff.extend_from_slice(&[0, 1]); // IFD0: one entry
    tiff.extend_from_slice(&[0x87, 0x69, 0, 4, 0, 0, 0, 1, 0, 0, 0, 26]); // Exif IFD pointer -> 26
    tiff.extend_from_slice(&[0, 0, 0, 0]); // no next IFD
    tiff.extend_from_slice(&[0, 1]); // Exif IFD: one entry
    tiff.extend_from_slice(&[0x90, 0x03, 0, 2, 0, 0, 0, 20, 0, 0, 0, 44]); // DateTimeOriginal at 44
    tiff.extend_from_slice(&[0, 0, 0, 0]);
    tiff.extend_from_slice(date.as_bytes());
    tiff.push(0);

    let mut app1 = b"Exif\0\0".to_vec();
    app1.extend_from_slice(&tiff);
    let len = u16::try_from(app1.len() + 2).unwrap();

    let mut jpeg = vec![0xFF, 0xD8, 0xFF, 0xE1];
    jpeg.extend_from_slice(&len.to_be_bytes());
    jpeg.extend_from_slice(&app1);
    jpeg.extend_from_slice(&[0xFF, 0xD9]);
    jpeg
}

fn plain_jpeg(fill: u8) -> Vec<u8> {
    let mut j = vec![
        0xFF, 0xD8, 0xFF, 0xE0, 0, 16, b'J', b'F', b'I', b'F', 0, 1, 1, 0, 0, 1, 0, 1, 0, 0,
    ];
    j.extend(std::iter::repeat_n(fill, 200));
    j.extend_from_slice(&[0xFF, 0xD9]);
    j
}

fn mp4() -> Vec<u8> {
    let mut v = vec![0, 0, 0, 0x18];
    v.extend_from_slice(b"ftypisom\0\0\x02\0isomiso2");
    v.extend(std::iter::repeat_n(0u8, 100));
    v
}

struct Tree(TempDir);

impl Tree {
    fn new() -> Self {
        Self(tempfile::tempdir().unwrap())
    }
    fn add(&self, rel: &str, bytes: &[u8]) -> PathBuf {
        let p = self.0.path().join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, bytes).unwrap();
        p
    }
    fn root(&self) -> &Path {
        self.0.path()
    }
    fn scan(&self) -> ScanReport {
        scan(self.root(), &ScanOptions::default(), &mut |_| {})
    }
}

fn item<'a>(r: &'a ScanReport, name: &str) -> &'a FoundItem {
    r.items
        .iter()
        .find(|i| i.path.file_name().unwrap() == name)
        .unwrap_or_else(|| panic!("{name} not found"))
}

#[test]
fn finds_media_by_content_not_extension() {
    let t = Tree::new();
    t.add("renamed.dat", &plain_jpeg(1));
    t.add("notes.jpg", b"just some text, not a photo at all");
    t.add("clip.mp4", &mp4());
    let r = t.scan();
    assert_eq!(r.items.len(), 2);
    assert_eq!(item(&r, "renamed.dat").kind, MediaKind::Photo);
    assert_eq!(item(&r, "renamed.dat").mime, "image/jpeg");
    assert_eq!(item(&r, "clip.mp4").kind, MediaKind::Video);
    assert_eq!(r.other_files, 1);
}

#[test]
fn falls_back_to_extension_for_formats_without_magic_bytes() {
    let t = Tree::new();
    t.add("00001.MTS", &[0x47, 1, 2, 3]);
    let r = t.scan();
    assert_eq!(item(&r, "00001.MTS").kind, MediaKind::Video);
}

#[test]
fn camera_date_beats_file_name_and_modified_time() {
    let t = Tree::new();
    t.add("IMG_20200101_000000.jpg", &jpeg_with_exif("2009:07:14 21:03:11"));
    let r = t.scan();
    let d = item(&r, "IMG_20200101_000000.jpg").date.unwrap();
    assert_eq!(d.source, DateSource::Exif);
    assert_eq!(d.date.to_string(), "2009-07-14T21:03:11");
}

#[test]
fn uses_file_name_then_folder_then_modified_time() {
    let t = Tree::new();
    t.add("a/IMG_20120304_153012.jpg", &plain_jpeg(1));
    t.add("2009-07 Spain/IMG_0001.jpg", &plain_jpeg(2));
    t.add("misc/IMG_0002.jpg", &plain_jpeg(3));
    let r = t.scan();
    assert_eq!(
        item(&r, "IMG_20120304_153012.jpg").date.unwrap().source,
        DateSource::Filename
    );
    let spain = item(&r, "IMG_0001.jpg").date.unwrap();
    assert_eq!(
        (spain.source, spain.date.to_string()),
        (DateSource::Folder, "2009-07".to_string())
    );
    assert_eq!(item(&r, "IMG_0002.jpg").date.unwrap().source, DateSource::Modified);
}

#[test]
fn reads_album_hints_from_the_users_folders() {
    let t = Tree::new();
    t.add(
        "My Pictures/Holidays/Cornwall/DCIM/100CANON/IMG_0001.JPG",
        &plain_jpeg(1),
    );
    let r = t.scan();
    assert_eq!(
        item(&r, "IMG_0001.JPG").folder.album_label().as_deref(),
        Some("Holidays / Cornwall")
    );
}

#[test]
fn skips_system_folders_and_says_why() {
    let t = Tree::new();
    t.add("Windows/Web/Wallpaper/img0.jpg", &plain_jpeg(1));
    t.add("Users/mark/AppData/Local/cache.jpg", &plain_jpeg(2));
    t.add("Users/mark/Pictures/keep.jpg", &plain_jpeg(3));
    t.add("Users/mark/Pictures/._keep.jpg", &plain_jpeg(4));
    let r = t.scan();
    let names: Vec<_> = r
        .items
        .iter()
        .map(|i| i.path.file_name().unwrap().to_string_lossy().to_string())
        .collect();
    assert_eq!(names, vec!["keep.jpg"]);
    let mut reasons: Vec<_> = r.skipped.iter().map(|s| s.reason).collect();
    reasons.sort_by_key(|r| r.describe());
    assert_eq!(reasons, vec![SkipReason::AppFolder, SkipReason::SystemFolder]);
}

#[test]
fn identical_files_share_a_hash() {
    let t = Tree::new();
    t.add("laptop/Spain/IMG_0001.jpg", &plain_jpeg(7));
    t.add("backup/Spain/IMG_0001.jpg", &plain_jpeg(7));
    t.add("backup/Spain/IMG_0002.jpg", &plain_jpeg(8));
    let r = t.scan();
    let hashes: std::collections::HashSet<_> = r.items.iter().map(|i| i.blake3.clone().unwrap()).collect();
    assert_eq!(hashes.len(), 2);
    let id = r.items[0].id().unwrap();
    assert_eq!(id.len(), 16);
    assert!(id.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn hashing_can_be_turned_off() {
    let t = Tree::new();
    t.add("a.jpg", &plain_jpeg(1));
    let r = scan(
        t.root(),
        &ScanOptions {
            hash: false,
            ..Default::default()
        },
        &mut |_| {},
    );
    assert!(r.items[0].blake3.is_none());
    assert!(r.items[0].id().is_none());
}

#[test]
fn hash_covers_the_whole_file_not_just_the_head() {
    let t = Tree::new();
    let mut a = plain_jpeg(1);
    a.extend(std::iter::repeat_n(0u8, SNIFF_BYTES * 3));
    let mut b = a.clone();
    *b.last_mut().unwrap() = 1;
    t.add("a.jpg", &a);
    t.add("b.jpg", &b);
    let r = t.scan();
    assert_ne!(item(&r, "a.jpg").blake3, item(&r, "b.jpg").blake3);
    assert_eq!(
        item(&r, "a.jpg").blake3.as_deref().unwrap(),
        blake3::hash(&a).to_hex().as_str()
    );
}

#[test]
fn never_changes_the_source() {
    let t = Tree::new();
    let p = t.add("Holidays/IMG_0001.jpg", &jpeg_with_exif("2009:07:14 21:03:11"));
    let before = (fs::read(&p).unwrap(), fs::metadata(&p).unwrap().modified().unwrap());
    let entries_before: Vec<_> = walkdir::WalkDir::new(t.root())
        .into_iter()
        .map(|e| e.unwrap().into_path())
        .collect();
    t.scan();
    let after = (fs::read(&p).unwrap(), fs::metadata(&p).unwrap().modified().unwrap());
    let entries_after: Vec<_> = walkdir::WalkDir::new(t.root())
        .into_iter()
        .map(|e| e.unwrap().into_path())
        .collect();
    assert_eq!(before, after);
    assert_eq!(entries_before, entries_after, "scan must not create or remove files");
}

#[test]
fn reports_progress_as_it_goes() {
    let t = Tree::new();
    t.add("a.jpg", &plain_jpeg(1));
    t.add("b.mp4", &mp4());
    t.add("c.txt", b"text");
    let mut seen = Vec::new();
    scan(t.root(), &ScanOptions::default(), &mut |p| seen.push(*p));
    assert_eq!(seen.len(), 3);
    assert_eq!(
        seen.last().unwrap(),
        &ScanProgress {
            files_seen: 3,
            photos: 1,
            videos: 1
        }
    );
}

#[test]
fn a_missing_source_is_reported_not_fatal() {
    let r = scan(
        Path::new("/definitely/not/here/keevoro"),
        &ScanOptions::default(),
        &mut |_| {},
    );
    assert!(r.items.is_empty());
    assert_eq!(r.unreadable.len(), 1);
}
