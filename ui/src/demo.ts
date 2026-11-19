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
  SendEvent,
  Backend,
  CheckEvent,
  Collection,
  Discover,
  FileType,
  GatherEvent,
  Photo,
  PhotoChoice,
  Sample,
  Sent,
  ScanEvent,
  SurveyProgress,
  Backup,
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
  const scanListeners = new Set<(e: ScanEvent) => void>()
  const surveyListeners = new Set<(e: SurveyProgress) => void>()
  const checkListeners = new Set<(event: CheckEvent) => void>()
  const gatherListeners = new Set<(e: GatherEvent) => void>()
  const sendListeners = new Set<(e: SendEvent) => void>()
  // What the last backup said when it ended, as the real app keeps it.
  let lastSaid: SendEvent | null = null
  sendListeners.add((e) => {
    if (e.finished) lastSaid = e
  })
  // Where the open backup goes: each place, and how many files are there.
  type Place = { name: string; kind: string; folder: string | null; there: number }
  let dests: Place[] = []
  let sendingTo: string | null = null
  const said = () => {
    sendingTo = dests.length === 0 ? null : dests.length === 1 ? dests[0].name : `${dests.length} places`
  }
  let stopAsked = false
  let stopLooking = false
  let systemCodecs = true
  let previews = true
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
  // One earlier send, so there is something to compare a new one with.
  const sends: Sent[] = [
    { finishedAt: Date.UTC(2026, 10, 9, 14, 2) / 1000, seconds: 41 * 60, bytes: 30.1 * 1024 ** 3, to: 'the folder F:\\Family Backup', stopped: false },
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
  const later = (ms: number) => new Promise((done) => setTimeout(done, ms))
  // Tests can ask for everything to happen at once.
  const pace = mode === 'fast' || new URLSearchParams(window.location.search).has('fast') ? 0 : 1

  // Backups: each keeps what it found, what was chosen and where it sends.
  type Kept = { collections: Collection[]; sources: typeof sources; junk: PhotoChoice[]; dests: Place[]; sentSoFar: number }
  let backups: Backup[] = library ? [{ id: 's1', name: 'My photos' }] : []
  let open: string | null = backups[0]?.id ?? null
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
  const state = (): AppState => ({
    library,
    suggestedLibrary: suggested,
    backup: backups.find((s) => s.id === open) ?? null,
    backups: [...backups],
    version: 'demo',
  })
  const view = (): Discover => {
    const kept = collections.filter((c) => c.kept)
    return {
      photos: collections.reduce((n, c) => n + c.photos, 0),
      videos: collections.reduce((n, c) => n + c.videos, 0),
      bytes: collections.reduce((n, c) => n + (c.photos * 3.2 + c.videos * 48) * 1024 * 1024, 0),
      duplicates: collections.length * 31,
      duplicateBytes: collections.length * 31 * 3.4 * 1024 * 1024,
      junk: junk.length,
      waitingToGather: kept.reduce((n, c) => n + c.photos + c.videos - c.gathered, 0),
      waitingBytes: waitingBytes(),
      sources: [...sources],
      destinations: dests.map((d) => {
        const all = kept.reduce((n, c) => n + c.photos + c.videos, 0)
        const there = Math.min(d.there, all)
        return { name: d.name, kind: d.kind, folder: d.folder, there, waiting: all - there, waitingBytes: (all - there) * 3.2 * 1024 * 1024 }
      }),
      collections: collections.map((c) => ({ ...c })),
    }
  }

  let opened = 0
  void opened
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
            if (stopLooking) {
              // Stopped: nothing from this search is kept.
              added.forEach((c) => collections.splice(collections.indexOf(c), 1))
              scanListeners.forEach((l) => l({ name, photos, videos, finished: true, stopped: true }))
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
        // Looking through a source again replaces what was known about it.
        const known = sources.findIndex((s) => s.root === root)
        const entry = { id: known >= 0 ? sources[known].id : Date.now(), name: known >= 0 ? sources[known].name : name, root, items: photos + videos, bytes: (photos * 3.2 + videos * 48) * 1024 * 1024 }
        if (known >= 0) sources[known] = entry
        else sources.push(entry)
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
    async startBackup(name) {
      putAway()
      const taken = (n: string) => backups.some((s) => s.name.toLowerCase() === n.toLowerCase())
      const wanted = name.trim() || 'My photos'
      let called = wanted
      for (let n = 2; taken(called); n++) called = `${wanted} ${n}`
      const id = `s${backups.length + 1}`
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
    // A send a third of the way through 30 GB, at a steady 12.4 MB a second.
    sendProgress: async () => ({
      bytesDone: 9.6 * 1024 ** 3,
      bytesTotal: 30.1 * 1024 ** 3,
      speed: 12.4 * 1024 * 1024,
      secondsLeft: Math.round(((30.1 - 9.6) * 1024) / 12.4),
      overNetwork: false,
      // `?sending` pretends a backup was already running when the screen opened.
      running: new URLSearchParams(window.location.search).has('sending'),
      stopping: false,
      line: new URLSearchParams(window.location.search).has('sending') ? 'Working out where each file goes: 2,500 of 11,245' : '',
    }),
    sendHistory: async () => [...sends],
    lastBackupSaid: async () => lastSaid,
    pauseWork: async (paused) => {
      pausedNow = paused
    },
    acceleration: async () => speed(),
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
      const folder = sendingTo?.startsWith('the folder') ? sendingTo.replace('the folder ', '') : null
      // `?full` pretends the chosen disk is nearly full.
      const free = folder ? (new URLSearchParams(window.location.search).has('full') ? 90 * 1024 * 1024 : 118 * 1024 ** 3) : null
      const needed = waitingBytes()
      return { needed, folder, free, enough: free === null || free > needed + 200 * 1024 * 1024 }
    },
    discover: async () => view(),
    places: async () => [
      { path: 'E:\\', name: 'KINGSTON (E:)', detail: 'USB or external drive \u00b7 32 GB', removable: true, askFirst: false },
      { path: 'C:\\', name: 'Windows (C:)', detail: 'This computer\u2019s disk \u00b7 512 GB', removable: false, askFirst: true },
    ],
    // A made-up survey of a big disk: three real places and one of icons.
    async surveyPlace(path) {
      const drive = path.replace(/[\\/]+$/, '')
      for (let step = 1; step <= 4; step++) {
        surveyListeners.forEach((tell) =>
          tell({ folders: step * 9_000, files: step * 120_000, photos: step * 3_100, videos: step * 90 }),
        )
        await new Promise((r) => setTimeout(r, new URLSearchParams(window.location.search).has('slow') ? 400 : 60))
      }
      return {
        hints: [
          { folder: `${drive}\\Users\\Mark\\Videos\\Drone`, photos: 333, videos: 26, bytes: 94 * 1024 ** 3, likelyPhotos: true, alreadyASource: false },
          { folder: `${drive}\\Users\\Mark\\Pictures`, photos: 11_204, videos: 310, bytes: 61 * 1024 ** 3, likelyPhotos: true, alreadyASource: false },
          { folder: `${drive}\\Old laptop backup\\Photos`, photos: 902, videos: 12, bytes: 3.4 * 1024 ** 3, likelyPhotos: true, alreadyASource: false },
          { folder: `${drive}\\Games\\Launcher\\assets`, photos: 1_840, videos: 0, bytes: 9 * 1024 ** 2, likelyPhotos: false, alreadyASource: false },
        ],
        folders: 36_412,
        files: 481_907,
        unreadable: 3,
        seconds: 6.2,
        stopped: false,
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
    fileTypes: async () => kinds.map((k) => ({ ...k })),
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
      if (!dests.some((d) => d.name === name)) {
        dests.push({ name, kind: request.kind, folder: request.kind === 'folder' ? request.place : null, there: 0 })
        if (dests.length > 1) collections.forEach((c) => (c.gathered = 0))
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
      const total = kept.reduce((n, c) => n + c.photos + c.videos, 0)
      stopAsked = false
      void (async () => {
        sendListeners.forEach((l) => l({ line: 'Telling apart files that look alike: 0 of 12', finished: false }))
        await later(200 * pace)
        for (let step = 1; step <= 6; step++) {
          // `?slow` leaves time to press Stop.
          await later(new URLSearchParams(window.location.search).has('slow') ? 500 : 200 * pace)
          const done = Math.round((total * step) / 6)
          if (stopAsked) {
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
        sends.unshift({ finishedAt: Date.now() / 1000, seconds: 96, bytes: 1.2 * 1024 ** 3, to: sendingTo ?? '', stopped: false })
        sentSoFar = total
        dests.forEach((d) => (d.there = total))
        // Sent means safe: nothing is waiting any more.
        kept.forEach((c) => (c.gathered = c.photos + c.videos))
        sendListeners.forEach((l) =>
          l({
            finished: true,
            summary: [
              fresh > 0
                ? `Safely backed up ${fresh.toLocaleString('en-GB')} files.`
                : `${total.toLocaleString('en-GB')} files already there.`,
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
