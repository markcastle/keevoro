use super::*;
use keevoro_scan::{scan, ScanOptions, ScanReport};
use std::fs;
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
    /// The backup's own folder, where previews are kept until they are sent.
    fn library(&self) -> PathBuf {
        self.dir.path().join("backup")
    }
    fn add(&self, rel: &str, bytes: &[u8]) {
        let p = self.drive().join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, bytes).unwrap();
    }
    fn scan(&self) -> ScanReport {
        scan(&self.drive(), &ScanOptions::default(), &mut |_| {})
    }
    /// Gives every file found its place, as a backup does before sending.
    fn place(&self, report: &ScanReport) {
        let items: Vec<&FoundItem> = report.items.iter().collect();
        agree_places(&self.catalog, &items, false, &mut |_, _| {}).unwrap();
    }
}

#[test]
fn free_space_is_reported_for_a_folder_that_does_not_exist_yet() {
    let dir = tempfile::tempdir().unwrap();
    assert!(free_space(&dir.path().join("not/made/yet")).is_some_and(|b| b > 0));
}

mod previews {
    use super::*;
    use crate::make_previews;
    use keevoro_catalog::RenditionState;

    pub(super) fn real_jpeg(shade: u8) -> Vec<u8> {
        let img = image::RgbImage::from_pixel(900, 600, image::Rgb([shade, 120, 60]));
        let mut bytes = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Jpeg)
            .unwrap();
        bytes
    }

    fn previews(w: &World, report: &ScanReport) -> crate::PreviewReport {
        let items: Vec<&FoundItem> = report.items.iter().collect();
        make_previews(&w.library(), &w.catalog, &items, &mut |_, _| {}).unwrap()
    }

    #[test]
    fn previews_are_made_from_the_source_and_named_by_content() {
        let w = World::new();
        w.add("Family/a.jpg", &real_jpeg(10));
        w.add(
            "Family/clip.mp4",
            b"\0\0\0\x18ftypisom\0\0\x02\0isomiso2 not really a video",
        );
        let report = w.scan();
        w.place(&report);
        let before = fs::read(w.drive().join("Family/a.jpg")).unwrap();

        let p = previews(&w, &report);
        assert_eq!(
            fs::read(w.drive().join("Family/a.jpg")).unwrap(),
            before,
            "the source is only read"
        );
        assert_eq!(
            (p.made, p.already_there, p.no_preview.len()),
            (1, 0, 0),
            "videos are not photos"
        );
        let photo = report.items.iter().find(|i| i.path.ends_with("a.jpg")).unwrap();
        let id = photo.id().unwrap();
        assert!(w.library().join(format!("renditions/thumb/{id}.webp")).is_file());
        assert!(w.library().join(format!("renditions/display/{id}.webp")).is_file());
        let state = w.catalog.rendition(photo.blake3.as_deref().unwrap()).unwrap();
        assert!(
            matches!(
                state,
                Some(RenditionState::Made {
                    width: 900,
                    height: 600,
                    ..
                })
            ),
            "{state:?}"
        );

        let again = previews(&w, &report);
        assert_eq!((again.made, again.already_there), (0, 1));
    }

    #[test]
    fn previews_made_at_a_pace_wait_their_turn_and_hear_a_stop_while_waiting() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let w = World::new();
        for (n, shade) in [10u8, 60, 110].iter().enumerate() {
            w.add(&format!("Family/{n}.jpg"), &real_jpeg(*shade));
        }
        let report = w.scan();
        w.place(&report);
        let items: Vec<&FoundItem> = report.items.iter().collect();
        assert_eq!(crate::previews_to_make(&w.library(), &w.catalog, &items).unwrap(), 3);

        // Nothing allowed: nothing is made, and a stop is heard while waiting.
        let started = AtomicUsize::new(0);
        let asked = AtomicUsize::new(0);
        let nothing = crate::Pace {
            allowed: &|| 0,
            on_worker: &|| {
                started.fetch_add(1, Ordering::SeqCst);
            },
        };
        let p = crate::make_previews_paced(
            &w.library(),
            &w.catalog,
            &items,
            &mut |_, _| {},
            &|| asked.fetch_add(1, Ordering::SeqCst) < 3,
            nothing,
        )
        .unwrap();
        assert_eq!(p.made, 0);
        assert!(started.load(Ordering::SeqCst) >= 1, "each worker was told it started");
        assert_eq!(crate::previews_to_make(&w.library(), &w.catalog, &items).unwrap(), 3);

        // One at a time: all of them, in the end.
        let one = crate::Pace {
            allowed: &|| 1,
            on_worker: &|| {},
        };
        let p = crate::make_previews_paced(&w.library(), &w.catalog, &items, &mut |_, _| {}, &|| true, one).unwrap();
        assert_eq!(p.made, 3);
        assert_eq!(crate::previews_to_make(&w.library(), &w.catalog, &items).unwrap(), 0);
    }

    #[test]
    fn a_deleted_thumbnail_is_made_again() {
        let w = World::new();
        w.add("Family/a.jpg", &real_jpeg(10));
        let report = w.scan();
        w.place(&report);
        previews(&w, &report);
        fs::remove_dir_all(w.library().join("renditions/thumb")).unwrap();
        assert_eq!(previews(&w, &report).made, 1);
    }

    #[test]
    fn photos_that_cannot_be_previewed_are_remembered_with_a_reason() {
        let w = World::new();
        w.add("Family/odd.jpg", &jpeg(1, 50));
        let report = w.scan();
        w.place(&report);

        let first = previews(&w, &report);
        assert_eq!((first.made, first.no_preview.len()), (0, 1));
        assert!(first.no_preview[0].path.ends_with("Family/odd.jpg"));
        assert!(!first.no_preview[0].reason.is_empty());

        // Not attempted again, but still reported.
        let mut calls = 0;
        let items: Vec<&FoundItem> = report.items.iter().collect();
        let second = make_previews(&w.library(), &w.catalog, &items, &mut |_, _| calls += 1).unwrap();
        assert_eq!((calls, second.no_preview.len()), (0, 1));
    }

    #[test]
    fn a_file_missing_from_its_source_is_retried_next_time_not_written_off() {
        let w = World::new();
        w.add("Family/a.jpg", &real_jpeg(10));
        let report = w.scan();
        w.place(&report);
        let original = report.items[0].path.clone();
        let bytes = fs::read(&original).unwrap();
        fs::remove_file(&original).unwrap();

        assert_eq!(previews(&w, &report).no_preview.len(), 1);
        fs::write(&original, bytes).unwrap();
        assert_eq!(previews(&w, &report).made, 1);
    }

    #[test]
    fn files_without_a_place_are_left_alone() {
        let w = World::new();
        w.add("Family/a.jpg", &real_jpeg(10));
        let report = w.scan();
        let p = previews(&w, &report);
        assert_eq!((p.made, p.already_there, p.no_preview.len()), (0, 0, 0));
        assert!(!w.library().join("renditions").exists());
    }
}

