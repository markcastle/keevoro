//! `sources.look` and `sources.look_again`: looking through a drive or a
//! folder quickly, and adding it to a backup as a source.
//!
//! A quick search reads only a little of each large file. What it only
//! sampled is read in full afterwards, in the background, by the check
//! for duplicates, which this starts again when it ends.

use super::{spawn, Context, DifferentDrive, Every, Job, JobHandle, JobKind, Listener, Said, ScanEvent};
use crate::agents::AgentAccess;
use crate::backups::Named;
use crate::error::{Error, Result};
use keevoro_catalog::Catalog;
use keevoro_core::recognise::{Mark, Recognised};
use keevoro_core::Kinds;
use keevoro_engine::friendly_path;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

/// How the person answered the question asked when the drive at a source's
/// place is not the one looked through there before.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum OtherDrive {
    /// Look through it as the source anyway, replacing what was found on
    /// it with what is there now.
    LookAnyway,
    /// Keep the source as it was, set aside for when its drive is plugged
    /// in again, and add the drive there now as a new source.
    AddAsNew,
}

/// One thing a search does, in turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Step {
    /// Looks through a place: a source, or a new one.
    Look(PathBuf),
    /// Sets the source with this id aside, keeping everything found on it,
    /// then looks through its place as a new source.
    SetAsideAndLook { id: i64, place: PathBuf },
    /// Brings a source that was set aside back to its place, setting aside
    /// whatever source is there meanwhile, then looks through it.
    BringBack { id: i64, place: PathBuf },
}

/// What a search will do, and what it passes over and why.
#[derive(Debug, Default)]
pub(super) struct ToLook {
    pub(super) steps: Vec<Step>,
    /// Sentences: sources not there, so not looked through.
    pub(super) passed_over: Vec<String>,
    /// Sources whose place holds another drive now.
    pub(super) different: Vec<DifferentDrive>,
}

impl ToLook {
    pub(super) fn one(place: PathBuf) -> Self {
        Self {
            steps: vec![Step::Look(place)],
            ..Self::default()
        }
    }

    /// The place the first step looks through, if any.
    fn first_place(&self) -> Option<&Path> {
        self.steps.first().map(|step| match step {
            Step::Look(place) | Step::SetAsideAndLook { place, .. } | Step::BringBack { place, .. } => place.as_path(),
        })
    }
}

/// The place a step looks through.
pub(super) fn place_of(step: &Step) -> &Path {
    match step {
        Step::Look(place) | Step::SetAsideAndLook { place, .. } | Step::BringBack { place, .. } => place,
    }
}

/// Where a source set aside is kept in the catalog: a place inside the
/// backup's own folder that is never made, so it is never looked through
/// and never mistaken for a real one.
fn set_aside_place(home: &Path, id: i64) -> PathBuf {
    home.join(".keevoro").join("set aside").join(id.to_string())
}

/// What a source is called when it is not there: the drive's name as the
/// computer gave it at the last look, for a whole drive, else its own.
pub(super) fn called(source: &keevoro_catalog::Source, mark: Option<&Mark>) -> (String, bool) {
    match mark.and_then(|m| m.drive.clone()) {
        Some(drive) => (drive, true),
        None if source.root.parent().is_none() => (source.name.clone(), true),
        None => (friendly_path(&source.root), false),
    }
}

/// Whether the drive at `place` now is `source`, which was last looked
/// through at its own root: by its volume, and when that differs, by a few
/// of the files found on it.
fn recognised_at(catalog: &Catalog, source: &keevoro_catalog::Source, mark: Option<&Mark>, place: &Path) -> Recognised {
    let now = keevoro_engine::reach::volume_of(place);
    keevoro_core::recognise::recognise(mark.and_then(|m| m.volume.as_deref()), now.as_deref(), || {
        let files = catalog.some_files_of(source.id).unwrap_or_default();
        let there = files
            .iter()
            .filter(|(path, bytes)| {
                let at = path
                    .strip_prefix(&source.root)
                    .map_or_else(|_| path.clone(), |rest| place.join(rest));
                std::fs::metadata(at).is_ok_and(|m| m.is_file() && m.len() == *bytes)
            })
            .count();
        (files.len(), there)
    })
}

