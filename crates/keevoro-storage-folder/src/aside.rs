//! Moving a damaged copy aside, inside the backup's own folder.
//!
//! Keevoro never replaces or deletes a file in a backup. When a copy there
//! is short, the wrong size or not the same as its original, and the person
//! asks Keevoro to put the backup right, the copy is moved (given another
//! name, never copied and deleted) into `Set aside by Keevoro/<date>/`,
//! keeping its path beneath, with a note saying what it is. Its real name is
//! then free for a good copy. Nothing is ever moved over a file that is
//! already there.

use keevoro_core::{SET_ASIDE_FOLDER, SET_ASIDE_NOTE};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

/// What the note beside the copies set aside says.
pub fn note_words(date: &str) -> String {
    format!(
        "These are copies Keevoro found in this backup that were not the same as the files they were backed up \
         from: shorter, another size, or with other contents. They are most likely damaged, for example by the \
         connection to this folder dropping while they were being written.\n\n\
         Keevoro never deletes or replaces anything in a backup, so when you asked it to put this backup right \
         (on {date}) it moved them here, kept in the folders they were in, and sent good copies of the originals \
         again in their place. Each good copy was read back and checked against its original.\n\n\
         Nothing here is counted as backed up, listed in the backup's index, or looked through as a source.\n\n\
         You can delete this folder once you are happy that your backup is as it should be.\n"
    )
}

/// The folder, relative to the backup's root, that copies set aside on
/// `date` go in.
pub fn set_aside_folder(date: &str) -> String {
    format!("{SET_ASIDE_FOLDER}/{date}")
}

