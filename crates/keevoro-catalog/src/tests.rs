use super::*;
use keevoro_core::{MediaKind, SkipReason};

fn photo(path: &str, hash: &str) -> FoundItem {
    FoundItem {
        path: PathBuf::from(path),
        kind: MediaKind::Photo,
        mime: "image/jpeg".into(),
        bytes: 2_483_312,
        blake3: Some(hash.repeat(64)),
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
fn gathered_files_are_remembered() {
    let c = Catalog::in_memory().unwrap();
    assert_eq!(c.collected_path("aa").unwrap(), None);
    assert_eq!(c.collected_totals().unwrap(), (0, 0));
    c.mark_collected("aa", "originals/Undated/a.jpg", 100).unwrap();
    c.mark_collected("bb", "originals/Undated/b.jpg", 50).unwrap();
    // Recording the same file twice is harmless.
    c.mark_collected("aa", "originals/Undated/a.jpg", 100).unwrap();
    assert_eq!(
        c.collected_path("aa").unwrap().as_deref(),
        Some("originals/Undated/a.jpg")
    );
    assert_eq!(c.collected_totals().unwrap(), (2, 150));
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
    c.mark_collected("abc", "originals/Undated/a.jpg", 1).unwrap();
}