/// The question about a source whose place holds another drive.
fn different(source: &keevoro_catalog::Source) -> DifferentDrive {
    let place = friendly_path(&source.root);
    DifferentDrive {
        source: source.id,
        name: source.name.clone(),
        asked: crate::words::different_drive_asked(&source.name),
        said: crate::words::different_drive_said(&source.name, &place),
        place,
    }
}

/// What `sources.look_again` does: the one source asked for, or every
/// one, in the order they were added. Passed over, so that what was found
/// on each is kept:
///
/// - a source the person left out (or one inside a folder left out), for
///   when it is put back;
/// - a source that is not there (a card in a drawer), said;
/// - a source whose place holds a different drive now, asked about, unless
///   `answer` says what to do with the one source asked for.
///
/// A source set aside before is brought back once its own drive is at its
/// place again.
pub(super) fn sources_to_look_again(backup: &Named, source: Option<i64>, answer: Option<OtherDrive>) -> Result<ToLook> {
    let (mut to_look, left_out) = what_to_look_again(backup, source, answer)?;
    if to_look.steps.is_empty() && to_look.different.is_empty() {
        return match (source, left_out, to_look.passed_over.len()) {
            (_, Some(folder), 0) => Err(Error::refused(left_out_said(&folder))),
            (Some(_), _, 1) => Err(Error::unreachable(to_look.passed_over.remove(0))),
            (None, _, n) if n > 0 => Err(Error::unreachable(crate::words::NONE_PLUGGED_IN.to_string())),
            (Some(_), _, _) => Err(Error::not_found("That source isn't part of this backup.")),
            (None, _, _) => Err(Error::refused(
                "This backup has no sources to look through again. Add one first.",
            )),
        };
    }
    Ok(to_look)
}

/// [`sources_to_look_again`] with nothing refused: for a backup that runs
/// by itself, which says each source it passes over even when it passes
/// over all of them. Also gives a folder left out that holds a source.
pub(super) fn what_to_look_again(
    backup: &Named,
    source: Option<i64>,
    answer: Option<OtherDrive>,
) -> Result<(ToLook, Option<String>)> {
    let catalog = Catalog::open_in_library(&backup.home)?;
    let sources = catalog.sources()?;
    let choices = catalog.choices()?;
    let asked: Vec<&keevoro_catalog::Source> = sources.iter().filter(|s| source.is_none_or(|id| s.id == id)).collect();
    let left_out = asked
        .iter()
        .find_map(|s| choices.left_out_folder(&s.root).map(friendly_path));
    let mut to_look = ToLook::default();
    let mut coming_back: Vec<PathBuf> = Vec::new();
    for s in asked.iter().filter(|s| choices.left_out_folder(&s.root).is_none()) {
        if keevoro_core::bucket::is_bucket_path(&s.root) {
            to_look.steps.push(Step::Look(s.root.clone()));
            continue;
        }
        let mark = catalog.source_mark(&s.root)?;
        if let Some(was_at) = mark.as_ref().and_then(|m| m.was_at.clone()).map(PathBuf::from) {
            if was_at.is_dir() && recognised_at(&catalog, s, mark.as_ref(), &was_at) != Recognised::Different {
                coming_back.push(was_at.clone());
                to_look.steps.push(Step::BringBack {
                    id: s.id,
                    place: was_at,
                });
            } else {
                let (name, drive) = called(s, mark.as_ref());
                to_look.passed_over.push(crate::words::not_plugged_in(&name, drive));
            }
            continue;
        }
        if !keevoro_engine::reach::is_there(&s.root) {
            let (name, drive) = called(s, mark.as_ref());
            to_look.passed_over.push(crate::words::not_plugged_in(&name, drive));
            continue;
        }
        match (
            recognised_at(&catalog, s, mark.as_ref(), &s.root),
            answer.filter(|_| source.is_some()),
        ) {
            (Recognised::Different, Some(OtherDrive::AddAsNew)) => to_look.steps.push(Step::SetAsideAndLook {
                id: s.id,
                place: s.root.clone(),
            }),
            (Recognised::Different, None) => to_look.different.push(different(s)),
            _ => to_look.steps.push(Step::Look(s.root.clone())),
        }
    }
    // A source whose place its own drive comes back to is set aside by
    // that step, so it is not asked about.
    to_look
        .different
        .retain(|d| !coming_back.iter().any(|p| friendly_path(p) == d.place));
    Ok((to_look, left_out))
}

