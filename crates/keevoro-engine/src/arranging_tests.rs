//! Keeping a source's folders as they are, end to end: from a search to
//! what is at a destination, and what happens when the choice changes
//! after files were backed up.

use crate::connect::{self, testing::MemorySecrets, Flags, Saved, World};
use crate::discover::tests::real_jpeg;
use crate::send::{everywhere, Stop};
use keevoro_catalog::Catalog;
use keevoro_core::Arrange;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Every file under `dir`, by its path from there, with its bytes.
fn everything_in(dir: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn walk(top: &Path, at: &Path, all: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in std::fs::read_dir(at).into_iter().flatten().filter_map(Result::ok) {
            let path = entry.path();
            if path.is_dir() {
                walk(top, &path, all);
            } else {
                all.insert(
                    path.strip_prefix(top).unwrap().to_path_buf(),
                    std::fs::read(&path).unwrap(),
                );
            }
        }
    }
    let mut all = BTreeMap::new();
    walk(dir, dir, &mut all);
    all
}

/// The tester's source: `Pictures/Scans/Family Scans`, with scans loose
/// in it and in a folder of their own. Scans have no date of their own.
struct Scans {
    dir: tempfile::TempDir,
    source: PathBuf,
    home: PathBuf,
    to: PathBuf,
    catalog: Catalog,
    secrets: MemorySecrets,
}

impl Scans {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("Pictures").join("Scans").join("Family Scans");
        let home = dir.path().join("home");
        let to = dir.path().join("usb");
        let catalog = Catalog::open_in_library(&home).unwrap();
        let scans = Self {
            dir,
            source,
            home,
            to,
            catalog,
            secrets: MemorySecrets::default(),
        };
        scans.add("scan0001.jpg", 10);
        scans.add("Smith/scan0002.jpg", 40);
        scans.add("Smith/1950s/scan0003.jpg", 70);
        scans
    }

    fn add(&self, rel: &str, shade: u8) {
        let p = self.source.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, real_jpeg(shade)).unwrap();
    }

    fn look(&mut self) {
        crate::scan_into(&mut self.catalog, &self.source, Some("Family Scans"), &mut |_| {}).unwrap();
    }

    fn world(&self) -> World<'_> {
        World {
            secrets: &self.secrets,
            environment: &connect::testing::no_environment,
        }
    }

    fn add_the_usb_stick(&self) {
        let folder = Saved::from_flags(&Flags {
            to_folder: Some(self.to.clone()),
            ..Flags::default()
        })
        .unwrap();
        connect::add(&mut Vec::new(), &self.catalog, &folder, None, &self.world()).unwrap();
    }

    /// A whole backup, looking at the destination first, as the app's does.
    fn back_up(&self) -> String {
        let mut said = Vec::new();
        let went = everywhere(
            &mut said,
            &self.catalog,
            &self.home,
            &self.world(),
            None,
            &Stop::default(),
            &keevoro_core::estimate::Tracker::new(keevoro_core::estimate::Remembered::default()),
            &mut |_| {},
            &mut |_| {},
        )
        .unwrap();
        assert_eq!(went.reached, 1);
        String::from_utf8(said).unwrap()
    }

    fn places(&self) -> Vec<String> {
        self.catalog.collected().unwrap().into_iter().map(|c| c.path).collect()
    }
}

#[test]
fn a_folder_of_scans_kept_as_it_is_lands_in_its_folders() {
    let mut s = Scans::new();
    s.look();
    s.catalog.set_arrange(Arrange::Folders).unwrap();
    s.add_the_usb_stick();
    let source_before = everything_in(&s.source);
    let said = s.back_up();
    assert!(said.contains("Safely backed up 3 files"), "{said}");
    assert!(!said.contains("Arranged by date"), "it was asked: {said}");

    let originals = s.to.join("originals");
    for (rel, at) in [
        ("scan0001.jpg", originals.join("Family Scans").join("scan0001.jpg")),
        (
            "Smith/scan0002.jpg",
            originals.join("Family Scans").join("Smith").join("scan0002.jpg"),
        ),
        (
            "Smith/1950s/scan0003.jpg",
            originals
                .join("Family Scans")
                .join("Smith")
                .join("1950s")
                .join("scan0003.jpg"),
        ),
    ] {
        assert_eq!(
            std::fs::read(&at).unwrap_or_else(|e| panic!("{}: {e}", at.display())),
            std::fs::read(s.source.join(rel)).unwrap(),
            "{rel} is an exact copy in its own folder"
        );
    }
    assert!(!originals.join("By date").exists() && !originals.join("Undated").exists());

    let guide = std::fs::read_to_string(s.to.join(crate::layout::GUIDE_NAME)).unwrap();
    let guide = guide.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(
        guide.contains("This backup keeps your folders as they were."),
        "{guide}"
    );
    assert!(guide.contains("starting with \"Family Scans\" itself"), "{guide}");

    // The index's albums are the person's folders.
    let index = std::fs::read_dir(s.to.join("index"))
        .unwrap()
        .filter_map(Result::ok)
        .find_map(|e| serde_json::from_slice::<serde_json::Value>(&std::fs::read(e.path()).ok()?).ok())
        .unwrap();
    let albums = index["albums"].to_string();
    assert!(albums.contains("Family Scans") && albums.contains("1950s"), "{albums}");

    // And the source is exactly as it was.
    assert_eq!(everything_in(&s.source), source_before);
    let _ = &s.dir;
}

