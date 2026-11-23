// "Put this backup right": one button for a backup a look or a check found
// files missing from, or not as they were sent. Keevoro never deletes or
// replaces anything in a backup, so this reads back what is there, moves
// any damaged copy aside into a folder of its own at the destination, sends
// those and anything missing again, and proves each new copy. It says what
// it will do, and about how long, before anything is done; it runs in the
// background and can be stopped and pressed again to carry on.
//
// The sentences are the engine's (keevoro_engine::put_right), so the app
// and `keevoro repair plan` say the same.
import { useState } from 'react'
import type { Backend, Destination, PutRightPlan } from './backend'
import { Explained } from './explained'
import { needingAttention } from './thereWords'

export function PutRight(props: {
  destinations: Destination[]
  api: Backend
  /** Starts the job with the words to show while it runs. */
  start: (what: string, job: () => Promise<void>) => void
  onProblem: (error: unknown) => void
}) {
  const [plan, setPlan] = useState<PutRightPlan | null>(null)
  const [asking, setAsking] = useState(false)
  const attention = needingAttention(props.destinations)
  const n = (x: number) => x.toLocaleString('en-GB')
  const ask = () => {
    setAsking(true)
    props.api
      .putRightPlan()
      .then(setPlan)
      .catch(props.onProblem)
      .finally(() => setAsking(false))
  }
  // Nothing known to be wrong: the full proof is still a press away, quietly,
  // for a backup that went through a share that dropped out.
  if (attention === 0 && plan === null) {
    if (!props.destinations.some((d) => d.there > 0)) return null
    return (
      <Explained
        className="prove"
        action={
          <button className="button small" disabled={asking} onClick={ask}>
            Prove every copy…
          </button>
        }
      >
        The thorough check: reads every copy back against its original. Slower; it says how long first.
      </Explained>
    )
  }
  const go = (thorough: boolean) => {
    setPlan(null)
    props.start('Putting this backup right…', () => props.api.putRight(thorough))
  }
  return (
    <div className="put-right" role="region" aria-label="Put this backup right">
      {plan === null ? (
        <>
          <p>
            Keevoro can put {attention === 1 ? 'this' : `these ${n(attention)}`} right itself. Nothing in your backup is
            deleted or replaced.
          </p>
          <p className="choices">
            <button className="primary" disabled={asking} onClick={ask}>
              Put this backup right…
            </button>
          </p>
        </>
      ) : (
        <>
          <p>
            <b>Put this backup right</b>
          </p>
          {plan.places.flatMap((place) => place.said).map((line) => (
            <p key={line}>{line}</p>
          ))}
          {plan.known > 0 && (
            <p className="quiet small">Checking every file also finds any problem not yet known; the known ones alone is quicker.</p>
          )}
          <p className="choices">
            <button className="primary" onClick={() => go(true)}>
              Put it right and check every file ({plan.thoroughSaid})
            </button>
            {plan.known > 0 && (
              <button className="button" onClick={() => go(false)}>
                Only the {n(plan.known)} known ({plan.quickSaid})
              </button>
            )}
            <button className="button" onClick={() => setPlan(null)}>
              Not now
            </button>
          </p>
        </>
      )}
    </div>
  )
}
