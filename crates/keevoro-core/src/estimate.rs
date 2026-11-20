//! How long until a backup is completely finished.
//!
//! A backup is several kinds of work, one after another and some side by
//! side: telling apart files that look alike, giving each its place,
//! sending the originals to each destination, making previews (in the
//! background, while the originals go), sending the previews, and writing
//! the list of what is there. A person wants one answer to "how long until
//! it is all done?", not a fresh guess at the start of every stage they had
//! forgotten about. [`Estimator`] gives that answer.
//!
//! It is pure: it is told the time, it is told what has been done, and it
//! reads no clock and no disk, so every rule here is tested with made-up
//! clocks and rates. [`Tracker`] is the thin wrapper a running backup uses,
//! with the real clock and the count of bytes sent.
//!
//! How it thinks:
//!
//! - **Each stage has an amount of work** (bytes or files), an amount done,
//!   and a rate. The rate of a running stage is measured over a window that
//!   grows with how long the stage has run (a third of it, between one
//!   minute and one hour), so a day-long backup is not swung by ten seconds
//!   of good or bad luck.
//! - **Conditions change.** A shorter window (a sixth of the long one,
//!   between 15 seconds and 10 minutes) is watched beside it. When the two
//!   disagree by half again or more, and keep disagreeing for as long as
//!   the short window, the speed really has changed (the network got
//!   slower, someone else started copying), and the long window starts
//!   again from where the change began. A stall is not adopted that way: it
//!   slides out of the long window gradually.
//! - **A stage not started yet** borrows a rate: one measured for the same
//!   kind of work earlier in this backup, then one remembered from earlier
//!   backups on this computer, and only then a cautious built-in guess. A
//!   figure resting on guesses is said more roughly, or not at all.
//! - **Side by side.** Previews are made in the background while the
//!   originals are sent. The sending of previews waits for both, so the
//!   total is worked out by playing the remaining stages forward on two
//!   lanes, not by adding them up.
//! - **The answer is published on a schedule** ([`republish_every`]): a
//!   tenth of the time left, at least every ten seconds and at most once an
//!   hour, so about five times an hour on a two-hour backup. Between times
//!   it counts down smoothly. It is worked out again at once when something
//!   material happens: a stage begins or ends, the work to do changes by a
//!   twentieth or more, the speed changes as above, or the countdown runs
//!   out before the work does.
//! - **Paused time does not count**, for rates or for the countdown: the
//!   time left stands still while Keevoro is paused.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, VecDeque};

/// A kind of work a backup does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    /// Reading in full files a quick search only sampled, where two look
    /// alike and must be told apart.
    TellingApart,
    /// Giving each kept file its place.
    Placing,
    /// Sending the originals to one destination.
    SendingOriginals,
    /// Making the previews of photos.
    MakingPreviews,
    /// Taking a still from each video.
    MakingVideoPreviews,
    /// Sending the previews to one destination.
    SendingPreviews,
    /// Writing the index at one destination.
    WritingIndex,
}

/// What a stage's work is counted in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Unit {
    Bytes,
    Files,
}

impl Kind {
    pub fn unit(self) -> Unit {
        match self {
            Kind::SendingOriginals | Kind::SendingPreviews => Unit::Bytes,
            _ => Unit::Files,
        }
    }

    /// True for the work done in the background, alongside the sending.
    pub fn alongside(self) -> bool {
        matches!(self, Kind::MakingPreviews | Kind::MakingVideoPreviews)
    }

    /// The cautious built-in guess at a rate, per second, used only when
    /// nothing better is known. On the slow side on purpose: finishing
    /// early is a pleasant surprise, finishing late is not.
    pub fn guess(self) -> f64 {
        const MB: f64 = 1024.0 * 1024.0;
        match self {
            Kind::TellingApart => 2.0,
            Kind::Placing => 500.0,
            Kind::SendingOriginals => 8.0 * MB,
            Kind::MakingPreviews => 2.0,
            Kind::MakingVideoPreviews => 0.5,
            Kind::SendingPreviews => 2.0 * MB,
            Kind::WritingIndex => 0.2,
        }
    }

    /// What it is called, for a person.
    pub fn label(self, alongside_sending: bool) -> &'static str {
        match self {
            Kind::TellingApart => "Telling apart files that look alike",
            Kind::Placing => "Working out where each file goes",
            Kind::SendingOriginals => "Backing up your files",
            Kind::MakingPreviews if alongside_sending => "Making previews in the background",
            Kind::MakingPreviews => "Making previews",
            Kind::MakingVideoPreviews if alongside_sending => "Making video previews in the background",
            Kind::MakingVideoPreviews => "Making video previews",
            Kind::SendingPreviews => "Backing up the previews",
            Kind::WritingIndex => "Writing the list of what is there",
        }
    }

    fn key(self) -> &'static str {
        match self {
            Kind::TellingApart => "tellingApart",
            Kind::Placing => "placing",
            Kind::SendingOriginals => "sendingOriginals",
            Kind::MakingPreviews => "makingPreviews",
            Kind::MakingVideoPreviews => "makingVideoPreviews",
            Kind::SendingPreviews => "sendingPreviews",
            Kind::WritingIndex => "writingIndex",
        }
    }
}

/// Rates measured in earlier backups on this computer, per second, by kind
/// of work and, for sending, by destination. Kept in Keevoro's own data,
/// never at a source or a destination.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Remembered {
    pub rates: BTreeMap<String, f64>,
    /// Bytes of previews made for each original, on average: how much
    /// sending the previews will be before they exist.
    #[serde(default)]
    pub preview_bytes_per_original: Option<f64>,
}

impl Remembered {
    fn key(kind: Kind, to: Option<&str>) -> String {
        match to {
            Some(to) => format!("{}:{to}", kind.key()),
            None => kind.key().to_string(),
        }
    }

    fn rate(&self, kind: Kind, to: Option<&str>) -> Option<f64> {
        let found = |key: &str| self.rates.get(key).copied().filter(|r| r.is_finite() && *r > 0.0);
        to.and_then(|to| found(&Self::key(kind, Some(to))))
            .or_else(|| found(&Self::key(kind, None)))
    }

    /// What was remembered, brought up to date with what was measured
    /// now. Each figure moves half way to the new one, so one odd backup
    /// does not undo many ordinary ones.
    pub fn updated_with(&self, measured: &Remembered) -> Remembered {
        let blend = |old: Option<f64>, new: f64| old.map_or(new, |old| (old + new) / 2.0);
        let mut out = self.clone();
        for (key, rate) in &measured.rates {
            out.rates
                .insert(key.clone(), blend(self.rates.get(key).copied(), *rate));
        }
        if let Some(new) = measured.preview_bytes_per_original {
            out.preview_bytes_per_original = Some(blend(self.preview_bytes_per_original, new));
        }
        out
    }

    /// The bytes of previews one original makes, remembered or guessed.
    pub fn preview_bytes(&self) -> f64 {
        self.preview_bytes_per_original
            .filter(|b| b.is_finite() && *b > 0.0)
            .unwrap_or(GUESSED_PREVIEW_BYTES)
    }
}

/// A thumbnail and a screen-sized picture together, as a guess.
const GUESSED_PREVIEW_BYTES: f64 = 400.0 * 1024.0;

/// How sure the answer is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Sureness {
    /// Measured: said as "about".
    Fair,
    /// Partly guessed, or measured only briefly: said more roundly, as
    /// "roughly".
    Rough,
    /// Too little is known to say anything honest yet.
    Unknown,
    /// Nothing has moved for a while.
    Stalled,
}

/// Where a stage stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum State {
    Done,
    /// What the backup is doing now.
    Now,
    /// Under way in the background, beside what it is doing now.
    Alongside,
    /// Still to come.
    Next,
}

