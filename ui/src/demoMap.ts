// Where the space goes, made up for the browser version: a typical
// case, a Windows disk whose Pictures folder holds a camera roll, screen
// recordings, texture packs of zip files, an old Keevoro library and
// screenshots, beside the system folders Keevoro never looks through.
// Answered the way the real `places.map` answers: to a depth, with what
// is too small to see merged, and what would not be backed up said.

import type { KindView, MapNode, MapRequest, MapView } from './backend'

type Kind = 'photo' | 'video' | 'other' | 'carried'
const KINDS: Kind[] = ['photo', 'video', 'carried', 'other']

/**
 * The endings a person has put on the kinds list as other files, to be
 * carried along: files with them are a kind of their own on the map, not
 * everything else. Set for each answer.
 */
let carriedNow = new Set<string>()

/** The kind a made-up folder's files of `kind` count as now. */
const now = (folder: string, kind: Kind, file?: string): Kind =>
  kind === 'other' && carriedNow.has(endingOf(folder, 'other', file)) ? 'carried' : kind

/** A made-up folder. */
type Made = {
  name: string
  folders?: Made[]
  /** Files big enough to be shown one by one: name and bytes. */
  files?: [string, number][]
  /** The rest of its own files, by kind: how many and how much room. */
  rest?: Partial<Record<Kind, [number, number]>>
  /** Why Keevoro never looks through it. */
  skipped?: string
  madeByKeevoro?: boolean
  /** Of its own files, the bytes only in OneDrive. */
  onlineOnly?: number
  /** A source of this backup. */
  source?: boolean
  /** Of the files the backup keeps from it directly, how many are safe, and how many it keeps. */
  backedUp?: [number, number]
}

const KB = 1024
const MB = 1024 ** 2
const GB = 1024 ** 3

const zips = (names: string[], each: number): [string, number][] => names.map((n, i) => [`${n}.zip`, Math.round(each * (1 - i * 0.07))])

/** A made-up Pictures folder. */
const PICTURES: Made = {
  name: 'Pictures',
  rest: { photo: [6, 19 * MB], other: [3, 40 * KB] },
  folders: [
    {
      name: 'Camera Roll',
      files: [
        ['VID_20190704_1402.mp4', 1.1 * GB],
        ['VID_20200815_0930.mp4', 0.8 * GB],
      ],
      rest: { photo: [1_204, 3.7 * GB], video: [35, 0.6 * GB], other: [12, 2 * MB] },
      onlineOnly: 0.3 * GB,
      source: true,
      backedUp: [1_180, 1_241],
    },
    {
      name: 'Captures',
      files: [
        ['Minecraft 2019-05-02 21-14-07.mp4', 2.4 * GB],
        ['Minecraft 2019-06-11 19-02-44.mp4', 1.7 * GB],
        ['Factorio 2020-01-03 22-40-12.mp4', 1.3 * GB],
      ],
      rest: { video: [38, 3.7 * GB] },
    },
    {
      name: 'Texture packs',
      rest: { other: [4, 12 * KB] },
      folders: [
        { name: 'Minecraft', files: zips(['Faithful 64x', 'Sphax PureBD', 'John Smith', 'Dokucraft'], 2.6 * GB), rest: { other: [96, 1.4 * GB] } },
        { name: 'Skyrim', files: zips(['Noble Skyrim', 'Skyrim HD 2K', 'Static Mesh'], 1.9 * GB), rest: { other: [40, 0.6 * GB] } },
        { name: 'Factorio', files: zips(['High res'], 0.9 * GB), rest: { other: [12, 0.2 * GB] } },
      ],
    },
    {
      name: 'Keevoro Library',
      skipped: 'A library from an older Keevoro',
      madeByKeevoro: true,
      rest: { other: [1, 4 * MB] },
      folders: [
        { name: 'originals', rest: { photo: [2_240, 8.1 * GB], video: [90, 0.5 * GB] } },
        { name: 'renditions', rest: { photo: [4_660, 0.8 * GB] } },
      ],
    },
    { name: 'Screenshots', rest: { photo: [300, 0.9 * GB] } },
    { name: '2019', source: true, folders: [{ name: 'Cornwall', rest: { photo: [120, 0.4 * GB], video: [4, 0.35 * GB] }, backedUp: [124, 124] }] },
    { name: '.thumbnails', skipped: 'Hidden folder', rest: { photo: [2_000, 90 * MB] } },
  ],
}

