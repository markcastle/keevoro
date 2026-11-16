// A stand-in for the engine, used when the screens are opened in a browser.
//
// It holds a small made-up library in memory and behaves like the real
// thing: adding a place "finds" photos over a couple of seconds, keeping a
// collection sticks, gathering counts up. The pictures are drawn on the
// spot, so nothing is downloaded and no real photos are in the repository.
//
// Add ?demo=new to the address to start as a first-time user with no
// library, or ?demo=empty for a library with nothing found yet.

import type { AppState, Backend, Collection, Discover, GatherEvent, Photo, PhotoChoice, ScanEvent } from './backend'

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
  const gatherListeners = new Set<(e: GatherEvent) => void>()
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
    async addPlace() {
      const name = sources.length === 0 ? 'Family hard drive' : `Old laptop ${sources.length}`
      const adding = FOUND.filter((_, i) => i >= collections.length).slice(0, sources.length === 0 ? 5 : 2)
      void (async () => {
        let photos = 0
        let videos = 0
        for (const make of adding) {
          const found = make()
          for (let step = 1; step <= 4; step++) {
            await later(220 * pace)
            scanListeners.forEach((l) =>
              l({ name, photos: photos + Math.round((found.photos * step) / 4), videos, finished: false }),
            )
          }
          photos += found.photos
          videos += found.videos
          collections.unshift(found)
        }
        if (junk.length === 0) junk.push(...junkFound())
        sources.push({ id: sources.length + 1, name, root: 'E:\\', items: photos + videos })
        scanListeners.forEach((l) => l({ name, photos, videos, finished: true }))
      })()
      return true
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
    onScan(listener) {
      scanListeners.add(listener)
      return () => scanListeners.delete(listener)
    },
    onGather(listener) {
      gatherListeners.add(listener)
      return () => gatherListeners.delete(listener)
    },
  }
}
