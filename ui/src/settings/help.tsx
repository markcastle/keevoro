// Settings, Help: telling Keevoro's maker about a problem or an idea, what
// Keevoro has been doing, the open source work it is
// built on, and, set apart at the bottom, putting Keevoro back as it was
// when first installed. Resetting asks first, in the page, and says
// exactly what it does and does not do.

import { useState } from 'react'
import type { AppState, Backend } from '../backend'
import { chooseTheme } from '../theme'
import { Group, OpenRow, Row, RowButton } from './parts'
import type { DetailId } from './places'

export function HelpSection(props: {
  api: Backend
  state: AppState
  onOpen: (detail: DetailId) => void
  /** Called after a reset, with the app as it is when first installed. */
  onReset: (state: AppState) => void
  onProblem: (error: unknown) => void
}) {
  const { api, onProblem } = props
  const [resetting, setResetting] = useState(false)
  const backups = props.state.backups.length
  return (
    <>
      <Group>
        <OpenRow
          id="report"
          about="Tell Keevoro’s maker in your own words, and see how your reports are getting on."
          onOpen={() => props.onOpen('report')}
        />
        <OpenRow
          id="log"
          about="Keevoro’s plain record of what it did and when."
          onOpen={() => props.onOpen('log')}
        />
        <OpenRow
          id="licences"
          about="The open source work Keevoro is built on, each with its licence."
          onOpen={() => props.onOpen('licences')}
        />
      </Group>

      <Group title="Start again" danger className="kv-danger-zone">
        <Row
          id="reset"
          about="Puts Keevoro back as it was when you first installed it: to clear out a trial run, or before passing the computer on."
          control={
            !resetting && (
              <RowButton danger onClick={() => setResetting(true)}>
                Reset Keevoro…
              </RowButton>
            )
          }
          below={
            resetting && (
              <div className="resetting" role="alertdialog" aria-labelledby="reset-title">
                <h4 id="reset-title">Reset Keevoro?</h4>
                <p>
                  <b>This removes:</b> {backups === 1 ? 'your backup' : `all ${backups} of your backups`} from Keevoro,
                  with what was found and what you chose to keep; the previews and maps Keevoro made; any storage
                  keys it saved; and its list of reports you sent.
                </p>
                <p>
                  <b>This does not touch:</b> anything on your sources, or any photo or video you have already sent
                  anywhere. Those stay exactly where they are. Keevoro simply forgets about them.
                </p>
                <p className="choices">
                  <button
                    type="button"
                    className="danger"
                    onClick={() =>
                      api
                        .resetEverything()
                        .then((s) => {
                          chooseTheme('system')
                          props.onReset(s)
                        })
                        .catch((e) => {
                          setResetting(false)
                          onProblem(e)
                        })
                    }
                  >
                    Yes, reset Keevoro
                  </button>
                  <button type="button" className="button" onClick={() => setResetting(false)}>
                    Cancel
                  </button>
                </p>
              </div>
            )
          }
        />
      </Group>
    </>
  )
}
