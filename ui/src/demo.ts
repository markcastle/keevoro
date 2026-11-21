// A stand-in for the engine, used when the screens are opened in a browser.
//
// It holds a small made-up backup in memory and behaves like the real
// thing: adding a place "finds" photos over a couple of seconds, keeping a
// collection sticks, backing up counts up. The pictures are drawn on the
// spot, so nothing is downloaded and no real photos are in the repository.
//
// Add ?demo=new to the address to start as a first-time user with no
// backup, or ?demo=empty for a backup with nothing found yet.

import { demoMap } from './demoMap'
import type {
  TopBox,
  AppState,
  FileKind,
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
} from './backend'
import { count, size } from './format'
import { PictureQueue, fileKey } from './pictureQueue'

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
    folder: ['Music', 'Holiday mix'],
    files: ['01 Summer Breeze.flac', '02 Sea Song.flac', '03 Long Road Home.flac', '04 Harbour Lights.flac', '05 Cornish Air.flac'].map(
      (n, i) => [n, (28 + i * 3) * 1024 ** 2],
    ),
  },
]

/** As Keevoro says it of files only in OneDrive, passed over unopened. */
const onlineOnlySaid = (n: number) =>
  `${count(n, 'file')} here ${n === 1 ? 'is' : 'are'} in OneDrive only, not on this computer, so Keevoro did not open ${n === 1 ? 'it' : 'them'}. To include ${n === 1 ? 'it' : 'them'}, make ${n === 1 ? 'it' : 'them'} available offline in OneDrive first, then look again.`

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
  const firstHome = 'C:\\Users\\You\\AppData\\Keevoro\\backups\\s1'
  let library: string | null = mode === 'new' ? null : firstHome
  const collections: Collection[] = mode === 'new' || mode === 'empty' ? [] : FOUND.slice(0, 5).map((make) => make())
  const sources = collections.length > 0 ? [{ id: 1, name: 'Family hard drive', root: 'E:\\', items: 240, bytes: 1.4 * 1024 ** 3 }] : []
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
  const otherFolders = (): Collection[] => {
    if (sources.length === 0 || !backupWants.includes('others')) return []
    return OTHER_FOLDERS.flatMap(({ folder, files }) => {
      const found = files.filter(([name]) => carriedFound.has(endingOfName(name)))
      if (found.length === 0) return []
      const rows = found.map(([name, bytes]) => {
        const where = `E:\\${[...folder, name].join('\\')}`
        let row = otherRows.get(where)
        if (!row) {
          const id = (nextId++).toString(16).padStart(16, '0')
          row = { id, isVideo: false, isOther: true, name, sourcePath: where, thumbnail: null, taken: null, changed: '2019-03-03', bytes, kept: true, junkReason: null }
          otherRows.set(where, row)
        }
        return row
      })
      const n = rows.length
      return [
        {
          key: ['Other files', ...folder].join(' / '),
          title: folder[folder.length - 1],
          summary: `${n} other file${n === 1 ? '' : 's'}`,
          parents: folder.slice(0, -1),
          photos: 0,
          videos: 0,
          others: n,
          byFolder: true,
          bytes: rows.reduce((t, r) => t + (r.bytes ?? 0), 0),
          kept: rows.some((r) => r.kept),
          gathered: rows.filter((r) => r.kept && otherSent.has(r.id)).length,
          cover: rows[0],
          previews: [],
        },
      ]
    })
  }
  const otherRowsIn = (key: string) => {
    const folder = OTHER_FOLDERS.find((f) => ['Other files', ...f.folder].join(' / ') === key)
    return folder ? [...otherRows.values()].filter((r) => r.sourcePath === `E:\\${[...folder.folder, r.name].join('\\')}`) : []
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
  // On a Mac or Linux an unplugged drive can leave the folder it was
  // mounted on behind, empty, on the computer's own disk.
  byHand.keevoroUnplugLeavingTheFolder = () => (leaveEmptyOnLook = true)
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
    const name = d.name.charAt(0).toUpperCase() + d.name.slice(1)
    return {
      at: now,
      lastReached: now,
      fresh: true,
      reached: true,
      gone,
      notAsSent: 0,
      said:
        gone > 0
          ? [`${name} looks emptied or replaced: it no longer holds any of the ${gone.toLocaleString('en-GB')} files Keevoro backed up there. They will be sent again.`]
          : [],
    }
  }
  let dests: Place[] = []
  let sendingTo: string | null = null
  const said = () => {
    sendingTo = dests.length === 0 ? null : dests.length === 1 ? dests[0].name : `${dests.length} places`
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
    ['G:\\Old Keevoro backup', 583],
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
      wants.includes('others') && othersListed() ? 'other files' : '',
    ].filter(Boolean)
    const joined = named.length === 0 ? 'other files' : named.length === 1 ? named[0] : `${named.slice(0, -1).join(', ')} and ${named[named.length - 1]}`
    return everything(wants) ? joined : `${joined} only`
  }
  const safe = ' Anything already backed up stays backed up; it is just no longer kept in what this backup chooses.'
  const wantsView = (wants: Wants, own?: boolean, changed = false) => ({
    wants,
    own,
    ...(othersListed() ? { offersOthers: true } : {}),
    said: `This backup is for ${words(wants)}.${changed && !everything(wants) ? safe : ''}`,
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
  // The kinds of file looked for, as Keevoro comes with them, and one added.
  const builtInKinds = (): FileKind[] => [
    ...'jpg jpeg jpe png heic heif tif tiff webp bmp avif jxl dng cr2 cr3 crw nef nrw arw srf sr2 orf rw2 raf pef srw x3f 3fr erf kdc mrw raw iiq dcr mef mos rwl'
      .split(' ')
      .map((ending) => ({ ending, kind: 'photo' as const, on: true, builtIn: true })),
    ...'mp4 mov m4v avi mkv wmv mpg mpeg webm insv asf flv mts m2ts 3gp 3g2 mod tod vob dv'
      .split(' ')
      .map((ending) => ({ ending, kind: 'video' as const, on: true, builtIn: true })),
  ]
  let lookedFor: FileKind[] = [...builtInKinds(), { ending: 'insp', kind: 'photo', on: true, builtIn: false }]
  const plainEnding = (typed: string) => {
    const ending = typed.trim().replace(/^\./, '').toLowerCase()
    if (!ending) throw new Error('Type the ending of the files, such as heif.')
    if (!/^[\p{L}\p{N}]+$/u.test(ending))
      throw new Error(`"${typed.trim()}" can’t be the ending of a file name. Use letters and numbers only, with no spaces or dots, such as heif.`)
    return ending
  }
  // A document, music, a program or an archive is never a photo or video.
  const refuseAsMedia = (ending: string, kind: FileKind['kind']) => {
    if (kind !== 'other' && ['exe', 'pdf', 'mp3', 'flac', 'zip', 'txt', 'docx'].includes(ending))
      throw new Error(`Files ending .${ending} are programs, documents, music or archives, never photos or videos. To back them up as they are, add .${ending} as an other file.`)
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
      seconds: 82_200,
      bytes: 720.6 * GB,
      to: nas,
      stopped: false,
      detail: {
        startedAt: Date.UTC(2026, 10, 18, 8, 51) / 1000,
        pausedSeconds: 600,
        ended: 'finished',
        startedBy: 'the app',
        version: '0.32.0',
        stages: [
          { kind: 'tellingApart', unit: 'files', done: 214, seconds: 480, finished: true, perSecond: 214 / 480, slowest: 0.31, fastest: 0.62 },
          { kind: 'placing', unit: 'files', done: 11_245, seconds: 40, finished: true, perSecond: 11_245 / 40, slowest: null, fastest: null },
          { kind: 'sendingOriginals', to: nas, unit: 'bytes', done: 716.8 * GB, seconds: 75_600, finished: true, perSecond: (716.8 * GB) / 75_600, slowest: 3.2 * MB, fastest: 14.6 * MB, problem: 'Couldn’t back up 2 files' },
          { kind: 'makingPreviews', unit: 'files', done: 10_896, seconds: 21_600, finished: true, perSecond: 10_896 / 21_600, slowest: 0.21, fastest: 1.9 },
          { kind: 'makingVideoPreviews', unit: 'files', done: 349, seconds: 2_400, finished: true, perSecond: 349 / 2_400, slowest: 0.09, fastest: 0.2 },
          { kind: 'sendingPreviews', to: nas, unit: 'bytes', done: 3.8 * GB, seconds: 4_800, finished: true, perSecond: (3.8 * GB) / 4_800, slowest: 0.6 * MB, fastest: 1.1 * MB },
          { kind: 'writingIndex', to: nas, unit: 'files', done: 1, seconds: 12, finished: true, perSecond: 1 / 12, slowest: null, fastest: null },
        ],
        destinations: [
          {
            to: nas,
            overNetwork: true,
            reached: true,
            seconds: 82_200,
            bytes: 720.6 * GB,
            originals: {
              sent: 11_243,
              sentBytes: 716.8 * GB,
              alreadyThere: 0,
              notSent: [
                { file: 'originals/2011/Summer/IMG_4410.JPG', reason: 'The source couldn’t be read: it may have been unplugged' },
                { file: 'originals/2011/Summer/IMG_4411.JPG', reason: 'The source couldn’t be read: it may have been unplugged' },
              ],
              notSentMore: 0,
            },
            previews: { sent: 21_792, sentBytes: 3.8 * GB, alreadyThere: 0, notSent: [], notSentMore: 0 },
            indexed: 11_243,
          },
        ],
        duplicates: 1_406,
        duplicateBytes: 38.2 * GB,
        previews: { photos: 10_896, videos: 349, photosWithout: 12, videosWithout: 0 },
        summary: [
          'Safely backed up 11,243 files (716.8 GB).',
          'Couldn’t back up 2 files:',
          '  originals/2011/Summer/IMG_4410.JPG: The source couldn’t be read: it may have been unplugged.',
          '  originals/2011/Summer/IMG_4411.JPG: The source couldn’t be read: it may have been unplugged.',
          'Back up again to try them once more.',
          'Previews added: 21,792 files (3.8 GB).',
          'That took about 22 hours 50 minutes: 720.6 GB at an average of 9.0 MB a second.',
        ],
      },
    },
    { finishedAt: Date.UTC(2026, 10, 9, 14, 2) / 1000, seconds: 41 * 60, bytes: 30.1 * GB, to: 'the folder F:\\Family Backup', stopped: false },
  ]
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
          stage({ kind: 'placing', state: 'now', unit: 'files', done: 2500, work: 11245, line: 'Working out where each file goes: 2,500 of 11,245' }),
          stage({ kind: 'sendingOriginals', state: 'next', unit: 'bytes', done: 0, work: 30.1 * GB, line: 'Backing up your files: 30.1 GB' }),
          stage({ kind: 'makingPreviews', state: 'next', unit: 'files', done: 0, work: 10547, line: 'Making previews: 10,547' }),
          stage({ kind: 'sendingPreviews', state: 'next', unit: 'bytes', done: 0, work: 4 * GB, line: 'Backing up the previews: 4.0 GB' }),
          stage({ kind: 'writingIndex', state: 'next', unit: 'files', done: 0, work: 1, line: 'Writing the list of what is there' }),
        ]
      : [
          stage({ kind: 'placing', state: 'done', unit: 'files', done: 11245, work: 11245, line: 'Working out where each file goes' }),
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
  // Tests can ask for everything to happen at once.
  const pace = mode === 'fast' || new URLSearchParams(window.location.search).has('fast') ? 0 : 1

  // Backups: each keeps what it found, what was chosen and where it sends.
  type Kept = { collections: Collection[]; sources: typeof sources; junk: PhotoChoice[]; dests: Place[]; sentSoFar: number }
  let backups: Backup[] = library ? [{ id: 's1', name: 'My photos' }] : []
  let open: string | null = backups[0]?.id ?? null
  let made = backups.length
  const shelf = new Map<string, Kept>()
  const putAway = () => {
    if (open) shelf.set(open, { collections: [...collections], sources: [...sources], junk: [...junk], dests: [...dests], sentSoFar })
  }
  const takeOut = (id: string) => {
    const kept = shelf.get(id) ?? { collections: [], sources: [], junk: [], dests: [], sentSoFar: 0 }
    collections.splice(0, collections.length, ...kept.collections)
    sources.splice(0, sources.length, ...kept.sources)
    junk.splice(0, junk.length, ...kept.junk)
    dests = [...kept.dests]
    said()
    sentSoFar = kept.sentSoFar
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
    const keptOthers = [...otherRows.values()].filter((r) => r.kept && folders.some((f) => otherRowsIn(f.key).includes(r)))
    const waitingOthers = keptOthers.filter((r) => !otherSent.has(r.id))
    return {
      photos: collections.reduce((n, c) => n + c.photos, 0),
      videos: collections.reduce((n, c) => n + c.videos, 0),
      ...(others > 0 ? { others } : {}),
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
      const adding = FOUND.filter((_, i) => i >= collections.length).slice(0, sources.length === 0 ? 5 : 2)
      stopLooking = false
      void (async () => {
        let photos = 0
        let videos = 0
        const added: Collection[] = []
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
        if (junk.length === 0) junk.push(...junkFound())
        // Other files of the kinds on the list now are found too.
        lookedFor.filter((k) => k.kind === 'other' && k.on).forEach((k) => carriedFound.add(k.ending))
        // Looking through a source again replaces what was known about it.
        const known = sources.findIndex((s) => s.root === root)
        // The owner's Pictures folder is kept in OneDrive, part of it online only.
        const online = root.endsWith('\\Pictures') ? { onlineOnly: 412, onlineOnlySaid: onlineOnlySaid(412) } : {}
        const entry = { id: known >= 0 ? sources[known].id : Date.now(), name: known >= 0 ? sources[known].name : name, root, items: photos + videos, bytes: (photos * 3.2 + videos * 48) * 1024 * 1024, ...online }
        if (known >= 0) sources[known] = entry
        else sources.push(entry)
        scanListeners.forEach((l) => l({ name, photos, videos, finished: true, ...online }))
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

  return {
    state: async () => state(),
    async startBackup(name) {
      putAway()
      const taken = (n: string) => backups.some((s) => s.name.toLowerCase() === n.toLowerCase())
      const wanted = name.trim() || 'My photos'
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
        backupStep: new URLSearchParams(window.location.search).has('sending') ? 'Working out where each file goes: 2,500 of 11,245' : null,
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
        line: sendingAtOpen && sendStarted !== null ? 'Working out where each file goes: 2,500 of 11,245' : '',
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
      if (lookedFor.some((k) => k.ending === ending)) throw new Error(`Keevoro already looks for .${ending} files.`)
      refuseAsMedia(ending, kind)
      const added = { ending, kind, on: true, builtIn: false }
      lookedFor = [...lookedFor, added]
      return { ...added }
    },
    changeFileKind: async (typed, change) => {
      const now = kindNamed(typed)
      const ending = change.to === undefined ? now.ending : plainEnding(change.to)
      if (ending !== now.ending) {
        if (now.builtIn)
          throw new Error(`.${now.ending} is one of the lookedFor Keevoro comes with. It can be switched off, but not removed or renamed.`)
        if (lookedFor.some((k) => k.ending === ending)) throw new Error(`Keevoro already looks for .${ending} files.`)
      }
      const changed = { ...now, ending, kind: change.kind ?? now.kind, on: change.on ?? now.on }
      refuseAsMedia(changed.ending, changed.kind)
      lookedFor = lookedFor.map((k) => (k === now ? changed : k))
      return { ...changed }
    },
    removeFileKind: async (typed) => {
      const gone = kindNamed(typed)
      if (gone.builtIn)
        throw new Error(`.${gone.ending} is one of the lookedFor Keevoro comes with. It can be switched off, but not removed or renamed.`)
      lookedFor = lookedFor.filter((k) => k !== gone)
      return { ...gone }
    },
    resetFileKinds: async () => {
      lookedFor = [...builtInKinds(), ...lookedFor.filter((k) => !k.builtIn)]
      return lookedFor.map((k) => ({ ...k }))
    },
    chooseAgentFolder: async () => (agents.lookIn.includes('D:\\Photos') ? 'F:\\Family Backup' : 'D:\\Photos'),
    // The owner's case: an old backup folder full of a stopped backup's working files.
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
      return { needed, folder, free, enough: free === null || free > needed + 200 * 1024 * 1024 }
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
            ...(step > 3 ? { sizing: true } : {}),
            top: topSoFar(drive, step / 5),
          }),
        )
        await new Promise((r) => setTimeout(r, new URLSearchParams(window.location.search).has('slow') ? 400 : 60))
      }
      const leftOutHere = [...leftOutFolders].filter((f) => within(f, drive))
      return {
        hints: [
          { folder: `${drive}\\Users\\Mark\\Videos\\Drone`, photos: 333, videos: 26, bytes: 94 * 1024 ** 3, likelyPhotos: true, alreadyASource: false },
          { folder: `${drive}\\Users\\Mark\\Pictures`, photos: 11_204, videos: 310, bytes: 61 * 1024 ** 3, likelyPhotos: true, alreadyASource: false, onlineOnly: 412, onlineOnlySaid: onlineOnlySaid(412) },
          { folder: `${drive}\\Old laptop backup\\Photos`, photos: 902, videos: 12, bytes: 3.4 * 1024 ** 3, likelyPhotos: true, alreadyASource: false },
          { folder: `${drive}\\Games\\Launcher\\assets`, photos: 1_840, videos: 0, bytes: 9 * 1024 ** 2, likelyPhotos: false, alreadyASource: false },
          // Other files carried along, once their kind is on the list.
          ...(listedAsOther('pdf')
            ? [{ folder: `${drive}\\Users\\Mark\\Documents`, photos: 0, videos: 0, others: 4_120, bytes: 2.1 * 1024 ** 3, likelyPhotos: true, alreadyASource: false }]
            : []),
          ...(listedAsOther('flac')
            ? [{ folder: `${drive}\\Users\\Mark\\Music`, photos: 0, videos: 0, others: 240, bytes: 7.2 * 1024 ** 3, likelyPhotos: true, alreadyASource: false }]
            : []),
        ].filter((hint) => !leftOutOf(hint.folder)),
        folders: 36_412,
        files: 481_907,
        unreadable: 3,
        seconds: 6.2,
        stopped: false,
        onlineOnly: 412,
        onlineOnlySaid: onlineOnlySaid(412),
        leftOut: [
          {
            folder: `${drive}\\Users\\Mark\\Pictures\\Keevoro Library`,
            what: 'oldLibrary',
            said: `Left out ${drive}\\Users\\Mark\\Pictures\\Keevoro Library: a folder an older version of Keevoro made, which it no longer uses. It takes 9.2 GB: 8.4 GB of copies of your photos it gathered there (2,310 files), and 0.8 GB of previews and working files it made. Keevoro has not checked that every copy is also in one of your backups, so it leaves the folder for you to decide about and never removes it.`,
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
    async scanPlace(path) {
      const name = path.startsWith('E')
        ? 'KINGSTON (E:)'
        : path === 'C:\\'
          ? 'Windows (C:)'
          : sources.length === 0
            ? 'Family hard drive'
            : `Old laptop ${sources.length}`
      look(name, path)
    },
    // The folder window is the system's own, so the stand-in just answers.
    pickPlace: async () => 'D:\\Photos',
    logTail: async () =>
      [
        '2026-11-16 14:02:09  Keevoro 9.8.0 started on windows (x86_64)',
        '2026-11-16 14:02:11  Looking through E:\\',
        '2026-11-16 14:02:19  Reading a large file (3.9 GB): E:\\Video\\wedding.mov',
        '2026-11-16 14:03:40  Looked through E:\\ in 89 seconds: 238 photos and videos, 11,204 other files, 3 folders skipped, 0 unreadable',
      ].join('\n'),
    openLog: async () => {},
    checkUpdate: async () =>
      new URLSearchParams(window.location.search).has('update')
        ? { version: '9.9.0', page: 'https://github.com/markcastle/keevoro/releases' }
        : null,
    installUpdate: async () => {
      await later(600 * pace)
    },
    openReleasePage: async () => {},
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
              ...photo('DJI_0042.MP4'),
              folder: 'E:\\Drone\\Lake District',
              bytes: 3.9 * 1024 ** 3,
              confirmed: false,
              copies: [{ name: 'DJI_0042.MP4', folder: 'E:\\Drone\\Unsorted' }],
            },
          ],
    setAside: async () => junk.map((file) => ({ ...file })),
    // Every source in the stand-in has the same made-up mix of kinds.
    backupWants: async () => wantsView(backupWants),
    setBackupWants: async (wants) => {
      backupWants = wants
      return wantsView(wants, undefined, true)
    },
    sourceWants: async (id) => sourceView(id),
    setSourceWants: async (id, wants) => {
      if (wants === 'backup') sourceChoices.delete(id)
      else sourceChoices.set(id, wants)
      return sourceView(id, true)
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
        return k.included ? { ...k } : { ...k, leftOutBy: 'ending' as const, why: 'files with this ending are left out of this source' }
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
    async connectDestination(request) {
      await later(300 * pace)
      // The stand-in refuses one obviously wrong answer, as the real one would.
      if (request.kind !== 'folder' && !request.secretAccessKey) throw new Error('Enter both keys for the storage.')
      const name =
        request.kind === 'folder'
          ? `the folder ${request.place}`
          : request.kind === 'r2'
            ? `the Cloudflare R2 bucket ${request.bucket}`
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
    async send() {
      const kept = collections.filter((c) => c.kept)
      const keptOthers = otherFolders().flatMap((f) => otherRowsIn(f.key)).filter((r) => r.kept)
      const total = kept.reduce((n, c) => n + c.photos + c.videos, 0) + keptOthers.length
      stopAsked = false
      sendStarted = Date.now()
      sendShare = 0
      void (async () => {
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
        sendListeners.forEach((l) =>
          l({
            finished: true,
            summary: [
              ...goneSaid,
              fresh > 0
                ? `Safely backed up ${fresh.toLocaleString('en-GB')} files.`
                : `${total.toLocaleString('en-GB')} files already there.`,
              ...(restored > 0 ? [`Restored ${restored.toLocaleString('en-GB')} files that had gone from there.`] : []),
              ...(fresh > 0 ? [`Previews added: ${(fresh * 2).toLocaleString('en-GB')} files.`] : []),
              ...(fresh > 0 ? ['That took about 2 minutes: 1.2 GB at an average of 12.8 MB a second.'] : []),
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
  }
}
