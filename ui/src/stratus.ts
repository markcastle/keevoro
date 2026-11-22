// Stratus, by Adam Sparshott, compares what cloud storage costs. Keevoro
// shows it live, inside a sheet, only when the person asks, and never
// keeps a price or a provider of its own: a price copied into Keevoro goes
// stale between releases, and a wrong price is worse than none.
//
// This file is the part that can be tested without a browser: what is
// sent to the page (the size of the backup and a currency, nothing else),
// how what comes back from it is checked, and which of Keevoro's own kinds
// of storage a choice made there matches. Everything the page sends is
// treated as untrusted: only known fields are read, each checked, and only
// ever shown as text.

/** The one place the comparison is loaded from. Messages from anywhere else are ignored. */
export const STRATUS_ORIGIN = 'https://stratus.adam-sparshott.workers.dev'
/** Its page as it shows itself inside another: the price board alone. */
export const STRATUS_EMBED = `${STRATUS_ORIGIN}/#embed`
/** Its page as a person opens it in their own browser. */
export const STRATUS_PAGE = `${STRATUS_ORIGIN}/`

export type Currency = 'GBP' | 'USD' | 'EUR'

/** The countries that pay in euros, by their two-letter code. */
const EURO_COUNTRIES = new Set([
  'AT', 'BE', 'CY', 'DE', 'EE', 'ES', 'FI', 'FR', 'GR', 'HR', 'IE', 'IT', 'LT', 'LU', 'LV', 'MT', 'NL', 'PT', 'SI', 'SK',
])
/** Languages spoken mostly where people pay in euros, for a locale with no country in it. */
const EURO_LANGUAGES = new Set(['de', 'fr', 'es', 'it', 'nl', 'pt', 'fi', 'el', 'et', 'lv', 'lt', 'sk', 'sl', 'mt', 'ga', 'hr'])

/**
 * The currency a person most likely pays in, from their computer's locale
 * (`en-GB`, `fr-FR`, `de`): pounds in the United Kingdom and its islands,
 * euros in the euro countries, dollars anywhere else. Stratus offers only
 * these three.
 */
export function currencyFor(locale: string | undefined): Currency {
  const parts = (locale ?? '').replace(/_/g, '-').split('-')
  const language = (parts[0] ?? '').toLowerCase()
  const country = parts.slice(1).find((p) => /^[A-Za-z]{2}$/.test(p))?.toUpperCase()
  if (country) {
    if (['GB', 'UK', 'GG', 'JE', 'IM'].includes(country)) return 'GBP'
    if (EURO_COUNTRIES.has(country)) return 'EUR'
    return 'USD'
  }
  if (EURO_LANGUAGES.has(language)) return 'EUR'
  if (language === 'cy' || language === 'gd') return 'GBP'
  return 'USD'
}

/**
 * The size to ask about, in gigabytes as storage is sold (1 GB is a
 * thousand million bytes): what is being kept, rounded up to two figures
 * and a whole gigabyte, so 123.4 GB is asked as 130 GB and 7.2 GB as 8 GB,
 * and never less than 1 GB.
 */
export function gigabytesToAsk(bytes: number): number {
  const gb = Number.isFinite(bytes) && bytes > 0 ? bytes / 1e9 : 0
  if (gb <= 1) return 1
  const step = Math.max(1, 10 ** (Math.floor(Math.log10(gb)) - 1))
  // A hair taken off first, so 130.0000001 from floating point stays 130.
  return Math.round(Math.ceil(gb / step - 1e-9) * step)
}

/**
 * What Keevoro tells the page: the size and a currency, and to show object
 * storage (pay for what you keep), which is what Keevoro can back up to.
 * Nothing else about the person or their backup is ever sent.
 */
export function setMessage(bytes: number, locale: string | undefined) {
  return { stratus: 'set', need: gigabytesToAsk(bytes), currency: currencyFor(locale), kind: 'object' } as const
}

