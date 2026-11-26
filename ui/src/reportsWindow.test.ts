import assert from 'node:assert/strict'
import { test } from 'node:test'
import { askThroughWindow, checkedCode, notSent, ReportNotSent, sendThroughWindow } from './reportsWindow.ts'

const TOKEN = '0123456789abcdef0123456789abcdef01234567'
type Asked = { url: string; init: RequestInit }

/** A stand-in for the window's fetch, answering each request in turn, and noting what it was asked. */
const service = (answers: (() => Response)[]) => {
  const asked: Asked[] = []
  const fetcher = async (url: string, init: RequestInit = {}) => {
    asked.push({ url, init })
    const next = answers.shift()
    if (!next) throw new TypeError('Failed to fetch')
    return next()
  }
  return { asked, fetcher }
}
const json = (status: number, body: unknown) => () => new Response(JSON.stringify(body), { status, headers: { 'content-type': 'application/json' } })
const offline = () => {
  throw new TypeError('Failed to fetch')
}

test('a report goes through the window exactly as made, with nothing of the window’s own', async () => {
  const { asked, fetcher } = service([json(201, { ok: true, reference: 'K-1042', token: TOKEN })])
  const body = '{"kind":"idea","message":"Hello"}'
  assert.deepEqual(await sendThroughWindow('https://reports.example.com', body, fetcher), { reference: 'K-1042', token: TOKEN })
  assert.equal(asked[0].url, 'https://reports.example.com/v1/reports')
  assert.equal(asked[0].init.method, 'POST')
  assert.equal(asked[0].init.body, body)
  assert.equal(asked[0].init.credentials, 'omit')
  assert.equal(asked[0].init.referrerPolicy, 'no-referrer')
  assert.deepEqual(asked[0].init.headers, { 'content-type': 'application/json' })
  assert.ok(asked[0].init.signal, 'with a wait that ends')
})

test('a refusal is the service’s own words; not reached at all is null', async () => {
  const why = 'That’s a lot of reports from one computer today. Please try again tomorrow.'
  assert.deepEqual(await sendThroughWindow('https://r', '{}', service([json(429, { ok: false, why })]).fetcher), { why })
  assert.deepEqual(await sendThroughWindow('https://r', '{}', service([json(502, 'not json')]).fetcher), {
    why: 'Keevoro’s reports service didn’t answer as expected (it said 502). Try again later.',
  })
  assert.equal(await sendThroughWindow('https://r', '{}', service([offline]).fetcher), null)
})

test('a wait that runs out counts as not reached', async () => {
  const hangs = (_url: string, init: RequestInit = {}) =>
    new Promise<Response>((_, fail) => init.signal?.addEventListener('abort', () => fail(new DOMException('aborted', 'AbortError'))))
  assert.equal(await sendThroughWindow('https://r', '{}', hangs, 1), null)
})

test('reports are asked after until the service cannot be reached, and say whether all were', async () => {
  const { asked, fetcher } = service([json(200, { ok: true, reference: 'K-2', status: 'shipped' }), json(404, { ok: false })])
  const reports = [
    { reference: 'K-2', token: TOKEN },
    { reference: 'K-1', token: TOKEN },
  ]
  assert.deepEqual(await askThroughWindow('https://r', reports, fetcher), {
    heard: [{ reference: 'K-2', answer: { ok: true, reference: 'K-2', status: 'shipped' } }],
    all: true,
  })
  assert.equal(asked[0].url, `https://r/v1/reports/K-2?token=${TOKEN}`)
  assert.equal(asked[0].init.credentials, 'omit')
  const cut = service([json(200, { ok: true, reference: 'K-2' })])
  assert.deepEqual(await askThroughWindow('https://r', reports, cut.fetcher), {
    heard: [{ reference: 'K-2', answer: { ok: true, reference: 'K-2' } }],
    all: false,
  })
})

test('what the app says when a report did not go is read into a plain error', () => {
  const unreached = notSent({ kind: 'unreachable', message: 'Couldn’t reach it.', cause: 'The network said no.' })
  assert.ok(unreached instanceof ReportNotSent)
  assert.deepEqual([unreached.message, unreached.unreachable, unreached.why], ['Couldn’t reach it.', true, 'The network said no.'])
  const refused = notSent({ kind: 'refused', message: 'Too many.' })
  assert.deepEqual([refused.message, refused.unreachable, refused.why], ['Too many.', false, undefined])
  assert.equal(notSent('Plain words').message, 'Plain words')
})

test('whether the service recognised a reporting code is read back, and said only when it said', async () => {
  const yes = service([json(201, { ok: true, reference: 'K-7', token: TOKEN, recognised: true })])
  assert.deepEqual(await sendThroughWindow('https://r', '{}', yes.fetcher), { reference: 'K-7', token: TOKEN, recognised: true })
  const no = service([json(201, { ok: true, reference: 'K-8', token: TOKEN, recognised: false })])
  assert.deepEqual(await sendThroughWindow('https://r', '{}', no.fetcher), { reference: 'K-8', token: TOKEN, recognised: false })
  const odd = service([json(201, { ok: true, reference: 'K-9', token: TOKEN, recognised: 'yes' })])
  assert.deepEqual(await sendThroughWindow('https://r', '{}', odd.fetcher), { reference: 'K-9', token: TOKEN })
})

test('the report with its reporting code, for the window to send, is never in what is printed of the error', () => {
  const unreached = notSent({ kind: 'unreachable', message: 'Couldn’t reach it.', sendAs: '{"code": "TESTER-1234"}' })
  assert.equal(unreached.sendAs, '{"code": "TESTER-1234"}')
  assert.ok(!Object.keys(unreached).includes('sendAs'))
  assert.doesNotMatch(JSON.stringify(unreached), /TESTER/)
  assert.equal(notSent({ kind: 'unreachable', message: 'x' }).sendAs, undefined)
})

test('a reporting code is letters, digits and hyphens, trimmed, at most 64, and refused in plain words otherwise', () => {
  assert.deepEqual(checkedCode('  TESTER-1234 '), { code: 'TESTER-1234' })
  assert.deepEqual(checkedCode(' '), { why: 'Type the reporting code first.' })
  for (const bad of ['TESTER 1234', 'tester_1', 'café', 'a;b']) {
    assert.deepEqual(checkedCode(bad), { why: 'A reporting code has only letters, digits and hyphens in it. Check it and try again.' }, bad)
  }
  assert.deepEqual(checkedCode('a'.repeat(64)), { code: 'a'.repeat(64) })
  assert.deepEqual(checkedCode('a'.repeat(65)), { why: 'A reporting code is at most 64 letters, digits and hyphens. That one is longer.' })
})
