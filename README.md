# Keevoro

Find every photo and video on your old drives, keep the ones that matter, and back them up safely.

Keevoro looks through old laptops, USB sticks and backup disks, finds your photos and videos, ignores the junk and the duplicates, and gathers everything into one library that you can back up to cloud storage such as Cloudflare R2 or Amazon S3. It never changes anything on your old drives.

It is in early development. The engine and a command-line tool work today; the desktop app for Windows and macOS comes next.

## Try the engine

You need [Rust](https://rustup.rs) 1.82 or newer.

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

When the same photo turns up in several places, the copy that was filed most carefully (the deepest named folder, then the best date) decides where it goes.

What Keevoro remembers lives in one file, `.keevoro/catalog.sqlite`, inside the library folder. It is only working notes: it is safe to delete, and scanning again rebuilds it. Your old drives are never written to.

## How it is organised

| Crate | What it does |
| --- | --- |
| `keevoro-core` | The rules: what counts as a photo, how dates and album names are worked out, what to skip, what looks like junk, how collections are formed. No file access. |
| `keevoro-scan` | Looks through a drive or folder. Read-only. |
| `keevoro-collect` | Copies what you kept into the library and proves each copy is exact. |
| `keevoro-catalog` | Remembers what was found on each drive and what you chose to keep, in a small SQLite file inside the library. |
| `keevoro-storage` | The plug-in interface every storage provider implements, and the tests they must all pass. |
| `keevoro-storage-folder` | Stores the library on a local disk, external drive or NAS. |
| `keevoro-cli` | The `keevoro` command. |

## Contributing

Read [PLANNING.md](PLANNING.md) for the design, [TASKS.md](TASKS.md) for what is next, and [AI_AGENT_PROMPT.md](AI_AGENT_PROMPT.md) for the rules every change must follow. Before opening a pull request:

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```
