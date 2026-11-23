// Opening a backup that is already somewhere: on a new computer, after a
// reinstall, or when Keevoro's record of it here was lost.
//
// Keevoro keeps its notes beside every backup it makes. The person points at
// the drive or folder, or gives the cloud storage and its keys; Keevoro reads
// the notes there, changing nothing, and says what it found. "Open this
// backup" makes it a backup on this computer that knows every file already
// there, so nothing is sent again. Where its photos live on this computer is
// the person's to say: each source is connected again by them, never guessed.

import { useState } from 'react'
import type { AppState, Backend, ConnectRequest, FoundBackup, OpenedBackup, WaitingSource } from './backend'
import { Explained } from './explained'
import { count, size } from './format'
import { StorageFields } from './storageForm'
import type { StorageAnswers } from './storageForm'

function said(error: unknown) {
  return error instanceof Error ? error.message : String(error)
}

export function OpenFrom(props: {
  api: Backend
  /** The backup is open; the app shows it. */
  onOpened: (state: AppState) => void
  onBack: () => void
}) {
  const { api } = props
  const [cloud, setCloud] = useState<StorageAnswers | null>(null)
  const [asked, setAsked] = useState<ConnectRequest | null>(null)
  const [found, setFound] = useState<FoundBackup | null>(null)
  const [opened, setOpened] = useState<OpenedBackup | null>(null)
  const [busy, setBusy] = useState<string | null>(null)
  const [refused, setRefused] = useState<string | null>(null)

  const look = (request: ConnectRequest) => {
    setBusy('Looking for Keevoro’s notes there…')
    setRefused(null)
    setAsked(request)
    api
      .findBackupAt(request)
      .then(setFound)
      .catch((e: unknown) => setRefused(said(e)))
      .finally(() => setBusy(null))
  }
  const openIt = () => {
    if (!asked) return
    setBusy('Opening it…')
    setRefused(null)
    api
      .openBackupFrom(asked)
      .then(setOpened)
      .catch((e: unknown) => setRefused(said(e)))
      .finally(() => setBusy(null))
  }
  const again = () => {
    setFound(null)
    setAsked(null)
    setCloud(null)
    setRefused(null)
  }
  const done = () => api.state().then(props.onOpened, (e: unknown) => setRefused(said(e)))

  if (opened) {
    return (
      <section className="open-from" aria-labelledby="open-from-title">
        <h1 id="open-from-title">“{opened.name}” is open</h1>
        {opened.said.map((line) => (
          <p key={line} className={line === opened.said[0] ? 'status' : 'quiet'}>
            {line}
          </p>
        ))}
        {(opened.waiting ?? []).length > 0 && (
          <div className="panel reconnect">
            <h2>Connect your sources again</h2>
            {(opened.waiting ?? []).map((source) => (
              <ConnectAgain key={source.id} api={api} source={source} />
            ))}
          </div>
        )}
        <div className="choices">
          <button className="primary" onClick={done}>
            Go to this backup
          </button>
        </div>
        {refused && (
          <p className="problem inline" role="alert">
            {refused}
          </p>
        )}
      </section>
    )
  }

  return (
    <section className="open-from" aria-labelledby="open-from-title">
      <button className="link" onClick={props.onBack}>
        ‹ Back
      </button>
      <h1 id="open-from-title">Where is your backup?</h1>
      <p className="status">
        Choose the drive, folder or cloud storage it is in. Keevoro reads the notes it keeps beside every backup, and
        changes nothing there until you say so.
      </p>

      {!found && !cloud && (
        <div className="places">
          <button
            className="place"
            disabled={busy !== null}
            onClick={() =>
              api
                .chooseWhereBackupIs()
                .then((folder) => folder && look({ kind: 'folder', place: folder }))
                .catch((e: unknown) => setRefused(said(e)))
            }
          >
            <b>A drive or folder</b>
            <span>A USB drive, another disk, or a network drive</span>
          </button>
          <button className="place" disabled={busy !== null} onClick={() => setCloud({ kind: 'r2', place: '', bucket: '' })}>
            <b>Cloud storage</b>
            <span>A bucket on Cloudflare R2, Amazon S3, Google Cloud Storage or similar</span>
          </button>
        </div>
      )}

      {!found && cloud && (
        <form
          className="connect"
          onSubmit={(e) => {
            e.preventDefault()
            look({ ...cloud })
          }}
        >
          <StorageFields form={cloud} onChange={setCloud} />
          <p className="quiet">
            Looking only reads. If you open the backup, the keys are kept in your computer’s keychain.
          </p>
          <div className="choices">
            <button className="primary" type="submit" disabled={busy !== null}>
              Look there
            </button>
            <button className="button" type="button" onClick={() => setCloud(null)}>
              Cancel
            </button>
          </div>
        </form>
      )}

      {busy && (
        <p className="quiet" role="status">
          {busy}
        </p>
      )}

      {found && <FoundSummary found={found} busy={busy !== null} onOpen={openIt} onAgain={again} />}

      {refused && (
        <p className="problem inline" role="alert">
          {refused}
        </p>
      )}
    </section>
  )
}