mod videos {
    use super::*;
    use crate::make_video_previews;
    use keevoro_catalog::VideoState;
    use keevoro_media::VideoTools;
    use std::process::Command;

    fn tools() -> Option<VideoTools> {
        let found = VideoTools::find();
        if found.is_none() {
            eprintln!("skipped: ffmpeg is not installed");
        }
        found
    }

    /// A one-second test video in the given format.
    fn video(tools: &VideoTools, name: &str, codec_args: &[&str]) -> Vec<u8> {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join(name);
        let ok = Command::new(&tools.ffmpeg)
            .args([
                "-v",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
                "testsrc=duration=1:size=320x240:rate=25",
            ])
            .args(["-f", "lavfi", "-i", "sine=frequency=440:duration=1"])
            .args(codec_args)
            .arg(&out)
            .status()
            .unwrap()
            .success();
        assert!(ok);
        fs::read(out).unwrap()
    }

    fn run(w: &World, report: &ScanReport, tools: &VideoTools) -> crate::VideoReport {
        let items: Vec<&FoundItem> = report.items.iter().collect();
        make_video_previews(&w.library(), &w.catalog, &items, Some(tools), &mut |_, _| {}).unwrap()
    }

    fn id_of<'a>(report: &'a ScanReport, name: &str) -> &'a FoundItem {
        report.items.iter().find(|i| i.path.ends_with(name)).unwrap()
    }

    #[test]
    fn every_video_gets_a_poster_made_from_its_source() {
        let Some(tools) = tools() else { return };
        let w = World::new();
        w.add(
            "Holiday/old.avi",
            &video(&tools, "old.avi", &["-c:v", "mpeg4", "-c:a", "mp3"]),
        );
        w.add(
            "Holiday/camcorder.mov",
            &video(
                &tools,
                "c.mov",
                &["-c:v", "libx264", "-pix_fmt", "yuv420p", "-c:a", "aac"],
            ),
        );
        w.add(
            "Holiday/phone.mp4",
            &video(
                &tools,
                "p.mp4",
                &["-c:v", "libx264", "-pix_fmt", "yuv420p", "-c:a", "aac"],
            ),
        );
        w.add("Holiday/photo.jpg", &jpeg(1, 100));
        let report = w.scan();
        w.place(&report);
        let before = fs::read(w.drive().join("Holiday/old.avi")).unwrap();

        let first = run(&w, &report, &tools);
        assert_eq!((first.posters, first.problems.len()), (3, 0));
        assert!(!w.library().join("renditions/video").exists());
        let old = id_of(&report, "old.avi");
        let state = w.catalog.video(old.blake3.as_deref().unwrap()).unwrap().unwrap();
        assert!(
            matches!(
                state,
                VideoState::Known {
                    width: 320,
                    height: 240,
                    poster: true,
                    mp4: false,
                    ..
                }
            ),
            "{state:?}"
        );
        assert!(w
            .library()
            .join(format!("renditions/poster/{}.webp", old.id().unwrap()))
            .is_file());

        let again = run(&w, &report, &tools);
        assert_eq!((again.posters, again.already_there), (0, 3));

        // The source is only read.
        assert_eq!(fs::read(w.drive().join("Holiday/old.avi")).unwrap(), before);
    }

    #[test]
    fn a_file_that_only_claims_to_be_a_video_is_remembered_not_retried() {
        let Some(tools) = tools() else { return };
        let w = World::new();
        // Looks like an MP4 to the scanner, but holds no video.
        w.add(
            "Holiday/fake.mp4",
            b"\0\0\0\x18ftypisom\0\0\x02\0isomiso2 nothing else here at all",
        );
        let report = w.scan();
        w.place(&report);

        let first = run(&w, &report, &tools);
        assert_eq!((first.posters, first.problems.len()), (0, 1));
        assert!(first.problems[0].path.ends_with("Holiday/fake.mp4"));
        let hash = report.items[0].blake3.as_deref().unwrap();
        assert!(matches!(
            w.catalog.video(hash).unwrap(),
            Some(VideoState::Unreadable { .. })
        ));

        let mut calls = 0;
        let items: Vec<&FoundItem> = report.items.iter().collect();
        let second =
            make_video_previews(&w.library(), &w.catalog, &items, Some(&tools), &mut |_, _| calls += 1).unwrap();
        assert_eq!(
            (calls, second.problems.len()),
            (0, 1),
            "still reported, not attempted again"
        );
    }

    #[test]
    fn a_deleted_poster_is_made_again_and_videos_without_a_place_are_left_alone() {
        let Some(tools) = tools() else { return };
        let w = World::new();
        w.add(
            "Holiday/old.avi",
            &video(&tools, "old.avi", &["-c:v", "mpeg4", "-c:a", "mp3"]),
        );
        let report = w.scan();
        let before = run(&w, &report, &tools);
        assert_eq!((before.posters, before.already_there, before.problems.len()), (0, 0, 0));

        w.place(&report);
        run(&w, &report, &tools);
        fs::remove_dir_all(w.library().join("renditions/poster")).unwrap();
        assert_eq!(run(&w, &report, &tools).posters, 1);
    }

    #[test]
    fn a_video_missing_everywhere_is_retried_next_time() {
        let Some(tools) = tools() else { return };
        let w = World::new();
        w.add(
            "Holiday/old.avi",
            &video(&tools, "old.avi", &["-c:v", "mpeg4", "-c:a", "mp3"]),
        );
        let report = w.scan();
        w.place(&report);
        let original = report.items[0].path.clone();
        let bytes = fs::read(&original).unwrap();
        fs::remove_file(&original).unwrap();
        assert_eq!(run(&w, &report, &tools).problems.len(), 1);
        assert_eq!(
            w.catalog.video(report.items[0].blake3.as_deref().unwrap()).unwrap(),
            None
        );
        fs::write(&original, bytes).unwrap();
        assert_eq!(run(&w, &report, &tools).posters, 1);
    }
}

