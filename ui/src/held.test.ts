import assert from 'node:assert/strict'
import { test } from 'node:test'
import { heldShare } from './held.ts'

test('the bar moves forward with the work', () => {
  assert.equal(heldShare(0.2, 0.25), 0.25)
})

test('a small slip back is held, so the bar does not twitch', () => {
  assert.equal(heldShare(0.5, 0.47), 0.5)
})

test('a reading well below what is shown wins, so the bar is never stuck on a wrong figure', () => {
  // The fault this guards against: one early reading of "all done" held the bar full for the whole backup.
  assert.equal(heldShare(0.999, 0.2), 0.2)
  assert.equal(heldShare(1, 0), 0)
})

test('a reading that is not a number changes nothing', () => {
  assert.equal(heldShare(0.3, Number.NaN), 0.3)
})
