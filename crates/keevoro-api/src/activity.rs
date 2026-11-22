//! What Keevoro is doing to this computer, said plainly.
//!
//! A program that makes a computer's fan spin with nothing on screen to say
//! why is a bad guest. This keeps a short record of how hard Keevoro itself
//! is working (its own share of the processor and the disk, not the whole
//! computer's) and names each job in hand with the reason for it, so the
//! person can see what is going on and pause it if they need the computer.
//!
//! This is `keevoro.activity`. The measurements are of the program that
//! asks: the app measures itself once a second for as long as it runs, so
//! a one-off question from the command line has what is being done now but
//! no history to speak of.

use keevoro_core::doing::Work;
use serde::Serialize;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, PoisonError};
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};

/// How many measurements are kept: two minutes at one a second.
const KEPT: usize = 120;

/// One measurement of Keevoro's own use of the computer.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Sample {
    /// Share of the whole computer's processor, 0 to 100.
    pub cpu: f32,
    /// Megabytes read from and written to disk per second.
    pub disk: f32,
    /// Megabytes of your files that safely arrived where they were sent,
    /// per second: the speed of a send, wherever it is going.
    pub sent: f32,
    /// The part of `sent` that went over the network, to cloud storage.
    pub network: f32,
    /// Megabytes of memory Keevoro is holding at that moment.
    pub memory: f32,
}

/// Turns the raw figures for one second into a [`Sample`]. The system
/// reports processor use as a percentage of one core, so a program busy on
/// four of twenty cores reads 400; divided by the cores it is 20% of the
/// computer, which is what a person means.
pub fn sample(of_one_core: f32, cores: usize, bytes: u64, sent: u64, network: u64, seconds: f32) -> Sample {
    let cores = cores.max(1) as f32;
    let rate = |bytes: u64| {
        if seconds > 0.0 {
            bytes as f32 / (1024.0 * 1024.0) / seconds
        } else {
            0.0
        }
    };
    Sample {
        cpu: (of_one_core / cores).clamp(0.0, 100.0),
        disk: rate(bytes),
        sent: rate(sent),
        network: rate(network),
        // Filled in when the sample is kept, alongside the reading it came with.
        memory: 0.0,
    }
}

/// The last two minutes of measurements, and how much memory is in use.
#[derive(Debug, Default)]
pub struct History {
    pub(crate) samples: VecDeque<Sample>,
    pub(crate) memory_bytes: u64,
}

impl History {
    pub fn push(&mut self, mut sample: Sample, memory_bytes: u64) {
        sample.memory = memory_bytes as f32 / (1024.0 * 1024.0);
        if self.samples.len() == KEPT {
            self.samples.pop_front();
        }
        self.samples.push_back(sample);
        self.memory_bytes = memory_bytes;
    }

    pub fn samples(&self) -> Vec<Sample> {
        self.samples.iter().copied().collect()
    }

    pub fn memory_bytes(&self) -> u64 {
        self.memory_bytes
    }
}

/// One thing Keevoro is doing, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Job {
    /// What it is doing, as a heading: two or three plain words.
    pub what: &'static str,
    /// Why, and what it costs the computer, in a sentence or two.
    pub why: &'static str,
}

/// How one kind of work is said to a person.
fn said(work: Work) -> Job {
    match work {
        Work::BackingUp => Job {
            what: "Backing up your files",
            why: "Reading each photo and video you are keeping and copying it to where you chose. \
                  This uses the disk steadily, and the network if you are backing up to cloud storage.",
        },
        Work::CheckingForDuplicates => Job {
            what: "Checking for duplicates",
            why: "Reading every file from start to finish, so that true copies can be told from photos \
                  that merely look alike. This is the heaviest thing Keevoro does to the disk. It is \
                  done once per file and remembered.",
        },
        Work::MakingVideoPreviews => Job {
            what: "Making video previews",
            why: "Taking a still picture from each video, so a gallery has something to show before \
                  the video plays. Opening a video is heavy on the processor for a moment each time.",
        },
        Work::MakingPreviews => Job {
            what: "Making previews",
            why: "Making a small and a screen-sized version of each photo, so the backup is quick to \
                  look through. This is the heaviest thing Keevoro does to the processor, and it uses \
                  half the cores at most. During a backup they are made beside it at low priority, one \
                  at a time while you are using the computer, and not at all while it is on battery or \
                  busy. Your originals are not changed.",
        },
        Work::LookingThroughASource => Job {
            what: "Looking through a source",
            why: "Walking the folders and reading a little of each file to find the photos and videos. \
                  Light on the processor, steady on the disk.",
        },
        Work::Surveying => Job {
            what: "Finding where your photos are",
            why: "Reading the list of what is in each folder, names and sizes only, to find the folders \
                  that hold photos and videos. No file is opened, so it is quick and light.",
        },
        Work::MakingThumbnails => Job {
            what: "Making thumbnails",
            why: "Making the small pictures for the screen you are looking at. Each is made once and \
                  kept, so the same screen is quick the next time.",
        },
    }
}

