//! The menus: File, Edit, View, Playback, Help. On macOS they're the
//! system's own menu bar at the top of the screen; on Windows and Linux
//! they're drawn in the window's top strip. Both come from `MENUS`, every
//! item is a named command run exactly like its key, and the shortcuts shown
//! are the user's own from keys.toml.

use crate::keys::Keymap;
use crate::layout::PANELS;

/// Each menu's commands; "-" is a separator, "panels" the panel toggles.
pub const MENUS: &[(&str, &[&str])] = &[
    ("File", &["new", "open", "-", "save", "save as", "-", "import", "export"]),
    ("Edit", &["undo", "redo", "-", "split", "delete", "append"]),
    ("View", &["panels", "-", "layout edit", "layout reset", "-", "zoom in", "zoom out", "zoom fit"]),
    ("Playback", &["play", "mute", "-", "start", "end", "-", "frame previous", "frame next", "cut previous", "cut next"]),
    ("Help", &["panel help", "panel log", "-", "config"]),
];

/// The menu text for a command.
pub fn label(command: &str) -> String {
    if let Some(kind) = command.strip_prefix("panel ") {
        return crate::layout::title(kind).to_owned();
    }
    match command {
        "new" => "New Project",
        "open" => "Open Project…",
        "save" => "Save",
        "save as" => "Save As…",
        "import" => "Import Media…",
        "export" => "Export Video…",
        "undo" => "Undo",
        "redo" => "Redo",
        "split" => "Split at Playhead",
        "delete" => "Delete Clip",
        "append" => "Add Media to End",
        "layout edit" => "Edit Layout",
        "layout reset" => "Reset Layout",
        "zoom in" => "Zoom In",
        "zoom out" => "Zoom Out",
        "zoom fit" => "Zoom to Fit",
        "play" => "Play / Pause",
        "mute" => "Mute",
        "start" => "Go to Start",
        "end" => "Go to End",
        "frame previous" => "Previous Frame",
        "frame next" => "Next Frame",
        "cut previous" => "Previous Cut",
        "cut next" => "Next Cut",
        "config" => "Open Settings Folder",
        "quit" => "Quit splice9000",
        other => other,
    }
    .to_owned()
}

/// The panel toggles, in menu order.
pub fn panel_commands() -> impl Iterator<Item = String> {
    PANELS.iter().map(|(k, _)| format!("panel {k}"))
}

/// keys.toml's "cmd+shift+z" as a menu accelerator, "CmdOrCtrl+Shift+Z".
/// Only keys with cmd or ctrl go in menus: a plain key there would be
/// taken from every text field.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn accelerator(key: &str) -> Option<String> {
    let parts: Vec<&str> = key.split('+').collect();
    if !parts.iter().any(|p| *p == "cmd" || *p == "ctrl") {
        return None;
    }
    let out: Vec<String> = parts
        .iter()
        .map(|p| match *p {
            "cmd" => "CmdOrCtrl".to_owned(),
            "ctrl" => "Ctrl".to_owned(),
            "alt" => "Alt".to_owned(),
            "shift" => "Shift".to_owned(),
            "plus" => "Equal".to_owned(),
            "minus" => "Minus".to_owned(),
            "comma" => "Comma".to_owned(),
            other => other.to_uppercase(),
        })
        .collect();
    Some(out.join("+"))
}

/// The shortcut as people read it: ⌘⇧Z on macOS, Ctrl+Shift+Z elsewhere.
pub fn shortcut_text(key: &str) -> String {
    let mac = cfg!(target_os = "macos");
    let parts: Vec<String> = key
        .split('+')
        .map(|p| match p {
            "cmd" if mac => "⌘".into(),
            "cmd" | "ctrl" if !mac => "Ctrl".into(),
            "ctrl" => "⌃".into(),
            "alt" if mac => "⌥".into(),
            "shift" if mac => "⇧".into(),
            "alt" => "Alt".into(),
            "shift" => "Shift".into(),
            "plus" => "+".into(),
            "minus" => "−".into(),
            "comma" => ",".into(),
            "space" => "Space".into(),
            "left" => "←".into(),
            "right" => "→".into(),
            "up" => "↑".into(),
            "down" => "↓".into(),
            k => k.chars().next().map_or(String::new(), |c| c.to_uppercase().chain(k.chars().skip(1)).collect()),
        })
        .collect();
    parts.join(if mac { "" } else { "+" })
}

