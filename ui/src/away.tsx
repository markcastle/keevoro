// "While you were away": at the top of the backup screen when the window
// comes back, what every backup did by itself since the person last had it
// open: each run and what it backed up or why it couldn't, runs that were
// missed because the computer was off and caught up, and what waits for
// their say. Done puts it away until something new happens.
//
// Every word comes from Keevoro (`schedule.away`), the same the command
// line's `keevoro away` says, so the screen never works a time out itself.

import { useCallback, useEffect, useState } from 'react'
import type { AwayRun, AwayView, Backend } from './backend'
import { Explained } from './explained'

/** A run's mark, in words, never colour alone. */
const MARKS: Record<AwayRun['outcome'], string> = {
  worked: 'Done',
  someFiles: 'Some files not sent',
  unreachable: 'Couldn’t reach it',
  refused: 'Couldn’t back up',
  stopped: 'Stopped',
  heldBack: 'Waiting for your say',
  notNow: 'Waited',
  skipped: 'Skipped',
}

const LOUD: AwayRun['outcome'][] = ['someFiles', 'unreachable', 'refused', 'heldBack']

export function AwayPanel(props: { api: Backend; onProblem: (error: unknown) => void }) {
  const { api, onProblem } = props
  const [view, setView] = useState<AwayView | null>(null)
  const read = useCallback(() => {
    api.away().then(setView).catch(onProblem)
  }, [api, onProblem])
  useEffect(read, [read])
  // A run by itself that ends while the window is open adds its line.
  useEffect(
    () =>
      api.onSend((event) => {
        if (event.finished) read()
      }),
    [api, read],
  )
  if (!view || (view.backups.length === 0 && view.waiting.length === 0)) return null
  const done = () => {
    api
      .awaySeen()
      .then(() => setView(null))
      .catch(onProblem)
  }
  return (
    <section className="away" aria-labelledby="away-title">
      <h2 id="away-title">While you were away</h2>
      {view.backups.map((b) => (
        <div className="away-backup" key={b.id}>
          <h3>{b.backup}</h3>
          <ul className="away-runs">
            {b.runs.map((run) => (
              <li key={run.at} className={LOUD.includes(run.outcome) ? 'loud' : undefined}>
                <span className="when">{run.when}</span>
                <span className="said">
                  <span className="mark">{MARKS[run.outcome] ?? run.outcome}.</span> {run.said}
                  {run.caughtUp && <span className="quiet"> {run.caughtUp}</span>}
                </span>
              </li>
            ))}
          </ul>
          {b.missed && <p className="quiet away-missed">{b.missed}</p>}
        </div>
      ))}
      {view.waiting.length > 0 && (
        <div className="away-backup">
          <h3>Waiting for your say</h3>
          <ul className="away-runs">
            {view.waiting.map((w) => (
              <li key={`${w.id} ${w.said}`} className="loud">
                <span className="when">{w.backup}</span>
                <span className="said">{w.said}</span>
              </li>
            ))}
          </ul>
        </div>
      )}
      <Explained
        className="away-done"
        action={
          <button className="button" onClick={done}>
            Done
          </button>
        }
      >
        That is everything since you last had Keevoro open. Each backup’s last few runs stay below, under Backing up by
        itself.
      </Explained>
    </section>
  )
}
