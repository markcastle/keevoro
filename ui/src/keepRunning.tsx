// Settings, "When you close Keevoro": whether closing the window keeps
// Keevoro running by the clock, whether minimising hides it too, and
// whether it starts with the computer. The sentences that depend on the
// computer (where its icon is, whether this desktop shows one) come from
// the app.

import { useEffect, useState } from 'react'
import type { Backend, KeepRunningChange, KeepRunningView } from './backend'

function Switch(props: { on: boolean; label: string; said: string; disabled?: boolean; onChange: (on: boolean) => void }) {
  return (
    <label className="switch-row">
      <button
        className={props.on ? 'kv-switch on' : 'kv-switch'}
        role="switch"
        aria-checked={props.on}
        aria-label={props.label}
        disabled={props.disabled}
        onClick={() => props.onChange(!props.on)}
      >
        <span className="knob" />
      </button>
      <span>
        <b>{props.label}</b>
        <span className="quiet small">{props.said}</span>
      </span>
    </label>
  )
}

export function KeepRunningSetting(props: { api: Backend; onProblem: (error: unknown) => void }) {
  const { api, onProblem } = props
  const [view, setView] = useState<KeepRunningView | null>(null)
  useEffect(() => {
    api.keepRunning().then(setView).catch(onProblem)
  }, [api, onProblem])
  if (!view) return null
  const change = (asked: KeepRunningChange) => api.setKeepRunning(asked).then(setView).catch(onProblem)
  const noIcon = view.tray === 'none'
  return (
    <div className="setting keep-running">
      <h3>When you close Keevoro</h3>
      <Switch
        on={view.keepRunning}
        label="Keep Keevoro running by the clock when its window is closed"
        said={`So backups that run by themselves happen on time. ${view.whereTheIconIs}`}
        disabled={noIcon}
        onChange={(on) => change({ keepRunning: on })}
      />
      <Switch
        on={view.minimiseToTray}
        label="Minimise to the tray as well"
        said="Minimising then hides the window to Keevoro’s icon by the clock, instead of leaving it on the taskbar."
        disabled={noIcon}
        onChange={(on) => change({ minimiseToTray: on })}
      />
      <Switch
        on={view.startWithComputer}
        label="Start Keevoro when this computer starts"
        said="It starts quietly, as an icon by the clock with no window, so backups by themselves happen without you opening it. It is listed with the programs your computer starts, under the name Keevoro."
        onChange={(on) => change({ startWithComputer: on })}
      />
      {view.said && <p className="quiet small">{view.said}</p>}
    </div>
  )
}
