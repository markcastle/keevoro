//! The requests the screens can make, one function each.
//!
//! Every function here does the same three things: find the library, call
//! the engine, and turn any error into a sentence the screen can show. If a
//! function grows beyond that, the extra belongs in `keevoro-engine`.

use crate::events::{CheckEvent, GatherEvent, ScanEvent, SendEvent, CHECK, GATHER, SCAN, SEND};
use crate::Shared;
use keevoro_catalog::Catalog;
use keevoro_engine::connect::{self, Flags, Keychain, Saved, World};
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
    let settings = shared.settings.lock().ok().map(|s| s.clone()).unwrap_or_default();
    let view = |s: &crate::settings::Backup| BackupView {
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
        .settings
        .lock()
        .ok()
        .and_then(|s| s.open_backup().map(|open| open.home.clone()))
        .ok_or_else(|| "Start a backup first.".to_string())
}

/// Starts a new backup with this name and opens it.
#[tauri::command]
pub fn start_backup(app: AppHandle, shared: State<'_, Shared>, name: String) -> Said<AppState> {
    let backups_dir = app.path().app_data_dir().map_err(say)?.join("backups");
    {
        let mut settings = shared.settings.lock().map_err(say)?;
        let backup = settings.start_backup(&name, &backups_dir).clone();
        // Opening the catalog proves the folder can be made and written to.
        Catalog::open_in_library(&backup.home).map_err(say)?;
        settings.save(&shared.config_dir).map_err(say)?;
        shared.log.say(&format!("Started backup \"{}\"", backup.name));
    }
    Ok(current_state(&app, &shared))
}

