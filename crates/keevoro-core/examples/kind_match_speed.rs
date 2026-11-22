//! How long the question a search asks of every file takes: "is this
//! ending looked for, and if not, is it passed over unopened?". It is
//! asked of 2,000,000 made-up endings, first as Keevoro 0.51 asked it
//! (57 kinds, a lower-case copy made for any ending with capitals), then
//! as Keevoro asks it now (137 kinds, lowered without a copy, with a hash
//! made for short endings, in one look).
//!
//! `cargo run -p keevoro-core --example kind_match_speed`
//!
//! It prints both times. It is not a test: how fast a computer is varies
//! too much for a time to pass or fail on.

#![allow(clippy::print_stdout, clippy::unwrap_used)]

use keevoro_core::{Kinds, MediaKind};
use std::collections::HashMap;
use std::hint::black_box;
use std::time::Instant;

const NAMES: usize = 2_000_000;

/// Endings as a drive might hold them: photos and videos in either case,
/// programs, documents, code, and the odd kind.
const ENDINGS: &[&str] = &[
    "jpg", "JPG", "jpeg", "png", "PNG", "heic", "HEIC", "mov", "MOV", "mp4", "MP4", "CR2", "nef", "ARW", "dng", "txt",
    "dll", "exe", "js", "html", "json", "css", "mp3", "pdf", "docx", "xml", "dat", "tmp", "bak", "ini", "tga", "psd",
    "mxf", "exr", "xyz", "MTS", "avi", "gif", "log", "sys",
];

/// The list Keevoro 0.51 came with, and how it was asked.
mod as_it_was {
    use super::*;

    const BUILT_IN: &[(&str, MediaKind)] = &[
        ("jpg", MediaKind::Photo),
        ("jpeg", MediaKind::Photo),
        ("jpe", MediaKind::Photo),
        ("png", MediaKind::Photo),
        ("heic", MediaKind::Photo),
        ("heif", MediaKind::Photo),
        ("tif", MediaKind::Photo),
        ("tiff", MediaKind::Photo),
        ("webp", MediaKind::Photo),
        ("bmp", MediaKind::Photo),
        ("avif", MediaKind::Photo),
        ("jxl", MediaKind::Photo),
        ("dng", MediaKind::Photo),
        ("cr2", MediaKind::Photo),
        ("cr3", MediaKind::Photo),
        ("crw", MediaKind::Photo),
        ("nef", MediaKind::Photo),
        ("nrw", MediaKind::Photo),
        ("arw", MediaKind::Photo),
        ("srf", MediaKind::Photo),
        ("sr2", MediaKind::Photo),
        ("orf", MediaKind::Photo),
        ("rw2", MediaKind::Photo),
        ("raf", MediaKind::Photo),
        ("pef", MediaKind::Photo),
        ("srw", MediaKind::Photo),
        ("x3f", MediaKind::Photo),
        ("3fr", MediaKind::Photo),
        ("erf", MediaKind::Photo),
        ("kdc", MediaKind::Photo),
        ("mrw", MediaKind::Photo),
        ("raw", MediaKind::Photo),
        ("iiq", MediaKind::Photo),
        ("dcr", MediaKind::Photo),
        ("mef", MediaKind::Photo),
        ("mos", MediaKind::Photo),
        ("rwl", MediaKind::Photo),
        ("mp4", MediaKind::Video),
        ("mov", MediaKind::Video),
        ("m4v", MediaKind::Video),
        ("avi", MediaKind::Video),
        ("mkv", MediaKind::Video),
        ("wmv", MediaKind::Video),
        ("mpg", MediaKind::Video),
        ("mpeg", MediaKind::Video),
        ("webm", MediaKind::Video),
        ("insv", MediaKind::Video),
        ("asf", MediaKind::Video),
        ("flv", MediaKind::Video),
        ("mts", MediaKind::Video),
        ("m2ts", MediaKind::Video),
        ("3gp", MediaKind::Video),
        ("3g2", MediaKind::Video),
        ("mod", MediaKind::Video),
        ("tod", MediaKind::Video),
        ("vob", MediaKind::Video),
        ("dv", MediaKind::Video),
    ];

    pub struct Kinds(HashMap<String, (MediaKind, bool)>);

