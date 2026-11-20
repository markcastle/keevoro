//! Everything Keevoro can do, said once.
//!
//! Three front ends sit on this crate: the desktop app, the `keevoro`
//! command line and the tools an AI agent is given (`keevoro mcp`). None of them
//! should hold any logic of its own. Each translates between its own world
//! (a button, an argument, a tool call) and the operations here.
//!
//! There are four parts:
//!
//! - the [`register`], which lists every operation with its name, what it
//!   does, whether it changes anything and where it is offered;
//! - [`Keevoro`], Keevoro as a whole: its named [`Backups`], its settings,
//!   its log and the keychain, with a method for each operation that is
//!   not about one backup;
//! - [`Backup`], one backup, whose methods are the operations on it;
//! - [`call`], the same operations reached by name with JSON in and JSON
//!   out, which is how the command line uses them; an agent's tools use
//!   [`call_for_agent`], which holds each call to the person's rules first.
//!
//! The long operations are [`jobs`]: starting one answers at once, and
//! the job is then heard, asked after, paused and stopped.
//!
//! Every operation in the register is done here. One added to the register
//! before it is built says so (`implemented: false`), and [`call`] answers
//! it with a plain "not yet". The app's parity test keeps that honest.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod activity;
pub mod agents;
pub mod backup;
pub mod backups;
pub mod destinations;
pub mod error;
pub mod file_kinds;
pub mod for_agents;
pub mod inside;
pub mod jobs;
pub mod keevoro;
pub mod leave_out;
pub mod log;
pub mod picture;
pub mod places;
mod rates;
pub mod register;
pub mod reset;
pub mod wants;
pub mod words;

pub use agents::{AgentAccess, AgentsAccessRequest};
pub use backup::{Backup, FileTypeView, Found};
pub use backups::{Backups, Named};
pub use destinations::{
    AddDestinationRequest, DestinationAdded, DestinationKind, Key, Rehearsal, RemoveDestinationRequest, Room,
    RoomAtAGlance, WaitingAt,
};
pub use error::{Error, Kind, Result};
pub use file_kinds::PhotoOrVideo;
pub use for_agents::{call_for_agent, read_for_agent};
pub use jobs::{Event, FrontEnd, JobHandle, JobId, JobKind, JobStatus, Listener, Outcome, Said, Which, PROGRESS_EVERY};
pub use keevoro::{BackupListing, Codecs, Folders, Keevoro, Offer, SettingsView};
pub use keevoro_core::estimate;
pub use keevoro_core::{FileKind, KindChange, MediaKind};
pub use keevoro_engine::connect::SecretStore;
pub use keevoro_engine::history;
pub use picture::{Picture, PictureRequest, PictureSize};
pub use register::{entries, Agents, Changes, Entry, Op, Tool};

use file_kinds::{AddFileKindRequest, ChangeFileKindRequest, FileKindRequest};
use keevoro::{ForgetBackupRequest, LogRequest, RenameBackupRequest, SettingsRequest, StartBackupRequest};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};

/// What Keevoro is and what it promises, for `keevoro.about`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct About {
    pub version: &'static str,
    /// The rules Keevoro is built around, one short sentence each.
    pub promises: &'static [&'static str],
    pub licence: &'static str,
}

/// The promises, as the README makes them, shortened to a line each.
const PROMISES: &[&str] = &[
    "Your sources are only ever read. Nothing on a source is edited, moved, renamed or deleted.",
    "A copy counts only when it is proved exact.",
    "No original in a backup is ever replaced or removed.",
    "Duplicates are exact, never guesses: two files are the same only when every byte matches.",
    "Nothing is hidden without saying so.",
    "You do not need Keevoro to get your photos back: a backup is ordinary files in ordinary folders.",
    "Stopping is safe. Keevoro carries on from where it was.",
];

const LICENCE: &str = "Functional Source License (FSL-1.1-MIT). Free to use, read, change and share, \
     but not to sell or offer as a competing product. Each release becomes plain MIT two years after it comes out.";

/// `keevoro.about`
pub fn about() -> About {
    About {
        version: env!("CARGO_PKG_VERSION"),
        promises: PROMISES,
        licence: LICENCE,
    }
}

/// `sources.forget`, `sources.file_types`
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SourceRequest {
    /// The source's id, as `sources.list` gives it.
    pub source: i64,
}

/// `sources.include_type`
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct IncludeTypeRequest {
    /// The source's id, as `sources.list` gives it.
    pub source: i64,
    /// The ending, such as `png`, with or without its dot.
    pub ending: String,
    /// True to include files with this ending, false to leave them out.
    pub included: bool,
}

/// `backup.history`
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HistoryRequest {
    /// One backup only, counting from 1 for the newest. Every one kept
    /// when left out.
    #[serde(default)]
    pub run: Option<usize>,
}

/// `collections.photos`
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CollectionRequest {
    /// The collection's key, as `collections.list` gives it.
    pub collection: String,
}

