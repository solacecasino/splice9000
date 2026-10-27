# splice9000's ideals and how it's built

The same ideals as Folio and Leaf. For the full how and why, see [ARCHITECTURE.md](../ARCHITECTURE.md).

## Core ideals

1. **Start bare.** A fresh install opens to the essential thing and nothing else: no ribbon, no start screen, no account, no template store. Every extra feature is off by default or not in the app at all, and arrives only when the user installs it.
2. **Endlessly customizable, through files.** There's no settings window. Behaviour, shortcuts and looks live in plain text files (`settings.toml`, `keys.toml`, `layout.toml`) in one folder.
   - The app reloads them as soon as they're saved, with no restart.
   - Copying the folder moves your whole setup.
   - Every file ships with commented examples, so opening it shows you what's possible.
3. **Absolute backward compatibility.** Nothing that worked before may stop working: documents, configs, key bindings, the extension APIs, file associations. Old names keep working alongside new ones. If a change can't stay compatible, it doesn't ship without an explicit decision.
4. **The user owns their data.** Files are local and readable (JSON or TOML). There's no cloud, telemetry or account, and the app itself never goes online. Paths in bug reports hide the home folder.
5. **Lean and honest about cost.**
   - Memory, download size and idle CPU are features, and they're measured, not guessed. Every release reports real numbers, including what didn't work.
   - Idle means idle: the app wakes only for input, timers it set, or background work.
6. **Never crash on the user's account.** Bad config falls back to defaults and shows the problem on screen. ffmpeg errors go to the Log and never stop the app. Crashes leave a report for next time, and the edit is recovered.
7. **Native where it matters, drawn where it doesn't.** Menus and file dialogs use each system's own. Everything else is drawn in the theme's colors.

## Architecture

- **One small core.** Timeline, preview, layout and export in one binary of about 3k lines, plus Folio's CPU-drawn window. No codecs are linked in: video is read and written by the system's ffmpeg, run as a separate program, so a bad file or a crashing decoder can't take the editor down.
- **Everything is a panel, and panels go anywhere.** The window is a tree of splitters with panels at the leaves, like foobar2000. It's rearranged in layout edit mode or in `layout.toml`, and any panel can appear more than once.
- **Everything is a named command.** Every action has a name (`play`, `split`, `zoom fit`, `panel log`). One table drives key bindings, the menus, buttons and the Help panel.
- **One owner of state.** All app state lives in one struct on the UI thread. ffprobe, the preview decoder and export run on their own threads and hand results back through channels and a one-frame mailbox, plus a wake-up.
- **The maths is cheap.** Clip start times are kept as running totals, so finding what's under the playhead, what's on screen or where a drop goes is a binary search, and an edit redoes only the totals after it.
- **Decode only what's shown.** The preview is decoded at the size it's shown and copied to the screen unfiltered. The decoder always takes the newest request and drops stale ones. Export decodes only the parts of each file that are used.
- **The CPU draws only what changed** (from Folio). There's no GPU or graphics driver; changed screen tiles are redrawn into buffers shared with the window server.

## How the work is done

- **Release often, in small patches.** Each release has notes with honest measurements, compatibility notes and known limits.
- **CI on every platform.** Not set up yet for splice9000; the goal is the same as Folio's: build, test and screenshot macOS, Windows and Linux before any release.
- **Measure before and after.** No optimization claim without numbers; failed experiments are reverted and written down.
- **Docs are for people.** The README is a user guide; `ARCHITECTURE.md` explains the how and the why for contributors.
- **Small, safe commits.** Tests and a clean linter before a release. Never reformat the whole codebase at once.
