//! Domain types and rules for keevoro.
//!
//! This crate has no I/O. Everything here is pure functions over plain data,
//! which keeps the rules that decide what a user's memories are called, dated
//! and skipped easy to test exhaustively.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod collections;
pub mod date;
pub mod folders;
pub mod junk;
pub mod libraries;
pub mod media;
pub mod skip;

pub use collections::{date_range_label, group_into_collections, Collection, CollectionInput};
pub use date::{best_date, date_from_filename, date_from_folder, DateGuess, DateSource, FolderDate, PartialDate};
pub use folders::{folder_hints, FolderHints};
pub use junk::{junk_hint, JunkInput, JunkReason};
pub use libraries::{holds_originals, library_album_segments, photo_library, PhotoLibrary};
pub use media::{kind_from_extension, kind_from_mime, MediaKind};
pub use skip::{SkipReason, SkipRules};
