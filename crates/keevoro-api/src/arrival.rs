//! "Back up when this is plugged in": a source's own choice, off unless
//! the person switches it on, and the watch that starts a backup by itself
//! when such a source is plugged in (`sources.on_arrival`).
//!
//! The choice is kept in the backup's catalog as `arrival:<root>`, `go`
//! when on (missing or anything else is off), so it travels with the
//! backup and older versions leave it alone. Cloud storage is never
//! plugged in, so it has no such choice.
//!
//! The watch (`Keevoro::notice_arrivals`) is called every few seconds by
//! the app while it runs. Its rules are pure, in `keevoro_core::arrival`:
//! a source is backed up only once it has been plugged in (not merely
//! there when Keevoro started) and has stayed in for a few seconds, at most
//! once in ten minutes, never while paused, and never as a drive Keevoro
//! has not been told about: only a source with the choice on, at its own
//! place, and recognised as the drive it was looked through on.

use crate::backups::Named;
use crate::error::{Error, Result};
use crate::jobs::JobHandle;
use crate::keevoro::Keevoro;
use keevoro_catalog::Catalog;
use keevoro_core::arrival::Watch;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// The catalog setting for one source's choice.
pub fn arrival_key(root: &Path) -> String {
    keevoro_core::arrival::key(root)
}

/// The sources with the choice on are read again from the catalogs at
/// most this often, in seconds, or at once when it is changed here.
pub const READ_AGAIN_EVERY: i64 = 60;

/// How long a backup started by a source being plugged in waits for a
/// good moment (off battery, say), in minutes, before giving up for now.
pub const WAITS_AT_MOST: u32 = 60;

/// `sources.on_arrival`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OnArrivalRequest {
    /// The source's id, as `sources.list` gives it.
    pub source: i64,
    /// True to back it up by itself whenever it is plugged in, false to
    /// stop. Left out, nothing changes.
    #[serde(default)]
    pub on: Option<bool>,
}

/// Whether a source is backed up by itself when it is plugged in.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OnArrivalView {
    pub source: i64,
    pub on: bool,
}

/// One backup started because its source was plugged in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Arrived {
    pub backup: Named,
    /// The source's name, as the person knows it.
    pub source: String,
    pub job: JobHandle,
}

/// One source with the choice on.
#[derive(Debug, Clone)]
struct Wanted {
    backup: Named,
    id: i64,
    root: PathBuf,
    name: String,
}

/// What the watch keeps between looks.
#[derive(Default)]
pub(crate) struct Arrivals {
    inner: Mutex<Kept>,
}

#[derive(Default)]
struct Kept {
    wanted: Vec<Wanted>,
    read_at: Option<i64>,
    watches: HashMap<(String, PathBuf), Watch>,
}

impl Arrivals {
    fn forget(&self) {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .read_at = None;
    }
}

/// Whether the choice is on for the source at `root`.
fn is_on(catalog: &Catalog, root: &Path) -> Result<bool> {
    Ok(catalog.setting(&arrival_key(root))?.is_some_and(|v| v.trim() == "go"))
}

impl Keevoro {
    /// `sources.on_arrival`: whether a source is backed up by itself when
    /// it is plugged in, and changing it.
    pub fn on_arrival(&self, backup: &Named, source: i64, on: Option<bool>) -> Result<OnArrivalView> {
        let catalog = Catalog::open_in_library(&backup.home)?;
        let found = catalog
            .sources()?
            .into_iter()
            .find(|s| s.id == source)
            .ok_or_else(|| Error::not_found("That source isn't part of this backup."))?;
        if let Some(on) = on {
            if on && found.bucket.is_some() {
                return Err(Error::refused(
                    "Cloud storage is never plugged in, so only a drive, a card or a folder can be backed up when it is.",
                ));
            }
            catalog.set_setting(&arrival_key(&found.root), if on { "go" } else { "" })?;
            self.arrivals.forget();
            self.log().say(&format!(
                "\"{}\" in \"{}\" {}",
                found.name,
                backup.name,
                if on {
                    "is backed up by itself whenever it is plugged in"
                } else {
                    "is no longer backed up by itself when it is plugged in"
                }
            ));
        }
        Ok(OnArrivalView {
            source,
            on: is_on(&catalog, &found.root)?,
        })
    }

