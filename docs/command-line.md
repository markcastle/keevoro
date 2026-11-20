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

`keevoro places` lists the drives plugged in. On a big disk, a survey finds the folders that hold photos in seconds. It reads folder listings only and never opens a file:

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

See what backing up would do first, if you like. It changes nothing:

```sh
keevoro rehearse
```

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

## Cloud storage and keys

The same command adds cloud storage. First make a key that can read and write **that one bucket and nothing else** (in Cloudflare: R2, Manage API tokens, Object Read & Write, limited to the bucket). Then give the place and the bucket:

```sh
keevoro destinations add --r2 0123456789abcdef0123456789abcdef --bucket family-photos
```

For Amazon S3 use `--s3 eu-west-2`, and for MinIO or a NAS that speaks S3 use `--endpoint https://nas.local:9000`.

Keevoro needs the two keys once. It checks they work, keeps them in your computer's own keychain (Credential Manager on Windows, Keychain on a Mac), and never shows them, writes them down or puts them in its log or its `--json` answers. It takes them, in this order:

- **From standard input,** with `--keys-from-stdin`: the access key ID on one line and the secret on the next. Nothing ends up in your shell's history.
- **From the environment,** as `KEEVORO_ACCESS_KEY_ID` and `KEEVORO_SECRET_ACCESS_KEY`. These also win over the keychain when backing up, which suits scripts.
- **On the command,** as `--access-key-id` and `--secret-access-key`. These end up in your shell's history, so use them only on a computer that is yours alone.
- **At the keyboard,** asked for when none of the above is given. The secret is not shown as you paste it.

Keevoro warns you if strangers are able to list the bucket's contents. The bucket itself is told never to replace an original.

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
| 64 | The command was not understood. |

To try Keevoro out without touching your real backups, set `KEEVORO_HOME` to an empty folder. Keevoro then keeps its settings, its notes about each backup and its log under that folder instead.

## Every command

The words each command shows in `keevoro --help` come from the same list the app and the API use, so they always say the same thing.

### Backups

| Command | What it does |
| --- | --- |
| `keevoro backups` | Every backup, with its sources, destinations, and how many files are safe and waiting. |
| `keevoro backups start NAME` | Starts a new, empty backup; `--photos-only` or `--videos-only` for a backup of one kind. |
| `keevoro backups rename NAME NEW` | Gives a backup a new name. |
| `keevoro backups forget NAME` | Takes a backup off Keevoro's list. Nothing at any source or destination is touched. |

### Finding

| Command | What it does |
| --- | --- |
| `keevoro places` | The drives plugged into this computer. |
| `keevoro survey PLACE` | Finds the folders in a drive or folder that hold photos and videos, reading listings only. |
| `keevoro folders PLACE` | The folders inside a surveyed place that hold photos or videos, with how many each holds. |
| `keevoro files FOLDER [--from N] [--count N] [--sort name\|newest\|oldest] [--photos\|--videos]` | The photos and videos directly in one folder, a page at a time, by name or by the file's date, with each video's length. Nothing is added. |
| `keevoro look PLACE` | Looks through a drive or folder and adds it as a source. |
| `keevoro look --again [SOURCE]` | Looks through one source, or all of them, for what is new. |
| `keevoro sources` | The backup's sources, each with its number. |
| `keevoro sources forget SOURCE` | Takes a source off the backup. Nothing on it is touched. |
| `keevoro sources types SOURCE` | The kinds of file on a source, and which are included. |
| `keevoro sources types SOURCE --leave-out png` | Leaves out one kind of file; `--include png` brings it back. |
| `keevoro sources photos-or-videos SOURCE [backup, both, photos or videos]` | Whether one source follows its backup's choice of photos or videos, or has its own. |

A source is named by its number, its place or its name, as `keevoro sources` lists them.

### The kinds of file Keevoro looks for

One list for the whole of Keevoro, the same as Settings, Advanced, in the app. A change counts from the next time a source is looked through (`keevoro look --again`) or a place surveyed, and never takes away anything already found or backed up.

| Command | What it does |
| --- | --- |
| `keevoro kinds` | Every kind on the list: its ending, photo or video, built in or yours, and whether it is looked for. |
| `keevoro kinds add heif --photo` | Adds an ending to look for, as photos; `--video` for videos. |
| `keevoro kinds off avi` | Stops looking for a kind; `keevoro kinds on avi` looks for it again. |
| `keevoro kinds change xyz --video` | Makes a kind videos (or `--photo`), or gives one you added a new ending with `--to xyz2`. |
| `keevoro kinds remove xyz` | Takes an ending you added off the list. A built-in one can only be switched off. |
| `keevoro kinds reset` | Puts back the kinds Keevoro comes with, all on. Kinds you added stay. |

An ending is typed with or without its dot, in any case: `.HEIF` and `heif` are the same. It is letters and numbers only, and cannot be one for programs, documents, music or archives (such as `exe`, `pdf`, `mp3` or `zip`), which Keevoro never looks inside. A kind you add is found by its ending alone, so Keevoro backs it up exactly even when it cannot open it, with no preview.

### Choosing

| Command | What it does |
| --- | --- |
| `keevoro found` | What the backup holds: counts, size, duplicates left out and room saved, set aside. |
| `keevoro collections` | The collections, newest first, each kept or not. |
| `keevoro photos COLLECTION` | The files in one collection, each with its code, kept or not. |
| `keevoro keep COLLECTION` | Keeps a whole collection. |
| `keevoro skip COLLECTION` | Leaves out a whole collection. Nothing is deleted. |
| `keevoro keep --photo CODE` | Keeps one photo or video; `skip --photo CODE` leaves one out. |
| `keevoro set-aside` | Pictures set aside as probably not photos, each with the reason. |
| `keevoro duplicates` | Files found more than once: the copy kept, and the copies left out. |
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
| `keevoro rehearse` | What backing up now would do: how many files, how much, to where. Changes nothing. |
| `keevoro run` | Backs up to every destination, originals first. |
| `keevoro check` | Confirms what is at each destination. |
| `keevoro history` | How earlier backups went: when, where, how much, how long, numbered from the newest. |
| `keevoro history N` | Everything recorded about one of them, by its number. |
| `keevoro last-backup` | What the last backup said when it ended. |
| `keevoro photos-or-videos [both, photos or videos]` | Whether the backup is for photos and videos, photos only or videos only. It counts at once on what was found; nothing already backed up is touched. |
| `keevoro previews [on or off]` | Whether the backup also makes previews for a gallery, or sends the originals alone. |

`look`, `survey`, `run` and `check` each take `--detach`.

### Jobs

| Command | What it does |
| --- | --- |
| `keevoro status [JOB]` | What is running: the step, counts, how long it has run and has left, who started it. |
| `keevoro follow [JOB]` | Each thing a running job says, as it says it, until it ends. |
| `keevoro pause` | Makes the work in hand wait after the file it is on. |
| `keevoro carry-on` | Lets paused work go on from where it was. |
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
| `keevoro update` | Puts the newest command line in place of this one, checked against its release; `--check` only says whether there is a newer Keevoro. |
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
