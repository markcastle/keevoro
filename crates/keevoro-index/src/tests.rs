use super::*;
use keevoro_core::PartialDate;

fn item(id: &str, path: &str, taken: Option<(&str, DateSource)>) -> ItemInput {
    ItemInput {
        id: id.to_string(),
        kind: MediaKind::Photo,
        path: path.to_string(),
        date: taken.map(|(d, source)| DateGuess {
            date: PartialDate::parse(d).unwrap(),
            source,
        }),
        width: Some(3264),
        height: Some(2448),
        bytes: 2_483_312,
        mime: "image/jpeg".into(),
        blurhash: Some("LKO2?U%2Tw=w]~RBVZRi};RPxuwH".into()),
        duration: None,
        renditions: vec![Rendition::Thumb, Rendition::Display],
    }
}

fn album(path: &[&str], items: &[&str]) -> AlbumInput {
    AlbumInput {
        path: path.iter().map(|s| (*s).to_string()).collect(),
        items: items.iter().map(|s| (*s).to_string()).collect(),
    }
}

fn spain() -> Index {
    let mut video = item(
        "41aa09cd7e5b2f10",
        "originals/Holidays/Spain 2009/Day 2/MVI_0042.MOV",
        Some(("2009-07-19T21:10:02", DateSource::Video)),
    );
    video.kind = MediaKind::Video;
    video.mime = "video/quicktime".into();
    video.blurhash = None;
    video.duration = Some(37.4004);
    video.renditions = vec![Rendition::Poster, Rendition::Video];
    build_index(
        "https://photos.example.com",
        1_794_749_400,
        &[
            album(
                &["Holidays", "Spain 2009"],
                &["41aa09cd7e5b2f10", "9f2c4be1a07d33e8", "not-published"],
            ),
            album(&["August 2008"], &["77aa000000000000"]),
        ],
        vec![
            item(
                "9f2c4be1a07d33e8",
                "originals/Holidays/Spain 2009/Day 2/IMG_1234.jpg",
                Some(("2009-07-12T21:03:11", DateSource::Exif)),
            ),
            video,
            item(
                "77aa000000000000",
                "originals/By date/2008/2008-08/a b#1.jpg",
                Some(("2008", DateSource::Folder)),
            ),
        ],
    )
}

#[test]
fn matches_the_planned_shape() {
    let v = serde_json::to_value(spain()).unwrap();
    assert_eq!(v["schema"], "keevoro.index/2");
    assert_eq!(v["generatedAt"], "2026-11-15T13:30:00Z");
    assert_eq!(v["baseUrl"], "https://photos.example.com/", "a missing slash is added");
    assert_eq!(v["renditions"]["thumb"]["path"], "renditions/thumb/{id}.webp");
    assert_eq!(v["renditions"]["thumb"]["maxEdge"], 400);
    assert_eq!(v["renditions"]["video"]["codec"], "h264+aac");
    assert!(v["renditions"]["video"].get("maxEdge").is_none());

    let photo = &v["items"]["9f2c4be1a07d33e8"];
    assert_eq!(photo["type"], "photo");
    assert_eq!(photo["path"], "originals/Holidays/Spain 2009/Day 2/IMG_1234.jpg");
    assert_eq!(photo["taken"], "2009-07-12T21:03:11");
    assert_eq!(photo["dateSource"], "exif");
    assert_eq!((photo["w"].as_u64(), photo["h"].as_u64()), (Some(3264), Some(2448)));
    assert_eq!(photo["renditions"], serde_json::json!(["thumb", "display"]));
    assert!(photo.get("duration").is_none());

    let video = &v["items"]["41aa09cd7e5b2f10"];
    assert_eq!(video["type"], "video");
    assert!(video.get("blurhash").is_none());
    assert_eq!(video["renditions"], serde_json::json!(["poster", "video"]));
    assert_eq!(video["duration"], 37.4);
}

#[test]
fn albums_nest_and_parents_are_filled_in() {
    let index = spain();
    let ids: Vec<_> = index.albums.iter().map(|a| a.id.as_str()).collect();
    assert_eq!(ids, ["august-2008", "holidays", "holidays-spain-2009"]);

    let parent = &index.albums[1];
    assert_eq!((parent.title.as_str(), parent.parent.as_deref()), ("Holidays", None));
    assert!(parent.items.is_empty() && parent.cover.is_none() && parent.date_range.is_none());

    let spain = &index.albums[2];
    assert_eq!(spain.title, "Spain 2009");
    assert_eq!(spain.parent.as_deref(), Some("holidays"));
    assert_eq!(
        spain.items,
        ["41aa09cd7e5b2f10", "9f2c4be1a07d33e8"],
        "unpublished ids are dropped"
    );
    assert_eq!(spain.date_range, Some(("2009-07-12".into(), "2009-07-19".into())));
    assert_eq!(
        spain.cover.as_deref(),
        Some("9f2c4be1a07d33e8"),
        "the cover is something with a thumbnail"
    );
}