/// What `sources.look` of `place` does: looks through it, unless it is a
/// source already and the drive there is not the one looked through there
/// before, which is asked about rather than replaced.
pub(super) fn place_to_look(backup: &Named, place: &Path) -> Result<ToLook> {
    let real = std::fs::canonicalize(place).unwrap_or_else(|_| place.to_path_buf());
    let catalog = Catalog::open_in_library(&backup.home)?;
    let Some(source) = catalog.sources()?.into_iter().find(|s| s.root == real) else {
        return Ok(ToLook::one(place.to_path_buf()));
    };
    let mark = catalog.source_mark(&source.root)?;
    Ok(match recognised_at(&catalog, &source, mark.as_ref(), &source.root) {
        Recognised::Different => ToLook {
            different: vec![different(&source)],
            ..ToLook::default()
        },
        _ => ToLook::one(place.to_path_buf()),
    })
}

/// Sets the source with this id aside, keeping all of it, remembering the
/// place it was at so it can be brought back.
fn set_aside(catalog: &mut Catalog, home: &Path, id: i64, place: &Path) -> Result<()> {
    let mut mark = catalog.source_mark(place)?.unwrap_or_default();
    mark.was_at = Some(place.to_string_lossy().into_owned());
    let away = set_aside_place(home, id);
    catalog.move_source(id, &away)?;
    catalog.set_source_mark(&away, &mark)?;
    Ok(())
}

/// Does what a step must before its place is looked through, and says it
/// in the log. Gives what to say on the screen, if anything.
pub(super) fn get_ready(ctx: &Context, home: &Path, step: &Step) -> Result<Option<String>> {
    let mut catalog = Catalog::open_in_library(home)?;
    match step {
        Step::Look(_) => Ok(None),
        Step::SetAsideAndLook { id, place } => {
            set_aside(&mut catalog, home, *id, place)?;
            ctx.log.say(&format!(
                "Set the source at {} aside, as a different drive is there now, keeping everything found on it; looking through the drive there as a new source",
                friendly_path(place)
            ));
            Ok(None)
        }
        Step::BringBack { id, place } => {
            let sources = catalog.sources()?;
            let name = sources
                .iter()
                .find(|s| s.id == *id)
                .map(|s| s.name.clone())
                .unwrap_or_default();
            let mut said = None;
            if let Some(other) = sources.iter().find(|s| s.root == *place && s.id != *id) {
                set_aside(&mut catalog, home, other.id, place)?;
                said = Some(crate::words::set_aside_for(&other.name, &name, &friendly_path(place)));
            }
            let away = set_aside_place(home, *id);
            let mut mark = catalog.source_mark(&away)?.unwrap_or_default();
            catalog.move_source(*id, place)?;
            mark.was_at = None;
            catalog.set_source_mark(place, &mark)?;
            ctx.log.say(&format!(
                "{name} is plugged in at {} again, so it is looked through there",
                friendly_path(place)
            ));
            if let Some(said) = &said {
                ctx.log.say(said);
            }
            Ok(said)
        }
    }
}

