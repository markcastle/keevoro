// The layout of a map of where the space goes: boxes whose areas are in
// proportion to the room each folder and file takes, as near square as
// they can be, so they are easy to compare and to point at.
//
// It is the squarified layout of Bruls, Huizing and van Wijk: the biggest
// first, laid in rows along the shorter side of what is left, each row
// taking the next box only while that makes its worst box squarer.
//
// Pure, with nothing of the page in it, so it is tested by Node on its own
// (`npm run ui:unit`) and runs again on every resize without asking the
// API anything. A folder big enough is opened up: a strip at its top for
// its name, and its contents laid inside. A box too small to see is not
// drawn on its own; its room stays its folder's.
//
// Folders that are each only a sliver of the folder they are in can be
// gathered into one box, "Smaller folders", as our first outside tester
// asked: below a share that adapts to the room there is, 2% of a big map
// and up to 10% of a small one, so the map stays readable. Files are never
// gathered, nor a single small folder.

export type Rect = { x: number; y: number; w: number; h: number }

/** One box laid out, with the boxes inside it. */
export type Laid<T> = Rect & {
  item: T
  /** How deep it is: 0 for the folder the map is of. */
  depth: number
  /** Where its contents are laid, below its name strip, when it has any. */
  inner?: Rect
  children: Laid<T>[]
}

/** How to read the tree being laid out. */
export type Reading<T> = {
  size: (item: T) => number
  children: (item: T) => readonly T[] | undefined
  /** May it be gathered with others its size, when small? Folders may; files may not. */
  gatherable?: (item: T) => boolean
  /** Makes one box standing for several small ones, each under `share` of the folder `within`. */
  gather?: (small: T[], within: T, share: number) => T
}

export type Spacing = {
  /** The name strip at the top of an opened folder. */
  strip: number
  /** The gap inside an opened folder, around its contents. */
  pad: number
  /** The least width and height of a folder that is opened up. */
  open: { w: number; h: number }
  /** A box with less area than this is not drawn on its own. */
  least: number
  /** The room a box needs for its name and size: what decides how small is gathered. */
  readable: number
}

export const SPACING: Spacing = { strip: 22, pad: 3, open: { w: 48, h: 32 }, least: 12, readable: 96 * 40 }

/** The least and most of a folder below which folders are gathered. */
export const GATHER_LEAST = 0.02
export const GATHER_MOST = 0.1

/**
 * Below what share of a folder its folders are gathered, for a box of this
 * area: what a readable box would take of it, kept between 2% and 10%.
 */
export function gatherShare(area: number, readable = SPACING.readable) {
  if (!(area > 0)) return GATHER_MOST
  return Math.min(GATHER_MOST, Math.max(GATHER_LEAST, readable / area))
}

/**
 * A folder's contents, with those that may be gathered and are each under
 * `share` of it put together into one, after the rest, when there are at
 * least two of them. In the order given otherwise. One pass.
 */
export function gathered<T>(item: T, contents: readonly T[], read: Reading<T>, share: number): readonly T[] {
  if (!read.gather || !read.gatherable) return contents
  const whole = contents.reduce((n, c) => n + Math.max(0, read.size(c)), 0)
  if (whole <= 0) return contents
  const small: T[] = []
  const kept: T[] = []
  for (const c of contents) {
    if (read.gatherable(c) && read.size(c) < share * whole) small.push(c)
    else kept.push(c)
  }
  if (small.length < 2) return contents
  return [...kept, read.gather(small, item, share)]
}

/** How far from square a row of areas laid along `side` is, at worst. */
function worst(row: number[], side: number) {
  let sum = 0
  let most = 0
  let fewest = Infinity
  for (const a of row) {
    sum += a
    most = Math.max(most, a)
    fewest = Math.min(fewest, a)
  }
  const s2 = sum * sum
  const side2 = side * side
  return Math.max((side2 * most) / s2, s2 / (side2 * fewest))
}

/**
 * Lays out `sizes` in `rect`, each box's area in proportion to its size.
 * The sizes should be biggest first; the boxes come back in the same order.
 * A size of nothing gets a box of no area.
 */
