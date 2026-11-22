//! Storage keys, and the other things a person is asked.
//!
//! Keys for cloud storage are looked for, in order:
//!
//! 1. `--keys-from-stdin`: two lines on standard input, the access key ID
//!    and then the secret. Nothing is left in the shell's history.
//! 2. `--access-key-id` and `--secret-access-key`. These end up in the
//!    shell's history, and the help says so.
//! 3. `KEEVORO_ACCESS_KEY_ID` and `KEEVORO_SECRET_ACCESS_KEY` in the
//!    environment. Then the request carries no keys, and the API uses the
//!    environment's and says so, as it does for the app and an agent.
//! 4. Asked for at the keyboard, the secret without being shown.
//!
//! Keys given go straight into the request for `destinations.add` or
//! `sources.add_bucket`, and
//! from there to the computer's keychain and nowhere else. Nothing here
//! prints, logs or keeps them.

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
    let command = command
        .arg(place(
            "folder",
            "FOLDER",
            "A folder: an external drive, a network share, anywhere on disk",
        ))
        .arg(place(
            "r2",
            "ACCOUNT_ID",
            "Cloudflare R2, by your Cloudflare account ID",
        ))
        .arg(place("s3", "REGION", "Amazon S3, by its region, such as eu-west-2"))
        .arg(
            Arg::new("gcs")
                .long("gcs")
                .action(ArgAction::SetTrue)
                .help("Google Cloud Storage, with HMAC keys made under Cloud Storage, Settings, Interoperability"),
        )
        .arg(place(
            "endpoint",
            "URL",
            "Other storage that speaks S3 (MinIO, a NAS), by its address",
        ))
        .group(
            ArgGroup::new("where")
                .args(["folder", "r2", "s3", "gcs", "endpoint"])
                .required(true),
        )
        .arg(
            Arg::new("bucket")
                .long("bucket")
                .value_name("NAME")
                .help("The bucket, for cloud storage"),
        );
    key_args(command)
}

/// What `keevoro sources add-bucket` takes: where the cloud storage is,
/// the bucket and a folder in it, and the keys.
pub fn bucket_args(command: Command) -> Command {
    let place = |name: &'static str, value: &'static str, help: &'static str| {
        Arg::new(name).long(name).value_name(value).help(help)
    };
    let command = command
        .arg(place(
            "r2",
            "ACCOUNT_ID",
            "Cloudflare R2, by your Cloudflare account ID",
        ))
        .arg(place("s3", "REGION", "Amazon S3, by its region, such as eu-west-2"))
        .arg(
            Arg::new("gcs")
                .long("gcs")
                .action(ArgAction::SetTrue)
                .help("Google Cloud Storage, with HMAC keys made under Cloud Storage, Settings, Interoperability"),
        )
        .arg(place(
            "endpoint",
            "URL",
            "Other storage that speaks S3 (MinIO, a NAS), by its address",
        ))
        .group(
            ArgGroup::new("where")
                .args(["r2", "s3", "gcs", "endpoint"])
                .required(true),
        )
        .arg(place("bucket", "NAME", "The bucket to read from").required(true))
        .arg(place(
            "prefix",
            "FOLDER",
            "A folder in the bucket to read from, such as Photos/2019; the whole bucket if left out",
        ))
        .arg(place(
            "name",
            "NAME",
            "What to call the source; the folder's or bucket's name if left out",
        ));
    key_args(command)
}

/// The request for `sources.add_bucket`, with the keys in it.
pub fn bucket_request(m: &ArgMatches, io: &mut Io<'_>) -> Result<Value, Failure> {
    let given = |name: &str| m.get_one::<String>(name).cloned();
    let (kind, place) = ["r2", "s3", "endpoint"]
        .into_iter()
        .find_map(|kind| given(kind).map(|place| (kind, place)))
        .or_else(|| m.get_flag("gcs").then(|| ("gcs", String::new())))
        .unwrap_or_default();
    let mut request = json!({ "kind": kind, "place": place, "bucket": given("bucket").unwrap_or_default() });
    for (field, name) in [("prefix", "prefix"), ("name", "name")] {
        if let Some(value) = given(name) {
            request[field] = Value::String(value);
        }
    }
    if let Some((access, secret)) = keys(m, io)? {
        request["accessKeyId"] = Value::String(access);
        request["secretAccessKey"] = Value::String(secret);
    }
    Ok(request)
}

/// The ways storage keys can be given.
fn key_args(command: Command) -> Command {
    command
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
    // Google Cloud Storage has one address, so it is said without one.
    .or_else(|| m.get_flag("gcs").then(|| ("gcs", String::new())))
    .unwrap_or_default();
    if kind == "folder" {
        return Ok(json!({ "kind": kind, "place": place }));
    }
    let bucket = given("bucket")
        .filter(|b| !b.trim().is_empty())
        .ok_or_else(|| Failure::not_understood("Say which bucket, with --bucket."))?;
    Ok(match keys(m, io)? {
        Some((access, secret)) => json!({
            "kind": kind,
            "place": place,
            "bucket": bucket,
            "accessKeyId": access,
            "secretAccessKey": secret,
        }),
        // The environment's, which the API reads itself, and says it did.
        None => json!({ "kind": kind, "place": place, "bucket": bucket }),
    })
}

