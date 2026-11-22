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
        "Removes a backup from Keevoro, to put back for a week. No source or destination is touched",
        changes: OnlyWhenAsked, job: false, command_line: true, agents: Agents::No, implemented: true;
    BackupsRemoved = "backups.removed",
        "The backups removed in the last week, which can still be put back",
        changes: Never, job: false, command_line: true, agents: Agents::No, implemented: true;
    BackupsPutBack = "backups.put_back",
        "Puts back a backup removed in the last week, exactly as it was",
        changes: Always, job: false, command_line: true, agents: Agents::No, implemented: true;
    BackupsClearAway = "backups.clear_away",
        "Clears away a removed backup now, with its keys no other backup uses. It can't then be put back",
        changes: Always, job: false, command_line: true, agents: Agents::No, implemented: true;

    // Finding
    PlacesList = "places.list",
        "Drives plugged into this computer, and folders a cloud service keeps on it, such as OneDrive",
        changes: Never, job: false, command_line: true, agents: Agents::Yes, implemented: true;
    PlacesSurvey = "places.survey",
        "Finds the folders in a place that hold photos and videos, reading listings only",
        changes: Never, job: true, command_line: true, agents: Agents::Yes, implemented: true;
    PlacesFolders = "places.folders",
        "The folders inside a surveyed place that hold photos or videos, with how many each holds",
        changes: Never, job: false, command_line: true, agents: Agents::Yes, implemented: true;
    PlacesFiles = "places.files",
        "The photos and videos in one folder, a page at a time, without adding anything",
        changes: Never, job: false, command_line: true, agents: Agents::Yes, implemented: true;
    PlacesMap = "places.map",
        "Where the space goes in a surveyed place: its folders and files by size and kind",
        changes: Never, job: false, command_line: true, agents: Agents::Yes, implemented: true;
    PlacesForgetMaps = "places.forget_maps",
        "Forgets every remembered survey and map of where the space goes. Nothing on any drive is touched",
        changes: Always, job: false, command_line: true, agents: Agents::No, implemented: true;
    SourcesList = "sources.list",
        "A backup's sources: name, place, how many photos and videos, size",
        changes: Never, job: false, command_line: true, agents: Agents::Yes, implemented: true;
    SourcesLook = "sources.look",
        "Looks through a drive or folder and adds it as a source",
        changes: Always, job: true, command_line: true, agents: Agents::Yes, implemented: true;
    SourcesAddBucket = "sources.add_bucket",
        "Adds a bucket, or a folder in one, as a source that is only ever read",
        changes: Always, job: false, command_line: true, agents: Agents::Yes, implemented: true;
    SourcesLookAgain = "sources.look_again",
        "Looks through one source, or all of them, for what is new",
        changes: Always, job: true, command_line: true, agents: Agents::Yes, implemented: true;
    SourcesForget = "sources.forget",
        "Takes a source off the backup. Nothing on it is touched",
        changes: Always, job: false, command_line: true, agents: Agents::Yes, implemented: true;
    SourcesWants = "sources.wants",
        "What one source is for: its backup's choice of photos, videos and other files, or its own",
        changes: OnlyWhenAsked, job: false, command_line: true, agents: Agents::Yes, implemented: true;
    SourcesArrange = "sources.arrange",
        "How one source's files are arranged: as its backup does, by date, or keeping its folders",
        changes: OnlyWhenAsked, job: false, command_line: true, agents: Agents::Yes, implemented: true;
    SourcesFileTypes = "sources.file_types",
        "The kinds of file on a source, and which are included",
        changes: Never, job: false, command_line: true, agents: Agents::Yes, implemented: true;
    SourcesIncludeType = "sources.include_type",
        "Includes or leaves out one kind of file on a source",
        changes: Always, job: false, command_line: true, agents: Agents::Yes, implemented: true;
    SourcesLeaveOut = "sources.leave_out",
        "Leaves a folder out of a backup's searches, or puts it back. Nothing on it is touched",
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
    DuplicatesCheck = "duplicates.check",
        "Checks for duplicates now, in the background, even after it was stopped",
        changes: Always, job: true, command_line: true, agents: Agents::No, implemented: true;
    PhotosPicture = "photos.picture",
        "A small or screen-sized picture of a found file, or of any photo or video in a folder",
        changes: Never, job: false, command_line: true, agents: Agents::Yes, implemented: true;

    // Destinations
    DestinationsList = "destinations.list",
        "Where a backup goes, and what has reached each place",
        changes: Never, job: false, command_line: true, agents: Agents::Yes, implemented: true;
    DestinationsAdd = "destinations.add",
        "Checks a place works, then adds it. A folder, or cloud storage with its keys",
        changes: Always, job: false, command_line: true, agents: Agents::Yes, implemented: true;
    DestinationsRemove = "destinations.remove",
        "Takes a place off the backup. Nothing backed up there is touched",
        changes: Always, job: false, command_line: true, agents: Agents::Yes, implemented: true;
    DestinationsTidy = "destinations.tidy",
        "Clears, or first says, Keevoro's unfinished copies in a backup folder. Nothing backed up is touched",
        changes: OnlyWhenAsked, job: false, command_line: true, agents: Agents::No, implemented: true;
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
    BackupPutRight = "backup.put_right",
        "Puts a backup right: proves every copy, moves damaged ones aside, sends them again",
        changes: Always, job: true, command_line: true, agents: Agents::No, implemented: true;
    BackupPutRightPlan = "backup.put_right_plan",
        "What putting a backup right would do, and about how long it would take. Changes nothing",
        changes: Never, job: false, command_line: true, agents: Agents::No, implemented: true;
    BackupHistory = "backup.history",
        "How earlier backups went, each in full: stages, speeds, files sent and not, previews",
        changes: Never, job: false, command_line: true, agents: Agents::Yes, implemented: true;
    BackupLastSummary = "backup.last_summary",
        "What the last backup said when it ended",
        changes: Never, job: false, command_line: true, agents: Agents::Yes, implemented: true;

    // Backing up by itself
    ScheduleShow = "schedule.show",
        "When a backup runs by itself, when it runs next, and its last few runs",
        changes: Never, job: false, command_line: true, agents: Agents::Yes, implemented: true;
    ScheduleSet = "schedule.set",
        "Sets or stops a backup running by itself: every day, week or month at a time, or pauses it",
        changes: Always, job: false, command_line: true, agents: Agents::No, implemented: true;
    ScheduleSkipNext = "schedule.skip_next",
        "Skips a backup's next run by itself, or stops skipping it",
        changes: Always, job: false, command_line: true, agents: Agents::No, implemented: true;
    ScheduleRun = "schedule.run",
        "Backs up by itself: looks through sources again, holds back what needs your say, sends what is new",
        changes: Always, job: true, command_line: true, agents: Agents::No, implemented: true;
    BackupWants = "backup.wants",
        "What a backup is for: photos, videos, other files, or some of them, and changing it",
        changes: OnlyWhenAsked, job: false, command_line: true, agents: Agents::Yes, implemented: true;
    BackupArrange = "backup.arrange",
        "How a backup arranges what it sends: by date, or keeping the folders as they are, and changing it",
        changes: OnlyWhenAsked, job: false, command_line: true, agents: Agents::Yes, implemented: true;
    BackupPreviews = "backup.previews",
        "Whether previews are made, and switching that on or off",
        changes: OnlyWhenAsked, job: false, command_line: true, agents: Agents::Yes, implemented: true;

    // Jobs
    JobsStatus = "jobs.status",
        "What is running on a backup: the step, counts, how long it has run and has left, who started it",
        changes: Never, job: false, command_line: true, agents: Agents::Yes, implemented: true;
    JobsFollow = "jobs.follow",
        "Each thing a running job says, as it says it",
        changes: Never, job: false, command_line: true, agents: Agents::Instead(Op::JobsStatus), implemented: true;
    JobsPause = "jobs.pause",
        "Makes every heavy job wait after the file in hand, or only the check for duplicates",
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

    // The kinds of file looked for
    FileKindsList = "file_kinds.list",
        "The kinds of file Keevoro looks for, by ending: photo, video or other, built in or added, on or off",
        changes: Never, job: false, command_line: true, agents: Agents::Yes, implemented: true;
    FileKindsAdd = "file_kinds.add",
        "Adds an ending for Keevoro to look for, as a photo, a video or an other file",
        changes: Always, job: false, command_line: true, agents: Agents::No, implemented: true;
    FileKindsChange = "file_kinds.change",
        "Switches a kind of file on or off, makes it a photo, video or other file, or renames yours",
        changes: Always, job: false, command_line: true, agents: Agents::No, implemented: true;
    FileKindsRemove = "file_kinds.remove",
        "Takes an ending you added off the list. Nothing already found is taken away",
        changes: Always, job: false, command_line: true, agents: Agents::No, implemented: true;
    FileKindsReset = "file_kinds.reset",
        "Puts back the kinds of file Keevoro comes with, all on. Your own stay",
        changes: Always, job: false, command_line: true, agents: Agents::No, implemented: true;
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

