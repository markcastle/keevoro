// What a backup that ran by itself did before it sent anything, in its
// history: the sources it looked through again and passed over, what was
// new, and what waits for the person's say. The sentences are Keevoro's
// own, the same as `keevoro history` says.

import type { ByItself } from './backend'

export function ByItselfDetail(props: { run: ByItself }) {
  const { run } = props
  return (
    <div className="by-itself">
      {(run.lookedAgain?.length ?? 0) > 0 && (
        <p className="quiet small">It looked through {run.lookedAgain?.join(', ')} again first.</p>
      )}
      {[...(run.passedOver ?? []), ...(run.found ?? [])].map((line) => (
        <p key={line} className="quiet small">
          {line}
        </p>
      ))}
      {(run.waiting?.length ?? 0) > 0 && (
        <>
          <h4>Waiting for your say</h4>
          <ul className="waiting">
            {run.waiting?.map((line) => (
              <li key={line}>{line}</li>
            ))}
          </ul>
        </>
      )}
    </div>
  )
}
