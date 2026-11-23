// The folders a survey suggests, grouped by where they live, so a long list
// of folders side by side in one place reads as one row that opens. Two or
// more suggestions sharing a parent are shown together under the nearest
// parent they share; a group never holds just one thing, and groups go no
// more than three deep. Kept apart from the screen so it can be checked
// without one, and quick enough for thousands of folders: each folder's
// path is walked once, then each level is sorted.

import type { Hint } from './backend'

/** What is inside a group, added up. `folders` counts every suggestion in it. */
export type Totals = { folders: number; photos: number; videos: number; others: number; music: number; bytes: number }

/** One suggested folder, labelled by the part of its path below the group it is in. */
export type FolderRow = { kind: 'folder'; hint: Hint; label: string }

/** Suggestions that live together, as one row that opens. */
export type GroupRow = {
  kind: 'group'
  /** The folder they share, written as the first of them writes it. */
  folder: string
  /** The same for the same folder however it is written, to remember it open or closed by. */
  key: string
  /** Its path below the group it is in, or all of it at the top. */
  label: string
  items: SurveyItem[]
  /** Every suggestion inside, at any depth, in the order given. */
  hints: Hint[]
  totals: Totals
  /** How many inside carry the note that they are probably not photos. */
  notPhotos: number
}

export type SurveyItem = FolderRow | GroupRow

/** Groups go no deeper than this on screen; below it their folders are listed flat. */
const DEEPEST = 3
/** A group holding more folders than this starts closed. */
const OPEN_UP_TO = 6
/** What a suggestion that is itself the group's folder is called inside it. */
const ITSELF = 'This folder itself'

/** True when a suggestion carries the note that its pictures are probably not photos. */
export const notPhotos = (hint: Hint) => !hint.likelyPhotos && !hint.alreadyASource

type Parsed = { root: string; rootKey: string; parts: string[]; keys: string[]; sep: string }

/**
 * A path as its root (a drive, a network share or `/`) and the names below
 * it. Windows paths are the same folder whatever their case or slashes.
 */
function parse(path: string): Parsed {
  const share = /^\\\\([^\\/]+)[\\/]+([^\\/]+)/.exec(path)
  const drive = /^([A-Za-z]):/.exec(path)
  const windows = Boolean(share || drive) || (!path.startsWith('/') && path.includes('\\'))
  const root = share ? `\\\\${share[1]}\\${share[2]}` : drive ? `${drive[1]}:` : path.startsWith('/') ? '/' : ''
  const rest = share ? path.slice(share[0].length) : drive ? path.slice(2) : path
  const parts = rest.split(windows ? /[\\/]+/ : /\/+/).filter(Boolean)
  return {
    root,
    rootKey: windows ? root.toLowerCase() : root,
    parts,
    keys: windows ? parts.map((p) => p.toLowerCase()) : parts,
    sep: windows ? '\\' : '/',
  }
}

/** True when `folder` is `parent` or anywhere beneath it. */
export function within(folder: string, parent: string): boolean {
  const a = parse(folder)
  const b = parse(parent)
  return a.rootKey === b.rootKey && b.keys.length <= a.keys.length && b.keys.every((k, i) => a.keys[i] === k)
}

/** The folder left out that holds this one, itself included, if there is one. */
export const outBy = (folder: string, out: Iterable<string>) => [...out].find((o) => within(folder, o))

/** A folder in the tree of suggestions, with what is at and beneath it. */
type Node = {
  name: string
  depth: number
  /** Its path, written as the first suggestion through it writes it. */
  path: string
  /** The names from the root down to it, as first written. */
  parts: string[]
  key: string
  children: Map<string, Node>
  /** Suggestions that are this very folder (more than one only when written differently). */
  own: { hint: Hint; at: number; parsed: Parsed }[]
  /** Suggestions at and beneath it. */
  count: number
  notPhotos: number
  /** The first suggestion beneath it, to keep the order given when sizes are equal. */
  first: number
  totals?: Totals
}

const node = (name: string, depth: number, path: string, parts: string[], key: string, at: number): Node => ({
  name,
  depth,
  path,
  parts,
  key,
  children: new Map(),
  own: [],
  count: 0,
  notPhotos: 0,
  first: at,
})

/** Steps taken, counted only when asked, to show the work grows with the folders and not their square. */
type Steps = { n: number }

/**
 * The suggestions as rows and groups, biggest first and probable icons
 * last, as the survey orders its folders. Rows keep the order given when
 * their sizes are equal.
 */
