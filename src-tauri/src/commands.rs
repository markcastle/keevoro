//! The requests the screens can make, one function each.
//!
//! Every function here does the same three things: find the backup, call
//! `keevoro-api` (or, for what has not moved there yet, the engine), and
//! turn any error into a sentence the screen can show. Long jobs are the
//! API's too: a command starts one and returns, and what the job says
//! reaches the window through the listener set up in `lib.rs`. If a function grows
//! beyond that, the extra belongs in the API, where the command line gets
//! it too. `parity.rs` keeps count of what has still to move.

use crate::Shared;
use keevoro_api::jobs::SurveyView;
use keevoro_api::{Backup, JobKind, Named, Which};
use keevoro_catalog::Catalog;
use keevoro_engine::connect;
use keevoro_engine::{Discover, PhotoChoice};
use serde::Deserialize;
use serde::Serialize;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager, Runtime, State};

/// Errors cross to the screen as plain sentences.
type Said<T> = Result<T, String>;

fn say(e: impl std::fmt::Display) -> String {
    format!("{e:#}")
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppState {
    /// The open backup's own folder, or `None` before the first backup.
    library: Option<String>,
    /// The backup that is open.
    backup: Option<BackupView>,
    /// Every backup, in the order they were started.
    backups: Vec<BackupView>,
    version: &'static str,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupView {
    id: String,
    name: String,
}

fn current_state(shared: &Shared) -> AppState {
    let settings = shared.keevoro.backups().read();
    let view = |s: &keevoro_api::Named| BackupView {
        id: s.id.clone(),
        name: s.name.clone(),
    };
    AppState {
        library: settings.open_backup().map(|s| s.home.display().to_string()),
        backup: settings.open_backup().map(view),
        backups: settings.backups.iter().map(view).collect(),
        version: env!("CARGO_PKG_VERSION"),
    }
}

/// The backup the window has open.
fn open_backup(shared: &Shared) -> Said<Named> {
    shared
        .keevoro
        .backups()
        .current()
        .ok_or_else(|| "Start a backup first.".to_string())
}

fn library(shared: &Shared) -> Said<PathBuf> {
    open_backup(shared).map(|open| open.home)
}

/// The open backup, through the API that the command line uses too.
fn backup(shared: &Shared) -> Said<Backup> {
    Backup::open(&library(shared)?).map_err(say)
}

/// Starts a new backup with this name and opens it.
#[tauri::command]
pub fn start_backup(shared: State<'_, Shared>, name: String) -> Said<AppState> {
    let started = shared.keevoro.start_backup(&name).map_err(say)?;
    // The window shows the one just started.
    shared.keevoro.backups().switch_to(&started.id).map_err(say)?;
    Ok(current_state(&shared))
}

/// Opens another backup. Whatever was open is left exactly as it was.
#[tauri::command]
pub fn switch_backup(shared: State<'_, Shared>, id: String) -> Said<AppState> {
    // Only this window's own work holds it to the backup it has open.
    if shared.keevoro.busy_here() {
        return Err("Keevoro is in the middle of something in this backup. Let that finish first.".into());
    }
    if !shared.keevoro.switch_to(&id).map_err(say)? {
        return Err("That backup is no longer there.".into());
    }
    check_in_background(&shared);
    Ok(current_state(&shared))
}

/// Puts Keevoro back as it was when first installed: every backup is
/// forgotten, along with what it found and chose, the previews, and any
/// storage keys in the keychain. Nothing on a source, nothing that was sent
/// anywhere, and no folder outside Keevoro's own data is touched.
#[tauri::command]
pub fn reset_everything(shared: State<'_, Shared>) -> Said<AppState> {
    // The API refuses while any of Keevoro's programs is working.
    shared.keevoro.reset().map_err(say)?;
    Ok(current_state(&shared))
}

/// Shows the folder this backup sends to, if it sends to a folder.
#[tauri::command]
pub fn open_destination<R: Runtime>(app: AppHandle<R>, shared: State<'_, Shared>, at: Option<usize>) -> Said<()> {
    use tauri_plugin_opener::OpenerExt;
    let catalog = Catalog::open_in_library(&library(&shared)?).map_err(say)?;
    // The one asked for, by its place in the list; the first otherwise.
    match connect::destinations(&catalog)
        .map_err(say)?
        .into_iter()
        .nth(at.unwrap_or(0))
    {
        Some(saved) if saved.kind == "folder" => app.opener().open_path(saved.place, None::<&str>).map_err(say),
        _ => Err("This backup doesn't send to a folder on this computer.".to_string()),
    }
}

#[tauri::command]
pub fn app_state(shared: State<'_, Shared>) -> AppState {
    current_state(&shared)
}

#[tauri::command]
pub fn discover(shared: State<'_, Shared>) -> Said<Discover> {
    backup(&shared)?.discover().map_err(say)
}

#[tauri::command]
pub fn keep_collection(shared: State<'_, Shared>, key: String, keep: bool) -> Said<()> {
    backup(&shared)?.keep_collection(&key, keep).map_err(say)?;
    Ok(())
}

/// Every file in one collection, for ticking and unticking.
#[tauri::command]
pub fn collection_photos(shared: State<'_, Shared>, key: String) -> Said<Vec<PhotoChoice>> {
    backup(&shared)?.collection_photos(&key).map_err(say)
}

/// Stops remembering a source. Its files are never touched.
#[tauri::command]
pub fn forget_source(shared: State<'_, Shared>, id: i64) -> Said<()> {
    backup(&shared)?.forget_source(id).map_err(say)?;
    shared.keevoro.log().say(&format!(
        "Removed source {id} from the list (its files were not touched)"
    ));
    Ok(())
}

/// The kinds of file found on a source, most numerous first.
#[tauri::command]
pub fn file_types(shared: State<'_, Shared>, id: i64) -> Said<Vec<keevoro_api::FileTypeView>> {
    backup(&shared)?.file_types(id).map_err(say)
}

/// Includes or leaves out every file of one kind on one source. Nothing on
/// the source is touched; left-out files are simply not offered or sent.
#[tauri::command]
pub fn include_file_type(shared: State<'_, Shared>, id: i64, ending: String, included: bool) -> Said<()> {
    backup(&shared)?.include_type(id, &ending, included).map_err(say)?;
    shared.keevoro.log().say(&format!(
        "{} .{ending} files on source {id}",
        if included { "Including" } else { "Leaving out" }
    ));
    Ok(())
}

/// Leaves a folder out of the open backup, or puts it back, and says what
/// that means. Nothing in the folder, or already backed up, is touched.
#[tauri::command]
pub fn leave_out_folder(
    shared: State<'_, Shared>,
    folder: String,
    left_out: bool,
) -> Said<keevoro_api::leave_out::LeftOutFolder> {
    let done = backup(&shared)?.leave_out(Path::new(&folder), left_out).map_err(say)?;
    shared.keevoro.log().say(&done.said);
    Ok(done)
}

/// Every file found more than once: which copy is kept, which are left out.
#[tauri::command]
pub fn duplicates(shared: State<'_, Shared>) -> Said<Vec<keevoro_engine::DuplicateGroup>> {
    backup(&shared)?.duplicates().map_err(say)
}

/// The pictures set aside as probably not memories, with the reason.
#[tauri::command]
pub fn set_aside(shared: State<'_, Shared>) -> Said<Vec<PhotoChoice>> {
    backup(&shared)?.set_aside().map_err(say)
}

#[tauri::command]
pub fn keep_photo(shared: State<'_, Shared>, id: String, keep: bool) -> Said<()> {
    backup(&shared)?.keep_photo(&id, keep).map_err(say)
}

/// A small picture of one found file, as raw WebP bytes.
#[tauri::command]
pub async fn preview<R: Runtime>(
    app: AppHandle<R>,
    shared: State<'_, Shared>,
    id: String,
) -> Said<tauri::ipc::Response> {
    picture(app, &shared, id, keevoro_api::PictureSize::Small).await
}

/// A screen-sized picture of one found file, for looking at it closely.
#[tauri::command]
pub async fn closer_look<R: Runtime>(
    app: AppHandle<R>,
    shared: State<'_, Shared>,
    id: String,
) -> Said<tauri::ipc::Response> {
    picture(app, &shared, id, keevoro_api::PictureSize::Screen).await
}

async fn picture<R: Runtime>(
    app: AppHandle<R>,
    shared: &State<'_, Shared>,
    id: String,
    size: keevoro_api::PictureSize,
) -> Said<tauri::ipc::Response> {
    let library = library(shared)?;
    // Making a picture reads a file and shrinks it; keep that off the
    // thread that draws the window. The API takes turns, so a screen that
    // asks for all its previews at once does not swamp the computer.
    let bytes = tauri::async_runtime::spawn_blocking(move || {
        let backup = Backup::open(&library)?;
        app.state::<Shared>().keevoro.picture(&backup, &id, size)
    })
    .await
    .map_err(say)?
    .map_err(say)?;
    Ok(tauri::ipc::Response::new(bytes))
}

/// The folders inside a surveyed place that hold photos or videos, from
/// the survey Keevoro remembers: the outline of a look inside.
#[tauri::command]
pub fn folders_inside(shared: State<'_, Shared>, place: String) -> Said<Option<keevoro_api::inside::FoldersView>> {
    // `None` when no survey covers it yet: the screen surveys it first.
    let backup = shared.keevoro.backups().current();
    shared
        .keevoro
        .folders_inside_if_surveyed(Path::new(&place), backup.as_ref())
        .map_err(say)
}

/// One page of the photos and videos directly in a folder, sorted and
/// shown as asked.
#[tauri::command]
pub async fn folder_files<R: Runtime>(
    app: AppHandle<R>,
    request: keevoro_api::inside::FilesRequest,
) -> Said<keevoro_api::inside::FilesView> {
    // A folder on a slow drive can take a moment to list.
    tauri::async_runtime::spawn_blocking(move || {
        let keevoro = &app.state::<Shared>().keevoro;
        let backup = keevoro.backups().current();
        keevoro.files_in(&request, backup.as_ref())
    })
    .await
    .map_err(say)?
    .map_err(say)
}

/// A small or screen-sized picture of any photo or video in a folder being
/// looked inside, as raw WebP bytes. Only Keevoro's cache is written.
#[tauri::command]
pub async fn file_picture<R: Runtime>(app: AppHandle<R>, path: String, large: bool) -> Said<tauri::ipc::Response> {
    let size = if large {
        keevoro_api::PictureSize::Screen
    } else {
        keevoro_api::PictureSize::Small
    };
    let bytes = tauri::async_runtime::spawn_blocking(move || {
        let keevoro = &app.state::<Shared>().keevoro;
        let backup = keevoro.backups().current();
        keevoro.picture_of_file(Path::new(&path), size, backup.as_ref())
    })
    .await
    .map_err(say)?
    .map_err(say)?;
    Ok(tauri::ipc::Response::new(bytes))
}

/// Reads, in the background, the files a quick search only sampled, so
/// that true duplicates are known and a backup has less to do. Does
/// nothing if a pass is already running. It gives way to a search or a
/// backup, and the API starts it again after each.
fn check_in_background(shared: &Shared) {
    if let Ok(open) = open_backup(shared) {
        shared.keevoro.keep_checking(&open);
    }
}

/// Asked by the screens when they open: carry on with any checking left
/// over from last time.
#[tauri::command]
pub fn keep_checking(shared: State<'_, Shared>) {
    check_in_background(&shared);
}

/// Whether Keevoro is using the computer's own codecs, and what that means
/// on this computer.
#[tauri::command]
pub fn acceleration(shared: State<'_, Shared>) -> keevoro_api::Codecs {
    shared.keevoro.settings().own_codecs
}

/// Switches the use of the computer's own codecs on or off, and remembers.
#[tauri::command]
pub fn set_acceleration(shared: State<'_, Shared>, on: bool) -> Said<keevoro_api::Codecs> {
    Ok(shared.keevoro.set_own_codecs(on).map_err(say)?.own_codecs)
}

/// What an AI agent may do, as the person has allowed: the folders it may
/// look through and back up to, and whether it may make changes.
#[tauri::command]
pub fn agent_access(shared: State<'_, Shared>) -> keevoro_api::AgentAccess {
    shared.keevoro.agent_access()
}

/// Changes what an AI agent may do. Each part given replaces what was there.
#[tauri::command]
pub fn set_agent_access(
    shared: State<'_, Shared>,
    change: keevoro_api::AgentsAccessRequest,
) -> Said<keevoro_api::AgentAccess> {
    shared.keevoro.set_agent_access(change).map_err(say)
}

/// Whether this backup is for photos and videos, photos only, or videos only.
#[tauri::command]
pub fn backup_wants(shared: State<'_, Shared>) -> Said<keevoro_api::wants::WantsView> {
    backup(&shared)?.wants().map_err(say)
}

/// Makes this backup for photos and videos, photos only, or videos only.
#[tauri::command]
pub fn set_backup_wants(
    shared: State<'_, Shared>,
    wants: keevoro_api::wants::WantsChoice,
) -> Said<keevoro_api::wants::WantsView> {
    let backup = backup(&shared)?;
    shared.keevoro.set_backup_wants(&backup, wants.into()).map_err(say)
}

/// Whether one source follows its backup's choice of photos or videos.
#[tauri::command]
pub fn source_wants(shared: State<'_, Shared>, id: i64) -> Said<keevoro_api::wants::WantsView> {
    backup(&shared)?.source_wants(id).map_err(say)
}

/// Gives one source its own choice of photos or videos, or the backup's.
#[tauri::command]
pub fn set_source_wants(
    shared: State<'_, Shared>,
    id: i64,
    wants: keevoro_api::wants::SourceWantsChoice,
) -> Said<keevoro_api::wants::WantsView> {
    let backup = backup(&shared)?;
    shared.keevoro.set_source_wants(&backup, id, wants.into()).map_err(say)
}

/// The kinds of file Keevoro looks for, the built-in ones first.
#[tauri::command]
pub fn file_kinds(shared: State<'_, Shared>) -> Vec<keevoro_api::FileKind> {
    shared.keevoro.file_kinds()
}

/// Adds an ending for Keevoro to look for, as photos or videos.
#[tauri::command]
pub fn add_file_kind(
    shared: State<'_, Shared>,
    ending: String,
    kind: keevoro_api::PhotoOrVideo,
) -> Said<keevoro_api::FileKind> {
    shared.keevoro.add_file_kind(&ending, kind.into()).map_err(say)
}

/// Changes one kind: photo or video, on or off, or, for one the person
/// added, its ending. Anything not given stays as it is.
#[tauri::command]
pub fn change_file_kind(
    shared: State<'_, Shared>,
    ending: String,
    kind: Option<keevoro_api::PhotoOrVideo>,
    on: Option<bool>,
    to: Option<String>,
) -> Said<keevoro_api::FileKind> {
    let change = keevoro_api::KindChange {
        kind: kind.map(Into::into),
        on,
        ending: to,
    };
    shared.keevoro.change_file_kind(&ending, &change).map_err(say)
}

/// Takes an ending the person added off the list.
#[tauri::command]
pub fn remove_file_kind(shared: State<'_, Shared>, ending: String) -> Said<keevoro_api::FileKind> {
    shared.keevoro.remove_file_kind(&ending).map_err(say)
}

/// Puts back the kinds Keevoro comes with, all on. The person's own stay.
#[tauri::command]
pub fn reset_file_kinds(shared: State<'_, Shared>) -> Said<Vec<keevoro_api::FileKind>> {
    shared.keevoro.reset_file_kinds().map_err(say)
}

/// What Keevoro is doing now, why, and how hard it has been working.
#[tauri::command]
pub fn activity(shared: State<'_, Shared>) -> keevoro_api::activity::ActivityView {
    shared.keevoro.activity()
}

/// This backup's earlier sends, newest first: when, how much, how long.
#[tauri::command]
pub fn send_history(shared: State<'_, Shared>) -> Said<Vec<keevoro_engine::history::Sent>> {
    backup(&shared)?.history().map_err(say)
}

/// How a send is getting on: how long it has run, and for a backup the one
/// estimate of how long the whole of it has left, stage by stage.
#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SendProgress {
    /// Seconds since it started, from the job itself, so coming back to
    /// the screen does not start the count again.
    running_seconds: u64,
    /// For a backup; `None` for a check.
    estimate: Option<keevoro_api::estimate::Estimate>,
    /// True when this send goes over the network, to cloud storage.
    over_network: bool,
    /// True while a send or a check is under way, so that a screen opened
    /// part-way through can say so at once instead of offering to start one.
    running: bool,
    /// True once that send has been asked to stop and is finishing its file.
    stopping: bool,
    /// What it last said it was doing, in the same words the screen shows
    /// as it runs. Empty when nothing is running.
    line: String,
}

