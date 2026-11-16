// The app's screens.
//
// Keevoro is used by people who would not call themselves technical, so
// the app is a short walk with one question per screen:
//
//   1  Where are your photos?        pick a drive, or a folder
//   2  Which should we keep?         collections, all kept unless you say
//   3  Gather them into one place    one button, then "they are safe"
//   4  Keep a second copy            another disk, or cloud storage
//
// Every screen has one heading that asks the question, and one big button
// at the bottom that moves on. The four steps down the side are the only
// navigation; there are no tabs to learn. Nothing is asked that has a
// sensible default: the library goes in the Pictures folder unless the
// user says otherwise.
//
// Nothing here decides anything about photos. Every fact on screen comes
// from the engine through backend.ts, and every button sends one request
// back. Wording follows the house rules: plain English, no jargon, and
// progress in human terms ("3,412 of 9,870 safely copied").

import { useCallback, useEffect, useState } from 'react'
import type { ReactNode } from 'react'

import { backend } from './backend'
import type {
  AppState,
  Backend,
  BackupEvent,
  Collection,
  ConnectRequest,
  Discover,
  GatherEvent,
  Photo,
  PhotoChoice,
  Place,
  ScanEvent,
} from './backend'
import { count, photosAndVideos, size, thousands } from './format'

type Step = 'find' | 'choose' | 'gather' | 'backup'
/** Looking inside a collection, or at what was set aside, sits on top of step 2. */
type Overlay = { collection: Collection } | 'setAside' | null

const STEPS: { id: Step; title: string }[] = [
  { id: 'find', title: 'Find your photos' },
  { id: 'choose', title: 'Choose what to keep' },
  { id: 'gather', title: 'Gather them' },
  { id: 'backup', title: 'Back them up' },
]

const all = (n: number) => count(n, 'photo and video').replace('photo and videos', 'photos and videos')

