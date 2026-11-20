# Keevoro for AI agents

An AI app, such as Claude Desktop or Claude Code, can use Keevoro for you. You can ask it "find the photos on my old laptop drive and back them up to the USB drive", and it finds them, tells you what it found, and backs them up, in the same named backups the Keevoro app shows.

It does this through `keevoro mcp`, a small server built into the `keevoro` command. It speaks the Model Context Protocol (MCP), which is how AI apps talk to local tools. The AI app starts it by itself when it needs it. It opens no network port, and talks to nothing but the AI app that started it and the places you back up to.

## What an agent can do, and what it cannot

Keevoro treats every source (a drive, a memory card or a folder it looks through for photos) as the only copy of someone's memories. An agent acts faster than a person and with less doubt, and it can be talked into things, so it gets less than you do.

Out of the box, an agent can only look. It can list your backups, what each one found, its sources, collections, duplicates and destinations, how earlier backups went, and what Keevoro is doing now. It cannot change anything, and it cannot look in any folder, until you say so.

In a folder you have named it can also look inside before anything is added: `folders_inside` lists the folders in a place it surveyed that hold photos or videos, `files_in_folder` lists one folder's photos and videos a page at a time (by name, or newest or oldest by the file's date, all or only photos or videos, with how many seconds each video lasts), and `show_picture` with a `file` gives a picture of any of them. None of that adds anything to a backup or changes anything; the pictures are kept only in Keevoro's own cache, and a file only in online storage (OneDrive, iCloud) is never opened, since that would download it.

When you allow it, it can also:

- look through folders you have named, and add them to a backup as sources;
- start and rename backups, keep or leave out collections and photos, and choose kinds of file;
- add a folder you have named as a destination, or cloud storage with its keys;
- back up, check what arrived, pause, carry on and stop.

Whatever you allow, an agent can never:

- change, move or delete anything on a source. Nothing in Keevoro can;
- delete a photo from a backup. Nothing in Keevoro can do that either;
- look in, or back up to, a folder you have not named. A path written to dodge the rule, with `..`, a shortcut (a symbolic link) or another spelling of the same Windows path, is judged by where it really leads, and refused;
- read back a storage key. An agent can give Keevoro a key when it adds cloud storage, and the key goes straight into your computer's keychain. No answer, error or line of the log ever contains it;
- forget a backup, reset Keevoro, change its settings (the kinds of file Keevoro looks for among them; it can read that list), or install an update. Those are for you;
- change what it is allowed to do. It can ask what it may do, and nothing more.

Everything an agent does is written to Keevoro's log, in plain words: which tool it used, on which backup, and how it went. Settings, "What Keevoro has been doing", shows it.

One thing to know about keys: a key you give an agent has passed through the AI app on its way, so it may be in that app's own history of your conversation. Use a key that can reach one bucket and nothing else, which is good practice anyway, or add cloud storage yourself in the app and let the agent only back up to it.

## Turn it on

### 1. Install the command line

`keevoro mcp` is part of the `keevoro` command. If you have not installed it, the README says how, in one line. Check it works:

```sh
keevoro --version
```

Note where it is, because the AI app needs the whole path. On Windows it is usually `%LOCALAPPDATA%\Programs\Keevoro\keevoro.exe`; on a Mac or Linux, `~/.local/bin/keevoro`. `where keevoro` (Windows) or `which keevoro` (Mac, Linux) says for sure.

### 2. Say what an agent may use

In the Keevoro app, open Settings and find **AI agents**. Add the folders an agent may look through (the drive or folder with your photos) and the folders it may back up to (your USB drive, say). Switch on **Let AI agents make changes** when you want it to do more than look.

Or at the command line:

```sh
keevoro agents allow --look "D:\" --back-up-to "F:\Family Backup"
keevoro agents allow-changes on
```

`keevoro agents` shows what is allowed now. The folders given replace those allowed before; `keevoro agents allow --none` takes them all away, and `keevoro agents allow-changes off` puts the agent back to looking only. A change counts at once, even while an AI app is using Keevoro.

### 3. Tell the AI app about Keevoro

#### Claude Desktop

Open Claude Desktop's Settings, then Developer, then **Edit Config**. That opens `claude_desktop_config.json`, which is in `%APPDATA%\Claude\` on Windows and `~/Library/Application Support/Claude/` on a Mac. Add Keevoro under `mcpServers`, with the whole path to the program:

```json
{
  "mcpServers": {
    "keevoro": {
      "command": "C:\\Users\\you\\AppData\\Local\\Programs\\Keevoro\\keevoro.exe",
      "args": ["mcp"]
    }
  }
}
```

On a Mac:

```json
{
  "mcpServers": {
    "keevoro": {
      "command": "/Users/you/.local/bin/keevoro",
      "args": ["mcp"]
    }
  }
}
```

If the file already has other servers, add the `"keevoro": { ... }` entry beside them, inside the same `mcpServers`. Quit Claude Desktop completely and open it again. Keevoro's tools then appear under the tools button in a new conversation.

This is the format Claude Desktop's documentation gives for a local server at the time of writing. If the app has moved things since, its own help page, "Connect to local MCP servers", is the one to follow; the part that is Keevoro's, a command of `keevoro` with the argument `mcp`, stays the same.

