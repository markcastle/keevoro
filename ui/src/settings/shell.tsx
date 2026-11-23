// Settings and Help: two places side by side, opened from the foot of the
// side band, built the same way so they feel like siblings. Settings holds
// only what you set; Help holds what helps: short answers, reporting a
// problem or an idea, the comparison of cloud storage, the log, and About.
//
// While either is open it has the whole window. Its own short list of
// sections takes the place of the side band, with a search at the top
// (which finds rows in both places, and says which each is in) and a way
// back above it, so there is never a second side band beside the first.
// In a narrow window the list is the first page and each section a page
// you go into, as on a phone.
//
// Each section is a column of rounded panels of rows (parts.tsx); anything
// long (the kinds of file, the AI agents' folders, the licences) is a page
// of its own inside its section, with a way back. Where each place was is
// remembered for as long as Keevoro is open, and a link from elsewhere in
// the app, the icon by the clock or a search lands on the right section
// with its row brought into view and marked for a moment.

import { useEffect, useRef, useState } from 'react'
import type { KeyboardEvent, ReactNode } from 'react'
import type { AppState, ArrangeView, Backend, Update, WantsView } from '../backend'
import { LicencesList } from '../licences'
import type { SetUp } from '../stratus'
import { AboutSection } from './about'
import { AdvancedSection } from './advanced'
import { AgentsPage } from './agents'
import { AppearanceSection } from './appearance'
import { BackingUpSection } from './backingUp'
import { FileKindsPage } from './fileKinds'
import { CompareSection, HowToSection } from './help'
import type { Going } from './help'
import { LogPage } from './log'
import { ReportPage } from './report'
import { Intro, SectionIcon } from './parts'
import { DETAILS, PLACE_TITLES, findRows, placeOfRow, sectionTitle, sectionsOf, whereSaid } from './places'
import type { DetailId, Place, SectionId } from './places'
import { ThisBackupSection } from './thisBackup'
import { UpdatesSection } from './updates'

/** Narrower than this, the list and a section are a page each (the same width the side band becomes a strip). */
const NARROW = '(max-width: 640px)'
const narrow = () => window.matchMedia(NARROW).matches

/** How long a row brought into view stays marked. */
const MARKED_FOR = 2400

/** How long to wait for a row whose section is still asking Keevoro what to show. */
const ROW_WAITED_FOR = 3000

