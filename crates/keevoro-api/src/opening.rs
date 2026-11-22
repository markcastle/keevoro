//! Opening a backup that is already somewhere: on a new computer, after a
//! reinstall, or when Keevoro's record of it here was lost.
//!
//! Every destination holds a copy of Keevoro's notes about the backup
//! (`keevoro_engine::notes`). `backups.find_at` reads them, only looking,
//! and says what is there; `backups.open_from` builds a new backup's record
//! from them, with that place as its destination, so it carries on without
//! sending anything again. Its sources wait to be connected again, by the
//! person (`sources.connect_again`): where the photos were on the old
//! computer is said, never assumed.

use crate::backup::Backup;
use crate::destinations::{place_of, without_keys, AddDestinationRequest, Key};
use crate::error::{Error, Result};
use crate::keevoro::Keevoro;
use crate::Named;
use keevoro_catalog::{Catalog, NOTES_ID};
use keevoro_engine::connect::{self, Keys, Saved};
use keevoro_engine::notes::{self, Found, Notes};
use keevoro_engine::summary;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// What Keevoro found at a place.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum WhatIsThere {
    /// A backup, with notes Keevoro can open it from.
    Notes,
    /// A backup whose newest notes are damaged; the copy before them is
    /// used.
    EarlierNotes,
    /// A backup whose notes a newer Keevoro wrote.
    Newer,
    /// A backup with no notes: made before Keevoro kept them.
    NoNotes,
    /// A backup whose notes, and the copy before them, are damaged.
    Damaged,
    /// No Keevoro backup.
    Nothing,
}

/// One of the backup's sources, as the computer it came from had it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FoundSource {
    pub name: String,
    pub was_at: String,
}

/// `backups.find_at`: what is at a place.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FoundBackup {
    pub found: WhatIsThere,
    /// The place in words: "the folder F:\Family Backup".
    pub place: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Its originals, and their size together.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub files: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bytes: Option<u64>,
    /// When it was last backed up (its notes written), in seconds since
    /// 1970, and as a day in words.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_backed_up: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_backed_up_said: Option<String>,
    /// When a copy there was last proved by reading it back.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_proved: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_proved_said: Option<String>,
    /// Which Keevoro wrote its notes: "Keevoro 0.49.0".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub made_by: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<FoundSource>,
    /// The backup on this computer that is this one already, by name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub already_open: Option<String>,
    /// All of it as a person reads it, a sentence each.
    pub said: Vec<String>,
}

/// A source opened from notes that waits to be connected again.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WaitingSource {
    pub id: i64,
    pub name: String,
    pub was_at: String,
}

/// `backups.open_from`: the backup opened.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenedBackup {
    pub id: String,
    pub name: String,
    /// True when it was on this computer already, and that one was opened.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub already_open: bool,
    pub files: usize,
    /// Its sources, each waiting to be connected on this computer.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub waiting: Vec<WaitingSource>,
    pub said: Vec<String>,
}

/// `sources.connect_again`
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConnectAgainRequest {
    /// The source, by the id `sources.list` gives.
    pub source: i64,
    /// Where its files are on this computer: a drive or folder.
    pub place: String,
}

/// What `sources.connect_again` did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectedAgain {
    pub source: i64,
    pub name: String,
    pub place: String,
    pub said: String,
}

/// What is said once a source is connected again.
pub const CONNECTED_SAID: &str = "Connected. Look through it again to carry on: files already backed up are \
                                  recognised by their fingerprints and not sent again.";

/// The place a request names, ready to be read, with any keys given.
fn to_read(
    request: &AddDestinationRequest,
    environment: &dyn Fn(&str) -> Option<String>,
) -> Result<(Saved, keevoro_engine::send::Destination)> {
    let saved = place_of(request)?;
    let access = request.access_key_id.as_ref().map(Key::reveal).unwrap_or_default();
    let secret = request.secret_access_key.as_ref().map(Key::reveal).unwrap_or_default();
    let given = Keys::new(access, secret);
    let destination =
        connect::to_read(&saved, given, environment).map_err(|e| without_keys(Error::from(e), &[access, secret]))?;
    Ok((saved, destination))
}

/// What is at `saved`, read, with any keys given taken out of a problem's
/// words.
fn look(request: &AddDestinationRequest, environment: &dyn Fn(&str) -> Option<String>) -> Result<(Saved, Found)> {
    let access = request.access_key_id.as_ref().map(Key::reveal).unwrap_or_default();
    let secret = request.secret_access_key.as_ref().map(Key::reveal).unwrap_or_default();
    let (saved, destination) = to_read(request, environment)?;
    let found = notes::look_for(destination)
        .map_err(|e| without_keys(Error::unreachable(format!("{e:#}")), &[access, secret]))?;
    Ok((saved, found))
}

