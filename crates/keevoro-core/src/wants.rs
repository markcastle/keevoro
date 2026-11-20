//! What is offered for a backup, and when something is not, which choice
//! left it out.
//!
//! Five choices decide whether a file is offered, made at different
//! levels, and [`Choices::offered`] composes them in one order, broadest
//! first, so a screen says the one that matters:
//!
//! 1. A folder Keevoro made (a backup, its own working files, an old
//!    library) is never a source.
//! 2. A folder the person left out of this backup, with everything
//!    beneath it.
//! 3. The kinds of file Keevoro looks for at all, for the whole of
//!    Keevoro. A kind switched off there is not found, so a file of it is
//!    only ever seen when looking inside a folder.
//! 4. Photos or videos: the source's own choice when it has one, otherwise
//!    the backup's. A source can differ from its backup either way.
//! 5. Each ending on a source, included or left out (`.png`, say): the
//!    fine control.
//!
//! It is a filter over what was found: changing a choice takes effect at
//! once, with no new search, and nothing already backed up is touched.
//! Folders are compared as real paths, by their parts, ignoring case on
//! Windows only (`crate::places`).

use crate::media::MediaKind;
use crate::places::within;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

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

/// The choice that left a file out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LeftOutBy {
    /// It is in a folder Keevoro made.
    MadeByKeevoro,
    /// It is in a folder the person left out of this backup.
    Folder,
    /// Keevoro does not look for this kind of file at all.
    Kinds,
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
            (LeftOutBy::MadeByKeevoro, _) => "Keevoro made this folder, so it is not a source",
            (LeftOutBy::Folder, _) => "you left this folder out of this backup",
            (LeftOutBy::Kinds, _) => "Keevoro doesn\u{2019}t look for this kind of file (Settings, Advanced)",
            (LeftOutBy::Backup, MediaKind::Photo) => "this backup is for videos only",
            (LeftOutBy::Backup, MediaKind::Video) => "this backup is for photos only",
            (LeftOutBy::Source, MediaKind::Photo) => "this source is set to videos only",
            (LeftOutBy::Source, MediaKind::Video) => "this source is set to photos only",
            (LeftOutBy::Ending, _) => "files with this ending are left out of this source",
        }
    }
}

/// One source's own choices.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SourceChoices {
    /// Where it is, as the computer gives its full path.
    pub root: PathBuf,
    /// Its own choice of photos or videos; `None` to follow the backup.
    pub wants: Option<Wants>,
    /// Endings left out, lower case without the dot.
    pub endings_left_out: BTreeSet<String>,
}

/// Every choice a person made for one backup that decides what is offered.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Choices {
    pub backup: Wants,
    pub sources: Vec<SourceChoices>,
    /// Folders left out, as the computer gives their full paths.
    pub left_out_folders: Vec<PathBuf>,
}

/// What is known about one file that the choices are asked about.
#[derive(Debug, Clone, Copy)]
pub struct FileFacts<'a> {
    /// Where it is, as the computer gives its full path.
    pub path: &'a Path,
    pub kind: MediaKind,
    /// False when its kind is switched off in the kinds Keevoro looks for.
    pub looked_for: bool,
    /// True when it is in a folder Keevoro made.
    pub made_by_keevoro: bool,
}

impl Choices {
    /// The source a file belongs to: the deepest one it is in.
    pub fn source_of(&self, path: &Path) -> Option<&SourceChoices> {
        self.sources
            .iter()
            .filter(|s| within(path, &s.root))
            .max_by_key(|s| s.root.components().count())
    }

    /// The folder left out that `path` is in, if any.
    pub fn left_out_folder(&self, path: &Path) -> Option<&Path> {
        self.left_out_folders
            .iter()
            .find(|folder| within(path, folder))
            .map(PathBuf::as_path)
    }

    /// Is this file offered? When not, the broadest choice that left it
    /// out, in the order the module explains. Its source is the deepest
    /// one it is in.
    pub fn offered(&self, file: &FileFacts<'_>) -> Result<(), LeftOutBy> {
        self.judge(file, self.source_of(file.path))
    }

    /// As [`Choices::offered`], for a file found by looking through the
    /// source at `root`, whose choices apply to it.
    pub fn offered_in(&self, file: &FileFacts<'_>, root: &Path) -> Result<(), LeftOutBy> {
        self.judge(
            file,
            self.sources.iter().find(|s| crate::places::same_place(&s.root, root)),
        )
    }

