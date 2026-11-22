//! The kinds of file Keevoro looks for, by the ending of their names.
//!
//! Keevoro comes with a list of endings it knows as photos and videos (the
//! built-in list). A person can add endings of their own, change whether
//! one is a photo, a video or an other file, switch any of them off, remove
//! their own, and put the built-in list back. One list serves the whole of
//! Keevoro, not one backup.
//!
//! An other file is one a person asks Keevoro to take along: music, a
//! document, a project file. Any ending can be added as one, even those
//! Keevoro passes over as never photos or videos; nothing is, until a
//! person adds it. Keevoro backs them up exactly, but cannot show them or
//! group them by date ([`OTHER_FILES_SAID`]).
//!
//! Only the person's changes are kept ([`FileKinds`]): the endings they
//! added, and the built-in ones they changed. So the list always starts as
//! the built-in one, and a built-in kind added in a later version simply
//! appears. A search or a survey turns the list into a [`Kinds`] once, at
//! the start, and asks it about each file, a single look in a table
//! whatever the length of the list; nothing here is read from disk.
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
//! - For a kind Keevoro has no way to read at all (a Photoshop file, a
//!   film scan, a cinema camera's raw video) nothing more is read than for
//!   any file it opens: it is not read for a date or a size, and has "No
//!   preview". It is backed up exactly all the same.
//! - An ending that is as often something else (`.dat`, `.bin`, `.ts`) is
//!   on the list switched off. While it is off it is as if it were not on
//!   the list at all: such a file is opened and kept only if it is a
//!   photo or video inside. Switched on, the ending is enough.
//!
//! The built-in kinds are in two tiers, for the list a person sees: the
//! usual ones, and the less common ones, listed only when asked for. The
//! tier changes nothing about what is looked for.
//! - A kind on the list as an other file is found by its ending alone. It
//!   is read only to be fingerprinted, never to be judged or shown.
//! - Any other file that is not plainly a program, a document or music is
//!   still opened and judged by what is inside, so a photo with an odd
//!   name is not missed.

use crate::media::{never_media_extension, MediaKind};
use crate::wants::Wants;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::sync::OnceLock;

/// What Keevoro does with what is inside a file of a built-in kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Inside {
    /// Recognised from inside (a JPEG, an MP4): a file with this ending
    /// that is something else inside is not taken for one. Read for its
    /// date and size, and previewed.
    Recognised,
    /// Known by its name (most camera raw and camcorder files), and read
    /// for a date as a photo or video is.
    ByName,
    /// Known by its name, and never read inside: Keevoro has no way to read
    /// these, so it does not try. Dated by the file and its folder, as any
    /// file Keevoro cannot read, and shown with "No preview". Unless what
    /// is inside turns out to be a kind Keevoro does read, such as a JPEG
    /// given another name: then it is read as that.
    NotRead,
}

/// One built-in kind.
struct BuiltIn {
    ending: &'static str,
    kind: MediaKind,
    inside: Inside,
    /// A camera's raw format (see [`crate::is_raw_extension`]).
    raw: bool,
    /// One most people meet. The rest are listed only when asked for,
    /// though every one is looked for just the same.
    usual: bool,
    /// An ending that is often something else entirely (`.dat`, `.bin`):
    /// on the list, but switched off until a person switches it on.
    unsure: bool,
}

impl BuiltIn {
    /// Whether it is looked for as Keevoro comes.
    fn on_at_first(&self) -> bool {
        !self.unsure
    }
}

const fn photo(ending: &'static str) -> BuiltIn {
    BuiltIn {
        ending,
        kind: MediaKind::Photo,
        inside: Inside::Recognised,
        raw: false,
        usual: false,
        unsure: false,
    }
}

const fn video(ending: &'static str) -> BuiltIn {
    BuiltIn {
        kind: MediaKind::Video,
        ..photo(ending)
    }
}

/// A kind known only by its name: there is nothing inside it that tells
/// Keevoro what it is.
const fn by_name(built_in: BuiltIn) -> BuiltIn {
    BuiltIn {
        inside: Inside::ByName,
        ..built_in
    }
}

/// A kind known only by its name, that Keevoro cannot read.
const fn not_read(built_in: BuiltIn) -> BuiltIn {
    BuiltIn {
        inside: Inside::NotRead,
        ..built_in
    }
}

const fn raw(built_in: BuiltIn) -> BuiltIn {
    BuiltIn { raw: true, ..built_in }
}

const fn usual(built_in: BuiltIn) -> BuiltIn {
    BuiltIn {
        usual: true,
        ..built_in
    }
}

/// An ending that is often not a photo or video at all: listed, off, and
/// never read inside once switched on.
const fn unsure(built_in: BuiltIn) -> BuiltIn {
    BuiltIn {
        unsure: true,
        inside: Inside::NotRead,
        ..built_in
    }
}

