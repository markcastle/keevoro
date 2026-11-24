// The green tick beside a line that says everything went well. It is only
// decoration for the words next to it, which say the same thing in full,
// so a screen reader skips it.
export function Tick() {
  return (
    <svg className="went-well-tick" viewBox="0 0 20 20" width="20" height="20" aria-hidden="true">
      <circle cx="10" cy="10" r="10" />
      <path d="M5.6 10.4l2.9 2.9 5.9-6.2" />
    </svg>
  )
}
