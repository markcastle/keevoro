import assert from 'node:assert/strict'
import { test } from 'node:test'
import {
  STRATUS_PAGE,
  currencyFor,
  fetchedSaid,
  gigabytesToAsk,
  kindLine,
  money,
  rowsOf,
  setUpFor,
  sizeSaid,
  sizeSteps,
} from './stratus.ts'
import type { Comparison, StorageOption } from './stratus.ts'

test('the currency is guessed from the computer’s locale', () => {
  assert.equal(currencyFor('en-GB'), 'GBP')
  assert.equal(currencyFor('cy-GB'), 'GBP')
  assert.equal(currencyFor('en_GB'), 'GBP', 'written the way some systems write it')
  assert.equal(currencyFor('fr-FR'), 'EUR')
  assert.equal(currencyFor('en-IE'), 'EUR')
  assert.equal(currencyFor('de'), 'EUR', 'a language spoken mostly in euro countries')
  assert.equal(currencyFor('de-CH'), 'USD', 'Switzerland pays in francs, which Stratus does not offer')
  assert.equal(currencyFor('en-US'), 'USD')
  assert.equal(currencyFor('zh-Hant-TW'), 'USD')
  assert.equal(currencyFor(''), 'USD')
  assert.equal(currencyFor(undefined), 'USD')
})

test('the size asked about is what is kept, rounded up to two figures', () => {
  assert.equal(gigabytesToAsk(0), 1)
  assert.equal(gigabytesToAsk(250e6), 1, 'never less than 1 GB')
  assert.equal(gigabytesToAsk(7.2e9), 8)
  assert.equal(gigabytesToAsk(10e9), 10)
  assert.equal(gigabytesToAsk(123.4e9), 130)
  assert.equal(gigabytesToAsk(130e9), 130, 'an exact figure stays as it is')
  assert.equal(gigabytesToAsk(1.234e12), 1300)
  assert.equal(gigabytesToAsk(Number.NaN), 1)
})

test('sizes are said as storage is sold, and the steps to choose from include this backup’s', () => {
  assert.equal(sizeSaid(1), '1 GB')
  assert.equal(sizeSaid(240), '240 GB')
  assert.equal(sizeSaid(1000), '1 TB')
  assert.equal(sizeSaid(2500), '2.5 TB')
  assert.equal(sizeSaid(1300), '1.3 TB')
  assert.equal(sizeSaid(10000), '10 TB')
  assert.deepEqual(sizeSteps(320), [100, 250, 320, 500, 1000, 2000, 5000, 10000])
  assert.deepEqual(sizeSteps(500), [100, 250, 500, 1000, 2000, 5000, 10000], 'never twice')
  assert.deepEqual(sizeSteps(null), [100, 250, 500, 1000, 2000, 5000, 10000])
})

test('a price is said in its currency, to the penny or cent', () => {
  assert.equal(money(1.4, 'GBP'), '£1.40')
  assert.equal(money(1234.5, 'USD'), '$1,234.50')
  assert.equal(money(0.3, 'EUR'), '€0.30')
  assert.equal(money(0.004, 'GBP'), 'under £0.01')
  assert.equal(money(0, 'GBP'), '£0.00')
})

test('when the prices were fetched is said as a person would', () => {
  const now = new Date(2026, 10, 2, 14, 30)
  assert.equal(fetchedSaid(new Date(2026, 10, 2, 6, 15).toISOString(), now), 'today at 06:15')
  assert.equal(fetchedSaid(new Date(2026, 10, 1, 21, 5).toISOString(), now), 'yesterday at 21:05')
  assert.equal(fetchedSaid(new Date(2026, 9, 28, 9, 0).toISOString(), now), 'on 28 October')
  assert.equal(fetchedSaid(null, now), 'just now')
  assert.equal(fetchedSaid('not a time', now), 'just now')
})

const option = (fields: Partial<StorageOption>): StorageOption => ({
  id: 'r2-standard',
  provider: 'cloudflare-r2',
  providerName: 'Cloudflare R2',
  name: 'R2 Standard',
  kind: 'object',
  capacityGB: null,
  metered: true,
  lifetime: false,
  monthly: 2.8,
  yearly: 33.6,
  url: 'https://developers.cloudflare.com/r2/pricing/',
  ...fields,
})

