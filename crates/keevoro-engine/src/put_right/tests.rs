use super::*;
use crate::connect::{self, testing::MemorySecrets, Flags};
use crate::send::everywhere;
use keevoro_core::estimate::{Remembered, Tracker};
use std::path::PathBuf;

/// A card of twelve photos backed up to a folder, as a backup leaves it.
struct Owner {
    dir: tempfile::TempDir,
    home: PathBuf,
    catalog: Catalog,
    secrets: MemorySecrets,
}

const TODAY: &str = "2026-11-22";

impl Owner {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let card = dir.path().join("CARD");
        for n in 0..12u8 {
            let p = card.join("DCIM").join(format!("IMG_04{:02}.JPG", n + 12));
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, crate::discover::tests::real_jpeg(10 + n * 19)).unwrap();
        }
        let home = dir.path().join("home");
        let mut catalog = Catalog::open_in_library(&home).unwrap();
        crate::scan_into(&mut catalog, &card, Some("CARD"), &mut |_| {}).unwrap();
        let owner = Self {
            dir,
            home,
            catalog,
            secrets: MemorySecrets::default(),
        };
        let share = owner.share();
        let to = Saved::from_flags(&Flags {
            to_folder: Some(share),
            ..Flags::default()
        })
        .unwrap();
        connect::add(&mut Vec::new(), &owner.catalog, &to, None, &owner.world()).unwrap();
        owner.back_up();
        owner
    }

    fn share(&self) -> PathBuf {
        self.dir.path().join("Photos")
    }

    fn world(&self) -> World<'_> {
        World {
            secrets: &self.secrets,
            environment: &connect::testing::no_environment,
        }
    }

    fn back_up(&self) -> String {
        let mut said = Vec::new();
        everywhere(
            &mut said,
            &self.catalog,
            &self.home,
            &self.world(),
            None,
            &Stop::default(),
            &Tracker::new(Remembered::default()),
            &mut |_| {},
            &mut |_| {},
        )
        .unwrap();
        String::from_utf8(said).unwrap()
    }

    fn saved(&self) -> Saved {
        connect::destinations(&self.catalog).unwrap().remove(0)
    }

    fn target(&self) -> String {
        self.saved().target.unwrap()
    }

    fn keys(&self) -> Vec<String> {
        let mut keys: Vec<String> = self
            .catalog
            .sent_to(&self.target())
            .unwrap()
            .into_iter()
            .map(|r| r.key)
            .filter(|k| k.starts_with("originals/"))
            .collect();
        keys.sort();
        keys
    }

    fn at_share(&self, key: &str) -> PathBuf {
        key.split('/').fold(self.share(), |p, s| p.join(s))
    }

    /// Every file on the card, with its contents and modified time.
    fn card(&self) -> Vec<(PathBuf, Vec<u8>, std::time::SystemTime)> {
        let mut all = Vec::new();
        for entry in std::fs::read_dir(self.dir.path().join("CARD/DCIM")).unwrap() {
            let path = entry.unwrap().path();
            let meta = std::fs::metadata(&path).unwrap();
            all.push((path.clone(), std::fs::read(&path).unwrap(), meta.modified().unwrap()));
        }
        all.sort();
        all
    }

    fn put_right(&self, thorough: bool, stop: &Stop, progress: &mut dyn FnMut(&str)) -> (Outcome, String) {
        let mut said = Vec::new();
        let outcome = put_right(
            &mut said,
            &self.catalog,
            &self.home,
            &self.saved(),
            &self.world(),
            HowToPutRight { thorough },
            TODAY,
            &|| 1_795_000_000,
            stop,
            progress,
        )
        .unwrap();
        assert!(outcome.adds_up(), "{outcome:?}");
        (outcome, String::from_utf8(said).unwrap())
    }

    /// The owner's backup as 0.42.0 left it, in miniature: of the files it
    /// recorded as safe, some are not there, some are another size, one is
    /// the right size with other contents (which no size check can see),
    /// and one the look wrongly said was another size. The records say what
    /// 0.42.0's look wrote down.
    fn as_0_42_left_it(&self) -> Broken {
        let keys = self.keys();
        let broken = Broken {
            missing: keys[0..2].to_vec(),
            short: keys[2..5].to_vec(),
            other_contents: keys[5].clone(),
            fine_but_said_short: keys[6].clone(),
        };
        for key in &broken.missing {
            std::fs::remove_file(self.at_share(key)).unwrap();
        }
        for key in &broken.short {
            let path = self.at_share(key);
            let whole = std::fs::read(&path).unwrap();
            std::fs::write(&path, &whole[..whole.len() / 2]).unwrap();
        }
        let path = self.at_share(&broken.other_contents);
        let mut bytes = std::fs::read(&path).unwrap();
        let middle = bytes.len() / 2;
        bytes[middle] ^= 0xff;
        std::fs::write(&path, bytes).unwrap();
        let mut seen: Vec<(String, Seen)> = broken.missing.iter().map(|k| (k.clone(), Seen::Missing)).collect();
        seen.extend(broken.short.iter().map(|k| (k.clone(), Seen::NotAsSent)));
        seen.push((broken.fine_but_said_short.clone(), Seen::NotAsSent));
        self.catalog.set_seen(&self.target(), &seen).unwrap();
        broken
    }
}