fn day(seconds: i64) -> Option<String> {
    keevoro_engine::layout::day_in_words(seconds)
}

/// Says what was found.
pub fn found_words(found: &FoundBackup) -> Vec<String> {
    let mut said = Vec::new();
    match found.found {
        WhatIsThere::Notes | WhatIsThere::EarlierNotes => {
            let name = found.name.as_deref().unwrap_or("A backup");
            said.push(format!(
                "\"{name}\": {} ({}), last backed up on {}.",
                summary::count(found.files.unwrap_or(0), "file"),
                summary::bytes(found.bytes.unwrap_or(0)),
                found.last_backed_up_said.as_deref().unwrap_or("a day not recorded")
            ));
            said.push(match &found.last_proved_said {
                Some(on) => format!("Its copies were last proved, by reading them back, on {on}."),
                None => "Its copies have not been proved by reading them back yet.".to_string(),
            });
            if let Some(made_by) = &found.made_by {
                said.push(format!("Its notes were written by {made_by}."));
            }
            if found.found == WhatIsThere::EarlierNotes {
                said.push(format!(
                    "Its newest notes are damaged, so Keevoro used the copy before them, from {}. Files backed up \
                     after then are recognised when you next back up.",
                    found.last_backed_up_said.as_deref().unwrap_or("an earlier backup")
                ));
            }
            if let Some(here) = &found.already_open {
                said.push(already_words(here));
            }
        }
        WhatIsThere::Newer => said.push(notes::newer_words(found.made_by.as_deref().unwrap_or_default())),
        WhatIsThere::NoNotes => said.push(format!(
            "There is a Keevoro backup in {}, but it has no notes: it was last backed up by a Keevoro from before \
             notes were kept. Your files are safe there. Back it up once from the computer that made it, and it \
             can be opened here.",
            found.place
        )),
        WhatIsThere::Damaged => said.push(format!(
            "There is a Keevoro backup in {}, but its notes and the copy before them are damaged, so Keevoro can't \
             open it from them. Your files are safe there.",
            found.place
        )),
        WhatIsThere::Nothing => said.push(format!("Keevoro found no backup in {}.", found.place)),
    }
    said
}

fn already_words(name: &str) -> String {
    format!("This backup is already on this computer, as \"{name}\". Opening it opens that one.")
}

impl Keevoro {
    /// `backups.find_at`: looks for a backup at a folder or in cloud
    /// storage, and says what is there. Only reads.
    pub fn find_backup_at(&self, request: &AddDestinationRequest) -> Result<FoundBackup> {
        let (saved, found) = look(request, self.world().environment)?;
        let mut answer = FoundBackup {
            found: WhatIsThere::Nothing,
            place: saved.describe(),
            name: None,
            files: None,
            bytes: None,
            last_backed_up: None,
            last_backed_up_said: None,
            last_proved: None,
            last_proved_said: None,
            made_by: None,
            sources: Vec::new(),
            already_open: None,
            said: Vec::new(),
        };
        match &found {
            Found::Notes { notes, from_earlier } => {
                let about = &notes.about;
                answer.found = if *from_earlier {
                    WhatIsThere::EarlierNotes
                } else {
                    WhatIsThere::Notes
                };
                answer.name = Some(about.name.clone());
                answer.files = Some(about.files);
                answer.bytes = Some(about.bytes);
                answer.last_backed_up = Some(about.written_at);
                answer.last_backed_up_said = day(about.written_at);
                answer.last_proved = about.last_proved;
                answer.last_proved_said = about.last_proved.and_then(day);
                answer.made_by = Some(about.made_by.clone()).filter(|m| !m.is_empty());
                answer.sources = notes
                    .noted
                    .sources
                    .iter()
                    .map(|s| FoundSource {
                        name: s.name.clone(),
                        was_at: s.was_at.clone(),
                    })
                    .collect();
                answer.already_open = self.already_here(notes, &saved).map(|n| n.name);
            }
            Found::Newer { made_by } => {
                answer.found = WhatIsThere::Newer;
                answer.made_by = Some(made_by.clone()).filter(|m| !m.is_empty());
            }
            Found::NoUsableNotes { damaged: true } => answer.found = WhatIsThere::Damaged,
            Found::NoUsableNotes { damaged: false } => answer.found = WhatIsThere::NoNotes,
            Found::Nothing => {}
        }
        answer.said = found_words(&answer);
        Ok(answer)
    }

    /// The backup on this computer that is the one these notes are about:
    /// the same backup by the name its notes carry, or one that already
    /// goes to the same place.
    fn already_here(&self, notes: &Notes, saved: &Saved) -> Option<Named> {
        self.backups().all().into_iter().find(|named| {
            let Ok(catalog) = Catalog::open_in_library(&named.home) else {
                return false;
            };
            let same_id = notes.id().is_some() && catalog.setting(NOTES_ID).ok().flatten().as_deref() == notes.id();
            let same_place = connect::destinations(&catalog)
                .map(|all| all.iter().any(|d| d.same_place(saved)))
                .unwrap_or(false);
            same_id || same_place
        })
    }