/// The running backup or check, as `jobs.status` tells it. Only this
/// app's own: the screens hear what its jobs say as they say it, and have
/// no way yet to follow one another program is running.
#[tauri::command]
pub fn send_progress(shared: State<'_, Shared>) -> SendProgress {
    shared
        .keevoro
        .jobs(None)
        .into_iter()
        .find(|job| !job.elsewhere && matches!(job.handle.job, JobKind::BackUp | JobKind::Check))
        .map(|job| SendProgress {
            running_seconds: job.running_seconds,
            estimate: job.estimate,
            over_network: job.over_network,
            running: true,
            stopping: job.stopping,
            line: job.line,
        })
        .unwrap_or_default()
}

/// Pauses or resumes every heavy job. Each waits at its next safe point,
/// between one file and the next, and carries on from there when resumed.
#[tauri::command]
pub fn pause_work(shared: State<'_, Shared>, paused: bool) {
    if paused {
        shared.keevoro.pause();
    } else {
        shared.keevoro.carry_on();
    }
}

/// Asks a search that is running to stop. It ends within a moment and
/// nothing from it is kept: a part-search would make the rest of the source
/// look as if it were not there.
#[tauri::command]
pub fn stop_scanning(shared: State<'_, Shared>) {
    if let Ok(open) = open_backup(&shared) {
        shared
            .keevoro
            .stop(Which::KindsOn(&[JobKind::Look, JobKind::LookAgain], &open.id));
    }
}

