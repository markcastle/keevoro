//! Sending a backup's files to their destination, and checking they
//! arrived.
//!
//! A backup is the named job ("Family photos"); this module is the act of
//! sending what it is keeping somewhere: a folder, or storage that speaks
//! S3. `run` sends originals and previews and then writes the index. The
//! order matters: the index goes last, so it never points at something
//! that is not there yet, and it lists only files confirmed stored.
//! `check` re-reads what is kept on this computer and, if asked, what was
//! sent.
//!
//! Both say what happened by writing plain sentences to `out`, one per
//! line, and report progress as a short line of text through a callback.
//! The command line prints both as they are; the app shows the progress
//! line while it runs and the sentences when it is done.

use crate::summary;
use anyhow::{Context, Result};
use keevoro_catalog::{Catalog, RenditionState, VideoState};
use keevoro_core::{group_into_collections, CollectionInput, DateSource};
use keevoro_index::{build_index, gallery_v1, index_key, rendition_path, AlbumInput, ItemInput, Rendition};
use keevoro_publish::{publish, put_document, PublishOptions, Upload};
use keevoro_scan::FoundItem;
use keevoro_storage::{MultipartStore, ObjectKey, ObjectStore};
use keevoro_storage_folder::FolderStore;
use keevoro_storage_s3::{Preset, S3Settings, S3Store};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::time::{SystemTime, UNIX_EPOCH};

/// A gathered file and everything known about its copies in the library.
struct Kept<'a> {
    item: &'a FoundItem,
    hash: &'a str,
    /// Library-relative path of the original, also its storage key.
    path: String,
    preview: Option<RenditionState>,
    video: Option<VideoState>,
}

impl Kept<'_> {
    fn id(&self) -> &str {
        &self.hash[..self.hash.len().min(16)]
    }
    fn renditions(&self) -> Vec<Rendition> {
        let mut made = Vec::new();
        if let Some(RenditionState::Made { .. }) = self.preview {
            made.extend([Rendition::Thumb, Rendition::Display]);
        }
        if let Some(VideoState::Known { poster, mp4, .. }) = self.video {
            if poster {
                made.push(Rendition::Poster);
            }
            if mp4 {
                made.push(Rendition::Video);
            }
        }
        made
    }
}

/// Where a backup goes.
#[derive(Clone)]
pub enum Destination {
    /// An external drive or a network share.
    Folder(PathBuf),
    /// An S3-compatible bucket, with the keys to it. The keys come from
    /// the environment or the system keychain (see `connect.rs`), never
    /// from the command line, where they would end up in shell history.
    Bucket {
        preset: Preset,
        bucket: String,
        access_key_id: String,
        secret_access_key: String,
    },
}

pub const ACCESS_KEY_VAR: &str = "KEEVORO_ACCESS_KEY_ID";
pub const SECRET_KEY_VAR: &str = "KEEVORO_SECRET_ACCESS_KEY";

/// An opened destination: the store, a stable name for it, and the address
/// to use in the index when the user gives none.
type Opened = (Box<dyn MultipartStore>, String, String);

