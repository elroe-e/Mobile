//! Equalizer settings and presets (M2-10). The sound itself is in `loss_audio::eq`; this is
//! what's saved and shared. Presets are JSON (≤ 64 KB) with the same field names Mornye uses, so
//! its exported presets import too.

use serde::{Deserialize, Serialize};

pub const BAND_COUNTS: [usize; 4] = [5, 10, 15, 31];
/// Largest preset accepted for import.
pub const MAX_PRESET_BYTES: usize = 64 * 1024;

/// Graphic band centres in Hz (the same table as `loss_audio::eq::centres`).
pub fn centres(count: usize) -> &'static [f64] {
    match count {
        5 => &[63.0, 250.0, 1000.0, 4000.0, 16000.0],
        15 => &[
            25.0, 40.0, 63.0, 100.0, 160.0, 250.0, 400.0, 630.0, 1000.0, 1600.0, 2500.0, 4000.0,
            6300.0, 10000.0, 16000.0,
        ],
        31 => &[
            20.0, 25.0, 31.5, 40.0, 50.0, 63.0, 80.0, 100.0, 125.0, 160.0, 200.0, 250.0, 315.0,
            400.0, 500.0, 630.0, 800.0, 1000.0, 1250.0, 1600.0, 2000.0, 2500.0, 3150.0, 4000.0,
            5000.0, 6300.0, 8000.0, 10000.0, 12500.0, 16000.0, 20000.0,
        ],
        _ => &[
            31.25, 62.5, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0,
        ],
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct EqSettings {
    pub enabled: bool,
    /// dB per graphic band (5, 10, 15 or 31 of them).
    pub gains: Vec<f64>,
    pub preamp: f64,
    pub bass: f64,
    pub treble: f64,
    /// −1 … 1.
    pub balance: f64,
    pub auto_headroom: bool,
    pub limiter: bool,
    pub compressor: bool,
    /// Compressor threshold (dB), ratio, attack and release (ms).
    pub threshold: f64,
    pub ratio: f64,
    pub attack: f64,
    pub release: f64,
}

impl Default for EqSettings {
    fn default() -> Self {
        EqSettings {
            enabled: false,
            gains: vec![0.0; 10],
            preamp: 0.0,
            bass: 0.0,
            treble: 0.0,
            balance: 0.0,
            auto_headroom: true,
            limiter: true,
            compressor: false,
            threshold: -18.0,
            ratio: 3.0,
            attack: 10.0,
            release: 150.0,
        }
    }
}

impl EqSettings {
    /// Every value in range, a valid band count (else 10 flat bands).
    pub fn sanitized(mut self) -> EqSettings {
        let fix = |v: f64, lo: f64, hi: f64, dflt: f64| {
            if v.is_finite() {
                v.clamp(lo, hi)
            } else {
                dflt
            }
        };
        if !BAND_COUNTS.contains(&self.gains.len()) {
            self.gains = vec![0.0; 10];
        }
        self.gains = self
            .gains
            .iter()
            .map(|&g| fix(g, -15.0, 15.0, 0.0))
            .collect();
        self.preamp = fix(self.preamp, -24.0, 12.0, 0.0);
        self.bass = fix(self.bass, -12.0, 12.0, 0.0);
        self.treble = fix(self.treble, -12.0, 12.0, 0.0);
        self.balance = fix(self.balance, -1.0, 1.0, 0.0);
        self.threshold = fix(self.threshold, -60.0, 0.0, -18.0);
        self.ratio = fix(self.ratio, 1.0, 20.0, 3.0);
        self.attack = fix(self.attack, 1.0, 100.0, 10.0);
        self.release = fix(self.release, 20.0, 1000.0, 150.0);
        self
    }

    /// The same curve over another number of bands (interpolated in log frequency).
    pub fn with_band_count(&self, count: usize) -> EqSettings {
        let from = centres(self.gains.len());
        let gain_at = |hz: f64| {
            let x = hz.log2();
            let xs: Vec<f64> = from.iter().map(|f| f.log2()).collect();
            if x <= xs[0] {
                return self.gains[0];
            }
            for i in 1..xs.len() {
                if x <= xs[i] {
                    let t = (x - xs[i - 1]) / (xs[i] - xs[i - 1]);
                    return self.gains[i - 1] + t * (self.gains[i] - self.gains[i - 1]);
                }
            }
            *self.gains.last().unwrap_or(&0.0)
        };
        EqSettings {
            gains: centres(count)
                .iter()
                .map(|&f| (gain_at(f) * 10.0).round() / 10.0)
                .collect(),
            ..self.clone()
        }
    }

    /// The sound of a preset (bands, pre-amp, tone), keeping this one's switches and dynamics.
    pub fn with_sound_of(&self, preset: &EqSettings) -> EqSettings {
        EqSettings {
            gains: preset.gains.clone(),
            preamp: preset.preamp,
            bass: preset.bass,
            treble: preset.treble,
            ..self.clone()
        }
    }

    /// Flat curve, default tone and dynamics; on/off stays.
    pub fn reset(&self) -> EqSettings {
        EqSettings {
            enabled: self.enabled,
            ..EqSettings::default()
        }
    }

    /// Same sound as `other` (ignores on/off).
    pub fn sounds_like(&self, other: &EqSettings) -> bool {
        self.gains == other.gains
            && self.preamp == other.preamp
            && self.bass == other.bass
            && self.treble == other.treble
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EqPreset {
    pub name: String,
    pub settings: EqSettings,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportError {
    TooBig,
    Invalid,
}

impl EqPreset {
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default()
    }

    /// A preset from JSON: `{name, settings}` or bare settings (named `fallback_name`).
    pub fn from_json(text: &str, fallback_name: &str) -> Result<EqPreset, ImportError> {
        if text.len() > MAX_PRESET_BYTES {
            return Err(ImportError::TooBig);
        }
        let v: serde_json::Value = serde_json::from_str(text).map_err(|_| ImportError::Invalid)?;
        let (name, settings) = match v.get("settings") {
            Some(s) => (
                v.get("name")
                    .and_then(|n| n.as_str())
                    .unwrap_or(fallback_name),
                s.clone(),
            ),
            None => (fallback_name, v.clone()),
        };
        // The bands must be there and valid; everything else falls back to defaults.
        let gains = settings
            .get("gains")
            .and_then(|g| g.as_array())
            .ok_or(ImportError::Invalid)?;
        if !BAND_COUNTS.contains(&gains.len())
            || gains
                .iter()
                .any(|g| !g.as_f64().is_some_and(f64::is_finite))
        {
            return Err(ImportError::Invalid);
        }
        let settings: EqSettings =
            serde_json::from_value(settings).map_err(|_| ImportError::Invalid)?;
        let name = name.trim();
        Ok(EqPreset {
            name: if name.is_empty() {
                fallback_name.into()
            } else {
                name.chars().take(60).collect()
            },
            settings: settings.sanitized(),
        })
    }
}

/// Built-in presets: (translation id, 10-band curve in dB). Loss' own curves.
pub const BUILT_IN: &[(&str, [f64; 10])] = &[
    ("eq-preset-flat", [0.0; 10]),
    (
        "eq-preset-bass",
        [5.5, 4.5, 3.0, 1.5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
    ),
    (
        "eq-preset-treble",
        [0.0, 0.0, 0.0, 0.0, 0.0, 0.5, 1.5, 3.0, 4.0, 4.5],
    ),
    (
        "eq-preset-vocal",
        [-2.5, -2.0, -1.0, 0.5, 2.0, 3.0, 3.0, 1.5, 0.0, -1.0],
    ),
    (
        "eq-preset-rock",
        [4.0, 3.0, 1.0, -1.0, -1.5, 0.0, 1.5, 3.0, 3.5, 4.0],
    ),
    (
        "eq-preset-pop",
        [-1.0, 0.5, 2.5, 3.5, 2.5, 0.5, -0.5, -0.5, 1.0, 1.5],
    ),
    (
        "eq-preset-jazz",
        [2.5, 2.0, 1.0, 1.5, -1.0, -1.0, 0.0, 1.0, 2.0, 2.5],
    ),
    (
        "eq-preset-classical",
        [3.0, 2.0, 1.0, 0.0, -1.0, -1.5, -0.5, 1.0, 2.0, 3.0],
    ),
    (
        "eq-preset-electronic",
        [4.5, 4.0, 1.5, -0.5, -1.5, 0.5, 1.0, 2.5, 3.5, 4.0],
    ),
    (
        "eq-preset-acoustic",
        [3.0, 2.5, 1.5, 1.5, 2.0, 1.5, 2.5, 2.0, 1.0, 1.5],
    ),
    (
        "eq-preset-loudness",
        [5.0, 3.5, 0.0, 0.0, -1.0, 0.0, -0.5, 0.0, 3.0, 4.0],
    ),
    (
        "eq-preset-spoken",
        [-4.0, -3.0, -1.0, 1.0, 3.0, 3.5, 3.0, 1.5, -1.0, -3.0],
    ),
];

/// A built-in preset's settings (10 bands).
pub fn built_in(curve: &[f64; 10]) -> EqSettings {
    EqSettings {
        gains: curve.to_vec(),
        ..EqSettings::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_round_trip_and_import_mornye_style_json() {
        let p = EqPreset {
            name: "Mine".into(),
            settings: EqSettings {
                gains: vec![1.0, 2.0, 3.0, 4.0, 5.0],
                bass: 2.0,
                ..EqSettings::default()
            },
        };
        assert_eq!(EqPreset::from_json(&p.to_json(), "x").unwrap(), p);
        // Bare settings with camelCase keys and out-of-range values.
        let bare = r#"{"enabled":true,"gains":[20,0,0,0,0,0,0,0,0,-3],"preamp":-3,"autoHeadroom":false,"ratio":99}"#;
        let q = EqPreset::from_json(bare, "Imported").unwrap();
        assert_eq!(q.name, "Imported");
        assert_eq!(q.settings.gains[0], 15.0);
        assert!(!q.settings.auto_headroom);
        assert_eq!(q.settings.ratio, 20.0);
        // Not a preset.
        assert_eq!(
            EqPreset::from_json(r#"{"gains":[1,2,3]}"#, "x"),
            Err(ImportError::Invalid)
        );
        assert_eq!(
            EqPreset::from_json("nonsense", "x"),
            Err(ImportError::Invalid)
        );
        let huge = format!(r#"{{"name":"{}"}}"#, "a".repeat(70_000));
        assert_eq!(EqPreset::from_json(&huge, "x"), Err(ImportError::TooBig));
    }

    #[test]
    fn changing_the_band_count_keeps_the_curve() {
        let s = built_in(&BUILT_IN[1].1);
        let five = s.with_band_count(5);
        assert_eq!(five.gains.len(), 5);
        // 63 Hz sits between 62.5 Hz (4.5) and 125 Hz (3.0): about 4.5.
        assert!((five.gains[0] - 4.5).abs() < 0.11, "{:?}", five.gains);
        assert_eq!(five.gains[4], 0.0);
        let back = five.with_band_count(31);
        assert_eq!(back.gains.len(), 31);
        assert!(BUILT_IN
            .iter()
            .all(|(_, c)| c.iter().all(|g| g.abs() <= 15.0)));
    }
}
