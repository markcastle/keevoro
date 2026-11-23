import assert from 'node:assert/strict'
import { test } from 'node:test'
import { groupHints, outBy, startsOpen, tickOf, within } from './surveyGroups.ts'
import type { GroupRow, SurveyItem } from './surveyGroups.ts'

type Made = {
  folder: string
  photos: number
  videos: number
  others?: number
  music?: number
  bytes: number
  likelyPhotos: boolean
  alreadyASource: boolean
}

const hint = (folder: string, bytes = 1_000, more: Partial<Made> = {}): Made => ({
  folder,
  photos: 10,
  videos: 1,
  bytes,
  likelyPhotos: true,
  alreadyASource: false,
  ...more,
})

// What the list looks like, written down plainly: a group is its label and
// what is in it, a folder its label.
type Shape = string | [string, Shape[]]
const shape = (items: SurveyItem[]): Shape[] =>
  items.map((item) => (item.kind === 'folder' ? item.label : [item.label, shape(item.items)]))

const onlyGroup = (items: SurveyItem[]): GroupRow => {
  const groups = items.filter((i): i is GroupRow => i.kind === 'group')
  assert.equal(groups.length, 1)
  return groups[0]
}

test('no folders, no rows', () => {
  assert.deepEqual(groupHints([]), [])
})

test('one folder is a plain row with its whole path', () => {
  assert.deepEqual(shape(groupHints([hint('C:\\Users\\You\\Pictures')])), ['C:\\Users\\You\\Pictures'])
})

test('folders with nothing in common stay plain rows, in the order given', () => {
  const items = groupHints([hint('C:\\Users\\You\\Pictures', 3_000), hint('C:\\Old laptop backup\\Photos', 2_000), hint('C:\\Games\\Launcher\\assets', 1_000)])
  assert.deepEqual(shape(items), ['C:\\Users\\You\\Pictures', 'C:\\Old laptop backup\\Photos', 'C:\\Games\\Launcher\\assets'])
})

test('folders sharing a parent are one group, labelled by the part below it', () => {
  const items = groupHints([
    hint('D:\\Archive\\Guides\\2017\\Walks', 500),
    hint('D:\\Archive\\Guides\\2018', 400),
    hint('D:\\Archive\\Guides\\Scans', 300),
    hint('D:\\Holiday videos', 200),
  ])
  assert.deepEqual(shape(items), [['D:\\Archive\\Guides', ['2017 › Walks', '2018', 'Scans']], 'D:\\Holiday videos'])
  const group = onlyGroup(items)
  assert.equal(group.folder, 'D:\\Archive\\Guides')
  assert.deepEqual(group.totals, { folders: 3, photos: 30, videos: 3, others: 0, music: 0, bytes: 1_200 })
  // Each row still knows its whole path, for its tooltip and for anyone listening.
  const first = group.items[0]
  assert.equal(first.kind === 'folder' && first.hint.folder, 'D:\\Archive\\Guides\\2017\\Walks')
})

test('a group nests when a parent has folders of its own and a deeper group', () => {
  const items = groupHints([
    hint('D:\\Archive\\Guides\\2017\\Walks', 900),
    hint('D:\\Archive\\Guides\\2017\\Maps', 800),
    hint('D:\\Archive\\Guides\\Scans', 700),
    hint('D:\\Archive\\Guides\\Covers', 600),
    hint('D:\\Holiday videos', 1),
  ])
  assert.deepEqual(shape(items), [
    ['D:\\Archive\\Guides', [['2017', ['Walks', 'Maps']], 'Scans', 'Covers']],
    'D:\\Holiday videos',
  ])
  assert.equal(onlyGroup(items).totals.folders, 4)
})

