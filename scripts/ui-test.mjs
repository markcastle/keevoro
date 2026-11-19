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
  await page.getByRole('button', { name: /^File types on / }).click()
  assert.equal(await page.locator('.types').count(), 0)
  // The bar is part of the window: on its bottom edge, full width.
  const bar = await page.locator('.next').boundingBox()
  const view = page.viewportSize()
  assert.ok(Math.abs(bar.y + bar.height - view.height) <= 1, 'the bar sits on the bottom edge of the window')
  // The careful checking carries on behind, and says so quietly.
  assert.match(await text('.foot .checking'), /Checking for duplicates in the background: [\d,]+ of [\d,]+/)
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
  assert.deepEqual(await page.locator('.sends th').allInnerTexts(), ['When', 'Where', 'Backed up', 'Took', 'Average speed'])
  assert.equal(await page.locator('.sends tbody tr').count(), 2, 'this send and the one before')
  assert.match(await page.locator('.sends tbody tr').nth(1).innerText(), /folder F:\\Family Backup\s+30\.1 GB\s+about 41 minutes\s+12\.5 MB\/s/)
  await page.getByText(/^Everything you are keeping is there: [\d,]+ files\.$/).waitFor()
  assert.ok(await page.getByRole('button', { name: 'Show me the folder' }).isVisible())
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
  // Unticking one leaves two, and the button says so.
  await ticks.nth(2).uncheck()
  await page.getByRole('button', { name: 'Look through these 2 folders' }).click()
  // Both are looked through, one after the other, with no more asked.
  await page.locator('.kept li.source').nth(1).waitFor()
  assert.equal(await page.locator('.kept li.source').count(), 2, 'each ticked folder became a source')
  assert.equal(await page.locator('.survey').count(), 0)

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
  await page.getByRole('button', { name: '‹ Back' }).click()

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
  await page.waitForTimeout(400) // let the colours finish easing across
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
  await page.getByRole('button', { name: '‹ Back' }).click()
  await page.getByRole('heading', { name: 'Where are your photos?', level: 1 }).waitFor()

  // --- A disk without room is refused before anything is sent.
  await page.goto(`${base}/?fast&full`)
  await page.getByRole('button', { name: 'Choose where they go' }).click()
  await page.getByRole('button', { name: /^A folder/ }).click()
  await page.getByText('There isn’t enough room there.').waitFor()
  assert.match(await text('.send .needs.short'), /need about [\d.]+ (MB|GB) and that disk has only 90\.0 MB free/)
  assert.ok(await page.getByRole('button', { name: 'Back up now' }).isDisabled(), 'sending waits until there is room')
  await shot('9g-not-enough-room')

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

  // --- Coming back to a backup that is already running shows it running,
  // never an offer to start another.
  await page.goto(`${base}/?sending`)
  await page.getByRole('button', { name: 'Choose where they go' }).click()
  await page.getByRole('button', { name: /^A folder/ }).click()
  await page.getByRole('button', { name: 'Stop', exact: true }).waitFor()
  assert.equal(await page.getByRole('button', { name: 'Back up now' }).count(), 0, 'no second start is offered')
  await page.locator('.speed').waitFor()
  // The screen says what step the backup is on straight away, in its own
  // words, and the activity page says the same.
  await page.getByText('Working out where each file goes: 2,500 of 11,245').waitFor()
  assert.equal(await page.locator('.send .done').count(), 0, 'nothing an earlier backup said is left showing')
  // There is always a bar while a backup runs, whatever step it is on.
  await page.locator('.send .speed progress').first().waitFor()
  await shot('9m-came-back-to-a-running-backup')
  await page.getByRole('button', { name: /^What Keevoro is doing/ }).click()
  await page.getByText(/^Right now: Working out where each file goes: 2,500 of 11,245$/).waitFor()
  await page.getByRole('button', { name: '‹ Back' }).click()

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
  assert.match(await text('.speed p'), /^[\d.]+ GB of [\d.]+ GB backed up at [\d.]+ MB\/s, about \d+ minutes left$/)
  // And the file just dealt with is named beneath.
  await page.getByText(/^Just done: originals\/Holidays\/Cornwall\/IMG_\d+\.jpg$/).waitFor()
  await shot('9l-send-speed')
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
