//! Is what Keevoro sent still there?
//!
//! Keevoro remembers each file it sends to a destination (the catalog's
//! `uploaded` records). Memory is not the destination: a person can empty
//! a backup folder, swap the drive for another, or a file can be cut short.
//! So before anything counts a file as "already there", Keevoro looks.
//!
//! **The one rule.** A file is at a destination when Keevoro sent it there
//! and the last look found it there at the size it was sent
//! (`Catalog::is_uploaded`). A look writes what it found into those very
//! records ([`Seen`]), so everything that asks (the plan of a backup, what
//! is waiting and safe, the room check, the rehearsal, the map's "Backed
//! up", [`crate::discover`]'s `Reach::safe` and the index) agrees at once.
//! A look only ever corrects a record Keevoro wrote: it never adds one for
//! a file it finds, so nothing someone else put there is taken for
//! Keevoro's own, and it never deletes or changes anything at the
//! destination.
//!
//! **How it looks.** One listing of names and sizes under `originals/` and
//! `renditions/`, never a file's contents: for a folder, each folder's
//! listing once (several at a time across a network); for cloud storage,
//! the store's listing of those prefixes, a page at a time. Reading every
//! file back stays with "Check they all arrived" ([`crate::send::check`]).
//! The records are read before the listing, so a file another program
//! finishes while Keevoro looks is not in what is checked, and cannot be
//! mistaken for one that has gone.
//!
//! **When.** At the start of every backup and every rehearsal, always;
//! and when the backup screen is opened or a destination is added, unless
//! the last look is younger than [`FRESH_FOR`]. What a screen shows as
//! certain is only what a look younger than that found ([`LookedView::fresh`]).
//!
//! **Out of reach is not empty.** A drive unplugged, a share offline or
//! keys refused make a look fail, and a failed look changes no record: it
//! is said in words ("Keevoro can't find the folder ... Is it plugged
//! in?"), and what was there when Keevoro last looked stays as it was.
//!
//! **Another disk is not the backup's drive.** On a Mac or Linux a drive
//! unplugged can leave the folder it was mounted on behind, empty, on the
//! computer's own disk. If that folder is the destination itself it is
//! still there, so it would look emptied, and a backup would be sent into
//! it, onto the wrong disk. So the disk a folder destination is on is
//! remembered when it is added and each time it is reached
//! ([`crate::reach::volume_of`]); a folder now on another disk that holds
//! nothing of a Keevoro backup ([`on_another_disk`]) is out of reach, said
//! as the drive not seeming to be plugged in, and nothing is sent there
//! until the person says this disk is the one ("Use this folder as it is
//! now", which adds it again). One saved before the disk was remembered
//! goes on as before until it is next reached.

use crate::connect::{Saved, World};
use crate::send::{folder_target, Destination};
use crate::summary;
use anyhow::Result;
use keevoro_catalog::{Catalog, Seen, SeenCounts};
use keevoro_storage::{ObjectKey, ObjectStore};
use keevoro_storage_folder::FolderStore;
use keevoro_storage_s3::{S3Settings, S3Store};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// How long a look is trusted by the screens before they look again. A
/// backup and a rehearsal always look afresh.
pub const FRESH_FOR: i64 = 5 * 60;

/// What Keevoro's originals and previews are kept under at a destination,
/// and so the only places a look lists.
const LOOKED_IN: [&str; 2] = ["originals/", "renditions/"];

/// The catalog's name for what the last look at a destination found.
fn setting(target: &str) -> String {
    format!("looked:{target}")
}

/// The last look at one destination, as it is remembered.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Looked {
    /// When it looked, in seconds since 1970, UTC.
    pub at: i64,
    /// False when the destination could not be reached.
    pub reached: bool,
    /// Why not, in words, when it could not.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub problem: Option<String>,
    /// When it was last reached, which is `at` when it was.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_reached: Option<i64>,
    /// The destination looked emptied or swapped for another: Keevoro's
    /// note and index gone, or most of what it sent there gone.
    #[serde(default)]
    pub emptied: bool,
    /// The folder is on another disk than the one it was on, and holds
    /// nothing of a backup: most likely the folder a drive was mounted on,
    /// left behind empty when it was unplugged. Out of reach until the
    /// person says otherwise.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub other_disk: bool,
}

