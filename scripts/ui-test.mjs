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
const shot = async (name) => {
  await settled()
  await page.screenshot({ path: join(shots, `${name}.png`) })
}
const text = (selector) => page.locator(selector).first().innerText()
// The step the list down the side says we are on.
const currentStep = () => page.locator('.steps li.now').innerText()
// Nothing on the page reaches past the window's right edge, or past the
// page's own column, except inside a box that scrolls sideways by itself
// (a wide table). The window itself never scrolls sideways.
const spills = () =>
  page.evaluate(() => {
    const found = []
    if (document.documentElement.scrollWidth > window.innerWidth) found.push(`the page is ${document.documentElement.scrollWidth} wide`)
    const main = document.querySelector('main')
    const edge = Math.min(window.innerWidth, main ? main.getBoundingClientRect().right : window.innerWidth)
    const scrollsItself = (el) => {
      for (let up = el.parentElement; up && up !== main; up = up.parentElement) {
        const x = getComputedStyle(up).overflowX
        if ((x === 'auto' || x === 'scroll') && up.getBoundingClientRect().right <= edge + 1) return true
      }
      return false
    }
    for (const el of document.querySelectorAll('body *')) {
      const box = el.getBoundingClientRect()
      if (box.width === 0 || box.right <= edge + 1 || getComputedStyle(el).position === 'fixed') continue
      if (scrollsItself(el)) continue
      found.push(`${el.tagName.toLowerCase()}.${String(el.className).split(' ')[0]} reaches ${Math.round(box.right)} of ${Math.round(edge)}`)
    }
    return found.slice(0, 5)
  })
// In a narrow window the side bar is a band across the top. Says how the
// page's first heading is covered by it, if it is: their boxes overlap.
// A heading scrolled away above the window is not covered.
const headingUnderBand = () =>
  page.evaluate(() => {
    const band = document.querySelector('.sidebar')?.getBoundingClientRect()
    const heading = document.querySelector('main h1, main h2')?.getBoundingClientRect()
    if (!band || !heading) return null
    const overlap = heading.top < band.bottom && heading.bottom > band.top && heading.left < band.right && heading.right > band.left
    return overlap ? `the heading (${Math.round(heading.top)} to ${Math.round(heading.bottom)}) is under the band (to ${Math.round(band.bottom)})` : null
  })
// The same, with the page as it is and scrolled a little way down, which is
// where a heading would slide under a band that stayed put.
const headingClearOfBand = async (where) => {
  assert.equal(await headingUnderBand(), null, where)
  const scrolled = await page.evaluate(() =>
    [document.scrollingElement, ...document.querySelectorAll('.app, main')].map((el) => {
      const was = el.scrollTop
      el.scrollTop = was + 60
      return was
    }),
  )
  assert.equal(await headingUnderBand(), null, `${where}, scrolled down a little`)
  await page.evaluate((was) => [document.scrollingElement, ...document.querySelectorAll('.app, main')].forEach((el, n) => (el.scrollTop = was[n])), scrolled)
}
// Waits until the page is drawn as it will stay: its typeface has loaded
// (until then a wider stand-in draws the words, which once pushed a button
// past the edge on a slow machine), every easing of colour or place has
// finished (the map's too), and two frames have been laid out since. Never a guess at a
// number of milliseconds.
const settled = () =>
  page.evaluate(async () => {
    await document.fonts.ready
    // Spinners and shimmers go round for ever; only easings end.
    const easing = document.getAnimations().filter((a) => a.effect?.getComputedTiming().iterations !== Infinity)
    await Promise.all(easing.map((a) => a.finished.catch(() => {})))
    // The map eases on a canvas, marking itself while it does.
    await new Promise((done) => {
      const check = () => (document.querySelector('[data-easing]') ? requestAnimationFrame(check) : done())
      check()
    })
    await new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(() => done())))
  })
// Checks the screen as it is at narrow widths, then puts the window back.
const fitsNarrow = async (screen) => {
  for (const width of [400, 560]) {
    await page.setViewportSize({ width, height: 800 })
    await settled()
    assert.deepEqual(await spills(), [], `${screen} at ${width} pixels wide spills sideways`)
    await headingClearOfBand(`${screen} at ${width} pixels wide`)
  }
  await page.setViewportSize({ width: 1440, height: 900 })
}