/// The kinds Keevoro knows, in the order they are listed: the usual ones
/// of each sort first.
const BUILT_IN: &[BuiltIn] = &[
    // Photos
    usual(photo("jpg")),
    usual(photo("jpeg")),
    usual(photo("png")),
    usual(photo("heic")),
    usual(photo("heif")),
    usual(photo("tif")),
    usual(photo("tiff")),
    usual(photo("webp")),
    usual(photo("bmp")),
    usual(photo("avif")),
    usual(photo("gif")),
    // Camera raw
    usual(raw(by_name(photo("dng")))),
    usual(raw(by_name(photo("cr2")))),
    usual(raw(by_name(photo("cr3")))),
    usual(raw(by_name(photo("nef")))),
    usual(raw(by_name(photo("arw")))),
    usual(raw(by_name(photo("orf")))),
    usual(raw(by_name(photo("rw2")))),
    usual(raw(by_name(photo("raf")))),
    usual(raw(by_name(photo("pef")))),
    usual(raw(by_name(photo("srw")))),
    // Videos
    usual(video("mp4")),
    usual(video("mov")),
    usual(video("m4v")),
    usual(video("avi")),
    usual(video("mkv")),
    usual(video("wmv")),
    usual(video("mpg")),
    usual(video("mpeg")),
    usual(video("webm")),
    // Camcorder and phone video
    usual(by_name(video("mts"))),
    usual(by_name(video("m2ts"))),
    usual(by_name(video("3gp"))),
    //
    // Less common photos: JPEG and its relations under other names, and
    // newer kinds, all recognised from inside.
    photo("jpe"),
    photo("jfif"),
    photo("jfi"),
    photo("jif"),
    photo("jps"),
    photo("mpo"),
    photo("insp"),
    photo("jxl"),
    photo("jp2"),
    photo("jpf"),
    photo("jpx"),
    photo("jxr"),
    photo("wdp"),
    photo("hdp"),
    photo("hif"),
    photo("dib"),
    photo("apng"),
    photo("ora"),
    // Less common photos Keevoro backs up but cannot read: from editing
    // programs, film and older computers.
    not_read(photo("psd")),
    not_read(photo("psb")),
    not_read(photo("xcf")),
    not_read(photo("kra")),
    not_read(photo("j2k")),
    not_read(photo("j2c")),
    not_read(photo("exr")),
    not_read(photo("dpx")),
    not_read(photo("cin")),
    not_read(photo("tga")),
    not_read(photo("pcx")),
    not_read(photo("pict")),
    not_read(photo("pct")),
    not_read(photo("ilbm")),
    not_read(photo("lbm")),
    not_read(photo("sgi")),
    not_read(photo("ras")),
    not_read(photo("ppm")),
    not_read(photo("pgm")),
    not_read(photo("pbm")),
    not_read(photo("pnm")),
    not_read(photo("mng")),
    not_read(photo("jng")),
    not_read(photo("qoi")),
    // Less common camera raw
    raw(by_name(photo("crw"))),
    raw(by_name(photo("nrw"))),
    raw(by_name(photo("srf"))),
    raw(by_name(photo("sr2"))),
    raw(by_name(photo("x3f"))),
    raw(by_name(photo("3fr"))),
    raw(by_name(photo("erf"))),
    raw(by_name(photo("kdc"))),
    raw(by_name(photo("mrw"))),
    raw(by_name(photo("raw"))),
    raw(by_name(photo("iiq"))),
    raw(by_name(photo("dcr"))),
    raw(by_name(photo("mef"))),
    raw(by_name(photo("mos"))),
    raw(by_name(photo("rwl"))),
    raw(by_name(photo("gpr"))),
    raw(not_read(photo("fff"))),
    raw(not_read(photo("ari"))),
    raw(not_read(photo("bay"))),
    raw(not_read(photo("k25"))),
    raw(not_read(photo("mdc"))),
    raw(not_read(photo("ptx"))),
    raw(not_read(photo("eip"))),
    // Less common videos, recognised from inside
    video("qt"),
    video("insv"),
    video("lrv"),
    video("asf"),
    video("flv"),
    video("f4v"),
    video("divx"),
    video("mk3d"),
    video("mpe"),
    video("m2v"),
    video("mp2v"),
    video("vro"),
    // Less common camcorder and disc video
    by_name(video("3g2")),
    by_name(video("mod")),
    by_name(video("tod")),
    by_name(video("vob")),
    by_name(video("dv")),
    // Less common videos Keevoro backs up but cannot read
    not_read(video("m2t")),
    not_read(video("dif")),
    not_read(video("mxf")),
    not_read(video("r3d")),
    not_read(video("braw")),
    not_read(video("crm")),
    not_read(video("ogv")),
    not_read(video("ogm")),
    not_read(video("rm")),
    not_read(video("rmvb")),
    not_read(video("wtv")),
    not_read(video("nut")),
    not_read(video("y4m")),
    // Endings that are as often something else: off until switched on.
    unsure(video("ts")),
    unsure(video("dat")),
    unsure(video("bin")),
    unsure(video("seq")),
    unsure(video("001")),
    unsure(photo("img")),
    unsure(photo("pic")),
    unsure(photo("iff")),
    unsure(raw(photo("cap"))),
];

