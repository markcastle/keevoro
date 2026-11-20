//! The long operations: looking through a source, surveying, backing up and
//! checking.
//!
//! Starting one waits for it to end, showing how it is getting on, as a
//! person at a terminal expects. When nobody is watching (the output goes
//! to a file or another program) each new step is a plain line instead of
//! one line rewritten in place. With `--json` each thing the job says is a
//! line of JSON as it says it, the same events the app hears. `--detach`
//! starts the job in a program of its own that carries on in the
//! background, and comes back at once.

use crate::commands::Ctx;
use crate::{show, Code, Failure, Io};
use keevoro_api::{Event, JobId, Op, Outcome, Said};
use serde_json::{json, Value};
use std::ffi::OsString;
use std::io::{BufRead, BufReader};
use std::sync::mpsc;

/// Set in the program `--detach` starts, which runs the job.
pub const DETACHED_VAR: &str = "KEEVORO_DETACHED_JOB";

/// What a job said, or that it has ended with these last words.
enum Heard {
    Said(Event),
    Ended(JobId, Option<Said>),
}

/// Starts a job and, unless detached, follows it to its end.
pub fn start(ctx: &Ctx<'_>, op: Op, request: Value, detach: bool, io: &mut Io<'_>) -> Result<Code, Failure> {
    if detach {
        return detached(ctx, io);
    }
    let (tell, heard) = mpsc::channel();
    // Everything this program's jobs say, from before the job has started.
    let said = tell.clone();
    ctx.keevoro.listen(move |event| {
        let _ = said.send(Heard::Said(event.clone()));
    });
    let started = match ctx.ask(op, request) {
        Ok(started) => started,
        Err(failure) if (io.env)(DETACHED_VAR).is_some() => {
            // The program that started this one is waiting to hear.
            writeln!(
                io.out,
                "{}",
                json!({ "problem": failure.message, "code": failure.code as u8 })
            )?;
            return Ok(failure.code);
        }
        Err(failure) => return Err(failure),
    };
    let id = JobId(started.get("id").and_then(Value::as_u64).unwrap_or_default());
    if (io.env)(DETACHED_VAR).is_some() {
        writeln!(io.out, "{}", json!({ "started": started }))?;
        io.out.flush()?;
        // Nobody is listening any more: the job is all that is left to do.
        let mut quiet = std::io::sink();
        let mut also_quiet = std::io::sink();
        let mut nothing = std::io::empty();
        let mut alone = Io {
            out: &mut quiet,
            err: &mut also_quiet,
            input: &mut nothing,
            env: io.env,
            watched: false,
            keyboard: false,
            ask_hidden: io.ask_hidden,
        };
        return hear(ctx, &[id], tell, heard, &mut alone, false);
    }
    if ctx.json {
        writeln!(io.out, "{started}")?;
    }
    hear(ctx, &[id], tell, heard, io, ctx.json)
}

/// `jobs.follow`: hears every job running, or the one asked for, here or
/// in another program, until they have ended.
pub fn follow(ctx: &Ctx<'_>, request: Value, io: &mut Io<'_>) -> Result<Code, Failure> {
    let running = ctx.ask(Op::JobsStatus, request.clone())?;
    let ids: Vec<JobId> = running
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|status| status.get("id").and_then(Value::as_u64))
        .map(JobId)
        .collect();
    if ids.is_empty() {
        return match request.get("job").and_then(Value::as_u64) {
            Some(job) => Err(Failure::not_found(format!("There is no job {job} running."))),
            None => {
                if !ctx.json {
                    writeln!(io.out, "Nothing is running.")?;
                }
                Ok(Code::Worked)
            }
        };
    }
    let (tell, heard) = mpsc::channel();
    for id in &ids {
        let said = tell.clone();
        ctx.keevoro.follow(
            *id,
            std::sync::Arc::new(move |event: &Event| {
                let _ = said.send(Heard::Said(event.clone()));
            }),
        );
    }
    hear(ctx, &ids, tell, heard, io, ctx.json)
}

