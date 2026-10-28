//! What each panel shows. Panels only read the app and queue commands or
//! make small edits; any panel can be placed anywhere, or twice.

use egui::{Align, Align2, Color32, CursorIcon, FontId, Id, Layout, Pos2, Rect, RichText, Sense, Stroke, StrokeKind, Ui, UiBuilder, pos2, vec2};

use crate::layout::Part;
use crate::{App, keys, log};

/// What dragging on the timeline is doing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TimelineDrag {
    None,
    Scrub,
    /// Trimming a clip's start (`left`) or end; `from` is the source time
    /// when the drag began, `x0` the pointer's x then.
    Trim { clip: usize, left: bool, from: f64, x0: f32 },
    Move { clip: usize },
}

/// A media file dragged from the Media panel.
struct MediaDrag(usize);

pub fn draw(app: &mut App, kind: &str, part: Part, ui: &mut Ui) {
    if part == Part::Header {
        header(app, kind, ui);
        return;
    }
    // The picture and the timeline run edge to edge; the rest get a margin.
    let edge = matches!(kind, "preview" | "timeline");
    let rect = if edge { ui.max_rect() } else { ui.max_rect().shrink2(vec2(12.0, 8.0)) };
    let mut ui = ui.new_child(UiBuilder::new().max_rect(rect));
    let ui = &mut ui;
    match kind {
        "preview" => preview(app, ui),
        "timeline" => timeline(app, ui),
        "media" => media(app, ui),
        "transport" => transport(app, ui),
        "clip" => clip(app, ui),
        "export" => export(app, ui),
        "log" => log_panel(ui),
        "help" => help(app, ui),
        "adapters" => adapters_panel(app, ui),
        k if k.starts_with("adapter:") => adapter_panel(app, k, ui),
        _ => {}
    }
}

/// Small controls at the right end of a panel's header.
fn header(app: &mut App, kind: &str, ui: &mut Ui) {
    let small = |ui: &mut Ui, text: &str, tip: &str| {
        ui.add(egui::Button::new(RichText::new(text).size(11.0)).frame(false).min_size(vec2(18.0, 18.0))).on_hover_text(tip)
    };
    match kind {
        "media" => {
            if small(ui, "+ Import", "Add media files").clicked() {
                app.queue.push("import".into());
            }
        }
        "adapters" => {
            if small(ui, "Catalog", "Find adapters in the catalog (opens your browser)").clicked() {
                app.queue.push("catalog".into());
            }
            if small(ui, "+ Install", "Install an adapter from its zip (or drop it on the window)").clicked() {
                app.queue.push("install adapter".into());
            }
        }
        "timeline" => {
            for (text, cmd, tip) in [("Fit", "zoom fit", "Fit the edit"), ("+", "zoom in", "Zoom in"), ("−", "zoom out", "Zoom out")] {
                if small(ui, text, tip).clicked() {
                    app.queue.push(cmd.into());
                }
            }
        }
        "preview" => {
            let text = if app.muted { "Muted" } else { "Sound" };
            if small(ui, text, "Turn the sound off or on (m)").clicked() {
                app.queue.push("mute".into());
            }
            let f = app.format();
            ui.label(RichText::new(format!("{}×{}", f.width, f.height)).size(10.5).color(app.palette.weak));
        }
        _ => {}
    }
}

/// "1:02:03:04" (h:m:s:frames), without hours when there are none.
pub fn timecode(t: f64, fps: f64) -> String {
    let frames = (t * fps + 1e-6).floor() as i64;
    let per_sec = fps.round().max(1.0) as i64;
    let secs = (t + 1e-6).floor() as i64;
    let f = frames - (secs as f64 * fps).round() as i64;
    let (h, m, s) = (secs / 3600, secs / 60 % 60, secs % 60);
    let f = f.clamp(0, per_sec - 1);
    if h > 0 { format!("{h}:{m:02}:{s:02}:{f:02}") } else { format!("{m:02}:{s:02}:{f:02}") }
}

fn short_time(t: f64) -> String {
    let s = t.round() as i64;
    if s >= 3600 { format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60) } else { format!("{}:{:02}", s / 60, s % 60) }
}

fn preview(app: &mut App, ui: &mut Ui) {
    let rect = ui.max_rect();
    app.preview_area = Some(rect);
    ui.painter().rect_filled(rect, 0.0, app.palette.well);
    let weak = app.palette.weak;
    if app.tools.is_none() {
        ui.painter().text(rect.center(), Align2::CENTER_CENTER, "ffmpeg wasn't found — see the Log", FontId::proportional(14.0), weak);
        return;
    }
    match &app.texture {
        Some(tex) if !app.timeline.is_empty() => {
            // At its own size on whole pixels when it fits, which draws it
            // as a plain copy; scaled to fit otherwise.
            let ppp = ui.ctx().pixels_per_point();
            let px = tex.size_vec2();
            let shown = if px.x <= rect.width() * ppp + 0.5 && px.y <= rect.height() * ppp + 0.5 {
                let min = ((rect.center().to_vec2() * ppp - px / 2.0).round()) / ppp;
                Rect::from_min_size(min.to_pos2(), px / ppp)
            } else {
                let scale = (rect.width() / px.x).min(rect.height() / px.y);
                Rect::from_center_size(rect.center(), px * scale)
            };
            ui.painter().image(tex.id(), shown, Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)), Color32::WHITE);
        }
        _ if app.timeline.is_empty() => {
            ui.painter().text(rect.center(), Align2::CENTER_CENTER, "Drop video files anywhere to start", FontId::proportional(15.0), weak);
        }
        _ => {}
    }
    // Clicking the picture plays and pauses.
    if ui.interact(rect, ui.scope_id().with("preview click"), Sense::click()).clicked() {
        app.queue.push("play".into());
    }
}