/// Why a place is not looked through: the person left it out.
pub(super) fn left_out_said(folder: &str) -> String {
    format!("You left {folder} out of this backup, so it isn't looked through. Put it back first to include it.")
}

/// Refuses to look through a place the person left out of `backup`, or
/// one inside such a folder, rather than finding nothing there.
pub(super) fn not_left_out(backup: &Named, place: &Path) -> Result<()> {
    let real = std::fs::canonicalize(place).unwrap_or_else(|_| place.to_path_buf());
    let folders = Catalog::open_in_library(&backup.home)?.left_out_folders()?;
    match folders.iter().find(|f| keevoro_core::places::within(&real, f)) {
        Some(folder) => Err(Error::refused(left_out_said(&friendly_path(folder)))),
        None => Ok(()),
    }
}

/// What a search is to look for, and how: the `kinds` of file the
/// settings named as it started, the folders to pass by, and, for a search
/// an AI agent asked for, what that agent may look in, which the check for
/// duplicates that follows is kept to.
pub(super) struct HowToLook {
    pub(super) kinds: Kinds,
    pub(super) skip: keevoro_core::SkipRules,
    pub(super) for_agent: Option<AgentAccess>,
}

/// Starts looking through what `to_look` says, one place after another.
pub(super) fn start(
    ctx: &Context,
    backup: &Named,
    to_look: ToLook,
    kind: JobKind,
    how: HowToLook,
    listener: Option<Listener>,
) -> Result<JobHandle> {
    let HowToLook { kinds, skip, for_agent } = how;
    let job = ctx.jobs.begin(kind, Some(backup), listener)?;
    let handle = job.handle.clone();
    // Said at once, so anyone who asks straight away hears it.
    if let Some(first) = to_look.first_place() {
        job.note_line(&format!("Looking through {}", name_of(first)));
    }
    for said in to_look
        .passed_over
        .iter()
        .chain(to_look.different.iter().map(|d| &d.said))
    {
        ctx.log.say(said);
    }
    let backup = backup.clone();
    spawn(ctx, &job, move |ctx, job, ending| {
        let ToLook {
            steps,
            mut passed_over,
            different,
        } = to_look;
        let mut last = None;
        for (n, step) in steps.iter().enumerate() {
            let said = match get_ready(ctx, &backup.home, step) {
                Ok(said) => {
                    passed_over.extend(said);
                    let (Step::Look(place) | Step::SetAsideAndLook { place, .. } | Step::BringBack { place, .. }) =
                        step;
                    let said = look_through(ctx, job, &backup.home, place, &kinds, &skip);
                    // A remembered map of anywhere this search covers may
                    // now be out of date, so it is surveyed again the next
                    // time it opens.
                    let real = std::fs::canonicalize(place).unwrap_or_else(|_| place.clone());
                    keevoro_engine::surveys::mark_out_of_date(&ctx.cache, &real);
                    said
                }
                Err(e) => ScanEvent {
                    finished: true,
                    problem: Some(e.message),
                    ..ScanEvent::default()
                },
            };
            // A search that was stopped stops the rest too.
            let more = n + 1 < steps.len() && !said.stopped && !job.stopped();
            if !more {
                last = Some(said);
                break;
            }
            ctx.jobs.heard(job, Said::Scan(said), true);
        }
        // What was passed over is said with the last word, so whoever hears
        // only that hears it too.
        let mut last = last.unwrap_or_else(|| ScanEvent {
            name: backup.name.clone(),
            finished: true,
            ..ScanEvent::default()
        });
        last.passed_over = passed_over;
        last.different_drive = different;
        // Another search may start the moment this one is heard to end.
        ctx.jobs.release(job);
        let last = Some(Said::Scan(last));
        if let Some(said) = &last {
            ctx.jobs.heard(job, said.clone(), true);
        }
        super::duplicates::start(ctx, &backup, for_agent, super::duplicates::Why::AfterWork);
        ending.with(last);
    })?;
    Ok(handle)
}

