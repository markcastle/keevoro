// The browser's own menu and keys, kept out of the app.
//
// The app's window is a web view, and by default a right-click opens the
// web view's menu, with Back, Refresh (Reload on a Mac) and Print. Our first
// tester used Refresh to make a drive appear. Refresh reloads the whole app:
// a search or survey in progress drops off the screen while it carries on
// unseen, a half-typed name or a question being answered is lost, and the
// app opens again on another step. A person needs none of it, so in the
// released app the menu is not shown and the reload keys (F5, Ctrl+R, and
// Cmd+R on a Mac) do nothing. Everything a person needs is a button.
//
// The menu is still offered where it is useful: in a box being typed in,
// for cut, copy and paste, and over text the person has selected, to copy
// it. While developing (`npm run dev`) everything is left as it was.

/** True when a right-click here should get the web view's own menu. */
export function menuWanted(target: EventTarget | null, selection: string) {
  if (selection.trim() !== '') return true
  // Anything with `closest` is an element; asked so as not to need the page's own types here.
  const element = target as { closest?: (selector: string) => unknown } | null
  return Boolean(element?.closest?.('input, textarea, [contenteditable="true"]'))
}

/** True for the keys that reload the page. */
export function reloadKey(key: string, ctrl: boolean, meta: boolean) {
  const lower = key.toLowerCase()
  return key === 'F5' || ((ctrl || meta) && lower === 'r')
}

/** Keeps the web view's own menu and reload keys out of the released app. */
export function keepTheBrowserOut() {
  document.addEventListener('contextmenu', (event) => {
    if (!menuWanted(event.target, window.getSelection()?.toString() ?? '')) event.preventDefault()
  })
  document.addEventListener('keydown', (event) => {
    if (reloadKey(event.key, event.ctrlKey, event.metaKey)) event.preventDefault()
  })
}