test('a chain of single folders collapses: no group ever holds just one thing', () => {
  const items = groupHints([
    hint('C:\\a\\b\\c\\d\\e\\one\\x\\y', 2),
    hint('C:\\a\\b\\c\\d\\e\\two', 1),
    hint('C:\\Elsewhere', 0),
  ])
  assert.deepEqual(shape(items), [['C:\\a\\b\\c\\d\\e', ['one › x › y', 'two']], 'C:\\Elsewhere'])
})

test('never more than three levels of groups; below that the rows are flat', () => {
  const deep: Made[] = []
  for (const a of ['1', '2'])
    for (const b of ['1', '2'])
      for (const c of ['1', '2'])
        for (const d of ['1', '2']) deep.push(hint(`/home/you/Photos/${a}/${b}/${c}/${d}`, 10))
  deep.push(hint('/home/you/Videos', 1))
  const depth = (items: SurveyItem[]): number =>
    Math.max(0, ...items.map((i) => (i.kind === 'group' ? 1 + depth(i.items) : 0)))
  const items = groupHints(deep)
  assert.equal(depth(items), 3)
  // The deepest groups hold their folders flat, each labelled from that group.
  const g = onlyGroup(items)
  const second = g.items[0] as GroupRow
  const third = second.items[0] as GroupRow
  assert.equal(third.folder, '/home/you/Photos/1/1')
  assert.deepEqual(shape(third.items), ['1 › 1', '1 › 2', '2 › 1', '2 › 2'])
  assert.equal(g.totals.folders, 16)
})

test('a whole drive, or the one place surveyed, is never a group of everything', () => {
  // All on C:, so all share C:\. That is no help, so it is not a group.
  assert.deepEqual(shape(groupHints([hint('C:\\Users\\You\\Pictures'), hint('C:\\Games\\assets')])), [
    'C:\\Users\\You\\Pictures',
    'C:\\Games\\assets',
  ])
  // All in the folder surveyed: a single group would only repeat the heading.
  assert.deepEqual(shape(groupHints([hint('D:\\Photos\\2019', 2), hint('D:\\Photos\\2020', 1)])), [
    'D:\\Photos\\2019',
    'D:\\Photos\\2020',
  ])
})

test('Windows paths are the same folder whatever their case or slashes, written as first seen', () => {
  const items = groupHints([
    hint('C:\\Users\\You\\Pictures\\2017', 3),
    hint('c:\\users\\you\\PICTURES\\2018', 2),
    hint('C:/Users/You/Pictures/2019', 1),
    hint('C:\\Elsewhere', 0),
  ])
  assert.deepEqual(shape(items), [['C:\\Users\\You\\Pictures', ['2017', '2018', '2019']], 'C:\\Elsewhere'])
  assert.equal(within('c:\\users\\you\\pictures\\2018', 'C:\\Users\\You\\Pictures'), true)
})

test('POSIX paths keep their case: Photos and photos are two folders', () => {
  const items = groupHints([
    hint('/home/you/Photos/a', 4),
    hint('/home/you/Photos/b', 3),
    hint('/home/you/photos/c', 2),
    hint('/home/you/photos/d', 1),
  ])
  assert.deepEqual(shape(items), [
    ['/home/you/Photos', ['a', 'b']],
    ['/home/you/photos', ['c', 'd']],
  ])
  assert.equal(within('/home/you/photos/c', '/home/you/Photos'), false)
})

test('a network share is its own root, and two shares are never one group', () => {
  const items = groupHints([
    hint('\\\\NAS\\Photos\\Family photos\\2019', 4),
    hint('\\\\NAS\\Photos\\Family photos\\2020', 3),
    hint('\\\\nas\\photos\\Holiday videos', 2),
    hint('\\\\NAS\\Backups\\Mum\u2019s card', 1),
  ])
  assert.deepEqual(shape(items), [
    ['\\\\NAS\\Photos\\Family photos', ['2019', '2020']],
    '\\\\nas\\photos\\Holiday videos',
    '\\\\NAS\\Backups\\Mum\u2019s card',
  ])
})