/// What a look found, before it is written down: pure, so its rules are
/// tested with made-up listings.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Found {
    /// Each record whose state changes, and what it changes to.
    pub changes: Vec<(String, Seen)>,
    /// Of the originals Keevoro sent: found as sent, missing, another size.
    pub originals: SeenCounts,
}

/// Compares what Keevoro sent with what is there. A record is there when
/// a file of its name and size is listed, missing when nothing of its
/// name is, and not as sent when something of its name is another size.
pub fn compare(sent: &[keevoro_catalog::Sent], listed: &HashMap<String, u64>) -> Found {
    let mut found = Found::default();
    for record in sent {
        let seen = match listed.get(&record.key) {
            Some(bytes) if *bytes == record.bytes => Seen::There,
            Some(_) => Seen::NotAsSent,
            None => Seen::Missing,
        };
        if record.key.starts_with(LOOKED_IN[0]) {
            match seen {
                Seen::There => found.originals.there += 1,
                Seen::Missing => found.originals.missing += 1,
                Seen::NotAsSent => found.originals.not_as_sent += 1,
            }
        }
        if seen != record.seen {
            found.changes.push((record.key.clone(), seen));
        }
    }
    found
}

/// Is a folder destination on another disk than the one it is known by, and
/// with nothing of a backup in it, so that it is not to be taken for the
/// backup's drive? Pure. A disk not known yet (a folder saved before it was
/// remembered), or one the system cannot name now, is not another; and a
/// folder that holds a backup is the backup's, whatever number its disk is
/// known by today (a drive plugged in again can be given another).
pub fn on_another_disk(remembered: Option<&str>, now: Option<&str>, holds_a_backup: bool) -> bool {
    matches!((remembered, now), (Some(then), Some(now)) if then != now) && !holds_a_backup
}

/// Does a folder hold anything of a Keevoro backup: its originals, or the
/// note it writes beside them?
fn holds_a_backup(folder: &std::path::Path) -> bool {
    folder.join("originals").exists() || folder.join(crate::layout::GUIDE_NAME).exists()
}

/// Refuses a folder destination now on another disk with nothing of a
/// backup in it, in the words a look uses: for work that reads what is at
/// a destination without looking first, such as "Check they all arrived".
pub fn not_on_another_disk(saved: &Saved, destination: &Destination) -> Result<()> {
    if let Destination::Folder(folder) = destination {
        let now = crate::reach::volume_of(folder);
        if on_another_disk(saved.volume.as_deref(), now.as_deref(), holds_a_backup(folder)) {
            anyhow::bail!("{}", other_disk_words(folder));
        }
    }
    Ok(())
}

/// What is said of a folder destination on another disk, with nothing of a
/// backup in it.
pub fn other_disk_words(folder: &std::path::Path) -> String {
    format!(
        "The drive this backup goes to does not seem to be plugged in: {} is there, but on another disk, and holds \
         nothing Keevoro backed up, so nothing is sent there. Plug the drive in, or, if this disk is where the \
         backup should go from now on, choose \"Use this folder as it is now\".",
        crate::friendly_path(folder)
    )
}

/// Does a destination look emptied or swapped, rather than missing a file
/// or two? Only asked when something is missing: Keevoro's note or index
/// gone, or most of what it sent gone.
pub fn looks_emptied(originals: SeenCounts, marks_there: bool) -> bool {
    originals.missing > 0 && (!marks_there || originals.missing * 2 > originals.sent())
}

/// The seconds since 1970 now.
pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|d| i64::try_from(d.as_secs()).ok())
        .unwrap_or(0)
}

