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
}

/// The trim for a file and how its packet timestamps convert to frames.
fn gapless_trim(
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
    item_id: String,
    /// `played_samples` value at which this track started.
    base: u64,
    start_offset: f64,
    duration: f64,
    counted: bool,
}

/// A gapless join waiting for playback to reach it.
struct Switch {
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
                self.graph = Graph::new(
                    self.src_rate,
                    self.src_channels,
                    out_rate,
                    out_channels,
                    self.quality,
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
                    self.graph.process(samples, &mut processed);
                    self.pending.extend(processed.iter().map(|&v| v as f32));
                    self.finish();
                    continue;
                }
            }
            let mut processed = Vec::with_capacity(samples.len());
            self.graph.process(samples, &mut processed);
            // The ring and the output are f32: exact for every 16- and 24-bit sample.
            self.pending.extend(processed.iter().map(|&v| v as f32));
        }
    }

    /// End of the file: the graph hands over what it held back (resampler tail).
    fn finish(&mut self) {
        let mut tail = Vec::new();
        self.graph.flush(&mut tail);
        self.pending.extend(tail.iter().map(|&v| v as f32));
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
    fn handover(
        prev: Session,
        item: QueueItem,
        opened: OpenedFile,
        out_rate: u32,
        out_channels: usize,
        quality: Quality,
    ) -> Session {
        let (format, decoder, track_id, mut info, duration) = opened;
        let (trim, ts_to_frames) =
            gapless_trim(&item.path, format.as_ref(), track_id, info.sample_rate);
        info.output_rate = out_rate;
        info.output = prev.info.output.clone();
        let src_channels = info.channels.max(1) as usize;
        let graph = Graph::new(
            info.sample_rate,
            src_channels,
            out_rate,
            out_channels,
            quality,
        );
        info.processing = graph.describe();
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
        let (trim, ts_to_frames) =
            gapless_trim(&item.path, format.as_ref(), track_id, info.sample_rate);
        let mut start_offset = 0.0;
        if at > 0.0 {
            // Positions count from the music, after any encoder priming.
            let priming = trim.delay as f64 / info.sample_rate.max(1) as f64;
            match format.seek(
                SeekMode::Accurate,
                SeekTo::Time {
                    time: Time::from(at + priming),
                    track_id: Some(track_id),
                },
            ) {
                Ok(_) => start_offset = at,
                Err(e) => log::warn!("seek failed: {e}"),
            }
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
        let graph = Graph::new(
            info.sample_rate,
            src_channels,
            out.rate,
            out.channels,
            quality,
        );
        info.processing = graph.describe();
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
                d.start_offset + heard as f64 / (o.rate as f64 * o.channels as f64)
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
                self.options = o;
                if reopen {
                    self.sink.set_prefs(o.output);
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
                let next = Session::handover(prev, item, opened, out_rate, out_channels, quality);
                self.switch = Some(Switch {
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
        };
        let (format, decoder, track_id, info, duration) = open_file(&paths[0]).unwrap();
        let (trim, ts_to_frames) =
            gapless_trim(&paths[0], format.as_ref(), track_id, info.sample_rate);
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
