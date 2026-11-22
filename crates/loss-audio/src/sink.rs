//! Outputs (M2-01). The engine decodes into a ring buffer; a [`Sink`] opens a stream that pulls
//! from it. [`CpalSink`] is the real one (AAudio on Android, ALSA/Pulse on Linux). [`FakeSink`]
//! is driven by tests, which advance its clock by exact frame counts.
//!
//! A sink reports what it accepts ([`Caps`]); [`choose_rate`] picks the output rate.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// Where the decoder's samples wait for the output (interleaved f32).
pub type Slot = Arc<Mutex<Option<rtrb::Consumer<f32>>>>;

/// What an output accepts.
#[derive(Debug, Clone, PartialEq)]
pub struct Caps {
    /// Sample rates it runs at natively. Empty with `any_rate` means "anything" (the system
    /// mixer converts, e.g. Android's shared AAudio).
    pub rates: Vec<u32>,
    pub any_rate: bool,
    pub channels: usize,
}

/// Rates worth trying, in Hz.
pub const COMMON_RATES: &[u32] = &[
    8000, 11025, 16000, 22050, 32000, 44100, 48000, 88200, 96000, 176400, 192000, 352800, 384000,
];

/// The output rate for a source at `src` Hz:
/// 1. the source's own rate, if the output takes it;
/// 2. else a rate an integer ratio away (192 kHz → 96 or 48, 88.2 → 44.1), highest first;
/// 3. else the highest rate the output takes (44.1 kHz → 48 kHz on the Beats Studio Pro).
pub fn choose_rate(src: u32, caps: &Caps) -> u32 {
    if caps.any_rate || caps.rates.is_empty() || caps.rates.contains(&src) {
        return src;
    }
    let mut rates = caps.rates.clone();
    rates.sort_unstable_by(|a, b| b.cmp(a));
    rates
        .iter()
        .copied()
        .find(|&r| (r > src && r.is_multiple_of(src)) || (r < src && src.is_multiple_of(r)))
        .unwrap_or(rates[0])
}

/// Pauses and resumes a stream.
pub trait Control {
    fn play(&self);
    fn pause(&self);
}

/// An open output stream.
pub struct Stream {
    pub slot: Slot,
    /// Samples (not frames) the output has taken from the ring.
    pub played: Arc<AtomicU64>,
    /// Frames between the ring and the listener's ears (device buffer and mixer).
    pub latency_frames: Arc<AtomicU64>,
    pub rate: u32,
    pub channels: usize,
    control: Box<dyn Control>,
}

impl Stream {
    pub fn play(&self) {
        self.control.play();
    }

    pub fn pause(&self) {
        self.control.pause();
    }

    /// Frames the listener has actually heard since `played` was last reset.
    pub fn heard_frames(&self) -> u64 {
        let frames = self.played.load(Ordering::Relaxed) / self.channels.max(1) as u64;
        frames.saturating_sub(self.latency_frames.load(Ordering::Relaxed))
    }
}

/// Something that can play: the phone's audio output, or a test double.
pub trait Sink: Send {
    fn caps(&mut self) -> Result<Caps, String>;
    /// Opens a stream at `rate` that applies `volume` (f32 bits, 0..1).
    fn open(&mut self, rate: u32, volume: Arc<AtomicU32>) -> Result<Stream, String>;
}

/// Pulls one buffer's worth from the ring, applying the volume. Returns samples taken.
fn fill<T: cpal::SizedSample + cpal::FromSample<f32>>(
    data: &mut [T],
    slot: &Slot,
    volume: &AtomicU32,
) -> u64 {
    let vol = f32::from_bits(volume.load(Ordering::Relaxed));
    let mut popped = 0u64;
    match slot.try_lock() {
        Ok(mut guard) => match guard.as_mut() {
            Some(cons) => {
                for s in data.iter_mut() {
                    let v = match cons.pop() {
                        Ok(v) => {
                            popped += 1;
                            v
                        }
                        Err(_) => 0.0,
                    };
                    // Unity volume leaves samples exactly as decoded.
                    *s = T::from_sample(if vol == 1.0 { v } else { v * vol });
                }
            }
            None => data.iter_mut().for_each(|s| *s = T::from_sample(0.0)),
        },
        Err(_) => data.iter_mut().for_each(|s| *s = T::from_sample(0.0)),
    }
    popped
}

// ---- cpal ----

/// The system's default audio output through cpal (AAudio on Android).
#[derive(Default)]
pub struct CpalSink;

struct CpalControl(cpal::Stream);

impl Control for CpalControl {
    fn play(&self) {
        let _ = self.0.play();
    }
    fn pause(&self) {
        let _ = self.0.pause();
    }
}

