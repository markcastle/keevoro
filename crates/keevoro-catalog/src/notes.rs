//! What a destination's copy of Keevoro's notes holds, read out of a
//! catalog and written back into one.
//!
//! After each backup Keevoro leaves a copy of its notes about that backup
//! at the destination (`keevoro_engine::notes` writes and reads the file),
//! so a new computer can pick the backup up again. This module is the part
//! that knows the catalog: [`Catalog::notes_for`] says, for one
//! destination, every file Keevoro sent there with its place, size,
//! fingerprints, dates and kind, the sources it came from (as history
//! only), and the backup's choices that make sense on another computer;
//! [`Catalog::take_notes`] builds a new catalog from them, and
//! [`Catalog::add_noted`] adds what another computer knows to this one.
//!
//! What never goes in: keys or anything from the keychain, the log, report
//! tokens, and anything that only means something on the computer that
//! wrote it as a fact to act on. A source's place on the old computer and
//! the drive it was on travel as history (`wasAt`), and a source opened
//! from the notes is never looked for there: the person connects it again.

use crate::{arrange_key, left_out_key, path_text, to_i64, wants_key, Catalog, Result};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The backup's own settings that make sense on another computer, by the
/// name the catalog keeps them under. Anything else (the destinations
/// with their disks, marks, what a look found, the history) stays here.
pub const SETTINGS_THAT_TRAVEL: [&str; 6] = [
    "wants",
    "arrange",
    "arranged_as",
    "arrange_changed_at",
    "previews",
    NOTES_ID,
];

/// The catalog setting holding the backup's own name in its notes: made
/// once, at random, the first time notes are written, and carried with
/// them, so the same backup opened twice on one computer is recognised.
pub const NOTES_ID: &str = "notes_id";

/// A source opened from notes, not yet connected on this computer: JSON
/// [`Elsewhere`], by the place it is kept at in the catalog.
pub fn elsewhere_key(root: &str) -> String {
    format!("elsewhere:{root}")
}

/// Where a source opened from notes was on the computer that wrote them.
/// History only: it is said, never looked at.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Elsewhere {
    pub was_at: String,
    /// `folder` or `bucket`.
    pub kind: String,
}

/// One source, as history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotedSource {
    pub name: String,
    /// Where it was on the computer that wrote the notes, as that computer
    /// wrote it.
    pub was_at: String,
    /// `folder` or `bucket`.
    pub kind: String,
    /// When it was last looked through, in seconds since 1970.
    pub looked_at: i64,
    /// Its own choices, as the catalog keeps them (JSON text): what it is
    /// for, how it is arranged, and the endings left out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wants: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arrange: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub left_out_endings: Option<String>,
}

/// What was found of a file on its source, as history and for the index.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotedFound {
    /// Which source, by its place in the list of sources.
    pub source: usize,
    /// Where it was inside that source, folders joined by `/`.
    pub path: String,
    /// `photo`, `video` or `other`.
    pub kind: String,
    pub mime: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date_source: Option<String>,
    /// The folders it was in, as the album the index shows (JSON text).
    pub album: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub folder_date: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub junk: Option<String>,
}

/// The previews made of a photo, so the index can go on pointing at them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotedPreview {
    pub width: u32,
    pub height: u32,
    pub blurhash: String,
}

/// What is known about a video and the copies made of it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NotedVideo {
    pub duration: f64,
    pub width: u32,
    pub height: u32,
    pub poster: bool,
    pub mp4: bool,
}

/// One file at the destination that Keevoro sent there: an original, or a
/// preview made of one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotedFile {
    /// Its place at the destination, which is also its storage key.
    pub key: String,
    pub bytes: u64,
    /// The BLAKE3 fingerprint of the whole file, for an original.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blake3: Option<String>,
    /// The quick fingerprint a fast search knows it by, when it had one,
    /// so a fast search on a new computer still recognises it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quick: Option<String>,
    /// What the last look there found: 0 there, 1 missing, 2 another size.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub gone: i64,
    /// When it was last read back in full and found exact, in seconds
    /// since 1970.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proved_at: Option<i64>,
    /// The person has since left it out: it is still there, never removed,
    /// but no longer offered.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub left_out: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub found: Option<NotedFound>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview: Option<NotedPreview>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub video: Option<NotedVideo>,
}

