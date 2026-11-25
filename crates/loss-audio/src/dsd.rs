//! DSD playback (M2-08): DSF and DSDIFF (DFF) readers and a DSD→PCM decoder, for Symphonia.
//!
//! The readers hand out packets of raw DSD in one layout: bytes interleaved by channel, oldest
//! bit first (MSB). The decoder low-pass filters the 1-bit stream with a linear-phase FIR
//! (Kaiser window, flat to 20 kHz, ≥ 100 dB down at the new Nyquist) and keeps every 32nd/64th/…
//! sample, giving 88.2 kHz PCM (96 kHz for 48 kHz-family DSD); the engine converts from there.
//! The filter runs a byte at a time from lookup tables (each table holds the contribution of
//! one byte's 8 bits at one position). Unity gain: SACD's 0 dB (50 % modulation) is −6 dBFS.
//! DST-compressed DFF is not supported (it fails with an error).

use std::io::{Read, Seek, SeekFrom};
use std::sync::Arc;
use symphonia::core::audio::{
    AsAudioBufferRef, AudioBuffer, AudioBufferRef, Channels, Signal, SignalSpec,
};
use symphonia::core::codecs::{
    decl_codec_type, CodecDescriptor, CodecParameters, CodecType, Decoder, DecoderOptions,
    FinalizeResult,
};
use symphonia::core::errors::{Error, Result};
use symphonia::core::formats::{
    Cue, FormatOptions, FormatReader, Packet, SeekMode, SeekTo, SeekedTo, Track,
};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::{Metadata, MetadataLog};
use symphonia::core::probe::{Descriptor, Instantiate, QueryDescriptor};
use symphonia::core::units::TimeBase;
use symphonia::core::{support_codec, support_format};

pub const CODEC_TYPE_DSD: CodecType = decl_codec_type(b"dsd");

/// Bytes per channel in one packet (DSF's block size; 1024 PCM frames at DSD64).
const PACKET_BYTES: usize = 4096;

/// The PCM rate a DSD rate decodes to and the decimation factor (DSD samples per PCM frame).
pub fn pcm_rate(dsd_rate: u32) -> Option<(u32, u32)> {
    for base in [88_200u32, 96_000] {
        if dsd_rate.is_multiple_of(base) {
            let factor = dsd_rate / base;
            // At least DSD64 (factor 32), a power of two.
            if factor >= 32 && factor.is_power_of_two() {
                return Some((base, factor));
            }
        }
    }
    None
}

fn layout(channels: usize) -> Result<Channels> {
    Ok(match channels {
        1 => Channels::FRONT_LEFT,
        2 => Channels::FRONT_LEFT | Channels::FRONT_RIGHT,
        3 => Channels::FRONT_LEFT | Channels::FRONT_RIGHT | Channels::FRONT_CENTRE,
        4 => {
            Channels::FRONT_LEFT
                | Channels::FRONT_RIGHT
                | Channels::REAR_LEFT
                | Channels::REAR_RIGHT
        }
        5 => {
            Channels::FRONT_LEFT
                | Channels::FRONT_RIGHT
                | Channels::FRONT_CENTRE
                | Channels::REAR_LEFT
                | Channels::REAR_RIGHT
        }
        6 => {
            Channels::FRONT_LEFT
                | Channels::FRONT_RIGHT
                | Channels::FRONT_CENTRE
                | Channels::LFE1
                | Channels::REAR_LEFT
                | Channels::REAR_RIGHT
        }
        _ => return Err(Error::Unsupported("dsd: unsupported channel count")),
    })
}

// ---- reader ----

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    /// DSF: blocks of `block` bytes per channel, one channel after another.
    Dsf { block: usize, lsb_first: bool },
    /// DFF: already byte-interleaved, MSB first.
    Dff,
}

pub struct DsdReader {
    source: MediaSourceStream,
    tracks: Vec<Track>,
    cues: Vec<Cue>,
    meta: MetadataLog,
    kind: Kind,
    channels: usize,
    data_start: u64,
    /// Bytes of DSD per channel (without DSF's block padding).
    bytes_per_channel: u64,
    /// Bytes per channel read so far.
    pos: u64,
    /// DSD bytes per channel per PCM frame.
    bytes_per_frame: u64,
}

