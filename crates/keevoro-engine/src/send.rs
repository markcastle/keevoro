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
                // A folder on a file share counts as sent over the network.
                .watched_by({
                    let remote = crate::reach::is_across_a_network(&folder);
                    std::sync::Arc::new(move |bytes| keevoro_core::meter::sent(bytes, remote))
                });
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
            format!("Telling apart files that look alike: {} of {}", n(done), n(total))
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

    // Then what makes them nice to look through, unless this backup is
    // for the originals alone.
    if !wants_previews(catalog)? {
        return files_sent(out, &files);
    }
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

/// How one destination's turn went, for whoever keeps the record.
pub struct Turn<'a> {
    pub to: &'a crate::connect::Saved,
    pub seconds: u64,
    /// Bytes that arrived there in this turn.
    pub bytes: u64,
    pub stopped: bool,
}

/// Backs up to every destination of this backup, one after another, the
/// originals first each time. A destination that cannot be reached (a
/// drive unplugged, a connection down) is said and passed over, and the
/// others still get their turn; it catches up next time. It is an error
/// only when none of them could be reached.
#[allow(clippy::too_many_arguments)]
pub fn everywhere(
    out: &mut dyn Write,
    catalog: &Catalog,
    home: &Path,
    world: &crate::connect::World<'_>,
    video_tools: Option<&keevoro_media::VideoTools>,
    stop: &Stop,
    on_progress: &mut dyn FnMut(&str),
    after_each: &mut dyn FnMut(Turn<'_>),
) -> Result<()> {
    let all = crate::connect::destinations(catalog)?;
    if all.is_empty() {
        anyhow::bail!("Choose where to back up to first.");
    }
    let several = all.len() > 1;
    let mut reached = 0;
    let mut last_refusal = None;
    for to in &all {
        if stop.load(Ordering::SeqCst) {
            break;
        }
        let name = to.describe();
        if several {
            writeln!(out, "To {name}:")?;
        }
        let started = std::time::Instant::now();
        let before = keevoro_core::meter::totals().0;
        let turn = (|| {
            let destination = to.open(world)?;
            if to.target.is_none() {
                // Learn the name its records are kept under, for counting.
                let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
                let (_, target, _) = open(&mut Vec::new(), &runtime, destination.clone())?;
                crate::connect::note_target(catalog, to, &target)?;
            }
            originals_first(
                out,
                catalog,
                home,
                destination,
                to.base_url.as_deref(),
                video_tools,
                stop,
                &mut |line| {
                    if several {
                        on_progress(&format!("To {name}: {line}"));
                    } else {
                        on_progress(line);
                    }
                },
            )
        })();
        match turn {
            Ok(()) => {
                reached += 1;
                after_each(Turn {
                    to,
                    seconds: started.elapsed().as_secs(),
                    bytes: keevoro_core::meter::totals().0.saturating_sub(before),
                    stopped: stop.load(Ordering::SeqCst),
                });
            }
            Err(refused) => {
                writeln!(out, "Couldn't back up to {name}. {refused:#}")?;
                last_refusal = Some(refused);
            }
        }
    }
    match (reached, last_refusal) {
        (0, Some(refused)) => Err(refused),
        _ => Ok(()),
    }
}

/// The catalog's name for the choice of whether a backup also gets previews.
const PREVIEWS: &str = "previews";

/// Does this backup also make and send previews (thumbnails, screen-sized
/// photos and video stills) for a gallery to show? It does unless the
/// person has said the originals alone are wanted.
pub fn wants_previews(catalog: &Catalog) -> Result<bool> {
    Ok(catalog.setting(PREVIEWS)?.as_deref() != Some("off"))
}

/// Remembers, for this backup, whether previews are wanted.
pub fn set_wants_previews(catalog: &Catalog, wanted: bool) -> Result<()> {
    catalog.set_setting(PREVIEWS, if wanted { "on" } else { "off" })?;
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

/// Everything being kept that has a place agreed for it, as the catalog
/// stands at this moment.
fn kept_now<'a>(catalog: &Catalog, reports: &'a [keevoro_scan::ScanReport]) -> Result<Vec<Kept<'a>>> {
    let (unique, _) = summary::unique_items(reports);
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
    Ok(kept)
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
    let kept = kept_now(catalog, &reports)?;
    if kept.is_empty() {
        writeln!(
            out,
            "There is nothing in the library to back up yet. Run `keevoro gather` first."
        )?;
        return Ok(());
    }

    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
    // Several files at once wherever the far end is across a network: six
    // to cloud storage, fewer to a folder on a file share. One at a time
    // to a disk, where more would only make it hop about.
    let (over_network, at_once) = match &destination {
        Destination::Bucket { .. } => (true, 6),
        Destination::Folder(folder) if crate::reach::is_across_a_network(folder) => {
            (true, crate::reach::AT_ONCE_ACROSS_A_NETWORK)
        }
        Destination::Folder(_) => (false, 1),
    };
    let (store, target, default_url) = open(out, &runtime, destination)?;
    let store = store.as_ref();
    let base_url = base_url.map_or(default_url, str::to_string);

    let uploads = uploads_for(library, catalog, &kept)?;
    let n = summary::thousands;
    let opts = PublishOptions {
        stop: stop.cloned(),
        over_network,
        at_once,
        ..PublishOptions::default()
    };
    let report = runtime.block_on(publish(store, catalog, &target, &uploads, &opts, &mut |p| {
        // After a tab: the file just dealt with, for a screen to show beneath.
        on_progress(&format!(
            "{} of {} safely backed up\t{}",
            n(p.done),
            n(p.total),
            p.latest
        ));
    }))?;

    // Files sent before their whole fingerprint was known now have one,
    // worked out as they went. Each is recorded, and from here on the file
    // goes by it: in the index below, and for its previews.
    if !report.learned.is_empty() {
        let by_place: std::collections::HashMap<&str, &Kept<'_>> = kept.iter().map(|k| (k.path.as_str(), k)).collect();
        for (place, whole) in &report.learned {
            if let Some(k) = by_place.get(place.as_str()) {
                catalog.set_whole_hash(&k.item.path, k.hash, whole)?;
            }
        }
    }
    let reports = catalog.load_reports()?;
    let kept = kept_now(catalog, &reports)?;

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
            // A file a search only sampled goes by a quick fingerprint,
            // which is not the file's own. Its real one is worked out as
            // it is sent, and it is labelled with nothing until then.
            metadata: if k.item.hashed {
                BTreeMap::from([("blake3".to_string(), k.hash.to_string())])
            } else {
                BTreeMap::new()
            },
            learn_fingerprint: !k.item.hashed,
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
                learn_fingerprint: false,
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
    fn a_backup_can_be_the_originals_alone_with_no_previews_made_or_sent() {
        let (dir, card, home, catalog) = one_backup();
        let to = dir.path().join("usb");
        assert!(
            wants_previews(&catalog).unwrap(),
            "previews are made unless switched off"
        );
        set_wants_previews(&catalog, false).unwrap();
        let said = send(&catalog, &home, &to);
        assert!(said.contains("Safely backed up"), "{said}");
        assert!(!said.contains("Previews added"), "{said}");
        // The originals are there, exact, and nothing else was made.
        assert_eq!(
            keevoro_scan::whole_hash(&to.join("originals/Flights/IMG_20240601_100000.jpg")).unwrap(),
            keevoro_scan::whole_hash(&card.join("Flights/IMG_20240601_100000.jpg")).unwrap()
        );
        assert!(!to.join("renditions").exists(), "no previews in the backup");
        assert!(!home.join("renditions").exists(), "and none made on this computer");
        assert!(
            to.join("index").join("gallery.json").is_file(),
            "the list of what is there is still written"
        );
        assert_eq!(crate::discover(&home, &catalog).unwrap().waiting_to_gather, 0);

        // Switching them back on adds the previews next time, and sends no original twice.
        set_wants_previews(&catalog, true).unwrap();
        let said = send(&catalog, &home, &to);
        assert!(said.contains("Previews added"), "{said}");
        assert!(to.join("renditions").join("thumb").is_dir());
    }

    #[test]
    fn a_backup_can_go_to_two_places_and_is_safe_only_when_it_is_at_both() {
        use crate::connect::{self, testing::MemorySecrets, Flags, Saved, World};
        let (dir, _card, home, catalog) = one_backup();
        let (first, second) = (dir.path().join("usb"), dir.path().join("nas"));
        let secrets = MemorySecrets::default();
        let world = World {
            secrets: &secrets,
            ask: &connect::testing::nobody_to_ask,
        };
        let folder = |to: &Path| {
            Saved::from_flags(&Flags {
                to_folder: Some(to.to_path_buf()),
                ..Flags::default()
            })
            .unwrap()
        };
        connect::add(&mut Vec::new(), &catalog, &folder(&first), &world).unwrap();
        let go = |catalog: &Catalog| {
            let mut said = Vec::new();
            let mut turns = Vec::new();
            everywhere(
                &mut said,
                catalog,
                &home,
                &world,
                None,
                &Stop::default(),
                &mut |_| {},
                &mut |turn| turns.push(turn.to.describe()),
            )
            .unwrap();
            (String::from_utf8(said).unwrap(), turns)
        };
        go(&catalog);
        let found = crate::discover(&home, &catalog).unwrap();
        assert_eq!(found.waiting_to_gather, 0, "with one destination, there is safe");
        let all = found.destinations[0].there;
        assert!(all > 0);

        // A second destination is added. Nothing is safe until it has them too.
        connect::add(&mut Vec::new(), &catalog, &folder(&second), &world).unwrap();
        let found = crate::discover(&home, &catalog).unwrap();
        assert_eq!(found.destinations.len(), 2);
        assert_eq!((found.destinations[0].there, found.destinations[0].waiting), (all, 0));
        assert_eq!((found.destinations[1].there, found.destinations[1].waiting), (0, all));
        assert!(found.destinations[1].waiting_bytes > 0);
        assert_eq!(found.waiting_to_gather, all, "at one of two is not safe yet");

        // Backing up again sends only to the one that lacks them.
        let (said, turns) = go(&catalog);
        assert_eq!(turns.len(), 2, "each destination had its turn: {said}");
        assert!(said.contains("To the folder"), "{said}");
        let found = crate::discover(&home, &catalog).unwrap();
        assert_eq!(found.waiting_to_gather, 0);
        assert_eq!(found.destinations[1].there, all);
        for to in [&first, &second] {
            assert!(to.join("originals/Flights/IMG_20240601_100000.jpg").is_file());
        }

        // Taking one off the list deletes nothing there, and the other still counts.
        connect::remove(&catalog, &world, 0).unwrap();
        assert!(first.join("originals/Flights/IMG_20240601_100000.jpg").is_file());
        let found = crate::discover(&home, &catalog).unwrap();
        assert_eq!((found.destinations.len(), found.waiting_to_gather), (1, 0));
    }

    #[test]
    fn a_destination_that_cannot_be_reached_does_not_stop_the_others() {
        use crate::connect::{self, testing::MemorySecrets, Flags, Saved, World};
        let (dir, _card, home, catalog) = one_backup();
        let good = dir.path().join("usb");
        // A file where a folder is wanted: it cannot be made into one.
        let blocked = dir.path().join("not a folder");
        std::fs::write(&blocked, b"in the way").unwrap();
        let secrets = MemorySecrets::default();
        let world = World {
            secrets: &secrets,
            ask: &connect::testing::nobody_to_ask,
        };
        connect::add(
            &mut Vec::new(),
            &catalog,
            &Saved::from_flags(&Flags {
                to_folder: Some(good.clone()),
                ..Flags::default()
            })
            .unwrap(),
            &world,
        )
        .unwrap();
        // Remembered from a time it worked, as an unplugged drive would be.
        let mut all = connect::destinations(&catalog).unwrap();
        all.insert(
            0,
            Saved {
                kind: "folder".into(),
                place: blocked.join("inside").to_string_lossy().to_string(),
                bucket: None,
                base_url: None,
                target: Some("folder:gone".into()),
            },
        );
        catalog
            .set_setting("destinations", &serde_json::to_string(&all).unwrap())
            .unwrap();

        let mut said = Vec::new();
        let mut turns = 0;
        everywhere(
            &mut said,
            &catalog,
            &home,
            &world,
            None,
            &Stop::default(),
            &mut |_| {},
            &mut |_| turns += 1,
        )
        .unwrap();
        let said = String::from_utf8(said).unwrap();
        assert!(said.contains("Couldn't back up to the folder"), "{said}");
        assert_eq!(turns, 1, "the one that works had its turn");
        assert!(good.join("originals/Flights/IMG_20240601_100000.jpg").is_file());
        // And it is not called safe: one of its two places has nothing.
        assert!(crate::discover(&home, &catalog).unwrap().waiting_to_gather > 0);
    }

    #[test]
    fn large_files_are_fingerprinted_as_they_are_sent_not_read_twice() {
        // A JPEG header then a long run of bytes that depends on `seed`.
        let big = |seed: u8| {
            let mut bytes = crate::discover::tests::real_jpeg(seed);
            let mut next = seed;
            bytes.resize_with(bytes.len() + 2_000_000, || {
                next = next.wrapping_mul(31).wrapping_add(7);
                next
            });
            bytes
        };
        let dir = tempfile::tempdir().unwrap();
        let card = dir.path().join("Card");
        let original = big(1);
        // Differs only between the places a quick search samples.
        let mut lookalike = original.clone();
        lookalike[700_000] ^= 1;
        let alone = big(2);
        for (rel, bytes) in [
            ("Trip/IMG_20100101_100000.jpg", &original),
            ("Trip/IMG_20100101_100001.jpg", &lookalike),
            ("Backup/IMG_20100101_100000.jpg", &original),
            ("Trip/IMG_20100103_120000.jpg", &alone),
        ] {
            let p = card.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, bytes).unwrap();
        }
        let home = dir.path().join("home");
        let mut catalog = Catalog::open_in_library(&home).unwrap();
        let quick = keevoro_scan::ScanOptions {
            quick: true,
            ..Default::default()
        };
        crate::scan_into_with(&mut catalog, &card, Some("Card"), &quick, &mut |_| {}, &mut || {}).unwrap();
        assert_eq!(catalog.unhashed().unwrap().len(), 4, "a quick search sampled all four");

        let to = dir.path().join("usb");
        let mut lines = Vec::new();
        originals_first(
            &mut Vec::new(),
            &catalog,
            &home,
            Destination::Folder(to.clone()),
            None,
            None,
            &Stop::default(),
            &mut |line| lines.push(line.to_string()),
        )
        .unwrap();

        // Only the three that look like one another were read beforehand.
        // The one nothing resembles went straight to being sent.
        assert!(
            lines
                .iter()
                .any(|l| l.starts_with("Telling apart files that look alike: 2 of 3")),
            "{lines:?}"
        );
        assert!(!lines
            .iter()
            .any(|l| l.contains("of 4") && l.starts_with("Telling apart")));

        // Every file now has its whole fingerprint, the right one.
        assert!(catalog.unhashed().unwrap().is_empty());
        // The true copy was left out; the lookalike was not.
        let there = |rel: &str| std::fs::read(to.join("originals").join(rel)).ok();
        assert!(there("Trip/IMG_20100101_100000.jpg").as_deref() == Some(&original[..]));
        assert!(there("Trip/IMG_20100101_100001.jpg").as_deref() == Some(&lookalike[..]));
        assert!(there("Trip/IMG_20100103_120000.jpg").as_deref() == Some(&alone[..]));
        assert!(
            there("Backup/IMG_20100101_100000.jpg").is_none(),
            "the duplicate is kept once"
        );
        // And all of it counts as safe, under the fingerprints it has now.
        let found = crate::discover(&home, &catalog).unwrap();
        assert_eq!(found.waiting_to_gather, 0);

        // Backing up again sends nothing: each is known to be there.
        let mut said = Vec::new();
        originals_first(
            &mut said,
            &catalog,
            &home,
            Destination::Folder(to.clone()),
            None,
            None,
            &Stop::default(),
            &mut |_| {},
        )
        .unwrap();
        let said = String::from_utf8(said).unwrap();
        assert!(
            !said.contains("Safely backed up 3") && !said.contains("Safely backed up 1 "),
            "{said}"
        );
        // The sources were only read.
        assert!(std::fs::read(card.join("Trip/IMG_20100103_120000.jpg")).unwrap() == alone);
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
