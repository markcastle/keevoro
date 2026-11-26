// A stand-in for the engine, used when the screens are opened in a browser.
//
// It holds a small made-up backup in memory and behaves like the real
// thing: adding a place "finds" photos over a couple of seconds, keeping a
// collection sticks, backing up counts up. The pictures are drawn on the
// spot, so nothing is downloaded and no real photos are in the repository.
//
// Add ?demo=new to the address to start as a first-time user with no
// backup, or ?demo=empty for a backup with nothing found yet, or
// ?demo=scans for a new backup with a folder of family scans in it, as our
// first outside tester had, not yet asked how to arrange it. Add
// ?page=settings (or a place in it, such as settings/byItself) to open
// there, as the icon by the clock does.

import builtInKindList from './builtInKinds.json'
import { demoMap } from './demoMap'
import type {
  KeepRunningView,
  WindowAsk,
  TopBox,
  AppState,
  FileKind,
  ListedAs,
  Wants,
  SendEvent,
  Backend,
  CheckEvent,
  Collection,
  Discover,
  FileType,
  Photo,
  PhotoChoice,
  Sample,
  Sent,
  ScanEvent,
  SurveyProgress,
  Backup,
  BackupListing,
  DuplicatesLeft,
  Place as Drive,
  Removal,
  RemovedBackup,
  BackupEstimate,
  BackupStage,
  FileView,
  FolderView,
  Arrange,
  ArrangeView,
  SourceArrangeView,
  ScheduleAttempt,
  ScheduleChoice,
  ScheduleView,
  ReportListed,
  AwayView,
  MannersView,
  ConnectRequest,
  FoundBackup,
} from './backend'
import { count, foundNew, size } from './format'
import { PictureQueue, fileKey } from './pictureQueue'
import { checkedCode, ReportNotSent } from './reportsWindow'

const SCENES: [string, string, string][] = [
  ['#7ec8e3', '#f6d365', '#2e7d6b'], // beach
  ['#fda085', '#f6d365', '#5b3a29'], // sunset
  ['#a1c4fd', '#c2e9fb', '#3f6fb5'], // sea
  ['#d4fc79', '#96e6a1', '#2f6b3a'], // garden
  ['#fbc2eb', '#a6c1ee', '#6a4c93'], // evening
  ['#ffecd2', '#fcb69f', '#9c4a2f'], // indoors
]

