//! Named commands and the keys that run them.
//!
//! `COMMANDS` is the one list of everything splice9000 can do. Key
//! bindings, the Help panel and the buttons all run commands by name, and
//! keys.toml binds any key to any of them.

use std::collections::BTreeMap;

use egui::{Key, KeyboardShortcut, Modifiers};

/// Name, default key, what it does.
pub const COMMANDS: &[(&str, &str, &str)] = &[
    ("play", "space", "Play or pause"),
    ("mute", "m", "Turn the sound off or on"),
    ("split", "s", "Cut the clip at the playhead"),
    ("delete", "backspace", "Remove the selected clip"),
    ("append", "enter", "Add the selected media to the end"),
    ("undo", "cmd+z", "Undo"),
    ("redo", "cmd+shift+z", "Redo"),
    ("frame next", "right", "One frame forward"),
    ("frame previous", "left", "One frame back"),
    ("cut next", "down", "Go to the next cut"),
    ("cut previous", "up", "Go to the previous cut"),
    ("start", "home", "Go to the start"),
    ("end", "end", "Go to the end"),
    ("zoom in", "cmd+plus", "Zoom the timeline in"),
    ("zoom out", "cmd+minus", "Zoom the timeline out"),
    ("zoom fit", "cmd+0", "Fit the edit in the timeline"),
    ("import", "cmd+i", "Add media files"),
    ("export", "cmd+e", "Export the edit"),
    ("new", "cmd+n", "Start an empty project"),
    ("open", "cmd+o", "Open a project"),
    ("save", "cmd+s", "Save the project"),
    ("save as", "cmd+shift+s", "Save the project under a new name"),
    ("layout edit", "cmd+l", "Move, add and remove panels"),
    ("layout reset", "", "Put the panels back as they came"),
    ("config", "cmd+comma", "Open the folder with settings, keys and layout"),
    ("quit", "cmd+q", "Quit"),
    ("panel preview", "", "Show or hide the Preview panel"),
    ("panel timeline", "", "Show or hide the Timeline panel"),
    ("panel media", "", "Show or hide the Media panel"),
    ("panel transport", "", "Show or hide the Transport panel"),
    ("panel clip", "", "Show or hide the Clip panel"),
    ("panel export", "", "Show or hide the Export panel"),
    ("panel log", "", "Show or hide the Log panel"),
    ("panel help", "", "Show or hide the Help panel"),
];

pub const EXAMPLE: &str = r#"# splice9000 keys: "key" = "command". Saved changes apply at once.
# cmd is Cmd on macOS and Ctrl elsewhere; also ctrl, alt, shift.
# The commands are listed in the Help panel (and in COMMANDS in keys.rs).
# To turn a default key off, bind it to "none".
#
# "k" = "play"
# "x" = "split"
# "delete" = "delete"
# "cmd+r" = "layout reset"
"#;

pub struct Keymap {
    /// Most modifiers first, so cmd+shift+z wins over cmd+z.
    bindings: Vec<(KeyboardShortcut, String)>,
}

impl Keymap {
    /// The defaults with keys.toml's entries on top. Bad entries are
    /// skipped and returned as problems.
    pub fn load(text: &str) -> (Self, Vec<String>) {
        let mut problems = Vec::new();
        let mut map: BTreeMap<String, String> =
            COMMANDS.iter().filter(|c| !c.1.is_empty()).map(|c| (c.1.to_owned(), c.0.to_owned())).collect();
        match toml::from_str::<BTreeMap<String, String>>(text) {
            Ok(user) => {
                for (key, cmd) in user {
                    if cmd != "none" && !COMMANDS.iter().any(|c| c.0 == cmd) {
                        problems.push(format!("keys.toml: no command called \"{cmd}\""));
                        continue;
                    }
                    map.insert(key, cmd);
                }
            }
            Err(e) => problems.push(format!("keys.toml: {}", e.message())),
        }
        let mut bindings = Vec::new();
        for (key, cmd) in map {
            if cmd == "none" {
                continue;
            }
            match parse(&key) {
                Some(s) => bindings.push((s, cmd)),
                None => problems.push(format!("keys.toml: can't read the key \"{key}\"")),
            }
        }
        bindings.sort_by_key(|(s, _)| std::cmp::Reverse(count(s.modifiers)));
        (Self { bindings }, problems)
    }

