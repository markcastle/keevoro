import assert from 'node:assert/strict'
import { test } from 'node:test'
import { DETAILS, ROWS, SECTIONS, findRows, parsePlace, placeOfRow, rowInfo, sectionsOf, whereOf, whereSaid } from './places.ts'

test('every row has one home, in a section that exists, on a page of that section', () => {
  const ids = ROWS.map((r) => r.id)
  assert.equal(new Set(ids).size, ids.length, 'no row is listed twice')
  for (const row of ROWS) {
    assert.ok(SECTIONS.some((s) => s.id === row.section), `${row.id} is in a section`)
    if (row.detail) assert.equal(DETAILS[row.detail].section, row.section, `${row.id}'s page is in its section`)
  }
  for (const section of SECTIONS) assert.ok(ROWS.some((r) => r.section === section.id), `${section.id} has something in it`)
})

test('Settings holds only what you set, and Help sits beside it with its own sections', () => {
  assert.deepEqual(
    sectionsOf('settings').map((s) => s.id),
    ['backup', 'byItself', 'appearance', 'updates', 'advanced'],
  )
  assert.deepEqual(
    sectionsOf('help').map((s) => s.id),
    ['howTo', 'report', 'compare', 'log', 'about'],
  )
  // Advanced, and About, come last, set apart.
  assert.deepEqual(SECTIONS.filter((s) => s.apart).map((s) => s.id), ['advanced', 'about'])
  assert.equal(whereOf('advanced'), 'settings')
  assert.equal(whereOf('about'), 'help')
  // Reset lives at the foot of Advanced now, not in Help.
  assert.equal(ROWS.filter((r) => r.section === 'advanced').at(-1)?.id, 'reset')
})

test('a page name from the icon by the clock opens the right place', () => {
  assert.deepEqual(parsePlace('settings'), { where: 'settings', section: null })
  assert.deepEqual(parsePlace('settings/byItself'), { where: 'settings', section: 'byItself' })
  assert.deepEqual(parsePlace('settings/advanced/kinds'), { where: 'settings', section: 'advanced', detail: 'kinds' })
  assert.deepEqual(parsePlace('settings/byItself/keepRunning'), { where: 'settings', section: 'byItself', detail: undefined, row: 'keepRunning' })
  assert.deepEqual(parsePlace('settings/advanced/agentChanges'), { where: 'settings', section: 'advanced', detail: 'agents', row: 'agentChanges' })
  assert.deepEqual(parsePlace('help'), { where: 'help', section: null })
  assert.deepEqual(parsePlace('help/compare'), { where: 'help', section: 'compare' })
  assert.deepEqual(parsePlace('help/about/licences'), { where: 'help', section: 'about', detail: 'licences' })
  assert.deepEqual(parsePlace('help/report/yourReports'), { where: 'help', section: 'report', detail: undefined, row: 'yourReports' })
  // A part not known still opens the place, never somewhere wrong.
  assert.deepEqual(parsePlace('settings/somewhere'), { where: 'settings', section: null })
  assert.deepEqual(parsePlace('help/somewhere'), { where: 'help', section: null })
  assert.deepEqual(parsePlace('help/backup'), { where: 'help', section: null }, 'a Settings section is not in Help')
  assert.equal(parsePlace('send'), null)
})

test('the names Settings, Help had before still lead to where each thing lives now', () => {
  assert.deepEqual(parsePlace('settings/help'), { where: 'help', section: null })
  assert.deepEqual(parsePlace('settings/help/report'), { where: 'help', section: 'report' })
  assert.deepEqual(parsePlace('settings/help/yourReports'), { where: 'help', section: 'report', detail: undefined, row: 'yourReports' })
  assert.deepEqual(parsePlace('settings/help/log'), { where: 'help', section: 'log' })
  assert.deepEqual(parsePlace('settings/help/licences'), { where: 'help', section: 'about', detail: 'licences' })
  assert.deepEqual(parsePlace('settings/help/reset'), { where: 'settings', section: 'advanced', detail: undefined, row: 'reset' })
  assert.deepEqual(parsePlace('settings/help/kinds'), { where: 'settings', section: 'advanced', detail: 'kinds' })
  // About was a screen of its own.
  assert.deepEqual(parsePlace('about'), { where: 'help', section: 'about' })
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
  // Telling Keevoro's maker about something, by any of the words for it.
  for (const word of ['feedback', 'bug', 'idea', 'suggest']) assert.equal(findRows(word)[0].id, 'report', word)
  assert.equal(findRows('problem')[0].id, 'report')
  assert.deepEqual(placeOfRow('yourReports'), { where: 'help', section: 'report', detail: undefined, row: 'yourReports' })
})

test('one search finds what is in Settings and what is in Help, and says where each is', () => {
  assert.equal(findRows('compare')[0].id, 'compare')
  assert.equal(findRows('prices')[0].id, 'compare')
  assert.equal(findRows('report')[0].id, 'report')
  assert.equal(findRows('licences')[0].id, 'licences')
  assert.equal(findRows('licenses')[0].id, 'licences')
  assert.equal(findRows('about')[0].id, 'about')
  assert.equal(findRows('reset')[0].section, 'advanced')
  assert.equal(findRows('log')[0].id, 'log')
  // A question finds its answer.
  assert.equal(findRows('new computer')[0].id, 'howNewComputer')
  assert.equal(findRows('plugged in')[0].id, 'howPluggedIn')
  // Searching for updates finds the setting first, and About's version too.
  const updates = findRows('updates').map((r) => r.id)
  assert.equal(updates[0], 'version')
  assert.ok(updates.includes('aboutVersion'))
  // A row that is a whole section leads to the section, with nothing to mark.
  assert.deepEqual(placeOfRow('compare'), { where: 'help', section: 'compare' })
  assert.deepEqual(placeOfRow('about'), { where: 'help', section: 'about' })
  assert.deepEqual(placeOfRow('licences'), { where: 'help', section: 'about', detail: undefined, row: 'licences' })
  assert.deepEqual(placeOfRow('reset'), { where: 'settings', section: 'advanced', detail: undefined, row: 'reset' })
  // Where a found row is, said beside it.
  assert.equal(whereOf(findRows('compare')[0].section), 'help')
  assert.equal(whereOf(findRows('dark')[0].section), 'settings')
  assert.equal(whereSaid(rowInfo('report')), 'Help')
  assert.equal(whereSaid(rowInfo('yourReports')), 'Help, Report a problem or suggest an idea')
  assert.equal(whereSaid(rowInfo('reset')), 'Settings, Advanced')
  assert.equal(whereSaid(rowInfo('agentChanges')), 'Settings, Advanced, AI agents')
})
