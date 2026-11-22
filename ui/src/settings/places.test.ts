import assert from 'node:assert/strict'
import { test } from 'node:test'
import { DETAILS, ROWS, SECTIONS, findRows, parsePlace, placeOfRow } from './places.ts'

test('every row has one home, in a section that exists, on a page of that section', () => {
  const ids = ROWS.map((r) => r.id)
  assert.equal(new Set(ids).size, ids.length, 'no row is listed twice')
  for (const row of ROWS) {
    assert.ok(SECTIONS.some((s) => s.id === row.section), `${row.id} is in a section`)
    if (row.detail) assert.equal(DETAILS[row.detail].section, row.section, `${row.id}'s page is in its section`)
  }
  for (const section of SECTIONS) assert.ok(ROWS.some((r) => r.section === section.id), `${section.id} has something in it`)
})

test('Advanced and Help come after the everyday sections, set apart', () => {
  const apart = SECTIONS.filter((s) => s.apart).map((s) => s.id)
  assert.deepEqual(apart, ['advanced', 'help'])
  assert.deepEqual(SECTIONS.slice(-2).map((s) => s.id), apart)
})

test('a page name from the icon by the clock opens the right place', () => {
  assert.deepEqual(parsePlace('settings'), { section: null })
  assert.deepEqual(parsePlace('settings/byItself'), { section: 'byItself' })
  assert.deepEqual(parsePlace('settings/advanced/kinds'), { section: 'advanced', detail: 'kinds' })
  assert.deepEqual(parsePlace('settings/byItself/keepRunning'), { section: 'byItself', detail: undefined, row: 'keepRunning' })
  assert.deepEqual(parsePlace('settings/advanced/agentChanges'), { section: 'advanced', detail: 'agents', row: 'agentChanges' })
  // A part not known still opens Settings, never somewhere wrong.
  assert.deepEqual(parsePlace('settings/somewhere'), { section: null })
  assert.deepEqual(parsePlace('settings/help/kinds'), { section: 'help' })
  assert.equal(parsePlace('send'), null)
})

test('a row is found by its name, by other words for it, and by its section', () => {
  assert.equal(findRows('dark')[0].id, 'theme')
  assert.equal(findRows('notifications')[0].id, 'notify')
  assert.equal(findRows('heic')[0].id, 'codecs')
  assert.equal(findRows('extensions')[0].id, 'kinds')
  assert.equal(findRows('Claude')[0].id, 'agents')
  assert.equal(findRows('thumbnails')[0].id, 'previews')
  assert.ok(findRows('advanced').every((r) => r.section === 'advanced'))
  assert.deepEqual(findRows('  '), [])
  assert.deepEqual(findRows('zebra'), [])
  // A name that starts with what is typed comes first.
  assert.equal(findRows('reset')[0].id, 'reset')
  assert.equal(placeOfRow('agentLookIn').detail, 'agents')
})
