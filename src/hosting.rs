//! What splice9000 does for adapters: answers their requests, asks before
//! installing or first running one, and puts their commands, keys, panels
//! and export formats where people find them.

use std::path::PathBuf;

use serde_json::{Value, json};

use crate::adapters::{self, EDIT_CHANGE, EDIT_READ, Manifest, Message, StartError, command_name, parse_command};
use crate::layout::Side;
use crate::timeline::{Clip, Timeline};
use crate::{App, log, menu};

/// The adapter catalog, opened in the browser. splice9000 itself never goes
/// online.
pub const CATALOG: &str = "https://github.com/solacecasino/splice9000-adapters/blob/main/CATALOG.md";

/// A question on screen, waiting for the user.
pub enum Prompt {
    /// A dropped adapter package: install it?
    Install { path: PathBuf, manifest: Box<Manifest> },
    /// An installed adapter's first run: allow it? Then run `then`.
    Allow { adapter: String, then: Option<String> },
    /// `ui.confirm` from an adapter.
    Confirm { adapter: String, id: Value, title: String, message: String },
    /// `ui.prompt` from an adapter.
    Ask { adapter: String, id: Value, title: String, message: String, text: String },
}

impl App {
    /// Adapters' default keys, for the keymap.
    pub fn adapter_keys(&self) -> Vec<(String, String)> {
        let mut keys = Vec::new();
        for a in &self.adapters.all {
            for c in &a.manifest.commands {
                if let Some(k) = &c.keys {
                    keys.push((k.clone(), command_name("adapter", &a.manifest.name, &c.id)));
                }
            }
        }
        keys
    }

    /// What adapters add to the menus.
    pub fn menu_extras(&self) -> menu::Extras {
        let mut extras = menu::Extras::default();
        for a in &self.adapters.all {
            let name = &a.manifest.name;
            for c in &a.manifest.commands {
                extras.commands.push((c.menu.clone().unwrap_or_default(), command_name("adapter", name, &c.id), c.label.clone()));
            }
        }
        extras.exports = self.adapters.exports().map(|(a, e)| (command_name("export", a, &e.id), e.label.clone())).collect();
        extras
    }

    /// Keys and menus again, after adapters or keys.toml changed.
    pub fn refresh_commands(&mut self) {
        let text = std::fs::read_to_string(self.dir.join("keys.toml")).unwrap_or_default();
        let (keymap, problems) = crate::keys::Keymap::load(&text, &self.adapter_keys());
        problems.into_iter().for_each(log::error);
        self.keymap = keymap;
        self.menu = menu::MenuBar::install(&self.keymap, &self.ctx, crate::icon_rgba(), &self.menu_extras());
        // A chosen export format that's gone falls back to settings.toml's.
        if let Some((a, id)) = &self.export_choice
            && self.adapters.export(a, id).is_none()
        {
            self.export_choice = None;
        }
    }

    /// Commands that act on adapters. Returns false if `command` isn't one.
    pub fn run_adapter_command(&mut self, command: &str) -> bool {
        if let Some((name, id)) = parse_command(command, "adapter") {
            let (name, id) = (name.to_owned(), id.to_owned());
            match self.adapters.run_command(&name, &id) {
                Ok(()) => {}
                Err(StartError::NeedsApproval) => self.ask(Prompt::Allow { adapter: name, then: Some(id) }),
                Err(StartError::Failed(e)) => self.say(e),
            }
        } else if let Some((name, id)) = parse_command(command, "export") {
            self.start_export(Some((name.to_owned(), id.to_owned())));
        } else if let Some(name) = quoted(command, "adapter-remove") {
            match self.adapters.remove(&name) {
                Ok(()) => {
                    self.forget_adapter_panels(&name);
                    self.refresh_commands();
                    self.say(format!("Removed {name}"));
                }
                Err(e) => self.say(format!("Couldn't remove {name}: {e}")),
            }
        } else if let Some(name) = quoted(command, "adapter-add-back") {
            match self.adapters.add_back(&name) {
                Ok(()) => {
                    self.refresh_commands();
                    self.say(format!("Added {name} back"));
                }
                Err(e) => self.say(format!("Couldn't add {name} back: {e}")),
            }
        } else if let Some(name) = quoted(command, "adapter-start") {
            self.start_adapter(&name);
        } else {
            match command {
                "install adapter" => {
                    if let Some(path) = rfd::FileDialog::new().add_filter("Adapter", &["zip"]).set_title("Install Adapter").pick_file() {
                        self.offer_install(path);
                    }
                }
                "install adapter folder" => {
                    if let Some(path) = rfd::FileDialog::new().set_title("Install Adapter from a Folder").pick_folder() {
                        self.offer_install(path);
                    }
                }
                "catalog" => self.ctx.open_url(egui::OpenUrl::new_tab(CATALOG)),
                _ => return false,
            }
        }
        true
    }

