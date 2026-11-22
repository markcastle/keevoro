import { useState } from 'react'
import type { Backend, BucketSourceRequest } from './backend'
import { StorageFields } from './storageForm'
import { InfoTip } from './infoTip'

/**
 * "Add cloud storage as a source…": a bucket, or a folder in one, on
 * Cloudflare R2, Amazon S3, Google Cloud Storage or another S3-compatible
 * service, with the same
 * questions as adding somewhere to back up to, and one more, the folder.
 * Keevoro checks the keys by listing the bucket, which changes nothing,
 * keeps them in the computer's keychain, and then looks through it, which
 * only lists it.
 */
export function AddBucketSource(props: { api: Backend; onAdded: (root: string) => void }) {
  const [form, setForm] = useState<BucketSourceRequest | null>(null)
  const [checking, setChecking] = useState(false)
  const [refused, setRefused] = useState<string | null>(null)
  if (!form) {
    return (
      <button className="place" onClick={() => setForm({ kind: 'r2', place: '', bucket: '', prefix: '' })}>
        <CloudSourceIcon />
        <b>Add cloud storage as a source…</b>
        <span>A bucket on Cloudflare R2, Amazon S3, Google Cloud Storage or similar, to move or merge an old cloud backup</span>
      </button>
    )
  }
  const add = () => {
    setChecking(true)
    setRefused(null)
    props.api
      .addBucketSource(form)
      .then((added) => {
        setForm(null)
        props.onAdded(added.root)
      })
      // A wrong key is the person's to fix, so it is said beside the form.
      .catch((error) => setRefused(error instanceof Error ? error.message : String(error)))
      .finally(() => setChecking(false))
  }
  return (
    <form
      className="connect bucket-source"
      aria-labelledby="bucket-source-title"
      onSubmit={(e) => {
        e.preventDefault()
        add()
      }}
    >
      <h2 id="bucket-source-title">Add cloud storage as a source</h2>
      <StorageFields form={form} onChange={setForm} />
      <label>
        Folder in the bucket (optional)
        <input
          value={form.prefix ?? ''}
          onChange={(e) => setForm({ ...form, prefix: e.target.value })}
          placeholder="Leave empty for the whole bucket"
        />
      </label>
      <p className="quiet">
        Keevoro only reads this bucket, so a key that can only read is enough.
        <InfoTip about="reading from this bucket">
          It lists what is in it, and reads each file when it backs it up. Nothing in it is ever changed or deleted.
          The key is kept in your computer’s keychain.
        </InfoTip>
      </p>
      {refused && (
        <p className="refused" role="alert">
          {refused}
        </p>
      )}
      <div className="choices">
        <button className="primary" type="submit" disabled={checking}>
          {checking ? 'Checking…' : 'Add and look through it'}
        </button>
        <button className="button" type="button" onClick={() => setForm(null)}>
          Cancel
        </button>
      </div>
    </form>
  )
}

/** What backing up now would cost to read out of cloud storage, said before Back up now. */
export function CloudCost(props: { said: string[] }) {
  if (props.said.length === 0) return null
  return (
    <div className="cloud-cost" role="note">
      <b>Reading from cloud storage can cost money</b>
      {props.said.map((line) => (
        <p key={line} className="quiet small">
          {line}
        </p>
      ))}
    </div>
  )
}

/** A cloud with an arrow coming out of it: something read from cloud storage. */
export function CloudSourceIcon() {
  return (
    <svg width={44} height={44} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.75" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
      <path d="M7 18a4 4 0 0 1-.6-7.96A5.5 5.5 0 0 1 17 9.5a4.25 4.25 0 0 1 .5 8.5" />
      <path d="M12 12v9M9 18l3 3 3-3" />
    </svg>
  )
}
