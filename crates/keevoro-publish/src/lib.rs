//! The upload engine.
//!
//! Storage providers only move bytes. Everything that makes an upload safe
//! and restartable lives here, written once and shared by every provider:
//!
//! - **Originals are write-once.** An original is never replaced in the
//!   bucket. If its name is already taken by the same file, that counts as
//!   done; if it is taken by a different file, the upload is refused and
//!   reported. Previews and the index can be replaced, because they can
//!   always be made again.
//! - **Everything resumes.** Each finished object is recorded in the
//!   catalog as it lands. Large files go up in parts, and the provider's
//!   upload id is saved before the first part, so after a crash, a reboot
//!   or a dropped connection only the missing parts are sent.
//! - **One bad file never stops the rest.** Problems are collected into the
//!   report with a plain reason.
//! - **A file that fails is tried again at the end, never on the spot.**
//!   One that fails for a reason that may pass (a dropped connection, a file
//!   in use) goes on a list and the pass carries on; at its end each is
//!   tried once more. A place that fails every file is asked whether it is
//!   still there, and if not, nothing more is started there until the end.
//!   Anything else (wrong key, no permission, no room) is reported straight
//!   away, because trying again would not help. Each failure carries its
//!   side and cause (`keevoro_core::trouble`), so it is said truthfully.
//!
//! The engine knows nothing about S3, R2 or folders. It sees only the
//! traits in `keevoro-storage`, which is what lets one test suite cover
//! every provider.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

use futures_util::stream::{FuturesUnordered, StreamExt};
use keevoro_catalog::{Catalog, CatalogError};
use keevoro_core::trouble::{self, Cause, Side, Try};
use keevoro_storage::{
    ByteSource, MultipartStore, ObjectKey, ObjectStore, PartReceipt, PutOptions, Step, StorageError, UploadId,
};
use serde::Serialize;
use std::collections::BTreeMap;
use std::future::Future;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// One file to send.
#[derive(Debug, Clone)]
pub struct Upload {
    pub key: ObjectKey,
    pub file: PathBuf,
    pub bytes: u64,
    /// True for originals: never replace what is already stored.
    pub write_once: bool,
    pub content_type: Option<String>,
    /// Stored on the object where the provider supports it, for example
    /// the BLAKE3 hash.
    pub metadata: BTreeMap<String, String>,
    /// True for a file whose whole fingerprint is not known yet (a search
    /// only sampled it). It is fingerprinted as it is sent, in the same
    /// pass over the file, and the answer comes back in the report, so the
    /// file never has to be read once to fingerprint it and again to send.
    pub learn_fingerprint: bool,
    /// For a file in cloud storage (a bucket source), how to bring it to
    /// this computer first: `file` is then only its name, never opened.
    /// It is fetched as its turn comes, sent from there, and the copy here
    /// removed, so no more are on this computer at once than are being
    /// sent at once.
    pub fetch: Option<std::sync::Arc<dyn Fetch>>,
}

/// Brings one file from where it is (a bucket source) to this computer, to
/// be sent from there.
#[async_trait::async_trait]
pub trait Fetch: Send + Sync + std::fmt::Debug {
    async fn fetch(&self) -> Result<Fetched, StorageError>;
}

/// A file fetched to this computer, and its fingerprint, worked out as it
/// was written. The copy is removed when this is dropped, however sending
/// it went.
#[derive(Debug)]
pub struct Fetched {
    pub file: PathBuf,
    pub bytes: u64,
    pub blake3: String,
}

impl Drop for Fetched {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.file);
    }
}

#[derive(Debug, Clone)]
pub struct PublishOptions {
    /// Files larger than this go up in parts.
    pub multipart_above: u64,
    /// Preferred part size; adjusted to fit the provider's limits.
    pub part_bytes: u64,
    /// Extra attempts after a network drop.
    pub retries: u32,
    /// Pause before the first retry; doubled each time.
    pub retry_pause: Duration,
    /// Set to true from anywhere to stop: the file being sent is finished,
    /// and no more are started. Everything already sent stays sent.
    pub stop: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    /// True when the destination is reached over the network (cloud
    /// storage), false for a folder. Only affects how what is sent is
    /// counted for the speed display.
    pub over_network: bool,
    /// How many files are sent at the same time. Over the network most of
    /// the time for a small file goes on waiting for the far end to answer,
    /// so sending several at once is several times quicker. To a disk it
    /// only makes the disk hop about, so there it stays at one.
    pub at_once: usize,
    /// A cheap question whether the place is still there (a folder still a
    /// folder), asked only once files start failing one after another. When
    /// it says no, no more files are started there in this pass; at the
    /// end it is asked again, and the rest are sent if it is back.
    pub reachable: Option<Reachable>,
}

