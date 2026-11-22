//! `file_kinds.*`: the kinds of file Keevoro looks for, which a person can
//! add to, change and put back.
//!
//! The list is one for the whole of Keevoro, kept in its settings file
//! beside the list of backups, and changed under the same lock, so two
//! programs changing it at once never undo each other. Only the person's
//! changes are written down; the rules are in `keevoro_core::kinds`. A
//! change counts from the next time a source is looked through or a place
//! surveyed: each of those reads the list once, as it starts. Nothing
//! already found or backed up is ever taken away by a change here.
//!
//! Any ending can be added as an other file (music, documents, project
//! files), even one Keevoro passes over as never a photo or video; it is
//! then found, backed up and proved exactly, but never shown or grouped by
//! date. An AI agent may read the list but never change it.

use crate::error::{Error, Result};
use crate::keevoro::Keevoro;
use keevoro_core::{FileKind, KindChange, KindRefusal, Kinds, MediaKind};
use serde::{Deserialize, Serialize};

/// What files with an ending are listed as, as a request says it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ListedAs {
    Photo,
    Video,
    /// An other file, carried along: backed up exactly, never shown or
    /// grouped by date.
    Other,
}

impl From<ListedAs> for MediaKind {
    fn from(kind: ListedAs) -> Self {
        match kind {
            ListedAs::Photo => MediaKind::Photo,
            ListedAs::Video => MediaKind::Video,
            ListedAs::Other => MediaKind::Other,
        }
    }
}

/// `file_kinds.add`
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AddFileKindRequest {
    /// The ending of the files, such as `heif`, with or without its dot.
    pub ending: String,
    /// `photo`, `video` or `other`. Documents, music, programs and
    /// archives can only be `other`.
    pub kind: ListedAs,
}

/// `file_kinds.change`: anything left out stays as it is.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChangeFileKindRequest {
    /// The ending as it is on the list now.
    pub ending: String,
    /// `photo`, `video` or `other`.
    #[serde(default)]
    pub kind: Option<ListedAs>,
    /// True to look for these files, false to pass them over.
    #[serde(default)]
    pub on: Option<bool>,
    /// A new ending, for one added by the person. One Keevoro comes with
    /// cannot be renamed.
    #[serde(default)]
    pub to: Option<String>,
}

/// `file_kinds.remove`
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FileKindRequest {
    /// The ending, such as `heif`, with or without its dot.
    pub ending: String,
}

fn refused(refusal: KindRefusal) -> Error {
    match refusal {
        KindRefusal::NotListed(_) => Error::not_found(refusal.to_string()),
        other => Error::refused(other.to_string()),
    }
}

fn plural(kind: MediaKind) -> &'static str {
    match kind {
        MediaKind::Photo => "photos",
        MediaKind::Video => "videos",
        MediaKind::Other => "other files",
    }
}

impl Keevoro {
    /// `file_kinds.list`: every kind Keevoro looks for or could, the
    /// built-in ones first, each marked usual or less common, and the few
    /// endings that are often something else marked unsure.
    pub fn file_kinds(&self) -> Vec<FileKind> {
        self.backups().read().file_kinds.list()
    }

    /// The list as a lookup, read once as a search or a survey starts.
    pub(crate) fn kinds_to_look_for(&self) -> Kinds {
        self.backups().read().file_kinds.lookup()
    }

    /// `file_kinds.add`
    pub fn add_file_kind(&self, ending: &str, kind: MediaKind) -> Result<FileKind> {
        let added = self
            .backups()
            .change(|settings| settings.file_kinds.add(ending, kind).map_err(refused))?;
        self.log().say(&format!(
            "Keevoro now looks for .{} files, as {}, from the next time a source is looked through{}",
            added.ending,
            plural(added.kind),
            if added.kind == MediaKind::Other {
                format!(". {}", keevoro_core::OTHER_FILES_SAID.trim_end_matches('.'))
            } else {
                String::new()
            }
        ));
        Ok(added)
    }