struct Broken {
    missing: Vec<String>,
    short: Vec<String>,
    other_contents: String,
    fine_but_said_short: String,
}

/// Is every original at the share exactly its file on the card?
fn every_copy_matches(owner: &Owner) -> bool {
    let card: HashSet<Vec<u8>> = owner.card().into_iter().map(|(_, bytes, _)| bytes).collect();
    owner.keys().iter().all(|key| {
        std::fs::read(owner.at_share(key))
            .map(|bytes| card.contains(&bytes))
            .unwrap_or(false)
    })
}

#[test]
fn one_press_puts_the_owners_backup_right_moving_the_damaged_copies_aside_and_proving_everything() {
    let owner = Owner::new();
    let card_before = owner.card();
    let broken = owner.as_0_42_left_it();
    assert!(!every_copy_matches(&owner));

    let (outcome, said) = owner.put_right(true, &Stop::default(), &mut |_| {});

    // Everything is there and proved.
    assert!(outcome.all_proved, "{outcome:?}\n{said}");
    assert_eq!((outcome.kept, outcome.proved, outcome.attention.len()), (12, 12, 0));
    assert!(every_copy_matches(&owner));
    // The damaged copies, the short ones and the one with other contents,
    // were moved aside, keeping their paths, and nothing else was.
    let mut moved: Vec<String> = outcome.set_aside.iter().map(|(k, _)| k.clone()).collect();
    moved.sort();
    let mut damaged = broken.short.clone();
    damaged.push(broken.other_contents.clone());
    damaged.sort();
    assert_eq!(moved, damaged);
    for (key, went) in &outcome.set_aside {
        assert_eq!(went, &format!("Set aside by Keevoro/{TODAY}/{key}"));
        assert!(owner.share().join(went).is_file(), "moved, never deleted: {went}");
    }
    assert!(owner
        .share()
        .join(keevoro_core::SET_ASIDE_FOLDER)
        .join(keevoro_core::SET_ASIDE_NOTE)
        .is_file());
    // What was missing or moved aside was sent again.
    assert_eq!(outcome.sent, broken.missing.len() + damaged.len());
    assert!(
        !moved.contains(&broken.fine_but_said_short),
        "a good copy is never moved"
    );
    assert!(
        said.contains(&format!(
            "Your backup at the folder {} is right: all 12 files you are keeping are there, and every one was read \
             back in full and matches its original.",
            owner.share().display()
        )),
        "{said}"
    );
    assert!(said.contains("Moved 4 damaged copies aside"), "{said}");
    assert!(said.contains("Sent 6 files again"), "{said}");

    // Not one byte at the source changed.
    assert_eq!(owner.card(), card_before);

    // The backup agrees: nothing waits, a look finds nothing to say, and a
    // backup counts every file there.
    assert_eq!(
        crate::discover(&owner.home, &owner.catalog).unwrap().waiting_to_gather,
        0
    );
    let again = owner.back_up();
    assert!(again.contains("12 files already there."), "{again}");
    assert!(
        !again.contains("no longer holds") && !again.contains("not the size"),
        "{again}"
    );
    // The damaged copies are never counted, never in the index, and never
    // a source.
    let index = owner
        .catalog
        .setting(&format!("index_key:{}", owner.target()))
        .unwrap()
        .unwrap();
    let index = std::fs::read_to_string(owner.share().join(index)).unwrap();
    assert!(!index.contains("Set aside"), "{index}");
    assert!(owner
        .catalog
        .sent_to(&owner.target())
        .unwrap()
        .iter()
        .all(|r| !r.key.contains("Set aside")));
    assert_eq!(
        keevoro_core::folder_made_by_keevoro(&owner.share().join(keevoro_core::SET_ASIDE_FOLDER)),
        Some(keevoro_core::SkipReason::KeevoroOwn)
    );

    // Pressed again, it reads everything back again and has nothing to do.
    let (outcome, _) = owner.put_right(true, &Stop::default(), &mut |_| {});
    assert!(outcome.all_proved && outcome.set_aside.is_empty() && outcome.sent == 0);
}