/// Finds which folders under `path` hold photos and videos, by reading
/// folder listings only. No file is opened and nothing is remembered: the
/// answer is a list of suggestions for the person to choose from.
#[tauri::command]
pub async fn survey_place<R: Runtime>(app: AppHandle<R>, shared: State<'_, Shared>, path: String) -> Said<SurveyView> {
    let open = shared.keevoro.backups().current();
    let job = shared
        .keevoro
        .survey(Path::new(&path), open.as_ref(), None)
        .map_err(say)?;
    // The survey runs on its own thread; waiting for it is kept off the
    // thread that draws the window.
    let found = tauri::async_runtime::spawn_blocking(move || app.state::<Shared>().keevoro.wait(job.id))
        .await
        .map_err(say)?;
    match found {
        Some(keevoro_api::Said::Surveyed(view)) => Ok(view),
        _ => Err("Keevoro couldn't finish looking for your photos.".to_string()),
    }
}

/// Asks a running survey to stop. It answers with what it has so far.
#[tauri::command]
pub fn stop_survey(shared: State<'_, Shared>) {
    shared.keevoro.stop(Which::Kinds(&[JobKind::Survey]));
}

/// Starts looking through a drive or folder. Returns at once; progress and
/// the result arrive as `scan` events.
#[tauri::command]
pub fn scan_place(shared: State<'_, Shared>, path: String) -> Said<()> {
    let open = open_backup(&shared)?;
    shared.keevoro.look(&open, Path::new(&path), None).map_err(say)?;
    Ok(())
}