/// Says what the jobs say until each has ended, and gives the exit code for
/// how they ended: the first that did not simply work.
fn hear(
    ctx: &Ctx<'_>,
    ids: &[JobId],
    tell: mpsc::Sender<Heard>,
    heard: mpsc::Receiver<Heard>,
    io: &mut Io<'_>,
    json: bool,
) -> Result<Code, Failure> {
    std::thread::scope(|waiting| {
        for id in ids {
            let tell = tell.clone();
            let keevoro = ctx.keevoro;
            waiting.spawn(move || {
                let last = keevoro.wait(*id);
                let _ = tell.send(Heard::Ended(*id, last));
            });
        }
        drop(tell);
        let mut progress = Progress::new(io.watched);
        // A backup's headline (how long it has run, how long is left),
        // asked for at most once a second, as the app asks.
        let mut headline = String::new();
        let mut asked = None::<std::time::Instant>;
        let mut left = ids.len();
        let mut code = Code::Worked;
        // Each job's last word, once it has been said as an event.
        let mut said_last: Vec<JobId> = Vec::new();
        while left > 0 {
            let Ok(next) = heard.recv() else { break };
            match next {
                Heard::Said(event) if ids.contains(&event.job) => {
                    if json {
                        writeln!(io.out, "{}", serde_json::to_string(&event).unwrap_or_default())?;
                    }
                    if event.said.is_last() {
                        said_last.push(event.job);
                    } else if !json {
                        if matches!(event.said, Said::Send(_)) && asked.is_none_or(|at| at.elapsed().as_secs() >= 1) {
                            asked = Some(std::time::Instant::now());
                            headline = ctx
                                .keevoro
                                .jobs(None)
                                .into_iter()
                                .find(|status| status.handle.id == event.job)
                                .and_then(|status| status.estimate)
                                .map(|estimate| estimate.headline)
                                .unwrap_or_default();
                        }
                        let step = step(&event.said);
                        if headline.is_empty() || step.is_empty() {
                            progress.show(io, &step)?;
                        } else {
                            progress.show_with(io, &step, &headline)?;
                        }
                    }
                }
                Heard::Said(_) => {}
                Heard::Ended(id, last) => {
                    left -= 1;
                    progress.clear(io)?;
                    if json && !said_last.contains(&id) {
                        // Ended in another program, which shared only its
                        // last status: say its last word as the event.
                        if let Some(last) = &last {
                            let event = json!({ "job": id, "event": last.name(), "said": last });
                            writeln!(io.out, "{event}")?;
                        }
                    }
                    if !json {
                        show::last_word(io, last.as_ref())?;
                    }
                    let ended = code_for(last.as_ref());
                    if code == Code::Worked {
                        code = ended;
                    }
                }
            }
        }
        Ok(code)
    })
}

/// The exit code for how a job ended.
pub fn code_for(last: Option<&Said>) -> Code {
    match last {
        Some(Said::Send(e)) => match e.outcome {
            Some(Outcome::Worked | Outcome::Stopped) => Code::Worked,
            Some(Outcome::SomeFiles) => Code::SomeFiles,
            Some(Outcome::Unreachable) => Code::Unreachable,
            Some(Outcome::Refused) => Code::Refused,
            None if e.problem.is_some() => Code::SomeFiles,
            None => Code::Worked,
        },
        Some(Said::Scan(e)) if e.problem.is_some() => Code::SomeFiles,
        Some(_) => Code::Worked,
        // It ended without a word: its program stopped part-way.
        None => Code::SomeFiles,
    }
}

/// The step a job is on, in a line.
fn step(said: &Said) -> String {
    let n = |n: u64| keevoro_api::words::thousands(usize::try_from(n).unwrap_or(usize::MAX));
    match said {
        Said::Send(e) => e
            .line
            .clone()
            .unwrap_or_default()
            .split('\t')
            .next()
            .unwrap_or_default()
            .to_string(),
        Said::Scan(e) if e.saving => format!("Looked through {}; saving what was found", e.name),
        Said::Scan(e) => format!(
            "Looking through {}: {} photos and {} videos so far",
            e.name,
            n(e.photos),
            n(e.videos)
        ),
        Said::Survey(e) => format!(
            "Looked in {} folders: {} photos and {} videos by name so far",
            keevoro_api::words::thousands(e.folders),
            keevoro_api::words::thousands(e.photos),
            keevoro_api::words::thousands(e.videos)
        ),
        Said::Check(e) => format!(
            "Checking for duplicates: {} of {}",
            keevoro_api::words::thousands(e.done),
            keevoro_api::words::thousands(e.total)
        ),
        Said::Surveyed(_) => String::new(),
    }
}