#[test]
fn stopped_part_way_it_carries_on_without_reading_again_what_it_proved() {
    let owner = Owner::new();
    owner.as_0_42_left_it();
    let stop = Stop::default();
    let mut lines = 0;
    let (first, said) = owner.put_right(true, &stop, &mut |line| {
        if line.starts_with("Reading back") {
            lines += 1;
            if lines == 8 {
                stop.store(true, Ordering::SeqCst);
            }
        }
    });
    assert!(first.stopped && !first.all_proved);
    assert!(said.contains("carries on from here"), "{said}");
    // The plan says so, and how many are left.
    let plan = plan(&owner.catalog, &owner.saved()).unwrap();
    assert!(plan.proved_already > 0, "{plan:?}");
    assert!(
        plan.said.iter().any(|l| l.contains("carries on from where it stopped")),
        "{plan:?}"
    );

    let mut read_again = 0;
    let (second, _) = owner.put_right(true, &Stop::default(), &mut |line| {
        if line.starts_with("Reading back") {
            read_again += 1;
        }
    });
    assert!(second.all_proved, "{second:?}");
    assert!(read_again < 12, "only what was not proved: {read_again}");
    assert!(every_copy_matches(&owner));
}

#[test]
fn a_share_that_drops_out_mid_repair_changes_nothing_it_has_not_seen_and_carries_on_once_back() {
    let owner = Owner::new();
    let broken = owner.as_0_42_left_it();
    let away = owner.dir.path().join("Photos, unplugged");
    let mut lines = 0;
    let (first, said) = owner.put_right(true, &Stop::default(), &mut |line| {
        if line.starts_with("Reading back") {
            lines += 1;
            if lines == 7 {
                std::fs::rename(owner.share(), &away).unwrap();
            }
        }
    });
    assert!(first.lost_connection, "{first:?}");
    assert!(said.contains("The connection to the folder"), "{said}");
    assert!(!owner.share().exists(), "nothing was made where the share was");
    // Back again: it carries on and finishes.
    std::fs::rename(&away, owner.share()).unwrap();
    let (second, _) = owner.put_right(true, &Stop::default(), &mut |_| {});
    assert!(second.all_proved, "{second:?}");
    assert!(every_copy_matches(&owner));
    assert!(owner.at_share(&broken.fine_but_said_short).is_file());
}