export function App() {
  const [api, setApi] = useState<Backend | null>(null)
  const [state, setState] = useState<AppState | null>(null)
  const [found, setFound] = useState<Discover | null>(null)
  const [step, setStep] = useState<Step>('find')
  const [overlay, setOverlay] = useState<Overlay>(null)
  const [scan, setScan] = useState<ScanEvent | null>(null)
  const [justFound, setJustFound] = useState<ScanEvent | null>(null)
  const [gather, setGather] = useState<GatherEvent | null>(null)
  const [problem, setProblem] = useState<string | null>(null)
  // Where the library is backed up to; undefined until asked.
  const [backedUpTo, setBackedUpTo] = useState<string | null | undefined>(undefined)

  // Anything that goes wrong is said once, in the engine's own plain words.
  const report = useCallback((error: unknown) => {
    setProblem(error instanceof Error ? error.message : String(error))
  }, [])

  const refresh = useCallback((from: Backend) => from.discover().then(setFound).catch(report), [report])

  useEffect(() => {
    let stopScan = () => {}
    let stopGather = () => {}
    let closed = false
    void backend().then(async (b) => {
      if (closed) return
      setApi(b)
      stopScan = b.onScan((event) => {
        setScan(event.finished ? null : event)
        if (event.problem) report(event.problem)
        if (event.finished) {
          setJustFound(event.problem ? null : event)
          void refresh(b)
        }
      })
      stopGather = b.onGather((event) => {
        setGather(event)
        if (event.problem) report(event.problem)
        if (event.step === 'finished') void refresh(b)
      })
      try {
        const s = await b.state()
        setState(s)
        if (s.library) {
          const d = await b.discover()
          setFound(d)
          const where = await b.backupState()
          setBackedUpTo(where)
          // Open on whichever step still needs doing.
          setStep(nextToDo(d, where))
        }
      } catch (error) {
        report(error)
      }
    })
    return () => {
      closed = true
      stopScan()
      stopGather()
    }
  }, [refresh, report])

  if (!api || !state) return <div className="blank" />

  if (!state.library) {
    const begin = (chosen: Promise<AppState | null>) =>
      chosen
        .then((s) => {
          if (!s) return
          setState(s)
          return refresh(api)
        })
        .catch(report)
    return (
      <Welcome
        suggested={state.suggestedLibrary}
        onStart={() => begin(api.useLibrary(state.suggestedLibrary))}
        onChoose={() => begin(api.chooseLibrary())}
        problem={problem}
      />
    )
  }

  const waiting = found?.waitingToGather ?? 0
  const kept = found?.collections.filter((c) => c.kept) ?? []
  const safe = kept.reduce((n, c) => n + c.gathered, 0)
  const gathering = gather !== null && gather.step !== 'finished'
  const anythingFound = (found?.collections.length ?? 0) > 0

  const go = (to: Step) => {
    setOverlay(null)
    setStep(to)
  }
  const keep = (collection: Collection, isKept: boolean) => {
    // Show the choice at once; the engine's answer follows.
    setFound((d) => d && { ...d, collections: d.collections.map((c) => (c.key === collection.key ? { ...c, kept: isKept } : c)) })
    api
      .keepCollection(collection.key, isKept)
      .then(() => refresh(api))
      .catch(report)
  }
  const startGather = () => {
    go('gather')
    setGather({ step: 'copying', done: 0, total: waiting })
    api.gather().catch((error) => {
      setGather(null)
      report(error)
    })
  }

  // What each step has to say for itself in the list down the side.
  const done: Record<Step, boolean> = {
    find: anythingFound,
    choose: anythingFound && (step === 'gather' || step === 'backup' || safe > 0),
    gather: safe > 0 && waiting === 0,
    backup: Boolean(backedUpTo),
  }
  const note: Record<Step, string> = {
    find: found && found.sources.length > 0 ? count(found.sources.length, 'source') : 'Start here',
    choose: anythingFound ? `Keeping ${thousands(waiting + safe)}` : '',
    gather: gathering ? 'Working…' : safe > 0 ? `${thousands(safe)} safe` : '',
    backup: backedUpTo ? 'Set up' : '',
  }
  // A step can be opened once the one before it has something to show.
  const open: Record<Step, boolean> = { find: true, choose: anythingFound, gather: anythingFound, backup: safe > 0 }

  return (
    <div className="app">
      <aside className="sidebar">
        <div className="brand">
          <Logo />
          <span>Keevoro</span>
        </div>
        <ol className="steps" aria-label="Steps">
          {STEPS.map((s, i) => (
            <li key={s.id} className={step === s.id ? 'now' : done[s.id] ? 'done' : ''}>
              <button onClick={() => go(s.id)} disabled={!open[s.id]} aria-current={step === s.id ? 'step' : undefined}>
                <span className="num" aria-hidden>
                  {done[s.id] && step !== s.id ? '✓' : i + 1}
                </span>
                <span>
                  {s.title}
                  {note[s.id] && <small>{note[s.id]}</small>}
                </span>
              </button>
            </li>
          ))}
        </ol>
        <div className="foot">
          <button className="link" onClick={() => api.openLibrary().catch(report)}>
            Open my library
          </button>
          <span title={state.library}>Your sources are only ever read.</span>
        </div>
      </aside>

      <main>
        {problem && (
          <div className="problem" role="alert">
            <span>{problem}</span>
            <button onClick={() => setProblem(null)} aria-label="Dismiss">
              ×
            </button>
          </div>
        )}

        {overlay ? (
          <PhotosScreen
            api={api}
            collection={overlay === 'setAside' ? null : overlay.collection}
            onBack={() => {
              setOverlay(null)
              void refresh(api)
            }}
            onProblem={report}
          />
        ) : step === 'find' ? (
          <FindScreen
            api={api}
            found={found}
            scan={scan}
            justFound={justFound}
            onProblem={report}
            onStarted={() => setJustFound(null)}
            next={
              anythingFound && !scan ? (
                <Next
                  say={`${photosAndVideos(found?.photos ?? 0, found?.videos ?? 0)} found so far.`}
                  button="See what was found"
                  onClick={() => go('choose')}
                />
              ) : null
            }
          />
        ) : step === 'choose' ? (
          <ChooseScreen
            api={api}
            found={found}
            onKeep={keep}
            onOpen={(collection) => setOverlay({ collection })}
            onSetAside={() => setOverlay('setAside')}
            next={
              waiting > 0 ? (
                <Next
                  back={() => go('find')}
                  say={
                    <>
                      Keeping <b>{all(waiting + safe)}</b> in {count(kept.length, 'collection')}.
                    </>
                  }
                  button="Gather them into my library"
                  onClick={startGather}
                />
              ) : safe > 0 ? (
                <Next back={() => go('find')} say="Everything you’re keeping is already in your library." button="Next" onClick={() => go('gather')} />
              ) : (
                <Next back={() => go('find')} say="Nothing is being kept yet. Choose at least one collection." />
              )
            }
          />
        ) : step === 'gather' ? (
          <GatherScreen
            library={state.library}
            waiting={waiting}
            safe={safe}
            collections={kept.length}
            bytes={found?.bytes ?? 0}
            gather={gather}
            gathering={gathering}
            onGather={startGather}
            onOpenLibrary={() => api.openLibrary().catch(report)}
            next={
              gathering ? null : safe > 0 && waiting === 0 ? (
                <Next back={() => go('choose')} say="One copy is not a backup." button="Keep a second copy" onClick={() => go('backup')} />
              ) : (
                <Next back={() => go('choose')} />
              )
            }
          />
        ) : (
          <BackupScreen
            api={api}
            safe={safe}
            gathering={gathering}
            onProblem={report}
            onBackedUpTo={setBackedUpTo}
            next={
              <Next
                back={() => go('gather')}
                say={backedUpTo ? 'That’s everything. Have more photos somewhere else?' : undefined}
                button={backedUpTo ? 'Add another source' : undefined}
                quiet
                onClick={() => go('find')}
              />
            }
          />
        )}
      </main>
    </div>
  )
}

