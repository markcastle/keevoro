//! Doing an operation for an AI agent: the person's rules, applied at the
//! moment of each call, and a line in the log for everything it does.
//!
//! An agent reaches Keevoro through its tools, and every tool call comes
//! here before it reaches [`crate::call`]. What a person has allowed
//! ([`crate::AgentAccess`]) is read afresh each time, so a change made in
//! the app or at the command line counts at once, even while the agent's
//! program is running. The rules:
//!
//! - Only operations the register offers to agents can be called. Those
//!   kept for a person (forgetting a backup, resetting Keevoro) cannot.
//! - An operation offered only to look ([`crate::Agents::LookOnly`]: the
//!   settings, updates, and what agents may do) takes no request at all,
//!   so an agent can never change its own permissions.
//! - Until a person allows changes, anything that would change something
//!   is refused with a sentence saying how they can allow it.
//! - Looking through, surveying, looking inside a folder, and making a
//!   picture of a file happen only in folders the person has allowed agents
//!   to look in; backing up,
//!   and checking what arrived, only to folders they have allowed agents to
//!   back up to. Each place is judged by where it really leads (see
//!   [`crate::agents`]). Cloud storage is not a folder: adding it is a
//!   change, so it needs changes allowed, and nothing more.
//! - Every call is written to the log: which tool, on which backup, and
//!   how it went. Never what the request held, so never a key.

use crate::agents::AgentAccess;
use crate::error::{Error, Kind, Result};
use crate::keevoro::Keevoro;
use crate::register::{Agents, Op};
use serde_json::Value;
use std::path::Path;

/// What a refusal says while agents may only look. The agent passes it on
/// to its person, so it says what they can do about it.
pub const LOOK_ONLY: &str = "Keevoro lets AI agents only look for now, not make changes. The person can allow changes \
     in the Keevoro app, under Settings, AI agents, or by typing: keevoro agents allow-changes on";

/// How a person allows a folder, for the end of a refusal.
const HOW_TO_ALLOW: &str = "The person can allow folders in the Keevoro app, under Settings, AI agents, or with \
     `keevoro agents allow`. my_access lists the folders allowed now.";

/// Does `op` for an AI agent, if the person's rules allow it, and writes
/// what happened to the log.
pub fn call_for_agent(keevoro: &Keevoro, op: Op, backup: Option<&str>, request: Value) -> Result<Value> {
    let keys = keys_in(&request);
    let outcome = allowed(keevoro, op, backup, &request).and_then(|()| crate::call(keevoro, op, backup, request));
    let outcome = outcome.map_err(|mut e| {
        e.message = without(&e.message, &keys);
        e
    });
    keevoro
        .log()
        .say(&without(&in_the_log(keevoro, op, backup, &outcome), &keys));
    outcome
}

/// Reads something the register offers an agent to read, not as a tool
/// (`keevoro.log`), and writes that it did to the log.
pub fn read_for_agent(keevoro: &Keevoro, op: Op) -> Result<Value> {
    if op.entry().agents != Agents::ToRead {
        return Err(Error::refused("That isn't something an AI agent can read."));
    }
    let read = crate::call(keevoro, op, None, Value::Null);
    let how = match &read {
        Ok(_) => "it worked".to_string(),
        Err(e) => format!("it didn't work. {}", e.message),
    };
    let what = match op {
        Op::KeevoroLog => "the log",
        _ => op.name(),
    };
    keevoro.log().say(&format!("An AI agent read {what}: {how}"));
    read
}

