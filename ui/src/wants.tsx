// What a backup, or one source, is for: photos, videos and, once a kind of
// other file is on Keevoro's list, other files. A switch for each, at
// least one always on. "Other files" is only offered once there is
// something it could mean, so a person who never adds a kind of other file
// sees "Photos" and "Videos" and nothing more.
//
// What the choice means is said once it is made, including that nothing
// already backed up is touched. The words come from the API, so the
// screens and the command line say the same.

import { useEffect, useState } from 'react'
import type { Wants, WantsView } from './backend'
import { Toggle } from './toggle'
import { shownWords, toggled } from './wantsRule'

/**
 * The switches themselves, a row each, as Settings lays out its rows: the
 * kind on the left, its switch on the right, a hairline between them. They
 * are rows of whatever panel they sit in; that panel is the caller's.
 */
function Ticks(props: {
  legend: string
  view: WantsView
  disabled?: boolean
  choose: (wants: Wants) => void
}) {
  const offers = !!props.view.offersOthers
  return (
    <fieldset className="wants" disabled={props.disabled}>
      <legend>{props.legend}</legend>
      {shownWords(offers).map(([word, label]) => {
        const on = props.view.wants.includes(word)
        const next = toggled(props.view.wants, word, !on, offers)
        return (
          <Toggle
            key={word}
            row
            on={on}
            label={label}
            // The last one on stays on: a backup is always for something.
            locked={next === null}
            onChange={() => next && props.choose(next)}
          />
        )
      })}
      {offers && (
        <p className="quiet small">Other files are backed up exactly, but cannot be shown or grouped by date.</p>
      )}
    </fieldset>
  )
}

/** What a whole backup is for. */
export function BackupWants(props: {
  load: () => Promise<WantsView>
  choose: (wants: Wants) => Promise<WantsView>
  onChanged?: () => void
  /** What a change means, for saying somewhere of the caller's choosing. */
  onSaid: (said: string) => void
  onProblem: (error: unknown) => void
}) {
  const { load, onProblem } = props
  const [view, setView] = useState<WantsView | null>(null)
  useEffect(() => {
    load().then(setView).catch(onProblem)
    // Asked once, when it is first shown.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])
  if (!view) return null
  return (
    <>
      <Ticks
        legend="This backup is for"
        view={view}
        choose={(wants) =>
          props
            .choose(wants)
            .then((changed) => {
              setView(changed)
              props.onSaid(`${changed.said} This counts at once, with no need to look again.`)
              props.onChanged?.()
            })
            .catch(onProblem)
        }
      />
    </>
  )
}

/** What one source is for: the same as its backup, or a choice of its own. */
export function SourceWants(props: {
  name: string
  load: () => Promise<WantsView>
  choose: (wants: Wants | 'backup') => Promise<WantsView>
  onChanged?: () => void
  onProblem: (error: unknown) => void
}) {
  const { load, onProblem } = props
  const [view, setView] = useState<WantsView | null>(null)
  const [said, setSaid] = useState<string | null>(null)
  useEffect(() => {
    load().then(setView).catch(onProblem)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])
  if (!view) return null
  const choose = (wants: Wants | 'backup') =>
    props
      .choose(wants)
      .then((changed) => {
        setView(changed)
        setSaid(changed.said)
        props.onChanged?.()
      })
      .catch(onProblem)
  return (
    <div className="source-wants">
      {/* One panel: whether it follows the backup, and, once it does not, a row for each kind. */}
      <div className="kv-panel kv-wants">
        <Toggle
          row
          on={!view.own}
          label={`From ${props.name}, back up the same as the backup`}
          // On again, it follows the backup; off, its own choice starts as
          // what it follows now.
          onChange={() => choose(view.own ? 'backup' : view.wants)}
        />
        {view.own && <Ticks legend={`From ${props.name}, back up`} view={view} choose={choose} />}
      </div>
      {said && (
        <p className="quiet small" role="status">
          {said} This counts at once, with no need to look again.
        </p>
      )}
    </div>
  )
}
