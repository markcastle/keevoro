// Stratus, by Adam Sparshott, compares what cloud storage costs. When the
// person opens the comparison, Keevoro fetches its prices for one size and
// one currency (`storage.compare`, in keevoro-api), and the sheet in
// ./compare.tsx shows them. Nothing is kept: a price copied into Keevoro
// goes stale, and a wrong price is worse than none.
//
// This file is the part that can be tested without a browser: the size and
// currency to ask about, how a price and a size are said, and what each
// option is for a person using Keevoro: the line that says what kind of
// thing it is, and the form it opens when Keevoro can back up there.

/** Stratus as a person opens it in their own browser. */
export const STRATUS_PAGE = 'https://stratus.adam-sparshott.workers.dev/'

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

/** A size in GB as storage is sold: "240 GB", "1 TB", "2.5 TB". */
export function sizeSaid(gigabytes: number): string {
  if (gigabytes < 1000) return `${Math.round(gigabytes).toLocaleString('en-GB')} GB`
  const terabytes = Math.round(gigabytes / 100) / 10
  return `${terabytes.toLocaleString('en-GB')} TB`
}

/** The usual sizes to choose from, in GB. */
const STEPS = [100, 250, 500, 1000, 2000, 5000, 10000]

/** The sizes offered, smallest first, with this backup's among them when there is one. */
export function sizeSteps(backup: number | null): number[] {
  const all = new Set(STEPS)
  if (backup !== null && backup > 0) all.add(backup)
  return [...all].sort((a, b) => a - b)
}

/** A price in its currency, to the penny or cent: "£1.40", "$1,234.50". */
export function money(amount: number, currency: Currency): string {
  const said = (n: number) =>
    new Intl.NumberFormat('en-GB', { style: 'currency', currency, currencyDisplay: 'narrowSymbol', minimumFractionDigits: 2, maximumFractionDigits: 2 }).format(n)
  return amount > 0 && amount < 0.005 ? `under ${said(0.01)}` : said(amount)
}

/** When Stratus fetched its prices, as a person would say it: "today at 06:15". */
export function fetchedSaid(at: string | null, now: Date = new Date()): string {
  const when = at ? new Date(at) : null
  if (!when || Number.isNaN(when.getTime())) return 'just now'
  const day = (d: Date) => new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime()
  const clock = `${String(when.getHours()).padStart(2, '0')}:${String(when.getMinutes()).padStart(2, '0')}`
  const daysAgo = Math.round((day(now) - day(when)) / 86_400_000)
  if (daysAgo === 0) return `today at ${clock}`
  if (daysAgo === 1) return `yesterday at ${clock}`
  return `on ${when.getDate()} ${when.toLocaleString('en-GB', { month: 'long' })}`
}

/** What cloud storage costs for one size, cheapest first, as keevoro-api gives it from Stratus. */
export type Comparison = {
  gigabytes: number
  currency: Currency
  /** When Stratus last fetched its prices, or null. */
  fetchedAt: string | null
  /** Stratus's names for sources of prices it could not refresh, such as `apple`. */
  notRefreshed: string[]
  options: StorageOption[]
}

/** One plan or kind of storage, in Stratus's own words. */
export type StorageOption = {
  id: string
  /** Stratus's short name for the provider, such as `cloudflare-r2`. */
  provider: string
  /** The provider as people know it: "Cloudflare R2". */
  providerName: string
  /** The plan or class: "R2 Standard". */
  name: string
  /** `object` for storage rented by the GB, `consumer` for a plan of a fixed size. */
  kind: string
  /** How much a plan of a fixed size holds, in GB. */
  capacityGB: number | null
  /** Paid for by the amount kept. */
  metered: boolean
  /** Paid for once. */
  lifetime: boolean
  monthly: number
  yearly: number | null
  /** The provider's own page. */
  url: string | null
}

/** The form an option opens in Keevoro: its kind, and its address when there is one to fill in. */
export type SetUp = { kind: 'r2' | 's3' | 'gcs' | 'endpoint'; place: string }

/** Archive storage that must be restored before a file can be read, which Keevoro's check of every copy cannot wait for. */
function needsRestoring(option: StorageOption): boolean {
  return /deep archive|cold archive|glacier(?! instant)/i.test(option.name)
}

/** Cheaper storage for files rarely read, which costs extra to read back. */
function rarelyRead(option: StorageOption): boolean {
  return /infrequent|-ia\b|archive|glacier|coldline|nearline|\bcold\b/i.test(option.name)
}

/**
 * Which of Keevoro's own kinds of storage an option is, by Stratus's name
 * for its provider: Cloudflare R2, Amazon S3 and Google Cloud Storage by
 * name, and those that speak S3 as "Other storage" (with no address, as
 * Stratus gives none). Anything else (a plan such as iCloud+, Azure, which
 * does not speak S3, or archive storage that must be restored before it
 * can be read) is `null`: Keevoro cannot back up there.
 */
export function setUpFor(option: StorageOption): SetUp | null {
  if (needsRestoring(option)) return null
  switch (option.provider) {
    case 'cloudflare-r2':
      return { kind: 'r2', place: '' }
    case 'aws-s3':
      return { kind: 's3', place: '' }
    case 'gcs':
      return { kind: 'gcs', place: '' }
    // These speak S3, so Keevoro reaches them as other storage.
    case 'backblaze-b2':
    case 'wasabi':
    case 'digitalocean':
    case 'scaleway':
    case 'ovh':
      return { kind: 'endpoint', place: '' }
    default:
      return null
  }
}

/** What kind of thing an option is, in one short plain line. */
export function kindLine(option: StorageOption): string {
  if (option.kind !== 'object') {
    return option.lifetime ? 'A plan you pay for once, shown spread over five years.' : 'A plan for your phone and computer, like iCloud+.'
  }
  if (needsRestoring(option)) return 'Archive storage. Files must be restored, which takes hours, before they can be read.'
  if (!setUpFor(option)) return 'Storage you rent by the GB, of a kind Keevoro doesn’t speak.'
  if (rarelyRead(option)) return 'Storage you rent by the GB, for files rarely read. Reading them back costs extra.'
  return 'Storage you rent by the GB, that programs like Keevoro back up to.'
}

/** One row of the list, with what the sheet shows of it worked out. */
export type Row = {
  option: StorageOption
  /** The form it opens, or null when Keevoro can't back up there. */
  setUp: SetUp | null
  /** The cheapest Keevoro can back up to. */
  cheapest: boolean
  /** Its price was not refreshed when Stratus last looked. */
  notRefreshed: boolean
  /** How much storage it is: the size asked, or the plan's own. */
  storage: string
}

/** The rows to show, cheapest first. */
export function rowsOf(comparison: Comparison): Row[] {
  const stale = new Set(comparison.notRefreshed)
  const sorted = [...comparison.options].sort((a, b) => a.monthly - b.monthly)
  let marked = false
  return sorted.map((option) => {
    const setUp = setUpFor(option)
    const cheapest = setUp !== null && !marked
    if (cheapest) marked = true
    return {
      option,
      setUp,
      cheapest,
      notRefreshed: stale.has(option.provider),
      storage: sizeSaid(option.metered || option.capacityGB === null ? comparison.gigabytes : option.capacityGB),
    }
  })
}
