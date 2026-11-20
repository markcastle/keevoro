// The map itself: a box for each folder and file, its area in proportion
// to the room it takes, drawn on a canvas in the style of WinDirStat.
//
// Two canvases are stacked. The one underneath holds the boxes and is
// drawn again only when the folder, the size of the window or the theme
// changes; the one on top holds only the outline of the box pointed at
// and the box chosen, so moving the pointer never redraws thousands of
// boxes. Both are drawn at the screen's own resolution.
//
// Colours come from the design tokens at the moment of drawing, in light
// and dark, and each kind has a pattern of its own (photos plain, videos
// striped, what Keevoro never looks through dotted, what is left out
// cross-hatched, what is only online with a dashed edge), so colour is
// never the only sign. The list beside it says everything in words, so
// the map is an image to a screen reader, with a summary.
//
// Going into a folder, its box grows to fill the map; coming back out, the
// folder shrinks back into its box. With reduced motion asked for, the new
// map simply appears.

import { useEffect, useLayoutEffect, useRef, useState } from 'react'
import type { MapNode } from './backend'
import { share, size } from './format'
import { called, holds, mainKind, notesOn } from './mapWords'
import { between, easeOut, hit, layout } from './treemap'
import type { Laid, Rect } from './treemap'

const READ = { size: (n: MapNode) => n.bytes, children: (n: MapNode) => n.children }
/** How long going in or out of a folder takes, when motion is welcome. */
const EASING_MS = 220

/** The colours and patterns of one theme, read from the design tokens. */
type Look = {
  photo: string
  photoEdge: string
  video: string
  videoEdge: string
  other: string
  otherEdge: string
  ground: string
  strip: string
  ink: string
  muted: string
  onSun: string
  onPine: string
  focus: string
  font: string
  stripes: CanvasPattern | null
  dots: CanvasPattern | null
  hatch: CanvasPattern | null
}

/** A colour from the design tokens, as written there. */
const token = (style: CSSStyleDeclaration, name: string) => style.getPropertyValue(name).trim()

