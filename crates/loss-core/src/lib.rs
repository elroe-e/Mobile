//! Loss core: music library model, folder scanning, tags, settings,
//! synced lyrics and playlist import. No UI and no platform code.

pub mod library;
pub mod lrc;
pub mod playlist;
pub mod registry;
pub mod settings;
pub mod tags;
pub mod vault;

pub use library::{Album, Artist, Library, ScanReport, Track, TrackOrigin};
pub use settings::{Settings, SubsonicConfig};

use std::time::{SystemTime, UNIX_EPOCH};

/// File extensions Loss can decode and play.
pub const AUDIO_EXTENSIONS: &[&str] = &[
    "mp3", "flac", "ogg", "oga", "opus", "m4a", "mp4", "aac", "wav", "aif", "aiff", "alac", "caf",
    "mka", "webm",
];

pub fn is_audio_file(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| AUDIO_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Stable short id from a string (FNV-1a 64-bit, hex).
pub fn stable_id(s: &str) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}")
}

/// Lower-case, strip punctuation and collapse whitespace, for fuzzy matching.
pub fn normalize(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut space = false;
    for c in s.chars().flat_map(char::to_lowercase) {
        if c.is_alphanumeric() {
            out.push(c);
            space = false;
        } else if !space && !out.is_empty() {
            out.push(' ');
            space = true;
        }
    }
    out.trim_end().to_string()
}

/// Make a string safe to use as a single file or folder name.
pub fn sanitize_file_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    let trimmed = cleaned.trim().trim_matches('.').trim();
    let mut s: String = trimmed.chars().take(150).collect();
    if s.is_empty() {
        s = "untitled".into();
    }
    s
}

/// Format seconds as m:ss or h:mm:ss.
pub fn format_duration(secs: f64) -> String {
    if !secs.is_finite() || secs < 0.0 {
        return "0:00".into();
    }
    let t = secs.round() as u64;
    let (h, m, s) = (t / 3600, (t % 3600) / 60, t % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_works() {
        assert_eq!(normalize("  Hello, World!! (Remix) "), "hello world remix");
    }

    #[test]
    fn sanitize_works() {
        assert_eq!(sanitize_file_name("AC/DC: Back?"), "AC_DC_ Back_");
        assert_eq!(sanitize_file_name(".."), "untitled");
    }

    #[test]
    fn duration_format() {
        assert_eq!(format_duration(65.4), "1:05");
        assert_eq!(format_duration(3725.0), "1:02:05");
    }
}
