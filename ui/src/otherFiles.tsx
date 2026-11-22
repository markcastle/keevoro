// Other files a person asked Keevoro to carry along: music, documents,
// project files. They are never shown as pictures or grouped by date. They
// are listed by the folder they came from, as the person arranged them,
// each folder kept or left out as a whole with the same button as a
// collection, and each opening to a plain list (name, kind, size, when it
// last changed) with the same ticks photos have.
//
// Nothing here opens a file: a file's closer look is its details and Show
// in File Explorer, never a picture.

import { useEffect, useState } from 'react'
import type { ReactNode } from 'react'
import type { Backend, Collection, Photo, PhotoChoice } from './backend'
import { count, size, thousands } from './format'
import { InfoTip } from './infoTip'
import { FILE_MANAGER } from './mapWords'

/** "PDF file", "FLAC file", or "File" for a name with no ending. */
export function kindOfFile(name: string) {
  const dot = name.lastIndexOf('.')
  return dot > 0 && dot < name.length - 1 ? `${name.slice(dot + 1).toUpperCase()} file` : 'File'
}

/** The folder a file is in, from where it is on its source. */
const folderOf = (path: string) => path.replace(/[\\/][^\\/]*$/, '')

/** A day as a person reads it: "3 March 2019", from "2019-03-03". */
function day(date: string) {
  const [y, m, d] = date.split(/[-T]/).map(Number)
  if (!y) return date
  const when = new Date(Date.UTC(y, (m || 1) - 1, d || 1))
  return when.toLocaleDateString('en-GB', {
    timeZone: 'UTC',
    year: 'numeric',
    ...(m ? { month: 'long' } : {}),
    ...(d ? { day: 'numeric' } : {}),
  })
}

/**
 * A plain tile for an other file, where a picture would be: its name and
 * its kind. Never a broken picture.
 */
export function FileTile(props: { photo: Photo }) {
  return (
    <div className="picture file-tile" title={props.photo.name}>
      <span className="file-mark" aria-hidden />
      <span className="file-name">{props.photo.name}</span>
      <span className="file-kind">{kindOfFile(props.photo.name)}</span>
    </div>
  )
}

/**
 * The folders of other files on "Choose what to keep", under the
 * collections: one row each, with how many files and how much room, kept
 * or left out as a whole, and opening to its files.
 */
export function OtherFilesSection(props: {
  folders: Collection[]
  onOpen: (folder: Collection) => void
  keep: (folder: Collection) => ReactNode
}) {
  if (props.folders.length === 0) return null
  const files = props.folders.reduce((n, f) => n + (f.others ?? 0), 0)
  const bytes = props.folders.reduce((n, f) => n + (f.bytes ?? 0), 0)
  return (
    <section className="other-files" aria-labelledby="other-files-heading">
      <h2 id="other-files-heading">Other files</h2>
      <p className="quiet">
        {count(files, 'file')} in {count(props.folders.length, 'folder')} · {size(bytes)}. Open a folder to choose file
        by file.
        <InfoTip about="other files">
          Kept as you arranged them. Keevoro backs these up exactly, but cannot show them or group them by date.
        </InfoTip>
      </p>
      <ul className="folder-rows">
        {props.folders.map((folder) => (
          <li key={folder.key} className={folder.kept ? undefined : 'skipped'}>
            <button className="open-folder" onClick={() => props.onOpen(folder)} aria-label={`Open ${folder.title}`}>
              <span className="folder-mark" aria-hidden />
              <span className="folder-words">
                <b>{folder.title}</b>
                {folder.parents.length > 0 && <span className="parents quiet small">{folder.parents.join(' › ')}</span>}
                <span className="quiet small">
                  {folder.summary} · {size(folder.bytes ?? 0)}
                </span>
              </span>
            </button>
            {props.keep(folder)}
          </li>
        ))}
      </ul>
    </section>
  )
}

/**
 * One folder of other files, opened: a plain list with a tick for each, and
 * the details of one with Show in File Explorer.
 */
