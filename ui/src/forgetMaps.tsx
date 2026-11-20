// Forgetting the remembered maps, in Settings under Advanced. A map lists
// folder names across a whole drive, so a person may want it gone; nothing
// on any drive is touched, and a place is simply surveyed again when next
// asked about. Reset forgets them too.

import { useState } from 'react'
import type { Backend } from './backend'
import { count } from './format'

export function ForgetMapsSetting(props: { api: Backend; onProblem: (error: unknown) => void }) {
  const [said, setSaid] = useState<string | null>(null)
  const forget = () =>
    props.api
      .forgetMaps()
      .then(({ forgotten }) =>
        setSaid(forgotten === 0 ? 'There were no remembered maps.' : `Forgot ${count(forgotten, 'remembered map')}.`),
      )
      .catch(props.onProblem)
  return (
    <div className="setting">
      <h3>Remembered maps</h3>
      <p className="quiet">
        So that a look inside a folder, and where the space goes, open at once, Keevoro remembers what it saw when it
        found where the photos are: the names of the folders, and how much each holds. It is kept only on this computer.
        Nothing on any drive is touched by forgetting it; a place is simply looked at again when you next ask.
      </p>
      <p className="choices">
        <button onClick={forget}>Forget remembered maps</button>
      </p>
      {said && (
        <p className="quiet small" role="status">
          {said}
        </p>
      )}
    </div>
  )
}
