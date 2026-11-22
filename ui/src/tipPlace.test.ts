import assert from 'node:assert/strict'
import { test } from 'node:test'
import { tipPlace } from './tipPlace.ts'

const wide = { width: 1440, height: 900 }
const size = { width: 300, height: 100 }

test('an explanation opens under its button, centred on it', () => {
  const button = { left: 600, right: 624, top: 400, bottom: 424 }
  assert.deepEqual(tipPlace(button, size, wide), { left: 462, top: 432, above: false })
})

test('near the bottom of the window it opens above its button', () => {
  const button = { left: 600, right: 624, top: 820, bottom: 844 }
  assert.deepEqual(tipPlace(button, size, wide), { left: 462, top: 712, above: true })
})

test('it never reaches past the gutter at either side', () => {
  assert.equal(tipPlace({ left: 4, right: 28, top: 100, bottom: 124 }, size, wide).left, 16)
  assert.equal(tipPlace({ left: 1420, right: 1440, top: 100, bottom: 124 }, size, wide).left, 1440 - 16 - 300)
})

test('in a window 400 pixels wide it keeps inside the gutters', () => {
  const narrow = { width: 400, height: 800 }
  const placed = tipPlace({ left: 360, right: 384, top: 100, bottom: 124 }, { width: 368, height: 120 }, narrow)
  assert.equal(placed.left, 16)
  assert.ok(placed.left + 368 <= 400 - 16)
})

test('with no room above or below, it stays inside the window from the top', () => {
  const short = { width: 800, height: 200 }
  const placed = tipPlace({ left: 400, right: 424, top: 80, bottom: 104 }, { width: 300, height: 180 }, short)
  assert.equal(placed.above, false)
  assert.equal(placed.top, 16)
})
