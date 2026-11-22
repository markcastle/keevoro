//! What went wrong with a file, said truthfully and plainly, and when to
//! stop trying a place that seems to have gone.
//!
//! The owner's backup to a network share showed "Couldn't read it from its
//! source (The media is write protected. (os error 19))" and "(This user
//! can't sign in because this account is currently disabled. (os error
//! 1331))" for 176 files in a row. Nothing was wrong with the source, the
//! media or the account: the connection to the share had dropped for a few
//! minutes, and Windows says so in those words when a share's session goes
//! part-way through. So:
//!
//! - **A side.** Every problem is said as the source's or the
//!   destination's, as it was ([`Side`]).
//! - **One table.** Operating-system error codes, for Windows, Linux and a
//!   Mac, become one of a few causes ([`cause_of_code`]). It is pure, and its
//!   tests run every system's codes on every computer. On a network path, a
//!   code that would otherwise mean "write protected" or "account disabled"
//!   during a run that was working means the connection was lost.
//! - **Grouped.** Files that failed for one cause at one place are said in
//!   one sentence with a count and the first few names ([`group_words`]),
//!   never hundreds of lines.
//! - **When to stop trying a place.** [`place_seems_gone`] looks at recent
//!   outcomes only, so a healthy run does no extra work.

use serde::{Deserialize, Serialize};

/// Which side a problem was on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Side {
    /// Where the file was found.
    Source,
    /// Where it was going.
    Destination,
}

/// Which operating system an error code came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum System {
    Windows,
    Linux,
    Mac,
}

impl System {
    /// The system this program runs on. (Other Unix systems are read as
    /// Linux, whose codes most of them share for these.)
    pub fn this() -> Self {
        if cfg!(windows) {
            System::Windows
        } else if cfg!(target_os = "macos") {
            System::Mac
        } else {
            System::Linux
        }
    }
}

/// Why a file could not be backed up, in a few kinds a person can act on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Cause {
    /// The connection to the place was lost, or the drive went away.
    ConnectionLost,
    NoRoom,
    NotAllowed,
    /// The file, or the place, was not there.
    NotFound,
    /// Another program had it open.
    InUse,
    WriteProtected,
    /// The place refused the account Keevoro connects with.
    AccountRefused,
    NameTooLong,
    /// The original changed after Keevoro looked at it.
    ChangedSinceLooked,
    /// The copy did not read back the same.
    NotTheSame,
    /// A different file is already there under its name.
    DifferentFileThere,
    Other,
}

impl Cause {
    /// May trying once more, later in the same run, work? A connection that
    /// came back, a program that let go of a file, or a copy written while
    /// the connection wavered.
    pub fn may_pass(self) -> bool {
        matches!(
            self,
            Cause::ConnectionLost | Cause::InUse | Cause::NotTheSame | Cause::Other
        )
    }
}

