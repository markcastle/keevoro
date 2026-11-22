// Settings, Help, What Keevoro has been doing: the last few hundred lines
// of Keevoro's plain record of what it did and when, newest at the bottom,
// and a way to open the whole file.

import { useCallback, useEffect, useRef, useState } from 'react'
import type { Backend } from '../backend'
import { Intro } from './parts'

export function LogPage(props: { api: Backend; onProblem: (error: unknown) => void }) {
  const { api, onProblem } = props
  const [log, setLog] = useState<string | null>(null)
  const box = useRef<HTMLPreElement>(null)
  const read = useCallback(
    () =>
      api
        .logTail()
        .then((text) => {
          setLog(text)
          // The newest lines are at the bottom, which is what people want.
          requestAnimationFrame(() => box.current?.scrollTo(0, box.current.scrollHeight))
        })
        .catch(onProblem),
    [api, onProblem],
  )
  useEffect(() => {
    void read()
  }, [read])

  return (
    <>
      <Intro>
        Every search, anything Keevoro could not read, and every backup, with when. It never records keys or passwords.
        If something seems slow or wrong, this is where to look.
      </Intro>
      <p className="choices">
        <button type="button" className="button small" onClick={() => void read()}>
          Refresh
        </button>
        <button type="button" className="button small" onClick={() => api.openLog().catch(onProblem)}>
          Open the log file
        </button>
      </p>
      <pre className="log" ref={box} tabIndex={0} aria-label="The log, newest at the bottom">
        {log === null ? '' : log || 'Nothing has been recorded yet.'}
      </pre>
    </>
  )
}
