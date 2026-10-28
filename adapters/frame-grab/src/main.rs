//! Frame Grab: a splice9000 adapter that saves the frame under the playhead
//! as a full-size PNG, with the same ffmpeg splice9000 uses.
//!
//! It speaks splice9000's adapter protocol (docs/adapters.md): JSON
//! messages, one per line, on stdin and stdout.

use std::io::{BufRead, Write};
use std::process::Command;

use serde_json::{Value, json};

struct Host {
    lines: std::io::Lines<std::io::StdinLock<'static>>,
    next_id: u64,
    ffmpeg: String,
}

impl Host {
    fn send(&self, message: Value) {
        let mut out = std::io::stdout().lock();
        let _ = writeln!(out, "{message}");
        let _ = out.flush();
    }

    fn notify(&self, method: &str, params: Value) {
        self.send(json!({ "method": method, "params": params }));
    }

    /// Asks splice9000 for something and waits for the answer. Anything
    /// else that arrives meanwhile is set aside.
    fn request(&mut self, method: &str, params: Value) -> Result<Value, String> {
        self.next_id += 1;
        let id = self.next_id;
        self.send(json!({ "id": id, "method": method, "params": params }));
        loop {
            let line = self.lines.next().ok_or("splice9000 went away")?.map_err(|e| e.to_string())?;
            let Ok(m) = serde_json::from_str::<Value>(&line) else { continue };
            if m.get("method").is_none() && m["id"] == json!(id) {
                return match m.get("error") {
                    Some(e) => Err(e.as_str().unwrap_or("error").to_owned()),
                    None => Ok(m["result"].clone()),
                };
            }
        }
    }
}

fn main() {
    let mut host = Host { lines: std::io::stdin().lines(), next_id: 0, ffmpeg: "ffmpeg".into() };
    while let Some(Ok(line)) = host.lines.next() {
        let Ok(m) = serde_json::from_str::<Value>(&line) else { continue };
        match m["method"].as_str() {
            Some("initialize") => {
                if let Some(f) = m["params"]["ffmpeg"].as_str() {
                    host.ffmpeg = f.to_owned();
                }
            }
            Some("command") if m["params"]["command"] == "grab" => {
                if let Err(e) = grab(&mut host) {
                    host.notify("notify", json!({ "text": e }));
                }
            }
            Some("shutdown") => return,
            _ => {}
        }
    }
}

/// Where the frame at `playhead` comes from: its file and the time in it.
fn locate(edit: &Value) -> Option<(String, f64, f64)> {
    let t = edit["playhead"].as_f64()?;
    let clips = edit["clips"].as_array()?;
    let clip = clips.iter().rev().find(|c| c["start"].as_f64().is_some_and(|s| s <= t + 1e-9))?;
    let src = clip["src_in"].as_f64()? + (t - clip["start"].as_f64()?);
    if src >= clip["src_out"].as_f64()? {
        return None;
    }
    let media = &edit["media"][clip["media"].as_u64()? as usize];
    Some((media["path"].as_str()?.to_owned(), src, media["fps"].as_f64().unwrap_or(30.0)))
}

fn grab(host: &mut Host) -> Result<(), String> {
    let edit = host.request("edit.get", json!({}))?;
    let (path, src, fps) = locate(&edit).ok_or("The playhead isn't on a clip.")?;
    let stem = std::path::Path::new(&path).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "frame".into());
    let frame = (src * fps + 1e-6).floor() as u64;
    let name = format!("{stem} frame {frame}.png");
    let out = host.request("ui.save_dialog", json!({ "title": "Save Frame", "file_name": name, "extension": "png" }))?;
    let Some(out) = out.as_str() else { return Ok(()) };
    // The exact frame: seek to just before its start time.
    let at = (frame as f64 / fps - 1e-4).max(0.0);
    let status = Command::new(&host.ffmpeg)
        .args(["-hide_banner", "-loglevel", "error", "-nostdin", "-y", "-ss", &format!("{at:.6}"), "-i", &path, "-frames:v", "1", "-update", "1", out])
        .status()
        .map_err(|e| format!("Couldn't run ffmpeg: {e}"))?;
    if !status.success() {
        return Err(format!("ffmpeg couldn't save the frame ({status})"));
    }
    host.notify("notify", json!({ "text": format!("Saved {}", std::path::Path::new(out).file_name().map(|n| n.to_string_lossy()).unwrap_or_default()) }));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_frame_under_the_playhead() {
        let edit = json!({
            "playhead": 3.5,
            "media": [{ "path": "/a.mp4", "fps": 30.0 }, { "path": "/b.mp4", "fps": 25.0 }],
            "clips": [
                { "media": 0, "src_in": 1.0, "src_out": 4.0, "start": 0.0 },
                { "media": 1, "src_in": 10.0, "src_out": 12.0, "start": 3.0 },
            ],
        });
        assert_eq!(locate(&edit), Some(("/b.mp4".into(), 10.5, 25.0)));
        let mut end = edit.clone();
        end["playhead"] = json!(5.0);
        assert_eq!(locate(&end), None);
    }
}
