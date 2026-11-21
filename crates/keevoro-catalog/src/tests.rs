use super::*;
use keevoro_core::{MediaKind, SkipReason};

fn photo(path: &str, hash: &str) -> FoundItem {
    FoundItem {
        path: PathBuf::from(path),
        kind: MediaKind::Photo,
        mime: "image/jpeg".into(),
        bytes: 2_483_312,
        blake3: Some(hash.repeat(64)),
        hashed: true,
        date: Some(DateGuess {
            date: PartialDate::parse("2009-07-14T21:03:11").unwrap(),
            source: DateSource::Exif,
        }),
        folder: FolderHints {
            album: vec!["Holidays".into(), "Spain 2009".into()],
            date: PartialDate::year(2009),
        },
        width: Some(3264),
        height: Some(2448),
        junk: None,
    }
}

fn report(root: &str) -> ScanReport {
    let mut video = photo(&format!("{root}/clip.3gp"), "b");
    video.kind = MediaKind::Video;
    video.date = None;
    video.folder = FolderHints::default();
    video.blake3 = None;
    video.width = None;
    video.height = None;
    let mut icon = photo(&format!("{root}/icons/save.png"), "c");
    icon.junk = Some(JunkReason::AppIcons);
    ScanReport {
        root: PathBuf::from(root),
        items: vec![
            photo(&format!("{root}/Holidays/Spain 2009/IMG_1234.jpg"), "a"),
            video,
            icon,
        ],
        skipped: vec![SkippedFolder {
            path: PathBuf::from(format!("{root}/Windows")),
            reason: SkipReason::SystemFolder,
        }],
        unreadable: vec![Unreadable {
            path: PathBuf::from(format!("{root}/broken.jpg")),
            error: "Input/output error".into(),
        }],
        other_files: 41,
        online_only: Vec::new(),
        stopped: false,
    }
}

/// Reports have no `PartialEq`; their JSON form is what the rest of the app
/// sees, so compare that.
fn json(r: &[ScanReport]) -> String {
    serde_json::to_string_pretty(r).unwrap()
}

#[test]
fn a_scan_comes_back_exactly_as_it_went_in() {
    let mut c = Catalog::in_memory().unwrap();
    let original = report("/media/dell");
    c.save_scan("Dad's old Dell", &original, 1_700_000_000).unwrap();
    assert_eq!(json(&c.load_reports().unwrap()), json(&[original]));
}

#[test]
fn sources_are_listed_with_their_friendly_names() {
    let mut c = Catalog::in_memory().unwrap();
    c.save_scan("Dad's old Dell", &report("/media/dell"), 100).unwrap();
    c.save_scan("2012 backup disk", &report("/media/backup"), 200).unwrap();
    let s = c.sources().unwrap();
    assert_eq!(s.len(), 2);
    assert_eq!(
        (s[0].name.as_str(), s[0].items, s[0].scanned_at),
        ("Dad's old Dell", 3, 100)
    );
    assert_eq!(s[1].root, PathBuf::from("/media/backup"));
}

#[test]
fn scanning_the_same_place_again_replaces_what_was_known() {
    let mut c = Catalog::in_memory().unwrap();
    c.save_scan("Dell", &report("/media/dell"), 100).unwrap();
    let mut second = report("/media/dell");
    second.items.truncate(1);
    second.skipped.clear();
    c.save_scan("Dad's old Dell", &second, 200).unwrap();

    let s = c.sources().unwrap();
    assert_eq!(s.len(), 1);
    assert_eq!(
        (s[0].name.as_str(), s[0].items, s[0].scanned_at),
        ("Dad's old Dell", 1, 200)
    );
    assert_eq!(json(&c.load_reports().unwrap()), json(&[second]));
}

