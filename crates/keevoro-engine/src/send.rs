//! Sending a backup's files to their destination, and checking they
//! arrived.
//!
//! A backup is the named job ("Family photos"); this module is the act of
//! sending what it is keeping somewhere: a folder, or storage that speaks
//! S3. [`everywhere`] sends to each destination in turn, originals first,
//! then previews, and then writes the index. Previews are made in the
//! background while the originals go (see [`crate::background`]), and sent
//! only once the originals have all arrived. The order matters: the index
//! goes last, so it never points at something that is not there yet, and
//! it lists only files confirmed stored. [`check`] confirms what is at a
//! destination.
//!
//! Both say what happened by writing plain sentences to `out`, one per
//! line, and report progress as a short line of text through a callback.
//! Every front end shows the progress line while it runs and the sentences
//! when it is done.

use crate::background::Previews;
use crate::summary;
use anyhow::{Context, Result};
use keevoro_catalog::{Catalog, RenditionState, VideoState};
use keevoro_core::estimate::{Kind, Planned, Tracker};
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

/// A kept file, its place, and the previews made of it.
struct Kept<'a> {
    item: &'a FoundItem,
    hash: &'a str,
    /// The original's place in a backup, which is also its storage key.
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
    /// the environment or the system keychain (see `connect.rs`).
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
fn step_line(step: crate::ReadyStep) -> String {
    let n = summary::thousands;
    match step {
        crate::ReadyStep::Checking { done, total } => {
            format!("Telling apart files that look alike: {} of {}", n(done), n(total))
        }
        crate::ReadyStep::Placing { done, total } => {
            format!("Working out where each file goes: {} of {}", n(done), n(total))
        }
        crate::ReadyStep::MakingPreviews { done, total } => format!("Making previews: {} of {}", n(done), n(total)),
        crate::ReadyStep::WorkingOnVideos { done, total } => {
            format!("Making video previews: {} of {}", n(done), n(total))
        }
    }
}

/// What a backup is doing for one destination, for the estimate: where it
/// is going, the estimate, and the previews being made in the background.
pub struct Going<'a> {
    pub stop: &'a Stop,
    pub tracker: &'a Tracker,
    /// The destination in words, as the plan names it.
    pub to: String,
    /// Made once, from the first destination's turn, for all of them.
    pub previews: &'a mut Option<Previews>,
    /// What this destination's turn did, for the backup's history.
    pub record: &'a mut crate::history::DestinationRecord,
}

/// One pass of sending: which kind of work it is, for the estimate, and
/// whether its index is the last one written.
#[derive(Clone, Copy)]
struct Pass<'a> {
    sending: Kind,
    to: &'a str,
    last: bool,
}

/// What a backup says when it has been stopped.
pub const STOPPED: &str =
    "Stopped. Everything backed up so far is safe where it is. Back up again whenever you like and it carries on from here.";

