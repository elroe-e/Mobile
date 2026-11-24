//! Sample-rate conversion (M2-07) and channel mapping, as stages of the DSP graph.
//!
//! The converter is `rubato` (pure Rust). Quality presets:
//! * **Fast**: cubic polynomial interpolation (cheap; for weak devices or previews).
//! * **Medium / High / Best**: FFT-based synchronous conversion with longer filters. Best keeps
//!   the passband flat within ±0.01 dB to 20 kHz and pushes aliasing below −120 dB (see tests).
//!
//! Lengths are exact: the filter's start-up delay is trimmed and the tail is flushed at the end
//! of a track, so a 1 s file at 44.1 kHz becomes exactly 48,000 frames at 48 kHz. That keeps
//! positions sample-accurate and gapless joins seamless.

use audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Async, Fft, FixedAsync, FixedSync, Indexing, PolynomialDegree, WindowFunction};

/// How carefully to convert sample rates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Quality {
    Fast,
    Medium,
    High,
    #[default]
    Best,
}

impl Quality {
    pub fn parse(s: &str) -> Quality {
        match s {
            "fast" => Quality::Fast,
            "medium" => Quality::Medium,
            "high" => Quality::High,
            _ => Quality::Best,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Quality::Fast => "fast",
            Quality::Medium => "medium",
            Quality::High => "high",
            Quality::Best => "best",
        }
    }
}

enum Engine {
    Poly(Async<f64>),
    Fft(Box<Fft<f64>>),
}

impl Engine {
    fn r(&mut self) -> &mut dyn rubato::Resampler<f64> {
        match self {
            Engine::Poly(r) => r,
            Engine::Fft(r) => r.as_mut(),
        }
    }
}

/// A streaming rate converter for interleaved f64 audio.
pub struct Resampler {
    channels: usize,
    from: u32,
    to: u32,
    engine: Engine,
    /// Input frames waiting for a full chunk.
    input: Vec<f64>,
    out_buf: Vec<f64>,
    /// Output frames still to drop (the filter's start-up delay).
    skip: usize,
    /// Input frames received, and output frames handed out, for exact lengths.
    frames_in: u64,
    frames_out: u64,
}

const CHUNK: usize = 1024;

impl Resampler {
    pub fn new(from: u32, to: u32, channels: usize, quality: Quality) -> Resampler {
        let channels = channels.max(1);
        let engine = match quality {
            Quality::Fast => Engine::Poly(
                Async::<f64>::new_poly(
                    to as f64 / from as f64,
                    1.0,
                    PolynomialDegree::Cubic,
                    CHUNK,
                    channels,
                    FixedAsync::Input,
                )
                .expect("valid resampler"),
            ),
            q => {
                // Fewer sub-chunks mean longer FFTs: a steeper, cleaner filter.
                let (sub_chunks, window) = match q {
                    Quality::Medium => (8, WindowFunction::Blackman2),
                    Quality::High => (2, WindowFunction::BlackmanHarris2),
                    _ => (1, WindowFunction::BlackmanHarris2),
                };
                let chunk = if q == Quality::Best { 4 * CHUNK } else { CHUNK };
                Engine::Fft(Box::new(
                    Fft::<f64>::new_custom(
                        from as usize,
                        to as usize,
                        chunk,
                        sub_chunks,
                        channels,
                        window,
                        FixedSync::Input,
                    )
                    .expect("valid resampler"),
                ))
            }
        };
        let mut r = Resampler {
            channels,
            from,
            to,
            engine,
            input: Vec::new(),
            out_buf: Vec::new(),
            skip: 0,
            frames_in: 0,
            frames_out: 0,
        };
        r.skip = r.engine.r().output_delay();
        let max = r.engine.r().output_frames_max();
        r.out_buf = vec![0.0; max * channels];
        r
    }

    /// Output frames the input so far should become.
    fn expected_out(&self) -> u64 {
        (self.frames_in as u128 * self.to as u128 / self.from as u128) as u64
    }

    fn run_chunk(&mut self, partial: Option<usize>, out: &mut Vec<f64>) {
        let ch = self.channels;
        let need = self.engine.r().input_frames_next();
        let have = self.input.len() / ch;
        let take = partial.unwrap_or(need).min(have);
        let mut chunk = self.input[..take * ch].to_vec();
        chunk.resize(need * ch, 0.0);
        self.input.drain(..take * ch);
        let indexing = Indexing {
            input_offset: 0,
            output_offset: 0,
            partial_len: partial.map(|_| take),
            active_channels_mask: None,
        };
        let cap = self.out_buf.len() / ch;
        let inp = InterleavedSlice::new(&chunk, ch, need).expect("input buffer");
        let mut outp =
            InterleavedSlice::new_mut(&mut self.out_buf, ch, cap).expect("output buffer");
        let (_, written) = self
            .engine
            .r()
            .process_into_buffer(&inp, &mut outp, Some(&indexing))
            .expect("resample");
        let mut frames = &self.out_buf[..written * ch];
        let drop = self.skip.min(written);
        self.skip -= drop;
        frames = &frames[drop * ch..];
        // Never hand out more than the input calls for (the flush pads with silence).
        let allowed = self.expected_out().saturating_sub(self.frames_out) as usize;
        let n = (frames.len() / ch).min(allowed);
        out.extend_from_slice(&frames[..n * ch]);
        self.frames_out += n as u64;
    }

