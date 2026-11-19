//! Everything Keevoro can do, said once.
//!
//! Three front ends sit on this crate: the desktop app, the `keevoro`
//! command line and, later, the tools an AI agent is given. None of them
//! should hold any logic of its own. Each translates between its own world
//! (a button, an argument, a tool call) and the operations here.
//!
//! There are three parts:
//!
//! - the [`register`], which lists every operation with its name, what it
//!   does, whether it changes anything and where it is offered;
//! - [`Backup`], one backup opened by its home folder, whose methods are
//!   the operations as plain Rust;
//! - [`call`], the same operations reached by name with JSON in and JSON
//!   out, which is how the command line and an agent's tools will use them.
//!
//! Not every operation in the register is done here yet. Those still done
//! by the app's shell say so (`implemented: false`), and [`call`] answers
//! them with a plain "not yet". The app's parity test keeps that honest.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod backup;
pub mod error;
pub mod register;

pub use backup::{Backup, FileTypeView, Found};
pub use error::{Error, Kind, Result};
pub use register::{entries, Agents, Changes, Entry, Op};

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

fn read<T: DeserializeOwned>(request: Value) -> Result<T> {
    // No request at all is the same as an empty one.
    let request = if request.is_null() {
        Value::Object(serde_json::Map::new())
    } else {
        request
    };
    serde_json::from_value(request).map_err(Error::not_understood)
}

fn answer(value: impl Serialize) -> Result<Value> {
    Ok(serde_json::to_value(value)?)
}

/// Does one operation, by name, with its request and answer as JSON. This
/// is the way in for the command line and an agent's tools. `backup` is
/// the backup it works on; an operation that needs one fails plainly
/// without it.
pub fn call(op: Op, backup: Option<&mut Backup>, request: Value) -> Result<Value> {
    let open = || backup.ok_or_else(Error::no_backup);
    match op {
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
        Op::DestinationsList => answer(open()?.destinations()?),
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

        // Still done by the app's shell, or not yet anywhere. Each moves
        // here in a later step of the plan, and its register entry says
        // `implemented: true` when it does.
        Op::BackupsList
        | Op::BackupsStart
        | Op::BackupsRename
        | Op::BackupsForget
        | Op::PlacesList
        | Op::PlacesSurvey
        | Op::SourcesLook
        | Op::SourcesLookAgain
        | Op::PhotosPicture
        | Op::DestinationsAdd
        | Op::DestinationsRemove
        | Op::DestinationsRoom
        | Op::BackupRehearse
        | Op::BackupRun
        | Op::BackupCheck
        | Op::JobsStatus
        | Op::JobsFollow
        | Op::JobsPause
        | Op::JobsCarryOn
        | Op::JobsStop
        | Op::KeevoroActivity
        | Op::KeevoroLog
        | Op::KeevoroSettings
        | Op::KeevoroUpdate
        | Op::KeevoroReset
        | Op::AgentsAccess => Err(Error::not_yet(op)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backup::tests::one_backup;
    use serde_json::json;

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
        assert_eq!(
            call(Op::KeevoroAbout, None, Value::Null).unwrap()["version"],
            about.version
        );
    }

    #[test]
    fn the_register_says_truly_which_operations_are_done_here() {
        let (_dir, _card, mut backup) = one_backup();
        for entry in entries() {
            let result = call(entry.op, Some(&mut backup), json!({}));
            let not_yet = matches!(&result, Err(e) if e.kind == Kind::NotYet);
            assert_eq!(
                not_yet, !entry.implemented,
                "{} is marked implemented: {}, but calling it gave {result:?}",
                entry.name, entry.implemented
            );
        }
    }

    #[test]
    fn operations_are_reached_by_name_with_json() {
        let (_dir, _card, mut backup) = one_backup();
        let op = |name| Op::named(name).unwrap();

        let sources = call(op("sources.list"), Some(&mut backup), Value::Null).unwrap();
        let id = sources[0]["id"].clone();
        let types = call(op("sources.file_types"), Some(&mut backup), json!({ "source": id })).unwrap();
        assert_eq!(types[0]["ending"], "jpg");
        call(
            op("sources.include_type"),
            Some(&mut backup),
            json!({ "source": id, "ending": "png", "included": false }),
        )
        .unwrap();

        let found = call(op("found.summary"), Some(&mut backup), Value::Null).unwrap();
        assert_eq!(found["photos"], 3);

        let changed = call(
            op("collections.keep"),
            Some(&mut backup),
            json!({ "collection": "Holidays / Cornwall", "keep": false }),
        )
        .unwrap();
        assert_eq!(changed, 2);
        let photos = call(
            op("collections.photos"),
            Some(&mut backup),
            json!({ "collection": "Holidays / Cornwall" }),
        )
        .unwrap();
        assert_eq!(photos[0]["kept"], false);
        let first = photos[0]["id"].clone();
        call(
            op("photos.keep"),
            Some(&mut backup),
            json!({ "photo": first, "keep": true }),
        )
        .unwrap();

        assert_eq!(
            call(op("set_aside.list"), Some(&mut backup), Value::Null).unwrap(),
            json!([])
        );
        assert_eq!(
            call(op("duplicates.list"), Some(&mut backup), Value::Null)
                .unwrap()
                .as_array()
                .map(Vec::len),
            Some(1)
        );
        assert_eq!(
            call(op("destinations.list"), Some(&mut backup), Value::Null).unwrap(),
            json!([])
        );
        assert_eq!(
            call(op("backup.history"), Some(&mut backup), Value::Null).unwrap(),
            json!([])
        );
        assert_eq!(
            call(op("backup.last_summary"), Some(&mut backup), Value::Null).unwrap(),
            Value::Null
        );

        // Reading previews changes nothing; setting them does.
        assert_eq!(call(op("backup.previews"), Some(&mut backup), json!({})).unwrap(), true);
        assert_eq!(
            call(op("backup.previews"), Some(&mut backup), json!({ "wanted": false })).unwrap(),
            false
        );
        assert!(!backup.previews().unwrap());

        let forgot = call(op("sources.forget"), Some(&mut backup), json!({ "source": id })).unwrap();
        assert_eq!(forgot, true);
        assert_eq!(
            call(op("sources.list"), Some(&mut backup), Value::Null).unwrap(),
            json!([])
        );
    }

    #[test]
    fn what_cannot_be_done_is_said_plainly() {
        let (_dir, _card, mut backup) = one_backup();
        let e = call(Op::SourcesList, None, Value::Null).unwrap_err();
        assert_eq!((e.kind, e.message.as_str()), (Kind::NoBackup, "Choose a backup first."));

        let e = call(Op::PhotosKeep, Some(&mut backup), json!({ "keep": true })).unwrap_err();
        assert_eq!(e.kind, Kind::NotUnderstood);
        assert!(e.message.contains("photo"), "{e}");
        let e = call(
            Op::SourcesForget,
            Some(&mut backup),
            json!({ "source": 1, "everything": true }),
        )
        .unwrap_err();
        assert_eq!(
            e.kind,
            Kind::NotUnderstood,
            "an unexpected field is not quietly ignored"
        );

        let e = call(
            Op::PhotosKeep,
            Some(&mut backup),
            json!({ "photo": "0000", "keep": true }),
        )
        .unwrap_err();
        assert_eq!(e.kind, Kind::Failed);
        assert_eq!(e.message, "That photo isn't known to this library.");

        let e = call(Op::BackupRun, Some(&mut backup), Value::Null).unwrap_err();
        assert_eq!(e.kind, Kind::NotYet);
    }
}
