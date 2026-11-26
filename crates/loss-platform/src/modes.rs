//! Output modes (M2-04): which ones this phone and DAC allow, and why not. The glue reports the
//! mixer facts as JSON (`LossBridge.mixerInfo`); the decisions are here.

use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Loss converts everything to the system mixer's rate (usually 48 kHz), so Android never
    /// resamples again.
    Standard,
    /// The output opens at the song's own rate when the device takes it.
    HiRes,
    /// Android 14+ hands the stream to a USB DAC untouched (when the phone offers it).
    BitPerfect,
    /// An exclusive (MMAP) stream, for players and phones that support it.
    Exclusive,
    /// Loss drives the USB DAC itself (M2-05).
    DirectUsb,
}

impl Mode {
    pub const ALL: [Mode; 5] = [
        Mode::Standard,
        Mode::HiRes,
        Mode::BitPerfect,
        Mode::Exclusive,
        Mode::DirectUsb,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Mode::Standard => "standard",
            Mode::HiRes => "hires",
            Mode::BitPerfect => "bitperfect",
            Mode::Exclusive => "exclusive",
            Mode::DirectUsb => "usb",
        }
    }

    pub fn parse(s: &str) -> Mode {
        Mode::ALL
            .into_iter()
            .find(|m| m.key() == s)
            .unwrap_or(Mode::HiRes)
    }
}

/// Why a mode can't be used right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unavailable {
    /// Needs Android 14.
    OldAndroid,
    /// Needs a USB DAC plugged in.
    NoUsbDac,
    /// The phone doesn't offer it for this DAC.
    NotOffered,
    /// Not built yet.
    NotYet,
    /// Only on Android.
    NotHere,
}

impl Unavailable {
    /// The translation id of the explanation.
    pub fn message_id(self) -> &'static str {
        match self {
            Unavailable::OldAndroid => "mode-unavailable-old-android",
            Unavailable::NoUsbDac => "mode-unavailable-no-usb",
            Unavailable::NotOffered => "mode-unavailable-not-offered",
            Unavailable::NotYet => "mode-unavailable-not-yet",
            Unavailable::NotHere => "mode-unavailable-not-here",
        }
    }
}

/// One format the phone plays bit-perfect on the DAC.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub struct BitPerfectFormat {
    pub rate: u32,
    /// Android `AudioFormat.ENCODING_*`.
    pub encoding: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct UsbMixer {
    pub id: i32,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub bit_perfect: Vec<BitPerfectFormat>,
}

/// The system Spatializer (Android 12+), as the glue reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
pub struct Spatializer {
    /// `Spatializer.getImmersiveAudioLevel()`: 0 none, 1 multichannel, 2 with objects.
    #[serde(default)]
    pub level: i32,
    /// Turned on in the system's sound settings.
    #[serde(default)]
    pub enabled: bool,
    /// Usable with the current output.
    #[serde(default)]
    pub available: bool,
    #[serde(default)]
    pub head_tracking: bool,
    /// The current media route can be spatialized (stereo PCM).
    #[serde(default)]
    pub output: bool,
}

/// Why spatial audio is or isn't applied to the music, for the Audio card.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpatialState {
    /// The phone has no spatializer (or Android is older than 12).
    Unsupported,
    /// Loss' setting is off: the stream tells Android never to spatialize.
    Off,
    /// The output mode (exclusive, bit-perfect, Direct USB) keeps the music untouched.
    Blocked,
    /// Switched off in the phone's sound settings.
    SystemOff,
    /// This output (speaker, a DAC without support…) can't be spatialized.
    NotThisOutput,
    /// Android may spatialize the music; `head_tracking` when the headphones track the head.
    On { head_tracking: bool },
}

/// Modes in which nothing may process the music, spatializer included.
pub fn mode_allows_spatial(mode: Mode) -> bool {
    matches!(mode, Mode::Standard | Mode::HiRes)
}

/// What mixer state and settings add up to.
pub fn spatial_state(sp: Option<&Spatializer>, mode: Mode, setting_on: bool) -> SpatialState {
    let Some(sp) = sp.filter(|s| s.level > 0) else {
        return SpatialState::Unsupported;
    };
    if !setting_on {
        return SpatialState::Off;
    }
    if !mode_allows_spatial(mode) {
        return SpatialState::Blocked;
    }
    if !sp.enabled {
        return SpatialState::SystemOff;
    }
    if !sp.available || !sp.output {
        return SpatialState::NotThisOutput;
    }
    SpatialState::On {
        head_tracking: sp.head_tracking,
    }
}