/// The keys given, or `None` when those set in the environment are to be
/// used.
fn keys(m: &ArgMatches, io: &mut Io<'_>) -> Result<Option<(String, String)>, Failure> {
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
        return access.zip(secret).map(Some).ok_or_else(missing);
    }
    let typed = |name: &str| some(m.get_one::<String>(name).cloned());
    match (typed("access-key-id"), typed("secret-access-key")) {
        (Some(access), Some(secret)) => return Ok(Some((access, secret))),
        (None, None) => {}
        _ => {
            return Err(Failure::not_understood(
                "Give both keys: --access-key-id and --secret-access-key.",
            ))
        }
    }
    if some((io.env)(ACCESS_VAR)).is_some() && some((io.env)(SECRET_VAR)).is_some() {
        return Ok(None);
    }
    if !io.keyboard {
        return Err(missing());
    }
    write!(io.err, "Access key ID: ")?;
    io.err.flush()?;
    let mut access = String::new();
    io.input.read_line(&mut access)?;
    let secret = (io.ask_hidden)("Secret access key (it won't be shown): ");
    some(Some(access)).zip(some(secret)).map(Some).ok_or_else(missing)
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The request `keevoro destinations add` makes, given what was typed
    /// and what is set in the environment.
    fn asked(typed: &[&str], env: &[(&str, &str)]) -> Result<Value, Failure> {
        asked_for(&["--endpoint", "http://127.0.0.1:9"], typed, env)
    }

    fn asked_for(place: &[&str], typed: &[&str], env: &[(&str, &str)]) -> Result<Value, Failure> {
        let mut all = vec!["add"];
        all.extend(place);
        all.extend(["--bucket", "family"]);
        all.extend(typed);
        let m = args(Command::new("add")).try_get_matches_from(all).unwrap();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let mut input = std::io::Cursor::new(Vec::new());
        let env: Vec<(String, String)> = env.iter().map(|(n, v)| (n.to_string(), v.to_string())).collect();
        let lookup = move |name: &str| env.iter().find(|(n, _)| n == name).map(|(_, v)| v.clone());
        let mut io = Io {
            out: &mut out,
            err: &mut err,
            input: &mut input,
            env: &lookup,
            watched: false,
            keyboard: false,
            ask_hidden: &|_| None,
        };
        request(&m, &mut io)
    }

    #[test]
    fn keys_typed_win_over_the_environments_which_are_left_for_the_api_to_use_and_say() {
        let env = [(ACCESS_VAR, "AKIAFROMENV"), (SECRET_VAR, "secret-from-env")];
        let typed = ["--access-key-id", "AKIATYPED", "--secret-access-key", "secret-typed"];
        let request = asked(&typed, &env).unwrap();
        assert_eq!(request["accessKeyId"], "AKIATYPED");
        assert_eq!(request["secretAccessKey"], "secret-typed");

        let request = asked(&[], &env).unwrap();
        assert!(request.get("accessKeyId").is_none(), "{request}");
        assert!(!request.to_string().contains("FROMENV"), "{request}");

        let half = asked(&["--access-key-id", "AKIATYPED"], &env).unwrap_err();
        assert!(half.message.contains("both keys"), "{}", half.message);
        let none = asked(&[], &[]).unwrap_err();
        assert!(none.message.contains("--keys-from-stdin"), "{}", none.message);
    }

    #[test]
    fn google_cloud_storage_is_named_without_a_place() {
        let env = [(ACCESS_VAR, "GOOGEXAMPLE1234567890AB"), (SECRET_VAR, "secret-from-env")];
        let request = asked_for(&["--gcs"], &[], &env).unwrap();
        assert_eq!(request, json!({ "kind": "gcs", "place": "", "bucket": "family" }));
        let both = args(Command::new("add")).try_get_matches_from(["add", "--gcs", "--s3", "eu-west-2"]);
        assert!(both.is_err(), "one place at a time");

        let m = bucket_args(Command::new("add-bucket"))
            .try_get_matches_from(["add-bucket", "--gcs", "--bucket", "old-scans"])
            .unwrap();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let mut input = std::io::Cursor::new(Vec::new());
        let lookup = move |name: &str| env.iter().find(|(n, _)| *n == name).map(|(_, v)| (*v).to_string());
        let mut io = Io {
            out: &mut out,
            err: &mut err,
            input: &mut input,
            env: &lookup,
            watched: false,
            keyboard: false,
            ask_hidden: &|_| None,
        };
        assert_eq!(
            bucket_request(&m, &mut io).unwrap(),
            json!({ "kind": "gcs", "place": "", "bucket": "old-scans" })
        );
    }
}
