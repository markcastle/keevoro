//! Keevoro's notes about a backup, kept at each destination, so the
//! backup can be picked up again on a new computer, after a reinstall, or
//! when the record on this computer is lost.
//!
//! **What.** One file, [`NOTES`], beside "What is in this backup.txt": a
//! gzip-compressed text file of JSON lines. The first line says what the
//! file is, which version of the format it is in, which Keevoro wrote it
//! and when, and the backup's name and choices; then a line per source, as
//! history; then a line per file Keevoro sent there (its place, size,
//! fingerprints, dates, kind, previews, when it was last proved); and last
//! a line with how many lines came before it and a BLAKE3 fingerprint of
//! them, so a file cut short or damaged is known for what it is. Ordinary
//! tools can read it (any gzip reader, then any text editor), it needs no
//! database to be read over a network share, and it is written whole in
//! one go to a folder and to cloud storage alike. It never holds keys,
//! anything from the keychain, the log or report tokens.
//!
//! **How it is written** ([`keep_at`]): after a backup, and after copies
//! are proved, for each destination reached. Keevoro made the file, so it
//! may be rewritten, as previews and the index are; originals are never
//! touched. A store writes a whole file under a new name and moves it into
//! place (a folder), or in one request (cloud storage), so the file is
//! never half-written. It is read back and its fingerprint checked. The
//! notes there before, when they could be read, are kept first as one
//! earlier generation, [`EARLIER`]. When nothing changed since they were
//! last written, nothing is written at all. A problem writing them never
//! fails a backup: it is said, and tried again next time.
//!
//! **Two computers, one destination.** Keevoro remembers the size and tag
//! (or time) of the notes it last wrote at each destination. Notes there
//! that differ were written by something else: another computer, or this
//! backup opened elsewhere. They are read, and every file they know of
//! that this computer does not is added to what it knows (the union, by
//! place), before the notes are written again. Nothing known here is
//! replaced, except that a later proof is taken. So knowledge of a file
//! that is there is never lost, however the two computers take turns.
//! Notes written by a newer Keevoro are left as they are.

use anyhow::{anyhow, bail, Result};
use keevoro_catalog::{Catalog, Noted, NotedFile, NotedSource, NOTES_ID};
use keevoro_storage::{ByteSource, ObjectKey, ObjectStore, PutOptions};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

/// The notes' name at a destination: plain words, beside the guide.
pub const NOTES: &str = "Keevoro notes.jsonl.gz";

/// The notes as they were before the last were written.
pub const EARLIER: &str = "Keevoro notes, earlier.jsonl.gz";

/// The version of the format this Keevoro writes. A later Keevoro that
/// changes what a line means writes a higher number, and reads this one.
pub const FORMAT: u32 = 1;

/// The most a notes file may hold once unpacked: far more than a million
/// files need, and a guard against a file that only claims to be notes.
const MOST_UNPACKED: u64 = 2 * 1024 * 1024 * 1024;

/// What the first line says the file is, for anyone who opens it.
const WHAT: &str = "Keevoro's notes about the backup in this folder: what was backed up, where each file went, \
                    its size and fingerprint, so Keevoro on another computer can pick this backup up again. \
                    Your photos and videos do not need this file: they are ordinary files in \"originals\".";

/// The first line.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct About {
    /// The version of the format.
    pub keevoro_notes: u32,
    #[serde(default)]
    pub what: String,
    /// "Keevoro 0.50.0"
    #[serde(default)]
    pub made_by: String,
    /// When these notes were written, in seconds since 1970: the end of
    /// the last backup or proof there.
    #[serde(default)]
    pub written_at: i64,
    /// The backup's name.
    #[serde(default)]
    pub name: String,
    /// Originals, and their size together.
    #[serde(default)]
    pub files: usize,
    #[serde(default)]
    pub bytes: u64,
    /// When a copy there was last read back in full and found exact.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_proved: Option<i64>,
    /// The backup's settings that travel, as the catalog keeps them.
    #[serde(default)]
    pub settings: BTreeMap<String, String>,
}

/// The last line.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct End {
    lines: usize,
    blake3: String,
}

/// One line after the first.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
enum Line<'a> {
    Source(&'a NotedSource),
    File(&'a NotedFile),
    End(End),
}

/// Notes read from a destination.
#[derive(Debug, Clone, PartialEq)]
pub struct Notes {
    pub about: About,
    pub noted: Noted,
}

impl Notes {
    /// The backup's own name in its notes, when it has one.
    pub fn id(&self) -> Option<&str> {
        self.about.settings.get(NOTES_ID).map(String::as_str)
    }
}

/// Why notes could not be used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unusable {
    /// A newer Keevoro wrote them, in a format this one does not know.
    Newer { made_by: String },
    /// They are cut short, damaged, or not Keevoro's notes at all.
    Damaged(String),
}

