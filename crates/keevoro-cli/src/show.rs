//! Each answer, said in plain English.
//!
//! The answers are the API's own JSON, exactly what `--json` prints, so
//! these only choose the words. Problems go to standard error and what was
//! asked for to standard output, so a script can keep one and not the
//! other.

use crate::{Code, Failure, Io};
use clap::ArgMatches;
use keevoro_api::words::{bytes, count, date_and_time, duration, thousands};
use keevoro_api::{Picture, Said};
use serde_json::Value;

type Said_ = Result<Code, Failure>;

fn s<'v>(v: &'v Value, name: &str) -> &'v str {
    v.get(name).and_then(Value::as_str).unwrap_or_default()
}

fn n(v: &Value, name: &str) -> usize {
    v.get(name)
        .and_then(Value::as_u64)
        .and_then(|n| usize::try_from(n).ok())
        .unwrap_or_default()
}

fn b(v: &Value, name: &str) -> u64 {
    v.get(name).and_then(Value::as_u64).unwrap_or_default()
}

fn yes(v: &Value, name: &str) -> bool {
    v.get(name).and_then(Value::as_bool).unwrap_or_default()
}

fn each(v: &Value) -> impl Iterator<Item = &Value> {
    v.as_array().into_iter().flatten()
}

fn photos_and_videos(photos: usize, videos: usize) -> String {
    format!("{} and {}", count(photos, "photo"), count(videos, "video"))
}

/// For answers that a job says as it goes, so nothing more is needed.
pub fn nothing_more(_: &mut Io<'_>, _: &ArgMatches, _: &Value) -> Said_ {
    Ok(Code::Worked)
}

pub fn backups(io: &mut Io<'_>, _: &ArgMatches, list: &Value) -> Said_ {
    if each(list).next().is_none() {
        writeln!(
            io.out,
            "There are no backups yet. Start one with: keevoro backups start \"Family photos\""
        )?;
    }
    for backup in each(list) {
        let open = if yes(backup, "open") {
            "  (the one the app has open)"
        } else {
            ""
        };
        writeln!(io.out, "{}{open}", s(backup, "name"))?;
        match backup.get("found").filter(|f| !f.is_null()) {
            None => writeln!(io.out, "  Its folder isn't there right now: {}", s(backup, "home"))?,
            Some(found) => {
                let waiting = match n(found, "waiting") {
                    0 => "nothing waiting".to_string(),
                    w => format!("{} waiting ({})", thousands(w), bytes(b(found, "waitingBytes"))),
                };
                writeln!(
                    io.out,
                    "  {} · {} · {} · {waiting}",
                    count(n(found, "sources"), "source"),
                    count(n(found, "destinations"), "destination"),
                    photos_and_videos(n(found, "photos"), n(found, "videos")),
                )?;
            }
        }
    }
    Ok(Code::Worked)
}

pub fn started(io: &mut Io<'_>, _: &ArgMatches, backup: &Value) -> Said_ {
    writeln!(io.out, "Started the backup \"{}\".", s(backup, "name"))?;
    writeln!(
        io.out,
        "Next, look through a source for it: keevoro look <a drive or folder> -b \"{}\"",
        s(backup, "name")
    )?;
    Ok(Code::Worked)
}

pub fn renamed(io: &mut Io<'_>, _: &ArgMatches, backup: &Value) -> Said_ {
    writeln!(io.out, "It is now called \"{}\".", s(backup, "name"))?;
    Ok(Code::Worked)
}

pub fn forgot(io: &mut Io<'_>, _: &ArgMatches, backup: &Value) -> Said_ {
    writeln!(
        io.out,
        "Forgot the backup \"{}\". Nothing at any source or destination was touched, and its own folder is where it was: {}",
        s(backup, "name"),
        s(backup, "home")
    )?;
    Ok(Code::Worked)
}

pub fn places(io: &mut Io<'_>, _: &ArgMatches, list: &Value) -> Said_ {
    if each(list).next().is_none() {
        writeln!(io.out, "No drives were found.")?;
    }
    for place in each(list) {
        writeln!(io.out, "{}  {}", s(place, "name"), s(place, "detail"))?;
        writeln!(io.out, "  {}", s(place, "path"))?;
    }
    Ok(Code::Worked)
}

pub fn sources(io: &mut Io<'_>, _: &ArgMatches, list: &Value) -> Said_ {
    if each(list).next().is_none() {
        writeln!(
            io.out,
            "This backup has no sources yet. Add one with: keevoro look <a drive or folder>"
        )?;
    }
    for source in each(list) {
        writeln!(
            io.out,
            "{}. {}  {}",
            b(source, "id"),
            s(source, "name"),
            s(source, "root")
        )?;
        let items = match n(source, "items") {
            1 => "1 photo or video".to_string(),
            many => format!("{} photos and videos", thousands(many)),
        };
        writeln!(io.out, "   {items} ({})", bytes(b(source, "bytes")))?;
        if let Some(online) = source.get("onlineOnlySaid").and_then(Value::as_str) {
            writeln!(io.out, "   {online}")?;
        }
        for folder in each(source.get("leftOut").unwrap_or(&Value::Null)).filter_map(Value::as_str) {
            writeln!(
                io.out,
                "   Left out: {folder}  (keevoro sources leave-out \"{folder}\" --put-back)"
            )?;
        }
    }
    Ok(Code::Worked)
}

