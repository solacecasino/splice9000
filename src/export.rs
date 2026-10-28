//! Export: the whole edit in one ffmpeg run.
//!
//! Each clip is read from its source with a seek (`-ss`/`-t` before `-i`),
//! so ffmpeg decodes only the parts that are used. A clip is scaled or
//! retimed only when it differs from the output, and audio is left out
//! entirely when no clip has any.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::adapters::ExportSpec;
use crate::media::{Media, Tools, quiet};
use crate::settings::Export as Settings;
use crate::timeline::Timeline;

/// The output's picture size and frame rate.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Format {
    pub width: u32,
    pub height: u32,
    pub fps: f64,
}

impl Format {
    /// From settings.toml, or the first clip's own where settings say 0.
    pub fn of(timeline: &Timeline, media: &[Media], s: &Settings) -> Self {
        let first = timeline.clips().first().and_then(|c| media.get(c.media));
        let even = |x: u32| (x / 2 * 2).max(2);
        Self {
            width: even(if s.width > 0 { s.width } else { first.map_or(1920, |m| m.width) }),
            height: even(if s.height > 0 { s.height } else { first.map_or(1080, |m| m.height) }),
            fps: if s.fps > 0.0 { s.fps } else { first.map_or(30.0, |m| m.fps) },
        }
    }
}

impl Format {
    /// This format with a preset's size and frame rate, where it sets them.
    pub fn with(self, preset: Option<&ExportSpec>) -> Self {
        let Some(p) = preset else { return self };
        let even = |x: u32| (x / 2 * 2).max(2);
        Self {
            width: if p.width > 0 { even(p.width) } else { self.width },
            height: if p.height > 0 { even(p.height) } else { self.height },
            fps: if p.fps > 0.0 { p.fps } else { self.fps },
        }
    }
}

/// The encoders a format asks ffmpeg for, in settings.toml's format or an
/// adapter's `preset`: what must be in [`Tools::encoders`] to export it.
pub fn encoders_needed(s: &Settings, preset: Option<&ExportSpec>) -> Vec<String> {
    let video = !preset.is_some_and(|p| p.no_video);
    let audio = !preset.is_some_and(|p| p.no_audio);
    let mut args: Vec<String> = Vec::new();
    if video {
        match preset.and_then(|p| p.video.as_ref()) {
            Some(v) => args.extend(v.iter().cloned()),
            None => args.extend(["-c:v".into(), s.video_codec.clone()]),
        }
    }
    if audio {
        match preset.and_then(|p| p.audio.as_ref()) {
            Some(a) => args.extend(a.iter().cloned()),
            None => args.extend(["-c:a".into(), s.audio_codec.clone()]),
        }
    }
    args.windows(2)
        .filter(|w| ["-c:v", "-c:a", "-vcodec", "-acodec", "-codec:v", "-codec:a"].contains(&w[0].as_str()) && w[1] != "copy")
        .map(|w| w[1].clone())
        .collect()
}

