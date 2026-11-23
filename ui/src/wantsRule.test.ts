import assert from 'node:assert/strict'
import { test } from 'node:test'
import { sameWants, shownWords, sourceArrangeChoice, sourceWantsChoice, toggled } from './wantsRule.ts'

test('other files are only a choice once a kind of them is on the list', () => {
  assert.deepEqual(
    shownWords(false).map(([w]) => w),
    ['photos', 'videos'],
  )
  assert.deepEqual(
    shownWords(true).map(([w]) => w),
    ['photos', 'videos', 'others'],
  )
})

test('unticking one keeps the others as they were, even one not shown', () => {
  assert.deepEqual(toggled(['photos', 'videos', 'others'], 'videos', false, false), ['photos', 'others'])
  assert.deepEqual(toggled(['photos'], 'others', true, true), ['photos', 'others'])
})

test('the last tick shown cannot be taken away', () => {
  assert.equal(toggled(['photos', 'others'], 'photos', false, false), null, 'other files are not shown, so nothing would be left')
  assert.equal(toggled(['others'], 'others', false, true), null)
  assert.deepEqual(toggled(['photos', 'others'], 'photos', false, true), ['others'])
})

test('a source set back to match the rest of the backup has no choice of its own again', () => {
  assert.equal(sourceWantsChoice(['videos', 'photos'], ['photos', 'videos']), 'backup', 'in any order')
  assert.deepEqual(sourceWantsChoice(['videos'], ['photos', 'videos']), ['videos'])
  assert.deepEqual(sourceWantsChoice(['photos', 'videos', 'others'], ['photos', 'videos']), ['photos', 'videos', 'others'])
  assert.equal(sameWants(['photos'], ['photos', 'others']), false)
})

test('the same for how a source is arranged', () => {
  assert.equal(sourceArrangeChoice('byDate', 'byDate'), 'backup')
  assert.equal(sourceArrangeChoice('folders', 'byDate'), 'folders')
})
