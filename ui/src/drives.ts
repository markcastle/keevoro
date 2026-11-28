// The drives plugged into this computer, kept up to date while a screen
// that shows them is in view. Drives come and go: the list is looked at
// when the window is returned to, when the system says one came or went,
// and every couple of seconds while the drives are on screen and the
// window can be seen. Each look asks only whether the list the system
// keeps has changed, and lists the drives in full only when it has. Used
// by the first step (where to look) and the last (where to back up to), so
// both react the same way to a card or drive being plugged in or pulled out.

import { useCallback, useEffect, useRef, useState } from 'react'
import type { Backend, Place } from './backend'

/** How long a drive that has just arrived is brought to notice, in milliseconds. */
export const ARRIVED_FOR = 8000
/** How often the list of drives is asked after while it is on screen, in milliseconds. */
export const LOOK_FOR_DRIVES_EVERY = 2000

export function useDrives(api: Backend, showing: boolean, onProblem: (error: unknown) => void) {
  const [places, setPlaces] = useState<Place[] | null>(null)
  // The quick list of drives last given, to ask whether anything changed.
  const mounts = useRef<string[] | null>(null)
  // Drives that have just arrived, brought to notice for a little while.
  const [arrived, setArrived] = useState<Set<string>>(new Set())
  const look = useCallback(
    (asked: boolean) => {
      api
        .placesIfChanged(mounts.current, asked)
        .then((now) => {
          if (!now) return
          mounts.current = now.mounts
          setPlaces(now.places)
          if (now.arrived.length > 0) setArrived(new Set(now.arrived))
        })
        .catch(onProblem)
    },
    [api, onProblem],
  )
  useEffect(() => {
    if (arrived.size === 0) return
    const timer = window.setTimeout(() => setArrived(new Set()), ARRIVED_FOR)
    return () => window.clearTimeout(timer)
  }, [arrived])
  useEffect(() => {
    look(false)
    const again = () => look(true)
    window.addEventListener('focus', again)
    const stopHearing = api.onDrives(again)
    return () => {
      window.removeEventListener('focus', again)
      stopHearing()
    }
  }, [api, look])
  useEffect(() => {
    if (!showing) return
    const timer = window.setInterval(() => {
      if (document.visibilityState === 'visible') look(false)
    }, LOOK_FOR_DRIVES_EVERY)
    return () => window.clearInterval(timer)
  }, [showing, look])
  return { places, arrived, look }
}

export { onDrive } from './drivesWords'
