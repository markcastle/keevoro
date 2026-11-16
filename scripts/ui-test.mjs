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
// The step the list down the side says we are on.
const currentStep = () => page.locator('.steps li.now').innerText()

try {
  // --- A first-time user is asked nothing they cannot answer: one button.
  await page.goto(`${base}/?demo=new&fast`)
  await page.getByRole('heading', { name: 'Let’s find your photos' }).waitFor()
  assert.equal(await page.getByRole('button').count(), 2, 'one way forward, and one quiet alternative')
  await shot('1-welcome')
  await page.getByRole('button', { name: 'Get started' }).click()

  // --- Step 1 asks where the photos are, and offers the drives by name.
  await page.getByRole('heading', { name: 'Where are your photos?', level: 1 }).waitFor()
  assert.match(await currentStep(), /Find your photos/)
  const places = page.locator('.places .place')
  assert.equal(await places.count(), 3, 'the USB drive, a folder, and this computer’s disk')
  assert.match(await places.first().innerText(), /^KINGSTON \(E:\)\s+USB or external drive · 32 GB$/)
  assert.ok(await page.locator('.steps button').nth(1).isDisabled(), 'later steps wait their turn')
  assert.equal(await page.locator('.next').count(), 0, 'nothing to move on to until something is found')
  await shot('2-find')

  await places.first().click()
  // While it looks, it says what it is doing for anyone who wonders.
  await page.getByText(/^Looked at [\d,]+ files so far\. Now in E:\\/).first().waitFor()
  await shot('2b-searching')
  await page.getByRole('heading', { name: /^Found [\d,]+ photos and \d+ videos on KINGSTON \(E:\)$/ }).waitFor()
  assert.equal(await text('.kept li span'), 'KINGSTON (E:)', 'what has been looked through is listed')
  await shot('3-found')
  await page.getByRole('button', { name: 'See what was found' }).click()

  // --- Step 2 shows collections, all kept, with one switch each.
  await page.getByRole('heading', { name: 'Which of these should we keep?', level: 1 }).waitFor()
  assert.match(await currentStep(), /Choose what to keep/)
  assert.equal(await page.locator('.hero .picture img').count(), 3, 'the hero shows three pictures')
  const collage = await page.locator('.hero .collage').boundingBox()
  const firstPicture = await page.locator('.hero .picture').first().boundingBox()
  assert.ok(firstPicture.width < collage.width * 0.8, 'the hero pictures sit side by side, not stacked')
  assert.ok((await page.locator('.grid .card').count()) >= 3)
  assert.match(await text('.next .say'), /^Keeping [\d,]+ photos and videos in 5 collections\.$/)
  await page.waitForFunction(() => [...document.images].every((i) => i.complete && i.naturalWidth > 0))
  await shot('4-choose')

  // One click leaves a collection out, and the same switch brings it back.
  const first = page.locator('.grid .card').first()
  assert.equal(await first.locator('.toggle').getAttribute('aria-pressed'), 'true')
  await first.locator('.toggle').click()
  await first.locator('.toggle.off').waitFor()
  assert.match(await text('.next .say'), /in 4 collections\.$/)
  await shot('5-choose-one-left-out')

  // Looking inside a collection, one photo can be left out.
  const opened = await page.locator('.hero h2').innerText()
  await page.getByRole('button', { name: `Open ${opened}` }).click()
  await page.getByRole('heading', { name: opened, level: 1 }).waitFor()
  const tiles = page.locator('.tiles .tile')
  const howMany = await tiles.count()
  assert.ok(howMany >= 3, 'the collection lists its photos')
  await tiles.nth(1).click()
  assert.equal(await tiles.nth(1).getAttribute('aria-pressed'), 'false')
  assert.match(await text('.photos .status'), new RegExp(`^Keeping ${howMany - 1} of ${howMany}\\.`))
  await page.waitForFunction(() => [...document.images].every((i) => i.complete && i.naturalWidth > 0))
  await shot('6-inside-a-collection')
  await page.getByRole('button', { name: '‹ Back' }).click()

  // What was set aside can be looked at, with the reason, and rescued.
  await page.getByRole('button', { name: /that don’t look like photos were set aside$/ }).click()
  await page.getByRole('heading', { name: 'Set aside', level: 1 }).waitFor()
  assert.equal(await text('.tiles .tile .why'), 'Looks like app icons')
  await page.locator('.tiles .tile').last().click()
  assert.equal(await page.locator('.tiles .tile').count(), 2, 'a rescued picture leaves the list')
  await page.getByRole('button', { name: '‹ Back' }).click()

  // --- Step 3: one button from where you already are, then proof.
  await page.getByRole('button', { name: 'Gather them into my library' }).click()
  await page.getByRole('heading', { name: 'Your photos are safe', level: 1 }).waitFor()
  assert.match(await text('.panel h2'), /^[\d,]+ photos and videos in 4 collections$/)
  assert.match(await text('.panel .done'), /^Safely copied [\d,]+ photos and videos into your library\.$/)
  assert.ok(await page.getByRole('button', { name: 'Show me my photos' }).isVisible())
  assert.equal(await page.locator('.steps li.done').count(), 2, 'finding and choosing are ticked off')
  await shot('7-gathered')
  await page.getByRole('button', { name: 'Keep a second copy' }).click()

  // --- Step 4: a second copy. Cloud storage asks only what it needs.
  await page.getByRole('heading', { name: 'Keep a second copy', level: 1 }).waitFor()
  await page.getByRole('button', { name: 'Use cloud storage instead' }).click()
  await page.getByLabel('Cloudflare account ID').fill('0123456789abcdef0123456789abcdef')
  assert.equal(await page.getByLabel('Secret access key').getAttribute('type'), 'password', 'the secret is hidden')
  await shot('8-backup-cloud-form')
  await page.getByRole('button', { name: 'Cancel' }).click()
  await page.getByRole('button', { name: 'Back up to another disk…' }).click()
  await page.getByText('A second copy is kept in the folder F:\\Family Backup.').waitFor()
  await page.getByRole('button', { name: 'Back up now' }).click()
  await page.locator('.backup .done').first().waitFor()
  assert.match(await text('.backup .done'), /^Safely backed up [\d,]+ files\.$/)
  await page.getByRole('button', { name: 'Check my backup' }).click()
  await page.getByText(/^Your backup: all [\d,]+ originals are there\.$/).waitFor()
  assert.equal(await page.locator('.steps li.done').count(), 3, 'everything before this step is ticked')
  await shot('9-backed-up')

  // --- A source can be removed, after being told what that does and does not do.
  await page.getByRole('button', { name: 'Add another source' }).click()
  await page.getByRole('heading', { name: 'Where are your photos?', level: 1 }).waitFor()
  assert.equal(await page.locator('.kept li.source').count(), 1)
  await page.getByRole('button', { name: /^Remove / }).click()
  assert.match(await text('.kept li.source .explain'), /Nothing in .+ itself is changed or deleted/)
  await shot('9b-remove-source')
  await page.getByRole('button', { name: 'Cancel' }).click()
  assert.equal(await page.locator('.kept li.source').count(), 1, 'cancelling keeps the source')
  await page.getByRole('button', { name: /^Remove / }).click()
  await page.getByRole('button', { name: 'Remove from Keevoro' }).click()
  await page.locator('.kept li.source').waitFor({ state: 'detached' })

  // --- Adding a source again finds its photos again.
  await page.getByRole('button', { name: /Choose a folder/ }).click()
  await page.getByRole('heading', { name: /^Found [\d,]+ photos/ }).waitFor()
  await page.getByRole('button', { name: 'See what was found' }).click()
  assert.match(await text('.next'), /Gather them into my library/, 'the new photos are waiting to be gathered')

  // --- A whole disk is asked about before it is looked through.
  await page.goto(`${base}/?demo=empty&fast`)
  await page.getByRole('heading', { name: 'Where are your photos?', level: 1 }).waitFor()
  await page.getByRole('button', { name: /Windows \(C:\)/ }).click()
  await page.getByRole('heading', { name: 'Look through all of Windows (C:)?' }).waitFor()
  assert.match(await text('.asking'), /whole disk \(512 GB\).*a few minutes to an hour or more/s)
  await shot('9d-whole-disk')
  await page.getByRole('button', { name: 'Cancel' }).click()
  assert.equal(await page.locator('.asking').count(), 0, 'cancelling goes back to the choices')
  assert.equal(await page.locator('.searching').count(), 0, 'and nothing was started')
  await page.getByRole('button', { name: /Windows \(C:\)/ }).click()
  await page.getByRole('button', { name: 'Look through all of it' }).click()
  await page.getByRole('heading', { name: /^Found [\d,]+ photos/ }).waitFor()

  // --- Settings holds everything in one place, including the log.
  await page.getByRole('button', { name: 'Settings' }).click()
  await page.getByRole('heading', { name: 'Settings', level: 1 }).waitFor()
  for (const title of ['Your library', 'Backup', 'Updates', 'What Keevoro has been doing']) {
    await page.getByRole('heading', { name: title, level: 3 }).waitFor()
  }
  await page.getByRole('button', { name: 'Show the log' }).click()
  assert.match(await text('.settings .log'), /Reading a large file \(3\.9 GB\): E:\\Video\\wedding\.mov/)
  await page.getByRole('button', { name: 'Check for updates' }).click()
  await page.getByText('That is the newest version.').waitFor()
  await shot('9e-settings')
  await page.getByRole('button', { name: '‹ Back' }).click()
  await page.getByRole('heading', { name: 'Where are your photos?', level: 1 }).waitFor()

  // --- A newer version is offered calmly and can be installed in one click.
  await page.goto(`${base}/?fast&update`)
  assert.match(await text('.update'), /A new version of Keevoro is ready \(9\.9\.0\)/)
  await shot('9c-update-ready')
  await page.getByRole('button', { name: 'Update now' }).click()
  await page.getByText(/has been downloaded and opened/).waitFor()
  await page.goto(`${base}/?fast&update`)
  await page.getByRole('button', { name: 'Later' }).click()
  assert.equal(await page.locator('.update').count(), 0, 'Later puts the offer away')

  // --- An existing library opens on the step that still needs doing, in dark mode too.
  await page.emulateMedia({ colorScheme: 'dark' })
  await page.goto(`${base}/?fast`)
  await page.getByRole('heading', { name: 'Which of these should we keep?', level: 1 }).waitFor()
  await page.waitForFunction(() => [...document.images].every((i) => i.complete && i.naturalWidth > 0))
  await shot('10-choose-dark')

  // --- Narrow windows still work.
  await page.emulateMedia({ colorScheme: 'light' })
  await page.setViewportSize({ width: 900, height: 700 })
  await page.reload()
  await page.getByRole('heading', { name: 'Which of these should we keep?', level: 1 }).waitFor()
  const overflow = await page.evaluate(() => document.documentElement.scrollWidth > window.innerWidth)
  assert.equal(overflow, false, 'nothing spills off the side of a narrow window')
  await shot('11-choose-narrow')

  assert.deepEqual(problems, [], 'the browser reported no errors')
  console.log(`UI test passed. Screenshots are in ${shots}`)
} finally {
  await browser.close()
  server.close()
}
