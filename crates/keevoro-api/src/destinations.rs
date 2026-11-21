//! Where a backup goes: adding and removing places, and weighing what is
//! waiting against the room each one has.
//!
//! Keys for cloud storage pass through here on their way to the
//! computer's keychain, and must go nowhere else. So:
//!
//! - a key is held in a [`Key`], which cannot be written out: its `Debug`
//!   form is `[hidden]`, it has no `Display` and it cannot be serialised;
//! - nothing this module answers has a key in it, only the place in words;
//! - an error that happens while a key is in hand has the key taken out
//!   of its words before anyone sees them, in case some part of the
//!   storage's reply repeated it;
//! - nothing here writes to the log; what the caller logs is the place in
//!   words.

use crate::backup::Backup;
use crate::error::{Error, Result};
use keevoro_engine::connect::{self, Flags, Keys, KeysFrom, Saved, SecretStore, World};
use keevoro_engine::summary;
use serde::{Deserialize, Deserializer, Serialize};
use std::path::Path;

/// One storage key, on its way to the keychain. It cannot be shown,
/// printed or written out: only the code that hands it to the keychain
/// reads it.
#[derive(Clone, Default)]
pub struct Key(String);

impl Key {
    pub fn new(key: impl Into<String>) -> Self {
        Self(key.into())
    }

    pub(crate) fn reveal(&self) -> &str {
        self.0.trim()
    }
}

impl std::fmt::Debug for Key {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("[hidden]")
    }
}

// Read by hand so that something that is not text is refused without
// repeating it, as the usual error would.
impl<'de> Deserialize<'de> for Key {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        match serde_json::Value::deserialize(deserializer)? {
            serde_json::Value::String(key) => Ok(Self(key)),
            _ => Err(serde::de::Error::custom("a storage key must be written as text")),
        }
    }
}

// Described as the text it is written as. The description says nothing of
// any key in particular.
impl schemars::JsonSchema for Key {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Key".into()
    }

    fn inline_schema() -> bool {
        true
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({ "type": "string" })
    }
}

/// What kind of place a destination is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum DestinationKind {
    /// A folder: an external drive, a network share, anywhere on disk.
    Folder,
    /// A Cloudflare R2 bucket; the place is the account ID.
    R2,
    /// An Amazon S3 bucket; the place is the region.
    S3,
    /// Any other S3-compatible storage; the place is its address.
    Endpoint,
}

/// `destinations.add`. Its `Debug` form never shows the keys.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AddDestinationRequest {
    pub kind: DestinationKind,
    /// The folder, Cloudflare account ID, Amazon region or server address.
    pub place: String,
    /// The bucket, for cloud storage.
    #[serde(default)]
    pub bucket: Option<String>,
    /// For cloud storage: the access key ID. Stored in the computer's
    /// keychain, never anywhere else, and never given back.
    #[serde(default)]
    pub access_key_id: Option<Key>,
    /// For cloud storage: the secret access key. Stored in the computer's
    /// keychain, never anywhere else, and never given back.
    #[serde(default)]
    pub secret_access_key: Option<Key>,
}

/// What `destinations.add` answers: the place in words, and where it is in
/// the list. Never the keys.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DestinationAdded {
    /// "the Cloudflare R2 bucket family-photos"
    pub name: String,
    /// Its place in `destinations.list`.
    pub at: usize,
    /// Said only when no keys were given and those set in the environment
    /// were tested and saved instead: which variables, never the keys.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keys: Option<String>,
}

/// `destinations.remove`: one place by its place in the list, or all of
/// them, said outright.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RemoveDestinationRequest {
    /// Which destination, by its place in `destinations.list`, counting
    /// from 0.
    #[serde(default)]
    pub at: Option<usize>,
    /// True to remove every destination, instead of one.
    #[serde(default)]
    pub all: bool,
}

