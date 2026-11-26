//! Transitions between tracks (M2-15): the last seconds of the outgoing track are mixed into the
//! start of the next one, with an effect on the outgoing side.
//!
//! The engine holds back the final `n` frames of a track (the *tail*) instead of sending them
//! on. When the next track is known, `Blend::new` prepares the outgoing side (fade, speed or
//! pitch drop, low-pass sweep, echo) and `apply` adds it to the first `n` frames of the new
//! track as they are produced, fading those in. Everything is interleaved f32 at the output
//! rate; the new track's samples are only ever scaled, so nothing is lost when it is quiet.

use std::f64::consts::{FRAC_PI_2, PI};

/// How the outgoing track leaves.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Effect {
    /// Equal-power fade.
    Crossfade,
    /// The outgoing track gets duller (low-pass sweeping down) while it fades.
    Muffled,
    /// The outgoing track fades into echoes.
    Echo,
    /// The outgoing track slows down like a stopping record.
    Pitch,
    /// Your own mix of the above.
    Custom(Custom),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Custom {
    /// Playback rate the outgoing track slows or speeds to by the end of the transition
    /// (1 = unchanged); the pitch follows.
    pub speed: f64,
    /// Extra pitch shift (semitones) reached by the end.
    pub semitones: f64,
    pub echo: bool,
    pub low_pass: bool,
}

impl Effect {
    pub fn parse(s: &str, custom: Custom) -> Option<Effect> {
        match s {
            "crossfade" => Some(Effect::Crossfade),
            "muffled" => Some(Effect::Muffled),
            "echo" => Some(Effect::Echo),
            "pitch" => Some(Effect::Pitch),
            "custom" => Some(Effect::Custom(custom)),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Effect::Crossfade => "crossfade",
            Effect::Muffled => "muffled",
            Effect::Echo => "echo",
            Effect::Pitch => "pitch",
            Effect::Custom(_) => "custom",
        }
    }

    fn plan(self) -> Plan {
        let none = Plan {
            rate_end: 1.0,
            low_pass: false,
            echo: false,
        };
        match self {
            Effect::Crossfade => none,
            Effect::Muffled => Plan {
                low_pass: true,
                ..none
            },
            Effect::Echo => Plan { echo: true, ..none },
            Effect::Pitch => Plan {
                rate_end: 0.5,
                ..none
            },
            Effect::Custom(c) => Plan {
                rate_end: (c.speed * 2f64.powf(c.semitones / 12.0)).clamp(0.25, 2.0),
                low_pass: c.low_pass,
                echo: c.echo,
            },
        }
    }
}

struct Plan {
    rate_end: f64,
    low_pass: bool,
    echo: bool,
}

/// Picks an effect for "automatic": a track that ends loud gets the muffled exit (a hard stop
/// would be obvious), one that has already faded out gets a plain crossfade.
pub fn auto_effect(tail: &[f32], ch: usize, rate: u32) -> Effect {
    let frames = (rate as usize / 2).min(tail.len() / ch.max(1));
    if frames == 0 {
        return Effect::Crossfade;
    }
    let last = &tail[tail.len() - frames * ch..];
    let rms = (last.iter().map(|&v| (v as f64).powi(2)).sum::<f64>() / last.len() as f64).sqrt();
    if rms > 0.1 {
        Effect::Muffled
    } else {
        Effect::Crossfade
    }
}

/// How the two tracks' beats line up.
pub struct Alignment {
    /// The outgoing end, time-stretched (pitch kept) to the new track's tempo.
    pub tail: Vec<f32>,
    /// Frames of the stretched tail that play alone before the new track comes in, so that its
    /// beats fall on the outgoing ones.
    pub lead: usize,
    pub from_bpm: f64,
    pub to_bpm: f64,
}

