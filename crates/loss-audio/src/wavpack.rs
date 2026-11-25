//! WavPack (M2-08), lossless mono and stereo, through the pure-Rust `wavicle` crate (MIT/Apache;
//! a port of the BSD reference decoder, checked bit-exact against ffmpeg here). WavPack blocks
//! are independent, so the reader hands out one block per packet and the decoder decodes each on
//! its own. Hybrid/lossy (`.wvc`), DSD-in-WavPack and more than two channels fail with an error.

use std::io::{Read, Seek, SeekFrom};
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

pub const CODEC_TYPE_WAVPACK_LOSS: CodecType = decl_codec_type(b"wvpk");

/// One block: where it is and which samples it holds.
#[derive(Clone, Copy)]
struct BlockAt {
    offset: u64,
    size: u32,
    first: u64,
    samples: u32,
}

pub struct WvReader {
    source: MediaSourceStream,
    tracks: Vec<Track>,
    cues: Vec<Cue>,
    meta: MetadataLog,
    blocks: Vec<BlockAt>,
    next: usize,
}

fn le32(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

impl WvReader {
    fn open(mut source: MediaSourceStream) -> Result<WvReader> {
        // Index every block header (32 bytes each, skipping the audio).
        let mut blocks = Vec::new();
        let mut at = 0u64;
        let mut head = [0u8; 32];
        loop {
            source.seek(SeekFrom::Start(at))?;
            if source.read_exact(&mut head).is_err() || &head[0..4] != b"wvpk" {
                break; // end of file, or a trailing APE tag
            }
            let size = le32(&head, 4) + 8;
            let first = le32(&head, 16) as u64 | (head[10] as u64) << 32;
            let samples = le32(&head, 20);
            if samples > 0 {
                blocks.push(BlockAt {
                    offset: at,
                    size,
                    first,
                    samples,
                });
            }
            at += size as u64;
            if blocks.len() > 4_000_000 {
                break;
            }
        }
        let first = *blocks
            .first()
            .ok_or(Error::DecodeError("wavpack: no audio blocks"))?;
        // The first block tells the format.
        source.seek(SeekFrom::Start(first.offset))?;
        let mut bytes = vec![0u8; first.size as usize];
        source.read_exact(&mut bytes)?;
        let d = wavicle::decode_stream(&bytes).map_err(|_| {
            Error::Unsupported("wavpack: this kind of WavPack file isn't supported")
        })?;
        let total: u64 = blocks.iter().map(|b| b.samples as u64).sum();
        let channels = match d.channels {
            1 => Channels::FRONT_LEFT,
            2 => Channels::FRONT_LEFT | Channels::FRONT_RIGHT,
            _ => return Err(Error::Unsupported("wavpack: more than two channels")),
        };
        let mut params = CodecParameters::new();
        params
            .for_codec(CODEC_TYPE_WAVPACK_LOSS)
            .with_sample_rate(d.sample_rate)
            .with_time_base(TimeBase::new(1, d.sample_rate))
            .with_n_frames(total)
            .with_channels(channels)
            .with_extra_data(Box::new([d.bits_per_sample as u8, d.is_float as u8]));
        if !d.is_float {
            params.with_bits_per_sample(d.bits_per_sample);
        }
        source.seek(SeekFrom::Start(first.offset))?;
        Ok(WvReader {
            source,
            tracks: vec![Track::new(0, params)],
            cues: vec![],
            meta: MetadataLog::default(),
            blocks,
            next: 0,
        })
    }
}

impl QueryDescriptor for WvReader {
    fn query() -> &'static [Descriptor] {
        &[support_format!(
            "wavpack",
            "WavPack",
            &["wv"],
            &["audio/wavpack", "audio/x-wavpack"],
            &[b"wvpk"]
        )]
    }

    fn score(_context: &[u8]) -> u8 {
        255
    }
}