#[test]
fn vague_and_copied_dates_do_not_make_a_date_range() {
    let index = build_index(
        "https://x/",
        0,
        &[album(&["Stuff"], &["a", "b"])],
        vec![
            item("a", "originals/Stuff/a.jpg", Some(("2008", DateSource::Folder))),
            item(
                "b",
                "originals/Stuff/b.jpg",
                Some(("2015-01-01T10:00:00", DateSource::Modified)),
            ),
        ],
    );
    assert_eq!(index.albums[0].date_range, None);
    assert_eq!(index.items["b"].date_source, Some(IndexDateSource::Mtime));
    assert_eq!(index.generated_at, "1970-01-01T00:00:00Z");
}

#[test]
fn album_ids_are_unique_and_never_empty() {
    let index = build_index(
        "https://x/",
        0,
        &[
            album(&["Mum & Dad"], &[]),
            album(&["Mum, Dad"], &[]),
            album(&["!!!"], &[]),
            album(&[], &[]),
        ],
        vec![],
    );
    let ids: Vec<_> = index.albums.iter().map(|a| a.id.as_str()).collect();
    assert_eq!(ids, ["album", "mum-dad", "mum-dad-2"]);
}

#[test]
fn the_same_album_listed_twice_is_merged() {
    let index = build_index(
        "https://x/",
        0,
        &[album(&["Spain"], &["a"]), album(&["Spain"], &["b"])],
        vec![
            item("a", "originals/Spain/a.jpg", None),
            item("b", "originals/Spain/b.jpg", None),
        ],
    );
    assert_eq!(index.albums.len(), 1);
    assert_eq!(index.albums[0].items, ["a", "b"]);
}

#[test]
fn v1_gallery_keeps_the_current_site_working() {
    let mut index = spain();
    // A photo no preview could be made for falls back to the full picture.
    index.items.get_mut("77aa000000000000").unwrap().renditions.clear();
    let v = serde_json::to_value(gallery_v1(&index)).unwrap();
    let albums = v["albums"].as_array().unwrap();
    assert_eq!(albums.len(), 2, "the empty parent album is left out");

    let spain = &albums[1];
    assert_eq!(spain["id"], "holidays-spain-2009");
    assert_eq!(spain["title"], "Holidays / Spain 2009");
    assert_eq!(spain["isPrivate"], false);
    let photos = spain["photos"].as_array().unwrap();
    assert_eq!(photos.len(), 1, "v1 has no videos");
    assert_eq!(photos[0]["filename"], "IMG_1234.jpg");
    assert_eq!(
        photos[0]["url"],
        "https://photos.example.com/originals/Holidays/Spain%202009/Day%202/IMG_1234.jpg"
    );
    assert_eq!(
        photos[0]["thumbnail"],
        "https://photos.example.com/renditions/thumb/9f2c4be1a07d33e8.webp"
    );
    assert_eq!(photos[0]["path"], "originals/Holidays/Spain 2009/Day 2/IMG_1234.jpg");

    let odd = &albums[0]["photos"][0];
    assert_eq!(
        odd["url"],
        "https://photos.example.com/originals/By%20date/2008/2008-08/a%20b%231.jpg"
    );
    assert_eq!(odd["thumbnail"], odd["url"]);
}

#[test]
fn index_names_are_long_random_and_different() {
    let a = index_key([0; 16]);
    let b = index_key([0xFF; 16]);
    let c = index_key(*b"sixteen byte key");
    for key in [&a, &b, &c] {
        assert!(key.starts_with("index/") && key.ends_with(".kvx"), "{key}");
        let name = &key[6..key.len() - 4];
        assert_eq!(name.len(), 22);
        assert!(name.chars().all(|ch| ch.is_ascii_alphanumeric()));
    }
    assert!(a != b && b != c && a != c);
    let one_bit = index_key([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]);
    assert_ne!(a, one_bit, "every bit counts");
}

#[test]
fn rendition_paths_follow_the_rules() {
    assert_eq!(rendition_path(Rendition::Thumb, "abc"), "renditions/thumb/abc.webp");
    assert_eq!(rendition_path(Rendition::Video, "abc"), "renditions/video/abc.mp4");
    assert_eq!(url_encode_path("a/b c/é~.jpg"), "a/b%20c/%C3%A9~.jpg");
}

/// The schema file in the repo is what other tools validate against, so it
/// must always match the code. To update it after a deliberate change:
/// `KEEVORO_UPDATE_SCHEMA=1 cargo test -p keevoro-index`.
#[test]
fn the_published_schema_matches_the_code() {
    let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("schema/keevoro.index.v2.schema.json");
    let current = schema_json();
    if std::env::var_os("KEEVORO_UPDATE_SCHEMA").is_some() {
        std::fs::write(&file, &current).unwrap();
    }
    let saved = std::fs::read_to_string(&file).unwrap_or_default().replace("\r\n", "\n");
    assert_eq!(
        saved, current,
        "schema file is out of date; see the comment on this test"
    );
    assert!(current.contains("\"generatedAt\"") && current.contains("\"blurhash\""));
}
