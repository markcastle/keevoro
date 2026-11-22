// Settings, Updates: which Keevoro this is, and a way to ask now whether
// there is a newer one. Keevoro also asks by itself each time it opens;
// an offer is made in the bar at the top of the window, never here alone.
// Help, About Keevoro, shows the very same row.

import { useState } from 'react'
import type { Backend, Update } from '../backend'
import { Group, Row, RowButton } from './parts'

type VersionProps = {
  api: Backend
  version: string
  update: Update | null
  onUpdate: (update: Update | null) => void
  onProblem: (error: unknown) => void
}

export function UpdatesSection(props: VersionProps) {
  return (
    <Group>
      <VersionRow id="version" {...props} />
    </Group>
  )
}

/** Which Keevoro this is, with Check for updates: in Updates, and in About under its own name. */
export function VersionRow(props: VersionProps & { id: 'version' | 'aboutVersion' }) {
  const { api, onProblem } = props
  const [checked, setChecked] = useState<'no' | 'checking' | 'current'>('no')
  return (
    <Row
      id={props.id}
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
  )
}
