//! Photos and videos, photos only, or videos only: what a backup, or one
//! of its sources, wants.
//!
//! Three choices decide whether a file that was found is offered, and they
//! are made at three levels, which compose in one order:
//!
//! 1. The kinds of file Keevoro looks for at all (`kinds`), for the whole
//!    of Keevoro. A file of a kind not looked for is never found, so it
//!    never reaches the other two.
//! 2. Photos or videos: the source's own choice when it has one, otherwise
//!    the backup's. A source can differ from its backup either way.
//! 3. Each ending on a source, included or left out (`.png`, say): the
//!    fine control.
//!
//! [`offered`] answers for one file and, when it is not offered, says at
//! which level it was left out, so a screen can say why something is
//! missing. It is a filter over what was found: changing a choice takes
//! effect at once, with no new search, and nothing already backed up is
//! touched.

use crate::media::MediaKind;
use serde::{Deserialize, Serialize};

/// What a backup or a source wants.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Wants {
    /// Photos and videos.
    #[default]
    Both,
    Photos,
    Videos,
}

impl Wants {
    /// Does it want a file of this kind?
    pub fn takes(self, kind: MediaKind) -> bool {
        match self {
            Wants::Both => true,
            Wants::Photos => kind == MediaKind::Photo,
            Wants::Videos => kind == MediaKind::Video,
        }
    }

    /// In a few words: "photos and videos", "photos only", "videos only".
    pub fn words(self) -> &'static str {
        match self {
            Wants::Both => "photos and videos",
            Wants::Photos => "photos only",
            Wants::Videos => "videos only",
        }
    }

    /// The one kind it wants, if only one.
    pub fn only(self) -> Option<MediaKind> {
        match self {
            Wants::Both => None,
            Wants::Photos => Some(MediaKind::Photo),
            Wants::Videos => Some(MediaKind::Video),
        }
    }
}

/// The level a file was left out at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LeftOutBy {
    /// The backup wants only the other kind.
    Backup,
    /// This source wants only the other kind.
    Source,
    /// Files with this ending are left out of this source.
    Ending,
}

impl LeftOutBy {
    /// Why, in words for a person, for a file of `kind`.
    pub fn why(self, kind: MediaKind) -> &'static str {
        match (self, kind) {
            (LeftOutBy::Backup, MediaKind::Photo) => "this backup is for videos only",
            (LeftOutBy::Backup, MediaKind::Video) => "this backup is for photos only",
            (LeftOutBy::Source, MediaKind::Photo) => "this source is set to videos only",
            (LeftOutBy::Source, MediaKind::Video) => "this source is set to photos only",
            (LeftOutBy::Ending, _) => "files with this ending are left out of this source",
        }
    }
}

/// Is a found file of `kind` offered? `source` is the source's own choice,
/// `None` for "the same as the backup"; `ending_included` is the choice
/// for its ending on that source. When it is not offered, says at which
/// level it was left out: the source's or backup's choice is asked first,
/// then the ending.
pub fn offered(kind: MediaKind, backup: Wants, source: Option<Wants>, ending_included: bool) -> Result<(), LeftOutBy> {
    match source {
        Some(own) if !own.takes(kind) => return Err(LeftOutBy::Source),
        None if !backup.takes(kind) => return Err(LeftOutBy::Backup),
        _ => {}
    }
    if ending_included {
        Ok(())
    } else {
        Err(LeftOutBy::Ending)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use MediaKind::{Photo, Video};

    #[test]
    fn everything_is_offered_until_someone_chooses() {
        assert_eq!(offered(Photo, Wants::Both, None, true), Ok(()));
        assert_eq!(offered(Video, Wants::default(), None, true), Ok(()));
    }

    #[test]
    fn a_backup_for_photos_only_leaves_videos_out_and_says_so() {
        assert_eq!(offered(Photo, Wants::Photos, None, true), Ok(()));
        assert_eq!(offered(Video, Wants::Photos, None, true), Err(LeftOutBy::Backup));
        assert_eq!(offered(Photo, Wants::Videos, None, true), Err(LeftOutBy::Backup));
        assert_eq!(LeftOutBy::Backup.why(Video), "this backup is for photos only");
    }

    #[test]
    fn a_source_can_differ_from_its_backup_either_way() {
        // The backup wants photos only, this source everything.
        assert_eq!(offered(Video, Wants::Photos, Some(Wants::Both), true), Ok(()));
        // The backup wants everything, this source videos only.
        assert_eq!(
            offered(Photo, Wants::Both, Some(Wants::Videos), true),
            Err(LeftOutBy::Source)
        );
        assert_eq!(LeftOutBy::Source.why(Photo), "this source is set to videos only");
    }

    #[test]
    fn the_ending_is_the_fine_control_asked_last() {
        assert_eq!(offered(Photo, Wants::Both, None, false), Err(LeftOutBy::Ending));
        // Left out by the backup's choice and its ending: the broader level is said.
        assert_eq!(offered(Video, Wants::Photos, None, false), Err(LeftOutBy::Backup));
        assert_eq!(
            offered(Video, Wants::Photos, Some(Wants::Videos), false),
            Err(LeftOutBy::Ending)
        );
    }

    #[test]
    fn choices_are_written_as_plain_words() {
        assert_eq!(serde_json::to_string(&Wants::Photos).unwrap(), "\"photos\"");
        assert_eq!(Wants::Videos.words(), "videos only");
        assert_eq!(Wants::Photos.only(), Some(Photo));
        assert_eq!(Wants::Both.only(), None);
    }
}
