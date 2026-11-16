// Opens the built screens in a real browser and uses them the way a person
// would, against the stand-in backend (ui/src/demo.ts).
//
//   npm run ui:build && npm run ui:test
//
// It checks what is on screen, in the words the user reads, and saves a
// screenshot of each screen to ui/tests/screens/ (not committed) so a
// change in looks can be seen at a glance. Set KEEVORO_SCREENS to choose
// another folder.

import assert from 'node:assert/strict'
import { createServer } from 'node:http'
import { mkdirSync, readFileSync, existsSync } from 'node:fs'
import { extname, join, resolve } from 'node:path'
import { chromium } from 'playwright'

const dist = resolve('dist')
assert.ok(existsSync(join(dist, 'index.html')), 'Build the screens first: npm run ui:build')
const shots = resolve(process.env.KEEVORO_SCREENS ?? 'ui/tests/screens')
mkdirSync(shots, { recursive: true })

const types = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css', '.svg': 'image/svg+xml' }
const server = createServer((request, response) => {
  const path = new URL(request.url, 'http://x').pathname
  const file = join(dist, path === '/' ? 'index.html' : path)
  if (!file.startsWith(dist) || !existsSync(file)) return response.writeHead(404).end()
  response.writeHead(200, { 'content-type': types[extname(file)] ?? 'application/octet-stream' })
  response.end(readFileSync(file))
}).listen(0)
const base = `http://127.0.0.1:${server.address().port}`

const browser = await chromium.launch()
const page = await browser.newPage({ viewport: { width: 1440, height: 900 } })
const problems = []
page.on('pageerror', (error) => problems.push(String(error)))
page.on('console', (message) => message.type() === 'error' && problems.push(message.text()))
const shot = (name) => page.screenshot({ path: join(shots, `${name}.png`) })
const text = (selector) => page.locator(selector).first().innerText()

try {
  // --- A first-time user is asked one thing: where the library lives.
  await page.goto(`${base}/?demo=new&fast`)
  await page.getByRole('heading', { name: 'Where should your library live?' }).waitFor()
  await shot('1-welcome')
  await page.getByRole('button', { name: 'Use my Pictures folder' }).click()

  // --- With nothing found yet, Discover says what to do.
  await page.getByRole('heading', { name: 'Add your first source' }).waitFor()
  await shot('2-discover-empty')

  // --- Adding a place finds photos and shows them as collections.
  await page.getByRole('button', { name: 'Add a source', exact: true }).click()
  await page.getByRole('heading', { name: 'More discoveries' }).waitFor()
  assert.match(await text('.status'), /^\s*[\d,]+ photos and \d+ videos found$/)
  assert.equal(await page.locator('.sources .source').count(), 1)
  assert.equal(await text('.sources .source'), 'Family hard drive')
  assert.equal(await page.locator('.hero .picture img').count(), 3, 'the hero shows three pictures')
  assert.ok((await page.locator('.grid .card').count()) >= 3)
  await page.waitForFunction(() => [...document.images].every((i) => i.complete && i.naturalWidth > 0))
  await shot('3-discover')

  // --- Everything is kept unless you say otherwise, and saying so sticks.
  const first = page.locator('.grid .card').first()
  const title = await first.locator('h4').innerText()
  assert.equal(await first.locator('.keeping').innerText(), '✓ Keeping')
  await first.getByRole('button', { name: 'Don’t keep' }).click()
  await first.getByRole('button', { name: 'Keep collection' }).waitFor()
  assert.equal(await first.locator('.keeping').count(), 0)
  await shot('4-discover-one-skipped')

  // --- Archive offers one button, counts in human terms, then says it is safe.
  await page.getByRole('button', { name: 'Archive' }).click()
  assert.match(await text('.panel h2'), /^[\d,]+ photos and videos ready to gather$/)
  const listed = await page.locator('.kept li span:first-child').allInnerTexts()
  assert.ok(!listed.includes(title), `"${title}" was skipped, so it is not listed as kept`)
  await shot('5-archive-ready')
  await page.getByRole('button', { name: 'Gather my photos' }).click()
  await page.getByRole('heading', { name: 'Everything you’re keeping is safe' }).waitFor()
  assert.match(await text('.panel .done'), /^Safely copied [\d,]+ photos and videos into your library\.$/)
  assert.ok((await page.locator('.badge.safe').count()) >= 3)
  await shot('6-archive-safe')

  // --- Changing your mind brings a collection back, with more to gather.
  await page.getByRole('button', { name: 'Discover' }).click()
  await page.locator('.grid .card').first().getByRole('button', { name: 'Keep collection' }).click()
  await page.getByRole('button', { name: 'Archive' }).click()
  await page.getByRole('button', { name: 'Gather my photos' }).waitFor()

  // --- Opening a collection shows its photos, and one can be left out.
  await page.getByRole('button', { name: 'Discover', exact: true }).click()
  const opened = await page.locator('.hero h2').innerText()
  await page.getByRole('button', { name: `Open ${opened}` }).click()
  await page.getByRole('heading', { name: opened, level: 1 }).waitFor()
  const tiles = page.locator('.tiles .tile')
  const howMany = await tiles.count()
  assert.ok(howMany >= 3, 'the collection lists its photos')
  assert.match(await text('.photos .status'), new RegExp(`^Keeping ${howMany} of ${howMany}\\.`))
  await tiles.nth(1).click()
  assert.equal(await tiles.nth(1).getAttribute('aria-pressed'), 'false')
  assert.match(await text('.photos .status'), new RegExp(`^Keeping ${howMany - 1} of ${howMany}\\.`))
  await page.waitForFunction(() => [...document.images].every((i) => i.complete && i.naturalWidth > 0))
  await shot('9-collection')
  await page.getByRole('button', { name: '‹ Discover' }).click()
  await page.getByRole('heading', { name: 'More discoveries' }).waitFor()

  // --- What was set aside can be looked at, with the reason, and rescued.
  await page.getByRole('button', { name: /set aside as probably not memories$/ }).click()
  await page.getByRole('heading', { name: 'Set aside', level: 1 }).waitFor()
  assert.equal(await page.locator('.tiles .tile').count(), 3)
  assert.equal(await text('.tiles .tile .why'), 'Looks like app icons')
  await shot('10-set-aside')
  await page.locator('.tiles .tile').last().click()
  assert.equal(await page.locator('.tiles .tile').count(), 2, 'a rescued picture leaves the list')
  await page.getByRole('button', { name: '‹ Discover' }).click()
  assert.match(await text('.tidied'), /2 pictures set aside/)

  // --- A library that already has photos opens straight onto them, in dark mode too.
  await page.emulateMedia({ colorScheme: 'dark' })
  await page.goto(`${base}/?fast`)
  await page.getByRole('heading', { name: 'Cornwall' }).waitFor()
  await page.waitForFunction(() => [...document.images].every((i) => i.complete && i.naturalWidth > 0))
  await shot('7-discover-dark')

  // --- Narrow windows still work.
  await page.emulateMedia({ colorScheme: 'light' })
  await page.setViewportSize({ width: 900, height: 700 })
  await page.reload()
  await page.getByRole('heading', { name: 'Cornwall' }).waitFor()
  const overflow = await page.evaluate(() => document.documentElement.scrollWidth > window.innerWidth)
  assert.equal(overflow, false, 'nothing spills off the side of a narrow window')
  await shot('8-discover-narrow')

  assert.deepEqual(problems, [], 'the browser reported no errors')
  console.log(`UI test passed. Screenshots are in ${shots}`)
} finally {
  await browser.close()
  server.close()
}
