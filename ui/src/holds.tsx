// What the open backup is for, so every screen's words can follow it:
// "Find your music", "1,204 tracks found so far". Set by the app as a
// backup opens and whenever what it is for changes; photos and videos
// until then, as every backup is unless the person says otherwise.

import { createContext, useContext } from 'react'
import type { WantsWord } from './backend'
import { files, foundOrNone, musicOnly } from './format'

export const HoldsContext = createContext<WantsWord[]>(['photos', 'videos'])

/** What the open backup is for. */
export function useHolds(): WantsWord[] {
  return useContext(HoldsContext)
}

/**
 * Counts found, said for the backup: a backup of music alone says its
 * tracks, and nothing of the photos and videos a search also passes; any
 * other backup says what it always did, and its tracks when it has some.
 */
export function foundFor(holds: readonly string[], photos: number, videos: number, others = 0, music = 0) {
  return musicOnly(holds) ? files(0, 0, others, music, holds) : files(photos, videos, others, music, holds)
}

/**
 * As `foundFor`, for a sentence about a search that has ended: when nothing
 * at all was found it names everything the backup is for, "no photos,
 * videos or music", never only the first of them.
 */
export function foundOrNoneFor(holds: readonly string[], photos: number, videos: number, others = 0, music = 0) {
  return musicOnly(holds) ? foundOrNone(holds, 0, 0, others, music) : foundOrNone(holds, photos, videos, others, music)
}