/// A source's friendly name: its folder's name, or the drive.
fn name_of(source: &Path) -> String {
    source
        .file_name()
        .map_or_else(|| source.display().to_string(), |n| n.to_string_lossy().to_string())
}

/// Looks through one place, saying how it goes, and gives the last event.
pub(super) fn look_through(
    ctx: &Context,
    job: &Arc<Job>,
    home: &Path,
    source: &Path,
    kinds: &Kinds,
    skip: &keevoro_core::SkipRules,
) -> ScanEvent {
    if keevoro_core::bucket::is_bucket_path(source) {
        return look_through_bucket(ctx, job, home, source, kinds, skip);
    }
    // A source looked through before keeps the name it has, which a person
    // may have given it; a new one is called after its folder, or the drive.
    let root = std::fs::canonicalize(source).unwrap_or_else(|_| source.to_path_buf());
    let name = Catalog::open_in_library(home)
        .and_then(|c| c.sources())
        .ok()
        .and_then(|sources| sources.into_iter().find(|s| s.root == root))
        .map_or_else(|| name_of(source), |s| s.name);
    // What to call it if it is unplugged part-way: the drive's own name
    // when the place is a whole drive. From the system's quick list, so it
    // costs next to nothing and never waits on a network share.
    let drive = crate::places::drive_name(source);
    let started = Instant::now();
    let log = &ctx.log;
    log.say(&format!("Looking through {}", friendly_path(source)));
    job.note_line(&format!("Looking through {name}"));
    let mut every = Every::after(ctx.jobs.progress_every());
    let mut latest = (0, 0, 0, 0);
    let mut announced: Option<PathBuf> = None;
    let event = |p: &keevoro_scan::ScanProgress, saving: bool| ScanEvent {
        name: name.clone(),
        photos: p.photos,
        videos: p.videos,
        others: p.others,
        files: p.files_seen,
        folder: friendly_path(&p.folder),
        reading: p.reading.as_ref().map(|(path, bytes)| (friendly_path(path), *bytes)),
        saving,
        ..ScanEvent::default()
    };
    let mut last_progress = keevoro_scan::ScanProgress::default();
    // What an earlier look through this place found, to say which files
    // are no longer offered if Keevoro's own folders are left out now.
    let found_before: Vec<PathBuf> = Catalog::open_in_library(home)
        .and_then(|c| c.load_reports())
        .map(|reports| {
            reports
                .into_iter()
                .filter(|r| r.root == root)
                .flat_map(|r| r.items.into_iter().map(|i| i.path))
                .collect()
        })
        .unwrap_or_default();
    let result = Catalog::open_in_library(home)
        .map_err(anyhow::Error::from)
        .and_then(|mut catalog| {
            let last_progress = std::cell::RefCell::new(&mut last_progress);
            keevoro_engine::scan_into_with(
                &mut catalog,
                source,
                Some(&name),
                &keevoro_scan::ScanOptions {
                    quick: true,
                    stop: Some(job.stop_switch()),
                    kinds: kinds.clone(),
                    skip: skip.clone(),
                    ..Default::default()
                },
                &mut |p| {
                    latest = (p.photos, p.videos, p.files_seen, p.others);
                    **last_progress.borrow_mut() = p.clone();
                    // A large file starting is always said at once: it may
                    // be the last thing to change for minutes.
                    let big = p.reading.as_ref().map(|(path, _)| path.clone());
                    let news = big.is_some() && big != announced;
                    if news {
                        if let Some((path, bytes)) = &p.reading {
                            log.say(&format!(
                                "Reading a large file ({}): {}",
                                keevoro_engine::summary::bytes(*bytes),
                                friendly_path(path)
                            ));
                        }
                        announced = big;
                    }
                    let tell = every.due_or(news);
                    ctx.jobs.heard(job, Said::Scan(event(p, false)), tell);
                },
                &mut || {
                    let p = last_progress.borrow();
                    log.say(&format!(
                        "Finished looking after {} files; saving what was found",
                        p.files_seen
                    ));
                    job.note_line(&format!("Saving what was found in {name}"));
                    ctx.jobs.heard(job, Said::Scan(event(&p, true)), true);
                },
            )
        });
    let was_stopped = result.as_ref().is_ok_and(|report| report.stopped);
    let unplugged = result
        .as_ref()
        .is_ok_and(|report| report.gone)
        .then(|| crate::words::unplugged_while_looking(drive.as_deref().unwrap_or(&name), drive.is_some()));
    if let Some(said) = &unplugged {
        log.say(&format!(
            "{} went part-way through looking, after {} files: {said}",
            friendly_path(source),
            latest.2
        ));
    }
    match &result {
        Ok(report) if report.stopped => log.say(&format!(
            "Stopped looking through {} after {} seconds and {} files. Nothing from it was kept.",
            friendly_path(source),
            started.elapsed().as_secs(),
            latest.2
        )),
        Ok(report) => {
            // What its drive is, so another drive at the same place is
            // never taken for it. Not after a look cut short by the drive
            // going, which may have left another in its place.
            if !report.gone {
                let mark = Mark {
                    volume: keevoro_engine::reach::volume_of(&report.root),
                    drive: drive.clone(),
                    was_at: None,
                };
                let kept = Catalog::open_in_library(home).and_then(|c| c.set_source_mark(&report.root, &mark));
                if let Err(e) = kept {
                    log.say(&format!(
                        "Couldn't remember which drive {} is on: {e}",
                        friendly_path(source)
                    ));
                }
            }
            let of = |kind| report.items.iter().filter(|i| i.kind == kind).count();
            let carried = of(keevoro_core::MediaKind::Other);
            // Said as ever, unless other files were carried along too.
            let (found, rest) = if carried == 0 {
                (format!("{} photos and videos", report.items.len()), "other files")
            } else {
                (
                    keevoro_core::words::files(
                        of(keevoro_core::MediaKind::Photo),
                        of(keevoro_core::MediaKind::Video),
                        carried,
                    ),
                    "files Keevoro does not look for",
                )
            };
            log.say(&format!(
                "Looked through {} in {} seconds: {found}, {} {rest}, {} folders skipped, {} unreadable",
                friendly_path(source),
                started.elapsed().as_secs(),
                report.other_files,
                report.skipped.len(),
                report.unreadable.len()
            ));
            if let Some(said) = online_only_said(report) {
                log.say(&format!("In {}: {said}", friendly_path(source)));
            }
            for left in &left_out_of(report, &found_before) {
                log.say(&left.said);
            }
            for unreadable in report.unreadable.iter().take(200) {
                log.say(&format!(
                    "Could not read {}: {}",
                    friendly_path(&unreadable.path),
                    unreadable.error
                ));
            }
        }
        Err(e) => log.say(&format!("Looking through {} failed: {e:#}", source.display())),
    }
    ScanEvent {
        name,
        photos: latest.0,
        videos: latest.1,
        others: latest.3,
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
            |r| r.unreadable.iter().take(5).map(|u| friendly_path(&u.path)).collect(),
        ),
        left_out: result
            .as_ref()
            .map_or_else(|_| Vec::new(), |r| left_out_of(r, &found_before)),
        // What could not be looked at without downloading it is said on
        // the screen too, so no photo is left out silently.
        online_only: result.as_ref().map_or(0, |r| r.online_only.len()),
        online_only_said: result.as_ref().ok().and_then(online_only_said),
        problem: result.err().map(|e| format!("{e:#}")),
        unplugged,
        ..ScanEvent::default()
    }
}

