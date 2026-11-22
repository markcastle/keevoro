//! Putting a backup right, with one button.
//!
//! An owner's backup to a network share that dropped out part-way was left
//! with files Keevoro had recorded as safe that a look could not find, or
//! found another size. They asked for Keevoro to put it right itself, with
//! nothing done by hand. This does, for one destination:
//!
//! 1. **What is known first.** Every file a look or a check found missing
//!    or not the size it was sent at is asked about afresh and read back in
//!    full. This takes minutes.
//! 2. **Then everything, when asked** (`thorough`). Every other file is read
//!    back in full and compared with its original's fingerprint: the same
//!    proof as "Check they all arrived" reading back, so nothing rests on
//!    sizes. Over a network share this takes hours, so what is proved is
//!    written down as it goes (catalog step 15, `proved_at`), and pressing
//!    again after a stop, a crash or the share dropping out carries on from
//!    where it was.
//! 3. **Damaged copies are moved aside, never deleted.** A copy that is
//!    short, another size or not the same as its original is asked about
//!    once more, then moved into `Set aside by Keevoro/<date>/` in the same
//!    folder, keeping its path, with a note
//!    ([`keevoro_storage_folder::set_aside`]). If the move fails, it stays
//!    exactly where it was and is said. In cloud storage a move is a copy
//!    and a delete, which Keevoro never does by itself, so there it is said,
//!    with what the person can do.
//! 4. **Then sent again and proved.** Everything missing or moved aside is
//!    sent the way a backup sends it, which proves each new copy by reading
//!    it back at its real name.
//! 5. **One account.** Every file being kept ends in exactly one of: proved
//!    (read back in full), there at the right size (not read back, after a
//!    quick pass), or needing attention with its reason ([`Outcome`]).
//!
//! Nothing at a source is ever written. Nothing at a destination is ever
//! deleted or replaced.

use crate::connect::{Saved, World};
use crate::send::{Destination, Stop};
use crate::summary;
use anyhow::Result;
use keevoro_catalog::{Catalog, Seen};
use keevoro_core::estimate::Kind;
use keevoro_publish::{publish, PublishOptions, Upload};
use keevoro_storage::MultipartStore;
use std::collections::{BTreeMap, HashSet};
use std::io::Write;
use std::path::Path;
use std::sync::atomic::Ordering;

/// How far to go.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HowToPutRight {
    /// Read every file back in full, not only those already known to be
    /// wrong.
    pub thorough: bool,
}

/// The catalog's note of a putting right under way at a destination: when
/// it began, so that what was proved since is not read again.
fn setting(target: &str) -> String {
    format!("putting_right:{target}")
}

/// Problems asking in a row, with the destination then out of reach,
/// after which it is taken that the connection has gone.
const IN_A_ROW: usize = 5;

/// How many proved files are written down together.
const PROVED_EVERY: usize = 100;

/// What one copy at the destination turned out to be.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Judged {
    /// Read back in full, and the same as its original.
    Proved,
    /// The right size, and nothing could read it back to be sure (cloud
    /// storage, for a file too large to read back in memory).
    RightSize,
    Missing,
    /// There, but not right: why, in words.
    Damaged(String),
    /// Could not be asked about: why, in words.
    Unsure(String),
}

/// Where every file being kept ended up. Each is in exactly one place.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Outcome {
    /// Every file being kept.
    pub kept: usize,
    /// Read back in full and the same as its original, in this putting
    /// right (and any part of it stopped before).
    pub proved: usize,
    /// There at the size it was sent at, as the last look or check found,
    /// and not read back in full.
    pub right_size: usize,
    /// Of those proved, how many were sent again now.
    pub sent: usize,
    /// Damaged copies moved aside: the backup's path, and where it went.
    pub set_aside: Vec<(String, String)>,
    /// Still needing attention: the file, and why.
    pub attention: Vec<(String, String)>,
    /// Stopped before the end, or the connection was lost.
    pub stopped: bool,
    pub lost_connection: bool,
    /// Every file was read back in full and proved, and nothing needs
    /// attention.
    pub all_proved: bool,
}

impl Outcome {
    /// Does every file being kept have exactly one place?
    pub fn adds_up(&self) -> bool {
        self.proved + self.right_size + self.attention.len() == self.kept
    }
}

