// "More": one quiet button that opens a small menu of the choices used
// less often, so a row keeps only the few used most.
//
// It behaves as a menu does everywhere: the button says it opens one and
// whether it is open, the arrow keys, Home and End move between its items,
// Escape or Tab closes it and puts the focus back on the button, and a
// click anywhere else closes it. It is drawn on the page itself, fixed to
// the window beside its button, so a panel that scrolls or clips what is
// inside it never cuts it off.

import { useCallback, useEffect, useId, useLayoutEffect, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import { menuMove, menuPlace } from './menu'

export type MenuChoice = {
  /** The item's words, as the button it stands for had them. */
  label: string
  onChoose: () => void
  /** A line more about it, shown when the pointer rests on it. */
  title?: string
}

export function MoreMenu(props: { choices: MenuChoice[]; label?: string }) {
  const [open, setOpen] = useState(false)
  // Where the menu is drawn, once it has been measured.
  const [at, setAt] = useState<{ left: number; top: number } | null>(null)
  const button = useRef<HTMLButtonElement>(null)
  const menu = useRef<HTMLDivElement>(null)
  // Which item to put the focus on as it opens: the first, or the last.
  const focusFirst = useRef(true)
  const id = useId()

  const items = () => [...(menu.current?.querySelectorAll<HTMLButtonElement>('[role="menuitem"]') ?? [])]
  const close = useCallback((refocus: boolean) => {
    setOpen(false)
    setAt(null)
    if (refocus) button.current?.focus()
  }, [])
  const place = useCallback(() => {
    if (!button.current || !menu.current) return
    const box = menu.current.getBoundingClientRect()
    setAt(
      menuPlace(
        button.current.getBoundingClientRect(),
        { width: box.width, height: box.height },
        { width: document.documentElement.clientWidth, height: window.innerHeight },
      ),
    )
  }, [])

  // Measured and placed before it is seen; once it can be seen, the focus
  // goes into it (a hidden item cannot take the focus).
  useLayoutEffect(() => {
    if (open) place()
  }, [open, place])
  const placed = at !== null
  useLayoutEffect(() => {
    if (!open || !placed) return
    const all = items()
    all[focusFirst.current ? 0 : all.length - 1]?.focus()
  }, [open, placed])

  // Kept beside its button as the page scrolls or the window changes, and
  // closed by a click anywhere else.
  useEffect(() => {
    if (!open) return
    const away = (event: PointerEvent) => {
      const target = event.target as Node
      if (!menu.current?.contains(target) && !button.current?.contains(target)) close(false)
    }
    window.addEventListener('scroll', place, true)
    window.addEventListener('resize', place)
    document.addEventListener('pointerdown', away)
    return () => {
      window.removeEventListener('scroll', place, true)
      window.removeEventListener('resize', place)
      document.removeEventListener('pointerdown', away)
    }
  }, [open, place, close])

  const opening = (first: boolean) => {
    focusFirst.current = first
    setOpen(true)
  }

  return (
    <>
      <button
        ref={button}
        type="button"
        className="button more"
        aria-haspopup="menu"
        aria-expanded={open}
        aria-controls={open ? id : undefined}
        onClick={() => (open ? close(true) : opening(true))}
        onKeyDown={(e) => {
          if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
            e.preventDefault()
            opening(e.key === 'ArrowDown')
          }
        }}
      >
        {props.label ?? 'More'}
        <span aria-hidden="true">{open ? '▴' : '▾'}</span>
      </button>
      {open &&
        createPortal(
          <div
            ref={menu}
            id={id}
            className="more-menu"
            role="menu"
            aria-label={props.label ?? 'More'}
            style={at ? { left: at.left, top: at.top } : { left: 0, top: 0, visibility: 'hidden' }}
            onKeyDown={(e) => {
              if (e.key === 'Escape' || e.key === 'Tab') {
                e.preventDefault()
                close(true)
                return
              }
              const all = items()
              const next = menuMove(e.key, all.indexOf(document.activeElement as HTMLButtonElement), all.length)
              if (next !== null) {
                e.preventDefault()
                all[next].focus()
              }
            }}
          >
            {props.choices.map((choice) => (
              <button
                key={choice.label}
                type="button"
                role="menuitem"
                tabIndex={-1}
                title={choice.title}
                onClick={() => {
                  close(true)
                  choice.onChoose()
                }}
              >
                {choice.label}
              </button>
            ))}
          </div>,
          document.body,
        )}
    </>
  )
}
