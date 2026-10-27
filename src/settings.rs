//! settings.toml. Every field has a default, so a missing or half-written
//! file still works, and unknown keys are ignored so old files keep working.

use serde::Deserialize;

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Path to ffmpeg; empty: look for it.
    pub ffmpeg: String,
    pub preview: Preview,
    pub export: Export,
    pub look: Look,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct Preview {
    /// The preview is decoded at most this tall, whatever the panel's size.
    pub max_height: u32,
    /// Frames per second while playing, at most.
    pub max_fps: f64,
    /// Let ffmpeg use the system's video decoder (VideoToolbox, D3D11, VAAPI).
    pub hwaccel: bool,
    /// Sound while playing.
    pub audio: bool,
    /// 1 is as recorded.
    pub volume: f32,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct Export {
    /// 0: the first clip's own.
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub video_codec: String,
    /// x264/x265 CRF: lower is better and bigger.
    pub quality: u32,
    pub preset: String,
    pub audio_codec: String,
    pub audio_bitrate: String,
    /// Passed to ffmpeg just before the output file.
    pub extra_args: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct Look {
    pub dark: bool,
    pub accent: String,
    pub font_size: f32,
    /// Each panel has a slim header to drag it by.
    pub panel_headers: bool,
}

impl Default for Preview {
    fn default() -> Self {
        Self { max_height: 1080, max_fps: 60.0, hwaccel: true, audio: true, volume: 1.0 }
    }
}

impl Default for Export {
    fn default() -> Self {
        Self {
            width: 0,
            height: 0,
            fps: 0.0,
            video_codec: "libx264".into(),
            quality: 20,
            preset: "veryfast".into(),
            audio_codec: "aac".into(),
            audio_bitrate: "192k".into(),
            extra_args: Vec::new(),
        }
    }
}

impl Default for Look {
    fn default() -> Self {
        Self { dark: true, accent: "#ff5c39".into(), font_size: 13.0, panel_headers: true }
    }
}

impl Look {
    pub fn accent(&self) -> egui::Color32 {
        let hex = self.accent.trim_start_matches('#');
        match (hex.len(), u32::from_str_radix(hex, 16)) {
            (6, Ok(v)) => egui::Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8),
            _ => crate::theme::SPLICE_ORANGE,
        }
    }
}

pub const EXAMPLE: &str = r##"# splice9000 settings. Saved changes apply at once; delete a line to get
# its default back. Everything here is optional.

# Where ffmpeg is, if splice9000 can't find it on its own.
# ffmpeg = "/opt/homebrew/bin/ffmpeg"

[preview]
# The preview is decoded at the panel's size, but never taller than this.
max_height = 1080
# Frames per second while playing, at most.
max_fps = 60
# Use the system's video decoder (less CPU, especially for 4K and HEVC).
hwaccel = true
# Sound while playing, and its volume (1 is as recorded). `m` mutes.
audio = true
volume = 1.0

[export]
# Picture size and frame rate. 0: take them from the first clip.
width = 0
height = 0
fps = 0
# Any ffmpeg encoder: libx264, libx265, h264_videotoolbox, hevc_videotoolbox,
# libsvtav1, prores_ks...
video_codec = "libx264"
# For libx264/libx265: CRF (lower = better and bigger) and speed preset.
quality = 20
preset = "veryfast"
audio_codec = "aac"
audio_bitrate = "192k"
# Anything else for ffmpeg, put just before the output file.
# extra_args = ["-tune", "film"]

[look]
dark = true
# The one signal color: playhead, selection, play button.
accent = "#ff5c39"
font_size = 13
# A slim header on each panel, to drag it somewhere else. Off: use
# Edit Layout (cmd+L) to move panels.
panel_headers = true
"##;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn example_parses_and_half_files_work() {
        let s: Settings = toml::from_str(EXAMPLE).unwrap();
        assert_eq!(s.export.quality, 20);
        let s: Settings = toml::from_str("[export]\nquality = 30\nsomething_new = 1").unwrap();
        assert_eq!((s.export.quality, s.export.preset.as_str(), s.preview.max_height), (30, "veryfast", 1080));
        assert_eq!(Look { accent: "#ff0000".into(), ..Look::default() }.accent(), egui::Color32::RED);
    }
}
