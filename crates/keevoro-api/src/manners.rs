//! How a backup by itself behaves as a guest on the computer
//! (`background.settings`): whether it waits while the computer is on
//! battery and less than half charged, whether a backup to cloud storage
//! waits on a metered connection, and the hours it may start in.
//!
//! Kept in a file of its own, `background.json` in Keevoro's config
//! folder, never in `settings.json`, which older versions write back
//! without parts they do not know. It is read a part at a time, so a part
//! that cannot be understood costs that part alone, and parts a later
//! version adds are kept and written back. A missing file means every
//! choice as it comes: wait on battery, wait on a metered connection, any
//! hour.
//!
//! The rules are pure, in `keevoro_core::background::may_start`; a run by
//! itself reads this file each time it looks whether it may start.

use crate::error::{Error, Result};
use crate::keevoro::Keevoro;
use keevoro_core::background::Hours;
use keevoro_core::unattended::Occasion;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// The file, in Keevoro's config folder.
pub const FILE: &str = "background.json";

const KNOWN: &[&str] = &["v", "waitOnBattery", "waitOnMetered", "between"];

/// How a backup by itself behaves as a guest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manners {
    /// Wait while on battery and less than half charged. On unless the
    /// person switched it off.
    pub wait_on_battery: bool,
    /// A backup to or from cloud storage waits on a metered connection.
    pub wait_on_metered: bool,
    /// Only start between these hours; `None` for any hour.
    pub between: Option<Hours>,
    later: serde_json::Map<String, serde_json::Value>,
}

impl Default for Manners {
    fn default() -> Self {
        Self {
            wait_on_battery: true,
            wait_on_metered: true,
            between: None,
            later: serde_json::Map::new(),
        }
    }
}

/// The hours as kept: `{ "from": "22:00", "to": "07:00" }`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Between {
    /// `HH:MM`, in this computer's own time.
    pub from: String,
    /// `HH:MM`; before `from` means across midnight.
    pub to: String,
}

impl Between {
    fn hours(&self) -> Option<Hours> {
        Some(Hours {
            from: keevoro_core::schedule::time_named(&self.from)?,
            to: keevoro_core::schedule::time_named(&self.to)?,
        })
    }

    fn of(hours: &Hours) -> Self {
        Self {
            from: keevoro_core::schedule::time_words(hours.from),
            to: keevoro_core::schedule::time_words(hours.to),
        }
    }
}

impl Manners {
    /// Reads them from `config`: as they come when there is no file or it
    /// cannot be read, each part on its own otherwise.
    pub fn read(config: &Path) -> Self {
        let Some(all) = std::fs::read_to_string(config.join(FILE))
            .ok()
            .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
            .and_then(|v| v.as_object().cloned())
        else {
            return Self::default();
        };
        let flag = |name: &str| all.get(name).and_then(serde_json::Value::as_bool).unwrap_or(true);
        Self {
            wait_on_battery: flag("waitOnBattery"),
            wait_on_metered: flag("waitOnMetered"),
            between: all
                .get("between")
                .and_then(|v| serde_json::from_value::<Between>(v.clone()).ok())
                .and_then(|b| b.hours()),
            later: all
                .iter()
                .filter(|(key, _)| !KNOWN.contains(&key.as_str()))
                .map(|(key, v)| (key.clone(), v.clone()))
                .collect(),
        }
    }

    /// Writes them beside the real file and moves it over, so a crash never
    /// leaves half a file.
    pub fn save(&self, config: &Path) -> std::io::Result<()> {
        let mut all = self.later.clone();
        all.insert("v".into(), 1.into());
        all.insert("waitOnBattery".into(), self.wait_on_battery.into());
        all.insert("waitOnMetered".into(), self.wait_on_metered.into());
        if let Some(hours) = &self.between {
            all.insert(
                "between".into(),
                serde_json::to_value(Between::of(hours)).map_err(std::io::Error::other)?,
            );
        }
        std::fs::create_dir_all(config)?;
        let text = serde_json::to_string_pretty(&serde_json::Value::Object(all)).map_err(std::io::Error::other)?;
        let temp = config.join(format!(".{FILE}.{}.part", std::process::id()));
        std::fs::write(&temp, text)?;
        std::fs::rename(&temp, config.join(FILE)).inspect_err(|_| {
            let _ = std::fs::remove_file(&temp);
        })
    }

    /// Whether a run that started for `occasion` must wait for the hours
    /// at `now` in `zone`. Only a run that came round by itself does: one
    /// the person asked for, a source plugged in, or the person's own
    /// scheduler, starts when it is asked to.
    pub fn outside_hours(&self, occasion: Occasion, now: jiff::Timestamp, zone: &jiff::tz::TimeZone) -> bool {
        let by_the_clock = matches!(occasion, Occasion::Schedule | Occasion::CatchUp);
        by_the_clock
            && self
                .between
                .is_some_and(|h| !h.contain(now.to_zoned(zone.clone()).time()))
    }
}

/// `background.settings`. Each part left out is left as it is.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MannersRequest {
    /// Wait while this computer is on battery and less than half charged.
    #[serde(default)]
    pub wait_on_battery: Option<bool>,
    /// A backup to or from cloud storage waits on a metered connection.
    #[serde(default)]
    pub wait_on_metered: Option<bool>,
    /// Only start a backup by itself between these hours. `anyHour` true
    /// takes them away.
    #[serde(default)]
    pub between: Option<Between>,
    #[serde(default)]
    pub any_hour: Option<bool>,
}