/** A Windows disk with that Pictures folder on it. */
const DISK: Made = {
  name: 'Windows (C:)',
  files: [
    ['pagefile.sys', 16 * GB],
    ['hiberfil.sys', 6.4 * GB],
  ],
  folders: [
    { name: 'Windows', skipped: 'System folder', rest: { other: [182_400, 28 * GB], photo: [5_210, 0.3 * GB] } },
    { name: 'Program Files', skipped: 'System folder', rest: { other: [64_000, 14 * GB] } },
    {
      name: 'Users',
      folders: [
        {
          name: 'You',
          folders: [
            PICTURES,
            { name: 'Videos', folders: [{ name: 'Holidays', rest: { photo: [287, 1.8 * GB], video: [31, 86.2 * GB] } }] },
            { name: 'Documents', rest: { other: [4_120, 2.1 * GB], photo: [14, 30 * MB] } },
            { name: 'Projects', folders: [{ name: 'Garden shed', rest: { other: [240, 7.2 * GB] } }] },
            { name: 'AppData', skipped: 'App or cache folder', rest: { other: [210_000, 9.4 * GB], photo: [30_000, 0.6 * GB] } },
          ],
        },
      ],
    },
    {
      name: 'Games',
      folders: [{ name: 'Launcher', rest: { other: [9_800, 41 * GB] }, folders: [{ name: 'assets', rest: { photo: [1_840, 9 * MB] } }] }],
    },
    { name: 'Old laptop backup', folders: [{ name: 'Photos', rest: { photo: [902, 2.9 * GB], video: [12, 0.5 * GB] } }] },
  ],
}

/**
 * A family drive that is nearly all photos, as our first outside tester's
 * was: a camera's folders, scans sorted by family, and an old phone's
 * thirty small albums, each a sliver of the whole, which the map gathers
 * into "Smaller folders".
 */
const FAMILY_DRIVE: Made = {
  name: 'Family hard drive',
  rest: { other: [3, 40 * KB] },
  folders: [
    {
      name: 'Pictures',
      folders: [
        { name: 'Camera', folders: [{ name: 'DCIM', rest: { photo: [8_420, 31 * GB], video: [140, 6.2 * GB] } }] },
        {
          name: 'Scans',
          folders: [
            {
              name: 'Family Scans',
              folders: ['Smith', 'Jones', 'Brown', 'Taylor'].map((name, i) => ({ name, rest: { photo: [420 - i * 60, (4.2 - i * 0.7) * GB] } })),
            },
          ],
        },
        { name: 'Weddings', rest: { photo: [1_900, 7.4 * GB], video: [12, 1.6 * GB] } },
        // An old phone's thirty albums, each a sliver of the whole.
        ...Array.from({ length: 30 }, (_, i) => ({
          name: `Phone album ${String(i + 1).padStart(2, '0')}`,
          rest: { photo: [40 + ((i * 7) % 30), (90 + ((i * 37) % 120)) * MB] } as Made['rest'],
        })),
      ],
    },
    { name: 'Documents', rest: { other: [1_200, 1.1 * GB] } },
    { name: 'Videos', rest: { video: [30, 4.8 * GB] } },
  ],
}

/** Any other place: a card or drive of photos with a few other things. */
const anyPlace = (name: string): Made => ({
  name,
  rest: { other: [2, 8 * KB] },
  folders: [
    { name: 'DCIM', folders: [{ name: '100CANON', rest: { photo: [210, 0.9 * GB], video: [30, 0.5 * GB] } }] },
    { name: 'Holidays', rest: { photo: [40, 0.1 * GB] } },
    { name: 'Work', rest: { other: [300, 0.4 * GB] } },
  ],
})

/** The ending each kind of file has in a made-up folder. */
const endingOf = (folder: string, kind: Kind, file?: string) =>
  file
    ? (file.split('.').pop() ?? '').toLowerCase()
    : kind === 'video'
      ? 'mp4'
      : kind === 'photo'
        ? folder === 'Screenshots'
          ? 'png'
          : 'jpg'
        : folder.startsWith('Texture') || ['Minecraft', 'Skyrim', 'Factorio'].includes(folder)
          ? 'zip'
          : folder === 'Windows' || folder === 'Program Files' || folder === 'AppData'
            ? 'dll'
            : folder === 'Documents'
              ? 'pdf'
              : folder === 'Garden shed'
                ? 'blend'
                : 'txt'