impl std::fmt::Display for Unusable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Unusable::Newer { made_by } => write!(f, "{}", newer_words(made_by)),
            Unusable::Damaged(why) => write!(f, "Keevoro's notes there are damaged ({why})"),
        }
    }
}

impl std::error::Error for Unusable {}

/// What is said of notes a newer Keevoro wrote.
pub fn newer_words(made_by: &str) -> String {
    let who = if made_by.trim().is_empty() {
        "a newer Keevoro"
    } else {
        made_by
    };
    format!(
        "This backup's notes were made by a newer Keevoro ({who}) than this one ({}). Update Keevoro first, then \
         open it again.",
        this_keevoro()
    )
}

/// "Keevoro 0.49.0"
pub fn this_keevoro() -> String {
    format!("Keevoro {}", env!("CARGO_PKG_VERSION"))
}

/// The notes as a file, and a fingerprint of what they say apart from
/// when they were written, which tells whether anything has changed.
pub fn encode(noted: &Noted, name: &str, written_at: i64) -> Result<(Vec<u8>, String)> {
    let originals = noted.files.iter().filter(|f| f.key.starts_with("originals/"));
    let about = About {
        keevoro_notes: FORMAT,
        what: WHAT.to_string(),
        made_by: this_keevoro(),
        written_at,
        name: name.to_string(),
        files: originals.clone().count(),
        bytes: originals.map(|f| f.bytes).sum(),
        last_proved: noted.files.iter().filter_map(|f| f.proved_at).max(),
        settings: noted.settings.clone(),
    };
    let mut text = Vec::with_capacity(noted.files.len() * 300 + 1024);
    serde_json::to_writer(&mut text, &about)?;
    text.push(b'\n');
    let mut lines = 1;
    let mut content = blake3::Hasher::new();
    content.update(name.as_bytes());
    content.update(serde_json::to_string(&noted.settings)?.as_bytes());
    for line in noted
        .sources
        .iter()
        .map(Line::Source)
        .chain(noted.files.iter().map(Line::File))
    {
        let start = text.len();
        serde_json::to_writer(&mut text, &line)?;
        text.push(b'\n');
        content.update(&text[start..]);
        lines += 1;
    }
    let end = Line::End(End {
        lines,
        blake3: blake3::hash(&text).to_hex().to_string(),
    });
    serde_json::to_writer(&mut text, &end)?;
    text.push(b'\n');
    let mut packed = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    packed.write_all(&text)?;
    Ok((packed.finish()?, content.finalize().to_hex().to_string()))
}

/// Reads notes from their file, checking every line arrived as written.
pub fn decode(file: &[u8]) -> std::result::Result<Notes, Unusable> {
    let damaged = |why: &str| Unusable::Damaged(why.to_string());
    let mut text = Vec::new();
    flate2::read::GzDecoder::new(file)
        .take(MOST_UNPACKED)
        .read_to_end(&mut text)
        .map_err(|_| damaged("it could not be unpacked"))?;
    let mut rest: &[u8] = &text;
    let mut first = true;
    let mut lines = 0;
    let mut about: Option<About> = None;
    let mut noted = Noted::default();
    loop {
        let Some(at) = rest.iter().position(|b| *b == b'\n') else {
            return Err(damaged("it is cut short"));
        };
        let line = &rest[..at];
        let read_so_far = text.len() - rest.len();
        rest = &rest[at + 1..];
        if first {
            first = false;
            let value: serde_json::Value =
                serde_json::from_slice(line).map_err(|_| damaged("its first line is not Keevoro's"))?;
            let format = value
                .get("keevoroNotes")
                .and_then(serde_json::Value::as_u64)
                .ok_or_else(|| damaged("its first line is not Keevoro's"))?;
            if format > u64::from(FORMAT) {
                let made_by = value.get("madeBy").and_then(|m| m.as_str()).unwrap_or_default();
                return Err(Unusable::Newer {
                    made_by: made_by.to_string(),
                });
            }
            about = Some(upgrade(value)?);
            lines += 1;
            continue;
        }
        let value: serde_json::Value = serde_json::from_slice(line).map_err(|_| damaged("a line is damaged"))?;
        let Some((kind, inside)) = value.as_object().and_then(|o| o.iter().next()) else {
            return Err(damaged("a line is damaged"));
        };
        match kind.as_str() {
            "end" => {
                let end: End =
                    serde_json::from_value(inside.clone()).map_err(|_| damaged("its last line is damaged"))?;
                if end.lines != lines || end.blake3 != blake3::hash(&text[..read_so_far]).to_hex().as_str() {
                    return Err(damaged("its fingerprint does not match"));
                }
                break;
            }
            "source" => noted
                .sources
                .push(serde_json::from_value(inside.clone()).map_err(|_| damaged("a source is damaged"))?),
            "file" => noted
                .files
                .push(serde_json::from_value(inside.clone()).map_err(|_| damaged("a file's line is damaged"))?),
            // A line of a kind added later in the same format is passed
            // over: it adds to what is known, never changes it.
            _ => {}
        }
        lines += 1;
    }
    let about = about.ok_or_else(|| damaged("it is empty"))?;
    noted.settings = about.settings.clone();
    Ok(Notes { about, noted })
}

