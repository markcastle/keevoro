// The rule for the tick boxes of what a backup or a source is for, pure so
// it is tested with Node alone (wantsRule.test.ts).

import type { Wants, WantsWord } from './backend'

const WORDS: [WantsWord, string][] = [
  ['photos', 'Photos'],
  ['videos', 'Videos'],
  ['music', 'Music'],
  ['others', 'Other files'],
]

/**
 * The words shown as switches: Photos, Videos and Music always, three equal
 * choices; Other files only once it can mean something.
 */
export function shownWords(offersOthers: boolean): [WantsWord, string][] {
  return WORDS.filter(([word]) => word !== 'others' || offersOthers)
}

/** What a new backup is for until the person says: photos and videos, no music. */
export const NEW_BACKUP_WANTS: WantsWord[] = ['photos', 'videos']

/**
 * The same rule for the question asked when a backup is made, where only
 * the three are asked: the last one on stays on (`null`).
 */
export function toggledNew(wants: WantsWord[], word: WantsWord, on: boolean): WantsWord[] | null {
  return toggled(wants, word, on, false)
}

/**
 * `wants` with `word` ticked or not. Unticking the last word shown is
 * refused (`null`): a backup is always for something. Words not shown keep
 * what they were.
 */
export function toggled(wants: Wants, word: WantsWord, on: boolean, offersOthers: boolean): Wants | null {
  const next = WORDS.map(([w]) => w).filter((w) => (w === word ? on : wants.includes(w)))
  const shown = shownWords(offersOthers).map(([w]) => w)
  return next.some((w) => shown.includes(w)) ? next : null
}

/** Whether two choices of what to back up are the same, in whatever order. */
export function sameWants(a: Wants, b: Wants): boolean {
  return a.length === b.length && a.every((w) => b.includes(w))
}

/**
 * What to store when a person sets a source's switches to `next`. While
 * they match the rest of the backup the source has no choice of its own
 * (`'backup'`), so a later change to the whole backup still reaches it;
 * only a difference is kept as its own.
 */
export function sourceWantsChoice(next: Wants, backup: Wants): Wants | 'backup' {
  return sameWants(next, backup) ? 'backup' : next
}

/** The same rule for how a source's files are arranged. */
export function sourceArrangeChoice<A extends string>(next: A, backup: A): A | 'backup' {
  return next === backup ? 'backup' : next
}
