//! Every command, made from the API's register.
//!
//! Each row is one command as it is typed (`keevoro backups start`) and the
//! operations it calls. What a command does, in its help, is the register's
//! own sentence for its operation, and a command for a long operation is
//! given `--detach` because the register says it is a job. A test fails if
//! the register has an operation for the command line that no command
//! calls, so a new operation cannot be left out.
//!
//! A row only turns what was typed into the operation's request: the
//! operation itself is the API's, reached through `keevoro_api::call`, and
//! with `--json` its answer is printed exactly as the API gives it.

use crate::show;
use crate::{Code, Failure, Io};
use clap::{Arg, ArgAction, ArgMatches, Command};
use keevoro_api::{call, Keevoro, Op};
use serde_json::{json, Value};
use std::ffi::OsString;

/// What a command needs to make its request.
pub struct Ctx<'k> {
    pub keevoro: &'k Keevoro,
    /// The backup named with `-b`, if one was.
    pub backup: Option<String>,
    pub json: bool,
}

impl Ctx<'_> {
    /// Asks the API something on the way to the real request: the list a
    /// name is looked up in, say.
    pub fn ask(&self, op: Op, request: Value) -> Result<Value, Failure> {
        Ok(call(self.keevoro, op, self.backup.as_deref(), request)?)
    }
}

/// Turns what was typed into one operation and its request.
type Ask = fn(&Ctx<'_>, &ArgMatches, &mut Io<'_>) -> Result<(Op, Value), Failure>;
/// Says the answer in plain English.
type Show = fn(&mut Io<'_>, &ArgMatches, &Value) -> Result<Code, Failure>;

/// One command, as typed.
pub struct Row {
    pub words: &'static [&'static str],
    /// Every operation the command can call; the first says what it does.
    pub ops: &'static [Op],
    /// What it does, when the register's sentence for its operation is not
    /// the whole story.
    pub about: Option<&'static str>,
    pub args: fn(Command) -> Command,
    pub ask: Ask,
    pub show: Show,
}

impl Row {
    /// What the command does, for its help.
    pub fn about(&self) -> String {
        match self.about {
            Some(about) => about.to_string(),
            None => format!("{}.", self.ops[0].entry().does),
        }
    }

    /// True for a command that starts a job, from the register.
    pub fn starts_a_job(&self) -> bool {
        self.ops.iter().any(|op| op.entry().job)
    }

    pub fn typed(&self) -> String {
        format!("keevoro {}", self.words.join(" "))
    }
}

fn none(command: Command) -> Command {
    command
}

fn nothing(_: &Ctx<'_>, _: &ArgMatches, _: &mut Io<'_>, op: Op) -> Result<(Op, Value), Failure> {
    Ok((op, json!({})))
}

fn text(m: &ArgMatches, name: &str) -> Option<String> {
    m.get_one::<String>(name).cloned()
}

fn required(m: &ArgMatches, name: &str) -> String {
    // clap has already refused the command if a required argument is missing.
    text(m, name).unwrap_or_default()
}

fn on_off(value: &str) -> Result<bool, Failure> {
    match value.to_ascii_lowercase().as_str() {
        "on" | "yes" | "true" => Ok(true),
        "off" | "no" | "false" => Ok(false),
        _ => Err(Failure::not_understood(format!("Say on or off, not \"{value}\"."))),
    }
}

/// Is this the same place as `typed`? A place is compared as typed, and
/// again as the computer resolves it, so `D:` and `D:\` or a link to it are
/// recognised.
fn same_place(listed: &str, typed: &str) -> bool {
    let trim = |p: &str| p.trim_end_matches(['/', '\\']).to_string();
    if trim(listed) == trim(typed) {
        return true;
    }
    matches!(
        (std::fs::canonicalize(listed), std::fs::canonicalize(typed)),
        (Ok(a), Ok(b)) if a == b
    )
}

/// The one listed item that `matches` picks out, or a plain refusal.
fn one_of(
    list: &Value,
    matches: impl Fn(&Value) -> bool,
    none: impl FnOnce() -> String,
    several: impl FnOnce() -> String,
) -> Result<&Value, Failure> {
    let found: Vec<&Value> = list.as_array().into_iter().flatten().filter(|v| matches(v)).collect();
    match found.as_slice() {
        [one] => Ok(one),
        [] => Err(Failure::not_found(none())),
        _ => Err(Failure::not_understood(several())),
    }
}

fn field<'v>(v: &'v Value, name: &str) -> &'v str {
    v.get(name).and_then(Value::as_str).unwrap_or_default()
}

/// A source by its number, its place or its name.
fn source(ctx: &Ctx<'_>, typed: &str) -> Result<i64, Failure> {
    let list = ctx.ask(Op::SourcesList, Value::Null)?;
    let by_number = typed.trim().parse::<i64>().ok();
    let found = one_of(
        &list,
        |s| {
            s.get("id").and_then(Value::as_i64) == by_number
                || same_place(field(s, "root"), typed)
                || field(s, "name").eq_ignore_ascii_case(typed.trim())
        },
        || format!("This backup has no source called \"{typed}\". `keevoro sources` lists them."),
        || format!("More than one source is called \"{typed}\". Give its number, as `keevoro sources` lists them."),
    )?;
    Ok(found.get("id").and_then(Value::as_i64).unwrap_or_default())
}

/// A collection by its whole name, or its title when only one has it.
fn collection(ctx: &Ctx<'_>, typed: &str) -> Result<String, Failure> {
    let list = ctx.ask(Op::CollectionsList, Value::Null)?;
    let typed = typed.trim();
    let whole = one_of(
        &list,
        |c| field(c, "key").eq_ignore_ascii_case(typed),
        String::new,
        String::new,
    );
    let found = match whole {
        Ok(found) => found,
        Err(_) => one_of(
            &list,
            |c| field(c, "title").eq_ignore_ascii_case(typed),
            || format!("This backup has no collection called \"{typed}\". `keevoro collections` lists them."),
            || {
                format!(
                    "More than one collection is called \"{typed}\". Give its whole name, as `keevoro collections` lists them."
                )
            },
        )?,
    };
    Ok(field(found, "key").to_string())
}

/// A destination by its number in the list, its folder or its name.
fn destination(ctx: &Ctx<'_>, typed: &str) -> Result<usize, Failure> {
    let list = ctx.ask(Op::DestinationsList, Value::Null)?;
    let typed = typed.trim();
    if let Ok(n) = typed.parse::<usize>() {
        let there = list.as_array().map_or(0, Vec::len);
        return match n {
            1.. if n <= there => Ok(n - 1),
            _ => Err(Failure::not_found(format!(
                "This backup has no destination number {n}. `keevoro destinations` lists them."
            ))),
        };
    }
    let found = one_of(
        &list,
        |d| {
            let name = field(d, "name");
            d.get("folder")
                .and_then(Value::as_str)
                .is_some_and(|folder| same_place(folder, typed))
                || name.eq_ignore_ascii_case(typed)
                || name.ends_with(&format!(" {typed}"))
        },
        || format!("This backup doesn't go to \"{typed}\". `keevoro destinations` lists where it goes."),
        || {
            format!(
                "More than one destination matches \"{typed}\". Give its number, as `keevoro destinations` lists them."
            )
        },
    )?;
    Ok(list
        .as_array()
        .and_then(|all| all.iter().position(|d| d == found))
        .unwrap_or_default())
}

fn job(m: &ArgMatches) -> Result<Value, Failure> {
    match text(m, "job") {
        None => Ok(Value::Null),
        Some(n) => n.trim().parse::<u64>().map(Value::from).map_err(|_| {
            Failure::not_understood(format!("A job is a number, as `keevoro status` shows it, not \"{n}\"."))
        }),
    }
}

