//! Planning a Direct USB stream (M2-05): which alternate setting carries a rate and bit depth,
//! the control requests that set the rate and volume, how many frames go in each isochronous
//! packet, and how to read the DAC's feedback. Pure logic; the I/O lives in `devfs`.

use crate::descriptors::{AltSetting, Parsed, Rates, Sync};

/// A chosen stream: the setting to select and how to fill its packets.
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    pub interface: u8,
    pub alt: u8,
    pub endpoint: u8,
    pub feedback: Option<u8>,
    pub sync: Sync,
    pub max_packet: u16,
    /// Endpoint bInterval (packets every 2^(n-1) (micro)frames).
    pub interval: u8,
    pub channels: u8,
    /// Bytes per sample on the wire.
    pub subslot: u8,
    /// Meaningful bits per sample (the rest are zero padding, low bits).
    pub bits: u8,
    pub rate: u32,
    pub uac2: bool,
    /// UAC2: the clock to set the rate on.
    pub clock: Option<u8>,
    pub control_interface: u8,
}

fn takes(a: &AltSetting, rate: u32, clock_rates: &[u32]) -> bool {
    match &a.rates {
        Rates::Discrete(r) => r.contains(&rate),
        Rates::Range(lo, hi) => *lo <= rate && rate <= *hi,
        Rates::FromClock => clock_rates.is_empty() || clock_rates.contains(&rate),
    }
}

/// Picks the playback setting for `rate`: stereo PCM first, then the exact bit depth wanted, else
/// the deepest one (so 16-bit music still plays bit-perfect in a 24-bit container).
/// `clock_rates` are the UAC2 clock's rates if they were asked for (empty: trust the device).
pub fn plan(p: &Parsed, rate: u32, bits: u8, clock_rates: &[u32]) -> Result<Plan, String> {
    let control_interface = p
        .control_interface
        .ok_or("the device has no audio control interface")?;
    let candidates: Vec<&AltSetting> = p
        .outputs()
        .filter(|a| a.endpoint.is_some() && a.format_tag == 1 && takes(a, rate, clock_rates))
        .collect();
    if candidates.is_empty() {
        return Err(format!("no playback setting takes {rate} Hz"));
    }
    let best = candidates
        .iter()
        .max_by_key(|a| {
            (
                a.channels == 2,
                a.bits == bits,
                a.bits >= bits,
                a.bits,
                std::cmp::Reverse(a.subslot),
            )
        })
        .copied()
        .ok_or("no setting")?;
    let ep = best.endpoint.as_ref().ok_or("no endpoint")?;
    let uac2 = p.audio_version.is_some_and(|v| v >= 0x0200);
    let clock = if uac2 {
        p.terminals
            .iter()
            .find(|t| t.id == best.terminal_link)
            .and_then(|t| t.clock)
            .or_else(|| p.clocks.first().copied())
    } else {
        None
    };
    Ok(Plan {
        interface: best.interface,
        alt: best.alt,
        endpoint: ep.address,
        feedback: best.feedback,
        sync: ep.sync,
        max_packet: ep.max_packet & 0x7ff,
        interval: ep.interval,
        channels: best.channels,
        subslot: best.subslot,
        bits: best.bits,
        rate,
        uac2,
        clock,
        control_interface,
    })
}

/// A class control request from host to device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlOut {
    pub request_type: u8,
    pub request: u8,
    pub value: u16,
    pub index: u16,
    pub data: Vec<u8>,
}

const SET_CUR: u8 = 0x01;

impl Plan {
    /// Sets the sample rate: UAC1 on the endpoint, UAC2 on the clock source.
    pub fn rate_request(&self) -> ControlOut {
        if self.uac2 {
            ControlOut {
                request_type: 0x21, // class, interface, out
                request: SET_CUR,   // CUR
                value: 0x01 << 8,   // SAM_FREQ_CONTROL
                index: ((self.clock.unwrap_or(0) as u16) << 8) | self.control_interface as u16,
                data: self.rate.to_le_bytes().to_vec(),
            }
        } else {
            ControlOut {
                request_type: 0x22, // class, endpoint, out
                request: SET_CUR,
                value: 0x01 << 8, // SAMPLING_FREQ_CONTROL
                index: self.endpoint as u16,
                data: self.rate.to_le_bytes()[..3].to_vec(),
            }
        }
    }

