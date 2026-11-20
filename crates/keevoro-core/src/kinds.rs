//! The kinds of file Keevoro looks for, by the ending of their names.
//!
//! Keevoro comes with a list of endings it knows as photos and videos (the
//! built-in list). A person can add endings of their own, change whether
//! one is a photo or a video, switch any of them off, remove their own,
//! and put the built-in list back. One list serves the whole of Keevoro,
//! not one backup.
//!
//! Only the person's changes are kept ([`FileKinds`]): the endings they
//! added, and the built-in ones they changed. So the list always starts as
//! the built-in one, and a built-in kind added in a later version simply
//! appears. A search or a survey turns the list into a [`Kinds`] once, at
//! the start, and asks it about each file; nothing here is read from disk.
//!
//! What the list means for a search:
//!
//! - A file whose ending is switched off is passed over without being
//!   opened, even if it is a photo inside.
//! - For the built-in kinds Keevoro can recognise from inside (a JPEG, a
//!   PNG, an MP4), it still looks inside, and goes by what it finds.
//! - For a kind it cannot recognise from inside (most camera raw and
//!   camcorder files, and every kind a person adds) the ending is enough:
//!   the file is found as the photo or video the list says, even when
//!   Keevoro has no way to open it.
//! - Any other file that is not plainly a program, a document or music is
//!   still opened and judged by what is inside, so a photo with an odd
//!   name is not missed.

use crate::media::{never_media_extension, MediaKind};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;

/// One built-in kind: its ending, whether it is a photo or a video, and
/// whether Keevoro can recognise it from what is inside.
struct BuiltIn {
    ending: &'static str,
    kind: MediaKind,
    known_inside: bool,
}

const fn photo(ending: &'static str) -> BuiltIn {
    BuiltIn {
        ending,
        kind: MediaKind::Photo,
        known_inside: true,
    }
}

const fn video(ending: &'static str) -> BuiltIn {
    BuiltIn {
        ending,
        kind: MediaKind::Video,
        known_inside: true,
    }
}

/// A kind known only by its name: there is nothing inside it that tells
/// Keevoro what it is.
const fn by_name(built_in: BuiltIn) -> BuiltIn {
    BuiltIn {
        known_inside: false,
        ..built_in
    }
}

/// The kinds Keevoro knows, in the order they are listed.
const BUILT_IN: &[BuiltIn] = &[
    // Photos
    photo("jpg"),
    photo("jpeg"),
    photo("jpe"),
    photo("png"),
    photo("heic"),
    photo("heif"),
    photo("tif"),
    photo("tiff"),
    photo("webp"),
    photo("bmp"),
    photo("avif"),
    photo("jxl"),
    // Camera raw
    by_name(photo("dng")),
    by_name(photo("cr2")),
    by_name(photo("cr3")),
    by_name(photo("crw")),
    by_name(photo("nef")),
    by_name(photo("nrw")),
    by_name(photo("arw")),
    by_name(photo("srf")),
    by_name(photo("sr2")),
    by_name(photo("orf")),
    by_name(photo("rw2")),
    by_name(photo("raf")),
    by_name(photo("pef")),
    by_name(photo("srw")),
    by_name(photo("x3f")),
    by_name(photo("3fr")),
    by_name(photo("erf")),
    by_name(photo("kdc")),
    by_name(photo("mrw")),
    by_name(photo("raw")),
    by_name(photo("iiq")),
    by_name(photo("dcr")),
    by_name(photo("mef")),
    by_name(photo("mos")),
    by_name(photo("rwl")),
    // Videos
    video("mp4"),
    video("mov"),
    video("m4v"),
    video("avi"),
    video("mkv"),
    video("wmv"),
    video("mpg"),
    video("mpeg"),
    video("webm"),
    video("insv"),
    video("asf"),
    video("flv"),
    // Camcorder and old phone video
    by_name(video("mts")),
    by_name(video("m2ts")),
    by_name(video("3gp")),
    by_name(video("3g2")),
    by_name(video("mod")),
    by_name(video("tod")),
    by_name(video("vob")),
    by_name(video("dv")),
];

