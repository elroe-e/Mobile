//! The settings registry: one entry per user setting, describing how it is shown, found and
//! changed. The settings screen and settings search are built from it, and backup/restore and
//! the per-key database rows use the same keys (the field names of [`Settings`]).
//!
//! Adding a setting: add the field to [`Settings`], an entry to [`SETTINGS`], and a line to
//! [`get`] and [`set`]. The tests fail if a field is missing from the registry (or isn't listed in
//! [`INTERNAL`] as app state rather than a setting).
//!
//! Text comes from the translations (crates/loss-i18n): `setting-<key>` (title),
//! `setting-<key>-desc` (optional description), `section-<name>` and `choice-<key>-<value>`.
//! The tests check that English has every one.

use crate::{normalize, Settings};
use std::path::PathBuf;

/// A setting's value, independent of its Rust type.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Bool(bool),
    Int(i64),
    Text(String),
}

/// How a setting is shown and what values it accepts.
#[derive(Debug, PartialEq)]
pub enum Kind {
    /// On/off switch.
    Toggle,
    /// One of a few named values; each has the label `choice-<key>-<value>`.
    Choice(&'static [&'static str]),
    /// One of a few numbers, shown as segments.
    Steps(&'static [i64]),
    /// Free text.
    Text { placeholder: &'static str },
    /// A folder path, typed in.
    Folder,
    /// One of these colours (`#rrggbb`).
    Color(&'static [&'static str]),
    /// Drawn by its own widget (lists, the server form); [`get`]/[`set`] don't handle it.
    Custom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Section {
    Library,
    Audio,
    Downloads,
    Sources,
    Look,
    Help,
}

impl Section {
    /// In screen order.
    pub const ALL: [Section; 6] = [
        Section::Library,
        Section::Audio,
        Section::Downloads,
        Section::Sources,
        Section::Look,
        Section::Help,
    ];

    fn id(self) -> &'static str {
        match self {
            Section::Library => "section-library",
            Section::Audio => "section-audio",
            Section::Downloads => "section-downloads",
            Section::Sources => "section-sources",
            Section::Look => "section-look",
            Section::Help => "section-help",
        }
    }

    pub fn title(self) -> String {
        loss_i18n::tr(self.id())
    }
}

/// Where a setting applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Platforms(u8);

impl Platforms {
    pub const ANDROID: Platforms = Platforms(1);
    pub const DESKTOP: Platforms = Platforms(2);
    pub const ALL: Platforms = Platforms(3);

    pub fn includes(self, p: Platforms) -> bool {
        self.0 & p.0 != 0
    }

    /// The platform this build runs on.
    pub fn current() -> Platforms {
        if cfg!(target_os = "android") {
            Platforms::ANDROID
        } else {
            Platforms::DESKTOP
        }
    }
}

#[derive(Debug, PartialEq)]
pub struct SettingDef {
    /// The field name in [`Settings`] (and the database key).
    pub key: &'static str,
    pub section: Section,
    pub kind: Kind,
    /// Extra words people might search for.
    pub keywords: &'static [&'static str],
    pub platforms: Platforms,
    /// The change takes effect after restarting the app.
    pub restart: bool,
}

const fn def(
    key: &'static str,
    section: Section,
    kind: Kind,
    keywords: &'static [&'static str],
) -> SettingDef {
    SettingDef {
        key,
        section,
        kind,
        keywords,
        platforms: Platforms::ALL,
        restart: false,
    }
}

/// A setting that only does something on Android.
const fn android(def: SettingDef) -> SettingDef {
    SettingDef {
        platforms: Platforms::ANDROID,
        ..def
    }
}

impl SettingDef {
    pub fn title(&self) -> String {
        loss_i18n::tr(&format!("setting-{}", self.key))
    }

    /// Empty when the setting has no description.
    pub fn description(&self) -> String {
        loss_i18n::tr_opt(&format!("setting-{}-desc", self.key)).unwrap_or_default()
    }