/** The step that still needs doing, for opening the app in the right place. */
function nextToDo(found: Discover, backedUpTo: string | null): Step {
  if (found.collections.length === 0) return 'find'
  if (found.waitingToGather > 0) return 'choose'
  return backedUpTo ? 'find' : 'backup'
}

/**
 * The bar at the foot of every screen: where you can go back to, one
 * sentence about where things stand, and the one button that moves on.
 */
function Next(props: { back?: () => void; say?: ReactNode; button?: string; onClick?: () => void; quiet?: boolean }) {
  return (
    <div className="next">
      {props.back && (
        <button className="link" onClick={props.back}>
          ‹ Back
        </button>
      )}
      <span className="say">{props.say}</span>
      {props.button && (
        <button className={props.quiet ? 'primary' : 'primary big'} onClick={props.onClick}>
          {props.button}
        </button>
      )}
    </div>
  )
}

function Welcome(props: { suggested: string; onStart: () => void; onChoose: () => void; problem: string | null }) {
  return (
    <div className="welcome">
      <Logo large />
      <h1>Let’s find your photos</h1>
      <p>
        Keevoro looks through old drives, USB sticks and folders, finds your photos and videos, and gathers the ones
        you want into one tidy place. It never changes or deletes anything it looks at.
      </p>
      <button className="primary big" onClick={props.onStart}>
        Get started
      </button>
      <p className="quiet small">
        Your photos will be gathered into a folder called “Keevoro Library” in your Pictures.{' '}
        <button className="link" onClick={props.onChoose}>
          Put it somewhere else
        </button>
      </p>
      {props.problem && (
        <div className="problem inline" role="alert">
          {props.problem}
        </div>
      )}
    </div>
  )
}