export function groupHints(hints: readonly Hint[], steps: Steps = { n: 0 }): SurveyItem[] {
  const roots = new Map<string, Node>()
  hints.forEach((hint, at) => {
    const parsed = parse(hint.folder)
    let here = roots.get(parsed.rootKey)
    if (!here) {
      here = node(parsed.root, 0, parsed.root, [], parsed.rootKey, at)
      roots.set(parsed.rootKey, here)
    }
    here.count += 1
    if (notPhotos(hint)) here.notPhotos += 1
    parsed.keys.forEach((key, i) => {
      steps.n += 1
      const from = here as Node
      let next = from.children.get(key)
      if (!next) {
        const name = parsed.parts[i]
        const joint = from.path === '' || from.path.endsWith(parsed.sep) ? '' : parsed.sep
        next = node(name, i + 1, `${from.path}${joint}${name}`, [...from.parts, name], `${from.key}\u0000${key}`, at)
        from.children.set(key, next)
      }
      next.count += 1
      if (notPhotos(hint)) next.notPhotos += 1
      here = next
    })
    here.own.push({ hint, at, parsed })
  })

  // Follows a folder down while it holds just one way on and no suggestion of its own.
  const settle = (from: Node): Node => {
    let here = from
    while (here.own.length === 0 && here.children.size === 1) {
      steps.n += 1
      here = here.children.values().next().value as Node
    }
    return here
  }
  // What a folder holds, added up once. A suggestion's own counts already
  // hold everything beneath it, so those beneath are not added again.
  const totalsOf = (n: Node): Totals => {
    if (n.totals) return n.totals
    steps.n += 1
    const sum: Totals = { folders: n.count, photos: 0, videos: 0, others: 0, music: 0, bytes: 0 }
    const add = (t: { photos: number; videos: number; others?: number; music?: number; bytes: number }) => {
      sum.photos += t.photos
      sum.videos += t.videos
      sum.others += t.others ?? 0
      sum.music += t.music ?? 0
      sum.bytes += t.bytes
    }
    if (n.own.length > 0) n.own.forEach((o) => add(o.hint))
    else n.children.forEach((c) => add(totalsOf(c)))
    n.totals = sum
    return sum
  }
  // Every suggestion at and beneath a folder, in the order given.
  const all = (n: Node): { hint: Hint; at: number; parsed: Parsed }[] => {
    const found: { hint: Hint; at: number; parsed: Parsed }[] = []
    const walk = (m: Node) => {
      steps.n += 1
      found.push(...m.own)
      m.children.forEach(walk)
    }
    walk(n)
    return found.sort((a, b) => a.at - b.at)
  }
  // The part of a path below a group, or all of it at the top.
  const below = (parts: string[], from: Node | null, whole: string) =>
    from === null ? whole : parts.slice(from.depth).join(' › ') || ITSELF
  // Probable icons go last, then the biggest first, then as given.
  const order = new WeakMap<SurveyItem, { last: boolean; bytes: number; at: number }>()
  const row = (o: { hint: Hint; at: number; parsed: Parsed }, from: Node | null): FolderRow => {
    const made: FolderRow = { kind: 'folder', hint: o.hint, label: below(o.parsed.parts, from, o.hint.folder) }
    order.set(made, { last: !o.hint.likelyPhotos, bytes: o.hint.bytes, at: o.at })
    return made
  }
  const groups = new WeakMap<GroupRow, Node>()
  const sorted = (items: SurveyItem[]) =>
    items.sort((a, b) => {
      steps.n += 1
      const x = order.get(a)
      const y = order.get(b)
      if (!x || !y) return 0
      return Number(x.last) - Number(y.last) || y.bytes - x.bytes || x.at - y.at
    })

  // What a folder and all beneath it show as, labelled from the group `from`.
  const shown = (from: Node | null, start: Node, depth: number): SurveyItem[] => {
    const n = settle(start)
    // A group never holds just one thing, and below the deepest the folders are listed flat.
    if (n.count === 1 || depth >= DEEPEST) return all(n).map((o) => row(o, from))
    const inside = all(n)
    const group: GroupRow = {
      kind: 'group',
      folder: n.path,
      key: n.key,
      label: below(n.parts, from, n.path),
      items: holds(n, depth + 1),
      hints: inside.map((o) => o.hint),
      totals: totalsOf(n),
      notPhotos: n.notPhotos,
    }
    groups.set(group, n)
    order.set(group, { last: inside.every((o) => !o.hint.likelyPhotos), bytes: group.totals.bytes, at: n.first })
    return [group]
  }
  // What is inside a group: its own suggestion first, then what is beneath.
  const holds = (n: Node, depth: number): SurveyItem[] => {
    const own = n.own.map((o) => row(o, n))
    const beneath = sorted([...n.children.values()].flatMap((c) => shown(n, c, depth)))
    return [...own, ...beneath]
  }

  // A drive or share's root is never a group: everything on it is there.
  const unGrouped = (n: Node) => [...n.own.map((o) => row(o, null)), ...[...n.children.values()].flatMap((c) => shown(null, c, 0))]
  let top = sorted([...roots.values()].flatMap(unGrouped))
  // Nor is the one place surveyed, when everything is in it: one group
  // would only say again what the heading says.
  const only = top.length === 1 ? top[0] : undefined
  if (only?.kind === 'group') {
    const n = groups.get(only)
    if (n) top = sorted(unGrouped(n))
  }
  return top
}

/** Whether a group starts open: not when it is long, or holds only probable icons. */
export const startsOpen = (group: GroupRow) =>
  group.totals.folders <= OPEN_UP_TO && group.notPhotos < group.totals.folders

/**
 * The tick a group shows for the folders in it: all ticked, none, some
 * (shown as mixed), or `off` when none of them can be ticked. A folder
 * that cannot be ticked (already a source, or left out) is not counted.
 */
export function tickOf(
  folders: readonly string[],
  chosen: (folder: string) => boolean,
  can: (folder: string) => boolean,
): 'all' | 'none' | 'mixed' | 'off' {
  const open = folders.filter(can)
  if (open.length === 0) return 'off'
  const ticked = open.filter(chosen).length
  return ticked === 0 ? 'none' : ticked === open.length ? 'all' : 'mixed'
}