/// What putting a backup right would do, said before it starts.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    /// The destination, in words.
    pub to: String,
    /// Files a look or a check already found missing or another size.
    pub known: usize,
    pub known_bytes: u64,
    /// Every file to read back for the full proof, and how much.
    pub files: usize,
    pub bytes: u64,
    /// Of those, already proved by a putting right that was stopped.
    pub proved_already: usize,
    /// About how long each takes, in seconds, from how fast this
    /// destination went before.
    pub quick_seconds: u64,
    pub thorough_seconds: u64,
    /// False for cloud storage, where nothing is moved aside.
    pub sets_aside: bool,
    /// What it will do, a sentence each.
    pub said: Vec<String>,
}

/// How long, said as an estimate: to the five minutes below an hour, to the
/// hour above. Pure.
pub fn about(seconds: u64) -> String {
    if seconds < 60 {
        return "a minute or so".to_string();
    }
    let minutes = seconds.div_ceil(60);
    if minutes < 60 {
        let five = minutes.div_ceil(5) * 5;
        return format!("about {five} minutes");
    }
    let hours = (minutes + 30) / 60;
    format!("about {hours} {}", if hours == 1 { "hour" } else { "hours" })
}

/// Bytes a second this destination is likely to manage: what its last real
/// backup measured, or a careful guess. Pure, given that history.
pub fn likely_rate(measured: Option<(u64, u64)>, across_a_network: bool) -> f64 {
    match measured {
        Some((bytes, seconds)) if seconds >= 60 && bytes >= 50 * 1024 * 1024 => bytes as f64 / seconds as f64,
        _ if across_a_network => 8.0 * 1024.0 * 1024.0,
        _ => 60.0 * 1024.0 * 1024.0,
    }
}

/// The most a recent backup sent to `to` in one turn, and how long it took.
fn measured_for(catalog: &Catalog, to: &str) -> Option<(u64, u64)> {
    crate::history::recent(catalog)
        .ok()?
        .into_iter()
        .filter_map(|sent| sent.detail)
        .flat_map(|detail| detail.destinations)
        .filter(|d| d.to == to && d.seconds >= 60)
        .map(|d| (d.bytes, d.seconds))
        .max_by_key(|(bytes, _)| *bytes)
}

/// The records of what was sent to `saved`, by key, and when a putting
/// right there began, if one is under way.
fn under_way(catalog: &Catalog, target: &str) -> Result<Option<i64>> {
    Ok(catalog
        .setting(&setting(target))?
        .and_then(|s| s.trim().parse::<i64>().ok())
        .filter(|at| *at > 0))
}

/// What putting `saved` right would do, and about how long it would take.
/// Changes nothing and asks the destination nothing.
pub fn plan(catalog: &Catalog, saved: &Saved) -> Result<Plan> {
    let to = saved.describe();
    let target = saved.target.clone().unwrap_or_default();
    let reports = catalog.load_reports()?;
    let kept = crate::send::kept_now(catalog, &reports)?;
    let since = under_way(catalog, &target)?;
    let proved = match since {
        Some(at) => catalog.proved_since(&target, at)?,
        None => HashSet::new(),
    };
    let mut plan = Plan {
        to: to.clone(),
        sets_aside: saved.kind == "folder",
        ..Plan::default()
    };
    for k in &kept {
        let bytes = k.item.bytes;
        if let Some(Seen::Missing | Seen::NotAsSent) = catalog.sent_state(&target, &k.path)? {
            plan.known += 1;
            plan.known_bytes += bytes;
        }
        if proved.contains(&k.path) {
            plan.proved_already += 1;
        } else {
            plan.files += 1;
            plan.bytes += bytes;
        }
    }
    let network = saved.kind != "folder" || crate::reach::is_across_a_network(Path::new(&saved.place));
    let rate = likely_rate(measured_for(catalog, &to), network);
    plan.quick_seconds = (plan.known_bytes as f64 / rate) as u64;
    plan.thorough_seconds = (plan.bytes as f64 / rate) as u64;
    plan.said = plan_words(&plan);
    Ok(plan)
}

