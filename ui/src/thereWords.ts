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
  if (waiting > 0) {
    const there = safe === 0 ? '' : sure ? ` ${n(safe)} already there.` : ` ${n(safe)} were there when Keevoro last looked${when}.`
    return `${n(waiting)} ${waiting === 1 ? 'file is' : 'files are'} waiting to be backed up.${there}`
  }
  if (safe === 0) return 'Nothing is being kept yet.'
  return sure
    ? `Everything you are keeping is there: ${n(safe)} files.`
    : `Everything you are keeping was there when Keevoro last looked${when}: ${n(safe)} files.`
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
