//! Loss downloads: a resumable, parallel download manager plus search
//! sources that only offer music you're allowed to download:
//! the Internet Archive, Jamendo, podcasts, your own Subsonic/Navidrome
//! server, and direct links (e.g. Bandcamp purchase downloads).

pub mod http;
pub mod lyrics;
pub mod manager;
pub mod postprocess;
pub mod sources;

pub use manager::{DownloadManager, JobSnapshot, JobStatus, ManagerConfig};
pub use sources::{Collection, SearchItem, SourceKind};

use loss_core::tags::TrackMeta;
use serde::{Deserialize, Serialize};

/// Everything needed to download one file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DownloadRequest {
    pub url: String,
    /// Relative path inside the download folder, e.g. "Artist/Album/01 Title.flac".
    pub rel_path: String,
    #[serde(default)]
    pub meta: TrackMeta,
    pub cover_url: Option<String>,
    pub source: SourceKind,
    #[serde(default)]
    pub headers: Vec<(String, String)>,
}

impl DownloadRequest {
    /// Build "Artist/Album/NN Title.ext" from metadata.
    pub fn build_rel_path(meta: &TrackMeta, fallback_name: &str, ext: &str) -> String {
        use loss_core::sanitize_file_name as s;
        let artist = meta
            .album_artist
            .clone()
            .or_else(|| meta.artist.clone())
            .unwrap_or_else(|| "Unknown Artist".into());
        let album = meta.album.clone().unwrap_or_else(|| "Singles".into());
        let title = meta.title.clone().unwrap_or_else(|| fallback_name.to_string());
        let name = match meta.track_no {
            Some(n) => format!("{n:02} {title}"),
            None => title,
        };
        let ext = ext.trim_start_matches('.');
        if ext.is_empty() {
            format!("{}/{}/{}", s(&artist), s(&album), s(&name))
        } else {
            format!("{}/{}/{}.{}", s(&artist), s(&album), s(&name), ext)
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("network error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Other(String),
}

impl From<String> for Error {
    fn from(s: String) -> Self {
        Error::Other(s)
    }
}

impl From<&str> for Error {
    fn from(s: &str) -> Self {
        Error::Other(s.to_string())
    }
}

pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rel_path() {
        let meta = TrackMeta {
            title: Some("Song: One".into()),
            artist: Some("AC/DC".into()),
            album: Some("Live".into()),
            track_no: Some(3),
            ..Default::default()
        };
        assert_eq!(
            DownloadRequest::build_rel_path(&meta, "x", "flac"),
            "AC_DC/Live/03 Song_ One.flac"
        );
    }
}
