//! `keevoro`: everything the Keevoro app does, from the command line.
//!
//! The command line and the app are two faces of the same thing. Both work
//! on the same named backups ("Holiday videos", "Family photos"), kept in
//! the same place on this computer, and both do everything through
//! `keevoro-api`. This program holds no logic about photos of its own: it
//! turns what was typed into one of the API's operations, and the answer
//! into plain sentences, or into exactly the API's JSON with `--json`.
//!
//! - `commands.rs` lists every command, each made from an operation in the
//!   API's register, so the two cannot drift apart;
//! - `show.rs` says each answer in plain English;
//! - `jobs.rs` runs and follows the long operations, with progress;
//! - `keys.rs` takes storage keys, which are written down nowhere;
//! - `keevoro mcp` hands the program over to `keevoro-mcp`, the server an
//!   AI agent speaks to.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

mod commands;
mod jobs;
mod keys;
mod show;
#[cfg(test)]
mod tests;

use keevoro_api::{Folders, FrontEnd, Keevoro, Kind};
use std::ffi::OsString;
use std::io::{BufRead, IsTerminal, Write};
use std::process::ExitCode;

/// What an exit code means, as `docs/command-line.md` lists them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Code {
    /// It worked.
    Worked = 0,
    /// It ran, but some files could not be backed up, or something else
    /// went wrong.
    SomeFiles = 1,
    /// A destination could not be reached.
    Unreachable = 2,
    /// There is no such backup, source, collection, photo or job.
    NotFound = 3,
    /// Something else is already being done to that backup.
    Busy = 4,
    /// Refused, such as not enough room.
    Refused = 5,
    /// A backup that ran by itself held something back for your say (other
    /// things may still have been sent).
    Waiting = 6,
    /// A backup that runs by itself did not run now: on battery or busy,
    /// and the time it could wait ran out.
    NotNow = 7,
    /// The command was not understood.
    NotUnderstood = 64,
}

impl Code {
    /// The exit code for a problem the API gave back.
    pub fn of(kind: Kind) -> Self {
        match kind {
            Kind::NotFound | Kind::NoBackup => Code::NotFound,
            Kind::Busy => Code::Busy,
            Kind::Refused => Code::Refused,
            Kind::Unreachable => Code::Unreachable,
            Kind::NotUnderstood => Code::NotUnderstood,
            Kind::Failed | Kind::NotYet => Code::SomeFiles,
        }
    }
}

/// A command that could not do what was asked: the sentence to say, and
/// the exit code that goes with it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    pub code: Code,
    pub message: String,
}

impl Failure {
    pub fn new(code: Code, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    pub fn not_understood(message: impl Into<String>) -> Self {
        Self::new(Code::NotUnderstood, message)
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(Code::NotFound, message)
    }
}

impl From<keevoro_api::Error> for Failure {
    fn from(e: keevoro_api::Error) -> Self {
        Self::new(Code::of(e.kind), e.message)
    }
}

impl From<std::io::Error> for Failure {
    fn from(e: std::io::Error) -> Self {
        // Whatever was reading the answer stopped reading (`| head`, say):
        // there is nobody left to tell.
        if e.kind() == std::io::ErrorKind::BrokenPipe {
            return Self::new(Code::SomeFiles, "");
        }
        Self::new(Code::SomeFiles, format!("Keevoro couldn't write its answer: {e}."))
    }
}

/// Everything the program reads from and writes to the world outside it,
/// so that a test can be the world instead.
pub struct Io<'a> {
    pub out: &'a mut dyn Write,
    pub err: &'a mut dyn Write,
    pub input: &'a mut dyn BufRead,
    /// An environment variable, if it is set.
    pub env: &'a dyn Fn(&str) -> Option<String>,
    /// True when a person is watching: progress is then shown on one line,
    /// rewritten as it goes. Otherwise each new step is a plain line.
    pub watched: bool,
    /// True when a person can be asked something at the keyboard.
    pub keyboard: bool,
    /// Asks a question at the keyboard without showing the answer.
    pub ask_hidden: &'a dyn Fn(&str) -> Option<String>,
}

/// The variable that puts all of Keevoro's folders under one, for trying
/// it out, or a test, without touching the real ones.
pub const HOME_VAR: &str = "KEEVORO_HOME";

/// Keevoro, open in the same folders as the app, or under `KEEVORO_HOME`.
fn open(io: &Io<'_>) -> Result<Keevoro, Failure> {
    let folders = match (io.env)(HOME_VAR).filter(|home| !home.trim().is_empty()) {
        Some(home) => Folders::under(std::path::Path::new(&home)),
        None => Folders::for_this_computer().ok_or_else(|| {
            Failure::new(
                Code::SomeFiles,
                format!(
                    "This computer doesn't say where programs keep their things. Set {HOME_VAR} to a folder for Keevoro."
                ),
            )
        })?,
    };
    Ok(Keevoro::open(&folders).as_front_end(FrontEnd::CommandLine))
}

/// Opens Keevoro, given what the program can see of the world.
pub type Opener<'a> = &'a dyn Fn(&Io<'_>) -> Result<Keevoro, Failure>;

/// Runs one command, as typed, and gives the exit code.
pub fn run(args: Vec<OsString>, io: &mut Io<'_>, keevoro: Opener<'_>) -> Code {
    let said = commands::parse(args).and_then(|asked| match asked {
        commands::Asked::Command(typed) => {
            let keevoro = keevoro(io)?;
            commands::run(&keevoro, &typed, io)
        }
        commands::Asked::AgentServer => {
            let keevoro = keevoro(io)?.as_front_end(FrontEnd::Agent);
            keevoro_mcp::serve_on_stdio(keevoro).map_err(|e| {
                Failure::new(
                    Code::SomeFiles,
                    format!("Keevoro's server for an AI agent stopped: {e}."),
                )
            })?;
            Ok(Code::Worked)
        }
    });
    match said {
        Ok(code) => code,
        Err(failure) if failure.code == Code::Worked => {
            // Help and the version are asked for, not problems.
            let _ = write!(io.out, "{}", failure.message);
            Code::Worked
        }
        Err(failure) => {
            if !failure.message.is_empty() {
                let _ = writeln!(io.err, "{}", failure.message.trim_end());
            }
            failure.code
        }
    }
}

fn main() -> ExitCode {
    // Not locked for the whole run: `keevoro mcp` hands standard input and
    // output to the agent server, which reads and writes them from threads
    // of its own and would wait for ever on a lock held here.
    let (mut out, mut err) = (std::io::stdout(), std::io::stderr());
    let mut input = std::io::BufReader::new(std::io::stdin());
    let mut io = Io {
        watched: std::io::stderr().is_terminal(),
        keyboard: std::io::stdin().is_terminal(),
        out: &mut out,
        err: &mut err,
        input: &mut input,
        env: &|name| std::env::var(name).ok(),
        ask_hidden: &|question| rpassword::prompt_password(question).ok(),
    };
    let code = run(std::env::args_os().collect(), &mut io, &open);
    ExitCode::from(code as u8)
}