    pub fn process(&mut self, input: &[f64], out: &mut Vec<f64>) {
        self.input.extend_from_slice(input);
        self.frames_in += (input.len() / self.channels) as u64;
        while self.input.len() / self.channels >= self.engine.r().input_frames_next() {
            self.run_chunk(None, out);
        }
    }

    /// End of the stream: converts what is left and the filter's tail, so the output has
    /// exactly the length the input calls for.
    pub fn flush(&mut self, out: &mut Vec<f64>) {
        let mut guard = 0;
        while self.frames_out < self.expected_out() && guard < 64 {
            let have = self.input.len() / self.channels;
            self.run_chunk(Some(have), out);
            guard += 1;
        }
    }
}

/// Converts interleaved audio from `src` channels to `dst` channels.
pub fn map_channels(input: &[f64], src: usize, dst: usize, out: &mut Vec<f64>) {
    if src == dst {
        out.extend_from_slice(input);
        return;
    }
    for frame in input.chunks_exact(src) {
        if dst == 1 {
            out.push(frame.iter().sum::<f64>() / src as f64);
        } else if src == 1 {
            out.extend(std::iter::repeat_n(frame[0], dst));
        } else {
            for c in 0..dst {
                out.push(frame[c.min(src - 1)]);
            }
        }
    }
}

// ---- Dither ----

/// Dither for reducing bit depth (e.g. to a 16-bit output): adds triangular (TPDF) noise of
/// ±1 LSB so rounding error becomes benign noise instead of distortion. `shaped` uses high-pass
/// TPDF (the difference of successive random values), which moves the noise up in frequency.
/// Used by [`crate::sink::Encoder`] for integer outputs (M2-02).
pub struct Dither {
    bits: u32,
    shaped: bool,
    channels: usize,
    rng: u64,
    prev: Vec<f64>,
}

impl Dither {
    pub fn new(bits: u32, channels: usize, shaped: bool) -> Dither {
        Dither {
            bits,
            shaped,
            channels: channels.max(1),
            rng: 0x9E37_79B9_7F4A_7C15,
            prev: vec![0.0; channels.max(1)],
        }
    }

    fn uniform(&mut self) -> f64 {
        // xorshift64*: uniform in [-0.5, 0.5).
        self.rng ^= self.rng >> 12;
        self.rng ^= self.rng << 25;
        self.rng ^= self.rng >> 27;
        let v = self.rng.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 11;
        v as f64 / (1u64 << 53) as f64 - 0.5
    }

    /// One sample of channel `c`, dithered and quantised: the integer value (as f64).
    pub fn quantize(&mut self, v: f64, c: usize) -> f64 {
        let scale = (1u64 << (self.bits - 1)) as f64;
        let c = c % self.channels;
        let noise = if self.shaped {
            let r = self.uniform();
            let n = r - self.prev[c];
            self.prev[c] = r;
            n
        } else {
            self.uniform() + self.uniform()
        };
        (v * scale + noise).round().clamp(-scale, scale - 1.0)
    }

