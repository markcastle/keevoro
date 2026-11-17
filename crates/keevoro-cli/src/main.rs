//! `keevoro`: the engine without a UI.
//!
//! Everything the desktop app will do is reachable from here first, so the
//! engine can be proven on real drives before any screens exist.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use keevoro_catalog::Catalog;
pub(crate) use keevoro_engine::wanted;
use keevoro_engine::{connect, send as publish};
use keevoro_engine::{gather_everything, summary, GatherStep};
use keevoro_scan::FoundItem;
use keevoro_scan::{scan, ScanOptions, ScanReport};
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Parser)]
#[command(
    name = "keevoro",
    version,
    about = "Find photos and videos on your sources (old drives, folders) and gather them into one library."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

/// Where to back up to. Shared by `connect` and `publish`.
#[derive(clap::Args, Debug, Default)]
struct DestinationArgs {
    /// A folder to back up to: an external drive or a NAS share.
    #[arg(long, group = "destination")]
    to_folder: Option<PathBuf>,
    /// Back up to Cloudflare R2. Give your Cloudflare account ID.
    #[arg(long, group = "destination", requires = "bucket", value_name = "ACCOUNT_ID")]
    to_r2: Option<String>,
    /// Back up to Amazon S3. Give the region, like eu-west-2.
    #[arg(long, group = "destination", requires = "bucket", value_name = "REGION")]
    to_s3: Option<String>,
    /// Back up to another S3-compatible server (MinIO, a NAS).
    /// Give its address, like https://nas.local:9000.
    #[arg(long, group = "destination", requires = "bucket", value_name = "URL")]
    to_endpoint: Option<String>,
    /// The bucket to use with --to-r2, --to-s3 or --to-endpoint.
    #[arg(long)]
    bucket: Option<String>,
    /// The web address the storage is served from, if it is.
    /// Used for links inside the index.
    #[arg(long)]
    base_url: Option<String>,
}

impl DestinationArgs {
    fn flags(self) -> connect::Flags {
        connect::Flags {
            to_folder: self.to_folder,
            to_r2: self.to_r2,
            to_s3: self.to_s3,
            to_endpoint: self.to_endpoint,
            bucket: self.bucket,
            base_url: self.base_url,
        }
    }
}

#[derive(Subcommand)]
enum Command {
    /// Look through one or more drives or folders and report what was found.
    /// Nothing is copied or changed.
    Scan {
        /// Drives or folders to look through.
        #[arg(required = true)]
        paths: Vec<PathBuf>,
        /// Print everything as JSON instead of a summary.
        #[arg(long)]
        json: bool,
        /// Skip hashing for a quicker first look (duplicates are not found).
        #[arg(long)]
        no_hash: bool,
        /// How many collections to list.
        #[arg(long, default_value_t = 20)]
        show: usize,
        /// Remember what was found in this library folder, so the drive
        /// does not need to be plugged in to look at it again.
        #[arg(long)]
        library: Option<PathBuf>,
        /// A friendly name for the drive, like "Dad's old Dell".
        /// Needs --library and a single drive.
        #[arg(long, requires = "library")]
        name: Option<String>,
    },
    /// Find which folders on a drive hold photos and videos, in seconds.
    /// Only folder listings are read: no file is opened, copied or changed.
    /// Use it to decide what to `scan`.
    Survey {
        /// The drive or folder to survey.
        path: PathBuf,
    },
    /// Show the collections found so far across every drive remembered in a
    /// library. No drive needs to be plugged in.
    Collections {
        /// The library folder given to `scan --library`.
        #[arg(long)]
        library: PathBuf,
        /// Print everything as JSON instead of a summary.
        #[arg(long)]
        json: bool,
        /// How many collections to list.
        #[arg(long, default_value_t = 20)]
        show: usize,
    },
    /// List the drives and folders a library remembers.
    Sources {
        /// The library folder given to `scan --library`.
        #[arg(long)]
        library: PathBuf,
    },
    /// Leave one or more collections out of the library.
    /// Nothing is deleted, and `keep` changes your mind.
    Skip {
        #[arg(long)]
        library: PathBuf,
        /// Collection names as shown by `collections`, like "Cornwall".
        #[arg(required_unless_present = "photo")]
        collections: Vec<String>,
        /// A single photo or video to leave out, by the code shown by
        /// `keevoro photos`. Can be given more than once.
        #[arg(long, value_name = "CODE")]
        photo: Vec<String>,
    },
    /// Put collections or single photos back after `skip`, or keep a
    /// picture that was set aside as junk.
    Keep {
        #[arg(long)]
        library: PathBuf,
        /// Collection names as shown by `collections`, like "Cornwall".
        #[arg(required_unless_present = "photo")]
        collections: Vec<String>,
        /// A single photo or video to keep, by the code shown by
        /// `keevoro photos`. Can be given more than once.
        #[arg(long, value_name = "CODE")]
        photo: Vec<String>,
    },
    /// List the photos and videos in a collection, each with a short code
    /// for `keep --photo` and `skip --photo`.
    Photos {
        #[arg(long)]
        library: PathBuf,
        /// The collection to list, as shown by `collections`.
        #[arg(required_unless_present = "junk")]
        collection: Option<String>,
        /// List the pictures set aside as junk instead, with the reason.
        #[arg(long, conflicts_with = "collection")]
        junk: bool,
    },
    /// Copy everything you are keeping into the library and check every
    /// copy. Your sources are only read. Safe to stop and run again.
    Gather {
        #[arg(long)]
        library: PathBuf,
        /// Also make an MP4 copy of videos in older formats (AVI, WMV, MPG,
        /// 3GP and the like) so they play on any phone, TV or browser. The
        /// originals are always kept. Needs ffmpeg to be installed.
        #[arg(long)]
        convert_videos: bool,
        /// Try again to make previews for files that had none. Useful
        /// after updating Keevoro, which may have learned a new format,
        /// or after installing ffmpeg.
        #[arg(long)]
        retry_previews: bool,
    },
    /// Back the library up to storage and write its index. Safe to stop
    /// and run again; originals already stored are never replaced.
    Publish {
        #[arg(long)]
        library: PathBuf,
        #[command(flatten)]
        to: DestinationArgs,
        /// Change the private link: write the index under a new random
        /// name and remove the old one, so the old link stops working.
        #[arg(long)]
        new_link: bool,
    },
    /// Make sure the library, and optionally its backup, are still intact.
    /// Every original is read again and compared with how it was when it
    /// was gathered. Nothing is changed.
    Check {
        #[arg(long)]
        library: PathBuf,
        /// Also check that every original is in the backup.
        #[arg(long)]
        backup: bool,
        /// With --backup: download each original and compare it exactly.
        /// Certain, but as slow as downloading the whole backup.
        #[arg(long, requires = "backup")]
        read_back: bool,
        #[command(flatten)]
        to: DestinationArgs,
    },
    /// Set up where this library is backed up to, once. Checks the
    /// storage works, saves the keys in this computer's keychain, and
    /// remembers the destination so `publish` needs nothing more. Keys are
    /// read from KEEVORO_ACCESS_KEY_ID and KEEVORO_SECRET_ACCESS_KEY, or
    /// asked for (the secret is not shown as you paste it).
    Connect {
        #[arg(long)]
        library: PathBuf,
        #[command(flatten)]
        to: DestinationArgs,
    },
    /// Forget this library's backup destination and remove its keys from
    /// the keychain. Nothing already backed up is touched.
    Disconnect {
        #[arg(long)]
        library: PathBuf,
    },
    /// Update Keevoro itself to the newest version. This is the only time
    /// Keevoro contacts the internet without being given a storage address.
    Update {
        /// Only say whether a newer version exists; change nothing.
        #[arg(long)]
        check: bool,
    },
}

fn main() {
    // Problems are said once, in plain words, with no "Error:" prefix or
    // stack trace: the messages are written for people, not debuggers.
    let world = connect::World {
        secrets: &connect::Keychain,
        ask: &ask_at_terminal,
    };
    if let Err(e) = run(Cli::parse(), &mut std::io::stdout().lock(), &world) {
        eprintln!("{e:#}");
        std::process::exit(1);
    }
}

/// Asks at the terminal. Secrets are not shown as they are typed.
fn ask_at_terminal(question: &str, hidden: bool) -> Result<String> {
    let answer = if hidden {
        rpassword::prompt_password(format!("{question}: "))
    } else {
        eprint!("{question}: ");
        let _ = std::io::stderr().flush();
        let mut line = String::new();
        std::io::stdin().read_line(&mut line).map(|_| line)
    };
    match answer {
        Ok(a) if !a.trim().is_empty() => Ok(a.trim().to_string()),
        _ => bail!(
            "Keevoro needs the storage keys. Set {} and {}, or run this again and paste them when asked.",
            publish::ACCESS_KEY_VAR,
            publish::SECRET_KEY_VAR
        ),
    }
}

/// Shows one line of progress on the terminal, rewritten in place. Silent
/// when the output is being captured by a script.
fn progress_line(text: &str) {
    if std::io::stderr().is_terminal() {
        eprint!("\r\x1b[2K  {text}");
        let _ = std::io::stderr().flush();
    }
}

/// Clears the progress line once the work is done.
fn progress_done() {
    if std::io::stderr().is_terminal() {
        eprint!("\r\x1b[2K");
    }
}

