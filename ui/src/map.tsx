// Where the space goes: what takes the room in a drive or folder, in the
// style of WinDirStat, from what a survey saw. Everything is here, photos
// or not, because seeing the zip files, screen recordings or an old
// library that fill a folder is what makes it useful; what is not a photo
// or video is shown muted, and what Keevoro would not back up says why.
//
// It only reads. Nothing is opened (the survey read folder listings only)
// and nothing is added or changed, except by the buttons that say so:
// leaving a folder out of the backup, or looking through it.
//
// The list is the whole of it for a keyboard or a screen reader: every
// folder and file the map shows, with its size and what it holds, and the
// trail above goes back up.

import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import type { Backend, LeftOutFolder, MapNode, MapView } from './backend'
import { ago, files, share, size } from './format'
import { nameOf } from './inside'
import { LeaveOutButton, LeftOutSaid } from './leaveOut'
import { MapCanvas, MapKey, mapReading } from './mapCanvas'
import { called, FILE_MANAGER, find, gatheredWords, holds, keyOf as keyFor, keysOf, notesOn } from './mapWords'
import type { Picked } from './mapWords'
import { KindsList } from './mapKinds'
import { SurveyingPanel, useAfterSurvey } from './surveyFirst'
import { SummaryRing } from './summaryRing'
import { chooseSpaceView, savedSpaceView } from './spaceView'
import type { SpaceView } from './spaceView'
import { gathered, GATHER_LEAST } from './treemap'

/** How many levels of folders are asked for at once. */
const DEPTH = 3
/** How many answers are kept, so going back up is instant. */
const KEPT = 8

