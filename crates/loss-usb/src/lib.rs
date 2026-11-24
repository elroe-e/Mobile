//! USB Audio Class support (M2-06, later M2-05's Direct USB driver). Pure Rust.
//!
//! [`descriptors::parse`] reads the raw descriptors Android or Linux hand over (device, then each
//! configuration) and finds what an audio DAC offers: class version, terminals, volume controls,
//! clocks and the streaming alternate settings with their formats and rates.
//! [`Probe::report`] turns that into the plain-text DAC report people send in.
//! On Linux and Android, [`devfs`] asks the device itself for what descriptors don't say
//! (volume ranges, UAC2 sample rates) through usbdevfs control transfers.

pub mod descriptors;
#[cfg(any(target_os = "linux", target_os = "android"))]
pub mod devfs;
pub mod stream;

pub use descriptors::{parse, AltSetting, Direction, Feature, Parsed, Rates, Sync, Terminal};

/// A volume range in dB: (min, max, step).
pub type DbRange = (f64, f64, f64);

/// What was asked of the device beyond its descriptors.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Queried {
    /// Feature unit id → volume range in dB (min, max, step).
    pub volume: Vec<(u8, Result<DbRange, String>)>,
    /// Clock source id → sample rates in Hz (UAC2).
    pub clock_rates: Vec<(u8, Result<Vec<u32>, String>)>,
}

/// A probed DAC: raw bytes, what they mean, and the answers to control requests.
#[derive(Debug, Clone)]
pub struct Probe {
    pub raw: Vec<u8>,
    pub parsed: Parsed,
    pub queried: Option<Queried>,
}

impl Probe {
    pub fn new(raw: Vec<u8>) -> Result<Probe, String> {
        let parsed = parse(&raw)?;
        Ok(Probe {
            raw,
            parsed,
            queried: None,
        })
    }

    /// Playback sample rates across all output settings (descriptor lists plus clock answers).
    pub fn output_rates(&self) -> Vec<u32> {
        let mut rates: Vec<u32> = Vec::new();
        for a in self.parsed.outputs() {
            if let Rates::Discrete(r) = &a.rates {
                rates.extend(r);
            }
        }
        if let Some(q) = &self.queried {
            for (_, r) in &q.clock_rates {
                if let Ok(r) = r {
                    rates.extend(r);
                }
            }
        }
        rates.sort_unstable();
        rates.dedup();
        rates
    }

    /// The report people copy and send (English on purpose, like other diagnostics).
    pub fn report(&self) -> String {
        let mut s = self.parsed.describe();
        if let Some(q) = &self.queried {
            s.push_str("\nAsked the device:\n");
            for (id, v) in &q.volume {
                match v {
                    Ok((lo, hi, step)) => s.push_str(&format!(
                        "  feature unit {id}: volume {lo:.2} to {hi:.2} dB, step {step:.3} dB\n"
                    )),
                    Err(e) => {
                        s.push_str(&format!("  feature unit {id}: volume not readable ({e})\n"))
                    }
                }
            }
            for (id, r) in &q.clock_rates {
                match r {
                    Ok(r) => s.push_str(&format!("  clock {id}: rates {}\n", join_rates(r))),
                    Err(e) => s.push_str(&format!("  clock {id}: rates not readable ({e})\n")),
                }
            }
        }
        s.push_str(&format!("\nRaw descriptors ({} bytes):\n", self.raw.len()));
        s.push_str(&hex(&self.raw));
        s
    }
}

pub fn join_rates(r: &[u32]) -> String {
    r.iter()
        .map(|r| r.to_string())
        .collect::<Vec<_>>()
        .join(", ")
        + " Hz"
}

/// Hex dump, one descriptor per line where the lengths make sense.
pub fn hex(raw: &[u8]) -> String {
    let mut out = String::new();
    let mut i = 0;
    while i < raw.len() {
        let len = (raw[i] as usize).max(1).min(raw.len() - i);
        let line: Vec<String> = raw[i..i + len].iter().map(|b| format!("{b:02x}")).collect();
        out.push_str(&line.join(" "));
        out.push('\n');
        i += len;
    }
    out
}

/// Reads hex back (as in a report), ignoring `#` comment lines and anything that isn't a hex
/// pair. For test fixtures.
pub fn unhex(text: &str) -> Vec<u8> {
    text.lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .flat_map(str::split_whitespace)
        .filter_map(|w| {
            (w.len() == 2)
                .then(|| u8::from_str_radix(w, 16).ok())
                .flatten()
        })
        .collect()
}