    /// Packets per second on the bus (full speed: 1 ms frames; high speed: 125 µs microframes).
    pub fn packets_per_second(&self, high_speed: bool) -> u32 {
        if high_speed {
            8000 >> self.interval.clamp(1, 4).saturating_sub(1)
        } else {
            1000
        }
    }

    /// Bytes per audio frame on the wire.
    pub fn frame_bytes(&self) -> usize {
        self.channels as usize * self.subslot as usize
    }
}

/// Sets a feature unit's volume (dB, 1/256 steps) on the master channel.
pub fn volume_request(control_interface: u8, unit: u8, db: f64) -> ControlOut {
    let v = (db * 256.0).round().clamp(i16::MIN as f64, i16::MAX as f64) as i16;
    ControlOut {
        request_type: 0x21,
        request: SET_CUR,
        value: 0x02 << 8, // VOLUME_CONTROL, channel 0
        index: ((unit as u16) << 8) | control_interface as u16,
        data: v.to_le_bytes().to_vec(),
    }
}

/// Mutes or unmutes a feature unit.
pub fn mute_request(control_interface: u8, unit: u8, mute: bool) -> ControlOut {
    ControlOut {
        request_type: 0x21,
        request: SET_CUR,
        value: 0x01 << 8, // MUTE_CONTROL, channel 0
        index: ((unit as u16) << 8) | control_interface as u16,
        data: vec![mute as u8],
    }
}

/// Frames per isochronous packet. Without feedback it spreads the rate exactly over the packets
/// (44.1 kHz at 1000 packets/s: nine packets of 44 frames, then one of 45). With an asynchronous
/// DAC it follows the feedback value, the DAC's own clock, as Q16.16 frames per packet.
#[derive(Debug, Clone)]
pub struct PacketSizer {
    rate: u32,
    pps: u32,
    acc: u64,
    /// Feedback in Q16.16 frames per packet, once the DAC has sent one.
    feedback: Option<u32>,
    acc_q16: u32,
}

impl PacketSizer {
    pub fn new(rate: u32, packets_per_second: u32) -> PacketSizer {
        PacketSizer {
            rate,
            pps: packets_per_second.max(1),
            acc: 0,
            feedback: None,
            acc_q16: 0,
        }
    }

    /// The nominal frames per packet in Q16.16.
    pub fn nominal_q16(&self) -> u32 {
        ((self.rate as u64) << 16).div_ceil(self.pps as u64) as u32
    }

    /// Frames for the next packet.
    pub fn next_frames(&mut self) -> usize {
        match self.feedback {
            Some(fb) => {
                let total = self.acc_q16 as u64 + fb as u64;
                self.acc_q16 = (total & 0xffff) as u32;
                (total >> 16) as usize
            }
            None => {
                self.acc += self.rate as u64;
                let n = self.acc / self.pps as u64;
                self.acc -= n * self.pps as u64;
                n as usize
            }
        }
    }

    /// Takes a feedback packet; ignores values that don't look like this stream's rate.
    pub fn feedback(&mut self, bytes: &[u8]) -> bool {
        match parse_feedback(bytes, self.nominal_q16()) {
            Some(v) => {
                self.feedback = Some(v);
                true
            }
            None => false,
        }
    }

    /// The most frames a packet can hold (for buffer sizes): nominal plus one, with headroom
    /// for feedback running fast.
    pub fn max_frames(&self) -> usize {
        (self.nominal_q16() as usize >> 16) + 2
    }
}