/// An answer that says what happened in its own words, as `said`.
pub fn said(io: &mut Io<'_>, _: &ArgMatches, answer: &Value) -> Said_ {
    writeln!(io.out, "{}", s(answer, "said"))?;
    Ok(Code::Worked)
}

pub fn forgot_source(io: &mut Io<'_>, _: &ArgMatches, _: &Value) -> Said_ {
    writeln!(
        io.out,
        "Took that source off the backup. Nothing on it was touched, and what is already backed up stays."
    )?;
    Ok(Code::Worked)
}

pub fn file_types(io: &mut Io<'_>, m: &ArgMatches, answer: &Value) -> Said_ {
    let change = |name: &str| m.get_one::<String>(name).cloned();
    if let Some(ending) = change("leave-out") {
        writeln!(
            io.out,
            "Leaving out .{} files on that source. Nothing on it is touched.",
            ending.trim_start_matches('.')
        )?;
        return Ok(Code::Worked);
    }
    if let Some(ending) = change("include") {
        writeln!(
            io.out,
            "Including .{} files on that source.",
            ending.trim_start_matches('.')
        )?;
        return Ok(Code::Worked);
    }
    for kind in each(answer) {
        let ending = match s(kind, "ending") {
            "" => "(no ending)".to_string(),
            e => format!(".{e}"),
        };
        writeln!(
            io.out,
            "{ending:<12} {:>14}  {:>9}  {}",
            count(n(kind, "files"), "file"),
            bytes(b(kind, "bytes")),
            match kind.get("why").and_then(Value::as_str) {
                Some(why) => format!("left out: {why}"),
                None if yes(kind, "included") => "included".to_string(),
                None => "left out".to_string(),
            }
        )?;
    }
    Ok(Code::Worked)
}

pub fn found(io: &mut Io<'_>, _: &ArgMatches, found: &Value) -> Said_ {
    writeln!(
        io.out,
        "Found {} ({}).",
        photos_and_videos(n(found, "photos"), n(found, "videos")),
        bytes(b(found, "bytes"))
    )?;
    if n(found, "duplicates") > 0 {
        writeln!(
            io.out,
            "{} left out, saving {}.",
            count(n(found, "duplicates"), "duplicate"),
            bytes(b(found, "duplicateBytes"))
        )?;
    }
    if n(found, "setAside") > 0 {
        writeln!(
            io.out,
            "{} set aside as probably not photos. `keevoro set-aside` lists them.",
            count(n(found, "setAside"), "picture")
        )?;
    }
    match n(found, "waiting") {
        0 => writeln!(io.out, "Everything being kept is safely backed up.")?,
        w => writeln!(
            io.out,
            "{} not yet safely backed up ({}).",
            count(w, "file"),
            bytes(b(found, "waitingBytes"))
        )?,
    }
    Ok(Code::Worked)
}

pub fn collections(io: &mut Io<'_>, _: &ArgMatches, list: &Value) -> Said_ {
    if each(list).next().is_none() {
        writeln!(io.out, "Nothing has been found yet.")?;
    }
    for c in each(list) {
        let left_out = if yes(c, "kept") { "" } else { "  (left out)" };
        writeln!(io.out, "{}  {}{left_out}", s(c, "key"), s(c, "summary"))?;
    }
    Ok(Code::Worked)
}

fn photo_line(io: &mut Io<'_>, photo: &Value) -> std::io::Result<()> {
    let left_out = if yes(photo, "kept") { "" } else { "  (left out)" };
    let reason = photo
        .get("junkReason")
        .and_then(Value::as_str)
        .map(|why| format!("  {why}"))
        .unwrap_or_default();
    writeln!(
        io.out,
        "{}  {}  {}{reason}{left_out}",
        s(photo, "id"),
        s(photo, "name"),
        photo.get("taken").and_then(Value::as_str).unwrap_or("no date")
    )
}

pub fn photos(io: &mut Io<'_>, _: &ArgMatches, list: &Value) -> Said_ {
    for photo in each(list) {
        photo_line(io, photo)?;
    }
    Ok(Code::Worked)
}

pub fn set_aside(io: &mut Io<'_>, _: &ArgMatches, list: &Value) -> Said_ {
    if each(list).next().is_none() {
        writeln!(io.out, "Nothing was set aside.")?;
        return Ok(Code::Worked);
    }
    for photo in each(list) {
        photo_line(io, photo)?;
    }
    writeln!(io.out, "To keep one after all: keevoro keep --photo <code>")?;
    Ok(Code::Worked)
}

pub fn kept(io: &mut Io<'_>, _: &ArgMatches, answer: &Value) -> Said_ {
    writeln!(io.out, "Keeping {}. It will be backed up.", chosen(answer))?;
    Ok(Code::Worked)
}

