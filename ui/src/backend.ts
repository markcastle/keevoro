// Everything the screens ask of the engine, in one place.
//
// The screens never talk to Tauri directly. They call the functions here,
// which reach the Rust side when the app is running for real, and a
// stand-in (demo.ts) when the screens are opened in an ordinary browser.
// That is what lets the screens be developed and tested with `npm run
// ui:dev` and no desktop shell at all.
//
// The shapes below mirror the Rust structs in crates/keevoro-engine
// (discover.rs) and src-tauri/src/commands.rs. If one changes, change both.

import { demoBackend } from './demo'
import { PictureQueue, fileKey, photoKey } from './pictureQueue'

export type Photo = {
  id: string
  isVideo: boolean
  /** An other file carried along: shown by its name and kind, never as a picture. */
  isOther?: boolean
  name: string
  sourcePath: string
  thumbnail: string | null
}

export type Collection = {
  key: string
  title: string
  summary: string
  parents: string[]
  photos: number
  videos: number
  /** Other files in it; only a folder of other files has any. */
  others?: number
  /** A folder of other files, listed under "Other files" by the folder it came from. */
  byFolder?: boolean
  /** For a folder of other files, the room its files take. */
  bytes?: number
  /** A folder of photos and videos from a source that keeps its folders: shown as it will be in the backup. */
  keptFolders?: boolean
  kept: boolean
  gathered: number
  cover: Photo
  previews: Photo[]
}

/** One file inside a collection, or one picture set aside as junk. */
export type PhotoChoice = Photo & {
  taken: string | null
  kept: boolean
  junkReason: string | null
  /** For an other file: the day it last changed, and its size. */
  changed?: string
  bytes?: number
}

/**
 * A file found more than once. The photo itself is the copy being kept;
 * `copies` are the others, which are not sent because they are the same
 * file. `confirmed` is false while that is still being checked.
 */
export type Duplicate = Photo & {
  folder: string
  bytes: number
  confirmed: boolean
  copies: { name: string; folder: string }[]
}

/** A folder a survey suggests looking through, everything beneath it counted in. */
export type Hint = {
  folder: string
  photos: number
  videos: number
  /** Other files a person put on the kinds list. */
  others?: number
  bytes: number
  /** False when the pictures are so small they are probably icons, not photos. */
  likelyPhotos: boolean
  alreadyASource: boolean
  /** Of its photos and videos, those only in online storage (OneDrive), which a search passes over unopened. */
  onlineOnly?: number
  /** The same in words, with how to include them. */
  onlineOnlySaid?: string
}

/** What a survey of a drive found. Counts are by file name alone. */
export type Survey = {
  hints: Hint[]
  folders: number
  files: number
  unreadable: number
  seconds: number
  stopped: boolean
  /** Folders Keevoro made that were left out, each with what is said. */
  leftOut?: LeftOut[]
  /** Photos and videos only in online storage, everywhere it looked, and that in words. */
  onlineOnly?: number
  onlineOnlySaid?: string
}

/** A folder Keevoro made (a backup, an old library, its own files), left out of a search or survey. */
/** A folder a search or survey left out: one Keevoro made, or one the person left out (which can be put back). */
export type LeftOut = { folder: string; what: 'keevoroBackup' | 'keevoroOwn' | 'oldLibrary' | 'leftOutByYou'; said: string }

/** What leaving a folder out, or putting it back, did, in words that say nothing backed up is touched. */
export type LeftOutFolder = { folder: string; leftOut: boolean; files: number; stillInside?: string; said: string }

export type SurveyProgress = {
  folders: number
  files: number
  photos: number
  videos: number
  /** Other files a person put on the kinds list, seen so far. */
  others?: number
  sizing?: boolean
  /** The top of the map as it grows: at most twice a second, the biggest first. */
  top?: TopBox[]
}

/** One box at the top of the map while a survey walks, named as a box of the map is. */
export type TopBox = {
  name: string
  /** A folder, the files directly in the place (`files`), or the smaller folders together. */
  node: 'folder' | 'files' | 'smaller'
  bytes: number
  photos: number
  videos: number
  others: number
  photoBytes: number
  videoBytes: number
  otherBytes: number
  /** Other files a person put on the kinds list, to be carried along. */
  carried?: number
  carriedBytes?: number
  /** For the smaller folders together, how many. */
  folders?: number
}

/** A drive plugged into this computer, offered as somewhere to look. */
export type Place = {
  path: string
  name: string
  detail: string
  removable: boolean
  /** A whole disk big enough to ask before looking through all of it. */
  askFirst: boolean
  /** The cloud service that keeps this folder in step, for a synced folder rather than a drive. */
  synced?: 'oneDrive' | 'dropbox' | 'googleDrive' | 'iCloud'
}

/**
 * One kind of file found on a source, by the ending of its name (`jpg`,
 * `dng`, `mp4`), and whether the person is including it.
 */
export type FileType = {
  ending: string
  files: number
  bytes: number
  /** False when this ending is left out of this source. */
  included: boolean
  /** When these files are not offered, the level they were left out at. */
  leftOutBy?: 'backup' | 'source' | 'ending'
  /** The same, in words: "this backup is for photos only". */
  why?: string
}

/** One thing a backup or a source can be for. */
export type WantsWord = 'photos' | 'videos' | 'others'

/** What a backup or a source is for: one or more of photos, videos and other files. */
export type Wants = WantsWord[]

/** What a backup or a source is for, and what that means, in words. */
export type WantsView = {
  wants: Wants
  /** For a source: true when it has its own choice rather than the backup's. */
  own?: boolean
  /** True once a kind of other file is on the list, so "Other files" is a choice. */
  offersOthers?: boolean
  said: string
}

export type Source = {
  id: number
  name: string
  root: string
  items: number
  /** The size of everything found on it, in bytes. */
  bytes: number
  /**
   * Files there that could be photos or videos but were only in online
   * storage (OneDrive) when it was last looked through, so were not opened,
   * and that in words with how to include them.
   */
  onlineOnly?: number
  onlineOnlySaid?: string
  /** How big those files are together, from each one's listing; left out when not known. */
  onlineOnlyBytes?: number
  /** Where it is in words, when it is cloud storage: "the Amazon S3 bucket family-photos". */
  bucket?: string
  /** Folders left out of this backup that are in it, or that it is in. */
  leftOut?: string[]
  /** Its own way of arranging its photos and videos; left out when it follows the backup. */
  arrange?: Arrange
  /** The folder its files are kept in at the top of the backup, when it keeps its folders. */
  keptAs?: string
  /** Other sources whose folders go into that same folder, and that in words. */
  sharesFolderWith?: string[]
  sharedSaid?: string
  /** Set aside when a different drive was plugged in at its place: that place, and this in words. Everything found on it is kept. */
  setAsideFrom?: string
  setAsideSaid?: string
}

