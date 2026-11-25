//! Equalizer (M2-10): graphic bands (5/10/15/31 ISO centres), parametric bands (peak, shelves,
//! high- and low-pass; AutoEq in M2-11), bass and treble shelves, balance, input gain with
//! automatic headroom, a compressor and a look-ahead peak limiter. All in f64 at the output rate.
//!
//! Filters are biquads from the RBJ Audio EQ Cookbook in transposed direct form II. Changing the
//! settings keeps the filters' memory, so adjusting a band while music plays doesn't click.

use std::f64::consts::PI;

/// Graphic EQ band centres in Hz for each band count.
pub fn centres(count: usize) -> &'static [f64] {
    match count {
        5 => &[63.0, 250.0, 1000.0, 4000.0, 16000.0],
        15 => &[
            25.0, 40.0, 63.0, 100.0, 160.0, 250.0, 400.0, 630.0, 1000.0, 1600.0, 2500.0, 4000.0,
            6300.0, 10000.0, 16000.0,
        ],
        31 => &[
            20.0, 25.0, 31.5, 40.0, 50.0, 63.0, 80.0, 100.0, 125.0, 160.0, 200.0, 250.0, 315.0,
            400.0, 500.0, 630.0, 800.0, 1000.0, 1250.0, 1600.0, 2000.0, 2500.0, 3150.0, 4000.0,
            5000.0, 6300.0, 8000.0, 10000.0, 12500.0, 16000.0, 20000.0,
        ],
        _ => &[
            31.25, 62.5, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0,
        ],
    }
}

/// The Q of a graphic band: constant-Q, so neighbouring bands meet near their −3 dB points.
pub fn graphic_q(count: usize) -> f64 {
    let c = centres(count);
    let step = (c[c.len() - 1] / c[0]).powf(1.0 / (c.len() - 1) as f64);
    step.sqrt() / (step - 1.0)
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    Peak,
    LowShelf,
    HighShelf,
    HighPass,
    LowPass,
}

/// One parametric band.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Band {
    pub kind: Kind,
    pub freq: f64,
    /// dB (ignored by the passes).
    pub gain: f64,
    pub q: f64,
}

/// What the equalizer does.
#[derive(Debug, Clone, PartialEq)]
pub struct EqParams {
    /// Graphic band gains in dB; the count picks the centres (5, 10, 15 or 31).
    pub gains: Vec<f64>,
    pub parametric: Vec<Band>,
    pub preamp_db: f64,
    pub bass_db: f64,
    pub treble_db: f64,
    /// −1 (left only) … 1 (right only).
    pub balance: f64,
    pub auto_headroom: bool,
    pub limiter: bool,
    pub compressor: Option<Compressor>,
}

