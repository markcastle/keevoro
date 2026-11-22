// What a backup said when it ended, split into what is shown at once and
// what waits under "More about this backup". Anything about what went
// wrong, or what is safely where, is always shown; only the routine
// figures (previews added, how long it took and how fast) wait to be
// asked for. Kept apart from the screen so it can be checked without one.

/** Lines that are routine figures, never news a person must see. */
const ROUTINE = [/^Previews added: /, /^That took about /]

export function splitSummary(lines: string[]): { shown: string[]; more: string[] } {
  const routine = (line: string) => ROUTINE.some((r) => r.test(line))
  return { shown: lines.filter((l) => !routine(l)), more: lines.filter(routine) }
}