/// What the phone's mixer offers.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Mixer {
    pub sdk: i32,
    /// The mixer's own rate (`PROPERTY_OUTPUT_SAMPLE_RATE`), 0 if unknown.
    #[serde(default)]
    pub mixer_rate: u32,
    #[serde(default)]
    pub usb: Option<UsbMixer>,
    #[serde(default)]
    pub spatial: Option<Spatializer>,
    /// `AudioManager.isVolumeFixed()`: the phone's volume doesn't change the output.
    #[serde(default)]
    pub volume_fixed: bool,
}

/// Android `AudioFormat` encodings for integer and float PCM.
pub const ENCODING_PCM_16BIT: i32 = 2;
pub const ENCODING_PCM_FLOAT: i32 = 4;
pub const ENCODING_PCM_24BIT_PACKED: i32 = 21;
pub const ENCODING_PCM_32BIT: i32 = 22;

impl Mixer {
    pub fn parse(json: &str) -> Option<Mixer> {
        serde_json::from_str(json).ok()
    }

    /// The mixer rate, or 48 kHz when the phone doesn't say.
    pub fn rate(&self) -> u32 {
        if self.mixer_rate > 0 {
            self.mixer_rate
        } else {
            48000
        }
    }

    /// Rates the DAC takes bit-perfect.
    pub fn bit_perfect_rates(&self) -> Vec<u32> {
        let mut r: Vec<u32> = self
            .usb
            .iter()
            .flat_map(|u| u.bit_perfect.iter().map(|f| f.rate))
            .collect();
        r.sort_unstable();
        r.dedup();
        r
    }

    /// The encoding to ask for at `rate` for music with `bits` per sample: the same depth if
    /// offered, else the smallest deeper one (24-bit music in a 32-bit container stays exact).
    pub fn bit_perfect_encoding(&self, rate: u32, bits: u32) -> Option<i32> {
        let offered: Vec<i32> = self
            .usb
            .iter()
            .flat_map(|u| u.bit_perfect.iter())
            .filter(|f| f.rate == rate)
            .map(|f| f.encoding)
            .collect();
        let depth = |e: i32| match e {
            ENCODING_PCM_16BIT => 16,
            ENCODING_PCM_24BIT_PACKED => 24,
            ENCODING_PCM_32BIT => 32,
            ENCODING_PCM_FLOAT => 25, // 24-bit mantissa, after the 24-bit integers
            _ => 0,
        };
        offered
            .iter()
            .copied()
            .filter(|&e| depth(e) >= bits)
            .min_by_key(|&e| depth(e))
    }

    pub fn availability(&self, mode: Mode) -> Result<(), Unavailable> {
        match mode {
            Mode::Standard | Mode::HiRes | Mode::Exclusive => Ok(()),
            Mode::BitPerfect if self.sdk < 34 => Err(Unavailable::OldAndroid),
            Mode::BitPerfect | Mode::DirectUsb if self.usb.is_none() => Err(Unavailable::NoUsbDac),
            Mode::BitPerfect if self.bit_perfect_rates().is_empty() => Err(Unavailable::NotOffered),
            Mode::BitPerfect => Ok(()),
            Mode::DirectUsb => Err(Unavailable::NotYet),
        }
    }

    /// The mode actually used: the chosen one, or Hi-Res when it isn't available.
    pub fn effective(&self, chosen: Mode) -> Mode {
        if self.availability(chosen).is_ok() {
            chosen
        } else {
            Mode::HiRes
        }
    }
}