#[test]
fn forgetting_a_source_removes_everything_found_on_it() {
    let mut c = Catalog::in_memory().unwrap();
    let id = c.save_scan("Dell", &report("/media/dell"), 100).unwrap();
    assert!(c.forget_source(id).unwrap());
    assert!(!c.forget_source(id).unwrap());
    assert!(c.sources().unwrap().is_empty());
    assert!(c.load_reports().unwrap().is_empty());
    let leftovers: i64 = c
        .conn
        .query_row(
            "SELECT (SELECT COUNT(*) FROM items) + (SELECT COUNT(*) FROM skipped) + (SELECT COUNT(*) FROM unreadable)",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(leftovers, 0);
}

#[test]
fn choices_belong_to_the_file_and_survive_a_rescan() {
    let mut c = Catalog::in_memory().unwrap();
    let hash = "a".repeat(64);
    c.save_scan("Dell", &report("/media/dell"), 100).unwrap();
    assert_eq!(c.choice(&hash).unwrap(), None);

    c.set_choice(&hash, false).unwrap();
    assert_eq!(c.choice(&hash).unwrap(), Some(false));
    c.set_choice(&hash, true).unwrap();
    assert_eq!(c.choice(&hash).unwrap(), Some(true));

    c.save_scan("Dell", &report("/media/dell"), 200).unwrap();
    let id = c.sources().unwrap()[0].id;
    c.forget_source(id).unwrap();
    assert_eq!(
        c.choice(&hash).unwrap(),
        Some(true),
        "a choice outlives the scan it was made on"
    );

    c.clear_choice(&hash).unwrap();
    assert_eq!(c.choice(&hash).unwrap(), None);
}

#[test]
fn the_catalog_file_is_created_inside_the_library_and_reopens() {
    let dir = tempfile::tempdir().unwrap();
    let library = dir.path().join("Keevoro Library");
    {
        let mut c = Catalog::open_in_library(&library).unwrap();
        c.save_scan("Dell", &report("/media/dell"), 100).unwrap();
        c.set_choice("abc", false).unwrap();
    }
    assert!(library.join(".keevoro").join("catalog.sqlite").is_file());
    let c = Catalog::open_in_library(&library).unwrap();
    assert_eq!(c.sources().unwrap().len(), 1);
    assert_eq!(c.choice("abc").unwrap(), Some(false));
    let version: i64 = c.conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
    assert_eq!(usize::try_from(version).unwrap(), MIGRATIONS.len());
}

#[test]
fn a_catalog_from_a_newer_keevoro_is_refused_politely() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("catalog.sqlite");
    drop(Catalog::open(&file).unwrap());
    Connection::open(&file)
        .unwrap()
        .pragma_update(None, "user_version", i64::try_from(MIGRATIONS.len()).unwrap() + 1)
        .unwrap();
    let err = Catalog::open(&file).err().unwrap();
    assert!(matches!(err, CatalogError::NewerVersion));
    assert!(err.to_string().contains("update Keevoro"));
}

#[test]
fn damaged_rows_are_reported_not_trusted() {
    for (column, value) in [
        ("date", "'not a date'"),
        ("date_source", "'tea leaves'"),
        ("kind", "'hologram'"),
        ("junk", "'nonsense'"),
        ("album", "'not json'"),
        ("folder_date", "'2009-99'"),
    ] {
        let mut c = Catalog::in_memory().unwrap();
        c.save_scan("Dell", &report("/media/dell"), 100).unwrap();
        c.conn
            .execute(&format!("UPDATE items SET {column} = {value} WHERE id = 1"), [])
            .unwrap();
        let err = c.load_reports().err().unwrap();
        assert!(matches!(err, CatalogError::Damaged(_)), "{column}");
        assert!(err.to_string().contains("Scanning your drives again"));
    }
    let mut c = Catalog::in_memory().unwrap();
    c.save_scan("Dell", &report("/media/dell"), 100).unwrap();
    c.conn.execute("UPDATE skipped SET reason = 'whim'", []).unwrap();
    assert!(matches!(c.load_reports().err().unwrap(), CatalogError::Damaged(_)));
}

#[test]
fn a_file_that_is_not_a_database_gives_a_plain_error() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("catalog.sqlite");
    std::fs::write(&file, b"this is not a database, it is a shopping list").unwrap();
    let err = Catalog::open(&file).err().unwrap();
    assert!(err.to_string().starts_with("Couldn't open the library's catalog"));
}

#[test]
fn places_are_agreed_once() {
    let c = Catalog::in_memory().unwrap();
    assert_eq!(c.collected_path("aa").unwrap(), None);
    c.mark_placed("aa", "originals/Undated/a.jpg", 100).unwrap();
    c.mark_placed("bb", "originals/Undated/b.jpg", 50).unwrap();
    // A file that has its place keeps it.
    c.mark_placed("aa", "originals/Undated/elsewhere.jpg", 100).unwrap();
    assert_eq!(
        c.collected_path("aa").unwrap().as_deref(),
        Some("originals/Undated/a.jpg")
    );
    assert!(c.place_taken("originals/Undated/b.jpg").unwrap());
    assert!(!c.place_taken("originals/Undated/elsewhere.jpg").unwrap());
}

#[test]
fn an_older_catalog_is_upgraded_in_place() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("catalog.sqlite");
    {
        let conn = Connection::open(&file).unwrap();
        conn.execute_batch(MIGRATIONS[0]).unwrap();
        conn.pragma_update(None, "user_version", 1).unwrap();
        conn.execute("INSERT INTO choices (blake3, keep) VALUES ('abc', 0)", [])
            .unwrap();
    }
    let c = Catalog::open(&file).unwrap();
    assert_eq!(
        c.choice("abc").unwrap(),
        Some(false),
        "existing data survives the upgrade"
    );
    c.mark_placed("abc", "originals/Undated/a.jpg", 1).unwrap();
}

