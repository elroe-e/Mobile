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

/// A sample format an output can take.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SampleFormat {
    #[default]
    F32,
    I16,
    /// 24-bit in 3 bytes.
    I24,
    I32,
}

impl SampleFormat {
    /// Bits of an integer format; None for float.
    pub fn int_bits(self) -> Option<u32> {
        match self {
            SampleFormat::F32 => None,
            SampleFormat::I16 => Some(16),
            SampleFormat::I24 => Some(24),
            SampleFormat::I32 => Some(32),
        }
    }

    pub fn bytes(self) -> usize {
        match self {
            SampleFormat::I16 => 2,
            SampleFormat::I24 => 3,
            SampleFormat::F32 | SampleFormat::I32 => 4,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            SampleFormat::F32 => "F32",
            SampleFormat::I16 => "I16",
            SampleFormat::I24 => "I24",
            SampleFormat::I32 => "I32",
        }
    }

    /// From the `output_format` setting: "float", "16", "24", "32"; None for "auto".
    pub fn parse_setting(s: &str) -> Option<SampleFormat> {
        match s {
            "float" => Some(SampleFormat::F32),
            "16" => Some(SampleFormat::I16),
            "24" => Some(SampleFormat::I24),
            "32" => Some(SampleFormat::I32),
            _ => None,
        }
    }
}

/// How to open the output (M2-02). Outputs that can't do something ignore it.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct OutputPrefs {
    /// None: float through the shared mixer, the device's own format when exclusive.
    pub format: Option<SampleFormat>,
    /// Ask for an exclusive (MMAP) stream; Android falls back to shared when it can't.
    pub exclusive: bool,
    /// Low-latency performance mode (smaller buffers, more battery) instead of power saving.
    pub low_latency: bool,
    /// Let Android's spatializer process the stream.
    pub spatial: bool,
    /// Play on this device id; 0 lets Android route.
    pub device_id: i32,
}

/// What an output actually opened with.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct OutputInfo {
    pub format: SampleFormat,
    pub exclusive: bool,
    pub low_latency: bool,
    pub device_id: i32,
    pub buffer_frames: u32,
    /// What the hardware runs at, when the system says (Android 14+), e.g. "I24 48000 Hz".
    pub hardware: Option<String>,
}

impl OutputInfo {
    /// For logs and the format details, e.g. "I24 · exclusive · low latency · 48000 Hz".
    pub fn describe(&self, rate: u32) -> String {
        let mut parts = vec![self.format.name().to_string()];
        parts.push(
            if self.exclusive {
                "exclusive"
            } else {
                "shared"
            }
            .into(),
        );
        parts.push(
            if self.low_latency {
                "low latency"
            } else {
                "power saving"
            }
            .into(),
        );
        parts.push(format!("{rate} Hz"));
        if self.buffer_frames > 0 {
            parts.push(format!("buffer {}", self.buffer_frames));
        }
        if let Some(h) = &self.hardware {
            parts.push(format!("hardware {h}"));
        }
        parts.join(" · ")
    }
}

/// Turns the ring's f32 samples into an output format's little-endian bytes. Integer formats
/// below 32 bits get TPDF dither when `dither` is set (the samples were changed on the way, or
/// have more bits than the output); otherwise samples are rounded, which keeps 16-bit content
/// bit-exact on a 16-bit output.
pub struct Encoder {
    format: SampleFormat,
    channels: usize,
    dither: Option<crate::resample::Dither>,
}

impl Encoder {
    pub fn new(format: SampleFormat, channels: usize) -> Encoder {
        let dither = match format.int_bits() {
            // High-pass TPDF at 16 bits moves the noise where it's least audible.
            Some(16) => Some(crate::resample::Dither::new(16, channels, true)),
            Some(24) => Some(crate::resample::Dither::new(24, channels, false)),
            _ => None,
        };
        Encoder {
            format,
            channels: channels.max(1),
            dither,
        }
    }

    /// Fills `out` (whole samples of the format) from `next`, which yields one sample per call.
    pub fn write(&mut self, out: &mut [u8], dither: bool, mut next: impl FnMut() -> f32) {
        let n = out.len() / self.format.bytes();
        for i in 0..n {
            let v = next() as f64;
            let c = i % self.channels;
            let at = i * self.format.bytes();
            match self.format {
                SampleFormat::F32 => out[at..at + 4].copy_from_slice(&(v as f32).to_le_bytes()),
                SampleFormat::I32 => {
                    let q = (v * 2_147_483_648.0)
                        .round()
                        .clamp(-2_147_483_648.0, 2_147_483_647.0);
                    out[at..at + 4].copy_from_slice(&(q as i32).to_le_bytes());
                }
                SampleFormat::I16 | SampleFormat::I24 => {
                    let bits = self.format.int_bits().unwrap_or(16);
                    let scale = (1u64 << (bits - 1)) as f64;
                    let q = match (&mut self.dither, dither) {
                        (Some(d), true) => d.quantize(v, c),
                        _ => (v * scale).round().clamp(-scale, scale - 1.0),
                    } as i32;
                    let b = q.to_le_bytes();
                    let w = self.format.bytes();
                    out[at..at + w].copy_from_slice(&b[..w]);
                }
            }
        }
    }
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
    /// The format and mode the output opened with.
    pub info: OutputInfo,
    /// Set by the engine: dither integer output (see [`Encoder`]).
    pub dither: Arc<AtomicBool>,
    /// Set by the output when its device went away (unplugged, re-routed); the engine reopens.
    pub lost: Arc<AtomicBool>,
    pub(crate) control: Box<dyn Control>,
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
    /// How later streams should open (format, exclusive, device…).
    fn set_prefs(&mut self, _prefs: OutputPrefs) {}
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
            info: OutputInfo {
                format: match format {
                    cpal::SampleFormat::I16 | cpal::SampleFormat::U16 => SampleFormat::I16,
                    cpal::SampleFormat::I32 => SampleFormat::I32,
                    _ => SampleFormat::F32,
                },
                ..OutputInfo::default()
            },
            dither: Arc::new(AtomicBool::new(false)),
            lost: Arc::new(AtomicBool::new(false)),
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
    prefs: Mutex<OutputPrefs>,
    /// The open stream's dither and lost flags.
    flags: Mutex<Option<(Arc<AtomicBool>, Arc<AtomicBool>)>>,
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

