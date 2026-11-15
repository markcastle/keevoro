use super::*;
use keevoro_scan::{scan, ScanOptions, ScanReport};
use std::collections::BTreeMap;
use tempfile::TempDir;

fn jpeg(fill: u8, len: usize) -> Vec<u8> {
    let mut j = vec![
        0xFF, 0xD8, 0xFF, 0xE0, 0, 16, b'J', b'F', b'I', b'F', 0, 1, 1, 0, 0, 1, 0, 1, 0, 0,
    ];
    j.extend(std::iter::repeat_n(fill, len));
    j
}

struct World {
    dir: TempDir,
    catalog: Catalog,
}

impl World {
    fn new() -> Self {
        Self {
            dir: tempfile::tempdir().unwrap(),
            catalog: Catalog::in_memory().unwrap(),
        }
    }
    fn drive(&self) -> PathBuf {
        self.dir.path().join("old drive")
    }
    fn library(&self) -> PathBuf {
        self.dir.path().join("Keevoro Library")
    }
    fn add(&self, rel: &str, bytes: &[u8]) {
        let p = self.drive().join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, bytes).unwrap();
    }
    fn scan(&self) -> ScanReport {
        scan(&self.drive(), &ScanOptions::default(), &mut |_| {})
    }
    fn gather(&self, report: &ScanReport) -> GatherReport {
        let items: Vec<&FoundItem> = report.items.iter().collect();
        gather(&self.library(), &self.catalog, &items, None, &mut |_| {}).unwrap()
    }
}

