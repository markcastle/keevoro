//! Making previews in the background while a backup sends the originals.
//!
//! A backup used to send every original, then make every preview, then
//! send them: on a day-long backup the previews came as a second, unlooked
//! for wait at the end. Now [`Previews`] makes them on a thread of its own
//! from the moment every file has its place, so that when the originals
//! are sent the previews are ready, or nearly.
//!
//! The originals still come first, and the person's own use of their
//! computer before both:
//!
//! - The previews are made into the backup's own folder, as always, and
//!   sent only after the originals. Nothing here touches the upload.
//! - The threads ask the operating system for low priority
//!   (`keevoro_frame::quiet`), which on Windows and macOS lowers their disk
//!   reads too.
//! - How many previews are made at once is decided afresh every couple of
//!   seconds by `keevoro_core::background::preview_workers`: none on
//!   battery or while other programs keep the processor busy, one while the
//!   person is using the computer, and otherwise fewer than the foreground
//!   would use, never more than one per source.
//! - Pause and stop reach it as they reach everything else, between one
//!   preview and the next. Whatever was made is kept, so a stop or a crash
//!   loses nothing, and the rest is made next time.
//! - A file whose whole fingerprint is not known yet (a quick search only
//!   sampled it) is left until its original has been sent, since that is
//!   when its fingerprint is learnt and its previews are named by it.

use anyhow::Result;
use keevoro_catalog::{Catalog, RenditionState, VideoState};
use keevoro_core::background::{preview_workers, Conditions};
use keevoro_core::estimate::{Kind, Tracker};
use keevoro_core::MediaKind;
use keevoro_media::VideoTools;
use keevoro_scan::FoundItem;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// How often the computer is asked how it is.
const LOOK_EVERY: Duration = Duration::from_secs(2);

/// What the computer says about itself, read a little at a time.
pub struct Computer {
    system: sysinfo::System,
    me: Option<sysinfo::Pid>,
    cores: usize,
    /// Other programs' share of the processor, the last few readings.
    others: std::collections::VecDeque<f32>,
    battery: Option<bool>,
    battery_read: Option<Instant>,
}

impl Computer {
    pub fn new() -> Self {
        let mut system = sysinfo::System::new();
        system.refresh_cpu_usage();
        Self {
            system,
            me: sysinfo::get_current_pid().ok(),
            cores: std::thread::available_parallelism().map_or(1, std::num::NonZero::get),
            others: std::collections::VecDeque::new(),
            battery: None,
            battery_read: None,
        }
    }

    /// The conditions now. Processor use is averaged over the last five
    /// readings (about ten seconds), so a moment's spike changes nothing.
    pub fn conditions(&mut self, sending_originals: bool, sources: usize) -> Conditions {
        self.system.refresh_cpu_usage();
        let mut mine = 0.0;
        if let Some(me) = self.me {
            self.system.refresh_processes_specifics(
                sysinfo::ProcessesToUpdate::Some(&[me]),
                false,
                sysinfo::ProcessRefreshKind::nothing().with_cpu(),
            );
            if let Some(process) = self.system.process(me) {
                mine = process.cpu_usage() / self.cores as f32;
            }
        }
        let others = (self.system.global_cpu_usage() - mine).clamp(0.0, 100.0);
        self.others.push_back(others);
        while self.others.len() > 5 {
            self.others.pop_front();
        }
        // Whether it is plugged in changes rarely; asked twice a minute.
        if self
            .battery_read
            .is_none_or(|at| at.elapsed() >= Duration::from_secs(30))
        {
            self.battery = keevoro_frame::quiet::on_battery();
            self.battery_read = Some(Instant::now());
        }
        Conditions {
            sending_originals,
            on_battery: self.battery,
            idle_seconds: keevoro_frame::quiet::idle_seconds(),
            others_cpu: Some(self.others.iter().sum::<f32>() / self.others.len().max(1) as f32),
            foreground: keevoro_collect::foreground_workers(),
            sources,
        }
    }
}