fn is_zero(n: &i64) -> bool {
    *n == 0
}

/// Everything the notes for one destination hold, before they are written
/// as a file.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Noted {
    /// The backup's settings that travel, by name, as the catalog keeps
    /// them.
    pub settings: BTreeMap<String, String>,
    pub sources: Vec<NotedSource>,
    /// In order of key.
    pub files: Vec<NotedFile>,
}

impl Catalog {
    /// Everything Keevoro's notes at `target` should say, as the catalog
    /// stands now. One query for the files, so it stays quick for a
    /// hundred thousand of them.
    pub fn notes_for(&self, target: &str) -> Result<Noted> {
        let mut settings = BTreeMap::new();
        for name in SETTINGS_THAT_TRAVEL {
            if let Some(value) = self.setting(name)? {
                settings.insert(name.to_string(), value);
            }
        }
        // Each source as history, and where each of its files was in it.
        let mut sources = Vec::new();
        let mut roots: BTreeMap<i64, (usize, PathBuf)> = BTreeMap::new();
        {
            let mut q = self
                .conn
                .prepare("SELECT id, name, root, kind, place, scanned_at FROM sources ORDER BY id")?;
            let rows = q.query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, Option<String>>(4)?,
                    r.get::<_, i64>(5)?,
                ))
            })?;
            for row in rows {
                let (id, name, root, kind, place, looked_at) = row?;
                // One opened from notes and not connected since keeps the
                // place it had on the computer it came from.
                let before: Option<Elsewhere> = self
                    .setting(&elsewhere_key(&root))?
                    .and_then(|text| serde_json::from_str(&text).ok());
                let (was_at, kind) = match before {
                    Some(before) => (before.was_at, before.kind),
                    None if kind == "bucket" => (place.unwrap_or_default(), kind),
                    None => (root.clone(), kind),
                };
                roots.insert(id, (sources.len(), PathBuf::from(&root)));
                sources.push(NotedSource {
                    name,
                    was_at,
                    kind,
                    looked_at,
                    wants: self.setting(&wants_key(&root))?,
                    arrange: self.setting(&arrange_key(&root))?,
                    left_out_endings: self.setting(&left_out_key(&root))?,
                });
            }
        }
        let mut q = self.conn.prepare(
            "SELECT u.key, u.bytes, u.gone, u.proved_at, c.blake3,
                    i.source_id, i.path, i.kind, i.mime, i.date, i.date_source, i.album, i.folder_date,
                    i.width, i.height, i.junk, i.quick,
                    ch.keep,
                    r.ok, r.width, r.height, r.blurhash,
                    v.ok, v.duration, v.width, v.height, v.poster, v.mp4
             FROM uploaded u
             LEFT JOIN collected c ON c.path = u.key
             LEFT JOIN items i ON i.id = (SELECT MIN(id) FROM items WHERE blake3 = c.blake3 AND hashed = 1)
             LEFT JOIN choices ch ON ch.blake3 = c.blake3
             LEFT JOIN renditions r ON r.blake3 = c.blake3
             LEFT JOIN videos v ON v.blake3 = c.blake3
             WHERE u.target = ?1
             ORDER BY u.key",
        )?;
        let mut rows = q.query([target])?;
        let mut files = Vec::new();
        while let Some(r) = rows.next()? {
            let source: Option<i64> = r.get(5)?;
            let found = match (source.and_then(|s| roots.get(&s)), r.get::<_, Option<String>>(6)?) {
                (Some((at, root)), Some(path)) => {
                    within(root, Path::new(&path)).map(|inside| -> rusqlite::Result<NotedFound> {
                        Ok(NotedFound {
                            source: *at,
                            path: inside,
                            kind: r.get(7)?,
                            mime: r.get(8)?,
                            date: r.get(9)?,
                            date_source: r.get(10)?,
                            album: r.get(11)?,
                            folder_date: r.get(12)?,
                            width: r.get(13)?,
                            height: r.get(14)?,
                            junk: r.get(15)?,
                        })
                    })
                }
                _ => None,
            }
            .transpose()?;
            let preview = match (
                r.get::<_, Option<bool>>(18)?,
                r.get::<_, Option<u32>>(19)?,
                r.get::<_, Option<u32>>(20)?,
                r.get::<_, Option<String>>(21)?,
            ) {
                (Some(true), Some(width), Some(height), Some(blurhash)) => Some(NotedPreview {
                    width,
                    height,
                    blurhash,
                }),
                _ => None,
            };
            let video = match (
                r.get::<_, Option<bool>>(22)?,
                r.get::<_, Option<f64>>(23)?,
                r.get::<_, Option<u32>>(24)?,
                r.get::<_, Option<u32>>(25)?,
            ) {
                (Some(true), Some(duration), Some(width), Some(height)) => Some(NotedVideo {
                    duration,
                    width,
                    height,
                    poster: r.get::<_, Option<bool>>(26)?.unwrap_or(false),
                    mp4: r.get::<_, Option<bool>>(27)?.unwrap_or(false),
                }),
                _ => None,
            };
            files.push(NotedFile {
                key: r.get(0)?,
                bytes: u64::try_from(r.get::<_, i64>(1)?).unwrap_or(0),
                gone: r.get(2)?,
                proved_at: r.get(3)?,
                blake3: r.get(4)?,
                quick: r.get(16)?,
                left_out: r.get::<_, Option<bool>>(17)? == Some(false),
                found,
                preview,
                video,
            });
        }
        Ok(Noted {
            settings,
            sources,
            files,
        })
    }

    /// Builds this catalog's knowledge of a backup from its notes, for the
    /// destination it read them from (`target`), as when the backup is
    /// opened on a new computer. Each source becomes one waiting to be
    /// connected again, kept at a place inside `aside` that is never made,
    /// so nothing is ever looked for where it was on the old computer. All
    /// of it is written in one go, or none of it.
    pub fn take_notes(&mut self, noted: &Noted, target: &str, aside: &Path, at: i64) -> Result<()> {
        let tx = self.conn.transaction()?;
        for (name, value) in &noted.settings {
            if SETTINGS_THAT_TRAVEL.contains(&name.as_str()) {
                tx.execute(
                    "INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)",
                    params![name, value],
                )?;
            }
        }
        let mut ids = Vec::new();
        for (n, source) in noted.sources.iter().enumerate() {
            let root = aside.join((n + 1).to_string());
            let root_text = path_text(&root);
            tx.execute(
                "INSERT INTO sources (name, root, other_files, scanned_at, kind, place) VALUES (?1, ?2, 0, ?3, 'folder', NULL)",
                params![source.name, root_text, source.looked_at.min(at)],
            )?;
            ids.push((tx.last_insert_rowid(), root));
            let before = Elsewhere {
                was_at: source.was_at.clone(),
                kind: source.kind.clone(),
            };
            let keep = |key: String, value: Option<&String>| -> rusqlite::Result<()> {
                if let Some(value) = value {
                    tx.execute(
                        "INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)",
                        params![key, value],
                    )?;
                }
                Ok(())
            };
            keep(
                elsewhere_key(&root_text),
                Some(&serde_json::to_string(&before).unwrap_or_default()),
            )?;
            keep(wants_key(&root_text), source.wants.as_ref())?;
            keep(arrange_key(&root_text), source.arrange.as_ref())?;
            keep(left_out_key(&root_text), source.left_out_endings.as_ref())?;
        }
        {
            let mut item = tx.prepare(
                "INSERT INTO items (source_id, path, kind, mime, bytes, blake3, date, date_source,
                                    album, folder_date, width, height, junk, hashed, quick)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, 1, ?14)",
            )?;
            for file in &noted.files {
                let (Some(found), Some(blake3)) = (&file.found, &file.blake3) else {
                    continue;
                };
                let Some((id, root)) = ids.get(found.source) else {
                    continue;
                };
                let mut path = root.clone();
                path.extend(found.path.split('/').filter(|part| !part.is_empty()));
                item.execute(params![
                    id,
                    path_text(&path),
                    found.kind,
                    found.mime,
                    to_i64(file.bytes),
                    blake3,
                    found.date,
                    found.date_source,
                    found.album,
                    found.folder_date,
                    found.width,
                    found.height,
                    found.junk,
                    file.quick,
                ])?;
            }
        }
        add_noted_in(&tx, &noted.files, target)?;
        tx.commit()?;
        Ok(())
    }

    /// Adds to what this catalog knows is at `target` the files in notes
    /// there that it does not know of yet: when another computer has backed
    /// up to the same place, or the backup is being opened from its notes.
    /// What is known already is kept as it is, except that a later proof by
    /// reading a copy back is taken. Answers how many files were new.
    pub fn add_noted(&self, files: &[NotedFile], target: &str) -> Result<usize> {
        let tx = self.conn.unchecked_transaction()?;
        let added = add_noted_in(&tx, files, target)?;
        tx.commit()?;
        Ok(added)
    }

    /// Where a source opened from notes was on the computer that wrote
    /// them, while it waits to be connected here. `None` for any other.
    pub fn elsewhere(&self, root: &Path) -> Result<Option<Elsewhere>> {
        Ok(self
            .setting(&elsewhere_key(&path_text(root)))?
            .and_then(|text| serde_json::from_str(&text).ok()))
    }

    /// The source is connected on this computer now: it is no longer
    /// waiting.
    pub fn connected_here(&self, root: &Path) -> Result<()> {
        self.conn
            .execute("DELETE FROM settings WHERE key = ?1", [elsewhere_key(&path_text(root))])?;
        Ok(())
    }
}