/// Moves the copy at `key` (a backup's own path, such as
/// `originals/2016/IMG_6132.JPG`) under `root` into the set-aside folder for
/// `date`, keeping its path beneath, and answers where it went, relative to
/// `root`. A name already taken there is never moved over: the copy is
/// given a number, `IMG_6132 (2).JPG`. A note is left in the folder the
/// first time. If anything fails, the copy is left exactly where it was.
pub fn set_aside(root: &Path, key: &str, date: &str) -> std::io::Result<String> {
    let parts: Vec<&str> = key.split('/').filter(|p| !p.is_empty()).collect();
    if parts.is_empty() || parts.iter().any(|p| *p == "." || *p == "..") {
        return Err(std::io::Error::new(ErrorKind::InvalidInput, "not a place in a backup"));
    }
    let from = parts.iter().fold(root.to_path_buf(), |p, s| p.join(s));
    if !std::fs::symlink_metadata(&from)?.is_file() {
        return Err(std::io::Error::new(ErrorKind::InvalidInput, "not a file"));
    }
    let folder = set_aside_folder(date);
    let base = folder.split('/').fold(root.to_path_buf(), |p, s| p.join(s));
    leave_notes(root, &base, date)?;
    let (name, folders) = parts.split_last().ok_or_else(|| std::io::Error::other("no name"))?;
    let within = folders.iter().fold(base.clone(), |p, s| p.join(s));
    std::fs::create_dir_all(&within)?;
    for n in 1..=1000u32 {
        let candidate = numbered(name, n);
        let to = within.join(&candidate);
        match move_without_replacing(&from, &to) {
            Ok(()) => {
                let mut relative = vec![folder.clone()];
                relative.extend(folders.iter().map(|s| (*s).to_string()));
                relative.push(candidate);
                return Ok(relative.join("/"));
            }
            Err(e) if e.kind() == ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(std::io::Error::new(
        ErrorKind::AlreadyExists,
        "too many copies set aside under this name",
    ))
}

/// `IMG_6132.JPG`, then `IMG_6132 (2).JPG`, `IMG_6132 (3).JPG`.
fn numbered(name: &str, n: u32) -> String {
    if n == 1 {
        return name.to_string();
    }
    match name.rfind('.') {
        Some(dot) if dot > 0 => format!("{} ({n}){}", &name[..dot], &name[dot..]),
        _ => format!("{name} ({n})"),
    }
}

/// Gives `from` the name `to`, refused if `to` is taken. A hard link then
/// the old name let go is a move that can never replace anything; where
/// there are no hard links (FAT drives, some shares), it is checked first
/// and then renamed.
fn move_without_replacing(from: &Path, to: &Path) -> std::io::Result<()> {
    match std::fs::hard_link(from, to) {
        Ok(()) => {
            if let Err(e) = std::fs::remove_file(from) {
                // Undo: the second name goes, and the copy stays exactly
                // where it was, under its first.
                let _ = std::fs::remove_file(to);
                return Err(e);
            }
            Ok(())
        }
        Err(e) if e.kind() == ErrorKind::AlreadyExists => Err(e),
        Err(_) => {
            if to.try_exists()? {
                return Err(std::io::Error::from(ErrorKind::AlreadyExists));
            }
            std::fs::rename(from, to)
        }
    }
}

/// The note, in the set-aside folder and in the folder for this date, made
/// once and never replaced.
fn leave_notes(root: &Path, dated: &Path, date: &str) -> std::io::Result<()> {
    for folder in [root.join(SET_ASIDE_FOLDER), dated.to_path_buf()] {
        std::fs::create_dir_all(&folder)?;
        let note: PathBuf = folder.join(SET_ASIDE_NOTE);
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&note) {
            Ok(mut file) => {
                use std::io::Write;
                file.write_all(note_words(date).as_bytes())?;
                file.sync_all()?;
            }
            Err(e) if e.kind() == ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_damaged_copy_is_moved_aside_keeping_its_path_and_never_over_another() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let key = "originals/By date/2016/2016-12/IMG_6132.JPG";
        let at = root.join("originals/By date/2016/2016-12");
        std::fs::create_dir_all(&at).unwrap();
        std::fs::write(at.join("IMG_6132.JPG"), b"short").unwrap();
        let went = set_aside(root, key, "2026-11-22").unwrap();
        assert_eq!(
            went,
            "Set aside by Keevoro/2026-11-22/originals/By date/2016/2016-12/IMG_6132.JPG"
        );
        assert!(!at.join("IMG_6132.JPG").exists(), "its real name is free");
        assert_eq!(std::fs::read(root.join(&went)).unwrap(), b"short", "moved, not lost");
        let note = std::fs::read_to_string(root.join(SET_ASIDE_FOLDER).join(SET_ASIDE_NOTE)).unwrap();
        assert!(note.contains("You can delete this folder once you are happy"), "{note}");
        assert!(root
            .join("Set aside by Keevoro/2026-11-22")
            .join(SET_ASIDE_NOTE)
            .is_file());

        // The same name set aside again the same day is numbered, never
        // moved over the first.
        std::fs::write(at.join("IMG_6132.JPG"), b"short again").unwrap();
        let again = set_aside(root, key, "2026-11-22").unwrap();
        assert!(again.ends_with("IMG_6132 (2).JPG"), "{again}");
        assert_eq!(std::fs::read(root.join(&went)).unwrap(), b"short");
        assert_eq!(std::fs::read(root.join(&again)).unwrap(), b"short again");
        // A note already there is kept as it is.
        std::fs::write(root.join(SET_ASIDE_FOLDER).join(SET_ASIDE_NOTE), b"mine").unwrap();
        std::fs::write(at.join("IMG_6132.JPG"), b"x").unwrap();
        set_aside(root, key, "2026-11-22").unwrap();
        assert_eq!(
            std::fs::read(root.join(SET_ASIDE_FOLDER).join(SET_ASIDE_NOTE)).unwrap(),
            b"mine"
        );
        // The folder it goes in is known for Keevoro's own.
        assert_eq!(
            keevoro_core::folder_made_by_keevoro(&root.join(SET_ASIDE_FOLDER)),
            Some(keevoro_core::SkipReason::KeevoroOwn)
        );
    }

    #[test]
    fn nothing_is_moved_that_is_not_a_file_in_the_backup() {
        let dir = tempfile::tempdir().unwrap();
        assert!(set_aside(dir.path(), "originals/../x.jpg", "2026-11-22").is_err());
        assert!(set_aside(dir.path(), "originals/not there.jpg", "2026-11-22").is_err());
        std::fs::create_dir_all(dir.path().join("originals/a folder")).unwrap();
        assert!(set_aside(dir.path(), "originals/a folder", "2026-11-22").is_err());
        assert!(dir.path().join("originals/a folder").is_dir());
    }
}
