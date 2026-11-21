// The ring above the map, tested by Node on its own: `npm run ui:unit`.

import assert from 'node:assert/strict'
import { test } from 'node:test'
import { LEAST_ARC, ringParts } from './ringParts.ts'
import type { MapNode } from './backend'

const top = (photo: number, video: number, other: number, carried?: number) =>
  ({
    name: 'Family hard drive',
    node: 'folder',
    bytes: photo + video + other + (carried ?? 0),
    photos: 1,
    videos: 1,
    others: 1,
    photoBytes: photo,
    videoBytes: video,
    otherBytes: other,
    ...(carried === undefined ? {} : { carriedBytes: carried, carried: 1 }),
    wouldBackUp: 0,
  }) as MapNode

test('the arcs go once round the circle, end to end, in the key’s order', () => {
  const parts = ringParts(top(62, 19, 17, 2))
  assert.deepEqual(
    parts.map((p) => p.kind),
    ['photo', 'video', 'carried', 'other'],
  )
  assert.equal(parts[0].from, 0)
  assert.equal(parts[parts.length - 1].to, 360)
  for (let i = 1; i < parts.length; i++) assert.equal(parts[i].from, parts[i - 1].to)
  assert.ok(Math.abs(parts[0].to - 0.62 * 360) < 1e-9)
})

test('a tiny part is given enough of the circle to be seen, and its figures stay exact', () => {
  const parts = ringParts(top(999_000, 0, 1_000))
  const other = parts.find((p) => p.kind === 'other')!
  assert.ok(Math.abs(other.to - other.from - LEAST_ARC) < 1e-9)
  assert.equal(other.share, 0.001)
  assert.equal(other.bytes, 1_000)
  assert.equal(parts[parts.length - 1].to, 360)
})

test('a part of nothing is left out, and other files chosen only appear when there are some', () => {
  assert.deepEqual(
    ringParts(top(10, 0, 5)).map((p) => p.kind),
    ['photo', 'other'],
  )
  assert.deepEqual(
    ringParts(top(10, 5, 5, 0)).map((p) => p.kind),
    ['photo', 'video', 'other'],
  )
  assert.deepEqual(ringParts(top(0, 0, 0)), [])
})