fn read_exact(src: &mut MediaSourceStream, n: usize) -> Result<Vec<u8>> {
    let mut v = vec![0u8; n];
    src.read_exact(&mut v)?;
    Ok(v)
}

fn le32(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

fn le64(b: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(b[at..at + 8].try_into().unwrap_or([0; 8]))
}

fn be64(b: &[u8]) -> u64 {
    u64::from_be_bytes(b[..8].try_into().unwrap_or([0; 8]))
}

/// (kind, channels, dsd rate, data start, bytes per channel)
type Found = (Kind, usize, u32, u64, u64);

fn read_dsf(src: &mut MediaSourceStream) -> Result<Found> {
    // "DSD " chunk: id, size (28), file size, metadata pointer.
    let head = read_exact(src, 28)?;
    if &head[0..4] != b"DSD " {
        return Err(Error::DecodeError("dsf: not a DSF file"));
    }
    let fmt = read_exact(src, 52)?;
    if &fmt[0..4] != b"fmt " {
        return Err(Error::DecodeError("dsf: no fmt chunk"));
    }
    if le32(&fmt, 16) != 0 {
        return Err(Error::Unsupported("dsf: compressed DSD"));
    }
    let channels = le32(&fmt, 24) as usize;
    let rate = le32(&fmt, 28);
    let bits = le32(&fmt, 32);
    let samples = le64(&fmt, 36);
    let block = le32(&fmt, 44) as usize;
    if channels == 0 || block == 0 || !(bits == 1 || bits == 8) {
        return Err(Error::DecodeError("dsf: bad fmt chunk"));
    }
    // Skip to the data chunk (fmt's size may exceed 52).
    let fmt_size = le64(&fmt, 4);
    let data_at = 28 + fmt_size;
    src.seek(SeekFrom::Start(data_at))?;
    let data = read_exact(src, 12)?;
    if &data[0..4] != b"data" {
        return Err(Error::DecodeError("dsf: no data chunk"));
    }
    Ok((
        Kind::Dsf {
            block,
            lsb_first: bits == 1,
        },
        channels,
        rate,
        data_at + 12,
        samples / 8,
    ))
}

fn read_dff(src: &mut MediaSourceStream) -> Result<Found> {
    let head = read_exact(src, 16)?;
    if &head[0..4] != b"FRM8" || &head[12..16] != b"DSD " {
        return Err(Error::DecodeError("dff: not a DSDIFF file"));
    }
    let end = 12 + be64(&head[4..12]);
    let mut at = 16u64;
    let (mut rate, mut channels) = (0u32, 0usize);
    while at + 12 <= end {
        src.seek(SeekFrom::Start(at))?;
        let ck = read_exact(src, 12)?;
        let size = be64(&ck[4..12]);
        let body = at + 12;
        match &ck[0..4] {
            b"PROP" => {
                // "SND " then sub-chunks.
                let mut sub = body + 4;
                while sub + 12 <= body + size {
                    src.seek(SeekFrom::Start(sub))?;
                    let sc = read_exact(src, 12)?;
                    let ssize = be64(&sc[4..12]);
                    match &sc[0..4] {
                        b"FS  " => {
                            let v = read_exact(src, 4)?;
                            rate = u32::from_be_bytes([v[0], v[1], v[2], v[3]]);
                        }
                        b"CHNL" => {
                            let v = read_exact(src, 2)?;
                            channels = u16::from_be_bytes([v[0], v[1]]) as usize;
                        }
                        b"CMPR" => {
                            let v = read_exact(src, 4)?;
                            if &v != b"DSD " {
                                return Err(Error::Unsupported("dff: DST-compressed DSD"));
                            }
                        }
                        _ => {}
                    }
                    sub += 12 + ssize + (ssize & 1);
                }
            }
            b"DSD " => {
                if rate == 0 || channels == 0 {
                    return Err(Error::DecodeError("dff: data before properties"));
                }
                return Ok((Kind::Dff, channels, rate, body, size / channels as u64));
            }
            b"DST " => return Err(Error::Unsupported("dff: DST-compressed DSD")),
            _ => {}
        }
        at = body + size + (size & 1);
    }
    Err(Error::DecodeError("dff: no DSD data"))
}

impl DsdReader {
    fn open(mut source: MediaSourceStream) -> Result<DsdReader> {
        let mut magic = [0u8; 4];
        source.read_exact(&mut magic)?;
        source.seek(SeekFrom::Start(0))?;
        let (kind, channels, rate, data_start, bytes_per_channel) = match &magic {
            b"DSD " => read_dsf(&mut source)?,
            b"FRM8" => read_dff(&mut source)?,
            _ => return Err(Error::DecodeError("dsd: unknown container")),
        };
        let (pcm, factor) =
            pcm_rate(rate).ok_or(Error::Unsupported("dsd: unsupported DSD rate"))?;
        let bytes_per_frame = factor as u64 / 8;
        let mut params = CodecParameters::new();
        params
            .for_codec(CODEC_TYPE_DSD)
            .with_sample_rate(pcm)
            .with_time_base(TimeBase::new(1, pcm))
            .with_n_frames(bytes_per_channel / bytes_per_frame)
            .with_channels(layout(channels)?)
            .with_extra_data(Box::new([factor as u8, (factor >> 8) as u8]));
        source.seek(SeekFrom::Start(data_start))?;
        Ok(DsdReader {
            source,
            tracks: vec![Track::new(0, params)],
            cues: vec![],
            meta: MetadataLog::default(),
            kind,
            channels,
            data_start,
            bytes_per_channel,
            pos: 0,
            bytes_per_frame,
        })
    }

    /// Bytes per channel per packet.
    fn chunk(&self) -> u64 {
        match self.kind {
            Kind::Dsf { block, .. } => block as u64,
            Kind::Dff => PACKET_BYTES as u64,
        }
    }
}

impl QueryDescriptor for DsdReader {
    fn query() -> &'static [Descriptor] {
        &[
            support_format!(
                "dsf",
                "DSD Stream File",
                &["dsf"],
                &["audio/dsf"],
                &[b"DSD "]
            ),
            support_format!(
                "dff",
                "DSD Interchange File Format",
                &["dff"],
                &["audio/dff"],
                &[b"FRM8"]
            ),
        ]
    }

    fn score(_context: &[u8]) -> u8 {
        255
    }
}

