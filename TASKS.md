# Keevoro: Tasks

Work top to bottom. Each task is done when its tests pass, clippy is clean and coverage holds. Tick tasks off in the same commit that completes them.

## What is next

As of 0.37.1. Version 0.31.0 finished the big piece of plumbing: one API, a command line that does everything the app does, and an MCP server for AI agents. The releases after it, up to 0.37.1, added the look inside a folder, the map of where the space goes, the kinds of file, photos or videos only, folders left out, the backup history in full, clearing unfinished copies, and one estimate for the whole backup. This section is the order of work from here. The detail of each item is further down, under its own heading.

### 1. Prove it on real computers

Everything here was built and tested by machine. Some of it has now been tried by the owner on a real Windows computer; the rest should be, before more is built on top.

Tried by a person, on Windows:

- [x] The app's buttons work through the new API: a check of an 11,245-file backup on 0.31.0, and a 15.9 GB backup on 0.36
- [x] One estimate of the time left for the whole backup, with its stages listed
- [x] Previews made in the background while the originals are sent

Not tried by a person yet. The command line and AI agents:

- [ ] The command line: a search, a backup, `keevoro status`, `follow` and `history` on a real backup the app also uses
- [ ] Connect Claude Code to `keevoro mcp` and ask it to do a small backup
- [ ] Connect Claude Desktop the same way, and correct the snippet in docs/mcp.md if the app wants something different (it was not checked against the official page)

The Mac:

- [ ] Open the app once. The Mac build has never been run by a person
- [ ] Files kept only in iCloud are passed over and said, not downloaded

What was added since 0.31, on Windows:

- [ ] The backup history in full: open a row under "How earlier backups went"
- [ ] The kinds of file Keevoro looks for: add one, switch one off, put the built-in list back
- [ ] A backup, and a source, for photos only or videos only
- [ ] Clearing unfinished copies from a folder, from the backup screen and from Settings
- [ ] Keevoro's own folders (a backup, an old library) left out of a search and a survey, and said
- [ ] The look inside a folder, on a big real folder
- [ ] Where the space goes, on a whole real drive, and the map growing as it surveys
- [ ] A step of a backup that goes wrong marked so, with the reason: an unplugged destination, say

On a real Windows computer, what only a real one has:

- [ ] Mapped network drives in the list on "Where are your photos?", and "Find where the photos are" marking a folder on one that is already a source
- [ ] OneDrive-only files on a real OneDrive folder: counted and said, and nothing downloaded
- [ ] Idle and battery: previews made during a backup wait on battery and slow down while the computer is in use
- [ ] Long paths, over 260 characters, found and backed up
- [ ] A long backup's progress bar keeps one length and never steps back

### 2. Loose ends from the API and MCP work

Small, known, and better closed while the work is fresh.

- [x] Close the gap between checking a folder for an agent and opening it: pass the checked path on, so a link swapped in between cannot be followed
- [x] After an agent looks through a folder, keep the automatic duplicate check to folders that agent may read
- [x] Decide what a key given in a request means when a key is also set in the environment: the keys given win, and are what is tested and saved; the environment's are used only when none are given, and that is said
- [ ] Pause and carry on should reach a survey or a duplicate check running alone in another program. They hold no lock, so no other program can see them; reaching them needs a way for a program to be seen without holding a backup, best built with showing other programs' jobs in the app
- [x] `keevoro status` straight after another program starts a job can say "Nothing is running" for a moment
- [x] `keevoro update --check` on Linux always says there is nothing newer
- [x] Remove the unused MP4 conversion code, or bring it back as a proper feature (see "Old video conversion prompt"): removed; git history has it
- [x] Third-party licence acknowledgements in the app, now including the MCP library
- [ ] Progress notifications to an agent as a job runs, so it need not ask. Left for now: a job answers at once, and the protocol ties progress to a request still waiting, so they could only come during a `job_status` wait

### 3. What the first outside tester found

Someone other than the owner has now used Keevoro (0.37.1, Windows 11 Home). They found it easy and fast, and found one fault that breaks a promise. Their points, in the order to do them. The detail is under "From the first tester". From here on, nothing a person set or backed up may be lost by an update.

- [ ] A backup deleted at its destination is reported as still there: confirm a file is really at the destination before it counts as "already there"
- [ ] Carry settings over on update, starting with the "photos only" choice that 0.39.0 reset
- [ ] Remove and rename a backup in the app
- [ ] Stopping the background check for duplicates, and saying plainly what it is doing
- [ ] Drives appear and leave on their own, with a Refresh button
- [ ] Keep my folders as they are: a choice asked the first time a backup is set up
- [ ] The map: calmer colours, a summary ring, small folders merged
- [ ] Cloud drives as sources: synced cloud folders done properly, then a bucket as a source

### 4. Other kinds of file, carried along

Keevoro stays a photo and video tool, but can take your other files with it: music, documents, project files. Asked for by the owner. It comes after the map, which is where a person sees what else a folder holds. See "Other kinds of file".

- [x] A third kind in the kinds list, "other": found, backed up exactly and listed by folder, with no previews and no clever grouping

### 5. Scheduled and repeat backups

The next thing a person would notice, and the reason the jobs work came first: a scheduled run is just another program asking the API to run a backup, taking its turn with the app. Start with the single command a system scheduler can call, then the schedule in the app, then running with the window closed. See "Scheduled and repeat backups".

### 6. The catalog travels with the backup

A copy of Keevoro's record inside each destination, so a backup can be opened on a new computer or after the record here is lost. This matters more once backups run unattended. See "The catalog".

### 7. Cloud storage as sure as a folder

A folder backup is fingerprinted as it is sent and read back afterwards. Cloud storage is not yet held to the same standard.