/// Sends everything being kept, the files themselves first, and answers
/// how many files could not be sent.
///
/// The order is the point. A person's photos and videos are what matter,
/// so they start moving as soon as each has been read and checked.
/// Previews and the index, which only make the result nicer to browse, are
/// made and sent afterwards. If this is stopped halfway, what has arrived
/// is the originals, and running it again carries on.
///
/// A folder is first checked for room, so nobody finds a disk full with
/// half their photos sent. Setting `stop` ends it after the file in hand,
/// and it then says [`STOPPED`].
#[allow(clippy::too_many_arguments)]
fn originals_first(
    out: &mut dyn Write,
    catalog: &Catalog,
    home: &Path,
    destination: Destination,
    base_url: Option<&str>,
    video_tools: Option<&keevoro_media::VideoTools>,
    going: &mut Going<'_>,
    on_progress: &mut dyn FnMut(&str),
) -> Result<usize> {
    let stop = going.stop;
    let tracker = going.tracker;
    let to = going.to.clone();
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
    let stopped = |out: &mut dyn Write| -> Result<usize> {
        writeln!(out, "{STOPPED}")?;
        Ok(0)
    };

    let not_told_apart = crate::check_and_place(home, catalog, &keep_going, &mut |step| {
        match step {
            crate::ReadyStep::Checking { done, total } => {
                tracker.set_work(Kind::TellingApart, None, total as u64);
                tracker.progress(Kind::TellingApart, None, done as u64);
            }
            crate::ReadyStep::Placing { done, total } => {
                tracker.set_work(Kind::Placing, None, total as u64);
                tracker.progress(Kind::Placing, None, done as u64);
            }
            _ => {}
        }
        on_progress(&step_line(step));
    })?;
    if !keep_going() {
        return stopped(out);
    }
    tracker.finish(Kind::TellingApart, None);
    tracker.finish(Kind::Placing, None);
    let unsure = |out: &mut dyn Write| -> Result<()> {
        if not_told_apart > 0 {
            writeln!(
                out,
                "{} like another but couldn't be read in full to tell them apart, so {} not backed up yet. \
                 Plug in {} source and back up again.",
                match not_told_apart {
                    1 => "1 file looks".to_string(),
                    n => format!("{} files look", summary::thousands(n)),
                },
                if not_told_apart == 1 { "it was" } else { "they were" },
                if not_told_apart == 1 { "its" } else { "their" },
            )?;
        }
        Ok(())
    };
    if let Destination::Folder(folder) = &destination {
        let needed = crate::discover(home, catalog)?.waiting_bytes;
        if let Some(room) = crate::room::room_in(folder, needed).filter(|room| !room.is_enough()) {
            return Err(crate::room::NotEnoughRoom(room.refusal(folder)).into());
        }
    }

    // Previews are made in the background from now on, while the files
    // themselves are sent, unless this backup is for the originals alone.
    let wanted = wants_previews(catalog)?;
    if wanted && going.previews.is_none() {
        *going.previews = Some(Previews::start(
            home,
            video_tools.cloned(),
            stop.clone(),
            tracker.clone(),
        ));
    }

    // The files themselves.
    let mut files = Vec::new();
    let failed = run_until(
        &mut files,
        catalog,
        home,
        destination.clone(),
        base_url,
        Some(stop),
        on_progress,
        Pass {
            sending: Kind::SendingOriginals,
            to: &to,
            last: !wanted,
        },
        tracker,
        going.record,
    )?;
    if !keep_going() {
        files_sent(out, &files)?;
        return stopped(out).map(|_| failed + not_told_apart);
    }

    // Anything that could not be sent is said once, with its reason.
    let files_failed = |out: &mut dyn Write| -> Result<()> {
        for line in said(&files).lines() {
            if line.starts_with("Couldn't back up") || line.starts_with("  ") || line.starts_with("Back up again") {
                writeln!(out, "{line}")?;
            }
        }
        Ok(())
    };
    let originals_failed = failed;

    // Then what makes them nice to look through, unless this backup is
    // for the originals alone.
    if !wanted {
        files_sent(out, &files)?;
        files_failed(out)?;
        unsure(out)?;
        return Ok(failed + not_told_apart);
    }
    // Whatever the background has not made yet is finished now, at the
    // foreground's pace, since there is nothing else to do.
    if let Some(previews) = going.previews.as_mut() {
        previews.wait(on_progress)?;
    }
    if !keep_going() {
        files_sent(out, &files)?;
        return stopped(out).map(|_| failed + not_told_apart);
    }
    let mut extras = Vec::new();
    let previews_failed = run_until(
        &mut extras,
        catalog,
        home,
        destination,
        base_url,
        Some(stop),
        on_progress,
        Pass {
            sending: Kind::SendingPreviews,
            to: &to,
            last: true,
        },
        tracker,
        going.record,
    )?;

    // Said once, in the order it happened: the files and any that could
    // not be sent, then the previews and anything about them.
    files_sent(out, &files)?;
    files_failed(out)?;
    for line in said(&extras).lines() {
        if let Some(rest) = line.strip_prefix("Safely backed up ") {
            writeln!(out, "Previews added: {rest}")?;
        } else if line.starts_with("Back up again") && originals_failed > 0 {
            // Already said, after the files.
        } else if !line.ends_with("already there.") && !line.starts_with("Warning:") {
            writeln!(out, "{line}")?;
        }
    }
    unsure(out)?;
    if !keep_going() {
        stopped(out)?;
    }
    Ok(originals_failed + previews_failed + not_told_apart)
}

/// How one destination's turn went, for whoever keeps the record.
pub struct Turn<'a> {
    pub to: &'a crate::connect::Saved,
    pub seconds: u64,
    /// Bytes that arrived there in this turn.
    pub bytes: u64,
    pub stopped: bool,
}

/// How backing up to every destination went, for whoever has to say so
/// in a word: a script's exit code, say.
#[derive(Debug, Default)]
pub struct Went {
    /// Destinations that had their turn.
    pub reached: usize,
    /// Destinations that could not be reached: a drive unplugged, a
    /// connection down, keys that no longer work.
    pub unreachable: usize,
    /// Destinations without the room for what is waiting.
    pub no_room: usize,
    /// Files that could not be sent, added up over every destination.
    pub failed_files: usize,
    /// True when it was stopped before it had finished.
    pub stopped: bool,
    /// Why, when no destination could be backed up to at all.
    pub refused: Option<anyhow::Error>,
    /// What each destination's turn did, in order, for the history.
    pub destinations: Vec<crate::history::DestinationRecord>,
    /// The previews made, when this backup makes them.
    pub previews: Option<crate::history::PreviewsMade>,
}

