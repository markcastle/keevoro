// Draws Keevoro's app icon and saves it as a 1024 px PNG.
//
//   node scripts/make-icon.mjs icon.png && npx tauri icon icon.png -o src-tauri/icons
//
// The icon is two stacked photo frames, the same mark as in the app's
// sidebar. Edit the SVG below and re-run to change it everywhere.

import { chromium } from 'playwright'
const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="1024" height="1024" viewBox="0 0 1024 1024">
<defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#3d8bff"/><stop offset="1" stop-color="#0b4fd6"/></linearGradient></defs>
<rect x="64" y="64" width="896" height="896" rx="200" fill="url(#g)"/>
<rect x="352" y="232" width="440" height="390" rx="84" fill="none" stroke="#fff" stroke-width="54" opacity=".55"/>
<rect x="232" y="372" width="440" height="390" rx="84" fill="#1a63ea" stroke="#fff" stroke-width="54"/>
</svg>`
const b = await chromium.launch(); const p = await b.newPage({ viewport: { width: 1024, height: 1024 } })
await p.setContent(`<body style="margin:0;background:transparent">${svg}</body>`)
await p.screenshot({ path: process.argv[2], omitBackground: true }); await b.close()