fn run(cli: Cli, out: &mut dyn Write, world: &connect::World<'_>) -> Result<()> {
    match cli.command {
        Command::Survey { path } => {
            let started = std::time::Instant::now();
            let found = keevoro_scan::survey::survey(&path, &keevoro_core::SkipRules::default(), None, &mut |_| {});
            println!(
                "Looked at {} in {} in {:.1} seconds, without opening any of them.",
                keevoro_engine::summary::count(found.files_seen, "file"),
                keevoro_engine::summary::count(found.folders_seen, "folder"),
                started.elapsed().as_secs_f32()
            );
            if found.hints.is_empty() {
                println!("No photos or videos were found by name.");
            }
            for hint in &found.hints {
                println!(
                    "{:>9}  {} and {}  {}{}",
                    keevoro_engine::summary::bytes(hint.bytes),
                    keevoro_engine::summary::count(hint.photos, "photo"),
                    keevoro_engine::summary::count(hint.videos, "video"),
                    hint.folder.display(),
                    if hint.likely == keevoro_scan::survey::Likely::AppPictures {
                        "  (small pictures, probably not photos)"
                    } else {
                        ""
                    }
                );
            }
            if found.unreadable > 0 {
                println!(
                    "{} could not be listed, usually for want of permission.",
                    keevoro_engine::summary::count(found.unreadable, "folder")
                );
            }
            Ok(())
        }
        Command::Scan {
            paths,
            json,
            no_hash,
            show,
            library,
            name,
        } => {
            if name.is_some() && paths.len() > 1 {
                bail!("Give one source at a time when using --name.");
            }
            let reports = run_scan(&paths, no_hash, library.is_some());
            if let Some(library) = &library {
                remember(library, name.as_deref(), &reports)?;
            }
            print_reports(out, &reports, &std::collections::HashSet::new(), json, show)
        }
        Command::Collections { library, json, show } => {
            let catalog = open_existing(&library)?;
            let reports = catalog.load_reports()?;
            if reports.is_empty() {
                writeln!(out, "Nothing has been scanned into this library yet.")?;
                return Ok(());
            }
            print_reports(out, &reports, &catalog.kept_hashes()?, json, show)?;
            if !json {
                let skipped = skipped_collections(&catalog, &reports)?;
                if !skipped.is_empty() {
                    writeln!(out, "Not keeping: {}.", skipped.join(", "))?;
                }
            }
            Ok(())
        }
        Command::Skip {
            library,
            collections,
            photo,
        } => choose(out, &library, &collections, &photo, false),
        Command::Keep {
            library,
            collections,
            photo,
        } => choose(out, &library, &collections, &photo, true),
        Command::Photos {
            library,
            collection,
            junk,
        } => list_photos(out, &library, collection.as_deref(), junk),
        Command::Gather {
            library,
            convert_videos,
            retry_previews,
        } => {
            if retry_previews {
                open_existing(&library)?.forget_preview_problems()?;
            }
            run_gather(
                out,
                &library,
                convert_videos,
                keevoro_media::VideoTools::find().as_ref(),
            )
        }
        Command::Update { check } => run_update(
            out,
            &keevoro_update::GitHub::from_env(),
            env!("CARGO_PKG_VERSION"),
            check,
            &keevoro_update::install,
        ),
        Command::Publish { library, to, new_link } => {
            let catalog = open_existing(&library)?;
            // What was asked for on the command line wins; otherwise the
            // destination this library was connected to.
            let flags = to.flags();
            let saved = match connect::Saved::from_flags(&flags) {
                Some(given) => given,
                None => match connect::Saved::load(&catalog)? {
                    Some(mut remembered) => {
                        if flags.base_url.is_some() {
                            remembered.base_url = flags.base_url;
                        }
                        remembered
                    }
                    None => bail!(
                        "Say where to back up to: --to-folder, --to-r2, --to-s3 or --to-endpoint. \
                         Or run `keevoro connect` once and it will be remembered."
                    ),
                },
            };
            let destination = saved.open(world)?;
            let done = publish::run(
                out,
                &catalog,
                &library,
                destination,
                saved.base_url.as_deref(),
                new_link,
                &mut progress_line,
            );
            progress_done();
            done
        }
        Command::Connect { library, to } => {
            let Some(saved) = connect::Saved::from_flags(&to.flags()) else {
                bail!("Say where to back up to: --to-folder, --to-r2, --to-s3 or --to-endpoint.");
            };
            // Connecting may be the first thing done with a new library.
            let catalog = Catalog::open_in_library(&library)
                .with_context(|| format!("Couldn't use {} as a library", library.display()))?;
            connect::connect(out, &catalog, &saved, world)
        }
        Command::Check {
            library,
            backup,
            read_back,
            to,
        } => {
            let catalog = open_existing(&library)?;
            let destination = if backup {
                let flags = to.flags();
                let saved = match connect::Saved::from_flags(&flags) {
                    Some(given) => given,
                    None => connect::Saved::load(&catalog)?.ok_or_else(|| {
                        anyhow::anyhow!(
                            "Say which backup to check: --to-folder, --to-r2, --to-s3 or --to-endpoint. \
                             Or run `keevoro connect` once and it will be remembered."
                        )
                    })?,
                };
                Some(saved.open(world)?)
            } else {
                None
            };
            let done = publish::check(out, &catalog, &library, destination, read_back, &mut progress_line);
            progress_done();
            done
        }
        Command::Disconnect { library } => connect::disconnect(out, &open_existing(&library)?, world),
        Command::Sources { library } => {
            let sources = open_existing(&library)?.sources()?;
            if sources.is_empty() {
                writeln!(out, "Nothing has been scanned into this library yet.")?;
            }
            for s in sources {
                let items = usize::try_from(s.items).unwrap_or(usize::MAX);
                writeln!(
                    out,
                    "{}  {}  ({})",
                    s.name,
                    summary::count(items, "photo or video").replace("photo or videos", "photos and videos"),
                    s.root.display()
                )?;
            }
            Ok(())
        }
    }
}

/// Opens a library's catalog without creating one: a mistyped folder should
/// be an error, not a new empty library.
fn open_existing(library: &Path) -> Result<Catalog> {
    if !library.join(keevoro_catalog::CATALOG_PATH).is_file() {
        bail!(
            "There is no Keevoro library at {}. Run `keevoro scan --library` first.",
            library.display()
        );
    }
    Ok(Catalog::open_in_library(library)?)
}

/// Records the user's decision for every file in the named collections.
fn choose(out: &mut dyn Write, library: &Path, names: &[String], photos: &[String], keep: bool) -> Result<()> {
    let catalog = open_existing(library)?;
    let reports = catalog.load_reports()?;
    let verb = if keep { "Keeping" } else { "Not keeping" };

    // Single photos first, so keeping a junk picture and then naming its
    // collection in the same command sees it there.
    let (everything, _) = summary::unique_items(&reports);
    for code in photos {
        let item = find_by_code(&everything, code)?;
        if let Some(hash) = &item.blake3 {
            catalog.set_choice(hash, keep)?;
        }
        writeln!(out, "{verb} {}.", file_name(item))?;
    }

    let rescued = catalog.kept_hashes()?;
    let (summary, items) = summary::summarise_with(&reports, &rescued);
    for name in names {
        let matches: Vec<_> = summary
            .collections
            .iter()
            .filter(|c| c.title.eq_ignore_ascii_case(name.trim()))
            .collect();
        if matches.is_empty() {
            bail!("There is no collection called \"{name}\". Run `keevoro collections` to see the names.");
        }
        for c in matches {
            for hash in c.members.iter().filter_map(|&m| items.get(m)?.blake3.as_deref()) {
                catalog.set_choice(hash, keep)?;
            }
            writeln!(out, "{verb} {} ({}).", c.title, c.summary())?;
        }
    }
    Ok(())
}

fn file_name(item: &FoundItem) -> String {
    item.path
        .file_name()
        .map_or_else(|| item.path.display().to_string(), |n| n.to_string_lossy().to_string())
}

/// The shortest code shown for a file: the start of its content id. Long
/// enough to be unique in any real library, short enough to type.
const CODE_LEN: usize = 8;

/// Finds the one file whose content id starts with `code`.
fn find_by_code<'a>(items: &[&'a FoundItem], code: &str) -> Result<&'a FoundItem> {
    let code = code.trim().to_lowercase();
    if code.len() < 4 || !code.chars().all(|c| c.is_ascii_hexdigit()) {
        bail!("\"{code}\" isn't a photo code. Run `keevoro photos` to see the codes; they look like 9f2c4be1.");
    }
    let mut matches = items
        .iter()
        .filter(|i| i.blake3.as_deref().is_some_and(|h| h.starts_with(&code)));
    match (matches.next(), matches.next()) {
        (Some(one), None) => Ok(one),
        (None, _) => bail!("No photo or video has the code {code}. Run `keevoro photos` to see the codes."),
        (Some(_), Some(_)) => bail!("More than one photo starts with {code}. Give a few more characters of the code."),
    }
}

/// Lists a collection's files, or everything set aside as junk.
fn list_photos(out: &mut dyn Write, library: &Path, collection: Option<&str>, junk: bool) -> Result<()> {
    let catalog = open_existing(library)?;
    let reports = catalog.load_reports()?;
    let rescued = catalog.kept_hashes()?;
    let line = |item: &FoundItem, note: &str| -> Result<String> {
        let code = item
            .blake3
            .as_deref()
            .map_or("--------", |h| &h[..h.len().min(CODE_LEN)]);
        let date = item.date.map_or_else(|| "undated".to_string(), |d| d.date.to_string());
        let kept = if wanted(&catalog, item)? { "" } else { "  (not keeping)" };
        Ok(format!("  {code}  {}  {date}{note}{kept}", file_name(item)))
    };

    if junk {
        let (everything, _) = summary::unique_items(&reports);
        let set_aside: Vec<_> = everything
            .into_iter()
            .filter(|i| i.junk.is_some() && !i.blake3.as_ref().is_some_and(|h| rescued.contains(h)))
            .collect();
        if set_aside.is_empty() {
            writeln!(out, "Nothing has been set aside as junk.")?;
            return Ok(());
        }
        writeln!(out, "Set aside as junk. To keep one: keevoro keep --photo <code>")?;
        for item in set_aside {
            let reason = item.junk.map(|r| format!("  [{}]", r.describe())).unwrap_or_default();
            writeln!(out, "{}", line(item, &reason)?)?;
        }
        return Ok(());
    }

    let name = collection.unwrap_or_default();
    let (summary, items) = summary::summarise_with(&reports, &rescued);
    let matches: Vec<_> = summary
        .collections
        .iter()
        .filter(|c| c.title.eq_ignore_ascii_case(name.trim()))
        .collect();
    if matches.is_empty() {
        bail!("There is no collection called \"{name}\". Run `keevoro collections` to see the names.");
    }
    for c in matches {
        writeln!(out, "{} ({})", c.title, c.summary())?;
        for item in c.members.iter().filter_map(|&m| items.get(m)) {
            writeln!(out, "{}", line(item, "")?)?;
        }
    }
    Ok(())
}

