//! A record of each send: when, how much, how long, how fast.
//!
//! "Was that slow?" can only be answered afterwards if someone wrote down
//! how it went. Each time a backup's files are sent, one line is kept with
//! the backup: when it finished, how much went, how long it took and where
//! to. From those the average speed follows. The last twenty are kept.

use crate::summary;
use anyhow::Result;
use keevoro_catalog::Catalog;
use serde::{Deserialize, Serialize};

const SETTING: &str = "send_history";
const KEPT: usize = 20;

/// How one send went.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Sent {
    /// When it finished, in seconds since 1970, UTC.
    pub finished_at: i64,
    /// How long it took, from pressing Send to the end.
    pub seconds: u64,
    /// How much arrived: the files themselves, their previews and the index.
    pub bytes: u64,
    /// Where it went, in words: "the folder Z:\\Backups".
    pub to: String,
    /// True when it was stopped before everything had gone.
    pub stopped: bool,
}

impl Sent {
    /// The average speed in bytes a second, over the whole send. This
    /// counts the time spent reading and checking files before they went,
    /// so it is what the person actually waited for, not a best moment.
    pub fn average_speed(&self) -> u64 {
        self.bytes / self.seconds.max(1)
    }

    /// The sentence to say when it finishes. Nothing is said about a send
    /// that had nothing to send.
    pub fn sentence(&self) -> Option<String> {
        (self.bytes > 0).then(|| {
            format!(
                "That took {}: {} at an average of {} a second.",
                duration(self.seconds),
                summary::bytes(self.bytes),
                summary::bytes(self.average_speed())
            )
        })
    }
}

/// A length of time as a person would say it, without pretending to the
/// second: "under a minute", "about 36 minutes", "about 2 hours 10 minutes".
pub fn duration(seconds: u64) -> String {
    if seconds < 60 {
        return "under a minute".to_string();
    }
    let minutes = (seconds + 30) / 60;
    if minutes < 60 {
        return format!("about {minutes} {}", if minutes == 1 { "minute" } else { "minutes" });
    }
    // Past an hour, the nearest five minutes is as fine as it should be.
    let rounded = (minutes + 2) / 5 * 5;
    let (hours, rest) = (rounded / 60, rounded % 60);
    let h = format!("{hours} {}", if hours == 1 { "hour" } else { "hours" });
    if rest == 0 {
        format!("about {h}")
    } else {
        format!("about {h} {rest} minutes")
    }
}

/// Adds one send to the backup's record, keeping the most recent twenty.
pub fn record(catalog: &Catalog, sent: &Sent) -> Result<()> {
    let mut all = recent(catalog)?;
    all.insert(0, sent.clone());
    all.truncate(KEPT);
    catalog.set_setting(SETTING, &serde_json::to_string(&all)?)?;
    Ok(())
}

/// The backup's sends, newest first. A record that cannot be read is
/// treated as empty: it is a convenience, never a reason to fail.
pub fn recent(catalog: &Catalog) -> Result<Vec<Sent>> {
    Ok(catalog
        .setting(SETTING)?
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    const GB: u64 = 1024 * 1024 * 1024;

    fn sent(bytes: u64, seconds: u64) -> Sent {
        Sent {
            finished_at: 1_794_751_331,
            seconds,
            bytes,
            to: "the folder Z:\\Backups".into(),
            stopped: false,
        }
    }

    #[test]
    fn a_finished_send_says_how_long_how_much_and_how_fast() {
        // 30 GB in 41 minutes.
        let it = sent(30 * GB, 41 * 60);
        assert_eq!(it.average_speed(), 30 * GB / (41 * 60));
        assert_eq!(
            it.sentence().unwrap(),
            "That took about 41 minutes: 30.0 GB at an average of 12.5 MB a second."
        );
        assert_eq!(
            sent(0, 12).sentence(),
            None,
            "nothing is said when nothing needed sending"
        );
        assert_eq!(
            sent(5000, 0).average_speed(),
            5000,
            "an instant send does not divide by nothing"
        );
    }

    #[test]
    fn lengths_of_time_are_said_as_a_person_would() {
        assert_eq!(duration(20), "under a minute");
        assert_eq!(duration(60), "about 1 minute");
        assert_eq!(duration(36 * 60), "about 36 minutes");
        assert_eq!(duration(3600), "about 1 hour");
        assert_eq!(duration(2 * 3600 + 9 * 60), "about 2 hours 10 minutes");
        assert_eq!(duration(3600 + 58 * 60), "about 2 hours");
    }

    #[test]
    fn the_last_twenty_are_kept_newest_first() {
        let catalog = Catalog::in_memory().unwrap();
        assert!(recent(&catalog).unwrap().is_empty());
        for n in 0..25 {
            record(&catalog, &sent(n, 60)).unwrap();
        }
        let kept = recent(&catalog).unwrap();
        assert_eq!(kept.len(), 20);
        assert_eq!((kept[0].bytes, kept[19].bytes), (24, 5));
    }

    #[test]
    fn a_record_that_cannot_be_read_is_empty_not_fatal() {
        let catalog = Catalog::in_memory().unwrap();
        catalog.set_setting(SETTING, "{ not a list").unwrap();
        assert!(recent(&catalog).unwrap().is_empty());
        record(&catalog, &sent(7, 1)).unwrap();
        assert_eq!(recent(&catalog).unwrap().len(), 1);
    }
}