pub fn open(out: &mut dyn Write, runtime: &tokio::runtime::Runtime, destination: Destination) -> Result<Opened> {
    match destination {
        Destination::Folder(folder) => {
            let store = runtime
                .block_on(FolderStore::open(&folder))
                .with_context(|| format!("Couldn't use {} as a backup folder", folder.display()))?
                // Counted as it is written, so a large video shows steady progress.
                .watched_by(std::sync::Arc::new(|bytes| keevoro_core::meter::sent(bytes, false)));
            let home = std::fs::canonicalize(&folder).unwrap_or(folder);
            Ok((
                Box::new(store),
                format!("folder:{}", home.display()),
                BESIDE_THE_INDEX.to_string(),
            ))
        }
        Destination::Bucket {
            preset,
            bucket,
            access_key_id,
            secret_access_key,
        } => {
            let store = S3Store::connect(&S3Settings {
                preset,
                bucket,
                access_key_id,
                secret_access_key,
            })
            .map_err(|e| anyhow::anyhow!("{}.", keevoro_publish::plain_reason(&e)))?;
            // Find out now whether the keys work, not 9,000 files in.
            let check = runtime.block_on(store.check_connection()).map_err(|e| {
                anyhow::anyhow!(
                    "Couldn't connect to the storage. {}.",
                    keevoro_publish::plain_reason(&e)
                )
            })?;
            if check.publicly_listable == Some(true) {
                writeln!(
                    out,
                    "Warning: anyone can see the list of your files in this bucket. \
                     Turn off public listing in your storage settings."
                )?;
            }
            let name = store.target_name().to_string();
            // Until a web address is given, links are relative to wherever
            // the bucket is served from.
            Ok((Box::new(store), name, "/".to_string()))
        }
    }
}

/// What a step of getting ready is called while it happens.
fn step_line(step: crate::GatherStep) -> String {
    let n = summary::thousands;
    match step {
        crate::GatherStep::Checking { done, total } => {
            format!("Reading each file in full first: {} of {}", n(done), n(total))
        }
        crate::GatherStep::Copying { done, total } => format!("Getting ready: {} of {}", n(done), n(total)),
        crate::GatherStep::MakingPreviews { done, total } => {
            format!("Your files are backed up. Making previews: {} of {}", n(done), n(total))
        }
        crate::GatherStep::WorkingOnVideos { done, total } => {
            format!(
                "Your files are backed up. Making video previews: {} of {}",
                n(done),
                n(total)
            )
        }
    }
}

/// Sends everything being kept, the files themselves first.
///
/// The order is the point. A person's photos and videos are what matter,
/// so they start moving as soon as each has been read and checked.
/// Previews and the index, which only make the result nicer to browse, are
/// made and sent afterwards. If this is stopped halfway, what has arrived
/// is the originals, and running it again carries on.
///
/// A folder is first checked for room, so nobody finds a disk full with
/// half their photos sent. Setting `stop` ends it after the file in hand.
#[allow(clippy::too_many_arguments)]
pub fn originals_first(
    out: &mut dyn Write,
    catalog: &Catalog,
    home: &Path,
    destination: Destination,
    base_url: Option<&str>,
    video_tools: Option<&keevoro_media::VideoTools>,
    stop: &Stop,
    on_progress: &mut dyn FnMut(&str),
) -> Result<()> {
    let keep_going = || !stop.load(Ordering::SeqCst);
    let said = |bytes: &[u8]| String::from_utf8_lossy(bytes).into_owned();
    let files_sent = |out: &mut dyn Write, files: &[u8]| -> Result<()> {
        for line in said(files).lines() {
            if line.starts_with("Safely backed up") || line.ends_with("already there.") || line.starts_with("Warning:")
            {
                writeln!(out, "{line}")?;
            }
        }
        Ok(())
    };
    let stopped = |out: &mut dyn Write| -> Result<()> {
        writeln!(
            out,
            "Stopped. Everything backed up so far is safe where it is. Back up again whenever you like and it carries on from here."
        )?;
        Ok(())
    };

    crate::check_and_place(home, catalog, &keep_going, &mut |step| on_progress(&step_line(step)))?;
    if !keep_going() {
        return stopped(out);
    }
    if let Destination::Folder(folder) = &destination {
        let needed = crate::discover(home, catalog)?.waiting_bytes;
        if let Some(room) = crate::room::room_in(folder, needed).filter(|room| !room.is_enough()) {
            anyhow::bail!(room.refusal(folder));
        }
    }

    // The files themselves.
    let mut files = Vec::new();
    run_until(
        &mut files,
        catalog,
        home,
        destination.clone(),
        base_url,
        false,
        Some(stop),
        on_progress,
    )?;
    if !keep_going() {
        files_sent(out, &files)?;
        return stopped(out);
    }

    // Then what makes them nice to look through.
    crate::prepare_to_send_while(home, catalog, video_tools, &keep_going, &mut |step| {
        on_progress(&step_line(step));
    })?;
    let mut extras = Vec::new();
    run_until(
        &mut extras,
        catalog,
        home,
        destination,
        base_url,
        false,
        Some(stop),
        on_progress,
    )?;

    // Said once, in the order it happened: the files, then the previews,
    // then anything that needs attention (from the second pass, which
    // tried again whatever the first could not send).
    files_sent(out, &files)?;
    for line in said(&extras).lines() {
        if let Some(rest) = line.strip_prefix("Safely backed up ") {
            writeln!(out, "Previews added: {rest}")?;
        } else if !line.ends_with("already there.") && !line.starts_with("Warning:") {
            writeln!(out, "{line}")?;
        }
    }
    if !keep_going() {
        stopped(out)?;
    }
    Ok(())
}

