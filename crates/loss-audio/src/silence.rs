//! Skip silence (M2-14): long silent stretches inside a song are cut down to a short natural gap.
//!
//! A frame is silent when every channel is under the threshold. A run of silence is left alone
//! if it ends within `min` frames; once it passes `min`, everything beyond the first `keep`
//! frames is dropped until sound returns. Streaming and exact: no frame is ever held back for
//! longer than the `min` frames needed to know how long the run turns out to be.
//!
//! The splice joins two stretches that are both under the threshold, so the step at the join
//! is at most the threshold (−55 dBFS by default): inaudible, and no fade is needed.

pub struct Skipper {
    ch: usize,
    thr: f64,
    /// A run this long (frames) is skipped.
    min: usize,
    /// How much of a skipped run still plays (frames).
    keep: usize,
    /// Silent frames in the current run.
    run: usize,
    /// Silent frames past `keep`, not yet known to be worth dropping (interleaved).
    held: Vec<f64>,
    dropping: bool,
}

impl Skipper {
    /// `threshold_db` is the level under which a frame is silent (−55 is quiet room noise);
    /// runs of `min_ms` or more are cut to `keep_ms`.
    pub fn new(
        threshold_db: f64,
        min_ms: f64,
        keep_ms: f64,
        rate: u32,
        channels: usize,
    ) -> Skipper {
        let frames = |ms: f64| ((ms / 1000.0) * rate as f64).round() as usize;
        let keep = frames(keep_ms);
        Skipper {
            ch: channels.max(1),
            thr: 10f64.powf(threshold_db / 20.0),
            min: frames(min_ms).max(keep + 1),
            keep,
            run: 0,
            held: Vec::new(),
            dropping: false,
        }
    }

    /// The defaults: −55 dBFS, runs of 0.8 s cut to 0.25 s.
    pub fn standard(rate: u32, channels: usize) -> Skipper {
        Skipper::new(-55.0, 800.0, 250.0, rate, channels)
    }

    /// Filters interleaved `input` into `out`; returns the frames dropped.
    pub fn process(&mut self, input: &[f64], out: &mut Vec<f64>) -> usize {
        let ch = self.ch;
        let mut dropped = 0;
        for frame in input.chunks(ch) {
            if frame.len() < ch {
                break;
            }
            let silent = frame.iter().all(|v| v.abs() < self.thr);
            if !silent {
                if self.dropping {
                    self.dropping = false;
                } else if !self.held.is_empty() {
                    // The run ended before it got long: it plays as it was.
                    out.append(&mut self.held);
                }
                self.run = 0;
                out.extend_from_slice(frame);
                continue;
            }
            self.run += 1;
            if self.dropping {
                dropped += 1;
            } else if self.run <= self.keep {
                out.extend_from_slice(frame);
            } else {
                self.held.extend_from_slice(frame);
                if self.run >= self.min {
                    dropped += self.held.len() / ch;
                    self.held.clear();
                    self.dropping = true;
                }
            }
        }
        dropped
    }

    /// The end of the stream: a short silent tail plays as it was.
    pub fn flush(&mut self, out: &mut Vec<f64>) {
        out.append(&mut self.held);
        self.run = 0;
        self.dropping = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 44_100;

    fn tone(secs: f64) -> Vec<f64> {
        (0..(RATE as f64 * secs) as usize)
            .map(|i| 0.5 * (i as f64 * 0.05 + 1.0).sin())
            .collect()
    }

    fn silence(secs: f64) -> Vec<f64> {
        // Quiet noise under the threshold, not digital zero.
        (0..(RATE as f64 * secs) as usize)
            .map(|i| 0.0005 * ((i * 7 % 13) as f64 / 6.0 - 1.0))
            .collect()
    }

    fn run(input: &[f64], chunk: usize) -> (Vec<f64>, usize) {
        let mut s = Skipper::standard(RATE, 1);
        let mut out = Vec::new();
        let mut dropped = 0;
        for c in input.chunks(chunk) {
            dropped += s.process(c, &mut out);
        }
        s.flush(&mut out);
        (out, dropped)
    }

    #[test]
    fn a_long_gap_is_cut_to_the_kept_part_and_the_rest_is_untouched() {
        let mut x = tone(1.0);
        x.extend(silence(3.0));
        x.extend(tone(1.0));
        let (y, dropped) = run(&x, 4410);
        let keep = (0.25 * RATE as f64).round() as usize;
        // (A tone sample right at a zero crossing counts as silent too: a frame either way.)
        let expected = silence(3.0).len() - keep;
        assert!(dropped.abs_diff(expected) <= 2, "{dropped} vs {expected}");
        assert_eq!(y.len() + dropped, x.len());
        // Sound before and after the gap is bit-identical (all but the frames at the very edges).
        let n = tone(1.0).len();
        assert_eq!(y[..n - 8], x[..n - 8]);
        assert_eq!(y[y.len() - (n - 8)..], x[x.len() - (n - 8)..]);
        // The splice joins quiet to quiet, then the sound comes in exactly as it did in the
        // original: the step from the last played silence to the first sound is the same.
        let at = n + keep;
        let onset = x.len() - n;
        let original_step = (x[onset] - x[onset - 1]).abs();
        let step = (y[at] - y[at - 1]).abs();
        assert!(
            (step - original_step).abs() < 0.002,
            "{step} vs {original_step}"
        );
        // Chunking doesn't matter.
        assert_eq!(run(&x, 777).0, y);
    }

    #[test]
    fn short_gaps_and_a_short_tail_play_as_they_were() {
        let mut x = tone(0.5);
        x.extend(silence(0.7)); // under 0.8 s: untouched
        x.extend(tone(0.5));
        x.extend(silence(0.5)); // a short tail
        let (y, dropped) = run(&x, 2048);
        assert_eq!(dropped, 0);
        assert_eq!(y, x);
        // A long tail is cut too.
        let mut z = tone(0.5);
        z.extend(silence(5.0));
        let (y, dropped) = run(&z, 4096);
        assert!(dropped > 4 * RATE as usize);
        assert_eq!(y.len() + dropped, z.len());
    }

    #[test]
    fn stereo_needs_both_channels_quiet() {
        let mut s = Skipper::standard(RATE, 2);
        // A left channel that's loud and a right that's silent is not silence.
        let x: Vec<f64> = (0..RATE as usize * 3)
            .flat_map(|i| [0.3 * (i as f64 * 0.05).sin(), 0.0])
            .collect();
        let mut out = Vec::new();
        assert_eq!(s.process(&x, &mut out), 0);
        assert_eq!(out, x);
    }
}
