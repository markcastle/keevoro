import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'

import '@fontsource/jetbrains-mono/400.css'
import '@fontsource/jetbrains-mono/500.css'

import { App } from './App'
import { applyTheme, savedTheme } from './theme'
import './tokens.css'
import './app.css'

// Before anything is drawn, so the window never flashes the wrong colours.
applyTheme(savedTheme())

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <App />
  </StrictMode>,
)