export function SpaceScreen(props: {
  api: Backend
  /** The place to map, written whole. */
  place: string
  /** What to call it, when it has a name of its own (a source's or a drive's). */
  name?: string
  /** The folder inside it to show first, to come back to where the person was. */
  at?: string
  /** Hears each folder gone into, for coming back to it. */
  onAt?: (folder: string) => void
  onBack: () => void
  /** A folder was left out or put back. */
  onChanged?: () => void
  onLookInside: (folder: string) => void
  onLookThrough: (folder: string) => void
  onProblem: (error: unknown) => void
}) {
  const { api, place, onProblem } = props
  const [folder, setFolder] = useState(props.at ?? place)
  const [version, setVersion] = useState(0)
  const [chosen, setChosen] = useState<string | null>(null)
  const [said, setSaid] = useState<LeftOutFolder | null>(null)
  // The row pointed at in the list, outlined on the map, and the other way round.
  const [listPoint, setListPoint] = useState<string | null>(null)
  const [mapPoint, setMapPoint] = useState<string | null>(null)
  // In a very narrow window the map is a strip of the biggest boxes until asked for.
  const narrow = useNarrow()
  const [wholeMap, setWholeMap] = useState(false)
  // The list by folder, or by kind of file with what was picked there.
  const [byKind, setByKind] = useState(false)
  const [picked, setPicked] = useState<Picked | null>(null)
  // The map beside the list, or the list alone, as last chosen here.
  const [shown, setShown] = useState<SpaceView>(savedSpaceView)
  // Below what share of the folder shown its folders are gathered: as the
  // map has it, or 2% for the list alone.
  const [mapShare, setMapShare] = useState(GATHER_LEAST)
  // Small folders gathered together, opened: shown as if a folder of their own.
  const [opened, setOpened] = useState<MapNode | null>(null)
  const answers = useRef(new Map<string, MapView>())

  const ask = useCallback(async () => {
    const kept = answers.current.get(folder)
    if (kept) return kept
    const view = await api.spaceMap({ place, ...(folder === place ? {} : { folder }), depth: DEPTH })
    if (view) {
      answers.current.set(folder, view)
      if (answers.current.size > KEPT) answers.current.delete(answers.current.keys().next().value as string)
    }
    return view
  }, [api, place, folder])
  const { answer: view, surveying, stop, mapAgain } = useAfterSurvey(api, place, ask, `${folder} ${version}`, onProblem)
  // What is shown: the folder, or the small folders gathered from it, opened.
  const shownTop = useMemo<MapNode | null>(
    () =>
      view && opened?.gathered
        ? { ...opened, node: 'folder', children: opened.gathered, gathered: undefined, gatheredKey: undefined }
        : (view?.top ?? null),
    [view, opened],
  )
  const keys = useMemo(() => (shownTop ? keysOf(shownTop) : new Map<MapNode, string>()), [shownTop])
  // A map out of date is mapped again once as it opens: by itself on a disk
  // of this computer, after asking across a network.
  const refreshed = useRef(false)
  const [answered, setAnswered] = useState(false)
  const again = useCallback(() => {
    refreshed.current = true
    setAnswered(true)
    answers.current.clear()
    mapAgain()
  }, [mapAgain])
  useEffect(() => {
    if (view?.refresh === 'byItself' && !refreshed.current && !surveying) again()
  }, [view, surveying, again])
  const asking = view?.refresh === 'askFirst' && !answered && !surveying

  // Asked afresh after a change, with nothing kept from before it.
  const afresh = () => {
    answers.current.clear()
    setVersion((v) => v + 1)
  }
  const changed = (done: LeftOutFolder) => {
    setSaid(done)
    afresh()
    props.onChanged?.()
  }
  const go = (to: string) => {
    setOpened(null)
    setChosen(null)
    setSaid(null)
    setListPoint(null)
    setMapPoint(null)
    // An ending is a place in this folder's list of kinds, so it goes with it.
    setPicked(null)
    setFolder(to)
    props.onAt?.(to)
  }

  const name = props.name ?? view?.trail[0]?.name ?? nameOf(place)
  if (!view) {
    return (
      <section className="space">
        <header>
          <button className="link back" onClick={props.onBack}>
            ‹ Back
          </button>
          <h1>Where the space goes on {name}</h1>
        </header>
        {surveying && <SurveyingPanel name={name} surveying={surveying} onStop={stop} />}
        {view === null && !surveying && <div className="empty quiet">Keevoro couldn’t map this folder.</div>}
      </section>
    )
  }

  const top = shownTop ?? view.top
  const share = shown === 'list' ? GATHER_LEAST : mapShare
  const rows = gathered(top, top.children ?? [], mapReading(keys), share)
  const openGathered = (node: MapNode) => {
    setChosen(null)
    setListPoint(null)
    setMapPoint(null)
    setOpened(node)
  }
  const path = find(top, keys, chosen)
  const selected = path.length > 1 ? path[path.length - 1] : null
  // A box chosen deep in the map is in one of the list's rows.
  const chosenRow = path.length > 1 ? (keys.get(path[1]) ?? null) : null
  const up = view.trail.length > 1 ? view.trail[view.trail.length - 2].folder : null
  return (
    <section className="space">
      <header>
        <button className="link back" onClick={opened ? () => setOpened(null) : props.onBack}>
          ‹ Back
        </button>
        <h1>Where the space goes on {name}</h1>
        <p className="status">
          {size(top.bytes)} in {top.name}: photos {size(top.photoBytes)}, videos {size(top.videoBytes)},{' '}
          {top.carriedBytes ? `other files to carry along ${size(top.carriedBytes)}, ` : ''}everything else{' '}
          {size(top.otherBytes)}.{' '}
          {view.backup
            ? `This backup would take ${size(top.wouldBackUp)} of it.`
            : `${size(top.wouldBackUp)} of it is ${top.carried ? 'photos, videos and other files' : 'photos and videos'} Keevoro would back up.`}
        </p>
        <p className="quiet small">
          Each box is a folder or file, sized by the room it takes; click one to choose it, and again to go inside.
          Grey boxes are not photos or videos. They are shown so you can see what is taking the room; Keevoro will not
          back them up. Nothing here is opened or changed. Mapped {ago(view.surveyedAt)}.{' '}
          <button
            className="link"
            onClick={again}
          >
            Map again
          </button>
        </p>
        {!view.complete && (
          <p className="quiet small">
            <b>Partly mapped.</b> The survey was stopped part-way, so there may be more here than is shown.
          </p>
        )}
        {view.complete && !view.sizedAll && (
          <p className="quiet small">
            The survey was stopped before everything was measured, so some folders may take more room than shown.
          </p>
        )}
      </header>
      {surveying && <SurveyingPanel name={name} surveying={surveying} onStop={stop} />}
      {asking && (
        <div className="panel asking map-again" role="region" aria-label="Map it again?">
          <p>
            This was mapped {ago(view.surveyedAt)}
            {view.searchedSince ? ', and a source in it has been looked through since' : ''}. Map it again? It may take a
            while over the network.
          </p>
          <p className="choices">
            <button className="primary" onClick={again}>
              Map it again
            </button>
            <button
              onClick={() => {
                refreshed.current = true
                setAnswered(true)
              }}
            >
              Use this map
            </button>
          </p>
        </div>
      )}
      <Trail view={view} onGo={go} gathered={opened ? () => setOpened(null) : undefined} />
      <div className="space-summary">
        <SummaryRing
          top={top}
          picked={byKind && picked && picked.ending === undefined ? picked.kind : null}
          onPick={(kind) => {
            setPicked(kind ? { kind } : null)
            if (kind) setByKind(true)
          }}
        />
        <div className="segmented view-switch" role="group" aria-label="Show as">
          {(['map', 'list'] as const).map((v) => (
            <button
              key={v}
              aria-pressed={shown === v}
              onClick={() => {
                setShown(v)
                chooseSpaceView(v)
              }}
            >
              {v === 'map' ? 'Map' : 'List'}
            </button>
          ))}
        </div>
      </div>
      <p className="visually-hidden" aria-live="polite">
        In {top.name}: {size(top.bytes)}
      </p>
      <LeftOutSaid done={said} />
      <div className={shown === 'list' ? 'space-body list-only' : 'space-body'}>
        {shown === 'map' && (
          <div className={narrow && !wholeMap ? 'space-map strip' : 'space-map'}>
            {narrow && (
              <button className="button small" onClick={() => setWholeMap((w) => !w)} aria-expanded={wholeMap}>
                {wholeMap ? 'Show less of the map' : 'Show the map'}
              </button>
            )}
            <MapCanvas
              top={top}
              keys={keys}
              depth={narrow && !wholeMap ? 1 : DEPTH}
              chosen={chosen}
              lit={listPoint}
              onChoose={setChosen}
              onGo={go}
              onPoint={setMapPoint}
              picked={byKind ? picked : null}
              onOpenGathered={openGathered}
              onShare={setMapShare}
            />
            <MapKey carried={(top.carried ?? 0) > 0} />
          </div>
        )}
        <div className="space-side">
          <div className="segmented list-tabs" role="tablist" aria-label="Show the list">
            <button role="tab" id="by-folder" aria-selected={!byKind} aria-controls="space-list-panel" onClick={() => setByKind(false)}>
              By folder
            </button>
            <button role="tab" id="by-kind" aria-selected={byKind} aria-controls="space-list-panel" onClick={() => setByKind(true)}>
              By kind of file
            </button>
          </div>
          <div id="space-list-panel" role="tabpanel" aria-labelledby={byKind ? 'by-kind' : 'by-folder'}>
            {byKind ? (
              <KindsList view={view} picked={picked} onPick={setPicked} />
            ) : (
              <SpaceList
                top={top}
                rows={rows}
                keys={keys}
                chosen={chosenRow}
                lit={mapPoint}
                onChoose={setChosen}
                onPoint={setListPoint}
                onGo={go}
                onUp={opened ? () => setOpened(null) : up ? () => go(up) : undefined}
              />
            )}
          </div>
        </div>
      </div>
      {/* Below the list, held at the foot of the window, so choosing a row never moves the rows. */}
      <Chosen
        api={api}
        node={selected}
        onLookInside={props.onLookInside}
        onLookThrough={props.onLookThrough}
        onChanged={changed}
        onProblem={onProblem}
      />
    </section>
  )
}

