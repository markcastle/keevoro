// What a report sent from this computer was, in a few plain sentences, for
// when it is opened under Your reports: what kind it was and when it went,
// the Keevoro and the screen it went from, and whether the log and a way
// to reach the person went with it. Never the log or the contact itself:
// Keevoro does not keep either.

import type { ReportListed } from './backend'

/** The day a report went, as a person writes it: "14 November 2026". */
export function sentOn(sentAt: number) {
  return new Date(sentAt * 1000).toLocaleDateString('en-GB', { day: 'numeric', month: 'long', year: 'numeric' })
}

/** The sentences said under a report's words when it is opened. */
export function reportFacts(r: Pick<ReportListed, 'kind' | 'sentAt' | 'appVersion' | 'screen' | 'withLog' | 'withContact'>): string[] {
  const facts = [`${r.kind === 'idea' ? 'An idea' : 'A problem'}, sent on ${sentOn(r.sentAt)}.`]
  if (r.appVersion) facts.push(`Sent from Keevoro ${r.appVersion}${r.screen ? ` on the ${r.screen} screen` : ''}.`)
  const went = [r.withLog && 'with your log', r.withContact && 'with your contact details'].filter(Boolean)
  if (went.length > 0) facts.push(`Sent ${went.join(' and ')}.`)
  return facts
}
