// Where everything in Settings lives: its sections, the pages that open
// from them, and every row, by name and by the other words a person might
// look for it under. One list, so the side list, the search, the links from
// elsewhere in the app and the icon by the clock all agree on where a
// setting is. Pure, with no imports, so it is tested on its own.

/** The sections down the side of Settings, everyday first. */
export type SectionId = 'backup' | 'byItself' | 'appearance' | 'updates' | 'advanced' | 'help'

/** The pages that open from a row, each with a way back. */
export type DetailId = 'kinds' | 'agents' | 'log' | 'licences'

export type Section = {
  id: SectionId
  title: string
  /** Set apart from the everyday sections above it. */
  apart?: boolean
}

export const SECTIONS: Section[] = [
  { id: 'backup', title: 'This backup' },
  { id: 'byItself', title: 'Backing up by itself' },
  { id: 'appearance', title: 'Appearance' },
  { id: 'updates', title: 'Updates' },
  { id: 'advanced', title: 'Advanced', apart: true },
  { id: 'help', title: 'Help', apart: true },
]

export const DETAILS: Record<DetailId, { section: SectionId; title: string }> = {
  kinds: { section: 'advanced', title: 'Kinds of file Keevoro looks for' },
  agents: { section: 'advanced', title: 'AI agents' },
  log: { section: 'help', title: 'What Keevoro has been doing' },
  licences: { section: 'help', title: 'Open source licences' },
}

export type RowInfo = {
  id: string
  section: SectionId
  /** The page the row is on, when it is not on the section itself. */
  detail?: DetailId
  title: string
  /** Other words a person might search for it by. */
  words: string
}

/** Every row in Settings, in the order a section shows them. */
export const ROWS: RowInfo[] = [
  { id: 'name', section: 'backup', title: 'Name', words: 'backup name called' },
  { id: 'notes', section: 'backup', title: 'Where its notes are kept', words: 'library data folder catalog previews stored location' },
  { id: 'destination', section: 'backup', title: 'Where it backs up to', words: 'destination folder drive cloud bucket sending send' },
  { id: 'backups', section: 'backup', title: 'Manage backups', words: 'rename remove delete open put back removed backups' },
  { id: 'wants', section: 'backup', title: 'What it backs up', words: 'photos videos other files only for kinds' },
  { id: 'previews', section: 'backup', title: 'Previews for a gallery', words: 'thumbnails renditions gallery originals only quicker' },
  { id: 'arrange', section: 'backup', title: 'How files are arranged', words: 'by date keep my folders organise organize structure layout' },
  { id: 'schedule', section: 'byItself', title: 'Back up by itself', words: 'schedule automatic automatically timer daily weekly monthly every day night' },
  { id: 'notify', section: 'byItself', title: 'Tell me when a backup by itself finishes', words: 'notifications notices alerts messages tell' },
  { id: 'keepRunning', section: 'byItself', title: 'Keep Keevoro running by the clock', words: 'close closing window background tray menu bar icon quit' },
  { id: 'minimise', section: 'byItself', title: 'Minimise to the icon by the clock', words: 'minimize tray taskbar hide window' },
  { id: 'startWithComputer', section: 'byItself', title: 'Start Keevoro when this computer starts', words: 'startup login log in boot open automatically' },
  { id: 'theme', section: 'appearance', title: 'Light or dark', words: 'appearance theme dark mode light mode colour color match my computer' },
  { id: 'version', section: 'updates', title: 'Check for updates', words: 'update upgrade new version newest' },
  { id: 'kinds', section: 'advanced', title: 'Kinds of file', words: 'file types endings extensions formats raw music documents other files look for' },
  { id: 'agents', section: 'advanced', title: 'AI agents', words: 'ai agent claude mcp assistant chatbot' },
  { id: 'agentChanges', section: 'advanced', detail: 'agents', title: 'Let AI agents make changes', words: 'ai agent allow changes claude mcp' },
  { id: 'agentLookIn', section: 'advanced', detail: 'agents', title: 'Folders an agent may look through', words: 'ai agent allow folders claude mcp' },
  { id: 'agentBackUpTo', section: 'advanced', detail: 'agents', title: 'Folders an agent may back up to', words: 'ai agent allow folders claude mcp' },
  { id: 'codecs', section: 'advanced', title: 'Use this computer’s own picture codecs', words: 'codecs decoder heic graphics hardware speed fast previews wrong' },
  { id: 'unfinished', section: 'advanced', title: 'Unfinished copies in a folder', words: 'clear tidy clean working files temporary leftover stopped' },
  { id: 'maps', section: 'advanced', title: 'Remembered maps', words: 'forget maps survey privacy folder names space' },
  { id: 'log', section: 'help', title: 'What Keevoro has been doing', words: 'log record history diagnostics problem wrong slow' },
  { id: 'licences', section: 'help', title: 'Open source licences', words: 'licenses licence credits open source third party' },
  { id: 'reset', section: 'help', title: 'Reset Keevoro', words: 'reset start again erase factory remove everything clear' },
]

export function rowInfo(id: string): RowInfo {
  const found = ROWS.find((r) => r.id === id)
  if (!found) throw new Error(`No row ${id} in Settings`)
  return found
}

export function sectionTitle(id: SectionId): string {
  return SECTIONS.find((s) => s.id === id)?.title ?? ''
}

/** Where Settings is open: a section (none yet, in a narrow window, is the list itself), a page in it, and a row to bring into view. */
export type SettingsPlace = { section: SectionId | null; detail?: DetailId; row?: string }

/** The place a row is in, with the row to bring into view. */
export function placeOfRow(id: string): SettingsPlace {
  const row = rowInfo(id)
  return { section: row.section, detail: row.detail, row: row.id }
}

const isSection = (word: string): word is SectionId => SECTIONS.some((s) => s.id === word)
const isDetail = (word: string): word is DetailId => word in DETAILS

/**
 * A page name from the icon by the clock or a link, as a place:
 * "settings", "settings/byItself", "settings/advanced/kinds" (a page) or
 * "settings/byItself/keepRunning" (a row). Anything else is null; a part
 * not known is passed over, so an older or newer name still opens Settings.
 */
export function parsePlace(page: string): SettingsPlace | null {
  const [first, second, third] = page.split('/')
  if (first !== 'settings') return null
  if (third && isDetail(third) && DETAILS[third].section === second) return { section: second, detail: third }
  if (third && ROWS.some((r) => r.id === third && r.section === second)) return placeOfRow(third)
  if (second && isSection(second)) return { section: second }
  return { section: null }
}

/** Rows whose name or other words hold every word typed, best first: names that start with it, then names holding it, then other words. */
export function findRows(typed: string): RowInfo[] {
  const wanted = typed.trim().toLowerCase().split(/\s+/).filter(Boolean)
  if (wanted.length === 0) return []
  const scored = ROWS.map((row) => {
    const title = row.title.toLowerCase()
    const all = `${title} ${row.words} ${sectionTitle(row.section).toLowerCase()}`
    if (!wanted.every((w) => all.includes(w))) return null
    const score = title.startsWith(wanted[0]) ? 0 : wanted.every((w) => title.includes(w)) ? 1 : 2
    return { row, score }
  }).filter((s): s is { row: RowInfo; score: number } => s !== null)
  return scored.sort((a, b) => a.score - b.score).map((s) => s.row)
}
