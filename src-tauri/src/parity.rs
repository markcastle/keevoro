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
//! for moving logic out of the shell. It must only ever get shorter, and
//! when the work is done it is empty.

use keevoro_api::Op;
use std::collections::{BTreeMap, BTreeSet};

/// Commands that only translate between the screens and the API.
const BACKED: &[(&str, &[Op])] = &[
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
    ("forget_source", &[Op::SourcesForget]),
    ("file_types", &[Op::SourcesFileTypes]),
    ("include_file_type", &[Op::SourcesIncludeType]),
    ("collection_photos", &[Op::CollectionsPhotos]),
    ("keep_collection", &[Op::CollectionsKeep]),
    ("keep_photo", &[Op::PhotosKeep]),
    ("set_aside", &[Op::SetAsideList]),
    ("duplicates", &[Op::DuplicatesList]),
    ("send_history", &[Op::BackupHistory]),
    ("last_backup_said", &[Op::BackupLastSummary]),
    ("previews_wanted", &[Op::BackupPreviews]),
    ("set_previews_wanted", &[Op::BackupPreviews]),
];

/// Commands that are about being a window: nothing in them is Keevoro's to
/// share with the command line or an agent.
const WINDOW_ONLY: &[&str] = &[
    // Shows the folder a backup goes to in the file manager.
    "open_destination",
    // Shows a source, or a folder in one, in the file manager.
    "open_place",
    // Shows the backup's own folder in the file manager.
    "open_library",
    // Opens the log file in the computer's text editor.
    "open_log",
    // Opens one of Keevoro's own web pages in the browser.
    "open_release_page",
    // Downloads and starts the app's own installer, then closes the window.
    "install_update",
];

/// Commands whose logic still lives in the shell, with the register
/// operation each will become, where there is one. Move one into the API,
/// and it moves from here to `BACKED`.
const NOT_YET: &[(&str, Option<Op>)] = &[
    // Named backups are kept in the app's settings.
    ("app_state", Some(Op::BackupsList)),
    ("start_backup", Some(Op::BackupsStart)),
    // Which backup the window has open is part of those settings.
    ("switch_backup", None),
    // The first-run screen of older builds. It goes with the library model.
    ("use_library", None),
    // Finding.
    ("places", Some(Op::PlacesList)),
    ("survey_place", Some(Op::PlacesSurvey)),
    ("scan_place", Some(Op::SourcesLook)),
    // Reading sampled files in full, in the background. Becomes a job.
    ("keep_checking", None),
    // Previews are made with the app's own cache and taken in turns.
    ("preview", Some(Op::PhotosPicture)),
    ("closer_look", Some(Op::PhotosPicture)),
    // Copying into the library. It goes with the library model, not moved.
    ("gather", None),
    // Destinations. The one-line summary reads only the saved list, which
    // is much quicker than `destinations.list` counting what has reached
    // each place, so it waits for a quick way to ask the API for names.
    ("destination", None),
    ("connect_destination", Some(Op::DestinationsAdd)),
    ("disconnect_destination", Some(Op::DestinationsRemove)),
    ("room", Some(Op::DestinationsRoom)),
    // Backing up to every destination and recording how it went.
    ("send", Some(Op::BackupRun)),
    ("check_arrived", Some(Op::BackupCheck)),
    // Jobs: what is running, pausing and stopping.
    ("send_progress", Some(Op::JobsStatus)),
    ("pause_work", Some(Op::JobsPause)),
    ("stop_sending", Some(Op::JobsStop)),
    ("stop_scanning", Some(Op::JobsStop)),
    ("stop_survey", Some(Op::JobsStop)),
    // Keevoro itself.
    ("activity", Some(Op::KeevoroActivity)),
    ("log_tail", Some(Op::KeevoroLog)),
    ("acceleration", Some(Op::KeevoroSettings)),
    ("set_acceleration", Some(Op::KeevoroSettings)),
    ("check_update", Some(Op::KeevoroUpdate)),
    ("reset_everything", Some(Op::KeevoroReset)),
];

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