/// The jobs in hand, heaviest first, each in plain words.
pub fn jobs(now: &[Work]) -> Vec<Job> {
    now.iter().map(|w| said(*w)).collect()
}

/// What Keevoro is doing, in one line, for a glance: "Making previews", or
/// "Backing up your files and making thumbnails" when there are two.
/// `None` when it is idle.
pub fn in_a_line(now: &[Work]) -> Option<String> {
    let whats: Vec<&str> = now.iter().map(|w| said(*w).what).collect();
    let (first, rest) = whats.split_first()?;
    let lower = |what: &str| {
        let mut letters = what.chars();
        letters
            .next()
            .map(|c| c.to_lowercase().chain(letters).collect::<String>())
            .unwrap_or_default()
    };
    Some(match rest {
        [] => (*first).to_string(),
        [second] => format!("{first} and {}", lower(second)),
        [second, others @ ..] => format!(
            "{first}, {} and {} more",
            lower(second),
            if others.len() == 1 {
                "one".to_string()
            } else {
                others.len().to_string()
            }
        ),
    })
}

/// How hard this program has been working, kept for as long as it runs.
static HISTORY: Mutex<History> = Mutex::new(History {
    samples: VecDeque::new(),
    memory_bytes: 0,
});

/// True once something is measuring, so asking again starts nothing more.
static WATCHING: AtomicBool = AtomicBool::new(false);

/// The measurements so far, oldest first, and the memory in use now.
pub fn recent() -> (Vec<Sample>, u64) {
    let history = HISTORY.lock().unwrap_or_else(PoisonError::into_inner);
    (history.samples(), history.memory_bytes())
}

/// What Keevoro is doing now, why, and how hard it has been working.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityView {
    pub paused: bool,
    /// When a pause for a while ends by itself, in words: "13:00",
    /// "tomorrow at 07:00". Absent for a pause until someone carries on.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub paused_until: Option<String>,
    /// What it is doing in one line, or `None` when idle.
    pub doing: Option<String>,
    /// The step a running backup is on, in its own words, such as
    /// "Working out where each file goes: 2,500 of 8,630". `None` when no
    /// backup is running.
    pub backup_step: Option<String>,
    pub jobs: Vec<Job>,
    /// One a second, oldest first, for the last two minutes at most.
    pub samples: Vec<Sample>,
    pub memory_bytes: u64,
    /// How many processor cores this computer has.
    pub cores: usize,
    /// The background check for duplicates, when one is running here.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub check: Option<CheckNow>,
}

/// The background check for duplicates running now: how far it has got,
/// whether it is paused on its own, and that in a line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckNow {
    /// Its job, to pause or stop it alone.
    pub job: crate::jobs::JobId,
    pub done: usize,
    pub total: usize,
    pub paused_alone: bool,
    pub said: String,
}

/// What is being done now. `backup_step` is what a running backup last
/// said it was doing, if one is running.
pub fn now(backup_step: Option<&str>) -> ActivityView {
    // Only the step, not the name of the file that follows it after a tab.
    let backup_step = backup_step
        .map(|line| line.split('\t').next().unwrap_or_default().to_string())
        .filter(|step| !step.is_empty());
    let (samples, memory_bytes) = recent();
    let mut now = keevoro_core::doing::now();
    // A backup is a backup from the moment it is asked for, including the
    // time it spends getting ready before the first file moves.
    if backup_step.is_some() && !now.contains(&Work::BackingUp) {
        now.insert(0, Work::BackingUp);
    }
    ActivityView {
        paused: keevoro_core::pause::is_paused(),
        paused_until: None,
        doing: in_a_line(&now),
        backup_step,
        jobs: jobs(&now),
        samples,
        memory_bytes,
        cores: std::thread::available_parallelism().map_or(1, std::num::NonZero::get),
        check: None,
    }
}

