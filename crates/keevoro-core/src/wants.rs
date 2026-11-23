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
//! 4. What it is for (photos, videos, music, other files): the source's own
//!    choice when it has one, otherwise the backup's. A source can differ
//!    from its backup either way.
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

/// What a backup or a source is for: any of photos, videos, music and
/// other files, at least one.
///
/// Photos and videos, and other files once a person adds a kind of them,
/// unless the person chooses: so a backup for "photos and videos" takes
/// other files too as soon as a person adds a kind of other file, and
/// nothing changes for anyone who never does. Music is only ever taken
/// when a person asks for it, so every backup made before Keevoro knew
/// music, whose choice has no word for it, is for no music.
///
/// Written as the list of what it is for: `["photos", "videos"]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Wants {
    pub photos: bool,
    pub videos: bool,
    pub music: bool,
    pub others: bool,
}

impl Default for Wants {
    fn default() -> Self {
        Self {
            photos: true,
            videos: true,
            music: false,
            others: true,
        }
    }
}

/// The words a [`Wants`] is written in, in order.
const WORDS: [(&str, MediaKind); 4] = [
    ("photos", MediaKind::Photo),
    ("videos", MediaKind::Video),
    ("music", MediaKind::Music),
    ("others", MediaKind::Other),
];

impl Wants {
    /// For one kind only.
    pub fn only(kind: MediaKind) -> Self {
        Self {
            photos: kind == MediaKind::Photo,
            videos: kind == MediaKind::Video,
            music: kind == MediaKind::Music,
            others: kind == MediaKind::Other,
        }
    }

    /// For music alone: the backup of just someone's music.
    pub fn music_only(self) -> bool {
        self.music && !self.photos && !self.videos
    }

    /// Does it want a file of this kind?
    pub fn takes(self, kind: MediaKind) -> bool {
        match kind {
            MediaKind::Photo => self.photos,
            MediaKind::Video => self.videos,
            MediaKind::Music => self.music,
            MediaKind::Other => self.others,
        }
    }

    /// True when it is for nothing at all, which is never allowed.
    pub fn is_nothing(self) -> bool {
        !(self.photos || self.videos || self.music || self.others)
    }

    /// True when it takes everything `before` did, and maybe more: nothing
    /// was switched off.
    pub fn takes_all_of(self, before: Wants) -> bool {
        WORDS.iter().all(|(_, kind)| !before.takes(*kind) || self.takes(*kind))
    }

    /// True when it takes every photo and video, and every other file
    /// a person can see a choice of: other files only count when some kind
    /// of other file is on the list. Music is a choice of its own, so a
    /// backup without it is not said to be for "photos and videos only".
    pub fn is_everything(self, others_listed: bool) -> bool {
        self.photos && self.videos && (self.others || !others_listed)
    }

    /// Its words: `photos`, `videos`, `music`, `others`.
    pub fn tokens(self) -> Vec<&'static str> {
        WORDS
            .iter()
            .filter(|(_, kind)| self.takes(*kind))
            .map(|(word, _)| *word)
            .collect()
    }

    /// Read from its words, in any order and any case. `None` for a word
    /// that is not one of them, or for none at all.
    pub fn from_tokens<'a>(words: impl IntoIterator<Item = &'a str>) -> Option<Self> {
        let mut wants = Self {
            photos: false,
            videos: false,
            music: false,
            others: false,
        };
        for word in words {
            match word.trim().to_lowercase().as_str() {
                "photos" | "photo" => wants.photos = true,
                "videos" | "video" => wants.videos = true,
                "music" | "songs" | "tracks" => wants.music = true,
                "others" | "other" => wants.others = true,
                _ => return None,
            }
        }
        (!wants.is_nothing()).then_some(wants)
    }

    /// In a few words: "photos and videos", "photos only", "music only",
    /// "photos, videos and music", "photos and other files only".
    /// Other files are only spoken of when some kind of other file is on
    /// the list (`others_listed`), so a person who never adds one hears
    /// exactly what they always did.
    pub fn words(self, others_listed: bool) -> String {
        let named: Vec<&str> = [
            (self.photos, "photos"),
            (self.videos, "videos"),
            (self.music, "music"),
            (self.others && others_listed, "other files"),
        ]
        .into_iter()
        .filter_map(|(on, word)| on.then_some(word))
        .collect();
        let joined = match named.as_slice() {
            [] => "other files".to_string(),
            [one] => (*one).to_string(),
            [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
        };
        if self.is_everything(others_listed) {
            joined
        } else {
            format!("{joined} only")
        }
    }
}

