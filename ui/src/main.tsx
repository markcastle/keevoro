import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'

import '@fontsource/jetbrains-mono/400.css'
import '@fontsource/jetbrains-mono/500.css'

import { App } from './App'
import { keepTheBrowserOut } from './noReload'
import { applyTheme, savedTheme } from './theme'
import './tokens.css'
import './app.css'

// Before anything is drawn, so the window never flashes the wrong colours.
applyTheme(savedTheme())

// In the released app, a right-click gives no Reload, and F5 reloads
// nothing: reloading would drop a search or survey off the screen.
if (import.meta.env.PROD && '__TAURI_INTERNALS__' in window) keepTheBrowserOut()

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <App />
  </StrictMode>,
)
