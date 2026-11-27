import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { test } from 'node:test'
import { dayOf, said, versions } from './changelogWords.ts'

const SAMPLE = `# Changelog

Words about the changelog.

## [Unreleased]

### Added

- **Not out yet.** This is not in the Keevoro a person is running.

## [0.2.0] - 2026-11-26

### Added

- **A green tick.** It shows when \`Back up again\` finds [everything](https://example.com/x) there.

### Fixed

- **A wobble.** Gone.
  It no longer wobbles.

## [0.1.0] - 2026-11-01

- A change with no heading.

[0.2.0]: https://example.com/compare
`

test('each released version is read with its date, headings and bullets; what is not out yet is left out', () => {
  const read = versions(SAMPLE)
  assert.deepEqual(read.map((v) => [v.version, v.date]), [['0.2.0', '2026-11-26'], ['0.1.0', '2026-11-01']])
  assert.deepEqual(read[0].groups.map((g) => [g.heading, g.changes.length]), [['Added', 1], ['Fixed', 1]])
  assert.deepEqual(read[0].groups[0].changes[0], [
    { words: 'A green tick.', bold: true },
    { words: ' It shows when ' },
    { words: 'Back up again', code: true },
    { words: ' finds everything there.' },
  ])
  assert.equal(read[0].groups[1].changes[0].map((s) => s.words).join(''), 'A wobble. Gone. It no longer wobbles.')
  assert.deepEqual(read[1].groups.map((g) => g.heading), ['Changed'])
})

test('a day is said as a person reads it', () => {
  assert.equal(dayOf('2026-11-26'), '26 November 2026')
  assert.equal(dayOf(''), '')
})

test('a link is said by its words, never its address', () => {
  assert.deepEqual(said('See [the guide](https://example.com/guide).'), [{ words: 'See the guide.' }])
})

test('the real changelog reads: every version has a date and at least one change, newest first', () => {
  const read = versions(readFileSync(new URL('../../CHANGELOG.md', import.meta.url), 'utf8'))
  assert.ok(read.length > 50)
  for (const v of read) {
    assert.match(v.date, /^\d{4}-\d{2}-\d{2}$/, v.version)
    assert.ok(v.groups.every((g) => ['Added', 'Changed', 'Fixed', 'Removed'].includes(g.heading)), `${v.version}: ${v.groups.map((g) => g.heading)}`)
  }
  const n = (v: string) => v.split('.').map(Number).reduce((a, b) => a * 1000 + b, 0)
  assert.ok(read.every((v, i) => i === 0 || n(read[i - 1].version) > n(v.version)), 'newest first')
})
