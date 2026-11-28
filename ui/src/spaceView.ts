// How a person last chose to see where the space goes: as a list alone or
// with the map beside it, and with the list by kind of file or by folder.
// Remembered on this computer like the theme: it is only how the window
// draws, so it is not kept in the register.
//
// Unset or unreadable, it is the list, by kind of file. A trusted tester
// found that the plainest way in: the ring and a short list say where the
// room went at a glance, and the map is one press away for the detail.

export type SpaceView = 'map' | 'list'
export type SpaceList = 'kind' | 'folder'

const VIEW = 'keevoro-space-view'
const LIST = 'keevoro-space-list'

function saved<T extends string>(key: string, other: T, usual: T): T {
  try {
    return localStorage.getItem(key) === other ? other : usual
  } catch {
    // No storage (a private window, say): the usual.
    return usual
  }
}

function choose(key: string, value: string) {
  try {
    localStorage.setItem(key, value)
  } catch {
    // It still applies for now; it just will not be remembered.
  }
}

export const savedSpaceView = () => saved<SpaceView>(VIEW, 'map', 'list')
export const chooseSpaceView = (view: SpaceView) => choose(VIEW, view)
export const savedSpaceList = () => saved<SpaceList>(LIST, 'folder', 'kind')
export const chooseSpaceList = (list: SpaceList) => choose(LIST, list)