type Sums = Record<Kind, [number, number]> & { online: number; folders: number }

function sums(made: Made): Sums {
  const total: Sums = { photo: [0, 0], video: [0, 0], other: [0, 0], carried: [0, 0], online: made.onlineOnly ?? 0, folders: 0 }
  for (const kind of ['photo', 'video', 'other'] as Kind[]) {
    const [files, bytes] = made.rest?.[kind] ?? [0, 0]
    const counted = now(made.name, kind)
    total[counted][0] += files
    total[counted][1] += bytes
  }
  for (const [name, bytes] of made.files ?? []) {
    const kind = now(made.name, kindOf(name), name)
    total[kind][0] += 1
    total[kind][1] += bytes
  }
  for (const inside of made.folders ?? []) {
    const more = sums(inside)
    for (const kind of KINDS) {
      total[kind][0] += more[kind][0]
      total[kind][1] += more[kind][1]
    }
    total.online += more.online
    total.folders += 1 + more.folders
  }
  return total
}

/** What the backup keeps from a made-up folder and has backed up, everything inside it counted in. */
function backedUpIn(made: Made): [number, number] {
  const own = made.backedUp ?? [0, 0]
  return (made.folders ?? []).map(backedUpIn).reduce(([a, b], [c, d]) => [a + c, b + d], own)
}

/** Is a made-up path a source, or inside one? */
function sourceOf(tree: Made, top: string, path: string): 'source' | 'inSource' | null {
  if (find(tree, top, path)?.source) return 'source'
  for (let up = path.slice(0, path.lastIndexOf('\\')); up.includes('\\') || up === top; up = up.slice(0, up.lastIndexOf('\\'))) {
    if (find(tree, top, up)?.source) return 'inSource'
    if (up === top) break
  }
  return null
}

/** The endings a made-up folder holds, everything inside it counted in. */
function endingsIn(made: Made, into = new Set<string>()) {
  for (const kind of ['photo', 'video', 'other'] as Kind[]) if ((made.rest?.[kind]?.[0] ?? 0) > 0) into.add(endingOf(made.name, kind))
  for (const [name] of made.files ?? []) into.add(endingOf(made.name, kindOf(name), name))
  made.folders?.forEach((f) => endingsIn(f, into))
  return into
}

const kindOf = (file: string): Kind => (/\.(mp4|mov)$/i.test(file) ? 'video' : /\.(jpg|png|heic)$/i.test(file) ? 'photo' : 'other')

/** The made-up folder at `path`, given the place the tree is of. */
function find(tree: Made, top: string, path: string): Made | null {
  if (path === top) return tree
  if (!path.startsWith(`${top}\\`)) return null
  let at: Made | undefined = tree
  for (const part of path.slice(top.length + 1).split('\\')) {
    at = at?.folders?.find((f) => f.name === part)
    if (!at) return null
  }
  return at ?? null
}

/** The tree a place's map is drawn from, and the folder it is at. */
function treeFor(place: string): { tree: Made; top: string } {
  if (/^C:\\?$/.test(place) || place.startsWith('C:\\')) return { tree: DISK, top: 'C:' }
  if (/\\Pictures$/.test(place)) return { tree: PICTURES, top: place }
  if (/^E:\\?$/.test(place) || place.startsWith('E:\\')) return { tree: FAMILY_DRIVE, top: 'E:' }
  return { tree: anyPlace(place.replace(/[\\/]+$/, '').split(/[\\/]/).pop() || place), top: place.replace(/[\\/]+$/, '') }
}

