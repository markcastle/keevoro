// The app's screens.
//
// There are only three, and each asks at most one thing of the person
// using it:
//
//   Welcome   "Where should your library live?"   (first run only)
//   Discover  "Which of these should we keep?"
//   Archive   "Gather my photos"
//
// Nothing here decides anything about photos. Every fact on screen comes
// from the engine through backend.ts, and every button sends one request
// back. Wording follows the house rules: plain English, no jargon, and
// progress said in human terms ("3,412 of 9,870 safely copied").

import { useCallback, useEffect, useState } from 'react'

import { backend } from './backend'
import type { AppState, Backend, Collection, Discover, GatherEvent, Photo, PhotoChoice, ScanEvent } from './backend'
import { count, photosAndVideos, size, thousands } from './format'

/** Where the user is. Opening a collection or the set-aside list sits on top of Discover. */
type Screen = 'discover' | 'archive' | { collection: Collection } | 'setAside'

export function App() {
  const [api, setApi] = useState<Backend | null>(null)
  const [state, setState] = useState<AppState | null>(null)
  const [found, setFound] = useState<Discover | null>(null)
  const [screen, setScreen] = useState<Screen>('discover')
  const [scan, setScan] = useState<ScanEvent | null>(null)
  const [gather, setGather] = useState<GatherEvent | null>(null)
  const [problem, setProblem] = useState<string | null>(null)

  // Anything that goes wrong is said once, in the engine's own plain words.
  const report = useCallback((error: unknown) => {
    setProblem(error instanceof Error ? error.message : String(error))
  }, [])

  const refresh = useCallback(
    (from: Backend) =>
      from
        .discover()
        .then(setFound)
        .catch(report),
    [report],
  )

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
        // What has been found so far is worth showing as it arrives.
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
        if (s.library) await refresh(b)
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
    return (
      <Welcome
        suggested={state.suggestedLibrary}
        onSuggested={() => api.useLibrary(state.suggestedLibrary).then(setState).then(() => refresh(api)).catch(report)}
        onChoose={() =>
          api
            .chooseLibrary()
            .then((s) => {
              if (s) {
                setState(s)
                return refresh(api)
              }
            })
            .catch(report)
        }
        problem={problem}
      />
    )
  }

  const addPlace = () => {
    setScreen('discover')
    api.addPlace().catch(report)
  }
  const keep = (collection: Collection, kept: boolean) => {
    // Show the choice at once; the engine's answer follows.
    setFound((d) => d && { ...d, collections: d.collections.map((c) => (c.key === collection.key ? { ...c, kept } : c)) })
    api
      .keepCollection(collection.key, kept)
      .then(() => refresh(api))
      .catch(report)
  }
  const gathering = gather !== null && gather.step !== 'finished'

  return (
    <div className="app">
      <aside className="sidebar">
        <div className="brand">
          <Logo />
          <span>Keevoro</span>
        </div>
        <nav>
          <button className={screen !== 'archive' ? 'nav on' : 'nav'} onClick={() => setScreen('discover')}>
            <SearchIcon /> Discover
          </button>
          <button className={screen === 'archive' ? 'nav on' : 'nav'} onClick={() => setScreen('archive')}>
            <ArchiveIcon /> Archive
            {found && found.waitingToGather > 0 && !gathering && <span className="dot" aria-hidden />}
          </button>
        </nav>
        <div className="sources">
          <h2>Sources</h2>
          {found?.sources.map((s) => (
            <div className="source" key={s.id} title={s.root}>
              <DriveIcon /> <span>{s.name}</span>
            </div>
          ))}
          {scan && (
            <div className="source busy">
              <DriveIcon /> <span>{scan.name}</span> <Spinner />
            </div>
          )}
          <button className="add" onClick={addPlace} disabled={scan !== null}>
            + Add a source
          </button>
        </div>
        <div className="foot">Your sources are only ever read.</div>
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
        {screen === 'discover' ? (
          <DiscoverScreen
            api={api}
            found={found}
            scan={scan}
            onAdd={addPlace}
            onKeep={keep}
            onOpen={(collection) => setScreen({ collection })}
            onSetAside={() => setScreen('setAside')}
          />
        ) : screen === 'setAside' || typeof screen === 'object' ? (
          <PhotosScreen
            api={api}
            collection={typeof screen === 'object' ? screen.collection : null}
            onBack={() => {
              setScreen('discover')
              void refresh(api)
            }}
            onProblem={report}
          />
        ) : (
          <ArchiveScreen
            found={found}
            library={state.library}
            gather={gather}
            gathering={gathering}
            onGather={() => {
              setGather({ step: 'copying', done: 0, total: found?.waitingToGather ?? 0 })
              api.gather().catch((error) => {
                setGather(null)
                report(error)
              })
            }}
          />
        )}
      </main>
    </div>
  )
}

function Welcome(props: { suggested: string; onSuggested: () => void; onChoose: () => void; problem: string | null }) {
  return (
    <div className="welcome">
      <Logo large />
      <h1>Let’s find your photos</h1>
      <p>
        Keevoro looks through your sources (old drives, USB sticks, folders), finds your photos and videos, and gathers the ones you want into one tidy
        library. It never changes anything on a source.
      </p>
      <h2>Where should your library live?</h2>
      <p className="quiet">Pick somewhere with plenty of room. You can back it up later.</p>
      <button className="primary big" onClick={props.onSuggested}>
        Use my Pictures folder
      </button>
      <div className="path">{props.suggested}</div>
      <button className="link" onClick={props.onChoose}>
        Choose somewhere else…
      </button>
      {props.problem && (
        <div className="problem inline" role="alert">
          {props.problem}
        </div>
      )}
    </div>
  )
}

