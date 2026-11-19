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
    assert!(g.failed[0].reason.contains("changed since the source was scanned"));
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
    assert!(g.failed[0].reason.starts_with("Couldn't read it from the source"));
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
    fn previews_are_made_from_the_library_and_named_by_content() {
        let w = World::new();
        w.add("Family/a.jpg", &real_jpeg(10));
        w.add(
            "Family/clip.mp4",
            b"\0\0\0\x18ftypisom\0\0\x02\0isomiso2 not really a video",
        );
        let report = w.scan();
        w.gather(&report);
        // The drive goes back in the drawer; previews do not need it.
        fs::remove_dir_all(w.drive()).unwrap();

        let p = previews(&w, &report);
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
    fn a_deleted_thumbnail_is_made_again() {
        let w = World::new();
        w.add("Family/a.jpg", &real_jpeg(10));
        let report = w.scan();
        w.gather(&report);
        previews(&w, &report);
        fs::remove_dir_all(w.library().join("renditions/thumb")).unwrap();
        assert_eq!(previews(&w, &report).made, 1);
    }

    #[test]
    fn photos_that_cannot_be_previewed_are_remembered_with_a_reason() {
        let w = World::new();
        w.add("Family/odd.jpg", &jpeg(1, 50));
        let report = w.scan();
        w.gather(&report);

        let first = previews(&w, &report);
        assert_eq!((first.made, first.no_preview.len()), (0, 1));
        assert!(first.no_preview[0].path.ends_with("originals/Family/odd.jpg"));
        assert!(!first.no_preview[0].reason.is_empty());

        // Not attempted again, but still reported.
        let mut calls = 0;
        let items: Vec<&FoundItem> = report.items.iter().collect();
        let second = make_previews(&w.library(), &w.catalog, &items, &mut |_, _| calls += 1).unwrap();
        assert_eq!((calls, second.no_preview.len()), (0, 1));
    }

    #[test]
    fn a_file_missing_from_the_library_is_previewed_from_its_source() {
        let w = World::new();
        w.add("Family/a.jpg", &real_jpeg(10));
        let report = w.scan();
        w.gather(&report);
        fs::remove_file(w.library().join("originals/Family/a.jpg")).unwrap();
        // The bytes are the same wherever they are read from.
        assert_eq!(previews(&w, &report).made, 1);
    }

    #[test]
    fn a_file_missing_everywhere_is_retried_next_time_not_written_off() {
        let w = World::new();
        w.add("Family/a.jpg", &real_jpeg(10));
        let report = w.scan();
        w.gather(&report);
        let original = w.library().join("originals/Family/a.jpg");
        let bytes = fs::read(&original).unwrap();
        fs::remove_file(&original).unwrap();
        fs::remove_file(&report.items[0].path).unwrap();

        assert_eq!(previews(&w, &report).no_preview.len(), 1);
        fs::write(&original, bytes).unwrap();
        assert_eq!(previews(&w, &report).made, 1);
    }

    #[test]
    fn files_not_yet_gathered_are_left_alone() {
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

    fn run(w: &World, report: &ScanReport, tools: &VideoTools, convert: bool) -> crate::VideoReport {
        let items: Vec<&FoundItem> = report.items.iter().collect();
        make_video_previews(&w.library(), &w.catalog, &items, Some(tools), convert, &mut |_, _| {}).unwrap()
    }

    fn id_of<'a>(report: &'a ScanReport, name: &str) -> &'a FoundItem {
        report.items.iter().find(|i| i.path.ends_with(name)).unwrap()
    }

    #[test]
    fn every_video_gets_a_poster_and_only_old_ones_are_converted_when_asked() {
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
        w.gather(&report);
        fs::remove_dir_all(w.drive()).unwrap();

        // Conversion off: posters only.
        let first = run(&w, &report, &tools, false);
        assert_eq!(
            (first.posters, first.converted, first.repackaged, first.problems.len()),
            (3, 0, 0, 0)
        );
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

        let again = run(&w, &report, &tools, false);
        assert_eq!((again.posters, again.already_there), (0, 3));

        // Conversion on: the AVI is re-encoded, the MOV repackaged, the
        // modern MP4 left alone.
        let converted = run(&w, &report, &tools, true);
        assert_eq!(
            (converted.posters, converted.converted, converted.repackaged),
            (0, 1, 1)
        );
        assert_eq!((converted.already_there, converted.problems.len()), (1, 0));
        for (name, expect_mp4) in [("old.avi", true), ("camcorder.mov", true), ("phone.mp4", false)] {
            let item = id_of(&report, name);
            let mp4 = w.library().join(format!("renditions/video/{}.mp4", item.id().unwrap()));
            assert_eq!(mp4.is_file(), expect_mp4, "{name}");
        }
        // The originals are all still there, untouched.
        assert_eq!(
            fs::read(w.library().join("originals/Holiday/old.avi")).unwrap().len() as u64,
            old.bytes
        );

        let settled = run(&w, &report, &tools, true);
        assert_eq!(
            (settled.converted, settled.repackaged, settled.already_there),
            (0, 0, 3)
        );
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
        w.gather(&report);

        let first = run(&w, &report, &tools, true);
        assert_eq!((first.posters, first.problems.len()), (0, 1));
        assert!(first.problems[0].path.ends_with("originals/Holiday/fake.mp4"));
        let hash = report.items[0].blake3.as_deref().unwrap();
        assert!(matches!(
            w.catalog.video(hash).unwrap(),
            Some(VideoState::Unreadable { .. })
        ));

        let mut calls = 0;
        let items: Vec<&FoundItem> = report.items.iter().collect();
        let second = make_video_previews(&w.library(), &w.catalog, &items, Some(&tools), true, &mut |_, _| {
            calls += 1
        })
        .unwrap();
        assert_eq!(
            (calls, second.problems.len()),
            (0, 1),
            "still reported, not attempted again"
        );
    }

    #[test]
    fn a_deleted_poster_is_made_again_and_ungathered_videos_are_left_alone() {
        let Some(tools) = tools() else { return };
        let w = World::new();
        w.add(
            "Holiday/old.avi",
            &video(&tools, "old.avi", &["-c:v", "mpeg4", "-c:a", "mp3"]),
        );
        let report = w.scan();
        let before = run(&w, &report, &tools, true);
        assert_eq!((before.posters, before.already_there, before.problems.len()), (0, 0, 0));

        w.gather(&report);
        run(&w, &report, &tools, false);
        fs::remove_dir_all(w.library().join("renditions/poster")).unwrap();
        assert_eq!(run(&w, &report, &tools, false).posters, 1);
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
        w.gather(&report);
        let original = w.library().join("originals/Holiday/old.avi");
        let bytes = fs::read(&original).unwrap();
        fs::remove_file(&original).unwrap();
        fs::remove_file(&report.items[0].path).unwrap();
        assert_eq!(run(&w, &report, &tools, false).problems.len(), 1);
        assert_eq!(
            w.catalog.video(report.items[0].blake3.as_deref().unwrap()).unwrap(),
            None
        );
        fs::write(&original, bytes).unwrap();
        assert_eq!(run(&w, &report, &tools, false).posters, 1);
    }
}