#[cfg(target_os = "macos")]
pub use native::MenuBar;

#[cfg(target_os = "macos")]
mod native {
    use std::sync::{Arc, Mutex, OnceLock};

    use muda::accelerator::Accelerator;
    use muda::{AboutMetadataBuilder, CheckMenuItem, Icon, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};

    use super::{Keymap, MENUS, accelerator, label, panel_commands};

    pub struct MenuBar {
        _menu: Menu,
        /// Item ids are indexes into this.
        commands: Vec<String>,
        panels: Vec<(String, CheckMenuItem)>,
        chosen: Arc<Mutex<Vec<String>>>,
    }

    static CHOSEN: OnceLock<Arc<Mutex<Vec<String>>>> = OnceLock::new();

    impl MenuBar {
        pub fn install(keys: &Keymap, ctx: &egui::Context, icon: Option<(Vec<u8>, u32, u32)>) -> Self {
            let mut commands = Vec::new();
            let accel = |c: &str| keys.key_for(c).and_then(|k| accelerator(&k)).and_then(|a| a.parse::<Accelerator>().ok());
            let item = |commands: &mut Vec<String>, c: &str| {
                let id = commands.len().to_string();
                commands.push(c.to_owned());
                MenuItem::with_id(id, label(c), true, accel(c))
            };
            let sep = PredefinedMenuItem::separator;
            let menu = Menu::new();

            let app = Submenu::new("splice9000", true);
            let about = AboutMetadataBuilder::new()
                .name(Some("splice9000"))
                .version(Some(env!("CARGO_PKG_VERSION")))
                .comments(Some("A bare, modular video editor."))
                .copyright(Some("© 2026 Vishal Mohan · MIT License"))
                .icon(icon.and_then(|(rgba, w, h)| Icon::from_rgba(rgba, w, h).ok()))
                .build();
            let settings = item(&mut commands, "config");
            let quit = item(&mut commands, "quit");
            app.append_items(&[
                &PredefinedMenuItem::about(Some("About splice9000"), Some(about)),
                &sep(),
                &settings,
                &sep(),
                &PredefinedMenuItem::hide(None),
                &PredefinedMenuItem::hide_others(None),
                &PredefinedMenuItem::show_all(None),
                &sep(),
                &quit,
            ])
            .expect("menu builds");
            menu.append(&app).expect("menu builds");

            let mut panels = Vec::new();
            let mut last = None;
            for (title, entries) in MENUS {
                let sub = Submenu::new(*title, true);
                for c in *entries {
                    match *c {
                        "-" => sub.append(&sep()).expect("menu builds"),
                        "panels" => {
                            let list = Submenu::new("Panels", true);
                            for p in panel_commands() {
                                let id = commands.len().to_string();
                                commands.push(p.clone());
                                let check = CheckMenuItem::with_id(id, label(&p), true, false, accel(&p));
                                list.append(&check).expect("menu builds");
                                panels.push((p, check));
                            }
                            sub.append(&list).expect("menu builds");
                        }
                        c => sub.append(&item(&mut commands, c)).expect("menu builds"),
                    }
                }
                if *title == "Help" {
                    last = Some(sub);
                } else {
                    menu.append(&sub).expect("menu builds");
                }
            }
            let window = Submenu::new("Window", true);
            window
                .append_items(&[&PredefinedMenuItem::minimize(None), &PredefinedMenuItem::maximize(None), &PredefinedMenuItem::fullscreen(None)])
                .expect("menu builds");
            menu.append(&window).expect("menu builds");
            menu.init_for_nsapp();
            window.set_as_windows_menu_for_nsapp();
            if let Some(help) = last {
                menu.append(&help).expect("menu builds");
                help.set_as_help_menu_for_nsapp();
            }

            // muda takes one handler per process; it hands ids to whichever
            // menu bar is current.
            let chosen = CHOSEN
                .get_or_init(|| {
                    let chosen = Arc::new(Mutex::new(Vec::new()));
                    let (c, ctx) = (chosen.clone(), ctx.clone());
                    MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
                        c.lock().unwrap_or_else(|e| e.into_inner()).push(e.id.0);
                        ctx.request_repaint();
                    }));
                    chosen
                })
                .clone();
            Self { _menu: menu, commands, panels, chosen }
        }

        /// The commands chosen since last asked.
        pub fn take(&self) -> Vec<String> {
            let ids = std::mem::take(&mut *self.chosen.lock().unwrap_or_else(|e| e.into_inner()));
            ids.iter().filter_map(|id| self.commands.get(id.parse::<usize>().ok()?).cloned()).collect()
        }

        /// Ticks the panels that are showing.
        pub fn sync(&self, shown: &dyn Fn(&str) -> bool) {
            for (c, item) in &self.panels {
                let on = shown(c.trim_start_matches("panel "));
                if item.is_checked() != on {
                    item.set_checked(on);
                }
            }
        }
    }
}

