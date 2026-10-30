//! splice9000's adapters: the 9000 series' shared host (core9000), with
//! what splice9000 adds to `adapter.toml` (export formats and snapping) and
//! the permissions of its own requests. The protocol is in docs/adapters.md.

use serde::{Deserialize, Serialize};

pub use core9000::adapters::{Env, Message, StartError, builtin_dir, command_name, describe, is_package, parse_command, remember_approval, summary};

/// The version of splice9000's own requests (`edit.*`, `playback.set`,
/// `media.*`), sent in `initialize` as `protocol`. New versions only add
/// things.
pub const PROTOCOL: u32 = 3;

pub const EDIT_READ: &str = "edit.read";
pub const EDIT_CHANGE: &str = "edit.change";

/// Who splice9000 is, for core9000.
pub static APP: core9000::App = core9000::App {
    name: "splice9000",
    version: env!("CARGO_PKG_VERSION"),
    env_prefix: "SPLICE9000",
    legacy_env_prefix: "",
    protocol: PROTOCOL,
    permissions: &[
        (EDIT_READ, "see your edit: the clips, media files and playhead"),
        (EDIT_CHANGE, "change your edit, add media and move the playhead"),
    ],
    events_permission: EDIT_READ,
};

/// What splice9000 reads from `adapter.toml` beyond what every sister does.
#[derive(Deserialize, Clone, Debug, Default)]
pub struct Extra {
    #[serde(default)]
    pub exports: Vec<ExportSpec>,
    /// Snapping while dragging on the timeline (protocol 3).
    #[serde(default)]
    pub snapping: Option<SnapSpec>,
}

impl core9000::adapters::Extra for Extra {
    fn adds(&self) -> Vec<String> {
        let mut adds = Vec::new();
        if !self.exports.is_empty() {
            adds.push(format!("{} export format{}", self.exports.len(), if self.exports.len() == 1 { "" } else { "s" }));
        }
        if self.snapping.is_some() {
            adds.push("snapping on the timeline".into());
        }
        adds
    }

    fn without_program(&self) -> bool {
        !self.exports.is_empty() || self.snapping.is_some()
    }

    const ALONE: &'static str = "`[[exports]]` or `[snapping]`";
}

pub type Manifest = core9000::adapters::Manifest<Extra>;
pub type Host = core9000::adapters::Host<Extra>;

pub fn package_manifest(path: &std::path::Path) -> Result<Manifest, String> {
    core9000::adapters::package_manifest(path)
}

pub fn install(source: &std::path::Path, config: &std::path::Path) -> Result<Manifest, String> {
    core9000::adapters::install(source, config)
}

/// splice9000's sections, across every adapter.
pub trait Sections {
    /// Every export format, with the adapter it comes from.
    fn exports(&self) -> impl Iterator<Item = (&str, &ExportSpec)>;
    fn export(&self, adapter: &str, id: &str) -> Option<&ExportSpec>;
    /// Snapping, from the first adapter that has it.
    fn snapping(&self) -> Option<&SnapSpec>;
}

impl Sections for Host {
    fn exports(&self) -> impl Iterator<Item = (&str, &ExportSpec)> {
        self.all.iter().flat_map(|a| a.manifest.extra.exports.iter().map(move |e| (a.manifest.name.as_str(), e)))
    }

    fn export(&self, adapter: &str, id: &str) -> Option<&ExportSpec> {
        self.exports().find(|(a, e)| *a == adapter && e.id == id).map(|(_, e)| e)
    }

    fn snapping(&self) -> Option<&SnapSpec> {
        self.all.iter().find_map(|a| a.manifest.extra.snapping.as_ref())
    }
}

/// What dragging on the timeline snaps to, and from how close.
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct SnapSpec {
    /// "cuts" (the playhead to the cuts) and "playhead" (a clip's end to
    /// the playhead).
    #[serde(default)]
    pub to: Vec<String>,
    #[serde(default = "snap_pixels")]
    pub pixels: f32,
}

fn snap_pixels() -> f32 {
    8.0
}

/// An export format: how ffmpeg writes the edit. Anything left out comes
/// from settings.toml.
#[derive(Deserialize, Serialize, Clone, Debug, Default, PartialEq)]
pub struct ExportSpec {
    pub id: String,
    pub label: String,
    /// The file extension, such as "mp4" or "gif".
    pub extension: String,
    /// Picture size and frame rate; 0 keeps the edit's own.
    #[serde(default)]
    pub width: u32,
    #[serde(default)]
    pub height: u32,
    #[serde(default)]
    pub fps: f64,
    /// ffmpeg's video encoder arguments, in place of settings.toml's codec,
    /// such as ["-c:v", "prores_ks", "-profile:v", "3"].
    #[serde(default)]
    pub video: Option<Vec<String>>,
    /// The same for sound, such as ["-c:a", "libopus", "-b:a", "128k"].
    #[serde(default)]
    pub audio: Option<Vec<String>>,
    /// An ffmpeg filter run on the finished picture, such as a GIF palette.
    #[serde(default)]
    pub video_filter: String,
    /// An ffmpeg filter run on the finished sound, such as `loudnorm`
    /// (protocol 2).
    #[serde(default)]
    pub audio_filter: String,
    /// Leave the picture out (sound-only formats).
    #[serde(default)]
    pub no_video: bool,
    /// Leave the sound out (GIF).
    #[serde(default)]
    pub no_audio: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapping_needs_no_program() {
        core9000::init(&APP);
        let m: Manifest = core9000::adapters::parse_manifest("name = \"Snapping\"\nrequires = \"0.0.7\"\n[snapping]\nto = [\"cuts\"]\n", "x").unwrap();
        assert_eq!(m.extra.snapping, Some(SnapSpec { to: vec!["cuts".into()], pixels: 8.0 }));
        assert!(summary(&m).iter().any(|l| l.contains("snapping on the timeline")));
        assert!(core9000::adapters::parse_manifest::<Extra>("name = \"Empty\"\n", "x").is_err());
    }
}
