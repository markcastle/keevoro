# Changelog

Every change to Keevoro that a person would notice, by version, newest first. Each release has what was **added**, what was **changed**, what was **fixed** and what was **removed**, written for the people using Keevoro rather than for the people building it. The Release workflow copies a version's section from here into its GitHub Release, which is what `keevoro update` points to as "What's new".

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the versions follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html) with one plain rule: a patch is for fixes and small adjustments, a minor is for anything you would notice as new. How a section gets here is in [docs/developing.md](docs/developing.md#cutting-a-release).

## [Unreleased]

Nothing yet.

## [0.10.0] - 2026-11-16

### Added

- **See what the search is doing.** While Keevoro looks through a source it now shows how many files it has looked at and the folder it is in at that moment, under the count of photos and videos found.

### Changed

- **Looking through a source is much faster.** Programs, documents, music and other files that are never a photo or video are passed over by name without being opened, and the files that might be are read several at a time. A whole computer's disk, which is mostly not photos, gains the most. A photo saved under a name like `.txt` or `.exe` is no longer found; files with unfamiliar endings or none are still opened and checked.

## [0.9.0] - 2026-11-16

### Added

- **The app updates itself.** When a newer Keevoro is out, a bar across the top says so. "Update now" downloads it, checks it is exactly the file that was published, and installs it; on Windows Keevoro closes and reopens by itself, and on a Mac the new disk image opens ready to drag across. "What's new" shows the release notes and "Later" puts it off. Your library is not affected. This starts working from this version onwards, so install this one by hand one last time.

## [0.8.0] - 2026-11-16

### Added

- **Remove a source.** Each source on "Where are your photos?" now has a Remove button. It only takes the source off Keevoro's list: nothing in the source itself is changed or deleted, photos you have already gathered stay in your library, and you can add the source again whenever you like.

### Fixed

- Folders on Windows are shown the ordinary way (`C:\Users\...`) without `\\?\` in front.
- A source with one file says "1 photo or video", not "1 photo and video".

## [0.7.0] - 2026-11-16

### Added

- **Plugged-in drives are offered by name.** "Where are your photos?" shows your USB sticks, external drives and memory cards as large buttons, so you no longer need to find a drive in a folder window. "Choose a folder" is still there for anywhere else.
- **"Show me my photos"** opens your library folder once the photos are gathered.

### Changed

- **The app is now four steps, one question at a time.** Find your photos, Choose what to keep, Gather them, Back them up. The steps down the side are the only navigation, each screen asks one thing, and one button at the bottom always takes you on. The Discover and Archive screens are gone; everything they did is in the step where you would look for it.
- **No question before you start.** Your photos are gathered into a folder called Keevoro Library in your Pictures; a small link lets you put it somewhere else.
- **Keeping a collection is a switch** that says "Keeping" or "Leaving out", in place of a button whose wording changed.

## [0.6.1] - 2026-11-16

### Changed

- **The app now shows you where you are and what to do next.** Four steps run across the top (Add a source, Choose what to keep, Gather, Back up) with the one you are on picked out, and Discover always has a bar at the bottom with the next thing to do, such as "Gather my photos". You no longer have to find that button on another screen.
- **The first screen explains how Keevoro works** in four short steps before asking where your library should live, and says what a library is.
- **Discover says how choosing works:** everything is kept unless you say otherwise, and you can click a collection to look inside.
- The Archive screen labels gathering and backing up as steps 3 and 4, and explains that backing up comes after gathering.

### Fixed

- **The large collage on Discover shows its photos side by side again.** In 0.5.0 and 0.6.0 they were stacked one above another.

## [0.6.0] - 2026-11-16

### Added

- **Back up from the app.** Once your photos are gathered, the Archive screen offers a second copy somewhere else: another disk, or cloud storage of your own (Cloudflare R2, Amazon S3, or another service that works the same way). Keevoro checks the storage works before remembering it, keeps the keys in your computer's keychain, and never replaces an original that is already backed up.
- **Check your backup from the app.** One click re-reads every original in your library and confirms each one is in the backup.

## [0.5.0] - 2026-11-16

### Added

- **Open a collection in the app.** Click a collection to see every photo in it, and click any photo to leave it out or put it back.
- **See what was set aside.** The app now shows the pictures it set aside as probably not memories, with the reason for each, and one click keeps any of them after all.

### Changed

- **The places Keevoro looks are now called sources** throughout the app and the command line, in place of "old drives". A source can be a drive, a USB stick or a folder.

## [0.4.0] - 2026-11-15

### Added

- **The first version of the desktop app.** Keevoro now has a window. It asks where your library should live, shows what it finds on a drive as collections with large photos, lets you keep or leave out each collection with one click, and gathers what you kept while showing how far it has got. It is early: backing up is still done from the command line, and the app does not yet update itself.
- **Installers for the app.** Each release now includes a Windows installer and a macOS disk image for the desktop app, alongside the command-line program.

## [0.3.0] - 2026-11-15

### Added

- **Choose single photos.** `keevoro photos` lists what is in a collection, each with a short code, and `keevoro skip --photo` or `keevoro keep --photo` leaves one out or puts it back.
- **Keep something that was set aside as junk.** `keevoro photos --junk` shows what was set aside and why. Keeping one brings it back into its album.
- **Check your library and your backup.** `keevoro check` reads every original again and confirms it is exactly as it was when gathered, so a failing disk is noticed while a good copy still exists. Add `--backup` to confirm every original is in the backup, and `--read-back` to download and compare each one.

## [0.2.0] - 2026-11-15

### Added

- **Videos get a poster.** Gathering now makes one still picture of each video for a gallery to show, chosen from the opening so it is not a black first frame.
- **Old videos can be made to play everywhere.** `keevoro gather --convert-videos` makes an MP4 copy of videos in older formats (AVI, WMV, MPG, 3GP and the like) so they play on any phone, TV or browser. Videos that only need rewrapping are repackaged without re-encoding, so nothing is lost. Every copy is checked: it must be the same length as the original and play through cleanly, or it is thrown away. Your originals are always kept, and modern MP4s are left exactly as they are.
- **Videos appear properly in the index.** Each one now carries its length, its size, and where its poster and MP4 copy are.
- **Change your private link.** `keevoro publish --new-link` writes the index under a new random name and removes the old one, so a link you have shared stops working.
- **Previews for camera RAW files.** Photos in RAW formats (CR2, NEF, ARW, DNG and others) now get a thumbnail and viewing copy, made from the finished picture the camera stores inside each file. The RAW file itself is kept untouched.
- **Connect your storage once.** `keevoro connect` checks your storage works, saves the keys in your computer's own keychain and remembers where the library is backed up. After that, `keevoro publish` needs nothing else. `keevoro disconnect` forgets it again.
- **Try missing previews again.** `keevoro gather --retry-previews` has another go at files that had no preview, which is worth doing after an update.

Posters and conversion use ffmpeg, which needs to be installed on the computer. Without it your videos are still gathered and backed up; Keevoro tells you what was skipped.

## [0.1.1] - 2026-11-15

### Fixed

- **`keevoro update` works behind office and school networks.** Some networks pass traffic through a proxy that uses its own security certificate. Keevoro now trusts the same certificates your computer does, so updating no longer fails there with a certificate error.
- **Problems are reported once, in plain words.** A failed command no longer prints "Error:" followed by a technical trace.

## [0.1.0] - 2026-11-15

### Added

- **Find every photo and video on an old drive.** `keevoro scan` looks through a drive or folder without changing anything on it. It recognises photos and videos by what is inside the file, not by its name, so renamed files are still found, and it skips system and program folders.
- **Dates worked out from every clue.** The camera's own record comes first, then the recording time inside videos, then dates in file and folder names. Each date remembers where it came from, so a weak guess is never passed off as certain.
- **Your folders become your albums.** `Holidays/Spain 2009` stays `Holidays / Spain 2009`. Camera folders such as `DCIM` and `100CANON`, and wrappers such as "Backup of C", are left out of the names.
- **Collections you can read at a glance.** Photos with no useful folder are grouped into moments by date ("August 2008"), alongside your named albums.
- **Duplicates are kept once.** The same photo on three drives counts once, and the copy that was filed most carefully decides where it goes.
- **Junk is set aside, not hidden.** App icons, game artwork, browser leftovers and pictures under 300 px are listed separately with the reason, and left out of your collections.
- **Apple Photos and iPhoto libraries are understood.** Keevoro takes the originals and ignores the thumbnails and caches. Old iPhoto event names are kept as albums.
- **A library that remembers your drives.** `keevoro scan --library` records what was found, so `keevoro collections` and `keevoro sources` work after the drive has gone back in the drawer.
- **Choose what to keep.** `keevoro skip` leaves a collection out and `keevoro keep` puts it back.
- **Gather your photos into one place.** `keevoro gather` copies what you kept into the library and reads every copy back to prove it matches the original. Nothing is ever overwritten, it checks there is room first, and it carries on where it left off if interrupted. Your old drives are only ever read.
- **Thumbnails and viewing copies.** Gathering makes a small and a large preview of each photo, turned the right way up. Location and camera details in your originals are never copied into them.
- **Back up to another disk.** `keevoro publish --to-folder` copies the library to an external drive or network share. An original already in the backup is never replaced.
- **Back up to Cloudflare R2 or Amazon S3.** `keevoro publish --to-r2`, `--to-s3` or `--to-endpoint`. Keevoro checks your keys work before sending anything and warns you if strangers could list your bucket.
- **An index of every backup.** Each backup ends by writing one file that lists every album and photo, under a long random name that cannot be guessed, plus a `gallery.json` for the existing gallery site.
- **Install in one line, update in one command.** A one-line installer for Windows, macOS and Linux, and `keevoro update` to move to the newest version. Every download is checked against the release's checksum before it is installed. Keevoro only contacts the internet for this when you ask it to.

[Unreleased]: https://github.com/markcastle/keevoro/compare/v0.10.0...HEAD
[0.10.0]: https://github.com/markcastle/keevoro/compare/v0.9.0...v0.10.0
[0.9.0]: https://github.com/markcastle/keevoro/compare/v0.8.0...v0.9.0
[0.8.0]: https://github.com/markcastle/keevoro/compare/v0.7.0...v0.8.0
[0.7.0]: https://github.com/markcastle/keevoro/compare/v0.6.1...v0.7.0
[0.6.1]: https://github.com/markcastle/keevoro/compare/v0.6.0...v0.6.1
[0.6.0]: https://github.com/markcastle/keevoro/compare/v0.5.0...v0.6.0
[0.5.0]: https://github.com/markcastle/keevoro/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/markcastle/keevoro/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/markcastle/keevoro/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/markcastle/keevoro/compare/v0.1.1...v0.2.0
[0.1.1]: https://github.com/markcastle/keevoro/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/markcastle/keevoro/releases/tag/v0.1.0