#[test]
fn previews_are_remembered_including_the_ones_that_could_not_be_made() {
    let c = Catalog::in_memory().unwrap();
    assert_eq!(c.rendition("aa").unwrap(), None);
    let made = RenditionState::Made {
        width: 3264,
        height: 2448,
        blurhash: "LKO2?U%2Tw=w]~RBVZRi};RPxuwH".into(),
    };
    c.set_rendition("aa", &made).unwrap();
    assert_eq!(c.rendition("aa").unwrap(), Some(made.clone()));

    let none = RenditionState::NoPreview {
        reason: "This kind of picture can't be previewed yet".into(),
    };
    c.set_rendition("bb", &none).unwrap();
    assert_eq!(c.rendition("bb").unwrap(), Some(none));
    // A later success replaces an earlier failure.
    c.set_rendition("bb", &made).unwrap();
    assert_eq!(c.rendition("bb").unwrap(), Some(made));
}

#[test]
fn uploads_are_tracked_per_storage_target() {
    let c = Catalog::in_memory().unwrap();
    assert!(!c.is_uploaded("r2:family", "originals/a.jpg").unwrap());
    c.mark_uploaded("r2:family", "originals/a.jpg", 10).unwrap();
    c.mark_uploaded("r2:family", "originals/a.jpg", 10).unwrap();
    assert!(c.is_uploaded("r2:family", "originals/a.jpg").unwrap());
    assert!(
        !c.is_uploaded("nas", "originals/a.jpg").unwrap(),
        "another target has its own record"
    );

    c.set_multipart("r2:family", "originals/big.mov", Some("upload-1"))
        .unwrap();
    c.set_multipart("r2:family", "originals/big.mov", Some("upload-2"))
        .unwrap();
    assert_eq!(
        c.multipart("r2:family", "originals/big.mov").unwrap().as_deref(),
        Some("upload-2")
    );
    c.mark_uploaded("r2:family", "originals/big.mov", 99).unwrap();
    assert_eq!(
        c.multipart("r2:family", "originals/big.mov").unwrap(),
        None,
        "finishing clears it"
    );

    c.set_multipart("r2:family", "x", Some("u")).unwrap();
    c.set_multipart("r2:family", "x", None).unwrap();
    assert_eq!(c.multipart("r2:family", "x").unwrap(), None);

    c.set_multipart("r2:family", "y", Some("u")).unwrap();
    c.forget_uploads("r2:family").unwrap();
    assert!(!c.is_uploaded("r2:family", "originals/a.jpg").unwrap());
    assert_eq!(c.multipart("r2:family", "y").unwrap(), None);
}

#[test]
fn a_file_found_gone_is_written_down_as_gone_and_counts_as_not_there_until_it_is_sent_again() {
    let c = Catalog::in_memory().unwrap();
    for key in ["originals/a.jpg", "originals/b.jpg", "originals/c.jpg"] {
        c.mark_uploaded("usb", key, 10).unwrap();
    }
    c.mark_uploaded("nas", "originals/a.jpg", 10).unwrap();
    c.set_seen(
        "usb",
        &[
            ("originals/a.jpg".into(), Seen::Missing),
            ("originals/b.jpg".into(), Seen::NotAsSent),
            // A look never adds what Keevoro did not send.
            ("originals/not-mine.jpg".into(), Seen::There),
        ],
    )
    .unwrap();
    assert!(!c.is_uploaded("usb", "originals/a.jpg").unwrap());
    assert!(!c.is_uploaded("usb", "originals/b.jpg").unwrap());
    assert!(c.is_uploaded("usb", "originals/c.jpg").unwrap());
    assert_eq!(c.sent_state("usb", "originals/a.jpg").unwrap(), Some(Seen::Missing));
    assert_eq!(c.sent_state("usb", "originals/not-mine.jpg").unwrap(), None);
    assert!(
        c.is_uploaded("nas", "originals/a.jpg").unwrap(),
        "another place is its own"
    );
    assert!(c.stored_anywhere("originals/a.jpg").unwrap(), "it is still at the NAS");
    assert!(!c.stored_anywhere("originals/b.jpg").unwrap());
    let sent = |key: &str, seen| Sent {
        key: key.into(),
        bytes: 10,
        seen,
    };
    assert_eq!(
        c.sent_to("usb").unwrap(),
        [
            sent("originals/a.jpg", Seen::Missing),
            sent("originals/b.jpg", Seen::NotAsSent),
            sent("originals/c.jpg", Seen::There),
        ]
    );
    assert_eq!(
        c.seen_counts("usb", "originals/").unwrap(),
        SeenCounts {
            there: 1,
            missing: 1,
            not_as_sent: 1
        }
    );
    assert_eq!(c.seen_counts("usb", "renditions/").unwrap().sent(), 0);
    // Sent again, it is there again; found back where it was, likewise.
    c.mark_uploaded("usb", "originals/a.jpg", 10).unwrap();
    c.set_seen("usb", &[("originals/b.jpg".into(), Seen::There)]).unwrap();
    assert!(c.is_uploaded("usb", "originals/a.jpg").unwrap());
    assert!(c.is_uploaded("usb", "originals/b.jpg").unwrap());
}