/// [`Catalog::add_noted`], within a change already begun.
fn add_noted_in(tx: &rusqlite::Connection, files: &[NotedFile], target: &str) -> Result<usize> {
    let mut added = 0;
    {
        let mut known = tx.prepare("SELECT bytes, proved_at FROM uploaded WHERE target = ?1 AND key = ?2")?;
        let mut sent =
            tx.prepare("INSERT INTO uploaded (target, key, bytes, gone, proved_at) VALUES (?1, ?2, ?3, ?4, ?5)")?;
        let mut proved = tx.prepare("UPDATE uploaded SET proved_at = ?3 WHERE target = ?1 AND key = ?2")?;
        let mut placed =
            tx.prepare("INSERT OR IGNORE INTO collected (blake3, path, bytes, here) VALUES (?1, ?2, ?3, 0)")?;
        let mut chosen = tx.prepare("INSERT OR IGNORE INTO choices (blake3, keep) VALUES (?1, ?2)")?;
        let mut previewed = tx.prepare(
            "INSERT OR IGNORE INTO renditions (blake3, ok, width, height, blurhash) VALUES (?1, 1, ?2, ?3, ?4)",
        )?;
        let mut video = tx.prepare(
            "INSERT OR IGNORE INTO videos (blake3, ok, duration, width, height, poster, mp4)
                 VALUES (?1, 1, ?2, ?3, ?4, ?5, ?6)",
        )?;
        for file in files {
            let here: Option<(i64, Option<i64>)> = known
                .query_row(params![target, file.key], |r| Ok((r.get(0)?, r.get(1)?)))
                .optional()?;
            match here {
                Some((bytes, was)) => {
                    let later = file.proved_at.filter(|p| was.is_none_or(|w| *p > w));
                    if let (true, Some(at)) = (bytes == to_i64(file.bytes), later) {
                        proved.execute(params![target, file.key, at])?;
                    }
                    continue;
                }
                None => {
                    sent.execute(params![target, file.key, to_i64(file.bytes), file.gone, file.proved_at])?;
                    added += 1;
                }
            }
            let Some(blake3) = &file.blake3 else { continue };
            if file.key.starts_with("originals/") {
                placed.execute(params![blake3, file.key, to_i64(file.bytes)])?;
                chosen.execute(params![blake3, !file.left_out])?;
            }
            if let Some(p) = &file.preview {
                previewed.execute(params![blake3, p.width, p.height, p.blurhash])?;
            }
            if let Some(v) = &file.video {
                video.execute(params![blake3, v.duration, v.width, v.height, v.poster, v.mp4])?;
            }
        }
    }
    Ok(added)
}

