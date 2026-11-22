//! The app, the command line and an AI agent must be able to do the same
//! things. This test keeps the app's side of that honest.
//!
//! Every command the screens can call must be one of three things:
//!
//! - backed by operations in `keevoro-api`'s register, which the API does
//!   itself ([`BACKED`]);
//! - about being a window, with nothing to share ([`WINDOW_ONLY`]);
//! - still holding logic of its own, so the command line cannot reach it
//!   yet ([`NOT_YET`]).
//!
//! The list of commands is read from `generate_handler!` in `lib.rs` as the
//! test runs, so a new button cannot slip past. `NOT_YET` is the to-do list
//! for moving logic out of the shell. The work is done, so it is empty;
//! it is there for a command that is honestly waiting for the API.

use keevoro_api::Op;
use std::collections::{BTreeMap, BTreeSet};

/// Commands that only translate between the screens and the API.
const BACKED: &[(&str, &[Op])] = &[
    // Named backups. The window's list needs only names, which the API
    // gives without opening or counting each backup.
    ("app_state", &[Op::BackupsList]),
    ("start_backup", &[Op::BackupsStart]),
    // The Discover screen asks for all four in one go.
    (
        "discover",
        &[
            Op::FoundSummary,
            Op::SourcesList,
            Op::DestinationsList,
            Op::CollectionsList,
        ],
    ),
    ("places", &[Op::PlacesList]),
    ("forget_source", &[Op::SourcesForget]),
    ("file_types", &[Op::SourcesFileTypes]),
    ("include_file_type", &[Op::SourcesIncludeType]),
    ("leave_out_folder", &[Op::SourcesLeaveOut]),
    ("collection_photos", &[Op::CollectionsPhotos]),
    ("keep_collection", &[Op::CollectionsKeep]),
    ("keep_photo", &[Op::PhotosKeep]),
    ("set_aside", &[Op::SetAsideList]),
    ("duplicates", &[Op::DuplicatesList]),
    ("preview", &[Op::PhotosPicture]),
    ("closer_look", &[Op::PhotosPicture]),
    // Looking inside a folder before it is added.
    ("folders_inside", &[Op::PlacesFolders]),
    ("folder_files", &[Op::PlacesFiles]),
    ("file_picture", &[Op::PhotosPicture]),
    // Where the space goes in a surveyed place.
    ("space_map", &[Op::PlacesMap]),
    ("forget_maps", &[Op::PlacesForgetMaps]),
    // The one-line summary names the places without counting what has
    // reached each, which the API does quickly.
    ("destination", &[Op::DestinationsList]),
    ("connect_destination", &[Op::DestinationsAdd]),
    ("add_bucket_source", &[Op::SourcesAddBucket]),
    ("disconnect_destination", &[Op::DestinationsRemove]),
    ("room", &[Op::DestinationsRoom]),
    ("send_history", &[Op::BackupHistory]),
    ("last_backup_said", &[Op::BackupLastSummary]),
    ("previews_wanted", &[Op::BackupPreviews]),
    ("set_previews_wanted", &[Op::BackupPreviews]),
    ("activity", &[Op::KeevoroActivity]),
    ("log_tail", &[Op::KeevoroLog]),
    // Reporting a problem or an idea: made and shown, then sent as shown.
    ("prepare_report", &[Op::ReportsPrepare]),
    ("send_report", &[Op::ReportsSend]),
    ("reports", &[Op::ReportsList]),
    ("refresh_reports", &[Op::ReportsRefresh]),
    // When Keevoro cannot reach the reports service, the window does.
    ("note_report", &[Op::ReportsNote]),
    ("reports_asking", &[Op::ReportsAsking]),
    ("reports_heard", &[Op::ReportsHeard]),
    ("acceleration", &[Op::KeevoroSettings]),
    ("set_acceleration", &[Op::KeevoroSettings]),
    // What an AI agent may do: for the person, never the agent.
    ("agent_access", &[Op::AgentsAccess]),
    ("set_agent_access", &[Op::AgentsAccess]),
    // Photos and videos, photos only, or videos only: a backup's and a source's.
    ("backup_wants", &[Op::BackupWants]),
    ("set_backup_wants", &[Op::BackupWants]),
    ("backup_arrange", &[Op::BackupArrange]),
    ("set_backup_arrange", &[Op::BackupArrange]),
    ("source_arrange", &[Op::SourcesArrange]),
    ("set_source_arrange", &[Op::SourcesArrange]),
    ("source_wants", &[Op::SourcesWants]),
    ("set_source_wants", &[Op::SourcesWants]),
    // The kinds of file looked for: one list for the whole of Keevoro.
    ("file_kinds", &[Op::FileKindsList]),
    ("add_file_kind", &[Op::FileKindsAdd]),
    ("change_file_kind", &[Op::FileKindsChange]),
    ("remove_file_kind", &[Op::FileKindsRemove]),
    ("reset_file_kinds", &[Op::FileKindsReset]),
    // Managing backups: the list in full, renaming, removing with a week
    // to put back, and clearing away sooner.
    ("backup_details", &[Op::BackupsList]),
    ("rename_backup", &[Op::BackupsRename]),
    ("remove_backup", &[Op::BackupsForget]),
    ("removed_backups", &[Op::BackupsRemoved]),
    ("put_back_backup", &[Op::BackupsPutBack]),
    ("clear_away_backup", &[Op::BackupsClearAway]),
    // The drives plugged in, listed again only when the quick list changed.
    ("places_if_changed", &[Op::PlacesList]),
    ("check_update", &[Op::KeevoroUpdate]),
    ("reset_everything", &[Op::KeevoroReset]),
    // Clearing unfinished copies from a folder, first saying how much.
    ("tidy_folder", &[Op::DestinationsTidy]),
    // Jobs. Each command starts, asks after or stops one in the API, and
    // what the job says reaches the window through `jobs.follow`'s stream,
    // the listener set up in lib.rs.
    ("survey_place", &[Op::PlacesSurvey]),
    ("stop_survey", &[Op::JobsStop]),
    ("scan_place", &[Op::SourcesLook]),
    ("look_again", &[Op::SourcesLookAgain]),
    // Backing up by itself.
    ("schedule", &[Op::ScheduleShow]),
    ("away", &[Op::ScheduleAway]),
    ("source_on_arrival", &[Op::SourcesOnArrival]),
    ("background_settings", &[Op::BackgroundSettings]),
    ("set_schedule", &[Op::ScheduleSet]),
    ("skip_next_run", &[Op::ScheduleSkipNext]),
    ("run_scheduled_now", &[Op::ScheduleRun]),
    ("stop_scanning", &[Op::JobsStop]),
    // The second half of looking through a source: reading in full, in the
    // background, what the quick search only sampled. The API starts it
    // after every search and backup; the screens ask for it when they open.
    ("keep_checking", &[Op::SourcesLook]),
    // The check's own Pause and Stop, and "Check for duplicates now".
    ("pause_checking", &[Op::JobsPause, Op::JobsCarryOn]),
    ("stop_checking", &[Op::JobsStop]),
    ("check_duplicates_now", &[Op::DuplicatesCheck]),
    ("send", &[Op::BackupRun]),
    ("stop_sending", &[Op::JobsStop]),
    ("check_arrived", &[Op::BackupCheck]),
    ("put_right", &[Op::BackupPutRight]),
    ("put_right_plan", &[Op::BackupPutRightPlan]),
    ("send_progress", &[Op::JobsStatus]),
    ("pause_work", &[Op::JobsPause, Op::JobsCarryOn]),
];

