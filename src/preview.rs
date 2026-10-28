//! Pictures for the preview, decoded by ffmpeg on a background thread.
//!
//! The worker always does only the latest thing asked: requests that pile
//! up while it's busy (dragging the playhead) are skipped. Frames come out
//! of ffmpeg already scaled to the preview's size, so a 4K file costs a
//! preview-sized picture, not a 4K one. Finished frames go into a one-frame
//! mailbox; a frame the interface hasn't shown yet is replaced, never queued.
//! While playing, the next clip's ffmpeg is started before the current one
//! ends, so cuts don't stall, and the clock starts with the first picture,
//! so starting to play skips nothing.

use std::io::Read;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use egui::{Color32, ColorImage};

use crate::audio::Shared;
use crate::media::{Tools, fit, quiet};

#[derive(Clone, Debug)]
pub struct Segment {
    pub path: PathBuf,
    pub width: u32,
    pub height: u32,
    /// The file has sound.
    pub audio: bool,
    pub src_in: f64,
    pub src_out: f64,
    /// Where the segment starts on the timeline.
    pub at: f64,
}

pub enum Request {
    Still { seg: Segment, t: f64, bw: u32, bh: u32 },
    Play { segs: Vec<Segment>, from: f64, bw: u32, bh: u32, fps: f64, audio: Option<Arc<Shared>> },
    Stop,
}

pub struct Frame {
    pub image: ColorImage,
    /// The timeline time it shows.
    pub time: f64,
    /// Asked for while stopped, not a frame of playback.
    pub still: bool,
}

#[derive(Default)]
pub struct Mailbox {
    pub frame: Option<Frame>,
    /// Playback reached the end of the edit.
    pub ended: bool,
    pub error: Option<String>,
}

pub struct Preview {
    tx: Sender<Request>,
    pub mailbox: Arc<Mutex<Mailbox>>,
}

impl Preview {
    pub fn new(tools: Option<Tools>, hwaccel: bool, wake: impl Fn() + Send + 'static) -> Self {
        let (tx, rx) = channel();
        let mailbox = Arc::new(Mutex::new(Mailbox::default()));
        let worker = Worker { tools, hwaccel, rx, mailbox: mailbox.clone(), wake: Box::new(wake) };
        let _ = std::thread::Builder::new().name("preview".into()).spawn(move || worker.run());
        Self { tx, mailbox }
    }

    pub fn send(&self, r: Request) {
        let _ = self.tx.send(r);
    }

    pub fn take(&self) -> Mailbox {
        std::mem::take(&mut *self.mailbox.lock().unwrap_or_else(|e| e.into_inner()))
    }
}

struct Worker {
    tools: Option<Tools>,
    hwaccel: bool,
    rx: Receiver<Request>,
    mailbox: Arc<Mutex<Mailbox>>,
    wake: Box<dyn Fn() + Send>,
}

impl Worker {
    fn run(self) {
        let mut next = None;
        loop {
            let mut req = match next.take() {
                Some(r) => r,
                None => match self.rx.recv() {
                    Ok(r) => r,
                    Err(_) => return,
                },
            };
            while let Ok(r) = self.rx.try_recv() {
                req = r;
            }
            match req {
                Request::Still { seg, t, bw, bh } => {
                    let (w, h) = fit(seg.width, seg.height, bw, bh);
                    let mut child = match self.spawn(&seg, t, None, w, h, None, true) {
                        Ok(c) => c,
                        Err(e) => {
                            self.post(|m| m.error = Some(e));
                            continue;
                        }
                    };
                    let image = read_frame(&mut child, w, h);
                    let _ = child.wait();
                    match image {
                        Some(image) => self.post(|m| m.frame = Some(Frame { image, time: seg.at + (t - seg.src_in), still: true })),
                        None => self.post(|m| m.error = Some(format!("couldn't decode {}", seg.path.display()))),
                    }
                }
                Request::Play { segs, from, bw, bh, fps, audio } => next = self.play(segs, from, bw, bh, fps, audio),
                Request::Stop => {}
            }
        }
    }