impl Serialize for Wants {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.tokens().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Wants {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let words = Vec::<String>::deserialize(deserializer)?;
        Wants::from_tokens(words.iter().map(String::as_str)).ok_or_else(|| {
            serde::de::Error::custom("say what it is for: one or more of photos, videos, music and others")
        })
    }
}

/// The choice that left a file out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeftOutBy {
    /// It is in a folder Keevoro made.
    MadeByKeevoro,
    /// It is in a folder the person left out of this backup.
    Folder,
    /// Keevoro does not look for this kind of file at all.
    Kinds,
    /// The backup is not for this kind: what it is for.
    Backup(Wants),
    /// This source is not for this kind: what it is for.
    Source(Wants),
    /// Files with this ending are left out of this source.
    Ending,
}

impl LeftOutBy {
    /// Why, in words for a person. Other files are only spoken of when
    /// some kind of other file is on the list (`others_listed`).
    pub fn why(self, others_listed: bool) -> String {
        match self {
            LeftOutBy::MadeByKeevoro => "Keevoro made this folder, so it is not a source".into(),
            LeftOutBy::Folder => "you left this folder out of this backup".into(),
            LeftOutBy::Kinds => {
                "Keevoro doesn\u{2019}t look for this kind of file (Settings, Advanced, Kinds of file)".into()
            }
            LeftOutBy::Backup(wants) => format!("this backup is for {}", wants.words(others_listed)),
            LeftOutBy::Source(wants) => format!("this source is set to {}", wants.words(others_listed)),
            LeftOutBy::Ending => "files with this ending are left out of this source".into(),
        }
    }

    /// Its name for a program: `madeByKeevoro`, `folder`, `kinds`,
    /// `backup`, `source` or `ending`.
    pub fn name(self) -> &'static str {
        match self {
            LeftOutBy::MadeByKeevoro => "madeByKeevoro",
            LeftOutBy::Folder => "folder",
            LeftOutBy::Kinds => "kinds",
            LeftOutBy::Backup(_) => "backup",
            LeftOutBy::Source(_) => "source",
            LeftOutBy::Ending => "ending",
        }
    }
}

impl Serialize for LeftOutBy {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.name())
    }
}

/// One source's own choices.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SourceChoices {
    /// Where it is, as the computer gives its full path.
    pub root: PathBuf,
    /// Its own choice of what it is for; `None` to follow the backup.
    pub wants: Option<Wants>,
    /// Endings left out, lower case without the dot.
    pub endings_left_out: BTreeSet<String>,
    /// Its own way of arranging its photos and videos; `None` to follow
    /// the backup.
    pub arrange: Option<crate::arrange::Arrange>,
}