/// The tool an AI agent is given for an operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tool {
    /// What the agent calls it, such as `list_backups`.
    pub name: &'static str,
    /// What the agent reads to decide when to use it: what it does, when
    /// to use it, what it gives back and what it will not do. The agent
    /// passes these words on to a person, so they are plain.
    pub tells: &'static str,
}

/// What an agent is told about choosing a backup, for every tool about one.
pub const NAMING_A_BACKUP: &str = "The backup to work on, by its name as list_backups gives it. Leave it out when \
     there is only one backup, or to use the one open in the Keevoro app.";

impl Op {
    /// True for an operation on one backup, which can be named. Without a
    /// name it is the one the app has open, which is the only one when
    /// there is one.
    pub const fn on_a_backup(self) -> bool {
        !matches!(
            self,
            Op::BackupsList
                | Op::BackupsStart
                | Op::BackupsRename
                | Op::BackupsForget
                | Op::BackupsRemoved
                | Op::BackupsPutBack
                | Op::BackupsClearAway
                | Op::PlacesList
                | Op::PlacesForgetMaps
                | Op::JobsPause
                | Op::JobsCarryOn
                | Op::KeevoroAbout
                | Op::KeevoroActivity
                | Op::KeevoroLog
                | Op::KeevoroSettings
                | Op::KeevoroUpdate
                | Op::KeevoroReset
                | Op::AgentsAccess
                | Op::DestinationsTidy
                | Op::FileKindsList
                | Op::FileKindsAdd
                | Op::FileKindsChange
                | Op::FileKindsRemove
                | Op::FileKindsReset
        )
    }

