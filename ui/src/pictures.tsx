// A picture in a grid, and the closer look at one, for anything that can
// be pictured: a photo a search found, or a file in a folder being looked
// inside before it is added. Each is given how to fetch its picture, so
// neither knows which it is showing.

import { useEffect, useRef, useState } from 'react'
import type { ReactNode } from 'react'
import { createPortal } from 'react-dom'
import { NotNeeded, photoKey } from './pictureQueue'
import type { Backend, Photo } from './backend'

/**
 * A small picture, with a shimmer while it is on its way and words in its
 * place when it cannot be shown. `load` is asked once for each `id`;
 * `drop`, when given, is called if the tile goes before it arrives, so a
 * request not started yet is never made. `missing` says, up front, why no
 * picture will be fetched at all ("In OneDrive only, not on this
 * computer"). Without `load` and `missing` it simply waits, shimmering.
 */
export function Picture(props: {
  id: string
  name: string
  isVideo: boolean
  load?: () => Promise<string>
  drop?: () => void
  missing?: string
  /** What is said when the picture could not be made. "No preview" when not said. */
  cannot?: string
}) {
  const { id, load, drop } = props
  const [url, setUrl] = useState<string | null>(null)
  const [failed, setFailed] = useState(false)
  // Kept in a ref, so a new function each render does not fetch again.
  const latest = useRef({ load, drop })
  useEffect(() => {
    latest.current = { load, drop }
  })
  const asking = Boolean(load) && !props.missing

  useEffect(() => {
    if (!asking) return
    let current = true
    let arrived = false
    latest.current
      .load?.()
      .then((address) => {
        arrived = true
        if (current) setUrl(address)
      })
      .catch((error) => {
        arrived = true
        if (current && !(error instanceof NotNeeded)) setFailed(true)
      })
    return () => {
      current = false
      if (!arrived) latest.current.drop?.()
    }
  }, [id, asking])

  const words = props.missing ?? (failed ? (props.isVideo ? 'Video' : (props.cannot ?? 'No preview')) : null)
  return (
    <div
      className={words ? 'picture missing' : url ? 'picture' : 'picture loading'}
      title={props.name}
      aria-busy={!url && !words}
    >
      {url && !words && <img src={url} alt="" draggable={false} />}
      {words && <span>{words}</span>}
      {props.isVideo && url && !words && <span className="play" aria-label="Video" />}
    </div>
  )
}

/** The small picture of a photo a search found. */
export function FoundPicture(props: { api: Backend; photo: Photo }) {
  const { api, photo } = props
  return (
    <Picture
      id={photo.id}
      name={photo.name}
      isVideo={photo.isVideo}
      load={() => api.preview(photo.id)}
      drop={() => api.notNeeded(photoKey(photo.id))}
    />
  )
}

/** What the closer look shows about one thing. */
export type Looked = {
  key: string
  name: string
  isVideo: boolean
  /** Where it is, as a person would write it. */
  where: string
  /** Why it cannot be shown at all, said instead of a picture. */
  missing?: string
}

/**
 * One photo or video still, as large as the window allows, over the grid.
 * Opened by clicking a tile. The small picture shows at once and the large
 * one replaces it when it arrives; arrow keys move to the next and
 * previous; Escape closes. `keep`, when given, shows whether it is kept and
 * lets that be changed; `actions` adds buttons of the screen's own.
 */
export function Viewer(props: {
  item: Looked
  place: string
  small: () => Promise<string>
  large: () => Promise<string>
  keep?: { kept: boolean; onKeep: () => void }
  actions?: ReactNode
  onMove: (by: 1 | -1) => void
  onClose: () => void
}) {
  const { item, onMove, onClose } = props
  const [shown, setShown] = useState<{ key: string; url: string } | null>(null)
  const [missing, setMissing] = useState<string | null>(null)
  const box = useRef<HTMLDivElement>(null)
  const fetchers = useRef({ small: props.small, large: props.large })
  useEffect(() => {
    fetchers.current = { small: props.small, large: props.large }
  })

  useEffect(() => {
    if (item.missing) return
    let current = true
    let made: string | null = null
    fetchers.current
      .small()
      .then((url) => current && setShown((now) => (now?.key === item.key ? now : { key: item.key, url })))
      .catch(() => {})
    fetchers.current
      .large()
      .then((url) => {
        made = url
        if (current) setShown({ key: item.key, url })
      })
      .catch(() => current && setMissing(item.key))
    return () => {
      current = false
      // The large picture is only held while it is being looked at.
      if (made?.startsWith('blob:')) URL.revokeObjectURL(made)
    }
  }, [item.key, item.missing])

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

  const url = shown?.key === item.key ? shown.url : null
  const words = item.missing ?? (missing === item.key ? 'This one can’t be shown.' : null)
  // Drawn on the page itself, not inside the screen that opened it, so it
  // covers the whole window whatever that screen is nested in.
  return createPortal(
    <div className="viewer" role="dialog" aria-modal="true" aria-label={item.name} tabIndex={-1} ref={box} onClick={onClose}>
      <figure onClick={(e) => e.stopPropagation()}>
        <div className="stage">
          {url && !words ? (
            <img src={url} alt={item.name} draggable={false} />
          ) : (
            <span className="quiet">{words ?? <span className="spinner" aria-hidden />}</span>
          )}
        </div>
        <figcaption>
          <div>
            <b>{item.name}</b>
            {item.isVideo && <span className="badge">Video: this is a still from it</span>}
            <span className="quiet small where">{item.where}</span>
          </div>
          <div className="choices">
            <span className="quiet small">{props.place}</span>
            <button className="button" onClick={() => onMove(-1)} aria-label="Previous">
              ‹
            </button>
            <button className="button" onClick={() => onMove(1)} aria-label="Next">
              ›
            </button>
            {props.keep && (
              <button className="button" role="switch" aria-checked={props.keep.kept} onClick={props.keep.onKeep}>
                {props.keep.kept ? '✓ Keeping' : 'Left out'}
              </button>
            )}
            {props.actions}
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