/// Every file under `root`, relative path to contents.
fn snapshot(root: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(dir: &Path, root: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
        let Ok(entries) = fs::read_dir(dir) else { return };
        for e in entries {
            let p = e.unwrap().path();
            if p.is_dir() {
                walk(&p, root, out);
            } else {
                let rel = p.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/");
                out.insert(rel, fs::read(&p).unwrap());
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(root, root, &mut out);
    out
}

#[test]
fn files_land_in_the_users_own_folders_and_match_exactly() {
    let w = World::new();
    w.add("My Pictures/Holidays/Spain 2009/IMG_1234.jpg", &jpeg(1, 5000));
    w.add("DCIM/100CANON/IMG_20120304_153012.jpg", &jpeg(2, 3000));
    w.add("DCIM/100CANON/scan.jpg", &jpeg(3, 10));
    let report = w.scan();
    let before = snapshot(&w.drive());

    let g = w.gather(&report);
    assert_eq!((g.copied, g.already_there, g.failed.len()), (3, 0, 0));
    assert_eq!(g.copied_bytes, report.items.iter().map(|i| i.bytes).sum::<u64>());

    let lib = snapshot(&w.library());
    assert_eq!(
        lib.keys().collect::<Vec<_>>(),
        [
            "originals/By date/2012/2012-03/IMG_20120304_153012.jpg",
            "originals/Holidays/Spain 2009/IMG_1234.jpg",
            "originals/Undated/scan.jpg",
        ]
    );
    assert_eq!(lib["originals/Holidays/Spain 2009/IMG_1234.jpg"], jpeg(1, 5000));
    assert_eq!(snapshot(&w.drive()), before, "the old drive is untouched");
    assert_eq!(w.catalog.collected_totals().unwrap().0, 3);
}

#[test]
fn running_again_copies_nothing_new() {
    let w = World::new();
    w.add("Family/a.jpg", &jpeg(1, 100));
    w.add("Family/b.jpg", &jpeg(2, 100));
    let report = w.scan();
    w.gather(&report);
    let first = snapshot(&w.library());

    let again = w.gather(&report);
    assert_eq!((again.copied, again.already_there), (0, 2));
    assert_eq!(snapshot(&w.library()), first);
}

#[test]
fn a_new_file_on_a_later_run_is_the_only_thing_copied() {
    let w = World::new();
    w.add("Family/a.jpg", &jpeg(1, 100));
    w.gather(&w.scan());
    w.add("Family/b.jpg", &jpeg(2, 100));
    let g = w.gather(&w.scan());
    assert_eq!((g.copied, g.already_there), (1, 1));
}

#[test]
fn a_lost_catalog_does_not_cause_duplicates_in_the_library() {
    let mut w = World::new();
    w.add("Family/a.jpg", &jpeg(1, 100));
    let report = w.scan();
    w.gather(&report);
    w.catalog = Catalog::in_memory().unwrap();

    let g = w.gather(&report);
    assert_eq!((g.copied, g.already_there), (0, 1));
    assert_eq!(snapshot(&w.library()).len(), 1);
    assert_eq!(
        w.catalog.collected_totals().unwrap().0,
        1,
        "and the catalog is repaired"
    );
}

#[test]
fn a_file_deleted_from_the_library_is_gathered_again() {
    let w = World::new();
    w.add("Family/a.jpg", &jpeg(1, 100));
    let report = w.scan();
    w.gather(&report);
    fs::remove_file(w.library().join("originals/Family/a.jpg")).unwrap();
    assert_eq!(w.gather(&report).copied, 1);
}

#[test]
fn different_files_with_the_same_name_are_both_kept() {
    let w = World::new();
    // Two cameras, both counting from IMG_0001, merged into one album.
    w.add("Christmas/IMG_0001.jpg", &jpeg(1, 100));
    w.add("Backup of C/Christmas/IMG_0001.jpg", &jpeg(2, 100));
    w.add("Copy of Christmas/IMG_0001.jpg", &jpeg(3, 100));
    let g = w.gather(&w.scan());
    assert_eq!(g.copied, 3);
    let lib = snapshot(&w.library());
    assert_eq!(
        lib.keys().collect::<Vec<_>>(),
        [
            "originals/Christmas/IMG_0001 (2).jpg",
            "originals/Christmas/IMG_0001 (3).jpg",
            "originals/Christmas/IMG_0001.jpg",
        ]
    );
    let mut fills: Vec<u8> = lib.values().map(|b| b[20]).collect();
    fills.sort_unstable();
    assert_eq!(fills, [1, 2, 3], "nothing was overwritten");
}

#[test]
fn a_file_changed_since_the_scan_is_reported_not_copied() {
    let w = World::new();
    w.add("Family/a.jpg", &jpeg(1, 100));
    w.add("Family/b.jpg", &jpeg(2, 100));
    let report = w.scan();
    w.add("Family/a.jpg", &jpeg(9, 100));

    let g = w.gather(&report);
    assert_eq!(
        (g.copied, g.failed.len()),
        (1, 1),
        "one bad file does not stop the rest"
    );
    assert!(g.failed[0].path.ends_with("a.jpg"));
    assert!(g.failed[0].reason.contains("changed since the drive was scanned"));
    assert_eq!(
        snapshot(&w.library()).keys().collect::<Vec<_>>(),
        ["originals/Family/b.jpg"],
        "no partial or temporary files are left"
    );
}

#[test]
fn an_unplugged_drive_is_reported_file_by_file() {
    let w = World::new();
    w.add("Family/a.jpg", &jpeg(1, 100));
    let report = w.scan();
    fs::remove_dir_all(w.drive()).unwrap();
    let g = w.gather(&report);
    assert_eq!((g.copied, g.failed.len()), (0, 1));
    assert!(g.failed[0].reason.starts_with("Couldn't read it from the old drive"));
    assert_eq!(w.catalog.collected_totals().unwrap().0, 0);
}

#[test]
fn files_scanned_without_hashes_cannot_be_gathered() {
    let w = World::new();
    w.add("Family/a.jpg", &jpeg(1, 100));
    let report = scan(
        &w.drive(),
        &ScanOptions {
            hash: false,
            ..Default::default()
        },
        &mut |_| {},
    );
    let g = w.gather(&report);
    assert_eq!(g.copied, 0);
    assert!(g.failed[0].reason.contains("without --no-hash"));
}

#[test]
fn refuses_to_start_when_the_library_disk_is_too_small() {
    let w = World::new();
    w.add("Family/a.jpg", &jpeg(1, 1000));
    w.add("Family/b.jpg", &jpeg(2, 1000));
    let report = w.scan();
    let items: Vec<&FoundItem> = report.items.iter().collect();
    let err = gather(&w.library(), &w.catalog, &items, Some(1500), &mut |_| {}).unwrap_err();
    assert!(matches!(
        err,
        GatherError::NotEnoughRoom {
            needed: 2040,
            free: 1500
        }
    ));
    assert!(!w.library().exists(), "nothing is written when it will not all fit");

    // Files already gathered do not count towards the space needed.
    gather(&w.library(), &w.catalog, &items[..1], Some(1500), &mut |_| {}).unwrap();
    gather(&w.library(), &w.catalog, &items, Some(1500), &mut |_| {}).unwrap();
}

#[test]
fn progress_counts_up_to_the_total() {
    let w = World::new();
    w.add("Family/a.jpg", &jpeg(1, 100));
    w.add("Family/b.jpg", &jpeg(2, 200));
    let report = w.scan();
    let items: Vec<&FoundItem> = report.items.iter().collect();
    gather(&w.library(), &w.catalog, &items[..1], None, &mut |_| {}).unwrap();

    let mut seen = Vec::new();
    gather(&w.library(), &w.catalog, &items, None, &mut |p| seen.push(*p)).unwrap();
    assert_eq!(
        seen.first().map(|p| (p.done, p.total)),
        Some((1, 2)),
        "starts from what is already there"
    );
    let last = seen.last().unwrap();
    assert_eq!((last.done, last.total), (2, 2));
    assert_eq!(last.bytes_done, last.bytes_total);
}

#[test]
fn the_copy_keeps_the_originals_modified_time() {
    let w = World::new();
    w.add("Family/a.jpg", &jpeg(1, 100));
    let old = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_247_605_802);
    File::options()
        .write(true)
        .open(w.drive().join("Family/a.jpg"))
        .unwrap()
        .set_modified(old)
        .unwrap();
    w.gather(&w.scan());
    let copied = fs::metadata(w.library().join("originals/Family/a.jpg")).unwrap();
    assert_eq!(copied.modified().unwrap(), old);
}

#[test]
fn copy_verified_handles_leftovers_and_bad_destinations() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("a.jpg");
    fs::write(&source, jpeg(1, 100)).unwrap();
    let hash = hash_file(&source).unwrap();
    let dest = dir.path().join("lib");

    // A half-written temporary file from a run that was cut short.
    fs::create_dir_all(&dest).unwrap();
    fs::write(dest.join(format!(".keevoro-part-{}", &hash[..16])), b"half").unwrap();
    let placed = copy_verified(&source, &dest, "a.jpg", &hash).unwrap();
    assert_eq!((placed.path.clone(), placed.already_there), (dest.join("a.jpg"), false));
    assert_eq!(fs::read(&placed.path).unwrap(), jpeg(1, 100));
    assert_eq!(fs::read_dir(&dest).unwrap().count(), 1);

    // The same file again under the same name: nothing to do.
    assert!(copy_verified(&source, &dest, "a.jpg", &hash).unwrap().already_there);

    // The destination "folder" is actually a file.
    let err = copy_verified(&source, &source, "a.jpg", &hash).unwrap_err();
    assert!(err.to_string().starts_with("Couldn't write it to the library"));
}

#[test]
fn free_space_is_reported_for_a_library_that_does_not_exist_yet() {
    let dir = tempfile::tempdir().unwrap();
    assert!(free_space(&dir.path().join("not/made/yet")).is_some_and(|b| b > 0));
}

#[test]
fn errors_read_plainly() {
    let e = GatherError::NotEnoughRoom { needed: 10, free: 5 };
    assert!(e.to_string().starts_with("There isn't enough room for your library."));
    assert!(CopyError::VerifyFailed
        .to_string()
        .contains("didn't match the original"));
    assert!(CopyError::NameTaken.to_string().contains("share this name"));
}