mod verify {
    use super::*;
    use crate::verify_library;

    #[test]
    fn an_untouched_library_is_intact_and_damage_is_found() {
        let w = World::new();
        w.add("Family/a.jpg", &jpeg(1, 500));
        w.add("Family/b.jpg", &jpeg(2, 500));
        w.add("Family/c.jpg", &jpeg(3, 500));
        w.add("Family/d.jpg", &jpeg(4, 500));
        w.gather(&w.scan());

        let mut seen = Vec::new();
        let fine = verify_library(&w.library(), &w.catalog, &mut |d, t| seen.push((d, t))).unwrap();
        assert_eq!((fine.intact, fine.problems.len()), (4, 0));
        assert_eq!(seen.last(), Some(&(4, 4)));

        let lib = w.library().join("originals/Family");
        // One byte flipped, same size: only reading it all can tell.
        let mut rotted = fs::read(lib.join("a.jpg")).unwrap();
        rotted[100] ^= 0xFF;
        fs::write(lib.join("a.jpg"), rotted).unwrap();
        fs::write(lib.join("b.jpg"), b"cut short").unwrap();
        fs::remove_file(lib.join("c.jpg")).unwrap();

        let r = verify_library(&w.library(), &w.catalog, &mut |_, _| {}).unwrap();
        assert_eq!(r.intact, 1);
        let reasons: Vec<(String, &str)> = r
            .problems
            .iter()
            .map(|p| {
                (
                    p.path.file_name().unwrap().to_string_lossy().to_string(),
                    p.reason.as_str(),
                )
            })
            .collect();
        assert_eq!(
            reasons,
            [
                (
                    "a.jpg".to_string(),
                    "Its contents have changed, so it is damaged or was edited"
                ),
                (
                    "b.jpg".to_string(),
                    "It has changed size, so it is damaged or was edited"
                ),
                ("c.jpg".to_string(), "It is missing from the library"),
            ]
        );
        // Checking changes nothing.
        assert_eq!(fs::read(lib.join("b.jpg")).unwrap(), b"cut short");
    }

    #[test]
    fn an_empty_library_has_nothing_to_check() {
        let w = World::new();
        let r = verify_library(&w.library(), &w.catalog, &mut |_, _| {}).unwrap();
        assert_eq!((r.intact, r.problems.len()), (0, 0));
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
    assert_eq!(agree_places(&w.catalog, &items, false).unwrap(), 2);
    assert_eq!(
        agree_places(&w.catalog, &items, false).unwrap(),
        0,
        "a place is agreed once"
    );
    let places: Vec<_> = w.catalog.collected().unwrap();
    assert!(places.iter().all(|p| !p.here), "nothing was copied here");
    assert!(!w.library().join("originals").exists());
    let mut names: Vec<_> = places.iter().map(|p| p.path.clone()).collect();
    names.dedup();
    assert_eq!(names.len(), 2);

    // Gathering later puts each file at the place already agreed.
    w.gather(&report);
    for p in w.catalog.collected().unwrap() {
        assert!(p.here);
        assert!(names.contains(&p.path));
        assert!(in_library(&w.library(), &p.path).is_file());
    }
}

#[test]
fn checking_the_library_ignores_files_that_were_never_meant_to_be_in_it() {
    let w = World::new();
    w.add("Trip/IMG_0001.jpg", &real_jpeg(10));
    let report = w.scan();
    let items: Vec<&FoundItem> = report.items.iter().collect();
    agree_places(&w.catalog, &items, false).unwrap();
    let checked = verify_library(&w.library(), &w.catalog, &mut |_, _| {}).unwrap();
    assert_eq!((checked.intact, checked.problems.len()), (0, 0));
}