/// How a backup by itself behaves as a guest, as a screen shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MannersView {
    pub wait_on_battery: bool,
    pub wait_on_metered: bool,
    /// False where this computer cannot say whether its connection is
    /// metered: then nothing waits for one, and the screen says so.
    pub metered_known: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub between: Option<Between>,
    /// The hours in words: "between 22:00 and 07:00".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub between_said: Option<String>,
}

impl MannersView {
    fn of(manners: &Manners) -> Self {
        Self {
            wait_on_battery: manners.wait_on_battery,
            wait_on_metered: manners.wait_on_metered,
            metered_known: keevoro_frame::network::CAN_TELL,
            between: manners.between.as_ref().map(Between::of),
            between_said: manners.between.as_ref().map(Hours::words),
        }
    }
}

impl Keevoro {
    /// How a backup by itself behaves as a guest.
    pub fn manners(&self) -> Manners {
        Manners::read(self.backups().config_folder())
    }

    /// `background.settings`: reads them, and changes the parts asked for.
    pub fn background_settings(&self, request: &MannersRequest) -> Result<MannersView> {
        let config = self.backups().config_folder();
        let mut manners = Manners::read(config);
        let asked = request.wait_on_battery.is_some()
            || request.wait_on_metered.is_some()
            || request.between.is_some()
            || request.any_hour.is_some();
        if !asked {
            return Ok(MannersView::of(&manners));
        }
        if let Some(on) = request.wait_on_battery {
            manners.wait_on_battery = on;
        }
        if let Some(on) = request.wait_on_metered {
            manners.wait_on_metered = on;
        }
        if let Some(between) = &request.between {
            manners.between = Some(
                between
                    .hours()
                    .ok_or_else(|| Error::not_understood("the hours should be written like 22:00"))?,
            );
        }
        if request.any_hour == Some(true) {
            manners.between = None;
        }
        manners
            .save(config)
            .map_err(|e| Error::failed(format!("Keevoro couldn't save how it backs up by itself: {e}.")))?;
        self.log().say(&format!(
            "Backing up by itself: {} on battery, {} on a metered connection, {}",
            if manners.wait_on_battery {
                "waits"
            } else {
                "does not wait"
            },
            if manners.wait_on_metered {
                "waits"
            } else {
                "does not wait"
            },
            manners
                .between
                .as_ref()
                .map_or_else(|| "at any hour".to_string(), Hours::words)
        ));
        Ok(MannersView::of(&manners))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(hour: i8) -> jiff::Timestamp {
        jiff::civil::date(2026, 6, 10)
            .at(hour, 0, 0, 0)
            .to_zoned(jiff::tz::TimeZone::UTC)
            .unwrap()
            .timestamp()
    }

    #[test]
    fn missing_damaged_and_later_files_are_read_a_part_at_a_time() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(Manners::read(dir.path()), Manners::default(), "none: as they come");
        std::fs::write(dir.path().join(FILE), "not json").unwrap();
        assert_eq!(Manners::read(dir.path()), Manners::default());
        std::fs::write(
            dir.path().join(FILE),
            r#"{ "v": 2, "waitOnBattery": false, "waitOnMetered": "maybe", "between": { "from": "25:00", "to": "07:00" }, "later": [1] }"#,
        )
        .unwrap();
        let read = Manners::read(dir.path());
        assert!(!read.wait_on_battery && read.wait_on_metered && read.between.is_none());
        read.save(dir.path()).unwrap();
        let kept = std::fs::read_to_string(dir.path().join(FILE)).unwrap();
        assert!(kept.contains("\"later\""), "a later version's part is kept: {kept}");
    }

    #[test]
    fn only_a_run_that_came_round_by_itself_waits_for_the_hours() {
        let manners = Manners {
            between: Some(Hours {
                from: keevoro_core::schedule::time_named("22:00").unwrap(),
                to: keevoro_core::schedule::time_named("07:00").unwrap(),
            }),
            ..Manners::default()
        };
        let utc = jiff::tz::TimeZone::UTC;
        assert!(manners.outside_hours(Occasion::Schedule, at(14), &utc));
        assert!(manners.outside_hours(Occasion::CatchUp, at(14), &utc));
        assert!(!manners.outside_hours(Occasion::Schedule, at(2), &utc));
        for asked in [Occasion::RunNow, Occasion::SourceArrived, Occasion::CommandLine] {
            assert!(!manners.outside_hours(asked, at(14), &utc), "{asked:?}");
        }
        assert!(!Manners::default().outside_hours(Occasion::Schedule, at(14), &utc));
    }

    #[test]
    fn the_settings_are_read_changed_and_said() {
        let dir = tempfile::tempdir().unwrap();
        let keevoro = crate::keevoro::tests::keevoro(dir.path());
        let shown = keevoro.background_settings(&MannersRequest::default()).unwrap();
        assert!(shown.wait_on_battery && shown.wait_on_metered && shown.between.is_none());
        assert_eq!(shown.metered_known, cfg!(windows));
        let changed = keevoro
            .background_settings(&MannersRequest {
                wait_on_metered: Some(false),
                between: Some(Between {
                    from: "22:00".into(),
                    to: "07:00".into(),
                }),
                ..MannersRequest::default()
            })
            .unwrap();
        assert!(!changed.wait_on_metered);
        assert_eq!(changed.between_said.as_deref(), Some("between 22:00 and 07:00"));
        let any = keevoro
            .background_settings(&MannersRequest {
                any_hour: Some(true),
                ..MannersRequest::default()
            })
            .unwrap();
        assert_eq!(any.between, None);
        let wrong = keevoro.background_settings(&MannersRequest {
            between: Some(Between {
                from: "late".into(),
                to: "07:00".into(),
            }),
            ..MannersRequest::default()
        });
        assert_eq!(wrong.unwrap_err().kind, crate::Kind::NotUnderstood);
    }
}
