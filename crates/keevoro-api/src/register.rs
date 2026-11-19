//! The register: every operation Keevoro offers, written down once.
//!
//! Each entry says what an operation is called, what it does in one plain
//! sentence, whether it changes anything, whether it runs for a while, and
//! where it is offered. The command line's help, the tools an AI agent sees
//! and the parity tests are all made from this list, so they cannot say
//! different things.
//!
//! The list and the [`Op`] enum are made by one macro from one table, so an
//! operation cannot exist without an entry, and an entry cannot exist
//! without an operation. Adding one is adding a line here; the compiler
//! then points at [`crate::call`], which must say what to do with it.

/// Whether an operation alters anything: Keevoro's own record of a backup,
/// or what is at a destination.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Changes {
    /// Only looks. Safe for an agent that may not make changes.
    Never,
    /// Always changes something.
    Always,
    /// Looks unless asked to change something: setting a choice, or
    /// installing an update.
    OnlyWhenAsked,
}

/// How an operation is offered to an AI agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Agents {
    /// As a tool, in full.
    Yes,
    /// As a tool that only looks. The part that changes something is for a
    /// person.
    LookOnly,
    /// As something to read, not a tool.
    ToRead,
    /// Not at all: it is for a person.
    No,
    /// Not as such: an agent asks this other operation instead.
    Instead(Op),
}

/// One line of the register.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry {
    pub op: Op,
    /// The stable name, such as `sources.list`.
    pub name: &'static str,
    /// What it does, in one plain sentence a person could read.
    pub does: &'static str,
    pub changes: Changes,
    /// True when it runs for a while and is started, watched and stopped
    /// rather than waited on.
    pub job: bool,
    /// True when the command line offers it.
    pub command_line: bool,
    pub agents: Agents,
    /// True once it is done here, in the API. Until then it is still done
    /// only by the app's shell, or not yet anywhere but the plan.
    pub implemented: bool,
}

macro_rules! register {
    ($(
        $op:ident = $name:literal, $does:literal,
        changes: $changes:ident, job: $job:literal, command_line: $cli:literal,
        agents: $agents:expr, implemented: $implemented:literal;
    )*) => {
        /// Every operation in the register, by name in code.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum Op {
            $($op,)*
        }

        impl Op {
            /// Every operation, in the register's order.
            pub const ALL: &'static [Op] = &[$(Op::$op,)*];

            /// This operation's line in the register.
            pub const fn entry(self) -> Entry {
                match self {
                    $(Op::$op => Entry {
                        op: Op::$op,
                        name: $name,
                        does: $does,
                        changes: Changes::$changes,
                        job: $job,
                        command_line: $cli,
                        agents: $agents,
                        implemented: $implemented,
                    },)*
                }
            }
        }
    };
}