/** How photos and videos are arranged in a backup. */
export type Arrange = 'byDate' | 'folders'

/** An arrangement worth suggesting, and why, in words. */
export type ArrangeSuggestion = { arrange: Arrange; said: string }

/** How a backup is arranged, whether the person was asked, and what its files suggest. */
export type ArrangeView = {
  arrange: Arrange
  /** True once chosen, or once anything was sent: the first-time question is not asked again. */
  asked: boolean
  said: string
  suggestion?: ArrangeSuggestion
  /** Its sources suggest different ways. */
  sourcesDiffer?: boolean
  /** The folder each source's files would start in, kept as they are. */
  topFolders?: string[]
}

/** How one source's photos and videos are arranged. */
export type SourceArrangeView = {
  arrange: Arrange
  /** True when it has its own choice, false when it follows the backup. */
  own: boolean
  keptAs?: string
  sharesFolderWith?: string[]
  said: string
  /** What its files suggest, when that is not what it does now. */
  suggestion?: ArrangeSuggestion
}

export type Discover = {
  photos: number
  videos: number
  /** Other files a person asked Keevoro to carry along. */
  others?: number
  bytes: number
  duplicates: number
  /** The room those extra copies would have taken: what leaving them out saves. */
  duplicateBytes: number
  junk: number
  waitingToGather: number
  /** How much room those waiting files need, in bytes. */
  waitingBytes: number
  sources: Source[]
  /** Each place this backup goes to, with what has reached it. */
  destinations: Destination[]
  collections: Collection[]
}

/**
 * One place a backup goes to. A backup can go to several, and a photo is
 * only safe once it is at all of them, so each is counted on its own.
 */
export type Destination = {
  /** In words: "the folder F:\\Family Backup". */
  name: string
  kind: string
  folder: string | null
  /** Kept files there, as the last look found; never counted from memory alone. */
  there: number
  waiting: number
  waitingBytes: number
  /** What Keevoro found when it last looked there. Absent before it has looked. */
  looked?: Looked
}

/** What the last look at a destination found, as keevoro_engine::confirm::LookedView says it. */
/** What putting a backup right would do at one destination (keevoro_engine::put_right::Plan). */
export type PutRightPlace = {
  to: string
  known: number
  knownBytes: number
  files: number
  bytes: number
  provedAlready: number
  quickSeconds: number
  thoroughSeconds: number
  setsAside: boolean
  said: string[]
}

/** `backup.put_right_plan`: what "Put this backup right" would do, and about how long. */
export type PutRightPlan = {
  places: PutRightPlace[]
  known: number
  quickSeconds: number
  thoroughSeconds: number
  quickSaid: string
  thoroughSaid: string
}

export type Looked = {
  /** When Keevoro last looked, in seconds since 1970. */
  at: number
  /** When it last reached it; null when it never has. */
  lastReached: number | null
  /** Looked at and reached a moment ago: only then is what is there certain. */
  fresh: boolean
  reached: boolean
  /** Originals sent there that were missing, and those another size, when it looked. */
  gone: number
  notAsSent: number
  /** What to say about it, a sentence each, in Keevoro's own words. */
  said: string[]
  /** Its folder is on another disk now and holds nothing of a backup: the drive seems unplugged, leaving its folder behind. */
  otherDisk?: boolean
}

/**
 * One job the person comes back to, such as "Drone footage": its own
 * sources, its own choices and its own place to send things.
 */
export type Backup = { id: string; name: string }

/** One backup in full, as Manage backups lists it: what tells it apart from the others. */
export type BackupListing = {
  id: string
  name: string
  /** Its own folder on this computer, which holds Keevoro's record of it. */
  home: string
  open: boolean
  /** What it holds; null when its folder is not there (on a drive that is unplugged). */
  found: BackupCounts | null
  /** Why it cannot be renamed or removed right now: it is being worked on. */
  busy?: string
}

export type BackupCounts = {
  sources: number
  destinations: number
  photos: number
  videos: number
  others?: number
  waiting: number
  waitingBytes: number
  bytes: number
  /** Where each source is. */
  sourcePlaces: string[]
  /** Where it backs up to, in words. */
  destinationNames: string[]
  /** When it last backed up, in seconds since 1970. */
  lastRan?: number
}

/** What removing a backup does, asked first or once done, in sentences. Nothing at a destination or source is touched. */
export type Removal = {
  id: string
  name: string
  removed: boolean
  sources: string[]
  destinations: string[]
  ownFolder: string
  ownFolderBytes: number
  clearsOwnFolder: boolean
  keysFor: string[]
  /** Once removed: when it is cleared away for good unless put back, in seconds since 1970. */
  keptUntil?: number
  said: string[]
}

/** A backup removed in the last week, which can still be put back. */
export type RemovedBackup = { id: string; name: string; removedAt: number; keptUntil: number; bytes: number; said: string }

/** The drives plugged in, given when they changed: the quick list to give back, every drive, and those just arrived. */
export type PlacesNow = { mounts: string[]; places: Place[]; arrived: string[] }

export type AppState = {
  /** The open backup's own folder, or null before the first backup. */
  library: string | null
  /** The backup that is open. */
  backup: Backup | null
  /** Every backup, in the order they were started. */
  backups: Backup[]
  version: string
}

/** Sent while a drive is being looked through, and once more when done. */
export type ScanEvent = {
  name: string
  photos: number
  videos: number
  /** Other files a person put on the kinds list. */
  others?: number
  /** Every file looked at so far, photos or not. */
  files?: number
  /** The folder being looked in right now. */
  folder?: string
  /** A large file being read right now: where it is, and its size in bytes. */
  reading?: [string, number]
  /** True once looking is over and what was found is being saved. */
  saving?: boolean
  /** On the last event: the search was stopped, so nothing from it was kept. */
  stopped?: boolean
  finished: boolean
  problem?: string
  /** On the last event: how many files could not be read, and the first few by name. */
  unreadable?: number
  unreadableNames?: string[]
  /** On the last event: folders Keevoro made that were left out, each with what is said. */
  leftOut?: LeftOut[]
  /** On the last event: files only in online storage (OneDrive), passed over unopened, and that in words. */
  onlineOnly?: number
  onlineOnlySaid?: string
  /** On the last event: the drive was unplugged part-way, in a sentence. What was found so far is kept. */
  unplugged?: string
  /** On the last event of looking again: sources not looked through, as they are not plugged in or were set aside, each in a sentence. */
  passedOver?: string[]
  /** On the last event: sources whose place holds a different drive now, so they were not looked through, each with the question to ask. */
  differentDrive?: DifferentDrive[]
}

