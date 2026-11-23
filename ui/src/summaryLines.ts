// What a backup said when it ended, split into the lines that are news and
// the routine figures (previews added, how long it took and how fast),
// which are said after them in quieter, smaller type. Anything about what
// went wrong, or what is safely where, is always among the news. Kept apart from the screen so it can be checked without one.

/** Lines that are routine figures, never news a person must see. */
const ROUTINE = [/^Previews added: /, /^That took about /]

export function splitSummary(lines: string[]): { shown: string[]; more: string[] } {
  const routine = (line: string) => ROUTINE.some((r) => r.test(line))
  return { shown: lines.filter((l) => !routine(l)), more: lines.filter(routine) }
}