/// Windows and Linux: the menus are drawn in the window (see `draw`).
#[cfg(not(target_os = "macos"))]
pub struct MenuBar;

#[cfg(not(target_os = "macos"))]
impl MenuBar {
    pub fn install(_: &Keymap, _: &egui::Context, _: Option<(Vec<u8>, u32, u32)>) -> Self {
        MenuBar
    }
    pub fn take(&self) -> Vec<String> {
        Vec::new()
    }
    pub fn sync(&self, _: &dyn Fn(&str) -> bool) {}
}

/// The menus drawn in the window, for systems without a menu bar of their
/// own. Chosen commands go into `out`.
#[cfg_attr(target_os = "macos", allow(dead_code))]
pub fn draw(ui: &mut egui::Ui, keys: &Keymap, shown: &dyn Fn(&str) -> bool, out: &mut Vec<String>) {
    let entries = |ui: &mut egui::Ui, list: &[&str], out: &mut Vec<String>| {
        ui.set_min_width(220.0);
        for c in list {
            match *c {
                "-" => {
                    ui.separator();
                }
                "panels" => {
                    ui.menu_button("Panels", |ui| {
                        for p in panel_commands() {
                            if ui.add(egui::Button::selectable(shown(p.trim_start_matches("panel ")), label(&p))).clicked() {
                                out.push(p);
                                ui.close();
                            }
                        }
                    });
                }
                c => {
                    let key = keys.key_for(c).map(|k| shortcut_text(&k)).unwrap_or_default();
                    if ui.add(egui::Button::new(label(c)).shortcut_text(key)).clicked() {
                        out.push(c.to_owned());
                        ui.close();
                    }
                }
            }
        }
    };
    egui::MenuBar::new().ui(ui, |ui| {
        for (title, list) in MENUS {
            ui.menu_button(*title, |ui| {
                entries(ui, list, out);
                if *title == "File" {
                    ui.separator();
                    if ui.button("Exit").clicked() {
                        out.push("quit".into());
                    }
                }
            });
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accelerators_and_labels() {
        assert_eq!(accelerator("cmd+shift+z").as_deref(), Some("CmdOrCtrl+Shift+Z"));
        assert_eq!(accelerator("cmd+plus").as_deref(), Some("CmdOrCtrl+Equal"));
        assert_eq!(accelerator("cmd+0").as_deref(), Some("CmdOrCtrl+0"));
        assert_eq!(accelerator("space"), None);
        // Every menu entry is a real command with a label of its own.
        for (_, list) in MENUS {
            for c in list.iter().filter(|c| !matches!(**c, "-" | "panels")) {
                assert!(crate::keys::COMMANDS.iter().any(|k| k.0 == *c), "{c} isn't a command");
                assert_ne!(label(c), *c, "{c} has no label");
            }
        }
        for p in panel_commands() {
            assert!(crate::keys::COMMANDS.iter().any(|k| k.0 == p), "{p} isn't a command");
        }
    }
}