#[test]
fn what_an_older_catalog_says_was_sent_is_kept_and_counts_as_there_until_it_is_looked_for() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("catalog.sqlite");
    {
        // As every version before this one left it.
        let conn = Connection::open(&file).unwrap();
        for step in &MIGRATIONS[..8] {
            conn.execute_batch(step).unwrap();
        }
        conn.pragma_update(None, "user_version", 8).unwrap();
        conn.execute(
            "INSERT INTO uploaded (target, key, bytes) VALUES ('folder:/usb', 'originals/a.jpg', 12)",
            [],
        )
        .unwrap();
    }
    let c = Catalog::open(&file).unwrap();
    assert!(c.is_uploaded("folder:/usb", "originals/a.jpg").unwrap());
    assert_eq!(
        c.sent_to("folder:/usb").unwrap(),
        [Sent {
            key: "originals/a.jpg".into(),
            bytes: 12,
            seen: Seen::There
        }]
    );
}

#[test]
fn settings_are_small_named_values() {
    let c = Catalog::in_memory().unwrap();
    assert_eq!(c.setting("index_key").unwrap(), None);
    c.set_setting("index_key", "index/abc.kvx").unwrap();
    c.set_setting("index_key", "index/def.kvx").unwrap();
    assert_eq!(c.setting("index_key").unwrap().as_deref(), Some("index/def.kvx"));
}

#[test]
fn videos_are_remembered_with_what_was_made_of_them() {
    let c = Catalog::in_memory().unwrap();
    assert_eq!(c.video("aa").unwrap(), None);
    let known = VideoState::Known {
        duration_secs: 37.4,
        width: 640,
        height: 480,
        poster: true,
        mp4: false,
        conversion_problem: Some("The picture seems to be damaged".into()),
    };
    c.set_video("aa", &known).unwrap();
    assert_eq!(c.video("aa").unwrap(), Some(known));

    let unreadable = VideoState::Unreadable {
        reason: "The picture seems to be damaged".into(),
    };
    c.set_video("bb", &unreadable).unwrap();
    assert_eq!(c.video("bb").unwrap(), Some(unreadable));
    // Later success replaces it.
    let fine = VideoState::Known {
        duration_secs: 2.0,
        width: 320,
        height: 240,
        poster: true,
        mp4: true,
        conversion_problem: None,
    };
    c.set_video("bb", &fine).unwrap();
    assert_eq!(c.video("bb").unwrap(), Some(fine));
}

#[test]
fn remembered_preview_problems_can_be_forgotten_so_they_are_tried_again() {
    let c = Catalog::in_memory().unwrap();
    let made = RenditionState::Made {
        width: 10,
        height: 10,
        blurhash: "x".into(),
    };
    c.set_rendition("good", &made).unwrap();
    c.set_rendition(
        "raw",
        &RenditionState::NoPreview {
            reason: "not yet".into(),
        },
    )
    .unwrap();
    c.set_video(
        "gone",
        &VideoState::Unreadable {
            reason: "damaged".into(),
        },
    )
    .unwrap();
    let stuck = VideoState::Known {
        duration_secs: 1.0,
        width: 2,
        height: 2,
        poster: true,
        mp4: false,
        conversion_problem: Some("couldn't".into()),
    };
    c.set_video("stuck", &stuck).unwrap();
    let fine = VideoState::Known {
        duration_secs: 1.0,
        width: 2,
        height: 2,
        poster: true,
        mp4: true,
        conversion_problem: None,
    };
    c.set_video("fine", &fine).unwrap();

    assert_eq!(c.forget_preview_problems().unwrap(), 3);
    assert_eq!(c.rendition("good").unwrap(), Some(made));
    assert_eq!(c.rendition("raw").unwrap(), None);
    assert_eq!(c.video("gone").unwrap(), None);
    assert_eq!(c.video("stuck").unwrap(), None);
    assert_eq!(c.video("fine").unwrap(), Some(fine));
    assert_eq!(c.forget_preview_problems().unwrap(), 0);
}

