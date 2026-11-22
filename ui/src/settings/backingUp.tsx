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
import type { Backend, KeepRunningChange, KeepRunningView, MannersChange, MannersView, ScheduleView, SetSchedule } from '../backend'
import { ScheduleDialog, TIMES } from '../schedule'
import { Group, Row, RowButton, RowSwitch } from './parts'

export function BackingUpSection(props: {
  api: Backend
  /** The open backup's name; none before the first backup is started, when there is no schedule to set. */
  name: string | null
  onProblem: (error: unknown) => void
}) {
  const { api, onProblem } = props
  const [schedule, setSchedule] = useState<ScheduleView | null>(null)
  const [choosing, setChoosing] = useState(false)
  const [view, setView] = useState<KeepRunningView | null>(null)
  const [manners, setManners] = useState<MannersView | null>(null)
  const name = props.name
  useEffect(() => {
    if (name !== null) api.schedule().then(setSchedule).catch(onProblem)
    api.keepRunning().then(setView).catch(onProblem)
    api.backgroundSettings().then(setManners).catch(onProblem)
  }, [api, name, onProblem])
  const mind = (asked: MannersChange) => api.backgroundSettings(asked).then(setManners).catch(onProblem)

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
      {schedule && name !== null && (
        <Group
          title={`Schedule for “${name}”`}
          tip="Run now, skipping the next one and the last few runs are on the backup screen."
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
                <ScheduleDialog name={name} view={schedule} onSave={save} onCancel={() => setChoosing(false)} />
              )
            }
          />
        </Group>
      )}

      {manners && (
        <Group
          title="Waiting for a good moment"
          footnote="Only a backup by itself waits. One you start yourself, or Back up now anyway, never does."
        >
          <Row
            id="waitOnBattery"
            about="Until it is plugged in, when under half charged."
            control={<RowSwitch on={manners.waitOnBattery} onChange={(on) => mind({ waitOnBattery: on })} />}
          />
          <Row
            id="waitOnMetered"
            about={
              manners.meteredKnown
                ? 'Such as a phone’s hotspot, for cloud storage.'
                : 'This computer can’t tell, so this is off.'
            }
            control={
              <RowSwitch
                on={manners.meteredKnown && manners.waitOnMetered}
                disabled={!manners.meteredKnown}
                onChange={(on) => mind({ waitOnMetered: on })}
              />
            }
          />
          <Row
            id="between"
            about={
              manners.between ? `Starts only ${manners.betweenSaid}.` : 'Starts at any hour.'
            }
            control={
              <RowSwitch
                on={!!manners.between}
                onChange={(on) => mind(on ? { between: { from: '22:00', to: '07:00' } } : { anyHour: true })}
              />
            }
            below={
              manners.between && (
                <div className="between-times">
                  <label>
                    From{' '}
                    <select
                      value={manners.between.from}
                      onChange={(e) => mind({ between: { from: e.target.value, to: manners.between?.to ?? '07:00' } })}
                    >
                      {TIMES.map((t) => (
                        <option key={t}>{t}</option>
                      ))}
                    </select>
                  </label>
                  <label>
                    until{' '}
                    <select
                      value={manners.between.to}
                      onChange={(e) => mind({ between: { from: manners.between?.from ?? '22:00', to: e.target.value } })}
                    >
                      {TIMES.map((t) => (
                        <option key={t}>{t}</option>
                      ))}
                    </select>
                  </label>
                </div>
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
              about="When something was backed up, needs your say or could not be done. Never while you are using Keevoro."
              control={<RowSwitch on={view.notify} onChange={(on) => change({ notify: on })} />}
            />
          </Group>

          <Group title="When the window is closed" footnote={view.said}>
            <Row
              id="keepRunning"
              about="So backups by themselves happen on time."
              tip={view.whereTheIconIs}
              control={<RowSwitch on={view.keepRunning} disabled={noIcon} onChange={(on) => change({ keepRunning: on })} />}
            />
            <Row
              id="minimise"
              tip="Minimising then hides the window there, instead of leaving it on the taskbar."
              control={
                <RowSwitch on={view.minimiseToTray} disabled={noIcon} onChange={(on) => change({ minimiseToTray: on })} />
              }
            />
            <Row
              id="startWithComputer"
              about="Quietly, as an icon by the clock."
              tip="With no window, so backups by themselves happen without you opening it. Your computer lists it among the programs it starts, as Keevoro."
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