/** True in a window too narrow for a map beside or above the list. */
function useNarrow() {
  const query = '(max-width: 480px)'
  const [narrow, setNarrow] = useState(() => window.matchMedia(query).matches)
  useEffect(() => {
    const watch = window.matchMedia(query)
    const changed = () => setNarrow(watch.matches)
    watch.addEventListener('change', changed)
    return () => watch.removeEventListener('change', changed)
  }, [])
  return narrow
}

/** The way back up, from the place to the folder shown. */
export function Trail(props: { view: MapView; onGo: (folder: string) => void; gathered?: () => void }) {
  const steps = props.view.trail
  const last = props.gathered ? steps.length : steps.length - 1
  return (
    <nav className="trail" aria-label="Where you are">
      <ol>
        {steps.map((step, i) => (
          <li key={step.folder}>
            {i === last ? (
              <span aria-current="page">{step.name}</span>
            ) : (
              <button className="link" onClick={() => (i === steps.length - 1 ? props.gathered?.() : props.onGo(step.folder))}>
                {step.name}
              </button>
            )}
          </li>
        ))}
        {props.gathered && (
          <li>
            <span aria-current="page">Smaller folders</span>
          </li>
        )}
      </ol>
    </nav>
  )
}

/**
 * What can be done with the row chosen: look inside a folder, look through
 * it properly, leave it out of the backup or put it back.
 */
