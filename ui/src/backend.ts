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
  kept: boolean
  gathered: number
  cover: Photo
  previews: Photo[]
}

/** One file inside a collection, or one picture set aside as junk. */
export type PhotoChoice = Photo & { taken: string | null; kept: boolean; junkReason: string | null }

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
export type LeftOut = { folder: string; what: 'keevoroBackup' | 'keevoroOwn' | 'oldLibrary'; said: string }

export type SurveyProgress = { folders: number; files: number; photos: number; videos: number }

/** A drive plugged into this computer, offered as somewhere to look. */
export type Place = {
  path: string
  name: string
  detail: string
  removable: boolean
  /** A whole disk big enough to ask before looking through all of it. */
  askFirst: boolean
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

/** Photos and videos, photos only, or videos only. */
export type Wants = 'both' | 'photos' | 'videos'

/** What a backup or a source wants, and what that means, in words. */
export type WantsView = {
  wants: Wants
  /** For a source: true when it has its own choice rather than the backup's. */
  own?: boolean
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
}

export type Discover = {
  photos: number
  videos: number
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
  there: number
  waiting: number
  waitingBytes: number
}

/**
 * One job the person comes back to, such as "Drone footage": its own
 * sources, its own choices and its own place to send things.
 */
export type Backup = { id: string; name: string }

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
}

/** Sent while files a quick search only sampled are read in the background. */
export type CheckEvent = { done: number; total: number; finished: boolean }

/** Sent while a backup or a check runs, and once more when it is done. */
export type SendEvent = {
  line?: string
  finished: boolean
  summary?: string[]
  problem?: string
  /** On the last event: how it ended, in a word. */
  outcome?: 'worked' | 'someFiles' | 'unreachable' | 'refused' | 'stopped'
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
export type Room = { needed: number; folder: string | null; free: number | null; enough: boolean }

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
  /** Done, what it is doing now, under way alongside in the background, or still to come. */
  state: 'done' | 'now' | 'alongside' | 'next'
  unit: 'bytes' | 'files'
  done: number
  work: number
  /** Per second, while it runs and once measured. */
  perSecond: number | null
  /** As shown: "Backing up your files: 708.4 GB of 716.8 GB at 7.5 MB/s". */
  line: string
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
  kind: 'photo' | 'video'
  /** Looked for. One switched off is passed over. */
  on: boolean
  /** Comes with Keevoro: it can be switched off, not removed or renamed. */
  builtIn: boolean
}

/** A change to one kind of file. Anything left out stays as it is. */
export type FileKindChange = { kind?: 'photo' | 'video'; on?: boolean; to?: string }

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
  bytes: number
  /** Of them, how many are only in online storage (OneDrive), never opened. */
  onlineOnly: number
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
  bytes: number
  onlineOnly: number
}

/** One photo or video in a folder being looked inside. */
export type FileView = {
  path: string
  name: string
  isVideo: boolean
  bytes: number
  /** When it was last changed (not taken), in seconds since 1970. */
  modified: number
  /** Only in online storage: no picture is made, as that would download it. */
  onlineOnly?: boolean
}

/** One page of the photos and videos in a folder. */
export type FilesView = {
  folder: string
  total: number
  photos: number
  videos: number
  onlineOnly: number
  files: FileView[]
}

/** A newer version of Keevoro that is ready to install. */
export type Update = { version: string; page: string }

