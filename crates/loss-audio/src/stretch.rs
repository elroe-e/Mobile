//! Time-stretching without changing pitch (M2-13): WSOLA, the waveform-similarity overlap-add.
//!
//! The input is cut into overlapping, Hann-windowed segments that are laid down again at a
//! different spacing. Each segment is nudged (within ±`tol`) to where its start best continues
//! the previous one, found by cross-correlation, so periodic sounds stay in phase. `tempo` is
//! the speed-up: 1.5 makes 3 s of input into 2 s of output.
//!
//! Streaming: `process` takes input as it comes and returns what's ready; `flush` ends the
//! stream. Output length is exact (`input / tempo`, rounded).

use std::f64::consts::PI;

pub struct Wsola {
    ch: usize,
    /// Half a window, in frames (the output hop).
    hs: usize,
    /// Search range either side of the ideal start, in frames.
    tol: usize,
    tempo: f64,
    /// Hann window of length 2·hs; shifted copies by hs add up to 1.
    win: Vec<f64>,
    /// Interleaved input not yet dropped; `base` is the frame number of its first frame.
    input: Vec<f64>,
    base: usize,
    /// Frame (absolute) where the last segment started; None before the first.
    last: Option<usize>,
    /// Segments laid down so far.
    k: u64,
    /// The windowed second half of the last segment, waiting to be overlapped.
    tail: Vec<f64>,
    /// Input and output frames, for the exact length.
    frames_in: u64,
    frames_out: u64,
}

impl Wsola {
    /// `rate` sets the window (about 24 ms) and the search range (about 10 ms).
    pub fn new(tempo: f64, rate: u32, channels: usize) -> Wsola {
        let hs = ((rate as f64 * 0.012).round() as usize).max(64);
        let win = (0..2 * hs)
            .map(|i| 0.5 - 0.5 * (2.0 * PI * i as f64 / (2 * hs) as f64).cos())
            .collect();
        Wsola {
            ch: channels.max(1),
            hs,
            tol: ((rate as f64 * 0.010).round() as usize).max(32),
            tempo: tempo.clamp(0.25, 4.0),
            win,
            input: Vec::new(),
            base: 0,
            last: None,
            k: 0,
            tail: vec![0.0; hs * channels.max(1)],
            frames_in: 0,
            frames_out: 0,
        }
    }

    pub fn tempo(&self) -> f64 {
        self.tempo
    }

    fn frames_buffered(&self) -> usize {
        self.input.len() / self.ch
    }

    /// Sample of the mono mix at absolute frame `f`.
    fn mono(&self, f: usize) -> f64 {
        let i = (f - self.base) * self.ch;
        self.input[i..i + self.ch].iter().sum::<f64>() / self.ch as f64
    }

    /// Where the next segment should start: the ideal spot, nudged to continue the last one.
    fn choose(&self) -> usize {
        let ideal = (self.k as f64 * self.hs as f64 * self.tempo).round() as usize;
        let Some(last) = self.last else {
            return 0;
        };
        // What the last segment's second half would have been had it simply carried on.
        let natural = last + self.hs;
        let lo = ideal.saturating_sub(self.tol).max(self.base);
        let hi = ideal + self.tol;
        let (mut best, mut best_score) = (ideal.max(lo), f64::MIN);
        // Every other position and sample: a cheap search; the overlap-add smooths the rest.
        for cand in (lo..=hi).step_by(2) {
            let mut dot = 0.0;
            let mut energy = 1e-9;
            for i in (0..self.hs).step_by(2) {
                let a = self.mono(natural + i);
                let b = self.mono(cand + i);
                dot += a * b;
                energy += b * b;
            }
            // Normalised, so loud stretches don't win just for being loud.
            let score = dot / energy.sqrt();
            if score > best_score {
                best_score = score;
                best = cand;
            }
        }
        best
    }

    /// Frames of input needed in the buffer to lay down the next segment.
    fn needed(&self) -> usize {
        let ideal = (self.k as f64 * self.hs as f64 * self.tempo).round() as usize;
        ideal + self.tol + 2 * self.hs
    }

    /// Appends input (interleaved) and returns the output that is ready.
    pub fn process(&mut self, samples: &[f64], out: &mut Vec<f64>) {
        self.frames_in += (samples.len() / self.ch) as u64;
        self.input.extend_from_slice(samples);
        while self.base + self.frames_buffered() >= self.needed() {
            self.segment(out);
        }
        self.drop_used();
    }

    fn segment(&mut self, out: &mut Vec<f64>) {
        let (hs, ch) = (self.hs, self.ch);
        let start = self.choose();
        let at = (start - self.base) * ch;
        let seg = &self.input[at..at + 2 * hs * ch];
        if self.k == 0 {
            // The very first half goes out as it is: no fade-in.
            out.extend_from_slice(&seg[..hs * ch]);
        } else {
            for i in 0..hs {
                for c in 0..ch {
                    out.push(self.tail[i * ch + c] + seg[i * ch + c] * self.win[i]);
                }
            }
        }
        for i in 0..hs {
            for c in 0..ch {
                self.tail[i * ch + c] = seg[(hs + i) * ch + c] * self.win[hs + i];
            }
        }
        self.frames_out += hs as u64;
        self.last = Some(start);
        self.k += 1;
    }