/// The first line in this version's form. Each later format adds an arm
/// here that turns the one before it into its own, so notes written by
/// any older Keevoro are read.
fn upgrade(value: serde_json::Value) -> std::result::Result<About, Unusable> {
    serde_json::from_value(value).map_err(|_| Unusable::Damaged("its first line is damaged".into()))
}

/// What Keevoro remembers about the notes it last wrote at a destination:
/// how they looked from outside, so notes written by anyone else are
/// noticed, and a fingerprint of what they said, so the same notes are not
/// written twice.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct Written {
    content: String,
    bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    etag: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    modified: Option<i64>,
}

fn setting(target: &str) -> String {
    format!("notes:{target}")
}

/// What keeping the notes at a destination did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kept {
    /// Nothing had changed, so nothing was written.
    Unchanged,
    /// Written. `merged` files were learnt from notes another computer
    /// wrote there; `damaged_before` says the notes there before could not
    /// be read, so the earlier generation was left as it was.
    Written { merged: usize, damaged_before: bool },
    /// A newer Keevoro's notes are there, and were left alone.
    LeftForNewer { made_by: String },
}

/// What a backup says of keeping its notes at `place` ("the folder
/// F:\Family Backup"), when there is anything worth saying.
pub fn kept_words(kept: &Kept, place: &str) -> Option<String> {
    match kept {
        Kept::Unchanged | Kept::Written { merged: 0, .. } => None,
        Kept::Written { merged, .. } => Some(format!(
            "Another computer has also backed up to {place}. Keevoro now knows of the {} it sent there, and keeps \
             notes of both.",
            crate::summary::count(*merged, "file")
        )),
        Kept::LeftForNewer { made_by } => Some(format!(
            "Keevoro's notes at {place} were written by a newer Keevoro ({made_by}), so this one has left them as \
             they are. Update Keevoro to keep them up to date."
        )),
    }
}

/// What a backup says when its notes could not be kept at `place`.
pub fn not_kept_words(place: &str, why: &str) -> String {
    format!(
        "Couldn't keep Keevoro's notes at {place} up to date ({why}). Your files are backed up all the same; \
         Keevoro will try again next time."
    )
}

