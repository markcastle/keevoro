// Settings, This backup: the backup that is open, what it backs up, how
// its files are arranged, and the way to every other backup. Each choice
// here is the open backup's own; another backup has its own.

import { useEffect, useState } from 'react'
import type { AppState, ArrangeView, Backend } from '../backend'
import { BackupArrange } from '../arrange'
import { BackupWants } from '../wants'
import { Group, OpenRow, Row, RowButton, RowSwitch, Said } from './parts'

export function ThisBackupSection(props: {
  api: Backend
  state: AppState
  sendingTo: string | null
  /** Bumped when the kinds of file change, so what the backup is for is asked again. */
  kindsChanged: number
  onLibrary: (state: AppState) => void
  onArranged: (view: ArrangeView) => void
  /** Goes to the step that chooses where it backs up to. */
  onSend: () => void
  onBackups: () => void
  onProblem: (error: unknown) => void
}) {
  const { api, state, onProblem } = props
  // Whether this backup also makes previews, or sends the originals alone.
  const [previews, setPreviews] = useState<boolean | null>(null)
  // What a change to what it backs up, or to how it is arranged, means, said under its panel.
  const [wantsSaid, setWantsSaid] = useState('')
  const [arrangeSaid, setArrangeSaid] = useState('')
  useEffect(() => {
    api.previewsWanted().then(setPreviews).catch(onProblem)
  }, [api, onProblem])

  return (
    <>
      <Group>
        <Row id="name" value={state.backup?.name} />
        <Row
          id="destination"
          about={props.sendingTo ? `It backs up to ${props.sendingTo}.` : 'Nowhere has been chosen yet.'}
          control={<RowButton onClick={props.onSend}>{props.sendingTo ? 'Change where' : 'Choose where'}</RowButton>}
        />
        {state.library && (
          <Row
            id="notes"
            about={<span className="where">{state.library}</span>}
            tip="On this computer. Your photos are not copied there; they go only where it backs up to."
            control={<RowButton onClick={() => api.openPlace(state.library ?? '').catch(onProblem)}>Show</RowButton>}
          />
        )}
      </Group>

      <Group
        title="What it backs up"
        className="kv-wants"
        tip="A source can have its own choice, and its own way of arranging, under File types beside it on the first step."
        footnote={<Said>{wantsSaid}</Said>}
      >
        <div id="setting-wants">
          <BackupWants
            key={props.kindsChanged}
            load={() => api.backupWants()}
            choose={(wants) => api.setBackupWants(wants)}
            onChanged={() => props.onLibrary(state)}
            onSaid={setWantsSaid}
            onProblem={onProblem}
          />
        </div>
        {previews !== null && (
          <Row
            id="previews"
            about="Small copies for looking through. Off is quicker."
            tip="Small and screen-sized versions of each photo and a still of each video, made gently while your files are backed up and sent once they are safe. Off, only the originals are backed up, which is quicker; switched on again, they are added on the next backup."
            control={
              <RowSwitch
                on={previews}
                onChange={(wanted) => {
                  setPreviews(wanted)
                  api.setPreviewsWanted(wanted).catch(onProblem)
                }}
              />
            }
          />
        )}
      </Group>

      <Group
        title="How files are arranged"
        className="kv-arrange"
        tip="Changing this only changes where files go from now on. Files already backed up stay where they are."
        footnote={<Said>{arrangeSaid}</Said>}
      >
        <div id="setting-arrange">
          <BackupArrange
            load={() => api.backupArrange()}
            choose={(arrange) => api.setBackupArrange(arrange)}
            onChanged={(view) => {
              props.onArranged(view)
              props.onLibrary(state)
            }}
            onSaid={setArrangeSaid}
            onProblem={onProblem}
          />
        </div>
      </Group>

      <Group>
        <OpenRow id="backups" about="Rename, open, remove or put one back." onOpen={props.onBackups} />
      </Group>
    </>
  )
}