/// Opens a destination only to look at it: a folder that is not there is
/// out of reach and is never made, and cloud storage is asked nothing
/// until it is listed. Also answers the name its records are kept under.
fn store_to_look_at(saved: &Saved, destination: &Destination) -> Result<(Box<dyn ObjectStore>, String), String> {
    match destination {
        Destination::Folder(folder) => {
            let store = FolderStore::look_at(folder).map_err(|_| {
                format!(
                    "Keevoro can't find the folder {}. Is it plugged in?",
                    crate::friendly_path(folder)
                )
            })?;
            let at_once = if crate::reach::is_across_a_network(folder) {
                crate::reach::AT_ONCE_ACROSS_A_NETWORK
            } else {
                1
            };
            let target = saved.target.clone().unwrap_or_else(|| folder_target(folder));
            Ok((Box::new(store.listing_at_once(at_once)), target))
        }
        Destination::Bucket {
            preset,
            bucket,
            access_key_id,
            secret_access_key,
        } => {
            let store = S3Store::connect(&S3Settings {
                preset: preset.clone(),
                bucket: bucket.clone(),
                access_key_id: access_key_id.clone(),
                secret_access_key: secret_access_key.clone(),
            })
            .map_err(|e| format!("{}.", keevoro_publish::plain_reason(&e)))?;
            let target = saved.target.clone().unwrap_or_else(|| store.target_name().to_string());
            Ok((Box::new(store), target))
        }
    }
}

/// Looks at one destination and writes down what was found. A problem
/// reaching it is the answer, not an error: only Keevoro's own record
/// failing to be read or written is.
pub fn look(catalog: &Catalog, saved: &Saved, world: &World<'_>) -> Result<Looked> {
    let opened = saved
        .open(&mut Vec::new(), world)
        .map_err(|e| format!("{e:#}"))
        .and_then(|destination| store_to_look_at(saved, &destination).map(|opened| (opened, destination)));
    match opened {
        Ok(((store, target), destination)) => {
            if saved.target.is_none() {
                crate::connect::note_target(catalog, saved, &target)?;
            }
            // A folder: is it still on the disk it is known by?
            let mut disk = None;
            if let Destination::Folder(folder) = &destination {
                let now = crate::reach::volume_of(folder);
                if on_another_disk(saved.volume.as_deref(), now.as_deref(), holds_a_backup(folder)) {
                    let mut looked = look_with(catalog, &target, Err(other_disk_words(folder)))?;
                    looked.other_disk = true;
                    catalog.set_setting(&setting(&target), &serde_json::to_string(&looked)?)?;
                    return Ok(looked);
                }
                disk = now;
            }
            let looked = look_with(catalog, &target, Ok(store.as_ref()))?;
            // Reached: the disk it is on now is the one it is known by.
            if let (true, Some(disk)) = (looked.reached, disk) {
                crate::connect::note_volume(catalog, saved, &disk)?;
            }
            Ok(looked)
        }
        Err(problem) => {
            // Without a name for its records nothing can be written down,
            // and nothing needs to be: a failed look changes no record.
            let Some(target) = saved.target.as_deref() else {
                return Ok(failed(None, problem));
            };
            look_with(catalog, target, Err(problem))
        }
    }
}

fn failed(before: Option<Looked>, problem: String) -> Looked {
    Looked {
        at: now(),
        reached: false,
        problem: Some(problem),
        last_reached: before.and_then(|b| b.last_reached),
        emptied: false,
        other_disk: false,
    }
}

