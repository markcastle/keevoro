//! The slow, careful half of looking through a source.
//!
//! A search is quick because it only samples large files (see
//! `keevoro_scan::ScanOptions::quick`). That is enough to show what was
//! found and to guess at duplicates, but not enough to copy anything: a
//! copy is proved exact against the fingerprint of the whole file, and two
//! files are only duplicates for certain when those fingerprints match.
//!
//! This module reads the files a search only sampled and records their
//! whole-file fingerprints. It runs in the background after a search, and
//! a backup runs it first for files that look alike, so nothing is ever
//! left out as a duplicate on the strength of a sample.

use anyhow::Result;
use keevoro_catalog::Catalog;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// What a pass over the sampled files did.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct DeepReport {
    /// Files whose whole fingerprint is now known.
    pub checked: usize,
    /// Files that could not be read, and why. They stay unchecked until a
    /// later pass manages to read them.
    pub failed: Vec<(PathBuf, String)>,
    /// True when the pass was asked to stop before it had finished.
    pub stopped: bool,
    /// Sources that were not there (not plugged in, or unplugged part-way
    /// through), each with how many of its files were left unchecked. Their
    /// files are not tried one by one: they stay as they were, known by
    /// their sample, for a check once the source is back.
    pub gone: Vec<Gone>,
}

/// A source the check could not reach, and what it left of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Gone {
    /// The source's root, as the catalog keeps it.
    pub source: PathBuf,
    /// Its files still known only by their sample.
    pub left: usize,
}

/// How far a pass has got.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeepProgress<'a> {
    pub done: usize,
    pub total: usize,
    /// The file about to be read, and its size.
    pub reading: &'a Path,
    pub bytes: u64,
}