/// Where this backup goes, in words, or `None` if nowhere yet.
/// Quick: the places are named without counting what has reached them.
#[tauri::command]
pub fn destination(shared: State<'_, Shared>) -> Said<Option<String>> {
    backup(&shared)?.destinations_in_words().map_err(say)
}

/// Whether this backup also makes and sends previews for a gallery.
#[tauri::command]
pub fn previews_wanted(shared: State<'_, Shared>) -> Said<bool> {
    backup(&shared)?.previews().map_err(say)
}

/// Sets, for this backup, whether previews are made and sent.
#[tauri::command]
pub fn set_previews_wanted(shared: State<'_, Shared>, wanted: bool) -> Said<()> {
    backup(&shared)?.set_previews(wanted).map_err(say)?;
    shared.keevoro.log().say(if wanted {
        "This backup will also make and send previews"
    } else {
        "This backup will send the originals alone, with no previews"
    });
    Ok(())
}

/// Weighs what is waiting to be sent against where it is going.
#[tauri::command]
pub fn room(shared: State<'_, Shared>) -> Said<keevoro_api::RoomAtAGlance> {
    Ok(backup(&shared)?.room().map_err(say)?.at_a_glance)
}

/// Clears Keevoro's unfinished copies from a folder, or with `dry_run`
/// only says what it would clear. Any folder the person chooses: one no
/// backup goes to any more is the case it is for.
#[tauri::command]
pub async fn tidy_folder<R: Runtime>(
    app: AppHandle<R>,
    folder: String,
    dry_run: bool,
) -> Said<keevoro_api::keevoro::Tidied> {
    tauri::async_runtime::spawn_blocking(move || app.state::<Shared>().keevoro.tidy_folder(Path::new(&folder), dry_run))
        .await
        .map_err(say)?
        .map_err(say)
}

