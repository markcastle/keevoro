# Changelog

Every change to Keevoro that a person would notice, by version, newest first. Each release has what was **added**, what was **changed**, what was **fixed** and what was **removed**, written for the people using Keevoro rather than for the people building it. The Release workflow copies a version's section from here into its GitHub Release, which is what `keevoro update` points to as "What's new".

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the versions follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html) with one plain rule: a patch is for fixes and small adjustments, a minor is for anything you would notice as new. How a section gets here is in [docs/developing.md](docs/developing.md#cutting-a-release).

## [Unreleased]

### Added

- **Videos get a poster.** Gathering now makes one still picture of each video for a gallery to show, chosen from the opening so it is not a black first frame.
- **Old videos can be made to play everywhere.** `keevoro gather --convert-videos` makes an MP4 copy of videos in older formats (AVI, WMV, MPG, 3GP and the like) so they play on any phone, TV or browser. Videos that only need rewrapping are repackaged without re-encoding, so nothing is lost. Every copy is checked: it must be the same length as the original and play through cleanly, or it is thrown away. Your originals are always kept, and modern MP4s are left exactly as they are.
- **Videos appear properly in the index.** Each one now carries its length, its size, and where its poster and MP4 copy are.
- **Change your private link.** `keevoro publish --new-link` writes the index under a new random name and removes the old one, so a link you have shared stops working.

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

[Unreleased]: https://github.com/markcastle/keevoro/compare/v0.1.1...HEAD
[0.1.1]: https://github.com/markcastle/keevoro/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/markcastle/keevoro/releases/tag/v0.1.0