/** Step 1. The drives plugged in are offered by name; a folder is the fallback. */
function FindScreen(props: {
  api: Backend
  found: Discover | null
  scan: ScanEvent | null
  justFound: ScanEvent | null
  onProblem: (error: unknown) => void
  onStarted: () => void
  next: ReactNode
}) {
  const { api, found, scan, justFound, onProblem } = props
  const [places, setPlaces] = useState<Place[] | null>(null)

  const look = useCallback(() => {
    api.places().then(setPlaces).catch(onProblem)
  }, [api, onProblem])
  // Drives come and go, so look again whenever the window is returned to.
  useEffect(() => {
    look()
    window.addEventListener('focus', look)
    return () => window.removeEventListener('focus', look)
  }, [look])

  const start = (begin: Promise<unknown>) => {
    props.onStarted()
    begin.catch(onProblem)
  }
  const done = new Set(found?.sources.map((s) => s.root.replace(/[\\/]+$/, '')))
  const removable = places?.filter((p) => p.removable) ?? []
  const fixed = places?.filter((p) => !p.removable) ?? []

  return (
    <section className="find">
      <header>
        <h1>Where are your photos?</h1>
        <p className="status">A source is anywhere your photos might be. Plug in an old drive, USB stick or memory card, then choose it here.</p>
      </header>

      {scan ? (
        <div className="panel searching">
          <Spinner />
          <h2>Looking through {scan.name}…</h2>
          <p className="big-count">{photosAndVideos(scan.photos, scan.videos)}</p>
          <p className="quiet">found so far. Nothing on it is being changed. You can leave this running.</p>
        </div>
      ) : (
        <>
          {justFound && (
            <div className="panel found-it" role="status">
              <h2>
                Found {photosAndVideos(justFound.photos, justFound.videos)} on {justFound.name}
              </h2>
              <p className="quiet">You can add another place below, or carry on.</p>
            </div>
          )}
          <div className="places">
            {removable.map((place) => (
              <PlaceCard key={place.path} place={place} added={done.has(place.path.replace(/[\\/]+$/, ''))} onClick={() => start(api.scanPlace(place.path))} />
            ))}
            <button className="place" onClick={() => start(api.addPlace())}>
              <FolderIcon large />
              <b>Choose a folder…</b>
              <span>Anywhere on this computer or a network</span>
            </button>
            {fixed.map((place) => (
              <PlaceCard key={place.path} place={place} added={done.has(place.path.replace(/[\\/]+$/, ''))} onClick={() => start(api.scanPlace(place.path))} />
            ))}
          </div>
          {places && removable.length === 0 && (
            <p className="quiet small">No USB or external drive is plugged in right now. Plug one in and it will appear here.</p>
          )}
        </>
      )}

      {found && found.sources.length > 0 && (
        <>
          <h3>Your sources</h3>
          <ul className="kept">
            {found.sources.map((s) => (
              <li key={s.id}>
                <span>{s.name}</span>
                <span className="quiet">{s.root}</span>
                <span className="badge">{all(s.items)}</span>
              </li>
            ))}
          </ul>
        </>
      )}
      {props.next}
    </section>
  )
}

function PlaceCard(props: { place: Place; added: boolean; onClick: () => void }) {
  const { place } = props
  return (
    <button className={place.removable ? 'place likely' : 'place'} onClick={props.onClick}>
      <DriveIcon large />
      <b>{place.name}</b>
      <span>{place.detail}</span>
      {props.added && <em>Looked through. Click to look again.</em>}
    </button>
  )
}