pub fn left_out(io: &mut Io<'_>, _: &ArgMatches, answer: &Value) -> Said_ {
    writeln!(
        io.out,
        "Leaving out {}. Nothing is deleted, and `keevoro keep` changes your mind.",
        chosen(answer)
    )?;
    Ok(Code::Worked)
}

/// What a choice was made for: a collection answers how many files.
fn chosen(answer: &Value) -> String {
    match answer.as_u64().and_then(|files| usize::try_from(files).ok()) {
        Some(files) => format!("that collection ({})", count(files, "file")),
        None => "that one".to_string(),
    }
}

pub fn duplicates(io: &mut Io<'_>, _: &ArgMatches, list: &Value) -> Said_ {
    if each(list).next().is_none() {
        writeln!(io.out, "No file was found more than once.")?;
    }
    for group in each(list) {
        let copies: Vec<&Value> = each(group.get("copies").unwrap_or(&Value::Null)).collect();
        let unsure = if yes(group, "confirmed") {
            ""
        } else {
            ", not yet confirmed byte for byte"
        };
        writeln!(
            io.out,
            "{} in {}  ({} left out{unsure})",
            s(group, "name"),
            s(group, "folder"),
            match copies.len() {
                1 => "1 copy".to_string(),
                many => format!("{} copies", thousands(many)),
            }
        )?;
        for copy in copies {
            writeln!(io.out, "  also {} in {}", s(copy, "name"), s(copy, "folder"))?;
        }
    }
    Ok(Code::Worked)
}

pub fn picture(io: &mut Io<'_>, m: &ArgMatches, answer: &Value) -> Said_ {
    let to = m.get_one::<String>("to").cloned().unwrap_or_default();
    let picture =
        Picture::bytes_of(answer).ok_or_else(|| Failure::new(Code::SomeFiles, "Keevoro made no picture of that."))?;
    std::fs::write(&to, picture).map_err(|e| {
        Failure::new(
            Code::SomeFiles,
            format!("Keevoro couldn't save the picture to {to}: {e}."),
        )
    })?;
    writeln!(io.out, "Saved the picture to {to}.")?;
    Ok(Code::Worked)
}

pub fn folders(io: &mut Io<'_>, _: &ArgMatches, view: &Value) -> Said_ {
    writeln!(
        io.out,
        "{} in {}, in {}:",
        photos_and_videos(n(view, "photos"), n(view, "videos")),
        s(view, "place"),
        count(each(view.get("folders").unwrap_or(&Value::Null)).count(), "folder")
    )?;
    for folder in each(view.get("folders").unwrap_or(&Value::Null)) {
        let online = match n(folder, "onlineOnly") {
            0 => String::new(),
            many => format!(", {} only online", thousands(many)),
        };
        writeln!(
            io.out,
            "{:>9}  {}{online}  {}",
            bytes(b(folder, "bytes")),
            photos_and_videos(n(folder, "photos"), n(folder, "videos")),
            s(folder, "folder")
        )?;
    }
    if view.get("complete").and_then(Value::as_bool) == Some(false) {
        writeln!(io.out, "The survey was stopped part-way, so there may be more.")?;
    }
    Ok(Code::Worked)
}

/// Of `whole`, as a percentage a person reads: "40%", "<1%".
fn share(part: u64, whole: u64) -> String {
    if whole == 0 {
        return "0%".to_string();
    }
    #[allow(clippy::cast_precision_loss)]
    let percent = part as f64 * 100.0 / whole as f64;
    if part > 0 && percent < 1.0 {
        "<1%".to_string()
    } else {
        format!("{percent:.0}%")
    }
}

/// What a box on the map holds, in words: "1,204 photos, 37 videos, 12
/// other files", leaving out kinds it has none of.
fn holds(node: &Value) -> String {
    let said: Vec<String> = [("photos", "photo"), ("videos", "video"), ("others", "other file")]
        .into_iter()
        .filter(|(field, _)| n(node, field) > 0)
        .map(|(field, word)| count(n(node, field), word))
        .collect();
    if said.is_empty() {
        "empty".to_string()
    } else {
        said.join(", ")
    }
}

