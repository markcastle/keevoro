//! The catalog: keevoro's memory between runs.
//!
//! Scanning an old drive can take an hour, and the user may unplug it and
//! come back next week. The catalog is a single SQLite file inside the
//! library (`.keevoro/catalog.sqlite`) that remembers what was found on each
//! source and which files the user chose to keep, so nothing has to be done
//! twice.
//!
//! It is working state, not the archive. It never leaves the machine, and it
//! can be rebuilt by scanning again. Nothing here ever touches a source.
//!
//! Two things are worth knowing before changing it:
//!
//! - **Choices are stored against a file's content hash, not its path.** The
//!   same photo on three drives is one decision, and a choice survives a
//!   drive being re-scanned under a different letter.
//! - **The schema only moves forward.** Add a new step to [`MIGRATIONS`];
//!   never edit an old one, because real catalogs have already run it.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

use keevoro_core::{DateGuess, DateSource, FolderHints, JunkReason, PartialDate};
use keevoro_scan::{FoundItem, ScanReport, SkippedFolder, Unreadable};
use rusqlite::{params, Connection, OptionalExtension};
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::path::{Path, PathBuf};

/// Where the catalog lives inside a library folder.
pub const CATALOG_PATH: &str = ".keevoro/catalog.sqlite";

/// Schema steps, oldest first. A catalog's `user_version` is how many it has
/// run.
const MIGRATIONS: &[&str] = &[
    // 1: sources, what was found on them, and the user's keep choices.
    "CREATE TABLE sources (
         id          INTEGER PRIMARY KEY,
         name        TEXT NOT NULL,
         root        TEXT NOT NULL UNIQUE,
         other_files INTEGER NOT NULL DEFAULT 0,
         scanned_at  INTEGER NOT NULL
     );
     CREATE TABLE items (
         id          INTEGER PRIMARY KEY,
         source_id   INTEGER NOT NULL REFERENCES sources(id) ON DELETE CASCADE,
         path        TEXT NOT NULL,
         kind        TEXT NOT NULL,
         mime        TEXT NOT NULL,
         bytes       INTEGER NOT NULL,
         blake3      TEXT,
         date        TEXT,
         date_source TEXT,
         album       TEXT NOT NULL,
         folder_date TEXT,
         width       INTEGER,
         height      INTEGER,
         junk        TEXT
     );
     CREATE INDEX items_by_source ON items(source_id);
     CREATE INDEX items_by_hash ON items(blake3);
     CREATE TABLE skipped (
         source_id INTEGER NOT NULL REFERENCES sources(id) ON DELETE CASCADE,
         path      TEXT NOT NULL,
         reason    TEXT NOT NULL
     );
     CREATE TABLE unreadable (
         source_id INTEGER NOT NULL REFERENCES sources(id) ON DELETE CASCADE,
         path      TEXT NOT NULL,
         error     TEXT NOT NULL
     );
     CREATE TABLE choices (
         blake3 TEXT PRIMARY KEY,
         keep   INTEGER NOT NULL
     );",
];

#[derive(Debug, thiserror::Error)]
pub enum CatalogError {
    #[error("Couldn't open the library's catalog: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("Couldn't create the library folder: {0}")]
    Io(#[from] std::io::Error),
    #[error("This library was made by a newer version of Keevoro. Please update Keevoro to open it.")]
    NewerVersion,
    #[error("The library's catalog is damaged ({0}). Scanning your drives again will rebuild it.")]
    Damaged(String),
}

pub type Result<T> = std::result::Result<T, CatalogError>;

/// A drive or folder the user has scanned.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Source {
    pub id: i64,
    /// The friendly name, like "Dad's old Dell".
    pub name: String,
    pub root: PathBuf,
    pub items: u64,
    /// Seconds since 1970, UTC.
    pub scanned_at: i64,
}

pub struct Catalog {
    conn: Connection,
}

impl Catalog {
    /// Opens the catalog of the library at `library`, creating it if needed.
    pub fn open_in_library(library: &Path) -> Result<Self> {
        Self::open(&library.join(CATALOG_PATH))
    }

