//! A backup that runs by itself, with nobody watching: what it holds back
//! for the person's say, and why it started.
//!
//! A person watching a backup hears what happens and can answer. Nobody is
//! watching one that runs on a schedule or for a system scheduler, so
//! anything that needs an answer is held back and said, never settled
//! silently. Each rule here is pure; the run gathers the facts.

use serde::{Deserialize, Serialize};

/// What reading from a bucket source may cost before a run by itself holds
/// back, in pounds: about £1 or more waits for the person's say.
pub const UNATTENDED_COST_LIMIT: f64 = 1.0;

/// How long a backup by itself trusts looking through a source quickly
/// (taking a file whose size and modified time have not changed as it was
/// found, unopened) before it reads every file again, so a file changed
/// without its size or time changing is still found.
pub const FULL_LOOK_EVERY: i64 = 30 * 86_400;

/// Whether a backup by itself may look through a source quickly at `now`,
/// given when every file on it was last read (seconds since 1970). Never
/// read, or read at a time more than a day ahead (the clock was wrong),
/// means read every file now.
pub fn look_quickly(last_full_look: Option<i64>, now: i64) -> bool {
    last_full_look.is_some_and(|at| at <= now + 86_400 && now - at < FULL_LOOK_EVERY)
}

/// Why a backup ran by itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Occasion {
    /// Its schedule came round.
    Schedule,
    /// Its schedule came round while the computer was off or asleep, and it
    /// caught up once.
    CatchUp,
    /// The person pressed Run now on its schedule.
    RunNow,
    /// The command line, for a system scheduler or a script.
    CommandLine,
}

impl Occasion {
    /// In a few words, after "Started by itself": "on its schedule".
    pub fn words(self) -> &'static str {
        match self {
            Occasion::Schedule => "on its schedule",
            Occasion::CatchUp => "to catch up a run that was missed",
            Occasion::RunNow => "when you pressed Run now",
            Occasion::CommandLine => "for a system scheduler or a script",
        }
    }

    /// True when the person asked for it there and then.
    pub fn person_asked(self) -> bool {
        matches!(self, Occasion::RunNow)
    }
}

/// Where in Keevoro the person answers something that waits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Answer {
    /// How to arrange the backup.
    Arrange,
    /// The pictures set aside as probably not photos.
    SetAside,
    /// The copies found.
    Duplicates,
    /// The sources, for a different drive at a source's place.
    Sources,
    /// The backup screen, for what reading from cloud storage costs, or a
    /// destination that looks emptied.
    BackUp,
}

/// One thing that waits for the person's say.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Waiting {
    /// What is said.
    pub said: String,
    /// True when nothing is sent in this run until it is answered.
    pub holds_the_run: bool,
    pub answer: Answer,
}

/// What a run by itself knows before it sends anything.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Facts {
    /// The backup's name, as the person called it.
    pub backup: String,
    /// False when the person was never asked how to arrange it.
    pub arrange_asked: bool,
    /// New pictures set aside as probably not photos.
    pub new_set_aside: usize,
    /// New files that are copies of others.
    pub new_copies: usize,
    /// Sources whose place holds a different drive now, by name.
    pub different_drives: Vec<String>,
    /// What reading from bucket sources would cost, in pounds, and that
    /// in words.
    pub cost: Option<(f64, String)>,
}

/// What waits for the person's say. Pictures set aside are never sent
/// until rescued, and copies are sent once, as for any backup; both are
/// listed. How to arrange the backup, never asked, and reading from cloud
/// storage that costs about [`UNATTENDED_COST_LIMIT`] or more, hold the
/// whole run back.
pub fn what_waits(f: &Facts) -> Vec<Waiting> {
    let mut waiting = Vec::new();
    if !f.arrange_asked {
        waiting.push(Waiting {
            said: format!(
                "Keevoro needs one answer before it can back up \"{}\" by itself: how to arrange it.",
                f.backup
            ),
            holds_the_run: true,
            answer: Answer::Arrange,
        });
    }
    if let Some((pounds, said)) = &f.cost {
        if *pounds >= UNATTENDED_COST_LIMIT {
            waiting.push(Waiting {
                said: format!("Nothing was sent, as reading from cloud storage costs money. {said} Back up from Keevoro to go ahead."),
                holds_the_run: true,
                answer: Answer::BackUp,
            });
        }
    }
    if f.new_set_aside > 0 {
        waiting.push(Waiting {
            said: format!(
                "{} set aside as probably not photos, and not backed up. Look at them in Keevoro to keep any.",
                counted(f.new_set_aside, "new picture was", "new pictures were")
            ),
            holds_the_run: false,
            answer: Answer::SetAside,
        });
    }
    if f.new_copies > 0 {
        waiting.push(Waiting {
            said: format!(
                "{} copies of other files. One copy of each was backed up; review them in Keevoro.",
                counted(f.new_copies, "new file is one of the", "new files are")
            ),
            holds_the_run: false,
            answer: Answer::Duplicates,
        });
    }
    for name in &f.different_drives {
        waiting.push(Waiting {
            said: format!(
                "A different drive is plugged in where {name} was, so {name} was not looked through. Open Keevoro to choose what to do."
            ),
            holds_the_run: false,
            answer: Answer::Sources,
        });
    }
    waiting
}

