// Words and numbers as a person would say them. Everything the app shows
// goes through here, so "1 photo" and "1,204 photos" are right everywhere.

export const thousands = (n: number) => n.toLocaleString('en-GB')

export function count(n: number, word: string) {
  return `${thousands(n)} ${word}${n === 1 ? '' : 's'}`
}

/**
 * "238 photos, 11 videos and 40 other files", leaving out whichever there
 * are none of, and "0 photos" when there is nothing at all. With no music
 * or other files, "12,431 photos and 312 videos" as ever; music is counted
 * in tracks: "238 photos and 1,204 tracks". The same rule as
 * `keevoro_core::words::Tally::said`, and both are tested against one table
 * of cases, `countWords.json`, so the screens and the engine never disagree.
 * `holds` says what nothing at all is counted in: "0 tracks" for music.
 */
export function files(photos: number, videos: number, others = 0, music = 0, holds?: readonly string[]) {
  const parts = []
  if (photos > 0) parts.push(count(photos, 'photo'))
  if (videos > 0) parts.push(count(videos, 'video'))
  if (music > 0) parts.push(count(music, 'track'))
  if (others > 0) parts.push(count(others, 'other file'))
  if (parts.length === 0) parts.push(count(0, firstWord(holds)))
  return joined(parts)
}

function joined(parts: string[]) {
  return parts.length > 1 ? `${parts.slice(0, -1).join(', ')} and ${parts[parts.length - 1]}` : parts.join('')
}

/** The word nothing at all is counted in, for what a backup is for. */
function firstWord(holds?: readonly string[]) {
  if (!holds) return 'photo'
  if (!holds.includes('photos') && !holds.includes('videos') && holds.includes('music')) return 'track'
  if (!holds.includes('photos') && holds.includes('videos')) return 'video'
  return 'photo'
}

/**
 * What a backup for `holds` holds, in a few words: "photos", "photos and
 * videos", "music", "photos and music", "photos, videos and music". Other
 * files are a person's own addition, said apart. The same as
 * `keevoro_core::words::named`, tested against `holdsWords.json`.
 */
export function named(holds: readonly string[]) {
  const words = (['photos', 'videos', 'music'] as const).filter((w) => holds.includes(w))
  return words.length ? joined([...words]) : 'files'
}

/** As `named`, with other files after: "photos, videos and other files". */
export function namedWithOthers(holds: readonly string[]) {
  const words = (['photos', 'videos', 'music'] as const).filter((w) => holds.includes(w))
  return joined([...words, 'other files'])
}

/**
 * The things a step's title names, as short as reads naturally: "photos"
 * for photos and videos, as Keevoro always said, "music", "photos and
 * music". `keevoro_core::words::heading`, tested against `holdsWords.json`.
 */
export function heading(holds: readonly string[]) {
  const pictures = holds.includes('photos') ? 'photos' : holds.includes('videos') ? 'videos' : null
  const words = [pictures, holds.includes('music') ? 'music' : null].filter((w): w is string => !!w)
  return words.length ? words.join(' and ') : 'files'
}

/** "Photos, Videos and Music": what a backup is for, as the line on the first step says it. */
export function holdsLine(holds: readonly string[]) {
  const words = (
    [
      ['photos', 'Photos'],
      ['videos', 'Videos'],
      ['music', 'Music'],
    ] as const
  )
    .filter(([w]) => holds.includes(w))
    .map(([, label]) => label)
  return words.join(', ')
}

/** True for a backup of music alone. */
export const musicOnly = (holds: readonly string[]) =>
  holds.includes('music') && !holds.includes('photos') && !holds.includes('videos')

/**
 * Is this a whole drive, or a folder so near the top that it holds most of
 * one (everybody's files, or one person's)? Looking through all of that can
 * take a long while, so the app asks first.
 */
export function wholePlace(path: string) {
  const p = path.replace(/^\\\\\?\\/, '').replace(/[\\/]+$/, '')
  return (
    p === '' || // "/"
    /^[A-Za-z]:$/.test(p) || // "K:"
    /^\\\\[^\\]+\\[^\\]+$/.test(p) || // a whole network share
    /^\/Volumes\/[^/]+$/.test(p) ||
    /^([A-Za-z]:\\Users|\/Users|\/home)([\\/][^\\/]+)?$/.test(p)
  )
}

/**
 * A length of time as a person would say it: "less than a minute",
 * "about 36 minutes", "about 2 hours 10 minutes". Never to the second,
 * because a guess that precise would be pretending.
 */
export function about(seconds: number) {
  if (seconds < 60) return 'less than a minute'
  const minutes = Math.round(seconds / 60)
  if (minutes < 60) return `about ${minutes} ${minutes === 1 ? 'minute' : 'minutes'}`
  // Past an hour, the nearest five minutes is as fine as a guess should be.
  const rounded = Math.round(minutes / 5) * 5
  const hours = Math.floor(rounded / 60)
  const rest = rounded % 60
  const h = `${hours} ${hours === 1 ? 'hour' : 'hours'}`
  return rest === 0 ? `about ${h}` : `about ${h} ${rest} minutes`
}

export function size(bytes: number) {
  const units = ['bytes', 'KB', 'MB', 'GB', 'TB']
  let value = bytes
  let unit = 0
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024
    unit += 1
  }
  return unit === 0 ? `${bytes} bytes` : `${value.toFixed(1)} ${units[unit]}`
}

/** How long a video lasts, as a video player shows it: "0:07", "1:42", "1:02:05". */
export function length(seconds: number) {
  const all = Math.max(0, Math.round(seconds))
  const [h, m, s] = [Math.floor(all / 3600), Math.floor(all / 60) % 60, all % 60]
  const two = (n: number) => String(n).padStart(2, '0')
  return h > 0 ? `${h}:${two(m)}:${two(s)}` : `${m}:${two(s)}`
}

/** How long ago a moment was, in seconds since 1970: "just now", "5 minutes ago", "3 days ago". */
export function ago(then: number, now = Date.now() / 1000) {
  const gone = Math.max(0, now - then)
  if (gone < 60) return 'just now'
  if (gone < 3_600) return `${count(Math.floor(gone / 60), 'minute')} ago`
  if (gone < 86_400) return `${count(Math.floor(gone / 3_600), 'hour')} ago`
  if (gone < 2_592_000) return `${count(Math.floor(gone / 86_400), 'day')} ago`
  return `on ${new Date(then * 1000).toLocaleDateString('en-GB', { day: 'numeric', month: 'long', year: 'numeric' })}`
}

/** A part of a whole as a person reads it: "40%", or "<1%" for a sliver. */
export function share(part: number, whole: number) {
  if (whole <= 0) return '0%'
  const percent = (part * 100) / whole
  return part > 0 && percent < 1 ? '<1%' : `${Math.round(percent)}%`
}