#[test]
fn changing_how_a_backup_is_arranged_moves_nothing_and_sends_nothing_twice() {
    let mut s = Scans::new();
    s.look();
    s.catalog.set_arrange(Arrange::ByDate).unwrap();
    s.add_the_usb_stick();
    let said = s.back_up();
    assert!(said.contains("Safely backed up 3 files"), "{said}");
    let places_by_date = s.places();
    assert!(
        places_by_date.iter().all(|p| !p.starts_with("originals/Family Scans/")),
        "{places_by_date:?}"
    );
    let at_the_destination = everything_in(&s.to);

    // A scan found after that backup is given its place by date, but the
    // backup is stopped before it is sent anywhere.
    s.add("Jones/scan0004.jpg", 100);
    s.look();
    crate::check_and_place(&s.home, &s.catalog, &|| true, &mut |_| {}).unwrap();
    assert_eq!(s.places().len(), 4);

    // Changed to keep the folders, with one more scan found since.
    s.catalog.set_arrange(Arrange::Folders).unwrap();
    s.add("Jones/scan0005.jpg", 130);
    s.look();
    let source_before = everything_in(&s.source);
    let said = s.back_up();

    // Only the two not yet backed up were sent; the three already there
    // were found there, not counted missing and not sent again.
    assert!(said.contains("Safely backed up 2 files"), "{said}");
    assert!(said.contains("3 files already there"), "{said}");
    assert!(
        !said.contains("no longer holds") && !said.contains("Restored"),
        "{said}"
    );
    let now = s.places();
    for place in &places_by_date {
        assert!(now.contains(place), "{place} kept its place: {now:?}");
    }
    // Both new ones, the one placed but never sent included, follow the
    // new choice.
    for name in ["scan0004.jpg", "scan0005.jpg"] {
        assert!(
            now.contains(&format!("originals/Family Scans/Jones/{name}")),
            "{name}: {now:?}"
        );
    }
    assert_eq!(now.len(), 5, "one place each, never two");
    // The look before sending found every file sent before where it was.
    let target = connect::destinations(&s.catalog).unwrap()[0].target.clone().unwrap();
    let seen = s.catalog.seen_counts(&target, "originals/").unwrap();
    assert_eq!((seen.there, seen.missing, seen.not_as_sent), (5, 0, 0));

    // Nothing at the destination was moved, changed or removed.
    let after = everything_in(&s.to);
    for (path, bytes) in &at_the_destination {
        if path.starts_with("originals") {
            assert_eq!(after.get(path), Some(bytes), "{} is as it was", path.display());
        }
    }
    assert_eq!(crate::discover(&s.home, &s.catalog).unwrap().waiting_to_gather, 0);
    let guide = std::fs::read_to_string(s.to.join(crate::layout::GUIDE_NAME)).unwrap();
    let guide = guide.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(
        guide.contains("This backup is arranged by date, except files from Family Scans."),
        "{guide}"
    );

    // A third backup sends nothing at all.
    let said = s.back_up();
    assert!(
        said.contains("5 files already there") || !said.contains("Safely backed up"),
        "{said}"
    );
    assert_eq!(everything_in(&s.source), source_before);
}