    /// Starts an adapter (for its panel), asking first if it must.
    pub fn start_adapter(&mut self, name: &str) {
        let Some(i) = self.adapters.index(name) else { return };
        match self.adapters.start(i) {
            Ok(_) => {}
            Err(StartError::NeedsApproval) => self.ask(Prompt::Allow { adapter: name.to_owned(), then: None }),
            Err(StartError::Failed(e)) => self.say(e),
        }
    }

    /// Shows or hides an adapter's panel, beside the preview like the others.
    pub fn toggle_adapter_panel(&mut self, adapter: &str, id: &str) {
        let kind = format!("adapter:{adapter}/{id}");
        let mut at = Vec::new();
        if self.layout.find(&kind, &mut at) {
            if !at.is_empty() {
                self.layout.remove(&at);
            }
        } else {
            let mut beside = Vec::new();
            if !self.layout.find("preview", &mut beside) {
                beside.clear();
            }
            self.layout.insert(&beside, &kind, Side::Right);
        }
        self.save_layout();
    }

    /// What a stopped adapter showed goes with it.
    fn forget_adapter_panels(&mut self, adapter: &str) {
        let prefix = format!("adapter:{adapter}/");
        self.adapter_panels.retain(|k, _| !k.starts_with(&prefix));
        for l in self.looks.values_mut() {
            if l.waveform.as_ref().is_some_and(|w| w.adapter == adapter) {
                l.waveform = None;
            }
            if l.thumbnails.as_ref().is_some_and(|t| t.adapter == adapter) {
                l.thumbnails = None;
            }
        }
    }

    /// Offers to install a dropped or picked package.
    pub fn offer_install(&mut self, path: PathBuf) {
        match adapters::package_manifest(&path) {
            Ok(manifest) => self.ask(Prompt::Install { path, manifest: Box::new(manifest) }),
            Err(e) => self.say(format!("{}: {e}", path.display())),
        }
    }

    fn ask(&mut self, prompt: Prompt) {
        if self.prompt.is_none() {
            self.prompt = Some(prompt);
        } else {
            self.prompts.push(prompt);
        }
    }

    /// A short message at the bottom of the window, and in the Log.
    pub fn say(&mut self, text: impl Into<String>) {
        let text = text.into();
        log::info(text.clone());
        let now = self.ctx.input(|i| i.time);
        self.note = Some((text, now + 4.0));
        self.ctx.request_repaint_after(std::time::Duration::from_secs_f64(4.1));
    }

    /// Handles what adapters sent since the last frame.
    pub fn serve_adapters(&mut self) {
        while let Ok(message) = self.adapter_rx.try_recv() {
            match message {
                Message::Stopped { adapter, generation } => {
                    if self.adapters.stopped(&adapter, generation) {
                        self.forget_adapter_panels(&adapter);
                    }
                }
                Message::Call { adapter, id, method, params } => {
                    if let Some(result) = self.call(&adapter, id.clone(), &method, params)
                        && let Some(id) = id
                    {
                        self.adapters.reply(&adapter, id, result);
                    }
                }
            }
        }
    }

