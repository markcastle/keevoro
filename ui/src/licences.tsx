// The open source licences, a page of Help, About Keevoro. Keevoro is
// built on other people's work, and this names every piece shipped in the
// app with its licence. The list is `licences.json`, made by `cargo xtask licences` from
// Cargo.lock and package-lock.json, and a test fails when it is out of date,
// so nothing here is written by hand.

import { useMemo, useState } from 'react'
import made from './licences.json'

/** One piece of other people's work, as `cargo xtask licences` lists it. */
export type Licensed = { name: string; version: string; licence: string; home: string }

const LISTS: { rust: Licensed[]; npm: Licensed[] } = made

/** The entries whose name holds `typed`, ignoring case. */
export function matching(entries: Licensed[], typed: string): Licensed[] {
  const wanted = typed.trim().toLowerCase()
  return wanted === '' ? entries : entries.filter((e) => e.name.toLowerCase().includes(wanted))
}

const THANKS =
  'Keevoro is built on the work of many open source projects. Each one in the app is listed here with its licence. Thank you to everyone who made them.'

/** The list itself, with a box to find one by name. */
export function LicencesList(props: { withThanks?: boolean }) {
  const [typed, setTyped] = useState('')
  const rust = useMemo(() => matching(LISTS.rust, typed), [typed])
  const npm = useMemo(() => matching(LISTS.npm, typed), [typed])
  return (
    <div className="panel licences">
      {props.withThanks && <p className="quiet">{THANKS}</p>}
      <label className="find">
        Find by name
        <input type="search" value={typed} onChange={(e) => setTyped(e.target.value)} placeholder="For example, react" />
      </label>
      <Listed
        title="The app and the keevoro command"
        about="Rust crates, from crates.io."
        entries={rust}
        of={LISTS.rust.length}
      />
      <Listed title="The app’s screens" about="Packages from npm." entries={npm} of={LISTS.npm.length} />
    </div>
  )
}

function Listed(props: { title: string; about: string; entries: Licensed[]; of: number }) {
  const shown = props.entries.length === props.of ? `${props.of}` : `${props.entries.length} of ${props.of}`
  return (
    <>
      <h2>
        {props.title} <span className="quiet">({shown})</span>
      </h2>
      <p className="quiet small">{props.about}</p>
      {props.entries.length === 0 ? (
        <p className="quiet">None by that name.</p>
      ) : (
        <ul className="licensed">
          {props.entries.map((e) => (
            <li key={`${e.name} ${e.version}`}>
              <span className="what">
                <strong>{e.name}</strong> <span className="quiet">{e.version}</span>
              </span>
              <span className="licence">{e.licence}</span>
              <span className="home quiet small">{e.home}</span>
            </li>
          ))}
        </ul>
      )}
    </>
  )
}