/// What a plan says, a sentence each. Pure.
pub fn plan_words(plan: &Plan) -> Vec<String> {
    let n = summary::thousands;
    let mut said = Vec::new();
    if plan.known > 0 {
        said.push(format!(
            "First, Keevoro puts right the {} it already knows {} wrong at {}, which should take {}.",
            summary::count(plan.known, "file"),
            if plan.known == 1 { "is" } else { "are" },
            plan.to,
            about(plan.quick_seconds)
        ));
    }
    let carrying_on = if plan.proved_already > 0 {
        format!(
            " It carries on from where it stopped: {} already proved {} not read again.",
            n(plan.proved_already),
            if plan.proved_already == 1 { "is" } else { "are" }
        )
    } else {
        String::new()
    };
    said.push(format!(
        "Then it reads back in full each of the {} you are keeping there and compares it with its original: {}, \
         which should take {} over this connection. It works in the background, so you can use the rest of \
         Keevoro meanwhile, and you can stop it and carry on later.{carrying_on}",
        summary::count(plan.files, "file"),
        summary::bytes(plan.bytes),
        about(plan.thorough_seconds)
    ));
    if plan.sets_aside {
        said.push(
            "Any copy that is short, the wrong size or not the same as its original is moved aside there, into a \
             folder called \"Set aside by Keevoro\", with a note saying what it is. Nothing is deleted or replaced. \
             Then the original is sent again and its new copy read back."
                .to_string(),
        );
    } else {
        said.push(
            "Any copy that is not right is said, with what you can do: in cloud storage moving a file means copying \
             and deleting it, which Keevoro never does by itself. Anything missing is sent again and read back."
                .to_string(),
        );
    }
    said
}

