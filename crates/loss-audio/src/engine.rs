//! The player thread: owns the decoder, the output stream and the queue.

use crate::dsp::Graph;
use crate::mp4gap::Trim;
use crate::sink::{choose_rate, Caps, Sink, Stream};
use crate::{
    AudioOptions, Command, PlayError, PlayErrorKind, PlayerState, Quality, QueueItem, Repeat,
    Status, StreamInfo,
};
use std::fs::File;
use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{Decoder, DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use symphonia::core::units::Time;

/// One playing track.
struct Session {
    item: QueueItem,
    format: Box<dyn FormatReader>,
    decoder: Box<dyn Decoder>,
    track_id: u32,
    src_rate: u32,
    src_channels: usize,
    graph: Graph,
    quality: Quality,
    producer: rtrb::Producer<f32>,
    capacity: usize,
    pending: Vec<f32>,
    pending_pos: usize,
    eof: bool,
    start_offset: f64,
    duration: f64,
    info: StreamInfo,
    /// Samples pushed into this ring buffer since it was created, including by
    /// earlier tracks that handed it over (gapless).
    pushed: u64,
    /// Encoder priming and padding to cut (MP4); in frames of the decoded stream.
    trim: Trim,
    /// Packet timestamps → frames: (numerator, denominator) of the track's time base × rate.
    ts_to_frames: (u64, u64),
    /// ReplayGain as a linear factor (1 for none), applied after the graph.
    gain: f64,
    /// The equalizer after the gain; handed over across gapless joins with its filter memory.
    eq: Option<crate::eq::Equalizer>,
    /// Crossfeed before the equalizer; handed over like it.
    crossfeed: Option<crate::crossfeed::Crossfeed>,
    /// Time-stretching before the graph (at the source rate), when speed or pitch changed.
    stretch: Option<crate::stretch::Wsola>,
    /// Source seconds per output second.
    speed: f64,
    tempo: crate::Tempo,
    /// Silence skipping, at the very start of the chain (source rate).
    skipper: Option<crate::silence::Skipper>,
    skips: SkipLog,
}

/// Silence skipped so far: (output sample count at that point, source seconds dropped), shared
/// between a session and the display of its track.
type SkipLog = std::sync::Arc<Mutex<Vec<(u64, f64)>>>;

/// Source seconds skipped before output sample `at`.
fn skipped_before(log: &SkipLog, at: u64) -> f64 {
    let log = log.lock().unwrap_or_else(|p| p.into_inner());
    log.iter()
        .take_while(|(when, _)| *when <= at)
        .map(|(_, s)| s)
        .sum()
}

/// The conversion graph for a source, with speed and pitch folded in: the time-stretch runs at
/// the source rate before it (`stretch`), and the pitch change plays the stretched audio faster
/// by treating it as a higher source rate. The polynomial resampler takes any ratio (the FFT
/// ones need whole-number ones). Returns the graph, the stretcher and the speed (source
/// seconds per output second).
fn build_graph(
    src_rate: u32,
    src_channels: usize,
    out_rate: u32,
    out_channels: usize,
    quality: Quality,
    tempo: &crate::Tempo,
) -> (Graph, Option<crate::stretch::Wsola>, f64) {
    let (resample, stretch) = tempo.plan();
    let from = if (resample - 1.0).abs() < 1e-6 {
        src_rate
    } else {
        ((src_rate as f64 * resample).round() as u32).max(1)
    };
    let quality = if from != src_rate {
        Quality::Fast
    } else {
        quality
    };
    let wsola = ((stretch - 1.0).abs() > 1e-3)
        .then(|| crate::stretch::Wsola::new(stretch, src_rate, src_channels));
    let speed = from as f64 / src_rate as f64 * if wsola.is_some() { stretch } else { 1.0 };
    (
        Graph::new(from, src_channels, out_rate, out_channels, quality),
        wsola,
        speed,
    )
}

/// "speed 1.25×, pitch +2 st" for the processing list.
fn tempo_note(t: &crate::Tempo) -> Option<String> {
    if t.is_normal() {
        return None;
    }
    let mut parts = vec![format!("speed {:.2}×", t.speed)];
    if t.semitones.abs() >= 1e-3 {
        parts.push(format!("pitch {:+.0} st", t.semitones));
    }
    if !t.keep_pitch && (t.speed - 1.0).abs() >= 1e-3 {
        parts.push("pitch follows speed".into());
    }
    Some(parts.join(", "))
}

/// The trim for a file and how its packet timestamps convert to frames.
/// The part of the file this item plays: encoder priming and padding cut (MP4), narrowed to the
/// item's own range (a CUE track), in frames of the decoded stream.
fn gapless_trim(
    item: &QueueItem,
    format: &dyn FormatReader,
    track_id: u32,
    rate: u32,
) -> (Trim, (u64, u64)) {
    let (codec, conv) = codec_trim(&item.path, format, track_id, rate);
    if item.start <= 0.0 && item.end.is_none() {
        return (codec, conv);
    }
    let frames = |secs: f64| (secs * rate as f64).round() as u64;
    let start = frames(item.start.max(0.0));
    let length = match item.end {
        Some(end) => Some(frames(end).saturating_sub(start)),
        None => codec.length.map(|n| n.saturating_sub(start)),
    };
    (
        Trim {
            delay: codec.delay + start,
            length,
        },
        conv,
    )
}

/// Seeks (accurately; the trim cuts the exact sample). False if the reader refused.
fn seek_to(format: &mut dyn FormatReader, track_id: u32, secs: f64) -> bool {
    match format.seek(
        SeekMode::Accurate,
        SeekTo::Time {
            time: Time::from(secs),
            track_id: Some(track_id),
        },
    ) {
        Ok(_) => true,
        Err(e) => {
            log::warn!("seek failed: {e}");
            false
        }
    }
}

/// How long an item plays: its range, or the whole file.
fn item_duration(item: &QueueItem, file: f64) -> f64 {
    if item.start <= 0.0 && item.end.is_none() {
        return file;
    }
    (item.end.unwrap_or(file) - item.start).max(0.0)
}

fn codec_trim(
    path: &Path,
    format: &dyn FormatReader,
    track_id: u32,
    rate: u32,
) -> (Trim, (u64, u64)) {
    let tb = format
        .tracks()
        .iter()
        .find(|t| t.id == track_id)
        .and_then(|t| t.codec_params.time_base);
    let conv = match tb {
        Some(tb) => (tb.numer as u64 * rate as u64, tb.denom as u64),
        None => (1, 1),
    };
    (crate::mp4gap::read(path).unwrap_or(Trim::NONE), conv)
}

/// The track the listener hears (it lags behind the decoder by the buffer).
struct Display {
    /// Source seconds per output second for this track (speed and pitch change).
    speed: f64,
    /// Silence skipped in this track.
    skips: SkipLog,
    item_id: String,
    /// `played_samples` value at which this track started.
    base: u64,
    start_offset: f64,
    duration: f64,
    counted: bool,
}

/// A gapless join waiting for playback to reach it.
struct Switch {
    speed: f64,
    skips: SkipLog,
    /// `played_samples` value where the next track starts.
    at: u64,
    item_id: String,
    duration: f64,
    info: StreamInfo,
}

/// An opened file: reader, decoder, track id, stream info, duration in seconds.
type OpenedFile = (
    Box<dyn FormatReader>,
    Box<dyn Decoder>,
    u32,
    StreamInfo,
    f64,
);

/// ALAC's bit depth from its codec config (ALACSpecificConfig: frame length u32, compatible
/// version u8, bit depth u8), which may still carry its 12-byte `alac` atom header.
fn alac_bits(params: &symphonia::core::codecs::CodecParameters) -> Option<u32> {
    if params.codec != symphonia::core::codecs::CODEC_TYPE_ALAC {
        return None;
    }
    let cfg = params.extra_data.as_deref()?;
    let cfg = if cfg.len() >= 36 && &cfg[4..8] == b"alac" {
        &cfg[12..]
    } else {
        cfg
    };
    let bits = *cfg.get(5)? as u32;
    matches!(bits, 16 | 20 | 24 | 32).then_some(bits)
}

fn open_file(path: &Path) -> Result<OpenedFile, String> {
    let file = File::open(path).map_err(|e| format!("cannot open file: {e}"))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let probed = crate::opus::probe()
        .format(
            &hint,
            mss,
            &FormatOptions {
                enable_gapless: true,
                ..Default::default()
            },
            &MetadataOptions::default(),
        )
        .map_err(|e| format!("unsupported file: {e}"))?;
    let format = probed.format;
    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or("no audio track")?;
    let params = track.codec_params.clone();
    let track_id = track.id;
    let decoder = crate::opus::codecs()
        .make(&params, &DecoderOptions::default())
        .map_err(|e| format!("unsupported codec: {e}"))?;
    let rate = params.sample_rate.unwrap_or(44100);
    let duration = match (params.n_frames, params.time_base) {
        (Some(n), Some(tb)) => {
            let t = tb.calc_time(n);
            t.seconds as f64 + t.frac
        }
        (Some(n), None) => n as f64 / rate as f64,
        _ => 0.0,
    };
    // MP4 with encoder priming and padding: the music's own length.
    let duration = match crate::mp4gap::read(path).and_then(|t| t.length) {
        Some(n) => n as f64 / rate.max(1) as f64,
        None => duration,
    };
    let codec = if params.codec == crate::dsd::CODEC_TYPE_DSD {
        // "dsd64", "dsd128", …: multiples of 44.1 kHz (48 kHz-family files count the same way).
        let factor = params
            .extra_data
            .as_deref()
            .map(|e| e[0] as u32 | (*e.get(1).unwrap_or(&0) as u32) << 8);
        format!("dsd{}", factor.unwrap_or(32) * 2)
    } else {
        crate::opus::codecs()
            .get_codec(params.codec)
            .map(|d| d.short_name.to_string())
            .unwrap_or_else(|| "audio".into())
    };
    let info = StreamInfo {
        codec,
        sample_rate: rate,
        bits_per_sample: params.bits_per_sample.or_else(|| alac_bits(&params)),
        channels: params.channels.map(|c| c.count() as u16).unwrap_or(2),
        output_rate: 0,
        processing: Vec::new(),
        output: String::new(),
    };
    Ok((format, decoder, track_id, info, duration))
}

impl Session {
    /// Decode more audio into the ring buffer. Returns false once the file is
    /// fully decoded.
    fn pump(&mut self, out_channels: usize, out_rate: u32) -> Result<(), String> {
        let mut budget = 16; // packets per call, so commands stay responsive
        loop {
            // Flush pending samples first.
            while self.pending_pos < self.pending.len() {
                if self.producer.push(self.pending[self.pending_pos]).is_err() {
                    return Ok(()); // ring full
                }
                self.pending_pos += 1;
                self.pushed += 1;
            }
            self.pending.clear();
            self.pending_pos = 0;
            if self.eof || budget == 0 {
                return Ok(());
            }
            budget -= 1;

            let packet = match self.format.next_packet() {
                Ok(p) => p,
                Err(SymError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                    self.finish();
                    continue;
                }
                Err(SymError::ResetRequired) => {
                    self.finish();
                    continue;
                }
                Err(e) => return Err(e.to_string()),
            };
            if packet.track_id() != self.track_id {
                continue;
            }
            let decoded = match self.decoder.decode(&packet) {
                Ok(d) => d,
                Err(SymError::DecodeError(e)) => {
                    log::warn!("skipping bad packet: {e}");
                    continue;
                }
                Err(SymError::IoError(_)) => continue,
                Err(e) => return Err(e.to_string()),
            };
            let spec = *decoded.spec();
            if spec.rate != self.src_rate || spec.channels.count() != self.src_channels {
                self.src_rate = spec.rate;
                self.src_channels = spec.channels.count();
                (self.graph, self.stretch, self.speed) = build_graph(
                    self.src_rate,
                    self.src_channels,
                    out_rate,
                    out_channels,
                    self.quality,
                    &self.tempo,
                );
            }
            let frames = decoded.frames() as u64;
            let mut buf = SampleBuffer::<f64>::new(decoded.capacity() as u64, spec);
            buf.copy_interleaved_ref(decoded);
            let mut samples = buf.samples();
            if self.trim != Trim::NONE {
                // Cut encoder priming and padding by the packet's place in the stream.
                let (num, den) = self.ts_to_frames;
                let start = packet.ts() * num / den.max(1);
                let keep = self.trim.keep(start, frames);
                let ch = self.src_channels.max(1);
                samples = &samples[keep.start as usize * ch..keep.end as usize * ch];
                let past_end = self
                    .trim
                    .length
                    .is_some_and(|n| start + frames >= self.trim.delay + n);
                if past_end && keep.end < frames {
                    // The music ends in this packet; what follows is padding.
                    let mut processed = Vec::with_capacity(samples.len());
                    self.run_graph(samples, &mut processed);
                    self.push_pending(&processed);
                    self.finish();
                    continue;
                }
            }
            let mut processed = Vec::with_capacity(samples.len());
            self.run_graph(samples, &mut processed);
            self.push_pending(&processed);
        }
    }

    /// Decoded samples through silence skipping, the time-stretch (if any) and the conversion
    /// graph.
    fn run_graph(&mut self, samples: &[f64], out: &mut Vec<f64>) {
        let Some(sk) = &mut self.skipper else {
            return self.stretch_and_convert(samples, out);
        };
        let mut kept = Vec::with_capacity(samples.len());
        let dropped = sk.process(samples, &mut kept);
        if dropped > 0 {
            // Where in the output this happened (roughly: the conversion delays it a little).
            let at = self.pushed + (self.pending.len() - self.pending_pos) as u64;
            let secs = dropped as f64 / self.src_rate.max(1) as f64;
            self.skips
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .push((at, secs));
        }
        self.stretch_and_convert(&kept, out);
    }

    fn stretch_and_convert(&mut self, samples: &[f64], out: &mut Vec<f64>) {
        match &mut self.stretch {
            Some(w) => {
                let mut stretched = Vec::new();
                w.process(samples, &mut stretched);
                self.graph.process(&stretched, out);
            }
            None => self.graph.process(samples, out),
        }
    }

    /// Queues processed samples for the ring, levelled by the ReplayGain factor. (The ring and
    /// the output are f32: exact for every 16- and 24-bit sample.)
    fn push_pending(&mut self, processed: &[f64]) {
        let g = self.gain;
        if self.eq.is_none() && self.crossfeed.is_none() {
            if g == 1.0 {
                self.pending.extend(processed.iter().map(|&v| v as f32));
            } else {
                self.pending
                    .extend(processed.iter().map(|&v| (v * g) as f32));
            }
            return;
        }
        // Gain, then crossfeed, then the equalizer (whose limiter comes last).
        let mut v: Vec<f64> = processed.iter().map(|&x| x * g).collect();
        if let Some(c) = &mut self.crossfeed {
            c.process(&mut v);
        }
        if let Some(eq) = &mut self.eq {
            eq.process_vec(&mut v);
        }
        self.pending.extend(v.iter().map(|&x| x as f32));
    }

    /// End of the file: the graph hands over what it held back (resampler tail). (The
    /// equalizer's look-ahead stays in its delay line: it plays on into a gapless next track.)
    fn finish(&mut self) {
        let mut tail = Vec::new();
        if let Some(sk) = &mut self.skipper {
            // A short silent tail plays as it was.
            let mut held = Vec::new();
            sk.flush(&mut held);
            if !held.is_empty() {
                self.stretch_and_convert(&held, &mut tail);
            }
        }
        if let Some(w) = &mut self.stretch {
            let mut rest = Vec::new();
            w.flush(&mut rest);
            self.graph.process(&rest, &mut tail);
        }
        self.graph.flush(&mut tail);
        self.push_pending(&tail);
        self.eof = true;
    }

    /// Everything is decoded and in the ring (it may still be playing).
    fn decoded_all(&self) -> bool {
        self.eof && self.pending_pos >= self.pending.len()
    }

    fn drained(&self) -> bool {
        self.decoded_all() && self.producer.slots() == self.capacity
    }

    /// Starts decoding the next track into the same ring buffer, so playback
    /// continues without a gap.
    #[allow(clippy::too_many_arguments)]
    fn handover(
        prev: Session,
        item: QueueItem,
        opened: OpenedFile,
        out_rate: u32,
        out_channels: usize,
        quality: Quality,
        gain: Option<(f64, bool)>,
        tempo: crate::Tempo,
        skip_silence: bool,
    ) -> Session {
        let (mut format, decoder, track_id, mut info, duration) = opened;
        let (trim, ts_to_frames) = gapless_trim(&item, format.as_ref(), track_id, info.sample_rate);
        let duration = item_duration(&item, duration);
        // A CUE track further into its file: start reading near it, not from the top.
        let skip = trim.delay as f64 / info.sample_rate.max(1) as f64;
        if skip > 0.05 {
            seek_to(format.as_mut(), track_id, skip);
        }
        info.output_rate = out_rate;
        info.output = prev.info.output.clone();
        let src_channels = info.channels.max(1) as usize;
        let src_rate = info.sample_rate;
        let (graph, stretch, speed) = build_graph(
            info.sample_rate,
            src_channels,
            out_rate,
            out_channels,
            quality,
            &tempo,
        );
        info.processing = graph.describe();
        info.processing.extend(tempo_note(&tempo));
        info.processing.extend(gain_note(gain));
        info.processing.extend(
            prev.crossfeed
                .as_ref()
                .map(|c| crossfeed_note(c.strength())),
        );
        info.processing
            .extend(prev.eq.as_ref().map(|e| e.params().describe()));
        Session {
            item,
            format,
            decoder,
            track_id,
            src_rate: info.sample_rate,
            src_channels,
            graph,
            quality,
            producer: prev.producer,
            capacity: prev.capacity,
            pending: Vec::new(),
            pending_pos: 0,
            eof: false,
            start_offset: 0.0,
            duration,
            info,
            pushed: prev.pushed,
            trim,
            ts_to_frames,
            gain: gain_factor(gain),
            eq: prev.eq,
            crossfeed: prev.crossfeed,
            stretch,
            speed,
            tempo,
            skipper: skip_silence
                .then(|| crate::silence::Skipper::standard(src_rate, src_channels)),
            skips: SkipLog::default(),
        }
    }
}

struct Engine {
    state: Arc<Mutex<PlayerState>>,
    options: AudioOptions,
    sink: Box<dyn Sink>,
    /// What the output accepts (asked once per output opening).
    caps: Option<Caps>,
    /// What the platform says the active output takes; preferred over `caps`.
    caps_override: Option<Caps>,
    /// Bits per sample of the music the open stream was opened for (bit-perfect mode).
    out_bits: Option<u32>,
    output: Option<Stream>,
    session: Option<Session>,
    display: Option<Display>,
    switch: Option<Switch>,
    /// The next track couldn't be joined gaplessly (different rate, error).
    no_preload: bool,
    volume: Arc<AtomicU32>,
    /// Queue order before shuffle was turned on.
    unshuffled: Option<Vec<QueueItem>>,
    rng: u64,
}

impl Engine {
    fn st(&self) -> std::sync::MutexGuard<'_, PlayerState> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn rand(&mut self) -> u64 {
        // xorshift64*
        self.rng ^= self.rng >> 12;
        self.rng ^= self.rng << 25;
        self.rng ^= self.rng >> 27;
        self.rng.wrapping_mul(0x2545F4914F6CDD1D)
    }

    fn load(&mut self, index: usize, at: f64, autoplay: bool) {
        let Some(item) = self.st().queue.get(index).cloned() else {
            self.stop();
            return;
        };
        {
            let mut s = self.st();
            s.index = Some(index);
            s.status = Status::Loading;
            s.error = None;
            s.position = at;
        }
        match self.start_session(item, at) {
            Ok(()) => {
                let (dur, info) = {
                    let s = self.session.as_ref().unwrap();
                    (s.duration, s.info.clone())
                };
                let s = self.session.as_ref().unwrap();
                self.display = Some(Display {
                    skips: s.skips.clone(),
                    speed: s.speed,
                    item_id: s.item.id.clone(),
                    base: 0,
                    start_offset: s.start_offset,
                    duration: s.duration,
                    counted: false,
                });
                self.switch = None;
                self.no_preload = false;
                if let Some(out) = &self.output {
                    if autoplay {
                        out.play();
                    } else {
                        out.pause();
                    }
                }
                let mut s = self.st();
                s.duration = dur;
                s.info = info;
                s.status = if autoplay {
                    Status::Playing
                } else {
                    Status::Paused
                };
            }
            Err(e) => {
                log::error!("cannot play: {e}");
                self.session = None;
                let mut s = self.st();
                s.status = Status::Stopped;
                s.error = Some(e);
            }
        }
    }

    fn start_session(&mut self, item: QueueItem, at: f64) -> Result<(), PlayError> {
        let (mut format, decoder, track_id, mut info, duration) =
            open_file(&item.path).map_err(|detail| PlayError {
                kind: if detail.starts_with("cannot open file") {
                    PlayErrorKind::Missing
                } else {
                    PlayErrorKind::Unsupported
                },
                detail,
            })?;
        let (trim, ts_to_frames) = gapless_trim(&item, format.as_ref(), track_id, info.sample_rate);
        let duration = item_duration(&item, duration);
        let mut start_offset = 0.0;
        // Positions count from the music: after encoder priming, and from a CUE track's start.
        let skip = trim.delay as f64 / info.sample_rate.max(1) as f64;
        if (at > 0.0 || skip > 0.05) && seek_to(format.as_mut(), track_id, at + skip) {
            start_offset = at;
        }
        // The output rate: the file's own if the output takes it (see `choose_rate`).
        if self.caps.is_none() || self.output.is_none() {
            self.caps = Some(self.sink.caps().unwrap_or(Caps {
                rates: vec![],
                any_rate: true,
                channels: 2,
            }));
        }
        let want = self.output_rate_for(info.sample_rate);
        let bits = info.bits_per_sample;
        // Bit-perfect streams carry the music's own format, so a new depth needs a new stream.
        let need_new = match &self.output {
            Some(o) => o.rate != want || (self.options.output.bit_perfect && self.out_bits != bits),
            None => true,
        };
        if need_new {
            self.output = None; // close the old stream first
            self.sink.set_source_bits(bits);
            self.out_bits = bits;
            let opened = self.sink.open(want, self.volume.clone());
            self.output = Some(opened.map_err(|detail| PlayError {
                kind: PlayErrorKind::Output,
                detail,
            })?);
        }
        let out = self.output.as_ref().unwrap();
        info.output_rate = out.rate;
        info.output = out.info.describe(out.rate);
        let capacity = (out.rate as usize * out.channels) / 2; // ~0.5 s
        let (producer, consumer) = rtrb::RingBuffer::new(capacity);
        *out.slot.lock().unwrap_or_else(|p| p.into_inner()) = Some(consumer);
        out.played.store(0, Ordering::Relaxed);
        let src_channels = info.channels.max(1) as usize;
        let quality = self.options.quality;
        let tempo = self.item_tempo();
        let (graph, stretch, speed) = build_graph(
            info.sample_rate,
            src_channels,
            out.rate,
            out.channels,
            quality,
            &tempo,
        );
        let gain = self.item_gain(&item);
        let eq = self.make_eq(out.rate, out.channels);
        let crossfeed = self.make_crossfeed(out.rate, out.channels);
        info.processing = graph.describe();
        info.processing.extend(tempo_note(&tempo));
        info.processing.extend(gain_note(gain));
        info.processing
            .extend(crossfeed.as_ref().map(|c| crossfeed_note(c.strength())));
        info.processing
            .extend(eq.as_ref().map(|e| e.params().describe()));
        out.dither
            .store(wants_dither(&info, out), Ordering::Relaxed);
        let mut session = Session {
            item,
            format,
            decoder,
            track_id,
            src_rate: info.sample_rate,
            src_channels,
            graph,
            quality,
            producer,
            capacity,
            pending: Vec::new(),
            pending_pos: 0,
            eof: false,
            start_offset,
            trim,
            ts_to_frames,
            gain: gain_factor(gain),
            eq,
            crossfeed,
            stretch,
            speed,
            tempo,
            skipper: self
                .options
                .skip_silence
                .then(|| crate::silence::Skipper::standard(info.sample_rate, src_channels)),
            skips: SkipLog::default(),
            duration,
            info,
            pushed: 0,
        };
        session
            .pump(out.channels, out.rate)
            .map_err(|detail| PlayError {
                kind: PlayErrorKind::Unsupported,
                detail,
            })?;
        self.session = Some(session);
        Ok(())
    }

    fn stop(&mut self) {
        self.session = None;
        self.display = None;
        self.switch = None;
        if let Some(o) = &self.output {
            o.pause();
            *o.slot.lock().unwrap_or_else(|p| p.into_inner()) = None;
        }
        let mut s = self.st();
        s.status = Status::Stopped;
        s.position = 0.0;
    }

    /// The output rate for a source rate: what the output takes (`choose_rate`), within the
    /// owner's maximum rate.
    fn output_rate_for(&self, src: u32) -> u32 {
        let out = &self.options.output;
        if out.fixed_rate > 0 && !out.bit_perfect {
            return out.fixed_rate;
        }
        let caps = self
            .caps_override
            .clone()
            .or(self.caps.clone())
            .unwrap_or(Caps {
                rates: vec![],
                any_rate: true,
                channels: 2,
            });
        let max = self.options.max_rate;
        // Bit-perfect plays the song's own rate when the path takes it (the limit is for
        // conversions Loss chooses, and there are none here).
        if max == 0 || src <= max || out.bit_perfect {
            return choose_rate(src, &caps);
        }
        // Above the limit: choose among the allowed rates as if the output took only those.
        let pool: Vec<u32> = if caps.any_rate || caps.rates.is_empty() {
            crate::sink::COMMON_RATES.to_vec()
        } else {
            caps.rates.clone()
        };
        let allowed: Vec<u32> = pool.into_iter().filter(|&r| r <= max).collect();
        if allowed.is_empty() {
            return choose_rate(src, &caps);
        }
        choose_rate(
            src,
            &Caps {
                rates: allowed,
                any_rate: false,
                channels: caps.channels,
            },
        )
    }

    /// Samples the listener has heard on the current output (played minus output latency).
    fn heard_samples(&self) -> u64 {
        self.output
            .as_ref()
            .map(|o| o.heard_frames() * o.channels as u64)
            .unwrap_or(0)
    }

    /// Where the listener is in the current track, in seconds: sample-accurate, counting only
    /// what has come out of the speaker (not what is still in the output's buffer).
    fn position(&self) -> f64 {
        match (&self.display, &self.output) {
            (Some(d), Some(o)) => {
                let heard = self.heard_samples().saturating_sub(d.base);
                // Output seconds heard, times the source seconds each one carries.
                d.start_offset
                    + heard as f64 / (o.rate as f64 * o.channels as f64) * d.speed
                    + skipped_before(&d.skips, self.heard_samples())
            }
            _ => 0.0,
        }
    }

    fn next_index(&self, user: bool) -> Option<usize> {
        let s = self.st();
        let i = s.index?;
        if s.repeat == Repeat::One && !user {
            return Some(i);
        }
        if i + 1 < s.queue.len() {
            Some(i + 1)
        } else if s.repeat != Repeat::Off && !s.queue.is_empty() {
            Some(0)
        } else {
            None
        }
    }

    fn set_shuffle(&mut self, on: bool) {
        let (mut queue, index) = {
            let s = self.st();
            if s.shuffle == on {
                return;
            }
            (s.queue.clone(), s.index)
        };
        let current = index.and_then(|i| queue.get(i).cloned());
        if on {
            self.unshuffled = Some(queue.clone());
            if let Some(i) = index {
                let cur = queue.remove(i);
                queue.insert(0, cur);
            }
            let start = if current.is_some() { 1 } else { 0 };
            for i in (start + 1..queue.len()).rev() {
                let j = start + (self.rand() as usize % (i - start + 1));
                queue.swap(i, j);
            }
        } else if let Some(orig) = self.unshuffled.take() {
            queue = orig;
        }
        let new_index = current.and_then(|c| queue.iter().position(|q| q.id == c.id));
        let mut s = self.st();
        s.queue = queue;
        s.index = new_index.or(index);
        s.shuffle = on;
    }

    fn handle(&mut self, cmd: Command) {
        match cmd {
            Command::SetQueue { items, start } => {
                self.unshuffled = None;
                let shuffle = {
                    let mut s = self.st();
                    s.queue = items;
                    let sh = s.shuffle;
                    s.shuffle = false;
                    sh
                };
                self.load(start, 0.0, true);
                if shuffle {
                    self.set_shuffle(true);
                }
            }
            Command::Enqueue(items) => {
                let empty = {
                    let mut s = self.st();
                    s.queue.extend(items.clone());
                    s.index.is_none()
                };
                if let Some(orig) = &mut self.unshuffled {
                    orig.extend(items);
                }
                if empty {
                    self.load(0, 0.0, true);
                }
            }
            Command::PlayNext(items) => {
                let empty = {
                    let mut s = self.st();
                    let at = s.index.map(|i| i + 1).unwrap_or(0).min(s.queue.len());
                    for (k, it) in items.iter().enumerate() {
                        s.queue.insert(at + k, it.clone());
                    }
                    s.index.is_none()
                };
                if let Some(orig) = &mut self.unshuffled {
                    orig.extend(items);
                }
                if empty {
                    self.load(0, 0.0, true);
                }
            }
            Command::Remove(i) => {
                let state = self.state.clone();
                let reload = {
                    let mut s = state.lock().unwrap_or_else(|p| p.into_inner());
                    if i >= s.queue.len() {
                        return;
                    }
                    let removed = s.queue.remove(i);
                    if let Some(orig) = &mut self.unshuffled {
                        if let Some(p) = orig.iter().position(|q| q == &removed) {
                            orig.remove(p);
                        }
                    }
                    match s.index {
                        Some(c) if c > i => {
                            s.index = Some(c - 1);
                            None
                        }
                        Some(c) if c == i => Some(c),
                        _ => None,
                    }
                };
                if let Some(c) = reload {
                    let len = self.st().queue.len();
                    if c < len {
                        self.load(c, 0.0, true);
                    } else {
                        self.stop();
                        self.st().index = None;
                    }
                }
            }
            Command::JumpTo(i) => self.load(i, 0.0, true),
            Command::Play => self.resume(),
            Command::Pause => self.pause(),
            Command::Toggle => {
                let status = self.st().status;
                match status {
                    Status::Playing => self.pause(),
                    _ => self.resume(),
                }
            }
            Command::Next => {
                if let Some(i) = self.next_index(true) {
                    self.load(i, 0.0, true);
                }
            }
            Command::Previous => {
                let pos = self.position();
                let idx = self.st().index;
                match idx {
                    Some(i) if pos < 3.0 && i > 0 => self.load(i - 1, 0.0, true),
                    Some(i) => self.load(i, 0.0, true),
                    None => {}
                }
            }
            Command::Seek(t) => {
                let (idx, playing) = {
                    let s = self.st();
                    (s.index, s.status == Status::Playing)
                };
                if let Some(i) = idx {
                    let dur = self.st().duration;
                    let t = if dur > 0.0 {
                        t.clamp(0.0, dur - 0.25)
                    } else {
                        t.max(0.0)
                    };
                    self.load(i, t, playing);
                }
            }
            Command::SetVolume(v) => {
                let v = v.clamp(0.0, 1.0);
                self.volume.store(v.to_bits(), Ordering::Relaxed);
                self.st().volume = v;
            }
            Command::SetShuffle(on) => self.set_shuffle(on),
            Command::SetAudioOptions(o) => {
                let reopen = o.output != self.options.output;
                let reskip = o.skip_silence != self.options.skip_silence;
                let retempo = o.tempo != self.options.tempo;
                let regain = o.normalize != self.options.normalize
                    || o.eq != self.options.eq
                    || o.crossfeed != self.options.crossfeed;
                self.options = o;
                if regain && !reopen {
                    self.regain();
                }
                // Speed and pitch change how the whole chain is built: start the track again
                // where it is (like a seek).
                if retempo && !reopen && self.session.is_some() {
                    self.reload_here(false);
                } else if reskip {
                    // Skipping starts or stops from here on in the track that's playing.
                    let on = self.options.skip_silence;
                    if let Some(s) = self.session.as_mut() {
                        s.skipper = on
                            .then(|| crate::silence::Skipper::standard(s.src_rate, s.src_channels));
                    }
                }
                if reopen {
                    self.sink.set_prefs(self.options.output);
                    if self.output.is_some() {
                        self.reload_here(true);
                    }
                }
            }
            Command::SetOutputCaps(c) => {
                self.caps_override = c;
                // Playing at a rate the new output doesn't suit: reopen where we are.
                let wrong = match (&self.session, &self.output) {
                    (Some(s), Some(o)) => self.output_rate_for(s.info.sample_rate) != o.rate,
                    _ => false,
                };
                if wrong {
                    self.reload_here(false);
                }
            }
            Command::SetRepeat(r) => self.st().repeat = r,
            Command::Stop => {
                self.stop();
                let mut s = self.st();
                s.index = None;
                s.queue.clear();
            }
        }
    }

    fn pause(&mut self) {
        if let Some(o) = &self.output {
            o.pause();
        }
        let mut s = self.st();
        if s.status == Status::Playing {
            s.status = Status::Paused;
        }
    }

    fn resume(&mut self) {
        let (status, idx) = {
            let s = self.st();
            (s.status, s.index)
        };
        match (status, &self.session) {
            (Status::Paused, Some(_)) => {
                if let Some(o) = &self.output {
                    o.play();
                }
                self.st().status = Status::Playing;
            }
            (Status::Stopped, _) => {
                let start = idx.unwrap_or(0);
                if !self.st().queue.is_empty() {
                    self.load(start, 0.0, true);
                }
            }
            _ => {}
        }
    }

    fn tick(&mut self) {
        // The device went away (unplugged, re-routed): open the output again where we were.
        if self
            .output
            .as_ref()
            .is_some_and(|o| o.lost.load(Ordering::Relaxed))
        {
            log::info!("output lost; reopening");
            self.caps = None;
            self.reload_here(true);
        }
        let Some(out) = &self.output else { return };
        let (ch, rate) = (out.channels, out.rate);
        let played = self.heard_samples();

        // A gapless join reached the speaker: show the next track.
        if self.switch.as_ref().is_some_and(|sw| played >= sw.at) {
            let sw = self.switch.take().unwrap();
            let mut st = self.st();
            let current = st.index.unwrap_or(0);
            // Look the track up by id: the queue may have changed meanwhile.
            let idx = st
                .queue
                .iter()
                .enumerate()
                .skip(current)
                .chain(st.queue.iter().enumerate().take(current))
                .find(|(i, q)| q.id == sw.item_id && (*i != current || st.repeat == Repeat::One))
                .map(|(i, _)| i)
                .unwrap_or(current);
            st.index = Some(idx);
            st.duration = sw.duration;
            out.dither
                .store(wants_dither(&sw.info, out), Ordering::Relaxed);
            st.info = sw.info;
            st.position = 0.0;
            drop(st);
            self.display = Some(Display {
                skips: sw.skips,
                speed: sw.speed,
                item_id: sw.item_id,
                base: sw.at,
                start_offset: 0.0,
                duration: sw.duration,
                counted: false,
            });
        }

        let pos = self.position();
        if let Some(s) = &mut self.session {
            if let Err(e) = s.pump(ch, rate) {
                log::error!("decode error: {e}");
                s.eof = true;
            }
        }

        // Count a play after 30 s or half the track.
        if let Some(d) = &mut self.display {
            let threshold = if d.duration > 0.0 {
                (d.duration * 0.5).min(30.0)
            } else {
                30.0
            };
            if !d.counted && pos >= threshold {
                d.counted = true;
                let id = d.item_id.clone();
                self.st().play_events.push(id);
            }
        }
        {
            let mut st = self.st();
            if st.status == Status::Playing {
                st.position = if st.duration > 0.0 {
                    pos.min(st.duration)
                } else {
                    pos
                };
            }
        }

        // Gapless: once this track is fully decoded, start decoding the next
        // one into the same buffer.
        let decoded_all = self.session.as_ref().is_some_and(|s| s.decoded_all());
        if decoded_all && self.switch.is_none() && !self.no_preload {
            self.preload_next(rate, ch);
        }

        let finished = self.session.as_ref().is_some_and(|s| s.drained()) && self.switch.is_none();
        if finished {
            match self.next_index(false) {
                Some(i) => self.load(i, 0.0, true),
                None => {
                    self.stop();
                    let mut s = self.st();
                    s.position = 0.0;
                }
            }
        }
    }

    fn preload_next(&mut self, out_rate: u32, out_channels: usize) {
        let Some(i) = self.next_index(false) else {
            self.no_preload = true;
            return;
        };
        let Some(item) = self.st().queue.get(i).cloned() else {
            self.no_preload = true;
            return;
        };
        let bit_perfect = self.options.output.bit_perfect;
        let same_output = |info: &StreamInfo| {
            self.output_rate_for(info.sample_rate) == out_rate
                && (!bit_perfect || info.bits_per_sample == self.out_bits)
        };
        match open_file(&item.path) {
            // The output stays as it is (same rate after conversion), so join seamlessly, even
            // between a 44.1 kHz and a 48 kHz file when both play at 48 kHz.
            Ok(opened) if same_output(&opened.3) => {
                let prev = self.session.take().unwrap();
                let at = prev.pushed;
                let quality = self.options.quality;
                let gain = self.item_gain(&item);
                let tempo = self.item_tempo();
                let next = Session::handover(
                    prev,
                    item,
                    opened,
                    out_rate,
                    out_channels,
                    quality,
                    gain,
                    tempo,
                    self.options.skip_silence,
                );
                self.switch = Some(Switch {
                    skips: next.skips.clone(),
                    speed: next.speed,
                    at,
                    item_id: next.item.id.clone(),
                    duration: next.duration,
                    info: next.info.clone(),
                });
                self.session = Some(next);
            }
            // The output must reopen at another rate, or an error (shown when the track is
            // loaded normally).
            _ => self.no_preload = true,
        }
    }
}