impl FormatReader for WvReader {
    fn try_new(source: MediaSourceStream, _options: &FormatOptions) -> Result<Self> {
        WvReader::open(source)
    }

    fn cues(&self) -> &[Cue] {
        &self.cues
    }

    fn metadata(&mut self) -> Metadata<'_> {
        self.meta.metadata()
    }

    fn seek(&mut self, _mode: SeekMode, to: SeekTo) -> Result<SeekedTo> {
        let rate = self.tracks[0].codec_params.sample_rate.unwrap_or(44_100) as u64;
        let required = match to {
            SeekTo::Time { time, .. } => time.seconds * rate + (time.frac * rate as f64) as u64,
            SeekTo::TimeStamp { ts, .. } => ts,
        };
        // The block holding the sample (blocks start where the previous one ended).
        let i = self
            .blocks
            .partition_point(|b| b.first + b.samples as u64 <= required)
            .min(self.blocks.len().saturating_sub(1));
        self.next = i;
        Ok(SeekedTo {
            track_id: 0,
            required_ts: required,
            actual_ts: self.blocks[i].first,
        })
    }

    fn tracks(&self) -> &[Track] {
        &self.tracks
    }

    fn next_packet(&mut self) -> Result<Packet> {
        let Some(b) = self.blocks.get(self.next).copied() else {
            return Err(Error::IoError(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "end of stream",
            )));
        };
        self.next += 1;
        self.source.seek(SeekFrom::Start(b.offset))?;
        let mut bytes = vec![0u8; b.size as usize];
        self.source.read_exact(&mut bytes)?;
        Ok(Packet::new_from_boxed_slice(
            0,
            b.first,
            b.samples as u64,
            bytes.into_boxed_slice(),
        ))
    }

    fn into_inner(self: Box<Self>) -> MediaSourceStream {
        self.source
    }
}

pub struct WvDecoder {
    params: CodecParameters,
    bits: u32,
    float: bool,
    buf: AudioBuffer<f32>,
}

impl Decoder for WvDecoder {
    fn try_new(params: &CodecParameters, _options: &DecoderOptions) -> Result<Self> {
        let extra = params.extra_data.as_deref().unwrap_or(&[16, 0]);
        let channels = params
            .channels
            .unwrap_or(Channels::FRONT_LEFT | Channels::FRONT_RIGHT);
        Ok(WvDecoder {
            params: params.clone(),
            bits: extra[0].max(8) as u32,
            float: extra.get(1) == Some(&1),
            buf: AudioBuffer::new(
                1 << 16,
                SignalSpec::new(params.sample_rate.unwrap_or(44_100), channels),
            ),
        })
    }

    fn supported_codecs() -> &'static [CodecDescriptor] {
        &[support_codec!(
            CODEC_TYPE_WAVPACK_LOSS,
            "wavpack",
            "WavPack"
        )]
    }

    fn reset(&mut self) {}

    fn codec_params(&self) -> &CodecParameters {
        &self.params
    }

    fn decode(&mut self, packet: &Packet) -> Result<AudioBufferRef<'_>> {
        let d = wavicle::decode_stream(&packet.data)
            .map_err(|_| Error::DecodeError("wavpack: invalid block"))?;
        let ch = d.channels.max(1) as usize;
        let frames = d.samples.len() / ch;
        if frames > self.buf.capacity() {
            let spec = *self.buf.spec();
            self.buf = AudioBuffer::new(frames as u64, spec);
        }
        self.buf.clear();
        self.buf.render_reserved(Some(frames));
        let scale = 1.0 / (1u64 << (self.bits - 1)) as f32;
        for c in 0..ch.min(self.buf.spec().channels.count()) {
            let plane = self.buf.chan_mut(c);
            for (i, s) in plane.iter_mut().enumerate().take(frames) {
                let v = d.samples[i * ch + c];
                *s = if self.float {
                    f32::from_bits(v as u32)
                } else {
                    v as f32 * scale
                };
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