#[test]
fn the_files_explicitly_kept_can_be_listed() {
    let c = Catalog::in_memory().unwrap();
    c.set_choice("aa", true).unwrap();
    c.set_choice("bb", false).unwrap();
    c.set_choice("cc", true).unwrap();
    let kept = c.kept_hashes().unwrap();
    assert_eq!(kept.len(), 2);
    assert!(kept.contains("aa") && kept.contains("cc") && !kept.contains("bb"));
}

#[test]
fn everything_with_a_place_can_be_listed_for_checking() {
    let c = Catalog::in_memory().unwrap();
    assert!(c.collected().unwrap().is_empty());
    c.mark_placed("bb", "originals/b.jpg", 50).unwrap();
    c.mark_placed("aa", "originals/a.jpg", 100).unwrap();
    let all = c.collected().unwrap();
    assert_eq!(all.len(), 2);
    assert_eq!(
        (all[0].path.as_str(), all[0].blake3.as_str(), all[0].bytes),
        ("originals/a.jpg", "aa", 100)
    );
}

#[test]
fn a_file_can_be_found_by_the_start_of_its_content_id() {
    let mut c = Catalog::in_memory().unwrap();
    c.save_scan("Dell", &report("/media/dell"), 100).unwrap();
    let (path, kind, hash) = c.find_item(&"a".repeat(16)).unwrap().unwrap();
    assert!(path.ends_with("IMG_1234.jpg"));
    assert_eq!((kind, hash.len()), (MediaKind::Photo, 64));
    assert!(c.find_item(&"A".repeat(16)).unwrap().is_some(), "case does not matter");
    assert_eq!(c.find_item("ffff").unwrap(), None);
    // Not hex, so it cannot be an id, and must not match everything.
    for odd in ["", "%", "a%", "_", "zz"] {
        assert_eq!(c.find_item(odd).unwrap(), None, "{odd:?}");
    }
}

/// A source with two files known only by quick fingerprints.
fn quick_report() -> ScanReport {
    let mut a = photo("/mnt/usb/a.jpg", "1");
    a.hashed = false;
    let mut b = photo("/mnt/usb/b.jpg", "1");
    b.hashed = false;
    ScanReport {
        root: PathBuf::from("/mnt/usb"),
        items: vec![a, b, photo("/mnt/usb/c.jpg", "c")],
        ..Default::default()
    }
}

#[test]
fn quick_fingerprints_are_listed_until_the_whole_one_is_known() {
    let mut c = Catalog::in_memory().unwrap();
    c.save_scan("USB", &quick_report(), 1).unwrap();
    let waiting = c.unhashed().unwrap();
    assert_eq!(waiting.len(), 2);
    assert_eq!(waiting[0].path, PathBuf::from("/mnt/usb/a.jpg"));
    assert_eq!(waiting[0].quick, "1".repeat(64));

    c.set_whole_hash(&waiting[0].path, &waiting[0].quick, &"a".repeat(64))
        .unwrap();
    assert_eq!(c.unhashed().unwrap().len(), 1);
    let items = &c.load_reports().unwrap()[0].items;
    assert_eq!(
        (items[0].blake3.as_deref(), items[0].hashed),
        (Some("a".repeat(64).as_str()), true)
    );
    assert!(!items[1].hashed);
    // Asking again, or for a file that is not there, changes nothing.
    c.set_whole_hash(&waiting[0].path, &waiting[0].quick, &"f".repeat(64))
        .unwrap();
    assert_eq!(c.load_reports().unwrap()[0].items[0].blake3, Some("a".repeat(64)));
}

#[test]
fn a_choice_made_early_follows_each_file_to_its_real_fingerprint() {
    let mut c = Catalog::in_memory().unwrap();
    c.save_scan("USB", &quick_report(), 1).unwrap();
    let quick = "1".repeat(64);
    c.set_choice(&quick, false).unwrap();

    // The two files shared a quick fingerprint but are really different.
    c.set_whole_hash(Path::new("/mnt/usb/a.jpg"), &quick, &"a".repeat(64))
        .unwrap();
    assert_eq!(c.choice(&"a".repeat(64)).unwrap(), Some(false));
    assert_eq!(c.choice(&quick).unwrap(), Some(false), "b.jpg still goes by it");
    c.set_whole_hash(Path::new("/mnt/usb/b.jpg"), &quick, &"b".repeat(64))
        .unwrap();
    assert_eq!(c.choice(&"b".repeat(64)).unwrap(), Some(false));
    assert_eq!(c.choice(&quick).unwrap(), None, "nothing goes by it any more");
}

#[test]
fn a_later_choice_is_not_overwritten_by_an_earlier_one() {
    let mut c = Catalog::in_memory().unwrap();
    c.save_scan("USB", &quick_report(), 1).unwrap();
    let quick = "1".repeat(64);
    c.set_choice(&quick, false).unwrap();
    c.set_choice(&"a".repeat(64), true).unwrap();
    c.set_whole_hash(Path::new("/mnt/usb/a.jpg"), &quick, &"a".repeat(64))
        .unwrap();
    assert_eq!(c.choice(&"a".repeat(64)).unwrap(), Some(true));
}