fn arg(name: &'static str, help: &'static str) -> Arg {
    Arg::new(name).help(help)
}

fn flag(name: &'static str, help: &'static str) -> Arg {
    Arg::new(name).long(name).action(ArgAction::SetTrue).help(help)
}

fn job_arg(command: Command) -> Command {
    command.arg(arg(
        "job",
        "One job, by the number `keevoro status` shows; every one when left out",
    ))
}

/// Every command. A test checks that every operation the register offers
/// the command line is here.
pub static ROWS: &[Row] = &[
    // Backups
    Row {
        words: &["backups"],
        ops: &[Op::BackupsList],
        about: None,
        args: none,
        ask: |c, m, io| nothing(c, m, io, Op::BackupsList),
        show: show::backups,
    },
    Row {
        words: &["backups", "start"],
        ops: &[Op::BackupsStart],
        about: None,
        args: |c| {
            c.arg(arg("name", "What to call it, such as \"Holiday videos\"").required(true))
                .arg(
                    Arg::new("wants")
                        .long("wants")
                        .value_name("KINDS")
                        .num_args(1..)
                        .help(format!("What it is for, when not everything Keevoro looks for: {WANTS_HELP}")),
                )
                .arg(
                    Arg::new("arrange")
                        .long("arrange")
                        .value_name("HOW")
                        .help("How it arranges files where it sends them: by-date or folders"),
                )
        },
        ask: |_, m, _| {
            let arrange = text(m, "arrange").map(|w| arrange_typed(&w)).transpose()?;
            Ok((
                Op::BackupsStart,
                json!({ "name": required(m, "name"), "wants": wants_typed(m, "wants"), "arrange": arrange }),
            ))
        },
        show: show::started,
    },
    Row {
        words: &["backups", "find-at"],
        ops: &[Op::BackupsFindAt],
        about: Some(
            "Looks for a Keevoro backup in a folder or in cloud storage, from the notes Keevoro keeps beside every backup, and says what it holds. Changes nothing. Open it with `keevoro backups open-from`.",
        ),
        args: crate::keys::args,
        ask: |_, m, io| Ok((Op::BackupsFindAt, crate::keys::request(m, io)?)),
        show: show::found_at,
    },
    Row {
        words: &["backups", "open-from"],
        ops: &[Op::BackupsOpenFrom],
        about: Some(
            "Opens a backup that is already somewhere (on a new computer, or after a reinstall) from Keevoro's notes there, as a backup on this computer going to that place. Nothing there is sent again. Keys go to this computer's keychain and are never shown or written down.",
        ),
        args: crate::keys::args,
        ask: |_, m, io| Ok((Op::BackupsOpenFrom, crate::keys::request(m, io)?)),
        show: show::opened_from,
    },
    Row {
        words: &["backups", "rename"],
        ops: &[Op::BackupsRename],
        about: None,
        args: |c| {
            c.arg(arg("name", "The backup's name now").required(true))
                .arg(arg("to", "Its new name").required(true))
        },
        ask: |_, m, _| {
            Ok((
                Op::BackupsRename,
                json!({ "backup": required(m, "name"), "to": required(m, "to") }),
            ))
        },
        show: show::renamed,
    },
    Row {
        words: &["backups", "forget"],
        ops: &[Op::BackupsForget],
        about: None,
        args: |c| {
            c.arg(arg("name", "The backup's name").required(true)).arg(flag(
                "dry-run",
                "Only say what removing it would do, and change nothing",
            ))
        },
        ask: |_, m, _| {
            Ok((
                Op::BackupsForget,
                json!({ "backup": required(m, "name"), "dryRun": m.get_flag("dry-run") }),
            ))
        },
        show: show::forgot,
    },
    Row {
        words: &["backups", "removed"],
        ops: &[Op::BackupsRemoved],
        about: None,
        args: none,
        ask: |c, m, io| nothing(c, m, io, Op::BackupsRemoved),
        show: show::removed_backups,
    },
    Row {
        words: &["backups", "put-back"],
        ops: &[Op::BackupsPutBack],
        about: None,
        args: |c| c.arg(arg("name", "The removed backup's name, as `keevoro backups removed` lists it").required(true)),
        ask: |_, m, _| Ok((Op::BackupsPutBack, json!({ "backup": required(m, "name") }))),
        show: show::put_back,
    },
    Row {
        words: &["backups", "clear-away"],
        ops: &[Op::BackupsClearAway],
        about: None,
        args: |c| c.arg(arg("name", "The removed backup's name, as `keevoro backups removed` lists it").required(true)),
        ask: |_, m, _| Ok((Op::BackupsClearAway, json!({ "backup": required(m, "name") }))),
        show: show::cleared_away,
    },
    // Finding
    Row {
        words: &["places"],
        ops: &[Op::PlacesList],
        about: None,
        args: none,
        ask: |c, m, io| nothing(c, m, io, Op::PlacesList),
        show: show::places,
    },
    Row {
        words: &["survey"],
        ops: &[Op::PlacesSurvey],
        about: None,
        args: |c| c.arg(arg("place", "A drive or folder").required(true)),
        ask: |_, m, _| Ok((Op::PlacesSurvey, json!({ "place": required(m, "place") }))),
        show: show::nothing_more,
    },
    Row {
        words: &["folders"],
        ops: &[Op::PlacesFolders],
        about: Some(
            "The folders inside a place that hold photos or videos themselves, with how many each holds, from what `keevoro survey` found there. Nothing is added or changed.",
        ),
        args: |c| c.arg(arg("place", "A drive or folder that was surveyed").required(true)),
        ask: |_, m, _| Ok((Op::PlacesFolders, json!({ "place": required(m, "place") }))),
        show: show::folders,
    },
    Row {
        words: &["files"],
        ops: &[Op::PlacesFiles],
        about: Some(
            "The photos and videos directly in one folder, a page at a time, without adding anything, with how long each video lasts when it says. Save a picture of one with `keevoro picture --file`.",
        ),
        args: |c| {
            c.arg(arg("folder", "The folder").required(true))
                .arg(
                    Arg::new("from")
                        .long("from")
                        .value_name("N")
                        .help("How many to pass over first; 0 when left out")
                        .value_parser(clap::value_parser!(usize)),
                )
                .arg(
                    Arg::new("count")
                        .long("count")
                        .value_name("N")
                        .help("How many to list, at most 500; 100 when left out")
                        .value_parser(clap::value_parser!(usize)),
                )
                .arg(
                    Arg::new("sort")
                        .long("sort")
                        .value_name("ORDER")
                        .help("name (the default), or newest or oldest by the file's date, which is when it was last changed, not when it was taken")
                        .value_parser(["name", "newest", "oldest"]),
                )
                .arg(flag("photos", "Only the photos").conflicts_with_all(["videos", "others"]))
                .arg(flag("videos", "Only the videos").conflicts_with("others"))
                .arg(flag("others", "Only the other files you asked Keevoro to carry along"))
        },
        ask: |_, m, _| {
            let only = ["photos", "videos", "others"]
                .into_iter()
                .find(|kind| m.get_flag(kind))
                .unwrap_or("all");
            Ok((
                Op::PlacesFiles,
                json!({
                    "place": required(m, "folder"),
                    "from": m.get_one::<usize>("from").copied().unwrap_or(0),
                    "count": m.get_one::<usize>("count").copied().unwrap_or(100),
                    "sort": m.get_one::<String>("sort").map_or("name", String::as_str),
                    "only": only,
                }),
            ))
        },
        show: show::files,
    },
    Row {
        words: &["map"],
        ops: &[Op::PlacesMap],
        about: Some(
            "Where the space goes in a place `keevoro survey` has surveyed: every folder and the biggest files, photos, videos and everything else, by size, from what the survey saw. Nothing is opened, added or changed.",
        ),
        args: |c| {
            c.arg(arg("place", "A drive or folder that was surveyed").required(true))
                .arg(
                    Arg::new("folder")
                        .long("folder")
                        .value_name("FOLDER")
                        .help("A folder inside the place to show instead of the whole of it"),
                )
                .arg(
                    Arg::new("depth")
                        .long("depth")
                        .value_name("N")
                        .help("How many levels of folders to show, 1 to 6; 3 when left out")
                        .value_parser(clap::value_parser!(u8).range(1..=6)),
                )
                .arg(flag("kinds", "List the kinds of file by ending instead of the folders"))
        },
        ask: |_, m, _| {
            let mut ask = json!({
                "place": required(m, "place"),
                "depth": m.get_one::<u8>("depth").copied().unwrap_or(3),
            });
            if let Some(folder) = text(m, "folder") {
                ask["folder"] = json!(folder);
            }
            Ok((Op::PlacesMap, ask))
        },
        show: show::space_map,
    },
    Row {
        words: &["forget-maps"],
        ops: &[Op::PlacesForgetMaps],
        about: Some(
            "Forgets every survey Keevoro remembers, and so every map of where the space goes, since a map lists folder names across a whole drive. Nothing on any drive is touched; a place is simply surveyed again when next asked about.",
        ),
        args: none,
        ask: |c, m, io| nothing(c, m, io, Op::PlacesForgetMaps),
        show: show::maps_forgotten,
    },
    Row {
        words: &["sources"],
        ops: &[Op::SourcesList],
        about: None,
        args: none,
        ask: |_, _, _| Ok((Op::SourcesList, Value::Null)),
        show: show::sources,
    },
    Row {
        words: &["look"],
        ops: &[Op::SourcesLook, Op::SourcesLookAgain],
        about: Some(
            "Looks through a drive or folder and adds it as a source. With --again, looks through one source, or all of them, for what is new.",
        ),
        args: |c| {
            c.arg(arg("place", "A drive or folder; with --again, one of the sources").required_unless_present("again"))
                .arg(flag("again", "Look through the sources again for what is new"))
                .arg(
                    arg(
                        "different-drive",
                        "When the drive at a source's place is not the one looked through there before: anyway, to look through it as that source, or new, to keep the source as it was and add the drive as a new source",
                    )
                    .long("different-drive")
                    .value_parser(["anyway", "new"])
                    .requires("place")
                    .requires("again"),
                )
        },
        ask: |c, m, _| {
            if m.get_flag("again") {
                let source = text(m, "place").map(|typed| source(c, &typed)).transpose()?;
                let mut ask = json!({ "source": source });
                match text(m, "different-drive").as_deref() {
                    Some("anyway") => ask["differentDrive"] = json!("lookAnyway"),
                    Some("new") => ask["differentDrive"] = json!("addAsNew"),
                    _ => {}
                }
                Ok((Op::SourcesLookAgain, ask))
            } else {
                Ok((Op::SourcesLook, json!({ "place": required(m, "place") })))
            }
        },
        show: show::nothing_more,
    },
    Row {
        words: &["sources", "connect-again"],
        ops: &[Op::SourcesConnectAgain],
        about: Some(
            "Says where a source of a backup opened from its notes is on this computer. Keevoro never guesses: only you know where these photos live here. Then look through it again with `keevoro look --again`.",
        ),
        args: |c| {
            c.arg(arg("source", "The source's number, place or name").required(true))
                .arg(arg("place", "The drive or folder where its files are on this computer").required(true))
        },
        ask: |c, m, _| {
            Ok((
                Op::SourcesConnectAgain,
                json!({ "source": source(c, &required(m, "source"))?, "place": required(m, "place") }),
            ))
        },
        show: show::connected_again,
    },
    Row {
        words: &["sources", "forget"],
        ops: &[Op::SourcesForget],
        about: None,
        args: |c| c.arg(arg("source", "The source's number, place or name").required(true)),
        ask: |c, m, _| Ok((Op::SourcesForget, json!({ "source": source(c, &required(m, "source"))? }))),
        show: show::forgot_source,
    },
    Row {
        words: &["sources", "types"],
        ops: &[Op::SourcesFileTypes, Op::SourcesIncludeType],
        about: Some(
            "The kinds of file on a source, and which are included. With --leave-out or --include, changes one. Nothing on the source is touched.",
        ),
        args: |c| {
            c.arg(arg("source", "The source's number, place or name").required(true))
                .arg(
                    Arg::new("leave-out")
                        .long("leave-out")
                        .value_name("ENDING")
                        .help("Leave out every file with this ending, such as png")
                        .conflicts_with("include"),
                )
                .arg(
                    Arg::new("include")
                        .long("include")
                        .value_name("ENDING")
                        .help("Include files with this ending again"),
                )
        },
        ask: |c, m, _| {
            let source = source(c, &required(m, "source"))?;
            let change = match (text(m, "leave-out"), text(m, "include")) {
                (Some(ending), _) => Some((ending, false)),
                (None, Some(ending)) => Some((ending, true)),
                (None, None) => None,
            };
            Ok(match change {
                None => (Op::SourcesFileTypes, json!({ "source": source })),
                Some((ending, included)) => (
                    Op::SourcesIncludeType,
                    json!({ "source": source, "ending": ending, "included": included }),
                ),
            })
        },
        show: show::file_types,
    },
    Row {
        words: &["sources", "leave-out"],
        ops: &[Op::SourcesLeaveOut],
        about: Some(
            "Leaves a folder out of this backup: it is not looked through, and what was found in it is no longer offered. With --put-back, includes it again. Nothing in the folder, and nothing already backed up, is touched.",
        ),
        args: |c| {
            c.arg(arg("folder", "The folder, which need not be in a source yet").required(true))
                .arg(flag("put-back", "Put the folder back, so it is looked through again"))
        },
        ask: |_, m, _| {
            Ok((
                Op::SourcesLeaveOut,
                json!({ "folder": required(m, "folder"), "leftOut": !m.get_flag("put-back") }),
            ))
        },
        show: show::said,
    },
    Row {
        words: &["sources", "on-arrival"],
        ops: &[Op::SourcesOnArrival],
        about: Some(
            "Whether a source (a card or a drive) is backed up by itself whenever it is plugged in, while the Keevoro app is running. Give on or off to change it; it is off unless you turn it on. Keevoro waits a few seconds after it is plugged in, backs it up at most once in ten minutes, never while paused, and never a different drive at the same place.",
        ),
        args: |c| {
            c.arg(arg("source", "The source's number, place or name").required(true))
                .arg(arg("on", "on or off").value_parser(["on", "off"]))
        },
        ask: |c, m, _| {
            let source = source(c, &required(m, "source"))?;
            let on = text(m, "on").map(|w| w == "on");
            Ok((Op::SourcesOnArrival, json!({ "source": source, "on": on })))
        },
        show: show::on_arrival,
    },
    Row {
        words: &["sources", "wants"],
        ops: &[Op::SourcesWants],
        about: Some(
            "What one source is for: the same as its backup, or its own choice of photos, videos and other files. Give backup, or one or more of photos, videos and others, to change it.",
        ),
        args: |c| {
            c.arg(arg("source", "The source's number, place or name").required(true))
                .arg(
                    Arg::new("wants")
                        .value_name("KINDS")
                        .num_args(1..)
                        .help(format!("backup, or {WANTS_HELP}")),
                )
        },
        ask: |c, m, _| {
            let source = source(c, &required(m, "source"))?;
            let wants = wants_typed(m, "wants").map(|words| {
                if words.iter().any(|w| w == "backup") {
                    json!("backup")
                } else {
                    json!(words)
                }
            });
            Ok((Op::SourcesWants, json!({ "source": source, "wants": wants })))
        },
        show: show::wants,
    },
    Row {
        words: &["sources", "arrange"],
        ops: &[Op::SourcesArrange],
        about: Some(
            "How one source's photos and videos are arranged where they are sent: the same as its backup, by date, or keeping its folders as they are. Give backup, by-date or folders to change it; nothing already backed up is moved.",
        ),
        args: |c| {
            c.arg(arg("source", "The source's number, place or name").required(true))
                .arg(arg("how", "backup, by-date or folders"))
        },
        ask: |c, m, _| {
            let source = source(c, &required(m, "source"))?;
            let arrange = text(m, "how")
                .map(|w| {
                    if w.eq_ignore_ascii_case("backup") {
                        Ok("backup")
                    } else {
                        arrange_typed(&w)
                    }
                })
                .transpose()?;
            Ok((Op::SourcesArrange, json!({ "source": source, "arrange": arrange })))
        },
        show: show::arrange,
    },
    // Choosing
    Row {
        words: &["found"],
        ops: &[Op::FoundSummary],
        about: None,
        args: none,
        ask: |_, _, _| Ok((Op::FoundSummary, Value::Null)),
        show: show::found,
    },
    Row {
        words: &["collections"],
        ops: &[Op::CollectionsList],
        about: None,
        args: none,
        ask: |_, _, _| Ok((Op::CollectionsList, Value::Null)),
        show: show::collections,
    },
    Row {
        words: &["photos"],
        ops: &[Op::CollectionsPhotos],
        about: None,
        args: |c| c.arg(arg("collection", "The collection, as `keevoro collections` lists it").required(true)),
        ask: |c, m, _| {
            Ok((
                Op::CollectionsPhotos,
                json!({ "collection": collection(c, &required(m, "collection"))? }),
            ))
        },
        show: show::photos,
    },
    Row {
        words: &["keep"],
        ops: &[Op::CollectionsKeep, Op::PhotosKeep],
        about: Some("Keeps a whole collection, or one photo or video with --photo, so that it is backed up."),
        args: choose_args,
        ask: |c, m, _| choose(c, m, true),
        show: show::kept,
    },
    Row {
        words: &["skip"],
        ops: &[Op::CollectionsKeep, Op::PhotosKeep],
        about: Some(
            "Leaves out a whole collection, or one photo or video with --photo. Leaving out only means it is not backed up; nothing is deleted.",
        ),
        args: choose_args,
        ask: |c, m, _| choose(c, m, false),
        show: show::left_out,
    },
    Row {
        words: &["set-aside"],
        ops: &[Op::SetAsideList],
        about: None,
        args: none,
        ask: |_, _, _| Ok((Op::SetAsideList, Value::Null)),
        show: show::set_aside,
    },
    Row {
        words: &["duplicates"],
        ops: &[Op::DuplicatesList],
        about: None,
        args: none,
        ask: |_, _, _| Ok((Op::DuplicatesList, Value::Null)),
        show: show::duplicates,
    },
    Row {
        words: &["check-duplicates"],
        ops: &[Op::DuplicatesCheck],
        about: None,
        args: none,
        ask: |c, m, io| nothing(c, m, io, Op::DuplicatesCheck),
        show: show::nothing_more,
    },
    Row {
        words: &["picture"],
        ops: &[Op::PhotosPicture],
        about: Some(
            "Saves a small or screen-sized picture of one photo or video, as a WebP file: one found, by its code, or any in a folder, by its path with --file. The file is only read.",
        ),
        args: |c| {
            c.arg(arg("photo", "The photo's code, as `keevoro photos` lists it").required_unless_present("file"))
                .arg(
                    Arg::new("file")
                        .long("file")
                        .value_name("PATH")
                        .conflicts_with("photo")
                        .help("Any photo or video, by its path, as `keevoro files` lists it"),
                )
                .arg(flag("large", "Screen-sized, not small"))
                .arg(
                    Arg::new("to")
                        .long("to")
                        .value_name("FILE")
                        .help("Where to save it; needed unless --json"),
                )
        },
        ask: |c, m, _| {
            if !c.json && text(m, "to").is_none() {
                return Err(Failure::not_understood(
                    "Say where to save the picture, with --to, such as --to look.webp.",
                ));
            }
            let size = if m.get_flag("large") { "screen" } else { "small" };
            let request = match text(m, "file") {
                Some(file) => json!({ "file": file, "size": size }),
                None => json!({ "photo": required(m, "photo"), "size": size }),
            };
            Ok((Op::PhotosPicture, request))
        },
        show: show::picture,
    },
    Row {
        words: &["sources", "add-bucket"],
        ops: &[Op::SourcesAddBucket],
        about: Some(
            "Adds cloud storage as a source: a bucket, or a folder in it with --prefix. Its keys are tested by listing, which writes nothing, and go to this computer's keychain. Nothing in the bucket is ever changed. Then look through it with: keevoro look --again NAME.",
        ),
        args: crate::keys::bucket_args,
        ask: |_, m, io| Ok((Op::SourcesAddBucket, crate::keys::bucket_request(m, io)?)),
        show: show::bucket_added,
    },
    // Destinations
    Row {
        words: &["destinations"],
        ops: &[Op::DestinationsList],
        about: None,
        args: none,
        ask: |_, _, _| Ok((Op::DestinationsList, Value::Null)),
        show: show::destinations,
    },
    Row {
        words: &["destinations", "add"],
        ops: &[Op::DestinationsAdd],
        about: Some(
            "Checks a place works, then adds it: a folder, or cloud storage with its keys. Keys go to this computer's keychain and are never shown or written down.",
        ),
        args: crate::keys::args,
        ask: |_, m, io| Ok((Op::DestinationsAdd, crate::keys::request(m, io)?)),
        show: show::added,
    },
    Row {
        words: &["destinations", "remove"],
        ops: &[Op::DestinationsRemove],
        about: None,
        args: |c| {
            c.arg(arg("destination", "Its number, folder or name, as `keevoro destinations` lists it").required_unless_present("all"))
                .arg(flag("all", "Every destination").conflicts_with("destination"))
        },
        ask: |c, m, _| {
            let request = match text(m, "destination") {
                Some(typed) => json!({ "at": destination(c, &typed)? }),
                None => json!({ "all": true }),
            };
            Ok((Op::DestinationsRemove, request))
        },
        show: show::removed,
    },
    Row {
        words: &["tidy"],
        ops: &[Op::DestinationsTidy],
        about: Some(
            "Clears away the unfinished copies an earlier backup that was stopped left in a backup folder (Keevoro's working folder, .keevoro-uploads), whether or not a backup still goes there. Nothing that was backed up is touched. A backup to the folder does this by itself.",
        ),
        args: |c| {
            c.arg(arg("folder", "The backup folder").required(true))
                .arg(flag("dry-run", "Only say what would be cleared, and clear nothing"))
        },
        ask: |_, m, _| {
            Ok((
                Op::DestinationsTidy,
                json!({ "place": required(m, "folder"), "dryRun": m.get_flag("dry-run") }),
            ))
        },
        show: show::tidied,
    },
    Row {
        words: &["room"],
        ops: &[Op::DestinationsRoom],
        about: None,
        args: none,
        ask: |c, m, io| nothing(c, m, io, Op::DestinationsRoom),
        show: show::room,
    },
    // Backing up
    Row {
        words: &["rehearse"],
        ops: &[Op::BackupRehearse],
        about: None,
        args: none,
        ask: |c, m, io| nothing(c, m, io, Op::BackupRehearse),
        show: show::rehearsal,
    },
    Row {
        words: &["run"],
        ops: &[Op::BackupRun, Op::ScheduleRun],
        about: Some(
            "Backs up to every destination, originals first. With --unattended, runs as a schedule does, for a system scheduler: looks through the sources again, holds back what needs your say, waits while this computer is on battery or busy, and sends what is new.",
        ),
        args: |c| {
            c.arg(flag(
                "unattended",
                "Run by itself, with nobody watching: hold back what needs your say, and never start on battery or while the computer is busy",
            ))
            .arg(
                flag(
                    "if-due",
                    "With --unattended, only when the backup's schedule says a run is due: once however many were missed, and never twice with the app",
                )
                .requires("unattended"),
            )
            .arg(
                flag(
                    "anyway",
                    "With --unattended, start now without waiting for a good moment: not on battery, a metered connection, a busy computer or the hours chosen",
                )
                .requires("unattended"),
            )
            .arg(
                arg("wait", "With --unattended, how many minutes to wait at most for a good moment; none by default")
                    .long("wait")
                    .value_name("MINUTES")
                    .value_parser(clap::value_parser!(u32))
                    .requires("unattended"),
            )
        },
        ask: |_, m, _| {
            if m.get_flag("unattended") {
                let mut ask = json!({});
                if let Some(minutes) = m.get_one::<u32>("wait") {
                    ask["waitMinutes"] = json!(minutes);
                }
                if m.get_flag("if-due") {
                    ask["ifDue"] = json!(true);
                }
                if m.get_flag("anyway") {
                    ask["anyway"] = json!(true);
                }
                Ok((Op::ScheduleRun, ask))
            } else {
                Ok((Op::BackupRun, json!({})))
            }
        },
        show: show::nothing_more,
    },
    Row {
        words: &["check"],
        ops: &[Op::BackupCheck],
        about: None,
        args: none,
        ask: |c, m, io| nothing(c, m, io, Op::BackupCheck),
        show: show::nothing_more,
    },
    Row {
        words: &["repair"],
        ops: &[Op::BackupPutRight],
        about: Some(
            "Puts this backup right, as the app's \"Put this backup right\" does: reads back every file at each destination, moves any damaged copy aside there into \"Set aside by Keevoro\" (never deleting anything), sends those and any missing again, and proves each. Over a network share this can take hours; it can be stopped and run again, and carries on from where it was. With --quick, only what a look or a check already found wrong is put right. To see what it would do first: keevoro repair plan.",
        ),
        args: |c| c.arg(flag("quick", "Only put right what Keevoro already knows is wrong, which takes minutes")),
        ask: |_, m, _| Ok((Op::BackupPutRight, json!({ "thorough": !m.get_flag("quick") }))),
        show: show::nothing_more,
    },
    Row {
        words: &["repair", "plan"],
        ops: &[Op::BackupPutRightPlan],
        about: None,
        args: none,
        ask: |c, m, io| nothing(c, m, io, Op::BackupPutRightPlan),
        show: show::put_right_plan,
    },
    Row {
        words: &["schedule"],
        ops: &[Op::ScheduleShow],
        about: None,
        args: none,
        ask: |c, m, io| nothing(c, m, io, Op::ScheduleShow),
        show: show::schedule,
    },
    Row {
        words: &["schedule", "set"],
        ops: &[Op::ScheduleSet],
        about: Some(
            "Sets when the backup backs up by itself, in this computer's own time: daily 02:30, weekly sun 09:00, monthly 31 02:30 (the last day of shorter months), or off. pause and carry-on pause it and carry on; carrying on never catches up what was missed while paused. Saving never runs it at once. Keevoro runs it while the app is open; for your own scheduler, see keevoro run --unattended --if-due.",
        ),
        args: |c| {
            c.arg(
                arg("how", "daily, weekly, monthly, off, pause or carry-on")
                    .required(true)
                    .value_parser(["daily", "weekly", "monthly", "off", "pause", "carry-on"]),
            )
            .arg(arg("first", "For daily, the time; for weekly, the day (mon to sun); for monthly, the day of the month (1 to 31)"))
            .arg(arg("second", "For weekly and monthly, the time, such as 02:30"))
        },
        ask: |_, m, _| {
            let first = text(m, "first");
            let second = text(m, "second");
            let wanted = |what: Option<String>, name: &str| {
                what.ok_or_else(|| Failure::not_understood(format!("give the {name} too")))
            };
            let ask = match required(m, "how").as_str() {
                "daily" => json!({ "every": "day", "at": wanted(first, "time, such as 02:30")? }),
                "weekly" => json!({
                    "every": "week",
                    "weekday": wanted(first, "day of the week, such as sun")?,
                    "at": wanted(second, "time, such as 09:00")?,
                }),
                "monthly" => {
                    let day: u8 = wanted(first, "day of the month, such as 31")?
                        .parse()
                        .map_err(|_| Failure::not_understood("the day of the month should be 1 to 31"))?;
                    json!({ "every": "month", "day": day, "at": wanted(second, "time, such as 02:30")? })
                }
                "off" => json!({ "every": "off" }),
                "pause" => json!({ "paused": true }),
                _ => json!({ "paused": false }),
            };
            Ok((Op::ScheduleSet, ask))
        },
        show: show::schedule,
    },
    Row {
        words: &["schedule", "skip"],
        ops: &[Op::ScheduleSkipNext],
        about: None,
        args: |c| c.arg(flag("undo", "Stop skipping it")),
        ask: |_, m, _| Ok((Op::ScheduleSkipNext, json!({ "undo": m.get_flag("undo") }))),
        show: show::schedule,
    },
    Row {
        words: &["history"],
        ops: &[Op::BackupHistory],
        about: Some(
            "How earlier backups went, newest first. Give a backup's number, as the list shows it, for everything recorded about it: each stage with its time and speed, what each destination was sent and what could not be, the previews, and what it said at the end.",
        ),
        args: |c| {
            c.arg(
                arg("run", "One backup, by its number in the list (1 is the newest)")
                    .value_parser(clap::value_parser!(usize)),
            )
        },
        ask: |_, m, _| {
            Ok((
                Op::BackupHistory,
                json!({ "run": m.get_one::<usize>("run").copied() }),
            ))
        },
        show: show::history,
    },
    Row {
        words: &["last-backup"],
        ops: &[Op::BackupLastSummary],
        about: None,
        args: none,
        ask: |_, _, _| Ok((Op::BackupLastSummary, Value::Null)),
        show: show::last_backup,
    },
    Row {
        words: &["previews"],
        ops: &[Op::BackupPreviews],
        about: Some(
            "Whether this backup also makes previews for a gallery, or sends the originals alone. Give on or off to change it.",
        ),
        args: |c| c.arg(arg("wanted", "on or off")),
        ask: |_, m, _| {
            let wanted = text(m, "wanted").map(|w| on_off(&w)).transpose()?;
            Ok((Op::BackupPreviews, json!({ "wanted": wanted })))
        },
        show: show::previews,
    },
    Row {
        words: &["wants"],
        ops: &[Op::BackupWants],
        about: Some(
            "What this backup is for: photos, videos and, once a kind of other file is on the list, other files. Give one or more of them to change it; it counts at once, and nothing already backed up is touched.",
        ),
        args: |c| c.arg(Arg::new("wants").value_name("KINDS").num_args(1..).help(WANTS_HELP)),
        ask: |_, m, _| Ok((Op::BackupWants, json!({ "wants": wants_typed(m, "wants") }))),
        show: show::wants,
    },
    Row {
        words: &["arrange"],
        ops: &[Op::BackupArrange],
        about: Some(
            "How this backup arranges photos and videos where it sends them: by date, or keeping your folders as they are. Give by-date or folders to change it; it only changes where files go from now on, and nothing already backed up is moved.",
        ),
        args: |c| c.arg(arg("how", "by-date or folders")),
        ask: |_, m, _| {
            let arrange = text(m, "how").map(|w| arrange_typed(&w)).transpose()?;
            Ok((Op::BackupArrange, json!({ "arrange": arrange })))
        },
        show: show::arrange,
    },
    // Jobs
    Row {
        words: &["status"],
        ops: &[Op::JobsStatus],
        about: None,
        args: job_arg,
        ask: |_, m, _| Ok((Op::JobsStatus, json!({ "job": job(m)? }))),
        show: show::status,
    },
    Row {
        words: &["follow"],
        ops: &[Op::JobsFollow],
        about: None,
        args: job_arg,
        ask: |_, m, _| Ok((Op::JobsFollow, json!({ "job": job(m)? }))),
        show: show::nothing_more,
    },
    Row {
        words: &["pause"],
        ops: &[Op::JobsPause],
        about: None,
        args: |c| {
            c.arg(arg(
                "job",
                "Only the check for duplicates, by the number `keevoro status` shows; everything when left out",
            ))
        },
        ask: |_, m, _| Ok((Op::JobsPause, json!({ "job": job(m)? }))),
        show: show::paused,
    },
    Row {
        words: &["carry-on"],
        ops: &[Op::JobsCarryOn],
        about: None,
        args: |c| {
            c.arg(arg(
                "job",
                "Only a check for duplicates paused on its own, by its number; everything when left out",
            ))
        },
        ask: |_, m, _| Ok((Op::JobsCarryOn, json!({ "job": job(m)? }))),
        show: show::paused,
    },
    Row {
        words: &["stop"],
        ops: &[Op::JobsStop],
        about: None,
        args: job_arg,
        ask: |_, m, _| Ok((Op::JobsStop, json!({ "job": job(m)? }))),
        show: show::stopped,
    },
    // Keevoro itself
    Row {
        words: &["about"],
        ops: &[Op::KeevoroAbout],
        about: None,
        args: none,
        ask: |c, m, io| nothing(c, m, io, Op::KeevoroAbout),
        show: show::about,
    },
    Row {
        words: &["background"],
        ops: &[Op::BackgroundSettings],
        about: Some(
            "How a backup by itself waits for a good moment: while this computer is on battery and less than half charged, on a metered connection (for cloud storage; only Windows can tell), and outside the hours you choose. With nothing given, says how it is now.",
        ),
        args: |c| {
            c.arg(
                arg("wait-on-battery", "on or off")
                    .long("wait-on-battery")
                    .value_parser(["on", "off"]),
            )
            .arg(
                arg("wait-on-metered", "on or off")
                    .long("wait-on-metered")
                    .value_parser(["on", "off"]),
            )
            .arg(
                arg("between", "Only start between these times, such as --between 22:00 07:00")
                    .long("between")
                    .num_args(2)
                    .value_names(["FROM", "TO"]),
            )
            .arg(flag("any-hour", "Start at any hour").conflicts_with("between"))
        },
        ask: |_, m, _| {
            let switch = |name: &str| text(m, name).map(|w| w == "on");
            let between = m
                .get_many::<String>("between")
                .map(|times| times.cloned().collect::<Vec<_>>())
                .map(|t| json!({ "from": t[0], "to": t[1] }));
            Ok((
                Op::BackgroundSettings,
                json!({
                    "waitOnBattery": switch("wait-on-battery"),
                    "waitOnMetered": switch("wait-on-metered"),
                    "between": between,
                    "anyHour": m.get_flag("any-hour").then_some(true),
                }),
            ))
        },
        show: show::background,
    },
    Row {
        words: &["away"],
        ops: &[Op::ScheduleAway],
        about: Some(
            "What every backup did by itself over the last few days, as the app's \"While you were away\" says it: each run, what it backed up or why it couldn't, runs that were missed because the computer was off and were caught up, and what waits for your say.",
        ),
        args: |c| {
            c.arg(
                Arg::new("days")
                    .long("days")
                    .value_name("N")
                    .help("How many days back; 7 when left out")
                    .value_parser(clap::value_parser!(u32)),
            )
        },
        ask: |_, m, _| Ok((Op::ScheduleAway, json!({ "days": m.get_one::<u32>("days") }))),
        show: show::away,
    },
    Row {
        words: &["activity"],
        ops: &[Op::KeevoroActivity],
        about: None,
        args: none,
        ask: |c, m, io| nothing(c, m, io, Op::KeevoroActivity),
        show: show::activity,
    },
    Row {
        words: &["log"],
        ops: &[Op::KeevoroLog],
        about: None,
        args: |c| {
            c.arg(
                Arg::new("lines")
                    .long("lines")
                    .value_name("N")
                    .value_parser(clap::value_parser!(u32))
                    .help("How many of the last lines; 400 when not said"),
            )
        },
        ask: |_, m, _| {
            let lines = m.get_one::<u32>("lines").copied();
            Ok((Op::KeevoroLog, json!({ "lines": lines })))
        },
        show: show::log,
    },
    Row {
        words: &["report"],
        ops: &[Op::ReportsSend, Op::ReportsPrepare],
        about: Some(
            "Tells Keevoro's maker about a problem or an idea, in your words. Only Keevoro's version, the operating system and your words go, unless you add the end of the log or a way to reach you. --dry-run shows exactly what would go, and sends nothing.",
        ),
        args: |c| {
            c.arg(
                Arg::new("problem")
                    .long("problem")
                    .value_name("WORDS")
                    .help("Something went wrong: say what happened"),
            )
            .arg(
                Arg::new("idea")
                    .long("idea")
                    .value_name("WORDS")
                    .help("An idea for Keevoro")
                    .conflicts_with("problem"),
            )
            .group(clap::ArgGroup::new("what").args(["problem", "idea"]).required(true))
            .arg(flag(
                "with-log",
                "Include the end of Keevoro's log, with your home folder and user name taken out. It can name your folders and files",
            ))
            .arg(
                Arg::new("contact")
                    .long("contact")
                    .value_name("HOW")
                    .help("A way to reach you, such as an email address"),
            )
            .arg(flag("dry-run", "Only show exactly what would be sent, and send nothing"))
        },
        ask: |c, m, _| {
            let (kind, words) = match text(m, "idea") {
                Some(words) => ("idea", words),
                None => ("problem", required(m, "problem")),
            };
            let request = json!({
                "kind": kind,
                "message": words,
                "withLog": m.get_flag("with-log"),
                "contact": text(m, "contact"),
                "screen": "The command line",
            });
            if m.get_flag("dry-run") {
                return Ok((Op::ReportsPrepare, request));
            }
            // What is sent is exactly what was made, as in the app.
            let prepared = c.ask(Op::ReportsPrepare, request)?;
            Ok((Op::ReportsSend, json!({ "prepared": prepared["body"] })))
        },
        show: show::report,
    },
    Row {
        words: &["reports"],
        ops: &[Op::ReportsRefresh, Op::ReportsList],
        about: Some("The reports sent from this computer, and how each is getting on."),
        args: |c| c.arg(flag("offline", "Say how they were last heard of, without asking")),
        ask: |_, m, _| {
            let op = if m.get_flag("offline") {
                Op::ReportsList
            } else {
                Op::ReportsRefresh
            };
            Ok((op, json!({})))
        },
        show: show::reports,
    },
    Row {
        words: &["settings"],
        ops: &[Op::KeevoroSettings],
        about: Some("Keevoro's settings, such as using this computer's own picture codecs."),
        args: none,
        ask: |c, m, io| nothing(c, m, io, Op::KeevoroSettings),
        show: show::settings,
    },
    Row {
        words: &["settings", "set"],
        ops: &[Op::KeevoroSettings],
        about: Some("Changes a setting: own-codecs on or off."),
        args: |c| {
            c.arg(
                arg("setting", "The setting")
                    .required(true)
                    .value_parser(["own-codecs"]),
            )
            .arg(arg("value", "on or off").required(true))
        },
        ask: |_, m, _| Ok((Op::KeevoroSettings, json!({ "ownCodecs": on_off(&required(m, "value"))? }))),
        show: show::settings,
    },
    Row {
        words: &["update"],
        ops: &[Op::KeevoroUpdate],
        about: Some(
            "Downloads the newest Keevoro command line, checks it and puts it in place of this one. With --check, only says whether there is a newer Keevoro.",
        ),
        args: |c| c.arg(flag("check", "Only say whether there is a newer version; change nothing")),
        ask: |c, m, io| nothing(c, m, io, Op::KeevoroUpdate),
        show: show::update,
    },
    Row {
        words: &["reset"],
        ops: &[Op::KeevoroReset],
        about: Some(
            "Puts Keevoro back as it was when first installed. Every backup is forgotten; nothing on a source or at a destination is touched.",
        ),
        args: |c| c.arg(flag("yes", "Do it without being asked first")),
        ask: |_, m, io| {
            crate::keys::sure(m.get_flag("yes"), io)?;
            Ok((Op::KeevoroReset, json!({})))
        },
        show: show::reset,
    },
    Row {
        words: &["agents"],
        ops: &[Op::AgentsAccess],
        about: Some("Which folders an AI agent may look through and back up to, and whether it may make changes."),
        args: none,
        ask: |c, m, io| nothing(c, m, io, Op::AgentsAccess),
        show: show::agents,
    },
    Row {
        words: &["agents", "allow"],
        ops: &[Op::AgentsAccess],
        about: Some(
            "Says which folders an AI agent may look through and back up to. The folders given replace those allowed before.",
        ),
        args: |c| {
            c.arg(
                Arg::new("look")
                    .long("look")
                    .value_name("FOLDER")
                    .action(ArgAction::Append)
                    .help("A folder it may look through; give it more than once for several"),
            )
            .arg(
                Arg::new("back-up-to")
                    .long("back-up-to")
                    .value_name("FOLDER")
                    .action(ArgAction::Append)
                    .help("A folder it may back up to; give it more than once for several"),
            )
            .arg(flag("none", "Allow no folders at all").conflicts_with_all(["look", "back-up-to"]))
            .group(
                clap::ArgGroup::new("folders")
                    .args(["look", "back-up-to", "none"])
                    .multiple(true)
                    .required(true),
            )
        },
        ask: |_, m, _| {
            let folders = |name: &str| -> Option<Vec<String>> {
                m.get_many::<String>(name).map(|all| all.cloned().collect())
            };
            let request = if m.get_flag("none") {
                json!({ "lookIn": [], "backUpTo": [] })
            } else {
                json!({ "lookIn": folders("look"), "backUpTo": folders("back-up-to") })
            };
            Ok((Op::AgentsAccess, request))
        },
        show: show::agents,
    },
    Row {
        words: &["agents", "allow-changes"],
        ops: &[Op::AgentsAccess],
        about: Some("Lets an AI agent make changes (on), or only look (off)."),
        args: |c| c.arg(arg("value", "on or off").required(true)),
        ask: |_, m, _| Ok((Op::AgentsAccess, json!({ "mayChange": on_off(&required(m, "value"))? }))),
        show: show::agents,
    },
    // The kinds of file looked for
    Row {
        words: &["kinds"],
        ops: &[Op::FileKindsList],
        about: Some(
            "The kinds of file Keevoro looks for on every source, by ending: photo, video or other file, built in or yours, on or off.",
        ),
        args: none,
        ask: |c, m, io| nothing(c, m, io, Op::FileKindsList),
        show: show::kinds,
    },
    Row {
        words: &["kinds", "add"],
        ops: &[Op::FileKindsAdd],
        about: Some(
            "Adds an ending for Keevoro to look for, as photos, videos or other files. Other files (music, documents, anything) are backed up exactly, but cannot be shown or grouped by date. Counts from the next time a source is looked through.",
        ),
        args: |c| {
            c.arg(arg("ending", "The ending, such as heif").required(true))
                .args(listed_as())
                .group(clap::ArgGroup::new("kind").args(["photo", "video", "other"]).required(true))
        },
        ask: |_, m, _| {
            Ok((
                Op::FileKindsAdd,
                json!({ "ending": required(m, "ending"), "kind": listed_as_given(m) }),
            ))
        },
        show: show::kind_changed,
    },
    Row {
        words: &["kinds", "change"],
        ops: &[Op::FileKindsChange],
        about: Some(
            "Makes a kind of file photos, videos or other files, or gives one you added a new ending. Counts from the next time a source is looked through.",
        ),
        args: |c| {
            c.arg(arg("ending", "The ending as it is on the list, such as heif").required(true))
                .args(listed_as())
                .arg(
                    Arg::new("to")
                        .long("to")
                        .value_name("ENDING")
                        .help("A new ending, for one you added"),
                )
                .group(
                    clap::ArgGroup::new("change")
                        .args(["photo", "video", "other", "to"])
                        .multiple(true)
                        .required(true),
                )
        },
        ask: |_, m, _| {
            Ok((
                Op::FileKindsChange,
                json!({ "ending": required(m, "ending"), "kind": listed_as_given(m), "to": text(m, "to") }),
            ))
        },
        show: show::kind_changed,
    },
    Row {
        words: &["kinds", "on"],
        ops: &[Op::FileKindsChange],
        about: Some("Looks for a kind of file again, from the next time a source is looked through."),
        args: |c| c.arg(arg("ending", "The ending, such as avi").required(true)),
        ask: |_, m, _| Ok((Op::FileKindsChange, json!({ "ending": required(m, "ending"), "on": true }))),
        show: show::kind_changed,
    },
    Row {
        words: &["kinds", "off"],
        ops: &[Op::FileKindsChange],
        about: Some(
            "Stops looking for a kind of file, from the next time a source is looked through. Nothing already found or backed up is taken away.",
        ),
        args: |c| c.arg(arg("ending", "The ending, such as avi").required(true)),
        ask: |_, m, _| Ok((Op::FileKindsChange, json!({ "ending": required(m, "ending"), "on": false }))),
        show: show::kind_changed,
    },
    Row {
        words: &["kinds", "remove"],
        ops: &[Op::FileKindsRemove],
        about: None,
        args: |c| c.arg(arg("ending", "An ending you added, such as heif").required(true)),
        ask: |_, m, _| Ok((Op::FileKindsRemove, json!({ "ending": required(m, "ending") }))),
        show: show::kind_removed,
    },
    Row {
        words: &["kinds", "reset"],
        ops: &[Op::FileKindsReset],
        about: None,
        args: none,
        ask: |c, m, io| nothing(c, m, io, Op::FileKindsReset),
        show: show::kinds,
    },
];

