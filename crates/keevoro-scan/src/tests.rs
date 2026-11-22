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

fn scan_with(t: &Tree, kinds: &keevoro_core::FileKinds) -> ScanReport {
    let opts = ScanOptions {
        kinds: kinds.lookup(),
        ..ScanOptions::default()
    };
    scan(t.root(), &opts, &mut |_| {})
}

#[test]
fn a_kind_a_person_added_is_found_by_its_name_even_when_nothing_inside_says_what_it_is() {
    let t = Tree::new();
    // Nothing Keevoro recognises inside either of them.
    t.add("Holiday/pano.xyz", &[7u8; 300]);
    t.add("Holiday/clip.qqq", b"PK\x03\x04 an archive inside");
    t.add("Holiday/other.abc", &[7u8; 300]);
    let mut kinds = keevoro_core::FileKinds::default();
    kinds.add("XYZ", MediaKind::Photo).unwrap();
    kinds.add("qqq", MediaKind::Video).unwrap();

    assert!(t.scan().items.is_empty(), "not looked for until added");
    let r = scan_with(&t, &kinds);
    assert_eq!(r.items.len(), 2, "{:?}", r.items);
    let pano = item(&r, "pano.xyz");
    assert_eq!((pano.kind, pano.mime.as_str()), (MediaKind::Photo, "application/x-xyz"));
    assert_eq!(
        pano.blake3.as_deref(),
        Some(blake3::hash(&[7u8; 300]).to_hex().as_str())
    );
    assert!(pano.hashed);
    assert_eq!((pano.width, pano.height), (None, None));
    assert_eq!(pano.junk, None, "not set aside for being impossible to open");
    assert_eq!(item(&r, "clip.qqq").kind, MediaKind::Video);
    assert_eq!(r.other_files, 1);
}

#[test]
fn what_is_inside_still_decides_whether_a_kind_it_knows_is_a_photo_or_a_video() {
    let t = Tree::new();
    t.add("really.xyz", &plain_jpeg(1));
    let mut kinds = keevoro_core::FileKinds::default();
    kinds.add("xyz", MediaKind::Video).unwrap();
    let r = scan_with(&t, &kinds);
    assert_eq!(
        (item(&r, "really.xyz").kind, item(&r, "really.xyz").mime.as_str()),
        (MediaKind::Photo, "image/jpeg")
    );
}

#[test]
fn a_kind_switched_off_is_passed_over_without_being_opened() {
    let t = Tree::new();
    t.add("a.jpg", &plain_jpeg(1));
    t.add("b.JPG", &plain_jpeg(2));
    t.add("00001.MTS", &[0x47, 1, 2, 3]);
    t.add("renamed.dat", &plain_jpeg(3));
    let mut kinds = keevoro_core::FileKinds::default();
    let off = keevoro_core::KindChange {
        on: Some(false),
        ..Default::default()
    };
    kinds.change("jpg", &off).unwrap();
    kinds.change("mts", &off).unwrap();
    let r = scan_with(&t, &kinds);
    let names: Vec<_> = r.items.iter().map(|i| i.path.file_name().unwrap().to_owned()).collect();
    assert_eq!(
        names,
        ["renamed.dat"],
        "a photo with an odd name is still found by what is inside"
    );
    assert_eq!(r.other_files, 3);
}