fn built_in(ending: &str) -> Option<&'static BuiltIn> {
    BUILT_IN.iter().find(|b| b.ending == ending)
}

/// The longest ending accepted. Real ones are three or four letters.
pub const LONGEST_ENDING: usize = 10;

/// One change a person made to the list: an ending they added, or a
/// built-in one they changed. This is what the settings file keeps.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KindChoice {
    pub ending: String,
    pub kind: MediaKind,
    #[serde(default = "on")]
    pub on: bool,
}

fn on() -> bool {
    true
}

/// The person's changes to the built-in list, in the order made. Empty
/// means the built-in list as it comes.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FileKinds(Vec<KindChoice>);

/// One line of the list as a person sees it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileKind {
    /// Lower case, without the dot: `jpg`.
    pub ending: String,
    pub kind: MediaKind,
    /// Looked for. A kind switched off is passed over.
    pub on: bool,
    /// Part of the built-in list: it can be switched off and changed
    /// between photo and video, but not renamed or removed.
    pub built_in: bool,
}

/// Why a change to the list was refused, in words for the person.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KindRefusal {
    Empty,
    TooLong,
    /// Holds a space, a dot or a character that belongs in a path.
    NotAnEnding(String),
    /// A program, a document, music or an archive: never a memory.
    NeverMedia(String),
    AlreadyListed(String),
    NotListed(String),
    /// A built-in kind can be switched off, never removed or renamed.
    BuiltIn(String),
}

impl fmt::Display for KindRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "Type the ending of the files, such as heif."),
            Self::TooLong => write!(
                f,
                "That is too long for the ending of a file name. Endings are usually three or four letters, such as heif."
            ),
            Self::NotAnEnding(ending) => write!(
                f,
                "\"{ending}\" can't be the ending of a file name. Use letters and numbers only, with no spaces or dots, such as heif."
            ),
            Self::NeverMedia(ending) => write!(
                f,
                "Files ending .{ending} are programs, documents, music or archives, never photos or videos, so Keevoro doesn't look inside them."
            ),
            Self::AlreadyListed(ending) => write!(f, "Keevoro already looks for .{ending} files."),
            Self::NotListed(ending) => write!(f, "Keevoro doesn't have .{ending} files on its list."),
            Self::BuiltIn(ending) => write!(
                f,
                ".{ending} is one of the kinds Keevoro comes with. It can be switched off, but not removed or renamed."
            ),
        }
    }
}

/// An ending as Keevoro compares it: without the leading dot, in lower
/// case, checked to be an ending at all.
pub fn plain_ending(typed: &str) -> Result<String, KindRefusal> {
    let trimmed = typed.trim();
    let ending = trimmed.strip_prefix('.').unwrap_or(trimmed).to_lowercase();
    if ending.is_empty() {
        return Err(KindRefusal::Empty);
    }
    if ending.chars().count() > LONGEST_ENDING {
        return Err(KindRefusal::TooLong);
    }
    // Letters and numbers only: anything else would be a space, a dot (so
    // never the part after the last one), or a character with a meaning
    // in a path.
    if !ending.chars().all(char::is_alphanumeric) {
        return Err(KindRefusal::NotAnEnding(trimmed.to_string()));
    }
    Ok(ending)
}

/// What a change to one line asks for. Anything left out stays as it is.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KindChange {
    pub kind: Option<MediaKind>,
    pub on: Option<bool>,
    /// A new ending, for one the person added.
    pub ending: Option<String>,
}

impl FileKinds {
    /// The built-in list with nothing changed.
    pub fn is_built_in(&self) -> bool {
        self.0.is_empty()
    }