fn listed_as() -> [Arg; 3] {
    [
        flag("photo", "They are photos").conflicts_with_all(["video", "other"]),
        flag("video", "They are videos").conflicts_with("other"),
        flag(
            "other",
            "They are other files, carried along: backed up exactly, but never shown or grouped by date",
        ),
    ]
}

/// `photo`, `video`, `other`, or nothing when none was given.
fn listed_as_given(m: &ArgMatches) -> Option<&'static str> {
    ["photo", "video", "other"].into_iter().find(|kind| m.get_flag(kind))
}

/// What a backup or a source is for, as typed: `photos`, `videos` and
/// `others`, each a word of its own or with commas between.
fn wants_typed(m: &ArgMatches, name: &str) -> Option<Vec<String>> {
    m.get_many::<String>(name).map(|words| {
        words
            .flat_map(|w| w.split(','))
            .map(|w| w.trim().to_lowercase())
            .filter(|w| !w.is_empty())
            .collect()
    })
}

/// `by-date` or `folders`, as typed, in the words a request uses.
fn arrange_typed(word: &str) -> Result<&'static str, Failure> {
    keevoro_api::arrange::Arrange::from_word(word)
        .map(keevoro_api::arrange::Arrange::name)
        .ok_or_else(|| Failure::not_understood(format!("Say by-date or folders, not \"{word}\".")))
}

