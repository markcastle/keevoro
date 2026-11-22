import type { ReactNode } from 'react'

/**
 * A small on/off switch with its words beside it: for a setting that is
 * simply on or off. (Choosing several things out of a list, such as which
 * folders to look through, stays as ticks: that is what ticks are for.)
 *
 * `locked` leaves it looking as it is but refuses the change, for a choice
 * that cannot be switched off just now (the last kind a backup is for).
 */
export function Toggle(props: {
  on: boolean
  label: ReactNode
  /** Said to a screen reader when the words beside it are not plain text. */
  name?: string
  disabled?: boolean
  locked?: boolean
  onChange: (on: boolean) => void
}) {
  return (
    <label className={props.locked ? 'toggle locked' : 'toggle'}>
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
        }}
      >
        <span className="knob" />
      </button>
      <span>{props.label}</span>
    </label>
  )
}