/** What was found, and what can be done with it. */
function FoundSummary(props: { found: FoundBackup; busy: boolean; onOpen: () => void; onAgain: () => void }) {
  const { found } = props
  const usable = found.found === 'notes' || found.found === 'earlierNotes'
  if (!usable) {
    return (
      <div className={found.found === 'newer' ? 'panel found-backup refused' : 'panel found-backup'}>
        <h2>{found.found === 'nothing' ? 'No backup there' : found.found === 'newer' ? 'Update Keevoro first' : 'This backup can’t be opened from here'}</h2>
        {found.said.map((line) => (
          <p key={line}>{line}</p>
        ))}
        <div className="choices">
          <button onClick={props.onAgain}>Look somewhere else</button>
        </div>
      </div>
    )
  }
  const sources = found.sources ?? []
  return (
    <div className="panel found-backup">
      <h2>{found.name}</h2>
      <dl className="facts">
        <dt>Holds</dt>
        <dd>
          {count(found.files ?? 0, 'file')}, {size(found.bytes ?? 0)}
        </dd>
        <dt>Last backed up</dt>
        <dd>{found.lastBackedUpSaid ?? 'Not recorded'}</dd>
        <dt>Last proved</dt>
        <dd>{found.lastProvedSaid ?? 'Not yet'}</dd>
        <dt>Notes written by</dt>
        <dd>{found.madeBy ?? 'Keevoro'}</dd>
        <dt>Where</dt>
        <dd>{found.place.replace(/^the (folder )?/, '')}</dd>
      </dl>
      {found.found === 'earlierNotes' && (
        <p className="quiet small earlier-said">{found.said.find((line) => line.startsWith('Its newest notes are damaged'))}</p>
      )}
      {sources.length > 0 && (
        <p className="quiet small">
          Its sources on the computer it came from: {sources.map((s) => `${s.name} (${s.wasAt})`).join(', ')}.
        </p>
      )}
      {found.alreadyOpen && <p className="status">This backup is already on this computer, as “{found.alreadyOpen}”.</p>}
      <div className="choices">
        <button className="primary" onClick={props.onOpen} disabled={props.busy}>
          {found.alreadyOpen ? 'Open it' : 'Open this backup'}
        </button>
        <button onClick={props.onAgain} disabled={props.busy}>
          Look somewhere else
        </button>
      </div>
    </div>
  )
}

/**
 * One source of a backup opened from its notes, waiting to be connected on
 * this computer: where it was, and the button that asks where it is now.
 * Connected, it is looked through again; files already backed up are
 * recognised by their fingerprints and not sent again.
 */
export function ConnectAgain(props: { api: Backend; source: WaitingSource; onConnected?: () => void }) {
  const { api, source } = props
  const [done, setDone] = useState<string | null>(null)
  const [refused, setRefused] = useState<string | null>(null)
  const connect = () =>
    api
      .pickPlace()
      .then(async (place) => {
        if (!place) return
        const connected = await api.connectSourceAgain(source.id, place)
        await api.lookAgain(source.id)
        setRefused(null)
        setDone(`Connected to ${connected.place}. Keevoro is looking through it now; files already backed up are not sent again.`)
        props.onConnected?.()
      })
      .catch((e: unknown) => setRefused(said(e)))
  return (
    <div className="connect-again">
      <Explained
        action={
          done ? undefined : (
            <button className="button" onClick={connect}>
              Choose where they are…
            </button>
          )
        }
      >
        <b>{source.name}</b>:{' '}
        {done ?? (
          <>
            Keevoro does not know where these photos live on this computer. On the computer this backup came from they
            were in <span className="where">{source.wasAt}</span>.
          </>
        )}
      </Explained>
      {refused && (
        <p className="problem inline" role="alert">
          {refused}
        </p>
      )}
    </div>
  )
}
