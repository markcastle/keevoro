import assert from 'node:assert/strict'
import { test } from 'node:test'
import { menuMove, menuPlace } from './menu.ts'

test('the arrow keys move down and up the menu, and go round at the ends', () => {
  assert.equal(menuMove('ArrowDown', 0, 3), 1)
  assert.equal(menuMove('ArrowDown', 2, 3), 0)
  assert.equal(menuMove('ArrowUp', 0, 3), 2)
  assert.equal(menuMove('ArrowUp', 2, 3), 1)
})

test('Home and End go to the first and the last', () => {
  assert.equal(menuMove('Home', 2, 3), 0)
  assert.equal(menuMove('End', 0, 3), 2)
})

test('from nowhere, down goes to the first and up to the last', () => {
  assert.equal(menuMove('ArrowDown', -1, 3), 0)
  assert.equal(menuMove('ArrowUp', -1, 3), 2)
})

test('any other key, or an empty menu, moves nothing', () => {
  assert.equal(menuMove('a', 1, 3), null)
  assert.equal(menuMove('ArrowDown', 0, 0), null)
})

const window = { width: 1440, height: 900 }

test('a menu opens under its button, lined up with its right edge', () => {
  const button = { left: 600, right: 680, top: 400, bottom: 440 }
  assert.deepEqual(menuPlace(button, { width: 240, height: 120 }, window), { left: 440, top: 446 })
})

test('with no room below, it opens above', () => {
  const button = { left: 600, right: 680, top: 800, bottom: 840 }
  assert.deepEqual(menuPlace(button, { width: 240, height: 120 }, window), { left: 440, top: 674 })
})

test('it never reaches past the gutter at either side of a narrow window', () => {
  const narrow = { width: 400, height: 800 }
  assert.equal(menuPlace({ left: 20, right: 90, top: 100, bottom: 140 }, { width: 240, height: 120 }, narrow).left, 16)
  assert.equal(menuPlace({ left: 300, right: 420, top: 100, bottom: 140 }, { width: 240, height: 120 }, narrow).left, 144)
})

test('with room neither below nor above, it opens below and stays in the window', () => {
  const short = { width: 400, height: 300 }
  assert.deepEqual(menuPlace({ left: 20, right: 300, top: 100, bottom: 140 }, { width: 240, height: 400 }, short), { left: 60, top: 16 })
})
