// Settings, Advanced, Kinds of file: the endings Keevoro looks for, for
// the whole of Keevoro. A built-in kind can be switched off and made a
// photo, a video or an other file; one of your own can also be renamed and
// removed. Any ending can be added as an other file (music, documents),
// carried along and backed up exactly, but never shown or grouped by date.
// A change counts from the next time a source is looked through, and never
// takes away anything already found or backed up.
//
// The usual kinds are listed; the less common ones (looked for just the
// same) wait behind one switch, remembered on this computer, and a search
// finds any of them. "Undo my changes…" says exactly what it would undo
// before it does anything, and can be taken back while the page is open.

import { useCallback, useEffect, useState } from 'react'
import type { Backend, FileKind, ListedAs } from '../backend'
import { InfoTip, MoreAbout } from '../infoTip'
import { Toggle } from '../toggle'
import { Intro } from './parts'

/** What a refusal from Keevoro says, as a sentence. */
function sentence(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}

/** An ending as it is shown: with its dot. */
const dotted = (ending: string) => `.${ending}`
const endings = (list: FileKind[]) => list.map((k) => dotted(k.ending)).join(', ')
const word = (kind: ListedAs) => (kind === 'photo' ? 'a photo' : kind === 'video' ? 'a video' : 'an other file')
const kinds = (n: number) => (n === 1 ? '1 kind' : `${n} kinds`)

// Whether the less common kinds are listed: only how the page draws, so
// remembered on this computer like the theme, not in the settings.
const LESS_COMMON = 'keevoro-less-common-kinds'
function savedLessCommon(): boolean {
  try {
    return localStorage.getItem(LESS_COMMON) === 'shown'
  } catch {
    return false
  }
}
function saveLessCommon(shown: boolean) {
  try {
    if (shown) localStorage.setItem(LESS_COMMON, 'shown')
    else localStorage.removeItem(LESS_COMMON)
  } catch {
    // It still applies for now; it just will not be remembered.
  }
}

/** A built-in kind the person changed from how it came. */
const changed = (k: FileKind) => k.builtIn && k.cameAs !== undefined && (k.kind !== k.cameAs || k.on === k.unsure)

/** What undoing the person's changes would do, line by line, in their words. */
function whatUndoes(list: FileKind[]): string[] {
  const own = list.filter((k) => !k.builtIn)
  const mine = list.filter(changed)
  const madeOther = mine.filter((k) => k.kind !== k.cameAs)
  const switchedOff = mine.filter((k) => k.kind === k.cameAs && !k.on)
  const switchedOn = mine.filter((k) => k.kind === k.cameAs && k.on)
  const said: string[] = []
  if (own.length) said.push(`${kinds(own.length)} you added will be taken off the list: ${endings(own)}.`)
  if (switchedOff.length)
    said.push(`${kinds(switchedOff.length)} you switched off will be switched back on: ${endings(switchedOff)}.`)
  if (switchedOn.length)
    said.push(`${kinds(switchedOn.length)} you switched on will be switched off again: ${endings(switchedOn)}.`)
  if (madeOther.length)
    said.push(
      `${kinds(madeOther.length)} you changed will be as Keevoro comes with them: ${madeOther
        .map(
          (k) =>
            `${dotted(k.ending)} ${word(k.cameAs ?? k.kind)} again${k.on === k.unsure ? (k.unsure ? ' and off' : ' and on') : ''}`,
        )
        .join(', ')}.`,
    )
  return said
}