    /// Input that no later segment can reach is dropped.
    fn drop_used(&mut self) {
        let ideal = (self.k as f64 * self.hs as f64 * self.tempo).round() as usize;
        let keep_from = ideal
            .saturating_sub(self.tol)
            .min(self.last.map(|l| l + self.hs).unwrap_or(0));
        if keep_from > self.base {
            let drop = (keep_from - self.base).min(self.frames_buffered());
            self.input.drain(..drop * self.ch);
            self.base += drop;
        }
    }

    /// Ends the stream: the rest of the input is stretched too, and the output is cut to the
    /// exact length (`input / tempo`).
    pub fn flush(&mut self, out: &mut Vec<f64>) {
        let want = (self.frames_in as f64 / self.tempo).round() as u64;
        // Pad with silence so the last segments can be laid down.
        let pad = (self.needed() + 2 * self.hs).saturating_sub(self.base + self.frames_buffered());
        self.input.extend(std::iter::repeat_n(0.0, pad * self.ch));
        let mut produced = Vec::new();
        while self.frames_out < want + self.hs as u64 {
            let need = self.needed();
            let have = self.base + self.frames_buffered();
            if have < need {
                self.input
                    .extend(std::iter::repeat_n(0.0, (need - have) * self.ch));
            }
            self.segment(&mut produced);
        }
        // `out` got everything up to the last `process`; this call adds the rest.
        let already = self.frames_out - (produced.len() / self.ch) as u64;
        let take = want.saturating_sub(already) as usize;
        produced.truncate(take * self.ch);
        out.extend(produced);
        self.input.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 44_100;

    fn sine(f: f64, secs: f64) -> Vec<f64> {
        (0..(RATE as f64 * secs) as usize)
            .map(|i| 0.5 * (2.0 * PI * f * i as f64 / RATE as f64).sin())
            .collect()
    }

    fn stretch(input: &[f64], tempo: f64, chunk: usize) -> Vec<f64> {
        let mut w = Wsola::new(tempo, RATE, 1);
        let mut out = Vec::new();
        for c in input.chunks(chunk) {
            w.process(c, &mut out);
        }
        w.flush(&mut out);
        out
    }

    /// Frequency of a clean tone from its zero crossings.
    fn frequency(x: &[f64]) -> f64 {
        let mut crossings = Vec::new();
        for i in 1..x.len() {
            if x[i - 1] < 0.0 && x[i] >= 0.0 {
                let frac = -x[i - 1] / (x[i] - x[i - 1]);
                crossings.push(i as f64 - 1.0 + frac);
            }
        }
        let n = crossings.len() - 1;
        n as f64 * RATE as f64 / (crossings[n] - crossings[0])
    }

    #[test]
    fn length_follows_the_tempo_exactly() {
        let x = sine(440.0, 3.0);
        for tempo in [0.5, 0.8, 1.25, 1.5, 2.0] {
            let y = stretch(&x, tempo, 4410);
            let want = x.len() as f64 / tempo;
            assert!(
                ((y.len() as f64) - want).abs() <= 1.0,
                "tempo {tempo}: {} vs {want}",
                y.len()
            );
            // Chunking the input differently changes nothing.
            assert_eq!(stretch(&x, tempo, 1000), y);
        }
    }

    #[test]
    fn pitch_stays_put_within_half_a_percent() {
        for f in [110.0, 440.0, 1760.0] {
            let x = sine(f, 3.0);
            for tempo in [0.6, 1.5, 2.0] {
                let y = stretch(&x, tempo, 4096);
                // Skip the ends; measure the middle.
                let mid = &y[y.len() / 5..y.len() * 4 / 5];
                let got = frequency(mid);
                assert!(
                    (got / f - 1.0).abs() < 0.005,
                    "{f} Hz at tempo {tempo}: measured {got:.2}"
                );
            }
        }
    }

    /// Speed on this machine (run with --release --nocapture): 60 s of stereo at tempo 1.5.
    #[test]
    #[ignore]
    fn how_fast_is_it() {
        let x: Vec<f64> = sine(300.0, 60.0)
            .iter()
            .flat_map(|&v| [v, v * 0.7])
            .collect();
        let started = std::time::Instant::now();
        let mut w = Wsola::new(1.5, RATE, 2);
        let mut out = Vec::new();
        for c in x.chunks(8192) {
            w.process(c, &mut out);
        }
        w.flush(&mut out);
        let took = started.elapsed();
        eprintln!(
            "60 s of stereo stretched in {took:?} ({:.1}% of real time)",
            took.as_secs_f64() / 60.0 * 100.0
        );
    }

    #[test]
    fn level_and_stereo_survive() {
        let x = sine(330.0, 2.0);
        let y = stretch(&x, 1.3, 2048);
        let rms = |v: &[f64]| (v.iter().map(|s| s * s).sum::<f64>() / v.len() as f64).sqrt();
        let mid = &y[y.len() / 5..y.len() * 4 / 5];
        assert!(
            (rms(mid) / rms(&x) - 1.0).abs() < 0.03,
            "{}",
            rms(mid) / rms(&x)
        );
        // Stereo: both channels stretched together.
        let st: Vec<f64> = x.iter().flat_map(|&v| [v, -v]).collect();
        let mut w = Wsola::new(1.3, RATE, 2);
        let mut out = Vec::new();
        w.process(&st, &mut out);
        w.flush(&mut out);
        assert!(out.chunks(2).all(|f| (f[0] + f[1]).abs() < 1e-9));
        assert!((out.len() / 2).abs_diff(y.len()) <= 1);
    }
}
