// Where everything in Settings and Help lives: the two places, their
// sections, the pages that open from them, and every row, by name and by
// the other words a person might look for it under. One list, so the side
// list, the search (which looks in both places), the links from elsewhere
// in the app and the icon by the clock all agree on where a thing is.
// Pure, with no imports, so it is tested on its own.

/** The two places the foot of the side band opens: what you set, and what helps. */
export type Where = 'settings' | 'help'

/** The sections down the side of Settings and of Help, everyday first. */
export type SectionId =
  | 'backup'
  | 'byItself'
  | 'appearance'
  | 'updates'
  | 'advanced'
  | 'howTo'
  | 'report'
  | 'compare'
  | 'log'
  | 'about'

/** The pages that open from a row, each with a way back. */
export type DetailId = 'kinds' | 'agents' | 'licences'

export type Section = {
  id: SectionId
  where: Where
  title: string
  /** Set apart from the everyday sections above it. */
  apart?: boolean
}

export const PLACE_TITLES: Record<Where, string> = { settings: 'Settings', help: 'Help' }

export const SECTIONS: Section[] = [
  { id: 'backup', where: 'settings', title: 'This backup' },
  { id: 'byItself', where: 'settings', title: 'Backing up by itself' },
  { id: 'appearance', where: 'settings', title: 'Appearance' },
  { id: 'updates', where: 'settings', title: 'Updates' },
  { id: 'advanced', where: 'settings', title: 'Advanced', apart: true },
  { id: 'howTo', where: 'help', title: 'How do I…' },
  { id: 'report', where: 'help', title: 'Report a problem or suggest an idea' },
  { id: 'compare', where: 'help', title: 'Compare cloud storage providers' },
  { id: 'log', where: 'help', title: 'What Keevoro has been doing' },
  { id: 'about', where: 'help', title: 'About Keevoro', apart: true },
]

