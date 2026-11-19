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
    // 2: which files are safely in the library, so gathering can resume.
    "CREATE TABLE collected (
         blake3 TEXT PRIMARY KEY,
         path   TEXT NOT NULL,
         bytes  INTEGER NOT NULL
     );",
    // 3: previews made, what has reached each storage target, and settings.
    "CREATE TABLE renditions (
         blake3   TEXT PRIMARY KEY,
         ok       INTEGER NOT NULL,
         width    INTEGER,
         height   INTEGER,
         blurhash TEXT,
         reason   TEXT
     );
     CREATE TABLE uploaded (
         target TEXT NOT NULL,
         key    TEXT NOT NULL,
         bytes  INTEGER NOT NULL,
         PRIMARY KEY (target, key)
     );
     CREATE TABLE multipart (
         target    TEXT NOT NULL,
         key       TEXT NOT NULL,
         upload_id TEXT NOT NULL,
         PRIMARY KEY (target, key)
     );
     CREATE TABLE settings (
         key   TEXT PRIMARY KEY,
         value TEXT NOT NULL
     );",
    // 4: what is known about each video and the copies made of it.
    "CREATE TABLE videos (
         blake3   TEXT PRIMARY KEY,
         ok       INTEGER NOT NULL,
         duration REAL,
         width    INTEGER,
         height   INTEGER,
         poster   INTEGER NOT NULL DEFAULT 0,
         mp4      INTEGER NOT NULL DEFAULT 0,
         reason   TEXT
     );",
    // 5: whether an item's fingerprint covers the whole file (1) or is a
    // quick one from a fast search (0). Everything found before this was
    // whole. `quick` keeps the quick fingerprint once the whole one is known.
    "ALTER TABLE items ADD COLUMN hashed INTEGER NOT NULL DEFAULT 1;
     ALTER TABLE items ADD COLUMN quick TEXT;
     CREATE INDEX items_unhashed ON items(hashed) WHERE hashed = 0;",
    // 6: a kept file's place (its folder and name) is agreed once and is the
    // same wherever it is copied to. `here` says whether a copy is in this
    // library's own folder; a file can have a place without one, when it
    // is sent straight from its source to other storage.
    "ALTER TABLE collected ADD COLUMN here INTEGER NOT NULL DEFAULT 1;",
    // 7: camera raw files were being set aside as "tiny images", because
    // the size in a raw file's header is that of its small built-in
    // preview. Those are photos; put them back, and forget the wrong size.
    "UPDATE items SET junk = NULL, width = NULL, height = NULL
     WHERE (lower(path) LIKE '%.dng' OR lower(path) LIKE '%.cr2' OR lower(path) LIKE '%.cr3' OR lower(path) LIKE '%.crw' OR lower(path) LIKE '%.nef' OR lower(path) LIKE '%.nrw' OR lower(path) LIKE '%.arw' OR lower(path) LIKE '%.srf' OR lower(path) LIKE '%.sr2' OR lower(path) LIKE '%.orf' OR lower(path) LIKE '%.rw2' OR lower(path) LIKE '%.raf' OR lower(path) LIKE '%.pef' OR lower(path) LIKE '%.srw' OR lower(path) LIKE '%.x3f' OR lower(path) LIKE '%.3fr' OR lower(path) LIKE '%.erf' OR lower(path) LIKE '%.kdc' OR lower(path) LIKE '%.mrw' OR lower(path) LIKE '%.raw' OR lower(path) LIKE '%.iiq' OR lower(path) LIKE '%.dcr' OR lower(path) LIKE '%.mef' OR lower(path) LIKE '%.mos' OR lower(path) LIKE '%.rwl');",
    // 8: looking a file up by its place, and asking whether a place has
    // reached any destination, were done by reading every row. With ten
    // thousand files that is a hundred million comparisons before a backup
    // can start. These make each an instant lookup.
    "CREATE INDEX IF NOT EXISTS collected_by_path ON collected(path);
     CREATE INDEX IF NOT EXISTS uploaded_by_key ON uploaded(key);",
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
    /// The size of everything found on it, in bytes.
    pub bytes: u64,
    /// Seconds since 1970, UTC.
    pub scanned_at: i64,
}

