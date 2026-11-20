// The one way pictures are fetched for a grid: a found photo's preview and
// a picture of a file being looked inside alike.
//
// - At most a few are asked for at once (`atOnce`), so a screen of two
//   hundred tiles does not send two hundred requests to a slow drive. The
//   app's side takes turns per drive as well.
// - A request not started yet is dropped when its tile leaves the screen
//   (`drop`). A request already running cannot be called back (a Tauri
//   call cannot be aborted), so only those few are ever wasted.
// - Each picture's address is kept for the next time it is wanted, up to
//   `keep` of them. The oldest is then let go, and a `blob:` address is
//   revoked so its bytes are freed: the leak the old preview map had.
//
// It holds addresses as strings, so the stand-in backend's drawn pictures
// go through it just the same.

/** Why a picture asked for was not fetched: its tile went away first. */
export class NotNeeded extends Error {
  constructor() {
    super('No longer needed')
  }
}

type Waiting = {
  key: string
  load: () => Promise<string>
  done: (url: string) => void
  failed: (error: unknown) => void
}

export class PictureQueue {
  private readonly atOnce: number
  private readonly keep: number
  private running = 0
  private waiting: Waiting[] = []
  /** Addresses made, oldest first (a Map keeps the order things were set in). */
  private made = new Map<string, string>()
  /** Requests running or waiting, so one asked twice is fetched once. */
  private asked = new Map<string, Promise<string>>()

  constructor(atOnce = 4, keep = 600) {
    this.atOnce = atOnce
    this.keep = keep
  }

  /**
   * The address of the picture known as `key`, fetched with `load` unless
   * it is held already. `first` puts it ahead of anything waiting.
   */
  want(key: string, load: () => Promise<string>, first = false): Promise<string> {
    const held = this.made.get(key)
    if (held) {
      // Used again: it becomes the newest.
      this.made.delete(key)
      this.made.set(key, held)
      return Promise.resolve(held)
    }
    const already = this.asked.get(key)
    if (already) return already
    const promise = new Promise<string>((done, failed) => {
      const job = { key, load, done, failed }
      if (first) this.waiting.unshift(job)
      else this.waiting.push(job)
    })
    this.asked.set(key, promise)
    // A failure is the caller's to show; the queue only forgets it.
    promise.catch(() => {}).finally(() => this.asked.delete(key))
    this.next()
    return promise
  }

  /** Stops waiting for `key`, if it has not been asked for yet. */
  drop(key: string) {
    const at = this.waiting.findIndex((job) => job.key === key)
    if (at < 0) return
    const [job] = this.waiting.splice(at, 1)
    job.failed(new NotNeeded())
  }

  /** How many are waiting and running: for tests and nothing else. */
  get size() {
    return { waiting: this.waiting.length, running: this.running, held: this.made.size }
  }

  private next() {
    while (this.running < this.atOnce && this.waiting.length > 0) {
      const job = this.waiting.shift()!
      this.running += 1
      job
        .load()
        .then((url) => {
          this.hold(job.key, url)
          job.done(url)
        })
        .catch(job.failed)
        .finally(() => {
          this.running -= 1
          this.next()
        })
    }
  }

  private hold(key: string, url: string) {
    this.made.set(key, url)
    while (this.made.size > this.keep) {
      const [oldest, address] = this.made.entries().next().value as [string, string]
      this.made.delete(oldest)
      if (address.startsWith('blob:')) URL.revokeObjectURL(address)
    }
  }
}

/** The key a found photo's small picture is known by. */
export const photoKey = (id: string) => `photo:${id}`

/** The key a file's small picture is known by, when looking inside a folder. */
export const fileKey = (path: string) => `file:${path}`