fn transport(app: &mut App, ui: &mut Ui) {
    let fps = app.format().fps;
    let pal = app.palette;
    ui.horizontal_centered(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        for (icon, cmd, tip) in [
            (Icon::Start, "start", "Start"),
            (Icon::Back, "frame previous", "One frame back"),
            (if app.playing { Icon::Pause } else { Icon::Play }, "play", "Play or pause"),
            (Icon::Forward, "frame next", "One frame forward"),
            (Icon::End, "end", "End"),
        ] {
            let main = matches!(icon, Icon::Play | Icon::Pause);
            if icon_button(ui, icon, main, &pal).on_hover_text(tip).clicked() {
                app.queue.push(cmd.into());
            }
        }
        ui.add_space(14.0);
        ui.label(RichText::new(timecode(app.playhead, fps)).monospace().size(15.0).color(pal.text));
        ui.label(RichText::new(format!(" / {}", timecode(app.timeline.duration(), fps))).monospace().size(12.0).color(pal.weak));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            for (label, cmd, tip) in [("Delete", "delete", "Remove the selected clip"), ("Split", "split", "Cut at the playhead (s)")] {
                if ui.button(label).on_hover_text(tip).clicked() {
                    app.queue.push(cmd.into());
                }
            }
        });
    });
}

fn media(app: &mut App, ui: &mut Ui) {
    if app.media.is_empty() {
        ui.add_space(8.0);
        ui.weak("Drop video files here, or Import.");
        return;
    }
    egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        for i in 0..app.media.len() {
            let m = &app.media[i];
            let details = format!(
                "{}×{} · {} fps · {}{}",
                m.width,
                m.height,
                (m.fps * 1000.0).round() / 1000.0,
                short_time(m.duration),
                if m.audio { " · audio" } else { "" }
            );
            let name = m.name();
            let selected = app.selected_media == Some(i);
            let r = ui
                .dnd_drag_source(Id::unique(("media", i)), MediaDrag(i), |ui| media_row(ui, &name, &details, selected))
                .inner;
            if r.clicked() {
                app.selected_media = Some(i);
            }
            if r.double_clicked() {
                app.selected_media = Some(i);
                app.queue.push("append".into());
            }
            r.on_hover_text("Double-click or drag to the timeline to use it").context_menu(|ui| {
                if ui.button("Add to the end").clicked() {
                    app.selected_media = Some(i);
                    app.queue.push("append".into());
                    ui.close();
                }
            });
        }
    });
}

fn media_row(ui: &mut Ui, name: &str, details: &str, selected: bool) -> egui::Response {
    let (rect, r) = ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::click());
    let v = ui.visuals();
    if selected {
        ui.painter().rect_filled(rect, 4.0, v.selection.bg_fill);
        ui.painter().rect_filled(Rect::from_min_size(rect.min + vec2(0.0, 8.0), vec2(2.0, rect.height() - 16.0)), 1.0, v.selection.stroke.color);
    } else if r.hovered() {
        ui.painter().rect_filled(rect, 4.0, v.widgets.hovered.weak_bg_fill);
    }
    let p = ui.painter().with_clip_rect(rect.shrink(2.0).intersect(ui.clip_rect()));
    p.text(rect.left_top() + vec2(10.0, 5.0), Align2::LEFT_TOP, name, FontId::proportional(13.0), v.strong_text_color());
    p.text(rect.left_top() + vec2(10.0, 22.0), Align2::LEFT_TOP, details, FontId::proportional(11.0), v.weak_text_color());
    r
}

#[derive(Clone, Copy)]
enum Icon {
    Start,
    Back,
    Play,
    Pause,
    Forward,
    End,
}

/// A flat button with a drawn icon: no font needs to have the symbol. The
/// `main` one (play) is a solid circle in the signal color.
fn icon_button(ui: &mut Ui, icon: Icon, main: bool, pal: &crate::theme::Palette) -> egui::Response {
    let size = if main { vec2(34.0, 34.0) } else { vec2(28.0, 28.0) };
    let (rect, r) = ui.allocate_exact_size(size, Sense::click());
    let c = rect.center();
    let p = ui.painter();
    let color = if main {
        let fill = if r.hovered() { pal.accent.gamma_multiply(0.85) } else { pal.accent };
        p.circle_filled(c, 16.0, fill);
        pal.on_accent
    } else {
        if r.hovered() {
            p.rect_filled(rect, 5.0, ui.visuals().widgets.hovered.weak_bg_fill);
        }
        if r.hovered() { pal.text } else { pal.weak }
    };
    let tri = |dir: f32, x: f32, w: f32, h: f32| {
        let pts = vec![pos2(x - dir * w / 2.0, c.y - h / 2.0), pos2(x + dir * w / 2.0, c.y), pos2(x - dir * w / 2.0, c.y + h / 2.0)];
        egui::Shape::convex_polygon(pts, color, Stroke::NONE)
    };
    let bar = |x: f32, h: f32| egui::Shape::rect_filled(Rect::from_center_size(pos2(x, c.y), vec2(2.0, h)), 0.0, color);
    match icon {
        Icon::Play => {
            p.add(tri(1.0, c.x + 1.5, 10.0, 12.0));
        }
        Icon::Pause => {
            p.add(egui::Shape::rect_filled(Rect::from_center_size(pos2(c.x - 2.5, c.y), vec2(3.0, 11.0)), 0.0, color));
            p.add(egui::Shape::rect_filled(Rect::from_center_size(pos2(c.x + 2.5, c.y), vec2(3.0, 11.0)), 0.0, color));
        }
        Icon::Back => {
            p.add(tri(-1.0, c.x + 1.0, 6.0, 9.0));
            p.add(bar(c.x - 3.5, 9.0));
        }
        Icon::Forward => {
            p.add(tri(1.0, c.x - 1.0, 6.0, 9.0));
            p.add(bar(c.x + 3.5, 9.0));
        }
        Icon::Start => {
            p.add(bar(c.x - 5.0, 10.0));
            p.add(tri(-1.0, c.x - 0.5, 6.0, 10.0));
            p.add(tri(-1.0, c.x + 4.5, 6.0, 10.0));
        }
        Icon::End => {
            p.add(tri(1.0, c.x - 4.5, 6.0, 10.0));
            p.add(tri(1.0, c.x + 0.5, 6.0, 10.0));
            p.add(bar(c.x + 5.0, 10.0));
        }
    }
    r
}