/// Puts the backup right at one destination, as the module says, and says
/// what it did to `out`. `today` names the folder damaged copies go in
/// (`2026-11-22`); `now` gives the time, in seconds since 1970, for what is
/// written down as proved.
#[allow(clippy::too_many_arguments)]
pub fn put_right(
    out: &mut dyn Write,
    catalog: &Catalog,
    home: &Path,
    saved: &Saved,
    world: &World<'_>,
    how: HowToPutRight,
    today: &str,
    now: &dyn Fn() -> i64,
    stop: &Stop,
    progress: &mut dyn FnMut(&str),
) -> Result<Outcome> {
    let destination = saved.open(out, world)?;
    crate::confirm::not_on_another_disk(saved, &destination)?;
    let folder = match &destination {
        Destination::Folder(folder) => Some(folder.clone()),
        Destination::Bucket { .. } => None,
    };
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
    let (store, opened_as, _) = crate::send::open(&mut Vec::new(), &runtime, destination.clone())?;
    let store: &dyn MultipartStore = store.as_ref();
    let target = saved.target.clone().unwrap_or(opened_as);
    let name = saved.describe();

    let reports = catalog.load_reports()?;
    let kept = crate::send::kept_now(catalog, &reports)?;
    let buckets = crate::bucket::Buckets::open(catalog, home, world)?;
    let uploads = crate::send::uploads_for(home, &kept, Kind::SendingOriginals, &buckets)?;
    let hashes: BTreeMap<&str, (&str, bool)> = kept
        .iter()
        .map(|k| (k.path.as_str(), (k.hash, k.item.hashed)))
        .collect();

    // Carrying on, or starting afresh.
    let started_at = match under_way(catalog, &target)? {
        Some(at) => at,
        None => {
            let at = now();
            catalog.set_setting(&setting(&target), &at.to_string())?;
            at
        }
    };
    let proved_before = catalog.proved_since(&target, started_at)?;

    // What to ask about: what is known to be wrong first, then, when
    // asked, everything else that was sent and is not proved yet.
    let mut known = Vec::new();
    let mut rest = Vec::new();
    for upload in &uploads {
        let key = upload.key.as_str();
        match catalog.sent_state(&target, key)? {
            Some(Seen::Missing | Seen::NotAsSent) => known.push(upload),
            Some(Seen::There) if how.thorough && !proved_before.contains(key) => rest.push(upload),
            _ => {}
        }
    }
    let to_ask: Vec<&Upload> = known.iter().chain(rest.iter()).copied().collect();
    let total_bytes: u64 = to_ask.iter().map(|u| u.bytes).sum();

    let mut outcome = Outcome::default();
    let mut why: BTreeMap<String, String> = BTreeMap::new();
    let mut proved_now: Vec<String> = Vec::new();
    let mut in_a_row = 0;
    let mut read = 0u64;
    let clock = std::time::Instant::now();
    for (at, upload) in to_ask.iter().enumerate() {
        if stop.load(Ordering::SeqCst) {
            outcome.stopped = true;
            break;
        }
        let key = upload.key.as_str().to_string();
        let (hash, hashed) = hashes.get(key.as_str()).copied().unwrap_or(("", false));
        let judged = judge(&runtime, store, upload, hash, hashed);
        match &judged {
            Judged::Unsure(_) => in_a_row += 1,
            _ => in_a_row = 0,
        }
        match judged {
            Judged::Proved => proved_now.push(key.clone()),
            Judged::RightSize => catalog.set_seen(&target, &[(key.clone(), Seen::There)])?,
            Judged::Missing => {
                // A share that has dropped out answers "not there" for
                // everything: that is out of reach, never gone.
                if !reachable(&runtime, store, &folder) {
                    outcome.lost_connection = true;
                    break;
                }
                catalog.set_seen(&target, &[(key.clone(), Seen::Missing)])?;
            }
            Judged::Damaged(first) => {
                catalog.set_seen(&target, &[(key.clone(), Seen::NotAsSent)])?;
                // Asked once more, just before anything is moved: a copy
                // is never moved aside on one answer.
                match (judge(&runtime, store, upload, hash, hashed), &folder) {
                    (Judged::Proved, _) => proved_now.push(key.clone()),
                    (Judged::Damaged(_), Some(root)) => match keevoro_storage_folder::set_aside(root, &key, today) {
                        Ok(went) => {
                            catalog.set_seen(&target, &[(key.clone(), Seen::Missing)])?;
                            outcome.set_aside.push((key.clone(), went));
                        }
                        Err(e) => {
                            why.insert(
                                key.clone(),
                                format!("{first}, and it couldn't be moved aside ({e}), so it was left as it is"),
                            );
                        }
                    },
                    (Judged::Damaged(_), None) => {
                        why.insert(
                            key.clone(),
                            format!(
                                "{first}. Keevoro never deletes anything in cloud storage: delete this copy in your \
                                 storage's own website, then back up again"
                            ),
                        );
                    }
                    (Judged::Missing, _) => catalog.set_seen(&target, &[(key.clone(), Seen::Missing)])?,
                    (Judged::RightSize, _) => catalog.set_seen(&target, &[(key.clone(), Seen::There)])?,
                    (Judged::Unsure(reason), _) => {
                        why.insert(key.clone(), format!("Couldn't be checked: {reason}"));
                    }
                }
            }
            Judged::Unsure(reason) => {
                why.insert(key.clone(), format!("Couldn't be checked: {reason}"));
            }
        }
        if proved_now.len() >= PROVED_EVERY {
            catalog.set_proved(&target, &std::mem::take(&mut proved_now), now())?;
        }
        read += upload.bytes;
        progress(&reading_words(
            at + 1,
            to_ask.len(),
            read,
            total_bytes,
            clock.elapsed().as_secs(),
        ));
        if in_a_row >= IN_A_ROW && !reachable(&runtime, store, &folder) {
            outcome.lost_connection = true;
            break;
        }
    }
    catalog.set_proved(&target, &proved_now, now())?;

    // Everything missing now, or moved aside, is sent again the way a
    // backup sends it: each new copy proved at its real name. Never to a
    // folder that has gone, which would be made again on whatever is left.
    if !outcome.stopped && !outcome.lost_connection && !reachable(&runtime, store, &folder) {
        outcome.lost_connection = true;
    }
    if !outcome.stopped && !outcome.lost_connection {
        let mut to_send = Vec::new();
        for upload in &uploads {
            let key = upload.key.as_str();
            if why.contains_key(key) {
                continue;
            }
            if catalog.sent_state(&target, key)? != Some(Seen::There) {
                to_send.push(upload.clone());
            }
        }
        if !to_send.is_empty() {
            let (over_network, at_once) = match &folder {
                None => (true, 6),
                Some(f) if crate::reach::is_across_a_network(f) => (true, crate::reach::AT_ONCE_ACROSS_A_NETWORK),
                Some(_) => (false, 1),
            };
            let opts = PublishOptions {
                stop: Some(stop.clone()),
                over_network,
                at_once,
                ..PublishOptions::default()
            };
            let n = summary::thousands;
            let report = runtime.block_on(publish(store, catalog, &target, &to_send, &opts, &mut |p| {
                progress(&format!("Sending again: {} of {}", n(p.done), n(p.total)));
            }))?;
            for (place, whole) in &report.learned {
                if let Some(k) = kept.iter().find(|k| k.path == *place) {
                    catalog.set_whole_hash(&k.item.path, k.hash, whole)?;
                }
            }
            for failed in &report.failed {
                why.insert(failed.key.clone(), format!("Couldn't be sent again: {}", failed.reason));
            }
            let mut sent_now = Vec::new();
            for upload in &to_send {
                let key = upload.key.as_str();
                if !why.contains_key(key) && catalog.sent_state(&target, key)? == Some(Seen::There) {
                    sent_now.push(key.to_string());
                }
            }
            outcome.sent = report.uploaded;
            outcome.stopped |= report.stopped;
            catalog.set_proved(&target, &sent_now, now())?;
        }
    }

    // One account, from the records as they now stand.
    let proved = catalog.proved_since(&target, started_at)?;
    outcome.kept = uploads.len();
    for upload in &uploads {
        let key = upload.key.as_str();
        if let Some(reason) = why.get(key) {
            outcome.attention.push((key.to_string(), reason.clone()));
        } else if proved.contains(key) {
            outcome.proved += 1;
        } else if catalog.sent_state(&target, key)? == Some(Seen::There) {
            outcome.right_size += 1;
        } else {
            let reason = if outcome.lost_connection {
                "Not checked yet: the connection was lost first"
            } else if outcome.stopped {
                "Not checked yet: stopped first"
            } else {
                "Not there yet"
            };
            outcome.attention.push((key.to_string(), reason.to_string()));
        }
    }
    let finished = !outcome.stopped && !outcome.lost_connection;
    outcome.all_proved = finished && how.thorough && outcome.proved == outcome.kept;
    if finished && how.thorough {
        // Done: the next putting right starts afresh.
        catalog.set_setting(&setting(&target), "0")?;
    }
    for line in outcome_words(&name, &outcome, how.thorough, today) {
        writeln!(out, "{line}")?;
    }
    Ok(outcome)
}

