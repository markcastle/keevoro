import assert from 'node:assert/strict'
import { test } from 'node:test'
import { splitSummary, wentWell } from './summaryLines.ts'

test('the routine figures wait under More about; what happened is shown', () => {
  const said = [
    'Safely backed up 48,078 files (312.4 GB).',
    'All 48,078 files you are keeping are safely at the folder D:\\Backups.',
    'Previews added: 96,156 files (21.0 GB).',
    'That took about 9 hours 40 minutes: 312.4 GB at an average of 9.0 MB a second.',
  ]
  assert.deepEqual(splitSummary(said), { shown: said.slice(0, 2), more: said.slice(2) })
})

test('anything that went wrong is always shown, wherever it comes', () => {
  const said = [
    'That took about 2 minutes: 1.2 GB at an average of 10.0 MB a second.',
    "Couldn't back up 132 files, because the connection to the folder Z:\\Photos was lost for a while.",
    '48,078 of the 48,210 files you are keeping are safely at the folder Z:\\Photos. 132 still need attention.',
  ]
  const { shown, more } = splitSummary(said)
  assert.deepEqual(shown, said.slice(1))
  assert.deepEqual(more, said.slice(0, 1))
})

test('nothing routine, nothing under More about', () => {
  assert.deepEqual(splitSummary(['Stopped.']), { shown: ['Stopped.'], more: [] })
  assert.deepEqual(splitSummary([]), { shown: [], more: [] })
})

test('a line saying everything is safely there went well; anything less did not', () => {
  for (const line of [
    'Your backup: all 2,017 originals are there.',
    'Your backup: all 2,017 originals are there, and each one was read back and matches exactly.',
    'All 48,078 files you are keeping are safely at the folder D:\\Backups.',
    'Your backup at the folder F:\\Family Backup is right: all 12 files you are keeping are there, and every one was read back in full and matches.',
  ]) {
    assert.equal(wentWell(line), true, line)
  }
  for (const line of [
    'Safely backed up 48,078 files (312.4 GB).',
    'Your backup: 2,010 fine, 7 with a problem:',
    '48,078 of the 48,210 files you are keeping are safely at the folder Z:\\Photos. 132 still need attention.',
    'Stopped.',
  ]) {
    assert.equal(wentWell(line), false, line)
  }
})