const WANTS_HELP: &str = "photos, videos and others: one or more, such as photos others";

fn choose_args(command: Command) -> Command {
    command
        .arg(arg("collection", "The collection, as `keevoro collections` lists it").required_unless_present("photo"))
        .arg(
            Arg::new("photo")
                .long("photo")
                .value_name("CODE")
                .help("One photo or video, by the code `keevoro photos` or `keevoro set-aside` shows")
                .conflicts_with("collection"),
        )
}

fn choose(ctx: &Ctx<'_>, m: &ArgMatches, keep: bool) -> Result<(Op, Value), Failure> {
    Ok(match text(m, "photo") {
        Some(photo) => (Op::PhotosKeep, json!({ "photo": photo, "keep": keep })),
        None => (
            Op::CollectionsKeep,
            json!({ "collection": collection(ctx, &required(m, "collection"))?, "keep": keep }),
        ),
    })
}

/// The whole command line, for clap, built from the rows.
pub fn program() -> Command {
    let mut top = Command::new("keevoro")
        .version(env!("CARGO_PKG_VERSION"))
        .about(
            "Finds the photos and videos on your sources (drives, memory cards, folders) and backs them up, \
             on the same named backups as the Keevoro app. Your sources are only ever read.",
        )
        .after_help(
            "A backup is named with -b. Without it, the backup the app has open is used, which is the only \
             one when there is one. Every command takes --json, for exactly what Keevoro's API answers.",
        )
        .subcommand_required(true)
        .arg_required_else_help(true)
        .arg(
            Arg::new("backup")
                .short('b')
                .long("backup")
                .value_name("NAME")
                .global(true)
                .help("The backup to work on, by its name"),
        )
        .arg(
            Arg::new("json")
                .long("json")
                .action(ArgAction::SetTrue)
                .global(true)
                .help("Answer in JSON, exactly as Keevoro's API does; a job says each thing on a line of its own"),
        );
    let mut firsts: Vec<&str> = Vec::new();
    for row in ROWS {
        if !firsts.contains(&row.words[0]) {
            firsts.push(row.words[0]);
        }
    }
    for first in firsts {
        let base = ROWS.iter().find(|r| r.words == [first]);
        let mut command = Command::new(first);
        command = match base {
            Some(row) => with_job_flags(row, (row.args)(command)).about(row.about()),
            None => command.subcommand_required(true),
        };
        let subs: Vec<&Row> = ROWS
            .iter()
            .filter(|r| r.words.len() == 2 && r.words[0] == first)
            .collect();
        if base.is_some() && !subs.is_empty() {
            command = command.args_conflicts_with_subcommands(true);
        }
        for sub in subs {
            let made = (sub.args)(Command::new(sub.words[1])).about(sub.about());
            command = command.subcommand(with_job_flags(sub, made));
        }
        top = top.subcommand(command);
    }
    top.subcommand(Command::new(AGENT_SERVER).about(
        "Starts Keevoro's server for an AI agent, which speaks the Model Context Protocol on standard input and \
         output. An AI app, such as Claude Desktop, starts it itself; docs/mcp.md says how to set that up. An agent \
         may only look until you allow more with `keevoro agents`.",
    ))
}

