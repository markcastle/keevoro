//! What an AI agent is allowed to do: `agents.access`.
//!
//! An agent acts faster than a person and with less doubt, and it can be
//! talked into things. So a person names the folders an agent may look
//! through and the places it may back up to, and decides whether it may
//! change anything at all. Until they do, it may use no folder and only
//! look.
//!
//! This is a security boundary, so the question "is this path inside one
//! of those folders?" is answered carefully, and "no" whenever there is
//! doubt:
//!
//! - A path is judged by where it really leads. Where it exists it is
//!   resolved by the operating system, so a symbolic link or a short name
//!   counts as the place it points to. Any part that does not exist yet is
//!   added on afterwards, as written.
//! - A path with `..` in it is refused outright, even where it would lead
//!   somewhere allowed: there is never a need for one.
//! - Paths are compared folder by folder, never as text, so `D:\Photos`
//!   does not allow `D:\Photos-private`.
//! - The many ways of writing one Windows path (`\\?\C:\`, `c:/`, a
//!   trailing slash, `\\?\UNC\server\share` for `\\server\share`) are
//!   brought to one form first, and spellings that Windows treats in
//!   surprising ways (a name ending in a dot or a space, a `:` stream, a
//!   device path) are refused.
//! - A relative path, an empty one, or one that is not valid text is
//!   refused.
//!
//! Nothing enforces this yet, because there is no agent yet. This module
//! keeps the person's choice and answers the question; the agent's tools
//! will ask it before every look and every backup.

use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};

/// Which folders an agent may use, and whether it may make changes.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentAccess {
    /// Folders an agent may look through, and everything inside them.
    #[serde(default)]
    pub look_in: Vec<String>,
    /// Folders an agent may back up to, and everything inside them.
    #[serde(default)]
    pub back_up_to: Vec<String>,
    /// False until a person allows changes: the agent may only look.
    #[serde(default)]
    pub may_change: bool,
}

/// `agents.access`: without anything in it, it only reads. Each part given
/// replaces what was there.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentsAccessRequest {
    #[serde(default)]
    pub look_in: Option<Vec<String>>,
    #[serde(default)]
    pub back_up_to: Option<Vec<String>>,
    #[serde(default)]
    pub may_change: Option<bool>,
}

impl AgentAccess {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// May an agent look in this path?
    pub fn may_look_in(&self, path: &Path) -> bool {
        is_inside_any(path, &self.look_in)
    }

    /// May an agent back up to this path?
    pub fn may_back_up_to(&self, path: &Path) -> bool {
        is_inside_any(path, &self.back_up_to)
    }

    /// Applies a request. Every folder named must be a whole path that
    /// could be checked: a folder that could never match anything is a
    /// mistake worth saying, not something to keep quietly.
    pub fn apply(&mut self, request: AgentsAccessRequest) -> Result<()> {
        let check = |folders: Vec<String>| -> Result<Vec<String>> {
            let mut kept: Vec<String> = Vec::new();
            for folder in folders {
                let folder = folder.trim().to_string();
                if spelled(&folder, Style::NATIVE).is_none() {
                    return Err(Error::refused(format!(
                        "\"{folder}\" isn't a folder Keevoro can allow. Give the whole path, \
                         such as {}, without \"..\" in it.",
                        Style::NATIVE.example()
                    )));
                }
                if !kept.contains(&folder) {
                    kept.push(folder);
                }
            }
            Ok(kept)
        };
        let look_in = request.look_in.map(check).transpose()?;
        let back_up_to = request.back_up_to.map(check).transpose()?;
        if let Some(look_in) = look_in {
            self.look_in = look_in;
        }
        if let Some(back_up_to) = back_up_to {
            self.back_up_to = back_up_to;
        }
        if let Some(may_change) = request.may_change {
            self.may_change = may_change;
        }
        Ok(())
    }
}

/// Is `path` inside (or the same as) one of `folders`?
fn is_inside_any(path: &Path, folders: &[String]) -> bool {
    let Some(asked) = resolve(path) else {
        return false;
    };
    folders
        .iter()
        .filter_map(|folder| resolve(Path::new(folder)))
        .any(|allowed| asked.is_inside(&allowed))
}

