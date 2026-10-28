# How splice9000 is built

For people changing the code. The ideals behind these choices are in
[docs/philosophy.md](docs/philosophy.md).

## The parts

| File | What it does |
|---|---|
| `main.rs` | `App`: all state, the commands, config loading, the session |
| `timeline.rs` | the edit: clips and the running totals that make lookups fast |
| `layout.rs` | the panel tree, splitters, layout edit mode, `layout.toml` |
| `panels.rs` | what each panel draws |
| `preview.rs` | the preview decoder thread, and the sound feeder |
| `audio.rs` | the sound output, its lock-free ring, and the playback clock |
| `menu.rs` | the menus: macOS's own menu bar, or drawn in the window |
| `theme.rs` | the palette every surface draws with |
| `adapters.rs` | adapters: finding, installing, approving, running, removing |
| `hosting.rs` | what the app does for adapters: their requests, prompts, menus, keys |
| `export.rs` | the ffmpeg command for an export, and running it |
| `media.rs` | finding ffmpeg, reading files with ffprobe |
| `keys.rs` | `COMMANDS`, the one list of commands, and `keys.toml` |
| `settings.rs` | `settings.toml` |
| `watch.rs` | notices saved config files (from Leaf) |
| `shell/` | the window, drawn by the CPU (from Folio) |
| `log.rs` | the Log panel's lines |

## One owner of state

Everything lives in `App`, on the interface thread. Three kinds of
background work exist, and each hands results back without locks on the
edit itself:

- **ffprobe** (importing): a thread per import sends results down a channel.
- **Preview**: one worker thread, one request channel in, a one-frame mailbox
  out. The worker always takes the newest request and drops the rest, so
  scrubbing never builds a queue. A frame the interface hasn't shown yet is
  replaced, not queued.
- **Export**: a thread runs ffmpeg and updates a progress value.

Each of them calls `request_repaint` when there's something new. Nothing
redraws on a timer, so an idle editor uses no CPU.

## The timeline's maths

`Timeline` keeps `starts`, the running total of clip lengths (one more entry
than clips, the last being the edit's length). So:

- what's under time `t` (`locate`): a binary search, O(log n)
- the first clip on screen (`first_after`): a binary search, so drawing
  costs only the clips in view however long the edit is
- where a dropped clip goes (`slot_at`): a binary search on clip midpoints
- an edit to clip `i`: only the totals from `i` on are redone

Times are seconds (`f64`). The playhead snaps to the edit's frames; trims
snap to their source's frames.

A clip's `fade` is a crossfade from the clip before, centered on the cut
and borrowing footage from past both clips' ends, so it never moves
anything: `starts` don't change. `pieces` turns the edit into what is
played or exported: each clip's footage, and between them, a piece that
reads both files and blends them (where a file runs out, ffmpeg's `tpad`
holds its last picture). Preview and export both work from pieces, so a
fade previews as it exports.

## Preview

Frames come out of ffmpeg already scaled to the preview panel's exact pixel
size (`fps` first, then `scale`, so frames that would be dropped aren't
scaled), as RGBA read straight into the texture's own memory. Shown at that
size on whole pixels, the CPU renderer copies them instead of filtering them
(`draw_rect` in `shell/raster.rs`).

Playing streams each clip from one ffmpeg process. The picture is paced by
the sound: late frames are dropped, not shown late. The next
clip's ffmpeg is started while the current one plays, so cuts don't stall.
Stills decode in software and playback with the system's decoder; see the
README for the numbers behind that.

Playing at `rate` asks ffmpeg for `fps / rate` frames a second of footage,
so the screen still gets about `fps` a second, and speeds the sound up with
`atempo` (same pitch). Playing backward is the still path run in reverse:
the playhead moves back with the clock and the worker decodes the latest
picture asked for.

## Sound

ffmpeg decodes each clip's sound at the output device's own rate and
channel count, so nothing is resampled in splice9000. A feeder thread writes
exactly as many frames as each clip is long into a lock-free ring (one
writer, one reader, a quarter second deep), padding with silence where a file
has no sound or ends early, so cuts stay in sync. The device's callback reads
the ring and counts the frames it really played: that count is the clock
the picture follows. If the sound stalls, the picture waits instead of
drifting. The device opens on first play and is paused whenever nothing
plays.

## Export

One ffmpeg run for the whole edit (`export::args`, unit tested):

- each clip is its own input with `-ss`/`-t` before `-i`, so only the parts
  used are decoded
- a clip is resampled (`fps`) or scaled and padded only when it differs from
  the output
- clips without audio get silence of the same length; audio is padded to
  each clip's length so it can't drift
- with no audio anywhere, there's no audio track at all
- `concat` joins them; progress comes from `-progress pipe:1`

## Layout

A `Node` is a panel or a split (row or column) with children and size
shares. `tidy` makes any hand-edited tree sound: sizes add up, empty and
one-child splits disappear, so a bad `layout.toml` can't break the window.
Moving a panel marks its old place, inserts it at the new one (which may
shift paths) and then removes the mark. Unknown panel names show a note
instead of failing, so layouts made by newer versions still open.

## Adapters

An adapter is a folder with an `adapter.toml` and, usually, a program that
speaks JSON lines over stdin/stdout ([docs/adapters.md](docs/adapters.md)).
The host is Leaf's, adapted: a reader thread per adapter turns its output
into messages for the interface thread, which answers them in
`hosting.rs`, so adapters never touch app state directly and the edit
stays single-owner. Requests that need the user (`ui.confirm`,
`ui.prompt`) are answered when the prompt is, without blocking the frame.

Adapters come from two folders: built-in ones beside the program (inside
the app on macOS) and installed ones in the config folder. Removing a
built-in one writes its name to `adapters.toml` instead of touching the
app's files, so Add Back works offline and app updates still update it.
Export formats are plain data (`[[exports]]`) that `export::args` reads, so
the most common kind of adapter runs no code; so is `[snapping]`.

The editing tools are adapters on small hooks in the core: `playback.set`
(Shuttle), `fade` in the edit (Crossfades), `media.waveform` and
`media.thumbnails` (drawn in `panels::draw_looks`; what an adapter sent
goes when it stops), and `[snapping]`. With the adapter removed, the hook
costs nothing. The built-in ones are built from the catalog repo at the
commit in `adapters/bundled.txt` (`scripts/stage-adapters.sh`).

## Commands

Every action is a named command in `keys::COMMANDS`. Keys, menus, buttons
and the Help panel all run them through `App::run`. New features add a row
there. Adapters' commands are named `adapter "Name" id` and their export
formats `export "Name" id`, and work the same way.

## Layout look

Panels are laid edge to edge with a one-point gap, and the window's line
color shows through it: that's the divider. Its grab area is four points
either side, and dividers are registered after the panels so they win at the
edges. Each panel has a 24-point header to drag it by; the drop target
shows the half (or the middle) it would land in.

## The window

`shell/` is Folio's: winit and egui, with pixels drawn by the CPU into a
buffer shared with the window server (two on macOS, see the README), and
only the screen tiles that changed are drawn. On Windows and Linux the whole
buffer is presented until one frame has reached a visible window: the first
frame is drawn while the window is still hidden, those systems drop it, and
sending only changed areas after that left most of the window blank (found
by CI's screenshots).

## Compatibility

Settings, keys, layouts and `.splice` files are read with every field
optional and unknown fields ignored. Keep it that way: old files must keep
opening, and command names, once shipped, never change meaning.
