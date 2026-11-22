// Settings, Advanced: what most people never need, and can safely leave
// as it is. The kinds of file Keevoro looks for and what AI agents may do
// each open a page of their own; how photos are opened, clearing a
// stopped backup's unfinished copies from a folder, and forgetting the
// remembered maps are a row each.
//
// A map lists folder names across a whole drive, so a person may want it
// gone: forgetting touches nothing on any drive, and a place is simply
// surveyed again when next asked about. Reset forgets them too.

import { useEffect, useState } from 'react'
import type { Acceleration, AgentAccess, Backend } from '../backend'
import { count } from '../format'
import { ClearUnfinished } from '../tidy'
import { Group, Intro, OpenRow, Row, RowButton, RowSwitch } from './parts'
import type { DetailId } from './places'

export function AdvancedSection(props: { api: Backend; onOpen: (detail: DetailId) => void; onProblem: (error: unknown) => void }) {
  const { api, onProblem } = props
  const [speed, setSpeed] = useState<Acceleration | null>(null)
  const [agents, setAgents] = useState<AgentAccess | null>(null)
  const [forgot, setForgot] = useState<string | null>(null)
  useEffect(() => {
    api.acceleration().then(setSpeed).catch(onProblem)
    api.agentAccess().then(setAgents).catch(onProblem)
  }, [api, onProblem])

  const forget = () =>
    api
      .forgetMaps()
      .then(({ forgotten }) =>
        setForgot(forgotten === 0 ? 'There were no remembered maps.' : `Forgot ${count(forgotten, 'remembered map')}.`),
      )
      .catch(onProblem)

  return (
    <>
      <Intro>Most people never need anything here. Keevoro works well if you leave all of it as it is.</Intro>

      <Group>
        <OpenRow
          id="kinds"
          about="The endings Keevoro looks for, such as .jpg, and any of your own."
          onOpen={() => props.onOpen('kinds')}
        />
        <OpenRow
          id="agents"
          about="What an AI app, such as Claude, may do with Keevoro for you."
          value={agents ? (agents.mayChange ? 'May make changes' : 'Looking only') : undefined}
          onOpen={() => props.onOpen('agents')}
        />
      </Group>

      {speed && (
        <Group
          title="Opening photos"
          footnote={`Photos are opened by: ${speed.ways.join(', then ')}. Previews are made ${speed.cores} at a time, on half of this computer’s processor cores at most, so the rest stays free for you.`}
        >
          <Row
            id="codecs"
            about={
              speed.available
                ? 'Usually the fastest way to open a photo, using your graphics hardware where it can, and the only way for some kinds, such as HEIC. A photo that will not open this way is opened by Keevoro’s own decoder instead. Switch this off only if previews look wrong.'
                : 'On this kind of computer Keevoro uses its own decoder for photos, so there is nothing to switch.'
            }
            control={
              <RowSwitch
                on={speed.on && speed.available}
                disabled={!speed.available}
                onChange={(on) => api.setAcceleration(on).then(setSpeed).catch(onProblem)}
              />
            }
          />
        </Group>
      )}

      <Group title="Tidying up">
        <Row
          id="unfinished"
          className="kv-row-may-ask"
          about="A backup stopped part-way can leave them in a folder it was sending to. Each backup clears its own; this is for a folder no backup goes to any more. Keevoro says what it finds first, and touches nothing that was backed up."
          control={<ClearUnfinished api={api} label="Choose a folder…" small onProblem={onProblem} />}
        />
        <Row
          id="maps"
          about="What Keevoro saw while finding your photos, the names of folders and how much each holds, so looking inside opens at once. Kept only on this computer; forgetting touches nothing on any drive."
          status={forgot}
          control={<RowButton onClick={forget}>Forget them</RowButton>}
        />
      </Group>
    </>
  )
}