    /// True for an operation that loses something a person cannot simply
    /// put back, though never a photo: forgetting a source loses what was
    /// found there and chosen, removing a destination loses its saved keys.
    /// An AI app may ask its person before one of these.
    pub const fn loses_something(self) -> bool {
        matches!(
            self,
            Op::SourcesForget | Op::DestinationsRemove | Op::BackupsForget | Op::BackupsClearAway | Op::KeevoroReset
        )
    }

    /// Would this request change anything? Always for an operation that
    /// always changes something, never for one that only looks, and for
    /// one that looks unless asked to change something, only when asked.
    pub fn changes_with(self, request: &serde_json::Value) -> bool {
        let given = |field: &str| request.get(field).is_some_and(|v| !v.is_null());
        match self.entry().changes {
            Changes::Never => false,
            Changes::Always => true,
            Changes::OnlyWhenAsked => match self {
                Op::BackupPreviews => given("wanted"),
                Op::BackupWants | Op::SourcesWants => given("wants"),
                Op::BackupArrange | Op::SourcesArrange => given("arrange"),
                Op::KeevoroSettings => given("ownCodecs"),
                // Asked only what it would clear, it clears nothing.
                Op::DestinationsTidy | Op::BackupsForget => {
                    request.get("dryRun").and_then(serde_json::Value::as_bool) != Some(true)
                }
                Op::AgentsAccess => given("lookIn") || given("backUpTo") || given("mayChange"),
                // Through the API, only the check: installing is the app's.
                Op::KeevoroUpdate => false,
                // Anything else that may change is taken to change.
                _ => true,
            },
        }
    }

    /// What its request may hold, as a JSON Schema: made from the very
    /// type the API reads the request into, so the two cannot differ.
    pub fn request_schema(self) -> serde_json::Value {
        use crate::agents::AgentsAccessRequest as Agents;
        use crate::destinations::{AddDestinationRequest as Add, RemoveDestinationRequest as Remove};
        use crate::file_kinds::{AddFileKindRequest, ChangeFileKindRequest, FileKindRequest};
        use crate::keevoro::{
            ForgetBackupRequest, LogRequest, RemovedBackupRequest, RenameBackupRequest, SettingsRequest,
            StartBackupRequest,
        };
        use crate::picture::PictureRequest;
        use crate::{
            CollectionRequest, HistoryRequest, IncludeTypeRequest, JobRequest, KeepCollectionRequest, KeepPhotoRequest,
            LookAgainRequest, Nothing, PlaceRequest, PreviewsRequest, SourceRequest, StatusRequest,
        };
        match self {
            Op::BackupsStart => schema::<StartBackupRequest>(),
            Op::BackupsRename => schema::<RenameBackupRequest>(),
            Op::BackupsForget => schema::<ForgetBackupRequest>(),
            Op::BackupsPutBack | Op::BackupsClearAway => schema::<RemovedBackupRequest>(),
            Op::PlacesSurvey | Op::SourcesLook | Op::PlacesFolders => schema::<PlaceRequest>(),
            Op::PlacesFiles => schema::<crate::inside::FilesRequest>(),
            Op::PlacesMap => schema::<crate::space::MapRequest>(),
            Op::SourcesLookAgain => schema::<LookAgainRequest>(),
            Op::SourcesForget | Op::SourcesFileTypes => schema::<SourceRequest>(),
            Op::SourcesIncludeType => schema::<IncludeTypeRequest>(),
            Op::SourcesLeaveOut => schema::<crate::leave_out::LeaveOutRequest>(),
            Op::CollectionsPhotos => schema::<CollectionRequest>(),
            Op::CollectionsKeep => schema::<KeepCollectionRequest>(),
            Op::PhotosKeep => schema::<KeepPhotoRequest>(),
            Op::PhotosPicture => schema::<PictureRequest>(),
            Op::DestinationsAdd => schema::<Add>(),
            Op::SourcesAddBucket => schema::<crate::bucket_sources::AddBucketRequest>(),
            Op::DestinationsRemove => schema::<Remove>(),
            Op::BackupPreviews => schema::<PreviewsRequest>(),
            Op::JobsStatus => schema::<StatusRequest>(),
            Op::JobsFollow | Op::JobsStop | Op::JobsPause | Op::JobsCarryOn => schema::<JobRequest>(),
            Op::KeevoroLog => schema::<LogRequest>(),
            Op::KeevoroSettings => schema::<SettingsRequest>(),
            Op::AgentsAccess => schema::<Agents>(),
            Op::BackupHistory => schema::<HistoryRequest>(),
            Op::BackupPutRight => schema::<crate::PutRightRequest>(),
            Op::ScheduleRun => schema::<crate::RunByItselfRequest>(),
            Op::ScheduleSet => schema::<crate::schedule::SetScheduleRequest>(),
            Op::ScheduleSkipNext => schema::<crate::schedule::SkipNextRequest>(),
            Op::BackupWants => schema::<crate::wants::BackupWantsRequest>(),
            Op::BackupArrange => schema::<crate::arrange::BackupArrangeRequest>(),
            Op::SourcesArrange => schema::<crate::arrange::SourceArrangeRequest>(),
            Op::SourcesWants => schema::<crate::wants::SourceWantsRequest>(),
            Op::DestinationsTidy => schema::<crate::keevoro::TidyRequest>(),
            Op::FileKindsAdd => schema::<AddFileKindRequest>(),
            Op::FileKindsChange => schema::<ChangeFileKindRequest>(),
            Op::FileKindsRemove => schema::<FileKindRequest>(),
            Op::BackupsList
            | Op::BackupsRemoved
            | Op::PlacesList
            | Op::SourcesList
            | Op::FoundSummary
            | Op::CollectionsList
            | Op::SetAsideList
            | Op::DuplicatesList
            | Op::DestinationsList
            | Op::DestinationsRoom
            | Op::BackupRehearse
            | Op::BackupRun
            | Op::BackupCheck
            | Op::BackupPutRightPlan
            | Op::ScheduleShow
            | Op::BackupLastSummary
            | Op::DuplicatesCheck
            | Op::KeevoroAbout
            | Op::KeevoroActivity
            | Op::KeevoroUpdate
            | Op::KeevoroReset
            | Op::FileKindsList
            | Op::FileKindsReset
            | Op::PlacesForgetMaps => schema::<Nothing>(),
        }
    }