/** How a backup's schedule is kept: every day, week or month, at a time in this computer's own time. */
export type ScheduleChoice = {
  v: number
  every: 'day' | 'week' | 'month'
  /** "02:30" */
  at: string
  /** For a week: mon to sun. */
  weekday?: string
  /** For a month: 1 to 31, the last day of shorter months. */
  day?: number
  paused: boolean
}

/** One run by itself, or one passed over. */
export type ScheduleAttempt = {
  at: number
  /** "on its schedule", "when you pressed Run now". */
  why: string
  outcome: 'worked' | 'someFiles' | 'unreachable' | 'refused' | 'stopped' | 'heldBack' | 'notNow' | 'skipped'
  said: string
}

/** When a backup backs up by itself, when next, and its last few runs. */
export type ScheduleView = {
  /** "Every day at 02:30"; absent when it has no schedule. */
  every?: string
  choice?: ScheduleChoice
  paused: boolean
  /** "Tonight at 02:30" */
  next?: string
  nextAt?: number
  skipping: boolean
  last: ScheduleAttempt[]
  /** A schedule this version could not read: it is kept, and counts as none. */
  unreadable?: boolean
}

/** What to set: `every` off for none; only `paused` to pause or carry on. */
export type SetSchedule = {
  every?: 'day' | 'week' | 'month' | 'off'
  at?: string
  weekday?: string
  day?: number
  paused?: boolean
}

/** What pressing Update did: started installing, or waits for the work in hand to finish, said in `waiting`. */
export type UpdateStarted = { waiting?: string }

/** A source whose place holds a different drive from the one looked through there before. */
export type DifferentDrive = {
  source: number
  name: string
  place: string
  /** "This looks like a different drive from the one called Card. Add it as a new source, or look through it as Card anyway?" */
  asked: string
  /** What happened, said when nobody answers: it was passed over and nothing found on it is lost. */
  said: string
}

/** The answer to that question. */
export type OtherDrive = 'lookAnyway' | 'addAsNew'

/** Sent while files a quick search only sampled are read in the background. */
export type CheckEvent = {
  done: number
  total: number
  finished: boolean
  /** On the last event: a person stopped it, so it does not start again by itself until the next search or backup. */
  stopped?: boolean
  /** On the last event: how many files are still to be checked. */
  left?: number
  /** On the last event: what that means, a sentence each, when a person stopped it; and each source not plugged in whose files wait for it. */
  said?: string[]
}

/** How the check for duplicates stands when none is running. */
export type DuplicatesLeft = { left: number; stopped: boolean; said?: string[] }

/** The check for duplicates running now, as the activity page shows it. */
export type CheckNow = { job: number; done: number; total: number; pausedAlone: boolean; said: string }

/** Sent while a backup or a check runs, and once more when it is done. */
export type SendEvent = {
  line?: string
  finished: boolean
  summary?: string[]
  problem?: string
  /** On the last event: how it ended, in a word. */
  outcome?: 'worked' | 'someFiles' | 'unreachable' | 'refused' | 'stopped'
  /** On the last event of a backup that ran by itself: what waits for your say, a sentence each. */
  heldBack?: string[]
  /** On the last event of a backup that ran by itself and did not run now, why. */
  notNow?: string
  /** On the last event of a backup: how many files it could not back up. */
  missed?: number
  /** On the last event of a backup during which a destination's connection was lost: offer "Check they all arrived" first. */
  checkAdvised?: boolean
}

/** What the "connect" form collects. Keys go to the system keychain. */
export type ConnectRequest = {
  kind: 'folder' | 'r2' | 's3' | 'endpoint'
  /** The folder, Cloudflare account ID, Amazon region or server address. */
  place: string
  bucket?: string
  accessKeyId?: string
  secretAccessKey?: string
}

/**
 * How much room the waiting files need, and what is known about where they
 * are going. `free` is only known for a folder; `enough` is false only when
 * the files are known not to fit.
 */
export type Room = {
  needed: number
  folder: string | null
  free: number | null
  enough: boolean
  /** About what backing up now would cost to read out of each cloud storage source, a sentence each. */
  cost?: string[]
}

/** What the form for cloud storage as a source sends. The keys go to the keychain and are never given back. */
export type BucketSourceRequest = {
  kind: 'r2' | 's3' | 'endpoint'
  /** The Cloudflare account ID, Amazon region or server address. */
  place: string
  bucket?: string
  /** A folder in the bucket; the whole bucket when empty. */
  prefix?: string
  accessKeyId?: string
  secretAccessKey?: string
}

/** Cloud storage added as a source, with where to look through it. */
export type BucketAdded = { id: number; name: string; root: string; place: string; keys?: string }

/** One second of Keevoro's own use of the computer. */
export type Sample = {
  /** Share of the whole computer's processor, 0 to 100. */
  cpu: number
  /** Megabytes read from and written to disk in that second. */
  disk: number
  /** Megabytes of your files that arrived where they were sent, in that second. */
  sent: number
  /** The part of `sent` that went over the network, to cloud storage. */
  network: number
  /** Megabytes of memory Keevoro is holding at that moment. */
  memory: number
}

/** How one finished send went. */
export type Sent = {
  /** When it finished, in seconds since 1970. */
  finishedAt: number
  /** How long it took, from pressing Send to the end. */
  seconds: number
  bytes: number
  /** Where it went, in words. */
  to: string
  stopped: boolean
  /** Everything else measured; missing for a backup recorded before it was kept. */
  detail?: RunDetail
}

