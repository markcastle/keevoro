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
}

/// Turns the raw figures for one second into a [`Sample`]. The system
/// reports processor use as a percentage of one core, so a program busy on
/// four of twenty cores reads 400; divided by the cores it is 20% of the
/// computer, which is what a person means.
pub fn sample(of_one_core: f32, cores: usize, bytes: u64, seconds: f32) -> Sample {
    let cores = cores.max(1) as f32;
    Sample {
        cpu: (of_one_core / cores).clamp(0.0, 100.0),
        disk: if seconds > 0.0 {
            bytes as f32 / (1024.0 * 1024.0) / seconds
        } else {
            0.0
        },
    }
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
        loop {
            std::thread::sleep(std::time::Duration::from_secs(1));
            system.refresh_processes_specifics(ProcessesToUpdate::Some(&[me]), false, what);
            let seconds = last.elapsed().as_secs_f32();
            last = std::time::Instant::now();
            let Some(process) = system.process(me) else { continue };
            let disk = process.disk_usage();
            let measured = sample(
                process.cpu_usage(),
                cores,
                disk.read_bytes + disk.written_bytes,
                seconds,
            );
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
        assert_eq!(sample(400.0, 20, 0, 1.0).cpu, 20.0);
        assert_eq!(sample(50.0, 1, 0, 1.0).cpu, 50.0);
        assert_eq!(sample(5000.0, 4, 0, 1.0).cpu, 100.0, "never more than all of it");
        assert_eq!(
            sample(10.0, 0, 0, 1.0).cpu,
            10.0,
            "a computer with no cores is one core"
        );
    }

    #[test]
    fn disk_use_is_megabytes_a_second() {
        assert_eq!(sample(0.0, 4, 92 * 1024 * 1024, 1.0).disk, 92.0);
        assert_eq!(sample(0.0, 4, 92 * 1024 * 1024, 2.0).disk, 46.0);
        assert_eq!(sample(0.0, 4, 1024, 0.0).disk, 0.0);
    }

    #[test]
    fn only_the_last_two_minutes_are_kept() {
        let mut history = History::default();
        for n in 0..200u16 {
            history.push(
                Sample {
                    cpu: f32::from(n),
                    disk: 0.0,
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