#[test]
fn a_folder_left_out_is_never_listed_and_a_sibling_with_a_like_name_is_searched() {
    let t = Tree::new();
    t.add("Games/IMG_1.jpg", &plain_jpeg(1));
    t.add("Games/deep/IMG_2.jpg", &plain_jpeg(2));
    t.add("Games 2/IMG_3.jpg", &plain_jpeg(3));
    let root = std::fs::canonicalize(t.root()).unwrap();
    #[cfg(unix)]
    {
        // Listing it would fail and be reported: it must not be listed.
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(root.join("Games"), std::fs::Permissions::from_mode(0o000)).unwrap();
    }
    let opts = ScanOptions {
        skip: SkipRules::default().with_left_out(vec![root.join("Games")]),
        ..ScanOptions::default()
    };
    let r = scan(&root, &opts, &mut |_| {});
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(root.join("Games"), std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let names: Vec<_> = r
        .items
        .iter()
        .map(|i| i.path.strip_prefix(&root).unwrap().to_path_buf())
        .collect();
    assert_eq!(names, [PathBuf::from("Games 2").join("IMG_3.jpg")]);
    assert!(r.unreadable.is_empty(), "{:?}", r.unreadable);
    let left: Vec<_> = r
        .skipped
        .iter()
        .filter(|s| s.reason == SkipReason::LeftOut)
        .map(|s| s.path.clone())
        .collect();
    assert_eq!(left, [root.join("Games")]);
    // Searched from inside it, it is left out all the same.
    let inside = scan(&root.join("Games").join("deep"), &opts, &mut |_| {});
    assert!(inside.items.is_empty());
}

#[test]
fn what_keevoro_made_is_never_searched_and_the_report_says_why() {
    let t = Tree::new();
    t.add("Holiday/IMG_1.jpg", &plain_jpeg(1));
    t.add("Keevoro Library/.keevoro/catalog.sqlite", b"x");
    t.add("Keevoro Library/originals/Holiday/IMG_1.jpg", &plain_jpeg(1));
    t.add("Keevoro Library/renditions/display/abc.webp", &plain_jpeg(2));
    t.add("Backup/What is in this backup.txt", b"x");
    t.add("Backup/index/gallery.json", b"{}");
    t.add("Backup/originals/Holiday/IMG_1.jpg", &plain_jpeg(1));
    t.add("Destination/IMG_9.jpg", &plain_jpeg(3));
    // A backup's originals folder chosen as the source itself: the
    // person meant it, so it is looked through.
    let root = std::fs::canonicalize(t.root()).unwrap();
    let opts = ScanOptions {
        skip: SkipRules::default().with_own_places(vec![(root.join("Destination"), SkipReason::KeevoroBackup)]),
        ..ScanOptions::default()
    };
    let r = scan(&root, &opts, &mut |_| {});
    let names: Vec<_> = r
        .items
        .iter()
        .map(|i| i.path.strip_prefix(&root).unwrap().to_path_buf())
        .collect();
    assert_eq!(names, [PathBuf::from("Holiday").join("IMG_1.jpg")]);
    let left: Vec<_> = r
        .skipped
        .iter()
        .filter(|s| s.reason.keevoro_made())
        .map(|s| (s.path.clone(), s.reason))
        .collect();
    assert_eq!(
        left,
        [
            (root.join("Backup"), SkipReason::KeevoroBackup),
            (root.join("Destination"), SkipReason::KeevoroBackup),
            (root.join("Keevoro Library"), SkipReason::OldLibrary),
        ]
    );
    let originals = scan(
        &root.join("Backup").join("originals"),
        &ScanOptions::default(),
        &mut |_| {},
    );
    assert_eq!(
        originals.items.len(),
        1,
        "a backup's originals, chosen on purpose, are looked through"
    );
    let whole = scan(&root.join("Backup"), &ScanOptions::default(), &mut |_| {});
    assert!(
        whole.items.is_empty() && whole.skipped[0].reason == SkipReason::KeevoroBackup,
        "even chosen whole"
    );
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
    scan(t.root(), &ScanOptions::default(), &mut |p| seen.push(p.clone()));
    assert_eq!(seen.len(), 3);
    let last = seen.last().unwrap();
    assert_eq!((last.files_seen, last.photos, last.videos), (3, 1, 1));
    assert!(last.bytes > 0, "the bytes read are counted");
    assert_eq!(last.folder, t.root(), "and it says where it is looking");
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

/// A PNG header of the given size. Enough for content detection and for
/// reading the pixel size; `salt` makes each file's contents different.
fn png(width: u32, height: u32, salt: u8) -> Vec<u8> {
    let mut p = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
    p.extend_from_slice(&width.to_be_bytes());
    p.extend_from_slice(&height.to_be_bytes());
    p.extend_from_slice(&[8, 2, 0, 0, 0, salt, salt, salt, salt]);
    p
}

#[test]
fn videos_are_dated_from_their_own_recording_time() {
    let t = Tree::new();
    // The header sits after the video data, as on files straight from a camera.
    t.add("MVI_0042.MOV", &video::tests::mp4_recorded_at(1_247_605_802, 0, true));
    let r = t.scan();
    let d = item(&r, "MVI_0042.MOV").date.unwrap();
    assert_eq!(d.source, DateSource::Video);
    assert_eq!(d.date.to_string(), "2009-07-14T21:10:02");
}

#[test]
fn a_video_with_no_recording_time_falls_back_to_weaker_clues() {
    let t = Tree::new();
    t.add("VID_20120304_153012.mp4", &mp4());
    let r = t.scan();
    assert_eq!(
        item(&r, "VID_20120304_153012.mp4").date.unwrap().source,
        DateSource::Filename
    );
}

#[test]
fn photo_sizes_are_read_and_junk_is_flagged_but_still_listed() {
    let t = Tree::new();
    t.add("Family/big.png", &png(3264, 2448, 1));
    t.add("Family/tiny.png", &png(120, 90, 2));
    t.add("Some App/icons/save.png", &png(48, 48, 3));
    t.add("Firefox/Cache2/A1B2C3", &png(1920, 1080, 4));
    t.add("Quake/textures/wall.png", &png(512, 512, 5));
    let r = t.scan();
    assert_eq!(r.items.len(), 5, "junk is listed, never hidden");

    let big = item(&r, "big.png");
    assert_eq!((big.width, big.height), (Some(3264), Some(2448)));
    assert_eq!(big.junk, None);
    assert_eq!(item(&r, "tiny.png").junk, Some(JunkReason::TinyImage));
    assert_eq!(item(&r, "save.png").junk, Some(JunkReason::AppIcons));
    assert_eq!(item(&r, "A1B2C3").junk, Some(JunkReason::BrowserCache));
    assert_eq!(item(&r, "wall.png").junk, Some(JunkReason::GameTextures));
}

#[test]
fn a_photo_whose_size_cannot_be_read_is_kept() {
    let t = Tree::new();
    t.add("Family/odd.jpg", &plain_jpeg(9));
    let r = t.scan();
    let odd = item(&r, "odd.jpg");
    assert_eq!((odd.width, odd.junk), (None, None));
}

#[test]
fn apple_photos_library_gives_originals_only() {
    let t = Tree::new();
    let lib = "Pictures/Photos Library.photoslibrary";
    t.add(
        &format!("{lib}/originals/A/A1B2.jpeg"),
        &jpeg_with_exif("2019:06:01 10:00:00"),
    );
    t.add(&format!("{lib}/originals/3/3C4D.jpeg"), &plain_jpeg(2));
    t.add(
        &format!("{lib}/resources/derivatives/A/A1B2_1_105_c.jpeg"),
        &plain_jpeg(3),
    );
    t.add(
        &format!("{lib}/resources/derivatives/masters/A/A1B2_4_5005_c.jpeg"),
        &plain_jpeg(4),
    );
    t.add(
        &format!("{lib}/private/com.apple.photoanalysisd/caches/face.jpg"),
        &plain_jpeg(5),
    );
    t.add(&format!("{lib}/database/Photos.sqlite"), b"SQLite format 3\0");
    let r = t.scan();

    let mut names: Vec<_> = r.items.iter().map(|i| i.path.file_name().unwrap().to_owned()).collect();
    names.sort();
    assert_eq!(names, ["3C4D.jpeg", "A1B2.jpeg"]);
    // The library's own storage folders are not album names.
    assert!(r.items.iter().all(|i| i.folder.album.is_empty()), "{:?}", r.items);
    // Its insides are not reported as skipped folders either.
    assert!(r.skipped.is_empty());
}

#[test]
fn old_iphoto_library_keeps_event_names() {
    let t = Tree::new();
    t.add(
        "iPhoto Library/Originals/2008/Cornwall Trip/IMG_0001.JPG",
        &plain_jpeg(1),
    );
    t.add(
        "iPhoto Library/Masters/2011/03/04/20110304-123456/IMG_0002.JPG",
        &plain_jpeg(2),
    );
    t.add("iPhoto Library/Data/2008/Cornwall Trip/IMG_0001.jpg", &plain_jpeg(3));
    t.add(
        "iPhoto Library/Modified/2008/Cornwall Trip/IMG_0001.JPG",
        &plain_jpeg(4),
    );
    let r = t.scan();
    assert_eq!(r.items.len(), 2);
    assert_eq!(item(&r, "IMG_0001.JPG").folder.album, ["Cornwall Trip"]);
    assert_eq!(item(&r, "IMG_0001.JPG").folder.date.unwrap().to_string(), "2008");
    assert!(item(&r, "IMG_0002.JPG").folder.album.is_empty());
}

#[test]
fn scanning_a_library_directly_works_too() {
    let t = Tree::new();
    t.add("Old.photoslibrary/Masters/2014/05/06/IMG_1.JPG", &plain_jpeg(1));
    t.add("Old.photoslibrary/Thumbnails/2014/05/06/IMG_1.jpg", &plain_jpeg(2));
    let root = t.root().join("Old.photoslibrary");
    let r = scan(&root, &ScanOptions::default(), &mut |_| {});
    assert_eq!(r.items.len(), 1);
    assert!(r.items[0].path.ends_with("Masters/2014/05/06/IMG_1.JPG"));
}

#[test]
fn programs_and_documents_are_counted_without_being_opened() {
    let t = Tree::new();
    // A photo's bytes under a program's name: proof it was never opened.
    t.add("Program Files X/setup.exe", &plain_jpeg(1));
    t.add("Letters/notes.txt", &plain_jpeg(2));
    t.add("Letters/scan.jpg", &plain_jpeg(3));
    let r = t.scan();
    assert_eq!(r.items.len(), 1);
    assert_eq!(r.other_files, 2);
}

#[test]
fn a_big_source_comes_out_in_the_same_order_every_time() {
    let t = Tree::new();
    for folder in 0..12u32 {
        for file in 0..25u32 {
            let shade = u8::try_from((folder * 25 + file) % 251).unwrap();
            t.add(&format!("f{folder:02}/img{file:03}.jpg"), &plain_jpeg(shade));
        }
    }
    let first = t.scan();
    let second = t.scan();
    let paths = |r: &ScanReport| r.items.iter().map(|i| i.path.clone()).collect::<Vec<_>>();
    assert_eq!(first.items.len(), 300);
    assert_eq!(paths(&first), paths(&second));
    let mut sorted = paths(&first);
    sorted.sort();
    assert_eq!(paths(&first), sorted, "in folder and name order");
}

#[test]
fn a_large_file_is_announced_before_it_is_read() {
    let t = Tree::new();
    let mut big = mp4();
    big.resize(usize::try_from(WORTH_ANNOUNCING).unwrap() + 1, 0);
    t.add("holiday.mp4", &big);
    t.add("small.jpg", &plain_jpeg(1));
    let mut announced = Vec::new();
    let mut last = ScanProgress::default();
    scan(t.root(), &ScanOptions::default(), &mut |p| {
        if let Some((path, bytes)) = &p.reading {
            announced.push((path.file_name().unwrap().to_owned(), *bytes));
        }
        last = p.clone();
    });
    assert!(announced
        .iter()
        .any(|(name, bytes)| name == "holiday.mp4" && *bytes > WORTH_ANNOUNCING));
    assert_eq!(last.reading, None, "and it is cleared once read");
    assert_eq!((last.files_seen, last.videos, last.photos), (2, 1, 1));
}

fn quick() -> ScanOptions {
    ScanOptions {
        quick: true,
        ..ScanOptions::default()
    }
}

/// A photo-shaped file of `len` bytes whose every byte depends on `seed`.
fn big_photo(len: usize, seed: u8) -> Vec<u8> {
    let mut bytes = plain_jpeg(seed);
    let mut next = seed;
    bytes.resize_with(len, || {
        next = next.wrapping_mul(31).wrapping_add(7);
        next
    });
    bytes
}

#[test]
fn a_quick_search_does_not_read_large_files_to_the_end() {
    let t = Tree::new();
    t.add("big.jpg", &big_photo(2_000_000, 1));
    t.add("small.jpg", &plain_jpeg(2));
    let r = scan(t.root(), &quick(), &mut |_| {});
    let (big, small) = (item(&r, "big.jpg"), item(&r, "small.jpg"));
    assert!(!big.hashed, "a large file gets a quick fingerprint");
    assert!(small.hashed, "a small one is simply read whole");
    assert_eq!(small.blake3.as_deref().unwrap(), whole_hash(&small.path).unwrap());
    assert_ne!(big.blake3.as_deref().unwrap(), whole_hash(&big.path).unwrap());
    assert_eq!(big.blake3.as_deref().unwrap().len(), 64);
}

#[test]
fn quick_fingerprints_match_for_copies_and_differ_for_different_files() {
    let t = Tree::new();
    let original = big_photo(2_000_000, 1);
    let mut different_end = original.clone();
    *different_end.last_mut().unwrap() ^= 1;
    t.add("a/original.jpg", &original);
    t.add("b/copy.jpg", &original);
    t.add("c/other.jpg", &big_photo(2_000_000, 9));
    t.add("d/longer.jpg", &big_photo(2_000_001, 1));
    t.add("e/different-end.jpg", &different_end);
    let r = scan(t.root(), &quick(), &mut |_| {});
    let print = |name: &str| item(&r, name).blake3.clone().unwrap();
    assert_eq!(print("original.jpg"), print("copy.jpg"));
    assert_ne!(print("original.jpg"), print("other.jpg"));
    assert_ne!(print("original.jpg"), print("longer.jpg"), "size is part of it");
    assert_ne!(print("original.jpg"), print("different-end.jpg"), "so is the end");
}

#[test]
fn a_quick_fingerprint_can_be_fooled_which_is_why_the_whole_hash_decides() {
    let t = Tree::new();
    let original = big_photo(2_000_000, 1);
    let mut changed = original.clone();
    changed[300_000] ^= 1; // Between the samples.
    t.add("a.jpg", &original);
    t.add("b.jpg", &changed);
    let r = scan(t.root(), &quick(), &mut |_| {});
    assert_eq!(item(&r, "a.jpg").blake3, item(&r, "b.jpg").blake3);
    assert_ne!(
        whole_hash(&item(&r, "a.jpg").path).unwrap(),
        whole_hash(&item(&r, "b.jpg").path).unwrap()
    );
}

#[test]
fn a_search_can_be_stopped_part_way() {
    let t = Tree::new();
    for folder in 0..40u32 {
        for file in 0..25u32 {
            let shade = u8::try_from((folder * 25 + file) % 251).unwrap();
            t.add(&format!("f{folder:02}/img{file:03}.jpg"), &plain_jpeg(shade));
        }
    }
    let stop = Arc::new(AtomicBool::new(false));
    let opts = ScanOptions {
        stop: Some(stop.clone()),
        ..ScanOptions::default()
    };
    let r = scan(t.root(), &opts, &mut |p| {
        if p.files_seen == 20 {
            stop.store(true, Ordering::SeqCst);
        }
    });
    assert!(r.stopped);
    // The readers work on threads of their own and are only held between
    // one file and the next, so on a quick computer they can have read
    // every one of these small files by the time the stop is seen. What
    // is certain is that it stopped having found at least what it had
    // reported.
    assert!(r.items.len() >= 20 && r.items.len() <= 1000, "found {}", r.items.len());

    // Asked to stop before it begins, it finds nothing and says it stopped:
    // this half does not depend on how quick the computer is.
    let stopped_first = Arc::new(AtomicBool::new(true));
    let none = scan(
        t.root(),
        &ScanOptions {
            stop: Some(stopped_first),
            ..ScanOptions::default()
        },
        &mut |_| {},
    );
    assert!(none.stopped);
    assert!(none.items.len() < 1000, "found {}", none.items.len());

    // Left alone, the same search finishes and says so.
    let whole = t.scan();
    assert!(!whole.stopped);
    assert_eq!(whole.items.len(), 1000);
}

#[test]
fn a_raw_file_is_never_judged_by_the_small_preview_in_its_header() {
    let t = Tree::new();
    // What a raw file looks like from the outside: a header describing a
    // small preview. The real photo inside is far bigger.
    t.add("DCIM/DJI_0001.DNG", &png(160, 120, 1));
    t.add("DCIM/DJI_0002.dng", &png(256, 256, 2));
    t.add("DCIM/thumb.png", &png(160, 120, 3));
    let r = t.scan();
    for name in ["DJI_0001.DNG", "DJI_0002.dng"] {
        let raw = item(&r, name);
        assert_eq!(raw.junk, None, "{name} is a photo, not junk");
        assert_eq!(
            (raw.width, raw.height),
            (None, None),
            "{name}: the preview's size is not taken as its own"
        );
    }
    assert_eq!(
        item(&r, "thumb.png").junk,
        Some(JunkReason::TinyImage),
        "an ordinary tiny picture still is"
    );
}

mod how_deep {
    use super::*;

    /// Says a file is only online when its name says so, on any computer,
    /// the way Windows' attributes or a Mac's flags say it for real.
    #[derive(Debug)]
    pub(crate) struct NamedOnline;

    impl crate::OnlineCheck for NamedOnline {
        fn online_only(&self, file: &Path, _: &std::fs::Metadata) -> bool {
            file.file_name()
                .is_some_and(|n| n.to_string_lossy().contains("(online)"))
        }
    }

    #[test]
    fn files_only_in_online_storage_are_counted_and_never_opened() {
        let t = Tree::new();
        t.add("OneDrive/Pictures/here.jpg", &plain_jpeg(1));
        let away = t.add("OneDrive/Pictures/away (online).jpg", &plain_jpeg(2));
        t.add("OneDrive/Pictures/clip (online).mp4", &mp4());
        // Not a photo by its name, so not counted as a photo left out.
        t.add("OneDrive/Documents/report (online).docx", b"words");
        #[cfg(unix)]
        {
            // Opening it would fail, and say so: it must not be opened.
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&away, fs::Permissions::from_mode(0o000)).unwrap();
        }
        let opts = ScanOptions {
            online: std::sync::Arc::new(NamedOnline),
            ..ScanOptions::default()
        };
        let r = scan(t.root(), &opts, &mut |_| {});
        assert_eq!(r.items.len(), 1, "only the photo on this computer is found");
        let names: Vec<String> = r
            .online_only
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["away (online).jpg", "clip (online).mp4"]);
        // Their sizes come from the listing, as the placeholder gives them,
        // never from opening them.
        assert_eq!(r.online_only_bytes, [plain_jpeg(2).len() as u64, mp4().len() as u64]);
        assert!(r.unreadable.is_empty(), "nothing online was opened: {:?}", r.unreadable);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&away, fs::Permissions::from_mode(0o644)).unwrap();
        }
    }

    #[test]
    fn a_survey_counts_what_is_only_online_beside_each_suggestion() {
        let t = Tree::new();
        for n in 0..6 {
            t.add(&format!("OneDrive/Pictures/IMG_{n}.jpg"), &[0u8; 100]);
        }
        for n in 0..4 {
            t.add(&format!("OneDrive/Pictures/IMG_{n} (online).jpg"), &[0u8; 100]);
        }
        t.add("Work/report (online).docx", &[0u8; 10]);
        for n in 0..10 {
            t.add(&format!("Work/{n}.docx"), &[0u8; 10]);
        }
        let found = survey(
            t.root(),
            &SkipRules::default(),
            &Kinds::default(),
            &NamedOnline,
            crate::map::Limits::default(),
            None,
            &mut |_| {},
        );
        assert_eq!(found.hints.len(), 1);
        assert_eq!((found.hints[0].photos, found.hints[0].online_only), (10, 4));
        assert_eq!(found.online_only, 4, "a document only online is not a photo left out");
        // On the map, the room every file only online takes is said as such,
        // photo or not, from its listing alone.
        let totals = found.map.totals();
        assert_eq!(totals[0].online_only_bytes, 4 * 100 + 10);
        let work = found.map.find(&t.root().join("Work")).unwrap();
        assert_eq!(totals[work as usize].online_only_bytes, 10);
        assert_eq!(totals[0].bytes(), 10 * 100 + 11 * 10, "and still counted in full");
    }
    use crate::survey::survey;
    use keevoro_core::Kinds;

    fn survey_of(root: &Path) -> crate::survey::Survey {
        survey(
            root,
            &SkipRules::default(),
            &Kinds::default(),
            &crate::ByAttributes,
            crate::map::Limits::default(),
            None,
            &mut |_| {},
        )
    }

    fn photos_found_by_survey(root: &Path) -> usize {
        survey_of(root).hints.iter().map(|h| h.photos).sum()
    }

    #[test]
    fn a_photo_forty_five_folders_deep_is_found_by_the_search_and_the_survey() {
        let t = Tree::new();
        let deep: Vec<String> = (0..45).map(|n| format!("d{n}")).collect();
        let rel = format!("{}/IMG_0001.jpg", deep.join("/"));
        t.add(&rel, &plain_jpeg(1));
        let r = t.scan();
        assert_eq!(r.items.len(), 1);
        assert_eq!(r.items[0].path.components().count(), t.root().components().count() + 46);
        assert_eq!(photos_found_by_survey(t.root()), 1);
    }

    #[test]
    fn long_names_and_names_in_any_language_do_not_stop_the_walk() {
        let t = Tree::new();
        let long = "a".repeat(200);
        for rel in [
            format!("{long}/{long}.jpg"),
            "Fotos de la familia/Año nuevo/IMG_ñ.jpg".to_string(),
            "家族の写真/旅行/写真.jpg".to_string(),
            "Ελλάδα 2009/παραλία.jpg".to_string(),
            "Emoji 🎉/party 🥳.jpg".to_string(),
        ] {
            t.add(&rel, &plain_jpeg(2));
        }
        assert_eq!(t.scan().items.len(), 5);
        assert_eq!(photos_found_by_survey(t.root()), 5);
    }

    /// Windows will not make these, but a drive from another computer may
    /// hold them.
    #[cfg(unix)]
    #[test]
    fn names_ending_in_dots_or_spaces_do_not_stop_the_walk() {
        let t = Tree::new();
        t.add("Holiday. /IMG_1.jpg.", &plain_jpeg(3));
        t.add("Holiday /IMG_2 .jpg", &plain_jpeg(4));
        t.add("Holiday./IMG_3.jpg", &plain_jpeg(5));
        assert_eq!(t.scan().items.len(), 3);
    }

    #[cfg(unix)]
    #[test]
    fn a_link_back_up_the_tree_is_not_followed_so_nothing_loops_or_counts_twice() {
        let t = Tree::new();
        t.add("Photos/IMG_1.jpg", &plain_jpeg(1));
        std::os::unix::fs::symlink(t.root(), t.root().join("Photos/back to the top")).unwrap();
        std::os::unix::fs::symlink(t.root().join("Photos/IMG_1.jpg"), t.root().join("Photos/shortcut.jpg")).unwrap();
        let r = t.scan();
        assert_eq!(r.items.len(), 1, "found once, where it really is");
        assert_eq!(photos_found_by_survey(t.root()), 1);
    }

    #[cfg(unix)]
    #[test]
    fn a_folder_that_cannot_be_opened_is_reported_and_the_walk_carries_on() {
        use std::os::unix::fs::PermissionsExt;
        let t = Tree::new();
        t.add("A/IMG_1.jpg", &plain_jpeg(1));
        t.add("Locked/IMG_2.jpg", &plain_jpeg(2));
        t.add("Z/IMG_3.jpg", &plain_jpeg(3));
        let locked = t.root().join("Locked");
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
        // Someone allowed everything (the root account) can open it anyway.
        let can_open = fs::read_dir(&locked).is_ok();
        let r = t.scan();
        let found = survey_of(t.root());
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
        if can_open {
            return;
        }
        assert_eq!(r.items.len(), 2, "the folders either side are still looked through");
        assert!(
            r.unreadable.iter().any(|u| u.path.ends_with("Locked")),
            "{:?}",
            r.unreadable
        );
        assert_eq!(found.unreadable, 1);
        assert_eq!(found.hints.iter().map(|h| h.photos).sum::<usize>(), 2);
    }
}

