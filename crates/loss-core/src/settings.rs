//! User settings, stored as JSON in the app's private data folder.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct SubsonicConfig {
    /// e.g. https://music.example.com
    pub url: String,
    pub username: String,
    /// Stored in the app's private storage. Used to build salted tokens;
    /// never sent in clear text.
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