/// What is waiting for one destination, and whether it has the room.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WaitingAt {
    /// In words: "the folder F:\\Family Backup".
    pub name: String,
    /// `folder`, `r2`, `s3` or `endpoint`.
    pub kind: String,
    /// The folder, when it is one.
    pub folder: Option<String>,
    /// Kept files already there.
    pub there: usize,
    /// Kept files still to go there, and the room they need.
    pub files: usize,
    pub bytes: u64,
    /// Bytes free on a folder's disk, when the system will say. Cloud
    /// storage has no disk to measure.
    pub free: Option<u64>,
    /// False only when the files are known not to fit.
    pub enough: bool,
    /// What Keevoro found when it last looked there. Not said before it
    /// has looked.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub looked: Option<keevoro_engine::confirm::LookedView>,
}

/// The one line the app shows about room: the destination most worth
/// mentioning, which is the first without room or, failing that, the
/// first folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoomAtAGlance {
    /// Bytes waiting to be sent.
    pub needed: u64,
    /// The folder being sent to, when it is a folder.
    pub folder: Option<String>,
    /// Bytes free on that folder's disk, when the system will say.
    pub free: Option<u64>,
    /// False only when the files are known not to fit.
    pub enough: bool,
    /// For each bucket source, about what backing up now would cost to
    /// read out of it, a sentence each, said before Back up now. Left out
    /// when the backup reads from no cloud storage.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub cost: Vec<String>,
}

/// `destinations.room`
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Room {
    /// Each destination, in the order they were added.
    pub places: Vec<WaitingAt>,
    pub at_a_glance: RoomAtAGlance,
}

/// `backup.rehearse`: what backing up now would do. Nothing is sent or
/// changed by working it out; only Keevoro's count of what is at each
/// destination is brought up to date with what is really there.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Rehearsal {
    /// What would go to each destination.
    pub to: Vec<WaitingAt>,
    /// Everything that would be sent, added up over every destination.
    pub files: usize,
    pub bytes: u64,
    /// Kept files that are not yet safely everywhere they should be.
    pub not_yet_safe: usize,
    /// The same, said as a person would want to hear it, a sentence each.
    pub said: Vec<String>,
}

impl Backup {
    /// Each place this backup goes, in words, read from its saved list
    /// without counting anything: quick enough to ask at any time.
    pub fn destination_names(&self) -> Result<Vec<String>> {
        Ok(connect::destinations(self.catalog())?
            .iter()
            .map(Saved::describe)
            .collect())
    }

    /// Where this backup goes, in a few words: the one place, or how many.
    /// `None` when it goes nowhere yet.
    pub fn destinations_in_words(&self) -> Result<Option<String>> {
        let names = self.destination_names()?;
        Ok(match names.as_slice() {
            [] => None,
            [one] => Some(one.clone()),
            several => Some(format!("{} places", several.len())),
        })
    }

    /// `destinations.add`: checks the place works, then adds it to those
    /// this backup goes to. Those already there stay. Adding one that is
    /// already there only refreshes its keys.
    ///
    /// The keys in the request are the ones tested and saved. Only when it
    /// gives none are those set in the environment used, and the answer
    /// then says so in [`DestinationAdded::keys`].
    pub fn add_destination(
        &self,
        request: &AddDestinationRequest,
        secrets: &dyn SecretStore,
        environment: &dyn Fn(&str) -> Option<String>,
    ) -> Result<DestinationAdded> {
        let access = request.access_key_id.as_ref().map(Key::reveal).unwrap_or_default();
        let secret = request.secret_access_key.as_ref().map(Key::reveal).unwrap_or_default();
        self.add_with_keys(request, access, secret, secrets, environment)
            .map_err(|e| without_keys(e, &[access, secret]))
    }

