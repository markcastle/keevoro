import assert from 'node:assert/strict'
import { test } from 'node:test'
import {
  PICTURE_MOST,
  pictureCheck,
  pictureLine,
  pictureNotSent,
  pictureSize,
  pictureType,
  smallerSaid,
  scaledTo,
  toBase64,
} from './reportPicture.ts'

const bytes = (...parts: (number[] | string)[]) =>
  new Uint8Array(parts.flatMap((p) => (typeof p === 'string' ? [...p].map((c) => c.charCodeAt(0)) : p)))
const PNG = bytes([0x89], 'PNG\r\n', [0x1a], '\n', [0, 0, 0, 13], 'IHDR')
const JPEG = bytes([0xff, 0xd8, 0xff, 0xe0, 0, 16], 'JFIF')
const WEBP = bytes('RIFF', [0x24, 0, 0, 0], 'WEBPVP8 ')

test('a picture is known by its first bytes, never its name', () => {
  assert.equal(pictureType(PNG), 'image/png')
  assert.equal(pictureType(JPEG), 'image/jpeg')
  assert.equal(pictureType(WEBP), 'image/webp')
  for (const not of [bytes(''), bytes([0x89], 'PN'), bytes('GIF89a'), bytes('RIFF', [0, 0, 0, 0], 'WAVEfmt '), bytes('%PDF-1.7')]) {
    assert.equal(pictureType(not), null)
  }
})

test('what is not a picture is refused in plain words; one too big is to be made smaller', () => {
  assert.deepEqual(pictureCheck(PNG), { type: 'image/png', fits: true })
  assert.deepEqual(pictureCheck(bytes('%PDF-1.7')), { why: 'That isn’t a picture Keevoro can send. Choose a PNG, JPEG or WebP picture.' })
  assert.deepEqual(pictureCheck(bytes('')), { why: 'That picture is empty.' })
  const big = new Uint8Array(PICTURE_MOST + 1)
  big.set(JPEG)
  assert.deepEqual(pictureCheck(big), { type: 'image/jpeg', fits: false })
  assert.equal(PICTURE_MOST, 3 * 1024 * 1024)
})

test('a picture’s size is said as a person would, as Keevoro says it', () => {
  assert.equal(pictureSize(800), '800 bytes')
  assert.equal(pictureSize(412 * 1024), '412 KB')
  assert.equal(pictureSize(412 * 1024 + 700), '413 KB')
  assert.equal(pictureSize(1024 * 1024 - 1), '1.0 MB')
  assert.equal(pictureSize(1_887_437), '1.8 MB')
  assert.equal(pictureLine(412 * 1024), 'A picture, 412 KB')
  assert.equal(smallerSaid(1_887_437), 'Made smaller to send: now 1.8 MB')
})

test('a big picture is scaled so its longer side is at most 2560, keeping its shape', () => {
  assert.deepEqual(scaledTo(5120, 2880), { width: 2560, height: 1440 })
  assert.deepEqual(scaledTo(3000, 4000), { width: 1920, height: 2560 })
  assert.deepEqual(scaledTo(1200, 800), { width: 1200, height: 800 }, 'never made bigger')
  assert.deepEqual(scaledTo(9999, 1), { width: 2560, height: 1 }, 'never thinner than a pixel')
})

test('a picture that could not follow its report says the report went', () => {
  assert.equal(
    pictureNotSent('That report already has a picture.'),
    'Your report was sent, but its picture could not be: That report already has a picture.',
  )
  assert.equal(
    pictureNotSent(' Pictures can’t be taken just now '),
    'Your report was sent, but its picture could not be: Pictures can’t be taken just now.',
  )
  assert.equal(pictureNotSent(''), 'Your report was sent, but its picture could not be: Keevoro’s reports service didn’t take it.')
})

test('a picture goes to Keevoro as standard Base64', () => {
  const enc = (s: string) => toBase64(bytes(s))
  assert.equal(enc(''), '')
  assert.equal(enc('f'), 'Zg==')
  assert.equal(enc('fo'), 'Zm8=')
  assert.equal(enc('foobar'), 'Zm9vYmFy')
  assert.equal(toBase64(bytes([0xff, 0xfe, 0])), '//4A')
  const many = new Uint8Array(100_000).map((_, i) => i % 251)
  assert.deepEqual(new Uint8Array(Buffer.from(toBase64(many), 'base64')), many, 'a long one too')
})