/// The built-in endings before the list was widened (Keevoro 0.51 and
/// earlier). A choice kept about one of these is always a change to a
/// built-in kind; a choice kept about any other ending, without the mark
/// that says it is a change to one, is an ending the person added.
const EARLIER: &[&str] = &[
    "jpg", "jpeg", "jpe", "png", "heic", "heif", "tif", "tiff", "webp", "bmp", "avif", "jxl", "dng", "cr2", "cr3",
    "crw", "nef", "nrw", "arw", "srf", "sr2", "orf", "rw2", "raf", "pef", "srw", "x3f", "3fr", "erf", "kdc", "mrw",
    "raw", "iiq", "dcr", "mef", "mos", "rwl", "mp4", "mov", "m4v", "avi", "mkv", "wmv", "mpg", "mpeg", "webm", "insv",
    "asf", "flv", "mts", "m2ts", "3gp", "3g2", "mod", "tod", "vob", "dv",
];

/// The built-in list by ending, made the first time it is asked for and
/// kept for as long as Keevoro runs.
fn built_ins() -> &'static HashMap<&'static str, &'static BuiltIn> {
    static BY_ENDING: OnceLock<HashMap<&'static str, &'static BuiltIn>> = OnceLock::new();
    BY_ENDING.get_or_init(|| BUILT_IN.iter().map(|b| (b.ending, b)).collect())
}

fn built_in(ending: &str) -> Option<&'static BuiltIn> {
    built_ins().get(ending).copied()
}

/// The ending in lower case. ASCII endings, which nearly every file has,
/// are lowered into `buffer` with nothing allocated; `None` for one too
/// long for any list, or with other letters (lowered by the caller, which
/// is rare).
pub(crate) fn ascii_lower<'a, const N: usize>(ending: &str, buffer: &'a mut [u8; N]) -> Option<&'a str> {
    let bytes = ending.as_bytes();
    if bytes.len() > N || !ending.is_ascii() {
        return None;
    }
    let lowered = &mut buffer[..bytes.len()];
    lowered.copy_from_slice(bytes);
    lowered.make_ascii_lowercase();
    std::str::from_utf8(lowered).ok()
}

/// Is this ending, as it comes from a file name with any case, a camera's
/// raw format on the built-in list?
pub(crate) fn raw_ending(ending: &str) -> bool {
    let ending = ending.strip_prefix('.').unwrap_or(ending);
    let mut buffer = [0u8; LONGEST_ENDING];
    match ascii_lower(ending, &mut buffer) {
        Some(lower) => built_in(lower).is_some_and(|b| b.raw),
        None => false,
    }
}

/// Is a photo with this ending one Keevoro has no way to make a preview
/// of? Such a photo is shown with "No preview" without being opened.
pub fn never_previewed(ending: &str) -> bool {
    let ending = ending.strip_prefix('.').unwrap_or(ending);
    let mut buffer = [0u8; LONGEST_ENDING];
    ascii_lower(ending, &mut buffer)
        .and_then(built_in)
        .is_some_and(|b| b.inside == Inside::NotRead)
}

/// The longest ending accepted. Real ones are three or four letters.
pub const LONGEST_ENDING: usize = 10;

/// What to expect of other files, said wherever one is added.
pub const OTHER_FILES_SAID: &str = "Keevoro will back these up exactly, but cannot show them or group them by date.";

/// One change a person made to the list: an ending they added, or a
/// built-in one they changed. This is what the settings file keeps.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KindChoice {
    pub ending: String,
    pub kind: MediaKind,
    #[serde(default = "on")]
    pub on: bool,
    /// A change to a built-in kind added to the list after Keevoro 0.51.
    /// Without it, a choice about such an ending is one the person added
    /// before Keevoro came with it, and stays theirs. (A change to one of
    /// the kinds Keevoro came with before needs no mark: see [`EARLIER`].)
    #[serde(default, rename = "builtIn", skip_serializing_if = "std::ops::Not::not")]
    pub built_in: bool,
}

fn on() -> bool {
    true
}

impl KindChoice {
    /// A change to a built-in kind, rather than an ending the person added.
    fn changes_built_in(&self) -> bool {
        (self.built_in || EARLIER.contains(&self.ending.as_str())) && built_in(&self.ending).is_some()
    }
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
    /// between photo, video and other file, but not renamed or removed.
    pub built_in: bool,
    /// Listed without asking for the less common kinds: a usual built-in
    /// kind, or one of the person's own. The rest are looked for just the
    /// same, but listed only when asked for, or searched for.
    pub usual: bool,
    /// An ending that is often not a photo or video at all, such as .dat:
    /// switched off as Keevoro comes. While it is off, a file with it is
    /// still kept when what is inside is a photo or video Keevoro knows.
    pub unsure: bool,
}

