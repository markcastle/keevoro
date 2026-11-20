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
cargo run -p keevoro-cli -- survey <a folder of photos>
```

CI runs the same on Windows, macOS and Linux, and fails if fewer than 90% of lines are covered by tests.

### The desktop app

The app is four layers, and almost all the thinking is in the bottom two:

| Layer | Where | What it is |
| --- | --- | --- |
| Screens | `ui/src` | React and TypeScript. They show what they are given and send one request per button. |
| Shell | `src-tauri/src` | A Tauri window and a handful of commands. Each command finds the open backup, calls the API, and turns an error into a sentence. Long jobs are the API's; what they say reaches the window through one listener set up in `lib.rs`. |
| API | `crates/keevoro-api` | Every operation Keevoro offers, listed once in a register. What the app, the command line and an AI agent all call. See [The API](#the-api) below. |
| Engine | `crates/keevoro-engine` | Everything Keevoro actually does: the Discover view, choosing, previews, backing up and connecting storage. Tested without a window. |

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

There they talk to a stand-in for the engine (`ui/src/demo.ts`) with drawn pictures, so no drive or backup is needed. Add `?demo=new` to the address to see the first-run screen, or `?demo=empty` for a backup with nothing found yet.

The app updates itself through `src-tauri/src/updates.rs`, which leans on `keevoro-update`: the same "find the latest release, download, compare with SHA256SUMS" rules the command line uses, applied to the installer. To see the offer in a browser, add `?update` to the address.

Searching is split in two. The search itself (`keevoro-scan` with `quick: true`) only samples large files and marks them `hashed: false`. `keevoro-engine/src/deep.rs` then reads those files whole, in the background. The rule that must never be broken: nothing is dropped as a duplicate on the strength of a quick fingerprint, and no copy counts until it is proved exact against the whole file.

A backup that goes straight to a destination does not read everything first, because that would read each file twice: once to fingerprint it and once to copy it. Before sending, it reads in full only the files whose quick fingerprint is shared with another file, since those have to be told apart (a true copy is left out, a lookalike is not). Every other sampled file is given its place under its quick fingerprint and marked `learn_fingerprint` on its `Upload`. The store or the uploader then works out the whole fingerprint in the same pass that sends it, the answer comes back in `PublishReport::learned`, and `Catalog::set_whole_hash` moves the file, its keep choice and its place over to the real fingerprint. A file that looks like another but cannot be read in full is given no place at all (`check_and_place` in `keevoro-engine/src/lib.rs`), so it is neither sent nor left out until it can be told apart.

Every write to a folder goes through Keevoro's working folder there, `.keevoro-uploads`, before it takes its real name. A copy that is stopped, crashes, is killed or loses its drive part-way leaves a `tmp-…` file there, and versions before 0.22 sent large files to folders in parts, leaving a folder of `part-…` files and a `target`. None of it can ever be carried on (a part-copied file is copied afresh, and folders are no longer sent in parts), so `sweep_working_files` in `keevoro-storage-folder` clears whatever has not been written to for an hour (`ABANDONED_AFTER`) and the working folder itself once empty. The hour is what keeps a live copy safe when another program, or another computer, backs up to the same folder at the same moment: a copy in progress writes every few seconds. It only ever removes Keevoro's own `tmp-…` files and hexadecimal upload folders inside that working folder, checks every path by where it really leads, never follows a link, and leaves anything it does not recognise. A backup sweeps each folder destination as its turn starts and ends (`send::tidy`), and says so in its summary and history when it freed 10 MB or more; taking a folder off a backup sweeps it too, and `destinations.tidy` (`keevoro tidy FOLDER`, and in the app the "Clear unfinished copies…" button beside a folder destination and in Settings, Advanced, in `ui/src/tidy.tsx`) does it for any folder. With `dryRun` it only says what it would clear (`working_files_to_clear`, the same rules without removing anything), which is what the app asks first; what is left is counted by why (`in_use`, `not_ours`), and the sentences come from `keevoro_api::words::tidied`, so the app and the command line say the same. For cloud storage, an upload in parts this backup started for a file it no longer sends is aborted at the end of the pass (`abandon_unfinished` in `send.rs`), from the uploads the catalog remembers; one still being sent is kept and carried on next time.

A copy to a folder is proved in `copy_file` in `keevoro-storage-folder`: the source is fingerprinted as it is read, the copy goes to a hidden file, is flushed to the disk, read back and fingerprinted again, and only takes its real name, without ever replacing a file, if both match. It keeps the original's modified time. A file already there under the same name counts only if it is the same size and, for a store that can read its own files, the same fingerprint (`same_or_refuse` in `keevoro-publish`).

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

A survey guesses from file endings (the list of kinds below), so it is a hint and nothing more. The person ticks folders and each becomes an ordinary source, searched properly. Try it from the command line with `keevoro survey D:\`.

### Looking inside a folder before choosing it

Each suggestion has **Look inside**, which opens a grid of its photos and a still of each video, folder by folder, before anything is added. It is read-only from end to end:

- **The outline is remembered, not walked again.** When a survey ends, every folder that holds photos or videos itself is written, with its counts, to `surveys/` in Keevoro's cache folder (`keevoro_engine::surveys`, lines of JSON named by a fingerprint of the place, at most eight places). `places.folders` answers from the newest one covering the place, and never says anything outside the place asked about, so an agent allowed one folder learns nothing of the rest of a drive. Reset clears the cache, and the surveys with it. This is the first, media-only form of the map the plan describes; the full map comes later.
- **One folder's files come from one listing.** `places.files` reads the folder's listing once (`keevoro_engine::look::list_folder`), keeps the last four in memory so paging is quick, and gives a page at a time by name. It lists only the kinds of file Keevoro looks for, and only photos or only videos when the backup in hand wants one kind. A folder Keevoro made (a backup, an old library) is refused with the same words a search uses, judged by itself alone as a search judges the folder it is pointed at.
- **A picture of any file.** `photos.picture` takes `file` as well as `photo`. `keevoro_engine::look::picture_of_file` never opens a file only in online storage, reads nothing else beyond the file itself, and writes only into the cache: the picture, named by the very fingerprint a search would give the file (`keevoro_scan::fingerprint`, the quick one the app's searches use), and an index, `look.sqlite`, that remembers which picture belongs to which file by its path, size and modified time, so a second look reads the picture back without reading the file, and a file that could not be shown is not tried again for a day. When the folder is later looked through properly, `preview_at` finds those pictures already made. They sit in the same cache as every other preview, under its 1 GB limit (`picture::trim_cache`, now also run after every 500 pictures of files). Each drive has its own turns, taken before the three shared ones: one at a time across a network, two on a disk (`DriveTurns` in `keevoro-api/src/picture.rs`).
- **The screens fetch only what is near.** `ui/src/tiles.tsx` (`VirtualTiles`) places every section and row from the counts alone, so the scroll bar is right at once, and draws only the rows within a screen above and below; each section's files are fetched a page at a time as they come near. Every small picture goes through `ui/src/pictureQueue.ts` (`PictureQueue`): four at a time, a request not started yet dropped when its tile leaves the screen, and the last 600 addresses kept, the oldest revoked (the old preview map never let go of one). The collection screen uses the same grid.

New screens and the pieces they share go in files of their own under `ui/src/` (`inside.tsx`, `tiles.tsx`, `pictures.tsx` for `Picture` and `Viewer`, `pictureQueue.ts`, `tidy.tsx`), imported by `App.tsx`, rather than growing `App.tsx` further. Do the same for the next screen.

### The kinds of file Keevoro looks for

Which endings count as photos and videos is a list the person can change, in Settings, Advanced, or with `keevoro kinds`. It is one list for the whole of Keevoro, not one per backup, and it is separate from leaving out a kind of file on one source (`sources.include_type`), which works on what was found.

How it flows from a click to a search:

1. **Kept as changes.** `keevoro_core::kinds` holds the built-in list (`BUILT_IN`) and the rules. The settings file keeps only what the person changed (`file_kinds` in `settings.json`): endings they added, and built-in ones they switched off or made the other kind. So the list always starts as the built-in one, a built-in kind added in a later version simply appears, and a settings file nobody changed is written exactly as before. It is changed under the settings lock like everything else there (`Backups::change`), so two programs never undo each other.
2. **Read once per job.** When a search (`sources.look`, `sources.look_again`) or a survey starts, `Keevoro::kinds_to_look_for` reads the settings once and makes a `Kinds`, a small lookup by ending. It goes into `ScanOptions::kinds` or `survey`'s `kinds` argument. Nothing consults the disk per file, and the pure functions take the list as an argument, so they are tested with lists made up in the test. A change made while a search runs counts from the next one.
3. **Asked about each file.** The survey counts a file when `Kinds::looked_for` knows its ending. The search passes over, unopened, a file whose ending is switched off, and plainly-not-media endings as before. For a built-in kind Keevoro recognises from inside (a JPEG, an MP4) what is inside still decides: a `.jpg` that is really text is not a photo. For a kind known only by its name (most camera raw and camcorder kinds, and every kind a person adds) `Kinds::by_name` gives the answer when the inside does not say it is a photo or a video, so such a file is found even though nothing can open it. A file whose ending is not on the list at all is still opened and judged by what is inside, so a photo called `IMG_0001.dat` is not missed.
4. **Nothing found is taken away.** `keep_found_before` in `keevoro-engine/src/lib.rs` keeps, when a source is looked through again, any file found there before whose kind is no longer looked for, as long as it is still there at the same size. Switching a kind off changes what is looked for from now on; it never drops a file from the list, from the next backup's index, or from anywhere it was backed up.

A kind Keevoro cannot open goes through everything else like any other file: it is fingerprinted, told apart from its copies, placed in its collection, and copied and proved exact. With no pixel size it is never set aside as tiny. Making its preview fails quietly: the catalog remembers `NoPreview` so it is not tried again, nothing is logged, the index lists it without previews, and the screens show their "No preview" square. `a_kind_added_that_keevoro_cannot_open_is_found_and_backed_up_exactly_with_no_preview_and_no_fuss` in `keevoro-api/src/jobs/tests.rs` walks that whole path.

The guard on what may be added is small: an ending is letters and numbers only (no spaces, dots or path characters, at most ten), compared without case or the leading dot, never twice, and never one of the endings Keevoro passes over as programs, documents, music or archives (`never_media_extension`), which would make no sense to look inside. An AI agent may read the list (`list_file_kinds`) but not change it, like Keevoro's other settings.

### Photos only, or videos only

Which found files are offered is decided at three levels, in one order, by one pure function, `keevoro_core::wants::offered`: the kinds of file Keevoro looks for at all (above; a file of a kind not looked for is never found), then photos or videos (a source's own choice if it has one, otherwise its backup's), then each ending on a source (`sources.include_type`). When a file is not offered it says at which level (`LeftOutBy`), and the "File types" list says it in words ("left out: this backup is for photos only"). The backup's choice is the catalog setting `wants`, a source's is `wants:<its place>`, and both are applied in `Catalog::load_reports`, the one place everything reads what was found, so it counts at once with no new search, and every count, the plan of a backup, the estimate and the room check follow it. Nothing already backed up is touched. A survey for a backup that wants one kind counts only that kind (`Kinds::only`).

### What a search and a survey leave out

`keevoro_core::SkipRules` decides which folders are never looked in: system and app folders, recycle bins, hidden folders and developer folders, by name, and Keevoro's own folders, by what is in them. The search (`keevoro-scan/src/lib.rs`) and the survey (`survey.rs`) both use it, and every folder left out is in the report with its `SkipReason`.

Keevoro's own folders are never a source, because backing a backup up into another is never what a person means:

- **A backup**, wherever it was sent and whatever it is called: a folder holding the note `What is in this backup.txt` beside an `index` folder (`SkipReason::KeevoroBackup`). The whole folder is left out, originals included.
- **A library from before 0.30**, the gathering model that was removed: `.keevoro/catalog.sqlite` beside an `originals` folder (`SkipReason::OldLibrary`). It holds copies of photos, so it is described (how big, how much is copies and how much previews) and never removed; the person decides.
- **A backup's own folder** (`.keevoro/catalog.sqlite` alone), and Keevoro's data, config, cache and log folders (`SkipReason::KeevoroOwn`).
- **Every folder a backup on this computer goes to**, by its full path, even before its first backup has written anything there. `Keevoro::skip_rules` (`keevoro-api/src/jobs/own.rs`) gathers these from the settings as each search or survey starts.

The survey only looks closely at a folder when one of those telltale names is in its listing, so it stays quick. Each folder left out is said, in the log, at the end of a search (`ScanEvent::left_out`) and of a survey (`SurveyView::left_out`), in the app as one quiet line that opens, and at the command line. Files found there by an earlier search drop out of what is offered at the next look, and the line says how many; anything already backed up stays backed up. To back up a backup's photos somewhere else on purpose, a person chooses its `originals` folder itself as the source: only the folder chosen is judged, never its parents, so that works, and an AI agent is still held to the folders it is allowed. Keevoro's previews copied out of a backup by hand (loose `.webp` files) are not recognised; nothing marks them as Keevoro's.

The walk has no depth limit and never follows a link or junction, so a loop cannot happen and nothing is counted twice; a photo reached only through a shortcut or junction is found where it really is, if that place is looked through. A folder that cannot be opened is reported and the walk carries on. Long names, names in any language, and (from other computers) names ending in dots or spaces do not stop it; `how_deep` in `keevoro-scan/src/tests.rs` pins all of this for the search and the survey. On Windows a path longer than 260 characters works throughout, because the source is canonicalised and Rust's file functions use the long form; `a_photo_with_a_path_longer_than_260_characters_is_found_pictured_and_backed_up` proves it on CI. A file only in online storage (OneDrive's "files on demand": recall on open or on data access, or offline; on a Mac, iCloud's "dataless" files) is passed over without being opened, since opening one downloads it; one found before stays found. Only a file that could be a photo or a video is asked about, and only by its attributes as the folder listing gives them, through the small `OnlineCheck` trait in `keevoro-scan/src/online.rs`: `ByAttributes` reads Windows' attributes or the Mac's file flags (cfg-gated), the pure `windows_online_only` and `mac_dataless` decide and are tested everywhere, and tests hand the search and the survey a check of their own (`ScanOptions::online`, `survey`'s `online`), so Linux CI covers the rest. How many is never only in the log: the search's last word (`ScanEvent::online_only` and `online_only_said`), each survey suggestion (`HintView`) and the survey as a whole, and each source (kept in the catalog as `online_only:<root>` until it is looked through again or forgotten, so it is still said after the person goes elsewhere) all carry the count and one sentence from `keevoro_engine::summary::online_only`, which the app, the command line and an agent's summaries show as it is.

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

### Making previews while the originals are sent

A backup used to send every original, then make every preview, then send those. On a day-long backup that made the previews a long second wait that nobody remembered was coming. Now `keevoro-engine/src/background.rs` makes them on a thread of its own from the moment every file has its place, so they are ready, or nearly, when the originals have all arrived. It is work inside the one backup job, not a second job: the lock, the stop switch and Pause are the backup's own.

The rules, and why:

- **Originals stay first.** Previews go into the backup's own folder, as always, and are sent in a pass of their own after the originals pass (`Pass` and `uploads_for` in `send.rs`), so no preview can hold up an original. The index written after the originals lists only previews that have actually arrived. The test `the_files_themselves_are_sent_before_any_preview_is_sent` holds this.
- **A good guest.** The threads ask the operating system for low priority through `keevoro_frame::quiet` (background mode on Windows and background quality of service on macOS, both of which lower disk reads too; nice 10 on Linux). How many previews are made at once is decided every two seconds by `keevoro_core::background::preview_workers`, which is pure and tested on every platform: none on battery or while other programs use 60% or more of the processor, one while the person has touched the keyboard or mouse in the last two minutes or other programs use 25% or more, and otherwise one fewer than the foreground would use. Processor use is averaged over about ten seconds, so a spike changes nothing.
- **Kind to slow sources.** While the originals are being sent, there is never more than one preview maker per source, and the files are taken one source after another, so a USB stick or a network share is read by the sender and at most one other reader.
- **The signals that are real.** Battery and idle time on Windows (`GetSystemPowerStatus`, `GetLastInputInfo`); idle time on macOS (`CGEventSourceSecondsSinceLastEventType`); battery on Linux (`/sys/class/power_supply`); processor load everywhere, from `sysinfo`. macOS battery and Linux idle time are not known, which the rules take the careful way: unknown idleness counts as "in use", and an unknown battery is not a reason to wait.
- **Safe.** Sources are only read. A stop or a crash keeps every preview made, since each is written whole and then recorded, and the rest are made next time. A file a quick search only sampled is left until its original has been sent, because that is when its real fingerprint is learnt, and its previews are named by it.
- **After the originals.** Once they have all arrived, previews are the backup's own work again and are finished at the foreground's pace, even on battery, as they always were. If previews are switched off for a backup, none of this runs.

To test it with made-up conditions, `Previews::start_looking` takes the conditions from a closure instead of the computer.

### How long a backup has left

One estimator answers "how long until this backup is completely finished": `keevoro_core::estimate`. It is pure and is told the time, so every rule is tested with made-up clocks and rates. A running backup holds a `Tracker` (the estimator with the real clock and the count of bytes sent); the engine tells it what is happening, and `jobs.status` carries what it says as `estimate`, with how long the job has run as `runningSeconds`. The app, `keevoro status`, `keevoro follow` and `keevoro run`, and an agent's summaries all show its words, so they cannot disagree.

How it thinks:

- **A plan of stages.** `send::everywhere` gives it every stage the backup will go through before it starts (`plan` in `send.rs`): telling apart files that look alike, giving each file its place, sending the originals to each destination, making previews and video previews, sending the previews to each destination, and writing the index there. Each has an amount of work in bytes or files. What is not known yet, such as the size of previews not yet made, is estimated and then corrected as it becomes known.
- **Rates that suit the job.** A running stage's rate is measured over a window that grows with how long it has run: a third of it, between one minute and one hour. Beside it a shorter window (a sixth of the long one, between 15 seconds and 10 minutes) watches for the speed really changing. When the two differ by half again or more for as long as the short window, the long window starts again from where the change began. A stall is not adopted like that; it slides out of the long window gradually, and if nothing moves for the whole of it, the estimate says so plainly.
- **Borrowed rates.** A stage not started yet uses, in order: a rate measured for the same kind of work earlier in this backup (for sending previews, half the originals' speed to the same place), a rate remembered from earlier backups on this computer, and a cautious built-in guess (`Kind::guess`). Remembered rates are in `measured rates.json` in Keevoro's own data folder (`keevoro-api/src/rates.rs`), each moving half way towards what each backup measured. Reset forgets them.
- **Side by side.** The rest of the backup is played forward on two lanes, the sending one and the background one, and the sending of previews waits for both, so the overlap is counted rather than added up.
- **When it says something new.** The published answer is worked out again every tenth of the time left, at least every ten seconds and at most once an hour (`republish_every`): every twelve minutes on a two-hour backup, once an hour on a day-long one. It is worked out at once when a stage begins or ends, the work changes by a twentieth or more, the speed changes as above, Keevoro is let go on after a pause, or the countdown runs out before the work does. In between it counts down smoothly. Paused time does not count, for the rates or the countdown.
- **Honest wording.** Under twenty minutes it is said to the minute ("About 12 minutes left"), then to five minutes, a quarter of an hour, an hour, and "about a day". When a tenth or more of what is left rests on guesses it is said a step more roundly ("Roughly 6 hours left"); when half or more does (as at the very start of a first backup, before anything has been measured), it says "Working out how long this will take". How long it has run is measured, so it is said to the minute with no "about", by the clock since it started, saying how much was paused.

When a backup ends, everything measured is written into its history (`keevoro-engine/src/history.rs`), one entry per backup, kept in the backup's own record (the catalog's settings, as the history always was, so no change to its tables was needed). Beside the line the list shows, an entry has a `RunDetail`: when it started, how long it was paused, how it ended and who started it, Keevoro's version, each stage as the estimator recorded it (`StageRecord`: its time with pauses left out, the files or bytes it did, its average speed, and the slowest and fastest it held for a whole long window of a minute or more), what each destination's passes sent, found already there or could not send (`DestinationRecord`, filled in by `run_until` in `send.rs`), the previews made and those Keevoro could not make, the duplicates left out, and the summary exactly as it was said. The last twenty are kept in full; a destination's files that could not be sent are named up to fifty and counted beyond. Entries from before this was kept have no detail and say only what they have. `keevoro_api::words::run_in_full` says one entry in plain lines for the command line and an agent; the app draws the same from the JSON. What is not recorded, because it is not measured honestly: speeds over less than a minute, and how long each single file took.

To add a stage: add a `Kind` with its unit, its cautious guess and its label; put it in the plan in `send.rs` in the order it happens; and have the engine call `Tracker::progress` as it goes and `Tracker::finish` when it ends (or `Tracker::metered` for work counted in bytes sent). A stage done in the background beside the sending is marked by `Kind::alongside`. The screen, the command line and agents then show it with no further change.

We looked at CUDA and chose not to use it. It only works on NVIDIA cards, needs a large runtime installed, and shrinking a still photo is not where a graphics card earns its keep: reading the file and unpacking the JPEG dominate. The system codecs give most of the gain on every computer with nothing to install.

Keevoro should be a good guest on someone's computer. Any loop that may run for more than a moment calls `keevoro_core::pause::wait_while_paused()` once per file, so the Pause button on the Activity page reaches it, and any new kind of long job gets an entry in `src-tauri/src/activity.rs` saying what it is and why, in words a person would accept as a reason.

The app keeps a log (`src-tauri/src/log.rs`): one line per event with the time in front, in the system's usual log folder for the app, shown in Settings. Log anything a person might later ask "what was it doing?" about, in the same plain words as the screens. Never log keys.

The app works in backups (see `src-tauri/src/settings.rs`): each is a named job, such as "Family photos" or "Drone footage", with its own home folder for its catalog. In the code a backup is the job (`Backup`, `start_backup`), and sending its files to their destination is "send" (`send`, `SendPanel`, `destination`). The engine module that does the sending is `keevoro_engine::send`, shared with the command line. Within a backup there are three steps (`find`, `choose`, `backup` in `ui/src/App.tsx`; the last is shown as "Back them up somewhere safe") and the steps in the sidebar are the only navigation. The rule for any new screen: one question as the heading, one main button in the bar at the bottom, and words a person who has never heard of Keevoro would understand. The drives offered on the first step come from `src-tauri/src/places.rs`, which decides what a drive is called and which system volumes to hide.

`ui/src/backend.ts` is the only place the screens touch the outside world. Its types mirror the Rust structs in `crates/keevoro-engine/src/discover.rs` and `src-tauri/src/events.rs`; change them together.

The shell's wiring has a test of its own, `src-tauri/src/wiring.rs`, because nobody can click through a real window in CI. It builds the app exactly as `run` does (the same plugins, commands, listener, `tauri.conf.json` and capabilities) on Tauri's stand-in for a window, `tauri::test`, with Keevoro's folders in a temporary folder. It then does what the screens do: invokes commands by the names in `backend.ts`, listens for the events `backend.ts` subscribes to, and checks every answer and every event against the types there. Along the way it surveys a made-up card, looks through it, backs it up to a folder, stops a search and a backup, and finally checks the photo arrived and the card was not touched. To make that possible, `app` in `lib.rs` takes a `tauri::Builder` for any runtime, and every command that needs the app's handle takes an `AppHandle<R>`, so a new command must too. The one step the stand-in cannot show is the page's own JavaScript receiving an event once Tauri has sent it; that is Tauri's work, and the browser test below covers the screens.

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

Over time a good deal of what the app does was written in its shell, where the command line cannot reach it. `crates/keevoro-api` is where that logic is moving, so that the app, the `keevoro` command and, later, an AI agent's tools can all do the same things with the same words. The work is planned in steps. The first put the crate and its register in place; the second moved everything else the app's shell did, first what is quick (named backups, destinations, room, the rehearsal, pictures, settings, the log, activity, reset, the update check and what agents may do) and then the jobs. Nothing a person would notice changed. Every operation in the register is now done by the API. The third step shared jobs between programs, so a backup the app started can be watched or stopped from the command line (see [Jobs across programs](#jobs-across-programs)). The fourth rebuilt the command line on the API (see [The command line](#the-command-line)). The fifth added `keevoro mcp`, a server for an AI agent built on it (see [The server for AI agents](#the-server-for-ai-agents)).

It has six parts:

- **The register** (`src/register.rs`). One line for every operation, such as `sources.list` or `backup.run`: what it does in a plain sentence, whether it changes anything, whether it is a long-running job, whether the command line and agents get it, and whether the API does it yet. Beside the table, for each operation: whether it is about one backup (`on_a_backup`), whether a given request changes anything (`changes_with`), whether it loses something a person cannot simply put back (`loses_something`), its request as a JSON Schema made from the very type `call` reads it into (`request_schema`, with `schemars`), and the tool an agent is given for it, with its name and what it tells the agent (`tool`). Each is an exhaustive `match`, so a new operation cannot be added without saying. The command line's help and an agent's list of tools are made from all this, so write each description as carefully as a button.
- **Keevoro as a whole** (`Keevoro` in `src/keevoro.rs`). Made once from its `Folders` (config, data, cache, logs), it holds the named backups, the log and the keychain, with a method for each operation that is not about one backup. Everything is reached through `&self`, and nothing slow happens while the settings are held, so the app can keep one and ask it things from any thread.
- **Named backups** (`Backups` in `src/backups.rs`). It owns the settings file in the config folder and hands out a `Backup` by name or id. The app and the command line may both have it open, so every question is answered from the file as it is now, and every change is made under a lock (`settings.lock`, held with the operating system's file locking, so a program that stops lets go) to the file as it is at that moment, then written beside it and moved into place. One program's change never undoes another's. Which backup is "open" is the app's idea; every operation also takes a backup's name, because the command line has no open backup. `Folders::for_this_computer` says where all of this lives, the same place for every Keevoro program; a test in the app checks it is where Tauri would put an app's things.
- **A backup** (`Backup` in `src/backup.rs`, with its destinations in `src/destinations.rs`). One backup, opened by its home folder, with a method for each operation on it. Each takes and gives plain types that can be written as JSON, reusing the engine's own views where they are right.
- **Jobs** (`src/jobs/`). The long operations: looking through a source, looking again, surveying, backing up and checking, and the background check for duplicates that Keevoro starts itself after a search or a backup. Starting one returns at once with a `JobHandle` (its id, what it is, which backup, when and by whom it was started), and the job runs on its own thread, with no Tauri or Tokio needed by the caller. What it says as it goes is a `Said`, in exactly the shapes the app's screens have always received (`src/jobs/said.rs`, mirrored in `ui/src/backend.ts`). A listener can be given when a job starts, `Keevoro::listen` hears every job, and `Keevoro::follow` joins one already running; `Keevoro::jobs` (`jobs.status`) says what each running job is doing for anyone who was not listening. `stop`, `pause` and `carry_on` act between one file and the next. The rules about what may run together are here, so every front end is held to them: one search and one backup or check at a time on a backup, refused in a plain sentence, and the check for duplicates giving way to both. All of this works the same whichever program runs the job, as the next section explains.
- **`call`** (`src/lib.rs`). The same operations reached by name, with JSON in and JSON out, which is how the command line uses them: `call(&keevoro, op, Some("Drone footage"), request)`. An agent's tools use `call_for_agent` (`src/for_agents.rs`) instead, which holds each call to the person's rules first and writes it to the log.

Errors are one type, `keevoro_api::Error`: the sentence the person sees, exactly as the engine wrote it, and a kind a program can act on.

Two parts need care whenever they are touched:

- **Storage keys.** They may be given to `destinations.add` and go to the keychain and nowhere else. They are held in a `Key`, whose `Debug` form is `[hidden]` and which cannot be printed or serialised; no answer carries one; an error raised while one is in hand has it taken out of its words. `a_key_given_is_never_given_back_shown_logged_or_written_down` in `src/lib.rs` gives a marker key and searches the answer, every form of the error, other operations' answers and every file Keevoro wrote for it.
- **What agents may do** (`src/agents.rs`). A person names the folders an agent may look through and back up to, and whether it may make changes; until they do, it may use none and only look. `AgentAccess::may_look_in` and `may_back_up_to` answer "is this path inside one of those folders?", judging a path by where it really leads and refusing on any doubt (`..`, links, the many spellings of one Windows path). Its tests attack it; add to them before changing it. `for_agents.rs` enforces it at every call an agent makes: what the person allowed is read afresh each time, operations kept for a person cannot be called, look-only operations take no request at all (so an agent can never change its own permissions), anything that changes something is refused until changes are allowed, and every folder an operation would look in or back up to (a survey's or a search's place, every source looked through again, the file behind a picture, a folder destination, each folder a backup or a check goes to) must be inside the allowed folders at that moment. Cloud storage is not a folder; adding it is a change, so it needs changes allowed and nothing more.

To add an operation:

1. Write its tests first, against real state in a temporary folder (`keevoro::tests::keevoro` makes a `Keevoro` with a keychain in memory and no network, and `backup::tests::one_backup` a backup with photos in it). A test that runs jobs takes its turn with `jobs::tests::one_at_a_time()`, because pausing is one switch for the whole program, and waits for every job with `wait_for_jobs` before it returns. To hold a job where the test wants it, pause before starting it, or stop it from its own listener when it says the thing the test is waiting for; do not sleep and hope.
2. Add its line to the `register!` table. The `Op` enum is made from the same table, so there is no other list to remember.
3. Write the method on `Keevoro` or `Backup` and its arm in `call`. The compiler insists on the arm, and a test calls every operation to check that `implemented` tells the truth.
4. Make the app's command call it, keeping the command's name, arguments and answer exactly as `ui/src/backend.ts` expects them.

The parity test (`src-tauri/src/parity.rs`) is what stops the app drifting ahead again. It reads the app's list of commands from `generate_handler!` in `src-tauri/src/lib.rs` as it runs, and fails if any command is not on exactly one of three lists:

- `BACKED`: it only translates between the screens and register operations the API does.
- `WINDOW_ONLY`: it is about being a window, such as showing a folder or opening a web page.
- `NOT_YET`: its logic still lives in the shell. The work of moving it all to the API is done, so this is empty; it is for a command that is honestly waiting for the API.

A new button with no API operation behind it turns the build red until it is given one, or deliberately listed.

### Jobs across programs

The app, the command line and an agent's tools are separate programs, and any of them may be running when another starts a job. A person who starts a backup in the app and then types `keevoro status` expects to see it, and one who starts a backup from the command line and then presses Back up in the app expects to be told it is already happening, not to get two backups fighting over one catalog. There is no background service to referee, on purpose: it is a lot to install and keep alive for something files can do. So the programs leave notes for each other, in `crates/keevoro-api/src/jobs/across/`.

Each backup gets a small folder of its own at `jobs/<backup id>/` inside Keevoro's data folder. It is never in a source and never in a destination, and it holds three things:

- **`holder.json`, the lock.** Whichever program is doing long work on the backup (a search, a backup or a check) makes it, and removes it when the work ends. It says which process holds it, when that process started, the computer's name, the front end, and a token unique to that hold. It is made whole beside its place and linked in, which the operating system refuses if the place is taken, so two programs racing for it cannot both win.
- **`status.json`, what is happening now.** The holder writes the same `jobs.status` it gives about its own jobs, a few times a second while anything changes and every two seconds regardless. That regular write is the heartbeat. Anyone may read it without asking the holder anything, which is how `status` and `follow` see another program's job. It also keeps the last words of the last few jobs that ended, so a program waiting for one learns how it ended.
- **`requests/`, what others ask.** Another program asks the holder to stop, pause or carry on by leaving a small file here, one per request so that two askers never overwrite each other. The holder looks four times a second, does what is asked, and removes each file.

Every file is written beside its place and renamed into it in one step, so nobody ever reads half of one. Files whose names begin with a dot are these half-made ones and are never read as the real thing. On Windows something else (a virus checker, the search indexer, or another Keevoro reading it) can have a file open for a moment, and reading, moving or replacing it is then refused. Every step is tried again for up to two seconds (`files::patiently`) rather than given up on, and a request that cannot be read yet is left for the next look rather than removed unread. Giving up at once once lost a request to pause, so the other program never paused.

What a developer needs to know:

- **Only one program at a time does long work on a backup.** Within that program the old rules stand, so a search and a backup still run side by side. They are not split across two programs, because the status would then have two writers and the requests two readers, and telling a person that Keevoro is already backing up their "Family" backup from the command line, and to let that finish or stop it first, is a small price for keeping that simple. The background check for duplicates takes no lock: it gives way, within half a second, when another program starts work on its backup, and that program checks for duplicates itself when it is done.
- **`busy`, `jobs`, `follow`, `wait`, `stop`, `pause` and `carry_on` work the same whichever program runs the job.** A job in another program is marked `elsewhere` in its status. Following one hears the latest thing it said each time its status changes, not every step, and always its last word. Pausing pauses every program working on a backup, as the Pause button always paused everything. The switch (`keevoro_core::pause`) remembers whether the pause was asked for in this program or by another, without a second switch: a pause asked for here lasts until someone carries on, but one another program asked for is lifted, and said in the log, as soon as this program lets go of its last hold (`Work::nothing_held`), because from then on no other program can see its work or ask it to carry on. Otherwise the check for duplicates, which takes no lock, could sit paused where nobody else could reach it. The app asks every two seconds whether it is paused, so the foot of every screen says so, with Carry on, whoever paused it. Job numbers are unique across programs running at the same time, because each program's begin at its process number times a million.
- **The app keeps to its own jobs on screen.** Its screens hear its own jobs as events and have no way yet to follow another program's, so `send_progress` and the stop buttons only look at jobs the app is running on its open backup. Showing other programs' jobs in the app is for later. Reset is refused while any program is working.
- **A program that stops without letting go must not shut anyone out.** A lock is stale, and may be taken, when its process has gone or its number now belongs to another process (process numbers are reused, quickly on Windows; the start time tells them apart), or when its heartbeat is more than a minute old by the clock and does not move while it is watched for five seconds. That last rule catches a stuck program, and a lock written on another computer, where the process cannot be looked up. The watching is there because clocks jump: after a computer wakes from sleep, or sets its clock, a live heartbeat can look an hour old for a moment, and one that moves while watched is alive whatever the clock says. A heartbeat that seems to be in the future counts as fresh. A stale lock is moved aside in one step and checked before it is removed, and a holder checks every two seconds that the lock is still its own; if another program has taken it, it stops its work rather than share. Taking over a stale lock is written in the log.
- **Last words outlive their program for a minute.** The command line closes the moment its job ends, so the status it leaves is kept, unheld, for anyone still waiting on it. Any Keevoro program sweeps away unheld statuses more than a minute old as it lets go of a backup or closes, and removes the backup's folder once it is empty. The lock and requests are always removed when the work ends.

The timing is all in `across/lock.rs` and `across/mod.rs` as named constants: the heartbeat (`BEAT`, two seconds), when a heartbeat is doubted (`STALE_AFTER`, a minute), how long it is watched (`WATCH`, five seconds), how often the holder looks for requests (`TICK`, a quarter of a second) and how long last words are kept (`SWEEP_AFTER`, a minute). The clock and the computer's processes are behind two small traits, `Clock` and `Processes`, so the stale rules are tested with a clock the test moves and processes the test says are alive (`across/tests.rs`). Two Keevoros in one temporary folder behave as two programs, which is how most of the behaviour is tested (`jobs/tests.rs`). What only real separate processes have (their own process number, their own pause switch, and being ended without warning) is tested in `crates/keevoro-api/tests/across_programs.rs`, which runs its own test program a second time as the other program.

### The command line

`crates/keevoro-cli` is the `keevoro` command, and it is translation only: every command turns what was typed into one operation in the register, calls it through `keevoro_api::call`, and says the answer in plain English, or prints it exactly as the API gave it with `--json`. It works on the same named backups as the app because it opens Keevoro in the same folders (`Folders::for_this_computer`), or under `KEEVORO_HOME` when that is set.

- `src/commands.rs` has one row for each command as it is typed (`keevoro backups start`): its words, the operations it can call, its arguments, how it makes the request and how it shows the answer. Its help is the register's own sentence, and a command for a job gets `--detach` because the register says it is one. `every_operation_the_register_offers_the_command_line_has_a_command` in `src/tests.rs` fails if an operation marked for the command line has no row, the command line's own parity test.
- `src/show.rs` says each answer, from the API's JSON.
- `src/jobs.rs` runs a job and follows it to its end, with progress on one rewritten line when a person is watching and a plain line for each new step otherwise. `follow` hears a job in any program. `--detach` starts the same command again as a program of its own (`KEEVORO_DETACHED_JOB` tells it so), which says on its first line how the job started and then runs it with nobody listening.
- `src/keys.rs` takes storage keys from standard input, the environment, the command or the keyboard, straight into the request. `a_key_given_is_never_shown_logged_or_written_down` gives a marker key every way and searches everything the program said and wrote for it; `tests/the_program.rs` does the same through the real program.

The exit codes are in `Code` in `src/main.rs`, made from the API's error `Kind` and, for a backup or a check, from the `outcome` of its last word, so a script need never read a sentence. They are listed for people in [docs/command-line.md](command-line.md#for-scripts).

To add an operation for the command line, add its row; the test above tells you if you forget. The tests in `src/tests.rs` run commands in the test program itself, with a keychain in memory and no network (`At::keevoro`); those that run jobs take their turn with `one_at_a_time()`, because pausing is one switch for the whole program. What only a real separate program shows (its exit code, `--detach`) is in `tests/the_program.rs`.

### The server for AI agents

`crates/keevoro-mcp` is `keevoro mcp`, a Model Context Protocol server over standard input and output, in the same program as the command line (`keevoro-cli` hands over to it). It uses `rmcp`, the official Rust SDK, with only its `server` and `transport-io` features: nothing that opens a network connection. Like the command line it is translation only, and it holds no rules of its own:

- `src/tools.rs` makes each tool from the register: name, description, input schema (the request's schema, with `backup` added for an operation about one backup, and nothing at all for a look-only one), and the protocol's hints: `readOnlyHint` for an operation that changes nothing, and `destructiveHint` for one that `loses_something`. Its tests fail if an operation offered to agents has no tool, or a tool has no operation offered to agents: the agents' parity test.
- `src/lib.rs` is the server. Each `tools/call` runs on a thread of its own, goes through `keevoro_api::call_for_agent`, and answers with a summary in plain English and the API's own JSON (as `structuredContent` too, when it is an object); a picture comes back as an image. A problem is a tool result with `isError`, the API's sentence and its kind; only a tool that does not exist is a protocol error. Keevoro's log is a resource, `keevoro://log`, read through `read_for_agent`.
- `src/said.rs` writes the summaries.

A job answers at once with its handle; `job_status` (`jobs.status`) follows it, and with `waitSeconds` waits up to 30 seconds (`LONGEST_WAIT`) for it to end. Asked about one job by its id once it has ended, `jobs.status` answers how it ended, with its last word (`ended` is true), whichever program ran it. There are no progress notifications: polling is enough, and works the same for a job the app started.

Standard output carries the protocol and nothing else, so nothing in Keevoro may print. The command line no longer locks standard input and output for the whole run, because the server reads and writes them from threads of its own. A panic in the server is written to Keevoro's log. When the AI app closes its side, the server asks its own jobs to stop after the file in hand (`stop_here_and_wait`, which leaves other programs' jobs alone), waits up to a minute, and ends.

`src/tests.rs` speaks to the server over a pipe in memory. `crates/keevoro-cli/tests/mcp.rs` starts the real `keevoro mcp` and speaks to it over its standard input and output, through a whole first backup with a person allowing each step from the command line beside it; it also gives a marker key and searches every answer, standard error, the log and Keevoro's folders for it, checks that every line of standard output is the protocol and that standard error is empty, and checks the agent and the command line take turns on one backup.

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
