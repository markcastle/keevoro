//! `backup.photos_or_videos` and `sources.photos_or_videos`: whether a
//! backup, or one of its sources, wants photos and videos, photos only, or
//! videos only.
//!
//! It is a choice about what is offered and backed up from now on, made
//! over what was already found (see `keevoro_core::wants` for how it
//! composes with the kinds of file Keevoro looks for and each ending's own
//! choice). So it counts at once, with no new search, and every count,
//! the plan of a backup and the room it needs follow it. Nothing already
//! backed up is touched: photos or videos sent before stay where they
//! are, and are simply no longer kept in the selection.

use crate::backup::Backup;
use crate::error::{Error, Result};
use keevoro_core::Wants;
use serde::{Deserialize, Serialize};

/// Photos and videos, photos only, or videos only, as a request says it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum WantsChoice {
    /// Photos and videos.
    Both,
    Photos,
    Videos,
}

impl From<WantsChoice> for Wants {
    fn from(choice: WantsChoice) -> Self {
        match choice {
            WantsChoice::Both => Wants::Both,
            WantsChoice::Photos => Wants::Photos,
            WantsChoice::Videos => Wants::Videos,
        }
    }
}

/// A source's choice: the backup's, or its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum SourceWantsChoice {
    /// The same as the backup.
    Backup,
    Both,
    Photos,
    Videos,
}

impl From<SourceWantsChoice> for Option<Wants> {
    fn from(choice: SourceWantsChoice) -> Self {
        match choice {
            SourceWantsChoice::Backup => None,
            SourceWantsChoice::Both => Some(Wants::Both),
            SourceWantsChoice::Photos => Some(Wants::Photos),
            SourceWantsChoice::Videos => Some(Wants::Videos),
        }
    }
}

/// `backup.photos_or_videos`: without `wants` it only reads.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BackupWantsRequest {
    /// `both` (photos and videos), `photos` or `videos`. Left out, nothing changes.
    #[serde(default)]
    pub wants: Option<WantsChoice>,
}

/// `sources.photos_or_videos`: without `wants` it only reads.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SourceWantsRequest {
    /// The source's id, as `sources.list` gives it.
    pub source: i64,
    /// `backup` (the same as the backup), `both`, `photos` or `videos`.
    /// Left out, nothing changes.
    #[serde(default)]
    pub wants: Option<SourceWantsChoice>,
}

/// What a backup or a source wants, and what that means now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WantsView {
    /// `both`, `photos` or `videos`: what is in effect.
    pub wants: Wants,
    /// For a source: true when it has its own choice, false when it
    /// follows the backup's.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub own: Option<bool>,
    /// In words: "This backup is for photos only."
    pub said: String,
}

/// What is said when the choice changes, so nobody fears for what is
/// already safe.
pub const ALREADY_SAFE: &str =
    "Anything already backed up stays backed up; it is just no longer kept in what this backup chooses.";

impl Backup {
    /// `backup.photos_or_videos`, read.
    pub fn wants(&self) -> Result<WantsView> {
        let wants = self.catalog().wants()?;
        Ok(WantsView {
            wants,
            own: None,
            said: format!("This backup is for {}.", wants.words()),
        })
    }

    /// `backup.photos_or_videos`, set.
    pub fn set_wants(&self, wants: Wants) -> Result<WantsView> {
        self.catalog().set_wants(wants)?;
        let mut view = self.wants()?;
        if wants != Wants::Both {
            view.said = format!("{} {ALREADY_SAFE}", view.said);
        }
        Ok(view)
    }

    /// `sources.photos_or_videos`, read.
    pub fn source_wants(&self, source: i64) -> Result<WantsView> {
        if !self.sources()?.iter().any(|s| s.id == source) {
            return Err(Error::not_found("That source isn't part of this backup."));
        }
        let own = self.catalog().source_wants(source)?;
        let wants = own.unwrap_or(self.catalog().wants()?);
        Ok(WantsView {
            wants,
            own: Some(own.is_some()),
            said: match own {
                Some(own) => format!("This source is set to {}.", own.words()),
                None => format!("This source follows the backup: {}.", wants.words()),
            },
        })
    }

    /// `sources.photos_or_videos`, set: its own choice, or with `None` the
    /// backup's.
    pub fn set_source_wants(&self, source: i64, wants: Option<Wants>) -> Result<WantsView> {
        if !self.catalog().set_source_wants(source, wants)? {
            return Err(Error::not_found("That source isn't part of this backup."));
        }
        let mut view = self.source_wants(source)?;
        if view.wants != Wants::Both {
            view.said = format!("{} {ALREADY_SAFE}", view.said);
        }
        Ok(view)
    }
}

#[cfg(test)]
mod tests {
    use crate::backup::tests::one_backup;
    use keevoro_core::Wants;

    #[test]
    fn photos_only_counts_at_once_everywhere_and_a_source_can_differ() {
        let (_dir, _card, backup) = one_backup();
        let before = backup.found().unwrap();
        assert!(before.photos > 0);
        assert_eq!(backup.wants().unwrap().said, "This backup is for photos and videos.");

        let set = backup.set_wants(Wants::Videos).unwrap();
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
            backup.source_wants(source).unwrap().said,
            "This source follows the backup: videos only."
        );
        let own = backup.set_source_wants(source, Some(Wants::Both)).unwrap();
        assert_eq!(own.own, Some(true));
        assert_eq!(backup.found().unwrap().photos, before.photos);
        let types = backup.file_types(source).unwrap();
        assert!(types.iter().all(|t| t.left_out_by.is_none()));

        backup.set_source_wants(source, None).unwrap();
        let types = backup.file_types(source).unwrap();
        let jpg = types.iter().find(|t| t.ending == "jpg").unwrap();
        assert_eq!(jpg.why.as_deref(), Some("this backup is for videos only"));
        assert_eq!(backup.source_wants(9999).unwrap_err().kind, crate::Kind::NotFound);
        assert_eq!(
            backup.set_source_wants(9999, None).unwrap_err().kind,
            crate::Kind::NotFound
        );
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
            json!({ "name": "Drone", "wants": "videos" }),
        )
        .unwrap();
        let read = call_for_agent(&k, Op::BackupPhotosOrVideos, Some("Drone"), json!({})).unwrap();
        assert_eq!(read["wants"], "videos");
        let e = call_for_agent(&k, Op::BackupPhotosOrVideos, Some("Drone"), json!({ "wants": "both" })).unwrap_err();
        assert_eq!(e.message, crate::for_agents::LOOK_ONLY);
        k.set_agent_access(crate::AgentsAccessRequest {
            look_in: None,
            back_up_to: None,
            may_change: Some(true),
        })
        .unwrap();
        let set = call_for_agent(
            &k,
            Op::BackupPhotosOrVideos,
            Some("Drone"),
            json!({ "wants": "photos" }),
        )
        .unwrap();
        assert_eq!(set["wants"], "photos");
        assert!(k
            .log_tail(10)
            .contains("This backup is now for photos only. Nothing already backed up was changed"));
    }
}
