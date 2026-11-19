# Developing Keevoro

Everything you need to build it from source, test it, and cut a release. For why the code is shaped the way it is, read [PLANNING.md](../PLANNING.md); for the rules every change must follow, read [AI_AGENT_PROMPT.md](../AI_AGENT_PROMPT.md).

## Building and testing

You need [Rust](https://rustup.rs). The repository pins the stable channel in `rust-toolchain.toml`, so `rustup` picks the right one. If CI complains about something your machine accepts, your stable is probably older than GitHub's:

```sh
rustup update stable
```

The four commands that must pass before a pull request:

```sh
cargo fmt --all
```

```sh
cargo clippy --workspace --all-targets -- -D warnings
```

```sh
cargo test --workspace
```

```sh
cargo run -p keevoro-cli -- scan <a folder of photos>
```

CI runs the same on Windows, macOS and Linux, and fails if fewer than 90% of lines are covered by tests.

### The desktop app

The app is four layers, and almost all the thinking is in the bottom two:

| Layer | Where | What it is |
| --- | --- | --- |
| Screens | `ui/src` | React and TypeScript. They show what they are given and send one request per button. |
| Shell | `src-tauri/src` | A Tauri window and a handful of commands. Each command finds the open backup, calls the API, and turns an error into a sentence. The jobs (looking through a source, backing up) still run here until the API has jobs of its own. |
| API | `crates/keevoro-api` | Every operation Keevoro offers, listed once in a register. What the app, the command line and an AI agent all call. See [The API](#the-api) below. |
| Engine | `crates/keevoro-engine` | Everything Keevoro actually does: the Discover view, choosing, gathering, previews, backing up and connecting storage. Shared with the command line, and tested without a window. |

If you are about to write a rule about photos in the shell or the screens, it belongs in the API or the engine instead.

```sh
npm install
```

```sh
npm run dev
```

That builds the screens, starts the shell and reloads as you edit. On Linux the shell needs a few system libraries first: `libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev libayatana-appindicator3-dev`.

The screens can also run alone in a browser, which is the quickest way to work on how things look:

```sh
npm run ui:dev
```

There they talk to a stand-in for the engine (`ui/src/demo.ts`) with drawn pictures, so no drive or library is needed. Add `?demo=new` to the address to see the first-run screen, or `?demo=empty` for a library with nothing found yet.

The app updates itself through `src-tauri/src/updates.rs`, which leans on `keevoro-update`: the same "find the latest release, download, compare with SHA256SUMS" rules the command line uses, applied to the installer. To see the offer in a browser, add `?update` to the address.

Searching is split in two. The search itself (`keevoro-scan` with `quick: true`) only samples large files and marks them `hashed: false`. `keevoro-engine/src/deep.rs` then reads those files whole, in the background in the app, and always before gathering. The rule that must never be broken: nothing is copied, and nothing is dropped as a duplicate, on the strength of a quick fingerprint. `gather_one` refuses an item that is not `hashed`.

A backup that goes straight to a destination does not read everything first, because that would read each file twice: once to fingerprint it and once to copy it. Before sending, it reads in full only the files whose quick fingerprint is shared with another file, since those have to be told apart (a true copy is left out, a lookalike is not). Every other sampled file is given its place under its quick fingerprint and marked `learn_fingerprint` on its `Upload`. The store or the uploader then works out the whole fingerprint in the same pass that sends it, the answer comes back in `PublishReport::learned`, and `Catalog::set_whole_hash` moves the file, its keep choice and its place over to the real fingerprint. The rule still holds: nothing is dropped as a duplicate on a sample, and a folder copy is still read back and compared before it counts.

Video stills come from `keevoro-frame`, the one crate allowed `unsafe`, because it calls the operating system's decoder directly. Its Windows and macOS halves cannot be built on Linux, but they can be type-checked from anywhere:

```sh
rustup target add x86_64-pc-windows-msvc aarch64-apple-darwin
cargo check -p keevoro-frame --target x86_64-pc-windows-msvc --tests
cargo check -p keevoro-frame --target aarch64-apple-darwin --tests
```

Its real test (`tests/native.rs`) decodes a tiny sample video and only runs on Windows and macOS, so CI is where it proves itself.

### Finding where the photos are

Knowing which folder to point Keevoro at is the hard part on a big disk, so there is a survey that answers it first: `crates/keevoro-scan/src/survey.rs`. It reads folder listings only, the names and sizes a file manager shows, and never opens a file. That is why it gets through hundreds of thousands of files in seconds where a search takes much longer.

It is in two halves, kept apart on purpose:

- `survey` walks the disk on several threads and counts what each folder holds directly.
- `suggest` turns those counts into folders to offer. It is pure arithmetic and never touches the disk, so its rules are tested with made-up counts.

The rules in `suggest` aim for the folder a person would have picked themselves. A folder that is mostly photos and videos, everything beneath it counted, is offered whole (`Pictures`, not each of its two hundred subfolders). A folder with photos loose in it is offered too, so they are not lost. Anything else is only a container and its subfolders are considered one by one. A whole drive is offered only when nine files in ten on it are photos, which is a memory card and not a system disk. No suggestion is ever inside another.

A survey guesses from file endings (`keevoro_core::kind_by_ending`), so it is a hint and nothing more. Nothing it finds is remembered. The person ticks folders and each becomes an ordinary source, searched properly. Try it from the command line with `keevoro survey D:\`.

### What a backup looks like on disk

A backup has three folders: `originals` (the files themselves, never altered), `renditions` (thumb, display, poster and video versions made for looking at, each named after a fingerprint of its original) and `index` (the list a gallery reads). `crates/keevoro-engine/src/layout.rs` holds the explanation of these in plain words and writes it into every backup as "What is in this backup.txt". The app shows the same on the backup screen.

If you add a folder to what a backup holds, add it to `PARTS` in `layout.rs` and to the list in `ui/src/App.tsx`. A test in `send.rs` fails if a backup contains a folder the note does not explain, so this cannot be forgotten quietly.

### How a photo gets opened

Opening a photo is the slow part of making previews, and there is more than one way to do it, so the ways sit behind one small trait in `crates/keevoro-media/src/decode.rs`:

```rust
pub trait Decoder: Send + Sync {
    fn name(&self) -> &'static str;
    fn decode(&self, file: &Path, max_edge: u32) -> Result<Decoded, MediaError>;
}
```

`Decoders` holds a list of them and asks each in turn. The first one that manages it wins, so a photo the computer's codecs cannot open still gets opened by the next way down. Today the list is:

- `SystemCodecs`: the computer's own picture codecs through `keevoro_frame::photo` (WIC on Windows). Usually the fastest, hardware-assisted where the codec is, and the only way HEIC and some raw files open at all.
- `BuiltIn`: the `image` crate. Works everywhere, which is why Linux and the tests use it.
- `EmbeddedPreview`: the JPEG a camera tucks inside its raw file.

Nothing else in the workspace knows which one did the work. To add a way of decoding (ImageIO on macOS is the obvious next one), write a type that implements `Decoder` and add it to `Decoders::for_this_computer`. Nothing that calls `render_photo` has to change.

Two rules worth knowing before touching it:

- A file that cannot be read at all stops the search straight away. Trying three more decoders on a drive that has been unplugged only wastes time.
- When every way fails, the reason given is the most useful one, not the last one: "this JPEG is damaged" beats "not a kind of file I know".

The switch in Settings ("Use this computer's own picture codecs") only decides whether `SystemCodecs` is in the list. It is on unless someone turns it off, and is kept as `plain_decoding` in the settings file so that an old settings file means "on".

Previews are made several at a time (`keevoro-collect/src/previews.rs`), on half the processor cores and never more than four, so the computer stays usable. The worker threads only decode and shrink; one thread writes to the catalog, so there is nothing to lock.

We looked at CUDA and chose not to use it. It only works on NVIDIA cards, needs a large runtime installed, and shrinking a still photo is not where a graphics card earns its keep: reading the file and unpacking the JPEG dominate. The system codecs give most of the gain on every computer with nothing to install.

Keevoro should be a good guest on someone's computer. Any loop that may run for more than a moment calls `keevoro_core::pause::wait_while_paused()` once per file, so the Pause button on the Activity page reaches it, and any new kind of long job gets an entry in `src-tauri/src/activity.rs` saying what it is and why, in words a person would accept as a reason.

The app keeps a log (`src-tauri/src/log.rs`): one line per event with the time in front, in the system's usual log folder for the app, shown in Settings. Log anything a person might later ask "what was it doing?" about, in the same plain words as the screens. Never log keys.

The app works in backups (see `src-tauri/src/settings.rs`): each is a named job, such as "Family photos" or "Drone footage", with its own home folder for its catalog. In the code a backup is the job (`Backup`, `start_backup`), and sending its files to their destination is "send" (`send`, `SendPanel`, `destination`). The engine module that does the sending is `keevoro_engine::send`, shared with the command line. Within a backup there are three steps (`find`, `choose`, `backup` in `ui/src/App.tsx`; the last is shown as "Back them up somewhere safe") and the steps in the sidebar are the only navigation. The rule for any new screen: one question as the heading, one main button in the bar at the bottom, and words a person who has never heard of Keevoro would understand. The drives offered on the first step come from `src-tauri/src/places.rs`, which decides what a drive is called and which system volumes to hide.

`ui/src/backend.ts` is the only place the screens touch the outside world. Its types mirror the Rust structs in `crates/keevoro-engine/src/discover.rs` and `src-tauri/src/events.rs`; change them together.

The browser test uses the screens the way a person would and saves a screenshot of each to `ui/tests/screens/`:

```sh
npm run ui:build
```

```sh
npm run ui:test
```

Every push to main builds real installers (the **installer** jobs in CI); download them from the run's page under Artifacts to try a change before it is released. To build one yourself on Windows or a Mac, run `scripts/build-installer.sh` (see the comment at its top).

The look comes from the Keevoro design system. Its tokens are kept in `design/tokens.json` and turned into `ui/src/tokens.css` by `npm run tokens`; `ui/src/app.css` uses only those names, so never write a colour, radius or shadow by hand. Light, dark and "match my computer" are the same tokens with different values (`ui/src/theme.ts` sets which). The logo files in `design/` are used as supplied (`keevoro-lockup-reversed.png` is the one for dark grounds). The typeface, Keevoro Sans, is three files in `ui/src/assets/fonts/`, declared by the generated `tokens.css`: Bold for names and headings, Medium for controls, Regular for sentences, and no other weights; the app icon set is made from one with `npx tauri icon design/keevoro-icon.png`.

### The API

Over time a good deal of what the app does was written in its shell, where the command line cannot reach it. `crates/keevoro-api` is where that logic is moving, so that the app, the `keevoro` command and, later, an AI agent's tools can all do the same things with the same words. The work is planned in steps. The first put the crate and its register in place; the second moved everything that is not a long-running job: named backups, destinations, room, the rehearsal, pictures, settings, the log, activity, reset, the update check and what agents may do. Nothing a person would notice changed. The jobs (looking through a source, surveying, backing up, checking, and stopping and pausing them) are the next step.

It has five parts:

- **The register** (`src/register.rs`). One line for every operation, such as `sources.list` or `backup.run`: what it does in a plain sentence, whether it changes anything, whether it is a long-running job, whether the command line and agents get it, and whether the API does it yet. The command line's help and an agent's list of tools will be made from it, so write each description as carefully as a button.
- **Keevoro as a whole** (`Keevoro` in `src/keevoro.rs`). Made once from its `Folders` (config, data, cache, logs), it holds the named backups, the log and the keychain, with a method for each operation that is not about one backup. Everything is reached through `&self`, and nothing slow happens while the settings are held, so the app can keep one and ask it things from any thread.
- **Named backups** (`Backups` in `src/backups.rs`). It owns the settings file in the config folder, in exactly the format earlier versions wrote, and hands out a `Backup` by name or id. Which backup is "open" is the app's idea; every operation also takes a backup's name, because the command line has no open backup.
- **A backup** (`Backup` in `src/backup.rs`, with its destinations in `src/destinations.rs`). One backup, opened by its home folder, with a method for each operation on it. Each takes and gives plain types that can be written as JSON, reusing the engine's own views where they are right.
- **`call`** (`src/lib.rs`). The same operations reached by name, with JSON in and JSON out, which is how the command line and an agent will use them: `call(&keevoro, op, Some("Drone footage"), request)`.

Errors are one type, `keevoro_api::Error`: the sentence the person sees, exactly as the engine wrote it, and a kind a program can act on.

Two parts need care whenever they are touched:

- **Storage keys.** They may be given to `destinations.add` and go to the keychain and nowhere else. They are held in a `Key`, whose `Debug` form is `[hidden]` and which cannot be printed or serialised; no answer carries one; an error raised while one is in hand has it taken out of its words. `a_key_given_is_never_given_back_shown_logged_or_written_down` in `src/lib.rs` gives a marker key and searches the answer, every form of the error, other operations' answers and every file Keevoro wrote for it.
- **What agents may do** (`src/agents.rs`). A person names the folders an agent may look through and back up to, and whether it may make changes; until they do, it may use none and only look. `AgentAccess::may_look_in` and `may_back_up_to` answer "is this path inside one of those folders?", judging a path by where it really leads and refusing on any doubt (`..`, links, the many spellings of one Windows path). Its tests attack it; add to them before changing it. Nothing enforces it yet, because there is no agent yet.

To add an operation:

1. Write its tests first, against real state in a temporary folder (`keevoro::tests::keevoro` makes a `Keevoro` with a keychain in memory and no network, and `backup::tests::one_backup` a backup with photos in it).
2. Add its line to the `register!` table. The `Op` enum is made from the same table, so there is no other list to remember.
3. Write the method on `Keevoro` or `Backup` and its arm in `call`. The compiler insists on the arm, and a test calls every operation to check that `implemented` tells the truth.
4. Make the app's command call it, keeping the command's name, arguments and answer exactly as `ui/src/backend.ts` expects them.

The parity test (`src-tauri/src/parity.rs`) is what stops the app drifting ahead again. It reads the app's list of commands from `generate_handler!` in `src-tauri/src/lib.rs` as it runs, and fails if any command is not on exactly one of three lists:

- `BACKED`: it only translates between the screens and register operations the API does.
- `WINDOW_ONLY`: it is about being a window, such as showing a folder or opening a web page.
- `NOT_YET`: its logic still lives in the shell. This is the honest to-do list. It must only get shorter, and it is empty when the work is done.

A new button with no API operation behind it turns the build red until it is given one, or deliberately listed.

### Tests that need a server

The S3 adapter is tested against a real S3-compatible server. CI starts MinIO for this. Locally those tests skip themselves unless you point them at a server; the variables are listed at the top of `crates/keevoro-storage-s3/src/tests.rs`.

## Cutting a release

The work of a release is done before it, in [CHANGELOG.md](../CHANGELOG.md). Every change a person would notice goes under **Unreleased** as it lands, in one of **Added**, **Changed**, **Fixed** or **Removed**, written for the person using Keevoro: what they will see, not which function moved. Those notes become the GitHub Release, and a release refuses to run while the section is empty.

The headings also decide the version number:

| Unreleased lists | The release is | From 0.2.1 |
| --- | --- | --- |
| Something **Added** or **Removed** | A minor release: something a person would notice as new, or gone | 0.3.0 |
| Only **Changed** and **Fixed** | A patch | 0.2.2 |

That is the whole rule. It lives in `next_version` in `crates/xtask/src/changelog.rs` and is covered by `cargo test`. It follows that the heading a change is filed under matters: a new ability belongs under Added even when it is small, and a rework of something that already exists belongs under Changed. You can always overrule it by giving a number yourself, which is how 1.0.0 will happen. Whatever the number, it must be higher than the last, because installed copies only ever move to a higher version.

### The button

On GitHub, open **Actions**, pick **Release** on the left, press **Run workflow**, and press the green button. Leave the version blank and the changelog decides it; type one to choose yourself. The run's page says which number it took and why. Only people with write access to the repository can do this. The workflow then:

1. Works out the version if none was given, and refuses if it is not higher than the last release, if CI has not passed on the latest commit to main, or if the changelog has nothing under Unreleased.
2. Dates the Unreleased section as the version and opens a fresh one, sets the version in `Cargo.toml`, updates `Cargo.lock`, commits and tags.
3. Creates the GitHub Release as a draft, with the changelog section as the notes.
4. Builds the command-line program for Windows (x64), macOS (one file for Apple silicon and Intel) and Linux (x64), checks each build reports the right version, and uploads them to the draft.
5. Builds the desktop app's installers for Windows and macOS with `scripts/build-installer.sh`, the same script CI uses, and uploads them too.
6. Adds a `SHA256SUMS` file and publishes the release, so "latest" never points at a release with nothing to download.

Day to day, steps 4 and 5 build for **Windows only**. GitHub's macOS machines are often slow to come by, and during fast development they held releases up for hours. For a milestone, tick **all platforms** and the release is built for macOS and Linux as well. The same goes for CI: every change is tested on Windows and on Linux (in the coverage job), and macOS is tested when the CI workflow is run by hand with **all platforms** ticked. Do that before a milestone release, since a Windows-only release says nothing about the Mac.

A Windows-only release has no Mac or Linux files on it. The Mac app only offers an update when the release has a Mac installer, so Mac users are simply not offered it.

Tick **Rehearse** to see steps 1 and 2 in the log without anything being pushed, built or published. It is also the way to ask what number the next release would be. A rehearsal can run from any branch.

The moment the workflow finishes, the one-line installers and `keevoro update` pick up the new version.

### The terminal

```sh
cargo xtask release
```

```sh
git push --follow-tags
```

If the release commit reaches main but the tag does not (some environments may push commits but not tags), finish it from the button: run **Release** and type that version. The workflow sees the version is already cut, tags it and builds.

The first command fetches the tags, works out the number the same way and says which it chose; `cargo xtask release 0.3.0` chooses for it. It is exactly what the button runs, so the two routes cannot drift. The tag push starts the Release workflow at the build step. The terminal route does not check CI for you.

`cargo xtask` is the project's own tooling, in `crates/xtask`. It is written in Rust so it behaves the same on every platform with nothing else to install. `cargo xtask` on its own lists what it can do.

## Rolling back a release

If a release turns out to be broken, open **Actions**, pick **Roll back**, press **Run workflow**, give the last good version and one sentence on why. The workflow then:

1. Marks every release newer than the good one as a pre-release, which withdraws it. GitHub's "latest" points at the good version again at once, so the installers hand out the good version and `keevoro update` no longer offers the bad one.
2. Puts the good version's code back on main as one commit, **Roll back to vX**, with a line in the changelog saying so and why. The release tooling (`.github`, `.cargo`, `crates/xtask`, the install scripts and `CHANGELOG.md`) keeps its current state.
3. Releases that code as the next patch number, higher than the withdrawn release, so copies that already took the bad version move to the good code the next time they update.

Step 3 is why a rollback is a release and not a deletion. The updater compares version numbers and only ever installs a higher one, so the only way to move a copy off 0.3.0 is to give it a 0.3.1. Anyone who wants an older build by hand can still download it from the releases page; the withdrawn ones stay there, marked pre-release.

Work on main that landed after the good version is no longer on main afterwards. It is not lost: bring it back with `git revert` of the Roll back commit, fix what was wrong, and release again.

## How installing and updating work

Each release carries the command-line program for three systems, the desktop app's installers for two, and a checksum file:

| File | For |
| --- | --- |
| `keevoro-windows-x64.exe` | Windows 10 and 11 |
| `keevoro-macos-universal` | macOS, Apple silicon and Intel |
| `keevoro-linux-x64` | Linux on x86_64 |
| `Keevoro-windows-x64-setup.exe` | The desktop app's Windows installer (per-user, no administrator) |
| `Keevoro-macos-universal.dmg` | The desktop app for macOS, Apple silicon and Intel |
| `SHA256SUMS` | Checking any of the above arrived intact |

The names carry no version, so "the latest Windows build" is always the same address.

`install.sh` and `install.ps1` at the repository root ask GitHub for the latest release, download the right file, check it against `SHA256SUMS`, and put it in the user's own programs folder (`~/.local/bin`, or `%LOCALAPPDATA%\Programs\Keevoro` on Windows, which is also added to the user's PATH). Neither needs an administrator. Running an installer again updates.

`keevoro update` does the same from inside the program (`crates/keevoro-update`): it asks for the latest release, and if that is a higher version it downloads the build for this computer, checks it, and swaps itself for it in one step. `keevoro update --check` only reports. This is the one time Keevoro contacts the internet without being given a storage address, and it only happens when asked; there is no background check, in keeping with "nothing leaves the machine without consent".

### If the repository is made private

The repository is public, so none of this needs a login. If it is ever made private again, GitHub stops showing its releases to strangers, and the installers and `keevoro update` need a token: a fine-grained personal access token with read-only **Contents** access to this one repository, in `GITHUB_TOKEN` (or `KEEVORO_GITHUB_TOKEN` if `GITHUB_TOKEN` is taken). The install scripts would then be fetched through `https://api.github.com/repos/markcastle/keevoro/contents/install.sh` with that token and the header `Accept: application/vnd.github.raw`.

Being public also matters for cost: GitHub Actions is free and unlimited for public repositories, but metered for private ones, where a macOS minute counts as ten.

### What is not signed yet

The checksum proves a download arrived intact. It does not prove who made it, because the checksum file comes from the same place as the program. Two things would add that, and both need something only the owner can provide:

| What | Needs | Gives |
| --- | --- | --- |
| Windows code signing | An Authenticode certificate, as repository secrets | No SmartScreen warning on first run |
| Apple Developer ID and notarisation | An Apple Developer account (99 USD a year), as repository secrets | No Gatekeeper warning; today the build has only a free ad-hoc signature |

Until then the install scripts clear the first-run warnings, as they are run by someone who has chosen to trust the repository. The desktop app (phase 3) will use Tauri's updater, which signs every update with a key of our own, the way Task Rail does.
