//! Podcasts: search the public iTunes podcast directory, then download
//! episodes straight from the podcast's own RSS feed.

use super::{Collection, ItemAction, SearchItem, SourceKind};
use crate::http::{as_f64, as_string, get_bytes, get_json};
use crate::DownloadRequest;
use loss_core::tags::TrackMeta;
use urlencoding::encode;

pub async fn search(client: &reqwest::Client, query: &str) -> crate::Result<Vec<SearchItem>> {
    let url = format!(
        "https://itunes.apple.com/search?media=podcast&entity=podcast&limit=30&term={}",
        encode(query)
    );
    let v = get_json(client, &url).await?;
    let results = v["results"].as_array().cloned().unwrap_or_default();
    Ok(results
        .iter()
        .filter_map(|p| {
            let feed = as_string(&p["feedUrl"])?;
            let title = as_string(&p["collectionName"]).unwrap_or_else(|| feed.clone());
            Some(SearchItem {
                subtitle: as_string(&p["artistName"]).unwrap_or_default(),
                cover_url: as_string(&p["artworkUrl600"]).or_else(|| as_string(&p["artworkUrl100"])),
                license: None,
                format: Some("Podcast".into()),
                duration: None,
                action: ItemAction::Open(Collection {
                    source: SourceKind::Podcast,
                    id: feed,
                    title: title.clone(),
                }),
                title,
            })
        })
        .collect())
}

fn parse_duration(s: &str) -> Option<f64> {
    as_f64(&serde_json::Value::String(s.trim().to_string()))
}

/// Episodes of a feed, newest first.
pub async fn open(client: &reqwest::Client, feed_url: &str) -> crate::Result<Vec<SearchItem>> {
    let bytes = get_bytes(client, feed_url).await?;
    parse_feed(&bytes)
}

pub fn parse_feed(bytes: &[u8]) -> crate::Result<Vec<SearchItem>> {
    let channel = rss::Channel::read_from(bytes).map_err(|e| format!("not a valid podcast feed: {e}"))?;
    let show = channel.title().to_string();
    let author = channel
        .itunes_ext()
        .and_then(|i| i.author())
        .map(str::to_string);
    let show_cover = channel
        .itunes_ext()
        .and_then(|i| i.image())
        .map(str::to_string)
        .or_else(|| channel.image().map(|i| i.url().to_string()));
    Ok(channel
        .items()
        .iter()
        .filter_map(|item| {
            let enc = item.enclosure()?;
            let url = enc.url().to_string();
            let title = item.title().unwrap_or("Episode").to_string();
            let ext = url
                .split(['?', '#'])
                .next()
                .and_then(|u| u.rsplit_once('.'))
                .map(|x| x.1.to_lowercase())
                .filter(|e| e.len() <= 4 && loss_core::AUDIO_EXTENSIONS.contains(&e.as_str()))
                .unwrap_or_else(|| match enc.mime_type() {
                    "audio/mp4" | "audio/x-m4a" => "m4a".into(),
                    "audio/ogg" => "ogg".into(),
                    _ => "mp3".into(),
                });
            let cover = item
                .itunes_ext()
                .and_then(|i| i.image())
                .map(str::to_string)
                .or(show_cover.clone());
            let year = item.pub_date().and_then(|d| {
                d.split_whitespace()
                    .find(|w| w.len() == 4 && w.chars().all(|c| c.is_ascii_digit()))
                    .and_then(|y| y.parse().ok())
            });
            let meta = TrackMeta {
                title: Some(title.clone()),
                artist: author.clone().or(Some(show.clone())),
                album: Some(show.clone()),
                album_artist: Some(show.clone()),
                genre: Some("Podcast".into()),
                year,
                ..Default::default()
            };
            let rel_path = format!(
                "Podcasts/{}/{}.{}",
                loss_core::sanitize_file_name(&show),
                loss_core::sanitize_file_name(&title),
                ext
            );
            Some(SearchItem {
                subtitle: item.pub_date().unwrap_or("").to_string(),
                cover_url: cover.clone(),
                license: None,
                format: Some(ext.to_uppercase()),
                duration: item
                    .itunes_ext()
                    .and_then(|i| i.duration())
                    .and_then(parse_duration),
                action: ItemAction::Download(Box::new(DownloadRequest {
                    url,
                    rel_path,
                    meta,
                    cover_url: cover,
                    source: SourceKind::Podcast,
                    headers: vec![],
                })),
                title,
            })
        })
        .collect())
}

#[cfg(test)]
mod tests {
    #[test]
    fn parses_feed() {
        let xml = r#"<?xml version="1.0"?>
<rss version="2.0" xmlns:itunes="http://www.itunes.com/dtds/podcast-1.0.dtd">
<channel><title>My Show</title><link>https://x</link><description>d</description>
<itunes:author>Host</itunes:author>
<item><title>Ep 1: Hello/World</title><pubDate>Mon, 02 Jun 2025 10:00:00 GMT</pubDate>
<enclosure url="https://cdn.example/ep1.mp3?x=1" length="100" type="audio/mpeg"/>
<itunes:duration>1:02:03</itunes:duration></item>
</channel></rss>"#;
        let items = super::parse_feed(xml.as_bytes()).unwrap();
        assert_eq!(items.len(), 1);
        let req = items[0].request().unwrap();
        assert_eq!(req.rel_path, "Podcasts/My Show/Ep 1_ Hello_World.mp3");
        assert_eq!(req.meta.year, Some(2025));
        assert_eq!(req.meta.artist.as_deref(), Some("Host"));
        assert_eq!(items[0].duration, Some(3723.0));
    }
}
