// What the backup screen says about what is where. Keevoro looks at each
// destination before it counts anything as there (see
// keevoro_engine::confirm), and a look is trusted for a few minutes. So
// "is there" is said only when every place was looked at a moment ago and
// reached; otherwise the screen says when Keevoro last looked, never more.
// Pure, so it is tested with Node alone (thereWords.test.ts); how long ago
// is said by whatever `ago` it is given, `format.ts`'s on screen.

import type { Looked } from './backend'

type Place = { there: number; waiting: number; looked?: Looked }

/** Is what Keevoro counts as there certain now, at every place? */
export function certain(places: Place[]) {
  return places.length > 0 && places.every((p) => p.looked?.fresh === true)
}

/** The oldest moment any place was last reached, for saying when Keevoro last looked. */
function lastLooked(places: Place[]) {
  const times = places.map((p) => p.looked?.lastReached ?? null)
  return times.some((t) => t === null) ? null : Math.min(...(times as number[]))
}

/** The line under the heading: how much is waiting, and how much is there, as surely as Keevoro knows it. */
export function thereNow(waiting: number, safe: number, places: Place[], ago: (then: number) => string) {
  const n = (x: number) => x.toLocaleString('en-GB')
  const sure = certain(places)
  const then = lastLooked(places)
  const when = then === null ? '' : `, ${ago(then)}`
  // Files a look found missing or another size are never "waiting" as if
  // nothing were wrong: they are said first, as needing attention, with the
  // whole in one sum, so the line can never contradict the reasons beneath.
  const attention = Math.min(waiting, needingAttention(places))
  if (attention > 0) {
    const rest = waiting - attention
    const more = rest === 0 ? '' : ` ${n(rest)} more ${rest === 1 ? 'is' : 'are'} waiting to be backed up.`
    return `${n(safe)} of your ${n(safe + waiting)} files are safely there. ${n(attention)} ${attention === 1 ? 'needs' : 'need'} attention.${more}`
  }
  if (waiting > 0) {
    const there = safe === 0 ? '' : sure ? ` ${n(safe)} already there.` : ` ${n(safe)} were there when Keevoro last looked${when}.`
    return `${n(waiting)} ${waiting === 1 ? 'file is' : 'files are'} waiting to be backed up.${there}`
  }
  if (safe === 0) return 'Nothing is being kept yet.'
  return sure
    ? `Everything you are keeping is there: ${n(safe)} files.`
    : `Everything you are keeping was there when Keevoro last looked${when}: ${n(safe)} files.`
}

/** Files the last look at each place reached found missing or another size. */
export function needingAttention(places: Place[]) {
  return places.reduce((sum, p) => sum + (p.looked?.reached ? p.looked.gone + p.looked.notAsSent : 0), 0)
}

/** One place's line in the list of several, when it could not be reached; null when it could. */
export function outOfReachLine(place: Place) {
  if (!place.looked || place.looked.reached) return null
  return `Can’t be reached just now · ${place.there.toLocaleString('en-GB')} there when last looked`
}

/** Every sentence the last looks have to say, in order, for the top of the backup screen. */
export function lookedSaid(places: Place[]) {
  return places.flatMap((p) => p.looked?.said ?? [])
}

/** What the main button says it will do: after a backup that missed files, which files it will send. */
export function mainButtonWords(waiting: number, missed: number) {
  if (waiting === 0) return 'Back up again'
  if (missed > 0 && missed === waiting) {
    return missed === 1 ? 'Back up the 1 file that was missed' : `Back up the ${missed.toLocaleString('en-GB')} files that were missed`
  }
  return 'Back up now'
}