/// Commands that are about being a window: nothing in them is Keevoro's to
/// share with the command line or an agent.
const WINDOW_ONLY: &[&str] = &[
    // The person is using the window, so a backup by itself waits until
    // they have finished. Only a window has a person in it.
    "window_used",
    // The person has read "While you were away": when the window last
    // showed it is the window's own to remember.
    "away_seen",
    // Which backup the window shows. The API keeps the choice in the
    // settings file, but it is not an operation: the command line and an
    // agent name the backup they mean each time.
    "switch_backup",
    // Shows the folder a backup goes to in the file manager.
    "open_destination",
    // Shows a source, or a folder in one, in the file manager.
    "open_place",
    // Opens the log file in the computer's text editor.
    "open_log",
    // Opens one of Keevoro's own web pages in the browser.
    "open_release_page",
    // Opens the comparison of cloud storage, or a provider's page it
    // named, in the browser. The command line and an agent have a browser
    // of their own.
    "open_web_page",
    // Downloads and starts the app's own installer, then closes the window.
    "install_update",
    // Keeping Keevoro running by the clock when its window is closed, and
    // minimising and starting with the computer: what a window does, and
    // the computer's list of programs it starts. Kept in `window.json`,
    // which only the app reads.
    "keep_running",
    "set_keep_running",
    // What waits for the window as it opens: a page, a question, a note.
    "window_ask",
    // The first-close explanation's OK: hide the window to the icon.
    "hide_to_tray",
    // Quitting the app, asking first with work in hand.
    "quit_keevoro",
];

