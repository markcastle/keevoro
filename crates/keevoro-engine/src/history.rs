//! A record of each backup: when, how much, how long, how fast, and, for
//! anyone who wants it, everything else that was measured.
//!
//! "Was that slow?" can only be answered afterwards if someone wrote down
//! how it went. Each time a backup runs, one entry is kept with the backup,
//! in its own record: when it finished, how much went, how long it took
//! and where to, from which the average speed follows, and its
//! [`RunDetail`]: every stage with its time and speed, what each
//! destination was sent and what could not be, the previews, and what it
//! said at the end. Entries from before the detail was kept have none, and
//! say only what they have. The last [`KEPT`] are kept, each in full: a few
//! kilobytes apiece, with the files that could not be sent listed only up
//! to [`NOT_SENT_KEPT`] and the rest counted.

use crate::summary;
use anyhow::Result;
use keevoro_catalog::Catalog;
use serde::{Deserialize, Serialize};

const SETTING: &str = "send_history";
/// How many backups are remembered, newest first.
pub const KEPT: usize = 20;
/// How many files that could not be sent are named, per destination and
/// pass; any more are counted.
pub const NOT_SENT_KEPT: usize = 50;

/// How one backup went.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    /// Everything else measured. None for a backup recorded before this
    /// was kept.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<Box<RunDetail>>,
}

/// How a backup ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Ended {
    /// It did all it could. Some files may still not have been sent; they
    /// are listed with each destination.
    Finished,
    /// The person stopped it.
    Stopped,
    /// A destination could not be reached, or had no room.
    Problem,
}

/// Everything measured about one backup.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunDetail {
    /// When it started, in seconds since 1970, UTC.
    pub started_at: i64,
    /// How much of its time was spent paused.
    pub paused_seconds: u64,
    pub ended: Ended,
    /// What went wrong, in the person's words, when it ended with a problem.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub problem: Option<String>,
    /// Who started it, in words: "the app", "the command line", "an AI agent".
    pub started_by: String,
    /// The version of Keevoro that ran it.
    pub version: String,
    /// Each destination, in the order they had their turn.
    pub destinations: Vec<DestinationRecord>,
    /// Each stage that began, in order, with its time and speed.
    pub stages: Vec<keevoro_core::estimate::StageRecord>,
    /// Copies of files left out because another copy was kept, and the
    /// room that saved.
    pub duplicates: usize,
    pub duplicate_bytes: u64,
    /// The previews made in this backup; none when it sent the originals alone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previews: Option<PreviewsMade>,
    /// What it said when it ended, exactly as the person saw it.
    pub summary: Vec<String>,
}

/// What one destination's turn did.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DestinationRecord {
    /// In words: "the folder Z:\\Backups".
    pub to: String,
    /// True when it was across a network: cloud storage, or a folder on
    /// another computer.
    pub over_network: bool,
    /// False when it could not be backed up to at all.
    pub reached: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub problem: Option<String>,
    pub seconds: u64,
    /// Everything that arrived there in its turn.
    pub bytes: u64,
    pub originals: Sending,
    /// None when no previews were sent there.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previews: Option<Sending>,
    /// How many files the index written there lists.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub indexed: Option<usize>,
}

impl DestinationRecord {
    pub fn new(to: &str) -> Self {
        Self {
            to: to.to_string(),
            ..Self::default()
        }
    }
}

/// What one pass of sending did at one destination.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Sending {
    pub sent: usize,
    pub sent_bytes: u64,
    pub already_there: usize,
    /// Files that could not be sent, each with the reason, up to
    /// [`NOT_SENT_KEPT`].
    pub not_sent: Vec<NotSent>,
    /// How many more could not be sent than are named.
    #[serde(default)]
    pub not_sent_more: usize,
}

/// A file that could not be sent, and why, as the summary said it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotSent {
    pub file: String,
    pub reason: String,
}

/// The previews a backup made.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewsMade {
    /// Made in this backup.
    pub photos: u64,
    pub videos: u64,
    /// Being kept, and without a preview because Keevoro could not open
    /// them, in this backup or an earlier one.
    pub photos_without: u64,
    pub videos_without: u64,
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

/// Adds one backup to its record, keeping the most recent [`KEPT`].
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
            detail: None,
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
    fn an_entry_from_before_the_detail_was_kept_still_reads_and_is_written_as_it_was() {
        let before = r#"[{"finishedAt":1794751331,"seconds":60,"bytes":5,"to":"the folder Z:","stopped":false}]"#;
        let catalog = Catalog::in_memory().unwrap();
        catalog.set_setting(SETTING, before).unwrap();
        let read = recent(&catalog).unwrap();
        assert_eq!((read[0].bytes, read[0].detail.is_none()), (5, true));
        assert_eq!(serde_json::to_string(&read).unwrap(), before);
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