    /// `file_kinds.change`
    pub fn change_file_kind(&self, ending: &str, change: &KindChange) -> Result<FileKind> {
        let before = self
            .file_kinds()
            .into_iter()
            .find(|k| Some(&k.ending) == keevoro_core::plain_ending(ending).ok().as_ref());
        let changed = self
            .backups()
            .change(|settings| settings.file_kinds.change(ending, change).map_err(refused))?;
        let mut said = Vec::new();
        if let Some(before) = before {
            if before.ending != changed.ending {
                said.push(format!("Renamed .{} to .{}", before.ending, changed.ending));
            }
            if before.kind != changed.kind {
                said.push(format!(".{} files are now {}", changed.ending, plural(changed.kind)));
            }
            if before.on != changed.on {
                said.push(if changed.on {
                    format!("Keevoro looks for .{} files again", changed.ending)
                } else {
                    format!(
                        "Keevoro no longer looks for .{} files. Those already found stay found",
                        changed.ending
                    )
                });
            }
        }
        for line in said {
            self.log().say(&line);
        }
        Ok(changed)
    }

    /// `file_kinds.remove`: only an ending the person added.
    pub fn remove_file_kind(&self, ending: &str) -> Result<FileKind> {
        let gone = self
            .backups()
            .change(|settings| settings.file_kinds.remove(ending).map_err(refused))?;
        self.log().say(&format!(
            "Took .{} off the kinds of file Keevoro looks for. Those already found stay found",
            gone.ending
        ));
        Ok(gone)
    }

    /// `file_kinds.reset`: every built-in kind back as it came. The
    /// person's own kinds stay.
    pub fn reset_file_kinds(&self) -> Result<Vec<FileKind>> {
        self.backups().change(|settings| {
            settings.file_kinds.put_back_built_in();
            Ok(())
        })?;
        self.log()
            .say("Put back the kinds of file Keevoro comes with, as they came");
        Ok(self.file_kinds())
    }
}

#[cfg(test)]
mod tests {
    use crate::keevoro::tests::keevoro;
    use crate::{call, Kind, Op};
    use serde_json::json;

