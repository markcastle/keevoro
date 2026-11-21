import assert from 'node:assert/strict'
import { test } from 'node:test'
import { shownWords, toggled } from './wantsRule.ts'

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