    /// The whole list: the built-in kinds as the person has them, then
    /// the ones they added, in the order added.
    pub fn list(&self) -> Vec<FileKind> {
        let mine = |ending: &str| self.0.iter().find(|c| c.ending == ending);
        let built: Vec<FileKind> = BUILT_IN
            .iter()
            .map(|b| {
                let (kind, on) = mine(b.ending).map_or((b.kind, true), |c| (c.kind, c.on));
                FileKind {
                    ending: b.ending.to_string(),
                    kind,
                    on,
                    built_in: true,
                }
            })
            .collect();
        let added = self
            .0
            .iter()
            .filter(|c| built_in(&c.ending).is_none())
            .map(|c| FileKind {
                ending: c.ending.clone(),
                kind: c.kind,
                on: c.on,
                built_in: false,
            });
        built.into_iter().chain(added).collect()
    }

    fn line(&self, ending: &str) -> Option<FileKind> {
        self.list().into_iter().find(|k| k.ending == ending)
    }

    /// Adds an ending as a photo or a video, switched on.
    pub fn add(&mut self, typed: &str, kind: MediaKind) -> Result<FileKind, KindRefusal> {
        let ending = plain_ending(typed)?;
        if self.line(&ending).is_some() {
            return Err(KindRefusal::AlreadyListed(ending));
        }
        if never_media_extension(&ending) {
            return Err(KindRefusal::NeverMedia(ending));
        }
        self.0.push(KindChoice {
            ending: ending.clone(),
            kind,
            on: true,
        });
        Ok(FileKind {
            ending,
            kind,
            on: true,
            built_in: false,
        })
    }

    /// Changes one line: photo or video, on or off, and, for one the
    /// person added, its ending.
    pub fn change(&mut self, typed: &str, change: &KindChange) -> Result<FileKind, KindRefusal> {
        let ending = plain_ending(typed)?;
        let now = self
            .line(&ending)
            .ok_or_else(|| KindRefusal::NotListed(ending.clone()))?;
        let renamed = match &change.ending {
            Some(to) => Some(plain_ending(to)?).filter(|to| *to != ending),
            None => None,
        };
        if let Some(to) = &renamed {
            if now.built_in {
                return Err(KindRefusal::BuiltIn(ending));
            }
            if self.line(to).is_some() {
                return Err(KindRefusal::AlreadyListed(to.clone()));
            }
            if never_media_extension(to) {
                return Err(KindRefusal::NeverMedia(to.clone()));
            }
        }
        let wanted = KindChoice {
            ending: renamed.unwrap_or_else(|| ending.clone()),
            kind: change.kind.unwrap_or(now.kind),
            on: change.on.unwrap_or(now.on),
        };
        let same_as_it_came = built_in(&wanted.ending).is_some_and(|b| b.kind == wanted.kind && wanted.on);
        match self.0.iter().position(|c| c.ending == ending) {
            // A built-in kind put back as it came needs no note of it.
            Some(at) if same_as_it_came => {
                self.0.remove(at);
            }
            Some(at) => self.0[at] = wanted.clone(),
            None if same_as_it_came => {}
            None => self.0.push(wanted.clone()),
        }
        Ok(FileKind {
            built_in: now.built_in,
            ending: wanted.ending,
            kind: wanted.kind,
            on: wanted.on,
        })
    }

    /// Removes an ending the person added. A built-in one is refused: it
    /// can be switched off instead.
    pub fn remove(&mut self, typed: &str) -> Result<FileKind, KindRefusal> {
        let ending = plain_ending(typed)?;
        let line = self
            .line(&ending)
            .ok_or_else(|| KindRefusal::NotListed(ending.clone()))?;
        if line.built_in {
            return Err(KindRefusal::BuiltIn(ending));
        }
        self.0.retain(|c| c.ending != ending);
        Ok(line)
    }

    /// Puts every built-in kind back as it came: switched on, as the photo
    /// or video it is. The person's own kinds stay as they are.
    pub fn put_back_built_in(&mut self) {
        self.0.retain(|c| built_in(&c.ending).is_none());
    }

