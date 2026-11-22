// Clearing the unfinished copies a stopped backup left in a folder.
//
// One button, then the same three steps wherever it is offered: say what
// would be cleared (asking costs one small folder listing), clear it when
// the person says so, and say what was cleared and what was left and why.
// The sentences are Keevoro's own, the same as `keevoro tidy` prints. The
// question is asked in the page, beside the button, never in a pop-up.
// Chosen from a menu instead, it has no button of its own: it looks at
// once, and says when its question is put away.

import { useEffect, useLayoutEffect, useRef, useState } from 'react'
import type { Backend, Tidied } from './backend'

export function ClearUnfinished(props: {
  api: Backend
  /** The folder to clear. Without one, the person is asked to choose a folder first. */
  folder?: string
  /** The button's words. */
  label: string
  /** A shorter button, for beside others. */
  small?: boolean
  /** Chosen from a menu: looks at once, with no button of its own. */
  asked?: boolean
  /** With `asked`, the question is put away, or there was none to ask. */
  onClosed?: () => void
  onProblem: (error: unknown) => void
}) {
  const { api, onProblem, asked, onClosed } = props
  // Nothing asked yet, what would be cleared, or what was.
  const [found, setFound] = useState<Tidied | null>(null)
  const [busy, setBusy] = useState(false)
  const group = useRef<HTMLDivElement>(null)

  const look = async () => {
    const folder = props.folder ?? (await api.chooseFolderToTidy())
    if (!folder) return onClosed?.()
    setFound(await api.tidyFolder(folder, true))
  }
  const run = (work: () => Promise<void>, ifNot?: () => void) => {
    setBusy(true)
    work()
      .catch((error) => {
        onProblem(error)
        ifNot?.()
      })
      .finally(() => setBusy(false))
  }
  const putAway = () => {
    setFound(null)
    if (asked) onClosed?.()
  }

  // Asked from a menu: looked at once, only once.
  const began = useRef(false)
  useEffect(() => {
    if (!asked || began.current) return
    began.current = true
    run(look, onClosed)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])
  // What it found is where the focus goes, since the menu that asked is
  // gone: once its buttons can be pressed.
  const ready = asked && found !== null && !busy
  useLayoutEffect(() => {
    if (ready) group.current?.querySelector('button')?.focus()
  }, [ready])

  if (!found) {
    if (asked) return null
    return (
      <button
        className={props.small ? 'button small' : 'button'}
        disabled={busy}
        onClick={() => run(look)}
        title="Clears the working files a backup that was stopped part-way left behind. Nothing that was backed up is touched. Keevoro says how much first."
      >
        {props.label}
      </button>
    )
  }
  const asking = !found.cleared && found.files > 0
  return (
    <div className="tidying" role="group" ref={group} aria-label={`Unfinished copies in ${found.folder}`}>
      {found.said.map((line) => (
        <p key={line}>{line}</p>
      ))}
      <p className="choices">
        {asking ? (
          <>
            <button
              className="primary"
              disabled={busy}
              onClick={() => run(async () => setFound(await api.tidyFolder(found.folder, false)))}
            >
              {busy ? 'Clearing…' : 'Clear them'}
            </button>
            <button className="button" disabled={busy} onClick={putAway}>
              Cancel
            </button>
          </>
        ) : (
          <button className="button" onClick={putAway}>
            Done
          </button>
        )}
      </p>
    </div>
  )
}