test('Cloudflare R2, Amazon S3 and Google Cloud open their own forms, and other S3 storage “Other storage”', () => {
  assert.deepEqual(setUpFor(option({})), { kind: 'r2', place: '' })
  assert.deepEqual(setUpFor(option({ provider: 'aws-s3', name: 'S3 Standard' })), { kind: 's3', place: '' })
  assert.deepEqual(setUpFor(option({ provider: 'gcs', name: 'GCS Standard' })), { kind: 'gcs', place: '' })
  for (const provider of ['backblaze-b2', 'wasabi', 'digitalocean', 'scaleway', 'ovh']) {
    assert.deepEqual(setUpFor(option({ provider, name: 'Standard' })), { kind: 'endpoint', place: '' }, provider)
  }
  for (const provider of ['azure', 'apple', 'google', 'dropbox', 'pcloud', 'hetzner']) {
    assert.equal(setUpFor(option({ provider })), null, provider)
  }
  // Archive storage must be restored before a file can be read, and Keevoro reads every copy back.
  for (const name of ['S3 Glacier Deep Archive', 'S3 Glacier Flexible Retrieval', 'OVHcloud Cold Archive', 'Scaleway Glacier']) {
    assert.equal(setUpFor(option({ provider: 'aws-s3', name })), null, name)
  }
  assert.deepEqual(setUpFor(option({ provider: 'aws-s3', name: 'S3 Glacier Instant Retrieval' })), { kind: 's3', place: '' })
  assert.deepEqual(setUpFor(option({ provider: 'gcs', name: 'GCS Archive' })), { kind: 'gcs', place: '' }, 'read at once, at a charge')
})

test('each kind of storage is said in one short plain line', () => {
  assert.equal(kindLine(option({})), 'Storage you rent by the GB, that programs like Keevoro back up to.')
  assert.equal(kindLine(option({ name: 'R2 Infrequent Access' })), 'Storage you rent by the GB, for files rarely read. Reading them back costs extra.')
  assert.equal(kindLine(option({ provider: 'gcs', name: 'GCS Archive' })), 'Storage you rent by the GB, for files rarely read. Reading them back costs extra.')
  assert.equal(kindLine(option({ provider: 'aws-s3', name: 'S3 Glacier Deep Archive' })), 'Archive storage. Files must be restored, which takes hours, before they can be read.')
  assert.equal(kindLine(option({ provider: 'azure', name: 'Azure Blob Hot (LRS)' })), 'Storage you rent by the GB, of a kind Keevoro doesn’t speak.')
  assert.equal(kindLine(option({ provider: 'apple', kind: 'consumer', metered: false, name: 'iCloud+ 2TB' })), 'A plan for your phone and computer, like iCloud+.')
  assert.equal(
    kindLine(option({ provider: 'pcloud', kind: 'consumer', metered: false, lifetime: true, name: 'pCloud Lifetime 500GB' })),
    'A plan you pay for once, shown spread over five years.',
  )
  for (const said of [kindLine(option({})), kindLine(option({ provider: 'apple', kind: 'consumer' }))]) {
    assert.ok(said.length <= 90, said)
    assert.doesNotMatch(said, /—|–/)
  }
})

const comparison: Comparison = {
  gigabytes: 240,
  currency: 'GBP',
  fetchedAt: '2026-11-02T06:15:00Z',
  notRefreshed: ['gcs', 'apple', 'fx'],
  options: [
    option({ id: 's3-deep', provider: 'aws-s3', providerName: 'Amazon S3', name: 'S3 Glacier Deep Archive', monthly: 0.25 }),
    option({ id: 'gcs-archive', provider: 'gcs', providerName: 'Google Cloud Storage', name: 'GCS Archive', monthly: 0.3 }),
    option({ id: 'b2', provider: 'backblaze-b2', providerName: 'Backblaze B2', name: 'B2 Cloud Storage', monthly: 1.4 }),
    option({ id: 'icloud-2tb', provider: 'apple', providerName: 'Apple iCloud+', name: 'iCloud+ 2TB', kind: 'consumer', metered: false, capacityGB: 2000, monthly: 8.99 }),
  ],
}

test('the rows are cheapest first, with the cheapest Keevoro can use marked and old prices said', () => {
  const rows = rowsOf(comparison)
  assert.deepEqual(rows.map((r) => r.option.id), ['s3-deep', 'gcs-archive', 'b2', 'icloud-2tb'])
  assert.deepEqual(rows.map((r) => r.cheapest), [false, true, false, false], 'archive storage needing a restore is not one Keevoro can use')
  assert.deepEqual(rows.map((r) => r.setUp?.kind ?? null), [null, 'gcs', 'endpoint', null])
  assert.deepEqual(rows.map((r) => r.notRefreshed), [false, true, false, true])
  assert.deepEqual(rows.map((r) => r.storage), ['240 GB', '240 GB', '240 GB', '2 TB'])
  // In whatever order the options came, they are shown cheapest first.
  const shuffled = rowsOf({ ...comparison, options: [...comparison.options].reverse() })
  assert.deepEqual(shuffled.map((r) => r.option.id), ['s3-deep', 'gcs-archive', 'b2', 'icloud-2tb'])
  assert.deepEqual(rowsOf({ ...comparison, options: [] }), [])
})

test('Stratus opens in the browser at its own page', () => {
  assert.equal(STRATUS_PAGE, 'https://stratus.adam-sparshott.workers.dev/')
})