impl Engine {
    /// Loads the current track again where it is (same play/pause state), on a fresh output
    /// stream when `reopen` is set.
    fn reload_here(&mut self, reopen: bool) {
        let (idx, playing) = {
            let s = self.st();
            (s.index, s.status == Status::Playing)
        };
        let at = self.position();
        if reopen {
            self.output = None;
        }
        match idx {
            Some(i) if self.session.is_some() => self.load(i, at, playing),
            _ => {}
        }
    }
}

/// "gain −6.5 dB (album)" for the processing list.
fn gain_note(gain: Option<(f64, bool)>) -> Option<String> {
    gain.map(|(db, album)| {
        format!(
            "gain {db:.1} dB ({})",
            if album { "album" } else { "track" }
        )
    })
}

fn crossfeed_note(s: crate::crossfeed::Strength) -> String {
    format!("crossfeed ({})", s.name())
}

fn gain_factor(gain: Option<(f64, bool)>) -> f64 {
    gain.map(|(db, _)| 10f64.powf(db / 20.0)).unwrap_or(1.0)
}

impl Engine {
    /// Speed and pitch for the next session: normal on bit-perfect paths (the audio doesn't pass
    /// through Loss' processing there).
    fn item_tempo(&self) -> crate::Tempo {
        if self.options.output.bit_perfect {
            crate::Tempo::default()
        } else {
            self.options.tempo
        }
    }

