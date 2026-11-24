//! The DSP graph (M2-01): what happens to samples between the decoder and the output, in f64.
//!
//! The graph holds only the stages a stream needs (channel mapping, rate conversion; later EQ,
//! ReplayGain, crossfeed…). When it holds none, samples go from the decoder to the output
//! unchanged: f32 represents every 16- and 24-bit sample exactly, so such a path is
//! bit-for-bit (see `passthrough_is_exact` in the engine tests).

use crate::resample::{map_channels, Quality, Resampler};

/// One processing step on interleaved f64 audio.
pub trait Stage: Send {
    /// Short name for the audio-info sheet ("resample 44100→48000 Hz").
    fn describe(&self) -> String;
    fn process(&mut self, input: &[f64], out: &mut Vec<f64>);
    /// End of the stream: emit anything held back (filter tails).
    fn flush(&mut self, _out: &mut Vec<f64>) {}
}

struct ChannelMap {
    src: usize,
    dst: usize,
}

impl Stage for ChannelMap {
    fn describe(&self) -> String {
        format!("channels {}→{}", self.src, self.dst)
    }
    fn process(&mut self, input: &[f64], out: &mut Vec<f64>) {
        map_channels(input, self.src, self.dst, out);
    }
}

struct Resample {
    from: u32,
    to: u32,
    quality: Quality,
    r: Resampler,
}

impl Stage for Resample {
    fn describe(&self) -> String {
        format!(
            "resample {}→{} Hz ({})",
            self.from,
            self.to,
            self.quality.name()
        )
    }
    fn process(&mut self, input: &[f64], out: &mut Vec<f64>) {
        self.r.process(input, out);
    }
    fn flush(&mut self, out: &mut Vec<f64>) {
        self.r.flush(out);
    }
}

/// The stages for one stream, in order.
pub struct Graph {
    stages: Vec<Box<dyn Stage>>,
    scratch: Vec<f64>,
}

impl Graph {
    /// The graph that turns `src_rate`/`src_channels` audio into what the output takes.
    pub fn new(
        src_rate: u32,
        src_channels: usize,
        out_rate: u32,
        out_channels: usize,
        quality: Quality,
    ) -> Graph {
        let mut stages: Vec<Box<dyn Stage>> = Vec::new();
        if src_channels != out_channels {
            stages.push(Box::new(ChannelMap {
                src: src_channels,
                dst: out_channels,
            }));
        }
        if src_rate != out_rate {
            stages.push(Box::new(Resample {
                from: src_rate,
                to: out_rate,
                quality,
                r: Resampler::new(src_rate, out_rate, out_channels, quality),
            }));
        }
        Graph {
            stages,
            scratch: Vec::new(),
        }
    }

    /// Samples pass through unchanged.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn passthrough(&self) -> bool {
        self.stages.is_empty()
    }

    /// The stages, for display.
    pub fn describe(&self) -> Vec<String> {
        self.stages.iter().map(|s| s.describe()).collect()
    }

    /// End of the stream: flushes each stage in order (a stage's tail passes through the rest).
    pub fn flush(&mut self, out: &mut Vec<f64>) {
        let n = self.stages.len();
        for k in 0..n {
            let mut tail = Vec::new();
            self.stages[k].flush(&mut tail);
            let mut cur = tail;
            for stage in self.stages[k + 1..].iter_mut() {
                let mut next = Vec::new();
                stage.process(&cur, &mut next);
                cur = next;
            }
            out.extend_from_slice(&cur);
        }
    }

    /// Runs `input` through every stage, appending the result to `out`.
    pub fn process(&mut self, input: &[f64], out: &mut Vec<f64>) {
        match self.stages.len() {
            0 => out.extend_from_slice(input),
            1 => self.stages[0].process(input, out),
            n => {
                let mut cur = input.to_vec();
                for (k, stage) in self.stages.iter_mut().enumerate() {
                    self.scratch.clear();
                    stage.process(&cur, &mut self.scratch);
                    if k + 1 == n {
                        out.extend_from_slice(&self.scratch);
                    } else {
                        std::mem::swap(&mut cur, &mut self.scratch);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_format_is_passthrough() {
        let mut g = Graph::new(48000, 2, 48000, 2, Quality::Best);
        assert!(g.passthrough());
        let input = [0.25, -0.5, 1.0, -1.0];
        let mut out = Vec::new();
        g.process(&input, &mut out);
        assert_eq!(out, input);
    }

    #[test]
    fn mono_44k_to_stereo_48k() {
        let mut g = Graph::new(44100, 1, 48000, 2, Quality::Best);
        assert!(!g.passthrough());
        assert_eq!(
            g.describe(),
            ["channels 1→2", "resample 44100→48000 Hz (best)"]
        );
        let input = vec![0.5; 44100];
        let mut out = Vec::new();
        g.process(&input, &mut out);
        g.flush(&mut out);
        assert_eq!(out.len() / 2, 48000);
    }
}
