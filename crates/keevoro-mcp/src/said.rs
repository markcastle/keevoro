//! A short summary, in plain English, of what a tool gave back.
//!
//! The agent gets the API's own JSON as well, exactly as the API answered;
//! this is the line it can pass straight on to its person. It says the
//! most useful thing in a sentence or a short list, and leaves the detail
//! to the JSON.

use keevoro_api::words::{bytes, count, thousands};
use keevoro_api::{Op, Said};
use serde_json::Value;

fn s<'v>(v: &'v Value, field: &str) -> &'v str {
    v.get(field).and_then(Value::as_str).unwrap_or_default()
}

fn n(v: &Value, field: &str) -> usize {
    v.get(field)
        .and_then(Value::as_u64)
        .map_or(0, |n| usize::try_from(n).unwrap_or(usize::MAX))
}

fn each(v: &Value) -> impl Iterator<Item = &Value> {
    v.as_array().into_iter().flatten()
}

fn photos_and_videos(photos: usize, videos: usize) -> String {
    format!("{} and {}", count(photos, "photo"), count(videos, "video"))
}

/// What a tool for `op` gave back, in a few plain words.
pub fn summary(op: Op, answer: &Value) -> String {
    if op.entry().job {
        return started(answer);
    }
    match op {
        Op::BackupsList => backups(answer),
        Op::BackupsStart => format!(
            "Started the backup \"{}\". It has no sources or destinations yet.",
            s(answer, "name")
        ),
        Op::BackupsRename => format!("It is now called \"{}\".", s(answer, "name")),
        Op::SourcesList => listed(answer, "source", |source| {
            format!(
                "{} (id {}, {}): {} files",
                s(source, "name"),
                source.get("id").cloned().unwrap_or_default(),
                s(source, "root"),
                thousands(n(source, "items"))
            )
        }),
        Op::FoundSummary => format!(
            "Found {}, {} in all. {} left out, {} set aside as probably not photos.",
            photos_and_videos(n(answer, "photos"), n(answer, "videos")),
            bytes(answer.get("bytes").and_then(Value::as_u64).unwrap_or(0)),
            count(n(answer, "duplicates"), "duplicate"),
            count(n(answer, "setAside"), "picture")
        ),
        Op::CollectionsList => listed(answer, "collection", |c| {
            format!(
                "{} ({}{})",
                s(c, "key"),
                count(n(c, "photos") + n(c, "videos"), "file"),
                if c.get("kept").and_then(Value::as_bool) == Some(false) {
                    ", left out"
                } else {
                    ""
                }
            )
        }),
        Op::DestinationsList => listed(answer, "destination", |d| {
            format!("{}: {} there", s(d, "name"), thousands(n(d, "there")))
        }),
        Op::DestinationsAdd => format!(
            "{} works, and the backup will go there too.",
            capital(s(answer, "name"))
        ),
        Op::DestinationsRemove => {
            let gone: Vec<&str> = each(answer).filter_map(Value::as_str).collect();
            format!(
                "No longer backing up to {}. Nothing stored there was changed.",
                gone.join(" or ")
            )
        }
        Op::BackupRehearse => {
            let said: Vec<&str> = each(answer.get("said").unwrap_or(&Value::Null))
                .filter_map(Value::as_str)
                .collect();
            if said.is_empty() {
                format!(
                    "Backing up now would send {} ({}).",
                    count(n(answer, "files"), "file"),
                    bytes(answer.get("bytes").and_then(Value::as_u64).unwrap_or(0))
                )
            } else {
                said.join("\n")
            }
        }
        Op::BackupLastSummary => match answer.get("summary").and_then(Value::as_array) {
            Some(lines) => lines.iter().filter_map(Value::as_str).collect::<Vec<_>>().join("\n"),
            None if answer.is_null() => "This backup has not been run yet.".to_string(),
            None => "That is what the last backup said.".to_string(),
        },
        Op::BackupPreviews => {
            if answer.as_bool() == Some(false) {
                "This backup sends the originals alone, with no previews.".to_string()
            } else {
                "This backup also makes previews, for a gallery to show.".to_string()
            }
        }
        Op::JobsStatus => status(answer),
        Op::JobsPause => "Paused: Keevoro's heavy work waits once the file in hand is done.".to_string(),
        Op::JobsCarryOn => "Carrying on from where it was.".to_string(),
        Op::JobsStop => match each(answer).count() {
            0 => "Nothing is running.".to_string(),
            jobs => format!(
                "Asked {} to stop once the file in hand is done. Everything done so far is kept.",
                count(jobs, "job")
            ),
        },
        Op::KeevoroAbout => format!("Keevoro {}.", s(answer, "version")),
        Op::KeevoroUpdate => match answer.get("version").and_then(Value::as_str) {
            Some(version) => format!("Keevoro {version} is available. The person can install it."),
            None => "This is the newest Keevoro.".to_string(),
        },
        Op::AgentsAccess => access(answer),
        _ => match answer {
            Value::Array(all) => format!("{}.", count(all.len(), "item")),
            _ => "Done.".to_string(),
        },
    }
}