/// Where `path` is inside `root`, folders joined by `/`, judged the way
/// this computer, which wrote both, reads its own paths.
fn within(root: &Path, path: &Path) -> Option<String> {
    let inside = path.strip_prefix(root).ok()?;
    let parts: Vec<String> = inside
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    (!parts.is_empty()).then(|| parts.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{RenditionState, Seen, VideoState};
    use keevoro_core::{DateGuess, DateSource, FolderHints, MediaKind, PartialDate};
    use keevoro_scan::{FoundItem, ScanReport};

    const TARGET: &str = "folder:/media/you/Family Backup";

    fn found(root: &Path, inside: &str, hash: char, kind: MediaKind) -> FoundItem {
        let mut path = root.to_path_buf();
        path.extend(inside.split('/'));
        FoundItem {
            path,
            kind,
            mime: "image/jpeg".into(),
            bytes: 2_483_312,
            blake3: Some(hash.to_string().repeat(64)),
            hashed: true,
            date: Some(DateGuess {
                date: PartialDate::parse("2009-07-14T21:03:11").unwrap(),
                source: DateSource::Exif,
            }),
            folder: FolderHints {
                album: vec!["Holidays".into()],
                date: None,
                kept: false,
            },
            width: Some(3264),
            height: Some(2448),
            junk: None,
            modified: Some(1),
        }
    }

    /// A backup of two files from one source, both sent and one proved,
    /// with previews of the photo and what is known of the video.
    fn backed_up() -> Catalog {
        let mut catalog = Catalog::in_memory().unwrap();
        let root = PathBuf::from("/home/you/Pictures");
        let report = ScanReport {
            root: root.clone(),
            items: vec![
                found(&root, "Holidays/IMG_0048.jpg", 'a', MediaKind::Photo),
                found(&root, "Holidays/IMG_0049.mov", 'b', MediaKind::Video),
            ],
            skipped: vec![],
            unreadable: vec![],
            other_files: 0,
            online_only: vec![],
            online_only_bytes: vec![],
            stopped: false,
            gone: false,
            unchanged: 0,
        };
        let source = catalog.save_scan("Family photos", &report, 1_795_219_200).unwrap();
        catalog.set_source_wants(source, Some(keevoro_core::Wants::default())).unwrap();
        catalog.set_setting("wants", "[\"photos\"]").unwrap();
        catalog.set_setting("index_key:x", "never travels").unwrap();
        for (hash, key) in [('a', "originals/2009/07/IMG_0048.jpg"), ('b', "originals/2009/07/IMG_0049.mov")] {
            let blake3 = hash.to_string().repeat(64);
            catalog.set_choice(&blake3, true).unwrap();
            catalog.mark_placed(&blake3, key, 2_483_312).unwrap();
            catalog.mark_uploaded(TARGET, key, 2_483_312).unwrap();
        }
        catalog.mark_uploaded(TARGET, "renditions/thumb/aaaa.webp", 4_210).unwrap();
        catalog
            .set_proved(TARGET, &["originals/2009/07/IMG_0048.jpg".to_string()], 1_795_300_000)
            .unwrap();
        catalog
            .set_rendition(
                &"a".repeat(64),
                &RenditionState::Made {
                    width: 3264,
                    height: 2448,
                    blurhash: "LEHV6nWB2yk8".into(),
                },
            )
            .unwrap();
        catalog
            .set_video(
                &"b".repeat(64),
                &VideoState::Known {
                    duration_secs: 12.5,
                    width: 1920,
                    height: 1080,
                    poster: true,
                    mp4: false,
                    conversion_problem: None,
                },
            )
            .unwrap();
        catalog
    }

    #[test]
    fn the_notes_say_every_file_sent_there_with_where_it_was_found_and_nothing_machine_bound_as_fact() {
        let noted = backed_up().notes_for(TARGET).unwrap();
        assert_eq!(noted.files.len(), 3, "both originals and the preview sent");
        let photo = &noted.files[0];
        assert_eq!(photo.key, "originals/2009/07/IMG_0048.jpg");
        assert_eq!(photo.blake3.as_deref(), Some("a".repeat(64).as_str()));
        assert_eq!(photo.proved_at, Some(1_795_300_000));
        let from = photo.found.as_ref().unwrap();
        assert_eq!((from.source, from.path.as_str()), (0, "Holidays/IMG_0048.jpg"));
        assert!(photo.preview.is_some());
        assert!(noted.files[1].video.as_ref().is_some_and(|v| v.poster));
        assert_eq!(noted.files[2].key, "renditions/thumb/aaaa.webp");
        assert!(noted.files[2].blake3.is_none());
        assert_eq!(noted.sources.len(), 1);
        assert_eq!(noted.sources[0].was_at, path_text(Path::new("/home/you/Pictures")));
        assert!(noted.sources[0].wants.is_some());
        assert_eq!(noted.settings.get("wants").map(String::as_str), Some("[\"photos\"]"));
        assert!(!noted.settings.keys().any(|k| k.starts_with("index_key")));
        assert!(noted.notes_for_nothing_else());
    }

    impl Noted {
        /// Only the settings that travel, and no setting keyed by a place.
        fn notes_for_nothing_else(&self) -> bool {
            self.settings.keys().all(|k| SETTINGS_THAT_TRAVEL.contains(&k.as_str()))
        }
    }

    #[test]
    fn a_catalog_built_from_the_notes_knows_every_file_is_there_and_waits_for_its_sources_to_be_connected() {
        let noted = backed_up().notes_for(TARGET).unwrap();
        let mut fresh = Catalog::in_memory().unwrap();
        let aside = PathBuf::from("/home/you/Keevoro/backup-2/.keevoro/elsewhere");
        fresh.take_notes(&noted, TARGET, &aside, 1_795_400_000).unwrap();

        let sources = fresh.sources().unwrap();
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].name, "Family photos");
        assert_eq!(sources[0].items, 2);
        assert!(sources[0].root.starts_with(&aside), "never where it was on the old computer");
        let before = fresh.elsewhere(&sources[0].root).unwrap().unwrap();
        assert_eq!(before.was_at, path_text(Path::new("/home/you/Pictures")));
        assert!(fresh.source_wants(sources[0].id).unwrap().is_some());

        for (hash, key) in [('a', "originals/2009/07/IMG_0048.jpg"), ('b', "originals/2009/07/IMG_0049.mov")] {
            let blake3 = hash.to_string().repeat(64);
            assert_eq!(fresh.choice(&blake3).unwrap(), Some(true));
            assert_eq!(fresh.collected_path(&blake3).unwrap().as_deref(), Some(key));
            assert!(fresh.is_uploaded(TARGET, key).unwrap());
        }
        assert!(fresh.is_uploaded(TARGET, "renditions/thumb/aaaa.webp").unwrap());
        assert_eq!(fresh.proved_since(TARGET, 1).unwrap().len(), 1);
        assert!(matches!(fresh.rendition(&"a".repeat(64)).unwrap(), Some(RenditionState::Made { .. })));
        assert!(matches!(fresh.video(&"b".repeat(64)).unwrap(), Some(VideoState::Known { poster: true, .. })));
        assert_eq!(fresh.wants().unwrap(), backed_up().wants().unwrap());

        // Its own notes say the same, with the old place still as history.
        let again = fresh.notes_for(TARGET).unwrap();
        assert_eq!(again.files, noted.files);
        assert_eq!(again.sources, noted.sources);
        // Connected here, it is just a source again.
        fresh.connected_here(&sources[0].root).unwrap();
        assert!(fresh.elsewhere(&sources[0].root).unwrap().is_none());
    }

    #[test]
    fn what_another_computer_noted_is_added_and_nothing_known_here_is_lost_or_overwritten() {
        let here = backed_up();
        let mut theirs = here.notes_for(TARGET).unwrap().files;
        // They proved the photo later, and sent a file this computer never saw.
        theirs[0].proved_at = Some(1_795_500_000);
        theirs[1].gone = 1;
        theirs.push(NotedFile {
            key: "originals/2009/07/IMG_0050.jpg".into(),
            bytes: 1_000,
            blake3: Some("c".repeat(64)),
            quick: None,
            gone: 0,
            proved_at: None,
            left_out: false,
            found: None,
            preview: None,
            video: None,
        });
        assert_eq!(here.add_noted(&theirs, TARGET).unwrap(), 1);
        assert!(here.is_uploaded(TARGET, "originals/2009/07/IMG_0050.jpg").unwrap());
        assert_eq!(
            here.collected_path(&"c".repeat(64)).unwrap().as_deref(),
            Some("originals/2009/07/IMG_0050.jpg")
        );
        assert_eq!(here.proved_since(TARGET, 1_795_400_000).unwrap().len(), 1, "the later proof is taken");
        assert_eq!(
            here.sent_state(TARGET, "originals/2009/07/IMG_0049.mov").unwrap(),
            Some(Seen::There),
            "what this computer last saw is kept"
        );
        assert_eq!(here.add_noted(&theirs, TARGET).unwrap(), 0, "adding again adds nothing");
        assert_eq!(here.notes_for(TARGET).unwrap().files.len(), 4);
    }
}