impl FormatReader for DsdReader {
    fn try_new(source: MediaSourceStream, _options: &FormatOptions) -> Result<Self> {
        DsdReader::open(source)
    }

    fn cues(&self) -> &[Cue] {
        &self.cues
    }

    fn metadata(&mut self) -> Metadata<'_> {
        self.meta.metadata()
    }

    fn seek(&mut self, _mode: SeekMode, to: SeekTo) -> Result<SeekedTo> {
        let rate = self.tracks[0].codec_params.sample_rate.unwrap_or(88_200) as u64;
        let required = match to {
            SeekTo::Time { time, .. } => (time.seconds * rate) + (time.frac * rate as f64) as u64,
            SeekTo::TimeStamp { ts, .. } => ts,
        };
        // Packets start on whole chunks.
        let byte = (required * self.bytes_per_frame).min(self.bytes_per_channel);
        let chunk = self.chunk();
        self.pos = byte / chunk * chunk;
        let file_at = self.data_start + self.pos * self.channels as u64;
        self.source.seek(SeekFrom::Start(file_at))?;
        Ok(SeekedTo {
            track_id: 0,
            required_ts: required,
            actual_ts: self.pos / self.bytes_per_frame,
        })
    }

    fn tracks(&self) -> &[Track] {
        &self.tracks
    }

    fn next_packet(&mut self) -> Result<Packet> {
        if self.pos >= self.bytes_per_channel {
            return Err(Error::IoError(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "end of stream",
            )));
        }
        let chunk = self.chunk() as usize;
        let ch = self.channels;
        let raw = read_exact(&mut self.source, chunk * ch)?;
        // The music in this chunk (the last DSF block is padded).
        let valid = ((self.bytes_per_channel - self.pos) as usize).min(chunk);
        let valid = valid / self.bytes_per_frame as usize * self.bytes_per_frame as usize;
        let mut out = vec![0u8; valid * ch];
        match self.kind {
            Kind::Dsf { lsb_first, .. } => {
                for c in 0..ch {
                    let block = &raw[c * chunk..c * chunk + valid];
                    for (i, &b) in block.iter().enumerate() {
                        out[i * ch + c] = if lsb_first { b.reverse_bits() } else { b };
                    }
                }
            }
            Kind::Dff => out.copy_from_slice(&raw[..valid * ch]),
        }
        let ts = self.pos / self.bytes_per_frame;
        let dur = valid as u64 / self.bytes_per_frame;
        self.pos += chunk as u64;
        Ok(Packet::new_from_boxed_slice(
            0,
            ts,
            dur,
            out.into_boxed_slice(),
        ))
    }

    fn into_inner(self: Box<Self>) -> MediaSourceStream {
        self.source
    }
}

