// Settings, Backing up by itself: when the open backup runs by itself,
// whether Keevoro says when one has finished, and what keeps it able to
// run: staying by the clock when the window is closed, and starting with
// the computer. The sentences that depend on the computer (where its icon
// is, whether this desktop shows one) come from the app.
//
// Run now, skipping one and the last few runs are on the backup screen,
// beside the backups themselves (schedule.tsx); this is where the schedule
// is set, with the same dialog.

import { useEffect, useState } from 'react'
import type { Backend, KeepRunningChange, KeepRunningView, ScheduleView, SetSchedule } from '../backend'
import { ScheduleDialog } from '../schedule'
import { Group, Row, RowButton, RowSwitch } from './parts'

export function BackingUpSection(props: { api: Backend; name: string; onProblem: (error: unknown) => void }) {
  const { api, onProblem } = props
  const [schedule, setSchedule] = useState<ScheduleView | null>(null)
  const [choosing, setChoosing] = useState(false)
  const [view, setView] = useState<KeepRunningView | null>(null)
  useEffect(() => {
    api.schedule().then(setSchedule).catch(onProblem)
    api.keepRunning().then(setView).catch(onProblem)
  }, [api, onProblem])

  const change = (asked: KeepRunningChange) => api.setKeepRunning(asked).then(setView).catch(onProblem)
  const save = (request: SetSchedule) =>
    api
      .setSchedule(request)
      .then((now) => {
        setSchedule(now)
        setChoosing(false)
      })
      .catch(onProblem)
  const noIcon = view?.tray === 'none'

  return (
    <>
      {schedule && (
        <Group
          title={`Schedule for “${props.name}”`}
          footnote="Run now, skipping the next one and the last few runs are on the backup screen."
        >
          <Row
            id="schedule"
            about={
              schedule.unreadable
                ? 'This backup has a schedule this version of Keevoro can’t read, so it doesn’t back up by itself.'
                : 'Looks through your sources for anything new and backs it up, while Keevoro is open.'
            }
            value={schedule.every ? `${schedule.every}${schedule.paused ? ' (paused)' : ''}` : 'Off'}
            control={
              <RowButton onClick={() => setChoosing(true)}>{schedule.every ? 'Change…' : 'Set it up…'}</RowButton>
            }
            below={
              choosing && (
                <ScheduleDialog name={props.name} view={schedule} onSave={save} onCancel={() => setChoosing(false)} />
              )
            }
          />
        </Group>
      )}

      {view && (
        <>
          <Group title="Notifications">
            <Row
              id="notify"
              about="Only when something was backed up, something waits for your say, or a backup could not be done; never while Keevoro’s window is in front of you."
              control={<RowSwitch on={view.notify} onChange={(on) => change({ notify: on })} />}
            />
          </Group>

          <Group title="When the window is closed" footnote={view.said}>
            <Row
              id="keepRunning"
              about={`So backups by themselves happen on time. ${view.whereTheIconIs}`}
              control={<RowSwitch on={view.keepRunning} disabled={noIcon} onChange={(on) => change({ keepRunning: on })} />}
            />
            <Row
              id="minimise"
              about="Minimising then hides the window there, instead of leaving it on the taskbar."
              control={
                <RowSwitch on={view.minimiseToTray} disabled={noIcon} onChange={(on) => change({ minimiseToTray: on })} />
              }
            />
            <Row
              id="startWithComputer"
              about="Quietly, as an icon by the clock with no window, so backups by themselves happen without you opening it. Your computer lists it among the programs it starts, as Keevoro."
              control={
                <RowSwitch on={view.startWithComputer} onChange={(on) => change({ startWithComputer: on })} />
              }
            />
          </Group>
        </>
      )}
    </>
  )
}
