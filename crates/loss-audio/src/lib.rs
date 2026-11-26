//! Loss audio engine: decodes with Symphonia (pure Rust: FLAC, MP3, AAC/ALAC,
//! Vorbis, WAV...) and plays through AAudio on Android (`aaudio.rs`), cpal elsewhere.
//!
//! The player runs on its own thread. The UI sends [`Command`]s and reads a
//! snapshot of [`PlayerState`].

#[cfg(target_os = "android")]
mod aaudio;
pub mod crossfeed;
mod dsd;
mod dsp;
mod engine;
pub mod eq;
mod mp4gap;
mod opus;
pub mod silence;
pub mod sink;
pub mod stretch;
mod wavpack;
pub use opus::codecs;
mod resample;
pub use resample::Quality;

/// How the engine converts and limits the output (M2-07).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AudioOptions {
    pub quality: Quality,
    /// Highest output rate in Hz; 0 for no limit.
    pub max_rate: u32,
    /// Format and mode of the output stream (M2-02).
    pub output: sink::OutputPrefs,
    /// ReplayGain levelling (M2-09); off on bit-perfect paths.
    pub normalize: Normalize,
    /// The equalizer (M2-10) when on; bypassed on bit-perfect paths.
    pub eq: Option<eq::EqParams>,
    /// Crossfeed for headphones (M2-12); bypassed on bit-perfect paths.
    pub crossfeed: Option<crossfeed::Strength>,
    /// Playback speed and pitch (M2-13); ignored on bit-perfect paths.
    pub tempo: Tempo,
    /// Cut long silent stretches short (M2-14). Doesn't alter the samples, so it works on
    /// bit-perfect paths too.
    pub skip_silence: bool,
}

/// Speed and pitch: `speed` is the playback rate (1 = normal); with `keep_pitch` the pitch stays
/// where it was (time-stretching), without it the pitch follows the speed like a record; and
/// `semitones` shifts the pitch on top.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tempo {
    pub speed: f64,
    pub keep_pitch: bool,
    pub semitones: f64,
}

impl Default for Tempo {
    fn default() -> Tempo {
        Tempo {
            speed: 1.0,
            keep_pitch: true,
            semitones: 0.0,
        }
    }
}

impl Tempo {
    /// Nothing to do: normal speed and pitch.
    pub fn is_normal(&self) -> bool {
        (self.speed - 1.0).abs() < 1e-3 && self.semitones.abs() < 1e-3
    }

    /// How the change is made: (`resample`, `stretch`). The audio is time-stretched by `stretch`
    /// (the speed-up, pitch unchanged) and then played `resample` times faster (pitch and speed
    /// both change). The pitch ends up `resample` times higher, the speed `resample * stretch`.
    pub fn plan(&self) -> (f64, f64) {
        let speed = self.speed.clamp(0.25, 4.0);
        let shift = 2f64.powf(self.semitones.clamp(-12.0, 12.0) / 12.0);
        if self.keep_pitch {
            (shift, speed / shift)
        } else {
            (speed * shift, 1.0 / shift)
        }
    }
}

use std::path::PathBuf;
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, PartialEq, Default)]
pub struct QueueItem {
    pub id: String,
    pub path: PathBuf,
    /// Where the track starts in the file, in seconds (CUE sheets; 0 for whole files).
    pub start: f64,
    /// Where it ends; None for the end of the file.
    pub end: Option<f64>,
    /// ReplayGain in dB (−18 LUFS reference), from the file's tags.
    pub gain_track: Option<f64>,
    pub gain_album: Option<f64>,
    /// Part of an album played in order (automatic gain mode uses the album gain).
    pub album_run: bool,
}

/// Which ReplayGain value levels a track.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GainMode {
    Track,
    Album,
    /// Album gain while an album plays in order, track gain otherwise.
    #[default]
    Auto,
}

impl GainMode {
    pub fn parse(s: &str) -> GainMode {
        match s {
            "track" => GainMode::Track,
            "album" => GainMode::Album,
            _ => GainMode::Auto,
        }
    }
}

/// Volume levelling from ReplayGain tags (M2-09). Like Mornye it only turns tracks down
/// (gain plus pre-amp capped at 0 dB), so it never clips.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Normalize {
    pub enabled: bool,
    pub mode: GainMode,
    /// Shifts the −18 LUFS reference: −6 dB aims at −24 LUFS.
    pub preamp_db: f64,
}