/** Everything measured about one backup. Mirrors `RunDetail` in keevoro-engine's history. */
export type RunDetail = {
  startedAt: number
  pausedSeconds: number
  ended: 'finished' | 'stopped' | 'problem'
  problem?: string
  /** "the app", "the command line" or "an AI agent". */
  startedBy: string
  version: string
  destinations: DestinationRecord[]
  stages: StageRecord[]
  duplicates: number
  duplicateBytes: number
  /** Missing when the backup sends the originals alone. */
  previews?: PreviewsMade
  /** What it said at the end, exactly as it was shown. */
  summary: string[]
  /** For a backup that ran by itself: why, what it looked through and passed over, what was new, and what waits for your say. */
  byItself?: ByItself
}

/** What a backup that ran by itself did before it sent anything. */
export type ByItself = {
  /** "on its schedule", "from the command line". */
  why: string
  lookedAgain?: string[]
  passedOver?: string[]
  found?: string[]
  waiting?: string[]
}

export type StageKind =
  | 'tellingApart'
  | 'placing'
  | 'sendingOriginals'
  | 'makingPreviews'
  | 'makingVideoPreviews'
  | 'sendingPreviews'
  | 'writingIndex'

/** One stage: its time with pauses left out, how much it did and how fast. */
export type StageRecord = {
  kind: StageKind
  to?: string
  unit: 'bytes' | 'files'
  done: number
  seconds: number
  finished: boolean
  perSecond: number | null
  /** Held for a minute or more; null for a shorter stage. */
  slowest: number | null
  fastest: number | null
  /** What went wrong, in words, for a stage that ended with a problem. */
  problem?: string
}

export type DestinationRecord = {
  to: string
  overNetwork: boolean
  reached: boolean
  problem?: string
  seconds: number
  bytes: number
  originals: Sending
  previews?: Sending
  indexed?: number
  /** Keevoro's own unfinished working files cleared there; never anything backed up. */
  clearedFiles?: number
  clearedBytes?: number
}

export type Sending = {
  sent: number
  sentBytes: number
  alreadyThere: number
  notSent: { file: string; reason: string }[]
  notSentMore: number
}

export type PreviewsMade = { photos: number; videos: number; photosWithout: number; videosWithout: number }

/** How a send is getting on. */
export type SendProgress = {
  /** True while a send or a check is under way, even if this screen was not open when it began. */
  running: boolean
  /** True once it has been asked to stop and is finishing the file in hand. */
  stopping: boolean
  /** What it last said it was doing; empty when nothing is running. */
  line: string
  /** Seconds since it started, by the clock, from the job itself. */
  runningSeconds: number
  /** For a backup: how long the whole of it has left, stage by stage. Null for a check. */
  estimate: BackupEstimate | null
  overNetwork: boolean
}

/** One stage of a backup, as `keevoro_core::estimate::StageView` says it. */
export type BackupStage = {
  kind: 'tellingApart' | 'placing' | 'sendingOriginals' | 'makingPreviews' | 'makingVideoPreviews' | 'sendingPreviews' | 'writingIndex'
  /** Which destination, for work done once per destination. */
  to: string | null
  /** Done, gone wrong, what it is doing now, under way alongside in the background, or still to come. */
  state: 'done' | 'problem' | 'now' | 'alongside' | 'next'
  unit: 'bytes' | 'files'
  done: number
  work: number
  /** Per second, while it runs and once measured. */
  perSecond: number | null
  /** As shown: "Backing up your files: 708.4 GB of 716.8 GB at 7.5 MB/s". What went wrong is in it too. */
  line: string
  /** What went wrong, in words, when it did: "Couldn't back up 3 files". */
  problem?: string
}

/** How long until a whole backup is done, as the one estimator in `keevoro_core::estimate` says. */
export type BackupEstimate = {
  /** Null when there is no honest answer yet. */
  secondsLeft: number | null
  sureness: 'fair' | 'rough' | 'unknown' | 'stalled'
  /** "About 12 minutes left", "Roughly 6 hours left", "Working out how long this will take". */
  left: string
  /** How much of the whole backup is done, from 0 to 1. */
  fractionDone: number
  stages: BackupStage[]
  pausedSeconds: number
  /** "Running for 5 hours 12 minutes, about 3 hours left". */
  headline: string
}

/** What Keevoro is doing, why, and how hard it has been working. */
export type Activity = {
  paused: boolean
  /** What it is doing in one line, for a glance; null when idle. */
  doing: string | null
  /** The step a running backup is on, in its own words; null when none is running. */
  backupStep: string | null
  jobs: { what: string; why: string }[]
  /** One a second, oldest first, for the last two minutes at most. */
  samples: Sample[]
  memoryBytes: number
  cores: number
  /** The background check for duplicates, when one is running. */
  check?: CheckNow
}

/**
 * How Keevoro decodes photos on this computer: whether the computer's own
 * codecs are in use, whether it has any to use, the ways that will be
 * tried in order, and how many previews are made at once.
 */
export type Acceleration = { on: boolean; available: boolean; ways: string[]; cores: number }

/**
 * What an AI agent may do, as the person has allowed: the folders it may look
 * through and back up to, and whether it may make changes. Nothing until they say.
 */
export type AgentAccess = { lookIn: string[]; backUpTo: string[]; mayChange: boolean }

/**
 * One kind of file Keevoro looks for, by the ending of its name (`jpg`,
 * lower case, without the dot). Mirrors `FileKind` in keevoro-core.
 */
export type FileKind = {
  ending: string
  /** A photo, a video, or an other file carried along (backed up exactly, never shown or grouped by date). */
  kind: ListedAs
  /** Looked for. One switched off is passed over. */
  on: boolean
  /** Comes with Keevoro: it can be switched off, not removed or renamed. */
  builtIn: boolean
}

/** A change to one kind of file. Anything left out stays as it is. */
export type FileKindChange = { kind?: ListedAs; on?: boolean; to?: string }

/** What files with an ending are listed as. */
export type ListedAs = 'photo' | 'video' | 'other'

/**
 * What clearing Keevoro's unfinished copies from a folder did or, asked
 * first (`cleared` false), would do. `said` is the same sentences the
 * command line prints.
 */
export type Tidied = {
  folder: string
  cleared: boolean
  files: number
  bytes: number
  /** Left: written to in the last hour, so perhaps in use right now. */
  inUse: number
  /** Left: not something Keevoro put there. */
  notOurs: number
  said: string[]
}

