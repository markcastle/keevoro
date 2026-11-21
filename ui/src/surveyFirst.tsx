// Asking about a place Keevoro may not have surveyed yet: the look inside
// a folder and the map of where the space goes are both answered from a
// remembered survey. When there is none, the place is surveyed first (it
// only reads folder listings, so it is quick), with how far it has got and
// a way to stop, and then asked again.
//
// A survey finds every photo and video first, so the suggestions and the
// look inside are ready, then measures the folders Keevoro never looks
// through (system and app folders, its own), so the map shows everything.
// While it measures, stopping loses nothing already found.

import { useEffect, useRef, useState } from 'react'
import type { Backend, SurveyProgress } from './backend'
import { count, files } from './format'
import { LiveMap } from './liveMap'

export type Surveying = { progress: SurveyProgress | null; stopping: boolean }

/**
 * Asks `ask` about `place`, surveying it first when the answer is `null`.
 * `ask` is asked again whenever it changes, so make it with `useCallback`.
 * `answer` is `undefined` until the first answer comes, and keeps the last
 * answer while a new one is asked for, so a screen does not flash.
 * `again` asks afresh whenever it changes. `mapAgain` surveys afresh.
 */
export function useAfterSurvey<T>(
  api: Backend,
  place: string,
  ask: () => Promise<T | null>,
  again: string,
  onProblem: (error: unknown) => void,
) {
  const [answer, setAnswer] = useState<T | null | undefined>(undefined)
  const [surveying, setSurveying] = useState<Surveying | null>(null)
  const [fresh, setFresh] = useState(0)
  // Map again: survey before asking, even though a survey is remembered.
  const resurvey = useRef(false)

  useEffect(() => {
    let current = true
    const unhear = api.onSurvey((progress) => {
      // The top of the map comes twice a second; between, the last is kept.
      if (current) setSurveying((s) => s && { ...s, progress: { ...progress, top: progress.top ?? s.progress?.top } })
    })
    const survey = async () => {
      setSurveying({ progress: null, stopping: false })
      await api.surveyPlace(place)
    }
    const first = resurvey.current ? survey().then(() => ask()) : ask()
    resurvey.current = false
    first
      .then(async (found) => {
        if (found !== null || !current) return found
        await survey()
        return ask()
      })
      .then((found) => {
        if (!current) return
        setSurveying(null)
        setAnswer(found)
      })
      .catch((error) => {
        if (!current) return
        setSurveying(null)
        onProblem(error)
      })
    return () => {
      current = false
      unhear()
    }
  }, [api, place, ask, again, fresh, onProblem])

  const stop = () => {
    setSurveying((s) => s && { ...s, stopping: true })
    api.stopSurvey().catch(onProblem)
  }
  const mapAgain = () => {
    resurvey.current = true
    setFresh((n) => n + 1)
  }
  return { answer, surveying, stop, mapAgain }
}

/** How far a survey of `name` has got, with a way to stop it. */
export function SurveyingPanel(props: { name: string; surveying: Surveying; onStop: () => void }) {
  const { progress, stopping } = props.surveying
  return (
    <div className="panel searching">
      <span className="spinner" aria-hidden />
      <h2>Finding what is in {props.name}…</h2>
      <p className="big-count" role="status">
        {progress ? files(progress.photos, progress.videos, progress.others ?? 0) : 'Starting…'}
      </p>
      <p className="quiet">
        spotted so far, by name. This only reads the list of what is in each folder. No file is opened, and nothing is
        added or changed.
      </p>
      <LiveMap name={props.name} top={progress?.top} />
      {progress?.sizing && (
        <p className="quiet">
          Every photo and video is found. Now measuring the folders Keevoro never looks through, such as system and app
          folders, so you can see what takes the room.
        </p>
      )}
      {progress && (
        <p className="quiet small doing" aria-live="off">
          Looked at {count(progress.files, 'file')} in {count(progress.folders, 'folder')} so far.
        </p>
      )}
      <p className="choices stop">
        <button disabled={stopping} onClick={props.onStop}>
          {stopping ? 'Stopping…' : progress?.sizing ? 'Show it now' : 'Stop and show what was found'}
        </button>
      </p>
    </div>
  )
}