/// One stage as a person sees it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StageView {
    pub kind: Kind,
    /// Which destination, for work done once per destination.
    pub to: Option<String>,
    pub state: State,
    pub unit: Unit,
    pub done: u64,
    pub work: u64,
    /// Per second, while it runs and once measured.
    pub per_second: Option<f64>,
    /// In a few words, as shown: "Backing up your files: 708.4 GB of 716.8
    /// GB at 7.5 MB/s".
    pub line: String,
}

/// How long until the whole backup is finished, as published.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Estimate {
    /// `None` when there is no honest answer yet.
    pub seconds_left: Option<u64>,
    pub sureness: Sureness,
    /// The time left in words: "About 12 minutes left", "Roughly 6 hours
    /// left", "Working out how long this will take".
    pub left: String,
    /// How much of the whole backup is done, from 0 to 1, by time.
    pub fraction_done: f64,
    /// Every stage, in order, done and still to come.
    pub stages: Vec<StageView>,
    /// Seconds Keevoro has been paused during this backup.
    pub paused_seconds: u64,
    /// How long it has run and how long is left, in one line (see
    /// [`headline`]). Filled in by whoever knows when it started.
    #[serde(default)]
    pub headline: String,
}

/// A stage of the plan: what kind, for which destination, and how much.
#[derive(Debug, Clone, PartialEq)]
pub struct Planned {
    pub kind: Kind,
    pub to: Option<String>,
    pub work: u64,
}

impl Planned {
    pub fn new(kind: Kind, to: Option<&str>, work: u64) -> Self {
        Self {
            kind,
            to: to.map(str::to_string),
            work,
        }
    }
}

/// How often the published time left is worked out afresh, given how
/// long is left: a tenth of it, at least every ten seconds and at most
/// once an hour. Two hours left is every twelve minutes, five times an
/// hour; a day left is once an hour; ten minutes left is every minute.
pub fn republish_every(seconds_left: f64) -> f64 {
    (seconds_left / 10.0).clamp(10.0, 3600.0)
}

/// The long window a running stage's rate is measured over, given how long
/// it has run: a third of that, between one minute and one hour.
pub fn long_window(running_for: f64) -> f64 {
    (running_for / 3.0).clamp(60.0, 3600.0)
}

/// The short window watched beside it, for a change in conditions: a sixth
/// of the long one, between 15 seconds and 10 minutes. A change must also
/// last this long before it is believed.
pub fn short_window(long: f64) -> f64 {
    (long / 6.0).clamp(15.0, 600.0)
}

/// By how much the short and long rates must differ, either way, to count
/// as conditions changing.
pub const CHANGE: f64 = 1.5;

/// A stage is measured once it has run this long and moved.
const MEASURED_AFTER: f64 = 20.0;

/// A speed counts as sustained once it has held for this long: the
/// slowest and fastest a stage records are over at least a minute.
const SUSTAINED_AFTER: f64 = 60.0;

/// What one stage of a backup did, for the backup's history: how long it
/// took (time paused left out), how much it did, and how fast.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StageRecord {
    pub kind: Kind,
    /// The destination, for a stage done at one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
    pub unit: Unit,
    /// Files or bytes, as `unit` says.
    pub done: u64,
    pub seconds: u64,
    /// False for a stage the backup did not get to the end of.
    pub finished: bool,
    /// The average over the whole stage, per second.
    pub per_second: Option<f64>,
    /// The slowest and fastest held for a minute or more; none for a stage
    /// shorter than that.
    pub slowest: Option<f64>,
    pub fastest: Option<f64>,
}

/// The work to do changing by this share counts as material.
const WORK_CHANGE: f64 = 0.05;

#[derive(Debug, Clone)]
struct Stage {
    kind: Kind,
    to: Option<String>,
    work: u64,
    done: u64,
    began: Option<f64>,
    ended: Option<f64>,
    /// (time, done), oldest first, thinned and trimmed to the long window.
    samples: VecDeque<(f64, u64)>,
    /// Where the current run of conditions began: the long window never
    /// reaches back past it.
    since: Option<(f64, u64)>,
    /// When the short and long rates began to disagree.
    diverged: Option<f64>,
    /// The slowest and fastest rates held for a whole long window, once it
    /// has run a minute: what a person would call its sustained speeds.
    slowest: Option<f64>,
    fastest: Option<f64>,
}

impl Stage {
    fn new(planned: Planned) -> Self {
        Self {
            kind: planned.kind,
            to: planned.to,
            work: planned.work,
            done: 0,
            began: None,
            ended: None,
            samples: VecDeque::new(),
            since: None,
            diverged: None,
            slowest: None,
            fastest: None,
        }
    }

    fn is(&self, kind: Kind, to: Option<&str>) -> bool {
        self.kind == kind && self.to.as_deref() == to
    }

    fn finished(&self) -> bool {
        self.ended.is_some()
    }

    fn running(&self) -> bool {
        self.began.is_some() && !self.finished()
    }

    fn left(&self) -> u64 {
        if self.finished() {
            0
        } else {
            self.work.saturating_sub(self.done)
        }
    }

    /// How much was done at `t`, from the samples.
    fn done_at(&self, t: f64) -> (f64, u64) {
        let mut best = self.samples.front().copied().unwrap_or((t, self.done));
        for &(at, done) in &self.samples {
            if at <= t {
                best = (at, done);
            } else {
                break;
            }
        }
        best
    }

    fn rate_over(&self, now: f64, window: f64) -> Option<f64> {
        let from = (now - window).max(self.since.map_or(f64::MIN, |s| s.0));
        let (at, done) = self.done_at(from);
        let (at, done) = match self.since {
            Some((s_at, s_done)) if s_at > at => (s_at, s_done),
            _ => (at, done),
        };
        let seconds = now - at;
        (seconds > 0.0).then(|| self.done.saturating_sub(done) as f64 / seconds)
    }

    fn running_for(&self, now: f64) -> f64 {
        self.began.map_or(0.0, |b| (self.ended.unwrap_or(now) - b).max(0.0))
    }

    /// The rate measured for this stage, once it has run long enough to
    /// say. Zero means it has stalled.
    fn measured(&self, now: f64) -> Option<f64> {
        if self.finished() {
            let seconds = self.running_for(now);
            return (seconds >= 1.0 && self.done > 0).then(|| self.done as f64 / seconds);
        }
        if self.running_for(now) < MEASURED_AFTER {
            return None;
        }
        let window = long_window(self.running_for(now));
        self.rate_over(now, window)
    }

    fn note(&mut self, now: f64) {
        let window = long_window(self.running_for(now));
        let spacing = (window / 200.0).max(1.0);
        if self.samples.back().is_none_or(|&(at, _)| now - at >= spacing) {
            self.samples.push_back((now, self.done));
        } else if let Some(last) = self.samples.back_mut() {
            last.1 = self.done;
        }
        let keep_from = now - window - spacing;
        while self.samples.len() > 2 && self.samples.get(1).is_some_and(|&(at, _)| at < keep_from) {
            self.samples.pop_front();
        }
        if self.running_for(now) >= SUSTAINED_AFTER {
            if let Some(rate) = self.rate_over(now, window) {
                self.slowest = Some(self.slowest.map_or(rate, |r| r.min(rate)));
                self.fastest = Some(self.fastest.map_or(rate, |r| r.max(rate)));
            }
        }
    }

    /// What it did, for the backup's history.
    fn record(&self, now: f64) -> StageRecord {
        let seconds = self.running_for(now);
        StageRecord {
            kind: self.kind,
            to: self.to.clone(),
            unit: self.kind.unit(),
            done: self.done,
            seconds: seconds.round() as u64,
            finished: self.finished(),
            per_second: (seconds >= 1.0 && self.done > 0).then(|| self.done as f64 / seconds),
            slowest: self.slowest,
            fastest: self.fastest,
        }
    }

