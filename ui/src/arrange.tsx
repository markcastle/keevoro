// How a backup arranges photos and videos where it sends them: by date, or
// keeping the person's folders exactly as they are.
//
// Asked once, the first time a backup is set up, as one question with two
// large choices (a radio group, the accepted way to choose one of two),
// the one Keevoro suggests already chosen, and why it suggests it. It can
// be changed later in Settings, and for each source beside its kinds of
// file. Every sentence about what a change means comes from the API, so
// the screens and the command line say the same.

import { useEffect, useState, type ReactNode } from 'react'
import type { Arrange, ArrangeView, Collection, Source, SourceArrangeView } from './backend'

/** "“Family Scans”", "“Family Scans” and “Card”", for up to three folders. */
export function folderNames(tops: string[]): string {
  const quoted = [...new Set(tops)].slice(0, 3).map((t) => `“${t}”`)
  if (quoted.length <= 1) return quoted[0] ?? 'the folder you chose'
  return `${quoted.slice(0, -1).join(', ')} and ${quoted[quoted.length - 1]}`
}

/** One of the two large choices. */
function Choice(props: {
  value: Arrange
  chosen: Arrange
  suggested: boolean
  title: string
  children: ReactNode
  onChoose: (value: Arrange) => void
}) {
  const on = props.chosen === props.value
  return (
    <label className={on ? 'arrange-choice on' : 'arrange-choice'}>
      <input type="radio" name="arrange" value={props.value} checked={on} onChange={() => props.onChoose(props.value)} />
      <span className="arrange-text">
        <b>
          {props.title}
          {props.suggested && <span className="suggested">Suggested</span>}
        </b>
        <span className="quiet">{props.children}</span>
      </span>
    </label>
  )
}

/** The two choices and what each means, shared by the question and Settings. */
function Choices(props: { view: ArrangeView; chosen: Arrange; onChoose: (a: Arrange) => void; label: string }) {
  const { view, chosen } = props
  const suggested = view.suggestion?.arrange
  return (
    <div className="arrange-choices" role="radiogroup" aria-label={props.label}>
      <Choice value="byDate" chosen={chosen} suggested={suggested === 'byDate'} title="Arrange by date" onChoose={props.onChoose}>
        A folder for each year and month, by when each photo was taken. Folders you have named, such as Holidays, are
        kept.
      </Choice>
      <Choice
        value="folders"
        chosen={chosen}
        suggested={suggested === 'folders'}
        title="Keep my folders as they are"
        onChoose={props.onChoose}
      >
        The same folders and names as on your sources, starting with {folderNames(view.topFolders ?? [])}. Best for
        scans, and for folders you have already sorted.
      </Choice>
    </div>
  )
}

/**
 * The first-time question: asked once, before "Choose what to keep", on a
 * backup nobody has chosen for and that has sent nothing. The suggestion is
 * chosen to begin with; Next writes the choice.
 */
export function ArrangeQuestion(props: {
  view: ArrangeView
  choose: (arrange: Arrange) => Promise<ArrangeView>
  onBack: () => void
  onChosen: (view: ArrangeView) => void
  onProblem: (error: unknown) => void
}) {
  const { view } = props
  const [chosen, setChosen] = useState<Arrange>(view.suggestion?.arrange ?? 'byDate')
  const [busy, setBusy] = useState(false)
  return (
    <section className="arrange-question">
      <header>
        <h1>How should we arrange them in the backup?</h1>
      </header>
      <Choices view={view} chosen={chosen} onChoose={setChosen} label="How should we arrange them in the backup?" />
      {view.suggestion && <p className="arrange-why">{view.suggestion.said}</p>}
      {view.sourcesDiffer && <p className="quiet small">You can choose differently for each source later.</p>}
      <p className="quiet small">
        You can change this later, and for each source, in Settings. It only changes where files go from then on.
      </p>
      <div className="next">
        <button className="link" onClick={props.onBack}>
          ‹ Back
        </button>
        <span className="say" />
        <button
          className="primary big"
          disabled={busy}
          onClick={() => {
            setBusy(true)
            props
              .choose(chosen)
              .then(props.onChosen)
              .catch((e) => {
                setBusy(false)
                props.onProblem(e)
              })
          }}
        >
          Next
        </button>
      </div>
    </section>
  )
}

