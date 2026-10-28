// No console window behind the app on Windows.
#![cfg_attr(windows, windows_subsystem = "windows")]

//! splice9000: a bare, modular video editor.
//!
//! One owner of state: everything lives in `App`, on the interface thread.
//! ffprobe, the preview decoder and export run on their own threads and
//! hand results back through channels and mailboxes, waking the interface
//! only when there's something new to show.

mod adapters;
mod audio;
mod export;
mod hosting;
mod keys;
mod layout;
mod log;
mod media;
mod menu;
mod panels;
mod preview;
mod settings;
mod shell;
mod theme;
mod timeline;
mod watch;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, channel};

use egui::{TextureHandle, TextureOptions, ViewportBuilder, ViewportCommand};
use serde::{Deserialize, Serialize};

use crate::export::{Format, Job};
use crate::keys::Keymap;
use crate::layout::{LayoutUi, Node, Side};
use crate::media::{Media, Tools};
use crate::preview::{Preview, Request, Segment};
use crate::settings::Settings;
use crate::timeline::{Clip, Timeline};

/// 256 px is all the dock, the window and About ever show.
const ICON_PNG: &[u8] = include_bytes!("../assets/icon-256.png");

/// The strip across the top of the window: on macOS under the transparent
/// titlebar (the menus are the system's), elsewhere the menus themselves.
const TOP: f32 = 32.0;

/// A project file (.splice), and the session splice9000 reopens.
#[derive(Default, Serialize, Deserialize)]
struct Project {
    #[serde(default)]
    media: Vec<Media>,
    #[serde(default)]
    clips: Vec<Clip>,
    #[serde(default)]
    playhead: f64,
    /// Session only: the project file it came from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    file: Option<PathBuf>,
}

/// Timeline view state.
pub struct View {
    /// Pixels per second.
    pub pps: f32,
    /// The time at the left edge.
    pub scroll: f64,
    /// The timeline panel's width when last drawn.
    pub width: f32,
    pub drag: panels::TimelineDrag,
}

pub struct App {
    dir: PathBuf,
    palette: theme::Palette,
    menu: menu::MenuBar,
    settings: Settings,
    keymap: Keymap,
    layout: Node,
    layout_ui: LayoutUi,
    config_changed: Arc<AtomicBool>,
    tools: Option<Tools>,

    media: Vec<Media>,
    timeline: Timeline,
    undo: Vec<Timeline>,
    redo: Vec<Timeline>,
    selected_media: Option<usize>,
    selected_clip: Option<usize>,
    playhead: f64,
    playing: bool,
    view: View,
    file: Option<PathBuf>,
    /// Unsaved changes to `file`.
    dirty: bool,
    /// The session needs writing.
    session_dirty: bool,

    preview: Preview,
    /// Where the preview panel drew its picture last frame.
    preview_area: Option<egui::Rect>,
    /// The system theme last asked for (the traffic lights follow it).
    system_dark: Option<bool>,
    /// The sound output, opened on first play.
    sound: Option<audio::Output>,
    muted: bool,
    texture: Option<TextureHandle>,
    /// What the preview last asked for: media, source frame, size.
    requested: Option<(usize, i64, u32, u32)>,
    preview_box: (u32, u32),
    probes: Option<Receiver<(Result<Media, String>, bool)>>,
    export: Option<Job>,
    last_export: Option<Result<String, String>>,
    /// Commands from buttons, run at the start of the next frame.
    queue: Vec<String>,
    adapters: adapters::Host,
    adapter_rx: Receiver<adapters::Message>,
    /// What adapters put in their panels, by panel kind.
    adapter_panels: HashMap<String, Vec<serde_json::Value>>,
    /// The question on screen, and the ones waiting behind it.
    prompt: Option<hosting::Prompt>,
    prompts: Vec<hosting::Prompt>,
    /// The export format picked in the Export panel (adapter, id); None:
    /// settings.toml's.
    export_choice: Option<(String, String)>,
    /// A short message at the bottom, until the time given.
    note: Option<(String, f64)>,
    /// The edit changed since adapters were last told.
    emit_changed: bool,
    /// The encoders this ffmpeg has, read the first time they matter.
    encoders: Option<Vec<String>>,
    title: String,
    ctx: egui::Context,
}

