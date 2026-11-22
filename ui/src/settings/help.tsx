// Help, How do I…: short, plain answers to what people most often want to
// do, each with a button that goes straight to the screen or setting that
// does it. Every answer is true of Keevoro as it is; one that would need a
// "not yet" says so rather than promising.
//
// And Help, Compare cloud storage providers: a few words about Stratus and
// a button that opens the same sheet "Where should they go?" opens.

import { useEffect, useState } from 'react'
import type { Backend, KeepRunningView } from '../backend'
import { CompareSheet } from '../compare'
import { gigabytesToAsk } from '../stratus'
import type { SetUp } from '../stratus'
import { Group, Intro, Row, RowButton } from './parts'

/** Where an answer's button goes: a step of the backup, or a row of Settings. */
export type Going = { step: 'find' } | { step: 'send'; adding?: boolean } | { row: string }

type Answer = { id: string; said: string; button?: string; going?: Going }

/** Closing the window, said for this computer: where its icon is, or that closing closes Keevoro. */
function whenClosed(view: KeepRunningView | null): string {
  if (!view) return 'Keevoro can keep running by the clock when its window is closed.'
  if (view.tray === 'none' || !view.keepRunning)
    return 'On this computer closing the window closes Keevoro. Open it again the way you opened it the first time.'
  return `Closing the window leaves Keevoro running by the clock. ${view.whereTheIconIs}`
}

export function HowToSection(props: { api: Backend; onGo: (going: Going) => void; onProblem: (error: unknown) => void }) {
  const { api, onProblem } = props
  const [closing, setClosing] = useState<KeepRunningView | null>(null)
  useEffect(() => {
    api.keepRunning().then(setClosing).catch(onProblem)
  }, [api, onProblem])

  const answers: Answer[] = [
    {
      id: 'howSource',
      said: 'On Find your photos, choose a drive, a card or a folder. Keevoro looks through it and adds what it finds to this backup.',
      button: 'Find your photos',
      going: { step: 'find' },
    },
    {
      id: 'howSecondPlace',
      said: 'On the backup screen, choose More, then Add another destination. A photo counts as safe only once it is at every one.',
      button: 'Add another destination',
      going: { step: 'send', adding: true },
    },
    {
      id: 'howByItself',
      said: 'Choose how often in Settings, Backing up by itself. While Keevoro is running, it looks through your sources for anything new and backs it up.',
      button: 'Set a schedule',
      going: { row: 'schedule' },
    },
    {
      id: 'howPluggedIn',
      said: 'On Find your photos, switch on “Back up when this is plugged in” under the drive or card. While Keevoro is running, it backs it up a few seconds after it is plugged in.',
      button: 'Find your photos',
      going: { step: 'find' },
    },
    {
      id: 'howCheck',
      said: 'On the backup screen, “Check they all arrived” looks for every file where it was sent. “Prove every copy…” reads each one back and compares it with its original.',
      button: 'Go to the backup screen',
      going: { step: 'send' },
    },
    {
      id: 'howWindowClosed',
      said: whenClosed(closing),
      button: 'Settings for closing',
      going: { row: 'keepRunning' },
    },
    {
      id: 'howLeaveOut',
      said: 'On Find your photos, choose Look inside or Where the space goes on a source, then Leave out beside the folder. Nothing in the folder is touched.',
      button: 'Find your photos',
      going: { step: 'find' },
    },
    {
      id: 'howKinds',
      said: 'Choose photos, videos or other files in Settings, This backup, What it backs up. A source can have its own choice under File types beside it.',
      button: 'What it backs up',
      going: { row: 'wants' },
    },
    {
      id: 'howNewComputer',
      said: 'Keevoro can’t move a backup to a new computer yet. The backup itself is ordinary files and folders, so you can copy it or open it anywhere, without Keevoro.',
    },
  ]

  return (
    <>
      <Intro>Short answers, each with a way straight there.</Intro>
      <Group>
        {answers.map((a) => (
          <Row
            key={a.id}
            id={a.id}
            about={a.said}
            control={a.button && a.going && <RowButton onClick={() => a.going && props.onGo(a.going)}>{a.button}</RowButton>}
          />
        ))}
      </Group>
    </>
  )
}

export function CompareSection(props: {
  api: Backend
  /** What is being kept, in bytes: the size the comparison is set to. */
  keptBytes: number
  /** Opens Keevoro's own form for a kind of storage; none before a backup is started. */
  onSetUp?: (setUp: SetUp) => void
  onProblem: (error: unknown) => void
}) {
  const [open, setOpen] = useState(false)
  const about = gigabytesToAsk(props.keptBytes).toLocaleString('en-GB')
  return (
    <>
      <Intro>
        Not sure where to buy cloud storage? Stratus, made by Adam Sparshott, compares what it costs, set to the size of
        this backup.
      </Intro>
      <Group
        footnote={`This loads a comparison page from the internet. Keevoro tells it only the size of this backup, about ${about} GB, and your currency. The prices are Stratus’s, not Keevoro’s.`}
      >
        <Row
          id="compare"
          title="Stratus"
          about={`What cloud storage for about ${about} GB costs, provider by provider.`}
          control={<RowButton onClick={() => setOpen(true)}>Compare providers</RowButton>}
        />
      </Group>
      {open && (
        <CompareSheet
          api={props.api}
          keptBytes={props.keptBytes}
          onClose={() => setOpen(false)}
          onSetUp={
            props.onSetUp &&
            ((setUp) => {
              setOpen(false)
              props.onSetUp?.(setUp)
            })
          }
          onProblem={props.onProblem}
        />
      )}
    </>
  )
}
