// Settings, Appearance: light, dark, or the same as the rest of the
// computer. Kept on this computer only (theme.ts), never in a backup.

import { useState } from 'react'
import { chooseTheme, savedTheme } from '../theme'
import type { Theme } from '../theme'
import { Group, Row } from './parts'

const THEMES: [Theme, string][] = [
  ['system', 'Match my computer'],
  ['light', 'Light'],
  ['dark', 'Dark'],
]

export function AppearanceSection() {
  const [theme, setTheme] = useState<Theme>(savedTheme)
  return (
    <Group>
      <Row
        id="theme"
        className="kv-row-wide-control"
        control={
          <span className="themes" role="group" aria-label="Appearance">
            {THEMES.map(([value, label]) => (
              <button
                key={value}
                type="button"
                aria-pressed={theme === value}
                onClick={() => {
                  chooseTheme(value)
                  setTheme(value)
                }}
              >
                {label}
              </button>
            ))}
          </span>
        }
      />
    </Group>
  )
}