#[test]
fn looking_again_does_not_forget_fingerprints_already_worked_out() {
    let mut c = Catalog::in_memory().unwrap();
    c.save_scan("USB", &quick_report(), 1).unwrap();
    let quick = "1".repeat(64);
    c.set_whole_hash(Path::new("/mnt/usb/a.jpg"), &quick, &"a".repeat(64))
        .unwrap();

    // The same quick search again: a.jpg is recognised, b.jpg still waits.
    c.save_scan("USB", &quick_report(), 2).unwrap();
    let waiting = c.unhashed().unwrap();
    assert_eq!(waiting.len(), 1);
    assert_eq!(waiting[0].path, PathBuf::from("/mnt/usb/b.jpg"));
    assert_eq!(c.load_reports().unwrap()[0].items[0].blake3, Some("a".repeat(64)));

    // A file that changed (a different quick fingerprint) is not trusted.
    let mut changed = quick_report();
    changed.items[0].blake3 = Some("2".repeat(64));
    c.save_scan("USB", &changed, 3).unwrap();
    assert_eq!(c.unhashed().unwrap().len(), 2);
}

#[test]
fn a_picture_can_still_be_found_by_the_id_from_its_quick_fingerprint() {
    let mut c = Catalog::in_memory().unwrap();
    c.save_scan("USB", &quick_report(), 1).unwrap();
    let quick = "1".repeat(64);
    c.set_whole_hash(Path::new("/mnt/usb/a.jpg"), &quick, &"a".repeat(64))
        .unwrap();
    assert_eq!(
        c.find_item(&quick[..16]).unwrap().unwrap().0,
        PathBuf::from("/mnt/usb/a.jpg")
    );
    assert_eq!(
        c.find_item(&"a".repeat(16)).unwrap().unwrap().0,
        PathBuf::from("/mnt/usb/a.jpg")
    );
}

/// A drone's card: paired JPG and raw files, and a video.
fn drone_report() -> ScanReport {
    let mut items = Vec::new();
    for (name, hash) in [
        ("DJI_0001.JPG", "1"),
        ("DJI_0001.DNG", "2"),
        ("DJI_0002.JPG", "3"),
        ("DJI_0002.DNG", "4"),
    ] {
        items.push(photo(&format!("/mnt/card/DCIM/{name}"), hash));
    }
    let mut clip = photo("/mnt/card/DCIM/DJI_0003.MP4", "5");
    clip.kind = MediaKind::Video;
    items.push(clip);
    ScanReport {
        root: PathBuf::from("/mnt/card"),
        items,
        ..Default::default()
    }
}

#[test]
fn the_kinds_of_file_on_a_source_are_counted() {
    let mut c = Catalog::in_memory().unwrap();
    let id = c.save_scan("Card", &drone_report(), 1).unwrap();
    let kinds: Vec<(String, u64, bool)> = c
        .file_types(id)
        .unwrap()
        .into_iter()
        .map(|k| (k.ending, k.files, k.included))
        .collect();
    assert_eq!(
        kinds,
        [
            ("dng".to_string(), 2, true),
            ("jpg".to_string(), 2, true),
            ("mp4".to_string(), 1, true)
        ]
    );
    assert_eq!(c.file_types(id).unwrap()[0].bytes, 2 * 2_483_312);
    assert!(c.file_types(9999).unwrap().is_empty());
}

#[test]
fn a_kind_left_out_of_a_source_is_simply_not_there_and_can_be_put_back() {
    let mut c = Catalog::in_memory().unwrap();
    let id = c.save_scan("Card", &drone_report(), 1).unwrap();
    assert_eq!(c.load_reports().unwrap()[0].items.len(), 5);

    c.set_type_included(id, ".DNG", false).unwrap();
    let names: Vec<String> = c.load_reports().unwrap()[0]
        .items
        .iter()
        .map(|i| i.path.file_name().unwrap().to_string_lossy().to_string())
        .collect();
    assert_eq!(names, ["DJI_0001.JPG", "DJI_0002.JPG", "DJI_0003.MP4"]);
    assert!(
        !c.file_types(id)
            .unwrap()
            .iter()
            .find(|k| k.ending == "dng")
            .unwrap()
            .included
    );

    // Looking through the source again does not forget the choice.
    let again = c.save_scan("Card", &drone_report(), 2).unwrap();
    assert_eq!(c.load_reports().unwrap()[0].items.len(), 3);

    c.set_type_included(again, "dng", true).unwrap();
    assert_eq!(c.load_reports().unwrap()[0].items.len(), 5);
}

