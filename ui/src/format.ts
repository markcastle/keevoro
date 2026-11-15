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
