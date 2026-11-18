//! Your own Subsonic / OpenSubsonic server (Navidrome, Gonic, Airsonic...).
//! Downloads the original files for offline listening.

use super::{Collection, ItemAction, SearchItem, SourceKind};
use crate::http::{as_f64, as_string, as_u32, get_json};
use crate::DownloadRequest;
use loss_core::tags::TrackMeta;
use loss_core::SubsonicConfig;
use urlencoding::encode;

fn salt() -> String {
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{:x}", md5::compute(n.to_le_bytes()))[..12].to_string()
}

/// Builds an authenticated API URL using a salted token (the password is
/// never sent in clear text).
pub fn api_url(cfg: &SubsonicConfig, method: &str, params: &[(&str, &str)]) -> String {
    let s = salt();
    let token = format!("{:x}", md5::compute(format!("{}{}", cfg.password, s)));
    let mut url = format!(
        "{}/rest/{}?u={}&t={}&s={}&v=1.16.1&c=Loss&f=json",
        cfg.url.trim_end_matches('/'),
        method,
        encode(&cfg.username),
        token,
        s
    );
    for (k, v) in params {
        url.push_str(&format!("&{}={}", k, encode(v)));
    }
    url
}

async fn call(client: &reqwest::Client, cfg: &SubsonicConfig, method: &str, params: &[(&str, &str)]) -> crate::Result<serde_json::Value> {
    let v = get_json(client, &api_url(cfg, method, params)).await?;
    let resp = v["subsonic-response"].clone();
    if resp["status"].as_str() != Some("ok") {
        let msg = resp["error"]["message"].as_str().unwrap_or("unknown error");
        return Err(format!("server: {msg}").into());
    }
    Ok(resp)
}

pub async fn ping(client: &reqwest::Client, cfg: &SubsonicConfig) -> crate::Result<()> {
    call(client, cfg, "ping", &[]).await.map(|_| ())
}

fn song_item(cfg: &SubsonicConfig, s: &serde_json::Value) -> Option<SearchItem> {
    let id = as_string(&s["id"])?;
    let title = as_string(&s["title"]).unwrap_or_else(|| id.clone());
    let suffix = as_string(&s["suffix"]).unwrap_or_else(|| "mp3".into());
    let artist = as_string(&s["artist"]);
    let album = as_string(&s["album"]);
    let meta = TrackMeta {
        title: Some(title.clone()),
        artist: artist.clone(),
        album: album.clone(),
        album_artist: as_string(&s["albumArtist"]).or(artist.clone()),
        genre: as_string(&s["genre"]),
        year: as_u32(&s["year"]),
        track_no: as_u32(&s["track"]),
        disc_no: as_u32(&s["discNumber"]),
        ..Default::default()
    };
    let cover = as_string(&s["coverArt"]).map(|c| api_url(cfg, "getCoverArt", &[("id", &c), ("size", "600")]));
    let bitrate = as_u32(&s["bitRate"]).map(|b| format!(" {b}")).unwrap_or_default();
    Some(SearchItem {
        subtitle: [artist, album].into_iter().flatten().collect::<Vec<_>>().join(" · "),
        cover_url: cover.clone(),
        license: None,
        format: Some(format!("{}{}", suffix.to_uppercase(), bitrate)),
        duration: as_f64(&s["duration"]),
        action: ItemAction::Download(Box::new(DownloadRequest {
            url: api_url(cfg, "download", &[("id", &id)]),
            rel_path: DownloadRequest::build_rel_path(&meta, &title, &suffix),
            meta,
            cover_url: cover,
            source: SourceKind::Subsonic,
            headers: vec![],
        })),
        title,
    })
}

fn album_item(cfg: &SubsonicConfig, a: &serde_json::Value) -> Option<SearchItem> {
    let id = as_string(&a["id"])?;
    let title = as_string(&a["name"]).or_else(|| as_string(&a["title"]))?;
    let count = as_u32(&a["songCount"]).map(|n| format!("{n} songs")).unwrap_or_default();
    Some(SearchItem {
        subtitle: [as_string(&a["artist"]).unwrap_or_default(), count]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" · "),
        cover_url: as_string(&a["coverArt"]).map(|c| api_url(cfg, "getCoverArt", &[("id", &c), ("size", "300")])),
        license: None,
        format: Some("Album".into()),
        duration: as_f64(&a["duration"]),
        action: ItemAction::Open(Collection {
            source: SourceKind::Subsonic,
            id,
            title: title.clone(),
        }),
        title,
    })
}

pub async fn search(client: &reqwest::Client, cfg: &SubsonicConfig, query: &str) -> crate::Result<Vec<SearchItem>> {
    let r = call(
        client,
        cfg,
        "search3",
        &[("query", query), ("albumCount", "20"), ("songCount", "40"), ("artistCount", "0")],
    )
    .await?;
    let res = &r["searchResult3"];
    let mut out: Vec<SearchItem> = res["album"]
        .as_array()
        .map(|a| a.iter().filter_map(|x| album_item(cfg, x)).collect())
        .unwrap_or_default();
    out.extend(
        res["song"]
            .as_array()
            .map(|a| a.iter().filter_map(|x| song_item(cfg, x)).collect::<Vec<_>>())
            .unwrap_or_default(),
    );
    Ok(out)
}

/// Recently added albums, shown before the user searches.
pub async fn recent_albums(client: &reqwest::Client, cfg: &SubsonicConfig) -> crate::Result<Vec<SearchItem>> {
    let r = call(client, cfg, "getAlbumList2", &[("type", "newest"), ("size", "40")]).await?;
    Ok(r["albumList2"]["album"]
        .as_array()
        .map(|a| a.iter().filter_map(|x| album_item(cfg, x)).collect())
        .unwrap_or_default())
}

pub async fn open_album(client: &reqwest::Client, cfg: &SubsonicConfig, id: &str) -> crate::Result<Vec<SearchItem>> {
    let r = call(client, cfg, "getAlbum", &[("id", id)]).await?;
    Ok(r["album"]["song"]
        .as_array()
        .map(|a| a.iter().filter_map(|x| song_item(cfg, x)).collect())
        .unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_has_token_not_password() {
        let cfg = SubsonicConfig {
            url: "https://music.local/".into(),
            username: "me".into(),
            password: "secret".into(),
        };
        let u = api_url(&cfg, "ping", &[("x", "a b")]);
        assert!(u.starts_with("https://music.local/rest/ping?u=me&t="));
        assert!(!u.contains("secret"));
        assert!(u.ends_with("&x=a%20b"));
    }
}
