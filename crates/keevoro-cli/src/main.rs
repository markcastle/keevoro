//! `keevoro`: the engine without a UI.

mod summary;

use anyhow::Result;
use clap::{Parser, Subcommand};
use keevoro_scan::{scan, ScanOptions, ScanReport};
use std::io::{IsTerminal, Write};
use std::path::PathBuf;

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
    },
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Scan {
            paths,
            json,
            no_hash,
            show,
        } => run_scan(&paths, json, no_hash, show),
    }
}

fn run_scan(paths: &[PathBuf], json: bool, no_hash: bool, show: usize) -> Result<()> {
    let opts = ScanOptions {
        hash: !no_hash,
        ..Default::default()
    };
    let live = std::io::stderr().is_terminal();
    let mut reports: Vec<ScanReport> = Vec::new();
    for path in paths {
        if live {
            eprintln!("Looking through {} ...", path.display());
        }
        let report = scan(path, &opts, &mut |p| {
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

    let (summary, unique) = summary::summarise(&reports);
    let mut out = std::io::stdout().lock();
    if json {
        #[derive(serde::Serialize)]
        struct JsonOut<'a> {
            summary: &'a summary::Summary,
            items: &'a [&'a keevoro_scan::FoundItem],
            sources: &'a [ScanReport],
        }
        serde_json::to_writer_pretty(
            &mut out,
            &JsonOut {
                summary: &summary,
                items: &unique,
                sources: &reports,
            },
        )?;
        writeln!(out)?;
    } else {
        write!(out, "{}", summary::render(&summary, show))?;
    }
    Ok(())
}
