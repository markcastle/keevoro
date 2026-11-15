# Keevoro

Find every photo and video on your old drives, keep the ones that matter, and back them up safely.

Keevoro looks through old laptops, USB sticks and backup disks, finds your photos and videos, ignores the junk and the duplicates, and gathers everything into one library that you can back up to cloud storage such as Cloudflare R2 or Amazon S3. It never changes anything on your old drives.

It is in early development. The engine and a command-line tool work today, from scanning a drive through to a verified backup on another disk; the desktop app for Windows and macOS comes next.

## Install

One line installs Keevoro, and running it again updates it. Nothing needs an administrator password.

**Windows** (PowerShell):

```powershell
irm https://raw.githubusercontent.com/markcastle/keevoro/main/install.ps1 | iex
```

**macOS and Linux:**

```sh
curl -fsSL https://raw.githubusercontent.com/markcastle/keevoro/main/install.sh | sh
```

Then:

```sh
keevoro --help
```

To move to the newest version later:

```sh
keevoro update
```

`keevoro update --check` only tells you whether there is one. Every download is checked against the release's checksum before it replaces anything. What changed in each version is in [CHANGELOG.md](CHANGELOG.md).

## Try the engine from source

You need [Rust](https://rustup.rs) 1.82 or newer. The examples below use `cargo run`; with Keevoro installed, replace `cargo run --release -p keevoro-cli --` with `keevoro`.

```sh
cargo run --release -p keevoro-cli -- scan "D:\" "E:\Old Laptop Backup"
```

Example output:

```text
Found 12 photos and 1 video (242.7 KB).
1 duplicate (21.8 KB) will be kept once.

Collections
  March 2012      3 photos
  Christmas 2011  December 2011 · 3 photos
  Cornwall        August 2008 · 5 photos · 1 video  (Holidays)
  Stuff           1 photo

Dates: 8 from camera data, 3 from file names, 2 guessed from file times.
Skipped 2 folders (system, app and hidden).
```

Add `--json` for the full details, or `--no-hash` for a quicker first look without duplicate checking.

Pictures that are clearly not memories (app icons, game artwork, web page leftovers, anything under 300 px) are listed separately with the reason, and left out of the collections. Nothing is ever hidden without saying so.

### Remembering a drive

A scan of a big old disk can take a while, so you can ask Keevoro to remember it. Give it a library folder and a friendly name:

```sh
cargo run --release -p keevoro-cli -- scan "E:\" --library "D:\Keevoro Library" --name "Dad's old Dell"
```

After that the drive can go back in the drawer. These work without it plugged in:

```sh
cargo run --release -p keevoro-cli -- collections --library "D:\Keevoro Library"
```

```sh
cargo run --release -p keevoro-cli -- sources --library "D:\Keevoro Library"
```

Scan a second drive into the same library and `collections` shows everything together, with copies that exist on both drives counted once. Scanning the same drive again replaces what was known about it rather than adding to it.

### Choosing and gathering

Everything found is kept unless you say otherwise. To leave a collection out, and to change your mind:

```sh
cargo run --release -p keevoro-cli -- skip --library "D:\Keevoro Library" "Stuff"
```

```sh
cargo run --release -p keevoro-cli -- keep --library "D:\Keevoro Library" "Stuff"
```

Then, with the old drive plugged in, gather the keepers into the library:

```sh
cargo run --release -p keevoro-cli -- gather --library "D:\Keevoro Library"
```

```text
Safely copied 9,870 files (41.0 GB) into your library.
```

Gathering copies each file, reads the copy back and compares it with the original before counting it. Your folders are kept (`originals/Holidays/Cornwall/...`); photos with no useful folder go under `originals/By date/2008/2008-08/`, and ones with no believable date under `originals/Undated/`. Nothing in the library is ever overwritten: two different photos called `IMG_0001.jpg` become `IMG_0001.jpg` and `IMG_0001 (2).jpg`.

It is safe to stop halfway, unplug the drive or run it twice. It carries on where it left off and never copies a file it already has. If a file can't be read, the rest still go through and the problem is listed at the end. It checks there is enough room before it starts.

Gathering also makes a thumbnail and a larger viewing copy of each photo, stored under `renditions/`. These are what a gallery shows, so pages load quickly. They are built from the picture alone: the location and camera details inside your originals are never copied into them. Some formats (HEIC from newer iPhones, camera RAW) can't be previewed yet; Keevoro says how many, and the originals are kept all the same.

### Videos

Each video gets a poster, a still picture for a gallery to show. Old camcorder and phone videos often will not play on a modern phone or in a browser, so you can also ask for an MP4 copy of those:

```sh
cargo run --release -p keevoro-cli -- gather --library "D:\Keevoro Library" --convert-videos
```

```text
Made posters for 140 videos.
Made MP4 copies of 96 videos (31 of them repackaged without re-encoding). The originals are kept.
```

Only videos that need it are converted; a modern MP4 is left exactly as it is. Where the video inside is already a modern kind in an old wrapper, it is repackaged rather than re-encoded, which takes seconds and loses nothing. Every MP4 is checked against its original (same length, plays through without errors) before it is kept, and the original always stays in the library.

This part needs [ffmpeg](https://ffmpeg.org/download.html) to be installed. Without it, videos are still found, gathered and backed up; Keevoro just says that posters and conversion were skipped.

### Backing up

To copy the library to an external drive or a network share:

```sh
cargo run --release -p keevoro-cli -- publish --library "D:\Keevoro Library" --to-folder "F:\Family Backup"
```

```text
Safely backed up 29,610 files (43.2 GB).
Index written for 9,870 files in 212 albums.
Your private index: file:///F:/Family%20Backup/index/q7Zp4xKm2Tf9LwR3bN8vYc.kvx
```

The backup has the same folders as the library, so what you see on one is what is on the other. An original that is already in the backup is never replaced, large videos are sent in pieces so an interruption only costs the piece in flight, and running it again sends only what is new.

The last thing written is the index: one JSON file listing every album and photo, with sizes, dates and where each thumbnail lives. Its name is long and random, so it can't be guessed. A second file, `index/gallery.json`, is the older format the current gallery site reads. If the backup will be served from a web address, add `--base-url https://photos.example.com` so the links inside the index point there. If you have shared the index's address and want it to stop working, add `--new-link`: the index is written under a new name and the old one is removed.

### Backing up to Cloudflare R2 or Amazon S3

The same command backs up to a bucket. First make a key that can read and write **that one bucket and nothing else** (in Cloudflare: R2, Manage API tokens, Object Read & Write, limited to the bucket). Put it where Keevoro will find it; keys are deliberately not accepted on the command line, where they would end up in your shell history:

```sh
export KEEVORO_ACCESS_KEY_ID="..."
```

```sh
export KEEVORO_SECRET_ACCESS_KEY="..."
```

Then, for R2, give your Cloudflare account ID and the bucket name:

```sh
cargo run --release -p keevoro-cli -- publish --library "D:\Keevoro Library" --to-r2 0123456789abcdef0123456789abcdef --bucket family-photos
```

For Amazon S3 use `--to-s3 eu-west-2` instead, and for MinIO or a NAS that speaks S3 use `--to-endpoint https://nas.local:9000`.

Keevoro checks the keys work before sending anything, and warns you if strangers are able to list the bucket's contents. The bucket itself is told never to replace an original, so even two computers backing up at once cannot overwrite one another. On a bucket the index's links are relative until you add `--base-url` with the address the bucket is served from.

This has been tested against MinIO, a stand-in that speaks the same language as R2 and S3. It has not yet been run against a real R2 or S3 bucket, so try it on a spare bucket first.

When the same photo turns up in several places, the copy that was filed most carefully (the deepest named folder, then the best date) decides where it goes.

What Keevoro remembers lives in one file, `.keevoro/catalog.sqlite`, inside the library folder. It is only working notes: it is safe to delete, and scanning again rebuilds it. Your old drives are never written to.

## How it is organised

| Crate | What it does |
| --- | --- |
| `keevoro-core` | The rules: what counts as a photo, how dates and album names are worked out, what to skip, what looks like junk, how collections are formed. No file access. |
| `keevoro-scan` | Looks through a drive or folder. Read-only. |
| `keevoro-collect` | Copies what you kept into the library and proves each copy is exact. |
| `keevoro-media` | Makes the thumbnails and viewing copies. |
| `keevoro-publish` | Sends the library to storage: resumable, and never replaces an original. |
| `keevoro-index` | Writes the index that describes a backup, and the JSON Schema other tools can check it against. |
| `keevoro-catalog` | Remembers what was found on each drive and what you chose to keep, in a small SQLite file inside the library. |
| `keevoro-storage` | The plug-in interface every storage provider implements, and the tests they must all pass. |
| `keevoro-storage-folder` | Stores the library on a local disk, external drive or NAS. |
| `keevoro-storage-s3` | Stores it in Cloudflare R2, Amazon S3 or anything else that speaks S3. |
| `keevoro-update` | Finds, checks and installs a newer version. |
| `keevoro-cli` | The `keevoro` command. |
| `xtask` | The project's own tooling: the changelog and cutting a release. |

## Contributing

Read [docs/developing.md](docs/developing.md) for building, testing and cutting a release, [PLANNING.md](PLANNING.md) for the design, [TASKS.md](TASKS.md) for what is next, and [AI_AGENT_PROMPT.md](AI_AGENT_PROMPT.md) for the rules every change must follow. Before opening a pull request:

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```
