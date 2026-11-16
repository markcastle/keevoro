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

The app is three layers, and almost all the thinking is in the bottom one:

| Layer | Where | What it is |
| --- | --- | --- |
| Screens | `ui/src` | React and TypeScript. They show what they are given and send one request per button. |
| Shell | `src-tauri/src` | A Tauri window and a handful of commands. Each command finds the library, calls the engine, and turns an error into a sentence. |
| Engine | `crates/keevoro-engine` | Everything Keevoro actually does: the Discover view, choosing, gathering, previews, backing up and connecting storage. Shared with the command line, and tested without a window. |

If you are about to write a rule about photos in the shell or the screens, it belongs in the engine instead.

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

The app keeps a log (`src-tauri/src/log.rs`): one line per event with the time in front, in the system's usual log folder for the app, shown in Settings. Log anything a person might later ask "what was it doing?" about, in the same plain words as the screens. Never log keys.

The app is four steps (`find`, `choose`, `gather`, `backup` in `ui/src/App.tsx`) and the steps in the sidebar are the only navigation. The rule for any new screen: one question as the heading, one main button in the bar at the bottom, and words a person who has never heard of Keevoro would understand. The drives offered on the first step come from `src-tauri/src/places.rs`, which decides what a drive is called and which system volumes to hide.

`ui/src/backend.ts` is the only place the screens touch the outside world. Its types mirror the Rust structs in `crates/keevoro-engine/src/discover.rs` and `src-tauri/src/events.rs`; change them together.

The browser test uses the screens the way a person would and saves a screenshot of each to `ui/tests/screens/`:

```sh
npm run ui:build
```

```sh
npm run ui:test
```

Every push to main builds real installers (the **installer** jobs in CI); download them from the run's page under Artifacts to try a change before it is released. To build one yourself on Windows or a Mac, run `scripts/build-installer.sh` (see the comment at its top).

The app icon is drawn by `scripts/make-icon.mjs`; the comment at the top of that file says how to regenerate the icon set.

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
