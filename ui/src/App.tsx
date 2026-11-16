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
  CheckEvent,
  ScanEvent,
  Session,
  Update,
} from './backend'
import { count, photosAndVideos, size, thousands, wholePlace } from './format'
import iconUrl from './assets/keevoro-icon.png'
import lockupDarkUrl from './assets/keevoro-lockup-reversed.png'
import lockupUrl from './assets/keevoro-lockup.png'
import { chooseTheme, savedTheme } from './theme'
import type { Theme } from './theme'

type Step = 'find' | 'choose' | 'backup'
/** Looking inside a collection, or at what was set aside, sits on top of step 2. */
type Overlay = { collection: Collection } | 'setAside' | 'settings' | null

const STEPS: { id: Step; title: string }[] = [
  { id: 'find', title: 'Find your photos' },
  { id: 'choose', title: 'Choose what to keep' },
  { id: 'backup', title: 'Send them somewhere safe' },
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
  const [problem, setProblem] = useState<string | null>(null)
  // A newer Keevoro, and how far along installing it is.
  const [update, setUpdate] = useState<Update | null>(null)
  const [updating, setUpdating] = useState<'no' | 'downloading' | 'opened'>('no')
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
    let stopCheck = () => {}
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
    }
  }, [refresh, report])

  if (!api || !state) return <div className="blank" />

  // Opening a session (a new one, or another one) shows what it holds.
  const openSession = (opened: Promise<AppState>) =>
    opened
      .then(async (s) => {
        setState(s)
        setOverlay(null)
        setJustFound(null)
        const d = await api.discover()
        setFound(d)
        const where = await api.backupState()
        setBackedUpTo(where)
        setStep(nextToDo(d, where))
      })
      .catch(report)

  if (!state.session) {
    return <Welcome onStart={(name) => openSession(api.startSession(name))} problem={problem} />
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
    choose: anythingFound && (step === 'backup' || safe > 0),
    backup: safe > 0 && waiting === 0,
  }
  const note: Record<Step, string> = {
    find: found && found.sources.length > 0 ? count(found.sources.length, 'source') : 'Start here',
    choose: anythingFound ? `Keeping ${thousands(waiting + safe)}` : '',
    backup: safe > 0 ? `${thousands(safe)} safe` : backedUpTo ? 'Ready to send' : '',
  }
  // A step can be opened once the one before it has something to show.
  const open: Record<Step, boolean> = { find: true, choose: anythingFound, backup: anythingFound }

  return (
    <div className="app">
      <aside className="sidebar">
        <div className="brand">
          <Logo />
        </div>
        <SessionPicker
          session={state.session}
          sessions={state.sessions}
          onSwitch={(id) => openSession(api.switchSession(id))}
          onStart={(name) => openSession(api.startSession(name))}
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

        {overlay === 'settings' ? (
          <SettingsScreen
            api={api}
            state={state}
            backedUpTo={backedUpTo ?? null}
            update={update}
            onUpdate={setUpdate}
            onLibrary={(s) => {
              setState(s)
              void refresh(api)
            }}
            onBackup={() => {
              setOverlay(null)
              setStep('backup')
            }}
            onBack={() => setOverlay(null)}
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
                  button="Choose where they go"
                  onClick={() => go('backup')}
                />
              ) : safe > 0 ? (
                <Next back={() => go('find')} say="Everything you’re keeping has already been sent." button="Next" onClick={() => go('backup')} />
              ) : (
                <Next back={() => go('find')} say="Nothing is being kept yet. Choose at least one collection." />
              )
            }
          />
        ) : (
          <BackupScreen
            api={api}
            waiting={waiting}
            safe={safe}
            onProblem={report}
            onBackedUpTo={setBackedUpTo}
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
  if (found.waitingToGather > 0) return sendingTo ? 'backup' : 'choose'
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

/** The open session's name, and the way to another one or a new one. */
function SessionPicker(props: {
  session: Session | null
  sessions: Session[]
  onSwitch: (id: string) => void
  onStart: (name: string) => void
}) {
  const [open, setOpen] = useState(false)
  const [naming, setNaming] = useState<string | null>(null)
  const others = props.sessions.filter((s) => s.id !== props.session?.id)
  const close = () => {
    setOpen(false)
    setNaming(null)
  }
  return (
    <div className="session">
      <button className="current" onClick={() => (open ? close() : setOpen(true))} aria-expanded={open} aria-label="Backup">
        <span>
          <small>Backup</small>
          {props.session?.name}
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
            <button className="link" onClick={() => setNaming('')}>
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
  const [places, setPlaces] = useState<Place[] | null>(null)
  // The source the person has asked to remove, until they confirm.
  const [removing, setRemoving] = useState<number | null>(null)
  // A whole disk the person picked, until they say they mean all of it.
  const [asking, setAsking] = useState<{ path: string; name: string; detail?: string } | null>(null)

  const look = useCallback(() => {
    api.places().then(setPlaces).catch(onProblem)
  }, [api, onProblem])
  // Drives come and go, so look again whenever the window is returned to.
  useEffect(() => {
    look()
    window.addEventListener('focus', look)
    return () => window.removeEventListener('focus', look)
  }, [look])

  const start = (path: string) => {
    setAsking(null)
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
              {scan.folder && (
                <>
                  . Now in <span className="where">{scan.folder}</span>
                </>
              )}
            </p>
          )}
        </div>
      ) : asking ? (
        <div className="panel asking" role="alertdialog" aria-labelledby="asking-title">
          <h2 id="asking-title">Look through all of {asking.name}?</h2>
          <p>
            That is a whole disk{asking.detail ? ` (${asking.detail.split(' · ').pop()})` : ''}, and most of what is on
            it will not be photos. It can take anything from a few minutes to an hour or more. You can leave it
            running, and nothing on it is changed.
          </p>
          <p className="quiet">If you know roughly where your photos are, choosing that folder is much quicker.</p>
          <p className="choices">
            <button className="primary" onClick={pick}>
              Choose a folder inside it
            </button>
            <button onClick={() => start(asking.path)}>Look through all of it</button>
            <button className="link" onClick={() => setAsking(null)}>
              Cancel
            </button>
          </p>
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
                <span className="quiet">{s.root}</span>
                <span className="badge">{all(s.items)}</span>
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
                  <button className="link" onClick={() => setRemoving(s.id)} aria-label={`Remove ${s.name}`}>
                    Remove
                  </button>
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

/** Everything that can be changed or looked up, in one place. */
function SettingsScreen(props: {
  api: Backend
  state: AppState
  backedUpTo: string | null
  update: Update | null
  onUpdate: (update: Update | null) => void
  onLibrary: (state: AppState) => void
  onBackup: () => void
  onBack: () => void
  onProblem: (error: unknown) => void
}) {
  const { api, state, onProblem } = props
  const [log, setLog] = useState<string | null>(null)
  const [checked, setChecked] = useState<'no' | 'checking' | 'current'>('no')
  const [theme, setTheme] = useState<Theme>(savedTheme)
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
          This is <b>{state.session?.name}</b>. A backup remembers its own sources, what you chose to keep and where
          it sends things. To open another or start a new one, use the backup’s name at the top of the side bar.
        </p>
        <p className="quiet small">
          Its notes and previews are kept in <span className="where">{state.library}</span>. Your photos themselves
          are not copied there.
        </p>
      </div>

      <div className="setting">
        <h3>Where it sends to</h3>
        <p>{props.backedUpTo ? `This backup sends to ${props.backedUpTo}.` : 'Nowhere has been chosen yet.'}</p>
        <p className="choices">
          <button onClick={props.onBackup}>{props.backedUpTo ? 'Go to sending' : 'Choose where'}</button>
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
          <button className="link" onClick={() => api.openLog().catch(onProblem)}>
            Open the log file
          </button>
        </p>
        {log !== null && (
          <pre className="log" ref={logBox} tabIndex={0}>
            {log || 'Nothing has been recorded yet.'}
          </pre>
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

/** Step 3. One question: where should they go? Then one button: send. */
function BackupScreen(props: {
  api: Backend
  waiting: number
  safe: number
  onProblem: (error: unknown) => void
  onBackedUpTo: (where: string | null) => void
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
      <BackupPanel
        api={props.api}
        waiting={props.waiting}
        safe={props.safe}
        onProblem={props.onProblem}
        onWhere={props.onBackedUpTo}
        onSent={props.onSent}
      />
      {props.next}
    </section>
  )
}

/**
 * Where this session sends to. Until a destination is chosen this offers the
 * kinds there are, a folder first; after that it is one button, Send.
 */
function BackupPanel(props: {
  api: Backend
  onProblem: (error: unknown) => void
  waiting: number
  safe: number
  /** Tells the rest of the app where this session sends to. */
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
  const [said, setSaid] = useState<BackupEvent | null>(null)

  useEffect(() => {
    api.backupState().then(setWhere).catch(onProblem)
    return api.onBackup((event) => {
      if (event.finished) {
        setRunning(null)
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
          {!form && (
            <div className="places destinations">
              <button
                className="place likely"
                onClick={() =>
                  api
                    .chooseBackupFolder()
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
          <h2>Sending to {where}</h2>
          <p>
            {props.waiting > 0
              ? `${props.waiting.toLocaleString('en-GB')} ${props.waiting === 1 ? 'file is' : 'files are'} waiting to be sent.${
                  props.safe > 0 ? ` ${props.safe.toLocaleString('en-GB')} already there.` : ''
                }`
              : props.safe > 0
                ? `Everything you are keeping is there: ${props.safe.toLocaleString('en-GB')} files.`
                : 'Nothing is being kept yet.'}
          </p>
          {running ? (
            <p className="running">
              <Spinner /> {running}
            </p>
          ) : (
            <div className="choices">
              <button className="primary" onClick={() => start('Starting…', api.backUp)} disabled={props.waiting + props.safe === 0}>
                {props.waiting > 0 ? 'Send them now' : 'Send again'}
              </button>
              <button className="link" onClick={() => start('Checking…', api.checkBackup)} disabled={props.safe === 0}>
                Check they all arrived
              </button>
              {where.startsWith('the folder') && (
                <button className="link" onClick={() => api.openDestination().catch(onProblem)}>
                  Show me the folder
                </button>
              )}
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
                Send somewhere else
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

/**
 * The logo, exactly as supplied by the design system: the full lockup in
 * the side bar (with its own version for dark grounds), the picture tiles
 * on their own where it is shown large. It is never redrawn or recoloured.
 */
function Logo(props: { large?: boolean }) {
  return props.large ? (
    <img className="mark" src={iconUrl} alt="" />
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