    fn judge(&self, file: &FileFacts<'_>, source: Option<&SourceChoices>) -> Result<(), LeftOutBy> {
        if file.made_by_keevoro {
            return Err(LeftOutBy::MadeByKeevoro);
        }
        if self.left_out_folder(file.path).is_some() {
            return Err(LeftOutBy::Folder);
        }
        if !file.looked_for {
            return Err(LeftOutBy::Kinds);
        }
        match source.and_then(|s| s.wants) {
            Some(own) if !own.takes(file.kind) => return Err(LeftOutBy::Source),
            None if !self.backup.takes(file.kind) => return Err(LeftOutBy::Backup),
            _ => {}
        }
        let ending = file
            .path
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        if source.is_some_and(|s| s.endings_left_out.contains(&ending)) {
            return Err(LeftOutBy::Ending);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use MediaKind::{Photo, Video};

    fn found(path: &str, kind: MediaKind) -> FileFacts<'_> {
        FileFacts {
            path: Path::new(path),
            kind,
            looked_for: true,
            made_by_keevoro: false,
        }
    }

    fn card(wants: Option<Wants>, endings: &[&str]) -> SourceChoices {
        SourceChoices {
            root: PathBuf::from("/card"),
            wants,
            endings_left_out: endings.iter().map(|e| (*e).to_string()).collect(),
        }
    }

    #[test]
    fn everything_is_offered_until_someone_chooses() {
        let choices = Choices::default();
        assert_eq!(choices.offered(&found("/card/a.jpg", Photo)), Ok(()));
        assert_eq!(choices.offered(&found("/card/a.mp4", Video)), Ok(()));
    }

    #[test]
    fn a_backup_for_photos_only_leaves_videos_out_and_says_so() {
        let choices = Choices {
            backup: Wants::Photos,
            sources: vec![card(None, &[])],
            ..Choices::default()
        };
        assert_eq!(choices.offered(&found("/card/a.jpg", Photo)), Ok(()));
        assert_eq!(choices.offered(&found("/card/a.mp4", Video)), Err(LeftOutBy::Backup));
        assert_eq!(LeftOutBy::Backup.why(Video), "this backup is for photos only");
    }

    #[test]
    fn a_source_can_differ_from_its_backup_either_way() {
        let more = Choices {
            backup: Wants::Photos,
            sources: vec![card(Some(Wants::Both), &[])],
            ..Choices::default()
        };
        assert_eq!(more.offered(&found("/card/a.mp4", Video)), Ok(()));
        let fewer = Choices {
            sources: vec![card(Some(Wants::Videos), &[])],
            ..Choices::default()
        };
        assert_eq!(fewer.offered(&found("/card/a.jpg", Photo)), Err(LeftOutBy::Source));
        assert_eq!(LeftOutBy::Source.why(Photo), "this source is set to videos only");
        // Outside the source, its choice does not apply.
        assert_eq!(fewer.offered(&found("/elsewhere/a.jpg", Photo)), Ok(()));
    }

    #[test]
    fn the_ending_is_the_fine_control_asked_last() {
        let choices = Choices {
            backup: Wants::Photos,
            sources: vec![card(None, &["png", "mp4"])],
            ..Choices::default()
        };
        assert_eq!(choices.offered(&found("/card/a.PNG", Photo)), Err(LeftOutBy::Ending));
        assert_eq!(choices.offered(&found("/card/a.jpg", Photo)), Ok(()));
        // Left out by the backup's choice and its ending: the broader is said.
        assert_eq!(choices.offered(&found("/card/a.mp4", Video)), Err(LeftOutBy::Backup));
    }

    #[test]
    fn a_folder_left_out_takes_everything_beneath_it_and_nothing_beside_it() {
        let choices = Choices {
            backup: Wants::Photos,
            sources: vec![card(None, &["png"])],
            left_out_folders: vec![PathBuf::from("/card/Games")],
        };
        assert_eq!(
            choices.offered(&found("/card/Games/a.jpg", Photo)),
            Err(LeftOutBy::Folder)
        );
        assert_eq!(
            choices.offered(&found("/card/Games/deep/a.png", Photo)),
            Err(LeftOutBy::Folder)
        );
        assert_eq!(
            choices.offered(&found("/card/Games 2/a.jpg", Photo)),
            Ok(()),
            "not a sibling"
        );
        assert_eq!(
            choices.left_out_folder(Path::new("/card/Games/x")),
            Some(Path::new("/card/Games"))
        );
        assert_eq!(LeftOutBy::Folder.why(Photo), "you left this folder out of this backup");
    }

    #[test]
    fn each_choice_is_said_in_order_broadest_first() {
        let choices = Choices {
            backup: Wants::Videos,
            sources: vec![card(None, &["jpg"])],
            left_out_folders: vec![PathBuf::from("/card/Out")],
        };
        let all_at_once = |path| FileFacts {
            path: Path::new(path),
            kind: Photo,
            looked_for: false,
            made_by_keevoro: true,
        };
        assert_eq!(
            choices.offered(&all_at_once("/card/Out/a.jpg")),
            Err(LeftOutBy::MadeByKeevoro)
        );
        let mut file = all_at_once("/card/Out/a.jpg");
        file.made_by_keevoro = false;
        assert_eq!(choices.offered(&file), Err(LeftOutBy::Folder));
        let mut file = all_at_once("/card/In/a.jpg");
        file.made_by_keevoro = false;
        assert_eq!(choices.offered(&file), Err(LeftOutBy::Kinds));
        file.looked_for = true;
        assert_eq!(choices.offered(&file), Err(LeftOutBy::Backup));
        assert!(LeftOutBy::Kinds.why(Photo).contains("Settings, Advanced"));
    }

    #[test]
    fn the_deepest_source_holding_a_file_is_its_source() {
        let choices = Choices {
            sources: vec![
                card(Some(Wants::Photos), &[]),
                SourceChoices {
                    root: PathBuf::from("/card/Videos"),
                    wants: Some(Wants::Videos),
                    endings_left_out: BTreeSet::new(),
                },
            ],
            ..Choices::default()
        };
        assert_eq!(choices.offered(&found("/card/Videos/a.mp4", Video)), Ok(()));
        assert_eq!(choices.offered(&found("/card/a.mp4", Video)), Err(LeftOutBy::Source));
    }

    #[test]
    fn choices_are_written_as_plain_words() {
        assert_eq!(serde_json::to_string(&Wants::Photos).unwrap(), "\"photos\"");
        assert_eq!(Wants::Videos.words(), "videos only");
        assert_eq!(Wants::Photos.only(), Some(Photo));
        assert_eq!(Wants::Both.only(), None);
    }
}
