// Made-up prices for the demo and the screen tests, in the shape
// keevoro-api gives Stratus's answer to the window (`storage.compare`).
// The figures are invented and round, so nobody takes them for real ones.

import type { Comparison, Currency, StorageOption } from './stratus'

/** Made-up rates from US dollars. */
const RATES: Record<Currency, number> = { USD: 1, GBP: 0.8, EUR: 0.92 }

type Rented = { id: string; provider: string; providerName: string; name: string; perTerabyte: number; url: string }
type Plan = { id: string; provider: string; providerName: string; name: string; capacityGB: number; monthly: number; lifetime?: boolean; url: string }

/** Storage rented by the amount kept: a made-up price a month for each TB, in dollars. */
const RENTED: Rented[] = [
  { id: 's3-deep', provider: 'aws-s3', providerName: 'Amazon S3', name: 'S3 Glacier Deep Archive', perTerabyte: 1, url: 'https://aws.amazon.com/s3/pricing/' },
  { id: 'gcs-archive', provider: 'gcs', providerName: 'Google Cloud Storage', name: 'GCS Archive', perTerabyte: 1.2, url: 'https://cloud.google.com/storage/pricing' },
  { id: 'b2', provider: 'backblaze-b2', providerName: 'Backblaze B2', name: 'B2 Cloud Storage', perTerabyte: 6, url: 'https://www.backblaze.com/cloud-storage/pricing' },
  { id: 'wasabi', provider: 'wasabi', providerName: 'Wasabi', name: 'Wasabi Hot Cloud Storage', perTerabyte: 7, url: 'https://wasabi.com/pricing' },
  { id: 'r2-ia', provider: 'cloudflare-r2', providerName: 'Cloudflare R2', name: 'R2 Infrequent Access', perTerabyte: 10, url: 'https://developers.cloudflare.com/r2/pricing/' },
  { id: 'r2-standard', provider: 'cloudflare-r2', providerName: 'Cloudflare R2', name: 'R2 Standard', perTerabyte: 15, url: 'https://developers.cloudflare.com/r2/pricing/' },
  { id: 'azure-hot', provider: 'azure', providerName: 'Azure Blob Storage', name: 'Azure Blob Hot (LRS)', perTerabyte: 20, url: 'https://azure.microsoft.com/pricing/details/storage/blobs/' },
  { id: 's3-standard', provider: 'aws-s3', providerName: 'Amazon S3', name: 'S3 Standard', perTerabyte: 23, url: 'https://aws.amazon.com/s3/pricing/' },
]

/** Plans of a fixed size, in dollars a month. Each provider shows its smallest plan that fits. */
const PLANS: Plan[] = [
  { id: 'icloud-200gb', provider: 'apple', providerName: 'Apple iCloud+', name: 'iCloud+ 200GB', capacityGB: 200, monthly: 3, url: 'https://support.apple.com/en-us/108047' },
  { id: 'icloud-2tb', provider: 'apple', providerName: 'Apple iCloud+', name: 'iCloud+ 2TB', capacityGB: 2000, monthly: 10, url: 'https://support.apple.com/en-us/108047' },
  { id: 'icloud-12tb', provider: 'apple', providerName: 'Apple iCloud+', name: 'iCloud+ 12TB', capacityGB: 12000, monthly: 60, url: 'https://support.apple.com/en-us/108047' },
  { id: 'gone-2tb', provider: 'google', providerName: 'Google One', name: 'Google One 2TB', capacityGB: 2000, monthly: 10, url: 'https://one.google.com/about/plans' },
  { id: 'pcloud-2tb-life', provider: 'pcloud', providerName: 'pCloud', name: 'pCloud Lifetime 2TB', capacityGB: 2000, monthly: 7, lifetime: true, url: 'https://www.pcloud.com/plans' },
]

/** What the demo's Stratus answers for a size and a currency. */
export function demoComparison(gigabytes: number, currency: Currency, now: Date = new Date()): Comparison {
  const rate = RATES[currency]
  // To a hundredth of a penny, so a small size is not shown as free.
  const cents = (n: number) => Math.round(n * rate * 10000) / 10000
  const rented: StorageOption[] = RENTED.map((r) => ({
    id: r.id,
    provider: r.provider,
    providerName: r.providerName,
    name: r.name,
    kind: 'object',
    capacityGB: null,
    metered: true,
    lifetime: false,
    monthly: cents((r.perTerabyte * gigabytes) / 1000),
    yearly: cents((r.perTerabyte * gigabytes * 12) / 1000),
    url: r.url,
  }))
  const smallest = new Map<string, Plan>()
  for (const plan of PLANS.filter((p) => p.capacityGB >= gigabytes)) {
    const before = smallest.get(plan.provider)
    if (!before || plan.capacityGB < before.capacityGB) smallest.set(plan.provider, plan)
  }
  const plans: StorageOption[] = [...smallest.values()].map((p) => ({
    id: p.id,
    provider: p.provider,
    providerName: p.providerName,
    name: p.name,
    kind: 'consumer',
    capacityGB: p.capacityGB,
    metered: false,
    lifetime: p.lifetime === true,
    monthly: cents(p.monthly),
    yearly: cents(p.monthly * 12),
    url: p.url,
  }))
  // Early this morning, or yesterday's when it is earlier still.
  const fetched = new Date(now.getFullYear(), now.getMonth(), now.getDate(), 6, 15)
  if (fetched > now) fetched.setDate(fetched.getDate() - 1)
  return {
    gigabytes,
    currency,
    fetchedAt: fetched.toISOString(),
    notRefreshed: ['gcs', 'apple'],
    options: [...rented, ...plans].sort((a, b) => a.monthly - b.monthly),
  }
}