fn real_jpeg(shade: u8) -> Vec<u8> {
    previews::real_jpeg(shade)
}

#[test]
fn places_are_agreed_without_copying_and_two_files_never_share_one() {
    let w = World::new();
    w.add("Trip/IMG_0001.jpg", &real_jpeg(10));
    w.add("Trip/sub/IMG_0002.jpg", &real_jpeg(20));
    let report = w.scan();
    let items: Vec<&FoundItem> = report.items.iter().collect();
    assert_eq!(agree_places(&w.catalog, &items, false, &mut |_, _| {}).unwrap(), 2);
    assert_eq!(
        agree_places(&w.catalog, &items, false, &mut |_, _| {}).unwrap(),
        0,
        "a place is agreed once"
    );
    let places: Vec<_> = w.catalog.collected().unwrap();
    assert!(!w.library().exists(), "nothing was copied anywhere");
    let mut names: Vec<_> = places.iter().map(|p| p.path.clone()).collect();
    names.dedup();
    assert_eq!(names.len(), 2);
}

#[test]
fn two_different_files_with_one_name_in_one_place_are_told_apart() {
    let w = World::new();
    // Undated, so both belong in the same folder under the same name.
    w.add("a/IMG_0001.jpg", &jpeg(1, 100));
    w.add("b/IMG_0001.jpg", &jpeg(2, 100));
    let report = w.scan();
    w.place(&report);
    let mut names: Vec<String> = w.catalog.collected().unwrap().into_iter().map(|p| p.path).collect();
    names.sort();
    assert_eq!(names.len(), 2);
    assert_ne!(names[0], names[1]);
    assert!(names.iter().any(|n| n.ends_with("IMG_0001 (2).jpg")), "{names:?}");
}

