//! The player thread: owns the decoder, the output stream and the queue.

use crate::resample::{map_channels, Resampler};
use crate::{Command, PlayerState, QueueItem, Repeat, Status, StreamInfo};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::fs::File;
use std::path::Path;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
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

type Slot = Arc<Mutex<Option<rtrb::Consumer<f32>>>>;

/// An open output stream. Reused across tracks with the same format.
struct Output {
    stream: cpal::Stream,
    slot: Slot,
    played_samples: Arc<AtomicU64>,
    rate: u32,
    channels: usize,
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
    volume: Arc<AtomicU32>,
) -> Result<cpal::Stream, String>
where
    T: cpal::SizedSample + cpal::FromSample<f32>,
{
    device
        .build_output_stream(
            config,
            move |data: &mut [T], _| {
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
                                *s = T::from_sample(v * vol);
                            }
                        }
                        None => data.iter_mut().for_each(|s| *s = T::from_sample(0.0)),
                    },
                    Err(_) => data.iter_mut().for_each(|s| *s = T::from_sample(0.0)),
                }
                played.fetch_add(popped, Ordering::Relaxed);
            },
            |e| log::error!("audio stream error: {e}"),
            None,
        )
        .map_err(|e| e.to_string())
}

impl Output {
    fn open(want_rate: u32, volume: Arc<AtomicU32>) -> Result<Output, String> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or("no audio output device")?;
        let supported = pick_config(&device, want_rate)?;
        let format = supported.sample_format();
        let config: cpal::StreamConfig = supported.into();
        let slot: Slot = Arc::new(Mutex::new(None));
        let played = Arc::new(AtomicU64::new(0));
        let (s, p, v) = (slot.clone(), played.clone(), volume);
        let stream = match format {
            cpal::SampleFormat::F32 => build_stream::<f32>(&device, &config, s, p, v)?,
            cpal::SampleFormat::I16 => build_stream::<i16>(&device, &config, s, p, v)?,
            cpal::SampleFormat::I32 => build_stream::<i32>(&device, &config, s, p, v)?,
            cpal::SampleFormat::U16 => build_stream::<u16>(&device, &config, s, p, v)?,
            other => return Err(format!("unsupported output format {other:?}")),
        };
        Ok(Output {
            stream,
            slot,
            played_samples: played,
            rate: config.sample_rate.0,
            channels: config.channels as usize,
        })
    }
}

/// One playing track.
struct Session {
    item: QueueItem,
    format: Box<dyn FormatReader>,
    decoder: Box<dyn Decoder>,
    track_id: u32,
    src_rate: u32,
    src_channels: usize,
    resampler: Resampler,
    producer: rtrb::Producer<f32>,
    capacity: usize,
    pending: Vec<f32>,
    pending_pos: usize,
    eof: bool,
    start_offset: f64,
    duration: f64,
    counted: bool,
    info: StreamInfo,
}

/// An opened file: reader, decoder, track id, stream info, duration in seconds.
type OpenedFile = (Box<dyn FormatReader>, Box<dyn Decoder>, u32, StreamInfo, f64);

fn open_file(path: &Path) -> Result<OpenedFile, String> {
    let file = File::open(path).map_err(|e| format!("cannot open file: {e}"))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let probed = symphonia::default::get_probe()
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
    let decoder = symphonia::default::get_codecs()
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
    let codec = symphonia::default::get_codecs()
        .get_codec(params.codec)
        .map(|d| d.short_name.to_string())
        .unwrap_or_else(|| "audio".into());
    let info = StreamInfo {
        codec,
        sample_rate: rate,
        bits_per_sample: params.bits_per_sample,
        channels: params.channels.map(|c| c.count() as u16).unwrap_or(2),
        output_rate: 0,
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
                    self.eof = true;
                    continue;
                }
                Err(SymError::ResetRequired) => {
                    self.eof = true;
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
                self.resampler = Resampler::new(self.src_rate, out_rate, out_channels);
            }
            let mut buf = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
            buf.copy_interleaved_ref(decoded);
            let mut mapped = Vec::with_capacity(buf.samples().len());
            map_channels(buf.samples(), self.src_channels, out_channels, &mut mapped);
            self.resampler.process(&mapped, &mut self.pending);
        }
    }

    fn drained(&self) -> bool {
        self.eof && self.pending_pos >= self.pending.len() && self.producer.slots() == self.capacity
    }
}

