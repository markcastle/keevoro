//! Everything Keevoro can do, said once.
//!
//! Three front ends sit on this crate: the desktop app, the `keevoro`
//! command line and, later, the tools an AI agent is given. None of them
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
//!   out, which is how the command line and an agent's tools will use them.
//!
//! Not every operation in the register is done here yet. Those still done
//! by the app's shell say so (`implemented: false`), and [`call`] answers
//! them with a plain "not yet". The app's parity test keeps that honest.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod activity;
pub mod agents;
pub mod backup;
pub mod backups;
pub mod destinations;
pub mod error;
pub mod keevoro;
pub mod log;
pub mod picture;
pub mod places;
pub mod register;
pub mod reset;

pub use agents::{AgentAccess, AgentsAccessRequest};
pub use backup::{Backup, FileTypeView, Found};
pub use backups::{Backups, Named};
pub use destinations::{
    AddDestinationRequest, DestinationAdded, DestinationKind, Key, Rehearsal, RemoveDestinationRequest, Room,
    RoomAtAGlance, WaitingAt,
};
pub use error::{Error, Kind, Result};
pub use keevoro::{BackupListing, Codecs, Folders, Keevoro, Offer, SettingsView};
pub use picture::{Picture, PictureRequest, PictureSize};
pub use register::{entries, Agents, Changes, Entry, Op};

use keevoro::{ForgetBackupRequest, LogRequest, RenameBackupRequest, SettingsRequest, StartBackupRequest};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

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
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SourceRequest {
    /// The source's id, as `sources.list` gives it.
    pub source: i64,
}

/// `sources.include_type`
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct IncludeTypeRequest {
    pub source: i64,
    /// The ending, such as `png`, with or without its dot.
    pub ending: String,
    pub included: bool,
}

/// `collections.photos`
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CollectionRequest {
    /// The collection's key, as `collections.list` gives it.
    pub collection: String,
}

/// `collections.keep`
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct KeepCollectionRequest {
    pub collection: String,
    pub keep: bool,
}

/// `photos.keep`
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct KeepPhotoRequest {
    /// The photo's short id, as `collections.photos` gives it.
    pub photo: String,
    pub keep: bool,
}

/// `backup.previews`: without `wanted` it only reads.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PreviewsRequest {
    #[serde(default)]
    pub wanted: Option<bool>,
}

/// For operations that take nothing: anything given is a mistake worth
/// saying, above all for one that changes things.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Nothing {}

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
    match op {
        Op::BackupsList => {
            read::<Nothing>(request)?;
            answer(keevoro.list_backups())
        }
        Op::BackupsStart => {
            let r: StartBackupRequest = read(request)?;
            answer(keevoro.start_backup(&r.name)?)
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
        Op::SourcesList => answer(open()?.sources()?),
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
        Op::FoundSummary => answer(open()?.found()?),
        Op::CollectionsList => answer(open()?.collections()?),
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
        Op::SetAsideList => answer(open()?.set_aside()?),
        Op::DuplicatesList => answer(open()?.duplicates()?),
        Op::PhotosPicture => {
            let r: PictureRequest = read(request)?;
            let bytes = keevoro.picture(&open()?, &r.photo, r.size)?;
            answer(Picture::webp(&bytes))
        }
        Op::DestinationsList => answer(open()?.destinations()?),
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
        Op::DestinationsRoom => {
            read::<Nothing>(request)?;
            answer(open()?.room()?)
        }
        Op::BackupRehearse => {
            read::<Nothing>(request)?;
            answer(open()?.rehearse()?)
        }
        Op::BackupHistory => answer(open()?.history()?),
        Op::BackupLastSummary => answer(open()?.last_summary()?),
        Op::BackupPreviews => {
            let r: PreviewsRequest = read(request)?;
            let backup = open()?;
            if let Some(wanted) = r.wanted {
                backup.set_previews(wanted)?;
            }
            answer(backup.previews()?)
        }
        Op::KeevoroAbout => answer(about()),
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
        Op::AgentsAccess => {
            let r: AgentsAccessRequest = read(request)?;
            if r.look_in.is_none() && r.back_up_to.is_none() && r.may_change.is_none() {
                answer(keevoro.agent_access())
            } else {
                answer(keevoro.set_agent_access(r)?)
            }
        }

        // Jobs, which run for a while and are watched and stopped. They
        // are the next step of the plan; until then the app's shell runs
        // them, and their register entries say `implemented: false`.
        Op::PlacesSurvey
        | Op::SourcesLook
        | Op::SourcesLookAgain
        | Op::BackupRun
        | Op::BackupCheck
        | Op::JobsStatus
        | Op::JobsFollow
        | Op::JobsPause
        | Op::JobsCarryOn
        | Op::JobsStop => Err(Error::not_yet(op)),
    }
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
        let (_dir, _card, keevoro) = one_keevoro();
        // Resetting would take the backup away from the rest, so it is last.
        let mut all: Vec<Entry> = entries().collect();
        all.sort_by_key(|e| e.op == Op::KeevoroReset);
        for entry in all {
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
    fn the_operations_still_to_come_are_the_jobs() {
        let not_yet: Vec<&str> = entries().filter(|e| !e.implemented).map(|e| e.name).collect();
        assert_eq!(
            not_yet,
            [
                "places.survey",
                "sources.look",
                "sources.look_again",
                "backup.run",
                "backup.check",
                "jobs.status",
                "jobs.follow",
                "jobs.pause",
                "jobs.carry_on",
                "jobs.stop",
            ]
        );
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
        assert_eq!(e.message, "That photo isn't known to this library.");

        let e = call(k, Op::BackupRun, None, Value::Null).unwrap_err();
        assert_eq!(e.kind, Kind::NotYet);
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
        assert_eq!(e.kind, Kind::Failed);

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