/// What an operating-system error code means, as one of the causes.
/// `across_a_network` is true when the place is a network share or cloud
/// storage; `was_working` when files had been sent or found there earlier
/// in the same run. Pure, so every system's codes are tested everywhere.
pub fn cause_of_code(system: System, code: i32, across_a_network: bool, was_working: bool) -> Cause {
    // Codes that, on a share that was working a moment ago, are what a lost
    // session looks like, whatever they say.
    let lost_if_working = |otherwise: Cause| {
        if across_a_network && was_working {
            Cause::ConnectionLost
        } else {
            otherwise
        }
    };
    match system {
        System::Windows => match code {
            2 | 3 => Cause::NotFound,
            5 => Cause::NotAllowed,
            19 => lost_if_working(Cause::WriteProtected),
            1326 | 1327 | 1330 | 1331 => lost_if_working(Cause::AccountRefused),
            21 | 51 | 53 | 55 | 59 | 64 | 67 | 121 | 1167 | 1222 | 1231 | 1232 | 1236 | 2250 => Cause::ConnectionLost,
            32 | 33 => Cause::InUse,
            39 | 112 => Cause::NoRoom,
            206 => Cause::NameTooLong,
            _ => Cause::Other,
        },
        System::Linux => match code {
            2 => Cause::NotFound,
            1 | 13 => Cause::NotAllowed,
            30 => lost_if_working(Cause::WriteProtected),
            5 => lost_if_working(Cause::Other),
            6 | 19 | 100 | 101 | 103 | 104 | 107 | 110 | 112 | 113 | 116 => Cause::ConnectionLost,
            16 | 26 => Cause::InUse,
            28 | 122 => Cause::NoRoom,
            36 => Cause::NameTooLong,
            _ => Cause::Other,
        },
        System::Mac => match code {
            2 => Cause::NotFound,
            1 | 13 => Cause::NotAllowed,
            30 => lost_if_working(Cause::WriteProtected),
            5 => lost_if_working(Cause::Other),
            6 | 19 | 50 | 51 | 53 | 54 | 57 | 60 | 64 | 65 | 70 => Cause::ConnectionLost,
            16 | 26 => Cause::InUse,
            28 | 69 => Cause::NoRoom,
            63 => Cause::NameTooLong,
            _ => Cause::Other,
        },
    }
}

/// What happened to one file, for deciding whether a place has gone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Try {
    Worked,
    Failed(Cause),
}

/// How many failures in a row, every one of a kind that may pass, before
/// the place is asked whether it is still there.
pub const GIVE_UP_AFTER: usize = 8;

/// Has a place started failing every file, as one that has gone does? True
/// when the last [`GIVE_UP_AFTER`] tries, and no fewer, all failed for a
/// cause that may pass. A failure that would not pass (no room, not
/// allowed) is said for each file and never stops the run here, and one
/// file that works starts the count again. Pure: the caller then asks the
/// place, cheaply, whether it is there, and only if it is not stops sending
/// to it for now.
pub fn place_seems_gone(recent: &[Try]) -> bool {
    recent.len() >= GIVE_UP_AFTER
        && recent[recent.len() - GIVE_UP_AFTER..]
            .iter()
            .all(|t| matches!(t, Try::Failed(cause) if cause.may_pass()))
}

/// Why, as the end of a sentence about one file (`one`) or several.
/// `place` is the place in words ("the folder Y:\personal");
/// `across_a_network` says whether it is a share or cloud storage. Pure.
pub fn cause_words(cause: Cause, side: Side, place: &str, across_a_network: bool, one: bool) -> String {
    let source = side == Side::Source;
    let (it, its, it_was, it_is) = if one {
        ("it", "its name is", "it was", "it is")
    } else {
        ("them", "their names are", "they were", "they are")
    };
    let they = if one { "it" } else { "they" };
    match cause {
        Cause::ConnectionLost if source => format!("the place {it_was} found went away part-way"),
        Cause::ConnectionLost if across_a_network => format!("the connection to {place} was lost for a while"),
        Cause::ConnectionLost => format!("{place} went away part-way, as a drive that is unplugged does"),
        Cause::NoRoom => format!("there was no room left at {place}"),
        Cause::NotAllowed if source => format!("Keevoro wasn't allowed to read {it}"),
        Cause::NotAllowed => format!("{place} did not allow Keevoro to write there"),
        Cause::NotFound if source => format!("{it_was} no longer where {it_was} found"),
        Cause::NotFound => format!("a folder at {place} could not be found"),
        Cause::InUse => format!("another program had {it} open"),
        Cause::WriteProtected if source => format!("the place {it_was} found is set to read only"),
        Cause::WriteProtected => format!("{place} is set to read only"),
        Cause::AccountRefused => format!("{place} refused the account this computer connects to it with"),
        Cause::NameTooLong => format!("{its} too long for {place}"),
        Cause::ChangedSinceLooked => {
            format!("{they} changed after Keevoro looked at {it}, so {it_is} looked at again next time")
        }
        Cause::NotTheSame => "a copy did not read back the same, so it was not kept".to_string(),
        Cause::DifferentFileThere => {
            "a different file is already there under the same name, and Keevoro never replaces one".to_string()
        }
        Cause::Other => "of a problem Keevoro did not recognise".to_string(),
    }
}