register! {
    // Backups
    BackupsList = "backups.list",
        "Every backup: name, sources, destinations, how many files are safe and waiting",
        changes: Never, job: false, command_line: true, agents: Agents::Yes, implemented: true;
    BackupsStart = "backups.start",
        "Starts a new, empty backup with a name",
        changes: Always, job: false, command_line: true, agents: Agents::Yes, implemented: true;
    BackupsRename = "backups.rename",
        "Gives a backup a new name",
        changes: Always, job: false, command_line: true, agents: Agents::Yes, implemented: true;
    BackupsForget = "backups.forget",
        "Removes a backup from Keevoro. Nothing at any destination or source is touched",
        changes: Always, job: false, command_line: true, agents: Agents::No, implemented: true;

    // Finding
    PlacesList = "places.list",
        "Drives plugged into this computer",
        changes: Never, job: false, command_line: true, agents: Agents::Yes, implemented: true;
    PlacesSurvey = "places.survey",
        "Finds the folders in a place that hold photos and videos, reading listings only",
        changes: Never, job: true, command_line: true, agents: Agents::Yes, implemented: true;
    SourcesList = "sources.list",
        "A backup's sources: name, place, how many photos and videos, size",
        changes: Never, job: false, command_line: true, agents: Agents::Yes, implemented: true;
    SourcesLook = "sources.look",
        "Looks through a drive or folder and adds it as a source",
        changes: Always, job: true, command_line: true, agents: Agents::Yes, implemented: true;
    SourcesLookAgain = "sources.look_again",
        "Looks through one source, or all of them, for what is new",
        changes: Always, job: true, command_line: true, agents: Agents::Yes, implemented: true;
    SourcesForget = "sources.forget",
        "Takes a source off the backup. Nothing on it is touched",
        changes: Always, job: false, command_line: true, agents: Agents::Yes, implemented: true;
    SourcesFileTypes = "sources.file_types",
        "The kinds of file on a source, and which are included",
        changes: Never, job: false, command_line: true, agents: Agents::Yes, implemented: true;
    SourcesIncludeType = "sources.include_type",
        "Includes or leaves out one kind of file on a source",
        changes: Always, job: false, command_line: true, agents: Agents::Yes, implemented: true;

    // Choosing
    FoundSummary = "found.summary",
        "What a backup holds: counts, size, duplicates left out and room saved, set aside",
        changes: Never, job: false, command_line: true, agents: Agents::Yes, implemented: true;
    CollectionsList = "collections.list",
        "The collections, newest first, each kept or not",
        changes: Never, job: false, command_line: true, agents: Agents::Yes, implemented: true;
    CollectionsPhotos = "collections.photos",
        "The files in one collection, each kept or not",
        changes: Never, job: false, command_line: true, agents: Agents::Yes, implemented: true;
    CollectionsKeep = "collections.keep",
        "Keeps or leaves out a whole collection",
        changes: Always, job: false, command_line: true, agents: Agents::Yes, implemented: true;
    PhotosKeep = "photos.keep",
        "Keeps or leaves out one photo or video",
        changes: Always, job: false, command_line: true, agents: Agents::Yes, implemented: true;
    SetAsideList = "set_aside.list",
        "Pictures set aside as probably not photos, each with the reason",
        changes: Never, job: false, command_line: true, agents: Agents::Yes, implemented: true;
    DuplicatesList = "duplicates.list",
        "Files found more than once: the copy kept, the copies left out, and whether confirmed",
        changes: Never, job: false, command_line: true, agents: Agents::Yes, implemented: true;
    PhotosPicture = "photos.picture",
        "A small or screen-sized picture of one file",
        changes: Never, job: false, command_line: true, agents: Agents::Yes, implemented: true;

    // Destinations
    DestinationsList = "destinations.list",
        "Where a backup goes, and what has reached each place",
        changes: Never, job: false, command_line: true, agents: Agents::Yes, implemented: true;
    DestinationsAdd = "destinations.add",
        "Checks a place works, then adds it. A folder, or cloud storage with its keys",
        changes: Always, job: false, command_line: true, agents: Agents::Yes, implemented: true;
    DestinationsRemove = "destinations.remove",
        "Takes a place off the backup. Nothing stored there is touched",
        changes: Always, job: false, command_line: true, agents: Agents::Yes, implemented: true;
    DestinationsRoom = "destinations.room",
        "What is waiting for each place, and whether it has the room",
        changes: Never, job: false, command_line: true, agents: Agents::Yes, implemented: true;

    // Backing up
    BackupRehearse = "backup.rehearse",
        "What backing up now would do: how many files, how much, to where. Changes nothing",
        changes: Never, job: false, command_line: true, agents: Agents::Yes, implemented: true;
    BackupRun = "backup.run",
        "Backs up to every destination, originals first",
        changes: Always, job: true, command_line: true, agents: Agents::Yes, implemented: true;
    BackupCheck = "backup.check",
        "Confirms what is at each destination",
        changes: Never, job: true, command_line: true, agents: Agents::Yes, implemented: true;
    BackupHistory = "backup.history",
        "How earlier backups went: when, where, how much, how long, how fast",
        changes: Never, job: false, command_line: true, agents: Agents::Yes, implemented: true;
    BackupLastSummary = "backup.last_summary",
        "What the last backup said when it ended",
        changes: Never, job: false, command_line: true, agents: Agents::Yes, implemented: true;
    BackupPreviews = "backup.previews",
        "Whether previews are made, and switching that on or off",
        changes: OnlyWhenAsked, job: false, command_line: true, agents: Agents::Yes, implemented: true;

    // Jobs
    JobsStatus = "jobs.status",
        "What is running on a backup: the step, counts, speed, time left, who started it",
        changes: Never, job: false, command_line: true, agents: Agents::Yes, implemented: true;
    JobsFollow = "jobs.follow",
        "Each thing a running job says, as it says it",
        changes: Never, job: false, command_line: true, agents: Agents::Instead(Op::JobsStatus), implemented: true;
    JobsPause = "jobs.pause",
        "Makes every heavy job wait after the file in hand",
        changes: Always, job: false, command_line: true, agents: Agents::Yes, implemented: true;
    JobsCarryOn = "jobs.carry_on",
        "Lets paused jobs go on from where they were",
        changes: Always, job: false, command_line: true, agents: Agents::Yes, implemented: true;
    JobsStop = "jobs.stop",
        "Stops a job after the file in hand. Nothing is lost",
        changes: Always, job: false, command_line: true, agents: Agents::Yes, implemented: true;

    // Keevoro itself
    KeevoroAbout = "keevoro.about",
        "Version, what it promises, the licence",
        changes: Never, job: false, command_line: true, agents: Agents::Yes, implemented: true;
    KeevoroActivity = "keevoro.activity",
        "What Keevoro is doing now and how hard it is working",
        changes: Never, job: false, command_line: true, agents: Agents::Yes, implemented: true;
    KeevoroLog = "keevoro.log",
        "The record of what Keevoro did",
        changes: Never, job: false, command_line: true, agents: Agents::ToRead, implemented: true;
    KeevoroSettings = "keevoro.settings",
        "Reads or changes a setting, such as using the computer's own codecs",
        changes: OnlyWhenAsked, job: false, command_line: true, agents: Agents::LookOnly, implemented: true;
    KeevoroUpdate = "keevoro.update",
        "Whether there is a newer version, and installing it",
        changes: OnlyWhenAsked, job: false, command_line: true, agents: Agents::LookOnly, implemented: true;
    KeevoroReset = "keevoro.reset",
        "Puts Keevoro back as it was when first installed",
        changes: Always, job: false, command_line: true, agents: Agents::No, implemented: true;
    AgentsAccess = "agents.access",
        "Which folders an agent may use, and whether it may make changes",
        changes: OnlyWhenAsked, job: false, command_line: true, agents: Agents::LookOnly, implemented: true;
}

