//! Tempo and beat-phase detection (M2-15), for lining up two tracks in an AutoMix transition.
//!
//! The signal becomes an onset envelope (how much louder the sound got in each 5 ms step, after
//! a high-pass so drums and plucks stand out). The tempo is the lag at which that envelope best
//! repeats; then a comb of equally spaced beats is slid over the envelope (tempo within 2% and
//! phase together) to find where the beats fall. Short, steady material is the target: a few
//! seconds at the start or end of a track.

/// A beat grid: beats fall at `first + k · period` seconds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Beats {
    pub period: f64,
    pub first: f64,
    /// 0..1: how strongly the envelope repeats at this tempo.
    pub strength: f64,
}

impl Beats {
    pub fn bpm(&self) -> f64 {
        60.0 / self.period
    }
}

const HOP_SECS: f64 = 0.005;

/// The envelope's real step: the hop is a whole number of samples, so it is close to
/// `HOP_SECS` but not exactly it, and at 120 BPM a 0.2% error is 10 ms after ten beats.
fn hop_secs(rate: u32) -> f64 {
    ((rate as f64 * HOP_SECS) as usize).max(1) as f64 / rate as f64
}
/// Window of the energy measure, in hops. A rise shows in the first window that touches the
/// onset, so on average the onset lies half a window after that window's start.
const WINDOW_HOPS: usize = 2;
const MIN_BPM: f64 = 70.0;
const MAX_BPM: f64 = 180.0;

/// The onset envelope: one value per `HOP_SECS`.
fn onset_envelope(x: &[f32], rate: u32) -> Vec<f64> {
    let hop = ((rate as f64 * HOP_SECS) as usize).max(1);
    let win = hop * WINDOW_HOPS;
    // First difference: tilts the spectrum so transients dominate bass and sustained notes.
    let diff: Vec<f64> = x.windows(2).map(|w| (w[1] - w[0]) as f64).collect();
    let frames = diff.len() / hop;
    let mut log_e = Vec::with_capacity(frames);
    for i in 0..frames {
        let start = i * hop;
        let end = (start + win).min(diff.len());
        let e: f64 = diff[start..end].iter().map(|v| v * v).sum::<f64>() / win as f64;
        log_e.push((1.0 + 1e4 * e).ln());
    }
    // Rising edges only.
    let mut env = vec![0.0; frames];
    for i in 1..frames {
        env[i] = (log_e[i] - log_e[i - 1]).max(0.0);
    }
    env
}

fn at(env: &[f64], pos: f64) -> f64 {
    if pos < 0.0 {
        return 0.0;
    }
    let i = pos.floor() as usize;
    if i + 1 >= env.len() {
        return 0.0;
    }
    let f = pos - i as f64;
    env[i] * (1.0 - f) + env[i + 1] * f
}

