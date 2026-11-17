//! What Keevoro is doing to this computer, said plainly.
//!
//! A program that makes a computer's fan spin with nothing on screen to say
//! why is a bad guest. This keeps a short record of how hard Keevoro itself
//! is working (its own share of the processor and the disk, not the whole
//! computer's) and names each job in hand with the reason for it, so the
//! person can see what is going on and pause it if they need the computer.

use serde::Serialize;
use std::collections::VecDeque;
use std::sync::Mutex;
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
    }
}

/// How fast a send is going, in bytes a second, and how long is left.
///
/// The speed is the average of the last few seconds, because files arrive
/// in lumps: a whole photo at once, a large video sixteen megabytes at a
/// time. A single second's figure would jump about and mean little. With
/// nothing arriving there is no honest guess at the time left, so there is
/// none.
pub fn speed_and_time_left(samples: &[Sample], bytes_left: u64) -> (u64, Option<u64>) {
    const OVER: usize = 10;
    let recent = &samples[samples.len().saturating_sub(OVER)..];
    if recent.is_empty() {
        return (0, None);
    }
    let megabytes: f32 = recent.iter().map(|s| s.sent).sum::<f32>() / recent.len() as f32;
    let speed = (megabytes * 1024.0 * 1024.0) as u64;
    let left = (speed > 0 && bytes_left > 0).then(|| bytes_left.div_ceil(speed));
    (speed, left)
}

/// The last two minutes of measurements, and how much memory is in use.
#[derive(Debug, Default)]
pub struct History {
    pub(crate) samples: VecDeque<Sample>,
    pub(crate) memory_bytes: u64,
}

impl History {
    pub fn push(&mut self, sample: Sample, memory_bytes: u64) {
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
    /// What it is doing, as a heading.
    pub what: &'static str,
    /// Why, and what it costs the computer, in a sentence or two.
    pub why: &'static str,
}

/// The jobs in hand, most demanding first.
pub fn jobs(searching: bool, checking: bool, sending: bool) -> Vec<Job> {
    let mut list = Vec::new();
    if sending {
        list.push(Job {
            what: "Sending your files",
            why: "Reading each photo and video you are keeping and copying it to where you chose. \
                  This uses the disk steadily, and the network if you are sending to cloud storage.",
        });
    }
    if checking {
        list.push(Job {
            what: "Checking for duplicates",
            why: "Reading every file from start to finish, so that true copies can be told from photos \
                  that merely look alike. This is the heaviest thing Keevoro does to the disk. It is \
                  done once per file and remembered.",
        });
    }
    if searching {
        list.push(Job {
            what: "Looking through a source",
            why: "Walking the folders and reading a little of each file to find the photos and videos. \
                  Light on the processor, steady on the disk.",
        });
    }
    list
}

/// Measures this program once a second for as long as it runs.
pub fn watch(history: &'static Mutex<History>) {
    std::thread::spawn(move || {
        let Ok(me) = sysinfo::get_current_pid() else { return };
        let cores = std::thread::available_parallelism().map_or(1, std::num::NonZero::get);
        let what = ProcessRefreshKind::nothing().with_cpu().with_disk_usage().with_memory();
        let mut system = System::new();
        let mut last = std::time::Instant::now();
        let mut counted = keevoro_core::meter::totals();
        loop {
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
            if let Ok(mut history) = history.lock() {
                history.push(measured, process.memory());
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

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
                },
                u64::from(n),
            );
        }
        let kept = history.samples();
        assert_eq!(kept.len(), KEPT);
        assert_eq!((kept[0].cpu, kept[KEPT - 1].cpu), (80.0, 199.0), "oldest first");
        assert_eq!(history.memory_bytes(), 199);
    }

    #[test]
    fn what_is_sent_and_what_went_over_the_network_are_megabytes_a_second() {
        let s = sample(0.0, 4, 0, 30 * 1024 * 1024, 12 * 1024 * 1024, 2.0);
        assert_eq!((s.sent, s.network), (15.0, 6.0));
    }

    fn sending(megabytes: &[f32]) -> Vec<Sample> {
        megabytes
            .iter()
            .map(|&sent| Sample {
                cpu: 0.0,
                disk: 0.0,
                sent,
                network: 0.0,
            })
            .collect()
    }

    #[test]
    fn speed_is_steadied_over_the_last_few_seconds() {
        const MB: u64 = 1024 * 1024;
        // A 48 MB file landing every four seconds is 12 MB a second.
        let lumpy = sending(&[0.0, 0.0, 0.0, 48.0, 0.0, 0.0, 0.0, 48.0]);
        let (speed, left) = speed_and_time_left(&lumpy, 1200 * MB);
        assert_eq!(speed, 12 * MB);
        assert_eq!(left, Some(100), "1,200 MB at 12 MB a second");
        // Only the last ten seconds count, so an old burst is forgotten.
        let mut old_burst = sending(&[500.0]);
        old_burst.extend(sending(&[2.0; 10]));
        assert_eq!(speed_and_time_left(&old_burst, 0).0, 2 * MB);
    }

    #[test]
    fn with_nothing_arriving_no_time_left_is_guessed() {
        assert_eq!(speed_and_time_left(&sending(&[0.0, 0.0]), 1000), (0, None));
        assert_eq!(speed_and_time_left(&[], 1000), (0, None));
        // Nothing left to send needs no time.
        assert_eq!(speed_and_time_left(&sending(&[5.0]), 0).1, None);
    }

    #[test]
    fn each_job_says_what_and_why() {
        assert!(jobs(false, false, false).is_empty());
        let all = jobs(true, true, true);
        let whats: Vec<&str> = all.iter().map(|j| j.what).collect();
        assert_eq!(
            whats,
            [
                "Sending your files",
                "Checking for duplicates",
                "Looking through a source"
            ]
        );
        for job in all {
            assert!(job.why.len() > 60, "{} needs a real reason", job.what);
            assert!(!job.why.contains('!'));
        }
    }
}