#### Claude Code

In a terminal:

```sh
claude mcp add --transport stdio keevoro -- keevoro mcp
```

Everything after `--` is the command Claude Code runs. Add `--scope user` before `keevoro` to have it in every project, not only this one. To share it with a project instead, put this in the project's `.mcp.json`:

```json
{
  "mcpServers": {
    "keevoro": {
      "command": "keevoro",
      "args": ["mcp"]
    }
  }
}
```

`claude mcp list` shows whether Claude Code can reach it.

#### Other AI apps

Any app that can start a local MCP server over standard input and output can use Keevoro. Give it the command `keevoro` (with its whole path if the app asks) and the one argument `mcp`.

#### Keevoro's folders

The server uses the same backups, settings and log as the Keevoro app on this computer. To try it out without touching them, set the environment variable `KEEVORO_HOME` to an empty folder in the server's settings (an `env` block beside `command` and `args`), and every Keevoro folder is kept under that one instead.

## A first conversation

Here is how a first backup might go, once you have allowed `E:\` to be looked through, `F:\Family Backup` to be backed up to, and changes. What the agent does is in brackets.

> **You:** I've plugged in Mum's old laptop drive, it's E:. Can you find the photos on it and back them up to my USB drive?
>
> **Agent:** (calls `my_access`, then `list_backups`) You don't have a backup yet. I'll start one called "Mum's laptop". (calls `start_backup`, then `survey_place` on `E:\`, then `job_status` until it ends) The photos are mostly in `E:\Users\Margaret\Pictures` (about 4,100 photos) and `E:\DCIM` (380 photos and 12 videos). Shall I look through both?
>
> **You:** Yes please.
>
> **Agent:** (calls `look_through` for each, and follows each job with `job_status`) Done. I found 4,421 photos and 12 videos, 9.8 GB in all. 230 were exact duplicates, so they'll be kept once, which saves 610 MB. 85 pictures were set aside as probably not photos, mostly small icons. (calls `add_destination` with `F:\Family Backup`, then `rehearse_backup`) Backing up now would send 4,203 files, 9.2 GB, to the folder `F:\Family Backup`, which has room. Shall I go ahead?
>
> **You:** Go for it.
>
> **Agent:** (calls `run_backup`, then `job_status` every so often) It's under way: about 20 minutes left. ... (calls `last_summary`) All done: 4,203 files are safely backed up, each one checked. Nothing on Mum's drive was changed.

If you had not allowed changes, the agent would have stopped at the first step with Keevoro's own words: "Keevoro lets AI agents only look for now, not make changes. The person can allow changes in the Keevoro app, under Settings, AI agents, or by typing: keevoro agents allow-changes on". It is written for you, so the agent can pass it on as it is.

## How it behaves

- **Long work answers at once.** Looking through a source, surveying a drive, backing up and checking can take hours. The tool that starts one answers straight away with a job number; the agent asks `job_status`, which can wait up to 30 seconds for the job to end before answering, and says how it ended once it has.
- **The app, the command line and an agent can all be open at once.** Only one of them works on a backup at a time: if the agent is backing up "Family photos", the app and the command line can watch it, pause it and stop it, but not start a second backup of the same thing until it finishes. The same holds the other way round.
- **Closing the AI app stops what the agent started,** once the file in hand is done, as closing the Keevoro app would. Nothing is lost; backing up again carries on from where it was. A backup that has to run for a day is better started from the app or with `keevoro run`.
- **It does nothing while it waits.** Between requests the server uses no processor time.

Every tool an agent sees, with what it does, is the same list as the command line's commands. `keevoro --help` and docs/command-line.md list them; the agent sees them under names such as `list_backups`, `look_through` and `run_backup`.

## Troubleshooting

**The AI app does not show Keevoro's tools.** Check the path in the app's settings is the whole path to `keevoro` (or `keevoro.exe`), and that `args` is `["mcp"]`. Run that same command in a terminal: it should sit waiting, saying nothing. Press Ctrl-C to stop it. Then quit the AI app completely and start it again; Claude Desktop reads its settings only when it starts. Claude Desktop keeps its own record of each server in its logs folder (on a Mac, `~/Library/Logs/Claude/`).

**"Keevoro lets AI agents only look for now".** That is the default. Switch on Let AI agents make changes in Settings, AI agents, or type `keevoro agents allow-changes on`.

**"Keevoro won't let an AI agent look in ...".** That folder is not one you allowed, or it is reached through a shortcut that leads outside one. Add the folder in Settings, AI agents. `keevoro agents` shows the folders allowed now, exactly as Keevoro reads them.

**"Keevoro is already working on your backup ... in the Keevoro app".** Another Keevoro program is backing up or looking through that backup. Let it finish, or stop it from the app or with `keevoro stop`.

**Something went wrong and the agent cannot say what.** Keevoro's log has every step, including each thing the agent did. It is in Settings, What Keevoro has been doing, or `keevoro log`.

**You want to take everything back.** `keevoro agents allow --none` and `keevoro agents allow-changes off`, or the same in Settings. To stop the agent using Keevoro at all, remove the `keevoro` entry from the AI app's settings.
