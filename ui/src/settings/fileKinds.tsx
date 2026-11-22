// Settings, Advanced, Kinds of file: the endings Keevoro looks for, for
// the whole of Keevoro. A built-in kind can be switched off and made a
// photo, a video or an other file; one of your own can also be renamed and
// removed. Any ending can be added as an other file (music, documents),
// carried along and backed up exactly, but never shown or grouped by date.
// A change counts from the next time a source is looked through, and never
// takes away anything already found or backed up.

import { useCallback, useEffect, useState } from 'react'
import type { Backend, FileKind, ListedAs } from '../backend'
import { InfoTip, MoreAbout } from '../infoTip'
import { Intro } from './parts'

/** What a refusal from Keevoro says, as a sentence. */
function sentence(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}

/** An ending as it is shown: with its dot. */
const dotted = (ending: string) => `.${ending}`

export function FileKindsPage(props: {
  api: Backend
  onSources: () => void
  onChanged: () => void
  onProblem: (error: unknown) => void
}) {
  const { api, onProblem } = props
  const [kinds, setKinds] = useState<FileKind[] | null>(null)
  // Which line is being edited ('' for a new one), with what is typed.
  const [editing, setEditing] = useState<{ was: string; ending: string; kind: ListedAs } | null>(null)
  const [refusal, setRefusal] = useState<string | null>(null)
  const [removing, setRemoving] = useState<string | null>(null)
  const [changed, setChanged] = useState(false)
  const load = useCallback(() => api.fileKinds().then(setKinds).catch(onProblem), [api, onProblem])
  useEffect(() => {
    void load()
  }, [load])

  const done = () => {
    setChanged(true)
    props.onChanged()
    setEditing(null)
    setRefusal(null)
    setRemoving(null)
    return load()
  }
  const save = (kind = editing?.kind) => {
    if (!editing || !kind) return
    const asked =
      editing.was === ''
        ? api.addFileKind(editing.ending, kind)
        : api.changeFileKind(editing.was, { kind, to: editing.ending })
    asked.then(done).catch((e) => setRefusal(sentence(e)))
  }
  const switchOn = (kind: FileKind) =>
    api
      .changeFileKind(kind.ending, { on: !kind.on })
      .then(done)
      .catch(onProblem)

  const form = (title: string) =>
    editing && (
      <form
        className="kind-form"
        aria-label={title}
        onSubmit={(e) => {
          e.preventDefault()
          save()
        }}
      >
        <label>
          Ending
          <input
            value={editing.ending}
            disabled={editing.was !== '' && kinds?.find((k) => k.ending === editing.was)?.builtIn}
            placeholder="Such as heif"
            autoFocus
            spellCheck={false}
            onChange={(e) => setEditing({ ...editing, ending: e.target.value })}
          />
        </label>
        <label>
          They are
          <select
            value={editing.kind}
            onChange={(e) => {
              setRefusal(null)
              setEditing({ ...editing, kind: e.target.value as ListedAs })
            }}
          >
            <option value="photo">Photos</option>
            <option value="video">Videos</option>
            <option value="other">Other files</option>
          </select>
        </label>
        {editing.kind === 'other' && (
          <p className="quiet small what-to-expect">
            Backed up exactly, but not shown or grouped by date.
            <InfoTip about="other files">
              Other files are anything you want kept with your photos, such as music or documents. Keevoro will back
              these up exactly, but cannot show them or group them by date.
            </InfoTip>
          </p>
        )}
        <span className="choices">
          <button type="submit" className="primary">
            {editing.was === '' ? 'Add' : 'Save'}
          </button>
          <button
            type="button"
            onClick={() => {
              setEditing(null)
              setRefusal(null)
            }}
          >
            Cancel
          </button>
        </span>
        {refusal && (
          <p className="refusal" role="alert">
            {refusal}
            {editing.kind !== 'other' && /as an other file/.test(refusal) && (
              <>
                {' '}
                <button type="button" className="link" onClick={() => save('other')}>
                  Add it as other files
                </button>
              </>
            )}
          </p>
        )}
      </form>
    )

  return (
    <div className="kinds">
      <Intro>Add a kind your camera makes that is missing here. One list serves every backup.</Intro>
      <MoreAbout className="kv-more">
        <p>
          Keevoro finds photos and videos by the ending of their names, such as .jpg, and for the kinds it knows, by
          looking inside too.
        </p>
        <p>
          A kind Keevoro cannot open is backed up just the same, exactly as it is, with “No preview” where its picture
          would be. Changes count from the next time a source is looked through. Switching a kind off or removing it
          never takes away anything already found or backed up.
        </p>
      </MoreAbout>
      <p className="choices again" role="status">
        {changed && (
          <>
            <span className="quiet small">Changed. Look through your sources again to find files of the new kinds.</span>
            <button className="button small" onClick={props.onSources}>
              Go to your sources
            </button>
          </>
        )}
      </p>
      {kinds && (
        <div className="kinds-table" tabIndex={0} aria-label="Kinds of file, scrolls sideways when narrow">
          <table>
            <thead>
              <tr>
                <th scope="col">Ending</th>
                <th scope="col">Kind</th>
                <th scope="col">From</th>
                <th scope="col">Looked for</th>
                <th scope="col">
                  <span className="visually-hidden">Change</span>
                </th>
              </tr>
            </thead>
            <tbody>
              {kinds.map((kind) =>
                removing === kind.ending ? (
                  <tr key={kind.ending} className="removing">
                    <td colSpan={5} role="alertdialog" aria-label={`Remove ${dotted(kind.ending)}?`}>
                      <span>
                        Take <b>{dotted(kind.ending)}</b> off the list? Files of this kind already found or backed up
                        stay where they are.
                      </span>
                      <span className="choices">
                        <button
                          className="danger"
                          onClick={() =>
                            api
                              .removeFileKind(kind.ending)
                              .then(done)
                              .catch(onProblem)
                          }
                        >
                          Yes, take it off
                        </button>
                        <button onClick={() => setRemoving(null)}>Cancel</button>
                      </span>
                    </td>
                  </tr>
                ) : editing?.was === kind.ending ? (
                  <tr key={kind.ending} className="editing">
                    <td colSpan={5}>{form(`Change ${dotted(kind.ending)}`)}</td>
                  </tr>
                ) : (
                  <tr key={kind.ending} className={kind.on ? undefined : 'off'}>
                    <td className="ending">{dotted(kind.ending)}</td>
                    <td>{kind.kind === 'photo' ? 'Photo' : kind.kind === 'video' ? 'Video' : 'Other file'}</td>
                    <td>{kind.builtIn ? 'Built in' : 'Yours'}</td>
                    <td>
                      <button
                        className={kind.on ? 'kv-switch small on' : 'kv-switch small'}
                        role="switch"
                        aria-checked={kind.on}
                        aria-label={`Look for ${dotted(kind.ending)} files`}
                        onClick={() => switchOn(kind)}
                      >
                        <span className="knob" />
                      </button>
                    </td>
                    <td className="actions">
                      <button
                        className="link"
                        aria-label={`Change ${dotted(kind.ending)}`}
                        onClick={() => {
                          setRemoving(null)
                          setRefusal(null)
                          setEditing({ was: kind.ending, ending: kind.ending, kind: kind.kind })
                        }}
                      >
                        Edit
                      </button>
                      {!kind.builtIn && (
                        <button
                          className="link"
                          aria-label={`Remove ${dotted(kind.ending)}…`}
                          onClick={() => {
                            setEditing(null)
                            setRemoving(kind.ending)
                          }}
                        >
                          Remove…
                        </button>
                      )}
                    </td>
                  </tr>
                ),
              )}
            </tbody>
          </table>
        </div>
      )}
      {editing?.was === '' && form('Add a kind of file')}
      <p className="choices">
        {editing?.was !== '' && (
          <button
            onClick={() => {
              setRemoving(null)
              setRefusal(null)
              setEditing({ was: '', ending: '', kind: 'photo' })
            }}
          >
            Add a kind of file…
          </button>
        )}
        <button
          onClick={() =>
            api
              .resetFileKinds()
              .then(done)
              .catch(onProblem)
          }
        >
          Put back the built-in list
        </button>
      </p>
    </div>
  )
}
