// Looking inside a folder before it is added: a grid of its photos and a
// still of each video, folder by folder, so the choice is made by looking
// rather than by guessing from a name.
//
// It only reads. Nothing is added to the backup, and the pictures are made
// as they come on screen and kept in Keevoro's own cache, never beside the
// photos. A file only in online storage is never opened (that would
// download it): its tile says so instead.
//
// Any folder can be looked inside: one the survey suggested, a source, or
// one the person picks. When Keevoro has not surveyed it yet, it does that
// first (it only reads folder listings, so it is quick), with Stop.

import { useEffect, useRef, useState } from 'react'
import type { Backend, FilesShow, FilesSort, FileView, FoldersView, SurveyProgress } from './backend'
import { count, photosAndVideos, size, thousands } from './format'
import { fileKey } from './pictureQueue'
import { Picture, Viewer } from './pictures'
import { VirtualTiles } from './tiles'
import type { TileSection } from './tiles'

/** The online storage a person here would know by name. */
const SERVICE = typeof navigator !== 'undefined' && /Mac/.test(navigator.platform) ? 'iCloud' : 'OneDrive'
const ONLINE_ONLY = `In ${SERVICE} only, not on this computer`

/** The orders on offer, in the words a person reads. A file's date is not when it was taken. */
const SORTS: { value: FilesSort; words: string }[] = [
  { value: 'name', words: 'Name' },
  { value: 'newest', words: 'Newest first (by file date)' },
  { value: 'oldest', words: 'Oldest first (by file date)' },
]
const SHOWS: { value: FilesShow; words: string }[] = [
  { value: 'all', words: 'All' },
  { value: 'photos', words: 'Photos' },
  { value: 'videos', words: 'Videos' },
]

/** The last part of a path, as a person would name the folder. */
export const nameOf = (place: string) => place.replace(/[\\/]+$/, '').split(/[\\/]/).pop() || place

