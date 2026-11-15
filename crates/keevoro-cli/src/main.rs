//! `keevoro`: the engine without a UI.
//!
//! Everything the desktop app will do is reachable from here first, so the
//! engine can be proven on real drives before any screens exist.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

mod publish;
mod summary;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use keevoro_catalog::Catalog;
use keevoro_collect::{free_space, gather, make_previews, GatherError};
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
    /// Back the library up to storage and write its index. Safe to stop
    /// and run again; originals already stored are never replaced.
    Publish {
        #[arg(long)]
        library: PathBuf,
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
        /// The keys are read from KEEVORO_ACCESS_KEY_ID and
        /// KEEVORO_SECRET_ACCESS_KEY, never from the command line.
        #[arg(long)]
        bucket: Option<String>,
        /// The web address the storage is served from, if it is.
        /// Used for links inside the index.
        #[arg(long)]
        base_url: Option<String>,
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
        Command::Publish {
            library,
            to_folder,
            to_r2,
            to_s3,
            to_endpoint,
            bucket,
            base_url,
        } => {
            let preset = if let Some(account_id) = to_r2 {
                Some(keevoro_storage_s3::Preset::CloudflareR2 { account_id })
            } else if let Some(region) = to_s3 {
                Some(keevoro_storage_s3::Preset::AmazonS3 { region })
            } else {
                to_endpoint.map(|endpoint| keevoro_storage_s3::Preset::Custom {
                    endpoint,
                    region: None,
                    path_style: true,
                })
            };
            let destination = match (to_folder, preset) {
                (Some(folder), _) => publish::Destination::Folder(folder),
                (None, Some(preset)) => publish::Destination::Bucket {
                    preset,
                    bucket: bucket.unwrap_or_default(),
                },
                (None, None) => bail!("Say where to back up to: --to-folder, --to-r2, --to-s3 or --to-endpoint."),
            };
            publish::run(
                out,
                &open_existing(&library)?,
                &library,
                destination,
                base_url.as_deref(),
            )
        }
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
pub(crate) fn wanted(catalog: &Catalog, item: &FoundItem) -> Result<bool> {
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

    // Previews come from the library's own copies, so they are made for
    // everything gathered so far, including on earlier runs.
    let previews = make_previews(library, &catalog, &chosen, &mut |done, total| {
        if live {
            eprint!("\r\x1b[2K  Making previews: {} of {}", n(done), n(total));
            let _ = std::io::stderr().flush();
        }
    })?;
    if live {
        eprint!("\r\x1b[2K");
    }
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
            .filter(|f| !f.starts_with("index/"))
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
            after
                .lines()
                .last()
                .unwrap()
                .starts_with("Your private index: file:///"),
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