/// How the operating system writes paths.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Style {
    Windows,
    Unix,
}

impl Style {
    const NATIVE: Style = if cfg!(windows) { Style::Windows } else { Style::Unix };

    fn example(self) -> &'static str {
        match self {
            Style::Windows => "D:\\Photos",
            Style::Unix => "/Volumes/Photos",
        }
    }
}

/// A path brought to one form: where it starts (a drive, a server's share,
/// or the root), and each folder after that.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Spelled {
    start: String,
    parts: Vec<String>,
}

impl Spelled {
    fn is_inside(&self, folder: &Spelled) -> bool {
        self.start == folder.start && self.parts.starts_with(&folder.parts)
    }
}

/// Brings a path, written as text, to one form, or `None` if it is not a
/// whole path that can be judged safely.
fn spelled(text: &str, style: Style) -> Option<Spelled> {
    match style {
        Style::Unix => {
            let rest = text.strip_prefix('/')?;
            let mut parts = Vec::new();
            for part in rest.split('/') {
                match part {
                    "" | "." => {}
                    ".." => return None,
                    _ if part.contains('\0') => return None,
                    _ => parts.push(part.to_string()),
                }
            }
            Some(Spelled {
                start: "/".to_string(),
                parts,
            })
        }
        Style::Windows => spelled_for_windows(text),
    }
}

fn spelled_for_windows(text: &str) -> Option<Spelled> {
    // A verbatim path (`\\?\`) is taken literally by Windows: a forward
    // slash in one is part of a name, and `.` is a real name. Rather than
    // reason about that, such paths must be plain apart from their prefix.
    let (start, rest, verbatim) = if let Some(rest) = text.strip_prefix(r"\\?\") {
        if rest.contains('/') {
            return None;
        }
        if let Some(unc) = rest.strip_prefix("UNC\\").or_else(|| rest.strip_prefix("unc\\")) {
            let (start, rest) = server_and_share(unc)?;
            (start, rest, true)
        } else {
            let (drive, rest) = drive(rest)?;
            (drive, rest, true)
        }
    } else {
        let text = text.replace('/', "\\");
        if text.starts_with(r"\\.\") || text.starts_with(r"\\?\") || text.starts_with(r"\??\") {
            // A device path, or a verbatim one spelled with forward slashes.
            return None;
        }
        if let Some(unc) = text.strip_prefix(r"\\") {
            let (start, rest) = server_and_share(unc)?;
            (start, rest, false)
        } else {
            let (drive, rest) = drive(&text)?;
            (drive, rest.to_string(), false)
        }
    };
    let mut parts = Vec::new();
    for part in rest.split('\\') {
        match part {
            "" => {}
            "." if !verbatim => {}
            "." | ".." => return None,
            // A `:` names a stream of a file; Windows drops a trailing dot
            // or space, so `Photos.` is `Photos`. Either way the name is
            // not what it seems.
            _ if part.contains([':', '\0', '*', '?', '"', '<', '>', '|']) => return None,
            _ if part.ends_with(['.', ' ']) => return None,
            _ => parts.push(part.to_lowercase()),
        }
    }
    Some(Spelled { start, parts })
}

/// `C:\rest` as (`c:`, `rest`). A drive with no backslash after it (`C:x`)
/// means "wherever that drive was last", which cannot be judged.
fn drive(text: &str) -> Option<(String, String)> {
    let mut chars = text.chars();
    let letter = chars.next().filter(char::is_ascii_alphabetic)?;
    if chars.next() != Some(':') {
        return None;
    }
    let rest = chars.as_str();
    if !(rest.is_empty() || rest.starts_with('\\')) {
        return None;
    }
    Some((format!("{}:", letter.to_ascii_lowercase()), rest.to_string()))
}

/// `server\share\rest` as (`\\server\share`, `rest`).
fn server_and_share(text: &str) -> Option<(String, String)> {
    let mut pieces = text.splitn(3, '\\');
    let server = pieces.next().filter(|s| !s.is_empty() && *s != "." && *s != "?")?;
    let share = pieces.next().filter(|s| !s.is_empty())?;
    if [server, share]
        .iter()
        .any(|p| p.contains([':', '*', '?', '"', '<', '>', '|']) || p.ends_with(['.', ' ']))
    {
        return None;
    }
    let rest = pieces.next().unwrap_or_default();
    Some((
        format!(r"\\{}\{}", server.to_lowercase(), share.to_lowercase()),
        rest.to_string(),
    ))
}

