// Which box scrolls the page.
//
// In a wide window the page scrolls inside `main`, beside the side bar. In
// a narrow one the side bar is a band across the top that scrolls away
// with the page, so the whole window scrolls instead. Anything that keeps
// or restores a place on the page asks here rather than assuming `main`.

/** The nearest box at or above `el` that scrolls up and down, if any. */
export function scrollerOf(el: Element | null): HTMLElement | null {
  for (let at = el; at; at = at.parentElement) {
    const y = getComputedStyle(at).overflowY
    if ((y === 'auto' || y === 'scroll') && at instanceof HTMLElement) return at
  }
  return null
}
