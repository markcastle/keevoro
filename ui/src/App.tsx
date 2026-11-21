// The app's screens.
//
// Keevoro is used by people who would not call themselves technical, so
// the app is a short walk with one question per screen:
//
//   1  Find your photos              pick a drive, or a folder
//   2  Choose what to keep           collections, all kept unless you say
//   3  Back them up somewhere safe   another disk, or cloud storage
//
// Every screen has one heading that asks the question, and one big button
// at the bottom that moves on. The three steps down the side are the only
// navigation; there are no tabs to learn. Nothing is asked that has a
// sensible default.
//
// Nothing here decides anything about photos. Every fact on screen comes
// from the engine through backend.ts, and every button sends one request
// back. Wording follows the house rules: plain English, no jargon, and
// progress in human terms ("3,412 of 9,870 safely backed up").

import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react'
import type { ReactNode } from 'react'

import { backend } from './backend'
import { scrollerOf } from './scroller'
import type {
  Acceleration,
  Destination,
  Survey,
  SurveyProgress,
  Duplicate,
  Activity,
  AgentAccess,
  FileKind,
  ListedAs,
  LeftOut,
  LeftOutFolder,
  StageRecord,
  BackupStage,
  Sending,
  AppState,
  Backend,
  SendEvent,
  Collection,
  ConnectRequest,
  Discover,
  FileType,
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
import { about, count, files, size, thousands, wholePlace } from './format'
import { heldShare } from './held'
import lockupDarkLargeUrl from './assets/keevoro-lockup-reversed-large.png'
import lockupShadowUrl from './assets/keevoro-lockup-shadow.png'
import lockupDarkUrl from './assets/keevoro-lockup-reversed.png'
import lockupUrl from './assets/keevoro-lockup.png'
import { chooseTheme, savedTheme } from './theme'
import { ClearUnfinished } from './tidy'
import { FoundPicture, Viewer } from './pictures'
import { LookInside } from './inside'
import { SpaceScreen } from './map'
import { LiveMap } from './liveMap'
import { ForgetMapsSetting } from './forgetMaps'
import { OpenSourceLicences } from './licences'
import { LeaveOutButton, LeftOutByYou, LeftOutSaid, leftOutByYou } from './leaveOut'
import { VirtualTiles } from './tiles'
import { BackupWants, SourceWants } from './wants'
import { OtherFilesScreen, OtherFilesSection } from './otherFiles'
import { ThereNow, WhatWasFound } from './whatIsThere'
import { ManageBackups } from './manageBackups'
import { certain, outOfReachLine } from './thereWords'
import type { Theme } from './theme'

type Step = 'find' | 'choose' | 'send'
/** Looking inside a collection, or at what was set aside, sits on top of step 2. */
type Overlay = { collection: Collection } | 'setAside' | 'duplicates' | 'settings' | 'activity' | 'about' | 'backups' | null

const STEPS: { id: Step; title: string }[] = [
  { id: 'find', title: 'Find your photos' },
  { id: 'choose', title: 'Choose what to keep' },
  { id: 'send', title: 'Back them up somewhere safe' },
]

/** How many are kept, in words: photos and videos as ever, or files once other files are among them. */
const all = (n: number, withOthers = false) =>
  withOthers ? count(n, 'file') : n === 1 ? '1 photo or video' : `${n.toLocaleString()} photos and videos`

export function App() {
  const [api, setApi] = useState<Backend | null>(null)
  const [state, setState] = useState<AppState | null>(null)
  const [found, setFound] = useState<Discover | null>(null)
  const [step, setStep] = useState<Step>('find')
  const [overlay, setOverlay] = useState<Overlay>(null)
  const [scan, setScan] = useState<ScanEvent | null>(null)
  const [justFound, setJustFound] = useState<ScanEvent | null>(null)
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
        .then((a) => {
          if (!open) return
          setDoing(a.doing)
          // Paused here or by another Keevoro program, it is said on
          // every screen, with Carry on, for as long as it lasts.
          setPaused(a.paused)
        })
        .catch(() => {})
    void ask()
    const timer = window.setInterval(ask, 2000)
    return () => {
      open = false
      window.clearInterval(timer)
    }
  }, [api])

  // Opening a collection or a list covers the screen; coming back should
  // land where the person was, not at the top. The place is noted when an
  // overlay opens and restored when it closes.
  const main = useRef<HTMLElement>(null)
  const wasAt = useRef(0)
  const show = (next: Overlay) => {
    if (overlay === null) wasAt.current = scrollerOf(main.current)?.scrollTop ?? 0
    setOverlay(next)
  }
  useLayoutEffect(() => {
    const scroller = scrollerOf(main.current)
    if (!scroller) return
    scroller.scrollTop = overlay === null ? wasAt.current : 0
  }, [overlay])

  const refresh = useCallback((from: Backend) => from.discover().then(setFound).catch(report), [report])

  useEffect(() => {
    let stopScan = () => {}
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
    // Said as safe only while Keevoro has looked there a moment ago.
    send:
      safe > 0
        ? certain(found?.destinations ?? [])
          ? `${thousands(safe)} safe`
          : `${thousands(safe)} safe when last looked`
        : sendingTo
          ? 'Ready to send'
          : '',
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
          onManage={() => show('backups')}
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
          {check && !paused && (
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
          <button className="link doing" onClick={() => show('activity')}>
            What Keevoro is doing
            {doing && !paused && (
              <span className="quiet small" role="status">
                <Spinner /> {doing}
              </span>
            )}
          </button>
          <button className="link" onClick={() => show('settings')}>
            Settings
          </button>
          <button className="link" onClick={() => show('about')}>
            About Keevoro
          </button>
        </div>
      </aside>

      <main ref={main}>
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

        {overlay === 'backups' ? (
          <ManageBackups
            api={api}
            state={state}
            onState={(s) => {
              // The open backup may be another now (the one open was
              // removed), or none at all: then the first-run screen shows.
              if (!s.backup) {
                setState(s)
                setFound(null)
                setSendingTo(null)
                setJustFound(null)
                setOverlay(null)
                setStep('find')
              } else if (s.backup.id !== state.backup?.id) {
                void openBackup(Promise.resolve(s)).then(() => setOverlay('backups'))
              } else {
                setState(s)
              }
            }}
            onOpen={(id) => openBackup(api.switchBackup(id))}
            onBack={() => setOverlay(null)}
            onProblem={report}
          />
        ) : overlay === 'activity' ? (
          <ActivityScreen api={api} onPaused={setPaused} onBack={() => setOverlay(null)} onProblem={report} />
        ) : overlay === 'about' ? (
          <AboutScreen api={api} version={state.version} onBack={() => setOverlay(null)} onProblem={report} />
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
            onSources={() => {
              setOverlay(null)
              setStep('find')
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
        ) : overlay !== 'setAside' && overlay?.collection.byFolder ? (
          <OtherFilesScreen
            api={api}
            folder={overlay.collection}
            onBack={() => {
              setOverlay(null)
              void refresh(api)
            }}
            onProblem={report}
          />
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
                  say={`${files(found?.photos ?? 0, found?.videos ?? 0, found?.others ?? 0)} found so far.`}
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
            onOpen={(collection) => show({ collection })}
            onSetAside={() => show('setAside')}
            onDuplicates={() => show('duplicates')}
            next={
              waiting > 0 ? (
                <Next
                  back={() => go('find')}
                  say={
                    <>
                      Keeping <b>{all(waiting + safe, (found?.others ?? 0) > 0)}</b> in{' '}
                      {kept.some((c) => c.byFolder)
                        ? `${count(kept.filter((c) => !c.byFolder).length, 'collection')} and ${count(kept.filter((c) => c.byFolder).length, 'folder')} of other files`
                        : count(kept.length, 'collection')}
                      .
                    </>
                  }
                  button="Choose where they go"
                  onClick={() => go('send')}
                />
              ) : safe > 0 ? (
                <Next
                  back={() => go('find')}
                  say={
                    certain(found?.destinations ?? [])
                      ? 'Everything you’re keeping has already been backed up.'
                      : 'Everything you’re keeping was backed up when Keevoro last looked.'
                  }
                  button="Next"
                  onClick={() => go('send')}
                />
              ) : (
                <Next back={() => go('find')} say="Nothing is being kept yet. Choose at least one collection." />
              )
            }
          />
        ) : (
          <SendScreen
            api={api}
            found={found}
            onDuplicates={() => show('duplicates')}
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
  onManage: () => void
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
            <>
              <button className="button" onClick={() => setNaming('')}>
                Start a new backup…
              </button>
              <button
                className="button"
                onClick={() => {
                  close()
                  props.onManage()
                }}
              >
                Manage backups…
              </button>
            </>
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
    path: string
    progress: SurveyProgress | null
    found: Survey | null
    chosen: Set<string>
  } | null>(null)
  // Folders waiting their turn to be looked through, one after another.
  const [queue, setQueue] = useState<string[]>([])
  // A folder being looked inside: one the survey suggested, a source, or
  // one picked to look inside first. The survey stays as it was
  // underneath, to come back to. `again` when it is a source already.
  const [inside, setInside] = useState<{ place: string; name?: string; again?: boolean } | null>(null)
  // Where the space goes in a place, under a look inside opened from it.
  const [space, setSpace] = useState<{ place: string; name?: string; at?: string } | null>(null)
  const surveyScroll = useRef(0)
  useEffect(
    () =>
      api.onSurvey((progress) =>
        // The top of the map comes twice a second; between, the last is kept.
        setSurvey((s) => (s && !s.found ? { ...s, progress: { ...progress, top: progress.top ?? s.progress?.top } } : s)),
      ),
    [api],
  )

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
    setSurvey({ name: place.name, path: place.path, progress: null, found: null, chosen: new Set() })
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
  // Looks through every source again, one after another, to pick up
  // anything added since. What is already backed up is not sent again.
  const lookAgain = () => {
    const [first, ...rest] = (found?.sources ?? []).map((s) => s.root)
    if (first === undefined) return
    setQueue(rest)
    start(first)
  }
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
  // Opens the look inside a folder, noting where the page was to come back to.
  const lookInside = (place: string, source?: { name: string }) => {
    surveyScroll.current = scroller()?.scrollTop ?? 0
    setInside({ place, name: source?.name, again: Boolean(source) })
    requestAnimationFrame(() => scroller()?.scrollTo(0, 0))
  }
  // Opens where the space goes in a place, noting where the page was.
  const seeSpace = (place: string, name?: string) => {
    surveyScroll.current = scroller()?.scrollTop ?? 0
    setSpace({ place, name })
    requestAnimationFrame(() => scroller()?.scrollTo(0, 0))
  }
  // Asks for any folder, then looks inside it first.
  const pickToLookInside = () => {
    api
      .pickPlace()
      .then((path) => path && lookInside(path))
      .catch(onProblem)
  }
  const choose = (place: Place) => {
    if (place.askFirst) setAsking({ path: place.path, name: place.name, detail: place.detail })
    else start(place.path)
  }
  const done = new Set(found?.sources.map((s) => s.root.replace(/[\\/]+$/, '')))
  const removable = places?.filter((p) => p.removable) ?? []
  const fixed = places?.filter((p) => !p.removable) ?? []
  const scroller = () => scrollerOf(document.querySelector('main'))

  if (inside) {
    return (
      <LookInside
        api={api}
        place={inside.place}
        name={inside.name}
        lookThrough={inside.again ? 'Look through it again' : undefined}
        onChanged={props.onChanged}
        onProblem={onProblem}
        onBack={() => {
          setInside(null)
          requestAnimationFrame(() => scroller()?.scrollTo(0, surveyScroll.current))
        }}
        onLookThrough={(folder) => {
          setInside(null)
          setSpace(null)
          setSurvey(null)
          setQueue([])
          start(folder)
        }}
      />
    )
  }

  if (space) {
    return (
      <SpaceScreen
        api={api}
        place={space.place}
        name={space.name}
        at={space.at}
        onAt={(at) => setSpace((s) => s && { ...s, at })}
        onChanged={props.onChanged}
        onProblem={onProblem}
        onBack={() => {
          setSpace(null)
          requestAnimationFrame(() => scroller()?.scrollTo(0, surveyScroll.current))
        }}
        onLookInside={(folder) => setInside({ place: folder })}
        onLookThrough={(folder) => {
          setSpace(null)
          setSurvey(null)
          setQueue([])
          start(folder)
        }}
      />
    )
  }

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
          <p className="big-count">{files(scan.photos, scan.videos, scan.others ?? 0)}</p>
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
            {survey.progress ? files(survey.progress.photos, survey.progress.videos, survey.progress.others ?? 0) : 'Starting…'}
          </p>
          <p className="quiet">
            spotted so far, by name. This only reads the list of what is in each folder. No file is opened.
          </p>
          <LiveMap name={survey.name} top={survey.progress?.top} />
          {survey.progress?.sizing && (
            <p className="quiet">
              Every folder with {(survey.progress.others ?? 0) > 0 ? 'photos, videos and other files' : 'photos and videos'} is
              found. Now measuring the rest, such as system and app folders, so you can see where the space goes.
            </p>
          )}
          <p className="choices stop">
            <button onClick={() => api.stopSurvey().catch(onProblem)}>
              {survey.progress?.sizing ? 'Show the folders now' : 'Stop and show what was found'}
            </button>
          </p>
          {survey.progress && (
            <p className="quiet small doing" aria-live="off">
              Looked at {count(survey.progress.files, 'file')} in {count(survey.progress.folders, 'folder')} so far.
            </p>
          )}
        </div>
      ) : survey?.found ? (
        <SurveyResult
          api={api}
          onProblem={onProblem}
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
          onLookInside={(folder) => {
            lookInside(folder)
          }}
          onSeeSpace={() => seeSpace(survey.path, survey.name)}
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
                    Found {files(justFound.photos, justFound.videos, justFound.others ?? 0)} on {justFound.name}
                  </h2>
                  <p className="quiet">You can add another place below, or carry on.</p>
                  {(justFound.unreadable ?? 0) > 0 && (
                    <p className="quiet small unreadable">
                      {count(justFound.unreadable ?? 0, 'file')} could not be read and{' '}
                      {justFound.unreadable === 1 ? 'was' : 'were'} left out, usually because the file is damaged or
                      this account is not allowed to open it:{' '}
                      <span className="where">{(justFound.unreadableNames ?? []).join(', ')}</span>
                      {(justFound.unreadable ?? 0) > (justFound.unreadableNames ?? []).length && ' and more'}. The full
                      list is in the log, in Settings.
                    </p>
                  )}
                  {justFound.onlineOnlySaid && <p className="online-only">{justFound.onlineOnlySaid}</p>}
                  <LeftOutNote api={api} leftOut={justFound.leftOut} onProblem={onProblem} />
                  <LeftOutByYou
                    api={api}
                    folders={leftOutByYou(justFound.leftOut)}
                    title="Passed by, as you left these out of this backup:"
                    onChanged={props.onChanged}
                    onProblem={onProblem}
                  />
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
          <p className="quiet small look-first">
            Not sure what is in a folder?{' '}
            <button className="link" onClick={pickToLookInside}>
              Look inside one first
            </button>
            . You see its photos and videos, and nothing is added until you choose.
          </p>
        </>
      )}

      {found && found.sources.length > 0 && (
        <>
          <h3>Your sources</h3>
          <p className="quiet small">Removing a source only takes it off this list. Keevoro never changes or deletes anything in a source.</p>
          {!scan && !survey && (
            <p className="choices again">
              <button className="button small" onClick={lookAgain}>
                {found.sources.length === 1 ? 'Look again for new photos' : `Look through all ${found.sources.length} again for new photos`}
              </button>
              <span className="quiet small">
                Taken more photos since last time? This finds what is new, and the next backup sends only that.
              </span>
            </p>
          )}
          <ul className="kept">
            {found.sources.map((s) => (
              <li key={s.id} className="source">
                <span className="glyph" aria-hidden>
                  {wholePlace(s.root) ? <DriveIcon large /> : <FolderIcon large />}
                </span>
                <div className="what">
                  <b>{sourceTitle(s, places)}</b>
                  <FolderLink api={api} path={s.root} onProblem={onProblem} />
                </div>
                <div className="count">
                  <b>{all(s.items, (found.others ?? 0) > 0)}</b>
                  <span className="quiet small">{size(s.bytes)}</span>
                </div>
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
                      onClick={() => lookInside(s.root, { name: sourceTitle(s, places) })}
                      aria-label={`Look inside ${s.name}`}
                      title="See its photos and videos, folder by folder. Nothing is changed."
                    >
                      Look inside
                    </button>
                    <button
                      className="button small"
                      onClick={() => seeSpace(s.root, sourceTitle(s, places))}
                      aria-label={`Where the space goes on ${s.name}`}
                      title="See what takes the room on it, photos or not. Nothing is changed."
                    >
                      Where the space goes
                    </button>
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
                {s.onlineOnlySaid && <p className="online-only small">{s.onlineOnlySaid}</p>}
                <LeftOutByYou
                  api={api}
                  folders={s.leftOut ?? []}
                  title="Left out of this backup:"
                  onChanged={props.onChanged}
                  onProblem={onProblem}
                />
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

/**
 * What to call a source in the list. A source someone named, or a folder,
 * keeps its name. A whole drive that was only ever known by its letter
 * ("I:\\") reads as a bare path, so it takes the name the computer gives
 * the drive when it is plugged in ("KINGSTON (I:)"), or "Drive I:" when not.
 */
function sourceTitle(source: Source, places: Place[] | null): string {
  const bare = (path: string) => path.replace(/[\\/]+$/, '')
  const named = source.name.trim()
  if (named !== '' && bare(named) !== bare(source.root)) return named
  const plugged = places?.find((p) => bare(p.path).toLowerCase() === bare(source.root).toLowerCase())
  if (plugged) return plugged.name
  const letter = /^([A-Za-z]):$/.exec(bare(source.root))
  if (letter) return `Drive ${letter[1].toUpperCase()}:`
  return bare(source.root).split(/[\\/]/).pop() || source.root
}

function PlaceCard(props: { place: Place; added: boolean; onClick: () => void }) {
  const { place } = props
  return (
    <button className="place" onClick={props.onClick}>
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
  const reload = () => api.fileTypes(source.id).then(setKinds).catch(onProblem)
  if (!kinds) return null
  return (
    <div className="types explain">
      <SourceWants
        name={source.name}
        load={() => api.sourceWants(source.id)}
        choose={(wants) => api.setSourceWants(source.id, wants)}
        onChanged={() => {
          void reload()
          props.onChanged()
        }}
        onProblem={onProblem}
      />
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
              {kind.why ? ` · left out: ${kind.why}` : !kind.included && ' · left out'}
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
                {job.what === 'Backing up your files' && now.backupStep && (
                  <p className="step-now" role="status">
                    <b>Right now:</b> {now.backupStep}
                  </p>
                )}
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
/**
 * What Keevoro is, what it promises, and the terms it comes under. The
 * promises are the ones the code is built around and tested for; nothing
 * is claimed here that the tests do not hold it to.
 */
function AboutScreen(props: { api: Backend; version: string; onBack: () => void; onProblem: (error: unknown) => void }) {
  const { api, onProblem } = props
  const [licences, setLicences] = useState(false)
  const open = (page: string) => api.openReleasePage(page).catch(onProblem)
  if (licences) return <OpenSourceLicences onBack={() => setLicences(false)} />
  return (
    <section className="about">
      <header>
        <button className="link" onClick={props.onBack}>
          ‹ Back
        </button>
        <h1>About Keevoro</h1>
        <p className="status">Keep things as they were.</p>
      </header>

      <div className="panel">
        <h2>Your photos are safe with Keevoro</h2>
        <p>
          These are the only copies some people have of the people and places they love. Keevoro is built around a few
          rules it is made never to break, and automated tests guard them every time the app changes.
        </p>
        <dl className="promises">
          <dt>Your sources are only ever read</dt>
          <dd>
            Keevoro never changes, moves, renames or deletes anything on a drive, card or folder it looks through.
            Removing a source only takes it off a list. If a source is damaged or vanishes mid-way, nothing on it has
            been touched.
          </dd>
          <dt>A copy only counts when it is proved exact</dt>
          <dd>
            Every file is fingerprinted from its whole contents. An original written to a folder or a drive is read
            back and compared with that fingerprint before Keevoro calls it safe, and one that does not match is
            thrown away and said so. If a file of the same name is already there, it is fingerprinted too, and
            counted only if it is the same file. An original sent to Cloudflare R2 or Amazon S3 travels over an
            encrypted, error-checked connection, and “Check they all arrived” confirms each one is there and the
            right size.
          </dd>
          <dt>No original in a backup is ever replaced or removed</dt>
          <dd>
            An original, once backed up, is never overwritten. Two different photos with the same name are both kept.
            Deleting a photo from a source does not delete it from your backup. Only what Keevoro makes itself, the
            previews and the list of contents, is ever rewritten.
          </dd>
          <dt>Duplicates are exact, never guesses</dt>
          <dd>
            A file is left out as a duplicate only when every byte matches the copy being kept. Photos that merely
            look alike are both kept, and you can see every duplicate before anything is backed up.
          </dd>
          <dt>Nothing is hidden from you</dt>
          <dd>
            Anything set aside is listed with the reason and can be brought back. Anything that could not be read is
            counted on screen and listed by name in the log, and anything that could not be backed up is named when
            the backup ends.
          </dd>
          <dt>You never need Keevoro to get your photos back</dt>
          <dd>
            A backup is ordinary files in ordinary folders, with a plain note inside explaining them. Copy the{' '}
            <code>originals</code> folder and you have everything.
          </dd>
          <dt>Your photos go only where you send them</dt>
          <dd>
            There is no Keevoro account and no Keevoro server. Your files go from your source to the destination you
            chose and nowhere else. Storage keys are kept in this computer’s own keychain. The only thing Keevoro
            contacts by itself is GitHub, to see whether there is a newer version. On this computer it keeps only
            its working notes: a list of what it found, small previews, and a log of what it did.
          </dd>
        </dl>
        <p className="quiet small">
          What no program can promise is that a disk will not fail. Keep more than one copy of anything you cannot
          afford to lose, in more than one place, and use “Check they all arrived” now and then.
        </p>
      </div>

      <div className="panel">
        <h2>The name</h2>
        <p>
          The name blends the sound of English <i>keep</i> with <i>voro</i>, an old Swedish word meaning “were”.
          Together they capture why we save photos and videos in the first place: to remember people and places as
          they were.
        </p>
      </div>

      <div className="panel">
        <h2>Licence</h2>
        <p>
          Copyright 2026 Mark Castle. Keevoro’s source is open to read, use, change and share under the Functional
          Source License (FSL-1.1-MIT). You may use it freely, at home or at work. You may not sell it or offer it as
          a product that competes with Keevoro. Each version becomes plain MIT two years after it comes out.
        </p>
        <p className="quiet small">
          That is a summary; the licence itself is what counts. The Keevoro name and logo are not covered by it, and
          it comes with no warranty.
        </p>
        <p className="choices">
          <button className="button small" onClick={() => open(`${PROJECT}/blob/main/LICENSE.md`)}>
            Read the licence
          </button>
          <button className="button small" onClick={() => open(`${PROJECT}/blob/main/TRADEMARKS.md`)}>
            The name and logo
          </button>
          <button className="button small" onClick={() => open(PROJECT)}>
            See the source
          </button>
          <button className="button small" onClick={() => setLicences(true)}>
            Open source licences
          </button>
        </p>
        <p className="quiet small">Version {props.version}</p>
      </div>
    </section>
  )
}

/** Said before each stage's line for a screen reader, since its mark is a picture. */
const STAGE_STATE_WORDS: Record<BackupStage['state'], string> = {
  done: 'Done: ',
  problem: 'Went wrong: ',
  now: 'Now: ',
  alongside: 'Now: ',
  next: 'Then: ',
}

/** Keevoro's home on the web: the source, the licence and the releases. */
const PROJECT = 'https://github.com/markcastle/keevoro'

/** How many seconds each point on a chart is averaged over. */
const STEADIED_OVER = 5

/**
 * Each value replaced by the average of itself and the few before it.
 *
 * The computer reports what a program read and wrote in lumps, not as a
 * steady flow: a second with nothing, then a second with double. Drawn as
 * it comes, a steady 8 MB/s looks like a saw. Averaged over a few seconds
 * it looks like what it is. The total is unchanged, only spread out.
 */
function steadied(values: number[], over: number): number[] {
  return values.map((_, i) => {
    const from = Math.max(0, i - over + 1)
    let sum = 0
    for (let at = from; at <= i; at++) sum += values[at]
    return sum / (i - from + 1)
  })
}

function UsageChart(props: { title: string; unit: string; about: string; values: number[]; floor: number }) {
  const { unit } = props
  const values = steadied(props.values, STEADIED_OVER)
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
          now · highest {said(peak)} in the last {values.length < WINDOW ? `${values.length} seconds` : 'two minutes'} ·
          averaged over {STEADIED_OVER} seconds
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
  /** Goes to the sources, where they can be looked through again. */
  onSources: () => void
  onBack: () => void
  onProblem: (error: unknown) => void
}) {
  const { api, state, onProblem } = props
  const [log, setLog] = useState<string | null>(null)
  // Counts each change to the kinds of file, so what the backup is for is
  // asked again: "Other files" is a choice once a kind of them is listed.
  const [kindsChanged, setKindsChanged] = useState(0)
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
        <details className="advanced">
          <summary>Advanced</summary>
          <BackupWants
            key={kindsChanged}
            load={() => api.backupWants()}
            choose={(wants) => api.setBackupWants(wants)}
            onChanged={() => props.onLibrary(state)}
            onProblem={onProblem}
          />
          <p className="quiet small">
            A source can have its own choice, under Kinds of file beside it on the first step.
          </p>
        </details>
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

      <AgentsSetting api={api} onProblem={onProblem} />

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

      <details className="advanced">
        <summary>Advanced</summary>
        <FileKindsSetting
          api={api}
          onSources={props.onSources}
          onChanged={() => setKindsChanged((n) => n + 1)}
          onProblem={onProblem}
        />
        <ForgetMapsSetting api={api} onProblem={onProblem} />
        <div className="setting">
          <h3>Unfinished copies</h3>
          <p className="quiet">
            A backup that is stopped part-way can leave unfinished copies in a working folder where it was going. Each
            backup clears its own as it starts and ends. For a folder no backup goes to any more, clear them here.
            Nothing that was backed up is touched, and Keevoro says how much there is first.
          </p>
          <div className="choices">
            <ClearUnfinished api={api} label="Clear unfinished copies from a folder…" onProblem={onProblem} />
          </div>
        </div>
      </details>

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
              Keevoro, with what was found and what you chose to keep; the previews and maps Keevoro made; and any storage
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

/**
 * What an AI agent may do through `keevoro mcp`. Nothing until the person
 * says: no folders, and looking only. An agent can never change this itself.
 */
function AgentsSetting(props: { api: Backend; onProblem: (error: unknown) => void }) {
  const { api, onProblem } = props
  const [access, setAccess] = useState<AgentAccess | null>(null)
  useEffect(() => {
    api.agentAccess().then(setAccess).catch(onProblem)
  }, [api, onProblem])
  const change = (part: Partial<AgentAccess>) => api.setAgentAccess(part).then(setAccess).catch(onProblem)
  const add = (list: 'lookIn' | 'backUpTo') =>
    api
      .chooseAgentFolder()
      .then((folder) => {
        if (folder && access && !access[list].includes(folder)) change({ [list]: [...access[list], folder] })
      })
      .catch(onProblem)

  const folders = (list: 'lookIn' | 'backUpTo', title: string, none: string) =>
    access && (
      <div className="agent-folders">
        <h4>{title}</h4>
        {access[list].length === 0 ? (
          <p className="quiet small">{none}</p>
        ) : (
          <ul aria-label={title}>
            {access[list].map((folder) => (
              <li key={folder}>
                <span className="where">{folder}</span>
                <button
                  className="link"
                  aria-label={`Stop allowing ${folder}`}
                  onClick={() => change({ [list]: access[list].filter((f) => f !== folder) })}
                >
                  Remove
                </button>
              </li>
            ))}
          </ul>
        )}
        <p className="choices">
          <button onClick={() => add(list)}>Add a folder…</button>
        </p>
      </div>
    )

  return (
    <div className="setting">
      <h3>AI agents</h3>
      <p className="quiet">
        An AI app, such as Claude, can use Keevoro for you once it is set up to start <code>keevoro mcp</code>. It can
        never change anything on your sources. Here you choose what else it may do. Everything it does is written in
        the log.
      </p>
      {access && (
        <>
          <label className="switch-row">
            <button
              className={access.mayChange ? 'kv-switch on' : 'kv-switch'}
              role="switch"
              aria-checked={access.mayChange}
              aria-label="Let AI agents make changes"
              onClick={() => change({ mayChange: !access.mayChange })}
            >
              <span className="knob" />
            </button>
            <span>
              <b>Let AI agents make changes</b>
              <span className="quiet small">
                Off, an agent can only look at your backups and say what it sees. On, it can also start backups, choose
                what to keep and back up, but only to the folders below. It can never delete a photo.
              </span>
            </span>
          </label>
          {folders('lookIn', 'Folders an agent may look through', 'None yet, so it cannot look for photos anywhere.')}
          {folders('backUpTo', 'Folders an agent may back up to', 'None yet, so it cannot back up to a folder.')}
        </>
      )}
    </div>
  )
}

/** What a refusal from Keevoro says, as a sentence. */
function sentence(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}

/** An ending as it is shown: with its dot. */
const dotted = (ending: string) => `.${ending}`

/**
 * The kinds of file Keevoro looks for, for the whole of Keevoro. A built-in
 * kind can be switched off and made a photo, a video or an other file; one
 * of your own can also be renamed and removed. Any ending can be added as an
 * other file (music, documents), carried along and backed up exactly, but
 * never shown or grouped by date. A change counts from the next time a source is
 * looked through, and never takes away anything already found or backed up.
 */
function FileKindsSetting(props: {
  api: Backend
  onSources: () => void
  onChanged: () => void
  onProblem: (error: unknown) => void
}) {
  const { api, onProblem } = props
  const [kinds, setKinds] = useState<FileKind[] | null>(null)
  // Which line is being edited ('' for a new one), with what is typed.
  const [editing, setEditing] = useState<{ was: string; ending: string; kind: ListedAs } | null>(null)
  const [refusal, setRefusal] = useState<string | null>(null)
  const [removing, setRemoving] = useState<string | null>(null)
  const [changed, setChanged] = useState(false)
  const load = useCallback(() => api.fileKinds().then(setKinds).catch(onProblem), [api, onProblem])
  useEffect(() => {
    void load()
  }, [load])

  const done = () => {
    setChanged(true)
    props.onChanged()
    setEditing(null)
    setRefusal(null)
    setRemoving(null)
    return load()
  }
  const save = (kind = editing?.kind) => {
    if (!editing || !kind) return
    const asked =
      editing.was === ''
        ? api.addFileKind(editing.ending, kind)
        : api.changeFileKind(editing.was, { kind, to: editing.ending })
    asked.then(done).catch((e) => setRefusal(sentence(e)))
  }
  const switchOn = (kind: FileKind) =>
    api
      .changeFileKind(kind.ending, { on: !kind.on })
      .then(done)
      .catch(onProblem)

  const form = (title: string) =>
    editing && (
      <form
        className="kind-form"
        aria-label={title}
        onSubmit={(e) => {
          e.preventDefault()
          save()
        }}
      >
        <label>
          Ending
          <input
            value={editing.ending}
            disabled={editing.was !== '' && kinds?.find((k) => k.ending === editing.was)?.builtIn}
            placeholder="Such as heif"
            autoFocus
            spellCheck={false}
            onChange={(e) => setEditing({ ...editing, ending: e.target.value })}
          />
        </label>
        <label>
          They are
          <select
            value={editing.kind}
            onChange={(e) => {
              setRefusal(null)
              setEditing({ ...editing, kind: e.target.value as ListedAs })
            }}
          >
            <option value="photo">Photos</option>
            <option value="video">Videos</option>
            <option value="other">Other files</option>
          </select>
        </label>
        {editing.kind === 'other' && (
          <p className="quiet small what-to-expect">
            Other files are anything you want kept with your photos, such as music or documents. Keevoro will back
            these up exactly, but cannot show them or group them by date.
          </p>
        )}
        <span className="choices">
          <button type="submit" className="primary">
            {editing.was === '' ? 'Add' : 'Save'}
          </button>
          <button
            type="button"
            onClick={() => {
              setEditing(null)
              setRefusal(null)
            }}
          >
            Cancel
          </button>
        </span>
        {refusal && (
          <p className="refusal" role="alert">
            {refusal}
            {editing.kind !== 'other' && /as an other file/.test(refusal) && (
              <>
                {' '}
                <button type="button" className="link" onClick={() => save('other')}>
                  Add it as other files
                </button>
              </>
            )}
          </p>
        )}
      </form>
    )

  return (
    <div className="setting kinds">
      <h3>Kinds of file Keevoro looks for</h3>
      <p className="quiet">
        Keevoro finds photos and videos by the ending of their names, such as .jpg, and for the kinds it knows, by
        looking inside too. Add a kind your camera makes that is missing here. Keevoro backs up a kind it cannot open
        just the same, exactly as it is, and shows “No preview” where its picture would be. One list serves every backup.
      </p>
      <p className="quiet small">
        Changes count from the next time a source is looked through. Switching a kind off or removing it never takes
        away anything already found or backed up.
      </p>
      {changed && (
        <p className="choices again" role="status">
          <span className="quiet small">Changed. Look through your sources again to find files of the new kinds.</span>
          <button className="button small" onClick={props.onSources}>
            Go to your sources
          </button>
        </p>
      )}
      {kinds && (
        <div className="kinds-table" tabIndex={0} aria-label="Kinds of file, scrolls sideways when narrow">
          <table>
            <thead>
              <tr>
                <th scope="col">Ending</th>
                <th scope="col">Kind</th>
                <th scope="col">From</th>
                <th scope="col">Looked for</th>
                <th scope="col">
                  <span className="visually-hidden">Change</span>
                </th>
              </tr>
            </thead>
            <tbody>
              {kinds.map((kind) =>
                removing === kind.ending ? (
                  <tr key={kind.ending} className="removing">
                    <td colSpan={5} role="alertdialog" aria-label={`Remove ${dotted(kind.ending)}?`}>
                      <span>
                        Take <b>{dotted(kind.ending)}</b> off the list? Files of this kind already found or backed up
                        stay where they are.
                      </span>
                      <span className="choices">
                        <button
                          className="danger"
                          onClick={() =>
                            api
                              .removeFileKind(kind.ending)
                              .then(done)
                              .catch(onProblem)
                          }
                        >
                          Yes, take it off
                        </button>
                        <button onClick={() => setRemoving(null)}>Cancel</button>
                      </span>
                    </td>
                  </tr>
                ) : editing?.was === kind.ending ? (
                  <tr key={kind.ending} className="editing">
                    <td colSpan={5}>{form(`Change ${dotted(kind.ending)}`)}</td>
                  </tr>
                ) : (
                  <tr key={kind.ending} className={kind.on ? undefined : 'off'}>
                    <td className="ending">{dotted(kind.ending)}</td>
                    <td>{kind.kind === 'photo' ? 'Photo' : kind.kind === 'video' ? 'Video' : 'Other file'}</td>
                    <td>{kind.builtIn ? 'Built in' : 'Yours'}</td>
                    <td>
                      <button
                        className={kind.on ? 'kv-switch on' : 'kv-switch'}
                        role="switch"
                        aria-checked={kind.on}
                        aria-label={`Look for ${dotted(kind.ending)} files`}
                        onClick={() => switchOn(kind)}
                      >
                        <span className="knob" />
                      </button>
                    </td>
                    <td className="actions">
                      <button
                        className="link"
                        aria-label={`Change ${dotted(kind.ending)}`}
                        onClick={() => {
                          setRemoving(null)
                          setRefusal(null)
                          setEditing({ was: kind.ending, ending: kind.ending, kind: kind.kind })
                        }}
                      >
                        Edit
                      </button>
                      {!kind.builtIn && (
                        <button
                          className="link"
                          aria-label={`Remove ${dotted(kind.ending)}`}
                          onClick={() => {
                            setEditing(null)
                            setRemoving(kind.ending)
                          }}
                        >
                          Remove
                        </button>
                      )}
                    </td>
                  </tr>
                ),
              )}
            </tbody>
          </table>
        </div>
      )}
      {editing?.was === '' && form('Add a kind of file')}
      <p className="choices">
        {editing?.was !== '' && (
          <button
            onClick={() => {
              setRemoving(null)
              setRefusal(null)
              setEditing({ was: '', ending: '', kind: 'photo' })
            }}
          >
            Add a kind of file…
          </button>
        )}
        <button
          onClick={() =>
            api
              .resetFileKinds()
              .then(done)
              .catch(onProblem)
          }
        >
          Put back the built-in list
        </button>
      </p>
    </div>
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
  // Folders of other files are listed apart, by folder, under the collections.
  const [hero, ...rest] = (found?.collections ?? []).filter((c) => !c.byFolder)
  const folders = (found?.collections ?? []).filter((c) => c.byFolder)

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
      {found && (hero || folders.length > 0) && (
        <Tally found={found} onDuplicates={props.onDuplicates} onSetAside={props.onSetAside} />
      )}

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
              <FoundPicture key={p.id} api={props.api} photo={p} />
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
                <FoundPicture key={c.cover.id} api={props.api} photo={c.cover} />
              </button>
              <h4>{c.title}</h4>
              <p>{c.summary}</p>
              <KeepButton collection={c} onKeep={props.onKeep} small />
            </article>
          ))}
        </div>
      )}

      <OtherFilesSection
        folders={folders}
        onOpen={props.onOpen}
        keep={(folder) => <KeepButton collection={folder} onKeep={props.onKeep} small />}
      />

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

/**
 * The figures for a backup, where they can be seen: what is being kept,
 * what was left out as a duplicate and the room that saves, and what was
 * set aside. The last two open their lists, so a number can always be
 * looked into.
 */
function Tally(props: { found: Discover; onDuplicates: () => void; onSetAside?: () => void }) {
  const { found } = props
  const keeping = found.collections.filter((c) => c.kept).reduce((n, c) => n + c.photos + c.videos + (c.others ?? 0), 0)
  const all = found.photos + found.videos + (found.others ?? 0) + found.duplicates
  return (
    <dl className="tally" aria-label="This backup in figures">
      <div>
        <dt>Keeping</dt>
        <dd>{thousands(keeping)}</dd>
        <dd className="quiet small">
          of {files(found.photos, found.videos, found.others ?? 0)} found · {size(found.bytes)}
        </dd>
      </div>
      <button className="stat" onClick={props.onDuplicates} disabled={found.duplicates === 0}>
        <dt>Duplicates left out</dt>
        <dd>{thousands(found.duplicates)}</dd>
        <dd className="quiet small">
          {found.duplicates === 0
            ? 'No file was found more than once'
            : `saving ${size(found.duplicateBytes)} · ${Math.round((found.duplicates / Math.max(all, 1)) * 100)}% of all files found · see them`}
        </dd>
      </button>
      {props.onSetAside && (
        <button className="stat" onClick={props.onSetAside} disabled={found.junk === 0}>
          <dt>Set aside</dt>
          <dd>{thousands(found.junk)}</dd>
          <dd className="quiet small">
            {found.junk === 0 ? 'Nothing looked like it was not a photo' : 'pictures that don’t look like photos · see them'}
          </dd>
        </button>
      )}
    </dl>
  )
}

/** Step 3. One question: where should they go? Then one button: send. */
function SendScreen(props: {
  api: Backend
  found: Discover | null
  onDuplicates: () => void
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
      {props.found && <Tally found={props.found} onDuplicates={props.onDuplicates} />}
      <SendPanel
        api={props.api}
        destinations={props.found?.destinations ?? []}
        withOtherFiles={(props.found?.others ?? 0) > 0}
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
  /** Each place this backup goes to, with what has reached it. */
  destinations: Destination[]
  /** The backup holds other files, so what is in it says where they go. */
  withOtherFiles?: boolean
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
  // True while a further destination is being chosen for a backup that has one.
  const [adding, setAdding] = useState(false)
  const { destinations } = props
  // Whether this backup also makes previews, or sends the originals alone.
  const [previews, setPreviews] = useState(true)
  useEffect(() => {
    api.previewsWanted().then(setPreviews).catch(() => {})
  }, [api])
  // How the send in hand is getting on, asked once a second while it runs.
  const [progress, setProgress] = useState<SendProgress | null>(null)
  // Earlier sends of this backup, for comparing one with another.
  const [history, setHistory] = useState<Sent[]>([])
  const sending = running !== null
  // The engine's line is what it is doing, then after a tab the file it
  // has just dealt with. A check has no estimate, so any "3 of 10" in its
  // line drives its bar.
  const [doingNow, latestFile] = (running ?? '').split('\t')
  const steadyFile = useSteady(latestFile, 700)
  const counts = /([\d,]+) of ([\d,]+)/.exec(doingNow)
  const counted = counts
    ? { done: Number(counts[1].replace(/,/g, '')), total: Number(counts[2].replace(/,/g, '')) }
    : null
  // A backup's own estimate of the whole of it: how long it has run, how
  // long is left, and every stage. Worked out by Keevoro, the same words
  // the command line and an agent are given.
  const estimate = progress?.estimate ?? null
  useEffect(() => {
    if (!sending) return
    let open = true
    // Answers can come back late or out of order when the disk is busy, and
    // a bar that slips backwards looks like lost work. So an answer older
    // than one already shown is dropped, and within one backup the amount
    // done only ever goes up.
    let asked = 0
    let shown = 0
    let furthest = 0
    const ask = () => {
      const mine = ++asked
      return api
        .sendProgress()
        .then((p) => {
          if (!open || mine < shown) return
          shown = mine
          // A re-estimate can make the share done slip back a little; the
          // bar holds where it was rather than looking like lost work, but
          // gives way to a reading well below it (see `heldShare`).
          if (p.estimate) {
            furthest = heldShare(furthest, p.estimate.fractionDone)
            setProgress({ ...p, estimate: { ...p.estimate, fractionDone: furthest } })
          } else {
            setProgress(p)
          }
        })
        .catch(() => {})
    }
    void ask()
    const timer = window.setInterval(ask, 1000)
    return () => {
      open = false
      window.clearInterval(timer)
    }
  }, [api, sending])
  // How much room is needed, and whether where it is going has it.
  const [room, setRoom] = useState<Room | null>(null)
  // Asking for the room first looks at what is really at each destination
  // (unless Keevoro did a moment ago), so the first answer is followed by
  // reading the counts again, once, to show what that look found.
  const lookedOnOpen = useRef(false)
  const countAgain = useRef(onSent)
  useEffect(() => {
    countAgain.current = onSent
  }, [onSent])
  const weigh = useCallback(
    () =>
      api
        .room()
        .then((weighed) => {
          setRoom(weighed)
          if (!lookedOnOpen.current) {
            lookedOnOpen.current = true
            countAgain.current()
          }
        })
        .catch(onProblem),
    [api, onProblem],
  )

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
          setRunning((now) => now ?? (p.line || 'Backing up…'))
        }
      })
      .catch(() => {})
      .finally(() => setAsked(true))
  }, [api])

  // What the last backup said when it ended stays on screen until the
  // next one, however many other screens are looked at in between.
  useEffect(() => {
    api
      .lastBackupSaid()
      .then((last) => last && setSaid((now) => now ?? last))
      .catch(() => {})
  }, [api])

  useEffect(() => {
    api.destination().then(setWhere).catch(onProblem)
    return api.onSend((event) => {
      if (event.finished) {
        setRunning(null)
        setStopping(false)
        setProgress(null)
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
      .then(() => api.destination())
      .then((description) => {
        // With more than one, this is how many; the list names each.
        setWhere(description)
        onWhere(description)
        setForm(null)
        setAdding(false)
        // What the last backup or check said was about the places as they were.
        setSaid(null)
        // The counts change: a new place has nothing in it yet.
        onSent()
      })
      // A wrong key is the user's to fix, so it is said beside the form.
      .catch((error) => setRefused(error instanceof Error ? error.message : String(error)))
      .finally(() => setConnecting(false))
  }
  // Stops backing up to one place (or to the only one). Nothing there is
  // deleted; it only comes off this backup's list.
  const remove = (at?: number) =>
    api
      .disconnectDestination(at)
      .then(() => api.destination())
      .then((description) => {
        setWhere(description)
        onWhere(description)
        setSaid(null)
        onSent()
      })
      .catch(onProblem)
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
      {where === null || adding ? (
        <>
          {adding && (
            <div className="adding">
              <h2>Add another destination</h2>
              <p className="quiet">
                Keevoro will back up to this one as well as {where}. It starts empty and catches up on the next
                backup. A photo only counts as safe once it is at every destination.
              </p>
              <p>
                <button
                  className="button small"
                  onClick={() => {
                    setAdding(false)
                    setForm(null)
                    setRefused(null)
                  }}
                >
                  Not now
                </button>
              </p>
            </div>
          )}
          {room && room.needed > 0 && (
            <p className="needs">
              You are backing up {props.waiting.toLocaleString('en-GB')} {props.waiting === 1 ? 'file' : 'files'}, about{' '}
              <b>{size(room.needed)}</b>. Wherever you choose needs at least that much free.
            </p>
          )}
          {!form && (
            <div className="places destinations">
              <button
                className="place"
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
            {destinations.length > 1 ? (
              `${destinations.length} places`
            ) : room?.folder ? (
              <>
                the folder <FolderLink api={api} path={room.folder} onProblem={onProblem} />
              </>
            ) : (
              where
            )}
          </h2>
          {destinations.length > 1 && (
            <ul className="places-to" aria-label="Where this backup goes">
              {destinations.map((to, at) => (
                <li key={to.name}>
                  <span className="name">
                    {to.folder ? (
                      <>
                        The folder <FolderLink api={api} path={to.folder} onProblem={onProblem} />
                      </>
                    ) : (
                      to.name.replace(/^the /, 'The ')
                    )}
                  </span>
                  <span className={to.waiting > 0 || outOfReachLine(to) ? 'quiet small' : 'quiet small all-there'}>
                    {outOfReachLine(to) ??
                      (to.waiting === 0
                        ? `All ${thousands(to.there)} there`
                        : `${thousands(to.there)} there · ${thousands(to.waiting)} waiting (${size(to.waitingBytes)})`)}
                  </span>
                  {!running && to.folder && (
                    <ClearUnfinished api={api} folder={to.folder} label="Clear unfinished copies…" small onProblem={onProblem} />
                  )}
                  {!running && (
                    <button
                      className="button small"
                      onClick={() => remove(at)}
                      aria-label={`Stop backing up to ${to.name}`}
                      title="Takes it off this backup. Nothing backed up there is deleted."
                    >
                      Remove
                    </button>
                  )}
                </li>
              ))}
            </ul>
          )}
          {/* While a backup runs its own progress says what is left; these
              counts are from before it began and would only mislead. */}
          {!sending && <WhatWasFound destinations={destinations} />}
          {!sending && <ThereNow destinations={destinations} waiting={props.waiting} safe={props.safe} />}
          {!sending && room && room.needed > 0 && (
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
          {running && (
            <div className="speed" role="status" aria-live="off">
              {/* One bar for the whole backup, the same length whatever it says. */}
              {estimate ? (
                <progress value={estimate.fractionDone} max={1} />
              ) : counted ? (
                <progress value={counted.done} max={counted.total} />
              ) : (
                <progress />
              )}
              <p>
                {estimate
                  ? estimate.headline
                  : counted
                    ? `${thousands(counted.done)} of ${thousands(counted.total)} done`
                    : 'Getting ready…'}
              </p>
              {estimate && estimate.stages.length > 0 && (
                <ol className="stages" aria-label="What this backup does">
                  {estimate.stages.map((stage) => (
                    <li key={`${stage.kind} ${stage.to ?? ''}`} className={`is-${stage.state}`}>
                      <span className="mark" aria-hidden="true" />
                      <span>
                        <span className="visually-hidden">{STAGE_STATE_WORDS[stage.state]}</span>
                        {stage.line}
                      </span>
                    </li>
                  ))}
                </ol>
              )}
              {estimate?.stages.some((stage) => stage.kind === 'makingPreviews' || stage.kind === 'sendingPreviews') && (
                <p className="quiet small">
                  Previews are made in the background while your files are backed up, and sent after them. To back
                  up the originals alone next time, switch off “Also make previews for a gallery” once this backup
                  ends.
                </p>
              )}
            </div>
          )}
          {running ? (
            <>
            <p className="running">
              <Spinner /> {stopping ? 'Stopping after this file…' : doingNow}
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
            {steadyFile && !stopping && (
              <p className="quiet small latest" aria-live="off">
                Just done: <span className="where">{steadyFile}</span>
              </p>
            )}
            </>
          ) : (
            <>
            <label className="switch-row previews">
              <button
                className={previews ? 'kv-switch on' : 'kv-switch'}
                role="switch"
                aria-checked={previews}
                aria-label="Also make previews for a gallery"
                onClick={() => {
                  const wanted = !previews
                  setPreviews(wanted)
                  api.setPreviewsWanted(wanted).catch(onProblem)
                }}
              >
                <span className="knob" />
              </button>
              <span>
                <b>Also make previews for a gallery</b>
                <span className="quiet small">
                  {previews
                    ? 'While your files are backed up, Keevoro makes small and screen-sized versions of each photo and a still of each video in the background, gently, and backs those up once your files are safe. Switch this off to back up the originals alone, which is quicker.'
                    : 'Only your original files will be backed up, exactly as they are. No thumbnails or other versions are made. You can switch this back on later and they will be added then.'}
                </span>
              </span>
            </label>
            <div className="choices">
              <button
                className="primary"
                onClick={() => start('Starting…', api.send)}
                disabled={props.waiting + props.safe === 0 || (room !== null && !room.enough)}
              >
                {props.waiting > 0 ? 'Back up now' : 'Back up again'}
              </button>
              <button
                className="button"
                onClick={() => start('Checking…', api.checkArrived)}
                disabled={props.safe === 0 && !destinations.some((d) => d.there > 0)}
              >
                Check they all arrived
              </button>
              {destinations.length <= 1 && where.startsWith('the folder') && (
                <button className="button" onClick={() => api.openDestination().catch(onProblem)}>
                  Show me the folder
                </button>
              )}
              {destinations.length <= 1 && destinations[0]?.folder && (
                <ClearUnfinished
                  api={api}
                  folder={destinations[0].folder}
                  label="Clear unfinished copies…"
                  onProblem={onProblem}
                />
              )}
              <button className="button" onClick={() => setAdding(true)}>
                Add another destination
              </button>
              {destinations.length <= 1 && (
                <button
                  className="button"
                  onClick={() => remove()}
                  title="Stops backing up here, so you can choose somewhere else. Nothing backed up there is deleted."
                >
                  Back up somewhere else
                </button>
              )}
            </div>
            </>
          )}
          {/* What the last backup said is about that one, so it steps aside while another runs. */}
          {!running &&
            said?.summary?.map((line) => (
              <p className="done" key={line}>
                {line}
              </p>
            ))}
          {!running && said?.problem && (
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
                {props.withOtherFiles && (
                  <>
                    {' '}
                    Other files you asked Keevoro to carry along, such as music and documents, are in{' '}
                    <code>Other files</code> inside it, in the folders they came from. Keevoro makes no renditions of
                    them.
                  </>
                )}
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
        {history.length > 0 && !running && <History history={history} />}
        </>
      )}
    </div>
  )
}

/** A moment, in seconds since 1970, as "18 Nov, 08:51". */
const when = (seconds: number) =>
  new Date(seconds * 1000).toLocaleString('en-GB', { day: 'numeric', month: 'short', hour: '2-digit', minute: '2-digit' })

/** A length of time, as the rest of the app says it, with "under a minute" for the shortest. */
const took = (seconds: number) => about(seconds).replace('less than a minute', 'under a minute')

/** A speed, per second for bytes; files a second, a minute or an hour. */
function rate(perSecond: number, unit: 'bytes' | 'files') {
  if (unit === 'bytes') return `${size(perSecond)} a second`
  if (perSecond >= 1) return `${count(Math.round(perSecond), 'file')} a second`
  if (perSecond * 60 >= 1) return `${count(Math.round(perSecond * 60), 'file')} a minute`
  return `${count(Math.round(perSecond * 3600), 'file')} an hour`
}

const STAGE_NAMES: Record<StageRecord['kind'], string> = {
  tellingApart: 'Telling apart files that look alike',
  placing: 'Working out where each file goes',
  sendingOriginals: 'Backing up your files',
  makingPreviews: 'Making previews',
  makingVideoPreviews: 'Making video previews',
  sendingPreviews: 'Backing up the previews',
  writingIndex: 'Writing the list of what is there',
}

/**
 * How earlier backups went: one row each, newest first, and every row
 * opens in place to everything Keevoro recorded about that backup.
 */
function History(props: { history: Sent[] }) {
  const [open, setOpen] = useState<number | null>(null)
  return (
    <details className="sends">
      <summary>How earlier backups went</summary>
      <div className="sends-scroll" tabIndex={0} aria-label="How earlier backups went, scrolls sideways when narrow">
        <table>
          <thead>
            <tr>
              <th scope="col">When</th>
              <th scope="col">Where</th>
              <th scope="col">Backed up</th>
              <th scope="col">Took</th>
              <th scope="col">Average speed</th>
              <th scope="col">
                <span className="visually-hidden">Details</span>
              </th>
            </tr>
          </thead>
          <tbody>
            {props.history.map((sent, at) => (
              <HistoryRow
                key={`${sent.finishedAt}-${at}`}
                sent={sent}
                id={`backup-run-${at}`}
                open={open === at}
                onToggle={() => setOpen(open === at ? null : at)}
              />
            ))}
          </tbody>
        </table>
      </div>
    </details>
  )
}

function HistoryRow(props: { sent: Sent; id: string; open: boolean; onToggle: () => void }) {
  const { sent } = props
  return (
    <>
      <tr className={props.open ? 'open' : undefined}>
        <td>{when(sent.finishedAt)}</td>
        <td className="where">{sent.to.replace(/^the /, '')}</td>
        <td>
          {size(sent.bytes)}
          {sent.stopped && ' (stopped)'}
        </td>
        <td>{took(sent.seconds)}</td>
        <td>{size(sent.bytes / Math.max(sent.seconds, 1))}/s</td>
        <td>
          <button
            className="link"
            aria-expanded={props.open}
            aria-controls={props.id}
            aria-label={`${props.open ? 'Hide' : 'Show'} everything about the backup on ${when(sent.finishedAt)}`}
            onClick={props.onToggle}
          >
            {props.open ? 'Hide' : 'Details'}
          </button>
        </td>
      </tr>
      {props.open && (
        <tr className="run">
          <td colSpan={6} id={props.id}>
            <RunDetailPanel sent={sent} />
          </td>
        </tr>
      )}
    </>
  )
}

/** One pass of sending, in a sentence. */
function sending(what: string, s: Sending) {
  const notSent = s.notSent.length + s.notSentMore
  return [
    `${count(s.sent, what)} sent (${size(s.sentBytes)})`,
    s.alreadyThere > 0 && `${thousands(s.alreadyThere)} already there`,
    notSent > 0 && `${thousands(notSent)} could not be sent`,
  ]
    .filter(Boolean)
    .join(', ')
}

/** Everything recorded about one backup, from the headline down. */
function RunDetailPanel(props: { sent: Sent }) {
  const { sent } = props
  const d = sent.detail
  const headline = `${size(sent.bytes)} to ${sent.to.replace(/^the /, 'the ')} in ${took(sent.seconds)}, at an average of ${size(
    sent.bytes / Math.max(sent.seconds, 1),
  )} a second.`
  if (!d) {
    return (
      <div className="run-detail">
        <p className="headline">{headline}</p>
        <p className="quiet small">
          Only these figures were kept for this backup. Fuller detail is kept for every backup from now on.
        </p>
      </div>
    )
  }
  const ended =
    d.ended === 'finished'
      ? 'It finished.'
      : d.ended === 'stopped'
        ? 'It was stopped.'
        : `It ended with a problem${d.problem ? `: ${d.problem}` : '.'}`
  return (
    <div className="run-detail">
      <p className="headline">{headline}</p>
      <p className="quiet small">
        Started {when(d.startedAt)} from {d.startedBy}, with Keevoro {d.version}. {ended}
        {d.pausedSeconds >= 60 && ` It was paused for ${took(d.pausedSeconds)}, which is not counted in the stages.`}
      </p>

      <h4>Stages</h4>
      <div className="stages-scroll">
        <table className="stages">
          <thead>
            <tr>
              <th scope="col">Stage</th>
              <th scope="col">How long</th>
              <th scope="col">How much</th>
              <th scope="col">Speed</th>
            </tr>
          </thead>
          <tbody>
            {d.stages.map((stage, at) => (
              <tr key={at}>
                <td>
                  {STAGE_NAMES[stage.kind]}
                  {stage.to && d.destinations.length > 1 && <span className="quiet"> to {stage.to.replace(/^the /, '')}</span>}
                  {!stage.finished && <span className="quiet"> (not finished)</span>}
                  {stage.problem && (
                    <span className="went-wrong">
                      <span className="mark" aria-hidden="true" /> Went wrong: {stage.problem}
                    </span>
                  )}
                </td>
                <td>{took(stage.seconds)}</td>
                <td>{stage.unit === 'bytes' ? size(stage.done) : count(stage.done, 'file')}</td>
                <td>
                  {stage.perSecond !== null ? rate(stage.perSecond, stage.unit) : ''}
                  {stage.slowest !== null && stage.fastest !== null && (
                    <span className="quiet small range">
                      {' '}
                      slowest {rate(stage.slowest, stage.unit)}, fastest {rate(stage.fastest, stage.unit)}
                    </span>
                  )}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      <p className="quiet small">Slowest and fastest are speeds held for a minute or more.</p>

      <h4>Your files</h4>
      <ul>
        {d.destinations.map((to) => (
          <li key={to.to}>
            {d.destinations.length > 1 && <>To {to.to.replace(/^the /, 'the ')}: </>}
            {sending('file', to.originals)}.
            {to.originals.notSent.length > 0 && (
              <ul className="not-sent">
                {to.originals.notSent.map((n) => (
                  <li key={n.file}>
                    <span className="where">{n.file}</span>: {n.reason.replace(/\.$/, '')}.
                  </li>
                ))}
                {to.originals.notSentMore > 0 && <li>and {thousands(to.originals.notSentMore)} more.</li>}
              </ul>
            )}
          </li>
        ))}
        {d.duplicates > 0 && (
          <li>
            {count(d.duplicates, 'duplicate')} left out, saving {size(d.duplicateBytes)}.
          </li>
        )}
      </ul>

      <h4>Previews</h4>
      {d.previews ? (
        <ul>
          <li>
            Made {count(d.previews.photos, 'photo preview')} and {count(d.previews.videos, 'video still')}.
          </li>
          {d.previews.photosWithout + d.previews.videosWithout > 0 && (
            <li>
              Without a preview, because Keevoro couldn’t open them: {count(d.previews.photosWithout, 'photo')} and{' '}
              {count(d.previews.videosWithout, 'video')}.
            </li>
          )}
          {d.destinations
            .filter((to) => to.previews)
            .map((to) => (
              <li key={to.to}>
                {d.destinations.length > 1 && <>To {to.to.replace(/^the /, 'the ')}: </>}
                {sending('preview file', to.previews!)}.
              </li>
            ))}
        </ul>
      ) : (
        <p>None: this backup sends the originals alone.</p>
      )}

      <h4>Where it went</h4>
      <ul>
        {d.destinations.map((to) => (
          <li key={to.to}>
            <span className="where">{to.to.replace(/^the /, '')}</span>
            {to.overNetwork && ', across a network'}
            {to.reached
              ? `: ${size(to.bytes)} in ${took(to.seconds)}${to.indexed !== undefined ? `, its index lists ${count(to.indexed, 'file')}` : ''}.`
              : `: couldn’t be backed up to. ${to.problem ?? ''}`}
            {!!to.clearedBytes && (
              <>
                {' '}
                Cleared {size(to.clearedBytes)} of unfinished copies left by an earlier backup that was stopped. They were
                Keevoro’s working files, not part of your backup.
              </>
            )}
          </li>
        ))}
      </ul>

      {d.summary.length > 0 && (
        <>
          <h4>What it said at the end</h4>
          <div className="summary-lines">
            {d.summary.map((line, at) => (
              <p key={at}>{line}</p>
            ))}
          </div>
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
  // Keeps or leaves out everything here in one go.
  const keepAll = (kept: boolean) => {
    if (!collection) return
    setPhotos((list) => list && list.map((p) => ({ ...p, kept })))
    api.keepCollection(collection.key, kept).catch(onProblem)
  }
  // The folders these files are in on their sources, for opening.
  const folders = [...new Set((photos ?? []).map((p) => p.sourcePath.replace(/[\\/][^\\/]*$/, '')))].sort()

  // A click opens a picture large; a double click ticks or unticks it.
  // The two would trip over each other (the first click of a double click
  // would open the picture), so a click waits a moment to see whether a
  // second one follows before it opens anything. The tick in the corner
  // is a button of its own, for ticking with one click.
  const [looking, setLooking] = useState<string | null>(null)
  const opening = useRef<number | undefined>(undefined)
  const clicked = (id: string) => {
    window.clearTimeout(opening.current)
    opening.current = window.setTimeout(() => setLooking(id), DOUBLE_CLICK_WAIT)
  }
  const doubleClicked = (photo: PhotoChoice) => {
    window.clearTimeout(opening.current)
    toggle(photo)
  }
  useEffect(() => () => window.clearTimeout(opening.current), [])
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
              ? `Keeping ${thousands(keeping)} of ${thousands(photos.length)}. Click a picture to see it larger. Double-click it, or click its tick, to leave it out or put it back.`
              : collection.summary
            : 'These look like app pictures or leftovers, not memories. Click one to see it larger. Double-click it, or click its tick, to keep it after all.'}
        </p>
        {collection && photos && photos.length > 0 && (
          <p className="choices within">
            <button className="button small" onClick={() => keepAll(true)} disabled={keeping === photos.length}>
              Keep all
            </button>
            <button className="button small" onClick={() => keepAll(false)} disabled={keeping === 0}>
              Keep none
            </button>
          </p>
        )}
        {folders.length > 0 && (
          <p className="quiet small from">
            {folders.length === 1 ? 'From ' : `From ${folders.length} folders: `}
            {folders.slice(0, 4).map((folder, i) => (
              <span key={folder}>
                {i > 0 && ', '}
                <button className="link where" onClick={() => api.openPlace(folder).catch(onProblem)} title="Open this folder">
                  {folder}
                </button>
              </span>
            ))}
            {folders.length > 4 && ` and ${folders.length - 4} more`}
          </p>
        )}
      </header>
      {photos && photos.length === 0 && (
        <div className="empty quiet">{collection ? 'Nothing here.' : 'Nothing is set aside.'}</div>
      )}
      {photos && photos.length > 0 && (
        <VirtualTiles
          label={collection ? `Photos and videos in ${collection.title}` : 'Pictures set aside'}
          sections={[
            {
              key: collection?.key ?? 'set aside',
              count: photos.length,
              load: async (from, count) => ({ total: photos.length, items: photos.slice(from, from + count) }),
            },
          ]}
          // The list in hand is the truth: a tick changes it, not the page fetched.
          tile={(_, index) => {
            const photo = photos[index]
            if (!photo) return null
            return (
              <div className={photo.kept ? 'tile' : 'tile out'}>
                <button
                  className="look"
                  onClick={() => clicked(photo.id)}
                  onDoubleClick={() => doubleClicked(photo)}
                  aria-label={`Look at ${photo.name}`}
                >
                  <FoundPicture key={photo.id} api={api} photo={photo} />
                </button>
                <button
                  className="tick"
                  role="switch"
                  aria-checked={photo.kept}
                  aria-label={`Keep ${photo.name}`}
                  title={photo.kept ? 'Keeping. Click to leave it out.' : 'Left out. Click to keep it.'}
                  onClick={() => toggle(photo)}
                >
                  {photo.kept ? '✓' : ''}
                </button>
                {photo.junkReason && <span className="why">{photo.junkReason}</span>}
              </div>
            )
          }}
        />
      )}
      {photos && at >= 0 && (
        <Viewer
          item={{ key: photos[at].id, name: photos[at].name, isVideo: photos[at].isVideo, where: photos[at].sourcePath }}
          small={() => api.preview(photos[at].id)}
          large={() => api.closerLook(photos[at].id)}
          place={`${thousands(at + 1)} of ${thousands(photos.length)}`}
          keep={{ kept: photos[at].kept, onKeep: () => toggle(photos[at]) }}
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

/**
 * How long a click waits, in milliseconds, to see whether it is the first
 * half of a double click. Short enough not to feel slow, long enough for
 * an unhurried double click.
 */
const DOUBLE_CLICK_WAIT = 260

/**
 * What a survey of a drive found: the folders that seem to hold photos
 * and videos, each with a tick. The person chooses, then Keevoro looks
 * through the ticked ones properly, one after another.
 */
/**
 * Folders Keevoro made (a backup, a library from an older version, its own
 * files) that a search or survey left out: one quiet line that opens to
 * say what each is, with a way to see it. Keevoro never removes them.
 */
function LeftOutNote(props: { api: Backend; leftOut?: LeftOut[]; onProblem: (error: unknown) => void }) {
  // Those the person left out are said apart, each with Put back.
  const leftOut = (props.leftOut ?? []).filter((left) => left.what !== 'leftOutByYou')
  if (leftOut.length === 0) return null
  return (
    <details className="left-out">
      <summary>
        Left out {leftOut.length === 1 ? 'a folder' : `${leftOut.length} folders`} Keevoro made, as{' '}
        {leftOut.length === 1 ? 'it is' : 'they are'} not a source
      </summary>
      <ul>
        {leftOut.map((left) => (
          <li key={left.folder}>
            <p>{left.said}</p>
            <button className="button small" onClick={() => props.api.openPlace(left.folder).catch(props.onProblem)}>
              Show me the folder
            </button>
          </li>
        ))}
      </ul>
    </details>
  )
}

function SurveyResult(props: {
  api: Backend
  onProblem: (error: unknown) => void
  name: string
  found: Survey
  chosen: Set<string>
  onToggle: (folder: string) => void
  onGo: () => void
  /** Opens a grid of what is in one suggested folder, before it is chosen. */
  onLookInside: (folder: string) => void
  /** Opens where the space goes in the place surveyed. */
  onSeeSpace: () => void
  onCancel: () => void
}) {
  const { found, chosen } = props
  // Suggestions left out from here, and what that did, said once.
  const [outNow, setOutNow] = useState<Set<string>>(new Set())
  const [said, setSaid] = useState<LeftOutFolder | null>(null)
  const picked = found.hints.filter((h) => chosen.has(h.folder) && !outNow.has(h.folder))
  const photos = picked.reduce((n, h) => n + h.photos, 0)
  const videos = picked.reduce((n, h) => n + h.videos, 0)
  const others = picked.reduce((n, h) => n + (h.others ?? 0), 0)
  const bytes = picked.reduce((n, h) => n + h.bytes, 0)
  // Other files are only spoken of when some are among the suggestions.
  const what = found.hints.some((h) => (h.others ?? 0) > 0) ? 'photos, videos and other files' : 'photos and videos'
  return (
    <div className="panel survey">
      <h2>
        {found.hints.length === 0
          ? `No photos or videos were spotted on ${props.name}`
          : `${count(found.hints.length, 'folder')} on ${props.name} ${found.hints.length === 1 ? 'seems' : 'seem'} to hold ${what}`}
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
                  checked={chosen.has(hint.folder) && !outNow.has(hint.folder)}
                  disabled={hint.alreadyASource || outNow.has(hint.folder)}
                  onChange={() => props.onToggle(hint.folder)}
                />
                <span className="where">{hint.folder}</span>
              </label>
              <span className="quiet small">
                {files(hint.photos, hint.videos, hint.others ?? 0)} · {size(hint.bytes)}
              </span>
              <button
                className="button small"
                onClick={() => props.onLookInside(hint.folder)}
                aria-label={`Look inside ${hint.folder}`}
                title="See its photos and videos first. Nothing is added or changed."
              >
                Look inside
              </button>
              {!hint.alreadyASource && (
                <LeaveOutButton
                  api={props.api}
                  folder={hint.folder}
                  leftOut={outNow.has(hint.folder)}
                  words="Leave out"
                  small
                  onChanged={(done) => {
                    setSaid(done)
                    setOutNow((now) => {
                      const next = new Set(now)
                      if (done.leftOut) next.add(hint.folder)
                      else next.delete(hint.folder)
                      return next
                    })
                  }}
                  onProblem={props.onProblem}
                />
              )}
              {outNow.has(hint.folder) && <span className="badge">Left out of this backup</span>}
              {hint.alreadyASource && <span className="badge">Already a source</span>}
              {!hint.likelyPhotos && !hint.alreadyASource && (
                <span className="badge" title="The pictures here are very small and there are no videos">
                  Small pictures, probably not photos
                </span>
              )}
              {hint.onlineOnlySaid && <p className="online-only small">{hint.onlineOnlySaid}</p>}
            </li>
          ))}
        </ul>
      )}
      <LeftOutSaid done={said} />
      <LeftOutNote api={props.api} leftOut={found.leftOut} onProblem={props.onProblem} />
      <LeftOutByYou
        api={props.api}
        folders={leftOutByYou(found.leftOut)}
        title="Passed by, as you left these out of this backup:"
        onChanged={setSaid}
        onProblem={props.onProblem}
      />
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
        <button className="button" onClick={props.onSeeSpace}>
          See where the space goes
        </button>
        <button className="button" onClick={props.onCancel}>
          {found.hints.length > 0 ? 'Cancel' : 'Back'}
        </button>
        {picked.length > 0 && (
          <span className="quiet small">
            About {files(photos, videos, others)}, {size(bytes)}.
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
              : `${count(found.length, 'file')} ${found.length === 1 ? 'was' : 'were'} found more than once. One copy of each is kept, and ${thousands(leftOut)} extra ${leftOut === 1 ? 'copy is' : 'copies are'} left out, saving ${size(found.reduce((n, d) => n + d.bytes * d.copies.length, 0))}.`
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
            <FoundPicture key={d.id} api={api} photo={d} />
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
