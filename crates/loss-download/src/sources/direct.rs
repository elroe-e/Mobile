//! Direct links: paste one or more URLs (one per line), e.g. the download
//! link Bandcamp, Qobuz or 7digital give you after a purchase, or a file on
//! your own server.

use super::SourceKind;
use crate::DownloadRequest;
use loss_core::tags::TrackMeta;

/// File name from the last path segment of a URL.
pub fn name_from_url(url: &str) -> String {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    let last = path.trim_end_matches('/').rsplit('/').next().unwrap_or("");
    let decoded = urlencoding::decode(last)
        .map(|c| c.into_owned())
        .unwrap_or_else(|_| last.to_string());
    let name = loss_core::sanitize_file_name(&decoded);
    if name == "untitled" || !last.contains('.') && last.is_empty() {
        "download".into()
    } else {
        name
    }
}

pub fn requests_from_text(text: &str) -> Result<Vec<DownloadRequest>, String> {
    let mut out = Vec::new();
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let lower = line.to_ascii_lowercase();
        if !(lower.starts_with("https://") || lower.starts_with("http://")) {
            return Err(format!("not a web link: {line}"));
        }
        let name = name_from_url(line);
        let title = name.rsplit_once('.').map(|x| x.0.to_string()).unwrap_or(name.clone());
        let (artist, title) = match title.split_once(" - ") {
            Some((a, t)) => (Some(a.trim().to_string()), t.trim().to_string()),
            None => (None, title),
        };
        out.push(DownloadRequest {
            url: line.to_string(),
            rel_path: format!("Direct/{name}"),
            meta: TrackMeta {
                title: Some(title),
                artist,
                ..Default::default()
            },
            cover_url: None,
            source: SourceKind::Direct,
            headers: vec![],
        });
    }
    if out.is_empty() {
        return Err("paste at least one link".into());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_links() {
        let r = requests_from_text("https://x.com/a/My%20Band%20-%20Song.flac?token=1\n\nhttps://y.org/album.zip").unwrap();
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].rel_path, "Direct/My Band - Song.flac");
        assert_eq!(r[0].meta.artist.as_deref(), Some("My Band"));
        assert_eq!(r[1].rel_path, "Direct/album.zip");
        assert!(requests_from_text("ftp://nope").is_err());
    }
}