/// Titles of collections the user has skipped entirely.
fn skipped_collections(catalog: &Catalog, reports: &[ScanReport]) -> Result<Vec<String>> {
    let rescued = catalog.kept_hashes()?;
    let (summary, items) = summary::summarise_with(reports, &rescued);
    let mut skipped = Vec::new();
    for c in &summary.collections {
        let mut any_wanted = false;
        for item in c.members.iter().filter_map(|&m| items.get(m)) {
            any_wanted |= wanted(catalog, item)?;
        }
        if !any_wanted {
            skipped.push(c.title.clone());
        }
    }
    Ok(skipped)
}

fn run_gather(
    out: &mut dyn Write,
    library: &Path,
    convert_videos: bool,
    video_tools: Option<&keevoro_media::VideoTools>,
) -> Result<()> {
    let catalog = open_existing(library)?;
    let live = std::io::stderr().is_terminal();
    let n = summary::thousands;
    // One line on the terminal, rewritten as each part of the work moves on.
    let outcome = gather_everything(library, &catalog, convert_videos, video_tools, &mut |step| {
        if !live {
            return;
        }
        let line = match step {
            GatherStep::Checking { done, total } => format!("Checking files: {} of {}", n(done), n(total)),
            GatherStep::Copying { done, total } => format!("{} of {} safely copied", n(done), n(total)),
            GatherStep::MakingPreviews { done, total } => format!("Making previews: {} of {}", n(done), n(total)),
            GatherStep::WorkingOnVideos { done, total } => format!("Working on videos: {} of {}", n(done), n(total)),
        };
        eprint!("\r\x1b[2K  {line}");
        let _ = std::io::stderr().flush();
    });
    if live {
        eprint!("\r\x1b[2K");
    }
    let Some(outcome) = outcome? else {
        writeln!(out, "There is nothing to gather yet.")?;
        return Ok(());
    };

    let report = &outcome.copied;
    if report.copied > 0 {
        writeln!(
            out,
            "Safely copied {} ({}) into your library.",
            summary::count(report.copied, "file"),
            summary::bytes(report.copied_bytes)
        )?;
    }
    if report.already_there > 0 {
        writeln!(out, "{} already there.", summary::count(report.already_there, "file"))?;
    }
    if !report.failed.is_empty() {
        writeln!(out, "Couldn't copy {}:", summary::count(report.failed.len(), "file"))?;
        for f in &report.failed {
            writeln!(out, "  {}: {}.", f.path.display(), f.reason)?;
        }
        writeln!(out, "Nothing was lost. Run `keevoro gather` again to retry.")?;
    }

    let previews = &outcome.previews;
    if previews.made > 0 {
        writeln!(out, "Made previews for {}.", summary::count(previews.made, "photo"))?;
    }
    if !previews.no_preview.is_empty() {
        writeln!(
            out,
            "{} no preview yet. The originals are safe in your library.",
            match previews.no_preview.len() {
                1 => "1 photo has".to_string(),
                many => format!("{} photos have", n(many)),
            }
        )?;
    }

    if let Some(videos) = &outcome.videos {
        if videos.posters > 0 {
            writeln!(out, "Made posters for {}.", summary::count(videos.posters, "video"))?;
        }
        let made = videos.converted + videos.repackaged;
        if made > 0 {
            let how = match videos.repackaged {
                0 => String::new(),
                r if r == made => " (repackaged without re-encoding, so nothing was lost)".to_string(),
                r => format!(" ({} of them repackaged without re-encoding)", n(r)),
            };
            writeln!(
                out,
                "Made MP4 copies of {}{how}. The originals are kept.",
                summary::count(made, "video")
            )?;
        }
        if !videos.problems.is_empty() {
            writeln!(
                out,
                "{} a problem:",
                match videos.problems.len() {
                    1 => "1 video had".to_string(),
                    many => format!("{} videos had", n(many)),
                }
            )?;
            for p in &videos.problems {
                writeln!(out, "  {}: {}.", p.path.display(), p.reason.trim_end_matches('.'))?;
            }
        }
    }
    if outcome.videos_need_ffmpeg {
        writeln!(
            out,
            "Your videos are safe in the library, but they have no posters{}: that needs ffmpeg, which isn't installed.",
            if convert_videos { " or MP4 copies" } else { "" }
        )?;
    }
    Ok(())
}

/// Checks for a newer release and, unless only checking, installs it.
/// `install` is passed in so tests can stand in for replacing the program.
fn run_update(
    out: &mut dyn Write,
    web: &dyn keevoro_update::Web,
    current: &str,
    check_only: bool,
    install: &dyn Fn(&[u8]) -> Result<(), keevoro_update::UpdateError>,
) -> Result<()> {
    let plain = |e: keevoro_update::UpdateError| anyhow::anyhow!("{e}.");
    let release = match keevoro_update::check(web, current).map_err(plain)? {
        keevoro_update::Check::UpToDate => {
            writeln!(out, "Keevoro {current} is the newest version.")?;
            return Ok(());
        }
        keevoro_update::Check::Available(release) => release,
    };
    if check_only {
        writeln!(out, "Keevoro {} is available (you have {current}).", release.version)?;
        writeln!(out, "What's new: {}", release.page)?;
        writeln!(out, "Run `keevoro update` to install it.")?;
        return Ok(());
    }
    writeln!(out, "Updating Keevoro {current} to {} ...", release.version)?;
    let program =
        keevoro_update::download(web, &release, std::env::consts::OS, std::env::consts::ARCH).map_err(plain)?;
    install(&program).map_err(plain)?;
    writeln!(out, "Keevoro {} is installed.", release.version)?;
    writeln!(out, "What's new: {}", release.page)?;
    Ok(())
}

fn run_scan(paths: &[PathBuf], no_hash: bool, for_library: bool) -> Vec<ScanReport> {
    let opts = ScanOptions {
        hash: !no_hash,
        ..Default::default()
    };
    let live = std::io::stderr().is_terminal();
    let mut reports: Vec<ScanReport> = Vec::new();
    for path in paths {
        // A remembered drive is recognised by its full path, so `D:` and
        // `D:\.` must come out the same.
        let path = if for_library {
            std::fs::canonicalize(path).unwrap_or_else(|_| path.clone())
        } else {
            path.clone()
        };
        if live {
            eprintln!("Looking through {} ...", path.display());
        }
        let report = scan(&path, &opts, &mut |p| {
            if live && p.files_seen % 250 == 0 {
                let n = |v: u64| summary::thousands(usize::try_from(v).unwrap_or(usize::MAX));
                eprint!("\r  {} photos and {} videos so far", n(p.photos), n(p.videos));
                let _ = std::io::stderr().flush();
            }
        });
        if live {
            eprint!("\r\x1b[2K");
        }
        reports.push(report);
    }
    reports
}

/// Saves each scan in the library's catalog under a friendly name.
fn remember(library: &Path, name: Option<&str>, reports: &[ScanReport]) -> Result<()> {
    let mut catalog = Catalog::open_in_library(library)
        .with_context(|| format!("Couldn't use {} as a library", library.display()))?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|d| i64::try_from(d.as_secs()).ok())
        .unwrap_or(0);
    for report in reports {
        let fallback = report.root.file_name().map_or_else(
            || report.root.display().to_string(),
            |n| n.to_string_lossy().to_string(),
        );
        catalog.save_scan(name.unwrap_or(&fallback), report, now)?;
    }
    Ok(())
}

