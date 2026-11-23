// What a backup, or one source, is for: photos, videos, music and, once a
// kind of other file is on Keevoro's list, other files. A switch for each,
// at least one always on. Photos, Videos and Music are three equal
// choices; "Other files" is only offered once there is something it could
// mean, so a person who never adds a kind of other file sees the three and
// nothing more.
//
// The same three are asked up front, as "What do you want to back up?",
// when a backup is made (`WhatToBackUp`), and shown on the first step as a
// line that says what the open backup is for (`BackingUpLine`), with
// "Change" opening the same switches in place.
//
// What the choice means is said once it is made, including that nothing
// already backed up is touched. The words come from the API, so the
// screens and the command line say the same.

import { useEffect, useState } from 'react'
import type { Wants, WantsView, WantsWord } from './backend'
import { holdsLine } from './format'
import { Toggle } from './toggle'
import { sameWants, shownWords, sourceWantsChoice, toggled, toggledNew } from './wantsRule'

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

/** Said when the last switch on is pressed: a backup is always for something. */
export const KEEP_ONE_ON = 'Keep at least one on.'

/** What the API says when music is asked for; the app looks through again itself, and says so. */
const LOOK_AGAIN_SAID = 'Look through your sources again to find your music.'
export const LOOKING_FOR_MUSIC = 'Keevoro is looking through your sources for your music now.'

/** What a change means, with what the app does about music in place of what the API asks. */
export function saidOf(view: WantsView) {
  return view.looksAgain ? view.said.replace(LOOK_AGAIN_SAID, LOOKING_FOR_MUSIC) : view.said
}

/**
 * The question asked when a backup is made: "What do you want to back
 * up?", with Photos, Videos and Music as three equal switches. At least one
 * stays on, and pressing the last says so plainly.
 */
export function WhatToBackUp(props: { wants: WantsWord[]; choose: (wants: WantsWord[]) => void; id?: string }) {
  const [refused, setRefused] = useState(false)
  return (
    <fieldset className="wants what-to-back-up" id={props.id}>
      <legend>What do you want to back up?</legend>
      <div className="kv-panel kv-wants">
        {shownWords(false).map(([word, label]) => {
          const on = props.wants.includes(word)
          const next = toggledNew(props.wants, word, !on)
          return (
            <Toggle
              key={word}
              row
              on={on}
              label={label}
              locked={next === null}
              onLocked={() => setRefused(true)}
              onChange={() => {
                setRefused(false)
                if (next) props.choose(next)
              }}
            />
          )
        })}
      </div>
      {refused && (
        <p className="quiet small" role="status">
          {KEEP_ONE_ON}
        </p>
      )}
    </fieldset>
  )
}

/**
 * What the open backup is for, always in view near the top of the first
 * step: "Backing up: Photos, Videos" and "Change", which opens the same
 * switches in place, with no trip to Settings.
 */
export function BackingUpLine(props: {
  load: () => Promise<WantsView>
  choose: (wants: Wants) => Promise<WantsView>
  /** Heard after every change, with what the backup is for now. */
  onChanged: (view: WantsView) => void
  onProblem: (error: unknown) => void
}) {
  const { load, onProblem } = props
  const [view, setView] = useState<WantsView | null>(null)
  const [open, setOpen] = useState(false)
  const [said, setSaid] = useState<string | null>(null)
  useEffect(() => {
    load().then(setView).catch(onProblem)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])
  if (!view) return null
  return (
    <div className="backing-up">
      <p className="backing-up-line">
        <span>
          Backing up: <strong>{holdsLine(view.wants)}</strong>
        </span>
        <button className="link" aria-expanded={open} onClick={() => setOpen(!open)}>
          {open ? 'Done' : 'Change'}
        </button>
      </p>
      {open && (
        <div className="kv-panel kv-wants backing-up-choices">
          <Ticks
            legend="What do you want to back up?"
            // The same three as when the backup was made; other files are a Settings matter.
            view={{ ...view, offersOthers: false }}
            choose={(wants) =>
              props
                .choose(wants)
                .then((changed) => {
                  setView(changed)
                  setSaid(saidOf(changed))
                  props.onChanged(changed)
                })
                .catch(onProblem)
            }
            onRefused={() => setSaid(KEEP_ONE_ON)}
          />
        </div>
      )}
      {open && said && (
        <p className="quiet small" role="status">
          {said}
        </p>
      )}
    </div>
  )
}

/** What a whole backup is for. */
export function BackupWants(props: {
  load: () => Promise<WantsView>
  choose: (wants: Wants) => Promise<WantsView>
  onChanged?: (view: WantsView) => void
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
        onRefused={() => props.onSaid(KEEP_ONE_ON)}
        choose={(wants) =>
          props
            .choose(wants)
            .then((changed) => {
              setView(changed)
              props.onSaid(
                changed.looksAgain ? saidOf(changed) : `${changed.said} This counts at once, with no need to look again.`,
              )
              props.onChanged?.(changed)
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
  onChanged?: (view: WantsView) => void
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
        const fewer = !view.wants.every((w) => changed.wants.includes(w))
        setSaid(changed.looksAgain ? LOOKING_FOR_MUSIC : fewer ? 'Anything already backed up stays backed up.' : null)
        props.onChanged?.(changed)
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
