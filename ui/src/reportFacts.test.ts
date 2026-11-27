import assert from 'node:assert/strict'
import { test } from 'node:test'
import { reportFacts, sentOn } from './reportFacts.ts'

// Midday, so the day is the same wherever the test runs.
const NOON = Date.UTC(2026, 10, 14, 12) / 1000

test('a report sent from this version says what it was, when, and from where', () => {
  assert.equal(sentOn(NOON), '14 November 2026')
  assert.deepEqual(reportFacts({ kind: 'idea', sentAt: NOON, appVersion: '0.59.0', screen: 'Choose what to keep' }), [
    'An idea, sent on 14 November 2026.',
    'Sent from Keevoro 0.59.0 on the Choose what to keep screen.',
  ])
})

test('the log and a way to reach the person are only said to have gone, never shown', () => {
  const base = { kind: 'problem' as const, sentAt: NOON, appVersion: '0.59.0' }
  assert.deepEqual(reportFacts({ ...base, withLog: true, withContact: true }), [
    'A problem, sent on 14 November 2026.',
    'Sent from Keevoro 0.59.0.',
    'Sent with your log and with your contact details.',
  ])
  assert.equal(reportFacts({ ...base, withLog: true })[2], 'Sent with your log.')
  assert.equal(reportFacts({ ...base, withContact: true })[2], 'Sent with your contact details.')
})

test('a report kept before this version says only what is known of it', () => {
  assert.deepEqual(reportFacts({ kind: 'problem', sentAt: NOON }), ['A problem, sent on 14 November 2026.'])
})

test('a picture is only said to have gone, with the rest that went', () => {
  const base = { kind: 'problem' as const, sentAt: NOON, appVersion: '0.59.0' }
  assert.equal(reportFacts({ ...base, withPicture: true })[2], 'Sent with a picture.')
  assert.equal(reportFacts({ ...base, withLog: true, withPicture: true })[2], 'Sent with your log and with a picture.')
  assert.equal(
    reportFacts({ ...base, withLog: true, withContact: true, withPicture: true })[2],
    'Sent with your log, with your contact details and with a picture.',
  )
})