    /// Whether the engine asked the open stream to dither.
    pub fn dithering(&self) -> bool {
        let f = self.inner.flags.lock().unwrap();
        f.as_ref().is_some_and(|(d, _)| d.load(Ordering::Relaxed))
    }

    /// The device goes away, as when the Beats are unplugged.
    pub fn lose_device(&self) {
        if let Some((_, lost)) = self.inner.flags.lock().unwrap().as_ref() {
            lost.store(true, Ordering::Relaxed);
        }
    }

    /// The preferences the sink was last given.
    pub fn prefs(&self) -> OutputPrefs {
        *self.inner.prefs.lock().unwrap()
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
        let prefs = *self.clock.inner.prefs.lock().unwrap();
        let (dither, lost) = (
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicBool::new(false)),
        );
        *self.clock.inner.flags.lock().unwrap() = Some((dither.clone(), lost.clone()));
        Ok(Stream {
            slot,
            played,
            latency_frames: Arc::new(AtomicU64::new(self.latency_frames)),
            rate,
            channels,
            info: OutputInfo {
                format: prefs.format.unwrap_or_default(),
                exclusive: prefs.exclusive,
                low_latency: prefs.low_latency,
                device_id: prefs.device_id,
                ..OutputInfo::default()
            },
            dither,
            lost,
            control: Box::new(FakeControl(self.clock.inner.clone())),
        })
    }

    fn set_prefs(&mut self, prefs: OutputPrefs) {
        *self.clock.inner.prefs.lock().unwrap() = prefs;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn enc(format: SampleFormat, dither: bool, input: &[f32]) -> Vec<u8> {
        let mut out = vec![0u8; input.len() * format.bytes()];
        let mut it = input.iter().copied();
        Encoder::new(format, 1).write(&mut out, dither, || it.next().unwrap_or(0.0));
        out
    }

    #[test]
    fn sixteen_bit_content_stays_bit_exact_without_dither() {
        // Every 16-bit value survives f32 and back unchanged.
        let values: Vec<i16> = (-32768..=32767).step_by(7).collect();
        let input: Vec<f32> = values.iter().map(|&v| v as f32 / 32768.0).collect();
        let out = enc(SampleFormat::I16, false, &input);
        let back: Vec<i16> = out
            .chunks(2)
            .map(|b| i16::from_le_bytes([b[0], b[1]]))
            .collect();
        assert_eq!(back, values);
    }

    #[test]
    fn twenty_four_bit_is_packed_little_endian() {
        let out = enc(SampleFormat::I24, false, &[0.5, -1.0, 1.0]);
        assert_eq!(&out[0..3], &[0x00, 0x00, 0x40]); // 0x400000
        assert_eq!(&out[3..6], &[0x00, 0x00, 0x80]); // -0x800000
        assert_eq!(&out[6..9], &[0xFF, 0xFF, 0x7F]); // clipped to the largest value
        let f = enc(SampleFormat::F32, false, &[0.25]);
        assert_eq!(f32::from_le_bytes([f[0], f[1], f[2], f[3]]), 0.25);
        let i = enc(SampleFormat::I32, false, &[-0.5]);
        assert_eq!(i32::from_le_bytes([i[0], i[1], i[2], i[3]]), -1 << 30);
    }

    #[test]
    fn dither_turns_a_quiet_tone_into_noise_not_steps() {
        // A tone at a quarter of an LSB: without dither it rounds to silence; with dither its
        // average survives (the error is noise, not distortion).
        let lsb = 1.0 / 32768.0;
        let input: Vec<f32> = (0..48000).map(|_| 0.25 * lsb).collect();
        let plain = enc(SampleFormat::I16, false, &input);
        assert!(plain.iter().all(|&b| b == 0));
        let d = enc(SampleFormat::I16, true, &input);
        let mean: f64 = d
            .chunks(2)
            .map(|b| i16::from_le_bytes([b[0], b[1]]) as f64)
            .sum::<f64>()
            / 48000.0;
        assert!((mean - 0.25).abs() < 0.05, "mean {mean}");
    }

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