impl Normalize {
    /// The gain for an item in dB (≤ 0) and whether the album value was used; None for none.
    pub fn gain_for(&self, item: &QueueItem) -> Option<(f64, bool)> {
        if !self.enabled {
            return None;
        }
        let album_first = match self.mode {
            GainMode::Track => false,
            GainMode::Album => true,
            GainMode::Auto => item.album_run,
        };
        let (g, album) = match (album_first, item.gain_track, item.gain_album) {
            (true, _, Some(a)) => (a, true),
            (false, Some(t), _) => (t, false),
            (_, Some(t), None) => (t, false),
            (_, None, Some(a)) => (a, true),
            (_, None, None) => return None,
        };
        let db = (g + self.preamp_db).min(0.0);
        (db < -0.005).then_some((db, album))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Status {
    #[default]
    Stopped,
    Loading,
    Playing,
    Paused,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Repeat {
    #[default]
    Off,
    All,
    One,
}

impl Repeat {
    pub fn next(self) -> Repeat {
        match self {
            Repeat::Off => Repeat::All,
            Repeat::All => Repeat::One,
            Repeat::One => Repeat::Off,
        }
    }
}

/// Technical details of the playing stream, for the quality label.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct StreamInfo {
    pub codec: String,
    pub sample_rate: u32,
    pub bits_per_sample: Option<u32>,
    pub channels: u16,
    pub output_rate: u32,
    /// What Loss does to the samples on the way out ("resample 44100→48000 Hz"); empty when they
    /// reach the output unchanged.
    pub processing: Vec<String>,
    /// How the output stream opened, e.g. "I24 · exclusive · low latency · 48000 Hz".
    pub output: String,
}

impl StreamInfo {
    /// Loss hands the decoded samples to the output unchanged. (The output may still convert:
    /// Android's mixer does unless bit-perfect or direct USB output is in use.)
    pub fn untouched(&self) -> bool {
        self.processing.is_empty()
    }
}

impl StreamInfo {
    pub fn label(&self) -> String {
        let mut s = self.codec.to_uppercase();
        if s.starts_with("DSD") {
            // "DSD64"; the 88.2 kHz it decodes to is an internal step, so only the output shows.
            if self.output_rate != 0 {
                let khz = self.output_rate as f64 / 1000.0;
                s.push_str(&format!(" → {} kHz", trim_float(khz)));
            }
            return s;
        }
        if let Some(b) = self.bits_per_sample {
            s.push_str(&format!(" · {b}-bit"));
        }
        if self.sample_rate > 0 {
            let khz = self.sample_rate as f64 / 1000.0;
            s.push_str(&format!(" · {} kHz", trim_float(khz)));
        }
        if self.output_rate != 0 && self.output_rate != self.sample_rate {
            let khz = self.output_rate as f64 / 1000.0;
            s.push_str(&format!(" → {} kHz", trim_float(khz)));
        }
        s
    }

    pub fn is_lossless(&self) -> bool {
        matches!(
            self.codec.to_lowercase().as_str(),
            "flac" | "alac" | "wav" | "pcm" | "aiff"
        ) || self.codec.to_lowercase().starts_with("pcm")
            || self.codec.to_lowercase().starts_with("dsd")
    }
}

fn trim_float(v: f64) -> String {
    let s = format!("{v:.1}");
    s.strip_suffix(".0").map(str::to_string).unwrap_or(s)
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct PlayerState {
    pub queue: Vec<QueueItem>,
    pub index: Option<usize>,
    pub status: Status,
    pub position: f64,
    pub duration: f64,
    pub volume: f32,
    pub shuffle: bool,
    pub repeat: Repeat,
    pub info: StreamInfo,
    /// Why the last song couldn't play.
    pub error: Option<PlayError>,
    /// Ids of tracks that counted as a "play" (30 s or half the track).
    /// The app drains this to update statistics.
    pub play_events: Vec<String>,
}

impl PlayerState {
    pub fn current(&self) -> Option<&QueueItem> {
        self.index.and_then(|i| self.queue.get(i))
    }
}

#[derive(Debug)]
pub enum Command {
    /// Conversion quality and rate limit; used from the next track (or seek) on.
    SetAudioOptions(AudioOptions),
    /// What the active output takes (from the platform's device list), or None to ask the
    /// output itself. A different best rate reopens the output at the current position.
    SetOutputCaps(Option<sink::Caps>),
    /// Replace the queue and start playing at `start`.
    SetQueue {
        items: Vec<QueueItem>,
        start: usize,
    },
    /// Append to the end of the queue.
    Enqueue(Vec<QueueItem>),
    /// Insert right after the current track.
    PlayNext(Vec<QueueItem>),
    Remove(usize),
    JumpTo(usize),
    Play,
    Pause,
    Toggle,
    Next,
    Previous,
    Seek(f64),
    SetVolume(f32),
    SetShuffle(bool),
    SetRepeat(Repeat),
    Stop,
}

#[derive(Clone)]
pub struct Player {
    tx: Sender<Command>,
    state: Arc<Mutex<PlayerState>>,
}

impl Player {
    pub fn new(volume: f32) -> Player {
        let state = Arc::new(Mutex::new(PlayerState {
            volume,
            ..Default::default()
        }));
        let (tx, rx) = mpsc::channel();
        let st = state.clone();
        std::thread::Builder::new()
            .name("loss-player".into())
            .spawn(move || engine::run(rx, st))
            .expect("spawn player thread");
        Player { tx, state }
    }

    pub fn send(&self, cmd: Command) {
        let _ = self.tx.send(cmd);
    }

    pub fn state(&self) -> PlayerState {
        self.state.lock().map(|s| s.clone()).unwrap_or_default()
    }

    pub fn take_play_events(&self) -> Vec<String> {
        self.state
            .lock()
            .map(|mut s| std::mem::take(&mut s.play_events))
            .unwrap_or_default()
    }
}

/// What kind of problem stopped playback, so the app can explain it plainly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayErrorKind {
    /// No audio output could be opened (e.g. the USB DAC was unplugged).
    Output,
    /// The file is gone or can't be read.
    Missing,
    /// The file's format or codec isn't supported.
    Unsupported,
}

/// A playback error: its kind, plus the technical detail for the log.
#[derive(Debug, Clone, PartialEq)]
pub struct PlayError {
    pub kind: PlayErrorKind,
    pub detail: String,
}

impl std::fmt::Display for PlayError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}: {}", self.kind, self.detail)
    }
}