/// Where a path really leads, in one form. The part that exists is
/// resolved by the operating system; the rest is added as written.
fn resolve(path: &Path) -> Option<Spelled> {
    let text = path.to_str()?;
    // Judged as written first: this refuses `..`, relative paths and odd
    // spellings before anything on disk is looked at.
    spelled(text, Style::NATIVE)?;
    if path.components().any(|c| c == Component::ParentDir) {
        return None;
    }
    let mut missing: Vec<&std::ffi::OsStr> = Vec::new();
    let mut here = path;
    let real: PathBuf = loop {
        match std::fs::symlink_metadata(here) {
            Ok(_) => match std::fs::canonicalize(here) {
                Ok(real) => break real,
                // There, but cannot be resolved: a link to nowhere, or a
                // folder that may not be read. Refused rather than guessed.
                Err(_) => return None,
            },
            Err(_) => {
                missing.push(here.file_name()?);
                here = here.parent()?;
            }
        }
    };
    let whole = missing.iter().rev().fold(real, |path, part| path.join(part));
    spelled(whole.to_str()?, Style::NATIVE)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn win(text: &str) -> Option<Spelled> {
        spelled(text, Style::Windows)
    }

    fn win_inside(path: &str, folder: &str) -> bool {
        match (win(path), win(folder)) {
            (Some(path), Some(folder)) => path.is_inside(&folder),
            _ => false,
        }
    }

    fn allowing(folder: &Path) -> AgentAccess {
        AgentAccess {
            look_in: vec![folder.display().to_string()],
            back_up_to: vec![folder.display().to_string()],
            may_change: false,
        }
    }

    #[test]
    fn an_agent_is_allowed_nothing_and_may_only_look_until_a_person_says() {
        let none = AgentAccess::default();
        assert!(!none.may_change);
        assert!(!none.may_look_in(Path::new("/")));
        assert!(!none.may_back_up_to(&std::env::temp_dir()));
        assert_eq!(
            serde_json::to_value(&none).unwrap(),
            serde_json::json!({ "lookIn": [], "backUpTo": [], "mayChange": false })
        );
    }

    #[test]
    fn a_folder_and_what_is_inside_it_are_allowed_and_nothing_else() {
        let dir = tempfile::tempdir().unwrap();
        let photos = dir.path().join("Photos");
        std::fs::create_dir_all(photos.join("2004")).unwrap();
        std::fs::create_dir_all(dir.path().join("Photos-private")).unwrap();
        let access = allowing(&photos);

        assert!(access.may_look_in(&photos));
        assert!(access.may_look_in(&photos.join("2004")));
        assert!(
            access.may_back_up_to(&photos.join("new folder").join("deeper")),
            "a folder not made yet is judged as written"
        );
        assert!(!access.may_look_in(dir.path()), "not the folder above");
        assert!(
            !access.may_look_in(&dir.path().join("Photos-private")),
            "a name that only starts the same is another folder"
        );
        assert!(!access.may_look_in(Path::new("/")));
    }

    #[test]
    fn climbing_out_with_dot_dot_is_refused_even_back_into_the_folder() {
        let dir = tempfile::tempdir().unwrap();
        let photos = dir.path().join("Photos");
        std::fs::create_dir_all(photos.join("2004")).unwrap();
        let access = allowing(&photos);
        let attempts = [
            photos.join(".."),
            photos.join("..").join("secret"),
            photos.join("2004").join("..").join("..").join("secret"),
            photos.join("not made").join("..").join("..").join("secret"),
            // Lands back inside, but there is never a need for it.
            photos.join("2004").join("..").join("2004"),
        ];
        for path in attempts {
            assert!(!access.may_look_in(&path), "{}", path.display());
            assert!(!access.may_back_up_to(&path), "{}", path.display());
        }
        assert!(
            !allowing(&photos.join("..")).may_look_in(dir.path()),
            "an allowed folder with .. in it allows nothing"
        );
    }

    #[test]
    fn relative_empty_and_odd_paths_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let access = allowing(dir.path());
        for path in ["", ".", "Photos", "./Photos"] {
            assert!(!access.may_look_in(Path::new(path)), "{path:?}");
        }
        let relative = AgentAccess {
            look_in: vec!["Photos".into(), String::new()],
            ..AgentAccess::default()
        };
        assert!(!relative.may_look_in(&std::env::current_dir().unwrap().join("Photos")));
    }

    #[test]
    fn separators_written_twice_or_at_the_end_change_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let photos = dir.path().join("Photos");
        std::fs::create_dir_all(&photos).unwrap();
        let written = format!("{}{}", photos.display(), std::path::MAIN_SEPARATOR);
        let access = AgentAccess {
            look_in: vec![written.clone()],
            ..AgentAccess::default()
        };
        assert!(access.may_look_in(&photos));
        assert!(access.may_look_in(Path::new(&format!("{written}{}2004", std::path::MAIN_SEPARATOR))));
    }

    #[cfg(unix)]
    #[test]
    fn a_link_is_judged_by_where_it_leads() {
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().unwrap();
        let photos = dir.path().join("Photos");
        let secret = dir.path().join("Secret");
        std::fs::create_dir_all(&photos).unwrap();
        std::fs::create_dir_all(&secret).unwrap();
        let access = allowing(&photos);

        // A link inside the allowed folder that leads out of it.
        symlink(&secret, photos.join("way out")).unwrap();
        assert!(!access.may_look_in(&photos.join("way out")));
        assert!(!access.may_look_in(&photos.join("way out").join("diary.txt")));
        assert!(!access.may_back_up_to(&photos.join("way out").join("not made yet")));

        // A link to a file outside.
        std::fs::write(secret.join("diary.txt"), "private").unwrap();
        symlink(secret.join("diary.txt"), photos.join("diary.txt")).unwrap();
        assert!(!access.may_look_in(&photos.join("diary.txt")));

        // A link to nowhere: where it would lead cannot be known yet.
        symlink(dir.path().join("Not yet"), photos.join("dangling")).unwrap();
        assert!(!access.may_back_up_to(&photos.join("dangling")));
        assert!(!access.may_back_up_to(&photos.join("dangling").join("x")));

        // A link from outside that leads in is inside.
        symlink(&photos, dir.path().join("shortcut")).unwrap();
        assert!(access.may_look_in(&dir.path().join("shortcut")));

        // And an allowed folder written through a link allows where it
        // leads, and nothing else.
        let through_a_link = allowing(&dir.path().join("shortcut"));
        assert!(through_a_link.may_look_in(&photos));
        assert!(!through_a_link.may_look_in(&secret));
    }

    #[test]
    fn windows_spellings_of_one_path_are_one_path() {
        let same = [
            r"C:\Users\Mark\Pictures",
            r"c:\users\mark\pictures",
            r"C:/Users/Mark/Pictures",
            r"C:\Users/Mark\Pictures\",
            r"C:\\Users\\Mark\\Pictures\\",
            r"C:\Users\.\Mark\Pictures",
            r"\\?\C:\Users\Mark\Pictures",
            r"\\?\c:\USERS\Mark\Pictures\",
        ];
        for spelling in same {
            assert_eq!(win(spelling), win(same[0]), "{spelling}");
        }
        assert_eq!(win(r"\\NAS\Photos\2004"), win(r"\\?\UNC\nas\photos\2004"));
        assert_eq!(win("//nas/photos/2004"), win(r"\\nas\Photos\2004"));
        assert!(win_inside(
            r"\\?\C:\Users\Mark\Pictures\2004\x.jpg",
            r"c:/users/mark/pictures/"
        ));
        assert!(win_inside(r"D:\", r"D:\"));
        assert!(win_inside(r"D:\DCIM", r"d:"));
    }

    #[test]
    fn windows_spellings_that_dodge_are_refused() {
        let photos = r"C:\Users\Mark\Pictures";
        let dodges = [
            // Climbing out, in every spelling.
            r"C:\Users\Mark\Pictures\..\Documents",
            r"C:/Users/Mark/Pictures/../Documents",
            r"\\?\C:\Users\Mark\Pictures\..\Documents",
            r"\\?\C:\Users\Mark\Pictures\.\x",
            // A name that only starts the same.
            r"C:\Users\Mark\Pictures-private",
            r"C:\Users\Mark\Pictures private\x",
            // Windows drops a trailing dot or space, and `:` is a stream.
            r"C:\Users\Mark\Pictures.\..\Documents",
            r"C:\Users\Mark\Pictures \x",
            r"C:\Users\Mark\Pictures\x.",
            r"C:\Users\Mark\Pictures:hidden",
            r"C:\Users\Mark\Pictures\x.jpg:stream",
            // Devices, other namespaces and a verbatim path with slashes.
            r"\\.\C:\Users\Mark\Pictures",
            r"\\?\GLOBALROOT\Device\HarddiskVolume1\Users\Mark\Pictures",
            r"\\?\Volume{1b3b1146-4076-11e1-84aa-806e6f6e6963}\Users\Mark\Pictures",
            r"\??\C:\Users\Mark\Pictures",
            r"//?/C:/Users/Mark/Pictures",
            r"\\?\C:\Users/Mark/Pictures",
            // Relative to something that cannot be known.
            r"C:Users\Mark\Pictures",
            r"\Users\Mark\Pictures",
            r"Users\Mark\Pictures",
            r"Pictures",
            "",
            // Another drive, or another server's share of the same name.
            r"D:\Users\Mark\Pictures",
            r"\\nas\c$\Users\Mark\Pictures",
        ];
        for dodge in dodges {
            assert!(!win_inside(dodge, photos), "{dodge} was let in");
        }
        assert!(!win_inside(r"\\nas\other\x", r"\\nas\photos"));
        assert!(!win_inside(r"\\other\photos", r"\\nas\photos"));
        // A server path cannot pass for a drive or the other way round.
        assert!(!win_inside(r"\\c:\Users", r"C:\"));
    }

    #[test]
    fn unix_spellings_that_dodge_are_refused() {
        let unix = |text| spelled(text, Style::Unix);
        assert_eq!(unix("/Volumes//Photos/./2004/"), unix("/Volumes/Photos/2004"));
        assert_eq!(unix("/Volumes/Photos/../x"), None);
        assert_eq!(unix("Volumes/Photos"), None);
        assert_ne!(
            unix("/volumes/photos"),
            unix("/Volumes/Photos"),
            "different case is refused, never assumed the same"
        );
        assert_ne!(
            unix(r"/Volumes/Photos\..\x"),
            None,
            "a backslash is part of a name here, not a way out"
        );
        assert!(!unix(r"/Volumes/Photos\..\x")
            .unwrap()
            .is_inside(&unix("/Volumes/Photos/x").unwrap()));
    }

    #[test]
    fn a_person_sets_the_folders_and_whether_changes_are_allowed() {
        let dir = tempfile::tempdir().unwrap();
        let photos = dir.path().join("Photos").display().to_string();
        let mut access = AgentAccess::default();
        access
            .apply(AgentsAccessRequest {
                look_in: Some(vec![format!(" {photos} "), photos.clone()]),
                back_up_to: None,
                may_change: Some(true),
            })
            .unwrap();
        assert_eq!(access.look_in, std::slice::from_ref(&photos), "trimmed, and once only");
        assert!(access.back_up_to.is_empty());
        assert!(access.may_change);

        // Only what is given changes.
        access
            .apply(AgentsAccessRequest {
                may_change: Some(false),
                ..AgentsAccessRequest::default()
            })
            .unwrap();
        assert_eq!(access.look_in, std::slice::from_ref(&photos));

        let before = access.clone();
        let e = access
            .apply(AgentsAccessRequest {
                look_in: Some(vec!["Photos".into()]),
                back_up_to: Some(vec![photos]),
                ..AgentsAccessRequest::default()
            })
            .unwrap_err();
        assert_eq!(e.kind, crate::Kind::Refused);
        assert!(
            e.message.starts_with("\"Photos\" isn't a folder Keevoro can allow."),
            "{e}"
        );
        assert!(!e.message.contains('\u{2014}'));
        assert_eq!(access, before, "a refusal changes nothing");
    }
}