export const DETAILS: Record<DetailId, { section: SectionId; title: string }> = {
  kinds: { section: 'advanced', title: 'Kinds of file Keevoro looks for' },
  agents: { section: 'advanced', title: 'AI agents' },
  licences: { section: 'about', title: 'Open source licences' },
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

/**
 * Every row in Settings and Help, in the order a section shows them. A
 * row with its section's own name (report, compare, log, about) stands
 * for the whole section.
 */
export const ROWS: RowInfo[] = [
  { id: 'name', section: 'backup', title: 'Name', words: 'backup name called' },
  { id: 'notes', section: 'backup', title: 'Where its notes are kept', words: 'library data folder catalog previews stored location not copied' },
  { id: 'destination', section: 'backup', title: 'Where it backs up to', words: 'destination folder drive cloud bucket sending send' },
  { id: 'backups', section: 'backup', title: 'Manage backups', words: 'rename remove delete open put back removed backups' },
  { id: 'wants', section: 'backup', title: 'What it backs up', words: 'photos videos other files only for kinds' },
  { id: 'previews', section: 'backup', title: 'Previews for a gallery', words: 'thumbnails renditions gallery originals only quicker small screen-sized still video' },
  { id: 'arrange', section: 'backup', title: 'How files are arranged', words: 'by date keep my folders organise organize structure layout year month scans from now on' },
  { id: 'schedule', section: 'byItself', title: 'Back up by itself', words: 'schedule automatic automatically timer daily weekly monthly every day night' },
  { id: 'notify', section: 'byItself', title: 'Tell me when a backup by itself finishes', words: 'notifications notices alerts messages tell' },
  { id: 'waitOnBattery', section: 'byItself', title: 'Wait while on battery', words: 'battery laptop charge charger charged half power unplugged plugged mains good manners' },
  { id: 'waitOnMetered', section: 'byItself', title: 'Wait on a metered connection', words: 'metered hotspot mobile data allowance phone network cloud' },
  { id: 'between', section: 'byItself', title: 'Only between these hours', words: 'hours night time window quiet when start between own time' },
  { id: 'keepRunning', section: 'byItself', title: 'Keep Keevoro running by the clock', words: 'close closing window background tray menu bar icon quit where on time' },
  { id: 'minimise', section: 'byItself', title: 'Minimise to the icon by the clock', words: 'minimize tray taskbar hide window' },
  { id: 'startWithComputer', section: 'byItself', title: 'Start Keevoro when this computer starts', words: 'startup login logon boot sign in open automatically quietly programs' },
  { id: 'theme', section: 'appearance', title: 'Light or dark', words: 'appearance theme dark mode light mode colour color match my computer' },
  { id: 'version', section: 'updates', title: 'Check for updates', words: 'update upgrade new version newest' },
  { id: 'kinds', section: 'advanced', title: 'Kinds of file', words: 'file types endings extensions formats raw music documents other files look for' },
  { id: 'agents', section: 'advanced', title: 'AI agents', words: 'ai agent claude mcp assistant chatbot' },
  { id: 'agentChanges', section: 'advanced', detail: 'agents', title: 'Let AI agents make changes', words: 'ai agent allow changes claude mcp look only delete' },
  { id: 'agentLookIn', section: 'advanced', detail: 'agents', title: 'Folders an agent may look through', words: 'ai agent allow folders claude mcp' },
  { id: 'agentBackUpTo', section: 'advanced', detail: 'agents', title: 'Folders an agent may back up to', words: 'ai agent allow folders claude mcp' },
  { id: 'codecs', section: 'advanced', title: 'Use this computer’s own picture codecs', words: 'codecs decoder heic graphics hardware speed fast previews wrong processor cores' },
  { id: 'unfinished', section: 'advanced', title: 'Unfinished copies in a folder', words: 'clear tidy clean working files temporary leftover stopped' },
  { id: 'maps', section: 'advanced', title: 'Remembered maps', words: 'forget maps survey privacy folder names sizes space' },
  { id: 'reset', section: 'advanced', title: 'Reset Keevoro', words: 'reset start again erase factory remove everything clear trial run passing on installed' },

  { id: 'howSource', section: 'howTo', title: 'Add another source', words: 'drive card usb stick folder more photos somewhere else look through' },
  { id: 'howSecondPlace', section: 'howTo', title: 'Back up to a second place', words: 'another destination two copies second drive cloud both' },
  { id: 'howByItself', section: 'howTo', title: 'Make it back up by itself', words: 'schedule automatic automatically every day night' },
  { id: 'howPluggedIn', section: 'howTo', title: 'Back up a card when it is plugged in', words: 'plug card camera memory drive arrives automatically' },
  { id: 'howCheck', section: 'howTo', title: 'Check a backup is all there', words: 'check arrived prove every copy verify safe missing damaged' },
  { id: 'howWindowClosed', section: 'howTo', title: 'Bring Keevoro back when its window is closed', words: 'closed window icon clock tray menu bar open again where gone' },
  { id: 'howLeaveOut', section: 'howTo', title: 'Leave a folder out', words: 'skip exclude ignore folder not back up' },
  { id: 'howKinds', section: 'howTo', title: 'Change which kinds of file are backed up', words: 'photos videos other files types documents music' },
  { id: 'howNewComputer', section: 'howTo', title: 'Move to a new computer', words: 'another computer move moving replace laptop' },

  { id: 'report', section: 'report', title: 'Report a problem or suggest an idea', words: 'feedback bug fault suggestion suggest idea problem wrong broken tell contact support send report' },
  { id: 'yourReports', section: 'report', title: 'Your reports', words: 'feedback reference status sent report problem idea' },
  { id: 'compare', section: 'compare', title: 'Compare cloud storage providers', words: 'prices cost cheapest buy cloud storage stratus provider' },
  { id: 'log', section: 'log', title: 'What Keevoro has been doing', words: 'log record history diagnostics problem wrong slow' },
  { id: 'about', section: 'about', title: 'About Keevoro', words: 'version name who made' },
  { id: 'aboutVersion', section: 'about', title: 'Version and updates', words: 'version updates check newer' },
  { id: 'promises', section: 'about', title: 'What Keevoro promises', words: 'promises safe never changes sources read only rules' },
  { id: 'privacy', section: 'about', title: 'What leaves this computer', words: 'privacy internet data sent account server telemetry' },
  { id: 'thanks', section: 'about', title: 'Thank you', words: 'credits thanks made by maker' },
  { id: 'licences', section: 'about', title: 'Open source licences', words: 'licenses licence credits open source third party' },
  { id: 'licence', section: 'about', title: 'Keevoro’s own licence', words: 'license terms copyright source code trademark logo' },
]

export function rowInfo(id: string): RowInfo {
  const found = ROWS.find((r) => r.id === id)
  if (!found) throw new Error(`No row ${id} in Settings or Help`)
  return found
}

export function sectionTitle(id: SectionId): string {
  return SECTIONS.find((s) => s.id === id)?.title ?? ''
}

/** The place a section is in. */
export function whereOf(id: SectionId): Where {
  return SECTIONS.find((s) => s.id === id)?.where ?? 'settings'
}

/** One place's sections, in order. */
export function sectionsOf(where: Where): Section[] {
  return SECTIONS.filter((s) => s.where === where)
}

/**
 * Where Settings or Help is open: a section (none chosen yet is the
 * first), a page in it, and a row to bring into view. In a narrow
 * window, where the list of sections is a page of its own, `list` (or no
 * section chosen yet) shows the list, the section kept so it is the one
 * marked there.
 */
export type Place = { where: Where; section: SectionId | null; detail?: DetailId; row?: string; list?: boolean }

/** The place a row is in, with the row to bring into view (none for a row that is a whole section). */
export function placeOfRow(id: string): Place {
  const row = rowInfo(id)
  const where = whereOf(row.section)
  if (row.id === row.section) return { where, section: row.section }
  return { where, section: row.section, detail: row.detail, row: row.id }
}

/** Where a row is, in words, beside it in what a search found: "Help, About Keevoro". */
export function whereSaid(row: RowInfo): string {
  const parts = [PLACE_TITLES[whereOf(row.section)]]
  if (row.id !== row.section) parts.push(sectionTitle(row.section))
  if (row.detail) parts.push(DETAILS[row.detail].title)
  return parts.join(', ')
}

const isSectionIn = (where: Where, word: string): word is SectionId =>
  SECTIONS.some((s) => s.id === word && s.where === where)
const isDetail = (word: string): word is DetailId => word in DETAILS

/**
 * A page name from the icon by the clock or a link, as a place:
 * "settings", "settings/byItself", "settings/advanced/kinds" (a page),
 * "settings/byItself/keepRunning" (a row), and the same for "help".
 * Anything else is null; a part not known is passed over, so an older or
 * newer name still opens Settings or Help. The names from before Help was
 * a place of its own ("settings/help/…", "about") lead to where each thing
 * lives now.
 */
export function parsePlace(page: string): Place | null {
  const [first, second, third] = page.split('/')
  if (first === 'about') return { where: 'help', section: 'about' }
  if (first === 'settings' && second === 'help') return movedFromHelp(third)
  if (first !== 'settings' && first !== 'help') return null
  const where: Where = first
  if (second && isSectionIn(where, second)) {
    if (third && isDetail(third) && DETAILS[third].section === second) return { where, section: second, detail: third }
    if (third && ROWS.some((r) => r.id === third && r.section === second)) return placeOfRow(third)
    return { where, section: second }
  }
  return { where, section: null }
}

/** Where a name from Settings' old Help section lives now. */
function movedFromHelp(name: string | undefined): Place {
  if (name && isDetail(name)) {
    const section = DETAILS[name].section
    return { where: whereOf(section), section, detail: name }
  }
  if (name && ROWS.some((r) => r.id === name)) return placeOfRow(name)
  return { where: 'help', section: null }
}

/**
 * Rows whose name or other words hold every word typed, best first: names
 * that start with it, then names holding it, then other words for it in
 * full ("log", not the end of "catalog"), then any of the rest.
 */
export function findRows(typed: string): RowInfo[] {
  const wanted = typed.trim().toLowerCase().split(/\s+/).filter(Boolean)
  if (wanted.length === 0) return []
  const scored = ROWS.map((row) => {
    const title = row.title.toLowerCase()
    const all = `${title} ${row.words} ${sectionTitle(row.section).toLowerCase()}`
    if (!wanted.every((w) => all.includes(w))) return null
    const whole = `${title} ${row.words}`.split(/\s+/)
    const score =
      title.startsWith(wanted[0]) ? 0
      : wanted.every((w) => title.includes(w)) ? 1
      : wanted.every((w) => whole.includes(w)) ? 2
      : 3
    return { row, score }
  }).filter((s): s is { row: RowInfo; score: number } => s !== null)
  return scored.sort((a, b) => a.score - b.score).map((s) => s.row)
}
