//! A destination that goes away part-way through a backup, as the owner's
//! network share did for a stretch of a fifteen-hour run, and files that
//! fail on two runs in a row.
//!
//! The owner's first run said 176 lines of "Couldn't read it from its
//! source (The media is write protected...)". These tests hold what it says
//! now: the trouble is said as the destination's, in one sentence with a
//! count and the first few names; the place is not tried file by file once
//! it seems gone; what is left waits and is said; the same files failing
//! again are said as such; and the main account still adds up.

use crate::connect::{self, testing::MemorySecrets, Flags, Saved, World};
use crate::send::{everywhere, Stop};
use keevoro_catalog::Catalog;
use keevoro_core::estimate::{Remembered, Tracker};
use std::path::PathBuf;

struct Run {
    dir: tempfile::TempDir,
    home: PathBuf,
    catalog: Catalog,
    secrets: MemorySecrets,
}

impl Run {
    /// Twenty photos on a card, going to a folder called share.
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let card = dir.path().join("CARD");
        for n in 0..20u8 {
            let p = card.join("DCIM").join(format!("IMG_{:04}.JPG", 6129 + u32::from(n)));
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, crate::discover::tests::real_jpeg(5 + n * 12)).unwrap();
        }
        let home = dir.path().join("home");
        let mut catalog = Catalog::open_in_library(&home).unwrap();
        crate::scan_into(&mut catalog, &card, Some("CARD"), &mut |_| {}).unwrap();
        crate::send::set_wants_previews(&catalog, false).unwrap();
        let run = Self {
            dir,
            home,
            catalog,
            secrets: MemorySecrets::default(),
        };
        let to = Saved::from_flags(&Flags {
            to_folder: Some(run.share()),
            ..Flags::default()
        })
        .unwrap();
        connect::add(&mut Vec::new(), &run.catalog, &to, None, &run.world()).unwrap();
        run
    }

    fn share(&self) -> PathBuf {
        self.dir.path().join("share")
    }

    fn world(&self) -> World<'_> {
        World {
            secrets: &self.secrets,
            environment: &connect::testing::no_environment,
        }
    }

    fn back_up(&self, on_progress: &mut dyn FnMut(&str)) -> (String, crate::send::Went) {
        let mut said = Vec::new();
        let went = everywhere(
            &mut said,
            &self.catalog,
            &self.home,
            &self.world(),
            None,
            &Stop::default(),
            &Tracker::new(Remembered::default()),
            on_progress,
            &mut |_| {},
        )
        .unwrap();
        (String::from_utf8(said).unwrap(), went)
    }
}

#[test]
fn a_destination_that_goes_away_part_way_is_said_once_as_the_destinations_and_not_tried_file_by_file() {
    let run = Run::new();
    let away = run.dir.path().join("share, gone");
    let share = run.share();
    let mut gone = false;
    let (said, went) = run.back_up(&mut |line| {
        if !gone && line.starts_with("5 of 20 safely backed up") {
            std::fs::rename(&share, &away).unwrap();
            gone = true;
        }
    });
    assert!(gone);
    let place = format!("the folder {}", share.display());
    // Never said as the source's, and never a line a file.
    assert!(!said.contains("from its source"), "{said}");
    assert!(!said.contains("  originals/"), "{said}");
    assert!(
        said.contains(&format!(
            "Couldn't back up 8 files, because {place} went away part-way, as a drive that is unplugged does: \
             IMG_6134.JPG, IMG_6135.JPG, IMG_6136.JPG and 5 more."
        )),
        "{said}"
    );
    // After a run of failures the folder was asked whether it was there,
    // and no more were tried there one by one.
    assert!(
        said.contains(&format!(
            "The folder {} went away part-way through and had not come back by the end, so 7 files were not sent.",
            share.display()
        )),
        "{said}"
    );
    assert!(
        said.contains(&format!(
            "5 of the 20 files you are keeping are safely at {place}. 15 still need attention: 8 couldn't be sent \
             and 7 are waiting for it to come back."
        )),
        "{said}"
    );
    assert_eq!(went.failed_files, 15, "{said}");
    assert!(
        said.contains("The index was not written, as the folder had gone"),
        "{said}"
    );
    assert!(said.contains("\"Check they all arrived\""), "{said}");
    assert!(!share.exists(), "nothing was made where the folder was");

    // It comes back, and the next backup sends the rest, said plainly.
    std::fs::rename(&away, &share).unwrap();
    let (said, _) = run.back_up(&mut |_| {});
    assert!(said.contains("Safely backed up 15 files"), "{said}");
    assert!(
        said.contains(&format!("All 20 files you are keeping are safely at {place}.")),
        "{said}"
    );
}

#[test]
fn the_same_files_failing_on_a_second_run_are_said_as_failing_again() {
    let run = Run::new();
    let card = run.dir.path().join("CARD/DCIM");
    let unreadable: Vec<PathBuf> = ["IMG_6140.JPG", "IMG_6141.JPG"].iter().map(|n| card.join(n)).collect();
    let aside: Vec<PathBuf> = unreadable.iter().map(|p| p.with_extension("away")).collect();
    for (from, to) in unreadable.iter().zip(&aside) {
        std::fs::rename(from, to).unwrap();
    }
    let (first, _) = run.back_up(&mut |_| {});
    assert!(
        first.contains("Couldn't back up 2 files, because they were no longer where they were found:"),
        "{first}"
    );
    let (second, _) = run.back_up(&mut |_| {});
    assert!(
        second.contains(
            "These 2 files could not be backed up again, because they were no longer where they were found: \
             IMG_6140.JPG and IMG_6141.JPG."
        ),
        "{second}"
    );
    // Back where they were: sent, and nothing more is said of them.
    for (from, to) in aside.iter().zip(&unreadable) {
        std::fs::rename(from, to).unwrap();
    }
    let (third, _) = run.back_up(&mut |_| {});
    assert!(third.contains("Safely backed up 2 files"), "{third}");
    assert!(!third.contains("could not be backed up"), "{third}");
}
