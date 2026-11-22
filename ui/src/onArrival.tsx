// "Back up when this is plugged in": a drive or card's own switch, off
// unless the person turns it on. With it on, Keevoro backs the source up
// by itself a few seconds after it is plugged in, while Keevoro is running
// (by the clock, with its window closed or open), and says so with a
// notice. It never does so for a different drive at the same place, never
// twice at once, and never while paused (`sources.on_arrival`).

import { useState } from 'react'
import type { Backend, Source } from './backend'
import { Toggle } from './toggle'
import { InfoTip } from './infoTip'

export function OnArrival(props: {
  api: Backend
  source: Source
  onChanged: () => void
  onProblem: (error: unknown) => void
}) {
  const { api, source, onProblem } = props
  // What the person just chose, until the list of sources says it too.
  const [chosen, setOn] = useState<boolean | null>(null)
  const on = chosen ?? source.onArrival ?? false
  const change = (next: boolean) => {
    setOn(next)
    api
      .setSourceOnArrival(source.id, next)
      .then((view) => {
        setOn(view.on)
        props.onChanged()
      })
      .catch((error) => {
        setOn(!next)
        onProblem(error)
      })
  }
  return (
    <div className="on-arrival">
      <Toggle
        on={on}
        label="Back up when this is plugged in"
        name={`Back up ${source.name} when it is plugged in`}
        onChange={change}
      />
      {on && (
        <p className="quiet small">
          Backed up by itself when plugged in.
          <InfoTip about="backing up when plugged in">
            A few seconds after it is plugged in, Keevoro backs it up by itself and tells you, while Keevoro is running.
          </InfoTip>
        </p>
      )}
    </div>
  )
}