#[test]
fn a_file_is_fingerprinted_on_its_own_exactly_as_a_search_does() {
    let t = Tree::new();
    let small = t.add("small.jpg", &plain_jpeg(3));
    let mut big = plain_jpeg(4);
    big.resize(600 * 1024, 7);
    let large = t.add("large.jpg", &big);
    for quick in [true, false] {
        let opts = ScanOptions {
            quick,
            ..ScanOptions::default()
        };
        let r = scan(t.root(), &opts, &mut |_| {});
        for path in [&small, &large] {
            let found = r.items.iter().find(|i| &i.path == path).unwrap();
            assert_eq!(
                found.blake3.as_deref(),
                Some(fingerprint(path, quick).unwrap().as_str()),
                "{path:?}, quick {quick}"
            );
        }
    }
}

/// Other files a person asked Keevoro to carry along.
mod other_files {
    use super::how_deep::NamedOnline;
    use super::*;
    use crate::survey::survey;

    fn with_others(endings: &[&str]) -> keevoro_core::FileKinds {
        let mut kinds = keevoro_core::FileKinds::default();
        for ending in endings {
            kinds.add(ending, MediaKind::Other).unwrap();
        }
        kinds
    }

    #[test]
    fn an_other_file_is_found_by_its_ending_fingerprinted_whole_and_kept_in_its_folder() {
        let t = Tree::new();
        // A JPEG inside, but its ending is on the list as an other file:
        // the ending decides, and nothing inside is looked at.
        t.add("Documents/Tax 2019/scan.dat", &plain_jpeg(3));
        t.add("Documents/Tax 2019/return.pdf", b"%PDF-1.4 a tax return");
        t.add("Music/song.flac", &[0u8; 1]);
        t.add("Documents/notes.txt", b"not on the list");
        t.add("photo.jpg", &plain_jpeg(1));

        let before = t.scan();
        let kinds: Vec<MediaKind> = before.items.iter().map(|i| i.kind).collect();
        assert_eq!(
            kinds,
            [MediaKind::Photo, MediaKind::Photo],
            "nothing is carried until a person adds a kind"
        );
        let r = scan_with(&t, &with_others(&["pdf", "flac", "dat"]));
        assert_eq!(r.items.len(), 4, "{:?}", r.items);
        let root_name = t.root().file_name().unwrap().to_string_lossy().to_string();
        for (name, folders) in [
            ("scan.dat", vec!["Documents", "Tax 2019"]),
            ("return.pdf", vec!["Documents", "Tax 2019"]),
            ("song.flac", vec!["Music"]),
        ] {
            let found = item(&r, name);
            assert_eq!(found.kind, MediaKind::Other, "{name}");
            assert_eq!(found.mime, "application/octet-stream", "never sniffed");
            assert!(found.hashed);
            assert_eq!(
                found.blake3.as_deref(),
                Some(whole_hash(&found.path).unwrap().as_str()),
                "fingerprinted from every byte"
            );
            let mut album = vec![root_name.clone()];
            album.extend(folders.iter().map(|f| (*f).to_string()));
            assert_eq!(found.folder.album, album, "{name}: the folders as they are");
            assert_eq!(found.junk, None, "a one-byte song is never junk");
            assert_eq!((found.width, found.height), (None, None));
            assert_eq!(found.date.map(|d| d.source), Some(DateSource::Modified));
        }
        assert_eq!(item(&r, "photo.jpg").kind, MediaKind::Photo, "photos as ever");
    }

