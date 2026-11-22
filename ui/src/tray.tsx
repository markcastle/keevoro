// What the window is told when it opens or comes forward from Keevoro's
// icon by the clock: a page to show, the explanation said the first time
// the window is closed, the question asked before quitting cuts work
// short, and anything the icon's menu could not do. The words that depend
// on the computer (where its icon is, what quitting would cut short) come
// from the app; each is shown over the window, so it cannot be missed.

import { useCallback, useEffect, useRef, useState } from 'react'
import type { ReactNode } from 'react'
import type { Backend, WindowAsk } from './backend'
import { Toggle } from './toggle'

/** Where the choices about closing are in Settings, as a page name (settings/places.ts). */
const SETTINGS_WHEN_CLOSED = 'settings/byItself/keepRunning'

/** Asks the app what waits for the window, as it opens and whenever it is told to, and shows it. */
export function WindowAsks(props: { api: Backend; onPage: (page: string) => void; onProblem: (error: unknown) => void }) {
  const { api, onProblem } = props
  const [asked, setAsked] = useState<WindowAsk | null>(null)
  // The latest page opener, so asking is not repeated each time the window draws.
  const onPage = useRef(props.onPage)
  useEffect(() => {
    onPage.current = props.onPage
  })
  const take = useCallback(
    () =>
      api
        .windowAsk()
        .then((ask) => {
          if (!ask) return
          if (ask.page) onPage.current(ask.page)
          if (ask.closing || ask.quitting || ask.note) setAsked((was) => ({ ...was, ...ask, page: undefined }))
        })
        .catch(() => {}),
    [api],
  )
  useEffect(() => {
    void take()
    return api.onWindowAsk(() => void take())
  }, [api, take])

  if (asked?.quitting) {
    return <QuitAsked api={api} question={asked.quitting} onDone={() => setAsked({ ...asked, quitting: undefined })} onProblem={onProblem} />
  }
  if (asked?.closing) {
    return (
      <CloseExplained
        api={api}
        whereTheIconIs={asked.closing}
        onQuitAsked={(question) => setAsked({ ...asked, closing: undefined, quitting: question })}
        onDone={() => setAsked({ ...asked, closing: undefined })}
        onSettings={() => {
          setAsked({ ...asked, closing: undefined })
          onPage.current(SETTINGS_WHEN_CLOSED)
        }}
        onProblem={onProblem}
      />
    )
  }
  if (asked?.note) {
    return (
      <Over labelledBy="tray-note">
        <p id="tray-note">{asked.note}</p>
        <p className="choices">
          <button className="primary" onClick={() => setAsked({ ...asked, note: undefined })}>
            OK
          </button>
        </p>
      </Over>
    )
  }
  return null
}

/** A small panel over the window. */
function Over(props: { labelledBy: string; children: ReactNode }) {
  return (
    <div className="window-ask">
      <div className="panel" role="dialog" aria-modal="true" aria-labelledby={props.labelledBy}>
        {props.children}
      </div>
    </div>
  )
}

/** Said once, the first time the window is closed: Keevoro keeps running by the clock. */
export function CloseExplained(props: {
  api: Backend
  whereTheIconIs: string
  onQuitAsked: (question: string) => void
  onDone: () => void
  /** Leaves the window open, on these choices in Settings. */
  onSettings: () => void
  onProblem: (error: unknown) => void
}) {
  const { api, onProblem } = props
  // Never ticked for the person: Keevoro starts with the computer only on their say.
  const [withComputer, setWithComputer] = useState(false)
  return (
    <Over labelledBy="close-explained-title">
      <h2 id="close-explained-title">Keevoro keeps running</h2>
      <p>It stays by the clock, so your backups happen on time.</p>
      <p>{props.whereTheIconIs}</p>
      <div className="ask-tick">
        <Toggle on={withComputer} label="Start Keevoro when this computer starts" onChange={setWithComputer} />
      </div>
      <p className="quiet small">
        You can change these in{' '}
        <button className="link" onClick={props.onSettings}>
          Settings, Backing up by itself
        </button>
        .
      </p>
      <p className="quiet small">Quit instead, and nothing backs up by itself until you open Keevoro again.</p>
      <p className="choices">
        <button
          onClick={() =>
            api
              .quitKeevoro(false)
              .then((answer) => answer.asked && props.onQuitAsked(answer.asked))
              .catch(onProblem)
          }
        >
          Quit Keevoro instead
        </button>
        <button
          className="primary"
          onClick={() =>
            api
              .hideToTray(withComputer)
              .then(props.onDone)
              .catch(onProblem)
          }
        >
          OK
        </button>
      </p>
    </Over>
  )
}

/** Asked before quitting cuts work in hand short. */
export function QuitAsked(props: { api: Backend; question: string; onDone: () => void; onProblem: (error: unknown) => void }) {
  const [stopping, setStopping] = useState(false)
  return (
    <Over labelledBy="quit-asked">
      <p id="quit-asked">{props.question}</p>
      <p className="choices">
        <button onClick={props.onDone} disabled={stopping}>
          Keep it running
        </button>
        <button
          className="primary"
          disabled={stopping}
          onClick={() => {
            setStopping(true)
            props.api
              .quitKeevoro(true)
              .then(props.onDone)
              .catch((e) => {
                setStopping(false)
                props.onProblem(e)
              })
          }}
        >
          {stopping ? 'Stopping…' : 'Stop it and quit'}
        </button>
      </p>
    </Over>
  )
}