/** Answers `places.map` for the made-up places. */
export function demoMap(
  request: MapRequest,
  leftOutOf: (path: string) => string | undefined,
  surveyedAt: number,
  carried: Set<string> = new Set(),
): MapView | null {
  carriedNow = carried
  const place = request.place.replace(/[\\/]+$/, '')
  const folder = (request.folder ?? request.place).replace(/[\\/]+$/, '')
  const { tree, top } = treeFor(place)
  const at = find(tree, top, folder)
  const start = find(tree, top, place)
  if (!at || !start) return null
  const total = sums(at)
  const bytes = total.photo[1] + total.video[1] + total.other[1] + total.carried[1]
  const smallest = Math.max(1, request.smallest ?? Math.floor(bytes / 2000))
  const depth = Math.min(6, Math.max(1, request.depth))

  // Each box says which endings it holds, as places in the answer's kinds.
  let places = new Map<string, number>()
  const placesOf = (endings: Iterable<string>) => [...endings].map((e) => places.get(e)).filter((i): i is number => i !== undefined).sort((a, b) => a - b)
  const node = (made: Made, path: string, level: number, passed: Made | null): MapNode => {
    const skipped = made.skipped ? made : passed
    const s = sums(made)
    const out = skipped ? undefined : leftOutOf(path)
    const media = s.photo[1] + s.video[1] + s.carried[1]
    const shown: MapNode = {
      name: made === tree ? (path === 'C:' ? 'Windows (C:)' : made.name) : made.name,
      path: path === 'C:' ? 'C:\\' : path,
      node: 'folder',
      bytes: s.photo[1] + s.video[1] + s.other[1] + s.carried[1],
      photos: s.photo[0],
      videos: s.video[0],
      others: s.other[0],
      photoBytes: s.photo[1],
      videoBytes: s.video[1],
      otherBytes: s.other[1],
      ...(s.carried[0] ? { carried: s.carried[0], carriedBytes: s.carried[1] } : {}),
      ...(s.online ? { onlineOnlyBytes: s.online } : {}),
      ...(s.folders ? { folders: s.folders } : {}),
      ...(skipped?.skipped ? { skipped: skipped.skipped } : {}),
      ...(skipped?.madeByKeevoro ? { madeByKeevoro: true, leftOut: 'Keevoro made this folder, so it is not a source' } : {}),
      ...(out ? { leftOut: 'you left this folder out of this backup', leftOutFolder: out } : {}),
      wouldBackUp: skipped || out ? 0 : media,
      endings: placesOf(endingsIn(made)),
    }
    const kept = backedUpIn(made)
    if (kept[1] > 0) shown.backedUp = { files: kept[0], of: kept[1] }
    const source = sourceOf(tree, top, path)
    if (source) shown[source] = true
    if (level === 0) {
      if ((made.folders?.length ?? 0) > 0 || made.files || made.rest) shown.deeper = true
      return shown
    }
    const children: MapNode[] = []
    const smaller = { bytes: 0, folders: 0, photo: [0, 0], video: [0, 0], other: [0, 0], carried: [0, 0], endings: new Set<string>() }
    for (const inside of made.folders ?? []) {
      const t = sums(inside)
      const b = t.photo[1] + t.video[1] + t.other[1] + t.carried[1]
      if (b >= smallest) children.push(node(inside, `${path}\\${inside.name}`, level - 1, skipped))
      else {
        smaller.bytes += b
        smaller.folders += 1 + t.folders
        endingsIn(inside, smaller.endings)
        for (const kind of KINDS) {
          smaller[kind][0] += t[kind][0]
          smaller[kind][1] += t[kind][1]
        }
      }
    }
    if (smaller.folders > 0) {
      children.push({
        name: 'Smaller folders',
        node: 'smaller',
        bytes: smaller.bytes,
        photos: smaller.photo[0],
        videos: smaller.video[0],
        others: smaller.other[0],
        photoBytes: smaller.photo[1],
        videoBytes: smaller.video[1],
        otherBytes: smaller.other[1],
        ...(smaller.carried[0] ? { carried: smaller.carried[0], carriedBytes: smaller.carried[1] } : {}),
        folders: smaller.folders,
        wouldBackUp: 0,
        endings: placesOf(smaller.endings),
      })
    }
    const rest: Record<Kind, [number, number]> = { photo: [0, 0], video: [0, 0], other: [0, 0], carried: [0, 0] }
    for (const kind of ['photo', 'video', 'other'] as Kind[]) {
      const [files, b] = made.rest?.[kind] ?? [0, 0]
      rest[now(made.name, kind)][0] += files
      rest[now(made.name, kind)][1] += b
    }
    const notes = (kind: Kind) =>
      skipped?.madeByKeevoro
        ? { skipped: skipped.skipped, madeByKeevoro: true, leftOut: 'Keevoro made this folder, so it is not a source' }
        : skipped
          ? { skipped: skipped.skipped }
          : out && kind !== 'other'
            ? { leftOut: 'you left this folder out of this backup' }
            : {}
    const one = (kind: Kind, files: number, b: number) => ({
      photos: kind === 'photo' ? files : 0,
      videos: kind === 'video' ? files : 0,
      others: kind === 'other' ? files : 0,
      photoBytes: kind === 'photo' ? b : 0,
      videoBytes: kind === 'video' ? b : 0,
      otherBytes: kind === 'other' ? b : 0,
      ...(kind === 'carried' ? { carried: files, carriedBytes: b } : {}),
    })
    for (const [name, b] of made.files ?? []) {
      const kind = now(made.name, kindOf(name), name)
      if (b < smallest) {
        rest[kind][0] += 1
        rest[kind][1] += b
        continue
      }
      children.push({
        name,
        node: 'file',
        kind,
        bytes: b,
        ...one(kind, 1, b),
        ...notes(kind),
        wouldBackUp: kind === 'other' || skipped || out ? 0 : b,
        endings: placesOf([endingOf(made.name, kind, name)]),
      })
    }
    for (const kind of KINDS) {
      const [files, b] = rest[kind]
      if (files === 0) continue
      children.push({
        name:
          kind === 'photo'
            ? 'Photos'
            : kind === 'video'
              ? 'Videos'
              : kind === 'carried'
                ? 'Other files to carry along'
                : carriedNow.size > 0
                  ? 'Everything else'
                  : 'Other files',
        node: 'files',
        kind,
        bytes: b,
        ...one(kind, files, b),
        ...(made.onlineOnly && kind !== 'other' ? { onlineOnlyBytes: Math.round(made.onlineOnly / 2) } : {}),
        ...notes(kind),
        wouldBackUp: kind === 'other' || skipped || out ? 0 : b,
        endings: placesOf([endingOf(made.name, kind)]),
      })
    }
    shown.children = children.sort((a, b) => b.bytes - a.bytes)
    return shown
  }

  // The files by ending, everything inside the folder shown counted in.
  const endings = new Map<string, KindView>()
  const count = (made: Made) => {
    for (const kind of ['photo', 'video', 'other'] as Kind[]) {
      const [files, b] = made.rest?.[kind] ?? [0, 0]
      if (files > 0) add(endingOf(made.name, kind), now(made.name, kind), files, b)
    }
    for (const [name, b] of made.files ?? []) add(endingOf(made.name, kindOf(name), name), now(made.name, kindOf(name), name), 1, b)
    made.folders?.forEach(count)
  }
  const add = (ending: string, kind: Kind, files: number, b: number) => {
    const was = endings.get(ending) ?? { ending, kind, files: 0, bytes: 0 }
    endings.set(ending, { ...was, files: was.files + files, bytes: was.bytes + b })
  }
  count(at)
  const kinds = [...endings.values()].sort((a, b) => b.bytes - a.bytes)
  places = new Map(kinds.map((k, i) => [k.ending, i]))

  const trail: { name: string; folder: string }[] = []
  const placePath = place === 'C:' ? 'C:' : place
  for (let path = folder; ; path = path.slice(0, path.lastIndexOf('\\'))) {
    const made = find(tree, top, path)
    trail.unshift({ name: made === tree && top === 'C:' ? 'Windows (C:)' : (made?.name ?? path), folder: path === 'C:' ? 'C:\\' : path })
    if (path === placePath || !path.includes('\\')) break
  }

  return {
    place: request.place,
    folder: request.folder ?? request.place,
    trail,
    surveyedAt,
    complete: true,
    sizedAll: true,
    top: node(at, folder, depth, ancestorsSkipped(tree, top, folder)),
    kinds,
    smallest,
    backup: 'Family photos',
  }
}

/** The folder Keevoro never looks through that `path` is inside, if any. */
function ancestorsSkipped(tree: Made, top: string, path: string): Made | null {
  for (let up = path.slice(0, path.lastIndexOf('\\')); up.length >= top.length && up.includes('\\'); up = up.slice(0, up.lastIndexOf('\\'))) {
    const made = find(tree, top, up)
    if (made?.skipped) return made
  }
  return null
}