    /// The tool an AI agent is given for it, if any. Every operation
    /// offered to agents has one, and no other does (a test sees to it).
    pub const fn tool(self) -> Option<Tool> {
        let (name, tells) = match self {
            Op::BackupsList => (
                "list_backups",
                "Lists the person's backups: each one's name, how many sources and destinations it has, how many \
                 photos and videos it found, and how many files are still waiting to be backed up. Start here; the \
                 names it gives are what the other tools take as \"backup\". Changes nothing.",
            ),
            Op::BackupsStart => (
                "start_backup",
                "Starts a new, empty backup with the name given, and gives back its name and id. It has no sources \
                 or destinations yet: add them with look_through and add_destination.",
            ),
            Op::BackupsRename => (
                "rename_backup",
                "Gives a backup a new name. Nothing else changes: its sources, choices and destinations stay as they \
                 were.",
            ),
            Op::PlacesList => (
                "list_places",
                "Lists the drives plugged into this computer that could be looked through for photos, with their \
                 names and sizes, and the folders OneDrive, Dropbox, Google Drive and iCloud Drive keep on it, by \
                 name. Changes nothing.",
            ),
            Op::PlacesSurvey => (
                "survey_place",
                "Finds which folders in a drive or folder hold photos and videos. Reads folder listings only and \
                 opens no file, so it is quick. Use it to decide what to look through. It starts a job and gives \
                 back its id at once; ask job_status with that id, and waitSeconds, for the folders it found. \
                 Changes nothing. Only works in folders the person has allowed (see my_access).",
            ),
            Op::PlacesFolders => (
                "folders_inside",
                "Lists the folders inside a drive or folder that hold photos or videos themselves, with how many of \
                 each, their size, and how many are only in online storage, from what survey_place found there \
                 (survey it first). Use it to look inside a suggestion before choosing it. Changes nothing and adds \
                 nothing. Only works in folders the person has allowed (see my_access).",
            ),
            Op::PlacesFiles => (
                "files_in_folder",
                "Lists the photos and videos directly in one folder, a page at a time (from and count, at most \
                 500), by name or by the file's date (sort: name, newest or oldest; the date is when the file last \
                 changed, not when it was taken), all or only photos, videos or others (only): each one's path, \
                 size, whether it is a video (or an other file) and how many seconds it lasts, and whether it is only \
                 in online storage. Other files the person put on the kinds list are listed too. \
                 Give a path to show_picture as file to see one. Reads the folder's listing only; changes nothing \
                 and adds nothing to a backup. Only works in folders the person has allowed (see my_access).",
            ),
            Op::PlacesMap => (
                "where_the_space_goes",
                "Says where the space goes in a drive or folder survey_place has surveyed (survey it first), in \
                 the style of WinDirStat: every folder and the biggest files, photos, videos and everything else, \
                 each with its size and how many files, the biggest first, to a depth (1 to 6) and with smaller \
                 ones merged; and the files by ending. Give folder, inside place, to go into one. Says which \
                 folders Keevoro never looks through or made itself, and what the backup would leave out and \
                 why. Answered from the survey, so nothing is walked or opened; changes nothing. Only works in \
                 folders the person has allowed (see my_access).",
            ),
            Op::SourcesList => (
                "list_sources",
                "Lists the places a backup looks for photos (its sources), with each one's id, where it is, and how \
                 many photos and videos were found there. Changes nothing.",
            ),
            Op::SourcesLook => (
                "look_through",
                "Looks through a drive or folder for photos and videos and adds it to a backup as a source. Only \
                 reads: nothing on the drive or folder is changed, moved or deleted. It starts a job and gives back \
                 its id at once; follow it with job_status. Only works in folders the person has allowed (see \
                 my_access).",
            ),
            Op::SourcesLookAgain => (
                "look_again",
                "Looks through a backup's sources again, or just one, to find photos added since last time. Only \
                 reads the sources. It starts a job and gives back its id at once; follow it with job_status.",
            ),
            Op::SourcesAddBucket => (
                "add_bucket_source",
                "Adds cloud storage as a source of a backup: a bucket, or a folder in one (prefix), on Cloudflare \
                 R2 (kind \"r2\", place the account ID), Amazon S3 (kind \"s3\", place the region) or other \
                 S3-compatible storage (kind \"endpoint\", place its address), with its keys. The keys are tested \
                 by listing, which writes nothing, kept in the computer's keychain, and can never be read back. \
                 Nothing in the bucket is ever written. Looking through it and backing up from it are for the \
                 person: the folders an agent is allowed cannot hold a bucket.",
            ),
            Op::SourcesForget => (
                "forget_source",
                "Takes a source off a backup, by the id list_sources gives. Nothing on the source is changed or \
                 deleted, and what has already been backed up stays where it is.",
            ),
            Op::SourcesWants => (
                "source_wants",
                "Says what one source, by the id list_sources gives, is for: its backup's choice, or its own of \
                 photos, videos and other files. Give wants (backup, or a list of one or more of photos, videos and \
                 others) to change it. It counts at once on what was found, with no new search. Leaving a kind out \
                 only means they are not offered or backed up from now on; nothing already backed up is touched.",
            ),
            Op::SourcesArrange => (
                "source_arrange",
                "Says how one source's photos and videos, by the id list_sources gives, are arranged where they are \
                 sent: as its backup does, byDate, or folders (the same folders and names as on the source). Give \
                 arrange (backup, byDate or folders) to change it. Two sources whose folders have the same name share \
                 one folder in the backup, and the answer says so. Nothing already backed up is moved. It only \
                 changes where files go from now on.",
            ),
            Op::SourcesFileTypes => (
                "list_file_types",
                "Lists the kinds of file found on a source (by ending, such as jpg or mov), how many of each, and \
                 which are included. Changes nothing.",
            ),
            Op::SourcesIncludeType => (
                "include_file_type",
                "Includes, or leaves out, every file with one ending on a source. Leaving out only means those files \
                 are not backed up; nothing is deleted.",
            ),
            Op::SourcesLeaveOut => (
                "leave_out_folder",
                "Leaves a folder out of a backup, or with leftOut false puts it back. A folder left out is not \
                 looked through by that backup's searches, and what was found in it before is no longer offered. \
                 It may be a folder in no source yet. Nothing in the folder is changed and nothing already backed \
                 up is touched. Needs changes allowed, and only works in folders the person has allowed (see \
                 my_access).",
            ),
            Op::FoundSummary => (
                "summarise_backup",
                "Says what a backup holds: how many photos and videos (and other files, when the person asked for \
                 any), their size, the duplicates left out and the room that saves, and how many pictures were set \
                 aside as probably not photos. Changes nothing.",
            ),
            Op::CollectionsList => (
                "list_collections",
                "Lists a backup's collections (the folders and moments its photos fall into), newest first, with \
                 each one's key, how many files it holds, and whether it is being kept. Other files the person asked \
                 Keevoro to carry along come last, one per folder they came from (byFolder), each kept or left out \
                 as a whole with keep_collection. Changes nothing.",
            ),
            Op::CollectionsPhotos => (
                "list_photos",
                "Lists the photos and videos in one collection, or the files in a folder of other files, by the key \
                 list_collections gives, with each one's short id and whether it is being kept (an other file also \
                 has its size and when it last changed). Changes nothing.",
            ),
            Op::CollectionsKeep => (
                "keep_collection",
                "Keeps, or leaves out, a whole collection, by the key list_collections gives. Leaving out only means \
                 it is not backed up; nothing is deleted. Gives back how many files changed.",
            ),
            Op::PhotosKeep => (
                "keep_photo",
                "Keeps, or leaves out, one photo or video, by the short id list_photos gives. Nothing is deleted.",
            ),
            Op::SetAsideList => (
                "list_set_aside",
                "Lists pictures Keevoro set aside as probably not photos (screenshots, icons, tiny images), each with \
                 the reason. Any of them can be kept after all with keep_photo. Changes nothing.",
            ),
            Op::DuplicatesList => (
                "list_duplicates",
                "Lists files found more than once: the copy being kept and the copies left out. A copy is left out \
                 only when it matches byte for byte, never on a guess. Changes nothing.",
            ),
            Op::PhotosPicture => (
                "show_picture",
                "Gives back a picture of one photo, or a still from a video: by the short id list_photos gives \
                 (photo), or of any photo or video by the path files_in_folder gives (file), small or screen-sized, \
                 so you can look at it or show it to the person. A file only in online storage is never opened. \
                 Changes nothing; the picture is kept only in Keevoro's own cache. Only for files in folders the \
                 person has allowed (see my_access).",
            ),
            Op::DestinationsList => (
                "list_destinations",
                "Lists where a backup goes (folders and cloud storage), in order, and how many files have reached \
                 each. Never gives back storage keys. Changes nothing.",
            ),
            Op::DestinationsAdd => (
                "add_destination",
                "Adds a place to back up to, after checking it works: a folder (kind \"folder\", place the folder), \
                 or cloud storage (kind \"r2\", \"s3\" or \"endpoint\", with bucket and keys). Keys are stored in \
                 the computer's keychain and can never be read back by anyone. A folder must be one the person has \
                 allowed (see my_access). Nothing already stored there is changed.",
            ),
            Op::DestinationsRemove => (
                "remove_destination",
                "Stops backing up to one place, by its position in list_destinations (counting from 0), or to every \
                 place with all. Nothing backed up there is deleted; only Keevoro's own unfinished copies are cleared.",
            ),
            Op::DestinationsRoom => (
                "check_room",
                "Says how much each destination still needs, how much room it has, and whether that is enough. \
                 Changes nothing.",
            ),
            Op::BackupRehearse => (
                "rehearse_backup",
                "Says what backing up now would do: how many files, how much, and to where. Changes nothing. Use it \
                 before run_backup and tell the person what it says.",
            ),
            Op::BackupRun => (
                "run_backup",
                "Backs up everything being kept to every destination, originals first, and proves each copy exact. \
                 It can take hours, so it starts a job and gives back its id at once; follow it with job_status, and \
                 read last_summary when it ends. Never changes or deletes anything on a source, and never replaces \
                 or removes anything already backed up. Safe to stop and run again. Every folder it backs up to must \
                 be one the person has allowed (see my_access).",
            ),
            Op::BackupCheck => (
                "check_arrived",
                "Confirms that what should be at each destination is there. Changes nothing. It starts a job and \
                 gives back its id at once; follow it with job_status.",
            ),
            Op::BackupHistory => (
                "backup_history",
                "Lists how earlier backups went, newest first: when, to where, how much, how long and how fast, and, \
                 for backups since this was recorded, everything measured: how it ended and who started it, each \
                 stage with its time and speed, what each destination was sent, already had or could not be sent \
                 and why, the previews made, and the summary the person saw. Give run (1 for the newest) for one \
                 backup. Changes nothing.",
            ),
            Op::BackupLastSummary => (
                "last_summary",
                "Gives back what the last backup said when it finished, in the words the person would have seen, or \
                 nothing if it has never run. Changes nothing.",
            ),
            Op::BackupWants => (
                "backup_wants",
                "Says what a backup is for: photos, videos, and other files once the person has put a kind of them \
                 on the list (offersOthers). Give wants (a list of one or more of photos, videos and others) to \
                 change it. It counts at once on what was found, with no new search, and a \
                 source can have its own choice (see source_wants). Nothing already backed up is touched.",
            ),
            Op::BackupArrange => (
                "backup_arrange",
                "Says how a backup arranges photos and videos where it sends them: byDate (a folder for each year \
                 and month, keeping folders with real names) or folders (the same folders and names as on the \
                 source, starting with the source's own folder). Give arrange to change it. Nothing already backed \
                 up is moved. It only changes where files go from now on.",
            ),
            Op::BackupPreviews => (
                "set_previews",
                "Says whether a backup also makes previews for a gallery, or sends the originals alone. Give wanted \
                 to change it; leave it out to only ask.",
            ),
            Op::JobsStatus => (
                "job_status",
                "Says what is running on a backup, or on any: each job's id, the step it is on, how far it has got, \
                 how long it has run (runningSeconds) and who started it. For a backup, estimate says how long until \
                 the whole of it is done, every stage it has (done, now, alongside in the background, or still to \
                 come) and, in headline, both in the words a person sees. Give job for one job; once it has ended \
                 it says so (ended is true) with its last word in last, such as the folders a survey found. Give \
                 waitSeconds (up to 30) to wait for it to end before answering, rather than asking again and again. \
                 Changes nothing.",
            ),
            Op::JobsPause => (
                "pause_jobs",
                "Makes all of Keevoro's heavy work wait once the file in hand is finished, in every Keevoro program. \
                 Nothing is lost; carry_on lets it go on. Give job (from job_status) to pause only the background \
                 check for duplicates, while everything else carries on.",
            ),
            Op::JobsCarryOn => (
                "carry_on",
                "Lets paused work go on from where it was. Give job to let only a check for duplicates paused on its \
                 own go on.",
            ),
            Op::JobsStop => (
                "stop_job",
                "Stops a running job once the file in hand is finished, or every job on a backup. Everything done so \
                 far is kept, and running it again carries on from there.",
            ),
            Op::KeevoroAbout => (
                "about_keevoro",
                "Gives back Keevoro's version, the promises it keeps about people's photos, and its licence. Changes \
                 nothing.",
            ),
            Op::KeevoroActivity => (
                "activity",
                "Says what Keevoro is doing right now and how hard it is working the computer. Changes nothing.",
            ),
            Op::KeevoroSettings => (
                "read_settings",
                "Gives back Keevoro's settings, such as whether it uses this computer's own picture codecs. You \
                 cannot change them; the person can.",
            ),
            Op::KeevoroUpdate => (
                "check_for_update",
                "Says whether a newer version of Keevoro is available. You cannot install it; the person can.",
            ),
            Op::AgentsAccess => (
                "my_access",
                "Says which folders you may look through and back up to, and whether you may make changes. If \
                 something is refused, this explains why; only the person can change it.",
            ),
            Op::FileKindsList => (
                "list_file_kinds",
                "Lists the kinds of file Keevoro looks for on every source, by ending (such as jpg or mov): whether \
                 each is a photo, a video or an other file carried along, whether it came with Keevoro or the person \
                 added it, and whether it is \
                 switched on. A search only finds files of kinds switched on, and a survey only counts them. You \
                 cannot change the list; the person can, in Settings or with the keevoro command. Changes nothing.",
            ),
            Op::ScheduleShow => (
                "backup_schedule",
                "Says whether a backup backs up by itself, and when: every day, week or month at a time, whether it \
                 is paused, when it runs next, and how its last few runs went. You cannot set, skip or pause a \
                 schedule, or start a run by itself: a run by itself happens long after this conversation, with \
                 nobody watching, so only the person sets one up. Changes nothing.",
            ),
            Op::DuplicatesCheck
            | Op::BackupPutRight
            | Op::BackupPutRightPlan
            | Op::ScheduleRun
            | Op::ScheduleSet
            | Op::ScheduleSkipNext
            | Op::BackupsForget
            | Op::BackupsRemoved
            | Op::BackupsPutBack
            | Op::BackupsClearAway
            | Op::KeevoroLog
            | Op::KeevoroReset
            | Op::JobsFollow
            | Op::DestinationsTidy
            | Op::FileKindsAdd
            | Op::FileKindsChange
            | Op::FileKindsRemove
            | Op::FileKindsReset
            | Op::PlacesForgetMaps => return None,
        };
        Some(Tool { name, tells })
    }
}