/// Looks at a store already opened (or says why it could not be), under
/// the records kept as `target`. What the tests drive.
pub fn look_with(catalog: &Catalog, target: &str, store: Result<&dyn ObjectStore, String>) -> Result<Looked> {
    let before = last_look(catalog, target)?;
    // The records first, then the listing: whatever is recorded was in
    // place before it was recorded, so it is in the listing unless it has
    // really gone.
    let sent = catalog.sent_to(target)?;
    let listed = store.and_then(|store| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| e.to_string())?;
        runtime.block_on(async {
            let mut listed = HashMap::new();
            for prefix in LOOKED_IN {
                for object in store.list_all(prefix).await.map_err(|e| unreachable_words(&e))? {
                    listed.insert(object.key.to_string(), object.bytes);
                }
            }
            // Keevoro's note and its index, which an emptied or swapped
            // destination no longer has.
            let mut marks = vec![crate::layout::GUIDE_NAME.to_string()];
            if let Some(index) = catalog.setting(&format!("index_key:{target}")).ok().flatten() {
                marks.push(index);
            }
            let mut marks_there = true;
            for mark in marks {
                let key = ObjectKey::new(mark).map_err(|e| e.to_string())?;
                marks_there &= store.head(&key).await.map_err(|e| unreachable_words(&e))?.is_some();
            }
            let mut found = compare(&sent, &listed);
            recheck(store, &sent, &mut found).await?;
            Ok((found, marks_there))
        })
    });
    let looked = match listed {
        Ok((found, marks_there)) => {
            catalog.set_seen(target, &found.changes)?;
            let at = now();
            Looked {
                at,
                reached: true,
                problem: None,
                last_reached: Some(at),
                emptied: looks_emptied(found.originals, marks_there),
                other_disk: false,
            }
        }
        Err(problem) => failed(before, problem),
    };
    catalog.set_setting(&setting(target), &serde_json::to_string(&looked)?)?;
    Ok(looked)
}

/// The most files a listing says are missing or another size that are each
/// asked about again, one by one, before a look believes it. Beyond this,
/// the listing is believed only if every one asked about agreed with it.
pub const RECHECK_AT_MOST: usize = 2_000;

/// A listing is not always the truth. On a network share that dropped out
/// and came back, one listing showed 12 files missing and 41 at another
/// size; the backup that followed read every one of them in full and found
/// each there and exact. So a file the listing says has gone, or is another
/// size, is asked about again by itself (its own size, from a fresh
/// question) before anything is written down, and what that says is what
/// counts. Asking one fails, and the whole look is a failed look that
/// changes no record: out of reach is never gone.
async fn recheck(store: &dyn ObjectStore, sent: &[keevoro_catalog::Sent], found: &mut Found) -> Result<(), String> {
    let bytes: HashMap<&str, u64> = sent.iter().map(|s| (s.key.as_str(), s.bytes)).collect();
    let before: HashMap<&str, Seen> = sent.iter().map(|s| (s.key.as_str(), s.seen)).collect();
    let mut asked = 0;
    let mut contradicted = false;
    let mut kept = Vec::with_capacity(found.changes.len());
    let mut unasked = Vec::new();
    for (key, said) in std::mem::take(&mut found.changes) {
        if said == Seen::There {
            kept.push((key, said));
            continue;
        }
        if asked >= RECHECK_AT_MOST {
            unasked.push((key, said));
            continue;
        }
        asked += 1;
        let object = ObjectKey::new(key.clone()).map_err(|e| e.to_string())?;
        let really = match store.head(&object).await.map_err(|e| unreachable_words(&e))? {
            None => Seen::Missing,
            Some(info) if Some(&info.bytes) == bytes.get(key.as_str()) => Seen::There,
            Some(_) => Seen::NotAsSent,
        };
        if really != said {
            contradicted = true;
            if key.starts_with(LOOKED_IN[0]) {
                found.originals.take(said);
                found.originals.add(really);
            }
        }
        if before.get(key.as_str()) != Some(&really) {
            kept.push((key, really));
        }
    }
    // What was not asked about is believed only when the listing proved
    // right every time it was asked; otherwise those records stay as they
    // were, and the counts with them.
    for (key, said) in unasked {
        if contradicted {
            if key.starts_with(LOOKED_IN[0]) {
                found.originals.take(said);
                found
                    .originals
                    .add(before.get(key.as_str()).copied().unwrap_or(Seen::There));
            }
        } else {
            kept.push((key, said));
        }
    }
    found.changes = kept;
    Ok(())
}