/// Finds the tempo and beat positions of `x` (mono), or None when the sound has no clear pulse.
pub fn analyze(x: &[f32], rate: u32) -> Option<Beats> {
    let env = onset_envelope(x, rate);
    let hop_secs = hop_secs(rate);
    if env.len() < (2.0 / hop_secs) as usize {
        return None; // under two seconds
    }
    let mean = env.iter().sum::<f64>() / env.len() as f64;
    let centred: Vec<f64> = env.iter().map(|v| v - mean).collect();
    let zero: f64 = centred.iter().map(|v| v * v).sum();
    if zero < 1e-9 {
        return None;
    }
    let lag_min = (60.0 / MAX_BPM / hop_secs).floor() as usize;
    let lag_max = (60.0 / MIN_BPM / hop_secs).ceil() as usize;
    if centred.len() <= lag_max * 2 {
        return None;
    }
    // Autocorrelation at whole-hop lags, weighted towards the common range around 120 BPM
    // (it settles octave doubts: 60 or 120).
    let mut ac = vec![0.0; lag_max + 2];
    for (lag, slot) in ac.iter_mut().enumerate().skip(lag_min.saturating_sub(1)) {
        let n = centred.len() - lag;
        let s: f64 = (0..n).map(|i| centred[i] * centred[i + lag]).sum();
        *slot = s / zero * (centred.len() as f64 / n as f64).min(2.0);
    }
    let weighted = |lag: usize| {
        let bpm = 60.0 / (lag as f64 * hop_secs);
        ac[lag] * (-0.5 * (bpm / 120.0).log2().powi(2)).exp()
    };
    let mut best = lag_min.max(1);
    for lag in lag_min.max(1)..=lag_max {
        if weighted(lag) > weighted(best) {
            best = lag;
        }
    }
    let strength = ac[best].clamp(0.0, 1.0);
    if strength < 0.12 {
        return None;
    }
    // Comb search over the period (±2%) and the phase, on an envelope smoothed over a few
    // hops so the score has one clear peak instead of a plateau.
    let env: Vec<f64> = (0..env.len())
        .map(|i| {
            let k = [1.0, 2.0, 3.0, 2.0, 1.0];
            (0..5)
                .map(|j| {
                    let idx = (i + j).saturating_sub(2).min(env.len() - 1);
                    env[idx] * k[j]
                })
                .sum::<f64>()
                / 9.0
        })
        .collect();
    let base = best as f64;
    let len = env.len() as f64;
    let mut top = (f64::MIN, base, 0.0);
    for step in -100..=100 {
        let p = base * (1.0 + step as f64 * 0.0002);
        let phases = p.ceil() as usize * 4;
        for k in 0..phases {
            let phase = k as f64 * p / phases as f64;
            let mut score = 0.0;
            let mut pos = phase;
            while pos < len - 1.0 {
                score += at(&env, pos);
                pos += p;
            }
            // Per beat the grid would have over the whole envelope, so tempos compare fairly.
            let score = score * p / len;
            if score > top.0 {
                top = (score, p, phase);
            }
        }
    }
    let (_, p, phase) = top;
    Some(Beats {
        period: p * hop_secs,
        first: (phase + WINDOW_HOPS as f64 / 2.0) * hop_secs,
        strength,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    const RATE: u32 = 22_050;

    /// A drum-like track: a decaying burst on every beat, over a steady bass note.
    pub fn drums(bpm: f64, offset: f64, secs: f64) -> Vec<f32> {
        drums_at(RATE, bpm, offset, secs)
    }

    pub fn drums_at(rate: u32, bpm: f64, offset: f64, secs: f64) -> Vec<f32> {
        let n = (rate as f64 * secs) as usize;
        let mut x = vec![0f32; n];
        let period = 60.0 / bpm;
        let mut seed = 12345u32;
        let mut t = offset;
        while t < secs {
            let start = (t * rate as f64) as usize;
            for i in 0..(rate as usize / 12) {
                if start + i >= n {
                    break;
                }
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                let noise = (seed >> 8) as f32 / (1u32 << 24) as f32 * 2.0 - 1.0;
                let env = (-(i as f32) / (rate as f32 * 0.012)).exp();
                x[start + i] += 0.7 * noise * env;
            }
            t += period;
        }
        for (i, v) in x.iter_mut().enumerate() {
            *v += 0.2 * (i as f32 * 110.0 * std::f32::consts::TAU / rate as f32).sin();
        }
        x
    }

    /// Distance from `t` to the nearest beat of the grid.
    pub fn off_grid(b: &Beats, t: f64) -> f64 {
        let k = ((t - b.first) / b.period).round();
        (t - (b.first + k * b.period)).abs()
    }

    #[test]
    fn finds_the_tempo_and_where_the_beats_fall() {
        for (bpm, offset) in [(120.0, 0.0), (120.0, 0.13), (96.0, 0.31), (140.0, 0.2)] {
            let x = drums(bpm, offset, 8.0);
            let b = analyze(&x, RATE).unwrap_or_else(|| panic!("no beats at {bpm}"));
            assert!(
                (b.bpm() / bpm - 1.0).abs() < 0.005,
                "{bpm} BPM measured as {:.2}",
                b.bpm()
            );
            // The beats the generator put down fall on the detected grid, within 10 ms.
            let period = 60.0 / bpm;
            for k in 0..(7.0 / period) as usize {
                let t = offset + k as f64 * period;
                assert!(
                    off_grid(&b, t) < 0.010,
                    "{bpm} BPM, beat {k}: {:.1} ms off",
                    off_grid(&b, t) * 1000.0
                );
            }
        }
    }

    #[test]
    fn silence_and_noise_have_no_pulse() {
        assert_eq!(analyze(&vec![0.0; RATE as usize * 6], RATE), None);
        let mut seed = 7u32;
        let noise: Vec<f32> = (0..RATE as usize * 6)
            .map(|_| {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                ((seed >> 8) as f32 / (1u32 << 24) as f32 - 0.5) * 0.2
            })
            .collect();
        assert_eq!(analyze(&noise, RATE), None);
        // Too short to tell.
        assert_eq!(analyze(&drums(120.0, 0.0, 1.0), RATE), None);
    }
}
