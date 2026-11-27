//! Music haptics (M2-19): turns the beat and onsets of a song into a vibration plan.
//!
//! The song is analysed ahead of playback (`analyze`, on a decimated mono mix) into one
//! amplitude per 20 ms. The player then hands the vibrator the slice that is about to be heard
//! (`Timeline::chunk`). An onset (a drum hit, a plucked note, a bass note starting) gives a short
//! strong pulse that dies away in about 60 ms; sustained sound gives nothing, so the phone
//! pulses with the rhythm instead of buzzing with the loudness.

/// One plan entry covers this many milliseconds.
pub const FRAME_MS: u32 = 20;

/// Vibration amplitudes (0 = off, 255 = the strongest) per `FRAME_MS`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Timeline {
    pub amps: Vec<u8>,
}

impl Timeline {
    pub fn is_empty(&self) -> bool {
        self.amps.iter().all(|&a| a == 0)
    }

    /// The plan from `from_secs` for `len_ms`, as (duration ms, amplitude) steps with equal
    /// neighbours merged: what `VibrationEffect.createWaveform` takes. Past the end it is quiet.
    pub fn chunk(&self, from_secs: f64, len_ms: u32) -> Vec<(u32, u8)> {
        let first = (from_secs.max(0.0) * 1000.0 / FRAME_MS as f64).floor() as usize;
        let frames = len_ms.div_ceil(FRAME_MS) as usize;
        let mut out: Vec<(u32, u8)> = Vec::new();
        for i in first..first + frames {
            let a = self.amps.get(i).copied().unwrap_or(0);
            match out.last_mut() {
                Some((ms, last)) if *last == a => *ms += FRAME_MS,
                _ => out.push((FRAME_MS, a)),
            }
        }
        out
    }
}