/// A switch that stops a send: set it to true from anywhere and the file
/// in hand is finished but no more are started.
pub type Stop = std::sync::Arc<std::sync::atomic::AtomicBool>;

pub fn run(
    out: &mut dyn Write,
    catalog: &Catalog,
    library: &Path,
    destination: Destination,
    base_url: Option<&str>,
    new_link: bool,
    on_progress: &mut dyn FnMut(&str),
) -> Result<()> {
    run_until(
        out,
        catalog,
        library,
        destination,
        base_url,
        new_link,
        None,
        on_progress,
    )
}

/// As [`run`], with a switch that stops it between files.
#[allow(clippy::too_many_arguments)]
pub fn run_until(
    out: &mut dyn Write,
    catalog: &Catalog,
    library: &Path,
    destination: Destination,
    base_url: Option<&str>,
    new_link: bool,
    stop: Option<&Stop>,
    on_progress: &mut dyn FnMut(&str),
) -> Result<()> {
    let reports = catalog.load_reports()?;
    let (unique, _) = summary::unique_items(&reports);
    let mut kept = Vec::new();
    for item in unique {
        let Some(hash) = item.blake3.as_deref() else { continue };
        // A file the user has since unticked stays in the library but is
        // not published.
        if !crate::wanted(catalog, item)? {
            continue;
        }
        if let Some(path) = catalog.collected_path(hash)? {
            kept.push(Kept {
                item,
                hash,
                path,
                preview: catalog.rendition(hash)?,
                video: catalog.video(hash)?,
            });
        }
    }
    if kept.is_empty() {
        writeln!(
            out,
            "There is nothing in the library to back up yet. Run `keevoro gather` first."
        )?;
        return Ok(());
    }

    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
    let over_network = matches!(destination, Destination::Bucket { .. });
    let (store, target, default_url) = open(out, &runtime, destination)?;
    let store = store.as_ref();
    let base_url = base_url.map_or(default_url, str::to_string);

    let uploads = uploads_for(library, catalog, &kept)?;
    let n = summary::thousands;
    let opts = PublishOptions {
        stop: stop.cloned(),
        over_network,
        // Several at once to cloud storage; one at a time to a disk.
        at_once: if over_network { 6 } else { 1 },
        ..PublishOptions::default()
    };
    let report = runtime.block_on(publish(store, catalog, &target, &uploads, &opts, &mut |p| {
        on_progress(&format!("{} of {} safely backed up", n(p.done), n(p.total)));
    }))?;

    // The index lists only what is confirmed to be in the storage.
    let mut stored = Vec::new();
    for k in &kept {
        if catalog.is_uploaded(&target, &k.path)? {
            stored.push(k);
        }
    }
    let setting = format!("index_key:{target}");
    let fresh_key = || -> Result<String> {
        let mut random = [0u8; 16];
        getrandom::fill(&mut random).map_err(|e| anyhow::anyhow!("Couldn't get random numbers ({e})"))?;
        Ok(index_key(random))
    };
    let previous = catalog.setting(&setting)?;
    // The old index is only removed once the new one is safely written.
    let (key, retired) = match previous {
        Some(old) if new_link => (fresh_key()?, Some(old)),
        Some(current) => (current, None),
        None => (fresh_key()?, None),
    };
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|d| i64::try_from(d.as_secs()).ok())
        .unwrap_or(0);
    let index = build_index(
        &base_url,
        now,
        &albums_for(&stored),
        stored.iter().map(|k| item_for(k)).collect(),
    );
    let documents = [
        (key.clone(), serde_json::to_vec(&index)?, "application/json"),
        (
            "index/gallery.json".to_string(),
            serde_json::to_vec(&gallery_v1(&index))?,
            "application/json",
        ),
        // A note for whoever opens the backup, explaining its folders.
        (
            crate::layout::GUIDE_NAME.to_string(),
            crate::layout::guide().into_bytes(),
            "text/plain; charset=utf-8",
        ),
    ];
    for (name, body, kind) in documents {
        runtime
            .block_on(put_document(store, &ObjectKey::new(name)?, body, kind, &opts))
            .map_err(|e| anyhow::anyhow!("Couldn't write the index. {}.", keevoro_publish::plain_reason(&e)))?;
    }

    catalog.set_setting(&setting, &key)?;
    if let Some(old) = retired {
        runtime.block_on(store.delete(&ObjectKey::new(old)?)).map_err(|e| {
            anyhow::anyhow!(
                "The new link works, but the old index couldn't be removed. {}.",
                keevoro_publish::plain_reason(&e)
            )
        })?;
        writeln!(out, "Your private link has changed. The old one no longer works.")?;
    }

    if report.uploaded > 0 {
        writeln!(
            out,
            "Safely backed up {} ({}).",
            summary::count(report.uploaded, "file"),
            summary::bytes(report.uploaded_bytes)
        )?;
    }
    if report.already_there > 0 {
        writeln!(out, "{} already there.", summary::count(report.already_there, "file"))?;
    }
    if !report.failed.is_empty() {
        writeln!(out, "Couldn't back up {}:", summary::count(report.failed.len(), "file"))?;
        for f in &report.failed {
            writeln!(out, "  {}: {}.", f.key, f.reason)?;
        }
        writeln!(out, "Run `keevoro publish` again to retry.")?;
    }
    writeln!(
        out,
        "Index written for {} in {}.",
        summary::count(index.items.len(), "file"),
        summary::count(index.albums.len(), "album")
    )?;
    if index.base_url == BESIDE_THE_INDEX {
        // In a folder the index is named by where it sits among the files
        // that were sent, not by where the folder happens to be.
        writeln!(out, "Your private index: {key}")?;
    } else {
        writeln!(out, "Your private index: {}{key}", index.base_url)?;
    }
    Ok(())
}