/// One line per box, indented by depth, the biggest first.
fn map_lines(io: &mut Io<'_>, node: &Value, whole: u64, depth: usize, above: Option<&str>) -> Result<(), Failure> {
    let mut notes = Vec::new();
    let skipped = node.get("skipped").and_then(Value::as_str);
    // Said once, on the folder itself, not again on everything inside it.
    if let Some(skipped) = skipped.filter(|said| Some(*said) != above) {
        notes.push(format!("{skipped}, never looked through"));
    }
    match node.get("leftOut").and_then(Value::as_str) {
        Some(why) if !yes(node, "madeByKeevoro") => notes.push(format!("not backed up: {why}")),
        _ => {}
    }
    if yes(node, "source") {
        notes.push("a source of this backup".to_string());
    }
    if let Some(kept) = node.get("backedUp") {
        let (safe, of) = (n(kept, "files"), n(kept, "of"));
        if of > 0 {
            notes.push(format!(
                "backed up: {} of {}",
                keevoro_api::words::thousands(safe),
                keevoro_api::words::thousands(of)
            ));
        }
    }
    if b(node, "onlineOnlyBytes") > 0 {
        notes.push(format!("{} only online", bytes(b(node, "onlineOnlyBytes"))));
    }
    if yes(node, "unreadable") {
        notes.push("couldn't be read".to_string());
    }
    if yes(node, "notSized") {
        notes.push("not all measured".to_string());
    }
    let name = match s(node, "node") {
        "smaller" => count(n(node, "folders"), "smaller folder"),
        "files" => format!("{} here", holds(node)),
        _ => s(node, "name").to_string(),
    };
    let what = if s(node, "node") == "files" {
        String::new()
    } else {
        format!("  {}", holds(node))
    };
    let notes = if notes.is_empty() {
        String::new()
    } else {
        format!("  ({})", notes.join("; "))
    };
    writeln!(
        io.out,
        "{:>9}  {:>4}  {}{name}{what}{notes}",
        bytes(b(node, "bytes")),
        share(b(node, "bytes"), whole),
        "  ".repeat(depth)
    )?;
    for child in each(node.get("children").unwrap_or(&Value::Null)) {
        map_lines(io, child, whole, depth + 1, skipped)?;
    }
    Ok(())
}

pub fn space_map(io: &mut Io<'_>, m: &ArgMatches, view: &Value) -> Said_ {
    let top = view.get("top").unwrap_or(&Value::Null);
    let whole = b(top, "bytes");
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(0));
    let when = view.get("surveyedAt").and_then(Value::as_i64).unwrap_or(0);
    writeln!(
        io.out,
        "Where the space goes in {} (mapped {})",
        s(view, "folder"),
        keevoro_api::words::ago(when, now)
    )?;
    if m.get_flag("kinds") {
        for kind in each(view.get("kinds").unwrap_or(&Value::Null)) {
            let ending = match s(kind, "ending") {
                "" => "(no ending)".to_string(),
                "…" => "(other endings)".to_string(),
                ending => ending.to_string(),
            };
            let what = match s(kind, "kind") {
                "photo" => "photos",
                "video" => "videos",
                _ => "everything else",
            };
            let left_out = match kind.get("leftOut").and_then(Value::as_str) {
                Some(why) => format!("  (not backed up: {why})"),
                None => String::new(),
            };
            writeln!(
                io.out,
                "{:>9}  {:>4}  {ending:<10}  {}, {what}{left_out}",
                bytes(b(kind, "bytes")),
                share(b(kind, "bytes"), whole),
                count(n(kind, "files"), "file")
            )?;
        }
        if yes(view, "kindsShort") {
            writeln!(io.out, "Some folders' endings were not kept, so these add up to less.")?;
        }
    } else {
        map_lines(io, top, whole, 0, None)?;
    }
    writeln!(
        io.out,
        "By kind: photos {} ({}), videos {} ({}), everything else {} ({})",
        bytes(b(top, "photoBytes")),
        share(b(top, "photoBytes"), whole),
        bytes(b(top, "videoBytes")),
        share(b(top, "videoBytes"), whole),
        bytes(b(top, "otherBytes")),
        share(b(top, "otherBytes"), whole)
    )?;
    match view.get("backup").and_then(Value::as_str) {
        Some(name) => writeln!(
            io.out,
            "The backup \"{name}\" would take {} of it: the photos and videos it wants.",
            bytes(b(top, "wouldBackUp"))
        )?,
        None => writeln!(
            io.out,
            "Of it, {} is photos and videos Keevoro would back up.",
            bytes(b(top, "wouldBackUp"))
        )?,
    }
    if view.get("refresh").is_some() {
        let why = if yes(view, "searchedSince") {
            "a source in it has been looked through since"
        } else {
            "it may have changed since"
        };
        writeln!(
            io.out,
            "This was mapped {} and {why}; keevoro survey maps it again.",
            keevoro_api::words::ago(when, now)
        )?;
    }
    if !yes(view, "complete") {
        writeln!(
            io.out,
            "Partly mapped: the survey was stopped part-way, so there may be more."
        )?;
    } else if !yes(view, "sizedAll") {
        writeln!(
            io.out,
            "The survey was stopped before everything was measured, so some folders may be bigger."
        )?;
    }
    Ok(Code::Worked)
}

pub fn files(io: &mut Io<'_>, _: &ArgMatches, view: &Value) -> Said_ {
    let list: Vec<&Value> = each(view.get("files").unwrap_or(&Value::Null)).collect();
    writeln!(
        io.out,
        "{} in {}{}",
        photos_and_videos(n(view, "photos"), n(view, "videos")),
        s(view, "folder"),
        if list.len() < n(view, "total") {
            format!(", {} of them here:", thousands(list.len()))
        } else {
            ":".to_string()
        }
    )?;
    for file in list {
        let what = match file.get("seconds").and_then(Value::as_f64) {
            Some(seconds) => format!("video {:>7}", keevoro_api::words::length(seconds)),
            None if yes(file, "isVideo") => "video        ".to_string(),
            None => "photo        ".to_string(),
        };
        let online = if yes(file, "onlineOnly") {
            "  (only online, so not opened)"
        } else {
            ""
        };
        writeln!(
            io.out,
            "{:>9}  {what}  {}{online}",
            bytes(b(file, "bytes")),
            s(file, "path")
        )?;
    }
    Ok(Code::Worked)
}