/// A request type's JSON Schema, as plain JSON: every part written out in
/// place, with no references to follow, as the simplest readers expect.
fn schema<T: schemars::JsonSchema>() -> serde_json::Value {
    let generator = schemars::generate::SchemaSettings::draft2020_12()
        .with(|settings| settings.inline_subschemas = true)
        .into_generator();
    let mut schema = generator.into_root_schema_for::<T>().to_value();
    if let Some(object) = schema.as_object_mut() {
        // Its name in code and its doc comment are for developers.
        for field in ["$schema", "title", "description"] {
            object.remove(field);
        }
    }
    unwrap_descriptions(&mut schema);
    schema
}

/// A doc comment is wrapped to fit the code; a description is one line.
fn unwrap_descriptions(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(object) => {
            for (key, inner) in object.iter_mut() {
                match inner {
                    serde_json::Value::String(text) if key == "description" => *text = text.replace('\n', " "),
                    _ => unwrap_descriptions(inner),
                }
            }
        }
        serde_json::Value::Array(all) => all.iter_mut().for_each(unwrap_descriptions),
        _ => {}
    }
}

/// Every line of the register, in order.
pub fn entries() -> impl Iterator<Item = Entry> {
    Op::ALL.iter().map(|op| op.entry())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use std::collections::HashSet;

    #[test]
    fn the_register_has_every_operation_in_the_plan() {
        assert_eq!(Op::ALL.len(), 68);
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
    fn only_the_operations_the_plan_names_are_kept_from_agents() {
        let kept_from_agents: HashSet<Op> = entries()
            .filter(|e| matches!(e.agents, Agents::No | Agents::LookOnly))
            .map(|e| e.op)
            .collect();
        // Forgetting a backup, changing settings (the kinds of file looked
        // for among them), installing an update, resetting, changing an
        // agent's own permissions, clearing working files from a folder, and
        // forgetting the remembered maps.
        let plan: HashSet<Op> = [
            Op::PlacesForgetMaps,
            Op::BackupsForget,
            Op::BackupsRemoved,
            Op::BackupsPutBack,
            Op::BackupsClearAway,
            Op::DuplicatesCheck,
            // Putting a backup right moves damaged copies aside at a
            // destination: the person's say, not an agent's.
            Op::BackupPutRight,
            Op::BackupPutRightPlan,
            Op::KeevoroSettings,
            Op::KeevoroUpdate,
            Op::KeevoroReset,
            Op::AgentsAccess,
            Op::FileKindsAdd,
            Op::FileKindsChange,
            Op::FileKindsRemove,
            Op::FileKindsReset,
            Op::DestinationsTidy,
            // A backup that runs by itself runs long after an agent's
            // conversation has ended, with nobody watching: only the
            // person starts one, or sets, skips or pauses a schedule.
            Op::ScheduleRun,
            Op::ScheduleSet,
            Op::ScheduleSkipNext,
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
            "schedule.run",
            "backup.check",
            "backup.put_right",
            "duplicates.check",
        ]
        .into();
        assert_eq!(jobs, plan);
    }

    /// The tools the plan lists, in its order.
    const PLANNED_TOOLS: &[&str] = &[
        "list_backups",
        "start_backup",
        "rename_backup",
        "list_places",
        "survey_place",
        "folders_inside",
        "files_in_folder",
        "where_the_space_goes",
        "list_sources",
        "look_through",
        "add_bucket_source",
        "look_again",
        "forget_source",
        "source_wants",
        "source_arrange",
        "list_file_types",
        "include_file_type",
        "leave_out_folder",
        "summarise_backup",
        "list_collections",
        "list_photos",
        "keep_collection",
        "keep_photo",
        "list_set_aside",
        "list_duplicates",
        "show_picture",
        "list_destinations",
        "add_destination",
        "remove_destination",
        "check_room",
        "rehearse_backup",
        "run_backup",
        "check_arrived",
        "backup_history",
        "last_summary",
        "backup_schedule",
        "backup_wants",
        "backup_arrange",
        "set_previews",
        "job_status",
        "pause_jobs",
        "carry_on",
        "stop_job",
        "about_keevoro",
        "activity",
        "read_settings",
        "check_for_update",
        "my_access",
        "list_file_kinds",
    ];

    #[test]
    fn every_operation_offered_to_agents_has_a_tool_and_no_other_does() {
        for entry in entries() {
            let offered = matches!(entry.agents, Agents::Yes | Agents::LookOnly);
            assert_eq!(
                entry.op.tool().is_some(),
                offered,
                "{} is offered to agents as {:?}",
                entry.name,
                entry.agents
            );
        }
        let names: Vec<&str> = Op::ALL.iter().filter_map(|op| op.tool()).map(|t| t.name).collect();
        assert_eq!(
            names, PLANNED_TOOLS,
            "the tools, named and ordered as the plan has them"
        );
    }

    #[test]
    fn what_a_tool_tells_an_agent_is_plain_and_names_only_real_tools() {
        let names: HashSet<&str> = PLANNED_TOOLS.iter().copied().collect();
        for tool in Op::ALL.iter().filter_map(|op| op.tool()) {
            let tells = tool.tells;
            assert!(tells.ends_with('.'), "{}", tool.name);
            assert!(
                !tells.contains('\u{2014}') && !tells.contains('\u{2013}'),
                "{}",
                tool.name
            );
            assert!(!tells.contains("  "), "{}", tool.name);
            assert!(tells.len() >= 40, "{} says too little", tool.name);
            // Any word written like a tool's name is one.
            for word in tells.split(|c: char| !(c.is_ascii_lowercase() || c == '_')) {
                if word.contains('_') {
                    assert!(names.contains(word), "{} mentions {word}, which is no tool", tool.name);
                }
            }
            for american in ["summarize", "organize", "color", "favorite", "canceled"] {
                assert!(!tells.contains(american), "{}: {american}", tool.name);
            }
        }
    }

    #[test]
    fn every_request_is_described_as_an_object_with_nothing_unknown_allowed() {
        for op in Op::ALL {
            let schema = op.request_schema();
            assert_eq!(schema["type"], "object", "{}", op.name());
            assert_eq!(schema["additionalProperties"], false, "{}", op.name());
            assert!(
                schema.get("$ref").is_none() && schema.get("$defs").is_none(),
                "{}",
                op.name()
            );
            assert!(!schema.to_string().contains("$ref"), "{} has a reference", op.name());
            if op.on_a_backup() {
                assert!(
                    schema["properties"].get("backup").is_none(),
                    "{}: \"backup\" is how a backup is named, so no request may use it",
                    op.name()
                );
            }
        }
        let add = Op::DestinationsAdd.request_schema();
        assert_eq!(add["required"], serde_json::json!(["kind", "place"]));
        let kinds: Vec<&Value> = add["properties"]["kind"]["oneOf"]
            .as_array()
            .unwrap()
            .iter()
            .map(|kind| &kind["const"])
            .collect();
        assert_eq!(kinds, ["folder", "r2", "s3", "endpoint"]);
        assert!(
            !add.to_string().contains("\\n"),
            "a description wrapped in the code is one line here"
        );
        assert_eq!(
            add["properties"]["secretAccessKey"]["type"],
            serde_json::json!(["string", "null"])
        );
        let status = Op::JobsStatus.request_schema();
        assert!(status["properties"]["waitSeconds"]["description"]
            .as_str()
            .unwrap()
            .contains("30"));
    }

    #[test]
    fn only_what_changes_things_can_lose_something() {
        for entry in entries() {
            if entry.op.loses_something() {
                // Always, or unless only asked what it would do.
                assert_ne!(entry.changes, Changes::Never, "{}", entry.name);
            }
        }
    }

    #[test]
    fn a_request_changes_things_only_where_it_asks_to() {
        use serde_json::json;
        assert!(!Op::SourcesList.changes_with(&json!({})));
        assert!(Op::BackupRun.changes_with(&json!({})));
        assert!(!Op::BackupPreviews.changes_with(&json!({})));
        assert!(!Op::BackupPreviews.changes_with(&json!({ "wanted": null })));
        assert!(Op::BackupPreviews.changes_with(&json!({ "wanted": false })));
        assert!(!Op::KeevoroSettings.changes_with(&Value::Null));
        assert!(Op::KeevoroSettings.changes_with(&json!({ "ownCodecs": true })));
        assert!(!Op::AgentsAccess.changes_with(&json!({})));
        assert!(Op::AgentsAccess.changes_with(&json!({ "mayChange": true })));
        assert!(Op::AgentsAccess.changes_with(&json!({ "lookIn": [] })));
        assert!(!Op::KeevoroUpdate.changes_with(&json!({})));
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
