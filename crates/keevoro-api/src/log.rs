//! A plain record of what Keevoro did and when.
//!
//! When something is slow or goes wrong, "what was it doing?" should have
//! an answer a person can read and send to someone. The log is an ordinary
//! text file, one line per event, with the time in front:
//!
//! ```text
//! 2026-11-16 14:02:11  Looking through K:\
//! 2026-11-16 14:02:19  Reading a large file (3.9 GB): K:\Video\wedding.mov
//! ```
//!
//! It records places, counts, timings and errors. It never records keys or
//! passwords. When it grows past a couple of megabytes the old half is kept
//! as `keevoro.previous.log` and a fresh file is started.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

const NAME: &str = "keevoro.log";
const PREVIOUS: &str = "keevoro.previous.log";
const FULL_AT: u64 = 2 * 1024 * 1024;

pub struct Log {
    dir: PathBuf,
    file: Mutex<Option<File>>,
}

impl Log {
    /// Opens (or starts) the log in `dir`. A log that cannot be opened is
    /// not a reason to stop the app: lines are then simply dropped.
    pub fn open(dir: &Path) -> Self {
        let _ = std::fs::create_dir_all(dir);
        let path = dir.join(NAME);
        if std::fs::metadata(&path).is_ok_and(|m| m.len() > FULL_AT) {
            let _ = std::fs::rename(&path, dir.join(PREVIOUS));
        }
        let file = OpenOptions::new().create(true).append(true).open(&path).ok();
        Self {
            dir: dir.to_path_buf(),
            file: Mutex::new(file),
        }
    }

    pub fn path(&self) -> PathBuf {
        self.dir.join(NAME)
    }

    /// Adds one line, with the time in front.
    pub fn say(&self, what: &str) {
        self.say_at(now(), what);
    }

    fn say_at(&self, seconds: u64, what: &str) {
        if let Ok(mut file) = self.file.lock() {
            if let Some(file) = file.as_mut() {
                // One event, one line, whatever the message holds.
                let what = what.replace(['\r', '\n'], " ");
                let _ = writeln!(file, "{}  {what}", stamp(seconds));
            }
        }
    }

    /// The last `lines` lines, oldest first, for showing in the app.
    pub fn tail(&self, lines: usize) -> String {
        let text = std::fs::read_to_string(self.path()).unwrap_or_default();
        let all: Vec<&str> = text.lines().collect();
        all[all.len().saturating_sub(lines)..].join("\n")
    }
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// Seconds since 1970 as `2026-11-16 14:02:11`, in UTC. Done by hand so the
/// app needs no date library for one line of text.
pub(crate) fn stamp(seconds: u64) -> String {
    let (days, rest) = (seconds / 86_400, seconds % 86_400);
    // Days since 1970 to a calendar date (Howard Hinnant's method).
    let z = days + 719_468;
    let era = z / 146_097;
    let day_of_era = z % 146_097;
    let year_of_era = (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    let year = year_of_era + era * 400 + u64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02}",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_are_written_as_a_person_would_read_them() {
        assert_eq!(stamp(0), "1970-01-01 00:00:00");
        assert_eq!(stamp(951_782_400), "2000-02-29 00:00:00");
        assert_eq!(stamp(1_794_751_331), "2026-11-15 14:02:11");
    }

    #[test]
    fn lines_are_kept_in_order_and_the_last_few_can_be_read_back() {
        let dir = tempfile::tempdir().unwrap();
        let log = Log::open(dir.path());
        log.say_at(0, "Keevoro started");
        log.say_at(61, "Looking through E:\\");
        log.say_at(62, "two\nlines");
        assert_eq!(
            log.tail(2),
            "1970-01-01 00:01:01  Looking through E:\\\n1970-01-01 00:01:02  two lines"
        );
        assert!(log.tail(50).starts_with("1970-01-01 00:00:00  Keevoro started"));
    }

    #[test]
    fn a_full_log_is_set_aside_and_a_new_one_started() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(NAME), vec![b'x'; usize::try_from(FULL_AT).unwrap() + 1]).unwrap();
        let log = Log::open(dir.path());
        log.say("fresh");
        assert!(dir.path().join(PREVIOUS).is_file());
        assert!(log.tail(5).ends_with("fresh"));
        assert_eq!(log.tail(5).lines().count(), 1);
    }

    #[test]
    fn a_log_that_cannot_be_opened_is_not_fatal() {
        let dir = tempfile::tempdir().unwrap();
        let in_the_way = dir.path().join("file");
        std::fs::write(&in_the_way, "x").unwrap();
        let log = Log::open(&in_the_way.join("logs"));
        log.say("dropped");
        assert_eq!(log.tail(5), "");
    }
}
