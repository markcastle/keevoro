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
// Stratus, the comparison of cloud storage, is never reached for real: a
// stand-in answers at its address. Like the real page in its embed mode,
// it says it is ready, shows what it was told, and says what was picked.
// Set `stratusOffline` to have the address not answer at all.
const STRATUS = /^https:\/\/stratus\.adam-sparshott\.workers\.dev(\/|$)/
const stratusAsked = []
let stratusOffline = false
const fakeStratus = `<!doctype html><meta charset="utf-8"><title>Stratus stand-in</title>
<p id="told">Not told anything yet</p>
<button id="r2">Pick R2 Standard</button> <button id="icloud">Pick iCloud+ 2TB</button>
<script>
  const pick = (option) => parent.postMessage({ stratus: 'select', option }, '*')
  addEventListener('message', (e) => {
    if (e.data && e.data.stratus === 'set') document.getElementById('told').textContent =
      'keys=' + Object.keys(e.data).sort().join(',') + ' need=' + e.data.need + ' currency=' + e.data.currency + ' kind=' + e.data.kind + ' from=' + e.origin
  })
  document.getElementById('r2').onclick = () => pick({ id: 'r2-standard', provider: 'cloudflare-r2', providerName: 'Cloudflare R2', name: 'R2 Standard', kind: 'object', monthly: 1.5, url: 'https://developers.cloudflare.com/r2/pricing/' })
  document.getElementById('icloud').onclick = () => pick({ id: 'icloud-2tb', provider: 'apple', providerName: 'Apple iCloud+', name: 'iCloud+ 2TB', kind: 'consumer', monthly: 8.99, url: 'https://support.apple.com/en-us/108047' })
  parent.postMessage({ stratus: 'ready', version: 'stand-in' }, '*')
</script>`
await page.route(STRATUS, (route) => {
  stratusAsked.push(route.request().url())
  if (stratusOffline) return route.abort('internetdisconnected')
  return route.fulfill({ status: 200, contentType: 'text/html', body: fakeStratus })
})
// Keevoro's reports service is never reached for real either. A stand-in
// answers at its address as `reportsService` says, as the real one does to
// the window (any origin may ask), and otherwise the address does not
// answer at all. What the window asked of it is noted.
const REPORTS = /^https:\/\/reports\.keevoro\.com\//
const reportsAsked = []
let reportsService = null
await page.route(REPORTS, (route) => {
  const request = route.request()
  const open = { 'access-control-allow-origin': '*', 'access-control-allow-methods': 'GET, POST, OPTIONS', 'access-control-allow-headers': 'content-type' }
  if (request.method() === 'OPTIONS') return route.fulfill({ status: 204, headers: open })
  reportsAsked.push({ method: request.method(), url: request.url(), body: request.postData(), cookie: request.headers().cookie })
  if (!reportsService) return route.abort('internetdisconnected')
  const { status, answer } = reportsService(request)
  return route.fulfill({ status, headers: { ...open, 'content-type': 'application/json' }, body: JSON.stringify(answer) })
})
const problems = []
page.on('pageerror', (error) => problems.push(String(error)))
page.on('console', (message) => message.type() === 'error' && problems.push(message.text()))
const shot = async (name) => {
  await settled()
  await page.screenshot({ path: join(shots, `${name}.png`) })
}
// The side band alone, to see its foot at a glance.
const footShot = async (name) => {
  await settled()
  // Clipped from the window as it is, so nothing on the page is scrolled to take it.
  const band = await page.locator('.sidebar').boundingBox()
  await page.screenshot({ path: join(shots, `${name}.png`), clip: band })
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
      // Says enough to find the cause from a failed run alone: where it starts,
      // how it is laid out, and what holds it.
      const up = el.parentElement
      const held = up ? `${up.tagName.toLowerCase()}.${String(up.className).replaceAll(' ', '.')} (${getComputedStyle(up).display}, ${Math.round(up.getBoundingClientRect().left)} to ${Math.round(up.getBoundingClientRect().right)})` : 'nothing'
      found.push(`${el.tagName.toLowerCase()}.${String(el.className).split(' ')[0]} reaches ${Math.round(box.right)} of ${Math.round(edge)}, from ${Math.round(box.left)}, ${getComputedStyle(el).display} ${getComputedStyle(el).whiteSpace}, in ${held}`)
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
    // Wherever the side band is, its foot is there too, however narrow.
    if ((await page.locator('.sidebar').count()) > 0) await footHere(`${screen} at ${width} pixels wide`)
    // Settings and Help put their list of sections in the band's place, with the same foot.
    else if ((await page.locator('.kv-side').count()) > 0) await footHere(`${screen} at ${width} pixels wide`, '.kv-side')
  }
  await page.setViewportSize({ width: 1440, height: 900 })
}
// Screenshots of one part of a screen in a narrow window, light and dark,
// brought into view, then the window put back.
const narrowShots = async (name, part) => {
  await page.setViewportSize({ width: 400, height: 800 })
  await part.scrollIntoViewIfNeeded()
  await shot(`${name}-400`)
  await page.emulateMedia({ colorScheme: 'dark' })
  await shot(`${name}-400-dark`)
  await page.emulateMedia({ colorScheme: 'light' })
  await page.setViewportSize({ width: 1440, height: 900 })
}
// The foot of the side band is the same on every main screen, in the same
// place: what Keevoro is doing (with its line), then Settings, then Help,
// and nothing else. In a narrow window the band is a strip across the top,
// with the foot in it all the same.
const footHere = async (where, band = '.sidebar') => {
  const foot = page.locator(`${band} .foot`)
  assert.equal(await foot.count(), 1, `${where}: the side band has its foot`)
  assert.ok(await foot.isVisible(), `${where}: the foot can be seen`)
  const links = await foot.locator(':scope > button.link').evaluateAll((all) => all.map((b) => b.firstChild?.textContent?.trim()))
  assert.deepEqual(links, ['What Keevoro is doing', 'Settings', 'Help'], `${where}: the foot holds what Keevoro is doing, Settings and Help`)
  const side = await page.locator(band).boundingBox()
  const box = await foot.boundingBox()
  assert.ok(side && box && box.x >= side.x && box.x + box.width <= side.x + side.width + 1, `${where}: the foot is inside the side band`)
}
// The backup screen's choices wanted now and then wait behind More, a menu
// beside the few buttons used most. Chooses one of them, as a person would.
const moreChoice = async (label) => {
  await page.locator('.panel.send .choices.actions').getByRole('button', { name: 'More' }).click()
  await page.getByRole('menu', { name: 'More' }).getByRole('menuitem', { name: label, exact: true }).click()
}
// What an "i" says: opened with a click or tap, read, and put away with
// Escape, which leaves the focus on the "i". `about` is what it is named
// after ("More about <about>"); `within` narrows where to look.
const tipSays = async (about, within = page) => {
  const button = within.getByRole('button', { name: `More about ${about}`, exact: true }).first()
  await button.click()
  const id = await button.getAttribute('aria-describedby')
  assert.ok(id, `the “i” for ${about} is described by what it says, once open`)
  const said = await page.locator(`[id="${id}"]`).innerText()
  await page.keyboard.press('Escape')
  await page.locator(`[id="${id}"]`).waitFor({ state: 'detached' })
  assert.equal(await button.evaluate((b) => b === document.activeElement), true, `the focus stays on the “i” for ${about}`)
  // Put down again, so no screenshot after shows a focus ring that was only the test's.
  await button.evaluate((b) => b.blur())
  return said
}
// How far apart the vertical centres of these elements are, in pixels: a
// row of controls that shares one centre line says close to 0.
const centresApart = (selectors) =>
  page.evaluate((selectors) => {
    const middles = selectors.map((selector) => {
      const r = document.querySelector(selector).getBoundingClientRect()
      return r.top + r.height / 2
    })
    return Math.max(...middles) - Math.min(...middles)
  }, selectors)
// A switch's words sit beside it, centred on it, never above or below.
const centredOnSwitch = (switches) =>
  switches.evaluateAll((all) =>
    all
      .map((button) => {
        const words = button.parentElement.querySelector(':scope > span')
        const a = button.getBoundingClientRect()
        const b = words.getBoundingClientRect()
        return Math.abs(a.top + a.height / 2 - (b.top + b.height / 2)) > 2 ? `“${words.innerText}” is off centre` : null
      })
      .filter(Boolean),
  )

