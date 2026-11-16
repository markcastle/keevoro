//! The requests the screens can make, one function each.
//!
//! Every function here does the same three things: find the library, call
//! the engine, and turn any error into a sentence the screen can show. If a
//! function grows beyond that, the extra belongs in `keevoro-engine`.

use crate::events::{BackupEvent, GatherEvent, ScanEvent, BACKUP, GATHER, SCAN};
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
        let mut last_said = Instant::now();
        let mut latest = (0, 0);
        let result = Catalog::open_in_library(&library)
            .map_err(anyhow::Error::from)
            .and_then(|mut catalog| {
                keevoro_engine::scan_into(&mut catalog, &source, Some(&name), &mut |p| {
                    latest = (p.photos, p.videos);
                    if last_said.elapsed() >= PROGRESS_EVERY {
                        last_said = Instant::now();
                        let _ = app.emit(
                            SCAN,
                            ScanEvent {
                                name: name.clone(),
                                photos: p.photos,
                                videos: p.videos,
                                finished: false,
                                problem: None,
                            },
                        );
                    }
                })
            });
        app.state::<Shared>().scanning.store(false, Ordering::SeqCst);
        let _ = app.emit(
            SCAN,
            ScanEvent {
                name,
                photos: latest.0,
                videos: latest.1,
                finished: true,
                problem: result.err().map(say),
            },
        );
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
        let tools = VideoTools::find();
        let result = Catalog::open_in_library(&library)
            .map_err(anyhow::Error::from)
            .and_then(|catalog| {
                // Converting old videos is opt-in and not offered in the
                // app yet, so it stays off here.
                keevoro_engine::gather_everything(&library, &catalog, false, tools.as_ref(), &mut |step| {
                    let _ = app.emit(GATHER, GatherEvent::from(step));
                })
            });
        app.state::<Shared>().gathering.store(false, Ordering::SeqCst);
        let last = match result {
            Ok(Some(outcome)) => GatherEvent::finished(outcome.sentences(), None),
            Ok(None) => GatherEvent::finished(vec!["There was nothing to gather.".to_string()], None),
            Err(e) => GatherEvent::finished(Vec::new(), Some(say(e))),
        };
        let _ = app.emit(GATHER, last);
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
        app.state::<Shared>().backing_up.store(false, Ordering::SeqCst);
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