    fn add_with_keys(
        &self,
        request: &AddDestinationRequest,
        access: &str,
        secret: &str,
        secrets: &dyn SecretStore,
        environment: &dyn Fn(&str) -> Option<String>,
    ) -> Result<DestinationAdded> {
        let place = Some(request.place.trim().to_string()).filter(|p| !p.is_empty());
        let mut flags = Flags {
            bucket: request.bucket.clone().filter(|b| !b.trim().is_empty()),
            ..Flags::default()
        };
        match request.kind {
            DestinationKind::Folder => flags.to_folder = place.map(Into::into),
            DestinationKind::R2 => flags.to_r2 = place,
            DestinationKind::S3 => flags.to_s3 = place,
            DestinationKind::Endpoint => flags.to_endpoint = place,
        }
        let saved = Saved::from_flags(&flags).ok_or_else(|| Error::failed("Say where to back up to first.".into()))?;
        // Half a pair is a mistake to say, not a reason to use other keys.
        let given = Keys::new(access, secret);
        if given.is_none() && !(access.is_empty() && secret.is_empty()) && request.kind != DestinationKind::Folder {
            return Err(Error::failed("Enter both keys for the storage.".into()));
        }
        let world = World { secrets, environment };
        // Any warning (such as a publicly listable bucket) and the command
        // line's hints are written here, and not passed on.
        let from = connect::add(&mut Vec::new(), self.catalog(), &saved, given, &world).map_err(|e| {
            if e.downcast_ref::<connect::CannotReach>().is_some() {
                Error::unreachable(format!("{e:#}"))
            } else {
                Error::from(e)
            }
        })?;
        // What is there already is looked at now, so a place backed up to
        // before (by this backup, before it was taken off) is counted as it
        // really is from the start.
        if let Some(added) = connect::destinations(self.catalog())?
            .into_iter()
            .find(|d| d.same_place(&saved))
        {
            keevoro_engine::confirm::look(self.catalog(), &added, &world)?;
        }
        // What the last backup said was about the places as they were.
        let _ = self.forget_last_summary();
        let at = connect::destinations(self.catalog())?
            .iter()
            .position(|d| d.same_place(&saved))
            .unwrap_or_default();
        Ok(DestinationAdded {
            name: saved.describe(),
            at,
            keys: (from == KeysFrom::Environment).then(|| connect::environment_keys_saved(&saved)),
        })
    }

    /// `destinations.remove`: takes one place off this backup by its place
    /// in the list, or every place when `at` is `None`, and removes their
    /// keys from the keychain. Nothing stored at any of them is touched.
    /// Answers the places removed, in words.
    pub fn remove_destination(&self, at: Option<usize>, secrets: &dyn SecretStore) -> Result<Vec<String>> {
        // What the last backup said was about the destinations as they were.
        let _ = self.forget_last_summary();
        match at {
            Some(at) => Ok(connect::remove(self.catalog(), secrets, at)?
                .iter()
                .map(Saved::describe)
                .collect()),
            None => {
                let names = self.destination_names()?;
                connect::disconnect(&mut Vec::new(), self.catalog(), secrets)?;
                Ok(names)
            }
        }
    }

    /// What is waiting for each destination, weighed against its room.
    fn waiting_at(&self) -> Result<(keevoro_engine::Discover, Vec<WaitingAt>)> {
        let found = self.discover()?;
        let places = found
            .destinations
            .iter()
            .map(|d| {
                let room = d
                    .folder
                    .as_ref()
                    .and_then(|folder| keevoro_engine::room::room_in(Path::new(folder), d.waiting_bytes));
                WaitingAt {
                    name: d.name.clone(),
                    kind: d.kind.clone(),
                    folder: d.folder.clone(),
                    there: d.there,
                    files: d.waiting,
                    bytes: d.waiting_bytes,
                    free: room.map(|r| r.free),
                    enough: room.is_none_or(|r| r.is_enough()),
                    looked: d.looked.clone(),
                }
            })
            .collect();
        Ok((found, places))
    }

    /// `destinations.room`, as the backup screen asks it when it opens:
    /// each destination is looked at first unless it was a moment ago
    /// (`keevoro_engine::confirm::FRESH_FOR`), so what is waiting is what
    /// is really missing there.
    pub fn room_looking(&self, world: &World<'_>) -> Result<Room> {
        keevoro_engine::confirm::look_where_due(self.catalog(), world, keevoro_engine::confirm::now())?;
        self.room()
    }