/// Writes Keevoro's notes about this backup at a destination (`target`
/// names it in the catalog), learning first from any notes there written
/// by someone else. See the module's comment for the rules.
pub async fn keep_at(store: &dyn ObjectStore, catalog: &Catalog, target: &str, name: &str, now: i64) -> Result<Kept> {
    let notes_key = ObjectKey::new(NOTES)?;
    let remembered: Option<Written> = catalog
        .setting(&setting(target))?
        .and_then(|text| serde_json::from_str(&text).ok());
    let there = store.head(&notes_key).await?;
    let ours = match (&there, &remembered) {
        (Some(info), Some(was)) => {
            info.bytes == was.bytes && info.etag == was.etag && (info.etag.is_some() || info.modified == was.modified)
        }
        (None, _) => true,
        (Some(_), None) => false,
    };
    // Notes written by anyone else are read, and what they know is learnt.
    let mut before: Option<Vec<u8>> = None;
    let mut merged = 0;
    let mut damaged_before = false;
    if there.is_some() && !ours {
        let file = store.get(&notes_key).await?;
        match decode(&file) {
            Ok(theirs) => {
                merged = catalog.add_noted(&theirs.noted.files, target)?;
                before = Some(file);
            }
            Err(Unusable::Newer { made_by }) => return Ok(Kept::LeftForNewer { made_by }),
            Err(Unusable::Damaged(_)) => damaged_before = true,
        }
    }
    // The backup's own name in its notes, made the first time.
    if catalog.setting(NOTES_ID)?.is_none() {
        let mut random = [0u8; 16];
        getrandom::fill(&mut random).map_err(|e| anyhow!("Couldn't get random numbers ({e})"))?;
        catalog.set_setting(NOTES_ID, &random.iter().map(|b| format!("{b:02x}")).collect::<String>())?;
    }
    let noted = catalog.notes_for(target)?;
    let (file, content) = encode(&noted, name, now)?;
    if ours && there.is_some() && remembered.as_ref().is_some_and(|was| was.content == content) {
        return Ok(Kept::Unchanged);
    }
    // The notes there before become the earlier generation, when they are
    // whole: damaged ones never replace a good earlier copy.
    if before.is_none() && there.is_some() && !damaged_before {
        let file = store.get(&notes_key).await?;
        match decode(&file) {
            Ok(_) => before = Some(file),
            Err(_) => damaged_before = true,
        }
    }
    let plain = PutOptions {
        content_type: Some("application/gzip".into()),
        ..PutOptions::default()
    };
    if let Some(before) = before {
        store
            .put(&ObjectKey::new(EARLIER)?, ByteSource::Bytes(before), &plain)
            .await?;
    }
    store.put(&notes_key, ByteSource::Bytes(file.clone()), &plain).await?;
    // Read back, and only then counted as written.
    let back = store.get(&notes_key).await?;
    if blake3::hash(&back) != blake3::hash(&file) {
        bail!("they read back differently from how they were written");
    }
    let info = store
        .head(&notes_key)
        .await?
        .ok_or_else(|| anyhow!("they were not there when Keevoro looked"))?;
    let written = Written {
        content,
        bytes: info.bytes,
        etag: info.etag,
        modified: info.modified,
    };
    catalog.set_setting(&setting(target), &serde_json::to_string(&written)?)?;
    Ok(Kept::Written { merged, damaged_before })
}

/// The catalog setting that holds the backup's name, as the person last
/// called it, for its notes: the name lives in Keevoro's settings, which
/// the engine does not read, so whoever starts a backup writes it here.
pub const NAME: &str = "name";

/// Keeps the notes at one destination after a backup, a check, or putting
/// it right there, and says to `out` anything worth saying. A problem is
/// said plainly and never stops what came before it: the notes are tried
/// again next time.
pub fn keep_after(
    out: &mut dyn Write,
    catalog: &Catalog,
    to: &crate::connect::Saved,
    world: &crate::connect::World<'_>,
) {
    let place = to.describe();
    let kept = (|| -> Result<Kept> {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
        let destination = to.open(&mut Vec::new(), world)?;
        let (store, target, _) = crate::send::open(&mut Vec::new(), &runtime, destination)?;
        let name = catalog.setting(NAME)?.unwrap_or_default();
        runtime.block_on(keep_at(store.as_ref(), catalog, &target, &name, crate::confirm::now()))
    })();
    let said = match kept {
        Ok(kept) => kept_words(&kept, &place),
        Err(e) => Some(not_kept_words(&place, &why(&e))),
    };
    if let Some(said) = said {
        let _ = writeln!(out, "{said}");
    }
}

/// Where the sources of a backup opened from its notes are kept in its
/// record until they are connected again: inside its own folder, at a
/// place that is never made, so nothing is ever looked for there.
pub fn elsewhere_folder(home: &Path) -> PathBuf {
    home.join(".keevoro").join("elsewhere")
}

/// Builds a new backup's record (its catalog, with its home at `home`)
/// from notes read at the destination `target` names, as when it is
/// opened on a new computer. Its sources wait to be connected again.
pub fn open_into(catalog: &mut Catalog, home: &Path, notes: &Notes, target: &str) -> Result<()> {
    catalog.take_notes(&notes.noted, target, &elsewhere_folder(home), crate::confirm::now())?;
    catalog.set_setting(NAME, &notes.about.name)?;
    Ok(())
}

/// What is said of a source that waits to be connected on this computer.
pub fn waiting_words(was_at: &str) -> String {
    format!(
        "Keevoro does not know where these photos live on this computer. On the computer this backup came from          they were in {was_at}. Connect them again to carry on backing them up: nothing already backed up is sent          again."
    )
}

