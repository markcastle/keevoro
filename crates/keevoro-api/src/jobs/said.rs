//! What a running job says, in the app's own words.
//!
//! These are the shapes the app's screens have always received, kept here
//! so the command line and an agent hear exactly what the app hears. The
//! app sends each as a window event named by [`Said::name`], and
//! `ui/src/backend.ts` has the same shapes as TypeScript types. Change both
//! together; the app's tests compare them.

use serde::{Deserialize, Deserializer, Serialize};

/// How far a survey of a drive has got. Counts are by file name alone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SurveyEvent {
    pub folders: usize,
    pub files: usize,
    pub photos: usize,
    pub videos: usize,
    /// Other files a person put on the list, seen so far. Never said when
    /// there are none.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub others: usize,
    /// Every suggestion is final; the folders Keevoro never looks through
    /// are being sized for the map of where the space goes.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub sizing: bool,
    /// The top of the map of where the space goes, as it grows: at most
    /// twice a second and 48 boxes, the biggest first. Absent in between.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub top: Vec<TopView>,
}

/// One box at the top of the map while a survey walks: a folder in the
/// place surveyed with everything inside it, the files directly in the
/// place, or the smaller folders together. The same names as a box of
/// `places.map`, so a screen draws both alike.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TopView {
    pub name: String,
    /// `folder`, `files` (directly in the place) or `smaller`.
    pub node: String,
    pub bytes: u64,
    pub photos: u64,
    pub videos: u64,
    pub others: u64,
    pub photo_bytes: u64,
    pub video_bytes: u64,
    pub other_bytes: u64,
    /// Other files a person put on the list, to be carried along.
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub carried: u64,
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub carried_bytes: u64,
    /// For the smaller folders together, how many.
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub folders: u64,
}

fn is_zero_u64(n: &u64) -> bool {
    *n == 0
}

impl From<&keevoro_scan::map::TopBox> for TopView {
    fn from(b: &keevoro_scan::map::TopBox) -> Self {
        use keevoro_scan::map::{Kind, TopItem};
        let (node, folders) = match b.item {
            TopItem::Folder => ("folder", 0),
            TopItem::FilesHere => ("files", 0),
            TopItem::Smaller(n) => ("smaller", u64::from(n)),
        };
        let k = &b.kinds;
        TopView {
            name: b.name.to_string(),
            node: node.to_string(),
            bytes: b.bytes(),
            photos: k[Kind::Photo.index()].files,
            videos: k[Kind::Video.index()].files,
            others: k[Kind::Other.index()].files,
            photo_bytes: k[Kind::Photo.index()].bytes,
            video_bytes: k[Kind::Video.index()].bytes,
            other_bytes: k[Kind::Other.index()].bytes,
            carried: k[Kind::Carried.index()].files,
            carried_bytes: k[Kind::Carried.index()].bytes,
            folders,
        }
    }
}

/// Progress reading, in the background, the files a quick search only
/// sampled, and a last one when the pass ends.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckEvent {
    pub done: usize,
    pub total: usize,
    pub finished: bool,
    /// On the last event: true when a person stopped it, so it does not
    /// start again by itself until the next search or backup.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub stopped: bool,
    /// On the last event: how many files are still to be checked.
    #[serde(default, skip_serializing_if = "is_none")]
    pub left: usize,
    /// What is said: while it runs, what it is doing, in a line; at the
    /// end of one a person stopped, what that means, a sentence each.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub said: Vec<String>,
}

fn is_none(n: &usize) -> bool {
    *n == 0
}

