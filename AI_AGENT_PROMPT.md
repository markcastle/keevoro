# Keevoro: Instructions for AI Coding Agents

Read this before changing anything. Then read [PLANNING.md](PLANNING.md) for the why and [TASKS.md](TASKS.md) for what to do next.

## What you are building

Keevoro rescues family photos and videos from old drives for people who are not technical. Treat every source drive as the only copy of someone's memories. A bug that loses or alters a file is the worst possible outcome, far worse than a crash or a missed feature.

## Non-negotiable rules

1. **Never write to a source.** Scanning and collecting open source files read-only. No renames, no metadata writes, no temp files on the source drive.
2. **Never overwrite or delete an original** in a backup, whether a folder or a bucket. Use `PutOptions { create_only: true, .. }` for originals.
3. **Verify every copy** by re-reading and comparing the BLAKE3 hash before counting it as collected.
4. **Nothing leaves the machine** except to storage the user connected. No telemetry, no third-party APIs for image analysis.
5. **No `unsafe`.** It is forbidden at the workspace level.
6. **No `unwrap` or `expect` outside tests.** Return errors. Problems with one file never stop a whole scan; they go in the report.

## How to work

- **TDD.** Write the failing test first. Coverage must stay at or above 90% overall and 95% for copy, verify and storage code.
- **One task at a time** from TASKS.md, top to bottom within the current phase. Tick it in the same commit.
- **Say what changed, for the user.** Anything a person using Keevoro would notice goes under **Unreleased** in [CHANGELOG.md](CHANGELOG.md) in the same commit, under Added, Changed, Fixed or Removed. The heading decides the next version number (Added or Removed makes a minor release), and the words become the release notes, so write what they will see, not which function moved. See [docs/developing.md](docs/developing.md#cutting-a-release).
- **Nothing a person set may be lost by an update.** Every change to what Keevoro keeps (a catalog, `settings.json`, anything in its data folder) comes with a conversion and a test that an older catalog and settings file open with everything in place. See [docs/developing.md](docs/developing.md#when-what-keevoro-keeps-changes).
- **Keep the core pure.** Rules about names, dates, folders, skipping and collections belong in `keevoro-core` with no I/O. I/O crates call into it.
- **Do not add a crate before a task needs it** (YAGNI). When you add one, add it to the table in PLANNING.md.
- **Storage providers** implement `ObjectStore` and `MultipartStore` and must pass `keevoro_storage::conformance::run_all`. Never make the engine depend on a provider SDK directly.
- **Cross-platform from the start.** Code must build and pass tests on Windows, macOS and Linux. Use `std::path` APIs, never string-split paths with `/` except for object keys.

## Commands

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p keevoro-cli -- survey <path>
```

All four must succeed before you finish.

## Style

- Plain names over clever ones. Small functions. Comments explain why, not what.
- Every module starts with a short doc comment saying what it is for, in human terms.
- User-facing text is plain English for non-technical people: "Couldn't read 2 files", never "IO error". No em dashes.
- Call the places Keevoro looks for photos **sources**, never "old drives": "Add a source", "Your sources are only ever read". A source can be a drive, a USB stick or a folder. Say what one is the first time a screen mentions it.
- Use British spelling in prose and user-facing text (organise, colour), and Rust conventions in code.

## The user experience bar

The design reference is [docs/design/discover-mockup.png](docs/design/discover-mockup.png): calm, spacious and "very Apple". When a choice affects what the user sees or reads, pick the option a non-technical grandparent would understand without help. Hide anything technical behind an "Advanced" door.

## Common mistakes to avoid

- Trusting file extensions. Detect by content first; extensions are a fallback for formats without magic bytes.
- Treating a file's modified time as a capture date. It is usually the date it was copied. It is the weakest date source and is not used to place files into moments.
- Flattening the user's folder structure. Their folders are their organisation; keep it.
- Inventing time zones for old photos. Capture times are local wall-clock time.
- Scanning one drive with many parallel readers. Spinning disks slow down badly; parallelise CPU work, not disk reads.
- Following symlinks or junctions during a scan. They cause loops and double counting.
