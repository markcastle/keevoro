// Looking inside a folder before it is added: a grid of its photos and a
// still of each video, folder by folder, so the choice is made by looking
// rather than by guessing from a name.
//
// It only reads. Nothing is added to the backup, and the pictures are made
// as they come on screen and kept in Keevoro's own cache, never beside the
// photos. A file only in online storage is never opened (that would
// download it): its tile says so instead.

import { useEffect, useRef, useState } from 'react'
import type { Backend, FileView, FoldersView } from './backend'
import { photosAndVideos, size, thousands } from './format'
import { fileKey } from './pictureQueue'
import { Picture, Viewer } from './pictures'
import { VirtualTiles } from './tiles'
import type { TileSection } from './tiles'

/** The online storage a person here would know by name. */
const SERVICE = typeof navigator !== 'undefined' && /Mac/.test(navigator.platform) ? 'iCloud' : 'OneDrive'
const ONLINE_ONLY = `In ${SERVICE} only, not on this computer`

export function LookInside(props: {
  api: Backend
  /** The folder to look inside, as the survey gave it. */
  place: string
  onBack: () => void
  /** Looks through this folder properly, adding it as a source. */
  onLookThrough: (folder: string) => void
  onProblem: (error: unknown) => void
}) {
  const { api, place, onProblem } = props
  const [outline, setOutline] = useState<FoldersView | null>(null)
  // What has been fetched, so the closer look can move to the next one.
  const fetched = useRef(new Map<string, Map<number, FileView>>())
  const [looking, setLooking] = useState<{ folder: string; at: number; file: FileView } | null>(null)

  useEffect(() => {
    let current = true
    api
      .foldersInside(place)
      .then((view) => current && setOutline(view))
      .catch(onProblem)
    return () => {
      current = false
    }
  }, [api, place, onProblem])

  const name = place.replace(/[\\/]+$/, '').split(/[\\/]/).pop() || place
  const sections: TileSection<FileView>[] = (outline?.folders ?? []).map((folder) => ({
    key: folder.folder,
    count: folder.photos + folder.videos,
    heading: (
      <>
        <b>{folder.relative || name}</b>
        <span className="quiet small">{photosAndVideos(folder.photos, folder.videos)}</span>
      </>
    ),
    load: (from, count) =>
      api.folderFiles(folder.folder, from, count).then((page) => {
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

  return (
    <section className="inside">
      <header>
        <button className="link back" onClick={props.onBack}>
          ‹ Back
        </button>
        <h1>Inside {name}</h1>
        <p className="status">
          {outline
            ? `${photosAndVideos(outline.photos, outline.videos)}, ${size(outline.bytes)}. Nothing here is added or changed until you choose.`
            : 'Nothing here is added or changed until you choose.'}
        </p>
        {outline && outline.onlineOnly > 0 && (
          <p className="online-only small">
            {thousands(outline.onlineOnly)} {outline.onlineOnly === 1 ? 'file is' : 'files are'} in {SERVICE} only, not
            on this computer, so Keevoro does not open {outline.onlineOnly === 1 ? 'it' : 'them'}. To include{' '}
            {outline.onlineOnly === 1 ? 'it' : 'them'}, make {outline.onlineOnly === 1 ? 'it' : 'them'} available
            offline in {SERVICE} first.
          </p>
        )}
        {outline && !outline.complete && (
          <p className="quiet small">The search for folders was stopped part-way, so there may be more here.</p>
        )}
        <p className="choices within">
          <button className="primary" onClick={() => props.onLookThrough(place)}>
            Look through this folder
          </button>
        </p>
      </header>
      {outline && outline.folders.length === 0 && <div className="empty quiet">No photos or videos were found here.</div>}
      {outline && (
        <VirtualTiles
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
      )}
      {current && looking && (
        <Viewer
          item={{
            key: current.path,
            name: current.name,
            isVideo: current.isVideo,
            where: current.path,
            missing: current.onlineOnly ? ONLINE_ONLY : undefined,
          }}
          small={() => api.filePicture(current.path)}
          large={() => api.closerLookAtFile(current.path)}
          place={`${thousands(looking.at + 1)} of ${thousands((inFolder?.photos ?? 0) + (inFolder?.videos ?? 0))}`}
          actions={
            <button className="button" onClick={() => props.onLookThrough(place)}>
              Look through this folder
            </button>
          }
          onMove={move}
          onClose={close}
        />
      )}
    </section>
  )
}