impl Op {
    /// The operation with this stable name, if there is one.
    pub fn named(name: &str) -> Option<Op> {
        Op::ALL.iter().copied().find(|op| op.entry().name == name)
    }

    /// Its stable name, such as `sources.list`.
    pub const fn name(self) -> &'static str {
        self.entry().name
    }
}

/// Every line of the register, in order.
pub fn entries() -> impl Iterator<Item = Entry> {
    Op::ALL.iter().map(|op| op.entry())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn the_register_has_every_operation_in_the_plan() {
        assert_eq!(Op::ALL.len(), 42);
        // Each entry is the one for its own operation.
        for op in Op::ALL {
            assert_eq!(op.entry().op, *op);
        }
    }

    #[test]
    fn names_are_unique_and_shaped_alike() {
        let mut seen = HashSet::new();
        for entry in entries() {
            assert!(seen.insert(entry.name), "{} is in the register twice", entry.name);
            let (group, action) = entry
                .name
                .split_once('.')
                .unwrap_or_else(|| panic!("{} should be group.action", entry.name));
            for part in [group, action] {
                assert!(
                    !part.is_empty() && part.chars().all(|c| c.is_ascii_lowercase() || c == '_'),
                    "{} should be lower case words joined by underscores",
                    entry.name
                );
            }
            assert_eq!(Op::named(entry.name), Some(entry.op));
        }
        assert_eq!(Op::named("sources.delete_everything"), None);
    }

    #[test]
    fn descriptions_are_one_plain_sentence() {
        for entry in entries() {
            let does = entry.does;
            assert!(!does.trim().is_empty(), "{} says nothing", entry.name);
            assert!(!does.contains('\u{2014}'), "{} has an em dash", entry.name);
            assert!(!does.contains('\u{2013}'), "{} has an en dash", entry.name);
            assert!(!does.contains('\n'), "{} runs over one line", entry.name);
            assert!(!does.ends_with('.'), "{} ends with a full stop", entry.name);
            assert!(
                does.chars().next().is_some_and(char::is_uppercase),
                "{} starts with a capital",
                entry.name
            );
            assert!(does.len() <= 100, "{} is more than one short line", entry.name);
            // House words: the places Keevoro looks are sources, not old drives.
            assert!(!does.to_lowercase().contains("old drive"), "{}", entry.name);
        }
    }

    #[test]
    fn only_the_five_operations_the_plan_names_are_kept_from_agents() {
        let kept_from_agents: HashSet<Op> = entries()
            .filter(|e| matches!(e.agents, Agents::No | Agents::LookOnly))
            .map(|e| e.op)
            .collect();
        // Forgetting a backup, changing settings, installing an update,
        // resetting, and changing an agent's own permissions.
        let plan: HashSet<Op> = [
            Op::BackupsForget,
            Op::KeevoroSettings,
            Op::KeevoroUpdate,
            Op::KeevoroReset,
            Op::AgentsAccess,
        ]
        .into();
        assert_eq!(kept_from_agents, plan);
    }

    #[test]
    fn what_an_agent_is_offered_is_consistent_with_what_changes() {
        for entry in entries() {
            match entry.agents {
                // Looking only makes sense where there is something else.
                Agents::LookOnly => assert_eq!(entry.changes, Changes::OnlyWhenAsked, "{}", entry.name),
                // Something to read changes nothing.
                Agents::ToRead => assert_eq!(entry.changes, Changes::Never, "{}", entry.name),
                // Pointing elsewhere must point at a tool an agent has, which
                // only looks, as the one it replaces does.
                Agents::Instead(other) => {
                    assert_eq!(other.entry().agents, Agents::Yes, "{}", entry.name);
                    assert_eq!(other.entry().changes, Changes::Never, "{}", entry.name);
                    assert_eq!(entry.changes, Changes::Never, "{}", entry.name);
                }
                Agents::Yes | Agents::No => {}
            }
        }
    }

    #[test]
    fn the_command_line_offers_everything() {
        for entry in entries() {
            assert!(entry.command_line, "{} is missing from the command line", entry.name);
        }
    }

    #[test]
    fn jobs_are_the_long_ones() {
        let jobs: HashSet<&str> = entries().filter(|e| e.job).map(|e| e.name).collect();
        let plan: HashSet<&str> = [
            "places.survey",
            "sources.look",
            "sources.look_again",
            "backup.run",
            "backup.check",
        ]
        .into();
        assert_eq!(jobs, plan);
    }

    #[test]
    fn nothing_that_changes_things_calls_itself_harmless() {
        for entry in entries() {
            if entry.does.contains("Changes nothing") {
                assert_eq!(entry.changes, Changes::Never, "{}", entry.name);
            }
        }
    }
}