    /// One look of the watch: starts a backup by itself for each source with
    /// the choice on that has just been plugged in and has settled, and
    /// gives those it started. The app calls it every few seconds.
    pub fn notice_arrivals(&self) -> Vec<Arrived> {
        let now = self.clock.now().as_second();
        let paused = keevoro_core::pause::is_paused();
        let mut kept = self
            .arrivals
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if kept.read_at.is_none_or(|at| now - at >= READ_AGAIN_EVERY || now < at) {
            kept.wanted = self.sources_wanted_on_arrival();
            kept.read_at = Some(now);
            let wanted: Vec<(String, PathBuf)> = kept
                .wanted
                .iter()
                .map(|w| (w.backup.id.clone(), w.root.clone()))
                .collect();
            kept.watches.retain(|key, _| wanted.contains(key));
        }
        let mut started = Vec::new();
        for wanted in kept.wanted.clone() {
            let there = keevoro_engine::reach::is_there(&wanted.root);
            let key = (wanted.backup.id.clone(), wanted.root.clone());
            let go = kept.watches.entry(key.clone()).or_default().look(there, now, paused);
            if !go {
                continue;
            }
            match self.start_for_arrival(&wanted) {
                Ok(Some(job)) => started.push(Arrived {
                    backup: wanted.backup.clone(),
                    source: wanted.name.clone(),
                    job,
                }),
                Ok(None) => {}
                Err(e) if e.kind == crate::Kind::Busy => {
                    if let Some(watch) = kept.watches.get_mut(&key) {
                        watch.busy(now);
                    }
                }
                Err(e) => self.log().say(&format!(
                    "Couldn't back up \"{}\" now that \"{}\" is plugged in: {e}",
                    wanted.backup.name, wanted.name
                )),
            }
        }
        started
    }

    /// Starts the backup for a source plugged in, unless the drive there is
    /// not the one it was looked through on.
    fn start_for_arrival(&self, wanted: &Wanted) -> Result<Option<JobHandle>> {
        if crate::jobs::there_as_itself(&wanted.backup, wanted.id)? {
            self.log().say(&format!(
                "\"{}\" is plugged in: backing up \"{}\" by itself",
                wanted.name, wanted.backup.name
            ));
            self.run_by_itself_for_source(&wanted.backup, wanted.id, WAITS_AT_MOST)
                .map(Some)
        } else {
            self.log().say(&format!(
                "A drive was plugged in at the place of \"{}\", but it is not the one Keevoro knows, so nothing was backed up",
                wanted.name
            ));
            Ok(None)
        }
    }

