// The map growing while a survey walks: the boxes at the top of the place,
// each folder with everything found in it so far, sent with the survey's
// progress at most twice a second. A person sees the shape of the place
// straight away instead of a spinner. Each new picture eases from the last,
// unless reduced motion is asked for, when it simply appears.
//
// It is drawn as the map is (mapCanvas.tsx), so its colours and patterns
// mean the same, and it is an image to a screen reader, with a summary; the
// counts beside it say the rest.

import { useEffect, useLayoutEffect, useRef, useState } from 'react'
import type { MapNode, TopBox } from './backend'
import { share, size } from './format'
import { draw, lookOf, motionWelcome } from './mapCanvas'
import { easeOut, layout } from './treemap'

/** How long one picture takes to ease into the next. */
const GROWING_MS = 400

const READ = { size: (n: MapNode) => n.bytes, children: (n: MapNode) => n.children }

/** The place so far, as a box of the map holding the top boxes. */
export function placeSoFar(name: string, top: TopBox[]): MapNode {
  const sum = (pick: (b: TopBox) => number) => top.reduce((n, b) => n + pick(b), 0)
  return {
    name,
    node: 'folder',
    bytes: sum((b) => b.bytes),
    photos: sum((b) => b.photos),
    videos: sum((b) => b.videos),
    others: sum((b) => b.others),
    photoBytes: sum((b) => b.photoBytes),
    videoBytes: sum((b) => b.videoBytes),
    otherBytes: sum((b) => b.otherBytes),
    wouldBackUp: 0,
    children: top.map((b) => ({ ...b, wouldBackUp: 0 })),
  }
}

/** Each box's size part of the way from one picture to the next, by name. */
function partWay(from: MapNode, to: MapNode, t: number): MapNode {
  const before = new Map((from.children ?? []).map((c) => [`${c.node}|${c.name}`, c.bytes]))
  const children = (to.children ?? []).map((c) => {
    const was = before.get(`${c.node}|${c.name}`) ?? 0
    return { ...c, bytes: was + (c.bytes - was) * t }
  })
  return { ...to, bytes: children.reduce((n, c) => n + c.bytes, 0), children }
}

export function LiveMap(props: { name: string; top: TopBox[] | undefined }) {
  const wrap = useRef<HTMLDivElement>(null)
  const canvas = useRef<HTMLCanvasElement>(null)
  const [area, setArea] = useState({ w: 0, h: 0 })
  const shown = useRef<MapNode | null>(null)
  const top = props.top
  const present = top !== undefined && top.length > 0

  useLayoutEffect(() => {
    const element = wrap.current
    if (!element) return
    const measure = () => setArea({ w: element.clientWidth, h: element.clientHeight })
    measure()
    const watch = new ResizeObserver(measure)
    watch.observe(element)
    return () => watch.disconnect()
  }, [present])

  useEffect(() => {
    const ctx = canvas.current?.getContext('2d')
    if (!ctx || !top || area.w === 0) return
    const ratio = window.devicePixelRatio || 1
    const [w, h] = [Math.round(area.w * ratio), Math.round(area.h * ratio)]
    if (canvas.current && (canvas.current.width !== w || canvas.current.height !== h)) {
      canvas.current.width = w
      canvas.current.height = h
    }
    ctx.setTransform(ratio, 0, 0, ratio, 0, 0)
    const look = lookOf(ctx)
    const next = placeSoFar(props.name, top)
    const from = shown.current
    const paint = (node: MapNode) => {
      ctx.clearRect(0, 0, area.w, area.h)
      draw(ctx, layout(node, { x: 0, y: 0, w: area.w, h: area.h }, READ, 1), look)
    }
    if (!from || !motionWelcome()) {
      shown.current = next
      paint(next)
      return
    }
    const element = wrap.current
    element?.setAttribute('data-easing', 'true')
    let frame = 0
    const started = performance.now()
    const step = (now: number) => {
      const t = Math.min(1, (now - started) / GROWING_MS)
      const at = partWay(from, next, easeOut(t))
      shown.current = at
      paint(at)
      if (t < 1) frame = requestAnimationFrame(step)
      else element?.removeAttribute('data-easing')
    }
    frame = requestAnimationFrame(step)
    return () => {
      cancelAnimationFrame(frame)
      element?.removeAttribute('data-easing')
    }
  }, [top, area, props.name])

  if (!top || !present) return null
  const place = placeSoFar(props.name, top)
  return (
    <div className="live-map" ref={wrap} data-boxes={top.length}>
      <canvas
        ref={canvas}
        role="img"
        aria-label={`Map of ${props.name} so far: ${size(place.bytes)}. Photos ${share(place.photoBytes, place.bytes)}, videos ${share(
          place.videoBytes,
          place.bytes,
        )}, everything else ${share(place.otherBytes, place.bytes)}.`}
      />
    </div>
  )
}