/// "1 new picture was", "4 new pictures were".
fn counted(n: usize, one: &str, many: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{} {many}", crate::words::thousands(n))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_source_is_looked_through_quickly_until_a_month_after_every_file_was_last_read() {
        let now = 1_763_700_000;
        assert!(!look_quickly(None, now), "never read in full: read now");
        assert!(look_quickly(Some(now - 86_400), now));
        assert!(look_quickly(Some(now - FULL_LOOK_EVERY + 1), now));
        assert!(!look_quickly(Some(now - FULL_LOOK_EVERY), now));
        assert!(
            look_quickly(Some(now + 3_600), now),
            "a little ahead is the clock's wobble"
        );
        assert!(
            !look_quickly(Some(now + 2 * 86_400), now),
            "far ahead: the clock was wrong"
        );
    }

    fn family() -> Facts {
        Facts {
            backup: "Family".into(),
            arrange_asked: true,
            ..Facts::default()
        }
    }

    #[test]
    fn nothing_waits_when_nothing_needs_a_say() {
        assert!(what_waits(&family()).is_empty());
    }

    #[test]
    fn a_backup_never_asked_how_to_arrange_it_is_held_back() {
        let waits = what_waits(&Facts {
            arrange_asked: false,
            ..family()
        });
        assert_eq!(waits.len(), 1);
        assert!(waits[0].holds_the_run);
        assert_eq!(waits[0].answer, Answer::Arrange);
        assert_eq!(
            waits[0].said,
            "Keevoro needs one answer before it can back up \"Family\" by itself: how to arrange it."
        );
    }

    #[test]
    fn reading_from_cloud_storage_that_costs_about_a_pound_or_more_is_held_back() {
        let cheap = what_waits(&Facts {
            cost: Some((0.99, "Reading 12 GB out of Amazon S3 costs under £1.".into())),
            ..family()
        });
        assert!(cheap.is_empty());
        let dear = what_waits(&Facts {
            cost: Some((
                UNATTENDED_COST_LIMIT,
                "Reading 20 GB out of Amazon S3 costs about £1.".into(),
            )),
            ..family()
        });
        assert_eq!(dear.len(), 1);
        assert!(dear[0].holds_the_run);
        assert!(dear[0].said.contains("costs about £1."), "{}", dear[0].said);
    }

    #[test]
    fn set_aside_pictures_copies_and_another_drive_are_listed_and_hold_nothing_back() {
        let waits = what_waits(&Facts {
            new_set_aside: 4,
            new_copies: 1,
            different_drives: vec!["Card".into()],
            ..family()
        });
        let said: Vec<&str> = waits.iter().map(|w| w.said.as_str()).collect();
        assert_eq!(
            said,
            [
                "4 new pictures were set aside as probably not photos, and not backed up. Look at them in Keevoro to keep any.",
                "1 new file is one of the copies of other files. One copy of each was backed up; review them in Keevoro.",
                "A different drive is plugged in where Card was, so Card was not looked through. Open Keevoro to choose what to do.",
            ]
        );
        assert!(waits.iter().all(|w| !w.holds_the_run));
    }

    #[test]
    fn only_run_now_counts_as_the_person_asking() {
        assert!(Occasion::RunNow.person_asked());
        for o in [Occasion::Schedule, Occasion::CatchUp, Occasion::CommandLine] {
            assert!(!o.person_asked());
        }
        assert_eq!(Occasion::Schedule.words(), "on its schedule");
    }
}
