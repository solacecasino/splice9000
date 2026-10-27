//! Sound while playing, and the clock the picture follows.
//!
//! ffmpeg decodes each clip's audio at the sound device's own rate and
//! channel count, so nothing is resampled here. The samples go through a
//! lock-free ring to the device's callback, which counts the frames it
//! actually plays. That count is the playback clock: the picture waits for
//! the sound, so the two can't drift apart, and if the sound stalls the
//! picture waits too. The device is opened on first play and paused
//! whenever nothing plays, so an idle editor costs nothing.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

/// One producer, one consumer, no locks: the device callback must never wait.
pub struct Ring {
    buf: Box<[AtomicU32]>,
    /// Samples ever written and read; the difference is what's waiting.
    written: AtomicUsize,
    read: AtomicUsize,
}

impl Ring {
    pub fn new(len: usize) -> Self {
        Self { buf: (0..len).map(|_| AtomicU32::new(0)).collect(), written: AtomicUsize::new(0), read: AtomicUsize::new(0) }
    }

    /// Writes as much of `samples` as fits; returns how much did.
    pub fn push(&self, samples: &[f32]) -> usize {
        let w = self.written.load(Ordering::Relaxed);
        let free = self.buf.len() - (w - self.read.load(Ordering::Acquire));
        let n = samples.len().min(free);
        for (i, s) in samples[..n].iter().enumerate() {
            self.buf[(w + i) % self.buf.len()].store(s.to_bits(), Ordering::Relaxed);
        }
        self.written.store(w + n, Ordering::Release);
        n
    }

    /// Fills `out` from the ring; returns how many samples were there.
    pub fn pop(&self, out: &mut [f32], gain: f32) -> usize {
        let r = self.read.load(Ordering::Relaxed);
        let n = out.len().min(self.written.load(Ordering::Acquire) - r);
        for (i, o) in out[..n].iter_mut().enumerate() {
            *o = f32::from_bits(self.buf[(r + i) % self.buf.len()].load(Ordering::Relaxed)) * gain;
        }
        self.read.store(r + n, Ordering::Release);
        n
    }

    /// Drops whatever is waiting. Only while nothing pushes or pops.
    pub fn clear(&self) {
        self.read.store(self.written.load(Ordering::Acquire), Ordering::Release);
    }
}

/// What the preview worker and the device callback share.
pub struct Shared {
    pub ring: Ring,
    pub rate: u32,
    pub channels: u16,
    /// Frames (samples per channel) played since playback began.
    played: AtomicU64,
    /// The callback plays from the ring only while this is set.
    pub active: AtomicBool,
    /// The volume, as f32 bits.
    gain: AtomicU32,
}

impl Shared {
    /// How much has been heard since playback began.
    pub fn clock(&self) -> Duration {
        Duration::from_secs_f64(self.played.load(Ordering::Acquire) as f64 / self.rate as f64)
    }

    /// Readies a new playback. Only while the callback isn't playing.
    pub fn reset(&self) {
        self.ring.clear();
        self.played.store(0, Ordering::Release);
    }

    pub fn set_gain(&self, gain: f32) {
        self.gain.store(gain.clamp(0.0, 4.0).to_bits(), Ordering::Relaxed);
    }
}

pub struct Output {
    stream: cpal::Stream,
    pub shared: Arc<Shared>,
}

impl Output {
    /// Opens the system's default sound output.
    pub fn open() -> Result<Self, String> {
        let device = cpal::default_host().default_output_device().ok_or("no sound output")?;
        let supported = device.default_output_config().map_err(|e| e.to_string())?;
        let config = supported.config();
        let (rate, channels) = (config.sample_rate, config.channels);
        // A quarter second of sound waiting at most: enough to ride out a
        // busy moment, little enough that stopping is immediate.
        let shared = Arc::new(Shared {
            ring: Ring::new(rate as usize * channels as usize / 4),
            rate,
            channels,
            played: AtomicU64::new(0),
            active: AtomicBool::new(false),
            gain: AtomicU32::new(1.0f32.to_bits()),
        });
        let s = shared.clone();
        let stream = device
            .build_output_stream::<f32, _, _>(
                config,
                move |out: &mut [f32], _| {
                    let got = if s.active.load(Ordering::Acquire) {
                        s.ring.pop(out, f32::from_bits(s.gain.load(Ordering::Relaxed)))
                    } else {
                        0
                    };
                    out[got..].fill(0.0);
                    s.played.fetch_add((got / s.channels as usize) as u64, Ordering::AcqRel);
                },
                |e| crate::log::error(format!("Sound: {e}")),
                None,
            )
            .map_err(|e| e.to_string())?;
        Ok(Self { stream, shared })
    }

    pub fn play(&self) {
        if let Err(e) = self.stream.play() {
            crate::log::error(format!("Sound: {e}"));
        }
    }

    pub fn pause(&self) {
        let _ = self.stream.pause();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_wraps_and_fills_to_capacity() {
        let r = Ring::new(4);
        assert_eq!(r.push(&[1.0, 2.0, 3.0]), 3);
        let mut out = [0.0; 2];
        assert_eq!(r.pop(&mut out, 1.0), 2);
        assert_eq!(out, [1.0, 2.0]);
        assert_eq!(r.push(&[4.0, 5.0, 6.0, 7.0]), 3);
        let mut out = [9.0; 5];
        assert_eq!(r.pop(&mut out, 0.5), 4);
        assert_eq!(&out[..4], &[1.5, 2.0, 2.5, 3.0]);
        r.push(&[1.0]);
        r.clear();
        assert_eq!(r.pop(&mut out, 1.0), 0);
    }
}