test('different drives are never grouped together', () => {
  assert.deepEqual(shape(groupHints([hint('D:\\Photos'), hint('E:\\Photos')])), ['D:\\Photos', 'E:\\Photos'])
})

test('a suggested folder that holds other suggestions is the group, shown first inside it', () => {
  const items = groupHints([
    hint('D:\\Family photos\\2019', 400, { photos: 4 }),
    hint('D:\\Family photos', 1_000, { photos: 10 }),
    hint('D:\\Family photos\\2020', 300, { photos: 3 }),
    hint('D:\\Elsewhere', 1),
  ])
  assert.deepEqual(shape(items), [['D:\\Family photos', ['This folder itself', '2019', '2020']], 'D:\\Elsewhere'])
  // Its own counts already hold those below it, so nothing is counted twice.
  const group = onlyGroup(items)
  assert.deepEqual(group.totals, { folders: 3, photos: 10, videos: 1, others: 0, music: 0, bytes: 1_000 })
})

test('groups and rows go biggest first, with probable icons after real photos', () => {
  const items = groupHints([
    hint('C:\\Games\\Launcher\\assets', 9_000, { likelyPhotos: false }),
    hint('C:\\Games\\Puzzle\\textures', 8_000, { likelyPhotos: false }),
    hint('C:\\Small\\a', 10),
    hint('C:\\Small\\b', 20),
    hint('C:\\Big one', 100),
  ])
  assert.deepEqual(shape(items), ['C:\\Big one', ['C:\\Small', ['b', 'a']], ['C:\\Games', ['Launcher › assets', 'Puzzle › textures']]])
})

test('music and other files are added up too', () => {
  const group = onlyGroup(
    groupHints([
      hint('/home/you/Music/a', 5, { photos: 0, videos: 0, music: 12 }),
      hint('/home/you/Music/b', 5, { photos: 0, videos: 0, music: 30, others: 2 }),
      hint('/home/you/Elsewhere', 1),
    ]),
  )
  assert.deepEqual(group.totals, { folders: 2, photos: 0, videos: 0, others: 2, music: 42, bytes: 10 })
})

test('the notes roll up: all the same is said once, some is counted', () => {
  const all = onlyGroup(
    groupHints([hint('C:\\Games\\a', 2, { likelyPhotos: false }), hint('C:\\Games\\b', 1, { likelyPhotos: false }), hint('C:\\Other', 0)]),
  )
  assert.equal(all.notPhotos, 2)
  assert.equal(all.totals.folders, 2)
  const some = onlyGroup(groupHints([hint('C:\\Mixed\\a', 2, { likelyPhotos: false }), hint('C:\\Mixed\\b', 1), hint('C:\\Other', 0)]))
  assert.equal(some.notPhotos, 1)
  // One already a source has no note of its own, so it is not counted.
  const done = onlyGroup(
    groupHints([hint('C:\\Done\\a', 2, { likelyPhotos: false, alreadyASource: true }), hint('C:\\Done\\b', 1, { likelyPhotos: false }), hint('C:\\Other', 0)]),
  )
  assert.equal(done.notPhotos, 1)
})

test('a group starts closed when it is long or holds only probable icons', () => {
  const few = onlyGroup(groupHints([...['a', 'b', 'c'].map((n) => hint(`C:\\Few\\${n}`)), hint('C:\\Other')]))
  assert.equal(startsOpen(few), true)
  const many = onlyGroup(groupHints([...['a', 'b', 'c', 'd', 'e', 'f', 'g'].map((n) => hint(`C:\\Many\\${n}`)), hint('C:\\Other')]))
  assert.equal(startsOpen(many), false)
  const icons = onlyGroup(groupHints([hint('C:\\Games\\a', 1, { likelyPhotos: false }), hint('C:\\Games\\b', 1, { likelyPhotos: false }), hint('C:\\Other')]))
  assert.equal(startsOpen(icons), false)
})

