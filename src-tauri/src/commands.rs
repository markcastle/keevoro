//! The requests the screens can make, one function each.
//!
//! Every function here does the same three things: find the library, call
//! `keevoro-api` (or, for what has not moved there yet, the engine), and
//! turn any error into a sentence the screen can show. If a function grows
//! beyond that, the extra belongs in the API, where the command line gets
//! it too. `parity.rs` keeps count of what has still to move.

use crate::events::{CheckEvent, GatherEvent, ScanEvent, SendEvent, SurveyEvent, CHECK, GATHER, SCAN, SEND, SURVEY};
use crate::Shared;
use keevoro_api::Backup;
use keevoro_catalog::Catalog;
use keevoro_engine::connect::{self, World};
use keevoro_engine::{Discover, PhotoChoice};
use keevoro_media::VideoTools;
use serde::Deserialize;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager, State};

/// How often to tell the screen how a search is going. Often enough to
/// feel alive, not so often that drawing slows the search down.
const PROGRESS_EVERY: Duration = Duration::from_millis(150);

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
    suggested_library: String,
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

fn current_state(app: &AppHandle, shared: &Shared) -> AppState {
    let settings = shared.keevoro.backups().read();
    let view = |s: &keevoro_api::Named| BackupView {
        id: s.id.clone(),
        name: s.name.clone(),
    };
    let library = settings.open_backup().map(|s| s.home.clone());
    // The Pictures folder is where people expect photos to live.
    let pictures = app
        .path()
        .picture_dir()
        .or_else(|_| app.path().home_dir())
        .unwrap_or_else(|_| PathBuf::from("."));
    AppState {
        library: library.map(|p| p.display().to_string()),
        suggested_library: pictures.join("Keevoro Library").display().to_string(),
        backup: settings.open_backup().map(view),
        backups: settings.backups.iter().map(view).collect(),
        version: env!("CARGO_PKG_VERSION"),
    }
}

fn library(shared: &Shared) -> Said<PathBuf> {
    shared
        .keevoro
        .backups()
        .current()
        .map(|open| open.home)
        .ok_or_else(|| "Start a backup first.".to_string())
}

/// The open backup, through the API that the command line uses too.
fn backup(shared: &Shared) -> Said<Backup> {
    Backup::open(&library(shared)?).map_err(say)
}

/// Starts a new backup with this name and opens it.
#[tauri::command]
pub fn start_backup(app: AppHandle, shared: State<'_, Shared>, name: String) -> Said<AppState> {
    let started = shared.keevoro.start_backup(&name).map_err(say)?;
    // The window shows the one just started.
    shared.keevoro.backups().switch_to(&started.id).map_err(say)?;
    Ok(current_state(&app, &shared))
}

/// Opens another backup. Whatever was open is left exactly as it was.
#[tauri::command]
pub fn switch_backup(app: AppHandle, shared: State<'_, Shared>, id: String) -> Said<AppState> {
    if shared.scanning.load(Ordering::SeqCst) || shared.sending.load(Ordering::SeqCst) {
        return Err("Keevoro is in the middle of something in this backup. Let that finish first.".into());
    }
    if !shared.keevoro.switch_to(&id).map_err(say)? {
        return Err("That backup is no longer there.".into());
    }
    check_in_background(&app);
    Ok(current_state(&app, &shared))
}

/// Puts Keevoro back as it was when first installed: every backup is
/// forgotten, along with what it found and chose, the previews, and any
/// storage keys in the keychain. Nothing on a source, nothing that was sent
/// anywhere, and no folder outside Keevoro's own data is touched.
#[tauri::command]
pub fn reset_everything(app: AppHandle, shared: State<'_, Shared>) -> Said<AppState> {
    if shared.scanning.load(Ordering::SeqCst)
        || shared.sending.load(Ordering::SeqCst)
        || shared.gathering.load(Ordering::SeqCst)
    {
        return Err(
            "Keevoro is in the middle of something. Let that finish, or stop it, before starting again.".into(),
        );
    }
    shared.keevoro.reset().map_err(say)?;
    Ok(current_state(&app, &shared))
}