/// Connects a source of a backup opened from its notes to where its files
/// are on this computer (`folder`), keeping everything found on it and
/// every choice made for it. Only the person can say where that is:
/// nothing is ever assumed. It still has to be looked through there.
pub fn connect_again(catalog: &mut Catalog, source: i64, folder: &Path) -> Result<()> {
    let Some(root) = catalog.sources()?.into_iter().find(|s| s.id == source).map(|s| s.root) else {
        bail!("This backup has no such source.");
    };
    if catalog.elsewhere(&root)?.is_none() {
        bail!("This source is already connected on this computer.");
    }
    if !folder.is_dir() {
        bail!(
            "Keevoro can't find the folder {}. Is it plugged in?",
            crate::friendly_path(folder)
        );
    }
    if catalog
        .sources()?
        .iter()
        .any(|s| keevoro_core::places::same_place(&s.root, folder))
    {
        bail!(
            "{} is already a source of this backup. Choose the folder where these photos are.",
            crate::friendly_path(folder)
        );
    }
    catalog.connected_here(&root)?;
    catalog.move_source(source, folder)?;
    Ok(())
}

/// Why something went wrong, in the words a person reads, without a full
/// stop, to go inside a sentence.
fn why(e: &anyhow::Error) -> String {
    let said = match e.downcast_ref::<keevoro_storage::StorageError>() {
        Some(storage) => keevoro_publish::plain_reason(storage),
        None => format!("{e:#}"),
    };
    let said = said.trim().trim_end_matches('.');
    let mut letters = said.chars();
    match letters.next() {
        Some(first) => first.to_lowercase().chain(letters).collect(),
        None => "something went wrong".into(),
    }
}

/// What was found when Keevoro looked for a backup's notes.
#[derive(Debug, Clone, PartialEq)]
pub enum Found {
    /// Notes Keevoro can use. `from_earlier` when the newest were damaged
    /// and the generation before them was used instead.
    Notes { notes: Box<Notes>, from_earlier: bool },
    /// Notes a newer Keevoro wrote. Never guessed at.
    Newer { made_by: String },
    /// A Keevoro backup is there, but neither its notes nor the earlier
    /// copy can be used (or it is from before notes were kept): its notes
    /// can only be made again from the files themselves.
    NoUsableNotes { damaged: bool },
    /// Nothing of a Keevoro backup is there.
    Nothing,
}

/// Looks for a backup's notes at a place not yet any backup's destination,
/// only reading: a folder is never made, nothing is written.
pub fn look_for(destination: crate::send::Destination) -> Result<Found> {
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
    let store: Box<dyn ObjectStore> = match destination {
        crate::send::Destination::Folder(folder) => {
            if !folder.is_dir() {
                bail!(
                    "Keevoro can't find the folder {}. Is it plugged in?",
                    crate::friendly_path(&folder)
                );
            }
            Box::new(keevoro_storage_folder::FolderStore::look_at(&folder)?)
        }
        crate::send::Destination::Bucket {
            preset,
            bucket,
            access_key_id,
            secret_access_key,
        } => Box::new(
            keevoro_storage_s3::S3Store::connect(&keevoro_storage_s3::S3Settings {
                preset,
                bucket,
                access_key_id,
                secret_access_key,
            })
            .map_err(|e| anyhow!("{}.", keevoro_publish::plain_reason(&e)))?,
        ),
    };
    runtime
        .block_on(find_at(store.as_ref()))
        .map_err(|e| match e.downcast_ref::<keevoro_storage::StorageError>() {
            Some(storage) => anyhow!("Couldn't look there. {}.", keevoro_publish::plain_reason(storage)),
            None => e,
        })
}

/// Looks for a backup's notes at a destination, falling back to the
/// earlier generation when the newest cannot be used.
pub async fn find_at(store: &dyn ObjectStore) -> Result<Found> {
    let mut damaged = false;
    for (name, from_earlier) in [(NOTES, false), (EARLIER, true)] {
        let key = ObjectKey::new(name)?;
        if store.head(&key).await?.is_none() {
            continue;
        }
        match decode(&store.get(&key).await?) {
            Ok(notes) => {
                return Ok(Found::Notes {
                    notes: Box::new(notes),
                    from_earlier,
                })
            }
            // A newer Keevoro's notes are refused outright, even with an
            // older copy beside them: that copy is out of date.
            Err(Unusable::Newer { made_by }) => return Ok(Found::Newer { made_by }),
            Err(Unusable::Damaged(_)) => damaged = true,
        }
    }
    let a_backup = damaged
        || store.head(&ObjectKey::new(crate::layout::GUIDE_NAME)?).await?.is_some()
        || !store.list_page("originals/", None).await?.objects.is_empty();
    Ok(if a_backup {
        Found::NoUsableNotes { damaged }
    } else {
        Found::Nothing
    })
}

#[cfg(test)]
mod tests;
