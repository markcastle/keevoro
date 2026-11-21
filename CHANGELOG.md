# Changelog

Every change to Keevoro that a person would notice, by version, newest first. Each release has what was **added**, what was **changed**, what was **fixed** and what was **removed**, written for the people using Keevoro rather than for the people building it. The Release workflow copies a version's section from here into its GitHub Release, which is what `keevoro update` points to as "What's new".

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the versions follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html) with one plain rule: a patch is for fixes and small adjustments, a minor is for anything you would notice as new. How a section gets here is in [docs/developing.md](docs/developing.md#cutting-a-release).

## [Unreleased]

Nothing yet.

## [0.40.0] - 2026-11-21

### Added

- **Manage backups: rename one, or remove one you no longer need.** Our first outside tester could not get rid of old trial backups. The backup switcher now has "Manage backups…", which lists every backup with what tells them apart: where it looks, where it backs up to, how many photos and videos it holds and how big, and when it last backed up. Rename one in place, open it, or remove it. Remove says exactly what it does before it does anything, on the page: Keevoro's record of that backup on this computer goes; everything already backed up stays where it is and is not touched; nothing on any source is touched; and its previews and working files on this computer (it says how much room they take) are cleared away. Only then does it offer "Remove this backup from Keevoro". In case of a slip, a removed backup can be put back, exactly as it was, for a week, from "Removed recently" on the same screen; after that it is cleared away by itself, or straight away with "Clear away now", and storage keys that only it used go from your computer's keychain then too. A backup that is being searched, backed up or checked cannot be renamed or removed until that finishes, and the screen says why. The last backup can be removed too, which takes Keevoro back to its first screen. A name can be at most 60 letters and must not be one another backup has. At the command line: `keevoro backups forget NAME` (with `--dry-run` to see what it would do first), `keevoro backups removed`, `keevoro backups put-back NAME` and `keevoro backups clear-away NAME`. An AI agent cannot remove a backup.
- **Drives appear and leave by themselves.** Our first outside tester plugged in a memory card and it did not appear on "Where are your photos?" until they right-clicked and chose Refresh. Now a drive, USB stick or memory card plugged in (or a card pushed into a reader already plugged in) appears within about two seconds, first in the list and marked "Just plugged in", and one taken out goes. On Windows the system tells Keevoro straight away. While that screen is showing, Keevoro asks the system every couple of seconds for its list of drives, which it keeps in memory and which costs next to nothing, and only lists the drives in full when that list has changed; it never asks while the window is hidden or another screen is showing. There is a Refresh button beside the list too.

### Changed

- **No more Reload by right-click.** Right-clicking in the app used to show the web view's own menu, with Refresh (Reload on a Mac), which reloads the whole app: a search or a survey in progress dropped off the screen while it carried on unseen, and a question half answered was lost. That menu, and the F5, Ctrl+R and Cmd+R keys, are now off. Right-clicking in a box you are typing in, or on text you have selected, still offers cut, copy and paste.
- **The check for duplicates has its own Pause and Stop.** After a search, Keevoro reads in full, in the background, the large files the search only sampled, so that true copies are known. Our first outside tester stopped a search and found this still running, with nothing to say how to stop it, and "You can carry on" read like an instruction. Now the foot of the window and "What Keevoro is doing" say "Checking for duplicates in the background (5,455 of 7,093). You can keep using Keevoro while it finishes.", with Pause and Stop beside it. Its Pause pauses only the check, and everything else carries on; Keevoro's own Pause still pauses everything. Stopping a search while it runs says "Still checking what was found before for duplicates." with "Stop that too". Once stopped it says what that means: until the files are checked the counts of copies may be a little out, but nothing is backed up twice or left out by mistake, because a backup reads in full any file that looks like another before sending it. It does not start again by itself when you open Keevoro, only after your next search or backup, or when you press "Check for duplicates now". At the command line, `keevoro pause JOB`, `keevoro carry-on JOB` and `keevoro stop JOB` work on the check alone, `keevoro found` says when it was stopped and how many files are left, and `keevoro check-duplicates` checks now.

### Fixed

- **A copy found after its original was checked is never backed up twice.** If the check for duplicates had already read a large photo in full, and a copy of it turned up later on another source (with the check for that one stopped, or giving way to the backup), the backup could send the copy as a different file. A backup now reads in full, before sending, any file that looks like one already checked, and leaves out a true copy. Stopping the check for duplicates while all of Keevoro was paused no longer lets everything else carry on.
- **A drive unplugged while it is looked through is said plainly, and nothing found on it is forgotten.** Before, pulling out a drive part-way through a search listed every file it could not then reach as unreadable, and kept only what had been found so far as everything on it, so files found there by an earlier search dropped out of the list. Now the search stops, says "KINGSTON (E:) was unplugged. What was found so far is kept; plug it in and look again to carry on.", and keeps both what it found and everything found there before.
- **A source unplugged during a backup is said once, in words.** Instead of a long list of files that could not be backed up, a backup now says, for example, "E:\ was unplugged part-way through, so 1,203 files from it were not backed up. Everything already sent is safe; plug it in and back up again to carry on."
- **An unplugged backup drive's empty folder is never backed up into.** On a Mac or Linux a backup drive unplugged can leave the folder it was mounted on behind, empty, on the computer's own disk. If that folder was the place a backup goes, Keevoro took it for the backup emptied and would have sent everything again into it, filling the computer's own disk. Keevoro now remembers which disk a backup folder is on, from when it is added and each time it is reached, and an empty folder that is now on another disk is said as "The drive this backup goes to does not seem to be plugged in" and nothing is sent there. If that disk really is where the backup should go from now on, "Use this folder as it is now" says so. A backup folder added before this version is remembered by its disk the next time it is reached.

## [0.39.1] - 2026-11-21

### Fixed

- **"Photos only" and "Videos only" are back as you set them.** Version 0.39.0 changed how Keevoro writes down what a backup, or a source, is for, and did not carry over a choice made before it, so a backup or source set to photos only or videos only quietly went back to taking everything. Keevoro now reads the old choice and keeps it, for every backup and every source. Nothing already backed up was ever touched.
- **Keevoro's settings are never thrown away because part of them cannot be read.** If part of the settings file could not be understood (written by a newer Keevoro, say, or damaged), Keevoro used to start again from nothing, forgetting your backups. Now only that part is set aside, the rest is read as it was, and the whole file is kept beside it, as "settings that could not be read.json". A settings file from before there were named backups opens its library as the first backup again, as it did before 0.30. Likewise one entry in a backup's history that cannot be read no longer hides the others, and is kept rather than dropped.
- **A backup deleted from where it was sent is no longer said to be safe.** This was a serious fault, reported by our first outside tester: Keevoro remembered what it had sent and believed that memory instead of looking. If the files were deleted from the backup folder, the folder was swapped for an empty one, or a file there was cut short, Keevoro still said "Everything being kept is already at the folder", a backup said "40 files already there" and wrote the index and the note again without the photos, and only "Check they all arrived" noticed. Now Keevoro looks at what is really there, by name and size, before it counts anything as backed up: at the start of every backup and every rehearsal, and when you open "Back them up somewhere safe" or add a destination (a look is trusted for five minutes, and anything older is said as "was there when Keevoro last looked" rather than as certain). Before anything is sent it says what has gone, for example "The folder E:\Backup looks emptied or replaced: it no longer holds any of the 40 files Keevoro backed up there. They will be sent again.", sends them again in the same backup, and says "Restored 40 files that had gone from there." at the end and in the backup's history. Previews that have gone are sent again too, and the index lists only what is really there. "Check they all arrived" now also corrects what Keevoro counts, so "Back up again" sends whatever it found missing. A file there that is no longer the size it was sent at is never counted as safe; Keevoro never replaces a file in a backup, so it says so and leaves it for you. A drive that is unplugged, a network share that is offline or cloud storage that refuses the keys is said to be out of reach ("Keevoro can't find the folder E:\Backup. Is it plugged in?"), never taken for empty: nothing Keevoro knows about it is forgotten, and a backup folder that has gone is never made again on whatever disk is left. Nothing at a destination is ever deleted, and nothing Keevoro did not put there is taken for its own.

## [0.39.0] - 2026-11-21

### Added

- **Carry your other files along with your photos.** Keevoro is still for photos and videos, but you can now have it take other files with it: music, documents, project files. In Settings, Advanced, add a kind of file and choose "Other files" (or `keevoro kinds add flac --other`). Any ending can be added this way, even ones Keevoro used to refuse, such as .pdf or .mp3. Keevoro finds them, leaves out exact copies, backs them up and checks every copy exactly like your photos, into an "Other files" folder inside "originals" in the folders they came from, and the note in the backup says so. It never opens them to show a picture or sorts them by date: on "Choose what to keep" they are listed under "Other files" by the folder they came from, each folder kept or left out as a whole, and each opening to a plain list of its files with their size and when they last changed, a tick each, and Show in File Explorer. Counts name them ("238 photos, 11 videos and 40 other files"), "Find where the photos are" can suggest a folder of them, and the map shows them as a kind of their own, with its own pattern in the key. Nothing changes until you add a kind: out of the box Keevoro finds exactly what it always did.

### Changed

- **What a backup is for is now a set of tick boxes.** "Photos and videos", "Photos only" and "Videos only" are now "Photos" and "Videos" tick boxes, with "Other files" beside them once you have added a kind of other file, and at least one always ticked; a source can follow its backup or have its own. At the command line this is `keevoro wants photos videos` (it was `keevoro photos-or-videos`), `keevoro sources wants`, and `keevoro backups start NAME --wants photos` (it was `--photos-only`). A choice of photos only or videos only made before this version reads as everything again, so set it once more if you had one.

### Fixed

- **Storage keys you give are the ones Keevoro uses.** If `KEEVORO_ACCESS_KEY_ID` and `KEEVORO_SECRET_ACCESS_KEY` were set on your computer, keys you gave when adding cloud storage, in the app, at the command line or through an AI app, were quietly ignored, and the other keys were checked and saved instead. The keys you give now always win. Those set on the computer are used only when you give none, and Keevoro then says so, naming the settings but never the keys, before saving them for that place. A backup uses the keys saved for each place first.

## [0.38.0] - 2026-11-21

### Added

- **Open source licences, under About.** Keevoro is built on the work of hundreds of open source projects. **Open source licences** on the About page names every one shipped in the app, with its version and licence, and you can find one by name.

### Fixed

- **"Where are your photos?" opens without a pause on Windows.** Looking for mapped network drives could hold the window still for up to a second and a half each time the screen was shown or the window was returned to, and longer if a network drive's server was switched off. The answer is now worked out without holding up the window, kept for half a minute, and a drive that is slow to answer is not asked again until it does.
- **A step of a backup that goes wrong now says so.** If a destination cannot be reached, or some files could not be sent, that step is marked with a red cross and the reason beside it, such as "Couldn't back up 3 files", instead of a green tick or being left as still to come. The same is shown in the backup's history, and said by `keevoro status`, `keevoro history` and to an AI app. The time left no longer waits for a destination that could not be reached.
- **An AI app can no longer be led out of a folder you allowed by a shortcut changed at just the wrong moment.** Keevoro already judged every folder an AI app named by where it really leads; it now works on that very place, rather than reading the shortcut again afterwards. A destination an AI app adds through a shortcut is listed under where it really is.
- **After an AI app looks through a folder, Keevoro's check for duplicates stays in the folders you allowed it.** The check Keevoro starts by itself after a search used to read, in full, large files from every source of the backup, including ones the AI app was not allowed to look in. Those are now left for the check that follows a search you start yourself, or for the next backup.
- **`keevoro status` sees a job another Keevoro program has only just started.** Asked the moment the app or another command started a backup or a search, it could say "Nothing is running" for a quarter of a second.
- **`keevoro update --check` on Linux now says when there is a newer Keevoro.** It asked whether there was a newer desktop app, which Linux does not have yet, so it always said there was nothing newer, even when `keevoro update` would have installed one. It now asks about the command line, as `keevoro update` does, on every kind of computer.

## [0.37.1] - 2026-11-20

### Fixed

- **No drive looks chosen before you have chosen one.** On "Where are your photos?", a USB or external drive was shaded to suggest it, which looked as if it was already selected. Plugged-in drives are still listed first, and every card now looks the same until you click one. The same goes for the choices of where to back up to.
- **A mapped network drive now appears among your drives.** A drive letter that leads to another computer or server, such as `Y:` for a share, was missing from "Where are your photos?" and could only be reached through "Choose a folder". It is now listed as "Network drive (Y:)".

## [0.37.0] - 2026-11-20

### Added

- **Watch the map take shape while Keevoro looks.** While "Find where the photos are" walks a drive or folder, the biggest folders appear as boxes that grow as it goes, so you see what fills the place straight away rather than a spinner. If you have asked your computer for less motion, each new picture simply appears.
- **Where the space goes by kind of file.** Beside **By folder**, **By kind of file** lists the photos, videos and everything else in the folder shown, each opening to its endings (jpg, mp4, zip and so on) with how many files and how much room. Choose one and every box on the map without it fades, so you can see at a glance where the zip files or the screen recordings are.
- **See what is already a source, and what is already backed up, on the map.** A folder that is a source of this backup has a firm outline, and a folder whose files are all safely backed up has a tick in its corner; the list says it in words, such as "A source of this backup" and "Backed up: 1,180 of 1,204", and the key explains both. It is worked out when you open the map, so it is always as things are now. `keevoro map` says the same.
- **Show in File Explorer from the map.** Choose a folder on the map and **Show in File Explorer** (Show in Finder on a Mac, Show in Files on Linux) opens it in your computer's own file manager. Only folders on a map, or ones Keevoro already knows, can be opened this way.
- **Remembered maps stay honest, and can be forgotten.** A map over an hour old is mapped again by itself as you open it, if the drive is in this computer; across a network Keevoro asks first ("This was mapped 3 days ago. Map it again? It may take a while over the network."). Looking through a source marks the maps of where it is as out of date, so they are mapped again next time, and a survey you stopped is labelled "Partly mapped". Keevoro remembers at most eight maps, taking at most 200 MB, the oldest forgotten first. Since a map lists folder names across a whole drive, Settings, Advanced has **Forget remembered maps** (`keevoro forget-maps`); Reset forgets them too. Nothing on any drive is touched.

### Changed

- **The steps of a backup are marked like traffic lights.** A green disc with a tick when a step is done, an amber dot while it is under way, and a grey ring for one still to come, so you can see at a glance how far along it is. Previews that are all made now show as done straight away, instead of looking unfinished until the whole backup ends.
- **A survey of a whole drive takes less of your computer's memory.** It now keeps up to 100,000 folders one by one, rather than half a million, so mapping a very large drive holds under 40 MB instead of over 100 MB. Every size and count is still exact; past that, deeper folders (on a system disk, nearly always inside system and app folders) are counted into the folder above them, which says how many more it holds.

### Fixed

- **What a backup would take, on the map, no longer counts a Mac's hidden companion files.** Files like `._IMG_1.jpg`, which no search ever takes, were counted in "This backup would take", and so were photos inside a folder Keevoro made when a very large drive was mapped. Now the figure is only what a backup would really take.

## [0.36.1] - 2026-11-20

### Fixed

- **The progress bar of a backup no longer sits at full from the start.** For an instant before a backup had worked out what it had to do, it reported itself as finished, and the bar then held that. It now starts empty and follows the work, and a wrong reading can no longer pin it.

## [0.36.0] - 2026-11-20

### Added

- **See where the space goes.** After "Find where the photos are", **See where the space goes** shows everything on the drive or folder, photos or not, as a map of boxes in the style of WinDirStat, each sized by the room it takes: photos in orange, videos striped green, everything else grey, so you can see at a glance what is filling it. Point at a box to see what it is, click it to choose it, and click again to go inside; the trail above goes back up. Beside the map, a list of the same folders and files, biggest first, with how much room each takes, how many photos, videos and other files it holds, and its share as a bar; pointing at either lights the other, and the list works with the keyboard alone. It is how to find the zip files, screen recordings or an old Keevoro library filling a Pictures folder. Folders Keevoro never looks through (system and app folders, hidden ones, a backup or library it made) are listed too and say so, and anything this backup would not take says why: a folder you left out, or videos in a backup for photos only. Choose a folder to look inside it, look through it, or leave it out. Each source has it too, beside Look inside. It comes from what the survey saw, so nothing is opened or changed; **Map again** surveys afresh. From the command line: `keevoro map PLACE`, and an AI app can ask in folders you have allowed it.

### Changed

- **Find where the photos are now measures everything it passes, without keeping you waiting.** It suggests folders as quickly as before, then measures what it never looks through (system and app folders, hidden ones, Keevoro's own), so where the space goes shows the whole drive. While it measures, **Show the folders now** gives you the suggestions straight away, losing none. Pause now holds a survey too.

## [0.35.0] - 2026-11-20

### Added

- **Look inside any folder, sorted as you like, with each video's length.** Each source now has **Look inside** beside File types, and the first step has "Not sure what is in a folder? Look inside one first", which lets you pick any folder. If Keevoro has not looked at the folder's listings before, it does that first, quickly, and you can stop it. Show all, only photos or only videos, and sort by name, newest first or oldest first. The dates are each file's own (when it was last changed or copied), and the screen says so, since that is not always when a photo was taken. A video shows how long it lasts, read from the file without playing it, and never for one only in OneDrive. From the command line: `keevoro files FOLDER --videos --sort newest`.
- **Leave a folder out of a backup, and put it back.** Beside each folder "Find where the photos are" suggests, in the look inside a folder (the whole of it, or one folder in it), there is **Leave out**. A folder left out is not looked through for this backup, and what was found in it before is no longer offered, at once. It is said under the source it is in, and in what a search or survey passed by, each with **Put back**. Nothing in the folder is changed, and nothing already backed up is touched, and Keevoro says so the moment you choose. The look inside now shows everything Keevoro could find and marks what would not be backed up, saying why: a folder you left out, a backup or source for photos only or videos only, a kind of file left out of a source, or a kind Keevoro does not look for. From the command line: `keevoro sources leave-out FOLDER` and `--put-back`; an AI app can do it in folders you have allowed it, when you allow changes.

### Fixed

- **A pause asked for by another Keevoro program no longer outlasts the work it paused.** If the command line or an AI app asked Keevoro to pause just as the app's search or backup was ending, the app could stay paused afterwards, with its check for duplicates waiting where nothing else could see it or tell it to carry on. Now such a pause ends with the work it was for, and the log says so. A pause you press in the app still lasts until you press Carry on, and the app now says "Keevoro is paused" at the foot of every screen whenever it is, whoever paused it.
- **The bar at the foot of a narrow window no longer pushes its button off the side.** If the window is very narrow and Keevoro's typeface is slow to load, the words wrap and the button moves to a line of its own.

## [0.34.0] - 2026-11-20

### Added

- **A backup can be for photos only, or videos only, and so can one source.** In Settings, under This backup, Advanced, choose photos and videos, photos only or videos only; beside a source, under File types, a source can follow its backup or have its own choice. It counts at once on what was already found, with no need to look again, and every count follows it. Anything already backed up stays backed up; it is just no longer kept in what the backup chooses. The File types list says why a kind is left out ("left out: this backup is for photos only"). From the command line: `keevoro backups start "Drone" --videos-only`, `keevoro photos-or-videos photos` and `keevoro sources photos-or-videos 1 both`.
- **A button to clear unfinished copies, for any folder.** Beside "Show me the folder" on the backup screen, and in Settings, Advanced, for a folder no backup goes to any more ("Clear unfinished copies from a folder…"), Keevoro first says how much a stopped backup left there and what it will leave alone, then clears it when you say so, in the page with no pop-up. Afterwards it says what was cleared and what was left and why, in the same words as `keevoro tidy`, which can now also say first with `--dry-run`. Nothing that was backed up is touched.
- **Keevoro now says on screen when files were only in OneDrive, so were not looked at.** After looking through a folder, beside each folder "Find where the photos are" suggests, and on the source itself for as long as it is a source, it says plainly how many: "412 files here are in OneDrive only, not on this computer, so Keevoro did not open them. To include them, make them available offline in OneDrive first, then look again." Opening them would download them all, so Keevoro still leaves them be; it just never leaves a photo out without saying. The command line (`keevoro look`, `keevoro survey`, `keevoro sources`) and an AI app say the same. On a Mac, files kept only in iCloud are recognised and said in the same way.
- **Look inside a folder before choosing it.** Each folder "Find where the photos are" suggests now has **Look inside**: a grid of its photos and a still of each video, folder by folder as you keep them, with a closer look at any one. Nothing is added or changed until you press **Look through this folder**. Pictures are made as they come on screen, a few at a time and gently on a slow or network drive, and kept only in Keevoro's own cache, so a folder of thousands opens at once and the search afterwards has them ready. A file only in OneDrive is shown as "In OneDrive only, not on this computer" and never opened; one that cannot be shown says so. It follows the kinds of file Keevoro looks for, and a backup for photos only shows no videos. From the command line: `keevoro folders`, `keevoro files` and `keevoro picture --file`, and an AI app can do the same in folders you have allowed it.

### Fixed

- **Keevoro no longer offers to back up the files it made itself.** Looking through a folder that held a Keevoro backup, a library left by an older version, or Keevoro's own previews could offer them as photos to back up, such as a collection called "display". Now they are left out of "Find where the photos are" and of every search, and Keevoro says so in one quiet line you can open: what each folder is, where, and a button to show it. A Keevoro backup is recognised by what Keevoro writes into it, whatever it is called, and every folder one of your backups goes to is left out too. Files found in such a folder before are no longer offered the next time you look again; anything already backed up stays backed up. A library from an older version is described (how big, how much of it is copies of your photos and how much is previews) and left for you to decide about: Keevoro never removes it. To back up an old backup's photos somewhere else on purpose, choose its originals folder itself as a source.
- **Looking through a folder kept in OneDrive no longer downloads every file that is only online.** Files that are in OneDrive (or other online storage) but not on this computer are passed over unopened, and the log says how many; files found there before are still offered.
- **Keevoro fits a narrow window.** Below about 640 pixels wide (a small window, or the page zoomed in) the side bar becomes a band across the top, with the three steps side by side, which scrolls away with the page so it never covers what you are reading, and no screen spills off the side any more: the backup screen, the settings and the activity charts all fit down to 400 pixels, and wide tables scroll inside their own box.

## [0.33.0] - 2026-11-20

### Added

- **You can choose the kinds of file Keevoro looks for.** In Settings, under the new Advanced section, a table lists every ending Keevoro looks for (.jpg, .mov, .cr2 and so on), whether each is a photo or a video, whether it came with Keevoro or you added it, and a switch to look for it or not. Add an ending your camera uses that Keevoro does not know, such as .insp, and its files are found and backed up like any other. If Keevoro cannot open them, they are still copied and checked exactly, and simply show "No preview". The kinds Keevoro comes with can be switched off, or made photos or videos, and "Put back the built-in list" brings them all back. Changes count from the next time a source is looked through, and switching a kind off never takes away anything already found or backed up. The same is there from the command line as `keevoro kinds`, and an AI app can see the list but not change it.

- **Each earlier backup now opens to show everything Keevoro recorded about it.** Under "How earlier backups went", press Details on a row to see when it started and finished, how long it was paused, how it ended and whether it was started from the app, the command line or an AI app; every stage with how long it took, how much it did and its speed, including the slowest and fastest it held for a minute or more; how many of your files were sent, were already there or could not be sent, with the reasons; the duplicates left out and the room saved; the previews made and sent; where it went and whether that was across a network; and what it said at the end. The same is there from the command line with `keevoro history 1`, and an AI app can read it. Backups from before this version show what was kept for them.

### Fixed

- **Keevoro no longer leaves unfinished copies lying about in your backup folders.** A backup that was stopped, or lost its drive part-way, could leave Keevoro's half-made copies in a hidden working folder called `.keevoro-uploads`, sometimes many gigabytes, and nothing ever cleared them. Now each backup to that folder clears them away as it starts and ends, and says so ("Cleared 15.0 GB of unfinished copies left by an earlier backup that was stopped"). They were Keevoro's working files, never part of your backup: nothing that was backed up is touched, and anything written to in the last hour is left alone in case another backup is using it. Taking a folder off a backup clears it too, and for a folder no backup goes to any more, `keevoro tidy "D:\Backups"` does it.
- **Cloud storage is no longer left holding unfinished uploads** for files a backup has stopped sending, which took room and could cost money.
- **The pictures Keevoro keeps for showing photos quickly now take at most 1 GB** on your computer, the oldest cleared first; they are made again when needed.
- **Pause, Carry on and Stop now always reach a backup another Keevoro program is running, on Windows too.** Pausing from the command line while the app was backing up, say, could on rare occasions go unheard, because Windows sometimes holds a just-written file for a moment (while a virus checker looks at it) and Keevoro gave up on it instead of trying again.

## [0.32.0] - 2026-11-20

### Added

- **A backup now says how long the whole of it has left, and how long it has been running.** The backup screen leads with one line, such as "Running for 5 hours 12 minutes, about 3 hours left", and one bar for the whole backup, which no longer fills up and then starts again for the previews. Beneath it is a short list of every stage, the one in hand and those still to come ("Then: Backing up the previews"), so nothing comes as a surprise at the end. The time left is worked out afresh on a schedule that suits the backup (about five times an hour on a two-hour backup, at least once an hour on a day-long one, and at once when the speed really changes or a stage ends) and counts down smoothly in between. It is said roughly ("Roughly 6 hours left") when part of it is a guess, and not at all ("Working out how long this will take") until there is something to go on. Each backup remembers how fast things went on this computer, so the next one has a better first guess. Paused time does not eat into the time left, and "Running for" says how much of it was paused. `keevoro status`, `keevoro follow`, `keevoro run` and an AI app all give the same words.
- **Previews are made while your files are being backed up, not after.** On a long backup they are ready, or nearly, by the time the originals have all arrived, instead of being a long second wait. Your originals still come first: previews are only sent once every original is safely there. While your files are being sent, previews are made gently: at low priority, with fewer workers than before and never more than one reading from each source, one at a time while you are using the computer, and not at all while it is on battery or busy with something else. Pause and Stop reach them as before, whatever was made is kept, and switching previews off for a backup means none of this happens.

### Changed

- **The "Making previews" reason on the activity page** now says that during a backup they are made beside it, at low priority, and wait while the computer is on battery or busy.

### Fixed

- **While a backup runs, the screen no longer keeps saying how many files are waiting and how much room they need.** Those figures were from before it began, and stayed on show even when every file had been sent.

## [0.31.0] - 2026-11-19

### Added

- **An AI app, such as Claude Desktop or Claude Code, can now use Keevoro for you.** Ask it to find the photos on a drive and back them up, and it can, through `keevoro mcp`, which is part of the `keevoro` command. It is careful by design: out of the box it can only look at your backups, and it can look in no folder at all. You choose, in Settings under the new AI agents section or with `keevoro agents`, which folders it may look through and back up to, and whether it may make changes. Whatever you allow, it can never change anything on a source, delete a photo, forget a backup, reset Keevoro or change what it is allowed to do, and a storage key you give it goes to your computer's keychain and is never shown again. Everything it does is written in Keevoro's log. How to set it up is in docs/mcp.md.
- **`keevoro status` with a job's number says how that job ended,** once it has, whichever Keevoro program ran it.

## [0.30.1] - 2026-11-19

### Fixed

- **The progress bar of a backup keeps one length.** It used to stretch and shrink with the words under it, which made it look as if progress was going up and down.

## [0.30.0] - 2026-11-19

### Changed

- **The `keevoro` command line has been rebuilt to work on the same named backups as the app.** A backup started in the app is there on the command line, and the other way round. Everything the app can do, the command line can do too: `keevoro backups`, `keevoro look`, `keevoro collections`, `keevoro destinations add`, `keevoro run`, `keevoro check` and the rest. Name a backup with `-b "Family photos"`, or leave it out to use the one the app has open. A backup shows how it is getting on as it runs; `--detach` starts one and comes back at once, and `keevoro status`, `follow`, `pause`, `carry-on` and `stop` work on it from anywhere, even one the app started. Every command takes `--json` for scripts, and the exit code says what happened. The walk-through is in docs/command-line.md.
- **A backup to a folder keeps each photo's own "date modified".** It is sometimes the only clue to when a photo was taken.
- **Looking through a drive that is not plugged in** is now refused at once with "Is it plugged in?", instead of starting and finding nothing.

### Fixed

- **Pressing Stop while a backup is still getting ready** now ends with the usual "Stopped. Everything backed up so far is safe where it is…" message. Before, it ended with an empty one.
- **The app and the command line see each other's backups straight away.** A backup started in one used to appear in the other only after it was closed and opened again, and two changes made at the same moment could undo each other. Now both see every change at once, and neither loses the other's.
- **A file is never left out as a duplicate on a guess.** When two large files look alike and one of them cannot be read in full to tell them apart (because its drive is unplugged, say), neither is backed up until it can be, and Keevoro says so, instead of backing up only one of them.
- **A file already in a backup folder only counts as there if it matches exactly,** including a large file Keevoro had not yet read in full. Before, a file of the same name and size was believed.
- **The progress bar of a long backup no longer slips backwards now and then.** The amount done only ever goes up.

### Removed

- **The command line's old "library" commands are gone:** `keevoro gather`, `publish`, `connect`, `disconnect`, `scan` and every command that took `--library`. They copied your photos into a library folder of Keevoro's own before backing up. Keevoro no longer does that: photos go straight from your sources to where you back them up, as the app already does. Making MP4 copies of old videos (`--convert-videos`) and changing a backup's private link (`--new-link`) went with them.

## [0.29.3] - 2026-11-19

### Changed

- **Getting a big backup ready takes seconds, not minutes.** Before the first file moves, Keevoro works out where each file will go at the destination. With thousands of files this took several minutes, because each one was written to Keevoro's record on its own and each lookup read the whole record. It is now done as one change with instant lookups.
- **Keevoro says what it is doing while it gets ready.** The backup screen now names each step as it happens: "Getting ready: reading what Keevoro knows about this backup", "Telling apart files that look alike: 12 of 300", "Working out where each file goes: 2,500 of 11,245", and then the files themselves. Each counted step shows a bar and, once it has been watched for a few seconds, how long that step has left.
- **Coming back to a running backup shows its real step at once,** not a bare "Backing up…".
- **"What Keevoro is doing" says which step a backup is on.** Under "Backing up your files" there is now a "Right now:" line with the same words as the backup screen. A backup also shows there from the moment it is asked for, including while it gets ready.
- **Keevoro's own record is quicker throughout,** which helps every screen that counts files while a backup is writing.

### Fixed

- **"Stopped" no longer shows beside a backup that is running.** If you stopped a backup and started another, the earlier one's closing message stayed on screen and made it look as if the new one had stopped too. Starting a backup now clears it.

## [0.29.2] - 2026-11-19

### Changed

- **A backup starts copying straight away.** Before sending anything, Keevoro used to read every large file from start to finish to fingerprint it ("Reading each file in full first"), and then read it all over again to copy it. On a big backup that meant hours before the first file moved. Each file is now fingerprinted while it is being copied, in the same pass. Only files that look like another file are read beforehand, because those have to be told apart first: a true copy is left out and a lookalike is not. That step now reads "Telling apart files that look alike".
- **The background check for duplicates waits while a backup runs,** so the two are not reading the same files at once, and carries on afterwards with whatever the backup did not cover.
- **Large files sent to cloud storage before their fingerprint was known** carry no fingerprint label on the stored object. The fingerprint is still worked out and kept in Keevoro's own record.

## [0.29.1] - 2026-11-18

### Changed

- **A tidier list of sources.** Each source is now a card with an icon (a drive or a folder), its name, where it is beneath in small type, how many photos and videos are on it and their size, and its buttons. A whole drive that was only known by its letter, shown before as a bare "I:\\", now takes the name the computer gives it, such as "KINGSTON (I:)", or "Drive I:" when it is not plugged in.

## [0.29.0] - 2026-11-17

### Added

- **A backup can go to more than one place.** On the backup screen, "Add another destination" adds a second (or third) place beside the first: any mix of folders, drives, Cloudflare R2, Amazon S3 and other storage. "Back up now" sends to each in turn. A new destination starts empty and catches up on the next backup; nothing already sent elsewhere is sent again.
- **Each destination shows its own count.** With more than one, the screen lists them, each with how many files are there and how many are waiting, and a Remove button. Removing one only takes it off the backup's list. Nothing stored there is deleted.
- **If one destination cannot be reached, the others still get their backup.** An unplugged drive or a dropped connection is reported by name and passed over; it catches up next time.

### Changed

- **A photo counts as safe only when it is at every destination.** With one destination this is as before. With several, a photo that has reached one but not another is still shown as waiting, so a second copy that is half made is never mistaken for a whole one.
- **After changing where a backup goes, the counts are honest.** Before, a backup that was pointed somewhere new still showed everything as safe, because it had once been sent somewhere. It now counts what is at the destinations it has today.
- **"Check they all arrived" checks every destination**, and each earlier backup in the history is listed with the destination it went to.

### Removed

- **The downloads badge on the project page.** It counted every file fetched from every release, including the fetches Keevoro's own release process makes, so the figure said nothing true about how many people use it.

## [0.28.0] - 2026-11-17

### Added

- **A backup's figures in plain sight.** The Choose screen and the backup screen now open with large figures: how many photos and videos you are keeping and their size, how many duplicates were left out with the room that saves and what share of everything found they were, and how many pictures were set aside. Clicking the duplicates or set-aside figure opens its list.
- **Where each earlier backup went.** "How earlier backups went" has a new Where column, so after changing a backup's destination you can still see where the earlier ones were sent.

### Changed

- **What a backup said when it finished stays on screen.** The summary at the end of a backup (how many files, the previews, how long it took and the speed) used to vanish as soon as you looked at another screen. It is now kept with the backup and shown until the next backup replaces it, even after closing Keevoro. Changing the destination clears it, since it was about the old one.
- **The duplicates list says how much room was saved.**

## [0.27.2] - 2026-11-17

### Changed

- **Click a picture to open it; double-click to keep or leave it out.** Inside a collection, a single click now opens a photo or video large, as you would expect. To keep it or leave it out, double-click it or click the tick in its corner. Before, a click changed the tick and you had to hold down to look. The line under the heading says how it works now. In the large view the Keeping switch, the arrows and Escape work as before.

## [0.27.1] - 2026-11-17

### Changed

- **A file already at the destination is fingerprinted before it counts.** When a backup to a folder or drive found a file of the same name and size already there, Keevoro took it to be the same file. It now reads that file and compares its fingerprint. If it is a different file, yours is reported as not backed up, and what is there is left alone.
- **Files that could not be read are said on screen.** After looking through a source, Keevoro now says how many files it could not read and names the first few. They used to be listed only in the log.
- **The promises on the About page say exactly what Keevoro does.** Each was checked against the code. Four were worded more strongly than was true and have been made exact: which copies are read back and compared, that it is originals that are never replaced (previews and the list of contents are rewritten), where unreadable files are reported, and what Keevoro keeps on this computer.

## [0.27.0] - 2026-11-17

### Added

- **Back up the originals alone.** The backup screen has a new switch, "Also make previews for a gallery". It is on to begin with. Switch it off and only your original files are backed up, exactly as they are, with no thumbnails or other versions made, which is quicker. Switch it back on later and the previews are added then. Each backup remembers its own choice.
- **An About page, with what Keevoro promises.** "About Keevoro" in the side bar opens a page headed "Your photos are safe with Keevoro". It sets out the rules Keevoro never breaks: your sources are only ever read, a copy only counts when it is proved exact, nothing in a backup is ever replaced or removed, duplicates are exact, nothing is hidden, you never need Keevoro to get your photos back, and your photos go only where you send them. The page also has the origin of the name and the licence, with buttons to read the licence, the note on the name and logo, and the source.
- **A progress bar for every step of a backup.** The bar used to show only while original files were going, and vanished while previews were made and sent. There is now always a bar, counting files when it cannot count bytes.
- **The file just backed up is named.** Under the progress, "Just done:" shows the file Keevoro has just dealt with, so you can see what is going by.

### Changed

- **Every copy written to a folder or drive is read back and proved exact.** When Keevoro backs a file up to a folder, it now fingerprints what it reads from your source, writes it, makes sure it is on the disk, reads it back and fingerprints it again. If either does not match the fingerprint taken when the file was found, the copy is thrown away, the file is reported, and it is tried again next time. This makes backups to a folder somewhat slower, because each file is read back once. Backups to cloud storage are unchanged.
- **Several files at once to a network drive.** A backup to a folder on a file share, such as a mapped drive letter that is really storage elsewhere, now sends four files at a time, which is much quicker for many small files. A disk in the computer still gets one at a time. This is worked out on Windows; on a Mac a network folder is still treated as a disk.
- **"Your sources are only ever read" has moved** from the bottom of the side bar to the About page, where it is explained properly.

## [0.26.1] - 2026-11-17

### Changed

- **Smoother graphs.** The graphs on "What Keevoro is doing" jumped up and down like a saw, even when Keevoro was working steadily, because the computer reports what a program has read and written in lumps. Each point is now the average of the last five seconds, so a steady 8 MB/s looks steady. The line under each graph says so.

## [0.26.0] - 2026-11-17

### Added

- **One button to pick up new photos.** Under "Your sources" there is now "Look again for new photos". It looks through every source of the backup again, one after another, and the next backup sends only what is new.

### Changed

- **Backing up to cloud storage is several times quicker for many small files.** Keevoro sent one file at a time and waited for the far end to answer before starting the next, which is slow when there are thousands of small previews. It now sends six at a time to Cloudflare R2, Amazon S3 and the like. Backups to a folder or a drive still go one at a time, where sending several only makes the disk work harder. Stop and Pause let the files already on their way finish.

## [0.25.0] - 2026-11-17

### Added

- **Find the folders with photos, in seconds.** Not sure where on a drive your photos are? Choose the whole drive and pick "Find the folders with photos". Keevoro reads only the list of what is in each folder, never opening a file, so it gets through hundreds of thousands of files in a few seconds. It then shows the folders that seem to hold photos and videos, with how many and how big, each with a tick. Tick the ones you want and Keevoro looks through those properly, one after another.
- **Probable icons are spotted.** A folder of very small pictures with no videos is marked "Small pictures, probably not photos" and left unticked, so a program's own pictures do not sneak in.
- **What Keevoro is doing, in plain words, at a glance.** The side bar now says what Keevoro is busy with on every screen, under "What Keevoro is doing": for example "Making previews" or "Backing up your files and making thumbnails". Nothing is shown when it is idle.
- **More kinds of work are named.** "What Keevoro is doing" now also names making previews, making video previews, making thumbnails for the screen you are on and finding where your photos are, each with a sentence on why and what it costs your computer.
- **Keep all or keep none inside a collection.** Two buttons at the top of a collection keep or leave out everything in it at once.
- **See where a collection came from.** The folder, or folders, its files are in on your source are named at the top of a collection. Click one to open it.
- **`keevoro survey` on the command line.** The same quick survey, for those who prefer a terminal.

### Changed

- **Back takes you back to where you were.** Coming back from a collection, the duplicates list, Settings or "What Keevoro is doing" returns to the same place on the screen, not the top.
- **The top line of "What Keevoro is doing" says what, not just that it is working.**

### Fixed

- **Opening a collection could show a different one.** When photos from two separate occasions in the same month were both titled, say, "October 2004", opening either showed the first, and "Don't keep" on one could act on the other. Each now opens and keeps its own. Where two share a month they are named to the day, such as "2 to 3 October 2004" and "20 October 2004".

## [0.24.0] - 2026-11-17

### Added

- **Hold down on a picture to see it larger.** Inside a collection, or in the set-aside list, press and hold on any photo or video (or right-click it) and it opens screen-sized, without changing whether it is kept. From there you can leave it out or put it back, move to the next and previous with the arrow keys or the buttons, and close with Escape. A quick click still ticks and unticks as before. A video shows a still from it. The line under the heading now says so: "Hold down on one to see it larger."
- **About Keevoro.** Settings now says where the name comes from: the sound of English "keep" with "voro", an old Swedish word meaning "were". Keep things as they were.

### Changed

- **Mac and Linux versions come at milestones for now.** While Keevoro is changing quickly, each release is built for Windows only, and Mac and Linux versions are made at bigger milestones. A Mac that has Keevoro is not offered an update it cannot install.
- **Pictures on their way show a shimmer.** A thumbnail that has not arrived yet used to be a blank tile, which looked as if nothing was happening. It now shimmers gently until the picture appears.

## [0.23.0] - 2026-11-17

### Added

- **Every backup explains itself.** Keevoro now writes a plain note, "What is in this backup.txt", into the place it backs up to. It says what each folder is for: `originals` (your photos and videos, untouched), `renditions` (smaller versions made for looking at, with what thumb, display, poster and video each mean) and `index` (the list a gallery reads). It also says how to get your photos back without Keevoro.
- **The same explanation in the app.** The backup screen has a new "What is in the backup" line to open.
- **Look at the duplicates before you back up.** On the Choose screen, "N duplicates will be kept only once" now opens a list of every file found more than once. For each it shows the copy being kept and the folder it is in, and each copy left out and where that is. Click a folder to open it and see for yourself.
- **How Keevoro decides, in plain words.** The same screen explains that two files count as duplicates only when every byte matches, never on name, size or date, and marks any that are still being checked.

### Changed

- **The folder named while searching can be read.** "Now in ..." used to change many times a second, too fast to read. It now changes every second and a half at most. The count of files still moves as it happens.

### Fixed

- **The main button lines up with the buttons beside it.** "Back up now" sat a few pixels lower than its neighbours.

## [0.22.0] - 2026-11-17

### Added

- **A Memory graph.** "What Keevoro is doing" now charts how much memory Keevoro is holding over the last two minutes, beside Processor, Disk and Network. It should stay level, not climb.

### Changed

- **Backing up large files to a folder is much faster.** Any file over 64 MB going to a folder or a drive used to be written there in pieces and then written a second time to join the pieces, so a big video cost the destination twice the work. Each file is now read once and written once. Backups to cloud storage are unchanged, because there the pieces are what make a dropped connection recoverable.
- **Steadier progress on big files.** The speed and time left now move every few megabytes while a large file is being written, not once per piece.
- **"Send them now" is now "Back up now".** After a backup it reads "Back up again". The rest of the screen follows: the step is "Back them up somewhere safe", and it says "Backing up to the folder", "waiting to be backed up", "Back up somewhere else" and "How earlier backups went".
- **Buttons look like buttons.** Check they all arrived, Show me the folder, Back up somewhere else, Stop, Cancel, Remove, File types, Start a new backup and Open the log file were drawn as plain text. They are now proper buttons. Only things that take you somewhere (Back, Settings, What's new) are still drawn as links.

### Fixed

- **Coming back to a running backup no longer offers to start it again.** If you looked at another screen while a backup was running and came back, the screen offered to start it for a few seconds before catching up. It now shows the backup in progress, with its speed and Stop button, straight away.
- **A backup that finished while you were on another screen is counted.** Coming back used to say every file was still waiting, although they had all arrived. The counts are now read again whenever a backup ends, whichever screen is showing.

## [0.21.0] - 2026-11-17

### Added

- **A Speed setting.** Settings has a new switch, "Use this computer's own picture codecs", which is on unless you turn it off. Beneath it Keevoro says which ways of opening a photo it will try, in order, and how many previews it makes at once.

### Changed

- **Previews are made much faster.** Keevoro now makes several previews at a time instead of one after another. It uses half of your processor cores at most, and never more than four, so the computer stays free for you. Pause still stops them.
- **On Windows, photos are opened by Windows itself.** The codecs built into Windows are usually quicker than Keevoro's own decoder and use your graphics hardware where they can. If a photo will not open that way, Keevoro quietly uses its own decoder instead, so nothing is lost by leaving the switch on.
- **More kinds of photo get a preview on Windows.** HEIC photos from phones, and camera raw files Windows has a codec for, now get a real preview where before they had a placeholder. Your original files are sent exactly as they are either way.

## [0.20.0] - 2026-11-17

### Added

- **See how fast a send is going.** While files are being sent, the Send screen shows how much has gone out of how much, the speed, and roughly how long is left: "9.6 GB of 30.1 GB sent at 12.4 MB/s, about 28 minutes left". The speed is steadied over the last few seconds, and no time is guessed while nothing is arriving.
- **A summary when a send finishes.** It says how long it took, how much was sent and the average speed over the whole send.
- **How earlier sends went.** Each backup keeps a record of its last twenty sends. On the Send screen, "How earlier sends went" lists when each finished, how much it sent, how long it took and its average speed, so one can be compared with another.
- **A Network graph.** "What Keevoro is doing" now also charts what Keevoro is sending to cloud storage each second. Sending to a folder shows under Disk, since it does not use the network.

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

[Unreleased]: https://github.com/markcastle/keevoro/compare/v0.40.0...HEAD
[0.40.0]: https://github.com/markcastle/keevoro/compare/v0.39.1...v0.40.0
[0.39.1]: https://github.com/markcastle/keevoro/compare/v0.39.0...v0.39.1
[0.39.0]: https://github.com/markcastle/keevoro/compare/v0.38.0...v0.39.0
[0.38.0]: https://github.com/markcastle/keevoro/compare/v0.37.1...v0.38.0
[0.37.1]: https://github.com/markcastle/keevoro/compare/v0.37.0...v0.37.1
[0.37.0]: https://github.com/markcastle/keevoro/compare/v0.36.1...v0.37.0
[0.36.1]: https://github.com/markcastle/keevoro/compare/v0.36.0...v0.36.1
[0.36.0]: https://github.com/markcastle/keevoro/compare/v0.35.0...v0.36.0
[0.35.0]: https://github.com/markcastle/keevoro/compare/v0.34.0...v0.35.0
[0.34.0]: https://github.com/markcastle/keevoro/compare/v0.33.0...v0.34.0
[0.33.0]: https://github.com/markcastle/keevoro/compare/v0.32.0...v0.33.0
[0.32.0]: https://github.com/markcastle/keevoro/compare/v0.31.0...v0.32.0
[0.31.0]: https://github.com/markcastle/keevoro/compare/v0.30.1...v0.31.0
[0.30.1]: https://github.com/markcastle/keevoro/compare/v0.30.0...v0.30.1
[0.30.0]: https://github.com/markcastle/keevoro/compare/v0.29.3...v0.30.0
[0.29.3]: https://github.com/markcastle/keevoro/compare/v0.29.2...v0.29.3
[0.29.2]: https://github.com/markcastle/keevoro/compare/v0.29.1...v0.29.2
[0.29.1]: https://github.com/markcastle/keevoro/compare/v0.29.0...v0.29.1
[0.29.0]: https://github.com/markcastle/keevoro/compare/v0.28.0...v0.29.0
[0.28.0]: https://github.com/markcastle/keevoro/compare/v0.27.2...v0.28.0
[0.27.2]: https://github.com/markcastle/keevoro/compare/v0.27.1...v0.27.2
[0.27.1]: https://github.com/markcastle/keevoro/compare/v0.27.0...v0.27.1
[0.27.0]: https://github.com/markcastle/keevoro/compare/v0.26.1...v0.27.0
[0.26.1]: https://github.com/markcastle/keevoro/compare/v0.26.0...v0.26.1
[0.26.0]: https://github.com/markcastle/keevoro/compare/v0.25.0...v0.26.0
[0.25.0]: https://github.com/markcastle/keevoro/compare/v0.24.0...v0.25.0
[0.24.0]: https://github.com/markcastle/keevoro/compare/v0.23.0...v0.24.0
[0.23.0]: https://github.com/markcastle/keevoro/compare/v0.22.0...v0.23.0
[0.22.0]: https://github.com/markcastle/keevoro/compare/v0.21.0...v0.22.0
[0.21.0]: https://github.com/markcastle/keevoro/compare/v0.20.0...v0.21.0
[0.20.0]: https://github.com/markcastle/keevoro/compare/v0.19.0...v0.20.0
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
