//! Opus decoding for Symphonia, using the pure-Rust `opus-decoder` crate.
//!
//! Symphonia's Ogg and Matroska/WebM readers already recognise Opus streams
//! (and pass the encoder pre-skip as gapless trimming); this supplies the
//! missing decoder.

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
        })
    }

    fn supported_codecs() -> &'static [CodecDescriptor] {
        &[support_codec!(CODEC_TYPE_OPUS, "opus", "Opus")]
    }

    fn reset(&mut self) {
        self.decoder.reset();
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
        // Encoder pre-skip at the start and padding at the end (gapless).
        self.buf
            .trim(packet.trim_start() as usize, packet.trim_end() as usize);
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
