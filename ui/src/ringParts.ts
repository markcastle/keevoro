// The parts of the ring above the map of where the space goes: photos,
// videos, other files chosen to carry along, and everything else, each with
// its figures exact and its arc on the circle. Pure, so Node tests it alone.
//
// The figures are the answer's own (`photoBytes`, `videoBytes`,
// `carriedBytes`, `otherBytes`); nothing more is asked of the API.

import type { MapKind, MapNode } from './backend'

export type RingPart = {
  kind: MapKind
  bytes: number
  /** Its share of the whole, from 0 to 1, exact. */
  share: number
  /** Where its arc starts and ends on the circle, in degrees from the top. */
  from: number
  to: number
}

/** The least arc a part over nothing is given, so it can be seen. */
export const LEAST_ARC = 360 * 0.015

/**
 * The ring's parts, in the key's order. A part of nothing is left out; other
 * files to carry along only appear when there are some. A part over nothing
 * gets at least 1.5% of the circle, taken from the others in proportion, while
 * its figures stay exact. The arcs always add up to 360 degrees.
 */
export function ringParts(top: MapNode): RingPart[] {
  const all: [MapKind, number][] = [
    ['photo', top.photoBytes],
    ['video', top.videoBytes],
    ['music', top.musicBytes ?? 0],
    ['carried', top.carriedBytes ?? 0],
    ['other', top.otherBytes],
  ]
  const parts = all.filter(([, bytes]) => bytes > 0)
  const total = parts.reduce((n, [, bytes]) => n + bytes, 0)
  if (total <= 0) return []
  // Small parts get the least arc; the rest share what is left in proportion.
  const small = parts.filter(([, bytes]) => (bytes / total) * 360 < LEAST_ARC)
  const bigTotal = parts.filter((p) => !small.includes(p)).reduce((n, [, bytes]) => n + bytes, 0)
  const left = 360 - small.length * LEAST_ARC
  let at = 0
  return parts.map(([kind, bytes], i) => {
    const arc = small.includes(parts[i]) ? LEAST_ARC : bigTotal > 0 ? (bytes / bigTotal) * left : 0
    const from = at
    at = i === parts.length - 1 ? 360 : at + arc
    return { kind, bytes, share: bytes / total, from, to: at }
  })
}