function Chosen(props: {
  api: Backend
  node: MapNode | null
  onLookInside: (folder: string) => void
  onLookThrough: (folder: string) => void
  onChanged: (done: LeftOutFolder) => void
  onProblem: (error: unknown) => void
}) {
  const node = props.node
  if (!node) {
    return <p className="chosen quiet small">Choose a folder in the list to see what you can do with it.</p>
  }
  const path = node.path
  return (
    <div className="chosen" aria-label="Chosen">
      <p>
        <b>{called(node)}</b> <span className="quiet">{size(node.bytes)}</span>
        {node.node !== 'files' && <span className="quiet small"> · {holds(node)}</span>}
      </p>
      {notesOn(node).map((note) => (
        <p key={note} className="quiet small">
          {note}
        </p>
      ))}
      {node.node === 'folder' && path && (
        <p className="choices within">
          {!node.skipped && (
            <button className="button small" onClick={() => props.onLookInside(path)} aria-label={`Look inside ${node.name}`}>
              Look inside
            </button>
          )}
          {!node.skipped && !node.leftOutFolder && (
            <button className="button small" onClick={() => props.onLookThrough(path)} aria-label={`Look through ${node.name}`}>
              Look through it
            </button>
          )}
          <button
            className="button small"
            onClick={() => props.api.openPlace(path).catch(props.onProblem)}
            aria-label={`Show ${node.name} in ${FILE_MANAGER}`}
          >
            Show in {FILE_MANAGER}
          </button>
          {!node.skipped && (
            <LeaveOutButton
              api={props.api}
              folder={node.leftOutFolder ?? path}
              leftOut={Boolean(node.leftOutFolder)}
              small
              onChanged={props.onChanged}
              onProblem={props.onProblem}
            />
          )}
        </p>
      )}
    </div>
  )
}

/**
 * Every box of the folder shown, the biggest first: a grid a keyboard moves
 * through with the arrow keys. Enter goes into a folder, and Backspace (or
 * Alt and the up arrow) goes back up.
 */