fn clip(app: &mut App, ui: &mut Ui) {
    let Some(i) = app.selected_clip.filter(|&i| i < app.timeline.len()) else {
        ui.weak("Select a clip on the timeline.");
        return;
    };
    let c = app.timeline.clips()[i].clone();
    let m = app.media[c.media].clone();
    let step = 1.0 / m.fps;
    ui.label(RichText::new(m.name()).strong());
    ui.weak(m.path.parent().map(|p| p.display().to_string()).unwrap_or_default());
    ui.add_space(4.0);
    let fmt = move |v: f64, _: std::ops::RangeInclusive<usize>| timecode(v, m.fps);
    egui::Grid::new("clip grid").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
        ui.label("In");
        let mut v = c.src_in;
        let r = ui.add(egui::DragValue::new(&mut v).speed(step).range(0.0..=c.src_out - step).custom_formatter(fmt));
        edit(app, &r, v != c.src_in, |a| a.timeline.set_in(i, v, step));
        ui.end_row();
        ui.label("Out");
        let mut v = c.src_out;
        let r = ui.add(egui::DragValue::new(&mut v).speed(step).range(c.src_in + step..=m.duration).custom_formatter(fmt));
        edit(app, &r, v != c.src_out, |a| a.timeline.set_out(i, v, m.duration, step));
        ui.end_row();
        ui.label("Length");
        ui.monospace(timecode(c.len(), m.fps));
        ui.end_row();
        ui.label("Starts at");
        ui.monospace(timecode(app.timeline.start(i), app.format().fps));
        ui.end_row();
    });
}

/// Applies an edit from a field, with one undo step per drag or typed value.
fn edit(app: &mut App, r: &egui::Response, changed: bool, f: impl FnOnce(&mut App)) {
    if r.drag_started() || (changed && !r.dragged()) {
        app.snapshot();
    }
    if changed {
        f(app);
        app.edited();
    }
}

fn export(app: &mut App, ui: &mut Ui) {
    // The format: settings.toml's, or one an adapter adds.
    let presets: Vec<(String, String, String)> = app.adapters.exports().map(|(a, e)| (a.to_owned(), e.id.clone(), e.label.clone())).collect();
    let chosen = app.export_choice.as_ref().and_then(|(a, id)| presets.iter().find(|p| &p.0 == a && &p.1 == id)).map(|p| p.2.clone());
    let mut choice = app.export_choice.clone();
    egui::ComboBox::from_id_salt("export format")
        .selected_text(chosen.unwrap_or_else(|| "Settings".into()))
        .width(ui.available_width().min(220.0))
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut choice, None, "Settings").on_hover_text("The format in settings.toml");
            for (a, id, label) in &presets {
                ui.selectable_value(&mut choice, Some((a.clone(), id.clone())), label).on_hover_text(format!("From {a}"));
            }
        });
    app.export_choice = choice;
    let spec = app.export_choice.as_ref().and_then(|(a, id)| app.adapters.export(a, id)).cloned();
    let f = app.format().with(spec.as_ref());
    let s = &app.settings.export;
    let line = match &spec {
        Some(p) if p.no_video => format!("Sound only · .{}", p.extension),
        Some(p) => format!("{}×{} · {} fps · .{}", f.width, f.height, (f.fps * 1000.0).round() / 1000.0, p.extension),
        None => {
            let quality = if s.video_codec.starts_with("libx26") { format!(" · CRF {} {}", s.quality, s.preset) } else { String::new() };
            format!("{}×{} · {} fps\n{}{quality}", f.width, f.height, (f.fps * 1000.0).round() / 1000.0, s.video_codec)
        }
    };
    ui.label(line);
    if spec.is_none() && ui.link("Change in settings.toml").clicked() {
        app.queue.push("config".into());
    }
    let missing = app.missing_encoders(spec.as_ref());
    if !missing.is_empty() {
        ui.colored_label(ui.visuals().error_fg_color, format!("Needs ffmpeg's {} encoder, which yours doesn't have.", missing.join(" and ")));
    }
    ui.add_space(6.0);
    if let Some(job) = &app.export {
        let done = job.progress.lock().unwrap_or_else(|e| e.into_inner()).done;
        let frac = (done / job.total.max(0.001)).clamp(0.0, 1.0) as f32;
        let secs = job.started.elapsed().as_secs_f64();
        let eta = if frac > 0.02 { format!(" · {} left", short_time(secs / frac as f64 - secs)) } else { String::new() };
        ui.add(egui::ProgressBar::new(frac).text(format!("{:.0}%{eta}", frac * 100.0)));
        if ui.button("Cancel").clicked() {
            app.cancel_export();
        }
    } else {
        let can = !app.timeline.is_empty() && app.tools.is_some() && missing.is_empty();
        let button = egui::Button::new(RichText::new("Export Video…").color(app.palette.on_accent)).fill(app.palette.accent).min_size(vec2(140.0, 28.0));
        if ui.add_enabled(can, button).clicked() {
            app.queue.push("export".into());
        }
        match &app.last_export {
            Some(Ok(s)) => {
                ui.weak(format!("Done: {s}"));
            }
            Some(Err(e)) => {
                ui.colored_label(ui.visuals().error_fg_color, e);
            }
            None => {}
        }
    }
}