    /// `backups.open_from`: opens the backup at a folder or in cloud
    /// storage as a new backup on this computer, from its notes there, with
    /// that place as its destination (and any keys given saved in the
    /// keychain). One already on this computer is opened instead.
    pub fn open_backup_from(&self, request: &AddDestinationRequest) -> Result<OpenedBackup> {
        let (saved, found) = look(request, self.world().environment)?;
        let notes = match found {
            Found::Notes { notes, .. } => notes,
            other => {
                let mut answer = self.find_backup_at(request)?;
                answer.found = match other {
                    Found::Newer { .. } => WhatIsThere::Newer,
                    Found::NoUsableNotes { damaged: true } => WhatIsThere::Damaged,
                    Found::NoUsableNotes { damaged: false } => WhatIsThere::NoNotes,
                    _ => WhatIsThere::Nothing,
                };
                let said = found_words(&answer).join(" ");
                return Err(match answer.found {
                    WhatIsThere::Nothing => Error::not_found(said),
                    _ => Error::refused(said),
                });
            }
        };
        if let Some(here) = self.already_here(&notes, &saved) {
            self.backups().switch_to(&here.id)?;
            return Ok(OpenedBackup {
                said: vec![already_words(&here.name)],
                files: notes.about.files,
                waiting: waiting_of(&Catalog::open_in_library(&here.home)?)?,
                id: here.id,
                name: here.name,
                already_open: true,
            });
        }
        let started = self.start_backup(&notes.about.name)?;
        let opened = (|| -> Result<OpenedBackup> {
            let backup = Backup::open(&started.home)?;
            self.add_destination(&backup, request)?;
            let added = connect::destinations(backup.catalog())?
                .into_iter()
                .find(|d| d.same_place(&saved))
                .ok_or_else(|| Error::failed("Keevoro couldn't add the place it found the backup in.".into()))?;
            let target = added.target.clone().unwrap_or_default();
            let mut catalog = Catalog::open_in_library(&started.home)?;
            notes::open_into(&mut catalog, &started.home, &notes, &target)?;
            keevoro_engine::confirm::look(&catalog, &added, &self.world())?;
            let waiting = waiting_of(&catalog)?;
            let mut said = vec![format!(
                "Opened \"{}\" from {}: {}, all known to be there.",
                started.name,
                saved.describe(),
                summary::count(notes.about.files, "file")
            )];
            if !waiting.is_empty() {
                said.push(
                    "Keevoro does not know where these photos live on this computer. Connect each source again to \
                     carry on backing it up: nothing already backed up is sent again."
                        .to_string(),
                );
            }
            Ok(OpenedBackup {
                id: started.id.clone(),
                name: started.name.clone(),
                already_open: false,
                files: notes.about.files,
                waiting,
                said,
            })
        })();
        match opened {
            Ok(opened) => {
                self.backups().switch_to(&started.id)?;
                self.log().say(&format!(
                    "Opened the backup \"{}\" from {}",
                    started.name,
                    saved.describe()
                ));
                Ok(opened)
            }
            Err(e) => {
                // Nothing half-made is left in the list.
                let _ = self.forget_backup(&started.id, false);
                Err(e)
            }
        }
    }
}

/// The sources of a backup that wait to be connected on this computer.
fn waiting_of(catalog: &Catalog) -> Result<Vec<WaitingSource>> {
    let mut waiting = Vec::new();
    for source in catalog.sources()? {
        if let Some(before) = catalog.elsewhere(&source.root)? {
            waiting.push(WaitingSource {
                id: source.id,
                name: source.name,
                was_at: before.was_at,
            });
        }
    }
    Ok(waiting)
}

impl Backup {
    /// `sources.connect_again`: says where a source of a backup opened from
    /// its notes is on this computer. It still has to be looked through.
    pub fn connect_again(&mut self, request: &ConnectAgainRequest) -> Result<ConnectedAgain> {
        let place = Path::new(request.place.trim());
        let name = self
            .catalog()
            .sources()?
            .into_iter()
            .find(|s| s.id == request.source)
            .map(|s| s.name)
            .ok_or_else(|| Error::not_found("That source isn't part of this backup."))?;
        notes::connect_again(self.catalog_mut(), request.source, place)
            .map_err(|e| Error::refused(format!("{e:#}")))?;
        Ok(ConnectedAgain {
            source: request.source,
            name,
            place: keevoro_engine::friendly_path(place),
            said: CONNECTED_SAID.to_string(),
        })
    }
}

#[cfg(test)]
mod tests;
