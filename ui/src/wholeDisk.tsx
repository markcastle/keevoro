// Asked before looking through a whole disk: three ways to go on, each a
// row of its own with what it does and how long it takes, in view, so
// nobody has to guess what a button might do before pressing it. The quick
// way is the one filled. Up and down move between the rows; Enter or a
// click chooses one. Each row is named by its title alone, and described
// by its line.

import { useEffect, useRef } from 'react'
import type { KeyboardEvent } from 'react'

export function WholeDiskAsked(props: {
  /** What the disk is called: "Work (G:)". */
  name: string
  /** How big it is ("512 GB"), when known. */
  size?: string
  onFindFolders: () => void
  onChooseFolder: () => void
  onLookThroughAll: () => void
  onCancel: () => void
}) {
  const list = useRef<HTMLDivElement>(null)
  // The question is answered from the keyboard as readily as with a click: the quick way has the focus to begin with.
  useEffect(() => {
    list.current?.querySelector<HTMLButtonElement>('button')?.focus({ preventScroll: true })
  }, [])
  const move = (event: KeyboardEvent) => {
    if (event.key !== 'ArrowDown' && event.key !== 'ArrowUp' && event.key !== 'Home' && event.key !== 'End') return
    const options = [...(list.current?.querySelectorAll<HTMLButtonElement>('button') ?? [])]
    const at = options.indexOf(document.activeElement as HTMLButtonElement)
    const next =
      event.key === 'Home'
        ? 0
        : event.key === 'End'
          ? options.length - 1
          : (at + (event.key === 'ArrowDown' ? 1 : -1) + options.length) % options.length
    event.preventDefault()
    options[next]?.focus()
  }
  return (
    <div className="panel asking whole-disk" role="alertdialog" aria-labelledby="asking-title" aria-describedby="asking-said">
      <h2 id="asking-title">Look through all of {props.name}?</h2>
      <p id="asking-said">
        That is a whole disk{props.size ? ` (${props.size})` : ''}, and most of it will not be photos. Nothing on it is
        changed, whichever you choose.
      </p>
      <div className="options" role="group" aria-label="How to look" ref={list} onKeyDown={move}>
        <button className="option primary" onClick={props.onFindFolders} aria-label="Find the folders with photos" aria-describedby="find-folders-said">
          <span className="option-title">
            Find the folders with photos <span className="option-badge">Recommended</span>
          </span>
          <span className="option-said" id="find-folders-said">
            Quick. Reads only the names of what is on it, opens nothing, then shows which folders hold photos for you to
            tick. Takes seconds.
          </span>
        </button>
        <button className="option" onClick={props.onChooseFolder} aria-label="Choose a folder inside it" aria-describedby="choose-folder-said">
          <span className="option-title">Choose a folder inside it</span>
          <span className="option-said" id="choose-folder-said">
            If you know where your photos are, pick that one folder to look through.
          </span>
        </button>
        <button className="option" onClick={props.onLookThroughAll} aria-label="Look through all of it" aria-describedby="look-all-said">
          <span className="option-title">Look through all of it</span>
          <span className="option-said" id="look-all-said">
            Goes through every file on the disk. A few minutes to an hour or more; you can leave it running.
          </span>
        </button>
      </div>
      <p className="cancel-line">
        <button className="link cancel" onClick={props.onCancel}>
          Cancel
        </button>
      </p>
    </div>
  )
}
