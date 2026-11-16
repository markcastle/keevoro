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

export type AppState = {
  /** Where the library lives, or null until the user has chosen. */
  library: string | null
  /** A sensible place to suggest for a new library. */
  suggestedLibrary: string
  version: string
}

/** Sent while a drive is being looked through, and once more when done. */
export type ScanEvent = { name: string; photos: number; videos: number; finished: boolean; problem?: string }

/** Sent while photos are being gathered, and once more when done. */
export type GatherEvent = {
  step: 'copying' | 'previews' | 'videos' | 'finished'
  done: number
  total: number
  /** On the last event: what happened, as sentences to show. */
  summary?: string[]
  problem?: string
}

/** Sent while a backup or a check runs, and once more when it is done. */
export type BackupEvent = { line?: string; finished: boolean; summary?: string[]; problem?: string }

/** What the "connect" form collects. Keys go to the system keychain. */
export type ConnectRequest = {
  kind: 'folder' | 'r2' | 's3' | 'endpoint'
  /** The folder, Cloudflare account ID, Amazon region or server address. */
  place: string
  bucket?: string
  accessKeyId?: string
  secretAccessKey?: string
}

export interface Backend {
  state(): Promise<AppState>
  /** Asks the user to pick a folder for the library. Null if they cancel. */
  chooseLibrary(): Promise<AppState | null>
  useLibrary(path: string): Promise<AppState>
  discover(): Promise<Discover>
  /** Asks for a source (a drive or folder) and starts looking through it. False if cancelled. */
  addPlace(): Promise<boolean>
  keepCollection(key: string, keep: boolean): Promise<void>
  /** Every file in a collection, in date order. */
  collectionPhotos(key: string): Promise<PhotoChoice[]>
  /** The pictures set aside as probably not memories. */
  setAside(): Promise<PhotoChoice[]>
  keepPhoto(id: string, keep: boolean): Promise<void>
  /** An address for a small picture of this file, usable in an <img>. */
  preview(id: string): Promise<string>
  gather(): Promise<void>
  /** Where the library is backed up to, in words, or null if nowhere yet. */
  backupState(): Promise<string | null>
  /** Asks for a folder to back up to. Null if they cancel. */
  chooseBackupFolder(): Promise<string | null>
  /** Checks a destination works, then remembers it. Returns it in words. */
  connectBackup(request: ConnectRequest): Promise<string>
  disconnectBackup(): Promise<void>
  backUp(): Promise<void>
  checkBackup(): Promise<void>
  onBackup(listener: (event: BackupEvent) => void): () => void
  onScan(listener: (event: ScanEvent) => void): () => void
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
    async chooseLibrary() {
      const path = await pickFolder('Choose where your library will live')
      return path ? invoke<AppState>('use_library', { path }) : null
    },
    discover: () => invoke('discover'),
    async addPlace() {
      const path = await pickFolder('Choose a source: a drive or folder to look through')
      if (!path) return false
      await invoke('scan_place', { path })
      return true
    },
    keepCollection: (key, keep) => invoke('keep_collection', { key, keep }),
    collectionPhotos: (key) => invoke('collection_photos', { key }),
    setAside: () => invoke('set_aside'),
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
    backupState: () => invoke('backup_state'),
    chooseBackupFolder: () => pickFolder('Choose a folder to back up to'),
    connectBackup: (request) => invoke('connect_backup', { request }),
    disconnectBackup: () => invoke('disconnect_backup'),
    backUp: () => invoke('back_up'),
    checkBackup: () => invoke('check_backup'),
    onBackup: (listener) => subscribe('backup', listener),
    onScan: (listener) => subscribe('scan', listener),
    onGather: (listener) => subscribe('gather', listener),
  }
}

let ready: Promise<Backend> | undefined

export function backend(): Promise<Backend> {
  ready ??= inTauri ? tauriBackend() : Promise.resolve(demoBackend())
  return ready
}