/** The same choice in Settings, for the whole backup. */
export function BackupArrange(props: {
  load: () => Promise<ArrangeView>
  choose: (arrange: Arrange) => Promise<ArrangeView>
  onChanged?: (view: ArrangeView) => void
  onProblem: (error: unknown) => void
}) {
  const { load, onProblem } = props
  const [view, setView] = useState<ArrangeView | null>(null)
  const [said, setSaid] = useState<string | null>(null)
  useEffect(() => {
    load().then(setView).catch(onProblem)
    // Asked once, when it is first shown.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])
  if (!view) return null
  return (
    <div className="backup-arrange">
      <Choices
        view={{ ...view, suggestion: undefined }}
        chosen={view.arrange}
        label="How files are arranged in the backup"
        onChoose={(arrange) =>
          arrange !== view.arrange &&
          props
            .choose(arrange)
            .then((changed) => {
              setView(changed)
              setSaid(changed.said)
              props.onChanged?.(changed)
            })
            .catch(onProblem)
        }
      />
      <p className="quiet small">
        Changing this only changes where files go from now on. Files already backed up stay where they are.
      </p>
      {said && (
        <p className="quiet small" role="status">
          {said}
        </p>
      )}
    </div>
  )
}

/** For one source: the same as its backup, or its own way, with what its files suggest. */
export function SourceArrange(props: {
  name: string
  load: () => Promise<SourceArrangeView>
  choose: (arrange: Arrange | 'backup') => Promise<SourceArrangeView>
  backup: Arrange
  onChanged?: () => void
  onProblem: (error: unknown) => void
}) {
  const { load, onProblem } = props
  const [view, setView] = useState<SourceArrangeView | null>(null)
  const [said, setSaid] = useState<string | null>(null)
  useEffect(() => {
    load().then(setView).catch(onProblem)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])
  if (!view) return null
  const choose = (arrange: Arrange | 'backup') =>
    props
      .choose(arrange)
      .then((changed) => {
        setView(changed)
        setSaid(changed.said)
        props.onChanged?.()
      })
      .catch(onProblem)
  const id = `arrange-${props.name.replace(/\W+/g, '-')}`
  const sameAs = `Same as the backup (${props.backup === 'folders' ? 'keep the folders' : 'by date'})`
  return (
    <div className="source-arrange">
      <label htmlFor={id}>Arrange files from {props.name}</label>
      <select id={id} value={view.own ? view.arrange : 'backup'} onChange={(e) => choose(e.target.value as Arrange | 'backup')}>
        <option value="backup">{sameAs}</option>
        <option value="byDate">By date</option>
        <option value="folders">Keep my folders as they are</option>
      </select>
      {view.suggestion && (
        <p className="quiet small suggestion">
          {view.suggestion.said}{' '}
          <button className="button small" onClick={() => choose(view.suggestion!.arrange)}>
            {view.suggestion.arrange === 'folders' ? 'Keep its folders' : 'Arrange it by date'}
          </button>
        </p>
      )}
      {said && (
        <p className="quiet small" role="status">
          {said}
        </p>
      )}
    </div>
  )
}

/**
 * On "Choose what to keep": folders kept as they are, a section for each
 * source, each folder a card in folder order, shown as it will be in the
 * backup. `card` draws one, as the grid above does.
 */
export function KeptFoldersSection(props: { folders: Collection[]; card: (c: Collection) => ReactNode }) {
  const bySource = new Map<string, Collection[]>()
  for (const c of props.folders) {
    const top = c.parents[0] ?? c.title
    bySource.set(top, [...(bySource.get(top) ?? []), c])
  }
  return (
    <>
      {[...bySource].map(([top, folders]) => (
        <section className="kept-folders" key={top} aria-label={`From ${top}, kept in your folders`}>
          <h2>From {top}, kept in your folders</h2>
          <p className="quiet small">Shown as they will be in the backup: the same folders, the same names.</p>
          <div className="grid">{folders.map((c) => props.card(c))}</div>
        </section>
      ))}
    </>
  )
}

/** The sentence the Choose screen adds when any folders are kept as they are. */
export function keptSaid(folders: Collection[]): string | null {
  const tops = [...new Set(folders.map((c) => c.parents[0] ?? c.title))]
  if (tops.length === 0) return null
  const names = tops.length === 1 ? tops[0] : `${tops.slice(0, -1).join(', ')} and ${tops[tops.length - 1]}`
  return `Files from ${names} are shown in their folders, as they will be kept.`
}

/**
 * The quiet line above "Back up now": "Arranged by date, except Family
 * Scans, which keeps its folders."
 */
export function arrangedLine(backup: Arrange, sources: Source[]): string {
  const keeps = (s: Source) => (s.arrange ?? backup) === 'folders'
  const differ = sources.filter((s) => keeps(s) !== (backup === 'folders')).map((s) => s.name)
  const list = (names: string[]) => (names.length === 1 ? names[0] : `${names.slice(0, -1).join(', ')} and ${names[names.length - 1]}`)
  if (backup === 'folders') {
    return differ.length === 0
      ? 'Your folders are kept as they are.'
      : `Your folders are kept as they are, except ${list(differ)}, which ${differ.length === 1 ? 'is' : 'are'} arranged by date.`
  }
  return differ.length === 0
    ? 'Arranged by date.'
    : `Arranged by date, except ${list(differ)}, which ${differ.length === 1 ? 'keeps its' : 'keep their'} folders.`
}
