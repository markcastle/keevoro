// The list of folders a survey suggests, with those that live together
// shown together: one row for the folder they share, with its counts and a
// tick for all of them, that opens to show each folder by the part of its
// path below it. Look inside and Leave out on that row act on the shared
// folder itself, as they do on any one folder. Which groups are open, like
// the ticks, is kept by the screen underneath, so looking inside one and
// coming back finds the list as it was.

import { useEffect, useId, useRef } from 'react'
import type { KeyboardEvent } from 'react'
import type { Backend, Hint, LeftOutFolder } from './backend'
import { count, size } from './format'
import { foundFor } from './holds'
import { LeaveOutButton } from './leaveOut'
import { notPhotos, outBy, startsOpen, tickOf, within } from './surveyGroups'
import type { GroupRow, SurveyItem } from './surveyGroups'

/** The note on a folder whose pictures are very small and which has no videos. */
const SMALL_PICTURES = 'Small pictures, probably not photos'
const SMALL_PICTURES_WHY = 'The pictures here are very small and there are no videos'

type Shared = {
  api: Backend
  holds: readonly string[]
  chosen: Set<string>
  /** Folders left out from this list, each with everything beneath it. */
  out: Set<string>
  /** Groups opened or closed by the person, by key; the rest are as they start. */
  opened: Record<string, boolean>
  onSet: (folders: string[], on: boolean) => void
  onOpen: (key: string, open: boolean) => void
  onLookInside: (folder: string) => void
  onLeftOut: (folder: string, done: LeftOutFolder) => void
  onProblem: (error: unknown) => void
}

/** True when it is left out itself, not only by being inside one left out. */
const outItself = (folder: string, out: Set<string>) => [...out].some((o) => within(folder, o) && within(o, folder))
const lastName = (folder: string) => folder.replace(/[\\/]+$/, '').split(/[\\/]/).pop() || folder

export function SurveyHints(props: Shared & { items: SurveyItem[] }) {
  const { items, ...shared } = props
  return (
    <ul className="hints">
      {items.map((item) => (
        <Item key={item.kind === 'group' ? item.key : item.hint.folder} item={item} shared={shared} noteAbove={false} />
      ))}
    </ul>
  )
}

function Item(props: { item: SurveyItem; shared: Shared; noteAbove: boolean }) {
  return props.item.kind === 'group' ? (
    <Group group={props.item} shared={props.shared} noteAbove={props.noteAbove} />
  ) : (
    <Folder hint={props.item.hint} label={props.item.label} shared={props.shared} noteAbove={props.noteAbove} />
  )
}

/** A path as shown: the part below its group, with all of it to hover over and to hear. */
function Where(props: { label: string; folder: string }) {
  if (props.label === props.folder) return <span className="where">{props.folder}</span>
  return (
    <span className="where" title={props.folder}>
      <span aria-hidden="true">{props.label}</span>
      <span className="visually-hidden">{props.folder}</span>
    </span>
  )
}

function Folder(props: { hint: Hint; label: string; shared: Shared; noteAbove: boolean }) {
  const { hint, shared } = props
  const out = outBy(hint.folder, shared.out) !== undefined
  return (
    <li className={hint.alreadyASource ? 'suggested done' : 'suggested'}>
      <label>
        <input
          type="checkbox"
          checked={shared.chosen.has(hint.folder) && !out}
          disabled={hint.alreadyASource || out}
          onChange={(event) => shared.onSet([hint.folder], event.target.checked)}
        />
        <Where label={props.label} folder={hint.folder} />
      </label>
      {/* What is in it, and what is known of it, together before its buttons. */}
      <span className="about">
        <span className="quiet small">
          {foundFor(shared.holds, hint.photos, hint.videos, hint.others ?? 0, hint.music ?? 0)} · {size(hint.bytes)}
        </span>
        {outItself(hint.folder, shared.out) && <span className="badge">Left out of this backup</span>}
        {hint.alreadyASource && <span className="badge">Already a source</span>}
        {notPhotos(hint) && !props.noteAbove && (
          <span className="badge" title={SMALL_PICTURES_WHY}>
            {SMALL_PICTURES}
          </span>
        )}
      </span>
      <LookInside folder={hint.folder} shared={shared} />
      {!hint.alreadyASource && (!out || outItself(hint.folder, shared.out)) && (
        <LeaveOut folder={hint.folder} shared={shared} />
      )}
      {hint.onlineOnlySaid && <p className="online-only small">{hint.onlineOnlySaid}</p>}
    </li>
  )
}