struct Engine {
    state: Arc<Mutex<PlayerState>>,
    output: Option<Output>,
    session: Option<Session>,
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
                if let Some(out) = &self.output {
                    if autoplay {
                        let _ = out.stream.play();
                    } else {
                        let _ = out.stream.pause();
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

    fn start_session(&mut self, item: QueueItem, at: f64) -> Result<(), String> {
        let (mut format, decoder, track_id, mut info, duration) = open_file(&item.path)?;
        let mut start_offset = 0.0;
        if at > 0.0 {
            match format.seek(
                SeekMode::Accurate,
                SeekTo::Time {
                    time: Time::from(at),
                    track_id: Some(track_id),
                },
            ) {
                Ok(_) => start_offset = at,
                Err(e) => log::warn!("seek failed: {e}"),
            }
        }
        let need_new = match &self.output {
            Some(o) => o.rate != info.sample_rate,
            None => true,
        };
        if need_new {
            self.output = None; // close the old stream first
            self.output = Some(Output::open(info.sample_rate, self.volume.clone())?);
        }
        let out = self.output.as_ref().unwrap();
        info.output_rate = out.rate;
        let capacity = (out.rate as usize * out.channels) / 2; // ~0.5 s
        let (producer, consumer) = rtrb::RingBuffer::new(capacity);
        *out.slot.lock().unwrap_or_else(|p| p.into_inner()) = Some(consumer);
        out.played_samples.store(0, Ordering::Relaxed);
        let src_channels = info.channels.max(1) as usize;
        let mut session = Session {
            item,
            format,
            decoder,
            track_id,
            src_rate: info.sample_rate,
            src_channels,
            resampler: Resampler::new(info.sample_rate, out.rate, out.channels),
            producer,
            capacity,
            pending: Vec::new(),
            pending_pos: 0,
            eof: false,
            start_offset,
            duration,
            counted: false,
            info,
        };
        session.pump(out.channels, out.rate)?;
        self.session = Some(session);
        Ok(())
    }

    fn stop(&mut self) {
        self.session = None;
        if let Some(o) = &self.output {
            let _ = o.stream.pause();
            *o.slot.lock().unwrap_or_else(|p| p.into_inner()) = None;
        }
        let mut s = self.st();
        s.status = Status::Stopped;
        s.position = 0.0;
    }

    fn position(&self) -> f64 {
        match (&self.session, &self.output) {
            (Some(s), Some(o)) => {
                s.start_offset
                    + o.played_samples.load(Ordering::Relaxed) as f64
                        / (o.rate as f64 * o.channels as f64)
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
            let _ = o.stream.pause();
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
                    let _ = o.stream.play();
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
        let Some(out) = &self.output else { return };
        let (ch, rate) = (out.channels, out.rate);
        let pos = self.position();
        let mut finished = false;
        let state = self.state.clone();
        if let Some(s) = &mut self.session {
            if let Err(e) = s.pump(ch, rate) {
                log::error!("decode error: {e}");
                s.eof = true;
            }
            let threshold = if s.duration > 0.0 {
                (s.duration * 0.5).min(30.0)
            } else {
                30.0
            };
            if !s.counted && pos >= threshold {
                s.counted = true;
                let id = s.item.id.clone();
                state
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .play_events
                    .push(id);
            }
            finished = s.drained();
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
}

pub fn run(rx: Receiver<Command>, state: Arc<Mutex<PlayerState>>) {
    let vol = state.lock().map(|s| s.volume).unwrap_or(1.0);
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x9E3779B97F4A7C15)
        | 1;
    let mut engine = Engine {
        state,
        output: None,
        session: None,
        volume: Arc::new(AtomicU32::new(vol.to_bits())),
        unshuffled: None,
        rng: seed,
    };
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

#[cfg(test)]
mod tests {
    use super::*;

    fn write_wav(path: &Path, rate: u32, secs: f32) {
        let frames = (rate as f32 * secs) as u32;
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
        for i in 0..frames {
            let v = ((i as f32 * 440.0 * std::f32::consts::TAU / rate as f32).sin() * 8000.0) as i16;
            b.extend_from_slice(&v.to_le_bytes());
            b.extend_from_slice(&v.to_le_bytes());
        }
        std::fs::write(path, b).unwrap();
    }

    #[test]
    fn decodes_wav_fully_with_resampling() {
        let dir = std::env::temp_dir().join(format!("loss-audio-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("tone.wav");
        write_wav(&path, 44100, 1.0);
        let (format, decoder, track_id, info, duration) = open_file(&path).unwrap();
        assert_eq!(info.sample_rate, 44100);
        assert_eq!(info.bits_per_sample, Some(16));
        assert!((duration - 1.0).abs() < 0.01, "{duration}");
        let capacity = 48000 * 2 * 2;
        let (producer, mut consumer) = rtrb::RingBuffer::new(capacity);
        let mut s = Session {
            item: QueueItem {
                id: "x".into(),
                path: path.clone(),
            },
            format,
            decoder,
            track_id,
            src_rate: 44100,
            src_channels: 2,
            resampler: Resampler::new(44100, 48000, 2),
            producer,
            capacity,
            pending: vec![],
            pending_pos: 0,
            eof: false,
            start_offset: 0.0,
            duration,
            counted: false,
            info,
        };
        let mut total = 0usize;
        for _ in 0..1000 {
            s.pump(2, 48000).unwrap();
            while consumer.pop().is_ok() {
                total += 1;
            }
            if s.drained() {
                break;
            }
        }
        assert!(s.drained());
        let frames = total / 2;
        assert!((frames as i64 - 48000).abs() < 50, "{frames}");
        std::fs::remove_dir_all(dir).ok();
    }
}