try {
  // --- A first-time user is asked one thing they can answer: what to call this.
  await page.goto(`${base}/?demo=new&fast`)
  await page.getByRole('heading', { name: 'Let’s find your photos' }).waitFor()
  assert.equal(await page.locator('.welcome .primary').count(), 1, 'one way forward')
  assert.deepEqual(
    (await page.locator('.welcome').getByRole('button').allInnerTexts()).filter((t) => t !== ''),
    ['Get started', 'I already have a Keevoro backup'],
    'and a quiet second choice for a backup made before',
  )
  // Settings and Help are where they always are, even before the first backup.
  await footHere('The first screen')
  assert.equal(await page.getByLabel('What shall we call this backup?').inputValue(), 'My photos', 'with a name already suggested')
  await page.getByLabel('What shall we call this backup?').fill('Family photos')
  // What Keevoro is for is said in view, in two short sentences, not behind a disclosure.
  assert.match(await text('.welcome'), /Keevoro finds the photos and videos on your drives, cards and folders, and keeps a copy somewhere safe\. It never\s+changes anything it looks at\./)
  assert.equal(await page.locator('.welcome details').count(), 0, 'nothing to open to understand the first screen')
  assert.equal(await page.locator('.welcome .brand img:visible').getAttribute('alt'), 'Keevoro', 'the logo, name and all')
  await shot('1-welcome')
  await footShot('1-welcome-foot')
  await page.setViewportSize({ width: 400, height: 800 })
  await footHere('The first screen at 400 pixels')
  await shot('1-welcome-foot-400')
  await page.setViewportSize({ width: 1440, height: 900 })
  // Help opens from there too; with no backup yet, Settings says what waits for one.
  await page.getByRole('button', { name: 'Help', exact: true }).click()
  await page.getByRole('heading', { name: 'How do I…', level: 2 }).waitFor()
  await page.getByRole('button', { name: '‹ Back', exact: true }).click()
  await page.getByRole('button', { name: 'Settings', exact: true }).click()
  await page.getByText('These are set for each backup. Start one first, on the first screen, and its settings appear here.').waitFor()
  await page.keyboard.press('Escape')
  await page.getByRole('heading', { name: 'Let’s find your photos' }).waitFor()
  await page.getByRole('button', { name: 'Get started' }).click()

  // --- Step 1 asks where the photos are, and offers the drives by name.
  await page.getByRole('heading', { name: 'Where are your photos?', level: 1 }).waitFor()
  assert.match(await currentStep(), /Find your photos/)
  const places = page.locator('.places .place')
  assert.equal(await places.count(), 5, 'the USB drive, OneDrive, a folder, cloud storage, and this computer’s disk')
  assert.match(await places.first().innerText(), /^KINGSTON \(E:\)\s+USB or external drive · 32 GB$/)
  // A folder OneDrive keeps in step is offered by its name, after the
  // drives plugged in, and never looks chosen.
  const oneDrive = page.locator('.place.synced')
  assert.match(await oneDrive.innerText(), /^OneDrive\s+Synced with Microsoft’s cloud · C:\\Users\\You\\OneDrive$/)
  assert.equal(await places.nth(1).innerText(), await oneDrive.innerText(), 'after the USB drive')
  assert.equal(
    await oneDrive.evaluate((el) => getComputedStyle(el).backgroundColor),
    await places.first().evaluate((el) => getComputedStyle(el).backgroundColor),
    'shaded like any other card',
  )
  assert.ok(await page.locator('.steps button').nth(1).isDisabled(), 'later steps wait their turn')
  assert.equal(await page.locator('.next').count(), 0, 'nothing to move on to until something is found')
  assert.equal(await text('.find header .status'), 'A source is anywhere your photos might be: a drive, USB stick, memory card or folder. Keevoro only ever reads it.')
  await shot('2-find')
  await footHere('Find your photos')
  await fitsNarrow('The first screen with a synced folder')

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
  assert.deepEqual(await page.locator('.types li b').allInnerTexts(), ['JPG', 'DNG', 'MP4', 'PSD'], 'a less common kind found is listed like any other')
  // Each says what it is behind its "i", in the same words as Settings, Kinds of file.
  assert.equal(await tipSays('.psd', page.locator('.types')), 'An Adobe Photoshop picture.')
  assert.equal(await page.locator('.types li .info-tip').count(), 4)
  const dng = page.getByRole('switch', { name: 'Include DNG files' })
  assert.equal(await dng.getAttribute('aria-checked'), 'true', 'raw files are included unless switched off')
  await dng.click()
  assert.equal(await dng.getAttribute('aria-checked'), 'false')
  assert.match(await page.locator('.types li').nth(1).innerText(), /left out/)
  await shot('3b-file-types')
  await dng.click()
  // One source can be photos only, whatever the backup is for, and each
  // ending says which level left it out.
  // Said as a person would: "Back up from …", then the plain switches, as
  // the rest of the backup is to begin with; no line until it differs.
  const sourceWants = page.getByRole('group', { name: 'Back up from KINGSTON (E:)' })
  assert.equal(await page.locator('.source-wants h4').innerText(), 'Back up from KINGSTON (E:)')
  assert.deepEqual(await sourceWants.getByRole('switch').evaluateAll((boxes) => boxes.map((b) => b.parentElement.innerText.trim())), ['Photos', 'Videos', 'Music'], 'no other files to choose until a kind of them is listed')
  assert.deepEqual(await sourceWants.getByRole('switch').evaluateAll((boxes) => boxes.map((b) => b.getAttribute('aria-checked'))), ['true', 'true', 'false'], 'music only when asked for')
  assert.equal(await page.locator('.source-wants .differs').count(), 0, 'nothing said while it is like the rest')
  assert.doesNotMatch(await text('.types'), /same as|follows|the backup\b/i, 'no words that need knowing how Keevoro works inside')
  await sourceWants.getByRole('switch', { name: 'Videos' }).uncheck()
  await page.locator('.source-wants').getByRole('status').getByText('Anything already backed up stays backed up.').waitFor()
  assert.equal(await text('.source-wants .differs'), 'Different from the rest of this backup. Use the usual')
  assert.match(await page.locator('.types li').nth(2).innerText(), /left out: this source is set to photos only/)
  // One panel of rows, as in Settings: the words on the left, each switch on the right.
  assert.deepEqual(await centredOnSwitch(page.locator('.source-wants [role=switch]')), [])
  await shot('3c-source-photos-only')
  // The last tick cannot be taken away: a source is always for something, and it says so plainly.
  await sourceWants.getByRole('switch', { name: 'Photos' }).click({ force: true })
  assert.equal(await sourceWants.getByRole('switch', { name: 'Photos' }).isChecked(), true)
  await page.locator('.source-wants').getByRole('status').getByText('Nothing would be backed up from KINGSTON (E:). Keep one on, or remove this source.').waitFor()
  // Set back to match the rest, it is like the rest again, with nothing said; Use the usual does the same.
  await sourceWants.getByRole('switch', { name: 'Videos' }).check()
  await page.locator('.source-wants .differs').waitFor({ state: 'detached' })
  await page.locator('.types li').nth(2).getByText(/left out/).waitFor({ state: 'detached' })
  await sourceWants.getByRole('switch', { name: 'Photos' }).uncheck()
  await page.locator('.source-wants .differs').getByRole('button', { name: 'Use the usual' }).click()
  await page.locator('.source-wants .differs').waitFor({ state: 'detached' })
  assert.equal(await sourceWants.getByRole('switch', { name: 'Photos' }).isChecked(), true)
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

  // --- The first time, moving on asks how to arrange the backup: one
  // question, the suggestion already chosen and why, as part of finding.
  await page.getByRole('heading', { name: 'How should we arrange them in the backup?', level: 1 }).waitFor()
  assert.match(await currentStep(), /Find your photos/, 'the question is part of finding')
  const arrangeChoices = page.getByRole('radiogroup', { name: 'How should we arrange them in the backup?' })
  assert.equal(await arrangeChoices.getByRole('radio').count(), 2)
  assert.equal(await page.getByRole('radio', { name: /^Arrange by date/ }).isChecked(), true, 'the suggestion is chosen to begin with')
  assert.match(await text('.arrange-choice.on'), /Suggested/)
  assert.equal(await text('.arrange-why'), 'Suggested because the photos in KINGSTON (E:) look like they came straight from a camera.')
  assert.match(await text('.arrange-question'), /starting with “KINGSTON”/)
  // What tells the two apart is said with each, in view, and that it can be changed later.
  assert.match(await text('.arrange-question'), /A folder for each year and month, by when each photo was taken\. Folders you named, such as Holidays, are kept\./)
  assert.match(await text('.arrange-question'), /Best for scans, and folders you have\s+already sorted\./)
  assert.match(await text('.arrange-question'), /You can change this later in Settings\. It only changes where files go from then on\./)
  assert.equal(await page.locator('.arrange-question .info-tip').count(), 0, 'no “i” to open to choose')
  await shot('3d-how-to-arrange')
  await fitsNarrow('the question how to arrange')
  // Back goes to finding; moving on asks again until it is answered.
  await page.getByRole('button', { name: '‹ Back' }).click()
  await page.locator('.arrange-question').waitFor({ state: 'detached' })
  await page.getByRole('button', { name: 'See what was found' }).click()
  await page.getByRole('heading', { name: 'How should we arrange them in the backup?', level: 1 }).waitFor()
  await page.locator('.arrange-question').getByRole('button', { name: 'Next' }).click()

  // --- Step 2 shows collections, all kept, with one switch each.
  await page.getByRole('heading', { name: 'Which of these should we keep?', level: 1 }).waitFor()
  assert.equal(
    await text('.status'),
    'Keevoro has sorted what it found into collections, by folder name or by month. Everything is kept unless you say otherwise; click one to choose photo by photo.',
    'why there is more than one, in view',
  )
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
  await footHere('Inside a collection')

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
  await footHere('Duplicates')
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
  assert.deepEqual(await kinds.allInnerTexts(), ['A folder', 'Cloudflare R2', 'Amazon S3', 'Google Cloud Storage', 'Other storage'])
  // It says how much room the files will need before a place is chosen.
  assert.match(await text('.send .needs'), /^[\d,]+ files · about [\d.]+ (MB|GB)\. Wherever you choose needs that much free\.$/)
  // Why the whole screen is there is said under its heading, in view.
  assert.equal(
    await text('.archive header .status'),
    'Keevoro copies what you keep straight from your sources, checks every copy, and never replaces a file already there. Your sources are not changed.',
  )
  assert.equal(await page.locator('.archive header details').count(), 0)
  await shot('7-where-should-they-go')
  await footHere('Where should they go?')
  // Not sure where to buy cloud storage? One quiet line, and nothing is
  // loaded from the internet until it is chosen.
  assert.equal(await text('.compare-line'), 'Not sure where to buy cloud storage? Compare providers')
  assert.deepEqual(stratusAsked, [], 'nothing is asked of Stratus before the person opens it')
  await page.getByRole('button', { name: 'Compare providers' }).click()
  const compare = page.getByRole('dialog', { name: 'Compare cloud storage' })
  await compare.waitFor()
  assert.match(await compare.innerText(), /This loads a comparison page from the internet\. Keevoro tells it only the size of this backup, about [\d,]+ GB, and your currency\. The prices are Stratus’s, not Keevoro’s/)
  assert.match(await compare.innerText(), /Comparison by Stratus, made by Adam Sparshott\./)
  const frame = compare.locator('iframe')
  assert.equal(await frame.getAttribute('sandbox'), 'allow-scripts allow-same-origin', 'no popups, no moving the app away, no forms')
  assert.equal(await frame.getAttribute('referrerpolicy'), 'no-referrer')
  // Told the size of this backup and a currency, and nothing else.
  const told = page.frameLocator('.compare-sheet iframe').locator('#told')
  await told.filter({ hasText: /^keys=/ }).waitFor()
  assert.match(await told.innerText(), /^keys=currency,kind,need,stratus need=\d+ currency=(GBP|USD|EUR) kind=object from=http:\/\/127\.0\.0\.1:\d+$/)
  assert.ok(stratusAsked.length > 0 && stratusAsked.every((url) => STRATUS.test(url)))
  await shot('7b-compare-storage')
  await fitsNarrow('Comparing cloud storage')
  await narrowShots('7b-compare-storage', compare)
  // Picking something Keevoro backs up to offers its form, ready to fill in.
  await page.frameLocator('.compare-sheet iframe').getByRole('button', { name: 'Pick R2 Standard' }).click()
  const setUp = compare.getByRole('button', { name: 'Set up Cloudflare R2 in Keevoro' })
  await setUp.waitFor()
  assert.match(await compare.locator('.compare-choice').innerText(), /You chose R2 Standard\. Once you have a bucket and its keys, Keevoro can back up there\./)
  await shot('7c-compare-storage-chosen')
  await setUp.click()
  await compare.waitFor({ state: 'detached' })
  assert.equal(await page.locator('form.connect').getByLabel('Storage').inputValue(), 'r2')
  await page.locator('form.connect').getByLabel('Cloudflare account ID').waitFor()
  await page.locator('form.connect').getByRole('button', { name: 'Cancel' }).click()
  // Something Keevoro can't back up to directly is named, with its page.
  await page.getByRole('button', { name: 'Compare providers' }).click()
  await page.frameLocator('.compare-sheet iframe').getByRole('button', { name: 'Pick iCloud+ 2TB' }).click()
  await compare.getByText('Keevoro can’t back up to Apple iCloud+ directly', { exact: false }).waitFor()
  assert.equal(await compare.getByRole('button', { name: /^Set up / }).count(), 0)
  await compare.getByRole('button', { name: 'Open Apple iCloud+’s page' }).click()
  await page.waitForFunction(() => (window.keevoroOpenedPages ?? []).includes('https://support.apple.com/en-us/108047'))
  await compare.getByRole('button', { name: 'Close' }).click()
  await compare.waitFor({ state: 'detached' })
  // With no connection, it says so after a short wait, and offers the browser.
  stratusOffline = true
  await page.getByRole('button', { name: 'Compare providers' }).click()
  await compare.getByText('Couldn’t load the comparison just now.', { exact: false }).waitFor({ timeout: 30_000 })
  await shot('7d-compare-storage-offline')
  await compare.locator('.compare-failed').getByRole('button', { name: 'Open Stratus in your browser' }).click()
  await page.waitForFunction(() => (window.keevoroOpenedPages ?? []).includes('https://stratus.adam-sparshott.workers.dev/'))
  await page.keyboard.press('Escape')
  await compare.waitFor({ state: 'detached' })
  stratusOffline = false
  // Cloud storage asks only what it needs, and hides the secret.
  await page.getByRole('button', { name: /Cloudflare R2/ }).click()
  await page.getByLabel('Cloudflare account ID').fill('0123456789abcdef0123456789abcdef')
  assert.equal(await page.getByLabel('Secret access key').getAttribute('type'), 'password', 'the secret is hidden')
  await shot('8-cloud-form')
  await page.getByRole('button', { name: 'Cancel' }).click()
  // Google Cloud Storage asks only for the bucket and its keys: Keevoro
  // fills in Google's address, and says where in Google's console the keys
  // are made, and which class to keep the bucket in.
  await page.getByRole('button', { name: /Google Cloud Storage/ }).click()
  const googleForm = page.locator('form.connect')
  assert.equal(await googleForm.getByLabel('Storage').inputValue(), 'gcs')
  for (const asked of ['Cloudflare account ID', 'Region', 'Address']) {
    assert.equal(await googleForm.getByLabel(asked).count(), 0, `Google Cloud Storage is not asked its ${asked}`)
  }
  // Where to make the keys is a step-by-step, so it opens in place when asked for.
  assert.doesNotMatch(await googleForm.innerText(), /Interoperability/)
  await googleForm.getByText('Where to make the keys').click()
  assert.match(await googleForm.innerText(), /Make the keys in the Google Cloud console under Cloud Storage, Settings, Interoperability/)
  // What costs money stays in sight; the detail is behind its "i".
  assert.match(await googleForm.innerText(), /Keep this bucket in Google’s Standard class\. Colder classes charge each time Keevoro checks a file\./)
  assert.match(await tipSays('Google’s bucket classes', googleForm), /Nearline, Coldline and Archive charge each time Keevoro reads a file back to check it/)
  assert.match(await googleForm.innerText(), /Use a key that can read and write this one bucket only\. It is kept in your computer’s keychain\./)
  assert.equal(await googleForm.getByLabel('Access key ID').getAttribute('placeholder'), 'GOOG…')
  assert.equal(await googleForm.getByLabel('Secret access key').getAttribute('type'), 'password', 'the secret is hidden')
  await googleForm.scrollIntoViewIfNeeded()
  await shot('8b-google-cloud-form')
  await fitsNarrow('The Google Cloud Storage form')
  await narrowShots('8b-google-cloud-form', googleForm)
  await googleForm.getByRole('button', { name: 'Cancel' }).click()
  // A folder is one click, then one button sends everything.
  await page.getByRole('button', { name: /^A folder/ }).click()
  await page.getByRole('heading', { name: 'Backing up to the folder F:\\Family Backup' }).waitFor()
  await page.getByText(/^[\d,]+ files are waiting to be backed up\.$/).waitFor()
  // The folder's name opens the folder, and the room is weighed against the disk.
  assert.ok(await page.getByRole('button', { name: 'F:\\Family Backup' }).isVisible())
  assert.match(await text('.send .needs'), /^Needs about [\d.]+ (MB|GB) · [\d.]+ GB free there\.$/)
  await page.getByRole('button', { name: 'Back up now' }).click()
  await page.locator('.send .done').first().waitFor()
  assert.match(await text('.send .done'), /^Safely backed up [\d,]+ files\.$/)
  // The routine figures (previews added, how long it took and how fast)
  // follow on one quieter line, in view.
  assert.match(
    await text('.send .done-more'),
    /^Previews added: [\d,]+ files\. That took about \d+ minutes: [\d.]+ GB at an average of [\d.]+ MB a second\.$/,
  )
  assert.equal(await page.locator('.send details.more-about').count(), 0, 'nothing to open to see how it went')
  // What tells the two most used buttons apart is said under them.
  assert.equal(await text('.send .actions-said'), 'Check they all arrived sends nothing: it looks for every file where it was sent.')
  // The backup's folders are explained, a rendition included.
  await page.getByText('What is in the backup', { exact: true }).click()
  assert.match(await text('.sends.layout'), /A rendition is a version of a photo or video made for looking at, not for keeping\./)
  assert.deepEqual(await page.locator('.sends.layout dt').allInnerTexts(), ['originals', 'renditions', 'index', 'What is in this backup.txt'])
  await shot('9n-what-is-in-the-backup')
  // A backup can be the originals alone. One quiet line says which, and
  // Change goes to the switch, in Settings, brought into view and marked.
  assert.equal(await text('.previews-line'), 'With previews for a gallery, made in the background. Change')
  await page.locator('.previews-line').getByRole('button', { name: 'Change' }).click()
  await page.getByRole('heading', { name: 'This backup', level: 2 }).waitFor()
  await page.locator('#setting-previews[data-marked]').waitFor()
  const withPreviews = page.getByRole('switch', { name: 'Previews for a gallery' })
  assert.equal(await withPreviews.getAttribute('aria-checked'), 'true', 'previews are made unless switched off')
  assert.equal(await page.evaluate(() => document.activeElement?.getAttribute('role')), 'switch', 'its switch has the focus')
  assert.equal(await withPreviews.evaluate((el) => document.getElementById(el.getAttribute('aria-describedby')).textContent), 'Small copies for looking through. Off is quicker.')
  // The rest of what it means is behind the "i" beside its name, reached from the keyboard.
  await page.keyboard.press('Shift+Tab')
  const previewsTip = page.locator('#setting-previews').getByRole('button', { name: 'More about Previews for a gallery' })
  assert.equal(await previewsTip.evaluate((el) => el === document.activeElement), true, 'the “i” is next to the switch, going back')
  assert.equal(await previewsTip.getAttribute('aria-expanded'), 'true', 'and opens as the focus reaches it')
  assert.match(await page.getByRole('tooltip').innerText(), /Off, only the originals are backed up, which is quicker; switched on again, they are added on the next backup\./)
  await shot('9m2-settings-previews-tip')
  await page.keyboard.press('Escape')
  assert.equal(await previewsTip.getAttribute('aria-expanded'), 'false', 'Escape puts it away')
  assert.equal(await previewsTip.evaluate((el) => el === document.activeElement), true, 'and leaves the focus on the “i”')
  assert.equal(await page.getByRole('heading', { name: 'This backup', level: 2 }).count(), 1, 'without leaving Settings')
  await page.keyboard.press('Tab')
  await shot('9m-settings-previews-marked')
  await page.keyboard.press('Space')
  assert.equal(await withPreviews.getAttribute('aria-checked'), 'false', 'switched from the keyboard')
  await page.getByRole('button', { name: '‹ Back', exact: true }).click()
  assert.equal(await text('.previews-line'), 'Originals only, with no previews. Change')
  await page.locator('.previews-line').getByRole('button', { name: 'Change' }).click()
  await withPreviews.click()
  await page.getByRole('button', { name: '‹ Back', exact: true }).click()
  await page.locator('.previews-line').getByText('With previews for a gallery, made in the background.').waitFor()
  await page.getByText('How earlier backups went').click()
  const runs = page.locator('.sends > .sends-scroll > table > tbody > tr')
  assert.deepEqual(await page.locator('.sends > .sends-scroll > table > thead th').allInnerTexts(), ['When', 'Where', 'Backed up', 'Took', 'Average speed', 'Details'])
  assert.equal(await runs.count(), 3, 'this backup and the two before')
  assert.match(await runs.nth(1).innerText(), /folder \\\\NAS\\Photos\s+316\.2 GB\s+about 9 hours 40 minutes\s+9\.3 MB\/s/)
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
  assert.match(await panel.innerText(), /316\.2 GB to the folder \\\\NAS\\Photos in about 9 hours 40 minutes, at an average of 9\.3 MB a second\./)
  assert.match(await panel.innerText(), /from the app, with Keevoro 0\.32\.0\. It finished\. It was paused for about 10 minutes/)
  assert.match(await panel.locator('table.stages').innerText(), /Backing up your files\s+Went wrong: Couldn’t back up 2 files\s+about 9 hours\s+312\.4 GB\s+9\.9 MB a second\s+slowest 3\.2 MB a second, fastest 14\.6 MB a second/)
  // A stage that went wrong says so in words beside its mark, not by colour alone.
  assert.equal(await panel.locator('table.stages .went-wrong').count(), 1)
  assert.match(await panel.innerText(), /8,628 files sent \(312\.4 GB\), 2 could not be sent\./)
  assert.match(await panel.innerText(), /1,118 duplicates left out, saving 27\.5 GB\./)
  assert.match(await panel.innerText(), /16,820 preview files sent \(3\.8 GB\)/)
  assert.match(await panel.innerText(), /NAS\\Photos, across a network: 316\.2 GB/)
  assert.match(await panel.innerText(), /What it said at the end\s+Safely backed up 8,628 files/)
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
  // Behind More, clearing what a stopped backup left there: said first, cleared on a yes.
  await moreChoice('Clear unfinished copies…')
  const unfinished = page.getByRole('group', { name: 'Unfinished copies in F:\\Family Backup' })
  assert.match(await unfinished.innerText(), /^There are 320\.4 MB of unfinished copies \(12 files\) in F:\\Family Backup/)
  await unfinished.getByRole('button', { name: 'Cancel' }).click()
  assert.equal(await unfinished.count(), 0, 'cancelling clears nothing and puts the question away')
  await moreChoice('Clear unfinished copies…')
  await unfinished.getByRole('button', { name: 'Clear them' }).click()
  await unfinished.getByText(/^Cleared 320\.4 MB of unfinished copies \(12 files\) from F:\\Family Backup\./).waitFor()
  await unfinished.getByRole('button', { name: 'Done' }).click()
  await page.getByRole('button', { name: 'Check they all arrived' }).click()
  await page.getByText(/^Your backup: all [\d,]+ originals are there\.$/).waitFor()
  // Both lines that say all is well carry the green tick; it is drawn, not text.
  assert.equal(await page.locator('p.went-well svg.went-well-tick').count(), 2, 'a green tick beside each line that says everything is there')
  assert.equal(await page.locator('.steps li.done').count(), 2, 'finding and choosing are ticked off')
  assert.equal(await page.locator('.steps li').count(), 3, 'there are three steps, not four')
  await shot('9-sent')
  // --- A backup can go to more than one place. A second destination is
  // added beside the first, starts empty, and catches up on the next backup.
  await moreChoice('Add another destination')
  await page.getByRole('heading', { name: 'Add another destination' }).waitFor()
  assert.match(await text('.send .adding'), /as well as the folder F:\\Family Backup, catching up on the next backup\. A photo counts as\s+safe once it is at every destination\./)
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
  await page.getByLabel('Name for the new backup').fill('Holiday videos')
  await shot('9a-new-backup')
  await page.getByRole('button', { name: 'Start', exact: true }).click()
  await page.getByRole('heading', { name: 'Where are your photos?', level: 1 }).waitFor()
  assert.match(await text('.backup .current'), /Holiday videos/)
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
  const asking = page.locator('.asking')
  assert.match(await text('.asking #asking-said'), /^That is a whole disk \(512 GB\), and most of it will not be photos\. Nothing on it is\s+changed, whichever you choose\.$/)
  // Each way on is a row of its own, saying what it does and how long it
  // takes, in view: nothing to open, and no "i" among them.
  const options = asking.getByRole('group', { name: 'How to look' }).getByRole('button')
  const said = async (name) => {
    const option = asking.getByRole('button', { name, exact: true })
    return option.evaluate((b) => document.getElementById(b.getAttribute('aria-describedby')).textContent.replace(/\s+/g, ' ').trim())
  }
  assert.deepEqual(await options.evaluateAll((all) => all.map((b) => b.getAttribute('aria-label'))), [
    'Find the folders with photos',
    'Choose a folder inside it',
    'Look through all of it',
  ])
  assert.equal(await said('Find the folders with photos'), 'Quick. Reads only the names of what is on it, opens nothing, then shows which folders hold photos for you to tick. Takes seconds.')
  assert.equal(await said('Choose a folder inside it'), 'If you know where your photos are, pick that one folder to look through.')
  assert.equal(await said('Look through all of it'), 'Goes through every file on the disk. A few minutes to an hour or more; you can leave it running.')
  assert.match(await asking.getByRole('button', { name: 'Find the folders with photos' }).innerText(), /Recommended/)
  assert.equal(await asking.locator('.info-tip, details').count(), 0, 'nothing to open to choose')
  assert.equal(await asking.locator('.primary').count(), 1, 'one filled, the quick way')
  // The whole row is the thing to press, and the rows are one under another, the same width.
  const rows = await options.evaluateAll((all) => all.map((b) => b.getBoundingClientRect()).map((r) => ({ left: Math.round(r.left), width: Math.round(r.width), top: r.top, height: r.height })))
  assert.equal(new Set(rows.map((r) => `${r.left} ${r.width}`)).size, 1, 'the rows line up, one under another')
  assert.ok(rows.every((r, at) => r.height >= 44 && (at === 0 || r.top > rows[at - 1].top)), 'each row is a large target, below the one before')
  assert.equal(await asking.getByRole('button', { name: 'Cancel' }).getAttribute('class'), 'link cancel', 'Cancel is a quiet link')
  // The quick way has the focus; up and down move between the rows.
  assert.equal(await page.evaluate(() => document.activeElement?.getAttribute('aria-label')), 'Find the folders with photos')
  await page.keyboard.press('ArrowDown')
  assert.equal(await page.evaluate(() => document.activeElement?.getAttribute('aria-label')), 'Choose a folder inside it')
  await page.keyboard.press('ArrowDown')
  assert.equal(await page.evaluate(() => document.activeElement?.getAttribute('aria-label')), 'Look through all of it')
  await page.keyboard.press('ArrowDown')
  assert.equal(await page.evaluate(() => document.activeElement?.getAttribute('aria-label')), 'Find the folders with photos', 'and round again')
  await page.keyboard.press('ArrowUp')
  assert.equal(await page.evaluate(() => document.activeElement?.getAttribute('aria-label')), 'Look through all of it')
  await page.keyboard.press('Shift+Tab')
  assert.equal(await page.evaluate(() => document.activeElement?.getAttribute('aria-label')), 'Choose a folder inside it', 'Tab goes through them too')
  await page.evaluate(() => document.activeElement?.blur())
  await settled()
  await shot('9d-whole-disk')
  await page.emulateMedia({ colorScheme: 'dark' })
  await settled()
  await shot('9d-whole-disk-dark')
  await page.emulateMedia({ colorScheme: 'light' })
  await fitsNarrow('The question about a whole disk')
  await narrowShots('9d-whole-disk', asking)
  await page.getByRole('button', { name: 'Cancel' }).click()
  assert.equal(await page.locator('.asking').count(), 0, 'cancelling goes back to the choices')
  assert.equal(await page.locator('.searching').count(), 0, 'and nothing was started')
  // Each row is chosen from the keyboard with Enter: looking through all of it.
  await page.getByRole('button', { name: /Windows \(C:\)/ }).click()
  await page.keyboard.press('ArrowUp')
  await page.keyboard.press('Enter')
  await page.getByRole('heading', { name: /^Found [\d,]+ photos/ }).waitFor()
  // Choosing a folder inside it, from the keyboard: the folder chosen is looked through.
  await page.goto(`${base}/?demo=empty&fast`)
  await page.getByRole('button', { name: /Windows \(C:\)/ }).click()
  await page.keyboard.press('ArrowDown')
  await page.keyboard.press('Enter')
  await page.getByRole('heading', { name: /^Found [\d,]+ photos/ }).waitFor()
  // The folder window opens inside that disk, not wherever it was last, so
  // the folder chosen is on it.
  assert.equal(await text('.kept li.source .what .folder'), 'C:\\Photos', 'chosen from inside the disk asked about')
  // And the quick way, with Enter where the focus starts.
  await page.goto(`${base}/?demo=empty&fast`)
  await page.getByRole('button', { name: /Windows \(C:\)/ }).click()
  await page.keyboard.press('Enter')
  await page.getByRole('heading', { name: /seem to hold photos and videos/ }).waitFor()

  // --- Not sure where the photos are? A quick survey finds the folders,
  // opening nothing, and the ticked ones are looked through one by one.
  await page.goto(`${base}/?demo=empty&fast`)
  await page.getByRole('button', { name: /Windows \(C:\)/ }).click()
  await page.getByRole('button', { name: 'Find the folders with photos' }).click()
  await page.getByRole('heading', { name: /^4 folders on Windows \(C:\) seem to hold photos and videos/ }).waitFor()
  assert.match(await text('.survey'), /Tick the folders you want, and Keevoro will\s+look through those properly\. Leave out keeps a folder out of this backup, now and later\./)
  assert.match(await tipSays('how these were found', page.locator('.survey')), /Looked at 352,640 files in 28,915 folders in 5\.4 seconds, without opening any of\s+them\. This goes by file names/)
  const ticks = page.locator('.survey .hints li.suggested > label input')
  assert.deepEqual(
    await ticks.evaluateAll((boxes) => boxes.map((b) => b.checked)),
    [true, true, true, false],
    'real photos are ticked to begin with; probable icons are not',
  )
  assert.match(await page.locator('.survey .hints li.suggested').last().innerText(), /Small pictures, probably not photos/)
  // Its note sits with the row's description, before the buttons, as the
  // other rows have theirs.
  const note = await page.locator('.survey .hints li.suggested').last().evaluate((li) => {
    const said = li.querySelector('.badge').getBoundingClientRect()
    const look = [...li.querySelectorAll('button')].find((b) => b.textContent === 'Look inside').getBoundingClientRect()
    return { before: said.right <= look.left, inAbout: li.querySelector('.about .badge') !== null }
  })
  assert.deepEqual(note, { before: true, inAbout: true })
  await shot('9q-survey')
  await footHere('The survey of a disk')
  await narrowShots('9q-survey', page.locator('.survey .hints li.suggested').last())
  // Files only in OneDrive are never opened, and never left out silently: each suggestion says so.
  const onlineSaid = /^412 files here are in OneDrive only, not on this computer, so Keevoro did not open them\. To include them, make them available offline in OneDrive first, then look again\.$/
  assert.match(await page.locator('.survey .hints li.suggested').nth(1).locator('.online-only').innerText(), onlineSaid)
  // A library an older Keevoro left is never offered, and is said quietly, on one line that opens.
  const leftOut = page.locator('.survey .left-out')
  assert.equal(await leftOut.locator('summary').innerText(), 'Left out a folder Keevoro made, as it is not a source')
  assert.equal(await leftOut.getByRole('button', { name: 'Show me the folder' }).isVisible(), false, 'folded away')
  await leftOut.locator('summary').click()
  assert.match(await leftOut.innerText(), /a folder an older version of Keevoro made, which it no longer uses\. It takes 9\.4 GB: 8\.6 GB of copies of your photos/)
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
  // After the search, on screen, not only in the log; and kept with the
  // source, with how much they come to, from each file's listing.
  const onlineSized = /^412 files, 18\.4 GB, are in OneDrive only, not on this computer, so Keevoro did not open them\. To include them, make them available offline in OneDrive first, then look again\.$/
  await page.locator('.found-it .online-only').getByText(onlineSized).waitFor()
  assert.match(await page.locator('.kept li.source').nth(1).locator('.online-only').innerText(), onlineSized)
  await page.locator('.kept li.source').nth(1).scrollIntoViewIfNeeded()
  await shot('9q2-online-only')
  await page.locator('.steps button').nth(1).click()
  await page.locator('.steps button').nth(0).click()
  assert.match(await page.locator('.kept li.source').nth(1).locator('.online-only').innerText(), onlineSized, 'still said after going elsewhere')

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

  // --- Help sits beside Settings, opened from the foot of the side band,
  // and built the same way: its own list of sections, a search, rows, and
  // pages with a way back. It opens on How do I….
  await footHere('A backup with nothing found yet')
  await page.getByRole('button', { name: 'Help', exact: true }).click()
  await page.getByRole('heading', { name: 'Help', level: 1 }).waitFor()
  const helpSections = page.getByRole('navigation', { name: 'Help sections' })
  assert.deepEqual(await helpSections.getByRole('button').allInnerTexts(), [
    'How do I…',
    'Report a problem or suggest an idea',
    'Compare cloud storage providers',
    'What Keevoro has been doing',
    'About Keevoro',
  ])
  await page.getByRole('heading', { name: 'How do I…', level: 2 }).waitFor()
  assert.equal(await page.evaluate(() => document.activeElement?.textContent), 'How do I…', 'arriving, the page’s heading has the focus')
  assert.equal(await page.locator('.steps').count(), 0, 'the list of sections takes the side band’s place, never beside it')
  assert.equal(await page.locator('#setting-howSecondPlace').getByRole('button').innerText(), 'Add another destination')
  assert.match(await page.locator('#setting-howWindowClosed').innerText(), /Closing the window leaves Keevoro running by the clock\. Its icon is by the clock/)
  assert.match(await page.locator('#setting-howNewComputer').innerText(), /Install Keevoro on the new computer and choose Manage backups, then “Open a backup that is already somewhere”\./)
  assert.equal(await page.locator('#setting-howNewComputer').getByRole('button').innerText(), 'Open a backup that is already somewhere')
  assert.equal(await page.locator('.kv-page .primary').count(), 0, 'no filled button among the answers')
  // Help keeps the same foot as every other screen.
  await footHere('Help', '.kv-side')
  await fitsNarrow('Help')
  await shot('9q-help')
  await narrowShots('9q-help', page.locator('.kv-side'))
  const helpSection = async (name) => {
    await helpSections.getByRole('button', { name, exact: true }).click()
    await page.getByRole('heading', { name, level: 2 }).waitFor()
  }
  // An answer's button goes straight there: a setting, with its row marked.
  await page.getByRole('button', { name: 'Set a schedule' }).click()
  await page.getByRole('heading', { name: 'Backing up by itself', level: 2 }).waitFor()
  await page.locator('#setting-schedule[data-marked]').waitFor()
  // Escape goes back out, and Help opens again where it was.
  await page.keyboard.press('Escape')
  await page.getByRole('heading', { name: 'Where are your photos?', level: 1 }).waitFor()
  await page.getByRole('button', { name: 'Help', exact: true }).click()
  await page.getByRole('heading', { name: 'How do I…', level: 2 }).waitFor()
  await page.locator('#setting-howSource').getByRole('button', { name: 'Find your photos' }).click()
  await page.getByRole('heading', { name: 'Where are your photos?', level: 1 }).waitFor()
  assert.match(await currentStep(), /Find your photos/)
  await page.getByRole('button', { name: 'Help', exact: true }).click()

  // Help, What Keevoro has been doing: the log, newest at the bottom.
  await helpSection('What Keevoro has been doing')
  await page.locator('.kv-settings .log').getByText(/Reading a large file \(3\.9 GB\): E:\\Video\\wedding\.mov/).waitFor()
  await shot('9h-help-log')

  // --- Help, About Keevoro: the version, with Check for updates, what
  // Keevoro promises, what leaves this computer, who made it, the name, and
  // the licence; the open source licences a page of their own.
  await helpSection('About Keevoro')
  assert.deepEqual(await page.locator('.kv-promises .kv-row-title').allInnerTexts(), [
    'Your sources are only ever read',
    'A copy only counts when it is proved exact',
    'No original in a backup is ever replaced or removed',
    'Duplicates are exact, never guesses',
    'Nothing is hidden from you',
    'You never need Keevoro to get your photos back',
  ])
  assert.match(
    await page.locator('#setting-privacy').innerText(),
    /^Your photos go only where you send them\s+You need no Keevoro account\..*If you choose to compare cloud storage providers, in Help or on the backup screen, it shows Stratus.*tells it only the size of the backup and your currency\. A report of a problem or an idea goes to Keevoro’s maker only when you send one, and you see exactly what it holds first\./s,
  )
  assert.match(await text('.kv-settings'), /voro, an old Swedish word meaning “were”/)
  assert.match(await text('.kv-settings'), /Copyright 2026 Mark Castle\..*Functional Source License \(FSL-1\.1-MIT\)/s)
  // Who makes it, and who helped.
  assert.match(await text('.thanks'), /Keevoro is made by Mark Castle\./)
  assert.match(await text('.thanks'), /A special thank you to Adam Sparshott, for all his help in testing Keevoro and for his excellent ideas\./)
  for (const name of ['Read the licence', 'The name and logo', 'See the source']) {
    await page.getByRole('button', { name }).waitFor()
  }
  // The same Check for updates as Settings, Updates.
  await page.locator('#setting-aboutVersion').getByRole('button', { name: 'Check for updates' }).click()
  await page.locator('#setting-aboutVersion').getByText('That is the newest version.').waitFor()
  await shot('9r-about')
  await fitsNarrow('About Keevoro')
  await narrowShots('9r-about', page.locator('#settings-page-title'))

  // --- From About, every piece of open source work in the app, with its licence.
  await page.getByRole('button', { name: 'Open source licences' }).click()
  await page.getByRole('heading', { name: 'Open source licences', level: 2 }).waitFor()
  assert.match(await text('.licences'), /The app and the keevoro command \(\d{3,}\)/)
  assert.match(await text('.licences'), /The app’s screens \(\d+\)/)
  assert.match(await page.locator('.licensed li', { hasText: 'react-dom' }).innerText(), /react-dom\s+[\d.]+\s+MIT/)
  await fitsNarrow('The open source licences')
  await page.getByLabel('Find by name').fill('serde_json')
  assert.match(await text('.licences'), /The app and the keevoro command \(1 of \d+\)/)
  assert.match(await text('.licences'), /None by that name\./)
  await shot('9r2-licences')
  await page.getByRole('button', { name: '‹ About Keevoro' }).click()
  await page.getByRole('heading', { name: 'About Keevoro', level: 2 }).waitFor()
  assert.equal(await page.evaluate(() => document.activeElement?.id), 'setting-licences', 'back on the row that opened it')

  // --- Help, Compare cloud storage providers: the same sheet, said the
  // same way, set to the size of this backup.
  await helpSection('Compare cloud storage providers')
  assert.match(await text('.kv-page'), /Stratus, made by Adam Sparshott, compares what it costs/)
  assert.match(await text('.kv-page'), /This loads a comparison page from the internet\. Keevoro tells it only the size of this backup, about 1 GB, and your currency\./)
  await shot('9q-help-compare')
  await fitsNarrow('Help, Compare cloud storage providers')
  await page.setViewportSize({ width: 400, height: 800 })
  await shot('9q-help-compare-400')
  await page.setViewportSize({ width: 1440, height: 900 })
  const askedBefore = stratusAsked.length
  await page.getByRole('button', { name: 'Compare providers' }).click()
  const helpCompare = page.getByRole('dialog', { name: 'Compare cloud storage' })
  await helpCompare.waitFor()
  assert.match(await helpCompare.innerText(), /This loads a comparison page from the internet\. Keevoro tells it only the size of this backup, about 1 GB, and your currency\. The prices are Stratus’s, not Keevoro’s/)
  assert.match(await helpCompare.innerText(), /Comparison by Stratus, made by Adam Sparshott\./)
  await page.frameLocator('.compare-sheet iframe').getByRole('button', { name: 'Pick R2 Standard' }).click()
  assert.ok(stratusAsked.length > askedBefore && stratusAsked.every((url) => STRATUS.test(url)), 'only the stand-in is ever asked')
  await shot('9q-help-compare-sheet')
  // Escape closes the sheet, and only the sheet.
  await helpCompare.getByRole('heading', { name: 'Compare cloud storage' }).click()
  await page.keyboard.press('Escape')
  await helpCompare.waitFor({ state: 'detached' })
  await page.getByRole('heading', { name: 'Compare cloud storage providers', level: 2 }).waitFor()
  // A choice Keevoro can back up to opens Keevoro's own form for it, on the backup screen.
  await page.getByRole('button', { name: 'Compare providers' }).click()
  await page.frameLocator('.compare-sheet iframe').getByRole('button', { name: 'Pick R2 Standard' }).click()
  await helpCompare.getByRole('button', { name: 'Set up Cloudflare R2 in Keevoro' }).click()
  await page.getByRole('heading', { name: 'Where should they go?', level: 1 }).waitFor()
  await page.getByLabel('Cloudflare account ID').waitFor()
  await page.getByRole('button', { name: 'Cancel' }).click()
  // How do I… goes to the backup screen ready to add a destination.
  await page.getByRole('button', { name: 'Help', exact: true }).click()
  await helpSection('How do I…')
  await page.getByRole('button', { name: 'Add another destination' }).click()
  await page.getByRole('heading', { name: 'Where should they go?', level: 1 }).waitFor()
  await page.locator('.destinations .place').first().waitFor()
  await page.getByRole('button', { name: 'Find your photos', exact: false }).first().click()
  await page.getByRole('heading', { name: 'Where are your photos?', level: 1 }).waitFor()
  // Afresh, so Settings opens as it does the first time.
  await page.goto(`${base}/?demo=empty&fast`)

  // --- Settings: a short list of sections down the side, one shown at a
  // time, everyday first, Advanced set apart at the end. Only what you set:
  // Help is a place of its own beside it.
  await page.getByRole('button', { name: 'Settings', exact: true }).click()
  await page.getByRole('heading', { name: 'Settings', level: 1 }).waitFor()
  const sections = page.getByRole('navigation', { name: 'Settings sections' })
  assert.deepEqual(await sections.getByRole('button').allInnerTexts(), ['This backup', 'Backing up by itself', 'Appearance', 'Updates', 'Advanced'])
  assert.equal(await sections.getByRole('button', { name: 'This backup' }).getAttribute('aria-current'), 'page', 'it opens on the first section')
  await page.getByRole('heading', { name: 'This backup', level: 2 }).waitFor()
  assert.equal(await page.locator('.steps').count(), 0, 'the list of sections takes the side bar’s place, never beside it')
  // So does Settings.
  await footHere('Settings', '.kv-side')
  await fitsNarrow('Settings')
  await shot('9e-settings')
  // Up and down move through the sections, and the page follows.
  await sections.getByRole('button', { name: 'This backup' }).focus()
  await page.keyboard.press('ArrowDown')
  await page.getByRole('heading', { name: 'Backing up by itself', level: 2 }).waitFor()
  assert.equal(await page.evaluate(() => document.activeElement?.textContent), 'Backing up by itself', 'the focus moves with it')
  await page.keyboard.press('End')
  await page.getByRole('heading', { name: 'Advanced', level: 2 }).waitFor()
  await page.keyboard.press('Home')
  await page.getByRole('heading', { name: 'This backup', level: 2 }).waitFor()
  // Every row's control is named by the row: a switch by its words, a button described by them.
  const settingsSection = async (name) => {
    await sections.getByRole('button', { name, exact: true }).click()
    await page.getByRole('heading', { name, level: 2 }).waitFor()
  }

  // Advanced says it is safe to leave alone. The computer's own codecs are
  // used unless switched off, and it says what that means.
  await settingsSection('Advanced')
  assert.equal(await text('.kv-intro'), 'Most people never need anything here.')
  assert.equal(await page.locator('.kinds-table').count(), 0, 'the kinds of file are a page of their own')
  const codecs = page.getByRole('switch', { name: 'Use this computer’s own picture codecs' })
  assert.equal(await codecs.getAttribute('aria-checked'), 'true', 'on unless switched off')
  assert.match(await codecs.evaluate((el) => document.getElementById(el.getAttribute('aria-describedby')).textContent), /Switch this off only if previews look wrong\./, 'described by its sentence')
  assert.match(await tipSays('Opening photos'), /Photos are opened by: this computer’s own codecs, then the built-in decoder/)
  await codecs.click()
  assert.equal(await codecs.getAttribute('aria-checked'), 'false')
  await page.waitForFunction(() => document.querySelector('#setting-codecs [role=switch]')?.getAttribute('aria-checked') === 'false')
  assert.match(await tipSays('Opening photos'), /Photos are opened by: the built-in decoder, then/)
  await codecs.click()
  await shot('9g-settings-advanced')
  // AI agents: a page of its own, looking only and no folders until the person says.
  assert.match(await page.locator('#setting-agents').innerText(), /AI agents\s+What an AI app, such as Claude, may do\.\s+Looking only/)
  await page.getByRole('button', { name: 'AI agents Looking only' }).click()
  await page.getByRole('heading', { name: 'AI agents', level: 2 }).waitFor()
  assert.equal(await page.evaluate(() => document.activeElement?.textContent), 'AI agents', 'the page’s heading has the focus')
  const agentChanges = page.getByRole('switch', { name: 'Let AI agents make changes' })
  assert.equal(await agentChanges.getAttribute('aria-checked'), 'false', 'an agent may only look until allowed')
  await page.getByText('None yet, so it cannot look for photos anywhere.').waitFor()
  await agentChanges.click()
  assert.equal(await agentChanges.getAttribute('aria-checked'), 'true')
  await page.locator('.agent-folders').nth(0).getByRole('button', { name: 'Add a folder…' }).click()
  await page.getByRole('list', { name: 'Folders an agent may look through' }).getByText('D:\\Photos').waitFor()
  await page.locator('.agent-folders').nth(1).getByRole('button', { name: 'Add a folder…' }).click()
  await page.getByRole('list', { name: 'Folders an agent may back up to' }).getByText('F:\\Family Backup').waitFor()
  await shot('9s-settings-agents')
  await page.getByRole('button', { name: 'Stop allowing D:\\Photos' }).click()
  await page.getByText('None yet, so it cannot look for photos anywhere.').waitFor()
  await agentChanges.click()
  assert.equal(await agentChanges.getAttribute('aria-checked'), 'false')
  // Escape leaves the page, back to the row that opened it.
  await page.keyboard.press('Escape')
  await page.getByRole('heading', { name: 'Advanced', level: 2 }).waitFor()
  assert.equal(await page.evaluate(() => document.activeElement?.id), 'setting-agents', 'back on the row that opened it')

  // Appearance: light, dark, or the same as the computer, and it is remembered.
  await settingsSection('Appearance')
  const ground = () => page.evaluate(() => getComputedStyle(document.body).backgroundColor)
  assert.equal(await ground(), 'rgb(255, 254, 251)', 'the cream ground of the light theme')
  assert.equal(await page.getByRole('button', { name: 'Match my computer' }).getAttribute('aria-pressed'), 'true')
  await page.getByRole('button', { name: 'Dark', exact: true }).click()
  assert.equal(await ground(), 'rgb(16, 39, 37)', 'the deep teal ground of the dark theme')
  await shot('9f-settings-dark')
  await settingsSection('This backup')
  await shot('9f2-settings-this-backup-dark')
  await settingsSection('Appearance')
  await page.getByRole('button', { name: 'Light', exact: true }).click()
  assert.equal(await ground(), 'rgb(255, 254, 251)')
  assert.equal(await page.evaluate(() => localStorage.getItem('keevoro-theme')), 'light', 'the choice is remembered')
  await page.getByRole('button', { name: 'Match my computer' }).click()
  assert.equal(await page.evaluate(() => document.documentElement.dataset.theme), undefined)
  assert.equal(await page.evaluate(() => getComputedStyle(document.body).fontFamily.includes('Keevoro Sans')), true)
  // Leaving Settings, the logo is the one for dark grounds while dark, and back again.
  await page.getByRole('button', { name: '‹ Back', exact: true }).click()
  await page.getByRole('button', { name: 'Settings' }).click()
  await page.getByRole('heading', { name: 'Appearance', level: 2 }).waitFor()
  await page.getByRole('button', { name: 'Dark', exact: true }).click()
  await page.getByRole('button', { name: '‹ Back', exact: true }).click()
  assert.match(await page.locator('.brand img:visible').getAttribute('src'), /lockup-reversed/, 'the logo for dark grounds')
  await page.getByRole('button', { name: 'Settings' }).click()
  // Settings opens where it was left.
  await page.getByRole('heading', { name: 'Appearance', level: 2 }).waitFor()
  await page.getByRole('button', { name: 'Match my computer' }).click()

  // Updates: which Keevoro this is, and a way to ask.
  await settingsSection('Updates')
  await page.getByRole('button', { name: 'Check for updates' }).click()
  await page.getByText('That is the newest version.').waitFor()
  await shot('9j-settings-updates')

  // --- Search finds a row by its words, or others like them, says which
  // place and section it is in, and goes there with the row marked.
  const searchBox = page.getByRole('searchbox', { name: 'Search Settings' })
  await searchBox.fill('dark mode')
  const results = page.getByRole('list', { name: 'Found in Settings and Help' })
  assert.match(await results.innerText(), /^Light or dark\s+Settings, Appearance$/)
  await searchBox.fill('notifications')
  assert.match(await results.getByRole('button').first().innerText(), /^Tell me when a backup by itself finishes\s+Settings, Backing up by itself$/)
  // It finds what is in Help too, says so, and goes there.
  await searchBox.fill('compare')
  assert.match(await results.getByRole('button').first().innerText(), /^Compare cloud storage providers\s+Help$/)
  await searchBox.fill('licences')
  assert.match(await results.getByRole('button').first().innerText(), /^Open source licences\s+Help, About Keevoro$/)
  await searchBox.fill('report')
  assert.match(await results.getByRole('button').first().innerText(), /^Report a problem or suggest an idea\s+Help$/)
  await searchBox.fill('about')
  await shot('9k-settings-search-help')
  await searchBox.press('Enter')
  await page.getByRole('heading', { name: 'About Keevoro', level: 2 }).waitFor()
  await page.getByRole('heading', { name: 'Help', level: 1 }).waitFor()
  assert.equal(await page.evaluate(() => document.activeElement?.id), 'settings-page-title', 'the page’s heading has the focus')
  // And from Help, what is in Settings.
  const helpSearch = page.getByRole('searchbox', { name: 'Search Help' })
  await helpSearch.fill('reset')
  assert.match(await results.getByRole('button').first().innerText(), /^Reset Keevoro\s+Settings, Advanced$/)
  await results.getByRole('button').first().click()
  await page.getByRole('heading', { name: 'Settings', level: 1 }).waitFor()
  await page.getByRole('heading', { name: 'Advanced', level: 2 }).waitFor()
  await page.locator('#setting-reset[data-marked]').waitFor()
  await searchBox.fill('heic')
  await shot('9k-settings-search')
  await results.getByRole('button', { name: /own picture codecs/ }).click()
  await page.getByRole('heading', { name: 'Advanced', level: 2 }).waitFor()
  await page.locator('#setting-codecs[data-marked]').waitFor()
  assert.equal(await searchBox.inputValue(), '', 'the search is cleared')
  assert.equal(await page.evaluate(() => document.activeElement?.getAttribute('role')), 'switch', 'the row’s switch has the focus')
  await searchBox.fill('zebra')
  await page.getByText('Nothing in Settings or Help matches “zebra”.').waitFor()
  await searchBox.press('Escape')
  assert.equal(await searchBox.inputValue(), '')
  await sections.waitFor()

  // --- A whole backup can be photos only, or videos only.
  await settingsSection('This backup')
  const backupWants = page.getByRole('group', { name: 'This backup is for' })
  assert.equal(await backupWants.getByRole('switch', { name: 'Photos' }).isChecked(), true)
  assert.equal(await backupWants.getByRole('switch', { name: 'Videos' }).isChecked(), true)
  assert.equal(await backupWants.getByRole('switch', { name: 'Other files' }).count(), 0, 'no other files to choose until a kind of them is listed')
  const wantsGroup = page.getByRole('group', { name: 'What it backs up' })
  const beforeSaid = await page.getByRole('group', { name: 'How files are arranged' }).boundingBox()
  await backupWants.getByRole('switch', { name: 'Photos' }).uncheck()
  await wantsGroup.getByRole('status').getByText(/This backup is for videos only\. Anything already backed up stays backed up; it is just no longer kept/).waitFor()
  assert.deepEqual(await page.getByRole('group', { name: 'How files are arranged' }).boundingBox(), beforeSaid, 'nothing below moves when it is said')
  await shot('9y-settings-videos-only')
  await backupWants.getByRole('switch', { name: 'Photos' }).check()
  await wantsGroup.getByRole('status').getByText(/^This backup is for photos and videos\. This counts at once/).waitFor()

  // --- Advanced: a folder no backup goes to any more can still be cleared
  // of a stopped backup's working files: chosen, then said, then cleared, in the page.
  await settingsSection('Advanced')
  await page.getByRole('button', { name: 'Choose a folder…' }).click()
  const oldFolder = page.getByRole('group', { name: 'Unfinished copies in G:\\Old Keevoro backup' })
  assert.match(await oldFolder.innerText(), /^There are 5\.5 GB of unfinished copies \(211 files\) in G:\\Old Keevoro backup, left by a backup that was stopped part-way\. Clearing them touches nothing that was backed up\./)
  await oldFolder.scrollIntoViewIfNeeded()
  await shot('9z-settings-clear-unfinished')
  await fitsNarrow('Clearing unfinished copies, in Settings')
  await oldFolder.getByRole('button', { name: 'Clear them' }).click()
  await oldFolder.getByText(/^Cleared 5\.5 GB of unfinished copies \(211 files\) from G:\\Old Keevoro backup\./).waitFor()
  assert.match(await oldFolder.innerText(), /Left 1 thing written to in the last hour, as a backup may be using it right now\. Try again later\./)
  await oldFolder.getByRole('button', { name: 'Done' }).click()
  await page.getByRole('button', { name: 'Choose a folder…' }).click()
  await oldFolder.getByText('There are no unfinished copies to clear in G:\\Old Keevoro backup.').waitFor()
  await oldFolder.getByRole('button', { name: 'Done' }).click()

  // --- Advanced, Kinds of file: the kinds looked for, on a page of their own.
  await page.getByRole('button', { name: 'Kinds of file', exact: true }).click()
  await page.getByRole('heading', { name: 'Kinds of file Keevoro looks for', level: 2 }).waitFor()
  const kindsTable = page.locator('.kinds-table')
  const row = (ending) => kindsTable.getByRole('row').filter({ has: page.locator(`td.ending > span:text-is("${ending}")`) })
  const lessCommon = page.getByRole('switch', { name: 'Show less common kinds' })
  const findKind = page.getByRole('searchbox', { name: 'Find a kind of file' })
  assert.match(await row('.jpg').innerText(), /Photo\s+Built in/)
  assert.match(await row('.xyz').innerText(), /Photo\s+Yours/)
  assert.equal(await row('.jpg').getByRole('button', { name: 'Remove .jpg…' }).count(), 0, 'a built-in kind cannot be removed')
  // Only the usual kinds to begin with; the rest are counted, and wait behind one switch.
  assert.equal(await lessCommon.getAttribute('aria-checked'), 'false')
  assert.equal(await row('.tga').count(), 0, 'a less common kind is not listed until asked for')
  assert.equal(await row('.dat').count(), 0)
  assert.match(await text('.kinds-counts'), /^\d+ usual kinds · \d+ less common$/)
  // The box, the switch and the count share one centre line, the count against the list's right edge.
  const findRow = ['.kinds-find input[type=search]', '.kinds-find .kv-toggle', '.kinds-find .kv-toggle > span', '.kinds-find .kinds-counts']
  assert.ok((await centresApart(findRow)) <= 1.5, `the row above the list shares one centre line (${await centresApart(findRow)} apart)`)
  const edges = await page.evaluate(() => [document.querySelector('.kinds-counts'), document.querySelector('.kinds-table')].map((el) => el.getBoundingClientRect().right))
  assert.ok(Math.abs(edges[0] - edges[1]) <= 1, `the count ends at the list's edge (${edges})`)
  // In each row of the list, the ending and its switch share a centre line too.
  const offCentre = await kindsTable.locator('tbody tr:not(.removing):not(.editing)').evaluateAll((rows) =>
    rows
      .map((tr) => {
        const middle = (el) => (el ? el.getBoundingClientRect().top + el.getBoundingClientRect().height / 2 : null)
        const words = middle(tr.querySelector('td.ending > span:not(.info-tip)'))
        const knob = middle(tr.querySelector('.kv-switch'))
        return Math.abs(words - knob) > 1.5 ? tr.querySelector('td.ending > span:not(.info-tip)').textContent : null
      })
      .filter(Boolean),
  )
  assert.deepEqual(offCentre, [], 'each switch is on its row’s centre line')
  // A search finds a less common kind even while they are hidden.
  await findKind.fill('.TGA')
  await row('.tga').waitFor()
  assert.match(await row('.tga').innerText(), /Photo\s+Built in/)
  assert.equal(await row('.jpg').count(), 0, 'only what was searched for')
  // What a kind is, and whose format, is behind the "i" at the end of its ending; one of your own has none.
  await findKind.fill('')
  assert.equal(await tipSays('.arw', row('.arw')), "Sony's raw photo format, from Alpha cameras.")
  assert.equal(await tipSays('.heic', row('.heic')), 'The photo format iPhones and many other phones use.')
  assert.equal(await row('.xyz').locator('.info-tip').count(), 0, 'a kind of your own says nothing it does not know')
  assert.equal(await kindsTable.locator('td.ending:not(:has(.info-tip))').count(), 1, 'every built-in kind has its “i”, only .xyz has none')
  // Searching finds words in what a kind is: a maker finds its kinds, the less common ones too.
  await findKind.fill('Sony')
  await row('.srf').waitFor()
  assert.deepEqual(await kindsTable.locator('td.ending > span:not(.info-tip)').allInnerTexts(), ['.arw', '.mts', '.srf', '.sr2'])
  await findKind.fill('iphone')
  await row('.heic').waitFor()
  await findKind.fill('Canon')
  await row('.crw').waitFor()
  assert.deepEqual(await kindsTable.locator('td.ending > span:not(.info-tip)').allInnerTexts(), ['.cr2', '.cr3', '.crw', '.crm'])
  await findKind.fill('nothing')
  await page.getByText('No kind ending .nothing is on the list.', { exact: false }).waitFor()
  await findKind.fill('')
  // The switch shows them all, and is remembered on this computer.
  await lessCommon.click()
  await row('.tga').waitFor()
  await page.goto(`${base}/?fast&page=settings/advanced/kinds`)
  await page.getByRole('heading', { name: 'Kinds of file Keevoro looks for', level: 2 }).waitFor()
  assert.equal(await lessCommon.getAttribute('aria-checked'), 'true', 'remembered')
  // An ending that is often something else is off, says why, and can be switched on.
  const dat = page.getByRole('switch', { name: 'Look for .dat files' })
  assert.equal(await dat.getAttribute('aria-checked'), 'false', 'off as Keevoro comes')
  assert.match(await tipSays('.dat', row('.dat')), /^Video from old Video CDs, but often any other data, so off\. While off, a file like this is still kept/)
  await dat.click()
  assert.equal(await dat.getAttribute('aria-checked'), 'true')
  await settled()
  await shot('9t0-settings-kinds-less-common')
  await fitsNarrow('The kinds list, less common ones shown')
  await lessCommon.click()
  assert.equal(await row('.tga').count(), 0)
  assert.equal(await row('.dat').count(), 1, 'one you changed stays in sight')
  // Switching one off says when it counts, and offers the way to the sources.
  const avi = page.getByRole('switch', { name: 'Look for .avi files' })
  const tableAt = await kindsTable.boundingBox()
  await avi.click()
  assert.equal(await avi.getAttribute('aria-checked'), 'false')
  await page.getByRole('status').getByRole('button', { name: 'Go to your sources' }).waitFor()
  assert.deepEqual(await kindsTable.boundingBox(), tableAt, 'the table does not move when it is said')
  // Adding: a refusal is said beside the box, in plain words.
  await page.getByRole('button', { name: 'Add a kind of file…' }).click()
  const adding = page.getByRole('form', { name: 'Add a kind of file' })
  await adding.getByLabel('Ending').fill('my pics')
  await adding.getByRole('button', { name: 'Add' }).click()
  await adding.getByRole('alert').getByText(/Use letters and numbers only/).waitFor()
  await adding.getByLabel('Ending').fill('.ABC')
  await adding.getByLabel('They are').selectOption('video')
  await adding.getByRole('button', { name: 'Add' }).click()
  assert.match(await row('.abc').innerText(), /Video\s+Yours/)
  // Editing a built-in kind: photo or video, but its ending stays.
  await page.getByRole('button', { name: 'Change .jpeg', exact: true }).click()
  const editing = page.getByRole('form', { name: 'Change .jpeg', exact: true })
  assert.equal(await editing.getByLabel('Ending').isDisabled(), true, 'a built-in ending cannot be renamed')
  await editing.getByRole('button', { name: 'Cancel' }).click()
  // Removing one of your own is asked about on the page, never in a pop-up.
  await page.getByRole('button', { name: 'Remove .xyz…' }).click()
  await page.getByRole('alertdialog', { name: 'Remove .xyz?' }).waitFor()
  await row('.abc').scrollIntoViewIfNeeded()
  await shot('9t-settings-kinds-of-file')
  await page.getByRole('button', { name: 'Yes, take it off' }).click()
  assert.equal(await row('.xyz').count(), 0)
  // Undoing your changes says exactly what it will do, in place, before anything changes.
  const undoButton = page.getByRole('button', { name: 'Undo my changes…' })
  assert.match(await tipSays('undoing your changes'), /You see exactly what will change before anything does/)
  await undoButton.click()
  const undoAsk = page.getByRole('alertdialog', { name: 'Undo my changes?' })
  assert.deepEqual(await undoAsk.locator('li').allInnerTexts(), [
    '1 kind you added will be taken off the list: .abc.',
    '1 kind you switched off will be switched back on: .avi.',
    '1 kind you switched on will be switched off again: .dat.',
  ])
  assert.match(await undoAsk.innerText(), /Nothing already found or backed up is touched\./)
  assert.equal(await avi.getAttribute('aria-checked'), 'false', 'nothing has changed yet')
  await undoAsk.scrollIntoViewIfNeeded()
  await shot('9t2-settings-kinds-undo')
  await undoAsk.getByRole('button', { name: 'Cancel' }).click()
  assert.equal(await row('.abc').count(), 1, 'cancelled, nothing undone')
  await undoButton.click()
  await undoAsk.getByRole('button', { name: 'Undo my changes', exact: true }).click()
  assert.equal(await avi.getAttribute('aria-checked'), 'true', 'every built-in kind is as it came')
  assert.equal(await row('.abc').count(), 0, 'your own kinds are taken off')
  assert.equal(await undoButton.count(), 0, 'with nothing changed, there is nothing to undo')
  // And can be put back while the page is open.
  await page.getByRole('status').getByRole('button', { name: 'Put my changes back' }).click()
  await row('.abc').waitFor()
  assert.equal(await avi.getAttribute('aria-checked'), 'false')
  assert.equal(await undoButton.count(), 1)
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
  await page.getByRole('navigation', { name: 'Settings sections' }).getByRole('button', { name: 'Advanced' }).click()
  await page.getByRole('button', { name: 'Kinds of file', exact: true }).click()
  await page.getByRole('button', { name: 'Add a kind of file…' }).click()
  const addOther = page.getByRole('form', { name: 'Add a kind of file' })
  await addOther.getByLabel('Ending').fill('pdf')
  await addOther.getByRole('button', { name: 'Add' }).click()
  // A document is never a photo: the refusal offers the way that works.
  await addOther.getByRole('alert').getByText(/never photos or videos\. To back them up as they are, add \.pdf as an other file\./).waitFor()
  await addOther.getByRole('button', { name: 'Add it as other files' }).click()
  const otherKinds = page.locator('.kinds-table')
  const otherRow = (ending) => otherKinds.getByRole('row').filter({ has: page.locator(`td.ending > span:text-is("${ending}")`) })
  assert.match(await otherRow('.pdf').innerText(), /Other file\s+Yours/)
  await page.getByRole('button', { name: 'Add a kind of file…' }).click()
  await addOther.getByLabel('Ending').fill('blend')
  await addOther.getByLabel('They are').selectOption('other')
  await addOther.getByText('Backed up exactly, but not shown or grouped by date.').waitFor()
  assert.match(await tipSays('other files', addOther), /such as documents or projects\. Keevoro will back\s+these up exactly, but cannot show them or group them by date\./)
  await addOther.scrollIntoViewIfNeeded()
  await shot('9v-settings-kinds-other-file')
  await addOther.getByRole('button', { name: 'Add' }).click()
  assert.match(await otherRow('.blend').innerText(), /Other file\s+Yours/)
  await otherRow('.blend').scrollIntoViewIfNeeded()
  await shot('9v2-settings-kinds-with-other-files')
  await fitsNarrow('The kinds list with other files')
  // Now "Other files" is a choice for the backup too.
  await page.getByRole('button', { name: '‹ Advanced' }).click()
  await page.getByRole('navigation', { name: 'Settings sections' }).getByRole('button', { name: 'This backup' }).click()
  const forBackup = page.getByRole('group', { name: 'This backup is for' })
  assert.equal(await forBackup.getByRole('switch', { name: 'Other files' }).isChecked(), true, 'everything Keevoro looks for, unless chosen otherwise')
  await forBackup.getByText('Other files are backed up exactly, but cannot be shown or grouped by date.').waitFor()
  await forBackup.scrollIntoViewIfNeeded()
  assert.deepEqual(await centredOnSwitch(forBackup.getByRole('switch')), [])
  await shot('9v3-settings-backup-for-other-files')
  await page.getByRole('button', { name: '‹ Back', exact: true }).click()
  await page.locator('.steps button').first().click()
  await page.getByRole('heading', { name: 'Where are your photos?', level: 1 }).waitFor()
  await page.getByRole('button', { name: 'Look again for new photos' }).click()
  await page.getByRole('button', { name: 'See what was found' }).click()
  await page.getByRole('heading', { name: 'Which of these should we keep?', level: 1 }).waitFor()
  const otherFiles = page.getByRole('region', { name: 'Other files' })
  await otherFiles.waitFor()
  assert.deepEqual(await otherFiles.locator('.folder-words b').allInnerTexts(), ['Documents', 'Tax 2019', 'Garden shed'], 'by folder, as they were arranged')
  assert.match(await otherFiles.innerText(), /^Other files\s+11 files in 3 folders · [\d.]+ MB\. Open a folder to choose file\s+by file\./)
  assert.match(await tipSays('other files', otherFiles), /^Kept as you arranged them\. Keevoro backs these up exactly/)
  assert.match(await text('.tally'), /of [\d,]+ photos, [\d,]+ videos and 11 other files found/)
  assert.match(await text('.next .say'), /^Keeping [\d,]+ files in \d+ collections and 3 folders of other files\.$/)
  await otherFiles.scrollIntoViewIfNeeded()
  await shot('9w-choose-other-files')
  await fitsNarrow('Choose what to keep, with other files')
  // A whole folder is left out with the same switch as a collection.
  const shed = otherFiles.locator('li', { hasText: 'Garden shed' })
  await shed.locator('.toggle').click()
  await shed.locator('.toggle[aria-pressed="false"]').waitFor()
  await shed.locator('.toggle').click()
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
  await page.getByRole('heading', { name: /^6 folders on Windows \(C:\) seem to hold photos, videos and other files/ }).waitFor()
  assert.match(await page.locator('.hints li.suggested', { hasText: 'Documents' }).innerText(), /4,120 other files/)
  await shot('9w3-survey-other-files')
  await page.getByRole('button', { name: 'See where the space goes' }).click()
  await page.getByRole('heading', { name: 'Where the space goes on Windows (C:)', level: 1 }).waitFor()
  await page.locator('.space-list').getByRole('button', { name: 'Users', exact: true }).click()
  await page.locator('.space-list').getByRole('button', { name: 'You', exact: true }).click()
  await page.getByRole('grid', { name: 'What is in You, the biggest first' }).waitFor()
  await page.getByRole('list', { name: 'Key' }).getByText('Other files to carry along').waitFor()
  assert.match(await text('.space .status'), /other files to carry along [\d.]+ GB/)
  await page.getByRole('tab', { name: 'By kind of file' }).click()
  const kindsHere = page.getByRole('table', { name: 'What is in You, by kind of file' })
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
  assert.match(await text('.send .needs.short'), /Needs about [\d.]+ (MB|GB); only 90\.0 MB free\. Free up some space, or back them up somewhere else\./)
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
  await page.getByText(/^0 of your [\d,]+ files are safely there\. [\d,]+ need attention\.$/).waitFor()
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

  // --- One filled button per screen, the thing most people want next: the
  // foot's button is quiet while the screen above shows a filled one.
  await page.goto(`${base}/?fast`)
  await page.locator('.steps button').nth(0).click()
  await page.getByRole('heading', { name: 'Where are your photos?', level: 1 }).waitFor()
  const footFilled = (words) => page.locator('.next .primary.big', { hasText: words })
  const footQuiet = (words) => page.locator('.next .button.big', { hasText: words })
  await footFilled('See what was found').waitFor()
  await page.getByRole('button', { name: /Windows \(C:\)/ }).click()
  await page.getByRole('heading', { name: 'Look through all of Windows (C:)?' }).waitFor()
  await footQuiet('See what was found').waitFor()
  assert.equal(await page.locator('main .primary').count(), 1, 'only the question’s own answer is filled')
  await page.getByRole('button', { name: 'Cancel' }).click()
  await footFilled('See what was found').waitFor()

  // --- The backup screen keeps the few buttons used most in its row, Back
  // up again the one filled, and the rest behind More: a menu, used with
  // the keyboard as well as the pointer, never cut off, and each of its
  // items doing what its button did.
  await page.goto(`${base}/?fast`)
  await page.getByRole('button', { name: 'Choose where they go' }).click()
  await page.getByRole('button', { name: /^A folder/ }).click()
  await page.getByRole('button', { name: 'Back up now' }).click()
  await page.getByText(/^Everything you are keeping is there: [\d,]+ files\.$/).waitFor()
  const actions = page.locator('.panel.send .choices.actions')
  assert.deepEqual(
    await actions.getByRole('button').evaluateAll((all) => all.map((b) => b.childNodes[0].textContent)),
    ['Back up again', 'Check they all arrived', 'Show me the folder', 'More'],
  )
  assert.deepEqual(await actions.locator('.primary').allInnerTexts(), ['Back up again'], 'one filled button')
  await footQuiet('Add another source').waitFor()
  assert.equal(await page.locator('main .primary').count(), 1, 'Add another source at the foot is quiet')
  const more = actions.getByRole('button', { name: 'More' })
  const menu = page.getByRole('menu', { name: 'More' })
  const items = menu.getByRole('menuitem')
  const focused = () => page.evaluate(() => document.activeElement?.textContent)
  assert.equal(await more.getAttribute('aria-haspopup'), 'menu')
  assert.equal(await more.getAttribute('aria-expanded'), 'false')
  await fitsNarrow('The backup screen with More')
  await more.click()
  await menu.waitFor()
  assert.equal(await more.getAttribute('aria-expanded'), 'true')
  assert.equal(await more.getAttribute('aria-controls'), await menu.getAttribute('id'))
  assert.deepEqual(await items.allInnerTexts(), ['Clear unfinished copies…', 'Add another destination', 'Compare providers', 'Back up somewhere else'])
  assert.equal(await focused(), 'Clear unfinished copies…', 'the focus goes to the first item')
  for (const [key, then] of [
    ['ArrowDown', 'Add another destination'],
    ['ArrowDown', 'Compare providers'],
    ['ArrowDown', 'Back up somewhere else'],
    ['ArrowDown', 'Clear unfinished copies…'],
    ['ArrowUp', 'Back up somewhere else'],
    ['Home', 'Clear unfinished copies…'],
    ['End', 'Back up somewhere else'],
  ]) {
    await page.keyboard.press(key)
    assert.equal(await focused(), then, `${key} goes to ${then}`)
  }
  await shot('9-more-menu')
  await page.emulateMedia({ colorScheme: 'dark' })
  await shot('9-more-menu-dark')
  await page.emulateMedia({ colorScheme: 'light' })
  // Escape closes it and puts the focus back on its button.
  await page.keyboard.press('Escape')
  await menu.waitFor({ state: 'detached' })
  assert.equal(await more.getAttribute('aria-expanded'), 'false')
  assert.equal(await page.evaluate(() => document.activeElement?.getAttribute('aria-haspopup')), 'menu', 'the focus is back on More')
  // Opened from the keyboard: down to the first item, up to the last.
  await page.keyboard.press('ArrowUp')
  await menu.waitFor()
  assert.equal(await focused(), 'Back up somewhere else')
  await page.keyboard.press('Tab')
  await menu.waitFor({ state: 'detached' })
  await page.keyboard.press('ArrowDown')
  assert.equal(await focused(), 'Clear unfinished copies…')
  // A click anywhere else closes it.
  await page.getByRole('heading', { name: 'Where should they go?', level: 1 }).click()
  await menu.waitFor({ state: 'detached' })
  // Never cut off, by the window or by anything it sits in, and in a narrow
  // window too.
  const wholly = () =>
    menu.evaluate((el) => {
      const box = el.getBoundingClientRect()
      const there = document.elementFromPoint(box.left + box.width / 2, box.bottom - 4)
      return box.left >= 16 && box.right <= window.innerWidth - 16 && box.top >= 0 && box.bottom <= window.innerHeight && el.contains(there)
    })
  await more.click()
  assert.ok(await wholly(), 'the menu is wholly in the window, on top')
  await page.keyboard.press('Escape')
  await page.setViewportSize({ width: 400, height: 800 })
  await more.scrollIntoViewIfNeeded()
  await more.click()
  assert.ok(await wholly(), 'at 400 pixels wide too')
  await shot('9-more-menu-400')
  await page.emulateMedia({ colorScheme: 'dark' })
  await shot('9-more-menu-400-dark')
  await page.emulateMedia({ colorScheme: 'light' })
  await page.keyboard.press('Escape')
  await page.setViewportSize({ width: 1440, height: 900 })
  // Each item does what its button did. Clearing unfinished copies, chosen
  // from the keyboard, asks first, with the focus on its question.
  await more.focus()
  await page.keyboard.press('ArrowDown')
  await page.keyboard.press('Enter')
  const clearing = page.getByRole('group', { name: 'Unfinished copies in F:\\Family Backup' })
  await clearing.waitFor()
  assert.equal(await focused(), 'Clear them', 'the focus goes to the question')
  await page.keyboard.press('Enter')
  await clearing.getByText(/^Cleared 320\.4 MB of unfinished copies \(12 files\) from F:\\Family Backup\./).waitFor()
  await clearing.getByRole('button', { name: 'Done' }).click()
  assert.equal(await clearing.count(), 0)
  // Adding another destination opens its choices, and Not now goes back.
  await moreChoice('Add another destination')
  await page.getByRole('heading', { name: 'Add another destination' }).waitFor()
  await page.getByRole('button', { name: 'Not now' }).click()
  await more.waitFor()
  // The comparison of cloud storage is there for a backup that already has
  // somewhere to go; a choice opens its form as another destination.
  await moreChoice('Compare providers')
  const moreCompare = page.getByRole('dialog', { name: 'Compare cloud storage' })
  await moreCompare.waitFor()
  await page.frameLocator('.compare-sheet iframe').getByRole('button', { name: 'Pick R2 Standard' }).click()
  await moreCompare.getByRole('button', { name: 'Set up Cloudflare R2 in Keevoro' }).click()
  await moreCompare.waitFor({ state: 'detached' })
  await page.getByRole('heading', { name: 'Add another destination' }).waitFor()
  await page.getByLabel('Cloudflare account ID').waitFor()
  await shot('9-more-compare-set-up')
  await page.getByRole('button', { name: 'Not now' }).click()
  await more.waitFor()
  // Backing up somewhere else takes this one off, deleting nothing, and
  // asks where instead.
  await more.click()
  assert.equal(
    await menu.getByRole('menuitem', { name: 'Back up somewhere else' }).getAttribute('title'),
    'Stops backing up here, so you can choose somewhere else. Nothing backed up there is deleted.',
  )
  await menu.getByRole('menuitem', { name: 'Back up somewhere else' }).click()
  await page.getByRole('button', { name: /^A folder/ }).waitFor()
  assert.equal(await page.getByRole('heading', { name: /^Backing up to/ }).count(), 0)
  // With several places, each has its own Clear and Remove in the list, so
  // More keeps only Add another destination.
  await page.getByRole('button', { name: /^A folder/ }).click()
  await page.getByRole('heading', { name: 'Backing up to the folder F:\\Family Backup' }).waitFor()
  await moreChoice('Add another destination')
  await page.getByRole('button', { name: /Cloudflare R2/ }).click()
  await page.getByLabel('Cloudflare account ID').fill('0123456789abcdef0123456789abcdef')
  await page.getByLabel('Bucket name').fill('family-photos')
  await page.getByLabel('Access key ID').fill('key')
  await page.getByLabel('Secret access key').fill('secret')
  await page.locator('.send form button[type=submit]').click()
  await page.getByRole('heading', { name: 'Backing up to 2 places' }).waitFor()
  await more.click()
  assert.deepEqual(await items.allInnerTexts(), ['Add another destination', 'Compare providers'])
  await page.keyboard.press('Escape')

  // --- A share that drops out during a backup: one grouped sentence, said
  // as the destination's, an account that adds up, and the main button
  // saying what it will do next.
  await page.goto(`${base}/?fast`)
  await page.getByRole('button', { name: 'Choose where they go' }).click()
  await page.evaluate(() => window.keevoroDropTheShareNextBackup())
  await page.getByRole('button', { name: /^A folder/ }).click()
  await page.getByRole('button', { name: 'Back up now' }).click()
  await page.getByText(/^Couldn't back up 5 files, because the connection to the folder F:\\Family Backup was lost for a while: IMG_1001\.jpg, IMG_1002\.jpg, IMG_1003\.jpg and 2 more\.$/).waitFor()
  await page.getByText(/^[\d,]+ of the [\d,]+ files you are keeping are safely at the folder F:\\Family Backup\. 5 still need attention: 5 couldn't be sent\.$/).waitFor()
  assert.equal(await page.getByText(/from its source/).count(), 0, 'a destination\u2019s trouble is never said as the source\u2019s')
  await page.getByRole('button', { name: 'Back up the 5 files that were missed' }).waitFor()
  await shot('9w-backup-share-dropped-out')
  await page.getByRole('button', { name: 'Back up the 5 files that were missed' }).click()
  await page.getByText(/^All [\d,]+ files you are keeping are safely at the folder F:\\Family Backup\.$/).waitFor()

  // --- The owner's network share as 0.42.0 left it: some files gone, some
  // cut short. The screen says how many are safely there and how many need
  // attention, never "everything", and one button puts it right.
  await page.goto(`${base}/?fast`)
  await page.getByRole('button', { name: 'Choose where they go' }).click()
  await page.getByRole('button', { name: /^A folder/ }).click()
  await page.getByRole('button', { name: 'Back up now' }).click()
  await page.getByText(/^All [\d,]+ files you are keeping are safely at the folder F:\\Family Backup\.$/).waitFor()
  await page.getByText(/^Everything you are keeping is there: [\d,]+ files\.$/).waitFor()
  await page.getByRole('button', { name: 'Back up again' }).click()
  await page.getByText(/^[\d,]+ files already there\.$/).waitFor()
  await page.getByText(/^All [\d,]+ files you are keeping are safely at the folder F:\\Family Backup\.$/).waitFor()
  await shot('9z0-second-backup-adds-up')
  await page.evaluate(() => window.keevoroDamageTheShare())
  await page.locator('.steps button').nth(1).click()
  await page.getByRole('button', { name: 'Next' }).click()
  const damaged = page.getByRole('status', { name: 'What Keevoro found where this backup goes' })
  await damaged.getByText(/^The folder F:\\Family Backup no longer holds 2 of the [\d,]+ files Keevoro backed up there\. They will be sent again\.$/).waitFor()
  await damaged.getByText(/under the names Keevoro gave them, but not the sizes they were backed up at/).waitFor()
  await page.getByText(/^[\d,]+ of your [\d,]+ files are safely there\. 5 need attention\.$/).waitFor()
  assert.equal(await page.getByText(/^Everything you are keeping is there/).count(), 0, 'never everything beside files that are not')
  assert.deepEqual(
    await page.locator('main .primary').allInnerTexts(),
    ['Put this backup right…'],
    'putting it right is the one filled button; Back up again is quiet beside it',
  )
  await shot('9z1-backup-needs-putting-right')
  await fitsNarrow('A backup that needs putting right')
  await page.getByRole('button', { name: 'Put this backup right…' }).click()
  await page.getByText(/^First, Keevoro puts right the 5 files it already knows are wrong/).waitFor()
  await page.getByText(/into a folder called "Set aside by Keevoro"/).waitFor()
  await shot('9z2-put-this-backup-right-plan')
  await fitsNarrow('What putting a backup right will do')
  await narrowShots('9z2b-put-this-backup-right-plan', page.getByRole('region', { name: 'Put this backup right' }))
  await page.getByRole('button', { name: /^Put it right and check every file/ }).click()
  await page.getByText(/^Moved 3 damaged copies aside into "Set aside by Keevoro\/2026-11-22"/).waitFor()
  await page.getByText(/^Your backup at the folder F:\\Family Backup is right: all [\d,]+ files you are keeping are there/).waitFor()
  await page.getByText(/^Everything you are keeping is there: [\d,]+ files\.$/).waitFor()
  assert.equal(await page.getByRole('button', { name: 'Put this backup right…' }).count(), 0, 'offered only when something needs it')
  await shot('9z3-backup-put-right')
  // With nothing known to be wrong, the full proof is still a quiet press
  // away, its sentence in a column of its own beside it: never running on
  // from the button, and never wrapping beneath it.
  const prove = page.getByRole('button', { name: 'Prove every copy…' })
  // What it does, and what it costs, in view, beside Check they all arrived's quicker look.
  assert.equal(
    await prove.getAttribute('aria-describedby').then((id) => page.locator(`[id="${id}"]`).innerText()),
    'The thorough check: reads every copy back against its original. Slower; it says how long first.',
  )
  assert.equal(await page.locator('.explained.prove .info-tip').count(), 0)
  const beside = await page.locator('.explained.prove').evaluate((row) => {
    const said = row.querySelector('.explained-said').getBoundingClientRect()
    const button = row.querySelector('.explained-action button').getBoundingClientRect()
    return { gap: Math.round(button.left - said.right), sameRow: button.top < said.bottom }
  })
  assert.ok(beside.sameRow && beside.gap >= 12, `the button sits clear of its sentence (${JSON.stringify(beside)})`)
  await fitsNarrow('Prove every copy, with its sentence')
  await narrowShots('9z3b-prove-every-copy', page.locator('.explained.prove'))
  await prove.click()
  await page.getByText(/^Keevoro reads back in full each of the [\d,]+ files you are keeping at the folder F:\\Family Backup/).waitFor()
  await shot('9z4-prove-every-copy')
  await page.getByRole('button', { name: 'Not now' }).click()

  // --- On a Mac or Linux an unplugged drive can leave the folder it was
  // mounted on behind, empty, on the computer's own disk. That is never
  // backed up into by itself; the person can say this disk is the one.
  await page.goto(`${base}/?fast`)
  await page.getByRole('button', { name: 'Choose where they go' }).click()
  await page.getByRole('button', { name: /^A folder/ }).click()
  await page.getByRole('button', { name: 'Back up now' }).click()
  await page.getByText(/^Everything you are keeping is there: [\d,]+ files\.$/).waitFor()
  await page.evaluate(() => window.keevoroUnplugLeavingTheFolder())
  await page.locator('.steps button').nth(1).click()
  await page.getByRole('button', { name: 'Next' }).click()
  const leftBehind = page.getByRole('status', { name: 'What Keevoro found where this backup goes' })
  await leftBehind.getByText(/^The drive this backup goes to does not seem to be plugged in: F:\\Family Backup is there, but on another disk/).waitFor()
  await shot('9y2-drive-left-its-folder-behind')
  await fitsNarrow('A drive that left its folder behind')
  await leftBehind.getByRole('button', { name: 'Use this folder as it is now' }).click()
  await leftBehind.getByText(/looks emptied or replaced/).waitFor()

  // --- A search can be stopped, and then nothing from it is kept.
  await page.goto(`${base}/?demo=empty&slow`)
  await page.getByRole('heading', { name: 'Where are your photos?', level: 1 }).waitFor()
  await page.getByRole('button', { name: /KINGSTON/ }).click()
  await page.getByRole('button', { name: 'Stop looking' }).click()
  await page.getByRole('heading', { name: 'Stopped looking through KINGSTON (E:)' }).waitFor()
  assert.match(await text('.panel'), /Nothing was added, as only part of it was looked at\. Choose it again, or a folder inside it\./)
  assert.equal(await page.locator('.kept li.source').count(), 0, 'the source was not added')
  assert.ok(await page.locator('.steps button').nth(1).isDisabled(), 'and there is nothing to choose from')
  await shot('9j-search-stopped')

  // --- A drive plugged in appears by itself within a couple of seconds,
  // first and brought to notice, never looking chosen; Refresh is a button.
  await page.goto(`${base}/?demo=empty&plugin`)
  await page.getByRole('heading', { name: 'Where are your photos?', level: 1 }).waitFor()
  await page.getByRole('button', { name: 'Refresh the list of drives' }).waitFor()
  assert.equal(await page.locator('.place').filter({ hasText: 'SanDisk' }).count(), 0, 'not plugged in yet')
  const arrivedCard = page.locator('.place.arrived').filter({ hasText: 'SanDisk Ultra (F:)' })
  await arrivedCard.waitFor()
  assert.match(await arrivedCard.innerText(), /Just plugged in/)
  assert.match(await page.locator('.places .place').first().innerText(), /SanDisk Ultra \(F:\)/, 'it comes first')
  const shade = await arrivedCard.evaluate((el) => getComputedStyle(el).backgroundColor)
  const other = await page.locator('.place').filter({ hasText: 'KINGSTON' }).evaluate((el) => getComputedStyle(el).backgroundColor)
  assert.equal(shade, other, 'it is not shaded as if chosen')
  await shot('2c-a-drive-just-plugged-in')
  await fitsNarrow('A drive just plugged in')
  await page.getByRole('button', { name: 'Refresh the list of drives' }).click()
  await page.locator('.place').filter({ hasText: 'SanDisk Ultra (F:)' }).waitFor()

  // --- A drive pulled out while it is looked through is said plainly, and
  // what was found so far is kept.
  await page.goto(`${base}/?demo=empty&unplug`)
  await page.getByRole('heading', { name: 'Where are your photos?', level: 1 }).waitFor()
  await page.getByRole('button', { name: /KINGSTON/ }).click()
  await page.getByRole('heading', { name: 'Stopped looking through KINGSTON (E:)' }).waitFor()
  assert.match(
    await text('.panel'),
    /KINGSTON \(E:\) was unplugged\. What was found so far is kept; plug it in and look again to carry on\./,
  )
  assert.equal(await page.locator('.kept li.source').count(), 1, 'what was found so far is kept')
  await shot('9j1-search-drive-unplugged')

  // --- A backup that ran by itself says so in its history, with what it
  // passed over and what waits for the person's say.
  await page.goto(`${base}/?byitself&fast`)
  await page.getByRole('heading', { name: 'Where should they go?', level: 1 }).waitFor()
  await page.getByText('How earlier backups went').click()
  const byItselfRun = page.locator('.sends > .sends-scroll > table > tbody > tr').first()
  await byItselfRun.getByRole('button', { name: /^Show everything/ }).click()
  const byItselfPanel = page.locator('.run-detail')
  assert.match(await byItselfPanel.innerText(), /Started by itself for a system scheduler or a script, \d+ Nov, \d\d:\d\d, from the command line, with Keevoro 0\.42\.0\. It finished\./)
  assert.match(await byItselfPanel.innerText(), /It looked through Family hard drive again first\./)
  assert.match(await byItselfPanel.innerText(), /KINGSTON \(E:\) is not plugged in, so it was not looked through\./)
  assert.match(await byItselfPanel.innerText(), /Waiting for your say\s+2 new pictures were set aside as probably not photos/)
  await byItselfPanel.scrollIntoViewIfNeeded()
  await shot('9v1-a-backup-that-ran-by-itself')
  await fitsNarrow('A backup that ran by itself, in its history')

  // --- Backing up by itself: set up from the backup screen, in Keevoro's
  // own words, with Skip the next one and Run now.
  await page.goto(`${base}/?byitself&fast`)
  await page.getByRole('heading', { name: 'Where should they go?', level: 1 }).waitFor()
  const scheduleCard = page.locator('.schedule-card')
  await scheduleCard.getByText('Every day, week or month, while Keevoro is open.').waitFor()
  await scheduleCard.getByRole('button', { name: 'Set it up…' }).click()
  const dialog = page.getByRole('dialog', { name: 'Back up “My photos” by itself?' })
  await dialog.waitFor()
  assert.equal(await dialog.getByRole('radio', { name: /^Every day at/ }).isChecked(), true, 'every day, at 02:30, to begin with')
  assert.equal(await dialog.getByLabel('Time each day').inputValue(), '02:30')
  assert.match(await dialog.innerText(), /Each time, Keevoro backs up anything new from your sources\./)
  // When it runs is said in view; the finer points wait behind one "i" at the end of that sentence.
  assert.match(await dialog.innerText(), /It runs while Keevoro is open, and catches up once if\s+the computer was off\./)
  assert.equal(await dialog.locator('details').count(), 0, 'nothing to open')
  const byItselfSaid = await tipSays('backing up by itself', dialog)
  assert.match(byItselfSaid, /Pictures it sets aside, and anything else that needs your say, wait for you\./)
  assert.match(byItselfSaid, /Saving does not start a backup\./)
  await dialog.scrollIntoViewIfNeeded()
  await shot('9x1-backing-up-by-itself-set-up')
  await fitsNarrow('Setting up backing up by itself')
  await dialog.getByRole('radio', { name: /^Every week on/ }).check()
  await dialog.getByLabel('Day of the week').selectOption('sun')
  await dialog.getByLabel('Time each week').selectOption('09:00')
  await dialog.getByRole('button', { name: 'Save' }).click()
  await scheduleCard.getByText('Every week on Sunday at 09:00').waitFor()
  assert.match(await scheduleCard.innerText(), /Next: sunday at 09:00/)
  await scheduleCard.getByRole('button', { name: 'Skip the next one' }).click()
  await scheduleCard.getByText(/which will be skipped, as you asked/).waitFor()
  await scheduleCard.getByRole('button', { name: 'Don’t skip it' }).click()
  await scheduleCard.getByRole('button', { name: 'Skip the next one' }).waitFor()
  // Settings has the same schedule, set with the same dialog.
  await page.getByRole('button', { name: 'Settings' }).click()
  await page.getByRole('navigation', { name: 'Settings sections' }).getByRole('button', { name: 'Backing up by itself' }).click()
  const scheduleRow = page.locator('#setting-schedule')
  assert.match(await scheduleRow.innerText(), /^Back up by itself\s+Looks through your sources for anything new and backs it up, while Keevoro is open\.\s+Every week on Sunday at 09:00\s+Change…$/)
  await scheduleRow.getByRole('button', { name: 'Change…' }).click()
  const settingsDialog = page.getByRole('dialog', { name: 'Back up “My photos” by itself?' })
  assert.equal(await settingsDialog.getByRole('radio', { name: /^Every week on/ }).isChecked(), true, 'as it is set')
  await settingsDialog.getByRole('radio', { name: /^Every day at/ }).check()
  await settingsDialog.getByRole('button', { name: 'Save' }).click()
  await scheduleRow.getByText('Every day at 02:30').waitFor()
  await shot('9x2-settings-backing-up-by-itself')
  await page.getByRole('button', { name: '‹ Back', exact: true }).click()
  await scheduleCard.getByText('Every day at 02:30').waitFor()

  await page.goto(`${base}/?byitself&schedule&fast`)
  await page.getByRole('heading', { name: 'Where should they go?', level: 1 }).waitFor()
  await scheduleCard.getByText('Every day at 02:30').waitFor()
  assert.deepEqual(await scheduleCard.locator('.schedule-last .mark').allInnerTexts(), ['Done', 'Done', 'Couldn’t reach it'])
  assert.match(await scheduleCard.locator('.schedule-last li').last().innerText(), /Keevoro can’t find the folder F:\\Family Backup\. Is it plugged in\? It will catch up when it is\./)
  await scheduleCard.scrollIntoViewIfNeeded()
  await shot('9x2-backing-up-by-itself')
  await fitsNarrow('Backing up by itself, with its last few runs')
  await scheduleCard.getByRole('button', { name: 'Run now' }).click()
  await scheduleCard.locator('.schedule-last li').first().getByText('Safely backed up 12 files (41.0 MB).').waitFor()
  await scheduleCard.getByRole('button', { name: 'Pause' }).click()
  await scheduleCard.getByText('(paused)').waitFor()
  await scheduleCard.getByRole('button', { name: 'Carry on' }).click()
  await scheduleCard.getByText(/^Next: tonight at 02:30/).waitFor()

  // --- While you were away: back after the window was closed, the backup
  // screen says what each backup did by itself, what was missed and
  // caught up, and what waits; Done puts it away.
  await page.goto(`${base}/?byitself&away&fast`)
  const away = page.locator('.away')
  await away.getByRole('heading', { name: 'While you were away', level: 2 }).waitFor()
  assert.deepEqual(await away.locator('h3').allInnerTexts(), ['Family photos', 'Holiday videos', 'Waiting for your say'])
  assert.deepEqual(await away.locator('.away-runs .mark').allInnerTexts(), ['Done.', 'Done.', 'Couldn’t reach it.'])
  await away.getByText('This caught up a run that was missed, perhaps because the computer was off or asleep.').waitFor()
  await away.getByText(/^The run due today at 02:30 didn't happen/).waitFor()
  await away.getByText(/^4 new pictures were set aside/).waitFor()
  await shot('9x2a-while-you-were-away')
  await fitsNarrow('While you were away')
  await narrowShots('9x2a-while-you-were-away', away)
  await away.getByRole('button', { name: 'Done' }).click()
  await away.waitFor({ state: 'detached' })
  await page.goto(`${base}/?byitself&fast`)
  await page.getByRole('heading', { name: 'Where should they go?', level: 1 }).waitFor()
  await settled()
  assert.equal(await page.locator('.away').count(), 0, 'nothing to say, nothing shown')

  // --- A run due that waits for a good moment says why, with Back up now
  // anyway beside it; Settings says what it waits for, and the hours.
  await page.goto(`${base}/?byitself&schedule&waiting&fast`)
  const waitingCard = page.locator('.schedule-card')
  await waitingCard.getByText('Waiting while this computer is on battery. The run due will start by itself once it can.').waitFor()
  await waitingCard.scrollIntoViewIfNeeded()
  await shot('9x2b-waiting-back-up-anyway')
  await fitsNarrow('A run waiting for a good moment')
  await narrowShots('9x2b-waiting-back-up-anyway', waitingCard)
  await waitingCard.getByRole('button', { name: 'Back up now anyway' }).click()
  await waitingCard.locator('.schedule-last li').first().getByText('Safely backed up 12 files (41.0 MB).').waitFor()
  await waitingCard.getByText(/^Waiting while this computer is on battery/).waitFor({ state: 'detached' })
  await page.getByRole('button', { name: 'Settings', exact: true }).click()
  await page.getByRole('navigation', { name: 'Settings sections' }).getByRole('button', { name: 'Backing up by itself' }).click()
  const waits = page.getByRole('group', { name: 'Waiting for a good moment' })
  assert.equal(await waits.getByRole('switch', { name: 'Wait while on battery' }).getAttribute('aria-checked'), 'true')
  assert.equal(await waits.getByRole('switch', { name: 'Wait on a metered connection' }).getAttribute('aria-checked'), 'true')
  await waits.getByRole('switch', { name: 'Only between these hours' }).click()
  await waits.getByText('Starts only between 22:00 and 07:00.').waitFor()
  await waits.locator('.between-times select').first().selectOption('23:00')
  await waits.getByText(/starts only between 23:00 and 07:00/i).waitFor()
  // That only a backup by itself waits is said under the group, in view; its rows have no "i" that would only say their line again.
  assert.equal(await waits.locator('.kv-footnote').innerText(), 'Only a backup by itself waits. One you start yourself, or Back up now anyway, never does.')
  assert.equal(await waits.locator('.info-tip').count(), 0)
  await waits.scrollIntoViewIfNeeded()
  await shot('9x4b-settings-waiting-for-a-good-moment')
  await fitsNarrow('Settings, waiting for a good moment')
  await narrowShots('9x4b-settings-waiting-for-a-good-moment', waits)
  await waits.getByRole('switch', { name: 'Only between these hours' }).click()
  await waits.getByText('Starts at any hour.').waitFor()

  // --- Keevoro by the clock. The first time the window is closed, it says
  // Keevoro keeps running, and where its icon is; starting with the
  // computer is never ticked for the person.
  await page.goto(`${base}/?closing&fast`)
  const explained = page.getByRole('dialog', { name: 'Keevoro keeps running' })
  await explained.waitFor()
  await explained.getByText('It stays by the clock, so your backups happen on time.').waitFor()
  await explained.getByText(/may be under the \^ arrow there/).waitFor()
  assert.equal(await explained.getByRole('switch', { name: 'Start Keevoro when this computer starts' }).isChecked(), false)
  assert.deepEqual(await centredOnSwitch(explained.getByRole('switch')), [], 'its words beside the switch, with no box around them')
  await shot('9x3-closing-keeps-running')
  await fitsNarrow('The first-close explanation')
  await explained.getByRole('switch', { name: 'Start Keevoro when this computer starts' }).check()
  await explained.getByRole('button', { name: 'OK' }).click()
  await explained.waitFor({ state: 'detached' })
  // Settings says the same, under Backing up by itself.
  await page.getByRole('button', { name: 'Settings' }).click()
  await page.getByRole('navigation', { name: 'Settings sections' }).getByRole('button', { name: 'Backing up by itself' }).click()
  const keeping = page.getByRole('group', { name: 'When the window is closed' })
  assert.equal(await keeping.getByRole('switch', { name: 'Keep Keevoro running by the clock' }).getAttribute('aria-checked'), 'true')
  assert.equal(await keeping.getByRole('switch', { name: 'Minimise to the icon by the clock' }).getAttribute('aria-checked'), 'false')
  assert.equal(await keeping.getByRole('switch', { name: 'Start Keevoro when this computer starts' }).getAttribute('aria-checked'), 'true', 'ticked in the explanation')
  await keeping.getByRole('switch', { name: 'Minimise to the icon by the clock' }).click()
  await page.waitForFunction(() => document.querySelector('#setting-minimise [role="switch"]')?.getAttribute('aria-checked') === 'true')
  assert.equal(await page.getByRole('group', { name: 'Notifications' }).getByRole('switch', { name: 'Tell me when a backup by itself finishes' }).getAttribute('aria-checked'), 'true')
  await shot('9x4-settings-when-you-close')
  await fitsNarrow('Settings, Backing up by itself')
  // The explanation's own words lead to these choices, without closing the window.
  await page.goto(`${base}/?closing&fast`)
  await explained.getByRole('button', { name: 'Settings, Backing up by itself' }).click()
  await explained.waitFor({ state: 'detached' })
  await page.getByRole('heading', { name: 'Backing up by itself', level: 2 }).waitFor()
  await page.locator('#setting-keepRunning[data-marked]').waitFor()
  assert.equal(await page.evaluate(() => document.activeElement?.getAttribute('aria-labelledby') && document.getElementById(document.activeElement.getAttribute('aria-labelledby')).textContent), 'Keep Keevoro running by the clock', 'its switch has the focus')
  await shot('9x4b-settings-from-the-explanation')
  // The icon by the clock opens a page by its name.
  await page.goto(`${base}/?fast&page=settings/advanced/kinds`)
  await page.getByRole('heading', { name: 'Kinds of file Keevoro looks for', level: 2 }).waitFor()
  await page.goto(`${base}/?fast&page=settings`)
  await page.getByRole('heading', { name: 'This backup', level: 2 }).waitFor()
  // A notice about a backup by itself, clicked, opens on the backup screen.
  await page.goto(`${base}/?fast&page=send`)
  await page.getByRole('heading', { name: 'Where should they go?', level: 1 }).waitFor()
  // Quitting with a backup going asks first, over the window.
  await page.goto(`${base}/?quitting&busy&fast`)
  const quitAsked = page.getByRole('dialog', { name: 'A backup is running. Stop it after the file in hand and quit? Nothing is lost.' })
  await quitAsked.waitFor()
  await shot('9x5-quit-asks-first')
  await quitAsked.getByRole('button', { name: 'Keep it running' }).click()
  await quitAsked.waitFor({ state: 'detached' })

  // --- A drive can be backed up by itself whenever it is plugged in: off
  // unless the person turns it on, its words beside its switch.
  await page.goto(`${base}/?fast`)
  await page.getByRole('button', { name: /^Find your photos/ }).click()
  await page.getByRole('heading', { name: 'Where are your photos?', level: 1 }).waitFor()
  const whenPluggedIn = page.getByRole('switch', { name: /^Back up .* when it is plugged in$/ })
  assert.equal(await whenPluggedIn.getAttribute('aria-checked'), 'false', 'off unless asked')
  await whenPluggedIn.click()
  await page.locator('.on-arrival').getByText('Backed up by itself a few seconds after it is plugged in, while Keevoro is running.').waitFor()
  assert.equal(await whenPluggedIn.getAttribute('aria-checked'), 'true')
  assert.deepEqual(await centredOnSwitch(whenPluggedIn), [], 'its words beside the switch, with no box around them')
  await page.locator('.kept li.source').first().scrollIntoViewIfNeeded()
  await shot('3d-back-up-when-plugged-in')
  await fitsNarrow('A source backed up when it is plugged in')
  await narrowShots('3d-back-up-when-plugged-in', page.locator('.on-arrival'))
  await whenPluggedIn.click()
  await page.locator('.on-arrival').getByText('Backed up by itself a few seconds after it is plugged in, while Keevoro is running.').waitFor({ state: 'detached' })

  // --- Looking again with a source's card in a drawer passes it over and
  // says so: what was found on it is kept.
  await page.goto(`${base}/?drives=away&fast`)
  await page.getByRole('button', { name: /^Find your photos/ }).click()
  await page.getByRole('heading', { name: 'Where are your photos?', level: 1 }).waitFor()
  await page.getByRole('button', { name: 'Look again for new photos' }).click()
  await page.locator('.passed-over').waitFor()
  assert.equal(
    await text('.passed-over'),
    'KINGSTON (E:) is not plugged in, so it was not looked through. What was found on it before is kept.',
  )
  await page.locator('.passed-over').scrollIntoViewIfNeeded()
  await shot('9j2-look-again-not-plugged-in')
  await fitsNarrow('Looking again with a source not plugged in')

  // --- Another card at a source's place is asked about, never taken for it.
  await page.goto(`${base}/?drives=different&fast`)
  await page.getByRole('button', { name: /^Find your photos/ }).click()
  await page.getByRole('heading', { name: 'Where are your photos?', level: 1 }).waitFor()
  await page.getByRole('button', { name: 'Look again for new photos' }).click()
  const asked = page.getByRole('alertdialog', { name: 'Is this Family hard drive?' })
  await asked.waitFor()
  assert.match(
    await asked.innerText(),
    /This looks like a different drive from the one called Family hard drive\. Add it as a new source, or look through it as Family hard drive anyway\?/,
  )
  assert.deepEqual(await asked.getByRole('button').allInnerTexts(), [
    'Add it as a new source',
    'Look through it as Family hard drive anyway',
    'Not now',
  ])
  assert.equal(await page.locator('.panel.found-it').count(), 0, 'nothing was looked through, so nothing is said to be found')
  await shot('9j3-a-different-drive-asked')
  await fitsNarrow('The question about a different drive')
  await asked.getByRole('button', { name: 'Add it as a new source' }).click()
  await page.getByRole('heading', { name: /^Found .* on KINGSTON \(E:\)$/ }).waitFor()
  const aside = page.locator('.kept li.source').filter({ hasText: 'Family hard drive' })
  assert.match(await aside.innerText(), /Set aside when a different drive was plugged in at E:\\\. Everything found on it is kept/)
  assert.equal(await page.locator('.kept li.source').count(), 2, 'the new drive is a source of its own beside it')
  await aside.scrollIntoViewIfNeeded()
  await shot('9j4-a-source-set-aside')
  await fitsNarrow('A source set aside')

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
      'Working out where each file goes: 2,500 of 8,630',
      'Backing up your files: 30.1 GB',
      'Making previews: 10,547',
      'Backing up the previews: 4.0 GB',
      'Writing the list of what is there',
    ],
  )
  assert.match(await text('.speed'), /Previews are made in the background, and sent after your files\./)
  assert.match(await tipSays('previews', page.locator('.speed')), /switch off Previews for a gallery in Settings, This\s+backup, once this backup ends/)
  assert.equal(await page.getByText(/^Stopped\./).count(), 0, 'no stale "Stopped" while it runs')
  // The screen says what step the backup is on straight away, in its own
  // words, and the activity page says the same.
  await page.locator('.send .running').getByText('Working out where each file goes: 2,500 of 8,630').waitFor()
  assert.equal(await page.locator('.send .done').count(), 0, 'nothing an earlier backup said is left showing')
  // There is always a bar while a backup runs, whatever step it is on.
  await page.locator('.send .speed progress').first().waitFor()
  await shot('9m-came-back-to-a-running-backup')
  await page.getByRole('button', { name: /^What Keevoro is doing/ }).click()
  await page.getByText(/^Right now: Working out where each file goes: 2,500 of 8,630$/).waitFor()
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
  await footHere('What Keevoro is doing')
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
  await footHere('Manage backups')
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
  await page.locator('.removed-recently').getByRole('button', { name: 'Bring it back' }).click()
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
  // Manage backups opens from This backup, and comes back to it.
  await page.getByRole('button', { name: 'Manage backups', exact: true }).click()
  await page.getByRole('heading', { name: 'Your backups', level: 1 }).waitFor()
  await page.getByRole('button', { name: '‹ Back' }).click()
  await page.getByRole('heading', { name: 'This backup', level: 2 }).waitFor()
  // Reset is set apart, on its own at the foot of Advanced, and asks first in the page.
  await page.getByRole('navigation', { name: 'Settings sections' }).getByRole('button', { name: 'Advanced' }).click()
  assert.equal(await page.locator('.kv-page .kv-group').last().evaluate((g) => g.classList.contains('kv-danger-zone')), true, 'the last thing in Advanced')
  assert.match(await page.getByRole('group', { name: 'Start again' }).innerText(), /Reset Keevoro\s+Back to how it was when first installed\./)
  assert.match(await tipSays('starting again'), /Puts Keevoro back as it was when you first installed it: to clear out a trial run, or before passing the computer on\./)
  await page.getByRole('button', { name: 'Reset Keevoro…' }).click()
  assert.match(await text('.resetting'), /This removes:.*your backup.*This does not touch:.*anything on your sources.*already sent/s)
  await shot('9i-reset')
  await page.getByRole('button', { name: 'Cancel' }).click()
  assert.equal(await page.locator('.resetting').count(), 0, 'cancelling changes nothing')
  await page.getByRole('navigation', { name: 'Settings sections' }).getByRole('button', { name: 'This backup' }).click()
  assert.match(await page.locator('#setting-name').innerText(), /My photos/)
  await page.getByRole('navigation', { name: 'Settings sections' }).getByRole('button', { name: 'Advanced' }).click()
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
  // While a backup is in hand the update waits for it, rather than closing
  // Keevoro part-way, and offers to stop it instead.
  await page.goto(`${base}/?fast&update&busy`)
  await page.getByRole('button', { name: 'Update now' }).click()
  await page.getByText(/^Keevoro will update once the work in hand finishes, so nothing is cut short\./).waitFor()
  await shot('9c2-update-waits-for-the-backup')
  await fitsNarrow('An update waiting for the backup in hand')
  await page.getByRole('button', { name: 'Stop it and update now' }).click()
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

  // --- Folders that live together are shown together: one row for the
  // folder they share, with what is in them added up and one tick for all
  // of them, that opens to show each by the part of its path below it.
  await page.goto(`${base}/?demo=empty&fast&survey=tree`)
  await page.getByRole('button', { name: /Windows \(C:\)/ }).click()
  await page.getByRole('button', { name: 'Find the folders with photos' }).click()
  await page.getByRole('heading', { name: /^13 folders on Windows \(C:\) seem to hold photos and videos/ }).waitFor()
  assert.equal(
    await tipSays('folders shown together', page.locator('.survey h2')),
    'Folders that sit together are shown together. Tick a group to choose everything in it.',
  )
  const folderTicks = page.locator('.survey .hints li.suggested > label input')
  const checkedCount = () => folderTicks.evaluateAll((boxes) => boxes.filter((b) => b.checked).length)
  // Every folder is still counted, ticked or not, as in the flat list: the
  // real photos are ticked to begin with, the probable icons are not.
  assert.equal(await page.locator('.survey .hints li.suggested').count(), 3, 'the long groups start closed')
  assert.ok(await page.getByRole('button', { name: 'Look through these 9 folders' }).isVisible(), 'the same nine are chosen as before')
  const groupHead = (folder) => page.locator('.survey .group-head').filter({ has: page.locator('.where', { hasText: new RegExp(`^${folder.replaceAll('\\', '\\\\')}$`) }) })
  const topRows = await page.locator('.survey > .hints > li').evaluateAll((rows) => rows.map((li) => li.querySelector('.where').textContent))
  assert.deepEqual(topRows, ['C:\\Users\\You', 'C:\\Old laptop backup\\Photos', 'C:\\Archive\\Guides', 'C:\\Games'], 'groups go by their totals, the biggest first, and probable icons last')
  const guides = groupHead('C:\\Archive\\Guides')
  assert.match(await guides.locator('.about .quiet').innerText(), /^8 folders · 133 photos and 3 videos · 15\.0 MB$/)
  assert.equal(await guides.locator('.badge').innerText(), '2 of 8 are probably not photos')
  const guidesOpen = guides.getByRole('button', { name: 'The folders in Guides' })
  assert.equal(await guidesOpen.getAttribute('aria-expanded'), 'false', 'eight is too many to start open')
  const games = groupHead('C:\\Games')
  assert.equal(await games.getByRole('button', { name: 'The folders in Games' }).getAttribute('aria-expanded'), 'false', 'only probable icons: closed')
  assert.equal(await games.locator('.badge').innerText(), 'Small pictures, probably not photos', 'the same note on all of them is said once')
  const guidesTick = guides.locator('label input')
  const tickState = (box) => box.evaluate((b) => (b.indeterminate ? 'mixed' : b.checked ? 'all' : 'none'))
  assert.equal(await tickState(guidesTick), 'mixed', 'six of the eight are ticked')
  assert.equal(await tickState(games.locator('label input')), 'none')
  assert.equal(await tickState(groupHead('C:\\Users\\You').locator('label input')), 'all')
  await shot('9q10-survey-grouped')
  // Opened from the keyboard: Right opens, Left closes.
  await guidesOpen.focus()
  await page.keyboard.press('ArrowRight')
  assert.equal(await guidesOpen.getAttribute('aria-expanded'), 'true')
  await page.keyboard.press('ArrowLeft')
  assert.equal(await guidesOpen.getAttribute('aria-expanded'), 'false')
  await guidesOpen.click()
  assert.equal(await guidesOpen.getAttribute('aria-expanded'), 'true')
  const guidesGroup = page.locator('.survey li.group').filter({ has: page.locator('.group-head .where', { hasText: /^C:\\Archive\\Guides$/ }) })
  const shownAs = () =>
    guidesGroup.evaluate((li) => [...li.querySelectorAll(':scope > .hints .where')].map((w) => [w.querySelector('[aria-hidden]')?.textContent ?? w.textContent, w.title]))
  const inside = await shownAs()
  assert.deepEqual(
    inside.map(([label]) => label),
    ['2017', 'Walks', 'Maps', '2018', 'Coast', 'Towns', '2019', 'Scans', 'Covers', 'Icons'],
    'each by the part of its path below the group, the biggest first',
  )
  assert.deepEqual(inside[1], ['Walks', 'C:\\Archive\\Guides\\2017\\Walks'], 'with all of it to hover over')
  assert.equal(await guidesGroup.locator('li.suggested', { hasText: 'Walks' }).locator('.where .visually-hidden').innerText(), 'C:\\Archive\\Guides\\2017\\Walks', 'and to hear')
  assert.match(await guidesGroup.locator('li.suggested', { hasText: 'Covers' }).innerText(), /Small pictures, probably not photos/, 'the two that are probably not photos keep their notes')
  // One tick for the group sets every folder in it, at any depth, and
  // shows some ticked when one is unticked.
  const walks = guidesGroup.locator('li.suggested', { hasText: 'Walks' }).locator('label input')
  await guidesTick.check()
  assert.equal(await tickState(guidesTick), 'all')
  assert.deepEqual(await guidesGroup.locator('li.suggested > label input').evaluateAll((boxes) => boxes.map((b) => b.checked)), Array(8).fill(true))
  await walks.uncheck()
  assert.equal(await tickState(guidesTick), 'mixed')
  assert.equal(await tickState(guidesGroup.locator('.group-head', { hasText: '2017' }).locator('label input')), 'mixed', 'and so does the group inside it')
  assert.equal(await guidesTick.evaluate((b) => b.matches(':indeterminate')), true, 'a real tick box, shown and said as mixed')
  await page.evaluate(() => document.activeElement?.blur())
  await shot('9q11-survey-group-open')
  await page.emulateMedia({ colorScheme: 'dark' })
  await shot('9q11-survey-group-open-dark')
  await page.emulateMedia({ colorScheme: 'light' })
  await fitsNarrow('The survey with folders grouped')
  await narrowShots('9q11-survey-group-open', guides)
  // Leave out on a group leaves out the folder they share, everything in
  // it with it, and Include again puts it back.
  const chosenBefore = await checkedCount()
  await guides.getByRole('button', { name: 'Leave out Guides' }).click()
  await guides.getByRole('button', { name: 'Include Guides again' }).waitFor()
  assert.match(await text('.survey .left-out-said'), /^Left out C:\\Archive\\Guides\. .*nothing already backed up is touched\.$/)
  assert.equal(await guides.locator('.badge', { hasText: 'Left out of this backup' }).count(), 1)
  assert.deepEqual(await guidesGroup.locator('li.suggested > label input').evaluateAll((boxes) => boxes.map((b) => b.disabled)), Array(8).fill(true), 'none in it can be ticked')
  assert.equal(await guidesGroup.locator('.hints').first().getByRole('button', { name: /^Leave out / }).count(), 0, 'and none is left out again on its own')
  assert.equal(await checkedCount(), chosenBefore - 7, 'the seven ticked in it are no longer chosen')
  await shot('9q12-survey-group-left-out')
  await guides.getByRole('button', { name: 'Include Guides again' }).click()
  await guides.getByRole('button', { name: 'Leave out Guides' }).waitFor()
  assert.equal(await checkedCount(), chosenBefore, 'and back, ticks and all')
  // Look inside a group is the folder they share; Back finds the list as it was.
  await guides.getByRole('button', { name: 'Look inside C:\\Archive\\Guides' }).click()
  await page.getByRole('heading', { name: 'Inside Guides', level: 1 }).waitFor()
  await page.getByRole('button', { name: '‹ Back' }).click()
  await page.getByRole('heading', { name: /^13 folders on Windows \(C:\)/ }).waitFor()
  assert.equal(await guidesOpen.getAttribute('aria-expanded'), 'true', 'still open')
  assert.equal(await games.getByRole('button', { name: 'The folders in Games' }).getAttribute('aria-expanded'), 'false', 'still closed')
  assert.equal(await walks.isChecked(), false, 'and the ticks as they were')
  assert.equal(await tickState(guidesTick), 'mixed')
  // What is looked through is the folders ticked, however they are grouped.
  await groupHead('C:\\Users\\You').locator('label input').uncheck()
  // From some ticked, the first press ticks them all and the next none.
  await guidesTick.click()
  assert.equal(await tickState(guidesTick), 'all')
  await guidesTick.click()
  assert.equal(await tickState(guidesTick), 'none')
  await guidesGroup.locator('.group-head', { hasText: '2017' }).locator('label input').check()
  assert.equal(await checkedCount(), 3)
  await page.getByRole('button', { name: 'Look through these 3 folders' }).click()
  await page.locator('.kept li.source').nth(2).waitFor()
  assert.deepEqual(
    (await page.locator('.kept li.source .what .folder').allInnerTexts()).sort(),
    ['C:\\Archive\\Guides\\2017\\Walks', 'C:\\Archive\\Guides\\2017\\Maps', 'C:\\Old laptop backup\\Photos'].sort(),
  )

  // --- Look inside a folder the survey suggests, before choosing it. Only
  // what is near the screen is fetched, pictures shimmer until they come,
  // and nothing is added until the person says so.
  await page.goto(`${base}/?demo=empty&fast&slowpictures`)
  await page.getByRole('button', { name: /Windows \(C:\)/ }).click()
  await page.getByRole('button', { name: 'Find the folders with photos' }).click()
  await page.getByRole('heading', { name: /^4 folders on Windows \(C:\) seem to hold photos and videos/ }).waitFor()
  await page.locator('.survey .hints li.suggested > label input').nth(2).uncheck()
  await page.getByRole('button', { name: 'Look inside C:\\Users\\You\\Pictures' }).click()
  await page.getByRole('heading', { name: 'Inside Pictures', level: 1 }).waitFor()
  assert.match(await text('.inside .status'), /^1,630 photos and 82 videos · [\d.]+ GB\. Nothing is added or changed until you choose\.$/)
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
  await page.getByRole('heading', { name: /^4 folders on Windows \(C:\) seem to hold photos and videos/ }).waitFor()
  assert.deepEqual(await page.locator('.survey .hints li.suggested > label input').evaluateAll((boxes) => boxes.map((b) => b.checked)), [true, true, false, false])
  assert.equal(await page.locator('.kept li.source').count(), 0, 'looking inside added nothing')
  // As it looks once the pictures have come.
  await page.goto(`${base}/?demo=empty&fast`)
  await page.getByRole('button', { name: /Windows \(C:\)/ }).click()
  await page.getByRole('button', { name: 'Find the folders with photos' }).click()
  await page.getByRole('button', { name: 'Look inside C:\\Users\\You\\Pictures' }).click()
  await page.getByRole('heading', { name: 'Inside Pictures', level: 1 }).waitFor()
  await page.waitForFunction(() => document.querySelectorAll('.inside .tile img').length >= 12)
  // A folder's heading sits clear above its pictures: its name, its counts
  // and its Leave out button never hang down over the first row.
  const clear = await page.locator('.inside .tiles-section').evaluateAll((sections) =>
    sections.flatMap((section) => {
      const heading = section.querySelector('.tiles-heading')
      const tile = section.querySelector('.tile')
      if (!heading || !tile) return []
      const top = tile.getBoundingClientRect().top
      return [...heading.children]
        .filter((part) => part.getBoundingClientRect().bottom > top + 0.5)
        .map((part) => `${part.tagName.toLowerCase()} "${part.textContent.trim().slice(0, 20)}" reaches ${Math.round(part.getBoundingClientRect().bottom)}, pictures start at ${Math.round(top)}`)
    }),
  )
  assert.deepEqual(clear, [], 'a folder heading stays above its pictures')
  await shot('9q4-look-inside')
  await footHere('Looking inside')
  await footShot('9q4-look-inside-foot')
  await fitsNarrow('Looking inside')
  // Only the videos, newest first by the file's date, each with its length.
  await page.getByRole('radiogroup', { name: 'Show' }).getByRole('radio', { name: 'Videos' }).click()
  await page.getByLabel('Sort by').selectOption({ label: 'Newest first (by file date)' })
  assert.match(await text('.inside header'), /By when each file was last changed or copied\./)
  assert.match(await tipSays('the dates shown'), /not always when it was taken/)
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
  await page.getByRole('button', { name: 'Look inside C:\\Users\\You\\Pictures' }).click()
  await page.getByRole('heading', { name: 'Inside Pictures', level: 1 }).waitFor()
  await page.locator('.inside header').getByRole('button', { name: 'Look through this folder' }).click()
  await page.locator('.kept li.source').first().waitFor()
  assert.match(await text('.kept li.source .what .folder'), /^C:\\Users\\You\\Pictures$/)

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
  await card.getByRole('button', { name: 'Include E: again' }).click()
  await card.locator('.left-out-by-you .left-out-said').waitFor()
  await card.locator('.left-out-by-you li').waitFor({ state: 'detached' })

  // --- Leave a folder out from the survey's suggestions, and from the
  // look inside, saying at the moment of the choice that nothing already
  // backed up is touched; Put back undoes it.
  await page.goto(`${base}/?demo=empty&fast`)
  await page.getByRole('button', { name: /Windows \(C:\)/ }).click()
  await page.getByRole('button', { name: 'Find the folders with photos' }).click()
  await page.getByRole('heading', { name: /^4 folders on Windows \(C:\) seem to hold photos and videos/ }).waitFor()
  await page.getByRole('button', { name: 'Leave out assets' }).click()
  await page.getByRole('button', { name: 'Include assets again' }).waitFor()
  assert.match(await text('.survey .left-out-said'), /^Left out C:\\Games\\Launcher\\assets\. .*nothing already backed up is touched\.$/)
  assert.equal(await page.locator('.survey .hints li.suggested > label input').nth(3).isDisabled(), true, 'it can no longer be ticked')
  await shot('9r-survey-left-out')
  await fitsNarrow('The survey with a folder left out')
  await page.getByRole('button', { name: 'Include assets again' }).click()
  await page.getByRole('button', { name: 'Leave out assets' }).waitFor()
  await page.getByRole('button', { name: 'Look inside C:\\Users\\You\\Pictures' }).click()
  await page.getByRole('heading', { name: 'Inside Pictures', level: 1 }).waitFor()
  await page.getByRole('button', { name: 'Leave out Captures' }).click()
  await page.getByRole('button', { name: 'Include Captures again' }).waitFor()
  await page.locator('.inside .tiles-heading .badge', { hasText: 'Left out' }).waitFor()
  await page.locator('.inside header').getByRole('button', { name: 'Leave out Pictures' }).click()
  await page.locator('.inside .left-out-here').waitFor()
  await page.locator('.inside .tile .out-mark').first().waitFor()
  assert.equal(await page.locator('.inside .tile .out-mark').first().getAttribute('title'), 'Left out: you left this folder out of this backup')
  await shot('9r2-look-inside-left-out')
  await fitsNarrow('Looking inside a folder left out')
  await page.locator('.inside .left-out-here').getByRole('button', { name: 'Include Pictures again' }).click()
  await page.locator('.inside .left-out-here').waitFor({ state: 'detached' })
  await page.getByRole('button', { name: 'Include Captures again' }).click()
  await page.getByRole('button', { name: 'Leave out Captures' }).waitFor()

  // --- Where the space goes: everything the survey saw, photos or not, by
  // folder, the biggest first, with the trail back up. Nothing is changed.
  await page.goto(`${base}/?demo=empty&fast`)
  await page.getByRole('button', { name: /Windows \(C:\)/ }).click()
  await page.getByRole('button', { name: 'Find the folders with photos' }).click()
  await page.getByRole('heading', { name: /^4 folders on Windows \(C:\) seem to hold photos and videos/ }).waitFor()
  await page.getByRole('button', { name: 'See where the space goes' }).click()
  await page.getByRole('heading', { name: 'Where the space goes on Windows (C:)', level: 1 }).waitFor()
  const spaceRows = () => page.locator('.space-list tbody tr .name > :first-child').allInnerTexts()
  // The list is filled once the map has been asked for: wait for its rows,
  // not for a moment to pass.
  await page.locator('.space-list tbody tr .name').first().waitFor()
  assert.deepEqual((await spaceRows()).slice(0, 3), ['Users', 'Games', 'Windows'], 'the biggest first')
  const windowsRow = page.locator('.space-list tbody tr', { hasText: 'Windows' }).first()
  assert.match(await windowsRow.innerText(), /System folder: Keevoro never looks through it/)
  assert.match(await windowsRow.getAttribute('class'), /muted/, 'not photos or videos, so muted')
  await page.locator('.space-list').getByRole('button', { name: 'Users', exact: true }).click()
  await page.getByRole('navigation', { name: 'Where you are' }).getByText('Users').waitFor()
  await page.locator('.space-list').getByRole('button', { name: 'You', exact: true }).click()
  await page.locator('.space-list').getByRole('button', { name: 'Pictures', exact: true }).click()
  await page.getByRole('grid', { name: 'What is in Pictures, the biggest first' }).waitFor()
  assert.deepEqual(
    await page.getByRole('navigation', { name: 'Where you are' }).locator('li').allInnerTexts(),
    ['Windows (C:)', 'Users', 'You', 'Pictures'],
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
  // Choosing a row low in the list can scroll the page, so the map is found afresh each time.
  const mapCorner = async () => {
    const box = await map.locator('canvas.over').boundingBox()
    return [box.x + 30, box.y + 8]
  }
  await page.mouse.move(...(await mapCorner()))
  await page.locator('.map-tip:not([hidden])').waitFor()
  assert.match(await text('.map-tip'), /^Texture packs: 17\.7 GB, 160 other files$/)
  await page.locator('.space-list tr.lit', { hasText: 'Texture packs' }).waitFor()
  assert.equal(await map.getAttribute('data-pointed'), 'C:\\Users\\You\\Pictures\\Texture packs')
  await page.locator('.space-list tbody tr', { hasText: 'Captures' }).hover()
  await page.waitForFunction(() => document.querySelector('.map-canvas')?.dataset.lit === 'C:\\Users\\You\\Pictures\\Captures')
  await page.locator('.space-list tbody tr', { hasText: 'Screenshots' }).click()
  assert.equal(await map.getAttribute('data-selected'), 'C:\\Users\\You\\Pictures\\Screenshots', 'the row chosen lights its box')
  await page.mouse.click(...(await mapCorner()))
  assert.equal(await map.getAttribute('data-selected'), 'C:\\Users\\You\\Pictures\\Texture packs', 'a box chosen on the map')
  assert.equal(await page.locator('.space-list tr[aria-selected="true"] .name > :first-child').innerText(), 'Texture packs', 'is chosen in the list')
  await page.locator('.space .chosen').getByRole('button', { name: 'Leave out Texture packs' }).waitFor()
  // Drawing every box of the map is quick: said, and held to a ceiling.
  const drawnMs = Number(await map.getAttribute('data-drawn-ms'))
  console.log(`The map of Pictures drew its boxes in ${drawnMs} ms.`)
  assert.ok(drawnMs < 200, `the map took ${drawnMs} ms to draw`)
  await shot('9t-space-map')
  await footHere('Where the space goes')
  await footShot('9t-space-map-foot')
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
  await page.mouse.click(...(await mapCorner()))
  await page.getByRole('grid', { name: 'What is in Texture packs, the biggest first' }).waitFor()
  await settled()
  assert.equal(await page.evaluate(() => window.keevoroEased), true, 'the folder grew to fill the map')
  await page.getByRole('navigation', { name: 'Where you are' }).getByRole('button', { name: 'Pictures' }).click()
  await page.getByRole('grid', { name: 'What is in Pictures, the biggest first' }).waitFor()
  await page.emulateMedia({ reducedMotion: 'reduce' })
  await page.evaluate(() => (window.keevoroEased = false))
  await page.mouse.dblclick(...(await mapCorner()))
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
  await chosen.getByRole('button', { name: 'Include Camera Roll again' }).waitFor()
  assert.match(await text('.space .left-out-said'), /nothing already backed up is touched/)
  assert.match(
    await page.locator('.space-list tbody tr', { hasText: 'Camera Roll' }).innerText(),
    /Not backed up: you left this folder out of this backup/,
  )
  await chosen.getByRole('button', { name: 'Include Camera Roll again' }).click()
  await chosen.getByRole('button', { name: 'Leave out Camera Roll' }).waitFor()
  await chosen.getByRole('button', { name: 'Look inside Camera Roll' }).click()
  await page.getByRole('heading', { name: 'Inside Camera Roll', level: 1 }).waitFor()
  await page.getByRole('button', { name: '‹ Back' }).click()
  await page.getByRole('grid', { name: 'What is in Pictures, the biggest first' }).waitFor()
  // By keyboard alone: the arrows move down the list, Enter goes in, Backspace comes back.
  await page.locator('.space-list tbody tr').first().focus()
  await page.keyboard.press('ArrowDown')
  await page.keyboard.press('ArrowDown')
  assert.equal(await page.evaluate(() => document.activeElement?.dataset.key), 'C:\\Users\\You\\Pictures\\Captures')
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
  await page.getByRole('heading', { name: /^4 folders on Windows \(C:\) seem to hold photos and videos/ }).waitFor()
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
  // The source is a family drive that is nearly all photos, as the first
  // outside tester's was: calm tints, not a wall of orange.
  await shot('9s5-space-of-a-source')
  await page.emulateMedia({ colorScheme: 'dark' })
  await shot('9s5b-space-of-a-source-dark')
  await page.emulateMedia({ colorScheme: 'light' })
  // As people with each kind of colour blindness see it: the kinds stay
  // apart by their edges and patterns.
  const cdp = await page.context().newCDPSession(page)
  for (const type of ['protanopia', 'deuteranopia', 'tritanopia']) {
    await cdp.send('Emulation.setEmulatedVisionDeficiency', { type })
    await shot(`9s5c-space-of-a-source-${type}`)
  }
  await cdp.send('Emulation.setEmulatedVisionDeficiency', { type: 'none' })
  await cdp.detach()

  // Above the map, a ring of what takes the room, with the figures in words;
  // each picks its kind, fading the boxes without it.
  const ringList = page.getByRole('list', { name: 'What takes the room in Family hard drive' })
  assert.deepEqual(await ringList.locator('li').allInnerTexts(), ['Photos 55.3 GB · 80%', 'Videos 12.6 GB · 18%', 'Everything else 1.1 GB · 2%'])
  assert.equal(await page.locator('.summary-ring svg').getAttribute('aria-hidden'), 'true', 'the ring is a picture; the list says it')
  await ringList.getByRole('button', { name: 'Photos' }).click()
  await page.waitForFunction(() => document.querySelector('.map-canvas')?.dataset.picked === 'photo')
  assert.equal(await ringList.getByRole('button', { name: 'Photos' }).getAttribute('aria-pressed'), 'true')
  assert.equal(await page.getByRole('tab', { name: 'By kind of file' }).getAttribute('aria-selected'), 'true', 'as By kind of file does')
  assert.ok(Number(await page.locator('.map-canvas').getAttribute('data-faded')) > 0, 'boxes without photos fade')
  await shot('9v-space-summary')
  await ringList.getByRole('button', { name: 'Photos' }).click()
  await page.waitForFunction(() => document.querySelector('.map-canvas')?.dataset.picked === '')
  await page.getByRole('tab', { name: 'By folder' }).click()
  await fitsNarrow('Where the space goes, with the ring above the map')

  // The list alone, chosen and remembered on this computer.
  const viewSwitch = page.getByRole('group', { name: 'Show as' })
  assert.equal(await viewSwitch.getByRole('button', { name: 'Map' }).getAttribute('aria-pressed'), 'true', 'the map, until another is chosen')
  await viewSwitch.getByRole('button', { name: 'List' }).click()
  await page.locator('.map-canvas').waitFor({ state: 'detached' })
  assert.ok((await page.locator('.space-list .share-bar').first().boundingBox()).width >= 190, 'wide share bars to compare')
  await shot('9v2-space-list-view')
  await page.emulateMedia({ colorScheme: 'dark' })
  await shot('9v2b-space-list-view-dark')
  await page.emulateMedia({ colorScheme: 'light' })
  await fitsNarrow('Where the space goes, as a list')
  await page.reload()
  await page.locator('.steps button').nth(0).click()
  await page.getByRole('button', { name: 'Where the space goes on Family hard drive' }).click()
  await page.getByRole('grid', { name: /^What is in / }).waitFor({ timeout: 15000 })
  assert.equal(await page.getByRole('group', { name: 'Show as' }).getByRole('button', { name: 'List' }).getAttribute('aria-pressed'), 'true', 'remembered')
  assert.equal(await page.locator('.map-canvas').count(), 0)
  await page.getByRole('group', { name: 'Show as' }).getByRole('button', { name: 'Map' }).click()
  await page.locator('.map-canvas').waitFor()

  // Small folders are gathered into one box and row, "Smaller folders",
  // saying why, opening in the list in place, and on the map with a click.
  await page.locator('.space-list tbody tr', { hasText: 'Pictures' }).getByRole('button', { name: 'Pictures' }).click()
  await page.getByRole('grid', { name: 'What is in Pictures, the biggest first' }).waitFor()
  const smallerRow = page.locator('.space-list tbody tr', { hasText: 'Smaller folders' })
  assert.match(
    await smallerRow.innerText(),
    /Smaller folders: 30 folders, [\d.]+ GB, each under \d+% of Pictures\. Put together so the map stays readable\./,
  )
  const disclosure = smallerRow.getByRole('button', { name: /Smaller folders/ })
  assert.equal(await disclosure.getAttribute('aria-expanded'), 'false')
  await disclosure.click()
  assert.equal(await page.locator('.space-list tbody tr.within').count(), 30, 'the folders it stands for, indented beneath it')
  await disclosure.click()
  assert.equal(await page.locator('.space-list tbody tr.within').count(), 0)
  const spaceMap = page.locator('.map-canvas')
  await page.waitForFunction(() => document.querySelector('.map-canvas')?.dataset.gatheredAt)
  const [gx, gy] = (await spaceMap.getAttribute('data-gathered-at')).split(',').map(Number)
  await spaceMap.scrollIntoViewIfNeeded()
  const mapAt = await spaceMap.boundingBox()
  await page.mouse.move(mapAt.x + gx, mapAt.y + gy)
  await page.locator('.map-tip:not([hidden])').waitFor()
  assert.match(await text('.map-tip'), /^Smaller folders: 30 folders, .* Click to open them\.$/)
  await page.mouse.click(mapAt.x + gx, mapAt.y + gy)
  await page.getByRole('grid', { name: 'What is in Smaller folders, the biggest first' }).waitFor()
  assert.equal(await page.getByLabel('Where you are').locator('[aria-current="page"]').innerText(), 'Smaller folders')
  assert.equal(await page.locator('.space-list tbody tr').count(), 30)
  await shot('9v3-space-smaller-folders')
  await fitsNarrow('Where the space goes, the smaller folders opened')
  await page.getByRole('button', { name: '‹ Back' }).first().click()
  await page.getByRole('grid', { name: 'What is in Pictures, the biggest first' }).waitFor()
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
  await page.getByRole('navigation', { name: 'Settings sections' }).getByRole('button', { name: 'Advanced' }).click()
  const mapsRow = page.locator('#setting-maps')
  const mapsAt = await mapsRow.boundingBox()
  await mapsRow.getByRole('button', { name: 'Forget them' }).click()
  assert.equal(await page.getByRole('status').filter({ hasText: 'remembered map' }).innerText(), 'Forgot 1 remembered map.')
  assert.deepEqual(await mapsRow.boundingBox(), mapsAt, 'the row does not grow when it is said')
  await mapsRow.scrollIntoViewIfNeeded()
  await shot('9u-settings-forget-maps')

  // --- Cloud storage as a source, from our first outside tester: the same
  // questions as a destination's, looked through by listing, and what
  // reading it would cost said before Back up now.
  await page.goto(`${base}/?fast`)
  await page.getByRole('heading', { name: 'Which of these should we keep?', level: 1 }).waitFor()
  await page.locator('.steps button').nth(0).click()
  await page.getByRole('heading', { name: 'Where are your photos?', level: 1 }).waitFor()
  await page.getByRole('button', { name: /^Add cloud storage as a source/ }).click()
  const bucketForm = page.getByRole('form', { name: 'Add cloud storage as a source' })
  // Google Cloud Storage is offered here too, with no place to ask.
  await bucketForm.getByLabel('Storage').selectOption('gcs')
  assert.equal(await bucketForm.getByLabel('Region').count(), 0)
  await bucketForm.getByText('Where to make the keys').click()
  assert.match(await bucketForm.innerText(), /Cloud Storage, Settings, Interoperability/)
  await bucketForm.getByLabel('Storage').selectOption('s3')
  await bucketForm.getByLabel('Region').fill('eu-west-2')
  await bucketForm.getByLabel('Bucket name').fill('old-family-backup')
  await bucketForm.getByLabel('Folder in the bucket (optional)').fill('Photos')
  await bucketForm.getByLabel('Access key ID').fill('AKIAEXAMPLE')
  await bucketForm.getByLabel('Secret access key').fill('not-a-real-secret')
  assert.match(await bucketForm.innerText(), /Keevoro only reads this bucket, so a key that can only read is enough\./)
  assert.match(await tipSays('reading from this bucket', bucketForm), /Nothing in it is ever changed or deleted\.\s+The key is kept in your computer’s keychain\./)
  await bucketForm.scrollIntoViewIfNeeded()
  await shot('2e-cloud-storage-as-a-source')
  await fitsNarrow('Adding cloud storage as a source')
  await bucketForm.getByRole('button', { name: 'Add and look through it' }).click()
  const bucketSource = page.locator('.kept li.source').filter({ hasText: 'old-family-backup' })
  await bucketSource.waitFor()
  assert.match(await bucketSource.innerText(), /In the folder Photos in the Amazon S3 bucket old-family-backup/)
  assert.equal(await bucketSource.getByRole('button', { name: /^Look inside/ }).count(), 0, 'nothing is downloaded to look inside it')
  await bucketSource.scrollIntoViewIfNeeded()
  await shot('2f-cloud-storage-looked-through')
  await page.locator('.steps button').nth(2).click()
  await page.getByRole('button', { name: /A folder/ }).click()
  await page.getByRole('heading', { name: /^Backing up to the folder/ }).waitFor()
  const cloudCost = page.locator('.cloud-cost')
  await cloudCost.waitFor()
  assert.match(
    await cloudCost.innerText(),
    /^Reading from cloud storage can cost money\s+Reading [\d.]+ (GB|MB) out of Amazon S3 costs (about £[\d,]+|under £1) at Amazon’s usual price; Cloudflare R2 charges nothing for this\. Amazon lets each account download about 100 GB a month free, which may make it less\.$/,
  )
  await cloudCost.scrollIntoViewIfNeeded()
  await shot('11c-cloud-cost-before-backing-up')
  await fitsNarrow('What reading from cloud storage costs')
  // A Google Cloud Storage bucket as a source: asked only its bucket and
  // keys, its files in colder classes counted and said, and what reading
  // it costs said with the others.
  await page.locator('.steps button').nth(0).click()
  await page.getByRole('heading', { name: 'Where are your photos?', level: 1 }).waitFor()
  await page.getByRole('button', { name: /^Add cloud storage as a source/ }).click()
  await bucketForm.getByLabel('Storage').selectOption('gcs')
  await bucketForm.getByLabel('Bucket name').fill('old-scans')
  await bucketForm.getByLabel('Access key ID').fill('GOOGEXAMPLE1234567890AB')
  await bucketForm.getByLabel('Secret access key').fill('not-a-real-secret')
  await bucketForm.getByRole('button', { name: 'Add and look through it' }).click()
  const googleSource = page.locator('.kept li.source').filter({ hasText: 'old-scans' })
  await googleSource.waitFor()
  assert.match(await googleSource.innerText(), /In the Google Cloud Storage bucket old-scans/)
  assert.match(await googleSource.innerText(), /are in Google’s Nearline, Coldline or Archive storage, which costs more to read out\. Keevoro backs them up all the same\./)
  await googleSource.scrollIntoViewIfNeeded()
  await shot('2g-google-cloud-storage-as-a-source')
  await page.locator('.steps button').nth(2).click()
  await cloudCost.waitFor()
  await page.waitForFunction(() => /Google Cloud Storage/.test(document.querySelector('.cloud-cost')?.textContent ?? ''))
  assert.match(await cloudCost.innerText(), /Reading [\d.]+ (GB|MB) out of Google Cloud Storage costs (about £[\d,]+|under £1) at Google’s usual price; Cloudflare R2 charges nothing for this\. Files kept in Nearline, Coldline or Archive storage cost about 1p to 4p a gigabyte more to read\./)

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
  await moreChoice('Clear unfinished copies…')
  await page.getByRole('group', { name: /^Unfinished copies in/ }).waitFor()
  await fitsNarrow('Backing up')
  await page.setViewportSize({ width: 400, height: 800 })
  await page.getByRole('heading', { name: /^Backing up to the folder/ }).scrollIntoViewIfNeeded()
  assert.equal(await headingUnderBand(), null, 'the backup screen’s heading is not under the band')
  await shot('11b-backup-400')
  await page.setViewportSize({ width: 1440, height: 900 })
  // Every section of Settings, and every page in one, fits a narrow window,
  // and is pictured wide and at 400 pixels.
  await page.getByRole('button', { name: 'Settings' }).click()
  const settingsList = page.getByRole('navigation', { name: 'Settings sections' })
  const settingsPages = [
    ['This backup', null, 'this-backup'],
    ['Backing up by itself', null, 'backing-up-by-itself'],
    ['Appearance', null, 'appearance'],
    ['Updates', null, 'updates'],
    ['Advanced', null, 'advanced'],
    ['Advanced', ['Kinds of file', 'Kinds of file Keevoro looks for'], 'kinds-of-file'],
    ['Advanced', ['AI agents Looking only', 'AI agents'], 'ai-agents'],
  ]
  for (const [name, inside, file] of settingsPages) {
    await settingsList.getByRole('button', { name, exact: true }).click()
    if (inside) await page.getByRole('button', { name: inside[0], exact: true }).click()
    await page.getByRole('heading', { name: inside ? inside[1] : name, level: 2, exact: true }).waitFor()
    await shot(`12-settings-${file}`)
    await fitsNarrow(`Settings, ${file}`)
    await page.setViewportSize({ width: 400, height: 800 })
    await settled()
    await shot(`12-settings-${file}-400`)
    // At 400 pixels a page has its way back, and no list beside it.
    assert.equal(await settingsList.isVisible(), false, `Settings, ${file}, at 400 pixels is a page of its own`)
    await page.getByRole('button', { name: inside ? `‹ ${name}` : '‹ Settings' }).waitFor()
    await page.setViewportSize({ width: 1440, height: 900 })
    if (inside) await page.getByRole('button', { name: `‹ ${name}` }).click()
  }
  // At 400 pixels the list is the first page, and a section the next, as on a phone.
  await page.setViewportSize({ width: 400, height: 800 })
  await page.getByRole('button', { name: '‹ Settings' }).click()
  await settingsList.waitFor()
  assert.equal(await page.locator('#settings-page-title').isVisible(), false, 'the list alone')
  assert.deepEqual(await spills(), [], 'the list of sections at 400 pixels spills sideways')
  assert.equal(await page.evaluate(() => document.activeElement?.getAttribute('aria-current') && document.activeElement.textContent), 'Advanced', 'back on the section it came from, marked as the one open')
  await shot('12-settings-list-400')
  await settingsList.getByRole('button', { name: 'Updates' }).click()
  await page.getByRole('heading', { name: 'Updates', level: 2 }).waitFor()
  assert.equal(await page.evaluate(() => document.activeElement?.id), 'settings-page-title', 'the page’s heading has the focus')
  await page.keyboard.press('Escape')
  await settingsList.waitFor()
  await page.emulateMedia({ colorScheme: 'dark' })
  await settingsList.getByRole('button', { name: 'Backing up by itself' }).click()
  await settled()
  await shot('12-settings-backing-up-by-itself-400-dark')
  await page.emulateMedia({ colorScheme: 'light' })
  await page.setViewportSize({ width: 1440, height: 900 })
  await page.getByRole('button', { name: '‹ Back', exact: true }).click()
  await page.getByRole('button', { name: 'What Keevoro is doing' }).click()
  await fitsNarrow('What Keevoro is doing')
  await page.getByRole('button', { name: '‹ Back' }).click()
  // Every section of Help, and its page, fits a narrow window too, and is
  // pictured wide, at 400 pixels, and dark.
  await page.getByRole('button', { name: 'Help', exact: true }).click()
  const helpList = page.getByRole('navigation', { name: 'Help sections' })
  const helpPages = [
    ['How do I…', null, 'how-do-i'],
    ['Report a problem or suggest an idea', null, 'report'],
    ['Compare cloud storage providers', null, 'compare'],
    ['What Keevoro has been doing', null, 'log'],
    ['About Keevoro', null, 'about'],
    ['About Keevoro', ['Open source licences', 'Open source licences'], 'licences'],
  ]
  for (const [name, inside, file] of helpPages) {
    await helpList.getByRole('button', { name, exact: true }).click()
    if (inside) await page.getByRole('button', { name: inside[0], exact: true }).click()
    await page.getByRole('heading', { name: inside ? inside[1] : name, level: 2, exact: true }).waitFor()
    await shot(`12-help-${file}`)
    await fitsNarrow(`Help, ${file}`)
    await page.setViewportSize({ width: 400, height: 800 })
    await settled()
    await shot(`12-help-${file}-400`)
    assert.equal(await helpList.isVisible(), false, `Help, ${file}, at 400 pixels is a page of its own`)
    await page.getByRole('button', { name: inside ? `‹ ${name}` : '‹ Help' }).waitFor()
    await page.emulateMedia({ colorScheme: 'dark' })
    await settled()
    await shot(`12-help-${file}-400-dark`)
    await page.emulateMedia({ colorScheme: 'light' })
    await page.setViewportSize({ width: 1440, height: 900 })
    if (inside) await page.getByRole('button', { name: `‹ ${name}` }).click()
  }
  await page.setViewportSize({ width: 400, height: 800 })
  await page.getByRole('button', { name: '‹ Help' }).click()
  await helpList.waitFor()
  assert.deepEqual(await spills(), [], 'the list of Help’s sections at 400 pixels spills sideways')
  await shot('12-help-list-400')
  await page.emulateMedia({ colorScheme: 'dark' })
  await settled()
  await shot('12-help-list-400-dark')
  await page.emulateMedia({ colorScheme: 'light' })
  await page.setViewportSize({ width: 1440, height: 900 })
  await page.getByRole('button', { name: '‹ Back', exact: true }).click()

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

  // --- Keep my folders as they are: the first outside tester's folder of
  // family scans, sorted by family, in a new backup not asked yet.
  await page.goto(`${base}/?demo=scans&fast`)
  await page.getByRole('heading', { name: 'How should we arrange them in the backup?', level: 1 }).waitFor()
  assert.equal(await page.getByRole('radio', { name: /^Keep my folders as they are/ }).isChecked(), true, 'the suggestion is chosen')
  assert.equal(await text('.arrange-why'), 'Suggested because the photos in Family Scans look like scans.')
  await page.getByText('You can choose differently for each source later.').waitFor()
  assert.match(await text('.arrange-question'), /starting with “Family hard drive” and “Family Scans”/)
  await shot('3e-how-to-arrange-scans')
  await page.emulateMedia({ colorScheme: 'dark' })
  await shot('3e2-how-to-arrange-scans-dark')
  await page.emulateMedia({ colorScheme: 'light' })
  await fitsNarrow('the question how to arrange, with a suggestion to keep the folders')
  // Choosing with the keyboard works as for any radio group.
  await page.getByRole('radio', { name: /^Arrange by date/ }).focus()
  await page.keyboard.press('ArrowDown')
  assert.equal(await page.getByRole('radio', { name: /^Keep my folders as they are/ }).isChecked(), true)
  await page.locator('.arrange-question').getByRole('button', { name: 'Next' }).click()

  // Choose shows the scans in their folders, as they will be in the backup.
  await page.getByRole('heading', { name: 'Which of these should we keep?', level: 1 }).waitFor()
  assert.match(await text('.discover .status'), /Files from Family Scans are shown in their folders, as they will be kept\.$/)
  const keptSection = page.getByRole('region', { name: 'From Family Scans, kept in your folders' })
  assert.deepEqual(await keptSection.locator('.card h4').allInnerTexts(), ['Family Scans', 'Jones', 'Smith'], 'one card per folder, in folder order')
  assert.match(await keptSection.innerText(), /Shown as they will be in the backup: the same folders, the same names\./)
  assert.equal(await page.locator('.hero h2').innerText().then((t) => ['Family Scans', 'Jones', 'Smith'].includes(t)), false, 'not the hero')
  await page.waitForFunction(() => [...document.images].every((i) => i.complete && i.naturalWidth > 0))
  await keptSection.scrollIntoViewIfNeeded()
  await shot('4b-choose-kept-folders')
  await page.emulateMedia({ colorScheme: 'dark' })
  await shot('4b2-choose-kept-folders-dark')
  await page.emulateMedia({ colorScheme: 'light' })
  await fitsNarrow('the folders kept as they are on Choose')
  // Asked once: moving on from finding again goes straight to choosing.
  await page.locator('.steps button').first().click()
  await page.getByRole('button', { name: 'See what was found' }).click()
  await page.getByRole('heading', { name: 'Which of these should we keep?', level: 1 }).waitFor()
  assert.equal(await page.locator('.arrange-question').count(), 0, 'not asked again')

  // Settings has the same choice for the whole backup, and says what a
  // change means for what is already safe.
  await page.getByRole('button', { name: 'Settings' }).click()
  await page.getByRole('heading', { name: 'How files are arranged', level: 3 }).waitFor()
  const settingsArrange = page.getByRole('radiogroup', { name: 'How files are arranged in the backup' })
  assert.equal(await settingsArrange.getByRole('radio', { name: /^Keep my folders as they are/ }).isChecked(), true)
  assert.equal(await tipSays('How files are arranged'), 'Changing this only changes where files go from now on. Files already backed up stay where they are.')
  await settingsArrange.getByRole('radio', { name: /^Arrange by date/ }).check()
  await page.locator('.kv-arrange').getByRole('status').getByText('From now on, new files are arranged by date.').waitFor()
  await shot('9p-settings-arranged')
  await page.getByRole('button', { name: '‹ Back' }).click()

  // One source can keep its folders while the backup goes by date, from
  // the suggestion beside it, and its card says so.
  await page.locator('.steps button').first().click()
  await page.getByRole('button', { name: 'File types on Family Scans' }).click()
  const scansArrange = page.getByRole('radiogroup', { name: 'Arrange files from Family Scans' })
  assert.deepEqual(await scansArrange.getByRole('radio').evaluateAll((all) => all.map((r) => `${r.parentElement.innerText.trim().split('\n')[0]} ${r.checked}`)), [
    'By date A folder for each year and month. true',
    'Keep its folders The same folders and names as on it. false',
  ])
  assert.equal(await page.locator('.source-arrange .differs').count(), 0, 'nothing said while it is arranged as the rest')
  assert.match(await text('.source-arrange .suggestion'), /^Suggestion: keep its folders as they are\. These look like scans\./)
  await page.locator('.source-arrange').scrollIntoViewIfNeeded()
  await shot('3f-source-arrange')
  await page.locator('.source-arrange').getByRole('button', { name: 'Keep its folders' }).click()
  await page.locator('.source-arrange').getByRole('status').getByText('New files from Family Scans go this way from now on. Files already backed up stay where they are.').waitFor()
  assert.equal(await scansArrange.getByRole('radio', { name: /^Keep its folders/ }).isChecked(), true)
  assert.equal(await text('.source-arrange .differs'), 'Different from the rest of this backup. Use the usual')
  await page.getByText('Kept in its own folders, starting with “Family Scans”.').waitFor()
  await fitsNarrow('a source keeping its folders')
  await narrowShots('3f2-source-file-types', page.locator('.source-arrange'))
  await narrowShots('3f3-source-wants', page.locator('.source-wants'))

  // Above Back up now, one quiet line says how it is arranged, with Change.
  await page.locator('.steps button').nth(2).click()
  await page.getByRole('heading', { name: 'Where should they go?', level: 1 }).waitFor()
  assert.equal(await text('.arranged'), 'Arranged by date, except Family Scans, which keeps its folders. Change')
  await page.locator('.arranged').getByRole('button', { name: 'Change' }).click()
  await page.getByRole('heading', { name: 'Settings', level: 1 }).waitFor()
  await page.getByRole('heading', { name: 'This backup', level: 2 }).waitFor()
  await page.locator('#setting-arrange[data-marked]').waitFor()
  await shot('9q-settings-arrange-marked')
  // The mark is for a moment, then goes.
  await page.waitForFunction(() => !document.querySelector('[data-marked]'))

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

  // --- Reporting a problem or an idea, from the foot of the window. Only
  // the person's words, the version, the system and the screen go unless
  // they switch more on; what is shown is what is sent; a refusal keeps
  // their words, says why and offers Try again.
  await page.setViewportSize({ width: 1440, height: 900 })
  await page.goto(`${base}/?fast&reportFails`)
  await page.getByRole('heading', { name: 'Which of these should we keep?', level: 1 }).waitFor()
  await page.getByRole('button', { name: 'Help', exact: true }).click()
  await page.getByRole('navigation', { name: 'Help sections' }).getByRole('button', { name: 'Report a problem or suggest an idea' }).click()
  await page.getByRole('heading', { name: 'Report a problem or suggest an idea', level: 2 }).waitFor()
  const reportKinds = page.getByRole('radiogroup', { name: 'What is it?' })
  assert.equal(await reportKinds.getByRole('radio', { name: 'Something went wrong' }).getAttribute('aria-checked'), 'true')
  assert.equal(await reportKinds.getByRole('radio', { name: 'I have an idea' }).getAttribute('aria-checked'), 'false')
  const withLog = page.getByRole('switch', { name: 'Include the end of Keevoro’s log' })
  const withContact = page.getByRole('switch', { name: 'Include a way to reach me' })
  assert.equal(await withLog.getAttribute('aria-checked'), 'false', 'the log is off until switched on')
  assert.equal(await withContact.getAttribute('aria-checked'), 'false', 'a way to reach them is off until switched on')
  await page.getByText(/The log can name your folders and files\./).waitFor()
  assert.equal(await page.getByRole('button', { name: 'Send', exact: true }).isDisabled(), true, 'nothing to send without words')
  await page.getByLabel('What happened?').fill('The map went blank after a search.')
  await page.getByText(/the screen you were on \(Choose what to keep\)/).waitFor()
  await page.getByText('See exactly what will be sent').click()
  const sentText = () => page.getByLabel('Exactly what will be sent').innerText()
  await page.waitForFunction(() => document.querySelector('.report-preview pre')?.textContent.includes('"message"'))
  assert.deepEqual(JSON.parse(await sentText()), {
    kind: 'problem',
    message: 'The map went blank after a search.',
    appVersion: '9.8.0',
    os: 'Windows 11 Home (26100)',
    screen: 'Choose what to keep',
  }, 'by default only the words, the version, the system and the screen')
  await withLog.click()
  await page.waitForFunction(() => document.querySelector('.report-preview pre')?.textContent.includes('"logTail"'))
  assert.match(await sentText(), /Looking through ~\\\\Pictures\\\\Family photos/, 'the home folder is a ~')
  assert.doesNotMatch(await sentText(), /Users/)
  await withContact.click()
  await page.getByLabel('How to reach you').fill('you@example.com')
  await page.waitForFunction(() => document.querySelector('.report-preview pre')?.textContent.includes('you@example.com'))
  const shown = await sentText()
  await shot('13-report-preview')
  await fitsNarrow('Reporting a problem')
  await page.setViewportSize({ width: 400, height: 800 })
  await settled()
  await shot('13-report-preview-400')
  await page.emulateMedia({ colorScheme: 'dark' })
  await settled()
  await shot('13-report-preview-400-dark')
  await page.emulateMedia({ colorScheme: 'light' })
  await page.setViewportSize({ width: 1440, height: 900 })
  // Refused: the words stay, with why, and Try again.
  await page.getByRole('button', { name: 'Send', exact: true }).click()
  const refused = page.getByRole('alert').filter({ hasText: 'Your report wasn’t sent.' })
  await refused.waitFor()
  assert.match(await refused.innerText(), /That’s a lot of reports from one computer today\. Please try again tomorrow\./)
  assert.equal(await page.getByLabel('What happened?').inputValue(), 'The map went blank after a search.', 'their words are kept')
  assert.equal(await sentText(), shown, 'what was shown is still what will go')
  await shot('13-report-refused')
  await fitsNarrow('A report refused')
  await page.getByRole('button', { name: 'Try again' }).click()
  await page.getByText('Thank you. Your reference is K-1042.').waitFor()
  // The list of reports says how each is getting on, newest first.
  const reportRows = page.locator('#setting-yourReports li')
  await page.waitForFunction(() => document.querySelectorAll('#setting-yourReports li').length === 3)
  assert.match(await reportRows.nth(0).innerText(), /^K-1042 The map went blank after a search\.\s+Received$/)
  assert.match(await reportRows.nth(1).innerText(), /^K-1041 Could the map of where the space goes be calmer\?\s+Done in version 0\.47\.0\s+Thank you\. The map is quieter now\.$/)
  assert.match(await reportRows.nth(2).innerText(), /Being worked on$/)
  await shot('13-report-sent')
  await fitsNarrow('A report sent, and the list of reports')
  await page.setViewportSize({ width: 400, height: 800 })
  await settled()
  await shot('13-report-sent-400')
  await page.emulateMedia({ colorScheme: 'dark' })
  await settled()
  await shot('13-report-sent-400-dark')
  await page.emulateMedia({ colorScheme: 'light' })
  await page.setViewportSize({ width: 1440, height: 900 })
  // Back goes to the screen it came from.
  await page.getByRole('button', { name: '‹ Back', exact: true }).click()
  await page.getByRole('heading', { name: 'Which of these should we keep?', level: 1 }).waitFor()
  // The name it had in Settings, Help, still leads to it, now in Help; the
  // list says quietly when it could not check.
  await page.goto(`${base}/?fast&reportsOffline&page=settings/help/report`)
  await page.getByRole('heading', { name: 'Help', level: 1 }).waitFor()
  await page.getByRole('heading', { name: 'Report a problem or suggest an idea', level: 2 }).waitFor()
  await page.getByText('Couldn’t check just now.').waitFor()
  await page.getByText(/the screen you were on \(Help\)/).waitFor()
  await reportKinds.getByRole('radio', { name: 'I have an idea' }).click()
  await page.getByLabel('What is your idea?').waitFor()
  // The search finds it by other words for it.
  await page.getByLabel('Search Help').fill('feedback')
  assert.match(await page.getByRole('list', { name: 'Found in Settings and Help' }).locator('li').first().innerText(), /^Report a problem or suggest an idea\s+Help$/)
  await page.getByLabel('Search Help').fill('')
  // The other old names lead to where each thing lives now.
  for (const [old, heading] of [
    ['settings/help', 'How do I…'],
    ['settings/help/log', 'What Keevoro has been doing'],
    ['settings/help/licences', 'Open source licences'],
    ['settings/help/reset', 'Advanced'],
    ['about', 'About Keevoro'],
    ['help/compare', 'Compare cloud storage providers'],
  ]) {
    await page.goto(`${base}/?fast&page=${old}`)
    await page.getByRole('heading', { name: heading, level: 2, exact: true }).waitFor()
  }

  // --- On a work computer whose proxy Keevoro cannot use, Keevoro cannot
  // reach the reports service, so the window sends the very same text
  // itself, through the system's own network settings, and Keevoro keeps
  // the reference as if it had sent it. It asks after reports the same way.
  const WINDOW_TOKEN = '89abcdef0123456789abcdef0123456789abcdef'
  reportsService = (request) =>
    request.method() === 'POST'
      ? { status: 201, answer: { ok: true, reference: 'K-2001', token: WINDOW_TOKEN } }
      : { status: 200, answer: { ok: true, reference: 'K-2001', status: 'started' } }
  await page.goto(`${base}/?fast&reportUnreachable&page=help/report`)
  await page.getByLabel('What happened?').fill('Sending a report sat on Sending.')
  await page.getByText('See exactly what will be sent').click()
  await page.waitForFunction(() => document.querySelector('.report-preview pre')?.textContent.includes('Sending a report'))
  const shownThroughWindow = await sentText()
  await page.getByRole('button', { name: 'Send', exact: true }).click()
  await page.getByText('Thank you. Your reference is K-2001.').waitFor()
  const posted = reportsAsked.filter((r) => r.method === 'POST')
  assert.equal(posted.length, 1)
  assert.equal(posted[0].url, 'https://reports.keevoro.com/v1/reports')
  assert.equal(posted[0].body, shownThroughWindow, 'exactly what was shown')
  assert.equal(posted[0].cookie, undefined, 'nothing of the window’s own')
  await page.waitForFunction(() => /K-2001/.test(document.querySelector('#setting-yourReports')?.textContent ?? ''))
  await shot('13-report-sent-through-the-window')
  // Asked after through the window too, next time the page opens.
  await page.getByRole('button', { name: '‹ Back', exact: true }).click()
  // Help opens again where it was.
  await page.getByRole('button', { name: 'Help', exact: true }).click()
  await page.locator('#setting-yourReports li', { hasText: 'K-2001' }).getByText('Being worked on').waitFor()
  assert.ok(
    reportsAsked.some((r) => r.method === 'GET' && r.url === `https://reports.keevoro.com/v1/reports/K-2001?token=${WINDOW_TOKEN}`),
    'asked after with its token',
  )
  // Keevoro said it couldn't check; once the window has, it no longer says so.
  await page.getByText('Couldn’t check just now.').waitFor({ state: 'detached' })

  // Neither way reaches it: the words stay, with why in plain words.
  reportsService = null
  const beforeOffline = problems.length
  await page.goto(`${base}/?fast&reportUnreachable&page=help/report`)
  await page.getByLabel('What happened?').fill('Sending a report sat on Sending.')
  await page.getByRole('button', { name: 'Send', exact: true }).click()
  const unreached = page.getByRole('alert').filter({ hasText: 'Your report wasn’t sent.' })
  await unreached.waitFor()
  assert.match(await unreached.innerText(), /Couldn’t reach Keevoro’s reports service just now\. Check this computer is online, then try again\./)
  assert.equal(
    await unreached.locator('.report-why').innerText(),
    'This computer’s network would not let Keevoro connect securely, as some work and school networks do.',
  )
  assert.equal(await page.getByLabel('What happened?').inputValue(), 'Sending a report sat on Sending.', 'their words are kept')
  await page.getByRole('button', { name: 'Try again' }).waitFor()
  await shot('13-report-unreachable')
  await fitsNarrow('A report that could not be sent either way')
  // The browser notes the address that did not answer; that was the point.
  problems.splice(beforeOffline, problems.length - beforeOffline, ...problems.slice(beforeOffline).filter((p) => !/ERR_INTERNET_DISCONNECTED|Failed to load resource|reports\.keevoro\.com/.test(p)))

  // --- A backup that is already somewhere: picked up on a new computer.
  await page.goto(`${base}/?demo=new&fast`)
  await page.getByRole('heading', { name: 'Let’s find your photos' }).waitFor()
  await page.getByRole('button', { name: 'I already have a Keevoro backup' }).click()
  await page.getByRole('heading', { name: 'Where is your backup?', level: 1 }).waitFor()
  assert.equal(
    await text('.open-from .status'),
    'Choose the drive, folder or cloud storage it is in. Keevoro reads the notes it keeps beside every backup, and changes nothing there until you say so.',
  )
  await shot('14-open-from-where')
  await fitsNarrow('Where is your backup?')
  await page.emulateMedia({ colorScheme: 'dark' })
  await shot('14-open-from-where-dark')
  await page.emulateMedia({ colorScheme: 'light' })
  // Cloud storage asks the same questions as anywhere else, and only looks.
  await page.getByRole('button', { name: /^Cloud storage/ }).click()
  await page.getByLabel('Bucket name').waitFor()
  assert.match(await text('.open-from .connect'), /Looking only reads\. If you open the backup, the keys are kept in your computer’s keychain\./)
  await page.getByRole('button', { name: 'Cancel' }).click()
  // A drive: what is there is said before anything is changed.
  await page.getByRole('button', { name: /^A drive or folder/ }).click()
  const foundThere = page.locator('.found-backup')
  await foundThere.waitFor()
  assert.match(
    await foundThere.innerText(),
    /Family photos\s+Holds\s+48,210 files, 312\.4 GB\s+Last backed up\s+21 November 2026\s+Last proved\s+15 November 2026\s+Notes written by\s+Keevoro 0\.49\.0\s+Where\s+F:\\Family Backup/,
  )
  assert.match(await foundThere.innerText(), /Its sources on the computer it came from: Family hard drive \(E:\\\), Mum’s card \(G:\\DCIM\)\./)
  assert.equal(await page.locator('main .primary').count(), 1, 'one filled button: Open this backup')
  await shot('14a-open-from-found')
  await fitsNarrow('What was found at the destination')
  await narrowShots('14a-open-from-found', foundThere)
  await page.getByRole('button', { name: 'Open this backup' }).click()
  await page.getByRole('heading', { name: '“Family photos” is open', level: 1 }).waitFor()
  // Where the photos live on this computer is the person's to say.
  const reconnect = page.locator('.reconnect')
  assert.match(
    await reconnect.innerText(),
    /Family hard drive: Keevoro does not know where these photos live on this computer\. On the computer this backup came from they were in E:\\\./,
  )
  assert.equal(await reconnect.getByRole('button', { name: 'Choose where they are…' }).count(), 2)
  assert.equal(await page.locator('main .primary').count(), 1, 'one filled button: Go to this backup')
  await shot('14b-open-from-reconnect')
  await fitsNarrow('Connecting sources again')
  await narrowShots('14b-open-from-reconnect', reconnect)
  await reconnect.getByRole('button', { name: 'Choose where they are…' }).first().click()
  await reconnect.getByText('Connected to D:\\Photos. Keevoro is looking through it now; files already backed up are not sent again.').waitFor()
  await page.getByRole('button', { name: 'Go to this backup' }).click()
  await page.locator('.sidebar .backup .current').getByText('Family photos').waitFor()
  // The source still waiting says so where the sources are, with the same question.
  await page.locator('.steps button').first().click()
  await page.getByRole('heading', { name: 'Where are your photos?', level: 1 }).waitFor()
  const waitingSource = page.locator('.source').filter({ hasText: 'Mum’s card' })
  assert.match(await waitingSource.innerText(), /Not connected on this computer yet/)
  await waitingSource.getByRole('button', { name: 'Choose where they are…' }).waitFor()
  await waitingSource.scrollIntoViewIfNeeded()
  await shot('14c-source-waiting')
  await fitsNarrow('A source waiting to be connected')
  // Opened again, it is the one already here, never a second.
  await page.getByRole('button', { name: 'Backup', exact: true }).click()
  await page.getByRole('button', { name: 'Manage backups…' }).click()
  await page.getByRole('heading', { name: 'Your backups', level: 1 }).waitFor()
  await page.getByRole('button', { name: 'Open a backup that is already somewhere…' }).click()
  await page.getByRole('button', { name: /^A drive or folder/ }).click()
  await page.locator('.found-backup').getByText('This backup is already on this computer, as “Family photos”.').waitFor()
  await page.getByRole('button', { name: 'Open it', exact: true }).waitFor()

  // Damaged notes: the copy before them is used, and said.
  await page.goto(`${base}/?demo=new&fast&opening=earlier`)
  await page.getByRole('button', { name: 'I already have a Keevoro backup' }).click()
  await page.getByRole('button', { name: /^A drive or folder/ }).click()
  await page.locator('.found-backup').waitFor()
  assert.match(await text('.found-backup .earlier-said'), /^Its newest notes are damaged, so Keevoro used the copy before them, from 14 November 2026\./)
  await shot('14d-open-from-earlier-notes')
  await fitsNarrow('Notes from the copy before')
  // Both damaged: said plainly, and nothing to open.
  await page.goto(`${base}/?demo=new&fast&opening=damaged`)
  await page.getByRole('button', { name: 'I already have a Keevoro backup' }).click()
  await page.getByRole('button', { name: /^A drive or folder/ }).click()
  await page.locator('.found-backup').getByText(/its notes and the copy before them are damaged/).waitFor()
  assert.equal(await page.getByRole('button', { name: 'Open this backup' }).count(), 0)
  await shot('14e-open-from-damaged')
  // Written by a newer Keevoro: refused plainly, never guessed at.
  await page.goto(`${base}/?demo=new&fast&opening=newer`)
  await page.getByRole('button', { name: 'I already have a Keevoro backup' }).click()
  await page.getByRole('button', { name: /^A drive or folder/ }).click()
  const newer = page.locator('.found-backup')
  await newer.getByRole('heading', { name: 'Update Keevoro first' }).waitFor()
  assert.match(await newer.innerText(), /made by a newer Keevoro \(Keevoro 9\.0\.0\) than this one \(Keevoro 0\.49\.0\)\. Update Keevoro first, then open it again\./)
  assert.equal(await page.getByRole('button', { name: 'Open this backup' }).count(), 0, 'nothing to open')
  await shot('14f-open-from-newer')
  await fitsNarrow('Notes from a newer Keevoro')
  await narrowShots('14f-open-from-newer', newer)
  await page.getByRole('button', { name: 'Look somewhere else' }).click()
  await page.getByRole('button', { name: /^A drive or folder/ }).waitFor()
  // Help's answer goes straight there.
  await page.goto(`${base}/?fast`)
  await page.getByRole('button', { name: 'Help', exact: true }).click()
  await page.locator('#setting-howNewComputer').getByRole('button', { name: 'Open a backup that is already somewhere' }).click()
  await page.getByRole('heading', { name: 'Where is your backup?', level: 1 }).waitFor()
  await page.getByRole('button', { name: '‹ Back' }).click()
  await page.getByRole('heading', { name: 'Your backups', level: 1 }).waitFor()

  // --- Detail on demand. An "i" beside a name shows a few sentences when
  // the pointer rests on it, when the keyboard reaches it, and when it is
  // clicked or tapped; Escape or a click elsewhere puts it away. It is
  // drawn on top of everything, never cut off by a panel that scrolls.
  await page.goto(`${base}/?fast&page=settings/byItself`)
  await page.getByRole('heading', { name: 'Backing up by itself', level: 2 }).waitFor()
  const batteryTip = page.locator('#setting-minimise').getByRole('button', { name: 'More about Minimise to the icon by the clock' })
  const tooltip = page.getByRole('tooltip')
  assert.equal(await batteryTip.getAttribute('aria-expanded'), 'false', 'shut until asked for')
  assert.equal(await tooltip.count(), 0)
  const tipBox = await batteryTip.boundingBox()
  assert.ok(tipBox.width >= 24 && tipBox.height >= 24, `the “i” is easy to press (${tipBox.width} by ${tipBox.height})`)
  // Resting the pointer on it.
  await batteryTip.hover()
  await tooltip.waitFor()
  assert.equal(await tooltip.innerText(), 'Minimising then hides the window there, instead of leaving it on the taskbar.')
  assert.equal(await batteryTip.getAttribute('aria-expanded'), 'true')
  assert.equal(await batteryTip.getAttribute('aria-describedby'), await tooltip.getAttribute('id'), 'and the “i” is described by it')
  // The pointer can cross into it, and its words can be selected and copied.
  await tooltip.hover()
  await tooltip.selectText()
  assert.match(await page.evaluate(() => String(window.getSelection())), /instead of leaving it on the taskbar/)
  assert.ok(await tooltip.isVisible(), 'still open with the pointer over it')
  await shot('15-info-tip-hover')
  await page.mouse.move(5, 5)
  await tooltip.waitFor({ state: 'detached' })
  // From the keyboard: Tab reaches it and it opens; Escape shuts it, leaves the
  // focus on the "i", and closes nothing else.
  await page.locator('#setting-keepRunning').getByRole('switch').focus()
  await page.keyboard.press('Tab')
  assert.equal(await batteryTip.evaluate((el) => el === document.activeElement), true, 'the next row’s “i”, before its switch')
  await tooltip.waitFor()
  assert.equal(await batteryTip.getAttribute('aria-expanded'), 'true', 'opened by the keyboard alone')
  await shot('15-info-tip-keyboard')
  await page.keyboard.press('Escape')
  await tooltip.waitFor({ state: 'detached' })
  assert.equal(await batteryTip.evaluate((el) => el === document.activeElement), true, 'the focus stays on the “i”')
  assert.equal(await page.getByRole('heading', { name: 'Backing up by itself', level: 2 }).count(), 1, 'Escape closed only the tip')
  await page.keyboard.press('Enter')
  await tooltip.waitFor()
  await page.keyboard.press('Enter')
  await tooltip.waitFor({ state: 'detached' })
  // A click keeps it open after the pointer leaves, until a click anywhere else.
  await batteryTip.click()
  await page.mouse.move(5, 5)
  await page.getByRole('heading', { name: 'Backing up by itself', level: 2 }).hover()
  assert.ok(await tooltip.isVisible(), 'a click keeps it open')
  await page.getByRole('heading', { name: 'Backing up by itself', level: 2 }).click()
  await tooltip.waitFor({ state: 'detached' })
  // Never cut off: low in a panel that scrolls, it is drawn whole, inside the window and on top.
  const lowTip = page.locator('#setting-startWithComputer').getByRole('button', { name: /^More about / })
  await lowTip.scrollIntoViewIfNeeded()
  await lowTip.click()
  await tooltip.waitFor()
  await settled()
  const whole = await tooltip.evaluate((box) => {
    const r = box.getBoundingClientRect()
    const corners = [[r.left + 4, r.top + 4], [r.right - 4, r.top + 4], [r.left + 4, r.bottom - 4], [r.right - 4, r.bottom - 4]]
    return {
      inside: r.left >= 0 && r.top >= 0 && r.right <= window.innerWidth && r.bottom <= window.innerHeight,
      onTop: corners.every(([x, y]) => box.contains(document.elementFromPoint(x, y))),
    }
  })
  assert.deepEqual(whole, { inside: true, onTop: true }, 'the tip is drawn whole, inside the window, on top of the page')
  await page.keyboard.press('Escape')
  // Dark, too.
  await page.emulateMedia({ colorScheme: 'dark' })
  await batteryTip.click()
  await tooltip.waitFor()
  await shot('15-info-tip-dark')
  await page.keyboard.press('Escape')
  await page.emulateMedia({ colorScheme: 'light' })
  // On a touch screen 400 pixels wide, a tap opens it, inside the gutters, and another tap shuts it.
  const touch = await browser.newContext({ hasTouch: true, isMobile: true, viewport: { width: 400, height: 800 } })
  const phone = await touch.newPage()
  await phone.goto(`${base}/?fast&page=settings/byItself`)
  const phoneTip = phone.locator('#setting-minimise').getByRole('button', { name: 'More about Minimise to the icon by the clock' })
  await phoneTip.scrollIntoViewIfNeeded()
  await phoneTip.tap()
  const phoneTooltip = phone.getByRole('tooltip')
  await phoneTooltip.waitFor()
  const onPhone = await phoneTooltip.boundingBox()
  assert.ok(onPhone.x >= 16 && onPhone.x + onPhone.width <= 400 - 16 + 1, `inside the gutters at 400 pixels (${JSON.stringify(onPhone)})`)
  await phone.evaluate(() => document.fonts.ready)
  await phone.screenshot({ path: join(shots, '15-info-tip-400-touch.png') })
  await phoneTip.tap()
  await phoneTooltip.waitFor({ state: 'detached' })
  await touch.close()

  // Kinds of file says what it is for in view; its finer points wait behind one "i" at the end of that.
  await page.goto(`${base}/?fast&page=settings/advanced/kinds`)
  await page.locator('.kinds .kv-intro').waitFor()
  assert.match(await text('.kinds .kv-intro'), /^Keevoro finds photos, videos and music by the ending of their names, such as \.jpg or \.mp3\. Add one that is\s+missing; switching one off never takes away anything already backed up\.$/)
  assert.equal(await page.locator('.kinds details.more-about').count(), 0, 'nothing to open to understand it')
  assert.match(await tipSays('kinds of file'), /is backed up\s+exactly as it is, with “No preview” where its picture would be/)

  // --- Every "i" is drawn the same wherever it is: no style for the
  // buttons around it reaches it, as one did between the choices of a
  // question, where it looked like a broken, empty button. And none sits
  // between two buttons as if it were one of them.
  const tipLooks = (where) =>
    page.evaluate((where) => {
      const look = (b) => {
        const c = getComputedStyle(b)
        const r = b.getBoundingClientRect()
        return JSON.stringify({
          width: Math.round(r.width),
          height: Math.round(r.height),
          border: `${c.borderTopWidth} ${c.borderTopStyle} ${c.borderRightWidth} ${c.borderBottomWidth} ${c.borderLeftWidth}`,
          background: `${c.backgroundColor} ${c.backgroundImage}`,
          padding: c.padding,
          radius: c.borderTopLeftRadius,
          shadow: c.boxShadow,
          opacity: c.opacity,
        })
      }
      const tips = [...document.querySelectorAll('.info-tip-button')].filter((b) => b.checkVisibility())
      const between = tips
        .map((b) => b.closest('.info-tip'))
        .filter((tip) => {
          const isButton = (el) => el && (el.matches('button:not(.link)') || el.matches('.with-tip:has(> button:not(.link))'))
          return (isButton(tip.previousElementSibling) && isButton(tip.nextElementSibling)) || tip.parentElement.matches('.choices')
        })
        .map((tip) => tip.querySelector('button').getAttribute('aria-label'))
      return { where, count: tips.length, looks: [...new Set(tips.map(look))], between }
    }, where)
  // An "i" put, on purpose, among the buttons of every kind of row that has rules for its buttons.
  const tipAmongButtons = () =>
    page.evaluate(() => {
      const tip = document.querySelector('.info-tip')
      const holder = document.createElement('div')
      holder.id = 'tips-among-buttons'
      holder.innerHTML = [
        '<div class="panel asking"><p class="choices"><button class="primary">A</button>TIP<button>B</button></p></div>',
        '<div class="panel send"><div class="choices actions"><button class="button">A</button>TIP</div></div>',
        '<ul class="kept"><li class="source"><span class="confirm"><button>A</button>TIP</span></li></ul>',
        '<div class="update"><p class="actions"><button>A</button>TIP</p></div>',
        '<div class="backup"><div class="menu"><form><button>A</button>TIP</form></div></div>',
        '<p class="row-actions"><button class="button small">A</button>TIP</p>',
        '<div class="themes">TIP</div><div class="segmented">TIP</div>',
      ]
        .join('')
        .replaceAll('TIP', tip.outerHTML)
      document.querySelector('main').append(holder)
    })
  let tipLook = null
  const tipsLookAlike = async (where) => {
    const seen = await tipLooks(where)
    assert.ok(seen.count > 0, `${where}: an “i” to check`)
    assert.equal(seen.looks.length, 1, `${where}: every “i” is drawn the same: ${seen.looks.join(' / ')}`)
    tipLook ??= seen.looks[0]
    assert.equal(seen.looks[0], tipLook, `${where}: drawn as on the other screens`)
    assert.deepEqual(seen.between, [], `${where}: no “i” between two buttons`)
    return seen.count
  }
  const tipsOn = {}
  await page.goto(`${base}/?demo=new&fast`)
  await page.getByRole('heading', { name: 'Let’s find your photos' }).waitFor()
  tipsOn['Welcome'] = await tipsLookAlike('The first screen')
  for (const [section, name] of [
    ['backup', 'Settings, This backup'],
    ['byItself', 'Settings, Backing up by itself'],
    ['advanced', 'Settings, Advanced'],
    ['advanced/kinds', 'Settings, Kinds of file'],
    ['advanced/agents', 'Settings, AI agents'],
  ]) {
    await page.goto(`${base}/?fast&page=settings/${section}`)
    await page.locator('.settings h2, .kv-page h2, main h2').first().waitFor()
    await page.waitForLoadState('networkidle')
    tipsOn[name] = await tipsLookAlike(name)
    // Each row's control is level with its name and sentence, a status line beneath or not.
    const unlevel = await page.evaluate(() =>
      [...document.querySelectorAll('.kv-row-main')]
        .filter((row) => row.querySelector(':scope > .kv-row-end') && row.checkVisibility())
        .map((row) => {
          const middle = (el) => el.getBoundingClientRect().top + el.getBoundingClientRect().height / 2
          const off = Math.abs(middle(row.querySelector(':scope > .kv-row-text')) - middle(row.querySelector(':scope > .kv-row-end')))
          return off > 2 ? `${row.querySelector('.kv-row-title').textContent} (${Math.round(off)} apart)` : null
        })
        .filter(Boolean),
    )
    assert.deepEqual(unlevel, [], `${name}: every row’s control is level with its words`)
  }
  await page.goto(`${base}/?fast`)
  await page.getByRole('button', { name: 'Choose where they go' }).click()
  await page.getByRole('button', { name: /Google Cloud Storage/ }).click()
  await page.getByLabel('Bucket name').waitFor()
  tipsOn['Google Cloud Storage form'] = await tipsLookAlike('The Google Cloud Storage form')
  await page.goto(`${base}/?demo=empty&fast`)
  await page.getByRole('button', { name: /Windows \(C:\)/ }).click()
  await page.getByRole('button', { name: 'Find the folders with photos' }).click()
  await page.locator('.survey > .hints').waitFor()
  tipsOn['The survey of a disk'] = await tipsLookAlike('The survey of a disk')
  await tipAmongButtons()
  const among = await tipLooks('among buttons')
  assert.equal(among.looks.length, 1, `an “i” among any buttons is drawn as anywhere else: ${among.looks.join(' / ')}`)
  assert.equal(among.looks[0], tipLook, 'and the same as on every screen')
  console.log(`The “i”s on the main screens: ${Object.entries(tipsOn).map(([name, n]) => `${name} ${n}`).join(', ')}.`)

  // --- Music, a kind of its own beside photos and videos. A backup says
  // what it is for, up front, with three equal switches, and every word
  // follows it.
  await page.goto(`${base}/?demo=new&fast`)
  await page.getByRole('heading', { name: 'Let’s find your photos' }).waitFor()
  const upFront = page.getByRole('group', { name: 'What do you want to back up?' })
  assert.deepEqual(await upFront.getByRole('switch').evaluateAll((all) => all.map((b) => b.parentElement.innerText.trim())), ['Photos', 'Videos', 'Music'], 'three equal choices, and no other files')
  assert.deepEqual(await upFront.getByRole('switch').evaluateAll((all) => all.map((b) => b.getAttribute('aria-checked'))), ['true', 'true', 'false'], 'photos and videos to begin with')
  assert.deepEqual(await centredOnSwitch(upFront.getByRole('switch')), [])
  // Just my music: the words follow at once, the suggested name too.
  await upFront.getByRole('switch', { name: 'Music' }).click()
  await upFront.getByRole('switch', { name: 'Photos' }).click()
  await upFront.getByRole('switch', { name: 'Videos' }).click()
  await page.getByRole('heading', { name: 'Let’s find your music' }).waitFor()
  assert.match(await text('.welcome > p'), /^Keevoro finds the music on your drives/)
  assert.equal(await page.getByLabel('What shall we call this backup?').inputValue(), 'My music')
  // The last one on stays on, and says why plainly.
  await upFront.getByRole('switch', { name: 'Music' }).click({ force: true })
  await upFront.getByRole('status').getByText('Keep at least one on.').waitFor()
  assert.equal(await upFront.getByRole('switch', { name: 'Music' }).getAttribute('aria-checked'), 'true')
  assert.equal(await page.locator('.welcome .primary').count(), 1, 'still one way forward')
  await shot('m1-welcome-music')
  await fitsNarrow('The first screen, for music')
  await narrowShots('m1-welcome-music', upFront)
  await page.getByRole('button', { name: 'Get started' }).click()
  // Step 1 names the music, and says what the backup is for in a line near the top.
  await page.getByRole('heading', { name: 'Where is your music?', level: 1 }).waitFor()
  assert.match(await currentStep(), /^1\s+Find your music/)
  assert.equal(await text('.find header .status'), 'A source is anywhere your music might be: a drive, USB stick, memory card or folder. Keevoro only ever reads it.')
  assert.match(await text('.backing-up-line'), /^Backing up: Music\s+Change$/)
  await page.getByRole('button', { name: 'Choose a folder…' }).click()
  await page.getByRole('heading', { name: /^Found 18 tracks on Music$/ }).waitFor()
  assert.match(await text('.kept li.source .count'), /^18 tracks\s/)
  assert.match(await text('.next .say'), /^18 tracks found so far\.$/)
  await shot('m2-find-music')
  await fitsNarrow('Find your music')
  // A music source looked inside: a music note where each picture would be, never a blank tile.
  await page.getByRole('button', { name: 'Look inside Music' }).click()
  await page.locator('.music-tile').first().waitFor()
  assert.equal(await page.locator('.inside img').count(), 0, 'music is never opened for a picture')
  assert.match(await page.locator('.music-tile').first().innerText(), /Track 01\.flac\s+No preview/)
  await shot('m3-music-source-tiles')
  await fitsNarrow('A music source looked inside')
  await narrowShots('m3-music-source-tiles', page.locator('.music-tile').first())
  await page.getByRole('button', { name: '‹ Back' }).first().click()
  // Nothing to arrange by date: a backup of music alone keeps its folders, and is not asked.
  await page.getByRole('button', { name: 'See what was found' }).click()
  await page.getByRole('heading', { name: 'Which of these should we keep?', level: 1 }).waitFor()
  assert.match(await text('.discover header .status'), /^Keevoro has sorted your music by album, as its folders are\./)
  const albums = page.getByRole('region', { name: 'Music' })
  assert.deepEqual(await albums.locator('.folder-words b').allInnerTexts(), ['Pretend Songs', 'Kitchen Sessions', 'Garden in spring'], 'an album is its folder')
  assert.match(await albums.innerText(), /^Music\s+18 tracks in 3 albums · [\d.]+ (MB|GB)\. Open an album to choose\s+track by track\./)
  assert.match(await text('.tally'), /of 18 tracks found/)
  assert.equal(await page.getByRole('button', { name: /^Set aside/ }).count(), 0, 'nothing is set aside from music')
  assert.match(await text('.next .say'), /^Keeping 18 tracks in 3 albums\.$/)
  await shot('m4-choose-music')
  await fitsNarrow('Choose what to keep, music')
  await albums.getByRole('button', { name: 'Open Pretend Songs' }).click()
  await page.getByRole('heading', { name: 'Pretend Songs', level: 1 }).waitFor()
  assert.equal(await page.getByRole('table', { name: 'Files in Pretend Songs' }).locator('tbody tr').count(), 8)
  await page.getByRole('button', { name: '‹ Back' }).click()
  await page.getByRole('button', { name: 'Choose where they go' }).click()
  assert.match(await text('.arranged'), /^Your folders are kept as they are\./)
  // A new backup from the switcher asks the same question.
  await page.getByRole('button', { name: 'Backup', exact: true }).click()
  await page.getByRole('button', { name: 'Start a new backup…' }).click()
  const askedAgain = page.locator('.backup .menu').getByRole('group', { name: 'What do you want to back up?' })
  assert.deepEqual(await askedAgain.getByRole('switch').evaluateAll((all) => all.map((b) => b.getAttribute('aria-checked'))), ['true', 'true', 'false'])
  await shot('m5-new-backup-asks')
  await fitsNarrow('A new backup, asked what it is for')
  await page.keyboard.press('Escape')

  // --- An existing backup: "Backing up: Photos, Videos" with Change, in place.
  await page.goto(`${base}/?fast`)
  await page.getByRole('heading', { name: 'Which of these should we keep?', level: 1 }).waitFor()
  assert.equal(await page.getByRole('region', { name: 'Music' }).count(), 0, 'no music until asked for')
  await page.locator('.steps button').first().click()
  await page.getByRole('heading', { name: 'Where are your photos?', level: 1 }).waitFor()
  assert.match(await text('.backing-up-line'), /^Backing up: Photos, Videos\s+Change$/)
  await page.locator('.backing-up-line').getByRole('button', { name: 'Change' }).click()
  const changing = page.locator('.backing-up').getByRole('group', { name: 'What do you want to back up?' })
  assert.deepEqual(await changing.getByRole('switch').evaluateAll((all) => all.map((b) => b.parentElement.innerText.trim())), ['Photos', 'Videos', 'Music'], 'the same three, in place')
  await shot('m6-change-line')
  await changing.getByRole('switch', { name: 'Music' }).click()
  await page.locator('.backing-up').getByRole('status').getByText(/Keevoro is looking through your sources for your music now\./).waitFor()
  assert.match(await text('.backing-up-line'), /^Backing up: Photos, Videos, Music\s+Done$/)
  // The words follow the backup on each step.
  await page.getByRole('heading', { name: 'Where are your photos and music?', level: 1 }).waitFor()
  assert.match(await currentStep(), /^1\s+Find your photos and music/)
  await page.getByRole('heading', { name: /^Found [\d,]+ photos(, [\d,]+ videos)? and 18 tracks on Family hard drive$/ }).waitFor()
  await shot('m7-mixed-found')
  await fitsNarrow('Find your photos and music')
  await narrowShots('m6-change-line', page.locator('.backing-up'))
  await page.getByRole('button', { name: 'See what was found' }).click()
  await page.getByRole('heading', { name: 'Which of these should we keep?', level: 1 }).waitFor()
  await page.getByRole('region', { name: 'Music' }).waitFor()
  assert.match(await text('.tally'), /of [\d,]+ photos, [\d,]+ videos and 18 tracks found/)
  assert.match(await text('.next .say'), /^Keeping [\d,]+ files in \d+ collections and 3 albums\.$/)
  await page.getByRole('region', { name: 'Music' }).scrollIntoViewIfNeeded()
  await shot('m8-mixed-choose')
  await fitsNarrow('Choose what to keep, photos and music')
  // Arranged by date, music keeps its folders, said in one short line.
  await page.getByRole('button', { name: 'Choose where they go' }).click()
  assert.match(await text('.arranged'), /^Arranged by date\. Music keeps its folders\./)
  // Settings, This backup: the same choice, as now.
  await page.getByRole('button', { name: 'Settings', exact: true }).click()
  await page.getByRole('navigation', { name: 'Settings sections' }).getByRole('button', { name: 'This backup' }).click()
  const backupIsFor = page.getByRole('group', { name: 'This backup is for' })
  assert.equal(await backupIsFor.getByRole('switch', { name: 'Music' }).isChecked(), true)
  await backupIsFor.scrollIntoViewIfNeeded()
  await shot('m9-settings-music')
  await fitsNarrow('Settings, This backup, with music')
  await page.getByRole('button', { name: '‹ Back', exact: true }).click()
  // Per source: Photos, Videos and Music, as the rest of the backup is.
  await page.locator('.steps button').first().click()
  await page.getByRole('button', { name: /^File types on / }).click()
  const fromDrive = page.getByRole('group', { name: 'Back up from Family hard drive' })
  assert.deepEqual(await fromDrive.getByRole('switch').evaluateAll((all) => all.map((b) => `${b.parentElement.innerText.trim()} ${b.getAttribute('aria-checked')}`)), ['Photos true', 'Videos true', 'Music true'])
  await fromDrive.getByRole('switch', { name: 'Music' }).click()
  assert.equal(await text('.source-wants .differs'), 'Different from the rest of this backup. Use the usual')
  await shot('m10-source-switches')
  await page.locator('.source-wants .differs').getByRole('button', { name: 'Use the usual' }).click()
  assert.equal(await page.locator('.source-wants .differs').count(), 0, 'matches again, so follows again')
  // Kinds of file: music among the kinds, each saying what it is, and found by a search.
  await page.goto(`${base}/?fast&page=settings/advanced/kinds`)
  await page.getByRole('heading', { name: 'Kinds of file Keevoro looks for', level: 2 }).waitFor()
  const musicKinds = page.locator('.kinds-table')
  const musicRow = (ending) => musicKinds.getByRole('row').filter({ has: page.locator(`td.ending > span:text-is("${ending}")`) })
  assert.match(await musicRow('.mp3').innerText(), /Music\s+Built in/)
  assert.equal(await tipSays('.mp3', musicRow('.mp3')), 'The most common music format, played almost everywhere.')
  const findMusic = page.getByRole('searchbox', { name: 'Find a kind of file' })
  await findMusic.fill('FLAC')
  await musicRow('.flac').waitFor()
  assert.match(await musicRow('.flac').innerText(), /Music\s+Built in/)
  await findMusic.fill('playlist')
  await musicRow('.m3u').waitFor()
  assert.deepEqual(await musicKinds.locator('td.ending > span:not(.info-tip)').allInnerTexts(), ['.m3u', '.m3u8', '.pls'])
  await findMusic.fill('audiobook')
  await musicRow('.m4b').waitFor()
  await shot('m11-kinds-music')
  await fitsNarrow('Kinds of file, music')

  assert.deepEqual(problems, [], 'the browser reported no errors')
  console.log(`UI test passed. Screenshots are in ${shots}`)
} finally {
  await browser.close()
  server.close()
}