/// The command that starts the server for an AI agent. It is not an
/// operation: it is a way in to all of them.
pub const AGENT_SERVER: &str = "mcp";

/// What was asked for: a command, or the server for an AI agent.
pub enum Asked {
    Command(Typed),
    AgentServer,
}

fn with_job_flags(row: &Row, command: Command) -> Command {
    if row.starts_a_job() {
        command.arg(flag(
            "detach",
            "Start it and come back at once; follow it later with `keevoro follow` or `keevoro status`",
        ))
    } else {
        command
    }
}

/// A command as it was typed and understood.
pub struct Typed {
    pub row: &'static Row,
    pub matches: ArgMatches,
}

impl Typed {
    pub fn json(&self) -> bool {
        self.matches.get_flag("json")
    }

    pub fn backup(&self) -> Option<String> {
        text(&self.matches, "backup")
    }

    pub fn detach(&self) -> bool {
        self.row.starts_a_job() && self.matches.get_flag("detach")
    }
}

/// Understands what was typed, or says why not. Help and the version come
/// back as a "failure" with the exit code for success.
pub fn parse(args: Vec<OsString>) -> Result<Asked, Failure> {
    let matches = program().try_get_matches_from(args).map_err(|e| {
        let code = match e.kind() {
            clap::error::ErrorKind::DisplayHelp
            | clap::error::ErrorKind::DisplayVersion
            | clap::error::ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand => Code::Worked,
            _ => Code::NotUnderstood,
        };
        Failure::new(code, e.render().to_string())
    })?;
    let (first, rest) = matches
        .subcommand()
        .ok_or_else(|| Failure::not_understood("Say what to do. `keevoro --help` lists everything."))?;
    if first == AGENT_SERVER {
        if text(&matches, "backup").is_some() || text(rest, "backup").is_some() {
            return Err(Failure::not_understood(
                "`keevoro mcp` isn't about one backup, so it takes no -b. An agent names the backup in each tool.",
            ));
        }
        return Ok(Asked::AgentServer);
    }
    let (words, leaf): (Vec<&str>, &ArgMatches) = match rest.subcommand() {
        Some((second, leaf)) => (vec![first, second], leaf),
        None => (vec![first], rest),
    };
    let row = ROWS
        .iter()
        .find(|r| r.words == words.as_slice())
        .ok_or_else(|| Failure::not_understood("Keevoro doesn't know that command. `keevoro --help` lists them."))?;
    Ok(Asked::Command(Typed {
        row,
        matches: leaf.clone(),
    }))
}

/// Does what was typed.
pub fn run(keevoro: &Keevoro, typed: &Typed, io: &mut Io<'_>) -> Result<Code, Failure> {
    let ctx = Ctx {
        keevoro,
        backup: typed.backup(),
        json: typed.json(),
    };
    if ctx.backup.is_some() && !typed.row.ops.iter().any(|op| op.on_a_backup()) {
        return Err(Failure::not_understood(format!(
            "`{}` isn't about one backup, so it takes no -b.",
            typed.row.typed()
        )));
    }
    let (op, request) = (typed.row.ask)(&ctx, &typed.matches, io)?;
    if op == Op::JobsFollow {
        return crate::jobs::follow(&ctx, request, io);
    }
    if op.entry().job {
        return crate::jobs::start(&ctx, op, request, typed.detach(), io);
    }
    // Installing has no answer to give as JSON, so `--json` only checks.
    if op == Op::KeevoroUpdate && !typed.matches.get_flag("check") && !ctx.json {
        return show::install_update(io);
    }
    let answer = ctx.ask(op, request)?;
    if ctx.json {
        writeln!(io.out, "{answer}")?;
        return Ok(Code::Worked);
    }
    (typed.row.show)(io, &typed.matches, &answer)
}