function DiscoverScreen(props: {
  api: Backend
  found: Discover | null
  scan: ScanEvent | null
  onAdd: () => void
  onKeep: (collection: Collection, kept: boolean) => void
  onOpen: (collection: Collection) => void
  onSetAside: () => void
}) {
  const { found, scan } = props
  const [hero, ...rest] = found?.collections ?? []
  const status = scan
    ? `Searching ${scan.name}… ${photosAndVideos(scan.photos, scan.videos)} found so far`
    : found && hero
      ? `${photosAndVideos(found.photos, found.videos)} found`
      : null

  return (
    <section className="discover">
      <header>
        <h1>Discover</h1>
        {status && (
          <p className="status">
            {scan && <Spinner />} {status}
          </p>
        )}
      </header>

      {!hero && !scan && (
        <div className="empty">
          <DriveIcon large />
          <h2>Add your first source</h2>
          <p>A source is anywhere your photos might be: a hard drive, a USB stick, an old laptop’s disk, or just a folder. Plug it in, then show Keevoro where it is.</p>
          <button className="primary big" onClick={props.onAdd}>
            Add a source
          </button>
        </div>
      )}
      {!hero && scan && <div className="empty quiet">Your photos will appear here as they are found.</div>}

      {hero && (
        <article className="hero">
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
        <>
          <h3>More discoveries</h3>
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
        </>
      )}

      {found && hero && (found.duplicates > 0 || found.junk > 0) && (
        <p className="tidied">
          {found.duplicates > 0 && <span>{count(found.duplicates, 'duplicate')} will be kept once</span>}
          {found.duplicates > 0 && found.junk > 0 && ' · '}
          {found.junk > 0 && (
            <button className="link" onClick={props.onSetAside}>
              {count(found.junk, 'picture')} set aside as probably not memories
            </button>
          )}
        </p>
      )}
    </section>
  )
}

/** Everything is kept unless you say otherwise, so the button shows which. */
function KeepButton(props: { collection: Collection; onKeep: (c: Collection, kept: boolean) => void; small?: boolean }) {
  const { collection, small } = props
  if (collection.kept) {
    return (
      <div className={small ? 'keep small' : 'keep'}>
        <span className="keeping">✓ Keeping</span>
        <button className="link" onClick={() => props.onKeep(collection, false)}>
          Don’t keep
        </button>
      </div>
    )
  }
  return (
    <div className={small ? 'keep small' : 'keep'}>
      <button className={small ? 'primary' : 'primary big'} onClick={() => props.onKeep(collection, true)}>
        Keep collection
      </button>
    </div>
  )
}

function ArchiveScreen(props: {
  found: Discover | null
  library: string
  gather: GatherEvent | null
  gathering: boolean
  onGather: () => void
}) {
  const { found, gather, gathering } = props
  const waiting = found?.waitingToGather ?? 0
  const keptCollections = found?.collections.filter((c) => c.kept) ?? []
  const safe = keptCollections.reduce((n, c) => n + c.gathered, 0)
  const steps = { copying: 'safely copied', previews: 'previews made', videos: 'videos done', finished: '' }

  return (
    <section className="archive">
      <header>
        <h1>Archive</h1>
        <p className="status">Your library lives in {props.library}</p>
      </header>

      <div className="panel">
        {gathering && gather ? (
          <>
            <h2>Gathering your photos…</h2>
            <progress value={gather.done} max={Math.max(gather.total, 1)} />
            <p>
              {thousands(gather.done)} of {thousands(gather.total)} {steps[gather.step]}
            </p>
            <p className="quiet">You can keep using your computer. If anything stops this, it carries on next time.</p>
          </>
        ) : waiting > 0 ? (
          <>
            <h2>{count(waiting, 'photo and video').replace('photo and videos', 'photos and videos')} ready to gather</h2>
            <p>
              They will be copied into your library and every copy checked against the original. Nothing on your
              sources is changed.
            </p>
            <button className="primary big" onClick={props.onGather}>
              Gather my photos
            </button>
          </>
        ) : safe > 0 ? (
          <>
            <h2>Everything you’re keeping is safe</h2>
            <p>
              {count(safe, 'photo and video').replace('photo and videos', 'photos and videos')} in{' '}
              {count(keptCollections.length, 'collection')} are in your library, each one checked against its original.
            </p>
          </>
        ) : (
          <>
            <h2>Nothing to gather yet</h2>
            <p>Add a source on the Discover screen and choose what to keep. It will appear here.</p>
          </>
        )}
        {!gathering && gather?.summary?.map((line) => (
          <p className="done" key={line}>
            {line}
          </p>
        ))}
      </div>

      {found && keptCollections.length > 0 && (
        <>
          <h3>What you’re keeping</h3>
          <ul className="kept">
            {keptCollections.map((c) => (
              <li key={c.key}>
                <span>{c.title}</span>
                <span className="quiet">{c.summary}</span>
                <span className={c.gathered >= c.photos + c.videos ? 'badge safe' : 'badge'}>
                  {c.gathered >= c.photos + c.videos ? 'In your library' : 'Waiting'}
                </span>
              </li>
            ))}
          </ul>
          <p className="tidied">About {size(found.bytes)} in all.</p>
        </>
      )}
    </section>
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
          ‹ Discover
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
const SearchIcon = () => icon('M11 4a7 7 0 1 0 0 14 7 7 0 0 0 0-14zM20 20l-4-4')
const ArchiveIcon = () => icon('M4 5h16v4H4zM6 9v10h12V9M10 13h4')
const DriveIcon = (props: { large?: boolean }) => icon('M4 14l2.5-8h11L20 14M4 14h16v5H4zM16.5 16.5h.01', props.large)
