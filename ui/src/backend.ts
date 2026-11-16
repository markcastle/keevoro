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

/** A drive plugged into this computer, offered as somewhere to look. */
export type Place = {
  path: string
  name: string
  detail: string
  removable: boolean
  /** A whole disk big enough to ask before looking through all of it. */
  askFirst: boolean
}

export type Source = { id: number; name: string; root: string; items: number }

export type Discover = {
  photos: number
  videos: number
  bytes: number
  duplicates: number
  junk: number
  waitingToGather: number
  sources: Source[]
  collections: Collection[]
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
  /** A sensible place to suggest for a new library. */
  suggestedLibrary: string
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
  finished: boolean
  problem?: string
}

/** Sent while files a quick search only sampled are read in the background. */
export type CheckEvent = { done: number; total: number; finished: boolean }

/** Sent while photos are being gathered, and once more when done. */
export type GatherEvent = {
  step: 'checking' | 'copying' | 'previews' | 'videos' | 'finished'
  done: number
  total: number
  /** On the last event: what happened, as sentences to show. */
  summary?: string[]
  problem?: string
}

/** Sent while a backup or a check runs, and once more when it is done. */
export type SendEvent = { line?: string; finished: boolean; summary?: string[]; problem?: string }

/** What the "connect" form collects. Keys go to the system keychain. */
export type ConnectRequest = {
  kind: 'folder' | 'r2' | 's3' | 'endpoint'
  /** The folder, Cloudflare account ID, Amazon region or server address. */
  place: string
  bucket?: string
  accessKeyId?: string
  secretAccessKey?: string
}

/** A newer version of Keevoro that is ready to install. */
export type Update = { version: string; page: string }

export interface Backend {
  state(): Promise<AppState>
  /** Asks the user to pick a folder for the library. Null if they cancel. */
  chooseLibrary(): Promise<AppState | null>
  useLibrary(path: string): Promise<AppState>
  /** Starts a new backup with this name and opens it. */
  startBackup(name: string): Promise<AppState>
  /** Opens another backup; the one that was open is left as it was. */
  switchBackup(id: string): Promise<AppState>
  /** Shows the folder this backup sends to, when it sends to a folder. */
  openDestination(): Promise<void>
  discover(): Promise<Discover>
  /** The drives plugged in right now, likeliest first. */
  places(): Promise<Place[]>
  /** Starts looking through the drive or folder at this path. */
  scanPlace(path: string): Promise<void>
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
  /** Shows the library folder in the system's file manager. */
  openLibrary(): Promise<void>
  keepCollection(key: string, keep: boolean): Promise<void>
  /** Every file in a collection, in date order. */
  collectionPhotos(key: string): Promise<PhotoChoice[]>
  /** Stops remembering a source. Nothing on the source itself is touched. */
  forgetSource(id: number): Promise<void>
  /** The pictures set aside as probably not memories. */
  setAside(): Promise<PhotoChoice[]>
  keepPhoto(id: string, keep: boolean): Promise<void>
  /** An address for a small picture of this file, usable in an <img>. */
  preview(id: string): Promise<string>
  gather(): Promise<void>
  /** Where the library is backed up to, in words, or null if nowhere yet. */
  destination(): Promise<string | null>
  /** Asks for a folder to back up to. Null if they cancel. */
  chooseDestinationFolder(): Promise<string | null>
  /** Checks a destination works, then remembers it. Returns it in words. */
  connectDestination(request: ConnectRequest): Promise<string>
  disconnectDestination(): Promise<void>
  send(): Promise<void>
  checkArrived(): Promise<void>
  onSend(listener: (event: SendEvent) => void): () => void
  onScan(listener: (event: ScanEvent) => void): () => void
  /** Carries on with any background checking left over from last time. */
  keepChecking(): Promise<void>
  onCheck(listener: (event: CheckEvent) => void): () => void
  onGather(listener: (event: GatherEvent) => void): () => void
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
  const previews = new Map<string, Promise<string>>()

  return {
    state: () => invoke('app_state'),
    useLibrary: (path) => invoke('use_library', { path }),
    startBackup: (name) => invoke('start_backup', { name }),
    switchBackup: (id) => invoke('switch_backup', { id }),
    openDestination: () => invoke('open_destination'),
    async chooseLibrary() {
      const path = await pickFolder('Choose where your library will live')
      return path ? invoke<AppState>('use_library', { path }) : null
    },
    discover: () => invoke('discover'),
    places: () => invoke('places'),
    scanPlace: (path) => invoke('scan_place', { path }),
    openLibrary: () => invoke('open_library'),
    logTail: () => invoke('log_tail'),
    openLog: () => invoke('open_log'),
    checkUpdate: () => invoke('check_update'),
    installUpdate: () => invoke('install_update'),
    openReleasePage: (page) => invoke('open_release_page', { page }),
    pickPlace: () => pickFolder('Choose a source: a drive or folder to look through'),
    keepCollection: (key, keep) => invoke('keep_collection', { key, keep }),
    collectionPhotos: (key) => invoke('collection_photos', { key }),
    setAside: () => invoke('set_aside'),
    forgetSource: (id) => invoke('forget_source', { id }),
    keepPhoto: (id, keep) => invoke('keep_photo', { id, keep }),
    preview(id) {
      // Each picture is fetched once and reused; the bytes come back raw.
      let url = previews.get(id)
      if (!url) {
        url = invoke<ArrayBuffer>('preview', { id }).then((bytes) =>
          URL.createObjectURL(new Blob([bytes], { type: 'image/webp' })),
        )
        url.catch(() => previews.delete(id))
        previews.set(id, url)
      }
      return url
    },
    gather: () => invoke('gather'),
    destination: () => invoke('destination'),
    chooseDestinationFolder: () => pickFolder('Choose a folder to back up to'),
    connectDestination: (request) => invoke('connect_destination', { request }),
    disconnectDestination: () => invoke('disconnect_destination'),
    send: () => invoke('send'),
    checkArrived: () => invoke('check_arrived'),
    onSend: (listener) => subscribe('send', listener),
    onScan: (listener) => subscribe('scan', listener),
    keepChecking: () => invoke('keep_checking'),
    onCheck: (listener) => subscribe('check', listener),
    onGather: (listener) => subscribe('gather', listener),
  }
}

let ready: Promise<Backend> | undefined

export function backend(): Promise<Backend> {
  ready ??= inTauri ? tauriBackend() : Promise.resolve(demoBackend())
  return ready
}