/// A problem listing a destination, in the words a backup uses when it
/// cannot reach one.
fn unreachable_words(e: &keevoro_storage::StorageError) -> String {
    match e {
        keevoro_storage::StorageError::Io(io) if io.kind() == std::io::ErrorKind::NotFound => {
            "Keevoro can't find it. Is it plugged in?".to_string()
        }
        keevoro_storage::StorageError::Io(_) => {
            "Keevoro couldn't read what is there. Is it plugged in, or is the network down?".to_string()
        }
        other => format!(
            "Couldn't connect to the storage. {}.",
            keevoro_publish::plain_reason(other)
        ),
    }
}

/// What the last look at the destination kept as `target` found, if
/// Keevoro has looked there.
pub fn last_look(catalog: &Catalog, target: &str) -> Result<Option<Looked>> {
    // One that cannot be read is as good as never having looked.
    Ok(catalog
        .setting(&setting(target))?
        .and_then(|json| serde_json::from_str(&json).ok()))
}

/// Looks at every destination of this backup, as a backup and a rehearsal
/// do before anything else: afresh, whenever the last look was.
pub fn look_everywhere(catalog: &Catalog, world: &World<'_>) -> Result<Vec<(Saved, Looked)>> {
    let mut looked = Vec::new();
    for saved in crate::connect::destinations(catalog)? {
        let seen = look(catalog, &saved, world)?;
        // The name it is kept under may have been learnt just now.
        let saved = crate::connect::destinations(catalog)?
            .into_iter()
            .find(|d| d.same_place(&saved))
            .unwrap_or(saved);
        looked.push((saved, seen));
    }
    Ok(looked)
}

/// Looks again at each destination whose last look is older than
/// [`FRESH_FOR`], or that has never been looked at: what the screens ask,
/// so opening one again and again does not ask a destination each time.
pub fn look_where_due(catalog: &Catalog, world: &World<'_>, now: i64) -> Result<()> {
    for saved in crate::connect::destinations(catalog)? {
        let due = match saved.target.as_deref() {
            Some(target) => last_look(catalog, target)?.is_none_or(|l| !is_fresh(&l, now)),
            None => true,
        };
        if due {
            look(catalog, &saved, world)?;
        }
    }
    Ok(())
}

/// Is this look young enough for a screen to trust without looking again?
pub fn is_fresh(looked: &Looked, now: i64) -> bool {
    (0..FRESH_FOR).contains(&now.saturating_sub(looked.at))
}

/// What a screen is told about one destination: how sure Keevoro is of
/// what is there, and what it has to say about it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LookedView {
    /// When Keevoro last looked, in seconds since 1970.
    pub at: i64,
    /// When it last reached it; `None` when it never has.
    pub last_reached: Option<i64>,
    /// The last look reached it, less than [`FRESH_FOR`] ago: only then is
    /// what is there said as certain.
    pub fresh: bool,
    pub reached: bool,
    /// Originals Keevoro sent there that were not there when it looked,
    /// and those there at another size.
    pub gone: usize,
    pub not_as_sent: usize,
    /// What to say about it, a sentence each: what has gone and what will
    /// happen, or why it could not be looked at.
    pub said: Vec<String>,
    /// A folder now on another disk with nothing of a backup in it: the
    /// screen offers "Use this folder as it is now".
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub other_disk: bool,
}

/// What is known of one destination now, from its records and its last
/// look, without asking it anything.
pub fn as_it_stands(catalog: &Catalog, saved: &Saved, now: i64) -> Result<Option<LookedView>> {
    let Some(target) = saved.target.as_deref() else {
        return Ok(None);
    };
    let Some(looked) = last_look(catalog, target)? else {
        return Ok(None);
    };
    let originals = catalog.seen_counts(target, LOOKED_IN[0])?;
    let name = saved.describe();
    let mut said = Vec::new();
    if looked.reached {
        said.extend(gone_words(&name, originals, looked.emptied, saved.kind == "folder"));
    } else if let Some(problem) = &looked.problem {
        said.push(out_of_reach_words(&name, problem, looked.last_reached, now));
    }
    Ok(Some(LookedView {
        at: looked.at,
        last_reached: looked.last_reached,
        fresh: looked.reached && is_fresh(&looked, now),
        reached: looked.reached,
        gone: originals.missing,
        not_as_sent: originals.not_as_sent,
        said,
        other_disk: looked.other_disk,
    }))
}