    #[test]
    fn a_big_other_file_is_sampled_by_a_quick_search_like_any_other() {
        let t = Tree::new();
        t.add("big.iso", &vec![9u8; 2 * 1024 * 1024]);
        let opts = ScanOptions {
            kinds: with_others(&["iso"]).lookup(),
            quick: true,
            ..ScanOptions::default()
        };
        let r = scan(t.root(), &opts, &mut |_| {});
        assert!(!item(&r, "big.iso").hashed, "to be read in full before it counts");
    }

    #[test]
    fn an_other_file_only_in_online_storage_is_counted_and_never_opened() {
        let t = Tree::new();
        let away = t.add("OneDrive/Documents/report (online).pdf", b"words");
        t.add("OneDrive/Documents/here.pdf", b"words here");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&away, fs::Permissions::from_mode(0o000)).unwrap();
        }
        let opts = ScanOptions {
            kinds: with_others(&["pdf"]).lookup(),
            online: std::sync::Arc::new(NamedOnline),
            ..ScanOptions::default()
        };
        let r = scan(t.root(), &opts, &mut |_| {});
        assert_eq!(r.items.len(), 1);
        assert_eq!(r.online_only, std::slice::from_ref(&away));
        assert!(r.unreadable.is_empty(), "nothing online was opened: {:?}", r.unreadable);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&away, fs::Permissions::from_mode(0o644)).unwrap();
        }
    }

    #[test]
    fn a_survey_suggests_a_folder_of_other_files_once_they_are_wanted_and_only_then() {
        let t = Tree::new();
        for n in 0..40 {
            t.add(&format!("Documents/letter {n}.pdf"), &[0u8; 100]);
        }
        for n in 0..10 {
            t.add(&format!("Pictures/IMG_{n}.jpg"), &[0u8; 100_000]);
        }
        for n in 0..200 {
            t.add(&format!("Tools/part {n}.dll"), &[0u8; 10]);
        }
        let run = |kinds: &Kinds| {
            survey(
                t.root(),
                &SkipRules::default(),
                kinds,
                &NamedOnline,
                crate::map::Limits::default(),
                None,
                &mut |_| {},
            )
        };
        let plain = run(&Kinds::default());
        let folders: Vec<_> = plain.hints.iter().map(|h| h.folder.clone()).collect();
        assert_eq!(folders, [t.root().join("Pictures")]);
        assert_eq!(plain.hints[0].others, 0);

        let wanted = run(&with_others(&["pdf"]).lookup());
        let documents = wanted
            .hints
            .iter()
            .find(|h| h.folder == t.root().join("Documents"))
            .expect("a folder of documents is suggested");
        assert_eq!((documents.photos, documents.videos, documents.others), (0, 0, 40));
        assert_eq!(documents.bytes, 4_000);
        // The map counts them as a kind of their own, not as everything else.
        let totals = wanted.map.totals();
        assert_eq!(totals[0].kinds[crate::map::Kind::Carried.index()].files, 40);
        assert_eq!(totals[0].kinds[crate::map::Kind::Other.index()].files, 200, "the tools");
        // Not wanted by a backup of photos only.
        let photos_only = with_others(&["pdf"])
            .lookup()
            .wanted_by(keevoro_core::Wants::only(MediaKind::Photo));
        let photos = run(&photos_only);
        assert!(photos.hints.iter().all(|h| h.folder != t.root().join("Documents")));
    }
}

