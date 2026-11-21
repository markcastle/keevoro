import { test } from 'node:test'
import assert from 'node:assert/strict'
import { menuWanted, reloadKey } from './noReload.ts'

test('the reload keys are the ones a person might press by habit', () => {
  assert.equal(reloadKey('F5', false, false), true)
  assert.equal(reloadKey('r', true, false), true)
  assert.equal(reloadKey('R', true, false), true, 'with Shift too')
  assert.equal(reloadKey('r', false, true), true, 'Cmd+R on a Mac')
  assert.equal(reloadKey('r', false, false), false, 'typing an r is typing')
  assert.equal(reloadKey('c', true, false), false, 'copying is copying')
})

test('the menu is kept for selected text and for boxes being typed in', () => {
  assert.equal(menuWanted(null, 'IMG_0001.jpg'), true)
  assert.equal(menuWanted(null, '   '), false)
  assert.equal(menuWanted(null, ''), false)
  const inABox = { closest: (selector: string) => (selector.includes('input') ? {} : null) }
  assert.equal(menuWanted(inABox as unknown as EventTarget, ''), true)
  const elsewhere = { closest: () => null }
  assert.equal(menuWanted(elsewhere as unknown as EventTarget, ''), false)
})