/// "The folder D:\\Backup" from "the folder D:\\Backup".
pub(crate) fn capitalised(name: &str) -> String {
    let mut chars = name.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// What to say about what has gone from a destination, before anything
/// is sent: what is missing, what is not as it was sent, and what will be
/// done about each. Nothing when everything is there.
pub fn gone_words(name: &str, originals: SeenCounts, emptied: bool, can_set_aside: bool) -> Vec<String> {
    let mut said = Vec::new();
    let place = capitalised(name);
    let (missing, sent) = (originals.missing, originals.sent());
    if missing > 0 {
        let what = match (missing == sent, sent) {
            (true, 1) => "the file Keevoro backed up there".to_string(),
            (true, _) => format!("any of the {} files Keevoro backed up there", summary::thousands(sent)),
            (false, _) => format!(
                "{} of the {} files Keevoro backed up there",
                summary::thousands(missing),
                summary::thousands(sent)
            ),
        };
        let again = if missing == 1 {
            "It will be sent again."
        } else {
            "They will be sent again."
        };
        said.push(if emptied {
            format!("{place} looks emptied or replaced: it no longer holds {what}. {again}")
        } else {
            format!("{place} no longer holds {what}. {again}")
        });
    }
    let other = originals.not_as_sent;
    if other > 0 {
        said.push(not_as_sent_words(&place, other, can_set_aside));
    }
    said
}

/// What is said of copies at a destination under the names Keevoro gave
/// them but not at the size they were sent at. Keevoro never replaces or
/// deletes a file in a backup, so what puts them right is said with it: in
/// a folder, "Put this backup right" moves them aside there and sends them
/// again; in cloud storage a move is a copy and a delete, which Keevoro
/// never does by itself, so the person is told what they can do.
pub fn not_as_sent_words(place: &str, files: usize, can_set_aside: bool) -> String {
    let one = files == 1;
    let (count, names, them, sizes, they_were, they_are) = if one {
        ("1 file".to_string(), "the name", "it", "the size", "it was", "it is")
    } else {
        (
            format!("{} files", summary::thousands(files)),
            "the names",
            "them",
            "the sizes",
            "they were",
            "they are",
        )
    };
    let (those, copies) = if one { ("that", "copy") } else { ("those", "copies") };
    let what_next = if can_set_aside {
        format!(
            "Keevoro never replaces a file in a backup: \"Put this backup right\" moves {those} {copies} aside \
             there, into a folder of their own, then sends {them} again and proves {}.",
            if one { "it" } else { "each one" }
        )
    } else {
        format!(
            "Keevoro never replaces or deletes a file in a backup. To put {them} right, delete {those} {copies} in \
             your storage's own website and back up again."
        )
    };
    format!(
        "{place} has {count} under {names} Keevoro gave {them}, but not {sizes} {they_were} backed up at, so \
         {they_are} not counted as safe. {what_next}"
    )
}

/// What to say about a destination Keevoro could not look at: why, in the
/// words a backup uses, and that what it last found is all it knows.
pub fn out_of_reach_words(name: &str, problem: &str, last_reached: Option<i64>, now: i64) -> String {
    let when = match last_reached {
        Some(then) => format!(
            ": everything is counted as it was when Keevoro last reached it, {}",
            summary::ago(then, now)
        ),
        None => String::new(),
    };
    format!(
        "{} Until Keevoro can reach {name}, what is there can't be confirmed{when}.",
        problem.trim()
    )
}

#[cfg(test)]
mod tests;