/// Pulling the card out is made by moving its folder away while it is
/// walked. Windows refuses to move a folder the walk has open, so there it
/// is proved on a real computer; the rule is the same code everywhere.
#[cfg(unix)]
#[test]
fn a_drive_unplugged_part_way_stops_the_search_and_is_not_taken_for_unreadable_files() {
    let dir = tempfile::tempdir().unwrap();
    let card = dir.path().join("Card");
    for folder in 0..40 {
        let at = card.join(format!("Folder {folder:02}"));
        std::fs::create_dir_all(&at).unwrap();
        for file in 0..50 {
            std::fs::write(
                at.join(format!("IMG_{file:04}.jpg")),
                plain_jpeg(u8::try_from(file).unwrap()),
            )
            .unwrap();
        }
    }
    let elsewhere = dir.path().join("Unplugged");
    let mut pulled = false;
    let report = scan(&card, &ScanOptions::default(), &mut |_| {
        // The card is pulled out as the first file is said.
        if !pulled {
            std::fs::rename(&card, &elsewhere).unwrap();
            pulled = true;
        }
    });
    assert!(report.gone, "the search knows the card went");
    assert!(!report.stopped);
    assert!(
        report.unreadable.is_empty(),
        "{:?}",
        &report.unreadable[..report.unreadable.len().min(3)]
    );
    // How far it had got when the card went depends on how quick the computer
    // is (a fast one has every name listed before the first file is said), so
    // the count is not judged: knowing the card went, and blaming no file, is.
    // Nothing of the card was changed: it is all there once plugged back.
    std::fs::rename(&elsewhere, &card).unwrap();
    let whole = scan(&card, &ScanOptions::default(), &mut |_| {});
    assert!(!whole.gone);
    assert_eq!(whole.items.len(), 2000);
}