/// Why a change to the list was refused, in words for the person.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KindRefusal {
    Empty,
    TooLong,
    /// Holds a space, a dot or a character that belongs in a path.
    NotAnEnding(String),
    /// A program, a document, music or an archive, asked for as a photo or
    /// a video: it can only be added as an other file.
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
                "Files ending .{ending} are programs, documents, music or archives, never photos or videos. To back them up as they are, add .{ending} as an other file."
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
    ///
    /// An ending the person added that Keevoro has since come to know is
    /// listed once, as theirs, as they set it.
    pub fn list(&self) -> Vec<FileKind> {
        let mine = |ending: &str| self.0.iter().find(|c| c.ending == ending);
        let built: Vec<FileKind> = BUILT_IN
            .iter()
            .filter_map(|b| {
                let (kind, on) = match mine(b.ending) {
                    Some(c) if !c.changes_built_in() => return None,
                    Some(c) => (c.kind, c.on),
                    None => (b.kind, b.on_at_first()),
                };
                Some(FileKind {
                    ending: b.ending.to_string(),
                    kind,
                    on,
                    built_in: true,
                    usual: b.usual,
                    unsure: b.unsure,
                })
            })
            .collect();
        let added = self.0.iter().filter(|c| !c.changes_built_in()).map(|c| FileKind {
            ending: c.ending.clone(),
            kind: c.kind,
            on: c.on,
            built_in: false,
            usual: true,
            unsure: false,
        });
        built.into_iter().chain(added).collect()
    }

    fn line(&self, ending: &str) -> Option<FileKind> {
        self.list().into_iter().find(|k| k.ending == ending)
    }

    /// Is anything on the list an other file, switched on or off? Only
    /// then are other files spoken of anywhere.
    pub fn has_other(&self) -> bool {
        self.0.iter().any(|c| c.kind == MediaKind::Other)
    }

    /// Adds an ending as a photo, a video or an other file, switched on.
    /// An ending that is plainly not a photo or video (a document, music)
    /// can only be added as an other file. A built-in ending that is off,
    /// or listed as another kind, is switched on as the kind asked for.
    pub fn add(&mut self, typed: &str, kind: MediaKind) -> Result<FileKind, KindRefusal> {
        let ending = plain_ending(typed)?;
        if let Some(listed) = self.line(&ending) {
            // A built-in kind switched off, or listed as something else, is
            // simply made what was asked: .dat added as an other file, say.
            if listed.built_in && (!listed.on || listed.kind != kind) {
                let change = KindChange {
                    kind: Some(kind),
                    on: Some(true),
                    ending: None,
                };
                return self.change(&ending, &change);
            }
            return Err(KindRefusal::AlreadyListed(ending));
        }
        refuse_as_media(&ending, kind)?;
        self.0.push(KindChoice {
            ending: ending.clone(),
            kind,
            on: true,
            built_in: false,
        });
        Ok(FileKind {
            ending,
            kind,
            on: true,
            built_in: false,
            usual: true,
            unsure: false,
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
        }
        let wanted = KindChoice {
            ending: renamed.unwrap_or_else(|| ending.clone()),
            kind: change.kind.unwrap_or(now.kind),
            on: change.on.unwrap_or(now.on),
            built_in: now.built_in && !EARLIER.contains(&ending.as_str()),
        };
        // A built-in kind is what it is: .ts, a video, is also the ending
        // of some programs' code, but can be switched on all the same.
        if !now.built_in {
            refuse_as_media(&wanted.ending, wanted.kind)?;
        }
        let same_as_it_came = now.built_in
            && built_in(&wanted.ending).is_some_and(|b| b.kind == wanted.kind && wanted.on == b.on_at_first());
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
            ..now
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

    /// Puts every built-in kind back as it came: the photo or video it is,
    /// switched on (or off, for an ending that is often something else).
    /// The person's own kinds stay as they are.
    pub fn put_back_built_in(&mut self) {
        self.0.retain(|c| !c.changes_built_in());
    }

    /// The list as a quick lookup, made once for a search or a survey.
    pub fn lookup(&self) -> Kinds {
        let by_ending = self
            .list()
            .into_iter()
            // An ending that is often something else, while it is off, is
            // as if it were not on the list: a file with it is opened and
            // judged by what is inside, as it always was, never passed over.
            .filter(|k| !(k.built_in && k.unsure && !k.on))
            .map(|k| {
                let inside = if k.built_in {
                    built_in(&k.ending).map(|b| b.inside)
                } else {
                    None
                };
                (
                    k.ending,
                    Looked {
                        kind: k.kind,
                        on: k.on,
                        known_inside: inside == Some(Inside::Recognised),
                        // The person's own kinds are read as they always were.
                        read_inside: inside != Some(Inside::NotRead),
                    },
                )
            })
            .collect();
        Kinds { by_ending }
    }
}

/// A document, music, a program or an archive is never a photo or a video.
fn refuse_as_media(ending: &str, kind: MediaKind) -> Result<(), KindRefusal> {
    if kind.is_media() && never_media_extension(ending) {
        return Err(KindRefusal::NeverMedia(ending.to_string()));
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Looked {
    kind: MediaKind,
    on: bool,
    known_inside: bool,
    read_inside: bool,
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
    /// One look in a table made once, whatever the length of the list.
    /// Nothing is allocated for an ending in plain letters and numbers,
    /// as nearly every one is, in whatever case (`IMG_0048.JPG`).
    fn get(&self, ending: &str) -> Option<&Looked> {
        let ending = ending.strip_prefix('.').unwrap_or(ending);
        if let Some(found) = self.by_ending.get(ending) {
            return Some(found);
        }
        let mut buffer = [0u8; LONGEST_ENDING];
        match ascii_lower(ending, &mut buffer) {
            Some(lower) if lower != ending => self.by_ending.get(lower),
            Some(_) => None,
            // Letters beyond plain English ones, which are rare.
            None if !ending.is_ascii() && ending.chars().any(char::is_uppercase) => {
                self.by_ending.get(&ending.to_lowercase())
            }
            None => None,
        }
    }

    /// The same list, looking only for the kinds a backup wants: for a
    /// survey for a backup of photos only, say.
    #[must_use]
    pub fn wanted_by(mut self, wants: Wants) -> Kinds {
        for looked in self.by_ending.values_mut() {
            if !wants.takes(looked.kind) {
                looked.on = false;
            }
        }
        self
    }

    /// Is any kind on the list an other file, on or off? Only then are
    /// other files spoken of.
    pub fn lists_other(&self) -> bool {
        self.by_ending.values().any(|l| l.kind == MediaKind::Other)
    }

    /// Is this ending looked for as an other file? Such a file is found by
    /// its ending alone, and never opened to see what it is.
    pub fn is_other(&self, ending: &str) -> bool {
        self.looked_for(ending) == Some(MediaKind::Other)
    }

    /// The kind a file with this ending is looked for as, or `None` if it
    /// is not on the list or switched off. This is all a survey, which
    /// never opens a file, goes by.
    pub fn looked_for(&self, ending: &str) -> Option<MediaKind> {
        self.get(ending).filter(|l| l.on).map(|l| l.kind)
    }

    /// The kind of a file with this ending, and whether it is looked for,
    /// for any ending on the list, switched on or off: what a look inside
    /// a folder shows, saying of one switched off that it is left out.
    pub fn listed(&self, ending: &str) -> Option<(MediaKind, bool)> {
        self.get(ending).map(|l| (l.kind, l.on))
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

    /// Whether a file with this ending, found by its name alone, is read
    /// inside for its date and size. False for the kinds Keevoro has no
    /// way to read, so a search spends nothing trying; true for every
    /// other ending, on the list or not.
    pub fn read_inside(&self, ending: &str) -> bool {
        self.get(ending).is_none_or(|l| l.read_inside)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(kinds: &FileKinds, ending: &str) -> FileKind {
        kinds.list().into_iter().find(|k| k.ending == ending).unwrap()
    }

    #[test]
    fn the_list_starts_as_the_built_in_one_with_everything_on_but_the_unsure_endings() {
        let kinds = FileKinds::default();
        let list = kinds.list();
        assert_eq!(list.len(), BUILT_IN.len());
        assert!(list.iter().all(|k| k.built_in && k.on == !k.unsure));
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
            // .ts is also the ending of program code, so it waits, off,
            // for a person who knows theirs are videos.
            assert!(!never_media_extension(b.ending) || b.unsure, "{}", b.ending);
            assert!(b.kind.is_media(), "{}", b.ending);
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
                built_in: false,
                usual: true,
                unsure: false,
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
        // Built in but off, or another kind: made what was asked.
        let dat = kinds.add(".DAT", MediaKind::Other).unwrap();
        assert_eq!((dat.kind, dat.on, dat.built_in), (MediaKind::Other, true, true));
        assert_eq!(kinds.add("tga", MediaKind::Video).unwrap().kind, MediaKind::Video);
        for never in ["exe", "PDF", "mp3", "zip", "lnk"] {
            assert!(
                matches!(kinds.add(never, MediaKind::Photo), Err(KindRefusal::NeverMedia(_))),
                "{never}"
            );
        }
        assert!(kinds.has_other());
        assert_eq!(
            KindRefusal::NeverMedia("exe".into()).to_string(),
            "Files ending .exe are programs, documents, music or archives, never photos or videos. To back them up as they are, add .exe as an other file."
        );
    }

    #[test]
    fn any_ending_can_be_added_as_an_other_file_but_never_a_document_as_a_photo() {
        let mut kinds = FileKinds::default();
        for ending in ["flac", "PDF", "docx", "zip", "blend", "sys"] {
            let added = kinds.add(ending, MediaKind::Other).unwrap();
            assert_eq!((added.kind, added.on, added.built_in), (MediaKind::Other, true, false));
        }
        assert!(kinds.has_other());
        // Changed to a photo: refused, as adding one would be.
        let photo = KindChange {
            kind: Some(MediaKind::Photo),
            ..KindChange::default()
        };
        assert_eq!(
            kinds.change("flac", &photo),
            Err(KindRefusal::NeverMedia("flac".into()))
        );
        // An other file of your own can take an ending Keevoro passes over.
        kinds.add("xyz", MediaKind::Other).unwrap();
        let renamed = KindChange {
            ending: Some("mp3".into()),
            ..KindChange::default()
        };
        assert_eq!(kinds.change("xyz", &renamed).unwrap().ending, "mp3");
        // A built-in kind can be carried as an other file too.
        let as_other = KindChange {
            kind: Some(MediaKind::Other),
            ..KindChange::default()
        };
        assert_eq!(kinds.change("tif", &as_other).unwrap().kind, MediaKind::Other);
        let lookup = kinds.lookup();
        assert!(lookup.lists_other());
        assert!(lookup.is_other("FLAC") && lookup.is_other("tif") && !lookup.is_other("jpg"));
        assert_eq!(lookup.looked_for("pdf"), Some(MediaKind::Other));
        assert!(OTHER_FILES_SAID.contains("exactly"));
    }

    #[test]
    fn nothing_is_an_other_file_until_a_person_adds_one() {
        let kinds = Kinds::default();
        assert!(!kinds.lists_other());
        for ending in ["mp3", "pdf", "docx", "zip", "flac", "txt"] {
            assert_eq!(kinds.looked_for(ending), None, "{ending}");
        }
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
        kinds.add("abc", MediaKind::Video).unwrap();
        kinds.add("xyz", MediaKind::Video).unwrap();
        let changed = kinds
            .change(
                "abc",
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
        assert!(kinds.list().iter().all(|k| k.ending != "abc"));
        let clash = KindChange {
            ending: Some("xyz".into()),
            ..KindChange::default()
        };
        assert_eq!(
            kinds.change("insx", &clash),
            Err(KindRefusal::AlreadyListed("xyz".into()))
        );
        assert_eq!(kinds.remove(".INSX").unwrap().ending, "insx");
        assert_eq!(kinds.remove("insx"), Err(KindRefusal::NotListed("insx".into())));
        assert_eq!(kinds.list().len(), BUILT_IN.len() + 1);
    }

    #[test]
    fn putting_back_the_built_in_list_keeps_your_own_kinds() {
        let mut kinds = FileKinds::default();
        kinds.add("xyz", MediaKind::Photo).unwrap();
        for ending in ["jpg", "mov", "tga"] {
            let off = KindChange {
                on: Some(false),
                kind: Some(MediaKind::Video),
                ..KindChange::default()
            };
            kinds.change(ending, &off).unwrap();
        }
        kinds.put_back_built_in();
        let list = kinds.list();
        assert!(list.iter().filter(|k| k.built_in).all(|k| k.on == !k.unsure));
        assert_eq!(line(&kinds, "jpg").kind, MediaKind::Photo);
        assert_eq!(line(&kinds, "tga").kind, MediaKind::Photo);
        assert_eq!(list.last().unwrap().ending, "xyz");
        assert!(kinds.list().iter().filter(|k| k.built_in).count() == BUILT_IN.len());
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
    fn a_list_for_what_a_backup_wants_looks_for_only_that() {
        let photos = Kinds::default().wanted_by(Wants::only(MediaKind::Photo));
        assert_eq!(photos.looked_for("jpg"), Some(MediaKind::Photo));
        assert_eq!(photos.looked_for("mov"), None);
        let mut mine = FileKinds::default();
        mine.add("flac", MediaKind::Other).unwrap();
        let no_others = mine.lookup().wanted_by(Wants {
            others: false,
            ..Wants::default()
        });
        assert_eq!(no_others.looked_for("flac"), None);
        assert_eq!(no_others.looked_for("mov"), Some(MediaKind::Video));
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

    /// The built-in list as Keevoro 0.51 had it: each ending, what it was,
    /// and whether it was recognised from inside.
    const AS_IT_WAS: &[(&str, MediaKind, bool)] = &[
        ("jpg", MediaKind::Photo, true),
        ("jpeg", MediaKind::Photo, true),
        ("jpe", MediaKind::Photo, true),
        ("png", MediaKind::Photo, true),
        ("heic", MediaKind::Photo, true),
        ("heif", MediaKind::Photo, true),
        ("tif", MediaKind::Photo, true),
        ("tiff", MediaKind::Photo, true),
        ("webp", MediaKind::Photo, true),
        ("bmp", MediaKind::Photo, true),
        ("avif", MediaKind::Photo, true),
        ("jxl", MediaKind::Photo, true),
        ("dng", MediaKind::Photo, false),
        ("cr2", MediaKind::Photo, false),
        ("cr3", MediaKind::Photo, false),
        ("crw", MediaKind::Photo, false),
        ("nef", MediaKind::Photo, false),
        ("nrw", MediaKind::Photo, false),
        ("arw", MediaKind::Photo, false),
        ("srf", MediaKind::Photo, false),
        ("sr2", MediaKind::Photo, false),
        ("orf", MediaKind::Photo, false),
        ("rw2", MediaKind::Photo, false),
        ("raf", MediaKind::Photo, false),
        ("pef", MediaKind::Photo, false),
        ("srw", MediaKind::Photo, false),
        ("x3f", MediaKind::Photo, false),
        ("3fr", MediaKind::Photo, false),
        ("erf", MediaKind::Photo, false),
        ("kdc", MediaKind::Photo, false),
        ("mrw", MediaKind::Photo, false),
        ("raw", MediaKind::Photo, false),
        ("iiq", MediaKind::Photo, false),
        ("dcr", MediaKind::Photo, false),
        ("mef", MediaKind::Photo, false),
        ("mos", MediaKind::Photo, false),
        ("rwl", MediaKind::Photo, false),
        ("mp4", MediaKind::Video, true),
        ("mov", MediaKind::Video, true),
        ("m4v", MediaKind::Video, true),
        ("avi", MediaKind::Video, true),
        ("mkv", MediaKind::Video, true),
        ("wmv", MediaKind::Video, true),
        ("mpg", MediaKind::Video, true),
        ("mpeg", MediaKind::Video, true),
        ("webm", MediaKind::Video, true),
        ("insv", MediaKind::Video, true),
        ("asf", MediaKind::Video, true),
        ("flv", MediaKind::Video, true),
        ("mts", MediaKind::Video, false),
        ("m2ts", MediaKind::Video, false),
        ("3gp", MediaKind::Video, false),
        ("3g2", MediaKind::Video, false),
        ("mod", MediaKind::Video, false),
        ("tod", MediaKind::Video, false),
        ("vob", MediaKind::Video, false),
        ("dv", MediaKind::Video, false),
    ];

    #[test]
    fn every_kind_keevoro_came_with_before_is_looked_for_exactly_as_it_was() {
        let was: Vec<&str> = AS_IT_WAS.iter().map(|w| w.0).collect();
        assert_eq!(was, EARLIER);
        let kinds = Kinds::default();
        for &(ending, kind, known_inside) in AS_IT_WAS {
            assert_eq!(kinds.looked_for(ending), Some(kind), "{ending}");
            assert_eq!(kinds.by_name(ending).is_none(), known_inside, "{ending}");
            assert!(kinds.read_inside(ending), "{ending}: read as before");
            let b = built_in(ending).unwrap();
            assert!(!b.unsure && b.inside != Inside::NotRead, "{ending}");
            assert_eq!(
                b.raw,
                crate::media::tests_support::WAS_RAW.contains(&ending),
                "{ending}"
            );
        }
    }

    #[test]
    fn the_list_is_in_two_tiers_with_every_photo_and_video_on_but_the_unsure_endings() {
        let list = FileKinds::default().list();
        let usual = list.iter().filter(|k| k.usual).count();
        let less_common = list.iter().filter(|k| !k.usual).count();
        assert!((25..=45).contains(&usual), "the usual kinds stay few: {usual}");
        assert!(less_common > usual, "{less_common}");
        for ending in ["jpg", "heic", "png", "gif", "cr2", "nef", "dng", "mp4", "mov", "mts"] {
            assert!(line(&FileKinds::default(), ending).usual, "{ending}");
        }
        for ending in ["tga", "psd", "exr", "mxf", "r3d", "braw", "insp", "lrv", "jfif", "crw"] {
            let kind = line(&FileKinds::default(), ending);
            assert!(!kind.usual && kind.on && !kind.unsure, "{ending}");
        }
        let unsure: Vec<&str> = list.iter().filter(|k| k.unsure).map(|k| k.ending.as_str()).collect();
        assert_eq!(unsure, ["ts", "dat", "bin", "seq", "001", "img", "pic", "iff", "cap"]);
        assert!(list.iter().filter(|k| k.unsure).all(|k| !k.on && !k.usual));
        for left_out in [
            "ico", "cur", "svg", "eps", "ai", "pdf", "dcm", "aep", "prproj", "411", "dds", "thm",
        ] {
            assert!(built_in(left_out).is_none(), "{left_out}");
        }
    }

    #[test]
    fn an_unsure_ending_left_off_is_judged_by_what_is_inside_as_it_always_was() {
        let mut kinds = FileKinds::default();
        let lookup = kinds.lookup();
        for ending in ["dat", "DAT", "bin", "img", "ts"] {
            assert_eq!(lookup.listed(ending), None, "{ending}: as if not on the list");
            assert!(!lookup.switched_off(ending), "{ending}: never passed over unopened");
            assert_eq!(lookup.by_name(ending), None, "{ending}");
        }
        let on = KindChange {
            on: Some(true),
            ..KindChange::default()
        };
        // .ts is a video here, though it is also the ending of code.
        assert!(kinds.change("ts", &on).unwrap().on);
        assert!(kinds.change("DAT", &on).unwrap().on);
        let lookup = kinds.lookup();
        assert_eq!(lookup.looked_for("ts"), Some(MediaKind::Video));
        assert_eq!(
            lookup.by_name("dat"),
            Some(MediaKind::Video),
            "found by its name once on"
        );
        assert!(!lookup.read_inside("dat"), "but never read inside");
        assert_eq!(
            serde_json::to_string(&kinds).unwrap(),
            r#"[{"ending":"ts","kind":"video","on":true,"builtIn":true},{"ending":"dat","kind":"video","on":true,"builtIn":true}]"#
        );
        // Off again is as it came: nothing kept about it.
        let off = KindChange {
            on: Some(false),
            ..KindChange::default()
        };
        kinds.change("dat", &off).unwrap();
        kinds.put_back_built_in();
        assert!(kinds.is_built_in());
        assert!(!line(&kinds, "ts").on);
    }

    #[test]
    fn an_ending_a_person_added_that_keevoro_now_knows_stays_theirs_as_they_set_it() {
        // As Keevoro 0.51 kept them: .tga added as a video, switched off;
        // .psd added as a photo; .lrv as an other file; .avi switched off.
        let kept = r#"[{"ending":"tga","kind":"video","on":false},{"ending":"psd","kind":"photo"},{"ending":"lrv","kind":"other"},{"ending":"avi","kind":"video","on":false}]"#;
        let mut kinds: FileKinds = serde_json::from_str(kept).unwrap();
        let list = kinds.list();
        for ending in ["tga", "psd", "lrv", "avi"] {
            assert_eq!(list.iter().filter(|k| k.ending == ending).count(), 1, "{ending} once");
        }
        let tga = line(&kinds, "tga");
        assert_eq!(
            (tga.kind, tga.on, tga.built_in, tga.usual),
            (MediaKind::Video, false, false, true)
        );
        assert!(!line(&kinds, "psd").built_in && !line(&kinds, "lrv").built_in);
        assert!(line(&kinds, "avi").built_in && !line(&kinds, "avi").on);
        let lookup = kinds.lookup();
        assert!(lookup.switched_off("tga"));
        assert!(
            lookup.is_other("lrv"),
            "theirs, an other file, wins over the video Keevoro knows"
        );
        assert_eq!(lookup.by_name("psd"), Some(MediaKind::Photo));
        assert!(lookup.read_inside("psd"), "read as their own kinds always were");
        // Written back as it was read, still theirs.
        let written = serde_json::to_string(&kinds).unwrap();
        assert!(!written.contains("builtIn"), "{written}");
        assert_eq!(serde_json::from_str::<FileKinds>(&written).unwrap(), kinds);
        // Putting back the built-in list keeps theirs.
        kinds.put_back_built_in();
        assert!(!line(&kinds, "tga").built_in && line(&kinds, "avi").on);
        // Removing theirs leaves the kind Keevoro comes with.
        kinds.remove("tga").unwrap();
        let tga = line(&kinds, "tga");
        assert_eq!((tga.kind, tga.on, tga.built_in), (MediaKind::Photo, true, true));
        assert_eq!(kinds.remove("tga"), Err(KindRefusal::BuiltIn("tga".into())));
    }

    #[test]
    fn a_change_to_a_kind_keevoro_has_just_come_to_know_is_kept_as_a_change() {
        let mut kinds = FileKinds::default();
        let off = KindChange {
            on: Some(false),
            ..KindChange::default()
        };
        kinds.change("tga", &off).unwrap();
        kinds.change("jpg", &off).unwrap();
        let text = serde_json::to_string(&kinds).unwrap();
        assert_eq!(
            text,
            r#"[{"ending":"tga","kind":"photo","on":false,"builtIn":true},{"ending":"jpg","kind":"photo","on":false}]"#
        );
        let read: FileKinds = serde_json::from_str(&text).unwrap();
        let tga = line(&read, "tga");
        assert!(tga.built_in && !tga.on);
        assert_eq!(read.list().len(), BUILT_IN.len());
    }

    #[test]
    fn a_kind_keevoro_cannot_read_is_found_by_name_and_never_read_or_previewed() {
        let kinds = Kinds::default();
        for ending in ["tga", "PSD", "exr", "mxf", "r3d", "braw", "fff"] {
            assert!(kinds.by_name(ending).is_some(), "{ending}");
            assert!(!kinds.read_inside(ending), "{ending}");
        }
        assert!(never_previewed("tga") && never_previewed(".PSD"));
        for ending in ["jpg", "cr2", "mts", "gif", "jfif", "xyz", ""] {
            assert!(kinds.read_inside(ending), "{ending}");
            assert!(!never_previewed(ending), "{ending}");
        }
        assert!(crate::is_raw_extension("GPR") && crate::is_raw_extension("fff"));
        assert!(!crate::is_raw_extension("tga"));
    }

    #[test]
    fn the_lookup_is_made_once_and_matches_any_case_without_making_a_copy() {
        assert!(std::ptr::eq(built_ins(), built_ins()), "made once, then kept");
        assert_eq!(built_ins().len(), BUILT_IN.len());
        let mut buffer = [0u8; LONGEST_ENDING];
        let lower = ascii_lower("JPG", &mut buffer).unwrap();
        assert_eq!(lower, "jpg");
        assert!(
            std::ptr::eq(lower.as_ptr(), buffer.as_ptr()),
            "lowered in place, nothing allocated"
        );
        assert_eq!(ascii_lower("abcdefghijk", &mut buffer), None, "too long for any list");
        assert_eq!(ascii_lower("ÉCLAIR", &mut buffer), None);
        let kinds = Kinds::default();
        for ending in ["jpg", "JPG", "Jpg", ".JPG", "tGa", "M2TS"] {
            assert!(kinds.looked_for(ending).is_some(), "{ending}");
        }
        assert_eq!(kinds.looked_for("JPGX"), None);
        assert_eq!(kinds.looked_for("ABCDEFGHIJKLMNOP"), None);
        let mut mine = FileKinds::default();
        mine.add("ÉCL", MediaKind::Photo).unwrap();
        assert_eq!(
            mine.lookup().looked_for("ÉCL"),
            Some(MediaKind::Photo),
            "other letters too"
        );
    }
}
