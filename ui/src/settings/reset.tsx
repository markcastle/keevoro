// Settings, Advanced, at its foot and on its own: putting Keevoro back as
// it was when first installed. Resetting asks first, in the page, and says
// exactly what it does and does not do.

import { useState } from 'react'
import type { AppState, Backend } from '../backend'
import { chooseTheme } from '../theme'
import { Group, Row, RowButton } from './parts'

export function ResetGroup(props: {
  api: Backend
  state: AppState
  /** Called after a reset, with the app as it is when first installed. */
  onReset: (state: AppState) => void
  onProblem: (error: unknown) => void
}) {
  const { api, onProblem } = props
  const [resetting, setResetting] = useState(false)
  const backups = props.state.backups.length
  return (
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
                with what was found and what you chose to keep; the previews and maps Keevoro made; any storage keys it
                saved; and its list of reports you sent.
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
  )
}