/** Two `#rrggbb` colours mixed, `part` of the first. */
function mix(a: string, b: string, part: number) {
  const rgb = (c: string) => [1, 3, 5].map((i) => parseInt(c.slice(i, i + 2), 16))
  if (!/^#[0-9a-f]{6}$/i.test(a) || !/^#[0-9a-f]{6}$/i.test(b)) return a
  const [x, y] = [rgb(a), rgb(b)]
  return `#${x.map((v, i) => Math.round(v * part + y[i] * (1 - part)).toString(16).padStart(2, '0')).join('')}`
}

/** A small tile repeated as a pattern. */
function pattern(ctx: CanvasRenderingContext2D, draw: (tile: CanvasRenderingContext2D) => void) {
  const tile = document.createElement('canvas')
  tile.width = 8
  tile.height = 8
  const t = tile.getContext('2d')
  if (!t) return null
  draw(t)
  return ctx.createPattern(tile, 'repeat')
}

function lookOf(ctx: CanvasRenderingContext2D): Look {
  const style = getComputedStyle(document.documentElement)
  const sunken = token(style, '--surface-sunken')
  const surface = token(style, '--surface')
  const ink = token(style, '--ink')
  const lineStrong = token(style, '--line-strong')
  const other = mix(lineStrong, sunken, 0.35)
  return {
    photo: token(style, '--sun'),
    photoEdge: token(style, '--sun-ink'),
    video: token(style, '--pine'),
    videoEdge: token(style, '--hill'),
    other,
    otherEdge: token(style, '--line-strong'),
    ground: surface,
    strip: mix(lineStrong, sunken, 0.18),
    ink,
    muted: token(style, '--ink-muted'),
    onSun: token(style, '--on-sun'),
    onPine: token(style, '--on-primary'),
    focus: token(style, '--focus-ring'),
    font: getComputedStyle(document.body).fontFamily,
    // Fine diagonal lines over a video's colour.
    stripes: pattern(ctx, (t) => {
      t.strokeStyle = mix(surface, token(style, '--pine'), 0.45)
      t.lineWidth = 1.5
      t.beginPath()
      t.moveTo(-2, 10)
      t.lineTo(10, -2)
      t.stroke()
    }),
    // Sparse dots: a folder Keevoro never looks through.
    dots: pattern(ctx, (t) => {
      t.fillStyle = lineStrong
      t.beginPath()
      t.arc(4, 4, 1.2, 0, Math.PI * 2)
      t.fill()
    }),
    // A cross-hatch: left out of this backup.
    hatch: pattern(ctx, (t) => {
      t.strokeStyle = ink
      t.globalAlpha = 0.5
      t.lineWidth = 1
      t.beginPath()
      t.moveTo(0, 0)
      t.lineTo(8, 8)
      t.moveTo(8, 0)
      t.lineTo(0, 8)
      t.stroke()
    }),
  }
}

/** Words cut to fit a width, with an ellipsis. */
function fit(ctx: CanvasRenderingContext2D, words: string, width: number) {
  if (ctx.measureText(words).width <= width) return words
  let cut = words
  while (cut.length > 1 && ctx.measureText(`${cut}…`).width > width) cut = cut.slice(0, -1)
  return cut.length > 1 ? `${cut}…` : ''
}

/** Draws every box of a layout. */
function draw(ctx: CanvasRenderingContext2D, top: Laid<MapNode>, look: Look) {
  ctx.fillStyle = look.ground
  ctx.fillRect(top.x, top.y, top.w, top.h)
  const box = (b: Laid<MapNode>, passedBy: boolean, leftOut: boolean) => {
    const n = b.item
    const never = passedBy || Boolean(n.skipped)
    const out = !never && (leftOut || Boolean(n.leftOut))
    const kind = never ? 'other' : mainKind(n)
    const fill = look[kind]
    const edge = never ? look.otherEdge : kind === 'photo' ? look.photoEdge : kind === 'video' ? look.videoEdge : look.otherEdge
    const opened = Boolean(b.inner && b.children.length > 0)
    ctx.fillStyle = opened ? mix(fill, look.ground, 0.35) : fill
    ctx.fillRect(b.x, b.y, b.w, b.h)
    if (!opened && kind === 'video' && look.stripes) {
      ctx.fillStyle = look.stripes
      ctx.fillRect(b.x, b.y, b.w, b.h)
    }
    if (never && look.dots) {
      ctx.fillStyle = look.dots
      ctx.fillRect(b.x, b.y, b.w, b.h)
    }
    if (opened) {
      ctx.fillStyle = look.strip
      ctx.fillRect(b.x, b.y, b.w, Math.min(b.h, (b.inner?.y ?? b.y) - b.y))
      b.children.forEach((c) => box(c, never, out))
    }
    if (out && look.hatch) {
      ctx.fillStyle = look.hatch
      ctx.fillRect(b.x, b.y, b.w, b.h)
    }
    ctx.strokeStyle = edge
    ctx.lineWidth = 1
    ctx.setLineDash(n.onlineOnlyBytes ? [4, 3] : [])
    ctx.strokeRect(b.x + 0.5, b.y + 0.5, Math.max(0, b.w - 1), Math.max(0, b.h - 1))
    ctx.setLineDash([])
    label(b, opened, kind)
  }
  const label = (b: Laid<MapNode>, opened: boolean, kind: 'photo' | 'video' | 'other') => {
    if (b.w < 36 || b.h < 14) return
    ctx.textBaseline = 'top'
    ctx.fillStyle = opened || kind === 'other' ? look.ink : kind === 'photo' ? look.onSun : look.onPine
    ctx.font = `${opened ? 600 : 400} 12px ${look.font}`
    const name = fit(ctx, called(b.item), b.w - 8)
    if (!name) return
    ctx.fillText(name, b.x + 4, b.y + 3)
    if (!opened && b.h >= 32) {
      ctx.font = `400 11px ${look.font}`
      ctx.fillText(fit(ctx, size(b.item.bytes), b.w - 8), b.x + 4, b.y + 18)
    }
  }
  top.children.forEach((c) => box(c, Boolean(top.item.skipped), Boolean(top.item.leftOut) && !top.item.skipped))
}

/** The box with this key in a layout, if it is there. */
function boxOf(top: Laid<MapNode>, keys: Map<MapNode, string>, key: string | null | undefined): Laid<MapNode> | null {
  if (!key) return null
  const visit = (b: Laid<MapNode>): Laid<MapNode> | null => {
    if (keys.get(b.item) === key) return b
    for (const c of b.children) {
      const found = visit(c)
      if (found) return found
    }
    return null
  }
  return visit(top)
}

const motionWelcome = () => !window.matchMedia('(prefers-reduced-motion: reduce)').matches

/** What the map is, in words, for a screen reader: the list beside it says the rest. */
function summary(top: MapNode) {
  return `Map of ${top.name}: ${size(top.bytes)}. Photos ${share(top.photoBytes, top.bytes)}, videos ${share(
    top.videoBytes,
    top.bytes,
  )}, everything else ${share(top.otherBytes, top.bytes)}. The list has every folder.`
}

export function MapCanvas(props: {
  top: MapNode
  keys: Map<MapNode, string>
  /** How many levels of folders are drawn inside one another. */
  depth: number
  /** The key of the box chosen, outlined. */
  chosen: string | null
  /** The key of the row pointed at in the list, outlined. */
  lit: string | null
  onChoose: (key: string) => void
  onGo: (folder: string) => void
  /** Hears the row the pointer is over on the map, to light it in the list. */
  onPoint: (key: string | null) => void
}) {
  const { top, keys } = props
  const wrap = useRef<HTMLDivElement>(null)
  const base = useRef<HTMLCanvasElement>(null)
  const over = useRef<HTMLCanvasElement>(null)
  const tip = useRef<HTMLDivElement>(null)
  const [area, setArea] = useState({ w: 0, h: 0 })
  const [theme, setTheme] = useState(0)
  // What is drawn now, to ease from when the folder changes.
  const drawn = useRef<{ top: MapNode; keys: Map<MapNode, string>; laid: Laid<MapNode> } | null>(null)
  const pointed = useRef<string | null>(null)
  const row = useRef<string | null>(null)
  // The latest of what the highlight is drawn from, for the pointer's sake.
  const marks = useRef({ chosen: props.chosen, lit: props.lit })
  useEffect(() => {
    marks.current = { chosen: props.chosen, lit: props.lit }
  })

  useLayoutEffect(() => {
    const element = wrap.current
    if (!element) return
    const measure = () => setArea({ w: element.clientWidth, h: element.clientHeight })
    measure()
    const watch = new ResizeObserver(measure)
    watch.observe(element)
    return () => watch.disconnect()
  }, [])

  // Drawn again in the other theme, whether the computer's or Keevoro's own changes.
  useEffect(() => {
    const dark = window.matchMedia('(prefers-color-scheme: dark)')
    const again = () => setTheme((n) => n + 1)
    dark.addEventListener('change', again)
    const watch = new MutationObserver(again)
    watch.observe(document.documentElement, { attributes: true, attributeFilter: ['data-theme'] })
    return () => {
      dark.removeEventListener('change', again)
      watch.disconnect()
    }
  }, [])

  /** A canvas made ready to draw on in CSS pixels at the screen's resolution. */
  const ready = (canvas: HTMLCanvasElement | null) => {
    if (!canvas || area.w === 0) return null
    const ratio = window.devicePixelRatio || 1
    const [w, h] = [Math.round(area.w * ratio), Math.round(area.h * ratio)]
    if (canvas.width !== w || canvas.height !== h) {
      canvas.width = w
      canvas.height = h
    }
    const ctx = canvas.getContext('2d')
    ctx?.setTransform(ratio, 0, 0, ratio, 0, 0)
    return ctx
  }

  const drawOver = () => {
    const ctx = ready(over.current)
    const now = drawn.current
    if (!ctx || !now) return
    ctx.clearRect(0, 0, area.w, area.h)
    const look = lookOf(ctx)
    const outline = (key: string | null, width: number, dashed: boolean) => {
      const b = boxOf(now.laid, now.keys, key)
      if (!b || b.depth === 0) return
      ctx.strokeStyle = look.focus
      ctx.lineWidth = width
      ctx.setLineDash(dashed ? [5, 3] : [])
      ctx.strokeRect(b.x + width / 2, b.y + width / 2, Math.max(0, b.w - width), Math.max(0, b.h - width))
    }
    outline(marks.current.lit, 2, true)
    outline(pointed.current, 2, true)
    outline(marks.current.chosen, 3, false)
  }

  // The boxes, eased from the folder before when it was gone into or out of.
  useEffect(() => {
    const ctx = ready(base.current)
    if (!ctx) return
    const full: Rect = { x: 0, y: 0, w: area.w, h: area.h }
    const laid = layout(top, full, READ, props.depth)
    const before = drawn.current
    drawn.current = { top, keys, laid }
    const look = lookOf(ctx)
    const at = (from: Laid<MapNode>, rect: Rect, alpha: number) => {
      ctx.save()
      ctx.globalAlpha = alpha
      ctx.beginPath()
      ctx.rect(rect.x, rect.y, rect.w, rect.h)
      ctx.clip()
      ctx.translate(rect.x, rect.y)
      ctx.scale(rect.w / full.w || 1, rect.h / full.h || 1)
      draw(ctx, from, look)
      ctx.restore()
    }
    const still = () => {
      ctx.clearRect(0, 0, full.w, full.h)
      draw(ctx, laid, look)
      drawOver()
    }
    const into = before && before.top.path !== top.path ? boxOf(before.laid, before.keys, top.path) : null
    const outOf = before && before.top.path !== top.path ? boxOf(laid, keys, before.top.path) : null
    if (!before || (!into && !outOf) || !motionWelcome()) {
      still()
      return
    }
    const element = wrap.current
    element?.setAttribute('data-easing', 'true')
    let frame = 0
    const started = performance.now()
    const step = (now: number) => {
      const t = Math.min(1, (now - started) / EASING_MS)
      const e = easeOut(t)
      ctx.clearRect(0, 0, full.w, full.h)
      if (into) {
        at(before.laid, full, 1 - e)
        at(laid, between(into, full, e), e)
      } else if (outOf) {
        at(laid, full, e)
        at(before.laid, between(full, outOf, e), 1 - e)
      }
      if (t < 1) frame = requestAnimationFrame(step)
      else {
        element?.removeAttribute('data-easing')
        still()
      }
    }
    frame = requestAnimationFrame(step)
    return () => {
      cancelAnimationFrame(frame)
      element?.removeAttribute('data-easing')
    }
    // The highlight is drawn by its own effect; this one is the boxes.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [top, keys, props.depth, area, theme])

  // eslint-disable-next-line react-hooks/exhaustive-deps
  useEffect(drawOver, [props.chosen, props.lit, area, theme])

  /** The boxes under the pointer, outermost first, and where it is. */
  const under = (event: React.PointerEvent | React.MouseEvent) => {
    const now = drawn.current
    const rect = over.current?.getBoundingClientRect()
    if (!now || !rect) return { path: [] as Laid<MapNode>[], x: 0, y: 0 }
    const [x, y] = [event.clientX - rect.left, event.clientY - rect.top]
    return { path: hit(now.laid, x, y), x, y }
  }

  const point = (event: React.PointerEvent) => {
    const { path, x, y } = under(event)
    const deepest = path.length > 1 ? path[path.length - 1] : null
    const key = deepest ? (keys.get(deepest.item) ?? null) : null
    const tipNow = tip.current
    if (tipNow) {
      if (deepest) {
        const node = deepest.item
        const notes = notesOn(node)
        tipNow.textContent = `${called(node)}: ${size(node.bytes)}${node.node === 'files' ? '' : `, ${holds(node)}`}${
          notes.length ? `. ${notes.join('. ')}` : ''
        }`
        tipNow.hidden = false
        // Beside the pointer, kept inside the map.
        const left = Math.min(x + 14, area.w - tipNow.offsetWidth - 4)
        const topAt = y + 18 + tipNow.offsetHeight > area.h ? y - tipNow.offsetHeight - 10 : y + 18
        tipNow.style.transform = `translate(${Math.max(4, left)}px, ${Math.max(4, topAt)}px)`
      } else tipNow.hidden = true
    }
    if (key !== pointed.current) {
      pointed.current = key
      wrap.current?.setAttribute('data-pointed', key ?? '')
      drawOver()
    }
    const rowKey = path.length > 1 ? (keys.get(path[1].item) ?? null) : null
    if (rowKey !== row.current) {
      row.current = rowKey
      props.onPoint(rowKey)
    }
  }

  const leave = () => {
    if (tip.current) tip.current.hidden = true
    pointed.current = null
    row.current = null
    wrap.current?.setAttribute('data-pointed', '')
    props.onPoint(null)
    drawOver()
  }

  // Click chooses a box; clicking a chosen folder again, or a double click, goes into it.
  const click = (event: React.MouseEvent) => {
    const { path } = under(event)
    const deepest = path.length > 1 ? path[path.length - 1] : null
    if (!deepest) return
    const key = keys.get(deepest.item) ?? null
    if (key && key === props.chosen && deepest.item.node === 'folder' && deepest.item.path) props.onGo(deepest.item.path)
    else if (key) props.onChoose(key)
  }
  const twice = (event: React.MouseEvent) => {
    const folder = under(event)
      .path.slice(1)
      .reverse()
      .find((b) => b.item.node === 'folder' && b.item.path)
    if (folder?.item.path) props.onGo(folder.item.path)
  }

  return (
    <div
      className="map-canvas"
      ref={wrap}
      data-selected={props.chosen ?? ''}
      data-lit={props.lit ?? ''}
      data-pointed=""
    >
      <canvas ref={base} role="img" aria-label={summary(top)} />
      <canvas
        ref={over}
        className="over"
        aria-hidden
        onPointerMove={point}
        onPointerLeave={leave}
        onClick={click}
        onDoubleClick={twice}
      />
      <div className="map-tip" ref={tip} aria-hidden hidden />
    </div>
  )
}

/** What each pattern on the map means, in words, so colour is never the only sign. */
export function MapKey() {
  return (
    <ul className="key" aria-label="Key">
      <li>
        <span className="swatch photo" aria-hidden /> Photos
      </li>
      <li>
        <span className="swatch video" aria-hidden /> Videos
      </li>
      <li>
        <span className="swatch other" aria-hidden /> Everything else
      </li>
      <li>
        <span className="swatch skipped" aria-hidden /> Never looked through by Keevoro
      </li>
      <li>
        <span className="swatch left-out" aria-hidden /> Left out of this backup
      </li>
      <li>
        <span className="swatch online" aria-hidden /> Partly in online storage only
      </li>
    </ul>
  )
}