/// Would the person's rules allow this call? Says why not, if not.
fn allowed(keevoro: &Keevoro, op: Op, backup: Option<&str>, request: &Value) -> Result<()> {
    let Some(tool) = op.tool() else {
        return Err(Error::refused(
            "Only the person can do that, in the Keevoro app or with the keevoro command. It isn't offered to AI agents.",
        ));
    };
    if op.entry().agents == Agents::LookOnly && asks_for_anything(request) {
        return Err(Error::refused(format!(
            "{} only reads for an AI agent. Changing this is for the person, in the Keevoro app or with the keevoro command.",
            tool.name
        )));
    }
    let access = keevoro.agent_access();
    if op.changes_with(request) && !access.may_change {
        return Err(Error::refused(LOOK_ONLY));
    }
    match op {
        Op::PlacesSurvey | Op::SourcesLook | Op::PlacesFolders | Op::PlacesFiles => {
            may_look_in(&access, place(request)?)
        }
        // Both where the trail starts and the folder shown, so a remembered
        // survey of a whole drive says nothing outside the folders allowed.
        Op::PlacesMap => {
            may_look_in(&access, place(request)?)?;
            match text(request, "folder") {
                "" => Ok(()),
                folder => may_look_in(&access, folder),
            }
        }
        Op::SourcesLookAgain => {
            let one = request.get("source").and_then(Value::as_i64);
            let sources = keevoro.backups().open(backup)?.sources()?;
            for source in sources.iter().filter(|s| one.is_none_or(|id| s.id == id)) {
                may_look_in(&access, &source.root)?;
            }
            Ok(())
        }
        Op::PhotosPicture if !text(request, "file").is_empty() => may_look_in(&access, text(request, "file")),
        Op::SourcesLeaveOut => match text(request, "folder") {
            "" => Err(Error::not_understood("it names no folder (give one as folder)")),
            folder => may_look_in(&access, folder),
        },
        Op::PhotosPicture => {
            let photo = text(request, "photo");
            let found = keevoro.backups().open(backup)?.catalog().find_item(photo)?;
            match found {
                Some((file, _, _)) => may_look_in(&access, &file.display().to_string()),
                // Not known: the operation says so in its own words.
                None => Ok(()),
            }
        }
        Op::DestinationsAdd => match text(request, "kind") {
            "r2" | "s3" | "endpoint" => Ok(()),
            // A folder, or a kind not understood, which the operation will
            // refuse: either way the place is judged as a folder first.
            _ => may_back_up_to(&access, place(request)?),
        },
        Op::BackupRun | Op::BackupCheck => {
            for destination in keevoro.backups().open(backup)?.destinations()? {
                if let Some(folder) = &destination.folder {
                    may_back_up_to(&access, folder)?;
                }
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

/// The folder a request names. One that names none is told so plainly,
/// rather than being refused as if an empty folder were not allowed.
fn place(request: &Value) -> Result<&str> {
    match text(request, "place") {
        "" => Err(Error::not_understood("it names no folder (give one as place)")),
        named => Ok(named),
    }
}

fn text<'r>(request: &'r Value, field: &str) -> &'r str {
    request.get(field).and_then(Value::as_str).unwrap_or_default()
}

/// True when a request holds anything at all.
fn asks_for_anything(request: &Value) -> bool {
    match request {
        Value::Null => false,
        Value::Object(fields) => fields.values().any(|v| !v.is_null()),
        _ => true,
    }
}

fn may_look_in(access: &AgentAccess, place: &str) -> Result<()> {
    if access.may_look_in(Path::new(place)) {
        return Ok(());
    }
    Err(Error::refused(format!(
        "Keevoro won't let an AI agent look in \"{place}\": the person hasn't allowed that folder. {HOW_TO_ALLOW}"
    )))
}

fn may_back_up_to(access: &AgentAccess, place: &str) -> Result<()> {
    if access.may_back_up_to(Path::new(place)) {
        return Ok(());
    }
    Err(Error::refused(format!(
        "Keevoro won't let an AI agent back up to \"{place}\": the person hasn't allowed that folder. {HOW_TO_ALLOW}"
    )))
}

/// The storage keys a request carries, so that they can be taken out of
/// anything said about it.
fn keys_in(request: &Value) -> Vec<String> {
    ["accessKeyId", "secretAccessKey"]
        .iter()
        .filter_map(|field| request.get(field))
        .map(|key| match key {
            Value::String(text) => text.trim().to_string(),
            other => other.to_string(),
        })
        .filter(|key| !key.is_empty())
        .collect()
}

/// `text` with every key in it replaced.
fn without(text: &str, keys: &[String]) -> String {
    keys.iter()
        .fold(text.to_string(), |text, key| text.replace(key.as_str(), "[hidden]"))
}

/// One line for the log: which tool, on which backup, and how it went.
fn in_the_log(keevoro: &Keevoro, op: Op, backup: Option<&str>, outcome: &Result<Value>) -> String {
    let tool = op.tool().map_or(op.name(), |t| t.name);
    let on = if op.on_a_backup() {
        keevoro
            .backups()
            .named_or_open(backup)
            .map(|named| format!(" on backup \"{}\"", named.name))
            .unwrap_or_default()
    } else {
        String::new()
    };
    let how = match outcome {
        Ok(answer) if op.entry().job => match answer.get("id") {
            Some(id) => format!("started job {id}"),
            None => "it worked".to_string(),
        },
        Ok(_) => "it worked".to_string(),
        Err(e) if e.kind == Kind::Refused => format!("refused. {}", e.message),
        Err(e) => format!("it didn't work. {}", e.message),
    };
    format!("An AI agent used {tool}{on}: {how}")
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_request_that_names_no_folder_is_told_so_not_refused() {
        let e = super::place(&serde_json::json!({ "folder": "/somewhere" })).unwrap_err();
        assert_eq!(e.kind, super::Kind::NotUnderstood);
        assert!(e.message.contains("names no folder"), "{e}");
        assert_eq!(super::place(&serde_json::json!({ "place": "/a" })).unwrap(), "/a");
    }

    use super::*;
    use crate::agents::AgentsAccessRequest;
    use crate::backup::tests::card_looked_through_into;
    use crate::keevoro::tests::keevoro;
    use serde_json::json;
    use std::path::PathBuf;

    /// Keevoro, as an agent's program opens it, with one backup that has
    /// looked through a memory card.
    fn for_an_agent() -> (tempfile::TempDir, PathBuf, Keevoro) {
        let dir = tempfile::tempdir().unwrap();
        let keevoro = keevoro(dir.path()).as_front_end(crate::FrontEnd::Agent);
        let home = keevoro.start_backup("Card photos").unwrap().home;
        let card = card_looked_through_into(dir.path(), &home);
        (dir, card, keevoro)
    }

    fn allow(keevoro: &Keevoro, look_in: &[&Path], back_up_to: &[&Path], may_change: bool) {
        let all = |paths: &[&Path]| paths.iter().map(|p| p.display().to_string()).collect();
        keevoro
            .set_agent_access(AgentsAccessRequest {
                look_in: Some(all(look_in)),
                back_up_to: Some(all(back_up_to)),
                may_change: Some(may_change),
            })
            .unwrap();
    }

    fn refused(result: Result<Value>) -> String {
        let e = result.unwrap_err();
        assert_eq!(e.kind, Kind::Refused, "{e}");
        e.message
    }

    #[test]
    fn out_of_the_box_an_agent_may_look_and_nothing_more() {
        let (dir, card, keevoro) = for_an_agent();
        let k = &keevoro;
        // Looking at Keevoro's own record needs nothing allowed.
        for op in [
            Op::BackupsList,
            Op::SourcesList,
            Op::FoundSummary,
            Op::CollectionsList,
            Op::JobsStatus,
        ] {
            call_for_agent(k, op, None, json!({})).unwrap();
        }
        assert_eq!(call_for_agent(k, Op::BackupPreviews, None, json!({})).unwrap(), true);

        // Anything that changes something is refused, saying how to allow it.
        for (op, request) in [
            (Op::BackupsStart, json!({ "name": "Drone" })),
            (
                Op::CollectionsKeep,
                json!({ "collection": "Holidays / Cornwall", "keep": false }),
            ),
            (Op::BackupPreviews, json!({ "wanted": false })),
            (Op::JobsPause, json!({})),
            (Op::BackupRun, json!({})),
            (
                Op::DestinationsAdd,
                json!({ "kind": "folder", "place": dir.path().join("USB").display().to_string() }),
            ),
        ] {
            assert_eq!(
                refused(call_for_agent(k, op, None, request)),
                LOOK_ONLY,
                "{}",
                op.name()
            );
        }
        assert!(LOOK_ONLY.contains("keevoro agents allow-changes on"));
        assert_eq!(k.backups().all().len(), 1, "nothing was started");
        assert!(
            k.backups().open(None).unwrap().previews().unwrap(),
            "nothing was changed"
        );
        assert!(!keevoro_core::pause::is_paused());

        // And no folder may be looked in until a person names it.
        let e = refused(call_for_agent(
            k,
            Op::PlacesSurvey,
            None,
            json!({ "place": card.display().to_string() }),
        ));
        assert!(e.contains("the person hasn't allowed that folder"), "{e}");
        assert!(e.contains("keevoro agents allow"), "{e}");
    }

    #[test]
    fn what_is_kept_for_a_person_cannot_be_called_at_all() {
        let (_dir, _card, keevoro) = for_an_agent();
        allow(&keevoro, &[], &[], true);
        for op in [Op::BackupsForget, Op::KeevoroReset, Op::KeevoroLog, Op::JobsFollow] {
            let e = refused(call_for_agent(&keevoro, op, None, json!({ "backup": "Card photos" })));
            assert!(e.starts_with("Only the person can do that"), "{e}");
        }
        assert_eq!(keevoro.backups().all().len(), 1, "nothing was forgotten or reset");
    }

    #[test]
    fn an_agent_can_read_its_permissions_and_settings_but_never_change_them() {
        let (_dir, card, keevoro) = for_an_agent();
        allow(&keevoro, &[], &[], true);
        let k = &keevoro;
        let mine = call_for_agent(k, Op::AgentsAccess, None, json!({})).unwrap();
        assert_eq!(mine["mayChange"], true);
        for (op, request) in [
            (Op::AgentsAccess, json!({ "lookIn": [card.display().to_string()] })),
            (Op::AgentsAccess, json!({ "mayChange": false })),
            (Op::KeevoroSettings, json!({ "ownCodecs": false })),
            (Op::KeevoroUpdate, json!({ "install": true })),
        ] {
            let e = refused(call_for_agent(k, op, None, request));
            assert!(e.contains("only reads for an AI agent"), "{e}");
        }
        assert_eq!(k.agent_access().look_in, Vec::<String>::new());
        assert!(k.settings().own_codecs.on || !k.settings().own_codecs.available);
        call_for_agent(k, Op::KeevoroSettings, None, Value::Null).unwrap();
    }

    #[test]
    fn folders_are_allowed_one_by_one_and_judged_where_they_really_lead() {
        let (dir, card, keevoro) = for_an_agent();
        let k = &keevoro;
        let usb = dir.path().join("USB");
        let elsewhere = dir.path().join("Elsewhere");
        std::fs::create_dir_all(&usb).unwrap();
        std::fs::create_dir_all(&elsewhere).unwrap();
        allow(k, &[&card], &[&usb], true);

        // Outside, and climbing out with "..", is refused.
        for place in [
            elsewhere.clone(),
            dir.path().to_path_buf(),
            card.join("..").join("Elsewhere"),
            card.join("DCIM").join("..").join(".."),
        ] {
            let e = refused(call_for_agent(
                k,
                Op::PlacesSurvey,
                None,
                json!({ "place": place.display().to_string() }),
            ));
            assert!(e.contains("look in"), "{e}");
        }
        for place in [elsewhere.clone(), usb.join("..").join("Elsewhere")] {
            let e = refused(call_for_agent(
                k,
                Op::DestinationsAdd,
                None,
                json!({ "kind": "folder", "place": place.display().to_string() }),
            ));
            assert!(e.contains("back up to"), "{e}");
        }
        // Allowed to look in is not allowed to back up to.
        refused(call_for_agent(
            k,
            Op::DestinationsAdd,
            None,
            json!({ "kind": "folder", "place": card.join("Backup here").display().to_string() }),
        ));

        // Inside is allowed.
        let job = call_for_agent(
            k,
            Op::PlacesSurvey,
            None,
            json!({ "place": card.display().to_string() }),
        )
        .unwrap();
        keevoro.wait(crate::JobId(job["id"].as_u64().unwrap()));
        call_for_agent(
            k,
            Op::DestinationsAdd,
            None,
            json!({ "kind": "folder", "place": usb.join("Family").display().to_string() }),
        )
        .unwrap();
        // A picture of a file found in an allowed folder can be made.
        let photos = call_for_agent(
            k,
            Op::CollectionsPhotos,
            None,
            json!({ "collection": "Holidays / Cornwall" }),
        )
        .unwrap();
        let photo = photos[0]["id"].as_str().unwrap().to_string();
        call_for_agent(k, Op::PhotosPicture, None, json!({ "photo": photo })).unwrap();

        // Once the folder is no longer allowed, what depends on it is
        // refused at the moment it is asked for, though nothing else changed.
        allow(k, &[], &[], true);
        refused(call_for_agent(k, Op::PhotosPicture, None, json!({ "photo": photo })));
        // Named by its last part: the refusal gives the source's full real
        // path, which on Windows is not always spelled as it was typed.
        let e = refused(call_for_agent(k, Op::SourcesLookAgain, None, json!({})));
        assert!(e.contains("look in") && e.contains("Card"), "{e}");
        let e = refused(call_for_agent(k, Op::BackupRun, None, json!({})));
        assert!(e.contains("back up to"), "{e}");
        refused(call_for_agent(k, Op::BackupCheck, None, json!({})));
        keevoro.wait_for_jobs();
    }

    #[cfg(unix)]
    #[test]
    fn a_link_out_of_an_allowed_folder_is_refused() {
        use std::os::unix::fs::symlink;
        let (dir, card, keevoro) = for_an_agent();
        let secret = dir.path().join("Secret");
        std::fs::create_dir_all(&secret).unwrap();
        symlink(&secret, card.join("way out")).unwrap();
        allow(&keevoro, &[&card], &[&card], true);
        for op in [Op::PlacesSurvey, Op::SourcesLook] {
            refused(call_for_agent(
                &keevoro,
                op,
                None,
                json!({ "place": card.join("way out").display().to_string() }),
            ));
        }
        refused(call_for_agent(
            &keevoro,
            Op::DestinationsAdd,
            None,
            json!({ "kind": "folder", "place": card.join("way out").join("new").display().to_string() }),
        ));
    }

    #[test]
    fn an_agent_leaves_a_folder_out_only_with_changes_allowed_and_only_in_allowed_folders() {
        let (dir, card, keevoro) = for_an_agent();
        let elsewhere = dir.path().join("Elsewhere");
        std::fs::create_dir_all(&elsewhere).unwrap();
        let ask =
            |folder: &Path, left_out: bool| json!({ "folder": folder.display().to_string(), "leftOut": left_out });
        let cornwall = card.join("Holidays").join("Cornwall");

        allow(&keevoro, &[&card], &[], false);
        assert_eq!(
            refused(call_for_agent(
                &keevoro,
                Op::SourcesLeaveOut,
                None,
                ask(&cornwall, true)
            )),
            LOOK_ONLY,
            "look-only: no change"
        );
        allow(&keevoro, &[&card], &[], true);
        let outside = refused(call_for_agent(
            &keevoro,
            Op::SourcesLeaveOut,
            None,
            ask(&elsewhere, true),
        ));
        assert!(outside.contains("hasn't allowed that folder"), "{outside}");
        let e = call_for_agent(&keevoro, Op::SourcesLeaveOut, None, json!({ "leftOut": true })).unwrap_err();
        assert_eq!(e.kind, Kind::NotUnderstood);

        let done = call_for_agent(&keevoro, Op::SourcesLeaveOut, None, ask(&cornwall, true)).unwrap();
        assert_eq!(done["leftOut"], true);
        let back = call_for_agent(&keevoro, Op::SourcesLeaveOut, None, ask(&cornwall, false)).unwrap();
        assert_eq!(back["leftOut"], false);
        assert!(keevoro
            .backups()
            .open(None)
            .unwrap()
            .catalog()
            .left_out_folders()
            .unwrap()
            .is_empty());
    }

    #[test]
    fn cloud_storage_needs_changes_allowed_and_its_keys_are_said_nowhere() {
        const ACCESS: &str = "AKIAAGENTMARKER0001";
        const SECRET: &str = "agent-secret-marker/0002+xyz";
        let (dir, _card, keevoro) = for_an_agent();
        let request = json!({
            "kind": "endpoint",
            // Nothing listens here, so the storage is refused.
            "place": "http://127.0.0.1:9",
            "bucket": "family",
            "accessKeyId": ACCESS,
            "secretAccessKey": SECRET,
        });
        assert_eq!(
            refused(call_for_agent(&keevoro, Op::DestinationsAdd, None, request.clone())),
            LOOK_ONLY
        );
        allow(&keevoro, &[], &[], true);
        let e = call_for_agent(&keevoro, Op::DestinationsAdd, None, request).unwrap_err();
        assert_eq!(e.kind, Kind::Unreachable, "not a folder, so not judged as one: {e}");
        let said = format!("{e} {e:?}");
        assert!(!said.contains(ACCESS) && !said.contains(SECRET), "{said}");
        let log = std::fs::read_to_string(keevoro.log().path()).unwrap();
        assert!(!log.contains(ACCESS) && !log.contains(SECRET), "{log}");
        assert!(
            log.contains("An AI agent used add_destination on backup \"Card photos\""),
            "{log}"
        );
        drop(dir);
    }

    #[test]
    fn an_agent_can_read_the_log_and_nothing_else_that_way() {
        let (_dir, _card, keevoro) = for_an_agent();
        keevoro.log().say("Something worth knowing");
        let log = read_for_agent(&keevoro, Op::KeevoroLog).unwrap();
        assert!(log.as_str().unwrap().contains("Something worth knowing"));
        assert!(keevoro.log_tail(1).ends_with("An AI agent read the log: it worked"));
        for op in [Op::KeevoroReset, Op::SourcesList, Op::AgentsAccess] {
            refused(read_for_agent(&keevoro, op));
        }
        assert_eq!(keevoro.backups().all().len(), 1);
    }

    #[test]
    fn everything_an_agent_does_is_in_the_log_in_plain_words() {
        let (_dir, card, keevoro) = for_an_agent();
        let k = &keevoro;
        call_for_agent(k, Op::FoundSummary, Some("card photos"), json!({})).unwrap();
        refused(call_for_agent(k, Op::BackupRun, None, json!({})));
        allow(k, &[&card], &[], true);
        let job = call_for_agent(k, Op::SourcesLookAgain, None, json!({})).unwrap();
        keevoro.wait(crate::JobId(job["id"].as_u64().unwrap()));
        keevoro.wait_for_jobs();
        call_for_agent(k, Op::PhotosKeep, None, json!({ "photo": "0000", "keep": true })).unwrap_err();
        let log = keevoro.log_tail(50);
        for line in [
            "An AI agent used summarise_backup on backup \"Card photos\": it worked".to_string(),
            format!("An AI agent used run_backup on backup \"Card photos\": refused. {LOOK_ONLY}"),
            format!("An AI agent used look_again on backup \"Card photos\": started job {}", job["id"]),
            "An AI agent used keep_photo on backup \"Card photos\": it didn't work. That photo isn't known to this backup."
                .to_string(),
        ] {
            assert!(log.contains(&line), "{line}\n\nnot in:\n{log}");
        }
    }
}
