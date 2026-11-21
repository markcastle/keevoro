// Where the space goes, by kind of file: photos, videos and everything
// else in the folder shown, each opening to its endings (jpg, mp4, zip)
// with how many files and how much room. Picking a kind or an ending fades
// every box on the map that holds none of it, as WinDirStat does, so a
// person can see where the zip files or the screen recordings are.
//
// The words are the command line's (`keevoro map --kinds`), from mapWords.

import { useState } from 'react'
import type { MapView } from './backend'
import { count, size } from './format'
import { endingName, KIND_NAMES } from './mapWords'
import type { Picked } from './mapWords'

const KINDS = ['photo', 'video', 'carried', 'other'] as const

export function KindsList(props: { view: MapView; picked: Picked | null; onPick: (picked: Picked | null) => void }) {
  const { view, picked, onPick } = props
  const top = view.top
  const [open, setOpen] = useState<Set<string>>(() => new Set())
  const of = {
    photo: [top.photos, top.photoBytes],
    video: [top.videos, top.videoBytes],
    carried: [top.carried ?? 0, top.carriedBytes ?? 0],
    other: [top.others, top.otherBytes],
  }
  const same = (a: Picked | null, b: Picked) => a !== null && a.kind === b.kind && a.ending === b.ending
  const toggle = (kind: string) => {
    const now = new Set(open)
    if (now.has(kind)) now.delete(kind)
    else now.add(kind)
    setOpen(now)
  }
  const shown = picked
    ? picked.ending !== undefined
      ? `${endingName(view.kinds[picked.ending]?.ending ?? '')} files`
      : KIND_NAMES[picked.kind].toLowerCase()
    : null
  return (
    <div className="space-list kinds-list">
      <table aria-label={`What is in ${top.name}, by kind of file`}>
        <thead>
          <tr>
            <th scope="col">Kind of file</th>
            <th scope="col" className="number">
              Files
            </th>
            <th scope="col" className="number">
              Size
            </th>
          </tr>
        </thead>
        <tbody>
          {KINDS.map((kind) => {
            const [files, bytes] = of[kind]
            if (files === 0) return null
            const endings = view.kinds.map((k, at) => ({ ...k, at })).filter((k) => k.kind === kind)
            const opened = open.has(kind)
            const group: Picked = { kind }
            return [
              <tr key={kind} className={`kind-group ${kind}`} aria-selected={same(picked, group)}>
                <td className="name">
                  <button
                    className="link disclosure"
                    aria-expanded={opened}
                    aria-label={`${opened ? 'Hide' : 'Show'} the endings of ${KIND_NAMES[kind].toLowerCase()}`}
                    onClick={() => toggle(kind)}
                  >
                    <span aria-hidden>{opened ? '▾' : '▸'}</span>
                  </button>{' '}
                  <span className={`swatch ${kind}`} aria-hidden />{' '}
                  <button className="link" aria-pressed={same(picked, group)} onClick={() => onPick(same(picked, group) ? null : group)}>
                    {KIND_NAMES[kind]}
                  </button>
                </td>
                <td className="number">{files.toLocaleString('en-GB')}</td>
                <td className="number">{size(bytes)}</td>
              </tr>,
              ...(opened
                ? endings.map((e) => {
                    const one: Picked = { kind, ending: e.at }
                    return (
                      <tr key={`${kind}.${e.at}`} className="ending" aria-selected={same(picked, one)}>
                        <td className="name">
                          <button className="link" aria-pressed={same(picked, one)} onClick={() => onPick(same(picked, one) ? null : one)}>
                            {endingName(e.ending)}
                          </button>
                          {e.leftOut && <span className="note small">Not backed up: {e.leftOut}</span>}
                        </td>
                        <td className="number">{e.files.toLocaleString('en-GB')}</td>
                        <td className="number">{size(e.bytes)}</td>
                      </tr>
                    )
                  })
                : []),
            ]
          })}
        </tbody>
      </table>
      {view.kindsShort && (
        <p className="quiet small">Some folders’ endings were not kept, so the endings add up to less.</p>
      )}
      <p className="quiet small picked" aria-live="polite">
        {shown ? (
          <>
            Only the boxes with {shown} are bright on the map.{' '}
            <button className="link" onClick={() => onPick(null)}>
              Show everything
            </button>
          </>
        ) : (
          `Choose a kind of file, or one ending, to see where it is on the map. ${count(view.kinds.length, 'ending')} here.`
        )}
      </p>
    </div>
  )
}
