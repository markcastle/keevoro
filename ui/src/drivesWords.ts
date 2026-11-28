// Whether a folder is on a drive: pure, so it is tested with Node alone (drives.test.ts).

/** True when `folder` is on the drive at `drive` (its root, written as the system writes it). */
export function onDrive(folder: string, drive: string): boolean {
  const tidy = (p: string) => p.replace(/[\\/]+$/, '').toLowerCase()
  const d = tidy(drive)
  const f = tidy(folder)
  return d !== '' && (f === d || f.startsWith(`${d}\\`) || f.startsWith(`${d}/`))
}