    /// One request or notification. None: the answer comes later (a prompt).
    fn call(&mut self, adapter: &str, id: Option<Value>, method: &str, params: Value) -> Option<Result<Value, String>> {
        let Some(i) = self.adapters.index(adapter) else { return Some(Err("unknown adapter".into())) };
        let may = |p: &str| self.adapters.all[i].has(p);
        let need = |p: &str| if may(p) { Ok(()) } else { Err(format!("{method} needs the \"{p}\" permission in adapter.toml")) };
        let text = |key: &str| params.get(key).and_then(Value::as_str).unwrap_or_default().to_owned();
        Some(match method {
            "log" => {
                log::info(format!("{adapter}: {}", text("message")));
                Ok(Value::Null)
            }
            "notify" => {
                self.say(format!("{adapter}: {}", text("text")));
                Ok(Value::Null)
            }
            "panel.set" => {
                let lines = params.get("lines").and_then(Value::as_array).cloned().unwrap_or_default();
                self.adapter_panels.insert(format!("adapter:{adapter}/{}", text("id")), lines);
                self.ctx.request_repaint();
                Ok(Value::Null)
            }
            "edit.get" => need(EDIT_READ).map(|()| self.edit_json()),
            "edit.set" => need(EDIT_CHANGE).and_then(|()| self.set_edit(&params)),
            "edit.import" => need(EDIT_CHANGE).map(|()| {
                let paths = params.get("paths").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).map(PathBuf::from).collect();
                let append = params.get("append").and_then(Value::as_bool).unwrap_or(false);
                self.import(paths, append);
                Value::Null
            }),
            "playhead.set" => need(EDIT_CHANGE).and_then(|()| {
                let t = params.get("time").and_then(Value::as_f64).ok_or("needs `time` in seconds")?;
                self.seek(t);
                Ok(json!(self.playhead))
            }),
            "playback.set" => match params.get("rate").and_then(Value::as_f64) {
                Some(rate) => {
                    self.set_rate(rate);
                    Ok(json!(self.rate()))
                }
                None => Err("needs `rate`".into()),
            },
            "media.waveform" => self.set_waveform(adapter, &params).map(|()| Value::Null),
            "media.thumbnails" => self.set_thumbnails(adapter, &params).map(|()| Value::Null),
            "ui.confirm" | "ui.prompt" => {
                let Some(id) = id else { return Some(Err("ui prompts are requests: give them an id".into())) };
                let (adapter, title, message) = (adapter.to_owned(), text("title"), text("message"));
                self.ask(if method == "ui.confirm" {
                    Prompt::Confirm { adapter, id, title, message }
                } else {
                    Prompt::Ask { adapter, id, title, message, text: text("default") }
                });
                return None;
            }
            "ui.save_dialog" => {
                let mut d = rfd::FileDialog::new().set_title(text("title")).set_file_name(text("file_name"));
                let ext = text("extension");
                if !ext.is_empty() {
                    d = d.add_filter(&ext, &[ext.as_str()]);
                }
                Ok(d.save_file().map_or(Value::Null, |p| json!(p)))
            }
            "ui.open_dialog" => {
                let mut d = rfd::FileDialog::new().set_title(text("title"));
                let exts: Vec<String> = params.get("extensions").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).map(String::from).collect();
                if !exts.is_empty() {
                    d = d.add_filter("Files", &exts);
                }
                Ok(d.pick_file().map_or(Value::Null, |p| json!(p)))
            }
            other => Err(format!("splice9000 doesn't know \"{other}\"")),
        })
    }

    /// `media.waveform`: a file's sound, drawn on its clips.
    fn set_waveform(&mut self, adapter: &str, params: &Value) -> Result<(), String> {
        let path = PathBuf::from(params.get("path").and_then(Value::as_str).ok_or("needs `path`")?);
        let per_second = params.get("per_second").and_then(Value::as_f64).filter(|r| *r > 0.0).ok_or("needs `per_second`")?;
        let peaks = params.get("peaks").and_then(Value::as_array).ok_or("needs `peaks`")?;
        let peaks = peaks.iter().map(|v| v.as_u64().unwrap_or(0).min(255) as u8).collect();
        self.looks.entry(path).or_default().waveform = Some(crate::Waveform { adapter: adapter.to_owned(), per_second, peaks });
        self.ctx.request_repaint();
        Ok(())
    }

    /// `media.thumbnails`: a grid of pictures from one file, read once.
    fn set_thumbnails(&mut self, adapter: &str, params: &Value) -> Result<(), String> {
        let num = |k: &str| params.get(k).and_then(Value::as_f64).filter(|x| *x > 0.0).ok_or(format!("needs `{k}`"));
        let (every, w, h, columns, count) = (num("every")?, num("width")? as usize, num("height")? as usize, num("columns")? as usize, num("count")? as usize);
        let image = params.get("image").and_then(Value::as_str).ok_or("needs `image`, a PNG file")?;
        let file = std::fs::File::open(image).map_err(|e| format!("{image}: {e}"))?;
        let mut decoder = png::Decoder::new(std::io::BufReader::new(file));
        decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
        let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
        let mut buf = vec![0; reader.output_buffer_size().ok_or("picture too big")?];
        let info = reader.next_frame(&mut buf).map_err(|e| e.to_string())?;
        let (iw, ih) = (info.width as usize, info.height as usize);
        let rows = count.div_ceil(columns);
        if iw < columns * w || ih < rows * h || iw * ih > 64 << 20 {
            return Err(format!("{image} is {iw}x{ih}, not {columns}x{rows} pictures of {w}x{h}"));
        }
        let pixels = match info.color_type {
            png::ColorType::Rgb => buf[..iw * ih * 3].as_chunks::<3>().0.iter().map(|p| egui::Color32::from_rgb(p[0], p[1], p[2])).collect(),
            png::ColorType::Rgba => buf[..iw * ih * 4].as_chunks::<4>().0.iter().map(|p| egui::Color32::from_rgba_unmultiplied(p[0], p[1], p[2], p[3])).collect(),
            other => return Err(format!("{image}: {other:?} pictures aren't read; send RGB")),
        };
        let texture = self.ctx.load_texture(format!("thumbs {image}"), egui::ColorImage::new([iw, ih], pixels), egui::TextureOptions::LINEAR);
        let path = PathBuf::from(params.get("path").and_then(Value::as_str).unwrap_or_default());
        let size = [iw as f32, ih as f32];
        self.looks.entry(path).or_default().thumbnails = Some(crate::Thumbnails { adapter: adapter.to_owned(), every, w, h, columns, count, size, texture });
        self.ctx.request_repaint();
        Ok(())
    }

    /// The edit as adapters see it.
    pub fn edit_json(&self) -> Value {
        let f = self.format();
        json!({
            "format": { "width": f.width, "height": f.height, "fps": f.fps },
            "duration": self.timeline.duration(),
            "playhead": self.playhead,
            "rate": self.rate(),
            "selected": self.selected_clip,
            "file": self.file,
            "media": self.media,
            "clips": self.timeline.clips().iter().enumerate().map(|(i, c)| json!({
                "media": c.media, "src_in": c.src_in, "src_out": c.src_out, "start": self.timeline.start(i), "fade": c.fade,
            })).collect::<Vec<_>>(),
        })
    }

    /// `edit.set`: the clips replaced in one undo step. Each clip is checked
    /// against its media, so an adapter can't make an edit splice9000
    /// couldn't have made.
    fn set_edit(&mut self, params: &Value) -> Result<Value, String> {
        let list = params.get("clips").and_then(Value::as_array).ok_or("needs `clips`")?;
        let mut clips = Vec::with_capacity(list.len());
        for (n, c) in list.iter().enumerate() {
            let media = c.get("media").and_then(Value::as_u64).map(|m| m as usize).filter(|&m| m < self.media.len());
            let media = media.ok_or(format!("clip {n}: `media` must be an index into the media list"))?;
            let duration = self.media[media].duration;
            let src_in = c.get("src_in").and_then(Value::as_f64).unwrap_or(0.0).clamp(0.0, duration);
            let src_out = c.get("src_out").and_then(Value::as_f64).unwrap_or(duration).clamp(0.0, duration);
            if src_out <= src_in {
                return Err(format!("clip {n}: `src_out` must be after `src_in`"));
            }
            // Protocol 3. Left out: none, as older adapters expect.
            let fade = c.get("fade").and_then(Value::as_f64).unwrap_or(0.0).clamp(0.0, 10.0);
            clips.push(Clip { media, src_in, src_out, fade });
        }
        self.snapshot();
        self.timeline = Timeline::new(clips);
        self.selected_clip = None;
        self.playhead = self.playhead.min(self.timeline.duration());
        self.requested = None;
        Ok(Value::Null)
    }

    /// Draws the prompt waiting for an answer, if any. Escape declines and
    /// Return does nothing: saying yes takes a click (or Tab, then Space).
    pub fn show_prompt(&mut self, ctx: &egui::Context) {
        let Some(prompt) = self.prompt.take() else { return };
        let pal = self.palette;
        let mut answer: Option<bool> = None;
        let mut typed = None;
        let modal = egui::Modal::new(egui::Id::unique("adapter prompt")).show(ctx, |ui| {
            ui.set_width(380.0);
            let (title, lines, yes): (String, Vec<String>, &str) = match &prompt {
                Prompt::Install { manifest, .. } => ("Install this adapter?".into(), adapters::summary(manifest), "Install"),
                Prompt::Allow { adapter, .. } => {
                    let lines = self.adapters.index(adapter).map(|i| adapters::summary(&self.adapters.all[i].manifest)).unwrap_or_default();
                    (format!("Allow {adapter} to run?"), lines, "Allow")
                }
                Prompt::Confirm { adapter, title, message, .. } => (format!("{adapter}: {title}"), vec![message.clone()], "OK"),
                Prompt::Ask { adapter, title, message, .. } => (format!("{adapter}: {title}"), vec![message.clone()], "OK"),
            };
            ui.label(egui::RichText::new(title).size(15.0).strong());
            ui.add_space(6.0);
            for line in lines {
                ui.label(line);
            }
            if matches!(prompt, Prompt::Install { .. } | Prompt::Allow { .. }) {
                ui.add_space(6.0);
                ui.label(egui::RichText::new("Adapters are separate programs that run as you. Only allow ones you trust.").small().color(pal.weak));
            }
            if let Prompt::Ask { text, .. } = &prompt {
                let mut t = text.clone();
                ui.text_edit_singleline(&mut t);
                typed = Some(t);
            }
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                let yes = egui::Button::new(egui::RichText::new(yes).color(pal.on_accent)).fill(pal.accent);
                if ui.add(yes).clicked() {
                    answer = Some(true);
                }
                if ui.button("Cancel").clicked() {
                    answer = Some(false);
                }
            });
        });
        let mut prompt = prompt;
        if let (Prompt::Ask { text, .. }, Some(t)) = (&mut prompt, typed) {
            *text = t;
        }
        if answer.is_none() && (modal.should_close() || ctx.input(|i| i.key_pressed(egui::Key::Escape))) {
            answer = Some(false);
        }
        let Some(yes) = answer else {
            self.prompt = Some(prompt);
            return;
        };
        self.answer(prompt, yes);
        if self.prompt.is_none() && !self.prompts.is_empty() {
            self.prompt = Some(self.prompts.remove(0));
        }
    }

    fn answer(&mut self, prompt: Prompt, yes: bool) {
        match prompt {
            Prompt::Install { path, manifest } if yes => match adapters::install(&path, &self.dir) {
                Ok(m) => {
                    // The prompt showed what it may do: that's its approval.
                    adapters::remember_approval(&self.dir, &m);
                    self.adapters.reload();
                    self.refresh_commands();
                    self.adapters.start_launch_adapters();
                    self.say(format!("Installed {} {}", m.name, m.version));
                    let _ = manifest;
                }
                Err(e) => self.say(e),
            },
            Prompt::Allow { adapter, then } if yes => {
                if let Some(i) = self.adapters.index(&adapter) {
                    self.adapters.allow(i);
                    match then {
                        Some(id) => self.run(&command_name("adapter", &adapter, &id)),
                        None => self.start_adapter(&adapter),
                    }
                }
            }
            Prompt::Confirm { adapter, id, .. } => self.adapters.reply(&adapter, id, Ok(json!(yes))),
            Prompt::Ask { adapter, id, text, .. } => self.adapters.reply(&adapter, id, Ok(if yes { json!(text) } else { Value::Null })),
            _ => {}
        }
    }
}

/// `adapter-remove "Name"` and the like, from buttons.
fn quoted(command: &str, verb: &str) -> Option<String> {
    let rest = command.strip_prefix(verb)?.strip_prefix(" \"")?;
    Some(rest.strip_suffix('"')?.to_owned())
}

#[cfg(test)]
mod tests {
    #[test]
    fn reads_button_commands() {
        assert_eq!(super::quoted(r#"adapter-remove "Frame Grab""#, "adapter-remove").as_deref(), Some("Frame Grab"));
        assert_eq!(super::quoted(r#"adapter-remove Frame"#, "adapter-remove"), None);
    }
}
