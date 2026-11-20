// The share of a backup done, as the bar shows it.
//
// A re-estimate can make the share slip back a little, and a bar that
// twitches backwards looks like lost work, so a small slip is held. But
// the bar must never be stuck on a figure that was wrong: a reading well
// below what is shown means the earlier one overshot (an estimate made
// before the work was known, say), and the truth wins.
export const SLIP_HELD = 0.05

export function heldShare(shown: number, next: number): number {
  if (!Number.isFinite(next)) return shown
  if (next >= shown) return next
  return shown - next <= SLIP_HELD ? shown : next
}
