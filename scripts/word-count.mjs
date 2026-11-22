// Counts the words a person sees on each of the main screens, against the
// stand-in backend (ui/src/demo.ts), so a screen that grows wordy shows up
// as a number. Only words that are drawn count: a closed "More about this"
// or an "i" not yet opened holds words a person reads only when they ask.
// The side band is left out, as it is the same on every screen.
//
//   npm run ui:build && node scripts/word-count.mjs
//
// Prints one line per screen. With --json it prints the figures as JSON.

import { createServer } from 'node:http'
import { existsSync, readFileSync } from 'node:fs'
import { extname, join, resolve } from 'node:path'
import { chromium } from 'playwright'

const dist = resolve('dist')
if (!existsSync(join(dist, 'index.html'))) {
  console.error('Build the screens first: npm run ui:build')
  process.exit(1)
}
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
// Nothing is fetched from the internet.
await page.route(/^https:\/\//, (route) => route.abort('internetdisconnected'))

/** The words drawn on the page now (or in `within`), outside the side band, in the whole of its scroll. */
export const visibleWords = (p, within = 'body') =>
  p.evaluate((within) => {
    const skip = (el) => el.closest('.sidebar, .visually-hidden, [aria-hidden="true"], .info-tip-box')
    let words = 0
    const root = document.querySelector(within) ?? document.body
    const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT)
    for (let node = walker.nextNode(); node; node = walker.nextNode()) {
      const el = node.parentElement
      if (!el || skip(el) || !node.textContent.trim()) continue
      if (!el.checkVisibility({ visibilityProperty: true, opacityProperty: true })) continue
      // Text inside a closed disclosure is not drawn, though the element still has a box in some engines.
      const shut = el.closest('details:not([open])')
      if (shut && !el.closest('summary')) continue
      words += node.textContent.split(/\s+/).filter((w) => /[\p{L}\p{N}]/u.test(w)).length
    }
    // Words typed into fields count too: a name already suggested is read.
    for (const field of root.querySelectorAll('input[type=text], input:not([type])')) {
      if (field.checkVisibility() && field.value && !skip(field)) words += field.value.split(/\s+/).length
    }
    return words
  }, within)

const screens = []
const count = async (name, within) => {
  await page.evaluate(() => document.fonts.ready)
  screens.push({ screen: name, words: await visibleWords(page, within) })
}
const heading = (name) => page.getByRole('heading', { name, level: 1 }).waitFor()
const settings = async (section) => {
  await page.goto(`${base}/?fast&page=settings/${section}`)
  await page.locator('.settings h2, .kv-page h2, main h2').first().waitFor()
  await page.waitForLoadState('networkidle')
}

try {
  await page.goto(`${base}/?demo=new&fast`)
  await page.getByRole('heading', { name: 'Let’s find your photos' }).waitFor()
  await count('Welcome')

  await page.getByRole('button', { name: 'I already have a Keevoro backup' }).click()
  await heading('Where is your backup?')
  await count('Open from: where is your backup?')

  await page.goto(`${base}/?demo=empty&fast`)
  await heading('Where are your photos?')
  await count('Find your photos (no sources yet)')

  await page.goto(`${base}/?fast`)
  await heading('Which of these should we keep?')
  await count('Choose what to keep')

  await page.getByRole('button', { name: /^Find your photos/ }).click()
  await heading('Where are your photos?')
  await page.getByRole('button', { name: /^File types on / }).first().click()
  await page.locator('.types').waitFor()
  await count('Find your photos, a source’s File types open')

  await page.goto(`${base}/?fast`)
  await heading('Which of these should we keep?')
  await page.getByRole('button', { name: 'Choose where they go' }).click()
  await heading('Where should they go?')
  await page.locator('.destinations .place').first().waitFor()
  await count('Backup screen, choosing a destination')

  await page.getByRole('button', { name: /Cloudflare R2/ }).click()
  await page.getByLabel('Cloudflare account ID').waitFor()
  await count('Destination form: Cloudflare R2')
  await page.getByRole('button', { name: 'Cancel' }).click()
  await page.getByRole('button', { name: /Google Cloud Storage/ }).click()
  await page.getByLabel('Bucket name').waitFor()
  await count('Destination form: Google Cloud Storage')
  await page.getByRole('button', { name: 'Cancel' }).click()

  await page.getByRole('button', { name: /^A folder/ }).click()
  await page.getByRole('heading', { name: /^Backing up to the folder/ }).waitFor()
  await count('Backup screen, ready to back up')
  await page.getByRole('button', { name: 'Back up now' }).click()
  await page.locator('.send .done').first().waitFor()
  await page.getByRole('button', { name: 'Back up again' }).waitFor()
  await count('Backup screen, after a backup')

  for (const [section, name] of [
    ['backup', 'Settings: This backup'],
    ['byItself', 'Settings: Backing up by itself'],
    ['appearance', 'Settings: Appearance'],
    ['updates', 'Settings: Updates'],
    ['advanced', 'Settings: Advanced'],
    ['advanced/kinds', 'Settings: Kinds of file'],
  ]) {
    await settings(section)
    await count(name)
  }

  await page.goto(`${base}/?fast`)
  await heading('Which of these should we keep?')
  await page.getByRole('button', { name: 'Backup', exact: true }).click()
  await page.getByRole('button', { name: 'Manage backups…' }).click()
  await heading('Your backups')
  await page.waitForFunction(() => document.querySelectorAll('.backup-row .facts').length > 0)
  await count('Manage backups')

  await page.goto(`${base}/?closing&fast`)
  await page.getByRole('dialog').first().waitFor()
  await count('The first-close explanation', '[role=dialog]')

  // Help and About are where people go on purpose to read; counted for information.
  await page.goto(`${base}/?fast&page=help/howTo`)
  await page.getByRole('heading', { name: 'How do I…', level: 2 }).waitFor()
  await count('Help: How do I… (for information)')
  await page.goto(`${base}/?fast&page=help/about`)
  await page.getByRole('heading', { name: 'About Keevoro', level: 2 }).waitFor()
  await count('Help: About Keevoro (for information)')
} finally {
  await browser.close()
  server.close()
}

if (process.argv.includes('--json')) console.log(JSON.stringify(screens, null, 2))
else for (const { screen, words } of screens) console.log(`${String(words).padStart(5)}  ${screen}`)
