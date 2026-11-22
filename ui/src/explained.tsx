// A button and the one sentence that says what it does, laid out the way a
// row of Settings is: the sentence on the left, in a column of its own that
// wraps within itself, and the button beside it on the right. In a narrow
// window the button moves beneath the sentence, never into the middle of it.
//
// The button is described by its sentence, for a screen reader.
//
// The sentence is kept to a few words. What more there is to say about the
// button waits behind an "i" just after it (`tip`), shown when asked for.

import { cloneElement, isValidElement, useId } from 'react'
import type { ReactElement, ReactNode } from 'react'
import { InfoTip } from './infoTip'

export function Explained(props: {
  /** The sentence. */
  children: ReactNode
  /** The button (or buttons) it explains. A single element is described by the sentence. */
  action: ReactNode
  /** More about it, behind an "i" after the sentence. */
  tip?: ReactNode
  /** What the "i" is named after: "More about <tipAbout>". */
  tipAbout?: string
  className?: string
}) {
  const id = useId()
  // Only a plain element takes the description; a fragment of several is left as it is.
  const action =
    isValidElement(props.action) && typeof props.action.type === 'string'
    ? cloneElement(props.action as ReactElement<{ 'aria-describedby'?: string }>, { 'aria-describedby': id })
    : props.action
  return (
    <div className={props.className ? `explained ${props.className}` : 'explained'}>
      <p className="explained-said">
        <span id={id}>{props.children}</span>
        {props.tip && <InfoTip about={props.tipAbout ?? 'this'}>{props.tip}</InfoTip>}
      </p>
      <div className="explained-action">{action}</div>
    </div>
  )
}