#[test]
fn the_quick_pass_puts_right_what_is_known_in_minutes_and_says_what_it_did_not_read() {
    let owner = Owner::new();
    let broken = owner.as_0_42_left_it();
    let (outcome, said) = owner.put_right(false, &Stop::default(), &mut |_| {});
    assert!(!outcome.all_proved);
    assert_eq!(outcome.attention.len(), 0, "{outcome:?}");
    // The known ones are right now; the one with other contents at the
    // right size is not something a size can show, so only the full proof
    // finds it, and the words say what was not read back.
    assert_eq!(outcome.set_aside.len(), broken.short.len());
    assert!(said.contains("To read every one of those back too"), "{said}");
    let (full, _) = owner.put_right(true, &Stop::default(), &mut |_| {});
    assert!(full.all_proved);
    assert_eq!(full.set_aside.len(), 1, "the one only reading finds");
    assert!(every_copy_matches(&owner));
}

#[test]
fn the_plan_says_what_it_will_do_and_about_how_long_before_anything_is_done() {
    let owner = Owner::new();
    owner.as_0_42_left_it();
    let before: Vec<_> = owner.catalog.sent_to(&owner.target()).unwrap();
    let plan = plan(&owner.catalog, &owner.saved()).unwrap();
    assert_eq!((plan.known, plan.files, plan.proved_already), (6, 12, 0));
    assert!(plan.sets_aside);
    assert!(
        plan.said[0].starts_with("First, Keevoro puts right the 6 files it already knows are wrong"),
        "{:?}",
        plan.said
    );
    assert!(
        plan.said[1].contains("reads back in full each of the 12 files"),
        "{:?}",
        plan.said
    );
    assert!(plan.said[2].contains("Set aside by Keevoro"), "{:?}", plan.said);
    assert_eq!(
        owner.catalog.sent_to(&owner.target()).unwrap(),
        before,
        "a plan changes nothing"
    );
}

#[test]
fn how_long_is_said_roundly_and_from_what_was_measured() {
    assert_eq!(about(20), "a minute or so");
    assert_eq!(about(7 * 60), "about 10 minutes");
    assert_eq!(about(59 * 60), "about 60 minutes");
    assert_eq!(about(90 * 60), "about 2 hours");
    // A long backup: 312.4 GB in 10 hours 15 minutes.
    let measured = Some((335_436_211_200, 36_900));
    let rate = likely_rate(measured, true);
    assert_eq!(about((335_436_211_200_f64 / rate) as u64), "about 10 hours");
    // Too little measured to go by: a careful guess.
    assert_eq!(likely_rate(Some((1_000, 10)), true), likely_rate(None, true));
    assert!(likely_rate(None, false) > likely_rate(None, true));
    assert_eq!(
        reading_words(10, 100, 1_000_000, 10_000_000, 60),
        "Reading back and checking 10 of 100 (976.6 KB of 9.5 MB). About 10 minutes left"
    );
    assert_eq!(
        reading_words(1, 100, 1, 10, 5),
        "Reading back and checking 1 of 100 (1 bytes of 10 bytes)"
    );
}

#[test]
fn every_outcome_adds_up_and_its_words_never_contradict_it() {
    let place = "the folder Z:\\Photos";
    for (proved, right, attention, thorough) in [(12, 0, 0, true), (3, 9, 0, false), (10, 0, 2, true), (0, 0, 1, true)]
    {
        let outcome = Outcome {
            kept: proved + right + attention,
            proved,
            right_size: right,
            attention: (0..attention)
                .map(|i| (format!("originals/{i}.jpg"), "Couldn't be checked".into()))
                .collect(),
            all_proved: thorough && attention == 0 && right == 0,
            ..Outcome::default()
        };
        assert!(outcome.adds_up());
        let said = outcome_words(place, &outcome, thorough, TODAY).join(" ");
        if outcome.all_proved {
            assert!(said.contains("is right"), "{said}");
        } else {
            assert!(!said.contains("is right"), "{said}");
        }
        if attention > 0 {
            assert!(said.contains(&format!("{attention} still need")), "{said}");
        }
    }
}
