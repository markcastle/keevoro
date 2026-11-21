//! What Keevoro does, as plain function calls.
//!
//! The crates below this one each do one job: scan a drive, copy a file,
//! make a thumbnail. This crate strings those jobs into the things a person
//! actually asks for ("look through this drive", "show me what you found",
//! "back up my photos"), and returns plain data describing the result.
//!
//! `keevoro-api` sits on top of it, and the app and the `keevoro` command
//! line sit on that; none of them contains any of this logic. A rule such
//! as "a picture the user chose to keep is not junk" is written once, in
//! [`wanted`], and every front end gets it.
//!
//! Nothing here prints, prompts or knows about windows. Progress is reported
//! through callbacks so each front end can show it in its own way.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod background;
pub mod bucket;
pub mod confirm;
pub mod connect;
pub mod deep;
pub mod discover;
pub mod history;
pub mod layout;
pub mod look;
pub mod preview;
pub mod reach;
pub mod room;
pub mod send;
pub mod summary;
pub mod surveys;

pub use discover::{
    collection_photos, discover, set_aside, set_collection_kept, set_photo_kept, CollectionView, Discover, PhotoChoice,
    PhotoView, SourceView,
};
pub use discover::{duplicates, forget_source, friendly_path, safe_by_folder, CopyView, DuplicateGroup};
pub use preview::{preview, preview_at, Look};

use anyhow::{bail, Result};
use keevoro_catalog::Catalog;
use keevoro_core::MediaKind;
use keevoro_media::VideoTools;
use keevoro_scan::{scan, FoundItem, ScanOptions, ScanProgress, ScanReport};
pub use keevoro_storage_folder::{sweep_working_files, working_files_to_clear, Swept, ABANDONED_AFTER};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Does the person want this file backed up?
///
/// An explicit choice always wins. Without one, everything is kept except
/// what looks like junk, which is how "junk is pre-unticked" works.
pub fn wanted(catalog: &Catalog, item: &FoundItem) -> Result<bool> {
    let choice = match &item.blake3 {
        Some(hash) => catalog.choice(hash)?,
        None => None,
    };
    Ok(choice.unwrap_or(item.junk.is_none()))
}

/// Looks through one drive or folder and remembers what was found in the
/// backup's catalog under a friendly name. Nothing on the drive is
/// changed. Scanning the same place again replaces what was known about it.
pub fn scan_into(
    catalog: &mut Catalog,
    source: &Path,
    name: Option<&str>,
    on_progress: &mut dyn FnMut(&ScanProgress),
) -> Result<ScanReport> {
    scan_into_with(catalog, source, name, &ScanOptions::default(), on_progress, &mut || {})
}

/// As [`scan_into`], with a say in how the search is done (a quick search
/// samples large files; see [`deep`]). `on_saving` is called once the
/// looking is over and what was found is about to be written down, which on
/// a big source takes a noticeable moment of its own.
pub fn scan_into_with(
    catalog: &mut Catalog,
    source: &Path,
    name: Option<&str>,
    options: &ScanOptions,
    on_progress: &mut dyn FnMut(&ScanProgress),
    on_saving: &mut dyn FnMut(),
) -> Result<ScanReport> {
    let _busy = keevoro_core::doing::doing(keevoro_core::doing::Work::LookingThroughASource);
    // A remembered drive is recognised by its full path, so `D:` and
    // `D:\.` must come out the same.
    let root = std::fs::canonicalize(source).unwrap_or_else(|_| source.to_path_buf());
    let mut report = scan(&root, options, on_progress);
    // A search that was stopped has only seen part of the source. Saving
    // that would make the rest look as if it were not there, so nothing
    // is remembered: whatever was known about this source stays as it was.
    if report.stopped {
        return Ok(report);
    }
    if report.gone {
        // The drive was unplugged part-way. What was found so far is kept,
        // and so is everything found there before that this look did not
        // reach: it cannot be checked now, and a drive unplugged is no
        // reason to forget what is on it. Looking again once it is back
        // puts it right.
        keep_all_found_before(catalog, &mut report)?;
    } else {
        keep_found_before(catalog, &mut report, &options.kinds)?;
    }
    let fallback = root
        .file_name()
        .map_or_else(|| root.display().to_string(), |n| n.to_string_lossy().to_string());
    on_saving();
    catalog.save_scan(name.unwrap_or(&fallback), &report, now())?;
    Ok(report)
}