impl Default for Computer {
    fn default() -> Self {
        Self::new()
    }
}

/// What the background maker shares with the backup that started it.
struct Shared {
    /// True while the originals are being sent; previews then go at the
    /// background's pace, and afterwards at the foreground's.
    sending_originals: AtomicBool,
    /// Set when the backup is over, however it ended.
    quit: AtomicBool,
    /// How many previews may be made at once, as last decided.
    allowed: AtomicU64,
    /// Done and to do, for saying so while the sender waits.
    done: AtomicU64,
    total: AtomicU64,
    /// Previews and video previews made, for the backup's history.
    photos_made: AtomicU64,
    videos_made: AtomicU64,
}

/// Where the background learns the conditions: given whether the
/// originals are still being sent and how many sources there are.
pub type Look = Box<dyn FnMut(bool, usize) -> Conditions + Send>;

/// Previews being made in the background for one backup.
pub struct Previews {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<Result<()>>>,
}

impl Previews {
    /// Starts making previews for everything being kept that has its
    /// place. `stop` is the backup's own stop switch.
    pub fn start(home: &Path, video_tools: Option<VideoTools>, stop: crate::send::Stop, tracker: Tracker) -> Self {
        let mut computer = None::<Computer>;
        Self::start_looking(
            home,
            video_tools,
            stop,
            tracker,
            // Made on the background's own thread, the first time it looks.
            Box::new(move |sending, sources| computer.get_or_insert_with(Computer::new).conditions(sending, sources)),
        )
    }

    /// As [`Previews::start`], learning the conditions from `look`.
    pub fn start_looking(
        home: &Path,
        video_tools: Option<VideoTools>,
        stop: crate::send::Stop,
        tracker: Tracker,
        look: Look,
    ) -> Self {
        let shared = Arc::new(Shared {
            sending_originals: AtomicBool::new(true),
            quit: AtomicBool::new(false),
            allowed: AtomicU64::new(1),
            done: AtomicU64::new(0),
            total: AtomicU64::new(0),
            photos_made: AtomicU64::new(0),
            videos_made: AtomicU64::new(0),
        });
        let home = home.to_path_buf();
        let thread = {
            let shared = shared.clone();
            std::thread::Builder::new()
                .name("keevoro previews".into())
                .spawn(move || make(&home, video_tools.as_ref(), &stop, &tracker, &shared, look))
                .ok()
        };
        Self { shared, thread }
    }

    /// The originals have all been sent to the destination in hand.
    /// Previews are the backup's work now, and go at the foreground's pace.
    pub fn originals_sent(&self) {
        self.shared.sending_originals.store(false, Ordering::SeqCst);
    }

    /// How many previews of photos, and of videos, have been made so far.
    pub fn made(&self) -> (u64, u64) {
        (
            self.shared.photos_made.load(Ordering::SeqCst),
            self.shared.videos_made.load(Ordering::SeqCst),
        )
    }