    /// The label of one of a [`Kind::Choice`]'s values.
    pub fn choice_label(&self, value: &str) -> String {
        loss_i18n::tr(&format!("choice-{}-{value}", self.key))
    }

    /// A choice's explanation (`choice-<key>-<value>-desc`), empty when it has none.
    pub fn choice_description(&self, value: &str) -> String {
        loss_i18n::tr_opt(&format!("choice-{}-{value}-desc", self.key)).unwrap_or_default()
    }
}

/// Languages offered in Settings, the phone's own first. Debug builds add the pseudo-locales
/// (accented and right-to-left) used to test translations and layout.
#[cfg(not(debug_assertions))]
pub const LANGUAGES: &[&str] = &["system", "en"];
#[cfg(debug_assertions)]
pub const LANGUAGES: &[&str] = &["system", "en", "en-XA", "ar-XB"];

pub const ACCENTS: &[&str] = &[
    "#e8615a", "#4fb3a9", "#7c8cff", "#e3a83b", "#c56bd8", "#5fbf5f",
];

/// Every user setting, in screen order within each section.
pub static SETTINGS: &[SettingDef] = &[
    def(
        "music_folders",
        Section::Library,
        Kind::Custom,
        &["library", "scan", "storage", "directory", "path"],
    ),
    def(
        "resample_quality",
        Section::Audio,
        Kind::Choice(&["best", "high", "medium", "fast"]),
        &[
            "resample",
            "sample rate",
            "conversion",
            "quality",
            "44.1",
            "48",
            "hi-res",
            "src",
        ],
    ),
    def(
        "max_sample_rate",
        Section::Audio,
        Kind::Choice(&["none", "48000", "96000", "192000"]),
        &["sample rate", "limit", "hi-res", "khz", "battery"],
    ),
    def(
        "output_mode",
        Section::Audio,
        Kind::Choice(&["standard", "hires", "bitperfect", "exclusive", "usb"]),
        &[
            "bit-perfect",
            "bitperfect",
            "exclusive",
            "mmap",
            "usb",
            "dac",
            "hi-res",
            "engine",
            "mixer",
            "resample",
        ],
    ),
    def(
        "normalization",
        Section::Audio,
        Kind::Toggle,
        &[
            "replaygain",
            "loudness",
            "normalize",
            "volume",
            "level",
            "r128",
            "lufs",
        ],
    ),
    def(
        "normalization_mode",
        Section::Audio,
        Kind::Choice(&["auto", "track", "album"]),
        &["replaygain", "album gain", "track gain"],
    ),
    def(
        "normalization_preamp",
        Section::Audio,
        Kind::Steps(&[-12, -9, -6, -3, 0]),
        &["replaygain", "pre-amp", "preamp", "loudness", "lufs"],
    ),
    def(
        "crossfeed",
        Section::Audio,
        Kind::Choice(&["off", "light", "medium", "strong"]),
        &[
            "headphones",
            "binaural",
            "bs2b",
            "stereo",
            "speakers",
            "fatigue",
        ],
    ),
    def(
        "skip_silence",
        Section::Audio,
        Kind::Toggle,
        &["silence", "gap", "quiet", "skip", "pause", "hidden track"],
    ),
    def(
        "pause_on_mute",
        Section::Audio,
        Kind::Toggle,
        &["mute", "silent", "volume", "zero", "pause"],
    ),
    def(
        "resume_on_connect",
        Section::Audio,
        Kind::Choice(&["off", "after_disconnect", "always"]),
        &[
            "headphones",
            "car",
            "bluetooth",
            "connect",
            "resume",
            "plug",
            "usb",
        ],
    ),
    def(
        "loud_volume_limit",
        Section::Audio,
        Kind::Steps(&[0, 60, 70, 80, 90]),
        &["loud", "volume", "warning", "hearing", "safe", "limit"],
    ),
    def(
        "hide_volume",
        Section::Audio,
        Kind::Toggle,
        &["volume", "slider", "hide", "player"],
    ),
    def(
        "auto_mix",
        Section::Audio,
        Kind::Toggle,
        &[
            "crossfade",
            "automix",
            "dj",
            "transition",
            "fade",
            "overlap",
        ],
    ),
    def(
        "auto_mix_duration",
        Section::Audio,
        Kind::Steps(&[0, 2, 4, 6, 8, 12, 16]),
        &["crossfade", "automix", "length", "seconds", "fade"],
    ),
    def(
        "auto_mix_effect",
        Section::Audio,
        Kind::Choice(&["auto", "crossfade", "muffled", "echo", "pitch", "custom"]),
        &["automix", "crossfade", "echo", "muffled", "transition"],
    ),
    def(
        "auto_mix_beat",
        Section::Audio,
        Kind::Toggle,
        &["automix", "beat", "tempo", "bpm", "sync", "dj"],
    ),
    def(
        "auto_mix_speed",
        Section::Audio,
        Kind::Steps(&[50, 75, 90, 100, 110, 125, 150]),
        &["automix", "custom", "speed", "slow down"],
    ),
    def(
        "auto_mix_pitch",
        Section::Audio,
        Kind::Steps(&[-12, -7, -5, -2, 0, 2, 5, 7, 12]),
        &["automix", "custom", "pitch", "semitone"],
    ),
    def(
        "auto_mix_echo",
        Section::Audio,
        Kind::Toggle,
        &["automix", "custom", "echo", "reverb"],
    ),
    def(
        "auto_mix_low_pass",
        Section::Audio,
        Kind::Toggle,
        &["automix", "custom", "muffled", "low-pass", "filter"],
    ),
    def(
        "playback_speed",
        Section::Audio,
        Kind::Steps(&[50, 75, 90, 100, 110, 125, 150, 200]),
        &[
            "speed",
            "tempo",
            "faster",
            "slower",
            "podcast",
            "audiobook",
            "rate",
        ],
    ),
    def(
        "playback_keep_pitch",
        Section::Audio,
        Kind::Toggle,
        &["pitch", "speed", "tempo", "chipmunk", "time stretch"],
    ),
    def(
        "playback_pitch",
        Section::Audio,
        Kind::Steps(&[-12, -7, -5, -2, -1, 0, 1, 2, 5, 7, 12]),
        &["pitch", "semitone", "key", "transpose", "karaoke"],
    ),
    android(def(
        "output_format",
        Section::Audio,
        Kind::Choice(&["auto", "float", "16", "24", "32"]),
        &[
            "bit depth",
            "bits",
            "24-bit",
            "16-bit",
            "float",
            "pcm",
            "dither",
            "dac",
        ],
    )),
    android(def(
        "low_latency_output",
        Section::Audio,
        Kind::Toggle,
        &["latency", "buffer", "battery", "performance"],
    )),
    android(def(
        "spatial_audio",
        Section::Audio,
        Kind::Toggle,
        &["spatial", "3d", "surround", "virtualizer", "head tracking"],
    )),
    def(
        "download_dir",
        Section::Downloads,
        Kind::Folder,
        &["save", "location", "storage", "path", "directory"],
    ),
    def(
        "max_parallel_downloads",
        Section::Downloads,
        Kind::Steps(&[1, 2, 3, 4, 5]),
        &["parallel", "simultaneous", "concurrent", "speed"],
    ),
    def(
        "max_retries",
        Section::Downloads,
        Kind::Steps(&[0, 2, 5, 10]),
        &["retry", "error", "network", "attempts"],
    ),
    def(
        "auto_tag",
        Section::Downloads,
        Kind::Toggle,
        &["metadata", "tags", "cover", "artwork", "id3"],
    ),
    def(
        "fetch_lyrics",
        Section::Downloads,
        Kind::Toggle,
        &["lrc", "lrclib", "words", "karaoke", "synced"],
    ),
    def(
        "extract_zips",
        Section::Downloads,
        Kind::Toggle,
        &["zip", "archive", "bandcamp", "unzip", "extract"],
    ),
    def(
        "archive_cc_only",
        Section::Sources,
        Kind::Toggle,
        &["license", "licence", "free", "archive.org", "legal"],
    ),
    def(
        "jamendo_client_id",
        Section::Sources,
        Kind::Text {
            placeholder: "Free from devportal.jamendo.com",
        },
        &["api", "key", "jamendo"],
    ),
    def(
        "subsonic",
        Section::Sources,
        Kind::Custom,
        &[
            "navidrome",
            "subsonic",
            "server",
            "self-hosted",
            "airsonic",
            "gonic",
        ],
    ),
    def(
        "theme",
        Section::Look,
        Kind::Choice(&["system", "dark", "light"]),
        &["dark mode", "light mode", "night", "appearance"],
    ),
    def(
        "language",
        Section::Look,
        Kind::Choice(LANGUAGES),
        &["language", "translation", "locale", "english"],
    ),
    def(
        "dynamic_color",
        Section::Look,
        Kind::Toggle,
        &["dynamic", "colour", "color", "cover", "material you"],
    ),
    def(
        "accent",
        Section::Look,
        Kind::Color(ACCENTS),
        &["color", "colour", "theme", "tint"],
    ),
    def(
        "detailed_logging",
        Section::Help,
        Kind::Toggle,
        &["debug", "log", "bug", "report", "diagnostics"],
    ),
];