/// ffmpeg's arguments for writing `timeline` to `out`, in settings.toml's
/// format or an adapter's `preset`.
pub fn args(timeline: &Timeline, media: &[Media], fmt: Format, s: &Settings, preset: Option<&ExportSpec>, out: &Path) -> Vec<String> {
    let clips = timeline.clips();
    let video = !preset.is_some_and(|p| p.no_video);
    // Sound-only formats always get a sound track, silent if need be.
    let audio = !preset.is_some_and(|p| p.no_audio) && (!video || clips.iter().any(|c| media[c.media].audio));
    let pieces = timeline.pieces(0, |m| media[m].duration);
    let mut a: Vec<String> = ["-hide_banner", "-loglevel", "error", "-nostdin", "-y"].map(String::from).to_vec();
    let input = |a: &mut Vec<String>, m: usize, from: f64, len: f64| {
        a.extend(["-ss".into(), format!("{from:.6}"), "-t".into(), format!("{len:.6}")]);
        a.extend(["-i".into(), media[m].path.to_string_lossy().into_owned()]);
    };
    let fps = fmt.fps;
    let mut graph = Vec::new();
    let mut joined = String::new();
    let mut n = 0;
    for (k, p) in pieces.iter().enumerate() {
        let m = &media[clips[p.clip].media];
        let len = p.len();
        let Some(x) = &p.mix else {
            input(&mut a, clips[p.clip].media, p.src_in, len);
            if video {
                graph.push(format!("[{n}:v:0]{}[v{k}]", picture(m, fmt, false)));
                joined.push_str(&format!("[v{k}]"));
            }
            if audio {
                graph.push(format!("{}[a{k}]", sound(m, n, len, 0.0)));
                joined.push_str(&format!("[a{k}]"));
            }
            n += 1;
            continue;
        };
        // A crossfade: both clips' footage, as long as the piece, faded.
        let b = &media[clips[x.into].media];
        input(&mut a, clips[p.clip].media, p.src_in, x.a_has);
        input(&mut a, clips[x.into].media, x.b_in, len - x.b_wait);
        if video {
            let hold_a = if len - x.a_has > 1e-6 { format!(",tpad=stop_mode=clone:stop_duration={:.6}", len - x.a_has) } else { String::new() };
            let hold_b = if x.b_wait > 1e-6 { format!(",tpad=start_mode=clone:start_duration={:.6}", x.b_wait) } else { String::new() };
            graph.push(format!("[{n}:v:0]{}{hold_a}[x{k}]", picture(m, fmt, true)));
            graph.push(format!("[{}:v:0]{}{hold_b}[y{k}]", n + 1, picture(b, fmt, true)));
            graph.push(format!("[x{k}][y{k}]xfade=transition=fade:duration={len:.6}:offset=0,setsar=1[v{k}]"));
            joined.push_str(&format!("[v{k}]"));
        }
        if audio {
            graph.push(format!("{}[p{k}]", sound(m, n, len, 0.0)));
            graph.push(format!("{}[q{k}]", sound(b, n + 1, len, x.b_wait)));
            graph.push(format!("[p{k}][q{k}]acrossfade=d={len:.6}:c1=tri:c2=tri[a{k}]"));
            joined.push_str(&format!("[a{k}]"));
        }
        n += 2;
    }
    let outs = [video.then_some("[v]"), audio.then_some("[a]")].into_iter().flatten().collect::<String>();
    graph.push(format!("{joined}concat=n={}:v={}:a={}{outs}", pieces.len(), u8::from(video), u8::from(audio)));
    let filter = preset.map_or("", |p| p.video_filter.trim());
    if video && !filter.is_empty() {
        graph.push(format!("[v]{filter}[vf]"));
    }
    let sound_filter = preset.map_or("", |p| p.audio_filter.trim());
    if audio && !sound_filter.is_empty() {
        graph.push(format!("[a]{sound_filter}[af]"));
    }
    a.extend(["-filter_complex".into(), graph.join(";")]);
    if video {
        a.extend(["-map".into(), if filter.is_empty() { "[v]" } else { "[vf]" }.into()]);
        match preset.and_then(|p| p.video.as_ref()) {
            Some(v) => a.extend(v.iter().cloned()),
            None => {
                a.extend(["-c:v".into(), s.video_codec.clone()]);
                if s.video_codec.starts_with("libx26") {
                    a.extend(["-crf".into(), s.quality.to_string(), "-preset".into(), s.preset.clone()]);
                }
                a.extend(["-pix_fmt".into(), "yuv420p".into()]);
            }
        }
        if filter.is_empty() {
            a.extend(["-r".into(), format!("{fps:.6}")]);
        }
    }
    if audio {
        a.extend(["-map".into(), if sound_filter.is_empty() { "[a]" } else { "[af]" }.into()]);
        match preset.and_then(|p| p.audio.as_ref()) {
            Some(x) => a.extend(x.iter().cloned()),
            None => a.extend(["-c:a".into(), s.audio_codec.clone(), "-b:a".into(), s.audio_bitrate.clone()]),
        }
    }
    a.extend(s.extra_args.iter().cloned());
    // Playable while still downloading, for the formats that have it.
    if out.extension().is_some_and(|e| ["mp4", "mov", "m4a", "m4v"].iter().any(|x| e.eq_ignore_ascii_case(x))) {
        a.extend(["-movflags".into(), "+faststart".into()]);
    }
    a.extend(["-progress".into(), "pipe:1".into(), "-nostats".into()]);
    a.push(out.to_string_lossy().into_owned());
    a
}

