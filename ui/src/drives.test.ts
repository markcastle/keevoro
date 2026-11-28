import assert from 'node:assert/strict'
import { test } from 'node:test'

// The hook needs a window; only the pure part is tested here.
const { onDrive } = await import('./drivesWords.ts')

test('a folder is on a drive when it is the drive or anywhere beneath it, however it is written', () => {
  assert.equal(onDrive('E:\\DCIM\\100CANON', 'E:\\'), true)
  assert.equal(onDrive('e:\\dcim', 'E:'), true)
  assert.equal(onDrive('E:\\', 'E:\\'), true)
  assert.equal(onDrive('F:\\Family Backup', 'E:\\'), false)
  assert.equal(onDrive('/Volumes/Card/DCIM', '/Volumes/Card'), true)
  assert.equal(onDrive('/Volumes/Cardigan', '/Volumes/Card'), false, 'a longer name is another drive')
  assert.equal(onDrive('C:\\Users\\You', ''), false)
})