    /// Waits for every preview to be made, saying how far it has got.
    pub fn wait(&mut self, on_line: &mut dyn FnMut(&str)) -> Result<()> {
        self.originals_sent();
        let Some(thread) = self.thread.take() else {
            return Ok(());
        };
        while !thread.is_finished() {
            let (done, total) = (
                self.shared.done.load(Ordering::SeqCst),
                self.shared.total.load(Ordering::SeqCst),
            );
            if total > 0 {
                on_line(&format!(
                    "Your files are backed up. Finishing the previews: {} of {}",
                    crate::summary::thousands(done as usize),
                    crate::summary::thousands(total as usize)
                ));
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        thread
            .join()
            .unwrap_or_else(|_| Err(anyhow::anyhow!("Making previews stopped unexpectedly.")))
    }
}

impl Drop for Previews {
    /// However the backup ended, the background stops after the preview in
    /// hand, keeping what it made.
    fn drop(&mut self) {
        self.shared.quit.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Everything being kept, once each, ordered so that one source follows
/// another: with as many workers as sources, each tends to read its own.
fn to_preview(catalog: &Catalog, reports: &[keevoro_scan::ScanReport], whole_only: bool) -> Result<Vec<FoundItem>> {
    let (unique, _) = crate::summary::unique_items(reports);
    let root_of = |item: &FoundItem| reports.iter().position(|r| item.path.starts_with(&r.root)).unwrap_or(0);
    let mut by_source: Vec<Vec<FoundItem>> = vec![Vec::new(); reports.len().max(1)];
    for item in unique {
        if (whole_only && !item.hashed) || !crate::wanted(catalog, item)? {
            continue;
        }
        let at = root_of(item).min(by_source.len() - 1);
        by_source[at].push(item.clone());
    }
    let mut out = Vec::new();
    let longest = by_source.iter().map(Vec::len).max().unwrap_or(0);
    for n in 0..longest {
        out.extend(by_source.iter().filter_map(|s| s.get(n).cloned()));
    }
    Ok(out)
}

/// How many photos and videos being kept have no previews yet, places or
/// no places: what the background will have to do, for the plan.
pub fn still_to_make(catalog: &Catalog) -> Result<(u64, u64)> {
    let reports = catalog.load_reports()?;
    let (unique, _) = crate::summary::unique_items(&reports);
    let (mut photos, mut videos) = (0, 0);
    for item in unique {
        let Some(hash) = item.blake3.as_deref() else { continue };
        if !crate::wanted(catalog, item)? {
            continue;
        }
        match item.kind {
            MediaKind::Photo => {
                if !matches!(
                    catalog.rendition(hash)?,
                    Some(RenditionState::Made { .. } | RenditionState::NoPreview { .. })
                ) {
                    photos += 1;
                }
            }
            MediaKind::Video => {
                if !matches!(
                    catalog.video(hash)?,
                    Some(VideoState::Known { poster: true, .. } | VideoState::Unreadable { .. })
                ) {
                    videos += 1;
                }
            }
        }
    }
    Ok((photos, videos))
}

/// How many photos and videos being kept have no preview because Keevoro
/// could not make one, in this backup or an earlier one.
pub fn without_previews(catalog: &Catalog) -> Result<(u64, u64)> {
    let reports = catalog.load_reports()?;
    let (unique, _) = crate::summary::unique_items(&reports);
    let (mut photos, mut videos) = (0, 0);
    for item in unique {
        let Some(hash) = item.blake3.as_deref() else { continue };
        if !crate::wanted(catalog, item)? {
            continue;
        }
        match item.kind {
            MediaKind::Photo => {
                if matches!(catalog.rendition(hash)?, Some(RenditionState::NoPreview { .. })) {
                    photos += 1;
                }
            }
            MediaKind::Video => {
                if matches!(catalog.video(hash)?, Some(VideoState::Unreadable { .. })) {
                    videos += 1;
                }
            }
        }
    }
    Ok((photos, videos))
}

/// The background's work: a pass while the originals go, over the files
/// whose fingerprints are known, then, once they are sent, a pass over
/// everything that is left.
fn make(
    home: &Path,
    video_tools: Option<&VideoTools>,
    stop: &crate::send::Stop,
    tracker: &Tracker,
    shared: &Shared,
    look: Look,
) -> Result<()> {
    keevoro_frame::quiet::lower_this_threads_priority();
    let catalog = Catalog::open_in_library(home)?;
    let keep_going = || !stop.load(Ordering::SeqCst) && !shared.quit.load(Ordering::SeqCst);
    let look = Mutex::new(look);
    let looked = Mutex::new(None::<Instant>);
    let sources = catalog.load_reports()?.len();
    // Decided afresh every couple of seconds, from whichever worker asks.
    let allowed = || -> usize {
        let mut last = looked.lock().unwrap_or_else(PoisonError::into_inner);
        if last.is_none_or(|at| at.elapsed() >= LOOK_EVERY) {
            *last = Some(Instant::now());
            let conditions = (look.lock().unwrap_or_else(PoisonError::into_inner))(
                shared.sending_originals.load(Ordering::SeqCst),
                sources,
            );
            shared
                .allowed
                .store(preview_workers(&conditions) as u64, Ordering::SeqCst);
        }
        usize::try_from(shared.allowed.load(Ordering::SeqCst)).unwrap_or(1)
    };
    let pace = keevoro_collect::Pace {
        allowed: &allowed,
        on_worker: &|| {
            keevoro_frame::quiet::lower_this_threads_priority();
        },
    };

    let videos_done = AtomicU64::new(0);
    let pass = |whole_only: bool| -> Result<()> {
        let reports = catalog.load_reports()?;
        let items = to_preview(&catalog, &reports, whole_only)?;
        let refs: Vec<&FoundItem> = items.iter().collect();
        let photos = keevoro_collect::previews_to_make(home, &catalog, &refs)? as u64;
        let before = shared.done.load(Ordering::SeqCst);
        shared.total.store(before + photos, Ordering::SeqCst);
        tracker.set_work(Kind::MakingPreviews, None, before + photos);
        tracker.progress(Kind::MakingPreviews, None, before);
        let report = keevoro_collect::make_previews_paced(
            home,
            &catalog,
            &refs,
            &mut |done, _| {
                let done = before + done as u64;
                shared.done.store(done, Ordering::SeqCst);
                tracker.progress(Kind::MakingPreviews, None, done);
            },
            &keep_going,
            pace,
        )?;
        shared.photos_made.fetch_add(report.made as u64, Ordering::SeqCst);
        if keep_going()
            && refs.iter().any(|i| i.kind == MediaKind::Video)
            && keevoro_media::video::can_make_posters(video_tools)
        {
            let videos = keevoro_collect::video_previews_to_make(home, &catalog, &refs)? as u64;
            let before = videos_done.load(Ordering::SeqCst);
            tracker.set_work(Kind::MakingVideoPreviews, None, before + videos);
            if videos > 0 {
                tracker.progress(Kind::MakingVideoPreviews, None, before);
            }
            let report = keevoro_collect::make_video_previews_paced(
                home,
                &catalog,
                &refs,
                video_tools,
                &mut |done, _| {
                    let done = before + done as u64;
                    videos_done.store(done, Ordering::SeqCst);
                    tracker.progress(Kind::MakingVideoPreviews, None, done);
                },
                &keep_going,
                pace,
            )?;
            shared.videos_made.fetch_add(report.posters as u64, Ordering::SeqCst);
        }
        Ok(())
    };

    pass(true)?;
    // The rest wait for the originals to be sent, which is when their
    // fingerprints are learnt.
    while keep_going() && shared.sending_originals.load(Ordering::SeqCst) {
        std::thread::sleep(Duration::from_millis(200));
    }
    if keep_going() {
        pass(false)?;
    }
    if keep_going() {
        tracker.finish(Kind::MakingPreviews, None);
        tracker.finish(Kind::MakingVideoPreviews, None);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use keevoro_core::estimate::Remembered;

    /// A source of three photos, searched and placed, nothing sent.
    fn placed() -> (tempfile::TempDir, std::path::PathBuf, Catalog) {
        let dir = tempfile::tempdir().unwrap();
        let card = dir.path().join("Card");
        std::fs::create_dir_all(card.join("Flights")).unwrap();
        for (n, shade) in [10u8, 90, 170].iter().enumerate() {
            std::fs::write(
                card.join(format!("Flights/IMG_2024060{n}_100000.jpg")),
                crate::discover::tests::real_jpeg(*shade),
            )
            .unwrap();
        }
        let home = dir.path().join("home");
        let mut catalog = Catalog::open_in_library(&home).unwrap();
        crate::scan_into(&mut catalog, &card, Some("Card"), &mut |_| {}).unwrap();
        crate::check_and_place(&home, &catalog, &|| true, &mut |_| {}).unwrap();
        (dir, home, catalog)
    }

    fn conditions(sending_originals: bool, on_battery: bool) -> Conditions {
        Conditions {
            sending_originals,
            on_battery: Some(on_battery),
            idle_seconds: Some(3600),
            others_cpu: Some(0.0),
            foreground: 2,
            sources: 1,
        }
    }

    fn made(catalog: &Catalog) -> usize {
        let reports = catalog.load_reports().unwrap();
        reports[0]
            .items
            .iter()
            .filter(|i| {
                matches!(
                    catalog.rendition(i.blake3.as_deref().unwrap()).unwrap(),
                    Some(RenditionState::Made { .. })
                )
            })
            .count()
    }

    #[test]
    fn previews_are_made_while_the_originals_are_still_being_sent() {
        let (_dir, home, catalog) = placed();
        assert_eq!(still_to_make(&catalog).unwrap(), (3, 0));
        let tracker = Tracker::new(Remembered::default());
        tracker.plan(vec![keevoro_core::estimate::Planned::new(
            Kind::MakingPreviews,
            None,
            3,
        )]);
        let mut previews = Previews::start_looking(
            &home,
            None,
            crate::send::Stop::default(),
            tracker.clone(),
            Box::new(|sending, _| conditions(sending, false)),
        );
        // Nothing has said the originals are sent, and yet they are made.
        let began = Instant::now();
        while made(&catalog) < 3 {
            assert!(began.elapsed() < Duration::from_secs(60), "previews were not made");
            std::thread::sleep(Duration::from_millis(50));
        }
        let estimate = tracker.estimate();
        assert_eq!(estimate.stages[0].done, 3);
        previews.wait(&mut |_| {}).unwrap();
        assert_eq!(still_to_make(&catalog).unwrap(), (0, 0));
        assert_eq!(tracker.estimate().stages[0].state, keevoro_core::estimate::State::Done);
    }

    #[test]
    fn on_battery_the_background_waits_until_the_originals_are_sent() {
        let (_dir, home, catalog) = placed();
        let mut previews = Previews::start_looking(
            &home,
            None,
            crate::send::Stop::default(),
            Tracker::new(Remembered::default()),
            Box::new(|sending, _| conditions(sending, true)),
        );
        std::thread::sleep(Duration::from_millis(800));
        assert_eq!(made(&catalog), 0, "nothing while on battery and sending");
        // The originals are sent: previews are the backup's work now.
        let mut said = Vec::new();
        previews.wait(&mut |line| said.push(line.to_string())).unwrap();
        assert_eq!(made(&catalog), 3);
        assert!(said
            .iter()
            .all(|l| l.starts_with("Your files are backed up. Finishing the previews")));
    }

    #[test]
    fn stopping_keeps_what_was_made_and_makes_nothing_more() {
        let (_dir, home, catalog) = placed();
        let stop = crate::send::Stop::default();
        stop.store(true, Ordering::SeqCst);
        let mut previews = Previews::start_looking(
            &home,
            None,
            stop,
            Tracker::new(Remembered::default()),
            Box::new(|sending, _| conditions(sending, false)),
        );
        previews.wait(&mut |_| {}).unwrap();
        assert_eq!(made(&catalog), 0);
        // And dropping one part-way lets it finish the preview in hand only.
        drop(Previews::start(
            &home,
            None,
            crate::send::Stop::default(),
            Tracker::new(Remembered::default()),
        ));
    }

    #[test]
    fn the_computer_can_always_be_asked_how_it_is() {
        let mut computer = Computer::new();
        let c = computer.conditions(true, 2);
        assert!(c.others_cpu.is_some_and(|cpu| (0.0..=100.0).contains(&cpu)));
        assert_eq!((c.sending_originals, c.sources), (true, 2));
        assert!(c.foreground >= 1);
    }
}