pub fn destinations(io: &mut Io<'_>, _: &ArgMatches, list: &Value) -> Said_ {
    if each(list).next().is_none() {
        writeln!(
            io.out,
            "This backup doesn't go anywhere yet. Add a place with: keevoro destinations add --folder <a folder>"
        )?;
    }
    for (at, d) in each(list).enumerate() {
        writeln!(io.out, "{}. {}", at + 1, s(d, "name"))?;
        writeln!(
            io.out,
            "   {} there, {} still to go ({})",
            thousands(n(d, "there")),
            thousands(n(d, "waiting")),
            bytes(b(d, "waitingBytes"))
        )?;
    }
    Ok(Code::Worked)
}

pub fn added(io: &mut Io<'_>, m: &ArgMatches, added: &Value) -> Said_ {
    writeln!(
        io.out,
        "Keevoro checked {} works, and will back up there.",
        s(added, "name")
    )?;
    if m.get_one::<String>("folder").is_none() {
        writeln!(
            io.out,
            "Its keys are kept in this computer's keychain, and nowhere else."
        )?;
    }
    Ok(Code::Worked)
}

pub fn removed(io: &mut Io<'_>, _: &ArgMatches, gone: &Value) -> Said_ {
    for name in each(gone).filter_map(Value::as_str) {
        writeln!(
            io.out,
            "No longer backing up to {name}. Nothing that was backed up there was changed."
        )?;
    }
    Ok(Code::Worked)
}

pub fn room(io: &mut Io<'_>, _: &ArgMatches, room: &Value) -> Said_ {
    let places: Vec<&Value> = each(room.get("places").unwrap_or(&Value::Null)).collect();
    if places.is_empty() {
        writeln!(io.out, "This backup doesn't go anywhere yet.")?;
    }
    let mut short = false;
    for p in places {
        let free = match p.get("free").and_then(Value::as_u64) {
            Some(free) => format!(", and {} is free there", bytes(free)),
            None => String::new(),
        };
        writeln!(
            io.out,
            "{}: {} to send ({}){free}.{}",
            s(p, "name"),
            count(n(p, "files"), "file"),
            bytes(b(p, "bytes")),
            if yes(p, "enough") {
                ""
            } else {
                " That isn't enough room."
            }
        )?;
        short |= !yes(p, "enough");
    }
    Ok(if short { Code::Refused } else { Code::Worked })
}

pub fn rehearsal(io: &mut Io<'_>, _: &ArgMatches, rehearsal: &Value) -> Said_ {
    for line in each(rehearsal.get("said").unwrap_or(&Value::Null)).filter_map(Value::as_str) {
        writeln!(io.out, "{line}")?;
    }
    Ok(Code::Worked)
}

pub fn history(io: &mut Io<'_>, m: &ArgMatches, list: &Value) -> Said_ {
    if m.get_one::<usize>("run").is_some() {
        // One backup, in full.
        for sent in each(list) {
            let sent: keevoro_api::history::Sent =
                serde_json::from_value(sent.clone()).map_err(std::io::Error::other)?;
            for line in keevoro_api::words::run_in_full(&sent) {
                writeln!(io.out, "{line}")?;
            }
        }
        return Ok(Code::Worked);
    }
    if each(list).next().is_none() {
        writeln!(io.out, "This backup hasn't been run yet.")?;
    }
    for (n, sent) in each(list).enumerate() {
        let finished = sent.get("finishedAt").and_then(Value::as_i64).unwrap_or_default();
        writeln!(
            io.out,
            "{}. {}  to {}: {} in {}{}",
            n + 1,
            date_and_time(finished),
            s(sent, "to"),
            bytes(b(sent, "bytes")),
            duration(b(sent, "seconds")),
            if yes(sent, "stopped") { ", stopped part-way" } else { "" }
        )?;
    }
    if each(list).next().is_some() {
        writeln!(io.out, "For everything recorded about one: keevoro history 1")?;
    }
    Ok(Code::Worked)
}

pub fn last_backup(io: &mut Io<'_>, _: &ArgMatches, said: &Value) -> Said_ {
    if said.is_null() {
        writeln!(
            io.out,
            "This backup hasn't been run since it last changed where it goes."
        )?;
        return Ok(Code::Worked);
    }
    for line in each(said.get("summary").unwrap_or(&Value::Null)).filter_map(Value::as_str) {
        writeln!(io.out, "{line}")?;
    }
    if let Some(problem) = said.get("problem").and_then(Value::as_str) {
        writeln!(io.out, "{problem}")?;
    }
    Ok(Code::Worked)
}

