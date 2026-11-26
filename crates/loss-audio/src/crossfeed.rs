//! Crossfeed for headphones (M2-12), after Bauer's stereophonic-to-binaural idea (as in BS2B):
//! on speakers each ear hears both channels, on headphones only its own, which makes hard-panned
//! music sound lopsided and tiring. A low-passed copy of each side is mixed into the other.
//!
//! `out_L = L + g · lp(R − L)`, `out_R = R + g · lp(L − R)`: at low frequencies the direct sound
//! is `1 − g` and the cross-fed `g`; high frequencies stay as they were. Mono is unchanged
//! exactly, and no sample can exceed its inputs' peak (for g ≤ ½ it is a convex mix).

/// How strong the crossfeed is. The numbers are BS2B's presets: feed level and cut-off.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Strength {
    /// Jan Meier: 9.5 dB, 650 Hz.
    #[default]
    Light,
    /// Chu Moy: 6 dB, 700 Hz.
    Medium,
    /// BS2B's default: 4.5 dB, 700 Hz.
    Strong,
}

impl Strength {
    /// "light", "medium" or "strong"; None for "off" and anything else.
    pub fn parse(s: &str) -> Option<Strength> {
        match s {
            "light" => Some(Strength::Light),
            "medium" => Some(Strength::Medium),
            "strong" => Some(Strength::Strong),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Strength::Light => "light",
            Strength::Medium => "medium",
            Strength::Strong => "strong",
        }
    }

    /// (feed level in dB, cut-off in Hz).
    fn preset(self) -> (f64, f64) {
        match self {
            Strength::Light => (9.5, 650.0),
            Strength::Medium => (6.0, 700.0),
            Strength::Strong => (4.5, 700.0),
        }
    }

    /// The share of the other side at low frequencies: the cross-fed level `r = 10^(−F/20)`
    /// against the direct one, as a fraction of their sum.
    fn mix(self) -> f64 {
        let r = 10f64.powf(-self.preset().0 / 20.0);
        r / (1.0 + r)
    }
}

pub struct Crossfeed {
    strength: Strength,
    g: f64,
    /// One-pole low-pass coefficient.
    a: f64,
    /// The low-passed (R − L); the other side's is its negative.
    lp: f64,
}

impl Crossfeed {
    pub fn new(strength: Strength, rate: u32) -> Crossfeed {
        let fc = strength.preset().1.min(rate as f64 * 0.45);
        let a = 1.0 - (-2.0 * std::f64::consts::PI * fc / rate as f64).exp();
        Crossfeed {
            strength,
            g: strength.mix(),
            a,
            lp: 0.0,
        }
    }

    pub fn strength(&self) -> Strength {
        self.strength
    }

    /// Filters interleaved stereo in place.
    pub fn process(&mut self, samples: &mut [f64]) {
        for frame in samples.chunks_exact_mut(2) {
            let (l, r) = (frame[0], frame[1]);
            // lp(L − R) is the negative of lp(R − L), so one filter serves both sides.
            self.lp += self.a * ((r - l) - self.lp);
            frame[0] = l + self.g * self.lp;
            frame[1] = r - self.g * self.lp;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::TAU;

    const RATE: u32 = 48_000;

    /// Gain (dB) from the left input to each output for a tone at `f`, right input silent.
    fn from_left(strength: Strength, f: f64) -> (f64, f64) {
        let mut c = Crossfeed::new(strength, RATE);
        let n = RATE as usize;
        let mut s: Vec<f64> = (0..n)
            .flat_map(|i| [0.5 * (TAU * f * i as f64 / RATE as f64).sin(), 0.0])
            .collect();
        c.process(&mut s);
        let rms = |ch: usize| {
            let v: Vec<f64> = s.chunks(2).skip(n / 2).map(|f| f[ch]).collect();
            (v.iter().map(|x| x * x).sum::<f64>() / v.len() as f64).sqrt()
        };
        let reference = 0.5 / 2f64.sqrt();
        (
            20.0 * (rms(0) / reference).log10(),
            20.0 * (rms(1) / reference).log10(),
        )
    }

    #[test]
    fn low_frequencies_leak_across_high_ones_stay_put() {
        let (direct, cross) = from_left(Strength::Medium, 100.0);
        // r = −6 dB, g = r/(1+r): direct 1−g = 0.667 (−3.5 dB), cross g = 0.333 (−9.5 dB).
        assert!((direct + 3.5).abs() < 0.3, "{direct}");
        assert!((cross + 9.5).abs() < 0.3, "{cross}");
        // The cross-fed level is −6 dB under the direct one, as the preset says.
        assert!((direct - cross - 6.0).abs() < 0.3);
        let (direct, cross) = from_left(Strength::Medium, 8000.0);
        assert!(direct.abs() < 0.5, "{direct}");
        assert!(cross < -25.0, "{cross}");
        // Stronger feeds more.
        assert!(from_left(Strength::Strong, 100.0).1 > from_left(Strength::Light, 100.0).1);
    }

    #[test]
    fn mono_is_exactly_unchanged_and_peaks_never_grow() {
        let mut c = Crossfeed::new(Strength::Strong, RATE);
        let mono: Vec<f64> = (0..4800)
            .flat_map(|i| {
                let v = 0.9 * (TAU * 220.0 * i as f64 / RATE as f64).sin();
                [v, v]
            })
            .collect();
        let mut out = mono.clone();
        c.process(&mut out);
        assert_eq!(out, mono);
        // Wild stereo: full-scale opposite signs.
        let mut s: Vec<f64> = (0..48_000)
            .flat_map(|i| {
                let v = (TAU * 90.0 * i as f64 / RATE as f64).sin();
                [v, -v]
            })
            .collect();
        c.process(&mut s);
        assert!(s.iter().all(|v| v.abs() <= 1.0 + 1e-9));
        assert_eq!(Strength::parse("medium"), Some(Strength::Medium));
        assert_eq!(Strength::parse("off"), None);
    }
}
