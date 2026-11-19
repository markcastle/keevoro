//! Storage keys, and the other things a person is asked.
//!
//! Keys for cloud storage are looked for, in order:
//!
//! 1. `--keys-from-stdin`: two lines on standard input, the access key ID
//!    and then the secret. Nothing is left in the shell's history.
//! 2. `KEEVORO_ACCESS_KEY_ID` and `KEEVORO_SECRET_ACCESS_KEY` in the
//!    environment.
//! 3. `--access-key-id` and `--secret-access-key`. These end up in the
//!    shell's history, and the help says so.
//! 4. Asked for at the keyboard, the secret without being shown.
//!
//! However they come, they go straight into the request for
//! `destinations.add`, and from there to the computer's keychain and
//! nowhere else. Nothing here prints, logs or keeps them.

use crate::{Code, Failure, Io};
use clap::{Arg, ArgAction, ArgGroup, ArgMatches, Command};
use serde_json::{json, Value};

pub const ACCESS_VAR: &str = "KEEVORO_ACCESS_KEY_ID";
pub const SECRET_VAR: &str = "KEEVORO_SECRET_ACCESS_KEY";

/// What `keevoro destinations add` takes.
pub fn args(command: Command) -> Command {
    let place = |name: &'static str, value: &'static str, help: &'static str| {
        Arg::new(name).long(name).value_name(value).help(help)
    };
    command
        .arg(place("folder", "FOLDER", "A folder: an external drive, a network share, anywhere on disk"))
        .arg(place("r2", "ACCOUNT_ID", "Cloudflare R2, by your Cloudflare account ID"))
        .arg(place("s3", "REGION", "Amazon S3, by its region, such as eu-west-2"))
        .arg(place(
            "endpoint",
            "URL",
            "Other storage that speaks S3 (MinIO, a NAS), by its address",
        ))
        .group(
            ArgGroup::new("where")
                .args(["folder", "r2", "s3", "endpoint"])
                .required(true),
        )
        .arg(
            Arg::new("bucket")
                .long("bucket")
                .value_name("NAME")
                .help("The bucket, for cloud storage"),
        )
        .arg(
            Arg::new("keys-from-stdin")
                .long("keys-from-stdin")
                .action(ArgAction::SetTrue)
                .help("Read the access key ID and then the secret, one a line, from standard input"),
        )
        .arg(
            Arg::new("access-key-id")
                .long("access-key-id")
                .value_name("KEY")
                .help(format!(
                    "The access key ID. Typed here it is kept in your shell's history: prefer {ACCESS_VAR} or --keys-from-stdin"
                ))
                .conflicts_with("keys-from-stdin"),
        )
        .arg(
            Arg::new("secret-access-key")
                .long("secret-access-key")
                .value_name("SECRET")
                .help(format!(
                    "The secret. Typed here it is kept in your shell's history: prefer {SECRET_VAR} or --keys-from-stdin"
                ))
                .conflicts_with("keys-from-stdin"),
        )
}

/// The request for `destinations.add`, with the keys in it for cloud
/// storage.
pub fn request(m: &ArgMatches, io: &mut Io<'_>) -> Result<Value, Failure> {
    let given = |name: &str| m.get_one::<String>(name).cloned();
    let (kind, place) = [
        ("folder", "folder"),
        ("r2", "r2"),
        ("s3", "s3"),
        ("endpoint", "endpoint"),
    ]
    .into_iter()
    .find_map(|(flag, kind)| given(flag).map(|place| (kind, place)))
    .unwrap_or_default();
    if kind == "folder" {
        return Ok(json!({ "kind": kind, "place": place }));
    }
    let bucket = given("bucket")
        .filter(|b| !b.trim().is_empty())
        .ok_or_else(|| Failure::not_understood("Say which bucket, with --bucket."))?;
    let (access, secret) = keys(m, io)?;
    Ok(json!({
        "kind": kind,
        "place": place,
        "bucket": bucket,
        "accessKeyId": access,
        "secretAccessKey": secret,
    }))
}

fn keys(m: &ArgMatches, io: &mut Io<'_>) -> Result<(String, String), Failure> {
    let missing = || {
        Failure::not_understood(format!(
            "Keevoro needs the storage keys. Give them with --keys-from-stdin, or set {ACCESS_VAR} and {SECRET_VAR}."
        ))
    };
    let some = |key: Option<String>| key.map(|k| k.trim().to_string()).filter(|k| !k.is_empty());
    if m.get_flag("keys-from-stdin") {
        let mut line = || {
            let mut text = String::new();
            io.input.read_line(&mut text).ok().and_then(|_| some(Some(text)))
        };
        let access = line();
        let secret = line();
        return access.zip(secret).ok_or_else(missing);
    }
    if let (Some(access), Some(secret)) = (some((io.env)(ACCESS_VAR)), some((io.env)(SECRET_VAR))) {
        return Ok((access, secret));
    }
    let typed = |name: &str| some(m.get_one::<String>(name).cloned());
    if let (Some(access), Some(secret)) = (typed("access-key-id"), typed("secret-access-key")) {
        return Ok((access, secret));
    }
    if !io.keyboard {
        return Err(missing());
    }
    write!(io.err, "Access key ID: ")?;
    io.err.flush()?;
    let mut access = String::new();
    io.input.read_line(&mut access)?;
    let secret = (io.ask_hidden)("Secret access key (it won't be shown): ");
    some(Some(access)).zip(some(secret)).ok_or_else(missing)
}

/// Asks before something that cannot be undone, unless `--yes` was given.
/// Without a keyboard to ask at, `--yes` is the only way to say so.
pub fn sure(yes: bool, io: &mut Io<'_>) -> Result<(), Failure> {
    if yes {
        return Ok(());
    }
    if !io.keyboard {
        return Err(Failure::not_understood(
            "This forgets every backup. Add --yes to say you are sure.",
        ));
    }
    write!(
        io.err,
        "This forgets every backup, what each found and chose, and any keys they saved. \
         Nothing on a source or at a destination is touched. Type yes to go ahead: "
    )?;
    io.err.flush()?;
    let mut answer = String::new();
    io.input.read_line(&mut answer)?;
    if answer.trim().eq_ignore_ascii_case("yes") {
        Ok(())
    } else {
        Err(Failure::new(Code::Refused, "Nothing was changed."))
    }
}
