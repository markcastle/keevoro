// Manage backups: every backup with what tells it apart, to rename, open
// or remove, and those removed in the last week, to put back.
//
// The list draws at once from the names the app already has; each backup's
// details (its sources, where it goes, what it holds, when it last ran) are
// counted one after another and filled in as they come, so a backup with a
// large record never holds up opening the screen.
//
// Removing says exactly what it does before it is done, in the page and
// never in a pop-up, in Keevoro's own sentences (the same the command line
// prints), and only then offers the button that does it. A removed backup
// can be put back for a week; nothing at a destination or on a source is
// ever touched.

import { useCallback, useEffect, useState } from 'react'
import type { AppState, Backend, Backup, BackupListing, Removal, RemovedBackup } from './backend'
import { Explained } from './explained'
import { InfoTip } from './infoTip'
import { ago, files, size } from './format'
import { OpenFrom } from './openFrom'

/** The longest name a backup can have, as the API holds it to. */
const LONGEST_NAME = 60

export function ManageBackups(props: {
  api: Backend
  state: AppState
  /** Keevoro's list of backups changed: a rename, a removal or one put back. */
  onState: (state: AppState) => void
  onOpen: (id: string) => void
  /** A backup opened from its notes at a destination is open now. */
  onOpenedFrom: (state: AppState) => void
  /** Opens on "Open a backup that is already somewhere", as Help's answer does. */
  opening?: boolean
  /** Back from opening one to the list. */
  onOpeningLeft?: () => void
  onBack: () => void
  onProblem: (error: unknown) => void
}) {
  const { api, state, onProblem } = props
  const [opening, setOpening] = useState(props.opening ?? false)
  const [details, setDetails] = useState<Record<string, BackupListing>>({})
  const [removed, setRemoved] = useState<RemovedBackup[]>([])
  // Said once something is done: what was removed, put back or cleared away.
  const [done, setDone] = useState<{ said: string; putBack?: string } | null>(null)

  const ids = state.backups.map((b) => b.id).join(' ')
  const countAll = useCallback(() => {
    let open = true
    void (async () => {
      // One after another, so a large backup does not hold up the rest.
      for (const backup of state.backups) {
        try {
          const listed = await api.backupDetails(backup.id)
          if (!open) return
          setDetails((d) => ({ ...d, [backup.id]: listed }))
        } catch {
          // Removed by another program a moment ago: the list catches up.
        }
      }
    })()
    api.removedBackups().then((r) => open && setRemoved(r), onProblem)
    return () => {
      open = false
    }
    // The list changes when its backups do.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [api, ids, onProblem])
  useEffect(countAll, [countAll])

  const changed = (next: AppState) => {
    props.onState(next)
  }

  if (opening) {
    return (
      <OpenFrom
        api={api}
        onOpened={props.onOpenedFrom}
        onBack={() => {
          setOpening(false)
          props.onOpeningLeft?.()
        }}
      />
    )
  }

  return (
    <section className="manage-backups">
      <header>
        <button className="link" onClick={props.onBack}>
          ‹ Back
        </button>
        <h1>Your backups</h1>
        <p className="status">
          Each backup has its own sources and its own places to send things.
          <InfoTip about="your backups">
            Rename one, open it, or remove one you no longer need. Removing a backup never touches your photos, and it
            can be put back for a week.
          </InfoTip>
        </p>
      </header>

      {done && (
        <p className="panel done-said" role="status">
          <span>{done.said}</span>
          {done.putBack && (
            <button
              onClick={() => {
                const id = done.putBack ?? ''
                api
                  .putBackBackup(id)
                  .then((s) => {
                    setDone({ said: 'Brought back, exactly as it was.' })
                    changed(s)
                  })
                  .catch(onProblem)
              }}
            >
              Put it back
            </button>
          )}
        </p>
      )}

      {state.backups.length === 0 && <p className="panel">There are no backups now. Start one whenever you like.</p>}

      <ul className="backup-list">
        {state.backups.map((backup) => (
          <BackupRow
            key={backup.id}
            api={api}
            backup={backup}
            listed={details[backup.id]}
            isOpen={state.backup?.id === backup.id}
            onOpen={() => props.onOpen(backup.id)}
            onRenamed={(s) => {
              setDone(null)
              changed(s)
            }}
            onRemoved={(removal) => {
              setDone({ said: removal.said[0] ?? `Removed “${removal.name}”.`, putBack: removal.id })
              api.state().then(changed).catch(onProblem)
            }}
          />
        ))}
      </ul>

      <Explained
        className="open-already"
        tipAbout="opening a backup that is already somewhere"
        tip="Keevoro reads the notes it keeps beside every backup and picks the backup up again, without sending anything twice."
        action={
          <button className="button" onClick={() => setOpening(true)}>
            Open a backup that is already somewhere…
          </button>
        }
      >
        On a new computer, or after reinstalling.
      </Explained>

      {removed.length > 0 && (
        <div className="removed-recently">
          <h2>Removed recently</h2>
          <p className="quiet small">
            Each can be put back for a week, then is cleared away.
            <InfoTip about="backups removed recently">
              These can be put back exactly as they were. After a week each is cleared away by itself and the room it
              took is freed.
            </InfoTip>
          </p>
          <ul className="backup-list">
            {removed.map((r) => (
              <li key={r.id} className="backup-row removed">
                <div className="what">
                  <h3>{r.name}</h3>
                  <p className="quiet small">{r.said}</p>
                </div>
                <p className="choices">
                  <button
                    onClick={() =>
                      api
                        .putBackBackup(r.id)
                        .then((s) => {
                          setDone({ said: `Brought back “${r.name}”, exactly as it was.` })
                          changed(s)
                        })
                        .catch(onProblem)
                    }
                  >
                    Bring it back
                  </button>
                  <button
                    className="button"
                    onClick={() =>
                      api
                        .clearAwayBackup(r.id)
                        .then((gone) => {
                          setDone({ said: `Cleared away “${gone.name}”: ${size(gone.bytes)} freed. It can no longer be put back.` })
                          setRemoved((all) => all.filter((x) => x.id !== gone.id))
                        })
                        .catch(onProblem)
                    }
                  >
                    Clear away now
                  </button>
                </p>
              </li>
            ))}
          </ul>
        </div>
      )}
    </section>
  )
}

/** One backup: what it is, and Open, Rename and Remove. */
function BackupRow(props: {
  api: Backend
  backup: Backup
  listed: BackupListing | undefined
  isOpen: boolean
  onOpen: () => void
  onRenamed: (state: AppState) => void
  onRemoved: (removal: Removal) => void
}) {
  const { api, backup, listed } = props
  const [naming, setNaming] = useState<string | null>(null)
  const [refused, setRefused] = useState<string | null>(null)
  // What removing it would do, once asked, until it is done or cancelled.
  const [asking, setAsking] = useState<Removal | null>(null)
  const [removing, setRemoving] = useState(false)
  const busy = listed?.busy
  const found = listed?.found

  const save = () => {
    if (naming === null) return
    api
      .renameBackup(backup.id, naming)
      .then((s) => {
        setNaming(null)
        setRefused(null)
        props.onRenamed(s)
      })
      .catch((e: unknown) => setRefused(e instanceof Error ? e.message : String(e)))
  }
  const askFirst = () =>
    api
      .removeBackup(backup.id, true)
      .then(setAsking)
      .catch((e: unknown) => setRefused(e instanceof Error ? e.message : String(e)))
  const remove = () => {
    setRemoving(true)
    api
      .removeBackup(backup.id, false)
      .then(props.onRemoved)
      .catch((e: unknown) => {
        setRemoving(false)
        setAsking(null)
        setRefused(e instanceof Error ? e.message : String(e))
      })
  }

  return (
    <li className={props.isOpen ? 'backup-row is-open' : 'backup-row'}>
      <div className="what">
        {naming === null ? (
          <h2>
            {backup.name}
            {props.isOpen && <span className="badge">Open now</span>}
          </h2>
        ) : (
          <form
            className="naming"
            onSubmit={(e) => {
              e.preventDefault()
              save()
            }}
          >
            <input
              value={naming}
              maxLength={LONGEST_NAME}
              aria-label={`New name for ${backup.name}`}
              autoFocus
              onChange={(e) => setNaming(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Escape') {
                  setNaming(null)
                  setRefused(null)
                }
              }}
            />
            <button className="primary" type="submit">
              Save
            </button>
            <button
              type="button"
              onClick={() => {
                setNaming(null)
                setRefused(null)
              }}
            >
              Cancel
            </button>
          </form>
        )}
        <Details listed={listed} />
      </div>

      {refused && (
        <p className="problem inline" role="alert">
          {refused}
        </p>
      )}
      {busy && !refused && <p className="quiet small busy-said">{busy}</p>}

      {asking ? (
        <div className="removing" role="region" aria-labelledby={`remove-${backup.id}`}>
          <h3 id={`remove-${backup.id}`}>Remove “{backup.name}” from Keevoro?</h3>
          <ul>
            {asking.said.map((line) => (
              <li key={line}>{line}</li>
            ))}
          </ul>
          <p className="choices">
            <button className="danger" onClick={remove} disabled={removing}>
              {removing ? 'Removing…' : 'Remove this backup from Keevoro'}
            </button>
            <button onClick={() => setAsking(null)} disabled={removing}>
              Cancel
            </button>
          </p>
        </div>
      ) : (
        naming === null && (
          <p className="choices">
            {!props.isOpen && (
              <button onClick={props.onOpen} disabled={Boolean(busy)}>
                Open
              </button>
            )}
            <button
              onClick={() => {
                setRefused(null)
                setNaming(backup.name)
              }}
              disabled={Boolean(busy)}
            >
              Rename
            </button>
            <button className="danger" onClick={askFirst} disabled={Boolean(busy)}>
              Remove…
            </button>
          </p>
        )
      )}
      {found === undefined && listed === undefined && <p className="quiet small">Counting what it holds…</p>}
    </li>
  )
}

/** What tells one backup from another: where it looks, where it goes, how much, and when it last ran. */
function Details(props: { listed: BackupListing | undefined }) {
  const listed = props.listed
  if (!listed) return null
  const found = listed.found
  if (!found) {
    return (
      <p className="quiet small">
        Its folder isn’t there right now (<span className="where">{listed.home}</span>). Is the drive it is on plugged in?
      </p>
    )
  }
  return (
    <dl className="facts">
      <dt>Looks in</dt>
      <dd>{found.sourcePlaces.length > 0 ? found.sourcePlaces.join(', ') : 'No sources yet'}</dd>
      <dt>Backs up to</dt>
      <dd>{found.destinationNames.length > 0 ? found.destinationNames.join(', ') : 'Nowhere yet'}</dd>
      <dt>Holds</dt>
      <dd>
        {found.photos + found.videos + (found.others ?? 0) > 0
          ? `${files(found.photos, found.videos, found.others ?? 0)}, ${size(found.bytes)}`
          : 'Nothing found yet'}
      </dd>
      <dt>Last backed up</dt>
      <dd>{found.lastRan ? ago(found.lastRan) : 'Never'}</dd>
    </dl>
  )
}
