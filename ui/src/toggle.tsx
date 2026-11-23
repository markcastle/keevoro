import type { ReactNode } from 'react'

/**
 * A small on/off switch with its words beside it: for a setting that is
 * simply on or off. (Choosing several things out of a list, such as which
 * folders to look through, stays as ticks: that is what ticks are for.)
 *
 * `locked` leaves it looking as it is but refuses the change, for a choice
 * that cannot be switched off just now (the last kind a backup is for).
 *
 * On its own it is the small switch with its words beside it, centred on
 * each other and never in a box. As a `row` of a panel it is laid out as
 * every row of Settings is: the words on the left, the switch on the right.
 */
export function Toggle(props: {
  on: boolean
  label: ReactNode
  /** Said to a screen reader when the words beside it are not plain text. */
  name?: string
  disabled?: boolean
  locked?: boolean
  /** Heard when a locked switch is pressed, to say why it stays as it is. */
  onLocked?: () => void
  /** A row of a panel: the words on the left, the switch on the right. */
  row?: boolean
  onChange: (on: boolean) => void
}) {
  return (
    <label className={['kv-toggle', props.row && 'row', props.locked && 'locked'].filter(Boolean).join(' ')}>
      <button
        type="button"
        className={props.on ? 'kv-switch small on' : 'kv-switch small'}
        role="switch"
        aria-checked={props.on}
        aria-label={props.name}
        aria-disabled={props.locked || undefined}
        disabled={props.disabled}
        onClick={() => {
          if (!props.locked) props.onChange(!props.on)
          else props.onLocked?.()
        }}
      >
        <span className="knob" />
      </button>
      <span>{props.label}</span>
    </label>
  )
}
