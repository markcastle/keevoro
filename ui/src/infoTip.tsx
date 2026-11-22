// Detail on demand: the two ways a screen keeps its explanations without
// showing them all at once.
//
//   InfoTip    a small "i" beside a label or a button. Resting the pointer
//              on it, moving the keyboard focus to it, or clicking or
//              tapping it shows a few sentences in a small box. Escape, or
//              a click anywhere else, puts it away and leaves the focus on
//              the "i". The box is drawn fixed to the window beside its
//              button, so a panel that scrolls or clips what is inside it
//              never cuts it off. Its words can be selected and copied.
//
//   MoreAbout  a quiet "More about this" that opens in place, for anything
//              longer: several sentences, a list, steps.
//
// The "i" is always there to be seen, in the same place on every row, not
// only when the pointer happens to be over it, so a new user knows there is
// more to read and a touch screen can reach it.

import { useCallback, useEffect, useId, useLayoutEffect, useRef, useState } from 'react'
import type { PointerEvent as ReactPointerEvent, ReactNode } from 'react'
import { tipPlace } from './tipPlace'

/** How long the pointer may take to cross from the "i" into its box, in milliseconds. */
const CROSSING = 200

export function InfoTip(props: {
  /** What it explains, for its name: "More about <about>". */
  about: string
  /** One to three sentences. */
  children: ReactNode
  className?: string
}) {
  const id = useId()
  const button = useRef<HTMLButtonElement>(null)
  const box = useRef<HTMLSpanElement>(null)
  // Three ways it can be open: the pointer resting on it, the keyboard
  // focus on it, or a click or tap that keeps it open until put away.
  const [hovered, setHovered] = useState(false)
  const [focused, setFocused] = useState(false)
  const [pinned, setPinned] = useState(false)
  const open = hovered || focused || pinned
  const [at, setAt] = useState<{ left: number; top: number } | null>(null)
  const leaving = useRef<number | null>(null)
  // Set while the focus is put back on the "i" as it closes, so that does not open it again.
  const returning = useRef(false)

  const close = useCallback((refocus: boolean) => {
    setHovered(false)
    setFocused(false)
    setPinned(false)
    setAt(null)
    if (refocus && document.activeElement !== button.current) {
      returning.current = true
      button.current?.focus({ preventScroll: true })
      returning.current = false
    }
  }, [])

  const place = useCallback(() => {
    if (!button.current || !box.current) return
    const size = box.current.getBoundingClientRect()
    const placed = tipPlace(
      button.current.getBoundingClientRect(),
      { width: size.width, height: size.height },
      { width: document.documentElement.clientWidth, height: window.innerHeight },
    )
    setAt({ left: placed.left, top: placed.top })
  }, [])

  useLayoutEffect(() => {
    // Measured and placed before it is drawn.
    if (open) place()
  }, [open, place])

  // Kept beside its button as the page scrolls or the window changes; put
  // away by Escape or a click anywhere else.
  useEffect(() => {
    if (!open) return
    const away = (event: PointerEvent) => {
      const target = event.target as Node
      if (!box.current?.contains(target) && !button.current?.contains(target)) {
        setPinned(false)
        setHovered(false)
      }
    }
    const escape = (event: KeyboardEvent) => {
      if (event.key !== 'Escape') return
      event.stopPropagation()
      // The focus goes back to the "i" if it was in the box; the box stays
      // shut until it is asked for again.
      close(!!box.current?.contains(document.activeElement))
    }
    window.addEventListener('scroll', place, true)
    window.addEventListener('resize', place)
    document.addEventListener('pointerdown', away)
    document.addEventListener('keydown', escape, true)
    return () => {
      window.removeEventListener('scroll', place, true)
      window.removeEventListener('resize', place)
      document.removeEventListener('pointerdown', away)
      document.removeEventListener('keydown', escape, true)
    }
  }, [open, place, close])

  useEffect(
    () => () => {
      if (leaving.current !== null) window.clearTimeout(leaving.current)
    },
    [],
  )
  const stay = () => {
    if (leaving.current !== null) window.clearTimeout(leaving.current)
    leaving.current = null
  }
  const pointerIn = (event: ReactPointerEvent) => {
    if (event.pointerType !== 'mouse') return
    stay()
    setHovered(true)
  }
  const pointerOut = (event: ReactPointerEvent) => {
    if (event.pointerType !== 'mouse') return
    stay()
    leaving.current = window.setTimeout(() => setHovered(false), CROSSING)
  }

  return (
    <span className={props.className ? `info-tip ${props.className}` : 'info-tip'}>
      <button
        ref={button}
        type="button"
        className="info-tip-button"
        aria-label={`More about ${props.about}`}
        aria-expanded={open}
        aria-describedby={open ? id : undefined}
        onPointerEnter={pointerIn}
        onPointerLeave={pointerOut}
        onFocus={(e) => {
          // Only the keyboard opens it by focus; a click or tap opens it by itself.
          if (!returning.current && e.currentTarget.matches(':focus-visible')) setFocused(true)
        }}
        onBlur={(e) => {
          if (!box.current?.contains(e.relatedTarget as Node)) setFocused(false)
        }}
        onClick={(e) => {
          // A click inside a label or a summary is the "i"'s alone.
          e.preventDefault()
          e.stopPropagation()
          if (pinned) close(false)
          else setPinned(true)
        }}
      >
        {/* The "i" itself is drawn by the style sheet, so it is never read as a word of the sentence beside it. */}
      </button>
      {open && (
        <span
          ref={box}
          id={id}
          role="tooltip"
          className="info-tip-box"
          style={at ? { left: at.left, top: at.top } : { left: 0, top: 0, visibility: 'hidden' }}
          onPointerEnter={pointerIn}
          onPointerLeave={pointerOut}
          onClick={(e) => e.stopPropagation()}
          onBlur={(e) => {
            if (e.relatedTarget !== button.current && !box.current?.contains(e.relatedTarget as Node)) setFocused(false)
          }}
        >
          {props.children}
        </span>
      )}
    </span>
  )
}

/** A quiet disclosure that opens in place, for the longer explanation of a screen or a group. */
export function MoreAbout(props: { children: ReactNode; label?: string; className?: string }) {
  return (
    <details className={props.className ? `more-about ${props.className}` : 'more-about'}>
      <summary>{props.label ?? 'More about this'}</summary>
      <div className="more-about-body">{props.children}</div>
    </details>
  )
}