/// Opens another backup. Whatever was open is left exactly as it was.
#[tauri::command]
pub fn switch_backup(app: AppHandle, shared: State<'_, Shared>, id: String) -> Said<AppState> {
    if shared.scanning.load(Ordering::SeqCst) || shared.sending.load(Ordering::SeqCst) {
        return Err("Keevoro is in the middle of something in this backup. Let that finish first.".into());
    }
    {
        let mut settings = shared.settings.lock().map_err(say)?;
        if !settings.switch_to(&id) {
            return Err("That backup is no longer there.".into());
        }
        settings.save(&shared.config_dir).map_err(say)?;
        if let Some(open) = settings.open_backup() {
            shared.log.say(&format!("Switched to backup \"{}\"", open.name));
        }
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
    let data_dir = app.path().app_data_dir().map_err(say)?;
    {
        let mut settings = shared.settings.lock().map_err(say)?;
        // Keys first, while each backup's notes can still be read. A key
        // that cannot be removed is not a reason to stop.
        for backup in &settings.backups {
            if let Ok(catalog) = Catalog::open_in_library(&backup.home) {
                let world = World {
                    secrets: &Keychain,
                    ask: &connect::testing::nobody_to_ask,
                };
                let _ = connect::disconnect(&mut Vec::new(), &catalog, &world);
            }
        }
        let done = crate::reset::reset(&mut settings, &shared.config_dir, &data_dir, &shared.cache_dir).map_err(say)?;
        shared.log.say(&format!(
            "Reset to how Keevoro was when first installed: {} backups removed, {} folders left where they are",
            done.removed.len(),
            done.left_alone.len()
        ));
        for folder in &done.left_alone {
            shared.log.say(&format!(
                "Left alone, because it is not Keevoro's own: {}",
                keevoro_engine::friendly_path(folder)
            ));
        }
    }
    Ok(current_state(&app, &shared))
}

/// Shows the folder this backup sends to, if it sends to a folder.
#[tauri::command]
pub fn open_destination(app: AppHandle, shared: State<'_, Shared>) -> Said<()> {
    use tauri_plugin_opener::OpenerExt;
    let catalog = Catalog::open_in_library(&library(&shared)?).map_err(say)?;
    match Saved::load(&catalog).map_err(say)? {
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
    let path = PathBuf::from(path);
    // Opening the catalog proves the folder can be created and written to.
    Catalog::open_in_library(&path).map_err(say)?;
    {
        let mut settings = shared.settings.lock().map_err(say)?;
        // Kept for the first-run screen of older builds: the folder becomes
        // a backup of its own.
        let id = format!("library-{}", settings.backups.len() + 1);
        settings.backups.push(crate::settings::Backup {
            id,
            name: "My photos".to_string(),
            home: path.clone(),
        });
        settings.current = settings.backups.last().map(|s| s.id.clone());
        settings.library = Some(path);
        settings.save(&shared.config_dir).map_err(say)?;
    }
    Ok(current_state(&app, &shared))
}

#[tauri::command]
pub fn discover(shared: State<'_, Shared>) -> Said<Discover> {
    let library = library(&shared)?;
    let catalog = Catalog::open_in_library(&library).map_err(say)?;
    keevoro_engine::discover(&library, &catalog).map_err(say)
}

#[tauri::command]
pub fn keep_collection(shared: State<'_, Shared>, key: String, keep: bool) -> Said<()> {
    let catalog = Catalog::open_in_library(&library(&shared)?).map_err(say)?;
    keevoro_engine::set_collection_kept(&catalog, &key, keep).map_err(say)?;
    Ok(())
}

/// Every file in one collection, for ticking and unticking.
#[tauri::command]
pub fn collection_photos(shared: State<'_, Shared>, key: String) -> Said<Vec<PhotoChoice>> {
    let library = library(&shared)?;
    let catalog = Catalog::open_in_library(&library).map_err(say)?;
    keevoro_engine::collection_photos(&library, &catalog, &key).map_err(say)
}

/// Stops remembering a source. Its files are never touched.
#[tauri::command]
pub fn forget_source(shared: State<'_, Shared>, id: i64) -> Said<()> {
    let mut catalog = Catalog::open_in_library(&library(&shared)?).map_err(say)?;
    keevoro_engine::forget_source(&mut catalog, id).map_err(say)?;
    shared.log.say(&format!(
        "Removed source {id} from the list (its files were not touched)"
    ));
    Ok(())
}

/// One kind of file found on a source, for the "File types" list.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileTypeView {
    ending: String,
    files: u64,
    bytes: u64,
    included: bool,
}

/// The kinds of file found on a source, most numerous first.
#[tauri::command]
pub fn file_types(shared: State<'_, Shared>, id: i64) -> Said<Vec<FileTypeView>> {
    let catalog = Catalog::open_in_library(&library(&shared)?).map_err(say)?;
    Ok(catalog
        .file_types(id)
        .map_err(say)?
        .into_iter()
        .map(|kind| FileTypeView {
            ending: kind.ending,
            files: kind.files,
            bytes: kind.bytes,
            included: kind.included,
        })
        .collect())
}

/// Includes or leaves out every file of one kind on one source. Nothing on
/// the source is touched; left-out files are simply not offered or sent.
#[tauri::command]
pub fn include_file_type(shared: State<'_, Shared>, id: i64, ending: String, included: bool) -> Said<()> {
    let catalog = Catalog::open_in_library(&library(&shared)?).map_err(say)?;
    catalog.set_type_included(id, &ending, included).map_err(say)?;
    shared.log.say(&format!(
        "{} .{ending} files on source {id}",
        if included { "Including" } else { "Leaving out" }
    ));
    Ok(())
}

/// Every file found more than once: which copy is kept, which are left out.
#[tauri::command]
pub fn duplicates(shared: State<'_, Shared>) -> Said<Vec<keevoro_engine::DuplicateGroup>> {
    let library = library(&shared)?;
    let catalog = Catalog::open_in_library(&library).map_err(say)?;
    keevoro_engine::duplicates(&library, &catalog).map_err(say)
}

/// The pictures set aside as probably not memories, with the reason.
#[tauri::command]
pub fn set_aside(shared: State<'_, Shared>) -> Said<Vec<PhotoChoice>> {
    let library = library(&shared)?;
    let catalog = Catalog::open_in_library(&library).map_err(say)?;
    keevoro_engine::set_aside(&library, &catalog).map_err(say)
}

#[tauri::command]
pub fn keep_photo(shared: State<'_, Shared>, id: String, keep: bool) -> Said<()> {
    let catalog = Catalog::open_in_library(&library(&shared)?).map_err(say)?;
    keevoro_engine::set_photo_kept(&catalog, &id, keep).map_err(say)
}

/// Lets only a few slow jobs run at once; the rest wait their turn.
struct Turns {
    free: std::sync::Mutex<usize>,
    freed: std::sync::Condvar,
}

/// Holding one of these is having a turn. Dropping it passes the turn on.
struct Turn<'a>(&'a Turns);

