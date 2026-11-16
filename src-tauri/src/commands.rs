//! The requests the screens can make, one function each.
//!
//! Every function here does the same three things: find the library, call
//! the engine, and turn any error into a sentence the screen can show. If a
//! function grows beyond that, the extra belongs in `keevoro-engine`.

use crate::events::{BackupEvent, CheckEvent, GatherEvent, ScanEvent, BACKUP, CHECK, GATHER, SCAN};
use crate::Shared;
use keevoro_catalog::Catalog;
use keevoro_engine::connect::{self, Flags, Keychain, Saved, World};
use keevoro_engine::{backup, Discover, PhotoChoice};
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
    library: Option<String>,
    suggested_library: String,
    version: &'static str,
}

fn current_state(app: &AppHandle, shared: &Shared) -> AppState {
    let library = shared.settings.lock().ok().and_then(|s| s.library.clone());
    // The Pictures folder is where people expect photos to live.
    let pictures = app
        .path()
        .picture_dir()
        .or_else(|_| app.path().home_dir())
        .unwrap_or_else(|_| PathBuf::from("."));
    AppState {
        library: library.map(|p| p.display().to_string()),
        suggested_library: pictures.join("Keevoro Library").display().to_string(),
        version: env!("CARGO_PKG_VERSION"),
    }
}

fn library(shared: &Shared) -> Said<PathBuf> {
    shared
        .settings
        .lock()
        .ok()
        .and_then(|s| s.library.clone())
        .ok_or_else(|| "Choose where your library will live first.".to_string())
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

/// A small picture of one found file, as raw WebP bytes.
#[tauri::command]
pub async fn preview(shared: State<'_, Shared>, id: String) -> Said<tauri::ipc::Response> {
    let library = library(&shared)?;
    let cache = shared.cache_dir.clone();
    // Making a preview reads a file and shrinks a picture; keep that off
    // the thread that draws the window.
    let bytes = tauri::async_runtime::spawn_blocking(move || {
        let catalog = Catalog::open_in_library(&library)?;
        keevoro_engine::preview(&library, &catalog, &cache, &id, VideoTools::find().as_ref())
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

/// Starts looking through a drive or folder. Returns at once; progress and
/// the result arrive as `scan` events.
#[tauri::command]
pub fn scan_place(app: AppHandle, shared: State<'_, Shared>, path: String) -> Said<()> {
    let library = library(&shared)?;
    if shared.scanning.swap(true, Ordering::SeqCst) {
        return Err("Keevoro is already looking through a source. Let that finish first.".into());
    }
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
        match &result {
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
pub fn backup_state(shared: State<'_, Shared>) -> Said<Option<String>> {
    let catalog = Catalog::open_in_library(&library(&shared)?).map_err(say)?;
    Ok(Saved::load(&catalog).map_err(say)?.map(|s| s.describe()))
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
pub async fn connect_backup(shared: State<'_, Shared>, request: ConnectRequest) -> Said<String> {
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
pub fn disconnect_backup(shared: State<'_, Shared>) -> Said<()> {
    let catalog = Catalog::open_in_library(&library(&shared)?).map_err(say)?;
    let world = World {
        secrets: &Keychain,
        ask: &connect::testing::nobody_to_ask,
    };
    connect::disconnect(&mut Vec::new(), &catalog, &world).map_err(say)
}

/// Runs a backup job on its own thread, reporting with `backup` events.
fn backup_job(
    app: AppHandle,
    shared: &Shared,
    job: impl FnOnce(
            &std::path::Path,
            &Catalog,
            backup::Destination,
            Option<&str>,
            &mut Vec<u8>,
            &mut dyn FnMut(&str),
        ) -> anyhow::Result<()>
        + Send
        + 'static,
) -> Said<()> {
    let library = library(shared)?;
    if shared.backing_up.swap(true, Ordering::SeqCst) {
        return Err("Keevoro is already working on your backup. Let that finish first.".into());
    }
    std::thread::spawn(move || {
        let mut said = Vec::new();
        let mut last_said = Instant::now() - PROGRESS_EVERY;
        let result = (|| {
            let catalog = Catalog::open_in_library(&library)?;
            let saved = Saved::load(&catalog)?.ok_or_else(|| anyhow::anyhow!("Choose where to back up to first."))?;
            let world = World {
                secrets: &Keychain,
                ask: &connect::testing::nobody_to_ask,
            };
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
                        let _ = app.emit(BACKUP, BackupEvent::progress(line));
                    }
                },
            )
        })();
        let state = app.state::<Shared>();
        state.backing_up.store(false, Ordering::SeqCst);
        match &result {
            Ok(()) => state.log.say(&format!(
                "Backup job finished: {}",
                String::from_utf8_lossy(&said).trim()
            )),
            Err(e) => state.log.say(&format!("Backup job failed: {e:#}")),
        }
        let _ = app.emit(BACKUP, BackupEvent::finished(&said, result.err().map(say)));
    });
    Ok(())
}

/// Starts backing the library up to where it is connected.
#[tauri::command]
pub fn back_up(app: AppHandle, shared: State<'_, Shared>) -> Said<()> {
    backup_job(
        app,
        &shared,
        |library, catalog, destination, base_url, said, progress| {
            backup::run(said, catalog, library, destination, base_url, false, progress)
        },
    )
}

/// Starts checking the library and its backup are both intact.
#[tauri::command]
pub fn check_backup(app: AppHandle, shared: State<'_, Shared>) -> Said<()> {
    backup_job(
        app,
        &shared,
        |library, catalog, destination, _base_url, said, progress| {
            backup::check(said, catalog, library, Some(destination), false, progress)
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