export interface Backend {
  state(): Promise<AppState>
  /** Starts a new backup with this name and opens it. */
  startBackup(name: string): Promise<AppState>
  /** Opens another backup; the one that was open is left as it was. */
  switchBackup(id: string): Promise<AppState>
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
  /** Whether this backup is for photos and videos, photos only, or videos only. */
  backupWants(): Promise<WantsView>
  setBackupWants(wants: Wants): Promise<WantsView>
  /** Whether one source follows its backup's choice ('backup') or has its own. */
  sourceWants(id: number): Promise<WantsView>
  setSourceWants(id: number, wants: Wants | 'backup'): Promise<WantsView>
  /** The kinds of file Keevoro looks for, the built-in ones first. */
  fileKinds(): Promise<FileKind[]>
  /** Adds an ending to look for, as photos or videos. */
  addFileKind(ending: string, kind: 'photo' | 'video'): Promise<FileKind>
  /** Changes one kind: photo or video, on or off, or a new ending for one you added. */
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
  /** Starts looking through the drive or folder at this path. */
  scanPlace(path: string): Promise<void>
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
  installUpdate(): Promise<void>
  /** Opens a release's "What's new" page in the browser. */
  openReleasePage(page: string): Promise<void>
  keepCollection(key: string, keep: boolean): Promise<void>
  /** Every file in a collection, in date order. */
  collectionPhotos(key: string): Promise<PhotoChoice[]>
  /** The kinds of file found on a source, most numerous first. */
  fileTypes(id: number): Promise<FileType[]>
  /** Includes or leaves out every file of one kind on one source. */
  includeFileType(id: number, ending: string, included: boolean): Promise<void>
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
  /** The folders inside a surveyed place that hold photos or videos. Nothing is added. */
  foldersInside(place: string): Promise<FoldersView>
  /** A page of the photos and videos directly in one folder, by name. Nothing is added. */
  folderFiles(place: string, from: number, count: number): Promise<FilesView>
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
  /**
   * Stops backing up to one destination, by its place in the list, or to
   * all of them when none is given. Nothing backed up there is touched.
   */
  disconnectDestination(at?: number): Promise<void>
  send(): Promise<void>
  checkArrived(): Promise<void>
  onSend(listener: (event: SendEvent) => void): () => void
  onScan(listener: (event: ScanEvent) => void): () => void
  /** Carries on with any background checking left over from last time. */
  keepChecking(): Promise<void>
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
    scanPlace: (path) => invoke('scan_place', { path }),
    surveyPlace: (path) => invoke('survey_place', { path }),
    stopSurvey: () => invoke('stop_survey'),
    onSurvey: (listener) => subscribe('survey', listener),
    logTail: () => invoke('log_tail'),
    openLog: () => invoke('open_log'),
    checkUpdate: () => invoke('check_update'),
    installUpdate: () => invoke('install_update'),
    openReleasePage: (page) => invoke('open_release_page', { page }),
    pickPlace: () => pickFolder('Choose a source: a drive or folder to look through'),
    keepCollection: (key, keep) => invoke('keep_collection', { key, keep }),
    collectionPhotos: (key) => invoke('collection_photos', { key }),
    duplicates: () => invoke('duplicates'),
    setAside: () => invoke('set_aside'),
    forgetSource: (id) => invoke('forget_source', { id }),
    fileTypes: (id) => invoke('file_types', { id }),
    includeFileType: (id, ending, included) => invoke('include_file_type', { id, ending, included }),
    keepPhoto: (id, keep) => invoke('keep_photo', { id, keep }),
    preview: (id) =>
      pictures.want(photoKey(id), () => invoke<ArrayBuffer>('preview', { id }).then(webp)),
    notNeeded: (key) => pictures.drop(key),
    closerLook: (id) => invoke<ArrayBuffer>('closer_look', { id }).then(webp),
    foldersInside: (place) => invoke('folders_inside', { place }),
    folderFiles: (place, from, count) => invoke('folder_files', { place, from, count }),
    filePicture: (path) =>
      pictures.want(fileKey(path), () => invoke<ArrayBuffer>('file_picture', { path, large: false }).then(webp)),
    closerLookAtFile: (path) => invoke<ArrayBuffer>('file_picture', { path, large: true }).then(webp),
    destination: () => invoke('destination'),
    chooseDestinationFolder: () => pickFolder('Choose a folder to back up to'),
    connectDestination: (request) => invoke('connect_destination', { request }),
    disconnectDestination: (at) => invoke('disconnect_destination', { at: at ?? null }),
    send: () => invoke('send'),
    checkArrived: () => invoke('check_arrived'),
    onSend: (listener) => subscribe('send', listener),
    onScan: (listener) => subscribe('scan', listener),
    keepChecking: () => invoke('keep_checking'),
    onCheck: (listener) => subscribe('check', listener),
  }
}

let ready: Promise<Backend> | undefined

export function backend(): Promise<Backend> {
  ready ??= inTauri ? tauriBackend() : Promise.resolve(demoBackend())
  return ready
}
