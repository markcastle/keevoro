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

import { useCallback, useEffect, useRef, useState } from 'react'
import type { ReactNode } from 'react'
import { createPortal } from 'react-dom'

import { backend } from './backend'
import type {
  Acceleration,
  Survey,
  SurveyProgress,
  Duplicate,
  Activity,
  AppState,
  Backend,
  SendEvent,
  Collection,
  ConnectRequest,
  Discover,
  FileType,
  GatherEvent,
  Photo,
  PhotoChoice,
  Place,
  CheckEvent,
  Room,
  ScanEvent,
  SendProgress,
  Sent,
  Source,
  Backup,
  Update,
} from './backend'
import { about, count, photosAndVideos, size, thousands, wholePlace } from './format'
import lockupDarkLargeUrl from './assets/keevoro-lockup-reversed-large.png'
import lockupShadowUrl from './assets/keevoro-lockup-shadow.png'
import lockupDarkUrl from './assets/keevoro-lockup-reversed.png'
import lockupUrl from './assets/keevoro-lockup.png'
import { chooseTheme, savedTheme } from './theme'
import type { Theme } from './theme'

type Step = 'find' | 'choose' | 'send'
/** Looking inside a collection, or at what was set aside, sits on top of step 2. */
type Overlay = { collection: Collection } | 'setAside' | 'duplicates' | 'settings' | 'activity' | null

const STEPS: { id: Step; title: string }[] = [
  { id: 'find', title: 'Find your photos' },
  { id: 'choose', title: 'Choose what to keep' },
  { id: 'send', title: 'Back them up somewhere safe' },
]

const all = (n: number) => (n === 1 ? '1 photo or video' : `${n.toLocaleString()} photos and videos`)

