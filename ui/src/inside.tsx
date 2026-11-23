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
//
// It can be left out of the backup from here, and each folder in it;
// whatever would not be backed up is marked, and the closer look says why.

import { useCallback, useRef, useState } from 'react'
import type { Backend, FilesShow, FilesSort, FileView, LeftOutFolder } from './backend'
import { files, musicOnly, size, thousands } from './format'
import { useHolds } from './holds'
import { InfoTip } from './infoTip'
import { LeaveOutButton, LeftOutSaid } from './leaveOut'
import { fileKey } from './pictureQueue'
import { Picture, Viewer } from './pictures'
import { FileTile } from './otherFiles'
import { FILE_MANAGER } from './mapWords'
import { SurveyingPanel, useAfterSurvey } from './surveyFirst'
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
  { value: 'music', words: 'Music' },
  { value: 'others', words: 'Other files' },
]

/** What the closer look says of an other file, which is never opened to be shown. */
const NOT_SHOWN = 'An other file: Keevoro backs it up exactly, but does not show it'
const MUSIC_NOT_SHOWN = 'Music: Keevoro backs it up exactly, but never opens it'

/** The folder a file is in. */
const folderOf = (path: string) => path.replace(/[\\/][^\\/]*$/, '')

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
  /** A folder was left out or put back, so what the backup holds has changed. */
  onChanged?: () => void
  /** Looks through this folder properly, adding it as a source. */
  onLookThrough: (folder: string) => void
  onProblem: (error: unknown) => void
}) {
  const holds = useHolds()
  const { api, place, onProblem } = props
  const lookThrough = props.lookThrough ?? 'Look through this folder'
  const [sort, setSort] = useState<FilesSort>('name')
  const [only, setOnly] = useState<FilesShow>('all')
  // What has been fetched, so the closer look can move to the next one.
  const fetched = useRef(new Map<string, Map<number, FileView>>())
  const [looking, setLooking] = useState<{ folder: string; at: number; file: FileView } | null>(null)
  // Asked again after a folder is left out or put back, so every mark follows.
  const [version, setVersion] = useState(0)
  const [said, setSaid] = useState<LeftOutFolder | null>(null)
  const changed = (done: LeftOutFolder) => {
    setSaid(done)
    fetched.current = new Map()
    setLooking(null)
    setVersion((v) => v + 1)
    props.onChanged?.()
  }

  // Finding what is in it first, when Keevoro has not surveyed it.
  const ask = useCallback(() => api.foldersInside(place), [api, place])
  const { answer, surveying, stop } = useAfterSurvey(api, place, ask, String(version), onProblem)
  const outline =
    answer === undefined
      ? null
      : (answer ?? { place, surveyedAt: 0, complete: false, folders: [], photos: 0, videos: 0, bytes: 0, onlineOnly: 0 })

  const choose = (next: { sort?: FilesSort; only?: FilesShow }) => {
    // A new order or kind starts the grid afresh.
    fetched.current = new Map()
    setLooking(null)
    if (next.sort) setSort(next.sort)
    if (next.only) setOnly(next.only)
  }

  const name = props.name ?? nameOf(place)
  const shownOf = (f: { photos: number; videos: number; others?: number; music?: number }) =>
    only === 'photos'
      ? f.photos
      : only === 'videos'
        ? f.videos
        : only === 'music'
          ? (f.music ?? 0)
          : only === 'others'
            ? (f.others ?? 0)
            : f.photos + f.videos + (f.others ?? 0) + (f.music ?? 0)
  // "Music" and "Other files" are only offered where there are some, and
  // in a folder of music alone, neither are photos and videos.
  const musicHere = (outline?.music ?? 0) > 0
  const picturesHere = (outline?.photos ?? 0) + (outline?.videos ?? 0) > 0 || !musicHere
  const shows = SHOWS.filter(
    (show) =>
      (show.value !== 'others' || (outline?.others ?? 0) > 0) &&
      (show.value !== 'music' || musicHere) &&
      ((show.value !== 'photos' && show.value !== 'videos') || picturesHere),
  )
  const sections: TileSection<FileView>[] = (outline?.folders ?? [])
    .filter((folder) => shownOf(folder) > 0)
    .map((folder) => ({
      key: folder.folder,
      count: shownOf(folder),
      heading: (
        <>
          <b>{folder.relative || name}</b>
          <span className="quiet small">{files(folder.photos, folder.videos, folder.others ?? 0, folder.music ?? 0)}</span>
          {folder.leftOut && <span className="badge">Left out</span>}
          {!outline?.leftOut && (
            <LeaveOutButton
              api={api}
              folder={folder.folder}
              leftOut={Boolean(folder.leftOut)}
              words="Leave out"
              small
              onChanged={changed}
              onProblem={onProblem}
            />
          )}
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
        {surveying && <SurveyingPanel name={name} surveying={surveying} onStop={stop} />}
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
          {`${files(outline.photos, outline.videos, outline.others ?? 0, outline.music ?? 0, holds)} · ${size(outline.bytes)}. Nothing is added or changed until you choose.`}
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
        {outline.leftOut ? (
          <div className="left-out-here">
            <p>
              You left <span className="where">{outline.leftOut}</span> out of this backup, so nothing here is looked
              through or backed up. Nothing already backed up was touched.
            </p>
            <LeaveOutButton api={api} folder={outline.leftOut} leftOut onChanged={changed} onProblem={onProblem} />
          </div>
        ) : (
          <p className="choices within">
            <button className="primary" onClick={() => props.onLookThrough(place)}>
              {lookThrough}
            </button>
            <LeaveOutButton api={api} folder={place} leftOut={false} onChanged={changed} onProblem={onProblem} />
          </p>
        )}
        <LeftOutSaid done={said} />
        {outline.folders.length > 0 && (
          <div className="arrange">
            <div className="segmented" role="radiogroup" aria-label="Show">
              {shows.map((show) => (
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
            By when each file was last changed or copied.
            <InfoTip about="the dates shown">
              A file’s date is when it was last changed or copied, which is not always when it was taken. Each folder
              stays together.
            </InfoTip>
          </p>
        )}
      </header>
      {outline.folders.length === 0 && (
        <div className="empty quiet">
          {musicOnly(holds) ? 'No music was found here.' : 'No photos or videos were found here.'}
        </div>
      )}
      {outline.folders.length > 0 && sections.length === 0 && <div className="empty quiet">There are no {only} here.</div>}
      <VirtualTiles
        key={`${sort} ${only} ${version}`}
        label={musicOnly(holds) ? `Music inside ${name}` : `Photos and videos inside ${name}`}
        sections={sections}
        tile={(file, at, section) => (
          <div className="tile">
            {file?.isOther || file?.isMusic ? (
              <button
                className="look"
                onClick={() => setLooking({ folder: section.key, at, file })}
                aria-label={`About ${file.name}`}
              >
                <FileTile
                  photo={{
                    id: file.path,
                    isVideo: false,
                    isOther: !file.isMusic,
                    isMusic: file.isMusic,
                    name: file.name,
                    sourcePath: file.path,
                    thumbnail: null,
                  }}
                />
                {file.leftOut && (
                  <span className="out-mark" title={`Left out: ${file.leftOut}`}>
                    Left out
                  </span>
                )}
              </button>
            ) : file ? (
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
                  leftOut={file.leftOut}
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
            missing: current.onlineOnly
              ? ONLINE_ONLY
              : current.isMusic
                ? MUSIC_NOT_SHOWN
                : current.isOther
                  ? NOT_SHOWN
                  : undefined,
            seconds: current.seconds,
            leftOut: current.leftOut,
          }}
          small={() => api.filePicture(current.path)}
          large={() => api.closerLookAtFile(current.path)}
          place={`${thousands(looking.at + 1)} of ${thousands(inFolder ? shownOf(inFolder) : 0)}`}
          actions={
            <>
              {(current.isOther || current.isMusic) && (
                <button className="button" onClick={() => api.openPlace(folderOf(current.path)).catch(onProblem)}>
                  Show in {FILE_MANAGER}
                </button>
              )}
              {!outline.leftOut && (
                <button className="button" onClick={() => props.onLookThrough(place)}>
                  {lookThrough}
                </button>
              )}
            </>
          }
          onMove={move}
          onClose={close}
        />
      )}
    </section>
  )
}