fn device() -> Result<cpal::Device, String> {
    cpal::default_host()
        .default_output_device()
        .ok_or_else(|| "no audio output device".to_string())
}

fn pick_config(
    device: &cpal::Device,
    want_rate: u32,
) -> Result<cpal::SupportedStreamConfig, String> {
    let default = device.default_output_config().map_err(|e| e.to_string())?;
    let want_ch = default.channels().clamp(1, 2);
    if let Ok(configs) = device.supported_output_configs() {
        let mut best: Option<cpal::SupportedStreamConfig> = None;
        for c in configs {
            if c.channels() != want_ch {
                continue;
            }
            let rate = cpal::SampleRate(want_rate);
            if c.min_sample_rate() <= rate && rate <= c.max_sample_rate() {
                let cand = c.with_sample_rate(rate);
                // Prefer float output, then 16-bit.
                let better = match &best {
                    None => true,
                    Some(b) => {
                        b.sample_format() != cpal::SampleFormat::F32
                            && cand.sample_format() == cpal::SampleFormat::F32
                    }
                };
                if better {
                    best = Some(cand);
                }
            }
        }
        if let Some(b) = best {
            return Ok(b);
        }
    }
    Ok(default)
}

fn build_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    slot: Slot,
    played: Arc<AtomicU64>,
    latency: Arc<AtomicU64>,
    volume: Arc<AtomicU32>,
) -> Result<cpal::Stream, String>
where
    T: cpal::SizedSample + cpal::FromSample<f32>,
{
    let rate = config.sample_rate.0 as f64;
    device
        .build_output_stream(
            config,
            move |data: &mut [T], info: &cpal::OutputCallbackInfo| {
                let popped = fill(data, &slot, &volume);
                played.fetch_add(popped, Ordering::Relaxed);
                // How far ahead of the speaker this buffer is.
                let ts = info.timestamp();
                if let Some(ahead) = ts.playback.duration_since(&ts.callback) {
                    latency.store((ahead.as_secs_f64() * rate) as u64, Ordering::Relaxed);
                }
            },
            |e| log::error!("audio stream error: {e}"),
            None,
        )
        .map_err(|e| e.to_string())
}

impl Sink for CpalSink {
    fn caps(&mut self) -> Result<Caps, String> {
        let device = device()?;
        let default = device.default_output_config().map_err(|e| e.to_string())?;
        let channels = default.channels().clamp(1, 2) as usize;
        let mut rates = Vec::new();
        let mut widest = (u32::MAX, 0u32);
        if let Ok(configs) = device.supported_output_configs() {
            for c in configs {
                let (lo, hi) = (c.min_sample_rate().0, c.max_sample_rate().0);
                widest = (widest.0.min(lo), widest.1.max(hi));
                for &r in COMMON_RATES {
                    if lo <= r && r <= hi && !rates.contains(&r) {
                        rates.push(r);
                    }
                }
            }
        }
        // A range from telephone to studio rates means the system converts anything.
        let any_rate = widest.0 <= 8000 && widest.1 >= 192_000;
        if rates.is_empty() {
            rates.push(default.sample_rate().0);
        }
        Ok(Caps {
            rates,
            any_rate,
            channels,
        })
    }

    fn open(&mut self, rate: u32, volume: Arc<AtomicU32>) -> Result<Stream, String> {
        let device = device()?;
        let supported = pick_config(&device, rate)?;
        let format = supported.sample_format();
        let config: cpal::StreamConfig = supported.into();
        let slot: Slot = Arc::new(Mutex::new(None));
        let played = Arc::new(AtomicU64::new(0));
        let latency = Arc::new(AtomicU64::new(0));
        let (s, p, l, v) = (slot.clone(), played.clone(), latency.clone(), volume);
        let stream = match format {
            cpal::SampleFormat::F32 => build_stream::<f32>(&device, &config, s, p, l, v)?,
            cpal::SampleFormat::I16 => build_stream::<i16>(&device, &config, s, p, l, v)?,
            cpal::SampleFormat::I32 => build_stream::<i32>(&device, &config, s, p, l, v)?,
            cpal::SampleFormat::U16 => build_stream::<u16>(&device, &config, s, p, l, v)?,
            other => return Err(format!("unsupported output format {other:?}")),
        };
        Ok(Stream {
            slot,
            played,
            latency_frames: latency,
            rate: config.sample_rate.0,
            channels: config.channels as usize,
            control: Box::new(CpalControl(stream)),
        })
    }
}

// ---- fake (tests) ----

/// The test side of a [`FakeSink`]: advance its clock and read what it "played".
#[derive(Clone, Default)]
pub struct FakeClock {
    inner: Arc<FakeShared>,
}

