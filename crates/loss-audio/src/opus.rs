//! Opus decoding for Symphonia, using the pure-Rust `opus-decoder` crate.
//!
//! Symphonia's Ogg and Matroska/WebM readers already recognise Opus streams; this supplies the
//! missing decoder. It also removes the encoder's pre-skip at the start of a stream: Symphonia
//! 0.5's Ogg reader puts it in `CodecParameters::delay` but doesn't trim it, which left a 6.5 ms
//! gap at every gapless join (found by `generated_gapless_sets`).

use std::sync::OnceLock;
use symphonia::core::audio::{
    AsAudioBufferRef, AudioBuffer, AudioBufferRef, Channels, Signal, SignalSpec,
};
use symphonia::core::codecs::{
    CodecDescriptor, CodecParameters, CodecRegistry, Decoder, DecoderOptions, FinalizeResult,
    CODEC_TYPE_OPUS,
};
use symphonia::core::errors::{Error, Result};
use symphonia::core::formats::Packet;
use symphonia::core::support_codec;

/// Opus always decodes at 48 kHz.
const RATE: u32 = 48_000;
/// Longest Opus packet: 120 ms at 48 kHz.
const MAX_FRAMES: usize = 5_760;

pub struct OpusDecoder {
    params: CodecParameters,
    decoder: opus_decoder::OpusDecoder,
    channels: usize,
    pcm: Vec<f32>,
    buf: AudioBuffer<f32>,
    /// Encoder pre-skip in samples (48 kHz), from the stream header.
    pre_skip: usize,
    /// Pre-skip samples still to drop; set at the start of the stream.
    to_skip: usize,
}

impl Decoder for OpusDecoder {
    fn try_new(params: &CodecParameters, _options: &DecoderOptions) -> Result<Self> {
        let channels = params.channels.map(|c| c.count()).unwrap_or(2);
        let layout = match channels {
            1 => Channels::FRONT_LEFT,
            2 => Channels::FRONT_LEFT | Channels::FRONT_RIGHT,
            _ => {
                return Err(Error::Unsupported(
                    "opus: surround files are not supported yet",
                ))
            }
        };
        let decoder = opus_decoder::OpusDecoder::new(RATE, channels)
            .map_err(|_| Error::Unsupported("opus: cannot create decoder"))?;
        Ok(OpusDecoder {
            params: params.clone(),
            decoder,
            channels,
            pcm: vec![0.0; MAX_FRAMES * channels],
            buf: AudioBuffer::new(MAX_FRAMES as u64, SignalSpec::new(RATE, layout)),
            pre_skip: params.delay.unwrap_or(0) as usize,
            to_skip: params.delay.unwrap_or(0) as usize,
        })
    }

    fn supported_codecs() -> &'static [CodecDescriptor] {
        &[support_codec!(CODEC_TYPE_OPUS, "opus", "Opus")]
    }

    fn reset(&mut self) {
        // After a seek: the pre-skip only applies again if playback restarts at the beginning
        // (see `decode`).
        self.decoder.reset();
        self.to_skip = 0;
    }

    fn codec_params(&self) -> &CodecParameters {
        &self.params
    }

    fn decode(&mut self, packet: &Packet) -> Result<AudioBufferRef<'_>> {
        let frames = self
            .decoder
            .decode_float(&packet.data, &mut self.pcm, false)
            .map_err(|_| Error::DecodeError("opus: invalid packet"))?;
        self.buf.clear();
        self.buf.render_reserved(Some(frames));
        for ch in 0..self.channels {
            let plane = self.buf.chan_mut(ch);
            for (i, s) in plane.iter_mut().enumerate().take(frames) {
                *s = self.pcm[i * self.channels + ch];
            }
        }
        if packet.ts() == 0 && self.to_skip == 0 && self.pre_skip > 0 && packet.trim_start() == 0 {
            // Back at the start (first packet, or a seek to 0).
            self.to_skip = self.pre_skip;
        }
        // The container's trimming (if it did any) counts towards the pre-skip.
        let container = packet.trim_start() as usize;
        self.to_skip = self.to_skip.saturating_sub(container);
        let extra = self.to_skip.min(frames.saturating_sub(container));
        self.to_skip -= extra;
        // Pre-skip at the start and encoder padding at the end (gapless).
        self.buf.trim(container + extra, packet.trim_end() as usize);
        Ok(self.buf.as_audio_buffer_ref())
    }

    fn finalize(&mut self) -> FinalizeResult {
        FinalizeResult::default()
    }

    fn last_decoded(&self) -> AudioBufferRef<'_> {
        self.buf.as_audio_buffer_ref()
    }
}

/// Symphonia's built-in codecs plus Opus.
pub fn codecs() -> &'static CodecRegistry {
    static REGISTRY: OnceLock<CodecRegistry> = OnceLock::new();
    REGISTRY.get_or_init(|| {
        let mut registry = CodecRegistry::new();
        symphonia::default::register_enabled_codecs(&mut registry);
        registry.register_all::<OpusDecoder>();
        registry
    })
}