    /// The list as a quick lookup, made once for a search or a survey.
    pub fn lookup(&self) -> Kinds {
        let by_ending = self
            .list()
            .into_iter()
            .map(|k| {
                let known_inside = k.built_in && built_in(&k.ending).is_some_and(|b| b.known_inside);
                (
                    k.ending,
                    Looked {
                        kind: k.kind,
                        on: k.on,
                        known_inside,
                    },
                )
            })
            .collect();
        Kinds { by_ending }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Looked {
    kind: MediaKind,
    on: bool,
    known_inside: bool,
}

/// The list of kinds, made into a lookup for a search or a survey. Every
/// question takes an ending as it comes from a file name, with any case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Kinds {
    by_ending: HashMap<String, Looked>,
}

impl Default for Kinds {
    /// The built-in list.
    fn default() -> Self {
        FileKinds::default().lookup()
    }
}

impl Kinds {
    fn get(&self, ending: &str) -> Option<&Looked> {
        let ending = ending.strip_prefix('.').unwrap_or(ending);
        match self.by_ending.get(ending) {
            Some(found) => Some(found),
            // Almost every ending on a disk is lower case already, so the
            // lower-case copy is only made when it is needed.
            None if ending.chars().any(char::is_uppercase) => self.by_ending.get(&ending.to_lowercase()),
            None => None,
        }
    }

    /// The same list, looking for one kind only: for a survey for a backup
    /// that wants photos only, or videos only.
    #[must_use]
    pub fn only(mut self, kind: MediaKind) -> Kinds {
        for looked in self.by_ending.values_mut() {
            if looked.kind != kind {
                looked.on = false;
            }
        }
        self
    }

    /// The kind a file with this ending is looked for as, or `None` if it
    /// is not on the list or switched off. This is all a survey, which
    /// never opens a file, goes by.
    pub fn looked_for(&self, ending: &str) -> Option<MediaKind> {
        self.get(ending).filter(|l| l.on).map(|l| l.kind)
    }

    /// True if files with this ending are passed over without being opened.
    pub fn switched_off(&self, ending: &str) -> bool {
        self.get(ending).is_some_and(|l| !l.on)
    }