    /// Watches for conditions changing. True when a change has lasted
    /// long enough to believe, and the long window starts again from it.
    fn conditions_changed(&mut self, now: f64) -> bool {
        if !self.running() || self.running_for(now) < MEASURED_AFTER {
            return false;
        }
        let long = long_window(self.running_for(now));
        let short = short_window(long);
        let (Some(long_rate), Some(short_rate)) = (self.rate_over(now, long), self.rate_over(now, short)) else {
            return false;
        };
        // A stall slides out of the long window by itself; adopting it
        // would turn a pause for breath into "no idea".
        let differs = long_rate > 0.0 && short_rate > 0.0 && {
            let ratio = short_rate / long_rate;
            !(1.0 / CHANGE..CHANGE).contains(&ratio)
        };
        if !differs {
            self.diverged = None;
            return false;
        }
        let since = *self.diverged.get_or_insert(now);
        if now - since < short.max(30.0) {
            return false;
        }
        // Believed: measure from where the short window began.
        self.since = Some(self.done_at(now - short));
        self.diverged = None;
        true
    }
}

/// Where a rate came from, best first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Source {
    Measured,
    ThisRun,
    Earlier,
    Guess,
}

/// The time left as last published, in the estimator's own time (which
/// stands still while paused).
#[derive(Debug, Clone)]
struct Published {
    at: f64,
    ends: Option<f64>,
    sureness: Sureness,
    fraction_at: f64,
}

/// Works out, and keeps working out, how long a backup has left.
#[derive(Debug, Clone)]
pub struct Estimator {
    stages: Vec<Stage>,
    remembered: Remembered,
    /// Seconds paused so far, and since when if paused now.
    paused: f64,
    paused_since: Option<f64>,
    published: Option<Published>,
    /// Something material has happened since the last publish.
    material: bool,
}

impl Estimator {
    pub fn new(remembered: Remembered) -> Self {
        Self {
            stages: Vec::new(),
            remembered,
            paused: 0.0,
            paused_since: None,
            published: None,
            material: true,
        }
    }

    /// The time that counts: the clock, less any time paused.
    fn active(&self, now: f64) -> f64 {
        now - self.paused - self.paused_since.map_or(0.0, |since| now - since)
    }

    /// Says what the backup will do, in order. Stages already known keep
    /// what has been done; their work is taken from the new plan.
    pub fn plan(&mut self, planned: Vec<Planned>) {
        let mut old = std::mem::take(&mut self.stages);
        for p in planned {
            match old.iter().position(|s| s.is(p.kind, p.to.as_deref())) {
                Some(at) => {
                    let mut stage = old.remove(at);
                    stage.work = p.work.max(stage.done);
                    self.stages.push(stage);
                }
                None => self.stages.push(Stage::new(p)),
            }
        }
        self.material = true;
    }

    /// The stage of this kind for this destination, added at the end if
    /// the plan did not have it.
    fn find_or_add(&mut self, kind: Kind, to: Option<&str>) -> usize {
        if let Some(at) = self.stages.iter().position(|s| s.is(kind, to)) {
            return at;
        }
        self.stages.push(Stage::new(Planned::new(kind, to, 0)));
        self.material = true;
        self.stages.len() - 1
    }

    /// The work a stage has to do, now that it is better known.
    pub fn set_work(&mut self, kind: Kind, to: Option<&str>, work: u64) {
        let at = self.find_or_add(kind, to);
        let stage = &mut self.stages[at];
        let before = stage.work.max(1) as f64;
        if (work as f64 - stage.work as f64).abs() / before >= WORK_CHANGE {
            self.material = true;
        }
        stage.work = work.max(stage.done);
    }

    /// How far a stage has got. The first word from a stage begins it.
    pub fn progress(&mut self, kind: Kind, to: Option<&str>, done: u64, now: f64) {
        let t = self.active(now);
        let at = self.find_or_add(kind, to);
        let stage = &mut self.stages[at];
        if stage.began.is_none() {
            stage.began = Some(t);
            stage.since = Some((t, done));
            self.material = true;
        }
        stage.done = done.max(stage.done);
        if stage.done > stage.work {
            stage.work = stage.done;
        }
        stage.note(t);
    }

    /// A stage is over, whether or not all its work was needed.
    /// A stage the plan never had is left out.
    pub fn finish(&mut self, kind: Kind, to: Option<&str>, now: f64) {
        let t = self.active(now);
        let Some(stage) = self.stages.iter_mut().find(|s| s.is(kind, to)) else {
            return;
        };
        if stage.began.is_none() {
            stage.began = Some(t);
        }
        if stage.ended.is_none() {
            stage.ended = Some(t);
            // Whatever was not needed (already there, say) is not left
            // over. A stage that said nothing as it went keeps its plan,
            // so it is still shown as done.
            if stage.done > 0 {
                stage.work = stage.done;
            }
            self.material = true;
        }
    }

    /// Keevoro has been paused, or let go on.
    pub fn set_paused(&mut self, paused: bool, now: f64) {
        match (paused, self.paused_since) {
            (true, None) => self.paused_since = Some(now),
            (false, Some(since)) => {
                self.paused += now - since;
                self.paused_since = None;
                self.material = true;
            }
            _ => {}
        }
    }

    /// Every stage that began, as it stands now, for the backup's history.
    pub fn record(&self, now: f64) -> Vec<StageRecord> {
        let t = self.active(now);
        self.stages
            .iter()
            .filter(|s| s.began.is_some())
            .map(|s| s.record(t))
            .collect()
    }

    /// Seconds spent paused so far.
    pub fn paused_seconds(&self, now: f64) -> u64 {
        (self.paused + self.paused_since.map_or(0.0, |since| now - since)).round() as u64
    }

    /// Rates measured in this backup, for remembering.
    pub fn measured(&self, now: f64) -> Remembered {
        let t = self.active(now);
        let mut out = Remembered::default();
        for s in &self.stages {
            // Only a fair stretch of work is worth remembering.
            if s.running_for(t) < 60.0 && !(s.finished() && s.running_for(t) >= 5.0) {
                continue;
            }
            if let Some(rate) = s.measured(t).filter(|r| *r > 0.0) {
                out.rates.insert(Remembered::key(s.kind, s.to.as_deref()), rate);
                out.rates.entry(Remembered::key(s.kind, None)).or_insert(rate);
            }
        }
        out
    }

    /// The rate for a stage, and where it came from.
    fn rate_for(&self, at: usize, t: f64) -> (Option<f64>, Source) {
        let stage = &self.stages[at];
        if let Some(rate) = stage.measured(t) {
            return (Some(rate), Source::Measured);
        }
        let to = stage.to.as_deref();
        let earlier_here = |kind: Kind, same_to: bool| {
            self.stages
                .iter()
                .enumerate()
                .filter(|(i, s)| *i != at && s.kind == kind && (!same_to || s.to.as_deref() == to))
                .find_map(|(_, s)| s.measured(t).filter(|r| *r > 0.0))
        };
        let this_run = earlier_here(stage.kind, true)
            .or_else(|| earlier_here(stage.kind, false))
            // Previews go the same way as the originals did, but as many
            // small files, which are slower: half the speed.
            .or_else(|| {
                (stage.kind == Kind::SendingPreviews)
                    .then(|| earlier_here(Kind::SendingOriginals, true).map(|r| r / 2.0))
                    .flatten()
            });
        if let Some(rate) = this_run {
            return (Some(rate), Source::ThisRun);
        }
        if let Some(rate) = self.remembered.rate(stage.kind, to) {
            return (Some(rate), Source::Earlier);
        }
        (Some(stage.kind.guess()), Source::Guess)
    }

