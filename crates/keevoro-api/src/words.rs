//! Numbers said the way Keevoro says them everywhere: "1,204 photos",
//! "4.2 GB", "about 3 minutes". The app, the command line and an agent use
//! these so that the same thing reads the same in each.

pub use keevoro_core::words::photos_and_videos;
pub use keevoro_engine::history::duration;
pub use keevoro_engine::summary::{ago, bytes, count, files, thousands};

/// A search cut short by its drive going: what happened and what to do.
/// `drive` is what the drive is called when the place looked through is a
/// whole drive; otherwise the folder's name.
pub fn unplugged_while_looking(name: &str, drive: bool) -> String {
    let what = if drive {
        format!("{name} was unplugged.")
    } else {
        format!("{name} could no longer be reached; its drive may have been unplugged.")
    };
    format!("{what} What was found so far is kept; plug it in and look again to carry on.")
}

/// A source passed over when looking again, because it is not there: a
/// card in a drawer, a drive unplugged. `drive` is true when the source is
/// a whole drive, named as the computer named it when it was last seen.
pub fn not_plugged_in(name: &str, drive: bool) -> String {
    if drive {
        format!("{name} is not plugged in, so it was not looked through. What was found on it before is kept.")
    } else {
        format!(
            "{name} can't be found, so it was not looked through. Is its drive plugged in? What was found in it before is kept."
        )
    }
}

/// A source of a backup opened from its notes, passed over because it is
/// not connected on this computer yet.
pub fn not_connected_yet(name: &str) -> String {
    format!(
        "{name} is not connected on this computer yet, so it was not looked through. Connect it again to say where \
         these photos live here. What was found on it before is kept."
    )
}

/// Every source passed over, none of them there.
pub const NONE_PLUGGED_IN: &str =
    "None of this backup's sources are plugged in, so nothing was looked through. What was found on them before is kept.";

/// The question asked when the drive at a source's place is not the one
/// looked through there before.
pub fn different_drive_asked(name: &str) -> String {
    format!("This looks like a different drive from the one called {name}. Add it as a new source, or look through it as {name} anyway?")
}

/// The same, said when nobody is there to answer: the source was passed
/// over and nothing found on it is lost.
pub fn different_drive_said(name: &str, place: &str) -> String {
    format!(
        "A different drive is plugged in where {name} was ({place}), so {name} was not looked through. What was found on it before is kept."
    )
}

/// A source set aside because its own drive is back at its place, and the
/// source that took that place meanwhile is not.
pub fn set_aside_for(other: &str, name: &str, place: &str) -> String {
    format!(
        "{name} is plugged in at {place} again, so {other} was set aside. What was found on {other} is kept for when its drive is plugged in."
    )
}

/// The background check for duplicates, running: what it is doing, said
/// so that nothing reads as an instruction.
pub fn checking(done: usize, total: usize) -> String {
    format!(
        "Checking for duplicates in the background ({} of {}). You can keep using Keevoro while it finishes.",
        thousands(done),
        thousands(total)
    )
}

/// The background check for duplicates, paused on its own.
pub fn checking_paused(done: usize, total: usize) -> String {
    format!(
        "Checking for duplicates is paused ({} of {}). Everything else carries on.",
        thousands(done),
        thousands(total)
    )
}

/// Files a check for duplicates could not read because their source is not
/// plugged in (or was unplugged part-way): they wait for it. `drive` is
/// true when the source is a whole drive, named as the computer named it.
pub fn check_waits_for(name: &str, drive: bool, left: usize) -> String {
    let files = count(left, "file");
    if drive {
        format!(
            "{name} is not plugged in, so its {files} not yet checked for duplicates will be checked when it is back."
        )
    } else {
        format!(
            "{name} can't be found, so its {files} not yet checked for duplicates will be checked when its drive is back."
        )
    }
}

