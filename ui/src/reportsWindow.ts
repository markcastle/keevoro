// Sending a report, and asking after reports, through the app's window
// when Keevoro itself cannot reach the reports service.
//
// A work computer may reach the internet only through a proxy set up by a
// script, or one that asks to be signed in to, which only the system's own
// network settings know about. The window goes through those, so when
// Keevoro says the service could not be reached at all, the window sends
// the very same text, and Keevoro keeps the answer (`noteReport`). It
// sends nothing of the window's own: no cookies, and no page it came from.

/** How long the window waits for the reports service, in milliseconds. */
const WAIT = 20_000

type Fetcher = (url: string, init?: RequestInit) => Promise<Response>

/** A report that did not go: whether the service could not be reached at all, and why in plain words if known. */
export class ReportNotSent extends Error {
  readonly unreachable: boolean
  /** Why it could not be reached, in plain words, when that is known. */
  readonly why?: string
  constructor(message: string, unreachable: boolean, why?: string) {
    super(message)
    this.unreachable = unreachable
    this.why = why
  }
}

/** What the app said when a report did not go, as an error with its parts. */
export function notSent(said: unknown): ReportNotSent {
  if (said && typeof said === 'object' && 'message' in said) {
    const { kind, message, cause } = said as { kind?: string; message: unknown; cause?: unknown }
    return new ReportNotSent(String(message), kind === 'unreachable', typeof cause === 'string' ? cause : undefined)
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
): Promise<{ reference: string; token: string } | { why: string } | null> {
  const response = await ask(
    fetcher,
    `${sendTo}/v1/reports`,
    { method: 'POST', headers: { 'content-type': 'application/json' }, body },
    wait,
  )
  if (!response) return null
  const answer = await answerOf(response)
  if (response.ok && answer.ok === true && typeof answer.reference === 'string' && typeof answer.token === 'string') {
    return { reference: answer.reference, token: answer.token }
  }
  const why = typeof answer.why === 'string' ? answer.why.replace(/\p{Cc}/gu, '').trim().slice(0, 300) : ''
  return { why: why || `Keevoro’s reports service didn’t answer as expected (it said ${response.status}). Try again later.` }
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