try {
  // --- A first-time user is asked one thing they can answer: what to call this.
  await page.goto(`${base}/?demo=new&fast`)
  await page.getByRole('heading', { name: 'Let’s find your photos' }).waitFor()
  assert.equal(await page.getByRole('button').count(), 1, 'one way forward')
  assert.equal(await page.getByLabel('What shall we call this backup?').inputValue(), 'My photos', 'with a name already suggested')
  await page.getByLabel('What shall we call this backup?').fill('Family photos')
  assert.equal(await page.locator('.welcome .brand img:visible').getAttribute('alt'), 'Keevoro', 'the logo, name and all')
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
  assert.equal(await text('.kept li.source .what b'), 'KINGSTON (E:)', 'what has been looked through is listed, by name')
  assert.equal(await text('.kept li.source .what .folder'), 'E:\\', 'with where it is beneath, to open')
  assert.match(await text('.kept li.source .count'), /^[\d,]+ photos and videos\s*[\d.]+ (MB|GB)$/, 'and how much is on it')
  // Kinds of file are folded away until asked for, then one switch each.
  assert.equal(await page.locator('.types').count(), 0, 'nothing extra until it is asked for')
  await page.getByRole('button', { name: /^File types on / }).click()
  assert.deepEqual(await page.locator('.types li b').allInnerTexts(), ['JPG', 'DNG', 'MP4'])
  const dng = page.getByRole('switch', { name: 'Include DNG files' })
  assert.equal(await dng.getAttribute('aria-checked'), 'true', 'raw files are included unless switched off')
  await dng.click()
  assert.equal(await dng.getAttribute('aria-checked'), 'false')
  assert.match(await page.locator('.types li').nth(1).innerText(), /left out/)
  await shot('3b-file-types')
  await dng.click()
  // One source can be photos only, whatever the backup is for, and each
  // ending says which level left it out.
  const sameAsBackup = page.getByRole('checkbox', { name: /^From .*, back up the same as the backup$/ })
  assert.equal(await sameAsBackup.isChecked(), true)
  await sameAsBackup.uncheck()
  const sourceWants = page.getByRole('group', { name: /^From .*, back up$/ })
  assert.deepEqual(await sourceWants.getByRole('checkbox').evaluateAll((boxes) => boxes.map((b) => b.parentElement.innerText.trim())), ['Photos', 'Videos'], 'no other files to choose until a kind of them is listed')
  await sourceWants.getByRole('checkbox', { name: 'Videos' }).uncheck()
  await page.locator('.source-wants').getByRole('status').getByText(/This source is set to photos only\. Anything already backed up stays backed up/).waitFor()
  assert.match(await page.locator('.types li').nth(2).innerText(), /left out: this source is set to photos only/)
  await shot('3c-source-photos-only')
  // The last tick cannot be taken away: a source is always for something.
  await sourceWants.getByRole('checkbox', { name: 'Photos' }).click({ force: true })
  assert.equal(await sourceWants.getByRole('checkbox', { name: 'Photos' }).isChecked(), true)
  await sameAsBackup.check()
  await page.locator('.types li').nth(2).getByText(/left out/).waitFor({ state: 'detached' })
  await page.getByRole('button', { name: /^File types on / }).click()
  assert.equal(await page.locator('.types').count(), 0)
  // The bar is part of the window: on its bottom edge, full width.
  const bar = await page.locator('.next').boundingBox()
  const view = page.viewportSize()
  assert.ok(Math.abs(bar.y + bar.height - view.height) <= 1, 'the bar sits on the bottom edge of the window')
  // The careful checking carries on behind, and says so quietly, as what
  // it is doing and never as an instruction, with its own Pause and Stop.
  assert.match(
    await text('.foot .checking'),
    /^Checking for duplicates in the background \([\d,]+ of [\d,]+\)\. You can keep using Keevoro while it finishes\./,
  )
  assert.doesNotMatch(await text('.foot'), /You can carry on/)
  await page.locator('.foot .checking').getByRole('button', { name: 'Stop checking for duplicates' }).waitFor()
  await page.locator('.foot .checking').waitFor({ state: 'detached' })
  await shot('3-found')
  await page.getByRole('button', { name: 'See what was found' }).click()

  // --- Step 2 shows collections, all kept, with one switch each.
  await page.getByRole('heading', { name: 'Which of these should we keep?', level: 1 }).waitFor()
  assert.match(await text('.status'), /sorted what it found into collections.*one source often makes several/s, 'why there is more than one')
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
  // The tick in a picture's corner leaves it out or puts it back.
  const kept = (n) => tiles.nth(n).getByRole('switch').getAttribute('aria-checked')
  await tiles.nth(1).getByRole('switch').click()
  assert.equal(await kept(1), 'false')
  assert.match(await text('.photos .status'), new RegExp(`^Keeping ${howMany - 1} of ${howMany}\\.`))
  await page.waitForFunction(() => [...document.images].every((i) => i.complete && i.naturalWidth > 0))
  await shot('6-inside-a-collection')

  // Everything here can be kept or left out in one go, and the folder it
  // came from is named, to open.
  await page.getByRole('button', { name: 'Keep none' }).click()
  assert.match(await text('.photos .status'), new RegExp(`^Keeping 0 of ${howMany}\\.`))
  await page.getByRole('button', { name: 'Keep all' }).click()
  assert.match(await text('.photos .status'), new RegExp(`^Keeping ${howMany} of ${howMany}\\.`))
  assert.ok(await page.getByRole('button', { name: 'Keep all' }).isDisabled())
  assert.match(await text('.photos .from'), /^From /)

  // A click on a picture shows it large and does not change the tick.
  const picture = (n) => tiles.nth(n).locator('.look')
  assert.equal(await kept(0), 'true')
  await picture(0).click()
  const viewer = page.getByRole('dialog')
  await viewer.waitFor()
  await viewer.locator('.stage img').waitFor()
  assert.equal(await kept(0), 'true', 'a click is a look, not a tick')
  assert.match(await viewer.innerText(), new RegExp(`1 of ${howMany}`))
  await shot('6b-a-closer-look')
  // It can be left out from here, the arrows move on, and Escape closes.
  await viewer.getByRole('switch').click()
  assert.equal(await kept(0), 'false')
  await viewer.getByRole('switch').click()
  await page.keyboard.press('ArrowRight')
  assert.match(await viewer.innerText(), new RegExp(`2 of ${howMany}`))
  await page.keyboard.press('Escape')
  assert.equal(await page.getByRole('dialog').count(), 0)
  // A double click ticks or unticks, and does not open the picture.
  await picture(0).dblclick()
  assert.equal(await kept(0), 'false')
  await page.waitForTimeout(500)
  assert.equal(await page.getByRole('dialog').count(), 0, 'a double click does not also open it')
  await picture(0).dblclick()
  assert.equal(await kept(0), 'true')
  assert.match(await text('.photos .status'), /Click a picture to see it larger\. Double-click it, or click its tick/)
  await page.getByRole('button', { name: '‹ Back' }).click()

  // Duplicates can be looked at before anything is backed up: which copy
  // is kept, which are left out, and how Keevoro knows they are the same.
  // The figures are in plain sight: what is kept, what was left out as a
  // duplicate and the room that saves, and what was set aside.
  assert.match(await text('.tally'), /Keeping\s*[\d,]+\s*of [\d,]+ photos and [\d,]+ videos found/)
  assert.match(await text('.tally'), /Duplicates left out\s*155\s*saving [\d.]+ MB · \d+% of all files found/)
  await page.getByRole('button', { name: /^Duplicates left out/ }).click()
  await page.getByRole('heading', { name: 'Duplicates', level: 1 }).waitFor()
  await page.locator('.duplicates .groups li').first().waitFor()
  assert.equal(await text('.duplicates .status'), '2 files were found more than once. One copy of each is kept, and 3 extra copies are left out, saving 3.9 GB.')
  assert.match(await text('.duplicates .knows'), /only when every byte of one matches every byte of the other/)
  assert.match(await text('.duplicates .knows'), /1 file below is marked still being checked/)
  assert.deepEqual(await page.locator('.duplicates .groups li').first().locator('.mark').allInnerTexts(), ['Kept', 'Left out', 'Left out'])
  await shot('9o-duplicates')
  await page.getByRole('button', { name: '‹ Back' }).click()

  // What was set aside can be looked at, with the reason, and rescued.
  await page.getByRole('button', { name: /^Set aside/ }).click()
  await page.getByRole('heading', { name: 'Set aside', level: 1 }).waitFor()
  assert.equal(await text('.tiles .tile .why'), 'Looks like app icons')
  await page.locator('.tiles .tile').last().getByRole('switch').click()
  assert.equal(await page.locator('.tiles .tile').count(), 2, 'a rescued picture leaves the list')
  await page.getByRole('button', { name: '‹ Back' }).click()

  // --- Step 3: one question, where should they go, with a folder first.
  await page.getByRole('button', { name: 'Choose where they go' }).click()
  await page.getByRole('heading', { name: 'Where should they go?', level: 1 }).waitFor()
  const kinds = page.locator('.destinations .place b')
  assert.deepEqual(await kinds.allInnerTexts(), ['A folder', 'Cloudflare R2', 'Amazon S3', 'Other storage'])
  // It says how much room the files will need before a place is chosen.
  assert.match(await text('.send .needs'), /^You are backing up [\d,]+ files, about [\d.]+ (MB|GB)\. Wherever you choose needs at least that much free\.$/)
  await shot('7-where-should-they-go')
  // Cloud storage asks only what it needs, and hides the secret.
  await page.getByRole('button', { name: /Cloudflare R2/ }).click()
  await page.getByLabel('Cloudflare account ID').fill('0123456789abcdef0123456789abcdef')
  assert.equal(await page.getByLabel('Secret access key').getAttribute('type'), 'password', 'the secret is hidden')
  await shot('8-cloud-form')
  await page.getByRole('button', { name: 'Cancel' }).click()
  // A folder is one click, then one button sends everything.
  await page.getByRole('button', { name: /^A folder/ }).click()
  await page.getByRole('heading', { name: 'Backing up to the folder F:\\Family Backup' }).waitFor()
  await page.getByText(/^[\d,]+ files are waiting to be backed up\.$/).waitFor()
  // The folder's name opens the folder, and the room is weighed against the disk.
  assert.ok(await page.getByRole('button', { name: 'F:\\Family Backup' }).isVisible())
  assert.match(await text('.send .needs'), /^They need about [\d.]+ (MB|GB)\. That disk has [\d.]+ GB free, so there is room\.$/)
  await page.getByRole('button', { name: 'Back up now' }).click()
  await page.locator('.send .done').first().waitFor()
  assert.match(await text('.send .done'), /^Safely backed up [\d,]+ files\.$/)
  await page.getByText(/^Previews added: [\d,]+ files\.$/).waitFor()
  // When it is done, it says how long it took and how fast it went.
  await page.getByText(/^That took about \d+ minutes: [\d.]+ GB at an average of [\d.]+ MB a second\.$/).waitFor()
  // The backup's folders are explained, a rendition included.
  await page.getByText('What is in the backup', { exact: true }).click()
  assert.match(await text('.sends.layout'), /A rendition is a version of a photo or video made for looking at, not for keeping\./)
  assert.deepEqual(await page.locator('.sends.layout dt').allInnerTexts(), ['originals', 'renditions', 'index', 'What is in this backup.txt'])
  await shot('9n-what-is-in-the-backup')
  // A backup can be the originals alone: the switch says what each choice means.
  const withPreviews = page.getByRole('switch', { name: 'Also make previews for a gallery' })
  assert.equal(await withPreviews.getAttribute('aria-checked'), 'true', 'previews are made unless switched off')
  await withPreviews.click()
  assert.equal(await withPreviews.getAttribute('aria-checked'), 'false')
  assert.match(await text('.switch-row.previews'), /Only your original files will be backed up, exactly as they are/)
  await withPreviews.click()
  await page.getByText('How earlier backups went').click()
  const runs = page.locator('.sends > .sends-scroll > table > tbody > tr')
  assert.deepEqual(await page.locator('.sends > .sends-scroll > table > thead th').allInnerTexts(), ['When', 'Where', 'Backed up', 'Took', 'Average speed', 'Details'])
  assert.equal(await runs.count(), 3, 'this backup and the two before')
  assert.match(await runs.nth(1).innerText(), /folder \\\\NAS\\Photos\s+720\.6 GB\s+about 22 hours 50 minutes\s+9\.0 MB\/s/)
  assert.match(await runs.nth(2).innerText(), /folder F:\\Family Backup\s+30\.1 GB\s+about 41 minutes\s+12\.5 MB\/s/)
  // A row opens in place to everything recorded about that backup.
  const details = runs.nth(1).locator('button[aria-controls]')
  assert.match(await details.getAttribute('aria-label'), /^Show everything about the backup on/)
  assert.equal(await details.getAttribute('aria-expanded'), 'false')
  await details.focus()
  await page.keyboard.press('Enter')
  assert.equal(await details.getAttribute('aria-expanded'), 'true', 'opened from the keyboard')
  assert.match(await details.getAttribute('aria-label'), /^Hide everything/)
  const panel = page.locator('.run-detail')
  assert.match(await panel.innerText(), /720\.6 GB to the folder \\\\NAS\\Photos in about 22 hours 50 minutes, at an average of 9\.0 MB a second\./)
  assert.match(await panel.innerText(), /from the app, with Keevoro 0\.32\.0\. It finished\. It was paused for about 10 minutes/)
  assert.match(await panel.locator('table.stages').innerText(), /Backing up your files\s+Went wrong: Couldn’t back up 2 files\s+about 21 hours\s+716\.8 GB\s+9\.7 MB a second\s+slowest 3\.2 MB a second, fastest 14\.6 MB a second/)
  // A stage that went wrong says so in words beside its mark, not by colour alone.
  assert.equal(await panel.locator('table.stages .went-wrong').count(), 1)
  assert.match(await panel.innerText(), /11,243 files sent \(716\.8 GB\), 2 could not be sent\./)
  assert.match(await panel.innerText(), /1,406 duplicates left out, saving 38\.2 GB\./)
  assert.match(await panel.innerText(), /21,792 preview files sent \(3\.8 GB\)/)
  assert.match(await panel.innerText(), /NAS\\Photos, across a network: 720\.6 GB/)
  assert.match(await panel.innerText(), /What it said at the end\s+Safely backed up 11,243 files/)
  await panel.scrollIntoViewIfNeeded()
  await shot('9v-a-backup-in-full')
  // An older one says only what was kept, and why.
  await runs.last().getByRole('button', { name: /^Show everything/ }).click()
  await page.getByText('Only these figures were kept for this backup. Fuller detail is kept for every backup from now on.').waitFor()
  assert.equal(await page.locator('.run-detail').count(), 1, 'one open at a time')
  // In a narrow window the table scrolls inside itself, never the page.
  await details.click()
  await page.setViewportSize({ width: 760, height: 700 })
  await panel.scrollIntoViewIfNeeded()
  assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > window.innerWidth), false, 'the page does not spill sideways')
  await shot('9w-a-backup-in-full-narrow')
  await page.setViewportSize({ width: 1440, height: 900 })
  await details.click()
  await page.getByText(/^Everything you are keeping is there: [\d,]+ files\.$/).waitFor()
  assert.ok(await page.getByRole('button', { name: 'Show me the folder' }).isVisible())
  // Beside it, clearing what a stopped backup left there: said first, cleared on a yes.
  await page.getByRole('button', { name: 'Clear unfinished copies…' }).click()
  const unfinished = page.getByRole('group', { name: 'Unfinished copies in F:\\Family Backup' })
  assert.match(await unfinished.innerText(), /^There are 320\.4 MB of unfinished copies \(12 files\) in F:\\Family Backup/)
  await unfinished.getByRole('button', { name: 'Cancel' }).click()
  assert.equal(await unfinished.count(), 0, 'cancelling clears nothing and puts the question away')
  await page.getByRole('button', { name: 'Clear unfinished copies…' }).click()
  await unfinished.getByRole('button', { name: 'Clear them' }).click()
  await unfinished.getByText(/^Cleared 320\.4 MB of unfinished copies \(12 files\) from F:\\Family Backup\./).waitFor()
  await unfinished.getByRole('button', { name: 'Done' }).click()
  await page.getByRole('button', { name: 'Check they all arrived' }).click()
  await page.getByText(/^Your backup: all [\d,]+ originals are there\.$/).waitFor()
  assert.equal(await page.locator('.steps li.done').count(), 2, 'finding and choosing are ticked off')
  assert.equal(await page.locator('.steps li').count(), 3, 'there are three steps, not four')
  await shot('9-sent')
  // --- A backup can go to more than one place. A second destination is
  // added beside the first, starts empty, and catches up on the next backup.
  await page.getByRole('button', { name: 'Add another destination' }).click()
  await page.getByRole('heading', { name: 'Add another destination' }).waitFor()
  assert.match(await text('.send .adding'), /as well as the folder F:\\Family Backup\. It starts empty/)
  await page.getByRole('button', { name: /Cloudflare R2/ }).click()
  await page.getByLabel('Cloudflare account ID').fill('0123456789abcdef0123456789abcdef')
  await page.getByLabel('Bucket name').fill('family-photos')
  await page.getByLabel('Access key ID').fill('key')
  await page.getByLabel('Secret access key').fill('secret')
  await page.locator('.send form button[type=submit]').click()
  await page.getByRole('heading', { name: 'Backing up to 2 places' }).waitFor()
  const placesTo = page.locator('.places-to li')
  assert.equal(await placesTo.count(), 2)
  assert.match(await placesTo.nth(0).innerText(), /The folder\s+F:\\Family Backup\s+All [\d,]+ there/)
  assert.match(await placesTo.nth(1).innerText(), /The Cloudflare R2 bucket family-photos\s+0 there · [\d,]+ waiting/)
  // Nothing is safe again until the new place has caught up.
  await page.getByText(/^[\d,]+ files are waiting to be backed up\.$/).waitFor()
  await shot('9s-two-destinations')
  await page.getByRole('button', { name: 'Back up now' }).click()
  await page.getByText(/^Everything you are keeping is there: [\d,]+ files\.$/).waitFor()
  assert.match(await placesTo.nth(1).innerText(), /All [\d,]+ there/)
  // Taking one off the list leaves the other, and deletes nothing.
  await page.getByRole('button', { name: 'Stop backing up to the Cloudflare R2 bucket family-photos' }).click()
  await page.getByRole('heading', { name: 'Backing up to the folder F:\\Family Backup' }).waitFor()
  assert.equal(await page.locator('.places-to').count(), 0)
  await page.getByRole('button', { name: 'Check they all arrived' }).click()
  await page.getByText(/^Your backup: all [\d,]+ originals are there\.$/).waitFor()

  // What it said at the end is still there after looking at another screen.
  await page.getByRole('button', { name: /^Choose what to keep/ }).click()
  await page.getByRole('heading', { name: 'Which of these should we keep?' }).waitFor()
  await page.getByRole('button', { name: /^Back them up somewhere safe/ }).click()
  await page.getByText(/^Your backup: all [\d,]+ originals are there\.$/).waitFor()

  // --- Backups: a second job, kept apart, and the first is still there afterwards.
  assert.match(await text('.backup .current'), /Family photos/)
  await page.getByRole('button', { name: 'Backup', exact: true }).click()
  await page.getByRole('button', { name: 'Start a new backup…' }).click()
  await page.getByLabel('Name for the new backup').fill('Drone footage')
  await shot('9a-new-backup')
  await page.getByRole('button', { name: 'Start', exact: true }).click()
  await page.getByRole('heading', { name: 'Where are your photos?', level: 1 }).waitFor()
  assert.match(await text('.backup .current'), /Drone footage/)
  assert.equal(await page.locator('.kept li.source').count(), 0, 'a new backup starts with no sources')
  assert.ok(await page.locator('.steps button').nth(1).isDisabled(), 'and nothing chosen')
  await page.getByRole('button', { name: 'Backup', exact: true }).click()
  await page.getByRole('button', { name: 'Family photos' }).click()
  await page.locator('.kept li.source').first().waitFor()
  assert.match(await text('.backup .current'), /Family photos/)
  assert.equal(await page.locator('.steps li.done').count(), 2, 'the first backup is exactly as it was left')
  await page.getByRole('button', { name: 'Back them up somewhere safe' }).click()
  await page.getByRole('heading', { name: 'Backing up to the folder F:\\Family Backup' }).waitFor()

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
  assert.match(await text('.next'), /Choose where they go/, 'the new photos are waiting to be sent')

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

  // --- Not sure where the photos are? A quick survey finds the folders,
  // opening nothing, and the ticked ones are looked through one by one.
  await page.goto(`${base}/?demo=empty&fast`)
  await page.getByRole('button', { name: /Windows \(C:\)/ }).click()
  assert.match(await text('.asking'), /Keevoro can find the folders that hold them first\. That takes seconds/)
  await page.getByRole('button', { name: 'Find the folders with photos' }).click()
  await page.getByRole('heading', { name: '4 folders on Windows (C:) seem to hold photos and videos' }).waitFor()
  assert.match(await text('.survey'), /Looked at 481,907 files in 36,412 folders in 6\.2 seconds, without opening any of them/)
  const ticks = page.locator('.survey .hints input')
  assert.deepEqual(
    await ticks.evaluateAll((boxes) => boxes.map((b) => b.checked)),
    [true, true, true, false],
    'real photos are ticked to begin with; probable icons are not',
  )
  assert.match(await page.locator('.survey .hints li').last().innerText(), /Small pictures, probably not photos/)
  await shot('9q-survey')
  // Files only in OneDrive are never opened, and never left out silently: each suggestion says so.
  const onlineSaid = /^412 files here are in OneDrive only, not on this computer, so Keevoro did not open them\. To include them, make them available offline in OneDrive first, then look again\.$/
  assert.match(await page.locator('.survey .hints li').nth(1).locator('.online-only').innerText(), onlineSaid)
  // A library an older Keevoro left is never offered, and is said quietly, on one line that opens.
  const leftOut = page.locator('.survey .left-out')
  assert.equal(await leftOut.locator('summary').innerText(), 'Left out a folder Keevoro made, as it is not a source')
  assert.equal(await leftOut.getByRole('button', { name: 'Show me the folder' }).isVisible(), false, 'folded away')
  await leftOut.locator('summary').click()
  assert.match(await leftOut.innerText(), /a folder an older version of Keevoro made, which it no longer uses\. It takes 9\.2 GB: 8\.4 GB of copies of your photos/)
  assert.match(await leftOut.innerText(), /never removes it/)
  await leftOut.getByRole('button', { name: 'Show me the folder' }).click()
  await leftOut.scrollIntoViewIfNeeded()
  await shot('9x-survey-old-library')
  // Unticking one leaves two, and the button says so.
  await ticks.nth(2).uncheck()
  await page.getByRole('button', { name: 'Look through these 2 folders' }).click()
  // Both are looked through, one after the other, with no more asked.
  await page.locator('.kept li.source').nth(1).waitFor()
  assert.equal(await page.locator('.kept li.source').count(), 2, 'each ticked folder became a source')
  assert.equal(await page.locator('.survey').count(), 0)
  // After the search, on screen, not only in the log; and kept with the source.
  await page.locator('.found-it .online-only').getByText(onlineSaid).waitFor()
  assert.match(await page.locator('.kept li.source').nth(1).locator('.online-only').innerText(), onlineSaid)
  await page.locator('.kept li.source').nth(1).scrollIntoViewIfNeeded()
  await shot('9q2-online-only')
  await page.locator('.steps button').nth(1).click()
  await page.locator('.steps button').nth(0).click()
  assert.match(await page.locator('.kept li.source').nth(1).locator('.online-only').innerText(), onlineSaid, 'still said after going elsewhere')

  // One button looks through every source again for anything new.
  await page.getByRole('button', { name: 'Look through all 2 again for new photos' }).click()
  await page.getByRole('heading', { name: /^Found [\d,]+ photos/ }).waitFor()
  await page.getByRole('button', { name: 'Look through all 2 again for new photos' }).waitFor()
  assert.equal(await page.locator('.kept li.source').count(), 2, 'looking again adds no source twice')

  // --- Back from a collection lands where you were, not at the top.
  await page.setViewportSize({ width: 1440, height: 520 })
  await page.goto(`${base}/`)
  await page.locator('.card').last().waitFor()
  await page.locator('.card').last().scrollIntoViewIfNeeded()
  const before = await page.locator('main').evaluate((m) => m.scrollTop)
  assert.ok(before > 100, 'the screen was scrolled down')
  await page.locator('.card').last().locator('button').first().click()
  await page.locator('.tiles .tile').first().waitFor()
  assert.equal(await page.locator('main').evaluate((m) => m.scrollTop), 0, 'a collection opens at its top')
  await page.getByRole('button', { name: '‹ Back' }).click()
  await page.locator('.card').last().waitFor()
  assert.equal(await page.locator('main').evaluate((m) => m.scrollTop), before, 'and back is where you were')
  await page.setViewportSize({ width: 1440, height: 900 })
  await page.goto(`${base}/?demo=empty&fast`)

  // --- About has its own page: what Keevoro promises, the name, the licence.
  await page.getByRole('button', { name: 'About Keevoro' }).click()
  await page.getByRole('heading', { name: 'About Keevoro', level: 1 }).waitFor()
  assert.deepEqual(await page.locator('.about .promises dt').allInnerTexts(), [
    'Your sources are only ever read',
    'A copy only counts when it is proved exact',
    'No original in a backup is ever replaced or removed',
    'Duplicates are exact, never guesses',
    'Nothing is hidden from you',
    'You never need Keevoro to get your photos back',
    'Your photos go only where you send them',
  ])
  assert.match(await text('.about'), /voro, an old Swedish word meaning “were”/)
  assert.match(await text('.about'), /Copyright 2026 Mark Castle\..*Functional Source License \(FSL-1\.1-MIT\)/s)
  for (const name of ['Read the licence', 'The name and logo', 'See the source']) {
    await page.getByRole('button', { name }).waitFor()
  }
  assert.equal(await page.getByText('Your sources are only ever read.', { exact: true }).count(), 0, 'no longer a stray line in the side bar')
  await shot('9r-about')

  // --- From About, every piece of open source work in the app, with its licence.
  await page.getByRole('button', { name: 'Open source licences' }).click()
  await page.getByRole('heading', { name: 'Open source licences', level: 1 }).waitFor()
  assert.match(await text('.licences'), /The app and the keevoro command \(\d{3,}\)/)
  assert.match(await text('.licences'), /The app’s screens \(\d+\)/)
  assert.match(await page.locator('.licensed li', { hasText: 'react-dom' }).innerText(), /react-dom\s+[\d.]+\s+MIT/)
  await fitsNarrow('The open source licences')
  await page.getByLabel('Find by name').fill('serde_json')
  assert.match(await text('.licences'), /The app and the keevoro command \(1 of \d+\)/)
  assert.match(await text('.licences'), /None by that name\./)
  await shot('9r2-licences')
  await page.getByRole('button', { name: '‹ Back to About' }).click()
  await page.getByRole('heading', { name: 'About Keevoro', level: 1 }).waitFor()
  await page.getByRole('button', { name: '‹ Back', exact: true }).click()

  // --- Settings holds everything in one place, including the log.
  await page.getByRole('button', { name: 'Settings' }).click()
  await page.getByRole('heading', { name: 'Settings', level: 1 }).waitFor()
  for (const title of ['This backup', 'Where it backs up to', 'Appearance', 'Speed', 'AI agents', 'Updates', 'What Keevoro has been doing']) {
    await page.getByRole('heading', { name: title, level: 3 }).waitFor()
  }
  // Speed: the computer's own codecs are used unless switched off, and it says what that means.
  const codecs = page.getByRole('switch', { name: 'Use this computer’s own picture codecs' })
  assert.equal(await codecs.getAttribute('aria-checked'), 'true', 'on unless switched off')
  assert.match(await text('.settings'), /Photos are opened by: this computer’s own codecs, then the built-in decoder/)
  await codecs.click()
  assert.equal(await codecs.getAttribute('aria-checked'), 'false')
  await page.getByText(/Photos are opened by: the built-in decoder, then/).waitFor()
  await codecs.click()
  // AI agents: looking only, and no folders, until the person says.
  const agentChanges = page.getByRole('switch', { name: 'Let AI agents make changes' })
  assert.equal(await agentChanges.getAttribute('aria-checked'), 'false', 'an agent may only look until allowed')
  await page.getByText('None yet, so it cannot look for photos anywhere.').waitFor()
  await agentChanges.click()
  assert.equal(await agentChanges.getAttribute('aria-checked'), 'true')
  await page.locator('.agent-folders').nth(0).getByRole('button', { name: 'Add a folder…' }).click()
  await page.getByRole('list', { name: 'Folders an agent may look through' }).getByText('D:\\Photos').waitFor()
  await page.locator('.agent-folders').nth(1).getByRole('button', { name: 'Add a folder…' }).click()
  await page.getByRole('list', { name: 'Folders an agent may back up to' }).getByText('F:\\Family Backup').waitFor()
  await page.locator('.agent-folders').nth(0).scrollIntoViewIfNeeded()
  await shot('9s-settings-agents')
  await page.getByRole('button', { name: 'Stop allowing D:\\Photos' }).click()
  await page.getByText('None yet, so it cannot look for photos anywhere.').waitFor()
  await agentChanges.click()
  assert.equal(await agentChanges.getAttribute('aria-checked'), 'false')
  // Appearance: light, dark, or the same as the computer, and it is remembered.
  const ground = () => page.evaluate(() => getComputedStyle(document.body).backgroundColor)
  assert.equal(await ground(), 'rgb(255, 254, 251)', 'the cream ground of the light theme')
  assert.equal(await page.getByRole('button', { name: 'Match my computer' }).getAttribute('aria-pressed'), 'true')
  await page.getByRole('button', { name: 'Dark', exact: true }).click()
  assert.equal(await ground(), 'rgb(16, 39, 37)', 'the deep teal ground of the dark theme')
  assert.match(await page.locator('.brand img:visible').getAttribute('src'), /lockup-reversed/, 'the logo for dark grounds')
  await shot('9f-settings-dark')
  await page.getByRole('button', { name: 'Light', exact: true }).click()
  assert.equal(await ground(), 'rgb(255, 254, 251)')
  assert.doesNotMatch(await page.locator('.brand img:visible').getAttribute('src'), /lockup-reversed/)
  assert.equal(await page.evaluate(() => localStorage.getItem('keevoro-theme')), 'light', 'the choice is remembered')
  await page.getByRole('button', { name: 'Match my computer' }).click()
  assert.equal(await page.evaluate(() => document.documentElement.dataset.theme), undefined)
  assert.equal(await page.evaluate(() => getComputedStyle(document.body).fontFamily.includes('Keevoro Sans')), true)
  await page.getByRole('button', { name: 'Show the log' }).click()
  assert.match(await text('.settings .log'), /Reading a large file \(3\.9 GB\): E:\\Video\\wedding\.mov/)
  await page.getByRole('button', { name: 'Check for updates' }).click()
  await page.getByText('That is the newest version.').waitFor()
  await shot('9e-settings')

  // --- A whole backup can be photos only, or videos only, behind its own Advanced door.
  await page.locator('.setting').filter({ hasText: 'This backup' }).getByText('Advanced', { exact: true }).click()
  const backupWants = page.getByRole('group', { name: 'This backup is for' })
  assert.equal(await backupWants.getByRole('checkbox', { name: 'Photos' }).isChecked(), true)
  assert.equal(await backupWants.getByRole('checkbox', { name: 'Videos' }).isChecked(), true)
  assert.equal(await backupWants.getByRole('checkbox', { name: 'Other files' }).count(), 0, 'no other files to choose until a kind of them is listed')
  await backupWants.getByRole('checkbox', { name: 'Photos' }).uncheck()
  await page.getByRole('status').getByText(/This backup is for videos only\. Anything already backed up stays backed up; it is just no longer kept/).waitFor()
  await backupWants.scrollIntoViewIfNeeded()
  await shot('9y-settings-videos-only')
  await backupWants.getByRole('checkbox', { name: 'Photos' }).check()
  await page.getByRole('status').getByText(/^This backup is for photos and videos\. This counts at once/).waitFor()

  // --- Advanced: the kinds of file looked for, behind a door of their own.
  assert.equal(await page.getByRole('heading', { name: 'Kinds of file Keevoro looks for' }).isVisible(), false, 'folded away')
  await page.locator('details.advanced').last().getByText('Advanced', { exact: true }).click()
  await page.getByRole('heading', { name: 'Kinds of file Keevoro looks for', level: 3 }).waitFor()
  // A folder no backup goes to any more can still be cleared of a stopped
  // backup's working files: chosen, then said, then cleared, in the page.
  await page.getByRole('button', { name: 'Clear unfinished copies from a folder…' }).click()
  const oldFolder = page.getByRole('group', { name: 'Unfinished copies in G:\\Old Keevoro backup' })
  assert.match(await oldFolder.innerText(), /^There are 15\.2 GB of unfinished copies \(583 files\) in G:\\Old Keevoro backup, left by a backup that was stopped part-way\. Clearing them touches nothing that was backed up\./)
  await oldFolder.scrollIntoViewIfNeeded()
  await shot('9z-settings-clear-unfinished')
  await oldFolder.getByRole('button', { name: 'Clear them' }).click()
  await oldFolder.getByText(/^Cleared 15\.2 GB of unfinished copies \(583 files\) from G:\\Old Keevoro backup\./).waitFor()
  assert.match(await oldFolder.innerText(), /Left 1 thing written to in the last hour, as a backup may be using it right now\. Try again later\./)
  await oldFolder.getByRole('button', { name: 'Done' }).click()
  await page.getByRole('button', { name: 'Clear unfinished copies from a folder…' }).click()
  await oldFolder.getByText('There are no unfinished copies to clear in G:\\Old Keevoro backup.').waitFor()
  await oldFolder.getByRole('button', { name: 'Done' }).click()
  const kindsTable = page.locator('.kinds-table')
  const row = (ending) => kindsTable.getByRole('row').filter({ has: page.getByRole('cell', { name: ending, exact: true }) })
  assert.match(await row('.jpg').innerText(), /Photo\s+Built in/)
  assert.match(await row('.insp').innerText(), /Photo\s+Yours/)
  assert.equal(await row('.jpg').getByRole('button', { name: 'Remove .jpg' }).count(), 0, 'a built-in kind cannot be removed')
  // Switching one off says when it counts, and offers the way to the sources.
  const avi = page.getByRole('switch', { name: 'Look for .avi files' })
  await avi.click()
  assert.equal(await avi.getAttribute('aria-checked'), 'false')
  await page.getByRole('status').getByRole('button', { name: 'Go to your sources' }).waitFor()
  // Adding: a refusal is said beside the box, in plain words.
  await page.getByRole('button', { name: 'Add a kind of file…' }).click()
  const adding = page.getByRole('form', { name: 'Add a kind of file' })
  await adding.getByLabel('Ending').fill('my pics')
  await adding.getByRole('button', { name: 'Add' }).click()
  await adding.getByRole('alert').getByText(/Use letters and numbers only/).waitFor()
  await adding.getByLabel('Ending').fill('.LRV')
  await adding.getByLabel('They are').selectOption('video')
  await adding.getByRole('button', { name: 'Add' }).click()
  assert.match(await row('.lrv').innerText(), /Video\s+Yours/)
  // Editing a built-in kind: photo or video, but its ending stays.
  await page.getByRole('button', { name: 'Change .jpe', exact: true }).click()
  const editing = page.getByRole('form', { name: 'Change .jpe', exact: true })
  assert.equal(await editing.getByLabel('Ending').isDisabled(), true, 'a built-in ending cannot be renamed')
  await editing.getByRole('button', { name: 'Cancel' }).click()
  // Removing one of your own is asked about on the page, never in a pop-up.
  await page.getByRole('button', { name: 'Remove .insp' }).click()
  await page.getByRole('alertdialog', { name: 'Remove .insp?' }).waitFor()
  await row('.lrv').scrollIntoViewIfNeeded()
  await shot('9t-settings-kinds-of-file')
  await page.getByRole('button', { name: 'Yes, take it off' }).click()
  assert.equal(await row('.insp').count(), 0)
  await page.getByRole('button', { name: 'Put back the built-in list' }).click()
  assert.equal(await avi.getAttribute('aria-checked'), 'true', 'every built-in kind is back on')
  assert.equal(await row('.lrv').count(), 1, 'your own kinds stay')
  // At a narrow width the table scrolls inside itself, never the page.
  await page.setViewportSize({ width: 560, height: 800 })
  await kindsTable.scrollIntoViewIfNeeded()
  assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > window.innerWidth), false, 'the page does not spill sideways')
  await shot('9u-settings-kinds-narrow')
  await page.setViewportSize({ width: 1440, height: 900 })
  await page.getByRole('status').getByRole('button', { name: 'Go to your sources' }).click()
  await page.getByRole('heading', { name: 'Where are your photos?', level: 1 }).waitFor()

  // --- Other files, carried along: added in the kinds list, found by
  // looking again, listed by folder under the collections, each opening to
  // a plain list, and on the map as a third kind wanted.
  await page.goto(`${base}/?fast`)
  await page.getByRole('heading', { name: 'Which of these should we keep?', level: 1 }).waitFor()
  assert.equal(await page.getByRole('heading', { name: 'Other files' }).count(), 0, 'nothing of the sort until a person adds a kind')
  await page.getByRole('button', { name: 'Settings' }).click()
  await page.locator('details.advanced').last().getByText('Advanced', { exact: true }).click()
  await page.getByRole('button', { name: 'Add a kind of file…' }).click()
  const addOther = page.getByRole('form', { name: 'Add a kind of file' })
  await addOther.getByLabel('Ending').fill('pdf')
  await addOther.getByRole('button', { name: 'Add' }).click()
  // A document is never a photo: the refusal offers the way that works.
  await addOther.getByRole('alert').getByText(/never photos or videos\. To back them up as they are, add \.pdf as an other file\./).waitFor()
  await addOther.getByRole('button', { name: 'Add it as other files' }).click()
  const otherKinds = page.locator('.kinds-table')
  const otherRow = (ending) => otherKinds.getByRole('row').filter({ has: page.getByRole('cell', { name: ending, exact: true }) })
  assert.match(await otherRow('.pdf').innerText(), /Other file\s+Yours/)
  await page.getByRole('button', { name: 'Add a kind of file…' }).click()
  await addOther.getByLabel('Ending').fill('flac')
  await addOther.getByLabel('They are').selectOption('other')
  await addOther.getByText('Keevoro will back these up exactly, but cannot show them or group them by date.').waitFor()
  await addOther.scrollIntoViewIfNeeded()
  await shot('9v-settings-kinds-other-file')
  await addOther.getByRole('button', { name: 'Add' }).click()
  assert.match(await otherRow('.flac').innerText(), /Other file\s+Yours/)
  await otherRow('.flac').scrollIntoViewIfNeeded()
  await shot('9v2-settings-kinds-with-other-files')
  await fitsNarrow('The kinds list with other files')
  // Now "Other files" is a choice for the backup too.
  await page.locator('.setting').filter({ hasText: 'This backup' }).getByText('Advanced', { exact: true }).click()
  const forBackup = page.getByRole('group', { name: 'This backup is for' })
  assert.equal(await forBackup.getByRole('checkbox', { name: 'Other files' }).isChecked(), true, 'everything Keevoro looks for, unless chosen otherwise')
  await forBackup.scrollIntoViewIfNeeded()
  await shot('9v3-settings-backup-for-other-files')
  await page.getByRole('status').getByRole('button', { name: 'Go to your sources' }).click()
  await page.getByRole('heading', { name: 'Where are your photos?', level: 1 }).waitFor()
  await page.getByRole('button', { name: 'Look again for new photos' }).click()
  await page.getByRole('button', { name: 'See what was found' }).click()
  await page.getByRole('heading', { name: 'Which of these should we keep?', level: 1 }).waitFor()
  const otherFiles = page.getByRole('region', { name: 'Other files' })
  await otherFiles.waitFor()
  assert.deepEqual(await otherFiles.locator('.folder-words b').allInnerTexts(), ['Documents', 'Tax 2019', 'Holiday mix'], 'by folder, as they were arranged')
  assert.match(await otherFiles.innerText(), /^Other files\s+11 files in 3 folders, [\d.]+ MB, kept as you arranged them\./)
  assert.match(await text('.tally'), /of [\d,]+ photos, [\d,]+ videos and 11 other files found/)
  assert.match(await text('.next .say'), /^Keeping [\d,]+ files in \d+ collections and 3 folders of other files\.$/)
  await otherFiles.scrollIntoViewIfNeeded()
  await shot('9w-choose-other-files')
  await fitsNarrow('Choose what to keep, with other files')
  // A whole folder is left out with the same switch as a collection.
  const music = otherFiles.locator('li', { hasText: 'Holiday mix' })
  await music.locator('.toggle').click()
  await music.locator('.toggle[aria-pressed="false"]').waitFor()
  await music.locator('.toggle').click()
  // A folder opened: a plain list, a tick each, and a file's details.
  await otherFiles.getByRole('button', { name: 'Open Tax 2019' }).click()
  await page.getByRole('heading', { name: 'Tax 2019', level: 1 }).waitFor()
  const fileRows = page.getByRole('table', { name: 'Files in Tax 2019' }).locator('tbody tr')
  assert.equal(await fileRows.count(), 3)
  assert.match(await fileRows.first().innerText(), /Return\.pdf\s+PDF file\s+[\d.]+ KB\s+3 March 2019/)
  assert.equal(await page.locator('.other-files-open img').count(), 0, 'no picture is ever attempted')
  await page.getByRole('switch', { name: 'Keep P60.pdf' }).click()
  assert.equal(await page.getByRole('switch', { name: 'Keep P60.pdf' }).getAttribute('aria-checked'), 'false')
  await page.getByRole('button', { name: 'Return.pdf' }).click()
  const fileDetails = page.getByRole('complementary', { name: 'About Return.pdf' })
  await fileDetails.getByRole('button', { name: /^Show in (File Explorer|Finder|Files)$/ }).waitFor()
  assert.match(await fileDetails.innerText(), /Kind\s+PDF file/)
  await shot('9w2-other-files-folder')
  await fitsNarrow('A folder of other files')
  await page.getByRole('button', { name: '‹ Back' }).click()
  await otherFiles.waitFor()
  // The map: other files to carry along are a third kind wanted, with a
  // pattern and a key entry of their own, light and dark.
  await page.locator('.steps button').nth(0).click()
  await page.getByRole('button', { name: /Windows \(C:\)/ }).click()
  await page.getByRole('button', { name: 'Find the folders with photos' }).click()
  await page.getByRole('heading', { name: '6 folders on Windows (C:) seem to hold photos, videos and other files' }).waitFor()
  assert.match(await page.locator('.hints li', { hasText: 'Documents' }).innerText(), /4,120 other files/)
  await shot('9w3-survey-other-files')
  await page.getByRole('button', { name: 'See where the space goes' }).click()
  await page.getByRole('heading', { name: 'Where the space goes on Windows (C:)', level: 1 }).waitFor()
  await page.locator('.space-list').getByRole('button', { name: 'Users', exact: true }).click()
  await page.locator('.space-list').getByRole('button', { name: 'Mark', exact: true }).click()
  await page.getByRole('grid', { name: 'What is in Mark, the biggest first' }).waitFor()
  await page.getByRole('list', { name: 'Key' }).getByText('Other files to carry along').waitFor()
  assert.match(await text('.space .status'), /other files to carry along [\d.]+ GB/)
  await page.getByRole('tab', { name: 'By kind of file' }).click()
  const kindsHere = page.getByRole('table', { name: 'What is in Mark, by kind of file' })
  assert.deepEqual(await kindsHere.locator('tr.kind-group button[aria-pressed]').allInnerTexts(), ['Photos', 'Videos', 'Other files to carry along', 'Everything else'])
  await page.getByRole('tab', { name: 'By folder' }).click()
  await shot('9w4-space-map-other-files')
  await page.emulateMedia({ colorScheme: 'dark' })
  await shot('9w4-space-map-other-files-dark')
  await page.emulateMedia({ colorScheme: 'light' })
  await fitsNarrow('Where the space goes, with other files')

  // --- A disk without room is refused before anything is sent.
  await page.goto(`${base}/?fast&full`)
  await page.getByRole('button', { name: 'Choose where they go' }).click()
  await page.getByRole('button', { name: /^A folder/ }).click()
  await page.getByText('There isn’t enough room there.').waitFor()
  assert.match(await text('.send .needs.short'), /need about [\d.]+ (MB|GB) and that disk has only 90\.0 MB free/)
  assert.ok(await page.getByRole('button', { name: 'Back up now' }).isDisabled(), 'sending waits until there is room')
  await shot('9g-not-enough-room')

  // --- A backup deleted from its folder (the first outside tester's
  // report) is said to be gone before anything is sent, never "already
  // there", and backing up again sends it all and says so.
  await page.goto(`${base}/?fast`)
  await page.getByRole('button', { name: 'Choose where they go' }).click()
  await page.getByRole('button', { name: /^A folder/ }).click()
  await page.getByRole('button', { name: 'Back up now' }).click()
  await page.getByText(/^Everything you are keeping is there: [\d,]+ files\.$/).waitFor()
  await page.evaluate(() => window.keevoroEmptyTheFolder())
  // Keevoro looks again when the backup screen is opened.
  await page.locator('.steps button').nth(1).click()
  await page.getByRole('button', { name: 'Next' }).click()
  const found = page.getByRole('status', { name: 'What Keevoro found where this backup goes' })
  await found
    .getByText(/^The folder F:\\Family Backup looks emptied or replaced: it no longer holds any of the [\d,]+ files Keevoro backed up there\. They will be sent again\.$/)
    .waitFor()
  await page.getByText(/^[\d,]+ files are waiting to be backed up\.$/).waitFor()
  assert.equal(await page.getByText(/\d already there/).count(), 0, 'nothing gone is said to be there')
  await shot('9x-backup-folder-emptied')
  await page.getByRole('button', { name: 'Back up now' }).click()
  await page.getByText(/^Restored [\d,]+ files that had gone from there\.$/).waitFor()
  await page.getByText(/^Everything you are keeping is there: [\d,]+ files\.$/).waitFor()
  assert.equal(await found.count(), 0, 'once sent again there is nothing more to say')
  // Unplugged, it is out of reach, never empty, and what is there is not said as certain.
  await page.evaluate(() => window.keevoroUnplugTheFolder())
  await page.locator('.steps button').nth(1).click()
  await page.getByRole('button', { name: 'Next' }).click()
  await found
    .getByText(/^Keevoro can't find the folder F:\\Family Backup\. Is it plugged in\? Until Keevoro can reach the folder F:\\Family Backup, what is there can't be confirmed: everything is counted as it was when Keevoro last reached it, 2 days ago\.$/)
    .waitFor()
  await page.getByText(/^Everything you are keeping was there when Keevoro last looked, 2 days ago: [\d,]+ files\.$/).waitFor()
  await shot('9y-backup-folder-unplugged')

  // --- A search can be stopped, and then nothing from it is kept.
  await page.goto(`${base}/?demo=empty&slow`)
  await page.getByRole('heading', { name: 'Where are your photos?', level: 1 }).waitFor()
  await page.getByRole('button', { name: /KINGSTON/ }).click()
  await page.getByRole('button', { name: 'Stop looking' }).click()
  await page.getByRole('heading', { name: 'Stopped looking through KINGSTON (E:)' }).waitFor()
  assert.match(await text('.panel'), /Nothing from it was added/)
  assert.equal(await page.locator('.kept li.source').count(), 0, 'the source was not added')
  assert.ok(await page.locator('.steps button').nth(1).isDisabled(), 'and there is nothing to choose from')
  await shot('9j-search-stopped')

  // --- The check for duplicates that follows a stopped search says what it
  // is doing, has its own Pause and Stop, and offers to be stopped too.
  await page.goto(`${base}/?slow&slowcheck`)
  await page.getByRole('button', { name: /^Find your photos/ }).click()
  await page.getByRole('heading', { name: 'Where are your photos?', level: 1 }).waitFor()
  await page.getByRole('button', { name: /KINGSTON/ }).click()
  await page.getByRole('button', { name: 'Stop looking' }).click()
  await page.getByRole('heading', { name: 'Stopped looking through KINGSTON (E:)' }).waitFor()
  await page.getByText('Still checking what was found before for duplicates.').waitFor()
  assert.match(
    await text('.foot .checking'),
    /^Checking for duplicates in the background \([\d,]+ of [\d,]+\)\. You can keep using Keevoro while it finishes\./,
  )
  await shot('9j2-search-stopped-still-checking')
  await page.getByRole('button', { name: 'Pause checking for duplicates' }).click()
  await page.locator('.foot .checking').getByText(/^Checking for duplicates is paused/).waitFor()
  assert.equal(await page.locator('.foot .paused').count(), 0, 'only the check is paused, not Keevoro')
  await shot('9j3-check-paused-alone')
  await page.getByRole('button', { name: 'Carry on checking for duplicates' }).click()
  await page.getByRole('button', { name: 'Stop that too' }).click()
  await page.locator('.foot .check-stopped').waitFor()
  assert.match(await text('.foot .check-stopped'), /^Stopped checking for duplicates, with [\d,]+ files not checked yet\./)
  assert.equal(await page.getByText('Still checking what was found before for duplicates.').count(), 0)
  await shot('9j4-check-stopped')
  // What that means, and when it starts again, on the activity page.
  await page.getByRole('button', { name: /^What Keevoro is doing/ }).click()
  await page.locator('.check-here .check-stopped').waitFor()
  assert.match(await text('.check-here'), /nothing is backed up twice or left out by mistake/)
  assert.match(await text('.check-here'), /Checking starts again by itself after your next search or backup/)
  await shot('9j5-check-stopped-activity')
  await fitsNarrow('The check stopped, on the activity page')
  await page.locator('.check-here').getByRole('button', { name: 'Check for duplicates now' }).click()
  await page.locator('.check-here .checking').waitFor()

  // --- Coming back to a backup that is already running shows it running,
  // never an offer to start another.
  // The last backup was stopped part-way, and what it said then is kept:
  // it must never show beside the one running now.
  await page.goto(`${base}/?sending&stoppedBefore`)
  await page.getByRole('button', { name: 'Choose where they go' }).click()
  await page.getByRole('button', { name: /^A folder/ }).click()
  await page.getByRole('button', { name: 'Stop', exact: true }).waitFor()
  assert.equal(await page.getByRole('button', { name: 'Back up now' }).count(), 0, 'no second start is offered')
  await page.locator('.speed').waitFor()
  // How long it has been running comes from the backup itself, not from
  // when this screen opened, beside the time left for the whole of it.
  await page.getByText(/^Running for 2 minutes, about \d+ minutes left$/).waitFor()
  // Every stage is listed from the start, previews included.
  assert.deepEqual(
    (await page.locator('.speed .stages li').allInnerTexts()).map((t) => t.replace(/^(Done|Now|Then):\s*/, '')),
    [
      'Working out where each file goes: 2,500 of 11,245',
      'Backing up your files: 30.1 GB',
      'Making previews: 10,547',
      'Backing up the previews: 4.0 GB',
      'Writing the list of what is there',
    ],
  )
  assert.match(await text('.speed'), /switch off “Also make previews for a gallery” once this backup\s+ends/)
  assert.equal(await page.getByText(/^Stopped\./).count(), 0, 'no stale "Stopped" while it runs')
  // The screen says what step the backup is on straight away, in its own
  // words, and the activity page says the same.
  await page.locator('.send .running').getByText('Working out where each file goes: 2,500 of 11,245').waitFor()
  assert.equal(await page.locator('.send .done').count(), 0, 'nothing an earlier backup said is left showing')
  // There is always a bar while a backup runs, whatever step it is on.
  await page.locator('.send .speed progress').first().waitFor()
  await shot('9m-came-back-to-a-running-backup')
  await page.getByRole('button', { name: /^What Keevoro is doing/ }).click()
  await page.getByText(/^Right now: Working out where each file goes: 2,500 of 11,245$/).waitFor()
  await page.getByRole('button', { name: '‹ Back' }).click()
  // Coming back again, it is still running, still counting from when it
  // began, and still nothing stale is shown.
  await page.getByText(/^Running for 2 minutes, about \d+ minutes left$/).waitFor()
  assert.equal(await page.getByText(/^Stopped\./).count(), 0, 'still no stale "Stopped" after looking away and back')
  assert.equal(await page.locator('.send .done').count(), 0)

  // --- Pictures that have not arrived yet show a shimmering placeholder.
  await page.goto(`${base}/?slowpictures`)
  await page.locator('.picture.loading').first().waitFor()
  assert.equal(await page.locator('.picture.loading').first().getAttribute('aria-busy'), 'true')
  await shot('9p-pictures-on-their-way')

  // --- A send can be stopped; what was sent stays sent.
  await page.goto(`${base}/?slow`)
  await page.getByRole('button', { name: 'Choose where they go' }).click()
  await page.getByRole('button', { name: /^A folder/ }).click()
  await page.getByRole('button', { name: 'Back up now' }).click()
  // While it sends, it says how much, how fast and how long is left.
  await page.locator('.speed').waitFor()
  // One line for the whole backup: how long it has run and how long is
  // left, then each stage, the one it is on and the previews alongside.
  await page.getByText(/^Running for (less than a minute|\d+ minutes?), about \d+ minutes left$/).waitFor()
  await page.getByText(/Making previews in the background: [\d,]+ of 10,547$/).waitFor()
  assert.match(await text('.speed .stages li.is-now'), /^Now:\s*Backing up your files: [\d.]+ GB of 30\.1 GB at 12\.4 MB\/s$/)
  assert.equal(await page.locator('.speed .stages li.is-next').count(), 2, 'what is still to come is shown')
  // And the file just dealt with is named beneath.
  await page.getByText(/^Just done: originals\/Holidays\/Cornwall\/IMG_\d+\.jpg$/).waitFor()
  await shot('9l-send-speed')
  // Counts from before the backup began are not left on show while it runs.
  assert.equal(await page.getByText(/waiting to be backed up/).count(), 0, 'no stale count of what is waiting')
  assert.equal(await page.locator('.send .needs').count(), 0, 'no stale room needed')
  // The bar keeps one length whatever the words under it say.
  const barWidth = () => page.locator('.speed progress').first().evaluate((bar) => bar.getBoundingClientRect().width)
  const wide = await barWidth()
  await page.locator('.speed p').first().evaluate((p) => (p.textContent = '1 GB'))
  assert.equal(await barWidth(), wide, 'the progress bar does not shrink to its words')
  await page.getByRole('button', { name: 'Stop', exact: true }).click()
  await page.getByText('Stopping after this file…').waitFor()
  await page.getByText(/^Stopped\. Everything backed up so far is safe where it is\./).waitFor()
  await shot('9h-stopped')

  // --- What Keevoro is doing: what, why, how much, and a way to make it wait.
  await page.goto(`${base}/?fast`)
  await page.getByRole('button', { name: 'What Keevoro is doing' }).click()
  await page.getByRole('heading', { name: 'What Keevoro is doing', level: 1 }).waitFor()
  await page.getByRole('heading', { name: 'Checking for duplicates' }).waitFor()
  // What it is doing is said in plain words, at a glance: in the heading
  // line, as a heading per job, and in the side bar on every screen.
  assert.match(await text('.activity .status'), /^Checking for duplicates and making thumbnails\. Here is why/)
  assert.deepEqual(await page.locator('.activity .jobs h2').allInnerTexts().then((l) => l.map((t) => t.trim())), ['Making thumbnails', 'Checking for duplicates'])
  assert.match(await text('.sidebar .link.doing'), /Checking for duplicates and making thumbnails/)
  assert.match(await page.locator('.activity .jobs').innerText(), /so that true copies can be told from photos that merely look alike/)
  assert.deepEqual(await page.locator('.chart figcaption b').allInnerTexts(), ['Processor', 'Disk', 'Network', 'Memory'])
  assert.match(await page.locator('.chart .now').first().innerText(), /^[\d.]+%$/)
  assert.match(await page.locator('.chart .now').nth(1).innerText(), /^[\d.]+ MB\/s$/)
  assert.equal(await page.locator('.chart .line').count(), 4, 'one line in each chart, never two measures on one')
  // Moving across a chart reads out the figure at that moment.
  const plot = await page.locator('.chart svg').first().boundingBox()
  await page.mouse.move(plot.x + plot.width * 0.9, plot.y + plot.height / 2)
  assert.match(await page.locator('.chart .read').first().innerText(), /^[\d.]+%, (\d+ seconds ago|just now)$/)
  await page.mouse.move(5, 5)
  await shot('9k-activity')
  // Pausing is one button, and the side bar keeps saying so wherever you go.
  await page.getByRole('button', { name: 'Pause Keevoro’s work' }).click()
  await page.getByText('Paused. Keevoro is waiting and using almost nothing.').waitFor()
  assert.match(await text('.foot .paused'), /Keevoro is paused/)
  await page.getByRole('button', { name: '‹ Back' }).click()
  assert.match(await text('.foot .paused'), /Keevoro is paused/, 'still said after leaving the page')
  await page.locator('.foot .paused').getByRole('button', { name: 'Carry on' }).click()
  await page.locator('.foot .paused').waitFor({ state: 'detached' })

  // --- Managing backups, as the first tester asked: each with what tells it
  // apart, renamed in place, removed only once told exactly what that does,
  // and put back.
  await page.goto(`${base}/?fast`)
  for (const name of ['Test 2', 'My photos', 'My photos']) {
    await page.getByRole('button', { name: 'Backup', exact: true }).click()
    await page.getByRole('button', { name: 'Start a new backup…' }).click()
    await page.getByLabel('Name for the new backup').fill(name)
    await page.getByRole('button', { name: 'Start', exact: true }).click()
    await page.getByRole('heading', { name: 'Where are your photos?', level: 1 }).waitFor()
  }
  await page.getByRole('button', { name: 'Backup', exact: true }).click()
  assert.deepEqual(await page.locator('.backup .menu > button').allInnerTexts(), [
    'My photos',
    'Test 2',
    'My photos 2',
    'Start a new backup…',
    'Manage backups…',
  ])
  await shot('9ab-switcher-with-manage')
  await page.getByRole('button', { name: 'Manage backups…' }).click()
  await page.getByRole('heading', { name: 'Your backups', level: 1 }).waitFor()
  const backupRow = (name) =>
    page.locator('.manage-backups > .backup-list > .backup-row').filter({ has: page.getByRole('heading', { name: new RegExp(`^${name}(\\s*Open now)?$`) }) })
  // Each is filled in as it is counted: where it looks, where it goes, how much, when it last ran.
  await page.waitForFunction(() => document.querySelectorAll('.backup-row .facts').length === 4)
  assert.match(await backupRow('My photos').innerText(), /Looks in\s+[A-Z]:\\/)
  assert.match(await backupRow('Test 2').innerText(), /Looks in\s+No sources yet\s+Backs up to\s+Nowhere yet\s+Holds\s+Nothing found yet\s+Last backed up\s+Never/)
  assert.match(await backupRow('My photos 3').innerText(), /Open now/)
  assert.equal(await backupRow('My photos 3').getByRole('button', { name: 'Open' }).count(), 0, 'the open one is not offered to open')
  await shot('9ac-manage-backups')
  await fitsNarrow('Manage backups')
  // Renamed in place; a name in use is refused, in words, and changes nothing.
  await backupRow('Test 2').getByRole('button', { name: 'Rename' }).click()
  await page.getByLabel('New name for Test 2').fill('my photos')
  await page.getByRole('button', { name: 'Save' }).click()
  await page.getByRole('alert').filter({ hasText: 'There is already a backup called "my photos". Choose another name.' }).waitFor()
  await shot('9ad-rename-refused')
  await page.getByLabel('New name for Test 2').fill('Trying it out')
  await page.getByRole('button', { name: 'Save' }).click()
  await backupRow('Trying it out').waitFor()
  // Removing says exactly what it does first, in the page, then asks.
  await backupRow('Trying it out').getByRole('button', { name: 'Remove…' }).click()
  await page.getByRole('heading', { name: 'Remove “Trying it out” from Keevoro?' }).waitFor()
  const removing = await text('.removing')
  assert.match(removing, /Keevoro's record of “Trying it out” on this computer goes/)
  assert.match(removing, /there is nothing anywhere else to keep/)
  assert.match(removing, /Nothing on any source is touched\./)
  assert.match(removing, /You can put it back from Manage backups for 7 days/)
  await shot('9ae-remove-backup')
  await fitsNarrow('Removing a backup')
  await page.getByRole('button', { name: 'Cancel' }).click()
  await backupRow('Trying it out').getByRole('button', { name: 'Remove…' }).click()
  await page.getByRole('button', { name: 'Remove this backup from Keevoro' }).click()
  await backupRow('Trying it out').waitFor({ state: 'detached' })
  assert.match(await text('.done-said'), /^Removed “Trying it out” from Keevoro/)
  await page.getByRole('heading', { name: 'Removed recently' }).waitFor()
  assert.match(await text('.removed-recently'), /It can be put back until/)
  await shot('9af-removed-recently')
  await page.locator('.removed-recently').getByRole('button', { name: 'Put back' }).click()
  await backupRow('Trying it out').waitFor()
  assert.equal(await page.locator('.removed-recently').count(), 0, 'nothing left to put back')
  // The open backup can be removed too: another opens in its place.
  await backupRow('My photos 3').getByRole('button', { name: 'Remove…' }).click()
  await page.getByRole('button', { name: 'Remove this backup from Keevoro' }).click()
  await backupRow('My photos 3').waitFor({ state: 'detached' })
  assert.match(await text('.backup .current'), /My photos/)
  assert.doesNotMatch(await text('.backup .current'), /My photos 3/)
  // And another is opened from the list.
  await backupRow('My photos 2').getByRole('button', { name: 'Open' }).click()
  await page.getByRole('heading', { name: 'Where are your photos?', level: 1 }).waitFor()
  assert.match(await text('.backup .current'), /My photos 2/)

  // --- Starting again: asked first, told exactly what goes and what stays.
  await page.goto(`${base}/?fast`)
  await page.getByRole('button', { name: 'Settings' }).click()
  await page.getByRole('button', { name: 'Reset Keevoro…' }).click()
  assert.match(await text('.resetting'), /This removes:.*your backup.*This does not touch:.*anything on your sources.*already sent/s)
  await shot('9i-reset')
  await page.getByRole('button', { name: 'Cancel' }).click()
  assert.equal(await page.locator('.resetting').count(), 0, 'cancelling changes nothing')
  assert.match(await text('.backup .current'), /My photos/)
  await page.getByRole('button', { name: 'Reset Keevoro…' }).click()
  await page.getByRole('button', { name: 'Yes, reset Keevoro' }).click()
  await page.getByRole('heading', { name: 'Let’s find your photos' }).waitFor()
  assert.equal(await page.getByLabel('What shall we call this backup?').inputValue(), 'My photos', 'as on first install')

  // --- A newer version is offered calmly and can be installed in one click.
  await page.goto(`${base}/?fast&update`)
  assert.match(await text('.update'), /A new version of Keevoro is ready \(9\.9\.0\)/)
  await shot('9c-update-ready')
  await page.getByRole('button', { name: 'Update now' }).click()
  await page.getByText(/has been downloaded and opened/).waitFor()
  await page.goto(`${base}/?fast&update`)
  await page.getByRole('button', { name: 'Later' }).click()
  assert.equal(await page.locator('.update').count(), 0, 'Later puts the offer away')

  // --- A search does not stop you doing other things, such as removing a source.
  await page.goto(`${base}/`)
  await page.getByRole('button', { name: /Find your photos/ }).click()
  await page.getByRole('button', { name: /KINGSTON/ }).click()
  await page.locator('.searching').waitFor()
  const remove = page.getByRole('button', { name: /^Remove / }).first()
  assert.ok(await remove.isEnabled(), 'Remove works while a search is running')
  await remove.click()
  await page.getByRole('button', { name: 'Remove from Keevoro' }).click()
  await page.locator('.kept li.source').waitFor({ state: 'detached' })
  assert.ok(await page.locator('.searching').isVisible(), 'and the search carries on')

  // --- Look inside a folder the survey suggests, before choosing it. Only
  // what is near the screen is fetched, pictures shimmer until they come,
  // and nothing is added until the person says so.
  await page.goto(`${base}/?demo=empty&fast&slowpictures`)
  await page.getByRole('button', { name: /Windows \(C:\)/ }).click()
  await page.getByRole('button', { name: 'Find the folders with photos' }).click()
  await page.getByRole('heading', { name: '4 folders on Windows (C:) seem to hold photos and videos' }).waitFor()
  await page.locator('.survey .hints input').nth(2).uncheck()
  await page.getByRole('button', { name: 'Look inside C:\\Users\\Mark\\Pictures' }).click()
  await page.getByRole('heading', { name: 'Inside Pictures', level: 1 }).waitFor()
  assert.match(await text('.inside .status'), /^1,630 photos and 82 videos, [\d.]+ GB\. Nothing here is added or changed until you choose\.$/)
  assert.match(await text('.inside header .online-only'), /^32 files are in OneDrive only, not on this computer, so Keevoro does not open them/)
  await page.locator('.inside .tile .picture.loading').first().waitFor()
  assert.ok((await page.locator('.inside .tile .picture.loading').count()) > 0, 'a shimmer before the pictures arrive')
  await shot('9q3-look-inside-waiting')
  await page.locator('.inside .tile img').first().waitFor({ timeout: 15000 })
  const tilesShown = await page.locator('.inside .tile').count()
  assert.ok(tilesShown > 0 && tilesShown < 1_712, `only the tiles near the screen are in the page (${tilesShown} of 1,712)`)
  assert.deepEqual(
    await page.locator('.inside .tiles-heading b').allInnerTexts(),
    ['Pictures', 'Camera Roll', 'Captures', '2019 › Cornwall', 'Screenshots'].slice(0, await page.locator('.inside .tiles-heading').count()),
    'the person’s own folders, in order, as sections',
  )
  // Scrolled into the camera roll: its tiles come, and those passed by are dropped.
  await page.getByRole('button', { name: 'Look at IMG_0013.jpg' }).first().scrollIntoViewIfNeeded()
  await page.getByRole('button', { name: 'Look at IMG_0013.jpg' }).first().getByText('Can’t be shown').waitFor({ timeout: 15000 })
  await page.locator('main').evaluate((main) => main.scrollBy(0, 1200))
  await page.getByRole('button', { name: 'Look at IMG_0050.jpg' }).first().waitFor()
  assert.equal(await page.getByRole('button', { name: 'Look at IMG_0050.jpg' }).first().innerText(), 'In OneDrive only, not on this computer')
  await page.getByRole('button', { name: 'Look at IMG_0050.jpg' }).first().scrollIntoViewIfNeeded()
  const made = await page.evaluate(() => window.keevoroPicturesMade())
  assert.ok(made < 400, `pictures are made only for what was near the screen (${made})`)
  await shot('9q4-look-inside-scrolled')
  // A closer look, with the way to look through the folder beside it.
  await page.getByRole('button', { name: 'Look at IMG_0048.jpg' }).first().click()
  const closer = page.getByRole('dialog', { name: 'IMG_0048.jpg' })
  await closer.locator('img').waitFor()
  assert.ok(await closer.getByRole('button', { name: 'Look through this folder' }).isVisible())
  await page.keyboard.press('ArrowRight')
  await page.getByRole('dialog', { name: 'IMG_0049.jpg' }).waitFor()
  await page.keyboard.press('ArrowRight')
  // One only in OneDrive is never opened, there either.
  await page.getByRole('dialog', { name: 'IMG_0050.jpg' }).getByText('In OneDrive only, not on this computer').waitFor()
  await shot('9q5-look-inside-closer')
  await page.keyboard.press('Escape')
  // Back is the survey as it was, ticks and all.
  await page.getByRole('button', { name: '‹ Back' }).click()
  await page.getByRole('heading', { name: '4 folders on Windows (C:) seem to hold photos and videos' }).waitFor()
  assert.deepEqual(await page.locator('.survey .hints input').evaluateAll((boxes) => boxes.map((b) => b.checked)), [true, true, false, false])
  assert.equal(await page.locator('.kept li.source').count(), 0, 'looking inside added nothing')
  // As it looks once the pictures have come.
  await page.goto(`${base}/?demo=empty&fast`)
  await page.getByRole('button', { name: /Windows \(C:\)/ }).click()
  await page.getByRole('button', { name: 'Find the folders with photos' }).click()
  await page.getByRole('button', { name: 'Look inside C:\\Users\\Mark\\Pictures' }).click()
  await page.getByRole('heading', { name: 'Inside Pictures', level: 1 }).waitFor()
  await page.waitForFunction(() => document.querySelectorAll('.inside .tile img').length >= 12)
  await shot('9q4-look-inside')
  await fitsNarrow('Looking inside')
  // Only the videos, newest first by the file's date, each with its length.
  await page.getByRole('radiogroup', { name: 'Show' }).getByRole('radio', { name: 'Videos' }).click()
  await page.getByLabel('Sort by').selectOption({ label: 'Newest first (by file date)' })
  assert.match(await text('.inside header'), /not always when it was taken/)
  await page.getByRole('button', { name: 'Look at VID_0037.mp4' }).first().waitFor()
  assert.deepEqual(
    await page.locator('.inside .tiles-heading b').allInnerTexts(),
    ['Camera Roll', 'Captures', '2019 › Cornwall'],
    'folders with no videos are not shown',
  )
  assert.equal(await page.locator('.inside .tile .look').first().getAttribute('aria-label'), 'Look at VID_0037.mp4', 'the newest first')
  await page.locator('.inside .tile img').first().waitFor()
  assert.equal(await page.getByRole('button', { name: 'Look at VID_0001.mp4' }).first().locator('.length').innerText(), '1:42')
  assert.equal(await page.getByRole('button', { name: 'Look at VID_0005.mp4' }).first().locator('.length').count(), 0, 'one that does not say shows none')
  await shot('9q6-look-inside-videos')
  await page.getByRole('button', { name: 'Look at VID_0001.mp4' }).first().click()
  await page.getByRole('dialog', { name: 'VID_0001.mp4' }).getByText('Video, 1:42 long: this is a still from it').waitFor()
  await page.keyboard.press('Escape')
  await fitsNarrow('Looking inside, videos only')
  await page.getByRole('button', { name: '‹ Back' }).click()
  // And from inside, one button looks through it properly.
  await page.getByRole('button', { name: 'Look inside C:\\Users\\Mark\\Pictures' }).click()
  await page.getByRole('heading', { name: 'Inside Pictures', level: 1 }).waitFor()
  await page.locator('.inside header').getByRole('button', { name: 'Look through this folder' }).click()
  await page.locator('.kept li.source').first().waitFor()
  assert.match(await text('.kept li.source .what .folder'), /^C:\\Users\\Mark\\Pictures$/)

  // --- Any folder can be looked inside: a source, surveyed first when
  // Keevoro has not seen inside it, and one picked from the first step.
  await page.goto(`${base}/?demo=empty&fast`)
  await page.getByRole('button', { name: 'Look inside one first' }).click()
  await page.getByRole('heading', { name: 'Inside Photos', level: 1 }).waitFor()
  await page.locator('.inside .tile').first().waitFor()
  assert.equal(await page.locator('.kept li.source').count(), 0, 'looking inside added nothing')
  await page.getByRole('button', { name: '‹ Back' }).click()
  await page.getByRole('heading', { name: 'Where are your photos?', level: 1 }).waitFor()
  await shot('9q7-look-inside-first')
  await page.goto(`${base}/?slow`)
  await page.locator('.steps button').nth(0).click()
  await page.getByRole('button', { name: 'Look inside Family hard drive' }).click()
  await page.getByRole('heading', { name: 'Finding what is in Family hard drive…' }).waitFor()
  assert.ok(await page.getByRole('button', { name: 'Stop and show what was found' }).isVisible())
  await shot('9q8-look-inside-a-source-surveying')
  await page.locator('.inside .tile').first().waitFor({ timeout: 15000 })
  assert.ok(await page.locator('.inside header').getByRole('button', { name: 'Look through it again' }).isVisible())
  await shot('9q9-look-inside-a-source')
  // A source can be left out from its look inside, and put back from its card.
  await page.locator('.inside header').getByRole('button', { name: 'Leave out E:' }).click()
  await page.locator('.inside .left-out-here').waitFor()
  assert.match(await text('.inside .left-out-said'), /nothing already backed up is touched/)
  await page.getByRole('button', { name: '‹ Back' }).click()
  const card = page.locator('.kept li.source').first()
  await card.locator('.left-out-by-you').waitFor()
  assert.match(await card.locator('.left-out-by-you').innerText(), /Left out of this backup:\s*E:/)
  await shot('9r3-source-left-out')
  await fitsNarrow('A source with a folder left out')
  await card.getByRole('button', { name: 'Put back E:' }).click()
  await card.locator('.left-out-by-you .left-out-said').waitFor()
  await card.locator('.left-out-by-you li').waitFor({ state: 'detached' })

  // --- Leave a folder out from the survey's suggestions, and from the
  // look inside, saying at the moment of the choice that nothing already
  // backed up is touched; Put back undoes it.
  await page.goto(`${base}/?demo=empty&fast`)
  await page.getByRole('button', { name: /Windows \(C:\)/ }).click()
  await page.getByRole('button', { name: 'Find the folders with photos' }).click()
  await page.getByRole('heading', { name: '4 folders on Windows (C:) seem to hold photos and videos' }).waitFor()
  await page.getByRole('button', { name: 'Leave out assets' }).click()
  await page.getByRole('button', { name: 'Put back assets' }).waitFor()
  assert.match(await text('.survey .left-out-said'), /^Left out C:\\Games\\Launcher\\assets\. .*nothing already backed up is touched\.$/)
  assert.equal(await page.locator('.survey .hints input').nth(3).isDisabled(), true, 'it can no longer be ticked')
  await shot('9r-survey-left-out')
  await fitsNarrow('The survey with a folder left out')
  await page.getByRole('button', { name: 'Put back assets' }).click()
  await page.getByRole('button', { name: 'Leave out assets' }).waitFor()
  await page.getByRole('button', { name: 'Look inside C:\\Users\\Mark\\Pictures' }).click()
  await page.getByRole('heading', { name: 'Inside Pictures', level: 1 }).waitFor()
  await page.getByRole('button', { name: 'Leave out Captures' }).click()
  await page.getByRole('button', { name: 'Put back Captures' }).waitFor()
  await page.locator('.inside .tiles-heading .badge', { hasText: 'Left out' }).waitFor()
  await page.locator('.inside header').getByRole('button', { name: 'Leave out Pictures' }).click()
  await page.locator('.inside .left-out-here').waitFor()
  await page.locator('.inside .tile .out-mark').first().waitFor()
  assert.equal(await page.locator('.inside .tile .out-mark').first().getAttribute('title'), 'Left out: you left this folder out of this backup')
  await shot('9r2-look-inside-left-out')
  await fitsNarrow('Looking inside a folder left out')
  await page.locator('.inside .left-out-here').getByRole('button', { name: 'Put back Pictures' }).click()
  await page.locator('.inside .left-out-here').waitFor({ state: 'detached' })
  await page.getByRole('button', { name: 'Put back Captures' }).click()
  await page.getByRole('button', { name: 'Leave out Captures' }).waitFor()

  // --- Where the space goes: everything the survey saw, photos or not, by
  // folder, the biggest first, with the trail back up. Nothing is changed.
  await page.goto(`${base}/?demo=empty&fast`)
  await page.getByRole('button', { name: /Windows \(C:\)/ }).click()
  await page.getByRole('button', { name: 'Find the folders with photos' }).click()
  await page.getByRole('heading', { name: '4 folders on Windows (C:) seem to hold photos and videos' }).waitFor()
  await page.getByRole('button', { name: 'See where the space goes' }).click()
  await page.getByRole('heading', { name: 'Where the space goes on Windows (C:)', level: 1 }).waitFor()
  const spaceRows = () => page.locator('.space-list tbody tr .name > :first-child').allInnerTexts()
  assert.deepEqual((await spaceRows()).slice(0, 3), ['Users', 'Games', 'Windows'], 'the biggest first')
  const windowsRow = page.locator('.space-list tbody tr', { hasText: 'Windows' }).first()
  assert.match(await windowsRow.innerText(), /System folder: Keevoro never looks through it/)
  assert.match(await windowsRow.getAttribute('class'), /muted/, 'not photos or videos, so muted')
  await page.locator('.space-list').getByRole('button', { name: 'Users', exact: true }).click()
  await page.getByRole('navigation', { name: 'Where you are' }).getByText('Users').waitFor()
  await page.locator('.space-list').getByRole('button', { name: 'Mark', exact: true }).click()
  await page.locator('.space-list').getByRole('button', { name: 'Pictures', exact: true }).click()
  await page.getByRole('grid', { name: 'What is in Pictures, the biggest first' }).waitFor()
  assert.deepEqual(
    await page.getByRole('navigation', { name: 'Where you are' }).locator('li').allInnerTexts(),
    ['Windows (C:)', 'Users', 'Mark', 'Pictures'],
    'the trail back up',
  )
  assert.deepEqual((await spaceRows()).slice(0, 4), ['Texture packs', 'Keevoro Library', 'Captures', 'Camera Roll'])
  const library = page.locator('.space-list tbody tr', { hasText: 'Keevoro Library' })
  assert.match(await library.innerText(), /A library from an older Keevoro: Keevoro never looks through it/)
  await shot('9s-space-list')
  await page.emulateMedia({ colorScheme: 'dark' })
  await shot('9s2-space-list-dark')
  await page.emulateMedia({ colorScheme: 'light' })
  // The map beside the list: pointing at a box says what it is and lights
  // its row, pointing at a row outlines its box, and choosing either
  // chooses both.
  const map = page.locator('.map-canvas')
  const mapBox = await map.locator('canvas.over').boundingBox()
  await page.mouse.move(mapBox.x + 30, mapBox.y + 8)
  await page.locator('.map-tip:not([hidden])').waitFor()
  assert.match(await text('.map-tip'), /^Texture packs: 17\.7 GB, 160 other files$/)
  await page.locator('.space-list tr.lit', { hasText: 'Texture packs' }).waitFor()
  assert.equal(await map.getAttribute('data-pointed'), 'C:\\Users\\Mark\\Pictures\\Texture packs')
  await page.locator('.space-list tbody tr', { hasText: 'Captures' }).hover()
  await page.waitForFunction(() => document.querySelector('.map-canvas')?.dataset.lit === 'C:\\Users\\Mark\\Pictures\\Captures')
  await page.locator('.space-list tbody tr', { hasText: 'Screenshots' }).click()
  assert.equal(await map.getAttribute('data-selected'), 'C:\\Users\\Mark\\Pictures\\Screenshots', 'the row chosen lights its box')
  await page.mouse.click(mapBox.x + 30, mapBox.y + 8)
  assert.equal(await map.getAttribute('data-selected'), 'C:\\Users\\Mark\\Pictures\\Texture packs', 'a box chosen on the map')
  assert.equal(await page.locator('.space-list tr[aria-selected="true"] .name > :first-child').innerText(), 'Texture packs', 'is chosen in the list')
  await page.locator('.space .chosen').getByRole('button', { name: 'Leave out Texture packs' }).waitFor()
  await shot('9t-space-map')
  await page.emulateMedia({ colorScheme: 'dark' })
  await shot('9t2-space-map-dark')
  await page.emulateMedia({ colorScheme: 'light' })
  // Clicking a chosen folder again goes into it, easing unless motion is unwelcome.
  await page.evaluate(() => {
    window.keevoroEased = false
    new MutationObserver(() => {
      if (document.querySelector('[data-easing]')) window.keevoroEased = true
    }).observe(document.body, { attributes: true, subtree: true, attributeFilter: ['data-easing'] })
  })
  await page.mouse.click(mapBox.x + 30, mapBox.y + 8)
  await page.getByRole('grid', { name: 'What is in Texture packs, the biggest first' }).waitFor()
  await settled()
  assert.equal(await page.evaluate(() => window.keevoroEased), true, 'the folder grew to fill the map')
  await page.getByRole('navigation', { name: 'Where you are' }).getByRole('button', { name: 'Pictures' }).click()
  await page.getByRole('grid', { name: 'What is in Pictures, the biggest first' }).waitFor()
  await page.emulateMedia({ reducedMotion: 'reduce' })
  await page.evaluate(() => (window.keevoroEased = false))
  await page.mouse.dblclick(mapBox.x + 30, mapBox.y + 8)
  await page.getByRole('grid', { name: 'What is in Texture packs, the biggest first' }).waitFor()
  await settled()
  assert.equal(await page.evaluate(() => window.keevoroEased), false, 'with reduced motion the new map simply appears')
  await page.getByRole('navigation', { name: 'Where you are' }).getByRole('button', { name: 'Pictures' }).click()
  await page.getByRole('grid', { name: 'What is in Pictures, the biggest first' }).waitFor()
  await page.emulateMedia({ reducedMotion: 'no-preference' })
  for (const [width, name] of [
    [700, '9t3-space-map-700'],
    [400, '9t5-space-map-400'],
  ]) {
    await page.setViewportSize({ width, height: 900 })
    await shot(name)
    await page.emulateMedia({ colorScheme: 'dark' })
    await shot(`${name}-dark`)
    await page.emulateMedia({ colorScheme: 'light' })
  }
  // In a very narrow window the map is a strip until asked for.
  await page.getByRole('button', { name: 'Show the map' }).click()
  await page.getByRole('button', { name: 'Show less of the map' }).waitFor()
  await fitsNarrow('Where the space goes, with the whole map')
  await page.setViewportSize({ width: 400, height: 900 })
  await shot('9t6-space-map-400-whole')
  await page.getByRole('button', { name: 'Show less of the map' }).click()
  await page.setViewportSize({ width: 1440, height: 900 })
  // A folder chosen: look inside it, look through it, or leave it out.
  await page.locator('.space-list tbody tr', { hasText: 'Camera Roll' }).click()
  const chosen = page.locator('.space .chosen')
  await chosen.getByRole('button', { name: 'Look inside Camera Roll' }).waitFor()
  assert.ok(await chosen.getByRole('button', { name: 'Look through Camera Roll' }).isVisible())
  assert.match(await chosen.innerText(), /307\.2 MB in OneDrive only, not on this computer/)
  // What is already a source, and how much of it is safely backed up, in
  // words beside the outline and tick on the map, and in the key.
  assert.match(await chosen.innerText(), /A source of this backup/)
  assert.match(await chosen.innerText(), /Backed up: 1,180 of 1,241/)
  assert.match(await page.locator('.space-list tbody tr', { hasText: '2019' }).innerText(), /A source of this backup\s+Backed up: all 124/)
  assert.match(await text('.space .key'), /A source of this backup\s+✓\s*All backed up/)
  // And it can be shown in the computer's own file manager.
  await chosen.getByRole('button', { name: /^Show Camera Roll in (File Explorer|Finder|Files)$/ }).click()
  await shot('9t8-space-sources-backed-up')
  await chosen.getByRole('button', { name: 'Leave out Camera Roll' }).click()
  await chosen.getByRole('button', { name: 'Put back Camera Roll' }).waitFor()
  assert.match(await text('.space .left-out-said'), /nothing already backed up is touched/)
  assert.match(
    await page.locator('.space-list tbody tr', { hasText: 'Camera Roll' }).innerText(),
    /Not backed up: you left this folder out of this backup/,
  )
  await chosen.getByRole('button', { name: 'Put back Camera Roll' }).click()
  await chosen.getByRole('button', { name: 'Leave out Camera Roll' }).waitFor()
  await chosen.getByRole('button', { name: 'Look inside Camera Roll' }).click()
  await page.getByRole('heading', { name: 'Inside Camera Roll', level: 1 }).waitFor()
  await page.getByRole('button', { name: '‹ Back' }).click()
  await page.getByRole('grid', { name: 'What is in Pictures, the biggest first' }).waitFor()
  // By keyboard alone: the arrows move down the list, Enter goes in, Backspace comes back.
  await page.locator('.space-list tbody tr').first().focus()
  await page.keyboard.press('ArrowDown')
  await page.keyboard.press('ArrowDown')
  assert.equal(await page.evaluate(() => document.activeElement?.dataset.key), 'C:\\Users\\Mark\\Pictures\\Captures')
  await page.keyboard.press('Enter')
  await page.getByRole('grid', { name: 'What is in Captures, the biggest first' }).waitFor()
  assert.equal(await page.getByLabel('Where you are').locator('[aria-current="page"]').innerText(), 'Captures')
  await page.locator('.space-list tbody tr').first().focus()
  await page.keyboard.press('Backspace')
  await page.getByRole('grid', { name: 'What is in Pictures, the biggest first' }).waitFor()
  // By kind of file: photos, videos and everything else, each opening to
  // its endings; picking one fades every box on the map without it.
  await page.getByRole('tab', { name: 'By kind of file' }).click()
  const byKindTable = page.getByRole('table', { name: 'What is in Pictures, by kind of file' })
  await byKindTable.waitFor()
  assert.deepEqual(await byKindTable.locator('tr.kind-group button[aria-pressed]').allInnerTexts(), ['Photos', 'Videos', 'Everything else'])
  assert.equal(await map.getAttribute('data-picked'), '', 'nothing picked, nothing faded')
  await byKindTable.getByRole('button', { name: 'Everything else', exact: true }).click()
  await page.waitForFunction(() => document.querySelector('.map-canvas')?.dataset.picked === 'other')
  assert.ok(Number(await map.getAttribute('data-faded')) > 0, 'boxes with no other files fade')
  await byKindTable.getByRole('button', { name: 'Show the endings of everything else' }).click()
  await byKindTable.getByRole('button', { name: 'zip', exact: true }).click()
  await page.waitForFunction(() => /^other:\d+$/.test(document.querySelector('.map-canvas')?.dataset.picked ?? ''))
  const fadedForZip = Number(await map.getAttribute('data-faded'))
  assert.ok(fadedForZip > 0, 'boxes without a zip file fade')
  assert.match(await text('.kinds-list .picked'), /Only the boxes with zip files are bright on the map\./)
  await shot('9t7-space-by-kind')
  await page.emulateMedia({ colorScheme: 'dark' })
  await shot('9t7-space-by-kind-dark')
  await page.emulateMedia({ colorScheme: 'light' })
  await fitsNarrow('Where the space goes, by kind of file')
  await page.locator('.kinds-list').getByRole('button', { name: 'Show everything' }).click()
  await page.waitForFunction(() => document.querySelector('.map-canvas')?.dataset.picked === '')
  await page.getByRole('tab', { name: 'By folder' }).click()
  await page.getByRole('grid', { name: 'What is in Pictures, the biggest first' }).waitFor()
  await fitsNarrow('Where the space goes')
  await page.setViewportSize({ width: 400, height: 800 })
  await shot('9s3-space-list-400')
  await page.emulateMedia({ colorScheme: 'dark' })
  await shot('9s4-space-list-400-dark')
  await page.emulateMedia({ colorScheme: 'light' })
  await page.setViewportSize({ width: 1440, height: 900 })
  await page.getByRole('navigation', { name: 'Where you are' }).getByRole('button', { name: 'Windows (C:)' }).click()
  await page.getByRole('grid', { name: 'What is in Windows (C:), the biggest first' }).waitFor()
  await page.getByRole('button', { name: '‹ Back' }).click()
  await page.getByRole('heading', { name: '4 folders on Windows (C:) seem to hold photos and videos' }).waitFor()
  assert.equal(await page.locator('.kept li.source').count(), 0, 'nothing was added')
  // While a survey measures what it never looks through, the folders found
  // can be shown at once.
  // The map grows while it walks, the top boxes coming with its progress,
  // easing from one picture to the next unless motion is unwelcome.
  const surveyGrowing = async () => {
    await page.goto(`${base}/?demo=empty&slow`)
    await page.evaluate(() => {
      window.keevoroEased = false
      new MutationObserver(() => {
        if (document.querySelector('.live-map[data-easing]')) window.keevoroEased = true
      }).observe(document.body, { attributes: true, subtree: true, attributeFilter: ['data-easing'] })
    })
    await page.getByRole('button', { name: /Windows \(C:\)/ }).click()
    await page.getByRole('button', { name: 'Find the folders with photos' }).click()
    await page.getByRole('button', { name: 'Show the folders now' }).waitFor({ timeout: 15000 })
    assert.match(await text('.searching'), /Now measuring the rest/)
    const growing = page.locator('.searching .live-map')
    await growing.waitFor()
    assert.ok(Number(await growing.getAttribute('data-boxes')) > 1, 'boxes, not a spinner alone')
    assert.match(await growing.locator('canvas').getAttribute('aria-label'), /^Map of Windows \(C:\) so far: /)
    return page.evaluate(() => window.keevoroEased)
  }
  assert.equal(await surveyGrowing(), true, 'the boxes grow')
  await page.emulateMedia({ reducedMotion: 'reduce' })
  assert.equal(await surveyGrowing(), false, 'with reduced motion each picture simply appears')
  await shot('9s6-survey-measuring')
  await page.emulateMedia({ reducedMotion: 'no-preference' })
  // From a source's card, surveyed first when Keevoro has not seen inside it.
  await page.goto(`${base}/?slow`)
  await page.locator('.steps button').nth(0).click()
  await page.getByRole('button', { name: 'Where the space goes on Family hard drive' }).click()
  await page.getByRole('heading', { name: 'Finding what is in Family hard drive…' }).waitFor()
  await page.getByRole('grid', { name: /^What is in / }).waitFor({ timeout: 15000 })
  assert.ok(await page.getByRole('heading', { name: 'Where the space goes on Family hard drive', level: 1 }).isVisible())
  await shot('9s5-space-of-a-source')
  // A map out of date is mapped again as it opens: by itself on a disk of
  // this computer, and after asking across a network.
  await page.goto(`${base}/?old=disk&slow`)
  await page.locator('.steps button').nth(0).click()
  await page.getByRole('button', { name: 'Where the space goes on Family hard drive' }).click()
  await page.getByRole('heading', { name: 'Finding what is in Family hard drive…' }).waitFor({ timeout: 15000 })
  assert.equal(await page.getByRole('region', { name: 'Map it again?' }).count(), 0, 'on a disk it is not asked')
  await page.getByRole('heading', { name: 'Finding what is in Family hard drive…' }).waitFor({ state: 'detached', timeout: 15000 })
  assert.match(await text('.space header'), /Mapped 2 minutes ago/)
  await page.goto(`${base}/?old=network&fast`)
  await page.locator('.steps button').nth(0).click()
  await page.getByRole('button', { name: 'Where the space goes on Family hard drive' }).click()
  const askAgain = page.getByRole('region', { name: 'Map it again?' })
  await askAgain.waitFor()
  assert.match(await askAgain.innerText(), /^This was mapped 3 days ago\. Map it again\? It may take a while over the network\./)
  await shot('9t9-space-map-again-asked')
  await fitsNarrow('Where the space goes, asking to map again')
  await askAgain.getByRole('button', { name: 'Use this map' }).click()
  await askAgain.waitFor({ state: 'detached' })
  assert.match(await text('.space header'), /Mapped 3 days ago/)
  await page.getByRole('button', { name: 'Map again' }).click()
  await page.getByRole('grid', { name: /^What is in / }).waitFor()
  await page.waitForFunction(() => /Mapped 2 minutes ago/.test(document.querySelector('.space header')?.textContent ?? ''))
  // A map lists folder names across a whole drive, so it can be forgotten.
  await page.getByRole('button', { name: '‹ Back' }).click()
  await page.getByRole('button', { name: 'Settings' }).click()
  await page.getByRole('heading', { name: 'Settings', level: 1 }).waitFor()
  await page.locator('details.advanced').last().getByText('Advanced', { exact: true }).click()
  await page.getByRole('button', { name: 'Forget remembered maps' }).click()
  assert.equal(await page.getByRole('status').filter({ hasText: 'remembered map' }).innerText(), 'Forgot 1 remembered map.')
  await page.locator('.setting', { hasText: 'Remembered maps' }).scrollIntoViewIfNeeded()
  await shot('9u-settings-forget-maps')

  // --- Every main screen fits a narrow window: the page never scrolls
  // sideways, and wide tables scroll inside their own box.
  await page.goto(`${base}/?fast`)
  await page.getByRole('heading', { name: 'Which of these should we keep?', level: 1 }).waitFor()
  await fitsNarrow('Choosing')
  await page.locator('.steps button').nth(0).click()
  await page.getByRole('heading', { name: 'Where are your photos?', level: 1 }).waitFor()
  await page.getByRole('button', { name: /^File types on / }).first().click()
  await fitsNarrow('Finding')
  await page.locator('.steps button').nth(2).click()
  await page.getByRole('button', { name: /A folder/ }).click()
  await page.getByRole('heading', { name: /^Backing up to the folder/ }).waitFor()
  await page.getByText('How earlier backups went').click()
  await page.getByText('What is in the backup', { exact: true }).click()
  await page.getByRole('button', { name: 'Clear unfinished copies…' }).click()
  await page.getByRole('group', { name: /^Unfinished copies in/ }).waitFor()
  await fitsNarrow('Backing up')
  await page.setViewportSize({ width: 400, height: 800 })
  await page.getByRole('heading', { name: /^Backing up to the folder/ }).scrollIntoViewIfNeeded()
  assert.equal(await headingUnderBand(), null, 'the backup screen’s heading is not under the band')
  await shot('11b-backup-400')
  await page.setViewportSize({ width: 1440, height: 900 })
  await page.getByRole('button', { name: 'Settings' }).click()
  for (const door of await page.locator('details.advanced').all()) await door.locator('summary').click()
  await fitsNarrow('Settings')
  await page.getByRole('button', { name: '‹ Back' }).click()
  await page.getByRole('button', { name: 'What Keevoro is doing' }).click()
  await fitsNarrow('What Keevoro is doing')
  await page.getByRole('button', { name: '‹ Back' }).click()
  await page.getByRole('button', { name: 'About Keevoro' }).click()
  await fitsNarrow('About')
  await page.getByRole('button', { name: '‹ Back' }).click()

  // --- The narrow window fits even when the typeface cannot be loaded,
  // whose stand-in draws words wider: the bar at the foot wraps instead.
  const plain = await browser.newPage({ viewport: { width: 400, height: 800 } })
  await plain.route('**/*.woff2', (route) => route.abort())
  await plain.goto(`${base}/?fast`)
  await plain.getByRole('heading', { name: 'Which of these should we keep?', level: 1 }).waitFor()
  await plain.evaluate(() => document.fonts.ready)
  const button = await plain.getByRole('button', { name: 'Choose where they go' }).boundingBox()
  assert.ok(button && button.x + button.width <= 400, `the button stays inside the window (${JSON.stringify(button)})`)
  await plain.close()

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
