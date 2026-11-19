//! A backup's status: what the program holding its lock is doing, for any
//! other program to read without asking it anything.
//!
//! `status.json` is written by the holder a few times a second while
//! something changes, and every [`super::lock::BEAT`] regardless, which is
//! its heartbeat. It holds the same `jobs.status` a program gives about its
//! own jobs, how many times each job has spoken (so a follower can tell
//! when there is something new), and the last words of the jobs that have
//! ended, so a program waiting for one can learn how it ended. It is
//! written whole each time, so it is never read half-done, and it stays
//! small: only this backup's jobs, and only the last few that ended.

use crate::jobs::{JobId, JobStatus, Said};
use serde::{Deserialize, Serialize};

/// What `status.json` says.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Status {
    /// The token of the lock this was written under. A status whose token
    /// is not the lock's is left over from an earlier hold.
    pub token: String,
    /// The heartbeat: when it was written, in milliseconds since 1970.
    pub beat: i64,
    /// True while the holder is paused.
    pub paused: bool,
    pub running: Vec<Running>,
    /// The last few jobs that ended, oldest first.
    pub ended: Vec<Ended>,
}

/// One running job.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Running {
    #[serde(flatten)]
    pub status: JobStatus,
    /// How many times it has said something to its listeners.
    pub told: u64,
}

/// One job that has ended, and its last word.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct Ended {
    pub job: JobId,
    pub last: Option<Said>,
    /// How it stood when it ended. Left out by a program from before this
    /// was shared.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<JobStatus>,
}

/// How many ended jobs a status remembers.
pub(crate) const ENDED_KEPT: usize = 8;

impl Status {
    pub(crate) fn to_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap_or_default()
    }

    pub(crate) fn from_bytes(bytes: &[u8]) -> Option<Self> {
        serde_json::from_slice(bytes).ok()
    }
}
