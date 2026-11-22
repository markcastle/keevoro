/**
 * The questions every kind of cloud storage asks: which service, where it
 * is, the bucket, and the two keys. Shared by adding somewhere to back up
 * to and adding cloud storage as a source, so the two forms are one form.
 * The secret is typed into a password box and is never shown again.
 *
 * Google Cloud Storage has one address for every bucket, so it is not asked
 * where it is: Keevoro fills that in, and says where in Google's console the
 * keys are made instead.
 */
import { InfoTip, MoreAbout } from './infoTip'

export type StorageAnswers = {
  kind: 'r2' | 's3' | 'gcs' | 'endpoint'
  /** The Cloudflare account ID, Amazon region or server address; empty for Google Cloud Storage. */
  place: string
  bucket?: string
  accessKeyId?: string
  secretAccessKey?: string
}

/** What each kind of storage asks for as its place, or nothing when it needs none. */
function placeQuestion(kind: StorageAnswers['kind']): { label: string; placeholder: string } | null {
  switch (kind) {
    case 'r2':
      return { label: 'Cloudflare account ID', placeholder: '32 letters and numbers' }
    case 's3':
      return { label: 'Region', placeholder: 'eu-west-2' }
    case 'gcs':
      return null
    default:
      return { label: 'Address', placeholder: 'https://…' }
  }
}

export function StorageFields<T extends StorageAnswers>(props: { form: T; onChange: (form: T) => void }) {
  const { form, onChange } = props
  const place = placeQuestion(form.kind)
  return (
    <>
      <label>
        Storage
        <select value={form.kind} onChange={(e) => onChange({ ...form, kind: e.target.value as T['kind'], place: '' })}>
          <option value="r2">Cloudflare R2</option>
          <option value="s3">Amazon S3</option>
          <option value="gcs">Google Cloud Storage</option>
          <option value="endpoint">Another S3-compatible service</option>
        </select>
      </label>
      {place && (
        <label>
          {place.label}
          <input
            value={form.place}
            onChange={(e) => onChange({ ...form, place: e.target.value })}
            placeholder={place.placeholder}
            required
          />
        </label>
      )}
      <label>
        Bucket name
        <input value={form.bucket ?? ''} onChange={(e) => onChange({ ...form, bucket: e.target.value })} required />
      </label>
      {form.kind === 'gcs' && (
        <MoreAbout className="storage-help" label="Where to make the keys">
          <p>
            Make the keys in the Google Cloud console under Cloud Storage, Settings, Interoperability: choose Create a
            key for a service account or for your own account. The access key starts with GOOG.
          </p>
        </MoreAbout>
      )}
      <label>
        Access key ID
        <input
          value={form.accessKeyId ?? ''}
          onChange={(e) => onChange({ ...form, accessKeyId: e.target.value })}
          placeholder={form.kind === 'gcs' ? 'GOOG…' : undefined}
          autoComplete="off"
          required
        />
      </label>
      <label>
        Secret access key
        <input
          type="password"
          value={form.secretAccessKey ?? ''}
          onChange={(e) => onChange({ ...form, secretAccessKey: e.target.value })}
          autoComplete="off"
          required
        />
      </label>
    </>
  )
}

/**
 * Said under the form when Google Cloud Storage is chosen to back up to: its
 * colder classes charge for each read and for a minimum time kept, and
 * Keevoro reads files back to check them.
 */
export function GoogleClassNote(props: { kind: string }) {
  if (props.kind !== 'gcs') return null
  return (
    <p className="quiet small storage-help">
      Keep this bucket in Google’s Standard class. Colder classes charge each time Keevoro checks a file.
      <InfoTip about="Google’s bucket classes">
        Nearline, Coldline and Archive charge each time Keevoro reads a file back to check it, and charge for every file
        for at least 30 to 365 days.
      </InfoTip>
    </p>
  )
}