/// Reads a feedback value as Q16.16 frames per packet. Full-speed devices send 10.14 in 3 bytes,
/// high-speed ones 16.16 in 4 bytes, and some send the other format in the other size; like
/// Linux, try the readings and keep the one within ±25 % of the nominal rate.
pub fn parse_feedback(bytes: &[u8], nominal_q16: u32) -> Option<u32> {
    let raw = match bytes.len() {
        3 => u32::from_le_bytes([bytes[0], bytes[1], bytes[2], 0]),
        4.. => u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
        _ => return None,
    };
    if raw == 0 {
        return None;
    }
    let near = |v: u64| {
        let n = nominal_q16 as u64;
        v * 4 >= n * 3 && v * 4 <= n * 5
    };
    // 10.14 → shift 2; 16.16 as is; 10.14 sent per microframe-in-frames etc. → shift 2 more.
    [2u32, 0, 4]
        .iter()
        .map(|&s| (raw as u64) << s)
        .chain([(raw as u64) >> 2])
        .find(|&v| near(v))
        .map(|v| v as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::descriptors::parse;

    fn headset() -> Parsed {
        parse(&crate::unhex(include_str!("../fixtures/uac1-headset.hex"))).unwrap()
    }

    #[test]
    fn picks_the_setting_for_the_bits_and_rate() {
        let p = headset();
        let a = plan(&p, 48000, 24, &[]).unwrap();
        assert_eq!((a.interface, a.alt, a.subslot, a.bits), (1, 2, 3, 24));
        let b = plan(&p, 48000, 16, &[]).unwrap();
        assert_eq!((b.alt, b.bits), (1, 16));
        // 24-bit music on a 16-bit-only choice would lose bits: 20-bit asks get the 24-bit one.
        assert_eq!(plan(&p, 32000, 20, &[]).unwrap().alt, 2);
        assert!(plan(&p, 44100, 16, &[]).is_err());
        assert_eq!(a.feedback, None);
        assert_eq!(a.frame_bytes(), 6);
    }

    #[test]
    fn rate_requests_follow_the_class_version() {
        let a = plan(&headset(), 48000, 24, &[]).unwrap();
        // UAC1: SET_CUR to the endpoint, 3 bytes.
        assert_eq!(
            a.rate_request(),
            ControlOut {
                request_type: 0x22,
                request: 1,
                value: 0x0100,
                index: 0x01,
                data: vec![0x80, 0xbb, 0x00],
            }
        );
        let mut b = a.clone();
        (b.uac2, b.clock) = (true, Some(41));
        let r = b.rate_request();
        assert_eq!((r.request_type, r.index), (0x21, 41 << 8));
        assert_eq!(r.data, 48000u32.to_le_bytes().to_vec());
        assert_eq!(
            volume_request(0, 2, -10.5).data,
            (-2688i16).to_le_bytes().to_vec()
        );
        assert_eq!(mute_request(0, 2, true).value, 0x0100);
    }

    #[test]
    fn cd_rate_packets_add_up_exactly() {
        let mut s = PacketSizer::new(44100, 1000);
        let sizes: Vec<usize> = (0..10).map(|_| s.next_frames()).collect();
        assert_eq!(sizes.iter().sum::<usize>(), 441);
        assert!(sizes.iter().all(|&n| n == 44 || n == 45));
        let total: usize = (0..990).map(|_| s.next_frames()).sum::<usize>() + 441;
        assert_eq!(total, 44100);
        // 48 kHz on high speed, every microframe: 6 frames each.
        let mut h = PacketSizer::new(48000, 8000);
        assert!((0..8000).all(|_| h.next_frames() == 6));
        assert_eq!(h.max_frames(), 8);
    }

    #[test]
    fn follows_the_dacs_clock() {
        let mut s = PacketSizer::new(48000, 1000);
        // Full speed, 10.14: 48.01 frames per ms (the DAC runs slightly fast).
        let v = (48.01 * 16384.0) as u32;
        assert!(s.feedback(&v.to_le_bytes()[..3]));
        let total: usize = (0..1000).map(|_| s.next_frames()).sum();
        assert!((48009..=48011).contains(&total), "{total}");
        // High speed, 16.16 per microframe (6 frames).
        let hs = (6.0 * 65536.0) as u32;
        assert_eq!(parse_feedback(&hs.to_le_bytes(), 6 << 16), Some(6 << 16));
        // Nonsense is ignored.
        assert!(!s.feedback(&[0, 0, 0]));
        assert!(!s.feedback(&[0xff, 0xff, 0xff, 0x7f]));
    }
}