    /// The kind a file with this ending is found as when Keevoro cannot
    /// tell from inside it: for a kind looked for that is known only by
    /// its name. `None` for one Keevoro recognises from inside (a file
    /// called .jpg that is not a JPEG is not a photo), and for any ending
    /// not looked for.
    pub fn by_name(&self, ending: &str) -> Option<MediaKind> {
        self.get(ending).filter(|l| l.on && !l.known_inside).map(|l| l.kind)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(kinds: &FileKinds, ending: &str) -> FileKind {
        kinds.list().into_iter().find(|k| k.ending == ending).unwrap()
    }

    #[test]
    fn the_list_starts_as_the_built_in_one_with_everything_on() {
        let kinds = FileKinds::default();
        let list = kinds.list();
        assert_eq!(list.len(), BUILT_IN.len());
        assert!(list.iter().all(|k| k.on && k.built_in));
        assert_eq!(line(&kinds, "jpg").kind, MediaKind::Photo);
        assert_eq!(line(&kinds, "mts").kind, MediaKind::Video);
        assert!(kinds.is_built_in());
    }

    #[test]
    fn no_built_in_ending_is_listed_twice_or_one_keevoro_never_opens() {
        let mut seen = std::collections::HashSet::new();
        for b in BUILT_IN {
            assert!(seen.insert(b.ending), "{} twice", b.ending);
            assert_eq!(plain_ending(b.ending).unwrap(), b.ending);
            assert!(!never_media_extension(b.ending), "{}", b.ending);
        }
    }

    #[test]
    fn endings_are_compared_without_case_or_the_dot() {
        assert_eq!(plain_ending(" .HEIF ").unwrap(), "heif");
        assert_eq!(plain_ending("Insp").unwrap(), "insp");
        let kinds = Kinds::default();
        for ending in ["jpg", "JPG", ".Jpg"] {
            assert_eq!(kinds.looked_for(ending), Some(MediaKind::Photo), "{ending}");
        }
    }

    #[test]
    fn what_cannot_be_an_ending_is_refused_in_plain_words() {
        for (typed, refusal) in [
            ("", KindRefusal::Empty),
            (" . ", KindRefusal::Empty),
            ("abcdefghijk", KindRefusal::TooLong),
            ("he if", KindRefusal::NotAnEnding("he if".into())),
            ("tar.gz", KindRefusal::NotAnEnding("tar.gz".into())),
            ("a/b", KindRefusal::NotAnEnding("a/b".into())),
            ("..\\x", KindRefusal::NotAnEnding("..\\x".into())),
            ("x:y", KindRefusal::NotAnEnding("x:y".into())),
            ("*", KindRefusal::NotAnEnding("*".into())),
        ] {
            assert_eq!(plain_ending(typed), Err(refusal), "{typed:?}");
        }
        assert_eq!(
            KindRefusal::NotAnEnding("tar.gz".into()).to_string(),
            "\"tar.gz\" can't be the ending of a file name. Use letters and numbers only, with no spaces or dots, such as heif."
        );
    }

    #[test]
    fn an_ending_is_added_once_and_never_one_for_programs_documents_or_music() {
        let mut kinds = FileKinds::default();
        let added = kinds.add(".XYZ", MediaKind::Video).unwrap();
        assert_eq!(
            added,
            FileKind {
                ending: "xyz".into(),
                kind: MediaKind::Video,
                on: true,
                built_in: false
            }
        );
        assert_eq!(kinds.list().last(), Some(&added), "added at the end");
        assert_eq!(
            kinds.add("xyz", MediaKind::Photo),
            Err(KindRefusal::AlreadyListed("xyz".into()))
        );
        assert_eq!(
            kinds.add("JPG", MediaKind::Photo),
            Err(KindRefusal::AlreadyListed("jpg".into()))
        );
        for never in ["exe", "PDF", "mp3", "zip", "lnk"] {
            assert!(
                matches!(kinds.add(never, MediaKind::Photo), Err(KindRefusal::NeverMedia(_))),
                "{never}"
            );
        }
        assert_eq!(
            KindRefusal::NeverMedia("exe".into()).to_string(),
            "Files ending .exe are programs, documents, music or archives, never photos or videos, so Keevoro doesn't look inside them."
        );
    }

    #[test]
    fn a_built_in_kind_can_be_switched_off_and_changed_but_not_renamed_or_removed() {
        let mut kinds = FileKinds::default();
        let off = kinds
            .change(
                "AVI",
                &KindChange {
                    on: Some(false),
                    ..KindChange::default()
                },
            )
            .unwrap();
        assert!(!off.on && off.built_in);
        assert!(!line(&kinds, "avi").on);
        assert_eq!(kinds.remove("avi"), Err(KindRefusal::BuiltIn("avi".into())));
        let renamed = KindChange {
            ending: Some("avx".into()),
            ..KindChange::default()
        };
        assert_eq!(kinds.change("avi", &renamed), Err(KindRefusal::BuiltIn("avi".into())));
        // Naming it as it is already is no rename.
        let same = KindChange {
            ending: Some(".AVI".into()),
            on: Some(true),
            ..KindChange::default()
        };
        assert!(kinds.change("avi", &same).unwrap().on);
        assert!(kinds.is_built_in(), "back as it came, so nothing is kept about it");
        assert_eq!(
            kinds.change("nothing", &KindChange::default()),
            Err(KindRefusal::NotListed("nothing".into()))
        );
    }

    #[test]
    fn a_kind_of_your_own_can_be_renamed_changed_and_removed() {
        let mut kinds = FileKinds::default();
        kinds.add("insp", MediaKind::Video).unwrap();
        kinds.add("lrv", MediaKind::Video).unwrap();
        let changed = kinds
            .change(
                "insp",
                &KindChange {
                    kind: Some(MediaKind::Photo),
                    ending: Some("INSX".into()),
                    on: Some(false),
                },
            )
            .unwrap();
        assert_eq!(
            (changed.ending.as_str(), changed.kind, changed.on),
            ("insx", MediaKind::Photo, false)
        );
        assert!(kinds.list().iter().all(|k| k.ending != "insp"));
        let clash = KindChange {
            ending: Some("lrv".into()),
            ..KindChange::default()
        };
        assert_eq!(
            kinds.change("insx", &clash),
            Err(KindRefusal::AlreadyListed("lrv".into()))
        );
        assert_eq!(kinds.remove(".INSX").unwrap().ending, "insx");
        assert_eq!(kinds.remove("insx"), Err(KindRefusal::NotListed("insx".into())));
        assert_eq!(kinds.list().len(), BUILT_IN.len() + 1);
    }

    #[test]
    fn putting_back_the_built_in_list_keeps_your_own_kinds() {
        let mut kinds = FileKinds::default();
        kinds.add("insp", MediaKind::Photo).unwrap();
        for ending in ["jpg", "mov"] {
            let off = KindChange {
                on: Some(false),
                kind: Some(MediaKind::Video),
                ..KindChange::default()
            };
            kinds.change(ending, &off).unwrap();
        }
        kinds.put_back_built_in();
        let list = kinds.list();
        assert!(list.iter().filter(|k| k.built_in).all(|k| k.on));
        assert_eq!(line(&kinds, "jpg").kind, MediaKind::Photo);
        assert_eq!(list.last().unwrap().ending, "insp");
    }

    #[test]
    fn the_lookup_says_what_is_looked_for_passed_over_and_known_by_name_alone() {
        let mut kinds = FileKinds::default();
        kinds.add("xyz", MediaKind::Photo).unwrap();
        let off = KindChange {
            on: Some(false),
            ..KindChange::default()
        };
        kinds.change("avi", &off).unwrap();
        let lookup = kinds.lookup();

        assert_eq!(lookup.looked_for("XYZ"), Some(MediaKind::Photo));
        assert_eq!(
            lookup.by_name("xyz"),
            Some(MediaKind::Photo),
            "yours: the name is enough"
        );
        assert_eq!(lookup.by_name("cr2"), Some(MediaKind::Photo), "raw: known by name");
        assert_eq!(lookup.by_name("MTS"), Some(MediaKind::Video));
        assert_eq!(lookup.by_name("jpg"), None, "a JPEG is recognised from inside");
        assert_eq!(lookup.looked_for("jpg"), Some(MediaKind::Photo));

        assert!(lookup.switched_off("AVI"));
        assert_eq!(lookup.looked_for("avi"), None);
        assert_eq!(lookup.by_name("avi"), None);

        assert!(!lookup.switched_off("dat"));
        assert_eq!(lookup.looked_for("dat"), None);
        assert_eq!(lookup.by_name(""), None);
    }

    #[test]
    fn a_list_for_one_kind_only_looks_for_that_kind() {
        let photos = Kinds::default().only(MediaKind::Photo);
        assert_eq!(photos.looked_for("jpg"), Some(MediaKind::Photo));
        assert_eq!(photos.looked_for("mov"), None);
    }

    #[test]
    fn only_the_changes_are_kept_in_the_settings() {
        let mut kinds = FileKinds::default();
        assert_eq!(serde_json::to_string(&kinds).unwrap(), "[]");
        kinds.add("xyz", MediaKind::Photo).unwrap();
        let off = KindChange {
            on: Some(false),
            ..KindChange::default()
        };
        kinds.change("avi", &off).unwrap();
        let text = serde_json::to_string(&kinds).unwrap();
        assert_eq!(
            text,
            r#"[{"ending":"xyz","kind":"photo","on":true},{"ending":"avi","kind":"video","on":false}]"#
        );
        assert_eq!(serde_json::from_str::<FileKinds>(&text).unwrap(), kinds);
        let without_on: FileKinds = serde_json::from_str(r#"[{"ending":"xyz","kind":"video"}]"#).unwrap();
        assert!(without_on.list().last().unwrap().on);
    }
}