fn main() {
    if std::env::args().nth(1).as_deref() == Some("--version") {
        println!("splice9000 {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    let dir = config_dir();
    let _ = std::fs::create_dir_all(&dir);
    crash_report(&dir);
    let mut builder = ViewportBuilder::default()
        .with_title("splice9000")
        .with_app_id("splice9000")
        .with_inner_size([1280.0, 800.0])
        .with_min_inner_size([480.0, 320.0]);
    if let Some((rgba, width, height)) = icon_rgba() {
        builder = builder.with_icon(Arc::new(egui::IconData { rgba, width, height }));
    }
    if cfg!(target_os = "macos") {
        // Drawn under a transparent titlebar, so the top strip is ours.
        builder = builder.with_fullsize_content_view(true).with_titlebar_shown(false).with_title_shown(false);
    }
    if let Err(e) = shell::run(builder, move |ctx| App::new(ctx, dir)) {
        eprintln!("splice9000: {e}");
        std::process::exit(1);
    }
}

/// The app icon as RGBA pixels.
fn icon_rgba() -> Option<(Vec<u8>, u32, u32)> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(ICON_PNG));
    decoder.set_transformations(png::Transformations::normalize_to_color8() | png::Transformations::ALPHA);
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    (info.color_type == png::ColorType::Rgba).then(|| {
        buf.truncate(info.buffer_size());
        (buf, info.width, info.height)
    })
}

/// macOS shows a bare program's icon as a blank one in the dock; give it ours.
#[cfg(target_os = "macos")]
fn set_dock_icon() {
    use objc2::runtime::{AnyClass, AnyObject};
    use objc2::{ClassType, msg_send};
    let data = objc2_foundation::NSData::with_bytes(ICON_PNG);
    // SAFETY: plain AppKit calls on the main thread with live objects.
    unsafe {
        let (Some(image), Some(app)) = (AnyClass::get(c"NSImage"), AnyClass::get(c"NSApplication")) else { return };
        let image: *mut AnyObject = msg_send![image, alloc];
        let image: *mut AnyObject = msg_send![image, initWithData: &*data];
        let app: *mut AnyObject = msg_send![app, sharedApplication];
        if !image.is_null() && !app.is_null() {
            let _: () = msg_send![app, setApplicationIconImage: image];
        }
    }
    let _ = objc2_foundation::NSData::class();
}

fn config_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("SPLICE9000_CONFIG_DIR") {
        return d.into();
    }
    let home = || PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    if cfg!(target_os = "macos") {
        home().join("Library/Application Support/splice9000")
    } else if cfg!(windows) {
        PathBuf::from(std::env::var_os("APPDATA").unwrap_or_default()).join("splice9000")
    } else {
        std::env::var_os("XDG_CONFIG_HOME").map_or_else(|| home().join(".config"), PathBuf::from).join("splice9000")
    }
}

/// A crash leaves crash.txt; the next start reports it in the Log.
fn crash_report(dir: &Path) {
    let file = dir.join("crash.txt");
    if let Ok(text) = std::fs::read_to_string(&file) {
        log::error(format!("splice9000 stopped unexpectedly last time. Your edit was kept. {}", text.trim()));
        let _ = std::fs::rename(&file, dir.join("crash-last.txt"));
    }
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = std::fs::write(&file, format!("{info}"));
        default(info);
    }));
}

/// Writes `text` to `path` through a temporary file, so a crash midway
/// can't leave half a file.
fn write_atomic(path: &Path, text: &str) -> std::io::Result<()> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(tmp, path)
}