/** Step 2. Everything is kept unless the user says otherwise. */
function ChooseScreen(props: {
  api: Backend
  found: Discover | null
  onKeep: (collection: Collection, kept: boolean) => void
  onOpen: (collection: Collection) => void
  onSetAside: () => void
  next: ReactNode
}) {
  const { found } = props
  const [hero, ...rest] = found?.collections ?? []

  return (
    <section className="discover">
      <header>
        <h1>Which of these should we keep?</h1>
        <p className="status">
          Everything is kept unless you say otherwise. Click any collection to look inside and choose photo by photo.
        </p>
      </header>

      {hero && (
        <article className={hero.kept ? 'hero' : 'hero skipped'}>
          <div
            className={`collage n${Math.min(hero.previews.length, 3)}`}
            role="button"
            tabIndex={0}
            aria-label={`Open ${hero.title}`}
            onClick={() => props.onOpen(hero)}
            onKeyDown={(e) => e.key === 'Enter' && props.onOpen(hero)}
          >
            {hero.previews.slice(0, 3).map((p) => (
              <Picture key={p.id} api={props.api} photo={p} />
            ))}
          </div>
          <div className="caption">
            <div>
              <h2>{hero.title}</h2>
              <p>
                {hero.summary}
                {hero.parents.length > 0 && <span className="parents"> · {hero.parents.join(' / ')}</span>}
              </p>
            </div>
            <KeepButton collection={hero} onKeep={props.onKeep} />
          </div>
        </article>
      )}

      {rest.length > 0 && (
        <div className="grid">
          {rest.map((c) => (
            <article className={c.kept ? 'card' : 'card skipped'} key={c.key}>
              <button className="open" onClick={() => props.onOpen(c)} aria-label={`Open ${c.title}`}>
                <Picture key={c.cover.id} api={props.api} photo={c.cover} />
              </button>
              <h4>{c.title}</h4>
              <p>{c.summary}</p>
              <KeepButton collection={c} onKeep={props.onKeep} small />
            </article>
          ))}
        </div>
      )}

      {found && hero && (found.duplicates > 0 || found.junk > 0) && (
        <p className="tidied">
          Keevoro has already tidied up:{' '}
          {found.duplicates > 0 && <span>{count(found.duplicates, 'duplicate')} will be kept only once</span>}
          {found.duplicates > 0 && found.junk > 0 && ', and '}
          {found.junk > 0 && (
            <button className="link" onClick={props.onSetAside}>
              {count(found.junk, 'picture')} that don’t look like photos were set aside
            </button>
          )}
          .
        </p>
      )}
      {props.next}
    </section>
  )
}

/** Everything is kept unless you say otherwise, so the button shows which. */
function KeepButton(props: { collection: Collection; onKeep: (c: Collection, kept: boolean) => void; small?: boolean }) {
  const { collection, small } = props
  if (collection.kept) {
    return (
      <div className={small ? 'keep small' : 'keep'}>
        <button className="toggle on" onClick={() => props.onKeep(collection, false)} aria-pressed="true">
          <span className="keeping">✓ Keeping</span>
          <span className="undo">Don’t keep</span>
        </button>
      </div>
    )
  }
  return (
    <div className={small ? 'keep small' : 'keep'}>
      <button className="toggle off" onClick={() => props.onKeep(collection, true)} aria-pressed="false">
        <span className="keeping">Not keeping</span>
        <span className="undo">Keep it</span>
      </button>
    </div>
  )
}

/** Step 3. One button, honest progress, then proof that it worked. */
function GatherScreen(props: {
  library: string
  waiting: number
  safe: number
  collections: number
  bytes: number
  gather: GatherEvent | null
  gathering: boolean
  onGather: () => void
  onOpenLibrary: () => void
  next: ReactNode
}) {
  const { gather, gathering, waiting, safe } = props
  const doing = { copying: 'safely copied', previews: 'previews made', videos: 'videos done', finished: '' }

  return (
    <section className="archive">
      <header>
        <h1>{gathering ? 'Gathering your photos…' : waiting > 0 ? 'Ready to gather your photos?' : 'Your photos are safe'}</h1>
        <p className="status">
          {waiting > 0 || gathering
            ? 'Each one is copied into your library and checked against the original. Nothing on your sources is changed.'
            : 'Each one was copied into your library and checked against its original.'}
        </p>
      </header>

      <div className="panel">
        {gathering && gather ? (
          <>
            <p className="big-count">
              {thousands(gather.done)} of {thousands(gather.total)}
            </p>
            <p>{doing[gather.step]}</p>
            <progress value={gather.done} max={Math.max(gather.total, 1)} />
            <p className="quiet">You can keep using your computer. If anything stops this, it carries on next time.</p>
          </>
        ) : waiting > 0 ? (
          <>
            <h2>{all(waiting)} to copy</h2>
            <p className="quiet">
              About {size(props.bytes)} in all, into {props.library}
            </p>
            <button className="primary big" onClick={props.onGather}>
              Gather my photos
            </button>
          </>
        ) : safe > 0 ? (
          <>
            <h2>
              {all(safe)} in {count(props.collections, 'collection')}
            </h2>
            <p className="quiet">They are in {props.library}</p>
            <button className="primary" onClick={props.onOpenLibrary}>
              Show me my photos
            </button>
          </>
        ) : (
          <>
            <h2>Nothing to gather yet</h2>
            <p>Go back and choose what to keep first.</p>
          </>
        )}
        {!gathering && gather?.summary?.map((line) => (
          <p className="done" key={line}>
            {line}
          </p>
        ))}
      </div>
      {props.next}
    </section>
  )
}

