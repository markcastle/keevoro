//! `backup.wants` and `sources.wants`: what a backup, or one of its
//! sources, is for: any of photos, videos, music and other files, at least
//! one.
//!
//! It is a choice about what is offered and backed up from now on, made
//! over what was already found (see `keevoro_core::wants` for how it
//! composes with the kinds of file Keevoro looks for and each ending's own
//! choice). So it counts at once, with no new search, and every count,
//! the plan of a backup and the room it needs follow it. Nothing already
//! backed up is touched: files sent before stay where they are, and are
//! simply no longer kept in the selection.
//!
//! Other files are only offered as a choice, and only spoken of, once some
//! kind of other file is on Keevoro's list (`offersOthers`), so a person who
//! never adds one sees photos and videos as ever.
//!
//! Music is the one exception to "with no new search": a backup that is
//! not for music never looks for it, so nothing is spent on it. Once music
//! is asked for, where none was looked for before, the view says so
//! (`looksAgain`), and the sources are looked through again to find it.

use crate::backup::Backup;
use crate::error::{Error, Result};
use keevoro_core::Wants;
use serde::{Deserialize, Serialize};

/// One thing a backup can be for, as a request says it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum WantsWord {
    Photos,
    Videos,
    /// Songs, recordings and playlists.
    Music,
    /// Other files a person put on the kinds list.
    Others,
}

impl WantsWord {
    fn word(self) -> &'static str {
        match self {
            WantsWord::Photos => "photos",
            WantsWord::Videos => "videos",
            WantsWord::Music => "music",
            WantsWord::Others => "others",
        }
    }
}

/// What it is for, from the words a request gives; at least one.
pub fn wants_of(words: &[WantsWord]) -> Result<Wants> {
    Wants::from_tokens(words.iter().map(|w| w.word()))
        .ok_or_else(|| Error::not_understood("Say what it is for: one or more of photos, videos, music and others."))
}

/// Following the backup, as a request for a source says it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum FollowBackup {
    /// The same as the backup.
    Backup,
}

/// A source's choice: the backup's, or its own.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(untagged)]
pub enum SourceWantsChoice {
    /// `backup`: the same as the backup.
    Backup(FollowBackup),
    /// Its own: one or more of `photos`, `videos`, `music` and `others`.
    Own(Vec<WantsWord>),
}

impl SourceWantsChoice {
    pub fn wants(&self) -> Result<Option<Wants>> {
        match self {
            SourceWantsChoice::Backup(_) => Ok(None),
            SourceWantsChoice::Own(words) => wants_of(words).map(Some),
        }
    }
}

/// `backup.wants`: without `wants` it only reads.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BackupWantsRequest {
    /// One or more of `photos`, `videos`, `music` and `others`. Left out,
    /// nothing changes.
    #[serde(default)]
    pub wants: Option<Vec<WantsWord>>,
}

/// `sources.wants`: without `wants` it only reads.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SourceWantsRequest {
    /// The source's id, as `sources.list` gives it.
    pub source: i64,
    /// `backup` (the same as the backup), or one or more of `photos`,
    /// `videos`, `music` and `others`. Left out, nothing changes.
    #[serde(default)]
    pub wants: Option<SourceWantsChoice>,
}

fn not(b: &bool) -> bool {
    !b
}

/// What a backup or a source is for, and what that means now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WantsView {
    /// What is in effect: one or more of `photos`, `videos`, `music` and
    /// `others`.
    pub wants: Wants,
    /// For a source: true when it has its own choice, false when it
    /// follows the backup's.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub own: Option<bool>,
    /// True once some kind of other file is on Keevoro's list, so "Other
    /// files" is a choice to offer.
    #[serde(skip_serializing_if = "not")]
    pub offers_others: bool,
    /// In words: "This backup is for photos only."
    pub said: String,
    /// True when this change asked for music where none was looked for
    /// before: the sources are to be looked through again to find it.
    #[serde(skip_serializing_if = "not")]
    pub looks_again: bool,
}

/// What is said when music is asked for where none was looked for before.
pub const LOOKS_FOR_MUSIC: &str = "Look through your sources again to find your music.";

/// What is said when the choice changes, so nobody fears for what is
/// already safe.
pub const ALREADY_SAFE: &str =
    "Anything already backed up stays backed up; it is just no longer kept in what this backup chooses.";

impl Backup {
    /// `backup.wants`, read. Other files are spoken of only when some kind
    /// of them is on the list (`others_listed`).
    pub fn wants(&self, others_listed: bool) -> Result<WantsView> {
        let wants = self.catalog().wants()?;
        Ok(WantsView {
            wants,
            own: None,
            offers_others: others_listed,
            said: format!("This backup is for {}.", wants.words(others_listed)),
            looks_again: false,
        })
    }

    /// `backup.wants`, set.
    pub fn set_wants(&self, wants: Wants, others_listed: bool) -> Result<WantsView> {
        let before = self.catalog().wants()?;
        let music_before = self.catalog().looks_for_music()?;
        self.catalog().set_wants(wants)?;
        let mut view = self.wants(others_listed)?;
        if !wants.takes_all_of(before) {
            view.said = format!("{} {ALREADY_SAFE}", view.said);
        }
        view.looks_again = !music_before && self.catalog().looks_for_music()?;
        if view.looks_again {
            view.said = format!("{} {LOOKS_FOR_MUSIC}", view.said);
        }
        Ok(view)
    }

