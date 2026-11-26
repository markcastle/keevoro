// "What's new in each version", under About: Keevoro's own changelog, as
// it is written in CHANGELOG.md, read into the app when it is built so it
// needs no internet. The newest versions are shown; the rest wait behind
// one button, as the list is long.

import { useState } from 'react'
import changelog from '../../../CHANGELOG.md?raw'
import { dayOf, versions } from '../changelogWords'
import type { Said } from '../changelogWords'

const ALL = versions(changelog)
/** How many versions are shown before the rest are asked for. */
const AT_FIRST = 8

function Words(props: { said: Said[] }) {
  return (
    <>
      {props.said.map((s, at) =>
        s.bold ? <b key={at}>{s.words}</b> : s.code ? <code key={at}>{s.words}</code> : <span key={at}>{s.words}</span>,
      )}
    </>
  )
}

export function ChangelogPage(props: { version: string }) {
  const [all, setAll] = useState(false)
  const shown = all ? ALL : ALL.slice(0, AT_FIRST)
  return (
    <div className="changelog">
      <p className="quiet">Every change you would notice, by version, newest first. You have Keevoro {props.version}.</p>
      {shown.map((v) => (
        <section key={v.version} aria-label={`Version ${v.version}`}>
          <h3>
            {v.version}
            {v.version === props.version && <span className="badge">The one you have</span>}
            {v.date && <span className="quiet small">{dayOf(v.date)}</span>}
          </h3>
          {v.groups.map((g) => (
            <div key={g.heading}>
              <h4>{g.heading}</h4>
              <ul>
                {g.changes.map((change, at) => (
                  <li key={at}>
                    <Words said={change} />
                  </li>
                ))}
              </ul>
            </div>
          ))}
        </section>
      ))}
      {!all && ALL.length > AT_FIRST && (
        <p className="choices">
          <button type="button" className="button" onClick={() => setAll(true)}>
            Show all {ALL.length} versions
          </button>
        </p>
      )}
    </div>
  )
}