/** One folder inside a surveyed place that holds photos or videos itself. */
export type FolderView = {
  folder: string
  /** Where it is inside the place, such as "2019 › Cornwall"; empty for the place itself. */
  relative: string
  photos: number
  videos: number
  /** Other files a person put on the kinds list. */
  others?: number
  bytes: number
  /** Of them, how many are only in online storage (OneDrive), never opened. */
  onlineOnly: number
  /** True when it, or a folder it is in, is left out of this backup. */
  leftOut?: boolean
}

/** The folders inside a place that hold photos or videos, from the survey Keevoro remembers. */
export type FoldersView = {
  place: string
  /** When that survey was made, in seconds since 1970. */
  surveyedAt: number
  /** False when the survey was stopped part-way, so there may be more. */
  complete: boolean
  folders: FolderView[]
  photos: number
  videos: number
  others?: number
  bytes: number
  onlineOnly: number
  /** The folder left out of this backup that the place is in, or is, if any. */
  leftOut?: string
}

/** One photo or video in a folder being looked inside. */
export type FileView = {
  path: string
  name: string
  isVideo: boolean
  /** An other file: a row with its name and kind, never a picture. */
  isOther?: boolean
  bytes: number
  /** When it was last changed (not taken), in seconds since 1970. */
  modified: number
  /** Only in online storage: no picture is made, as that would download it. */
  onlineOnly?: boolean
  /** How long a video lasts, in seconds, when its file says. */
  seconds?: number
  /** Why it would not be backed up, when it would not, in words. */
  leftOut?: string
}

/** The order a folder's files are given in: by name, or by the file's date (not the date taken). */
export type FilesSort = 'name' | 'newest' | 'oldest'
/** Which of a folder's files are given. */
export type FilesShow = 'all' | 'photos' | 'videos' | 'others'

/** Asks for one page of a folder's photos and videos. */
export type FilesRequest = { place: string; from: number; count: number; sort: FilesSort; only: FilesShow }

/** One page of the photos and videos in a folder. */
export type FilesView = {
  folder: string
  /** How many are shown in all, of the kind asked for: what `from` pages through. */
  total: number
  photos: number
  videos: number
  others?: number
  onlineOnly: number
  files: FileView[]
}

/** Asks where the space goes in a surveyed place. */
export type MapRequest = {
  /** The place, written whole: where the trail back starts. */
  place: string
  /** A folder inside it to show; the place itself when left out. */
  folder?: string
  /** How many levels of folders to open, 1 to 6. */
  depth: number
  /** The least room a box must take to be shown on its own; smaller ones are merged. */
  smallest?: number
}

/** What a box on the map holds most of, or an ending is: `carried` for other files a person put on the list, `other` for everything else. */
export type MapKind = 'photo' | 'video' | 'other' | 'carried'

/** One box on the map: a folder, one of the biggest files, or several together. */
export type MapNode = {
  /** A folder's or file's name; for several together "Photos", "Videos", "Other files" or "Smaller folders". */
  name: string
  /** A folder, written whole. */
  path?: string
  node: 'folder' | 'file' | 'files' | 'smaller'
  /** For a file, or files together, which kind: `other` is everything Keevoro does not look for, `carried` other files a person put on the list. */
  kind?: MapKind
  bytes: number
  photos: number
  videos: number
  others: number
  photoBytes: number
  videoBytes: number
  otherBytes: number
  /** Other files a person put on the kinds list, to be carried along. */
  carried?: number
  carriedBytes?: number
  /** Of its bytes, those only in online storage, never opened. */
  onlineOnlyBytes?: number
  /** Folders inside it; for smaller folders together, how many. */
  folders?: number
  /** Why Keevoro never looks through it, such as "System folder". */
  skipped?: string
  /** Keevoro made it: a backup, an old library, its own files. */
  madeByKeevoro?: boolean
  /** Why none of its photos and videos would be backed up, in words. */
  leftOut?: string
  /** The folder left out of this backup that this folder is, or is in: what Put back takes. */
  leftOutFolder?: string
  /** Of its photos and videos, the bytes this backup would take. */
  wouldBackUp: number
  /** It is a source of this backup. */
  source?: boolean
  /** It is inside a source of this backup. */
  inSource?: boolean
  /** Of the files this backup keeps from it, everything inside counted in, how many are safely everywhere it goes. */
  backedUp?: BackedUp
  unreadable?: boolean
  /** Something in it was not measured: the survey was stopped first. */
  notSized?: boolean
  /** Folders inside it counted into it, past what a map keeps one by one. */
  moreFolders?: number
  /** There is more inside than the depth asked for shows. */
  deeper?: boolean
  /** The endings of the files it holds, as places in the answer's `kinds`. */
  endings?: number[]
  children?: MapNode[]
  /**
   * Made by the screen, never by the API: small folders gathered into one
   * box, "Smaller folders", with the folders it stands for, how it is known,
   * and the share each is under of the folder it is in, and that folder's name.
   */
  gathered?: MapNode[]
  gatheredKey?: string
  gatheredUnder?: number
  gatheredIn?: string
}

/** How much of what this backup keeps from a folder is safely everywhere it goes: "Backed up: 1,180 of 1,204". */
export type BackedUp = { files: number; of: number }

/** One step of the trail from the place to the folder shown. */
export type TrailStep = { name: string; folder: string }

/** One kind of file by ending, at or inside the folder shown. */
export type KindView = {
  /** Lower case without the dot; empty for no ending, "…" for rarer ones. */
  ending: string
  kind: MapKind
  files: number
  bytes: number
  /** Why files with it here would not be backed up, when they would not. */
  leftOut?: string
}

/** Where the space goes in a surveyed place, from the survey Keevoro remembers. */
export type MapView = {
  place: string
  folder: string
  trail: TrailStep[]
  /** When the survey was made, in seconds since 1970. */
  surveyedAt: number
  /** False when the survey was stopped before it had seen every folder. */
  complete: boolean
  /** False when it was stopped before everything was measured. */
  sizedAll: boolean
  top: MapNode
  kinds: KindView[]
  /** Some folders' endings were not kept, so the kinds add up to less. */
  kindsShort?: boolean
  smallest: number
  /** The backup whose choices are said. */
  backup?: string
  /** Out of date (over an hour old, or a source in it looked through since): mapped again by itself on a disk, or asked first across a network. */
  refresh?: 'byItself' | 'askFirst'
  /** A source in it has been looked through since it was mapped. */
  searchedSince?: boolean
}

/** A newer version of Keevoro that is ready to install. */
export type Update = { version: string; page: string }