/// The background check for duplicates, stopped by the person with `left`
/// files still to check: what that means, and when it starts again.
///
/// What is true, and tested (`a_copy_found_after_its_original_was_checked_
/// is_still_told_apart_before_a_backup` in `keevoro-engine/src/deep.rs`):
/// until a file is read in full it is known by a sample, so the counts of
/// copies can be out (two files that only look alike can show as copies,
/// and a copy of a file already checked can be counted twice), but a
/// backup reads in full, before sending, every file that shares its sample
/// with another, checked or not, so it sends each different file and
/// leaves out only true copies.
pub fn check_stopped(left: usize) -> Vec<String> {
    if left == 0 {
        return vec!["Stopped checking for duplicates. Every file has been checked.".to_string()];
    }
    vec![
        format!(
            "Stopped checking for duplicates, with {} not checked yet.",
            count(left, "file")
        ),
        "Until they are, the counts of copies may be a little out, but nothing is backed up twice or left out by \
         mistake: before sending, a backup reads in full any file that looks like another."
            .to_string(),
        "Checking starts again by itself after your next search or backup, or now if you ask.".to_string(),
    ]
}

/// What clearing Keevoro's unfinished copies from a folder did (`cleared`)
/// or, asked first, would do: what goes, and what stays and why.
pub fn tidied(folder: &str, swept: &keevoro_engine::Swept, cleared: bool) -> Vec<String> {
    let mut said = Vec::new();
    let files = |n: u64| count(usize::try_from(n).unwrap_or(usize::MAX), "file");
    let things = |n: u64| count(usize::try_from(n).unwrap_or(usize::MAX), "thing");
    let one = |n: u64, it: &'static str, them: &'static str| if n == 1 { it } else { them };
    match (swept.files, cleared) {
        (0, true) => said.push(format!("There were no unfinished copies to clear in {folder}.")),
        (0, false) => said.push(format!("There are no unfinished copies to clear in {folder}.")),
        (n, true) => said.push(format!(
            "Cleared {} of unfinished copies ({}) from {folder}. They were Keevoro's working files; nothing that was \
             backed up was touched.",
            bytes(swept.bytes),
            files(n)
        )),
        (n, false) => said.push(format!(
            "There {} {} of unfinished copies ({}) in {folder}, left by a backup that was stopped part-way. \
             Clearing them touches nothing that was backed up.",
            one(swept.bytes.max(n), "is", "are"),
            bytes(swept.bytes),
            files(n)
        )),
    }
    if swept.in_use > 0 {
        let n = things(swept.in_use);
        said.push(if cleared {
            format!(
                "Left {n} written to in the last hour, as a backup may be using {} right now. Try again later.",
                one(swept.in_use, "it", "them")
            )
        } else {
            format!(
                "{n} written to in the last hour will be left, as a backup may be using {} right now.",
                one(swept.in_use, "it", "them")
            )
        });
    }
    if swept.not_ours > 0 {
        let n = things(swept.not_ours);
        said.push(if cleared {
            format!("Left {n} in its working folder that Keevoro did not put there.")
        } else {
            format!(
                "{n} in its working folder that Keevoro did not put there will be left as {}.",
                one(swept.not_ours, "it is", "they are")
            )
        });
    }
    said
}

/// A moment, given in seconds since 1970, as `2026-11-16 14:02:11` in UTC,
/// the same way the log writes it.
pub fn date_and_time(seconds: i64) -> String {
    crate::log::stamp(u64::try_from(seconds).unwrap_or(0))
}