/// Reads every file that has only a quick fingerprint and records its
/// whole one. `only` limits the pass to those quick fingerprints (files
/// about to be backed up that look alike, say); `None` means all of them. `keep_going` is
/// asked before each file, so a pass can be stopped between files.
///
/// `cache` is Keevoro's cache folder, when pictures made while looking
/// inside a folder may be there: a file's pictures are named by its quick
/// fingerprint, so they take its whole one too, and are not made twice.
/// Only a file that alone went by its quick fingerprint has them renamed;
/// files that looked alike may have shared one picture, which is then
/// left to be made afresh for each.
pub fn deep_look(
    catalog: &Catalog,
    cache: Option<&Path>,
    only: Option<&HashSet<String>>,
    on_progress: &mut dyn FnMut(DeepProgress<'_>),
    keep_going: &dyn Fn() -> bool,
) -> Result<DeepReport> {
    deep_look_where(catalog, cache, only, &|_| true, on_progress, keep_going)
}

/// [`deep_look`], reading only the files `may_read` allows. The rest are
/// left as they are, with their quick fingerprint, for a check that may
/// read them. It is how a check that follows an AI agent's search keeps to
/// the folders that agent may read.
pub fn deep_look_where(
    catalog: &Catalog,
    cache: Option<&Path>,
    only: Option<&HashSet<String>>,
    may_read: &dyn Fn(&Path) -> bool,
    on_progress: &mut dyn FnMut(DeepProgress<'_>),
    keep_going: &dyn Fn() -> bool,
) -> Result<DeepReport> {
    deep_look_unless_gone(
        catalog,
        cache,
        only,
        may_read,
        &crate::reach::is_there,
        on_progress,
        keep_going,
    )
}

/// [`deep_look_where`], asking `is_there` whether each source is plugged
/// in: once for each source before reading anything, and again only when a
/// file of it could not be read. A source that is not there ends the work
/// on it at once; its files are never tried one by one, which on a drive
/// pulled out or a share gone can take a long while each.
fn deep_look_unless_gone(
    catalog: &Catalog,
    cache: Option<&Path>,
    only: Option<&HashSet<String>>,
    may_read: &dyn Fn(&Path) -> bool,
    is_there: &dyn Fn(&Path) -> bool,
    on_progress: &mut dyn FnMut(DeepProgress<'_>),
    keep_going: &dyn Fn() -> bool,
) -> Result<DeepReport> {
    let _busy = keevoro_core::doing::doing(keevoro_core::doing::Work::CheckingForDuplicates);
    let unhashed = catalog.unhashed()?;
    let mut sharing = std::collections::HashMap::<&str, usize>::new();
    for file in &unhashed {
        *sharing.entry(file.quick.as_str()).or_default() += 1;
    }
    let alone: HashSet<String> = sharing
        .into_iter()
        .filter(|(_, files)| *files == 1)
        .map(|(quick, _)| quick.to_string())
        .collect();
    let wanted: Vec<_> = unhashed
        .iter()
        .filter(|file| only.is_none_or(|wanted| wanted.contains(&file.quick)))
        .filter(|file| may_read(&file.path))
        .collect();
    let mut report = DeepReport::default();
    // One look at each source, not one per file.
    let mut sources: Vec<&Path> = wanted.iter().map(|f| f.source.as_path()).collect();
    sources.sort();
    sources.dedup();
    let mut gone: HashSet<&Path> = sources.into_iter().filter(|root| !is_there(root)).collect();
    let mut left_on = std::collections::BTreeMap::<&Path, usize>::new();
    for file in &wanted {
        if gone.contains(file.source.as_path()) {
            *left_on.entry(file.source.as_path()).or_default() += 1;
        }
    }
    let waiting: Vec<_> = wanted
        .into_iter()
        .filter(|file| !gone.contains(file.source.as_path()))
        .collect();
    let mut total = waiting.len();
    let mut done = 0;
    for (at, file) in waiting.iter().enumerate() {
        if gone.contains(file.source.as_path()) {
            continue;
        }
        // Asked to stop while Keevoro is paused, it stops at once and leaves
        // the pause as it was for everything else.
        keevoro_core::pause::wait_while_paused_unless(&|| !keep_going());
        if !keep_going() {
            report.stopped = true;
            break;
        }
        on_progress(DeepProgress {
            done,
            total,
            reading: &file.path,
            bytes: file.bytes,
        });
        done += 1;
        match keevoro_scan::whole_hash(&file.path) {
            Ok(whole) => {
                catalog.set_whole_hash(&file.path, &file.quick, &whole)?;
                if let (Some(cache), true) = (cache, alone.contains(&file.quick)) {
                    crate::look::now_known_as(cache, &file.quick, &whole);
                }
                report.checked += 1;
            }
            // Its source went part-way through: this file and the rest of
            // that source are left as they were, and none is tried again.
            Err(_) if !is_there(&file.source) => {
                let source = file.source.as_path();
                gone.insert(source);
                let rest = waiting[at + 1..].iter().filter(|f| f.source == source).count();
                *left_on.entry(source).or_default() += rest + 1;
                total -= rest + 1;
                done -= 1;
            }
            Err(e) => report.failed.push((file.path.clone(), e.to_string())),
        }
    }
    report.gone = left_on
        .into_iter()
        .map(|(source, left)| Gone {
            source: source.to_path_buf(),
            left,
        })
        .collect();
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{discover, scan_into_with, set_collection_kept, wanted};
    use keevoro_scan::ScanOptions;
    use std::fs;

    /// A JPEG header followed by `len` bytes that depend on `seed`.
    fn big_photo(len: usize, seed: u8) -> Vec<u8> {
        let mut bytes = crate::discover::tests::real_jpeg(seed);
        let mut next = seed;
        bytes.resize_with(bytes.len() + len, || {
            next = next.wrapping_mul(31).wrapping_add(7);
            next
        });
        bytes
    }

    /// A source searched quickly: two large photos that differ only between
    /// the samples, a true copy of the first, and a small photo.
    fn world() -> (tempfile::TempDir, PathBuf, PathBuf, Catalog) {
        let dir = tempfile::tempdir().unwrap();
        let drive = dir.path().join("Card");
        let original = big_photo(2_000_000, 1);
        let mut nearly = original.clone();
        nearly[700_000] ^= 1;
        for (rel, bytes) in [
            ("Trip/IMG_20100101_100000.jpg", original.clone()),
            ("Trip/IMG_20100101_100001.jpg", nearly),
            ("Backup/IMG_20100101_100000.jpg", original),
            ("Trip/IMG_20100102_090000.jpg", crate::discover::tests::real_jpeg(40)),
        ] {
            let p = drive.join(rel);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, bytes).unwrap();
        }
        let library = dir.path().join("Keevoro Library");
        let mut catalog = Catalog::open_in_library(&library).unwrap();
        let quick = ScanOptions {
            quick: true,
            ..ScanOptions::default()
        };
        scan_into_with(&mut catalog, &drive, Some("Card"), &quick, &mut |_| {}, &mut || {}).unwrap();
        (dir, drive, library, catalog)
    }

    #[test]
    fn a_quick_search_leaves_large_files_to_be_checked() {
        let (_dir, _drive, library, catalog) = world();
        assert_eq!(catalog.unhashed().unwrap().len(), 3);
        // The sample cannot tell the nearly-identical photo apart, so for
        // now all three large files look like one.
        assert_eq!(discover(&library, &catalog).unwrap().photos, 2);
    }

    #[test]
    fn checking_tells_true_copies_from_lookalikes() {
        let (_dir, _drive, library, catalog) = world();
        let mut seen = Vec::new();
        let report = deep_look(&catalog, None, None, &mut |p| seen.push((p.done, p.total)), &|| true).unwrap();
        assert_eq!((report.checked, report.failed.len(), report.stopped), (3, 0, false));
        assert_eq!(seen, [(0, 3), (1, 3), (2, 3)]);
        assert!(catalog.unhashed().unwrap().is_empty());

        let found = discover(&library, &catalog).unwrap();
        assert_eq!(found.photos, 3, "the lookalike is its own photo");
        assert_eq!(found.duplicates, 1, "and the backup copy is a true duplicate");
    }

    #[test]
    fn a_check_kept_to_some_folders_reads_nothing_outside_them() {
        let (_dir, drive, _library, catalog) = world();
        let backup = std::fs::canonicalize(drive.join("Backup")).unwrap();
        let mut read = Vec::new();
        let report = deep_look_where(
            &catalog,
            None,
            None,
            &|path| path.starts_with(&backup),
            &mut |p| read.push(p.reading.to_path_buf()),
            &|| true,
        )
        .unwrap();
        assert_eq!(report.checked, 1);
        assert_eq!(read, [backup.join("IMG_20100101_100000.jpg")]);
        let left: Vec<PathBuf> = catalog.unhashed().unwrap().into_iter().map(|f| f.path).collect();
        assert_eq!(left.len(), 2, "left for a check that may read them: {left:?}");
        assert!(left.iter().all(|path| !path.starts_with(&backup)));
    }

    #[test]
    fn a_picture_made_while_looking_is_found_after_the_check_without_being_made_again() {
        let (dir, drive, library, catalog) = world();
        let cache = dir.path().join("cache");
        // The lone large photo: pictured while looking, then checked.
        fs::write(drive.join("Trip/IMG_20100103_090000.jpg"), big_photo(2_000_000, 9)).unwrap();
        let mut catalog = catalog;
        let quick = ScanOptions {
            quick: true,
            ..ScanOptions::default()
        };
        scan_into_with(&mut catalog, &drive, Some("Card"), &quick, &mut |_| {}, &mut || {}).unwrap();
        let lone = std::fs::canonicalize(drive.join("Trip/IMG_20100103_090000.jpg")).unwrap();
        let lookalike = std::fs::canonicalize(drive.join("Trip/IMG_20100101_100001.jpg")).unwrap();
        for file in [&lone, &lookalike] {
            crate::look::picture_of_file(
                &cache,
                file,
                keevoro_core::MediaKind::Photo,
                crate::Look::Small,
                None,
                &keevoro_scan::ByAttributes,
                None,
            )
            .unwrap();
        }
        let pictures = || -> std::collections::BTreeSet<_> {
            fs::read_dir(&cache)
                .unwrap()
                .filter_map(Result::ok)
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| n.ends_with(".webp"))
                .collect()
        };
        let before = pictures();

        deep_look(&catalog, Some(&cache), None, &mut |_| {}, &|| true).unwrap();
        let whole = keevoro_scan::whole_hash(&lone).unwrap();
        let quick_of_lone = keevoro_scan::fingerprint(&lone, true).unwrap();
        let after = pictures();
        assert!(after.contains(&format!("{}.webp", &whole[..16])), "{after:?}");
        assert!(!after.contains(&format!("{}.webp", &quick_of_lone[..16])));
        assert_eq!(after.len(), before.len(), "renamed, not copied");

        // The Discover grid's picture of it is the one made while looking.
        let id = catalog
            .load_reports()
            .unwrap()
            .iter()
            .flat_map(|r| r.items.clone())
            .find(|i| i.path.ends_with("IMG_20100103_090000.jpg"))
            .and_then(|i| i.id().map(str::to_string))
            .unwrap();
        let renamed = cache.join(format!("{}.webp", &whole[..16]));
        let made = fs::metadata(&renamed).unwrap().modified().unwrap();
        crate::preview(&library, &catalog, &cache, &id, None).unwrap();
        assert_eq!(pictures().len(), after.len(), "nothing was made twice");
        assert!(fs::metadata(&renamed).unwrap().modified().unwrap() >= made);
        // The lookalikes shared a quick fingerprint, so theirs is left.
        let shared = keevoro_scan::fingerprint(&lookalike, true).unwrap();
        assert!(after.contains(&format!("{}.webp", &shared[..16])));
    }

    #[test]
    fn a_choice_made_before_checking_still_holds_afterwards() {
        let (_dir, _drive, library, catalog) = world();
        let key = discover(&library, &catalog).unwrap().collections[0].key.clone();
        set_collection_kept(&catalog, &key, false).unwrap();
        deep_look(&catalog, None, None, &mut |_| {}, &|| true).unwrap();
        for report in catalog.load_reports().unwrap() {
            for item in &report.items {
                assert!(!wanted(&catalog, item).unwrap(), "{} was left out", item.path.display());
            }
        }
    }

    #[test]
    fn a_pass_can_be_stopped_and_picked_up_again() {
        let (_dir, _drive, _library, catalog) = world();
        let asked = std::cell::Cell::new(0);
        let report = deep_look(&catalog, None, None, &mut |_| {}, &|| {
            asked.set(asked.get() + 1);
            asked.get() <= 1
        })
        .unwrap();
        assert_eq!((report.checked, report.stopped), (1, true));
        let rest = deep_look(&catalog, None, None, &mut |_| {}, &|| true).unwrap();
        assert_eq!((rest.checked, rest.stopped), (2, false));
    }

    #[test]
    fn a_file_that_has_gone_is_reported_and_left_unchecked() {
        let (_dir, drive, _library, catalog) = world();
        fs::remove_file(drive.join("Backup/IMG_20100101_100000.jpg")).unwrap();
        let report = deep_look(&catalog, None, None, &mut |_| {}, &|| true).unwrap();
        assert_eq!((report.checked, report.failed.len()), (2, 1));
        assert_eq!(catalog.unhashed().unwrap().len(), 1);
    }

    #[test]
    fn a_pass_can_be_limited_to_some_files() {
        let (_dir, _drive, _library, catalog) = world();
        let none = HashSet::new();
        assert_eq!(
            deep_look(&catalog, None, Some(&none), &mut |_| {}, &|| true)
                .unwrap()
                .checked,
            0
        );
        let all: HashSet<String> = catalog.unhashed().unwrap().into_iter().map(|f| f.quick).collect();
        assert_eq!(
            deep_look(&catalog, None, Some(&all), &mut |_| {}, &|| true)
                .unwrap()
                .checked,
            3
        );
    }

    #[test]
    fn getting_ready_after_a_quick_search_tells_lookalikes_apart_and_places_every_different_photo() {
        let (_dir, drive, library, catalog) = world();
        let mut steps = Vec::new();
        let waiting = crate::check_and_place(&library, &catalog, &|| true, &mut |s| steps.push(s)).unwrap();
        assert_eq!(waiting, 0);
        assert!(steps.contains(&crate::ReadyStep::Checking { done: 0, total: 3 }));
        assert!(catalog.unhashed().unwrap().is_empty());
        // The original, the lookalike and the small photo each have their
        // own place; the backup copy is a true duplicate and has none.
        let placed = catalog.collected().unwrap();
        assert_eq!(placed.len(), 3);
        for file in &placed {
            assert!(!file.path.contains("Backup"), "{}", file.path);
        }
        assert!(
            drive.join("Backup/IMG_20100101_100000.jpg").is_file(),
            "the source is only read"
        );
    }

    #[test]
    fn a_copy_found_after_its_original_was_checked_is_still_told_apart_before_a_backup() {
        // The check read the first card in full, then a second card with a
        // copy of one of its photos was searched, and the check for it was
        // stopped (or gave way to the backup). The copy shares no quick
        // fingerprint with anything else still unchecked, only with a file
        // already read in full.
        let (dir, drive, library, mut catalog) = world();
        deep_look(&catalog, None, None, &mut |_| {}, &|| true).unwrap();
        let second = dir.path().join("Card 2");
        fs::create_dir_all(&second).unwrap();
        fs::copy(drive.join("Trip/IMG_20100101_100000.jpg"), second.join("IMG_0001.jpg")).unwrap();
        let quick = ScanOptions {
            quick: true,
            ..ScanOptions::default()
        };
        scan_into_with(&mut catalog, &second, Some("Card 2"), &quick, &mut |_| {}, &mut || {}).unwrap();
        assert_eq!(catalog.unhashed_count().unwrap(), 1);
        let waiting = crate::check_and_place(&library, &catalog, &|| true, &mut |_| {}).unwrap();
        assert_eq!(waiting, 0);
        assert_eq!(catalog.unhashed_count().unwrap(), 0, "the copy was read in full first");
        assert_eq!(
            catalog.collected().unwrap().len(),
            3,
            "and, being a true copy, it is not given a place of its own, so it is never sent twice"
        );
    }

    /// The world, with a second card of two large photos searched after the
    /// first, and its root.
    fn two_cards() -> (tempfile::TempDir, PathBuf, PathBuf, Catalog) {
        let (dir, card, _library, mut catalog) = world();
        let second = dir.path().join("Card 2");
        fs::create_dir_all(&second).unwrap();
        fs::write(second.join("IMG_0001.jpg"), big_photo(2_000_000, 21)).unwrap();
        fs::write(second.join("IMG_0002.jpg"), big_photo(2_000_000, 22)).unwrap();
        let quick = ScanOptions {
            quick: true,
            ..ScanOptions::default()
        };
        scan_into_with(&mut catalog, &second, Some("Card 2"), &quick, &mut |_| {}, &mut || {}).unwrap();
        assert_eq!(catalog.unhashed().unwrap().len(), 5);
        (dir, card, second, catalog)
    }

    /// `is_there` as the check asks it, counting each look.
    fn counted() -> (std::cell::Cell<usize>, impl Fn(&std::cell::Cell<usize>, &Path) -> bool) {
        (
            std::cell::Cell::new(0),
            |looks: &std::cell::Cell<usize>, root: &Path| {
                looks.set(looks.get() + 1);
                crate::reach::is_there(root)
            },
        )
    }

    #[test]
    fn a_healthy_check_looks_at_each_source_once_not_at_each_file() {
        let (_dir, _card, _second, catalog) = two_cards();
        let (looks, is_there) = counted();
        let report = deep_look_unless_gone(
            &catalog,
            None,
            None,
            &|_| true,
            &|root| is_there(&looks, root),
            &mut |_| {},
            &|| true,
        )
        .unwrap();
        assert_eq!((report.checked, report.failed.len()), (5, 0));
        assert!(report.gone.is_empty());
        assert_eq!(looks.get(), 2, "one look for each of the two sources");
    }

    #[test]
    fn a_source_not_plugged_in_is_passed_over_at_once_and_its_files_stay_unchecked() {
        let (dir, card, second, catalog) = two_cards();
        let card_root = catalog.unhashed().unwrap()[0].source.clone();
        fs::rename(&card, dir.path().join("In a drawer")).unwrap();
        let (looks, is_there) = counted();
        let mut read = Vec::new();
        let report = deep_look_unless_gone(
            &catalog,
            None,
            None,
            &|_| true,
            &|root| is_there(&looks, root),
            &mut |p| read.push((p.reading.to_path_buf(), p.done, p.total)),
            &|| true,
        )
        .unwrap();
        assert_eq!(report.checked, 2, "the other card is checked");
        assert!(report.failed.is_empty(), "nothing on the card in a drawer was tried");
        assert_eq!(
            report.gone,
            [Gone {
                source: card_root,
                left: 3
            }]
        );
        assert_eq!(looks.get(), 2);
        let second = fs::canonicalize(&second).unwrap();
        assert!(read
            .iter()
            .all(|(path, _, total)| path.starts_with(&second) && *total == 2));
        let left = catalog.unhashed().unwrap();
        assert_eq!(left.len(), 3, "still not checked, exactly as before");
        assert!(left.iter().all(|f| !f.source.starts_with(&second)));
    }

    #[test]
    fn a_source_unplugged_part_way_ends_the_work_on_it_and_the_other_carries_on() {
        let (dir, card, second, catalog) = two_cards();
        let drawer = dir.path().join("In a drawer");
        let (looks, is_there) = counted();
        let mut seen = Vec::new();
        let report = deep_look_unless_gone(
            &catalog,
            None,
            None,
            &|_| true,
            &|root| is_there(&looks, root),
            &mut |p| {
                seen.push((p.done, p.total));
                // Pulled out once the first file of the first card is read.
                if p.done == 1 && card.is_dir() {
                    fs::rename(&card, &drawer).unwrap();
                }
            },
            &|| true,
        )
        .unwrap();
        assert_eq!(report.checked, 3, "one from the first card, both from the second");
        assert!(report.failed.is_empty(), "the file in hand is not counted as a failure");
        assert_eq!(report.gone.len(), 1);
        assert_eq!(report.gone[0].left, 2, "the file in hand and the one after it");
        assert_eq!(looks.get(), 3, "two at the start, one when a read failed");
        assert_eq!(
            seen,
            [(0, 5), (1, 5), (1, 3), (2, 3)],
            "the count shrinks, never stalls"
        );
        let second = fs::canonicalize(&second).unwrap();
        let left = catalog.unhashed().unwrap();
        assert_eq!(left.len(), 2);
        assert!(left.iter().all(|f| !f.source.starts_with(&second)));
    }

    #[test]
    fn a_file_that_cannot_be_told_apart_is_never_sent_or_left_out_on_trust() {
        let (_dir, drive, library, catalog) = world();
        // The two true copies go; their lookalike stays.
        fs::remove_file(drive.join("Trip/IMG_20100101_100000.jpg")).unwrap();
        fs::remove_file(drive.join("Backup/IMG_20100101_100000.jpg")).unwrap();
        let waiting = crate::check_and_place(&library, &catalog, &|| true, &mut |_| {}).unwrap();
        assert_eq!(waiting, 2, "both unreadable copies wait to be told apart");
        assert_eq!(
            catalog.collected().unwrap().len(),
            2,
            "only the lookalike and the small photo have a place"
        );
        assert_eq!(catalog.unhashed().unwrap().len(), 2, "the missing ones stay unchecked");
    }
}