/** Step 4. A second copy somewhere else. */
function BackupScreen(props: {
  api: Backend
  safe: number
  gathering: boolean
  onProblem: (error: unknown) => void
  onBackedUpTo: (where: string | null) => void
  next: ReactNode
}) {
  return (
    <section className="archive">
      <header>
        <h1>Keep a second copy</h1>
        <p className="status">
          Disks fail. With a second copy somewhere else, your photos survive it. Keevoro never replaces a photo that is
          already backed up.
        </p>
      </header>
      {props.safe > 0 ? (
        <BackupPanel api={props.api} onProblem={props.onProblem} busy={props.gathering} onWhere={props.onBackedUpTo} />
      ) : (
        <div className="panel">
          <h2>Gather your photos first</h2>
          <p>Once they are in your library, you can back them up from here.</p>
        </div>
      )}
      {props.next}
    </section>
  )
}

/**
 * A second copy somewhere else. Until a destination is connected this asks
 * one question, "where?"; after that it is two buttons.
 */
function BackupPanel(props: {
  api: Backend
  onProblem: (error: unknown) => void
  busy: boolean
  /** Tells the rest of the app where the library is backed up to. */
  onWhere: (where: string | null) => void
}) {
  const { api, onProblem, onWhere } = props
  const [where, setWhere] = useState<string | null | undefined>(undefined)
  const [form, setForm] = useState<ConnectRequest | null>(null)
  const [connecting, setConnecting] = useState(false)
  const [refused, setRefused] = useState<string | null>(null)
  const [running, setRunning] = useState<string | null>(null)
  const [said, setSaid] = useState<BackupEvent | null>(null)

  useEffect(() => {
    api.backupState().then(setWhere).catch(onProblem)
    return api.onBackup((event) => {
      if (event.finished) {
        setRunning(null)
        setSaid(event)
      } else if (event.line) {
        setRunning(event.line)
      }
    })
  }, [api, onProblem])

  const connect = (request: ConnectRequest) => {
    setConnecting(true)
    setRefused(null)
    api
      .connectBackup(request)
      .then((description) => {
        setWhere(description)
        onWhere(description)
        setForm(null)
      })
      // A wrong key is the user's to fix, so it is said beside the form.
      .catch((error) => setRefused(error instanceof Error ? error.message : String(error)))
      .finally(() => setConnecting(false))
  }
  const start = (what: string, job: () => Promise<void>) => {
    setSaid(null)
    setRunning(what)
    job().catch((error) => {
      setRunning(null)
      onProblem(error)
    })
  }

  if (where === undefined) return null
  return (
    <div className="panel backup">
      {where === null ? (
        <>
          <h2>Where should the second copy go?</h2>
          <p>
            One copy is not a backup. Keevoro can keep another on a separate disk, or in cloud storage of your own, and
            will never replace an original that is already there.
          </p>
          {!form && (
            <div className="choices">
              <button
                className="primary"
                onClick={() =>
                  api
                    .chooseBackupFolder()
                    .then((folder) => folder && connect({ kind: 'folder', place: folder }))
                    .catch(onProblem)
                }
                disabled={connecting}
              >
                Back up to another disk…
              </button>
              <button className="link" onClick={() => setForm({ kind: 'r2', place: '', bucket: '' })}>
                Use cloud storage instead
              </button>
            </div>
          )}
          {form && (
            <form
              className="connect"
              onSubmit={(e) => {
                e.preventDefault()
                connect(form)
              }}
            >
              <label>
                Storage
                <select
                  value={form.kind}
                  onChange={(e) => setForm({ ...form, kind: e.target.value as ConnectRequest['kind'], place: '' })}
                >
                  <option value="r2">Cloudflare R2</option>
                  <option value="s3">Amazon S3</option>
                  <option value="endpoint">Another S3-compatible service</option>
                </select>
              </label>
              <label>
                {form.kind === 'r2' ? 'Cloudflare account ID' : form.kind === 's3' ? 'Region' : 'Address'}
                <input
                  value={form.place}
                  onChange={(e) => setForm({ ...form, place: e.target.value })}
                  placeholder={form.kind === 'r2' ? '32 letters and numbers' : form.kind === 's3' ? 'eu-west-2' : 'https://…'}
                  required
                />
              </label>
              <label>
                Bucket name
                <input value={form.bucket ?? ''} onChange={(e) => setForm({ ...form, bucket: e.target.value })} required />
              </label>
              <label>
                Access key ID
                <input
                  value={form.accessKeyId ?? ''}
                  onChange={(e) => setForm({ ...form, accessKeyId: e.target.value })}
                  autoComplete="off"
                  required
                />
              </label>
              <label>
                Secret access key
                <input
                  type="password"
                  value={form.secretAccessKey ?? ''}
                  onChange={(e) => setForm({ ...form, secretAccessKey: e.target.value })}
                  autoComplete="off"
                  required
                />
              </label>
              <p className="quiet">
                Use a key that can read and write this one bucket only. It is kept in your computer’s keychain, not in
                your library.
              </p>
              {refused && (
                <p className="refused" role="alert">
                  {refused}
                </p>
              )}
              <div className="choices">
                <button className="primary" type="submit" disabled={connecting}>
                  {connecting ? 'Checking…' : 'Connect'}
                </button>
                <button className="link" type="button" onClick={() => setForm(null)}>
                  Cancel
                </button>
              </div>
            </form>
          )}
          {!form && refused && (
            <p className="refused" role="alert">
              {refused}
            </p>
          )}
        </>
      ) : (
        <>
          <h2>Your backup</h2>
          <p>A second copy is kept in {where}.</p>
          {running ? (
            <p className="running">
              <Spinner /> {running}
            </p>
          ) : (
            <div className="choices">
              <button className="primary" onClick={() => start('Starting…', api.backUp)} disabled={props.busy}>
                Back up now
              </button>
              <button className="link" onClick={() => start('Checking…', api.checkBackup)} disabled={props.busy}>
                Check my backup
              </button>
              <button
                className="link quiet"
                onClick={() =>
                  api
                    .disconnectBackup()
                    .then(() => {
                      setWhere(null)
                      onWhere(null)
                      setSaid(null)
                    })
                    .catch(onProblem)
                }
              >
                Disconnect
              </button>
            </div>
          )}
          {said?.summary?.map((line) => (
            <p className="done" key={line}>
              {line}
            </p>
          ))}
          {said?.problem && (
            <p className="refused" role="alert">
              {said.problem}
            </p>
          )}
        </>
      )}
    </div>
  )
}