/// Looks through a bucket source by listing it, downloading nothing, and
/// gives the last event as a search of a folder would.
fn look_through_bucket(
    ctx: &Context,
    job: &Arc<Job>,
    home: &Path,
    source: &Path,
    kinds: &Kinds,
    skip: &keevoro_core::SkipRules,
) -> ScanEvent {
    let started = Instant::now();
    let mut name = name_of(source);
    let mut latest = keevoro_scan::ScanProgress::default();
    let result = (|| -> anyhow::Result<keevoro_scan::ScanReport> {
        let mut catalog = Catalog::open_in_library(home)?;
        let found = catalog.sources()?.into_iter().find(|s| s.root == source);
        let Some((place, kept_name)) =
            found.and_then(|s| Some((keevoro_engine::bucket::place_of(s.bucket.as_deref())?, s.name)))
        else {
            anyhow::bail!("That source isn't part of this backup.");
        };
        name = kept_name;
        ctx.log.say(&format!("Looking through {}", place.describe()));
        job.note_line(&format!("Looking through {name}"));
        let world = keevoro_engine::connect::World {
            secrets: &*ctx.secrets,
            environment: &*ctx.environment,
        };
        let (keys, from) = keevoro_engine::bucket::keys_for(&place, &world)?;
        if from == keevoro_engine::connect::KeysFrom::Environment {
            ctx.log.say(&format!(
                "Using the storage keys set in {} and {} for {}, as none are saved in this computer's keychain for it.",
                keevoro_engine::send::ACCESS_KEY_VAR,
                keevoro_engine::send::SECRET_KEY_VAR,
                place.describe()
            ));
        }
        let store = keevoro_engine::bucket::open(&place, &keys)?;
        let mut every = Every::after(ctx.jobs.progress_every());
        keevoro_engine::bucket::look_through(
            &mut catalog,
            &name,
            &place,
            &store,
            kinds,
            skip,
            &|| job.stopped(),
            &mut |p| {
                latest = p.clone();
                let event = ScanEvent {
                    name: name.clone(),
                    photos: p.photos,
                    videos: p.videos,
                    others: p.others,
                    files: p.files_seen,
                    folder: place.describe(),
                    ..ScanEvent::default()
                };
                ctx.jobs.heard(job, Said::Scan(event), every.due());
            },
        )
    })();
    match &result {
        Ok(report) if report.stopped => ctx.log.say(&format!("Stopped looking through {name}. Nothing from it was kept.")),
        Ok(report) => ctx.log.say(&format!(
            "Looked through {name} in {} seconds by its listing, downloading nothing: {} files Keevoro looks for, {} others",
            started.elapsed().as_secs(),
            report.items.len(),
            report.other_files
        )),
        Err(e) => ctx.log.say(&format!("Looking through {name} failed: {e:#}")),
    }
    ScanEvent {
        name,
        photos: latest.photos,
        videos: latest.videos,
        others: latest.others,
        files: latest.files_seen,
        stopped: result.as_ref().is_ok_and(|r| r.stopped),
        finished: true,
        left_out: result.as_ref().map_or_else(|_| Vec::new(), |r| left_out_of(r, &[])),
        online_only: result.as_ref().map_or(0, |r| r.online_only.len()),
        online_only_said: result.as_ref().ok().and_then(online_only_said),
        problem: result.err().map(|e| format!("{e:#}")),
        ..ScanEvent::default()
    }
}

/// What a look through said of the files it found only in online storage.
fn online_only_said(report: &keevoro_scan::ScanReport) -> Option<String> {
    let bytes =
        (report.online_only_bytes.len() == report.online_only.len()).then(|| report.online_only_bytes.iter().sum());
    keevoro_engine::summary::online_only(report.online_only.len(), bytes, &report.root)
}

/// The folders Keevoro made that a look through left out, said.
fn left_out_of(report: &keevoro_scan::ScanReport, found_before: &[PathBuf]) -> Vec<super::own::LeftOut> {
    let skipped: Vec<(PathBuf, keevoro_core::SkipReason)> =
        report.skipped.iter().map(|s| (s.path.clone(), s.reason)).collect();
    super::own::left_out(&skipped, found_before)
}