fn log_panel(ui: &mut Ui) {
    egui::ScrollArea::vertical().auto_shrink(false).stick_to_bottom(true).show(ui, |ui| {
        let err = ui.visuals().error_fg_color;
        log::each(|is_err, text| {
            if is_err {
                ui.colored_label(err, text);
            } else {
                ui.weak(text);
            }
        });
    });
}

fn help(app: &mut App, ui: &mut Ui) {
    ui.weak("Click to run. Keys are set in keys.toml.");
    egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        egui::Grid::new("help").num_columns(2).striped(true).show(ui, |ui| {
            for (name, _, what) in keys::COMMANDS {
                if ui.add(egui::Button::new(crate::menu::label(name)).frame(false)).on_hover_text(format!("{what}  ·  \"{name}\" in keys.toml")).clicked() {
                    app.queue.push((*name).into());
                }
                let key = app.keymap.key_for(name).map(|k| crate::menu::shortcut_text(&k)).unwrap_or_default();
                ui.label(RichText::new(key).monospace().color(app.palette.weak));
                ui.end_row();
            }
        });
    });
}

/// The splice9000 mark at `left_center`, `w` wide: a light strip and a
/// signal-colored one joined at a slanted cut.
pub fn mark(p: &egui::Painter, left_center: Pos2, w: f32, pal: &crate::theme::Palette) {
    let (h, gap, slant) = (w * 0.3, w * 0.06, w * 0.08);
    let (x0, x1, mid) = (left_center.x, left_center.x + w, left_center.x + w / 2.0);
    let (y0, y1) = (left_center.y - h / 2.0, left_center.y + h / 2.0);
    let light = if pal.dark { Color32::from_rgb(0xe8, 0xe9, 0xeb) } else { Color32::from_rgb(0x2a, 0x2c, 0x31) };
    p.add(egui::Shape::convex_polygon(
        vec![pos2(x0, y0), pos2(mid - gap / 2.0 + slant, y0), pos2(mid - gap / 2.0 - slant, y1), pos2(x0, y1)],
        light,
        Stroke::NONE,
    ));
    p.add(egui::Shape::convex_polygon(
        vec![pos2(mid + gap / 2.0 + slant, y0), pos2(x1, y0), pos2(x1, y1), pos2(mid + gap / 2.0 - slant, y1)],
        pal.accent,
        Stroke::NONE,
    ));
}

/// Every adapter: what it adds, what it may do, and Remove; then built-in
/// ones that were removed, with Add Back.
fn adapters_panel(app: &mut App, ui: &mut Ui) {
    let pal = app.palette;
    egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        // The way to more: the catalog, in the browser.
        let browse = egui::Button::new(RichText::new("Browse the adapter catalog").color(pal.on_accent)).fill(pal.accent);
        if ui.add(browse).on_hover_text("Opens in your browser. Download a zip, then drop it on this window.").clicked() {
            app.queue.push("catalog".into());
        }
        ui.label(RichText::new("Download a zip, then drop it on this window.").small().color(pal.weak));
        ui.add_space(8.0);
        if app.adapters.all.is_empty() {
            ui.weak("No adapters yet.");
        }
        let mut toggles = Vec::new();
        for (i, a) in app.adapters.all.iter().enumerate() {
            let m = &a.manifest;
            if i > 0 {
                ui.separator();
            }
            ui.horizontal(|ui| {
                ui.label(RichText::new(&m.name).strong().color(pal.text));
                ui.label(RichText::new(&m.version).small().color(pal.weak));
                if a.builtin {
                    ui.label(RichText::new("BUILT IN").size(9.5).color(pal.accent));
                }
                if a.is_running() {
                    ui.label(RichText::new("running").size(9.5).color(pal.weak));
                }
            });
            if !m.description.is_empty() {
                ui.label(RichText::new(&m.description).color(pal.weak));
            }
            let can = if m.run.is_empty() {
                if m.exports.is_empty() { "No program: nothing runs" } else { "Presets only: nothing runs" }.to_owned()
            } else if m.permissions.is_empty() {
                "No permissions".to_owned()
            } else {
                format!("Can {}", m.permissions.iter().map(|p| crate::adapters::describe(p)).collect::<Vec<_>>().join("; "))
            };
            ui.label(RichText::new(can).small().color(pal.weak));
            if !m.exports.is_empty() {
                let list: Vec<&str> = m.exports.iter().map(|e| e.label.as_str()).collect();
                ui.label(RichText::new(format!("Export formats: {}", list.join(", "))).small().color(pal.weak));
            }
            ui.horizontal_wrapped(|ui| {
                for c in &m.commands {
                    if ui.small_button(&c.label).clicked() {
                        app.queue.push(crate::adapters::command_name("adapter", &m.name, &c.id));
                    }
                }
                for p in &m.panels {
                    let kind = format!("adapter:{}/{}", m.name, p.id);
                    if ui.add(egui::Button::selectable(app.layout.contains(&kind), RichText::new(format!("{} panel", p.title)).small())).clicked() {
                        toggles.push((m.name.clone(), p.id.clone()));
                    }
                }
                let tip = if a.builtin { "Take it out of splice9000; Add Back brings it again" } else { "Move it to the Trash" };
                if ui.small_button("Remove").on_hover_text(tip).clicked() {
                    app.queue.push(format!("adapter-remove \"{}\"", m.name));
                }
                if !m.homepage.is_empty() && ui.small_button("Homepage").clicked() {
                    ui.ctx().open_url(egui::OpenUrl::new_tab(&m.homepage));
                }
            });
        }
        for (a, id) in toggles {
            app.toggle_adapter_panel(&a, &id);
        }
        let removed = app.adapters.removed_builtins();
        if !removed.is_empty() {
            ui.add_space(10.0);
            ui.label(RichText::new("REMOVED").size(10.0).color(pal.weak));
            for m in removed {
                ui.horizontal(|ui| {
                    ui.label(&m.name);
                    if ui.small_button("Add Back").clicked() {
                        app.queue.push(format!("adapter-add-back \"{}\"", m.name));
                    }
                });
            }
        }
        ui.add_space(10.0);
        if ui.link("Install from a folder instead…").clicked() {
            app.queue.push("install adapter folder".into());
        }
    });
}