/**
 * The files in one collection, or the pictures that were set aside, each
 * with a tick. Ticking is the only thing this screen does.
 */
function PhotosScreen(props: {
  api: Backend
  collection: Collection | null
  onBack: () => void
  onProblem: (error: unknown) => void
}) {
  const { api, collection, onProblem } = props
  const [photos, setPhotos] = useState<PhotoChoice[] | null>(null)

  useEffect(() => {
    let current = true
    const asked = collection ? api.collectionPhotos(collection.key) : api.setAside()
    asked.then((list) => current && setPhotos(list)).catch(onProblem)
    return () => {
      current = false
    }
  }, [api, collection, onProblem])

  const toggle = (photo: PhotoChoice) => {
    const kept = !photo.kept
    // In the set-aside list, keeping a picture takes it off the list.
    setPhotos((list) =>
      list && (collection ? list.map((p) => (p.id === photo.id ? { ...p, kept } : p)) : list.filter((p) => p.id !== photo.id)),
    )
    api.keepPhoto(photo.id, kept).catch(onProblem)
  }
  const keeping = photos?.filter((p) => p.kept).length ?? 0

  return (
    <section className="photos">
      <header>
        <button className="link back" onClick={props.onBack}>
          ‹ Back
        </button>
        <h1>{collection ? collection.title : 'Set aside'}</h1>
        <p className="status">
          {collection
            ? photos
              ? `Keeping ${thousands(keeping)} of ${thousands(photos.length)}. Click a picture to leave it out or put it back.`
              : collection.summary
            : 'These look like app pictures or leftovers, not memories. Click any you want to keep after all.'}
        </p>
      </header>
      {photos && photos.length === 0 && (
        <div className="empty quiet">{collection ? 'Nothing here.' : 'Nothing is set aside.'}</div>
      )}
      <div className="tiles">
        {photos?.map((photo) => (
          <button
            key={photo.id}
            className={photo.kept ? 'tile' : 'tile out'}
            onClick={() => toggle(photo)}
            aria-pressed={photo.kept}
            aria-label={`${photo.name}, ${photo.kept ? 'keeping' : 'not keeping'}`}
          >
            <Picture api={api} photo={photo} />
            <span className="tick" aria-hidden>
              {photo.kept ? '✓' : ''}
            </span>
            {photo.junkReason && <span className="why">{photo.junkReason}</span>}
          </button>
        ))}
      </div>
    </section>
  )
}

