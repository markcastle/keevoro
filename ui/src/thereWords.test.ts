import assert from 'node:assert/strict'
import { test } from 'node:test'
import { ago } from './format.ts'
import type { Looked } from './backend.ts'
import { certain, lookedSaid, mainButtonWords, outOfReachLine, thereNow } from './thereWords.ts'

const NOW = 1_800_000_000
const at = (secondsAgo: number) => NOW - secondsAgo
const said = (then: number) => ago(then, NOW)
const fresh: Looked = { at: at(10), lastReached: at(10), fresh: true, reached: true, gone: 0, notAsSent: 0, said: [] }
const old: Looked = { ...fresh, at: at(3 * 86_400), lastReached: at(3 * 86_400), fresh: false }
const outOfReach: Looked = {
  ...fresh,
  at: at(5),
  lastReached: at(2 * 86_400),
  fresh: false,
  reached: false,
  said: ['Keevoro can\u2019t reach the folder E:\\Backup just now, so it can\u2019t confirm what is there.'],
}

test('what is there is said as certain only when every place was looked at a moment ago and reached', () => {
  assert.equal(certain([{ there: 4, waiting: 0, looked: fresh }]), true)
  assert.equal(certain([{ there: 4, waiting: 0, looked: fresh }, { there: 4, waiting: 0, looked: old }]), false)
  assert.equal(certain([{ there: 4, waiting: 0 }]), false, 'never looked at is not certain')
  assert.equal(certain([]), false)
  assert.equal(thereNow(0, 40, [{ there: 40, waiting: 0, looked: fresh }], said), 'Everything you are keeping is there: 40 files.')
})

test('an older look, or a place out of reach, says when Keevoro last looked and no more', () => {
  assert.equal(
    thereNow(0, 40, [{ there: 40, waiting: 0, looked: old }], said),
    'Everything you are keeping was there when Keevoro last looked, 3 days ago: 40 files.',
  )
  assert.equal(
    thereNow(0, 1_200, [{ there: 1_200, waiting: 0, looked: outOfReach }], said),
    'Everything you are keeping was there when Keevoro last looked, 2 days ago: 1,200 files.',
  )
  assert.equal(
    thereNow(0, 3, [{ there: 3, waiting: 0 }], said),
    'Everything you are keeping was there when Keevoro last looked: 3 files.',
  )
  assert.equal(
    thereNow(4, 36, [{ there: 36, waiting: 4, looked: outOfReach }], said),
    '4 files are waiting to be backed up. 36 were there when Keevoro last looked, 2 days ago.',
  )
  assert.equal(thereNow(1, 0, [{ there: 0, waiting: 1, looked: fresh }], said), '1 file is waiting to be backed up.')
  assert.equal(thereNow(4, 36, [{ there: 36, waiting: 4, looked: fresh }], said), '4 files are waiting to be backed up. 36 already there.')
  assert.equal(thereNow(0, 0, [], said), 'Nothing is being kept yet.')
})

test('a place in a list says when it cannot be reached, and what each look said comes first', () => {
  assert.equal(outOfReachLine({ there: 40, waiting: 0, looked: fresh }), null)
  assert.equal(outOfReachLine({ there: 40, waiting: 0 }), null)
  assert.equal(outOfReachLine({ there: 40, waiting: 0, looked: outOfReach }), 'Can\u2019t be reached just now · 40 there when last looked')
  const gone = { ...fresh, gone: 40, said: ['The folder F:\\Backup looks emptied or replaced.'] }
  assert.deepEqual(lookedSaid([{ there: 0, waiting: 40, looked: gone }, { there: 40, waiting: 0, looked: outOfReach }]), [
    'The folder F:\\Backup looks emptied or replaced.',
    outOfReach.said[0],
  ])
})

test('the owner\u2019s second screen: never "everything is there" beside files that are not', () => {
  const found = { ...fresh, gone: 12, notAsSent: 41 }
  assert.equal(
    thereNow(53, 48_157, [{ there: 48_157, waiting: 53, looked: found }], said),
    '48,157 of your 48,210 files are safely there. 53 need attention.',
  )
  assert.equal(
    thereNow(54, 48_156, [{ there: 48_156, waiting: 54, looked: { ...fresh, gone: 1, notAsSent: 0 } }], said),
    '48,156 of your 48,210 files are safely there. 1 needs attention. 53 more are waiting to be backed up.',
  )
  // A look that could not reach the place says nothing of what it did not see.
  assert.equal(thereNow(0, 40, [{ there: 40, waiting: 0, looked: { ...outOfReach, gone: 3 } }], said).startsWith('Everything'), true)
})

test('after a backup that missed files, the main button says what it will do', () => {
  assert.equal(mainButtonWords(132, 132), 'Back up the 132 files that were missed')
  assert.equal(mainButtonWords(1, 1), 'Back up the 1 file that was missed')
  assert.equal(mainButtonWords(200, 132), 'Back up now', 'new files too: not only the missed ones')
  assert.equal(mainButtonWords(4, 0), 'Back up now')
  assert.equal(mainButtonWords(0, 0), 'Back up again')
})