/// Shows a folder Keevoro has mentioned in the computer's file manager.
/// Only folders it knows about are opened: a source, where this backup
/// sends to, or the backup's own folder. Anything else is refused, so a
/// screen can never be used to open an arbitrary place.
#[tauri::command]
pub fn open_place<R: Runtime>(app: AppHandle<R>, shared: State<'_, Shared>, path: String) -> Said<()> {
    use tauri_plugin_opener::OpenerExt;
    let library = library(&shared)?;
    let catalog = Catalog::open_in_library(&library).map_err(say)?;
    let mut known = vec![library.display().to_string(), keevoro_engine::friendly_path(&library)];
    // A folder inside a source is known too, so a copy can be looked at.
    let mut inside_a_source = false;
    for source in catalog.sources().map_err(say)? {
        let shown = keevoro_engine::friendly_path(&source.root);
        inside_a_source |= std::path::Path::new(&path).starts_with(&shown)
            && !std::path::Path::new(&path)
                .components()
                .any(|c| c == std::path::Component::ParentDir);
        known.push(shown);
        known.push(source.root.display().to_string());
    }
    for saved in connect::destinations(&catalog).map_err(say)? {
        if saved.kind == "folder" {
            known.push(saved.place);
        }
    }
    if !known.contains(&path) && !inside_a_source {
        return Err("Keevoro doesn't know that folder.".to_string());
    }
    if !std::path::Path::new(&path).is_dir() {
        return Err(format!("{path} isn't there right now. Is the drive plugged in?"));
    }
    app.opener().open_path(path, None::<&str>).map_err(say)
}

