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
        args: |c| c.arg(arg("name", "What to call it, such as \"Drone footage\"").required(true)),
        ask: |_, m, _| Ok((Op::BackupsStart, json!({ "name": required(m, "name") }))),
        show: show::started,
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
        args: |c| c.arg(arg("name", "The backup's name").required(true)),
        ask: |_, m, _| Ok((Op::BackupsForget, json!({ "backup": required(m, "name") }))),
        show: show::forgot,
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
        },
        ask: |c, m, _| {
            if m.get_flag("again") {
                let source = text(m, "place").map(|typed| source(c, &typed)).transpose()?;
                Ok((Op::SourcesLookAgain, json!({ "source": source })))
            } else {
                Ok((Op::SourcesLook, json!({ "place": required(m, "place") })))
            }
        },
        show: show::nothing_more,
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
        words: &["picture"],
        ops: &[Op::PhotosPicture],
        about: Some("Saves a small or screen-sized picture of one photo or video, as a WebP file."),
        args: |c| {
            c.arg(arg("photo", "The photo's code, as `keevoro photos` lists it").required(true))
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
            Ok((Op::PhotosPicture, json!({ "photo": required(m, "photo"), "size": size })))
        },
        show: show::picture,
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
        ops: &[Op::BackupRun],
        about: None,
        args: none,
        ask: |c, m, io| nothing(c, m, io, Op::BackupRun),
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
        words: &["history"],
        ops: &[Op::BackupHistory],
        about: None,
        args: none,
        ask: |_, _, _| Ok((Op::BackupHistory, Value::Null)),
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
        args: none,
        ask: |c, m, io| nothing(c, m, io, Op::JobsPause),
        show: show::paused,
    },
    Row {
        words: &["carry-on"],
        ops: &[Op::JobsCarryOn],
        about: None,
        args: none,
        ask: |c, m, io| nothing(c, m, io, Op::JobsCarryOn),
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
];

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

/// True for an operation on one backup, which `-b` can name.
fn on_a_backup(op: Op) -> bool {
    !matches!(
        op,
        Op::BackupsList
            | Op::BackupsStart
            | Op::BackupsRename
            | Op::BackupsForget
            | Op::PlacesList
            | Op::JobsPause
            | Op::JobsCarryOn
            | Op::KeevoroAbout
            | Op::KeevoroActivity
            | Op::KeevoroLog
            | Op::KeevoroSettings
            | Op::KeevoroUpdate
            | Op::KeevoroReset
            | Op::AgentsAccess
    )
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
    top
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
pub fn parse(args: Vec<OsString>) -> Result<Typed, Failure> {
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
    let (words, leaf): (Vec<&str>, &ArgMatches) = match rest.subcommand() {
        Some((second, leaf)) => (vec![first, second], leaf),
        None => (vec![first], rest),
    };
    let row = ROWS
        .iter()
        .find(|r| r.words == words.as_slice())
        .ok_or_else(|| Failure::not_understood("Keevoro doesn't know that command. `keevoro --help` lists them."))?;
    Ok(Typed {
        row,
        matches: leaf.clone(),
    })
}

/// Does what was typed.
pub fn run(keevoro: &Keevoro, typed: &Typed, io: &mut Io<'_>) -> Result<Code, Failure> {
    let ctx = Ctx {
        keevoro,
        backup: typed.backup(),
        json: typed.json(),
    };
    if ctx.backup.is_some() && !typed.row.ops.iter().any(|op| on_a_backup(*op)) {
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
