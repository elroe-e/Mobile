//! Standard USB and USB Audio Class (1.0, 2.0, 3.0) descriptor parsing.
//!
//! Descriptors are a flat list of `[bLength, bDescriptorType, ...]` records. Class-specific
//! interface records (0x24) mean different things in AudioControl and AudioStreaming interfaces,
//! so the parser tracks the current interface.

const DEVICE: u8 = 1;
const CONFIG: u8 = 2;
const INTERFACE: u8 = 4;
const ENDPOINT: u8 = 5;
const CS_INTERFACE: u8 = 0x24;

const AUDIO: u8 = 1;
const AUDIO_CONTROL: u8 = 1;
const AUDIO_STREAMING: u8 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// Host to DAC (playback).
    Out,
    /// DAC to host (microphone).
    In,
}

/// Isochronous synchronisation type (endpoint attributes bits 2–3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sync {
    None,
    Async,
    Adaptive,
    Sync,
}

impl Sync {
    fn from_attrs(a: u8) -> Sync {
        match (a >> 2) & 3 {
            1 => Sync::Async,
            2 => Sync::Adaptive,
            3 => Sync::Sync,
            _ => Sync::None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Sync::None => "no sync",
            Sync::Async => "asynchronous",
            Sync::Adaptive => "adaptive",
            Sync::Sync => "synchronous",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Rates {
    Discrete(Vec<u32>),
    Range(u32, u32),
    /// UAC2/3: rates come from the clock source (a control request), not the descriptors.
    FromClock,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Endpoint {
    pub address: u8,
    pub sync: Sync,
    pub max_packet: u16,
    pub interval: u8,
}

/// One alternate setting of a streaming interface (alt 0 is "idle" and has no format).
#[derive(Debug, Clone, PartialEq)]
pub struct AltSetting {
    pub interface: u8,
    pub alt: u8,
    pub direction: Direction,
    pub terminal_link: u8,
    /// UAC1 wFormatTag / UAC2 bmFormats bit (1 = PCM).
    pub format_tag: u32,
    pub channels: u8,
    /// Bytes per sample on the wire (subframe / subslot size).
    pub subslot: u8,
    pub bits: u8,
    pub rates: Rates,
    pub endpoint: Option<Endpoint>,
    /// The asynchronous feedback endpoint (IN) for an OUT setting, if any.
    pub feedback: Option<u8>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Terminal {
    pub id: u8,
    pub kind: u16,
    pub input: bool,
    /// UAC2: the clock entity the terminal runs on.
    pub clock: Option<u8>,
}

impl Terminal {
    pub fn name(&self) -> String {
        let k = match self.kind {
            0x0101 => "USB streaming",
            0x0201 => "microphone",
            0x0301 => "speaker",
            0x0302 => "headphones",
            0x0402 => "headset",
            0x0603 => "line",
            0x0605 => "S/PDIF",
            _ => "",
        };
        if k.is_empty() {
            format!("type {:04x}", self.kind)
        } else {
            k.to_string()
        }
    }
}

/// A feature unit and what it can control on the master channel.
#[derive(Debug, Clone, PartialEq)]
pub struct Feature {
    pub id: u8,
    pub source: u8,
    pub mute: bool,
    pub volume: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Parsed {
    pub vendor: u16,
    pub product: u16,
    pub usb_version: u16,
    pub configs: u8,
    pub interfaces: u8,
    /// bcdADC of the AudioControl header (0x0100 = UAC1, 0x0200 = UAC2).
    pub audio_version: Option<u16>,
    /// The AudioControl interface number (control requests go there).
    pub control_interface: Option<u8>,
    pub terminals: Vec<Terminal>,
    pub features: Vec<Feature>,
    pub clocks: Vec<u8>,
    pub settings: Vec<AltSetting>,
}

impl Parsed {
    pub fn outputs(&self) -> impl Iterator<Item = &AltSetting> {
        self.settings
            .iter()
            .filter(|a| a.direction == Direction::Out)
    }

    pub fn is_audio(&self) -> bool {
        self.audio_version.is_some()
    }

    pub fn class_name(&self) -> &'static str {
        match self.audio_version {
            Some(v) if v >= 0x0300 => "USB Audio Class 3",
            Some(v) if v >= 0x0200 => "USB Audio Class 2",
            Some(_) => "USB Audio Class 1",
            None => "not an audio device",
        }
    }

    /// Plain-text description (part of the report).
    pub fn describe(&self) -> String {
        let mut s = format!(
            "USB {:04x}:{:04x}, USB {}.{:x}, {} configuration(s), {} interface(s)\n{}\n",
            self.vendor,
            self.product,
            self.usb_version >> 8,
            (self.usb_version >> 4) & 0xf,
            self.configs,
            self.interfaces,
            self.class_name(),
        );
        for t in &self.terminals {
            s.push_str(&format!(
                "  {} terminal {}: {}\n",
                if t.input { "input" } else { "output" },
                t.id,
                t.name()
            ));
        }
        for f in &self.features {
            s.push_str(&format!(
                "  feature unit {} (from {}): {}{}\n",
                f.id,
                f.source,
                if f.volume { "volume " } else { "" },
                if f.mute { "mute" } else { "" }
            ));
        }
        for c in &self.clocks {
            s.push_str(&format!("  clock source {c}\n"));
        }
        for a in &self.settings {
            s.push_str(&format!("  {}\n", a.describe()));
        }
        s
    }
}

impl AltSetting {
    pub fn describe(&self) -> String {
        let rates = match &self.rates {
            Rates::Discrete(r) => crate::join_rates(r),
            Rates::Range(lo, hi) => format!("{lo}–{hi} Hz"),
            Rates::FromClock => "rates from the clock".into(),
        };
        let ep = self
            .endpoint
            .as_ref()
            .map(|e| {
                format!(
                    ", endpoint {:02x} {} max {} bytes",
                    e.address,
                    e.sync.name(),
                    e.max_packet & 0x7ff
                )
            })
            .unwrap_or_default();
        format!(
            "{} interface {} alt {}: {} ch, {}-bit in {} bytes, {}{}{}",
            if self.direction == Direction::Out {
                "playback"
            } else {
                "capture"
            },
            self.interface,
            self.alt,
            self.channels,
            self.bits,
            self.subslot,
            rates,
            if self.format_tag == 1 { ", PCM" } else { "" },
            ep
        )
    }
}

fn u16_at(b: &[u8], i: usize) -> u16 {
    match b.get(i..i + 2) {
        Some(x) => u16::from_le_bytes([x[0], x[1]]),
        None => 0,
    }
}

fn u24_at(b: &[u8], i: usize) -> u32 {
    match b.get(i..i + 3) {
        Some(x) => u32::from_le_bytes([x[0], x[1], x[2], 0]),
        None => 0,
    }
}

fn u32_at(b: &[u8], i: usize) -> u32 {
    match b.get(i..i + 4) {
        Some(x) => u32::from_le_bytes([x[0], x[1], x[2], x[3]]),
        None => 0,
    }
}

/// Parses raw descriptors (device descriptor first, then configurations).
pub fn parse(raw: &[u8]) -> Result<Parsed, String> {
    let mut p = Parsed::default();
    let mut i = 0;
    // The interface the following records belong to: (number, alt, class, subclass).
    let mut cur: Option<(u8, u8, u8, u8)> = None;
    let mut uac2 = false;
    let mut seen_device = false;
    while i + 2 <= raw.len() {
        let len = raw[i] as usize;
        if len < 2 || i + len > raw.len() {
            return Err(format!("bad descriptor length {len} at byte {i}"));
        }
        let d = &raw[i..i + len];
        match d[1] {
            DEVICE if len >= 18 => {
                seen_device = true;
                p.usb_version = u16_at(d, 2);
                p.vendor = u16_at(d, 8);
                p.product = u16_at(d, 10);
                p.configs = d[17];
            }
            CONFIG if len >= 9 => {
                p.interfaces = p.interfaces.max(d[4]);
                cur = None;
            }
            INTERFACE if len >= 9 => {
                cur = Some((d[2], d[3], d[5], d[6]));
                if d[5] == AUDIO && d[6] == AUDIO_STREAMING && d[3] > 0 {
                    // A format setting; filled in by the class-specific records that follow.
                    p.settings.push(AltSetting {
                        interface: d[2],
                        alt: d[3],
                        direction: Direction::Out,
                        terminal_link: 0,
                        format_tag: 0,
                        channels: 0,
                        subslot: 0,
                        bits: 0,
                        rates: Rates::Discrete(vec![]),
                        endpoint: None,
                        feedback: None,
                    });
                }
                if d[5] == AUDIO && d[6] == AUDIO_CONTROL {
                    p.control_interface.get_or_insert(d[2]);
                }
            }
            ENDPOINT if len >= 7 => {
                if let (Some((n, a, AUDIO, AUDIO_STREAMING)), Some(s)) =
                    (cur, p.settings.last_mut())
                {
                    // Only the data endpoint (isochronous with a sync type); the feedback
                    // endpoint follows it with usage bits set.
                    let iso = d[3] & 3 == 1;
                    let feedback = (d[3] >> 4) & 3 == 1;
                    let ours = s.interface == n && s.alt == a && iso;
                    // An IN endpoint after an OUT data endpoint feeds back the DAC's clock
                    // (UAC2 marks it with usage bits; UAC1 leaves them zero).
                    let out_data = s.endpoint.as_ref().is_some_and(|e| e.address & 0x80 == 0);
                    if ours && out_data && d[2] & 0x80 != 0 && s.feedback.is_none() {
                        s.feedback = Some(d[2]);
                    }
                    if ours && !feedback && s.endpoint.is_none() {
                        s.direction = if d[2] & 0x80 != 0 {
                            Direction::In
                        } else {
                            Direction::Out
                        };
                        s.endpoint = Some(Endpoint {
                            address: d[2],
                            sync: Sync::from_attrs(d[3]),
                            max_packet: u16_at(d, 4),
                            interval: d[6],
                        });
                    }
                }
            }
            CS_INTERFACE if len >= 3 => match cur {
                Some((_, _, AUDIO, AUDIO_CONTROL)) => match d[2] {
                    1 if len >= 5 => {
                        let v = u16_at(d, 3);
                        uac2 = v >= 0x0200;
                        p.audio_version.get_or_insert(v);
                    }
                    // UAC2 terminals name their clock (input: byte 7, output: byte 8).
                    2 if len >= 6 => p.terminals.push(Terminal {
                        id: d[3],
                        kind: u16_at(d, 4),
                        input: true,
                        clock: uac2.then(|| d.get(7).copied()).flatten(),
                    }),
                    3 if len >= 6 => p.terminals.push(Terminal {
                        id: d[3],
                        kind: u16_at(d, 4),
                        input: false,
                        clock: uac2.then(|| d.get(8).copied()).flatten(),
                    }),
                    6 if len >= 6 => {
                        let (mute, volume) = if uac2 {
                            let c = u32_at(d, 5);
                            (c & 0b11 != 0, c & 0b1100 != 0)
                        } else {
                            let c = d.get(6).copied().unwrap_or(0);
                            (c & 1 != 0, c & 2 != 0)
                        };
                        p.features.push(Feature {
                            id: d[3],
                            source: d[4],
                            mute,
                            volume,
                        });
                    }
                    0x0A if uac2 && len >= 4 => p.clocks.push(d[3]),
                    _ => {}
                },
                Some((n, a, AUDIO, AUDIO_STREAMING)) => {
                    let Some(s) = p
                        .settings
                        .last_mut()
                        .filter(|s| s.interface == n && s.alt == a)
                    else {
                        i += len;
                        continue;
                    };
                    match d[2] {
                        // AS_GENERAL
                        1 if len >= 7 => {
                            s.terminal_link = d[3];
                            if uac2 {
                                s.format_tag = u32_at(d, 6);
                                s.channels = d.get(10).copied().unwrap_or(0);
                            } else {
                                s.format_tag = u16_at(d, 5) as u32;
                            }
                        }
                        // FORMAT_TYPE (type I)
                        2 if len >= 6 => {
                            if uac2 {
                                s.subslot = d[4];
                                s.bits = d[5];
                                s.rates = Rates::FromClock;
                            } else if len >= 8 {
                                s.channels = d[4];
                                s.subslot = d[5];
                                s.bits = d[6];
                                let n = d[7] as usize;
                                s.rates = if n == 0 {
                                    Rates::Range(u24_at(d, 8), u24_at(d, 11))
                                } else {
                                    Rates::Discrete((0..n).map(|k| u24_at(d, 8 + 3 * k)).collect())
                                };
                            }
                        }
                        _ => {}
                    }
                }
                _ => {}
            },
            _ => {}
        }
        i += len;
    }
    if !seen_device {
        return Err("no device descriptor".into());
    }
    // A setting's direction follows its terminal when it had no endpoint we recognised.
    let inputs: Vec<u8> = p
        .terminals
        .iter()
        .filter(|t| t.input && t.kind == 0x0101)
        .map(|t| t.id)
        .collect();
    for s in p.settings.iter_mut().filter(|s| s.endpoint.is_none()) {
        if !inputs.is_empty() && !inputs.contains(&s.terminal_link) {
            s.direction = Direction::In;
        }
    }
    Ok(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A UAC1 headset like the Beats: 2-channel 16- and 24-bit playback at 32/48 kHz, a mono
    /// microphone, hardware volume. Made up from the UAC1 spec (the real report replaces it).
    #[rustfmt::skip]
    pub(crate) fn uac1_headset() -> Vec<u8> {
        let mut b = vec![
            // device: USB 2.0, vendor 290b, product 0900, 1 configuration
            18, 1, 0x00, 0x02, 0, 0, 0, 64, 0x0b, 0x29, 0x00, 0x09, 0x00, 0x01, 1, 2, 0, 1,
            // configuration: 3 interfaces
            9, 2, 0, 0, 3, 1, 0, 0x80, 50,
            // AudioControl interface 0
            9, 4, 0, 0, 0, 1, 1, 0, 0,
            // header bcdADC 1.00
            10, 0x24, 1, 0x00, 0x01, 0, 0, 2, 1, 2,
            // input terminal 1: USB streaming
            12, 0x24, 2, 1, 0x01, 0x01, 0, 2, 3, 0, 0, 0,
            // feature unit 2 from 1: master mute + volume
            9, 0x24, 6, 2, 1, 1, 0x03, 0x00, 0,
            // output terminal 3: headphones
            9, 0x24, 3, 3, 0x02, 0x03, 0, 2, 0,
            // input terminal 4: microphone; output terminal 5: USB streaming
            12, 0x24, 2, 4, 0x01, 0x02, 0, 1, 0, 0, 0, 0,
            9, 0x24, 3, 5, 0x01, 0x01, 0, 4, 0,
            // streaming interface 1 alt 0 (idle)
            9, 4, 1, 0, 0, 1, 2, 0, 0,
            // alt 1: 16-bit
            9, 4, 1, 1, 1, 1, 2, 0, 0,
            7, 0x24, 1, 1, 1, 0x01, 0x00,
            14, 0x24, 2, 1, 2, 2, 16, 2, 0x00, 0x7d, 0x00, 0x80, 0xbb, 0x00,
            9, 5, 0x01, 0x09, 0xc0, 0x00, 1, 0, 0,
            7, 0x25, 1, 1, 0, 0, 0,
            // alt 2: 24-bit
            9, 4, 1, 2, 1, 1, 2, 0, 0,
            7, 0x24, 1, 1, 1, 0x01, 0x00,
            14, 0x24, 2, 1, 2, 3, 24, 2, 0x00, 0x7d, 0x00, 0x80, 0xbb, 0x00,
            9, 5, 0x01, 0x09, 0x20, 0x01, 1, 0, 0,
            7, 0x25, 1, 1, 0, 0, 0,
            // microphone interface 2 alt 1: mono 16-bit 48 kHz, IN endpoint
            9, 4, 2, 0, 0, 1, 2, 0, 0,
            9, 4, 2, 1, 1, 1, 2, 0, 0,
            7, 0x24, 1, 5, 1, 0x01, 0x00,
            11, 0x24, 2, 1, 1, 2, 16, 1, 0x80, 0xbb, 0x00,
            9, 5, 0x82, 0x05, 0x60, 0x00, 1, 0, 0,
            7, 0x25, 1, 1, 0, 0, 0,
        ];
        let total = (b.len() - 18) as u16;
        b[18 + 2..18 + 4].copy_from_slice(&total.to_le_bytes());
        b
    }

    /// A UAC2 DAC: async 32-bit playback, rates from clock source 41.
    #[rustfmt::skip]
    fn uac2_dac() -> Vec<u8> {
        vec![
            18, 1, 0x00, 0x02, 0xef, 2, 1, 64, 0x34, 0x12, 0x78, 0x56, 0x00, 0x01, 1, 2, 0, 1,
            9, 2, 0, 0, 2, 1, 0, 0x80, 50,
            // interface association
            8, 0x0b, 0, 2, 1, 0, 0x20, 0,
            // AudioControl, header bcdADC 2.00, clock source 41
            9, 4, 0, 0, 0, 1, 1, 0x20, 0,
            9, 0x24, 1, 0x00, 0x02, 8, 0, 0, 0,
            8, 0x24, 0x0A, 41, 3, 7, 0, 0,
            // input terminal 1 (USB streaming), feature unit 2 (mute + volume), output 3
            17, 0x24, 2, 1, 0x01, 0x01, 0, 41, 2, 3, 0, 0, 0, 0, 0, 0, 0,
            18, 0x24, 6, 2, 1, 0x0f, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            12, 0x24, 3, 3, 0x02, 0x03, 0, 2, 41, 0, 0, 0,
            // streaming interface 1: alt 0 idle, alt 1 PCM 2 ch 32-bit
            9, 4, 1, 0, 0, 1, 2, 0x20, 0,
            9, 4, 1, 1, 2, 1, 2, 0x20, 0,
            16, 0x24, 1, 1, 0, 1, 1, 0, 0, 0, 2, 3, 0, 0, 0, 0,
            6, 0x24, 2, 1, 4, 32,
            // async data endpoint, then its feedback endpoint
            7, 5, 0x01, 0x05, 0x00, 0x04, 1,
            8, 0x25, 1, 0, 0, 0, 0, 0,
            7, 5, 0x81, 0x11, 0x04, 0x00, 4,
        ]
    }

    #[test]
    fn the_fixture_file_matches() {
        // `fixtures/uac1-headset.hex` is this headset, for the app's desktop preview.
        let file = include_str!("../fixtures/uac1-headset.hex");
        let want = crate::hex(&uac1_headset());
        if crate::unhex(file) != uac1_headset() {
            panic!("regenerate fixtures/uac1-headset.hex:\n{want}");
        }
    }

    #[test]
    fn reads_a_uac1_headset() {
        let p = parse(&uac1_headset()).unwrap();
        assert_eq!((p.vendor, p.product), (0x290b, 0x0900));
        assert_eq!(p.class_name(), "USB Audio Class 1");
        assert_eq!(p.control_interface, Some(0));
        assert_eq!(
            p.features,
            vec![Feature {
                id: 2,
                source: 1,
                mute: true,
                volume: true
            }]
        );
        let out: Vec<_> = p.outputs().collect();
        assert_eq!(out.len(), 2);
        assert_eq!((out[0].channels, out[0].subslot, out[0].bits), (2, 2, 16));
        assert_eq!((out[1].subslot, out[1].bits), (3, 24));
        assert_eq!(out[0].rates, Rates::Discrete(vec![32000, 48000]));
        assert_eq!(out[0].endpoint.as_ref().unwrap().sync, Sync::Adaptive);
        let mic: Vec<_> = p
            .settings
            .iter()
            .filter(|s| s.direction == Direction::In)
            .collect();
        assert_eq!(mic.len(), 1);
        assert_eq!(mic[0].channels, 1);
        let text = p.describe();
        assert!(
            text.contains("playback interface 1 alt 2: 2 ch, 24-bit in 3 bytes, 32000, 48000 Hz")
        );
        assert!(text.contains("output terminal 3: headphones"));
    }

    #[test]
    fn reads_a_uac2_dac() {
        let p = parse(&uac2_dac()).unwrap();
        assert_eq!(p.class_name(), "USB Audio Class 2");
        assert_eq!(p.clocks, vec![41]);
        assert_eq!(
            p.features,
            vec![Feature {
                id: 2,
                source: 1,
                mute: true,
                volume: true
            }]
        );
        let out: Vec<_> = p.outputs().collect();
        assert_eq!(out.len(), 1);
        assert_eq!((out[0].channels, out[0].subslot, out[0].bits), (2, 4, 32));
        assert_eq!(out[0].rates, Rates::FromClock);
        let ep = out[0].endpoint.as_ref().unwrap();
        // The data endpoint, not the feedback one, which is noted separately.
        assert_eq!((ep.address, ep.sync), (0x01, Sync::Async));
        assert_eq!(out[0].feedback, Some(0x81));
        assert_eq!(p.terminals[0].clock, Some(41));
    }

    #[test]
    fn rejects_garbage_and_survives_round_trips_through_the_report() {
        assert!(parse(&[]).is_err());
        assert!(parse(&[5, 1, 0]).is_err());
        let probe = crate::Probe::new(uac1_headset()).unwrap();
        let report = probe.report();
        assert_eq!(
            crate::unhex(report.split("Raw descriptors").nth(1).unwrap()),
            uac1_headset()
        );
        assert_eq!(probe.output_rates(), vec![32000, 48000]);
    }
}