/// `collections.keep`
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct KeepCollectionRequest {
    /// The collection's key, as `collections.list` gives it.
    pub collection: String,
    /// True to keep it, so it is backed up; false to leave it out.
    pub keep: bool,
}

/// `photos.keep`
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct KeepPhotoRequest {
    /// The photo's short id, as `collections.photos` gives it.
    pub photo: String,
    /// True to keep it, so it is backed up; false to leave it out.
    pub keep: bool,
}

/// `backup.previews`: without `wanted` it only reads.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PreviewsRequest {
    /// True to make previews as well as sending the originals, false to
    /// send the originals alone. Left out, nothing changes.
    #[serde(default)]
    pub wanted: Option<bool>,
}

/// `places.survey`, `sources.look`
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlaceRequest {
    /// The drive or folder, written whole as the computer writes it, such
    /// as `D:\DCIM` or `/Volumes/Card/DCIM`.
    pub place: String,
}

/// `sources.look_again`: one source by its id, or every one.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LookAgainRequest {
    /// One source, by its id as `sources.list` gives it. Left out, every
    /// source is looked through again.
    #[serde(default)]
    pub source: Option<i64>,
}

/// `jobs.follow`, `jobs.stop`: one job by its id, or every one running on
/// the backup named, or on any backup when none is named.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JobRequest {
    /// One job, by the id it was started with. Left out, every job.
    #[serde(default)]
    pub job: Option<JobId>,
}

/// The longest `jobs.status` waits for jobs to end, in seconds. A caller
/// that must not be kept waiting long, such as an agent's tool call, asks
/// again rather than waiting for hours.
pub const LONGEST_WAIT: u64 = 30;

/// `jobs.status`: as [`JobRequest`], and it may wait a while for the jobs
/// to end before it answers.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StatusRequest {
    /// One job, by the id it was started with. Left out, every job.
    #[serde(default)]
    pub job: Option<JobId>,
    /// Wait up to this many seconds (at most 30) for the job, or every
    /// job, to end before answering. Left out, it answers at once.
    #[serde(default)]
    pub wait_seconds: Option<u64>,
}

/// `jobs.pause`, `jobs.carry_on`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Paused {
    pub paused: bool,
}

/// For operations that take nothing: anything given is a mistake worth
/// saying, above all for one that changes things.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Nothing {}

/// How often `jobs.status` looks again while it waits for jobs to end.
const LOOK_AGAIN_EVERY: std::time::Duration = std::time::Duration::from_millis(200);

/// How many lines of the log `keevoro.log` gives when not told.
const LOG_LINES: usize = 400;

fn read<T: DeserializeOwned>(request: Value) -> Result<T> {
    // No request at all is the same as an empty one.
    let request = if request.is_null() {
        Value::Object(serde_json::Map::new())
    } else {
        request
    };
    serde_json::from_value(request).map_err(Error::not_understood)
}

/// Reads a request that may carry storage keys. If it cannot be read, the
/// keys are taken out of the reason before it is said.
fn read_with_keys<T: DeserializeOwned>(request: Value) -> Result<T> {
    let keys: Vec<String> = ["accessKeyId", "secretAccessKey"]
        .iter()
        .filter_map(|field| request.get(field))
        .map(|key| match key {
            Value::String(text) => text.trim().to_string(),
            other => other.to_string(),
        })
        .collect();
    read(request).map_err(|e| {
        let keys: Vec<&str> = keys.iter().map(String::as_str).collect();
        destinations::without_keys(e, &keys)
    })
}

fn answer(value: impl Serialize) -> Result<Value> {
    Ok(serde_json::to_value(value)?)
}