/// Backs up to every destination of this backup, one after another, the
/// originals first each time. A destination that cannot be reached (a
/// drive unplugged, a connection down) is said and passed over, and the
/// others still get their turn; it catches up next time. When none of them
/// could be backed up to, [`Went::refused`] says why.
#[allow(clippy::too_many_arguments)]
pub fn everywhere(
    out: &mut dyn Write,
    catalog: &Catalog,
    home: &Path,
    world: &crate::connect::World<'_>,
    video_tools: Option<&keevoro_media::VideoTools>,
    stop: &Stop,
    tracker: &Tracker,
    on_progress: &mut dyn FnMut(&str),
    after_each: &mut dyn FnMut(Turn<'_>),
) -> Result<Went> {
    let all = crate::connect::destinations(catalog)?;
    if all.is_empty() {
        anyhow::bail!("Choose where to back up to first.");
    }
    tracker.plan(plan(catalog, home, &all, tracker)?);
    let several = all.len() > 1;
    let mut went = Went::default();
    let mut previews = None;
    for to in &all {
        // Stopped before this one's turn, even before the first: said the
        // same way as when it is stopped part-way through one.
        if stop.load(Ordering::SeqCst) {
            writeln!(out, "{STOPPED}")?;
            went.stopped = true;
            break;
        }
        let name = to.describe();
        if several {
            writeln!(out, "To {name}:")?;
        }
        let started = std::time::Instant::now();
        let before = keevoro_core::meter::totals().0;
        let mut record = crate::history::DestinationRecord::new(&name);
        let mut cleared = keevoro_storage_folder::Swept::default();
        let turn = (|| {
            let destination = to.open(world)?;
            // Working files an earlier backup left behind go first, so the
            // room they took is free for this one.
            if let Destination::Folder(folder) = &destination {
                add_swept(&mut cleared, tidy(folder));
            }
            if to.target.is_none() {
                // Learn the name its records are kept under, for counting.
                let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
                let (_, target, _) = open(&mut Vec::new(), &runtime, destination.clone())?;
                crate::connect::note_target(catalog, to, &target)?;
            }
            let folder = match &destination {
                Destination::Folder(folder) => Some(folder.clone()),
                Destination::Bucket { .. } => None,
            };
            let failed = originals_first(
                out,
                catalog,
                home,
                destination,
                to.base_url.as_deref(),
                video_tools,
                &mut Going {
                    stop,
                    tracker,
                    to: name.clone(),
                    previews: &mut previews,
                    record: &mut record,
                },
                &mut |line| {
                    if several {
                        on_progress(&format!("To {name}: {line}"));
                    } else {
                        on_progress(line);
                    }
                },
            );
            // And again at the end, for anything left by a turn stopped part-way.
            if let Some(folder) = &folder {
                add_swept(&mut cleared, tidy(folder));
            }
            failed
        })();
        if cleared.bytes >= NOTICEABLE {
            writeln!(out, "{}", cleared_words(cleared.bytes))?;
        }
        record.cleared_files = cleared.files;
        record.cleared_bytes = cleared.bytes;
        let stopped = stop.load(Ordering::SeqCst);
        record.seconds = started.elapsed().as_secs();
        record.bytes = keevoro_core::meter::totals().0.saturating_sub(before);
        record.reached = turn.is_ok();
        record.problem = turn.as_ref().err().map(|e| format!("{e:#}"));
        went.destinations.push(record);
        match turn {
            Ok(failed) => {
                went.reached += 1;
                went.failed_files += failed;
                after_each(Turn {
                    to,
                    seconds: started.elapsed().as_secs(),
                    bytes: keevoro_core::meter::totals().0.saturating_sub(before),
                    stopped,
                });
                // A turn that was stopped has said so itself.
                if stopped {
                    went.stopped = true;
                    break;
                }
            }
            Err(refused) => {
                writeln!(out, "Couldn't back up to {name}. {refused:#}")?;
                if refused.downcast_ref::<crate::room::NotEnoughRoom>().is_some() {
                    went.no_room += 1;
                } else {
                    went.unreachable += 1;
                }
                went.refused = Some(refused);
            }
        }
    }
    if went.reached > 0 {
        went.refused = None;
    }
    if let Some(made) = previews.as_ref() {
        let (photos, videos) = made.made();
        let (photos_without, videos_without) = crate::background::without_previews(catalog)?;
        went.previews = Some(crate::history::PreviewsMade {
            photos,
            videos,
            photos_without,
            videos_without,
        });
    }
    Ok(went)
}

/// Tells the storage to throw away the uploads in parts this backup
/// started there for files of this pass that it no longer sends (left
/// out since, or no longer found), so that their parts stop taking room,
/// and costing money in cloud storage. One for a file still being sent is
/// kept: the next backup carries it on. Only uploads this backup itself
/// started, as its own record remembers them, are ever touched.
fn abandon_unfinished(
    runtime: &tokio::runtime::Runtime,
    store: &dyn MultipartStore,
    catalog: &Catalog,
    target: &str,
    uploads: &[Upload],
    pass: Kind,
) -> Result<()> {
    let this_pass = if pass == Kind::SendingPreviews {
        "renditions/"
    } else {
        "originals/"
    };
    let still_sent: std::collections::HashSet<&str> = uploads.iter().map(|u| u.key.as_str()).collect();
    for (key, id) in catalog.multiparts(target)? {
        if key.starts_with(this_pass) && !still_sent.contains(key.as_str()) {
            // Gone already is as good as thrown away.
            let _ = runtime.block_on(store.abort(&keevoro_storage::UploadId(id)));
            catalog.set_multipart(target, &key, None)?;
        }
    }
    Ok(())
}

/// Clearing away less than this is done without a word on the screen.
const NOTICEABLE: u64 = 10 * 1024 * 1024;

/// What clearing away an earlier backup's unfinished copies says.
pub fn cleared_words(bytes: u64) -> String {
    format!(
        "Cleared {} of unfinished copies left by an earlier backup that was stopped. They were Keevoro's working \
         files, not part of your backup.",
        summary::bytes(bytes)
    )
}

/// Clears the abandoned working files from a folder destination's
/// `.keevoro-uploads` (see `keevoro_storage_folder::sweep_working_files`).
/// A folder that cannot be tidied is no reason to stop a backup, so a
/// problem here only means nothing was cleared.
pub fn tidy(folder: &Path) -> keevoro_storage_folder::Swept {
    keevoro_storage_folder::sweep_working_files(folder, keevoro_storage_folder::ABANDONED_AFTER).unwrap_or_default()
}

fn add_swept(total: &mut keevoro_storage_folder::Swept, more: keevoro_storage_folder::Swept) {
    total.files += more.files;
    total.bytes += more.bytes;
    total.in_use = more.in_use;
    total.not_ours = more.not_ours;
}

/// Everything this backup will do, in order, with how much of each there
/// is as far as can be known before it starts: the plan the estimate of
/// the time left is made from. What is not known yet (how many files look
/// alike, the size of previews not made) is filled in as it becomes known.
fn plan(catalog: &Catalog, home: &Path, all: &[crate::connect::Saved], tracker: &Tracker) -> Result<Vec<Planned>> {
    let found = crate::discover(home, catalog)?;
    let previews = wants_previews(catalog)?;
    let (photos, videos) = if previews {
        crate::background::still_to_make(catalog)?
    } else {
        (0, 0)
    };
    let per_original = tracker.preview_bytes();
    let mut planned = vec![
        Planned::new(Kind::TellingApart, None, 0),
        Planned::new(Kind::Placing, None, (found.photos + found.videos) as u64),
    ];
    for (at, to) in all.iter().enumerate() {
        let name = to.describe();
        let waiting = found.destinations.iter().find(|d| d.name == name);
        planned.push(Planned::new(
            Kind::SendingOriginals,
            Some(&name),
            waiting.map_or(found.waiting_bytes, |d| d.waiting_bytes),
        ));
        if previews {
            if at == 0 {
                planned.push(Planned::new(Kind::MakingPreviews, None, photos));
                planned.push(Planned::new(Kind::MakingVideoPreviews, None, videos));
            }
            let files = waiting.map_or(found.waiting_to_gather, |d| d.waiting) as f64;
            planned.push(Planned::new(
                Kind::SendingPreviews,
                Some(&name),
                (files * per_original) as u64,
            ));
        }
        planned.push(Planned::new(Kind::WritingIndex, Some(&name), 1));
    }
    Ok(planned)
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

/// Everything being kept that has a place agreed for it, as the catalog
/// stands at this moment.
fn kept_now<'a>(catalog: &Catalog, reports: &'a [keevoro_scan::ScanReport]) -> Result<Vec<Kept<'a>>> {
    let (unique, _) = summary::unique_items(reports);
    let mut kept = Vec::new();
    for item in unique {
        let Some(hash) = item.blake3.as_deref() else { continue };
        // A file the person has since unticked is not sent.
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

/// Sends everything that has its place and has not reached this
/// destination yet, then writes the index. `stop` stops it between files.
/// Answers how many files could not be sent.
#[allow(clippy::too_many_arguments)]
fn run_until(
    out: &mut dyn Write,
    catalog: &Catalog,
    home: &Path,
    destination: Destination,
    base_url: Option<&str>,
    stop: Option<&Stop>,
    on_progress: &mut dyn FnMut(&str),
    pass: Pass<'_>,
    tracker: &Tracker,
    record: &mut crate::history::DestinationRecord,
) -> Result<usize> {
    let reports = catalog.load_reports()?;
    let kept = kept_now(catalog, &reports)?;
    if kept.is_empty() {
        writeln!(out, "Nothing is being kept, so there is nothing to back up yet.")?;
        return Ok(0);
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
    record.over_network = over_network;
    let (store, target, default_url) = open(out, &runtime, destination)?;
    let store = store.as_ref();
    let base_url = base_url.map_or(default_url, str::to_string);

    let uploads = uploads_for(home, &kept, pass.sending)?;
    let n = summary::thousands;
    let opts = PublishOptions {
        stop: stop.cloned(),
        over_network,
        at_once,
        ..PublishOptions::default()
    };
    // What this pass has to send, now that it is known exactly.
    let mut pending = 0;
    for upload in &uploads {
        if !catalog.is_uploaded(&target, upload.key.as_str())? {
            pending += upload.bytes;
        }
    }
    if pass.sending == Kind::SendingPreviews {
        let with_previews = kept.iter().filter(|k| !k.renditions().is_empty()).count();
        if with_previews > 0 {
            tracker.note_preview_bytes(uploads.iter().map(|u| u.bytes).sum::<u64>() as f64 / with_previews as f64);
        }
    }
    tracker.set_work(pass.sending, Some(pass.to), pending);
    tracker.metered(pass.sending, Some(pass.to));
    let report = runtime.block_on(publish(store, catalog, &target, &uploads, &opts, &mut |p| {
        // After a tab: the file just dealt with, for a screen to show beneath.
        on_progress(&format!(
            "{} of {} safely backed up\t{}",
            n(p.done),
            n(p.total),
            p.latest
        ));
    }))?;
    tracker.finish(pass.sending, Some(pass.to));

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
    let key = match catalog.setting(&setting)? {
        Some(current) => current,
        None => fresh_key()?,
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
        stored
            .iter()
            .map(|k| {
                // Only previews that have arrived: the index never points
                // at something that is not there yet.
                let mut there = Vec::new();
                for kind in k.renditions() {
                    if catalog.is_uploaded(&target, &rendition_path(kind, k.id()))? {
                        there.push(kind);
                    }
                }
                Ok(item_for(k, there))
            })
            .collect::<Result<Vec<_>>>()?,
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
    if pass.last {
        tracker.progress(Kind::WritingIndex, Some(pass.to), 0);
    }
    for (name, body, kind) in documents {
        runtime
            .block_on(put_document(store, &ObjectKey::new(name)?, body, kind, &opts))
            .map_err(|e| anyhow::anyhow!("Couldn't write the index. {}.", keevoro_publish::plain_reason(&e)))?;
    }

    catalog.set_setting(&setting, &key)?;
    if pass.last {
        tracker.progress(Kind::WritingIndex, Some(pass.to), 1);
        tracker.finish(Kind::WritingIndex, Some(pass.to));
    }

    abandon_unfinished(&runtime, store, catalog, &target, &uploads, pass.sending)?;

    let sending = if pass.sending == Kind::SendingPreviews {
        record.previews.get_or_insert_with(Default::default)
    } else {
        &mut record.originals
    };
    sending.sent += report.uploaded;
    sending.sent_bytes += report.uploaded_bytes;
    sending.already_there += report.already_there;
    for f in &report.failed {
        if sending.not_sent.len() < crate::history::NOT_SENT_KEPT {
            sending.not_sent.push(crate::history::NotSent {
                file: f.key.to_string(),
                reason: f.reason.to_string(),
            });
        } else {
            sending.not_sent_more += 1;
        }
    }
    record.indexed = Some(index.items.len());

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
        writeln!(out, "Back up again to try them once more.")?;
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
    Ok(report.failed.len())
}

/// Confirms that every kept file sent to this destination is there, and
/// says so plainly. With `read_back`, each one is also read back from the
/// destination and compared exactly. Answers how many files need
/// attention. Nothing is changed.
pub fn check(
    out: &mut dyn Write,
    catalog: &Catalog,
    destination: Destination,
    read_back: bool,
    on_progress: &mut dyn FnMut(&str),
) -> Result<usize> {
    let n = summary::thousands;
    let files = catalog.collected()?;
    if files.is_empty() {
        writeln!(out, "Nothing has been backed up yet, so there is nothing to check.")?;
        return Ok(0);
    }
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
        writeln!(out, "Back up again to send anything that is missing.")?;
    }
    Ok(result.problems.len())
}

/// What one pass sends: the originals, or the previews. They are never
/// sent together, so no preview can hold up an original.
fn uploads_for(home: &Path, kept: &[Kept<'_>], sending: Kind) -> Result<Vec<Upload>> {
    let local = |relative: &str| relative.split('/').fold(home.to_path_buf(), |p, s| p.join(s));
    let mut uploads = Vec::new();
    if sending == Kind::SendingPreviews {
        for k in kept {
            for kind in k.renditions() {
                let relative = rendition_path(kind, k.id());
                let file = local(&relative);
                // A preview deleted from the backup's own folder is simply
                // left out; the next backup makes it again.
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
        return Ok(uploads);
    }
    for k in kept {
        uploads.push(Upload {
            key: ObjectKey::new(k.path.clone())?,
            // Straight from where it was found, only read.
            file: k.item.path.clone(),
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
    Ok(uploads)
}

fn item_for(k: &Kept<'_>, renditions: Vec<Rendition>) -> ItemInput {
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
        renditions,
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

    /// One destination's turn, as a backup with only that one has it.
    #[allow(clippy::too_many_arguments)]
    fn turn_alone(
        out: &mut dyn Write,
        catalog: &Catalog,
        home: &Path,
        destination: Destination,
        base_url: Option<&str>,
        video_tools: Option<&keevoro_media::VideoTools>,
        stop: &Stop,
        on_progress: &mut dyn FnMut(&str),
    ) -> Result<usize> {
        let tracker = Tracker::new(keevoro_core::estimate::Remembered::default());
        let mut previews = None;
        let mut record = crate::history::DestinationRecord::new("the folder");
        originals_first(
            out,
            catalog,
            home,
            destination,
            base_url,
            video_tools,
            &mut Going {
                stop,
                tracker: &tracker,
                to: "the folder".into(),
                previews: &mut previews,
                record: &mut record,
            },
            on_progress,
        )
    }

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
        turn_alone(
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
    fn what_is_safely_backed_up_is_counted_by_folder_by_the_same_rule_as_what_waits() {
        let (dir, card, home, catalog) = one_backup();
        let flights = std::fs::canonicalize(card.join("Flights")).unwrap();
        // Kept, and nowhere yet.
        let before = crate::safe_by_folder(&catalog).unwrap();
        assert_eq!(before.get(&flights), Some(&(0, 2)), "{before:?}");
        // Backed up: safe, exactly as what is waiting says.
        send(&catalog, &home, &dir.path().join("usb"));
        assert_eq!(crate::safe_by_folder(&catalog).unwrap()[&flights], (2, 2));
        assert_eq!(crate::discover(&home, &catalog).unwrap().waiting_to_gather, 0);
        // Nothing in the source was touched by counting.
        assert_eq!(std::fs::read_dir(card.join("Flights")).unwrap().count(), 2);
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
                &Tracker::new(keevoro_core::estimate::Remembered::default()),
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
    fn the_estimate_follows_every_stage_of_the_backup_to_its_end() {
        use crate::connect::{self, testing::MemorySecrets, Flags, Saved, World};
        use keevoro_core::estimate::{Kind, State};
        let (dir, _card, home, catalog) = one_backup();
        let secrets = MemorySecrets::default();
        let world = World {
            secrets: &secrets,
            ask: &connect::testing::nobody_to_ask,
        };
        let to = Saved::from_flags(&Flags {
            to_folder: Some(dir.path().join("usb")),
            ..Flags::default()
        })
        .unwrap();
        connect::add(&mut Vec::new(), &catalog, &to, &world).unwrap();
        let tracker = Tracker::new(keevoro_core::estimate::Remembered::default());
        let mut seen = Vec::new();
        everywhere(
            &mut Vec::new(),
            &catalog,
            &home,
            &world,
            None,
            &Stop::default(),
            &tracker,
            &mut |_| seen.push(tracker.estimate()),
            &mut |_| {},
        )
        .unwrap();
        // From the very start, the previews were in the plan.
        let first = &seen[0];
        assert!(
            first.stages.iter().any(|s| s.kind == Kind::SendingPreviews),
            "{:?}",
            first.stages
        );
        assert!(first
            .stages
            .iter()
            .any(|s| s.kind == Kind::MakingPreviews && s.work == 2));
        // And at the end every stage is done, the whole backup counted.
        let end = tracker.estimate();
        let kinds: Vec<Kind> = end.stages.iter().map(|s| s.kind).collect();
        assert_eq!(
            kinds,
            [
                Kind::Placing,
                Kind::SendingOriginals,
                Kind::MakingPreviews,
                Kind::SendingPreviews,
                Kind::WritingIndex
            ]
        );
        assert!(end.stages.iter().all(|s| s.state == State::Done), "{:?}", end.stages);
        assert_eq!(end.fraction_done, 1.0);
        // The size of previews was measured, for the next estimate.
        assert!(tracker.measured().preview_bytes_per_original.is_some());
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
        let went = everywhere(
            &mut said,
            &catalog,
            &home,
            &world,
            None,
            &Stop::default(),
            &Tracker::new(keevoro_core::estimate::Remembered::default()),
            &mut |_| {},
            &mut |_| turns += 1,
        )
        .unwrap();
        let said = String::from_utf8(said).unwrap();
        assert!(said.contains("Couldn't back up to the folder"), "{said}");
        assert_eq!(turns, 1, "the one that works had its turn");
        assert_eq!(
            (
                went.reached,
                went.unreachable,
                went.no_room,
                went.failed_files,
                went.stopped
            ),
            (1, 1, 0, 0, false)
        );
        assert!(went.refused.is_none(), "one was backed up to");
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
        turn_alone(
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
        turn_alone(
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
        // Only the new photo, then its two previews; the two originals from
        // before, and their four previews, are not sent again.
        assert!(later.contains("Safely backed up 1 file "), "{later}");
        assert!(later.contains("2 files already there"), "{later}");
        assert!(later.contains("Previews added: 2 files"), "{later}");
        assert!(to
            .join("originals")
            .join("Flights")
            .join("IMG_20240702_090000.jpg")
            .is_file());
    }

    #[test]
    fn the_files_themselves_are_sent_before_any_preview_is_sent() {
        let (dir, _card, home, catalog) = one_backup();
        let to = dir.path().join("Drone footage");
        let mut lines = Vec::new();
        let mut said = Vec::new();
        turn_alone(
            &mut said,
            &catalog,
            &home,
            Destination::Folder(to.clone()),
            None,
            None,
            &Stop::default(),
            &mut |line| {
                // Note, each time something is said, whether both originals have arrived.
                let arrived = ["IMG_20240601_100000.jpg", "IMG_20240601_100500.jpg"]
                    .iter()
                    .all(|name| to.join("originals").join("Flights").join(name).is_file());
                lines.push((line.to_string(), arrived));
            },
        )
        .unwrap();
        // Previews are made in the background meanwhile, but the first one
        // sent goes after every original has arrived.
        let first_preview = lines
            .iter()
            .position(|(line, _)| line.contains("\trenditions/"))
            .unwrap();
        assert!(
            lines[first_preview].1,
            "the originals were already there when previews were sent"
        );
        assert!(lines[..first_preview]
            .iter()
            .any(|(line, _)| line.contains("safely backed up\toriginals/")));
        assert!(
            !lines[first_preview..]
                .iter()
                .any(|(line, _)| line.contains("\toriginals/")),
            "no original after a preview: {lines:?}"
        );

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
    fn a_backup_stopped_before_its_first_destination_says_it_was_stopped() {
        use crate::connect::{self, testing::MemorySecrets, Flags, Saved, World};
        let (dir, _card, home, catalog) = one_backup();
        let secrets = MemorySecrets::default();
        let world = World {
            secrets: &secrets,
            ask: &connect::testing::nobody_to_ask,
        };
        let to = dir.path().join("usb");
        let folder = Saved::from_flags(&Flags {
            to_folder: Some(to.clone()),
            ..Flags::default()
        })
        .unwrap();
        connect::add(&mut Vec::new(), &catalog, &folder, &world).unwrap();
        // Asked to stop while it was still getting ready.
        let stop = Stop::default();
        stop.store(true, Ordering::SeqCst);
        let mut said = Vec::new();
        let went = everywhere(
            &mut said,
            &catalog,
            &home,
            &world,
            None,
            &stop,
            &Tracker::new(keevoro_core::estimate::Remembered::default()),
            &mut |_| {},
            &mut |_| {},
        )
        .unwrap();
        assert_eq!(String::from_utf8(said).unwrap().trim_end(), STOPPED);
        assert!(went.stopped && went.reached == 0 && went.refused.is_none());
        assert!(!to.join("originals").exists(), "nothing was sent");
    }

    #[test]
    fn unfinished_copies_an_earlier_backup_left_are_cleared_and_said_and_a_live_one_is_not() {
        use crate::connect::{self, testing::MemorySecrets, Flags, Saved, World};
        let (dir, _card, home, catalog) = one_backup();
        let secrets = MemorySecrets::default();
        let world = World {
            secrets: &secrets,
            ask: &connect::testing::nobody_to_ask,
        };
        let to = dir.path().join("usb");
        let folder = Saved::from_flags(&Flags {
            to_folder: Some(to.clone()),
            ..Flags::default()
        })
        .unwrap();
        connect::add(&mut Vec::new(), &catalog, &folder, &world).unwrap();
        let work = to.join(".keevoro-uploads");
        std::fs::create_dir_all(work.join("18e866207c01f87097a80")).unwrap();
        let aged = |path: &Path, bytes: usize, seconds_ago: u64| {
            std::fs::write(path, vec![7u8; bytes]).unwrap();
            let file = std::fs::File::options().write(true).open(path).unwrap();
            file.set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(seconds_ago))
                .unwrap();
        };
        // Parts of a large video from a backup stopped two days ago.
        aged(
            &work.join("18e866207c01f87097a80/part-00001"),
            12 * 1024 * 1024,
            2 * 86_400,
        );
        aged(&work.join("18e866207c01f87097a80/target"), 20, 2 * 86_400);
        // Another program copying to the same folder this very moment.
        aged(&work.join("tmp-someone-elses"), 1000, 0);

        let mut said = Vec::new();
        let went = everywhere(
            &mut said,
            &catalog,
            &home,
            &world,
            None,
            &Stop::default(),
            &Tracker::new(keevoro_core::estimate::Remembered::default()),
            &mut |_| {},
            &mut |_| {},
        )
        .unwrap();
        let said = String::from_utf8(said).unwrap();
        assert!(said.contains(&cleared_words(12 * 1024 * 1024 + 20)), "{said}");
        assert!(said.contains("Cleared 12.0 MB of unfinished copies left by an earlier backup that was stopped."));
        assert!(!work.join("18e866207c01f87097a80").exists());
        assert!(work.join("tmp-someone-elses").is_file(), "a live copy is never touched");
        assert_eq!(went.destinations[0].cleared_bytes, 12 * 1024 * 1024 + 20);
        assert!(to.join("originals").is_dir(), "and the backup itself went ahead");
    }

    #[test]
    fn an_upload_in_parts_for_a_file_no_longer_sent_is_thrown_away_and_one_still_wanted_is_kept() {
        use crate::connect::{self, testing::MemorySecrets, Flags, Saved, World};
        use keevoro_storage::MultipartStore as _;
        let (dir, _card, home, catalog) = one_backup();
        let secrets = MemorySecrets::default();
        let world = World {
            secrets: &secrets,
            ask: &connect::testing::nobody_to_ask,
        };
        let to = dir.path().join("usb");
        let folder = Saved::from_flags(&Flags {
            to_folder: Some(to.clone()),
            ..Flags::default()
        })
        .unwrap();
        connect::add(&mut Vec::new(), &catalog, &folder, &world).unwrap();
        let target = format!("folder:{}", std::fs::canonicalize(&to).unwrap().display());
        // Two uploads this backup started earlier: one for a file it has
        // stopped keeping, and one in a part of the backup not sent now.
        let runtime = tokio::runtime::Builder::new_current_thread().build().unwrap();
        let store = runtime.block_on(FolderStore::open(&to)).unwrap();
        let start = |key: &str| {
            let id = runtime
                .block_on(store.start(&ObjectKey::new(key).unwrap(), &keevoro_storage::PutOptions::default()))
                .unwrap();
            catalog.set_multipart(&target, key, Some(&id.0)).unwrap();
            id
        };
        let gone = start("originals/Left out/big.mov");
        let elsewhere = start("someone/else.mov");

        everywhere(
            &mut Vec::new(),
            &catalog,
            &home,
            &world,
            None,
            &Stop::default(),
            &Tracker::new(keevoro_core::estimate::Remembered::default()),
            &mut |_| {},
            &mut |_| {},
        )
        .unwrap();
        let left: Vec<String> = catalog
            .multiparts(&target)
            .unwrap()
            .into_iter()
            .map(|(k, _)| k)
            .collect();
        assert_eq!(left, ["someone/else.mov"]);
        assert!(
            !to.join(".keevoro-uploads").join(&gone.0).exists(),
            "its parts are gone"
        );
        assert!(to.join(".keevoro-uploads").join(&elsewhere.0).exists());
    }

    #[test]
    fn a_destination_without_room_is_told_apart_from_one_out_of_reach() {
        let refused: anyhow::Error = crate::room::NotEnoughRoom("There isn't enough room.".into()).into();
        assert!(refused.downcast_ref::<crate::room::NotEnoughRoom>().is_some());
        assert_eq!(format!("{refused:#}"), "There isn't enough room.");
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
        turn_alone(
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
