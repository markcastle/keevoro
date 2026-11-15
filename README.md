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

## How it is organised

| Crate | What it does |
| --- | --- |
| `keevoro-core` | The rules: what counts as a photo, how dates and album names are worked out, what to skip, how collections are formed. No file access. |
| `keevoro-scan` | Looks through a drive or folder. Read-only. |
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