/// Commands whose logic still lives in the shell, with the register
/// operation each will become, where there is one. Move one into the API,
/// and it moves from here to `BACKED`.
const NOT_YET: &[(&str, Option<Op>)] = &[];

/// The commands the app registers, read from `generate_handler!` itself.
fn registered_commands() -> Vec<String> {
    let source = include_str!("lib.rs");
    let start = source
        .find("generate_handler![")
        .expect("lib.rs should register its commands with generate_handler!")
        + "generate_handler![".len();
    let end = start + source[start..].find(']').expect("generate_handler! should end with ]");
    source[start..end]
        .split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(|name| name.rsplit("::").next().unwrap_or(name).to_string())
        .collect()
}

#[test]
fn the_commands_are_read_from_the_app_itself() {
    let commands = registered_commands();
    assert!(commands.len() > 40, "only found {commands:?}");
    assert!(commands.contains(&"discover".to_string()));
    assert!(commands.contains(&"check_arrived".to_string()));
    assert!(commands
        .iter()
        .all(|c| c.chars().all(|ch| ch.is_ascii_lowercase() || ch == '_')));
}

#[test]
fn every_command_is_backed_by_the_api_or_a_window_thing_or_honestly_not_yet() {
    let mut lists: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for (command, _) in BACKED {
        lists.entry(command).or_default().push("BACKED");
    }
    for command in WINDOW_ONLY {
        lists.entry(command).or_default().push("WINDOW_ONLY");
    }
    for (command, _) in NOT_YET {
        lists.entry(command).or_default().push("NOT_YET");
    }

    let commands = registered_commands();
    let unique: BTreeSet<&str> = commands.iter().map(String::as_str).collect();
    assert_eq!(unique.len(), commands.len(), "a command is registered twice");

    for command in &commands {
        match lists.get(command.as_str()).map(Vec::as_slice) {
            None => panic!(
                "The app has a command, {command}, that is on none of the lists. \
                 Do it in keevoro-api and add it to BACKED, or, if it is only about \
                 being a window, add it to WINDOW_ONLY."
            ),
            Some([_]) => {}
            Some(several) => panic!("{command} is on more than one list: {several:?}"),
        }
    }
    for listed in lists.keys() {
        assert!(
            unique.contains(listed),
            "{listed} is listed here but the app no longer has it; take it off"
        );
    }
}

#[test]
fn what_is_backed_is_really_done_by_the_api() {
    for (command, ops) in BACKED {
        assert!(!ops.is_empty(), "{command} names no operation");
        for op in *ops {
            assert!(
                op.entry().implemented,
                "{command} is said to be backed by {}, which the API does not do yet",
                op.name()
            );
        }
    }
}

#[test]
fn what_is_not_yet_done_is_not_already_done() {
    // An operation the API now does should be used, and its command moved
    // to BACKED.
    for (command, op) in NOT_YET {
        if let Some(op) = op {
            assert!(
                !op.entry().implemented,
                "{command} is waiting for {}, which the API now does. Use it, and move {command} to BACKED.",
                op.name()
            );
        }
    }
}
