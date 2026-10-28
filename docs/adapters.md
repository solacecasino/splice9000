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
  an `adapter.toml` of presets;
- **waveforms and pictures** drawn on the timeline's clips;
- **playback control**, such as J/K/L;
- **snapping** while dragging, also with no program.

splice9000's own editing tools are built this way, so each one can be
taken out.

## Finding and installing

Adapters are listed in the
[catalog](https://github.com/solacecasino/splice9000-adapters/blob/main/CATALOG.md):
**Adapters** at the top right of the window, then **Browse the adapter
catalog**.
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

Seven adapters come with splice9000:

| Adapter | Adds |
|---|---|
| **Waveforms** | Each clip's sound drawn on the timeline. Worked out once per file and kept. |
| **Thumbnails** | Pictures from each clip along it on the timeline. Made once per file and kept. |
| **Shuttle** | J/K/L playback: L plays, again for 2x, 4x, 8x; J the same backward; K stops. |
| **Snapping** | The playhead snaps to cuts, and a clip's end to the playhead, while dragging. Hold Alt (Option) to drag freely. Nothing of it runs. |
| **Crossfades** | **Edit → Crossfade at Nearest Cut** (⌘D), **Remove Crossfade** (⌘⇧D), **Crossfade Every Cut**, **Remove All Crossfades**, **Crossfade Length…** |
| **Export Presets** | YouTube 1080p, vertical 9:16, square, GIF, ProRes 422 HQ, WebM and sound-only export formats. Presets only: nothing of it runs. |
| **Frame Grab** | **File → Save Frame as PNG…** (⌘⇧G): the frame under the playhead, full size. |

The first five are the catalog's, built from
[splice9000-adapters](https://github.com/solacecasino/splice9000-adapters)
at the commit named in `adapters/bundled.txt`; the catalog has each one too.

A crossfade belongs to the edit, not to the Crossfades adapter: removing
the adapter takes away its commands, and the fades already in a project
still play and export.

Don't need one? **Remove** it in the Adapters panel. A built-in adapter is
then left out entirely (no menus, no keys, nothing runs), and **Add Back**
returns it, offline. The removal is kept in `adapters.toml` in the config
folder. An installed adapter with the same name as a built-in one replaces
it, which is how a newer version from the catalog takes over.

Removing an installed adapter moves its folder to the Trash.

## Making one

The [template](https://github.com/solacecasino/splice9000-adapters/tree/main/template)
is a working Python adapter (or use
[`splice.rs`](https://github.com/solacecasino/splice9000-adapters/blob/main/sdk/rust/splice.rs)
for Rust, which needs nothing installed to run), with
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
requires = "0.0.6"              # optional: the oldest splice9000 it works with

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
# audio_filter = "loudnorm=I=-14"  # the same for the sound (protocol 2)
no_audio = true                 # leave the sound out; `no_video` leaves the picture out

[snapping]                      # protocol 3; no program needed
to = ["cuts", "playhead"]       # the playhead to cuts; a clip's end to the playhead
pixels = 8                      # how close, in points
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
- `keys` can be a plain letter (`"l"`), as Shuttle's are: it works
  whenever you aren't typing in a field.
- Whatever the program prints to stderr shows in the Log panel.
- Commands can be bound in keys.toml: `"cmd+k" = 'adapter "Scene Split" split'`.
  Export formats too: `"cmd+shift+e" = 'export "Export Presets" youtube'`.
- Panels go in layout.toml as `panel = "adapter:<name>/<panel id>"`, or
  with the panel buttons in the Adapters panel.
- Unknown keys are ignored, so newer adapter.toml files work in older
  splice9000s as far as they can. When that isn't far enough (an
  `audio_filter` an older splice9000 would quietly leave out), set
  `requires`: a splice9000 older than that refuses the adapter and says
  which version it needs.

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
| `initialize` | `splice9000` (version), `protocol` (`2`), `permissions`, `ffmpeg`, `ffprobe` (paths, or null) | Right after starting |
| `command` | `command`: the command's `id` | One of its commands was chosen |
| `event` | `kind` plus fields below | Something happened, if listed in `events` (needs `edit.read`) |
| `shutdown` | | splice9000 is quitting or the adapter is being removed; exit soon |

| Event `kind` | Fields |
|---|---|
| `changed` | `duration`, `clips` (how many): at most once a frame while editing |
| `media` | `paths`: the media list, when files are added or a project opened (protocol 3) |
| `exported` | `path` |
| `saved` | `path`: a project file was saved |

### Adapter → splice9000

| Method | Params | Result | Permission |
|---|---|---|---|
| `edit.get` | | The edit (below) | `edit.read` |
| `edit.set` | `clips`: `[{ "media", "src_in", "src_out", "fade" }]` (`fade` is optional) | `null`. Replaces the timeline as one undo step. Each clip is checked against its media file | `edit.change` |
| `edit.import` | `paths`, `append` (bool) | `null`. Adds media; with `append`, also to the end of the timeline | `edit.change` |
| `playhead.set` | `time` (seconds) | The playhead, snapped to a frame | `edit.change` |
| `playback.set` | `rate`: 1 real time, 2 twice as fast, 0 stopped, below 0 backward; at most 8 either way | The rate it plays at. Backward shows pictures without sound | none |
| `media.waveform` | `path` (a media file), `per_second`, `peaks`: numbers 0 to 255, the loudest moment in each `1 / per_second` | `null`. Drawn on that file's clips until the adapter stops | none |
| `media.thumbnails` | `path`, `image` (a PNG of pictures in a grid), `width`, `height` (each picture's), `columns`, `count`, `every` (seconds between pictures) | `null`. Picture *k* is the file at *k* × `every`; drawn on its clips until the adapter stops | none |
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
  "rate": 0,
  "selected": 1,
  "file": "/Users/me/trip.splice",
  "media": [
    { "path": "/Users/me/beach.mp4", "duration": 20.0, "width": 1920, "height": 1080, "fps": 30.0, "audio": true }
  ],
  "clips": [
    { "media": 0, "src_in": 2.0, "src_out": 8.0, "start": 0.0, "fade": 0.0 }
  ]
}
```

Times are seconds. `src_in` and `src_out` are times in the media file;
`start` is where the clip begins on the timeline. `selected` is a clip
index, or `null`. `rate` is how fast it's playing (0 stopped, below 0
backward).

`fade` is seconds of crossfade from the clip before (protocol 3). The fade
is centered on the cut and borrows footage from past both clips' ends, so
it never moves anything on the timeline; where a file has no more footage,
its last (or first) picture holds. splice9000 keeps a fade to at most half
of either clip. Older adapters that leave `fade` out of `edit.set` make
cuts without fades.

## Example exchange

```
→ {"method":"initialize","params":{"splice9000":"0.0.8","protocol":3,"permissions":["edit.read"],"ffmpeg":"/opt/homebrew/bin/ffmpeg","ffprobe":"/opt/homebrew/bin/ffprobe"}}
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

- **1** (splice9000 0.0.5): commands, panels, export formats, the edit,
  events and prompts.
- **2** (splice9000 0.0.6): `audio_filter` on export formats, and
  `requires` in adapter.toml.
- **3** (splice9000 0.0.8; 0.0.7 was tagged but not released): `playback.set`, `media.waveform`,
  `media.thumbnails`, the `media` event, `fade` and `rate` in the edit, and
  `[snapping]` in adapter.toml. Adapters using them should say
  `requires = "0.0.7"`.