/// Beat-matches the outgoing `tail` (interleaved, `ch` channels, `rate`) to the start of the
/// next track (`head`, mono at `head_rate`). Returns None when either has no clear beat or
/// the tempos are more than 8% apart (after allowing for double and half time): then a plain
/// blend is better than a stretch that would be heard.
pub fn align(
    tail: &[f32],
    ch: usize,
    rate: u32,
    head: &[f32],
    head_rate: u32,
) -> Option<Alignment> {
    let ch = ch.max(1);
    let mono: Vec<f32> = tail
        .chunks(ch)
        .map(|f| f.iter().sum::<f32>() / ch as f32)
        .collect();
    let a = crate::beat::analyze(&mono, rate)?;
    let b = crate::beat::analyze(head, head_rate)?;
    // The factor that brings the outgoing tempo to the new one, folded into one octave.
    let mut tempo = b.bpm() / a.bpm();
    while tempo > 1.41 {
        tempo /= 2.0;
    }
    while tempo < 0.71 {
        tempo *= 2.0;
    }
    if (tempo - 1.0).abs() > 0.08 {
        return None;
    }
    let stretched: Vec<f32> = if (tempo - 1.0).abs() < 0.0005 {
        tail.to_vec()
    } else {
        let mut w = crate::stretch::Wsola::new(tempo, rate, ch);
        let input: Vec<f64> = tail.iter().map(|&v| v as f64).collect();
        let mut out = Vec::with_capacity(input.len());
        w.process(&input, &mut out);
        w.flush(&mut out);
        out.iter().map(|&v| v as f32).collect()
    };
    // Where the stretched tail's beats fall, and the common period to line the phases up on.
    let tail_period = a.period / tempo;
    let tail_first = a.first / tempo;
    // With double or half time one track's beat is a multiple of the other's: line up on the
    // longer period.
    let common = tail_period.max(b.period);
    let lead = (tail_first - b.first).rem_euclid(common);
    let lead_frames = (lead * rate as f64).round() as usize;
    if lead_frames > stretched.len() / ch / 2 {
        return None;
    }
    Some(Alignment {
        tail: stretched,
        lead: lead_frames,
        from_bpm: a.bpm(),
        to_bpm: b.bpm(),
    })
}

/// A transition in progress.
pub struct Blend {
    ch: usize,
    /// The outgoing side, already faded: interleaved, `frames` long.
    out: Vec<f32>,
    frames: usize,
    /// Frames mixed so far.
    pos: usize,
}

fn soft_clip(x: f32) -> f32 {
    const KNEE: f32 = 0.95;
    let a = x.abs();
    if a <= KNEE {
        x
    } else {
        x.signum() * (KNEE + (1.0 - KNEE) * ((a - KNEE) / (1.0 - KNEE)).tanh())
    }
}

impl Blend {
    pub fn new(effect: Effect, tail: &[f32], ch: usize, rate: u32) -> Blend {
        let ch = ch.max(1);
        let frames = tail.len() / ch;
        let plan = effect.plan();
        let n = frames.max(1) as f64;
        let mut out = vec![0f32; frames * ch];

        // 1. Speed / pitch: read the tail at a rate sliding from 1 to `rate_end`.
        let mut src: Vec<f64> = tail.iter().map(|&v| v as f64).collect();
        if (plan.rate_end - 1.0).abs() > 1e-6 {
            let mut p = 0.0f64;
            let mut moved = vec![0f64; frames * ch];
            for i in 0..frames {
                let t = i as f64 / n;
                let k = p.floor() as usize;
                let f = p - k as f64;
                for c in 0..ch {
                    let a = if k < frames { src[k * ch + c] } else { 0.0 };
                    let b = if k + 1 < frames {
                        src[(k + 1) * ch + c]
                    } else {
                        0.0
                    };
                    moved[i * ch + c] = a + (b - a) * f;
                }
                p += 1.0 + (plan.rate_end - 1.0) * t;
            }
            src = moved;
        }

        // 2. Low-pass sweeping from open to muffled (two one-poles per channel).
        if plan.low_pass {
            let mut s1 = vec![0f64; ch];
            let mut s2 = vec![0f64; ch];
            for i in 0..frames {
                let t = i as f64 / n;
                let fc = 18_000f64 * (250.0f64 / 18_000.0).powf(t);
                let fc = fc.min(rate as f64 * 0.45);
                let a = 1.0 - (-2.0 * PI * fc / rate as f64).exp();
                for c in 0..ch {
                    s1[c] += a * (src[i * ch + c] - s1[c]);
                    s2[c] += a * (s1[c] - s2[c]);
                    src[i * ch + c] = s2[c];
                }
            }
        }

        // 3. Echo: the signal itself fades in the first half; its repeats ring on.
        if plan.echo {
            let d = ((rate as f64 * 0.3) as usize).max(1);
            let mut e = vec![0f64; frames * ch];
            for i in 0..frames {
                let t = i as f64 / n;
                let drive = (1.0 - 2.0 * t).clamp(0.0, 1.0);
                for c in 0..ch {
                    let fb = if i >= d {
                        0.55 * e[(i - d) * ch + c]
                    } else {
                        0.0
                    };
                    e[i * ch + c] = src[i * ch + c] * drive + fb;
                }
            }
            src = e;
        }

        // 4. The fade-out.
        for i in 0..frames {
            let t = i as f64 / n;
            let g = if plan.echo {
                (1.0 - t).powi(2)
            } else {
                (FRAC_PI_2 * t).cos()
            };
            for c in 0..ch {
                out[i * ch + c] = (src[i * ch + c] * g) as f32;
            }
        }
        Blend {
            ch,
            out,
            frames,
            pos: 0,
        }
    }

    pub fn frames(&self) -> usize {
        self.frames
    }

    pub fn done(&self) -> bool {
        self.pos >= self.frames
    }

