import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { test } from 'node:test'
import { files } from './format.ts'

// The one table keevoro_core::words is tested against too, so the screens
// and the engine say counts the same way.
const cases: [number, number, number, string][] = JSON.parse(readFileSync(new URL('./countWords.json', import.meta.url), 'utf8'))

test('counts are said the way the engine says them', () => {
  assert.ok(cases.length >= 8)
  for (const [photos, videos, others, said] of cases) assert.equal(files(photos, videos, others), said)
})

test('with no other files the words are what they always were', () => {
  assert.equal(files(12_431, 312), '12,431 photos and 312 videos')
  assert.equal(files(0, 0), '0 photos')
})