/// True while nobody can see the measurements: the app's window is closed
/// and Keevoro waits by the clock. Measuring then stops, so an idle
/// Keevoro wakes the computer no more than its scheduler's once a minute.
static RESTING: Mutex<bool> = Mutex::new(false);
static WOKEN: std::sync::Condvar = std::sync::Condvar::new();

/// Stops measuring while `resting`, and starts again when not. The app
/// rests it while its window is closed.
pub fn rest(resting: bool) {
    *RESTING.lock().unwrap_or_else(PoisonError::into_inner) = resting;
    WOKEN.notify_all();
}

/// Waits here while measuring rests. True if it waited.
fn wait_while_resting() -> bool {
    let mut resting = RESTING.lock().unwrap_or_else(PoisonError::into_inner);
    let rested = *resting;
    while *resting {
        resting = WOKEN.wait(resting).unwrap_or_else(PoisonError::into_inner);
    }
    rested
}

/// Measures this program once a second for as long as it runs, except
/// while it rests (see [`rest`]). Asking more than once is harmless: there
/// is only ever one measuring.
pub fn watch() {
    if WATCHING.swap(true, Ordering::SeqCst) {
        return;
    }
    let history = &HISTORY;
    std::thread::spawn(move || {
        let Ok(me) = sysinfo::get_current_pid() else { return };
        let cores = std::thread::available_parallelism().map_or(1, std::num::NonZero::get);
        let what = ProcessRefreshKind::nothing().with_cpu().with_disk_usage().with_memory();
        let mut system = System::new();
        let mut last = std::time::Instant::now();
        let mut counted = keevoro_core::meter::totals();
        loop {
            if wait_while_resting() {
                // What happened while resting was not watched: start afresh.
                system.refresh_processes_specifics(ProcessesToUpdate::Some(&[me]), false, what);
                last = std::time::Instant::now();
                counted = keevoro_core::meter::totals();
            }
            std::thread::sleep(std::time::Duration::from_secs(1));
            system.refresh_processes_specifics(ProcessesToUpdate::Some(&[me]), false, what);
            let seconds = last.elapsed().as_secs_f32();
            last = std::time::Instant::now();
            let Some(process) = system.process(me) else { continue };
            let disk = process.disk_usage();
            let now = keevoro_core::meter::totals();
            let measured = sample(
                process.cpu_usage(),
                cores,
                disk.read_bytes + disk.written_bytes,
                now.0.saturating_sub(counted.0),
                now.1.saturating_sub(counted.1),
                seconds,
            );
            counted = now;
            history
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(measured, process.memory());
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measuring_waits_while_it_rests_and_goes_on_when_woken() {
        rest(true);
        let passed = std::sync::Arc::new(AtomicBool::new(false));
        let waiting = {
            let passed = passed.clone();
            std::thread::spawn(move || {
                let waited = wait_while_resting();
                passed.store(true, Ordering::SeqCst);
                waited
            })
        };
        std::thread::sleep(std::time::Duration::from_millis(200));
        assert!(!passed.load(Ordering::SeqCst), "nothing is measured while resting");
        rest(false);
        assert!(waiting.join().unwrap(), "it said it had waited");
        assert!(!wait_while_resting(), "awake, it does not wait");
    }

    #[test]
    fn processor_use_is_a_share_of_the_whole_computer() {
        // Busy on four of twenty cores is a fifth of the computer.
        assert_eq!(sample(400.0, 20, 0, 0, 0, 1.0).cpu, 20.0);
        assert_eq!(sample(50.0, 1, 0, 0, 0, 1.0).cpu, 50.0);
        assert_eq!(sample(5000.0, 4, 0, 0, 0, 1.0).cpu, 100.0, "never more than all of it");
        assert_eq!(
            sample(10.0, 0, 0, 0, 0, 1.0).cpu,
            10.0,
            "a computer with no cores is one core"
        );
    }

    #[test]
    fn disk_use_is_megabytes_a_second() {
        assert_eq!(sample(0.0, 4, 92 * 1024 * 1024, 0, 0, 1.0).disk, 92.0);
        assert_eq!(sample(0.0, 4, 92 * 1024 * 1024, 0, 0, 2.0).disk, 46.0);
        assert_eq!(sample(0.0, 4, 1024, 0, 0, 0.0).disk, 0.0);
    }

    #[test]
    fn only_the_last_two_minutes_are_kept() {
        let mut history = History::default();
        for n in 0..200u16 {
            history.push(
                Sample {
                    cpu: f32::from(n),
                    disk: 0.0,
                    sent: 0.0,
                    network: 0.0,
                    memory: 0.0,
                },
                u64::from(n) * 1024 * 1024,
            );
        }
        let kept = history.samples();
        assert_eq!(kept.len(), KEPT);
        assert_eq!((kept[0].cpu, kept[KEPT - 1].cpu), (80.0, 199.0), "oldest first");
        assert_eq!(history.memory_bytes(), 199 * 1024 * 1024);
        assert_eq!(
            (kept[0].memory, kept[KEPT - 1].memory),
            (80.0, 199.0),
            "each moment keeps its own memory reading, in megabytes"
        );
    }

    #[test]
    fn what_is_sent_and_what_went_over_the_network_are_megabytes_a_second() {
        let s = sample(0.0, 4, 0, 30 * 1024 * 1024, 12 * 1024 * 1024, 2.0);
        assert_eq!((s.sent, s.network), (15.0, 6.0));
    }

    #[test]
    fn every_kind_of_work_says_what_and_why() {
        assert!(jobs(&[]).is_empty());
        let all = jobs(&keevoro_core::doing::ALL);
        let whats: Vec<&str> = all.iter().map(|j| j.what).collect();
        assert_eq!(
            whats,
            [
                "Backing up your files",
                "Checking for duplicates",
                "Making video previews",
                "Making previews",
                "Looking through a source",
                "Finding where your photos are",
                "Making thumbnails",
            ]
        );
        for job in all {
            assert!(
                job.what.split(' ').count() <= 5,
                "{} is short enough to read at a glance",
                job.what
            );
            assert!(job.why.len() > 60, "{} needs a real reason", job.what);
            assert!(!job.why.contains('!'));
        }
    }

    #[test]
    fn a_running_backup_is_named_by_its_step_alone() {
        let view = now(Some(
            "Working out where each file goes: 2,500 of 8,630\tD:\\DCIM\\a.jpg",
        ));
        assert_eq!(
            view.backup_step.as_deref(),
            Some("Working out where each file goes: 2,500 of 8,630")
        );
        assert_eq!(view.jobs[0].what, "Backing up your files");
        assert!(view.doing.unwrap().starts_with("Backing up your files"));
        assert!(view.cores >= 1);
        // What the app has always received.
        let json = serde_json::to_value(now(None)).unwrap();
        for field in [
            "paused",
            "doing",
            "backupStep",
            "jobs",
            "samples",
            "memoryBytes",
            "cores",
        ] {
            assert!(json.get(field).is_some(), "{field} is missing from {json}");
        }
        assert_eq!(json["backupStep"], serde_json::Value::Null);
        assert_eq!(now(Some("")).backup_step, None, "an empty step is no step");
    }

    #[test]
    fn measuring_is_started_once_however_often_it_is_asked() {
        watch();
        watch();
        assert!(WATCHING.load(Ordering::SeqCst));
        let (samples, _) = recent();
        assert!(samples.len() <= KEPT);
    }

    #[test]
    fn what_it_is_doing_fits_in_one_line() {
        assert_eq!(in_a_line(&[]), None);
        assert_eq!(in_a_line(&[Work::MakingPreviews]).unwrap(), "Making previews");
        assert_eq!(
            in_a_line(&[Work::BackingUp, Work::MakingThumbnails]).unwrap(),
            "Backing up your files and making thumbnails"
        );
        assert_eq!(
            in_a_line(&[Work::BackingUp, Work::CheckingForDuplicates, Work::MakingThumbnails]).unwrap(),
            "Backing up your files, checking for duplicates and one more"
        );
    }
}
