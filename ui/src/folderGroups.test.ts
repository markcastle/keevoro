import assert from 'node:assert/strict'
import { test } from 'node:test'
import type { Collection } from './backend.ts'
import { folderGroups, keptOf, startsOpen, type FolderItem } from './folderGroups.ts'

const album = (parents: string[], title: string, music = 1, bytes = 10, kept = true) =>
  ({ key: [...parents, title].join('/'), title, parents, music, bytes, kept, byFolder: true }) as unknown as Collection

/** The list as a person would read it: groups in brackets with what is inside. */
const read = (items: FolderItem[]): unknown[] =>
  items.map((i) => (i.kind === 'folder' ? i.label : [i.label, read(i.items)]))

test('albums in the same folder are one folder that opens; one on its own stays a row', () => {
  const said = folderGroups([
    album(['Audio', 'NCS'], 'Uncharted'),
    album(['Audio', 'NCS'], 'Victory'),
    album(['Audio', 'NCS'], 'Voyage'),
    album(['Audio'], 'Demos'),
  ])
  assert.deepEqual(read(said), [['NCS', ['Uncharted', 'Victory', 'Voyage']], 'Demos'])
  const [ncs] = said
  assert.equal(ncs.kind, 'group')
  if (ncs.kind === 'group') {
    assert.equal(ncs.files, 3)
    assert.equal(ncs.bytes, 30)
    assert.equal(ncs.collections.length, 3)
  }
})

test('a folder with tracks of its own and folders beneath shows its own first', () => {
  const said = folderGroups([
    album(['Audio'], 'Own', 7),
    album(['Audio', 'Own'], 'Blastballs', 22),
    album(['Audio'], 'Other'),
  ])
  assert.deepEqual(read(said), [['Own', ['In this folder itself', 'Blastballs']], 'Other'])
})

test('the one place everything is in is not a group, and a lone album is never wrapped', () => {
  assert.deepEqual(read(folderGroups([album(['Music'], 'A'), album(['Music'], 'B')])), ['A', 'B'])
  assert.deepEqual(read(folderGroups([album(['Music', 'Deep', 'Down'], 'A')])), ['A'])
  assert.deepEqual(read(folderGroups([])), [])
})

test('two sources stay apart, each a folder of its albums', () => {
  const said = folderGroups([album(['Audio'], 'A'), album(['Audio'], 'B'), album(['Card'], 'C'), album(['Card'], 'D')])
  assert.deepEqual(read(said), [['Audio', ['A', 'B']], ['Card', ['C', 'D']]])
})

test('folders with nothing between them are named in one go', () => {
  const said = folderGroups([
    album(['Audio', 'Bands', 'Live'], 'One'),
    album(['Audio', 'Bands', 'Live'], 'Two'),
    album(['Audio'], 'Loose'),
  ])
  assert.deepEqual(read(said), [['Bands › Live', ['One', 'Two']], 'Loose'])
})

test('a group says whether all, some or none of it is kept, and long ones start closed', () => {
  const kept = [album(['A'], '1'), album(['A'], '2')]
  assert.equal(keptOf(kept), 'all')
  assert.equal(keptOf([kept[0], album(['A'], '3', 1, 1, false)]), 'some')
  assert.equal(keptOf([album(['A'], '3', 1, 1, false)]), 'none')
  const many = folderGroups([...Array.from({ length: 9 }, (_, i) => album(['Audio', 'NCS'], `T${i}`)), album(['Audio'], 'X')])
  assert.equal(many[0].kind === 'group' && startsOpen(many[0]), false)
  const few = folderGroups([album(['Audio', 'NCS'], 'a'), album(['Audio', 'NCS'], 'b'), album(['Audio'], 'X')])
  assert.equal(few[0].kind === 'group' && startsOpen(few[0]), true)
})

test('thousands of albums are grouped in a blink', () => {
  const lots = Array.from({ length: 20_000 }, (_, i) => album(['Audio', `Artist ${i % 500}`], `Album ${i}`))
  const began = performance.now()
  const said = folderGroups(lots)
  assert.equal(said.length, 500)
  assert.ok(performance.now() - began < 500)
})