/// Progress looking through a drive, and a last one when it is done.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanEvent {
    /// The drive's friendly name.
    pub name: String,
    pub photos: u64,
    pub videos: u64,
    /// Other files a person put on the list. Never said when there are none.
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub others: u64,
    /// Every file looked at so far, photos or not.
    pub files: u64,
    /// The folder being looked in right now, for the curious.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub folder: String,
    /// A large file being read right now: where it is and its size in
    /// bytes. Reading one can take minutes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reading: Option<(String, u64)>,
    /// True once the looking is over and what was found is being saved.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub saving: bool,
    /// On the last event: true when the search was stopped part-way, so
    /// nothing from it was kept.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub stopped: bool,
    pub finished: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub problem: Option<String>,
    /// On the last event: how many files could not be read, and the first
    /// few by name. All of them are in the log.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub unreadable: usize,
    #[serde(rename = "unreadableNames", default, skip_serializing_if = "Vec::is_empty")]
    pub unreadable_names: Vec<String>,
    /// On the last event: folders Keevoro made (a backup, an old library)
    /// that were left out, each with what is said about it.
    #[serde(rename = "leftOut", default, skip_serializing_if = "Vec::is_empty")]
    pub left_out: Vec<super::own::LeftOut>,
    /// On the last event: files that could be photos or videos but were
    /// only in online storage, so were passed over unopened, and that said
    /// with how to include them.
    #[serde(rename = "onlineOnly", default, skip_serializing_if = "is_zero")]
    pub online_only: usize,
    #[serde(rename = "onlineOnlySaid", default, skip_serializing_if = "Option::is_none")]
    pub online_only_said: Option<String>,
    /// On the last event: the drive went part-way, a drive unplugged, said
    /// in a sentence. What was found so far is kept.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unplugged: Option<String>,
    /// On the last event of looking again: the sources not looked through
    /// because they are not there, or were set aside, each said in a
    /// sentence. What was found on them is kept.
    #[serde(rename = "passedOver", default, skip_serializing_if = "Vec::is_empty")]
    pub passed_over: Vec<String>,
    /// On the last event: sources whose place holds a different drive now,
    /// so they were not looked through, each with the question to ask.
    #[serde(rename = "differentDrive", default, skip_serializing_if = "Vec::is_empty")]
    pub different_drive: Vec<DifferentDrive>,
}

/// A source whose place holds a different drive from the one looked
/// through there before. Nothing found on it was replaced; the person
/// answers with `sources.look_again` and `differentDrive`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DifferentDrive {
    /// The source's id.
    pub source: i64,
    /// Its name, such as "Card".
    pub name: String,
    /// Its place, as a person writes it.
    pub place: String,
    /// The question: add it as a new source, or look through it anyway.
    pub asked: String,
    /// What happened, for when nobody is there to answer.
    pub said: String,
}

fn is_zero(n: &usize) -> bool {
    *n == 0
}

/// Progress backing up or checking, and a last one when it is done.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SendEvent {
    /// A short line saying how far it has got, while it runs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<String>,
    pub finished: bool,
    /// On the last event: what happened, as sentences to show.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub summary: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub problem: Option<String>,
    /// On the last event: how it ended, in a word, for a program to act on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<Outcome>,
}

/// How a backup or a check ended, in a word, for a program to act on. The
/// sentences say the same to a person.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Outcome {
    /// Everything it set out to do was done.
    Worked,
    /// It ran, but some files could not be backed up, or are not as they
    /// should be at a destination.
    SomeFiles,
    /// A destination could not be reached.
    Unreachable,
    /// It was refused: a destination without the room, or nowhere to back
    /// up to yet.
    Refused,
    /// It was stopped before it had finished. Nothing is lost.
    Stopped,
}

impl SendEvent {
    pub fn progress(line: &str) -> Self {
        Self {
            line: Some(line.to_string()),
            finished: false,
            summary: Vec::new(),
            problem: None,
            outcome: None,
        }
    }

    /// The last event: the sentences the engine wrote, any problem, and
    /// how it ended.
    pub fn finished(said: &[u8], problem: Option<String>, outcome: Outcome) -> Self {
        Self {
            line: None,
            finished: true,
            summary: String::from_utf8_lossy(said)
                .lines()
                .map(str::trim_end)
                .filter(|l| !l.is_empty())
                .map(str::to_string)
                .collect(),
            problem,
            outcome: Some(outcome),
        }
    }
}

/// A folder a survey suggests looking through.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HintView {
    /// The folder, as it is shown and as it is handed back to be searched.
    pub folder: String,
    pub photos: usize,
    pub videos: usize,
    /// Other files a person put on the list. Never said when there are none.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub others: usize,
    pub bytes: u64,
    /// False when the pictures are so small they are probably not photos.
    pub likely_photos: bool,
    /// True when this folder is already one of the backup's sources.
    pub already_a_source: bool,
    /// Of its photos and videos, those only in online storage, which a
    /// search will pass over unopened, and that said.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub online_only: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub online_only_said: Option<String>,
}

/// What a survey of a drive found: its last word.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SurveyView {
    pub hints: Vec<HintView>,
    pub folders: usize,
    pub files: usize,
    pub unreadable: usize,
    pub seconds: f32,
    pub stopped: bool,
    /// Folders Keevoro made that were left out, each with what is said.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub left_out: Vec<super::own::LeftOut>,
    /// Photos and videos only in online storage, everywhere it looked, and
    /// that said.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub online_only: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub online_only_said: Option<String>,
}

