//! `keevoro`: the engine without a UI.
//!
//! Everything the desktop app will do is reachable from here first, so the
//! engine can be proven on real drives before any screens exist.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

mod summary;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use keevoro_catalog::Catalog;
use keevoro_collect::{free_space, gather, GatherError};
use keevoro_scan::FoundItem;
use keevoro_scan::{scan, ScanOptions, ScanReport};
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Parser)]
#[command(
    name = "keevoro",
    version,
    about = "Find photos and videos on old drives and gather them into one library."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
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
        #[arg(required = true)]
        collections: Vec<String>,
    },
    /// Put collections back after `skip`.
    Keep {
        #[arg(long)]
        library: PathBuf,
        /// Collection names as shown by `collections`, like "Cornwall".
        #[arg(required = true)]
        collections: Vec<String>,
    },
    /// Copy everything you are keeping into the library and check every
    /// copy. Your old drives are only read. Safe to stop and run again.
    Gather {
        #[arg(long)]
        library: PathBuf,
    },
}

fn main() -> Result<()> {
    run(Cli::parse(), &mut std::io::stdout().lock())
}

fn run(cli: Cli, out: &mut dyn Write) -> Result<()> {
    match cli.command {
        Command::Scan {
            paths,
            json,
            no_hash,
            show,
            library,
            name,
        } => {
            if name.is_some() && paths.len() > 1 {
                bail!("Give one drive at a time when using --name.");
            }
            let reports = run_scan(&paths, no_hash, library.is_some());
            if let Some(library) = &library {
                remember(library, name.as_deref(), &reports)?;
            }
            print_reports(out, &reports, json, show)
        }
        Command::Collections { library, json, show } => {
            let catalog = open_existing(&library)?;
            let reports = catalog.load_reports()?;
            if reports.is_empty() {
                writeln!(out, "Nothing has been scanned into this library yet.")?;
                return Ok(());
            }
            print_reports(out, &reports, json, show)?;
            if !json {
                let skipped = skipped_collections(&catalog, &reports)?;
                if !skipped.is_empty() {
                    writeln!(out, "Not keeping: {}.", skipped.join(", "))?;
                }
            }
            Ok(())
        }
        Command::Skip { library, collections } => choose(out, &library, &collections, false),
        Command::Keep { library, collections } => choose(out, &library, &collections, true),
        Command::Gather { library } => run_gather(out, &library),
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
fn choose(out: &mut dyn Write, library: &Path, names: &[String], keep: bool) -> Result<()> {
    let catalog = open_existing(library)?;
    let reports = catalog.load_reports()?;
    let (summary, items) = summary::summarise(&reports);
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
            let verb = if keep { "Keeping" } else { "Not keeping" };
            writeln!(out, "{verb} {} ({}).", c.title, c.summary())?;
        }
    }
    Ok(())
}

/// Did the user say no to this file? Undecided files follow the default:
/// keep, unless it looks like junk.
fn wanted(catalog: &Catalog, item: &FoundItem) -> Result<bool> {
    let choice = match &item.blake3 {
        Some(hash) => catalog.choice(hash)?,
        None => None,
    };
    Ok(choice.unwrap_or(item.junk.is_none()))
}

/// Titles of collections the user has skipped entirely.
fn skipped_collections(catalog: &Catalog, reports: &[ScanReport]) -> Result<Vec<String>> {
    let (summary, items) = summary::summarise(reports);
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

fn run_gather(out: &mut dyn Write, library: &Path) -> Result<()> {
    let catalog = open_existing(library)?;
    let reports = catalog.load_reports()?;
    // Rule one: never write to a source. A library inside a scanned folder
    // would do exactly that.
    let home = std::fs::canonicalize(library).unwrap_or_else(|_| library.to_path_buf());
    if let Some(r) = reports.iter().find(|r| home.starts_with(&r.root)) {
        bail!(
            "The library is inside {}, which is one of the places being gathered from. \
             Keevoro never writes to those. Choose a library folder somewhere else.",
            r.root.display()
        );
    }

    let (unique, _) = summary::unique_items(&reports);
    let mut chosen = Vec::new();
    for item in unique {
        if wanted(&catalog, item)? {
            chosen.push(item);
        }
    }
    if chosen.is_empty() {
        writeln!(out, "There is nothing to gather yet.")?;
        return Ok(());
    }

    let live = std::io::stderr().is_terminal();
    let n = summary::thousands;
    let result = gather(library, &catalog, &chosen, free_space(library), &mut |p| {
        if live {
            eprint!("\r\x1b[2K  {} of {} safely copied", n(p.done), n(p.total));
            let _ = std::io::stderr().flush();
        }
    });
    if live {
        eprint!("\r\x1b[2K");
    }
    let report = match result {
        Ok(r) => r,
        Err(GatherError::NotEnoughRoom { needed, free }) => bail!(
            "There isn't enough room for your library. It needs {} and the disk has {} free.",
            summary::bytes(needed),
            summary::bytes(free)
        ),
        Err(e) => return Err(e.into()),
    };

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

fn print_reports(out: &mut dyn Write, reports: &[ScanReport], json: bool, show: usize) -> Result<()> {
    let (summary, unique) = summary::summarise(reports);
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
        let cli = Cli::try_parse_from(std::iter::once("keevoro").chain(args.iter().copied()))?;
        run(cli, &mut out)?;
        Ok(String::from_utf8(out).unwrap())
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
        assert_eq!(err.to_string(), "Give one drive at a time when using --name.");
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
        assert_eq!(said, "3 files already there.\n");
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
        assert!(said.contains("Couldn't read it from the old drive"), "{said}");
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