#[test]
fn a_choice_about_one_source_does_not_touch_another() {
    let mut c = Catalog::in_memory().unwrap();
    let card = c.save_scan("Card", &drone_report(), 1).unwrap();
    let mut other = drone_report();
    other.root = PathBuf::from("/mnt/other");
    c.save_scan("Other", &other, 1).unwrap();
    c.set_type_included(card, "dng", false).unwrap();
    let reports = c.load_reports().unwrap();
    assert_eq!((reports[0].items.len(), reports[1].items.len()), (3, 5));
}

#[test]
fn raw_files_wrongly_set_aside_by_an_earlier_version_are_put_back() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("catalog.sqlite");
    {
        let mut c = Catalog::open(&file).unwrap();
        let mut report = drone_report();
        for item in &mut report.items {
            // As an earlier version recorded them.
            item.junk = Some(JunkReason::TinyImage);
            item.width = Some(160);
            item.height = Some(120);
        }
        c.save_scan("Card", &report, 1).unwrap();
        // Rewind to before the fix, so opening again applies it, taking
        // away what later steps add so that they can add it again.
        c.conn.execute_batch("ALTER TABLE uploaded DROP COLUMN gone").unwrap();
        c.conn.pragma_update(None, "user_version", 6).unwrap();
    }
    let c = Catalog::open(&file).unwrap();
    for item in &c.load_reports().unwrap()[0].items {
        let raw = item.path.extension().unwrap() == "DNG";
        assert_eq!(item.junk.is_none(), raw, "{}", item.path.display());
        assert_eq!(item.width.is_none(), raw, "{}", item.path.display());
    }
}

#[test]
fn a_folder_left_out_takes_what_was_found_beneath_it_away_and_put_back_brings_it_back() {
    use keevoro_core::LeftOutBy;
    let mut c = Catalog::in_memory().unwrap();
    let mut report = drone_report();
    let mut twin = photo("/mnt/card/DCIM 2/DJI_0009.JPG", "9");
    twin.kind = MediaKind::Photo;
    report.items.push(twin);
    let id = c.save_scan("Card", &report, 1).unwrap();
    let count = |c: &Catalog| c.load_reports().unwrap()[0].items.len();
    assert_eq!(count(&c), 6);

    // It need not be a source, and saying it twice changes nothing.
    let dcim = PathBuf::from("/mnt/card/DCIM");
    assert!(c.set_folder_left_out(&dcim, true).unwrap());
    assert!(!c.set_folder_left_out(&dcim, true).unwrap());
    assert_eq!(c.left_out_folders().unwrap(), std::slice::from_ref(&dcim));
    assert_eq!(count(&c), 1, "only the photo in DCIM 2, beside it, is left");
    assert_eq!(
        c.load_reports().unwrap()[0].items[0].path,
        PathBuf::from("/mnt/card/DCIM 2/DJI_0009.JPG")
    );
    let choices = c.choices().unwrap();
    let facts = keevoro_core::FileFacts {
        path: Path::new("/mnt/card/DCIM/DJI_0001.JPG"),
        kind: MediaKind::Photo,
        looked_for: true,
        made_by_keevoro: false,
    };
    assert_eq!(choices.offered(&facts), Err(LeftOutBy::Folder));
    // The found files themselves are kept, only not offered.
    assert_eq!(c.file_types(id).unwrap().iter().map(|t| t.files).sum::<u64>(), 6);

    // Put back, all of it is offered again, with no new search.
    assert!(c.set_folder_left_out(&dcim, false).unwrap());
    assert!(!c.set_folder_left_out(&dcim, false).unwrap(), "nothing to put back");
    assert_eq!(count(&c), 6);
    assert!(c.left_out_folders().unwrap().is_empty());
}

