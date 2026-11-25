//! AutoEq headphone corrections (M2-11). AutoEq (github.com/jaakkopasanen/AutoEq, MIT licence;
//! `data/AUTOEQ-LICENSE`) publishes a parametric EQ for thousands of headphones. Loss ships its
//! index (`data/autoeq-index.tsv`, made by `scripts/update-autoeq-index.py`) for searching, and
//! downloads the chosen profile (`loss_download::autoeq`). Profiles can also be pasted.

use serde::{Deserialize, Serialize};

/// One filter of a profile.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParamBand {
    /// "peak", "lowshelf", "highshelf", "lowpass" or "highpass".
    pub kind: String,
    pub freq: f64,
    #[serde(default)]
    pub gain: f64,
    #[serde(default = "default_q")]
    pub q: f64,
}

fn default_q() -> f64 {
    0.707
}

impl ParamBand {
    pub const KINDS: [&'static str; 5] = ["peak", "lowshelf", "highshelf", "lowpass", "highpass"];

    pub fn peak(freq: f64) -> ParamBand {
        ParamBand {
            kind: "peak".into(),
            freq,
            gain: 0.0,
            q: 1.0,
        }
    }

    /// Values in range (freq 10 Hz–22 kHz, gain ±24 dB, Q 0.1–20).
    pub fn sanitized(mut self) -> ParamBand {
        if !Self::KINDS.contains(&self.kind.as_str()) {
            self.kind = "peak".into();
        }
        let fix =
            |v: f64, lo: f64, hi: f64, d: f64| if v.is_finite() { v.clamp(lo, hi) } else { d };
        self.freq = fix(self.freq, 10.0, 22_000.0, 1000.0);
        self.gain = fix(self.gain, -24.0, 24.0, 0.0);
        self.q = fix(self.q, 0.1, 20.0, 0.707);
        self
    }
}

/// A correction curve for one headphone model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    /// "Beats Studio Pro" (or what the person called a pasted one).
    pub name: String,
    /// Where it comes from ("Rtings", "oratory1990", "pasted").
    #[serde(default)]
    pub source: String,
    /// AutoEq's suggested pre-amp (dB): automatic headroom replaces it when that's on.
    pub preamp: f64,
    pub bands: Vec<ParamBand>,
}

/// Reads AutoEq's "ParametricEQ.txt" (also Equalizer APO's config lines):
///
/// ```text
/// Preamp: -3.8 dB
/// Filter 1: ON LSC Fc 105 Hz Gain -3.1 dB Q 0.70
/// Filter 2: ON PK Fc 8990 Hz Gain -5.1 dB Q 2.02
/// ```
pub fn parse(text: &str, name: &str, source: &str) -> Option<Profile> {
    let mut preamp: f64 = 0.0;
    let mut bands = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        let lower = line.to_ascii_lowercase();
        if let Some(rest) = lower.strip_prefix("preamp:") {
            preamp = rest.split_whitespace().next()?.parse().ok()?;
            continue;
        }
        if !lower.starts_with("filter") {
            continue;
        }
        let Some((_, spec)) = line.split_once(':') else {
            continue;
        };
        let words: Vec<&str> = spec.split_whitespace().collect();
        if !words.first().is_some_and(|w| w.eq_ignore_ascii_case("on")) {
            continue; // OFF filters don't count
        }
        let kind = match words.get(1).map(|w| w.to_ascii_uppercase()).as_deref() {
            Some("PK" | "PEQ") => "peak",
            Some("LSC" | "LS" | "LSQ") => "lowshelf",
            Some("HSC" | "HS" | "HSQ") => "highshelf",
            Some("LP" | "LPQ") => "lowpass",
            Some("HP" | "HPQ") => "highpass",
            _ => continue,
        };
        // Values follow their keyword: "Fc 105 Hz", "Gain -3.1 dB", "Q 0.70".
        let after = |key: &str| {
            words
                .iter()
                .position(|w| w.eq_ignore_ascii_case(key))
                .and_then(|i| words.get(i + 1))
                .and_then(|v| v.parse::<f64>().ok())
        };
        let Some(freq) = after("Fc") else { continue };
        bands.push(
            ParamBand {
                kind: kind.into(),
                freq,
                gain: after("Gain").unwrap_or(0.0),
                q: after("Q").unwrap_or(0.707),
            }
            .sanitized(),
        );
    }
    (!bands.is_empty()).then(|| Profile {
        name: name.trim().chars().take(80).collect(),
        source: source.into(),
        preamp: if preamp.is_finite() {
            preamp.clamp(-30.0, 10.0)
        } else {
            0.0
        },
        bands,
    })
}

