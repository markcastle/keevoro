// What the backup screen says about what is really at each destination.
// Keevoro looks at each one when the screen opens (the `room` request does
// it, unless it looked a moment ago) and before every backup; what a look
// found that a person needs to know, such as a folder emptied or a drive
// unplugged, is said here in Keevoro's own words, before anything is sent.
import type { Destination } from './backend'
import { ago } from './format'
import { lookedSaid, thereNow } from './thereWords'

/** Each sentence the last looks at this backup's destinations have to say. */
export function WhatWasFound(props: { destinations: Destination[] }) {
  const said = lookedSaid(props.destinations)
  if (said.length === 0) return null
  return (
    <div className="looked" role="status" aria-label="What Keevoro found where this backup goes">
      {said.map((line) => (
        <p key={line}>{line}</p>
      ))}
    </div>
  )
}

/** How much is waiting and how much is there, said as surely as Keevoro knows it. */
export function ThereNow(props: { destinations: Destination[]; waiting: number; safe: number }) {
  return <p>{thereNow(props.waiting, props.safe, props.destinations, (then) => ago(then))}</p>
}