/// A panel an adapter fills with `panel.set`: lines of text, or buttons
/// that run its commands.
fn adapter_panel(app: &mut App, kind: &str, ui: &mut Ui) {
    let Some((name, _)) = crate::layout::adapter_panel(kind) else { return };
    let Some(i) = app.adapters.index(name) else {
        ui.weak(format!("{name} isn't installed."));
        return;
    };
    if !app.adapters.all[i].is_running() {
        if app.adapters.approved(i) {
            // Started once when its panel shows; after a crash, by hand.
            let tried = ui.ctx().data_mut(|d| std::mem::replace(d.get_temp_mut_or_default::<bool>(Id::unique(("started", kind))), true));
            if !tried || ui.button(format!("Start {name}")).clicked() {
                app.queue.push(format!("adapter-start \"{name}\""));
            }
        } else if ui.button(format!("Allow {name}…")).clicked() {
            app.queue.push(format!("adapter-start \"{name}\""));
        }
    }
    let lines = app.adapter_panels.get(kind).cloned().unwrap_or_default();
    egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        for line in &lines {
            match line {
                serde_json::Value::String(t) => {
                    ui.label(t);
                }
                serde_json::Value::Object(o) => {
                    let text = o.get("text").and_then(|t| t.as_str()).unwrap_or_default();
                    match o.get("command").and_then(|c| c.as_str()) {
                        Some(c) => {
                            if ui.button(text).clicked() {
                                app.queue.push(crate::adapters::command_name("adapter", name, c));
                            }
                        }
                        None => {
                            ui.label(text);
                        }
                    }
                }
                _ => {}
            }
        }
    });
}

/// The short message adapters and installs leave, at the bottom.
pub fn note(ui: &Ui, rect: Rect, app: &mut App) {
    let now = ui.ctx().input(|i| i.time);
    let Some((text, until)) = &app.note else { return };
    if now > *until {
        app.note = None;
        return;
    }
    let pal = app.palette;
    let painter = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Tooltip, Id::unique("note")));
    let galley = painter.layout(text.clone(), FontId::proportional(12.0), pal.text, (rect.width() - 80.0).max(200.0));
    let r = Rect::from_center_size(pos2(rect.center().x, rect.bottom() - 26.0), galley.size() + vec2(28.0, 14.0));
    painter.rect_filled(r, 6.0, pal.header);
    painter.rect_stroke(r, 6.0, Stroke::new(1.0, pal.line), StrokeKind::Inside);
    painter.galley(r.min + vec2(14.0, 7.0), galley, pal.text);
}

pub fn edit_banner(ui: &Ui, rect: Rect, pal: &crate::theme::Palette) {
    let painter = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Tooltip, Id::unique("layout banner")));
    let text = "EDIT LAYOUT  ·  drag panels onto each other  ·  right-click to add or remove  ·  Esc when done";
    let galley = painter.layout_no_wrap(text.into(), FontId::proportional(11.0), pal.on_accent);
    let r = Rect::from_center_size(pos2(rect.center().x, rect.bottom() - 22.0), galley.size() + vec2(24.0, 12.0));
    painter.rect_filled(r, 6.0, pal.accent);
    painter.galley(r.min + vec2(12.0, 6.0), galley, pal.on_accent);
}

