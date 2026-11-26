// Sending a report, and asking after reports, through the app's window
// when Keevoro itself cannot reach the reports service.
//
// A work computer may reach the internet only through a proxy set up by a
// script, or one that asks to be signed in to, which only the system's own
// network settings know about. The window goes through those, so when
// Keevoro says the service could not be reached at all, the window sends
// the very same text, and Keevoro keeps the answer (`noteReport`). It
// sends nothing of the window's own: no cookies, and no page it came from.
//
// A reporting code is kept by Keevoro in the keychain and never given to
// the window, except here: when Keevoro cannot reach the service and a code
// is to go, it hands over the report with the code in (`sendAs`), because
// the window must send exactly that. It passes through once, straight to
// `fetch`, and is kept out of everything that is rendered or logged.

/** How long the window waits for the reports service, in milliseconds. */
const WAIT = 20_000

type Fetcher = (url: string, init?: RequestInit) => Promise<Response>

/** A report that did not go: whether the service could not be reached at all, and why in plain words if known. */
export class ReportNotSent extends Error {
  readonly unreachable: boolean
  /** Why it could not be reached, in plain words, when that is known. */
  readonly why?: string
  /**
   * When a reporting code was to go: the report with it in, for the window
   * to send in place of the one shown. Not enumerable, so printing the
   * error, or putting it in state that is shown, never carries the code.
   */
  declare readonly sendAs?: string
  constructor(message: string, unreachable: boolean, why?: string, sendAs?: string) {
    super(message)
    this.unreachable = unreachable
    this.why = why
    Object.defineProperty(this, 'sendAs', { value: sendAs, enumerable: false })
  }
}

/** The most a reporting code may be, in letters. */
export const CODE_MOST = 64

/** A reporting code as typed, trimmed, or why it cannot be one, in Keevoro's own words. */
export function checkedCode(typed: string): { code: string } | { why: string } {
  const code = typed.trim()
  if (!code) return { why: 'Type the reporting code first.' }
  if (!/^[A-Za-z0-9-]+$/.test(code)) return { why: 'A reporting code has only letters, digits and hyphens in it. Check it and try again.' }
  if (code.length > CODE_MOST) return { why: `A reporting code is at most ${CODE_MOST} letters, digits and hyphens. That one is longer.` }
  return { code }
}

/** What the app said when a report did not go, as an error with its parts. */
export function notSent(said: unknown): ReportNotSent {
  if (said && typeof said === 'object' && 'message' in said) {
    const { kind, message, cause, sendAs } = said as { kind?: string; message: unknown; cause?: unknown; sendAs?: unknown }
    return new ReportNotSent(
      String(message),
      kind === 'unreachable',
      typeof cause === 'string' ? cause : undefined,
      typeof sendAs === 'string' ? sendAs : undefined,
    )
  }
  return new ReportNotSent(said instanceof Error ? said.message : String(said), false)
}

/** Asks the service through the window, giving up after `wait`; null when it could not be reached at all. */
async function ask(fetcher: Fetcher, url: string, init: RequestInit, wait: number): Promise<Response | null> {
  const stop = new AbortController()
  const timer = setTimeout(() => stop.abort(), wait)
  try {
    return await fetcher(url, { ...init, credentials: 'omit', referrerPolicy: 'no-referrer', signal: stop.signal })
  } catch {
    return null
  } finally {
    clearTimeout(timer)
  }
}

async function answerOf(response: Response): Promise<Record<string, unknown>> {
  try {
    const answer: unknown = await response.json()
    return answer && typeof answer === 'object' ? (answer as Record<string, unknown>) : {}
  } catch {
    return {}
  }
}

/**
 * Sends a report exactly as it was made: its reference and token, the
 * service's own words if it said no, or null when it could not be reached.
 */
export async function sendThroughWindow(
  sendTo: string,
  body: string,
  fetcher: Fetcher = fetch,
  wait = WAIT,
): Promise<{ reference: string; token: string; recognised?: boolean } | { why: string } | null> {
  const response = await ask(
    fetcher,
    `${sendTo}/v1/reports`,
    { method: 'POST', headers: { 'content-type': 'application/json' }, body },
    wait,
  )
  if (!response) return null
  const answer = await answerOf(response)
  if (response.ok && answer.ok === true && typeof answer.reference === 'string' && typeof answer.token === 'string') {
    // Whether a reporting code sent with it was recognised; an older
    // service says nothing, and so does Keevoro.
    const recognised = typeof answer.recognised === 'boolean' ? { recognised: answer.recognised } : {}
    return { reference: answer.reference, token: answer.token, ...recognised }
  }
  const why = typeof answer.why === 'string' ? answer.why.replace(/\p{Cc}/gu, '').trim().slice(0, 300) : ''
  return { why: why || `Keevoro’s reports service didn’t answer as expected (it said ${response.status}). Try again later.` }
}

/** Said when the window could not reach the service to send a picture. */
export const PICTURE_UNREACHED = 'This computer couldn’t reach Keevoro’s reports service to send it.'

/**
 * Sends a picture after the report `reference` the service has accepted,
 * with that report's token: whether it went, and if not why, in the
 * service's own words (empty when it gave none) or plainly when it could
 * not be reached. The picture goes as it is, with its kind.
 */
export async function sendPictureThroughWindow(
  sendTo: string,
  reference: string,
  token: string,
  picture: { bytes: Uint8Array; type: string },
  fetcher: Fetcher = fetch,
  wait = WAIT,
): Promise<{ sent: true } | { sent: false; why: string }> {
  const url = `${sendTo}/v1/reports/${encodeURIComponent(reference)}/picture?token=${encodeURIComponent(token)}`
  const response = await ask(
    fetcher,
    url,
    { method: 'POST', headers: { 'content-type': picture.type }, body: picture.bytes as BodyInit },
    wait,
  )
  if (!response) return { sent: false, why: PICTURE_UNREACHED }
  const answer = await answerOf(response)
  if (response.ok && answer.ok === true) return { sent: true }
  const why = typeof answer.why === 'string' ? answer.why.replace(/\p{Cc}/gu, '').trim().slice(0, 300) : ''
  return { sent: false, why }
}

/**
 * Asks after each report in turn, stopping at the first the service could
 * not be reached for: what was heard of each, and whether all were asked.
 */
export async function askThroughWindow(
  sendTo: string,
  reports: { reference: string; token: string }[],
  fetcher: Fetcher = fetch,
  wait = WAIT,
): Promise<{ heard: { reference: string; answer: Record<string, unknown> }[]; all: boolean }> {
  const heard: { reference: string; answer: Record<string, unknown> }[] = []
  for (const { reference, token } of reports) {
    const url = `${sendTo}/v1/reports/${encodeURIComponent(reference)}?token=${encodeURIComponent(token)}`
    const response = await ask(fetcher, url, { method: 'GET' }, wait)
    if (!response) return { heard, all: false }
    if (response.ok) heard.push({ reference, answer: await answerOf(response) })
  }
  return { heard, all: true }
}
