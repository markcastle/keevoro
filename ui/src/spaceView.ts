// Which view of where the space goes a person last chose: the map beside
// the list, or the list alone with wide share bars. Remembered on this
// computer like the theme: it is only how the window draws, so it is not
// kept in the register. Unset or unreadable, it is the map, as before.

export type SpaceView = 'map' | 'list'

const KEY = 'keevoro-space-view'

export function savedSpaceView(): SpaceView {
  try {
    return localStorage.getItem(KEY) === 'list' ? 'list' : 'map'
  } catch {
    // No storage (a private window, say): the map.
    return 'map'
  }
}

export function chooseSpaceView(view: SpaceView) {
  try {
    if (view === 'map') localStorage.removeItem(KEY)
    else localStorage.setItem(KEY, view)
  } catch {
    // It still applies for now; it just will not be remembered.
  }
}