/// Fields of [`Settings`] that are app state, not settings: they aren't shown in the settings
/// screen or found by search.
pub const INTERNAL: &[&str] = &[
    "volume",
    "last_track",
    "last_position",
    "podcasts",
    "reliability_notice_dismissed",
    "album_grid_size",
    // Edited on the Equalizer screen.
    "equalizer",
    "eq_presets",
    "autoeq",
];

pub fn find(key: &str) -> Option<&'static SettingDef> {
    SETTINGS.iter().find(|d| d.key == key)
}

/// Settings of `section` that apply on this platform.
pub fn in_section(section: Section) -> impl Iterator<Item = &'static SettingDef> {
    let here = Platforms::current();
    SETTINGS
        .iter()
        .filter(move |d| d.section == section && d.platforms.includes(here))
}

/// The current value of a setting ([`Kind::Custom`] settings return None).
pub fn get(s: &Settings, key: &str) -> Option<Value> {
    use Value::*;
    Some(match key {
        "download_dir" => Text(s.download_dir.to_string_lossy().into_owned()),
        "max_parallel_downloads" => Int(s.max_parallel_downloads as i64),
        "max_retries" => Int(s.max_retries as i64),
        "auto_tag" => Bool(s.auto_tag),
        "fetch_lyrics" => Bool(s.fetch_lyrics),
        "extract_zips" => Bool(s.extract_zips),
        "archive_cc_only" => Bool(s.archive_cc_only),
        "jamendo_client_id" => Text(s.jamendo_client_id.clone()),
        "theme" => Text(s.theme.clone()),
        "dynamic_color" => Bool(s.dynamic_color),
        "accent" => Text(s.accent.clone()),
        "detailed_logging" => Bool(s.detailed_logging),
        "language" => Text(s.language.clone()),
        "resample_quality" => Text(s.resample_quality.clone()),
        "max_sample_rate" => Text(match s.max_sample_rate {
            0 => "none".into(),
            r => r.to_string(),
        }),
        "output_format" => Text(s.output_format.clone()),
        "output_mode" => Text(s.output_mode.clone()),
        "low_latency_output" => Bool(s.low_latency_output),
        "spatial_audio" => Bool(s.spatial_audio),
        "crossfeed" => Text(s.crossfeed.clone()),
        "skip_silence" => Bool(s.skip_silence),
        "pause_on_mute" => Bool(s.pause_on_mute),
        "resume_on_connect" => Text(s.resume_on_connect.clone()),
        "loud_volume_limit" => Int(s.loud_volume_limit),
        "hide_volume" => Bool(s.hide_volume),
        "auto_mix" => Bool(s.auto_mix),
        "auto_mix_duration" => Int(s.auto_mix_duration),
        "auto_mix_effect" => Text(s.auto_mix_effect.clone()),
        "auto_mix_beat" => Bool(s.auto_mix_beat),
        "auto_mix_speed" => Int(s.auto_mix_speed),
        "auto_mix_pitch" => Int(s.auto_mix_pitch),
        "auto_mix_echo" => Bool(s.auto_mix_echo),
        "auto_mix_low_pass" => Bool(s.auto_mix_low_pass),
        "playback_speed" => Int(s.playback_speed),
        "playback_keep_pitch" => Bool(s.playback_keep_pitch),
        "playback_pitch" => Int(s.playback_pitch),
        "normalization" => Bool(s.normalization),
        "normalization_mode" => Text(s.normalization_mode.clone()),
        "normalization_preamp" => Int(s.normalization_preamp),
        _ => return None,
    })
}

