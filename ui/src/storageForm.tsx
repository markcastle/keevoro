/**
 * The questions every kind of cloud storage asks: which service, where it
 * is, the bucket, and the two keys. Shared by adding somewhere to back up
 * to and adding cloud storage as a source, so the two forms are one form.
 * The secret is typed into a password box and is never shown again.
 */
export type StorageAnswers = {
  kind: 'r2' | 's3' | 'endpoint'
  /** The Cloudflare account ID, Amazon region or server address. */
  place: string
  bucket?: string
  accessKeyId?: string
  secretAccessKey?: string
}

export function StorageFields<T extends StorageAnswers>(props: { form: T; onChange: (form: T) => void }) {
  const { form, onChange } = props
  return (
    <>
      <label>
        Storage
        <select value={form.kind} onChange={(e) => onChange({ ...form, kind: e.target.value as T['kind'], place: '' })}>
          <option value="r2">Cloudflare R2</option>
          <option value="s3">Amazon S3</option>
          <option value="endpoint">Another S3-compatible service</option>
        </select>
      </label>
      <label>
        {form.kind === 'r2' ? 'Cloudflare account ID' : form.kind === 's3' ? 'Region' : 'Address'}
        <input
          value={form.place}
          onChange={(e) => onChange({ ...form, place: e.target.value })}
          placeholder={form.kind === 'r2' ? '32 letters and numbers' : form.kind === 's3' ? 'eu-west-2' : 'https://…'}
          required
        />
      </label>
      <label>
        Bucket name
        <input value={form.bucket ?? ''} onChange={(e) => onChange({ ...form, bucket: e.target.value })} required />
      </label>
      <label>
        Access key ID
        <input
          value={form.accessKeyId ?? ''}
          onChange={(e) => onChange({ ...form, accessKeyId: e.target.value })}
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