test('each group has a key that is the same for the same folder in any case', () => {
  const a = onlyGroup(groupHints([hint('C:\\Few\\a'), hint('C:\\Few\\b'), hint('C:\\Other')]))
  const b = onlyGroup(groupHints([hint('c:\\FEW\\a'), hint('c:\\few\\b'), hint('C:\\Other')]))
  assert.equal(a.key, b.key)
})

test('5,000 folders group in steps that grow with their number, not its square', () => {
  const many: Made[] = []
  for (let n = 0; n < 5_000; n++) many.push(hint(`D:\\Archive\\${n % 7}\\${n % 50}\\${Math.floor(n / 50)}\\Folder ${n}`, n))
  const steps = { n: 0 }
  const items = groupHints(many, steps)
  const rows = (items: SurveyItem[]): number => items.reduce((n, i) => n + (i.kind === 'group' ? rows(i.items) : 1), 0)
  assert.equal(rows(items), 5_000)
  // Each folder is 6 names deep, and sorting costs about log2(5,000), so
  // some tens of steps each. Comparing every folder with every other would
  // take 25 million.
  assert.ok(steps.n < 5_000 * 60, `${steps.n} steps`)
})

test('the tick for a group: all, none or some of what can be ticked', () => {
  const folders = ['a', 'b', 'c']
  const can = (f: string) => f !== 'c'
  assert.equal(tickOf(folders, (f) => f === 'a' || f === 'b', can), 'all')
  assert.equal(tickOf(folders, () => false, can), 'none')
  assert.equal(tickOf(folders, (f) => f === 'a', can), 'mixed')
  // One that cannot be ticked does not make it mixed.
  assert.equal(tickOf(folders, (f) => f !== 'c', can), 'all')
  assert.equal(tickOf(folders, () => true, () => false), 'off')
})

test('the tick for a nested group counts every folder inside, at any depth', () => {
  const items = groupHints([
    hint('D:\\Archive\\Guides\\2017\\Walks', 900),
    hint('D:\\Archive\\Guides\\2017\\Maps', 800),
    hint('D:\\Archive\\Guides\\Scans', 700),
    hint('D:\\Elsewhere', 1),
  ])
  const outer = onlyGroup(items)
  assert.deepEqual(
    outer.hints.map((h) => h.folder),
    ['D:\\Archive\\Guides\\2017\\Walks', 'D:\\Archive\\Guides\\2017\\Maps', 'D:\\Archive\\Guides\\Scans'],
  )
  const folders = outer.hints.map((h) => h.folder)
  assert.equal(tickOf(folders, (f) => f.endsWith('Scans'), () => true), 'mixed')
  assert.equal(tickOf(folders, (f) => f.includes('2017'), () => true), 'mixed')
  assert.equal(tickOf(folders, () => true, () => true), 'all')
})

test('within is true for the folder itself and anything beneath it, and nothing beside it', () => {
  assert.equal(within('D:\\Archive\\Guides', 'D:\\Archive\\Guides'), true)
  assert.equal(within('D:\\Archive\\Guides\\2017', 'D:\\Archive\\Guides\\'), true)
  assert.equal(within('D:\\Archive\\Guides books', 'D:\\Archive\\Guides'), false)
  assert.equal(within('D:\\Archive', 'D:\\Archive\\Guides'), false)
  assert.equal(within('E:\\Archive\\Guides', 'D:\\Archive\\Guides'), false)
})

test('a folder is left out when it, or a folder holding it, is', () => {
  const out = new Set(['D:\\Archive\\Guides'])
  assert.equal(outBy('d:\\archive\\guides\\2017\\Walks', out), 'D:\\Archive\\Guides')
  assert.equal(outBy('D:\\Archive\\Guides', out), 'D:\\Archive\\Guides')
  assert.equal(outBy('D:\\Archive\\Other', out), undefined)
})
