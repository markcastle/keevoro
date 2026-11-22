// A small menu opened from a button: which item an arrow key moves to,
// and where on the window the menu is drawn. Kept apart from the screen so
// it can be checked without one.

/** The space kept clear at each edge of the window, as everywhere else. */
const GUTTER = 16
/** The gap between the button and the menu. */
const GAP = 6

/**
 * The item a key moves to, from the one at `at` (-1 for none) in a menu
 * of `count` items: down and up go round at the ends. Null for a key that
 * moves nothing.
 */
export function menuMove(key: string, at: number, count: number): number | null {
  if (count === 0) return null
  switch (key) {
    case 'ArrowDown':
      return at < 0 ? 0 : (at + 1) % count
    case 'ArrowUp':
      return at < 0 ? count - 1 : (at - 1 + count) % count
    case 'Home':
      return 0
    case 'End':
      return count - 1
    default:
      return null
  }
}

type Box = { left: number; right: number; top: number; bottom: number }

/**
 * Where a menu of `size` goes on a window of `window`, for its button at
 * `button`: under it, lined up with its right edge, or above it when there
 * is no room below, and never past the gutter at any edge.
 */
export function menuPlace(
  button: Box,
  size: { width: number; height: number },
  window: { width: number; height: number },
): { left: number; top: number } {
  const left = Math.max(GUTTER, Math.min(button.right - size.width, window.width - GUTTER - size.width))
  const below = button.bottom + GAP
  const above = button.top - GAP - size.height
  const fitsBelow = below + size.height <= window.height - GUTTER
  const top = fitsBelow || above < GUTTER ? Math.min(below, Math.max(GUTTER, window.height - GUTTER - size.height)) : above
  return { left, top }
}