    /// Plays from `from` until the end or until another request comes,
    /// which is returned. With `audio`, the sound plays too and its clock
    /// paces the picture; without, the wall clock does.
    #[allow(clippy::too_many_arguments)]
    fn play(&self, segs: Vec<Segment>, from: f64, bw: u32, bh: u32, fps: f64, audio: Option<Arc<Shared>>) -> Option<Request> {
        let frame_len = 1.0 / fps;
        // Each segment starts at `from` or its own start, whichever is later.
        let start_in = |s: &Segment| s.src_in + (from - s.at).max(0.0);
        let feeder = audio.as_ref().and_then(|a| {
            a.active.store(false, Ordering::Release);
            a.reset();
            let tools = self.tools.clone()?;
            let parts = segs.iter().map(|s| (s.path.clone(), s.audio, start_in(s), s.src_out - start_in(s))).collect();
            Some(Feeder::start(tools, a.clone(), parts))
        });
        // The clock starts with the first picture, not the key press: the
        // decoder takes a moment to start, and a clock already running would
        // count its first frames as late and skip them.
        let started = std::cell::Cell::new(None::<Instant>);
        let clock = || match &audio {
            Some(a) => a.clock(),
            None => started.get().map_or(Duration::ZERO, |s| s.elapsed()),
        };
        let finish = |child: Option<Child>, upcoming: Option<(usize, Child)>| {
            child.into_iter().chain(upcoming.map(|u| u.1)).for_each(kill);
            if let Some(f) = &feeder {
                f.stop();
            }
            if let Some(a) = &audio {
                a.active.store(false, Ordering::Release);
            }
        };
        let mut upcoming: Option<(usize, Child)> = None;
        for (i, seg) in segs.iter().enumerate() {
            let (w, h) = fit(seg.width, seg.height, bw, bh);
            let begin = start_in(seg);
            let mut child = match upcoming.take() {
                Some((j, c)) if j == i => c,
                _ => match self.spawn(seg, begin, Some(seg.src_out - begin), w, h, Some(fps), false) {
                    Ok(c) => c,
                    Err(e) => {
                        finish(None, None);
                        self.post(|m| m.error = Some(e));
                        return None;
                    }
                },
            };
            let mut n = 0u64;
            while let Some(image) = read_frame(&mut child, w, h) {
                // The next clip's decoder starts once this one is showing,
                // so the two don't compete for the first picture.
                if n == 0
                    && let Some(next) = segs.get(i + 1)
                {
                    let (nw, nh) = fit(next.width, next.height, bw, bh);
                    let nb = start_in(next);
                    upcoming = self.spawn(next, nb, Some(next.src_out - nb), nw, nh, Some(fps), false).ok().map(|c| (i + 1, c));
                }
                if started.get().is_none() {
                    started.set(Some(Instant::now()));
                    if let Some(a) = &audio {
                        // The sound has been filling the ring meanwhile.
                        a.active.store(true, Ordering::Release);
                    }
                }
                let time = seg.at + (begin - seg.src_in) + n as f64 * frame_len;
                n += 1;
                let due = Duration::from_secs_f64((time - from).max(0.0));
                let now = clock();
                if now > due + Duration::from_secs_f64(frame_len) {
                    // Behind: drop this frame and catch up.
                    continue;
                }
                match self.rx.recv_timeout(due.saturating_sub(now)) {
                    Ok(r) => {
                        finish(Some(child), upcoming);
                        return Some(r);
                    }
                    Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => return None,
                }
                self.post(|m| m.frame = Some(Frame { image, time, still: false }));
            }
            let _ = child.wait();
        }
        // Let the last of the sound play out.
        if let (Some(a), Some(last)) = (&audio, segs.last()) {
            let end = Duration::from_secs_f64((last.at + (last.src_out - last.src_in) - from).max(0.0));
            let deadline = Instant::now() + Duration::from_millis(500);
            while a.clock() < end && Instant::now() < deadline {
                if let Ok(r) = self.rx.recv_timeout(Duration::from_millis(10)) {
                    finish(None, None);
                    return Some(r);
                }
            }
        }
        finish(None, None);
        self.post(|m| m.ended = true);
        None
    }

    #[allow(clippy::too_many_arguments)]
    fn spawn(&self, seg: &Segment, t: f64, dur: Option<f64>, w: u32, h: u32, fps: Option<f64>, one: bool) -> Result<Child, String> {
        let tools = self.tools.as_ref().ok_or("ffmpeg wasn't found")?;
        let mut cmd = quiet(Command::new(&tools.ffmpeg));
        cmd.args(["-hide_banner", "-loglevel", "error", "-nostdin"]);
        // The system decoder pays off over many frames; for one frame its
        // start-up costs more than it saves (measured: 260 ms against 95 ms
        // for a 1080p H.264 frame), so stills always decode in software.
        if self.hwaccel && !one {
            cmd.args(["-hwaccel", "auto"]);
        }
        // Seeking before -i jumps to the nearest keyframe and decodes forward
        // to the exact time: fast and frame-accurate.
        cmd.arg("-ss").arg(format!("{t:.6}"));
        if let Some(d) = dur {
            cmd.arg("-t").arg(format!("{:.6}", d.max(0.0)));
        }
        cmd.arg("-i").arg(&seg.path).args(["-an", "-sn", "-dn"]);
        // Dropping frames before scaling means scaling only the ones shown.
        let mut vf = String::new();
        if let Some(f) = fps {
            vf.push_str(&format!("fps={f:.6},"));
        }
        vf.push_str(&format!("scale={w}:{h}:flags=fast_bilinear"));
        cmd.arg("-vf").arg(vf);
        if one {
            cmd.args(["-frames:v", "1"]);
        }
        cmd.args(["-pix_fmt", "rgba", "-f", "rawvideo", "-"]);
        cmd.stdout(Stdio::piped()).stderr(Stdio::null());
        cmd.spawn().map_err(|e| format!("couldn't run ffmpeg: {e}"))
    }

