// Leaving a folder out of a backup, and putting it back.
//
// A folder left out is not looked through for this backup, and what was
// found in it is no longer offered. Nothing in the folder is touched, and
// nothing already backed up is: the words that come back say so at the
// moment of the choice. The same button and list are used wherever a
// folder is shown: a survey's suggestions, the look inside a folder, and
// each source's card.

import { useState } from 'react'
import type { Backend, LeftOut, LeftOutFolder } from './backend'

/**
 * "Leave it out", or "Put back" when it is out, with what that did said
 * beside it. `onChanged` hears the new state, for the screen to follow.
 */
export function LeaveOutButton(props: {
  api: Backend
  folder: string
  leftOut: boolean
  /** The label's words for leaving out: "Leave it out" or "Leave out". */
  words?: string
  small?: boolean
  onChanged: (done: LeftOutFolder) => void
  onProblem: (error: unknown) => void
}) {
  const [busy, setBusy] = useState(false)
  const name = props.folder.replace(/[\\/]+$/, '').split(/[\\/]/).pop() || props.folder
  const click = () => {
    setBusy(true)
    props.api
      .leaveOutFolder(props.folder, !props.leftOut)
      .then(props.onChanged)
      .catch(props.onProblem)
      .finally(() => setBusy(false))
  }
  return (
    <button
      className={props.small ? 'button small' : 'button'}
      onClick={click}
      disabled={busy}
      aria-label={props.leftOut ? `Put back ${name}` : `Leave out ${name}`}
      title={
        props.leftOut
          ? 'Include it again in this backup'
          : 'Keep it out of this backup. Nothing in it, or already backed up, is touched.'
      }
    >
      {props.leftOut ? 'Put back' : (props.words ?? 'Leave it out')}
    </button>
  )
}

/** What leaving out or putting back just did, said once, quietly. */
export function LeftOutSaid(props: { done: LeftOutFolder | null }) {
  if (!props.done) return null
  return (
    <p className="left-out-said small" role="status">
      {props.done.said}
    </p>
  )
}

/**
 * The folders the person left out, each with Put back: under a source, or
 * as a survey or search passed them by.
 */
export function LeftOutByYou(props: {
  api: Backend
  folders: string[]
  /** What leads the list, such as "Left out of this backup". */
  title: string
  onChanged: (done: LeftOutFolder) => void
  onProblem: (error: unknown) => void
}) {
  const [done, setDone] = useState<LeftOutFolder | null>(null)
  if (props.folders.length === 0 && !done) return null
  return (
    <div className="left-out-by-you">
      {props.folders.length > 0 && (
        <>
          <p className="small">
            <b>{props.title}</b>
          </p>
          <ul>
            {props.folders.map((folder) => (
              <li key={folder}>
                <span className="where">{folder}</span>
                <LeaveOutButton
                  api={props.api}
                  folder={folder}
                  leftOut
                  small
                  onChanged={(d) => {
                    setDone(d)
                    props.onChanged(d)
                  }}
                  onProblem={props.onProblem}
                />
              </li>
            ))}
          </ul>
        </>
      )}
      <LeftOutSaid done={done} />
    </div>
  )
}

/** The folders a search or survey passed by because the person left them out. */
export const leftOutByYou = (leftOut?: LeftOut[]) =>
  (leftOut ?? []).filter((l) => l.what === 'leftOutByYou').map((l) => l.folder)
