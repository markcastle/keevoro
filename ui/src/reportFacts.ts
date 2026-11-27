// What a report sent from this computer was, in a few plain sentences, for
// when it is opened under Your reports: what kind it was and when it went,
// the Keevoro and the screen it went from, and whether the log and a way
// to reach the person went with it, and a picture. Never the log, the
// contact or the picture itself: Keevoro keeps none of them.

import type { ReportListed } from './backend'

/** The day a report went, as a person writes it: "14 November 2026". */
export function sentOn(sentAt: number) {
  return new Date(sentAt * 1000).toLocaleDateString('en-GB', { day: 'numeric', month: 'long', year: 'numeric' })
}

/** The sentences said under a report's words when it is opened. */
export function reportFacts(
  r: Pick<ReportListed, 'kind' | 'sentAt' | 'appVersion' | 'screen' | 'withLog' | 'withContact' | 'withPicture'>,
): string[] {
  const facts = [`${r.kind === 'idea' ? 'An idea' : 'A problem'}, sent on ${sentOn(r.sentAt)}.`]
  if (r.appVersion) facts.push(`Sent from Keevoro ${r.appVersion}${r.screen ? ` on the ${r.screen} screen` : ''}.`)
  const went = [r.withLog && 'with your log', r.withContact && 'with your contact details', r.withPicture && 'with a picture'].filter(
    (w): w is string => Boolean(w),
  )
  if (went.length > 0) facts.push(`Sent ${went.length > 1 ? `${went.slice(0, -1).join(', ')} and ${went[went.length - 1]}` : went[0]}.`)
  return facts
}
