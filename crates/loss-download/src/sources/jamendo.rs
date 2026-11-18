//! Jamendo: independent artists releasing under Creative Commons.
//! Needs a free API client id from https://devportal.jamendo.com

use super::{license_label, ItemAction, SearchItem, SourceKind};
use crate::http::{as_f64, as_string, as_u32, get_json};
use crate::DownloadRequest;
use loss_core::tags::TrackMeta;
use urlencoding::encode;

pub async fn search(
    client: &reqwest::Client,
    client_id: &str,
    query: &str,
) -> crate::Result<Vec<SearchItem>> {
    if client_id.trim().is_empty() {
        return Err("Add your free Jamendo client id in Settings to search Jamendo.".into());
    }
    let url = format!(
        "https://api.jamendo.com/v3.0/tracks/?client_id={}&format=json&limit=40&search={}&audiodlformat=flac&include=licenses",
        encode(client_id.trim()),
        encode(query)
    );
    let v = get_json(client, &url).await?;
    if let Some(err) = v["headers"]["error_message"].as_str().filter(|s| !s.is_empty()) {
        return Err(format!("Jamendo: {err}").into());
    }
    let results = v["results"].as_array().cloned().unwrap_or_default();
    Ok(results
        .iter()
        .filter_map(|t| {
            if t["audiodownload_allowed"].as_bool() == Some(false) {
                return None;
            }
            let url = as_string(&t["audiodownload"])?;
            let title = as_string(&t["name"])?;
            let artist = as_string(&t["artist_name"]);
            let album = as_string(&t["album_name"]);
            let year = as_string(&t["releasedate"]).and_then(|d| d.get(..4).and_then(|y| y.parse().ok()));
            let meta = TrackMeta {
                title: Some(title.clone()),
                artist: artist.clone(),
                album: album.clone(),
                album_artist: artist.clone(),
                year,
                track_no: as_u32(&t["position"]),
                ..Default::default()
            };
            let ext = if url.contains("format=flac") || url.contains("flac") { "flac" } else { "mp3" };
            let cover = as_string(&t["album_image"]).or_else(|| as_string(&t["image"]));
            Some(SearchItem {
                subtitle: [artist, album].into_iter().flatten().collect::<Vec<_>>().join(" · "),
                cover_url: cover.clone(),
                license: as_string(&t["license_ccurl"]).map(|l| license_label(&l)),
                format: Some(ext.to_uppercase()),
                duration: as_f64(&t["duration"]),
                action: ItemAction::Download(Box::new(DownloadRequest {
                    rel_path: DownloadRequest::build_rel_path(&meta, &title, ext),
                    url,
                    meta,
                    cover_url: cover,
                    source: SourceKind::Jamendo,
                    headers: vec![],
                })),
                title,
            })
        })
        .collect())
}