fn print_reports(
    out: &mut dyn Write,
    reports: &[ScanReport],
    rescued: &std::collections::HashSet<String>,
    json: bool,
    show: usize,
) -> Result<()> {
    let (summary, unique) = summary::summarise_with(reports, rescued);
    if json {
        #[derive(serde::Serialize)]
        struct JsonOut<'a> {
            summary: &'a summary::Summary,
            items: &'a [&'a keevoro_scan::FoundItem],
            sources: &'a [ScanReport],
        }
        serde_json::to_writer_pretty(
            &mut *out,
            &JsonOut {
                summary: &summary,
                items: &unique,
                sources: reports,
            },
        )?;
        writeln!(out)?;
    } else {
        write!(out, "{}", summary::render(&summary, show))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn jpeg(fill: u8) -> Vec<u8> {
        let mut j = vec![
            0xFF, 0xD8, 0xFF, 0xE0, 0, 16, b'J', b'F', b'I', b'F', 0, 1, 1, 0, 0, 1, 0, 1, 0, 0,
        ];
        j.extend(std::iter::repeat_n(fill, 64));
        j
    }

    fn keevoro(args: &[&str]) -> Result<String> {
        let mut out = Vec::new();
        let secrets = connect::testing::MemorySecrets::default();
        keevoro_with(args, &secrets, &mut out)?;
        Ok(String::from_utf8(out).unwrap())
    }

    /// Runs a command against a keychain the test controls, with nobody at
    /// the keyboard to answer questions.
    fn keevoro_with(args: &[&str], secrets: &connect::testing::MemorySecrets, out: &mut Vec<u8>) -> Result<()> {
        let cli = Cli::try_parse_from(std::iter::once("keevoro").chain(args.iter().copied()))?;
        let world = connect::World {
            secrets,
            ask: &connect::testing::nobody_to_ask,
        };
        run(cli, out, &world)
    }

    /// A pretend old drive with one album, one duplicate and a system folder.
    fn drive(dir: &Path, name: &str) -> String {
        let root = dir.join(name);
        for (rel, fill) in [
            ("Holidays/Cornwall/IMG_20080812_101500.jpg", 1),
            ("Holidays/Cornwall/IMG_20080813_090000.jpg", 2),
            ("Backup/IMG_20080812_101500.jpg", 1),
            ("Windows/wallpaper.jpg", 3),
        ] {
            let p = root.join(rel);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, jpeg(fill)).unwrap();
        }
        root.to_string_lossy().to_string()
    }

    #[test]
    fn scan_prints_a_plain_summary() {
        let dir = tempfile::tempdir().unwrap();
        let text = keevoro(&["scan", &drive(dir.path(), "dell")]).unwrap();
        assert!(text.starts_with("Found 2 photos and 0 videos"), "{text}");
        assert!(text.contains("1 duplicate"), "{text}");
        assert!(text.contains("Cornwall"), "{text}");
        assert!(text.contains("Skipped 1 folder"), "{text}");
    }

    #[test]
    fn scan_json_lists_items_and_sources() {
        let dir = tempfile::tempdir().unwrap();
        let text = keevoro(&["scan", "--json", "--no-hash", &drive(dir.path(), "dell")]).unwrap();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["summary"]["photos"], 3, "without hashes every copy counts");
        assert_eq!(v["items"].as_array().unwrap().len(), 3);
        assert_eq!(v["sources"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn a_library_remembers_drives_after_they_are_unplugged() {
        let dir = tempfile::tempdir().unwrap();
        let library = dir.path().join("Keevoro Library").to_string_lossy().to_string();
        let dell = drive(dir.path(), "dell");
        let scanned = keevoro(&["scan", "--library", &library, "--name", "Dad's old Dell", &dell]).unwrap();

        fs::remove_dir_all(&dell).unwrap();
        let later = keevoro(&["collections", "--library", &library]).unwrap();
        assert_eq!(later, scanned, "the remembered summary matches the live one");

        let json = keevoro(&["collections", "--library", &library, "--json"]).unwrap();
        assert!(json.trim_start().starts_with('{'));

        let sources = keevoro(&["sources", "--library", &library]).unwrap();
        assert!(
            sources.starts_with("Dad's old Dell  3 photos and videos  ("),
            "{sources}"
        );
    }

    #[test]
    fn scanning_a_drive_again_does_not_double_count_it() {
        let dir = tempfile::tempdir().unwrap();
        let library = dir.path().join("lib").to_string_lossy().to_string();
        let dell = drive(dir.path(), "dell");
        keevoro(&["scan", "--library", &library, &dell]).unwrap();
        // The same drive, spelled differently.
        keevoro(&["scan", "--library", &library, &format!("{dell}/.")]).unwrap();
        let sources = keevoro(&["sources", "--library", &library]).unwrap();
        assert_eq!(sources.lines().count(), 1, "{sources}");
        assert!(
            sources.starts_with("dell  "),
            "unnamed drives are called after their folder: {sources}"
        );
    }

    #[test]
    fn two_drives_are_merged_into_one_picture() {
        let dir = tempfile::tempdir().unwrap();
        let library = dir.path().join("lib").to_string_lossy().to_string();
        keevoro(&[
            "scan",
            "--library",
            &library,
            &drive(dir.path(), "dell"),
            &drive(dir.path(), "backup"),
        ])
        .unwrap();
        let text = keevoro(&["collections", "--library", &library]).unwrap();
        assert!(text.starts_with("Found 2 photos and 0 videos"), "{text}");
        assert!(text.contains("4 duplicates"), "{text}");
    }

    #[test]
    fn mistakes_get_plain_answers() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("nowhere").to_string_lossy().to_string();
        let err = keevoro(&["collections", "--library", &missing]).unwrap_err();
        assert!(err.to_string().starts_with("There is no Keevoro library at"), "{err}");
        assert!(!Path::new(&missing).exists(), "looking must not create a library");
        assert!(keevoro(&["sources", "--library", &missing]).is_err());

        let err = keevoro(&["scan", "--library", &missing, "--name", "x", "a", "b"]).unwrap_err();
        assert_eq!(err.to_string(), "Give one source at a time when using --name.");
        assert!(
            keevoro(&["scan", "--name", "x", "a"]).is_err(),
            "--name needs --library"
        );
    }

    #[test]
    fn skip_keep_and_gather_tell_the_whole_story() {
        let dir = tempfile::tempdir().unwrap();
        let library = dir.path().join("Keevoro Library");
        let lib = library.to_string_lossy().to_string();
        let dell = drive(dir.path(), "dell");
        fs::write(Path::new(&dell).join("IMG_20120304_153012.jpg"), jpeg(7)).unwrap();
        keevoro(&["scan", "--library", &lib, &dell]).unwrap();

        // Leave Cornwall out, gather, and only the loose photo arrives.
        let said = keevoro(&["skip", "--library", &lib, "cornwall"]).unwrap();
        assert!(said.starts_with("Not keeping Cornwall (August 2008"), "{said}");
        assert!(keevoro(&["collections", "--library", &lib])
            .unwrap()
            .ends_with("Not keeping: Cornwall.\n"));
        let said = keevoro(&["gather", "--library", &lib]).unwrap();
        assert!(said.starts_with("Safely copied 1 file ("), "{said}");
        assert!(library
            .join("originals/By date/2012/2012-03/IMG_20120304_153012.jpg")
            .is_file());
        assert!(!library.join("originals/Holidays").exists());

        // Change of heart: Cornwall comes too, and nothing is copied twice.
        let said = keevoro(&["keep", "--library", &lib, "Cornwall"]).unwrap();
        assert!(said.starts_with("Keeping Cornwall"), "{said}");
        let said = keevoro(&["gather", "--library", &lib]).unwrap();
        assert!(said.starts_with("Safely copied 2 files ("), "{said}");
        assert!(said.contains("1 file already there."), "{said}");
        assert!(library
            .join("originals/Holidays/Cornwall/IMG_20080812_101500.jpg")
            .is_file());

        let said = keevoro(&["gather", "--library", &lib]).unwrap();
        assert!(said.starts_with("3 files already there.\n"), "{said}");
        assert!(
            said.ends_with("3 photos have no preview yet. The originals are safe in your library.\n"),
            "{said}"
        );
    }

    #[test]
    fn gather_reports_what_it_could_not_copy() {
        let dir = tempfile::tempdir().unwrap();
        let lib = dir.path().join("lib").to_string_lossy().to_string();
        let dell = drive(dir.path(), "dell");
        keevoro(&["scan", "--library", &lib, &dell]).unwrap();
        fs::remove_dir_all(Path::new(&dell).join("Holidays")).unwrap();
        fs::remove_dir_all(Path::new(&dell).join("Backup")).unwrap();
        let said = keevoro(&["gather", "--library", &lib]).unwrap();
        assert!(said.starts_with("Couldn't copy 2 files:"), "{said}");
        assert!(said.contains("Couldn't read it from the source"), "{said}");
        assert!(
            said.ends_with("Nothing was lost. Run `keevoro gather` again to retry.\n"),
            "{said}"
        );
    }

    #[test]
    fn gather_refuses_a_library_inside_a_source() {
        let dir = tempfile::tempdir().unwrap();
        let dell = drive(dir.path(), "dell");
        let lib = Path::new(&dell).join("Keevoro Library").to_string_lossy().to_string();
        keevoro(&["scan", "--library", &lib, &dell]).unwrap();
        let err = keevoro(&["gather", "--library", &lib]).unwrap_err();
        assert!(err.to_string().contains("Keevoro never writes to those"), "{err}");
        assert!(!Path::new(&lib).join("originals").exists());
    }

    #[test]
    fn choosing_and_gathering_need_something_to_work_on() {
        let dir = tempfile::tempdir().unwrap();
        let lib = dir.path().join("lib").to_string_lossy().to_string();
        keevoro(&["scan", "--library", &lib, &drive(dir.path(), "dell")]).unwrap();
        let err = keevoro(&["skip", "--library", &lib, "Narnia"]).unwrap_err();
        assert!(
            err.to_string().starts_with("There is no collection called \"Narnia\"."),
            "{err}"
        );

        keevoro(&["skip", "--library", &lib, "Cornwall"]).unwrap();
        assert_eq!(
            keevoro(&["gather", "--library", &lib]).unwrap(),
            "There is nothing to gather yet.\n"
        );
    }

    fn real_jpeg(shade: u8) -> Vec<u8> {
        let img = image::RgbImage::from_pixel(640, 480, image::Rgb([shade, 120, 60]));
        let mut bytes = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Jpeg)
            .unwrap();
        bytes
    }

    /// Every file under `root`, as sorted relative paths.
    fn files(root: &Path) -> Vec<String> {
        fn walk(dir: &Path, root: &Path, out: &mut Vec<String>) {
            for e in fs::read_dir(dir).unwrap() {
                let p = e.unwrap().path();
                if p.is_dir() {
                    walk(&p, root, out);
                } else {
                    out.push(p.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/"));
                }
            }
        }
        let mut out = Vec::new();
        walk(root, root, &mut out);
        out.sort();
        out
    }

    #[test]
    fn from_an_old_drive_to_a_backup_with_an_index() {
        let dir = tempfile::tempdir().unwrap();
        let lib_dir = dir.path().join("Keevoro Library");
        let lib = lib_dir.to_string_lossy().to_string();
        let backup = dir.path().join("backup disk");
        let to = backup.to_string_lossy().to_string();
        let dell = dir.path().join("dell");
        for (rel, bytes) in [
            ("Holidays/Spain 2009/IMG_20090714_210311.jpg", real_jpeg(10)),
            ("Holidays/Spain 2009/IMG_20090715_090000.jpg", real_jpeg(20)),
            ("DCIM/100CANON/IMG_20120304_153012.jpg", real_jpeg(30)),
            ("DCIM/100CANON/odd.jpg", jpeg(4)),
        ] {
            fs::create_dir_all(dell.join(rel).parent().unwrap()).unwrap();
            fs::write(dell.join(rel), bytes).unwrap();
        }
        let dell = dell.to_string_lossy().to_string();

        assert!(
            keevoro(&["publish", "--library", &lib, "--to-folder", &to]).is_err(),
            "no library yet"
        );
        keevoro(&["scan", "--library", &lib, &dell]).unwrap();
        let early = keevoro(&["publish", "--library", &lib, "--to-folder", &to]).unwrap();
        assert!(
            early.starts_with("There is nothing in the library to back up yet."),
            "{early}"
        );

        let gathered = keevoro(&["gather", "--library", &lib]).unwrap();
        assert!(gathered.contains("Made previews for 3 photos."), "{gathered}");
        assert!(gathered.contains("1 photo has no preview yet."), "{gathered}");

        let said = keevoro(&[
            "publish",
            "--library",
            &lib,
            "--to-folder",
            &to,
            "--base-url",
            "https://photos.example.com",
        ])
        .unwrap();
        // Four originals, and a thumbnail and display copy for three of them.
        assert!(said.starts_with("Safely backed up 10 files ("), "{said}");
        assert!(said.contains("Index written for 4 files in 4 albums."), "{said}");
        let link = said.lines().last().unwrap();
        let key = link
            .strip_prefix("Your private index: https://photos.example.com/")
            .unwrap();
        assert!(
            key.starts_with("index/") && key.ends_with(".kvx") && key.len() == 32,
            "{key}"
        );

        // The backup mirrors the library, minus the catalog.
        let stored: Vec<String> = files(&backup)
            .into_iter()
            // The index and the note explaining the backup are written there, not copied.
            .filter(|f| !f.starts_with("index/") && f != keevoro_engine::layout::GUIDE_NAME)
            .collect();
        let local: Vec<String> = files(&lib_dir)
            .into_iter()
            .filter(|f| !f.starts_with(".keevoro/"))
            .collect();
        assert_eq!(stored, local);
        assert!(stored.contains(&"originals/Holidays/Spain 2009/IMG_20090714_210311.jpg".to_string()));

        let index: serde_json::Value = serde_json::from_slice(&fs::read(backup.join(key)).unwrap()).unwrap();
        assert_eq!(index["schema"], "keevoro.index/2");
        assert_eq!(index["baseUrl"], "https://photos.example.com/");
        let spain = index["albums"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["id"] == "holidays-spain-2009")
            .unwrap();
        assert_eq!(spain["parent"], "holidays");
        assert_eq!(spain["dateRange"], serde_json::json!(["2009-07-14", "2009-07-15"]));
        let cover = &index["items"][spain["cover"].as_str().unwrap()];
        assert_eq!((cover["w"].as_u64(), cover["h"].as_u64()), (Some(640), Some(480)));
        assert_eq!(cover["renditions"], serde_json::json!(["thumb", "display"]));
        assert!(cover["blurhash"].is_string());
        let odd = index["items"]
            .as_object()
            .unwrap()
            .values()
            .find(|i| i["path"] == "originals/Undated/odd.jpg")
            .unwrap();
        assert_eq!(odd["renditions"], serde_json::json!([]));

        let gallery: serde_json::Value =
            serde_json::from_slice(&fs::read(backup.join("index/gallery.json")).unwrap()).unwrap();
        let album = gallery["albums"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["title"] == "Holidays / Spain 2009")
            .unwrap();
        assert_eq!(album["photos"].as_array().unwrap().len(), 2);
        let thumb = album["photos"][0]["thumbnail"].as_str().unwrap();
        assert!(
            thumb.starts_with("https://photos.example.com/renditions/thumb/"),
            "{thumb}"
        );

        // Again: nothing new to send, and the private link stays the same.
        let again = keevoro(&[
            "publish",
            "--library",
            &lib,
            "--to-folder",
            &to,
            "--base-url",
            "https://photos.example.com",
        ])
        .unwrap();
        assert!(again.starts_with("10 files already there.\n"), "{again}");
        assert_eq!(again.lines().last().unwrap(), link);
        assert_eq!(files(&backup).iter().filter(|f| f.ends_with(".kvx")).count(), 1);

        // Skipping an album afterwards takes it out of the index, but
        // nothing already backed up is deleted.
        keevoro(&["skip", "--library", &lib, "Spain 2009"]).unwrap();
        let after = keevoro(&["publish", "--library", &lib, "--to-folder", &to]).unwrap();
        assert!(after.contains("Index written for 2 files in 2 albums."), "{after}");
        assert!(
            after.lines().last().unwrap().starts_with("Your private index: index/"),
            "{after}"
        );
        assert!(backup
            .join("originals/Holidays/Spain 2009/IMG_20090714_210311.jpg")
            .is_file());
    }

    #[test]
    fn bucket_backups_ask_plainly_for_what_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        let lib = dir.path().join("lib").to_string_lossy().to_string();
        let dell = dir.path().join("dell");
        fs::create_dir_all(&dell).unwrap();
        fs::write(dell.join("a.jpg"), real_jpeg(1)).unwrap();
        keevoro(&["scan", "--library", &lib, &dell.to_string_lossy()]).unwrap();
        keevoro(&["gather", "--library", &lib]).unwrap();

        let nowhere = keevoro(&["publish", "--library", &lib]).unwrap_err();
        assert!(nowhere.to_string().starts_with("Say where to back up to"), "{nowhere}");
        assert!(
            keevoro(&["publish", "--library", &lib, "--to-s3", "eu-west-2"]).is_err(),
            "a bucket is needed"
        );
        let both = [
            "publish",
            "--library",
            &lib,
            "--to-folder",
            "x",
            "--to-s3",
            "eu-west-2",
            "--bucket",
            "b",
        ];
        assert!(keevoro(&both).is_err(), "one destination at a time");

        // No keys in the environment: say exactly what to set. (Skipped
        // where CI has set keys for the test server.)
        if std::env::var_os(publish::ACCESS_KEY_VAR).is_none() {
            let no_keys = keevoro(&[
                "publish",
                "--library",
                &lib,
                "--to-s3",
                "eu-west-2",
                "--bucket",
                "family",
            ])
            .unwrap_err();
            assert!(no_keys.to_string().contains("KEEVORO_ACCESS_KEY_ID"), "{no_keys}");
        }
    }

    /// Runs only where a test S3 server is configured (CI starts MinIO);
    /// see `keevoro-storage-s3/src/tests.rs` for the variables.
    #[test]
    fn backs_up_to_an_s3_compatible_bucket() {
        let (Ok(endpoint), Ok(bucket)) = (
            std::env::var("KEEVORO_S3_TEST_ENDPOINT"),
            std::env::var("KEEVORO_S3_TEST_BUCKET"),
        ) else {
            eprintln!("skipped: no KEEVORO_S3_TEST_* server configured");
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let lib = dir.path().join("lib").to_string_lossy().to_string();
        // A folder name no other test run has used, so reruns against the
        // same bucket never meet each other's files.
        let album = format!("Bucket test {}", std::process::id());
        let dell = dir.path().join("dell");
        fs::create_dir_all(dell.join(&album)).unwrap();
        fs::write(dell.join(&album).join("a.jpg"), real_jpeg(1)).unwrap();
        fs::write(dell.join(&album).join("b.jpg"), real_jpeg(2)).unwrap();
        keevoro(&["scan", "--library", &lib, &dell.to_string_lossy()]).unwrap();
        keevoro(&["gather", "--library", &lib]).unwrap();

        let to = [
            "publish",
            "--library",
            &lib,
            "--to-endpoint",
            &endpoint,
            "--bucket",
            &bucket,
        ];
        let said = keevoro(&to).unwrap();
        // Two originals, each with a thumbnail and a display copy.
        assert!(said.starts_with("Safely backed up 6 files ("), "{said}");
        assert!(
            !said.contains("Warning"),
            "a private bucket gives no listing warning: {said}"
        );
        assert!(said.contains("Index written for 2 files in 1 album."), "{said}");
        let link = said.lines().last().unwrap().to_string();
        assert!(
            link.starts_with("Your private index: /index/") && link.ends_with(".kvx"),
            "{link}"
        );

        let again = keevoro(&to).unwrap();
        assert!(again.starts_with("6 files already there.\n"), "{again}");
        assert_eq!(again.lines().last().unwrap(), link);

        // With the catalog's memory of the upload gone, the bucket itself
        // still stops originals being sent or replaced a second time.
        let catalog = Catalog::open_in_library(Path::new(&lib)).unwrap();
        catalog.forget_uploads(&format!("s3:{endpoint}/{bucket}")).unwrap();
        drop(catalog);
        let relearned = keevoro(&to).unwrap();
        assert!(relearned.contains("2 files already there."), "{relearned}");
        assert!(
            relearned.starts_with("Safely backed up 4 files ("),
            "previews are simply re-sent: {relearned}"
        );

        let wrong = [
            "publish",
            "--library",
            &lib,
            "--to-endpoint",
            "http://127.0.0.1:9",
            "--bucket",
            &bucket,
        ];
        let err = keevoro(&wrong).unwrap_err();
        assert!(err.to_string().starts_with("Couldn't connect to the storage."), "{err}");
    }

    /// A pretend GitHub with one release, built the way the Release
    /// workflow builds a real one.
    struct FakeGitHub {
        tag: &'static str,
        program: Vec<u8>,
        sums_for: Vec<u8>,
    }

    impl keevoro_update::Web for FakeGitHub {
        fn text(&self, _url: &str) -> Result<String, keevoro_update::UpdateError> {
            let asset = keevoro_update::asset_for(std::env::consts::OS, std::env::consts::ARCH).unwrap();
            Ok(serde_json::json!({
                "tag_name": self.tag,
                "html_url": format!("https://github.com/markcastle/keevoro/releases/tag/{}", self.tag),
                "assets": [{ "name": asset, "url": "program" }, { "name": "SHA256SUMS", "url": "sums" }],
            })
            .to_string())
        }
        fn bytes(&self, url: &str) -> Result<Vec<u8>, keevoro_update::UpdateError> {
            let asset = keevoro_update::asset_for(std::env::consts::OS, std::env::consts::ARCH).unwrap();
            Ok(match url {
                "program" => self.program.clone(),
                _ => format!("{}  {asset}\n", keevoro_update::sha256_hex(&self.sums_for)).into_bytes(),
            })
        }
    }

    fn update(web: &FakeGitHub, current: &str, check: bool) -> (Result<String>, Vec<Vec<u8>>) {
        let installed = std::cell::RefCell::new(Vec::new());
        let mut out = Vec::new();
        let result = run_update(&mut out, web, current, check, &|program| {
            installed.borrow_mut().push(program.to_vec());
            Ok(())
        });
        (result.map(|()| String::from_utf8(out).unwrap()), installed.into_inner())
    }

    #[test]
    fn update_installs_a_newer_version_and_says_what_is_new() {
        let web = FakeGitHub {
            tag: "v0.3.0",
            program: b"new keevoro".to_vec(),
            sums_for: b"new keevoro".to_vec(),
        };
        let (said, installed) = update(&web, "0.2.0", false);
        assert_eq!(
            said.unwrap(),
            "Updating Keevoro 0.2.0 to 0.3.0 ...\nKeevoro 0.3.0 is installed.\n\
             What's new: https://github.com/markcastle/keevoro/releases/tag/v0.3.0\n"
        );
        assert_eq!(installed, [b"new keevoro".to_vec()]);

        // Only looking changes nothing.
        let (said, installed) = update(&web, "0.2.0", true);
        assert!(said
            .unwrap()
            .starts_with("Keevoro 0.3.0 is available (you have 0.2.0)."));
        assert!(installed.is_empty());

        // Already there, or ahead of it: nothing to do.
        for current in ["0.3.0", "0.4.0"] {
            let (said, installed) = update(&web, current, false);
            assert_eq!(said.unwrap(), format!("Keevoro {current} is the newest version.\n"));
            assert!(installed.is_empty());
        }
    }

    #[test]
    fn update_never_installs_a_download_that_fails_its_checksum() {
        let web = FakeGitHub {
            tag: "v0.3.0",
            program: b"damaged on the way".to_vec(),
            sums_for: b"new keevoro".to_vec(),
        };
        let (said, installed) = update(&web, "0.2.0", false);
        assert!(said.unwrap_err().to_string().contains("Nothing was changed"));
        assert!(installed.is_empty());
    }

    /// A one-second test video, or `None` (with a note) without ffmpeg.
    fn test_video(name: &str, codec_args: &[&str]) -> Option<Vec<u8>> {
        let Some(tools) = keevoro_media::VideoTools::find() else {
            eprintln!("skipped: ffmpeg is not installed");
            return None;
        };
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join(name);
        let ok = std::process::Command::new(&tools.ffmpeg)
            .args([
                "-v",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
                "testsrc=duration=1:size=320x240:rate=25",
            ])
            .args(["-f", "lavfi", "-i", "sine=frequency=440:duration=1"])
            .args(codec_args)
            .arg(&out)
            .status()
            .unwrap()
            .success();
        assert!(ok);
        Some(fs::read(out).unwrap())
    }

    #[test]
    fn old_videos_get_posters_mp4_copies_and_a_place_in_the_index() {
        let Some(avi) = test_video("old.avi", &["-c:v", "mpeg4", "-c:a", "mp3"]) else {
            return;
        };
        let mov = test_video("c.mov", &["-c:v", "libx264", "-pix_fmt", "yuv420p", "-c:a", "aac"]).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let lib_dir = dir.path().join("lib");
        let lib = lib_dir.to_string_lossy().to_string();
        let backup = dir.path().join("backup");
        let to = backup.to_string_lossy().to_string();
        let dell = dir.path().join("dell");
        fs::create_dir_all(dell.join("Wedding")).unwrap();
        fs::write(dell.join("Wedding/speech.avi"), avi).unwrap();
        fs::write(dell.join("Wedding/dance.mov"), mov).unwrap();
        keevoro(&["scan", "--library", &lib, &dell.to_string_lossy()]).unwrap();

        // Without asking for conversion: posters only.
        let said = keevoro(&["gather", "--library", &lib]).unwrap();
        assert!(said.contains("Made posters for 2 videos."), "{said}");
        assert!(!said.contains("MP4"), "{said}");
        assert!(!lib_dir.join("renditions/video").exists());

        let said = keevoro(&["gather", "--library", &lib, "--convert-videos"]).unwrap();
        assert!(
            said.contains(
                "Made MP4 copies of 2 videos (1 of them repackaged without re-encoding). The originals are kept."
            ),
            "{said}"
        );
        assert!(lib_dir.join("originals/Wedding/speech.avi").is_file());

        let said = keevoro(&["publish", "--library", &lib, "--to-folder", &to]).unwrap();
        // Two originals, two posters, two MP4 copies.
        assert!(said.starts_with("Safely backed up 6 files ("), "{said}");
        let key = said.lines().last().unwrap().rsplit("index/").next().unwrap();
        let index: serde_json::Value =
            serde_json::from_slice(&fs::read(backup.join("index").join(key)).unwrap()).unwrap();
        let speech = index["items"]
            .as_object()
            .unwrap()
            .iter()
            .find(|(_, item)| item["path"] == "originals/Wedding/speech.avi")
            .unwrap();
        assert_eq!(speech.1["type"], "video");
        assert_eq!(speech.1["renditions"], serde_json::json!(["poster", "video"]));
        assert_eq!((speech.1["w"].as_u64(), speech.1["h"].as_u64()), (Some(320), Some(240)));
        assert!(
            (speech.1["duration"].as_f64().unwrap() - 1.0).abs() < 0.2,
            "{}",
            speech.1["duration"]
        );
        assert!(backup.join(format!("renditions/video/{}.mp4", speech.0)).is_file());
        assert!(backup.join(format!("renditions/poster/{}.webp", speech.0)).is_file());
    }

    #[test]
    fn without_ffmpeg_videos_are_still_gathered_and_the_gap_is_explained() {
        let dir = tempfile::tempdir().unwrap();
        let lib_dir = dir.path().join("lib");
        let lib = lib_dir.to_string_lossy().to_string();
        let dell = dir.path().join("dell");
        fs::create_dir_all(&dell).unwrap();
        fs::write(
            dell.join("clip.mp4"),
            b"\0\0\0\x18ftypisom\0\0\x02\0isomiso2 pretend video",
        )
        .unwrap();
        keevoro(&["scan", "--library", &lib, &dell.to_string_lossy()]).unwrap();

        let mut out = Vec::new();
        run_gather(&mut out, &lib_dir, true, None).unwrap();
        let said = String::from_utf8(out).unwrap();
        assert!(said.starts_with("Safely copied 1 file ("), "{said}");
        if keevoro_media::video::can_make_posters(None) {
            // Windows and macOS read videos themselves, so they try, and say
            // plainly that this (pretend) one cannot be played.
            assert!(said.contains("1 video had a problem"), "{said}");
            assert!(said.contains("can't play this video"), "{said}");
        } else {
            assert!(
                said.ends_with("they have no posters or MP4 copies: that needs ffmpeg, which isn't installed.\n"),
                "{said}"
            );
            let mut out = Vec::new();
            run_gather(&mut out, &lib_dir, false, None).unwrap();
            assert!(String::from_utf8(out)
                .unwrap()
                .contains("no posters: that needs ffmpeg"));
        }
    }

    #[test]
    fn the_private_link_can_be_changed() {
        let dir = tempfile::tempdir().unwrap();
        let lib = dir.path().join("lib").to_string_lossy().to_string();
        let backup = dir.path().join("backup");
        let to = backup.to_string_lossy().to_string();
        let dell = dir.path().join("dell");
        fs::create_dir_all(&dell).unwrap();
        fs::write(dell.join("a.jpg"), real_jpeg(1)).unwrap();
        keevoro(&["scan", "--library", &lib, &dell.to_string_lossy()]).unwrap();
        keevoro(&["gather", "--library", &lib]).unwrap();

        let index_files = || {
            let mut names: Vec<String> = fs::read_dir(backup.join("index"))
                .unwrap()
                .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
                .filter(|n| n.ends_with(".kvx"))
                .collect();
            names.sort();
            names
        };
        let first = keevoro(&["publish", "--library", &lib, "--to-folder", &to]).unwrap();
        assert!(
            !first.contains("link has changed"),
            "nothing to change the first time: {first}"
        );
        let old = index_files();
        assert_eq!(old.len(), 1);

        let said = keevoro(&["publish", "--library", &lib, "--to-folder", &to, "--new-link"]).unwrap();
        assert!(
            said.contains("Your private link has changed. The old one no longer works."),
            "{said}"
        );
        let new = index_files();
        assert_eq!(new.len(), 1, "the old index is gone");
        assert_ne!(new, old);
        assert!(said.lines().last().unwrap().ends_with(&new[0]));

        // And the new link is the one that sticks.
        let again = keevoro(&["publish", "--library", &lib, "--to-folder", &to]).unwrap();
        assert!(again.lines().last().unwrap().ends_with(&new[0]));
        assert_eq!(index_files(), new);
    }

    #[test]
    fn connect_once_then_publish_needs_no_destination() {
        let dir = tempfile::tempdir().unwrap();
        let lib = dir.path().join("lib").to_string_lossy().to_string();
        let backup = dir.path().join("backup");
        let to = backup.to_string_lossy().to_string();
        let dell = dir.path().join("dell");
        fs::create_dir_all(&dell).unwrap();
        fs::write(dell.join("a.jpg"), real_jpeg(1)).unwrap();

        // Connecting can be the very first thing done with a library.
        let said = keevoro(&[
            "connect",
            "--library",
            &lib,
            "--to-folder",
            &to,
            "--base-url",
            "https://photos.example.com",
        ])
        .unwrap();
        assert!(said.starts_with(&format!("Connected to the folder {to}.")), "{said}");
        assert!(!said.contains("keychain"), "a folder has no keys: {said}");

        keevoro(&["scan", "--library", &lib, &dell.to_string_lossy()]).unwrap();
        keevoro(&["gather", "--library", &lib]).unwrap();
        let said = keevoro(&["publish", "--library", &lib]).unwrap();
        assert!(said.starts_with("Safely backed up 3 files ("), "{said}");
        assert!(backup.join("originals/Undated/a.jpg").is_file());
        assert!(
            said.lines()
                .last()
                .unwrap()
                .starts_with("Your private index: https://photos.example.com/index/"),
            "the web address was remembered too: {said}"
        );

        // A destination given on the command line still wins, once.
        let elsewhere = dir.path().join("elsewhere").to_string_lossy().to_string();
        let said = keevoro(&["publish", "--library", &lib, "--to-folder", &elsewhere]).unwrap();
        assert!(said.starts_with("Safely backed up 3 files ("), "{said}");

        let said = keevoro(&["disconnect", "--library", &lib]).unwrap();
        assert!(said.starts_with("Disconnected from the folder"), "{said}");
        assert!(said.contains("Nothing stored there was changed."));
        assert!(backup.join("originals/Undated/a.jpg").is_file());
        let err = keevoro(&["publish", "--library", &lib]).unwrap_err();
        assert!(err.to_string().contains("run `keevoro connect` once"), "{err}");
        assert_eq!(
            keevoro(&["disconnect", "--library", &lib]).unwrap(),
            "This library isn't connected to a backup destination.\n"
        );
        assert!(
            keevoro(&["connect", "--library", &lib]).is_err(),
            "connect needs a destination"
        );
    }

    #[test]
    fn connecting_to_a_bucket_asks_for_keys_and_saves_nothing_if_they_do_not_work() {
        if std::env::var_os(publish::ACCESS_KEY_VAR).is_some() {
            return; // CI has keys in the environment; nobody would be asked.
        }
        let dir = tempfile::tempdir().unwrap();
        let lib = dir.path().join("lib").to_string_lossy().to_string();
        let args = [
            "connect",
            "--library",
            &lib,
            "--to-endpoint",
            "http://127.0.0.1:9",
            "--bucket",
            "family",
        ];

        // Nobody at the keyboard and nothing in the environment.
        let err = keevoro(&args).unwrap_err();
        assert!(err.to_string().contains("KEEVORO_ACCESS_KEY_ID"), "{err}");

        // Someone pastes keys, but nothing answers at that address.
        let asked = std::cell::RefCell::new(Vec::new());
        let secrets = connect::testing::MemorySecrets::default();
        let world = connect::World {
            secrets: &secrets,
            ask: &|question, hidden| {
                asked.borrow_mut().push((question.to_string(), hidden));
                Ok("pasted".to_string())
            },
        };
        let cli = Cli::try_parse_from(std::iter::once("keevoro").chain(args)).unwrap();
        let err = run(cli, &mut Vec::new(), &world).unwrap_err();
        assert!(err.to_string().starts_with("Couldn't connect to the storage."), "{err}");
        let asked = asked.into_inner();
        assert_eq!(asked.len(), 2);
        assert_eq!((asked[0].1, asked[1].1), (false, true), "only the secret is hidden");
        assert!(secrets.0.borrow().is_empty(), "keys that don't work are not kept");
        let catalog = Catalog::open_in_library(Path::new(&lib)).unwrap();
        assert_eq!(connect::Saved::load(&catalog).unwrap(), None, "nor is the destination");
    }

    /// Runs only where a test S3 server is configured (CI starts MinIO).
    #[test]
    fn a_connected_bucket_is_backed_up_to_with_keys_from_the_keychain() {
        let (Ok(endpoint), Ok(bucket)) = (
            std::env::var("KEEVORO_S3_TEST_ENDPOINT"),
            std::env::var("KEEVORO_S3_TEST_BUCKET"),
        ) else {
            eprintln!("skipped: no KEEVORO_S3_TEST_* server configured");
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let lib = dir.path().join("lib").to_string_lossy().to_string();
        let album = format!("Keychain test {}", std::process::id());
        let dell = dir.path().join("dell");
        fs::create_dir_all(dell.join(&album)).unwrap();
        fs::write(dell.join(&album).join("a.jpg"), real_jpeg(7)).unwrap();

        let secrets = connect::testing::MemorySecrets::default();
        let mut out = Vec::new();
        keevoro_with(
            &[
                "connect",
                "--library",
                &lib,
                "--to-endpoint",
                &endpoint,
                "--bucket",
                &bucket,
            ],
            &secrets,
            &mut out,
        )
        .unwrap();
        let said = String::from_utf8(out).unwrap();
        assert!(
            said.contains("The keys are saved in this computer's keychain, not in the library."),
            "{said}"
        );
        assert_eq!(secrets.0.borrow().len(), 1);
        // Nothing secret is in the library's catalog.
        let catalog_bytes = fs::read(Path::new(&lib).join(".keevoro/catalog.sqlite")).unwrap();
        let secret = std::env::var("KEEVORO_S3_TEST_SECRET_KEY").unwrap();
        assert!(!catalog_bytes.windows(secret.len()).any(|w| w == secret.as_bytes()));

        let mut out = Vec::new();
        keevoro_with(
            &["scan", "--library", &lib, &dell.to_string_lossy()],
            &secrets,
            &mut out,
        )
        .unwrap();
        keevoro_with(&["gather", "--library", &lib], &secrets, &mut out).unwrap();
        let mut out = Vec::new();
        keevoro_with(&["publish", "--library", &lib], &secrets, &mut out).unwrap();
        assert!(String::from_utf8(out)
            .unwrap()
            .starts_with("Safely backed up 3 files ("));

        let mut out = Vec::new();
        keevoro_with(&["disconnect", "--library", &lib], &secrets, &mut out).unwrap();
        assert!(secrets.0.borrow().is_empty(), "disconnecting removes the keys");
    }

    #[test]
    fn previews_that_failed_can_be_tried_again() {
        let dir = tempfile::tempdir().unwrap();
        let lib_dir = dir.path().join("lib");
        let lib = lib_dir.to_string_lossy().to_string();
        let dell = dir.path().join("dell");
        fs::create_dir_all(&dell).unwrap();
        fs::write(dell.join("odd.jpg"), jpeg(4)).unwrap();
        keevoro(&["scan", "--library", &lib, &dell.to_string_lossy()]).unwrap();
        keevoro(&["gather", "--library", &lib]).unwrap();

        // The library's copy is repaired (standing in for Keevoro learning
        // a format). An ordinary gather does not look again; a retry does.
        fs::write(lib_dir.join("originals/Undated/odd.jpg"), real_jpeg(9)).unwrap();
        let said = keevoro(&["gather", "--library", &lib]).unwrap();
        assert!(said.contains("1 photo has no preview yet."), "{said}");
        let said = keevoro(&["gather", "--library", &lib, "--retry-previews"]).unwrap();
        assert!(said.contains("Made previews for 1 photo."), "{said}");
    }

    /// A PNG header claiming the given size; enough for the scanner.
    fn png(width: u32, height: u32, salt: u8) -> Vec<u8> {
        let mut p = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
        p.extend_from_slice(&width.to_be_bytes());
        p.extend_from_slice(&height.to_be_bytes());
        p.extend_from_slice(&[8, 2, 0, 0, 0, salt, salt, salt, salt]);
        p
    }

    /// The code `keevoro photos` shows next to a file name.
    fn code_for(listing: &str, name: &str) -> String {
        let line = listing
            .lines()
            .find(|l| l.contains(name))
            .unwrap_or_else(|| panic!("{name} not in:\n{listing}"));
        line.split_whitespace().next().unwrap().to_string()
    }

    #[test]
    fn single_photos_can_be_left_out_and_put_back() {
        let dir = tempfile::tempdir().unwrap();
        let lib_dir = dir.path().join("lib");
        let lib = lib_dir.to_string_lossy().to_string();
        let dell = dir.path().join("dell");
        fs::create_dir_all(dell.join("Cornwall")).unwrap();
        fs::write(dell.join("Cornwall/beach.png"), png(3000, 2000, 1)).unwrap();
        fs::write(dell.join("Cornwall/blurry.png"), png(3000, 2000, 2)).unwrap();
        keevoro(&["scan", "--library", &lib, &dell.to_string_lossy()]).unwrap();

        let listing = keevoro(&["photos", "--library", &lib, "cornwall"]).unwrap();
        assert!(listing.starts_with("Cornwall (2 photos)\n"), "{listing}");
        let blurry = code_for(&listing, "blurry.png");
        assert_eq!(blurry.len(), 8);

        let said = keevoro(&["skip", "--library", &lib, "--photo", &blurry]).unwrap();
        assert_eq!(said, "Not keeping blurry.png.\n");
        let listing = keevoro(&["photos", "--library", &lib, "Cornwall"]).unwrap();
        assert!(
            listing
                .lines()
                .any(|l| l.contains("blurry.png") && l.ends_with("(not keeping)")),
            "{listing}"
        );
        assert!(
            listing
                .lines()
                .any(|l| l.contains("beach.png") && !l.contains("not keeping")),
            "{listing}"
        );

        let said = keevoro(&["gather", "--library", &lib]).unwrap();
        assert!(said.starts_with("Safely copied 1 file ("), "{said}");
        assert!(lib_dir.join("originals/Cornwall/beach.png").is_file());
        assert!(!lib_dir.join("originals/Cornwall/blurry.png").exists());

        // A longer code works too, and so does changing your mind.
        let said = keevoro(&["keep", "--library", &lib, "--photo", &blurry.to_uppercase()]).unwrap();
        assert_eq!(said, "Keeping blurry.png.\n");
        let said = keevoro(&["gather", "--library", &lib]).unwrap();
        assert!(said.starts_with("Safely copied 1 file ("), "{said}");
        assert!(lib_dir.join("originals/Cornwall/blurry.png").is_file());
    }

    #[test]
    fn a_picture_set_aside_as_junk_can_be_kept_after_all() {
        let dir = tempfile::tempdir().unwrap();
        let lib_dir = dir.path().join("lib");
        let lib = lib_dir.to_string_lossy().to_string();
        let dell = dir.path().join("dell");
        fs::create_dir_all(dell.join("Grandad")).unwrap();
        fs::write(dell.join("Grandad/portrait.png"), png(3000, 2000, 1)).unwrap();
        // A tiny scan of an old print: small, but the only copy there is.
        fs::write(dell.join("Grandad/1952.png"), png(240, 180, 2)).unwrap();
        keevoro(&["scan", "--library", &lib, &dell.to_string_lossy()]).unwrap();

        assert_eq!(
            keevoro(&["photos", "--library", &lib, "Grandad"])
                .unwrap()
                .lines()
                .count(),
            2,
            "junk is not listed"
        );
        let junk = keevoro(&["photos", "--library", &lib, "--junk"]).unwrap();
        assert!(
            junk.starts_with("Set aside as junk. To keep one: keevoro keep --photo <code>\n"),
            "{junk}"
        );
        assert!(
            junk.contains("1952.png") && junk.contains("[Tiny images (under 300 px)]"),
            "{junk}"
        );

        let code = code_for(&junk, "1952.png");
        keevoro(&["keep", "--library", &lib, "--photo", &code]).unwrap();
        assert_eq!(
            keevoro(&["photos", "--library", &lib, "--junk"]).unwrap(),
            "Nothing has been set aside as junk.\n"
        );
        // It is back in its album, in the summary, and gets gathered.
        let listing = keevoro(&["photos", "--library", &lib, "Grandad"]).unwrap();
        assert!(
            listing.starts_with("Grandad (2 photos)\n") && listing.contains("1952.png"),
            "{listing}"
        );
        assert!(keevoro(&["collections", "--library", &lib])
            .unwrap()
            .starts_with("Found 2 photos"));
        keevoro(&["gather", "--library", &lib]).unwrap();
        assert!(lib_dir.join("originals/Grandad/1952.png").is_file());
    }

    #[test]
    fn photo_codes_that_do_not_fit_get_plain_answers() {
        let dir = tempfile::tempdir().unwrap();
        let lib = dir.path().join("lib").to_string_lossy().to_string();
        keevoro(&["scan", "--library", &lib, &drive(dir.path(), "dell")]).unwrap();
        for (code, expected) in [
            ("zz", "isn't a photo code"),
            ("ab", "isn't a photo code"),
            ("0000000000", "No photo or video has the code"),
        ] {
            let err = keevoro(&["skip", "--library", &lib, "--photo", code]).unwrap_err();
            assert!(err.to_string().contains(expected), "{code}: {err}");
        }
        assert!(
            keevoro(&["skip", "--library", &lib]).is_err(),
            "something must be named"
        );
        assert!(
            keevoro(&["photos", "--library", &lib]).is_err(),
            "a collection or --junk is needed"
        );
        let err = keevoro(&["photos", "--library", &lib, "Narnia"]).unwrap_err();
        assert!(err.to_string().starts_with("There is no collection called \"Narnia\"."));
        // Two files sharing the start of a code must be told apart.
        let a = keevoro_scan::FoundItem {
            path: "a.jpg".into(),
            kind: keevoro_core::MediaKind::Photo,
            mime: "image/jpeg".into(),
            bytes: 1,
            blake3: Some("abcd1111".into()),
            hashed: true,
            date: None,
            folder: Default::default(),
            width: None,
            height: None,
            junk: None,
        };
        let mut b = a.clone();
        b.blake3 = Some("abcd2222".into());
        let err = find_by_code(&[&a, &b], "abcd").unwrap_err();
        assert!(
            err.to_string().starts_with("More than one photo starts with abcd"),
            "{err}"
        );
        assert_eq!(find_by_code(&[&a, &b], "abcd2").unwrap().blake3, b.blake3);
    }

    #[test]
    fn check_confirms_a_healthy_library_and_backup_and_finds_damage() {
        let dir = tempfile::tempdir().unwrap();
        let lib_dir = dir.path().join("lib");
        let lib = lib_dir.to_string_lossy().to_string();
        let backup = dir.path().join("backup");
        let to = backup.to_string_lossy().to_string();
        let dell = dir.path().join("dell");
        fs::create_dir_all(dell.join("Family")).unwrap();
        for (name, shade) in [("a.jpg", 10), ("b.jpg", 120), ("c.jpg", 240)] {
            fs::write(dell.join("Family").join(name), real_jpeg(shade)).unwrap();
        }
        keevoro(&["scan", "--library", &lib, &dell.to_string_lossy()]).unwrap();
        assert!(keevoro(&["check", "--library", &lib])
            .unwrap()
            .starts_with("There is nothing in the library to check yet."));
        keevoro(&["gather", "--library", &lib]).unwrap();
        keevoro(&["connect", "--library", &lib, "--to-folder", &to]).unwrap();
        keevoro(&["publish", "--library", &lib]).unwrap();

        assert_eq!(
            keevoro(&["check", "--library", &lib]).unwrap(),
            "Your library: all 3 files are intact.\n"
        );
        let said = keevoro(&["check", "--library", &lib, "--backup", "--read-back"]).unwrap();
        assert_eq!(
            said,
            "Your library: all 3 files are intact.\n\
             Your backup: all 3 originals are there, and each one was read back and matches exactly.\n"
        );
        assert!(
            keevoro(&["check", "--library", &lib, "--read-back"]).is_err(),
            "--read-back needs --backup"
        );

        // A file rots in the library, and one goes missing from the backup.
        let mut rotted = fs::read(lib_dir.join("originals/Family/a.jpg")).unwrap();
        let middle = rotted.len() / 2;
        rotted[middle] ^= 0xFF;
        fs::write(lib_dir.join("originals/Family/a.jpg"), rotted).unwrap();
        fs::remove_file(backup.join("originals/Family/b.jpg")).unwrap();

        let mut out = Vec::new();
        let secrets = connect::testing::MemorySecrets::default();
        let err = keevoro_with(&["check", "--library", &lib, "--backup"], &secrets, &mut out).unwrap_err();
        assert_eq!(err.to_string(), "2 files need attention.");
        let said = String::from_utf8(out).unwrap();
        assert!(said.contains("Your library: 2 intact, 1 with a problem:"), "{said}");
        assert!(
            said.contains("a.jpg: Its contents have changed, so it is damaged or was edited."),
            "{said}"
        );
        assert!(said.contains("Your backup: 2 fine, 1 with a problem:"), "{said}");
        assert!(
            said.contains("originals/Family/b.jpg: It is missing from the backup."),
            "{said}"
        );

        // The backup still has a good copy of the rotted file: the check
        // that reads it back says so.
        let mut out = Vec::new();
        let _ = keevoro_with(
            &["check", "--library", &lib, "--backup", "--read-back"],
            &secrets,
            &mut out,
        );
        let said = String::from_utf8(out).unwrap();
        assert!(said.contains("Your backup: 2 fine, 1 with a problem:"), "{said}");
        assert!(
            !said.contains("a.jpg: The backup's copy"),
            "the backup's a.jpg is still good: {said}"
        );
    }

    #[test]
    fn checking_a_backup_needs_to_know_which_one() {
        let dir = tempfile::tempdir().unwrap();
        let lib = dir.path().join("lib").to_string_lossy().to_string();
        let dell = dir.path().join("dell");
        fs::create_dir_all(&dell).unwrap();
        fs::write(dell.join("a.jpg"), real_jpeg(1)).unwrap();
        keevoro(&["scan", "--library", &lib, &dell.to_string_lossy()]).unwrap();
        keevoro(&["gather", "--library", &lib]).unwrap();
        let err = keevoro(&["check", "--library", &lib, "--backup"]).unwrap_err();
        assert!(err.to_string().starts_with("Say which backup to check"), "{err}");
        // An empty folder named on the command line: everything is missing.
        let empty = dir.path().join("empty").to_string_lossy().to_string();
        let err = keevoro(&["check", "--library", &lib, "--backup", "--to-folder", &empty]).unwrap_err();
        assert_eq!(err.to_string(), "1 file needs attention.");
    }

    #[test]
    fn an_empty_library_says_so() {
        let dir = tempfile::tempdir().unwrap();
        drop(Catalog::open_in_library(dir.path()).unwrap());
        let library = dir.path().to_string_lossy().to_string();
        for command in ["collections", "sources"] {
            let text = keevoro(&[command, "--library", &library]).unwrap();
            assert_eq!(text, "Nothing has been scanned into this library yet.\n");
        }
    }
}