impl Default for EqParams {
    fn default() -> Self {
        EqParams {
            gains: vec![0.0; 10],
            parametric: vec![],
            preamp_db: 0.0,
            bass_db: 0.0,
            treble_db: 0.0,
            balance: 0.0,
            auto_headroom: true,
            limiter: true,
            compressor: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Compressor {
    pub threshold_db: f64,
    pub ratio: f64,
    pub attack_ms: f64,
    pub release_ms: f64,
}

/// Where the bass and treble shelves sit.
const BASS_HZ: f64 = 150.0;
const TREBLE_HZ: f64 = 6000.0;
const SHELF_Q: f64 = 0.707;
/// The limiter's ceiling: −0.5 dBFS.
pub const LIMIT: f64 = 0.944_060_876_285_923;

#[derive(Debug, Clone, Copy, PartialEq)]
struct Coeffs {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
}

impl Coeffs {
    fn design(b: Band, rate: f64) -> Coeffs {
        // Keep the centre below Nyquist.
        let f = b.freq.clamp(1.0, rate * 0.45);
        let w = 2.0 * PI * f / rate;
        let (sin, cos) = w.sin_cos();
        let q = b.q.max(0.05);
        let alpha = sin / (2.0 * q);
        let a = 10f64.powf(b.gain / 40.0);
        let (b0, b1, b2, a0, a1, a2) = match b.kind {
            Kind::Peak => (
                1.0 + alpha * a,
                -2.0 * cos,
                1.0 - alpha * a,
                1.0 + alpha / a,
                -2.0 * cos,
                1.0 - alpha / a,
            ),
            Kind::LowShelf => {
                let s = 2.0 * a.sqrt() * alpha;
                (
                    a * ((a + 1.0) - (a - 1.0) * cos + s),
                    2.0 * a * ((a - 1.0) - (a + 1.0) * cos),
                    a * ((a + 1.0) - (a - 1.0) * cos - s),
                    (a + 1.0) + (a - 1.0) * cos + s,
                    -2.0 * ((a - 1.0) + (a + 1.0) * cos),
                    (a + 1.0) + (a - 1.0) * cos - s,
                )
            }
            Kind::HighShelf => {
                let s = 2.0 * a.sqrt() * alpha;
                (
                    a * ((a + 1.0) + (a - 1.0) * cos + s),
                    -2.0 * a * ((a - 1.0) + (a + 1.0) * cos),
                    a * ((a + 1.0) + (a - 1.0) * cos - s),
                    (a + 1.0) - (a - 1.0) * cos + s,
                    2.0 * ((a - 1.0) - (a + 1.0) * cos),
                    (a + 1.0) - (a - 1.0) * cos - s,
                )
            }
            Kind::HighPass => (
                (1.0 + cos) / 2.0,
                -(1.0 + cos),
                (1.0 + cos) / 2.0,
                1.0 + alpha,
                -2.0 * cos,
                1.0 - alpha,
            ),
            Kind::LowPass => (
                (1.0 - cos) / 2.0,
                1.0 - cos,
                (1.0 - cos) / 2.0,
                1.0 + alpha,
                -2.0 * cos,
                1.0 - alpha,
            ),
        };
        Coeffs {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
        }
    }

    /// Magnitude in dB at `f` Hz.
    fn response_db(&self, f: f64, rate: f64) -> f64 {
        let w = 2.0 * PI * f / rate;
        // |B(e^jw)| / |A(e^jw)| with z^-1 = e^-jw.
        let (c1, s1, c2, s2) = (w.cos(), w.sin(), (2.0 * w).cos(), (2.0 * w).sin());
        let br = self.b0 + self.b1 * c1 + self.b2 * c2;
        let bi = -(self.b1 * s1 + self.b2 * s2);
        let ar = 1.0 + self.a1 * c1 + self.a2 * c2;
        let ai = -(self.a1 * s1 + self.a2 * s2);
        10.0 * ((br * br + bi * bi) / (ar * ar + ai * ai)).log10()
    }
}

impl EqParams {
    /// Every filter in order (graphic, parametric, tone), skipping ones that do nothing.
    fn filters(&self) -> Vec<Band> {
        let mut v = Vec::new();
        let q = graphic_q(self.gains.len());
        for (&g, &f) in self.gains.iter().zip(centres(self.gains.len())) {
            if g.abs() > 1e-3 {
                v.push(Band {
                    kind: Kind::Peak,
                    freq: f,
                    gain: g.clamp(-24.0, 24.0),
                    q,
                });
            }
        }
        v.extend(
            self.parametric.iter().copied().filter(|b| {
                matches!(b.kind, Kind::HighPass | Kind::LowPass) || b.gain.abs() > 1e-3
            }),
        );
        if self.bass_db.abs() > 1e-3 {
            v.push(Band {
                kind: Kind::LowShelf,
                freq: BASS_HZ,
                gain: self.bass_db,
                q: SHELF_Q,
            });
        }
        if self.treble_db.abs() > 1e-3 {
            v.push(Band {
                kind: Kind::HighShelf,
                freq: TREBLE_HZ,
                gain: self.treble_db,
                q: SHELF_Q,
            });
        }
        v
    }

    /// The filters' combined response in dB at `f` (before the input gain).
    pub fn response_db(&self, f: f64, rate: f64) -> f64 {
        self.filters()
            .into_iter()
            .map(|b| Coeffs::design(b, rate).response_db(f, rate))
            .sum()
    }

    /// The curve's highest boost (0 if it only cuts), checked across 20 Hz–20 kHz.
    pub fn peak_boost_db(&self, rate: f64) -> f64 {
        (0..=240)
            .map(|i| 20.0 * 1000f64.powf(i as f64 / 240.0))
            .filter(|&f| f < rate * 0.49)
            .map(|f| self.response_db(f, rate))
            .fold(0.0, f64::max)
    }

    /// Input gain in dB: the pre-amp, lowered by the curve's highest boost with auto headroom.
    pub fn input_gain_db(&self, rate: f64) -> f64 {
        let headroom = if self.auto_headroom {
            self.peak_boost_db(rate).clamp(0.0, 24.0)
        } else {
            0.0
        };
        (self.preamp_db - headroom).clamp(-48.0, 12.0)
    }

    /// Does nothing at all (no stage needed).
    pub fn is_flat(&self) -> bool {
        self.filters().is_empty()
            && self.preamp_db.abs() < 1e-3
            && self.balance.abs() < 1e-3
            && self.compressor.is_none()
    }

    /// For the processing list, e.g. "equalizer (10 bands, limiter)".
    pub fn describe(&self) -> String {
        let mut parts = vec![format!("{} bands", self.gains.len())];
        if !self.parametric.is_empty() {
            parts.push(format!("{} parametric", self.parametric.len()));
        }
        if self.compressor.is_some() {
            parts.push("compressor".into());
        }
        if self.limiter {
            parts.push("limiter".into());
        }
        format!("equalizer ({})", parts.join(", "))
    }
}

/// A running equalizer for `channels` interleaved channels.
pub struct Equalizer {
    rate: f64,
    channels: usize,
    params: EqParams,
    coeffs: Vec<Coeffs>,
    /// Per filter, per channel: (s1, s2).
    state: Vec<Vec<(f64, f64)>>,
    input_gain: f64,
    balance: (f64, f64),
    comp_env: f64,
    comp: Option<(f64, f64, f64, f64)>, // threshold dB, ratio, attack coef, release coef
    limiter: Option<Limiter>,
}

impl Equalizer {
    pub fn new(params: EqParams, rate: u32, channels: usize) -> Equalizer {
        let mut eq = Equalizer {
            rate: rate as f64,
            channels: channels.max(1),
            params: EqParams::default(),
            coeffs: vec![],
            state: vec![],
            input_gain: 1.0,
            balance: (1.0, 1.0),
            comp_env: 0.0,
            comp: None,
            limiter: None,
        };
        eq.set(params);
        eq
    }

    /// New settings; filter memory is kept where the filter list keeps its shape.
    pub fn set(&mut self, params: EqParams) {
        let filters = params.filters();
        let coeffs: Vec<Coeffs> = filters
            .iter()
            .map(|&b| Coeffs::design(b, self.rate))
            .collect();
        if coeffs.len() != self.state.len() {
            self.state = vec![vec![(0.0, 0.0); self.channels]; coeffs.len()];
        }
        self.coeffs = coeffs;
        self.input_gain = 10f64.powf(params.input_gain_db(self.rate) / 20.0);
        let b = params.balance.clamp(-1.0, 1.0);
        self.balance = ((1.0 - b).min(1.0), (1.0 + b).min(1.0));
        let coef = |ms: f64| (-1.0 / (ms.max(0.1) * 0.001 * self.rate)).exp();
        self.comp = params.compressor.map(|c| {
            (
                c.threshold_db.clamp(-60.0, 0.0),
                c.ratio.clamp(1.0, 20.0),
                coef(c.attack_ms.clamp(1.0, 100.0)),
                coef(c.release_ms.clamp(20.0, 1000.0)),
            )
        });
        match (params.limiter, self.limiter.is_some()) {
            (true, false) => self.limiter = Some(Limiter::new(self.rate, self.channels)),
            (false, _) => self.limiter = None,
            _ => {}
        }
        self.params = params;
    }

    pub fn params(&self) -> &EqParams {
        &self.params
    }

    /// Filters `samples` (interleaved) in place.
    pub fn process(&mut self, samples: &mut [f64]) {
        let ch = self.channels;
        for frame in samples.chunks_mut(ch) {
            for (c, s) in frame.iter_mut().enumerate() {
                let mut x = *s * self.input_gain;
                for (k, f) in self.coeffs.iter().enumerate() {
                    let st = &mut self.state[k][c];
                    let y = f.b0 * x + st.0;
                    st.0 = f.b1 * x - f.a1 * y + st.1;
                    st.1 = f.b2 * x - f.a2 * y;
                    x = y;
                }
                if ch == 2 {
                    x *= if c == 0 {
                        self.balance.0
                    } else {
                        self.balance.1
                    };
                }
                *s = x;
            }
            if let Some((thr, ratio, att, rel)) = self.comp {
                // Linked peak detector: one gain for all channels.
                let peak = frame.iter().fold(0.0f64, |m, v| m.max(v.abs()));
                let coef = if peak > self.comp_env { att } else { rel };
                self.comp_env = coef * self.comp_env + (1.0 - coef) * peak;
                let level = 20.0 * self.comp_env.max(1e-9).log10();
                if level > thr {
                    let reduce = (level - thr) * (1.0 - 1.0 / ratio);
                    let g = 10f64.powf(-reduce / 20.0);
                    frame.iter_mut().for_each(|v| *v *= g);
                }
            }
        }
        if let Some(l) = &mut self.limiter {
            l.process(samples);
        }
    }

    /// Frames the output lags the input (the limiter's look-ahead).
    pub fn latency(&self) -> usize {
        self.limiter.as_ref().map(|l| l.lookahead).unwrap_or(0)
    }

    /// The limiter's held-back samples at the end of the stream.
    pub fn flush(&mut self, out: &mut Vec<f64>) {
        if let Some(l) = &mut self.limiter {
            let mut tail = vec![0.0; l.lookahead * self.channels];
            l.process(&mut tail);
            out.extend(tail);
        }
    }
}

/// Look-ahead peak limiter: the gain falls before a peak arrives, so no sample exceeds the
/// ceiling; it recovers over 60 ms.
struct Limiter {
    channels: usize,
    lookahead: usize,
    /// Delay line, interleaved.
    delay: std::collections::VecDeque<f64>,
    /// Required gain for each frame in the window (sliding minimum over it).
    needed: std::collections::VecDeque<f64>,
    gain: f64,
    release: f64,
}

impl Limiter {
    fn new(rate: f64, channels: usize) -> Limiter {
        let lookahead = (rate * 0.005).round().max(1.0) as usize;
        Limiter {
            channels,
            lookahead,
            delay: std::iter::repeat_n(0.0, lookahead * channels).collect(),
            needed: std::iter::repeat_n(1.0, lookahead).collect(),
            gain: 1.0,
            release: (-1.0 / (0.06 * rate)).exp(),
        }
    }

    fn process(&mut self, samples: &mut [f64]) {
        let ch = self.channels;
        for frame in samples.chunks_mut(ch) {
            let peak = frame.iter().fold(0.0f64, |m, v| m.max(v.abs()));
            let need = if peak > LIMIT { LIMIT / peak } else { 1.0 };
            self.needed.pop_front();
            self.needed.push_back(need);
            let target = self.needed.iter().copied().fold(1.0, f64::min);
            // Down at once to what any sample in the window needs; up slowly.
            self.gain = if target < self.gain {
                target
            } else {
                target + (self.gain - target) * self.release
            };
            for s in frame.iter_mut() {
                self.delay.push_back(*s);
                let out = self.delay.pop_front().unwrap_or(0.0) * self.gain;
                *s = out.clamp(-LIMIT, LIMIT);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: f64 = 48_000.0;

    /// The measured gain (dB) of a steady sine at `f` through `eq`.
    fn measure(params: &EqParams, f: f64) -> f64 {
        let mut eq = Equalizer::new(
            EqParams {
                limiter: false,
                ..params.clone()
            },
            RATE as u32,
            1,
        );
        let n = RATE as usize; // 1 s; measure the last half (filters settled)
        let mut s: Vec<f64> = (0..n)
            .map(|i| 0.1 * (2.0 * PI * f * i as f64 / RATE).sin())
            .collect();
        eq.process(&mut s);
        let rms = |v: &[f64]| (v.iter().map(|x| x * x).sum::<f64>() / v.len() as f64).sqrt();
        20.0 * (rms(&s[n / 2..]) / (0.1 / 2f64.sqrt())).log10()
    }

    #[test]
    fn measured_response_matches_the_designed_curve() {
        let params = EqParams {
            gains: vec![6.0, -3.0, 0.0, 4.5, -8.0, 2.0, 0.0, 3.0, -2.0, 5.0],
            bass_db: 3.0,
            treble_db: -2.0,
            parametric: vec![Band {
                kind: Kind::Peak,
                freq: 3200.0,
                gain: -4.0,
                q: 2.0,
            }],
            auto_headroom: false,
            ..EqParams::default()
        };
        for f in [40.0, 100.0, 440.0, 1000.0, 2500.0, 3200.0, 7000.0, 12000.0] {
            let want = params.response_db(f, RATE);
            let got = measure(&params, f);
            assert!(
                (got - want).abs() < 0.1,
                "{f} Hz: measured {got:.2} dB, designed {want:.2}"
            );
        }
    }

    #[test]
    fn a_band_peaks_at_its_centre_with_its_gain() {
        for count in [5, 10, 15, 31] {
            let c = centres(count);
            let i = count / 2;
            let mut gains = vec![0.0; count];
            gains[i] = 6.0;
            let p = EqParams {
                gains,
                auto_headroom: false,
                ..EqParams::default()
            };
            let at = p.response_db(c[i], RATE);
            assert!((at - 6.0).abs() < 0.01, "{count} bands: {at}");
            // A neighbour is far less affected (constant-Q bands).
            assert!(p.response_db(c[i + 1], RATE) < 3.5);
        }
    }

    #[test]
    fn auto_headroom_and_flat() {
        let p = EqParams {
            gains: vec![0.0, 0.0, 0.0, 0.0, 0.0, 6.0, 0.0, 0.0, 0.0, 0.0],
            ..EqParams::default()
        };
        assert!((p.input_gain_db(RATE) + 6.0).abs() < 0.05);
        let cut = EqParams {
            gains: vec![-6.0; 10],
            ..EqParams::default()
        };
        assert_eq!(cut.input_gain_db(RATE), 0.0);
        assert!(EqParams::default().is_flat());
        // Flat passes samples through exactly.
        let mut eq = Equalizer::new(
            EqParams {
                limiter: false,
                ..EqParams::default()
            },
            48_000,
            2,
        );
        let mut s = vec![0.25, -0.5, 0.125, 0.0];
        eq.process(&mut s);
        assert_eq!(s, vec![0.25, -0.5, 0.125, 0.0]);
    }

    #[test]
    fn limiter_keeps_peaks_under_the_ceiling() {
        let mut eq = Equalizer::new(
            EqParams {
                preamp_db: 12.0,
                auto_headroom: false,
                ..EqParams::default()
            },
            48_000,
            2,
        );
        let mut s: Vec<f64> = (0..96_000)
            .map(|i| 0.9 * (2.0 * PI * 100.0 * (i / 2) as f64 / RATE).sin())
            .collect();
        eq.process(&mut s);
        let peak = s.iter().fold(0.0f64, |m, v| m.max(v.abs()));
        assert!(peak <= LIMIT + 1e-12, "{peak}");
        // And it isn't just clipping: the waveform stays near the ceiling, smoothly.
        assert!(peak > 0.9 * LIMIT);
        assert_eq!(eq.latency(), 240);
    }

    #[test]
    fn balance_and_compressor() {
        let mut eq = Equalizer::new(
            EqParams {
                balance: 0.5,
                limiter: false,
                ..EqParams::default()
            },
            48_000,
            2,
        );
        let mut s = vec![1.0, 1.0];
        eq.process(&mut s);
        assert_eq!(s, vec![0.5, 1.0]);
        // A loud steady tone 12 dB over a −24 dB threshold at 4:1 comes out ~9 dB quieter.
        let p = EqParams {
            compressor: Some(Compressor {
                threshold_db: -24.0,
                ratio: 4.0,
                attack_ms: 5.0,
                release_ms: 100.0,
            }),
            limiter: false,
            ..EqParams::default()
        };
        let amp = 10f64.powf(-12.0 / 20.0);
        let mut eq = Equalizer::new(p, 48_000, 1);
        let mut s: Vec<f64> = (0..48_000)
            .map(|i| amp * (2.0 * PI * 1000.0 * i as f64 / RATE).sin())
            .collect();
        eq.process(&mut s);
        let peak = s[40_000..].iter().fold(0.0f64, |m, v| m.max(v.abs()));
        let reduction = 20.0 * (amp / peak).log10();
        assert!((reduction - 9.0).abs() < 1.0, "{reduction} dB");
    }
}
