//! User settings, stored as JSON in the app's private data folder.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct SubsonicConfig {
    /// e.g. https://music.example.com
    pub url: String,
    pub username: String,
    /// Lives in the encrypted vault, never in the settings file (it is filled
    /// in when the app reads settings). Used to build salted tokens; never
    /// sent in clear text.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub password: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub music_folders: Vec<PathBuf>,
    pub download_dir: PathBuf,
    pub max_parallel_downloads: usize,
    pub max_retries: u32,
    /// Add tags, cover art and lyrics to downloaded files.
    pub auto_tag: bool,
    pub fetch_lyrics: bool,
    /// Extract audio files from downloaded .zip archives (e.g. Bandcamp purchases).
    pub extract_zips: bool,
    /// Only show Creative Commons / public domain results from the Internet Archive.
    pub archive_cc_only: bool,
    /// Free key from https://devportal.jamendo.com
    pub jamendo_client_id: String,
    pub subsonic: Option<SubsonicConfig>,
    pub podcasts: Vec<String>,
    pub volume: f32,
    pub accent: String,
    /// "system", "dark" or "light".
    pub theme: String,
    /// Colour the app from the playing song's cover art.
    pub dynamic_color: bool,
    /// For "Continue listening" on Home.
    pub last_track: Option<String>,
    pub last_position: f64,
    /// Record debug-level lines in the log (Settings → Logs).
    pub detailed_logging: bool,
    /// "system" (the phone's language) or a language tag such as "en".
    pub language: String,
    /// The Home notice about background limits was dismissed.
    pub reliability_notice_dismissed: bool,
    /// Smallest album-cover width in the library grid (px); pinch changes it.
    pub album_grid_size: u32,
    /// Sample-rate conversion quality: "best", "high", "medium" or "fast".
    pub resample_quality: String,
    /// Highest output sample rate in Hz; 0 for no limit.
    pub max_sample_rate: u32,
    /// Output sample format: "auto", "float", "16", "24" or "32".
    pub output_format: String,
    /// Output mode: "standard", "hires", "bitperfect", "exclusive" or "usb" (M2-04).
    pub output_mode: String,
    /// Low-latency output (smaller buffers) instead of power saving.
    pub low_latency_output: bool,
    /// Let Android's spatializer process music.
    pub spatial_audio: bool,
    /// Volume levelling from ReplayGain/R128 tags (M2-09).
    pub normalization: bool,
    /// "auto", "track" or "album".
    pub normalization_mode: String,
    /// Pre-amp in dB (≤ 0; shifts the −18 LUFS reference).
    pub normalization_preamp: i64,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            music_folders: vec![],
            download_dir: PathBuf::new(),
            max_parallel_downloads: 3,
            max_retries: 5,
            auto_tag: true,
            fetch_lyrics: true,
            extract_zips: true,
            archive_cc_only: true,
            jamendo_client_id: String::new(),
            subsonic: None,
            podcasts: vec![],
            volume: 1.0,
            accent: "#e8615a".into(),
            theme: "system".into(),
            dynamic_color: true,
            last_track: None,
            last_position: 0.0,
            detailed_logging: false,
            language: "system".into(),
            reliability_notice_dismissed: false,
            album_grid_size: 150,
            resample_quality: "best".into(),
            max_sample_rate: 0,
            output_format: "auto".into(),
            output_mode: "hires".into(),
            low_latency_output: false,
            spatial_audio: false,
            normalization: false,
            normalization_mode: "auto".into(),
            normalization_preamp: 0,
        }
    }
}

impl Settings {
    pub fn load(path: &Path) -> Option<Settings> {
        let bytes = std::fs::read(path).ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(tmp, path)
    }
}
