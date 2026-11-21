import type { Place } from './backend'

/**
 * A folder OneDrive, Dropbox, Google Drive or iCloud Drive keeps in step
 * with this computer, offered by the name the person knows, beside the
 * drives. Like a drive's card it is a plain button and never looks chosen.
 */
export function SyncedCard(props: { place: Place; added: boolean; onClick: () => void }) {
  const { place } = props
  return (
    <button className="place synced" onClick={props.onClick}>
      <svg width={44} height={44} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.75" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
        <path d="M7 18a4 4 0 0 1-.6-7.96A5.5 5.5 0 0 1 17 9.5a4.25 4.25 0 0 1 .5 8.5z" />
      </svg>
      <b>{place.name}</b>
      <span>{place.detail}</span>
      {props.added && <em>Looked through. Click to look again.</em>}
    </button>
  )
}