/// Does one operation, by name, with its request and answer as JSON. This
/// is the way in for the command line and an agent's tools.
///
/// `backup` names the backup an operation works on, by its name or id.
/// Without one, the backup the app has open is used; an operation that
/// needs a backup fails plainly when there is none.
pub fn call(keevoro: &Keevoro, op: Op, backup: Option<&str>, request: Value) -> Result<Value> {
    let open = || keevoro.backups().open(backup);
    let named = || keevoro.backups().named_or_open(backup);
    // A backup used only for its choices (photos or videos only) and what
    // its searches know: the one named, else the open one, if any.
    let any_backup = || -> Result<Option<Named>> {
        match backup {
            Some(_) => Ok(Some(keevoro.backups().named_or_open(backup)?)),
            None => Ok(keevoro.backups().current()),
        }
    };
    match op {
        Op::BackupsList => {
            read::<Nothing>(request)?;
            answer(keevoro.list_backups())
        }
        Op::BackupsStart => {
            let r: StartBackupRequest = read(request)?;
            let started = keevoro.start_backup(&r.name)?;
            if let Some(choice) = r.wants {
                keevoro.set_backup_wants(&Backup::open(&started.home)?, choice.into())?;
            }
            answer(started)
        }
        Op::BackupsRename => {
            let r: RenameBackupRequest = read(request)?;
            answer(keevoro.rename_backup(&r.backup, &r.to)?)
        }
        Op::BackupsForget => {
            let r: ForgetBackupRequest = read(request)?;
            answer(keevoro.forget_backup(&r.backup)?)
        }
        Op::PlacesList => {
            read::<Nothing>(request)?;
            answer(places::plugged_in())
        }
        Op::SourcesList => {
            read::<Nothing>(request)?;
            answer(open()?.sources()?)
        }
        Op::SourcesForget => {
            let r: SourceRequest = read(request)?;
            answer(open()?.forget_source(r.source)?)
        }
        Op::SourcesFileTypes => {
            let r: SourceRequest = read(request)?;
            answer(open()?.file_types(r.source)?)
        }
        Op::SourcesIncludeType => {
            let r: IncludeTypeRequest = read(request)?;
            answer(open()?.include_type(r.source, &r.ending, r.included)?)
        }
        Op::SourcesLeaveOut => {
            let r: leave_out::LeaveOutRequest = read(request)?;
            answer(open()?.leave_out(Path::new(&r.folder), r.left_out)?)
        }
        Op::FoundSummary => {
            read::<Nothing>(request)?;
            answer(open()?.found()?)
        }
        Op::CollectionsList => {
            read::<Nothing>(request)?;
            answer(open()?.collections()?)
        }
        Op::CollectionsPhotos => {
            let r: CollectionRequest = read(request)?;
            answer(open()?.collection_photos(&r.collection)?)
        }
        Op::CollectionsKeep => {
            let r: KeepCollectionRequest = read(request)?;
            answer(open()?.keep_collection(&r.collection, r.keep)?)
        }
        Op::PhotosKeep => {
            let r: KeepPhotoRequest = read(request)?;
            answer(open()?.keep_photo(&r.photo, r.keep)?)
        }
        Op::SetAsideList => {
            read::<Nothing>(request)?;
            answer(open()?.set_aside()?)
        }
        Op::DuplicatesList => {
            read::<Nothing>(request)?;
            answer(open()?.duplicates()?)
        }
        Op::PhotosPicture => {
            let r: PictureRequest = read(request)?;
            let bytes = match (r.photo, r.file) {
                (Some(photo), None) => keevoro.picture(&open()?, &photo, r.size)?,
                (None, Some(file)) => keevoro.picture_of_file(Path::new(&file), r.size, any_backup()?.as_ref())?,
                _ => return Err(Error::not_understood("give a photo or a file, not both")),
            };
            answer(Picture::webp(&bytes))
        }
        Op::PlacesFolders => {
            let r: PlaceRequest = read(request)?;
            answer(keevoro.folders_inside(Path::new(&r.place), any_backup()?.as_ref())?)
        }
        Op::PlacesFiles => {
            let r: inside::FilesRequest = read(request)?;
            answer(keevoro.files_in(&r, any_backup()?.as_ref())?)
        }
        Op::DestinationsList => {
            read::<Nothing>(request)?;
            answer(open()?.destinations()?)
        }
        Op::DestinationsAdd => {
            let r: AddDestinationRequest = read_with_keys(request)?;
            answer(keevoro.add_destination(&open()?, &r)?)
        }
        Op::DestinationsRemove => {
            let r: RemoveDestinationRequest = read(request)?;
            let at = match (r.at, r.all) {
                (Some(at), false) => Some(at),
                (None, true) => None,
                _ => {
                    return Err(Error::not_understood(
                        "say which destination to remove with \"at\", or every one with \"all\": true",
                    ))
                }
            };
            answer(keevoro.remove_destination(&open()?, at)?)
        }
        Op::DestinationsTidy => {
            let r: keevoro::TidyRequest = read(request)?;
            answer(keevoro.tidy_folder(Path::new(&r.place), r.dry_run)?)
        }
        Op::DestinationsRoom => {
            read::<Nothing>(request)?;
            answer(open()?.room()?)
        }
        Op::BackupRehearse => {
            read::<Nothing>(request)?;
            answer(open()?.rehearse()?)
        }
        Op::BackupHistory => {
            let r: HistoryRequest = read(request)?;
            let all = open()?.history()?;
            match r.run {
                None => answer(all),
                Some(n) => match n.checked_sub(1).and_then(|at| all.get(at)) {
                    Some(one) => answer([one]),
                    None => Err(Error::not_found(format!(
                        "There is no backup number {n} in this backup's history. It keeps the last {}; {} so far.",
                        keevoro_engine::history::KEPT,
                        all.len()
                    ))),
                },
            }
        }
        Op::BackupLastSummary => {
            read::<Nothing>(request)?;
            answer(open()?.last_summary()?)
        }
        Op::BackupPhotosOrVideos => {
            let r: wants::BackupWantsRequest = read(request)?;
            let backup = open()?;
            match r.wants {
                Some(choice) => answer(keevoro.set_backup_wants(&backup, choice.into())?),
                None => answer(backup.wants()?),
            }
        }
        Op::SourcesPhotosOrVideos => {
            let r: wants::SourceWantsRequest = read(request)?;
            let backup = open()?;
            match r.wants {
                Some(choice) => answer(keevoro.set_source_wants(&backup, r.source, choice.into())?),
                None => answer(backup.source_wants(r.source)?),
            }
        }
        Op::BackupPreviews => {
            let r: PreviewsRequest = read(request)?;
            let backup = open()?;
            if let Some(wanted) = r.wanted {
                backup.set_previews(wanted)?;
            }
            answer(backup.previews()?)
        }
        Op::KeevoroAbout => {
            read::<Nothing>(request)?;
            answer(about())
        }
        Op::KeevoroActivity => {
            read::<Nothing>(request)?;
            answer(keevoro.activity())
        }
        Op::KeevoroLog => {
            let r: LogRequest = read(request)?;
            answer(keevoro.log_tail(r.lines.unwrap_or(LOG_LINES)))
        }
        Op::KeevoroSettings => {
            let r: SettingsRequest = read(request)?;
            match r.own_codecs {
                Some(on) => answer(keevoro.set_own_codecs(on)?),
                None => answer(keevoro.settings()),
            }
        }
        // Only the check. Installing replaces the program that is running,
        // which is the app's to do.
        Op::KeevoroUpdate => {
            read::<Nothing>(request)?;
            answer(keevoro.update_offer())
        }
        Op::KeevoroReset => {
            read::<Nothing>(request)?;
            answer(keevoro.reset()?)
        }
        Op::FileKindsList => {
            read::<Nothing>(request)?;
            answer(keevoro.file_kinds())
        }
        Op::FileKindsAdd => {
            let r: AddFileKindRequest = read(request)?;
            answer(keevoro.add_file_kind(&r.ending, r.kind.into())?)
        }
        Op::FileKindsChange => {
            let r: ChangeFileKindRequest = read(request)?;
            let change = keevoro_core::KindChange {
                kind: r.kind.map(Into::into),
                on: r.on,
                ending: r.to,
            };
            answer(keevoro.change_file_kind(&r.ending, &change)?)
        }
        Op::FileKindsRemove => {
            let r: FileKindRequest = read(request)?;
            answer(keevoro.remove_file_kind(&r.ending)?)
        }
        Op::FileKindsReset => {
            read::<Nothing>(request)?;
            answer(keevoro.reset_file_kinds()?)
        }
        Op::AgentsAccess => {
            let r: AgentsAccessRequest = read(request)?;
            if r.look_in.is_none() && r.back_up_to.is_none() && r.may_change.is_none() {
                answer(keevoro.agent_access())
            } else {
                answer(keevoro.set_agent_access(r)?)
            }
        }

        Op::PlacesSurvey => {
            let r: PlaceRequest = read(request)?;
            // A backup is only used to say which folders are already its
            // sources, so the open one is used if there is one.
            let named = match backup {
                Some(_) => Some(keevoro.backups().named_or_open(backup)?),
                None => keevoro.backups().current(),
            };
            answer(keevoro.survey(Path::new(&r.place), named.as_ref(), None)?)
        }
        Op::SourcesLook => {
            let r: PlaceRequest = read(request)?;
            answer(keevoro.look(&named()?, Path::new(&r.place), None)?)
        }
        Op::SourcesLookAgain => {
            let r: LookAgainRequest = read(request)?;
            answer(keevoro.look_again(&named()?, r.source, None)?)
        }
        Op::BackupRun => {
            read::<Nothing>(request)?;
            answer(keevoro.run_backup(&named()?, None)?)
        }
        Op::BackupCheck => {
            read::<Nothing>(request)?;
            answer(keevoro.check_backup(&named()?, None)?)
        }
        Op::JobsStatus => {
            let r: StatusRequest = read(request)?;
            let seconds = r.wait_seconds.unwrap_or(0).min(LONGEST_WAIT);
            let until = std::time::Instant::now() + std::time::Duration::from_secs(seconds);
            let mut running = statuses(keevoro, backup, r.job)?;
            while !running.is_empty() && std::time::Instant::now() < until {
                std::thread::sleep(LOOK_AGAIN_EVERY);
                running = statuses(keevoro, backup, r.job)?;
            }
            // One job asked for by its id, and ended: how it ended.
            if let (Some(id), true) = (r.job, running.is_empty()) {
                running.extend(keevoro.ended_job(id));
            }
            answer(running)
        }
        // The command line prints each event as it is said, through
        // `Keevoro::follow`. Asked by name, it waits for the jobs to end
        // and answers everything they said meanwhile.
        Op::JobsFollow => {
            let r: JobRequest = read(request)?;
            let heard: Arc<Mutex<Vec<Event>>> = Arc::default();
            let ids: Vec<JobId> = statuses(keevoro, backup, r.job)?
                .into_iter()
                .map(|s| s.handle.id)
                .collect();
            for id in &ids {
                let heard = heard.clone();
                keevoro.follow(
                    *id,
                    Arc::new(move |event: &Event| {
                        heard.lock().unwrap_or_else(PoisonError::into_inner).push(event.clone());
                    }),
                );
            }
            for id in &ids {
                keevoro.wait(*id);
            }
            let heard = heard.lock().unwrap_or_else(PoisonError::into_inner).clone();
            answer(heard)
        }
        Op::JobsPause => {
            read::<Nothing>(request)?;
            keevoro.pause();
            answer(Paused { paused: true })
        }
        Op::JobsCarryOn => {
            read::<Nothing>(request)?;
            keevoro.carry_on();
            answer(Paused { paused: false })
        }
        Op::JobsStop => {
            let r: JobRequest = read(request)?;
            let on = backup.map(|_| named()).transpose()?;
            let which = match (r.job, &on) {
                (Some(id), _) => Which::Job(id),
                (None, Some(on)) => Which::OnBackup(&on.id),
                (None, None) => Which::All,
            };
            let asked = keevoro.stop(which);
            match r.job {
                Some(id) if asked.is_empty() => Err(Error::no_such_job(id)),
                _ => answer(asked),
            }
        }
    }
}

