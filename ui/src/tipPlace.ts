// Where a small explanation opened from an "i" button is drawn on the
// window: above the button, centred on it, or under it when there is no
// room above, and never past the space kept clear at any edge. Kept apart
// from the screen so it can be checked without one.

/** The space kept clear at each edge of the window, as everywhere else. */
const GUTTER = 16
/** The gap between the button and the explanation. */
const GAP = 8

type Box = { left: number; right: number; top: number; bottom: number }

/**
 * Where an explanation of `size` goes on a window of `window`, for its
 * button at `button`, and whether it ended up above it.
 */
export function tipPlace(
  button: Box,
  size: { width: number; height: number },
  window: { width: number; height: number },
): { left: number; top: number; above: boolean } {
  const centre = (button.left + button.right) / 2
  const left = Math.max(GUTTER, Math.min(centre - size.width / 2, window.width - GUTTER - size.width))
  const below = button.bottom + GAP
  const above = button.top - GAP - size.height
  // Above when there is room: what a label explains is usually the line
  // right under it, and an explanation should not cover that.
  if (above >= GUTTER) return { left, top: above, above: true }
  return { left, top: Math.max(GUTTER, Math.min(below, window.height - GUTTER - size.height)), above: false }
}
