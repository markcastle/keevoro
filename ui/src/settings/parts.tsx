// The pieces every section of Settings is made of, so each one looks and
// behaves the same:
//
//   Group   a rounded panel of rows, with an optional small heading above
//           and a sentence of footnote below
//   Row     one setting: its name on the left, at most one short sentence
//           beneath it in quieter type, and its control on the right
//   OpenRow a row that opens a page of its own, with a chevron
//
// A row's control is named by the row's name and described by its
// sentence, for a screen reader, through RowSwitch and RowButton. Each row
// has the id `setting-<its name in places.ts>`, which is how a link from
// elsewhere in the app, or a search, finds it to bring it into view.

import { createContext, useContext, useId } from 'react'
import type { ReactNode } from 'react'
import { rowInfo } from './places'
import type { SectionId } from './places'

type Ids = { title: string; about?: string }
const RowIds = createContext<Ids | null>(null)

/** A rounded panel of rows, with a small heading above and a footnote below. */
export function Group(props: {
  title?: ReactNode
  footnote?: ReactNode
  /** Something that cannot be undone lives here, set apart. */
  danger?: boolean
  children: ReactNode
  className?: string
}) {
  const id = useId()
  return (
    <div
      className={`kv-group${props.className ? ` ${props.className}` : ''}`}
      role={props.title ? 'group' : undefined}
      aria-labelledby={props.title ? id : undefined}
    >
      {props.title && (
        <h3 className="kv-group-title" id={id}>
          {props.title}
        </h3>
      )}
      <div className={props.danger ? 'kv-panel danger' : 'kv-panel'}>{props.children}</div>
      {props.footnote && <div className="kv-footnote">{props.footnote}</div>}
    </div>
  )
}

/**
 * One setting. `status` is a sentence said once something is done; its
 * line is kept whether or not there is one (`status` given as null), so
 * nothing below moves when it appears.
 */
export function Row(props: {
  id: string
  /** The row's name, when it is not the one in places.ts (a version number, say). */
  title?: ReactNode
  about?: ReactNode
  /** What it is set to, in words, before the control. */
  value?: ReactNode
  control?: ReactNode
  status?: ReactNode
  /** Shown under the row, the width of the panel: a question asked in the page, say. */
  below?: ReactNode
  className?: string
}) {
  const base = useId()
  const ids: Ids = { title: `${base}t`, about: props.about ? `${base}a` : undefined }
  return (
    <div
      className={`kv-row${props.className ? ` ${props.className}` : ''}`}
      id={`setting-${props.id}`}
    >
      <div className="kv-row-main">
        <div className="kv-row-text">
          <span className="kv-row-title" id={ids.title}>
            {props.title ?? rowInfo(props.id).title}
          </span>
          {props.about && (
            <span className="kv-row-about" id={ids.about}>
              {props.about}
            </span>
          )}
          {props.status !== undefined && (
            <span className="kv-row-status" role="status">
              {props.status}
            </span>
          )}
        </div>
        {(props.value || props.control) && (
          <div className="kv-row-end">
            {props.value && <span className="kv-row-value">{props.value}</span>}
            <RowIds.Provider value={ids}>{props.control}</RowIds.Provider>
          </div>
        )}
      </div>
      {props.below && <div className="kv-row-below">{props.below}</div>}
    </div>
  )
}

/** The design system's switch, as a row's control: named by the row, described by its sentence. */
export function RowSwitch(props: { on: boolean; disabled?: boolean; onChange: (on: boolean) => void }) {
  const ids = useContext(RowIds)
  return (
    <button
      type="button"
      className={props.on ? 'kv-switch on' : 'kv-switch'}
      role="switch"
      aria-checked={props.on}
      aria-labelledby={ids?.title}
      aria-describedby={ids?.about}
      disabled={props.disabled}
      onClick={() => props.onChange(!props.on)}
    >
      <span className="knob" />
    </button>
  )
}