/// The jobs running on the backup named, or on any backup; only the one
/// asked for, when one is.
fn statuses(keevoro: &Keevoro, backup: Option<&str>, job: Option<JobId>) -> Result<Vec<JobStatus>> {
    let on = backup.map(|_| keevoro.backups().named_or_open(backup)).transpose()?;
    Ok(keevoro
        .jobs(on.as_ref().map(|named| named.id.as_str()))
        .into_iter()
        .filter(|s| job.is_none_or(|id| s.handle.id == id))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backup::tests::card_looked_through_into;
    use crate::keevoro::tests::keevoro;
    use serde_json::json;
    use std::path::{Path, PathBuf};

    /// Keevoro in a temporary folder with one backup, "Card photos", that
    /// has looked through a memory card.
    fn one_keevoro() -> (tempfile::TempDir, PathBuf, Keevoro) {
        let dir = tempfile::tempdir().unwrap();
        let keevoro = keevoro(dir.path());
        let home = keevoro.start_backup("Card photos").unwrap().home;
        let card = card_looked_through_into(dir.path(), &home);
        (dir, card, keevoro)
    }

    fn op(name: &str) -> Op {
        Op::named(name).unwrap()
    }

    #[test]
    fn about_gives_the_version_and_the_promises() {
        let about = about();
        assert_eq!(about.version, env!("CARGO_PKG_VERSION"));
        assert!(!about.promises.is_empty());
        for line in about.promises.iter().chain([&about.licence]) {
            assert!(!line.contains('\u{2014}'), "{line}");
            assert!(line.ends_with('.'), "{line}");
        }
        assert!(about.licence.contains("FSL-1.1-MIT"));
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            call(&keevoro(dir.path()), Op::KeevoroAbout, None, Value::Null).unwrap()["version"],
            about.version
        );
    }

    #[test]
    fn the_register_says_truly_which_operations_are_done_here() {
        // Some start jobs, and pausing is one switch for the whole program.
        let _turn = crate::jobs::tests::one_at_a_time();
        let _after = crate::jobs::tests::CarryOnAfter;
        let (_dir, _card, keevoro) = one_keevoro();
        // Resetting would take the backup away from the rest, so it is last.
        let mut all: Vec<Entry> = entries().collect();
        all.sort_by_key(|e| e.op == Op::KeevoroReset);
        for entry in all {
            if entry.op == Op::KeevoroReset {
                // Not while the jobs started above are running.
                keevoro.wait_for_jobs();
            }
            let result = call(&keevoro, entry.op, None, json!({}));
            let not_yet = matches!(&result, Err(e) if e.kind == Kind::NotYet);
            assert_eq!(
                not_yet, !entry.implemented,
                "{} is marked implemented: {}, but calling it gave {result:?}",
                entry.name, entry.implemented
            );
        }
    }

    #[test]
    fn unfinished_copies_are_said_first_then_cleared_and_what_stays_is_said_with_why() {
        let dir = tempfile::tempdir().unwrap();
        let keevoro = keevoro(dir.path());
        let folder = dir.path().join("Old backup");
        let work = folder.join(".keevoro-uploads");
        std::fs::create_dir_all(&work).unwrap();
        std::fs::create_dir_all(folder.join("originals")).unwrap();
        std::fs::write(folder.join("originals/IMG_1.jpg"), b"a photo").unwrap();
        let old = work.join("tmp-abandoned");
        std::fs::write(&old, [0u8; 4096]).unwrap();
        std::fs::File::options()
            .write(true)
            .open(&old)
            .unwrap()
            .set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(3 * 3600))
            .unwrap();
        std::fs::write(work.join("tmp-live"), b"being written").unwrap();
        let place = folder.display().to_string();

        let first = call(
            &keevoro,
            Op::DestinationsTidy,
            None,
            json!({ "place": place, "dryRun": true }),
        )
        .unwrap();
        assert_eq!(first["cleared"], false);
        assert_eq!(
            (first["files"].as_u64(), first["bytes"].as_u64()),
            (Some(1), Some(4096))
        );
        assert_eq!(first["inUse"], 1);
        assert!(old.is_file(), "asking first clears nothing");
        assert!(first["said"][0]
            .as_str()
            .unwrap()
            .starts_with("There are 4.0 KB of unfinished copies (1 file) in"));
        assert!(!Op::DestinationsTidy.changes_with(&json!({ "place": place, "dryRun": true })));
        assert!(Op::DestinationsTidy.changes_with(&json!({ "place": place })));

        let done = call(&keevoro, Op::DestinationsTidy, None, json!({ "place": place })).unwrap();
        assert_eq!(done["cleared"], true);
        assert_eq!(done["files"], 1);
        assert!(!old.exists());
        assert!(work.join("tmp-live").is_file(), "a copy being made now is left");
        assert_eq!(std::fs::read(folder.join("originals/IMG_1.jpg")).unwrap(), b"a photo");
        let said: Vec<&str> = done["said"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(Value::as_str)
            .collect();
        assert!(
            said[0].starts_with("Cleared 4.0 KB of unfinished copies (1 file) from"),
            "{said:?}"
        );
        assert!(
            said[1].starts_with("Left 1 thing written to in the last hour"),
            "{said:?}"
        );
    }

    #[test]
    fn every_operation_in_the_register_is_done_here() {
        let not_yet: Vec<&str> = entries().filter(|e| !e.implemented).map(|e| e.name).collect();
        assert_eq!(not_yet, Vec::<&str>::new());
    }

    #[test]
    fn operations_on_a_backup_are_reached_by_name_with_json() {
        let (_dir, _card, keevoro) = one_keevoro();
        let k = &keevoro;

        let sources = call(k, op("sources.list"), None, Value::Null).unwrap();
        let id = sources[0]["id"].clone();
        let types = call(k, op("sources.file_types"), None, json!({ "source": id })).unwrap();
        assert_eq!(types[0]["ending"], "jpg");
        call(
            k,
            op("sources.include_type"),
            None,
            json!({ "source": id, "ending": "png", "included": false }),
        )
        .unwrap();

        let found = call(k, op("found.summary"), None, Value::Null).unwrap();
        assert_eq!(found["photos"], 3);

        let changed = call(
            k,
            op("collections.keep"),
            None,
            json!({ "collection": "Holidays / Cornwall", "keep": false }),
        )
        .unwrap();
        assert_eq!(changed, 2);
        let photos = call(
            k,
            op("collections.photos"),
            None,
            json!({ "collection": "Holidays / Cornwall" }),
        )
        .unwrap();
        assert_eq!(photos[0]["kept"], false);
        let first = photos[0]["id"].clone();
        call(k, op("photos.keep"), None, json!({ "photo": first, "keep": true })).unwrap();

        let picture = call(
            k,
            op("photos.picture"),
            None,
            json!({ "photo": first, "size": "screen" }),
        )
        .unwrap();
        assert_eq!(picture["format"], "webp");
        assert!(
            picture["base64"].as_str().unwrap().starts_with("UklGR"),
            "RIFF, in Base64"
        );

        assert_eq!(call(k, op("set_aside.list"), None, Value::Null).unwrap(), json!([]));
        assert_eq!(
            call(k, op("duplicates.list"), None, Value::Null)
                .unwrap()
                .as_array()
                .map(Vec::len),
            Some(1)
        );
        assert_eq!(call(k, op("destinations.list"), None, Value::Null).unwrap(), json!([]));
        assert_eq!(call(k, op("backup.history"), None, Value::Null).unwrap(), json!([]));
        assert_eq!(
            call(k, op("backup.last_summary"), None, Value::Null).unwrap(),
            Value::Null
        );

        // Reading previews changes nothing; setting them does.
        assert_eq!(call(k, op("backup.previews"), None, json!({})).unwrap(), true);
        assert_eq!(
            call(k, op("backup.previews"), None, json!({ "wanted": false })).unwrap(),
            false
        );
        assert!(!k.backups().open(None).unwrap().previews().unwrap());

        let forgot = call(k, op("sources.forget"), None, json!({ "source": id })).unwrap();
        assert_eq!(forgot, true);
        assert_eq!(call(k, op("sources.list"), None, Value::Null).unwrap(), json!([]));
    }

    #[test]
    fn every_operation_on_a_backup_can_name_it_without_one_being_open() {
        let (dir, _card, keevoro) = one_keevoro();
        let other = keevoro.start_backup("Empty").unwrap();
        keevoro.switch_to(&other.id).unwrap();
        let k = &keevoro;

        assert_eq!(call(k, op("found.summary"), None, Value::Null).unwrap()["photos"], 0);
        assert_eq!(
            call(k, op("found.summary"), Some("card photos"), Value::Null).unwrap()["photos"],
            3
        );
        let usb = dir.path().join("USB");
        let added = call(
            k,
            op("destinations.add"),
            Some("Card photos"),
            json!({ "kind": "folder", "place": usb.display().to_string() }),
        )
        .unwrap();
        assert_eq!(
            added,
            json!({ "name": format!("the folder {}", usb.display()), "at": 0 })
        );
        let rehearsal = call(k, op("backup.rehearse"), Some("Card photos"), json!({})).unwrap();
        assert_eq!(rehearsal["files"], 3);
        let room = call(k, op("destinations.room"), Some("Card photos"), Value::Null).unwrap();
        assert_eq!(room["places"][0]["files"], 3);
        assert_eq!(room["atAGlance"]["enough"], true);
        assert_eq!(
            call(k, op("destinations.list"), None, Value::Null).unwrap(),
            json!([]),
            "the open backup was left alone"
        );
        assert_eq!(
            call(k, op("destinations.remove"), Some("Card photos"), json!({ "at": 0 })).unwrap(),
            json!([format!("the folder {}", usb.display())])
        );

        let e = call(k, op("found.summary"), Some("Holidays"), Value::Null).unwrap_err();
        assert_eq!(
            (e.kind, e.message.as_str()),
            (Kind::NotFound, "There is no backup called \"Holidays\".")
        );
    }

    #[test]
    fn backups_are_listed_started_renamed_and_forgotten_by_name() {
        let (_dir, _card, keevoro) = one_keevoro();
        let k = &keevoro;
        let started = call(k, op("backups.start"), None, json!({ "name": "Drone footage" })).unwrap();
        assert_eq!(started["name"], "Drone footage");
        let list = call(k, op("backups.list"), None, Value::Null).unwrap();
        assert_eq!(list[0]["name"], "Card photos");
        assert_eq!(list[0]["open"], true);
        assert_eq!(list[0]["found"]["photos"], 3);
        assert_eq!(list[1]["found"]["photos"], 0);

        call(
            k,
            op("backups.rename"),
            None,
            json!({ "backup": "drone footage", "to": "Drone" }),
        )
        .unwrap();
        let e = call(
            k,
            op("backups.rename"),
            None,
            json!({ "backup": "Drone", "to": "card photos" }),
        )
        .unwrap_err();
        assert_eq!(e.kind, Kind::Refused);
        let gone = call(k, op("backups.forget"), None, json!({ "backup": "Drone" })).unwrap();
        assert_eq!(gone["name"], "Drone");
        assert_eq!(
            call(k, op("backups.list"), None, Value::Null)
                .unwrap()
                .as_array()
                .map(Vec::len),
            Some(1)
        );
    }

    #[test]
    fn keevoro_itself_is_reached_by_name_with_json() {
        let dir = tempfile::tempdir().unwrap();
        let keevoro = keevoro(dir.path());
        let k = &keevoro;
        assert!(call(k, op("places.list"), None, Value::Null).unwrap().is_array());
        assert!(call(k, op("keevoro.activity"), None, Value::Null).unwrap()["cores"].as_u64() >= Some(1));
        assert_eq!(call(k, op("keevoro.update"), None, Value::Null).unwrap(), Value::Null);

        let settings = call(k, op("keevoro.settings"), None, json!({})).unwrap();
        assert!(settings["ownCodecs"]["on"].is_boolean());
        let off = call(k, op("keevoro.settings"), None, json!({ "ownCodecs": false })).unwrap();
        assert_eq!(off["ownCodecs"]["on"], false);
        call(k, op("keevoro.settings"), None, json!({ "ownCodecs": true })).unwrap();

        let log = call(k, op("keevoro.log"), None, json!({ "lines": 1 })).unwrap();
        assert!(log
            .as_str()
            .unwrap()
            .ends_with("Using this computer's own picture codecs again"));

        let access = call(k, op("agents.access"), None, Value::Null).unwrap();
        assert_eq!(access, json!({ "lookIn": [], "backUpTo": [], "mayChange": false }));
        let photos = dir.path().join("Photos").display().to_string();
        let access = call(
            k,
            op("agents.access"),
            None,
            json!({ "lookIn": [photos], "mayChange": true }),
        )
        .unwrap();
        assert_eq!(access["lookIn"], json!([photos]));
        assert_eq!(access["mayChange"], true);

        k.start_backup("Drone").unwrap();
        let done = call(k, op("keevoro.reset"), None, Value::Null).unwrap();
        assert_eq!(done["removed"], json!(["Drone"]));
        assert_eq!(
            call(k, op("agents.access"), None, Value::Null).unwrap()["mayChange"],
            false,
            "a reset takes back what agents were allowed"
        );
    }

    #[test]
    fn what_cannot_be_done_is_said_plainly() {
        let (_dir, _card, keevoro) = one_keevoro();
        let k = &keevoro;
        let dir = tempfile::tempdir().unwrap();
        let e = call(
            &crate::keevoro::tests::keevoro(dir.path()),
            Op::SourcesList,
            None,
            Value::Null,
        )
        .unwrap_err();
        assert_eq!((e.kind, e.message.as_str()), (Kind::NoBackup, "Choose a backup first."));

        let e = call(k, Op::PhotosKeep, None, json!({ "keep": true })).unwrap_err();
        assert_eq!(e.kind, Kind::NotUnderstood);
        assert!(e.message.contains("photo"), "{e}");
        let e = call(k, Op::SourcesForget, None, json!({ "source": 1, "everything": true })).unwrap_err();
        assert_eq!(
            e.kind,
            Kind::NotUnderstood,
            "an unexpected field is not quietly ignored"
        );
        let e = call(k, Op::KeevoroReset, None, json!({ "really": false })).unwrap_err();
        assert_eq!(
            e.kind,
            Kind::NotUnderstood,
            "nor is one given to something that takes nothing"
        );
        assert_eq!(k.backups().all().len(), 1, "and nothing was reset");
        for unclear in [json!({}), json!({ "at": 0, "all": true })] {
            let e = call(k, Op::DestinationsRemove, None, unclear).unwrap_err();
            assert_eq!(e.kind, Kind::NotUnderstood);
            assert!(e.message.contains("\"all\": true"), "{e}");
        }

        let e = call(k, Op::PhotosKeep, None, json!({ "photo": "0000", "keep": true })).unwrap_err();
        assert_eq!(e.kind, Kind::Failed);
        assert_eq!(e.message, "That photo isn't known to this backup.");

        let e = call(k, Op::BackupRun, None, json!({ "now": true })).unwrap_err();
        assert_eq!(e.kind, Kind::NotUnderstood, "nor one given to a job");
    }

    /// Every file under `dir`, as bytes, for searching.
    fn everything_written(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
        let mut found = Vec::new();
        let mut folders = vec![dir.to_path_buf()];
        while let Some(folder) = folders.pop() {
            for entry in std::fs::read_dir(folder).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    folders.push(path);
                } else if let Ok(bytes) = std::fs::read(&path) {
                    found.push((path, bytes));
                }
            }
        }
        found
    }

    #[test]
    fn a_key_given_is_never_given_back_shown_logged_or_written_down() {
        const ACCESS: &str = "AKIAKEEVOROMARKER0001";
        const SECRET: &str = "keevoro-secret-marker/0002+abc";
        let has_a_key = |text: &str| text.contains(ACCESS) || text.contains(SECRET);
        let (dir, _card, keevoro) = one_keevoro();
        let k = &keevoro;

        // Its Debug form.
        let request = json!({
            "kind": "endpoint",
            // Nothing listens here, so the storage is refused.
            "place": "http://127.0.0.1:9",
            "bucket": "family",
            "accessKeyId": ACCESS,
            "secretAccessKey": SECRET,
        });
        let parsed: AddDestinationRequest = serde_json::from_value(request.clone()).unwrap();
        let debug = format!("{parsed:?} {parsed:#?}");
        assert!(!has_a_key(&debug), "{debug}");
        assert!(debug.contains("[hidden]"), "{debug}");

        // Its error, when the storage is refused, in every form an error
        // takes.
        let e = call(k, op("destinations.add"), None, request).unwrap_err();
        let said = format!("{e} {e:#} {e:?} {}", serde_json::to_string(&e).unwrap());
        assert!(!has_a_key(&said), "{said}");
        assert_eq!(e.kind, Kind::Unreachable, "{e}");

        // Its error, when the request itself is wrong.
        let e = call(
            k,
            op("destinations.add"),
            None,
            json!({ "kind": "r2", "place": "x", "accessKeyId": ACCESS, "secretAccessKey": SECRET, "extra": 1 }),
        )
        .unwrap_err();
        assert_eq!(e.kind, Kind::NotUnderstood);
        assert!(!has_a_key(&format!("{e:?}")), "{e:?}");
        let e = call(
            k,
            op("destinations.add"),
            None,
            json!({ "kind": ACCESS, "place": "x", "accessKeyId": ACCESS, "secretAccessKey": SECRET }),
        )
        .unwrap_err();
        assert!(!has_a_key(&format!("{e:?}")), "even where serde would repeat it: {e:?}");

        // Its answer, when the destination is accepted. A folder needs no
        // keys, and the ones given are not kept with it.
        let added = call(
            k,
            op("destinations.add"),
            None,
            json!({
                "kind": "folder",
                "place": dir.path().join("USB").display().to_string(),
                "accessKeyId": ACCESS,
                "secretAccessKey": SECRET,
            }),
        )
        .unwrap();
        assert!(!has_a_key(&added.to_string()), "{added}");

        // Nor in what any other operation says about the backup.
        for name in [
            "destinations.list",
            "destinations.room",
            "backup.rehearse",
            "backups.list",
            "keevoro.log",
        ] {
            let answer = call(k, op(name), None, Value::Null).unwrap();
            assert!(!has_a_key(&answer.to_string()), "{name}: {answer}");
        }

        // Nor anywhere Keevoro wrote: its log, its settings, the backup's
        // record, the folder destination.
        let written = everything_written(dir.path());
        assert!(written.iter().any(|(path, _)| path.ends_with("keevoro.log")));
        for (path, bytes) in written {
            let text = String::from_utf8_lossy(&bytes);
            assert!(!has_a_key(&text), "a key was written to {}", path.display());
        }
    }
}
