// Backing up by itself: the card on the backup screen and the dialog that
// sets it up.
//
// Every word about when it runs comes from Keevoro ("Every day at 02:30",
// "Tonight at 02:30"), the same the command line says, so the screen never
// works a time out for itself. Saving never runs a backup at once; "Run
// now" does, as the schedule would. Keevoro runs it while the app is open.

import { useCallback, useEffect, useState } from 'react'
import type { Backend, ScheduleAttempt, ScheduleView, SetSchedule } from './backend'
import { Explained } from './explained'
import { InfoTip, MoreAbout } from './infoTip'

/** The times offered: every quarter of an hour, as a person reads them. */
export const TIMES = Array.from({ length: 96 }, (_, i) => `${String(Math.floor(i / 4)).padStart(2, '0')}:${String((i % 4) * 15).padStart(2, '0')}`)

const WEEKDAYS: [string, string][] = [
  ['mon', 'Monday'],
  ['tue', 'Tuesday'],
  ['wed', 'Wednesday'],
  ['thu', 'Thursday'],
  ['fri', 'Friday'],
  ['sat', 'Saturday'],
  ['sun', 'Sunday'],
]

const ordinal = (n: number) => {
  const tens = n % 100
  const suffix = tens >= 11 && tens <= 13 ? 'th' : n % 10 === 1 ? 'st' : n % 10 === 2 ? 'nd' : n % 10 === 3 ? 'rd' : 'th'
  return `${n}${suffix}`
}

/** When a run was, as "Today, 02:31" or "18 Nov, 02:30". */
const whenRan = (seconds: number) => {
  const then = new Date(seconds * 1000)
  const time = then.toLocaleTimeString('en-GB', { hour: '2-digit', minute: '2-digit' })
  const today = new Date()
  const yesterday = new Date(today.getTime() - 86_400_000)
  if (then.toDateString() === today.toDateString()) return `Today, ${time}`
  if (then.toDateString() === yesterday.toDateString()) return `Yesterday, ${time}`
  return `${then.toLocaleDateString('en-GB', { day: 'numeric', month: 'short' })}, ${time}`
}

/** A run's mark: words, never colour alone. */
const MARKS: Record<ScheduleAttempt['outcome'], string> = {
  worked: 'Done',
  someFiles: 'Some files not sent',
  unreachable: 'Couldn’t reach it',
  refused: 'Couldn’t back up',
  stopped: 'Stopped',
  heldBack: 'Waiting for your say',
  notNow: 'Not run then',
  skipped: 'Skipped',
}

export function ScheduleCard(props: {
  api: Backend
  /** The backup's name, for the dialog's question. */
  name: string
  /** True while a backup is running, so Run now waits. */
  running: boolean
  onProblem: (error: unknown) => void
}) {
  const { api, onProblem } = props
  const [view, setView] = useState<ScheduleView | null>(null)
  const [choosing, setChoosing] = useState(false)
  const read = useCallback(() => {
    api.schedule().then(setView).catch(onProblem)
  }, [api, onProblem])
  useEffect(read, [read])
  // A run by itself is heard as a backup; when one ends, its line appears.
  useEffect(
    () =>
      api.onSend((event) => {
        if (event.finished) read()
      }),
    [api, read],
  )
  if (!view) return null
  // While a backup is running the screen is about that backup: an offer to
  // set a schedule can wait until it is done. A schedule already set stays.
  if (props.running && !view.every) return null
  const change = (request: SetSchedule) => {
    api
      .setSchedule(request)
      .then((now) => {
        setView(now)
        setChoosing(false)
      })
      .catch(onProblem)
  }
  return (
    <section className="schedule-card" aria-labelledby="schedule-title">
      <h3 id="schedule-title">Backing up by itself</h3>
      {view.every ? (
        <>
          <p className="schedule-line">
            <b>{view.every}</b>
            {view.paused && <span className="quiet"> (paused)</span>}
            <span className="actions">
              <button className="button small" onClick={() => setChoosing(true)}>
                Change
              </button>
              <button className="button small" onClick={() => change({ paused: !view.paused })}>
                {view.paused ? 'Carry on' : 'Pause'}
              </button>
            </span>
          </p>
          {view.waiting && !view.paused && (
            <Explained
              className="schedule-waiting"
              action={
                <button
                  className="button small"
                  disabled={props.running}
                  onClick={() => api.runScheduledNow(true).then(read).catch(onProblem)}
                >
                  Back up now anyway
                </button>
              }
            >
              {view.waiting}. The run due will start by itself once it can.
            </Explained>
          )}
          {view.next && !view.paused && (
            <p className="schedule-line">
              <span>
                Next: {view.next.charAt(0).toLowerCase() + view.next.slice(1)}
                {view.skipping && <span className="quiet">, which will be skipped, as you asked</span>}
              </span>
              <span className="actions">
                <button
                  className="button small"
                  onClick={() => api.skipNextRun(view.skipping).then(setView).catch(onProblem)}
                >
                  {view.skipping ? 'Don’t skip it' : 'Skip the next one'}
                </button>
              </span>
            </p>
          )}
        </>
      ) : (
        <p className="schedule-line">
          <span className="quiet">
            {view.unreadable
              ? 'This backup has a schedule this version of Keevoro can’t read, so it doesn’t back up by itself.'
              : 'Every day, week or month, while Keevoro is open.'}
          </span>
          <span className="actions">
            <button className="button small" onClick={() => setChoosing(true)}>
              Set it up…
            </button>
          </span>
        </p>
      )}
      {view.last.length > 0 && (
        <>
          <h4>Last few</h4>
          <ul className="schedule-last">
            {view.last.slice(0, 5).map((run) => (
              <li key={`${run.at}-${run.outcome}`} className={`ran ${run.outcome}`}>
                <span className="mark">{MARKS[run.outcome]}</span>
                <span className="when">{whenRan(run.at)}</span>
                <span className="said">{run.said}</span>
              </li>
            ))}
          </ul>
        </>
      )}
      {view.every && (
        <p className="schedule-line run-now">
          <span className="quiet small">
            Backs up anything new, as the schedule does.
            <InfoTip about="running the schedule now">
              Run now looks through your sources again and backs up what is new, as the schedule does.
            </InfoTip>
          </span>
          <span className="actions">
            <button
              className="button small"
              disabled={props.running}
              onClick={() => api.runScheduledNow().catch(onProblem)}
            >
              Run now
            </button>
          </span>
        </p>
      )}
      {choosing && (
        <ScheduleDialog name={props.name} view={view} onSave={change} onCancel={() => setChoosing(false)} />
      )}
    </section>
  )
}

