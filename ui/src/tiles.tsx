// A grid of tiles, in sections, that only draws what is near the screen.
//
// A folder of ten thousand photos opens at once: the height of every
// section is known from its count before anything is fetched, so the
// scroll bar is right from the start, and only the rows within a screen
// above and below what is shown exist in the page. Each section's items
// are fetched a page at a time as they come near; until then their tiles
// shimmer. A tile that leaves the screen goes from the page, and whatever
// it was waiting for is dropped (see `Picture`'s `drop`).
//
// It uses the grid's own `.tiles` and `.tile` look, so it looks exactly
// like the grids it replaces.

import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react'
import type { ReactNode } from 'react'
import { scrollerOf } from './scroller'

/** One section of the grid: a heading and the items under it. */
export type TileSection<T> = {
  key: string
  /** Its heading; a grid of one section may have none. */
  heading?: ReactNode
  /** How many it holds, as far as is known before it is listed. */
  count: number
  /** A page of its items. `total` corrects `count` once it is known. */
  load: (from: number, count: number) => Promise<{ total: number; items: T[] }>
}

/** The smallest a tile may be, and the space between tiles, as `.tiles` has them. */
const SMALLEST = 170
const GAP = 12
/** The height of a section's heading, as `.tiles-heading` has it. */
const HEADING = 56
/** The height of one row when the items are shown as a list, as `.tiles.list` has it. */
const LIST_ROW = 44
/** How many items are fetched at a time. */
const PAGE = 100

export function VirtualTiles<T>(props: {
  sections: TileSection<T>[]
  /** A tile for an item, or for one not fetched yet (`null`). */
  tile: (item: T | null, at: number, section: TileSection<T>) => ReactNode
  /** A label for the whole grid, for a screen reader. */
  label: string
  /** Shown as a list, one item to a row, rather than as tiles. */
  list?: boolean
}) {
  const { sections } = props
  const box = useRef<HTMLDivElement>(null)
  const [width, setWidth] = useState(0)
  // What is near the screen, in pixels from the top of the grid.
  const [near, setNear] = useState({ from: 0, to: 2000 })
  // What has been fetched: each section's pages, and its true total. The
  // pages asked for are noted apart, so each is asked for once.
  const [fetched, setFetched] = useState<Record<string, { total: number; pages: Record<number, T[]> }>>({})
  const asked = useRef(new Set<string>())

  useLayoutEffect(() => {
    const el = box.current
    if (!el) return
    const measure = () => setWidth(el.clientWidth)
    measure()
    const watch = new ResizeObserver(measure)
    watch.observe(el)
    return () => watch.disconnect()
  }, [])

  const look = useCallback(() => {
    const el = box.current
    if (!el) return
    const scroller = scrollerOf(el)
    const view = scroller ? scroller.getBoundingClientRect() : { top: 0, height: window.innerHeight }
    const top = el.getBoundingClientRect().top - view.top
    const height = view.height
    // One screen above and one below what is shown.
    const from = Math.max(0, -top - height)
    const to = -top + 2 * height
    setNear((now) => (Math.abs(now.from - from) < 40 && Math.abs(now.to - to) < 40 ? now : { from, to }))
  }, [])

  useEffect(() => {
    // Which box scrolls changes with the window's width, so every scroll
    // is heard, whichever box it is in.
    look()
    document.addEventListener('scroll', look, { capture: true, passive: true })
    window.addEventListener('resize', look)
    return () => {
      document.removeEventListener('scroll', look, { capture: true })
      window.removeEventListener('resize', look)
    }
  }, [look])

  const columns = props.list ? 1 : Math.max(1, Math.floor((width + GAP) / (SMALLEST + GAP)))
  const tileSize = width > 0 ? (width - GAP * (columns - 1)) / columns : SMALLEST
  const row = props.list ? LIST_ROW : tileSize + GAP

  // Where each section sits, from the counts alone.
  const placed = sections.reduce<{ section: TileSection<T>; count: number; top: number; heading: number; rows: number }[]>(
    (all, section) => {
      const before = all[all.length - 1]
      const top = before ? before.top + before.heading + before.rows * row + GAP : 0
      const count = fetched[section.key]?.total ?? section.count
      return [...all, { section, count, top, heading: section.heading ? HEADING : 0, rows: Math.ceil(count / columns) }]
    },
    [],
  )
  const last = placed[placed.length - 1]
  const height = last ? last.top + last.heading + last.rows * row : 0

  // Fetch the pages that have come near, once each.
  useEffect(() => {
    for (const { section, count, top: sectionTop, heading, rows } of placed) {
      const first = Math.floor((near.from - sectionTop - heading) / row)
      const last = Math.ceil((near.to - sectionTop - heading) / row)
      if (last < 0 || first >= rows) continue
      const from = Math.max(0, first) * columns
      const to = Math.min(count, (Math.min(rows, last) + 1) * columns)
      for (let page = Math.floor(from / PAGE); page * PAGE < to; page++) {
        const name = `${section.key}\n${page}`
        if (asked.current.has(name)) continue
        asked.current.add(name)
        section
          .load(page * PAGE, PAGE)
          .then(({ total, items }) =>
            setFetched((now) => ({
              ...now,
              [section.key]: { total, pages: { ...now[section.key]?.pages, [page]: items } },
            })),
          )
          .catch(() => asked.current.delete(name))
      }
    }
  })

  const item = (key: string, at: number): T | null => fetched[key]?.pages[Math.floor(at / PAGE)]?.[at % PAGE] ?? null

  return (
    <div className="virtual-tiles" ref={box} role="list" aria-label={props.label} style={{ height }}>
      {placed.map(({ section, count, top: sectionTop, heading, rows }) => {
        const first = Math.max(0, Math.floor((near.from - sectionTop - heading) / row))
        const last = Math.min(rows - 1, Math.ceil((near.to - sectionTop - heading) / row))
        const shown: number[] = []
        for (let at = first * columns; at <= Math.min(count - 1, (last + 1) * columns - 1); at++) shown.push(at)
        return (
          <section
            key={section.key}
            className="tiles-section"
            style={{ top: sectionTop, height: heading + rows * row }}
            role="listitem"
          >
            {section.heading && <div className="tiles-heading">{section.heading}</div>}
            {shown.length > 0 && (
              <div
                className={props.list ? 'tiles list' : 'tiles'}
                role="list"
                style={{ top: heading + first * row, gridTemplateColumns: `repeat(${columns}, minmax(0, 1fr))` }}
              >
                {shown.map((at) => (
                  <div key={at} role="listitem">
                    {props.tile(item(section.key, at), at, section)}
                  </div>
                ))}
              </div>
            )}
          </section>
        )
      })}
    </div>
  )
}