/** A soft landscape made of gradients: sky, a hill and a sun. */
function scene(seed: number, video: boolean) {
  const [sky, glow, land] = SCENES[seed % SCENES.length]
  const hill = 55 + ((seed * 13) % 20)
  const sun = 20 + ((seed * 29) % 60)
  const play = video
    ? `<circle cx="50%" cy="50%" r="34" fill="rgba(0,0,0,.45)"/><path d="M290 232 l34 20 -34 20z" fill="#fff" transform="translate(${4},-${2})"/>`
    : ''
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 600 500" preserveAspectRatio="xMidYMid slice">
    <defs><linearGradient id="s" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="${sky}"/><stop offset="1" stop-color="${glow}"/></linearGradient></defs>
    <rect width="600" height="500" fill="url(#s)"/>
    <circle cx="${sun}%" cy="28%" r="38" fill="#fff" opacity=".75"/>
    <path d="M0 ${hill * 5} Q 150 ${hill * 5 - 90} 300 ${hill * 5 - 20} T 600 ${hill * 5 - 60} V500 H0z" fill="${land}"/>
    <path d="M0 ${hill * 5 + 70} Q 200 ${hill * 5} 420 ${hill * 5 + 50} T 600 ${hill * 5 + 30} V500 H0z" fill="${land}" opacity=".6"/>
    ${play}
  </svg>`
  return `data:image/svg+xml;utf8,${encodeURIComponent(svg)}`
}

let nextId = 1
const pictures = new Map<string, string>()

function photo(name: string, video = false): Photo {
  const n = nextId++
  const id = n.toString(16).padStart(16, '0')
  pictures.set(id, scene(n, video))
  return { id, isVideo: video, name, sourcePath: `E:\\Photos\\${name}`, thumbnail: null }
}

function collection(title: string, when: string, photos: number, videos: number, parents: string[] = []): Collection {
  const shown = Array.from({ length: Math.min(5, photos + videos) }, (_, i) =>
    photo(`IMG_${String(1000 + nextId)}.${i === 2 && videos > 0 ? 'MOV' : 'jpg'}`, i === 2 && videos > 0),
  )
  const parts = [when, `${photos.toLocaleString('en-GB')} photo${photos === 1 ? '' : 's'}`]
  if (videos > 0) parts.push(`${videos} video${videos === 1 ? '' : 's'}`)
  return {
    key: [...parents, title].join(' / '),
    title,
    summary: parts.filter((p) => p && p !== title).join(' \u00b7 '),
    parents,
    photos,
    videos,
    kept: true,
    gathered: 0,
    cover: shown[0],
    previews: shown,
  }
}

const FOUND: (() => Collection)[] = [
  () => collection('Cornwall', 'August 2008', 83, 6, ['Holidays']),
  () => collection('Christmas 2011', 'December 2011', 42, 3, ['Family']),
  () => collection('Walks with Max', 'October 2014', 61, 0),
  () => collection('Summer evenings', 'July 2016', 27, 2),
  () => collection('March 2012', '', 14, 0),
  () => collection('Spain 2009', 'July 2009', 118, 9, ['Holidays']),
  () => collection('Undated', '', 9, 0),
]

/**
 * Other files on the family hard drive, by the folder they are in, with
 * their sizes: found only once a person puts their kind on the list as
 * other files and looks again, as the real search does.
 */
const OTHER_FOLDERS: { folder: string[]; files: [string, number][] }[] = [
  {
    folder: ['Documents'],
    files: ['Letter to Gran.pdf', 'House insurance.pdf', 'Wedding speech.docx', 'Recipes.pdf'].map((n, i) => [n, (180 + i * 97) * 1024]),
  },
  { folder: ['Documents', 'Tax 2019'], files: ['Return.pdf', 'P60.pdf', 'Receipts.pdf'].map((n, i) => [n, (420 + i * 61) * 1024]) },
  {
    folder: ['Projects', 'Garden shed'],
    files: ['Shed plan.blend', 'Shed roof.blend', 'Shed door.blend', 'Shed window.blend', 'Shed floor.blend'].map(
      (n, i) => [n, (28 + i * 3) * 1024 ** 2],
    ),
  },
]

/**
 * Music in the Music folder of this computer, by artist and album, every
 * name made up: found once a backup is for music and its sources are
 * looked through, as the real search does. Never opened, so a music note
 * stands where a picture would.
 */
const MUSIC_ROOT = 'C:\\Users\\You'
const MUSIC_ALBUMS: { folder: string[]; files: [string, number][] }[] = [
  {
    folder: ['Music', 'The Made-Up Band', 'Pretend Songs'],
    files: Array.from({ length: 8 }, (_, i): [string, number] => [`Track ${String(i + 1).padStart(2, '0')}.flac`, (31 + i * 2) * 1024 ** 2]),
  },
  {
    folder: ['Music', 'Willow Street Trio', 'Kitchen Sessions'],
    files: [
      ...Array.from({ length: 6 }, (_, i): [string, number] => [`Track ${String(i + 1).padStart(2, '0')}.mp3`, (7 + i) * 1024 ** 2]),
      ['Kitchen Sessions.m3u', 1024],
    ],
  },
  {
    folder: ['Music', 'Field Recordings', 'Garden in spring'],
    files: Array.from({ length: 3 }, (_, i): [string, number] => [`Recording ${i + 1}.wav`, (52 + i * 4) * 1024 ** 2]),
  },
  {
    folder: ['Music', 'The Made-Up Band', 'Second Helpings'],
    files: Array.from({ length: 2 }, (_, i): [string, number] => [`Track ${String(i + 1).padStart(2, '0')}.flac`, (28 + i * 3) * 1024 ** 2]),
  },
]
/** How many tracks the made-up music holds. */
const MUSIC_TRACKS = MUSIC_ALBUMS.reduce((n, a) => n + a.files.length, 0)

/** As Keevoro says it of files only in OneDrive, passed over unopened. */
const onlineOnlySaid = (n: number) =>
  `${count(n, 'file')} here ${n === 1 ? 'is' : 'are'} in OneDrive only, not on this computer, so Keevoro did not open ${n === 1 ? 'it' : 'them'}. To include ${n === 1 ? 'it' : 'them'}, make ${n === 1 ? 'it' : 'them'} available offline in OneDrive first, then look again.`

/** Cloud storage added as a source, by its root: where it is in words, and its kind. */
const bucketPlaces = new Map<string, { place: string; kind: string }>()

/** As Keevoro says what reading from cloud storage would cost (`keevoro_core::egress`). */
const cloudCost = (source: { root: string }, bytes: number) => {
  const gb = bytes / 1024 ** 3
  const size = gb >= 1 ? `${gb.toFixed(1)} GB` : `${(bytes / 1024 ** 2).toFixed(1)} MB`
  if (source.root.startsWith('r2://')) return `Reading ${size} out of Cloudflare R2 costs about nothing: Cloudflare R2 doesn’t charge for downloading.`
  if (source.root.startsWith('gcs://')) {
    const pounds = gb * 0.12 * 0.79
    const said = pounds < 1 ? 'under £1' : `about £${Math.round(pounds).toLocaleString('en-GB')}`
    return `Reading ${size} out of Google Cloud Storage costs ${said} at Google’s usual price; Cloudflare R2 charges nothing for this. Files kept in Nearline, Coldline or Archive storage cost about 1p to 4p a gigabyte more to read.`
  }
  if (source.root.startsWith('s3://')) {
    const pounds = gb * 0.09 * 0.79
    const said = pounds < 1 ? 'under £1' : `about £${Math.round(pounds).toLocaleString('en-GB')}`
    return `Reading ${size} out of Amazon S3 costs ${said} at Amazon’s usual price; Cloudflare R2 charges nothing for this. Amazon lets each account download about 100 GB a month free, which may make it less.`
  }
  return `Some cloud storage charges about 7p a gigabyte to read files out of it, and Keevoro doesn’t know what this storage charges for ${size}, so check before you start.`
}

/** As a search says it, knowing how big they are from each file's listing. */
const onlineOnlySized = (n: number, size: string) =>
  `${count(n, 'file')}, ${size}, ${n === 1 ? 'is' : 'are'} in OneDrive only, not on this computer, so Keevoro did not open ${n === 1 ? 'it' : 'them'}. To include ${n === 1 ? 'it' : 'them'}, make ${n === 1 ? 'it' : 'them'} available offline in OneDrive first, then look again.`

/**
 * The owner's own case, made up: a Pictures folder that holds a camera roll
 * big enough to need a grid that only draws what is on screen, screen
 * recordings, and a holiday, with a few files only in OneDrive.
 */
const INSIDE: { relative: string; photos: number; videos: number }[] = [
  { relative: '', photos: 6, videos: 0 },
  { relative: 'Camera Roll', photos: 1_204, videos: 37 },
  { relative: 'Captures', photos: 0, videos: 41 },
  { relative: '2019 › Cornwall', photos: 120, videos: 4 },
  { relative: 'Screenshots', photos: 300, videos: 0 },
]

/** The files in one made-up folder, by name: photos, then videos, every 50th only in OneDrive. */
function demoFiles(folder: string, photos: number, videos: number): FileView[] {
  const files: FileView[] = []
  for (let i = 0; i < photos; i++) {
    files.push({
      path: `${folder}\\IMG_${String(i + 1).padStart(4, '0')}.jpg`,
      name: `IMG_${String(i + 1).padStart(4, '0')}.jpg`,
      isVideo: false,
      bytes: 3.1 * 1024 ** 2,
      modified: 1_560_000_000 + i * 600,
      ...(i % 50 === 49 ? { onlineOnly: true } : {}),
    })
  }
  for (let i = 0; i < videos; i++) {
    files.push({
      path: `${folder}\\VID_${String(i + 1).padStart(4, '0')}.mp4`,
      name: `VID_${String(i + 1).padStart(4, '0')}.mp4`,
      isVideo: true,
      bytes: 88 * 1024 ** 2,
      // Taken between the photos, so sorting by date mixes them.
      modified: 1_560_000_000 + i * 900 + 300,
      // The first is one minute 42 seconds; every fifth says nothing.
      ...(i % 5 === 4 ? {} : { seconds: 102 + i * 37 }),
    })
  }
  return files
}


/**
 * The top of a made-up map part of the way through its survey: the
 * folders found so far, each with that part of what it holds, as the real
 * survey sends them while it walks.
 */
function topSoFar(place: string, part: number): TopBox[] {
  const whole = demoMap({ place, depth: 1, smallest: 1 }, () => undefined, 0)?.top.children ?? []
  const boxes: TopBox[] = []
  const here: TopBox = { name: place, node: 'files', bytes: 0, photos: 0, videos: 0, others: 0, photoBytes: 0, videoBytes: 0, otherBytes: 0 }
  whole.forEach((n, i) => {
    // Folders are come upon one after another, and fill as they are walked.
    if (i / whole.length > part) return
    const scaled = (v: number) => Math.round(v * part)
    const box: TopBox = {
      name: n.name,
      node: n.node === 'smaller' ? 'smaller' : 'folder',
      bytes: scaled(n.bytes),
      photos: scaled(n.photos),
      videos: scaled(n.videos),
      others: scaled(n.others),
      photoBytes: scaled(n.photoBytes),
      videoBytes: scaled(n.videoBytes),
      otherBytes: scaled(n.otherBytes),
      ...(n.node === 'smaller' ? { folders: n.folders } : {}),
    }
    if (n.node === 'folder' || n.node === 'smaller') boxes.push(box)
    else for (const k of ['bytes', 'photos', 'videos', 'others', 'photoBytes', 'videoBytes', 'otherBytes'] as const) here[k] += box[k]
  })
  if (here.bytes > 0) boxes.push(here)
  return boxes.sort((a, b) => b.bytes - a.bytes)
}

export function demoBackend(): Backend {
  const mode = new URLSearchParams(window.location.search).get('demo')
  // Keeping Keevoro running by the clock, as on Windows. `?closing` opens on
  // the explanation said the first time the window is closed; `?quitting`
  // on the question asked before quitting cuts a backup short.
  let keepRunning: KeepRunningView = {
    keepRunning: true,
    minimiseToTray: false,
    startWithComputer: false,
    notify: true,
    tray: 'shown',
    whereTheIconIs:
      'Its icon is by the clock, at the bottom right of the screen (it may be under the ^ arrow there). Open or quit Keevoro from there.',
  }
  const opening = new URLSearchParams(window.location.search)
  // `?page=settings/advanced/kinds` opens on a page, as the icon by the clock's "Settings…" does.
  let windowAsked: WindowAsk | null = opening.has('closing')
    ? { closing: keepRunning.whereTheIconIs }
    : opening.has('quitting')
      ? { quitting: 'A backup is running. Stop it after the file in hand and quit? Nothing is lost.' }
      : opening.get('page')
        ? { page: opening.get('page') ?? undefined }
        : null
  const firstHome = 'C:\\Users\\You\\AppData\\Keevoro\\backups\\s1'
  let library: string | null = mode === 'new' ? null : firstHome
  const collections: Collection[] = mode === 'new' || mode === 'empty' ? [] : FOUND.slice(0, 5).map((make) => make())
  const sources: {
    id: number
    name: string
    root: string
    items: number
    bytes: number
    bucket?: string
    setAsideFrom?: string
    setAsideSaid?: string
    waitingFrom?: string
    waitingSaid?: string
  }[] =
    collections.length > 0 ? [{ id: 1, name: 'Family hard drive', root: 'E:\\', items: 240, bytes: 1.4 * 1024 ** 3 }] : []
  // The sources backed up by themselves whenever they are plugged in.
  const onArrival = new Set<number>()
  // The files inside each collection are made up the first time it is opened.
  const inside = new Map<string, PhotoChoice[]>()
  const filesOf = (c: Collection) => {
    let files = inside.get(c.key)
    if (!files) {
      const more = Math.min(c.photos + c.videos, 12) - c.previews.length
      const all = [...c.previews, ...Array.from({ length: Math.max(more, 0) }, (_, i) => photo(`IMG_${2000 + i}.jpg`))]
      files = all.map((p, i) => ({ ...p, taken: `2008-08-${String(12 + (i % 9)).padStart(2, '0')}`, kept: c.kept, junkReason: null }))
      inside.set(c.key, files)
    }
    return files
  }
  // Every source turns up a few pictures that are not memories.
  const junkFound = (): PhotoChoice[] => [
    { ...photo('save_icon.png'), taken: null, kept: false, junkReason: 'Looks like app icons' },
    { ...photo('thumb_0001.jpg'), taken: null, kept: false, junkReason: 'Tiny images (under 300 px)' },
    { ...photo('1952 scan.png'), taken: null, kept: false, junkReason: 'Tiny images (under 300 px)' },
  ]
  const junk: PhotoChoice[] = collections.length === 0 ? [] : junkFound()
  // Other files found by the last look, by ending; each file's row, once made.
  const carriedFound = new Set<string>()
  const otherRows = new Map<string, PhotoChoice>()
  const otherSent = new Set<string>()
  const endingOfName = (name: string) => name.split('.').pop()?.toLowerCase() ?? ''
  // The backups whose sources have been looked through for music.
  const musicIn = new Set<string>()
  const wantsMusic = () => backupWants.includes('music') || [...sourceChoices.values()].some((w) => w.includes('music'))
  const musicFound = () => open !== null && musicIn.has(open) && wantsMusic()
  const folderRows = (root: string, folder: string[], files: [string, number][], music: boolean) =>
    files.map(([name, bytes]) => {
      const where = `${root}\\${[...folder, name].join('\\')}`
      let row = otherRows.get(where)
      if (!row) {
        const id = (nextId++).toString(16).padStart(16, '0')
        row = {
          id,
          isVideo: false,
          ...(music ? { isMusic: true } : { isOther: true }),
          name,
          sourcePath: where,
          thumbnail: null,
          taken: null,
          changed: '2019-03-03',
          bytes,
          kept: true,
          junkReason: null,
        }
        otherRows.set(where, row)
      }
      return row
    })
  const folderCollection = (key: string, folder: string[], rows: PhotoChoice[], music: boolean): Collection => {
    const n = rows.length
    return {
      key,
      title: folder[folder.length - 1],
      summary: music ? `${n} track${n === 1 ? '' : 's'}` : `${n} other file${n === 1 ? '' : 's'}`,
      parents: folder.slice(0, -1),
      photos: 0,
      videos: 0,
      ...(music ? { music: n } : { others: n }),
      byFolder: true,
      bytes: rows.reduce((t, r) => t + (r.bytes ?? 0), 0),
      kept: rows.some((r) => r.kept),
      gathered: rows.filter((r) => r.kept && otherSent.has(r.id)).length,
      cover: rows[0],
      previews: [],
    }
  }
  const otherFolders = (): Collection[] => {
    const carried =
      sources.length === 0 || !backupWants.includes('others')
        ? []
        : OTHER_FOLDERS.flatMap(({ folder, files }) => {
            const found = files.filter(([name]) => carriedFound.has(endingOfName(name)))
            if (found.length === 0) return []
            return [folderCollection(['Other files', ...folder].join(' / '), folder, folderRows('E:', folder, found, false), false)]
          })
    // Music, by album, as the real search groups it: its folder.
    const music = musicFound()
      ? MUSIC_ALBUMS.map(({ folder, files }) =>
          folderCollection(['Music', ...folder].join(' / '), folder, folderRows(MUSIC_ROOT, folder, files, true), true),
        )
      : []
    return [...carried, ...music]
  }
  const otherRowsIn = (key: string) => {
    const folder = OTHER_FOLDERS.find((f) => ['Other files', ...f.folder].join(' / ') === key)
    if (folder) return [...otherRows.values()].filter((r) => r.sourcePath === `E:\\${[...folder.folder, r.name].join('\\')}`)
    const album = MUSIC_ALBUMS.find((f) => ['Music', ...f.folder].join(' / ') === key)
    return album ? [...otherRows.values()].filter((r) => r.sourcePath === `${MUSIC_ROOT}\\${[...album.folder, r.name].join('\\')}`) : []
  }
  const scanListeners = new Set<(e: ScanEvent) => void>()
  const surveyListeners = new Set<(e: SurveyProgress) => void>()
  // The places surveyed, so looking inside one surveys it first, as the real one does.
  const surveyed = new Set<string>()
  // `?old=disk` or `?old=network`: the source's drive was mapped days ago,
  // so opening its map surveys it again by itself, or asks first.
  const old = new URLSearchParams(window.location.search).get('old')
  if (old) surveyed.add('E:')
  const mappedAgain = new Set<string>()
  // Folders left out of the backup, and whether a path is in one (case
  // ignored, as on Windows; never fooled by "Photos 2" beside "Photos").
  const leftOutFolders = new Set<string>()
  const within = (path: string, folder: string) => {
    const [p, f] = [path.toLowerCase().replace(/\\+$/, ''), folder.toLowerCase().replace(/\\+$/, '')]
    return p === f || p.startsWith(`${f}\\`)
  }
  const leftOutOf = (path: string) => [...leftOutFolders].find((f) => within(path, f))
  const checkListeners = new Set<(event: CheckEvent) => void>()
  const sendListeners = new Set<(e: SendEvent) => void>()
  // What the last backup said when it ended, as the real app keeps it.
  // `?stoppedBefore` pretends the last one was stopped part-way.
  let lastSaid: SendEvent | null = new URLSearchParams(window.location.search).has('stoppedBefore')
    ? {
        finished: true,
        summary: [
          'Safely backed up 40 files.',
          'Stopped. Everything backed up so far is safe where it is. Back up again whenever you like and it carries on from here.',
        ],
      }
    : null
  sendListeners.add((e) => {
    if (e.finished) lastSaid = e
  })
  // Where the open backup goes: each place, and how many files are there.
  type Place = {
    name: string
    kind: string
    folder: string | null
    there: number
    /** Files a look found gone from it, until a backup sends them again. */
    gone?: number
    /** Files a look found there at another size than they were sent at. */
    notAsSent?: number
    /** The last look could not reach it. */
    outOfReach?: boolean
    /** The last look found its folder on another disk and empty: the drive left it behind. */
    otherDisk?: boolean
  }
  // A test empties the first destination, or unplugs it, by hand, as a
  // person might; Keevoro finds out the next time it looks there.
  let emptyOnLook = false
  let unplugOnLook = false
  let leaveEmptyOnLook = false
  const byHand = window as unknown as {
    keevoroEmptyTheFolder: () => void
    keevoroUnplugTheFolder: () => void
    keevoroUnplugLeavingTheFolder: () => void
  }
  byHand.keevoroEmptyTheFolder = () => (emptyOnLook = true)
  byHand.keevoroUnplugTheFolder = () => (unplugOnLook = true)
  // The next backup's share drops out for a stretch, as the owner's did.
  let dropNextBackup = false
  ;(window as unknown as { keevoroDropTheShareNextBackup: () => void }).keevoroDropTheShareNextBackup = () =>
    (dropNextBackup = true)
  // As the owner's network share was left: some files gone, some cut short.
  let damageOnLook = false
  ;(window as unknown as { keevoroDamageTheShare: () => void }).keevoroDamageTheShare = () => (damageOnLook = true)
  // On a Mac or Linux an unplugged drive can leave the folder it was
  // mounted on behind, empty, on the computer's own disk.
  byHand.keevoroUnplugLeavingTheFolder = () => (leaveEmptyOnLook = true)
  // Photos copied onto the first source since it was last looked through,
  // which the next backup finds before it sends.
  let addedSinceTheLook = 0
  ;(window as unknown as { keevoroAddPhotos: (n: number) => void }).keevoroAddPhotos = (n) => (addedSinceTheLook += n)
  // What Keevoro would say of a destination it has just looked at, in the
  // words keevoro_engine::confirm uses.
  const lookedAt = (d: Place) => {
    const now = Date.now() / 1000
    if (d.otherDisk) {
      return {
        at: now,
        lastReached: now - 2 * 86_400,
        fresh: false,
        reached: false,
        gone: 0,
        notAsSent: 0,
        otherDisk: true,
        said: [
          `The drive this backup goes to does not seem to be plugged in: ${d.folder} is there, but on another disk, and holds nothing Keevoro backed up, so nothing is sent there. Plug the drive in, or, if this disk is where the backup should go from now on, choose "Use this folder as it is now". Until Keevoro can reach ${d.name}, what is there can't be confirmed: everything is counted as it was when Keevoro last reached it, 2 days ago.`,
        ],
      }
    }
    if (d.outOfReach) {
      return {
        at: now,
        lastReached: now - 2 * 86_400,
        fresh: false,
        reached: false,
        gone: 0,
        notAsSent: 0,
        said: [
          `Keevoro can't find the folder ${d.folder}. Is it plugged in? Until Keevoro can reach ${d.name}, what is there can't be confirmed: everything is counted as it was when Keevoro last reached it, 2 days ago.`,
        ],
      }
    }
    const gone = d.gone ?? 0
    const notAsSent = d.notAsSent ?? 0
    const name = d.name.charAt(0).toUpperCase() + d.name.slice(1)
    const sent = d.there + gone + notAsSent
    const n = (x: number) => x.toLocaleString('en-GB')
    const said: string[] = []
    if (gone > 0 && d.there === 0 && notAsSent === 0) {
      said.push(`${name} looks emptied or replaced: it no longer holds any of the ${n(gone)} files Keevoro backed up there. They will be sent again.`)
    } else if (gone > 0) {
      said.push(`${name} no longer holds ${n(gone)} of the ${n(sent)} files Keevoro backed up there. ${gone === 1 ? 'It' : 'They'} will be sent again.`)
    }
    if (notAsSent > 0) {
      said.push(
        `${name} has ${n(notAsSent)} files under the names Keevoro gave them, but not the sizes they were backed up at, so they are not counted as safe. Keevoro never replaces a file in a backup: "Put this backup right" moves those copies aside there, into a folder of their own, then sends them again and proves each one.`,
      )
    }
    return { at: now, lastReached: now, fresh: true, reached: true, gone, notAsSent, said }
  }
  let dests: Place[] = []
  let sendingTo: string | null = null
  const said = () => {
    sendingTo = dests.length === 0 ? null : dests.length === 1 ? dests[0].name : `${dests.length} places`
  }
  // A backup that already goes somewhere, for `?byitself`.
  if (new URLSearchParams(window.location.search).has('byitself')) {
    dests = [{ name: 'the folder \\\\NAS\\Photos', kind: 'folder', folder: '\\\\NAS\\Photos', there: 0 }]
    said()
  }
  let stopAsked = false
  let stopLooking = false
  // Pictures of files in a folder being looked inside, a few at a time.
  const looks = new PictureQueue()
  // How many have been made, so a test can see that only what is on screen was fetched.
  let looked = 0
  ;(window as unknown as { keevoroPicturesMade: () => number }).keevoroPicturesMade = () => looked
  // Folders holding a stopped backup's working files, and how many.
  const untidy = new Map<string, number>([
    ['F:\\Family Backup', 12],
    ['G:\\Old Keevoro backup', 211],
  ])
  let systemCodecs = true
  let agents = { lookIn: [] as string[], backUpTo: [] as string[], mayChange: false }
  let previews = true
  // What the backup is for, and each source's own choice, as keevoro_core::Wants says them.
  let backupWants: Wants = ['photos', 'videos', 'others']
  const sourceChoices = new Map<number, Wants>()
  // Other files are only spoken of once a kind of them is on the list.
  const othersListed = () => lookedFor.some((k) => k.kind === 'other')
  const listedAsOther = (ending: string) => lookedFor.some((k) => k.kind === 'other' && k.on && k.ending === ending)
  const everything = (wants: Wants) =>
    wants.includes('photos') && wants.includes('videos') && (wants.includes('others') || !othersListed())
  const words = (wants: Wants) => {
    const named = [
      wants.includes('photos') ? 'photos' : '',
      wants.includes('videos') ? 'videos' : '',
      wants.includes('music') ? 'music' : '',
      wants.includes('others') && othersListed() ? 'other files' : '',
    ].filter(Boolean)
    const joined = named.length === 0 ? 'other files' : named.length === 1 ? named[0] : `${named.slice(0, -1).join(', ')} and ${named[named.length - 1]}`
    return everything(wants) ? joined : `${joined} only`
  }
  // How the backup arranges what it sends; null until someone chooses,
  // which a backup that existed before the choice never needs to.
  let arrange: Arrange | null = mode === 'new' || mode === 'scans' ? null : 'byDate'
  const sourceArranges = new Map<number, Arrange>()
  // The tester's folder of scans: 240 scanned that one day, in folders by family.
  const SCANS_ID = 7
  const scansRoot = 'D:\\Pictures\\Scans\\Family Scans'
  const scansByDate = [collection('14 March 2021', '', 240, 0)]
  const scansKept = [
    collection('Family Scans', '', 34, 0),
    collection('Jones', '', 86, 0, ['Family Scans']),
    collection('Smith', '', 120, 0, ['Family Scans']),
  ].map((c) => ({ ...c, key: `Kept as they are / ${c.key}`, keptFolders: true }))
  const scanKeys = new Set([...scansByDate, ...scansKept].map((c) => c.key))
  const arrangementOf = (id: number): Arrange => sourceArranges.get(id) ?? arrange ?? 'byDate'
  // The scans' collections follow how their source is arranged.
  const arrangeScans = () => {
    for (let i = collections.length - 1; i >= 0; i--) if (scanKeys.has(collections[i].key)) collections.splice(i, 1)
    if (!sources.some((s) => s.id === SCANS_ID)) return
    collections.push(...(arrangementOf(SCANS_ID) === 'folders' ? scansKept : scansByDate))
  }
  const topOf = (s: { name: string; root: string }) =>
    /^[A-Za-z]:\\?$/.test(s.root) ? s.name.replace(/ \([A-Za-z]:\)$/, '') : (s.root.split('\\').filter(Boolean).pop() ?? s.name)
  const suggestionNow = (): ArrangeView['suggestion'] => {
    const scans = sources.find((s) => s.id === SCANS_ID)
    if (scans) return { arrange: 'folders', said: `Suggested because the photos in ${scans.name} look like scans.` }
    const card = sources.find((s) => s.name.startsWith('KINGSTON'))
    if (card) return { arrange: 'byDate', said: `Suggested because the photos in ${card.name} look like they came straight from a camera.` }
    return undefined
  }
  const arrangeView = (said?: string): ArrangeView => {
    const suggestion = suggestionNow()
    return {
      arrange: arrange ?? 'byDate',
      asked: arrange !== null || sentSoFar > 0,
      said: said ?? (arrange === 'folders' ? 'This backup keeps your folders as they are.' : 'This backup is arranged by date.'),
      ...(suggestion ? { suggestion } : {}),
      ...(sources.some((s) => s.id === SCANS_ID) && sources.length > 1 ? { sourcesDiffer: true } : {}),
      topFolders: sources.map(topOf),
    }
  }
  const sourceArrangeView = (id: number, changed = false): SourceArrangeView => {
    const source = sources.find((s) => s.id === id)
    if (!source) throw new Error('That source isn\u2019t part of this backup.')
    const own = sourceArranges.has(id)
    const now = arrangementOf(id)
    const said = own
      ? now === 'folders'
        ? `Files from ${source.name} keep their folders as they are.`
        : `Files from ${source.name} are arranged by date.`
      : `Files from ${source.name} are arranged as the backup is: ${now === 'folders' ? 'keeping your folders as they are' : 'by date'}.`
    const suggested = id === SCANS_ID && now !== 'folders'
    return {
      arrange: now,
      own,
      ...(now === 'folders' ? { keptAs: topOf(source) } : {}),
      said: changed ? `${said} From now on its new files are ${now === 'folders' ? 'kept in its folders' : 'arranged by date'}; files already backed up stay where they are.` : said,
      ...(suggested ? { suggestion: { arrange: 'folders' as const, said: 'Suggestion: keep its folders as they are. These look like scans.' } } : {}),
    }
  }
  const safe = ' Anything already backed up stays backed up; it is just no longer kept in what this backup chooses.'
  const wantsView = (wants: Wants, own?: boolean, changed = false, before?: Wants, looksAgain = false) => ({
    wants,
    own,
    ...(othersListed() ? { offersOthers: true } : {}),
    said: `This backup is for ${words(wants)}.${changed && before && !before.every((w) => wants.includes(w)) ? safe : ''}${
      looksAgain ? ' Look through your sources again to find your music.' : ''
    }`,
    ...(looksAgain ? { looksAgain: true } : {}),
  })
  const sourceView = (id: number, changed = false) => {
    const own = sourceChoices.get(id)
    const wants = own ?? backupWants
    return {
      wants,
      own: own !== undefined,
      ...(othersListed() ? { offersOthers: true } : {}),
      said:
        (own ? `This source is set to ${words(own)}.` : `This source follows the backup: ${words(wants)}.`) +
        (changed && !everything(wants) ? safe : ''),
    }
  }
  // The kinds of file looked for, as Keevoro comes with them (the same
  // list as keevoro-core's, which a test there keeps in step), and one added.
  const builtInKinds = (): FileKind[] =>
    builtInKindList.map((k) => ({
      ending: k.ending,
      kind: k.kind as ListedAs,
      on: !k.unsure,
      builtIn: true,
      usual: k.usual,
      unsure: k.unsure,
      cameAs: k.kind as ListedAs,
      about: k.about,
    }))
  const ownKind = (ending: string, kind: ListedAs): FileKind => ({ ending, kind, on: true, builtIn: false, usual: true, unsure: false })
  let lookedFor: FileKind[] = [...builtInKinds(), ownKind('xyz', 'photo')]
  const plainEnding = (typed: string) => {
    const ending = typed.trim().replace(/^\./, '').toLowerCase()
    if (!ending) throw new Error('Type the ending of the files, such as heif.')
    if (!/^[\p{L}\p{N}]+$/u.test(ending))
      throw new Error(`"${typed.trim()}" can’t be the ending of a file name. Use letters and numbers only, with no spaces or dots, such as heif.`)
    return ending
  }
  // A document, music, a program or an archive is never a photo or video.
  const refuseAsMedia = (ending: string, kind: FileKind['kind']) => {
    if ((kind === 'photo' || kind === 'video') && lookedFor.some((k) => k.builtIn && k.cameAs === 'music' && k.ending === ending))
      throw new Error(`Files ending .${ending} are music, never photos or videos. To back them up, switch on Music for this backup.`)
    if (kind !== 'other' && kind !== 'music' && ['exe', 'pdf', 'zip', 'txt', 'docx'].includes(ending))
      throw new Error(`Files ending .${ending} are programs, documents or archives, never photos or videos. To back them up as they are, add .${ending} as an other file.`)
  }
  const kindNamed = (typed: string) => {
    const found = lookedFor.find((k) => k.ending === typed.trim().replace(/^\./, '').toLowerCase())
    if (!found) throw new Error(`Keevoro doesn’t have .${typed} files on its list.`)
    return found
  }
  const speed = () => ({
    on: systemCodecs,
    available: true,
    ways: [
      ...(systemCodecs ? ['this computer’s own codecs'] : []),
      'the built-in decoder',
      'the picture inside the raw file',
    ],
    cores: 4,
  })
  // Earlier backups, so there is something to compare a new one with: the
  // owner's real long one to a network folder, with everything recorded,
  // and an older one from before the detail was kept.
  const GB = 1024 ** 3
  const MB = 1024 ** 2
  const nas = 'the folder \\\\NAS\\Photos'
  const sends: Sent[] = [
    {
      finishedAt: Date.UTC(2026, 10, 19, 7, 41) / 1000,
      seconds: 34_800,
      bytes: 316.2 * GB,
      to: nas,
      stopped: false,
      detail: {
        startedAt: Date.UTC(2026, 10, 18, 22, 1) / 1000,
        pausedSeconds: 600,
        ended: 'finished',
        startedBy: 'the app',
        version: '0.32.0',
        stages: [
          { kind: 'tellingApart', unit: 'files', done: 214, seconds: 480, finished: true, perSecond: 214 / 480, slowest: 0.31, fastest: 0.62 },
          { kind: 'placing', unit: 'files', done: 8_630, seconds: 40, finished: true, perSecond: 8_630 / 40, slowest: null, fastest: null },
          { kind: 'sendingOriginals', to: nas, unit: 'bytes', done: 312.4 * GB, seconds: 32_400, finished: true, perSecond: (312.4 * GB) / 32_400, slowest: 3.2 * MB, fastest: 14.6 * MB, problem: 'Couldn’t back up 2 files' },
          { kind: 'makingPreviews', unit: 'files', done: 8_410, seconds: 9_000, finished: true, perSecond: 8_410 / 9_000, slowest: 0.21, fastest: 1.9 },
          { kind: 'makingVideoPreviews', unit: 'files', done: 349, seconds: 2_400, finished: true, perSecond: 349 / 2_400, slowest: 0.09, fastest: 0.2 },
          { kind: 'sendingPreviews', to: nas, unit: 'bytes', done: 3.8 * GB, seconds: 4_800, finished: true, perSecond: (3.8 * GB) / 4_800, slowest: 0.6 * MB, fastest: 1.1 * MB },
          { kind: 'writingIndex', to: nas, unit: 'files', done: 1, seconds: 12, finished: true, perSecond: 1 / 12, slowest: null, fastest: null },
        ],
        destinations: [
          {
            to: nas,
            overNetwork: true,
            reached: true,
            seconds: 34_800,
            bytes: 316.2 * GB,
            originals: {
              sent: 8_628,
              sentBytes: 312.4 * GB,
              alreadyThere: 0,
              notSent: [
                { file: 'originals/2011/Summer/IMG_4410.JPG', reason: 'The source couldn’t be read: it may have been unplugged' },
                { file: 'originals/2011/Summer/IMG_4411.JPG', reason: 'The source couldn’t be read: it may have been unplugged' },
              ],
              notSentMore: 0,
            },
            previews: { sent: 16_820, sentBytes: 3.8 * GB, alreadyThere: 0, notSent: [], notSentMore: 0 },
            indexed: 8_628,
          },
        ],
        duplicates: 1_118,
        duplicateBytes: 27.5 * GB,
        previews: { photos: 8_410, videos: 349, photosWithout: 12, videosWithout: 0 },
        summary: [
          'Safely backed up 8,628 files (312.4 GB).',
          'Couldn’t back up 2 files:',
          '  originals/2011/Summer/IMG_4410.JPG: The source couldn’t be read: it may have been unplugged.',
          '  originals/2011/Summer/IMG_4411.JPG: The source couldn’t be read: it may have been unplugged.',
          'Back up again to try them once more.',
          'Previews added: 16,820 files (3.8 GB).',
          'That took about 9 hours 40 minutes: 316.2 GB at an average of 9.3 MB a second.',
        ],
      },
    },
    { finishedAt: Date.UTC(2026, 10, 9, 14, 2) / 1000, seconds: 41 * 60, bytes: 30.1 * GB, to: 'the folder F:\\Family Backup', stopped: false },
  ]
  // `?byitself` adds a backup that ran by itself, from the command line,
  // with what it passed over and what waits for the person's say.
  if (new URLSearchParams(window.location.search).has('byitself')) {
    sends.unshift({
      finishedAt: Date.UTC(2026, 10, 20, 2, 33) / 1000,
      seconds: 180,
      bytes: 96 * MB,
      to: nas,
      stopped: false,
      detail: {
        startedAt: Date.UTC(2026, 10, 20, 2, 30) / 1000,
        pausedSeconds: 0,
        ended: 'finished',
        startedBy: 'the command line',
        version: '0.42.0',
        stages: [
          { kind: 'sendingOriginals', to: nas, unit: 'bytes', done: 96 * MB, seconds: 150, finished: true, perSecond: (96 * MB) / 150, slowest: null, fastest: null },
        ],
        destinations: [
          {
            to: nas,
            overNetwork: true,
            reached: true,
            seconds: 150,
            bytes: 96 * MB,
            originals: { sent: 23, sentBytes: 96 * MB, alreadyThere: 8_628, notSent: [], notSentMore: 0 },
          },
        ],
        duplicates: 0,
        duplicateBytes: 0,
        summary: [
          'KINGSTON (E:) is not plugged in, so it was not looked through. What was found on it before is kept.',
          'Safely backed up 23 files (96.0 MB).',
        ],
        byItself: {
          why: 'for a system scheduler or a script',
          lookedAgain: ['Family hard drive'],
          passedOver: ['KINGSTON (E:) is not plugged in, so it was not looked through. What was found on it before is kept.'],
          found: ['New since the last look: 23 photos and 2 videos.'],
          waiting: ['2 new pictures were set aside as probably not photos, and not backed up. Look at them in Keevoro to keep any.'],
        },
      },
    })
  }
  // Backing up by itself: `?schedule` starts with one set, and its last
  // few runs, one of which could not reach the backup drive.
  const scheduled = new URLSearchParams(window.location.search).has('schedule')
  let scheduleChoice: ScheduleChoice | null = scheduled ? { v: 1, every: 'day', at: '02:30', paused: false } : null
  let skippingNext = false
  const hoursAgo = (h: number) => Math.floor(Date.now() / 1000) - h * 3600
  const scheduleLast: ScheduleAttempt[] = scheduled
    ? [
        { at: hoursAgo(6), why: 'on its schedule', outcome: 'worked', said: 'Safely backed up 23 files (96.0 MB).' },
        { at: hoursAgo(30), why: 'on its schedule', outcome: 'worked', said: 'Nothing new to back up.' },
        {
          at: hoursAgo(54),
          why: 'on its schedule',
          outcome: 'unreachable',
          said: 'Keevoro can’t find the folder F:\\Family Backup. Is it plugged in? It will catch up when it is.',
        },
      ]
    : []
  // "While you were away": `?away` opens with two backups that ran by
  // themselves while the window was closed, one catching up a night the
  // computer was off, one that could not reach its drive, and something
  // waiting for the person's say.
  let awayView: AwayView = new URLSearchParams(window.location.search).has('away')
    ? {
        since: hoursAgo(60),
        backups: [
          {
            backup: 'Family photos',
            id: 'backup-1',
            runs: [
              {
                at: hoursAgo(30),
                when: 'Yesterday at 08:05',
                outcome: 'worked',
                said: 'Safely backed up 23 files (96.0 MB).',
                caughtUp: 'This caught up a run that was missed, perhaps because the computer was off or asleep.',
              },
              { at: hoursAgo(6), when: 'Today at 02:31', outcome: 'worked', said: 'Nothing new to back up.' },
            ],
          },
          {
            backup: 'Holiday videos',
            id: 'backup-2',
            runs: [
              {
                at: hoursAgo(54),
                when: 'Tuesday at 02:30',
                outcome: 'unreachable',
                said: "Keevoro can't find the folder F:\\Family Backup. Is it plugged in?",
              },
            ],
            missed: "The run due today at 02:30 didn't happen, perhaps because the computer was off or asleep. It will run once, in a few minutes.",
          },
        ],
        waiting: [
          {
            backup: 'Family photos',
            id: 'backup-1',
            said: '4 new pictures were set aside as probably not photos, and not backed up. Look at them in Keevoro to keep any.',
          },
        ],
      }
    : { since: hoursAgo(1), backups: [], waiting: [] }
  // How a backup by itself waits; `?waiting` opens with the run due
  // waiting while the computer is on battery.
  let manners: MannersView = { waitOnBattery: true, waitOnMetered: true, meteredKnown: true }
  let waitingNow: string | null = new URLSearchParams(window.location.search).has('waiting')
    ? 'Waiting while this computer is on battery'
    : null
  const ordinalOf = (n: number) =>
    `${n}${n % 100 >= 11 && n % 100 <= 13 ? 'th' : n % 10 === 1 ? 'st' : n % 10 === 2 ? 'nd' : n % 10 === 3 ? 'rd' : 'th'}`
  const DAYS: Record<string, string> = { mon: 'Monday', tue: 'Tuesday', wed: 'Wednesday', thu: 'Thursday', fri: 'Friday', sat: 'Saturday', sun: 'Sunday' }
  const scheduleView = (): ScheduleView => {
    const c = scheduleChoice
    if (!c) return { paused: false, skipping: false, last: scheduleLast }
    const every =
      c.every === 'day'
        ? `Every day at ${c.at}`
        : c.every === 'week'
          ? `Every week on ${DAYS[c.weekday ?? 'sun']} at ${c.at}`
          : `Every month on the ${ordinalOf(c.day ?? 1)}${(c.day ?? 1) > 28 ? ' (or the last day of shorter months)' : ''} at ${c.at}`
    const next =
      c.every === 'day' ? `Tonight at ${c.at}` : c.every === 'week' ? `${DAYS[c.weekday ?? 'sun']} at ${c.at}` : `On ${c.day ?? 1} December at ${c.at}`
    return {
      every,
      choice: c,
      paused: c.paused,
      ...(c.paused ? {} : { next, nextAt: hoursAgo(-8) }),
      skipping: skippingNext && !c.paused,
      ...(waitingNow && !c.paused ? { waiting: waitingNow } : {}),
      last: scheduleLast,
    }
  }
  let pausedNow = false
  let tick = 0
  const usage: Sample[] = Array.from({ length: 60 }, (_, i) => ({
    cpu: 11 + 6 * Math.sin(i / 5),
    disk: 78 + 22 * Math.sin(i / 3),
    sent: 12 + 4 * Math.sin(i / 4),
    network: 12 + 4 * Math.sin(i / 4),
    memory: 520 + 40 * Math.sin(i / 9),
  }))
  const kinds: FileType[] = [
    { ending: 'jpg', files: 198, bytes: 620 * 1024 * 1024, included: true },
    { ending: 'dng', files: 29, bytes: 1.4 * 1024 ** 3, included: true },
    { ending: 'mp4', files: 11, bytes: 530 * 1024 * 1024, included: true },
    // A less common kind, found all the same.
    { ending: 'psd', files: 4, bytes: 210 * 1024 * 1024, included: true },
  ]
  // What is kept but not yet sent, at a made-up 3.2 MB a photo and 48 MB a video.
  const waitingBytes = () =>
    collections
      .filter((c) => c.kept && c.gathered < c.photos + c.videos)
      .reduce((n, c) => n + (c.photos * 3.2 + c.videos * 48) * 1024 * 1024, 0)
  let sentSoFar = 0
  // The send in hand, as the real app's job keeps it: when it started, so
  // "Running for" carries on across screens, and how far it has got.
  // `?sending` pretends one began two minutes before the page opened.
  const sendingAtOpen = new URLSearchParams(window.location.search).has('sending')
  let sendStarted: number | null = sendingAtOpen ? Date.now() - 2 * 60 * 1000 : null
  let sendShare = 0
  const runningFor = (seconds: number) => {
    const minutes = Math.floor(seconds / 60)
    if (minutes === 0) return 'less than a minute'
    const [h, m] = [Math.floor(minutes / 60), minutes % 60]
    const unit = (n: number, word: string) => `${n} ${word}${n === 1 ? '' : 's'}`
    return h === 0 ? unit(m, 'minute') : m === 0 ? unit(h, 'hour') : `${unit(h, 'hour')} ${unit(m, 'minute')}`
  }
  // A made-up 30 GB backup at a steady 12.4 MB a second, with previews made
  // alongside, as `keevoro_core::estimate` would say it.
  const demoEstimate = (runningSeconds: number): BackupEstimate => {
    const GB = 1024 ** 3
    const stage = (s: Omit<BackupStage, 'to' | 'perSecond'> & { perSecond?: number }): BackupStage => ({
      to: null,
      perSecond: null,
      ...s,
    })
    const stages: BackupStage[] = sendingAtOpen
      ? [
          stage({ kind: 'placing', state: 'now', unit: 'files', done: 2500, work: 8630, line: 'Working out where each file goes: 2,500 of 8,630' }),
          stage({ kind: 'sendingOriginals', state: 'next', unit: 'bytes', done: 0, work: 30.1 * GB, line: 'Backing up your files: 30.1 GB' }),
          stage({ kind: 'makingPreviews', state: 'next', unit: 'files', done: 0, work: 10547, line: 'Making previews: 10,547' }),
          stage({ kind: 'sendingPreviews', state: 'next', unit: 'bytes', done: 0, work: 4 * GB, line: 'Backing up the previews: 4.0 GB' }),
          stage({ kind: 'writingIndex', state: 'next', unit: 'files', done: 0, work: 1, line: 'Writing the list of what is there' }),
        ]
      : [
          stage({ kind: 'placing', state: 'done', unit: 'files', done: 8630, work: 8630, line: 'Working out where each file goes' }),
          stage({
            kind: 'sendingOriginals',
            state: 'now',
            unit: 'bytes',
            done: (9.6 + 20 * sendShare) * GB,
            work: 30.1 * GB,
            perSecond: 12.4 * 1024 * 1024,
            line: `Backing up your files: ${(9.6 + 20 * sendShare).toFixed(1)} GB of 30.1 GB at 12.4 MB/s`,
          }),
          stage({
            kind: 'makingPreviews',
            state: 'alongside',
            unit: 'files',
            done: Math.round(3200 + 6000 * sendShare),
            work: 10547,
            line: `Making previews in the background: ${Math.round(3200 + 6000 * sendShare).toLocaleString('en-GB')} of 10,547`,
          }),
          stage({ kind: 'sendingPreviews', state: 'next', unit: 'bytes', done: 0, work: 4 * GB, line: 'Backing up the previews: 4.0 GB' }),
          stage({ kind: 'writingIndex', state: 'next', unit: 'files', done: 0, work: 1, line: 'Writing the list of what is there' }),
        ]
    const secondsLeft = Math.round(((30.1 - 9.6 - 20 * sendShare) * 1024) / 12.4 + 330)
    const left = `About ${Math.round(secondsLeft / 60)} minutes left`
    return {
      secondsLeft,
      sureness: 'fair',
      left,
      fractionDone: Math.min(0.32 + 0.6 * sendShare, 0.99),
      stages,
      pausedSeconds: 0,
      headline: `Running for ${runningFor(runningSeconds)}, ${left.toLowerCase()}`,
    }
  }
  const later = (ms: number) => new Promise((done) => setTimeout(done, ms))
  // Reports sent from this computer, newest first: two from before.
  let reportRefused = false
  // The tokens the window was given for reports it sent itself.
  const tokens = new Map<string, string>()
  // The reporting code kept in this computer's keychain, as Keevoro keeps
  // it: never given back. The stand-in service recognises the made-up
  // `TESTER-1234` and no other.
  let reportingCode: string | null = null
  const DEMO_CODE = 'TESTER-1234'
  const codeSaid = (recognised: boolean | undefined) =>
    recognised === undefined
      ? {}
      : {
          recognised,
          codeSaid: recognised
            ? 'Your reporting code was recognised.'
            : 'That reporting code was not recognised, so this was sent as an ordinary report. Check the code with whoever gave it to you.',
        }
  const reportsSent: ReportListed[] = [
    // Sent from a version that keeps the whole message, to open and read again.
    {
      reference: 'K-1041',
      kind: 'idea',
      firstLine: 'Could the map of where the space goes be calmer?',
      message:
        'Could the map of where the space goes be calmer?\nThe bright colours make it hard to read.\n\nSofter ones would suit it better.',
      appVersion: '9.7.0',
      screen: 'Where the space goes',
      withContact: true,
      sentAt: 1_794_657_600,
      status: 'shipped',
      statusSaid: 'Done in version 0.47.0',
      shippedIn: '0.47.0',
      note: 'Thank you. The map is quieter now.',
    },
    // Sent before whole messages were kept: only its first line.
    {
      reference: 'K-1040',
      kind: 'problem',
      firstLine: 'The previews of Family photos were very slow',
      keptSaid: 'Only the first line was kept for reports sent before this version.',
      sentAt: 1_793_620_800,
      status: 'in_progress',
      statusSaid: 'Being worked on',
    },
  ]
  // A report as Keevoro keeps it once sent: the whole message and the few
  // facts about it, only whether the log and a way to reach them went; and
  // the thank you's last sentence, that no email comes.
  const keptReport = (prepared: string, reference: string, recognised: boolean | undefined): ReportListed => {
    const report = JSON.parse(prepared) as {
      kind: 'problem' | 'idea'
      message: string
      appVersion: string
      screen?: string
      logTail?: string
      contact?: string
    }
    return {
      reference,
      kind: report.kind,
      firstLine: report.message.split('\n')[0].slice(0, 80),
      message: report.message,
      appVersion: report.appVersion,
      ...(report.screen ? { screen: report.screen } : {}),
      ...(report.logTail ? { withLog: true } : {}),
      ...(report.contact ? { withContact: true } : {}),
      sentAt: Math.floor(Date.now() / 1000),
      status: 'new',
      statusSaid: 'Received',
      ...(recognised ? { recognised } : {}),
    }
  }
  const emailSaid = (prepared: string) =>
    (JSON.parse(prepared) as { contact?: string }).contact
      ? "Keevoro does not send an email to confirm. This report is listed under Your reports below, and Keevoro's maker may write to you about it."
      : 'Keevoro does not send emails. This report is listed under Your reports below, where you can see how it is getting on.'
  // Tests can ask for everything to happen at once.
  const pace = mode === 'fast' || new URLSearchParams(window.location.search).has('fast') ? 0 : 1

  // Backups: each keeps what it found, what was chosen and where it sends.
  type Kept = {
    collections: Collection[]
    sources: typeof sources
    junk: PhotoChoice[]
    dests: Place[]
    sentSoFar: number
    wants?: Wants
    arrange?: Arrange | null
  }
  let backups: Backup[] = library ? [{ id: 's1', name: 'My photos' }] : []
  let open: string | null = backups[0]?.id ?? null
  let made = backups.length
  const shelf = new Map<string, Kept>()
  const putAway = () => {
    if (open)
      shelf.set(open, { collections: [...collections], sources: [...sources], junk: [...junk], dests: [...dests], sentSoFar, wants: backupWants, arrange })
  }
  const takeOut = (id: string) => {
    const kept = shelf.get(id) ?? { collections: [], sources: [], junk: [], dests: [], sentSoFar: 0 }
    collections.splice(0, collections.length, ...kept.collections)
    sources.splice(0, sources.length, ...kept.sources)
    junk.splice(0, junk.length, ...kept.junk)
    dests = [...kept.dests]
    said()
    sentSoFar = kept.sentSoFar
    if (kept.wants) backupWants = kept.wants
    if (kept.arrange !== undefined) arrange = kept.arrange
    open = id
  }
  // The drives plugged in. With `?plugin`, a memory card arrives a moment
  // after the page opens, as one pushed into a reader would.
  const openedAt = Date.now()
  const drivesNow = (): Drive[] => {
    const card: Drive[] =
      new URLSearchParams(window.location.search).has('plugin') && Date.now() - openedAt > 1500
        ? [{ path: 'F:\\', name: 'SanDisk Ultra (F:)', detail: 'USB or external drive \u00b7 32 GB', removable: true, askFirst: false }]
        : []
    return [
      { path: 'E:\\', name: 'KINGSTON (E:)', detail: 'USB or external drive \u00b7 32 GB', removable: true, askFirst: false },
      ...card,
      {
        path: 'C:\\Users\\You\\OneDrive',
        name: 'OneDrive',
        detail: 'Synced with Microsoft\u2019s cloud \u00b7 C:\\Users\\You\\OneDrive',
        removable: false,
        askFirst: false,
        synced: 'oneDrive',
      },
      { path: 'C:\\', name: 'Windows (C:)', detail: 'This computer\u2019s disk \u00b7 512 GB', removable: false, askFirst: true },
    ]
  }

  // Backups removed in the last week, with what each held, to put back.
  const removedRecently: { listed: RemovedBackup; backup: Backup; kept: Kept }[] = []
  const homeOf = (id: string) => `C:\\Users\\You\\AppData\\Roaming\\com.markcastle.keevoro\\backups\\${id}`
  const listOf = (things: string[]) =>
    things.length <= 1 ? (things[0] ?? '') : `${things.slice(0, -1).join(', ')} and ${things[things.length - 1]}`
  const keptOf = (id: string): Kept => {
    putAway()
    return shelf.get(id) ?? { collections: [], sources: [], junk: [], dests: [], sentSoFar: 0 }
  }
  const removalOf = (backup: Backup, done: boolean): Removal => {
    const kept = keptOf(backup.id)
    const photos = kept.collections.reduce((n, c) => n + c.photos + c.videos, 0)
    const room = 40 * 1024 * 1024 + photos * 90 * 1024
    const sourcesNow = kept.sources.map((s) => s.root)
    const destinationsNow = kept.dests.map((d) => d.name)
    return {
      id: backup.id,
      name: backup.name,
      removed: done,
      sources: sourcesNow,
      destinations: destinationsNow,
      ownFolder: homeOf(backup.id),
      ownFolderBytes: room,
      clearsOwnFolder: true,
      keysFor: [],
      ...(done ? { keptUntil: Date.now() / 1000 + 7 * 86_400 } : {}),
      said: [
        done
          ? `Removed “${backup.name}” from Keevoro: its sources, what it found, your choices and its history.`
          : `Keevoro's record of “${backup.name}” on this computer goes: its sources, what it found, your choices and its history.`,
        destinationsNow.length === 0
          ? 'It has not been set to back up anywhere, so there is nothing anywhere else to keep.'
          : `Everything already backed up to ${listOf(destinationsNow)} stays where it is. Keevoro does not touch it.`,
        sourcesNow.length === 0 ? 'Nothing on any source is touched.' : `Nothing on its sources (${listOf(sourcesNow)}) is touched.`,
        `Its previews and working files on this computer (${(room / 1024 / 1024).toFixed(1)} MB) are cleared away. You can put it back from Manage backups for 7 days; after that the room is freed.`,
      ],
    }
  }
  const state = (): AppState => ({
    library,
    backup: backups.find((s) => s.id === open) ?? null,
    backups: [...backups],
    version: 'demo',
  })
  const view = (): Discover => {
    const kept = collections.filter((c) => c.kept)
    const folders = otherFolders()
    const others = folders.reduce((n, c) => n + (c.others ?? 0), 0)
    const music = folders.reduce((n, c) => n + (c.music ?? 0), 0)
    const keptOthers = [...otherRows.values()].filter((r) => r.kept && folders.some((f) => otherRowsIn(f.key).includes(r)))
    const waitingOthers = keptOthers.filter((r) => !otherSent.has(r.id))
    return {
      photos: collections.reduce((n, c) => n + c.photos, 0),
      videos: collections.reduce((n, c) => n + c.videos, 0),
      ...(others > 0 ? { others } : {}),
      ...(music > 0 ? { music } : {}),
      bytes:
        collections.reduce((n, c) => n + (c.photos * 3.2 + c.videos * 48) * 1024 * 1024, 0) +
        folders.reduce((n, c) => n + (c.bytes ?? 0), 0),
      duplicates: collections.length * 31,
      duplicateBytes: collections.length * 31 * 3.4 * 1024 * 1024,
      junk: junk.length,
      waitingToGather: kept.reduce((n, c) => n + c.photos + c.videos - c.gathered, 0) + waitingOthers.length,
      waitingBytes: waitingBytes() + waitingOthers.reduce((n, r) => n + (r.bytes ?? 0), 0),
      sources: sources.map((s) => ({
        ...s,
        leftOut: [...leftOutFolders].filter((f) => within(f, s.root) || within(s.root, f)),
        ...(sourceArranges.has(s.id) ? { arrange: sourceArranges.get(s.id) } : {}),
        ...(arrangementOf(s.id) === 'folders' ? { keptAs: topOf(s) } : {}),
        ...(onArrival.has(s.id) ? { onArrival: true } : {}),
      })),
      destinations: dests.map((d) => {
        const all = kept.reduce((n, c) => n + c.photos + c.videos, 0) + keptOthers.length
        const there = Math.min(d.there, all)
        return {
          name: d.name,
          kind: d.kind,
          folder: d.folder,
          there,
          waiting: all - there,
          waitingBytes: (all - there) * 3.2 * 1024 * 1024,
          looked: lookedAt(d),
        }
      }),
      collections: [...collections.map((c) => ({ ...c })), ...folders],
    }
  }

  // Pretends to look through a place, finding collections as it goes.
  const look = (name: string, root: string) => {
      const forMusic = wantsMusic()
      const musicAlone = forMusic && !backupWants.includes('photos') && !backupWants.includes('videos')
      // A backup of music alone has nothing of the photos to show.
      const adding = musicAlone ? [] : FOUND.filter((_, i) => i >= collections.length).slice(0, sources.length === 0 ? 5 : 2)
      stopLooking = false
      void (async () => {
        let photos = 0
        let videos = 0
        const music = forMusic ? MUSIC_TRACKS : 0
        const added: Collection[] = []
        if (musicAlone) {
          for (let step = 1; step <= 4; step++) {
            await later(new URLSearchParams(window.location.search).has('slow') ? 400 : 220 * pace)
            scanListeners.forEach((l) =>
              l({
                name,
                photos: 0,
                videos: 0,
                music: Math.round((music * step) / 4),
                files: music * 3 * step,
                folder: `${root}\\${MUSIC_ALBUMS[Math.min(step, MUSIC_ALBUMS.length) - 1].folder.join('\\')}`,
                finished: false,
              }),
            )
          }
        }
        for (const make of adding) {
          const found = make()
          for (let step = 1; step <= 4; step++) {
            // `?slow` leaves time to press Stop.
            await later(new URLSearchParams(window.location.search).has('slow') ? 400 : 220 * pace)
            // `?unplug` pulls the drive out part-way: what was found so far is kept.
            if (new URLSearchParams(window.location.search).has('unplug') && added.length > 0) {
              const kept = { id: Date.now(), name, root, items: photos + videos, bytes: (photos * 3.2 + videos * 48) * 1024 * 1024 }
              if (!sources.some((s) => s.root === root)) sources.push(kept)
              scanListeners.forEach((l) =>
                l({
                  name,
                  photos,
                  videos,
                  finished: true,
                  unplugged: `${name} was unplugged. What was found so far is kept; plug it in and look again to carry on.`,
                }),
              )
              return
            }
            if (stopLooking) {
              // Stopped: nothing from this search is kept.
              added.forEach((c) => collections.splice(collections.indexOf(c), 1))
              scanListeners.forEach((l) => l({ name, photos, videos, finished: true, stopped: true }))
              // As in the real app, the check for duplicates of what was
              // found before still follows.
              if (collections.length > 0) void checkInBackground(collections.length * 8)
              return
            }
            scanListeners.forEach((l) =>
              l({
                name,
                photos: photos + Math.round((found.photos * step) / 4),
                videos,
                files: (photos + found.photos * step) * 37,
                folder: `${root}Users\\Family\\Pictures\\${found.title}`,
                finished: false,
              }),
            )
          }
          photos += found.photos
          videos += found.videos
          collections.unshift(found)
          added.push(found)
        }
        if (junk.length === 0 && !musicAlone) junk.push(...junkFound())
        // Other files of the kinds on the list now are found too, and music for a backup that is for it.
        lookedFor.filter((k) => k.kind === 'other' && k.on).forEach((k) => carriedFound.add(k.ending))
        if (forMusic && open) musicIn.add(open)
        // Looking through a source again replaces what was known about it.
        const known = sources.findIndex((s) => s.root === root)
        // The owner's Pictures folder is kept in OneDrive, part of it online only.
        const online = root.endsWith('\\Pictures')
          ? { onlineOnly: 412, onlineOnlySaid: onlineOnlySized(412, '18.4 GB'), onlineOnlyBytes: 19_756_849_561 }
          : {}
        // Cloud storage added as a source is said by where it is.
        const inCloud = bucketPlaces.get(root)
        // Some of an old Google Cloud Storage backup is kept in its colder classes.
        const colder =
          inCloud?.kind === 'gcs'
            ? { colderSaid: '96 files, 1.4 GB, are in Google’s Nearline, Coldline or Archive storage, which costs more to read out. Keevoro backs them up all the same.' }
            : {}
        const cloud = inCloud ? { bucket: inCloud.place, ...colder } : {}
        const entry = { id: known >= 0 ? sources[known].id : Date.now(), name: known >= 0 ? sources[known].name : inCloud ? root.split('/').pop() ?? name : name, root, items: photos + videos + music, bytes: (photos * 3.2 + videos * 48 + music * 20) * 1024 * 1024, ...online, ...cloud }
        if (known >= 0) sources[known] = entry
        else sources.push(entry)
        scanListeners.forEach((l) => l({ name, photos, videos, ...(music > 0 ? { music } : {}), finished: true, ...online }))
        // Then, as the real app does, the careful checking carries on behind.
        await checkInBackground(Math.max(1, Math.round((photos + videos) / 6)))
      })()
  }

  // The background check for duplicates: its own pause and stop, and once
  // stopped by the person it waits to be asked, or for the next search.
  let checkHeld = false
  let checkStopAsked = false
  let checkRunning = false
  let checkLeftOver: DuplicatesLeft | null = null
  let checkAt = { done: 0, total: 0 }
  const checkStoppedSaid = (left: number) => [
    `Stopped checking for duplicates, with ${left.toLocaleString('en-GB')} files not checked yet.`,
    'Until they are, the counts of copies may be a little out, but nothing is backed up twice or left out by mistake: before sending, a backup reads in full any file that looks like another.',
    'Checking starts again by itself after your next search or backup, or now if you ask.',
  ]
  const checkInBackground = async (total: number) => {
    if (checkRunning) return
    checkRunning = true
    checkStopAsked = false
    checkLeftOver = null
    // `?slowcheck` keeps it going long enough to pause and stop it.
    const step = new URLSearchParams(window.location.search).has('slowcheck') ? 600 : 300
    let done = 0
    while (done < total) {
      while (checkHeld && !checkStopAsked) await later(100)
      if (checkStopAsked) break
      checkAt = { done, total }
      checkListeners.forEach((l) => l({ done, total, finished: false }))
      // Long enough to be seen, however fast the rest is asked to go.
      await later(step)
      done = Math.min(total, done + Math.ceil(total / 5))
    }
    checkRunning = false
    checkHeld = false
    if (checkStopAsked) {
      const left = total - done
      checkLeftOver = { left, stopped: true, said: checkStoppedSaid(left) }
      checkListeners.forEach((l) => l({ done, total, finished: true, stopped: true, left, said: checkStoppedSaid(left) }))
    } else {
      checkListeners.forEach((l) => l({ done: total, total, finished: true }))
    }
  }

  // Opening a backup that is already somewhere. `?opening=` says what the
  // place holds: earlier (its newest notes damaged), damaged, newer, none
  // (from before notes were kept) or nothing; a notes file otherwise.
  const openedFrom = new Map<string, string>()
  const foundAt = (request: ConnectRequest): FoundBackup => {
    const place =
      request.kind === 'folder'
        ? `the folder ${request.place}`
        : request.kind === 'r2'
          ? `the Cloudflare R2 bucket ${request.bucket}`
          : request.kind === 'gcs'
            ? `the Google Cloud Storage bucket ${request.bucket}`
            : `the bucket ${request.bucket}`
    const asked = new URLSearchParams(window.location.search).get('opening')
    if (asked === 'newer')
      return {
        found: 'newer',
        place,
        madeBy: 'Keevoro 9.0.0',
        said: [
          'This backup’s notes were made by a newer Keevoro (Keevoro 9.0.0) than this one (Keevoro 0.49.0). Update Keevoro first, then open it again.',
        ],
      }
    if (asked === 'damaged')
      return {
        found: 'damaged',
        place,
        said: [
          `There is a Keevoro backup in ${place}, but its notes and the copy before them are damaged, so Keevoro can’t open it from them. Your files are safe there.`,
        ],
      }
    if (asked === 'none')
      return {
        found: 'noNotes',
        place,
        said: [
          `There is a Keevoro backup in ${place}, but it has no notes: it was last backed up by a Keevoro from before notes were kept. Your files are safe there. Back it up once from the computer that made it, and it can be opened here.`,
        ],
      }
    if (asked === 'nothing') return { found: 'nothing', place, said: [`Keevoro found no backup in ${place}.`] }
    const earlier = asked === 'earlier'
    const here = openedFrom.get(place)
    const alreadyOpen = here ? backups.find((b) => b.id === here)?.name : undefined
    const files = earlier ? 47_960 : 48_210
    const bytes = (earlier ? 310.8 : 312.4) * 1024 ** 3
    const backedUp = earlier ? '14 November 2026' : '21 November 2026'
    return {
      found: earlier ? 'earlierNotes' : 'notes',
      place,
      name: 'Family photos',
      files,
      bytes,
      lastBackedUp: 1_795_219_200,
      lastBackedUpSaid: backedUp,
      lastProved: 1_794_700_800,
      lastProvedSaid: '15 November 2026',
      madeBy: 'Keevoro 0.49.0',
      sources: [
        { name: 'Family hard drive', wasAt: 'E:\\' },
        { name: 'Mum’s card', wasAt: 'G:\\DCIM' },
      ],
      ...(alreadyOpen ? { alreadyOpen } : {}),
      said: [
        `“Family photos”: ${count(files, 'file')} (${size(bytes)}), last backed up on ${backedUp}.`,
        'Its copies were last proved, by reading them back, on 15 November 2026.',
        'Its notes were written by Keevoro 0.49.0.',
        ...(earlier
          ? [
              `Its newest notes are damaged, so Keevoro used the copy before them, from ${backedUp}. Files backed up after then are recognised when you next back up.`,
            ]
          : []),
        ...(alreadyOpen ? [`This backup is already on this computer, as “${alreadyOpen}”. Opening it opens that one.`] : []),
      ],
    }
  }

  // The tester's folder of scans, already looked through, and the backup
  // not yet asked how to arrange it.
  if (mode === 'scans') {
    sources.push({ id: SCANS_ID, name: 'Family Scans', root: scansRoot, items: 240, bytes: 240 * 4.1 * 1024 ** 2 })
    arrangeScans()
  }

  return {
    state: async () => state(),
    async startBackup(name, wants) {
      putAway()
      const taken = (n: string) => backups.some((s) => s.name.toLowerCase() === n.toLowerCase())
      const musicAlone = !!wants && wants.includes('music') && !wants.includes('photos') && !wants.includes('videos')
      const wanted = name.trim() || (musicAlone ? 'My music' : 'My photos')
      let called = wanted
      for (let n = 2; taken(called); n++) called = `${wanted} ${n}`
      // Never reused, as a real backup's id is not, even once removed.
      made += 1
      const id = `s${made}`
      backups = [...backups, { id, name: called }]
      library = `C:\\Users\\You\\AppData\\Keevoro\\backups\\${id}`
      // A new backup starts with nothing found, unless the page asked for a full one.
      if (backups.length > 1) shelf.set(id, { collections: [], sources: [], junk: [], dests: [], sentSoFar: 0 })
      else putAway()
      takeOut(id)
      // What it is for, as asked; a backup of music alone keeps its folders.
      backupWants = wants ?? ['photos', 'videos']
      arrange = musicAlone ? 'folders' : null
      return state()
    },
    async switchBackup(id) {
      putAway()
      takeOut(id)
      return state()
    },
    async backupDetails(id) {
      const backup = backups.find((b) => b.id === id)
      if (!backup) throw new Error(`There is no backup called "${id}".`)
      const kept = keptOf(id)
      const photos = kept.collections.reduce((n, c) => n + c.photos, 0)
      const videos = kept.collections.reduce((n, c) => n + c.videos, 0)
      const listed: BackupListing = {
        id,
        name: backup.name,
        home: homeOf(id),
        open: id === open,
        found: {
          sources: kept.sources.length,
          destinations: kept.dests.length,
          photos,
          videos,
          waiting: 0,
          waitingBytes: 0,
          bytes: (photos * 3.2 + videos * 48) * 1024 * 1024,
          sourcePlaces: kept.sources.map((s) => s.root),
          destinationNames: kept.dests.map((d) => d.name),
          ...(kept.sentSoFar > 0 ? { lastRan: Date.now() / 1000 - 3 * 86_400 } : {}),
        },
      }
      return listed
    },
    async renameBackup(id, to) {
      const name = to.trim()
      if (!name) throw new Error('Give the backup a name.')
      if (name.length > 60) throw new Error('Keep the name to 60 letters or fewer.')
      if (backups.some((b) => b.id !== id && b.name.toLowerCase() === name.toLowerCase()))
        throw new Error(`There is already a backup called "${name}". Choose another name.`)
      backups = backups.map((b) => (b.id === id ? { ...b, name } : b))
      return state()
    },
    async removeBackup(id, dryRun) {
      const backup = backups.find((b) => b.id === id)
      if (!backup) throw new Error(`There is no backup called "${id}".`)
      const removal = removalOf(backup, !dryRun)
      if (dryRun) return removal
      const kept = keptOf(id)
      const removedAt = Date.now() / 1000
      const keptUntil = removedAt + 7 * 86_400
      const day = new Date(keptUntil * 1000).toLocaleDateString('en-GB', { day: 'numeric', month: 'long', year: 'numeric' })
      removedRecently.push({
        backup,
        kept,
        listed: {
          id,
          name: backup.name,
          removedAt,
          keptUntil,
          bytes: removal.ownFolderBytes,
          said: `Removed just now. It can be put back until ${day}, when its ${(removal.ownFolderBytes / 1024 / 1024).toFixed(1)} MB of previews and working files are cleared away.`,
        },
      })
      shelf.delete(id)
      backups = backups.filter((b) => b.id !== id)
      if (open === id) {
        open = null
        const next = backups[0]
        if (next) takeOut(next.id)
        else library = null
      }
      return removal
    },
    removedBackups: async () => removedRecently.map((r) => r.listed),
    async putBackBackup(id) {
      const at = removedRecently.findIndex((r) => r.listed.id === id)
      if (at < 0) throw new Error(`There is no removed backup called "${id}" to put back.`)
      const [back] = removedRecently.splice(at, 1)
      backups = [...backups, back.backup]
      shelf.set(id, back.kept)
      if (!open) {
        library = homeOf(id)
        takeOut(id)
      }
      return state()
    },
    async clearAwayBackup(id) {
      const at = removedRecently.findIndex((r) => r.listed.id === id)
      if (at < 0) throw new Error(`There is no removed backup called "${id}" to clear away.`)
      return removedRecently.splice(at, 1)[0].listed
    },
    openDestination: async () => {},
    openPlace: async () => {},
    async resetEverything() {
      shelf.clear()
      backups = []
      open = null
      library = null
      collections.length = 0
      sources.length = 0
      junk.length = 0
      dests = []
      said()
      sentSoFar = 0
      return state()
    },
    // A made-up two minutes: busy, with the odd lull, and quiet when paused.
    activity: async () => {
      tick += 1
      const busy = !pausedNow
      usage.push({
        cpu: busy ? 11 + 6 * Math.sin(tick / 5) + (tick % 17 === 0 ? 9 : 0) : 0.3,
        disk: busy ? 78 + 22 * Math.sin(tick / 3) : 0,
        sent: busy ? 12 + 4 * Math.sin(tick / 4) : 0,
        network: busy ? 12 + 4 * Math.sin(tick / 4) : 0,
        memory: busy ? 520 + 40 * Math.sin(tick / 9) : 310,
      })
      if (usage.length > 120) usage.shift()
      return {
        ...(checkRunning
          ? {
              check: {
                job: 1,
                ...checkAt,
                pausedAlone: checkHeld,
                said: checkHeld ? 'Checking for duplicates is paused.' : 'Checking for duplicates in the background.',
              },
            }
          : {}),
        paused: pausedNow,
        doing: 'Checking for duplicates and making thumbnails',
        backupStep: new URLSearchParams(window.location.search).has('sending') ? 'Working out where each file goes: 2,500 of 8,630' : null,
        jobs: [
          {
            what: 'Making thumbnails',
            why: 'Making the small pictures for the screen you are looking at. Each is made once and kept, so the same screen is quick the next time.',
          },
          ...(new URLSearchParams(window.location.search).has('sending')
            ? [
                {
                  what: 'Backing up your files',
                  why: 'Reading each photo and video you are keeping and copying it to where you chose. This uses the disk steadily, and the network if you are backing up to cloud storage.',
                },
              ]
            : []),
          {
            what: 'Checking for duplicates',
            why: 'Reading every file from start to finish, so that true copies can be told from photos that merely look alike. This is the heaviest thing Keevoro does to the disk. It is done once per file and remembered.',
          },
        ],
        samples: [...usage],
        memoryBytes: 560 * 1024 * 1024,
        cores: 20,
      }
    },
    // The send in hand, if there is one: how long it has run and its estimate.
    sendProgress: async () => {
      const runningSeconds = sendStarted === null ? 0 : Math.round((Date.now() - sendStarted) / 1000)
      return {
        running: sendStarted !== null,
        stopping: false,
        line: sendingAtOpen && sendStarted !== null ? 'Working out where each file goes: 2,500 of 8,630' : '',
        runningSeconds,
        estimate: sendStarted === null ? null : demoEstimate(runningSeconds),
        overNetwork: false,
      }
    },
    sendHistory: async () => [...sends],
    lastBackupSaid: async () => lastSaid,
    pauseWork: async (paused) => {
      pausedNow = paused
    },
    acceleration: async () => speed(),
    agentAccess: async () => ({ ...agents }),
    setAgentAccess: async (change) => {
      agents = { ...agents, ...change }
      return { ...agents }
    },
    fileKinds: async () => lookedFor.map((k) => ({ ...k })),
    addFileKind: async (typed, kind) => {
      const ending = plainEnding(typed)
      const listed = lookedFor.find((k) => k.ending === ending)
      if (listed?.builtIn && (!listed.on || listed.kind !== kind)) {
        refuseAsMedia(ending, kind)
        const changed = { ...listed, kind, on: true }
        lookedFor = lookedFor.map((k) => (k === listed ? changed : k))
        return { ...changed }
      }
      if (listed) throw new Error(`Keevoro already looks for .${ending} files.`)
      refuseAsMedia(ending, kind)
      const added = ownKind(ending, kind)
      lookedFor = [...lookedFor, added]
      return { ...added }
    },
    changeFileKind: async (typed, change) => {
      const now = kindNamed(typed)
      const ending = change.to === undefined ? now.ending : plainEnding(change.to)
      if (ending !== now.ending) {
        if (now.builtIn)
          throw new Error(`.${now.ending} is one of the kinds Keevoro comes with. It can be switched off, but not removed or renamed.`)
        if (lookedFor.some((k) => k.ending === ending)) throw new Error(`Keevoro already looks for .${ending} files.`)
      }
      const changed = { ...now, ending, kind: change.kind ?? now.kind, on: change.on ?? now.on }
      if (!now.builtIn) refuseAsMedia(changed.ending, changed.kind)
      lookedFor = lookedFor.map((k) => (k === now ? changed : k))
      return { ...changed }
    },
    removeFileKind: async (typed) => {
      const gone = kindNamed(typed)
      if (gone.builtIn)
        throw new Error(`.${gone.ending} is one of the kinds Keevoro comes with. It can be switched off, but not removed or renamed.`)
      lookedFor = lookedFor.filter((k) => k !== gone)
      return { ...gone }
    },
    resetFileKinds: async () => {
      lookedFor = [...builtInKinds(), ...lookedFor.filter((k) => !k.builtIn)]
      return lookedFor.map((k) => ({ ...k }))
    },
    chooseAgentFolder: async () => (agents.lookIn.includes('D:\\Photos') ? 'F:\\Family Backup' : 'D:\\Photos'),
    // A typical case: an old backup folder full of a stopped backup's working files.
    tidyFolder: async (folder, dryRun) => {
      const files = untidy.get(folder) ?? 0
      const bytes = files * 26.7 * 1024 ** 2
      const inUse = files > 0 ? 1 : 0
      if (!dryRun) untidy.delete(folder)
      const said =
        files === 0
          ? [`There ${dryRun ? 'are' : 'were'} no unfinished copies to clear in ${folder}.`]
          : dryRun
            ? [
                `There are ${size(bytes)} of unfinished copies (${count(files, 'file')}) in ${folder}, left by a backup that was stopped part-way. Clearing them touches nothing that was backed up.`,
                '1 thing written to in the last hour will be left, as a backup may be using it right now.',
              ]
            : [
                `Cleared ${size(bytes)} of unfinished copies (${count(files, 'file')}) from ${folder}. They were Keevoro's working files; nothing that was backed up was touched.`,
                'Left 1 thing written to in the last hour, as a backup may be using it right now. Try again later.',
              ]
      return { folder, cleared: !dryRun, files, bytes, inUse, notOurs: 0, said }
    },
    chooseFolderToTidy: async () => 'G:\\Old Keevoro backup',
    setAcceleration: async (on) => {
      systemCodecs = on
      return speed()
    },
    stopScanning: async () => {
      stopLooking = true
    },
    stopSending: async () => {
      stopAsked = true
    },
    // A folder has a disk that can be measured; cloud storage does not.
    previewsWanted: async () => previews,
    setPreviewsWanted: async (wanted) => {
      previews = wanted
    },
    room: async () => {
      // Asking for the room looks at each destination first.
      const first = dests[0]
      if (first && first.there > 0 && emptyOnLook) {
        emptyOnLook = false
        first.gone = first.there
        first.there = 0
        sentSoFar = 0
        collections.forEach((c) => (c.gathered = 0))
        otherSent.clear()
      }
      if (first && first.there > 5 && damageOnLook) {
        damageOnLook = false
        first.gone = 2
        first.notAsSent = 3
        first.there -= 5
        sentSoFar -= 5
        let left = 5
        for (const c of collections) {
          const take = Math.min(left, c.gathered)
          c.gathered -= take
          left -= take
        }
      }
      if (first && first.there > 0 && unplugOnLook) {
        unplugOnLook = false
        first.outOfReach = true
      }
      if (first && first.there > 0 && leaveEmptyOnLook) {
        leaveEmptyOnLook = false
        first.otherDisk = true
      }
      const folder = sendingTo?.startsWith('the folder') ? sendingTo.replace('the folder ', '') : null
      // `?full` pretends the chosen disk is nearly full.
      const free = folder ? (new URLSearchParams(window.location.search).has('full') ? 90 * 1024 * 1024 : 118 * 1024 ** 3) : null
      const needed = waitingBytes()
      // Reading from cloud storage can cost money, and is said first.
      const cloud = sources.filter((s) => s.bucket)
      const share = cloud.reduce((n, s) => n + s.bytes, 0)
      const cost =
        dests.length > 0 && share > 0 && needed > 0
          ? cloud.map((s) => cloudCost(s, Math.min(needed, s.bytes) * dests.length))
          : []
      return { needed, folder, free, enough: free === null || free > needed + 200 * 1024 * 1024, ...(cost.length > 0 ? { cost } : {}) }
    },
    async addBucketSource(request) {
      await later(500 * pace)
      if (!request.accessKeyId?.trim() || !request.secretAccessKey?.trim()) throw new Error('Enter both keys for the storage.')
      const prefix = (request.prefix ?? '').replace(/^\/+|\/+$/g, '')
      const bucket = (request.bucket ?? '').trim()
      const root = `${request.kind}://${bucket}${prefix ? `/${prefix}` : ''}`
      const provider =
        request.kind === 'r2'
          ? 'Cloudflare R2'
          : request.kind === 's3'
            ? 'Amazon S3'
            : request.kind === 'gcs'
              ? 'Google Cloud Storage'
              : 'cloud storage'
      const where = `the ${provider} bucket ${bucket}`
      const place = prefix ? `the folder ${prefix} in ${where}` : where
      bucketPlaces.set(root, { place, kind: request.kind })
      return { id: Date.now(), name: prefix.split('/').pop() || bucket, root, place }
    },
    discover: async () => view(),
    places: async () => drivesNow(),
    // The quick list is each drive's place; it changes when `?plugin` has
    // a memory card arrive a moment after the page opens.
    async placesIfChanged(known, asked) {
      const now = drivesNow()
      const mounts = now.map((p) => p.path).sort()
      if (known && !asked && known.join('|') === mounts.join('|')) return null
      const arrived = known ? mounts.filter((m) => !known.includes(m)) : []
      return { mounts, places: now, arrived }
    },
    onDrives: () => () => {},
    // A made-up survey of a big disk: three real places and one of icons.
    async surveyPlace(path) {
      const drive = path.replace(/[\\/]+$/, '')
      surveyed.add(drive)
      mappedAgain.add(drive)
      for (let step = 1; step <= 5; step++) {
        surveyListeners.forEach((tell) =>
          // The last steps measure what Keevoro never looks through, for the map.
          tell({
            folders: step * 9_000,
            files: step * 120_000,
            photos: Math.min(step, 3) * 3_100,
            videos: Math.min(step, 3) * 90,
            ...(othersListed() ? { others: Math.min(step, 3) * 1_450 } : {}),
            ...(backupWants.includes('music') ? { music: Math.min(step, 3) * 400 } : {}),
            ...(step > 3 ? { sizing: true } : {}),
            top: topSoFar(drive, step / 5),
          }),
        )
        await new Promise((r) => setTimeout(r, new URLSearchParams(window.location.search).has('slow') ? 400 : 60))
      }
      const leftOutHere = [...leftOutFolders].filter((f) => within(f, drive))
      return {
        hints: [
          { folder: `${drive}\\Users\\You\\Videos\\Holidays`, photos: 287, videos: 31, bytes: 88 * 1024 ** 3, likelyPhotos: true, alreadyASource: false },
          { folder: `${drive}\\Users\\You\\Pictures`, photos: 9_318, videos: 274, bytes: 52 * 1024 ** 3, likelyPhotos: true, alreadyASource: false, onlineOnly: 412, onlineOnlySaid: onlineOnlySaid(412) },
          { folder: `${drive}\\Old laptop backup\\Photos`, photos: 902, videos: 12, bytes: 3.4 * 1024 ** 3, likelyPhotos: true, alreadyASource: false },
          { folder: `${drive}\\Games\\Launcher\\assets`, photos: 1_840, videos: 0, bytes: 9 * 1024 ** 2, likelyPhotos: false, alreadyASource: false },
          // `?survey=tree` adds folders that live together, for the groups:
          // eight in one place (two of them icons), and two of icons alone.
          ...(new URLSearchParams(window.location.search).get('survey') === 'tree'
            ? [
                { folder: `${drive}\\Archive\\Guides\\2017\\Walks`, photos: 41, videos: 2, bytes: 4.1 * 1024 ** 2, likelyPhotos: true, alreadyASource: false },
                { folder: `${drive}\\Archive\\Guides\\2017\\Maps`, photos: 23, videos: 0, bytes: 2.6 * 1024 ** 2, likelyPhotos: true, alreadyASource: false },
                { folder: `${drive}\\Archive\\Guides\\2018\\Coast`, photos: 19, videos: 1, bytes: 3.2 * 1024 ** 2, likelyPhotos: true, alreadyASource: false },
                { folder: `${drive}\\Archive\\Guides\\2018\\Towns`, photos: 14, videos: 0, bytes: 1.9 * 1024 ** 2, likelyPhotos: true, alreadyASource: false },
                { folder: `${drive}\\Archive\\Guides\\2019`, photos: 12, videos: 0, bytes: 1.5 * 1024 ** 2, likelyPhotos: true, alreadyASource: false },
                { folder: `${drive}\\Archive\\Guides\\Scans`, photos: 9, videos: 0, bytes: 1.2 * 1024 ** 2, likelyPhotos: true, alreadyASource: false },
                { folder: `${drive}\\Archive\\Guides\\Covers`, photos: 8, videos: 0, bytes: 0.3 * 1024 ** 2, likelyPhotos: false, alreadyASource: false },
                { folder: `${drive}\\Archive\\Guides\\Icons`, photos: 7, videos: 0, bytes: 0.2 * 1024 ** 2, likelyPhotos: false, alreadyASource: false },
                { folder: `${drive}\\Games\\Puzzle\\textures`, photos: 620, videos: 0, bytes: 3 * 1024 ** 2, likelyPhotos: false, alreadyASource: false },
              ]
            : []),
          // Other files carried along, once their kind is on the list.
          ...(listedAsOther('pdf')
            ? [{ folder: `${drive}\\Users\\You\\Documents`, photos: 0, videos: 0, others: 4_120, bytes: 2.1 * 1024 ** 3, likelyPhotos: true, alreadyASource: false }]
            : []),
          ...(listedAsOther('blend')
            ? [{ folder: `${drive}\\Users\\You\\Projects`, photos: 0, videos: 0, others: 240, bytes: 7.2 * 1024 ** 3, likelyPhotos: true, alreadyASource: false }]
            : []),
          // Music, for a backup that is for it: by name, never opened.
          ...(backupWants.includes('music')
            ? [{ folder: `${drive}\\Users\\You\\Music`, photos: 0, videos: 0, music: 1_204, bytes: 7.2 * 1024 ** 3, likelyPhotos: true, alreadyASource: false }]
            : []),
        ].filter((hint) => !leftOutOf(hint.folder)),
        folders: 28_915,
        files: 352_640,
        unreadable: 3,
        seconds: 5.4,
        stopped: false,
        onlineOnly: 412,
        onlineOnlySaid: onlineOnlySaid(412),
        leftOut: [
          {
            folder: `${drive}\\Users\\You\\Pictures\\Keevoro Library`,
            what: 'oldLibrary',
            said: `Left out ${drive}\\Users\\You\\Pictures\\Keevoro Library: a folder an older version of Keevoro made, which it no longer uses. It takes 9.4 GB: 8.6 GB of copies of your photos it gathered there (2,330 files), and 0.8 GB of previews and working files it made. Keevoro has not checked that every copy is also in one of your backups, so it leaves the folder for you to decide about and never removes it.`,
          },
          ...leftOutHere.map((folder) => ({
            folder,
            what: 'leftOutByYou' as const,
            said: `Left out ${folder}, as you chose: it is not looked through for this backup. Put it back to include it again.`,
          })),
        ],
      }
    },
    stopSurvey: async () => {},
    onSurvey: (listener) => {
      surveyListeners.add(listener)
      return () => surveyListeners.delete(listener)
    },
    // `?drives=away` has a second source whose card is in a drawer, and
    // `?drives=different` another card at the first source's place.
    async lookAgain(source, differentDrive) {
      const drives = new URLSearchParams(window.location.search).get('drives')
      const first = sources[0]
      if (!first) throw new Error('This backup has no sources to look through again. Add one first.')
      if (drives === 'away') {
        void later(400 * pace).then(() =>
          scanListeners.forEach((l) =>
            l({
              name: first.name,
              photos: 240,
              videos: 0,
              files: 8880,
              finished: true,
              passedOver: ['KINGSTON (E:) is not plugged in, so it was not looked through. What was found on it before is kept.'],
            }),
          ),
        )
        return
      }
      if (drives === 'different' && differentDrive === undefined) {
        void later(300 * pace).then(() =>
          scanListeners.forEach((l) =>
            l({
              name: 'Card photos',
              photos: 0,
              videos: 0,
              finished: true,
              differentDrive: [
                {
                  source: first.id,
                  name: first.name,
                  place: first.root,
                  asked: `This looks like a different drive from the one called ${first.name}. Add it as a new source, or look through it as ${first.name} anyway?`,
                  said: `A different drive is plugged in where ${first.name} was (${first.root}), so ${first.name} was not looked through. What was found on it before is kept.`,
                },
              ],
            }),
          ),
        )
        return
      }
      if (differentDrive === 'addAsNew') {
        const at = sources.findIndex((s) => s.id === source)
        if (at >= 0) {
          const was = sources[at]
          sources[at] = {
            ...was,
            root: `set aside ${was.id}`,
            setAsideFrom: was.root,
            setAsideSaid: `Set aside when a different drive was plugged in at ${was.root}. Everything found on it is kept; plug its own drive in and look again to bring it back.`,
          }
          look('KINGSTON (E:)', was.root)
          return
        }
      }
      const one = sources.find((s) => s.id === source) ?? first
      look(one.name, one.root)
    },
    async scanPlace(path) {
      const name = /\\Music$/.test(path)
        ? 'Music'
        : path.startsWith('E')
        ? 'KINGSTON (E:)'
        : path === 'C:\\'
          ? 'Windows (C:)'
          : sources.length === 0
            ? 'Family hard drive'
            : `Old laptop ${sources.length}`
      look(name, path)
    },
    // The folder window is the system's own, so the stand-in just answers:
    // a folder inside the disk it was asked to start in, when it was.
    pickPlace: async (startAt) =>
      backupWants.includes('music') && !backupWants.includes('photos') && !backupWants.includes('videos')
        ? `${MUSIC_ROOT}\\Music`
        : startAt
          ? `${startAt.replace(/[\\/]+$/, '')}\\Photos`
          : 'D:\\Photos',
    logTail: async () =>
      [
        '2026-11-16 14:02:09  Keevoro 9.8.0 started on windows (x86_64)',
        '2026-11-16 14:02:11  Looking through E:\\',
        '2026-11-16 14:02:19  Reading a large file (3.9 GB): E:\\Video\\wedding.mov',
        '2026-11-16 14:03:40  Looked through E:\\ in 89 seconds: 238 photos and videos, 8,906 other files, 3 folders skipped, 0 unreadable',
      ].join('\n'),
    openLog: async () => {},
    // Reports to Keevoro's maker. `?reportFails` refuses the first one sent,
    // as the service does when too many come from one computer, and
    // `?reportsOffline` cannot ask after them. `?reportUnreachable` cannot
    // reach the service at all, as on a work computer whose proxy Keevoro
    // cannot use, so the window sends it and asks after them itself (the UI
    // test stands in for the service; nothing is sent anywhere real).
    async prepareReport(request) {
      const message = request.message.trim()
      if (!message) throw new Error(request.kind === 'problem' ? 'Say what went wrong first.' : 'Say what your idea is first.')
      if ([...message].length > 5000) throw new Error(`Keep it to 5,000 letters or fewer. This is ${[...message].length.toLocaleString('en-GB')}.`)
      const contact = request.contact?.trim() || undefined
      if (contact && [...contact].length > 200) throw new Error('Keep the way to reach you to 200 letters or fewer.')
      const log = [
        '2026-11-16 14:02:09  Keevoro 9.8.0 started on windows (x86_64)',
        '2026-11-16 14:02:11  Looking through C:\\Users\\You\\Pictures\\Family photos',
        '2026-11-16 14:03:40  Looked through it in 89 seconds: 238 photos and videos',
      ]
        .join('\n')
        .replaceAll('C:\\Users\\You', '~')
      const report = {
        kind: request.kind,
        message,
        appVersion: '9.8.0',
        os: 'Windows 11 Home (26100)',
        screen: request.screen || undefined,
        logTail: request.withLog ? log : undefined,
        contact,
      }
      return { body: JSON.stringify(report, null, 2), sendTo: 'https://reports.keevoro.com', withCode: reportingCode !== null }
    },
    async reportingCode(change) {
      if (change?.code !== undefined && change.forget) throw new Error('Ask to keep a code or to forget one, not both.')
      if (change?.code !== undefined) {
        const checked = checkedCode(change.code)
        if ('why' in checked) throw new Error(checked.why)
        reportingCode = checked.code
      }
      if (change?.forget) reportingCode = null
      return { hasCode: reportingCode !== null }
    },
    async sendReport(prepared) {
      await later(300 * pace)
      if (opening.has('reportUnreachable')) {
        // As Keevoro does, the report with the code in, for the window alone.
        const sendAs = reportingCode === null ? undefined : `${prepared.replace(/\s*\}\s*$/, '')},\n  "code": "${reportingCode}"\n}`
        throw new ReportNotSent(
          'Couldn’t reach Keevoro’s reports service just now. Check this computer is online, then try again.',
          true,
          'This computer’s network would not let Keevoro connect securely, as some work and school networks do.',
          sendAs,
        )
      }
      if (opening.has('reportFails') && !reportRefused) {
        reportRefused = true
        throw new Error('That’s a lot of reports from one computer today. Please try again tomorrow.')
      }
      const reference = `K-${1042 + reportsSent.length - 2}`
      const recognised = reportingCode === null ? undefined : reportingCode === DEMO_CODE
      reportsSent.unshift(keptReport(prepared, reference, recognised))
      return { reference, said: `Thank you. Your reference is ${reference}.`, ...codeSaid(recognised), emailSaid: emailSaid(prepared) }
    },
    async noteReport(prepared, reference, token, recognised) {
      if (!/^K-\d{1,12}$/.test(reference) || !/^[0-9a-f]{40}$/.test(token)) {
        throw new Error('That isn’t an answer from Keevoro’s reports service, so the report isn’t noted.')
      }
      reportsSent.unshift(keptReport(prepared, reference, recognised))
      tokens.set(reference, token)
      return { reference, said: `Thank you. Your reference is ${reference}.`, ...codeSaid(recognised), emailSaid: emailSaid(prepared) }
    },
    reportsAsking: async () => ({
      sendTo: 'https://reports.keevoro.com',
      reports: opening.has('reportUnreachable')
        ? reportsSent.filter((r) => tokens.has(r.reference)).map((r) => ({ reference: r.reference, token: tokens.get(r.reference) ?? '' }))
        : [],
    }),
    async reportsHeard(heard, all) {
      for (const { reference, answer } of heard) {
        const said = answer as { ok?: boolean; reference?: string; status?: string; shippedIn?: string; note?: string }
        const kept = reportsSent.find((r) => r.reference === reference)
        if (!kept || said.ok !== true || (said.reference && said.reference !== reference)) continue
        kept.status = said.status ?? kept.status
        kept.shippedIn = said.shippedIn
        kept.note = said.note
        kept.statusSaid =
          said.status === 'shipped' ? `Done in version ${said.shippedIn ?? ''}`.trim() : said.status === 'new' ? 'Received' : 'Being worked on'
      }
      return all ? { reports: reportsSent.slice(), checked: true } : { reports: reportsSent.slice(), checked: false, said: 'Couldn’t check just now.' }
    },
    reports: async () => ({ reports: reportsSent.slice(), checked: false }),
    refreshReports: async () =>
      opening.has('reportsOffline') || opening.has('reportUnreachable')
        ? { reports: reportsSent.slice(), checked: false, said: 'Couldn’t check just now.' }
        : { reports: reportsSent.slice(), checked: true },
    checkUpdate: async () =>
      new URLSearchParams(window.location.search).has('update')
        ? { version: '9.9.0', page: 'https://github.com/markcastle/keevoro/releases' }
        : null,
    // `?update&busy` has a backup in hand, so the update waits for it.
    installUpdate: async (stopFirst) => {
      if (new URLSearchParams(window.location.search).has('busy') && !stopFirst) {
        return {
          waiting:
            'Keevoro will update once the work in hand finishes, so nothing is cut short. You can stop it and update now instead.',
        }
      }
      await later(600 * pace)
      return {}
    },
    openReleasePage: async () => {},
    // Remembered, so a test can see which page would have opened in the browser.
    openWebPage: async (page) => {
      const opened = window as unknown as { keevoroOpenedPages?: string[] }
      opened.keevoroOpenedPages = [...(opened.keevoroOpenedPages ?? []), page]
    },
    async keepCollection(key, keep) {
      otherRowsIn(key).forEach((row) => (row.kept = keep))
      const found = collections.find((c) => c.key === key)
      if (!found) return
      found.kept = keep
      inside.get(key)?.forEach((file) => (file.kept = keep))
    },
    async collectionPhotos(key) {
      const rows = otherRowsIn(key)
      if (rows.length > 0) return rows.map((row) => ({ ...row }))
      const found = collections.find((c) => c.key === key)
      if (!found) throw new Error('That collection isn\u2019t there any more.')
      return filesOf(found).map((file) => ({ ...file }))
    },
    // Two made-up duplicates: one checked byte for byte, one still being checked.
    duplicates: async () =>
      collections.length === 0
        ? []
        : [
            {
              ...photo('IMG_2041.jpg'),
              folder: 'E:\\Holidays\\Cornwall 2019',
              bytes: 4.2 * 1024 * 1024,
              confirmed: true,
              copies: [
                { name: 'IMG_2041.jpg', folder: 'E:\\Backup of phone\\DCIM' },
                { name: 'IMG_2041 (1).jpg', folder: 'E:\\Downloads' },
              ],
            },
            {
              ...photo('MVI_0042.MP4'),
              folder: 'E:\\Camcorder\\Lake District',
              bytes: 3.9 * 1024 ** 3,
              confirmed: false,
              copies: [{ name: 'MVI_0042.MP4', folder: 'E:\\Camcorder\\Unsorted' }],
            },
          ],
    setAside: async () => junk.map((file) => ({ ...file })),
    // Every source in the stand-in has the same made-up mix of kinds.
    backupArrange: async () => arrangeView(),
    async setBackupArrange(to) {
      const before = arrange ?? 'byDate'
      arrange = to
      arrangeScans()
      const already = collections.reduce((n, c) => n + c.gathered, 0)
      if (before === to) return arrangeView()
      const first = to === 'folders' ? 'From now on, new files are kept in your folders.' : 'From now on, new files are arranged by date.'
      return arrangeView(
        already === 0 ? first : `${first} The ${already.toLocaleString('en-GB')} files already backed up stay where they are.`,
      )
    },
    sourceArrange: async (id) => sourceArrangeView(id),
    async setSourceArrange(id, to) {
      const before = arrangementOf(id)
      if (to === 'backup') sourceArranges.delete(id)
      else sourceArranges.set(id, to)
      arrangeScans()
      return sourceArrangeView(id, before !== arrangementOf(id))
    },
    backupWants: async () => wantsView(backupWants),
    setBackupWants: async (wants) => {
      const before = backupWants
      backupWants = wants
      // Music asked for where none was looked for: the sources are looked through again.
      const looksAgain = wants.includes('music') && open !== null && !musicIn.has(open) && sources.length > 0
      return wantsView(wants, undefined, true, before, looksAgain)
    },
    sourceWants: async (id) => sourceView(id),
    setSourceOnArrival: async (source, on) => {
      if (on) onArrival.add(source)
      else onArrival.delete(source)
      return { source, on }
    },
    setSourceWants: async (id, wants) => {
      const looking = open !== null && !musicIn.has(open) && !wantsMusic()
      if (wants === 'backup') sourceChoices.delete(id)
      else sourceChoices.set(id, wants)
      // Music asked for where none was looked for: this source is looked through again.
      return looking && wantsMusic() ? { ...sourceView(id, true), looksAgain: true } : sourceView(id, true)
    },
    fileTypes: async (id) =>
      kinds.map((k) => {
        const isVideo = ['mov', 'mp4', 'mts', 'avi'].includes(k.ending)
        const own = sourceChoices.get(id)
        const wants = own ?? backupWants
        const left = !wants.includes(isVideo ? 'videos' : 'photos')
        if (left) {
          const level = own ? 'source' : 'backup'
          return { ...k, leftOutBy: level, why: `this ${level} is ${level === 'backup' ? 'for' : 'set to'} ${words(wants)}` }
        }
        const about = builtInKindList.find((b) => b.ending === k.ending)?.about
        const said = about ? { ...k, about } : { ...k }
        return k.included ? said : { ...said, leftOutBy: 'ending' as const, why: 'files with this ending are left out of this source' }
      }),
    leaveOutFolder: async (folder, leftOut) => {
      const at = folder.replace(/\\+$/, '')
      if (leftOut) leftOutFolders.add(at)
      else [...leftOutFolders].filter((f) => f.toLowerCase() === at.toLowerCase()).forEach((f) => leftOutFolders.delete(f))
      const still = leftOutOf(at)
      return {
        folder: at,
        leftOut,
        files: 0,
        ...(!leftOut && still ? { stillInside: still } : {}),
        said: leftOut
          ? `Left out ${at}. This backup won\u2019t look through it. Nothing in it is changed, and nothing already backed up is touched.`
          : still
            ? `Put back ${at}, but it is inside ${still}, which is still left out. Put that back to include it.`
            : `Put back ${at}. It is looked through again the next time its source is.`,
      }
    },
    includeFileType: async (_id, ending, included) => {
      const kind = kinds.find((k) => k.ending === ending)
      if (kind) kind.included = included
    },
    forgetSource: async (id) => {
      const at = sources.findIndex((s) => s.id === id)
      if (at < 0) return
      sources.splice(at, 1)
      // The made-up collections are not tied to a source, so the newest go.
      collections.splice(0, sources.length === 0 ? collections.length : 2)
      if (sources.length === 0) junk.length = 0
    },
    async keepPhoto(id, keep) {
      const row = [...otherRows.values()].find((r) => r.id === id)
      if (row) row.kept = keep
      const at = junk.findIndex((file) => file.id === id)
      if (at >= 0 && keep) {
        junk.splice(at, 1)
        return
      }
      for (const c of collections) {
        const file = inside.get(c.key)?.find((f) => f.id === id)
        if (!file) continue
        file.kept = keep
        // A collection is kept while anything in it is.
        c.kept = filesOf(c).some((f) => f.kept)
      }
    },
    // `?slowpictures` makes each picture take a while, to show the placeholders.
    preview: async (id) => {
      if (new URLSearchParams(window.location.search).has('slowpictures')) await new Promise((r) => setTimeout(r, 4000))
      return pictures.get(id) ?? Promise.reject(new Error('This one can\u2019t be previewed'))
    },
    closerLook: async (id) => pictures.get(id) ?? Promise.reject(new Error('This one can\u2019t be previewed')),
    notNeeded: (key) => looks.drop(key),
    // Looking inside a folder: a Pictures folder like the owner's, or any
    // other with a few photos in it.
    foldersInside: async (place) => {
      if (![...surveyed].some((root) => place === root || place.startsWith(`${root}\\`))) return null
      // The Music folder, by artist and album: tracks, never pictures.
      if (/\\Music$/.test(place)) {
        const albums: FolderView[] = MUSIC_ALBUMS.map((a) => ({
          folder: `${place}\\${a.folder.slice(1).join('\\')}`,
          relative: a.folder.slice(1).join(' › '),
          photos: 0,
          videos: 0,
          music: a.files.length,
          bytes: a.files.reduce((n, [, b]) => n + b, 0),
          onlineOnly: 0,
        }))
        return {
          place,
          surveyedAt: Math.floor(Date.now() / 1000) - 120,
          complete: true,
          folders: albums,
          photos: 0,
          videos: 0,
          music: MUSIC_TRACKS,
          bytes: albums.reduce((n, f) => n + f.bytes, 0),
          onlineOnly: 0,
        }
      }
      const plan = /\\Pictures$/.test(place) ? INSIDE : [{ relative: '', photos: 40, videos: 2 }]
      const folders: FolderView[] = plan.map((f) => ({
        folder: f.relative ? `${place}\\${f.relative.replace(/ › /g, '\\')}` : place,
        relative: f.relative,
        photos: f.photos,
        videos: f.videos,
        bytes: (f.photos * 3.1 + f.videos * 88) * 1024 ** 2,
        onlineOnly: Math.floor(f.photos / 50),
        ...(leftOutOf(f.relative ? `${place}\\${f.relative.replace(/ › /g, '\\')}` : place) ? { leftOut: true } : {}),
      }))
      const out = leftOutOf(place)
      return {
        ...(out ? { leftOut: out } : {}),
        place,
        surveyedAt: Math.floor(Date.now() / 1000) - 120,
        complete: true,
        folders,
        photos: folders.reduce((n, f) => n + f.photos, 0),
        videos: folders.reduce((n, f) => n + f.videos, 0),
        bytes: folders.reduce((n, f) => n + f.bytes, 0),
        onlineOnly: folders.reduce((n, f) => n + f.onlineOnly, 0),
      }
    },
    // Where the space goes: the owner's own disk, or any other place.
    spaceMap: async (request) => {
      const { place } = request
      if (![...surveyed].some((root) => place === root || place.startsWith(`${root}\\`))) return null
      await later(40 * pace)
      const root = [...surveyed].find((r) => place === r || place.startsWith(`${r}\\`)) ?? place
      const stale = old !== null && !mappedAgain.has(root)
      const carried = new Set(lookedFor.filter((k) => k.kind === 'other' && k.on).map((k) => k.ending))
      const map = demoMap(request, leftOutOf, Math.floor(Date.now() / 1000) - (stale ? 3 * 24 * 3600 : 120), carried)
      return map && stale ? { ...map, refresh: old === 'network' ? 'askFirst' : 'byItself' } : map
    },
    forgetMaps: async () => {
      const forgotten = surveyed.size
      surveyed.clear()
      return { forgotten }
    },
    folderFiles: async ({ place, from, count, sort, only }) => {
      await later(80 * pace)
      const album = MUSIC_ALBUMS.find((a) => place.endsWith(`\\${a.folder.join('\\')}`))
      if (album) {
        const tracks: FileView[] = album.files.map(([name, bytes], i) => ({
          path: `${place}\\${name}`,
          name,
          isVideo: false,
          isMusic: true,
          bytes,
          modified: 1_560_000_000 + i * 600,
        }))
        const shown = only === 'all' || only === 'music' ? tracks : []
        return {
          folder: place,
          total: shown.length,
          photos: 0,
          videos: 0,
          music: tracks.length,
          onlineOnly: 0,
          files: shown.slice(from, from + Math.min(count, 500)),
        }
      }
      const known = INSIDE.find((f) => (f.relative ? place.endsWith(`\\${f.relative.replace(/ › /g, '\\')}`) : /\\Pictures$/.test(place)))
      const files = demoFiles(place, known?.photos ?? 40, known?.videos ?? 2)
      // As the real one: by name, or by the file's date with ties by name.
      const shown = files
        .filter((f) => only === 'all' || f.isVideo === (only === 'videos'))
        .sort((a, b) =>
          sort === 'name' ? a.name.localeCompare(b.name) : (sort === 'newest' ? b.modified - a.modified : a.modified - b.modified) || a.name.localeCompare(b.name),
        )
      return {
        folder: place,
        total: shown.length,
        photos: files.filter((f) => !f.isVideo).length,
        videos: files.filter((f) => f.isVideo).length,
        onlineOnly: files.filter((f) => f.onlineOnly).length,
        files: shown
          .slice(from, from + Math.min(count, 500))
          .map((f) => (leftOutOf(f.path) ? { ...f, leftOut: 'you left this folder out of this backup' } : f)),
      }
    },
    // Each picture takes a moment, a few at a time, as the real ones do.
    // `?slowpictures` makes them take a while, to show the placeholders.
    filePicture: (path) =>
      looks.want(fileKey(path), async () => {
        looked += 1
        await later(new URLSearchParams(window.location.search).has('slowpictures') ? 1500 : 40)
        if (path.endsWith('IMG_0013.jpg')) throw new Error('This one can\u2019t be shown.')
        return scene([...path].reduce((n, c) => n + c.charCodeAt(0), 0), /\.mp4$/.test(path))
      }),
    closerLookAtFile: async (path) => scene([...path].reduce((n, c) => n + c.charCodeAt(0), 0), /\.mp4$/.test(path)),
    destination: async () => sendingTo,
    chooseDestinationFolder: async () => 'F:\\Family Backup',
    async findBackupAt(request) {
      await later(300 * pace)
      return foundAt(request)
    },
    async openBackupFrom(request) {
      await later(300 * pace)
      const found = foundAt(request)
      if (found.found !== 'notes' && found.found !== 'earlierNotes') {
        const refused = new Error(found.said.join(' '))
        throw refused
      }
      const here = openedFrom.get(found.place)
      if (here && backups.some((b) => b.id === here)) {
        putAway()
        takeOut(here)
        const name = backups.find((b) => b.id === here)?.name ?? ''
        return {
          id: here,
          name,
          alreadyOpen: true,
          files: found.files ?? 0,
          said: [`This backup is already on this computer, as “${name}”. Opening it opens that one.`],
        }
      }
      await this.startBackup(found.name ?? 'My photos')
      const id = open ?? ''
      openedFrom.set(found.place, id)
      const aside = `${homeOf(id)}\\.keevoro\\elsewhere`
      const waiting = (found.sources ?? []).map((source, at) => ({ id: at + 1, name: source.name, wasAt: source.wasAt }))
      sources.splice(
        0,
        sources.length,
        ...waiting.map((w, at) => ({
          id: w.id,
          name: w.name,
          root: `${aside}\\${w.id}`,
          items: at === 0 ? 31_480 : 16_730,
          bytes: (at === 0 ? 204.1 : 108.3) * 1024 ** 3,
          waitingFrom: w.wasAt,
          waitingSaid: `Keevoro does not know where these photos live on this computer. On the computer this backup came from they were in ${w.wasAt}. Connect them again to carry on backing them up: nothing already backed up is sent again.`,
        })),
      )
      dests = [{ name: found.place, kind: request.kind, folder: request.kind === 'folder' ? request.place : null, there: found.files ?? 0 }]
      said()
      return {
        id,
        name: backups.find((b) => b.id === id)?.name ?? '',
        files: found.files ?? 0,
        waiting,
        said: [
          `Opened “${found.name}” from ${found.place}: ${count(found.files ?? 0, 'file')}, all known to be there.`,
          'Keevoro does not know where these photos live on this computer. Connect each source again to carry on backing it up: nothing already backed up is sent again.',
        ],
      }
    },
    chooseWhereBackupIs: async () => 'F:\\Family Backup',
    async connectSourceAgain(source, place) {
      const at = sources.find((s) => s.id === source)
      if (!at) throw new Error('That source isn’t part of this backup.')
      if (!at.waitingFrom) throw new Error('This source is already connected on this computer.')
      at.root = place
      delete at.waitingFrom
      delete at.waitingSaid
      return {
        source,
        name: at.name,
        place,
        said: 'Connected. Look through it again to carry on: files already backed up are recognised by their fingerprints and not sent again.',
      }
    },
    async connectDestination(request) {
      await later(300 * pace)
      // The stand-in refuses one obviously wrong answer, as the real one would.
      if (request.kind !== 'folder' && !request.secretAccessKey) throw new Error('Enter both keys for the storage.')
      const name =
        request.kind === 'folder'
          ? `the folder ${request.place}`
          : request.kind === 'r2'
            ? `the Cloudflare R2 bucket ${request.bucket}`
            : request.kind === 'gcs'
              ? `the Google Cloud Storage bucket ${request.bucket}`
              : `the bucket ${request.bucket}`
      // Added to the list; a new place has nothing in it yet, so nothing
      // is safe until it has caught up.
      const already = dests.find((d) => d.name === name)
      if (!already) {
        dests.push({ name, kind: request.kind, folder: request.kind === 'folder' ? request.place : null, there: 0 })
        if (dests.length > 1) collections.forEach((c) => (c.gathered = 0))
      } else if (already.otherDisk) {
        // Added again as it is now: this disk is the one, and it is empty.
        already.otherDisk = false
        already.gone = already.there
        already.there = 0
        sentSoFar = 0
        collections.forEach((c) => (c.gathered = 0))
      }
      said()
      return name
    },
    async disconnectDestination(at) {
      if (at === undefined) dests = []
      else dests.splice(at, 1)
      // What is left may already have everything.
      const all = collections.filter((c) => c.kept).reduce((n, c) => n + c.photos + c.videos, 0)
      if (dests.length > 0 && dests.every((d) => d.there >= all)) collections.forEach((c) => c.kept && (c.gathered = c.photos + c.videos))
      said()
    },
    schedule: async () => scheduleView(),
    setSchedule: async (request) => {
      if (request.every === 'off') scheduleChoice = null
      else if (request.every)
        scheduleChoice = { v: 1, every: request.every, at: request.at ?? '02:30', weekday: request.weekday, day: request.day, paused: false }
      else if (scheduleChoice) scheduleChoice = { ...scheduleChoice, paused: request.paused ?? scheduleChoice.paused }
      skippingNext = false
      return scheduleView()
    },
    skipNextRun: async (undo) => {
      skippingNext = !undo
      return scheduleView()
    },
    async runScheduledNow() {
      waitingNow = null
      scheduleLast.unshift({ at: hoursAgo(0), why: 'when you pressed Run now', outcome: 'worked', said: 'Safely backed up 12 files (41.0 MB).' })
      await this.send()
    },
    backgroundSettings: async (request) => {
      if (request?.waitOnBattery !== undefined) manners.waitOnBattery = request.waitOnBattery
      if (request?.waitOnMetered !== undefined) manners.waitOnMetered = request.waitOnMetered
      if (request?.between) manners = { ...manners, between: request.between, betweenSaid: `between ${request.between.from} and ${request.between.to}` }
      if (request?.anyHour) manners = { ...manners, between: undefined, betweenSaid: undefined }
      return { ...manners }
    },
    windowUsed: async () => {},
    away: async () => awayView,
    awaySeen: async () => {
      awayView = { since: hoursAgo(0), backups: [], waiting: [] }
    },
    async send() {
      stopAsked = false
      sendStarted = Date.now()
      sendShare = 0
      void (async () => {
        // It looks through each source for anything new first, as the
        // engine does, and says what it found at the end.
        const found: string[] = []
        for (const source of sources) {
          sendListeners.forEach((l) => l({ line: `Looking for anything new in ${source.name}\u2026`, finished: false }))
          await later(new URLSearchParams(window.location.search).has('slow') ? 500 : 200 * pace)
          if (stopAsked) {
            sendStarted = null
            sendListeners.forEach((l) => l({ finished: true, outcome: 'stopped', summary: ['Stopped. Everything backed up so far is safe where it is. Back up again whenever you like and it carries on from here.'] }))
            return
          }
          const into = collections.find((c) => c.kept)
          if (source === sources[0] && addedSinceTheLook > 0 && into) {
            into.photos += addedSinceTheLook
            source.items += addedSinceTheLook
            found.push(foundNew(source.name, addedSinceTheLook, 0) ?? '')
            addedSinceTheLook = 0
          }
        }
        const kept = collections.filter((c) => c.kept)
        const keptOthers = otherFolders().flatMap((f) => otherRowsIn(f.key)).filter((r) => r.kept)
        const total = kept.reduce((n, c) => n + c.photos + c.videos, 0) + keptOthers.length
        sendListeners.forEach((l) => l({ line: 'Telling apart files that look alike: 0 of 12', finished: false }))
        await later(200 * pace)
        for (let step = 1; step <= 6; step++) {
          // `?slow` leaves time to press Stop.
          await later(new URLSearchParams(window.location.search).has('slow') ? 500 : 200 * pace)
          const done = Math.round((total * step) / 6)
          sendShare = step / 6
          if (stopAsked) {
            sendStarted = null
            sendListeners.forEach((l) =>
              l({
                finished: true,
                summary: [
                  `Safely backed up ${done.toLocaleString('en-GB')} files.`,
                  'Stopped. Everything backed up so far is safe where it is. Back up again whenever you like and it carries on from here.',
                ],
              }),
            )
            return
          }
          sendListeners.forEach((l) =>
            l({ line: `${done.toLocaleString('en-GB')} of ${total.toLocaleString('en-GB')} safely backed up\toriginals/Holidays/Cornwall/IMG_${1000 + done}.jpg`, finished: false }),
          )
        }
        const fresh = total - sentSoFar
        // What a look found gone is said first, and sent again.
        const goneSaid = dests.flatMap((d) => lookedAt(d).said)
        const restored = dests.reduce((n, d) => n + (d.gone ?? 0), 0)
        dests.forEach((d) => (d.gone = 0))
        sendStarted = null
        const now = Date.now() / 1000
        const to = sendingTo ?? ''
        sends.unshift({
          finishedAt: now,
          seconds: 96,
          bytes: 1.2 * GB,
          to,
          stopped: false,
          detail: {
            startedAt: now - 96,
            pausedSeconds: 0,
            ended: 'finished',
            startedBy: 'the app',
            version: '0.32.0',
            stages: [
              { kind: 'placing', unit: 'files', done: fresh, seconds: 1, finished: true, perSecond: fresh, slowest: null, fastest: null },
              { kind: 'sendingOriginals', to, unit: 'bytes', done: 1.1 * GB, seconds: 80, finished: true, perSecond: (1.1 * GB) / 80, slowest: 13 * MB, fastest: 15 * MB },
              { kind: 'sendingPreviews', to, unit: 'bytes', done: 0.1 * GB, seconds: 14, finished: true, perSecond: (0.1 * GB) / 14, slowest: null, fastest: null },
            ],
            destinations: [
              {
                to,
                overNetwork: false,
                reached: true,
                seconds: 96,
                bytes: 1.2 * GB,
                originals: { sent: fresh, sentBytes: 1.1 * GB, alreadyThere: sentSoFar, notSent: [], notSentMore: 0 },
                indexed: total,
              },
            ],
            duplicates: 0,
            duplicateBytes: 0,
            summary: [`Safely backed up ${fresh.toLocaleString('en-GB')} files (1.1 GB).`],
          },
        })
        sentSoFar = total
        dests.forEach((d) => (d.there = total))
        // Sent means safe: nothing is waiting any more.
        kept.forEach((c) => (c.gathered = c.photos + c.videos))
        keptOthers.forEach((r) => otherSent.add(r.id))
        if (dropNextBackup && fresh > 5) {
          // Five could not be sent while the share was away; they wait.
          dropNextBackup = false
          sentSoFar = total - 5
          dests.forEach((d) => (d.there = total - 5))
          let left = 5
          for (const c of kept) {
            const take = Math.min(left, c.gathered)
            c.gathered -= take
            left -= take
          }
          const n = (x: number) => x.toLocaleString('en-GB')
          sendListeners.forEach((l) =>
            l({
              finished: true,
              outcome: 'someFiles',
              missed: 5,
              checkAdvised: true,
              summary: [
                ...found,
                `Safely backed up ${n(fresh - 5)} files.`,
                `Couldn't back up 5 files, because the connection to ${to} was lost for a while: IMG_1001.jpg, IMG_1002.jpg, IMG_1003.jpg and 2 more.`,
                'Back up again to try them once more.',
                `The connection to ${to} was lost during this backup. To be certain, "Check they all arrived" looks at each one again.`,
                `${n(total - 5)} of the ${n(total)} files you are keeping are safely at ${to}. 5 still need attention: 5 couldn't be sent.`,
              ],
            }),
          )
          return
        }
        sendListeners.forEach((l) =>
          l({
            finished: true,
            summary: [
              ...found,
              ...goneSaid,
              fresh > 0
                ? `Safely backed up ${fresh.toLocaleString('en-GB')} files.`
                : `${total.toLocaleString('en-GB')} files already there.`,
              ...(restored > 0 ? [`Restored ${restored.toLocaleString('en-GB')} files that had gone from there.`] : []),
              `All ${total.toLocaleString('en-GB')} files you are keeping are safely at ${to}.`,
              ...(fresh > 0 ? [`Previews added: ${(fresh * 2).toLocaleString('en-GB')} files.`] : []),
              ...(fresh > 0 ? ['That took about 2 minutes: 1.2 GB at an average of 12.8 MB a second.'] : []),
            ],
          }),
        )
      })()
    },
    putRightPlan: async () => {
      const first = dests[0]
      const known = (first?.gone ?? 0) + (first?.notAsSent ?? 0)
      const files = sentSoFar + known
      const bytes = files * 3.2 * MB
      const place = {
        to: first?.name ?? '',
        known,
        knownBytes: known * 3.2 * MB,
        files,
        bytes,
        provedAlready: 0,
        quickSeconds: 120,
        thoroughSeconds: 4 * 3600,
        setsAside: first?.kind === 'folder',
        said: [
          ...(known > 0
            ? [`First, Keevoro puts right the ${known} files it already knows are wrong at ${first?.name}, which should take about 5 minutes.`]
            : []),
          `${known > 0 ? 'Then it reads' : 'Keevoro reads'} back in full each of the ${files.toLocaleString('en-GB')} files you are keeping ${known > 0 ? 'there' : `at ${first?.name}`} and compares it with its original: ${(bytes / GB).toFixed(1)} GB, which should take about 4 hours over this connection. It works in the background, so you can use the rest of Keevoro meanwhile, and you can stop it and carry on later.`,
          'Any copy that is short, the wrong size or not the same as its original is moved aside there, into a folder called "Set aside by Keevoro", with a note saying what it is. Nothing is deleted or replaced. Then the original is sent again and its new copy read back.',
        ],
      }
      return { places: [place], known, quickSeconds: 120, thoroughSeconds: 4 * 3600, quickSaid: 'about 5 minutes', thoroughSaid: 'about 4 hours' }
    },
    async putRight(thorough) {
      void (async () => {
        const first = dests[0]
        if (!first) return
        const moved = first.notAsSent ?? 0
        const sentAgain = moved + (first.gone ?? 0)
        const total = sentSoFar + sentAgain
        for (let step = 1; step <= 4; step++) {
          await later(150 * pace)
          sendListeners.forEach((l) =>
            l({ line: `Reading back and checking ${Math.round((total * step) / 4).toLocaleString('en-GB')} of ${total.toLocaleString('en-GB')}`, finished: false }),
          )
        }
        first.there += sentAgain
        first.gone = 0
        first.notAsSent = 0
        sentSoFar = total
        collections.filter((c) => c.kept).forEach((c) => (c.gathered = c.photos + c.videos))
        const n = total.toLocaleString('en-GB')
        sendListeners.forEach((l) =>
          l({
            finished: true,
            outcome: 'worked',
            summary: [
              ...(moved > 0
                ? [`Moved ${moved} damaged copies aside into "Set aside by Keevoro/2026-11-22" at ${first.name}, with a note saying what they are. Nothing was deleted; you can delete that folder once you are happy.`]
                : []),
              ...(sentAgain > 0 ? [`Sent ${sentAgain} files again, and read each one back: they match.`] : []),
              thorough
                ? `Your backup at ${first.name} is right: all ${n} files you are keeping are there, and every one was read back in full and matches its original.`
                : `All ${n} files you are keeping are at ${first.name}: ${sentAgain} read back in full and they match, and ${(total - sentAgain).toLocaleString('en-GB')} were the right size when Keevoro last looked.`,
            ],
          }),
        )
      })()
    },
    async checkArrived() {
      void (async () => {
        await later(400 * pace)
        sendListeners.forEach((l) =>
          l({
            finished: true,
            summary: [
              `Your backup: all ${sentSoFar.toLocaleString('en-GB')} originals are there.`,
            ],
          }),
        )
      })()
    },
    onSend(listener) {
      sendListeners.add(listener)
      return () => sendListeners.delete(listener)
    },
    onScan(listener) {
      scanListeners.add(listener)
      return () => scanListeners.delete(listener)
    },
    keepChecking: async () => checkLeftOver,
    pauseChecking: async (paused) => {
      checkHeld = paused && checkRunning
    },
    stopChecking: async () => {
      if (checkRunning) checkStopAsked = true
    },
    checkDuplicatesNow: async () => {
      void checkInBackground(Math.max(1, checkLeftOver?.left ?? 40))
    },
    onCheck(listener) {
      checkListeners.add(listener)
      return () => checkListeners.delete(listener)
    },
    keepRunning: async () => ({ ...keepRunning }),
    setKeepRunning: async (change) => {
      keepRunning = {
        ...keepRunning,
        keepRunning: change.keepRunning ?? keepRunning.keepRunning,
        minimiseToTray: change.minimiseToTray ?? keepRunning.minimiseToTray,
        startWithComputer: change.startWithComputer ?? keepRunning.startWithComputer,
        notify: change.notify ?? keepRunning.notify,
      }
      return { ...keepRunning }
    },
    windowAsk: async () => {
      const asked = windowAsked
      windowAsked = null
      return asked
    },
    onWindowAsk: () => () => {},
    hideToTray: async (startWithComputer) => {
      // A browser has no icon by the clock to hide to.
      if (startWithComputer) keepRunning = { ...keepRunning, startWithComputer: true }
    },
    quitKeevoro: async (stop) =>
      new URLSearchParams(window.location.search).has('busy') && !stop
        ? { asked: 'A backup is running. Stop it after the file in hand and quit? Nothing is lost.' }
        : {},
  }
}
