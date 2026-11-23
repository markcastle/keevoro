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
import { sameWants, shownWords, sourceWantsChoice, toggled } from './wantsRule'

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
  /** Heard when the last one on is pressed, which stays on. */
  onRefused?: () => void
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
            onLocked={props.onRefused}
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

/**
 * What one source is for, said as a person would: "Back up from Desktop",
 * then Photos, Videos and Other files. The switches start as the rest of
 * the backup is, and flipping one is all it takes for this source to
 * differ; set back to match, it goes with the rest of the backup again
 * (wantsRule.ts), so a later change to the whole backup still reaches it.
 * A quiet line says when it differs, with the way back.
 */
export function SourceWants(props: {
  name: string
  load: () => Promise<WantsView>
  /** What the rest of the backup is for. */
  loadBackup: () => Promise<WantsView>
  choose: (wants: Wants | 'backup') => Promise<WantsView>
  onChanged?: () => void
  onProblem: (error: unknown) => void
}) {
  const { load, loadBackup, onProblem } = props
  const [view, setView] = useState<WantsView | null>(null)
  const [backup, setBackup] = useState<Wants | null>(null)
  const [said, setSaid] = useState<string | null>(null)
  useEffect(() => {
    load().then(setView).catch(onProblem)
    loadBackup()
      .then((b) => setBackup(b.wants))
      .catch(onProblem)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])
  if (!view || !backup) return null
  const choose = (wants: Wants | 'backup') =>
    props
      .choose(wants)
      .then((changed) => {
        setView(changed)
        // Switching something off never takes away what is already safe: said at the moment of the change.
        setSaid(sameWants(changed.wants, view.wants) || changed.wants.length >= view.wants.length ? null : 'Anything already backed up stays backed up.')
        props.onChanged?.()
      })
      .catch(onProblem)
  const differs = !!view.own && !sameWants(view.wants, backup)
  const heading = `Back up from ${props.name}`
  return (
    <div className="source-wants">
      <h4 className="source-choice-head">{heading}</h4>
      <div className="kv-panel kv-wants">
        <Ticks
          legend={heading}
          view={view}
          choose={(wants) => choose(sourceWantsChoice(wants, backup))}
          onRefused={() => setSaid(`Nothing would be backed up from ${props.name}. Keep one on, or remove this source.`)}
        />
      </div>
      {differs && (
        <p className="quiet small differs">
          Different from the rest of this backup.{' '}
          <button className="link" onClick={() => choose('backup')}>
            Use the usual
          </button>
        </p>
      )}
      {said && (
        <p className="quiet small" role="status">
          {said}
        </p>
      )}
    </div>
  )
}