- [ ] Send a checksum with each file and part, so the storage itself refuses a damaged upload
- [ ] Fingerprint what is read from the source as it is sent
- [ ] Label an object with its fingerprint when that is only learnt during the upload
- [ ] Several destinations sent at the same time, each file read once

### Later

- [ ] Prove, as a job the person starts, whether every photo in an old library is already in a backup, so it can be removed knowing nothing is lost (see "Keevoro's own folders are never a source")
- [ ] Show other programs' jobs in the app, and let Pause reach a survey or a check for duplicates running alone in another program
- [ ] Music done properly: artist and album from the file's tags, cover art as the picture, lengths (see "Other kinds of file"). Only if carrying music along as "other" proves to be used
- [ ] Agents on other machines: reaching Keevoro over a network, with sign-in and encryption
- [ ] The website: what Keevoro is, how to get it, that it is in beta, and where to send feedback (the spec is written; the domain and 16 questions are open)
- [ ] Windows code signing and Apple notarisation, when certificates exist (the tester saw Microsoft's download warning; the owner has a guide to doing it cheaply and will decide when)
- [ ] Signing in to a cloud service directly as a source (Dropbox, Google Drive, OneDrive without their apps installed): each needs its own sign-in and the provider's approval, and it changes what Keevoro is. Only on evidence that people want it
- [ ] Watch someone non-technical use the app without help

### Done since 0.31

- [x] See inside a source before choosing it: a look inside a folder (a grid of its photos and video stills, before it is added) and a map of where the space goes, with what is a source and what is backed up, growing as it surveys, and remembered honestly. Asked for by the owner after pointing Keevoro at a Pictures folder that also held texture packs, zip files and screen recordings. See "Seeing inside a source"

## Phase 1: Engine and CLI

Done when `keevoro scan` on a real old drive produces a believable summary and catalog.

### Done

- [x] Cargo workspace, shared lints (`unsafe` forbidden, `unwrap` and `expect` linted)
- [x] `keevoro-core`: media kinds, MIME and extension classification
- [x] `keevoro-core`: partial dates, EXIF date parsing, file-name dates, folder-name dates, best-date choice
- [x] `keevoro-core`: folder hints (album path from the user's folders, noise trimming, date hints)
- [x] `keevoro-core`: skip rules for Windows, macOS, Linux and developer folders, with plain reasons
- [x] `keevoro-core`: collections (named albums, moments by date gap, undated, cover choice, summary line)
- [x] `keevoro-scan`: read-only walk, content detection, EXIF dates, BLAKE3 hashing, unreadable reporting, progress
- [x] `keevoro-storage`: `ObjectStore`, `MultipartStore`, `Capabilities`, `ObjectKey`, errors
- [x] `keevoro-storage`: conformance suite (`run_all`)
- [x] `keevoro-storage-folder`: folder provider passing the conformance suite, Windows-safe names, create-only puts
- [x] `keevoro-cli`: `keevoro scan` with a Discover-style summary and `--json`
- [x] Video capture dates: read the recording time from MP4, MOV and 3GP containers (`DateSource::Video`)
- [x] Treat Apple Photos, iPhoto and Aperture libraries as one source: take originals, skip thumbnails and caches, keep old iPhoto event names
- [x] Junk hints: tiny images (under 300 px on the long edge), icons, game textures, browser cache folders, each with a plain reason; listed but left out of collections
- [x] `keevoro-catalog`: SQLite schema and migrations for sources, items, skipped folders, unreadable files and keep choices
- [x] `keevoro scan --library` writes to the catalog; `keevoro collections` and `keevoro sources` read from it
- [x] `keevoro skip` and `keevoro keep` tick and untick whole collections; choices are stored against content hashes
- [x] Duplicates keep the best-described copy (deepest named album, then strongest date), not the first one found
- [x] CI on GitHub Actions: fmt, clippy, tests on Windows, macOS and Linux, coverage gate at 90% (first run will confirm the number)

### To do

- [ ] Fixture drives in `tests/fixtures/`: generated by a script, covering DCIM trees, Picasa `.picasa.ini`, FAT timestamps, broken JPEGs, duplicates across "drives", AppleDouble files
- [ ] Read timeouts and a retry list for slow or failing drives
- [x] `keevoro-catalog`: persistent job queue. Not needed after all: jobs are shared between programs through small files in Keevoro's data folder instead (0.31, see "Jobs across programs" in docs/developing.md)
- [ ] Remember sources by volume serial (Windows) or volume UUID (macOS), so a drive is recognised under a new letter (today a source is recognised by its full path)
- [ ] Video dates for AVI, MTS and WMV, which do not use the MP4 container
- [ ] Near-duplicate grouping by perceptual hash ("Keep the best copy")

## Phase 2: Collect and storage

Done when the CLI can publish the current gallery's photos to R2 and the existing site still works.

This phase built the library model: photos were gathered into a library folder of Keevoro's own, then published from it. In 0.30 that model was removed. Photos now go straight from a source to each destination, and the command line works on the app's named backups. The lines below marked "removed in 0.30" are kept as the history of what was built and why it went.

- [x] `keevoro-collect`: copy to temp, re-read and hash, link into place without overwriting; never touch the source
- [x] Library layout that mirrors the user's folders, `By date/` for unnamed files, `Undated/`
- [x] Space check before copying
- [x] `keevoro gather`: resumable, reports failures file by file, refuses a library inside a source (removed in 0.30)
- [x] Ticking single photos and keeping junk the user wants (now `keevoro photos`, `keep`, `skip` and `set-aside` on a named backup)
- [ ] Merge same-named albums from different parents only when the person confirms (the preview tree this was planned for went with the library; it would be on the Choose screen now)
- [x] `keevoro-media`: thumbnails (400 px) and display copies (2048 px) as WebP, named by item id, with blurhash and upright rotation
- [x] Previews made from the library's own copies, remembering which files cannot be previewed (now made from the source during a backup)
- [x] Strip GPS and camera serials from published renditions (they are built from pixels only; a test checks no EXIF survives)
- [x] `keevoro-media`: previews for RAW files from the largest embedded JPEG, turned upright
- [x] `keevoro gather --retry-previews` for files that had no preview before (removed in 0.30)
- [ ] `keevoro-media`: previews for HEIC on a Mac and Linux (on Windows the computer's own codecs open HEIC when its extension is installed; elsewhere it needs libheif, a newer ffmpeg, or ImageIO on a Mac)
- [x] `keevoro-media`: video posters and opt-in MP4 conversion (remux when already H.264 and AAC; deinterlace; verify duration and a clean decode), using the ffmpeg found on the computer. The conversion went with `--convert-videos` in 0.30 and its unused code was removed after 0.37.1; posters stay
- [x] `keevoro gather --convert-videos`; videos carry duration, size, poster and MP4 in the index (the conversion was removed in 0.30; duration, size and poster stay)
- [x] Ship ffmpeg beside Keevoro, or use the OS decoders, so video features need no separate install: stills come from the computer's own decoder ("Video without extra software"). Shipping ffmpeg is only worth revisiting if MP4 conversion comes back (see the open question on licensing)
- [ ] Hardware encoders (NVENC, Quick Sync, VideoToolbox) and an honest time estimate for conversion. Only if MP4 conversion comes back
- [ ] "Keep only the MP4" option, with a clear warning. Only if MP4 conversion comes back
- [x] `keevoro-storage-s3`: S3-compatible adapter on `aws-sdk-s3`, with presets for R2 and AWS S3; checksums only when required for compatibility
- [x] Run the conformance suite against MinIO in Docker in CI
- [ ] Run the conformance suite against real R2 and S3 buckets on demand (needs a throwaway bucket and a scoped key)
- [x] Upload engine (`keevoro-publish`): resumable multipart, retry policy, `create_only` for originals, BLAKE3 in object metadata
- [x] `keevoro publish --to-folder`: backs up to an external drive or NAS and writes the index last (now `keevoro destinations add --folder` and `keevoro run`)
- [x] `keevoro publish --to-r2 / --to-s3 / --to-endpoint`, keys from the environment, connection checked before anything is sent (now `keevoro destinations add`)
- [x] Bucket listing check with a plain warning
- [x] `keevoro-index`: v2 index with JSON Schema; unguessable name (`index/<random>.kvx`); v1 `gallery.json` export
- [x] "Check my backup": `keevoro check` re-reads the library; `--backup` confirms every original is stored; `--read-back` downloads and compares (now `keevoro check` on a named backup, at every destination)
- [ ] Split the index into per-album files above about 5,000 items
- [x] "Change my private link": `keevoro publish --new-link` writes the index under a new name and removes the old one (removed in 0.30 with `publish`; not offered since)
- [x] Secrets in the OS keychain via `keyring`: `keevoro connect` and `keevoro disconnect`; the destination is remembered in the library (now `keevoro destinations add` and `remove`, remembered in the backup)

## Releases and installing

Modelled on Task Rail's. See [docs/developing.md](docs/developing.md#cutting-a-release).

- [x] `CHANGELOG.md` in Keep a Changelog form; the headings decide the next version number
- [x] `cargo xtask release` cuts a release: dates the changelog, sets the version, commits, tags
- [x] Release workflow: one button, optional rehearsal, CI must be green, draft first, builds for Windows, macOS (universal) and Linux, checksums, then publish
- [x] Roll back workflow: withdraw the bad release, restore the good code, release it under a higher number
- [x] One-line installers (`install.ps1`, `install.sh`) that check the download and need no administrator
- [x] `keevoro update` and `keevoro update --check`
- [x] First real release (0.1.0), installed on Windows with the one-line installer
- [ ] Try the Roll back workflow once, on a release that does not matter
- [ ] Replace the design mockup: it still says KeepSweep
- [ ] Windows code signing and Apple notarisation, when certificates exist
- [ ] A short address for the install lines, if a domain is bought

## Phase 3: Friendly app (Windows)

Done when a non-technical tester gets from a drive to a backup without help.

- [x] `keevoro-engine`: the logic the command line and the app share (summary, discover view, gather, previews; gather removed in 0.30)
- [x] Tauri 2 shell and app icon; screens in React, testable in a browser against a stand-in backend
- [x] Welcome screen: one question, where the library lives
- [x] Guided flow: a four-step journey always in view, one "next thing to do" on Discover, and a first-run explanation
- [x] One question per screen: Find, Choose, Gather, Back up are the only navigation; each screen asks one thing and has one main button; plugged-in drives are offered by name; no library question up front (Gather went with the library: three steps now)
- [ ] Watch someone non-technical use the app without help and note where they hesitate (the plan's test for this phase)
- [x] Discover screen as in the mockup: live search, hero collection, "Keep collection", more discoveries
- [x] Sources sidebar with friendly names and live status
- [x] Archive screen: what is kept, gathering with progress in human terms (now the backup screen)
- [x] Installers: the Release workflow builds a Windows installer (NSIS, per-user) and a universal macOS disk image; CI builds them on main as downloadable artefacts
- [x] In-app updates: the app offers a newer release and installs it in one click, using the same checksum check as `keevoro update` (no signing key needed)
- [ ] Signed updates (Tauri updater signature, as Task Rail does) on top of the checksum check; needs an updater signing key in repository secrets
- [x] Archive screen: connect a backup (another disk, or R2, S3 or another S3-compatible service), "Back up now", "Check my backup", disconnect
- [x] Backup and connect logic moved into `keevoro-engine`, shared by the app and the command line
- [ ] Storage setup with provider logos, and a link to instructions for making a scoped key
- [x] Open a collection to see and tick its photos; show what was set aside as junk, with one click to keep
- [x] Remove a source from the list (the source itself is never touched; gathered photos stay)
- [x] Faster search: programs, documents and music are passed over without being opened, and several files are read at once
- [x] The search says what it is doing: files looked at and the folder it is in
- [x] Ask before looking through a whole disk or top-level folder
- [x] Settings screen: library, backup, updates and the log in one place
- [x] A log of what the app did (`keevoro.log` in the app's log folder), viewable from Settings
- [x] Snappy search: a quick fingerprint while searching, the whole-file one afterwards in the background, and always before a file is gathered
- [x] Fingerprint a file while copying it, so a file gathered straight after a search is read once, not twice
- [x] A quick search from the command line too: `keevoro look` searches the way the app does
- [x] Stop a search part-way (nothing from a stopped search is kept)
- [x] Review duplicates before backing up: the copy kept, the copies left out, and where each is
- [ ] Choose which copy of a duplicate is the one kept (today Keevoro picks the best filed one)
- [ ] Spot duplicates across different backups, not only across the sources of one
- [ ] Near-duplicates: the same photo resized or re-saved (shown for review, never left out automatically)
- [x] Survey a drive in seconds, opening no file, and suggest the folders that hold photos and videos
- [x] Look through several ticked folders one after another
- [ ] Offer the survey for any folder, not only a whole drive. Partly there: any folder is surveyed for the look inside and the map, and `keevoro survey FOLDER` works, but "Find the folders with photos" is offered only for a whole drive
- [ ] Remember a survey so the suggestions are there next time without asking again. Surveys are remembered for the look inside and the map ("Seeing inside a source"); the suggestions screen does not yet read them
- [ ] Windows: read the NTFS file table directly for a survey of a whole disk in about a second (needs administrator rights)
- [ ] Survey every drive at once from the first screen
- [x] Hold down on a picture to see it screen-sized without changing its tick
- [x] Shimmering placeholders for thumbnails still on their way
- [ ] Play a video in the closer look (today it shows a still and its length). Decided against for now
- [x] Only fetch thumbnails for the tiles on screen, so a collection of a thousand does not ask for them all at once (the collection uses the look inside's grid, `VirtualTiles` and `PictureQueue`)
- [x] One button to look through every source again for new photos
- [ ] Say how many are new after looking again ("12 new photos"), not only the total
- [ ] A changed or deleted file on a source: say so (today an edited photo is backed up as a new file and nothing is ever removed from a backup)
- [ ] Show roughly how long a search has left
- [x] Camera raw files are never judged by the preview size in their header
- [x] Per-source choice of which kinds of file to include, folded away for those who want it
- [ ] Show a raw file's true pixel size (read from inside the file, not the header)
- [x] Activity page: what is being done and why, graphs of Keevoro's own processor and disk use, and Pause
- [x] Memory graph on the Activity page
- [x] Previews made during a backup wait by themselves while on battery or while the computer is busy with something else, and slow to one at a time while it is in use
- [ ] Pause the rest of Keevoro's work by itself when on battery or when the computer is busy with something else
- [x] Make previews during a backup at low priority, with fewer workers than the foreground and one reader per source, so they give way to everything else
- [ ] Run the background check for duplicates at low priority, and with fewer readers, so it gives way to everything else
- [x] Notice when a drive is plugged in or taken out: the first screen updates by itself within about two seconds (at once on Windows, from the system's own notice), with a Refresh button and the new drive brought to notice; the web view's own Reload menu is off. Asked for by the first outside tester
- [ ] Offer to look through a drive when it is plugged in (the seam is `places::if_changed`'s `arrived`; see "Run when a known source is plugged in")
- [ ] Time left ("About 20 minutes") for a search. Stopping a search is built; gather went in 0.30
- [ ] "Show folders" view with tick boxes, for people who want control
- [ ] Old video conversion prompt. Not asked for since the library went; the code was removed after 0.37.1 and git history has it

### Speed

- [x] Photos opened by the computer's own codecs on Windows (WIC), with Keevoro's own decoder as the fallback; on unless switched off in Settings
- [x] Previews made several at a time, on half the processor cores at most
- [x] One `Decoder` trait so a new way of opening photos is one new type, not a change everywhere
- [ ] macOS: open photos with ImageIO (the trait is ready for it; only the Windows half is built)
- [ ] Let the graphics card decode video for stills (Media Foundation can do this through DXVA; today it decodes in software)
- [ ] Measure previews per second with the switch on and off on a real Windows computer, and show the figure on the Activity page
- [x] Files go to a folder whole, read once and written once (they were written twice above 64 MB)
- [ ] Measure folder backup speed against a plain Windows copy on the same disks, and close any gap (bigger pieces, or the system's own copy call with progress)
- [x] Several destinations for one backup, sent one after another, each counted on its own
- [ ] Several destinations sent at the same time, each file read from the source once
- [ ] Previews on or off per destination (today it is per backup)
- [x] Command line: every destination of a backup, the same list as the app's (`keevoro destinations`, since 0.30)
- [x] Files are fingerprinted as they are sent, not read once beforehand; only lookalikes are read first
- [ ] Cloud storage: label an object with its fingerprint when that is only learnt during the upload
- [x] Six files at once to cloud storage
- [x] Four files at once to a folder on a file share (Windows)
- [x] Every copy written to a folder is read back and compared with the original's fingerprint
- [ ] Cloud storage: send a checksum with each file and part so the storage itself refuses a damaged upload
- [ ] Cloud storage: fingerprint what is read from the source as it is sent, as a folder backup now does
- [ ] macOS and Linux: tell a network folder from a disk
- [x] A backup of the originals alone, with no previews
- [x] Third-party licence acknowledgements in the app
- [ ] Choose how many files go at once (some connections want fewer, fast ones more)
- [ ] Send the parts of one large file at once, not only several files
- [ ] Back up two or three small files at once to a folder or network drive
- [ ] A stopped backup to a folder starts a half-written large file again; cloud storage carries on from the last piece
- [ ] Not planned: CUDA. NVIDIA only, a large runtime to install, and little to gain on still photos

### Look and feel

- [x] Reskin with the Keevoro design system: tokens, Keevoro Sans, the logo, soft-square photo tiles
- [x] Light, dark and match-my-computer, chosen in Settings
- [x] The design system's reversed logo, shown whenever the app is dark
- [ ] Sunset placeholder art where a photo has no preview
- [ ] Title bar that follows the chosen appearance on Windows

### Video without extra software

- [x] Thumbnails and posters from the computer's own decoder (`keevoro-frame`: Media Foundation on Windows, AVFoundation on macOS), tested against a real video on GitHub's Windows and macOS machines
- [x] ffmpeg only as a second try, and for converting old videos
- [ ] Windows: honour the rotation note on videos shot sideways on a phone
- [ ] Evaluate a pure-Rust decoder (`yscv-video` is young; `video-utils-rs` and a thumbnail-making `rivet` were not found on crates.io) as a fallback for formats the computer cannot play
- [ ] Say on the placeholder why a video has no thumbnail ("this computer can't play HEVC", with what to install)

### Backups and destinations

Separate, remembered jobs ("Drone footage to R2", "Family photos to a folder"), each sent straight to its destination. The thinking is in [PLANNING.md](PLANNING.md#backups-and-destinations).

- [x] Backups: named, separate, switchable from the side bar; the old single library becomes the first one
- [x] Destinations as a list: a folder first, then R2, S3 and other S3-compatible storage
- [x] Send straight from the source to the destination; no local library in between
- [x] Sending again later only sends what is new
- [x] Files first, previews and the index afterwards
- [x] Say how much room is needed; refuse a folder whose disk does not have it
- [x] Stop a send between files
- [x] Folder names open the folder
- [x] Live speed and time left while sending; a summary when it finishes; a record of earlier sends
- [x] One estimate of the time left for the whole backup, every stage listed, re-estimated on a schedule that suits its length, and how long it has been running
- [x] Previews made in the background while the originals are sent
- [x] Network graph on the Activity page (what Keevoro sends to cloud storage)
- [x] Count bytes as they are read, so a single huge file to a folder shows steady progress and not one jump at the end
- [ ] Stop part-way through one very large file (it currently finishes the file in hand)
- [ ] Show how much room a cloud bucket has, where the provider will say
- [x] More than one destination per backup
- [x] Reset to factory settings from Settings (only ever removes Keevoro's own data)
- [x] Rename and remove a backup in the app: Manage backups, in the switcher, with each backup's sources, destinations, size and last run, Rename in place, and Remove that says first what it does, sets the backup's own folder aside for a week to put back, and touches nothing at a source or destination. Asked for by the first outside tester
- [x] A backup that only looks for videos, or only photos, and one source that differs from its backup
- [x] Verify a folder destination by reading it back automatically after sending
- [x] Backups on the command line

### One API, a complete command line, and an MCP server

Built in five steps: the API and its register, the logic moved out of the app's shell, jobs shared between programs, the command line rebuilt on it, and `keevoro mcp` (docs/mcp.md). What is left is shipping it. The aim: anything a person can do in the app, a script can do from the command line and a program through an API, and on top of that an MCP server so an AI agent can run Keevoro for someone, safely.

Why it is more than adding commands: the app's shell has 47 operations and the command line 13, and much of what the app does (named backups, backing up to every destination, history, stop and pause, room, activity) is written in the shell where nothing else can reach it. That logic has to move to one place first.

The API

- [x] A `keevoro-api` crate above the engine: one function per operation, with plain requests and answers that can be written as JSON
- [x] Jobs for the long operations (look, survey, back up, check): start, watch, pause, carry on, stop
- [x] Events from a job in the app's own words: step, counts, speed, time left, closing summary
- [x] A register of every operation: name, description, what it takes and gives, and whether it only reads or also changes something
- [x] Move out of the shell and into the API: named backups, backing up everywhere with history, stop and pause, room, activity, settings, reset
- [x] One job per backup at a time across processes, with a lock a crashed process cannot leave shut
- [x] A running job's step and counts readable from another process, and stop and pause requests it can leave
- [x] A parity test: the build fails if the app can do something the command line or the MCP server cannot

The command line

- [x] Named backups, the same ones the app shows, in place of `--library`
- [x] Everything the app can do: backups, quick look, look again, remove a source, file types, duplicates, several destinations, originals only, run, stop, pause, status, history, room, activity, settings, reset
- [x] `--json` on every command; one line per event for jobs
- [x] Exit codes that tell "worked", "some files failed", "a destination could not be reached" and "no such thing" apart
- [x] Remove the old library-and-gather model everywhere, with nothing kept for compatibility: the `--library` commands, `gather`, copying into the library in the engine, and the app's leftover `gather` command
- [x] Before that code goes, confirm the folder store proves a copy as strictly as gathering did, so no check is lost

The MCP server

- [x] `keevoro mcp`: the Model Context Protocol over standard input and output, in the same binary, calling the API directly. No network port
- [x] Tools made from the register, each marked as reading or changing
- [x] A rehearsal tool: what backing up would do, changing nothing
- [x] Folders an agent may look through and back up to, set by a person; nothing allowed until they are
- [x] Storage keys can be supplied through the API, the command line and the MCP server alike, and are write-only everywhere: kept in the keychain, never returned, never logged
- [x] A test that supplies a marker key through each front end and searches everything Keevoro wrote for it
- [x] No tool to reset Keevoro, remove a backup or delete from one
- [x] A look-only mode, which is how the server starts; changes are switched on deliberately
- [x] Everything an agent does is in the log, marked as such
- [x] An "AI agents" section in Settings: whether changes are allowed, and the folders, with a way to add and remove each
- [x] A guide to setting it up in an AI app
- [x] Built for Windows, Mac and Linux in the release that ships it

Later

- [ ] Agents on other machines: reaching Keevoro over a network, with the sign-in and encryption that needs. Local only until then

Decided

- [x] "API" means a Rust crate all three front ends call, plus JSON from every command. No web API over the network
- [x] No old commands are kept; nobody else uses Keevoro yet
- [x] Keys can be supplied from the API, the command line and the MCP server, write-only everywhere
- [x] An agent is allowed no folders until a person names them
- [x] The server starts in look-only mode
- [x] This work comes before scheduled backups and the catalog work
- [x] The register (42 operations at first; 47 with the kinds of file looked for), the commands as typed, the tool list and the design for sharing a backup between programs are written out in the plan

Before building

- [x] The owner's go-ahead

### The kinds of file Keevoro looks for

- [x] One list for the whole of Keevoro, kept in its settings: each ending a photo or a video, built in or added, on or off
- [x] Add, edit and remove kinds; built-in ones can be switched off but not removed; a way to put the built-in list back
- [x] A kind that is added is found, told apart from its copies and backed up exactly, even when Keevoro cannot open it
- [x] Changes count from the next look through a source and never take away anything already found or backed up
- [x] The list is read once per search or survey, never per file
- [x] In the register, on the command line (`keevoro kinds`) and readable by an AI agent, which cannot change it
- [x] Settings, Advanced, in the app

### How earlier backups went, in full

- [x] Each backup's entry in the history keeps everything measured: stages with time and speed, what each destination was sent and could not be, previews, how it ended, who started it, and what it said
- [x] Each row opens in place in the app; `keevoro history N` and the API give the same, and an agent can read it

### A good guest on people's computers

- [x] Clear the unfinished copies a stopped backup leaves in a backup folder, at the start and end of each backup, when a folder is taken off a backup, and on request (`keevoro tidy`)
- [x] Abort uploads in parts at cloud storage for files a backup no longer sends
- [x] Keep the pictures cache within 1 GB, and clear settings files half-written by a program that stopped
- [x] The background check for duplicates said as what it is doing, with a Pause and a Stop of its own wherever it shows, an offer to stop it too when a search is stopped, and, once stopped, what that means and "Check for duplicates now"; it starts again by itself only after the next search or backup. Asked for by the first outside tester
- [ ] When cloud storage is taken off a backup, abort the unfinished uploads in parts this backup started there. Not done yet: taking it off forgets its keys at once, so the storage cannot be asked; doing it needs the removal to open the storage first, which may be offline. Until then they are aborted only while the destination is still part of the backup
- [ ] List and clear uploads in parts at cloud storage that this backup started but its record no longer knows (ListMultipartUploads under its own keys). Its record saves each upload before any part is sent, so this only matters for a record that was lost or reset

### Keevoro's own folders are never a source

- [x] Leave out backups, old libraries, a backup's own folder, Keevoro's own folders and every folder a backup goes to, in the survey and the search, and say so
- [x] Describe an old library (how big, how much is copies of photos) and offer to show it; never remove it
- [ ] Prove, as a job the person starts, whether every photo in an old library is already in one of this computer's backups (by fingerprint against each catalog), so they can remove it knowing nothing is lost. Not done yet: it reads every copy in full, gigabytes, so it must be a job with progress and a stop, not something done unasked
- [x] Say on screen, not only in the log, how many files a search passed over because they are only in online storage (OneDrive's "files on demand"), with how to include them: after a search, on each survey suggestion and on the source, at the command line and to an agent
- [ ] Check the online-only count against a real OneDrive folder on Windows and a real iCloud Drive on a Mac, which CI cannot do: the attributes are read from the folder listing, and should not download anything
- [ ] Recognise Keevoro's previews copied out of a backup by hand (loose `.webp` thumbnails). Nothing marks them as Keevoro's yet

### Seeing inside a source

Finding photos is hard when you do not know what a folder holds. "Find where the photos are" names the folders; these two views show what is in them, so the choice is made by looking, not by guessing from a name.

A look inside a folder

- [x] Each folder the survey suggests opens to a grid of thumbnails: photos, and a still from each video (its length comes in the next step)
- [x] The same look for any folder you pick yourself, and for a source already added
- [x] Thumbnails are made as you look and kept in Keevoro's own cache, never beside the originals; the source is only read
- [x] Only what is on screen is fetched, so a folder of ten thousand opens at once
- [x] Sort by date or name, and show photos only or videos only (by the file's date, said as such)
- [x] Open one in the closer look, and a video's length on its tile and there (playing it was decided against for now)
- [x] From the grid: look through this folder, or leave it out (and put it back), there, in the survey and under each source
- [x] Say plainly when a picture cannot be opened, with a placeholder, and carry on

A map of where the space goes

- [x] Where the space goes as a list first: every folder and the biggest files, photos or not, by size, with a share bar, the trail back up, and look inside, look through it or leave it out on the folder chosen; from the survey's result and each source; `keevoro map` and the agent tool where_the_space_goes
- [x] The survey records everything it passes, within fixed bounds and with exact totals, suggesting as quickly as before and measuring what it never looks through afterwards
- [x] Boxes sized by how much room each folder and file takes, drawn from folder listings alone so it stays as quick as the survey
- [x] Coloured by kind: photos, videos, and everything else (zip files, documents, programs), with a key and a pattern for each, in light and dark
- [x] A list beside it, by folder, with sizes and counts; choosing a row lights up its box, and the other way round
- [x] The list by kind of file too: a "By kind of file" tab with Photos, Videos and Everything else, each opening to its endings, and choosing one dims every box without it (`places.map` already gives the endings as `kinds`, and `keevoro map --kinds` lists them)
- [x] Click a box to go into that folder; a trail above to come back out, eased unless reduced motion is asked for
- [x] Point at a box to see its name, size and how many photos and videos it holds
- [x] From a folder on the map: look through it, leave it out, or open the look inside
- [x] Show what is already a source, and what is already backed up, on the map (`safe_by_folder` from the destinations' waiting logic, rolled up the map; a green outline for a source and a tick for what is backed up, in the key)
- [x] The map growing while the survey walks: the top-level boxes sent with its progress at most twice a second (`SurveyEvent.top`), drawn as they grow
- [x] Remembered maps that stay honest: open a map over an hour old on a local disk and it is surveyed again by itself (asked first on a network drive), a search of a source marks the surveys covering it out of date, at most 8 surveys and 200 MB, and Forget remembered maps in Settings
- [x] Show in File Explorer (Finder, Files) from a folder on the map: `open_place` allowed inside the remembered survey
- [x] Usable without a mouse and by a screen reader: the list carries everything the map does
- [x] Remember a survey, so the map is there next time without waiting

Not built

- [ ] Proving that every photo in an old library is already backed up, so it can be removed: a job of its own, under "Keevoro's own folders are never a source"

Decided

- [x] Whether the map shows everything on the drive or only folders that hold photos and videos (the first is more useful for finding things; the second is closer to what Keevoro is for): everything, with what is not a photo or video muted
- [x] Whether these views are also offered from the command line and to AI agents (the figures yes, as lists; the pictures only through `picture`, as now): yes, `keevoro folders`, `keevoro files`, `keevoro picture --file`, and the agent tools folders_inside and files_in_folder

### Other kinds of file

The backup itself does not mind what a file is: finding, fingerprinting, leaving out exact copies, copying, proving the copy and the history all work on any file. What is particular to photos and videos is everything around that: the grouping into collections, the previews, and the words on nearly every screen. So this comes in two steps, and the first is deliberately small.

Step 1: "other" files, carried along

- [x] A third kind beside photo and video in the kinds list: "other". Any ending can be added as it, including the ones refused today (music, documents, archives); the guard becomes a choice, with a plain line about what to expect. Nothing is refused by ending; Windows' page, hibernation and swap files and the Mac's sleep image are never taken, by name
- [x] Nothing is added by default: Keevoro finds photos and videos unless a person adds something
- [x] Other files are found, fingerprinted, told apart from their exact copies, backed up and proved exactly like any other file
- [x] Listed by the folders they came from, kept as the person arranged them, not grouped by date: "Other files" under the collections, a folder at a time, each opening to a plain list
- [x] No previews are attempted: a plain tile with the file's name and kind, and a closer look that is its details and Show in File Explorer
- [x] "Photos only" and "videos only" gain their natural partners, so a backup or a source can be for other files only, or for everything: tick boxes, at least one ticked, "Other files" only once a kind of them is listed
- [x] Counts and wording stay true: "238 photos, 11 videos and 40 other files", and screens that say "photos and videos" are checked one by one
- [x] The survey and the map count a wanted "other" kind as wanted, not as "everything else"
- [x] The backup's layout at the destination says where other files go (`originals/Other files`, in their folders), and the guide note explains it
- [x] On the command line and in the API; readable by an AI agent, which cannot change the list

Step 2: music done properly (later, and only if step 1 is used for music)

- [ ] Read artist, album, title, track and year from the file's tags, without changing the file
- [ ] Group by artist and album, falling back to folders when there are no tags
- [ ] Cover art from inside the file as the picture, and the length on the tile
- [ ] The same exact-copy rule as everything else: two files are duplicates only when they match byte for byte, never because their tags agree
- [ ] Decide what Keevoro is before building this: a full music mode starts to change it from a tool for keeping photos and videos into a tool for backing up anything

### From the first tester

Feedback on 0.37.1 from the first person other than the owner to use Keevoro. What they said is quoted or paraphrased; what to do about it follows.

A backup that was deleted is reported as still there

"If I deleted the backup from the destination and tried to repeat the steps I see '40 files already there'. It has recreated the index folder and the What Is In This Backup file again, but not the photos." Keevoro trusted its own record of what it had sent and did not look at the destination. Of everything here, this is the one that could cost someone their photos.

- [ ] Before a file counts as "already there", confirm it is at the destination: a listing of names and sizes for a folder, one paged listing for cloud storage, never a request per file
- [ ] One rule, used by the backup, the rehearsal, the counts on screen, the map's "Backed up" figures and the index
- [ ] A destination that cannot be reached is never treated as an empty one
- [ ] A destination that looks emptied or swapped is said plainly before sending, and what is missing is sent again in the same run
- [ ] Reading every file back byte for byte stays with "Check they all arrived"

Nothing a person set is lost on update

- [ ] Carry over a backup's and a source's "photos only" or "videos only" choice from before 0.39.0
- [ ] A catalog and a settings file as 0.37.1 wrote them, opened by the current code in a standing test
- [ ] Review every change to stored data since 0.29.3 for the same fault

Old backups cannot be removed

- [ ] "Manage backups…" in the switcher: each backup with its sources, destinations, size and last run, with Rename and Remove
- [ ] Remove says exactly what it does first: Keevoro's record goes; the photos already backed up stay where they are; nothing on a source is touched
- [ ] A running backup cannot be removed, and a removed one leaves no working folder behind

"This process kept running after I had stopped a scan"

- [ ] The background check for duplicates has its own Pause and Stop wherever it shows
- [ ] Stopping a search says what remains, and offers to stop that too
- [ ] "You can carry on" reworded so it is not read as an instruction
- [ ] If the check is stopped, say what that means and offer "Check for duplicates now"

A drive plugged in or removed does not show until refreshed

- [ ] Update the first screen when the system says a drive arrived or left, on Windows, a Mac and Linux, with a slow check as a fallback
- [ ] A visible Refresh button; nobody should need a right-click
- [ ] A drive that leaves while being looked through or backed up from is reported plainly and the work stops safely

Keep my folders as they are

Their source was Pictures / Scans / Family Scans; the backup put the files under By date / 2021 / 2021-01. For scans the file's date is the day of scanning, which means nothing, and they had already arranged them by family.

- [ ] Asked the first time a backup is set up, in one plain sentence: "Arrange by date" or "Keep my folders as they are" (decided by the owner: ask on first use)
- [x] "Keep my folders" writes originals / source name / their folders / file, the same arrangement other files use
- [ ] Per source as well, so a camera card can go by date while a folder of scans keeps its folders
- [x] Changing it later affects only what is sent from then on; nothing already backed up is moved or removed
- [ ] A suggestion with its reason when a source's files nearly all share one date or sit in folders with real names
- [x] Backups made before the choice existed stay "by date"; the guide note says which was used

The map

"A useful view, but could it use more neutral colour tones? Or even a choice of views, like a pie chart? Possibly everything under 10% of the backup merged into Others." They liked the list beside it a lot.

- [ ] Calmer colours: softer tints for photos and videos, the strong colour kept for the chosen box and what needs attention
- [ ] A summary ring above the map: photos, videos, other files chosen, everything else, with figures
- [ ] A choice of view, remembered: the map, or a plain bar per folder
- [ ] Small folders drawn as one "Smaller folders" box and row, opening on a click, the share adjusting with the window

Cloud drives as sources

"Could the software be used to scan and backup cloud drives… merging up several old cloud backups into one cloud or local drive. Moving to a cheaper cloud." Decided by the owner: the first two steps, with signing in to a service directly left for later.

- [ ] Recognise the synced folders of OneDrive, Dropbox, Google Drive and iCloud Drive and offer them by name on the first screen
- [ ] For files that are online only, offer "Fetch these and back them up", a few at a time, freeing each afterwards so the disk does not fill
- [ ] A bucket as a source: read from Amazon S3, Cloudflare R2 and compatible storage, so a backup can be moved to a cheaper cloud or two merged

Already answered

- [x] "Could the same software be used for more than just photos/videos? Music files, document file backups? Manual selection of file extension to search for?" Yes, from 0.39.0: Settings, Advanced, kinds of file, "Other files"

### Scheduled and repeat backups

Today a backup runs when someone presses "Back up now". These make it run by itself, which is what turns a one-off rescue into something that keeps photos safe as new ones are taken.

- [ ] Repeat a backup on a schedule: every day, every week, or every month, at a time the person chooses
- [ ] Each run looks through the sources again first, then sends only what is new
- [ ] Run when a known source is plugged in (a camera card, a drone's drive), with a choice of "ask me" or "just do it". The first half is built: Keevoro notices a drive arriving (`keevoro_api::places::if_changed` gives the drives that `arrived`, and on Windows `watch_for_drives` hears the system's notice); what is left is matching an arrival against each backup's sources and asking
- [ ] Run when a destination comes back (a backup drive plugged in, the network returns) if a run was missed
- [ ] Carry on in the background with the window closed: an icon by the clock, and a way to start with the computer
- [ ] Say what happened: a notification when a run finishes, and louder when one fails or a destination has not been reached for a while
- [ ] Be a good guest: wait while on battery, on a metered connection, or while the computer is busy, and never start two runs at once
- [ ] Never back up without the person's say on anything new: files a schedule finds are backed up, but newly found duplicates and set-aside pictures still follow the same rules
- [ ] Show the next run and the last few on the backup screen, with "Run now" and "Skip the next one"
- [ ] A missed run (the computer was off) is caught up at the next chance, once, not once per run missed
- [ ] Command line: a single command a system scheduler can call, for those who prefer their own

### The catalog (Keevoro's own record of a backup)

The catalog is one SQLite file per backup, `.keevoro/catalog.sqlite`. Losing it never loses a photo, but it does lose choices that cannot be found again by looking: what was unticked, what was rescued from the set-aside list, file-type choices, the destinations and the history.

- [ ] Keep a copy of the catalog inside each destination, written at the end of every backup, so a backup carries its own record
- [ ] Open a backup from the copy at its destination: on a new computer, or after the catalog here is lost
- [ ] Check the catalog for damage when a backup is opened, and say so plainly if it is damaged
- [ ] Offer to rebuild a damaged catalog: from the copy at a destination if there is one, otherwise by looking through the sources again
- [x] Use SQLite's write-ahead mode, so the screen can read counts while a backup writes, without queueing
- [ ] Warn when a backup's own folder is on a network drive, where SQLite is not reliable

## Phase 4: macOS

- [ ] `keevoro-platform` for macOS: DiskArbitration, volume UUIDs, Keychain
- [ ] Apple Photos and iPhoto library import with album names
- [x] CI builds and tests on macOS (by hand, with all platforms ticked) and releases have a universal Mac build
- [ ] Developer ID signing and notarisation

## Phase 5: Polish

- [ ] Rescue mode for failing drives
- [ ] Encrypted backups (`age`) with a printable recovery kit
- [ ] Password-protected 7z export (AES-256, encrypted names)
- [ ] Private-link and sign-in sharing
- [ ] Smarter collection titles and cover choice, on-device only
- [ ] More storage presets (B2, Wasabi, MinIO). RAW previews (phase 2) and updates (phase 3) are done