/// One thing a job said. It is written out as the shape inside it, with
/// nothing around it, which is exactly what the app's screens receive.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum Said {
    /// Looking through a source, and its end.
    Scan(ScanEvent),
    /// How far a survey has got.
    Survey(SurveyEvent),
    /// What a survey found, when it ends. The app has it as the answer to
    /// `survey_place`, not as an event.
    Surveyed(SurveyView),
    /// Backing up or checking, and its end.
    Send(SendEvent),
    /// Checking for duplicates in the background, and its end.
    Check(CheckEvent),
}

impl Said {
    /// Its name, which is also the name of the window event the app sends
    /// it as: `scan`, `survey`, `send` or `check`. A survey's result is
    /// `surveyed`.
    pub fn name(&self) -> &'static str {
        match self {
            Said::Scan(_) => "scan",
            Said::Survey(_) => "survey",
            Said::Surveyed(_) => "surveyed",
            Said::Send(_) => "send",
            Said::Check(_) => "check",
        }
    }

    /// Reads back what another program wrote down. Each shape has a field
    /// only it has, which says which it is; the shapes are written with
    /// nothing around them, as the app's screens receive them.
    fn from_value(value: serde_json::Value) -> serde_json::Result<Self> {
        let has = |field: &str| value.get(field).is_some();
        if has("hints") {
            serde_json::from_value(value).map(Said::Surveyed)
        } else if has("name") {
            serde_json::from_value(value).map(Said::Scan)
        } else if has("done") {
            serde_json::from_value(value).map(Said::Check)
        } else if has("folders") {
            serde_json::from_value(value).map(Said::Survey)
        } else {
            serde_json::from_value(value).map(Said::Send)
        }
    }

    /// True for the last thing a job says.
    pub fn is_last(&self) -> bool {
        match self {
            Said::Scan(e) => e.finished,
            Said::Send(e) => e.finished,
            Said::Check(e) => e.finished,
            Said::Surveyed(_) => true,
            Said::Survey(_) => false,
        }
    }
}

impl<'de> Deserialize<'de> for Said {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let value = serde_json::Value::deserialize(d)?;
        Said::from_value(value).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whatever_a_job_says_reads_back_as_what_it_was() {
        let hint = HintView {
            folder: "DCIM".into(),
            photos: 1,
            videos: 2,
            others: 0,
            bytes: 3,
            likely_photos: true,
            already_a_source: false,
            online_only: 2,
            online_only_said: keevoro_engine::summary::online_only(2, None, std::path::Path::new("/media/card")),
        };
        let all = [
            Said::Scan(ScanEvent {
                name: "Card".into(),
                photos: 1,
                videos: 2,
                others: 0,
                files: 3,
                folder: String::new(),
                reading: None,
                saving: false,
                stopped: true,
                finished: true,
                problem: None,
                unreadable: 0,
                unreadable_names: Vec::new(),
                left_out: Vec::new(),
                online_only: 412,
                online_only_said: keevoro_engine::summary::online_only(412, None, std::path::Path::new("/media/card")),
                unplugged: Some(crate::words::unplugged_while_looking("KINGSTON (E:)", true)),
                passed_over: vec![crate::words::not_plugged_in("KINGSTON (E:)", true)],
                different_drive: vec![DifferentDrive {
                    source: 2,
                    name: "Card".into(),
                    place: "E:\\".into(),
                    asked: crate::words::different_drive_asked("Card"),
                    said: crate::words::different_drive_said("Card", "E:\\"),
                }],
            }),
            Said::Survey(SurveyEvent {
                folders: 1,
                files: 2,
                photos: 3,
                videos: 4,
                others: 0,
                sizing: false,
                top: vec![TopView {
                    name: "Camera Roll".into(),
                    node: "folder".into(),
                    bytes: 9,
                    photos: 2,
                    videos: 0,
                    others: 1,
                    photo_bytes: 8,
                    video_bytes: 0,
                    other_bytes: 1,
                    carried: 0,
                    carried_bytes: 0,
                    folders: 0,
                }],
            }),
            Said::Surveyed(SurveyView {
                hints: vec![hint],
                folders: 1,
                files: 2,
                unreadable: 0,
                seconds: 0.5,
                stopped: false,
                left_out: Vec::new(),
                online_only: 2,
                online_only_said: keevoro_engine::summary::online_only(2, None, std::path::Path::new("/media/card")),
            }),
            Said::Send(SendEvent::progress("Getting ready")),
            Said::Send(SendEvent::finished(b"Done.\n", Some("x".into()), Outcome::Unreachable)),
            // The fewest fields any of them has.
            Said::Send(SendEvent::finished(b"", None, Outcome::Stopped)),
            Said::Check(CheckEvent {
                done: 1,
                total: 2,
                finished: false,
                ..CheckEvent::default()
            }),
            Said::Check(CheckEvent {
                done: 1,
                total: 2,
                finished: true,
                stopped: true,
                left: 1,
                said: crate::words::check_stopped(1),
            }),
        ];
        for said in all {
            let written = serde_json::to_string(&said).unwrap();
            let read: Said = serde_json::from_str(&written).unwrap();
            assert_eq!(read, said, "{written}");
        }
    }

