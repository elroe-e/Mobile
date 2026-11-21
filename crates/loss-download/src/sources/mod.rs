//! Where Loss can find music to download. Every source here offers files
//! that you are allowed to download: Creative Commons / public domain
//! catalogues, podcasts, your own server, and links you provide.

pub mod archive;
pub mod direct;
pub mod jamendo;
pub mod podcast;
pub mod subsonic;

use crate::DownloadRequest;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum SourceKind {
    InternetArchive,
    Jamendo,
    Podcast,
    Subsonic,
    #[default]
    Direct,
}

impl SourceKind {
    pub fn label(self) -> &'static str {
        match self {
            SourceKind::InternetArchive => "Internet Archive",
            SourceKind::Jamendo => "Jamendo",
            SourceKind::Podcast => "Podcasts",
            SourceKind::Subsonic => "My server",
            SourceKind::Direct => "Direct link",
        }
    }
}

/// Something that contains more items: an Internet Archive item, a Subsonic
/// album, a podcast feed.
#[derive(Debug, Clone, PartialEq)]
pub struct Collection {
    pub source: SourceKind,
    pub id: String,
    pub title: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ItemAction {
    /// Ready to download.
    Download(Box<DownloadRequest>),
    /// Open to see what's inside.
    Open(Collection),
}

#[derive(Debug, Clone, PartialEq)]
pub struct SearchItem {
    pub title: String,
    pub subtitle: String,
    pub cover_url: Option<String>,
    /// Licence name or URL, shown to the user.
    pub license: Option<String>,
    /// e.g. "FLAC", "MP3 320"
    pub format: Option<String>,
    pub duration: Option<f64>,
    pub action: ItemAction,
}

impl SearchItem {
    pub fn request(&self) -> Option<&DownloadRequest> {
        match &self.action {
            ItemAction::Download(r) => Some(r),
            ItemAction::Open(_) => None,
        }
    }
}

/// Short human name for a Creative Commons licence URL.
pub fn license_label(url: &str) -> String {
    let u = url.to_lowercase();
    if u.contains("publicdomain/zero") {
        return "CC0".into();
    }
    if u.contains("publicdomain") {
        return "Public domain".into();
    }
    if let Some(i) = u.find("/licenses/") {
        let rest = &u[i + 10..];
        let mut parts = rest.split('/');
        let kind = parts.next().unwrap_or("");
        let ver = parts.next().unwrap_or("");
        if !kind.is_empty() {
            return format!("CC {} {}", kind.to_uppercase(), ver)
                .trim()
                .to_string();
        }
    }
    url.to_string()
}

#[cfg(test)]
mod tests {
    #[test]
    fn license_names() {
        assert_eq!(
            super::license_label("http://creativecommons.org/licenses/by-nc-sa/2.5/"),
            "CC BY-NC-SA 2.5"
        );
        assert_eq!(
            super::license_label("https://creativecommons.org/publicdomain/zero/1.0/"),
            "CC0"
        );
    }
}