export function SpaceList(props: {
  top: MapNode
  rows: readonly MapNode[]
  keys: Map<MapNode, string>
  chosen: string | null
  onChoose: (key: string) => void
  onGo: (folder: string) => void
  onUp?: () => void
  /** The row pointed at on the map, lit and scrolled to. */
  lit?: string | null
  onPoint?: (key: string | null) => void
}) {
  const { top } = props
  const body = useRef<HTMLTableSectionElement>(null)
  const keyOf = (node: MapNode) => keyFor(props.keys, node)
  // Small folders gathered into one row open in place, the folders indented beneath it.
  const [open, setOpen] = useState<Set<string>>(new Set())
  const toggle = (key: string) =>
    setOpen((was) => {
      const now = new Set(was)
      if (now.has(key)) now.delete(key)
      else now.add(key)
      return now
    })
  const rows = props.rows.flatMap((node) => {
    const key = keyOf(node)
    const members = node.gathered && open.has(key) ? node.gathered.map((m) => ({ node: m, within: true })) : []
    return [{ node, within: false }, ...members]
  })
  const at = Math.max(0, rows.findIndex((r) => keyOf(r.node) === props.chosen))
  // The row pointed at on the map is brought into view in the list.
  useEffect(() => {
    if (!props.lit) return
    body.current?.querySelector<HTMLElement>(`[data-key="${CSS.escape(props.lit)}"]`)?.scrollIntoView({ block: 'nearest' })
  }, [props.lit])
  const focusRow = (i: number) => {
    const row = body.current?.querySelectorAll<HTMLElement>('tr')[i]
    row?.focus()
  }
  const keys = (e: React.KeyboardEvent, i: number, node: MapNode) => {
    if (e.key === 'ArrowDown') focusRow(Math.min(rows.length - 1, i + 1))
    else if (e.key === 'ArrowUp' && !e.altKey) focusRow(Math.max(0, i - 1))
    else if (e.key === 'Home') focusRow(0)
    else if (e.key === 'End') focusRow(rows.length - 1)
    else if (e.key === 'Enter' && node.gathered) toggle(keyOf(node))
    else if (e.key === 'Enter' && node.node === 'folder' && node.path) props.onGo(node.path)
    else if ((e.key === 'Backspace' || (e.key === 'ArrowUp' && e.altKey)) && props.onUp) props.onUp()
    else return
    e.preventDefault()
  }
  if (rows.length === 0) return <div className="empty quiet">This folder is empty.</div>
  return (
    <div className="space-list">
      <table role="grid" aria-label={`What is in ${top.name}, the biggest first`}>
        <thead>
          <tr>
            <th scope="col">Name</th>
            <th scope="col" className="number">
              Size
            </th>
            <th scope="col" className="share-column">
              Share
            </th>
          </tr>
        </thead>
        <tbody ref={body}>
          {rows.map(({ node, within }, i) => {
            const key = keyOf(node)
            const notes = node.gathered ? [gatheredWords(node).replace(' Click to open them.', '')] : notesOn(node)
            const muted = node.photos + node.videos === 0 || Boolean(node.skipped)
            return (
              <tr
                key={key}
                data-key={key}
                tabIndex={i === at ? 0 : -1}
                aria-selected={key === props.chosen}
                className={[muted ? 'muted' : '', key === props.lit ? 'lit' : '', within ? 'within' : ''].join(' ').trim() || undefined}
                onFocus={() => props.onChoose(key)}
                onClick={() => props.onChoose(key)}
                onPointerEnter={() => props.onPoint?.(key)}
                onPointerLeave={() => props.onPoint?.(null)}
                onKeyDown={(e) => keys(e, i, node)}
              >
                <td className="name">
                  {node.gathered ? (
                    <button
                      className="link disclosure"
                      tabIndex={-1}
                      aria-expanded={open.has(key)}
                      onClick={(e) => {
                        e.stopPropagation()
                        toggle(key)
                      }}
                    >
                      {open.has(key) ? '▾' : '▸'} Smaller folders
                    </button>
                  ) : node.node === 'folder' && node.path ? (
                    <button className="link" tabIndex={-1} onClick={() => node.path && props.onGo(node.path)}>
                      {node.name}
                    </button>
                  ) : (
                    <span>{called(node)}</span>
                  )}
                  <span className="holds small quiet">{node.node === 'files' ? '' : holds(node)}</span>
                  {notes.map((note) => (
                    <span key={note} className="note small">
                      {note}
                    </span>
                  ))}
                </td>
                <td className="number">{size(node.bytes)}</td>
                <td className="share-column">
                  <ShareBar node={node} whole={top.bytes} />
                </td>
              </tr>
            )
          })}
        </tbody>
      </table>
    </div>
  )
}

/** A row's share of the folder: a bar in parts, photos, videos, other files carried along and the rest, and the figure. */
function ShareBar(props: { node: MapNode; whole: number }) {
  const { node, whole } = props
  const part = (bytes: number) => `${whole > 0 ? (bytes * 100) / whole : 0}%`
  return (
    <span className="share" title={`${files(node.photos, node.videos, node.carried ?? 0)}, ${size(node.bytes)}`}>
      <span className="share-bar" aria-hidden>
        <span className="photo" style={{ width: part(node.photoBytes) }} />
        <span className="video" style={{ width: part(node.videoBytes) }} />
        {node.carriedBytes ? <span className="carried" style={{ width: part(node.carriedBytes) }} /> : null}
        <span className="other" style={{ width: part(node.otherBytes) }} />
      </span>
      <span className="share-figure">{share(node.bytes, whole)}</span>
    </span>
  )
}
