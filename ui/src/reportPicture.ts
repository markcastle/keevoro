// A picture sent with a report, such as a screenshot: what kind it is, how
// big, and how one too big is made smaller before it goes.
//
// The person adds it themselves (chosen, pasted or dropped) and sees it
// before sending; Keevoro never takes one by itself. It is held only in
// memory, and goes after the report once the service has accepted it.
// The service takes PNG, JPEG and WebP, told by their first bytes, of at
// most 3 MB. Keevoro checks the same way here and again in Rust.

/** The most a picture may be: 3 MB, as the reports service takes. */
export const PICTURE_MOST = 3 * 1024 * 1024

/** The longest side a picture made smaller may have. */
export const LONGEST_SIDE = 2560

/** The JPEG qualities tried in turn when a picture is made smaller, until one fits. */
export const QUALITIES = [0.9, 0.82, 0.74, 0.66, 0.58, 0.5, 0.42]

export type PictureType = 'image/png' | 'image/jpeg' | 'image/webp'

/** A picture added to a report, as it will be sent. */
export type Picture = {
  bytes: Uint8Array
  type: PictureType
  /** Its size before it was made smaller, when it was. */
  wasSize?: number
}

const starts = (bytes: Uint8Array, at: number, ...want: number[]) => want.every((b, i) => bytes[at + i] === b)
const letters = (text: string) => [...text].map((c) => c.charCodeAt(0))

/** What kind of picture these bytes are, by their first bytes; null when not one Keevoro can send. */
export function pictureType(bytes: Uint8Array): PictureType | null {
  if (starts(bytes, 0, 0x89, ...letters('PNG\r\n'), 0x1a, 0x0a)) return 'image/png'
  if (starts(bytes, 0, 0xff, 0xd8, 0xff)) return 'image/jpeg'
  if (bytes.length >= 12 && starts(bytes, 0, ...letters('RIFF')) && starts(bytes, 8, ...letters('WEBP'))) return 'image/webp'
  return null
}

/** Whether these bytes are a picture Keevoro can send, and whether it fits as it is, or why not in plain words. */
export function pictureCheck(bytes: Uint8Array): { type: PictureType; fits: boolean } | { why: string } {
  if (bytes.length === 0) return { why: 'That picture is empty.' }
  const type = pictureType(bytes)
  if (!type) return { why: 'That isn’t a picture Keevoro can send. Choose a PNG, JPEG or WebP picture.' }
  return { type, fits: bytes.length <= PICTURE_MOST }
}

/** A picture's size as a person would say it: "412 KB", "1.8 MB". */
export function pictureSize(bytes: number) {
  if (bytes < 1024) return `${bytes} bytes`
  const kb = Math.round(bytes / 1024)
  if (kb < 1024) return `${kb} KB`
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
}

/** The line in what will be sent: "A picture, 412 KB". */
export const pictureLine = (bytes: number) => `A picture, ${pictureSize(bytes)}`

/** Said of a picture made smaller: "Made smaller to send: now 1.8 MB". */
export const smallerSaid = (bytes: number) => `Made smaller to send: now ${pictureSize(bytes)}`

/** The size a picture is drawn at to make it smaller: its longer side at most `longest`, its shape kept. */
export function scaledTo(width: number, height: number, longest = LONGEST_SIDE) {
  const scale = Math.min(1, longest / Math.max(width, height))
  return { width: Math.max(1, Math.round(width * scale)), height: Math.max(1, Math.round(height * scale)) }
}

/** Said when the report went and its picture did not, with why. */
export function pictureNotSent(why: string) {
  const said = why.trim() || 'Keevoro’s reports service didn’t take it.'
  return `Your report was sent, but its picture could not be: ${said}${/[.!?]$/.test(said) ? '' : '.'}`
}

/** Bytes in standard Base64, as Keevoro takes a picture. */
export function toBase64(bytes: Uint8Array) {
  let text = ''
  const step = 0x8000
  for (let at = 0; at < bytes.length; at += step) text += String.fromCharCode(...bytes.subarray(at, at + step))
  return btoa(text)
}

/** Said when a picture is too big even made smaller. */
export const STILL_TOO_BIG = 'That picture is too big to send, even made smaller. Try a smaller one, or crop it first.'
/** Said when a picture cannot be opened to make it smaller. */
export const CANNOT_OPEN = 'That picture couldn’t be opened to make it smaller. Try a smaller one, or crop it first.'

/**
 * A picture added to a report, as it will be sent, or why it cannot go.
 * One that is not a picture is refused; one too big is made smaller in the
 * window: scaled so its longer side is at most 2560 and saved as JPEG at
 * lower and lower quality until it fits.
 */
export async function pictureFrom(file: Blob): Promise<Picture | { why: string }> {
  const bytes = new Uint8Array(await file.arrayBuffer())
  const checked = pictureCheck(bytes)
  if ('why' in checked) return checked
  if (checked.fits) return { bytes, type: checked.type }
  let drawn: ImageBitmap
  try {
    drawn = await createImageBitmap(file)
  } catch {
    return { why: CANNOT_OPEN }
  }
  const { width, height } = scaledTo(drawn.width, drawn.height)
  const canvas = document.createElement('canvas')
  canvas.width = width
  canvas.height = height
  const pen = canvas.getContext('2d')
  if (!pen) return { why: CANNOT_OPEN }
  // A see-through picture becomes JPEG on white, as it would be printed.
  pen.fillStyle = '#fff'
  pen.fillRect(0, 0, width, height)
  pen.drawImage(drawn, 0, 0, width, height)
  drawn.close()
  for (const quality of QUALITIES) {
    const made = await new Promise<Blob | null>((done) => canvas.toBlob(done, 'image/jpeg', quality))
    if (made && made.size <= PICTURE_MOST) return { bytes: new Uint8Array(await made.arrayBuffer()), type: 'image/jpeg', wasSize: bytes.length }
  }
  return { why: STILL_TOO_BIG }
}
