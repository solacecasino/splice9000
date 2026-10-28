# splice9000 adapters

splice9000 ships bare. An **adapter** is an add-on you choose to have, like
foobar2000's components: a folder with an `adapter.toml` and, usually, a
program in any language. splice9000 starts the program when it's needed and
talks to it with JSON messages, one per line, over stdin and stdout. An
adapter can crash without taking splice9000 down.

An adapter can add:

- **commands**, in any menu and bindable to keys;
- **panels** you place in the layout like any other;
- **export formats**, which need no program at all: an adapter can be only
  an `adapter.toml` of presets.

## Finding and installing

Adapters are listed in the
[catalog](https://github.com/solacecasino/splice9000-adapters/blob/main/CATALOG.md).
Download the zip for your computer and **drop it on splice9000's window**
(or **Adapters → Install Adapter…**). splice9000 shows what it adds and what
it may do, and installs it only when you click **Install**: Return does
nothing and Escape declines, so a stray key press can't install anything. splice9000 itself never goes online; the catalog is a
web page.

Installed adapters live in `adapters/` in the config folder. A folder with
an `adapter.toml` can be dropped too, which is how you try your own.

The first time an adapter you copied in by hand runs, splice9000 asks
before starting it. Installing through the prompt counts as allowing it.
Answers are kept in `approvals.json` until the adapter asks for more.

## Built-in adapters

Two adapters come with splice9000:

| Adapter | Adds |
|---|---|
| **Export Presets** | YouTube 1080p, vertical 9:16, square, GIF, ProRes 422 HQ, WebM and sound-only export formats. Presets only: nothing of it runs. |
| **Frame Grab** | **File → Save Frame as PNG…** (⌘⇧G): the frame under the playhead, full size. |

Don't need one? **Remove** it in the Adapters panel. A built-in adapter is
then left out entirely (no menus, no keys, nothing runs), and **Add Back**
returns it, offline. The removal is kept in `adapters.toml` in the config
folder. An installed adapter with the same name as a built-in one replaces
it, which is how a newer version from the catalog takes over.

Removing an installed adapter moves its folder to the Trash.

## Making one

The [template](https://github.com/solacecasino/splice9000-adapters/tree/main/template)
is a working Python adapter, with
[`splice.py`](https://github.com/solacecasino/splice9000-adapters/blob/main/sdk/python/splice.py),
a one-file helper that handles the messages below:

```python
import splice

@splice.command("count")
def count():
    edit = splice.edit()
    splice.notify(f"{len(edit['clips'])} clips")

splice.run()
```

Push a version tag and its workflow publishes correctly named zips (one for
a script adapter; one per kind of computer for a Rust one). Then list it
with the catalog's
[List my adapter](https://github.com/solacecasino/splice9000-adapters/issues/new?template=list-my-adapter.yml)
form.

## `adapter.toml`

```toml
name = "Scene Split"            # shown in menus and prompts; the adapter's identity
version = "0.1.0"
author = "you"
license = "MIT"
description = "One sentence about what it does."
homepage = "https://github.com/you/scene-split"

run = ["python3", "main.py"]    # program + arguments, run from this folder
permissions = ["edit.read", "edit.change"]
start = "command"               # "command" (default) or "launch"
events = ["changed"]            # events to receive while running

[[commands]]
id = "split"                    # sent to you when chosen
label = "Split at Scene Changes"
keys = "cmd+shift+k"            # optional default shortcut; keys.toml can change it
menu = "edit"                   # optional: "file", "edit", "view" or "playback"; else Adapters

[[panels]]
id = "scenes"                   # filled with panel.set
title = "Scenes"

[[exports]]                     # no program needed for these
id = "gif"
label = "GIF (480 px)"
extension = "gif"
width = 480                     # 0 or left out: the edit's own
height = 270
fps = 0
video = ["-loop", "0"]          # ffmpeg encoder arguments, in place of settings.toml's
audio = ["-c:a", "aac"]         # the same for sound
video_filter = "fps=15,split[a][b];[a]palettegen[p];[b][p]paletteuse"
no_audio = true                 # leave the sound out; `no_video` leaves the picture out
```

- `run` is started in the adapter's folder. A program given as a path
  (`./frame-grab`) is found there, and on Windows `.exe` is added if that's
  what's there, so one `adapter.toml` works everywhere.
- A compiled program built for another kind of computer is refused when
  you install it, with which download to get instead.
- splice9000 sets `SPLICE9000_CONFIG_DIR`, `SPLICE9000_FFMPEG` and
  `SPLICE9000_FFPROBE`, so adapters can keep their own files and use the
  same ffmpeg without bringing one.
- `start = "command"` starts the adapter the first time one of its commands
  is chosen or its panel is shown. `launch` starts it with splice9000.
- Whatever the program prints to stderr shows in the Log panel.
- Commands can be bound in keys.toml: `"cmd+k" = 'adapter "Scene Split" split'`.
  Export formats too: `"cmd+shift+e" = 'export "Export Presets" youtube'`.
- Panels go in layout.toml as `panel = "adapter:<name>/<panel id>"`, or
  with the panel buttons in the Adapters panel.
- Unknown keys are ignored, so newer adapter.toml files work in older
  splice9000s as far as they can.

### Permissions

| Permission | Allows |
|---|---|
| `edit.read` | `edit.get`, and events |
| `edit.change` | `edit.set`, `edit.import`, `playhead.set` |
| `network` | Nothing through splice9000; it tells people the adapter goes online |

An adapter is a normal program running as you, so permissions control what
it can do **through splice9000**, and `network` is an honest declaration,
not a sandbox. Only install adapters you trust; being open source is what
lets people check.

## Messages

Every message is one JSON object on one line, in UTF-8. A **notification**
has `method` and `params` and gets no reply. A **request** also has an `id`;
the reply has the same `id` and either `result` or `error` (a string). Only
adapters send requests.

### splice9000 → adapter

| Method | Params | When |
|---|---|---|
| `initialize` | `splice9000` (version), `protocol` (`1`), `permissions`, `ffmpeg`, `ffprobe` (paths, or null) | Right after starting |
| `command` | `command`: the command's `id` | One of its commands was chosen |
| `event` | `kind` plus fields below | Something happened, if listed in `events` (needs `edit.read`) |
| `shutdown` | | splice9000 is quitting or the adapter is being removed; exit soon |

| Event `kind` | Fields |
|---|---|
| `changed` | `duration`, `clips` (how many): at most once a frame while editing |
| `exported` | `path` |
| `saved` | `path`: a project file was saved |

### Adapter → splice9000

| Method | Params | Result | Permission |
|---|---|---|---|
| `edit.get` | | The edit (below) | `edit.read` |
| `edit.set` | `clips`: `[{ "media", "src_in", "src_out" }]` | `null`. Replaces the timeline as one undo step. Each clip is checked against its media file | `edit.change` |
| `edit.import` | `paths`, `append` (bool) | `null`. Adds media; with `append`, also to the end of the timeline | `edit.change` |
| `playhead.set` | `time` (seconds) | The playhead, snapped to a frame | `edit.change` |
| `panel.set` | `id`, `lines` | `null`. Each line is a string, or `{ "text", "command" }` for a button running one of your commands | none |
| `notify` | `text` | `null`. A short message at the bottom of the window | none |
| `log` | `message` | Written to the Log panel | none |
| `ui.confirm` | `title`, `message` | `true` or `false` | none |
| `ui.prompt` | `title`, `message`, `default` | The text typed, or `null` | none |
| `ui.save_dialog` | `title`, `file_name`, `extension` | The chosen path, or `null` | none |
| `ui.open_dialog` | `title`, `extensions` | The chosen path, or `null` | none |

### The edit

```json
{
  "format": { "width": 1920, "height": 1080, "fps": 30.0 },
  "duration": 15.0,
  "playhead": 3.0,
  "selected": 1,
  "file": "/Users/me/trip.splice",
  "media": [
    { "path": "/Users/me/beach.mp4", "duration": 20.0, "width": 1920, "height": 1080, "fps": 30.0, "audio": true }
  ],
  "clips": [
    { "media": 0, "src_in": 2.0, "src_out": 8.0, "start": 0.0 }
  ]
}
```

Times are seconds. `src_in` and `src_out` are times in the media file;
`start` is where the clip begins on the timeline. `selected` is a clip
index, or `null`.

## Example exchange

```
→ {"method":"initialize","params":{"splice9000":"0.0.4","protocol":1,"permissions":["edit.read"],"ffmpeg":"/opt/homebrew/bin/ffmpeg","ffprobe":"/opt/homebrew/bin/ffprobe"}}
→ {"method":"command","params":{"command":"grab"}}
← {"id":1,"method":"edit.get","params":{}}
→ {"id":1,"result":{"playhead":2.5,"clips":[...],"media":[...]}}
← {"id":2,"method":"ui.save_dialog","params":{"title":"Save Frame","file_name":"beach frame 75.png","extension":"png"}}
→ {"id":2,"result":"/Users/me/Desktop/beach frame 75.png"}
← {"method":"notify","params":{"text":"Saved beach frame 75.png"}}
```

(`→` is splice9000 to the adapter, `←` the adapter to splice9000.)

## Versions

`initialize` says which protocol version splice9000 speaks. New versions
only add things, so an adapter written for an older version keeps working.

- **1** (splice9000 0.0.4): commands, panels, export formats, the edit,
  events and prompts.
