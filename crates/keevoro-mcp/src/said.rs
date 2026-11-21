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

/// "30 photos and 2 videos", with any other files carried along after:
/// what was found in `v`, said the one way (`keevoro_api::words`).
fn found_in(v: &Value) -> String {
    keevoro_api::words::photos_and_videos(n(v, "photos"), n(v, "videos"), n(v, "others"))
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
            let online = source
                .get("onlineOnlySaid")
                .and_then(Value::as_str)
                .map(|said| format!(" {said}"))
                .unwrap_or_default();
            format!(
                "{} (id {}, {}): {} files.{online}",
                s(source, "name"),
                source.get("id").cloned().unwrap_or_default(),
                s(source, "root"),
                thousands(n(source, "items"))
            )
        }),
        Op::PlacesFolders => {
            let folders: Vec<String> = each(answer.get("folders").unwrap_or(&Value::Null))
                .map(|f| {
                    let online = match n(f, "onlineOnly") {
                        0 => String::new(),
                        many => format!(" ({} only in online storage)", thousands(many)),
                    };
                    format!("{}: {}{online}", s(f, "folder"), found_in(f))
                })
                .collect();
            let mut lines = vec![format!(
                "{} in {}, in {}. files_in_folder lists a folder's files.",
                found_in(answer),
                s(answer, "place"),
                count(folders.len(), "folder")
            )];
            lines.extend(folders);
            lines.join("\n")
        }
        Op::PlacesMap => {
            let top = answer.get("top").unwrap_or(&Value::Null);
            let size = |v: &Value, field: &str| bytes(v.get(field).and_then(Value::as_u64).unwrap_or(0));
            let carried = match top.get("carriedBytes").and_then(Value::as_u64) {
                Some(b) if b > 0 => format!(", other files to carry along {}", bytes(b)),
                _ => String::new(),
            };
            let mut lines = vec![format!(
                "{} in {}: photos {}, videos {}{carried}, everything else {}; the backup would take {}. The biggest:",
                size(top, "bytes"),
                s(answer, "folder"),
                size(top, "photoBytes"),
                size(top, "videoBytes"),
                size(top, "otherBytes"),
                size(top, "wouldBackUp"),
            )];
            lines.extend(each(top.get("children").unwrap_or(&Value::Null)).take(12).map(|c| {
                let why = c
                    .get("skipped")
                    .or_else(|| c.get("leftOut"))
                    .and_then(Value::as_str)
                    .map(|why| format!(" ({why})"))
                    .unwrap_or_default();
                format!("{}: {}{why}", s(c, "name"), size(c, "bytes"))
            }));
            lines.join("\n")
        }
        Op::PlacesFiles => {
            let shown = each(answer.get("files").unwrap_or(&Value::Null)).count();
            let online = match n(answer, "onlineOnly") {
                0 => String::new(),
                many => format!(
                    " {} {} only in online storage, so Keevoro won't open {}.",
                    count(many, "file"),
                    if many == 1 { "is" } else { "are" },
                    if many == 1 { "it" } else { "them" }
                ),
            };
            format!(
                "{} in {}; {} given here. show_picture with a file's path shows it.{online}",
                found_in(answer),
                s(answer, "folder"),
                thousands(shown)
            )
        }
        Op::FoundSummary => format!(
            "Found {}, {} in all. {} left out, {} set aside as probably not photos.",
            found_in(answer),
            bytes(answer.get("bytes").and_then(Value::as_u64).unwrap_or(0)),
            count(n(answer, "duplicates"), "duplicate"),
            count(n(answer, "setAside"), "picture")
        ),
        Op::CollectionsList => listed(answer, "collection", |c| {
            format!(
                "{} ({}{})",
                s(c, "key"),
                count(n(c, "photos") + n(c, "videos") + n(c, "others"), "file"),
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
        Op::DestinationsAdd => {
            let works = format!(
                "{} works, and the backup will go there too.",
                capital(s(answer, "name"))
            );
            match answer.get("keys").and_then(Value::as_str) {
                Some(keys) => format!("{works} {keys}"),
                None => works,
            }
        }
        Op::DestinationsRemove => {
            let gone: Vec<&str> = each(answer).filter_map(Value::as_str).collect();
            format!(
                "No longer backing up to {}. Nothing that was backed up there was changed.",
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
        Op::SourcesLeaveOut => s(answer, "said").to_string(),
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
        Op::FileKindsList => file_kinds(answer),
        Op::BackupHistory => history(answer),
        _ => match answer {
            Value::Array(all) => format!("{}.", count(all.len(), "item")),
            _ => "Done.".to_string(),
        },
    }
}

/// One backup in full when one was asked for; otherwise how many, and the
/// newest in a line. The JSON beside it has everything.
fn history(answer: &Value) -> String {
    let all: Vec<keevoro_api::history::Sent> = each(answer)
        .filter_map(|sent| serde_json::from_value(sent.clone()).ok())
        .collect();
    match all.as_slice() {
        [] => "This backup hasn't been run yet.".to_string(),
        [one] => keevoro_api::words::run_in_full(one).join("\n"),
        [newest, ..] => format!(
            "{} backups are recorded, newest first. The newest: {} Ask with run for one in full.",
            all.len(),
            keevoro_api::words::run_in_full(newest)[0]
        ),
    }
}

/// The endings looked for, photos then videos, then any other files
/// carried along, and those switched off.
fn file_kinds(answer: &Value) -> String {
    let endings = |pick: &dyn Fn(&Value) -> bool| -> String {
        let all: Vec<String> = each(answer)
            .filter(|k| pick(k))
            .map(|k| format!(".{}", s(k, "ending")))
            .collect();
        if all.is_empty() {
            "none".to_string()
        } else {
            all.join(", ")
        }
    };
    let on = |k: &Value| k.get("on").and_then(Value::as_bool).unwrap_or_default();
    let others = if each(answer).any(|k| s(k, "kind") == "other") {
        format!(
            " And these other files, carried along: {}.",
            endings(&|k| on(k) && s(k, "kind") == "other")
        )
    } else {
        String::new()
    };
    format!(
        "Keevoro looks for these photos: {}. And these videos: {}.{others} Switched off: {}.",
        endings(&|k| on(k) && s(k, "kind") == "photo"),
        endings(&|k| on(k) && s(k, "kind") == "video"),
        endings(&|k| !on(k)),
    )
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
            found_in(found),
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
            let n = |n: u64| usize::try_from(n).unwrap_or(usize::MAX);
            let found = keevoro_api::words::photos_and_videos(n(e.photos), n(e.videos), n(e.others));
            let mut lines = vec![format!("Looked through {}: found {found}.", e.name)];
            if e.unreadable > 0 {
                lines.push(format!("{} couldn't be read.", count(e.unreadable, "file")));
            }
            lines.extend(e.online_only_said.clone());
            lines.extend(e.left_out.iter().map(|left| left.said.clone()));
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
                    keevoro_api::words::photos_and_videos(hint.photos, hint.videos, hint.others),
                    if hint.already_a_source {
                        " (a source already)"
                    } else if !hint.likely_photos {
                        " (small pictures, probably not photos)"
                    } else {
                        ""
                    }
                ));
                lines.extend(hint.online_only_said.clone());
            }
            lines.extend(found.left_out.iter().map(|left| left.said.clone()));
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
    fn the_kinds_looked_for_are_named() {
        let list = json!([
            { "ending": "jpg", "kind": "photo", "on": true, "builtIn": true },
            { "ending": "avi", "kind": "video", "on": false, "builtIn": true },
            { "ending": "heix", "kind": "photo", "on": true, "builtIn": false },
        ]);
        assert_eq!(
            summary(Op::FileKindsList, &list),
            "Keevoro looks for these photos: .jpg, .heix. And these videos: none. Switched off: .avi."
        );
    }

    #[test]
    fn files_only_online_are_said_in_the_summaries_not_only_the_log() {
        let said = "412 files here are in OneDrive only, not on this computer, so Keevoro did not open them.";
        let sources = summary(
            Op::SourcesList,
            &json!([{ "id": 1, "name": "OneDrive", "root": "C:\\Users\\Mark\\OneDrive", "items": 30, "onlineOnly": 412, "onlineOnlySaid": said }]),
        );
        assert!(sources.ends_with(&format!("30 files. {said}")), "{sources}");
        let looked = summary(
            Op::JobsStatus,
            &json!([{ "id": 3, "ended": true, "last": { "name": "OneDrive", "photos": 30, "videos": 0, "files": 99, "finished": true, "onlineOnly": 412, "onlineOnlySaid": said } }]),
        );
        assert!(looked.contains(said), "{looked}");
        let surveyed = summary(
            Op::JobsStatus,
            &json!([{ "id": 4, "ended": true, "last": { "hints": [{ "folder": "C:\\Users\\Mark\\OneDrive\\Pictures", "photos": 500, "videos": 0, "bytes": 1, "likelyPhotos": true, "alreadyASource": false, "onlineOnly": 412, "onlineOnlySaid": said }], "folders": 1, "files": 2, "unreadable": 0, "seconds": 0.1, "stopped": false } }]),
        );
        assert!(surveyed.contains(said), "{surveyed}");
    }

    #[test]
    fn looking_inside_is_summed_up_in_words() {
        let folders = summary(
            Op::PlacesFolders,
            &json!({ "place": "C:\\Pictures", "photos": 120, "videos": 4, "folders": [
                { "folder": "C:\\Pictures\\2019", "photos": 120, "videos": 4, "onlineOnly": 3 }
            ] }),
        );
        assert_eq!(
            folders,
            "120 photos and 4 videos in C:\\Pictures, in 1 folder. files_in_folder lists a folder's files.\nC:\\Pictures\\2019: 120 photos and 4 videos (3 only in online storage)"
        );
        let map = summary(
            Op::PlacesMap,
            &json!({ "folder": "C:\\Pictures", "top": { "bytes": 3_u64 << 30, "photoBytes": 1_u64 << 30, "videoBytes": 0, "otherBytes": 2_u64 << 30, "wouldBackUp": 1_u64 << 30, "children": [
                { "name": "Texture packs", "bytes": 2_u64 << 30 },
                { "name": "Old Keevoro", "bytes": 1_u64 << 30, "skipped": "A library from an older Keevoro" }
            ] } }),
        );
        assert!(map.starts_with("3.0 GB in C:\\Pictures: photos 1.0 GB"), "{map}");
        assert!(
            map.ends_with("Old Keevoro: 1.0 GB (A library from an older Keevoro)"),
            "{map}"
        );
        let files = summary(
            Op::PlacesFiles,
            &json!({ "folder": "C:\\Pictures\\2019", "photos": 120, "videos": 4, "onlineOnly": 1, "files": [{}, {}] }),
        );
        assert!(
            files.starts_with("120 photos and 4 videos in C:\\Pictures\\2019; 2 given here."),
            "{files}"
        );
        assert!(
            files.ends_with("1 file is only in online storage, so Keevoro won't open it."),
            "{files}"
        );
    }

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