    /// The ReplayGain for an item, or None: off, no tags, or a bit-perfect path.
    fn item_gain(&self, item: &QueueItem) -> Option<(f64, bool)> {
        if self.options.output.bit_perfect {
            return None;
        }
        self.options.normalize.gain_for(item)
    }

    /// The equalizer for an output, or None when it's off, flat or bypassed (bit-perfect).
    fn make_eq(&self, rate: u32, channels: usize) -> Option<crate::eq::Equalizer> {
        let params = self.options.eq.clone()?;
        if self.options.output.bit_perfect || params.is_flat() {
            return None;
        }
        Some(crate::eq::Equalizer::new(params, rate, channels))
    }

    /// Crossfeed for a stereo output, or None when it's off or bypassed (bit-perfect).
    fn make_crossfeed(&self, rate: u32, channels: usize) -> Option<crate::crossfeed::Crossfeed> {
        let strength = self.options.crossfeed?;
        if self.options.output.bit_perfect || channels != 2 {
            return None;
        }
        Some(crate::crossfeed::Crossfeed::new(strength, rate))
    }

    /// Levelling or equalizer settings changed: the playing track takes them from here on
    /// (the equalizer keeps its filter memory, so there's no click).
    fn regain(&mut self) {
        let Some(item) = self.session.as_ref().map(|s| s.item.clone()) else {
            return;
        };
        let gain = self.item_gain(&item);
        let (rate, channels) = match &self.output {
            Some(o) => (o.rate, o.channels),
            None => return,
        };
        let want = self.make_eq(rate, channels).map(|e| e.params().clone());
        let want_cf = self.make_crossfeed(rate, channels).map(|c| c.strength());
        let Some(s) = self.session.as_mut() else {
            return;
        };
        s.gain = gain_factor(gain);
        match (want_cf, s.crossfeed.as_ref().map(|c| c.strength())) {
            (Some(w), Some(h)) if w == h => {}
            (Some(w), _) => s.crossfeed = Some(crate::crossfeed::Crossfeed::new(w, rate)),
            (None, _) => s.crossfeed = None,
        }
        match (want, s.eq.as_mut()) {
            (Some(p), Some(eq)) => eq.set(p),
            (Some(p), None) => s.eq = Some(crate::eq::Equalizer::new(p, rate, channels)),
            (None, _) => s.eq = None,
        }
        s.info.processing = s.graph.describe();
        s.info.processing.extend(gain_note(gain));
        let cf_note = s.crossfeed.as_ref().map(|c| crossfeed_note(c.strength()));
        s.info.processing.extend(cf_note);
        s.info
            .processing
            .extend(s.eq.as_ref().map(|e| e.params().describe()));
        let info = s.info.clone();
        if let Some(out) = &self.output {
            out.dither
                .store(wants_dither(&info, out), Ordering::Relaxed);
        }
        // The track on screen is this one unless a gapless switch is still on its way: then the
        // switch carries the new list, and the track on screen gets the new equalizer entry (its
        // own gain and conversion are already in the buffer).
        match self.switch.as_mut() {
            None => self.st().info.processing = info.processing,
            Some(sw) => {
                sw.info.processing = info.processing.clone();
                let notes: Vec<String> = info
                    .processing
                    .iter()
                    .filter(|p| p.starts_with("equalizer") || p.starts_with("crossfeed"))
                    .cloned()
                    .collect();
                let mut st = self.st();
                st.info
                    .processing
                    .retain(|p| !p.starts_with("equalizer") && !p.starts_with("crossfeed"));
                st.info.processing.extend(notes);
            }
        }
    }
}

