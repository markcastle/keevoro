// A stand-in for the engine, used when the screens are opened in a browser.
//
// It holds a small made-up library in memory and behaves like the real
// thing: adding a place "finds" photos over a couple of seconds, keeping a
// collection sticks, gathering counts up. The pictures are drawn on the
// spot, so nothing is downloaded and no real photos are in the repository.
//
// Add ?demo=new to the address to start as a first-time user with no
// library, or ?demo=empty for a library with nothing found yet.

import type {
  AppState,
  BackupEvent,
  Backend,
  CheckEvent,
  Collection,
  Discover,
  GatherEvent,
  Photo,
  PhotoChoice,
  ScanEvent,
} from './backend'

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

export function demoBackend(): Backend {
  const mode = new URLSearchParams(window.location.search).get('demo')
  const suggested = 'C:\\Users\\You\\Pictures\\Keevoro Library'
  let library: string | null = mode === 'new' ? null : suggested
  const collections: Collection[] = mode === 'new' || mode === 'empty' ? [] : FOUND.slice(0, 5).map((make) => make())
  const sources = collections.length > 0 ? [{ id: 1, name: 'Family hard drive', root: 'E:\\', items: 240 }] : []
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
  const scanListeners = new Set<(e: ScanEvent) => void>()
  const checkListeners = new Set<(event: CheckEvent) => void>()
  const gatherListeners = new Set<(e: GatherEvent) => void>()
  const backupListeners = new Set<(e: BackupEvent) => void>()
  let backedUpTo: string | null = null
  let backedUp = 0
  const later = (ms: number) => new Promise((done) => setTimeout(done, ms))
  // Tests can ask for everything to happen at once.
  const pace = mode === 'fast' || new URLSearchParams(window.location.search).has('fast') ? 0 : 1

  const state = (): AppState => ({ library, suggestedLibrary: suggested, version: 'demo' })
  const view = (): Discover => {
    const kept = collections.filter((c) => c.kept)
    return {
      photos: collections.reduce((n, c) => n + c.photos, 0),
      videos: collections.reduce((n, c) => n + c.videos, 0),
      bytes: collections.reduce((n, c) => n + (c.photos * 3.2 + c.videos * 48) * 1024 * 1024, 0),
      duplicates: collections.length * 31,
      junk: junk.length,
      waitingToGather: kept.reduce((n, c) => n + c.photos + c.videos - c.gathered, 0),
      sources: [...sources],
      collections: collections.map((c) => ({ ...c })),
    }
  }

  let opened = 0
  void opened
  // Pretends to look through a place, finding collections as it goes.
  const look = (name: string, root: string) => {
      const adding = FOUND.filter((_, i) => i >= collections.length).slice(0, sources.length === 0 ? 5 : 2)
      void (async () => {
        let photos = 0
        let videos = 0
        for (const make of adding) {
          const found = make()
          for (let step = 1; step <= 4; step++) {
            await later(220 * pace)
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
        }
        if (junk.length === 0) junk.push(...junkFound())
        sources.push({ id: Date.now(), name, root, items: photos + videos })
        scanListeners.forEach((l) => l({ name, photos, videos, finished: true }))
        // Then, as the real app does, the careful checking carries on behind.
        const total = Math.max(1, Math.round((photos + videos) / 6))
        for (let done = 0; done < total; done += Math.ceil(total / 5)) {
          checkListeners.forEach((l) => l({ done, total, finished: false }))
          // Long enough to be seen, however fast the rest is asked to go.
          await later(300)
        }
        checkListeners.forEach((l) => l({ done: total, total, finished: true }))
      })()
  }

  return {
    state: async () => state(),
    async chooseLibrary() {
      library = suggested
      return state()
    },
    async useLibrary(path) {
      library = path
      return state()
    },
    discover: async () => view(),
    places: async () => [
      { path: 'E:\\', name: 'KINGSTON (E:)', detail: 'USB or external drive \u00b7 32 GB', removable: true, askFirst: false },
      { path: 'C:\\', name: 'Windows (C:)', detail: 'This computer\u2019s disk \u00b7 512 GB', removable: false, askFirst: true },
    ],
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
    openLibrary: async () => {
      opened += 1
    },
    async keepCollection(key, keep) {
      const found = collections.find((c) => c.key === key)
      if (!found) return
      found.kept = keep
      inside.get(key)?.forEach((file) => (file.kept = keep))
    },
    async collectionPhotos(key) {
      const found = collections.find((c) => c.key === key)
      if (!found) throw new Error('That collection isn\u2019t there any more.')
      return filesOf(found).map((file) => ({ ...file }))
    },
    setAside: async () => junk.map((file) => ({ ...file })),
    forgetSource: async (id) => {
      const at = sources.findIndex((s) => s.id === id)
      if (at < 0) return
      sources.splice(at, 1)
      // The made-up collections are not tied to a source, so the newest go.
      collections.splice(0, sources.length === 0 ? collections.length : 2)
      if (sources.length === 0) junk.length = 0
    },
    async keepPhoto(id, keep) {
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
    preview: async (id) => pictures.get(id) ?? Promise.reject(new Error('This one can\u2019t be previewed')),
    async gather() {
      const kept = collections.filter((c) => c.kept)
      const total = kept.reduce((n, c) => n + c.photos + c.videos - c.gathered, 0)
      void (async () => {
        for (let step = 1; step <= 10; step++) {
          await later(180 * pace)
          gatherListeners.forEach((l) => l({ step: 'copying', done: Math.round((total * step) / 10), total }))
        }
        await later(300 * pace)
        gatherListeners.forEach((l) => l({ step: 'previews', done: total, total }))
        kept.forEach((c) => (c.gathered = c.photos + c.videos))
        await later(300 * pace)
        gatherListeners.forEach((l) =>
          l({
            step: 'finished',
            done: total,
            total,
            summary: [`Safely copied ${total.toLocaleString('en-GB')} photos and videos into your library.`],
          }),
        )
      })()
    },
    backupState: async () => backedUpTo,
    chooseBackupFolder: async () => 'F:\\Family Backup',
    async connectBackup(request) {
      await later(300 * pace)
      // The stand-in refuses one obviously wrong answer, as the real one would.
      if (request.kind !== 'folder' && !request.secretAccessKey) throw new Error('Enter both keys for the storage.')
      backedUpTo =
        request.kind === 'folder'
          ? `the folder ${request.place}`
          : request.kind === 'r2'
            ? `the Cloudflare R2 bucket ${request.bucket}`
            : `the bucket ${request.bucket}`
      return backedUpTo
    },
    async disconnectBackup() {
      backedUpTo = null
    },
    async backUp() {
      const total = collections.filter((c) => c.kept).reduce((n, c) => n + c.gathered, 0)
      void (async () => {
        for (let step = 1; step <= 6; step++) {
          await later(200 * pace)
          const done = Math.round((total * step) / 6)
          backupListeners.forEach((l) =>
            l({ line: `${done.toLocaleString('en-GB')} of ${total.toLocaleString('en-GB')} safely backed up`, finished: false }),
          )
        }
        const fresh = total - backedUp
        backedUp = total
        backupListeners.forEach((l) =>
          l({
            finished: true,
            summary: [
              fresh > 0
                ? `Safely backed up ${fresh.toLocaleString('en-GB')} files.`
                : `${total.toLocaleString('en-GB')} files already there.`,
            ],
          }),
        )
      })()
    },
    async checkBackup() {
      void (async () => {
        await later(400 * pace)
        backupListeners.forEach((l) =>
          l({
            finished: true,
            summary: [
              `Your library: all ${backedUp.toLocaleString('en-GB')} files are intact.`,
              `Your backup: all ${backedUp.toLocaleString('en-GB')} originals are there.`,
            ],
          }),
        )
      })()
    },
    onBackup(listener) {
      backupListeners.add(listener)
      return () => backupListeners.delete(listener)
    },
    onScan(listener) {
      scanListeners.add(listener)
      return () => scanListeners.delete(listener)
    },
    keepChecking: async () => {},
    onCheck(listener) {
      checkListeners.add(listener)
      return () => checkListeners.delete(listener)
    },
    onGather(listener) {
      gatherListeners.add(listener)
      return () => gatherListeners.delete(listener)
    },
  }
}