/// What the "connect" form sends. The keys are held only for the length of
/// this request: they go to the system keychain, never to disk or a log.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectRequest {
    /// `folder`, `r2`, `s3` or `endpoint`.
    kind: String,
    /// The folder, Cloudflare account ID, Amazon region or server address.
    place: String,
    #[serde(default)]
    bucket: Option<String>,
    #[serde(default)]
    access_key_id: String,
    #[serde(default)]
    secret_access_key: String,
}

impl From<ConnectRequest> for keevoro_api::AddDestinationRequest {
    fn from(form: ConnectRequest) -> Self {
        use keevoro_api::DestinationKind;
        Self {
            kind: match form.kind.as_str() {
                "folder" => DestinationKind::Folder,
                "r2" => DestinationKind::R2,
                "s3" => DestinationKind::S3,
                _ => DestinationKind::Endpoint,
            },
            place: form.place,
            bucket: form.bucket,
            access_key_id: Some(keevoro_api::Key::new(form.access_key_id)),
            secret_access_key: Some(keevoro_api::Key::new(form.secret_access_key)),
        }
    }
}

/// Checks a backup destination works, then remembers it. Returns it in words.
#[tauri::command]
pub async fn connect_destination<R: Runtime>(
    app: AppHandle<R>,
    shared: State<'_, Shared>,
    request: ConnectRequest,
) -> Said<String> {
    let library = library(&shared)?;
    let request = keevoro_api::AddDestinationRequest::from(request);
    // Connecting talks to the storage, which can take a few seconds.
    tauri::async_runtime::spawn_blocking(move || {
        let backup = Backup::open(&library)?;
        // Added to the places this backup goes to; those already there stay.
        Ok(app.state::<Shared>().keevoro.add_destination(&backup, &request)?.name)
    })
    .await
    .map_err(say)?
    .map_err(|e: keevoro_api::Error| say(e))
}