    /// The commands whose keys were pressed this frame. While typing into a
    /// field only keys with cmd/ctrl count.
    pub fn take(&self, ctx: &egui::Context) -> Vec<String> {
        let typing = ctx.egui_wants_keyboard_input();
        ctx.input_mut(|i| {
            self.bindings
                .iter()
                .filter(|(s, _)| !typing || s.modifiers.command || s.modifiers.ctrl)
                .filter(|(s, _)| i.consume_shortcut(s))
                .map(|(_, c)| c.clone())
                .collect()
        })
    }

    /// The key for `command`, as written in keys.toml.
    pub fn key_for(&self, command: &str) -> Option<String> {
        self.bindings.iter().find(|(_, c)| c == command).map(|(s, _)| describe(s))
    }
}

fn count(m: Modifiers) -> u8 {
    m.alt as u8 + m.shift as u8 + m.ctrl as u8 + m.command as u8
}

fn parse(text: &str) -> Option<KeyboardShortcut> {
    let mut modifiers = Modifiers::NONE;
    let mut key = None;
    for part in text.to_lowercase().split('+').map(str::trim) {
        match part {
            "cmd" | "command" => modifiers |= Modifiers::COMMAND,
            "ctrl" | "control" => modifiers |= Modifiers::CTRL,
            "alt" | "option" => modifiers |= Modifiers::ALT,
            "shift" => modifiers |= Modifiers::SHIFT,
            "" => key = Some(Key::Plus), // "cmd++"
            name => key = Some(key_named(name)?),
        }
    }
    Some(KeyboardShortcut::new(modifiers, key?))
}

fn key_named(name: &str) -> Option<Key> {
    Some(match name {
        "space" => Key::Space,
        "enter" | "return" => Key::Enter,
        "backspace" => Key::Backspace,
        "delete" | "del" => Key::Delete,
        "esc" | "escape" => Key::Escape,
        "tab" => Key::Tab,
        "left" => Key::ArrowLeft,
        "right" => Key::ArrowRight,
        "up" => Key::ArrowUp,
        "down" => Key::ArrowDown,
        "home" => Key::Home,
        "end" => Key::End,
        "pageup" => Key::PageUp,
        "pagedown" => Key::PageDown,
        "plus" | "=" | "equals" => Key::Equals,
        "minus" | "-" => Key::Minus,
        "comma" | "," => Key::Comma,
        "period" | "." => Key::Period,
        other => {
            let mut chars = other.chars();
            match (chars.next(), chars.next()) {
                (Some(c), None) if c.is_ascii_alphanumeric() => Key::from_name(&c.to_ascii_uppercase().to_string())?,
                _ => Key::from_name(other).or_else(|| Key::from_name(&other.to_uppercase()))?,
            }
        }
    })
}

fn describe(s: &KeyboardShortcut) -> String {
    let m = s.modifiers;
    let mut parts = Vec::new();
    if m.command {
        parts.push("cmd");
    }
    if m.ctrl {
        parts.push("ctrl");
    }
    if m.alt {
        parts.push("alt");
    }
    if m.shift {
        parts.push("shift");
    }
    let key = match s.logical_key {
        Key::Equals => "plus".to_owned(),
        Key::ArrowLeft => "left".to_owned(),
        Key::ArrowRight => "right".to_owned(),
        Key::ArrowUp => "up".to_owned(),
        Key::ArrowDown => "down".to_owned(),
        k => k.name().to_lowercase(),
    };
    parts.push(&key);
    parts.join("+")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_and_overrides() {
        let (k, problems) = Keymap::load("");
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(k.key_for("redo").as_deref(), Some("cmd+shift+z"));
        assert_eq!(k.key_for("zoom in").as_deref(), Some("cmd+plus"));
        assert_eq!(k.bindings.len(), COMMANDS.iter().filter(|c| !c.1.is_empty()).count());
        let (k, problems) = Keymap::load("\"k\" = \"play\"\n\"s\" = \"none\"\n\"q\" = \"fly\"\n\"cmd+bogus\" = \"play\"");
        assert_eq!(problems.len(), 2, "{problems:?}");
        assert!(k.key_for("split").is_none());
        assert!(k.bindings.iter().any(|(s, c)| c == "play" && s.logical_key == Key::K));
        let (_, problems) = Keymap::load("not toml [");
        assert_eq!(problems.len(), 1);
    }
}