// ---- decoder ----

/// The decimation filter as byte tables: `tables[k][b]` is the sum over the 8 bits of byte `b`
/// sitting `k` bytes back of tap × (±1).
fn design(factor: u32) -> Vec<[f32; 256]> {
    // Taps: passband to 20 kHz, stopband from the PCM Nyquist (44.1/48 kHz), ~100 dB.
    let fs = 88_200.0 * factor as f64; // relative units are what matter
    let pcm = fs / factor as f64;
    let (pass, stop) = (20_000.0 / 88_200.0 * pcm, pcm / 2.0);
    let cutoff = (pass + stop) / 2.0 / fs; // cycles per DSD sample
    let width = (stop - pass) / fs;
    let atten = 100.0;
    let beta = 0.1102 * (atten - 8.7);
    let mut n = ((atten - 8.0) / (2.285 * 2.0 * std::f64::consts::PI * width)).ceil() as usize;
    n = n.div_ceil(8) * 8; // whole bytes
    let i0 = |x: f64| {
        let (mut sum, mut term, mut k) = (1.0, 1.0, 1.0);
        while term > 1e-12 * sum {
            term *= (x / 2.0) * (x / 2.0) / (k * k);
            sum += term;
            k += 1.0;
        }
        sum
    };
    let mid = (n as f64 - 1.0) / 2.0;
    let mut h: Vec<f64> = (0..n)
        .map(|i| {
            let t = i as f64 - mid;
            let sinc = if t == 0.0 {
                2.0 * cutoff
            } else {
                (2.0 * std::f64::consts::PI * cutoff * t).sin() / (std::f64::consts::PI * t)
            };
            let r = t / (mid + 0.5);
            sinc * i0(beta * (1.0 - r * r).max(0.0).sqrt()) / i0(beta)
        })
        .collect();
    let dc: f64 = h.iter().sum();
    h.iter_mut().for_each(|v| *v /= dc);
    (0..n / 8)
        .map(|k| {
            let mut t = [0f32; 256];
            for (b, slot) in t.iter_mut().enumerate() {
                // Bit 7 (MSB) is the oldest sample of the byte, bit 0 the newest; tap index
                // 8k + j weights the sample j places back from this byte's newest one.
                *slot = (0..8)
                    .map(|j| h[8 * k + j] * if (b >> j) & 1 == 1 { 1.0 } else { -1.0 })
                    .sum::<f64>() as f32;
            }
            t
        })
        .collect()
}

pub struct DsdDecoder {
    params: CodecParameters,
    channels: usize,
    /// Bytes per channel per PCM frame.
    step: usize,
    tables: Arc<Vec<[f32; 256]>>,
    /// Per channel: the last `tables.len()` bytes, newest last (a ring).
    history: Vec<Vec<u8>>,
    head: usize,
    buf: AudioBuffer<f32>,
}

