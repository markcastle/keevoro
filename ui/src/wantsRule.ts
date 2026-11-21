// The rule for the tick boxes of what a backup or a source is for, pure so
// it is tested with Node alone (wantsRule.test.ts).

import type { Wants, WantsWord } from './backend'

const WORDS: [WantsWord, string][] = [
  ['photos', 'Photos'],
  ['videos', 'Videos'],
  ['others', 'Other files'],
]

/** The words shown as tick boxes: other files only once they can mean something. */
export function shownWords(offersOthers: boolean): [WantsWord, string][] {
  return WORDS.filter(([word]) => word !== 'others' || offersOthers)
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