/// Keeps everything found on this source before that `report` does not
/// have, for a look cut short by the drive going.
fn keep_all_found_before(catalog: &Catalog, report: &mut ScanReport) -> Result<()> {
    let Some(before) = catalog
        .load_reports_as_found()?
        .into_iter()
        .find(|r| r.root == report.root)
    else {
        return Ok(());
    };
    let found_now: std::collections::HashSet<PathBuf> = report.items.iter().map(|i| i.path.clone()).collect();
    report
        .items
        .extend(before.items.into_iter().filter(|item| !found_now.contains(&item.path)));
    report.items.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(())
}

/// Keeps what was found on this source before whose kind is no longer
/// looked for, as long as it is still there, the same size, and what was
/// found before and is now only in online storage (not opened, so as not
/// to download it). Switching a
/// kind off, or removing one, changes what is looked for from now on; it
/// never takes away a file already found, which may already be backed up
/// and in the backup's index. A file of a kind still looked for that was
/// not found again has gone, or changed, and is not kept.
fn keep_found_before(catalog: &Catalog, report: &mut ScanReport, kinds: &keevoro_core::Kinds) -> Result<()> {
    let Some(before) = catalog
        .load_reports_as_found()?
        .into_iter()
        .find(|r| r.root == report.root)
    else {
        return Ok(());
    };
    let found_now: std::collections::HashSet<&Path> = report.items.iter().map(|i| i.path.as_path()).collect();
    let online: std::collections::HashSet<&Path> = report.online_only.iter().map(PathBuf::as_path).collect();
    let kept: Vec<FoundItem> = before
        .items
        .into_iter()
        .filter(|item| !found_now.contains(item.path.as_path()))
        .filter(|item| {
            // Moved to online storage since: still there, only not on this
            // computer, so still found.
            if online.contains(item.path.as_path()) {
                return true;
            }
            let ending = item.path.extension().map(|e| e.to_string_lossy()).unwrap_or_default();
            kinds.looked_for(&ending).is_none()
                && std::fs::metadata(&item.path).is_ok_and(|m| m.is_file() && m.len() == item.bytes)
        })
        .collect();
    if !kept.is_empty() {
        report.other_files = report.other_files.saturating_sub(kept.len() as u64);
        report.items.extend(kept);
        report.items.sort_by(|a, b| a.path.cmp(&b.path));
    }
    Ok(())
}

/// Which part of getting ready to back up is under way, for a progress
/// display.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadyStep {
    /// Reading in full files a quick search only sampled, where two look
    /// alike and must be told apart.
    Checking {
        done: usize,
        total: usize,
    },
    /// Giving each file its place.
    Placing {
        done: usize,
        total: usize,
    },
    MakingPreviews {
        done: usize,
        total: usize,
    },
    WorkingOnVideos {
        done: usize,
        total: usize,
    },
}

