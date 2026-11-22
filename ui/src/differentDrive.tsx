// What looking again passed over, and the question asked when a source's
// place holds a different drive.
//
// A source that is not plugged in is never looked through, so what was
// found on it is kept; that is said in Keevoro's own sentence. A drive at a
// source's place that is not the one looked through there before (another
// card in the same reader) is never taken for it without the person's say:
// they choose to add it as a new source, which keeps the first one set
// aside with everything found on it, or to look through it as that source
// anyway.

import { useState } from 'react'
import type { Backend, DifferentDrive, OtherDrive } from './backend'

/** The sentences for the sources passed over, one under another. */
export function PassedOver(props: { said?: string[] }) {
  if (!props.said || props.said.length === 0) return null
  return (
    <ul className="passed-over quiet small">
      {props.said.map((said) => (
        <li key={said}>{said}</li>
      ))}
    </ul>
  )
}

/** One question for each source whose place holds a different drive. */
export function DifferentDriveAsked(props: {
  api: Backend
  drives?: DifferentDrive[]
  onStarted: () => void
  onProblem: (error: unknown) => void
}) {
  const [answered, setAnswered] = useState<Set<number>>(new Set())
  const waiting = (props.drives ?? []).filter((d) => !answered.has(d.source))
  if (waiting.length === 0) return null
  const answer = (drive: DifferentDrive, how: OtherDrive) => {
    setAnswered((a) => new Set([...a, drive.source]))
    props.onStarted()
    props.api.lookAgain(drive.source, how).catch(props.onProblem)
  }
  return (
    <>
      {waiting.map((drive) => (
        <div key={drive.source} className="panel different-drive" role="alertdialog" aria-labelledby={`different-${drive.source}`}>
          <h2 id={`different-${drive.source}`}>Is this {drive.name}?</h2>
          <p>{drive.asked}</p>
          <p className="quiet small">
            Nothing found on {drive.name} has been changed. Adding it as a new source keeps {drive.name} as it was, for when
            its own drive is plugged in again.
          </p>
          <p className="choices">
            <button className="primary" onClick={() => answer(drive, 'addAsNew')}>
              Add it as a new source
            </button>
            <button onClick={() => answer(drive, 'lookAnyway')}>Look through it as {drive.name} anyway</button>
            <button className="button" onClick={() => setAnswered((a) => new Set([...a, drive.source]))}>
              Not now
            </button>
          </p>
        </div>
      ))}
    </>
  )
}
