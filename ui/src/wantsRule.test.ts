import assert from 'node:assert/strict'
import { test } from 'node:test'
import { NEW_BACKUP_WANTS, sameWants, shownWords, sourceArrangeChoice, sourceWantsChoice, toggled, toggledNew } from './wantsRule.ts'

test('other files are only a choice once a kind of them is on the list', () => {
  assert.deepEqual(
    shownWords(false).map(([w]) => w),
    ['photos', 'videos', 'music'],
  )
  assert.deepEqual(
    shownWords(true).map(([w]) => w),
    ['photos', 'videos', 'music', 'others'],
  )
})

test('a new backup is for photos and videos until the person says, and keeps one on', () => {
  assert.deepEqual(NEW_BACKUP_WANTS, ['photos', 'videos'])
  assert.deepEqual(toggledNew(['photos', 'videos'], 'music', true), ['photos', 'videos', 'music'])
  assert.deepEqual(toggledNew(['photos'], 'music', true), ['photos', 'music'])
  assert.deepEqual(toggledNew(['photos', 'music'], 'photos', false), ['music'])
  assert.equal(toggledNew(['music'], 'music', false), null, 'at least one stays on')
})

test('music is a switch of its own, beside the others', () => {
  assert.deepEqual(toggled(['photos', 'videos'], 'music', true, false), ['photos', 'videos', 'music'])
  assert.equal(toggled(['music'], 'music', false, false), null)
  assert.deepEqual(sourceWantsChoice(['music'], ['photos', 'videos']), ['music'])
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