    /// Plays the rest of the backup forward: the sending lane one stage
    /// after another, the background lane beside it, and the sending of
    /// previews waiting for the background to finish. Answers the seconds
    /// left, the share of them that rests on guesses, and whether the
    /// stage in hand has stalled or is too new to measure.
    fn play_forward(&self, t: f64) -> Played {
        let mut sender = 0.0_f64;
        // The background is under way already, or starts once every file
        // has its place.
        let mut background: Option<f64> = self
            .stages
            .iter()
            .any(|s| s.kind.alongside() && s.began.is_some())
            .then_some(0.0);
        let mut guessed = 0.0;
        let mut stalled = false;
        let mut new_and_guessed = false;
        for (at, stage) in self.stages.iter().enumerate() {
            if background.is_none() && !matches!(stage.kind, Kind::TellingApart | Kind::Placing) {
                background = Some(sender);
            }
            let left = stage.left();
            if left == 0 {
                continue;
            }
            let (rate, source) = self.rate_for(at, t);
            let seconds = match rate {
                Some(r) if r > 0.0 => left as f64 / r,
                _ => {
                    stalled = stalled || stage.running();
                    0.0
                }
            };
            if source == Source::Guess {
                guessed += seconds;
                new_and_guessed = new_and_guessed || stage.running() || !stage.kind.alongside();
            }
            if stage.kind.alongside() {
                background = Some(background.unwrap_or(sender) + seconds);
            } else {
                if stage.kind == Kind::SendingPreviews {
                    sender = sender.max(background.unwrap_or(0.0));
                }
                sender += seconds;
            }
        }
        Played {
            total: sender.max(background.unwrap_or(0.0)),
            guessed,
            stalled,
            new_and_guessed,
        }
    }

    /// The time left, published on the schedule in the module's notes.
    pub fn estimate(&mut self, now: f64) -> Estimate {
        let t = self.active(now);
        for stage in &mut self.stages {
            if stage.conditions_changed(t) {
                self.material = true;
            }
        }
        let played = self.play_forward(t);
        let sureness = if played.stalled {
            Sureness::Stalled
        } else if played.total > 0.0 && played.guessed / played.total >= 0.5 && played.new_and_guessed {
            Sureness::Unknown
        } else if played.total > 0.0 && played.guessed / played.total >= 0.1 {
            Sureness::Rough
        } else {
            Sureness::Fair
        };
        let all_done = self.stages.iter().all(|s| s.left() == 0);
        let due = match &self.published {
            None => true,
            Some(p) => {
                self.material
                    || p.sureness != sureness
                    || t - p.at >= republish_every(p.ends.map_or(0.0, |e| e - p.at))
                    || p.ends.is_some_and(|e| t >= e && !all_done)
            }
        };
        if due {
            let ends = matches!(sureness, Sureness::Fair | Sureness::Rough).then_some(t + played.total);
            self.published = Some(Published {
                at: t,
                ends,
                sureness,
                fraction_at: self.fraction_by_stages(),
            });
            self.material = false;
        }
        let published = self.published.clone().unwrap_or(Published {
            at: t,
            ends: None,
            sureness,
            fraction_at: 0.0,
        });
        let seconds_left = published.ends.map(|e| (e - t).max(0.0).round() as u64);
        let fraction_done = match published.ends {
            // By time: what has gone, of what has gone and what is left.
            Some(e) if t > 0.0 => {
                let first = self.stages.iter().filter_map(|s| s.began).fold(t, f64::min);
                let gone = (t - first).max(0.0);
                let left = (e - t).max(0.0);
                if gone + left > 0.0 {
                    gone / (gone + left)
                } else {
                    1.0
                }
            }
            _ => published.fraction_at,
        };
        Estimate {
            seconds_left,
            sureness: published.sureness,
            left: left_in_words(seconds_left, published.sureness),
            fraction_done: if all_done { 1.0 } else { fraction_done.clamp(0.0, 0.999) },
            stages: self.views(t),
            paused_seconds: (now - t).max(0.0).round() as u64,
            headline: String::new(),
        }
    }

    /// Share of the stages done, for when there is no time to go by.
    fn fraction_by_stages(&self) -> f64 {
        let n = self.stages.len().max(1) as f64;
        self.stages
            .iter()
            .map(|s| {
                if s.finished() {
                    1.0
                } else if s.work > 0 {
                    s.done as f64 / s.work as f64
                } else {
                    0.0
                }
            })
            .sum::<f64>()
            / n
    }

    fn views(&self, t: f64) -> Vec<StageView> {
        let sending = self.stages.iter().any(|s| s.running() && !s.kind.alongside());
        let several = {
            let mut to: Vec<&str> = self.stages.iter().filter_map(|s| s.to.as_deref()).collect();
            to.dedup();
            to.len() > 1
        };
        self.stages
            .iter()
            // A stage that had nothing to do is not worth a line.
            .filter(|s| s.work > 0 || s.done > 0 || s.running())
            .map(|s| {
                // Work made alongside (previews) has no moment it is told it
                // is over until the sending ends, but once every one is made
                // it is done, and should say so.
                let all_made = s.kind.alongside() && s.running() && s.work > 0 && s.left() == 0;
                let state = if s.finished() || all_made {
                    State::Done
                } else if s.running() && s.kind.alongside() && sending {
                    State::Alongside
                } else if s.running() {
                    State::Now
                } else {
                    State::Next
                };
                let per_second = if s.running() {
                    s.measured(t).filter(|r| *r > 0.0)
                } else {
                    None
                };
                let mut label = s.kind.label(state == State::Alongside).to_string();
                if several {
                    if let Some(to) = &s.to {
                        label = format!("{label} to {to}");
                    }
                }
                let line = match (state, s.kind.unit()) {
                    // One small file per destination: nothing to count.
                    _ if s.kind == Kind::WritingIndex => label,
                    (State::Done, _) => label,
                    (State::Next, _) if s.work == 0 => label,
                    (State::Next, Unit::Bytes) => format!("{label}: {}", size(s.work)),
                    (State::Next, Unit::Files) => format!("{label}: {}", thousands(s.work)),
                    (_, Unit::Bytes) => format!(
                        "{label}: {} of {}{}",
                        size(s.done),
                        size(s.work),
                        per_second.map_or(String::new(), |r| format!(" at {}/s", size(r as u64)))
                    ),
                    (_, Unit::Files) if s.work == 0 => label,
                    (_, Unit::Files) => format!("{label}: {} of {}", thousands(s.done), thousands(s.work)),
                };
                StageView {
                    kind: s.kind,
                    to: s.to.clone(),
                    state,
                    unit: s.kind.unit(),
                    done: s.done,
                    work: s.work,
                    per_second,
                    line,
                }
            })
            .collect()
    }
}

struct Played {
    total: f64,
    guessed: f64,
    stalled: bool,
    new_and_guessed: bool,
}

/// The time left, as a person would say it: finer when close, rounder
/// when far off or rough, and plain about not knowing.
pub fn left_in_words(seconds_left: Option<u64>, sureness: Sureness) -> String {
    match (sureness, seconds_left) {
        (Sureness::Stalled, _) => "Nothing has moved for a while, so Keevoro can't say how long is left".to_string(),
        (Sureness::Unknown, _) | (_, None) => "Working out how long this will take".to_string(),
        (Sureness::Rough, Some(s)) => format!("Roughly {} left", rough_span(s)),
        (_, Some(s)) => {
            let span = span(s);
            let mut words = format!("{span} left");
            if let Some(first) = words.get(..1) {
                words = format!("{}{}", first.to_uppercase(), &words[1..]);
            }
            words
        }
    }
}

