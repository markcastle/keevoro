# Changelog

Every change to Keevoro that a person would notice, by version, newest first. Each release has what was **added**, what was **changed**, what was **fixed** and what was **removed**, written for the people using Keevoro rather than for the people building it. The Release workflow copies a version's section from here into its GitHub Release, which is what `keevoro update` points to as "What's new".

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the versions follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html) with one plain rule: a patch is for fixes and small adjustments, a minor is for anything you would notice as new. How a section gets here is in [docs/developing.md](docs/developing.md#cutting-a-release).

## [Unreleased]

Nothing yet.

## [0.19.0] - 2026-11-17

### Added

- **See what Keevoro is doing, and why.** A new page, "What Keevoro is doing" at the bottom of the side bar, names each job in hand (looking through a source, checking for duplicates, sending your files) and says in plain words why it is doing it and what it costs your computer.
- **Graphs of how hard it is working.** The same page charts Keevoro's own use of the processor and the disk over the last two minutes, with the figures now and at their highest. These are Keevoro's figures alone, not the whole computer's. Move across a chart to read any moment.
- **Pause Keevoro's work.** One button makes every heavy job wait once the file in hand is finished, and "Carry on" picks up exactly where it left off, with nothing lost or done twice. Use it when you need the computer for something else or are on battery. While paused, the side bar says so on every screen.

## [0.18.0] - 2026-11-17

### Added

- **Choose which kinds of file to include, source by source.** For people who want it, each source now has a quiet "File types" link. It shows the kinds of file Keevoro found there (JPG, DNG, MP4 and so on) with how many and how big, and a switch for each. Switch one off and every file of that kind on that source is left out of the backup; nothing on the source is changed, and the choice is remembered if you look through the source again. Everything is included unless you say otherwise, and adding a source is still one click.

### Changed

- **The Choose screen says why there is more than one collection.** Keevoro sorts what it finds into collections by folder name, or by month where folders have no names, so one source often makes several. The screen now says so.

### Fixed

- **Camera raw files are no longer set aside.** DNG files, and the raw files of other cameras, were being put in "pictures that don't look like photos" as tiny images. The size written at the start of a raw file is that of its small built-in preview, not of the photo, and Keevoro was taking it at face value. Raw files are now always treated as the photos they are. Any that were wrongly set aside are put back the next time you open Keevoro; you do not need to look through the source again.

## [0.17.0] - 2026-11-17

### Added

- **Each source shows how big it is.** The list of sources now gives the size of what was found on each, in MB or GB, beside the number of photos and videos.

### Changed

- **The bar with the next step is fixed to the bottom of the window.** It used to float over the page as a rounded card. It now runs along the bottom edge, full width, and stays there whether the page is long or short.

## [0.16.0] - 2026-11-17

### Added

- **Stop a search.** While Keevoro is looking through a source there is now a "Stop looking" button. It stops within a moment. Nothing from a stopped search is added, because only part of the source had been looked at; whatever Keevoro already knew about that source stays as it was, and you can choose it again whenever you like.

## [0.15.1] - 2026-11-17

### Changed

- The first screen now shows the whole Keevoro logo, with its name, not the picture tiles on their own.

## [0.15.0] - 2026-11-17

### Added

- **Reset Keevoro.** Settings has a new "Start again" section that puts Keevoro back as it was when first installed. It removes your backups from Keevoro (with what each found and what you chose), the previews it made and any storage keys it saved. It never touches your sources or anything you have already sent: those stay exactly where they are. It asks first and says all of this before doing anything.

## [0.14.1] - 2026-11-17

### Fixed

- **The index in a folder no longer spells out where the folder is.** When you send to a folder, the list of what was sent (in its `index` folder) used to contain the folder's full location for every file, such as `file:///Z:/Backups/...`. It now points to each file relative to itself. So the folder can be moved to another disk or drive letter and the list still works, and it no longer records how your disks are laid out. Folders you have already sent to are put right the next time you send to them.

## [0.14.0] - 2026-11-17

### Added

- **Keevoro tells you how much room your files need.** Before you choose where to send them it says how many files there are and how big they are in all. Once you have chosen a folder it compares that with the free space on that disk, and says so either way.
- **A disk without enough room is refused before anything is sent.** If the files will not fit, Keevoro says how much is needed and how much is free, and Send waits until there is room or you choose somewhere else. You no longer find out halfway through.
- **Stop a send.** While files are being sent there is a Stop link. The file in hand is finished, nothing is left half-sent, and everything already sent stays where it is. Send again later and it carries on from there.
- **Folder names open the folder.** Wherever Keevoro names a folder (where a backup sends to, each source, the backup's own folder in Settings) you can click the name to open it.

### Changed

- **Your files are sent first, previews afterwards.** Sending used to make a preview of every photo before the first file moved. Now each file is read, checked and sent straight away, and the previews and the index are made and sent once the files themselves are safe. If a send is interrupted, what has arrived is your photos and videos.

### Fixed

- The main button on the Send screen looked like an ordinary button after the redesign. It is the dark main action again.

## [0.13.2] - 2026-11-16

### Changed

- **Tidying behind the name change.** Now that sessions are called backups, Keevoro's own settings file and folders use that word too. Nothing looks or works differently, and the backups you already have open exactly as before.

### Fixed

- The words on the first screen can no longer be highlighted by accident when you click or press Ctrl+A.

## [0.13.1] - 2026-11-16

### Changed

- **Sessions are now called backups.** "Family photos" and "Drone footage" are backups: the name at the top of the side bar says Backup, and you "Start a new backup". Nothing about how they work has changed, and yours are all still there.
- **Keevoro's own typeface.** The app is now set in Keevoro Sans, drawn from the logo's lettering, in place of the stand-in it used before.
- **A lighter light and a softer dark.** Light is now near-white with cream kept for photo mats and the step you are on, so screens feel brighter. Dark is a soft deep teal with calmer text.
- The logo shown when the app is dark is now the design system's own reversed version.

## [0.13.0] - 2026-11-16

### Added

- **Light, dark, or match your computer.** Settings has a new Appearance choice. "Match my computer" follows whatever the rest of your computer is set to and changes with it; Light and Dark stay put. Your choice is remembered.

### Changed

- **Keevoro has its own look.** The app now wears the Keevoro design: the cream and deep teal of the logo, the Figtree typeface, photos shown as soft squares on a cream mat, and the real Keevoro logo in the side bar (with its cream-lettered version when the app is dark), on the first screen and as the app's icon. The fonts are built in, so it looks the same with no internet connection.

## [0.12.3] - 2026-11-16

### Fixed

- **You can remove a source while another is being looked through.** The Remove buttons were greyed out for as long as a search was running.

## [0.12.2] - 2026-11-16

### Changed

- **Video thumbnails no longer need any extra software.** Keevoro now asks the computer's own video player for a picture from each video (Media Foundation on Windows, AVFoundation on a Mac), so thumbnails and posters work on a freshly installed computer and no other program is started to make them. The picture is taken a little way into the video and found by jumping straight there, so a long video takes no longer than a short one. Videos in a format the computer itself cannot play show a plain placeholder; if ffmpeg happens to be installed it is still tried for those. Converting old videos to MP4, which only the command line offers, still uses ffmpeg.

## [0.12.1] - 2026-11-16

### Fixed

- **No more black windows flashing up on Windows.** Making a thumbnail for a video uses a helper program (ffmpeg), and each time it ran Windows opened a console window for it, so opening a collection with videos in it filled the screen with them. The helper now runs with no window at all. Keevoro also looks for it once instead of once per picture, and makes at most three previews at a time, so opening a large collection no longer slows the computer down.

## [0.12.0] - 2026-11-16

### Added

- **Sessions.** A session is one job you come back to, such as "Family photos" or "Drone footage". Each has its own sources, its own choices about what to keep and its own place to send things, and none of them affect the others. The session's name is at the top of the side bar: click it to open another or start a new one. What you already had becomes a session called "My photos", with everything it had found and chosen.
- **Come back later and send only what is new.** Open a session again, look through its source again, and Send copies only the files that were not sent before.
- **Choose where your photos go from a list.** A folder (on this computer, another disk or a network drive) comes first, then Cloudflare R2, Amazon S3 and other storage that works like S3. They are all the same kind of thing to Keevoro: somewhere to send your files.

### Changed

- **Three steps, not four: Find, Choose, Send.** There is no longer a separate "gather" step that copied everything into a library on this computer before it could go anywhere else. Files now go straight from the source to the place you chose, each one read in full and checked first, with no second copy made along the way. If you want them in a folder on this computer, choose that folder as the place to send them.
- "Check they all arrived" replaces "Check my backup", and "Send somewhere else" replaces "Disconnect".

### Removed

- The "Gather them" step and the Keevoro Library folder in Pictures as a required stop. Photos already gathered into it stay exactly where they are. The command-line `keevoro gather` is unchanged.

## [0.11.0] - 2026-11-16

### Added

- **Settings, all in one place.** A Settings link at the bottom of the side bar shows where your library is (and lets you use a different folder), where it backs up to, which version you have with a "Check for updates" button, and the log.
- **A log of what Keevoro has been doing.** Keevoro now keeps a plain record of every search, each large file it reads, anything it could not read, gathering and backups, with the time of each. "Show the log" in Settings displays it and "Open the log file" opens it as an ordinary text file you can send to someone. It never records keys or passwords.
- **Keevoro asks before looking through a whole disk.** Choosing your computer's disk, a large external drive or a top-level folder now asks first, says it may take a while, and offers "Choose a folder inside it" as the quicker way. USB sticks and memory cards start straight away as before.
- **The search says when it is on a large file, and when it is saving.** A long video can take minutes to read, during which the counts do not move; the screen now names the file and its size, and says "Saving what was found" at the end.

### Changed

- **Looking through a source no longer reads every file to the end.** The search now only identifies what is there (a few samples from each large file, not the whole thing), so a drive full of long videos is listed in a fraction of the time. The careful work happens afterwards, in the background, while you carry on: Keevoro reads each file fully to confirm which are true duplicates, and a quiet line in the side bar says how far it has got. Until that finishes, the duplicate count is a close estimate.
- **Gathering checks before it copies.** Any file not yet fully read is read first ("checked before copying"), so a copy is still proved exact against its original and two different files are never mistaken for one. A file that cannot be read in full is reported and not copied.

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

[Unreleased]: https://github.com/markcastle/keevoro/compare/v0.19.0...HEAD
[0.19.0]: https://github.com/markcastle/keevoro/compare/v0.18.0...v0.19.0
[0.18.0]: https://github.com/markcastle/keevoro/compare/v0.17.0...v0.18.0
[0.17.0]: https://github.com/markcastle/keevoro/compare/v0.16.0...v0.17.0
[0.16.0]: https://github.com/markcastle/keevoro/compare/v0.15.1...v0.16.0
[0.15.1]: https://github.com/markcastle/keevoro/compare/v0.15.0...v0.15.1
[0.15.0]: https://github.com/markcastle/keevoro/compare/v0.14.1...v0.15.0
[0.14.1]: https://github.com/markcastle/keevoro/compare/v0.14.0...v0.14.1
[0.14.0]: https://github.com/markcastle/keevoro/compare/v0.13.2...v0.14.0
[0.13.2]: https://github.com/markcastle/keevoro/compare/v0.13.1...v0.13.2
[0.13.1]: https://github.com/markcastle/keevoro/compare/v0.13.0...v0.13.1
[0.13.0]: https://github.com/markcastle/keevoro/compare/v0.12.3...v0.13.0
[0.12.3]: https://github.com/markcastle/keevoro/compare/v0.12.2...v0.12.3
[0.12.2]: https://github.com/markcastle/keevoro/compare/v0.12.1...v0.12.2
[0.12.1]: https://github.com/markcastle/keevoro/compare/v0.12.0...v0.12.1
[0.12.0]: https://github.com/markcastle/keevoro/compare/v0.11.0...v0.12.0
[0.11.0]: https://github.com/markcastle/keevoro/compare/v0.10.0...v0.11.0
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