    fn post(&self, f: impl FnOnce(&mut Mailbox)) {
        f(&mut self.mailbox.lock().unwrap_or_else(|e| e.into_inner()));
        (self.wake)();
    }
}

/// Reads one `w` x `h` RGBA frame straight into the picture's own pixels.
fn read_frame(child: &mut Child, w: u32, h: u32) -> Option<ColorImage> {
    let out = child.stdout.as_mut()?;
    let n = w as usize * h as usize;
    // Not cleared first: every byte is about to be read in.
    let mut pixels: Vec<Color32> = Vec::with_capacity(n);
    // SAFETY: Color32 is four bytes, r g b a, with no padding, and any bytes
    // are a valid Color32; ffmpeg's frames are opaque, so they're the same
    // premultiplied or not. The length is set only once all n are read.
    unsafe {
        let bytes = std::slice::from_raw_parts_mut(pixels.as_mut_ptr().cast::<u8>(), n * 4);
        out.read_exact(bytes).ok()?;
        pixels.set_len(n);
    }
    Some(ColorImage::new([w as usize, h as usize], pixels))
}

fn kill(mut child: Child) {
    let _ = child.kill();
    let _ = child.wait();
}

/// Feeds each segment's sound into the ring on its own thread: exactly as
/// many frames as the segment is long, padded with silence where a file
/// has no sound or ends early, so every cut stays in sync.
struct Feeder {
    stop: Arc<AtomicBool>,
    thread: Mutex<Option<std::thread::JoinHandle<()>>>,
}

impl Feeder {
    /// `parts`: path, has sound, start, length.
    fn start(tools: Tools, out: Arc<Shared>, parts: Vec<(PathBuf, bool, f64, f64)>) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let thread = std::thread::Builder::new().name("sound".into()).spawn(move || {
            let spawn = |p: &(PathBuf, bool, f64, f64)| -> Option<Child> {
                if !p.1 {
                    return None;
                }
                let mut cmd = quiet(Command::new(&tools.ffmpeg));
                cmd.args(["-hide_banner", "-loglevel", "error", "-nostdin"]);
                cmd.arg("-ss").arg(format!("{:.6}", p.2)).arg("-t").arg(format!("{:.6}", p.3.max(0.0)));
                cmd.arg("-i").arg(&p.0).args(["-vn", "-sn", "-dn", "-f", "f32le"]);
                cmd.arg("-ac").arg(out.channels.to_string()).arg("-ar").arg(out.rate.to_string()).arg("-");
                cmd.stdout(Stdio::piped()).stderr(Stdio::null()).spawn().ok()
            };
            let ch = out.channels as usize;
            let mut upcoming = parts.first().and_then(spawn);
            for (i, part) in parts.iter().enumerate() {
                let mut child = upcoming.take();
                upcoming = parts.get(i + 1).and_then(spawn);
                let mut left = (part.3.max(0.0) * out.rate as f64).round() as usize * ch;
                let mut bytes = vec![0u8; 16 * 1024];
                let mut have = 0;
                let mut samples = Vec::with_capacity(bytes.len() / 4);
                while left > 0 && !flag.load(Ordering::Relaxed) {
                    let read = child.as_mut().and_then(|c| c.stdout.as_mut()).map_or(0, |o| o.read(&mut bytes[have..]).unwrap_or(0));
                    samples.clear();
                    if read == 0 {
                        // No sound (left): silence, a block at a time.
                        samples.resize(left.min(4096), 0.0);
                    } else {
                        have += read;
                        let whole = have / 4 * 4;
                        samples.extend(bytes[..whole].as_chunks::<4>().0.iter().map(|b| f32::from_le_bytes(*b)));
                        bytes.copy_within(whole..have, 0);
                        have -= whole;
                        samples.truncate(left);
                    }
                    left -= samples.len();
                    let mut at = 0;
                    while at < samples.len() && !flag.load(Ordering::Relaxed) {
                        at += out.ring.push(&samples[at..]);
                        if at < samples.len() {
                            // Full: the sound starts with the first picture.
                            std::thread::sleep(Duration::from_millis(5));
                        }
                    }
                }
                if let Some(c) = child {
                    kill(c);
                }
            }
            if let Some(c) = upcoming {
                kill(c);
            }
        });
        Self { stop, thread: Mutex::new(thread.ok()) }
    }

    /// Stops feeding and waits until the thread is done with the ring.
    fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.lock().unwrap_or_else(|e| e.into_inner()).take() {
            let _ = t.join();
        }
    }
}
