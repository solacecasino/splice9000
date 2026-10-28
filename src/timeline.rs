//! The edit: one track of clips played back to back.
//!
//! Each clip is a stretch (`src_in`..`src_out`, in seconds) of a media file.
//! Where each clip starts on the timeline is kept in `starts`, a running
//! total, so finding the clip under any time is a binary search and an edit
//! only redoes the totals from the edited clip on.
//!
//! A clip can fade in from the one before it (a crossfade). The fade is
//! centered on the cut and borrows footage from past the clips' ends, so
//! it never changes where anything is on the timeline; where a clip has no
//! more footage, its last (or first) picture holds.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Clip {
    /// Index into the project's media list.
    pub media: usize,
    pub src_in: f64,
    pub src_out: f64,
    /// Seconds of crossfade from the clip before this one. Files from
    /// before crossfades simply have none.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub fade: f64,
}

fn is_zero(x: &f64) -> bool {
    *x == 0.0
}

/// A stretch of the edit as it's played or exported: one clip's footage,
/// or a crossfade between two.
#[derive(Clone, Debug, PartialEq)]
pub struct Piece {
    pub clip: usize,
    pub src_in: f64,
    pub src_out: f64,
    /// Where the piece starts on the timeline.
    pub at: f64,
    pub mix: Option<Mix>,
}

impl Piece {
    pub fn len(&self) -> f64 {
        self.src_out - self.src_in
    }
}

/// A crossfade from clip `piece.clip` into clip `into`. The outgoing clip
/// plays `piece.src_in..` for `piece.len()`, of which only `a_has` seconds
/// exist (its last picture holds for the rest); the incoming one starts at
/// `b_in` after `b_wait` seconds of holding its first picture.
#[derive(Clone, Debug, PartialEq)]
pub struct Mix {
    pub into: usize,
    pub a_has: f64,
    pub b_in: f64,
    pub b_wait: f64,
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

    /// The cut (or the start or end of the edit) nearest `t`.
    pub fn nearest_cut(&self, t: f64) -> f64 {
        let i = self.starts.partition_point(|&s| s < t);
        let after = self.starts.get(i).copied().unwrap_or(self.duration());
        let before = if i > 0 { self.starts[i - 1] } else { after };
        if t - before <= after - t { before } else { after }
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
        let right = Clip { media: clip.media, src_in: src, src_out: clip.src_out, fade: 0.0 };
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

    /// How long the crossfade into clip `i` really is: as asked, but at most
    /// half of either clip, so two fades never meet.
    pub fn fade(&self, i: usize) -> f64 {
        if i == 0 || i >= self.clips.len() {
            return 0.0;
        }
        let half = self.clips[i - 1].len().min(self.clips[i].len()) * 0.5;
        self.clips[i].fade.clamp(0.0, half)
    }

    /// The edit from clip `from` on as pieces to play or export, in order.
    /// `length(media)` is how long a media file is.
    pub fn pieces(&self, from: usize, length: impl Fn(usize) -> f64) -> Vec<Piece> {
        let mut out = Vec::with_capacity((self.clips.len() - from.min(self.clips.len())) * 2);
        for i in from..self.clips.len() {
            let c = &self.clips[i];
            let (d_in, d_out) = (self.fade(i), self.fade(i + 1));
            if d_in > 0.0 && i > from {
                let a = &self.clips[i - 1];
                let half = d_in * 0.5;
                out.push(Piece {
                    clip: i - 1,
                    src_in: a.src_out - half,
                    src_out: a.src_out + half,
                    at: self.starts[i] - half,
                    mix: Some(Mix {
                        into: i,
                        a_has: (length(a.media) - (a.src_out - half)).clamp(0.0, d_in),
                        b_in: (c.src_in - half).max(0.0),
                        b_wait: (half - c.src_in).max(0.0),
                    }),
                });
            }
            // A piece starting inside a fade begins after it: the caller
            // plays the fade from the piece before.
            let skip = if i > from { d_in * 0.5 } else { 0.0 };
            let piece = Piece { clip: i, src_in: c.src_in + skip, src_out: c.src_out - d_out * 0.5, at: self.starts[i] + skip, mix: None };
            if piece.len() > 1e-9 {
                out.push(piece);
            }
        }
        out
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
        Clip { media, src_in: a, src_out: b, fade: 0.0 }
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
        assert_eq!([-1.0, 0.9, 1.1, 4.4, 5.6, 99.0].map(|x| t.nearest_cut(x)), [0.0, 0.0, 2.0, 5.0, 6.0, 6.0]);
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

    #[test]
    fn crossfades_borrow_footage_past_the_cut() {
        let mut t = three();
        // Into clip 1 (5..8 of media 1, 3 s), from clip 0 (0..2 of media 0).
        t.clips[1].fade = 1.0;
        t.clips[2].fade = 4.0; // more than half a clip: cut down to 0.5
        assert_eq!((t.fade(1), t.fade(2)), (1.0, 0.5));
        let len = |m: usize| [10.0, 20.0][m];
        let p = t.pieces(0, len);
        let spans: Vec<_> = p.iter().map(|p| (p.clip, p.src_in, p.src_out, p.at, p.mix.is_some())).collect();
        assert_eq!(
            spans,
            [
                (0, 0.0, 1.5, 0.0, false),
                (0, 1.5, 2.5, 1.5, true),
                (1, 5.5, 7.75, 2.5, false),
                (1, 7.75, 8.25, 4.75, true),
                (2, 10.25, 11.0, 5.25, false),
            ]
        );
        // The pieces fill the edit exactly.
        assert!((p.iter().map(Piece::len).sum::<f64>() - t.duration()).abs() < 1e-9);
        assert_eq!(p[1].mix, Some(Mix { into: 1, a_has: 1.0, b_in: 4.5, b_wait: 0.0 }));
        // Whole files back to back: each side has half the footage it
        // needs, and the pictures at the cut hold for the other half.
        let mut u = Timeline::new(vec![clip(0, 0.0, 10.0), clip(1, 0.0, 4.0)]);
        u.clips[1].fade = 2.0;
        let mix = u.pieces(0, len)[1].mix.clone().unwrap();
        assert_eq!((mix.a_has, mix.b_in, mix.b_wait), (1.0, 0.0, 1.0));
        // Starting from a clip: its own fade in isn't played.
        assert_eq!(t.pieces(1, len)[0], Piece { clip: 1, src_in: 5.0, src_out: 7.75, at: 2.0, mix: None });
        let f: Clip = serde_json::from_str(r#"{"media":0,"src_in":1,"src_out":2}"#).unwrap();
        assert_eq!(f.fade, 0.0);
        assert!(!serde_json::to_string(&f).unwrap().contains("fade"));
    }
}
