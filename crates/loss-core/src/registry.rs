//! The settings registry: one entry per user setting, describing how it is shown, found and
//! changed. The settings screen and settings search are built from it, and backup/restore and
//! the per-key database rows use the same keys (the field names of [`Settings`]).
//!
//! Adding a setting: add the field to [`Settings`], an entry to [`SETTINGS`], and a line to
//! [`get`] and [`set`]. The tests fail if a field is missing from the registry (or isn't listed in
//! [`INTERNAL`] as app state rather than a setting).
//!
//! Titles and descriptions are English for now; M0-09 turns them into translation keys.

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
    /// One of a few named values: (stored value, label).
    Choice(&'static [(&'static str, &'static str)]),
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
    Downloads,
    Sources,
    Look,
    Help,
}

impl Section {
    /// In screen order.
    pub const ALL: [Section; 5] = [
        Section::Library,
        Section::Downloads,
        Section::Sources,
        Section::Look,
        Section::Help,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Section::Library => "Music folders",
            Section::Downloads => "Downloads",
            Section::Sources => "Sources",
            Section::Look => "Look",
            Section::Help => "Help",
        }
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
    pub title: &'static str,
    pub description: &'static str,
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
    title: &'static str,
    description: &'static str,
    keywords: &'static [&'static str],
) -> SettingDef {
    SettingDef {
        key,
        section,
        kind,
        title,
        description,
        keywords,
        platforms: Platforms::ALL,
        restart: false,
    }
}

pub const ACCENTS: &[&str] = &[
    "#e8615a", "#4fb3a9", "#7c8cff", "#e3a83b", "#c56bd8", "#5fbf5f",
];

/// Every user setting, in screen order within each section.
pub static SETTINGS: &[SettingDef] = &[
    def(
        "music_folders",
        Section::Library,
        Kind::Custom,
        "Music folders",
        "Folders Loss scans for your music",
        &["library", "scan", "storage", "directory", "path"],
    ),
    def(
        "download_dir",
        Section::Downloads,
        Kind::Folder,
        "Download folder",
        "The default needs no storage permission. Files there are removed if you uninstall Loss.",
        &["save", "location", "storage", "path", "directory"],
    ),
    def(
        "max_parallel_downloads",
        Section::Downloads,
        Kind::Steps(&[1, 2, 3, 4, 5]),
        "Downloads at once",
        "",
        &["parallel", "simultaneous", "concurrent", "speed"],
    ),
    def(
        "max_retries",
        Section::Downloads,
        Kind::Steps(&[0, 2, 5, 10]),
        "Retries for a failed download",
        "How often Loss tries again after a network error",
        &["retry", "error", "network", "attempts"],
    ),
    def(
        "auto_tag",
        Section::Downloads,
        Kind::Toggle,
        "Tag downloads",
        "Add title, artist, album and cover art when missing",
        &["metadata", "tags", "cover", "artwork", "id3"],
    ),
    def(
        "fetch_lyrics",
        Section::Downloads,
        Kind::Toggle,
        "Fetch lyrics",
        "Save synced lyrics from LRCLIB next to each download",
        &["lrc", "lrclib", "words", "karaoke", "synced"],
    ),
    def(
        "extract_zips",
        Section::Downloads,
        Kind::Toggle,
        "Unpack zip albums",
        "Extract songs from .zip downloads (e.g. Bandcamp purchases)",
        &["zip", "archive", "bandcamp", "unzip", "extract"],
    ),
    def(
        "archive_cc_only",
        Section::Sources,
        Kind::Toggle,
        "Internet Archive: Creative Commons only",
        "Hide items without an open licence",
        &["license", "licence", "free", "archive.org", "legal"],
    ),
    def(
        "jamendo_client_id",
        Section::Sources,
        Kind::Text {
            placeholder: "Free from devportal.jamendo.com",
        },
        "Jamendo client id",
        "Needed to search Jamendo's free music",
        &["api", "key", "jamendo"],
    ),
    def(
        "subsonic",
        Section::Sources,
        Kind::Custom,
        "My server (Subsonic / Navidrome)",
        "Stream and download from your own music server",
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
        Kind::Choice(&[("system", "Auto"), ("dark", "Dark"), ("light", "Light")]),
        "Theme",
        "",
        &["dark mode", "light mode", "night", "appearance"],
    ),
    def(
        "dynamic_color",
        Section::Look,
        Kind::Toggle,
        "Colours from album art",
        "The whole app takes on the colours of what's playing",
        &["dynamic", "colour", "color", "cover", "material you"],
    ),
    def(
        "accent",
        Section::Look,
        Kind::Color(ACCENTS),
        "Accent colour",
        "Used when nothing is playing, or always if colours from album art are off",
        &["color", "colour", "theme", "tint"],
    ),
    def(
        "detailed_logging",
        Section::Help,
        Kind::Toggle,
        "Detailed logging",
        "Record more detail. Turn on when reporting a problem.",
        &["debug", "log", "bug", "report", "diagnostics"],
    ),
];

/// Fields of [`Settings`] that are app state, not settings: they aren't shown in the settings
/// screen or found by search.
pub const INTERNAL: &[&str] = &["volume", "last_track", "last_position", "podcasts"];

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
        (Kind::Choice(opts), Value::Text(v)) => opts.iter().any(|(o, _)| o == v),
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
            let title = normalize(d.title);
            let mut hay = format!(
                "{title} {} {} {}",
                normalize(d.description),
                normalize(&d.keywords.join(" ")),
                normalize(d.section.title())
            );
            if let Kind::Choice(opts) = &d.kind {
                for (_, label) in opts.iter() {
                    hay.push(' ');
                    hay.push_str(&normalize(label));
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
                Kind::Choice(o) => Value::Text(o.last().unwrap().0.into()),
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
        // Choice labels are searchable.
        assert_eq!(keys("light"), vec!["theme"]);
    }

    #[test]
    fn every_section_has_settings() {
        for s in Section::ALL {
            assert!(SETTINGS.iter().any(|d| d.section == s), "{s:?} is empty");
        }
    }
}