impl App {
    fn new(ctx: &egui::Context, dir: PathBuf) -> Self {
        for (name, text) in [("settings.toml", settings::EXAMPLE), ("keys.toml", keys::EXAMPLE)] {
            let path = dir.join(name);
            if !path.exists() {
                let _ = std::fs::write(path, text);
            }
        }
        let layout_path = dir.join("layout.toml");
        if !layout_path.exists() {
            let _ = std::fs::write(&layout_path, layout::to_toml(&layout::default_layout()));
        }
        let config_changed = Arc::new(AtomicBool::new(false));
        {
            let (flag, ctx) = (config_changed.clone(), ctx.clone());
            watch::config_folder(&dir, move || {
                flag.store(true, Ordering::Relaxed);
                ctx.request_repaint();
            });
        }
        #[cfg(target_os = "macos")]
        set_dock_icon();
        let (tx, adapter_rx) = channel();
        let adapters = {
            let (tx, ctx) = (std::sync::Mutex::new(tx), ctx.clone());
            let env = adapters::Env { config: dir.clone(), builtin: adapters::builtin_dir(), ffmpeg: None, ffprobe: None };
            adapters::Host::new(env, move |m| {
                let _ = tx.lock().unwrap_or_else(|e| e.into_inner()).send(m);
                ctx.request_repaint();
            })
        };
        let mut app = Self {
            menu: menu::MenuBar::install(&Keymap::load("", &[]).0, ctx, None, &menu::Extras::default()),
            palette: theme::Palette::new(&settings::Look::default()),
            dir,
            settings: Settings::default(),
            keymap: Keymap::load("", &[]).0,
            layout: layout::default_layout(),
            layout_ui: LayoutUi::default(),
            config_changed,
            tools: None,
            media: Vec::new(),
            timeline: Timeline::default(),
            undo: Vec::new(),
            redo: Vec::new(),
            selected_media: None,
            selected_clip: None,
            playhead: 0.0,
            playing: false,
            view: View { pps: 60.0, scroll: 0.0, width: 800.0, drag: panels::TimelineDrag::None },
            file: None,
            dirty: false,
            session_dirty: false,
            preview: Preview::new(None, false, || {}),
            preview_area: None,
            system_dark: None,
            sound: None,
            muted: false,
            texture: None,
            requested: None,
            preview_box: (640, 360),
            probes: None,
            export: None,
            last_export: None,
            queue: Vec::new(),
            adapters,
            adapter_rx,
            adapter_panels: HashMap::new(),
            prompt: None,
            prompts: Vec::new(),
            export_choice: None,
            note: None,
            emit_changed: false,
            encoders: None,
            title: String::new(),
            ctx: ctx.clone(),
        };
        app.load_config(true);
        app.adapters.start_launch_adapters();
        let session = std::fs::read_to_string(app.dir.join("session.json")).ok().and_then(|t| serde_json::from_str::<Project>(&t).ok());
        if let Some(p) = session {
            app.file = p.file.clone();
            app.set_project(p);
        }
        // Files named on the command line (Open With on Windows and Linux).
        let named: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).filter(|p| p.exists()).collect();
        app.open_paths(named);
        app
    }

    /// (Re)reads settings.toml, keys.toml and layout.toml. A file that
    /// can't be read leaves the rest working and says why in the Log.
    fn load_config(&mut self, first: bool) {
        let read = |name: &str| std::fs::read_to_string(self.dir.join(name)).unwrap_or_default();
        let settings = match toml::from_str::<Settings>(&read("settings.toml")) {
            Ok(s) => s,
            Err(e) => {
                log::error(format!("settings.toml: {} (using the defaults until it's fixed)", e.message()));
                Settings::default()
            }
        };
        match layout::from_toml(&read("layout.toml")) {
            Ok(root) => self.layout = root,
            Err(e) => log::error(format!("layout.toml: {e} (keeping the current layout)")),
        }
        let restart = first
            || settings.ffmpeg != self.settings.ffmpeg
            || settings.preview.hwaccel != self.settings.preview.hwaccel;
        self.settings = settings;
        self.apply_look();
        if restart {
            self.tools = Tools::find(&self.settings.ffmpeg);
            match &self.tools {
                Some(t) => log::info(format!("Using {}", t.ffmpeg.display())),
                None => log::error("ffmpeg wasn't found. Install it (ffmpeg.org, or `brew install ffmpeg`) or set `ffmpeg = \"...\"` in settings.toml."),
            }
            let ctx = self.ctx.clone();
            self.preview = Preview::new(self.tools.clone(), self.settings.preview.hwaccel, move || ctx.request_repaint());
            self.requested = None;
            self.encoders = None;
            // Adapters get the same ffmpeg.
            self.adapters.env.ffmpeg = self.tools.as_ref().map(|t| t.ffmpeg.clone());
            self.adapters.env.ffprobe = self.tools.as_ref().map(|t| t.ffprobe.clone());
        }
        // Keys (the user's and adapters') and the menus that show them.
        self.refresh_commands();
        self.ctx.request_repaint();
    }

    fn apply_look(&mut self) {
        let look = &self.settings.look;
        self.palette = theme::Palette::new(look);
        let theme = if look.dark { egui::Theme::Dark } else { egui::Theme::Light };
        self.ctx.set_theme(theme);
        self.ctx.set_visuals_of(theme, self.palette.visuals());
        let size = look.font_size.clamp(8.0, 32.0);
        self.ctx.global_style_mut(|s| {
            use egui::TextStyle::*;
            for (style, scale) in [(Body, 1.0), (Button, 1.0), (Monospace, 1.0), (Small, 0.8), (Heading, 1.4)] {
                if let Some(f) = s.text_styles.get_mut(&style) {
                    f.size = size * scale;
                }
            }
        });
    }

    fn set_project(&mut self, p: Project) {
        self.stop();
        self.media = p.media;
        let n = self.media.len();
        self.timeline = Timeline::new(p.clips.into_iter().filter(|c| c.media < n).collect());
        self.playhead = p.playhead.clamp(0.0, self.timeline.duration());
        self.selected_clip = None;
        self.selected_media = None;
        self.undo.clear();
        self.redo.clear();
        self.requested = None;
        self.texture = None;
        self.dirty = false;
        self.zoom_fit();
    }

    fn project(&self) -> Project {
        Project { media: self.media.clone(), clips: self.timeline.clips().to_vec(), playhead: self.playhead, file: None }
    }

    pub fn format(&self) -> Format {
        Format::of(&self.timeline, &self.media, &self.settings.export)
    }

    /// The length of one frame of the edit.
    pub fn frame(&self) -> f64 {
        1.0 / self.format().fps
    }

    pub fn snap(&self, t: f64) -> f64 {
        let fps = self.format().fps;
        ((t * fps).round() / fps).clamp(0.0, self.timeline.duration())
    }

    /// Call before changing the timeline, so it can be undone.
    pub fn snapshot(&mut self) {
        self.stop();
        self.undo.push(self.timeline.clone());
        if self.undo.len() > 500 {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.edited();
    }

    pub fn edited(&mut self) {
        self.dirty = true;
        self.session_dirty = true;
        self.emit_changed = true;
    }

    pub fn seek(&mut self, t: f64) {
        self.stop();
        self.playhead = self.snap(t);
        self.session_dirty = true;
    }

    pub fn run(&mut self, command: &str) {
        if self.run_adapter_command(command) {
            return;
        }
        let frame = self.frame();
        match command {
            "play" => {
                if self.playing {
                    self.stop();
                } else {
                    self.play();
                }
            }
            "mute" => {
                self.muted = !self.muted;
                if let Some(o) = &self.sound {
                    o.shared.set_gain(if self.muted { 0.0 } else { self.settings.preview.volume });
                }
            }
            "split" => {
                let t = self.playhead;
                let mut next = self.timeline.clone();
                if next.split(t, frame * 0.5) {
                    self.snapshot();
                    self.timeline = next;
                }
            }
            "delete" => {
                if let Some(i) = self.selected_clip.filter(|&i| i < self.timeline.len()) {
                    self.snapshot();
                    self.timeline.remove(i);
                    self.selected_clip = None;
                    self.playhead = self.playhead.min(self.timeline.duration());
                }
            }
            "append" => {
                if let Some(m) = self.selected_media {
                    self.add_clip(m, self.timeline.len());
                }
            }
            "undo" | "redo" => {
                let (from, to) = if command == "undo" { (&mut self.undo, &mut self.redo) } else { (&mut self.redo, &mut self.undo) };
                if let Some(t) = from.pop() {
                    to.push(std::mem::replace(&mut self.timeline, t));
                    self.stop();
                    self.selected_clip = None;
                    self.playhead = self.playhead.min(self.timeline.duration());
                    self.edited();
                }
            }
            "frame next" => self.seek(self.playhead + frame),
            "frame previous" => self.seek(self.playhead - frame),
            "cut next" => {
                if let Some((i, _)) = self.timeline.locate(self.playhead) {
                    self.seek(self.timeline.start(i + 1));
                }
            }
            "cut previous" => {
                let t = self.playhead;
                let i = self.timeline.first_after(t - frame * 0.5).min(self.timeline.len());
                let start = if i < self.timeline.len() { self.timeline.start(i) } else { self.timeline.duration() };
                let to = if t - start > frame * 0.5 { start } else if i > 0 { self.timeline.start(i - 1) } else { 0.0 };
                self.seek(to);
            }
            "start" => self.seek(0.0),
            "end" => self.seek(self.timeline.duration()),
            "zoom in" => self.view.pps = (self.view.pps * 1.5).min(2000.0),
            "zoom out" => self.view.pps = (self.view.pps / 1.5).max(0.5),
            "zoom fit" => self.zoom_fit(),
            "import" => {
                let picked = rfd::FileDialog::new()
                    .add_filter("Video", &["mp4", "mov", "m4v", "mkv", "webm", "avi", "mts", "m2ts", "mpg", "mpeg", "wmv", "flv", "3gp", "mxf", "ts"])
                    .add_filter("All files", &["*"])
                    .pick_files();
                if let Some(paths) = picked {
                    self.import(paths, false);
                }
            }
            "export" => self.start_export(self.export_choice.clone()),
            "new" => {
                self.snapshot();
                let undo = std::mem::take(&mut self.undo);
                self.set_project(Project::default());
                self.undo = undo;
                self.file = None;
                self.session_dirty = true;
            }
            "open" => {
                if let Some(path) = rfd::FileDialog::new().add_filter("splice9000 project", &["splice"]).pick_file() {
                    match std::fs::read_to_string(&path).map_err(|e| e.to_string()).and_then(|t| serde_json::from_str::<Project>(&t).map_err(|e| e.to_string())) {
                        Ok(p) => {
                            self.set_project(p);
                            self.file = Some(path);
                            self.session_dirty = true;
                        }
                        Err(e) => log::error(format!("Couldn't open {}: {e}", path.display())),
                    }
                }
            }
            "save" | "save as" => {
                let path = match (&self.file, command) {
                    (Some(f), "save") => Some(f.clone()),
                    _ => rfd::FileDialog::new().add_filter("splice9000 project", &["splice"]).set_file_name("edit.splice").save_file(),
                };
                if let Some(path) = path {
                    let text = serde_json::to_string_pretty(&self.project()).unwrap_or_default();
                    match write_atomic(&path, &text) {
                        Ok(()) => {
                            self.adapters.emit("saved", serde_json::json!({ "path": path }));
                            self.file = Some(path);
                            self.dirty = false;
                            self.session_dirty = true;
                        }
                        Err(e) => log::error(format!("Couldn't save {}: {e}", path.display())),
                    }
                }
            }
            "layout edit" => self.layout_ui.editing = !self.layout_ui.editing,
            c if c.starts_with("panel ") => self.toggle_panel(&c["panel ".len()..]),
            "layout reset" => {
                self.layout = layout::default_layout();
                self.save_layout();
            }
            "config" => open_folder(&self.dir),
            "quit" => self.ctx.send_viewport_cmd(ViewportCommand::Close),
            other => log::error(format!("No command called \"{other}\"")),
        }
    }

    /// Hides a panel that's showing, or shows one that isn't (beside the
    /// preview). The last panel stays.
    fn toggle_panel(&mut self, kind: &str) {
        if !layout::PANELS.iter().any(|p| p.0 == kind) {
            return;
        }
        let mut at = Vec::new();
        if self.layout.find(kind, &mut at) {
            if at.is_empty() {
                return;
            }
            self.layout.remove(&at);
        } else {
            let mut beside = Vec::new();
            if !self.layout.find("preview", &mut beside) {
                beside.clear();
            }
            self.layout.insert(&beside, kind, Side::Right);
        }
        self.save_layout();
    }

    /// The strip across the top: the mark and name, the project, and the
    /// tools used most. Dragging it moves the window.
    fn top_strip(&mut self, ui: &mut egui::Ui, rect: egui::Rect) {
        use egui::{Align2, FontId, RichText, Sense, vec2};
        let pal = self.palette;
        let painter = ui.painter().clone();
        painter.rect_filled(rect, 0.0, pal.header);
        painter.rect_filled(egui::Rect::from_x_y_ranges(rect.x_range(), rect.bottom() - 1.0..=rect.bottom()), 0.0, pal.line);
        let drag = ui.interact(rect, egui::Id::unique("top strip"), Sense::click_and_drag());
        if drag.double_clicked() {
            let maximized = ui.input(|i| i.viewport().maximized.unwrap_or(false));
            ui.ctx().send_viewport_cmd(ViewportCommand::Maximized(!maximized));
        } else if drag.drag_started() {
            ui.ctx().send_viewport_cmd(ViewportCommand::StartDrag);
        }

        // Left: the mark and the name (after the traffic lights on macOS),
        // or the menus elsewhere.
        let mut x = rect.left() + if cfg!(target_os = "macos") { 84.0 } else { 8.0 };
        if !cfg!(target_os = "macos") {
            let menus = egui::Rect::from_min_max(egui::pos2(x, rect.top()), egui::pos2(rect.left() + 360.0, rect.bottom()));
            let mut mui = ui.new_child(egui::UiBuilder::new().max_rect(menus));
            let layout = &self.layout;
            let extras = self.menu_extras();
            menu::draw(&mut mui, &self.keymap, &extras, &|k| layout.contains(k), &mut self.queue);
            x = menus.right() + 8.0;
        }
        let cy = rect.center().y;
        panels::mark(&painter, egui::pos2(x, cy), 22.0, &pal);
        let name = painter.text(egui::pos2(x + 30.0, cy), Align2::LEFT_CENTER, "splice", FontId::proportional(14.0), pal.text);
        painter.text(egui::pos2(name.right() + 1.0, cy + 0.5), Align2::LEFT_CENTER, "9000", FontId::monospace(12.5), pal.accent);

        // Middle: the project.
        let project = self.file.as_ref().and_then(|f| f.file_stem()).map_or("Untitled".to_owned(), |s| s.to_string_lossy().into_owned());
        let project = if self.dirty { format!("{project}  •") } else { project };
        painter.text(rect.center(), Align2::CENTER_CENTER, project, FontId::proportional(12.0), pal.weak);

        // Right: tools.
        let tools = egui::Rect::from_min_max(egui::pos2(rect.center().x + 120.0, rect.top() + 4.0), egui::pos2(rect.right() - 10.0, rect.bottom() - 4.0));
        let mut tui = ui.new_child(egui::UiBuilder::new().max_rect(tools).layout(egui::Layout::right_to_left(egui::Align::Center)));
        tui.spacing_mut().item_spacing.x = 6.0;
        let can_export = !self.timeline.is_empty() && self.tools.is_some() && self.export.is_none();
        let export = egui::Button::new(RichText::new("Export").size(12.0).color(pal.on_accent)).fill(pal.accent);
        if tui.add_enabled(can_export, export).on_hover_text("Export the edit as a video").clicked() {
            self.queue.push("export".into());
        }
        if tui.add(egui::Button::new(RichText::new("Import").size(12.0))).on_hover_text("Add media files").clicked() {
            self.queue.push("import".into());
        }
        let layout = egui::Button::selectable(self.layout_ui.editing, RichText::new("Layout").size(12.0));
        if tui.add(layout).on_hover_text("Move, add and remove panels (or drag any panel's header)").clicked() {
            self.queue.push("layout edit".into());
        }
        let _ = vec2;
    }

    pub fn add_clip(&mut self, m: usize, at: usize) {
        let Some(media) = self.media.get(m) else { return };
        let clip = Clip { media: m, src_in: 0.0, src_out: media.duration };
        let was_empty = self.timeline.is_empty();
        self.snapshot();
        self.timeline.insert(at, clip);
        self.selected_clip = Some(at.min(self.timeline.len() - 1));
        if was_empty {
            self.zoom_fit();
        }
    }

    fn zoom_fit(&mut self) {
        let width = self.view.width.max(200.0);
        let d = self.timeline.duration();
        if d > 0.0 {
            self.view.pps = ((width - 40.0) / d as f32).clamp(0.5, 2000.0);
        }
        self.view.scroll = 0.0;
    }

    /// Reads media files with ffprobe on another thread. `append`: also add
    /// them to the end of the timeline once read.
    fn import(&mut self, paths: Vec<PathBuf>, append: bool) {
        let Some(tools) = self.tools.clone() else {
            log::error("Can't read media without ffmpeg.");
            return;
        };
        let (tx, rx) = channel();
        let ctx = self.ctx.clone();
        std::thread::spawn(move || {
            for p in paths {
                let r = tools.probe(&p).map_err(|e| format!("{}: {e}", p.display()));
                let _ = tx.send((r, append));
                ctx.request_repaint();
            }
        });
        self.probes = Some(rx);
    }

    fn take_probes(&mut self) {
        let Some(rx) = &self.probes else { return };
        let got: Vec<_> = rx.try_iter().collect();
        for (r, append) in got {
            match r {
                Ok(m) => {
                    let i = match self.media.iter().position(|x| x.path == m.path) {
                        Some(i) => {
                            self.media[i] = m;
                            i
                        }
                        None => {
                            self.media.push(m);
                            self.media.len() - 1
                        }
                    };
                    self.selected_media = Some(i);
                    if append {
                        self.add_clip(i, self.timeline.len());
                    }
                    self.edited();
                }
                Err(e) => log::error(e),
            }
        }
    }

    fn play(&mut self) {
        if self.timeline.is_empty() {
            return;
        }
        if self.playhead >= self.timeline.duration() - self.frame() * 0.5 {
            self.playhead = 0.0;
        }
        let Some((first, _)) = self.timeline.locate(self.playhead) else { return };
        let segs = (first..self.timeline.len())
            .map(|i| {
                let c = &self.timeline.clips()[i];
                let m = &self.media[c.media];
                Segment {
                    path: m.path.clone(),
                    width: m.width,
                    height: m.height,
                    audio: m.audio,
                    src_in: c.src_in,
                    src_out: c.src_out,
                    at: self.timeline.start(i),
                }
            })
            .collect();
        let fps = self.format().fps.min(self.settings.preview.max_fps.max(1.0));
        let (bw, bh) = self.preview_box;
        let gain = if self.muted { 0.0 } else { self.settings.preview.volume };
        let audio = self.sound_output().map(|o| {
            o.shared.set_gain(gain);
            o.play();
            o.shared.clone()
        });
        self.preview.send(Request::Play { segs, from: self.playhead, bw, bh, fps, audio });
        self.playing = true;
        self.requested = None;
    }

    /// The sound output, opened the first time it's wanted.
    fn sound_output(&mut self) -> Option<&audio::Output> {
        if !self.settings.preview.audio {
            return None;
        }
        if self.sound.is_none() {
            match audio::Output::open() {
                Ok(o) => self.sound = Some(o),
                Err(e) => {
                    log::error(format!("No sound while playing: {e}"));
                    self.settings.preview.audio = false;
                }
            }
        }
        self.sound.as_ref()
    }

    pub fn stop(&mut self) {
        if let Some(o) = &self.sound {
            o.pause();
        }
        if self.playing {
            self.preview.send(Request::Stop);
            self.playing = false;
            self.playhead = self.snap(self.playhead);
        }
    }

    fn update_preview(&mut self, ctx: &egui::Context) {
        let mail = self.preview.take();
        if let Some(e) = mail.error {
            log::error(e);
        }
        // A frame of playback that arrives after pausing would show the
        // picture a moment past the pause, then jump back.
        if let Some(f) = mail.frame.filter(|f| f.still || self.playing) {
            match &mut self.texture {
                Some(t) => t.set(f.image, TextureOptions::LINEAR),
                None => self.texture = Some(ctx.load_texture("preview", f.image, TextureOptions::LINEAR)),
            }
            if self.playing {
                self.playhead = f.time;
            }
        }
        if mail.ended && self.playing {
            self.playing = false;
            if let Some(o) = &self.sound {
                o.pause();
            }
            self.playhead = self.timeline.duration();
        }
        if self.playing {
            return;
        }
        let (bw, bh) = self.preview_box;
        let want = self.timeline.locate(self.playhead).map(|(i, src)| {
            let c = &self.timeline.clips()[i];
            let m = &self.media[c.media];
            (c.media, (src * m.fps + 1e-6).floor() as i64, bw, bh)
        });
        if want != self.requested {
            self.requested = want;
            match want {
                Some((mi, n, bw, bh)) => {
                    let m = &self.media[mi];
                    let seg = Segment {
                        path: m.path.clone(),
                        width: m.width,
                        height: m.height,
                        audio: false,
                        src_in: 0.0,
                        src_out: m.duration,
                        at: 0.0,
                    };
                    let t = (n as f64 / m.fps - 1e-4).max(0.0);
                    self.preview.send(Request::Still { seg, t, bw, bh });
                }
                None => self.texture = None,
            }
        }
    }

    /// Exports the edit, in settings.toml's format or an adapter's `preset`.
    fn start_export(&mut self, preset: Option<(String, String)>) {
        if self.export.is_some() {
            return;
        }
        let Some(tools) = self.tools.clone() else {
            log::error("Can't export without ffmpeg.");
            return;
        };
        if self.timeline.is_empty() {
            log::error("Nothing to export yet: add a clip to the timeline first.");
            return;
        }
        let spec = match &preset {
            Some((a, id)) => match self.adapters.export(a, id) {
                Some(s) => Some(s.clone()),
                None => {
                    log::error(format!("No export format \"{id}\" in {a}"));
                    return;
                }
            },
            None => None,
        };
        let missing = self.missing_encoders(spec.as_ref());
        if !missing.is_empty() {
            let what = spec.as_ref().map_or("This format".to_owned(), |s| s.label.clone());
            self.say(format!("{what} needs ffmpeg's {} encoder, which your ffmpeg doesn't have.", missing.join(" and ")));
            return;
        }
        let name = self.file.as_ref().and_then(|f| f.file_stem()).map_or("edit".into(), |s| s.to_string_lossy().into_owned());
        let dialog = match &spec {
            Some(s) => rfd::FileDialog::new().add_filter(&s.label, &[s.extension.as_str()]).set_file_name(format!("{name}.{}", s.extension)),
            None => rfd::FileDialog::new().add_filter("Video", &["mp4", "mov", "mkv", "webm"]).set_file_name(format!("{name}.mp4")),
        };
        let Some(out) = dialog.save_file() else {
            return;
        };
        self.stop();
        let fmt = self.format().with(spec.as_ref());
        let args = export::args(&self.timeline, &self.media, fmt, &self.settings.export, spec.as_ref(), &out);
        let ctx = self.ctx.clone();
        self.last_export = None;
        self.export = Some(Job::start(&tools, args, out, self.timeline.duration(), move || ctx.request_repaint()));
    }

    /// The encoders a format needs that this ffmpeg doesn't have.
    pub fn missing_encoders(&mut self, preset: Option<&adapters::ExportSpec>) -> Vec<String> {
        let Some(tools) = &self.tools else { return Vec::new() };
        let have = self.encoders.get_or_insert_with(|| tools.encoders());
        if have.is_empty() {
            return Vec::new(); // couldn't tell; let ffmpeg say
        }
        export::encoders_needed(&self.settings.export, preset).into_iter().filter(|e| !have.contains(e)).collect()
    }

    pub fn cancel_export(&self) {
        if let Some(job) = &self.export {
            job.cancel();
        }
    }

    fn check_export(&mut self) {
        let Some(job) = &self.export else { return };
        let finished = job.progress.lock().unwrap_or_else(|e| e.into_inner()).finished.take();
        if let Some(r) = finished {
            match &r {
                Ok(s) => {
                    log::info(format!("Exported {}: {s}", job.out.display()));
                    self.adapters.emit("exported", serde_json::json!({ "path": job.out }));
                }
                Err(e) => log::error(format!("Export failed: {e}")),
            }
            self.last_export = Some(r);
            self.export = None;
        }
    }

    fn save_layout(&self) {
        if let Err(e) = write_atomic(&self.dir.join("layout.toml"), &layout::to_toml(&self.layout)) {
            log::error(format!("Couldn't save layout.toml: {e}"));
        }
    }

    fn save_session(&mut self) {
        let mut p = self.project();
        p.file = self.file.clone();
        if let Ok(text) = serde_json::to_string(&p) {
            let _ = write_atomic(&self.dir.join("session.json"), &text);
        }
        self.session_dirty = false;
    }

    fn dropped_files(&mut self, ctx: &egui::Context) {
        let paths: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty()).collect());
        self.open_paths(paths);
    }

    /// Opens dropped files, or files named when splice9000 was started:
    /// adapters are offered for install, a project opens, media is added.
    fn open_paths(&mut self, paths: Vec<PathBuf>) {
        if paths.is_empty() {
            return;
        }
        // Adapters (zips or folders with an adapter.toml) are offered for
        // install; the rest is projects and media.
        let (packages, paths): (Vec<_>, Vec<_>) = paths.into_iter().partition(|p| adapters::is_package(p));
        for p in packages {
            self.offer_install(p);
        }
        let (projects, media): (Vec<_>, Vec<_>) = paths.into_iter().partition(|p| p.extension().is_some_and(|e| e == "splice"));
        if let Some(p) = projects.first()
            && let Ok(proj) = std::fs::read_to_string(p).map_err(|e| e.to_string()).and_then(|t| serde_json::from_str::<Project>(&t).map_err(|e| e.to_string()))
        {
            self.set_project(proj);
            self.file = Some(p.clone());
        }
        if !media.is_empty() {
            self.import(media, true);
        }
    }

    fn update_title(&mut self, ctx: &egui::Context) {
        let name = self.file.as_ref().and_then(|f| f.file_stem()).map(|s| s.to_string_lossy().into_owned());
        let title = match name {
            Some(n) => format!("{n}{} — splice9000", if self.dirty { " •" } else { "" }),
            None => "splice9000".to_owned(),
        };
        if title != self.title {
            ctx.send_viewport_cmd(ViewportCommand::Title(title.clone()));
            self.title = title;
        }
    }
}