#[derive(Debug, PartialEq)]
pub enum SetError {
    UnknownKey,
    /// The value's type doesn't fit the setting, or it isn't one of the allowed values.
    Invalid,
}

/// Changes a setting after checking the value against its [`Kind`].
pub fn set(s: &mut Settings, key: &str, value: Value) -> Result<(), SetError> {
    let d = find(key).ok_or(SetError::UnknownKey)?;
    let ok = match (&d.kind, &value) {
        (Kind::Toggle, Value::Bool(_)) => true,
        (Kind::Choice(opts), Value::Text(v)) => opts.contains(&v.as_str()),
        (Kind::Steps(opts), Value::Int(v)) => opts.contains(v),
        (Kind::Text { .. }, Value::Text(_)) | (Kind::Folder, Value::Text(_)) => true,
        (Kind::Color(opts), Value::Text(v)) => opts.contains(&v.as_str()),
        _ => false,
    };
    if !ok {
        return Err(SetError::Invalid);
    }
    match (key, value) {
        ("download_dir", Value::Text(v)) => s.download_dir = PathBuf::from(v.trim()),
        ("max_parallel_downloads", Value::Int(v)) => s.max_parallel_downloads = v as usize,
        ("max_retries", Value::Int(v)) => s.max_retries = v as u32,
        ("auto_tag", Value::Bool(v)) => s.auto_tag = v,
        ("fetch_lyrics", Value::Bool(v)) => s.fetch_lyrics = v,
        ("extract_zips", Value::Bool(v)) => s.extract_zips = v,
        ("archive_cc_only", Value::Bool(v)) => s.archive_cc_only = v,
        ("jamendo_client_id", Value::Text(v)) => s.jamendo_client_id = v.trim().to_string(),
        ("theme", Value::Text(v)) => s.theme = v,
        ("dynamic_color", Value::Bool(v)) => s.dynamic_color = v,
        ("accent", Value::Text(v)) => s.accent = v,
        ("detailed_logging", Value::Bool(v)) => s.detailed_logging = v,
        ("language", Value::Text(v)) => s.language = v,
        ("resample_quality", Value::Text(v)) => s.resample_quality = v,
        ("max_sample_rate", Value::Text(v)) => s.max_sample_rate = v.parse().unwrap_or(0),
        ("output_format", Value::Text(v)) => s.output_format = v,
        ("output_mode", Value::Text(v)) => s.output_mode = v,
        ("low_latency_output", Value::Bool(v)) => s.low_latency_output = v,
        ("spatial_audio", Value::Bool(v)) => s.spatial_audio = v,
        ("crossfeed", Value::Text(v)) => s.crossfeed = v,
        ("skip_silence", Value::Bool(v)) => s.skip_silence = v,
        ("pause_on_mute", Value::Bool(v)) => s.pause_on_mute = v,
        ("resume_on_connect", Value::Text(v)) => s.resume_on_connect = v,
        ("loud_volume_limit", Value::Int(v)) => s.loud_volume_limit = v.clamp(0, 100),
        ("hide_volume", Value::Bool(v)) => s.hide_volume = v,
        ("auto_mix", Value::Bool(v)) => s.auto_mix = v,
        ("auto_mix_duration", Value::Int(v)) => s.auto_mix_duration = v.clamp(0, 20),
        ("auto_mix_effect", Value::Text(v)) => s.auto_mix_effect = v,
        ("auto_mix_beat", Value::Bool(v)) => s.auto_mix_beat = v,
        ("auto_mix_speed", Value::Int(v)) => s.auto_mix_speed = v.clamp(25, 200),
        ("auto_mix_pitch", Value::Int(v)) => s.auto_mix_pitch = v.clamp(-12, 12),
        ("auto_mix_echo", Value::Bool(v)) => s.auto_mix_echo = v,
        ("auto_mix_low_pass", Value::Bool(v)) => s.auto_mix_low_pass = v,
        ("playback_speed", Value::Int(v)) => s.playback_speed = v.clamp(25, 400),
        ("playback_keep_pitch", Value::Bool(v)) => s.playback_keep_pitch = v,
        ("playback_pitch", Value::Int(v)) => s.playback_pitch = v.clamp(-12, 12),
        ("normalization", Value::Bool(v)) => s.normalization = v,
        ("normalization_mode", Value::Text(v)) => s.normalization_mode = v,
        ("normalization_preamp", Value::Int(v)) => s.normalization_preamp = v.clamp(-24, 0),
        _ => return Err(SetError::Invalid),
    }
    Ok(())
}