/// How many of the first names are said before "and N more".
pub const NAMES_SAID: usize = 3;

/// One group of files that failed for one cause, in a sentence: how many,
/// why, the first few names, and how many more. `again` when every one of
/// them also failed last time. Pure.
pub fn group_words(names: &[String], why: &str, again: bool) -> String {
    let n = names.len();
    let lead = match (again, n) {
        (true, 1) => "This file could not be backed up again".to_string(),
        (true, _) => format!(
            "These {} files could not be backed up again",
            crate::words::thousands(n)
        ),
        (false, _) => format!("Couldn't back up {}", crate::words::count(n, "file")),
    };
    let shown: Vec<&str> = names
        .iter()
        .take(NAMES_SAID)
        .map(|k| k.rsplit('/').next().unwrap_or(k))
        .collect();
    let list = if n > NAMES_SAID {
        format!(
            "{} and {} more",
            shown.join(", "),
            crate::words::thousands(n - NAMES_SAID)
        )
    } else {
        match shown.split_last() {
            Some((last, [])) => (*last).to_string(),
            Some((last, rest)) => format!("{} and {last}", rest.join(", ")),
            None => String::new(),
        }
    };
    format!("{lead}, because {why}: {list}.")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_owners_codes_on_a_share_that_was_working_are_a_lost_connection_never_a_disabled_account() {
        for code in [19, 1331, 53, 59, 64, 67, 1231, 1326] {
            assert_eq!(
                cause_of_code(System::Windows, code, true, true),
                Cause::ConnectionLost,
                "{code}"
            );
        }
        // Said by a local disk, or at the very start, they mean what they say.
        assert_eq!(cause_of_code(System::Windows, 19, false, true), Cause::WriteProtected);
        assert_eq!(cause_of_code(System::Windows, 19, true, false), Cause::WriteProtected);
        assert_eq!(cause_of_code(System::Windows, 1331, true, false), Cause::AccountRefused);
        // A dropped connection is a dropped connection whenever it happens.
        assert_eq!(cause_of_code(System::Windows, 64, false, false), Cause::ConnectionLost);
    }

    #[test]
    fn each_systems_codes_mean_the_same_causes() {
        let table = [
            (System::Windows, 112, Cause::NoRoom),
            (System::Linux, 28, Cause::NoRoom),
            (System::Mac, 28, Cause::NoRoom),
            (System::Linux, 122, Cause::NoRoom),
            (System::Mac, 69, Cause::NoRoom),
            (System::Windows, 5, Cause::NotAllowed),
            (System::Linux, 13, Cause::NotAllowed),
            (System::Mac, 1, Cause::NotAllowed),
            (System::Windows, 2, Cause::NotFound),
            (System::Linux, 2, Cause::NotFound),
            (System::Windows, 32, Cause::InUse),
            (System::Linux, 16, Cause::InUse),
            (System::Windows, 206, Cause::NameTooLong),
            (System::Linux, 36, Cause::NameTooLong),
            (System::Mac, 63, Cause::NameTooLong),
            (System::Linux, 104, Cause::ConnectionLost),
            (System::Mac, 54, Cause::ConnectionLost),
            (System::Linux, 116, Cause::ConnectionLost),
            (System::Mac, 70, Cause::ConnectionLost),
            (System::Linux, 110, Cause::ConnectionLost),
            (System::Mac, 60, Cause::ConnectionLost),
            (System::Windows, 121, Cause::ConnectionLost),
            (System::Windows, 9999, Cause::Other),
            (System::Linux, 9999, Cause::Other),
        ];
        for (system, code, cause) in table {
            assert_eq!(cause_of_code(system, code, false, false), cause, "{system:?} {code}");
        }
        // A read-only file system, or a plain input/output error, on a share
        // that was working: the connection.
        assert_eq!(cause_of_code(System::Linux, 30, true, true), Cause::ConnectionLost);
        assert_eq!(cause_of_code(System::Mac, 5, true, true), Cause::ConnectionLost);
        assert_eq!(cause_of_code(System::Linux, 30, false, true), Cause::WriteProtected);
        assert_eq!(cause_of_code(System::Linux, 5, false, true), Cause::Other);
    }

    #[test]
    fn a_place_seems_gone_only_after_a_run_of_failures_that_may_pass() {
        use Try::*;
        let lost = Failed(Cause::ConnectionLost);
        assert!(!place_seems_gone(&[]));
        assert!(!place_seems_gone(&[lost; GIVE_UP_AFTER - 1]), "not yet");
        assert!(place_seems_gone(&[lost; GIVE_UP_AFTER]));
        let mut one_worked = vec![lost; GIVE_UP_AFTER - 1];
        one_worked.push(Worked);
        one_worked.extend([lost; GIVE_UP_AFTER - 1]);
        assert!(!place_seems_gone(&one_worked), "one that works starts the count again");
        one_worked.push(lost);
        assert!(place_seems_gone(&one_worked));
        // No room, or not allowed, is said for each file: trying the place
        // later would not help, and nor does stopping.
        assert!(!place_seems_gone(&[Failed(Cause::NoRoom); 50]));
        let mut mixed = vec![lost; GIVE_UP_AFTER];
        mixed[3] = Failed(Cause::NotAllowed);
        assert!(!place_seems_gone(&mixed));
        // A healthy run is never asked about.
        assert!(!place_seems_gone(&[Worked; 1000]));
    }

    #[test]
    fn files_are_said_in_groups_by_side_and_cause_with_the_first_few_names() {
        let names: Vec<String> = (6129..6305)
            .map(|n| format!("originals/By date/2016/2016-12/IMG_{n}.JPG"))
            .collect();
        let why = cause_words(
            Cause::ConnectionLost,
            Side::Destination,
            "the folder Y:\\personal",
            true,
            false,
        );
        assert_eq!(
            group_words(&names, &why, false),
            "Couldn't back up 176 files, because the connection to the folder Y:\\personal was lost for a while: \
             IMG_6129.JPG, IMG_6130.JPG, IMG_6131.JPG and 173 more."
        );
        assert_eq!(
            group_words(&names[..12], &why, true),
            "These 12 files could not be backed up again, because the connection to the folder Y:\\personal was \
             lost for a while: IMG_6129.JPG, IMG_6130.JPG, IMG_6131.JPG and 9 more."
        );
        assert_eq!(
            group_words(
                &names[..2],
                &cause_words(Cause::NotFound, Side::Source, "x", false, false),
                false
            ),
            "Couldn't back up 2 files, because they were no longer where they were found: IMG_6129.JPG and \
             IMG_6130.JPG."
        );
        assert_eq!(
            group_words(&names[..1], &why, true),
            "This file could not be backed up again, because the connection to the folder Y:\\personal was lost \
             for a while: IMG_6129.JPG."
        );
        assert_eq!(
            cause_words(Cause::NotFound, Side::Source, "x", false, true),
            "it was no longer where it was found"
        );
        // A destination's problem is never said as the source's.
        for cause in [
            Cause::ConnectionLost,
            Cause::NoRoom,
            Cause::NotAllowed,
            Cause::WriteProtected,
        ] {
            let said = cause_words(cause, Side::Destination, "the folder Y:\\personal", true, false);
            assert!(!said.contains("found"), "{said}");
            assert!(!said.contains('\u{2014}'), "{said}");
        }
    }
}