/** A choice made on the page, with only the fields Keevoro uses, each checked. */
export type Choice = {
  /** Stratus's own short name for the provider, such as `cloudflare-r2`. */
  provider: string
  /** The provider as people know it: "Cloudflare R2". */
  providerName: string
  /** The plan or class: "R2 Standard". */
  plan: string
  /** The provider's own page, when it gave a safe one. */
  url?: string
  /** The S3-compatible address to use, when Stratus gives one. */
  endpoint?: string
}

export type FromStratus = { type: 'ready' } | { type: 'select'; choice: Choice }

/** A short line of plain text, or nothing: no control characters, and not too long to show. */
function words(value: unknown, longest: number): string | undefined {
  if (typeof value !== 'string') return undefined
  // eslint-disable-next-line no-control-regex
  const plain = value.replace(/[\u0000-\u001f\u007f-\u009f]/g, ' ').replace(/\s+/g, ' ').trim()
  return plain.length > 0 && plain.length <= longest ? plain : undefined
}

/**
 * An address that is safe to open in the person's browser or to fill in as
 * storage: `https`, a real host name, nobody's name or password in it, and
 * not absurdly long. Anything else is dropped.
 */
export function safeHttps(value: unknown): string | undefined {
  if (typeof value !== 'string' || value.length > 2000) return undefined
  let url: URL
  try {
    url = new URL(value)
  } catch {
    return undefined
  }
  if (url.protocol !== 'https:' || url.username || url.password) return undefined
  if (!/^[a-z0-9.-]+$/i.test(url.hostname) || !url.hostname.includes('.')) return undefined
  return url.href
}

/**
 * What a message from the page means, or `null` for anything that is not
 * from Stratus or not understood. `origin` is the sender's, as the browser
 * says it; only Stratus's own is believed.
 */
export function readMessage(origin: string, data: unknown): FromStratus | null {
  if (origin !== STRATUS_ORIGIN) return null
  if (typeof data !== 'object' || data === null || Array.isArray(data)) return null
  const message = data as Record<string, unknown>
  if (message.stratus === 'ready') return { type: 'ready' }
  if (message.stratus !== 'select') return null
  const option = message.option
  if (typeof option !== 'object' || option === null || Array.isArray(option)) return null
  const fields = option as Record<string, unknown>
  const provider = words(fields.provider, 64)
  const providerName = words(fields.providerName, 120)
  if (!provider || !/^[a-z0-9-]+$/.test(provider) || !providerName) return null
  const choice: Choice = { provider, providerName, plan: words(fields.name, 120) ?? providerName }
  const url = safeHttps(fields.url)
  if (url) choice.url = url
  const endpoint = safeHttps(fields.endpoint)
  if (endpoint) choice.endpoint = endpoint
  return { type: 'select', choice }
}

/** The form a choice opens in Keevoro: its kind, and its address when there is one to fill in. */
export type SetUp = { kind: 'r2' | 's3' | 'gcs' | 'endpoint'; place: string }

/**
 * Which of Keevoro's own kinds of storage a choice is, by Stratus's name
 * for its provider: Cloudflare R2, Amazon S3 and Google Cloud Storage by
 * name, and those Keevoro knows speak S3 as "another S3-compatible
 * service", with the address only when Stratus gave one. Anything else
 * (a subscription such as iCloud+, or Azure, which does not speak S3) is
 * `null`: Keevoro cannot back up there directly.
 */
export function setUpFor(choice: Choice): SetUp | null {
  switch (choice.provider) {
    case 'cloudflare-r2':
      return { kind: 'r2', place: '' }
    case 'aws-s3':
      return { kind: 's3', place: '' }
    case 'gcs':
      return { kind: 'gcs', place: '' }
    // These speak S3, so Keevoro reaches them as another S3-compatible service.
    case 'backblaze-b2':
    case 'wasabi':
    case 'digitalocean':
    case 'scaleway':
    case 'ovh':
      return { kind: 'endpoint', place: choice.endpoint ?? '' }
    default:
      // An address given for something else means it speaks S3 too.
      return choice.endpoint ? { kind: 'endpoint', place: choice.endpoint } : null
  }
}