    #[test]
    fn events_have_the_shape_the_screens_expect() {
        let going = SendEvent::progress("3,412 of 9,870 safely backed up");
        assert_eq!(
            serde_json::to_value(&going).unwrap(),
            serde_json::json!({ "line": "3,412 of 9,870 safely backed up", "finished": false })
        );
        let done = SendEvent::finished(
            b"Safely backed up 3 files (1.2 MB).\n\nYour private index: x\n",
            None,
            Outcome::Worked,
        );
        assert_eq!(
            done.summary,
            ["Safely backed up 3 files (1.2 MB).", "Your private index: x"]
        );
        assert!(done.finished && done.line.is_none());
        assert_eq!(serde_json::to_value(&done).unwrap()["outcome"], "worked");
        let failed = SendEvent::finished(
            b"",
            Some("Couldn't connect to the storage.".into()),
            Outcome::Unreachable,
        );
        assert_eq!(
            serde_json::to_value(&failed).unwrap(),
            serde_json::json!({ "finished": true, "problem": "Couldn't connect to the storage.", "outcome": "unreachable" })
        );
        for (outcome, word) in [
            (Outcome::SomeFiles, "someFiles"),
            (Outcome::Refused, "refused"),
            (Outcome::Stopped, "stopped"),
        ] {
            assert_eq!(serde_json::to_value(outcome).unwrap(), word);
        }

        let scan = ScanEvent {
            name: "Dad's old Dell".into(),
            photos: 4210,
            videos: 312,
            files: 98_000,
            folder: String::new(),
            reading: None,
            saving: false,
            stopped: false,
            finished: false,
            problem: None,
            unreadable: 0,
            ..ScanEvent::default()
        };
        assert_eq!(
            serde_json::to_value(&scan).unwrap(),
            serde_json::json!({ "name": "Dad's old Dell", "photos": 4210, "videos": 312, "files": 98000, "finished": false })
        );
    }

    #[test]
    fn what_is_said_is_written_as_the_shape_inside_it_and_named_as_the_app_names_it() {
        let check = CheckEvent {
            done: 2,
            total: 9,
            finished: false,
            ..CheckEvent::default()
        };
        assert_eq!(
            serde_json::to_value(Said::Check(check.clone())).unwrap(),
            serde_json::to_value(&check).unwrap()
        );
        let mut survey = SurveyEvent {
            folders: 3,
            files: 40,
            photos: 30,
            videos: 1,
            others: 0,
            sizing: false,
            top: Vec::new(),
        };
        assert_eq!(
            serde_json::to_value(Said::Survey(survey.clone())).unwrap(),
            serde_json::json!({ "folders": 3, "files": 40, "photos": 30, "videos": 1 }),
            "as older readers expect it"
        );
        survey.sizing = true;
        assert_eq!(
            serde_json::to_value(Said::Survey(survey.clone())).unwrap()["sizing"],
            true
        );
        // The top of the map as it grows, read back as a survey's progress
        // still, by its folders.
        let mut grown = survey.clone();
        grown.top = vec![TopView::from(&keevoro_scan::map::TopBox {
            name: "Others".into(),
            item: keevoro_scan::map::TopItem::Smaller(3),
            kinds: Default::default(),
        })];
        let written = serde_json::to_value(Said::Survey(grown.clone())).unwrap();
        assert_eq!(written["top"][0]["node"], "smaller");
        assert_eq!(written["top"][0]["folders"], 3);
        assert_eq!(serde_json::from_value::<Said>(written).unwrap(), Said::Survey(grown));
        let names: Vec<&str> = [
            Said::Scan(ScanEvent {
                name: String::new(),
                photos: 0,
                videos: 0,
                files: 0,
                folder: String::new(),
                reading: Some(("a".into(), 1)),
                saving: true,
                stopped: true,
                finished: true,
                problem: Some("x".into()),
                unreadable: 1,
                unreadable_names: vec!["b".into()],
                ..ScanEvent::default()
            }),
            Said::Survey(survey.clone()),
            Said::Send(SendEvent::progress("x")),
            Said::Check(check),
        ]
        .iter()
        .map(Said::name)
        .collect();
        assert_eq!(names, ["scan", "survey", "send", "check"]);
        assert!(!Said::Send(SendEvent::progress("x")).is_last());
        assert!(Said::Send(SendEvent::finished(b"", None, Outcome::Worked)).is_last());
    }
}
