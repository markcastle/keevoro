// Light, dark, or whatever the computer is set to.
//
// The choice is remembered on this computer and put on the page as a
// `data-theme` attribute, which is what tokens.css looks at. "system" means
// no attribute at all, so the page simply follows the computer and changes
// with it at sunset if the computer does.

export type Theme = 'system' | 'light' | 'dark'

const KEY = 'keevoro-theme'

export function savedTheme(): Theme {
  try {
    const saved = localStorage.getItem(KEY)
    if (saved === 'light' || saved === 'dark') return saved
  } catch {
    // No storage (a private window, say): follow the computer.
  }
  return 'system'
}

export function applyTheme(theme: Theme) {
  if (theme === 'system') delete document.documentElement.dataset.theme
  else document.documentElement.dataset.theme = theme
}

export function chooseTheme(theme: Theme) {
  applyTheme(theme)
  try {
    if (theme === 'system') localStorage.removeItem(KEY)
    else localStorage.setItem(KEY, theme)
  } catch {
    // It still applies for now; it just will not be remembered.
  }
}
