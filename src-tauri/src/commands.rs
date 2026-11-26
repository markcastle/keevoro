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
pub fn start_backup(
    shared: State<'_, Shared>,
    name: String,
    wants: Option<Vec<keevoro_api::wants::WantsWord>>,
) -> Said<AppState> {
    // What it is for is checked before anything is started.
    let wants = wants
        .as_deref()
        .map(keevoro_api::wants::wants_of)
        .transpose()
        .map_err(say)?;
    let started = shared.keevoro.start_backup_for(&name, wants, None).map_err(say)?;
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

/// One backup as the list of backups shows it: its sources, where it goes,
/// what it holds and when it last ran. Off the window's thread, as counting
/// a large backup takes a moment; the screen fills in each as it comes.
#[tauri::command]
pub async fn backup_details<R: Runtime>(app: AppHandle<R>, id: String) -> Said<keevoro_api::BackupListing> {
    tauri::async_runtime::spawn_blocking(move || app.state::<Shared>().keevoro.backup_listing(&id))
        .await
        .map_err(say)?
        .map_err(say)
}

/// Gives a backup a new name. Refused while it is being worked on.
#[tauri::command]
pub fn rename_backup(shared: State<'_, Shared>, id: String, to: String) -> Said<AppState> {
    shared.keevoro.rename_backup(&id, &to).map_err(say)?;
    Ok(current_state(&shared))
}

/// Removes a backup from Keevoro, or with `dry_run` only says what that
/// would do. Nothing at a destination or on a source is touched, and it
/// can be put back for a week. Stopping the background check on it can
/// take a moment, so it is done off the window's thread.
#[tauri::command]
pub async fn remove_backup<R: Runtime>(
    app: AppHandle<R>,
    id: String,
    dry_run: bool,
) -> Said<keevoro_api::removing::Removal> {
    tauri::async_runtime::spawn_blocking(move || app.state::<Shared>().keevoro.forget_backup(&id, dry_run))
        .await
        .map_err(say)?
        .map_err(say)
}

/// The backups removed in the last week, which can still be put back.
#[tauri::command]
pub async fn removed_backups<R: Runtime>(app: AppHandle<R>) -> Vec<keevoro_api::removing::RemovedBackup> {
    tauri::async_runtime::spawn_blocking(move || app.state::<Shared>().keevoro.removed_backups())
        .await
        .unwrap_or_default()
}

/// Puts back a backup removed in the last week, exactly as it was.
#[tauri::command]
pub fn put_back_backup(shared: State<'_, Shared>, id: String) -> Said<AppState> {
    shared.keevoro.put_back_backup(&id).map_err(say)?;
    Ok(current_state(&shared))
}

/// Clears away a removed backup now: its own folder on this computer and
/// any keys no other backup uses. It can no longer be put back.
#[tauri::command]
pub async fn clear_away_backup<R: Runtime>(
    app: AppHandle<R>,
    id: String,
) -> Said<keevoro_api::removing::RemovedBackup> {
    tauri::async_runtime::spawn_blocking(move || app.state::<Shared>().keevoro.clear_away_backup(&id))
        .await
        .map_err(say)?
        .map_err(say)
}

/// Puts Keevoro back as it was when first installed: every backup is
/// forgotten, along with what it found and chose, the previews, and any
/// storage keys in the keychain. Nothing on a source, nothing that was sent
/// anywhere, and no folder outside Keevoro's own data is touched.
#[tauri::command]
pub fn reset_everything(shared: State<'_, Shared>) -> Said<AppState> {
    // The API refuses while any of Keevoro's programs is working.
    shared.keevoro.reset().map_err(say)?;
    // Nor does Keevoro start with the computer any more.
    if let Err(why) = shared.desk.startup.set(false) {
        shared.keevoro.log().say(&crate::startup::could_not(false, &why));
    }
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
    backup(&shared)?
        .file_types(id, shared.keevoro.lists_other_files())
        .map_err(say)
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

/// Where the space goes in a surveyed place, from the survey Keevoro
/// remembers; `None` when no survey covers it yet, so the screen surveys it
/// first.
#[tauri::command]
pub async fn space_map<R: Runtime>(
    app: AppHandle<R>,
    request: keevoro_api::space::MapRequest,
) -> Said<Option<keevoro_api::space::MapView>> {
    // A map of a whole drive can take a moment to read back.
    tauri::async_runtime::spawn_blocking(move || {
        let keevoro = &app.state::<Shared>().keevoro;
        let backup = keevoro.backups().current();
        keevoro.space_map_if_surveyed(&request, backup.as_ref())
    })
    .await
    .map_err(say)?
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
/// over from last time, unless the person stopped it, and say how it
/// stands, so a check stopped before is said with its way to start again.
#[tauri::command]
pub fn keep_checking(shared: State<'_, Shared>) -> Option<keevoro_api::jobs::DuplicatesLeft> {
    let open = open_backup(&shared).ok()?;
    shared.keevoro.keep_checking(&open);
    Some(shared.keevoro.duplicates_left(&open))
}

/// The background check for duplicates on the open backup, running in
/// this app.
fn running_check(shared: &Shared) -> Option<keevoro_api::JobId> {
    let open = open_backup(shared).ok()?;
    shared
        .keevoro
        .jobs(Some(&open.id))
        .into_iter()
        .find(|job| !job.elsewhere && job.handle.job == JobKind::Duplicates)
        .map(|job| job.handle.id)
}

/// Pauses the background check for duplicates on its own, or lets it go
/// on. Everything else carries on either way.
#[tauri::command]
pub fn pause_checking(shared: State<'_, Shared>, paused: bool) -> Said<()> {
    let Some(job) = running_check(&shared) else {
        return Ok(());
    };
    if paused {
        shared.keevoro.pause_job(job).map_err(say)
    } else {
        shared.keevoro.carry_on_job(job).map_err(say)
    }
}

/// Stops the background check for duplicates. It does not start again by
/// itself until the next search or backup, or until asked.
#[tauri::command]
pub fn stop_checking(shared: State<'_, Shared>) {
    if let Ok(open) = open_backup(&shared) {
        shared.keevoro.stop(Which::KindsOn(&[JobKind::Duplicates], &open.id));
    }
}

/// "Check for duplicates now": starts the check, even after it was stopped.
#[tauri::command]
pub fn check_duplicates_now(shared: State<'_, Shared>) -> Said<()> {
    let open = open_backup(&shared)?;
    shared.keevoro.check_for_duplicates_now(&open).map_err(say)?;
    Ok(())
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

/// What this backup is for: photos, videos, other files, or some of them.
#[tauri::command]
pub fn backup_wants(shared: State<'_, Shared>) -> Said<keevoro_api::wants::WantsView> {
    backup(&shared)?.wants(shared.keevoro.lists_other_files()).map_err(say)
}

/// Makes this backup for one or more of photos, videos and other files.
#[tauri::command]
pub fn set_backup_wants(
    shared: State<'_, Shared>,
    wants: Vec<keevoro_api::wants::WantsWord>,
) -> Said<keevoro_api::wants::WantsView> {
    let backup = backup(&shared)?;
    let wants = keevoro_api::wants::wants_of(&wants).map_err(say)?;
    shared.keevoro.set_backup_wants(&backup, wants).map_err(say)
}

/// Whether one source follows its backup's choice of what it is for.
#[tauri::command]
pub fn source_wants(shared: State<'_, Shared>, id: i64) -> Said<keevoro_api::wants::WantsView> {
    backup(&shared)?
        .source_wants(id, shared.keevoro.lists_other_files())
        .map_err(say)
}

/// Gives one source its own choice of what it is for, or the backup's.
#[tauri::command]
pub fn set_source_wants(
    shared: State<'_, Shared>,
    id: i64,
    wants: keevoro_api::wants::SourceWantsChoice,
) -> Said<keevoro_api::wants::WantsView> {
    let backup = backup(&shared)?;
    let wants = wants.wants().map_err(say)?;
    shared.keevoro.set_source_wants(&backup, id, wants).map_err(say)
}

/// How this backup arranges what it sends, whether it was asked, and
/// what the files found suggest.
#[tauri::command]
pub fn backup_arrange(shared: State<'_, Shared>) -> Said<keevoro_api::arrange::ArrangeView> {
    backup(&shared)?.arrangement().map_err(say)
}

/// Arranges this backup by date, or keeping the folders as they are.
#[tauri::command]
pub fn set_backup_arrange(
    shared: State<'_, Shared>,
    arrange: keevoro_api::arrange::ArrangeWord,
) -> Said<keevoro_api::arrange::ArrangeView> {
    let backup = backup(&shared)?;
    shared.keevoro.set_arrangement(&backup, arrange.arrange()).map_err(say)
}

/// How one source's files are arranged, and what they suggest.
#[tauri::command]
pub fn source_arrange(shared: State<'_, Shared>, id: i64) -> Said<keevoro_api::arrange::SourceArrangeView> {
    backup(&shared)?.source_arrangement(id).map_err(say)
}

/// Gives one source its own arrangement, or the backup's.
#[tauri::command]
pub fn set_source_arrange(
    shared: State<'_, Shared>,
    id: i64,
    arrange: keevoro_api::arrange::SourceArrangeWord,
) -> Said<keevoro_api::arrange::SourceArrangeView> {
    let backup = backup(&shared)?;
    shared
        .keevoro
        .set_source_arrangement(&backup, id, arrange.own())
        .map_err(say)
}

/// The kinds of file Keevoro looks for, the built-in ones first.
#[tauri::command]
pub fn file_kinds(shared: State<'_, Shared>) -> Vec<keevoro_api::FileKind> {
    shared.keevoro.file_kinds()
}

/// Adds an ending for Keevoro to look for, as photos, videos or other files.
#[tauri::command]
pub fn add_file_kind(
    shared: State<'_, Shared>,
    ending: String,
    kind: keevoro_api::ListedAs,
) -> Said<keevoro_api::FileKind> {
    shared.keevoro.add_file_kind(&ending, kind.into()).map_err(say)
}

/// Changes one kind: photo, video or other file, on or off, or, for one
/// the person added, its ending. Anything not given stays as it is.
#[tauri::command]
pub fn change_file_kind(
    shared: State<'_, Shared>,
    ending: String,
    kind: Option<keevoro_api::ListedAs>,
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
pub fn pause_work<R: Runtime>(app: AppHandle<R>, shared: State<'_, Shared>, paused: bool) {
    if paused {
        shared.keevoro.pause();
    } else {
        shared.keevoro.carry_on();
    }
    // The icon by the clock says so too.
    crate::tray::nudge(&app);
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

/// Looks through one source again, or every one. A source that is not
/// plugged in is passed over and said; one whose place holds a different
/// drive is asked about, and `different_drive` is the person's answer for
/// the one source named.
#[tauri::command]
pub fn look_again(
    shared: State<'_, Shared>,
    source: Option<i64>,
    different_drive: Option<keevoro_api::jobs::OtherDrive>,
) -> Said<()> {
    let open = open_backup(&shared)?;
    shared
        .keevoro
        .look_again(&open, source, different_drive, None)
        .map_err(say)?;
    Ok(())
}

/// When the open backup backs up by itself, when next, and its last few
/// runs.
#[tauri::command]
pub fn schedule(shared: State<'_, Shared>) -> Said<keevoro_api::schedule::ScheduleView> {
    shared.keevoro.schedule(&open_backup(&shared)?).map_err(say)
}

/// Sets, changes, pauses or stops the open backup backing up by itself.
#[tauri::command]
pub fn set_schedule(
    shared: State<'_, Shared>,
    request: keevoro_api::schedule::SetScheduleRequest,
) -> Said<keevoro_api::schedule::ScheduleView> {
    shared
        .keevoro
        .set_schedule(&open_backup(&shared)?, &request)
        .map_err(say)
}

/// Skips the next run by itself, or stops skipping it.
#[tauri::command]
pub fn skip_next_run(shared: State<'_, Shared>, undo: bool) -> Said<keevoro_api::schedule::ScheduleView> {
    shared.keevoro.skip_next_run(&open_backup(&shared)?, undo).map_err(say)
}

/// Runs the open backup now as its schedule does: looks through the
/// sources again, holds back what needs a say, sends what is new.
/// With `anyway`, it starts without waiting for a good moment: "Back up
/// now anyway" beside a run that waits.
#[tauri::command]
pub fn run_scheduled_now(shared: State<'_, Shared>, anyway: Option<bool>) -> Said<()> {
    let open = open_backup(&shared)?;
    if anyway == Some(true) {
        shared
            .keevoro
            .run_by_itself_anyway(&open, keevoro_api::Occasion::RunNow)
            .map_err(say)?;
    } else {
        shared
            .keevoro
            .run_by_itself(&open, keevoro_api::Occasion::RunNow, 0, None)
            .map_err(say)?;
    }
    Ok(())
}

/// How a backup by itself waits: on battery, on a metered connection, and
/// the hours it may start in; and changing them.
#[tauri::command]
pub fn background_settings(
    shared: State<'_, Shared>,
    request: Option<keevoro_api::manners::MannersRequest>,
) -> Said<keevoro_api::manners::MannersView> {
    shared
        .keevoro
        .background_settings(&request.unwrap_or_default())
        .map_err(say)
}

/// "While you were away": what every backup did by itself since the
/// window was last seen, and what waits for the person's say.
#[tauri::command]
pub fn away(shared: State<'_, Shared>) -> keevoro_api::schedule::AwayView {
    shared.keevoro.away(&keevoro_api::schedule::AwayRequest {
        since: shared.desk.away_since(),
        days: None,
    })
}

/// Whether a source of the open backup is backed up by itself whenever it
/// is plugged in, and changing it.
#[tauri::command]
pub fn source_on_arrival(
    shared: State<'_, Shared>,
    source: i64,
    on: Option<bool>,
) -> Said<keevoro_api::arrival::OnArrivalView> {
    shared
        .keevoro
        .on_arrival(&open_backup(&shared)?, source, on)
        .map_err(say)
}

/// The person pressed Done on "While you were away".
#[tauri::command]
pub fn away_seen(shared: State<'_, Shared>) {
    shared.desk.away_seen(shared.keevoro.now().as_second());
}

/// The person is using the window: a backup by itself waits until they
/// have finished.
#[tauri::command]
pub fn window_used(shared: State<'_, Shared>) {
    shared.keevoro.note_window_used();
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

/// Weighs what is waiting to be sent against where it is going, first
/// looking at what is really at each destination unless that was done a
/// moment ago, which across a network can take a moment, so it is done off
/// the window's thread.
#[tauri::command]
pub async fn room<R: Runtime>(app: AppHandle<R>) -> Said<keevoro_api::RoomAtAGlance> {
    tauri::async_runtime::spawn_blocking(move || {
        let shared = app.state::<Shared>();
        let backup = backup(&shared)?;
        shared.keevoro.room(&backup).map(|room| room.at_a_glance).map_err(say)
    })
    .await
    .map_err(say)?
}

/// Forgets every remembered survey, and so every map of where the space
/// goes. Nothing on any drive is touched.
#[tauri::command]
pub fn forget_maps(shared: State<'_, Shared>) -> Said<keevoro_api::space::Forgotten> {
    Ok(shared.keevoro.forget_maps())
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
/// Only folders it knows about are opened: one on a remembered map, a
/// source or a folder in one, where this backup sends to, or the backup's
/// own folder. Anything else is refused (`Keevoro::folder_to_show` decides),
/// so a screen can never be used to open an arbitrary place.
#[tauri::command]
pub fn open_place<R: Runtime>(app: AppHandle<R>, shared: State<'_, Shared>, path: String) -> Said<()> {
    use tauri_plugin_opener::OpenerExt;
    let backup = shared.keevoro.backups().current();
    let folder = shared
        .keevoro
        .folder_to_show(std::path::Path::new(&path), backup.as_ref())
        .map_err(say)?;
    app.opener()
        .open_path(folder.display().to_string(), None::<&str>)
        .map_err(say)
}

/// What the "connect" form sends. The keys are held only for the length of
/// this request: they go to the system keychain, never to disk or a log.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectRequest {
    /// `folder`, `r2`, `s3`, `gcs` or `endpoint`.
    kind: String,
    /// The folder, Cloudflare account ID, Amazon region or server address;
    /// none for Google Cloud Storage.
    #[serde(default)]
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
                "gcs" => DestinationKind::Gcs,
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

/// Looks for a Keevoro backup in a folder or in cloud storage, from the
/// notes Keevoro keeps beside every backup, and says what it holds.
/// Changes nothing. Off the window's thread, as cloud storage takes a
/// moment to answer.
#[tauri::command]
pub async fn find_backup_at<R: Runtime>(
    app: AppHandle<R>,
    request: ConnectRequest,
) -> Said<keevoro_api::opening::FoundBackup> {
    let request = keevoro_api::AddDestinationRequest::from(request);
    tauri::async_runtime::spawn_blocking(move || app.state::<Shared>().keevoro.find_backup_at(&request))
        .await
        .map_err(say)?
        .map_err(say)
}

/// Opens a backup found with `find_backup_at` as a backup on this
/// computer, from its notes, going to that place, and opens it in the
/// window. One already here is opened instead.
#[tauri::command]
pub async fn open_backup_from<R: Runtime>(
    app: AppHandle<R>,
    request: ConnectRequest,
) -> Said<keevoro_api::opening::OpenedBackup> {
    let request = keevoro_api::AddDestinationRequest::from(request);
    let opened = tauri::async_runtime::spawn_blocking({
        let app = app.clone();
        move || app.state::<Shared>().keevoro.open_backup_from(&request)
    })
    .await
    .map_err(say)?
    .map_err(say)?;
    check_in_background(&app.state::<Shared>());
    Ok(opened)
}

/// Says where a source of a backup opened from its notes is on this
/// computer. The window then looks through it again.
#[tauri::command]
pub fn connect_source_again(
    shared: State<'_, Shared>,
    source: i64,
    place: String,
) -> Said<keevoro_api::opening::ConnectedAgain> {
    let open = open_backup(&shared)?;
    let request = keevoro_api::opening::ConnectAgainRequest { source, place };
    let connected = Backup::open(&open.home)
        .map_err(say)?
        .connect_again(&request)
        .map_err(say)?;
    shared
        .keevoro
        .log()
        .say(&format!("Connected {} again, at {}", connected.name, connected.place));
    Ok(connected)
}

/// What the app's form for cloud storage as a source sends. The keys are
/// only in this request: they go to the system keychain, never to disk or
/// a log.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BucketSourceRequest {
    /// `r2`, `s3`, `gcs` or `endpoint`.
    kind: String,
    /// The Cloudflare account ID, Amazon region or server address; none
    /// for Google Cloud Storage.
    #[serde(default)]
    place: String,
    bucket: String,
    #[serde(default)]
    prefix: Option<String>,
    #[serde(default)]
    access_key_id: String,
    #[serde(default)]
    secret_access_key: String,
}

/// Checks cloud storage can be read with these keys, writing nothing, then
/// adds it as a source. Answers it, with where to look through it.
#[tauri::command]
pub async fn add_bucket_source<R: Runtime>(
    app: AppHandle<R>,
    shared: State<'_, Shared>,
    request: BucketSourceRequest,
) -> Said<keevoro_api::bucket_sources::BucketAdded> {
    use keevoro_api::bucket_sources::{AddBucketRequest, BucketKind};
    let library = library(&shared)?;
    let request = AddBucketRequest {
        kind: match request.kind.as_str() {
            "r2" => BucketKind::R2,
            "s3" => BucketKind::S3,
            "gcs" => BucketKind::Gcs,
            _ => BucketKind::Endpoint,
        },
        place: request.place,
        bucket: request.bucket,
        prefix: request.prefix,
        name: None,
        access_key_id: Some(keevoro_api::Key::new(request.access_key_id)),
        secret_access_key: Some(keevoro_api::Key::new(request.secret_access_key)),
    };
    // Checking talks to the storage, which can take a few seconds.
    tauri::async_runtime::spawn_blocking(move || {
        let backup = Backup::open(&library)?;
        app.state::<Shared>().keevoro.add_bucket_source(&backup, &request)
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
        // A check, and putting a backup right, which is one, stop the same way.
        shared
            .keevoro
            .stop(Which::KindsOn(&[JobKind::BackUp, JobKind::Check], &open.id));
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

/// What "Put this backup right" would do at each destination, and about
/// how long it would take, for the screen to say before anything is done.
#[tauri::command]
pub fn put_right_plan(shared: State<'_, Shared>) -> Said<keevoro_api::destinations::PutRightPlan> {
    backup(&shared)?.put_right_plan().map_err(say)
}

/// Starts putting this backup right: reading back, moving damaged copies
/// aside, sending them again. Returns at once; progress and what it did
/// arrive as `send` events.
#[tauri::command]
pub fn put_right(shared: State<'_, Shared>, thorough: bool) -> Said<()> {
    let open = open_backup(&shared)?;
    shared.keevoro.put_right(&open, thorough, None).map_err(say)?;
    Ok(())
}

/// The drives plugged into this computer, for the "Where are your photos?"
/// screen to offer as buttons. Off the window's own thread: asking the
/// drive letters for network drives can take a moment, and a command that
/// is not `async` runs on the thread that draws the window, freezing it.
#[tauri::command]
pub async fn places() -> Said<Vec<keevoro_api::places::Place>> {
    tauri::async_runtime::spawn_blocking(keevoro_api::places::plugged_in)
        .await
        .map_err(say)
}

/// The drives plugged in, if they are not those the screen last knew
/// (`known`, the quick list it was given last time): nothing when they are
/// the same, which is what the first screen hears almost every time it
/// asks, every couple of seconds while it shows. `asked` lists them in full
/// regardless: the person pressed Refresh, or the system said a drive came
/// or went.
#[tauri::command]
pub async fn places_if_changed(
    known: Option<Vec<String>>,
    asked: bool,
) -> Said<Option<keevoro_api::places::PlacesNow>> {
    tauri::async_runtime::spawn_blocking(move || keevoro_api::places::if_changed(known.as_deref(), asked))
        .await
        .map_err(say)
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

/// Is this a plain web page, safe to hand to the person's browser: `https`,
/// a host name and nothing odd? It is how Stratus, the comparison of cloud
/// storage the person opened, and a provider's page it named are opened.
/// Nobody's name or password, no other kind of address (a file, a
/// program), no spaces or control characters, and nothing absurdly long.
fn is_a_web_page(page: &str) -> bool {
    let Some(rest) = page.strip_prefix("https://") else {
        return false;
    };
    let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
    page.len() <= 2000
        && !page.chars().any(|c| c.is_whitespace() || c.is_control() || c == '\\')
        && host.contains('.')
        && host.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
}

/// Opens a page about cloud storage in the person's browser: Stratus, or a
/// provider's page it named. Only plain `https` pages; the screen is not
/// trusted to have checked.
#[tauri::command]
pub fn open_web_page<R: Runtime>(app: AppHandle<R>, page: String) -> Said<()> {
    use tauri_plugin_opener::OpenerExt;
    if !is_a_web_page(&page) {
        return Err("Keevoro only opens secure (https) web pages.".to_string());
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

/// A report of a problem or an idea, exactly as it would be sent, for the
/// person to see first. Reading the log is kept off the window's thread.
#[tauri::command]
pub async fn prepare_report<R: Runtime>(
    app: AppHandle<R>,
    request: keevoro_api::reports::PrepareReportRequest,
) -> Said<keevoro_api::reports::Prepared> {
    tauri::async_runtime::spawn_blocking(move || app.state::<Shared>().keevoro.prepare_report(&request))
        .await
        .map_err(say)?
        .map_err(say)
}

/// Sends a report exactly as it was prepared and shown. It waits on the
/// network, so never on the window's thread. Not sent, it says whether
/// the service could not be reached at all, so the window can send it
/// itself, and why in plain words.
#[tauri::command]
pub async fn send_report<R: Runtime>(
    app: AppHandle<R>,
    prepared: String,
) -> Result<keevoro_api::reports::Sent, keevoro_api::reports::NotSent> {
    tauri::async_runtime::spawn_blocking(move || app.state::<Shared>().keevoro.send_report_saying_why(&prepared, None))
        .await
        .map_err(|e| keevoro_api::reports::NotSent {
            error: keevoro_api::Error {
                kind: keevoro_api::Kind::Failed,
                message: say(e),
            },
            cause: None,
            send_as: None,
        })?
}

/// Whether a reporting code is kept in this computer's keychain, keeping
/// one or forgetting it. It is never given back to the window.
#[tauri::command]
pub fn reporting_code(
    shared: State<'_, Shared>,
    request: keevoro_api::reports::ReportingCodeRequest,
) -> Said<keevoro_api::reports::CodeView> {
    shared.keevoro.reporting_code(&request).map_err(say)
}

/// Keeps a report the window sent itself, with the service's answer.
#[tauri::command]
pub fn note_report(
    shared: State<'_, Shared>,
    request: keevoro_api::reports::NoteReportRequest,
) -> Said<keevoro_api::reports::Sent> {
    shared.keevoro.note_report(&request).map_err(say)
}

/// Where reports go, and the newest with their tokens, for the window to
/// ask after them itself when Keevoro cannot.
#[tauri::command]
pub fn reports_asking(shared: State<'_, Shared>) -> keevoro_api::reports::Asking {
    shared.keevoro.reports_asking()
}

/// Notes how reports are getting on, as the window heard.
#[tauri::command]
pub fn reports_heard(
    shared: State<'_, Shared>,
    request: keevoro_api::reports::HeardRequest,
) -> keevoro_api::reports::ReportsView {
    shared.keevoro.reports_heard(&request)
}

/// The reports sent from this computer, as last heard of.
#[tauri::command]
pub fn reports(shared: State<'_, Shared>) -> keevoro_api::reports::ReportsView {
    shared.keevoro.reports()
}

/// Asks how each report sent from this computer is getting on.
#[tauri::command]
pub async fn refresh_reports<R: Runtime>(app: AppHandle<R>) -> Said<keevoro_api::reports::ReportsView> {
    tauri::async_runtime::spawn_blocking(move || app.state::<Shared>().keevoro.refresh_reports())
        .await
        .map_err(say)
}

/// A newer Keevoro, if there is one this computer can install.
#[tauri::command]
pub async fn check_update<R: Runtime>(app: AppHandle<R>) -> Said<Option<keevoro_api::Offer>> {
    tauri::async_runtime::spawn_blocking(move || app.state::<Shared>().keevoro.update_offer())
        .await
        .map_err(say)
}

/// What pressing Update did: started installing, or is waiting for the
/// work in hand to finish, said in `waiting`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStarted {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub waiting: Option<String>,
}

/// Downloads the newer Keevoro, checks it, and starts installing it. On
/// Windows the app then closes so the installer can replace it, so it never
/// does while a search, a backup or a check is in hand: it answers that it
/// is waiting, and the window asks again once the work is done. With
/// `stop_first`, the work in hand is stopped after the file in hand (nothing
/// is lost) and the update goes ahead.
#[tauri::command]
pub async fn install_update<R: Runtime>(app: AppHandle<R>, stop_first: Option<bool>) -> Said<UpdateStarted> {
    let waiting = || UpdateStarted {
        waiting: Some(crate::updates::WAITING.to_string()),
    };
    let busy = || app.state::<Shared>().keevoro.busy_here();
    match crate::updates::may_install(busy(), stop_first.unwrap_or(false)) {
        crate::updates::Install::Wait => return Ok(waiting()),
        crate::updates::Install::StopFirst => {
            let handle = app.clone();
            let stopped = tauri::async_runtime::spawn_blocking(move || {
                handle
                    .state::<Shared>()
                    .keevoro
                    .stop_here_and_wait(std::time::Duration::from_secs(60))
            })
            .await
            .map_err(say)?;
            if !stopped {
                return Err(
                    "Keevoro couldn't stop the work in hand in time, so it hasn't updated yet. Try again in a moment."
                        .into(),
                );
            }
        }
        crate::updates::Install::Now => {}
    }
    let os = std::env::consts::OS;
    let into = app.path().app_cache_dir().map_err(say)?.join("update");
    let installer =
        tauri::async_runtime::spawn_blocking(move || crate::updates::fetch(env!("CARGO_PKG_VERSION"), os, &into))
            .await
            .map_err(say)?
            .map_err(|e| format!("Keevoro couldn't update itself: {e}."))?;
    // Something may have started while it downloaded.
    if busy() {
        return Ok(waiting());
    }
    let (program, args) = crate::updates::launch_plan(os, &installer)
        .ok_or_else(|| "Keevoro can't update itself on this kind of computer yet.".to_string())?;
    std::process::Command::new(program)
        .args(args)
        .spawn()
        .map_err(|e| format!("Keevoro downloaded the update but couldn't start it: {e}."))?;
    if os == "windows" {
        app.exit(0);
    }
    Ok(UpdateStarted { waiting: None })
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

    #[test]
    fn only_plain_secure_web_pages_are_opened_for_cloud_storage() {
        for page in [
            "https://stratus.adam-sparshott.workers.dev/",
            "https://www.backblaze.com/cloud-storage/pricing",
            "https://cloud.google.com/storage/pricing?hl=en-GB#prices",
        ] {
            assert!(is_a_web_page(page), "{page}");
        }
        for page in [
            "http://stratus.adam-sparshott.workers.dev/",
            "file:///C:/Windows/System32/calc.exe",
            "javascript:alert(1)",
            "https://user:secret@example.com/",
            "https://localhost/",
            "https://example.com/a page",
            "https://example.com\\..\\x",
            "https://",
            "",
        ] {
            assert!(!is_a_web_page(page), "{page}");
        }
        assert!(!is_a_web_page(&format!("https://example.com/{}", "a".repeat(2000))));
    }
}
