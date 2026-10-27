//! Media files and the ffmpeg programs that read and write them.
//!
//! splice9000 links no codecs: it runs the system's `ffmpeg` and `ffprobe`
//! as separate programs, the way Folio runs adapters. A broken file or a
//! crashing decoder can't take the editor down with it.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Media {
    pub path: PathBuf,
    pub duration: f64,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub audio: bool,
}

impl Media {
    pub fn name(&self) -> String {
        self.path.file_name().map_or_else(|| self.path.display().to_string(), |n| n.to_string_lossy().into_owned())
    }
}

#[derive(Clone, Debug)]
pub struct Tools {
    pub ffmpeg: PathBuf,
    pub ffprobe: PathBuf,
}

impl Tools {
    /// Finds ffmpeg and ffprobe: `configured` (from settings.toml) if set,
    /// otherwise on the PATH or where package managers put them. Apps
    /// started from Finder don't get the shell's PATH, hence the list.
    pub fn find(configured: &str) -> Option<Self> {
        let ffmpeg = if configured.trim().is_empty() {
            search(exe("ffmpeg"))?
        } else {
            let p = PathBuf::from(configured.trim());
            if !p.is_file() {
                return None;
            }
            p
        };
        // ffprobe sits beside ffmpeg in every package of it.
        let ffprobe = ffmpeg.with_file_name(exe("ffprobe"));
        let ffprobe = if ffprobe.is_file() { ffprobe } else { search(exe("ffprobe"))? };
        Some(Self { ffmpeg, ffprobe })
    }

    pub fn probe(&self, path: &Path) -> Result<Media, String> {
        let out = quiet(Command::new(&self.ffprobe))
            .args(["-v", "error", "-of", "json", "-show_entries"])
            .arg("format=duration:stream=codec_type,width,height,avg_frame_rate,r_frame_rate,duration")
            .arg(path)
            .output()
            .map_err(|e| format!("couldn't run ffprobe: {e}"))?;
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr);
            return Err(err.lines().last().unwrap_or("ffprobe couldn't read it").to_owned());
        }
        parse_probe(path, &out.stdout)
    }
}

fn parse_probe(path: &Path, json: &[u8]) -> Result<Media, String> {
    let v: serde_json::Value = serde_json::from_slice(json).map_err(|e| e.to_string())?;
    let streams = v["streams"].as_array().cloned().unwrap_or_default();
    let video = streams
        .iter()
        .find(|s| s["codec_type"] == "video" && s["width"].as_u64().is_some())
        .ok_or("no video in it")?;
    let audio = streams.iter().any(|s| s["codec_type"] == "audio");
    let number = |x: &serde_json::Value| x.as_str().and_then(|s| s.parse::<f64>().ok()).filter(|d| *d > 0.0);
    let duration = number(&v["format"]["duration"]).or_else(|| number(&video["duration"])).ok_or("no length")?;
    let fps = rate(video["avg_frame_rate"].as_str()).or_else(|| rate(video["r_frame_rate"].as_str())).unwrap_or(30.0);
    Ok(Media {
        path: path.to_path_buf(),
        duration,
        width: video["width"].as_u64().unwrap_or(0) as u32,
        height: video["height"].as_u64().unwrap_or(0) as u32,
        fps,
        audio,
    })
}

/// "30000/1001" -> 29.97. Still images report 0/0 or silly rates.
fn rate(s: Option<&str>) -> Option<f64> {
    let (n, d) = s?.split_once('/')?;
    let r = n.parse::<f64>().ok()? / d.parse::<f64>().ok()?;
    (r.is_finite() && (1.0..=240.0).contains(&r)).then_some(r)
}

fn exe(name: &str) -> String {
    if cfg!(windows) { format!("{name}.exe") } else { name.to_owned() }
}

fn search(name: String) -> Option<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    dirs.extend(["/opt/homebrew/bin", "/usr/local/bin", "/opt/local/bin", "/usr/bin", "/snap/bin"].map(PathBuf::from));
    dirs.into_iter().map(|d| d.join(&name)).find(|p| p.is_file())
}

/// A command whose window (on Windows) never shows.
pub fn quiet(mut cmd: Command) -> Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    cmd.stdin(std::process::Stdio::null());
    cmd
}

/// The largest even size of a `w` x `h` picture that fits in `bw` x `bh`,
/// never enlarged. Encoders want even sizes.
pub fn fit(w: u32, h: u32, bw: u32, bh: u32) -> (u32, u32) {
    if w == 0 || h == 0 {
        return (2, 2);
    }
    let scale = (bw as f64 / w as f64).min(bh as f64 / h as f64).min(1.0);
    let even = |x: f64| ((x / 2.0).floor() as u32 * 2).max(2);
    (even(w as f64 * scale), even(h as f64 * scale))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_ffprobe_output() {
        let json = br#"{"streams":[{"codec_type":"audio"},{"codec_type":"video","width":1920,"height":1080,
            "avg_frame_rate":"30000/1001","r_frame_rate":"30000/1001"}],"format":{"duration":"12.5"}}"#;
        let m = parse_probe(Path::new("/a/b.mp4"), json).unwrap();
        assert_eq!((m.width, m.height, m.audio, m.duration), (1920, 1080, true, 12.5));
        assert!((m.fps - 29.97).abs() < 0.01);
        assert_eq!(m.name(), "b.mp4");
        assert!(parse_probe(Path::new("x"), br#"{"streams":[{"codec_type":"audio"}],"format":{"duration":"3"}}"#).is_err());
    }

    #[test]
    fn rates() {
        assert_eq!(rate(Some("25/1")), Some(25.0));
        assert_eq!(rate(Some("0/0")), None);
        assert_eq!(rate(Some("90000/1")), None);
    }

    #[test]
    fn fits_even_sizes() {
        assert_eq!(fit(1920, 1080, 960, 960), (960, 540));
        assert_eq!(fit(1080, 1920, 800, 450), (252, 450));
        assert_eq!(fit(640, 360, 4000, 4000), (640, 360));
        assert_eq!(fit(641, 361, 4000, 4000), (640, 360));
        assert_eq!(fit(0, 10, 10, 10), (2, 2));
    }
}
