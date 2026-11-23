// Settings, Advanced, AI agents: what an AI agent may do through
// `keevoro mcp`. Nothing until the person says: no folders, and looking
// only. An agent can never change this itself.

import { useEffect, useState } from 'react'
import type { AgentAccess, Backend } from '../backend'
import { InfoTip } from '../infoTip'
import { Group, Intro, Row, RowSwitch } from './parts'

type List = 'lookIn' | 'backUpTo'

export function AgentsPage(props: { api: Backend; onProblem: (error: unknown) => void }) {
  const { api, onProblem } = props
  const [access, setAccess] = useState<AgentAccess | null>(null)
  useEffect(() => {
    api.agentAccess().then(setAccess).catch(onProblem)
  }, [api, onProblem])
  const change = (part: Partial<AgentAccess>) => api.setAgentAccess(part).then(setAccess).catch(onProblem)
  const add = (list: List) =>
    api
      .chooseAgentFolder()
      .then((folder) => {
        if (folder && access && !access[list].includes(folder)) change({ [list]: [...access[list], folder] })
      })
      .catch(onProblem)

  const folders = (list: List, row: string, title: string, none: string) =>
    access && (
      <Group title={title} className="agent-folders">
        <div id={`setting-${row}`}>
          {access[list].length === 0 ? (
            <p className="kv-row kv-row-about">{none}</p>
          ) : (
            <ul aria-label={title}>
              {access[list].map((folder) => (
                <li key={folder} className="kv-row">
                  <span className="kv-row-main">
                    <span className="kv-row-text">
                      <span className="where">{folder}</span>
                    </span>
                    <span className="kv-row-end">
                      <button
                        type="button"
                        className="button small"
                        aria-label={`Stop allowing ${folder}`}
                        onClick={() => change({ [list]: access[list].filter((f) => f !== folder) })}
                      >
                        Remove
                      </button>
                    </span>
                  </span>
                </li>
              ))}
            </ul>
          )}
        </div>
        <div className="kv-row kv-row-add">
          <button type="button" className="link" onClick={() => add(list)}>
            Add a folder…
          </button>
        </div>
      </Group>
    )

  return (
    <>
      <Intro>
        An AI app, such as Claude, can use Keevoro for you. It can never change anything on your sources.
        <InfoTip about="AI agents">
          Once it is set up to start <code>keevoro mcp</code>. Everything it does is written in the log.
        </InfoTip>
      </Intro>
      {access && (
        <>
          <Group>
            <Row
              id="agentChanges"
              about="Off, it can only look. On, it can back up, but only to the folders below. It can never delete a photo."
              control={<RowSwitch on={access.mayChange} onChange={(on) => change({ mayChange: on })} />}
            />
          </Group>
          {folders('lookIn', 'agentLookIn', 'Folders an agent may look through', 'None yet, so it cannot look for photos anywhere.')}
          {folders('backUpTo', 'agentBackUpTo', 'Folders an agent may back up to', 'None yet, so it cannot back up to a folder.')}
        </>
      )}
    </>
  )
}
