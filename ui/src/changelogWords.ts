// Keevoro's changelog, read from CHANGELOG.md as it is written, for the
// "What's new in each version" page under About. Only what a person reads
// is kept: each released version, its date, and its bullets under Added,
// Changed, Fixed and Removed. What has not been released yet is left out,
// as it is not in the Keevoro they are running. Pure, so it is tested with
// Node alone (changelogWords.test.ts).

/** A run of words, in bold or plain, or the name of a thing in code type. */
export type Said = { words: string; bold?: boolean; code?: boolean }

export type Version = {
  version: string
  /** The day it came out, as written: "2026-11-26". */
  date: string
  /** Its headings in the order written, each with its bullets. */
  groups: { heading: string; changes: Said[][] }[]
}

/** One line's words: **bold**, `code`, and a link said by its words alone. */
export function said(line: string): Said[] {
  const plain = line.replace(/\[([^\]]+)\]\([^)]+\)/g, '$1')
  const parts: Said[] = []
  for (const piece of plain.split(/(\*\*[^*]+\*\*|`[^`]+`)/)) {
    if (!piece) continue
    if (piece.startsWith('**')) parts.push({ words: piece.slice(2, -2), bold: true })
    else if (piece.startsWith('`')) parts.push({ words: piece.slice(1, -1), code: true })
    else parts.push({ words: piece })
  }
  return parts
}

/** Every released version in the changelog, newest first as it is written. */
export function versions(changelog: string): Version[] {
  const found: Version[] = []
  let now: Version | null = null
  let group: Version['groups'][number] | null = null
  for (const line of changelog.split(/\r?\n/)) {
    const version = /^## \[(\d+\.\d+\.\d+)\](?: - (\d{4}-\d{2}-\d{2}))?/.exec(line)
    if (version) {
      now = { version: version[1], date: version[2] ?? '', groups: [] }
      group = null
      found.push(now)
    } else if (line.startsWith('## ')) {
      // Unreleased, or anything else that is not a version.
      now = null
      group = null
    } else if (now && line.startsWith('### ')) {
      group = { heading: line.slice(4).trim(), changes: [] }
      now.groups.push(group)
    } else if (now && line.startsWith('- ')) {
      if (!group) {
        group = { heading: 'Changed', changes: [] }
        now.groups.push(group)
      }
      group.changes.push(said(line.slice(2).trim()))
    } else if (now && group && group.changes.length > 0 && /^\s+\S/.test(line)) {
      // A bullet carried onto a second line.
      group.changes[group.changes.length - 1].push(...said(` ${line.trim()}`))
    }
  }
  return found.filter((v) => v.groups.some((g) => g.changes.length > 0))
}

/** A version's day as a person reads it: "26 November 2026". */
export function dayOf(date: string): string {
  const [y, m, d] = date.split('-').map(Number)
  if (!y || !m || !d) return ''
  return new Date(Date.UTC(y, m - 1, d)).toLocaleDateString('en-GB', { timeZone: 'UTC', day: 'numeric', month: 'long', year: 'numeric' })
}