fn capital(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

fn listed(answer: &Value, thing: &str, line: impl Fn(&Value) -> String) -> String {
    let all: Vec<String> = each(answer).map(line).collect();
    if all.is_empty() {
        return format!("There are no {thing}s.");
    }
    format!("{}:\n{}", capital(&count(all.len(), thing)), all.join("\n"))
}

fn backups(answer: &Value) -> String {
    if each(answer).next().is_none() {
        return "There are no backups yet. start_backup starts one.".to_string();
    }
    listed(answer, "backup", |b| {
        let found = b.get("found").unwrap_or(&Value::Null);
        format!(
            "\"{}\"{}: {}, {} waiting to be backed up",
            s(b, "name"),
            if b.get("open").and_then(Value::as_bool) == Some(true) {
                " (open in the app)"
            } else {
                ""
            },
            photos_and_videos(n(found, "photos"), n(found, "videos")),
            thousands(n(found, "waiting"))
        )
    })
}

fn started(answer: &Value) -> String {
    let id = answer.get("id").cloned().unwrap_or_default();
    format!(
        "Started job {id}. Ask job_status with job {id} (and waitSeconds, to wait for it) to follow it and to hear how it ended."
    )
}

fn status(answer: &Value) -> String {
    let mut lines = Vec::new();
    for job in each(answer) {
        let id = job.get("id").cloned().unwrap_or_default();
        if job.get("ended").and_then(Value::as_bool) == Some(true) {
            lines.push(format!("Job {id} has ended."));
            let last: Option<Said> = job.get("last").and_then(|l| serde_json::from_value(l.clone()).ok());
            lines.extend(last_word(last.as_ref()));
            continue;
        }
        let step = s(job, "step");
        lines.push(format!(
            "Job {id} is running{}{}",
            if step.is_empty() {
                String::new()
            } else {
                format!(": {step}")
            },
            if job.get("paused").and_then(Value::as_bool) == Some(true) {
                " (paused)"
            } else if job.get("stopping").and_then(Value::as_bool) == Some(true) {
                " (stopping)"
            } else {
                ""
            }
        ));
        // A backup's whole estimate, in the words a person sees in the app.
        if let Some(estimate) = job
            .get("estimate")
            .and_then(|e| serde_json::from_value::<keevoro_api::estimate::Estimate>(e.clone()).ok())
        {
            lines.extend(keevoro_api::estimate::in_lines(&estimate));
        }
    }
    if lines.is_empty() {
        return "Nothing is running.".to_string();
    }
    lines.join("\n")
}

/// How a job ended, in a line or a few.
fn last_word(last: Option<&Said>) -> Vec<String> {
    match last {
        Some(Said::Send(e)) => e.summary.iter().cloned().chain(e.problem.clone()).collect(),
        Some(Said::Scan(e)) if e.stopped => vec![format!(
            "It was stopped. Nothing from this search of {} was kept.",
            e.name
        )],
        Some(Said::Scan(e)) => {
            let found = photos_and_videos(
                usize::try_from(e.photos).unwrap_or(usize::MAX),
                usize::try_from(e.videos).unwrap_or(usize::MAX),
            );
            let mut lines = vec![format!("Looked through {}: found {found}.", e.name)];
            if e.unreadable > 0 {
                lines.push(format!("{} couldn't be read.", count(e.unreadable, "file")));
            }
            lines.extend(e.problem.clone());
            lines
        }
        Some(Said::Surveyed(found)) => {
            let mut lines = vec![format!(
                "Looked at {} in {}, without opening any.",
                count(found.files, "file"),
                count(found.folders, "folder")
            )];
            if found.hints.is_empty() {
                lines.push("No photos or videos were found by name.".to_string());
            }
            for hint in &found.hints {
                lines.push(format!(
                    "{}: {}{}",
                    hint.folder,
                    photos_and_videos(hint.photos, hint.videos),
                    if hint.already_a_source {
                        " (a source already)"
                    } else if !hint.likely_photos {
                        " (small pictures, probably not photos)"
                    } else {
                        ""
                    }
                ));
            }
            lines
        }
        Some(_) => Vec::new(),
        None => vec!["It ended without saying how.".to_string()],
    }
}

fn access(answer: &Value) -> String {
    let folders = |field: &str| -> String {
        let all: Vec<&str> = each(answer.get(field).unwrap_or(&Value::Null))
            .filter_map(Value::as_str)
            .collect();
        if all.is_empty() {
            "no folders".to_string()
        } else {
            all.join(", ")
        }
    };
    let changes = if answer.get("mayChange").and_then(Value::as_bool) == Some(true) {
        "You may make changes."
    } else {
        "You may only look, not make changes. The person can allow changes in the Keevoro app, under Settings, AI agents, or with: keevoro agents allow-changes on"
    };
    format!(
        "You may look in {}, and back up to {}. {changes}",
        folders("lookIn"),
        folders("backUpTo")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn summaries_are_plain_sentences() {
        assert_eq!(
            summary(Op::BackupsList, &json!([])),
            "There are no backups yet. start_backup starts one."
        );
        let listed = summary(
            Op::BackupsList,
            &json!([{ "name": "Card photos", "open": true, "found": { "photos": 3, "videos": 1, "waiting": 2 } }]),
        );
        assert_eq!(
            listed,
            "1 backup:\n\"Card photos\" (open in the app): 3 photos and 1 video, 2 waiting to be backed up"
        );
        assert_eq!(
            summary(Op::BackupRun, &json!({ "id": 42 })),
            "Started job 42. Ask job_status with job 42 (and waitSeconds, to wait for it) to follow it and to hear how it ended."
        );
        assert_eq!(summary(Op::JobsStatus, &json!([])), "Nothing is running.");
        assert_eq!(
            summary(
                Op::JobsStatus,
                &json!([{ "id": 7, "ended": true, "last": { "summary": ["Safely backed up 2 files."], "finished": true } }])
            ),
            "Job 7 has ended.\nSafely backed up 2 files."
        );
        assert_eq!(
            summary(
                Op::JobsStatus,
                &json!([{ "id": 7, "step": "Getting ready", "paused": true }])
            ),
            "Job 7 is running: Getting ready (paused)"
        );
        let access = summary(
            Op::AgentsAccess,
            &json!({ "lookIn": [], "backUpTo": [], "mayChange": false }),
        );
        assert!(access.starts_with("You may look in no folders, and back up to no folders."));
        assert!(access.contains("keevoro agents allow-changes on"));
        for text in [listed, access] {
            assert!(!text.contains('\u{2014}'));
        }
    }
}