export function PlaceScreen(props: {
  api: Backend
  state: AppState
  sendingTo: string | null
  update: Update | null
  onUpdate: (update: Update | null) => void
  onLibrary: (state: AppState) => void
  /** Called once how the backup is arranged has changed. */
  onArranged: (view: ArrangeView) => void
  /** What the open backup is for changed, here in This backup. */
  onWantsChanged: (view: WantsView) => void
  /** Called after a reset, with the app as it is when first installed. */
  onReset: (state: AppState) => void
  /** Goes to the step that chooses where the backup goes. */
  onSend: () => void
  /** Goes to the sources, where they can be looked through again. */
  onSources: () => void
  onBackups: () => void
  /** Where an answer in How do I… goes. */
  onGo: (going: Going) => void
  /** What the backup keeps, in bytes, for the comparison of cloud storage. */
  keptBytes: number
  /** Opens Keevoro's own form for a kind of storage chosen in the comparison; none before a backup is started. */
  onSetUp?: (setUp: SetUp) => void
  /** Where Settings or Help is, kept by the app so each opens there again. */
  place: Place
  onPlace: (place: Place) => void
  onBack: () => void
  onProblem: (error: unknown) => void
  /** The bars that can show on any page: an update ready, a problem. */
  banner: ReactNode
  /** The foot of the side band, as on every other screen. */
  foot?: ReactNode
  /** The screen the person was on, when they came here to report something. */
  reportFrom?: string | null
}) {
  const { api, state, place, onPlace, onProblem } = props
  const where = place.where
  const placeTitle = PLACE_TITLES[where]
  const sections = sectionsOf(where)
  const section: SectionId = place.section ?? sections[0].id
  const detail = place.detail
  // In a narrow window: a section's page, rather than the list.
  const atPage = place.section !== null && !place.list
  const [typed, setTyped] = useState('')
  const [kindsChanged, setKindsChanged] = useState(0)
  const heading = useRef<HTMLHeadingElement>(null)
  const title = useRef<HTMLHeadingElement>(null)
  const list = useRef<HTMLUListElement>(null)
  // What gets focus once the page has changed: the page's heading, a row, or the list.
  const focusNext = useRef<'heading' | 'list' | { row: string } | null>(null)

  const go = (to: Place, focus: typeof focusNext.current) => {
    focusNext.current = focus
    onPlace(to)
  }
  const openSection = (id: SectionId) => go({ where, section: id }, narrow() ? 'heading' : null)
  const openDetail = (id: DetailId) => go({ where, section: DETAILS[id].section, detail: id }, 'heading')
  const back = () => {
    if (detail) go({ where, section }, { row: detail })
    else go({ where, section, list: true }, 'list')
  }
  const openFound = (id: string) => {
    setTyped('')
    const to = placeOfRow(id)
    go(to, to.row ? null : 'heading')
  }

  // Arriving, the focus is on the heading of the page arrived at (or, in
  // a narrow window showing the list, on the place's name), unless a row
  // was asked for, which takes it itself.
  useEffect(() => {
    if (place.row) return
    if (narrow() && !atPage) title.current?.focus()
    else heading.current?.focus()
    // Only on arriving.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  useEffect(() => {
    const next = focusNext.current
    focusNext.current = null
    if (next === 'heading') heading.current?.focus()
    else if (next === 'list') list.current?.querySelector<HTMLElement>('[aria-current="page"]')?.focus()
    else if (next) document.getElementById(`setting-${next.row}`)?.focus()
  }, [place])

  // Escape goes back: out of a page within a section, out of a section in
  // a narrow window, and otherwise out of Settings or Help.
  useEffect(() => {
    const onKey = (e: globalThis.KeyboardEvent) => {
      if (e.key !== 'Escape' || e.defaultPrevented) return
      const there = e.target as HTMLElement | null
      if (there?.closest('input, textarea, select, [role="dialog"], [role="alertdialog"], [role="menu"]')) return
      // A sheet over the page closes first, on its own Escape.
      if (document.querySelector('[aria-modal="true"]')) return
      e.preventDefault()
      if (detail || (narrow() && atPage)) back()
      else props.onBack()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  })

  // A row asked for is brought into view and marked, once its section has drawn it.
  useEffect(() => {
    const row = place.row
    if (!row) return
    let waiting = 0
    let unmark = 0
    const started = performance.now()
    const look = () => {
      const found = document.getElementById(`setting-${row}`)
      if (!found) {
        if (performance.now() - started < ROW_WAITED_FOR) waiting = requestAnimationFrame(look)
        return
      }
      found.scrollIntoView({ block: 'center' })
      found.dataset.marked = ''
      const control = found.matches('button') ? found : found.querySelector<HTMLElement>('button:not(:disabled):not(.info-tip-button), input, select')
      control?.focus({ preventScroll: true })
      unmark = window.setTimeout(() => {
        delete found.dataset.marked
        onPlace({ where, section: place.section, detail: place.detail })
      }, MARKED_FOR)
    }
    waiting = requestAnimationFrame(look)
    return () => {
      cancelAnimationFrame(waiting)
      window.clearTimeout(unmark)
      document.getElementById(`setting-${row}`)?.removeAttribute('data-marked')
    }
    // Only a new row asked for starts this again.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [place.row, place.section, place.detail])

  // Up and down move through the sections; in a wide window the section follows.
  const onListKey = (e: KeyboardEvent<HTMLUListElement>) => {
    const buttons = [...(list.current?.querySelectorAll<HTMLButtonElement>('button[data-section]') ?? [])]
    const at = buttons.indexOf(document.activeElement as HTMLButtonElement)
    const to =
      e.key === 'ArrowDown' ? Math.min(at + 1, buttons.length - 1)
      : e.key === 'ArrowUp' ? Math.max(at - 1, 0)
      : e.key === 'Home' ? 0
      : e.key === 'End' ? buttons.length - 1
      : null
    if (to === null || at < 0) return
    e.preventDefault()
    buttons[to].focus()
    if (!narrow()) onPlace({ where, section: buttons[to].dataset.section as SectionId })
  }

  const found = findRows(typed)
  const searching = typed.trim() !== ''
  const noBackup = state.backup === null

  const page = (() => {
    switch (detail) {
      case 'kinds':
        return (
          <FileKindsPage
            api={api}
            onSources={props.onSources}
            onChanged={() => setKindsChanged((n) => n + 1)}
            onProblem={onProblem}
          />
        )
      case 'agents':
        return <AgentsPage api={api} onProblem={onProblem} />
      case 'licences':
        return <LicencesList withThanks />
    }
    switch (section) {
      case 'backup':
        return noBackup ? (
          <Intro>These are set for each backup. Start one first, on the first screen, and its settings appear here.</Intro>
        ) : (
          <ThisBackupSection
            api={api}
            state={state}
            sendingTo={props.sendingTo}
            kindsChanged={kindsChanged}
            onLibrary={props.onLibrary}
            onArranged={props.onArranged}
            onWantsChanged={props.onWantsChanged}
            onSend={props.onSend}
            onBackups={props.onBackups}
            onProblem={onProblem}
          />
        )
      case 'byItself':
        return <BackingUpSection api={api} name={state.backup?.name ?? null} onProblem={onProblem} />
      case 'appearance':
        return <AppearanceSection />
      case 'updates':
        return (
          <UpdatesSection api={api} version={state.version} update={props.update} onUpdate={props.onUpdate} onProblem={onProblem} />
        )
      case 'advanced':
        return <AdvancedSection api={api} state={state} onOpen={openDetail} onReset={props.onReset} onProblem={onProblem} />
      case 'howTo':
        return <HowToSection api={api} onGo={props.onGo} onProblem={onProblem} />
      case 'report':
        return <ReportPage api={api} from={props.reportFrom ?? 'Help'} />
      case 'compare':
        return <CompareSection api={api} keptBytes={props.keptBytes} onSetUp={props.onSetUp} onProblem={onProblem} />
      case 'log':
        return <LogPage api={api} onProblem={onProblem} />
      case 'about':
        return (
          <AboutSection
            api={api}
            version={state.version}
            update={props.update}
            onUpdate={props.onUpdate}
            onOpen={openDetail}
            onProblem={onProblem}
          />
        )
    }
  })()

  return (
    <main className="settings-main">
      <div className="kv-settings" data-at={atPage ? 'page' : 'list'} data-place={where}>
        <div className="kv-side">
          <button type="button" className="link kv-leave" onClick={props.onBack}>
            ‹ Back
          </button>
          <h1 className="kv-title" tabIndex={-1} ref={title}>
            {placeTitle}
          </h1>
          <input
            className="kv-search"
            type="search"
            value={typed}
            placeholder="Search"
            aria-label={`Search ${placeTitle}`}
            aria-controls="settings-found"
            spellCheck={false}
            onChange={(e) => setTyped(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Escape' && typed) {
                e.preventDefault()
                setTyped('')
              } else if (e.key === 'Enter' && found[0]) {
                openFound(found[0].id)
              } else if (e.key === 'ArrowDown' && found[0]) {
                e.preventDefault()
                document.querySelector<HTMLElement>('#settings-found button')?.focus()
              }
            }}
          />
          {searching ? (
            <div className="kv-found">
              <p className="visually-hidden" role="status">
                {found.length === 0 ? 'Nothing matches.' : `${found.length} found.`}
              </p>
              {found.length === 0 ? (
                <p className="quiet small kv-none">Nothing in Settings or Help matches “{typed.trim()}”.</p>
              ) : (
                <ul id="settings-found" aria-label="Found in Settings and Help">
                  {found.map((row) => (
                    <li key={row.id}>
                      <button type="button" onClick={() => openFound(row.id)}>
                        <span>{row.title}</span>
                        <small>{whereSaid(row)}</small>
                      </button>
                    </li>
                  ))}
                </ul>
              )}
            </div>
          ) : (
            <nav aria-label={`${placeTitle} sections`}>
              <ul ref={list} onKeyDown={onListKey}>
                {sections.map((s) => (
                  <li key={s.id} className={s.apart ? 'apart' : undefined}>
                    <button
                      type="button"
                      data-section={s.id}
                      aria-current={section === s.id ? 'page' : undefined}
                      tabIndex={section === s.id ? 0 : -1}
                      onClick={() => openSection(s.id)}
                    >
                      <SectionIcon id={s.id} />
                      <span>{s.title}</span>
                    </button>
                  </li>
                ))}
              </ul>
            </nav>
          )}
          {props.foot}
        </div>

        <div className="kv-page">
          {props.banner}
          <section className="kv-page-inner" aria-labelledby="settings-page-title">
            <header className="kv-page-head">
              {detail ? (
                <button type="button" className="link kv-back" onClick={back}>
                  ‹ {sectionTitle(section)}
                </button>
              ) : (
                <button type="button" className="link kv-back kv-narrow-only" onClick={back}>
                  ‹ {placeTitle}
                </button>
              )}
              <h2 id="settings-page-title" tabIndex={-1} ref={heading}>
                {detail ? DETAILS[detail].title : sectionTitle(section)}
              </h2>
            </header>
            {page}
          </section>
        </div>
      </div>
    </main>
  )
}
