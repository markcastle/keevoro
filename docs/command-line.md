# Keevoro on the command line

The `keevoro` command does everything the Keevoro app does, on the same backups. A backup you start in the app is there when you type `keevoro backups`, and one you start here shows up in the app. Use whichever suits you at the moment, or both at once.

The first half of this page walks through a first backup from start to finish. The second half is a reference to every command. For installing the command, see the [README](../README.md#-the-command-line). If you are working from source, replace `keevoro` with `cargo run -p keevoro-cli --` in the examples.

## A first backup, from start to finish

### Start a backup

A **backup** is one job you come back to, with a name: "Family photos", say, or "Drone footage". Each has its own sources, its own choices and its own places to back up to.

```sh
keevoro backups start "Drone footage"
```

```text
Started the backup "Drone footage".
Next, look through a source for it: keevoro look <a drive or folder> -b "Drone footage"
```

Most commands work on one backup. Name it with `-b`. Leave `-b` out and the backup the app has open is used, which is simply your backup when you only have one, so the rest of this walk-through leaves it out.

### Find where the photos are

A **source** is anywhere Keevoro looks for photos: a drive, a USB stick, a memory card, a folder. Your sources are only ever read. Nothing on them is changed, moved or deleted.

`keevoro places` lists the drives plugged in, and the folders OneDrive, Dropbox, Google Drive and iCloud Drive keep on this computer, by name. On a big disk, a survey finds the folders that hold photos in seconds. It reads folder listings only and never opens a file:

```sh
keevoro survey "D:\"
```

```text
Looked at 481,907 files in 36,412 folders in 6.2 seconds, without opening any of them.
  94.0 GB  333 photos and 26 videos  D:\Users\Mark\Videos\Drone
  61.0 GB  11,204 photos and 310 videos  D:\Users\Mark\Pictures
   9.0 MB  1,840 photos and 0 videos  D:\Games\Launcher\assets  (small pictures, probably not photos)
```

It goes by file names, so it is a good guess and not a promise. Keevoro remembers what it found, so you can look inside a suggestion before choosing it, without waiting again. Nothing is added or changed:

```sh
keevoro folders "D:\Users\Mark\Pictures"
keevoro files "D:\Users\Mark\Pictures\Camera Roll" --count 20
keevoro files "D:\Users\Mark\Pictures\Camera Roll" --videos --sort newest
keevoro picture --file "D:\Users\Mark\Pictures\Camera Roll\IMG_0001.jpg" --to look.webp
```

`folders` lists the folders inside that hold photos or videos, with how many each holds; `files` lists one folder's photos and videos a page at a time (`--from` and `--count`, at most 500), by name or with `--sort newest` or `--sort oldest` by each file's date (when it was last changed or copied, which is not always when it was taken), all of them or only `--photos` or `--videos`, with how long each video lasts when its file says (MP4, MOV, M4V and 3GP); `picture --file` saves a picture of any of them. The picture is kept in Keevoro's own cache, and a file only in OneDrive (or iCloud) is never opened.

The survey also remembers where the space goes: every folder and the biggest files, photos or not, so you can see what fills a drive (zip files, screen recordings, an old library) before choosing what to back up. Folders Keevoro never looks through, such as system folders, are measured last, once the suggestions are ready:

```sh
keevoro map "D:\Users\Mark\Pictures"
```

```text
Where the space goes in D:\Users\Mark\Pictures (mapped 2 minutes ago)
  44.0 GB  100%  Pictures  12,436 photos, 179 videos, 172 other files
  17.7 GB   40%    Texture packs  160 other files
   9.2 GB   21%    Keevoro Library  6,830 photos, 100 videos, 1 other file  (A library from an older Keevoro, never looked through)
   9.1 GB   21%    Captures  41 videos
By kind: photos 13.8 GB (31%), videos 12.4 GB (28%), everything else 17.7 GB (40%)
Of it, 17.1 GB is photos and videos Keevoro would back up.
```

`--folder` shows one folder inside the place, `--depth` how many levels of folders (1 to 6, 3 when left out), and `--kinds` lists the kinds of file by ending instead (`zip`, `mp4`, `jpg` and so on). Each line says what Keevoro would make of it for the backup in hand: a folder it made or never looks through, a folder you left out, or photos or videos the backup does not want, and why; which folder is a source of the backup; and how much of what it keeps from each folder is safely backed up ("backed up: 1,180 of 1,204"). It is answered from the survey, so nothing is walked or opened again; survey again for a fresh one. A map over an hour old, or one with a source in it looked through since, says so. Keevoro remembers at most eight maps, taking at most 200 MB between them, the oldest forgotten first; `keevoro forget-maps` forgets them all, touching nothing on any drive.

### Look through a source

```sh
keevoro look "D:\Users\Mark\Videos\Drone"
```

```text
Looked through Drone: found 333 photos and 26 videos.
```

While it looks, a line underneath says how far it has got. The source is now part of the backup. `keevoro sources` lists them, and `keevoro look --again` looks through them all again later for anything new.

### Choose what to keep

Everything found is kept, except pictures that are clearly not photos (app icons, game artwork, anything tiny), which are set aside with the reason. Nothing is hidden without saying so.

```sh
keevoro found
```

```text
Found 333 photos and 26 videos (94.0 GB).
12 duplicates left out, saving 1.1 GB.
40 pictures set aside as probably not photos. `keevoro set-aside` lists them.
359 files not yet safely backed up (92.9 GB).
```

Files are grouped into collections: your own folders, and moments by date for the rest.

```sh
keevoro collections
```

```text
Drone / Coast  June 2024 · 120 photos · 12 videos
Drone / Test flights  May 2024 · 213 photos · 14 videos
```

To leave a collection out, and to change your mind:

```sh
keevoro skip "Test flights"
```

```sh
keevoro keep "Test flights"
```

Leaving something out only means it is not backed up. Nothing is ever deleted. To choose photo by photo, list a collection; each line starts with a short code:

```sh
keevoro photos "Coast"
```

```text
9f2c4be1a7d03e55  DJI_0001.JPG  2024-06-12T10:15:00
41aa09cd22e1f0b7  DJI_0002.JPG  2024-06-12T10:16:41
```

Give a code to `keevoro skip --photo` or `keevoro keep --photo`. The same works for a picture that was set aside: a tiny scan of an old print may be the only copy there is. `keevoro duplicates` lists every file found more than once, with the copy kept and the copies left out. A copy is only left out when it matches byte for byte. To look at one, `keevoro picture 9f2c4be1a7d03e55 --large --to look.webp` saves a picture of it.

### Choose where to back up to

A **destination** is where a backup goes: an external drive, a network share, or cloud storage. A backup can have several, and is only called safe once it is at every one.

```sh
keevoro destinations add --folder "Y:\Drone footage"
```

```text
Keevoro checked the folder Y:\Drone footage works, and will back up there.
```

Keevoro checks every place works before adding it. For cloud storage, see [Cloud storage and keys](#cloud-storage-and-keys) below.

### Back up

See what backing up would do first, if you like. It sends nothing:

```sh
keevoro rehearse
```

Both this and `keevoro run` first look at what is really at each place you back up to, by name and size, so a backup folder emptied, swapped or cut short is said before anything is sent, and what has gone is sent again:

```text
The folder E:\Backup looks emptied or replaced: it no longer holds any of the 359 files Keevoro backed up there. They will be sent again.
```

A place that cannot be reached (a drive unplugged, a share offline, keys refused) is said to be out of reach, and nothing Keevoro knows about it is forgotten.

Then back up:

```sh
keevoro run
```

```text
Safely backed up 359 files (92.9 GB).
Previews added: 1,413 files (310.2 MB).
Index written for 359 files in 2 albums.
Your private index: index/q7Zp4xKm2Tf9LwR3bN8vYc.kvx
That took about 36 minutes: 93.2 GB at an average of 43.1 MB a second.
```

The originals go first, straight from your sources, each one proved exact: it is fingerprinted as it is read, and the copy is read back and compared before it counts. Previews for a gallery come after. An original already in the backup is never replaced, and running it again sends only what is new. If you would rather send the originals alone, `keevoro previews off`.

While it runs, a line underneath says the step it is on. It is safe to stop at any time: press Ctrl-C, or type `keevoro stop` in another window, and it carries on from there next time.

### Check it all arrived

Disks fail quietly. To be sure everything is where it should be:

```sh
keevoro check
```

```text
Your backup: all 359 originals are there.
```

`keevoro history` lists how earlier backups went, numbered from the newest, and `keevoro history 1` shows everything recorded about one of them: each stage with how long it took and how fast it went, what each destination was sent, already had or could not be sent and why, the previews, and what it said at the end. `keevoro last-backup` repeats what the last one said.

## Running in the background

A long backup does not have to keep a window open. `--detach` starts it as a program of its own and comes back at once:

```sh
keevoro run --detach
```

```text
Backing up "Drone footage", in the background (job 4123000001). See how it is getting on with: keevoro follow
```

Then, from any window:

- `keevoro status` says what is running, the step it is on, how long it has been running, and who started it. For a backup it says how long the whole of it has left and lists every stage: done, now, and still to come. `keevoro run` and `keevoro follow` show the same line as they go.
- `keevoro follow` shows it as it goes, until it ends.
- `keevoro pause` makes it wait once the file in hand is done, and `keevoro carry-on` lets it go on.
- `keevoro stop` stops it once the file in hand is done. Everything done so far is kept.

These work whichever program is running the job: the app, the command line, or another window. Only one program at a time works on a backup, so if the app is already backing it up, `keevoro run` says so and does nothing more.

After a search or a backup, Keevoro checks for duplicates in the background: it reads in full the large files a quick search only sampled. `keevoro status` shows it ("Checking "Family" for duplicates", with how far it has got) in the program running it. `keevoro pause JOB` with its number pauses only that check while everything else carries on, `keevoro carry-on JOB` lets it go on, and `keevoro stop JOB` stops it. A check you stop does not start again by itself until your next search or backup on that backup; `keevoro found` says how many files are not checked yet and what that means, and `keevoro check-duplicates` checks now. Nothing is backed up twice or left out because of it: before sending, a backup reads in full any file that looks like another. A check the app is running is not seen from the command line, as it holds no lock on the backup; stop or pause it in the app.

## Cloud storage and keys

The same command adds cloud storage. First make a key that can read and write **that one bucket and nothing else** (in Cloudflare: R2, Manage API tokens, Object Read & Write, limited to the bucket). Then give the place and the bucket:

```sh
keevoro destinations add --r2 0123456789abcdef0123456789abcdef --bucket family-photos
```

For Amazon S3 use `--s3 eu-west-2`, and for MinIO or a NAS that speaks S3 use `--endpoint https://nas.local:9000`.

Keevoro needs the two keys once. It checks they work, keeps them in your computer's own keychain (Credential Manager on Windows, Keychain on a Mac), and never shows them, writes them down or puts them in its log or its `--json` answers. It takes them, in this order:

- **From standard input,** with `--keys-from-stdin`: the access key ID on one line and the secret on the next. Nothing ends up in your shell's history.
- **On the command,** as `--access-key-id` and `--secret-access-key`. These end up in your shell's history, so use them only on a computer that is yours alone.
- **From the environment,** as `KEEVORO_ACCESS_KEY_ID` and `KEEVORO_SECRET_ACCESS_KEY`, only when no keys are given either of those ways. Keevoro then says it used them, naming the variables (never the keys), and that it saved them in the keychain for that destination; the log says the same, and so does `--json`, as `keys`.
- **At the keyboard,** asked for when none of the above is given. The secret is not shown as you paste it.

Keys you give are always the ones checked and saved, whatever the environment holds. When backing up, the keys saved for a destination are used; the environment's are used only for one with none saved, and the backup says so.

Keevoro warns you if strangers are able to list the bucket's contents. The bucket itself is told never to replace an original.

### Cloud storage as a source

A bucket, or a folder in one, can be a source too: to move an old cloud backup somewhere cheaper, merge two, or bring one down to a drive. It is only ever read. Make a key that can only **list and read** that bucket, then:

```sh
keevoro sources add-bucket --s3 eu-west-2 --bucket old-family-backup --prefix Photos --keys-from-stdin
```

```text
Keevoro can read the folder Photos in the Amazon S3 bucket old-family-backup, and added it as the source "Photos". Nothing in it will ever be changed.
Its keys are kept in this computer's keychain, and nowhere else.
Look through it next, which only lists it and downloads nothing: keevoro look --again "Photos"
```

The keys are taken the same four ways as above, checked by listing the bucket (which changes nothing), and kept in the keychain under the source's own name. Looking through it reads only the bucket's listing; nothing is downloaded. A source bucket keeps its folders as they are in the backup unless you choose otherwise with `keevoro sources arrange`. When it is backed up, each file is brought through this computer, checked, sent, and removed from this computer again, one or two at a time, and only when there is room for it with 2 GB to spare. `keevoro backup --dry-run` says first about what reading it out will cost, for example "Reading 120 GB out of Amazon S3 costs about £9 at Amazon’s usual price; Cloudflare R2 charges nothing for this." Files in Amazon's archive storage are said, with how much they come to, and are not backed up until they are restored. `keevoro sources forget` takes it off the backup and its keys out of the keychain; nothing in the bucket is touched.

## For scripts

Every command takes `--json` and then answers with exactly what Keevoro's API answers, the same the app receives. A job (`look`, `survey`, `run`, `check`) prints a line saying how it started, then one line for each thing it says as it goes, the same events the app hears, the last one last. Progress and problems go to standard error, and what was asked for to standard output.

When nobody is watching, as when the output goes to a file, progress is a plain line for each new step instead of one line rewritten in place.

The exit code says what happened, without reading the words:

| Code | Meaning |
| --- | --- |
| 0 | It worked. A backup that was stopped also ends with 0, and says "Stopped". |
| 1 | It ran, but some files could not be backed up or need attention, or something else went wrong. |
| 2 | A destination could not be reached: a drive unplugged, a connection down, keys that do not work. |
| 3 | No such backup, source, collection, photo or job, or no backup chosen yet. |
| 4 | Something is already being done to that backup, here or in another program. |
| 5 | Refused, such as a destination without the room, or nowhere to back up to yet. |
| 6 | A backup that ran by itself (`keevoro run --unattended`) held something back for your say. Other things may still have been sent. |
| 7 | A backup that runs by itself did not run now: this computer was on battery or busy, and `--wait` ran out. |
| 64 | The command was not understood. |

When more than one thing happened, the most serious decides: 2, then 5, then 1, then 6, then 7.

### For a system scheduler

`keevoro run --unattended` backs up the way a schedule does, for those who would rather use their computer's own scheduler (Task Scheduler on Windows, launchd on a Mac, a systemd timer or cron on Linux). Keevoro itself never writes anything into the system's scheduler. Each time it:

1. Waits while this computer is on battery or busy, for at most `--wait MINUTES` (none by default), and otherwise ends with code 7: "Not backed up now. Waiting while this computer is on battery."
2. Looks through every source again. A source that is not plugged in is passed over and said, and what was found on it is kept. A different drive where a source's drive was is passed over and said, never taken for it.
3. Holds back what needs your say. A backup never asked how to arrange its files, or one whose reading from cloud storage would cost about £1 or more, sends nothing. New pictures set aside as probably not photos are not sent (as always) and are listed; new copies of other files are sent once (as always) and listed.
4. Sends what is new. A backup drive that looks emptied or swapped for another is not sent to: "Keevoro has not sent anything there. Open Keevoro to decide."
5. Says what waits for your say under "Waiting for your say:", ends with code 6 if anything does, and writes it all into the backup's history (`keevoro history 1`).

Only one backup runs by itself at a time on a computer; a second one asked for meanwhile ends with code 4. It works at a low priority, and so does the check for duplicates that follows it, which also gives way while the computer is on battery or busy.

A daily run at 02:30 in Task Scheduler, on Windows, with the `keevoro` command installed (`install.ps1` puts it on your PATH):

```bat
schtasks /Create /SC DAILY /ST 02:30 /TN "Keevoro Family photos" /TR "keevoro run --unattended -b \"Family photos\" --wait 60"
```

As a systemd user timer on Linux, `~/.config/systemd/user/keevoro.service` and `keevoro.timer`:

```ini
[Service]
Type=oneshot
ExecStart=%h/.local/bin/keevoro run --unattended -b "Family photos" --wait 60

[Timer]
OnCalendar=*-*-* 02:30
Persistent=true

[Install]
WantedBy=timers.target
```

With cron, `30 2 * * * $HOME/.local/bin/keevoro run --unattended -b "Family photos"`.

To try Keevoro out without touching your real backups, set `KEEVORO_HOME` to an empty folder. Keevoro then keeps its settings, its notes about each backup and its log under that folder instead.

## Every command

The words each command shows in `keevoro --help` come from the same list the app and the API use, so they always say the same thing.

### Backups

| Command | What it does |
| --- | --- |
| `keevoro backups` | Every backup, with its sources, destinations, and how many files are safe and waiting. |
| `keevoro backups start NAME` | Starts a new, empty backup, for everything Keevoro looks for; `--wants photos` (or `videos`, `others`, several with commas) for a backup of only some kinds; `--arrange folders` to keep its sources' folders as they are rather than arrange it by date. |
| `keevoro backups rename NAME NEW` | Gives a backup a new name, of at most 60 letters and not one another backup has. Refused while it is being searched, backed up or checked. |
| `keevoro backups forget NAME [--dry-run]` | Removes a backup from Keevoro: its record on this computer goes, and its own folder of previews and working files is set aside for a week, then cleared away. Nothing at any source or destination is touched. `--dry-run` only says what it would do. Refused while it is being searched, backed up or checked. |
| `keevoro backups removed` | The backups removed in the last week, which can still be put back. |
| `keevoro backups put-back NAME` | Puts back a backup removed in the last week, exactly as it was. |
| `keevoro backups clear-away NAME` | Clears away a removed backup now instead of a week after: its own folder on this computer, and storage keys no other backup uses. It can no longer be put back. |

### Finding

| Command | What it does |
| --- | --- |
| `keevoro places` | The drives plugged into this computer, and the folders OneDrive, Dropbox, Google Drive and iCloud Drive keep on it. |
| `keevoro survey PLACE` | Finds the folders in a drive or folder that hold photos and videos, reading listings only. |
| `keevoro folders PLACE` | The folders inside a surveyed place that hold photos or videos, with how many each holds. |
| `keevoro files FOLDER [--from N] [--count N] [--sort name\|newest\|oldest] [--photos\|--videos\|--others]` | The photos and videos (and other files carried along) directly in one folder, a page at a time, by name or by the file's date, with each video's length. Nothing is added. |
| `keevoro map PLACE [--folder FOLDER] [--depth N] [--kinds]` | Where the space goes in a surveyed place: every folder and the biggest files, photos or not, by size, or with `--kinds` by ending. Nothing is opened or changed. |
| `keevoro forget-maps` | Forgets every survey Keevoro remembers, and so every map. Nothing on any drive is touched. |
| `keevoro look PLACE` | Looks through a drive or folder and adds it as a source. |
| `keevoro look --again [SOURCE]` | Looks through one source, or all of them, for what is new. A source that is not plugged in is passed over and said ("KINGSTON (E:) is not plugged in, so it was not looked through. What was found on it before is kept."). A source whose place holds a different drive from the one looked through there before (another card in the same reader) is passed over and said too, never replaced. |
| `keevoro look --again SOURCE --different-drive anyway\|new` | Answers for that source: `anyway` looks through the drive there as the source, replacing what was found on it; `new` keeps the source as it was, set aside until its own drive is plugged in again, and adds the drive there as a new source. |
| `keevoro sources add-bucket --r2 ID\|--s3 REGION\|--endpoint URL --bucket NAME [--prefix FOLDER] [--name NAME]` | Adds cloud storage as a source, after checking its keys can read it. Nothing in it is ever changed. Then `keevoro look --again NAME` lists it. |
| `keevoro sources` | The backup's sources, each with its number. |
| `keevoro sources forget SOURCE` | Takes a source off the backup. Nothing on it is touched. |
| `keevoro sources types SOURCE` | The kinds of file on a source, and which are included. |
| `keevoro sources types SOURCE --leave-out png` | Leaves out one kind of file; `--include png` brings it back. |
| `keevoro sources arrange SOURCE [backup \| by-date \| folders]` | How one source's photos and videos are arranged: the same as its backup (`backup`), or its own. Two sources whose folders have the same name, both kept as they are, share one folder in the backup, and both say so ("In the backup, its folder Photos is shared with E:\Photos."). |
| `keevoro sources wants SOURCE [backup \| KINDS]` | What one source is for: the same as its backup (`backup`), or its own choice of one or more of `photos`, `videos` and `others`. |
| `keevoro sources leave-out FOLDER [--put-back]` | Leaves a folder out of this backup, so it is not looked through and what was found in it is no longer offered; `--put-back` includes it again. It need not be in a source yet. Nothing in it, or already backed up, is touched. |

A source is named by its number, its place or its name, as `keevoro sources` lists them.

### The kinds of file Keevoro looks for

One list for the whole of Keevoro, the same as Settings, Advanced, in the app. A change counts from the next time a source is looked through (`keevoro look --again`) or a place surveyed, and never takes away anything already found or backed up.

| Command | What it does |
| --- | --- |
| `keevoro kinds` | Every kind on the list: its ending, photo, video or other, built in or yours, and whether it is looked for. |
| `keevoro kinds add heif --photo` | Adds an ending to look for, as photos; `--video` for videos. |
| `keevoro kinds add flac --other` | Adds an ending as other files, carried along: music, documents, anything. Keevoro will back these up exactly, but cannot show them or group them by date. |
| `keevoro kinds off avi` | Stops looking for a kind; `keevoro kinds on avi` looks for it again. |
| `keevoro kinds change xyz --video` | Makes a kind videos (or `--photo`, or `--other`), or gives one you added a new ending with `--to xyz2`. |
| `keevoro kinds remove xyz` | Takes an ending you added off the list. A built-in one can only be switched off. |
| `keevoro kinds reset` | Puts back the kinds Keevoro comes with, all on. Kinds you added stay. |

An ending is typed with or without its dot, in any case: `.HEIF` and `heif` are the same. It is letters and numbers only. One for programs, documents, music or archives (such as `exe`, `pdf`, `mp3` or `zip`) is never a photo or a video, so it can only be added with `--other`. A kind you add is found by its ending alone, so Keevoro backs it up exactly even when it cannot open it, with no preview.

### Other files, carried along

Once a kind is on the list as other files, a search finds them (reading each only to fingerprint it), and they are backed up and proved exactly like photos, into `originals/Other files`, in the folders they came from. They are listed by those folders, never by date: `keevoro collections` lists them last, under "Other files", each folder kept or left out as a whole with `keevoro keep "Other files / Mum / Documents"` (or `skip`), and `keevoro photos "Other files / Mum / Documents"` lists its files, each with its size and when it last changed, to keep or skip one by one with `--photo CODE`. Counts name them: "30 photos, 2 videos and 40 other files". `keevoro files FOLDER --others` lists only the other files in a folder, and `keevoro wants photos videos` leaves them out of a backup while keeping the kind on the list. Nothing is an other file until you add a kind: out of the box, Keevoro finds exactly what it always did. Three system files are never taken, whatever is on the list: Windows' `pagefile.sys`, `hiberfil.sys` and `swapfile.sys`, and the Mac's `sleepimage`.

### Choosing

| Command | What it does |
| --- | --- |
| `keevoro found` | What the backup holds: counts, size, duplicates left out and room saved, set aside, and, when you stopped the check for duplicates, how many files it has not checked and what that means. |
| `keevoro collections` | The collections, newest first, each kept or not. |
| `keevoro photos COLLECTION` | The files in one collection, each with its code, kept or not. |
| `keevoro keep COLLECTION` | Keeps a whole collection. |
| `keevoro skip COLLECTION` | Leaves out a whole collection. Nothing is deleted. |
| `keevoro keep --photo CODE` | Keeps one photo or video; `skip --photo CODE` leaves one out. |
| `keevoro set-aside` | Pictures set aside as probably not photos, each with the reason. |
| `keevoro duplicates` | Files found more than once: the copy kept, and the copies left out. |
| `keevoro check-duplicates` | Checks for duplicates now, in the background, even after the check was stopped. |
| `keevoro picture CODE --to FILE` | Saves a small picture of one file; `--large` for a screen-sized one. |
| `keevoro picture --file PATH --to FILE` | The same for any photo or video, by its path, found or not. Only Keevoro's cache is written. |

A collection is named by its whole name, such as "Holidays / Cornwall", or by its last part, "Cornwall", when only one has it.

### Destinations

| Command | What it does |
| --- | --- |
| `keevoro destinations` | Where the backup goes, numbered, and what has reached each place. |
| `keevoro destinations add --folder FOLDER` | Checks a folder works, then adds it. |
| `keevoro destinations add --r2 ACCOUNT --bucket NAME` | The same for cloud storage, with `--s3 REGION` or `--endpoint URL` for others. |
| `keevoro destinations remove DESTINATION` | Stops backing up there, by its number, folder or name; `--all` for every one. Nothing backed up there is touched; Keevoro's own unfinished copies there are cleared. |
| `keevoro room` | What is waiting for each place, and whether it has the room. |
| `keevoro tidy FOLDER [--dry-run]` | Clears away the unfinished copies a stopped backup left in a backup folder's working folder (`.keevoro-uploads`), whether or not a backup still goes there, and says what was cleared and what was left and why. Nothing that was backed up is touched; anything written to in the last hour is left, in case it is in use. `--dry-run` only says what it would clear. The app has the same as a button beside a folder destination, and in Settings, Advanced. |

### Backing up

| Command | What it does |
| --- | --- |
| `keevoro rehearse` | What backing up now would do: how many files, how much, to where, after looking at what is really at each place. Sends nothing. |
| `keevoro run` | Backs up to every destination, originals first. |
| `keevoro run --unattended [--wait MINUTES]` | Backs up by itself, as a schedule does, for a system scheduler: waits while the computer is on battery or busy, looks through the sources again, holds back what needs your say, and sends what is new. See [For a system scheduler](#for-a-system-scheduler). |
| `keevoro check` | Confirms what is at each destination. |
| `keevoro history` | How earlier backups went: when, where, how much, how long, numbered from the newest. |
| `keevoro history N` | Everything recorded about one of them, by its number. |
| `keevoro last-backup` | What the last backup said when it ended. |
| `keevoro arrange [by-date or folders]` | How the backup arranges photos and videos where it sends them: `by-date` (a folder for each year and month, keeping folders with real names) or `folders` (the same folders and names as on the source, starting with the source's own folder). It only changes where files go from now on: nothing already backed up is moved, sent again or counted missing. A backup made before this choice existed is by date. Until you choose, it says what Keevoro suggests and why ("Suggested because the photos in Family Scans look like scans."). |
| `keevoro wants [KINDS]` | What the backup is for: one or more of `photos`, `videos` and `others` (other files, once a kind of them is on the list), as words of their own or with commas. It counts at once on what was found; nothing already backed up is touched. |
| `keevoro previews [on or off]` | Whether the backup also makes previews for a gallery, or sends the originals alone. |

`look`, `survey`, `run` and `check` each take `--detach`.

### Jobs

| Command | What it does |
| --- | --- |
| `keevoro status [JOB]` | What is running: the step, counts, how long it has run and has left, who started it. |
| `keevoro follow [JOB]` | Each thing a running job says, as it says it, until it ends. |
| `keevoro pause [JOB]` | Makes the work in hand wait after the file it is on; with the number of a check for duplicates, only that check. |
| `keevoro carry-on [JOB]` | Lets paused work go on from where it was; with a job's number, only a check for duplicates paused on its own. |
| `keevoro stop [JOB]` | Stops a job after the file in hand. Nothing is lost. |

Without `-b`, `status`, `follow` and `stop` cover every backup; with it, only that one.

### Keevoro itself

| Command | What it does |
| --- | --- |
| `keevoro about` | Keevoro's version, what it promises, and its licence. |
| `keevoro activity` | What Keevoro is doing now and how hard it is working. |
| `keevoro log [--lines N]` | The record of what Keevoro did. |
| `keevoro settings` | Keevoro's settings. |
| `keevoro settings set own-codecs off` | Stops using this computer's own picture codecs; `on` to use them again. |
| `keevoro update` | Puts the newest command line in place of this one, checked against its release; `--check` only says whether there is a newer one for this computer, the same question `keevoro update` asks before it installs. |
| `keevoro reset` | Puts Keevoro back as it was when first installed. Asks first, or give `--yes`. Nothing on a source or at a destination is touched. |
| `keevoro agents` | Which folders an AI agent may look through and back up to, and whether it may make changes. |
| `keevoro agents allow --look FOLDER --back-up-to FOLDER` | Says which folders an agent may use. Give either more than once for several; `--none` for none. |
| `keevoro agents allow-changes on` | Lets an agent make changes; `off` lets it only look. |
| `keevoro mcp` | Starts Keevoro's server for an AI agent, which speaks the Model Context Protocol on standard input and output. An AI app starts it itself; you do not type it. |

## AI agents

An AI app, such as Claude Desktop or Claude Code, can use Keevoro for you through `keevoro mcp`. Out of the box it may only look, and may use no folder at all. You decide what more it may do:

```sh
keevoro agents allow --look "D:\" --back-up-to "F:\Family Backup"
keevoro agents allow-changes on
```

`keevoro agents` shows what is allowed now, and Settings in the app shows the same. The folders given replace those allowed before, so name them all each time; `--none` takes them all away. A change counts at once, even while an agent is at work. An agent can read what it is allowed, but can never change it. How to set up an AI app, and what an agent can and cannot do, is in [docs/mcp.md](mcp.md).

## Where Keevoro keeps its things

Each backup has a folder of Keevoro's own, in Keevoro's data folder on this computer, holding its notes (`.keevoro/catalog.sqlite`) and the previews it makes until they are sent. It is working space, never a copy of your photos. Your photos are only ever in their sources and at the destinations you chose. What a backup looks like at a destination is explained in the note "What is in this backup.txt" that Keevoro writes into each one.