/// How long a video lasts, as a video player shows it: `0:07`, `1:42`,
/// `1:02:05`. Rounded to the nearest second.
pub fn length(seconds: f64) -> String {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let all = seconds.max(0.0).round() as u64;
    let (hours, minutes, seconds) = (all / 3600, all / 60 % 60, all % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

use keevoro_core::estimate::{Kind, StageRecord, Unit};
use keevoro_engine::history::{Ended, Sending, Sent};

/// A speed, as "9.0 MB a second" for bytes or "12 files a second" (or a
/// minute, or an hour, for slow ones) for files.
pub fn rate(per_second: f64, unit: Unit) -> String {
    match unit {
        Unit::Bytes => format!("{} a second", bytes(per_second.round() as u64)),
        Unit::Files if per_second >= 1.0 => format!("{} a second", count(per_second.round() as usize, "file")),
        Unit::Files if per_second * 60.0 >= 1.0 => {
            format!("{} a minute", count((per_second * 60.0).round() as usize, "file"))
        }
        Unit::Files => format!("{} an hour", count((per_second * 3600.0).round() as usize, "file")),
    }
}

/// How much a stage did.
fn amount(done: u64, unit: Unit) -> String {
    match unit {
        Unit::Bytes => bytes(done),
        Unit::Files => count(usize::try_from(done).unwrap_or(usize::MAX), "file"),
    }
}

/// One stage on a line: what, how long, how much, how fast.
pub fn stage_line(stage: &StageRecord) -> String {
    let what = match (&stage.to, stage.kind) {
        (Some(to), Kind::SendingOriginals | Kind::SendingPreviews | Kind::WritingIndex) => {
            format!("{} (to {to})", stage.kind.label(false))
        }
        _ => stage.kind.label(false).to_string(),
    };
    let mut line = format!(
        "{what}: {}, {}",
        duration(stage.seconds),
        amount(stage.done, stage.unit)
    );
    if let Some(average) = stage.per_second {
        line.push_str(&format!(", {} on average", rate(average, stage.unit)));
    }
    if let (Some(slowest), Some(fastest)) = (stage.slowest, stage.fastest) {
        line.push_str(&format!(
            " (slowest {}, fastest {}, each held a minute or more)",
            rate(slowest, stage.unit),
            rate(fastest, stage.unit)
        ));
    }
    if !stage.finished {
        line.push_str(", not finished");
    }
    if let Some(problem) = &stage.problem {
        line.push_str(&format!(". Went wrong: {problem}"));
    }
    line
}

/// What one pass of sending did, in a sentence.
fn sending_line(what: &str, sending: &Sending) -> String {
    let mut line = format!("{} sent ({})", count(sending.sent, what), bytes(sending.sent_bytes));
    if sending.restored > 0 {
        line.push_str(&format!(
            ", {} of them sent again because {} had gone from there",
            thousands(sending.restored),
            if sending.restored == 1 { "it" } else { "they" }
        ));
    }
    if sending.already_there > 0 {
        line.push_str(&format!(", {} already there", thousands(sending.already_there)));
    }
    let not_sent = sending.not_sent.len() + sending.not_sent_more;
    if not_sent > 0 {
        line.push_str(&format!(", {} could not be sent", thousands(not_sent)));
    }
    line
}

/// Everything recorded about one backup, in lines a person reads from the
/// top: the headline, then the stages, the originals, the previews, where
/// it went, and what it said at the end. A backup recorded before the
/// detail was kept says only what it has, and so.
pub fn run_in_full(sent: &Sent) -> Vec<String> {
    let mut lines = vec![format!(
        "Backed up to {}, finishing {} (UTC): {} in {}, at an average of {} a second{}.",
        sent.to,
        date_and_time(sent.finished_at),
        bytes(sent.bytes),
        duration(sent.seconds),
        bytes(sent.average_speed()),
        if sent.stopped { ", stopped part-way" } else { "" }
    )];
    let Some(detail) = &sent.detail else {
        lines.push(
            "Only these figures were kept for this backup. Fuller detail is kept for every backup from now on.".into(),
        );
        return lines;
    };
    let ended = match detail.ended {
        Ended::Finished => "It finished".to_string(),
        Ended::Stopped => "It was stopped".to_string(),
        Ended::Problem => match &detail.problem {
            Some(problem) => format!("It ended with a problem: {}", problem.trim_end_matches('.')),
            None => "It ended with a problem".to_string(),
        },
    };
    let paused = if detail.paused_seconds >= 1 {
        format!(" It was paused for {}.", duration(detail.paused_seconds))
    } else {
        String::new()
    };
    let started = match &detail.by_itself {
        Some(by_itself) => format!("Started by itself {}, from {}", by_itself.why, detail.started_by),
        None => format!("Started from {}", detail.started_by),
    };
    lines.push(format!(
        "{started} at {} (UTC), with Keevoro {}. {ended}.{paused}",
        date_and_time(detail.started_at),
        detail.version
    ));
    if let Some(by_itself) = &detail.by_itself {
        if !by_itself.looked_again.is_empty() {
            lines.push(format!(
                "It looked through {} again first.",
                by_itself.looked_again.join(", ")
            ));
        }
        lines.extend(by_itself.passed_over.iter().cloned());
        lines.extend(by_itself.found.iter().cloned());
        if !by_itself.waiting.is_empty() {
            lines.push("Waiting for your say:".into());
            lines.extend(by_itself.waiting.iter().map(|w| format!("  {w}")));
        }
    }

    lines.push(String::new());
    lines.push("Stages, with time paused left out:".into());
    lines.extend(detail.stages.iter().map(|stage| format!("  {}", stage_line(stage))));

    lines.push(String::new());
    lines.push("Originals:".into());
    for at in &detail.destinations {
        lines.push(format!("  To {}: {}.", at.to, sending_line("file", &at.originals)));
        for not_sent in &at.originals.not_sent {
            lines.push(format!(
                "    {}: {}.",
                not_sent.file,
                not_sent.reason.trim_end_matches('.')
            ));
        }
        if at.originals.not_sent_more > 0 {
            lines.push(format!("    and {} more.", thousands(at.originals.not_sent_more)));
        }
    }
    if detail.duplicates > 0 {
        lines.push(format!(
            "  Duplicates left out: {}, saving {}.",
            thousands(detail.duplicates),
            bytes(detail.duplicate_bytes)
        ));
    }

    lines.push(String::new());
    match &detail.previews {
        None => lines.push("Previews: none, as this backup sends the originals alone.".into()),
        Some(made) => {
            lines.push("Previews:".into());
            lines.push(format!(
                "  Made {} and {}.",
                count(usize::try_from(made.photos).unwrap_or(0), "photo preview"),
                count(usize::try_from(made.videos).unwrap_or(0), "video still")
            ));
            if made.photos_without + made.videos_without > 0 {
                lines.push(format!(
                    "  Without a preview, because Keevoro couldn't open them: {} and {}.",
                    count(usize::try_from(made.photos_without).unwrap_or(0), "photo"),
                    count(usize::try_from(made.videos_without).unwrap_or(0), "video")
                ));
            }
            for at in &detail.destinations {
                if let Some(previews) = &at.previews {
                    lines.push(format!("  To {}: {}.", at.to, sending_line("preview file", previews)));
                }
            }
        }
    }

    lines.push(String::new());
    lines.push("Where it went:".into());
    for at in &detail.destinations {
        let mut line = format!("  {}", at.to);
        if at.over_network {
            line.push_str(", across a network");
        }
        if at.reached {
            line.push_str(&format!(": {} in {}", bytes(at.bytes), duration(at.seconds)));
            if let Some(indexed) = at.indexed {
                line.push_str(&format!(", its index lists {}", count(indexed, "file")));
            }
        } else {
            line.push_str(&format!(
                ": couldn't be backed up to. {}",
                at.problem.as_deref().unwrap_or_default()
            ));
        }
        lines.push(format!("{}.", line.trim_end_matches('.')));
        if at.cleared_bytes > 0 {
            lines.push(format!("    {}", keevoro_engine::send::cleared_words(at.cleared_bytes)));
        }
    }

    if !detail.summary.is_empty() {
        lines.push(String::new());
        lines.push("What it said at the end:".into());
        lines.extend(detail.summary.iter().map(|line| format!("  {line}")));
    }
    lines
}

#[cfg(test)]
mod tests {
    #[test]
    fn how_long_ago_is_said_as_a_person_says_it() {
        let now = 1_800_000_000;
        assert_eq!(super::ago(now - 5, now), "just now");
        assert_eq!(super::ago(now - 60, now), "1 minute ago");
        assert_eq!(super::ago(now - 7_200, now), "2 hours ago");
        assert_eq!(super::ago(now - 3 * 86_400, now), "3 days ago");
        assert_eq!(super::ago(1_700_000_000, now), "on 2023-11-14");
        assert_eq!(super::ago(now + 99, now), "just now", "a clock that moved back");
    }

    use super::*;

    #[test]
    fn a_videos_length_reads_as_a_player_shows_it() {
        assert_eq!(length(7.4), "0:07");
        assert_eq!(length(102.0), "1:42");
        assert_eq!(length(3725.0), "1:02:05");
        assert_eq!(length(59.6), "1:00");
        assert_eq!(length(-1.0), "0:00");
    }

    #[test]
    fn numbers_read_as_a_person_would_say_them() {
        assert_eq!(count(1, "photo"), "1 photo");
        assert_eq!(count(1204, "photo"), "1,204 photos");
        assert_eq!(thousands(9870), "9,870");
        assert!(bytes(4_200_000_000).ends_with("GB"), "{}", bytes(4_200_000_000));
        assert!(!duration(200).is_empty());
        assert_eq!(date_and_time(0), "1970-01-01 00:00:00");
        assert_eq!(date_and_time(-5), "1970-01-01 00:00:00");
    }

    #[test]
    fn clearing_unfinished_copies_says_what_goes_and_what_stays_and_why_before_and_after() {
        let swept = keevoro_engine::Swept {
            files: 211,
            bytes: 6 * 1024 * 1024 * 1024,
            in_use: 1,
            not_ours: 2,
        };
        let before = tidied("D:\\Backup", &swept, false);
        assert_eq!(
            before,
            [
                "There are 6.0 GB of unfinished copies (211 files) in D:\\Backup, left by a backup that was stopped \
                 part-way. Clearing them touches nothing that was backed up.",
                "1 thing written to in the last hour will be left, as a backup may be using it right now.",
                "2 things in its working folder that Keevoro did not put there will be left as they are.",
            ]
        );
        let after = tidied("D:\\Backup", &swept, true);
        assert!(after[0].starts_with("Cleared 6.0 GB of unfinished copies (211 files) from D:\\Backup."));
        assert!(after[1].starts_with("Left 1 thing written to in the last hour"));
        assert!(after[2].starts_with("Left 2 things"));
        let none = keevoro_engine::Swept::default();
        assert_eq!(
            tidied("E:\\", &none, false),
            ["There are no unfinished copies to clear in E:\\."]
        );
        assert_eq!(
            tidied("E:\\", &none, true),
            ["There were no unfinished copies to clear in E:\\."]
        );
        for line in before.iter().chain(&after) {
            assert!(!line.contains('\u{2014}') && !line.contains("  "), "{line}");
        }
    }
}

#[cfg(test)]
mod a_run_in_full {
    use super::*;
    use keevoro_engine::history::{DestinationRecord, NotSent, PreviewsMade, RunDetail};

    const GB: u64 = 1024 * 1024 * 1024;

    fn stage(kind: Kind, to: Option<&str>, done: u64, seconds: u64) -> StageRecord {
        StageRecord {
            kind,
            to: to.map(str::to_string),
            unit: kind.unit(),
            done,
            seconds,
            finished: true,
            per_second: Some(done as f64 / seconds as f64),
            slowest: None,
            fastest: None,
            problem: None,
        }
    }

    /// A long backup: 8,630 files to a network folder in 9 hours 40
    /// minutes.
    fn long_run() -> Sent {
        let to = "the folder \\\\nas\\photos";
        let mut sending = stage(Kind::SendingOriginals, Some(to), 3124 * GB / 10, 32_400);
        sending.slowest = Some(2.0 * 1024.0 * 1024.0);
        sending.fastest = Some(11.0 * 1024.0 * 1024.0);
        sending.problem = Some("Couldn't back up 2 files".into());
        Sent {
            finished_at: 1_794_751_331,
            seconds: 34_800,
            bytes: 3162 * GB / 10,
            to: to.into(),
            stopped: false,
            detail: Some(Box::new(RunDetail {
                started_at: 1_794_751_331 - 34_800,
                paused_seconds: 720,
                ended: Ended::Finished,
                problem: None,
                started_by: "the app".into(),
                version: "0.33.0".into(),
                destinations: vec![DestinationRecord {
                    to: to.into(),
                    over_network: true,
                    reached: true,
                    problem: None,
                    seconds: 34_800,
                    bytes: 3162 * GB / 10,
                    originals: Sending {
                        sent: 8_628,
                        sent_bytes: 3124 * GB / 10,
                        restored: 40,
                        already_there: 0,
                        not_sent: vec![NotSent {
                            file: "originals/2019/IMG_0001.jpg".into(),
                            reason: "The source couldn't be read".into(),
                        }],
                        not_sent_more: 1,
                    },
                    previews: Some(Sending {
                        sent: 16_820,
                        sent_bytes: 38 * GB / 10,
                        ..Sending::default()
                    }),
                    indexed: Some(8_628),
                    cleared_files: 211,
                    cleared_bytes: 6 * GB,
                }],
                stages: vec![stage(Kind::Placing, None, 8_630, 40), sending],
                duplicates: 245,
                duplicate_bytes: 3 * GB,
                previews: Some(PreviewsMade {
                    photos: 8_410,
                    videos: 0,
                    photos_without: 3,
                    videos_without: 0,
                }),
                summary: vec!["Safely backed up 8,628 files (312.4 GB).".into()],
                by_itself: None,
            })),
        }
    }

    #[test]
    fn a_long_backup_reads_from_the_headline_down() {
        let lines = run_in_full(&long_run()).join("\n");
        for said in [
            "316.2 GB in about 9 hours 40 minutes, at an average of 9.3 MB a second.",
            "Started from the app at",
            "with Keevoro 0.33.0. It finished. It was paused for about 12 minutes.",
            "  Working out where each file goes: under a minute, 8,630 files, 216 files a second on average",
            "312.4 GB, 9.9 MB a second on average (slowest 2.0 MB a second, fastest 11.0 MB a second, each held a minute or more). Went wrong: Couldn't back up 2 files",
            "8,628 files sent (312.4 GB), 40 of them sent again because they had gone from there, 2 could not be sent.",
            "    originals/2019/IMG_0001.jpg: The source couldn't be read.",
            "    and 1 more.",
            "Duplicates left out: 245, saving 3.0 GB.",
            "Made 8,410 photo previews and 0 video stills.",
            "Without a preview, because Keevoro couldn't open them: 3 photos and 0 videos.",
            "16,820 preview files sent (3.8 GB).",
            ", across a network: 316.2 GB in about 9 hours 40 minutes, its index lists 8,628 files.",
            "  Safely backed up 8,628 files (312.4 GB).",
            "    Cleared 6.0 GB of unfinished copies left by an earlier backup that was stopped. They were Keevoro's working files, not part of your backup.",
        ] {
            assert!(lines.contains(said), "{said}\n---\n{lines}");
        }
        assert!(!lines.contains('\u{2014}'), "no em dashes");
    }

    #[test]
    fn an_older_backup_says_only_what_was_kept() {
        let mut older = long_run();
        older.detail = None;
        let lines = run_in_full(&older);
        assert_eq!(lines.len(), 2);
        assert!(lines[1].starts_with("Only these figures were kept for this backup."));
    }

    #[test]
    fn slow_work_is_counted_by_the_minute_or_hour() {
        assert_eq!(rate(0.5, Unit::Files), "30 files a minute");
        assert_eq!(rate(0.001, Unit::Files), "4 files an hour");
        assert_eq!(rate(3.0, Unit::Files), "3 files a second");
    }
}
