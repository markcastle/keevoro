// Help, About Keevoro: which version this is, with Check for updates (the
// same row as Settings, Updates), what Keevoro promises, what leaves this
// computer, who made it and who helped, and the terms it comes under. The
// open source work it is built on is a page of its own.
//
// The promises are the ones the code is built around and tested for;
// nothing is claimed here that the tests do not hold it to.

import type { Backend, Update } from '../backend'
import { Group, Intro, OpenRow, Row, RowButton } from './parts'
import type { DetailId } from './places'
import { VersionRow } from './updates'

/** Keevoro's home on the web: the source, the licence and the releases. */
const PROJECT = 'https://github.com/markcastle/keevoro'

const PROMISES: [string, string, string][] = [
  [
    'promises',
    'Your sources are only ever read',
    'Keevoro never changes, moves, renames or deletes anything on a drive, card or folder it looks through. Removing a source only takes it off a list. If a source is damaged or vanishes mid-way, nothing on it has been touched.',
  ],
  [
    'promise-exact',
    'A copy only counts when it is proved exact',
    'Every file is fingerprinted from its whole contents. An original written to a folder or a drive is read back from its place in the backup and compared with that fingerprint before Keevoro calls it safe, and one that does not match is never counted, and is said so. If a file of the same name is already there, it is fingerprinted too, and counted only if it is the same file. An original sent to Cloudflare R2, Amazon S3 or Google Cloud Storage travels over an encrypted, error-checked connection, and “Check they all arrived” confirms each one is there and the right size.',
  ],
  [
    'promise-kept',
    'No original in a backup is ever replaced or removed',
    'An original, once backed up, is never overwritten. Two different photos with the same name are both kept. Deleting a photo from a source does not delete it from your backup. Only what Keevoro makes itself, the previews and the list of contents, is ever rewritten. A copy found damaged is never deleted either: when you ask Keevoro to put the backup right, it is moved aside into a folder of its own there, with a note.',
  ],
  [
    'promise-duplicates',
    'Duplicates are exact, never guesses',
    'A file is left out as a duplicate only when every byte matches the copy being kept. Photos that merely look alike are both kept, and you can see every duplicate before anything is backed up.',
  ],
  [
    'promise-open',
    'Nothing is hidden from you',
    'Anything set aside is listed with the reason and can be brought back. Anything that could not be read is counted on screen and listed by name in the log, and anything that could not be backed up is named when the backup ends.',
  ],
  [
    'promise-yours',
    'You never need Keevoro to get your photos back',
    'A backup is ordinary files in ordinary folders, with a plain note inside explaining them. Copy the originals folder and you have everything.',
  ],
]

/** What leaves this computer, and what stays. Said once, here, word for word. */
export const LEAVES_THIS_COMPUTER =
  'You need no Keevoro account. Your files go from your source to the destination you chose and nowhere else. Storage keys are kept in this computer’s own keychain. The only thing Keevoro contacts by itself is GitHub, to see whether there is a newer version. If you choose to compare cloud storage providers, in Help or on the backup screen, it shows Stratus, a comparison of cloud storage prices on the internet, and tells it only the size of the backup and your currency. A report of a problem or an idea goes to Keevoro’s maker only when you send one, and you see exactly what it holds first. On this computer it keeps only its working notes: a list of what it found, small previews, and a log of what it did.'

export function AboutSection(props: {
  api: Backend
  version: string
  update: Update | null
  onUpdate: (update: Update | null) => void
  onOpen: (detail: DetailId) => void
  onProblem: (error: unknown) => void
}) {
  const { api, onProblem } = props
  const open = (page: string) => api.openReleasePage(page).catch(onProblem)
  return (
    <>
      <Intro>Keep things as they were.</Intro>

      <Group>
        <VersionRow
          id="aboutVersion"
          api={api}
          version={props.version}
          update={props.update}
          onUpdate={props.onUpdate}
          onProblem={onProblem}
        />
      </Group>

      <Group
        title="What Keevoro promises"
        className="kv-promises"
        footnote="What no program can promise is that a disk will not fail. Keep more than one copy of anything you cannot afford to lose, in more than one place, and use “Check they all arrived” now and then."
      >
        {PROMISES.map(([id, title, said]) => (
          <Row key={id} id={id} title={title} about={said} />
        ))}
      </Group>

      <Group title="What leaves this computer">
        <Row id="privacy" title="Your photos go only where you send them" about={LEAVES_THIS_COMPUTER} />
      </Group>

      <Group title="Thank you" className="thanks">
        <Row
          id="thanks"
          title="Keevoro is made by Mark Castle."
          about="A special thank you to Adam Sparshott, for all his help in testing Keevoro and for his excellent ideas. He found the faults that mattered most, and many of the things you see here began as his suggestions: keeping your folders as they are, backing up other kinds of file, cloud drives as sources, managing your backups, and a calmer map of where the space goes."
        />
        <OpenRow
          id="licences"
          about="Keevoro is also built on the work of many open source projects, each listed with its licence."
          onOpen={() => props.onOpen('licences')}
        />
      </Group>

      <Group title="The name">
        <Row
          id="aboutName"
          title="Where the name comes from"
          about="The name blends the sound of English keep with voro, an old Swedish word meaning “were”. Together they capture why we save photos and videos in the first place: to remember people and places as they were."
        />
      </Group>

      <Group
        title="Licence"
        footnote="That is a summary; the licence itself is what counts. The Keevoro name and logo are not covered by it, and it comes with no warranty."
      >
        <Row
          id="licence"
          about="Copyright 2026 Mark Castle. Keevoro’s source is open to read, use, change and share under the Functional Source License (FSL-1.1-MIT). You may use it freely, at home or at work. You may not sell it or offer it as a product that competes with Keevoro. Each version becomes plain MIT two years after it comes out."
          control={<RowButton onClick={() => open(`${PROJECT}/blob/main/LICENSE.md`)}>Read the licence</RowButton>}
          below={
            <p className="choices">
              <button type="button" className="button small" onClick={() => open(`${PROJECT}/blob/main/TRADEMARKS.md`)}>
                The name and logo
              </button>
              <button type="button" className="button small" onClick={() => open(PROJECT)}>
                See the source
              </button>
            </p>
          }
        />
      </Group>
    </>
  )
}