/// Makes the previews and video posters a gallery shows, for everything
/// being kept, from each file where it was found. They are kept in the
/// backup's own folder (`home`) until they are sent. Whatever was made
/// stays made if this is stopped.
pub fn make_previews(
    home: &Path,
    catalog: &Catalog,
    video_tools: Option<&VideoTools>,
    keep_going: &dyn Fn() -> bool,
    on_step: &mut dyn FnMut(ReadyStep),
) -> Result<()> {
    let reports = catalog.load_reports()?;
    // A file in cloud storage is not downloaded to make its previews.
    let chosen: Vec<&FoundItem> = chosen(catalog, &reports)?
        .into_iter()
        .filter(|i| !keevoro_core::bucket::is_bucket_path(&i.path))
        .collect();
    keevoro_collect::make_previews_while(
        home,
        catalog,
        &chosen,
        &mut |done, total| on_step(ReadyStep::MakingPreviews { done, total }),
        keep_going,
    )?;
    if chosen.iter().any(|i| i.kind == MediaKind::Video) && keevoro_media::video::can_make_posters(video_tools) {
        keevoro_collect::make_video_previews_while(
            home,
            catalog,
            &chosen,
            video_tools,
            &mut |done, total| on_step(ReadyStep::WorkingOnVideos { done, total }),
            keep_going,
        )?;
    }
    Ok(())
}

/// Everything being kept, once each.
fn chosen<'a>(catalog: &Catalog, reports: &'a [ScanReport]) -> Result<Vec<&'a FoundItem>> {
    let (unique, _) = summary::unique_items(reports);
    let mut chosen = Vec::new();
    for item in unique {
        if wanted(catalog, item)? {
            chosen.push(item);
        }
    }
    Ok(chosen)
}

/// The part of getting ready that has to come before anything is sent:
/// files that look alike are told apart, and every kept file is given its
/// place. Previews are made afterwards, in the background while the
/// originals are sent (see [`background`]), because the originals are what
/// matter and should start moving first.
///
/// A file that looks like another but could not be read in full to tell
/// them apart (its source unplugged, say) is given no place, so it is
/// neither sent nor left out as a duplicate on the strength of a sample.
/// It waits until it can be read. Answers how many files are waiting so.
pub fn check_and_place(
    home: &Path,
    catalog: &Catalog,
    keep_going: &dyn Fn() -> bool,
    on_step: &mut dyn FnMut(ReadyStep),
) -> Result<usize> {
    let reports = catalog.load_reports()?;
    // Rule one: never write to a source. A backup's own folder, where its
    // previews are made, inside a source would do exactly that.
    let home = std::fs::canonicalize(home).unwrap_or_else(|_| home.to_path_buf());
    if let Some(r) = reports.iter().find(|r| home.starts_with(&r.root)) {
        bail!(
            "This backup's own folder is inside {}, which is one of its sources. \
             Keevoro never writes to those.",
            r.root.display()
        );
    }

    // A quick search only sampled the large files, so two of them can look
    // alike without being the same. Those must be told apart before
    // anything moves, since a true copy is left out and a lookalike is not.
    // A file looks like another when it shares its quick fingerprint with
    // another still unchecked, or with one read in full since (by the
    // background check, say, before it was stopped or gave way): it may be
    // a true copy of that one. Every other file is fingerprinted as it is
    // sent, in the same pass over it, so nothing is read twice.
    let checked = catalog.checked_quick_fingerprints()?;
    let mut looks_like = std::collections::HashMap::<&str, usize>::new();
    for item in reports.iter().flat_map(|r| &r.items) {
        if let (false, Some(quick)) = (item.hashed, &item.blake3) {
            *looks_like.entry(quick.as_str()).or_default() += 1;
        }
    }
    let mut to_check = std::collections::HashSet::new();
    for item in reports.iter().flat_map(|r| &r.items) {
        // A file in cloud storage has a quick fingerprint of its own that
        // nothing else shares: it is told apart from copies once it is
        // read, as it is backed up, never downloaded twice to do it here.
        if keevoro_core::bucket::is_bucket_path(&item.path) {
            continue;
        }
        if let (false, Some(quick)) = (item.hashed, &item.blake3) {
            let alike = looks_like.get(quick.as_str()).copied().unwrap_or(0) > 1 || checked.contains(quick);
            if alike && wanted(catalog, item)? {
                to_check.insert(quick.clone());
            }
        }
    }
    let reports = if to_check.is_empty() {
        reports
    } else {
        // Only files that look alike are checked here, and a picture made
        // while looking cannot be told apart between them, so none is renamed.
        deep::deep_look(
            catalog,
            None,
            Some(&to_check),
            &mut |p| {
                on_step(ReadyStep::Checking {
                    done: p.done,
                    total: p.total,
                });
            },
            keep_going,
        )?;
        catalog.load_reports()?
    };

    // Files still only sampled are, by now, ones nothing else looks like,
    // unless they could not be read.
    let mut looks_like = std::collections::HashMap::<&str, usize>::new();
    for item in reports.iter().flat_map(|r| &r.items) {
        if let (false, Some(quick)) = (item.hashed, &item.blake3) {
            *looks_like.entry(quick.as_str()).or_default() += 1;
        }
    }
    let alike = |item: &FoundItem| {
        !item.hashed
            && !keevoro_core::bucket::is_bucket_path(&item.path)
            && item
                .blake3
                .as_deref()
                .is_some_and(|quick| looks_like.get(quick).copied().unwrap_or(0) > 1 || checked.contains(quick))
    };
    let mut waiting = 0;
    for item in reports.iter().flat_map(|r| &r.items) {
        if alike(item) && wanted(catalog, item)? {
            waiting += 1;
        }
    }
    let chosen: Vec<&FoundItem> = chosen(catalog, &reports)?.into_iter().filter(|i| !alike(i)).collect();
    follow_a_changed_arrangement(catalog, &chosen)?;
    keevoro_collect::agree_places(catalog, &chosen, true, &mut |done, total| {
        on_step(ReadyStep::Placing { done, total });
    })?;
    Ok(waiting)
}