    impl Kinds {
        pub fn new() -> Self {
            Self(BUILT_IN.iter().map(|(e, k)| ((*e).to_string(), (*k, true))).collect())
        }

        fn get(&self, ending: &str) -> Option<&(MediaKind, bool)> {
            let ending = ending.strip_prefix('.').unwrap_or(ending);
            match self.0.get(ending) {
                Some(found) => Some(found),
                None if ending.chars().any(char::is_uppercase) => self.0.get(&ending.to_lowercase()),
                None => None,
            }
        }

        pub fn passed_over(&self, ending: &str) -> bool {
            match self.get(ending) {
                Some((_, on)) => !on,
                None => never_media_extension(ending),
            }
        }
    }

    /// As 0.51 had it: a lower-case copy of every ending asked about.
    pub fn never_media_extension(ext: &str) -> bool {
        let ext = ext.trim_start_matches('.').to_ascii_lowercase();
        matches!(
            ext.as_str(),
            "exe"
                | "dll"
                | "sys"
                | "msi"
                | "cab"
                | "ocx"
                | "drv"
                | "bat"
                | "cmd"
                | "ps1"
                | "sh"
                | "so"
                | "dylib"
                | "lib"
                | "obj"
                | "o"
                | "a"
                | "class"
                | "jar"
                | "pyc"
                | "pdb"
                | "mui"
                | "manifest"
                | "cat"
                | "inf"
                | "nls"
                | "txt"
                | "log"
                | "md"
                | "rtf"
                | "doc"
                | "docx"
                | "xls"
                | "xlsx"
                | "ppt"
                | "pptx"
                | "pdf"
                | "odt"
                | "ods"
                | "csv"
                | "epub"
                | "chm"
                | "hlp"
                | "html"
                | "htm"
                | "css"
                | "js"
                | "mjs"
                | "ts"
                | "tsx"
                | "jsx"
                | "json"
                | "xml"
                | "yml"
                | "yaml"
                | "ini"
                | "cfg"
                | "conf"
                | "toml"
                | "c"
                | "h"
                | "cpp"
                | "hpp"
                | "cs"
                | "java"
                | "py"
                | "rb"
                | "go"
                | "rs"
                | "php"
                | "sql"
                | "map"
                | "lock"
                | "zip"
                | "rar"
                | "7z"
                | "gz"
                | "tar"
                | "bz2"
                | "xz"
                | "iso"
                | "mp3"
                | "wav"
                | "flac"
                | "ogg"
                | "m4a"
                | "wma"
                | "aac"
                | "mid"
                | "ttf"
                | "otf"
                | "woff"
                | "woff2"
                | "lnk"
                | "url"
        )
    }
}

/// The question as a search asks it now.
fn passed_over_now(kinds: &Kinds, ending: &str) -> bool {
    kinds.passed_over(ending)
}

fn main() {
    let endings: Vec<&str> = (0..NAMES).map(|i| ENDINGS[(i * 7 + i / 13) % ENDINGS.len()]).collect();

    let before = as_it_was::Kinds::new();
    let now = Kinds::default();
    // Both must answer the same for the endings both lists know.
    for ending in ENDINGS {
        if !["tga", "psd", "mxf", "exr", "gif"].contains(&ending.to_lowercase().as_str()) {
            assert_eq!(before.passed_over(ending), passed_over_now(&now, ending), "{ending}");
        }
    }

    for round in 1..=3 {
        let started = Instant::now();
        let mut over = 0usize;
        for ending in &endings {
            over += usize::from(black_box(before.passed_over(black_box(ending))));
        }
        let was = started.elapsed();

        let started = Instant::now();
        let mut over_now = 0usize;
        for ending in &endings {
            over_now += usize::from(black_box(passed_over_now(&now, black_box(ending))));
        }
        let is = started.elapsed();
        println!(
            "round {round}: {NAMES} endings. Before (57 kinds): {was:?}, {:.0} ns each, {over} passed over. Now (137 kinds): {is:?}, {:.0} ns each, {over_now} passed over.",
            was.as_nanos() as f64 / NAMES as f64,
            is.as_nanos() as f64 / NAMES as f64,
        );
    }
}
