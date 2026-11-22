//! Small streaming linear resampler plus channel mapping (f64), used as stages of the DSP graph
//! (`dsp.rs`) when the output can't run at the file's own rate or channel count. M2-07 replaces
//! the linear resampler with a high-quality one.

pub struct Resampler {
    channels: usize,
    /// Input frames consumed per output frame.
    ratio: f64,
    pos: f64,
    buf: Vec<f64>,
}

impl Resampler {
    pub fn new(src_rate: u32, dst_rate: u32, channels: usize) -> Resampler {
        Resampler {
            channels,
            ratio: src_rate as f64 / dst_rate as f64,
            pos: 0.0,
            buf: Vec::new(),
        }
    }

    pub fn is_passthrough(&self) -> bool {
        (self.ratio - 1.0).abs() < f64::EPSILON
    }

    pub fn process(&mut self, input: &[f64], out: &mut Vec<f64>) {
        if self.is_passthrough() {
            out.extend_from_slice(input);
            return;
        }
        let ch = self.channels;
        self.buf.extend_from_slice(input);
        let frames = self.buf.len() / ch;
        while self.pos + 1.0 < frames as f64 {
            let i = self.pos as usize;
            let frac = self.pos - i as f64;
            for c in 0..ch {
                let a = self.buf[i * ch + c];
                let b = self.buf[(i + 1) * ch + c];
                out.push(a + (b - a) * frac);
            }
            self.pos += self.ratio;
        }
        let consumed = (self.pos as usize).min(frames.saturating_sub(1));
        self.buf.drain(..consumed * ch);
        self.pos -= consumed as f64;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resample_length() {
        let mut r = Resampler::new(44100, 48000, 2);
        let input = vec![0.5f64; 44100 * 2];
        let mut out = Vec::new();
        r.process(&input, &mut out);
        let frames = out.len() / 2;
        assert!((frames as i64 - 48000).abs() < 5, "{frames}");
        assert!(out.iter().all(|v| (*v - 0.5).abs() < 1e-6));
    }

    #[test]
    fn mono_to_stereo() {
        let mut out = Vec::new();
        map_channels(&[0.1, 0.2], 1, 2, &mut out);
        assert_eq!(out, vec![0.1, 0.1, 0.2, 0.2]);
    }
}