/// When how the backup is arranged has changed since its places were last
/// agreed, frees the place of each chosen file that no longer matches it
/// and was never sent anywhere, so it is placed afresh. A file sent
/// anywhere keeps its place: nothing backed up is ever moved, sent twice,
/// or counted as missing. Answers how many files it looked at, which is
/// none when nothing changed, so a backup whose choice is as it was pays
/// nothing.
pub fn follow_a_changed_arrangement(catalog: &Catalog, chosen: &[&FoundItem]) -> Result<usize> {
    let now = keevoro_core::arrange::arrangement_said(&catalog.choices()?);
    let before = catalog
        .arranged_as()?
        .unwrap_or_else(|| keevoro_core::Arrange::ByDate.name().to_string());
    if now == before {
        return Ok(0);
    }
    let mut stale = Vec::new();
    for item in chosen {
        let Some(hash) = item.blake3.as_deref() else { continue };
        let Some(place) = catalog.collected_path(hash)? else {
            continue;
        };
        let (folders, _) = keevoro_core::fit_place(
            keevoro_core::library_folder(item.kind, &item.folder.album, item.date.as_ref()),
            String::new(),
        );
        let wanted = format!("{}/", folders.join("/")).to_ascii_lowercase();
        let rest = place.to_ascii_lowercase();
        // Directly in the folder it would go in now: as it should be.
        if !(rest.starts_with(&wanted) && !rest[wanted.len()..].contains('/')) {
            stale.push(hash);
        }
    }
    catalog.forget_unsent_places(&stale)?;
    catalog.set_arranged_as(&now)?;
    Ok(chosen.len())
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|d| i64::try_from(d.as_secs()).ok())
        .unwrap_or(0)
}