fn timeline(app: &mut App, ui: &mut Ui) {
    let rect = ui.max_rect();
    app.view.width = rect.width();
    let pal = app.palette;
    let accent = pal.accent;
    let ruler = Rect::from_min_size(rect.min, vec2(rect.width(), 22.0));
    let track_h = (rect.height() - ruler.height() - 20.0).clamp(24.0, 84.0);
    let track = Rect::from_min_size(pos2(rect.left(), ruler.bottom() + 10.0), vec2(rect.width(), track_h));
    let painter = ui.painter().clone();
    painter.rect_filled(ruler, 0.0, pal.header);
    painter.rect_filled(Rect::from_x_y_ranges(rect.x_range(), ruler.bottom()..=ruler.bottom() + 1.0), 0.0, pal.line);
    painter.rect_filled(track, 0.0, pal.header);

    let bg = ui.interact(rect, ui.scope_id().with("timeline bg"), Sense::click_and_drag());

    // Scroll and zoom (cmd/ctrl + scroll, or pinch) under the pointer.
    if bg.contains_pointer() {
        let (scroll, zoom, ptr) = ui.input(|i| (i.smooth_scroll_delta(), i.zoom_delta(), i.pointer.hover_pos()));
        if zoom != 1.0 {
            let x = ptr.map_or(0.0, |p| p.x - rect.left());
            let at = app.view.scroll + (x / app.view.pps) as f64;
            app.view.pps = (app.view.pps * zoom).clamp(0.5, 2000.0);
            app.view.scroll = at - (x / app.view.pps) as f64;
        } else if scroll != egui::Vec2::ZERO {
            app.view.scroll -= ((scroll.x + scroll.y) / app.view.pps) as f64;
        }
    }
    let duration = app.timeline.duration();
    let max_scroll = (duration - (rect.width() * 0.5 / app.view.pps) as f64).max(0.0);
    // While playing, the view follows the playhead.
    if app.playing {
        let right = app.view.scroll + (rect.width() / app.view.pps) as f64;
        if app.playhead > right - 0.05 * (right - app.view.scroll) || app.playhead < app.view.scroll {
            app.view.scroll = app.playhead - 0.1 * (rect.width() / app.view.pps) as f64;
        }
    }
    app.view.scroll = app.view.scroll.clamp(0.0, max_scroll);

    let (pps, scroll) = (app.view.pps, app.view.scroll);
    let x_of = |t: f64| rect.left() + ((t - scroll) as f32) * pps;
    let t_of = |x: f32| scroll + ((x - rect.left()) / pps) as f64;
    let t_right = t_of(rect.right());

    // Ruler: a tick step that leaves room for its labels.
    let fps = app.format().fps;
    let steps = [1.0 / fps, 0.1, 0.25, 0.5, 1.0, 2.0, 5.0, 10.0, 15.0, 30.0, 60.0, 120.0, 300.0, 600.0, 1800.0, 3600.0];
    let step = steps.iter().copied().find(|s| s * pps as f64 >= 70.0).unwrap_or(3600.0);
    let mut t = (scroll / step).floor() * step;
    while t <= t_right {
        let x = x_of(t);
        painter.line_segment([pos2(x, ruler.bottom() - 5.0), pos2(x, ruler.bottom())], Stroke::new(1.0, pal.weak));
        let label = if step < 1.0 { timecode(t, fps) } else { short_time(t) };
        painter.text(pos2(x + 4.0, ruler.top() + 4.0), Align2::LEFT_TOP, label, FontId::monospace(9.5), pal.weak);
        t += step;
    }

    // Clips, starting from the first one in view.
    let pointer = ui.input(|i| i.pointer.interact_pos());
    let mut first = app.timeline.first_after(scroll);
    let n = app.timeline.len();
    let mut new_drag = None;
    while first < n && app.timeline.start(first) < t_right {
        let i = first;
        first += 1;
        let c = app.timeline.clips()[i].clone();
        let r = Rect::from_x_y_ranges(x_of(app.timeline.start(i))..=x_of(app.timeline.start(i + 1)), track.y_range());
        let body = ui.interact(r, ui.scope_id().with(("clip", i)), Sense::click_and_drag());
        // A hairline between clips, like the one between panels.
        let shape = Rect::from_x_y_ranges(r.left() + 0.5..=r.right() - 0.5, r.y_range());
        painter.rect_filled(shape, 3.0, pal.clip(c.media));
        let looks = app.looks.get(&app.media[c.media].path);
        if let Some(l) = looks {
            draw_looks(&painter.with_clip_rect(shape.shrink(1.0).intersect(rect)), shape, r.left(), &c, l, pps, pal.text);
        }
        if app.selected_clip == Some(i) {
            painter.rect_stroke(shape, 3.0, Stroke::new(1.5, accent), StrokeKind::Inside);
        }
        if r.width() > 24.0 {
            let m = &app.media[c.media];
            let clip_painter = painter.with_clip_rect(r.shrink(4.0).intersect(rect));
            let name = clip_painter.layout_no_wrap(m.name(), FontId::proportional(11.0), pal.text);
            let time = clip_painter.layout_no_wrap(short_time(c.len()), FontId::monospace(9.5), pal.text.gamma_multiply(0.6));
            // Over pictures, the words get a backing of the clip's color.
            if looks.is_some_and(|l| l.thumbnails.is_some()) {
                let w = name.size().x.max(time.size().x);
                clip_painter.rect_filled(Rect::from_min_size(r.left_top() + vec2(5.0, 4.0), vec2(w + 6.0, 30.0)), 3.0, pal.clip(c.media).gamma_multiply(0.85));
            }
            clip_painter.galley(r.left_top() + vec2(8.0, 6.0), name, pal.text);
            clip_painter.galley(r.left_top() + vec2(8.0, 21.0), time, pal.text);
        }
        if body.clicked() || body.drag_started() {
            app.selected_clip = Some(i);
        }
        if body.clicked()
            && let Some(p) = pointer
        {
            app.seek(t_of(p.x));
        }
        if body.drag_started() {
            new_drag = Some(TimelineDrag::Move { clip: i });
        }
        // Trim handles at each end.
        for left in [true, false] {
            let x = if left { r.left() } else { r.right() };
            let h = Rect::from_x_y_ranges(x - 4.0..=x + 4.0, track.y_range());
            let hr = ui.interact(h, ui.scope_id().with(("trim", i, left)), Sense::drag());
            if hr.hovered() || hr.dragged() {
                ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal);
                painter.rect_filled(Rect::from_x_y_ranges(x - 1.5..=x + 1.5, track.y_range()), 1.0, accent);
            }
            if hr.drag_started()
                && let Some(p) = pointer
            {
                app.snapshot();
                app.selected_clip = Some(i);
                new_drag = Some(TimelineDrag::Trim { clip: i, left, from: if left { c.src_in } else { c.src_out }, x0: p.x });
            }
        }
    }
    if let Some(d) = new_drag {
        app.view.drag = d;
    }

    // Crossfades: a cross over each, as long as the fade.
    let mut i = app.timeline.first_after(scroll).max(1);
    while i < n && app.timeline.start(i) < t_right + 10.0 {
        let d = app.timeline.fade(i);
        if d > 0.0 {
            let at = app.timeline.start(i);
            let (x0, x1) = (x_of(at - d * 0.5), x_of(at + d * 0.5));
            let (top, bottom) = (track.top() + 2.0, track.bottom() - 2.0);
            let f = Rect::from_x_y_ranges(x0..=x1, top..=bottom);
            painter.rect_filled(f, 2.0, pal.text.gamma_multiply(0.12));
            let stroke = Stroke::new(1.0, pal.text.gamma_multiply(0.7));
            painter.line_segment([pos2(x0, bottom), pos2(x1, top)], stroke);
            painter.line_segment([pos2(x0, top), pos2(x1, bottom)], stroke);
        }
        i += 1;
    }

    // Snapping, when an adapter adds it; held Alt (Option) turns it off.
    let snapping = app.adapters.snapping().filter(|_| !ui.input(|i| i.modifiers.alt)).cloned();
    let snaps_to = |what: &str| snapping.as_ref().is_some_and(|s| s.to.iter().any(|t| t == what));
    let near = snapping.as_ref().map_or(0.0, |s| (s.pixels / pps) as f64);
    let mut snapped_at = None;
    let seek_to = |app: &mut App, t: f64, snapped_at: &mut Option<f64>| {
        let cut = app.timeline.nearest_cut(t);
        if snaps_to("cuts") && (cut - t).abs() <= near {
            *snapped_at = Some(cut);
            app.seek(cut);
        } else {
            app.seek(t);
        }
    };

    // Ongoing drags.
    let released = ui.input(|i| i.pointer.any_released());
    match app.view.drag {
        TimelineDrag::Trim { clip, left, from, x0 } if clip < app.timeline.len() => {
            if let Some(p) = pointer {
                let m = &app.media[app.timeline.clips()[clip].media];
                let (mfps, end) = (m.fps, m.duration);
                let v = from + ((p.x - x0) / pps) as f64;
                let mut v = (v * mfps).round() / mfps;
                // A clip's end snaps to the playhead.
                let start = app.timeline.start(clip);
                let src_in = app.timeline.clips()[clip].src_in;
                if !left && snaps_to("playhead") && (start + (v - src_in) - app.playhead).abs() <= near && app.playhead > start {
                    v = src_in + (app.playhead - start);
                    snapped_at = Some(app.playhead);
                }
                if left {
                    app.timeline.set_in(clip, v, 1.0 / mfps);
                } else {
                    app.timeline.set_out(clip, v, end, 1.0 / mfps);
                }
                app.edited();
            }
        }
        TimelineDrag::Move { clip } if clip < app.timeline.len() => {
            if let Some(p) = pointer {
                let slot = app.timeline.slot_at(t_of(p.x));
                let at = if slot < app.timeline.len() { app.timeline.start(slot) } else { app.timeline.duration() };
                painter.line_segment([pos2(x_of(at), track.top() - 4.0), pos2(x_of(at), track.bottom() + 4.0)], Stroke::new(3.0, accent));
                ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
                if released && slot != clip && slot != clip + 1 {
                    app.snapshot();
                    app.timeline.move_clip(clip, slot);
                    app.selected_clip = Some(if slot > clip { slot - 1 } else { slot });
                }
            }
        }
        TimelineDrag::Scrub => {
            if let Some(p) = pointer {
                seek_to(app, t_of(p.x), &mut snapped_at);
            }
        }
        _ => {}
    }
    if bg.drag_started() && app.view.drag == TimelineDrag::None {
        app.view.drag = TimelineDrag::Scrub;
    }
    if bg.clicked()
        && let Some(p) = pointer
    {
        seek_to(app, t_of(p.x), &mut snapped_at);
    }
    if let Some(t) = snapped_at {
        let x = x_of(t);
        painter.line_segment([pos2(x, ruler.bottom()), pos2(x, track.bottom() + 6.0)], Stroke::new(1.0, pal.text.gamma_multiply(0.8)));
    }
    if released || !ui.input(|i| i.pointer.any_down()) {
        app.view.drag = TimelineDrag::None;
    }

    // Media dragged in from the Media panel.
    if let Some(p) = pointer
        && bg.contains_pointer()
        && let Some(d) = bg.dnd_hover_payload::<MediaDrag>()
    {
        let slot = app.timeline.slot_at(t_of(p.x));
        let at = if slot < app.timeline.len() { app.timeline.start(slot) } else { duration };
        painter.line_segment([pos2(x_of(at), track.top() - 4.0), pos2(x_of(at), track.bottom() + 4.0)], Stroke::new(3.0, accent));
        let _ = d;
    }
    if let Some(d) = bg.dnd_release_payload::<MediaDrag>()
        && let Some(p) = pointer
    {
        let slot = app.timeline.slot_at(t_of(p.x));
        app.add_clip(d.0, slot);
    }

    if app.timeline.is_empty() {
        painter.text(track.center(), Align2::CENTER_CENTER, "Drag media here, or double-click it", FontId::proportional(12.0), pal.weak);
    }

    // Playhead.
    let x = x_of(app.playhead);
    if rect.x_range().contains(x) {
        painter.line_segment([pos2(x, ruler.top()), pos2(x, track.bottom() + 6.0)], Stroke::new(1.5, accent));
        painter.add(egui::Shape::convex_polygon(
            vec![pos2(x - 5.0, ruler.top()), pos2(x + 5.0, ruler.top()), pos2(x, ruler.top() + 7.0)],
            accent,
            Stroke::NONE,
        ));
    }
}