function Group(props: { group: GroupRow; shared: Shared; noteAbove: boolean }) {
  const { group, shared } = props
  const id = useId()
  const box = useRef<HTMLInputElement>(null)
  const open = shared.opened[group.key] ?? startsOpen(group)
  const out = outBy(group.folder, shared.out) !== undefined
  const free = group.hints.filter((h) => !h.alreadyASource && outBy(h.folder, shared.out) === undefined).map((h) => h.folder)
  const can = new Set(free)
  const tick = tickOf(
    group.hints.map((h) => h.folder),
    (f) => shared.chosen.has(f),
    (f) => can.has(f),
  )
  // A native tick box shows some ticked, and says "mixed" to anyone listening, only when told in code.
  useEffect(() => {
    if (box.current) box.current.indeterminate = tick === 'mixed'
  }, [tick])
  const { totals } = group
  const allSame = totals.folders > 0 && group.notPhotos === totals.folders
  const allSources = group.hints.every((h) => h.alreadyASource)
  // Right opens and Left closes, as in any list that opens.
  const keys = (event: KeyboardEvent) => {
    if (event.key === 'ArrowRight' && !open) shared.onOpen(group.key, true)
    else if (event.key === 'ArrowLeft' && open) shared.onOpen(group.key, false)
    else return
    event.preventDefault()
  }
  return (
    <li className={allSources ? 'group done' : 'group'}>
      <div className="group-head" onKeyDown={keys}>
        <button
          className="disclosure"
          aria-expanded={open}
          aria-controls={open ? id : undefined}
          aria-label={`The folders in ${lastName(group.folder)}`}
          onClick={() => shared.onOpen(group.key, !open)}
        >
          <span aria-hidden="true">›</span>
        </button>
        <label>
          <input
            ref={box}
            type="checkbox"
            checked={tick === 'all'}
            disabled={tick === 'off'}
            onChange={() => shared.onSet(free, tick !== 'all')}
          />
          <Where label={group.label} folder={group.folder} />
        </label>
        <span className="about">
          <span className="quiet small">
            {count(totals.folders, 'folder')} · {foundFor(shared.holds, totals.photos, totals.videos, totals.others, totals.music)} ·{' '}
            {size(totals.bytes)}
          </span>
          {outItself(group.folder, shared.out) && <span className="badge">Left out of this backup</span>}
          {allSources && <span className="badge">Already sources</span>}
          {allSame && !props.noteAbove && (
            <span className="badge" title={SMALL_PICTURES_WHY}>
              {SMALL_PICTURES}
            </span>
          )}
          {group.notPhotos > 0 && !allSame && !props.noteAbove && (
            <span className="badge">
              {group.notPhotos} of {totals.folders} {group.notPhotos === 1 ? 'is' : 'are'} probably not photos
            </span>
          )}
        </span>
        <LookInside folder={group.folder} shared={shared} />
        {!allSources && (!out || outItself(group.folder, shared.out)) && <LeaveOut folder={group.folder} shared={shared} />}
      </div>
      {open && (
        <ul className="hints" id={id}>
          {group.items.map((item) => (
            <Item
              key={item.kind === 'group' ? item.key : item.hint.folder}
              item={item}
              shared={shared}
              noteAbove={props.noteAbove || allSame}
            />
          ))}
        </ul>
      )}
    </li>
  )
}

function LookInside(props: { folder: string; shared: Shared }) {
  return (
    <button
      className="button small"
      onClick={() => props.shared.onLookInside(props.folder)}
      aria-label={`Look inside ${props.folder}`}
      title="See its photos and videos first. Nothing is added or changed."
    >
      Look inside
    </button>
  )
}

function LeaveOut(props: { folder: string; shared: Shared }) {
  const { shared } = props
  return (
    <LeaveOutButton
      api={shared.api}
      folder={props.folder}
      leftOut={outItself(props.folder, shared.out)}
      words="Leave out"
      small
      onChanged={(done) => shared.onLeftOut(props.folder, done)}
      onProblem={shared.onProblem}
    />
  )
}
