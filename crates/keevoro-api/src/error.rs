//! What goes wrong, as a sentence a person can act on.
//!
//! Every front end shows the same words: the app on a screen, the command
//! line in a terminal, an agent to the person it is helping. So the error
//! carries the sentence itself, exactly as the engine wrote it, and a kind
//! a program can act on without reading the words.

use crate::register::Op;
use serde::Serialize;

/// What sort of problem it was, for a program to act on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// The operation was tried and could not be done.
    Failed,
    /// It needs a backup, and none was given.
    NoBackup,
    /// The backup, or the thing in it, that was named is not there.
    NotFound,
    /// It was understood but refused, such as a name already in use or a
    /// destination without the room.
    Refused,
    /// Something else is already being done to that backup, here or in
    /// another of Keevoro's programs. Let it finish, or stop it, first.
    Busy,
    /// The place asked for could not be reached: a drive unplugged, a
    /// connection down, keys that are wrong.
    Unreachable,
    /// The request could not be understood.
    NotUnderstood,
    /// The API cannot do this yet; only the app can, for now.
    NotYet,
}

/// One problem, in plain English.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Error {
    pub kind: Kind,
    /// The sentence to show, exactly as it is shown today.
    pub message: String,
}

impl Error {
    // Every operation in the register is done here now. This is what one
    // added to the register would answer until it is built.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn not_yet(op: Op) -> Self {
        Self {
            kind: Kind::NotYet,
            message: format!(
                "Keevoro can't do \"{}\" from here yet. For now it can only be done in the Keevoro app.",
                op.name()
            ),
        }
    }

    pub(crate) fn no_such_job(job: crate::jobs::JobId) -> Self {
        Self {
            kind: Kind::NotFound,
            message: format!("There is no job {} running.", job.0),
        }
    }

    pub(crate) fn no_backup() -> Self {
        Self {
            kind: Kind::NoBackup,
            message: "Choose a backup first.".to_string(),
        }
    }

    pub(crate) fn no_such_backup(name: &str) -> Self {
        Self {
            kind: Kind::NotFound,
            message: format!("There is no backup called \"{}\".", name.trim()),
        }
    }

    pub(crate) fn not_found(message: impl Into<String>) -> Self {
        Self {
            kind: Kind::NotFound,
            message: message.into(),
        }
    }

    pub(crate) fn refused(message: impl Into<String>) -> Self {
        Self {
            kind: Kind::Refused,
            message: message.into(),
        }
    }

    pub(crate) fn busy(message: impl Into<String>) -> Self {
        Self {
            kind: Kind::Busy,
            message: message.into(),
        }
    }

    pub(crate) fn unreachable(message: String) -> Self {
        Self {
            kind: Kind::Unreachable,
            message,
        }
    }

    pub(crate) fn not_understood(why: impl std::fmt::Display) -> Self {
        Self {
            kind: Kind::NotUnderstood,
            message: format!("Keevoro didn't understand that request: {why}."),
        }
    }

    pub(crate) fn failed(message: String) -> Self {
        Self {
            kind: Kind::Failed,
            message,
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Error {}

// The `#` form is what the app has always shown: the sentence, followed by
// whatever caused it.
impl From<anyhow::Error> for Error {
    fn from(e: anyhow::Error) -> Self {
        Self::failed(format!("{e:#}"))
    }
}

impl From<keevoro_catalog::CatalogError> for Error {
    fn from(e: keevoro_catalog::CatalogError) -> Self {
        Self::failed(format!("{e:#}"))
    }
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Self::failed(format!("Keevoro couldn't write down its answer: {e}."))
    }
}

/// What every operation gives back.
pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sentence_is_the_engines_own_with_its_cause() {
        let e = anyhow::anyhow!("disk full").context("Couldn't save what was found");
        let ours = Error::from(e);
        assert_eq!(ours.kind, Kind::Failed);
        assert_eq!(ours.to_string(), "Couldn't save what was found: disk full");
        // The app formats errors with `#`; that must say the same.
        assert_eq!(format!("{ours:#}"), ours.to_string());
    }

    #[test]
    fn problems_of_our_own_are_plain_and_have_no_em_dash() {
        for e in [
            Error::not_yet(Op::BackupRun),
            Error::no_backup(),
            Error::not_understood("missing field `source`"),
            Error::no_such_backup("Holidays"),
        ] {
            assert!(!e.message.contains('\u{2014}'), "{}", e.message);
            assert!(e.message.ends_with('.'), "{}", e.message);
        }
        assert!(Error::not_yet(Op::BackupRun).message.contains("backup.run"));
    }
}