/** Sets when a backup backs up by itself, or stops it: from the card here, and from Settings. */
export function ScheduleDialog(props: {
  name: string
  view: ScheduleView
  onSave: (request: SetSchedule) => void
  onCancel: () => void
}) {
  const chosen = props.view.choice
  const [every, setEvery] = useState<'off' | 'day' | 'week' | 'month'>(chosen?.every ?? 'day')
  const [dayAt, setDayAt] = useState(chosen?.every === 'day' ? chosen.at : '02:30')
  const [weekAt, setWeekAt] = useState(chosen?.every === 'week' ? chosen.at : '09:00')
  const [monthAt, setMonthAt] = useState(chosen?.every === 'month' ? chosen.at : '09:00')
  const [weekday, setWeekday] = useState(chosen?.weekday ?? 'sun')
  const [day, setDay] = useState(chosen?.day ?? 1)
  const save = () => {
    if (every === 'off') props.onSave({ every: 'off' })
    else if (every === 'day') props.onSave({ every, at: dayAt })
    else if (every === 'week') props.onSave({ every, at: weekAt, weekday })
    else props.onSave({ every, at: monthAt, day })
  }
  const times = (value: string, set: (v: string) => void, label: string) => (
    <select aria-label={label} value={value} onChange={(e) => set(e.target.value)}>
      {(TIMES.includes(value) ? TIMES : [value, ...TIMES]).map((t) => (
        <option key={t}>{t}</option>
      ))}
    </select>
  )
  return (
    <div className="panel schedule-dialog" role="dialog" aria-labelledby="schedule-dialog-title">
      <h2 id="schedule-dialog-title">Back up “{props.name}” by itself?</h2>
      <fieldset>
        <legend className="quiet small">How often</legend>
        <label>
          <input type="radio" name="every" checked={every === 'off'} onChange={() => setEvery('off')} /> Only when I
          press Back up
        </label>
        <label>
          <input type="radio" name="every" checked={every === 'day'} onChange={() => setEvery('day')} /> Every day at{' '}
          {times(dayAt, setDayAt, 'Time each day')}
        </label>
        <label>
          <input type="radio" name="every" checked={every === 'week'} onChange={() => setEvery('week')} /> Every week on{' '}
          <select aria-label="Day of the week" value={weekday} onChange={(e) => setWeekday(e.target.value)}>
            {WEEKDAYS.map(([short, long]) => (
              <option key={short} value={short}>
                {long}
              </option>
            ))}
          </select>{' '}
          at {times(weekAt, setWeekAt, 'Time each week')}
        </label>
        <label>
          <input type="radio" name="every" checked={every === 'month'} onChange={() => setEvery('month')} /> Every month
          on the{' '}
          <select aria-label="Day of the month" value={day} onChange={(e) => setDay(Number(e.target.value))}>
            {Array.from({ length: 31 }, (_, i) => i + 1).map((n) => (
              <option key={n} value={n}>
                {ordinal(n)}
              </option>
            ))}
          </select>{' '}
          at {times(monthAt, setMonthAt, 'Time each month')}
        </label>
      </fieldset>
      {every === 'month' && day > 28 && (
        <p className="quiet small">In a shorter month it runs on the last day.</p>
      )}
      <p>Each time, Keevoro backs up anything new from your sources.</p>
      <MoreAbout>
        <p>
          Each time, Keevoro looks through your sources for anything new and backs that up. Pictures it sets aside, and
          anything else that needs your say, wait for you. It waits while this computer is on battery or busy, and never
          runs two at once.
        </p>
        <p>
          It runs while Keevoro is open, in this computer’s own time. If the computer is off or asleep when a run is due,
          it catches up once when Keevoro next opens. Saving does not start a backup.
        </p>
      </MoreAbout>
      <p className="choices">
        <button onClick={props.onCancel}>Cancel</button>
        <button className="primary" onClick={save}>
          Save
        </button>
      </p>
    </div>
  )
}

/** How often the window says it is being used, at most. */
const SAY_USED_EVERY = 60_000

/** Tells Keevoro, at most once a minute, that the person is using the window, so a backup by itself waits until they have finished. */
export function useNoteWindowUse(api: Backend | null) {
  useEffect(() => {
    if (!api) return
    let said = 0
    const used = () => {
      const now = Date.now()
      if (now - said < SAY_USED_EVERY) return
      said = now
      api.windowUsed().catch(() => {})
    }
    window.addEventListener('pointerdown', used)
    window.addEventListener('keydown', used)
    return () => {
      window.removeEventListener('pointerdown', used)
      window.removeEventListener('keydown', used)
    }
  }, [api])
}
