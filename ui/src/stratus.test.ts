import assert from 'node:assert/strict'
import { test } from 'node:test'
import { STRATUS_ORIGIN, currencyFor, gigabytesToAsk, readMessage, safeHttps, setMessage, setUpFor } from './stratus.ts'

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

test('only the size and a currency are sent, as object storage', () => {
  const sent = setMessage(123.4e9, 'en-GB')
  assert.deepEqual(sent, { stratus: 'set', need: 130, currency: 'GBP', kind: 'object' })
  assert.deepEqual(Object.keys(sent).sort(), ['currency', 'kind', 'need', 'stratus'])
})

const option = {
  id: 'r2-standard',
  provider: 'cloudflare-r2',
  providerName: 'Cloudflare R2',
  name: 'R2 Standard',
  kind: 'object',
  monthly: 1.23,
  url: 'https://developers.cloudflare.com/r2/pricing/',
  badge: { text: 'R2', color: '#f38020' },
}

test('a message is believed only from Stratus itself', () => {
  assert.deepEqual(readMessage(STRATUS_ORIGIN, { stratus: 'ready', version: '1.0' }), { type: 'ready' })
  for (const origin of ['https://stratus.adam-sparshott.workers.dev.example.com', 'http://stratus.adam-sparshott.workers.dev', 'null', '', 'https://example.com']) {
    assert.equal(readMessage(origin, { stratus: 'ready' }), null, origin)
  }
  for (const nonsense of [null, 'ready', 7, [], { stratus: 'quote' }, { stratus: 'select' }, { stratus: 'select', option: 'x' }]) {
    assert.equal(readMessage(STRATUS_ORIGIN, nonsense), null, JSON.stringify(nonsense))
  }
})

test('a choice keeps only the fields Keevoro uses, checked', () => {
  assert.deepEqual(readMessage(STRATUS_ORIGIN, { stratus: 'select', option }), {
    type: 'select',
    choice: {
      provider: 'cloudflare-r2',
      providerName: 'Cloudflare R2',
      plan: 'R2 Standard',
      url: 'https://developers.cloudflare.com/r2/pricing/',
    },
  })
  // Markup is kept as the text it is, never made into markup; control
  // characters and the provider's own odd name are not let through.
  const odd = readMessage(STRATUS_ORIGIN, {
    stratus: 'select',
    option: { ...option, providerName: '<img src=x onerror=alert(1)>\u0007', url: 'javascript:alert(1)' },
  })
  assert.equal(odd?.type, 'select')
  if (odd?.type === 'select') {
    assert.equal(odd.choice.providerName, '<img src=x onerror=alert(1)>')
    assert.equal(odd.choice.url, undefined, 'only an https address is kept')
  }
  assert.equal(readMessage(STRATUS_ORIGIN, { stratus: 'select', option: { ...option, provider: 'Cloudflare R2!' } }), null)
  assert.equal(readMessage(STRATUS_ORIGIN, { stratus: 'select', option: { ...option, providerName: 'x'.repeat(500) } }), null)
  assert.equal(readMessage(STRATUS_ORIGIN, { stratus: 'select', option: { ...option, providerName: 42 } }), null)
})

test('only a plain https address is opened or filled in', () => {
  assert.equal(safeHttps('https://wasabi.com/pricing'), 'https://wasabi.com/pricing')
  for (const unsafe of ['http://wasabi.com', 'javascript:alert(1)', 'file:///C:/Windows', 'https://user:pass@example.com', 'https://localhost', 'not a url', 42, null]) {
    assert.equal(safeHttps(unsafe), undefined, String(unsafe))
  }
})

test('a choice Keevoro can back up to opens the right form', () => {
  const choice = (provider: string, endpoint?: string) => ({ provider, providerName: provider, plan: provider, ...(endpoint ? { endpoint } : {}) })
  assert.deepEqual(setUpFor(choice('cloudflare-r2')), { kind: 'r2', place: '' })
  assert.deepEqual(setUpFor(choice('aws-s3')), { kind: 's3', place: '' })
  assert.deepEqual(setUpFor(choice('gcs')), { kind: 'gcs', place: '' })
  assert.deepEqual(setUpFor(choice('backblaze-b2')), { kind: 'endpoint', place: '' }, 'no address is made up')
  assert.deepEqual(setUpFor(choice('wasabi', 'https://s3.eu-central-2.wasabisys.com/')), {
    kind: 'endpoint',
    place: 'https://s3.eu-central-2.wasabisys.com/',
  })
  assert.deepEqual(setUpFor(choice('some-new-store', 'https://s3.example.net/')), { kind: 'endpoint', place: 'https://s3.example.net/' })
  assert.equal(setUpFor(choice('apple')), null, 'a subscription is not storage Keevoro can reach directly')
  assert.equal(setUpFor(choice('azure')), null, 'Azure does not speak S3')
})