/// An adapter's thumbnails and waveform inside a clip's `shape`; `left` is
/// where the clip begins, at `c.src_in`.
fn draw_looks(p: &egui::Painter, shape: Rect, left: f32, c: &crate::timeline::Clip, l: &crate::Looks, pps: f32, ink: Color32) {
    let view = p.clip_rect().intersect(shape);
    if view.width() <= 0.0 {
        return;
    }
    let both = l.thumbnails.is_some() && l.waveform.is_some();
    let split = if both { shape.top() + shape.height() * 0.62 } else { shape.bottom() };
    let src_at = |x: f32| c.src_in + ((x - left) / pps) as f64;
    if let Some(t) = &l.thumbnails {
        let lane = Rect::from_x_y_ranges(shape.x_range(), shape.top()..=split);
        let tw = lane.height() * t.w as f32 / t.h as f32;
        // Tiles line up with the clip's start, and only those in view.
        let mut x = left + ((view.left() - left) / tw).floor().max(0.0) * tw;
        while x < view.right() {
            let k = ((src_at(x + tw * 0.5) / t.every) as usize).min(t.count - 1);
            let (col, row) = ((k % t.columns) as f32, (k / t.columns) as f32);
            let uv = Rect::from_min_size(
                pos2(col * t.w as f32 / t.size[0], row * t.h as f32 / t.size[1]),
                vec2(t.w as f32 / t.size[0], t.h as f32 / t.size[1]),
            );
            p.image(t.texture.id(), Rect::from_min_size(pos2(x, lane.top()), vec2(tw, lane.height())), uv, Color32::from_gray(225));
            x += tw;
        }
    }
    if let Some(w) = &l.waveform {
        let lane = Rect::from_x_y_ranges(shape.x_range(), if both { split..=shape.bottom() } else { shape.top()..=shape.bottom() }).shrink2(vec2(0.0, 3.0));
        let (mid, half) = (lane.center().y, lane.height() * 0.5);
        let color = ink.gamma_multiply(if both { 0.55 } else { 0.4 });
        let mut mesh = egui::Mesh::default();
        // One bar per point: the loudest peak under it.
        let mut x = view.left().floor();
        while x < view.right() {
            let a = ((src_at(x) * w.per_second) as usize).min(w.peaks.len());
            let b = ((src_at(x + 1.0) * w.per_second).ceil() as usize).clamp(a + 1, w.peaks.len().max(a + 1)).min(w.peaks.len());
            let peak = w.peaks[a..b].iter().copied().max().unwrap_or(0);
            let h = (peak as f32 / 255.0 * half).max(0.5);
            mesh.add_colored_rect(Rect::from_x_y_ranges(x..=x + 1.0, mid - h..=mid + h), color);
            x += 1.0;
        }
        p.add(mesh);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timecodes() {
        assert_eq!(timecode(0.0, 30.0), "00:00:00");
        assert_eq!(timecode(61.5, 30.0), "01:01:15");
        assert_eq!(timecode(3600.0 + 2.0 / 30.0, 30.0), "1:00:00:02");
        assert_eq!(timecode(10.0, 30000.0 / 1001.0), "00:10:00");
        assert_eq!(timecode(10.0 - 1e-9, 25.0), "00:10:00");
        assert_eq!(short_time(75.0), "1:15");
    }

    /// Drags on the timeline with real pointer events through egui, with
    /// a Snapping adapter (only its adapter.toml) built in.
    #[test]
    fn snapping_follows_its_adapter() {
        let dir = std::env::temp_dir().join(format!("splice9000-snap-{}", std::process::id()));
        let builtin = dir.join("builtin");
        std::fs::create_dir_all(builtin.join("snapping")).unwrap();
        std::fs::create_dir_all(dir.join("config")).unwrap();
        std::fs::write(builtin.join("snapping/adapter.toml"), "name = \"Snapping\"\n[snapping]\nto = [\"cuts\", \"playhead\"]\npixels = 8\n").unwrap();
        // SAFETY: only this test reads or writes the environment.
        unsafe { std::env::set_var("SPLICE9000_BUILTIN_ADAPTERS", &builtin) };
        let ctx = egui::Context::default();
        let mut app = App::new(&ctx, dir.join("config"));
        assert!(app.adapters.snapping().is_some());
        let m = crate::media::Media { path: "/m.mp4".into(), duration: 4.0, width: 640, height: 360, fps: 30.0, audio: false };
        app.media = vec![m];
        let clip = |a, b| crate::timeline::Clip { media: 0, src_in: a, src_out: b, fade: 0.0 };
        app.timeline = crate::timeline::Timeline::new(vec![clip(0.0, 2.0), clip(2.0, 3.0)]);
        let screen = Rect::from_min_size(Pos2::ZERO, vec2(1000.0, 200.0));
        let frame = |app: &mut App, mut events: Vec<egui::Event>, alt: bool| {
            events.insert(0, egui::Event::ModifiersChanged(egui::Modifiers { alt, ..Default::default() }));
            let input = egui::RawInput { screen_rect: Some(screen), events, ..Default::default() };
            let mut out = ctx.run_ui(input, |ui| {
                app.view.pps = 100.0;
                app.view.scroll = 0.0;
                timeline(app, ui);
            });
            out.textures_delta.clear();
        };
        let button = |pos: Pos2, pressed| egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
        let drag = |app: &mut App, from: Pos2, to: Pos2, alt: bool| {
            frame(app, vec![], alt);
            frame(app, vec![egui::Event::PointerMoved(from)], alt);
            frame(app, vec![button(from, true)], alt);
            for k in 1..=8 {
                frame(app, vec![egui::Event::PointerMoved(from + (to - from) * (k as f32 / 8.0))], alt);
            }
            frame(app, vec![button(to, false)], alt);
        };
        // Scrubbing on the ruler (100 points a second) to 1.96 s: 4 points
        // from the cut at 2 s, so it lands on the cut; with Alt it doesn't.
        drag(&mut app, pos2(150.0, 10.0), pos2(196.0, 10.0), false);
        assert_eq!(app.playhead, 2.0);
        drag(&mut app, pos2(150.0, 10.0), pos2(196.0, 10.0), true);
        assert!((app.playhead - 59.0 / 30.0).abs() < 1e-9, "{}", app.playhead);
        // Trimming the last clip's end (at 5 s) out to 5.95 s, near the
        // playhead at 6 s: the end lands on the playhead.
        // With Alt, it ends where it was let go (as egui saw the drag start).
        for alt in [false, true] {
            app.playhead = 6.0;
            app.timeline = crate::timeline::Timeline::new(vec![clip(0.0, 2.0), clip(0.0, 3.0)]);
            drag(&mut app, pos2(500.0, 60.0), pos2(595.0, 60.0), alt);
            let end = app.timeline.clips()[1].src_out;
            assert!(if alt { (3.9..3.97).contains(&end) } else { end == 4.0 }, "{:?}", app.timeline.clips());
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
