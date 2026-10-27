//! The edit: one track of clips played back to back.
//!
//! Each clip is a stretch (`src_in`..`src_out`, in seconds) of a media file.
//! Where each clip starts on the timeline is kept in `starts`, a running
//! total, so finding the clip under any time is a binary search and an edit
//! only redoes the totals from the edited clip on.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Clip {
    /// Index into the project's media list.
    pub media: usize,
    pub src_in: f64,
    pub src_out: f64,
}

impl Clip {
    pub fn len(&self) -> f64 {
        self.src_out - self.src_in
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Timeline {
    clips: Vec<Clip>,
    /// `starts[i]` is where clip `i` begins; the last entry is the length of
    /// the whole edit, so there's always one more entry than clips.
    starts: Vec<f64>,
}

impl Default for Timeline {
    /// Empty, with its one running total (0) in place.
    fn default() -> Self {
        Self::new(Vec::new())
    }
}

impl Timeline {
    pub fn new(clips: Vec<Clip>) -> Self {
        let mut t = Self { clips, starts: Vec::new() };
        t.restack(0);
        t
    }

    pub fn clips(&self) -> &[Clip] {
        &self.clips
    }

    pub fn len(&self) -> usize {
        self.clips.len()
    }

    pub fn is_empty(&self) -> bool {
        self.clips.is_empty()
    }

    pub fn duration(&self) -> f64 {
        self.starts.last().copied().unwrap_or(0.0)
    }

    /// Where clip `i` begins (`i == len()`: the end of the edit).
    pub fn start(&self, i: usize) -> f64 {
        self.starts[i]
    }

    /// The clip playing at `t`, and the time in its source file.
    pub fn locate(&self, t: f64) -> Option<(usize, f64)> {
        if t < 0.0 || t >= self.duration() {
            return None;
        }
        let i = self.starts.partition_point(|&s| s <= t) - 1;
        Some((i, self.clips[i].src_in + (t - self.starts[i])))
    }

    /// The first clip that ends after `t`: where drawing a view from `t` begins.
    pub fn first_after(&self, t: f64) -> usize {
        self.starts[1..].partition_point(|&end| end <= t)
    }

    /// Where a clip dropped at `t` goes: before the first clip whose middle
    /// is after `t`.
    pub fn slot_at(&self, t: f64) -> usize {
        let mut lo = 0;
        let mut hi = self.clips.len();
        while lo < hi {
            let mid = (lo + hi) / 2;
            if (self.starts[mid] + self.starts[mid + 1]) * 0.5 < t {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        lo
    }

    pub fn insert(&mut self, i: usize, clip: Clip) {
        let i = i.min(self.clips.len());
        self.clips.insert(i, clip);
        self.restack(i);
    }

    pub fn remove(&mut self, i: usize) -> Clip {
        let clip = self.clips.remove(i);
        self.restack(i);
        clip
    }

    /// Moves clip `from` so it ends up before the clip now at `to` (as
    /// counted before the move; `to == len()` puts it last).
    pub fn move_clip(&mut self, from: usize, to: usize) {
        let to = if to > from { to - 1 } else { to };
        if from == to {
            return;
        }
        let clip = self.clips.remove(from);
        self.clips.insert(to, clip);
        self.restack(from.min(to));
    }

    /// Cuts the clip under `t` in two at `t`. Nothing happens when `t` is
    /// within `min` seconds of a cut already there.
    pub fn split(&mut self, t: f64, min: f64) -> bool {
        let Some((i, src)) = self.locate(t) else { return false };
        let clip = &self.clips[i];
        if src - clip.src_in < min || clip.src_out - src < min {
            return false;
        }
        let right = Clip { media: clip.media, src_in: src, src_out: clip.src_out };
        self.clips[i].src_out = src;
        self.clips.insert(i + 1, right);
        self.restack(i);
        true
    }

    /// Sets where clip `i` starts in its source, kept at least `min` seconds
    /// long and not before the source's start.
    pub fn set_in(&mut self, i: usize, src_in: f64, min: f64) {
        let c = &mut self.clips[i];
        c.src_in = src_in.clamp(0.0, (c.src_out - min).max(0.0));
        self.restack(i);
    }

    /// Sets where clip `i` ends in its source, kept at least `min` seconds
    /// long and not past the source's `end`.
    pub fn set_out(&mut self, i: usize, src_out: f64, end: f64, min: f64) {
        let c = &mut self.clips[i];
        c.src_out = src_out.clamp((c.src_in + min).min(end), end);
        self.restack(i);
    }

    /// Redoes the running totals from clip `from` on.
    fn restack(&mut self, from: usize) {
        self.starts.truncate(from + 1);
        if self.starts.is_empty() {
            self.starts.push(0.0);
        }
        let mut at = self.starts[from];
        for clip in &self.clips[from..] {
            at += clip.len();
            self.starts.push(at);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clip(media: usize, a: f64, b: f64) -> Clip {
        Clip { media, src_in: a, src_out: b }
    }

    fn three() -> Timeline {
        Timeline::new(vec![clip(0, 0.0, 2.0), clip(1, 5.0, 8.0), clip(0, 10.0, 11.0)])
    }

    #[test]
    fn locates_clips() {
        let t = three();
        assert_eq!(t.duration(), 6.0);
        assert_eq!(t.locate(0.0), Some((0, 0.0)));
        assert_eq!(t.locate(2.0), Some((1, 5.0)));
        assert_eq!(t.locate(4.5), Some((1, 7.5)));
        assert_eq!(t.locate(5.5), Some((2, 10.5)));
        assert_eq!(t.locate(6.0), None);
        assert_eq!(t.locate(-1.0), None);
        let empty = Timeline::default();
        assert_eq!((empty.locate(0.0), empty.first_after(0.0), empty.slot_at(3.0), empty.duration()), (None, 0, 0, 0.0));
    }

    #[test]
    fn finds_first_visible_and_drop_slot() {
        let t = three();
        assert_eq!(t.first_after(0.0), 0);
        assert_eq!(t.first_after(2.0), 1);
        assert_eq!(t.first_after(5.9), 2);
        assert_eq!(t.first_after(9.0), 3);
        assert_eq!(t.slot_at(0.5), 0);
        assert_eq!(t.slot_at(1.5), 1);
        assert_eq!(t.slot_at(3.0), 1);
        assert_eq!(t.slot_at(4.0), 2);
        assert_eq!(t.slot_at(100.0), 3);
    }

    #[test]
    fn splits_and_keeps_length() {
        let mut t = three();
        assert!(t.split(3.0, 0.01));
        assert_eq!(t.len(), 4);
        assert_eq!(t.clips()[1], clip(1, 5.0, 6.0));
        assert_eq!(t.clips()[2], clip(1, 6.0, 8.0));
        assert_eq!(t.duration(), 6.0);
        // On an existing cut: nothing.
        assert!(!t.split(3.0, 0.01));
        assert!(!t.split(2.0, 0.01));
    }

    #[test]
    fn moves_and_removes() {
        let mut t = three();
        t.move_clip(0, 3);
        assert_eq!(t.clips().iter().map(|c| c.src_in).collect::<Vec<_>>(), [5.0, 10.0, 0.0]);
        assert_eq!(t.start(2), 4.0);
        t.move_clip(2, 0);
        assert_eq!(t, three());
        t.move_clip(1, 2);
        assert_eq!(t, three());
        t.remove(1);
        assert_eq!(t.duration(), 3.0);
        assert_eq!(t.locate(2.5), Some((1, 10.5)));
    }

    #[test]
    fn trims_within_the_source() {
        let mut t = three();
        t.set_in(1, -4.0, 0.1);
        assert_eq!(t.clips()[1].src_in, 0.0);
        t.set_in(1, 9.0, 0.1);
        assert!((t.clips()[1].src_in - 7.9).abs() < 1e-9);
        t.set_out(0, 50.0, 20.0, 0.1);
        assert_eq!(t.clips()[0].src_out, 20.0);
        t.set_out(0, -1.0, 20.0, 0.1);
        assert!((t.clips()[0].src_out - 0.1).abs() < 1e-9);
        assert!((t.duration() - (0.1 + 0.1 + 1.0)).abs() < 1e-9);
    }
}
