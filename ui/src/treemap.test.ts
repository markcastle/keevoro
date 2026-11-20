// The map's layout, tested by Node on its own: `npm run ui:unit`.

import assert from 'node:assert/strict'
import { test } from 'node:test'
import { everyBox, hit, layout, squarify } from './treemap.ts'
import type { Laid, Rect } from './treemap.ts'

type Item = { name: string; size: number; children?: Item[] }
const read = { size: (i: Item) => i.size, children: (i: Item) => i.children }
const folder = (name: string, children: Item[]): Item => ({ name, size: children.reduce((n, c) => n + c.size, 0), children })
const file = (name: string, size: number): Item => ({ name, size })

const area = (r: Rect) => r.w * r.h
const overlap = (a: Rect, b: Rect) =>
  Math.max(0, Math.min(a.x + a.w, b.x + b.w) - Math.max(a.x, b.x)) * Math.max(0, Math.min(a.y + a.h, b.y + b.h) - Math.max(a.y, b.y))
const insideOf = (inner: Rect, outer: Rect) =>
  inner.x >= outer.x - 1e-6 &&
  inner.y >= outer.y - 1e-6 &&
  inner.x + inner.w <= outer.x + outer.w + 1e-6 &&
  inner.y + inner.h <= outer.y + outer.h + 1e-6

/** A made-up spread of sizes, like a folder's: a few big and many small. */
const spread = (n: number) => Array.from({ length: n }, (_, i) => Math.round(10_000 / (i + 1) ** 1.3) + (i % 7))

/** How far from square the boxes are, at worst on average. */
const aspect = (boxes: Rect[]) =>
  boxes.filter((b) => area(b) > 0).reduce((n, b) => n + Math.max(b.w / b.h, b.h / b.w), 0) / boxes.filter((b) => area(b) > 0).length

/** Slices the rectangle in strips along its longer side: what a squarified layout must beat. */
const sliced = (sizes: number[], r: Rect): Rect[] => {
  const total = sizes.reduce((n, s) => n + s, 0)
  let at = 0
  return sizes.map((s) => {
    const part = s / total
    const box = r.w >= r.h ? { x: r.x + at * r.w, y: r.y, w: part * r.w, h: r.h } : { x: r.x, y: r.y + at * r.h, w: r.w, h: part * r.h }
    at += part
    return box
  })
}

const canvas = { x: 0, y: 0, w: 800, h: 500 }

test('each box takes room in proportion to its size, within half of one per cent', () => {
  const sizes = spread(60)
  const total = sizes.reduce((n, s) => n + s, 0)
  const boxes = squarify(sizes, canvas)
  sizes.forEach((s, i) => {
    const wanted = (s / total) * area(canvas)
    assert.ok(Math.abs(area(boxes[i]) - wanted) <= wanted * 0.005, `box ${i}: ${area(boxes[i])} for ${wanted}`)
  })
})

test('no two boxes overlap, and every box is inside the space it was given', () => {
  const sizes = spread(200)
  const boxes = squarify(sizes, canvas)
  for (let i = 0; i < boxes.length; i++) {
    assert.ok(insideOf(boxes[i], canvas), `box ${i} is inside`)
    for (let j = i + 1; j < boxes.length; j++) assert.ok(overlap(boxes[i], boxes[j]) < 1e-6, `boxes ${i} and ${j} overlap`)
  }
})

test('boxes are nearer square than slicing the space in strips', () => {
  const sizes = spread(40)
  assert.ok(aspect(squarify(sizes, canvas)) < aspect(sliced(sizes, canvas)) / 3)
})

test('something of no size gets no room, and nothing at all gets nothing', () => {
  const boxes = squarify([500, 0, 300, 0], canvas)
  assert.equal(area(boxes[1]), 0)
  assert.equal(area(boxes[3]), 0)
  assert.ok(Math.abs(area(boxes[0]) + area(boxes[2]) - area(canvas)) < 1e-6)
  assert.deepEqual(squarify([0, 0], canvas).map(area), [0, 0])
})

test('the same sizes are always laid out the same way, equal ones in the order given', () => {
  const tree = folder('top', [file('a', 10), file('b', 10), file('c', 10), file('d', 10)])
  const once = layout(tree, canvas, read)
  const again = layout(tree, canvas, read)
  assert.deepEqual(
    once.children.map((b) => [b.item.name, b.x, b.y, b.w, b.h]),
    again.children.map((b) => [b.item.name, b.x, b.y, b.w, b.h]),
  )
  assert.deepEqual(
    once.children.map((b) => b.item.name),
    ['a', 'b', 'c', 'd'],
  )
})

const pictures = folder('Pictures', [
  folder('Texture packs', [file('rocks.zip', 5_000), file('grass.zip', 3_000), folder('Old', [file('a.zip', 900), file('b.zip', 700)])]),
  folder('Camera Roll', [file('Photos', 4_000), file('Videos', 2_000)]),
  file('loose.jpg', 600),
  file('tiny.txt', 0.001),
])

test('folders big enough are opened up, with a strip for the name and their contents inside', () => {
  const laid = layout(pictures, canvas, read, 3)
  const packs = laid.children.find((b) => b.item.name === 'Texture packs') as Laid<Item>
  assert.ok(packs.inner, 'opened up')
  assert.equal(packs.inner.y - packs.y, 18, 'below its name strip')
  assert.equal(packs.children.length, 3)
  for (const box of everyBox(laid)) {
    for (const child of box.children) assert.ok(insideOf(child, box.inner as Rect), `${child.item.name} is inside ${box.item.name}`)
    for (let i = 0; i < box.children.length; i++)
      for (let j = i + 1; j < box.children.length; j++) assert.ok(overlap(box.children[i], box.children[j]) < 1e-6)
  }
  assert.ok(!laid.children.some((b) => b.item.name === 'tiny.txt'), 'too small to see is left to its folder')
  // To one level only, nothing is opened up.
  assert.ok(layout(pictures, canvas, read, 1).children.every((b) => b.children.length === 0))
})

test('a point finds the deepest box under it and the boxes above it', () => {
  const laid = layout(pictures, canvas, read, 3)
  const packs = laid.children.find((b) => b.item.name === 'Texture packs') as Laid<Item>
  const rocks = packs.children.find((b) => b.item.name === 'rocks.zip') as Laid<Item>
  const path = hit(laid, rocks.x + rocks.w / 2, rocks.y + rocks.h / 2)
  assert.deepEqual(
    path.map((b) => b.item.name),
    ['Pictures', 'Texture packs', 'rocks.zip'],
  )
  assert.deepEqual(
    hit(laid, packs.x + 4, packs.y + 4).map((b) => b.item.name),
    ['Pictures', 'Texture packs'],
    'the name strip is the folder itself',
  )
  assert.deepEqual(hit(laid, -5, 10), [], 'outside the map, nothing')
})

test('eight thousand boxes are laid out quickly', () => {
  const many = folder(
    'many',
    Array.from({ length: 80 }, (_, i) => folder(`f${i}`, spread(100).map((s, j) => file(`${i}-${j}`, s)))),
  )
  const started = performance.now()
  const laid = layout(many, { x: 0, y: 0, w: 1200, h: 800 }, read, 2)
  const took = performance.now() - started
  assert.ok(everyBox(laid).length > 1_000)
  assert.ok(took < 200, `took ${took} ms`)
})
