// What the backup screen says about what is really at each destination.
// Keevoro looks at each one when the screen opens (the `room` request does
// it, unless it looked a moment ago) and before every backup; what a look
// found that a person needs to know, such as a folder emptied or a drive
// unplugged, is said here in Keevoro's own words, before anything is sent.
//
// A folder destination whose drive seems unplugged, leaving behind an empty
// folder on another disk, is never backed up into by itself. If that disk
// really is where the backup should go from now on, "Use this folder as it
// is now" adds it again as it is, which is the person saying so.
import { useState } from 'react'
import type { Backend, Destination } from './backend'
import { ago } from './format'
import { lookedSaid, thereNow } from './thereWords'

/** Each sentence the last looks at this backup's destinations have to say. */
export function WhatWasFound(props: {
  destinations: Destination[]
  api: Backend
  onChanged: () => void
  onProblem: (error: unknown) => void
}) {
  const [using, setUsing] = useState(false)
  const said = lookedSaid(props.destinations)
  if (said.length === 0) return null
  const leftBehind = props.destinations.filter((d) => d.looked?.otherDisk && d.folder)
  return (
    <div className="looked" role="status" aria-label="What Keevoro found where this backup goes">
      {said.map((line) => (
        <p key={line}>{line}</p>
      ))}
      {leftBehind.map((d) => (
        <p key={d.name} className="choices">
          <button
            className="button small"
            disabled={using}
            onClick={() => {
              setUsing(true)
              props.api
                .connectDestination({ kind: 'folder', place: d.folder ?? '' })
                .then(props.onChanged)
                .catch(props.onProblem)
                .finally(() => setUsing(false))
            }}
          >
            Use this folder as it is now
          </button>
        </p>
      ))}
    </div>
  )
}

/** How much is waiting and how much is there, said as surely as Keevoro knows it. */
export function ThereNow(props: { destinations: Destination[]; waiting: number; safe: number }) {
  return <p>{thereNow(props.waiting, props.safe, props.destinations, (then) => ago(then))}</p>
}
