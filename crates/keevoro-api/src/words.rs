//! Numbers said the way Keevoro says them everywhere: "1,204 photos",
//! "4.2 GB", "about 3 minutes". The app, the command line and an agent use
//! these so that the same thing reads the same in each.

pub use keevoro_engine::history::duration;
pub use keevoro_engine::summary::{bytes, count, thousands};

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
    line
}

/// What one pass of sending did, in a sentence.
fn sending_line(what: &str, sending: &Sending) -> String {
    let mut line = format!("{} sent ({})", count(sending.sent, what), bytes(sending.sent_bytes));
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
    lines.push(format!(
        "Started from {} at {} (UTC), with Keevoro {}. {ended}.{paused}",
        detail.started_by,
        date_and_time(detail.started_at),
        detail.version
    ));

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
    use super::*;

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
            files: 583,
            bytes: 15 * 1024 * 1024 * 1024,
            in_use: 1,
            not_ours: 2,
        };
        let before = tidied("D:\\Backup", &swept, false);
        assert_eq!(
            before,
            [
                "There are 15.0 GB of unfinished copies (583 files) in D:\\Backup, left by a backup that was stopped \
                 part-way. Clearing them touches nothing that was backed up.",
                "1 thing written to in the last hour will be left, as a backup may be using it right now.",
                "2 things in its working folder that Keevoro did not put there will be left as they are.",
            ]
        );
        let after = tidied("D:\\Backup", &swept, true);
        assert!(after[0].starts_with("Cleared 15.0 GB of unfinished copies (583 files) from D:\\Backup."));
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
        }
    }

    /// The owner's real backup: 11,245 files to a network folder in 22
    /// hours 50 minutes.
    fn long_run() -> Sent {
        let to = "the folder \\\\nas\\photos";
        let mut sending = stage(Kind::SendingOriginals, Some(to), 7168 * GB / 10, 80_000);
        sending.slowest = Some(2.0 * 1024.0 * 1024.0);
        sending.fastest = Some(11.0 * 1024.0 * 1024.0);
        Sent {
            finished_at: 1_794_751_331,
            seconds: 82_200,
            bytes: 7206 * GB / 10,
            to: to.into(),
            stopped: false,
            detail: Some(Box::new(RunDetail {
                started_at: 1_794_751_331 - 82_200,
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
                    seconds: 82_200,
                    bytes: 7206 * GB / 10,
                    originals: Sending {
                        sent: 11_243,
                        sent_bytes: 7168 * GB / 10,
                        already_there: 0,
                        not_sent: vec![NotSent {
                            file: "originals/2019/IMG_0001.jpg".into(),
                            reason: "The source couldn't be read".into(),
                        }],
                        not_sent_more: 1,
                    },
                    previews: Some(Sending {
                        sent: 21_792,
                        sent_bytes: 38 * GB / 10,
                        ..Sending::default()
                    }),
                    indexed: Some(11_243),
                    cleared_files: 583,
                    cleared_bytes: 15 * GB,
                }],
                stages: vec![stage(Kind::Placing, None, 11_245, 40), sending],
                duplicates: 312,
                duplicate_bytes: 4 * GB,
                previews: Some(PreviewsMade {
                    photos: 10_896,
                    videos: 0,
                    photos_without: 3,
                    videos_without: 0,
                }),
                summary: vec!["Safely backed up 11,243 files (716.8 GB).".into()],
            })),
        }
    }

    #[test]
    fn a_long_backup_reads_from_the_headline_down() {
        let lines = run_in_full(&long_run()).join("\n");
        for said in [
            "720.6 GB in about 22 hours 50 minutes, at an average of 9.0 MB a second.",
            "Started from the app at",
            "with Keevoro 0.33.0. It finished. It was paused for about 12 minutes.",
            "  Working out where each file goes: under a minute, 11,245 files, 281 files a second on average",
            "716.8 GB, 9.2 MB a second on average (slowest 2.0 MB a second, fastest 11.0 MB a second, each held a minute or more)",
            "11,243 files sent (716.8 GB), 2 could not be sent.",
            "    originals/2019/IMG_0001.jpg: The source couldn't be read.",
            "    and 1 more.",
            "Duplicates left out: 312, saving 4.0 GB.",
            "Made 10,896 photo previews and 0 video stills.",
            "Without a preview, because Keevoro couldn't open them: 3 photos and 0 videos.",
            "21,792 preview files sent (3.8 GB).",
            ", across a network: 720.6 GB in about 22 hours 50 minutes, its index lists 11,243 files.",
            "  Safely backed up 11,243 files (716.8 GB).",
            "    Cleared 15.0 GB of unfinished copies left by an earlier backup that was stopped. They were Keevoro's working files, not part of your backup.",
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