#[test]
fn a_folder_spelt_another_way_goes_into_the_first_spelling_and_a_name_differing_by_case_is_numbered() {
    // Folders on this computer's disk may ignore case themselves, so the
    // files are found in folders of their own and given their albums after.
    let w = World::new();
    w.add("one/IMG_0001.jpg", &jpeg(1, 100));
    w.add("two/img_0001.JPG", &jpeg(2, 100));
    w.add("three/b.jpg", &jpeg(3, 100));
    let mut report = w.scan();
    report.items.sort_by(|a, b| a.path.cmp(&b.path));
    let albums: [&[&str]; 3] = [&["Smith"], &["SMITH", "Holiday"], &["SMITH"]];
    for (item, album) in report.items.iter_mut().zip(albums) {
        item.folder.album = album.iter().map(|a| (*a).to_string()).collect();
    }
    // Placed one at a time, the way two backups a week apart would.
    for item in &report.items {
        agree_places(&w.catalog, &[item], false, &mut |_, _| {}).unwrap();
    }
    let mut names: Vec<String> = w.catalog.collected().unwrap().into_iter().map(|p| p.path).collect();
    names.sort();
    assert!(
        names.iter().all(|n| n.starts_with("originals/Smith/")),
        "one folder, spelt as it was first: {names:?}"
    );
    assert!(
        names.iter().any(|n| n.ends_with(" (2).jpg") || n.ends_with(" (2).JPG")),
        "{names:?}"
    );
    assert!(names.iter().any(|n| n.ends_with("/Holiday/b.jpg")), "{names:?}");
}

#[test]
fn a_place_too_long_for_a_destination_is_cut_to_fit() {
    // Too long for any disk to hold here, so the file is found under a
    // short name and its folders and name are made long afterwards, as a
    // share from another computer could give them.
    let w = World::new();
    w.add("a.jpg", &jpeg(1, 100));
    let mut report = w.scan();
    let item = &mut report.items[0];
    item.folder.album = (0..12).map(|i| format!("{i} {}", "folder name ".repeat(10))).collect();
    item.path = item.path.with_file_name(format!("{}.jpg", "n".repeat(300)));
    w.place(&report);
    let place = &w.catalog.collected().unwrap()[0].path;
    assert!(place.len() <= keevoro_core::layout::LONGEST_PLACE, "{}", place.len());
    assert!(place
        .split('/')
        .all(|part| part.len() <= keevoro_core::layout::LONGEST_NAME));
    assert!(place.ends_with(".jpg"));
}

/// Placing is the part of getting ready that touches every kept file, so
/// how long it takes for a big backup is measured here, by hand:
/// `cargo test -p keevoro-collect how_long_placing -- --ignored --nocapture`.
#[test]
#[ignore = "a measurement, run by hand"]
fn how_long_placing_100_000_files_takes() {
    let w = World::new();
    w.add("a.jpg", &jpeg(1, 100));
    let template = w.scan().items.remove(0);
    let items: Vec<FoundItem> = (0..100_000u32)
        .map(|i| {
            let mut item = template.clone();
            item.blake3 = Some(format!("{i:064x}"));
            item.path = item.path.with_file_name(format!("IMG_{:04}.jpg", i % 5000));
            item.folder.album = vec![format!("Family {}", i % 40), format!("Box {}", i % 7)];
            item
        })
        .collect();
    let refs: Vec<&FoundItem> = items.iter().collect();
    let started = std::time::Instant::now();
    agree_places(&w.catalog, &refs, true, &mut |_, _| {}).unwrap();
    println!("placed 100,000 files in {:?}", started.elapsed());
    assert_eq!(w.catalog.collected().unwrap().len(), 100_000);
}
