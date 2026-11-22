// "Not sure where to buy cloud storage?": one quiet line on "Where should
// they go?" that opens Stratus, by Adam Sparshott, which compares what
// cloud storage costs, inside a sheet. The same sheet opens from the
// backup screen's More menu and from Help, Compare cloud storage providers.
//
// Nothing is loaded until the person opens it. Then the page is loaded in
// a frame that may run its own scripts but cannot open windows, move the
// app away from itself or reach Keevoro's commands, and is told only the
// size of this backup and a currency. What it says back is checked in
// ./stratus.ts and only ever shown as text. A choice Keevoro can back up to
// opens Keevoro's own form for it; prices stay Stratus's.

import { useEffect, useRef, useState } from 'react'
import type { Backend } from './backend'
import { Explained } from './explained'
import { STRATUS_EMBED, STRATUS_ORIGIN, STRATUS_PAGE, readMessage, setMessage, setUpFor, gigabytesToAsk } from './stratus'
import type { Choice, SetUp } from './stratus'

/** How long the page has to say it is ready before Keevoro says it couldn't load it. */
const WAIT_FOR_IT_MS = 8000

/** The quiet line, and the sheet it opens. */
export function CompareStorage(props: {
  api: Backend
  /** What is being kept, in bytes: the size the comparison is set to. */
  keptBytes: number
  /** Opens Keevoro's own form for a kind of storage. */
  onSetUp: (setUp: SetUp) => void
  onProblem: (error: unknown) => void
}) {
  const [open, setOpen] = useState(false)
  return (
    <>
      <p className="quiet small compare-line">
        Not sure where to buy cloud storage?{' '}
        <button className="link" onClick={() => setOpen(true)}>
          Compare providers
        </button>
      </p>
      {open && (
        <CompareSheet
          api={props.api}
          keptBytes={props.keptBytes}
          onClose={() => setOpen(false)}
          onSetUp={(setUp) => {
            setOpen(false)
            props.onSetUp(setUp)
          }}
          onProblem={props.onProblem}
        />
      )}
    </>
  )
}

type Loading = 'waiting' | 'ready' | 'failed'

/**
 * The sheet itself. Without `onSetUp` (no backup has been started yet), a
 * choice Keevoro can back up to is named, with where to set it up later.
 */
export function CompareSheet(props: {
  api: Backend
  keptBytes: number
  onClose: () => void
  onSetUp?: (setUp: SetUp) => void
  onProblem: (error: unknown) => void
}) {
  const { api, keptBytes, onClose, onProblem } = props
  const frame = useRef<HTMLIFrameElement>(null)
  const [loading, setLoading] = useState<Loading>('waiting')
  const [choice, setChoice] = useState<Choice | null>(null)
  const sheet = useRef<HTMLDivElement>(null)

  useEffect(() => {
    // Heard only from the frame Keevoro opened, from Stratus's own address.
    const heard = (event: MessageEvent) => {
      if (!frame.current || event.source !== frame.current.contentWindow) return
      const message = readMessage(event.origin, event.data)
      if (!message) return
      if (message.type === 'ready') {
        setLoading('ready')
        // The size and a currency, and nothing else, only to Stratus.
        frame.current.contentWindow?.postMessage(setMessage(keptBytes, navigator.language), STRATUS_ORIGIN)
      } else {
        setChoice(message.choice)
      }
    }
    window.addEventListener('message', heard)
    const late = window.setTimeout(() => setLoading((now) => (now === 'waiting' ? 'failed' : now)), WAIT_FOR_IT_MS)
    return () => {
      window.removeEventListener('message', heard)
      window.clearTimeout(late)
    }
  }, [keptBytes])

  // The latest way to close, so the sheet takes the focus once, as it opens.
  const close = useRef(onClose)
  useEffect(() => {
    close.current = onClose
  })
  useEffect(() => {
    sheet.current?.focus()
    // Said as handled, so the page behind does not go back as well.
    const escape = (event: KeyboardEvent) => {
      if (event.key !== 'Escape') return
      event.preventDefault()
      close.current()
    }
    window.addEventListener('keydown', escape)
    return () => window.removeEventListener('keydown', escape)
  }, [])

  const openInBrowser = (page: string) => api.openWebPage(page).catch(onProblem)
  const setUp = choice ? setUpFor(choice) : null
  return (
    <div className="compare-sheet">
      <div className="panel" role="dialog" aria-modal="true" aria-labelledby="compare-title" tabIndex={-1} ref={sheet}>
        <header>
          <h2 id="compare-title">Compare cloud storage</h2>
          <button className="button small" onClick={onClose}>
            Close
          </button>
        </header>
        <p className="quiet small">
          This loads a comparison page from the internet. Keevoro tells it only the size of this backup, about{' '}
          {gigabytesToAsk(keptBytes).toLocaleString('en-GB')} GB, and your currency. The prices are Stratus’s, not
          Keevoro’s: check them with the provider before you buy.
        </p>
        {choice && (
          <div className="compare-choice" role="status">
            {setUp && props.onSetUp ? (
              <Explained
                action={
                  <button className="primary" onClick={() => props.onSetUp?.(setUp)}>
                    Set up {choice.providerName} in Keevoro
                  </button>
                }
              >
                You chose {choice.plan}. Once you have a bucket and its keys, Keevoro can back up there.
              </Explained>
            ) : setUp ? (
              <p>
                You chose {choice.plan}. Once you have a bucket and its keys, and have started a backup, Keevoro can
                back up there: choose it under “Where should they go?”.
              </p>
            ) : (
              <Explained
                action={
                  choice.url ? (
                    <button className="button" onClick={() => openInBrowser(choice.url ?? STRATUS_PAGE)}>
                      Open {choice.providerName}’s page
                    </button>
                  ) : null
                }
              >
                You chose {choice.plan}. Keevoro can’t back up to {choice.providerName} directly. If it puts a folder
                on this computer, back up to that folder instead.
              </Explained>
            )}
          </div>
        )}
        {loading === 'failed' ? (
          <div className="compare-failed" role="alert">
            <Explained
              action={
                <button className="button" onClick={() => openInBrowser(STRATUS_PAGE)}>
                  Open Stratus in your browser
                </button>
              }
            >
              Couldn’t load the comparison just now. It needs a connection to the internet.
            </Explained>
          </div>
        ) : (
          <>
          {loading === 'waiting' && <p className="quiet small">Loading the comparison…</p>}
          <iframe
            ref={frame}
            className={loading === 'ready' ? 'compare-frame' : 'compare-frame waiting'}
            src={STRATUS_EMBED}
            title="Cloud storage prices, from Stratus"
            // Its own scripts, at its own address, and nothing more: no
            // windows of its own, no moving the app away, no forms sent.
            sandbox="allow-scripts allow-same-origin"
            referrerPolicy="no-referrer"
            allow=""
          />
          </>
        )}
        <p className="quiet small compare-credit">
          Comparison by{' '}
          <button className="link" onClick={() => openInBrowser(STRATUS_PAGE)} title="Opens in your browser">
            Stratus
          </button>
          , made by Adam Sparshott.
        </p>
      </div>
    </div>
  )
}