export function OtherFilesScreen(props: {
  api: Backend
  folder: Collection
  onBack: () => void
  onProblem: (error: unknown) => void
}) {
  const { api, folder, onProblem } = props
  const [files, setFiles] = useState<PhotoChoice[] | null>(null)
  const [looking, setLooking] = useState<string | null>(null)
  useEffect(() => {
    let current = true
    api
      .collectionPhotos(folder.key)
      .then((list) => current && setFiles(list))
      .catch(onProblem)
    return () => {
      current = false
    }
  }, [api, folder.key, onProblem])

  const toggle = (file: PhotoChoice) => {
    const kept = !file.kept
    setFiles((list) => list && list.map((f) => (f.id === file.id ? { ...f, kept } : f)))
    api.keepPhoto(file.id, kept).catch(onProblem)
  }
  const keepAll = (kept: boolean) => {
    setFiles((list) => list && list.map((f) => ({ ...f, kept })))
    api.keepCollection(folder.key, kept).catch(onProblem)
  }
  const keeping = files?.filter((f) => f.kept).length ?? 0
  const where = files && files.length > 0 ? folderOf(files[0].sourcePath) : null
  const chosen = files?.find((f) => f.id === looking) ?? null
  const show = (path: string) => api.openPlace(path).catch(onProblem)

  return (
    <section className="photos other-files-open">
      <header>
        <button className="link back" onClick={props.onBack}>
          ‹ Back
        </button>
        <h1>{folder.title}</h1>
        <p className="status">
          {files
            ? `Keeping ${thousands(keeping)} of ${thousands(files.length)}. Click one for its details.`
            : folder.summary}
          {files && <InfoTip about="other files">Other files are backed up exactly as they are.</InfoTip>}
        </p>
        {files && files.length > 0 && (
          <p className="choices within">
            <button className="button small" onClick={() => keepAll(true)} disabled={keeping === files.length}>
              Keep all
            </button>
            <button className="button small" onClick={() => keepAll(false)} disabled={keeping === 0}>
              Keep none
            </button>
            {where && (
              <button className="button small" onClick={() => show(where)}>
                Show in {FILE_MANAGER}
              </button>
            )}
          </p>
        )}
      </header>
      {files && files.length === 0 && <div className="empty quiet">Nothing here.</div>}
      {files && files.length > 0 && (
        <div className="file-table">
          <table aria-label={`Files in ${folder.title}`}>
            <thead>
              <tr>
                <th scope="col">
                  <span className="visually-hidden">Keep</span>
                </th>
                <th scope="col">Name</th>
                <th scope="col" className="number">
                  Size
                </th>
                <th scope="col">Changed</th>
              </tr>
            </thead>
            <tbody>
              {files.map((file) => (
                <tr key={file.id} className={file.kept ? undefined : 'out'} aria-selected={file.id === looking}>
                  <td>
                    <button
                      className="tick"
                      role="switch"
                      aria-checked={file.kept}
                      aria-label={`Keep ${file.name}`}
                      title={file.kept ? 'Keeping. Click to leave it out.' : 'Left out. Click to keep it.'}
                      onClick={() => toggle(file)}
                    >
                      {file.kept ? '✓' : ''}
                    </button>
                  </td>
                  <td className="name">
                    <button className="link" onClick={() => setLooking(file.id === looking ? null : file.id)}>
                      {file.name}
                    </button>
                    <span className="quiet small"> {kindOfFile(file.name)}</span>
                  </td>
                  <td className="number">{size(file.bytes ?? 0)}</td>
                  <td>{file.changed ? day(file.changed) : ''}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
      {chosen && (
        <aside className="file-details" aria-label={`About ${chosen.name}`}>
          <h2>{chosen.name}</h2>
          <dl>
            <dt>Kind</dt>
            <dd>{kindOfFile(chosen.name)}</dd>
            <dt>Size</dt>
            <dd>{size(chosen.bytes ?? 0)}</dd>
            {chosen.changed && (
              <>
                <dt>Last changed</dt>
                <dd>{day(chosen.changed)}</dd>
              </>
            )}
            <dt>Where it is</dt>
            <dd>{chosen.sourcePath}</dd>
            <dt>Backed up</dt>
            <dd>{chosen.kept ? 'Kept: it is backed up exactly as it is' : 'Left out of this backup'}</dd>
          </dl>
          <p className="choices">
            <button className="button small" onClick={() => show(folderOf(chosen.sourcePath))}>
              Show in {FILE_MANAGER}
            </button>
            <button className="button small" onClick={() => toggle(chosen)}>
              {chosen.kept ? 'Leave it out' : 'Keep it'}
            </button>
            <button className="link" onClick={() => setLooking(null)}>
              Close
            </button>
          </p>
        </aside>
      )}
    </section>
  )
}