impl Turns {
    const fn new(at_once: usize) -> Self {
        Self {
            free: std::sync::Mutex::new(at_once),
            freed: std::sync::Condvar::new(),
        }
    }

    fn wait_for_a_turn(&self) -> Turn<'_> {
        // A poisoned lock only means another preview panicked; the count
        // inside is still right, so carry on with it.
        let mut free = self.free.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        while *free == 0 {
            free = self.freed.wait(free).unwrap_or_else(std::sync::PoisonError::into_inner);
        }
        *free -= 1;
        Turn(self)
    }
}

impl Drop for Turn<'_> {
    fn drop(&mut self) {
        *self.0.free.lock().unwrap_or_else(std::sync::PoisonError::into_inner) += 1;
        self.0.freed.notify_one();
    }
}

static PREVIEWS: Turns = Turns::new(3);

/// A small picture of one found file, as raw WebP bytes.
#[tauri::command]
pub async fn preview(shared: State<'_, Shared>, id: String) -> Said<tauri::ipc::Response> {
    picture(&shared, id, keevoro_engine::Look::Small).await
}

/// A screen-sized picture of one found file, for looking at it closely.
#[tauri::command]
pub async fn closer_look(shared: State<'_, Shared>, id: String) -> Said<tauri::ipc::Response> {
    picture(&shared, id, keevoro_engine::Look::Closer).await
}