/// How a job is getting on, on standard error. Watched, it is one line,
/// rewritten in place. Otherwise each new step is a line of its own, and
/// the counts within a step are not repeated.
struct Progress {
    watched: bool,
    shown: bool,
    last_step: String,
    last_headline: String,
}

impl Progress {
    fn new(watched: bool) -> Self {
        Self {
            watched,
            shown: false,
            last_step: String::new(),
            last_headline: String::new(),
        }
    }

    fn show(&mut self, io: &mut Io<'_>, line: &str) -> std::io::Result<()> {
        if line.is_empty() {
            return Ok(());
        }
        if self.watched {
            self.shown = true;
            write!(io.err, "\r\x1b[2K  {line}")?;
            return io.err.flush();
        }
        // The words without the numbers: "Working out where each file goes: of".
        let words: String = line.chars().filter(|c| !c.is_ascii_digit() && *c != ',').collect();
        if words != self.last_step {
            self.last_step = words;
            writeln!(io.err, "{line}")?;
        }
        Ok(())
    }

    /// A step with the backup's headline. Watched, both share the line;
    /// otherwise the headline is its own line, written when its words
    /// change, which is when the estimate is worked out again or a minute
    /// passes.
    fn show_with(&mut self, io: &mut Io<'_>, line: &str, headline: &str) -> std::io::Result<()> {
        if self.watched {
            return self.show(io, &format!("{line}. {headline}"));
        }
        self.show(io, line)?;
        if headline != self.last_headline {
            headline.clone_into(&mut self.last_headline);
            writeln!(io.err, "{headline}")?;
        }
        Ok(())
    }

    fn clear(&mut self, io: &mut Io<'_>) -> std::io::Result<()> {
        if self.shown {
            self.shown = false;
            write!(io.err, "\r\x1b[2K")?;
        }
        Ok(())
    }
}

/// Starts the same command again as a program of its own, which runs the
/// job in the background, and comes back once it has started.
fn detached(ctx: &Ctx<'_>, io: &mut Io<'_>) -> Result<Code, Failure> {
    let program = std::env::current_exe().map_err(|e| {
        Failure::new(
            Code::SomeFiles,
            format!("Keevoro couldn't find itself to start that: {e}."),
        )
    })?;
    let args: Vec<OsString> = std::env::args_os().skip(1).filter(|arg| arg != "--detach").collect();
    let mut command = std::process::Command::new(program);
    command
        .args(args)
        .env(DETACHED_VAR, "1")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    on_its_own(&mut command);
    let mut child = command.spawn().map_err(|e| {
        Failure::new(
            Code::SomeFiles,
            format!("Keevoro couldn't start that in the background: {e}."),
        )
    })?;
    let mut first = String::new();
    if let Some(out) = child.stdout.take() {
        let _ = BufReader::new(out).read_line(&mut first);
    }
    let said: Value = serde_json::from_str(first.trim()).unwrap_or(Value::Null);
    if let Some(started) = said.get("started") {
        if ctx.json {
            writeln!(io.out, "{started}")?;
        } else {
            show::started_job(io, started)?;
        }
        return Ok(Code::Worked);
    }
    let code = match said.get("code").and_then(Value::as_u64) {
        Some(2) => Code::Unreachable,
        Some(3) => Code::NotFound,
        Some(4) => Code::Busy,
        Some(5) => Code::Refused,
        Some(64) => Code::NotUnderstood,
        _ => Code::SomeFiles,
    };
    let message = said
        .get("problem")
        .and_then(Value::as_str)
        .unwrap_or("Keevoro couldn't start that in the background.")
        .to_string();
    Err(Failure::new(code, message))
}

/// The program carries on when the one that started it ends, and is not
/// stopped by Ctrl-C pressed for that one.
#[cfg(unix)]
fn on_its_own(command: &mut std::process::Command) {
    use std::os::unix::process::CommandExt;
    command.process_group(0);
}

#[cfg(windows)]
fn on_its_own(command: &mut std::process::Command) {
    use std::os::windows::process::CommandExt;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    command.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
}

#[cfg(not(any(unix, windows)))]
fn on_its_own(_: &mut std::process::Command) {}