export function squarify(sizes: readonly number[], rect: Rect): Rect[] {
  const total = sizes.reduce((n, s) => n + Math.max(0, s), 0)
  const boxes: Rect[] = sizes.map(() => ({ x: rect.x, y: rect.y, w: 0, h: 0 }))
  if (total <= 0 || rect.w <= 0 || rect.h <= 0) return boxes
  const scale = (rect.w * rect.h) / total
  const areas = sizes.map((s) => Math.max(0, s) * scale)
  let { x, y, w, h } = rect
  let i = 0
  while (i < areas.length) {
    if (areas[i] <= 0) {
      boxes[i] = { x, y, w: 0, h: 0 }
      i += 1
      continue
    }
    const side = Math.min(w, h)
    const row = [areas[i]]
    let j = i + 1
    while (j < areas.length && areas[j] > 0 && worst([...row, areas[j]], side) <= worst(row, side)) {
      row.push(areas[j])
      j += 1
    }
    const sum = row.reduce((n, a) => n + a, 0)
    if (w >= h) {
      // A column down the left of what is left.
      const across = h > 0 ? sum / h : 0
      let down = y
      row.forEach((a, k) => {
        const tall = across > 0 ? a / across : 0
        boxes[i + k] = { x, y: down, w: across, h: tall }
        down += tall
      })
      x += across
      w = Math.max(0, w - across)
    } else {
      // A row along the top of what is left.
      const tall = w > 0 ? sum / w : 0
      let along = x
      row.forEach((a, k) => {
        const wide = tall > 0 ? a / tall : 0
        boxes[i + k] = { x: along, y, w: wide, h: tall }
        along += wide
      })
      y += tall
      h = Math.max(0, h - tall)
    }
    i = j
  }
  return boxes
}

/**
 * The biggest first; equal ones in the order they came, so the same tree
 * is always laid out the same way.
 */
function biggestFirst<T>(items: readonly T[], size: (item: T) => number) {
  return items
    .map((item, at) => ({ item, at, size: Math.max(0, size(item)) }))
    .sort((a, b) => b.size - a.size || a.at - b.at)
}

/**
 * Lays out `top` filling `rect`: its contents directly, and inside each of
 * them, while there is room and depth, theirs. `depth` is how many levels
 * of contents to lay out (1 lays out only `top`'s own).
 */
export function layout<T>(top: T, rect: Rect, read: Reading<T>, depth = 3, spacing: Spacing = SPACING): Laid<T> {
  const root: Laid<T> = { ...rect, item: top, depth: 0, inner: rect, children: [] }
  root.children = inside(top, rect, read, 1, depth, spacing)
  return root
}

function inside<T>(item: T, rect: Rect, read: Reading<T>, level: number, depth: number, spacing: Spacing): Laid<T>[] {
  const contents = gathered(item, read.children(item) ?? [], read, gatherShare(rect.w * rect.h, spacing.readable))
  if (level > depth || contents.length === 0) return []
  const order = biggestFirst(contents, read.size)
  const boxes = squarify(
    order.map((o) => o.size),
    rect,
  )
  const laid: Laid<T>[] = []
  order.forEach((o, k) => {
    const box = boxes[k]
    // Too small to see: its room is left to the folder it is in.
    if (box.w * box.h < spacing.least) return
    const one: Laid<T> = { ...box, item: o.item, depth: level, children: [] }
    const opens = (read.children(o.item)?.length ?? 0) > 0 && box.w >= spacing.open.w && box.h >= spacing.open.h
    if (opens && level < depth) {
      one.inner = {
        x: box.x + spacing.pad,
        y: box.y + spacing.strip,
        w: Math.max(0, box.w - 2 * spacing.pad),
        h: Math.max(0, box.h - spacing.strip - spacing.pad),
      }
      one.children = inside(o.item, one.inner, read, level + 1, depth, spacing)
    }
    laid.push(one)
  })
  return laid
}

/** The deepest box at a point, and every box above it, the outermost first. */
export function hit<T>(top: Laid<T>, x: number, y: number): Laid<T>[] {
  const within = (b: Rect) => x >= b.x && x < b.x + b.w && y >= b.y && y < b.y + b.h
  const path: Laid<T>[] = []
  let at: Laid<T> | undefined = within(top) ? top : undefined
  while (at) {
    path.push(at)
    at = at.children.find(within)
  }
  return path
}

/** Every box laid out, outermost first. */
export function everyBox<T>(top: Laid<T>): Laid<T>[] {
  const all: Laid<T>[] = []
  const visit = (b: Laid<T>) => {
    all.push(b)
    b.children.forEach(visit)
  }
  visit(top)
  return all
}

/** A rectangle part of the way from one to another, for an easing between layouts. */
export function between(from: Rect, to: Rect, t: number): Rect {
  return {
    x: from.x + (to.x - from.x) * t,
    y: from.y + (to.y - from.y) * t,
    w: from.w + (to.w - from.w) * t,
    h: from.h + (to.h - from.h) * t,
  }
}

/** Eases out: quick at first, gentle at the end. */
export const easeOut = (t: number) => 1 - (1 - t) ** 3
