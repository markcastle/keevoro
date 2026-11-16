// Turns design/tokens.json (Keevoro's design system) into ui/src/tokens.css.
//
// The design system is the source of truth for every colour, size and
// shadow. This writes them out as CSS variables three ways, so the app can
// be light, dark, or follow the computer:
//
//   :root                      the light values
//   :root[data-theme="dark"]   the dark values, when the person chose dark
//   @media (dark) :root:not([data-theme="light"])
//                              the dark values, when following the computer
//
// Run `npm run tokens` after changing design/tokens.json. Never edit
// tokens.css by hand.

import { readFileSync, writeFileSync } from 'node:fs'

const tokens = JSON.parse(readFileSync(new URL('../design/tokens.json', import.meta.url), 'utf8'))
const themed = [...tokens.color.tokens, ...tokens.shadow.tokens]
const plain = [...tokens.spacing.tokens, ...tokens.radius.tokens]

const value = (token, theme) => (typeof token.value === 'string' ? token.value : (token.value[theme] ?? token.value.light))
const lines = (list, theme, indent) => list.map((t) => `${indent}--${t.name}: ${value(t, theme)};`).join('\n')
const changed = themed.filter((t) => value(t, 'dark') !== value(t, 'light'))

const css = `/* Keevoro design tokens. Generated from design/tokens.json by scripts/make-tokens.mjs: do not edit. */

:root {
  color-scheme: light;
  --font-sans: ${tokens.type.families.sans.replace('"Figtree"', '"Figtree Variable", "Figtree"')};
  --font-mono: ${tokens.type.families.mono};
${lines(themed, 'light', '  ')}
${lines(plain, 'light', '  ')}
}

:root[data-theme='dark'] {
  color-scheme: dark;
${lines(changed, 'dark', '  ')}
}

@media (prefers-color-scheme: dark) {
  :root:not([data-theme='light']) {
    color-scheme: dark;
${lines(changed, 'dark', '    ')}
  }
}
`
writeFileSync(new URL('../ui/src/tokens.css', import.meta.url), css)
console.log(`Wrote ui/src/tokens.css: ${themed.length + plain.length} tokens, ${changed.length} of them different in dark.`)