    #[test]
    fn the_list_can_be_added_to_changed_and_put_back_and_another_program_sees_it_at_once() {
        let dir = tempfile::tempdir().unwrap();
        let k = keevoro(dir.path());
        let other = keevoro(dir.path());
        let list = call(&k, Op::FileKindsList, None, json!({})).unwrap();
        assert!(list
            .as_array()
            .unwrap()
            .iter()
            .all(|k| k["builtIn"] == true && k["on"] == (k["unsure"] == false)));

        let added = call(&k, Op::FileKindsAdd, None, json!({ "ending": ".XYZ", "kind": "photo" })).unwrap();
        assert_eq!(
            added,
            json!({ "ending": "xyz", "kind": "photo", "on": true, "builtIn": false, "usual": true, "unsure": false })
        );
        assert_eq!(
            other.file_kinds().last().unwrap().ending,
            "xyz",
            "seen by another program"
        );

        let e = call(
            &other,
            Op::FileKindsAdd,
            None,
            json!({ "ending": "xyz", "kind": "video" }),
        )
        .unwrap_err();
        assert_eq!(
            (e.kind, e.message.as_str()),
            (Kind::Refused, "Keevoro already looks for .xyz files.")
        );
        let e = call(
            &k,
            Op::FileKindsAdd,
            None,
            json!({ "ending": "my pics", "kind": "photo" }),
        )
        .unwrap_err();
        assert_eq!(e.kind, Kind::Refused);
        assert!(call(&k, Op::FileKindsAdd, None, json!({ "ending": "x", "kind": "music" })).is_err());
        // A document is never a photo, but can be carried along as an
        // other file.
        let e = call(&k, Op::FileKindsAdd, None, json!({ "ending": "pdf", "kind": "photo" })).unwrap_err();
        assert!(e.message.contains("add .pdf as an other file"), "{}", e.message);
        let flac = call(&k, Op::FileKindsAdd, None, json!({ "ending": "flac", "kind": "other" })).unwrap();
        assert_eq!(
            flac,
            json!({ "ending": "flac", "kind": "other", "on": true, "builtIn": false, "usual": true, "unsure": false })
        );
        assert!(k.lists_other_files());
        assert!(k.log_tail(5).contains("as other files"));
        call(&k, Op::FileKindsRemove, None, json!({ "ending": "flac" })).unwrap();

        let off = call(
            &other,
            Op::FileKindsChange,
            None,
            json!({ "ending": "avi", "on": false }),
        )
        .unwrap();
        assert_eq!(off["on"], false);
        let renamed = call(
            &k,
            Op::FileKindsChange,
            None,
            json!({ "ending": "xyz", "to": "insx", "kind": "video" }),
        )
        .unwrap();
        assert_eq!(
            (renamed["ending"].as_str(), renamed["kind"].as_str()),
            (Some("insx"), Some("video"))
        );
        let e = call(&k, Op::FileKindsChange, None, json!({ "ending": "jpg", "to": "jpx" })).unwrap_err();
        assert_eq!(e.kind, Kind::Refused);
        let e = call(&k, Op::FileKindsRemove, None, json!({ "ending": "jpg" })).unwrap_err();
        assert_eq!(
            e.message,
            ".jpg is one of the kinds Keevoro comes with. It can be switched off, but not removed or renamed."
        );
        let e = call(&k, Op::FileKindsRemove, None, json!({ "ending": "nothing" })).unwrap_err();
        assert_eq!(e.kind, Kind::NotFound);

        let reset = call(&k, Op::FileKindsReset, None, json!({})).unwrap();
        let reset = reset.as_array().unwrap();
        assert!(reset
            .iter()
            .filter(|k| k["builtIn"] == true)
            .all(|k| k["on"] == (k["unsure"] == false)));
        assert_eq!(reset.last().unwrap()["ending"], "insx", "your own stay");
        call(&k, Op::FileKindsRemove, None, json!({ "ending": "insx" })).unwrap();
        assert!(k.backups().read().file_kinds.is_built_in());

        let log = k.log_tail(20);
        for line in [
            "Keevoro now looks for .xyz files, as photos",
            "Renamed .xyz to .insx",
            ".insx files are now videos",
            "Took .insx off the kinds of file Keevoro looks for",
            "Put back the kinds of file Keevoro comes with",
        ] {
            assert!(log.contains(line), "{line}\n{log}");
        }
        assert!(other.log_tail(20).contains("Keevoro no longer looks for .avi files"));
    }

    #[test]
    fn an_ai_agent_may_read_the_list_but_never_change_it_even_when_allowed_changes() {
        let dir = tempfile::tempdir().unwrap();
        let k = keevoro(dir.path());
        k.set_agent_access(crate::AgentsAccessRequest {
            look_in: None,
            back_up_to: None,
            may_change: Some(true),
        })
        .unwrap();
        let list = crate::call_for_agent(&k, Op::FileKindsList, None, json!({})).unwrap();
        assert!(!list.as_array().unwrap().is_empty());
        for (op, request) in [
            (Op::FileKindsAdd, json!({ "ending": "xyz", "kind": "photo" })),
            (Op::FileKindsChange, json!({ "ending": "jpg", "on": false })),
            (Op::FileKindsRemove, json!({ "ending": "xyz" })),
            (Op::FileKindsReset, json!({})),
        ] {
            let e = crate::call_for_agent(&k, op, None, request).unwrap_err();
            assert_eq!(e.kind, Kind::Refused, "{}", op.name());
        }
        assert!(k.backups().read().file_kinds.is_built_in(), "nothing changed");
    }

    #[test]
    fn a_settings_file_with_no_changes_to_the_list_is_written_as_before() {
        let dir = tempfile::tempdir().unwrap();
        let k = keevoro(dir.path());
        k.start_backup("Beach").unwrap();
        let text = std::fs::read_to_string(dir.path().join("config").join("settings.json")).unwrap();
        assert!(!text.contains("file_kinds"), "{text}");
        k.add_file_kind("xyz", keevoro_core::MediaKind::Photo).unwrap();
        let text = std::fs::read_to_string(dir.path().join("config").join("settings.json")).unwrap();
        assert!(text.contains("\"file_kinds\""), "{text}");
    }
}