/// Whether a place is still there. See [`PublishOptions::reachable`].
#[derive(Clone)]
pub struct Reachable(pub std::sync::Arc<dyn Fn() -> bool + Send + Sync>);

impl std::fmt::Debug for Reachable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Reachable")
    }
}

impl Default for PublishOptions {
    fn default() -> Self {
        Self {
            multipart_above: 64 * 1024 * 1024,
            part_bytes: 16 * 1024 * 1024,
            retries: 3,
            retry_pause: Duration::from_secs(2),
            stop: None,
            over_network: false,
            at_once: 1,
            reachable: None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct FailedUpload {
    pub key: String,
    /// In words, with the operating system's own text in brackets.
    pub reason: String,
    /// Why, as one of a few causes a person can act on, and which side it
    /// was on, when that is known: what grouped sentences are made from.
    pub cause: Cause,
    pub side: Option<Side>,
}

impl FailedUpload {
    /// A failure, with its cause worked out from the error.
    pub fn of(key: &str, e: &StorageError, across_a_network: bool, was_working: bool) -> Self {
        let (side, cause) = cause_of(e, across_a_network, was_working);
        Self {
            key: key.to_string(),
            reason: plain_reason(e),
            cause,
            side,
        }
    }
}

/// Which side a storage problem was on and what it means, as
/// `keevoro_core::trouble` says causes. The operating system's code, when
/// there is one, goes through the one table.
pub fn cause_of(e: &StorageError, across_a_network: bool, was_working: bool) -> (Option<Side>, Cause) {
    let side = match e.step() {
        Some(step) if !step.at_the_destination() => Some(Side::Source),
        Some(_) => Some(Side::Destination),
        None => match e {
            StorageError::AccessDenied(_)
            | StorageError::Network(_)
            | StorageError::AlreadyExists(_)
            | StorageError::Provider(_) => Some(Side::Destination),
            _ => None,
        },
    };
    let cause = match e {
        StorageError::Network(_) => Cause::ConnectionLost,
        StorageError::AccessDenied(_) => Cause::NotAllowed,
        StorageError::AlreadyExists(_) => Cause::DifferentFileThere,
        StorageError::NotTheSame(why) if why.starts_with("the file is not as it was") => Cause::ChangedSinceLooked,
        StorageError::NotTheSame(_) => Cause::NotTheSame,
        StorageError::Provider(said) if said.starts_with("There isn't room") => Cause::NoRoom,
        _ => match e.io() {
            Some(io) => match io.raw_os_error() {
                Some(code) => trouble::cause_of_code(trouble::System::this(), code, across_a_network, was_working),
                None => match io.kind() {
                    std::io::ErrorKind::NotFound => Cause::NotFound,
                    std::io::ErrorKind::PermissionDenied => Cause::NotAllowed,
                    std::io::ErrorKind::StorageFull => Cause::NoRoom,
                    std::io::ErrorKind::TimedOut
                    | std::io::ErrorKind::ConnectionReset
                    | std::io::ErrorKind::ConnectionAborted
                    | std::io::ErrorKind::NotConnected
                    | std::io::ErrorKind::BrokenPipe => Cause::ConnectionLost,
                    _ => Cause::Other,
                },
            },
            None => Cause::Other,
        },
    };
    // Keevoro makes the folders it writes into, so a destination that says
    // "not found" part-way through a run that was working has gone away.
    let cause = if cause == Cause::NotFound && side == Some(Side::Destination) && was_working {
        Cause::ConnectionLost
    } else {
        cause
    };
    (side, cause)
}

#[derive(Debug, Default, Serialize)]
pub struct PublishReport {
    /// Files fetched from cloud storage that turned out, once read, to be
    /// exact copies of a file already given its place: not sent again.
    pub copies_left_out: usize,
    pub uploaded: usize,
    pub uploaded_bytes: u64,
    pub already_there: usize,
    /// Of those uploaded, how many Keevoro had sent before and a look at
    /// the storage had found gone: sent again, so back where they were.
    pub restored: usize,
    pub failed: Vec<FailedUpload>,
    /// True when it was asked to stop before every file had been tried.
    pub stopped: bool,
    /// Files not tried at all because it was asked to stop first.
    pub not_tried: usize,
    /// Files that failed for a reason that may pass and were tried once
    /// more at the end of the pass, however that went.
    pub tried_again: usize,
    /// The place stopped answering part-way and was not back by the end:
    /// the files never tried there because of it, by key.
    pub waiting_for_place: Vec<String>,
    /// The fingerprints worked out while sending, for uploads that asked:
    /// the key each went under, and its whole-file BLAKE3.
    pub learned: Vec<(String, String)>,
}

/// Where every file a pass was given ended up. The one sum a closing
/// summary is made from, so what it says always adds up: every file given
/// is in exactly one of these.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    /// Every file the pass was given.
    pub given: usize,
    /// There before, as this backup's look found, or found there exact.
    pub already_there: usize,
    /// Sent now, and proved.
    pub sent: usize,
    /// Exact copies of a file already being backed up: not sent twice.
    pub copies_left_out: usize,
    /// Tried, and could not be sent.
    pub failed: usize,
    /// Not tried, because the pass was stopped first.
    pub not_tried: usize,
    /// Not tried, because the place went away part-way and was not back.
    pub waiting_for_place: usize,
}

impl Account {
    /// Safely there now, one way or another.
    pub fn safe(&self) -> usize {
        self.already_there + self.sent + self.copies_left_out
    }

    /// Still needing something done.
    pub fn attention(&self) -> usize {
        self.failed + self.not_tried + self.waiting_for_place
    }

    /// Does every file given have exactly one place in the sum?
    pub fn adds_up(&self) -> bool {
        self.safe() + self.attention() == self.given
    }
}

impl PublishReport {
    /// Where each of the `given` files ended up.
    pub fn account(&self, given: usize) -> Account {
        Account {
            given,
            already_there: self.already_there,
            sent: self.uploaded,
            copies_left_out: self.copies_left_out,
            failed: self.failed.len(),
            not_tried: self.not_tried,
            waiting_for_place: self.waiting_for_place.len(),
        }
    }
}

/// Counts for "3,412 of 9,870 safely backed up".
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PublishProgress {
    pub done: usize,
    pub total: usize,
    /// The file most recently dealt with, as it is named in the storage,
    /// so a person can see what is going by. Empty before the first.
    pub latest: String,
}

enum Outcome {
    /// Sent now: the size that arrived, and its fingerprint if that was
    /// asked for and found.
    Sent(u64, Option<String>),
    AlreadyThere,
    /// Fetched and found to be an exact copy of a file already given its
    /// place (whose fingerprint this is), so not sent.
    Copy(String),
}

/// Sends `uploads` to `store`, skipping whatever the catalog says is
/// already there. `target` is a stable name for this storage place; it
/// keeps the records of two different buckets apart.
pub async fn publish(
    store: &dyn MultipartStore,
    catalog: &Catalog,
    target: &str,
    uploads: &[Upload],
    opts: &PublishOptions,
    on_progress: &mut dyn FnMut(&PublishProgress),
) -> Result<PublishReport, CatalogError> {
    let _busy = keevoro_core::doing::doing(keevoro_core::doing::Work::BackingUp);
    let mut report = PublishReport::default();
    let mut progress = PublishProgress {
        done: 0,
        total: uploads.len(),
        latest: String::new(),
    };
    let stop_asked = || {
        opts.stop
            .as_ref()
            .is_some_and(|stop| stop.load(std::sync::atomic::Ordering::SeqCst))
    };
    // Up to `at_once` files are on their way at any moment. They take
    // turns on this one task, so the catalog is still only ever touched
    // from here, one thing at a time.
    // The fingerprints of files fetched and sent in this pass, by where
    // each went, so a second copy fetched later is known for one.
    let sent_now = std::sync::Mutex::new(std::collections::HashMap::<String, String>::new());
    let sent_now = &sent_now;
    let mut waiting = uploads.iter();
    let mut in_flight = FuturesUnordered::new();
    // Files being fetched to this computer first, never more than
    // FETCHED_AT_ONCE of them on its disk at once.
    let mut fetching = 0;
    // What happened to the last few files, to notice a place that has
    // gone; the files that failed for a reason that may pass, to try once
    // more at the end, in order; and those not started because the place
    // seemed gone.
    let mut recent: Vec<Try> = Vec::new();
    let mut later: Vec<(&Upload, bool, FailedUpload)> = Vec::new();
    let mut held: Vec<&Upload> = Vec::new();
    let mut gone = false;
    loop {
        while in_flight.len() < opts.at_once.max(1) {
            if fetching >= FETCHED_AT_ONCE && waiting.as_slice().first().is_some_and(|u| u.fetch.is_some()) {
                break;
            }
            // Pausing holds back new files; those already going finish.
            while keevoro_core::pause::is_paused() && !stop_asked() {
                tokio::time::sleep(Duration::from_millis(150)).await;
            }
            if stop_asked() {
                report.stopped |= waiting.len() > 0;
                report.not_tried += waiting.len();
                waiting = [].iter();
                break;
            }
            if gone {
                held.extend(waiting.by_ref());
                break;
            }
            let Some(upload) = waiting.next() else { break };
            // Already there only by Keevoro's record, which a look at the
            // storage keeps true; a file found gone has a record that says so.
            let recorded = catalog.sent_state(target, upload.key.as_str())?;
            if recorded == Some(keevoro_catalog::Seen::There) {
                report.already_there += 1;
                progress.done += 1;
                progress.latest = upload.key.to_string();
                on_progress(&progress);
                continue;
            }
            let was_gone = recorded.is_some();
            fetching += usize::from(upload.fetch.is_some());
            in_flight.push(async move {
                (
                    upload,
                    was_gone,
                    send_one(store, catalog, target, upload, opts, sent_now).await,
                )
            });
        }
        let Some((upload, was_gone, sent)) = in_flight.next().await else {
            break;
        };
        fetching -= usize::from(upload.fetch.is_some());
        match sent {
            Err(SendError::Catalog(e)) => return Err(e),
            Err(SendError::Storage(e)) => {
                let was_working = report.uploaded + report.already_there > 0;
                let failed = FailedUpload::of(upload.key.as_str(), &e, opts.over_network, was_working);
                recent.push(Try::Failed(failed.cause));
                // Not waited on and tried again now: it goes on the list
                // for the end, and the run carries on with everything else.
                if failed.cause.may_pass() {
                    later.push((upload, was_gone, failed));
                } else {
                    report.failed.push(failed);
                }
                if trouble::place_seems_gone(&recent) && opts.reachable.as_ref().is_some_and(|r| !(r.0)()) {
                    gone = true;
                }
            }
            Ok(outcome) => {
                recent.push(Try::Worked);
                record(catalog, target, upload, was_gone, outcome, &mut report)?;
            }
        }
        if recent.len() > trouble::GIVE_UP_AFTER {
            recent.remove(0);
        }
        progress.done += 1;
        progress.latest = upload.key.to_string();
        on_progress(&progress);
    }

    // At the end, once: by now a share that dropped out has usually come
    // back. If the place went away part-way, it is asked first whether it
    // is back; if not, what is left waits for the next backup.
    let back = !gone || opts.reachable.as_ref().is_none_or(|r| (r.0)());
    if !back {
        report.waiting_for_place = held.iter().map(|u| u.key.to_string()).collect();
        report.failed.extend(later.into_iter().map(|(_, _, failed)| failed));
        return Ok(report);
    }
    let mut again: Vec<(&Upload, bool, Option<FailedUpload>)> = later
        .into_iter()
        .map(|(u, was_gone, failed)| (u, was_gone, Some(failed)))
        .collect();
    again.extend(held.into_iter().map(|u| (u, false, None)));
    for (upload, was_gone, first) in again {
        if stop_asked() {
            match first {
                Some(failed) => report.failed.push(failed),
                None => report.not_tried += 1,
            }
            continue;
        }
        let was_gone = was_gone || catalog.sent_state(target, upload.key.as_str())?.is_some();
        report.tried_again += usize::from(first.is_some());
        match send_one(store, catalog, target, upload, opts, sent_now).await {
            Err(SendError::Catalog(e)) => return Err(e),
            Err(SendError::Storage(e)) => {
                let was_working = report.uploaded + report.already_there > 0;
                report.failed.push(FailedUpload::of(
                    upload.key.as_str(),
                    &e,
                    opts.over_network,
                    was_working,
                ));
            }
            Ok(outcome) => record(catalog, target, upload, was_gone, outcome, &mut report)?,
        }
        if first.is_none() {
            progress.done += 1;
            progress.latest = upload.key.to_string();
            on_progress(&progress);
        }
    }
    Ok(report)
}

/// Writes down a file that was sent, or found there, or left out as a copy.
fn record(
    catalog: &Catalog,
    target: &str,
    upload: &Upload,
    was_gone: bool,
    outcome: Outcome,
    report: &mut PublishReport,
) -> Result<(), CatalogError> {
    if let Outcome::Copy(whole) = outcome {
        if upload.learn_fingerprint {
            report.learned.push((upload.key.to_string(), whole));
        }
        report.copies_left_out += 1;
        return Ok(());
    }
    // The size recorded is the size that arrived, which a look compares
    // with what it finds.
    let arrived = match &outcome {
        Outcome::Sent(bytes, _) => *bytes,
        _ => upload.bytes,
    };
    catalog.mark_uploaded(target, upload.key.as_str(), arrived)?;
    match outcome {
        Outcome::Sent(_, fingerprint) => {
            if let Some(fingerprint) = fingerprint {
                report.learned.push((upload.key.to_string(), fingerprint));
            }
            report.uploaded += 1;
            report.uploaded_bytes += upload.bytes;
            report.restored += usize::from(was_gone);
        }
        Outcome::AlreadyThere => report.already_there += 1,
        Outcome::Copy(_) => {}
    }
    Ok(())
}

/// Stores a small document that may be replaced, such as the index.
pub async fn put_document(
    store: &dyn ObjectStore,
    key: &ObjectKey,
    body: Vec<u8>,
    content_type: &str,
    opts: &PublishOptions,
) -> Result<(), StorageError> {
    let put = PutOptions {
        create_only: false,
        expect_blake3: None,
        learn_blake3: false,
        content_type: Some(content_type.to_string()),
        metadata: BTreeMap::new(),
    };
    with_retries(opts, || store.put(key, ByteSource::Bytes(body.clone()), &put)).await?;
    Ok(())
}

/// The most files brought to this computer from cloud storage at once, to
/// be sent on: whatever is being sent at once, never more than this is on
/// its disk.
pub const FETCHED_AT_ONCE: usize = 2;

/// Reading a file back means holding it in memory. Files larger than this
/// (long videos) are checked by presence and size only.
pub const MAX_READ_BACK_BYTES: u64 = 512 * 1024 * 1024;

/// One original as it should be in the storage.
#[derive(Debug, Clone)]
pub struct Expected {
    pub key: ObjectKey,
    pub bytes: u64,
    /// Full BLAKE3 hash, hex.
    pub blake3: String,
}

#[derive(Debug, Default, Serialize)]
pub struct BackupCheck {
    /// Originals found with the right size.
    pub present: usize,
    /// Of those, how many were also downloaded and matched exactly.
    pub read_back: usize,
    pub problems: Vec<FailedUpload>,
    /// What was found of each original that could be looked for: there,
    /// missing, or not as it was sent (another size, or other contents).
    /// One that could not be asked about (the connection dropped) is not
    /// here, so it is never written down as gone.
    #[serde(skip)]
    pub seen: Vec<(String, keevoro_catalog::Seen)>,
}

/// Checks that originals are really in the storage. Every one is looked up
/// and its size compared. With `read_back`, each is also downloaded and its
/// contents compared with the hash from when it was gathered, which is the
/// only way to be certain and costs a full download.
pub async fn check_backup(
    store: &dyn ObjectStore,
    expected: &[Expected],
    read_back: bool,
    opts: &PublishOptions,
    on_progress: &mut dyn FnMut(&PublishProgress),
) -> BackupCheck {
    let mut check = BackupCheck::default();
    let mut progress = PublishProgress {
        done: 0,
        total: expected.len(),
        latest: String::new(),
    };
    use keevoro_catalog::Seen;
    for want in expected {
        let key = want.key.as_str();
        let wrong = |reason: &str, cause: Cause| FailedUpload {
            key: key.to_string(),
            reason: reason.to_string(),
            cause,
            side: Some(Side::Destination),
        };
        let (problem, seen) = match with_retries(opts, || store.head(&want.key)).await {
            Ok(None) => (
                Some(wrong("It is missing from the backup", Cause::NotFound)),
                Some(Seen::Missing),
            ),
            Ok(Some(found)) if found.bytes != want.bytes => (
                Some(wrong(
                    "The backup's copy is a different size from the original",
                    Cause::NotTheSame,
                )),
                Some(Seen::NotAsSent),
            ),
            Ok(Some(_)) if read_back && want.bytes <= MAX_READ_BACK_BYTES => {
                match with_retries(opts, || store.get(&want.key)).await {
                    Ok(bytes) if blake3_hex(&bytes) == want.blake3 => {
                        check.read_back += 1;
                        (None, Some(Seen::There))
                    }
                    Ok(_) => (
                        Some(wrong(
                            "The backup's copy does not match the original",
                            Cause::NotTheSame,
                        )),
                        Some(Seen::NotAsSent),
                    ),
                    Err(e) => (Some(FailedUpload::of(key, &e, opts.over_network, true)), None),
                }
            }
            Ok(Some(_)) => (None, Some(Seen::There)),
            Err(e) => (Some(FailedUpload::of(key, &e, opts.over_network, true)), None),
        };
        if let Some(seen) = seen {
            check.seen.push((want.key.to_string(), seen));
        }
        match problem {
            None => check.present += 1,
            Some(failed) => check.problems.push(failed),
        }
        progress.done += 1;
        on_progress(&progress);
    }
    check
}

fn blake3_hex(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}

/// How the messages a store writes for a person begin, so they are passed
/// on as they are.
const ALREADY_SAID: [&str; 8] = [
    "That doesn't look like",
    "The storage address",
    "There isn't room",
    "Keevoro needs the storage keys",
    "Enter only the bucket's name",
    "Google Cloud Storage needs an HMAC key",
    "These keys can't be right",
    "The storage answered in a way Keevoro could not read",
];

/// Says a storage problem in words a non-technical person can act on.
pub fn plain_reason(e: &StorageError) -> String {
    match e {
        StorageError::AccessDenied(_) => {
            "The storage refused access. Check that the key is right and is allowed to write".into()
        }
        StorageError::Network(_) => "The connection kept dropping. It will be tried again next time".into(),
        StorageError::AlreadyExists(_) => {
            "A different file is already stored under this name, and Keevoro never replaces an original".into()
        }
        StorageError::At { step, source } => match step {
            Step::ReadingTheSource => format!("Couldn't read it from its source ({source})"),
            Step::Writing | Step::Flushing => format!("Couldn't write the copy at the backup ({source})"),
            Step::MovingIntoPlace => format!("Couldn't give the copy its name at the backup ({source})"),
            Step::ReadingBack => format!("Couldn't read the copy back from the backup to prove it ({source})"),
        },
        // Which side it was on is not known, so neither is named.
        StorageError::Io(io) => format!("The disk or the connection reported a problem ({io})"),
        StorageError::NotTheSame(why) => format!(
            "The copy was not the same as the original, so it was not kept ({why}). It will be tried again next time"
        ),
        // Messages already written for the user, such as a mistyped
        // account ID, are passed on as they are.
        StorageError::Provider(said) if ALREADY_SAID.iter().any(|start| said.starts_with(start)) => said.clone(),
        other => format!("The storage reported a problem ({other})"),
    }
}

enum SendError {
    Storage(StorageError),
    Catalog(CatalogError),
}

impl From<StorageError> for SendError {
    fn from(e: StorageError) -> Self {
        Self::Storage(e)
    }
}

impl From<CatalogError> for SendError {
    fn from(e: CatalogError) -> Self {
        Self::Catalog(e)
    }
}

/// Sends one upload, fetching it first when it is in cloud storage.
///
/// A fetched file is checked before it is sent: if Keevoro labelled it
/// with its fingerprint when it backed it up, what was read must match;
/// and if it is an exact copy of a file already given its place (in the
/// catalog, or sent in this pass), it is not sent again. Then it is sent
/// as any file is, from the copy here, labelled with the fingerprint
/// worked out as it was fetched, so a folder proves its copy against it.
async fn send_one(
    store: &dyn MultipartStore,
    catalog: &Catalog,
    target: &str,
    upload: &Upload,
    opts: &PublishOptions,
    sent_now: &std::sync::Mutex<std::collections::HashMap<String, String>>,
) -> Result<Outcome, SendError> {
    let Some(fetch) = &upload.fetch else {
        return send(store, catalog, target, upload, opts).await;
    };
    // Tried once now; if it fails for a reason that may pass, once more at
    // the end of the pass.
    let fetched = fetch.fetch().await?;
    if let Some(label) = upload.metadata.get("blake3") {
        if *label != fetched.blake3 {
            return Err(StorageError::NotTheSame(
                "what was read from the cloud storage is not the file Keevoro labelled it as".into(),
            )
            .into());
        }
    }
    let whole = fetched.blake3.clone();
    let key = upload.key.to_string();
    let placed_elsewhere = catalog.collected_path(&whole)?.is_some_and(|place| place != key);
    let sent_elsewhere = sent_now
        .lock()
        .map(|sent| sent.get(&whole).is_some_and(|other| *other != key))
        .unwrap_or(false);
    if placed_elsewhere || sent_elsewhere {
        return Ok(Outcome::Copy(whole));
    }
    let mut metadata = upload.metadata.clone();
    metadata.insert("blake3".to_string(), whole.clone());
    let here = Upload {
        file: fetched.file.clone(),
        bytes: fetched.bytes,
        metadata,
        learn_fingerprint: false,
        fetch: None,
        ..upload.clone()
    };
    let outcome = send(store, catalog, target, &here, opts).await?;
    drop(fetched);
    if let Ok(mut sent) = sent_now.lock() {
        sent.insert(whole.clone(), key);
    }
    Ok(match outcome {
        Outcome::Sent(bytes, _) => Outcome::Sent(bytes, upload.learn_fingerprint.then_some(whole)),
        other => other,
    })
}

async fn send(
    store: &dyn MultipartStore,
    catalog: &Catalog,
    target: &str,
    upload: &Upload,
    opts: &PublishOptions,
) -> Result<Outcome, SendError> {
    let caps = store.capabilities();
    // A folder is sent whole files: in parts, each would be written to the
    // disk and then written again when the parts are joined.
    let in_parts = caps.multipart.is_some() && !caps.whole_files_preferred && upload.bytes > opts.multipart_above;
    // Asking first costs one small request. It is needed when the provider
    // cannot refuse an overwrite itself, and before a large upload it saves
    // sending gigabytes only to be told the file is already there.
    if upload.write_once && (in_parts || !caps.conditional_create) {
        if let Some(found) = with_retries(opts, || store.head(&upload.key)).await? {
            return same_or_refuse(store, upload, found.bytes).await.map_err(Into::into);
        }
    }
    let put = PutOptions {
        create_only: upload.write_once && caps.conditional_create,
        // The fingerprint goes along whatever the store keeps of it, so a
        // store that can prove its copy exact does.
        expect_blake3: upload.metadata.get("blake3").cloned(),
        learn_blake3: upload.learn_fingerprint,
        content_type: upload.content_type.clone(),
        metadata: if caps.custom_metadata {
            upload.metadata.clone()
        } else {
            BTreeMap::new()
        },
    };
    let sent = if in_parts {
        send_in_parts(store, catalog, target, upload, &put, opts)
            .await
            .map(|fingerprint| (upload.bytes, fingerprint))
    } else {
        // A store that cannot fingerprint as it writes (cloud storage, which
        // streams the file itself) has it done here first. For the sizes
        // that go up whole this is a quick local read.
        let before = if upload.learn_fingerprint && !caps.whole_files_preferred {
            Some(fingerprint_of(&upload.file).await?)
        } else {
            None
        };
        // Never waited on and tried again on the spot: a file that fails for
        // a reason that may pass is tried once more at the end of the pass.
        store
            .put(&upload.key, ByteSource::File(upload.file.clone()), &put)
            .await
            .map(|receipt| {
                // A store that counts as it writes has already reported these.
                if !caps.counts_its_own_bytes {
                    keevoro_core::meter::sent(upload.bytes, opts.over_network);
                }
                (receipt.bytes, receipt.blake3.or(before))
            })
            .map_err(SendError::from)
    };
    match sent {
        Ok((bytes, fingerprint)) => Ok(Outcome::Sent(bytes, fingerprint.filter(|_| upload.learn_fingerprint))),
        Err(SendError::Storage(StorageError::AlreadyExists(_))) => {
            let found = with_retries(opts, || store.head(&upload.key)).await?;
            same_or_refuse(store, upload, found.map_or(u64::MAX, |f| f.bytes))
                .await
                .map_err(Into::into)
        }
        Err(e) => Err(e),
    }
}

/// An object is already stored under this key. It counts as the file being
/// sent only if it is the same size and, where the store can read its own
/// contents, has the same fingerprint, worked out from the file now for one
/// not yet fingerprinted. Otherwise the upload is refused rather than
/// replace what is there or pretend it is the original. A store that
/// cannot tell without downloading the object (cloud storage) is believed
/// on size, since keys are unique per file in a backup.
async fn same_or_refuse(
    store: &dyn MultipartStore,
    upload: &Upload,
    stored_bytes: u64,
) -> Result<Outcome, StorageError> {
    let different = || Err(StorageError::AlreadyExists(upload.key.to_string()));
    if stored_bytes != upload.bytes {
        return different();
    }
    // A file a search only sampled has no fingerprint yet, so one is
    // worked out from the file itself: a size that matches is not proof.
    let fingerprint = match upload.metadata.get("blake3") {
        Some(known) => Some(known.clone()),
        None if upload.learn_fingerprint => Some(fingerprint_of(&upload.file).await?),
        None => None,
    };
    if let Some(fingerprint) = fingerprint {
        if store.holds_exactly(&upload.key, &fingerprint).await? == Some(false) {
            return different();
        }
    }
    Ok(Outcome::AlreadyThere)
}

async fn send_in_parts(
    store: &dyn MultipartStore,
    catalog: &Catalog,
    target: &str,
    upload: &Upload,
    put: &PutOptions,
    opts: &PublishOptions,
) -> Result<Option<String>, SendError> {
    let key = upload.key.as_str();
    // Carry on an earlier attempt if the provider still knows about it.
    let mut held = Vec::new();
    let mut id = None;
    if let Some(saved) = catalog.multipart(target, key)? {
        let saved = UploadId(saved);
        match with_retries(opts, || store.list_parts(&saved)).await {
            Ok(parts) => {
                held = parts;
                id = Some(saved);
            }
            Err(StorageError::UploadNotFound(_)) => {}
            Err(e) => return Err(e.into()),
        }
    }
    let id = match id {
        Some(id) => id,
        None => {
            let fresh = with_retries(opts, || store.start(&upload.key, put)).await?;
            // Saved before any part is sent, so a crash cannot orphan it.
            catalog.set_multipart(target, key, Some(&fresh.0))?;
            fresh
        }
    };

    let part_bytes = part_size(store, upload.bytes, opts.part_bytes);
    // Fingerprinted part by part as it is read, when every part is read
    // here. If some parts were sent on an earlier run they are not read
    // again, so the file is fingerprinted on its own afterwards.
    let mut whole = (upload.learn_fingerprint && held.is_empty()).then(blake3::Hasher::new);
    let mut receipts = Vec::new();
    let mut offset = 0u64;
    let mut number: u16 = 1;
    while offset < upload.bytes {
        let len = part_bytes.min(upload.bytes - offset);
        let receipt = match held.iter().find(|p| p.number == number && p.bytes == len) {
            Some(already) => already.clone(),
            None => {
                let body = read_range(&upload.file, offset, len)?;
                if let Some(hasher) = whole.as_mut() {
                    hasher.update(&body);
                }
                let landed =
                    with_retries(opts, || store.put_part(&id, number, ByteSource::Bytes(body.clone()))).await?;
                // Counted part by part, so a large file shows steady progress.
                keevoro_core::meter::sent(len, opts.over_network);
                landed
            }
        };
        receipts.push(receipt);
        offset += len;
        number = number
            .checked_add(1)
            .ok_or_else(|| StorageError::InvalidParts("too many parts".into()))?;
    }
    finish(store, &id, &receipts, opts).await?;
    Ok(match whole {
        Some(hasher) => Some(hasher.finalize().to_hex().to_string()),
        None if upload.learn_fingerprint => Some(fingerprint_of(&upload.file).await?),
        None => None,
    })
}

/// The whole-file BLAKE3 of `file`, read in pieces off the async thread.
async fn fingerprint_of(file: &std::path::Path) -> Result<String, StorageError> {
    let file = file.to_path_buf();
    tokio::task::spawn_blocking(move || {
        use std::io::Read;
        let source = |e| StorageError::at(Step::ReadingTheSource, e);
        let mut input = std::fs::File::open(&file).map_err(source)?;
        let mut hasher = blake3::Hasher::new();
        let mut buffer = vec![0u8; 1024 * 1024];
        loop {
            match input.read(&mut buffer) {
                Ok(0) => break,
                Ok(n) => {
                    hasher.update(&buffer[..n]);
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(e) => return Err(source(e)),
            }
        }
        Ok(hasher.finalize().to_hex().to_string())
    })
    .await
    .map_err(|e| StorageError::Provider(e.to_string()))?
}

async fn finish(
    store: &dyn MultipartStore,
    id: &UploadId,
    receipts: &[PartReceipt],
    opts: &PublishOptions,
) -> Result<(), StorageError> {
    match with_retries(opts, || store.complete(id, receipts)).await {
        Ok(_) => Ok(()),
        Err(e) => {
            // Tidy up when the provider has refused for good, so abandoned
            // parts do not sit there costing storage.
            if !e.is_retryable() {
                let _ = store.abort(id).await;
            }
            Err(e)
        }
    }
}

/// Picks a part size inside the provider's limits that also keeps the
/// number of parts under its maximum.
fn part_size(store: &dyn MultipartStore, total: u64, preferred: u64) -> u64 {
    let Some(limits) = store.capabilities().multipart else {
        return total.max(1);
    };
    let needed_for_count = total.div_ceil(u64::from(limits.max_parts.max(1)));
    preferred
        .max(needed_for_count)
        .clamp(limits.min_part_bytes.max(1), limits.max_part_bytes.max(1))
}

fn read_range(file: &Path, offset: u64, len: u64) -> Result<Vec<u8>, StorageError> {
    let source = |e| StorageError::at(Step::ReadingTheSource, e);
    let mut f = std::fs::File::open(file).map_err(source)?;
    f.seek(SeekFrom::Start(offset)).map_err(source)?;
    let mut buf = vec![0u8; usize::try_from(len).map_err(|_| StorageError::InvalidParts("part too large".into()))?];
    f.read_exact(&mut buf).map_err(source)?;
    Ok(buf)
}

/// Runs `action`, trying again after a pause when the error is the kind
/// that may pass (a dropped connection).
async fn with_retries<T, F, Fut>(opts: &PublishOptions, mut action: F) -> Result<T, StorageError>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, StorageError>>,
{
    let mut pause = opts.retry_pause;
    let mut attempts_left = opts.retries;
    loop {
        match action().await {
            Err(e) if e.is_retryable() && attempts_left > 0 => {
                attempts_left -= 1;
                tokio::time::sleep(pause).await;
                pause = pause.saturating_mul(2);
            }
            other => return other,
        }
    }
}

#[cfg(test)]
mod tests;