/// Without Android (desktop): only the system mixer's modes.
pub fn desktop_availability(mode: Mode) -> Result<(), Unavailable> {
    match mode {
        Mode::Standard | Mode::HiRes => Ok(()),
        _ => Err(Unavailable::NotHere),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The Xiaomi 14 Ultra with the Beats: Android 16, mixer at 48 kHz, no bit-perfect formats.
    const XIAOMI: &str = r#"{"sdk":36,"mixer_rate":48000,"usb":{"id":33408,"name":"Beats Studio Pro","bit_perfect":[]}}"#;

    #[test]
    fn the_xiaomi_offers_no_bit_perfect_and_falls_back_to_hi_res() {
        let m = Mixer::parse(XIAOMI).unwrap();
        assert_eq!(
            m.availability(Mode::BitPerfect),
            Err(Unavailable::NotOffered)
        );
        assert_eq!(m.availability(Mode::DirectUsb), Err(Unavailable::NotYet));
        assert_eq!(m.availability(Mode::Standard), Ok(()));
        assert_eq!(m.effective(Mode::BitPerfect), Mode::HiRes);
        assert_eq!(m.effective(Mode::Standard), Mode::Standard);
        assert_eq!(m.rate(), 48000);
    }

    #[test]
    fn spatial_audio_depends_on_the_phone_the_output_and_the_mode() {
        let on = Spatializer {
            level: 1,
            enabled: true,
            available: true,
            head_tracking: true,
            output: true,
        };
        let state =
            |sp: Option<Spatializer>, mode, setting| spatial_state(sp.as_ref(), mode, setting);
        assert_eq!(
            state(Some(on), Mode::HiRes, true),
            SpatialState::On {
                head_tracking: true
            }
        );
        assert_eq!(state(None, Mode::HiRes, true), SpatialState::Unsupported);
        assert_eq!(
            state(Some(Spatializer { level: 0, ..on }), Mode::HiRes, true),
            SpatialState::Unsupported
        );
        assert_eq!(state(Some(on), Mode::Standard, false), SpatialState::Off);
        // Never next to the modes that keep the music untouched.
        for mode in [Mode::Exclusive, Mode::BitPerfect, Mode::DirectUsb] {
            assert_eq!(state(Some(on), mode, true), SpatialState::Blocked);
        }
        assert_eq!(
            state(
                Some(Spatializer {
                    enabled: false,
                    ..on
                }),
                Mode::HiRes,
                true
            ),
            SpatialState::SystemOff
        );
        assert_eq!(
            state(
                Some(Spatializer {
                    output: false,
                    ..on
                }),
                Mode::HiRes,
                true
            ),
            SpatialState::NotThisOutput
        );
        // The JSON the glue sends.
        let m = Mixer::parse(
            r#"{"sdk":36,"spatial":{"level":2,"enabled":true,"available":false,"head_tracking":false,"output":false}}"#,
        )
        .unwrap();
        assert_eq!(m.spatial.unwrap().level, 2);
        assert_eq!(
            spatial_state(m.spatial.as_ref(), Mode::HiRes, true),
            SpatialState::NotThisOutput
        );
    }

    #[test]
    fn a_fixed_volume_is_reported() {
        let m = Mixer::parse(r#"{"sdk":36,"volume_fixed":true,"usb":{"id":1}}"#).unwrap();
        assert!(m.volume_fixed && m.usb.is_some());
        assert!(!Mixer::parse(XIAOMI).unwrap().volume_fixed);
    }

    #[test]
    fn a_phone_that_offers_bit_perfect() {
        let m = Mixer::parse(
            r#"{"sdk":34,"usb":{"id":7,"bit_perfect":[
                {"rate":44100,"encoding":2},{"rate":44100,"encoding":21},
                {"rate":96000,"encoding":22},{"rate":48000,"encoding":4}]}}"#,
        )
        .unwrap();
        assert_eq!(m.availability(Mode::BitPerfect), Ok(()));
        assert_eq!(m.bit_perfect_rates(), vec![44100, 48000, 96000]);
        assert_eq!(m.bit_perfect_encoding(44100, 16), Some(ENCODING_PCM_16BIT));
        assert_eq!(
            m.bit_perfect_encoding(44100, 24),
            Some(ENCODING_PCM_24BIT_PACKED)
        );
        // 24-bit music in a 32-bit container.
        assert_eq!(m.bit_perfect_encoding(96000, 24), Some(ENCODING_PCM_32BIT));
        assert_eq!(m.bit_perfect_encoding(48000, 16), Some(ENCODING_PCM_FLOAT));
        assert_eq!(m.bit_perfect_encoding(48000, 32), None);
        assert_eq!(m.bit_perfect_encoding(192000, 16), None);
        assert_eq!(m.rate(), 48000);
    }

    #[test]
    fn old_android_and_no_dac() {
        let old = Mixer::parse(r#"{"sdk":33,"usb":{"id":1}}"#).unwrap();
        assert_eq!(
            old.availability(Mode::BitPerfect),
            Err(Unavailable::OldAndroid)
        );
        let none = Mixer::parse(r#"{"sdk":35}"#).unwrap();
        assert_eq!(
            none.availability(Mode::BitPerfect),
            Err(Unavailable::NoUsbDac)
        );
        assert_eq!(
            none.availability(Mode::DirectUsb),
            Err(Unavailable::NoUsbDac)
        );
        assert_eq!(Mode::parse("bitperfect"), Mode::BitPerfect);
        assert_eq!(Mode::parse("nonsense"), Mode::HiRes);
        assert_eq!(
            desktop_availability(Mode::Exclusive),
            Err(Unavailable::NotHere)
        );
    }
}