    /// `sources.wants`, read.
    pub fn source_wants(&self, source: i64, others_listed: bool) -> Result<WantsView> {
        if !self.sources()?.iter().any(|s| s.id == source) {
            return Err(Error::not_found("That source isn't part of this backup."));
        }
        let own = self.catalog().source_wants(source)?;
        let wants = own.unwrap_or(self.catalog().wants()?);
        Ok(WantsView {
            wants,
            own: Some(own.is_some()),
            offers_others: others_listed,
            said: match own {
                Some(own) => format!("This source is set to {}.", own.words(others_listed)),
                None => format!("This source follows the backup: {}.", wants.words(others_listed)),
            },
            looks_again: false,
        })
    }

    /// `sources.wants`, set: its own choice, or with `None` the backup's.
    pub fn set_source_wants(&self, source: i64, wants: Option<Wants>, others_listed: bool) -> Result<WantsView> {
        let before = self.source_wants(source, others_listed)?.wants;
        let music_before = self.catalog().looks_for_music()?;
        if !self.catalog().set_source_wants(source, wants)? {
            return Err(Error::not_found("That source isn't part of this backup."));
        }
        let mut view = self.source_wants(source, others_listed)?;
        if !view.wants.takes_all_of(before) {
            view.said = format!("{} {ALREADY_SAFE}", view.said);
        }
        view.looks_again = !music_before && self.catalog().looks_for_music()?;
        if view.looks_again {
            view.said = format!("{} {LOOKS_FOR_MUSIC}", view.said);
        }
        Ok(view)
    }
}

#[cfg(test)]
mod tests {
    use crate::backup::tests::one_backup;
    use keevoro_core::{MediaKind, Wants};

    #[test]
    fn photos_only_counts_at_once_everywhere_and_a_source_can_differ() {
        let (_dir, _card, backup) = one_backup();
        let before = backup.found().unwrap();
        assert!(before.photos > 0);
        assert_eq!(
            backup.wants(false).unwrap().said,
            "This backup is for photos and videos."
        );

        let set = backup.set_wants(Wants::only(MediaKind::Video), false).unwrap();
        assert!(set
            .said
            .starts_with("This backup is for videos only. Anything already backed up stays backed up"));
        let after = backup.found().unwrap();
        assert_eq!(
            (after.photos, after.videos),
            (0, before.videos),
            "counts follow at once"
        );
        assert_eq!(after.waiting, before.videos);

        let source = backup.sources().unwrap()[0].id;
        assert_eq!(
            backup.source_wants(source, false).unwrap().said,
            "This source follows the backup: videos only."
        );
        let own = backup.set_source_wants(source, Some(Wants::default()), false).unwrap();
        assert_eq!(own.own, Some(true));
        assert_eq!(backup.found().unwrap().photos, before.photos);
        let types = backup.file_types(source, false).unwrap();
        assert!(types.iter().all(|t| t.left_out_by.is_none()));

        backup.set_source_wants(source, None, false).unwrap();
        let types = backup.file_types(source, false).unwrap();
        let jpg = types.iter().find(|t| t.ending == "jpg").unwrap();
        assert_eq!(jpg.why.as_deref(), Some("this backup is for videos only"));
        assert_eq!(
            backup.source_wants(9999, false).unwrap_err().kind,
            crate::Kind::NotFound
        );
        assert_eq!(
            backup.set_source_wants(9999, None, false).unwrap_err().kind,
            crate::Kind::NotFound
        );
        // With other files on the list, they are named too.
        backup.set_wants(Wants::default(), true).unwrap();
        assert_eq!(
            backup.wants(true).unwrap().said,
            "This backup is for photos, videos and other files."
        );
        let photos = Wants {
            videos: false,
            others: false,
            ..Wants::default()
        };
        assert!(backup
            .set_wants(photos, true)
            .unwrap()
            .said
            .starts_with("This backup is for photos only."));
    }

    #[test]
    fn an_agent_may_read_it_and_change_it_only_once_changes_are_allowed_and_a_backup_can_start_with_it() {
        use crate::{call, call_for_agent, Op};
        use serde_json::json;
        let dir = tempfile::tempdir().unwrap();
        let k = crate::keevoro::tests::keevoro(dir.path());
        call(
            &k,
            Op::BackupsStart,
            None,
            json!({ "name": "Beach", "wants": ["videos"] }),
        )
        .unwrap();
        let read = call_for_agent(&k, Op::BackupWants, Some("Beach"), json!({})).unwrap();
        assert_eq!(read["wants"], json!(["videos"]));
        assert!(read.get("offersOthers").is_none(), "{read}");
        let e = call_for_agent(
            &k,
            Op::BackupWants,
            Some("Beach"),
            json!({ "wants": ["photos", "videos"] }),
        )
        .unwrap_err();
        assert_eq!(e.message, crate::for_agents::LOOK_ONLY);
        k.set_agent_access(crate::AgentsAccessRequest {
            look_in: None,
            back_up_to: None,
            may_change: Some(true),
        })
        .unwrap();
        let set = call_for_agent(&k, Op::BackupWants, Some("Beach"), json!({ "wants": ["photos"] })).unwrap();
        assert_eq!(set["wants"], json!(["photos"]));
        let e = call_for_agent(&k, Op::BackupWants, Some("Beach"), json!({ "wants": [] })).unwrap_err();
        assert_eq!(e.kind, crate::Kind::NotUnderstood, "at least one");
        assert!(k
            .log_tail(10)
            .contains("This backup is now for photos only. Nothing already backed up was changed"));
    }
}