/// Integer output dithers unless the samples arrive untouched with no more bits than it holds.
fn wants_dither(info: &StreamInfo, out: &Stream) -> bool {
    out.info
        .format
        .int_bits()
        .is_some_and(|bits| !info.untouched() || info.bits_per_sample.is_none_or(|b| b > bits))
}

pub fn run(rx: Receiver<Command>, state: Arc<Mutex<PlayerState>>) {
    #[cfg(target_os = "android")]
    let sink: Box<dyn Sink> = Box::new(crate::aaudio::AAudioSink::default());
    #[cfg(not(target_os = "android"))]
    let sink: Box<dyn Sink> = Box::new(crate::sink::CpalSink);
    let mut engine = Engine::new(state, sink);
    loop {
        let active = engine.session.is_some();
        let wait = Duration::from_millis(if active { 10 } else { 200 });
        match rx.recv_timeout(wait) {
            Ok(cmd) => {
                engine.handle(cmd);
                // Drain any queued commands before decoding more.
                while let Ok(cmd) = rx.try_recv() {
                    engine.handle(cmd);
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        engine.tick();
    }
}

impl Engine {
    fn new(state: Arc<Mutex<PlayerState>>, sink: Box<dyn Sink>) -> Engine {
        let vol = state.lock().map(|s| s.volume).unwrap_or(1.0);
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x9E3779B97F4A7C15)
            | 1;
        Engine {
            state,
            options: AudioOptions::default(),
            sink,
            caps: None,
            caps_override: None,
            out_bits: None,
            output: None,
            session: None,
            display: None,
            switch: None,
            no_preload: false,
            volume: Arc::new(AtomicU32::new(vol.to_bits())),
            unshuffled: None,
            rng: seed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::GainMode;
    use std::path::PathBuf;

    /// 16-bit stereo 440 Hz WAV holding samples `from..to` of a continuous sine.
    fn write_wav(path: &Path, rate: u32, _secs: f32, from: u32, to: u32) {
        let frames = to - from;
        let data_len = frames * 4;
        let mut b = Vec::new();
        b.extend_from_slice(b"RIFF");
        b.extend_from_slice(&(36 + data_len).to_le_bytes());
        b.extend_from_slice(b"WAVEfmt ");
        b.extend_from_slice(&16u32.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes()); // PCM
        b.extend_from_slice(&2u16.to_le_bytes()); // stereo
        b.extend_from_slice(&rate.to_le_bytes());
        b.extend_from_slice(&(rate * 4).to_le_bytes());
        b.extend_from_slice(&4u16.to_le_bytes());
        b.extend_from_slice(&16u16.to_le_bytes());
        b.extend_from_slice(b"data");
        b.extend_from_slice(&data_len.to_le_bytes());
        for i in from..to {
            let v =
                ((i as f32 * 440.0 * std::f32::consts::TAU / rate as f32).sin() * 8000.0) as i16;
            b.extend_from_slice(&v.to_le_bytes());
            b.extend_from_slice(&v.to_le_bytes());
        }
        std::fs::write(path, b).unwrap();
    }

    /// A 24-bit stereo WAV with a quiet tone.
    fn write_wav24(path: &Path, rate: u32, frames: u32) {
        let data_len = frames * 6;
        let mut b = Vec::new();
        b.extend_from_slice(b"RIFF");
        b.extend_from_slice(&(36 + data_len).to_le_bytes());
        b.extend_from_slice(b"WAVEfmt ");
        b.extend_from_slice(&16u32.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&2u16.to_le_bytes());
        b.extend_from_slice(&rate.to_le_bytes());
        b.extend_from_slice(&(rate * 6).to_le_bytes());
        b.extend_from_slice(&6u16.to_le_bytes());
        b.extend_from_slice(&24u16.to_le_bytes());
        b.extend_from_slice(b"data");
        b.extend_from_slice(&data_len.to_le_bytes());
        for i in 0..frames {
            let v = ((i as f32 * 0.05).sin() * 1_000_000.0) as i32;
            let x = &v.to_le_bytes()[..3];
            b.extend_from_slice(x);
            b.extend_from_slice(x);
        }
        std::fs::write(path, b).unwrap();
    }

    /// Decodes `paths` back to back the way the engine does (gapless handover
    /// into one ring buffer). Returns interleaved samples and the sample index
    /// where each later track starts.
    fn decode_chain(paths: &[PathBuf], out_rate: u32, out_ch: usize) -> (Vec<f32>, Vec<u64>) {
        let capacity = out_rate as usize * out_ch;
        let (producer, mut consumer) = rtrb::RingBuffer::new(capacity);
        let item = |p: &PathBuf| QueueItem {
            id: p.display().to_string(),
            path: p.clone(),
            ..Default::default()
        };
        let (format, decoder, track_id, info, duration) = open_file(&paths[0]).unwrap();
        let (trim, ts_to_frames) = gapless_trim(
            &item(&paths[0]),
            format.as_ref(),
            track_id,
            info.sample_rate,
        );
        let mut s = Session {
            item: item(&paths[0]),
            format,
            decoder,
            track_id,
            src_rate: info.sample_rate,
            src_channels: info.channels as usize,
            graph: Graph::new(
                info.sample_rate,
                info.channels as usize,
                out_rate,
                out_ch,
                Quality::Best,
            ),
            quality: Quality::Best,
            producer,
            capacity,
            pending: vec![],
            pending_pos: 0,
            eof: false,
            start_offset: 0.0,
            duration,
            info,
            pushed: 0,
            trim,
            ts_to_frames,
            gain: 1.0,
            eq: None,
            crossfeed: None,
            stretch: None,
            speed: 1.0,
            tempo: crate::Tempo::default(),
            skipper: None,
            skips: SkipLog::default(),
        };
        let mut out = Vec::new();
        let mut joins = Vec::new();
        let mut next = 1;
        for _ in 0..100_000 {
            s.pump(out_ch, out_rate).unwrap();
            while let Ok(v) = consumer.pop() {
                out.push(v);
            }
            if s.decoded_all() && next < paths.len() {
                joins.push(s.pushed);
                let opened = open_file(&paths[next]).unwrap();
                s = Session::handover(
                    s,
                    item(&paths[next]),
                    opened,
                    out_rate,
                    out_ch,
                    Quality::Best,
                    None,
                    crate::Tempo::default(),
                    false,
                );
                next += 1;
            } else if s.drained() {
                break;
            }
        }
        assert!(s.drained(), "decoding did not finish");
        (out, joins)
    }

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name)
    }

    fn rms(v: &[f32]) -> f32 {
        (v.iter().map(|x| x * x).sum::<f32>() / v.len().max(1) as f32).sqrt()
    }

    #[test]
    fn decodes_wav_fully_with_resampling() {
        let dir = std::env::temp_dir().join(format!("loss-audio-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("tone.wav");
        write_wav(&path, 44100, 1.0, 0, 44100);
        let (_, _, _, info, duration) = open_file(&path).unwrap();
        assert_eq!(info.sample_rate, 44100);
        assert_eq!(info.bits_per_sample, Some(16));
        assert!((duration - 1.0).abs() < 0.01, "{duration}");
        let (out, _) = decode_chain(&[path], 48000, 2);
        let frames = out.len() / 2;
        assert!((frames as i64 - 48000).abs() < 50, "{frames}");
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn plays_opus_in_ogg_and_webm() {
        for name in ["tone.opus", "tone.webm"] {
            let path = fixture(name);
            let (_, _, _, info, _) = open_file(&path).unwrap();
            assert_eq!(info.codec, "opus", "{name}");
            assert_eq!(info.sample_rate, 48000, "{name}");
            let (out, _) = decode_chain(&[path], 48000, 2);
            let frames = out.len() / 2;
            // One second of audio, with the encoder pre-skip removed.
            assert!(
                (frames as i64 - 48000).abs() <= 960,
                "{name}: {frames} frames"
            );
            // A 440 Hz tone at the generator's level, not silence or noise.
            let level = rms(&out[4800..out.len() - 4800]);
            assert!(level > 0.05 && level < 0.6, "{name}: rms {level}");
        }
    }

    /// Files from `scripts/make-fixtures.sh` (`target/fixtures`, or `LOSS_FIXTURES`).
    fn generated(name: &str) -> Option<PathBuf> {
        let dir = std::env::var_os("LOSS_FIXTURES")
            .map(PathBuf::from)
            .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/fixtures"));
        let p = dir.join(name);
        p.exists().then_some(p)
    }

    /// Run `scripts/make-fixtures.sh` first; CI does.
    #[test]
    #[ignore]
    fn generated_fixtures_decode() {
        let cases = [
            ("flac-16-44100.flac", 44100, Some(16)),
            ("flac-24-48000.flac", 48000, Some(24)),
            ("flac-24-96000.flac", 96000, Some(24)),
            ("flac-24-192000.flac", 192000, Some(24)),
            // Symphonia doesn't report ALAC's bit depth; `alac_bits` reads the codec config.
            ("alac-16-44100.m4a", 44100, Some(16)),
            ("flac-24-384000.flac", 384000, Some(24)),
            // 5.1 folded to stereo.
            ("flac-51-48000.flac", 48000, Some(16)),
            ("wav-24-96000.wav", 96000, Some(24)),
            ("wavpack-16-44100.wv", 44100, Some(16)),
            ("wavpack-24-96000.wv", 96000, Some(24)),
            ("wavpack-f32-48000.wv", 48000, None),
            ("wav-f32-48000.wav", 48000, None),
            ("tagged-cover.flac", 44100, Some(16)),
        ];
        for (name, rate, bits) in cases {
            let path =
                generated(name).unwrap_or_else(|| panic!("{name}: run scripts/make-fixtures.sh"));
            let (_, _, _, info, duration) =
                open_file(&path).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(info.sample_rate, rate, "{name}");
            if bits.is_some() {
                assert_eq!(info.bits_per_sample, bits, "{name}");
            }
            assert!((duration - 2.0).abs() < 0.05, "{name}: {duration}");
            let (out, _) = decode_chain(std::slice::from_ref(&path), 48000, 2);
            let secs = out.len() as f64 / 2.0 / 48000.0;
            assert!((secs - 2.0).abs() < 0.02, "{name}: decoded {secs} s");
            // ffmpeg's sine source is at 1/8 of full scale (rms ≈ 0.088).
            let level = rms(&out[9600..out.len() - 9600]);
            assert!(level > 0.05 && level < 0.15, "{name}: rms {level}");
            // Seeking to 1 s lands at or before it, and decoding goes on from there.
            let (mut format, mut decoder, track_id, _, _) = open_file(&path).unwrap();
            let to = format
                .seek(
                    SeekMode::Accurate,
                    SeekTo::Time {
                        time: Time::from(1.0),
                        track_id: Some(track_id),
                    },
                )
                .unwrap_or_else(|e| panic!("{name}: seek: {e}"));
            assert!(to.actual_ts <= to.required_ts, "{name}: {to:?}");
            decoder.reset();
            let packet = format
                .next_packet()
                .unwrap_or_else(|e| panic!("{name}: {e}"));
            assert!(decoder.decode(&packet).is_ok(), "{name}: decode after seek");
        }
        // Not decodable yet; must fail cleanly, not panic or play noise: 32-bit FLAC, which
        // Symphonia 0.5 treats as reserved (it also stops at 655,350 Hz, the old FLAC limit).
        // Revisit with Symphonia 0.6.
        let path = generated("flac-32-96000.flac").unwrap();
        assert!(open_file(&path).is_err(), "32-bit FLAC");
    }

    /// Longest run of near-silence around each join, in ms; and the total length in seconds.
    fn join_report(prefix: &str, ext: &str) -> (Vec<f64>, f64) {
        let paths: Vec<PathBuf> = (0..3)
            .map(|i| {
                generated(&format!("gapless/{prefix}-{i}.{ext}"))
                    .expect("run scripts/make-fixtures.sh")
            })
            .collect();
        let rate = 48000;
        let (out, joins) = decode_chain(&paths, rate, 2);
        let left: Vec<f32> = out.iter().step_by(2).copied().collect();
        let gaps = joins
            .iter()
            .map(|&j| {
                let j = (j / 2) as usize;
                let window = &left[j.saturating_sub(4800)..(j + 4800).min(left.len())];
                // Longest run below -40 dBFS (the sine peaks near -6 dB, so a dip only
                // lasts one sample at its zero crossings).
                let mut run = 0usize;
                let mut longest = 0usize;
                for v in window {
                    if v.abs() < 0.01 {
                        run += 1;
                        longest = longest.max(run);
                    } else {
                        run = 0;
                    }
                }
                longest as f64 * 1000.0 / rate as f64
            })
            .collect();
        (gaps, left.len() as f64 / rate as f64)
    }

    /// Gapless across real encoders: the split sine plays as one tone.
    #[test]
    #[ignore]
    fn generated_gapless_sets() {
        for (prefix, ext, max_gap_ms) in [
            ("flac", "flac", 0.2),
            ("mp3", "mp3", 1.0),
            ("opus", "opus", 1.0),
            // MP4: priming and padding cut from the edit list (mp4gap).
            ("aac", "m4a", 1.0),
        ] {
            let (gaps, secs) = join_report(prefix, ext);
            eprintln!("{prefix}: joins {gaps:?} ms of silence, total {secs:.3} s");
            assert!((secs - 3.0).abs() < 0.03, "{prefix}: {secs} s");
            for g in gaps {
                assert!(g <= max_gap_ms, "{prefix}: {g} ms gap at a join");
            }
        }
    }

    #[test]
    fn dsd_plays_through_the_engine() {
        let rate = 2_822_400u32;
        let n = rate as usize / 4; // 0.25 s
        let tone =
            (0..n).map(|i| 0.5 * (i as f64 * 440.0 * std::f64::consts::TAU / rate as f64).sin());
        let file = crate::dsd::tests::dsf(rate, &crate::dsd::tests::modulate(tone));
        let path = tmp("tone.dsf");
        std::fs::write(&path, file).unwrap();
        let (_, _, _, info, duration) = open_file(&path).unwrap();
        assert_eq!((info.codec.as_str(), info.sample_rate), ("dsd64", 88_200));
        assert!(info.is_lossless());
        assert_eq!(info.label(), "DSD64");
        assert!((duration - 0.25).abs() < 0.001, "{duration}");
        let (out, _) = decode_chain(&[path], 48_000, 2);
        let secs = out.len() as f64 / 2.0 / 48_000.0;
        assert!((secs - 0.25).abs() < 0.002, "{secs}");
    }

    /// The CUE fixture: one 6 s tone, three 2 s tracks. Played in a row they must give back
    /// the tone without a gap or a jump at the joins, each track jumping into its file.
    #[test]
    #[ignore]
    fn generated_cue_tracks_play_back_to_back() {
        let path = generated("cue-album.flac").expect("run scripts/make-fixtures.sh");
        let (sink, clock) = FakeSink::new(
            Caps {
                rates: vec![],
                any_rate: true,
                channels: 2,
            },
            0,
        );
        let state = Arc::new(Mutex::new(PlayerState {
            volume: 1.0,
            ..Default::default()
        }));
        let mut e = Engine::new(state, Box::new(sink));
        let range = |n: u32, start: f64, end: Option<f64>| QueueItem {
            id: format!("cue#{n}"),
            path: path.clone(),
            start,
            end,
            ..Default::default()
        };
        e.handle(Command::SetQueue {
            items: vec![
                range(1, 0.0, Some(2.0)),
                range(2, 2.0, Some(4.0)),
                range(3, 4.0, None),
            ],
            start: 0,
        });
        assert!((e.st().duration - 2.0).abs() < 0.01);
        play(&mut e, &clock, 44_100 * 7, 2048);
        assert_eq!(e.st().status, Status::Stopped);
        let left: Vec<f32> = clock.heard().iter().step_by(2).copied().collect();
        let sound = left.iter().rposition(|v| v.abs() > 1e-4).unwrap() + 1;
        assert!((sound as i64 - 6 * 44_100).abs() < 200, "{sound} frames");
        let jump = left[..sound]
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0f32, f32::max);
        // A 330 Hz sine at 1/8 scale moves at most ~0.006 per sample.
        assert!(jump < 0.02, "discontinuity {jump}");

        // Starting at the second track jumps straight into the file.
        e.handle(Command::SetQueue {
            items: vec![range(2, 2.0, Some(4.0))],
            start: 0,
        });
        assert!(e.position().abs() < 0.01);
    }

    #[test]
    fn replaygain_modes_only_turn_down() {
        let n = |mode, preamp_db| crate::Normalize {
            enabled: true,
            mode,
            preamp_db,
        };
        let item = |t: Option<f64>, a: Option<f64>, run: bool| QueueItem {
            gain_track: t,
            gain_album: a,
            album_run: run,
            ..Default::default()
        };
        let it = item(Some(-6.0), Some(-8.0), false);
        assert_eq!(n(GainMode::Track, 0.0).gain_for(&it), Some((-6.0, false)));
        assert_eq!(n(GainMode::Album, 0.0).gain_for(&it), Some((-8.0, true)));
        // Automatic: album gain only for an album played in order.
        assert_eq!(n(GainMode::Auto, 0.0).gain_for(&it), Some((-6.0, false)));
        let run = item(Some(-6.0), Some(-8.0), true);
        assert_eq!(n(GainMode::Auto, 0.0).gain_for(&run), Some((-8.0, true)));
        // Missing values fall back to the other one.
        assert_eq!(
            n(GainMode::Album, 0.0).gain_for(&item(Some(-3.0), None, true)),
            Some((-3.0, false))
        );
        // Never louder: a quiet track's +4 dB stays at unity (no gain); pre-amp lowers it.
        assert_eq!(
            n(GainMode::Track, 0.0).gain_for(&item(Some(4.0), None, false)),
            None
        );
        assert_eq!(
            n(GainMode::Track, -6.0).gain_for(&item(Some(4.0), None, false)),
            Some((-2.0, false))
        );
        assert_eq!(crate::Normalize::default().gain_for(&it), None);
    }

    #[test]
    fn replaygain_switches_on_the_exact_sample_at_a_gapless_join() {
        let (sink, clock) = FakeSink::new(
            Caps {
                rates: vec![44100],
                any_rate: false,
                channels: 2,
            },
            0,
        );
        let state = Arc::new(Mutex::new(PlayerState {
            volume: 1.0,
            ..Default::default()
        }));
        let mut e = Engine::new(state, Box::new(sink));
        e.handle(Command::SetAudioOptions(AudioOptions {
            normalize: crate::Normalize {
                enabled: true,
                mode: GainMode::Track,
                preamp_db: 0.0,
            },
            ..AudioOptions::default()
        }));
        let (a, b) = (tmp("rg-a.wav"), tmp("rg-b.wav"));
        write_wav(&a, 44100, 0.2, 0, 8820);
        write_wav(&b, 44100, 0.2, 8820, 17640);
        let half = -20.0 * 2f64.log10(); // −6.02 dB
        e.handle(Command::SetQueue {
            items: vec![
                QueueItem {
                    gain_track: Some(half),
                    ..item(&a)
                },
                item(&b),
            ],
            start: 0,
        });
        assert_eq!(
            e.st().info.processing,
            vec!["gain -6.0 dB (track)".to_string()]
        );
        play(&mut e, &clock, 20_000, 1000);
        let heard: Vec<f32> = clock.heard().iter().step_by(2).copied().collect();
        for (i, h) in heard.iter().enumerate().take(17_640) {
            let raw = ((i as f32 * 440.0 * std::f32::consts::TAU / 44100.0).sin() * 8000.0) as i16;
            let want = raw as f32 / 32768.0 * if i < 8820 { 0.5 } else { 1.0 };
            assert!((h - want).abs() < 1e-6, "sample {i}: {h} vs {want}");
        }
    }

    #[test]
    fn replaygain_is_off_bit_perfect_and_follows_setting_changes() {
        let (mut e, clock) = hires_dac();
        let path = tmp("rg-live.wav");
        write_wav(&path, 44100, 1.0, 0, 44100);
        let tagged = QueueItem {
            gain_track: Some(-9.0),
            ..item(&path)
        };
        e.handle(Command::SetQueue {
            items: vec![tagged.clone()],
            start: 0,
        });
        play(&mut e, &clock, 4410, 1000);
        assert!(e.st().info.processing.is_empty());
        let on = crate::Normalize {
            enabled: true,
            mode: GainMode::Track,
            preamp_db: 0.0,
        };
        // Turned on while playing: the gain applies from here on.
        e.handle(Command::SetAudioOptions(AudioOptions {
            normalize: on,
            ..AudioOptions::default()
        }));
        assert_eq!(
            e.st().info.processing,
            vec!["gain -9.0 dB (track)".to_string()]
        );
        // Bit-perfect leaves the samples alone.
        e.handle(Command::SetAudioOptions(AudioOptions {
            normalize: on,
            output: crate::sink::OutputPrefs {
                bit_perfect: true,
                ..Default::default()
            },
            ..AudioOptions::default()
        }));
        assert!(
            e.st().info.processing.is_empty(),
            "{:?}",
            e.st().info.processing
        );
    }

    #[test]
    fn equalizer_turns_on_live_crosses_gapless_joins_and_steps_aside_for_bit_perfect() {
        let (mut e, clock) = hires_dac();
        let (a, b) = (tmp("eq-a.wav"), tmp("eq-b.wav"));
        write_wav(&a, 44100, 0.5, 0, 22050);
        write_wav(&b, 44100, 0.5, 22050, 44100);
        e.handle(Command::SetQueue {
            items: vec![item(&a), item(&b)],
            start: 0,
        });
        play(&mut e, &clock, 4410, 1000);
        assert!(e.st().info.processing.is_empty());
        let mut eq = crate::eq::EqParams::default();
        eq.gains[3] = 6.0;
        e.handle(Command::SetAudioOptions(AudioOptions {
            eq: Some(eq.clone()),
            ..AudioOptions::default()
        }));
        assert_eq!(
            e.st().info.processing,
            vec!["equalizer (10 bands, limiter)".to_string()]
        );
        // Through the gapless join into the second file.
        play(&mut e, &clock, 30_000, 1000);
        assert!(e
            .st()
            .info
            .processing
            .contains(&"equalizer (10 bands, limiter)".to_string()));
        assert_eq!(clock.opened().len(), 1, "still one stream (gapless)");
        // Bit-perfect leaves the samples alone.
        e.handle(Command::SetAudioOptions(AudioOptions {
            eq: Some(eq),
            output: crate::sink::OutputPrefs {
                bit_perfect: true,
                ..Default::default()
            },
            ..AudioOptions::default()
        }));
        assert!(e.st().info.processing.is_empty());
    }

    #[test]
    fn crossfeed_mixes_the_sides_live_and_steps_aside_for_bit_perfect() {
        let (mut e, clock) = hires_dac();
        // A left-only 440 Hz tone.
        let path = tmp("cf-left.wav");
        write_wav(&path, 44100, 1.0, 0, 44100);
        e.handle(Command::SetQueue {
            items: vec![item(&path)],
            start: 0,
        });
        play(&mut e, &clock, 2000, 1000);
        assert!(e.st().info.processing.is_empty());
        e.handle(Command::SetAudioOptions(AudioOptions {
            crossfeed: Some(crate::crossfeed::Strength::Medium),
            ..AudioOptions::default()
        }));
        assert_eq!(
            e.st().info.processing,
            vec!["crossfeed (medium)".to_string()]
        );
        // write_wav makes both channels equal (mono): crossfeed leaves mono alone.
        let before = clock.heard().len();
        play(&mut e, &clock, 6000, 1000);
        let heard = clock.heard();
        let (l, r): (Vec<f32>, Vec<f32>) = heard[before..].chunks(2).map(|f| (f[0], f[1])).unzip();
        assert!(!l.is_empty() && l.iter().zip(&r).all(|(a, b)| (a - b).abs() < 1e-6));
        e.handle(Command::SetAudioOptions(AudioOptions {
            crossfeed: Some(crate::crossfeed::Strength::Medium),
            output: crate::sink::OutputPrefs {
                bit_perfect: true,
                ..Default::default()
            },
            ..AudioOptions::default()
        }));
        assert!(e.st().info.processing.is_empty());
    }

    /// Heard samples of `ch` (0 left, 1 right) from the fake output.
    fn heard_ch(clock: &FakeClock, ch: usize) -> Vec<f32> {
        clock.heard().chunks(2).map(|f| f[ch]).collect()
    }

    fn rms32(v: &[f32]) -> f64 {
        (v.iter().map(|x| (*x as f64).powi(2)).sum::<f64>() / v.len() as f64).sqrt()
    }

    #[test]
    fn the_equalizer_really_changes_the_samples() {
        let (mut e, clock) = hires_dac();
        let path = tmp("eq-real.wav");
        write_wav(&path, 44100, 1.0, 0, 44100);
        let dry = {
            e.handle(Command::SetQueue {
                items: vec![item(&path)],
                start: 0,
            });
            play(&mut e, &clock, 44100 + 2000, 1000);
            heard_ch(&clock, 0)
        };
        let (mut e, clock) = hires_dac();
        // −6.02 dB of pre-amp, nothing else: every sample is exactly halved.
        let mut p = crate::eq::EqParams {
            preamp_db: -20.0 * 2f64.log10(),
            auto_headroom: false,
            limiter: false,
            ..Default::default()
        };
        e.handle(Command::SetAudioOptions(AudioOptions {
            eq: Some(p.clone()),
            ..AudioOptions::default()
        }));
        e.handle(Command::SetQueue {
            items: vec![item(&path)],
            start: 0,
        });
        play(&mut e, &clock, 44100 + 2000, 1000);
        let wet = heard_ch(&clock, 0);
        assert_eq!(wet.len(), dry.len());
        assert!(dry
            .iter()
            .zip(&wet)
            .all(|(d, w)| (d * 0.5 - w).abs() < 1e-6));
        // A +9 dB band at the tone's pitch: the measured gain matches the designed one.
        let (mut e, clock) = hires_dac();
        p.preamp_db = 0.0;
        p.gains = vec![0.0, 0.0, 0.0, 0.0, 9.0, 0.0, 0.0, 0.0, 0.0, 0.0];
        let designed = p.response_db(440.0, 44100.0);
        e.handle(Command::SetAudioOptions(AudioOptions {
            eq: Some(p),
            ..AudioOptions::default()
        }));
        e.handle(Command::SetQueue {
            items: vec![item(&path)],
            start: 0,
        });
        play(&mut e, &clock, 44100 + 2000, 1000);
        let wet = heard_ch(&clock, 0);
        let gain = 20.0 * (rms32(&wet[20_000..40_000]) / rms32(&dry[20_000..40_000])).log10();
        assert!(
            (gain - designed).abs() < 0.2,
            "measured {gain:.2} dB, designed {designed:.2}"
        );
        assert!(gain > 3.0, "the equalizer did nothing audible: {gain}");
    }

    #[test]
    fn the_limiter_costs_no_time_and_joins_tracks_gaplessly() {
        let (mut e, clock) = hires_dac();
        e.handle(Command::SetAudioOptions(AudioOptions {
            eq: Some(crate::eq::EqParams {
                preamp_db: 0.1, // not flat, so the stage (and its limiter) is in
                auto_headroom: false,
                ..Default::default()
            }),
            ..AudioOptions::default()
        }));
        let (a, b) = (tmp("lim-a.wav"), tmp("lim-b.wav"));
        write_wav(&a, 44100, 0.2, 0, 8820);
        write_wav(&b, 44100, 0.2, 8820, 17640);
        e.handle(Command::SetQueue {
            items: vec![item(&a), item(&b)],
            start: 0,
        });
        play(&mut e, &clock, 20_000, 1000);
        let left = heard_ch(&clock, 0);
        // The first sound arrives at once (no look-ahead silence)...
        assert!(left[..200].iter().any(|v| v.abs() > 0.01));
        // ...and the join is one continuous tone: no gap, no jump.
        let jump = left[8_000..10_000]
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0f32, f32::max);
        assert!(jump < 0.05, "discontinuity {jump}");
        // All but the last 5 ms of the queue plays.
        let sound = left.iter().rposition(|v| v.abs() > 1e-4).unwrap() + 1;
        assert!((17_640 - 300..=17_640).contains(&sound), "{sound}");
    }

    #[test]
    fn crossfeed_really_moves_sound_to_the_other_ear() {
        let (mut e, clock) = hires_dac();
        // A tone on the left only.
        let path = tmp("cf-real.wav");
        let frames = 44100u32;
        let mut b = Vec::new();
        b.extend_from_slice(b"RIFF");
        b.extend_from_slice(&(36 + frames * 4).to_le_bytes());
        b.extend_from_slice(b"WAVEfmt ");
        b.extend_from_slice(&16u32.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&2u16.to_le_bytes());
        b.extend_from_slice(&44100u32.to_le_bytes());
        b.extend_from_slice(&(44100u32 * 4).to_le_bytes());
        b.extend_from_slice(&4u16.to_le_bytes());
        b.extend_from_slice(&16u16.to_le_bytes());
        b.extend_from_slice(b"data");
        b.extend_from_slice(&(frames * 4).to_le_bytes());
        let tone: Vec<i16> = (0..frames)
            .map(|i| ((i as f32 * 150.0 * std::f32::consts::TAU / 44100.0).sin() * 8000.0) as i16)
            .collect();
        for v in &tone {
            b.extend_from_slice(&v.to_le_bytes());
            b.extend_from_slice(&0i16.to_le_bytes());
        }
        std::fs::write(&path, b).unwrap();
        e.handle(Command::SetAudioOptions(AudioOptions {
            crossfeed: Some(crate::crossfeed::Strength::Strong),
            ..AudioOptions::default()
        }));
        e.handle(Command::SetQueue {
            items: vec![item(&path)],
            start: 0,
        });
        play(&mut e, &clock, 44100 + 1000, 1000);
        let (l, r) = (heard_ch(&clock, 0), heard_ch(&clock, 1));
        // The same filter applied by hand gives the same samples.
        let mut want: Vec<f64> = tone
            .iter()
            .flat_map(|&v| [v as f64 / 32768.0, 0.0])
            .collect();
        crate::crossfeed::Crossfeed::new(crate::crossfeed::Strength::Strong, 44100)
            .process(&mut want);
        assert!(l
            .iter()
            .zip(want.chunks(2))
            .all(|(h, w)| (*h as f64 - w[0]).abs() < 1e-6));
        assert!(r
            .iter()
            .zip(want.chunks(2))
            .all(|(h, w)| (*h as f64 - w[1]).abs() < 1e-6));
        // And it is audible: the right ear now hears a good share of the left's bass.
        let share = rms32(&r[10_000..30_000]) / rms32(&l[10_000..30_000]);
        assert!(share > 0.3, "right/left {share}");
    }

    /// Frequency (Hz) of a clean tone from its zero crossings.
    fn freq_of(x: &[f32], rate: f64) -> f64 {
        let mut crossings = Vec::new();
        for i in 1..x.len() {
            if x[i - 1] < 0.0 && x[i] >= 0.0 {
                let frac = (-x[i - 1] / (x[i] - x[i - 1])) as f64;
                crossings.push(i as f64 - 1.0 + frac);
            }
        }
        let n = crossings.len() - 1;
        n as f64 * rate / (crossings[n] - crossings[0])
    }

    /// Plays a 2 s, 440 Hz file with `tempo`; returns the heard left channel and the engine.
    fn play_tempo(tempo: crate::Tempo, bit_perfect: bool) -> (Vec<f32>, Engine) {
        let (mut e, clock) = hires_dac();
        let path = tmp("tempo.wav");
        write_wav(&path, 44100, 2.0, 0, 88200);
        e.handle(Command::SetAudioOptions(AudioOptions {
            tempo,
            output: crate::sink::OutputPrefs {
                bit_perfect,
                ..Default::default()
            },
            ..AudioOptions::default()
        }));
        e.handle(Command::SetQueue {
            items: vec![item(&path)],
            start: 0,
        });
        play(&mut e, &clock, 200_000, 1024);
        let left = heard_ch(&clock, 0);
        let end = left.iter().rposition(|v| v.abs() > 1e-4).unwrap() + 1;
        (left[..end].to_vec(), e)
    }

    #[test]
    fn speed_and_pitch_are_accurate_to_half_a_percent() {
        let rate = 44100.0;
        let normal = crate::Tempo::default();
        let (base, _) = play_tempo(normal, false);
        assert!((base.len() as i64 - 88_200).abs() < 200);
        // (speed, keep pitch, semitones) → (length factor, frequency factor)
        let cases = [
            (1.5, true, 0.0, 1.0 / 1.5, 1.0),
            (0.75, true, 0.0, 1.0 / 0.75, 1.0),
            (1.0, true, 12.0, 1.0, 2.0),
            (1.0, true, -5.0, 1.0, 2f64.powf(-5.0 / 12.0)),
            (1.25, true, 7.0, 1.0 / 1.25, 2f64.powf(7.0 / 12.0)),
            (2.0, false, 0.0, 0.5, 2.0), // pitch follows speed
        ];
        for (speed, keep_pitch, semitones, length, pitch) in cases {
            let (heard, _) = play_tempo(
                crate::Tempo {
                    speed,
                    keep_pitch,
                    semitones,
                },
                false,
            );
            let got_len = heard.len() as f64 / base.len() as f64;
            assert!(
                (got_len / length - 1.0).abs() < 0.01,
                "speed {speed} pitch {semitones}: length ×{got_len:.3}, wanted ×{length:.3}"
            );
            let mid = &heard[heard.len() / 5..heard.len() * 4 / 5];
            let got_f = freq_of(mid, rate) / 440.0;
            assert!(
                (got_f / pitch - 1.0).abs() < 0.005,
                "speed {speed} pitch {semitones}: frequency ×{got_f:.4}, wanted ×{pitch:.4}"
            );
        }
    }

    #[test]
    fn speed_is_ignored_on_bit_perfect_and_position_counts_source_seconds() {
        let fast = crate::Tempo {
            speed: 2.0,
            ..Default::default()
        };
        let (heard, _) = play_tempo(fast, true);
        assert!((heard.len() as i64 - 88_200).abs() < 200, "{}", heard.len());
        // At double speed, one heard second is two seconds of the song.
        let (mut e, clock) = hires_dac();
        let path = tmp("tempo-pos.wav");
        write_wav(&path, 44100, 2.0, 0, 88200);
        e.handle(Command::SetAudioOptions(AudioOptions {
            tempo: fast,
            ..AudioOptions::default()
        }));
        e.handle(Command::SetQueue {
            items: vec![item(&path)],
            start: 0,
        });
        play(&mut e, &clock, 22_050, 1000);
        assert!((e.position() - 1.0).abs() < 0.05, "{}", e.position());
        // Changing the speed while playing carries on from the same place.
        let before = e.position();
        e.handle(Command::SetAudioOptions(AudioOptions {
            tempo: crate::Tempo::default(),
            ..AudioOptions::default()
        }));
        assert!(
            (e.position() - before).abs() < 0.05,
            "{} vs {before}",
            e.position()
        );
        play(&mut e, &clock, 22_050, 1000);
        assert!(
            (e.position() - (before + 0.5)).abs() < 0.06,
            "{}",
            e.position()
        );
    }

    /// A 16-bit stereo WAV (both channels the same) from these samples.
    fn write_wav_pcm(path: &Path, rate: u32, samples: &[i16]) {
        let data_len = samples.len() as u32 * 4;
        let mut b = Vec::new();
        b.extend_from_slice(b"RIFF");
        b.extend_from_slice(&(36 + data_len).to_le_bytes());
        b.extend_from_slice(b"WAVEfmt ");
        b.extend_from_slice(&16u32.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&2u16.to_le_bytes());
        b.extend_from_slice(&rate.to_le_bytes());
        b.extend_from_slice(&(rate * 4).to_le_bytes());
        b.extend_from_slice(&4u16.to_le_bytes());
        b.extend_from_slice(&16u16.to_le_bytes());
        b.extend_from_slice(b"data");
        b.extend_from_slice(&data_len.to_le_bytes());
        for v in samples {
            b.extend_from_slice(&v.to_le_bytes());
            b.extend_from_slice(&v.to_le_bytes());
        }
        std::fs::write(path, b).unwrap();
    }

    #[test]
    fn skip_silence_shortens_long_gaps_and_the_position_counts_the_skipped_time() {
        let rate = 44100usize;
        let tone = |n: usize| (0..n).map(|i| (((i as f32 * 0.05 + 1.0).sin()) * 8000.0) as i16);
        let quiet = |n: usize| (0..n).map(|i| ((i * 7 % 13) as i16 - 6) * 3); // about −70 dBFS
        let mut samples: Vec<i16> = tone(rate).collect();
        samples.extend(quiet(3 * rate));
        samples.extend(tone(rate));
        let path = tmp("skip.wav");
        write_wav_pcm(&path, rate as u32, &samples);

        let play_all = |skip: bool| {
            let (mut e, clock) = hires_dac();
            e.handle(Command::SetAudioOptions(AudioOptions {
                skip_silence: skip,
                ..AudioOptions::default()
            }));
            e.handle(Command::SetQueue {
                items: vec![item(&path)],
                start: 0,
            });
            play(&mut e, &clock, 5 * rate + 2000, 1024);
            let l = heard_ch(&clock, 0);
            l.iter().rposition(|v| v.abs() > 1e-4).unwrap() + 1
        };
        let normal = play_all(false);
        let skipped = play_all(true);
        // 3 s of silence cut to 0.25 s: 2.75 s shorter.
        let cut = (normal - skipped) as f64 / rate as f64;
        assert!((cut - 2.75).abs() < 0.05, "cut {cut} s");

        // Part-way through the second tone, the position is where the song really is.
        let (mut e, clock) = hires_dac();
        e.handle(Command::SetAudioOptions(AudioOptions {
            skip_silence: true,
            ..AudioOptions::default()
        }));
        e.handle(Command::SetQueue {
            items: vec![item(&path)],
            start: 0,
        });
        // Heard: 1 s tone + 0.25 s kept gap + 0.4 s of the second tone.
        play(&mut e, &clock, (1.65 * rate as f64) as usize, 1024);
        let heard_secs = 1.65;
        let real = 1.0 + 3.0 + 0.4; // seconds into the file
        assert!(
            (e.position() - real).abs() < 0.1,
            "position {} (heard {heard_secs} s), file time {real} s",
            e.position()
        );
    }

    #[test]
    fn opus_seeks() {
        let (mut format, _, track_id, _, _) = open_file(&fixture("tone.opus")).unwrap();
        let seeked = format.seek(
            SeekMode::Accurate,
            SeekTo::Time {
                time: Time::from(0.5),
                track_id: Some(track_id),
            },
        );
        assert!(seeked.is_ok(), "{seeked:?}");
    }

    #[test]
    fn gapless_join_is_sample_exact() {
        // One continuous sine split into two files at an arbitrary sample, so a
        // gap or overlap at the join shows up as a jump in the waveform.
        let dir = std::env::temp_dir().join(format!("loss-gapless-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (a, b) = (dir.join("a.wav"), dir.join("b.wav"));
        let total = 44100;
        let split = 30_011;
        write_wav(&a, 44100, 0.0, 0, split);
        write_wav(&b, 44100, 0.0, split, total);
        let (out, joins) = decode_chain(&[a, b], 44100, 2);
        assert_eq!(joins, vec![split as u64 * 2]);
        assert_eq!(out.len(), total as usize * 2, "no samples lost or added");
        let left: Vec<f32> = out.iter().step_by(2).copied().collect();
        let max_step = left
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0, f32::max);
        // 440 Hz at 8000/32768 amplitude moves at most ~0.016 per sample.
        assert!(max_step < 0.02, "discontinuity {max_step}");
        std::fs::remove_dir_all(dir).ok();
    }

    // ---- Engine against the fake sink (M2-01) ----

    use crate::sink::{FakeClock, FakeSink};

    fn engine_with(rates: &[u32], latency: u64) -> (Engine, FakeClock) {
        let (sink, clock) = FakeSink::new(
            Caps {
                rates: rates.to_vec(),
                any_rate: false,
                channels: 2,
            },
            latency,
        );
        let state = Arc::new(Mutex::new(PlayerState {
            volume: 1.0,
            ..Default::default()
        }));
        (Engine::new(state, Box::new(sink)), clock)
    }

    fn tmp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("loss-engine-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(name)
    }

    fn item(path: &Path) -> QueueItem {
        QueueItem {
            id: path.file_name().unwrap().to_string_lossy().into_owned(),
            path: path.to_path_buf(),
            ..Default::default()
        }
    }

    /// Plays `frames` frames in `step`-frame bursts, ticking the engine like its thread does.
    fn play(e: &mut Engine, clock: &FakeClock, frames: usize, step: usize) {
        let mut done = 0;
        while done < frames {
            e.tick();
            let n = step.min(frames - done);
            clock.advance(n);
            done += n;
        }
        e.tick();
    }

    #[test]
    fn positions_are_sample_exact_and_count_latency() {
        let path = tmp("pos.wav");
        write_wav(&path, 48000, 2.0, 0, 96000);
        let (mut e, clock) = engine_with(&[48000], 480);
        e.handle(Command::SetQueue {
            items: vec![item(&path)],
            start: 0,
        });
        assert_eq!(clock.opened(), vec![48000]);
        assert!(clock.is_playing());
        // Nothing heard until the output's latency has passed.
        play(&mut e, &clock, 480, 480);
        assert_eq!(e.position(), 0.0);
        for heard in [4800usize, 24000, 48000] {
            let so_far = e.heard_samples() as usize / 2;
            play(&mut e, &clock, heard - so_far, 480);
            assert!(
                (e.position() - heard as f64 / 48000.0).abs() < 1e-12,
                "{}",
                e.position()
            );
        }
        // Paused: the clock stops and so does the position.
        e.handle(Command::Pause);
        let at = e.position();
        play(&mut e, &clock, 9600, 480);
        assert_eq!(e.position(), at);
        e.handle(Command::Play);
        play(&mut e, &clock, 4800, 480);
        assert!((e.position() - (at + 0.1)).abs() < 1e-12);
    }

    #[test]
    fn untouched_path_is_bit_exact() {
        let path = tmp("exact.wav");
        write_wav(&path, 48000, 0.5, 0, 24000);
        let (mut e, clock) = engine_with(&[48000], 0);
        e.handle(Command::SetQueue {
            items: vec![item(&path)],
            start: 0,
        });
        assert!(e.st().info.untouched());
        play(&mut e, &clock, 24000, 1000);
        let heard = clock.heard();
        // The same 16-bit samples the file holds, exactly.
        for i in 0..24000usize {
            let v = ((i as f32 * 440.0 * std::f32::consts::TAU / 48000.0).sin() * 8000.0) as i16;
            assert_eq!((heard[i * 2] * 32768.0) as i16, v, "frame {i}");
        }
    }

    #[test]
    fn twenty_four_bit_samples_survive_f32_exactly() {
        // The ring and output are f32: every 24-bit value must come back unchanged.
        for v in [-(1 << 23), -1234567, -1, 0, 1, 7654321, (1 << 23) - 1] {
            let f = (v as f64 / (1u32 << 23) as f64) as f32;
            assert_eq!((f as f64 * (1u32 << 23) as f64) as i32, v);
        }
    }

    #[test]
    fn converts_to_what_the_output_takes() {
        // Beats Studio Pro: 48 kHz only. A 44.1 kHz file is resampled by Loss.
        let path = tmp("cd.wav");
        write_wav(&path, 44100, 1.0, 0, 44100);
        let (mut e, clock) = engine_with(&[32000, 48000], 0);
        e.handle(Command::SetQueue {
            items: vec![item(&path)],
            start: 0,
        });
        assert_eq!(clock.opened(), vec![48000]);
        let info = e.st().info.clone();
        assert_eq!(info.output_rate, 48000);
        assert_eq!(
            info.processing,
            vec!["resample 44100→48000 Hz (best)".to_string()]
        );
        assert!(!info.untouched());
        play(&mut e, &clock, 24000, 1000);
        assert!((e.position() - 0.5).abs() < 1e-9);
    }

    #[test]
    fn plugging_in_the_beats_reopens_at_48k_where_the_song_was() {
        // The output itself says "anything" (Android's mixer); then the platform reports the
        // Beats (32/48 kHz) as the active output.
        let path = tmp("plug.wav");
        write_wav(&path, 44100, 1.0, 0, 44100);
        let (sink, clock) = FakeSink::new(
            Caps {
                rates: vec![],
                any_rate: true,
                channels: 2,
            },
            0,
        );
        let state = Arc::new(Mutex::new(PlayerState {
            volume: 1.0,
            ..Default::default()
        }));
        let mut e = Engine::new(state, Box::new(sink));
        e.handle(Command::SetQueue {
            items: vec![item(&path)],
            start: 0,
        });
        assert_eq!(clock.opened(), vec![44100]);
        play(&mut e, &clock, 22050, 1000);
        e.handle(Command::SetOutputCaps(Some(Caps {
            rates: vec![32000, 48000],
            any_rate: false,
            channels: 2,
        })));
        assert_eq!(clock.opened(), vec![44100, 48000]);
        assert!((e.position() - 0.5).abs() < 0.01, "{}", e.position());
        assert_eq!(e.st().status, Status::Playing);
        assert_eq!(
            e.st().info.processing,
            vec!["resample 44100→48000 Hz (best)".to_string()]
        );
    }

    fn beats_engine() -> (Engine, FakeClock) {
        let (sink, clock) = FakeSink::new(
            Caps {
                rates: vec![32000, 48000],
                any_rate: false,
                channels: 2,
            },
            0,
        );
        let state = Arc::new(Mutex::new(PlayerState {
            volume: 1.0,
            ..Default::default()
        }));
        (Engine::new(state, Box::new(sink)), clock)
    }

    #[test]
    fn a_16_bit_output_dithers_only_when_the_samples_changed() {
        let (mut e, clock) = beats_engine();
        let exact = tmp("dither48.wav");
        write_wav(&exact, 48000, 0.2, 0, 9600);
        let cd = tmp("dither44.wav");
        write_wav(&cd, 44100, 0.2, 0, 8820);
        let i16_out = crate::sink::OutputPrefs {
            format: Some(crate::sink::SampleFormat::I16),
            ..Default::default()
        };
        e.handle(Command::SetAudioOptions(AudioOptions {
            output: i16_out,
            ..AudioOptions::default()
        }));
        assert_eq!(clock.prefs(), i16_out);
        e.handle(Command::SetQueue {
            items: vec![item(&exact), item(&cd)],
            start: 0,
        });
        // 16-bit at 48 kHz reaches a 16-bit output untouched: bit-exact, no dither.
        assert!(e.st().info.output.starts_with("I16 · shared"));
        assert!(!clock.dithering());
        // The CD-rate file is resampled, so its samples are dithered down to 16 bits.
        e.handle(Command::Next);
        assert!(clock.dithering());
    }

    #[test]
    fn changing_the_output_format_or_losing_the_device_reopens_where_we_were() {
        let (mut e, clock) = beats_engine();
        let path = tmp("reopen.wav");
        write_wav(&path, 48000, 1.0, 0, 48000);
        e.handle(Command::SetQueue {
            items: vec![item(&path)],
            start: 0,
        });
        play(&mut e, &clock, 12000, 1000);
        assert_eq!(e.st().info.output, "F32 · shared · power saving · 48000 Hz");
        e.handle(Command::SetAudioOptions(AudioOptions {
            output: crate::sink::OutputPrefs {
                format: Some(crate::sink::SampleFormat::I24),
                exclusive: true,
                low_latency: true,
                ..Default::default()
            },
            ..AudioOptions::default()
        }));
        assert_eq!(clock.opened(), vec![48000, 48000]);
        assert_eq!(
            e.st().info.output,
            "I24 · exclusive · low latency · 48000 Hz"
        );
        assert!((e.position() - 0.25).abs() < 0.01, "{}", e.position());
        assert_eq!(e.st().status, Status::Playing);
        // Unplugged mid-song: the stream reports it lost and the engine opens a new one.
        clock.lose_device();
        e.tick();
        assert_eq!(clock.opened().len(), 3);
        assert!((e.position() - 0.25).abs() < 0.01, "{}", e.position());
    }

    fn hires_dac() -> (Engine, FakeClock) {
        let (sink, clock) = FakeSink::new(
            Caps {
                rates: vec![44100, 48000, 96000],
                any_rate: false,
                channels: 2,
            },
            0,
        );
        let state = Arc::new(Mutex::new(PlayerState {
            volume: 1.0,
            ..Default::default()
        }));
        (Engine::new(state, Box::new(sink)), clock)
    }

    #[test]
    fn standard_mode_converts_everything_to_the_mixer_rate() {
        let (mut e, clock) = hires_dac();
        e.handle(Command::SetAudioOptions(AudioOptions {
            output: crate::sink::OutputPrefs {
                fixed_rate: 48000,
                ..Default::default()
            },
            ..AudioOptions::default()
        }));
        let path = tmp("standard96.wav");
        write_wav(&path, 96000, 0.1, 0, 9600);
        e.handle(Command::SetQueue {
            items: vec![item(&path)],
            start: 0,
        });
        assert_eq!(clock.opened(), vec![48000]);
        assert_eq!(
            e.st().info.processing,
            vec!["resample 96000→48000 Hz (best)".to_string()]
        );
    }

    #[test]
    fn bit_perfect_keeps_rate_and_depth_and_reopens_for_a_new_depth() {
        let (mut e, clock) = hires_dac();
        e.handle(Command::SetAudioOptions(AudioOptions {
            // The limit doesn't apply: there is no conversion to limit.
            max_rate: 48000,
            output: crate::sink::OutputPrefs {
                bit_perfect: true,
                fixed_rate: 48000,
                ..Default::default()
            },
            ..AudioOptions::default()
        }));
        let cd = tmp("bp-cd.wav");
        write_wav(&cd, 44100, 0.1, 0, 4410);
        let hd = tmp("bp-hd.wav");
        write_wav24(&hd, 96000, 9600);
        e.handle(Command::SetQueue {
            items: vec![item(&cd), item(&hd)],
            start: 0,
        });
        assert_eq!(clock.opened(), vec![44100]);
        let info = e.st().info.clone();
        assert!(info.untouched(), "{:?}", info.processing);
        assert!(
            info.output.starts_with("I16 · bit-perfect"),
            "{}",
            info.output
        );
        assert!(!clock.dithering());
        e.handle(Command::Next);
        assert_eq!(clock.opened(), vec![44100, 96000]);
        assert!(e.st().info.output.starts_with("I24 · bit-perfect"));
        assert!(e.st().info.untouched());
    }

    #[test]
    fn hi_res_reaches_the_beats_at_48k_and_respects_the_limit() {
        // 96 kHz → 48 kHz (integer ratio), the whole file comes out at exactly its length.
        let path = tmp("hires.wav");
        write_wav(&path, 96000, 0.5, 0, 48000);
        let (mut e, clock) = engine_with(&[32000, 48000], 0);
        e.handle(Command::SetQueue {
            items: vec![item(&path)],
            start: 0,
        });
        assert_eq!(clock.opened(), vec![48000]);
        play(&mut e, &clock, 30000, 1000);
        // 0.5 s at 48 kHz, to the frame, then the output stops.
        let heard: usize = clock.heard().len() / 2;
        assert_eq!(heard, 24000);
        // The owner's limit: a DAC that takes 192 kHz, capped at 96 kHz.
        let (mut e, clock) = engine_with(&[44100, 48000, 96000, 192000], 0);
        e.handle(Command::SetAudioOptions(AudioOptions {
            quality: Quality::High,
            max_rate: 96000,
            ..AudioOptions::default()
        }));
        let path = tmp("hires192.wav");
        write_wav(&path, 192000, 0.1, 0, 19200);
        e.handle(Command::SetQueue {
            items: vec![item(&path)],
            start: 0,
        });
        assert_eq!(clock.opened(), vec![96000]);
        assert_eq!(
            e.st().info.processing,
            vec!["resample 192000→96000 Hz (high)".to_string()]
        );
    }

    #[test]
    fn gapless_across_source_rates_when_the_output_is_the_same() {
        // 44.1 kHz then 48 kHz, both played at 48 kHz: no reopening, the switch lands exactly.
        let a = tmp("a441.wav");
        let b = tmp("b48.wav");
        write_wav(&a, 44100, 0.5, 0, 22050);
        write_wav(&b, 48000, 0.5, 0, 24000);
        let (mut e, clock) = engine_with(&[48000], 240);
        e.handle(Command::SetQueue {
            items: vec![item(&a), item(&b)],
            start: 0,
        });
        play(&mut e, &clock, 20000, 500);
        assert_eq!(e.st().index, Some(0));
        play(&mut e, &clock, 8000, 500);
        assert_eq!(e.st().index, Some(1), "switched to the second track");
        assert_eq!(clock.opened(), vec![48000], "the output never reopened");
        let pos = e.position();
        assert!(pos > 0.0 && pos < 0.2, "{pos}");
    }
}