/// `x` is mono at `rate` (about 8–16 kHz is plenty); `sensitivity` and `strength` are 0..1:
/// how small a hit still pulses, and how hard the pulses are.
pub fn analyze(x: &[f32], rate: u32, sensitivity: f64, strength: f64) -> Timeline {
    let hop = ((rate / 100) as usize).max(1); // 10 ms
    let hop_secs = hop as f64 / rate as f64;
    let sensitivity = sensitivity.clamp(0.0, 1.0);
    let strength = strength.clamp(0.0, 1.0);

    // Bass (one-pole low-pass near 150 Hz) and the sharp edges (first difference).
    let a = 1.0 - (-2.0 * std::f64::consts::PI * 150.0 / rate as f64).exp();
    let mut lp = 0.0f64;
    let (mut bass_log, mut edge_log) = (Vec::new(), Vec::new());
    let mut prev = 0.0f64;
    for block in x.chunks(hop) {
        let (mut eb, mut ee) = (0.0, 0.0);
        for &s in block {
            let s = s as f64;
            lp += a * (s - lp);
            eb += lp * lp;
            let d = s - prev;
            prev = s;
            ee += d * d;
        }
        let n = block.len().max(1) as f64;
        bass_log.push((1.0 + 1e4 * eb / n).ln());
        edge_log.push((1.0 + 1e4 * ee / n).ln());
    }
    let rise = |v: &[f64]| -> Vec<f64> {
        (0..v.len())
            .map(|i| {
                if i == 0 {
                    0.0
                } else {
                    (v[i] - v[i - 1]).max(0.0)
                }
            })
            .collect()
    };
    let (rb, re) = (rise(&bass_log), rise(&edge_log));

    // Against what this song has been doing lately (so quiet and loud songs both pulse), with a
    // floor so silence isn't blown up. Then a gate from the sensitivity, and the decay.
    let ref_decay = (-hop_secs / 3.0).exp();
    let pulse_decay = (-hop_secs / 0.06).exp();
    let gate = 0.75 - 0.7 * sensitivity;
    let gain = 0.25 + 0.75 * strength;
    let mut reference = 0.5f64;
    let mut env = 0.0f64;
    let mut per_hop = Vec::with_capacity(rb.len());
    for i in 0..rb.len() {
        let f = 0.6 * rb[i] + 0.4 * re[i];
        reference = (reference * ref_decay).max(f).max(0.5);
        let x = (f / reference).min(1.0);
        let hit = ((x - gate) / (1.0 - gate)).max(0.0);
        env = (env * pulse_decay).max(hit * gain);
        per_hop.push(env);
    }
    // Two hops per plan entry; tiny amplitudes are below what a motor can do.
    let amps = per_hop
        .chunks(2)
        .map(|p| {
            let m = p.iter().cloned().fold(0.0, f64::max);
            let v = (m * 255.0).round().min(255.0) as u8;
            if v < 12 {
                0
            } else {
                v
            }
        })
        .collect();
    Timeline { amps }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 11_025;

    /// Bursts of noise at `times` (seconds) with the given amplitudes, over a quiet hum.
    fn hits(times: &[(f64, f32)], secs: f64) -> Vec<f32> {
        let n = (RATE as f64 * secs) as usize;
        let mut x = vec![0f32; n];
        let mut seed = 99u32;
        for &(t, amp) in times {
            let start = (t * RATE as f64) as usize;
            for i in 0..(RATE as usize / 15) {
                if start + i >= n {
                    break;
                }
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                let noise = (seed >> 8) as f32 / (1u32 << 24) as f32 * 2.0 - 1.0;
                x[start + i] += amp * noise * (-(i as f32) / (RATE as f32 * 0.012)).exp();
            }
        }
        for (i, v) in x.iter_mut().enumerate() {
            *v += 0.01 * (i as f32 * 80.0 * std::f32::consts::TAU / RATE as f32).sin();
        }
        x
    }

    fn peak_between(t: &Timeline, from: f64, to: f64) -> u8 {
        let a = (from * 1000.0 / FRAME_MS as f64) as usize;
        let b = (to * 1000.0 / FRAME_MS as f64) as usize;
        t.amps[a..b.min(t.amps.len())]
            .iter()
            .copied()
            .max()
            .unwrap_or(0)
    }

    #[test]
    fn a_beat_gives_a_pulse_right_then_and_quiet_between_beats() {
        let beats: Vec<(f64, f32)> = (0..12).map(|k| (0.25 + k as f64 * 0.5, 0.7)).collect();
        let t = analyze(&hits(&beats, 7.0), RATE, 0.5, 0.5);
        for &(at, _) in beats.iter().skip(2) {
            // The pulse starts within 30 ms of the beat...
            assert!(peak_between(&t, at, at + 0.06) > 80, "beat at {at}");
            // ...and has died away well before the next one.
            assert_eq!(peak_between(&t, at + 0.25, at + 0.45), 0, "after {at}");
        }
    }

    #[test]
    fn silence_and_steady_sound_do_not_pulse() {
        assert!(analyze(&vec![0.0; RATE as usize * 4], RATE, 1.0, 1.0).is_empty());
        let hum: Vec<f32> = (0..RATE as usize * 6)
            .map(|i| 0.4 * (i as f32 * 220.0 * std::f32::consts::TAU / RATE as f32).sin())
            .collect();
        let t = analyze(&hum, RATE, 0.5, 0.5);
        // (The very start of the tone is an onset, nothing after it is.)
        assert_eq!(peak_between(&t, 1.0, 6.0), 0);
    }

    #[test]
    fn sensitivity_decides_whether_weak_hits_count_and_strength_how_hard() {
        // Strong hits every second, weak ones half-way between.
        let mut times = Vec::new();
        for k in 0..6 {
            times.push((0.5 + k as f64, 0.8));
            times.push((1.0 + k as f64, 0.22));
        }
        let x = hits(&times, 7.0);
        let weak_peak = |sens| {
            let t = analyze(&x, RATE, sens, 0.5);
            (2..5)
                .map(|k| peak_between(&t, 1.0 + k as f64, 1.0 + k as f64 + 0.08))
                .max()
                .unwrap()
        };
        assert_eq!(weak_peak(0.0), 0, "a low sensitivity ignores the weak hits");
        assert!(weak_peak(1.0) > 0, "a high one feels them");
        let strong = |strength| {
            let t = analyze(&x, RATE, 0.5, strength);
            peak_between(&t, 3.5, 3.58)
        };
        assert!(
            strong(1.0) > strong(0.2) + 60,
            "{} vs {}",
            strong(1.0),
            strong(0.2)
        );
    }

    #[test]
    fn chunks_merge_equal_steps_and_go_quiet_past_the_end() {
        let t = Timeline {
            amps: vec![0, 0, 200, 200, 100, 0],
        };
        // From 20 ms: frames 1.. = 0, 200, 200, 100, 0, then past the end.
        assert_eq!(
            t.chunk(0.020, 140),
            vec![(20, 0), (40, 200), (20, 100), (60, 0)]
        );
        assert_eq!(t.chunk(10.0, 60), vec![(60, 0)]);
        assert!(!t.is_empty());
    }
}