export interface Backend {
  state(): Promise<AppState>
  /** Starts a new backup with this name and opens it. */
  startBackup(name: string): Promise<AppState>
  /** Opens another backup; the one that was open is left as it was. */
  switchBackup(id: string): Promise<AppState>
  /** One backup in full: its sources, where it goes, what it holds, when it last ran. */
  backupDetails(id: string): Promise<BackupListing>
  /** Gives a backup a new name. Refused while it is being worked on. */
  renameBackup(id: string, to: string): Promise<AppState>
  /** Removes a backup from Keevoro, or with `dryRun` only says what that would do. */
  removeBackup(id: string, dryRun: boolean): Promise<Removal>
  /** The backups removed in the last week, which can still be put back. */
  removedBackups(): Promise<RemovedBackup[]>
  /** Puts back a backup removed in the last week, exactly as it was. */
  putBackBackup(id: string): Promise<AppState>
  /** Clears away a removed backup now; it can no longer be put back. */
  clearAwayBackup(id: string): Promise<RemovedBackup>
  /**
   * Puts Keevoro back as it was when first installed. Sources and anything
   * already sent are not touched.
   */
  resetEverything(): Promise<AppState>
  /** What Keevoro is doing now, why, and how hard it has been working. */
  activity(): Promise<Activity>
  /** This backup's earlier sends, newest first. */
  sendHistory(): Promise<Sent[]>
  /** What this backup's last run said when it ended, kept until the next one. */
  lastBackupSaid(): Promise<SendEvent | null>
  /** How a send in hand is getting on: how much, how fast, how long left. */
  sendProgress(): Promise<SendProgress>
  /** Pauses or resumes every heavy job, between one file and the next. */
  pauseWork(paused: boolean): Promise<void>
  /** Whether the computer's own picture codecs are in use, and what that means here. */
  acceleration(): Promise<Acceleration>
  /** Switches the use of the computer's own codecs on or off, and remembers. */
  setAcceleration(on: boolean): Promise<Acceleration>
  /** What an AI agent may do, as the person has allowed. */
  agentAccess(): Promise<AgentAccess>
  /** Changes what an AI agent may do. Each part given replaces what was there. */
  setAgentAccess(change: Partial<AgentAccess>): Promise<AgentAccess>
  /** What this backup is for: photos, videos and, once a kind of them is listed, other files. */
  backupWants(): Promise<WantsView>
  setBackupWants(wants: Wants): Promise<WantsView>
  /** Whether one source follows its backup's choice ('backup') or has its own. */
  sourceWants(id: number): Promise<WantsView>
  setSourceWants(id: number, wants: Wants | 'backup'): Promise<WantsView>
  /** How this backup arranges what it sends: by date, or keeping the folders as they are. */
  backupArrange(): Promise<ArrangeView>
  setBackupArrange(arrange: Arrange): Promise<ArrangeView>
  /** How one source's files are arranged: as the backup ('backup'), or its own way. */
  sourceArrange(id: number): Promise<SourceArrangeView>
  setSourceArrange(id: number, arrange: Arrange | 'backup'): Promise<SourceArrangeView>
  /** The kinds of file Keevoro looks for, the built-in ones first. */
  fileKinds(): Promise<FileKind[]>
  /** Adds an ending to look for, as photos, videos or other files. */
  addFileKind(ending: string, kind: ListedAs): Promise<FileKind>
  /** Changes one kind: photo, video or other file, on or off, or a new ending for one you added. */
  changeFileKind(ending: string, change: FileKindChange): Promise<FileKind>
  /** Takes an ending you added off the list. */
  removeFileKind(ending: string): Promise<FileKind>
  /** Puts back the kinds Keevoro comes with, all on. Your own stay. */
  resetFileKinds(): Promise<FileKind[]>
  /**
   * Clears Keevoro's unfinished copies (its `.keevoro-uploads` working
   * files) from a folder, or with `dryRun` only says what it would clear.
   * Nothing that was backed up is touched.
   */
  tidyFolder(folder: string, dryRun: boolean): Promise<Tidied>
  /** Asks for a folder to clear unfinished copies from. Null if they cancel. */
  chooseFolderToTidy(): Promise<string | null>
  /** Asks for a folder an AI agent may use. Null if they cancel. */
  chooseAgentFolder(): Promise<string | null>
  /** Asks a running search to stop. Nothing from it is kept. */
  stopScanning(): Promise<void>
  /** Asks a running send to stop after the file in hand. */
  stopSending(): Promise<void>
  /** How much room what is waiting needs, against where it is going. */
  room(): Promise<Room>
  /** Whether this backup also makes and sends previews for a gallery. */
  previewsWanted(): Promise<boolean>
  setPreviewsWanted(wanted: boolean): Promise<void>
  /** Shows a folder Keevoro has mentioned (a source, a destination) in the file manager. */
  openPlace(path: string): Promise<void>
  /** Shows the folder this backup sends to, when it sends to a folder. */
  /** Shows a folder destination: the one at this place in the list, or the first. */
  openDestination(at?: number): Promise<void>
  discover(): Promise<Discover>
  /** The drives plugged in right now, likeliest first. */
  places(): Promise<Place[]>
  /**
   * The drives plugged in, only if they are not those `known` (the `mounts`
   * given last time); null when nothing changed. With `asked` (Refresh, or
   * the system's notice) they are listed regardless. Cheap enough to ask
   * every couple of seconds.
   */
  placesIfChanged(known: string[] | null, asked: boolean): Promise<PlacesNow | null>
  /** Heard when the system says a drive arrived or left (on Windows). */
  onDrives(listener: () => void): () => void
  /** Starts looking through the drive or folder at this path. */
  scanPlace(path: string): Promise<void>
  /** When this backup backs up by itself, when next, and its last few runs. */
  schedule(): Promise<ScheduleView>
  setSchedule(request: SetSchedule): Promise<ScheduleView>
  skipNextRun(undo: boolean): Promise<ScheduleView>
  /** Runs it now as its schedule does; heard as `send` events. */
  runScheduledNow(): Promise<void>
  /** The person is using the window, so a backup by itself waits until they have finished. */
  windowUsed(): Promise<void>
  /** Looks through one source again, or every one; a source not plugged in is passed over and said, and `differentDrive` answers the question about one. */
  lookAgain(source?: number, differentDrive?: OtherDrive): Promise<void>
  /**
   * Finds which folders in a drive hold photos and videos, by reading
   * folder listings only. Quick, and no file is opened.
   */
  surveyPlace(path: string): Promise<Survey>
  /** Asks a running survey to stop; it answers with what it has so far. */
  stopSurvey(): Promise<void>
  onSurvey(listener: (event: SurveyProgress) => void): () => void
  /** Asks for a folder to look through. Null if cancelled. */
  pickPlace(): Promise<string | null>
  /** The last few hundred lines of the record of what Keevoro did. */
  logTail(): Promise<string>
  /** Opens the log file in the computer's text editor. */
  openLog(): Promise<void>
  /** A newer Keevoro this computer can install, or null. */
  checkUpdate(): Promise<Update | null>
  /** Downloads, checks and starts the update. On Windows the app then closes. */
  /** Installs the update, unless work is in hand: then it answers `waiting`, unless `stopFirst` stops that work first. */
  installUpdate(stopFirst?: boolean): Promise<UpdateStarted>
  /** Opens a release's "What's new" page in the browser. */
  openReleasePage(page: string): Promise<void>
  keepCollection(key: string, keep: boolean): Promise<void>
  /** Every file in a collection, in date order. */
  collectionPhotos(key: string): Promise<PhotoChoice[]>
  /** The kinds of file found on a source, most numerous first. */
  fileTypes(id: number): Promise<FileType[]>
  /** Includes or leaves out every file of one kind on one source. */
  includeFileType(id: number, ending: string, included: boolean): Promise<void>
  /** Leaves a folder out of this backup, or puts it back. Nothing in it, or already backed up, is touched. */
  leaveOutFolder(folder: string, leftOut: boolean): Promise<LeftOutFolder>
  /** Stops remembering a source. Nothing on the source itself is touched. */
  forgetSource(id: number): Promise<void>
  /** Every file found more than once: the copy kept, and the copies left out. */
  duplicates(): Promise<Duplicate[]>
  /** The pictures set aside as probably not memories. */
  setAside(): Promise<PhotoChoice[]>
  keepPhoto(id: string, keep: boolean): Promise<void>
  /** An address for a small picture of this file, usable in an <img>. */
  preview(id: string): Promise<string>
  /**
   * Stops waiting for a small picture asked for with `preview` or
   * `filePicture` that has not been fetched yet, by its key (`photoKey`,
   * `fileKey`): its tile has left the screen.
   */
  notNeeded(key: string): void
  /**
   * The folders inside a place that hold photos or videos, from the survey
   * Keevoro remembers; `null` when no survey covers it yet. Nothing is added.
   */
  foldersInside(place: string): Promise<FoldersView | null>
  /** A page of the photos and videos directly in one folder, as asked. Nothing is added. */
  folderFiles(request: FilesRequest): Promise<FilesView>
  /**
   * Where the space goes in a place, from the survey Keevoro remembers;
   * `null` when no survey covers it yet. Nothing is opened or changed.
   */
  spaceMap(request: MapRequest): Promise<MapView | null>
  /** Forgets every remembered survey and map; says how many. */
  forgetMaps(): Promise<{ forgotten: number }>
  /** An address for a small picture of any photo or video, by its path. Only Keevoro's cache is written. */
  filePicture(path: string): Promise<string>
  /** A screen-sized picture of any photo or video, by its path. Fetched afresh each time. */
  closerLookAtFile(path: string): Promise<string>
  /** A screen-sized picture of one file, for looking at it closely. Fetched afresh each time. */
  closerLook(id: string): Promise<string>
  /** Where this backup goes, in words, or null if nowhere yet. */
  destination(): Promise<string | null>
  /** Asks for a folder to back up to. Null if they cancel. */
  chooseDestinationFolder(): Promise<string | null>
  /** Checks a destination works, then remembers it. Returns it in words. */
  connectDestination(request: ConnectRequest): Promise<string>
  /** Adds cloud storage as a source, once its keys can read it. Then look through `root`. */
  addBucketSource(request: BucketSourceRequest): Promise<BucketAdded>
  /**
   * Stops backing up to one destination, by its place in the list, or to
   * all of them when none is given. Nothing backed up there is touched.
   */
  disconnectDestination(at?: number): Promise<void>
  send(): Promise<void>
  checkArrived(): Promise<void>
  /** What "Put this backup right" would do, and about how long. Changes nothing. */
  putRightPlan(): Promise<PutRightPlan>
  /** Puts this backup right: progress and what it did arrive as send events. */
  putRight(thorough: boolean): Promise<void>
  onSend(listener: (event: SendEvent) => void): () => void
  onScan(listener: (event: ScanEvent) => void): () => void
  /** Carries on with any background checking left over from last time, unless it was stopped; says how it stands. */
  keepChecking(): Promise<DuplicatesLeft | null>
  /** Pauses the background check for duplicates on its own, or lets it go on. */
  pauseChecking(paused: boolean): Promise<void>
  /** Stops the background check for duplicates until the next search or backup, or until asked. */
  stopChecking(): Promise<void>
  /** Checks for duplicates now, even after the check was stopped. */
  checkDuplicatesNow(): Promise<void>
  onCheck(listener: (event: CheckEvent) => void): () => void
}

const inTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window

async function tauriBackend(): Promise<Backend> {
  const { invoke } = await import('@tauri-apps/api/core')
  const { listen } = await import('@tauri-apps/api/event')
  const { open } = await import('@tauri-apps/plugin-dialog')

  // Listening is asynchronous in Tauri; hand back a way to stop that works
  // even if it is called before the listener has finished attaching.
  const subscribe = <T,>(event: string, listener: (payload: T) => void) => {
    const stop = listen<T>(event, (e) => listener(e.payload))
    return () => void stop.then((unlisten) => unlisten())
  }
  const pickFolder = async (title: string) => {
    const picked = await open({ directory: true, multiple: false, title })
    return typeof picked === 'string' ? picked : null
  }
  // Every small picture goes through one queue: a few at a time, dropped
  // when its tile leaves the screen, and let go of when long unused.
  const pictures = new PictureQueue()
  const webp = (bytes: ArrayBuffer) => URL.createObjectURL(new Blob([bytes], { type: 'image/webp' }))

  return {
    state: () => invoke('app_state'),
    startBackup: (name) => invoke('start_backup', { name }),
    switchBackup: (id) => invoke('switch_backup', { id }),
    backupDetails: (id) => invoke('backup_details', { id }),
    renameBackup: (id, to) => invoke('rename_backup', { id, to }),
    removeBackup: (id, dryRun) => invoke('remove_backup', { id, dryRun }),
    removedBackups: () => invoke('removed_backups'),
    putBackBackup: (id) => invoke('put_back_backup', { id }),
    clearAwayBackup: (id) => invoke('clear_away_backup', { id }),
    openDestination: (at) => invoke('open_destination', { at: at ?? null }),
    room: () => invoke('room'),
    previewsWanted: () => invoke('previews_wanted'),
    setPreviewsWanted: (wanted) => invoke('set_previews_wanted', { wanted }),
    acceleration: () => invoke('acceleration'),
    setAcceleration: (on) => invoke('set_acceleration', { on }),
    agentAccess: () => invoke('agent_access'),
    setAgentAccess: (change) => invoke('set_agent_access', { change }),
    backupWants: () => invoke('backup_wants'),
    setBackupWants: (wants) => invoke('set_backup_wants', { wants }),
    sourceWants: (id) => invoke('source_wants', { id }),
    setSourceWants: (id, wants) => invoke('set_source_wants', { id, wants }),
    backupArrange: () => invoke('backup_arrange'),
    setBackupArrange: (arrange) => invoke('set_backup_arrange', { arrange }),
    sourceArrange: (id) => invoke('source_arrange', { id }),
    setSourceArrange: (id, arrange) => invoke('set_source_arrange', { id, arrange }),
    fileKinds: () => invoke('file_kinds'),
    addFileKind: (ending, kind) => invoke('add_file_kind', { ending, kind }),
    changeFileKind: (ending, change) =>
      invoke('change_file_kind', {
        ending,
        kind: change.kind ?? null,
        on: change.on ?? null,
        to: change.to ?? null,
      }),
    removeFileKind: (ending) => invoke('remove_file_kind', { ending }),
    resetFileKinds: () => invoke('reset_file_kinds'),
    chooseAgentFolder: () => pickFolder('Choose a folder an AI agent may use'),
    tidyFolder: (folder, dryRun) => invoke('tidy_folder', { folder, dryRun }),
    chooseFolderToTidy: () => pickFolder('Choose a folder to clear unfinished copies from'),
    activity: () => invoke('activity'),
    sendProgress: () => invoke('send_progress'),
    sendHistory: () => invoke('send_history'),
    lastBackupSaid: () => invoke('last_backup_said'),
    pauseWork: (paused) => invoke('pause_work', { paused }),
    stopScanning: () => invoke('stop_scanning'),
    resetEverything: () => invoke('reset_everything'),
    stopSending: () => invoke('stop_sending'),
    openPlace: (path) => invoke('open_place', { path }),
    discover: () => invoke('discover'),
    places: () => invoke('places'),
    placesIfChanged: (known, asked) => invoke('places_if_changed', { known, asked }),
    onDrives: (listener) => subscribe('drives', () => listener()),
    scanPlace: (path) => invoke('scan_place', { path }),
    lookAgain: (source, differentDrive) => invoke('look_again', { source, differentDrive }),
    schedule: () => invoke('schedule'),
    setSchedule: (request) => invoke('set_schedule', { request }),
    skipNextRun: (undo) => invoke('skip_next_run', { undo }),
    runScheduledNow: () => invoke('run_scheduled_now'),
    windowUsed: () => invoke('window_used'),
    surveyPlace: (path) => invoke('survey_place', { path }),
    stopSurvey: () => invoke('stop_survey'),
    onSurvey: (listener) => subscribe('survey', listener),
    logTail: () => invoke('log_tail'),
    openLog: () => invoke('open_log'),
    checkUpdate: () => invoke('check_update'),
    installUpdate: (stopFirst) => invoke('install_update', { stopFirst }),
    openReleasePage: (page) => invoke('open_release_page', { page }),
    pickPlace: () => pickFolder('Choose a source: a drive or folder to look through'),
    keepCollection: (key, keep) => invoke('keep_collection', { key, keep }),
    collectionPhotos: (key) => invoke('collection_photos', { key }),
    duplicates: () => invoke('duplicates'),
    setAside: () => invoke('set_aside'),
    forgetSource: (id) => invoke('forget_source', { id }),
    fileTypes: (id) => invoke('file_types', { id }),
    includeFileType: (id, ending, included) => invoke('include_file_type', { id, ending, included }),
    leaveOutFolder: (folder, leftOut) => invoke('leave_out_folder', { folder, leftOut }),
    keepPhoto: (id, keep) => invoke('keep_photo', { id, keep }),
    preview: (id) =>
      pictures.want(photoKey(id), () => invoke<ArrayBuffer>('preview', { id }).then(webp)),
    notNeeded: (key) => pictures.drop(key),
    closerLook: (id) => invoke<ArrayBuffer>('closer_look', { id }).then(webp),
    foldersInside: (place) => invoke('folders_inside', { place }),
    folderFiles: (request) => invoke('folder_files', { request }),
    spaceMap: (request) => invoke('space_map', { request }),
    forgetMaps: () => invoke('forget_maps'),
    filePicture: (path) =>
      pictures.want(fileKey(path), () => invoke<ArrayBuffer>('file_picture', { path, large: false }).then(webp)),
    closerLookAtFile: (path) => invoke<ArrayBuffer>('file_picture', { path, large: true }).then(webp),
    destination: () => invoke('destination'),
    chooseDestinationFolder: () => pickFolder('Choose a folder to back up to'),
    connectDestination: (request) => invoke('connect_destination', { request }),
    addBucketSource: (request) => invoke('add_bucket_source', { request }),
    disconnectDestination: (at) => invoke('disconnect_destination', { at: at ?? null }),
    send: () => invoke('send'),
    checkArrived: () => invoke('check_arrived'),
    putRightPlan: () => invoke('put_right_plan'),
    putRight: (thorough) => invoke('put_right', { thorough }),
    onSend: (listener) => subscribe('send', listener),
    onScan: (listener) => subscribe('scan', listener),
    keepChecking: () => invoke('keep_checking'),
    pauseChecking: (paused) => invoke('pause_checking', { paused }),
    stopChecking: () => invoke('stop_checking'),
    checkDuplicatesNow: () => invoke('check_duplicates_now'),
    onCheck: (listener) => subscribe('check', listener),
  }
}

let ready: Promise<Backend> | undefined

export function backend(): Promise<Backend> {
  ready ??= inTauri ? tauriBackend() : Promise.resolve(demoBackend())
  return ready
}