    /// Every source with the choice on, in every backup whose catalog can
    /// be opened now.
    fn sources_wanted_on_arrival(&self) -> Vec<Wanted> {
        let mut wanted = Vec::new();
        for backup in self.backups().all() {
            let Some(catalog) = crate::backup::existing_catalog(&backup.home) else {
                continue;
            };
            let Ok(sources) = catalog.sources() else { continue };
            for s in sources.into_iter().filter(|s| s.bucket.is_none()) {
                if is_on(&catalog, &s.root).unwrap_or(false) {
                    wanted.push(Wanted {
                        backup: backup.clone(),
                        id: s.id,
                        root: s.root,
                        name: s.name,
                    });
                }
            }
        }
        wanted
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jobs::tests::{card_going_to_usb_for, keevoro_for_tests, one_at_a_time, plugged_in_and_quiet};
    use std::sync::Arc;

    #[derive(Clone)]
    struct Hand(Arc<Mutex<i64>>);

    impl crate::schedule::Clock for Hand {
        fn now(&self) -> jiff::Timestamp {
            jiff::Timestamp::from_second(*self.0.lock().unwrap()).unwrap()
        }
        fn zone(&self) -> jiff::tz::TimeZone {
            jiff::tz::TimeZone::UTC
        }
    }

    #[test]
    fn a_source_asked_for_is_backed_up_by_itself_once_plugged_in_and_settled() {
        let _turn = one_at_a_time();
        let dir = tempfile::tempdir().unwrap();
        let clock = Hand(Arc::new(Mutex::new(1_780_000_000)));
        let at = |t: i64| *clock.0.lock().unwrap() = 1_780_000_000 + t;
        let keevoro = keevoro_for_tests(dir.path())
            .with_signals(plugged_in_and_quiet())
            .with_clock(clock.clone());
        let (named, card, usb) = card_going_to_usb_for(dir.path(), &keevoro);
        let catalog = Catalog::open_in_library(&named.home).unwrap();
        catalog.set_arrange(keevoro_core::Arrange::ByDate).unwrap();
        let source = catalog.sources().unwrap()[0].id;
        let drawer = dir.path().join("In a drawer");
        let unplug = || std::fs::rename(&card, &drawer).unwrap();
        let plug_in = || std::fs::rename(&drawer, &card).unwrap();

        // Off unless asked: plugging it in does nothing.
        assert!(!keevoro.on_arrival(&named, source, None).unwrap().on);
        keevoro.notice_arrivals();
        unplug();
        keevoro.notice_arrivals();
        plug_in();
        at(100);
        assert!(keevoro.notice_arrivals().is_empty());
        at(200);
        assert!(keevoro.notice_arrivals().is_empty());

        // Asked for. In already when the watch first looks: left alone.
        assert!(keevoro.on_arrival(&named, source, Some(true)).unwrap().on);
        at(300);
        assert!(keevoro.notice_arrivals().is_empty());
        unplug();
        at(301);
        assert!(keevoro.notice_arrivals().is_empty());

        // Plugged in: nothing until it has settled, then one backup of it.
        plug_in();
        at(400);
        assert!(keevoro.notice_arrivals().is_empty());
        at(405);
        assert!(keevoro.notice_arrivals().is_empty());
        at(410);
        let started = keevoro.notice_arrivals();
        assert_eq!(started.len(), 1);
        assert_eq!(
            (started[0].source.as_str(), started[0].backup.name.as_str()),
            ("Card", "Card photos")
        );
        assert_eq!(
            started[0].job.by_itself,
            Some(keevoro_core::unattended::Occasion::SourceArrived)
        );
        keevoro.wait(started[0].job.id);
        keevoro.wait_for_jobs();
        assert!(usb.join("originals").is_dir(), "it backed up");
        at(420);
        assert!(keevoro.notice_arrivals().is_empty(), "once");

        // Paused: nothing starts.
        unplug();
        at(2_000);
        keevoro.notice_arrivals();
        plug_in();
        keevoro_core::pause::pause(keevoro_core::pause::PausedBy::ThisProgram);
        let _carry_on = crate::jobs::tests::CarryOnAfter;
        at(2_100);
        keevoro.notice_arrivals();
        at(2_200);
        assert!(keevoro.notice_arrivals().is_empty(), "paused");
        keevoro_core::pause::carry_on();
        at(2_201);
        let started = keevoro.notice_arrivals();
        assert_eq!(started.len(), 1, "carried on, and still plugged in");
        keevoro.wait(started[0].job.id);
        keevoro.wait_for_jobs();

        // Switched off: plugging it in does nothing again.
        assert!(!keevoro.on_arrival(&named, source, Some(false)).unwrap().on);
        unplug();
        at(4_000);
        keevoro.notice_arrivals();
        plug_in();
        at(4_100);
        keevoro.notice_arrivals();
        at(4_200);
        assert!(keevoro.notice_arrivals().is_empty());
        let e = keevoro.on_arrival(&named, 999, Some(true)).unwrap_err();
        assert_eq!(e.kind, crate::Kind::NotFound);
    }
}