/** One picture, fetched when first shown. A gentle placeholder until then. */
function Picture(props: { api: Backend; photo: Photo }) {
  const { api, photo } = props
  const [url, setUrl] = useState<string | null>(null)
  const [missing, setMissing] = useState(false)

  // Callers give each Picture a key of its photo's id, so a different photo
  // is a fresh component and this runs once per picture.
  useEffect(() => {
    let current = true
    api
      .preview(photo.id)
      .then((address) => current && setUrl(address))
      .catch(() => current && setMissing(true))
    return () => {
      current = false
    }
  }, [api, photo.id])

  return (
    <div className={missing ? 'picture missing' : 'picture'} title={photo.name}>
      {url && <img src={url} alt="" draggable={false} />}
      {missing && <span>{photo.isVideo ? 'Video' : 'No preview'}</span>}
      {photo.isVideo && url && <span className="play" aria-label="Video" />}
    </div>
  )
}

function Spinner() {
  return <span className="spinner" aria-hidden />
}

function Logo(props: { large?: boolean }) {
  const s = props.large ? 56 : 26
  return (
    <svg width={s} height={s} viewBox="0 0 32 32" aria-hidden>
      <rect x="9" y="5" width="19" height="17" rx="4" fill="none" stroke="currentColor" strokeWidth="2.2" opacity=".55" />
      <rect x="4" y="10" width="19" height="17" rx="4" fill="var(--panel)" stroke="currentColor" strokeWidth="2.2" />
    </svg>
  )
}

const icon = (path: string, large = false) => (
  <svg width={large ? 44 : 18} height={large ? 44 : 18} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
    <path d={path} />
  </svg>
)
const FolderIcon = (props: { large?: boolean }) => icon('M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z', props.large)
const DriveIcon = (props: { large?: boolean }) => icon('M4 14l2.5-8h11L20 14M4 14h16v5H4zM16.5 16.5h.01', props.large)