export function LookInside(props: {
  api: Backend
  /** The folder to look inside, written whole. */
  place: string
  /** What to call it, when it has a name of its own (a source's); its folder's name otherwise. */
  name?: string
  /** What the main button says: "Look through this folder", or for a source, "Look through it again". */
  lookThrough?: string
  onBack: () => void
  /** Looks through this folder properly, adding it as a source. */
  onLookThrough: (folder: string) => void
  onProblem: (error: unknown) => void
}) {
  const { api, place, onProblem } = props
  const lookThrough = props.lookThrough ?? 'Look through this folder'
  const [outline, setOutline] = useState<FoldersView | null>(null)
  // Finding what is in it first, when Keevoro has not surveyed it.
  const [surveying, setSurveying] = useState<{ progress: SurveyProgress | null; stopping: boolean } | null>(null)
  const [sort, setSort] = useState<FilesSort>('name')
  const [only, setOnly] = useState<FilesShow>('all')
  // What has been fetched, so the closer look can move to the next one.
  const fetched = useRef(new Map<string, Map<number, FileView>>())
  const [looking, setLooking] = useState<{ folder: string; at: number; file: FileView } | null>(null)

  useEffect(() => {
    let current = true
    const unhear = api.onSurvey((progress) => {
      if (current) setSurveying((s) => s && { ...s, progress })
    })
    api
      .foldersInside(place)
      .then(async (view) => {
        if (view || !current) return view
        setSurveying({ progress: null, stopping: false })
        await api.surveyPlace(place)
        return api.foldersInside(place)
      })
      .then((view) => {
        if (!current) return
        setSurveying(null)
        setOutline(view ?? { place, surveyedAt: 0, complete: false, folders: [], photos: 0, videos: 0, bytes: 0, onlineOnly: 0 })
      })
      .catch((error) => {
        if (!current) return
        setSurveying(null)
        onProblem(error)
      })
    return () => {
      current = false
      unhear()
    }
  }, [api, place, onProblem])

  const choose = (next: { sort?: FilesSort; only?: FilesShow }) => {
    // A new order or kind starts the grid afresh.
    fetched.current = new Map()
    setLooking(null)
    if (next.sort) setSort(next.sort)
    if (next.only) setOnly(next.only)
  }

  const name = props.name ?? nameOf(place)
  const shownOf = (f: { photos: number; videos: number }) =>
    only === 'photos' ? f.photos : only === 'videos' ? f.videos : f.photos + f.videos
  const sections: TileSection<FileView>[] = (outline?.folders ?? [])
    .filter((folder) => shownOf(folder) > 0)
    .map((folder) => ({
      key: folder.folder,
      count: shownOf(folder),
      heading: (
        <>
          <b>{folder.relative || name}</b>
          <span className="quiet small">{photosAndVideos(folder.photos, folder.videos)}</span>
        </>
      ),
      load: (from, count) =>
        api.folderFiles({ place: folder.folder, from, count, sort, only }).then((page) => {
          let mine = fetched.current.get(folder.folder)
          if (!mine) {
            mine = new Map()
            fetched.current.set(folder.folder, mine)
          }
          page.files.forEach((file, i) => mine.set(from + i, file))
          return { total: page.total, items: page.files }
        }),
    }))

  const move = (by: 1 | -1) =>
    setLooking((now) => {
      const next = now && fetched.current.get(now.folder)?.get(now.at + by)
      return now && next ? { ...now, at: now.at + by, file: next } : now
    })
  const close = () => setLooking(null)
  const current = looking?.file
  const inFolder = outline?.folders.find((f) => f.folder === looking?.folder)

  if (!outline) {
    return (
      <section className="inside">
        <header>
          <button className="link back" onClick={props.onBack}>
            ‹ Back
          </button>
          <h1>Inside {name}</h1>
        </header>
        {surveying && (
          <div className="panel searching">
            <span className="spinner" aria-hidden />
            <h2>Finding what is in {name}…</h2>
            <p className="big-count" role="status">
              {surveying.progress ? photosAndVideos(surveying.progress.photos, surveying.progress.videos) : 'Starting…'}
            </p>
            <p className="quiet">
              spotted so far, by name. This only reads the list of what is in each folder. No file is opened, and
              nothing is added or changed.
            </p>
            {surveying.progress && (
              <p className="quiet small doing" aria-live="off">
                Looked at {count(surveying.progress.files, 'file')} in {count(surveying.progress.folders, 'folder')} so
                far.
              </p>
            )}
            <p className="choices stop">
              <button
                disabled={surveying.stopping}
                onClick={() => {
                  setSurveying((s) => s && { ...s, stopping: true })
                  api.stopSurvey().catch(onProblem)
                }}
              >
                {surveying.stopping ? 'Stopping…' : 'Stop and show what was found'}
              </button>
            </p>
          </div>
        )}
      </section>
    )
  }

  return (
    <section className="inside">
      <header>
        <button className="link back" onClick={props.onBack}>
          ‹ Back
        </button>
        <h1>Inside {name}</h1>
        <p className="status">
          {`${photosAndVideos(outline.photos, outline.videos)}, ${size(outline.bytes)}. Nothing here is added or changed until you choose.`}
        </p>
        {outline.onlineOnly > 0 && (
          <p className="online-only small">
            {thousands(outline.onlineOnly)} {outline.onlineOnly === 1 ? 'file is' : 'files are'} in {SERVICE} only, not
            on this computer, so Keevoro does not open {outline.onlineOnly === 1 ? 'it' : 'them'}. To include{' '}
            {outline.onlineOnly === 1 ? 'it' : 'them'}, make {outline.onlineOnly === 1 ? 'it' : 'them'} available
            offline in {SERVICE} first.
          </p>
        )}
        {!outline.complete && (
          <p className="quiet small">The search for folders was stopped part-way, so there may be more here.</p>
        )}
        <p className="choices within">
          <button className="primary" onClick={() => props.onLookThrough(place)}>
            {lookThrough}
          </button>
        </p>
        {outline.folders.length > 0 && (
          <div className="arrange">
            <div className="segmented" role="radiogroup" aria-label="Show">
              {SHOWS.map((show) => (
                <button
                  key={show.value}
                  role="radio"
                  aria-checked={only === show.value}
                  onClick={() => choose({ only: show.value })}
                >
                  {show.words}
                </button>
              ))}
            </div>
            <label className="sort">
              <span className="quiet small">Sort by</span>
              <select value={sort} onChange={(e) => choose({ sort: e.target.value as FilesSort })}>
                {SORTS.map((s) => (
                  <option key={s.value} value={s.value}>
                    {s.words}
                  </option>
                ))}
              </select>
            </label>
          </div>
        )}
        {sort !== 'name' && (
          <p className="quiet small">
            A file’s date is when it was last changed or copied, which is not always when it was taken. Each folder
            stays together.
          </p>
        )}
      </header>
      {outline.folders.length === 0 && <div className="empty quiet">No photos or videos were found here.</div>}
      {outline.folders.length > 0 && sections.length === 0 && <div className="empty quiet">There are no {only} here.</div>}
      <VirtualTiles
        key={`${sort} ${only}`}
        label={`Photos and videos inside ${name}`}
        sections={sections}
        tile={(file, at, section) => (
          <div className="tile">
            {file ? (
              <button
                className="look"
                onClick={() => setLooking({ folder: section.key, at, file })}
                aria-label={`Look at ${file.name}`}
              >
                <Picture
                  id={file.path}
                  name={file.name}
                  isVideo={file.isVideo}
                  seconds={file.seconds}
                  load={() => api.filePicture(file.path)}
                  drop={() => api.notNeeded(fileKey(file.path))}
                  missing={file.onlineOnly ? ONLINE_ONLY : undefined}
                  cannot="Can’t be shown"
                />
              </button>
            ) : (
              <Picture id={`${section.key} ${at}`} name="" isVideo={false} />
            )}
          </div>
        )}
      />
      {current && looking && (
        <Viewer
          item={{
            key: current.path,
            name: current.name,
            isVideo: current.isVideo,
            where: current.path,
            missing: current.onlineOnly ? ONLINE_ONLY : undefined,
            seconds: current.seconds,
          }}
          small={() => api.filePicture(current.path)}
          large={() => api.closerLookAtFile(current.path)}
          place={`${thousands(looking.at + 1)} of ${thousands(inFolder ? shownOf(inFolder) : 0)}`}
          actions={
            <button className="button" onClick={() => props.onLookThrough(place)}>
              {lookThrough}
            </button>
          }
          onMove={move}
          onClose={close}
        />
      )}
    </section>
  )
}