async fn picture(shared: &State<'_, Shared>, id: String, look: keevoro_engine::Look) -> Said<tauri::ipc::Response> {
    let library = library(shared)?;
    let cache = shared.cache_dir.clone();
    // Making a preview reads a file and shrinks a picture; keep that off
    // the thread that draws the window.
    let bytes = tauri::async_runtime::spawn_blocking(move || {
        // A screen of pictures asks for all of its previews at once. A few
        // at a time keeps the computer responsive, above all for videos,
        // where each preview runs a separate program.
        let _turn = PREVIEWS.wait_for_a_turn();
        let catalog = Catalog::open_in_library(&library)?;
        keevoro_engine::preview_at(&library, &catalog, &cache, &id, VideoTools::find().as_ref(), look)
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
                            state.log.say(&format!("Checking {} files in the background", p.total));
                        }
                        if p.bytes >= 64 * 1024 * 1024 {
                            state.log.say(&format!(
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
                    &|| !state.scanning.load(Ordering::SeqCst) && !state.gathering.load(Ordering::SeqCst),
                )
            });
        state.checking.store(false, Ordering::SeqCst);
        let done = match &result {
            Ok(report) => {
                if report.checked > 0 || !report.failed.is_empty() || report.stopped {
                    state.log.say(&format!(
                        "Background checking {} after {} seconds: {} checked, {} could not be read",
                        if report.stopped { "paused" } else { "finished" },
                        started.elapsed().as_secs(),
                        report.checked,
                        report.failed.len()
                    ));
                }
                for (path, why) in report.failed.iter().take(200) {
                    state.log.say(&format!(
                        "Could not check {}: {why}",
                        keevoro_engine::friendly_path(path)
                    ));
                }
                report.checked
            }
            Err(e) => {
                state.log.say(&format!("Background checking failed: {e:#}"));
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
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccelerationView {
    /// The person's choice. On unless they switched it off.
    on: bool,
    /// False on a computer where Keevoro has no system codecs to ask, so
    /// the switch would change nothing.
    available: bool,
    /// The ways a photo will be decoded, in the order they are tried.
    ways: Vec<&'static str>,
    /// How many previews are made at once.
    cores: usize,
}

#[tauri::command]
pub fn acceleration() -> AccelerationView {
    let on = keevoro_media::decode::use_system();
    let cores = std::thread::available_parallelism().map_or(2, std::num::NonZero::get);
    AccelerationView {
        on,
        available: keevoro_media::decode::system_codecs_available(),
        ways: keevoro_media::Decoders::for_this_computer(on).names(),
        cores: (cores / 2).clamp(1, 4),
    }
}

/// Switches the use of the computer's own codecs on or off, and remembers.
#[tauri::command]
pub fn set_acceleration(shared: State<'_, Shared>, on: bool) -> Said<AccelerationView> {
    keevoro_media::decode::set_use_system(on);
    {
        let mut settings = shared.settings.lock().map_err(say)?;
        settings.plain_decoding = !on;
        settings.save(&shared.config_dir).map_err(say)?;
    }
    shared.log.say(if on {
        "Using this computer's own picture codecs again"
    } else {
        "Switched to the built-in picture decoder only"
    });
    Ok(acceleration())
}

/// The record of how hard Keevoro has been working, kept for as long as
/// the app runs.
pub static HISTORY: std::sync::Mutex<crate::activity::History> = std::sync::Mutex::new(crate::activity::History {
    samples: std::collections::VecDeque::new(),
    memory_bytes: 0,
});

/// What Keevoro is doing now, why, and how hard it has been working.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityView {
    paused: bool,
    /// What it is doing in one line, or `None` when idle.
    doing: Option<String>,
    jobs: Vec<crate::activity::Job>,
    /// One a second, oldest first, for the last two minutes at most.
    samples: Vec<crate::activity::Sample>,
    memory_bytes: u64,
    /// How many processor cores this computer has.
    cores: usize,
}

#[tauri::command]
pub fn activity(shared: State<'_, Shared>) -> ActivityView {
    let (samples, memory_bytes) = HISTORY
        .lock()
        .map(|h| (h.samples(), h.memory_bytes()))
        .unwrap_or_default();
    let now = keevoro_core::doing::now();
    ActivityView {
        paused: keevoro_core::pause::is_paused(),
        doing: crate::activity::in_a_line(&now),
        jobs: crate::activity::jobs(&now),
        samples,
        memory_bytes,
        cores: std::thread::available_parallelism().map_or(1, std::num::NonZero::get),
    }
}

/// This backup's earlier sends, newest first: when, how much, how long.
#[tauri::command]
pub fn send_history(shared: State<'_, Shared>) -> Said<Vec<keevoro_engine::history::Sent>> {
    let catalog = Catalog::open_in_library(&library(&shared)?).map_err(say)?;
    keevoro_engine::history::recent(&catalog).map_err(say)
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
}

#[tauri::command]
pub fn send_progress(shared: State<'_, Shared>) -> SendProgress {
    let total = shared.send_total.load(Ordering::SeqCst);
    let done = keevoro_core::meter::totals()
        .0
        .saturating_sub(shared.send_began_at.load(Ordering::SeqCst))
        .min(total);
    let samples = HISTORY.lock().map(|h| h.samples()).unwrap_or_default();
    let (speed, seconds_left) = crate::activity::speed_and_time_left(&samples, total - done);
    SendProgress {
        bytes_done: done,
        bytes_total: total,
        speed,
        seconds_left,
        over_network: samples.iter().rev().take(10).any(|s| s.network > 0.0),
        running: shared.sending.load(Ordering::SeqCst),
        stopping: shared.stop_sending.load(Ordering::SeqCst),
    }
}

/// Pauses or resumes every heavy job. Each waits at its next safe point,
/// between one file and the next, and carries on from there when resumed.
#[tauri::command]
pub fn pause_work(shared: State<'_, Shared>, paused: bool) {
    keevoro_core::pause::set_paused(paused);
    shared.log.say(if paused {
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
    shared.log.say("Asked to stop the search");
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
        let log = &state.log;
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
        state.log.say("Gathering started");
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
                        state.log.say(&format!("Gathering: {now}"));
                    }
                    let _ = app.emit(GATHER, GatherEvent::from(step));
                })
            });
        state.gathering.store(false, Ordering::SeqCst);
        match &result {
            Ok(Some(outcome)) => state
                .log
                .say(&format!("Gathering finished: {}", outcome.sentences().join(" "))),
            Ok(None) => state.log.say("Gathering: there was nothing to gather"),
            Err(e) => state.log.say(&format!("Gathering failed: {e:#}")),
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
#[tauri::command]
pub fn destination(shared: State<'_, Shared>) -> Said<Option<String>> {
    let catalog = Catalog::open_in_library(&library(&shared)?).map_err(say)?;
    Ok(Saved::load(&catalog).map_err(say)?.map(|s| s.describe()))
}

/// How much room what is waiting needs, and how that compares with where
/// it is going.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoomView {
    /// Bytes waiting to be sent.
    needed: u64,
    /// The folder being sent to, when it is a folder.
    folder: Option<String>,
    /// Bytes free on that folder's disk, when the system will say.
    free: Option<u64>,
    /// False only when the files are known not to fit.
    enough: bool,
}

/// Weighs what is waiting to be sent against where it is going.
#[tauri::command]
pub fn room(shared: State<'_, Shared>) -> Said<RoomView> {
    let library = library(&shared)?;
    let catalog = Catalog::open_in_library(&library).map_err(say)?;
    let needed = keevoro_engine::discover(&library, &catalog).map_err(say)?.waiting_bytes;
    let folder = Saved::load(&catalog)
        .map_err(say)?
        .filter(|saved| saved.kind == "folder")
        .map(|saved| saved.place);
    let room = folder
        .as_ref()
        .and_then(|folder| keevoro_engine::room::room_in(std::path::Path::new(folder), needed));
    Ok(RoomView {
        needed,
        folder,
        free: room.map(|r| r.free),
        enough: room.is_none_or(|r| r.is_enough()),
    })
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
    if let Some(saved) = Saved::load(&catalog).map_err(say)?.filter(|s| s.kind == "folder") {
        known.push(saved.place);
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

/// Checks a backup destination works, then remembers it. Returns it in words.
#[tauri::command]
pub async fn connect_destination(shared: State<'_, Shared>, request: ConnectRequest) -> Said<String> {
    let library = library(&shared)?;
    // Connecting talks to the storage, which can take a few seconds.
    tauri::async_runtime::spawn_blocking(move || {
        let place = Some(request.place.trim().to_string()).filter(|p| !p.is_empty());
        let mut flags = Flags {
            bucket: request.bucket.clone().filter(|b| !b.trim().is_empty()),
            ..Flags::default()
        };
        match request.kind.as_str() {
            "folder" => flags.to_folder = place.map(Into::into),
            "r2" => flags.to_r2 = place,
            "s3" => flags.to_s3 = place,
            _ => flags.to_endpoint = place,
        }
        let saved = Saved::from_flags(&flags).ok_or_else(|| anyhow::anyhow!("Say where to back up to first."))?;
        // The form supplies what a terminal would be asked for.
        let (access, secret) = (
            request.access_key_id.trim().to_string(),
            request.secret_access_key.trim().to_string(),
        );
        let ask = move |_: &str, hidden: bool| {
            let answer = if hidden { &secret } else { &access };
            if answer.is_empty() {
                anyhow::bail!("Enter both keys for the storage.");
            }
            Ok(answer.clone())
        };
        let world = World {
            secrets: &Keychain,
            ask: &ask,
        };
        let catalog = Catalog::open_in_library(&library)?;
        // Any warning (such as a publicly listable bucket) is said before
        // the confirmation; keep the warning, drop the command-line hints.
        let mut said = Vec::new();
        connect::connect(&mut said, &catalog, &saved, &world)?;
        Ok(saved.describe())
    })
    .await
    .map_err(say)?
    .map_err(|e: anyhow::Error| say(e))
}

/// Forgets the destination and removes its keys from the keychain.
#[tauri::command]
pub fn disconnect_destination(shared: State<'_, Shared>) -> Said<()> {
    let catalog = Catalog::open_in_library(&library(&shared)?).map_err(say)?;
    let world = World {
        secrets: &Keychain,
        ask: &connect::testing::nobody_to_ask,
    };
    connect::disconnect(&mut Vec::new(), &catalog, &world).map_err(say)
}

/// Runs a backup job on its own thread, reporting with `backup` events.
fn send_job(
    app: AppHandle,
    shared: &Shared,
    record: bool,
    job: impl FnOnce(
            &std::path::Path,
            &Catalog,
            keevoro_engine::send::Destination,
            Option<&str>,
            &mut Vec<u8>,
            &mut dyn FnMut(&str),
        ) -> anyhow::Result<()>
        + Send
        + 'static,
) -> Said<()> {
    let library = library(shared)?;
    if shared.sending.swap(true, Ordering::SeqCst) {
        return Err("Keevoro is already working on your backup. Let that finish first.".into());
    }
    std::thread::spawn(move || {
        let mut said = Vec::new();
        let mut last_said = Instant::now() - PROGRESS_EVERY;
        let started = Instant::now();
        let began_at = keevoro_core::meter::totals().0;
        let mut going_to = String::new();
        let result = (|| {
            let catalog = Catalog::open_in_library(&library)?;
            let saved = Saved::load(&catalog)?.ok_or_else(|| anyhow::anyhow!("Choose where to back up to first."))?;
            let world = World {
                secrets: &Keychain,
                ask: &connect::testing::nobody_to_ask,
            };
            going_to = saved.describe();
            let destination = saved.open(&world)?;
            let web_address = saved.base_url.clone();
            job(
                &library,
                &catalog,
                destination,
                web_address.as_deref(),
                &mut said,
                &mut |line| {
                    if last_said.elapsed() >= PROGRESS_EVERY {
                        last_said = Instant::now();
                        let _ = app.emit(SEND, SendEvent::progress(line));
                    }
                },
            )
        })();
        let state = app.state::<Shared>();
        state.sending.store(false, Ordering::SeqCst);
        // How it went, said to the person and written into the backup's
        // record, so "was that slow?" has an answer afterwards.
        if record && result.is_ok() {
            let sent = keevoro_engine::history::Sent {
                finished_at: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .ok()
                    .and_then(|d| i64::try_from(d.as_secs()).ok())
                    .unwrap_or(0),
                seconds: started.elapsed().as_secs(),
                bytes: keevoro_core::meter::totals().0.saturating_sub(began_at),
                to: going_to.clone(),
                stopped: state.stop_sending.load(Ordering::SeqCst),
            };
            if let Some(sentence) = sent.sentence() {
                said.extend_from_slice(sentence.as_bytes());
                said.push(b'\n');
                if let Ok(catalog) = Catalog::open_in_library(&library) {
                    let _ = keevoro_engine::history::record(&catalog, &sent);
                }
            }
        }
        match &result {
            Ok(()) => state.log.say(&format!(
                "Backup job finished: {}",
                String::from_utf8_lossy(&said).trim()
            )),
            Err(e) => state.log.say(&format!("Backup job failed: {e:#}")),
        }
        let _ = app.emit(SEND, SendEvent::finished(&said, result.err().map(say)));
    });
    Ok(())
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
            .map_or(0, |found| found.waiting_bytes);
        shared.send_total.store(waiting, Ordering::SeqCst);
        shared
            .send_began_at
            .store(keevoro_core::meter::totals().0, Ordering::SeqCst);
    }
    send_job(
        app,
        &shared,
        true,
        move |library, catalog, destination, base_url, said, progress| {
            // The files themselves go first; previews are made and sent after.
            keevoro_engine::send::originals_first(
                said,
                catalog,
                library,
                destination,
                base_url,
                VideoTools::find().as_ref(),
                &stop,
                progress,
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
    shared.log.say("Asked to stop sending after the file in hand");
}

/// Starts checking the library and its backup are both intact.
#[tauri::command]
pub fn check_arrived(app: AppHandle, shared: State<'_, Shared>) -> Said<()> {
    send_job(
        app,
        &shared,
        false,
        |library, catalog, destination, _base_url, said, progress| {
            keevoro_engine::send::check(said, catalog, library, Some(destination), false, progress)
        },
    )
}

/// The drives plugged into this computer, for the "Where are your photos?"
/// screen to offer as buttons.
#[tauri::command]
pub fn places() -> Vec<crate::places::Place> {
    crate::places::plugged_in()
}

/// Opens the page describing a release in the person's browser. Only
/// Keevoro's own release pages are opened, whatever the screen asks for.
#[tauri::command]
pub fn open_release_page(app: AppHandle, page: String) -> Said<()> {
    use tauri_plugin_opener::OpenerExt;
    if !page.starts_with("https://github.com/markcastle/keevoro/releases") {
        return Err("That isn't one of Keevoro's release pages.".to_string());
    }
    app.opener().open_url(page, None::<&str>).map_err(say)
}

/// The last few hundred lines of the log, for showing in Settings.
#[tauri::command]
pub fn log_tail(shared: State<'_, Shared>) -> String {
    shared.log.tail(400)
}

/// Opens the log file in whatever the computer uses for text files.
#[tauri::command]
pub fn open_log(app: AppHandle, shared: State<'_, Shared>) -> Said<()> {
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_path(shared.log.path().display().to_string(), None::<&str>)
        .map_err(say)
}

/// A newer Keevoro, if there is one this computer can install.
#[tauri::command]
pub async fn check_update() -> Said<Option<crate::updates::Offer>> {
    tauri::async_runtime::spawn_blocking(|| crate::updates::offer(env!("CARGO_PKG_VERSION"), std::env::consts::OS))
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
    use std::sync::atomic::AtomicUsize;

    #[test]
    fn only_a_few_slow_jobs_run_at_once() {
        static TURNS: Turns = Turns::new(2);
        static RUNNING: AtomicUsize = AtomicUsize::new(0);
        static MOST: AtomicUsize = AtomicUsize::new(0);
        let jobs: Vec<_> = (0..12)
            .map(|_| {
                std::thread::spawn(|| {
                    let _turn = TURNS.wait_for_a_turn();
                    let now = RUNNING.fetch_add(1, Ordering::SeqCst) + 1;
                    MOST.fetch_max(now, Ordering::SeqCst);
                    std::thread::sleep(Duration::from_millis(5));
                    RUNNING.fetch_sub(1, Ordering::SeqCst);
                })
            })
            .collect();
        for job in jobs {
            job.join().unwrap();
        }
        assert_eq!(MOST.load(Ordering::SeqCst), 2);
        assert_eq!(RUNNING.load(Ordering::SeqCst), 0);
    }
}
