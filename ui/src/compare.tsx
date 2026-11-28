// "Not sure where to buy cloud storage?": one quiet line on "Where should
// they go?" that opens a sheet comparing what cloud storage costs, from
// Stratus, by Adam Sparshott. The same sheet opens from the backup
// screen's More menu and from Help, Compare cloud storage providers.
//
// Nothing is asked until the person opens it. Then Keevoro itself (not
// this window) fetches Stratus's prices for one size and one currency, and
// they are shown here as a plain list, cheapest first, and kept nowhere.
// Changing the size or the currency fetches them again. An option Keevoro
// can back up to opens Keevoro's own form for it; prices stay Stratus's.

import { useEffect, useRef, useState } from 'react'
import type { Backend } from './backend'
import { STRATUS_PAGE, currencyFor, fetchedSaid, gigabytesToAsk, kindLine, money, rowsOf, sizeSaid, sizeSteps } from './stratus'
import type { Comparison, Currency, Row, SetUp } from './stratus'

/** How long the prices may take before the sheet says it is fetching them. */
const A_MOMENT_MS = 400

const CURRENCIES: { code: Currency; said: string }[] = [
  { code: 'GBP', said: 'Pounds (£)' },
  { code: 'USD', said: 'US dollars ($)' },
  { code: 'EUR', said: 'Euros (€)' },
]