#[test]
fn a_backup_whose_arrangement_has_not_changed_does_no_extra_work_when_placing() {
    let mut s = Scans::new();
    s.look();
    crate::check_and_place(&s.home, &s.catalog, &|| true, &mut |_| {}).unwrap();
    let reports = s.catalog.load_reports().unwrap();
    let items: Vec<&keevoro_scan::FoundItem> = reports.iter().flat_map(|r| &r.items).collect();
    assert_eq!(
        crate::follow_a_changed_arrangement(&s.catalog, &items).unwrap(),
        0,
        "nothing is walked while the choice is as it was"
    );
    s.catalog.set_arrange(Arrange::Folders).unwrap();
    assert_eq!(crate::follow_a_changed_arrangement(&s.catalog, &items).unwrap(), 3);
    assert_eq!(crate::follow_a_changed_arrangement(&s.catalog, &items).unwrap(), 0);
}

#[test]
fn a_backup_nobody_asked_goes_by_date_and_says_so_once() {
    let mut s = Scans::new();
    s.look();
    s.add_the_usb_stick();
    assert_eq!(s.catalog.arrange().unwrap(), None);
    let said = s.back_up();
    assert!(said.contains(crate::summary::ARRANGED_BY_DATE_UNASKED), "{said}");
    assert_eq!(s.catalog.arrange().unwrap(), Some(Arrange::ByDate));
    s.add("Later/scan0009.jpg", 200);
    s.look();
    assert!(!s.back_up().contains("Arranged by date"));
}

#[test]
fn the_same_photo_in_a_source_by_date_and_one_keeping_its_folders_is_kept_in_the_folders() {
    let dir = tempfile::tempdir().unwrap();
    let photo = real_jpeg(77);
    // By date, the camera card's copy sits deep in named folders, which
    // would win by itself.
    let card = dir.path().join("Card");
    let deep = card.join("Holidays").join("Spain").join("Day 2");
    std::fs::create_dir_all(&deep).unwrap();
    std::fs::write(deep.join("IMG_0001.jpg"), &photo).unwrap();
    let scans = dir.path().join("Family Scans");
    std::fs::create_dir_all(&scans).unwrap();
    std::fs::write(scans.join("scan0001.jpg"), &photo).unwrap();
    let home = dir.path().join("home");
    let mut catalog = Catalog::open_in_library(&home).unwrap();
    crate::scan_into(&mut catalog, &card, Some("Card"), &mut |_| {}).unwrap();
    crate::scan_into(&mut catalog, &scans, Some("Family Scans"), &mut |_| {}).unwrap();
    catalog.set_arrange(Arrange::ByDate).unwrap();
    let id = catalog.sources().unwrap()[1].id;
    catalog.set_source_arrange(id, Some(Arrange::Folders)).unwrap();

    crate::check_and_place(&home, &catalog, &|| true, &mut |_| {}).unwrap();
    assert_eq!(
        catalog
            .collected()
            .unwrap()
            .into_iter()
            .map(|c| c.path)
            .collect::<Vec<_>>(),
        ["originals/Family Scans/scan0001.jpg"]
    );
    // And the list of duplicates says the same copy is the one kept.
    let reports = catalog.load_reports().unwrap();
    let groups = crate::summary::duplicate_groups(&reports);
    assert!(groups[0].0.path.ends_with("scan0001.jpg"));
}

#[test]
fn two_sources_called_photos_that_keep_their_folders_share_one_and_both_say_so() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let mut catalog = Catalog::open_in_library(&home).unwrap();
    for (disk, shade) in [("D", 10), ("E", 200)] {
        let photos = dir.path().join(disk).join("Photos");
        std::fs::create_dir_all(&photos).unwrap();
        std::fs::write(photos.join("IMG_0001.jpg"), real_jpeg(shade)).unwrap();
        crate::scan_into(&mut catalog, &photos, Some("Photos"), &mut |_| {}).unwrap();
    }
    catalog.set_arrange(Arrange::Folders).unwrap();
    crate::check_and_place(&home, &catalog, &|| true, &mut |_| {}).unwrap();
    let mut places: Vec<String> = catalog.collected().unwrap().into_iter().map(|c| c.path).collect();
    places.sort();
    assert_eq!(
        places,
        ["originals/Photos/IMG_0001 (2).jpg", "originals/Photos/IMG_0001.jpg"],
        "one folder, the second file numbered"
    );
    let sources = crate::discover(&home, &catalog).unwrap().sources;
    for (mine, theirs) in [(0, 1), (1, 0)] {
        assert_eq!(sources[mine].kept_as.as_deref(), Some("Photos"));
        assert_eq!(
            sources[mine].shared_said.as_deref(),
            Some(
                format!(
                    "In the backup, its folder Photos is shared with {}.",
                    sources[theirs].root
                )
                .as_str()
            )
        );
    }
}