fn plural(n: u64, word: &str) -> String {
    if n == 1 {
        format!("1 {word}")
    } else {
        format!("{n} {word}s")
    }
}

fn hours_and_minutes(minutes: u64) -> String {
    let (h, m) = (minutes / 60, minutes % 60);
    match (h, m) {
        (0, m) => plural(m, "minute"),
        (h, 0) => plural(h, "hour"),
        (h, m) => format!("{} {}", plural(h, "hour"), plural(m, "minute")),
    }
}

fn round_to(seconds: u64, step_minutes: u64) -> u64 {
    let minutes = (seconds as f64 / 60.0 / step_minutes as f64).round() as u64 * step_minutes;
    minutes.max(step_minutes)
}

/// "about 12 minutes", "about 1 hour 45 minutes", "about 6 hours",
/// "about a day": to the minute under twenty minutes, then five minutes,
/// a quarter of an hour, a whole hour, and days.
pub fn span(seconds: u64) -> String {
    const MIN: u64 = 60;
    const HOUR: u64 = 3600;
    if seconds < MIN {
        return "less than a minute".to_string();
    }
    let about = |s: String| format!("about {s}");
    if seconds < 20 * MIN {
        about(hours_and_minutes(round_to(seconds, 1)))
    } else if seconds < HOUR {
        about(hours_and_minutes(round_to(seconds, 5)))
    } else if seconds < 3 * HOUR {
        about(hours_and_minutes(round_to(seconds, 15)))
    } else {
        days_or_hours(seconds, "about")
    }
}

/// As [`span`], a step rounder: "a few minutes", "40 minutes", "1 hour 30
/// minutes", "6 hours", "a day".
pub fn rough_span(seconds: u64) -> String {
    const MIN: u64 = 60;
    const HOUR: u64 = 3600;
    if seconds < 10 * MIN {
        "a few minutes".to_string()
    } else if seconds < HOUR {
        hours_and_minutes(round_to(seconds, 10))
    } else if seconds < 3 * HOUR {
        hours_and_minutes(round_to(seconds, 30))
    } else {
        days_or_hours(seconds, "")
    }
}

fn days_or_hours(seconds: u64, about: &str) -> String {
    const HOUR: u64 = 3600;
    let words = if seconds < 20 * HOUR {
        plural((seconds as f64 / HOUR as f64).round() as u64, "hour")
    } else if seconds < 36 * HOUR {
        "a day".to_string()
    } else {
        plural((seconds as f64 / (24 * HOUR) as f64).round() as u64, "day")
    };
    if about.is_empty() {
        words
    } else {
        format!("{about} {words}")
    }
}

/// How long something has run, to the minute and with no "about", since
/// it is measured: "less than a minute", "5 hours 12 minutes", "1 day 3
/// hours".
pub fn running_for(seconds: u64) -> String {
    let minutes = seconds / 60;
    if minutes == 0 {
        "less than a minute".to_string()
    } else if minutes < 24 * 60 {
        hours_and_minutes(minutes)
    } else {
        let (d, h) = (minutes / (24 * 60), (minutes % (24 * 60)) / 60);
        if h == 0 {
            plural(d, "day")
        } else {
            format!("{} {}", plural(d, "day"), plural(h, "hour"))
        }
    }
}

/// The line a backup's screen leads with: how long it has run (by the
/// clock, since it started, saying how much of that was paused) and how
/// long is left. "Running for 5 hours 12 minutes, about 3 hours left."
pub fn headline(running_seconds: u64, estimate: &Estimate) -> String {
    let mut running = format!("Running for {}", running_for(running_seconds));
    if estimate.paused_seconds >= 60 {
        running.push_str(&format!(" ({} of it paused)", running_for(estimate.paused_seconds)));
    }
    let left = &estimate.left;
    let lower = |s: &str| {
        let mut chars = s.chars();
        chars
            .next()
            .map(|c| c.to_lowercase().chain(chars).collect::<String>())
            .unwrap_or_default()
    };
    match estimate.sureness {
        Sureness::Fair | Sureness::Rough if estimate.seconds_left.is_some() => format!("{running}, {}", lower(left)),
        _ => format!("{running}. {left}."),
    }
}

/// The headline and then each stage on a line of its own, marked done,
/// now or still to come, for front ends that write text: the command line
/// and an agent's summaries.
pub fn in_lines(estimate: &Estimate) -> Vec<String> {
    let mut lines = vec![estimate.headline.clone()];
    lines.retain(|l| !l.is_empty());
    for stage in &estimate.stages {
        let mark = match stage.state {
            State::Done => "Done",
            State::Now | State::Alongside => "Now",
            State::Next => "Then",
        };
        lines.push(format!("{mark}: {}", stage.line));
    }
    lines
}

/// Bytes as the app shows them: "708.4 GB".
pub fn size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["bytes", "KB", "MB", "GB", "TB"];
    let mut v = bytes as f64;
    let mut unit = 0;
    while v >= 1024.0 && unit < UNITS.len() - 1 {
        v /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} bytes")
    } else {
        format!("{v:.1} {}", UNITS[unit])
    }
}

