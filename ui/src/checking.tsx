// The background check for duplicates, in a line of its own, wherever it
// shows: the foot of the side band and "What Keevoro is doing".
//
// The first tester stopped a search and found the check that follows it
// still running, with nothing to say how to stop it and "You can carry on"
// reading like an instruction. So it is said as what it is doing, with a
// Pause and a Stop of its own beside it as real buttons. Its own Pause
// pauses only the check; Keevoro's Pause still pauses everything, the
// check included. Stopped, it says what that means and offers to check
// now; it does not start again by itself until the next search or backup.
//
// The sentences match `keevoro_api::words::checking`, `checking_paused`
// and `check_stopped`, which the command line and an agent are given.

import type { Backend, CheckEvent, DuplicatesLeft } from './backend'
import { thousands } from './format'

/** What the check is doing now, as a sentence. */
export function checkingWords(check: CheckEvent, pausedAlone: boolean) {
  const at = `(${thousands(check.done)} of ${thousands(check.total)})`
  return pausedAlone
    ? `Checking for duplicates is paused ${at}. Everything else carries on.`
    : `Checking for duplicates in the background ${at}. You can keep using Keevoro while it finishes.`
}

/** The check while it runs: what it is doing, with Pause (or Carry on) and Stop. */
export function CheckLine(props: {
  api: Backend
  check: CheckEvent
  pausedAlone: boolean
  onPausedAlone: (paused: boolean) => void
  /** For the narrow foot of the side band: what Pause and Stop each do waits behind their names. */
  brief?: boolean
  onProblem: (error: unknown) => void
}) {
  const { api, check, pausedAlone, onProblem } = props
  const pauseSaid = 'Holds the check where it is, to carry on later.'
  const stopSaid = 'Ends the check. It starts again with the next search or backup.'
  return (
    <div className="checking" role="status">
      <p>
        {!pausedAlone && <span className="spinner" aria-hidden />}
        {checkingWords(check, pausedAlone)}
      </p>
      <p className="row-actions">
        <button
          className="button small"
          onClick={() =>
            api
              .pauseChecking(!pausedAlone)
              .then(() => props.onPausedAlone(!pausedAlone))
              .catch(onProblem)
          }
          aria-label={pausedAlone ? 'Carry on checking for duplicates' : 'Pause checking for duplicates'}
          title={props.brief && !pausedAlone ? pauseSaid : undefined}
        >
          {pausedAlone ? 'Carry on' : 'Pause'}
        </button>
        <button
          className="button small"
          onClick={() => api.stopChecking().catch(onProblem)}
          aria-label="Stop checking for duplicates"
          title={props.brief ? stopSaid : undefined}
        >
          Stop
        </button>
      </p>
      {!props.brief && (
        <p className="quiet small check-said">
          Pause holds the check where it is. Stop ends it until the next search or backup.
        </p>
      )}
    </div>
  )
}

/**
 * The check, stopped by the person, with files left to read: what that
 * means and when it starts again, and "Check for duplicates now". `brief`
 * says only the first sentence, for the narrow foot of the side band.
 */
export function CheckStopped(props: {
  api: Backend
  left: DuplicatesLeft
  brief?: boolean
  onStarted: () => void
  onProblem: (error: unknown) => void
}) {
  const said = props.left.said ?? []
  const [first, ...rest] = said
  return (
    <div className="check-stopped" role="status">
      <p>{first}</p>
      {!props.brief && rest.map((line) => (
        <p key={line} className="quiet small">
          {line}
        </p>
      ))}
      <p className="row-actions">
        <button
          className="button small"
          title={props.brief ? rest.join(' ') : undefined}
          onClick={() =>
            props.api
              .checkDuplicatesNow()
              .then(props.onStarted)
              .catch(props.onProblem)
          }
        >
          Check for duplicates now
        </button>
      </p>
    </div>
  )
}