export function FileKindsPage(props: {
  api: Backend
  onSources: () => void
  onChanged: () => void
  onProblem: (error: unknown) => void
}) {
  const { api, onProblem } = props
  const [list, setList] = useState<FileKind[] | null>(null)
  // Which line is being edited ('' for a new one), with what is typed.
  const [editing, setEditing] = useState<{ was: string; ending: string; kind: ListedAs } | null>(null)
  const [refusal, setRefusal] = useState<string | null>(null)
  const [removing, setRemoving] = useState<string | null>(null)
  const [changedHere, setChangedHere] = useState(false)
  const [lessCommon, setLessCommon] = useState(savedLessCommon)
  const [typed, setTyped] = useState('')
  const [undoing, setUndoing] = useState(false)
  // The list as it was before "Undo my changes", to put back while the page is open.
  const [undone, setUndone] = useState<FileKind[] | null>(null)
  const load = useCallback(() => api.fileKinds().then(setList).catch(onProblem), [api, onProblem])
  useEffect(() => {
    void load()
  }, [load])

  const done = (keepUndone = false) => {
    setChangedHere(true)
    if (!keepUndone) setUndone(null)
    props.onChanged()
    setEditing(null)
    setRefusal(null)
    setRemoving(null)
    setUndoing(false)
    return load()
  }
  const save = (kind = editing?.kind) => {
    if (!editing || !kind) return
    const asked =
      editing.was === ''
        ? api.addFileKind(editing.ending, kind)
        : api.changeFileKind(editing.was, { kind, to: editing.ending })
    asked.then(() => done()).catch((e) => setRefusal(sentence(e)))
  }
  const switchOn = (kind: FileKind) =>
    api
      .changeFileKind(kind.ending, { on: !kind.on })
      .then(() => done())
      .catch(onProblem)

  // Undo: every built-in kind back as it came, and the person's own taken
  // off. What it will do is said before, in place.
  const undo = async (before: FileKind[]) => {
    await api.resetFileKinds()
    for (const own of before.filter((k) => !k.builtIn)) await api.removeFileKind(own.ending)
    setUndone(before)
    await done(true)
  }
  const putBack = async (before: FileKind[]) => {
    for (const own of before.filter((k) => !k.builtIn)) {
      await api.addFileKind(own.ending, own.kind)
      if (!own.on) await api.changeFileKind(own.ending, { on: false })
    }
    for (const mine of before.filter(changed)) await api.changeFileKind(mine.ending, { kind: mine.kind, on: mine.on })
    await done()
  }

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
            disabled={editing.was !== '' && list?.find((k) => k.ending === editing.was)?.builtIn}
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

  const sought = typed.trim().replace(/^\./, '').toLowerCase()
  // Searching finds any kind, usual or not; otherwise the usual ones, your
  // own and any you changed, or every one with the switch on.
  const shown = (list ?? []).filter((k) =>
    sought ? k.ending.includes(sought) : lessCommon || k.usual || !k.builtIn || changed(k),
  )
  const usualCount = (list ?? []).filter((k) => k.builtIn && k.usual).length
  const lessCommonCount = (list ?? []).filter((k) => k.builtIn && !k.usual).length
  const undoes = list ? whatUndoes(list) : []
  const takesOwn = (list ?? []).some((k) => !k.builtIn)

  return (
    <div className="kinds">
      <Intro>Add a kind your camera makes that is missing here. One list serves every backup.</Intro>
      <MoreAbout className="kv-more">
        <p>
          Keevoro finds photos and videos by the ending of their names, such as .jpg, and for the kinds it knows, by
          looking inside too. The less common kinds are looked for just the same as the usual ones; they are only
          left out of the list until you ask to see them.
        </p>
        <p>
          A kind Keevoro cannot open is backed up just the same, exactly as it is, with “No preview” where its picture
          would be. Changes count from the next time a source is looked through. Switching a kind off or removing it
          never takes away anything already found or backed up.
        </p>
      </MoreAbout>
      <div className="kinds-find">
        <input
          type="search"
          value={typed}
          placeholder="Find an ending, such as tga"
          aria-label="Find a kind of file"
          spellCheck={false}
          onChange={(e) => setTyped(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Escape' && typed) {
              e.preventDefault()
              setTyped('')
            }
          }}
        />
        <Toggle
          on={lessCommon}
          label="Show less common kinds"
          onChange={(on) => {
            setLessCommon(on)
            saveLessCommon(on)
          }}
        />
        {list && (
          <span className="quiet small kinds-counts">
            {usualCount} usual kinds · {lessCommonCount} less common
          </span>
        )}
      </div>
      <p className="choices again" role="status">
        {changedHere && !undone && (
          <>
            <span className="quiet small">Changed. Look through your sources again to find files of the new kinds.</span>
            <button className="button small" onClick={props.onSources}>
              Go to your sources
            </button>
          </>
        )}
        {undone && (
          <>
            <span className="quiet small">Your changes are undone.</span>
            <button className="button small" onClick={() => putBack(undone).catch(onProblem)}>
              Put my changes back
            </button>
          </>
        )}
      </p>
      {list && shown.length === 0 && (
        <p className="quiet kinds-none">
          No kind ending {dotted(sought)} is on the list. You can add it with “Add a kind of file…”.
        </p>
      )}
      {list && shown.length > 0 && (
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
              {shown.map((kind) =>
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
                              .then(() => done())
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
                    <td className="ending">
                      <span>{dotted(kind.ending)}</span>
                      {kind.unsure && (
                        <InfoTip about={dotted(kind.ending)}>
                          Often not a photo or video, so it is off. While off, a file like this is still kept if Keevoro
                          can tell from inside that it is one. Switch it on if yours are photos or videos.
                        </InfoTip>
                      )}
                    </td>
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
      {editing?.was !== '' && (
        <p className="choices">
          <button
            onClick={() => {
              setRemoving(null)
              setRefusal(null)
              setEditing({ was: '', ending: '', kind: 'photo' })
            }}
          >
            Add a kind of file…
          </button>
        </p>
      )}
      {list && undoes.length > 0 && !undoing && (
        <p className="kinds-undo">
          <button className="link" onClick={() => setUndoing(true)}>
            Undo my changes…
          </button>
          <InfoTip about="undoing your changes">
            Puts the list back as Keevoro comes with it. You see exactly what will change before anything does, and
            nothing already found or backed up is touched.
          </InfoTip>
        </p>
      )}
      {list && undoing && (
        <div className="kinds-undo asking" role="alertdialog" aria-label="Undo my changes?">
          <p>This puts the list back as Keevoro comes with it:</p>
          <ul>
            {undoes.map((line) => (
              <li key={line}>{line}</li>
            ))}
          </ul>
          <p className="quiet small">Nothing already found or backed up is touched.</p>
          <span className="choices">
            <button className={takesOwn ? 'danger' : 'primary'} onClick={() => undo(list).catch(onProblem)}>
              Undo my changes
            </button>
            <button onClick={() => setUndoing(false)}>Cancel</button>
          </span>
        </div>
      )}
    </div>
  )
}
