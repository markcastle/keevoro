// A small ring above the map: what takes the room in the folder shown, by
// kind, with the figures beside it in words. Our first outside tester asked
// for "a choice of views, like a pie chart"; the ring gives the share at a
// glance, and the words give it exactly.
//
// The ring itself is a picture a screen reader passes over; the figures
// are a real list, and each is a button that picks that kind, fading every
// box on the map without it, exactly as "By kind of file" does.

import type { MapKind, MapNode } from './backend'
import { share, size } from './format'
import { KIND_NAMES } from './mapWords'
import { ringParts } from './ringParts'

/** The ring's radius and thickness, in the units of its picture. */
const R = 26
const THICK = 12
const ROUND = 2 * Math.PI * R

export function SummaryRing(props: { top: MapNode; picked: MapKind | null; onPick: (kind: MapKind | null) => void }) {
  const parts = ringParts(props.top)
  if (parts.length === 0) return null
  return (
    <figure className="summary-ring">
      <svg viewBox="0 0 64 64" aria-hidden className="ring">
        <defs>
          <pattern id="ring-stripes" width="4" height="4" patternUnits="userSpaceOnUse" patternTransform="rotate(45)">
            <rect width="4" height="4" className="ring-video" />
            <line x1="0" y1="0" x2="0" y2="4" className="ring-video-line" />
          </pattern>
          <pattern id="ring-rows" width="4" height="4" patternUnits="userSpaceOnUse">
            <rect width="4" height="4" className="ring-carried" />
            <line x1="0" y1="2" x2="4" y2="2" className="ring-carried-line" />
          </pattern>
        </defs>
        <circle cx="32" cy="32" r={R} className="ring-ground" strokeWidth={THICK} fill="none" />
        {parts.map((p) => {
          const length = ((p.to - p.from) / 360) * ROUND
          return (
            <circle
              key={p.kind}
              cx="32"
              cy="32"
              r={R}
              fill="none"
              strokeWidth={THICK}
              className={`ring-part ${p.kind}${props.picked && props.picked !== p.kind ? ' faded' : ''}`}
              strokeDasharray={`${length} ${ROUND - length}`}
              strokeDashoffset={-((p.from / 360) * ROUND)}
              transform="rotate(-90 32 32)"
            />
          )
        })}
      </svg>
      <figcaption>
        <ul aria-label={`What takes the room in ${props.top.name}`}>
          {parts.map((p) => (
            <li key={p.kind}>
              <button
                className="link"
                aria-pressed={props.picked === p.kind}
                onClick={() => props.onPick(props.picked === p.kind ? null : p.kind)}
              >
                <span className={`swatch ${p.kind}`} aria-hidden />
                {KIND_NAMES[p.kind]}
              </button>
              <span className="figure">{size(p.bytes)}</span>
              <span className="figure">{share(p.bytes, props.top.bytes)}</span>
            </li>
          ))}
        </ul>
      </figcaption>
    </figure>
  )
}
