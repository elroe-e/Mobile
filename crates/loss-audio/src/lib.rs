//! Loss audio engine: decodes with Symphonia (pure Rust: FLAC, MP3, AAC/ALAC,
//! Vorbis, WAV...) and plays through AAudio on Android (`aaudio.rs`), cpal elsewhere.
//!
//! The player runs on its own thread. The UI sends [`Command`]s and reads a
//! snapshot of [`PlayerState`].

#[cfg(target_os = "android")]
mod aaudio;
mod dsd;
mod dsp;
mod engine;
mod mp4gap;
mod opus;
pub mod sink;
pub use opus::codecs;
mod resample;
pub use resample::Quality;

/// How the engine converts and limits the output (M2-07).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct AudioOptions {
    pub quality: Quality,
    /// Highest output rate in Hz; 0 for no limit.
    pub max_rate: u32,
    /// Format and mode of the output stream (M2-02).
    pub output: sink::OutputPrefs,
}

use std::path::PathBuf;
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, PartialEq)]
pub struct QueueItem {
    pub id: String,
    pub path: PathBuf,
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