    /// Mixes the next stretch of the new track (interleaved) with the outgoing side, in place.
    pub fn apply(&mut self, block: &mut [f32]) {
        let ch = self.ch;
        let n = self.frames.max(1) as f64;
        for frame in block.chunks_mut(ch) {
            if self.pos >= self.frames || frame.len() < ch {
                break;
            }
            let g_in = (FRAC_PI_2 * (self.pos as f64 / n)).sin() as f32;
            for (c, x) in frame.iter_mut().enumerate() {
                *x = soft_clip(*x * g_in + self.out[self.pos * ch + c]);
            }
            self.pos += 1;
        }
    }

    /// The outgoing side that no new sample was there to mix with (the new track was shorter
    /// than the transition).
    pub fn rest(&mut self) -> Vec<f32> {
        let r = self.out[self.pos * self.ch..].to_vec();
        self.pos = self.frames;
        r
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 48_000;

    fn tone(f: f64, secs: f64, amp: f64) -> Vec<f32> {
        (0..(RATE as f64 * secs) as usize)
            .flat_map(|i| {
                let v = (amp * (2.0 * PI * f * i as f64 / RATE as f64).sin()) as f32;
                [v, v]
            })
            .collect()
    }

    fn rms(v: &[f32]) -> f64 {
        (v.iter().map(|&x| (x as f64).powi(2)).sum::<f64>() / v.len().max(1) as f64).sqrt()
    }

    fn blend(effect: Effect, tail: &[f32], head: &[f32]) -> Vec<f32> {
        let mut b = Blend::new(effect, tail, 2, RATE);
        let mut out = head.to_vec();
        for c in out.chunks_mut(1000) {
            b.apply(c);
        }
        out
    }

    #[test]
    fn a_crossfade_keeps_the_power_and_ends_on_the_new_track() {
        let tail = tone(440.0, 4.0, 0.5);
        let head = tone(880.0, 6.0, 0.5);
        let y = blend(Effect::Crossfade, &tail, &head);
        // Start: the outgoing track at full level; end of the blend: the new one alone.
        let n = tail.len();
        assert!(rms(&y[..2000]) > 0.3, "{}", rms(&y[..2000]));
        assert_eq!(y[n..], head[n..]);
        // Two unrelated tones keep their combined power within 1 dB all the way through.
        let win = 4800 * 2;
        for k in 1..(n / win - 1) {
            let level = rms(&y[k * win..(k + 1) * win]) / (0.5 / 2f64.sqrt());
            assert!(
                (20.0 * level.log10()).abs() < 1.0,
                "window {k}: {:.2} dB",
                20.0 * level.log10()
            );
        }
    }

    #[test]
    fn muffled_removes_the_highs_of_the_outgoing_track() {
        let tail = tone(6000.0, 4.0, 0.5);
        let silence = vec![0f32; tail.len()];
        let y = blend(Effect::Muffled, &tail, &silence);
        // The high tone dies faster than a plain fade does.
        let plain = blend(Effect::Crossfade, &tail, &silence);
        let w = tail.len() / 2; // half way: the cut-off is near 2 kHz
        let (a, b) = (rms(&y[w..w + 9600]), rms(&plain[w..w + 9600]));
        assert!(a < b * 0.5, "{a} vs {b}");
        // Low tones come through the filter at the start.
        let low = tone(200.0, 4.0, 0.5);
        let yl = blend(Effect::Muffled, &low, &silence);
        assert!(rms(&yl[..9600]) > 0.3);
    }

    #[test]
    fn pitch_slows_the_outgoing_track_down() {
        let tail = tone(1000.0, 4.0, 0.5);
        let silence = vec![0f32; tail.len()];
        let mut b = Blend::new(Effect::Pitch, &tail, 2, RATE);
        let mut y = silence.clone();
        b.apply(&mut y);
        let freq = |x: &[f32]| {
            let mono: Vec<f64> = x.chunks(2).map(|f| f[0] as f64).collect();
            let mut up = Vec::new();
            for i in 1..mono.len() {
                if mono[i - 1] < 0.0 && mono[i] >= 0.0 {
                    up.push(i);
                }
            }
            (up.len() - 1) as f64 * RATE as f64 / (up[up.len() - 1] - up[0]) as f64
        };
        let early = freq(&y[..2 * 12_000]);
        let late = freq(&y[2 * 120_000..2 * 150_000]);
        assert!((early / 1000.0 - 1.0).abs() < 0.1, "{early}");
        assert!(late < early * 0.8, "{late} vs {early}");
    }

    #[test]
    fn echo_rings_on_after_the_signal_has_faded() {
        // A burst at the start of the tail, then nothing.
        let mut tail = tone(500.0, 0.2, 0.6);
        tail.extend(vec![0f32; 2 * RATE as usize * 4]);
        let silence = vec![0f32; tail.len()];
        let y = blend(Effect::Echo, &tail, &silence);
        // 0.3-0.6 s: after the burst, the repeat is there.
        let a = (RATE as f64 * 0.35) as usize * 2;
        assert!(rms(&y[a..a + 4800]) > 0.02, "{}", rms(&y[a..a + 4800]));
        // A plain crossfade has nothing there.
        let plain = blend(Effect::Crossfade, &tail, &silence);
        assert!(rms(&plain[a..a + 4800]) < 1e-6);
    }

    #[test]
    fn loud_overlaps_never_clip_and_a_short_new_track_hands_back_the_rest() {
        let tail = tone(440.0, 2.0, 1.0);
        let head = tone(440.0, 2.0, 1.0);
        let y = blend(Effect::Crossfade, &tail, &head);
        assert!(y.iter().all(|v| v.abs() <= 1.0));
        let mut b = Blend::new(Effect::Crossfade, &tail, 2, RATE);
        let mut short = vec![0.1f32; 2 * 1000];
        b.apply(&mut short);
        assert!(!b.done());
        let rest = b.rest();
        assert_eq!(rest.len(), tail.len() - 2000);
        assert!(b.done());
    }

    fn drums_stereo(bpm: f64, offset: f64, secs: f64) -> Vec<f32> {
        crate::beat::tests::drums_at(RATE, bpm, offset, secs)
            .iter()
            .flat_map(|&v| [v, v])
            .collect()
    }

    #[test]
    fn beats_line_up_within_20_ms_after_the_tempo_match() {
        for (bpm_out, off_out, bpm_in, off_in) in [
            (120.0, 0.13, 126.0, 0.31),
            (100.0, 0.05, 96.0, 0.2),
            (128.0, 0.4, 128.0, 0.0),
            (90.0, 0.2, 178.0, 0.1), // double time: 90 against 178
        ] {
            let tail = drums_stereo(bpm_out, off_out, 6.0);
            let head: Vec<f32> = crate::beat::tests::drums_at(RATE, bpm_in, off_in, 6.0);
            let al = align(&tail, 2, RATE, &head, RATE)
                .unwrap_or_else(|| panic!("{bpm_out} to {bpm_in}: no alignment"));
            // The stretched tail is as long as the tempo change says.
            let tempo = {
                let mut t = bpm_in / bpm_out;
                while t > 1.41 {
                    t /= 2.0;
                }
                while t < 0.71 {
                    t *= 2.0;
                }
                t
            };
            let want = tail.len() as f64 / 2.0 / tempo;
            // (The detected tempos stand in for the nominal ones: within 0.3%.)
            assert!(
                (al.tail.len() as f64 / 2.0 / want - 1.0).abs() < 0.003,
                "{} vs {want}",
                al.tail.len() / 2
            );
            // Where the outgoing beats really fall in the stretched audio (measured, not
            // calculated) against where the new track's beats fall once it starts `lead` in.
            let mono: Vec<f32> = al.tail.chunks(2).map(|f| f[0]).collect();
            let grid = crate::beat::analyze(&mono, RATE).expect("beats survive the stretch");
            let period_in = 60.0 / bpm_in;
            let period = grid.period.max(period_in);
            let lead = al.lead as f64 / RATE as f64;
            // At double time only every other beat of the faster track has a partner (which
            // of the two is a coin toss in the music itself: the half-time grid can sit on
            // either).
            let step = (period / period_in).round().max(1.0) as usize;
            let miss = |j: usize| {
                let t = lead + off_in + j as f64 * period_in;
                let k = ((t - grid.first) / grid.period).round();
                (t - (grid.first + k * grid.period)).abs()
            };
            let ok = (0..step).any(|r| (r..8).step_by(step).all(|j| miss(j) < 0.020));
            assert!(
                ok,
                "{bpm_out}→{bpm_in}: misses {:?} ms (period {period:.3})",
                (0..8)
                    .map(|j| (miss(j) * 1000.0).round())
                    .collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn no_beat_match_when_tempos_are_far_apart_or_there_is_no_pulse() {
        let tail = drums_stereo(120.0, 0.0, 6.0);
        let slow = crate::beat::tests::drums_at(RATE, 84.0, 0.0, 6.0); // 1.43× apart
        assert!(align(&tail, 2, RATE, &slow, RATE).is_none());
        let flat = vec![0f32; RATE as usize * 6];
        assert!(align(&tail, 2, RATE, &flat, RATE).is_none());
    }

    #[test]
    fn auto_picks_by_how_the_track_ends() {
        let loud = tone(200.0, 2.0, 0.6);
        let quiet = tone(200.0, 2.0, 0.01);
        assert_eq!(auto_effect(&loud, 2, RATE), Effect::Muffled);
        assert_eq!(auto_effect(&quiet, 2, RATE), Effect::Crossfade);
    }
}