/// One kind of file found on a source, by the ending of its name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileType {
    /// Lower case, without the dot: `jpg`, `dng`, `mp4`. Empty for files
    /// whose names have no ending.
    pub ending: String,
    pub files: u64,
    pub bytes: u64,
    /// False when the person has left this kind out of this source.
    pub included: bool,
}

fn left_out_key(root: &str) -> String {
    format!("left_out_types:{root}")
}

/// The ending of a file's name, lower case, without the dot.
fn ending_of(path: &Path) -> String {
    path.extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

/// A file known only by a quick fingerprint so far.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unhashed {
    pub path: PathBuf,
    pub quick: String,
    pub bytes: u64,
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
        // The app reads and writes from more than one thread (a search, and
        // the checking that follows it), so wait for a turn, do not fail.
        conn.busy_timeout(std::time::Duration::from_secs(30))?;
        // Write-ahead mode: a screen can read counts while a backup writes,
        // and each small write no longer waits for the disk twice over.
        // "Normal" syncing is safe from corruption in this mode; at worst a
        // power cut loses the last moment's notes, which a backup works
        // out again. Where the mode is not available (some network folders)
        // SQLite keeps its old one, so the answer is not checked.
        let _ = conn.query_row("PRAGMA journal_mode = WAL", [], |_| Ok(()));
        let _ = conn.pragma_update(None, "synchronous", "NORMAL");
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
        // Reading a whole file is the slow part of knowing it, so whole-file
        // fingerprints worked out for this source before are kept: a file
        // still at the same place with the same quick fingerprint has not
        // changed.
        let mut known: std::collections::HashMap<(String, String), String> = std::collections::HashMap::new();
        {
            let mut q = tx.prepare(
                "SELECT i.path, i.quick, i.blake3 FROM items i JOIN sources s ON s.id = i.source_id
                 WHERE s.root = ?1 AND i.hashed = 1 AND i.quick IS NOT NULL",
            )?;
            let mut rows = q.query([&root])?;
            while let Some(r) = rows.next()? {
                known.insert((r.get(0)?, r.get(1)?), r.get(2)?);
            }
        }
        tx.execute("DELETE FROM sources WHERE root = ?1", [&root])?;
        tx.execute(
            "INSERT INTO sources (name, root, other_files, scanned_at) VALUES (?1, ?2, ?3, ?4)",
            params![name, root, to_i64(report.other_files), scanned_at],
        )?;
        let source = tx.last_insert_rowid();
        {
            let mut insert = tx.prepare(
                "INSERT INTO items (source_id, path, kind, mime, bytes, blake3, date, date_source,
                                    album, folder_date, width, height, junk, hashed, quick)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
            )?;
            for i in &report.items {
                let path = path_text(&i.path);
                // A quick fingerprint is remembered beside the whole one, so
                // the next search of this source can recognise the file.
                let quick = if i.hashed { None } else { i.blake3.clone() };
                let whole = quick
                    .as_ref()
                    .and_then(|q| known.get(&(path.clone(), q.clone())).cloned());
                let hashed = i.hashed || whole.is_some();
                insert.execute(params![
                    source,
                    path,
                    to_name(&i.kind)?,
                    i.mime,
                    to_i64(i.bytes),
                    whole.or_else(|| i.blake3.clone()),
                    i.date.map(|d| d.date.to_string()),
                    i.date.map(|d| d.source.as_str()),
                    serde_json::to_string(&i.folder.album).map_err(damaged)?,
                    i.folder.date.map(|d| d.to_string()),
                    i.width,
                    i.height,
                    i.junk.map(JunkReason::as_str),
                    hashed,
                    quick,
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
            "SELECT s.id, s.name, s.root, s.scanned_at,
                    (SELECT COUNT(*) FROM items i WHERE i.source_id = s.id),
                    (SELECT COALESCE(SUM(i.bytes), 0) FROM items i WHERE i.source_id = s.id)
             FROM sources s ORDER BY s.id",
        )?;
        let rows = q.query_map([], |r| {
            Ok(Source {
                id: r.get(0)?,
                name: r.get(1)?,
                root: PathBuf::from(r.get::<_, String>(2)?),
                scanned_at: r.get(3)?,
                items: u64::try_from(r.get::<_, i64>(4)?).unwrap_or(0),
                bytes: u64::try_from(r.get::<_, i64>(5)?).unwrap_or(0),
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Every file whose fingerprint is still a quick one: its path, that
    /// quick fingerprint and its size, in the order found.
    pub fn unhashed(&self) -> Result<Vec<Unhashed>> {
        let mut q = self
            .conn
            .prepare("SELECT path, blake3, bytes FROM items WHERE hashed = 0 AND blake3 IS NOT NULL ORDER BY id")?;
        let rows = q.query_map([], |r| {
            Ok(Unhashed {
                path: PathBuf::from(r.get::<_, String>(0)?),
                quick: r.get(1)?,
                bytes: u64::try_from(r.get::<_, i64>(2)?).unwrap_or(0),
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Records the whole-file fingerprint of a file that had only a quick
    /// one. A keep choice made while only the quick one was known follows
    /// the file to its real fingerprint, so nothing the user decided is
    /// lost, and two different files that happened to share a quick
    /// fingerprint each keep the choice.
    pub fn set_whole_hash(&self, path: &Path, quick: &str, whole: &str) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        let changed = tx.execute(
            "UPDATE items SET blake3 = ?1, quick = ?2, hashed = 1 WHERE path = ?3 AND blake3 = ?2 AND hashed = 0",
            params![whole, quick, path_text(path)],
        )?;
        if changed > 0 {
            tx.execute(
                "INSERT INTO choices (blake3, keep) SELECT ?1, keep FROM choices WHERE blake3 = ?2
                 ON CONFLICT(blake3) DO NOTHING",
                params![whole, quick],
            )?;
            // Once no file goes by the quick fingerprint, its choice is spent.
            tx.execute(
                "DELETE FROM choices WHERE blake3 = ?1
                 AND NOT EXISTS (SELECT 1 FROM items WHERE blake3 = ?1 AND hashed = 0)",
                [quick],
            )?;
            // A file sent while only its quick fingerprint was known was
            // given its place under that fingerprint. The place follows the
            // file to its real one, so it is still known to be there. This
            // waits until no file goes by the quick fingerprint any more.
            let still_used: bool = tx
                .query_row(
                    "SELECT EXISTS (SELECT 1 FROM items WHERE blake3 = ?1 AND hashed = 0)",
                    [quick],
                    |r| r.get(0),
                )
                .unwrap_or(true);
            if !still_used {
                tx.execute(
                    "UPDATE OR IGNORE collected SET blake3 = ?1 WHERE blake3 = ?2",
                    params![whole, quick],
                )?;
                tx.execute("DELETE FROM collected WHERE blake3 = ?1", [quick])?;
                // Previews made under the quick fingerprint are named after
                // it; they are forgotten, and made again under the real one.
                tx.execute("DELETE FROM renditions WHERE blake3 = ?1", [quick])?;
                tx.execute("DELETE FROM videos WHERE blake3 = ?1", [quick])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Forgets a source and everything found on it. Keep choices stay.
    /// Returns false when there was no such source.
    pub fn forget_source(&mut self, id: i64) -> Result<bool> {
        Ok(self.conn.execute("DELETE FROM sources WHERE id = ?1", [id])? > 0)
    }

    /// The kinds of file found on a source, most numerous first, and
    /// whether each kind is being included.
    pub fn file_types(&self, source: i64) -> Result<Vec<FileType>> {
        let root: Option<String> = self
            .conn
            .query_row("SELECT root FROM sources WHERE id = ?1", [source], |r| r.get(0))
            .optional()?;
        let Some(root) = root else { return Ok(Vec::new()) };
        let left_out = self.left_out_types(&root)?;
        let mut kinds: std::collections::BTreeMap<String, (u64, u64)> = std::collections::BTreeMap::new();
        for item in self.items_of(source)? {
            let kind = kinds.entry(ending_of(&item.path)).or_default();
            kind.0 += 1;
            kind.1 += item.bytes;
        }
        let mut list: Vec<FileType> = kinds
            .into_iter()
            .map(|(ending, (files, bytes))| FileType {
                included: !left_out.contains(&ending),
                ending,
                files,
                bytes,
            })
            .collect();
        list.sort_by(|a, b| b.files.cmp(&a.files).then_with(|| a.ending.cmp(&b.ending)));
        Ok(list)
    }

    /// Includes or leaves out every file with this ending on one source.
    /// The choice belongs to the source's place, so it is still there after
    /// the source is looked through again.
    pub fn set_type_included(&self, source: i64, ending: &str, included: bool) -> Result<()> {
        let root: Option<String> = self
            .conn
            .query_row("SELECT root FROM sources WHERE id = ?1", [source], |r| r.get(0))
            .optional()?;
        let Some(root) = root else { return Ok(()) };
        let mut left_out = self.left_out_types(&root)?;
        let ending = ending.trim_start_matches('.').to_lowercase();
        if included {
            left_out.remove(&ending);
        } else {
            left_out.insert(ending);
        }
        let list: Vec<&String> = left_out.iter().collect();
        self.set_setting(&left_out_key(&root), &serde_json::to_string(&list).map_err(damaged)?)
    }

    fn left_out_types(&self, root: &str) -> Result<std::collections::BTreeSet<String>> {
        Ok(match self.setting(&left_out_key(root))? {
            Some(text) => serde_json::from_str(&text).unwrap_or_default(),
            None => std::collections::BTreeSet::new(),
        })
    }

    /// Everything known about every source, in the same shape a fresh scan
    /// gives, so the rest of keevoro does not care which one it is given.
    /// Kinds of file the person has left out of a source are not in it, so
    /// to everything else they are simply not there.
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
                    root: PathBuf::from(&root),
                    items: {
                        let left_out = self.left_out_types(&root)?;
                        let mut items = self.items_of(id)?;
                        if !left_out.is_empty() {
                            items.retain(|item| !left_out.contains(&ending_of(&item.path)));
                        }
                        items
                    },
                    skipped: self.skipped_of(id)?,
                    unreadable: self.unreadable_of(id)?,
                    other_files: u64::try_from(other).unwrap_or(0),
                    stopped: false,
                })
            })
            .collect()
    }

    fn items_of(&self, source: i64) -> Result<Vec<FoundItem>> {
        let mut q = self.conn.prepare(
            "SELECT path, kind, mime, bytes, blake3, date, date_source, album, folder_date, width, height, junk,
                    hashed
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
                hashed: r.get(12)?,
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

    /// Where on a source the file with this content id can be read from,
    /// with its kind and full hash. `id` is the start of the hash; the
    /// app uses the first 16 characters. `None` when nothing matches.
    pub fn find_item(&self, id: &str) -> Result<Option<(PathBuf, keevoro_core::MediaKind, String)>> {
        // Only hex is expected; anything else cannot match and must not be
        // allowed to act as a LIKE wildcard.
        if id.is_empty() || !id.chars().all(|c| c.is_ascii_hexdigit()) {
            return Ok(None);
        }
        let row = self
            .conn
            .query_row(
                // A picture already on screen may still be known by the id
                // made from its quick fingerprint.
                "SELECT path, kind, blake3 FROM items
                 WHERE blake3 LIKE ?1 || '%' OR quick LIKE ?1 || '%' ORDER BY id LIMIT 1",
                [id.to_lowercase()],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)),
            )
            .optional()?;
        match row {
            Some((path, kind, hash)) => Ok(Some((PathBuf::from(path), from_name(&kind)?, hash))),
            None => Ok(None),
        }
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

    /// The content hashes of every file the user has explicitly said to
    /// keep. Used to bring wrongly set-aside junk back into view.
    pub fn kept_hashes(&self) -> Result<std::collections::HashSet<String>> {
        let mut q = self.conn.prepare("SELECT blake3 FROM choices WHERE keep = 1")?;
        let rows = q.query_map([], |r| r.get::<_, String>(0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
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

impl Catalog {
    /// Records that a file has been copied into the library and verified.
    /// `path` is relative to the library folder, with `/` between folders.
    pub fn mark_collected(&self, blake3: &str, path: &str, bytes: u64) -> Result<()> {
        self.conn.execute(
            "INSERT INTO collected (blake3, path, bytes, here) VALUES (?1, ?2, ?3, 1)
             ON CONFLICT(blake3) DO UPDATE SET path = excluded.path, bytes = excluded.bytes, here = 1",
            params![blake3, path, to_i64(bytes)],
        )?;
        Ok(())
    }

    /// Agrees the place a kept file will have wherever it is copied to,
    /// without saying a copy exists in this library's folder. Does nothing
    /// if the file already has a place.
    pub fn mark_placed(&self, blake3: &str, path: &str, bytes: u64) -> Result<()> {
        self.conn.execute(
            "INSERT INTO collected (blake3, path, bytes, here) VALUES (?1, ?2, ?3, 0)
             ON CONFLICT(blake3) DO NOTHING",
            params![blake3, path, to_i64(bytes)],
        )?;
        Ok(())
    }

    /// Runs `work` as one change to the catalog: all of it is kept, or, if
    /// it fails, none of it. It is also far quicker for many small writes,
    /// which otherwise each wait for the disk on their own.
    pub fn in_one_go<T, E: From<CatalogError>>(
        &self,
        work: impl FnOnce() -> std::result::Result<T, E>,
    ) -> std::result::Result<T, E> {
        let tx = self.conn.unchecked_transaction().map_err(CatalogError::from)?;
        let done = work()?;
        tx.commit().map_err(CatalogError::from)?;
        Ok(done)
    }

    /// Is a copy of this file in this library's own folder?
    pub fn is_here(&self, blake3: &str) -> Result<bool> {
        Ok(self
            .conn
            .query_row("SELECT here FROM collected WHERE blake3 = ?1", [blake3], |r| {
                r.get::<_, bool>(0)
            })
            .optional()?
            .unwrap_or(false))
    }

    /// Is this place already taken by some file?
    pub fn place_taken(&self, path: &str) -> Result<bool> {
        Ok(self
            .conn
            .query_row("SELECT 1 FROM collected WHERE path = ?1", [path], |_| Ok(()))
            .optional()?
            .is_some())
    }

    /// Has anything been stored under this name in any storage?
    pub fn stored_anywhere(&self, key: &str) -> Result<bool> {
        Ok(self
            .conn
            .query_row("SELECT 1 FROM uploaded WHERE key = ?1 LIMIT 1", [key], |_| Ok(()))
            .optional()?
            .is_some())
    }

    /// Where in the library a file with this content was put, if it has
    /// been gathered already.
    pub fn collected_path(&self, blake3: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row("SELECT path FROM collected WHERE blake3 = ?1", [blake3], |r| r.get(0))
            .optional()?)
    }

    /// Every file gathered into the library: its content hash, its
    /// library-relative path and its size, in path order.
    pub fn collected(&self) -> Result<Vec<Collected>> {
        let mut q = self
            .conn
            .prepare("SELECT blake3, path, bytes, here FROM collected ORDER BY path")?;
        let rows = q.query_map([], |r| {
            Ok(Collected {
                blake3: r.get(0)?,
                path: r.get(1)?,
                bytes: u64::try_from(r.get::<_, i64>(2)?).unwrap_or(0),
                here: r.get(3)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// How many files are in the library, and their total size in bytes.
    pub fn collected_totals(&self) -> Result<(u64, u64)> {
        let (n, bytes): (i64, i64) = self.conn.query_row(
            "SELECT COUNT(*), COALESCE(SUM(bytes), 0) FROM collected WHERE here = 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        Ok((u64::try_from(n).unwrap_or(0), u64::try_from(bytes).unwrap_or(0)))
    }
}

/// One file in the library, as recorded when it was gathered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Collected {
    pub blake3: String,
    /// Relative to the library folder, with `/` between folders.
    pub path: String,
    pub bytes: u64,
    /// True when a copy is in this library's own folder.
    pub here: bool,
}

/// What is known about one video and the copies made of it.
#[derive(Debug, Clone, PartialEq)]
pub enum VideoState {
    /// The video could be read.
    Known {
        duration_secs: f64,
        /// Size as shown, after any rotation.
        width: u32,
        height: u32,
        /// A poster picture exists.
        poster: bool,
        /// A converted MP4 exists.
        mp4: bool,
        /// Why conversion was tried and given up on, so it is not tried
        /// again on every run.
        conversion_problem: Option<String>,
    },
    /// The video could not be read at all, with the plain reason.
    Unreadable { reason: String },
}

impl Catalog {
    pub fn set_video(&self, blake3: &str, state: &VideoState) -> Result<()> {
        match state {
            VideoState::Known {
                duration_secs,
                width,
                height,
                poster,
                mp4,
                conversion_problem,
            } => self.conn.execute(
                "INSERT OR REPLACE INTO videos (blake3, ok, duration, width, height, poster, mp4, reason)
                 VALUES (?1, 1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![blake3, duration_secs, width, height, poster, mp4, conversion_problem],
            )?,
            VideoState::Unreadable { reason } => self.conn.execute(
                "INSERT OR REPLACE INTO videos (blake3, ok, reason) VALUES (?1, 0, ?2)",
                params![blake3, reason],
            )?,
        };
        Ok(())
    }

    pub fn video(&self, blake3: &str) -> Result<Option<VideoState>> {
        let row = self
            .conn
            .query_row(
                "SELECT ok, duration, width, height, poster, mp4, reason FROM videos WHERE blake3 = ?1",
                [blake3],
                |r| {
                    Ok((
                        r.get::<_, bool>(0)?,
                        r.get::<_, Option<f64>>(1)?,
                        r.get::<_, Option<u32>>(2)?,
                        r.get::<_, Option<u32>>(3)?,
                        r.get::<_, bool>(4)?,
                        r.get::<_, bool>(5)?,
                        r.get::<_, Option<String>>(6)?,
                    ))
                },
            )
            .optional()?;
        Ok(row.map(|row| match row {
            (true, Some(duration_secs), Some(width), Some(height), poster, mp4, conversion_problem) => {
                VideoState::Known {
                    duration_secs,
                    width,
                    height,
                    poster,
                    mp4,
                    conversion_problem,
                }
            }
            (_, _, _, _, _, _, reason) => VideoState::Unreadable {
                reason: reason.unwrap_or_default(),
            },
        }))
    }
}

/// What is known about the preview copies of one file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenditionState {
    /// Thumbnail and display copy exist. The size is the original's,
    /// turned upright.
    Made { width: u32, height: u32, blurhash: String },
    /// No preview could be made, with the plain reason. Remembered so the
    /// same file is not tried again on every run.
    NoPreview { reason: String },
}

impl Catalog {
    pub fn set_rendition(&self, blake3: &str, state: &RenditionState) -> Result<()> {
        let (ok, width, height, blurhash, reason) = match state {
            RenditionState::Made {
                width,
                height,
                blurhash,
            } => (true, Some(*width), Some(*height), Some(blurhash.as_str()), None),
            RenditionState::NoPreview { reason } => (false, None, None, None, Some(reason.as_str())),
        };
        self.conn.execute(
            "INSERT OR REPLACE INTO renditions (blake3, ok, width, height, blurhash, reason)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![blake3, ok, width, height, blurhash, reason],
        )?;
        Ok(())
    }

    /// Forgets every "couldn't be previewed" and "couldn't be converted"
    /// note, so those files are tried again. Worth doing after Keevoro
    /// learns a new format or ffmpeg is installed. Returns how many.
    pub fn forget_preview_problems(&self) -> Result<usize> {
        let photos = self.conn.execute("DELETE FROM renditions WHERE ok = 0", [])?;
        let videos = self
            .conn
            .execute("DELETE FROM videos WHERE ok = 0 OR reason IS NOT NULL", [])?;
        Ok(photos + videos)
    }

    pub fn rendition(&self, blake3: &str) -> Result<Option<RenditionState>> {
        let row = self
            .conn
            .query_row(
                "SELECT ok, width, height, blurhash, reason FROM renditions WHERE blake3 = ?1",
                [blake3],
                |r| {
                    Ok((
                        r.get::<_, bool>(0)?,
                        r.get::<_, Option<u32>>(1)?,
                        r.get::<_, Option<u32>>(2)?,
                        r.get::<_, Option<String>>(3)?,
                        r.get::<_, Option<String>>(4)?,
                    ))
                },
            )
            .optional()?;
        Ok(row.map(|row| match row {
            (true, Some(width), Some(height), Some(blurhash), _) => RenditionState::Made {
                width,
                height,
                blurhash,
            },
            (_, _, _, _, reason) => RenditionState::NoPreview {
                reason: reason.unwrap_or_default(),
            },
        }))
    }

    /// Records that `key` is safely stored on a storage target. `target` is
    /// any stable name for the place, such as a bucket address.
    pub fn mark_uploaded(&self, target: &str, key: &str, bytes: u64) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO uploaded (target, key, bytes) VALUES (?1, ?2, ?3)",
            params![target, key, to_i64(bytes)],
        )?;
        self.conn.execute(
            "DELETE FROM multipart WHERE target = ?1 AND key = ?2",
            params![target, key],
        )?;
        Ok(())
    }

    pub fn is_uploaded(&self, target: &str, key: &str) -> Result<bool> {
        Ok(self
            .conn
            .query_row(
                "SELECT 1 FROM uploaded WHERE target = ?1 AND key = ?2",
                params![target, key],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }

    /// Forgets what was uploaded to a target, so everything is checked
    /// against the storage again. Nothing stored is touched.
    pub fn forget_uploads(&self, target: &str) -> Result<()> {
        self.conn.execute("DELETE FROM uploaded WHERE target = ?1", [target])?;
        self.conn.execute("DELETE FROM multipart WHERE target = ?1", [target])?;
        Ok(())
    }

    /// Remembers a large upload in progress so it can carry on after a
    /// restart instead of starting again.
    pub fn set_multipart(&self, target: &str, key: &str, upload_id: Option<&str>) -> Result<()> {
        match upload_id {
            Some(id) => self.conn.execute(
                "INSERT OR REPLACE INTO multipart (target, key, upload_id) VALUES (?1, ?2, ?3)",
                params![target, key, id],
            )?,
            None => self.conn.execute(
                "DELETE FROM multipart WHERE target = ?1 AND key = ?2",
                params![target, key],
            )?,
        };
        Ok(())
    }

    pub fn multipart(&self, target: &str, key: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row(
                "SELECT upload_id FROM multipart WHERE target = ?1 AND key = ?2",
                params![target, key],
                |r| r.get(0),
            )
            .optional()?)
    }

    /// Small named values, such as the private name of a published index.
    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn setting(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| r.get(0))
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
