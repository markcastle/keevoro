import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

// The app's screens live in ui/ and build to dist/ at the repo root, which
// is where src-tauri/tauri.conf.json expects to find them.
export default defineConfig({
  root: 'ui',
  plugins: [react()],
  clearScreen: false,
  server: { port: 5173, strictPort: true },
  build: { outDir: '../dist', emptyOutDir: true },
})