/// Asks about one copy: its size afresh, then reads it back in full and
/// compares it with the original's fingerprint.
fn judge(
    runtime: &tokio::runtime::Runtime,
    store: &dyn MultipartStore,
    upload: &Upload,
    hash: &str,
    hashed: bool,
) -> Judged {
    let reason = keevoro_publish::plain_reason;
    let info = match runtime.block_on(store.head(&upload.key)) {
        Ok(None) => return Judged::Missing,
        Ok(Some(info)) => info,
        Err(e) => return Judged::Unsure(reason(&e)),
    };
    if info.bytes != upload.bytes {
        return Judged::Damaged(format!(
            "It was {} there, not {} as its original is",
            summary::bytes(info.bytes),
            summary::bytes(upload.bytes)
        ));
    }
    // A file a search only sampled is fingerprinted from its original,
    // which is only ever read.
    let fingerprint = if hashed {
        hash.to_string()
    } else {
        match fingerprint_of(&upload.file) {
            Ok(f) => f,
            Err(e) => return Judged::Unsure(format!("its original couldn't be read to compare it with ({e})")),
        }
    };
    match runtime.block_on(store.holds_exactly(&upload.key, &fingerprint)) {
        Ok(Some(true)) => Judged::Proved,
        Ok(Some(false)) => Judged::Damaged("Its contents are not the same as its original's".to_string()),
        Ok(None) if upload.bytes <= keevoro_publish::MAX_READ_BACK_BYTES => {
            match runtime.block_on(store.get(&upload.key)) {
                Ok(bytes) if blake3::hash(&bytes).to_hex().as_str() == fingerprint => Judged::Proved,
                Ok(_) => Judged::Damaged("Its contents are not the same as its original's".to_string()),
                Err(e) => Judged::Unsure(reason(&e)),
            }
        }
        Ok(None) => Judged::RightSize,
        Err(e) => Judged::Unsure(reason(&e)),
    }
}