/// One headphone in AutoEq's index.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub name: &'static str,
    /// Who measured it ("Rtings", "oratory1990", …).
    pub source: &'static str,
    /// The measurement rig, when named.
    pub rig: &'static str,
    /// URL-encoded folder under AutoEq's results.
    pub path: &'static str,
}

const INDEX: &str = include_str!("../data/autoeq-index.tsv");

fn entries() -> impl Iterator<Item = Entry> {
    INDEX.lines().filter_map(|l| {
        let mut f = l.split('\t');
        Some(Entry {
            name: f.next()?,
            source: f.next()?,
            rig: f.next()?,
            path: f.next()?,
        })
    })
}

/// How many headphones the index knows.
pub fn index_len() -> usize {
    INDEX.lines().count()
}

/// Headphones whose name holds every word of the query (case-insensitive), best first: names
/// that start with the query, then shorter names; at most `limit`.
pub fn search(query: &str, limit: usize) -> Vec<Entry> {
    let words: Vec<String> = query.split_whitespace().map(|w| w.to_lowercase()).collect();
    if words.is_empty() {
        return vec![];
    }
    let q = query.trim().to_lowercase();
    let mut hits: Vec<(bool, usize, Entry)> = entries()
        .filter_map(|e| {
            let name = e.name.to_lowercase();
            words
                .iter()
                .all(|w| name.contains(w.as_str()))
                .then(|| (!name.starts_with(&q), e.name.len(), e))
        })
        .collect();
    hits.sort_by_key(|h| (h.0, h.1));
    hits.into_iter().take(limit).map(|(_, _, e)| e).collect()
}

/// Where AutoEq keeps the entry's parametric profile.
pub fn profile_url(e: &Entry) -> String {
    let file = format!("{} ParametricEQ.txt", folder_name(e.path));
    format!(
        "https://raw.githubusercontent.com/jaakkopasanen/AutoEq/master/results/{}/{}",
        e.path,
        encode(&file)
    )
}

/// The decoded last folder of a path ("Beats%20Studio%20Pro" → "Beats Studio Pro").
fn folder_name(path: &str) -> String {
    decode(path.rsplit('/').next().unwrap_or(path))
}

fn decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn encode(s: &str) -> String {
    s.bytes()
        .map(|c| match c {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (c as char).to_string()
            }
            _ => format!("%{c:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_beats_studio_pro_profile() {
        let text = include_str!("../fixtures/autoeq-beats-studio-pro.txt");
        let p = parse(text, "Beats Studio Pro", "Rtings").unwrap();
        assert_eq!(p.preamp, -3.8);
        assert_eq!(p.bands.len(), 10);
        assert_eq!(
            p.bands[0],
            ParamBand {
                kind: "lowshelf".into(),
                freq: 105.0,
                gain: -3.1,
                q: 0.7
            }
        );
        assert_eq!(p.bands[5].kind, "highshelf");
        assert_eq!(
            (p.bands[1].freq, p.bands[1].gain, p.bands[1].q),
            (8990.0, -5.1, 2.02)
        );
        assert!(parse(
            "Preamp: -1 dB\nFilter 1: OFF PK Fc 100 Hz Gain 3 dB Q 1",
            "x",
            "y"
        )
        .is_none());
        assert!(parse("nonsense", "x", "y").is_none());
    }

    #[test]
    fn finds_headphones_in_the_index() {
        assert!(index_len() > 5000);
        let hits = search("beats studio pro", 10);
        assert!(
            hits.iter().any(|e| e.name == "Beats Studio Pro"),
            "{hits:?}"
        );
        let beats = hits.iter().find(|e| e.name == "Beats Studio Pro").unwrap();
        assert_eq!(
            profile_url(beats),
            "https://raw.githubusercontent.com/jaakkopasanen/AutoEq/master/results/Rtings/Bruel%20&%20Kjaer%205128%20over-ear/Beats%20Studio%20Pro/Beats%20Studio%20Pro%20ParametricEQ.txt"
        );
        // Names that start with the query come first.
        let hd = search("sennheiser hd 6", 5);
        assert!(hd[0].name.starts_with("Sennheiser HD 6"), "{hd:?}");
        // Several measurements of one model: shortest names (exact model) first.
        assert_eq!(search("HD 600", 1)[0].name, "Sennheiser HD 600");
        assert!(search("   ", 5).is_empty());
        assert_eq!(decode("A%20%26%20B"), "A & B");
    }
}