/// Forgets one destination, or all of them, and removes their keys from
/// the keychain. Nothing stored at any of them is changed.
#[tauri::command]
pub fn disconnect_destination(shared: State<'_, Shared>, at: Option<usize>) -> Said<()> {
    shared.keevoro.remove_destination(&backup(&shared)?, at).map_err(say)?;
    Ok(())
}

/// What this backup's last run said when it ended, or nothing if it has
/// never run or now goes somewhere else.
#[tauri::command]
pub fn last_backup_said(shared: State<'_, Shared>) -> Said<Option<serde_json::Value>> {
    backup(&shared)?.last_summary().map_err(say)
}

/// Starts backing up to every destination. Returns at once; progress and
/// what it said at the end arrive as `send` events.
#[tauri::command]
pub fn send(shared: State<'_, Shared>) -> Said<()> {
    let open = open_backup(&shared)?;
    shared.keevoro.run_backup(&open, None).map_err(say)?;
    Ok(())
}

/// Asks a send that is running to stop. The file in hand is finished, so
/// nothing is left half-sent; everything already sent stays where it is.
#[tauri::command]
pub fn stop_sending(shared: State<'_, Shared>) {
    if let Ok(open) = open_backup(&shared) {
        shared.keevoro.stop(Which::KindsOn(&[JobKind::BackUp], &open.id));
    }
}

/// Starts checking what is at each destination. Returns at once; progress
/// and what it found arrive as `send` events.
#[tauri::command]
pub fn check_arrived(shared: State<'_, Shared>) -> Said<()> {
    let open = open_backup(&shared)?;
    shared.keevoro.check_backup(&open, None).map_err(say)?;
    Ok(())
}