#[test]
fn photos_only_or_videos_only_is_a_filter_over_what_was_found_for_a_backup_or_one_source() {
    use keevoro_core::{LeftOutBy, Wants};
    let mut c = Catalog::in_memory().unwrap();
    let id = c.save_scan("Card", &drone_report(), 1).unwrap();
    let kinds = |c: &Catalog| -> Vec<MediaKind> { c.load_reports().unwrap()[0].items.iter().map(|i| i.kind).collect() };
    assert_eq!(c.wants().unwrap(), Wants::default());

    c.set_wants(Wants::only(MediaKind::Photo)).unwrap();
    assert!(kinds(&c).iter().all(|k| *k == MediaKind::Photo) && kinds(&c).len() == 4);
    let why = |c: &Catalog, ending: &str| {
        c.file_types(id)
            .unwrap()
            .into_iter()
            .find(|k| k.ending == ending)
            .unwrap()
            .left_out_by
    };
    assert_eq!(why(&c, "mp4"), Some(LeftOutBy::Backup(Wants::only(MediaKind::Photo))));
    assert_eq!(why(&c, "jpg"), None);

    // This source differs: everything.
    assert!(c.set_source_wants(id, Some(Wants::default())).unwrap());
    assert_eq!(kinds(&c).len(), 5);
    assert_eq!(c.source_wants(id).unwrap(), Some(Wants::default()));
    c.set_source_wants(id, Some(Wants::only(MediaKind::Video))).unwrap();
    assert_eq!(kinds(&c), [MediaKind::Video]);
    assert_eq!(why(&c, "jpg"), Some(LeftOutBy::Source(Wants::only(MediaKind::Video))));

    // Back to the backup's choice, and the choice is the place's, so it
    // outlasts the source being looked through again.
    c.set_source_wants(id, None).unwrap();
    assert_eq!(c.source_wants(id).unwrap(), None);
    c.set_source_wants(id, Some(Wants::only(MediaKind::Video))).unwrap();
    let again = c.save_scan("Card", &drone_report(), 2).unwrap();
    assert_eq!(c.source_wants(again).unwrap(), Some(Wants::only(MediaKind::Video)));
    assert!(!c.set_source_wants(9999, None).unwrap(), "no such source");
    // Kept as the list of what it is for.
    assert_eq!(c.setting("wants").unwrap().as_deref(), Some(r#"["photos"]"#));

    // Nothing found was forgotten.
    c.set_wants(Wants::default()).unwrap();
    c.set_source_wants(again, None).unwrap();
    assert_eq!(kinds(&c).len(), 5);
}

#[test]
fn how_many_files_were_only_online_is_kept_with_the_source_until_it_is_looked_through_again_or_forgotten() {
    let mut c = Catalog::in_memory().unwrap();
    let mut first = report("/media/onedrive");
    first.online_only = vec![
        PathBuf::from("/media/onedrive/a.jpg"),
        PathBuf::from("/media/onedrive/b.mov"),
    ];
    c.save_scan("OneDrive", &first, 100).unwrap();
    assert_eq!(c.sources().unwrap()[0].online_only, 2);
    // Made available on this computer, then looked through again.
    let id = c.save_scan("OneDrive", &report("/media/onedrive"), 200).unwrap();
    assert_eq!(c.sources().unwrap()[0].online_only, 0);
    c.save_scan("OneDrive", &first, 300).unwrap();
    let id = c.sources().unwrap()[0].id.max(id);
    assert!(c.forget_source(id).unwrap());
    let left: i64 = c
        .conn
        .query_row(
            "SELECT COUNT(*) FROM settings WHERE key LIKE 'online_only:%'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(left, 0, "nothing is kept about a source forgotten");
}

/// A catalog as 0.38.0 wrote it, with the backup set to photos only, one
/// source to videos only and another to both, which 0.39.0 read as nothing
/// chosen.
#[test]
fn photos_only_or_videos_only_chosen_before_0_39_is_carried_over() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("catalog.sqlite");
    {
        let conn = Connection::open(&file).unwrap();
        for step in &MIGRATIONS[..8] {
            conn.execute_batch(step).unwrap();
        }
        conn.pragma_update(None, "user_version", 8).unwrap();
        conn.execute_batch(
            r#"INSERT INTO sources (id, name, root, scanned_at) VALUES (1, 'Card', '/media/card', 1), (2, 'Drone', '/media/drone', 1), (3, 'Phone', '/media/phone', 1);
               INSERT INTO settings (key, value) VALUES
                 ('wants', '"photos"'),
                 ('wants:/media/card', '"videos"'),
                 ('wants:/media/drone', '"both"'),
                 ('left_out_types:/media/card', '["png"]');"#,
        )
        .unwrap();
    }
    let c = Catalog::open(&file).unwrap();
    assert_eq!(c.wants().unwrap(), keevoro_core::Wants::only(MediaKind::Photo));
    assert_eq!(
        c.source_wants(1).unwrap(),
        Some(keevoro_core::Wants::only(MediaKind::Video))
    );
    assert_eq!(
        c.source_wants(2).unwrap(),
        Some(keevoro_core::Wants::default()),
        "both, chosen for one source, still overrides the backup's photos only"
    );
    assert_eq!(
        c.source_wants(3).unwrap(),
        None,
        "a source with no choice still follows its backup"
    );
    // Nothing else is touched, and a list written since is left alone.
    assert_eq!(
        c.setting("left_out_types:/media/card").unwrap().as_deref(),
        Some(r#"["png"]"#)
    );
    c.set_wants(keevoro_core::Wants::only(MediaKind::Video)).unwrap();
    drop(c);
    let c = Catalog::open(&file).unwrap();
    assert_eq!(c.wants().unwrap(), keevoro_core::Wants::only(MediaKind::Video));
}
