//! Domain types and rules for keevoro.
//!
//! This crate has no I/O. Everything here is pure functions over plain data,
//! which keeps the rules that decide what a user's memories are called, dated
//! and skipped easy to test exhaustively.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod arrange;
pub mod background;
pub mod bucket;
pub mod collections;
pub mod date;
pub mod doing;
pub mod egress;
pub mod estimate;
pub mod folders;
pub mod junk;
pub mod kinds;
pub mod layout;
pub mod libraries;
pub mod media;
pub mod meter;
pub mod pause;
pub mod places;
pub mod recognise;
pub mod schedule;
pub mod skip;
pub mod synced;
pub mod unattended;
pub mod wants;
pub mod words;

pub use arrange::Arrange;
pub use collections::{date_range_label, group_into_collections, Collection, CollectionInput, NOT_IN_A_FOLDER};
pub use date::{best_date, date_from_filename, date_from_folder, DateGuess, DateSource, FolderDate, PartialDate};
pub use folders::{folder_hints, FolderHints};
pub use junk::{junk_hint, JunkInput, JunkReason};
pub use kinds::{plain_ending, FileKind, FileKinds, KindChange, KindRefusal, Kinds, OTHER_FILES_SAID};
pub use layout::{fit_place, library_folder, numbered_name, safe_name, OTHER_FILES};
pub use libraries::{holds_originals, library_album_segments, photo_library, PhotoLibrary};
pub use media::{is_raw_extension, kind_from_mime, never_media_extension, MediaKind};
pub use skip::{folder_made_by_keevoro, made_by_keevoro, SkipReason, SkipRules, BACKUP_GUIDE, RECORD};
pub use wants::{Choices, FileFacts, LeftOutBy, SourceChoices, Wants};
