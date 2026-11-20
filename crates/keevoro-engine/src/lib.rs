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
pub mod connect;
pub mod deep;
pub mod discover;
pub mod history;
pub mod layout;
pub mod preview;
pub mod reach;
pub mod room;
pub mod send;
pub mod summary;

pub use discover::{
    collection_photos, discover, set_aside, set_collection_kept, set_photo_kept, CollectionView, Discover, PhotoChoice,
    PhotoView, SourceView,
};
pub use discover::{duplicates, forget_source, friendly_path, CopyView, DuplicateGroup};
pub use preview::{preview, preview_at, Look};

use anyhow::{bail, Result};
use keevoro_catalog::Catalog;
use keevoro_core::MediaKind;
use keevoro_media::VideoTools;
use keevoro_scan::{scan, FoundItem, ScanOptions, ScanProgress, ScanReport};
pub use keevoro_storage_folder::{sweep_working_files, Swept, ABANDONED_AFTER};
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
    keep_found_before(catalog, &mut report, &options.kinds)?;
    let fallback = root
        .file_name()
        .map_or_else(|| root.display().to_string(), |n| n.to_string_lossy().to_string());
    on_saving();
    catalog.save_scan(name.unwrap_or(&fallback), &report, now())?;
    Ok(report)
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
    let Some(before) = catalog.load_reports()?.into_iter().find(|r| r.root == report.root) else {
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
    let chosen = chosen(catalog, &reports)?;
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
    // Every other file is fingerprinted as it is sent, in the same pass
    // over it, so nothing is read twice.
    let mut looks_like = std::collections::HashMap::<&str, usize>::new();
    for item in reports.iter().flat_map(|r| &r.items) {
        if let (false, Some(quick)) = (item.hashed, &item.blake3) {
            *looks_like.entry(quick.as_str()).or_default() += 1;
        }
    }
    let mut to_check = std::collections::HashSet::new();
    for item in reports.iter().flat_map(|r| &r.items) {
        if let (false, Some(quick)) = (item.hashed, &item.blake3) {
            let alike = looks_like.get(quick.as_str()).copied().unwrap_or(0) > 1;
            if alike && wanted(catalog, item)? {
                to_check.insert(quick.clone());
            }
        }
    }
    let reports = if to_check.is_empty() {
        reports
    } else {
        deep::deep_look(
            catalog,
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
            && item
                .blake3
                .as_deref()
                .is_some_and(|quick| looks_like.get(quick).copied().unwrap_or(0) > 1)
    };
    let mut waiting = 0;
    for item in reports.iter().flat_map(|r| &r.items) {
        if alike(item) && wanted(catalog, item)? {
            waiting += 1;
        }
    }
    let chosen: Vec<&FoundItem> = chosen(catalog, &reports)?.into_iter().filter(|i| !alike(i)).collect();
    keevoro_collect::agree_places(catalog, &chosen, true, &mut |done, total| {
        on_step(ReadyStep::Placing { done, total });
    })?;
    Ok(waiting)
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|d| i64::try_from(d.as_secs()).ok())
        .unwrap_or(0)
}

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