/// Every choice a person made for one backup that decides what is offered.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Choices {
    pub backup: Wants,
    pub sources: Vec<SourceChoices>,
    /// Folders left out, as the computer gives their full paths.
    pub left_out_folders: Vec<PathBuf>,
    /// How the backup arranges photos and videos, unless a source says
    /// otherwise.
    pub arrange: crate::arrange::Arrange,
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

    /// How the photos and videos found by looking through the source at
    /// `root` are arranged: its own choice, or else the backup's.
    pub fn arrangement_in(&self, root: &Path) -> crate::arrange::Arrange {
        self.sources
            .iter()
            .find(|s| crate::places::same_place(&s.root, root))
            .and_then(|s| s.arrange)
            .unwrap_or(self.arrange)
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
            Some(own) if !own.takes(file.kind) => return Err(LeftOutBy::Source(own)),
            None if !self.backup.takes(file.kind) => return Err(LeftOutBy::Backup(self.backup)),
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
    use MediaKind::{Music, Other, Photo, Video};

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
            arrange: None,
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
            backup: Wants::only(Photo),
            sources: vec![card(None, &[])],
            ..Choices::default()
        };
        assert_eq!(choices.offered(&found("/card/a.jpg", Photo)), Ok(()));
        assert_eq!(
            choices.offered(&found("/card/a.mp4", Video)),
            Err(LeftOutBy::Backup(Wants::only(Photo)))
        );
        assert_eq!(
            LeftOutBy::Backup(Wants::only(Photo)).why(false),
            "this backup is for photos only"
        );
    }

    #[test]
    fn a_source_can_differ_from_its_backup_either_way() {
        let more = Choices {
            backup: Wants::only(Photo),
            sources: vec![card(Some(Wants::default()), &[])],
            ..Choices::default()
        };
        assert_eq!(more.offered(&found("/card/a.mp4", Video)), Ok(()));
        let fewer = Choices {
            sources: vec![card(Some(Wants::only(Video)), &[])],
            ..Choices::default()
        };
        assert_eq!(
            fewer.offered(&found("/card/a.jpg", Photo)),
            Err(LeftOutBy::Source(Wants::only(Video)))
        );
        assert_eq!(
            LeftOutBy::Source(Wants::only(Video)).why(false),
            "this source is set to videos only"
        );
        // Outside the source, its choice does not apply.
        assert_eq!(fewer.offered(&found("/elsewhere/a.jpg", Photo)), Ok(()));
    }

    #[test]
    fn the_ending_is_the_fine_control_asked_last() {
        let choices = Choices {
            backup: Wants::only(Photo),
            sources: vec![card(None, &["png", "mp4"])],
            ..Choices::default()
        };
        assert_eq!(choices.offered(&found("/card/a.PNG", Photo)), Err(LeftOutBy::Ending));
        assert_eq!(choices.offered(&found("/card/a.jpg", Photo)), Ok(()));
        // Left out by the backup's choice and its ending: the broader is said.
        assert_eq!(
            choices.offered(&found("/card/a.mp4", Video)),
            Err(LeftOutBy::Backup(Wants::only(Photo)))
        );
    }

    #[test]
    fn a_folder_left_out_takes_everything_beneath_it_and_nothing_beside_it() {
        let choices = Choices {
            backup: Wants::only(Photo),
            sources: vec![card(None, &["png"])],
            left_out_folders: vec![PathBuf::from("/card/Games")],
            ..Choices::default()
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
        assert_eq!(LeftOutBy::Folder.why(false), "you left this folder out of this backup");
    }

    #[test]
    fn each_choice_is_said_in_order_broadest_first() {
        let choices = Choices {
            backup: Wants::only(Video),
            sources: vec![card(None, &["jpg"])],
            left_out_folders: vec![PathBuf::from("/card/Out")],
            ..Choices::default()
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
        assert_eq!(choices.offered(&file), Err(LeftOutBy::Backup(Wants::only(Video))));
        assert!(LeftOutBy::Kinds
            .why(false)
            .contains("Settings, Advanced, Kinds of file"));
    }

    #[test]
    fn the_deepest_source_holding_a_file_is_its_source() {
        let choices = Choices {
            sources: vec![
                card(Some(Wants::only(Photo)), &[]),
                SourceChoices {
                    root: PathBuf::from("/card/Videos"),
                    wants: Some(Wants::only(Video)),
                    endings_left_out: BTreeSet::new(),
                    arrange: None,
                },
            ],
            ..Choices::default()
        };
        assert_eq!(choices.offered(&found("/card/Videos/a.mp4", Video)), Ok(()));
        assert_eq!(
            choices.offered(&found("/card/a.mp4", Video)),
            Err(LeftOutBy::Source(Wants::only(Photo)))
        );
    }

    #[test]
    fn choices_are_written_as_plain_words() {
        assert_eq!(serde_json::to_string(&Wants::only(Photo)).unwrap(), r#"["photos"]"#);
        assert_eq!(
            serde_json::to_string(&Wants::default()).unwrap(),
            r#"["photos","videos","others"]"#
        );
        let read: Wants = serde_json::from_str(r#"["Videos","others"]"#).unwrap();
        assert_eq!(
            read,
            Wants {
                photos: false,
                ..Wants::default()
            }
        );
        // Written before Keevoro knew music: no word for it, so no music.
        let before: Wants = serde_json::from_str(r#"["photos","videos","others"]"#).unwrap();
        assert!(!before.music && before == Wants::default());
        assert!(serde_json::from_str::<Wants>("[]").is_err(), "at least one");
        assert!(serde_json::from_str::<Wants>(r#"["songbooks"]"#).is_err());
        let music: Wants = serde_json::from_str(r#"["music"]"#).unwrap();
        assert_eq!(music, Wants::only(MediaKind::Music));
        assert!(music.music_only());
        assert_eq!(serde_json::to_string(&music).unwrap(), r#"["music"]"#);
        assert!(serde_json::from_str::<Wants>(r#""both""#).is_err());
        assert_eq!(
            serde_json::to_string(&LeftOutBy::Backup(Wants::default())).unwrap(),
            r#""backup""#
        );
    }

    #[test]
    fn with_no_kind_of_other_file_listed_the_words_are_what_they_always_were() {
        let photos_only = Wants {
            videos: false,
            ..Wants::default()
        };
        let videos_only = Wants {
            photos: false,
            ..Wants::default()
        };
        for (wants, said) in [
            (Wants::default(), "photos and videos"),
            (photos_only, "photos only"),
            (videos_only, "videos only"),
            (Wants::only(Photo), "photos only"),
        ] {
            assert_eq!(wants.words(false), said);
        }
        assert!(Wants::default().is_everything(false));
        assert!(Wants {
            others: false,
            ..Wants::default()
        }
        .is_everything(false));
    }

    #[test]
    fn with_other_files_listed_they_are_named_too() {
        for (wants, said) in [
            (Wants::default(), "photos, videos and other files"),
            (
                Wants {
                    others: false,
                    ..Wants::default()
                },
                "photos and videos only",
            ),
            (
                Wants {
                    videos: false,
                    ..Wants::default()
                },
                "photos and other files only",
            ),
            (Wants::only(Other), "other files only"),
            (Wants::only(Video), "videos only"),
        ] {
            assert_eq!(wants.words(true), said);
        }
        assert_eq!(
            LeftOutBy::Backup(Wants::only(Photo)).why(true),
            "this backup is for photos only"
        );
        let mine = Choices {
            backup: Wants::only(Other),
            ..Choices::default()
        };
        assert_eq!(mine.offered(&found("/a/song.flac", Other)), Ok(()));
        assert!(mine.offered(&found("/a/b.jpg", Photo)).is_err());
    }

    #[test]
    fn music_is_a_choice_of_its_own_said_by_name() {
        let photos_and_music = Wants {
            videos: false,
            music: true,
            ..Wants::default()
        };
        let everything = Wants {
            music: true,
            ..Wants::default()
        };
        for (wants, said) in [
            (Wants::only(Music), "music only"),
            (photos_and_music, "photos and music only"),
            (everything, "photos, videos and music"),
            (Wants::default(), "photos and videos"),
        ] {
            assert_eq!(wants.words(false), said);
        }
        assert!(!Wants::default().takes(Music), "never taken unless asked for");
        let mine = Choices {
            backup: Wants::only(Music),
            ..Choices::default()
        };
        assert_eq!(mine.offered(&found("/a/Artist/Album/01.flac", Music)), Ok(()));
        assert_eq!(
            mine.offered(&found("/a/b.jpg", Photo)),
            Err(LeftOutBy::Backup(Wants::only(Music)))
        );
        assert_eq!(
            Choices::default().offered(&found("/a/01.mp3", Music)),
            Err(LeftOutBy::Backup(Wants::default()))
        );
        assert!(everything.takes_all_of(Wants::default()));
        assert!(!Wants::default().takes_all_of(everything));
        assert!(Wants::from_tokens(["Music"]).is_some_and(Wants::music_only));
    }
}