/// Settings matching `query` (every word must appear in the title, description, keywords,
/// section or choice labels), best matches first: title matches before others. Only settings
/// for this platform.
pub fn search(query: &str) -> Vec<&'static SettingDef> {
    let words: Vec<String> = normalize(query)
        .split_whitespace()
        .map(str::to_string)
        .collect();
    if words.is_empty() {
        return Vec::new();
    }
    let here = Platforms::current();
    let mut hits: Vec<(u8, usize, &'static SettingDef)> = SETTINGS
        .iter()
        .enumerate()
        .filter(|(_, d)| d.platforms.includes(here))
        .filter_map(|(i, d)| {
            let title = normalize(&d.title());
            let mut hay = format!(
                "{title} {} {} {}",
                normalize(&d.description()),
                normalize(&d.keywords.join(" ")),
                normalize(&d.section.title())
            );
            if let Kind::Choice(opts) = &d.kind {
                for v in opts.iter() {
                    hay.push(' ');
                    hay.push_str(&normalize(&d.choice_label(v)));
                }
            }
            if !words.iter().all(|w| hay.contains(w.as_str())) {
                return None;
            }
            let rank = if words.iter().all(|w| title.contains(w.as_str())) {
                0
            } else {
                1
            };
            Some((rank, i, d))
        })
        .collect();
    hits.sort_by_key(|(rank, i, _)| (*rank, *i));
    hits.into_iter().map(|(_, _, d)| d).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fields() -> Vec<String> {
        match serde_json::to_value(Settings::default()).unwrap() {
            serde_json::Value::Object(m) => m.keys().cloned().collect(),
            _ => unreachable!(),
        }
    }

    #[test]
    fn every_field_is_registered_or_internal() {
        for f in fields() {
            assert!(
                find(&f).is_some() || INTERNAL.contains(&f.as_str()),
                "Settings::{f} is neither in the registry nor in INTERNAL"
            );
        }
        // `subsonic` is None by default, so it isn't serialised; check it explicitly.
        assert!(find("subsonic").is_some());
    }

    #[test]
    fn every_key_is_a_field_and_unique() {
        let f = fields();
        for d in SETTINGS {
            assert!(
                f.contains(&d.key.to_string()) || d.key == "subsonic",
                "{} is not a Settings field",
                d.key
            );
            assert_eq!(SETTINGS.iter().filter(|e| e.key == d.key).count(), 1);
        }
    }

    #[test]
    fn get_and_set_cover_every_plain_setting() {
        let mut s = Settings::default();
        for d in SETTINGS {
            if d.kind == Kind::Custom {
                assert_eq!(get(&s, d.key), None);
                continue;
            }
            let v = get(&s, d.key).unwrap_or_else(|| panic!("get misses {}", d.key));
            // Setting a valid value round-trips.
            let valid = match &d.kind {
                Kind::Toggle => Value::Bool(true),
                Kind::Choice(o) => Value::Text(o.last().unwrap().to_string()),
                Kind::Steps(o) => Value::Int(*o.last().unwrap()),
                Kind::Text { .. } | Kind::Folder => Value::Text("x".into()),
                Kind::Color(o) => Value::Text(o.last().unwrap().to_string()),
                Kind::Custom => unreachable!(),
            };
            set(&mut s, d.key, valid.clone()).unwrap_or_else(|e| panic!("{}: {e:?}", d.key));
            assert_eq!(get(&s, d.key), Some(valid), "{}", d.key);
            // The default value is valid for its kind (except free folders/text).
            assert!(
                set(&mut s.clone(), d.key, v).is_ok(),
                "default of {}",
                d.key
            );
        }
    }

    #[test]
    fn invalid_values_are_refused() {
        let mut s = Settings::default();
        assert_eq!(
            set(&mut s, "theme", Value::Text("purple".into())),
            Err(SetError::Invalid)
        );
        assert_eq!(
            set(&mut s, "auto_tag", Value::Int(1)),
            Err(SetError::Invalid)
        );
        assert_eq!(
            set(&mut s, "max_parallel_downloads", Value::Int(50)),
            Err(SetError::Invalid)
        );
        assert_eq!(
            set(&mut s, "nope", Value::Bool(true)),
            Err(SetError::UnknownKey)
        );
        assert_eq!(
            set(&mut s, "subsonic", Value::Text("x".into())),
            Err(SetError::Invalid)
        );
        assert_eq!(s, Settings::default());
    }

    #[test]
    fn search_finds_settings_by_keyword() {
        let keys = |q: &str| search(q).iter().map(|d| d.key).collect::<Vec<_>>();
        assert_eq!(keys("dark mode"), vec!["theme"]);
        assert_eq!(keys("lyrics"), vec!["fetch_lyrics"]);
        assert!(keys("navidrome").contains(&"subsonic"));
        assert!(keys("Bandcamp").contains(&"extract_zips"));
        // Title matches come first.
        assert_eq!(keys("colour")[0], "dynamic_color");
        assert_eq!(keys("accent colour")[0], "accent");
        // All words must match.
        assert!(keys("lyrics zip").is_empty());
        assert!(keys("   ").is_empty());
        // Choice labels are searchable (the theme's Light, and Crossfeed's).
        let light = keys("light");
        assert!(
            light.contains(&"theme") && light.contains(&"crossfeed"),
            "{light:?}"
        );
    }

    #[test]
    fn every_text_is_translated() {
        for d in SETTINGS {
            assert!(
                loss_i18n::has(&format!("setting-{}", d.key)),
                "setting-{} missing",
                d.key
            );
            if let Kind::Choice(opts) = &d.kind {
                for v in opts.iter() {
                    let id = format!("choice-{}-{v}", d.key);
                    assert!(loss_i18n::has(&id), "{id} missing");
                }
            }
        }
        for s in Section::ALL {
            assert!(loss_i18n::has(s.id()), "{} missing", s.id());
        }
    }

    #[test]
    fn every_section_has_settings() {
        for s in Section::ALL {
            assert!(SETTINGS.iter().any(|d| d.section == s), "{s:?} is empty");
        }
    }
}
