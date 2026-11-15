# Keevoro: Planning

Keevoro finds every photo and video on a pile of old drives, gathers the keepers into one tidy library, and archives it to S3-compatible storage with a browsable index. It runs on Windows and macOS and is built for people who are not technical.

The full product thinking (business model, security levels, index rationale) lives in the planning doc: [Keevoro: Planning](https://claude.ai/artifact/X9Gx1eYMVmbPABA2u9od7y). This file is the engineering summary that the code follows.

## Who it is for

Someone with a drawer of old laptops, USB sticks and backup disks who knows the family photos are "in there somewhere". They should never see a hash, a bucket, an endpoint or an EXIF tag.

The promise, in their words:

- "Plug in my old drives and it finds all my photos and videos."
- "I keep what I want. It ignores the junk and the duplicates."
- "Everything ends up in one place, safely backed up."
- "It never touches or deletes anything on my old drives."

## Design direction: very Apple

The reference mockup is [docs/design/discover-mockup.png](docs/design/discover-mockup.png).

![Discover screen mockup](docs/design/discover-mockup.png)

What that means in practice:

- **Moments, not folders.** The main screen is **Discover**: large photos grouped into collections ("Cornwall, August 2008, 83 photos, 6 videos") with one primary action, **Keep collection**. The folder tree exists but sits behind a quiet "Show folders" option.
- **Scanning feels alive.** Collections appear while the drive is still being searched ("Searching family hard drive..."). Nobody waits for a progress bar to finish before seeing their photos.
- **Sources are named like people name things.** "Family hard drive", "Dad's old Dell".
- **One decision per screen**, written in plain words. Safe defaults everywhere: junk and duplicates are already handled, and nothing is copied until the user says so.
- **Generous space, big imagery, system fonts, soft materials.** No dense tables, no badges, no jargon. Advanced settings live behind one clearly labelled door.

This is why the UI plan is Tauri 2 with a web front end: big responsive image grids, smooth transitions and a polished look are far quicker to reach there than with native widget toolkits.

## Collections

A collection is the unit the user sees and keeps. The engine builds them in `keevoro-core::collections`:

- **Named albums** come from the user's own folders. `Pictures\Holidays\Cornwall` becomes "Cornwall" with "Holidays" as a quiet subtitle.
- **Moments** group unnamed files by date. A gap of more than 3 days starts a new moment, titled by its date range ("March 2012").
- **Undated** holds files with no believable date and no album.
- **The cover** is the largest photo for now. Later it will be the sharpest, best-exposed photo, chosen on-device.
- **Later:** smarter moment titles ("Walks with Max", "Summer evenings") from on-device image labelling and place names. This must never upload photos to a third party.

## Respecting the user's folders

People have often organised their photos already. Keevoro keeps that work:

- The meaningful part of each folder path becomes a nested album.
- Noise is trimmed from the path: user profile paths, `My Pictures`, `DCIM`, camera folders like `100CANON`, `New Folder (2)`, and "Copy of" or "Backup of C" wrappers.
- The same folder found on two drives becomes one album.
- Folder names give date hints ("2009-07 Spain", "July 2009"). Folders that are only a date (`2009_07_14`) are hints, not albums.

## Hard safety rules

These are requirements, not aims. Each one has tests.

1. **Source drives are read-only.** Keevoro never writes, renames, moves or deletes anything on a source.
2. **Copy, then verify.** A file counts as collected only when a re-read hash matches.
3. **Verify remote copies too**, with server checksums where supported and BLAKE3 stored on each object.
4. **Never overwrite originals.** Library and bucket are add-only (`create_only` puts).
5. **Be gentle with dying drives.** Timeouts, a retry list, and a rescue mode that copies first and analyses later.
6. **Check space before copying.**
7. **Everything resumes.** Every step is a persistent job.
8. **Nothing leaves the machine without consent.** No telemetry by default.

## Architecture

A Rust workspace of small crates. The core has no I/O, and storage sits behind narrow traits.

| Crate | Status | Responsibility |
| --- | --- | --- |
| `keevoro-core` | Built | Domain rules: media kinds, dates, folder hints, skip rules, junk hints, photo library layouts, collections. No I/O. |
| `keevoro-scan` | Built | Walk a source, detect media by content, read EXIF and video dates and pixel sizes, hash with BLAKE3. Read-only. |
| `keevoro-storage` | Built | Storage traits, errors, keys, and the shared conformance suite. |
| `keevoro-storage-folder` | Built | Local folder, external drive or NAS target. Reference provider. |
| `keevoro-cli` | Built | `keevoro scan`, `collections`, `sources`, `keep`, `skip`, `gather` and `publish`: the engine without a UI. |
| `keevoro-update` | Built | `keevoro update`: finds the latest GitHub release, checks the download against its checksum file and swaps the program (`ureq`, `sha2`, `self-replace`). |
| `xtask` | Built | Release tooling: changelog rules, version choice, release cut. Run with `cargo xtask`. |
| `keevoro-catalog` | Built | SQLite catalog (`rusqlite`, bundled): sources, what was found, keep choices. The job queue arrives with `keevoro-collect`. |
| `keevoro-collect` | Built | Copy and verify into the library (`fs4` for the free-space check). Renditions and conversion are separate, in `keevoro-media`. |
| `keevoro-media` | Part built | Thumbnails and display copies as WebP (`image`, `webp`), blurhash, rotation baked in, no metadata carried over. Video posters and MP4 conversion by running the computer's own ffmpeg as a separate program. HEIC and RAW previews to come. |
| `keevoro-publish` | Built | The upload engine: write-once originals, resumable multipart, retries. Depends only on the storage traits. |
| `keevoro-storage-s3` | Built | One S3-compatible adapter (`aws-sdk-s3`) with presets for R2, AWS S3 and any custom endpoint. Checked against MinIO in CI. B2 and Wasabi presets to come. |
| `keevoro-index` | Built | v2 index writer, its JSON Schema (`schemars`), and v1 gallery.json export. |
| `keevoro-platform` | Planned | Drive events, volume ids, keychain, per-OS differences. |
| `keevoro-engine` | Built | What Keevoro does as plain function calls: scan into a library, the Discover view, keep or skip, gather, previews. The command line and the app both sit on it. |
| `keevoro-app` (`src-tauri`) | Part built | Tauri 2 desktop shell: a thin layer of commands over the engine. Screens are in `ui/` (React, Vite, TypeScript). No installer or updater yet. |

### Storage plug-ins

S3-compatible storage is the priority. The engine depends only on these traits from `keevoro-storage`:

- `ObjectStore`: capabilities, connection check, head, get, put, list (paged), delete.
- `MultipartStore`: start, put part, complete, abort, list parts. The engine persists upload ids and part receipts so big uploads resume after a crash.
- `Capabilities` tells the engine what a provider supports (multipart limits, conditional create, server checksums, custom metadata) instead of it assuming.

Every provider must pass `keevoro_storage::conformance::run_all`. The suite writes under a unique prefix and cleans up, so it is safe against a real bucket.

### Index

Every upload writes a v2 JSON index under an unguessable name (`index/<128-bit random>.kvx`, served as JSON). Items are keyed by the first 16 hex characters of their BLAKE3 hash, albums nest, and private albums never appear in a public index. A v1 `gallery.json` is also exported so the current gallery site keeps working. The full schema is in the planning doc.

## Engineering principles

- SOLID, YAGNI, KISS. Crates are created when the work needs them, not before.
- TDD. At least 90% test coverage overall, at least 95% on copy, verify and storage code.
- `unsafe` is forbidden. `unwrap` and `expect` are linted outside tests.
- Pure rules live in `keevoro-core` so they can be tested exhaustively without touching disks.
- Clippy with `-D warnings` and `cargo fmt` must pass before any merge.
- GitHub is the only source control. CI runs on Windows, macOS and Linux.

## Decisions made

| Decision | Choice |
| --- | --- |
| Platforms | Windows and macOS. Linux builds but is not a v1 target. |
| Language | Rust |
| UI | Tauri 2 with a web front end, styled to the "very Apple" direction |
| Storage | Pluggable traits. S3-compatible first (R2 and AWS S3), plus local folder. |
| Folder handling | Keep the user's folders by default. Sort by date is an option. |
| Old videos | Opt-in automatic MP4 (H.264 and AAC) conversion in the first release |
| Index | Always written on upload, under an unguessable name |
| Hashing | BLAKE3. Item id is the first 16 hex characters. |
| Audience | Personal use first. Commercial options kept open. |

## Open questions

- Which encoder for MP4 conversion: bundled ffmpeg with x264 (GPL), or the OS encoders?
- Should keevoro publish straight into the existing gallery's bucket, moving the gallery site to the v2 index?
- How should private albums be protected: signed links, Cloudflare Access, or a separate bucket?
- Should encrypted backups move into the first release?
- HEIC and RAW in v1?
- Open source, open core, or closed?