export function App() {
  const [api, setApi] = useState<Backend | null>(null)
  const [state, setState] = useState<AppState | null>(null)
  const [found, setFound] = useState<Discover | null>(null)
  const [step, setStep] = useState<Step>('find')
  const [overlay, setOverlay] = useState<Overlay>(null)
  const [scan, setScan] = useState<ScanEvent | null>(null)
  const [justFound, setJustFound] = useState<ScanEvent | null>(null)
  const [, setGather] = useState<GatherEvent | null>(null)
  // Background checking of files a quick search only sampled.
  const [check, setCheck] = useState<CheckEvent | null>(null)
  // True while the person has paused Keevoro's heavy work.
  const [paused, setPaused] = useState(false)
  const [problem, setProblem] = useState<string | null>(null)
  // A newer Keevoro, and how far along installing it is.
  const [update, setUpdate] = useState<Update | null>(null)
  const [updating, setUpdating] = useState<'no' | 'downloading' | 'opened'>('no')
  // Where the library is backed up to; undefined until asked.
  const [sendingTo, setSendingTo] = useState<string | null | undefined>(undefined)

  // Anything that goes wrong is said once, in the engine's own plain words.
  const report = useCallback((error: unknown) => {
    setProblem(error instanceof Error ? error.message : String(error))
  }, [])

  // What Keevoro is busy with, in a line, shown in the side bar on every
  // screen. Asked every couple of seconds; it costs next to nothing.
  const [doing, setDoing] = useState<string | null>(null)
  useEffect(() => {
    if (!api) return
    let open = true
    const ask = () =>
      api
        .activity()
        .then((a) => open && setDoing(a.doing))
        .catch(() => {})
    void ask()
    const timer = window.setInterval(ask, 2000)
    return () => {
      open = false
      window.clearInterval(timer)
    }
  }, [api])

  const refresh = useCallback((from: Backend) => from.discover().then(setFound).catch(report), [report])

  useEffect(() => {
    let stopScan = () => {}
    let stopGather = () => {}
    let stopCheck = () => {}
    let stopSend = () => {}
    let closed = false
    void backend().then(async (b) => {
      if (closed) return
      setApi(b)
      // Never worth an error: no answer simply means no offer today.
      void b.checkUpdate().then(setUpdate, () => {})
      stopScan = b.onScan((event) => {
        setScan(event.finished ? null : event)
        if (event.problem) report(event.problem)
        if (event.finished) {
          setJustFound(event.problem ? null : event)
          void refresh(b)
        }
      })
      stopCheck = b.onCheck((event) => {
        setCheck(event.finished ? null : event)
        // Duplicates may have been confirmed or ruled out, so count again.
        if (event.finished && event.done > 0) void refresh(b)
      })
      // A backup can finish while another screen is showing, so the counts
      // are read again here and not only by the screen that started it.
      stopSend = b.onSend((event) => {
        if (event.finished) void refresh(b)
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
          const where = await b.destination()
          setSendingTo(where)
          // Open on whichever step still needs doing.
          setStep(nextToDo(d, where))
          void b.keepChecking().catch(() => {})
        }
      } catch (error) {
        report(error)
      }
    })
    return () => {
      closed = true
      stopScan()
      stopGather()
      stopCheck()
      stopSend()
    }
  }, [refresh, report])

  if (!api || !state) return <div className="blank" />

  // Opening a backup (a new one, or another one) shows what it holds.
  const openBackup = (opened: Promise<AppState>) =>
    opened
      .then(async (s) => {
        setState(s)
        setOverlay(null)
        setJustFound(null)
        const d = await api.discover()
        setFound(d)
        const where = await api.destination()
        setSendingTo(where)
        setStep(nextToDo(d, where))
      })
      .catch(report)

  if (!state.backup) {
    return <Welcome onStart={(name) => openBackup(api.startBackup(name))} problem={problem} />
  }

  const waiting = found?.waitingToGather ?? 0
  const kept = found?.collections.filter((c) => c.kept) ?? []
  const safe = kept.reduce((n, c) => n + c.gathered, 0)
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

  // What each step has to say for itself in the list down the side.
  const done: Record<Step, boolean> = {
    find: anythingFound,
    choose: anythingFound && (step === 'send' || safe > 0),
    send: safe > 0 && waiting === 0,
  }
  const note: Record<Step, string> = {
    find: found && found.sources.length > 0 ? count(found.sources.length, 'source') : 'Start here',
    choose: anythingFound ? `Keeping ${thousands(waiting + safe)}` : '',
    send: safe > 0 ? `${thousands(safe)} safe` : sendingTo ? 'Ready to send' : '',
  }
  // A step can be opened once the one before it has something to show.
  const open: Record<Step, boolean> = { find: true, choose: anythingFound, send: anythingFound }

  return (
    <div className="app">
      <aside className="sidebar">
        <div className="brand">
          <Logo />
        </div>
        <BackupPicker
          backup={state.backup}
          backups={state.backups}
          onSwitch={(id) => openBackup(api.switchBackup(id))}
          onStart={(name) => openBackup(api.startBackup(name))}
        />
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
          {check && (
            <span className="checking" role="status">
              <Spinner /> Checking for duplicates in the background: {thousands(check.done)} of {thousands(check.total)}.
              You can carry on.
            </span>
          )}
          {paused && (
            <span className="paused" role="status">
              Keevoro is paused.{' '}
              <button
                className="button small"
                onClick={() =>
                  api
                    .pauseWork(false)
                    .then(() => setPaused(false))
                    .catch(report)
                }
              >
                Carry on
              </button>
            </span>
          )}
          <button className="link doing" onClick={() => setOverlay('activity')}>
            What Keevoro is doing
            {doing && !paused && (
              <span className="quiet small" role="status">
                <Spinner /> {doing}
              </span>
            )}
          </button>
          <button className="link" onClick={() => setOverlay('settings')}>
            Settings
          </button>
          <span>Your sources are only ever read.</span>
        </div>
      </aside>

      <main>
        {update && (
          <div className="update" role="status">
            {updating === 'no' && (
              <>
                <span>
                  <strong>A new version of Keevoro is ready</strong> ({update.version}). Updating takes about a minute and
                  your library is not affected.
                </span>
                <span className="actions">
                  <button className="link" onClick={() => api.openReleasePage(update.page).catch(report)}>
                    What’s new
                  </button>
                  <button onClick={() => setUpdate(null)}>Later</button>
                  <button
                    className="primary"
                    onClick={() => {
                      setUpdating('downloading')
                      api
                        .installUpdate()
                        .then(() => setUpdating('opened'))
                        .catch((e) => {
                          setUpdating('no')
                          report(e)
                        })
                    }}
                  >
                    Update now
                  </button>
                </span>
              </>
            )}
            {updating === 'downloading' && (
              <span>
                <span className="spinner" /> Downloading the new version. Keevoro will close and reopen by itself.
              </span>
            )}
            {updating === 'opened' && (
              <span>The new version has been downloaded and opened. Follow the window that appeared to finish.</span>
            )}
          </div>
        )}
        {problem && (
          <div className="problem" role="alert">
            <span>{problem}</span>
            <button onClick={() => setProblem(null)} aria-label="Dismiss">
              ×
            </button>
          </div>
        )}

        {overlay === 'activity' ? (
          <ActivityScreen api={api} onPaused={setPaused} onBack={() => setOverlay(null)} onProblem={report} />
        ) : overlay === 'settings' ? (
          <SettingsScreen
            api={api}
            state={state}
            sendingTo={sendingTo ?? null}
            update={update}
            onUpdate={setUpdate}
            onLibrary={(s) => {
              setState(s)
              void refresh(api)
            }}
            onSend={() => {
              setOverlay(null)
              setStep('send')
            }}
            onReset={(s) => {
              setOverlay(null)
              setFound(null)
              setSendingTo(null)
              setJustFound(null)
              setStep('find')
              setState(s)
            }}
            onBack={() => setOverlay(null)}
            onProblem={report}
          />
        ) : overlay === 'duplicates' ? (
          <DuplicatesScreen api={api} onBack={() => setOverlay(null)} onProblem={report} />
        ) : overlay ? (
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
            onChanged={() => {
              setJustFound(null)
              void refresh(api)
            }}
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
            onDuplicates={() => setOverlay('duplicates')}
            next={
              waiting > 0 ? (
                <Next
                  back={() => go('find')}
                  say={
                    <>
                      Keeping <b>{all(waiting + safe)}</b> in {count(kept.length, 'collection')}.
                    </>
                  }
                  button="Choose where they go"
                  onClick={() => go('send')}
                />
              ) : safe > 0 ? (
                <Next back={() => go('find')} say="Everything you’re keeping has already been backed up." button="Next" onClick={() => go('send')} />
              ) : (
                <Next back={() => go('find')} say="Nothing is being kept yet. Choose at least one collection." />
              )
            }
          />
        ) : (
          <SendScreen
            api={api}
            waiting={waiting}
            safe={safe}
            onProblem={report}
            onSendingTo={setSendingTo}
            onSent={() => void refresh(api)}
            next={
              <Next
                back={() => go('choose')}
                say={safe > 0 && waiting === 0 ? 'That’s everything. Have more photos somewhere else?' : undefined}
                button={safe > 0 && waiting === 0 ? 'Add another source' : undefined}
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
function nextToDo(found: Discover, sendingTo: string | null): Step {
  if (found.collections.length === 0) return 'find'
  // Somewhere to send to is already chosen: sending is what is left.
  if (found.waitingToGather > 0) return sendingTo ? 'send' : 'choose'
  return 'find'
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

function Welcome(props: { onStart: (name: string) => void; problem: string | null }) {
  const [name, setName] = useState('My photos')
  return (
    <form
      className="welcome"
      onSubmit={(e) => {
        e.preventDefault()
        props.onStart(name)
      }}
    >
      <Logo large />
      <h1>Let’s find your photos</h1>
      <p>
        Keevoro looks through drives, memory cards and folders, finds your photos and videos, and sends the ones you
        want somewhere safe: a folder, another disk, or cloud storage. It never changes or deletes anything it looks at.
      </p>
      <label className="naming">
        What shall we call this backup?
        <input value={name} onChange={(e) => setName(e.target.value)} maxLength={60} autoFocus />
      </label>
      <button className="primary big" type="submit">
        Get started
      </button>
      <p className="quiet small">
        A name such as “Family photos” or “Drone footage”. You can have several backups, each with its own sources
        and its own place to send things, and come back to any of them later.
      </p>
      {props.problem && (
        <div className="problem inline" role="alert">
          {props.problem}
        </div>
      )}
    </form>
  )
}

/** The open backup's name, and the way to another one or a new one. */
function BackupPicker(props: {
  backup: Backup | null
  backups: Backup[]
  onSwitch: (id: string) => void
  onStart: (name: string) => void
}) {
  const [open, setOpen] = useState(false)
  const [naming, setNaming] = useState<string | null>(null)
  const others = props.backups.filter((s) => s.id !== props.backup?.id)
  const close = () => {
    setOpen(false)
    setNaming(null)
  }
  return (
    <div className="backup">
      <button className="current" onClick={() => (open ? close() : setOpen(true))} aria-expanded={open} aria-label="Backup">
        <span>
          <small>Backup</small>
          {props.backup?.name}
        </span>
        <span aria-hidden>{open ? '▴' : '▾'}</span>
      </button>
      {open && (
        <div className="menu">
          {others.map((s) => (
            <button
              key={s.id}
              onClick={() => {
                close()
                props.onSwitch(s.id)
              }}
            >
              {s.name}
            </button>
          ))}
          {naming === null ? (
            <button className="button" onClick={() => setNaming('')}>
              Start a new backup…
            </button>
          ) : (
            <form
              onSubmit={(e) => {
                e.preventDefault()
                const name = naming
                close()
                props.onStart(name)
              }}
            >
              <input
                value={naming}
                onChange={(e) => setNaming(e.target.value)}
                placeholder="Such as Drone footage"
                aria-label="Name for the new backup"
                maxLength={60}
                autoFocus
              />
              <button className="primary" type="submit">
                Start
              </button>
            </form>
          )}
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
  onChanged: () => void
  next: ReactNode
}) {
  const { api, found, scan, justFound, onProblem } = props
  // A search passes through folders far faster than anyone can read, so the
  // one named on screen changes only every second and a half.
  const folder = useSteady(scan?.folder, 1500)
  const [places, setPlaces] = useState<Place[] | null>(null)
  // The source the person has asked to remove, until they confirm.
  const [removing, setRemoving] = useState<number | null>(null)
  // A whole disk the person picked, until they say they mean all of it.
  const [asking, setAsking] = useState<{ path: string; name: string; detail?: string } | null>(null)
  // The source whose kinds of file are being looked at, if any.
  const [typesOf, setTypesOf] = useState<number | null>(null)
  const [stopping, setStopping] = useState(false)
  // A quick survey of a drive: where it is, how far it has got, and then
  // the folders it suggests, with a tick beside each one to look through.
  const [survey, setSurvey] = useState<{
    name: string
    progress: SurveyProgress | null
    found: Survey | null
    chosen: Set<string>
  } | null>(null)
  // Folders waiting their turn to be looked through, one after another.
  const [queue, setQueue] = useState<string[]>([])
  useEffect(() => api.onSurvey((progress) => setSurvey((s) => (s && !s.found ? { ...s, progress } : s))), [api])

  const look = useCallback(() => {
    api.places().then(setPlaces).catch(onProblem)
  }, [api, onProblem])
  // Drives come and go, so look again whenever the window is returned to.
  useEffect(() => {
    look()
    window.addEventListener('focus', look)
    return () => window.removeEventListener('focus', look)
  }, [look])

  const findFolders = (place: { path: string; name: string }) => {
    setAsking(null)
    setSurvey({ name: place.name, progress: null, found: null, chosen: new Set() })
    api
      .surveyPlace(place.path)
      .then((found) =>
        setSurvey((s) =>
          s && {
            ...s,
            found,
            // Ticked to begin with: the real photos not already added.
            chosen: new Set(found.hints.filter((h) => h.likelyPhotos && !h.alreadyASource).map((h) => h.folder)),
          },
        ),
      )
      .catch((error) => {
        setSurvey(null)
        onProblem(error)
      })
  }
  // A search has ended when one was showing and now is not. The next
  // folder in the queue, if there is one, starts then.
  const searching = scan !== null
  const wasSearching = useRef(false)
  useEffect(() => {
    if (searching) {
      wasSearching.current = true
      return
    }
    if (!wasSearching.current) return
    wasSearching.current = false
    if (stopping) {
      setQueue([])
      return
    }
    const [next, ...rest] = queue
    if (next === undefined) return
    setQueue(rest)
    props.onStarted()
    api.scanPlace(next).catch(onProblem)
    // Only the end of a search moves the queue on.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [searching])
  const lookThroughChosen = () => {
    if (!survey?.found) return
    const [first, ...rest] = survey.found.hints.map((h) => h.folder).filter((f) => survey.chosen.has(f))
    setSurvey(null)
    if (first === undefined) return
    setQueue(rest)
    start(first)
  }

  const start = (path: string) => {
    setAsking(null)
    // Each search starts afresh: a Stop pressed last time does not linger.
    setStopping(false)
    props.onStarted()
    api.scanPlace(path).catch(onProblem)
  }
  // Asks for a folder, then asks again if it turns out to be a whole drive.
  const pick = () => {
    api
      .pickPlace()
      .then((path) => {
        if (!path) return
        if (wholePlace(path)) setAsking({ path, name: path })
        else start(path)
      })
      .catch(onProblem)
  }
  const choose = (place: Place) => {
    if (place.askFirst) setAsking({ path: place.path, name: place.name, detail: place.detail })
    else start(place.path)
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
          {!scan.saving && (
            <p className="choices stop">
              {queue.length > 0 && (
                <span className="quiet small">
                  {count(queue.length, 'more folder')} to look through after this.
                </span>
              )}
              <button
                disabled={stopping}
                onClick={() => {
                  setStopping(true)
                  api.stopScanning().catch(onProblem)
                }}
              >
                {stopping ? 'Stopping…' : 'Stop looking'}
              </button>
            </p>
          )}
          {scan.saving ? (
            <p className="quiet small doing">Finished looking. Saving what was found…</p>
          ) : scan.reading ? (
            <p className="quiet small doing">
              Reading a large file ({size(scan.reading[1])}), which can take a few minutes:{' '}
              <span className="where">{scan.reading[0]}</span>
            </p>
          ) : null}
          {scan.files !== undefined && !scan.saving && (
            <p className="quiet small doing" aria-live="off">
              Looked at {count(scan.files, 'file')} so far
              {(folder ?? scan.folder) && (
                <>
                  . Now in <span className="where">{folder ?? scan.folder}</span>
                </>
              )}
            </p>
          )}
        </div>
      ) : survey && !survey.found ? (
        <div className="panel searching">
          <Spinner />
          <h2>Finding where the photos are on {survey.name}…</h2>
          <p className="big-count">
            {survey.progress ? photosAndVideos(survey.progress.photos, survey.progress.videos) : 'Starting…'}
          </p>
          <p className="quiet">
            spotted so far, by name. This only reads the list of what is in each folder. No file is opened.
          </p>
          <p className="choices stop">
            <button onClick={() => api.stopSurvey().catch(onProblem)}>Stop and show what was found</button>
          </p>
          {survey.progress && (
            <p className="quiet small doing" aria-live="off">
              Looked at {count(survey.progress.files, 'file')} in {count(survey.progress.folders, 'folder')} so far.
            </p>
          )}
        </div>
      ) : survey?.found ? (
        <SurveyResult
          name={survey.name}
          found={survey.found}
          chosen={survey.chosen}
          onToggle={(folder) =>
            setSurvey(
              (s) =>
                s && {
                  ...s,
                  chosen: new Set(s.chosen.has(folder) ? [...s.chosen].filter((f) => f !== folder) : [...s.chosen, folder]),
                },
            )
          }
          onGo={lookThroughChosen}
          onCancel={() => setSurvey(null)}
        />
      ) : asking ? (
        <div className="panel asking" role="alertdialog" aria-labelledby="asking-title">
          <h2 id="asking-title">Look through all of {asking.name}?</h2>
          <p>
            That is a whole disk{asking.detail ? ` (${asking.detail.split(' · ').pop()})` : ''}, and most of what is on
            it will not be photos. It can take anything from a few minutes to an hour or more. You can leave it
            running, and nothing on it is changed.
          </p>
          <p className="quiet">
            Not sure where your photos are? Keevoro can find the folders that hold them first. That takes seconds, not
            hours, because it only reads the list of what is in each folder and opens nothing.
          </p>
          <p className="choices">
            <button className="primary" onClick={() => findFolders(asking)}>
              Find the folders with photos
            </button>
            <button onClick={pick}>Choose a folder inside it</button>
            <button onClick={() => start(asking.path)}>Look through all of it</button>
            <button className="button" onClick={() => setAsking(null)}>
              Cancel
            </button>
          </p>
        </div>
      ) : (
        <>
          {justFound && (
            <div className={justFound.stopped ? 'panel' : 'panel found-it'} role="status">
              {justFound.stopped ? (
                <>
                  <h2>Stopped looking through {justFound.name}</h2>
                  <p className="quiet">
                    Nothing from it was added, because only part of it had been looked at. Choose it again whenever
                    you like, or choose a folder inside it to look through less.
                  </p>
                </>
              ) : (
                <>
                  <h2>
                    Found {photosAndVideos(justFound.photos, justFound.videos)} on {justFound.name}
                  </h2>
                  <p className="quiet">You can add another place below, or carry on.</p>
                </>
              )}
            </div>
          )}
          <div className="places">
            {removable.map((place) => (
              <PlaceCard key={place.path} place={place} added={done.has(place.path.replace(/[\\/]+$/, ''))} onClick={() => choose(place)} />
            ))}
            <button className="place" onClick={pick}>
              <FolderIcon large />
              <b>Choose a folder…</b>
              <span>Anywhere on this computer or a network</span>
            </button>
            {fixed.map((place) => (
              <PlaceCard key={place.path} place={place} added={done.has(place.path.replace(/[\\/]+$/, ''))} onClick={() => choose(place)} />
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
          <p className="quiet small">Removing a source only takes it off this list. Keevoro never changes or deletes anything in a source.</p>
          <ul className="kept">
            {found.sources.map((s) => (
              <li key={s.id} className="source">
                <span>{s.name}</span>
                <FolderLink api={api} path={s.root} onProblem={onProblem} />
                <span className="badge">
                  {all(s.items)} · {size(s.bytes)}
                </span>
                {removing === s.id ? (
                  <span className="confirm">
                    <button
                      className="danger"
                      onClick={() => {
                        setRemoving(null)
                        api.forgetSource(s.id).then(props.onChanged).catch(onProblem)
                      }}
                    >
                      Remove from Keevoro
                    </button>
                    <button onClick={() => setRemoving(null)}>Cancel</button>
                  </span>
                ) : (
                  <span className="row-actions">
                    <button
                      className="button small"
                      onClick={() => setTypesOf(typesOf === s.id ? null : s.id)}
                      aria-expanded={typesOf === s.id}
                      aria-label={`File types on ${s.name}`}
                    >
                      File types
                    </button>
                    <button className="button small" onClick={() => setRemoving(s.id)} aria-label={`Remove ${s.name}`}>
                      Remove
                    </button>
                  </span>
                )}
                {typesOf === s.id && removing !== s.id && (
                  <FileTypes api={api} source={s} onChanged={props.onChanged} onProblem={onProblem} />
                )}
                {removing === s.id && (
                  <p className="quiet small explain">
                    Keevoro will forget what it found in {s.name}. Nothing in {s.name} itself is changed or deleted, and
                    photos you have already gathered stay in your library. You can add it again at any time.
                  </p>
                )}
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

/**
 * For people who want it: the kinds of file found on one source, each with
 * a switch. It stays folded away, so adding a source is still one click
 * for everyone else.
 */
function FileTypes(props: { api: Backend; source: Source; onChanged: () => void; onProblem: (error: unknown) => void }) {
  const { api, source, onProblem } = props
  const [kinds, setKinds] = useState<FileType[] | null>(null)
  useEffect(() => {
    api.fileTypes(source.id).then(setKinds).catch(onProblem)
  }, [api, source.id, onProblem])
  if (!kinds) return null
  return (
    <div className="types explain">
      <p className="quiet small">
        Everything Keevoro found on {source.name} is included. Switch off a kind of file to leave all of those out of
        this backup; nothing on the source is changed, and you can switch it back on at any time.
      </p>
      <ul>
        {kinds.map((kind) => (
          <li key={kind.ending}>
            <button
              className={kind.included ? 'kv-switch on' : 'kv-switch'}
              role="switch"
              aria-checked={kind.included}
              aria-label={`Include ${kind.ending ? kind.ending.toUpperCase() : 'unnamed'} files`}
              onClick={() => {
                const included = !kind.included
                setKinds(kinds.map((k) => (k.ending === kind.ending ? { ...k, included } : k)))
                api.includeFileType(source.id, kind.ending, included).then(props.onChanged).catch(onProblem)
              }}
            >
              <span className="knob" />
            </button>
            <b>{kind.ending ? kind.ending.toUpperCase() : 'No ending'}</b>
            <span className="quiet">
              {count(kind.files, 'file')} · {size(kind.bytes)}
              {!kind.included && ' · left out'}
            </span>
          </li>
        ))}
      </ul>
    </div>
  )
}

/**
 * A folder's name that opens the folder when clicked. Anywhere Keevoro
 * names a folder, the person can go and look at it.
 */
function FolderLink(props: { api: Backend; path: string; onProblem: (error: unknown) => void }) {
  return (
    <button
      className="folder"
      title="Open this folder"
      onClick={(e) => {
        e.stopPropagation()
        props.api.openPlace(props.path).catch(props.onProblem)
      }}
    >
      {props.path}
    </button>
  )
}

/**
 * What Keevoro is doing to this computer, and why, with a way to make it
 * wait. Nobody should have to wonder why their fan is spinning.
 */
function ActivityScreen(props: {
  api: Backend
  onPaused: (paused: boolean) => void
  onBack: () => void
  onProblem: (error: unknown) => void
}) {
  const { api, onPaused, onProblem } = props
  const [now, setNow] = useState<Activity | null>(null)

  // Measured once a second, for as long as this page is open.
  useEffect(() => {
    let open = true
    const look = () =>
      api
        .activity()
        .then((a) => {
          if (!open) return
          setNow(a)
          onPaused(a.paused)
        })
        .catch(onProblem)
    void look()
    const timer = window.setInterval(look, 1000)
    return () => {
      open = false
      window.clearInterval(timer)
    }
  }, [api, onPaused, onProblem])

  if (!now) return <section className="activity" />
  const pause = (paused: boolean) =>
    api
      .pauseWork(paused)
      .then(() => {
        setNow({ ...now, paused })
        onPaused(paused)
      })
      .catch(onProblem)
  const busy = now.jobs.length > 0

  return (
    <section className="activity">
      <header>
        <button className="link" onClick={props.onBack}>
          ‹ Back
        </button>
        <h1>What Keevoro is doing</h1>
        <p className="status">
          {now.paused
            ? 'Paused. Keevoro is waiting and using almost nothing.'
            : busy
              ? `${now.doing ?? 'Working'}. Here is why, and how much of your computer it is using.`
              : 'Nothing at the moment. Keevoro is idle.'}
        </p>
      </header>

      <div className="panel">
        {busy ? (
          <ul className="jobs">
            {now.jobs.map((job) => (
              <li key={job.what}>
                <h2>
                  {!now.paused && <Spinner />} {job.what}
                  {now.paused && <span className="badge">Paused</span>}
                </h2>
                <p>{job.why}</p>
              </li>
            ))}
          </ul>
        ) : (
          <p>
            Keevoro works when you ask it to look through a source or send your files, and for a while afterwards while
            it checks for duplicates. Then it stops.
          </p>
        )}
        <p className="choices">
          {now.paused ? (
            <button className="primary" onClick={() => pause(false)}>
              Carry on
            </button>
          ) : (
            <button className="primary" onClick={() => pause(true)} disabled={!busy}>
              Pause Keevoro’s work
            </button>
          )}
        </p>
        <p className="quiet small">
          Pausing makes every job wait once the file in hand is finished, and Carry on picks up exactly there. Nothing
          is lost or done twice. Use it when you need the computer for something else, or are on battery.
        </p>
      </div>

      <div className="usage">
        <UsageChart
          title="Processor"
          unit="%"
          about={`Keevoro’s share of all ${now.cores} of this computer’s processor cores.`}
          values={now.samples.map((s) => s.cpu)}
          floor={25}
        />
        <UsageChart
          title="Disk"
          unit=" MB/s"
          about="How much Keevoro is reading and writing each second."
          values={now.samples.map((s) => s.disk)}
          floor={10}
        />
        <UsageChart
          title="Network"
          unit=" MB/s"
          about="What Keevoro is sending to cloud storage each second. Backing up to a folder shows under Disk."
          values={now.samples.map((s) => s.network)}
          floor={5}
        />
        <UsageChart
          title="Memory"
          unit=" MB"
          about="How much of this computer’s memory Keevoro is holding. It should stay level, not climb."
          values={now.samples.map((s) => s.memory)}
          floor={256}
        />
      </div>
      <p className="quiet small">These are Keevoro’s own figures, not the whole computer’s.</p>
    </section>
  )
}

/** How long the charts look back, in seconds. */
const WINDOW = 120

/** A round number a little above `peak`, so the line never touches the top. */
function ceiling(peak: number, floor: number) {
  const steps = [5, 10, 25, 50, 100, 150, 200, 300, 500, 1000, 2000]
  return steps.find((s) => s >= Math.max(peak * 1.15, floor)) ?? Math.ceil(peak / 1000) * 1000
}

/**
 * One measure over the last two minutes: a single thin line on a quiet
 * grid, with the figure under the pointer shown as you move across it.
 */
function UsageChart(props: { title: string; unit: string; about: string; values: number[]; floor: number }) {
  const { values, unit } = props
  const [hover, setHover] = useState<number | null>(null)
  const W = 520
  const H = 160
  const pad = { left: 44, right: 10, top: 10, bottom: 22 }
  const top = ceiling(Math.max(0, ...values), props.floor)
  const x = (i: number) => pad.left + ((WINDOW - values.length + i) / (WINDOW - 1)) * (W - pad.left - pad.right)
  const y = (v: number) => pad.top + (1 - Math.min(v, top) / top) * (H - pad.top - pad.bottom)
  const said = (v: number) => `${v >= 10 ? Math.round(v) : v.toFixed(1)}${unit}`
  const latest = values.length > 0 ? values[values.length - 1] : 0
  const peak = values.length > 0 ? Math.max(...values) : 0
  const line = values.map((v, i) => `${i === 0 ? 'M' : 'L'}${x(i).toFixed(1)},${y(v).toFixed(1)}`).join(' ')
  const at = hover !== null && hover < values.length ? hover : null

  return (
    <figure className="chart">
      <figcaption>
        <b>{props.title}</b>
        <span className="now">{said(latest)}</span>
        <span className="quiet small">
          now · highest {said(peak)} in the last {values.length < WINDOW ? `${values.length} seconds` : 'two minutes'}
        </span>
      </figcaption>
      <svg
        viewBox={`0 0 ${W} ${H}`}
        role="img"
        aria-label={`${props.title}: ${said(latest)} now, highest ${said(peak)}. ${props.about}`}
        onMouseLeave={() => setHover(null)}
        onMouseMove={(e) => {
          const box = e.currentTarget.getBoundingClientRect()
          const px = ((e.clientX - box.left) / box.width) * W
          const i = Math.round(((px - pad.left) / (W - pad.left - pad.right)) * (WINDOW - 1)) - (WINDOW - values.length)
          setHover(i >= 0 && i < values.length ? i : null)
        }}
      >
        {[0, 0.5, 1].map((f) => (
          <g key={f}>
            <line className="grid" x1={pad.left} x2={W - pad.right} y1={y(top * f)} y2={y(top * f)} />
            <text className="tick" x={pad.left - 8} y={y(top * f) + 4} textAnchor="end">
              {Math.round(top * f)}
            </text>
          </g>
        ))}
        <text className="tick" x={pad.left} y={H - 4}>
          2 min ago
        </text>
        <text className="tick" x={W - pad.right} y={H - 4} textAnchor="end">
          now
        </text>
        {values.length > 1 && <path className="line" d={line} />}
        {at !== null && (
          <g>
            <line className="cross" x1={x(at)} x2={x(at)} y1={pad.top} y2={H - pad.bottom} />
            <circle className="dot" cx={x(at)} cy={y(values[at])} r="4" />
          </g>
        )}
      </svg>
      <p className="quiet small read" aria-live="off">
        {at !== null
          ? `${said(values[at])}, ${values.length - 1 - at === 0 ? 'just now' : `${values.length - 1 - at} seconds ago`}`
          : props.about}
      </p>
    </figure>
  )
}

/** Everything that can be changed or looked up, in one place. */
function SettingsScreen(props: {
  api: Backend
  state: AppState
  sendingTo: string | null
  update: Update | null
  onUpdate: (update: Update | null) => void
  onLibrary: (state: AppState) => void
  /** Called after a reset, with the app as it is when first installed. */
  onReset: (state: AppState) => void
  onSend: () => void
  onBack: () => void
  onProblem: (error: unknown) => void
}) {
  const { api, state, onProblem } = props
  const [log, setLog] = useState<string | null>(null)
  const [checked, setChecked] = useState<'no' | 'checking' | 'current'>('no')
  const [theme, setTheme] = useState<Theme>(savedTheme)
  const [speed, setSpeed] = useState<Acceleration | null>(null)
  useEffect(() => {
    api.acceleration().then(setSpeed).catch(onProblem)
  }, [api, onProblem])
  // Resetting asks first, and says exactly what it does and does not do.
  const [resetting, setResetting] = useState(false)
  const logBox = useRef<HTMLPreElement>(null)

  const showLog = () =>
    api
      .logTail()
      .then((text) => {
        setLog(text)
        // The newest lines are at the bottom, which is what people want.
        requestAnimationFrame(() => logBox.current?.scrollTo(0, logBox.current.scrollHeight))
      })
      .catch(onProblem)

  return (
    <section className="settings">
      <header>
        <button className="link" onClick={props.onBack}>
          ‹ Back
        </button>
        <h1>Settings</h1>
      </header>

      <div className="setting">
        <h3>This backup</h3>
        <p>
          This is <b>{state.backup?.name}</b>. A backup remembers its own sources, what you chose to keep and where
          it sends things. To open another or start a new one, use the backup’s name at the top of the side bar.
        </p>
        <p className="quiet small">
          Its notes and previews are kept in{' '}
          {state.library && <FolderLink api={api} path={state.library} onProblem={onProblem} />}. Your photos themselves
          are not copied there.
        </p>
      </div>

      <div className="setting">
        <h3>Where it backs up to</h3>
        <p>{props.sendingTo ? `This backup sends to ${props.sendingTo}.` : 'Nowhere has been chosen yet.'}</p>
        <p className="choices">
          <button onClick={props.onSend}>{props.sendingTo ? 'Go to sending' : 'Choose where'}</button>
        </p>
      </div>

      <div className="setting">
        <h3>Appearance</h3>
        <p className="quiet">Light, dark, or the same as the rest of your computer.</p>
        <p className="choices">
          <span className="themes" role="group" aria-label="Appearance">
            {(
              [
                ['system', 'Match my computer'],
                ['light', 'Light'],
                ['dark', 'Dark'],
              ] as [Theme, string][]
            ).map(([value, label]) => (
              <button
                key={value}
                className="link"
                aria-pressed={theme === value}
                onClick={() => {
                  chooseTheme(value)
                  setTheme(value)
                }}
              >
                {label}
              </button>
            ))}
          </span>
        </p>
      </div>

      <div className="setting">
        <h3>Speed</h3>
        {speed && (
          <>
            <label className="switch-row">
              <button
                className={speed.on && speed.available ? 'kv-switch on' : 'kv-switch'}
                role="switch"
                aria-checked={speed.on && speed.available}
                aria-label="Use this computer’s own picture codecs"
                disabled={!speed.available}
                onClick={() => api.setAcceleration(!speed.on).then(setSpeed).catch(onProblem)}
              >
                <span className="knob" />
              </button>
              <span>
                <b>Use this computer’s own picture codecs</b>
                <span className="quiet small">
                  {speed.available
                    ? 'They are usually the fastest way to open a photo, use your graphics hardware where they can, and open kinds of photo Keevoro’s own decoder cannot, such as HEIC. If a photo will not open that way, Keevoro falls back to its own decoder by itself. Switch this off only if previews look wrong.'
                    : 'On this kind of computer Keevoro uses its own decoder for photos, so there is nothing to switch.'}
                </span>
              </span>
            </label>
            <p className="quiet small">
              Photos are opened by: {speed.ways.join(', then ')}. Previews are made {speed.cores} at a time, on half of
              this computer’s processor cores at most, so the rest stays free for you.
            </p>
          </>
        )}
      </div>

      <div className="setting">
        <h3>Updates</h3>
        <p>
          This is Keevoro {state.version}.{' '}
          {props.update
            ? `Version ${props.update.version} is ready; see the bar at the top.`
            : checked === 'current'
              ? 'That is the newest version.'
              : ''}
        </p>
        <p className="choices">
          <button
            disabled={checked === 'checking'}
            onClick={() => {
              setChecked('checking')
              api
                .checkUpdate()
                .then((found) => {
                  props.onUpdate(found)
                  setChecked(found ? 'no' : 'current')
                })
                .catch((e) => {
                  setChecked('no')
                  onProblem(e)
                })
            }}
          >
            {checked === 'checking' ? 'Checking…' : 'Check for updates'}
          </button>
        </p>
      </div>

      <div className="setting">
        <h3>What Keevoro has been doing</h3>
        <p className="quiet">
          Keevoro keeps a plain record of what it did and when: every search, anything it could not read, gathering and
          backups. It never records keys or passwords. If something seems slow or wrong, this is where to look.
        </p>
        <p className="choices">
          <button onClick={showLog}>{log === null ? 'Show the log' : 'Refresh'}</button>
          <button className="button" onClick={() => api.openLog().catch(onProblem)}>
            Open the log file
          </button>
        </p>
        {log !== null && (
          <pre className="log" ref={logBox} tabIndex={0}>
            {log || 'Nothing has been recorded yet.'}
          </pre>
        )}
      </div>

      <div className="setting about">
        <h3>About Keevoro</h3>
        <p className="motto">Keep things as they were.</p>
        <p>
          The name blends the sound of English <i>keep</i> with <i>voro</i>, an old Swedish word meaning “were”.
          Together they capture why we save photos and videos in the first place: to remember people and places as
          they were.
        </p>
        <p className="quiet small">Version {state.version}</p>
      </div>

      <div className="setting">
        <h3>Start again</h3>
        <p className="quiet">
          Put Keevoro back as it was when you first installed it. Use this to clear out a trial run, or before passing
          the computer on.
        </p>
        {!resetting ? (
          <p className="choices">
            <button className="danger" onClick={() => setResetting(true)}>
              Reset Keevoro…
            </button>
          </p>
        ) : (
          <div className="resetting" role="alertdialog" aria-labelledby="reset-title">
            <h4 id="reset-title">Reset Keevoro?</h4>
            <p>
              <b>This removes:</b>{' '}
              {props.state.backups.length === 1 ? 'your backup' : `all ${props.state.backups.length} of your backups`} from
              Keevoro, with what was found and what you chose to keep; the previews Keevoro made; and any storage
              keys it saved.
            </p>
            <p>
              <b>This does not touch:</b> anything on your sources, or any photo or video you have already sent
              anywhere. Those stay exactly where they are. Keevoro simply forgets about them.
            </p>
            <p className="choices">
              <button
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
              <button onClick={() => setResetting(false)}>Cancel</button>
            </p>
          </div>
        )}
      </div>
    </section>
  )
}

/** Step 2. Everything is kept unless the user says otherwise. */
function ChooseScreen(props: {
  api: Backend
  found: Discover | null
  onKeep: (collection: Collection, kept: boolean) => void
  onOpen: (collection: Collection) => void
  onSetAside: () => void
  onDuplicates: () => void
  next: ReactNode
}) {
  const { found } = props
  const [hero, ...rest] = found?.collections ?? []

  return (
    <section className="discover">
      <header>
        <h1>Which of these should we keep?</h1>
        <p className="status">
          Keevoro has sorted what it found into collections: by folder name where your folders have names, and by
          month where they don’t. So one source often makes several. Everything is kept unless you say otherwise;
          click a collection to look inside and choose photo by photo.
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
          {found.duplicates > 0 && (
            <button className="link" onClick={props.onDuplicates}>
              {count(found.duplicates, 'duplicate')} will be kept only once
            </button>
          )}
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

/** Step 3. One question: where should they go? Then one button: send. */
function SendScreen(props: {
  api: Backend
  waiting: number
  safe: number
  onProblem: (error: unknown) => void
  onSendingTo: (where: string | null) => void
  onSent: () => void
  next: ReactNode
}) {
  return (
    <section className="archive">
      <header>
        <h1>Where should they go?</h1>
        <p className="status">
          Keevoro copies what you are keeping straight from your sources to the place you choose, checks every copy, and
          never replaces a file that is already there. Your sources are not changed.
        </p>
      </header>
      <SendPanel
        api={props.api}
        waiting={props.waiting}
        safe={props.safe}
        onProblem={props.onProblem}
        onWhere={props.onSendingTo}
        onSent={props.onSent}
      />
      {props.next}
    </section>
  )
}

/**
 * Where this backup sends to. Until a destination is chosen this offers the
 * kinds there are, a folder first; after that it is one button, Send.
 */
function SendPanel(props: {
  api: Backend
  onProblem: (error: unknown) => void
  waiting: number
  safe: number
  /** Tells the rest of the app where this backup sends to. */
  onWhere: (where: string | null) => void
  /** Called when a send finishes, so the counts can be read again. */
  onSent: () => void
}) {
  const { api, onProblem, onWhere, onSent } = props
  const [where, setWhere] = useState<string | null | undefined>(undefined)
  const [form, setForm] = useState<ConnectRequest | null>(null)
  const [connecting, setConnecting] = useState(false)
  const [refused, setRefused] = useState<string | null>(null)
  const [running, setRunning] = useState<string | null>(null)
  const [said, setSaid] = useState<SendEvent | null>(null)
  const [stopping, setStopping] = useState(false)
  // How the send in hand is getting on, asked once a second while it runs.
  const [progress, setProgress] = useState<SendProgress | null>(null)
  // Earlier sends of this backup, for comparing one with another.
  const [history, setHistory] = useState<Sent[]>([])
  const sending = running !== null
  useEffect(() => {
    if (!sending) return
    let open = true
    const ask = () =>
      api
        .sendProgress()
        .then((p) => open && setProgress(p))
        .catch(() => {})
    void ask()
    const timer = window.setInterval(ask, 1000)
    return () => {
      open = false
      window.clearInterval(timer)
    }
  }, [api, sending])
  // How much room is needed, and whether where it is going has it.
  const [room, setRoom] = useState<Room | null>(null)
  const weigh = useCallback(() => api.room().then(setRoom).catch(onProblem), [api, onProblem])

  useEffect(() => {
    void weigh()
    api.sendHistory().then(setHistory).catch(() => {})
  }, [api, weigh, where, props.waiting, props.safe])

  // A backup carries on when you look at another screen. On coming back,
  // ask whether one is running before offering to start another.
  const [asked, setAsked] = useState(false)
  useEffect(() => {
    api
      .sendProgress()
      .then((p) => {
        if (p.running) {
          setProgress(p)
          setStopping(p.stopping)
          setRunning((now) => now ?? 'Backing up…')
        }
      })
      .catch(() => {})
      .finally(() => setAsked(true))
  }, [api])

  useEffect(() => {
    api.destination().then(setWhere).catch(onProblem)
    return api.onSend((event) => {
      if (event.finished) {
        setRunning(null)
        setStopping(false)
        setSaid(event)
        onSent()
      } else if (event.line) {
        setRunning(event.line)
      }
    })
  }, [api, onProblem, onSent])

  const connect = (request: ConnectRequest) => {
    setConnecting(true)
    setRefused(null)
    api
      .connectDestination(request)
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

  if (where === undefined || !asked) return null
  return (
    <div className="panel send">
      {where === null ? (
        <>
          {room && room.needed > 0 && (
            <p className="needs">
              You are backing up {props.waiting.toLocaleString('en-GB')} {props.waiting === 1 ? 'file' : 'files'}, about{' '}
              <b>{size(room.needed)}</b>. Wherever you choose needs at least that much free.
            </p>
          )}
          {!form && (
            <div className="places destinations">
              <button
                className="place likely"
                onClick={() =>
                  api
                    .chooseDestinationFolder()
                    .then((folder) => folder && connect({ kind: 'folder', place: folder }))
                    .catch(onProblem)
                }
                disabled={connecting}
              >
                <FolderIcon large />
                <b>A folder</b>
                <span>On this computer, another disk or a network drive</span>
              </button>
              <button className="place" onClick={() => setForm({ kind: 'r2', place: '', bucket: '' })} disabled={connecting}>
                <CloudIcon />
                <b>Cloudflare R2</b>
                <span>Cloud storage of your own</span>
              </button>
              <button className="place" onClick={() => setForm({ kind: 's3', place: '', bucket: '' })} disabled={connecting}>
                <CloudIcon />
                <b>Amazon S3</b>
                <span>Cloud storage of your own</span>
              </button>
              <button
                className="place"
                onClick={() => setForm({ kind: 'endpoint', place: '', bucket: '' })}
                disabled={connecting}
              >
                <CloudIcon />
                <b>Other storage</b>
                <span>Anything that works like S3</span>
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
                <button className="button" type="button" onClick={() => setForm(null)}>
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
          <h2>
            Backing up to{' '}
            {room?.folder ? (
              <>
                the folder <FolderLink api={api} path={room.folder} onProblem={onProblem} />
              </>
            ) : (
              where
            )}
          </h2>
          <p>
            {props.waiting > 0
              ? `${props.waiting.toLocaleString('en-GB')} ${props.waiting === 1 ? 'file is' : 'files are'} waiting to be backed up.${
                  props.safe > 0 ? ` ${props.safe.toLocaleString('en-GB')} already there.` : ''
                }`
              : props.safe > 0
                ? `Everything you are keeping is there: ${props.safe.toLocaleString('en-GB')} files.`
                : 'Nothing is being kept yet.'}
          </p>
          {room && room.needed > 0 && (
            <p className={room.enough ? 'needs' : 'needs short'} role={room.enough ? undefined : 'alert'}>
              {room.free === null ? (
                <>
                  They need about <b>{size(room.needed)}</b>.
                </>
              ) : room.enough ? (
                <>
                  They need about <b>{size(room.needed)}</b>. That disk has {size(room.free)} free, so there is room.
                </>
              ) : (
                <>
                  <b>There isn’t enough room there.</b> These files need about {size(room.needed)} and that disk has
                  only {size(room.free)} free. Free up some space, or back them up somewhere else.
                </>
              )}
            </p>
          )}
          {running && progress && progress.bytesTotal > 0 && (
            <div className="speed" role="status" aria-live="off">
              <progress value={progress.bytesDone} max={progress.bytesTotal} />
              <p>
                <b>
                  {size(progress.bytesDone)} of {size(progress.bytesTotal)}
                </b>{' '}
                backed up
                {progress.speed > 0 ? (
                  <>
                    {' '}
                    at <b>{size(progress.speed)}/s</b>
                    {progress.secondsLeft !== null && <>, {about(progress.secondsLeft)} left</>}
                  </>
                ) : (
                  <span className="quiet">. Working out the speed…</span>
                )}
              </p>
            </div>
          )}
          {running ? (
            <p className="running">
              <Spinner /> {stopping ? 'Stopping after this file…' : running}
              {!stopping && (
                <button
                  className="button small"
                  onClick={() => {
                    setStopping(true)
                    api.stopSending().catch(onProblem)
                  }}
                >
                  Stop
                </button>
              )}
            </p>
          ) : (
            <div className="choices">
              <button
                className="primary"
                onClick={() => start('Starting…', api.send)}
                disabled={props.waiting + props.safe === 0 || (room !== null && !room.enough)}
              >
                {props.waiting > 0 ? 'Back up now' : 'Back up again'}
              </button>
              <button className="button" onClick={() => start('Checking…', api.checkArrived)} disabled={props.safe === 0}>
                Check they all arrived
              </button>
              {where.startsWith('the folder') && (
                <button className="button" onClick={() => api.openDestination().catch(onProblem)}>
                  Show me the folder
                </button>
              )}
              <button
                className="button"
                onClick={() =>
                  api
                    .disconnectDestination()
                    .then(() => {
                      setWhere(null)
                      onWhere(null)
                      setSaid(null)
                    })
                    .catch(onProblem)
                }
              >
                Back up somewhere else
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
          <details className="sends layout">
            <summary>What is in the backup</summary>
            <p>
              Your photos and videos are in <code>originals</code>, untouched.
              Everything else is there to make them quick to look through, and
              can be made again.
            </p>
            <dl>
              <dt>
                <code>originals</code>
              </dt>
              <dd>
                Your photos and videos exactly as they were on your sources,
                each checked against the original after it was copied. This
                folder is the backup. Inside, there is one folder per
                collection, and <code>By date</code> for anything that did not
                belong to one.
              </dd>
              <dt>
                <code>renditions</code>
              </dt>
              <dd>
                A rendition is a version of a photo or video made for looking
                at, not for keeping. Keevoro makes them so a gallery can show
                your photos quickly: <code>thumb</code> for small pictures in a
                grid, <code>display</code> for screen-sized ones,{' '}
                <code>poster</code> for a still of each video, and{' '}
                <code>video</code> for versions of old videos that modern
                devices can play. Safe to delete if you need the room.
              </dd>
              <dt>
                <code>index</code>
              </dt>
              <dd>
                The list of what is in the backup: every photo and video, its
                date, its collection and where its renditions are. A gallery
                website or app reads <code>gallery.json</code> from here.
              </dd>
              <dt>
                <code>What is in this backup.txt</code>
              </dt>
              <dd>
                This same explanation, kept with the backup so it is there even
                without Keevoro.
              </dd>
            </dl>
            <p>
              To get your photos back you do not need Keevoro: copy the{' '}
              <code>originals</code> folder to wherever you want them. They are
              ordinary files.
            </p>
          </details>
        {history.length > 0 && !running && (
            <details className="sends">
              <summary>How earlier backups went</summary>
              <table>
                <thead>
                  <tr>
                    <th>When</th>
                    <th>Backed up</th>
                    <th>Took</th>
                    <th>Average speed</th>
                  </tr>
                </thead>
                <tbody>
                  {history.slice(0, 8).map((sent) => (
                    <tr key={sent.finishedAt}>
                      <td>
                        {new Date(sent.finishedAt * 1000).toLocaleString('en-GB', {
                          day: 'numeric',
                          month: 'short',
                          hour: '2-digit',
                          minute: '2-digit',
                        })}
                      </td>
                      <td>
                        {size(sent.bytes)}
                        {sent.stopped && ' (stopped)'}
                      </td>
                      <td>{about(sent.seconds).replace('less than a minute', 'under a minute')}</td>
                      <td>{size(sent.bytes / Math.max(sent.seconds, 1))}/s</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </details>
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

  // Holding down on a picture (or right-clicking it) opens it large,
  // without changing whether it is kept. `held` remembers that the press
  // was a look, so the click that follows letting go is not also a tick.
  const [looking, setLooking] = useState<string | null>(null)
  const hold = useRef<number | undefined>(undefined)
  const held = useRef(false)
  const press = (id: string) => {
    held.current = false
    window.clearTimeout(hold.current)
    hold.current = window.setTimeout(() => {
      held.current = true
      setLooking(id)
    }, HOLD_TO_LOOK)
  }
  const release = () => window.clearTimeout(hold.current)
  useEffect(() => () => window.clearTimeout(hold.current), [])
  const at = photos && looking ? photos.findIndex((p) => p.id === looking) : -1

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
              ? `Keeping ${thousands(keeping)} of ${thousands(photos.length)}. Click a picture to leave it out or put it back. Hold down on one to see it larger.`
              : collection.summary
            : 'These look like app pictures or leftovers, not memories. Click any you want to keep after all. Hold down on one to see it larger.'}
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
            onPointerDown={(e) => e.button === 0 && press(photo.id)}
            onPointerUp={release}
            onPointerLeave={release}
            onPointerCancel={release}
            onContextMenu={(e) => {
              e.preventDefault()
              release()
              setLooking(photo.id)
            }}
            onClick={() => {
              // Letting go after a long press is not a click on the tick.
              if (held.current) held.current = false
              else toggle(photo)
            }}
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
      {photos && at >= 0 && (
        <Viewer
          api={api}
          photo={photos[at]}
          place={`${thousands(at + 1)} of ${thousands(photos.length)}`}
          onKeep={() => toggle(photos[at])}
          onMove={(by) => {
            const next = photos[at + by]
            if (next) setLooking(next.id)
          }}
          onClose={() => setLooking(null)}
        />
      )}
    </section>
  )
}

/** How long a press must last, in milliseconds, to count as "show me this one". */
const HOLD_TO_LOOK = 450

/**
 * One photo or video still, as large as the window allows, over the grid.
 * Opened by holding down on a tile. It shows the picture, where it is on
 * the source and whether it is being kept, and lets that be changed here
 * too. Arrow keys move to the next and previous; Escape closes.
 */
function Viewer(props: {
  api: Backend
  photo: PhotoChoice
  place: string
  onKeep: () => void
  onMove: (by: 1 | -1) => void
  onClose: () => void
}) {
  const { api, photo, onMove, onClose } = props
  const [shown, setShown] = useState<{ id: string; url: string } | null>(null)
  const [missing, setMissing] = useState<string | null>(null)
  const box = useRef<HTMLDivElement>(null)

  useEffect(() => {
    let current = true
    let made: string | null = null
    // The small picture is already on screen, so show it at once and swap
    // in the large one when it arrives.
    api
      .preview(photo.id)
      .then((url) => current && setShown((now) => (now?.id === photo.id ? now : { id: photo.id, url })))
      .catch(() => {})
    api
      .closerLook(photo.id)
      .then((url) => {
        made = url
        if (current) setShown({ id: photo.id, url })
      })
      .catch(() => current && setMissing(photo.id))
    return () => {
      current = false
      // The large picture is only held while it is being looked at.
      if (made?.startsWith('blob:')) URL.revokeObjectURL(made)
    }
  }, [api, photo.id])

  useEffect(() => {
    box.current?.focus()
    const keys = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose()
      else if (e.key === 'ArrowRight') onMove(1)
      else if (e.key === 'ArrowLeft') onMove(-1)
      else return
      e.preventDefault()
    }
    window.addEventListener('keydown', keys)
    return () => window.removeEventListener('keydown', keys)
  }, [onClose, onMove])

  const url = shown?.id === photo.id ? shown.url : null
  // Drawn on the page itself, not inside the screen that opened it, so it
  // covers the whole window whatever that screen is nested in.
  return createPortal(
    <div className="viewer" role="dialog" aria-modal="true" aria-label={photo.name} tabIndex={-1} ref={box} onClick={onClose}>
      <figure onClick={(e) => e.stopPropagation()}>
        <div className="stage">
          {url ? (
            <img src={url} alt={photo.name} draggable={false} />
          ) : (
            <span className="quiet">{missing === photo.id ? 'This one can’t be shown.' : <Spinner />}</span>
          )}
        </div>
        <figcaption>
          <div>
            <b>{photo.name}</b>
            {photo.isVideo && <span className="badge">Video: this is a still from it</span>}
            <span className="quiet small where">{photo.sourcePath}</span>
          </div>
          <div className="choices">
            <span className="quiet small">{props.place}</span>
            <button className="button" onClick={() => onMove(-1)} aria-label="Previous">
              ‹
            </button>
            <button className="button" onClick={() => onMove(1)} aria-label="Next">
              ›
            </button>
            <button className="button" role="switch" aria-checked={photo.kept} onClick={props.onKeep}>
              {photo.kept ? '✓ Keeping' : 'Left out'}
            </button>
            <button className="primary" onClick={onClose}>
              Close
            </button>
          </div>
        </figcaption>
      </figure>
    </div>,
    document.body,
  )
}

/**
 * What a survey of a drive found: the folders that seem to hold photos
 * and videos, each with a tick. The person chooses, then Keevoro looks
 * through the ticked ones properly, one after another.
 */
function SurveyResult(props: {
  name: string
  found: Survey
  chosen: Set<string>
  onToggle: (folder: string) => void
  onGo: () => void
  onCancel: () => void
}) {
  const { found, chosen } = props
  const picked = found.hints.filter((h) => chosen.has(h.folder))
  const photos = picked.reduce((n, h) => n + h.photos, 0)
  const videos = picked.reduce((n, h) => n + h.videos, 0)
  const bytes = picked.reduce((n, h) => n + h.bytes, 0)
  return (
    <div className="panel survey">
      <h2>
        {found.hints.length === 0
          ? `No photos or videos were spotted on ${props.name}`
          : `${count(found.hints.length, 'folder')} on ${props.name} ${found.hints.length === 1 ? 'seems' : 'seem'} to hold photos and videos`}
      </h2>
      <p className="quiet">
        Looked at {count(found.files, 'file')} in {count(found.folders, 'folder')} in{' '}
        {found.seconds < 10 ? found.seconds.toFixed(1) : Math.round(found.seconds)} seconds, without opening any of
        them{found.stopped ? ', and stopped before the end, so there may be more' : ''}. This goes by file names, so
        it is a good guess, not a promise. Tick the folders you want and Keevoro will look through those properly.
      </p>
      {found.hints.length > 0 && (
        <ul className="hints">
          {found.hints.map((hint) => (
            <li key={hint.folder} className={hint.alreadyASource ? 'done' : undefined}>
              <label>
                <input
                  type="checkbox"
                  checked={chosen.has(hint.folder)}
                  disabled={hint.alreadyASource}
                  onChange={() => props.onToggle(hint.folder)}
                />
                <span className="where">{hint.folder}</span>
              </label>
              <span className="quiet small">
                {photosAndVideos(hint.photos, hint.videos)} · {size(hint.bytes)}
              </span>
              {hint.alreadyASource && <span className="badge">Already a source</span>}
              {!hint.likelyPhotos && !hint.alreadyASource && (
                <span className="badge" title="The pictures here are very small and there are no videos">
                  Small pictures, probably not photos
                </span>
              )}
            </li>
          ))}
        </ul>
      )}
      {found.unreadable > 0 && (
        <p className="quiet small">
          {count(found.unreadable, 'folder')} could not be listed, usually because this account is not allowed to
          look in {found.unreadable === 1 ? 'it' : 'them'}.
        </p>
      )}
      <p className="choices">
        {found.hints.length > 0 && (
          <button className="primary" onClick={props.onGo} disabled={picked.length === 0}>
            {picked.length === 0
              ? 'Tick a folder to look through'
              : `Look through ${picked.length === 1 ? 'this folder' : `these ${picked.length} folders`}`}
          </button>
        )}
        <button className="button" onClick={props.onCancel}>
          {found.hints.length > 0 ? 'Cancel' : 'Back'}
        </button>
        {picked.length > 0 && (
          <span className="quiet small">
            About {photosAndVideos(photos, videos)}, {size(bytes)}.
          </span>
        )}
      </p>
    </div>
  )
}

/** How many duplicates are drawn at once. More than this is only a wall of rows. */
const DUPLICATES_SHOWN = 200

/**
 * Every file found more than once: the copy being kept beside the copies
 * left out, so a person can see Keevoro got it right before anything is
 * backed up. Nothing can be changed here, because there is nothing to
 * lose: a copy is only left out when it is the same file, byte for byte.
 */
function DuplicatesScreen(props: { api: Backend; onBack: () => void; onProblem: (error: unknown) => void }) {
  const { api, onProblem } = props
  const [found, setFound] = useState<Duplicate[] | null>(null)
  useEffect(() => {
    let current = true
    api
      .duplicates()
      .then((list) => current && setFound(list))
      .catch(onProblem)
    return () => {
      current = false
    }
  }, [api, onProblem])
  const leftOut = found?.reduce((n, d) => n + d.copies.length, 0) ?? 0
  const unsure = found?.filter((d) => !d.confirmed).length ?? 0
  const show = (folder: string) => api.openPlace(folder).catch(onProblem)

  return (
    <section className="photos duplicates">
      <header>
        <button className="link back" onClick={props.onBack}>
          ‹ Back
        </button>
        <h1>Duplicates</h1>
        <p className="status">
          {found
            ? found.length === 0
              ? 'No file was found more than once.'
              : `${count(found.length, 'file')} ${found.length === 1 ? 'was' : 'were'} found more than once. One copy of each is kept, and ${thousands(leftOut)} extra ${leftOut === 1 ? 'copy is' : 'copies are'} left out.`
            : 'Looking…'}
        </p>
      </header>
      {found && found.length > 0 && (
        <div className="knows">
          <p>
            <b>How Keevoro knows.</b> Two files count as duplicates only when every byte of one matches every byte of
            the other. The same name, size or date is never enough, and two photos that merely look alike are both
            kept. So a copy left out here is not a different photo: it is the same file in a second place.
          </p>
          {unsure > 0 && (
            <p>
              {count(unsure, 'file')} below {unsure === 1 ? 'is' : 'are'} marked <b>still being checked</b>. Those copies match so far
              but have not been read all the way through yet. Keevoro finishes reading them before it backs anything
              up, and if they turn out to differ, both are kept.
            </p>
          )}
          <p className="quiet small">Nothing on your sources is changed or deleted. Left out only means not backed up twice.</p>
        </div>
      )}
      <ul className="groups">
        {found?.slice(0, DUPLICATES_SHOWN).map((d) => (
          <li key={d.id}>
            <Picture api={api} photo={d} />
            <div>
              <p className="name">
                <b>{d.name}</b> <span className="quiet">{size(d.bytes)}</span>
                {!d.confirmed && <span className="badge">still being checked</span>}
              </p>
              <p className="copy kept">
                <span className="mark">Kept</span>
                <button className="link where" onClick={() => show(d.folder)} title="Show this folder">
                  {d.folder}
                </button>
              </p>
              {d.copies.map((c, i) => (
                <p className="copy" key={i}>
                  <span className="mark">Left out</span>
                  <button className="link where" onClick={() => show(c.folder)} title="Show this folder">
                    {c.folder}
                  </button>
                  {c.name !== d.name && <span className="quiet"> as {c.name}</span>}
                </p>
              ))}
            </div>
          </li>
        ))}
      </ul>
      {found && found.length > DUPLICATES_SHOWN && (
        <p className="quiet">
          Showing the first {thousands(DUPLICATES_SHOWN)} of {thousands(found.length)}.
        </p>
      )}
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
    <div
      className={missing ? 'picture missing' : url ? 'picture' : 'picture loading'}
      title={photo.name}
      aria-busy={!url && !missing}
    >
      {url && <img src={url} alt="" draggable={false} />}
      {missing && <span>{photo.isVideo ? 'Video' : 'No preview'}</span>}
      {photo.isVideo && url && <span className="play" aria-label="Video" />}
    </div>
  )
}

/**
 * A value that changes on screen no more often than `every` milliseconds,
 * however fast it really changes. For words a person is meant to read:
 * text replaced many times a second is only a flicker.
 */
function useSteady<T>(value: T, every: number): T {
  const [shown, setShown] = useState(value)
  const latest = useRef(value)
  useEffect(() => {
    latest.current = value
  }, [value])
  useEffect(() => {
    const timer = window.setInterval(() => setShown(latest.current), every)
    return () => window.clearInterval(timer)
  }, [every])
  return shown
}

function Spinner() {
  return <span className="spinner" aria-hidden />
}

/**
 * The logo, exactly as supplied by the design system: the full lockup in
 * the side bar and, larger, on the first screen, each with its own
 * version for dark grounds. It is never redrawn or recoloured.
 */
function Logo(props: { large?: boolean }) {
  return props.large ? (
    // The first thing anyone sees: the whole logo, name and all. The
    // design system's shadowed version is for large use on light grounds.
    <span className="brand hero">
      <img className="on-light" src={lockupShadowUrl} alt="Keevoro" />
      <img className="on-dark" src={lockupDarkLargeUrl} alt="Keevoro" />
    </span>
  ) : (
    <>
      <img className="on-light" src={lockupUrl} alt="Keevoro" />
      <img className="on-dark" src={lockupDarkUrl} alt="Keevoro" />
    </>
  )
}

const icon = (path: string, large = false) => (
  <svg width={large ? 44 : 18} height={large ? 44 : 18} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.75" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
    <path d={path} />
  </svg>
)
const FolderIcon = (props: { large?: boolean }) => icon('M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z', props.large)
const CloudIcon = () => icon('M7 18a4 4 0 0 1-.6-7.96A5.5 5.5 0 0 1 17 9.5a4.25 4.25 0 0 1 .5 8.5z', true)
const DriveIcon = (props: { large?: boolean }) => icon('M4 14l2.5-8h11L20 14M4 14h16v5H4zM16.5 16.5h.01', props.large)