/// `keevoro check`: re-read the library, and optionally the backup, and
/// say plainly whether everything is still as it was. Fails (so scripts
/// notice) when anything is wrong.
pub fn check(
    out: &mut dyn Write,
    catalog: &Catalog,
    library: &Path,
    backup: Option<Destination>,
    read_back: bool,
    on_progress: &mut dyn FnMut(&str),
) -> Result<()> {
    let n = summary::thousands;
    let files = catalog.collected()?;
    if files.is_empty() {
        writeln!(
            out,
            "There is nothing in the library to check yet. Run `keevoro gather` first."
        )?;
        return Ok(());
    }

    let report = keevoro_collect::verify_library(library, catalog, &mut |done, total| {
        on_progress(&format!("Checking your library: {} of {}", n(done), n(total)));
    })?;
    let mut wrong = report.problems.len();
    if report.intact == 0 && report.problems.is_empty() {
        // Nothing is kept in this folder: everything was sent straight
        // from its source, so there is no library to speak of.
    } else if report.problems.is_empty() {
        writeln!(
            out,
            "Your library: all {} are intact.",
            summary::count(report.intact, "file")
        )?;
    } else {
        writeln!(
            out,
            "Your library: {} intact, {} with a problem:",
            n(report.intact),
            n(report.problems.len())
        )?;
        for p in &report.problems {
            writeln!(out, "  {}: {}.", p.path.display(), p.reason)?;
        }
        writeln!(
            out,
            "To put these right, plug in the source they came from and run `keevoro gather`, or copy them back from your backup."
        )?;
    }

    if let Some(destination) = backup {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
        let (store, _, _) = open(out, &runtime, destination)?;
        let expected = files
            .iter()
            .map(|f| {
                Ok(keevoro_publish::Expected {
                    key: ObjectKey::new(f.path.clone())?,
                    bytes: f.bytes,
                    blake3: f.blake3.clone(),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let opts = PublishOptions::default();
        let result = runtime.block_on(keevoro_publish::check_backup(
            store.as_ref(),
            &expected,
            read_back,
            &opts,
            &mut |p| {
                on_progress(&format!("Checking your backup: {} of {}", n(p.done), n(p.total)));
            },
        ));
        wrong += result.problems.len();
        if result.problems.is_empty() {
            let how = if read_back && result.read_back == result.present {
                ", and each one was read back and matches exactly"
            } else if read_back {
                ", and all but the largest were read back and match exactly"
            } else {
                ""
            };
            writeln!(
                out,
                "Your backup: all {} are there{how}.",
                summary::count(result.present, "original")
            )?;
        } else {
            writeln!(
                out,
                "Your backup: {} fine, {} with a problem:",
                n(result.present),
                n(result.problems.len())
            )?;
            for p in &result.problems {
                writeln!(out, "  {}: {}.", p.key, p.reason)?;
            }
            writeln!(out, "Run `keevoro publish` to send anything that is missing.")?;
        }
    }
    if wrong > 0 {
        anyhow::bail!(
            "{} {} attention.",
            summary::count(wrong, "file"),
            if wrong == 1 { "needs" } else { "need" }
        );
    }
    Ok(())
}

/// Originals first, then previews.
fn uploads_for(library: &Path, catalog: &Catalog, kept: &[Kept<'_>]) -> Result<Vec<Upload>> {
    let local = |relative: &str| relative.split('/').fold(library.to_path_buf(), |p, s| p.join(s));
    let mut uploads = Vec::new();
    for k in kept {
        uploads.push(Upload {
            key: ObjectKey::new(k.path.clone())?,
            // The copy in this library if there is one, otherwise the
            // file where it was found: the destination does not care.
            file: keevoro_collect::bytes_of(library, catalog, k.item)?,
            bytes: k.item.bytes,
            write_once: true,
            content_type: Some(k.item.mime.clone()),
            metadata: BTreeMap::from([("blake3".to_string(), k.hash.to_string())]),
        });
    }
    for k in kept {
        for kind in k.renditions() {
            let relative = rendition_path(kind, k.id());
            let file = local(&relative);
            // A preview deleted from the library is simply left out; the
            // next `gather` makes it again.
            let Ok(meta) = std::fs::metadata(&file) else { continue };
            uploads.push(Upload {
                key: ObjectKey::new(relative)?,
                file,
                bytes: meta.len(),
                write_once: false,
                content_type: Some(
                    if kind == Rendition::Video {
                        "video/mp4"
                    } else {
                        "image/webp"
                    }
                    .to_string(),
                ),
                metadata: BTreeMap::new(),
            });
        }
    }
    Ok(uploads)
}

fn item_for(k: &Kept<'_>) -> ItemInput {
    let (width, height, blurhash) = match &k.preview {
        Some(RenditionState::Made {
            width,
            height,
            blurhash,
        }) => (Some(*width), Some(*height), Some(blurhash.clone())),
        // No preview, but the scan may still have read the size.
        _ => (k.item.width, k.item.height, None),
    };
    // For a video the size and length come from reading the video itself.
    let (width, height, duration) = match &k.video {
        Some(VideoState::Known {
            duration_secs,
            width,
            height,
            ..
        }) => (Some(*width), Some(*height), Some(*duration_secs)),
        _ => (width, height, None),
    };
    ItemInput {
        id: k.id().to_string(),
        kind: k.item.kind,
        path: k.path.clone(),
        date: k.item.date,
        width,
        height,
        bytes: k.item.bytes,
        mime: k.item.mime.clone(),
        blurhash,
        duration,
        renditions: k.renditions(),
    }
}

/// The same collections the Discover screen shows become the albums:
/// the user's named folders, then moments for everything unfiled.
fn albums_for(stored: &[&Kept<'_>]) -> Vec<AlbumInput> {
    let inputs: Vec<CollectionInput> = stored
        .iter()
        .map(|k| CollectionInput {
            album: k.item.folder.album.clone(),
            date: k.item.date.filter(|d| d.source != DateSource::Modified).map(|d| d.date),
            kind: k.item.kind,
            bytes: k.item.bytes,
        })
        .collect();
    group_into_collections(&inputs)
        .into_iter()
        .map(|c| AlbumInput {
            path: c.parents.iter().cloned().chain([c.title.clone()]).collect(),
            items: c
                .members
                .iter()
                .filter_map(|&m| stored.get(m))
                .map(|k| k.id().to_string())
                .collect(),
        })
        .collect()
}

/// The address a folder's index uses for its own files: relative to the
/// index, which sits in `index/` one level down.
///
/// A folder of photos gets moved: to another disk, another drive letter,
/// another computer. Addresses that spell out where the folder was on the
/// day it was made (`file:///Z:/Backups/...`) would break the moment it
/// moved, and would tell anyone handed the index how the owner's disks are
/// laid out. Relative ones go wherever the folder goes and say nothing.
const BESIDE_THE_INDEX: &str = "../";

#[cfg(test)]
mod tests {
    use super::*;

    /// A source with two photos, searched quickly into a backup whose
    /// home folder is somewhere else entirely.
    fn one_backup() -> (tempfile::TempDir, PathBuf, PathBuf, Catalog) {
        let dir = tempfile::tempdir().unwrap();
        let card = dir.path().join("Card");
        for (rel, shade) in [
            ("Flights/IMG_20240601_100000.jpg", 10),
            ("Flights/IMG_20240601_100500.jpg", 90),
        ] {
            let p = card.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, crate::discover::tests::real_jpeg(shade)).unwrap();
        }
        let home = dir.path().join("home");
        let mut catalog = Catalog::open_in_library(&home).unwrap();
        crate::scan_into(&mut catalog, &card, Some("Card"), &mut |_| {}).unwrap();
        (dir, card, home, catalog)
    }

    fn send(catalog: &Catalog, home: &Path, to: &Path) -> String {
        let mut said = Vec::new();
        originals_first(
            &mut said,
            catalog,
            home,
            Destination::Folder(to.to_path_buf()),
            None,
            None,
            &Stop::default(),
            &mut |_| {},
        )
        .unwrap();
        String::from_utf8(said).unwrap()
    }

    #[test]
    fn files_go_straight_from_the_source_to_a_folder_with_no_copy_in_between() {
        let (dir, card, home, catalog) = one_backup();
        let to = dir.path().join("Drone footage");
        let said = send(&catalog, &home, &to);
        assert!(said.contains("Safely backed up"), "{said}");

        // Each original is in the destination, exact, under its album.
        for name in ["IMG_20240601_100000.jpg", "IMG_20240601_100500.jpg"] {
            let sent = to.join("originals").join("Flights").join(name);
            assert_eq!(
                keevoro_scan::whole_hash(&sent).unwrap(),
                keevoro_scan::whole_hash(&card.join("Flights").join(name)).unwrap()
            );
        }
        // And the backup's own folder holds no second copy of them.
        assert!(!home.join("originals").exists());
        // The app counts them as safe.
        assert_eq!(crate::discover(&home, &catalog).unwrap().waiting_to_gather, 0);
    }

    #[test]
    fn a_backup_explains_itself_and_leaves_no_folder_unexplained() {
        let (dir, _card, home, catalog) = one_backup();
        let to = dir.path().join("usb");
        send(&catalog, &home, &to);
        let guide = std::fs::read_to_string(to.join(crate::layout::GUIDE_NAME)).unwrap();
        assert_eq!(guide, crate::layout::guide());
        for entry in std::fs::read_dir(&to).unwrap() {
            let entry = entry.unwrap();
            let name = entry.file_name().to_string_lossy().to_string();
            // The work area is hidden and empty between backups.
            if entry.path().is_dir() && !name.starts_with('.') {
                assert!(
                    crate::layout::PARTS.iter().any(|p| p.folder == name),
                    "the folder \"{name}\" is in the backup but not in the guide"
                );
            }
        }
    }

    #[test]
    fn sending_again_later_only_sends_what_is_new() {
        let (dir, card, home, mut catalog) = one_backup();
        let to = dir.path().join("Drone footage");
        send(&catalog, &home, &to);
        let again = send(&catalog, &home, &to);
        assert!(!again.contains("Safely backed up"), "nothing new to send: {again}");

        // Another flight, another day: the same backup, looked through again.
        std::fs::write(
            card.join("Flights").join("IMG_20240702_090000.jpg"),
            crate::discover::tests::real_jpeg(200),
        )
        .unwrap();
        crate::scan_into(&mut catalog, &card, Some("Card"), &mut |_| {}).unwrap();
        assert_eq!(crate::discover(&home, &catalog).unwrap().waiting_to_gather, 1);
        let later = send(&catalog, &home, &to);
        // Only the new photo, then its two previews; the six files from
        // before are not sent again.
        assert!(later.contains("Safely backed up 1 file "), "{later}");
        assert!(later.contains("6 files already there"), "{later}");
        assert!(later.contains("Previews added: 2 files"), "{later}");
        assert!(to
            .join("originals")
            .join("Flights")
            .join("IMG_20240702_090000.jpg")
            .is_file());
    }

    #[test]
    fn the_files_themselves_are_sent_before_any_preview_is_made() {
        let (dir, _card, home, catalog) = one_backup();
        let to = dir.path().join("Drone footage");
        let mut lines = Vec::new();
        let mut said = Vec::new();
        originals_first(
            &mut said,
            &catalog,
            &home,
            Destination::Folder(to.clone()),
            None,
            None,
            &Stop::default(),
            &mut |line| {
                // Note, each time something is said, whether the originals have arrived.
                let arrived = to
                    .join("originals")
                    .join("Flights")
                    .join("IMG_20240601_100000.jpg")
                    .is_file();
                lines.push((line.to_string(), arrived));
            },
        )
        .unwrap();
        let first_preview = lines
            .iter()
            .position(|(line, _)| line.contains("Making previews"))
            .unwrap();
        assert!(
            lines[first_preview].1,
            "the originals were already there when previews began"
        );
        assert!(lines[..first_preview]
            .iter()
            .any(|(line, _)| line.contains("safely backed up")));

        let said = String::from_utf8(said).unwrap();
        let files = said.find("Safely backed up 2 files").unwrap();
        let previews = said.find("Previews added: 4 files").unwrap();
        assert!(files < previews, "{said}");
        assert_eq!(
            said.matches("Index written").count(),
            1,
            "the index is spoken of once: {said}"
        );
    }

    #[test]
    fn a_folder_without_room_is_refused_before_anything_is_sent() {
        let room = crate::room::Room {
            needed: 10 << 30,
            free: 1 << 30,
        };
        assert!(!room.is_enough());
        // And a real send to a real folder with room goes ahead.
        let (dir, _card, home, catalog) = one_backup();
        let to = dir.path().join("out");
        assert!(send(&catalog, &home, &to).contains("Safely backed up"));
    }

    #[test]
    fn a_send_can_be_stopped_and_carried_on_later() {
        let (dir, _card, home, catalog) = one_backup();
        let to = dir.path().join("Drone footage");
        let stop = Stop::default();
        let mut said = Vec::new();
        originals_first(
            &mut said,
            &catalog,
            &home,
            Destination::Folder(to.clone()),
            None,
            None,
            &stop,
            // Asked to stop as soon as the first file has gone.
            &mut |line| {
                if line.starts_with("1 of") {
                    stop.store(true, Ordering::SeqCst);
                }
            },
        )
        .unwrap();
        let said = String::from_utf8(said).unwrap();
        assert!(said.contains("Safely backed up 1 file "), "{said}");
        assert!(said.contains("Stopped. Everything backed up so far is safe"), "{said}");
        assert!(!said.contains("Previews added"), "nothing more was started: {said}");
        assert!(!to
            .join("originals")
            .join("Flights")
            .join("IMG_20240601_100500.jpg")
            .exists());

        // Sending again carries on from where it stopped.
        let again = send(&catalog, &home, &to);
        assert!(again.contains("Safely backed up 1 file "), "{again}");
        assert!(again.contains("1 file already there"), "{again}");
        assert!(to
            .join("originals")
            .join("Flights")
            .join("IMG_20240601_100500.jpg")
            .is_file());
    }

    #[test]
    fn a_folders_index_never_says_where_the_folder_is() {
        let (dir, _card, home, catalog) = one_backup();
        let to = dir.path().join("Drone footage");
        let said = send(&catalog, &home, &to);
        assert!(said.contains("Your private index: index/"), "{said}");
        assert!(!said.contains("file://"), "{said}");

        let here = to.to_string_lossy().replace('\\', "/");
        let mut seen = 0;
        for entry in std::fs::read_dir(to.join("index")).unwrap() {
            let file = entry.unwrap().path();
            let text = std::fs::read_to_string(&file).unwrap();
            assert!(!text.contains("file://"), "{} has a file address", file.display());
            assert!(!text.contains(&here), "{} names the folder it is in", file.display());
            assert!(
                !text.contains(&*dir.path().to_string_lossy()),
                "{} names a path",
                file.display()
            );
            seen += 1;
        }
        assert_eq!(seen, 2, "the index and the gallery list");

        // Every address in the gallery list leads, from the index's own
        // folder, to a file that was really sent.
        let gallery: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(to.join("index").join("gallery.json")).unwrap()).unwrap();
        let mut urls = Vec::new();
        fn collect(v: &serde_json::Value, urls: &mut Vec<String>) {
            match v {
                serde_json::Value::Object(map) => {
                    for (key, value) in map {
                        match (key.as_str(), value.as_str()) {
                            ("url" | "thumbnail", Some(text)) => urls.push(text.to_string()),
                            _ => collect(value, urls),
                        }
                    }
                }
                serde_json::Value::Array(list) => list.iter().for_each(|v| collect(v, urls)),
                _ => {}
            }
        }
        collect(&gallery, &mut urls);
        assert!(urls.len() >= 4, "{urls:?}");
        for url in urls {
            assert!(url.starts_with("../"), "{url}");
            let decoded = url.trim_start_matches("../").replace("%20", " ");
            assert!(to.join(decoded).is_file(), "{url} leads nowhere");
        }
    }
}