/// The filters that make one input's picture the output's size and rate.
/// `mixed`: going into a crossfade, which needs both sides alike.
pub fn picture(m: &Media, fmt: Format, mixed: bool) -> String {
    let (w, h, fps) = (fmt.width, fmt.height, fmt.fps);
    let mut v = vec![];
    if mixed || (m.fps - fps).abs() > 0.001 {
        v.push(format!("fps={fps:.6}"));
    }
    if (m.width, m.height) != (w, h) {
        v.push(format!("scale={w}:{h}:force_original_aspect_ratio=decrease"));
        v.push(format!("pad={w}:{h}:(ow-iw)/2:(oh-ih)/2"));
    }
    v.push("setsar=1".into());
    if mixed {
        v.push("format=yuv420p".into());
    }
    v.join(",")
}

/// Input `n`'s sound as `len` seconds of 48 kHz stereo, after `wait`
/// seconds of silence; silence throughout for a file without sound.
fn sound(m: &Media, n: usize, len: f64, wait: f64) -> String {
    let fmt = "aformat=sample_rates=48000:sample_fmts=fltp:channel_layouts=stereo";
    if !m.audio {
        return format!("anullsrc=r=48000:cl=stereo:d={len:.6},{fmt}");
    }
    let delay = if wait > 1e-6 { format!(",adelay=delays={}:all=1", (wait * 1000.0).round() as u64) } else { String::new() };
    // Padded to the piece's length, so a short audio track can't pull the
    // following clips out of sync.
    format!("[{n}:a:0]{fmt}{delay},apad=whole_dur={len:.6}")
}

#[derive(Default)]
pub struct Progress {
    /// Seconds of the edit written so far.
    pub done: f64,
    pub finished: Option<Result<String, String>>,
}

pub struct Job {
    pub out: PathBuf,
    pub total: f64,
    pub started: Instant,
    pub progress: Arc<Mutex<Progress>>,
    cancel: Arc<AtomicBool>,
}