pub fn wants(io: &mut Io<'_>, _: &ArgMatches, view: &Value) -> Said_ {
    writeln!(io.out, "{}", s(view, "said"))?;
    Ok(Code::Worked)
}

pub fn previews(io: &mut Io<'_>, _: &ArgMatches, wanted: &Value) -> Said_ {
    if wanted.as_bool().unwrap_or(true) {
        writeln!(io.out, "This backup also makes previews, for a gallery to show.")?;
    } else {
        writeln!(io.out, "This backup sends the originals alone, with no previews.")?;
    }
    Ok(Code::Worked)
}

fn job_in_words(job: &Value) -> String {
    let backup = job
        .get("backup")
        .and_then(|b| b.get("name"))
        .and_then(Value::as_str)
        .map(|name| format!(" \"{name}\""))
        .unwrap_or_default();
    match s(job, "job") {
        "survey" => "Finding where the photos are".to_string(),
        "look" => format!("Looking through a source for{backup}"),
        "lookAgain" => format!("Looking through the sources of{backup} again"),
        "backUp" => format!("Backing up{backup}"),
        "check" => format!("Checking what has reached the destinations of{backup}"),
        _ => format!("Checking{backup} for duplicates"),
    }
}

fn started_by(job: &Value) -> &'static str {
    match s(job, "startedBy") {
        "commandLine" => "from the command line",
        "agent" => "for an AI agent",
        _ => "in the Keevoro app",
    }
}

pub fn started_job(io: &mut Io<'_>, job: &Value) -> std::io::Result<()> {
    writeln!(
        io.out,
        "{}, in the background (job {}). See how it is getting on with: keevoro follow",
        job_in_words(job),
        b(job, "id")
    )
}

pub fn status(io: &mut Io<'_>, _: &ArgMatches, list: &Value) -> Said_ {
    if each(list).next().is_none() {
        writeln!(io.out, "Nothing is running.")?;
    }
    for job in each(list) {
        if yes(job, "ended") {
            // Only a job asked for by its number, once it has ended.
            writeln!(io.out, "Job {} has ended.", b(job, "id"))?;
            let last: Option<Said> = job
                .get("last")
                .and_then(|last| serde_json::from_value(last.clone()).ok());
            last_word(io, last.as_ref())?;
            continue;
        }
        writeln!(
            io.out,
            "{} {} (job {}){}",
            job_in_words(job),
            started_by(job),
            b(job, "id"),
            if yes(job, "paused") {
                ", paused"
            } else if yes(job, "stopping") {
                ", stopping"
            } else {
                ""
            }
        )?;
        if !s(job, "step").is_empty() {
            writeln!(io.out, "  {}", s(job, "step"))?;
        }
        // A backup says how long it has run, how long is left, and every
        // stage, in the same words the app and an agent are given.
        match job
            .get("estimate")
            .and_then(|e| serde_json::from_value::<keevoro_api::estimate::Estimate>(e.clone()).ok())
        {
            Some(estimate) => {
                for line in keevoro_api::estimate::in_lines(&estimate) {
                    writeln!(io.out, "  {line}")?;
                }
            }
            None => writeln!(
                io.out,
                "  Running for {}",
                keevoro_api::estimate::running_for(b(job, "runningSeconds"))
            )?,
        }
    }
    Ok(Code::Worked)
}

pub fn paused(io: &mut Io<'_>, _: &ArgMatches, answer: &Value) -> Said_ {
    if yes(answer, "paused") {
        writeln!(
            io.out,
            "Paused: whatever Keevoro is working on waits once the file in hand is done. Nothing is lost."
        )?;
    } else {
        writeln!(io.out, "Carrying on from where it was.")?;
    }
    Ok(Code::Worked)
}

pub fn stopped(io: &mut Io<'_>, _: &ArgMatches, asked: &Value) -> Said_ {
    match each(asked).count() {
        0 => writeln!(io.out, "Nothing is running.")?,
        jobs => writeln!(
            io.out,
            "Asked {} to stop once the file in hand is done. Everything done so far is kept.",
            if jobs == 1 {
                "it".to_string()
            } else {
                format!("{jobs} jobs")
            }
        )?,
    }
    Ok(Code::Worked)
}

pub fn about(io: &mut Io<'_>, _: &ArgMatches, about: &Value) -> Said_ {
    writeln!(io.out, "Keevoro {}", s(about, "version"))?;
    for promise in each(about.get("promises").unwrap_or(&Value::Null)).filter_map(Value::as_str) {
        writeln!(io.out, "  {promise}")?;
    }
    writeln!(io.out, "{}", s(about, "licence"))?;
    Ok(Code::Worked)
}

pub fn activity(io: &mut Io<'_>, _: &ArgMatches, now: &Value) -> Said_ {
    match now.get("doing").and_then(Value::as_str) {
        Some(doing) => writeln!(io.out, "{doing}")?,
        None => writeln!(io.out, "Keevoro is resting.")?,
    }
    if let Some(step) = now.get("backupStep").and_then(Value::as_str).filter(|s| !s.is_empty()) {
        writeln!(io.out, "  {}", step.split('\t').next().unwrap_or_default())?;
    }
    for job in each(now.get("jobs").unwrap_or(&Value::Null)) {
        writeln!(io.out, "  {}: {}", s(job, "what"), s(job, "why"))?;
    }
    if yes(now, "paused") {
        writeln!(io.out, "Paused. `keevoro carry-on` lets it go on.")?;
    }
    Ok(Code::Worked)
}

