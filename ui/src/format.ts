// Words and numbers as a person would say them. Everything the app shows
// goes through here, so "1 photo" and "1,204 photos" are right everywhere.

export const thousands = (n: number) => n.toLocaleString('en-GB')

export function count(n: number, word: string) {
  return `${thousands(n)} ${word}${n === 1 ? '' : 's'}`
}

/** "12,431 photos and 312 videos", leaving out whichever there are none of. */
export function photosAndVideos(photos: number, videos: number) {
  const parts = []
  if (photos > 0 || videos === 0) parts.push(count(photos, 'photo'))
  if (videos > 0) parts.push(count(videos, 'video'))
  return parts.join(' and ')
}

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