    /// `destinations.room` from what Keevoro knows, asking no destination.
    pub fn room(&self) -> Result<Room> {
        let (found, places) = self.waiting_at()?;
        let folders: Vec<&WaitingAt> = places.iter().filter(|p| p.folder.is_some()).collect();
        let chosen = folders
            .iter()
            .find(|p| p.free.is_some() && !p.enough)
            .or(folders.first());
        let at_a_glance = match chosen {
            Some(p) => RoomAtAGlance {
                needed: p.bytes,
                folder: p.folder.clone(),
                free: p.free,
                enough: p.enough,
                cost: Vec::new(),
            },
            None => RoomAtAGlance {
                needed: found.waiting_bytes,
                folder: None,
                free: None,
                enough: true,
                cost: Vec::new(),
            },
        };
        let at_a_glance = RoomAtAGlance {
            cost: found.from_cloud.iter().map(|c| c.cost.clone()).collect(),
            ..at_a_glance
        };
        Ok(Room { places, at_a_glance })
    }

    /// `backup.rehearse`: what backing up now would send, and to where.
    /// Each destination is looked at first, as a backup does, so what has
    /// gone from one is said before anything else, and one that cannot be
    /// reached is said to be out of reach, never empty.
    pub fn rehearse(&self, world: &World<'_>) -> Result<Rehearsal> {
        keevoro_engine::confirm::look_everywhere(self.catalog(), world)?;
        let (found, to) = self.waiting_at()?;
        let mut said = Vec::new();
        if to.is_empty() {
            said.push("There is nowhere to back up to yet. Add a destination first.".to_string());
        }
        let out_of_reach = |place: &WaitingAt| place.looked.as_ref().is_some_and(|l| !l.reached);
        for place in &to {
            if let Some(looked) = &place.looked {
                said.extend(looked.said.iter().cloned());
            }
        }
        let files = to.iter().filter(|p| !out_of_reach(p)).map(|p| p.files).sum();
        let bytes = to.iter().filter(|p| !out_of_reach(p)).map(|p| p.bytes).sum();
        for place in &to {
            // Already said, and a backup will not go there.
            if out_of_reach(place) {
                continue;
            }
            if place.files == 0 {
                said.push(format!("Everything being kept is already at {}.", place.name));
                continue;
            }
            said.push(format!(
                "{} to {}.",
                files_and_size(place.files, place.bytes),
                place.name
            ));
            if let (false, Some(free), Some(folder)) = (place.enough, place.free, &place.folder) {
                let room = keevoro_engine::room::Room {
                    needed: place.bytes,
                    free,
                };
                said.push(room.refusal(Path::new(folder)));
            }
        }
        // What reading from cloud storage would cost, before anything starts.
        said.extend(found.from_cloud.iter().map(|c| c.cost.clone()));
        Ok(Rehearsal {
            to,
            files,
            bytes,
            not_yet_safe: found.waiting_to_gather,
            said,
        })
    }
}

/// "1 file (2.1 MB)", "3,204 files (41 GB)"
fn files_and_size(files: usize, bytes: u64) -> String {
    format!("{} ({})", summary::count(files, "file"), summary::bytes(bytes))
}