pub fn log(io: &mut Io<'_>, _: &ArgMatches, text: &Value) -> Said_ {
    writeln!(io.out, "{}", text.as_str().unwrap_or_default())?;
    Ok(Code::Worked)
}

pub fn settings(io: &mut Io<'_>, _: &ArgMatches, settings: &Value) -> Said_ {
    let codecs = settings.get("ownCodecs").unwrap_or(&Value::Null);
    writeln!(
        io.out,
        "own-codecs: {}{}",
        if yes(codecs, "on") { "on" } else { "off" },
        if yes(codecs, "available") {
            ""
        } else {
            " (this computer has none for Keevoro to use, so it makes no difference)"
        }
    )?;
    let ways: Vec<&str> = each(codecs.get("ways").unwrap_or(&Value::Null))
        .filter_map(Value::as_str)
        .collect();
    writeln!(io.out, "  Photos are opened with: {}", ways.join(", then "))?;
    Ok(Code::Worked)
}

pub fn update(io: &mut Io<'_>, _: &ArgMatches, offer: &Value) -> Said_ {
    if offer.is_null() {
        writeln!(io.out, "There is no newer Keevoro to install here.")?;
    } else {
        writeln!(
            io.out,
            "Keevoro {} is available. What's new: {}",
            s(offer, "version"),
            s(offer, "page")
        )?;
    }
    Ok(Code::Worked)
}

/// `keevoro update`: puts the newest command line in place of this one.
/// Installing the app is the app's to do.
pub fn install_update(io: &mut Io<'_>) -> Result<Code, Failure> {
    let current = env!("CARGO_PKG_VERSION");
    let web = keevoro_update::GitHub::from_env();
    let plain = |e: keevoro_update::UpdateError| Failure::new(Code::Unreachable, format!("{e}."));
    let release = match keevoro_update::check(&web, current).map_err(plain)? {
        keevoro_update::Check::UpToDate => {
            writeln!(io.out, "Keevoro {current} is the newest version.")?;
            return Ok(Code::Worked);
        }
        keevoro_update::Check::Available(release) => release,
    };
    writeln!(io.out, "Updating Keevoro {current} to {} ...", release.version)?;
    let program =
        keevoro_update::download(&web, &release, std::env::consts::OS, std::env::consts::ARCH).map_err(plain)?;
    keevoro_update::install(&program).map_err(|e| Failure::new(Code::SomeFiles, format!("{e}.")))?;
    writeln!(
        io.out,
        "Keevoro {} is installed. What's new: {}",
        release.version, release.page
    )?;
    Ok(Code::Worked)
}

pub fn reset(io: &mut Io<'_>, _: &ArgMatches, done: &Value) -> Said_ {
    writeln!(
        io.out,
        "Keevoro is as it was when first installed: {} forgotten.",
        count(each(done.get("removed").unwrap_or(&Value::Null)).count(), "backup")
    )?;
    for folder in each(done.get("leftAlone").unwrap_or(&Value::Null)).filter_map(Value::as_str) {
        writeln!(io.out, "Left alone, because it is not Keevoro's own: {folder}")?;
    }
    Ok(Code::Worked)
}

/// One line of the list of kinds, as `keevoro kinds` shows it.
fn kind_line(kind: &Value) -> String {
    format!(
        ".{:<10} {:<6} {:<9} {}",
        s(kind, "ending"),
        s(kind, "kind"),
        if yes(kind, "builtIn") { "built in" } else { "yours" },
        if yes(kind, "on") { "on" } else { "off" }
    )
}

pub fn kinds(io: &mut Io<'_>, _: &ArgMatches, list: &Value) -> Said_ {
    writeln!(io.out, "{:<11} {:<6} {:<9} looked for", "ending", "kind", "from")?;
    for kind in each(list) {
        writeln!(io.out, "{}", kind_line(kind))?;
    }
    writeln!(
        io.out,
        "Changes count from the next time a source is looked through (keevoro look --again)."
    )?;
    Ok(Code::Worked)
}

pub fn kind_changed(io: &mut Io<'_>, _: &ArgMatches, kind: &Value) -> Said_ {
    let ending = s(kind, "ending");
    let what = if s(kind, "kind") == "video" { "videos" } else { "photos" };
    if yes(kind, "on") {
        writeln!(io.out, "Keevoro looks for .{ending} files, as {what}.")?;
    } else {
        writeln!(
            io.out,
            "Keevoro no longer looks for .{ending} files. Nothing already found or backed up is taken away."
        )?;
    }
    writeln!(io.out, "This counts from the next time a source is looked through.")?;
    Ok(Code::Worked)
}