/** A button as a row's control: its own words name it, and the row says what it is for. */
export function RowButton(props: {
  children: ReactNode
  onClick: () => void
  disabled?: boolean
  danger?: boolean
}) {
  const ids = useContext(RowIds)
  return (
    <button
      type="button"
      className={props.danger ? 'button small danger' : 'button small'}
      aria-describedby={[ids?.title, ids?.about].filter(Boolean).join(' ') || undefined}
      disabled={props.disabled}
      onClick={props.onClick}
    >
      {props.children}
    </button>
  )
}

/** A row that opens a page of its own. The whole row is the button. */
export function OpenRow(props: { id: string; about?: ReactNode; value?: ReactNode; onOpen: () => void }) {
  const base = useId()
  return (
    <button
      type="button"
      className="kv-row kv-open-row"
      id={`setting-${props.id}`}
      aria-labelledby={`${base}t${props.value ? ` ${base}v` : ''}`}
      aria-describedby={props.about ? `${base}a` : undefined}
      onClick={props.onOpen}
    >
      <span className="kv-row-main">
        <span className="kv-row-text">
          <span className="kv-row-title" id={`${base}t`}>
            {rowInfo(props.id).title}
          </span>
          {props.about && (
            <span className="kv-row-about" id={`${base}a`}>
              {props.about}
            </span>
          )}
        </span>
        <span className="kv-row-end">
          {props.value && (
            <span className="kv-row-value" id={`${base}v`}>
              {props.value}
            </span>
          )}
          <Chevron />
        </span>
      </span>
    </button>
  )
}

export function Chevron(props: { back?: boolean }) {
  return (
    <svg className="kv-chevron" width="8" height="14" viewBox="0 0 8 14" aria-hidden="true">
      <path d={props.back ? 'M7 1 1 7l6 6' : 'm1 1 6 6-6 6'} fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" />
    </svg>
  )
}

/**
 * What a change means, said under its panel. Its two lines are kept when
 * there is nothing to say, so nothing below moves when there is.
 */
export function Said(props: { children: ReactNode }) {
  return (
    <p className="kv-said" role="status">
      {props.children}
    </p>
  )
}

/** A section's one-line introduction, under its name. */
export function Intro(props: { children: ReactNode }) {
  return <p className="kv-intro">{props.children}</p>
}

/** Each section's simple picture, drawn in the side list beside its name. */
export function SectionIcon(props: { id: SectionId }) {
  const paths: Record<SectionId, ReactNode> = {
    // A box of keepsakes.
    backup: (
      <>
        <rect x="3.5" y="4" width="13" height="4" rx="1" />
        <path d="M4.5 8v7a1 1 0 0 0 1 1h9a1 1 0 0 0 1-1V8M8 11h4" />
      </>
    ),
    // A clock.
    byItself: (
      <>
        <circle cx="10" cy="10" r="6.5" />
        <path d="M10 6.5V10l2.5 1.5" />
      </>
    ),
    // Half light, half dark.
    appearance: (
      <>
        <circle cx="10" cy="10" r="6.5" />
        <path d="M10 3.5a6.5 6.5 0 0 1 0 13Z" fill="currentColor" />
      </>
    ),
    // An arrow coming down.
    updates: (
      <>
        <circle cx="10" cy="10" r="6.5" />
        <path d="M10 6.5v6.5M7.25 10.5 10 13.25l2.75-2.75" />
      </>
    ),
    // Two sliders.
    advanced: (
      <>
        <path d="M4 7h12M4 13h12" />
        <circle cx="8" cy="7" r="1.75" fill="var(--kv-icon-ground)" />
        <circle cx="12.5" cy="13" r="1.75" fill="var(--kv-icon-ground)" />
      </>
    ),
    // A question.
    help: (
      <>
        <circle cx="10" cy="10" r="6.5" />
        <path d="M8 8.25a2 2 0 1 1 2.75 1.85c-.5.2-.75.6-.75 1.15v.25" />
        <circle cx="10" cy="13.6" r="0.4" fill="currentColor" />
      </>
    ),
  }
  return (
    <span className="kv-icon" aria-hidden="true">
      <svg width="20" height="20" viewBox="0 0 20 20" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
        {paths[props.id]}
      </svg>
    </span>
  )
}