    /// Adds the dither and quantises to the target depth (values stay in -1..1).
    #[cfg(test)]
    pub fn process(&mut self, input: &[f64], out: &mut Vec<f64>) {
        let scale = (1u64 << (self.bits - 1)) as f64;
        for (i, &v) in input.iter().enumerate() {
            out.push(self.quantize(v, i) / scale);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::TAU;

    fn sine(freq: f64, rate: u32, secs: f64, amp: f64) -> Vec<f64> {
        let n = (rate as f64 * secs) as usize;
        (0..n)
            .flat_map(|i| {
                let v = (TAU * freq * i as f64 / rate as f64).sin() * amp;
                [v, v]
            })
            .collect()
    }

    fn convert(input: &[f64], from: u32, to: u32, q: Quality) -> Vec<f64> {
        let mut r = Resampler::new(from, to, 2, q);
        let mut out = Vec::new();
        // Feed in odd-sized pieces, like the decoder does.
        for piece in input.chunks(2 * 777) {
            r.process(piece, &mut out);
        }
        r.flush(&mut out);
        out
    }

    /// Amplitude of `freq` in the left channel (Goertzel over a Hann-windowed middle part).
    fn level(samples: &[f64], rate: u32, freq: f64) -> f64 {
        let left: Vec<f64> = samples.iter().step_by(2).copied().collect();
        let n = left.len();
        let (a, b) = (n / 4, n * 3 / 4);
        let len = b - a;
        let w = TAU * freq / rate as f64;
        let (mut re, mut im, mut norm) = (0.0, 0.0, 0.0);
        for (k, &x) in left[a..b].iter().enumerate() {
            let hann = 0.5 - 0.5 * (TAU * k as f64 / len as f64).cos();
            re += x * hann * (w * k as f64).cos();
            im += x * hann * (w * k as f64).sin();
            norm += hann;
        }
        2.0 * (re * re + im * im).sqrt() / norm
    }

    fn db(x: f64) -> f64 {
        20.0 * x.log10()
    }

    #[test]
    fn exact_lengths_for_every_quality() {
        for q in [Quality::Fast, Quality::Medium, Quality::High, Quality::Best] {
            for (from, to) in [
                (44100, 48000),
                (96000, 48000),
                (48000, 44100),
                (192000, 48000),
            ] {
                let input = sine(440.0, from, 1.0, 0.5);
                let out = convert(&input, from, to, q);
                assert_eq!(out.len() / 2, to as usize, "{q:?} {from}→{to}");
            }
        }
    }

    #[test]
    fn delay_is_trimmed() {
        // A tone starting at sample 0 must start at sample 0 after conversion too: the first
        // samples follow the sine, not a block of silence.
        let input = sine(1000.0, 44100, 0.2, 0.5);
        let out = convert(&input, 44100, 48000, Quality::Best);
        let peak_early = out[..2 * 100].iter().fold(0.0f64, |m, v| m.max(v.abs()));
        assert!(peak_early > 0.3, "start is silent: {peak_early}");
    }

    #[test]
    fn best_passband_is_flat_to_20k() {
        for freq in [100.0, 1000.0, 5000.0, 10000.0, 15000.0, 20000.0] {
            let input = sine(freq, 44100, 1.0, 0.5);
            let out = convert(&input, 44100, 48000, Quality::Best);
            let ripple = db(level(&out, 48000, freq) / 0.5);
            assert!(ripple.abs() < 0.01, "{freq} Hz: {ripple:.4} dB");
        }
    }

    #[test]
    fn best_aliasing_is_below_minus_120_db() {
        // 96 → 48 kHz: a 26 kHz tone can't exist at 48 kHz; it must not fold back to 22 kHz.
        let input = sine(26000.0, 96000, 1.0, 0.5);
        let out = convert(&input, 96000, 48000, Quality::Best);
        let alias = db(level(&out, 48000, 22000.0) / 0.5);
        assert!(alias < -120.0, "alias at {alias:.1} dB");
        // And a 44.1 kHz source's top octave doesn't produce images below 24 kHz.
        let input = sine(21000.0, 44100, 1.0, 0.5);
        let out = convert(&input, 44100, 48000, Quality::Best);
        let image = db(level(&out, 48000, 23100.0) / 0.5);
        assert!(image < -120.0, "image at {image:.1} dB");
    }

    #[test]
    fn tpdf_dither_is_quiet_and_unbiased() {
        let mut d = Dither::new(16, 2, false);
        let silence = vec![0.0; 2 * 48000];
        let mut out = Vec::new();
        d.process(&silence, &mut out);
        let mean = out.iter().sum::<f64>() / out.len() as f64;
        let rms = (out.iter().map(|v| v * v).sum::<f64>() / out.len() as f64).sqrt();
        let lsb = 1.0 / 32768.0;
        assert!(mean.abs() < 0.05 * lsb, "mean {mean}");
        assert!(rms > 0.3 * lsb && rms < 1.2 * lsb, "rms {} LSB", rms / lsb);
        // Every output is on the 16-bit grid.
        assert!(out.iter().all(|v| (v * 32768.0).fract() == 0.0));
    }

    #[test]
    fn shaped_dither_pushes_noise_up() {
        let mut d = Dither::new(16, 1, true);
        let mut out = Vec::new();
        d.process(&vec![0.0; 48000], &mut out);
        // High-pass TPDF: successive samples are negatively correlated.
        let corr: f64 = out.windows(2).map(|w| w[0] * w[1]).sum::<f64>()
            / out.iter().map(|v| v * v).sum::<f64>();
        assert!(corr < -0.2, "lag-1 correlation {corr}");
    }

    #[test]
    fn mono_to_stereo() {
        let mut out = Vec::new();
        map_channels(&[0.1, 0.2], 1, 2, &mut out);
        assert_eq!(out, vec![0.1, 0.1, 0.2, 0.2]);
    }
}