/** The quiet line, and the sheet it opens. */
export function CompareStorage(props: {
  api: Backend
  /** What is being kept, in bytes: the size the comparison starts at. */
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

type Fetched = { state: 'waiting' } | { state: 'ready'; comparison: Comparison } | { state: 'failed'; why: string }

/**
 * The sheet itself. Without `onSetUp` (no backup has been started yet), an
 * option Keevoro can back up to is said to be one, with where to choose it
 * later.
 */
export function CompareSheet(props: {
  api: Backend
  keptBytes: number
  onClose: () => void
  onSetUp?: (setUp: SetUp) => void
  onProblem: (error: unknown) => void
}) {
  const { api, keptBytes, onClose, onProblem } = props
  const backup = keptBytes > 0 ? gigabytesToAsk(keptBytes) : null
  const [gigabytes, setGigabytes] = useState(backup ?? 1000)
  const [currency, setCurrency] = useState<Currency>(() => currencyFor(navigator.language))
  const [attempt, setAttempt] = useState(0)
  const [fetched, setFetched] = useState<Fetched>({ state: 'waiting' })
  // The last prices shown, kept on screen, faded, while new ones come.
  const [shown, setShown] = useState<Comparison | null>(null)
  const [slow, setSlow] = useState(false)
  const sheet = useRef<HTMLDivElement>(null)

  // Asked again from the event that changed what is asked, so the old
  // answer is set aside at once.
  const askAgain = (change: () => void) => {
    setFetched({ state: 'waiting' })
    setSlow(false)
    change()
  }
  useEffect(() => {
    let current = true
    const moment = window.setTimeout(() => current && setSlow(true), A_MOMENT_MS)
    api
      .compareStorage(gigabytes, currency)
      .then((comparison) => {
        if (!current) return
        setFetched({ state: 'ready', comparison })
        setShown(comparison)
      })
      .catch((error) => current && setFetched({ state: 'failed', why: error instanceof Error ? error.message : String(error) }))
      .finally(() => window.clearTimeout(moment))
    return () => {
      current = false
      window.clearTimeout(moment)
    }
  }, [api, gigabytes, currency, attempt])

  // The latest way to close, so the sheet takes the focus once, as it
  // opens, and gives it back to what opened it as it closes.
  const close = useRef(onClose)
  useEffect(() => {
    close.current = onClose
  })
  useEffect(() => {
    const opener = document.activeElement instanceof HTMLElement ? document.activeElement : null
    sheet.current?.focus()
    // Said as handled, so the page behind does not go back as well.
    const escape = (event: KeyboardEvent) => {
      if (event.key !== 'Escape') return
      event.preventDefault()
      close.current()
    }
    window.addEventListener('keydown', escape)
    return () => {
      window.removeEventListener('keydown', escape)
      if (opener?.isConnected) opener.focus()
    }
  }, [])

  const openInBrowser = (page: string) => api.openWebPage(page).catch(onProblem)
  const comparison = fetched.state === 'ready' ? fetched.comparison : fetched.state === 'waiting' ? shown : null
  const rows = comparison ? rowsOf(comparison) : []
  const forWhat = gigabytes === backup ? `For about ${sizeSaid(gigabytes)}, the size of this backup.` : `For ${sizeSaid(gigabytes)} of storage.`
  return (
    <div className="compare-sheet">
      <div
        className="panel"
        role="dialog"
        aria-modal="true"
        aria-labelledby="compare-title"
        aria-describedby="compare-for"
        tabIndex={-1}
        ref={sheet}
      >
        <header>
          <h2 id="compare-title">Compare cloud storage</h2>
          <button className="button small" onClick={onClose}>
            Close
          </button>
        </header>
        <div className="compare-asked">
          <p id="compare-for" className="compare-for">
            {forWhat}
          </p>
          <div className="compare-choose">
            <label>
              <span>Size</span>
              <select value={gigabytes} onChange={(e) => askAgain(() => setGigabytes(Number(e.target.value)))}>
                {sizeSteps(backup).map((gb) => (
                  <option key={gb} value={gb}>
                    {gb === backup ? `About ${sizeSaid(gb)}, this backup` : sizeSaid(gb)}
                  </option>
                ))}
              </select>
            </label>
            <label>
              <span>Currency</span>
              <select value={currency} onChange={(e) => askAgain(() => setCurrency(e.target.value as Currency))}>
                {CURRENCIES.map((c) => (
                  <option key={c.code} value={c.code}>
                    {c.said}
                  </option>
                ))}
              </select>
            </label>
          </div>
        </div>
        {!props.onSetUp && (
          <p className="quiet small">Once you have started a backup, choose where it goes under “Where should they go?”.</p>
        )}
        <div className="compare-list" aria-busy={fetched.state === 'waiting'}>
          {fetched.state === 'waiting' && slow && (
            <p className="quiet compare-fetching" role="status">
              Fetching prices…
            </p>
          )}
          {fetched.state === 'failed' && (
            <div className="compare-failed" role="alert">
              <p>{fetched.why}</p>
              <div className="compare-failed-actions">
                <button className="primary" onClick={() => askAgain(() => setAttempt((n) => n + 1))}>
                  Try again
                </button>
                <button className="button" onClick={() => openInBrowser(STRATUS_PAGE)}>
                  Open Stratus in your browser
                </button>
              </div>
            </div>
          )}
          {comparison && rows.length === 0 && fetched.state === 'ready' && (
            <p className="quiet">Stratus has nothing that fits {sizeSaid(comparison.gigabytes)} just now.</p>
          )}
          {comparison && rows.length > 0 && (
            <table className={fetched.state === 'waiting' ? 'compare-table stale' : 'compare-table'}>
              <caption className="visually-hidden">Cloud storage for {sizeSaid(comparison.gigabytes)}, cheapest first</caption>
              <thead>
                <tr>
                  <th scope="col">Provider and plan</th>
                  <th scope="col" className="num">
                    Storage
                  </th>
                  <th scope="col" className="num">
                    A month
                  </th>
                  <th scope="col" className="num year">
                    A year
                  </th>
                  <th scope="col">
                    <span className="visually-hidden">Using it with Keevoro</span>
                  </th>
                </tr>
              </thead>
              <tbody>
                {rows.map((row) => (
                  <OptionRow key={row.option.id} row={row} currency={comparison.currency} onSetUp={props.onSetUp} onOpen={openInBrowser} />
                ))}
              </tbody>
            </table>
          )}
        </div>
        <footer className="quiet small compare-credit">
          {comparison && (
            <p>Prices are Stratus’, fetched {fetchedSaid(comparison.fetchedAt)}. Check with the provider before you buy.</p>
          )}
          <p>
            Comparison by{' '}
            <button className="link" onClick={() => openInBrowser(STRATUS_PAGE)} title="Opens in your browser">
              Stratus
            </button>
            , made by Adam Sparshott.
          </p>
        </footer>
      </div>
    </div>
  )
}

/** One option: who and what, how much, and what Keevoro can do with it. */
function OptionRow(props: { row: Row; currency: Currency; onSetUp?: (setUp: SetUp) => void; onOpen: (page: string) => void }) {
  const { row, currency, onSetUp, onOpen } = props
  const { option, setUp } = row
  return (
    <tr className={row.cheapest ? 'cheapest' : undefined}>
      <th scope="row">
        <span className="compare-name">
          <b>{option.providerName}</b> <span>{option.name}</span>
        </span>
        {row.cheapest && <span className="compare-best">Cheapest Keevoro can use</span>}
        <span className="compare-kind">
          {kindLine(option)}
          {row.notRefreshed && <span className="compare-old"> Price not refreshed today.</span>}
        </span>
      </th>
      <td className="num">{row.storage}</td>
      <td className="num month">
        <b>{money(option.monthly, currency)}</b>
      </td>
      <td className="num year">{option.yearly === null ? '' : money(option.yearly, currency)}</td>
      <td className="compare-act">
        {setUp && onSetUp ? (
          // Named with its plan, which the visible words start, so each button is told apart.
          <button className="button small" onClick={() => onSetUp(setUp)} aria-label={`Set this up: ${option.name}`}>
            Set this up
          </button>
        ) : setUp ? (
          <span className="quiet small">Keevoro can back up to this</span>
        ) : (
          <>
            <span className="quiet small">Keevoro can’t back up to this</span>
            {option.url && (
              <button
                className="link small"
                onClick={() => option.url && onOpen(option.url)}
                aria-label={`Their website: ${option.providerName}`}
                title="Opens in your browser"
              >
                Their website
              </button>
            )}
          </>
        )}
      </td>
    </tr>
  )
}