impl shell::App for App {
    fn ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        if self.config_changed.swap(false, Ordering::Relaxed) {
            self.load_config(false);
        }
        self.dropped_files(&ctx);
        self.serve_adapters();
        let mut commands = std::mem::take(&mut self.queue);
        commands.extend(self.menu.take());
        // While a question is on screen, keys go to it, not to the edit.
        if self.prompt.is_none() {
            commands.extend(self.keymap.take(&ctx));
        }
        if self.layout_ui.editing && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            self.layout_ui.editing = false;
        }
        for c in commands {
            self.run(&c);
        }
        self.take_probes();
        self.check_export();
        self.update_preview(&ctx);

        // The traffic lights follow the system theme: match it to ours.
        if self.system_dark != Some(self.palette.dark) {
            self.system_dark = Some(self.palette.dark);
            let theme = if self.palette.dark { egui::SystemTheme::Dark } else { egui::SystemTheme::Light };
            ctx.send_viewport_cmd(ViewportCommand::SetTheme(theme));
        }
        let full = ui.max_rect();
        let strip = egui::Rect::from_min_size(full.min, egui::vec2(full.width(), TOP));
        self.top_strip(ui, strip);
        let rect = egui::Rect::from_min_max(egui::pos2(full.left(), strip.bottom()), full.max);
        let mut root = std::mem::replace(&mut self.layout, Node::panel("preview"));
        let mut lui = std::mem::take(&mut self.layout_ui);
        let palette = self.palette;
        self.preview_area = None;
        let headers = self.settings.look.panel_headers;
        let titles: HashMap<String, String> = self
            .adapters
            .all
            .iter()
            .flat_map(|a| a.manifest.panels.iter().map(move |p| (format!("adapter:{}/{}", a.manifest.name, p.id), p.title.clone())))
            .collect();
        let title = |k: &str| titles.get(k).cloned().unwrap_or_else(|| layout::title(k).to_owned());
        let out = lui.show(
            ui,
            &mut root,
            rect,
            &mut layout::Frame { palette: &palette, headers, panel: &mut |kind, part, ui| panels::draw(self, kind, part, ui), title: &title },
        );
        self.layout = root;
        self.layout_ui = lui;
        if out.changed {
            self.save_layout();
        }
        if self.layout_ui.editing {
            panels::edit_banner(ui, rect, &palette);
        }
        let layout = &self.layout;
        self.menu.sync(&|k| layout.contains(k));
        self.show_prompt(&ctx);
        panels::note(ui, rect, self);
        if std::mem::take(&mut self.emit_changed) {
            let summary = serde_json::json!({ "duration": self.timeline.duration(), "clips": self.timeline.len() });
            self.adapters.emit("changed", summary);
        }

        // The preview is decoded at the size it's shown, within limits.
        let ppp = ctx.pixels_per_point();
        if let Some(r) = self.preview_area {
            let max_h = self.settings.preview.max_height.max(90) as f32;
            // Whole pixels, so the picture is drawn as a plain copy.
            let h = (r.height() * ppp).min(max_h).floor().max(16.0);
            let w = (r.width() * ppp).min(max_h * 4.0).floor().max(16.0);
            self.preview_box = (w as u32, h as u32);
        }

        if self.session_dirty && !ctx.input(|i| i.pointer.any_down()) {
            self.save_session();
        }
        self.update_title(&ctx);
        if ctx.input(|i| i.viewport().close_requested()) {
            self.stop();
            self.save_session();
            self.adapters.stop_all();
        }
    }
}

fn open_folder(dir: &Path) {
    let program = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(windows) {
        "explorer"
    } else {
        "xdg-open"
    };
    if let Err(e) = std::process::Command::new(program).arg(dir).spawn() {
        log::error(format!("Couldn't open {}: {e}", dir.display()));
    }
}