impl Decoder for DsdDecoder {
    fn try_new(params: &CodecParameters, _options: &DecoderOptions) -> Result<Self> {
        let extra = params.extra_data.as_deref().unwrap_or(&[32, 0]);
        let factor = extra[0] as u32 | (*extra.get(1).unwrap_or(&0) as u32) << 8;
        let channels = params.channels.map(|c| c.count()).unwrap_or(2);
        let rate = params.sample_rate.unwrap_or(88_200);
        let tables = Arc::new(design(factor));
        let taps = tables.len();
        Ok(DsdDecoder {
            params: params.clone(),
            channels,
            step: factor as usize / 8,
            tables,
            history: vec![vec![0x69; taps]; channels], // 0x69: DSD silence pattern
            head: 0,
            buf: AudioBuffer::new(
                (PACKET_BYTES / (factor as usize / 8)).max(1) as u64 * 4,
                SignalSpec::new(rate, layout(channels)?),
            ),
        })
    }

    fn supported_codecs() -> &'static [CodecDescriptor] {
        &[support_codec!(
            CODEC_TYPE_DSD,
            "dsd",
            "Direct Stream Digital"
        )]
    }

    fn reset(&mut self) {
        for h in &mut self.history {
            h.fill(0x69);
        }
        self.head = 0;
    }

    fn codec_params(&self) -> &CodecParameters {
        &self.params
    }

    fn decode(&mut self, packet: &Packet) -> Result<AudioBufferRef<'_>> {
        let ch = self.channels;
        let bytes = packet.data.len() / ch;
        let frames = bytes / self.step;
        self.buf.clear();
        self.buf.render_reserved(Some(frames));
        let taps = self.tables.len();
        let tables = self.tables.clone();
        for f in 0..frames {
            for c in 0..ch {
                let hist = &mut self.history[c];
                // Feed this frame's bytes, then filter at the newest one.
                let mut head = self.head;
                for s in 0..self.step {
                    head = (head + 1) % taps;
                    hist[head] = packet.data[(f * self.step + s) * ch + c];
                }
                let mut acc = 0f32;
                let mut at = head;
                for t in tables.iter() {
                    acc += t[hist[at] as usize];
                    at = if at == 0 { taps - 1 } else { at - 1 };
                }
                self.buf.chan_mut(c)[f] = acc;
                if c == ch - 1 {
                    self.head = head;
                }
            }
        }
        Ok(self.buf.as_audio_buffer_ref())
    }

    fn finalize(&mut self) -> FinalizeResult {
        FinalizeResult::default()
    }

    fn last_decoded(&self) -> AudioBufferRef<'_> {
        self.buf.as_audio_buffer_ref()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A 1-bit sigma-delta modulator (2nd order) for test signals.
    pub(crate) fn modulate(signal: impl Iterator<Item = f64>) -> Vec<u8> {
        let (mut i1, mut i2, mut out) = (0.0f64, 0.0f64, 0.0f64);
        let mut bits = Vec::new();
        for x in signal {
            i1 += x - out;
            i2 += i1 - out;
            out = if i2 >= 0.0 { 1.0 } else { -1.0 };
            bits.push(out > 0.0);
        }
        bits.chunks(8)
            .map(|c| c.iter().fold(0u8, |b, &bit| (b << 1) | bit as u8))
            .collect()
    }

    pub(crate) fn dsf(rate: u32, bytes: &[u8]) -> Vec<u8> {
        // Stereo DSF, both channels the same, MSB first (bits per sample 8), block 4096.
        let block = 4096usize;
        let blocks = bytes.len().div_ceil(block);
        let mut data = Vec::new();
        for b in 0..blocks {
            for _ in 0..2 {
                let mut chunk = bytes[b * block..((b + 1) * block).min(bytes.len())].to_vec();
                chunk.resize(block, 0x69);
                data.extend(chunk);
            }
        }
        let mut f = Vec::new();
        f.extend(b"DSD ");
        f.extend(28u64.to_le_bytes());
        f.extend((28 + 52 + 12 + data.len() as u64).to_le_bytes());
        f.extend(0u64.to_le_bytes());
        f.extend(b"fmt ");
        f.extend(52u64.to_le_bytes());
        f.extend(1u32.to_le_bytes()); // version
        f.extend(0u32.to_le_bytes()); // DSD raw
        f.extend(2u32.to_le_bytes()); // stereo
        f.extend(2u32.to_le_bytes()); // channels
        f.extend(rate.to_le_bytes());
        f.extend(8u32.to_le_bytes()); // MSB first
        f.extend((bytes.len() as u64 * 8).to_le_bytes());
        f.extend((block as u32).to_le_bytes());
        f.extend(0u32.to_le_bytes());
        f.extend(b"data");
        f.extend((12 + data.len() as u64).to_le_bytes());
        f.extend(data);
        f
    }

    #[test]
    fn rates() {
        assert_eq!(pcm_rate(2_822_400), Some((88_200, 32)));
        assert_eq!(pcm_rate(5_644_800), Some((88_200, 64)));
        assert_eq!(pcm_rate(3_072_000), Some((96_000, 32)));
        assert_eq!(pcm_rate(44_100), None);
    }

    #[test]
    fn decodes_a_dsd64_tone() {
        let rate = 2_822_400;
        let secs = 0.5;
        let n = (rate as f64 * secs) as usize;
        let tone =
            (0..n).map(|i| 0.5 * (i as f64 * 1000.0 * std::f64::consts::TAU / rate as f64).sin());
        let file = dsf(rate, &modulate(tone));
        let path = std::env::temp_dir().join(format!("loss-dsd-{}.dsf", std::process::id()));
        std::fs::write(&path, file).unwrap();
        let mss = MediaSourceStream::new(
            Box::new(std::fs::File::open(&path).unwrap()),
            Default::default(),
        );
        let mut reader = DsdReader::try_new(mss, &FormatOptions::default()).unwrap();
        let params = reader.tracks()[0].codec_params.clone();
        assert_eq!(params.sample_rate, Some(88_200));
        assert_eq!(params.n_frames, Some(44_100));
        let mut dec = DsdDecoder::try_new(&params, &DecoderOptions::default()).unwrap();
        let mut left = Vec::new();
        let started = std::time::Instant::now();
        while let Ok(p) = reader.next_packet() {
            let out = dec.decode(&p).unwrap();
            if let AudioBufferRef::F32(b) = out {
                left.extend_from_slice(b.chan(0));
            }
        }
        eprintln!("decoded 0.5 s of DSD64 stereo in {:?}", started.elapsed());
        assert_eq!(left.len(), 44_100);
        // After the filter settles: a clean 1 kHz sine at half scale.
        let s = &left[4410..left.len() - 100];
        let rms = (s.iter().map(|v| (*v as f64).powi(2)).sum::<f64>() / s.len() as f64).sqrt();
        assert!((rms - 0.5 / 2f64.sqrt()).abs() < 0.01, "rms {rms}");
        // Compare with the ideal sine (same phase, the filter is linear-phase with known delay).
        let peak = s.iter().fold(0f32, |m, v| m.max(v.abs()));
        assert!((peak - 0.5).abs() < 0.02, "peak {peak}");
        // Noise and distortion: subtract the best-fitting 1 kHz sine and measure what's left.
        let w = 1000.0 * std::f64::consts::TAU / 88_200.0;
        let (mut a, mut b) = (0.0, 0.0);
        for (i, v) in s.iter().enumerate() {
            a += *v as f64 * (w * i as f64).sin();
            b += *v as f64 * (w * i as f64).cos();
        }
        let (a, b) = (2.0 * a / s.len() as f64, 2.0 * b / s.len() as f64);
        let resid = (s
            .iter()
            .enumerate()
            .map(|(i, v)| (*v as f64 - a * (w * i as f64).sin() - b * (w * i as f64).cos()).powi(2))
            .sum::<f64>()
            / s.len() as f64)
            .sqrt();
        let snr = 20.0 * (rms / resid).log10();
        eprintln!("DSD64 → 88.2 kHz: signal/(noise+distortion) {snr:.1} dB");
        // The test modulator is only 2nd order (real SACD modulators are 5th–7th); its in-band
        // noise is what's left, not the filter's.
        assert!(snr > 60.0, "{snr} dB");
        // Seeking lands on a chunk boundary at or before the time asked.
        let seeked = reader
            .seek(
                SeekMode::Accurate,
                SeekTo::TimeStamp {
                    ts: 30_000,
                    track_id: 0,
                },
            )
            .unwrap();
        assert!(seeked.actual_ts <= 30_000 && seeked.actual_ts % 1024 == 0);
        let _ = std::fs::remove_file(path);
    }
}
