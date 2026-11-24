// Albums of music and folders of other files on "Choose what to keep",
// shown in the folders they sit in. Ones that share a folder are one row
// for that folder, which opens to show them; a folder is never shown
// holding just one thing, and folders nest no more than three deep. Kept
// apart from the screen so it can be checked without one, and quick enough
// for thousands: each one's path is walked once.

import type { Collection } from './backend'

/** One album or folder of files, labelled by its path below the folder it is shown in. */
export type FolderRow = { kind: 'folder'; collection: Collection; label: string; inGroup: boolean }

/** Albums or folders that sit together, as one row that opens. */
export type FolderGroup = {
  kind: 'group'
  key: string
  /** Its path below the folder it is shown in. */
  label: string
  items: FolderItem[]
  /** Everything inside, at any depth, in the order given. */
  collections: Collection[]
  /** Tracks, or other files, inside. */
  files: number
  bytes: number
}

export type FolderItem = FolderRow | FolderGroup

/** Folders nest no deeper than this on screen; below it they are listed flat. */
const DEEPEST = 3
/** A folder holding more than this starts closed. */
const OPEN_UP_TO = 6
/** What an album that is the folder itself is called inside it. */
export const ITSELF = 'In this folder itself'

type Node = { name: string; key: string; depth: number; own: Collection[]; children: Map<string, Node>; count: number }

const filesIn = (c: Collection) => (c.music ?? 0) + (c.others ?? 0)

export function folderGroups(collections: Collection[]): FolderItem[] {
  const node = (name: string, key: string, depth: number): Node => ({ name, key, depth, own: [], children: new Map(), count: 0 })
  const root = node('', '', 0)
  for (const c of collections) {
    let here = root
    here.count += 1
    for (const name of [...c.parents, c.title]) {
      let next = here.children.get(name)
      if (!next) {
        next = node(name, `${here.key}\u0000${name}`, here.depth + 1)
        here.children.set(name, next)
      }
      next.count += 1
      here = next
    }
    here.own.push(c)
  }

  const all = (n: Node, into: Collection[] = []): Collection[] => {
    into.push(...n.own)
    n.children.forEach((c) => all(c, into))
    return into
  }
  const path = (c: Collection) => [...c.parents, c.title]
  const row = (c: Collection, from: Node | null): FolderRow => ({
    kind: 'folder',
    collection: c,
    label: from === null ? c.title : path(c).slice(from.depth).join(' › ') || ITSELF,
    inGroup: from !== null,
  })
  // What a folder and all beneath it show as, named from the folder `from`.
  const shown = (from: Node | null, start: Node, names: string[], depth: number): FolderItem[] => {
    let n = start
    const said = [...names, n.name]
    // Folders with nothing of their own and one way on are named in one go.
    while (n.own.length === 0 && n.children.size === 1) {
      n = n.children.values().next().value as Node
      said.push(n.name)
    }
    if (n.count === 1 || depth >= DEEPEST) return all(n).map((c) => row(c, from))
    const inside = all(n)
    return [
      {
        kind: 'group',
        key: n.key,
        label: said.join(' › '),
        items: holds(n, depth + 1),
        collections: inside,
        files: inside.reduce((sum, c) => sum + filesIn(c), 0),
        bytes: inside.reduce((sum, c) => sum + (c.bytes ?? 0), 0),
      },
    ]
  }
  const holds = (n: Node, depth: number): FolderItem[] => [
    ...n.own.map((c) => row(c, n)),
    ...[...n.children.values()].flatMap((c) => shown(n, c, [], depth)),
  ]

  // The one place everything is in is not a folder to open: it would only
  // say again what the heading says. So the top is opened up while it is
  // one folder.
  let top: Node = root
  let items = [...top.children.values()].flatMap((c) => shown(null, c, [], 0))
  while (items.length === 1 && items[0].kind === 'group') {
    while (top.own.length === 0 && top.children.size === 1) top = top.children.values().next().value as Node
    const from = top
    items = [...from.own.map((c) => row(c, null)), ...[...from.children.values()].flatMap((c) => shown(null, c, [], 0))]
    if (from.own.length > 0 || from.children.size !== 1) break
  }
  return items
}

/** Whether all, some or none of these are being kept. */
export function keptOf(collections: Collection[]): 'all' | 'some' | 'none' {
  const kept = collections.filter((c) => c.kept).length
  return kept === 0 ? 'none' : kept === collections.length ? 'all' : 'some'
}

/** Whether a folder starts open: not when it is long. */
export const startsOpen = (group: FolderGroup) => group.collections.length <= OPEN_UP_TO