/// A count with thousands marked: "10,547".
pub fn thousands(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// A running backup's estimator, with the real clock and the count of
/// bytes sent. Shared between the backup's threads and whoever asks how it
/// is getting on.
#[derive(Clone)]
pub struct Tracker {
    inner: std::sync::Arc<std::sync::Mutex<Tracked>>,
    began: std::time::Instant,
}

struct Tracked {
    estimator: Estimator,
    /// Bytes of previews per original, as measured in this backup.
    preview_bytes: Option<f64>,
    /// The sending stage measured by the count of bytes sent, and that
    /// count when it began.
    metered: Option<(Kind, Option<String>, u64)>,
}

impl Tracker {
    pub fn new(remembered: Remembered) -> Self {
        Self {
            inner: std::sync::Arc::new(std::sync::Mutex::new(Tracked {
                estimator: Estimator::new(remembered),
                preview_bytes: None,
                metered: None,
            })),
            began: std::time::Instant::now(),
        }
    }

    fn now(&self) -> f64 {
        self.began.elapsed().as_secs_f64()
    }

    fn with<T>(&self, f: impl FnOnce(&mut Tracked, f64) -> T) -> T {
        let now = self.now();
        let mut tracked = self.inner.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        tracked.estimator.set_paused(crate::pause::is_paused(), now);
        f(&mut tracked, now)
    }

    pub fn plan(&self, planned: Vec<Planned>) {
        self.with(|t, _| t.estimator.plan(planned));
    }

    pub fn set_work(&self, kind: Kind, to: Option<&str>, work: u64) {
        self.with(|t, _| t.estimator.set_work(kind, to, work));
    }

    pub fn progress(&self, kind: Kind, to: Option<&str>, done: u64) {
        self.with(|t, now| t.estimator.progress(kind, to, done, now));
    }

    pub fn finish(&self, kind: Kind, to: Option<&str>) {
        self.with(|t, now| {
            Self::read_meter(t, now);
            if t.metered
                .as_ref()
                .is_some_and(|(k, d, _)| *k == kind && d.as_deref() == to)
            {
                t.metered = None;
            }
            t.estimator.finish(kind, to, now);
        });
    }

    /// From now on this sending stage's progress is the bytes sent.
    pub fn metered(&self, kind: Kind, to: Option<&str>) {
        self.with(|t, now| {
            t.metered = Some((kind, to.map(str::to_string), crate::meter::totals().0));
            t.estimator.progress(kind, to, 0, now);
        });
    }

    fn read_meter(t: &mut Tracked, now: f64) {
        if let Some((kind, to, from)) = &t.metered {
            let done = crate::meter::totals().0.saturating_sub(*from);
            t.estimator.progress(*kind, to.as_deref(), done, now);
        }
    }

    pub fn estimate(&self) -> Estimate {
        self.with(|t, now| {
            Self::read_meter(t, now);
            t.estimator.estimate(now)
        })
    }

    /// Every stage that began, as it stands now, for the backup's history.
    pub fn record(&self) -> Vec<StageRecord> {
        self.with(|t, now| {
            Self::read_meter(t, now);
            t.estimator.record(now)
        })
    }

    /// Seconds since it began, by the clock, paused time included.
    pub fn running_seconds(&self) -> u64 {
        self.now().round() as u64
    }

    /// Seconds spent paused so far.
    pub fn paused_seconds(&self) -> u64 {
        self.with(|t, now| t.estimator.paused_seconds(now))
    }

    /// What this backup measured, for remembering.
    pub fn measured(&self) -> Remembered {
        self.with(|t, now| Remembered {
            preview_bytes_per_original: t.preview_bytes,
            ..t.estimator.measured(now)
        })
    }

    /// The bytes of previews one original makes, as best known.
    pub fn preview_bytes(&self) -> f64 {
        self.with(|t, _| {
            t.preview_bytes
                .unwrap_or_else(|| t.estimator.remembered.preview_bytes())
        })
    }

    /// The bytes of previews one original made, as measured now.
    pub fn note_preview_bytes(&self, per_original: f64) {
        if per_original.is_finite() && per_original > 0.0 {
            self.with(|t, _| t.preview_bytes = Some(per_original));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MB: u64 = 1024 * 1024;
    const GB: u64 = 1024 * MB;

    /// A backup of `bytes` to one folder, with `photos` previews to make.
    fn one_backup(bytes: u64, photos: u64) -> Estimator {
        let mut e = Estimator::new(Remembered::default());
        e.plan(vec![
            Planned::new(Kind::Placing, None, 1000),
            Planned::new(Kind::SendingOriginals, Some("the folder N:"), bytes),
            Planned::new(Kind::MakingPreviews, None, photos),
            Planned::new(Kind::SendingPreviews, Some("the folder N:"), photos * 400 * 1024),
            Planned::new(Kind::WritingIndex, Some("the folder N:"), 1),
        ]);
        e
    }

    /// Placing done at once, then originals at `rate` bytes a second from
    /// t=0 for `seconds`, sampled every second.
    fn send_for(e: &mut Estimator, from: f64, seconds: u64, rate: u64, start_done: u64) -> u64 {
        let mut done = start_done;
        for s in 0..=seconds {
            done = start_done + rate * s;
            e.progress(Kind::SendingOriginals, Some("the folder N:"), done, from + s as f64);
        }
        done
    }

    #[test]
    fn each_stage_is_recorded_with_its_time_work_and_sustained_speeds() {
        let mut e = one_backup(10 * GB, 100);
        e.progress(Kind::Placing, None, 0, 0.0);
        e.progress(Kind::Placing, None, 1000, 2.0);
        e.finish(Kind::Placing, None, 2.0);
        // Ten minutes at 10 MB a second, then a pause of five minutes, then
        // ten minutes at 30 MB a second.
        let to = Some("the folder N:");
        let mut done = 0;
        for s in 0..=600u64 {
            done = s * 10 * MB;
            e.progress(Kind::SendingOriginals, to, done, 2.0 + s as f64);
        }
        e.set_paused(true, 602.0);
        e.set_paused(false, 902.0);
        for s in 1..=600u64 {
            e.progress(Kind::SendingOriginals, to, done + s * 30 * MB, 902.0 + s as f64);
        }
        e.finish(Kind::SendingOriginals, to, 1502.0);
        // Previews barely begun, and never finished.
        e.progress(Kind::MakingPreviews, None, 3, 1502.0);

        let record = e.record(1502.0);
        let kinds: Vec<Kind> = record.iter().map(|r| r.kind).collect();
        assert_eq!(
            kinds,
            [Kind::Placing, Kind::SendingOriginals, Kind::MakingPreviews],
            "only stages that began, in the plan's order"
        );
        assert_eq!(
            (record[0].done, record[0].seconds, record[0].unit),
            (1000, 2, Unit::Files)
        );
        let sending = &record[1];
        assert_eq!(sending.to.as_deref(), to);
        assert_eq!(sending.seconds, 1200, "the five minutes paused are left out");
        assert_eq!(sending.done, 24_000 * MB);
        assert!((sending.per_second.unwrap() - 20.0 * MB as f64).abs() < MB as f64);
        let (slowest, fastest) = (sending.slowest.unwrap(), sending.fastest.unwrap());
        assert!((slowest - 10.0 * MB as f64).abs() < 0.5 * MB as f64, "{slowest}");
        assert!((fastest - 30.0 * MB as f64).abs() < 0.5 * MB as f64, "{fastest}");
        assert!(sending.finished && !record[2].finished);
        assert_eq!(record[2].slowest, None, "nothing sustained in under a minute");
        assert_eq!(e.paused_seconds(1502.0), 300);
    }

    #[test]
    fn re_estimating_suits_the_length_of_the_job() {
        // Five times an hour on a two-hour backup.
        assert_eq!(republish_every(2.0 * 3600.0), 720.0);
        // At least once an hour on a very long one.
        assert_eq!(republish_every(30.0 * 3600.0), 3600.0);
        // More often on a short one, never more than every ten seconds.
        assert_eq!(republish_every(600.0), 60.0);
        assert_eq!(republish_every(30.0), 10.0);
    }

    #[test]
    fn the_window_grows_with_the_job() {
        assert_eq!(long_window(10.0), 60.0);
        assert_eq!(long_window(3.0 * 3600.0), 3600.0);
        assert_eq!(long_window(30.0 * 60.0), 600.0);
        assert_eq!(short_window(3600.0), 600.0);
        assert_eq!(short_window(60.0), 15.0);
    }

    #[test]
    fn the_whole_backup_is_counted_not_only_the_originals() {
        let mut e = one_backup(10 * GB, 1000);
        e.finish(Kind::Placing, None, 0.0);
        // Previews made at 4 a second in the background, originals at 10 MB/s.
        for s in 0..=60u64 {
            e.progress(Kind::SendingOriginals, Some("the folder N:"), s * 10 * MB, s as f64);
            e.progress(Kind::MakingPreviews, None, s * 4, s as f64);
        }
        let est = e.estimate(60.0);
        let left = est.seconds_left.unwrap() as f64;
        // Originals: (10 GB - 600 MB) at 10 MB/s = 964 s. Previews made
        // long before then (940 left at 4/s = 235 s). Then the previews
        // are sent at half the originals' speed (400 MB at 5 MB/s = 80 s)
        // and the index written (5 s, guessed).
        assert!((left - (964.0 + 80.0 + 5.0)).abs() < 30.0, "{left}");
        let states: Vec<State> = est.stages.iter().map(|s| s.state).collect();
        assert_eq!(
            states,
            [State::Done, State::Now, State::Alongside, State::Next, State::Next]
        );
        assert_eq!(est.stages[2].line, "Making previews in the background: 240 of 1,000");
        assert_eq!(
            est.stages[1].line,
            "Backing up your files: 600.0 MB of 10.0 GB at 10.0 MB/s"
        );
        assert_eq!(est.stages[3].line, "Backing up the previews: 390.6 MB");
    }

    #[test]
    fn slow_previews_hold_up_their_sending_and_the_overlap_is_counted() {
        let mut e = one_backup(GB, 10_000);
        e.finish(Kind::Placing, None, 0.0);
        for s in 0..=60u64 {
            e.progress(Kind::SendingOriginals, Some("the folder N:"), s * 10 * MB, s as f64);
            e.progress(Kind::MakingPreviews, None, s, s as f64);
        }
        let left = e.estimate(60.0).seconds_left.unwrap() as f64;
        // Previews take 9,940 s more, far beyond the originals' 42 s, and
        // the previews (3.8 GB at 5 MB/s, 781 s) are sent after them.
        assert!((left - (9940.0 + 781.0 + 5.0)).abs() < 60.0, "{left}");
    }

    #[test]
    fn between_re_estimates_the_time_left_counts_down_smoothly() {
        let mut e = one_backup(100 * GB, 0);
        e.finish(Kind::Placing, None, 0.0);
        e.set_work(Kind::SendingPreviews, Some("the folder N:"), 0);
        let done = send_for(&mut e, 0.0, 600, 10 * MB, 0);
        let first = e.estimate(600.0).seconds_left.unwrap();
        // A little faster for a while, not enough to count as a change.
        send_for(&mut e, 600.0, 60, 12 * MB, done);
        let later = e.estimate(660.0).seconds_left.unwrap();
        assert_eq!(first - later, 60, "it counted down by the minute that passed");
    }

    #[test]
    fn a_lasting_change_in_speed_is_acted_on_at_once_and_a_blip_is_not() {
        let mut e = one_backup(200 * GB, 0);
        e.finish(Kind::Placing, None, 0.0);
        e.set_work(Kind::SendingPreviews, Some("the folder N:"), 0);
        // An hour at 20 MB/s.
        let mut done = send_for(&mut e, 0.0, 3600, 20 * MB, 0);
        let before = e.estimate(3600.0).seconds_left.unwrap();
        // A burst for a few seconds changes nothing.
        done = send_for(&mut e, 3600.0, 5, 200 * MB, done);
        let blip = e.estimate(3605.0).seconds_left.unwrap();
        assert_eq!(before - blip, 5);
        // The network slows to a quarter and stays there.
        let mut t = 3605.0;
        let mut acted = None;
        for _ in 0..900 {
            t += 1.0;
            done += 5 * MB;
            e.progress(Kind::SendingOriginals, Some("the folder N:"), done, t);
            let left = e.estimate(t).seconds_left.unwrap();
            if left as f64 > (before as f64) * 1.5 {
                acted = Some(t - 3605.0);
                break;
            }
        }
        let after = acted.expect("the slower speed was acted on");
        // An hour in, the long window is 20 minutes and the short one 200
        // seconds. The short one must see the change, and the change must
        // last that long again: acted on within about seven minutes, well
        // before the twelve-minute re-estimate would have come round.
        assert!((200.0..=450.0).contains(&after), "{after}");
    }

    #[test]
    fn a_stall_is_said_plainly_and_a_short_pause_for_breath_is_not() {
        let mut e = one_backup(10 * GB, 0);
        e.finish(Kind::Placing, None, 0.0);
        let done = send_for(&mut e, 0.0, 120, 10 * MB, 0);
        // Thirty seconds of nothing: still an estimate.
        e.progress(Kind::SendingOriginals, Some("the folder N:"), done, 150.0);
        assert!(e.estimate(150.0).seconds_left.is_some());
        // Nothing for the whole long window: it says so.
        e.progress(Kind::SendingOriginals, Some("the folder N:"), done, 400.0);
        let est = e.estimate(400.0);
        assert_eq!(est.sureness, Sureness::Stalled);
        assert_eq!(est.seconds_left, None);
        assert!(est.left.starts_with("Nothing has moved"), "{}", est.left);
    }

    #[test]
    fn a_stage_not_started_borrows_a_rate_this_run_then_earlier_runs_then_a_guess() {
        // Nothing measured, nothing remembered: a guess, and too much
        // rests on it to say a time.
        let mut e = one_backup(10 * GB, 100);
        e.progress(Kind::Placing, None, 10, 1.0);
        assert_eq!(e.estimate(1.0).sureness, Sureness::Unknown);
        assert_eq!(e.estimate(1.0).left, "Working out how long this will take");

        // Remembered from an earlier backup on this computer: an answer.
        let mut remembered = Remembered::default();
        remembered
            .rates
            .insert("sendingOriginals:the folder N:".into(), (10 * MB) as f64);
        remembered.rates.insert("makingPreviews".into(), 5.0);
        remembered.rates.insert("writingIndex".into(), 1.0);
        let mut e2 = Estimator::new(remembered);
        e2.plan(vec![
            Planned::new(Kind::SendingOriginals, Some("the folder N:"), 10 * GB),
            Planned::new(Kind::MakingPreviews, None, 100),
            Planned::new(Kind::WritingIndex, Some("the folder N:"), 1),
        ]);
        let est = e2.estimate(0.0);
        assert_eq!(est.sureness, Sureness::Fair);
        assert_eq!(est.seconds_left, Some(1025));

        // Previews to a second destination use those measured to the first.
        let mut e3 = Estimator::new(Remembered::default());
        e3.plan(vec![
            Planned::new(Kind::SendingPreviews, Some("a"), 100 * MB),
            Planned::new(Kind::SendingPreviews, Some("b"), 100 * MB),
        ]);
        for s in 0..=30u64 {
            e3.progress(Kind::SendingPreviews, Some("a"), s * MB, s as f64);
        }
        let (rate, source) = e3.rate_for(1, 30.0);
        assert_eq!(source, Source::ThisRun);
        assert!((rate.unwrap() - MB as f64).abs() < 1.0);
    }

    #[test]
    fn a_minor_stage_resting_on_a_guess_makes_it_rough() {
        let mut e = one_backup(GB, 1000);
        e.finish(Kind::Placing, None, 0.0);
        send_for(&mut e, 0.0, 30, MB, 0);
        // Originals measured; previews (500 s at the guess) still guessed.
        let est = e.estimate(30.0);
        assert_eq!(est.sureness, Sureness::Rough);
        assert!(est.left.starts_with("Roughly "), "{}", est.left);
    }

    #[test]
    fn paused_time_stands_still() {
        let mut e = one_backup(10 * GB, 0);
        e.finish(Kind::Placing, None, 0.0);
        e.set_work(Kind::SendingPreviews, Some("the folder N:"), 0);
        send_for(&mut e, 0.0, 120, 10 * MB, 0);
        let before = e.estimate(120.0).seconds_left.unwrap();
        e.set_paused(true, 120.0);
        let paused = e.estimate(1000.0);
        assert_eq!(paused.seconds_left.unwrap(), before, "no time passes while paused");
        assert_eq!(paused.paused_seconds, 880);
        e.set_paused(false, 1000.0);
        e.progress(Kind::SendingOriginals, Some("the folder N:"), 121 * 10 * MB, 1001.0);
        let after = e.estimate(1001.0);
        assert!(after.seconds_left.unwrap() + 2 >= before - 1);
    }

    #[test]
    fn previews_all_made_while_the_originals_are_still_going_are_shown_as_done() {
        let mut e = one_backup(GB, 1000);
        e.finish(Kind::Placing, None, 0.0);
        e.progress(Kind::SendingOriginals, Some("the folder N:"), GB / 10, 10.0);
        e.progress(Kind::MakingPreviews, None, 500, 10.0);
        let half = e.estimate(11.0);
        let previews = |est: &Estimate| {
            est.stages
                .iter()
                .find(|s| s.kind == Kind::MakingPreviews)
                .map(|s| s.state)
        };
        assert_eq!(previews(&half), Some(State::Alongside));
        e.progress(Kind::MakingPreviews, None, 1000, 20.0);
        assert_eq!(previews(&e.estimate(21.0)), Some(State::Done), "every preview is made");
    }

    #[test]
    fn progress_covers_the_whole_backup_and_never_reaches_the_end_early() {
        let mut e = one_backup(GB, 1000);
        e.finish(Kind::Placing, None, 0.0);
        let done = send_for(&mut e, 0.0, 99, 10 * MB, 0);
        e.progress(Kind::MakingPreviews, None, 50, 99.0);
        e.progress(Kind::SendingOriginals, Some("the folder N:"), done.max(GB), 100.0);
        e.finish(Kind::SendingOriginals, Some("the folder N:"), 100.0);
        let est = e.estimate(100.0);
        // The originals are all there, but the previews are not: far from done.
        assert!(est.fraction_done < 0.6, "{}", est.fraction_done);
        for k in [Kind::MakingPreviews, Kind::SendingPreviews, Kind::WritingIndex] {
            e.finish(
                k,
                if k == Kind::MakingPreviews {
                    None
                } else {
                    Some("the folder N:")
                },
                200.0,
            );
        }
        assert_eq!(e.estimate(200.0).fraction_done, 1.0);
    }

    #[test]
    fn remembered_rates_move_half_way_and_ignore_nonsense() {
        let mut old = Remembered::default();
        old.rates.insert("makingPreviews".into(), 4.0);
        let mut new = Remembered::default();
        new.rates.insert("makingPreviews".into(), 8.0);
        new.rates.insert("placing".into(), 100.0);
        new.preview_bytes_per_original = Some(300_000.0);
        let both = old.updated_with(&new);
        assert_eq!(both.rates["makingPreviews"], 6.0);
        assert_eq!(both.rates["placing"], 100.0);
        assert_eq!(both.preview_bytes(), 300_000.0);
        let mut bad = Remembered::default();
        bad.rates.insert("placing".into(), f64::NAN);
        assert_eq!(bad.rate(Kind::Placing, None), None);
        assert_eq!(Remembered::default().preview_bytes(), GUESSED_PREVIEW_BYTES);
        // What a run measured is offered back for remembering.
        let mut e = one_backup(10 * GB, 0);
        send_for(&mut e, 0.0, 120, 10 * MB, 0);
        let measured = e.measured(120.0);
        assert!((measured.rates["sendingOriginals:the folder N:"] - (10 * MB) as f64).abs() < 1000.0);
    }

    #[test]
    fn time_left_is_said_finer_when_close_and_rounder_when_far_or_rough() {
        assert_eq!(span(30), "less than a minute");
        assert_eq!(span(12 * 60 + 10), "about 12 minutes");
        assert_eq!(span(60), "about 1 minute");
        assert_eq!(span(37 * 60), "about 35 minutes");
        assert_eq!(span(100 * 60), "about 1 hour 45 minutes");
        assert_eq!(span(6 * 3600 + 1200), "about 6 hours");
        assert_eq!(span(26 * 3600), "about a day");
        assert_eq!(span(80 * 3600), "about 3 days");
        assert_eq!(rough_span(5 * 60), "a few minutes");
        assert_eq!(rough_span(37 * 60), "40 minutes");
        assert_eq!(rough_span(100 * 60), "1 hour 30 minutes");
        assert_eq!(rough_span(6 * 3600 + 1200), "6 hours");
        assert_eq!(left_in_words(Some(720), Sureness::Fair), "About 12 minutes left");
        assert_eq!(left_in_words(Some(30), Sureness::Fair), "Less than a minute left");
        assert_eq!(left_in_words(Some(6 * 3600), Sureness::Rough), "Roughly 6 hours left");
        assert_eq!(
            left_in_words(None, Sureness::Fair),
            "Working out how long this will take"
        );
    }

    #[test]
    fn how_long_it_has_run_is_said_to_the_minute_with_pauses_owned_up_to() {
        assert_eq!(running_for(20), "less than a minute");
        assert_eq!(running_for(5 * 3600 + 12 * 60 + 40), "5 hours 12 minutes");
        assert_eq!(running_for(27 * 3600), "1 day 3 hours");
        assert_eq!(running_for(48 * 3600), "2 days");
        let mut e = one_backup(10 * GB, 0);
        e.finish(Kind::Placing, None, 0.0);
        e.set_work(Kind::SendingPreviews, Some("the folder N:"), 0);
        send_for(&mut e, 0.0, 600, 10 * MB, 0);
        let est = e.estimate(600.0);
        assert_eq!(
            headline(5 * 3600 + 12 * 60, &est),
            "Running for 5 hours 12 minutes, about 7 minutes left"
        );
        e.set_paused(true, 600.0);
        let est = e.estimate(1800.0);
        assert_eq!(
            headline(3 * 3600, &est),
            "Running for 3 hours (20 minutes of it paused), about 7 minutes left"
        );
        let unknown = one_backup(GB, 0).estimate(0.0);
        assert_eq!(
            headline(30, &unknown),
            "Running for less than a minute. Working out how long this will take."
        );
    }

    #[test]
    fn the_whole_estimate_can_be_written_as_lines() {
        let mut e = one_backup(10 * GB, 1000);
        e.finish(Kind::Placing, None, 0.0);
        send_for(&mut e, 0.0, 60, 10 * MB, 0);
        e.progress(Kind::MakingPreviews, None, 240, 60.0);
        let mut est = e.estimate(60.0);
        est.headline = headline(65, &est);
        let lines = in_lines(&est);
        assert!(lines[0].starts_with("Running for 1 minute, "), "{lines:?}");
        assert_eq!(lines[1], "Done: Working out where each file goes");
        assert!(lines[2].starts_with("Now: Backing up your files: "));
        assert_eq!(lines[3], "Now: Making previews in the background: 240 of 1,000");
        assert_eq!(lines[4], "Then: Backing up the previews: 390.6 MB");
        assert_eq!(lines[5], "Then: Writing the list of what is there");
    }

    #[test]
    fn numbers_are_written_as_the_app_writes_them() {
        assert_eq!(size(512), "512 bytes");
        assert_eq!(size(700 * GB), "700.0 GB");
        assert_eq!(size(7 * MB + MB / 2), "7.5 MB");
        assert_eq!(thousands(10_547), "10,547");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(1_000_000), "1,000,000");
    }

    #[test]
    fn several_destinations_each_get_their_name() {
        let mut e = Estimator::new(Remembered::default());
        e.plan(vec![
            Planned::new(Kind::SendingOriginals, Some("the folder A:"), GB),
            Planned::new(Kind::SendingOriginals, Some("the bucket b"), GB),
        ]);
        let views = e.estimate(0.0).stages;
        assert_eq!(views[0].line, "Backing up your files to the folder A:: 1.0 GB");
        assert_eq!(views[1].to.as_deref(), Some("the bucket b"));
    }

    #[test]
    fn the_tracker_follows_the_bytes_sent() {
        let tracker = Tracker::new(Remembered::default());
        tracker.plan(vec![Planned::new(Kind::SendingOriginals, Some("x"), 10 * GB)]);
        tracker.metered(Kind::SendingOriginals, Some("x"));
        crate::meter::sent(5 * MB, false);
        let est = tracker.estimate();
        assert!(est.stages[0].done >= 5 * MB);
        assert_eq!(est.stages[0].state, State::Now);
        tracker.finish(Kind::SendingOriginals, Some("x"));
        assert_eq!(tracker.estimate().stages[0].state, State::Done);
        assert!(tracker.measured().rates.is_empty(), "too short to remember");
        assert_eq!(tracker.preview_bytes(), GUESSED_PREVIEW_BYTES);
        tracker.note_preview_bytes(300_000.0);
        tracker.note_preview_bytes(f64::NAN);
        assert_eq!(tracker.preview_bytes(), 300_000.0);
        assert_eq!(tracker.measured().preview_bytes_per_original, Some(300_000.0));
    }
}