    /// Opens or creates a catalog file and brings its schema up to date.
    pub fn open(file: &Path) -> Result<Self> {
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir)?;
        }
        Self::prepare(Connection::open(file)?)
    }

    /// A catalog that lives only in memory, for tests.
    pub fn in_memory() -> Result<Self> {
        Self::prepare(Connection::open_in_memory()?)
    }

    fn prepare(mut conn: Connection) -> Result<Self> {
        conn.pragma_update(None, "foreign_keys", true)?;
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        let done = usize::try_from(version).unwrap_or(usize::MAX);
        if done > MIGRATIONS.len() {
            return Err(CatalogError::NewerVersion);
        }
        for (i, step) in MIGRATIONS.iter().enumerate().skip(done) {
            // Each step and its version bump land together or not at all, so
            // a crash mid-upgrade cannot leave a half-built schema.
            let tx = conn.transaction()?;
            tx.execute_batch(step)?;
            tx.pragma_update(None, "user_version", i64::try_from(i + 1).unwrap_or(i64::MAX))?;
            tx.commit()?;
        }
        Ok(Self { conn })
    }

    /// Records the result of scanning one source. Scanning the same place
    /// again replaces what was known about it; the user's keep choices are
    /// untouched because they belong to the files, not the scan.
    pub fn save_scan(&mut self, name: &str, report: &ScanReport, scanned_at: i64) -> Result<i64> {
        let tx = self.conn.transaction()?;
        let root = path_text(&report.root);
        tx.execute("DELETE FROM sources WHERE root = ?1", [&root])?;
        tx.execute(
            "INSERT INTO sources (name, root, other_files, scanned_at) VALUES (?1, ?2, ?3, ?4)",
            params![name, root, to_i64(report.other_files), scanned_at],
        )?;
        let source = tx.last_insert_rowid();
        {
            let mut insert = tx.prepare(
                "INSERT INTO items (source_id, path, kind, mime, bytes, blake3, date, date_source,
                                    album, folder_date, width, height, junk)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            )?;
            for i in &report.items {
                insert.execute(params![
                    source,
                    path_text(&i.path),
                    to_name(&i.kind)?,
                    i.mime,
                    to_i64(i.bytes),
                    i.blake3,
                    i.date.map(|d| d.date.to_string()),
                    i.date.map(|d| d.source.as_str()),
                    serde_json::to_string(&i.folder.album).map_err(damaged)?,
                    i.folder.date.map(|d| d.to_string()),
                    i.width,
                    i.height,
                    i.junk.map(JunkReason::as_str),
                ])?;
            }
            let mut skipped = tx.prepare("INSERT INTO skipped (source_id, path, reason) VALUES (?1, ?2, ?3)")?;
            for s in &report.skipped {
                skipped.execute(params![source, path_text(&s.path), to_name(&s.reason)?])?;
            }
            let mut unreadable = tx.prepare("INSERT INTO unreadable (source_id, path, error) VALUES (?1, ?2, ?3)")?;
            for u in &report.unreadable {
                unreadable.execute(params![source, path_text(&u.path), u.error])?;
            }
        }
        tx.commit()?;
        Ok(source)
    }

    /// Every source scanned so far, in the order they were first added.
    pub fn sources(&self) -> Result<Vec<Source>> {
        let mut q = self.conn.prepare(
            "SELECT s.id, s.name, s.root, s.scanned_at, (SELECT COUNT(*) FROM items i WHERE i.source_id = s.id)
             FROM sources s ORDER BY s.id",
        )?;
        let rows = q.query_map([], |r| {
            Ok(Source {
                id: r.get(0)?,
                name: r.get(1)?,
                root: PathBuf::from(r.get::<_, String>(2)?),
                scanned_at: r.get(3)?,
                items: u64::try_from(r.get::<_, i64>(4)?).unwrap_or(0),
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Forgets a source and everything found on it. Keep choices stay.
    /// Returns false when there was no such source.
    pub fn forget_source(&mut self, id: i64) -> Result<bool> {
        Ok(self.conn.execute("DELETE FROM sources WHERE id = ?1", [id])? > 0)
    }

    /// Everything known about every source, in the same shape a fresh scan
    /// gives, so the rest of keevoro does not care which one it is given.
    pub fn load_reports(&self) -> Result<Vec<ScanReport>> {
        let mut sources = self
            .conn
            .prepare("SELECT id, root, other_files FROM sources ORDER BY id")?;
        let heads = sources
            .query_map([], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        heads
            .into_iter()
            .map(|(id, root, other)| {
                Ok(ScanReport {
                    root: PathBuf::from(root),
                    items: self.items_of(id)?,
                    skipped: self.skipped_of(id)?,
                    unreadable: self.unreadable_of(id)?,
                    other_files: u64::try_from(other).unwrap_or(0),
                })
            })
            .collect()
    }

    fn items_of(&self, source: i64) -> Result<Vec<FoundItem>> {
        let mut q = self.conn.prepare(
            "SELECT path, kind, mime, bytes, blake3, date, date_source, album, folder_date, width, height, junk
             FROM items WHERE source_id = ?1 ORDER BY id",
        )?;
        let mut rows = q.query([source])?;
        let mut items = Vec::new();
        while let Some(r) = rows.next()? {
            let date: Option<String> = r.get(5)?;
            let date_source: Option<String> = r.get(6)?;
            let date = match (date, date_source) {
                (Some(d), Some(s)) => Some(DateGuess {
                    date: PartialDate::parse(&d).ok_or_else(|| damaged(format!("date {d}")))?,
                    source: DateSource::parse(&s).ok_or_else(|| damaged(format!("date source {s}")))?,
                }),
                _ => None,
            };
            let folder_date: Option<String> = r.get(8)?;
            let junk: Option<String> = r.get(11)?;
            items.push(FoundItem {
                path: PathBuf::from(r.get::<_, String>(0)?),
                kind: from_name(&r.get::<_, String>(1)?)?,
                mime: r.get(2)?,
                bytes: u64::try_from(r.get::<_, i64>(3)?).unwrap_or(0),
                blake3: r.get(4)?,
                date,
                folder: FolderHints {
                    album: serde_json::from_str(&r.get::<_, String>(7)?).map_err(damaged)?,
                    date: match folder_date {
                        Some(d) => Some(PartialDate::parse(&d).ok_or_else(|| damaged(format!("date {d}")))?),
                        None => None,
                    },
                },
                width: r.get(9)?,
                height: r.get(10)?,
                junk: match junk {
                    Some(j) => Some(JunkReason::parse(&j).ok_or_else(|| damaged(format!("junk reason {j}")))?),
                    None => None,
                },
            });
        }
        Ok(items)
    }

    fn skipped_of(&self, source: i64) -> Result<Vec<SkippedFolder>> {
        let mut q = self
            .conn
            .prepare("SELECT path, reason FROM skipped WHERE source_id = ?1 ORDER BY rowid")?;
        let mut rows = q.query([source])?;
        let mut out = Vec::new();
        while let Some(r) = rows.next()? {
            out.push(SkippedFolder {
                path: PathBuf::from(r.get::<_, String>(0)?),
                reason: from_name(&r.get::<_, String>(1)?)?,
            });
        }
        Ok(out)
    }

    fn unreadable_of(&self, source: i64) -> Result<Vec<Unreadable>> {
        let mut q = self
            .conn
            .prepare("SELECT path, error FROM unreadable WHERE source_id = ?1 ORDER BY rowid")?;
        let rows = q.query_map([source], |r| {
            Ok(Unreadable {
                path: PathBuf::from(r.get::<_, String>(0)?),
                error: r.get(1)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Remembers whether the user wants a file kept. `blake3` is the file's
    /// full content hash.
    pub fn set_choice(&self, blake3: &str, keep: bool) -> Result<()> {
        self.conn.execute(
            "INSERT INTO choices (blake3, keep) VALUES (?1, ?2)
             ON CONFLICT(blake3) DO UPDATE SET keep = excluded.keep",
            params![blake3, keep],
        )?;
        Ok(())
    }

    /// Puts a file back to "not decided yet".
    pub fn clear_choice(&self, blake3: &str) -> Result<()> {
        self.conn.execute("DELETE FROM choices WHERE blake3 = ?1", [blake3])?;
        Ok(())
    }

    /// The user's decision about a file, or `None` if they have not made one
    /// (in which case the default applies: keep, unless it looks like junk).
    pub fn choice(&self, blake3: &str) -> Result<Option<bool>> {
        Ok(self
            .conn
            .query_row("SELECT keep FROM choices WHERE blake3 = ?1", [blake3], |r| r.get(0))
            .optional()?)
    }
}

/// Paths are stored as text. A name that is not valid Unicode (rare, and
/// only possible on Linux and macOS) is stored with the odd bytes replaced;
/// the file is still listed, and scanning again finds the real name.
fn path_text(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

/// SQLite integers are signed. Nothing keevoro counts gets near the limit.
fn to_i64(n: u64) -> i64 {
    i64::try_from(n).unwrap_or(i64::MAX)
}

fn damaged(what: impl ToString) -> CatalogError {
    CatalogError::Damaged(what.to_string())
}

/// Stores small enums under the same lowercase names the JSON output uses.
fn to_name<T: Serialize>(value: &T) -> Result<String> {
    match serde_json::to_value(value).map_err(damaged)? {
        serde_json::Value::String(s) => Ok(s),
        other => Err(damaged(other)),
    }
}

fn from_name<T: DeserializeOwned>(name: &str) -> Result<T> {
    serde_json::from_value(serde_json::Value::String(name.to_string())).map_err(|_| damaged(format!("unknown {name}")))
}

#[cfg(test)]
mod tests;