#[cfg(test)]
mod arranging_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use keevoro_core::{FileKinds, KindChange};
    use std::fs;

    fn look(catalog: &mut Catalog, drive: &Path, kinds: &FileKinds) -> Vec<String> {
        let options = ScanOptions {
            kinds: kinds.lookup(),
            ..ScanOptions::default()
        };
        scan_into_with(catalog, drive, None, &options, &mut |_| {}, &mut || {}).unwrap();
        let mut names: Vec<String> = catalog.load_reports().unwrap()[0]
            .items
            .iter()
            .map(|i| i.path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    /// Moving the folder away while it is walked stands for pulling the
    /// drive out. Windows refuses to move a folder the walk has open, so
    /// there it is proved on a real computer.
    #[cfg(unix)]
    #[test]
    fn a_drive_unplugged_while_it_is_looked_through_again_keeps_everything_found_on_it() {
        let dir = tempfile::tempdir().unwrap();
        let card = dir.path().join("Card");
        let photos: Vec<Vec<u8>> = (0..25u8).map(crate::discover::tests::real_jpeg).collect();
        for folder in 0..40 {
            let at = card.join(format!("Folder {folder:02}"));
            fs::create_dir_all(&at).unwrap();
            for (file, photo) in photos.iter().enumerate() {
                fs::write(at.join(format!("IMG_{file:04}.jpg")), photo).unwrap();
            }
        }
        let mut catalog = Catalog::in_memory().unwrap();
        let options = ScanOptions::default();
        scan_into_with(&mut catalog, &card, Some("Card"), &options, &mut |_| {}, &mut || {}).unwrap();
        let before = catalog.load_reports().unwrap()[0].items.len();
        assert_eq!(before, 1000);

        // Looked through again, and pulled out as the first file is said.
        let away = dir.path().join("In a drawer");
        let mut pulled = false;
        let report = scan_into_with(
            &mut catalog,
            &card,
            Some("Card"),
            &options,
            &mut |_| {
                if !pulled {
                    fs::rename(&card, &away).unwrap();
                    pulled = true;
                }
            },
            &mut || {},
        )
        .unwrap();
        assert!(report.gone);
        assert!(
            report.unreadable.is_empty(),
            "the files are not unreadable, the card is gone"
        );
        assert_eq!(
            catalog.load_reports().unwrap()[0].items.len(),
            before,
            "nothing found on it before is forgotten"
        );
        fs::rename(&away, &card).unwrap();
    }

    #[test]
    fn switching_a_kind_off_or_removing_it_never_takes_away_what_was_found() {
        let dir = tempfile::tempdir().unwrap();
        let drive = dir.path().join("drive");
        fs::create_dir_all(&drive).unwrap();
        fs::write(drive.join("a.xyz"), [1u8; 100]).unwrap();
        fs::write(drive.join("b.xyz"), [2u8; 100]).unwrap();
        fs::write(drive.join("c.xyz"), [3u8; 100]).unwrap();
        let mut catalog = Catalog::open_in_library(&dir.path().join("home")).unwrap();
        let mut kinds = FileKinds::default();
        kinds.add("xyz", keevoro_core::MediaKind::Photo).unwrap();
        assert_eq!(look(&mut catalog, &drive, &kinds), ["a.xyz", "b.xyz", "c.xyz"]);

        // Switched off, then looked through again: the ones found stay found,
        // unless they have gone or changed meanwhile.
        let off = KindChange {
            on: Some(false),
            ..KindChange::default()
        };
        kinds.change("xyz", &off).unwrap();
        fs::remove_file(drive.join("b.xyz")).unwrap();
        fs::write(drive.join("c.xyz"), [3u8; 50]).unwrap();
        fs::write(drive.join("d.xyz"), [4u8; 100]).unwrap();
        assert_eq!(
            look(&mut catalog, &drive, &kinds),
            ["a.xyz"],
            "nothing new of that kind is found"
        );

        kinds.remove("xyz").unwrap();
        assert_eq!(look(&mut catalog, &drive, &kinds), ["a.xyz"]);

        // Looked for again, a file that has gone is gone.
        kinds.add("xyz", keevoro_core::MediaKind::Photo).unwrap();
        fs::remove_file(drive.join("a.xyz")).unwrap();
        assert_eq!(look(&mut catalog, &drive, &kinds), ["c.xyz", "d.xyz"]);
    }
}
