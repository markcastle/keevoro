import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { test } from 'node:test'
import { files, foundNew, heading, holdsLine, musicOnly, named, namedWithOthers, foundOrNone } from './format.ts'

// The one table keevoro_core::words is tested against too, so the screens
// and the engine say counts the same way.
const cases: [number, number, number, number, string][] = JSON.parse(
  readFileSync(new URL('./countWords.json', import.meta.url), 'utf8'),
)

test('counts are said the way the engine says them', () => {
  assert.ok(cases.length >= 8)
  for (const [photos, videos, others, music, said] of cases) assert.equal(files(photos, videos, others, music), said)
})

// The one table keevoro_core::words::found_new is tested against too.
const foundNewCases: [string, number, number, number, number, string | null][] = JSON.parse(
  readFileSync(new URL('./foundNewWords.json', import.meta.url), 'utf8'),
)

test('what looking again found new is said the way the engine says it, and nothing new says nothing', () => {
  assert.ok(foundNewCases.length >= 6)
  for (const [name, photos, videos, others, music, said] of foundNewCases)
    assert.equal(foundNew(name, photos, videos, others, music), said)
})

// The one table keevoro_core::words::named and heading are tested against.
const holds: [boolean, boolean, boolean, string, string][] = JSON.parse(
  readFileSync(new URL('./holdsWords.json', import.meta.url), 'utf8'),
)

test('what a backup holds is named the way the engine names it', () => {
  for (const [photos, videos, music, said, title] of holds) {
    const wants = [photos && 'photos', videos && 'videos', music && 'music'].filter((w): w is string => !!w)
    assert.equal(named(wants), said)
    assert.equal(heading(wants), title)
  }
})

test('music is counted in tracks, and nothing is said in what the backup is for', () => {
  assert.equal(files(0, 0, 0, 1204), '1,204 tracks')
  assert.equal(files(238, 0, 0, 1204), '238 photos and 1,204 tracks')
  assert.equal(files(0, 0, 0, 0, ['music']), '0 tracks')
  assert.equal(files(0, 0, 0, 0, ['photos', 'videos']), '0 photos')
  assert.equal(holdsLine(['photos', 'videos']), 'Photos, Videos')
  assert.equal(holdsLine(['music']), 'Music')
  assert.ok(musicOnly(['music']) && !musicOnly(['photos', 'music']))
  assert.equal(namedWithOthers(['photos', 'videos']), 'photos, videos and other files')
  assert.equal(namedWithOthers(['music']), 'music and other files')
})

test('with no other files the words are what they always were', () => {
  assert.equal(files(12_431, 312), '12,431 photos and 312 videos')
  assert.equal(files(0, 0), '0 photos')
})

test('finding nothing names everything that was looked for, never only photos', () => {
  assert.equal(foundOrNone(['photos', 'videos', 'music'], 0, 0, 0, 0), 'no photos, videos or music')
  assert.equal(foundOrNone(['photos', 'videos'], 0, 0), 'no photos or videos')
  assert.equal(foundOrNone(['music'], 0, 0), 'no music')
  assert.equal(foundOrNone(['photos', 'others'], 0, 0), 'no photos or other files')
  assert.equal(foundOrNone(['photos', 'videos', 'music'], 3, 0, 0, 2), '3 photos and 2 tracks')
})
