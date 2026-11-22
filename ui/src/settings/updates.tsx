// Settings, Updates: which Keevoro this is, and a way to ask now whether
// there is a newer one. Keevoro also asks by itself each time it opens;
// an offer is made in the bar at the top of the window, never here alone.

import { useState } from 'react'
import type { Backend, Update } from '../backend'
import { Group, Row, RowButton } from './parts'

export function UpdatesSection(props: {
  api: Backend
  version: string
  update: Update | null
  onUpdate: (update: Update | null) => void
  onProblem: (error: unknown) => void
}) {
  const { api, onProblem } = props
  const [checked, setChecked] = useState<'no' | 'checking' | 'current'>('no')
  return (
    <Group>
      <Row
        id="version"
        title={`Keevoro ${props.version}`}
        about={
          props.update
            ? `Version ${props.update.version} is ready; see the bar at the top.`
            : checked === 'current'
              ? 'That is the newest version.'
              : 'Keevoro looks for a newer version each time it opens.'
        }
        control={
          <RowButton
            disabled={checked === 'checking'}
            onClick={() => {
              setChecked('checking')
              api
                .checkUpdate()
                .then((found) => {
                  props.onUpdate(found)
                  setChecked(found ? 'no' : 'current')
                })
                .catch((e) => {
                  setChecked('no')
                  onProblem(e)
                })
            }}
          >
            {checked === 'checking' ? 'Checking…' : 'Check for updates'}
          </RowButton>
        }
      />
    </Group>
  )
}