/// Takes any key out of an error's words, in case some part of what went
/// wrong repeated it.
pub(crate) fn without_keys(mut e: Error, keys: &[&str]) -> Error {
    for key in keys.iter().filter(|k| !k.is_empty()) {
        e.message = e.message.replace(key, "[hidden]");
    }
    e
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backup::tests::one_backup;
    use keevoro_engine::connect::testing::{no_environment, MemorySecrets};

    fn folder(place: &Path) -> AddDestinationRequest {
        AddDestinationRequest {
            kind: DestinationKind::Folder,
            place: place.display().to_string(),
            bucket: None,
            access_key_id: None,
            secret_access_key: None,
        }
    }

    #[test]
    fn a_folder_is_added_checked_and_named() {
        let (dir, _card, backup) = one_backup();
        assert_eq!(backup.destinations_in_words().unwrap(), None);
        let secrets = MemorySecrets::default();
        let usb = dir.path().join("USB stick");
        let added = backup
            .add_destination(&folder(&usb), &secrets, &no_environment)
            .unwrap();
        assert_eq!(added.name, format!("the folder {}", usb.display()));
        assert_eq!(added.at, 0);
        assert!(usb.is_dir(), "checking it works makes the folder");
        assert!(secrets.0.borrow().is_empty(), "a folder needs no keys");
        assert_eq!(backup.destination_names().unwrap(), std::slice::from_ref(&added.name));
        assert_eq!(backup.destinations_in_words().unwrap(), Some(added.name.clone()));

        let nas = dir.path().join("NAS");
        assert_eq!(
            backup
                .add_destination(&folder(&nas), &secrets, &no_environment)
                .unwrap()
                .at,
            1
        );
        assert_eq!(backup.destinations_in_words().unwrap().as_deref(), Some("2 places"));
        // Adding one that is there already adds nothing.
        assert_eq!(
            backup
                .add_destination(&folder(&usb), &secrets, &no_environment)
                .unwrap()
                .at,
            0
        );
        assert_eq!(backup.destination_names().unwrap().len(), 2);
    }

    #[test]
    fn adding_a_destination_means_the_last_summary_no_longer_applies() {
        let (dir, _card, backup) = one_backup();
        backup
            .remember_last_summary(&serde_json::json!({ "finished": true }))
            .unwrap();
        backup
            .add_destination(
                &folder(&dir.path().join("USB")),
                &MemorySecrets::default(),
                &no_environment,
            )
            .unwrap();
        assert_eq!(backup.last_summary().unwrap(), None);
    }

    #[test]
    fn a_place_that_is_not_said_is_refused_plainly() {
        let (_dir, _card, backup) = one_backup();
        let mut request = folder(Path::new("  "));
        let e = backup
            .add_destination(&request, &MemorySecrets::default(), &no_environment)
            .unwrap_err();
        assert_eq!(e.message, "Say where to back up to first.");
        request.kind = DestinationKind::R2;
        request.bucket = Some("family".into());
        let e = backup
            .add_destination(&request, &MemorySecrets::default(), &no_environment)
            .unwrap_err();
        assert_eq!(e.message, "Say where to back up to first.");
    }

    #[test]
    fn cloud_storage_without_both_keys_is_refused_before_anything_is_tried() {
        let (_dir, _card, backup) = one_backup();
        let request = AddDestinationRequest {
            kind: DestinationKind::R2,
            place: "0123456789abcdef0123456789abcdef".into(),
            bucket: Some("family".into()),
            access_key_id: Some(Key::new("AKIAONLYONE")),
            secret_access_key: None,
        };
        let e = backup
            .add_destination(&request, &MemorySecrets::default(), &no_environment)
            .unwrap_err();
        assert_eq!(e.message, "Enter both keys for the storage.");
        assert!(backup.destination_names().unwrap().is_empty());
    }

    #[test]
    fn one_place_or_all_of_them_can_be_removed_and_nothing_there_is_touched() {
        let (dir, _card, backup) = one_backup();
        let secrets = MemorySecrets::default();
        let (usb, nas) = (dir.path().join("USB"), dir.path().join("NAS"));
        for place in [&usb, &nas] {
            backup
                .add_destination(&folder(place), &secrets, &no_environment)
                .unwrap();
        }
        std::fs::write(usb.join("kept.txt"), "mine").unwrap();

        assert_eq!(
            backup.remove_destination(Some(0), &secrets).unwrap(),
            [format!("the folder {}", usb.display())]
        );
        assert_eq!(
            backup.destination_names().unwrap(),
            [format!("the folder {}", nas.display())]
        );
        assert!(backup.remove_destination(Some(5), &secrets).unwrap().is_empty());
        assert_eq!(
            backup.remove_destination(None, &secrets).unwrap(),
            [format!("the folder {}", nas.display())]
        );
        assert!(backup.destination_names().unwrap().is_empty());
        assert_eq!(std::fs::read_to_string(usb.join("kept.txt")).unwrap(), "mine");
        assert!(nas.is_dir());
    }

    #[test]
    fn removing_a_cloud_destination_removes_its_keys() {
        let (_dir, _card, backup) = one_backup();
        backup
            .catalog()
            .set_setting(
                "destinations",
                r#"[{"kind":"r2","place":"0123456789abcdef0123456789abcdef","bucket":"family"}]"#,
            )
            .unwrap();
        let secrets = MemorySecrets::default();
        secrets
            .set("r2:0123456789abcdef0123456789abcdef/family", "access\nsecret")
            .unwrap();
        assert_eq!(
            backup.remove_destination(Some(0), &secrets).unwrap(),
            ["the Cloudflare R2 bucket family"]
        );
        assert!(secrets.0.borrow().is_empty());
    }

    #[test]
    fn room_is_weighed_for_each_place_and_one_is_picked_for_a_glance() {
        let (dir, _card, backup) = one_backup();
        let none = backup.room().unwrap();
        assert!(none.places.is_empty());
        let found = backup.found().unwrap();
        assert_eq!(
            none.at_a_glance,
            RoomAtAGlance {
                needed: found.waiting_bytes,
                folder: None,
                free: None,
                enough: true,
                cost: Vec::new(),
            }
        );
        // What the app has always received.
        assert_eq!(
            serde_json::to_value(&none.at_a_glance).unwrap(),
            serde_json::json!({ "needed": found.waiting_bytes, "folder": null, "free": null, "enough": true })
        );

        let usb = dir.path().join("USB");
        backup
            .add_destination(&folder(&usb), &MemorySecrets::default(), &no_environment)
            .unwrap();
        let room = backup.room().unwrap();
        assert_eq!(room.places.len(), 1);
        let place = &room.places[0];
        assert_eq!((place.files, place.there, place.bytes), (3, 0, found.waiting_bytes));
        assert_eq!(place.folder.as_deref(), Some(usb.display().to_string().as_str()));
        assert!(
            place.free.is_some() && place.enough,
            "a temporary folder has room for three photos"
        );
        assert_eq!(room.at_a_glance.folder, place.folder);
        assert_eq!(room.at_a_glance.needed, place.bytes);
    }

    #[test]
    fn a_rehearsal_says_what_would_go_where_and_changes_nothing() {
        let (dir, _card, backup) = one_backup();
        let world = World {
            secrets: &MemorySecrets::default(),
            environment: &no_environment,
        };
        let nowhere = backup.rehearse(&world).unwrap();
        assert_eq!((nowhere.files, nowhere.bytes, nowhere.not_yet_safe), (0, 0, 3));
        assert_eq!(
            nowhere.said,
            ["There is nowhere to back up to yet. Add a destination first."]
        );

        let secrets = MemorySecrets::default();
        let (usb, nas) = (dir.path().join("USB"), dir.path().join("NAS"));
        for place in [&usb, &nas] {
            backup
                .add_destination(&folder(place), &secrets, &no_environment)
                .unwrap();
        }
        let before = backup.found().unwrap();
        let rehearsal = backup.rehearse(&world).unwrap();
        assert_eq!(rehearsal.to.len(), 2);
        assert_eq!(rehearsal.files, 6, "three files to each of two places");
        assert_eq!(rehearsal.bytes, 2 * before.waiting_bytes);
        assert_eq!(
            rehearsal.said[0],
            format!(
                "3 files ({}) to the folder {}.",
                summary::bytes(before.waiting_bytes),
                usb.display()
            )
        );
        for line in &rehearsal.said {
            assert!(!line.contains('\u{2014}'), "{line}");
        }
        assert_eq!(backup.found().unwrap(), before, "nothing changed");
        assert!(!usb.join("originals").exists(), "nothing was sent");
    }

    #[test]
    fn keys_are_taken_out_of_any_error_that_repeats_them() {
        let e = Error::failed("The key AKIAMARKER was refused, and so was s3cr3tMARKER.".into());
        let e = without_keys(e, &["AKIAMARKER", "s3cr3tMARKER", ""]);
        assert_eq!(e.message, "The key [hidden] was refused, and so was [hidden].");
    }

    #[test]
    fn a_key_cannot_be_printed_and_something_not_text_is_not_repeated() {
        let key = Key::new("AKIAMARKER");
        assert_eq!(format!("{key:?}"), "[hidden]");
        let e = serde_json::from_value::<AddDestinationRequest>(serde_json::json!({
            "kind": "r2", "place": "x", "accessKeyId": 987_654_321
        }))
        .unwrap_err();
        assert!(!e.to_string().contains("987654321"), "{e}");
        assert!(e.to_string().contains("must be written as text"), "{e}");
    }
}