/// The drives plugged into this computer, for the "Where are your photos?"
/// screen to offer as buttons.
#[tauri::command]
pub fn places() -> Vec<keevoro_api::places::Place> {
    keevoro_api::places::plugged_in()
}

/// Is this one of Keevoro's own pages: the project's home, or something
/// beneath it such as a release or the licence?
fn is_a_keevoro_page(page: &str) -> bool {
    const HOME: &str = "https://github.com/markcastle/keevoro";
    page == HOME || page.strip_prefix(HOME).is_some_and(|rest| rest.starts_with('/'))
}

/// Opens one of Keevoro's own pages in the person's browser: a release,
/// the licence, the source. Nothing else is opened, whatever the screen
/// asks for.
#[tauri::command]
pub fn open_release_page<R: Runtime>(app: AppHandle<R>, page: String) -> Said<()> {
    use tauri_plugin_opener::OpenerExt;
    if !is_a_keevoro_page(&page) {
        return Err("That isn't one of Keevoro's own pages.".to_string());
    }
    app.opener().open_url(page, None::<&str>).map_err(say)
}

/// The last few hundred lines of the log, for showing in Settings.
#[tauri::command]
pub fn log_tail(shared: State<'_, Shared>) -> String {
    shared.keevoro.log_tail(400)
}

/// Opens the log file in whatever the computer uses for text files.
#[tauri::command]
pub fn open_log<R: Runtime>(app: AppHandle<R>, shared: State<'_, Shared>) -> Said<()> {
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_path(shared.keevoro.log().path().display().to_string(), None::<&str>)
        .map_err(say)
}

/// A newer Keevoro, if there is one this computer can install.
#[tauri::command]
pub async fn check_update<R: Runtime>(app: AppHandle<R>) -> Said<Option<keevoro_api::Offer>> {
    tauri::async_runtime::spawn_blocking(move || app.state::<Shared>().keevoro.update_offer())
        .await
        .map_err(say)
}

/// Downloads the newer Keevoro, checks it, and starts installing it. On
/// Windows the app then closes so the installer can replace it.
#[tauri::command]
pub async fn install_update<R: Runtime>(app: AppHandle<R>) -> Said<()> {
    let os = std::env::consts::OS;
    let into = app.path().app_cache_dir().map_err(say)?.join("update");
    let installer =
        tauri::async_runtime::spawn_blocking(move || crate::updates::fetch(env!("CARGO_PKG_VERSION"), os, &into))
            .await
            .map_err(say)?
            .map_err(|e| format!("Keevoro couldn't update itself: {e}."))?;
    let (program, args) = crate::updates::launch_plan(os, &installer)
        .ok_or_else(|| "Keevoro can't update itself on this kind of computer yet.".to_string())?;
    std::process::Command::new(program)
        .args(args)
        .spawn()
        .map_err(|e| format!("Keevoro downloaded the update but couldn't start it: {e}."))?;
    if os == "windows" {
        app.exit(0);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_keevoros_own_pages_are_opened() {
        assert!(is_a_keevoro_page("https://github.com/markcastle/keevoro"));
        assert!(is_a_keevoro_page(
            "https://github.com/markcastle/keevoro/releases/tag/v0.26.0"
        ));
        assert!(is_a_keevoro_page(
            "https://github.com/markcastle/keevoro/blob/main/LICENSE.md"
        ));
        // A look-alike that merely starts the same way is not.
        assert!(!is_a_keevoro_page(
            "https://github.com/markcastle/keevoro-evil/releases"
        ));
        assert!(!is_a_keevoro_page("https://github.com/someone/keevoro"));
        assert!(!is_a_keevoro_page(
            "https://example.com/https://github.com/markcastle/keevoro/"
        ));
        assert!(!is_a_keevoro_page("file:///C:/Windows"));
    }
}