impl Job {
    pub fn start(tools: &Tools, args: Vec<String>, out: PathBuf, total: f64, wake: impl Fn() + Send + 'static) -> Self {
        let progress = Arc::new(Mutex::new(Progress::default()));
        let cancel = Arc::new(AtomicBool::new(false));
        let (p, c, ffmpeg, path) = (progress.clone(), cancel.clone(), tools.ffmpeg.clone(), out.clone());
        let _ = std::thread::Builder::new().name("export".into()).spawn(move || {
            let started = Instant::now();
            let result = run(&ffmpeg, &args, &p, &c, &wake);
            let result = match result {
                Ok(()) => {
                    let size = std::fs::metadata(&path).map_or(0, |m| m.len());
                    let secs = started.elapsed().as_secs_f64();
                    Ok(format!(
                        "{:.1} MB in {secs:.1} s ({:.1}x real time)",
                        size as f64 / 1e6,
                        total / secs.max(0.001)
                    ))
                }
                Err(e) => {
                    // Only ffmpeg's own half-written file goes.
                    let _ = std::fs::remove_file(&path);
                    Err(e)
                }
            };
            p.lock().unwrap_or_else(|e| e.into_inner()).finished = Some(result);
            wake();
        });
        Self { out, total, started: Instant::now(), progress, cancel }
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

fn run(ffmpeg: &Path, args: &[String], p: &Mutex<Progress>, cancel: &AtomicBool, wake: &dyn Fn()) -> Result<(), String> {
    let mut child = quiet(Command::new(ffmpeg))
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("couldn't run ffmpeg: {e}"))?;
    let stderr = child.stderr.take();
    let errors = std::thread::spawn(move || {
        let mut s = String::new();
        if let Some(mut e) = stderr {
            let _ = std::io::Read::read_to_string(&mut e, &mut s);
        }
        s
    });
    if let Some(out) = child.stdout.take() {
        for line in BufReader::new(out).lines().map_while(Result::ok) {
            if cancel.load(Ordering::Relaxed) {
                let _ = child.kill();
                let _ = child.wait();
                return Err("canceled".into());
            }
            if let Some(us) = line.strip_prefix("out_time_us=").and_then(|v| v.parse::<f64>().ok()) {
                p.lock().unwrap_or_else(|e| e.into_inner()).done = us / 1e6;
                wake();
            }
        }
    }
    let status = child.wait().map_err(|e| e.to_string())?;
    let errors = errors.join().unwrap_or_default();
    if status.success() {
        Ok(())
    } else {
        Err(errors.lines().last().unwrap_or("ffmpeg failed").to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timeline::Clip;

    fn media(w: u32, h: u32, fps: f64, audio: bool) -> Media {
        Media { path: format!("/m/{w}.mp4").into(), duration: 60.0, width: w, height: h, fps, audio }
    }

    #[test]
    fn leaves_matching_clips_alone() {
        let m = vec![media(1920, 1080, 30.0, true), media(1280, 720, 25.0, false)];
        let t = Timeline::new(vec![
            Clip { media: 0, src_in: 1.0, src_out: 3.0, fade: 0.0 },
            Clip { media: 1, src_in: 0.0, src_out: 2.5, fade: 0.0 },
        ]);
        let s = Settings::default();
        let fmt = Format::of(&t, &m, &s);
        assert_eq!(fmt, Format { width: 1920, height: 1080, fps: 30.0 });
        let a = args(&t, &m, fmt, &s, None, Path::new("/o.mp4"));
        let graph = &a[a.iter().position(|x| x == "-filter_complex").unwrap() + 1];
        assert!(graph.starts_with("[0:v:0]setsar=1[v0];"), "{graph}");
        assert!(graph.contains("[1:v:0]fps=30.000000,scale=1920:1080"), "{graph}");
        assert!(graph.contains("anullsrc=r=48000:cl=stereo:d=2.500000"), "{graph}");
        assert!(graph.ends_with("[v0][a0][v1][a1]concat=n=2:v=1:a=1[v][a]"), "{graph}");
        assert!(a.windows(2).any(|w| w == ["-ss", "1.000000"]));
        assert_eq!(a.last().unwrap(), "/o.mp4");
    }

    #[test]
    fn no_audio_means_no_audio_track() {
        let m = vec![media(640, 360, 24.0, false)];
        let t = Timeline::new(vec![Clip { media: 0, src_in: 0.0, src_out: 1.0, fade: 0.0 }]);
        let s = Settings::default();
        let a = args(&t, &m, Format::of(&t, &m, &s), &s, None, Path::new("/o.mp4"));
        assert!(!a.iter().any(|x| x == "[a]" || x == "-c:a"));
        assert!(a.iter().any(|x| x == "[0:v:0]setsar=1[v0];[v0]concat=n=1:v=1:a=0[v]"));
    }

    #[test]
    fn names_the_encoders_a_format_needs() {
        let s = Settings::default();
        assert_eq!(encoders_needed(&s, None), ["libx264", "aac"]);
        let webm = ExportSpec {
            video: Some(vec!["-c:v".into(), "libvpx-vp9".into(), "-crf".into(), "32".into()]),
            audio: Some(vec!["-c:a".into(), "libopus".into()]),
            ..ExportSpec::default()
        };
        assert_eq!(encoders_needed(&s, Some(&webm)), ["libvpx-vp9", "libopus"]);
        let gif = ExportSpec { video: Some(vec!["-loop".into(), "0".into()]), no_audio: true, ..ExportSpec::default() };
        assert!(encoders_needed(&s, Some(&gif)).is_empty());
        let sound = ExportSpec { no_video: true, ..ExportSpec::default() };
        assert_eq!(encoders_needed(&s, Some(&sound)), ["aac"]);
    }

    #[test]
    fn presets_change_size_codecs_and_tracks() {
        let m = vec![media(1920, 1080, 30.0, true), media(1280, 720, 25.0, false)];
        let t = Timeline::new(vec![Clip { media: 0, src_in: 0.0, src_out: 2.0, fade: 0.0 }, Clip { media: 1, src_in: 0.0, src_out: 1.0, fade: 0.0 }]);
        let s = Settings::default();
        let base = Format::of(&t, &m, &s);
        let gif = ExportSpec {
            id: "gif".into(),
            label: "GIF".into(),
            extension: "gif".into(),
            width: 480,
            height: 270,
            video_filter: "fps=15,split[a][b];[a]palettegen[p];[b][p]paletteuse".into(),
            no_audio: true,
            video: Some(vec!["-loop".into(), "0".into()]),
            ..ExportSpec::default()
        };
        let fmt = base.with(Some(&gif));
        assert_eq!((fmt.width, fmt.height, fmt.fps), (480, 270, 30.0));
        let a = args(&t, &m, fmt, &s, Some(&gif), Path::new("/o.gif"));
        let graph = &a[a.iter().position(|x| x == "-filter_complex").unwrap() + 1];
        assert!(graph.ends_with("concat=n=2:v=1:a=0[v];[v]fps=15,split[a][b];[a]palettegen[p];[b][p]paletteuse[vf]"), "{graph}");
        assert!(a.windows(2).any(|w| w == ["-map", "[vf]"]) && a.windows(2).any(|w| w == ["-loop", "0"]));
        assert!(!a.iter().any(|x| x == "-c:a" || x == "[a]" || x == "-movflags" || x == "libx264"));

        let sound = ExportSpec {
            id: "m4a".into(),
            label: "Sound".into(),
            extension: "m4a".into(),
            no_video: true,
            audio_filter: "loudnorm=I=-16".into(),
            ..ExportSpec::default()
        };
        let a = args(&t, &m, base, &s, Some(&sound), Path::new("/o.m4a"));
        let graph = &a[a.iter().position(|x| x == "-filter_complex").unwrap() + 1];
        assert!(graph.ends_with("[a0][a1]concat=n=2:v=0:a=1[a];[a]loudnorm=I=-16[af]"), "{graph}");
        assert!(a.windows(2).any(|w| w == ["-map", "[af]"]));
        assert!(!graph.contains(":v:0") && !a.iter().any(|x| x == "-c:v" || x == "[v]"));
        assert!(a.iter().any(|x| x == "-movflags"));
    }

    /// A real export: two clips of different sizes, rates and audio,
    /// joined. Skipped where ffmpeg isn't installed.
    #[test]
    fn exports_with_ffmpeg() {
        let Some(tools) = Tools::find("") else {
            assert!(std::env::var_os("SPLICE9000_REQUIRE_FFMPEG").is_none(), "ffmpeg wasn't found");
            return;
        };
        let dir = std::env::temp_dir().join(format!("splice9000-export-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let make = |name: &str, extra: &[&str]| {
            let path = dir.join(name);
            let ok = Command::new(&tools.ffmpeg)
                .args(["-v", "error", "-y"])
                .args(extra)
                .args(["-c:v", "libx264", "-preset", "ultrafast", "-pix_fmt", "yuv420p"])
                .arg(&path)
                .status()
                .unwrap()
                .success();
            assert!(ok);
            tools.probe(&path).unwrap()
        };
        let a = make("a.mp4", &["-f", "lavfi", "-i", "testsrc2=s=640x360:r=30:d=3", "-f", "lavfi", "-i", "sine=f=440:d=3", "-shortest"]);
        let b = make("b.mp4", &["-f", "lavfi", "-i", "testsrc2=s=320x240:r=25:d=2"]);
        assert!(a.audio && !b.audio);
        let m = vec![a, b];
        let t = Timeline::new(vec![
            Clip { media: 0, src_in: 0.5, src_out: 2.0, fade: 0.0 },
            Clip { media: 1, src_in: 0.0, src_out: 2.0, fade: 0.0 },
            Clip { media: 0, src_in: 2.0, src_out: 3.0, fade: 0.0 },
        ]);
        let s = Settings { preset: "ultrafast".into(), ..Settings::default() };
        let out = dir.join("out.mp4");
        let job = Job::start(&tools, args(&t, &m, Format::of(&t, &m, &s), &s, None, &out), out.clone(), t.duration(), || {});
        let result = loop {
            if let Some(r) = job.progress.lock().unwrap().finished.take() {
                break r;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        };
        result.unwrap();
        let got = tools.probe(&out).unwrap();
        assert_eq!((got.width, got.height, got.audio), (640, 360, true));
        assert!((got.duration - 4.5).abs() < 0.1, "{}", got.duration);
        assert!((got.fps - 30.0).abs() < 0.01);

        // The same with crossfades: one with footage on both sides, one
        // into a file's very start (its first picture holds). The length
        // doesn't change.
        let mut clips = t.clips().to_vec();
        clips[1].fade = 0.8;
        clips[2].fade = 0.5;
        let t = Timeline::new(clips);
        let out = dir.join("faded.mp4");
        let job = Job::start(&tools, args(&t, &m, Format::of(&t, &m, &s), &s, None, &out), out.clone(), t.duration(), || {});
        let result = loop {
            if let Some(r) = job.progress.lock().unwrap().finished.take() {
                break r;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        };
        result.unwrap();
        let got = tools.probe(&out).unwrap();
        assert_eq!((got.width, got.height, got.audio), (640, 360, true));
        assert!((got.duration - 4.5).abs() < 0.1, "{}", got.duration);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Every format the built-in Export Presets adapter ships, exported for
    /// real, so a preset ffmpeg can't do never ships. Skipped where ffmpeg
    /// isn't installed.
    #[test]
    fn built_in_presets_export() {
        let Some(tools) = Tools::find("") else {
            assert!(std::env::var_os("SPLICE9000_REQUIRE_FFMPEG").is_none(), "ffmpeg wasn't found");
            return;
        };
        let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/adapters/export-presets/adapter.toml")).unwrap();
        let manifest: crate::adapters::Manifest = toml::from_str(&text).unwrap();
        assert!(manifest.exports.len() >= 7);
        let dir = std::env::temp_dir().join(format!("splice9000-presets-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let clip = dir.join("in.mp4");
        let ok = Command::new(&tools.ffmpeg)
            .args(["-v", "error", "-y", "-f", "lavfi", "-i", "testsrc2=s=320x180:r=30:d=1", "-f", "lavfi", "-i", "sine=f=440:d=1", "-shortest"])
            .args(["-c:v", "libx264", "-preset", "ultrafast", "-pix_fmt", "yuv420p"])
            .arg(&clip)
            .status()
            .unwrap()
            .success();
        assert!(ok);
        let m = vec![tools.probe(&clip).unwrap()];
        let t = Timeline::new(vec![Clip { media: 0, src_in: 0.0, src_out: 0.5, fade: 0.0 }, Clip { media: 0, src_in: 0.5, src_out: 1.0, fade: 0.0 }]);
        let s = Settings::default();
        let have = tools.encoders();
        assert!(have.iter().any(|e| e == "libx264"), "the encoder list couldn't be read");
        // Some ffmpeg builds leave these out; splice9000 says so instead of
        // exporting. Anything else missing is a mistake in the preset.
        const OPTIONAL: [&str; 4] = ["libvpx-vp9", "libopus", "libx265", "libsvtav1"];
        for p in &manifest.exports {
            let missing: Vec<String> = encoders_needed(&s, Some(p)).into_iter().filter(|e| !have.contains(e)).collect();
            if !missing.is_empty() {
                assert!(missing.iter().all(|e| OPTIONAL.contains(&e.as_str())), "{}: no encoder {missing:?}", p.label);
                eprintln!("{}: skipped, this ffmpeg has no {}", p.label, missing.join(", "));
                continue;
            }
            let out = dir.join(format!("{}.{}", p.id, p.extension));
            let fmt = Format::of(&t, &m, &s).with(Some(p));
            let job = Job::start(&tools, args(&t, &m, fmt, &s, Some(p), &out), out.clone(), t.duration(), || {});
            let result = loop {
                if let Some(r) = job.progress.lock().unwrap().finished.take() {
                    break r;
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            };
            result.unwrap_or_else(|e| panic!("{}: {e}", p.label));
            // What came out has the tracks and size the preset promises.
            let probe = Command::new(&tools.ffprobe)
                .args(["-v", "error", "-of", "json", "-show_entries", "stream=codec_type,width,height"])
                .arg(&out)
                .output()
                .unwrap();
            let v: serde_json::Value = serde_json::from_slice(&probe.stdout).unwrap();
            let streams = v["streams"].as_array().unwrap();
            let has = |kind: &str| streams.iter().any(|s| s["codec_type"] == kind);
            assert_eq!(has("video"), !p.no_video, "{}: {v}", p.label);
            assert_eq!(has("audio"), !p.no_audio, "{}: {v}", p.label);
            if p.width > 0
                && let Some(video) = streams.iter().find(|s| s["codec_type"] == "video")
            {
                assert_eq!((video["width"].as_u64(), video["height"].as_u64()), (Some(p.width as u64), Some(p.height as u64)), "{}", p.label);
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}