fn fingerprint_of(file: &Path) -> std::io::Result<String> {
    use std::io::Read;
    let mut input = std::fs::File::open(file)?;
    let mut hasher = blake3::Hasher::new();
    let mut buffer = vec![0u8; 1024 * 1024];
    loop {
        match input.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => {
                hasher.update(&buffer[..n]);
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(hasher.finalize().to_hex().to_string())
}

/// Is the destination still there? A folder by whether it still holds
/// Keevoro's note; cloud storage by whether that note can be asked about.
fn reachable(
    runtime: &tokio::runtime::Runtime,
    store: &dyn MultipartStore,
    folder: &Option<std::path::PathBuf>,
) -> bool {
    match folder {
        Some(f) => f.join(crate::layout::GUIDE_NAME).is_file(),
        None => keevoro_storage::ObjectKey::new(crate::layout::GUIDE_NAME)
            .is_ok_and(|key| runtime.block_on(store.head(&key)).is_ok()),
    }
}

/// The progress line while copies are read back. Pure.
pub fn reading_words(done: usize, total: usize, read: u64, of: u64, seconds: u64) -> String {
    let n = summary::thousands;
    let mut line = format!(
        "Reading back and checking {} of {} ({} of {})",
        n(done),
        n(total),
        summary::bytes(read),
        summary::bytes(of)
    );
    // Only once there is a fair measure.
    if seconds >= 30 && read > 0 && of > read {
        let left = ((of - read) as f64 / (read as f64 / seconds as f64)) as u64;
        line.push_str(&format!(". {} left", capital(&about(left))));
    }
    line
}

fn capital(s: &str) -> String {
    crate::confirm::capitalised(s)
}

/// What putting right says when it ends, a sentence each, every number from
/// one [`Outcome`]. Pure.
pub fn outcome_words(place: &str, outcome: &Outcome, thorough: bool, today: &str) -> Vec<String> {
    let n = summary::thousands;
    let mut said = Vec::new();
    if !outcome.set_aside.is_empty() {
        let k = outcome.set_aside.len();
        said.push(format!(
            "Moved {} aside into \"{}\" at {place}, with a note saying what {}. Nothing was deleted; you can delete \
             that folder once you are happy.",
            if k == 1 {
                "1 damaged copy".to_string()
            } else {
                format!("{} damaged copies", n(k))
            },
            keevoro_storage_folder::set_aside_folder(today),
            if k == 1 { "it is" } else { "they are" }
        ));
    }
    if outcome.sent > 0 {
        said.push(format!(
            "Sent {} again, and read {} back: {}.",
            summary::count(outcome.sent, "file"),
            if outcome.sent == 1 { "it" } else { "each one" },
            if outcome.sent == 1 { "it matches" } else { "they match" }
        ));
    }
    let attention = outcome.attention.len();
    if attention == 0 && outcome.all_proved {
        said.push(format!(
            "Your backup at {place} is right: all {} you are keeping are there, and every one was read back in full \
             and matches its original.",
            summary::count(outcome.kept, "file")
        ));
    } else if attention == 0 {
        said.push(format!(
            "All {} you are keeping are at {place}: {} read back in full and {} match, and {} {} the right size when \
             Keevoro last looked.",
            summary::count(outcome.kept, "file"),
            n(outcome.proved),
            if outcome.proved == 1 { "it" } else { "they" },
            n(outcome.right_size),
            if outcome.right_size == 1 { "was" } else { "were" },
        ));
        if !thorough && outcome.right_size > 0 {
            said.push(
                "To read every one of those back too, choose \"Put this backup right\" and \"Check every file\"."
                    .to_string(),
            );
        }
    } else {
        said.push(format!(
            "{} of the {} you are keeping are at {place} and right. {} still {} attention:",
            n(outcome.proved + outcome.right_size),
            summary::count(outcome.kept, "file"),
            n(attention),
            if attention == 1 { "needs" } else { "need" }
        ));
        for (key, why) in outcome.attention.iter().take(5) {
            said.push(format!("  {key}: {why}."));
        }
        if attention > 5 {
            said.push(format!("  and {} more.", n(attention - 5)));
        }
    }
    if outcome.lost_connection {
        said.push(format!(
            "The connection to {place} was lost part-way, so Keevoro stopped. Everything it proved is remembered: \
             choose \"Put this backup right\" again once it is back, and it carries on from where it stopped."
        ));
    } else if outcome.stopped {
        said.push(
            "Stopped. Everything proved so far is remembered: \"Put this backup right\" carries on from here."
                .to_string(),
        );
    }
    said
}

#[cfg(test)]
mod tests;