/// Shows the folder this backup sends to, if it sends to a folder.
#[tauri::command]
pub fn open_destination(app: AppHandle, shared: State<'_, Shared>, at: Option<usize>) -> Said<()> {
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
pub fn app_state(app: AppHandle, shared: State<'_, Shared>) -> AppState {
    current_state(&app, &shared)
}

/// Sets where the library lives, creating it if it is new.
#[tauri::command]
pub fn use_library(app: AppHandle, shared: State<'_, Shared>, path: String) -> Said<AppState> {
    // Kept for the first-run screen of older builds: the folder becomes a
    // backup of its own.
    shared
        .keevoro
        .backups()
        .adopt_library(std::path::Path::new(&path))
        .map_err(say)?;
    Ok(current_state(&app, &shared))
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
pub async fn preview(app: AppHandle, shared: State<'_, Shared>, id: String) -> Said<tauri::ipc::Response> {
    picture(app, &shared, id, keevoro_api::PictureSize::Small).await
}

/// A screen-sized picture of one found file, for looking at it closely.
#[tauri::command]
pub async fn closer_look(app: AppHandle, shared: State<'_, Shared>, id: String) -> Said<tauri::ipc::Response> {
    picture(app, &shared, id, keevoro_api::PictureSize::Screen).await
}

async fn picture(
    app: AppHandle,
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

/// Reads, in the background, the files a quick search only sampled, so
/// that true duplicates are known and gathering has less to do. Does
/// nothing if a pass is already running. It gives way to a search or a
/// gather, which start it again when they finish.
fn check_in_background(app: &AppHandle) {
    let state = app.state::<Shared>();
    let Ok(library) = library(&state) else { return };
    if state.checking.swap(true, Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        let state = app.state::<Shared>();
        let started = Instant::now();
        let mut last_said = Instant::now() - PROGRESS_EVERY;
        let mut total = 0;
        let result = Catalog::open_in_library(&library)
            .map_err(anyhow::Error::from)
            .and_then(|catalog| {
                keevoro_engine::deep::deep_look(
                    &catalog,
                    None,
                    &mut |p| {
                        total = p.total;
                        if p.done == 0 {
                            state
                                .keevoro
                                .log()
                                .say(&format!("Checking {} files in the background", p.total));
                        }
                        if p.bytes >= 64 * 1024 * 1024 {
                            state.keevoro.log().say(&format!(
                                "Checking a large file ({}): {}",
                                keevoro_engine::summary::bytes(p.bytes),
                                keevoro_engine::friendly_path(p.reading)
                            ));
                        }
                        if last_said.elapsed() >= PROGRESS_EVERY {
                            last_said = Instant::now();
                            let _ = app.emit(
                                CHECK,
                                CheckEvent {
                                    done: p.done,
                                    total: p.total,
                                    finished: false,
                                },
                            );
                        }
                    },
                    // It gives way to a search and to a backup. A backup
                    // reads the same files, and fingerprints them as it goes,
                    // so checking alongside it would read them twice over.
                    &|| {
                        !state.scanning.load(Ordering::SeqCst)
                            && !state.gathering.load(Ordering::SeqCst)
                            && !state.sending.load(Ordering::SeqCst)
                    },
                )
            });
        state.checking.store(false, Ordering::SeqCst);
        let done = match &result {
            Ok(report) => {
                if report.checked > 0 || !report.failed.is_empty() || report.stopped {
                    state.keevoro.log().say(&format!(
                        "Background checking {} after {} seconds: {} checked, {} could not be read",
                        if report.stopped { "paused" } else { "finished" },
                        started.elapsed().as_secs(),
                        report.checked,
                        report.failed.len()
                    ));
                }
                for (path, why) in report.failed.iter().take(200) {
                    state.keevoro.log().say(&format!(
                        "Could not check {}: {why}",
                        keevoro_engine::friendly_path(path)
                    ));
                }
                report.checked
            }
            Err(e) => {
                state.keevoro.log().say(&format!("Background checking failed: {e:#}"));
                0
            }
        };
        let _ = app.emit(
            CHECK,
            CheckEvent {
                done,
                total,
                finished: true,
            },
        );
    });
}

/// Asked by the screens when they open: carry on with any checking left
/// over from last time.
#[tauri::command]
pub fn keep_checking(app: AppHandle) {
    check_in_background(&app);
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

/// How a send is getting on: how much has arrived, how fast, and roughly
/// how long is left.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SendProgress {
    bytes_done: u64,
    bytes_total: u64,
    /// Bytes a second, steadied over the last few seconds.
    speed: u64,
    /// `None` while nothing is arriving, when no honest guess can be made.
    seconds_left: Option<u64>,
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

#[tauri::command]
pub fn send_progress(shared: State<'_, Shared>) -> SendProgress {
    let total = shared.send_total.load(Ordering::SeqCst);
    let done = keevoro_core::meter::totals()
        .0
        .saturating_sub(shared.send_began_at.load(Ordering::SeqCst))
        .min(total);
    let (samples, _) = keevoro_api::activity::recent();
    let (speed, seconds_left) = keevoro_api::activity::speed_and_time_left(&samples, total - done);
    SendProgress {
        bytes_done: done,
        bytes_total: total,
        speed,
        seconds_left,
        over_network: samples.iter().rev().take(10).any(|s| s.network > 0.0),
        running: shared.sending.load(Ordering::SeqCst),
        stopping: shared.stop_sending.load(Ordering::SeqCst),
        line: shared.keevoro.backup_step(),
    }
}

/// Pauses or resumes every heavy job. Each waits at its next safe point,
/// between one file and the next, and carries on from there when resumed.
#[tauri::command]
pub fn pause_work(shared: State<'_, Shared>, paused: bool) {
    keevoro_core::pause::set_paused(paused);
    shared.keevoro.log().say(if paused {
        "Paused: all heavy work is waiting"
    } else {
        "Resumed: work is carrying on from where it paused"
    });
}

/// Asks a search that is running to stop. It ends within a moment and
/// nothing from it is kept: a part-search would make the rest of the source
/// look as if it were not there.
#[tauri::command]
pub fn stop_scanning(shared: State<'_, Shared>) {
    shared.stop_scanning.store(true, Ordering::SeqCst);
    // A paused job is waiting, and must wake to see it has been asked to stop.
    keevoro_core::pause::set_paused(false);
    shared.keevoro.log().say("Asked to stop the search");
}

/// A folder a survey suggests looking through.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HintView {
    /// The folder, as it is shown and as it is handed back to be searched.
    folder: String,
    photos: usize,
    videos: usize,
    bytes: u64,
    /// False when the pictures are so small they are probably not photos.
    likely_photos: bool,
    /// True when this folder is already one of the backup's sources.
    already_a_source: bool,
}

/// What a survey of a drive found.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SurveyView {
    hints: Vec<HintView>,
    folders: usize,
    files: usize,
    unreadable: usize,
    seconds: f32,
    stopped: bool,
}

/// Finds which folders under `path` hold photos and videos, by reading
/// folder listings only. No file is opened and nothing is remembered: the
/// answer is a list of suggestions for the person to choose from.
#[tauri::command]
pub async fn survey_place(app: AppHandle, shared: State<'_, Shared>, path: String) -> Said<SurveyView> {
    let sources: Vec<String> = match library(&shared) {
        Ok(library) => Catalog::open_in_library(&library)
            .and_then(|c| c.sources())
            .map(|list| list.iter().map(|s| keevoro_engine::friendly_path(&s.root)).collect())
            .unwrap_or_default(),
        Err(_) => Vec::new(),
    };
    shared.stop_survey.store(false, Ordering::SeqCst);
    let stop = shared.stop_survey.clone();
    shared
        .keevoro
        .log()
        .say(&format!("Finding where the photos are in {path}"));
    let root = PathBuf::from(&path);
    let started = Instant::now();
    let found = tauri::async_runtime::spawn_blocking(move || {
        let _busy = keevoro_core::doing::doing(keevoro_core::doing::Work::Surveying);
        keevoro_scan::survey::survey(&root, &keevoro_core::SkipRules::default(), Some(&stop), &mut |p| {
            let _ = app.emit(
                SURVEY,
                SurveyEvent {
                    folders: p.folders_seen,
                    files: p.files_seen,
                    photos: p.photos,
                    videos: p.videos,
                },
            );
        })
    })
    .await
    .map_err(say)?;
    let seconds = started.elapsed().as_secs_f32();
    shared.keevoro.log().say(&format!(
        "Looked at {} files in {} folders in {seconds:.1} seconds without opening any: {} folders suggested{}",
        found.files_seen,
        found.folders_seen,
        found.hints.len(),
        if found.stopped { " (stopped early)" } else { "" }
    ));
    Ok(SurveyView {
        hints: found
            .hints
            .into_iter()
            .map(|h| {
                let folder = keevoro_engine::friendly_path(&h.folder);
                HintView {
                    already_a_source: sources.contains(&folder),
                    folder,
                    photos: h.photos,
                    videos: h.videos,
                    bytes: h.bytes,
                    likely_photos: h.likely == keevoro_scan::survey::Likely::Photos,
                }
            })
            .collect(),
        folders: found.folders_seen,
        files: found.files_seen,
        unreadable: found.unreadable,
        seconds,
        stopped: found.stopped,
    })
}

/// Asks a running survey to stop. It answers with what it has so far.
#[tauri::command]
pub fn stop_survey(shared: State<'_, Shared>) {
    shared.stop_survey.store(true, Ordering::SeqCst);
}

/// Starts looking through a drive or folder. Returns at once; progress and
/// the result arrive as `scan` events.
#[tauri::command]
pub fn scan_place(app: AppHandle, shared: State<'_, Shared>, path: String) -> Said<()> {
    let library = library(&shared)?;
    if shared.scanning.swap(true, Ordering::SeqCst) {
        return Err("Keevoro is already looking through a source. Let that finish first.".into());
    }
    // A fresh start: an earlier request to stop does not carry over.
    shared.stop_scanning.store(false, Ordering::SeqCst);
    let stop = shared.stop_scanning.clone();
    let source = PathBuf::from(path);
    let name = source
        .file_name()
        .map_or_else(|| source.display().to_string(), |n| n.to_string_lossy().to_string());

    std::thread::spawn(move || {
        let started = Instant::now();
        let state = app.state::<Shared>();
        let log = state.keevoro.log();
        log.say(&format!("Looking through {}", keevoro_engine::friendly_path(&source)));
        let mut last_said = Instant::now();
        let mut latest = (0, 0, 0);
        let mut announced: Option<PathBuf> = None;
        let event = |p: &keevoro_scan::ScanProgress, saving: bool| ScanEvent {
            name: name.clone(),
            photos: p.photos,
            videos: p.videos,
            files: p.files_seen,
            folder: keevoro_engine::friendly_path(&p.folder),
            reading: p
                .reading
                .as_ref()
                .map(|(path, bytes)| (keevoro_engine::friendly_path(path), *bytes)),
            saving,
            stopped: false,
            finished: false,
            problem: None,
            unreadable: 0,
            unreadable_names: Vec::new(),
        };
        let mut last_progress = keevoro_scan::ScanProgress::default();
        let result = Catalog::open_in_library(&library)
            .map_err(anyhow::Error::from)
            .and_then(|mut catalog| {
                let last_progress = std::cell::RefCell::new(&mut last_progress);
                keevoro_engine::scan_into_with(
                    &mut catalog,
                    &source,
                    Some(&name),
                    &keevoro_scan::ScanOptions {
                        quick: true,
                        stop: Some(stop.clone()),
                        ..Default::default()
                    },
                    &mut |p| {
                        latest = (p.photos, p.videos, p.files_seen);
                        **last_progress.borrow_mut() = p.clone();
                        // A large file starting is always said at once: it
                        // may be the last thing to change for minutes.
                        let big = p.reading.as_ref().map(|(path, _)| path.clone());
                        let news = big.is_some() && big != announced;
                        if news {
                            if let Some((path, bytes)) = &p.reading {
                                log.say(&format!(
                                    "Reading a large file ({}): {}",
                                    keevoro_engine::summary::bytes(*bytes),
                                    keevoro_engine::friendly_path(path)
                                ));
                            }
                            announced = big;
                        }
                        if news || last_said.elapsed() >= PROGRESS_EVERY {
                            last_said = Instant::now();
                            let _ = app.emit(SCAN, event(p, false));
                        }
                    },
                    &mut || {
                        let p = last_progress.borrow();
                        log.say(&format!(
                            "Finished looking after {} files; saving what was found",
                            p.files_seen
                        ));
                        let _ = app.emit(SCAN, event(&p, true));
                    },
                )
            });
        state.scanning.store(false, Ordering::SeqCst);
        let was_stopped = result.as_ref().is_ok_and(|report| report.stopped);
        match &result {
            Ok(report) if report.stopped => log.say(&format!(
                "Stopped looking through {} after {} seconds and {} files. Nothing from it was kept.",
                keevoro_engine::friendly_path(&source),
                started.elapsed().as_secs(),
                latest.2
            )),
            Ok(report) => {
                log.say(&format!(
                    "Looked through {} in {} seconds: {} photos and videos, {} other files, {} folders skipped, {} unreadable",
                    keevoro_engine::friendly_path(&source),
                    started.elapsed().as_secs(),
                    report.items.len(),
                    report.other_files,
                    report.skipped.len(),
                    report.unreadable.len()
                ));
                for unreadable in report.unreadable.iter().take(200) {
                    log.say(&format!(
                        "Could not read {}: {}",
                        keevoro_engine::friendly_path(&unreadable.path),
                        unreadable.error
                    ));
                }
            }
            Err(e) => log.say(&format!("Looking through {} failed: {e:#}", source.display())),
        }
        let _ = app.emit(
            SCAN,
            ScanEvent {
                name,
                photos: latest.0,
                videos: latest.1,
                files: latest.2,
                folder: String::new(),
                reading: None,
                saving: false,
                stopped: was_stopped,
                finished: true,
                // What could not be read is said on the screen, not only in the log.
                unreadable: result.as_ref().map_or(0, |r| r.unreadable.len()),
                unreadable_names: result.as_ref().map_or_else(
                    |_| Vec::new(),
                    |r| {
                        r.unreadable
                            .iter()
                            .take(5)
                            .map(|u| keevoro_engine::friendly_path(&u.path))
                            .collect()
                    },
                ),
                problem: result.err().map(say),
            },
        );
        check_in_background(&app);
    });
    Ok(())
}

/// Starts gathering everything the user is keeping. Returns at once;
/// progress and the result arrive as `gather` events.
#[tauri::command]
pub fn gather(app: AppHandle, shared: State<'_, Shared>) -> Said<()> {
    let library = library(&shared)?;
    if shared.gathering.swap(true, Ordering::SeqCst) {
        return Err("Keevoro is already gathering your photos.".into());
    }
    std::thread::spawn(move || {
        let state = app.state::<Shared>();
        state.keevoro.log().say("Gathering started");
        let mut stage = "";
        let tools = VideoTools::find();
        let result = Catalog::open_in_library(&library)
            .map_err(anyhow::Error::from)
            .and_then(|catalog| {
                // Converting old videos is opt-in and not offered in the
                // app yet, so it stays off here.
                keevoro_engine::gather_everything(&library, &catalog, false, tools.as_ref(), &mut |step| {
                    let now = match step {
                        keevoro_engine::GatherStep::Checking { .. } => "checking files before copying",
                        keevoro_engine::GatherStep::Copying { .. } => "copying",
                        keevoro_engine::GatherStep::MakingPreviews { .. } => "making previews",
                        keevoro_engine::GatherStep::WorkingOnVideos { .. } => "working on videos",
                    };
                    if now != stage {
                        stage = now;
                        state.keevoro.log().say(&format!("Gathering: {now}"));
                    }
                    let _ = app.emit(GATHER, GatherEvent::from(step));
                })
            });
        state.gathering.store(false, Ordering::SeqCst);
        match &result {
            Ok(Some(outcome)) => state
                .keevoro
                .log()
                .say(&format!("Gathering finished: {}", outcome.sentences().join(" "))),
            Ok(None) => state.keevoro.log().say("Gathering: there was nothing to gather"),
            Err(e) => state.keevoro.log().say(&format!("Gathering failed: {e:#}")),
        }
        let last = match result {
            Ok(Some(outcome)) => GatherEvent::finished(outcome.sentences(), None),
            Ok(None) => GatherEvent::finished(vec!["There was nothing to gather.".to_string()], None),
            Err(e) => GatherEvent::finished(Vec::new(), Some(say(e))),
        };
        let _ = app.emit(GATHER, last);
        check_in_background(&app);
    });
    Ok(())
}

/// Where this library is backed up to, in words, or `None` if nowhere yet.
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

/// Shows a folder Keevoro has mentioned in the computer's file manager.
/// Only folders it knows about are opened: a source, where this backup
/// sends to, or the backup's own folder. Anything else is refused, so a
/// screen can never be used to open an arbitrary place.
#[tauri::command]
pub fn open_place(app: AppHandle, shared: State<'_, Shared>, path: String) -> Said<()> {
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
pub async fn connect_destination(app: AppHandle, shared: State<'_, Shared>, request: ConnectRequest) -> Said<String> {
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

/// Runs a backup job on its own thread, reporting with `backup` events.
fn send_job(
    app: AppHandle,
    shared: &Shared,
    record: bool,
    job: impl FnOnce(
            &std::path::Path,
            &Catalog,
            &World<'_>,
            &mut Vec<u8>,
            &mut dyn FnMut(&str),
            &mut Vec<keevoro_engine::history::Sent>,
        ) -> anyhow::Result<()>
        + Send
        + 'static,
) -> Said<()> {
    let library = library(shared)?;
    if shared.sending.swap(true, Ordering::SeqCst) {
        return Err("Keevoro is already working on your backup. Let that finish first.".into());
    }
    // Said at once, before the first real step has anything to report.
    let opening = if record {
        "Getting ready: reading what Keevoro knows about this backup"
    } else {
        "Getting ready to check"
    };
    shared.keevoro.note_backup_step(opening);
    let _ = app.emit(SEND, SendEvent::progress(opening));
    // What an earlier backup said when it ended is about that one. A new
    // one starts with a clean slate, so "Stopped" is never left showing
    // beside a backup that is running.
    if record {
        let _ = Backup::open(&library).and_then(|b| b.forget_last_summary());
    }
    std::thread::spawn(move || {
        let mut said = Vec::new();
        let mut last_said = Instant::now() - PROGRESS_EVERY;
        // How each destination's turn went, for the backup's record.
        let mut turns = Vec::new();
        let result = (|| {
            let catalog = Catalog::open_in_library(&library)?;
            let state = app.state::<Shared>();
            let world = World {
                secrets: state.keevoro.secrets(),
                ask: &connect::testing::nobody_to_ask,
            };
            job(
                &library,
                &catalog,
                &world,
                &mut said,
                &mut |line| {
                    // Remembered every time, shown a few times a second.
                    state.keevoro.note_backup_step(line);
                    if last_said.elapsed() >= PROGRESS_EVERY {
                        last_said = Instant::now();
                        let _ = app.emit(SEND, SendEvent::progress(line));
                    }
                },
                &mut turns,
            )
        })();
        let state = app.state::<Shared>();
        state.sending.store(false, Ordering::SeqCst);
        state.keevoro.clear_backup_step();
        // How it went, said to the person and written into the backup's
        // record, so "was that slow?" has an answer afterwards.
        if record && result.is_ok() {
            let several = turns.len() > 1;
            for sent in &turns {
                if let Some(sentence) = sent.sentence() {
                    // With more than one destination, each sentence says whose it is.
                    let line = if several {
                        format!("To {}: {}", sent.to, sentence.to_lowercase())
                    } else {
                        sentence
                    };
                    said.extend_from_slice(line.as_bytes());
                    said.push(b'\n');
                    if let Ok(catalog) = Catalog::open_in_library(&library) {
                        let _ = keevoro_engine::history::record(&catalog, sent);
                    }
                }
            }
        }
        match &result {
            Ok(()) => state.keevoro.log().say(&format!(
                "Backup job finished: {}",
                String::from_utf8_lossy(&said).trim()
            )),
            Err(e) => state.keevoro.log().say(&format!("Backup job failed: {e:#}")),
        }
        let last = SendEvent::finished(&said, result.err().map(say));
        // What a backup said when it ended is kept with the backup, so it
        // is still there after looking at another screen or closing the
        // app, until the next backup replaces it.
        if record {
            let _ = Backup::open(&library).and_then(|b| b.remember_last_summary(&last));
        }
        let _ = app.emit(SEND, last);
        // Whatever the backup did not fingerprint is picked up again now.
        check_in_background(&app);
    });
    Ok(())
}

/// What this backup's last run said when it ended, or nothing if it has
/// never run or now goes somewhere else.
#[tauri::command]
pub fn last_backup_said(shared: State<'_, Shared>) -> Said<Option<serde_json::Value>> {
    backup(&shared)?.last_summary().map_err(say)
}

/// Starts backing the library up to where it is connected.
#[tauri::command]
pub fn send(app: AppHandle, shared: State<'_, Shared>) -> Said<()> {
    // A fresh start: an earlier request to stop does not carry over.
    shared.stop_sending.store(false, Ordering::SeqCst);
    let stop = shared.stop_sending.clone();
    // What this send sets out to do, so its speed can become a time left.
    if let Ok(library) = library(&shared) {
        let waiting = Catalog::open_in_library(&library)
            .ok()
            .and_then(|catalog| keevoro_engine::discover(&library, &catalog).ok())
            .map_or(0, |found| {
                // What every destination still lacks, added up.
                let everywhere: u64 = found.destinations.iter().map(|d| d.waiting_bytes).sum();
                everywhere.max(found.waiting_bytes)
            });
        shared.send_total.store(waiting, Ordering::SeqCst);
        shared
            .send_began_at
            .store(keevoro_core::meter::totals().0, Ordering::SeqCst);
    }
    send_job(
        app,
        &shared,
        true,
        move |library, catalog, world, said, progress, turns| {
            // To each destination in turn. The files themselves go first;
            // previews are made and sent after.
            keevoro_engine::send::everywhere(
                said,
                catalog,
                library,
                world,
                VideoTools::find().as_ref(),
                &stop,
                progress,
                &mut |turn| {
                    turns.push(keevoro_engine::history::Sent {
                        finished_at: std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .ok()
                            .and_then(|d| i64::try_from(d.as_secs()).ok())
                            .unwrap_or(0),
                        seconds: turn.seconds,
                        bytes: turn.bytes,
                        to: turn.to.describe(),
                        stopped: turn.stopped,
                    });
                },
            )
        },
    )
}

/// Asks a send that is running to stop. The file in hand is finished, so
/// nothing is left half-sent; everything already sent stays where it is.
#[tauri::command]
pub fn stop_sending(shared: State<'_, Shared>) {
    shared.stop_sending.store(true, Ordering::SeqCst);
    // A paused job is waiting, and must wake to see it has been asked to stop.
    keevoro_core::pause::set_paused(false);
    shared.keevoro.log().say("Asked to stop sending after the file in hand");
}

/// Starts checking the library and its backup are both intact.
#[tauri::command]
pub fn check_arrived(app: AppHandle, shared: State<'_, Shared>) -> Said<()> {
    send_job(
        app,
        &shared,
        false,
        |library, catalog, world, said, progress, _turns| {
            let all = connect::destinations(catalog)?;
            if all.is_empty() {
                anyhow::bail!("Choose where to back up to first.");
            }
            let several = all.len() > 1;
            for to in &all {
                if several {
                    said.extend_from_slice(format!("At {}:\n", to.describe()).as_bytes());
                }
                let destination = to.open(world)?;
                keevoro_engine::send::check(said, catalog, library, Some(destination), false, progress)?;
            }
            Ok(())
        },
    )
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
pub fn open_release_page(app: AppHandle, page: String) -> Said<()> {
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
pub fn open_log(app: AppHandle, shared: State<'_, Shared>) -> Said<()> {
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_path(shared.keevoro.log().path().display().to_string(), None::<&str>)
        .map_err(say)
}

/// A newer Keevoro, if there is one this computer can install.
#[tauri::command]
pub async fn check_update(app: AppHandle) -> Said<Option<keevoro_api::Offer>> {
    tauri::async_runtime::spawn_blocking(move || app.state::<Shared>().keevoro.update_offer())
        .await
        .map_err(say)
}

/// Downloads the newer Keevoro, checks it, and starts installing it. On
/// Windows the app then closes so the installer can replace it.
#[tauri::command]
pub async fn install_update(app: AppHandle) -> Said<()> {
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

/// Shows the library folder in the system's file manager, so the user can
/// see their photos are really there.
#[tauri::command]
pub fn open_library(app: AppHandle, shared: State<'_, Shared>) -> Said<()> {
    use tauri_plugin_opener::OpenerExt;
    let library = library(&shared)?;
    // The originals are what people want to see; fall back to the library
    // itself before anything has been gathered.
    let originals = library.join("originals");
    let show = if originals.is_dir() { originals } else { library };
    app.opener()
        .open_path(show.display().to_string(), None::<&str>)
        .map_err(say)
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