/// The open fake stream: ring, played-sample counter, channels, volume.
type FakeStream = (Slot, Arc<AtomicU64>, usize, Arc<AtomicU32>);

#[derive(Default)]
struct FakeShared {
    stream: Mutex<Option<FakeStream>>,
    playing: AtomicBool,
    heard: Mutex<Vec<f32>>,
    opened: Mutex<Vec<u32>>,
}

impl FakeClock {
    /// Lets the output take `frames` frames from the ring (if playing), like a sound card.
    /// Returns the frames it got (missing ones are silence).
    pub fn advance(&self, frames: usize) -> usize {
        if !self.inner.playing.load(Ordering::Relaxed) {
            return 0;
        }
        let guard = self.inner.stream.lock().unwrap();
        let Some((slot, played, channels, volume)) = guard.as_ref() else {
            return 0;
        };
        let mut buf = vec![0f32; frames * channels];
        let popped = fill(&mut buf, slot, volume);
        played.fetch_add(popped, Ordering::Relaxed);
        self.inner.heard.lock().unwrap().extend_from_slice(&buf);
        popped as usize / channels
    }

    /// Everything played so far (interleaved).
    pub fn heard(&self) -> Vec<f32> {
        self.inner.heard.lock().unwrap().clone()
    }

    /// The rates streams were opened at, in order.
    pub fn opened(&self) -> Vec<u32> {
        self.inner.opened.lock().unwrap().clone()
    }

    pub fn is_playing(&self) -> bool {
        self.inner.playing.load(Ordering::Relaxed)
    }
}

/// A sink with fixed capabilities and latency whose clock the test drives.
pub struct FakeSink {
    pub caps: Caps,
    pub latency_frames: u64,
    clock: FakeClock,
}

impl FakeSink {
    pub fn new(caps: Caps, latency_frames: u64) -> (FakeSink, FakeClock) {
        let clock = FakeClock::default();
        (
            FakeSink {
                caps,
                latency_frames,
                clock: clock.clone(),
            },
            clock,
        )
    }
}

struct FakeControl(Arc<FakeShared>);

impl Control for FakeControl {
    fn play(&self) {
        self.0.playing.store(true, Ordering::Relaxed);
    }
    fn pause(&self) {
        self.0.playing.store(false, Ordering::Relaxed);
    }
}

impl Sink for FakeSink {
    fn caps(&mut self) -> Result<Caps, String> {
        Ok(self.caps.clone())
    }

    fn open(&mut self, rate: u32, volume: Arc<AtomicU32>) -> Result<Stream, String> {
        let slot: Slot = Arc::new(Mutex::new(None));
        let played = Arc::new(AtomicU64::new(0));
        let channels = self.caps.channels;
        *self.clock.inner.stream.lock().unwrap() =
            Some((slot.clone(), played.clone(), channels, volume));
        self.clock.inner.opened.lock().unwrap().push(rate);
        self.clock.inner.playing.store(false, Ordering::Relaxed);
        Ok(Stream {
            slot,
            played,
            latency_frames: Arc::new(AtomicU64::new(self.latency_frames)),
            rate,
            channels,
            control: Box::new(FakeControl(self.clock.inner.clone())),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn caps(rates: &[u32]) -> Caps {
        Caps {
            rates: rates.to_vec(),
            any_rate: false,
            channels: 2,
        }
    }

    #[test]
    fn rate_choice_for_the_beats() {
        // Beats Studio Pro over USB-C: 32 and 48 kHz only.
        let beats = caps(&[32000, 48000]);
        assert_eq!(choose_rate(48000, &beats), 48000);
        assert_eq!(choose_rate(44100, &beats), 48000);
        assert_eq!(choose_rate(96000, &beats), 48000);
        assert_eq!(choose_rate(192000, &beats), 48000);
        assert_eq!(choose_rate(88200, &beats), 48000);
        assert_eq!(choose_rate(16000, &beats), 48000); // highest integer ratio (×3)
        assert_eq!(choose_rate(16000, &caps(&[32000, 44100])), 32000);
    }

    #[test]
    fn rate_choice_for_a_hi_res_dac() {
        let dac = caps(&[44100, 48000, 88200, 96000, 176400, 192000]);
        assert_eq!(choose_rate(44100, &dac), 44100);
        assert_eq!(choose_rate(352800, &dac), 176400);
        assert_eq!(choose_rate(384000, &dac), 192000);
        // The system mixer takes anything.
        let mixer = Caps {
            rates: vec![],
            any_rate: true,
            channels: 2,
        };
        assert_eq!(choose_rate(44100, &mixer), 44100);
    }
}
