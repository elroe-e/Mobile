//! Lyrics from LRCLIB (https://lrclib.net), a free, open lyrics database.

use crate::http::as_f64;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq)]
pub struct FoundLyrics {
    /// LRC text with timestamps, when available.
    pub synced: Option<String>,
    pub plain: Option<String>,
}

impl FoundLyrics {
    pub fn best(&self) -> Option<&str> {
        self.synced.as_deref().or(self.plain.as_deref())
    }
}

fn parse(v: &serde_json::Value) -> Option<FoundLyrics> {
    let get = |k: &str| {
        v.get(k)
            .and_then(|x| x.as_str())
            .map(str::to_string)
            .filter(|s| !s.trim().is_empty())
    };
    let found = FoundLyrics {
        synced: get("syncedLyrics"),
        plain: get("plainLyrics"),
    };
    (found.synced.is_some() || found.plain.is_some()).then_some(found)
}

pub async fn fetch(
    client: &reqwest::Client,
    artist: &str,
    title: &str,
    album: Option<&str>,
    duration: Option<f64>,
) -> crate::Result<Option<FoundLyrics>> {
    use urlencoding::encode;
    let mut url = format!(
        "https://lrclib.net/api/get?artist_name={}&track_name={}",
        encode(artist),
        encode(title)
    );
    if let Some(a) = album {
        url.push_str(&format!("&album_name={}", encode(a)));
    }
    if let Some(d) = duration.filter(|d| *d > 0.0) {
        url.push_str(&format!("&duration={}", d.round() as u64));
    }
    let resp = client
        .get(&url)
        .timeout(Duration::from_secs(20))
        .send()
        .await?;
    if resp.status().is_success() {
        let v: serde_json::Value = resp.json().await?;
        if let Some(found) = parse(&v) {
            return Ok(Some(found));
        }
    }
    // Fallback: fuzzy search, pick the closest duration.
    let url = format!(
        "https://lrclib.net/api/search?track_name={}&artist_name={}",
        encode(title),
        encode(artist)
    );
    let resp = client
        .get(&url)
        .timeout(Duration::from_secs(20))
        .send()
        .await?;
    if !resp.status().is_success() {
        return Ok(None);
    }
    let v: serde_json::Value = resp.json().await?;
    let mut results: Vec<&serde_json::Value> = v.as_array().map(|a| a.iter().collect()).unwrap_or_default();
    if let Some(d) = duration.filter(|d| *d > 0.0) {
        results.sort_by(|a, b| {
            let da = a.get("duration").and_then(as_f64).map(|x| (x - d).abs()).unwrap_or(1e9);
            let db = b.get("duration").and_then(as_f64).map(|x| (x - d).abs()).unwrap_or(1e9);
            da.total_cmp(&db)
        });
    }
    Ok(results.into_iter().find_map(parse))
}