pub fn maps_forgotten(io: &mut Io<'_>, _: &ArgMatches, forgot: &Value) -> Said_ {
    match n(forgot, "forgotten") {
        0 => writeln!(io.out, "Keevoro remembered no maps.")?,
        many => writeln!(
            io.out,
            "Forgot {}. Nothing on any drive was touched; a place is surveyed again when next asked about.",
            count(many, "remembered map")
        )?,
    }
    Ok(Code::Worked)
}

pub fn kind_removed(io: &mut Io<'_>, _: &ArgMatches, kind: &Value) -> Said_ {
    writeln!(
        io.out,
        "Took .{} off the list. Nothing already found or backed up is taken away.",
        s(kind, "ending")
    )?;
    Ok(Code::Worked)
}

pub fn tidied(io: &mut Io<'_>, _: &ArgMatches, tidied: &Value) -> Said_ {
    // The API's own sentences, so the app says exactly the same.
    for line in each(tidied.get("said").unwrap_or(&Value::Null)).filter_map(Value::as_str) {
        writeln!(io.out, "{line}")?;
    }
    Ok(Code::Worked)
}

pub fn agents(io: &mut Io<'_>, _: &ArgMatches, access: &Value) -> Said_ {
    let folders = |name: &str| -> Vec<String> {
        each(access.get(name).unwrap_or(&Value::Null))
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect()
    };
    for (what, name) in [("look through", "lookIn"), ("back up to", "backUpTo")] {
        let list = folders(name);
        if list.is_empty() {
            writeln!(io.out, "An AI agent may {what} no folders.")?;
        } else {
            writeln!(io.out, "An AI agent may {what}:")?;
            for folder in list {
                writeln!(io.out, "  {folder}")?;
            }
        }
    }
    writeln!(
        io.out,
        "{}",
        if yes(access, "mayChange") {
            "It may make changes."
        } else {
            "It may only look, and change nothing."
        }
    )?;
    Ok(Code::Worked)
}

/// How a job ended, as a person reads it: what it said to standard output,
/// any problem to standard error.
pub fn last_word(io: &mut Io<'_>, last: Option<&Said>) -> std::io::Result<()> {
    match last {
        Some(Said::Send(e)) => {
            for line in &e.summary {
                writeln!(io.out, "{line}")?;
            }
            if let Some(problem) = &e.problem {
                writeln!(io.err, "{problem}")?;
            }
        }
        Some(Said::Scan(e)) => {
            if e.stopped {
                writeln!(
                    io.out,
                    "Stopped. Nothing from this search of {} was kept; what was known before is as it was.",
                    e.name
                )?;
            } else if e.problem.is_none() {
                writeln!(
                    io.out,
                    "Looked through {}: found {}.",
                    e.name,
                    photos_and_videos(
                        usize::try_from(e.photos).unwrap_or(usize::MAX),
                        usize::try_from(e.videos).unwrap_or(usize::MAX)
                    )
                )?;
            }
            if e.unreadable > 0 {
                writeln!(
                    io.out,
                    "{} couldn't be read, such as {}. The log lists them all.",
                    count(e.unreadable, "file"),
                    e.unreadable_names.join(", ")
                )?;
            }
            if let Some(online) = &e.online_only_said {
                writeln!(io.out, "{online}")?;
            }
            for left in &e.left_out {
                writeln!(io.out, "{}", left.said)?;
            }
            if let Some(problem) = &e.problem {
                writeln!(io.err, "{problem}")?;
            }
        }
        Some(Said::Surveyed(found)) => {
            writeln!(
                io.out,
                "Looked at {} in {} in {:.1} seconds, without opening any of them.",
                count(found.files, "file"),
                count(found.folders, "folder"),
                found.seconds
            )?;
            if found.hints.is_empty() {
                writeln!(io.out, "No photos or videos were found by name.")?;
            }
            for hint in &found.hints {
                let note = match (hint.already_a_source, hint.likely_photos) {
                    (true, _) => "  (a source already)",
                    (false, false) => "  (small pictures, probably not photos)",
                    (false, true) => "",
                };
                writeln!(
                    io.out,
                    "{:>9}  {}  {}{note}",
                    bytes(hint.bytes),
                    photos_and_videos(hint.photos, hint.videos),
                    hint.folder
                )?;
                if let Some(online) = &hint.online_only_said {
                    writeln!(io.out, "{:>9}  {online}", "")?;
                }
            }
            if found.unreadable > 0 {
                writeln!(
                    io.out,
                    "{} could not be listed, usually for want of permission.",
                    count(found.unreadable, "folder")
                )?;
            }
            for left in &found.left_out {
                writeln!(io.out, "{}", left.said)?;
            }
            if found.stopped {
                writeln!(io.out, "Stopped early, so there may be more.")?;
            }
        }
        Some(Said::Check(e)) => {
            writeln!(io.out, "Checked {} for duplicates.", count(e.done, "file"))?;
        }
        Some(Said::Survey(_)) => {}
        None => writeln!(
            io.err,
            "It ended without saying how it went, perhaps because its program was closed. The log may say more."
        )?,
    }
    Ok(())
}
